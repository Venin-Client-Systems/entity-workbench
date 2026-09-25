//! Test-only compatibility launcher. No application command or public adapter.
use super::*;
use crate::engines::{ocr::digest, CancellationToken};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Seek, SeekFrom, Write},
    os::unix::fs::PermissionsExt,
    path::{Component, PathBuf},
};

mod canonical_graph;
mod engine_recipes;
mod hostile;
use engine_recipes::Recipe;
mod import_diagnostics;
mod listeners;

const MANIFEST: &str = "4dc6fd171e842d1f9254be7fc5cb16e2e01203896403dcd9839a8aec69dad822";
const BOOTSTRAP: &[u8] = include_bytes!("../../../../../workers/python/probe/bootstrap.py");
const COMPATIBILITY: &[u8] = include_bytes!("../../../../../workers/python/probe/compatibility.py");
const IMPORT_DIAGNOSTICS: &[u8] =
    include_bytes!("../../../../../workers/python/probe/import_diagnostics.py");
const ADAPTER: &[u8] = include_bytes!("../../../../../workers/python/transaction_totals.py");
const FIXTURE: &[u8] = include_bytes!("../../../../../workers/python/probe/fixture.json");
const EXPECTED: &[u8] = include_bytes!("../../../../../workers/python/probe/expected.json");
const RESULT_LIMIT: u64 = 1024 * 1024;
const DIAGNOSTIC_LIMIT: u64 = 128 * 1024;
const IMPORTS: [&str; 6] = ["duckdb", "networkx", "spacy", "click", "splink", "pyarrow"];
const PHASES: [&str; 8] = [
    "bootstrap",
    "versions",
    "imports",
    "mentions",
    "graph",
    "transactions",
    "plugins",
    "complete",
];

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Mention {
    text: String,
    start: u64,
    end: u64,
    review: String,
}
#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Total {
    currency: String,
    net: String,
    transaction_ids: Vec<String>,
}
#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Totals {
    totals: Vec<Total>,
}
#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ReaderValue {
    synthetic: bool,
    reference: String,
}
#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Registry {
    metadata_matched: bool,
    architecture: String,
    layer: String,
    reader: ReaderValue,
}
#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Checks {
    #[serde(deserialize_with = "unique_versions")]
    versions: BTreeMap<String, String>,
    imported_modules: Vec<String>,
    phrase_matches: Vec<Mention>,
    phrase_empty: bool,
    graph_path: Vec<String>,
    graph_assertions: Vec<String>,
    graph_unreachable: bool,
    transaction_totals: Totals,
    transfer_rejected: bool,
    registry: Registry,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WorkerResult<C = Checks> {
    schema_version: u32,
    recipe: String,
    job_id: String,
    manifest_sha256: String,
    python_version: String,
    isolated: bool,
    no_site: bool,
    no_bytecode: bool,
    verified_paths: bool,
    checks: C,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Asset {
    bytes: u64,
    sha256: String,
    executable: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkpoint {
    phase: String,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ImportCheckpoint {
    module: String,
    boundary: String,
}

fn unique_versions<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<BTreeMap<String, String>, D::Error> {
    struct Versions;
    impl<'de> serde::de::Visitor<'de> for Versions {
        type Value = BTreeMap<String, String>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("unique selected version fields")
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            let mut result = BTreeMap::new();
            while let Some((name, version)) = map.next_entry::<String, String>()? {
                if result.len() >= 58 || result.insert(name, version).is_some() {
                    return Err(serde::de::Error::custom(
                        "Duplicate or excess version field",
                    ));
                }
            }
            Ok(result)
        }
    }
    deserializer.deserialize_map(Versions)
}

fn profile(prefix: &Path, job: &Path) -> Result<String> {
    Ok(format!(
        "(version 1)\n(deny default)\n(import \"dyld-support.sb\")\n(deny process-fork)\n(allow sysctl-read)\n(allow file-read-metadata)\n(allow process-exec (literal {}))\n(allow file-read* file-map-executable (subpath {}) (subpath \"/usr/lib\") (subpath \"/System/Library\"))\n(allow file-read* (literal {}) (subpath {}) (subpath {}) (subpath {}) (literal \"/dev/null\") (literal \"/dev/random\") (literal \"/dev/urandom\"))\n(allow file-write* (subpath {}))\n",
        quote(&prefix.join("install/bin/python3.13"))?, quote(prefix)?, quote(job)?,
        quote(&job.join("code"))?, quote(&job.join("input"))?, quote(&job.join("scratch"))?, quote(&job.join("scratch"))?
    ))
}

fn reject_linked_ancestors(path: &Path) -> Result<()> {
    require(path.is_absolute(), "Probe paths must be absolute")?;
    let mut current = PathBuf::new();
    for part in path.components() {
        require(
            matches!(part, Component::RootDir | Component::Normal(_)),
            "Invalid probe path",
        )?;
        current.push(part);
        require(
            !fs::symlink_metadata(&current)?.file_type().is_symlink(),
            "Linked probe ancestor rejected",
        )?;
    }
    Ok(())
}

fn verify_prefix(prefix: &Path, expected_manifest: &str, expected_count: usize) -> Result<()> {
    reject_linked_ancestors(prefix)?;
    let raw = read_result(&prefix.join("manifest.json"), 16 * 1024 * 1024)?;
    require(
        digest(&raw) == expected_manifest,
        "Probe runtime manifest identity mismatch",
    )?;
    let value: serde_json::Value = serde_json::from_slice(&raw)?;
    let files = value
        .get("files")
        .ok_or_else(|| Error::Validation("Probe file inventory missing".into()))?;
    let mut assets: BTreeMap<String, Asset> = serde_json::from_value(files.clone())?;
    require(
        assets.len() + 1 == expected_count && expected_count <= 20_000,
        "Probe runtime file count mismatch",
    )?;
    assets.insert(
        "manifest.json".into(),
        Asset {
            bytes: raw.len() as u64,
            sha256: expected_manifest.into(),
            executable: false,
        },
    );
    let mut actual = BTreeSet::new();
    let mut pending = vec![(prefix.to_owned(), 0usize)];
    let mut entries = 0;
    while let Some((directory, depth)) = pending.pop() {
        require(depth <= 16, "Probe runtime depth exceeded")?;
        for item in fs::read_dir(directory)? {
            let item = item?;
            entries += 1;
            require(entries <= 40_000, "Probe runtime entry count exceeded")?;
            let info = fs::symlink_metadata(item.path())?;
            require(
                !info.file_type().is_symlink(),
                "Linked probe runtime entry rejected",
            )?;
            if info.is_dir() {
                pending.push((item.path(), depth + 1));
            } else {
                require(
                    info.is_file() && info.nlink() == 1,
                    "Special or hardlinked probe runtime entry",
                )?;
                let path = item.path();
                let relative = path
                    .strip_prefix(prefix)
                    .map_err(|_| Error::Validation("Invalid runtime path".into()))?;
                actual.insert(
                    relative
                        .to_str()
                        .ok_or_else(|| Error::Validation("Non-UTF8 runtime path".into()))?
                        .to_owned(),
                );
            }
        }
    }
    require(
        actual == assets.keys().cloned().collect(),
        "Probe runtime has missing or unlisted assets",
    )?;
    let mut total = 0u64;
    for (name, asset) in assets {
        require(
            Path::new(&name)
                .components()
                .all(|part| matches!(part, Component::Normal(_))),
            "Invalid runtime asset path",
        )?;
        total = total
            .checked_add(asset.bytes)
            .ok_or_else(|| Error::Validation("Runtime size overflow".into()))?;
        require(
            asset.bytes <= 80 * 1024 * 1024 && total <= 768 * 1024 * 1024,
            "Probe runtime size exceeded",
        )?;
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(prefix.join(name))?;
        let before = file.metadata()?;
        require(
            before.is_file()
                && before.nlink() == 1
                && before.len() == asset.bytes
                && before.permissions().mode() & 0o7777
                    == if asset.executable { 0o755 } else { 0o644 },
            "Probe runtime asset metadata mismatch",
        )?;
        let mut hash = Sha256::new();
        let mut size = 0u64;
        let mut buffer = [0; 64 * 1024];
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            size += n as u64;
            require(size <= asset.bytes, "Probe runtime grew during hashing")?;
            hash.update(&buffer[..n]);
        }
        let after = file.metadata()?;
        require(
            size == asset.bytes
                && format!("{:x}", hash.finalize()) == asset.sha256
                && before.mtime_nsec() == after.mtime_nsec()
                && before.mtime() == after.mtime()
                && before.ctime_nsec() == after.ctime_nsec()
                && before.ctime() == after.ctime(),
            "Probe runtime integrity mismatch",
        )?;
    }
    Ok(())
}

fn accept(bytes: &[u8], job_id: &str) -> Result<WorkerResult> {
    require(
        bytes.len() as u64 <= RESULT_LIMIT,
        "Probe result exceeds limit",
    )?;
    let result: WorkerResult = serde_json::from_slice(bytes)?;
    let expected: Checks = serde_json::from_slice(EXPECTED)?;
    require(
        result.schema_version == 1
            && result.recipe == "python-compatibility-v1"
            && result.job_id == job_id
            && result.manifest_sha256 == MANIFEST
            && result.python_version == "3.13.15"
            && result.isolated
            && result.no_site
            && result.no_bytecode
            && result.verified_paths
            && result.checks == expected,
        "Probe did not meet the fixed compatibility assertions",
    )?;
    Ok(result)
}

fn checkpoint(job: &Path, phases: &[&str]) -> Option<String> {
    let mut last = None;
    for (index, phase) in phases.iter().enumerate() {
        let bytes = read_result(&job.join(format!("scratch/checkpoint-{index}.json")), 512).ok()?;
        let record: Checkpoint = serde_json::from_slice(&bytes).ok()?;
        if record.phase != *phase {
            return None;
        }
        last = Some(record.phase);
        if !job
            .join(format!("scratch/checkpoint-{}.json", index + 1))
            .exists()
        {
            break;
        }
    }
    last
}

fn quota_kind(error: &Error) -> Option<&'static str> {
    let Error::QuotaExhausted(message) = error else {
        return None;
    };
    Some(match message.as_str() {
        "Local worker wall-time limit exhausted" => "wall-time",
        "Worker output nesting limit exceeded" => "tree-depth",
        "Worker file-count limit exceeded" => "tree-entry-count",
        "Worker disk budget exceeded" => "tree-or-file-bytes",
        "Worker output size overflow" => "tree-size-overflow",
        _ => "other",
    })
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().try_into().unwrap_or(u64::MAX)
}

fn failure(error: &Error) -> &'static str {
    match error {
        Error::TerminationUnverified(_) => "termination-unverified",
        Error::Cleanup(_) => "cleanup-failed",
        Error::QuotaExhausted(_) => "quota-exhausted",
        _ => "compatibility-failed",
    }
}

fn save(file: &mut File, value: &serde_json::Value) -> Result<()> {
    let data = serde_json::to_vec_pretty(value)?;
    require(data.len() <= 1024 * 1024, "Probe observation exceeds limit")?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&data)?;
    file.set_len(data.len() as u64)?;
    file.sync_all()?;
    Ok(())
}

fn run(
    prefix: &Path,
    artifacts: &Path,
    job_id: &str,
    observation: &mut serde_json::Value,
    output: &mut File,
    recipe: Recipe,
) -> Result<serde_json::Value> {
    let preparation_started = Instant::now();
    observation["phase"] = "runtime-inventory".into();
    save(output, observation)?;
    verify_prefix(prefix, MANIFEST, 11_320)?;
    observation["runtime_verified"] = true.into();
    let interpreter = read_result(&prefix.join("install/bin/python3.13"), 80 * 1024 * 1024)?;
    observation["candidate_interpreter"] = serde_json::json!({"path":"install/bin/python3.13","bytes":interpreter.len(),"sha256":digest(&interpreter)});
    let mut canonical = if recipe == Recipe::CanonicalGraph {
        let context = canonical_graph::Context::new(
            artifacts,
            observation["campaign_id"]
                .as_str()
                .ok_or_else(|| Error::Validation("Campaign identity missing".into()))?,
        )?;
        observation["canonical_capture"] = context.capture();
        save(output, observation)?;
        Some(context)
    } else {
        None
    };
    let job = tempfile::tempdir_in(artifacts)?;
    let result = (|| {
        for name in ["code", "input", "scratch"] {
            fs::create_dir(job.path().join(name))?;
        }
        let mut assets: Vec<engine_recipes::AssignedAsset<'_>> = recipe.assets();
        if let Some(context) = canonical.as_ref() {
            assets.push(("input/graph-request.json", context.request(), 64 * 1024));
        }
        for (name, data, maximum) in &assets {
            require(
                data.len() <= *maximum,
                "Compiled probe input exceeds reviewed bound",
            )?;
            super::super::write_new(&job.path().join(name), data)?;
        }
        let mut assignment = serde_json::json!({"schema_version":1,"job_id":job_id,"prefix":prefix,"manifest_sha256":MANIFEST});
        if let Some(context) = canonical.as_ref() {
            context.assignment(&mut assignment);
        }
        let assignment = serde_json::to_vec(&assignment)?;
        super::super::write_new(&job.path().join("input/assignment.json"), &assignment)?;
        let expanded_profile = profile(prefix, job.path())?;
        super::super::write_new(&job.path().join("worker.sb"), expanded_profile.as_bytes())?;
        observation["profile_sha256"] = digest(expanded_profile.as_bytes()).into();
        let mut assigned = BTreeMap::new();
        for name in assets
            .iter()
            .map(|(name, _, _)| *name)
            .chain(["input/assignment.json"])
        {
            let data = read_result(&job.path().join(name), 64 * 1024)?;
            assigned.insert(
                name.to_owned(),
                serde_json::json!({"bytes":data.len(),"sha256":digest(&data)}),
            );
        }
        observation["assigned_files"] = serde_json::to_value(&assigned)?;
        drop(assets);
        let stdout = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(job.path().join("scratch/stdout.txt"))?;
        let stderr = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(job.path().join("scratch/stderr.txt"))?;
        let mut command = Command::new("/usr/bin/sandbox-exec");
        command
            .args(["-f"])
            .arg(job.path().join("worker.sb"))
            .arg(prefix.join("install/bin/python3.13"))
            .args(["-I", "-S", "-B"])
            .arg(job.path().join("code/bootstrap.py"))
            .arg(recipe.identity())
            .current_dir(job.path().join("scratch"))
            .env_clear()
            .envs(std::env::vars_os().filter(|(key, _)| key == "HOME"))
            .env("TMPDIR", job.path().join("scratch"))
            .env("OMP_THREAD_LIMIT", "1")
            .env("OMP_NUM_THREADS", "1")
            .env("OPENBLAS_NUM_THREADS", "1")
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr);
        configure_process(&mut command)?;
        observation["preparation_elapsed_ms"] = elapsed_ms(preparation_started).into();
        observation["phase"] = "confined-compatibility".into();
        observation["termination_state"] = "unconfirmed".into();
        save(output, observation)?;
        let supervised_started = Instant::now();
        let child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                observation["supervised_elapsed_ms"] = elapsed_ms(supervised_started).into();
                observation["termination_state"] = "not-started".into();
                return Err(error.into());
            }
        };
        let status = wait_assigned(
            child,
            job.path(),
            None,
            Duration::from_secs(30),
            Some(&CancellationToken::default()),
        );
        observation["supervised_elapsed_ms"] = elapsed_ms(supervised_started).into();
        if let Err(error) = &status {
            observation["quota_kind"] = serde_json::json!(quota_kind(error));
        }
        // Never inspect/copy worker output unless termination was confirmed.
        let status = match status {
            Err(error @ Error::TerminationUnverified(_)) => {
                observation["termination_state"] = "unverified".into();
                return Err(error);
            }
            other => other,
        };
        observation["termination_state"] = "confirmed".into();
        observation["last_worker_checkpoint"] =
            serde_json::json!(checkpoint(job.path(), recipe.phases()));
        let complete_diagnostics = if recipe == Recipe::Compatibility {
            let imports = import_diagnostics::collect(job.path());
            observation["last_import_checkpoint"] = serde_json::to_value(imports.last())?;
            observation["import_diagnostics"] = serde_json::to_value(&imports)?;
            imports.complete()
        } else {
            let timings = engine_recipes::collect(job.path());
            observation["engine_diagnostics"] = serde_json::to_value(&timings)?;
            timings.complete()
        };
        for name in ["stdout.txt", "stderr.txt"] {
            if let Ok(data) = read_result(&job.path().join("scratch").join(name), DIAGNOSTIC_LIMIT)
            {
                if super::super::write_new(&artifacts.join(name), &data).is_err() {
                    observation["diagnostics_within_bound"] = false.into();
                }
            } else {
                observation["diagnostics_within_bound"] = false.into();
            }
        }
        let status = status?;
        observation["exit_code"] = serde_json::json!(status.code());
        require(
            status.success(),
            "Confined Python compatibility process failed",
        )?;
        require(
            observation["diagnostics_within_bound"] == true,
            "Probe diagnostic output exceeds limit",
        )?;
        check_tree(job.path(), 0, &mut 0, &mut 0)?;
        for (name, expected) in assigned {
            let data = read_result(&job.path().join(name), 64 * 1024)?;
            require(
                expected == serde_json::json!({"bytes":data.len(),"sha256":digest(&data)}),
                "Assigned probe input changed",
            )?;
        }
        require(
            checkpoint(job.path(), recipe.phases()).as_deref() == Some("complete"),
            "Incomplete Python probe checkpoints",
        )?;
        require(
            complete_diagnostics,
            "Incomplete Python diagnostic checkpoints",
        )?;
        let accepted = if let Some(context) = canonical.as_mut() {
            context.read_accept(job.path(), artifacts, job_id, observation)?
        } else {
            recipe.accept(
                &read_result(&job.path().join("scratch/result.json"), RESULT_LIMIT)?,
                job_id,
            )?
        };
        verify_prefix(prefix, MANIFEST, 11_320)?;
        Ok(accepted)
    })();
    finish_job(job, result)
}

fn initial_observation(campaign_id: &str) -> serde_json::Value {
    serde_json::json!({"schema_version":1,"recipe":"python-compatibility-v1","runtime_manifest_sha256":MANIFEST,
        "campaign_id":campaign_id,"job_id":uuid::Uuid::new_v4().to_string(),"architecture":std::env::consts::ARCH,"runtime_verified":false,
        "candidate_interpreter":null,"profile_sha256":null,"assigned_files":{},"termination_state":"not-started",
        "passed":false,"complete_release":false,"phase":"not-started","diagnostics_within_bound":true,
        "last_worker_checkpoint":null,"last_import_checkpoint":null,"import_diagnostics":null,"quota_kind":null,
        "preparation_elapsed_ms":null,"supervised_elapsed_ms":null,"exit_code":null,"failure":null,"result":null})
}

#[test]
fn python_probe_initial_receipt_has_fresh_job_identity_before_any_preparation() {
    let campaign = uuid::Uuid::new_v4().to_string();
    let first = initial_observation(&campaign);
    let second = initial_observation(&campaign);
    assert_eq!(first["campaign_id"], campaign);
    assert_ne!(first["job_id"], second["job_id"]);
    assert!(uuid::Uuid::parse_str(first["job_id"].as_str().unwrap()).is_ok());
    assert_eq!(first["passed"], false);
    assert_eq!(first["phase"], "not-started");
    assert_eq!(first["termination_state"], "not-started");
    assert!(first["candidate_interpreter"].is_null());
    assert!(first["profile_sha256"].is_null());
    assert_eq!(first["assigned_files"], serde_json::json!({}));
}

#[test]
#[ignore = "requires explicit reviewed macOS-arm64 candidate execution"]
fn native_python_compatibility() {
    native_recipe(Recipe::Compatibility);
}

fn native_recipe(recipe: Recipe) {
    assert!(cfg!(target_arch = "aarch64"));
    let prefix = PathBuf::from(
        std::env::var_os("WORKBENCH_TEST_PYTHON_PREFIX").expect("Explicit prefix required"),
    );
    let artifacts = PathBuf::from(
        std::env::var_os("WORKBENCH_TEST_PYTHON_ARTIFACTS").expect("Fresh artifacts required"),
    );
    let campaign_id =
        std::env::var("WORKBENCH_TEST_PYTHON_CAMPAIGN").expect("Campaign identity required");
    assert_eq!(
        uuid::Uuid::parse_str(&campaign_id).unwrap().to_string(),
        campaign_id
    );
    reject_linked_ancestors(&artifacts).unwrap();
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(artifacts.join("native-report.json"))
        .unwrap();
    let mut observation = initial_observation(&campaign_id);
    observation["recipe"] = recipe.identity().into();
    if recipe != Recipe::Compatibility {
        observation["engine_diagnostics"] = serde_json::Value::Null;
    }
    if recipe == Recipe::CanonicalGraph {
        observation["canonical_capture"] = serde_json::Value::Null;
        observation["graph_output_identity"] = serde_json::Value::Null;
        observation["wrapper_output_identity"] = serde_json::Value::Null;
    }
    let job_id = observation["job_id"].as_str().unwrap().to_owned();
    save(&mut output, &observation).unwrap();
    match run(
        &prefix,
        &artifacts,
        &job_id,
        &mut observation,
        &mut output,
        recipe,
    ) {
        Ok(result) => {
            observation["result"] = serde_json::to_value(result).unwrap();
            observation["passed"] = true.into();
            observation["phase"] = "complete".into();
        }
        Err(error) => {
            observation["failure"] = failure(&error).into();
            if observation["quota_kind"].is_null() {
                observation["quota_kind"] = serde_json::json!(quota_kind(&error));
            }
        }
    }
    save(&mut output, &observation).unwrap();
    assert_eq!(observation["passed"], true, "{observation}");
}

#[test]
fn python_probe_profile_has_only_assigned_writes_and_no_network_or_fork() {
    let text = profile(Path::new("/bundle"), Path::new("/job")).unwrap();
    assert!(text.contains("(deny process-fork)"));
    assert!(!text.contains("allow network"));
    assert!(text.contains("(allow file-write* (subpath \"/job/scratch\"))"));
    assert!(!text.contains("(allow file-write* (subpath \"/bundle\"))"));
    assert_eq!(text.matches("allow file-write").count(), 1);
}

#[test]
fn python_probe_result_rejects_missing_altered_duplicate_or_unknown_fields() {
    let checks: serde_json::Value = serde_json::from_slice(EXPECTED).unwrap();
    let value = serde_json::json!({"schema_version":1,"recipe":"python-compatibility-v1","job_id":"job","manifest_sha256":MANIFEST,
        "python_version":"3.13.15","isolated":true,"no_site":true,"no_bytecode":true,"verified_paths":true,"checks":checks});
    accept(&serde_json::to_vec(&value).unwrap(), "job").unwrap();
    for key in ["isolated", "no_site", "no_bytecode", "verified_paths"] {
        let mut bad = value.clone();
        bad[key] = false.into();
        assert!(accept(&serde_json::to_vec(&bad).unwrap(), "job").is_err());
    }
    let mut bad = value.clone();
    bad["private_path"] = "/private/not-allowed".into();
    assert!(accept(&serde_json::to_vec(&bad).unwrap(), "job").is_err());
    let mut bad = value.clone();
    bad["checks"]["transfer_rejected"] = false.into();
    assert!(accept(&serde_json::to_vec(&bad).unwrap(), "job").is_err());
    assert!(accept(&serde_json::to_vec(&value).unwrap(), "another-job").is_err());
    let text = serde_json::to_string(&value).unwrap().replacen(
        "\"schema_version\":1",
        "\"schema_version\":1,\"schema_version\":1",
        1,
    );
    assert!(accept(text.as_bytes(), "job").is_err());
    let text = serde_json::to_string(&value).unwrap().replacen(
        "\"networkx\":\"3.6.1\"",
        "\"networkx\":\"wrong\",\"networkx\":\"3.6.1\"",
        1,
    );
    assert!(accept(text.as_bytes(), "job").is_err());
    assert!(accept(&vec![b' '; RESULT_LIMIT as usize + 1], "job").is_err());
}

#[test]
fn python_probe_runtime_requires_exact_inventory_without_links_or_mutation() {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let prefix = temp.path().join("prefix");
    fs::create_dir_all(prefix.join("install/bin")).unwrap();
    let binary = prefix.join("install/bin/python3.13");
    fs::write(&binary, b"synthetic bytes never executed").unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
    let raw = serde_json::to_vec(&serde_json::json!({"files":{"install/bin/python3.13":{
        "bytes":fs::metadata(&binary).unwrap().len(),"sha256":digest(b"synthetic bytes never executed"),"executable":true
    }}})).unwrap();
    fs::write(prefix.join("manifest.json"), &raw).unwrap();
    fs::set_permissions(
        prefix.join("manifest.json"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    let pin = digest(&raw);
    verify_prefix(&prefix, &pin, 2).unwrap();
    fs::write(prefix.join("extra"), b"unexpected").unwrap();
    assert!(verify_prefix(&prefix, &pin, 2).is_err());
    fs::remove_file(prefix.join("extra")).unwrap();
    let alias = temp.path().join("alias");
    std::os::unix::fs::symlink(&prefix, &alias).unwrap();
    assert!(verify_prefix(&alias, &pin, 2).is_err());
    fs::hard_link(&binary, prefix.join("linked")).unwrap();
    assert!(verify_prefix(&prefix, &pin, 2).is_err());
    fs::remove_file(prefix.join("linked")).unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(verify_prefix(&prefix, &pin, 2).is_err());
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(binary, b"changed").unwrap();
    assert!(verify_prefix(&prefix, &pin, 2).is_err());
}

#[test]
fn python_probe_quota_categories_never_expose_unrecognized_error_text() {
    for (message, expected) in [
        ("Local worker wall-time limit exhausted", "wall-time"),
        ("Worker output nesting limit exceeded", "tree-depth"),
        ("Worker file-count limit exceeded", "tree-entry-count"),
        ("Worker disk budget exceeded", "tree-or-file-bytes"),
        ("Worker output size overflow", "tree-size-overflow"),
        ("unrecognized private diagnostic", "other"),
    ] {
        assert_eq!(
            quota_kind(&Error::QuotaExhausted(message.into())),
            Some(expected)
        );
    }
    assert_eq!(
        quota_kind(&Error::TerminationUnverified("private".into())),
        None
    );
    assert_eq!(quota_kind(&Error::Validation("private".into())), None);
}

#[test]
#[ignore = "explicit trusted Rust prefix hashing measurement; never executes Python"]
fn native_python_prefix_hash_measurement() {
    assert!(
        !cfg!(debug_assertions),
        "Measurement requires recorded release profile"
    );
    let prefix = PathBuf::from(
        std::env::var_os("WORKBENCH_TEST_PYTHON_PREFIX").expect("Explicit prefix required"),
    );
    let artifacts = PathBuf::from(
        std::env::var_os("WORKBENCH_TEST_PYTHON_ARTIFACTS").expect("Fresh artifacts required"),
    );
    let campaign = std::env::var("WORKBENCH_TEST_PYTHON_CAMPAIGN").expect("Campaign required");
    assert_eq!(
        uuid::Uuid::parse_str(&campaign).unwrap().to_string(),
        campaign
    );
    reject_linked_ancestors(&artifacts).unwrap();
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(artifacts.join("native-report.json"))
        .unwrap();
    let mut observation = serde_json::json!({"schema_version":1,"recipe":"trusted-prefix-hash-v1","campaign_id":campaign,
        "measurement_id":uuid::Uuid::new_v4().to_string(),"manifest_sha256":MANIFEST,"passed":false,"complete_release":false,
        "candidate_executed":false,"build_profile":"release","debug_assertions":false,"elapsed_ms":null,"failure":null});
    save(&mut output, &observation).unwrap();
    let started = Instant::now();
    let verified = verify_prefix(&prefix, MANIFEST, 11_320);
    observation["elapsed_ms"] = elapsed_ms(started).into();
    match verified {
        Ok(()) => observation["passed"] = true.into(),
        Err(_) => observation["failure"] = "prefix-verification-failed".into(),
    }
    save(&mut output, &observation).unwrap();
    assert_eq!(observation["passed"], true, "{observation}");
}
