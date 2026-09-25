//! Fixed synthetic controls; this module is reachable only from ignored tests.
use super::listeners::{Counts, Listener};
use super::*;

const CODE: &[u8] = include_bytes!("../../../../../../workers/python/probe/hostile.py");
const SENTINEL: &[u8] = b"fixed assigned synthetic sentinel\n";

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FileAttempt {
    opened: bool,
    read_completed: bool,
    errno: Option<i32>,
}
impl FileAttempt {
    fn denied(&self) -> bool {
        !self.opened
            && !self.read_completed
            && matches!(self.errno, Some(libc::EPERM | libc::EACCES))
    }
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Files {
    sibling_read: FileAttempt,
    original_write_open: FileAttempt,
    prefix_write_open: FileAttempt,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct NetworkAttempt {
    attempted: bool,
    socket_created: bool,
    connected: bool,
    send_accepted: bool,
    received_echo: bool,
    errno: Option<i32>,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Network {
    tcp: NetworkAttempt,
    udp: NetworkAttempt,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct HostileResult {
    schema_version: u32,
    recipe: String,
    job_id: String,
    manifest_sha256: String,
    python_version: String,
    isolated: bool,
    no_site: bool,
    no_bytecode: bool,
    verified_paths: bool,
    assigned_read: bool,
    scratch_roundtrip: bool,
    sentinel_sha256: String,
    files: Files,
    network: Network,
}
fn decode_hostile(raw: &[u8], job_id: &str) -> Result<HostileResult> {
    require(raw.len() <= 16 * 1024, "Hostile result exceeds bound")?;
    let result: HostileResult = serde_json::from_slice(raw)?;
    require(
        result.schema_version == 1
            && result.recipe == "python-hostile-v1"
            && result.job_id == job_id
            && result.manifest_sha256 == MANIFEST
            && result.python_version == "3.13.15"
            && result.isolated
            && result.no_site
            && result.no_bytecode
            && result.verified_paths
            && result.assigned_read
            && result.scratch_roundtrip
            && result.sentinel_sha256 == digest(SENTINEL),
        "Hostile result identity failed",
    )?;
    Ok(result)
}
fn accept_hostile(raw: &[u8], job_id: &str) -> Result<HostileResult> {
    let result = decode_hostile(raw, job_id)?;
    require(
        result.files.sibling_read.denied()
            && result.files.original_write_open.denied()
            && result.files.prefix_write_open.denied(),
        "Hostile file controls failed",
    )?;
    let tcp = &result.network.tcp;
    let udp = &result.network.udp;
    require(
        tcp.attempted
            && !tcp.connected
            && !tcp.send_accepted
            && !tcp.received_echo
            && matches!(tcp.errno, Some(libc::EPERM | libc::EACCES))
            && udp.attempted
            && !udp.connected
            && !udp.received_echo
            && ((udp.socket_created && udp.send_accepted && udp.errno == Some(libc::ETIMEDOUT))
                || (!udp.send_accepted && matches!(udp.errno, Some(libc::EPERM | libc::EACCES)))),
        "Hostile network attempts failed",
    )?;
    Ok(result)
}

fn host_files(sibling: &Path, original: &Path, prefix: &Path) -> Result<()> {
    require(
        read_result(sibling, 64)? == SENTINEL && read_result(original, 64)? == SENTINEL,
        "Host sentinel read failed",
    )?;
    // Exact write-open controls: no truncation, data write or permission repair.
    drop(
        OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(original)?,
    );
    drop(
        OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(prefix.join("install/bin/python3.13"))?,
    );
    Ok(())
}

fn join_listener(
    listener: &mut Option<Listener>,
    observation: &mut serde_json::Value,
    name: &str,
) -> Result<()> {
    if let Some(mut listener) = listener.take() {
        observation["network_observer"][name] = serde_json::to_value(listener.finish()?)?;
    }
    Ok(())
}
fn retain_precedence<T>(preceding: Result<T>, cleanup: Result<()>) -> Result<T> {
    if matches!(preceding, Err(Error::TerminationUnverified(_))) {
        return preceding;
    }
    match cleanup {
        Ok(()) => preceding,
        Err(_) => Err(Error::Cleanup(
            "Host listener shutdown failed; preceding outcome retained in receipt".into(),
        )),
    }
}

fn execute(
    prefix: &Path,
    artifacts: &Path,
    job_id: &str,
    observation: &mut serde_json::Value,
    receipt: &mut File,
) -> Result<HostileResult> {
    let started = Instant::now();
    observation["phase"] = "runtime-inventory".into();
    save(receipt, observation)?;
    verify_prefix(prefix, MANIFEST, 11_320)?;
    observation["runtime_verified"] = true.into();
    let executable = read_result(&prefix.join("install/bin/python3.13"), 80 * 1024 * 1024)?;
    observation["candidate_interpreter"] = serde_json::json!({"path":"install/bin/python3.13","bytes":executable.len(),"sha256":digest(&executable)});
    let sentinels = tempfile::tempdir_in(artifacts)?;
    let outcome = (|| {
        let job = tempfile::tempdir_in(artifacts)?;
        let outcome = (|| {
            let mut tcp = None;
            let mut udp = None;
            let result = (|| {
                for name in ["code", "input", "scratch"] {
                    fs::create_dir(job.path().join(name))?;
                }
                let sibling = sentinels.path().join("sibling.txt");
                let original = sentinels.path().join("original.txt");
                super::super::super::write_new(&sibling, SENTINEL)?;
                super::super::super::write_new(&original, SENTINEL)?;
                host_files(&sibling, &original, prefix)?;
                observation["host_controls"]["before"] = true.into();
                let marker = uuid::Uuid::new_v4().simple().to_string();
                tcp = Some(Listener::start(true, &marker)?);
                udp = Some(Listener::start(false, &marker)?);
                tcp.as_ref().unwrap().positive(false)?;
                udp.as_ref().unwrap().positive(false)?;
                let assignment = serde_json::to_vec(
                    &serde_json::json!({"schema_version":1,"job_id":job_id,"prefix":prefix,
                    "manifest_sha256":MANIFEST,"sibling":sibling,"original":original,
                    "tcp_port":tcp.as_ref().unwrap().port,"udp_port":udp.as_ref().unwrap().port,"marker":marker}),
                )?;
                let mut assigned = BTreeMap::new();
                for (name, bytes) in [
                    ("code/bootstrap.py", CODE),
                    ("input/sentinel.txt", SENTINEL),
                    ("input/assignment.json", assignment.as_slice()),
                ] {
                    require(bytes.len() <= 64 * 1024, "Hostile assignment exceeds bound")?;
                    super::super::super::write_new(&job.path().join(name), bytes)?;
                    assigned.insert(
                        name,
                        serde_json::json!({"bytes":bytes.len(),"sha256":digest(bytes)}),
                    );
                }
                observation["assigned_files"] = serde_json::to_value(&assigned)?;
                let expanded_profile = profile(prefix, job.path())?;
                super::super::super::write_new(
                    &job.path().join("worker.sb"),
                    expanded_profile.as_bytes(),
                )?;
                observation["profile_sha256"] = digest(expanded_profile.as_bytes()).into();
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
                    .arg("-f")
                    .arg(job.path().join("worker.sb"))
                    .arg(prefix.join("install/bin/python3.13"))
                    .args(["-I", "-S", "-B"])
                    .arg(job.path().join("code/bootstrap.py"))
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
                observation["phase"] = "confined-hostile".into();
                observation["preparation_elapsed_ms"] = elapsed_ms(started).into();
                observation["termination_state"] = "unconfirmed".into();
                save(receipt, observation)?;
                let supervised = Instant::now();
                let child = match command.spawn() {
                    Ok(child) => child,
                    Err(error) => {
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
                observation["supervised_elapsed_ms"] = elapsed_ms(supervised).into();
                if let Err(error) = &status {
                    observation["failure"] = failure(error).into();
                    observation["quota_kind"] = serde_json::json!(quota_kind(error));
                }
                if matches!(status, Err(Error::TerminationUnverified(_))) {
                    observation["termination_state"] = "unverified".into();
                    return Err(status.err().unwrap());
                }
                observation["termination_state"] = "confirmed".into();
                for name in ["stdout.txt", "stderr.txt"] {
                    match read_result(&job.path().join("scratch").join(name), DIAGNOSTIC_LIMIT) {
                        Ok(data) => {
                            if super::super::super::write_new(&artifacts.join(name), &data).is_err()
                            {
                                observation["diagnostics_within_bound"] = false.into();
                            }
                        }
                        Err(_) => observation["diagnostics_within_bound"] = false.into(),
                    }
                }
                // Controls and observations are host-owned; collect them even after a confirmed worker failure.
                let controls = host_files(&sibling, &original, prefix)
                    .and_then(|_| tcp.as_ref().unwrap().positive(true))
                    .and_then(|_| udp.as_ref().unwrap().positive(true));
                observation["host_controls"]["after"] = controls.is_ok().into();
                let status = status?;
                observation["exit_code"] = serde_json::json!(status.code());
                controls?;
                require(
                    status.success() && observation["diagnostics_within_bound"] == true,
                    "Hostile process failed",
                )?;
                check_tree(job.path(), 0, &mut 0, &mut 0)?;
                for (name, expected) in assigned {
                    let bytes = read_result(&job.path().join(name), 64 * 1024)?;
                    require(
                        expected
                            == serde_json::json!({"bytes":bytes.len(),"sha256":digest(&bytes)}),
                        "Hostile assignment changed",
                    )?;
                }
                let checkpoint: Checkpoint = serde_json::from_slice(&read_result(
                    &job.path().join("scratch/checkpoint.json"),
                    512,
                )?)?;
                require(
                    checkpoint.phase == "complete",
                    "Hostile checkpoint incomplete",
                )?;
                observation["last_worker_checkpoint"] = "complete".into();
                let raw = read_result(&job.path().join("scratch/result.json"), 16 * 1024)?;
                // Store only identity-checked typed observations, including a safely reported boundary violation.
                observation["result"] = serde_json::to_value(decode_hostile(&raw, job_id)?)?;
                accept_hostile(&raw, job_id)
            })();
            // Always join both independently; preserve possible-live-process precedence.
            let tcp_cleanup = join_listener(&mut tcp, observation, "tcp");
            let udp_cleanup = join_listener(&mut udp, observation, "udp");
            let result = retain_precedence(result, tcp_cleanup.and(udp_cleanup));
            let result = result?;
            for name in ["tcp", "udp"] {
                let counts: Counts =
                    serde_json::from_value(observation["network_observer"][name].clone())?;
                require(
                    counts.qualifies(),
                    "Host loopback delivery observation failed",
                )?;
            }
            Ok(result)
        })();
        finish_job(job, outcome)
    })();
    let outcome = finish_job(sentinels, outcome);
    // Read-only integrity verification is safe only after no process started or confirmed termination.
    if observation["termination_state"] != "unverified" {
        let integrity = verify_prefix(prefix, MANIFEST, 11_320);
        observation["post_runtime_verified"] = integrity.is_ok().into();
        if outcome.is_ok() {
            integrity?;
        }
    }
    outcome
}

#[test]
#[ignore = "requires reviewed hostile Python candidate execution"]
fn native_python_hostile() {
    assert!(cfg!(target_arch = "aarch64"));
    let prefix = PathBuf::from(
        std::env::var_os("WORKBENCH_TEST_PYTHON_PREFIX").expect("Explicit prefix required"),
    );
    let artifacts = PathBuf::from(
        std::env::var_os("WORKBENCH_TEST_PYTHON_ARTIFACTS").expect("Fresh artifacts required"),
    );
    let campaign =
        std::env::var("WORKBENCH_TEST_PYTHON_CAMPAIGN").expect("Campaign identity required");
    assert_eq!(
        uuid::Uuid::parse_str(&campaign).unwrap().to_string(),
        campaign
    );
    reject_linked_ancestors(&artifacts).unwrap();
    let mut receipt = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(artifacts.join("native-report.json"))
        .unwrap();
    let mut observation = initial_observation(&campaign);
    observation["recipe"] = "python-hostile-v1".into();
    observation["host_controls"] = serde_json::json!({"before":false,"after":false});
    observation["network_observer"] = serde_json::json!({"tcp":null,"udp":null});
    observation["post_runtime_verified"] = false.into();
    let job = observation["job_id"].as_str().unwrap().to_owned();
    save(&mut receipt, &observation).unwrap();
    match execute(&prefix, &artifacts, &job, &mut observation, &mut receipt) {
        Ok(result) => {
            observation["result"] = serde_json::to_value(result).unwrap();
            observation["passed"] = true.into();
            observation["phase"] = "complete".into();
        }
        Err(error) => {
            observation["failure"] = failure(&error).into();
        }
    }
    save(&mut receipt, &observation).unwrap();
    assert_eq!(observation["passed"], true, "{observation}");
}

#[test]
fn file_denial_requires_actual_permission_error_not_missing_or_read_failure() {
    for code in [libc::EPERM, libc::EACCES] {
        assert!(FileAttempt {
            opened: false,
            read_completed: false,
            errno: Some(code)
        }
        .denied());
    }
    for code in [libc::ENOENT, libc::EIO, libc::EBADF] {
        assert!(!FileAttempt {
            opened: false,
            read_completed: false,
            errno: Some(code)
        }
        .denied());
    }
    assert!(!FileAttempt {
        opened: true,
        read_completed: false,
        errno: Some(libc::EPERM)
    }
    .denied());
}
#[test]
fn listener_cleanup_never_hides_unverified_termination() {
    let result: Result<()> = retain_precedence(
        Err(Error::TerminationUnverified("fixed".into())),
        Err(Error::Cleanup("fixed".into())),
    );
    assert!(matches!(result, Err(Error::TerminationUnverified(_))));
    let result: Result<()> = retain_precedence(
        Err(Error::Validation("fixed".into())),
        Err(Error::Cleanup("fixed".into())),
    );
    assert!(matches!(result, Err(Error::Cleanup(_))));
}

#[test]
fn hostile_result_is_typed_bounded_and_requires_all_controls() {
    let denied = serde_json::json!({"opened":false,"read_completed":false,"errno":libc::EPERM});
    let network = serde_json::json!({"attempted":true,"socket_created":true,"connected":false,"send_accepted":false,"received_echo":false,"errno":libc::EPERM});
    let value = serde_json::json!({"schema_version":1,"recipe":"python-hostile-v1","job_id":"job","manifest_sha256":MANIFEST,
        "python_version":"3.13.15","isolated":true,"no_site":true,"no_bytecode":true,"verified_paths":true,
        "assigned_read":true,"scratch_roundtrip":true,"sentinel_sha256":digest(SENTINEL),
        "files":{"sibling_read":denied,"original_write_open":denied,"prefix_write_open":denied},"network":{"tcp":network,"udp":network}});
    accept_hostile(&serde_json::to_vec(&value).unwrap(), "job").unwrap();
    assert!(accept_hostile(&serde_json::to_vec(&value).unwrap(), "other-job").is_err());
    let mut changed = value.clone();
    changed["files"]["prefix_write_open"]["opened"] = true.into();
    assert!(decode_hostile(&serde_json::to_vec(&changed).unwrap(), "job").is_ok());
    assert!(accept_hostile(&serde_json::to_vec(&changed).unwrap(), "job").is_err());
    let mut changed = value.clone();
    changed["network"]["tcp"]["received_echo"] = true.into();
    assert!(accept_hostile(&serde_json::to_vec(&changed).unwrap(), "job").is_err());
    let mut changed = value.clone();
    changed["private_path"] = "not-allowed".into();
    assert!(decode_hostile(&serde_json::to_vec(&changed).unwrap(), "job").is_err());
    let duplicate = serde_json::to_string(&value).unwrap().replacen(
        "\"schema_version\":1",
        "\"schema_version\":1,\"schema_version\":1",
        1,
    );
    assert!(decode_hostile(duplicate.as_bytes(), "job").is_err());
    assert!(decode_hostile(&vec![b' '; 16 * 1024 + 1], "job").is_err());
}
