//! One opt-in native campaign. Ordinary tests below never invoke Java.
use super::*;
use crate::{
    domain::{SourceAnchor, SourceExcerpt},
    engines::{search::probe::Guard, search_lifecycle::INTENT, SearchResults},
    require,
    store::hash,
};
use serde_json::json;
use std::{fs, path::PathBuf};

#[path = "coordinator_search_native_inventory.rs"]
mod inventory;
const POLICY: &str = "fixed-coordinator-search-five-recipes-v1";
const PREFIX: &str = "EW_NATIVE_SEARCH_EVENT=";
const RECIPES: &[&str] = &["index", "search", "search", "search", "search"];
const QUERIES: &[&str] = &["cooperative", "Mira AND depot", "(", "riverside"];
const TEXTS: &[(&str, &[u8])] = &[
    (
        "alpha.txt",
        b"Rowan Ellis synthetic cooperative alpha memorandum.\n",
    ),
    (
        "beta.txt",
        b"Rowan Ellis synthetic riverside beta memorandum.\n",
    ),
    ("gamma.txt", b"Mira Chen synthetic gamma depot note.\n"),
];
const PRIOR_INTENT: &[u8] =
    b"host-injected retained execution intent; no unknown process created by this control\n";
struct Context {
    source: String,
    nonce: String,
    runtime_sha: String,
}
impl Context {
    fn event(&self, kind: &str, details: Value) {
        println!(
            "{PREFIX}{}",
            json!({"policy":POLICY,"source":self.source,"nonce":self.nonce,
            "runtime_sha256":self.runtime_sha,"kind":kind,"details":details})
        );
    }
}
fn fixture(root: &Path, runtime: &Path) -> Result<Workspace> {
    fs::create_dir(root)?; // Fresh/no-clobber, deliberately retained on every outcome.
    let mut workspace = Workspace::open(root)?;
    for (name, bytes) in TEXTS {
        workspace.import(name, bytes)?;
    }
    workspace.attach_runtime(Runtime {
        root: runtime.to_owned(),
    });
    Ok(workspace)
}
fn state(c: &JobCoordinator) -> Result<(u64, String)> {
    let w = c
        .shared
        .workspace
        .lock()
        .map_err(|_| Error::Blocked("Probe workspace unavailable".into()))?;
    Ok((
        w.revision()?,
        crate::store::graph_analysis::probe_fixture::canonical_state(&w)?,
    ))
}
fn originals(c: &JobCoordinator, root: &Path, revision: u64) -> Result<Value> {
    originals_with(root, revision, |anchor| {
        Ok(serde_json::from_value(
            c.dispatch(Command::InspectSource { anchor })?,
        )?)
    })
}
fn originals_after_join(c: &JobCoordinator, root: &Path, revision: u64) -> Result<Value> {
    let workspace = c
        .shared
        .workspace
        .lock()
        .map_err(|_| Error::Blocked("Joined probe workspace unavailable".into()))?;
    originals_with(root, revision, |anchor| workspace.inspect_source(&anchor))
}
fn originals_with(
    root: &Path,
    revision: u64,
    mut inspect: impl FnMut(SourceAnchor) -> Result<SourceExcerpt>,
) -> Result<Value> {
    let directory = root.join("originals");
    inventory::ordinary_root(&directory)?;
    let mut names = fs::read_dir(&directory)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<std::io::Result<Vec<_>>>()?;
    names.sort();
    let mut expected = TEXTS
        .iter()
        .map(|(_, bytes)| std::ffi::OsString::from(hash(bytes)))
        .collect::<Vec<_>>();
    expected.sort();
    require(names == expected, "Unexpected retained original set")?;
    let mut identities = Vec::new();
    for (name, bytes) in TEXTS {
        let id = hash(bytes);
        let (_, _, length, digest) = inventory::file_identity(&directory.join(&id), 1024)?;
        require(
            length == bytes.len() as u64 && digest == id,
            "Retained original bytes differ",
        )?;
        let result = inspect(SourceAnchor::Text {
            evidence_id: id.clone(),
            line_start: 1,
            line_end: 1,
        })?;
        require(
            result.evidence_id == id
                && result.workspace_revision == revision
                && !result.truncated
                && result.quote.as_bytes() == *bytes,
            "Original/anchor fixture differs",
        )?;
        identities.push(json!({"name":name,"sha256":id,"bytes":bytes.len(),"anchor":{"line_start":1,"line_end":1}}));
    }
    Ok(json!(identities))
}
fn cache_snapshot(cache: &Path, revision: u64) -> Result<Value> {
    inventory::ordinary_root(cache)?;
    let mut names = fs::read_dir(cache)?
        .map(|e| e.map(|e| e.file_name()))
        .collect::<std::io::Result<Vec<_>>>()?;
    names.sort();
    require(
        names == ["coordinator.lock", "index"].map(std::ffi::OsString::from),
        "Search assignment or intent was not cleaned",
    )?;
    let index = cache.join("index");
    inventory::ordinary_root(&index)?;
    let mut files = Vec::new();
    let mut total = 0;
    for entry in fs::read_dir(&index)? {
        require(files.len() < 128, "Probe index file bound")?;
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| Error::Validation("Invalid index name".into()))?;
        require(
            !name.is_empty()
                && name.len() <= 180
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)),
            "Invalid index filename",
        )?;
        let (dev, ino, size, sha) = inventory::file_identity(&entry.path(), 8 * 1024 * 1024)?;
        total += size;
        require(total <= 24 * 1024 * 1024, "Probe index aggregate bound")?;
        files.push((name, dev, ino, size, sha));
    }
    files.sort();
    require(!files.is_empty(), "Index is empty")?;
    let marker = cache
        .parent()
        .ok_or_else(|| Error::Validation("Missing index parent".into()))?
        .join("lucene-revision.txt");
    let marker_identity = inventory::file_identity(&marker, 32)?;
    require(
        marker_identity.2 == revision.to_string().len() as u64
            && marker_identity.3 == hash(revision.to_string().as_bytes()),
        "Index marker changed",
    )?;
    Ok(json!({"files":files,"marker":marker_identity,"logical_bytes":total}))
}
fn accept_result(value: Value, query_number: usize, revision: u64) -> Result<Value> {
    let result: SearchResults = serde_json::from_value(value)?;
    let expected = match query_number {
        0 => 0,
        1 => 2,
        3 => 1,
        _ => return Err(Error::Validation("Unexpected successful query case".into())),
    };
    require(
        result.workspace_revision == revision.to_string()
            && result.total == 1
            && result.hits.len() == 1
            && result.hits[0].id == hash(TEXTS[expected].1)
            && result.hits[0].name == TEXTS[expected].0
            && result.hits[0].score.is_finite()
            && result.hits[0].score > 0.0,
        "Fixed synthetic Search result differs",
    )?;
    Ok(serde_json::to_value(result)?)
}
fn error_kind(error: &Error) -> &'static str {
    match error {
        Error::TerminationUnverified(_) => "termination_unverified",
        Error::Cleanup(_) => "cleanup_failed",
        Error::Blocked(_) => "blocked",
        Error::Validation(_) => "validation",
        Error::Io(_) => "io",
        Error::Database(_) => "database",
        Error::Json(_) => "json",
        _ => "other",
    }
}
fn completed_query(result: Result<Value>, sequence: usize, revision: u64) -> Result<Value> {
    if sequence == 2 {
        match result {
            Err(Error::Validation(_)) => Ok(json!({"error":"validation"})),
            Err(error) => Err(error),
            Ok(_) => Err(Error::Validation(
                "Malformed fixed query unexpectedly succeeded".into(),
            )),
        }
    } else {
        accept_result(result?, sequence, revision)
    }
}
fn healthy(root: &Path, runtime: &Path, context: &Context) -> Result<Value> {
    let path = root.join("healthy-workspace");
    let workspace = fixture(&path, runtime)?;
    let before = (
        workspace.revision()?,
        crate::store::graph_analysis::probe_fixture::canonical_state(&workspace)?,
    );
    let c = JobCoordinator::start(workspace, 1)?;
    let guard = Guard::install(RECIPES)?;
    let outcome = (|| {
        require(c.shared.ownership.held(), "Healthy owner missing")?;
        let original_before = originals(&c, &path, before.0)?;
        require(
            Workspace::open(&path)?.lock_processing().is_err(),
            "Coordinator did not retain execution ownership",
        )?;
        context.event("healthy_start",json!({"workspace_revision":before.0,"canonical_sha256":before.1,"originals":original_before}));
        let mut initial_index = None;
        let mut outputs = Vec::new();
        for (i, query) in QUERIES.iter().enumerate() {
            context.event(
                "query_before",
                json!({"sequence":i,"query":query,"recipe_entries":guard.entries()}),
            );
            let result = c.dispatch(Command::Search {
                query: (*query).into(),
            });
            // Unknown stop/cleanup and every unexpected failure exit before index/canonical/original reads.
            let output = completed_query(result, i, before.0)?;
            let index = cache_snapshot(&path.join("indexes/lucene"), before.0)?;
            if let Some(initial) = &initial_index {
                require(
                    *initial == index,
                    "Read-only query rebuilt or changed index",
                )?;
            } else {
                initial_index = Some(index.clone());
            }
            require(state(&c)? == before, "Search changed canonical contents")?;
            require(
                originals(&c, &path, before.0)? == original_before,
                "Search changed retained originals",
            )?;
            require(c.shared.ownership.held(), "Confirmed Search lost ownership")?;
            let detail = json!({"sequence":i,"query":query,"result":output,"recipe_entries":guard.entries(),
                "index":index,"canonical_unchanged":true,"originals_unchanged":true,"assignment_cleanup":true});
            context.event("query_after", detail.clone());
            outputs.push(detail);
        }
        require(guard.complete(), "Fixed recipe count/order differs")?;
        Ok(
            json!({"workspace_revision":before.0,"canonical_sha256":before.1,"originals":original_before,
            "queries":outputs,"recipe_entries":guard.entries()}),
        )
    })();
    // Joins coordinator threads even on a typed failure, but never reads worker paths on it.
    let stopped = c.shutdown();
    match outcome {
        Err(error) => return Err(error), // Preserve primary unknown termination above shutdown errors.
        Ok(_) if stopped.is_err() => return Err(stopped.expect_err("failed shutdown")),
        _ => {}
    }
    let mut result = outcome?;
    require(
        !c.shared.ownership.publication_held(),
        "Joined coordinator retained healthy ownership",
    )?;
    let check = Workspace::open(&path)?;
    let owner = check.lock_processing()?;
    drop(owner);
    require(
        check.revision()? == before.0
            && crate::store::graph_analysis::probe_fixture::canonical_state(&check)? == before.1,
        "Shutdown changed canonical data",
    )?;
    require(
        originals_after_join(&c, &path, before.0)? == result["originals"],
        "Shutdown changed originals",
    )?;
    result["shutdown_joined"] = json!(true);
    result["ownership_released"] = json!(true);
    context.event("healthy_complete", result.clone());
    Ok(result)
}
fn retained_intent(root: &Path, runtime: &Path, context: &Context) -> Result<Value> {
    let path = root.join("intent-workspace");
    let workspace = fixture(&path, runtime)?;
    let before = (
        workspace.revision()?,
        crate::store::graph_analysis::probe_fixture::canonical_state(&workspace)?,
    );
    let cache = path.join("indexes/lucene");
    fs::create_dir_all(cache.join("index"))?;
    fs::write(cache.join(INTENT), PRIOR_INTENT)?;
    fs::write(
        cache.join("index/host-sentinel"),
        b"unmodified synthetic derivative",
    )?;
    let intent = inventory::file_identity(&cache.join(INTENT), 1024)?;
    let sentinel = inventory::file_identity(&cache.join("index/host-sentinel"), 1024)?;
    let guard = Guard::install(&[])?;
    let c = JobCoordinator::start(workspace, 1)?;
    require(
        !c.shared.ownership.held(),
        "Retained intent did not quarantine startup",
    )?;
    let result = c.dispatch(Command::Search {
        query: "cooperative".into(),
    });
    require(
        matches!(result, Err(Error::Blocked(_))) && guard.complete(),
        "Intent failed to refuse before recipe entry",
    )?;
    // This control injected only inert host bytes; it did not create an unknown child.
    require(state(&c)? == before, "Refusal changed canonical data")?;
    let original = originals(&c, &path, before.0)?;
    require(
        inventory::file_identity(&cache.join(INTENT), 1024)? == intent
            && inventory::file_identity(&cache.join("index/host-sentinel"), 1024)? == sentinel,
        "Refusal changed retained intent/index",
    )?;
    require(
        c.shutdown().is_err() && c.shared.ownership.publication_held(),
        "Quarantined shutdown must not claim release",
    )?;
    require(
        state(&c)? == before && originals_after_join(&c, &path, before.0)? == original,
        "Quarantined shutdown changed canonical data or originals",
    )?;
    let report = json!({"host_injected":true,"unknown_child_created":false,"search_error":"blocked",
        "recipe_entries":guard.entries(),"canonical_unchanged":true,"canonical_sha256":before.1,
        "workspace_revision":before.0,"originals":original,"intent_identity":intent,"index_sentinel_identity":sentinel,
        "intent_unchanged":true,"index_unchanged":true,"shutdown_joined":true,"shutdown_quarantined":true,"ownership_released":false});
    context.event("intent_complete", report.clone());
    Ok(report)
}
fn campaign(root: &Path, runtime: &Path, context: &Context) -> Result<Value> {
    let (before, files) = inventory::runtime(runtime)?;
    require(
        before == context.runtime_sha,
        "Selected runtime differs from reviewed digest",
    )?;
    context.event("runtime_verified", json!({"sha256":before,"files":files}));
    let healthy = healthy(root, runtime, context)?;
    let refusal = retained_intent(root, runtime, context)?;
    let (after, post_files) = inventory::runtime(runtime)?;
    require(
        before == after && files == post_files,
        "Selected runtime changed",
    )?;
    Ok(
        json!({"passed":true,"healthy":healthy,"retained_intent":refusal,"runtime_unchanged":true,
        "runtime_sha256":after,"recipe_entries":RECIPES,"native_scope":"macos_selected_development_runtime"}),
    )
}

#[test]
#[ignore = "one reviewed native macOS Search campaign; explicit signed-source opt-in required"]
fn native_coordinator_search_campaign() {
    assert_eq!(
        std::env::var("EW_NATIVE_SEARCH_PROOF").as_deref(),
        Ok(POLICY)
    );
    let source = option_env!("EW_NATIVE_SEARCH_BUILD_SOURCE").unwrap_or("");
    assert!(
        source.len() == 40
            && source
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
    assert_eq!(
        std::env::var("EW_NATIVE_SEARCH_SOURCE").as_deref(),
        Ok(source)
    );
    let nonce = std::env::var("EW_NATIVE_SEARCH_NONCE").expect("Campaign nonce");
    assert_eq!(uuid::Uuid::parse_str(&nonce).unwrap().to_string(), nonce);
    let runtime_sha =
        std::env::var("EW_NATIVE_SEARCH_RUNTIME_SHA256").expect("Reviewed runtime pin");
    assert!(
        runtime_sha.len() == 64
            && runtime_sha
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
    let runtime =
        PathBuf::from(std::env::var_os("EW_NATIVE_SEARCH_RUNTIME").expect("Explicit runtime"));
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
        .join("artifacts/native-coordinator-search")
        .join(&nonce);
    inventory::ordinary_root(&root).unwrap();
    let context = Context {
        source: source.into(),
        nonce,
        runtime_sha,
    };
    context.event("start",json!({"recipe_entry_limit":5,"worker_seconds":30,"queries":QUERIES,
        "fixtures":TEXTS.iter().map(|(name,bytes)|json!({"name":name,"sha256":hash(bytes),"bytes":bytes.len()})).collect::<Vec<_>>()}));
    match campaign(&root, &runtime, &context) {
        Ok(report) => context.event("final", report),
        Err(error) => {
            context.event(
                "failed",
                json!({"category":error_kind(&error),"dependent_postreads":false,"retry":false}),
            );
            panic!("Fixed Search campaign failed; retain the initial receipt and raw log");
        }
    }
}

#[test]
fn native_search_result_contract_rejects_substituted_ids_revision_and_false_empty() {
    let valid = json!({"workspace_revision":"3","total":1,"hits":[{"id":hash(TEXTS[0].1),"name":"alpha.txt","score":1.0}]});
    assert!(accept_result(valid.clone(), 0, 3).is_ok());
    for (key, value) in [
        ("workspace_revision", json!("4")),
        ("total", json!(0)),
        ("hits", json!([])),
        ("extra", json!(true)),
    ] {
        let mut changed = valid.clone();
        changed[key] = value;
        assert!(accept_result(changed, 0, 3).is_err());
    }
    assert!(accept_result(valid, 1, 3).is_err());
}

#[test]
fn native_search_fixture_and_inert_intent_control_need_no_worker() {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let context = Context {
        source: "source-only".into(),
        nonce: uuid::Uuid::new_v4().to_string(),
        runtime_sha: "0".repeat(64),
    };
    let report = retained_intent(temp.path(), &temp.path().join("unavailable"), &context).unwrap();
    assert_eq!(report["recipe_entries"], json!([]));
    assert_eq!(report["ownership_released"], false);
    assert_eq!(report["canonical_unchanged"], true);
}

#[test]
fn native_search_unknown_and_cleanup_errors_precede_all_query_postreads() {
    for sequence in 0..4 {
        for error in [
            Error::TerminationUnverified("retained primary".into()),
            Error::Cleanup("retained cleanup".into()),
            Error::Io(std::io::Error::other("synthetic")),
        ] {
            let expected = error_kind(&error);
            let mut inspected = false;
            let result = completed_query(Err(error), sequence, 3).map(|_| {
                inspected = true;
            });
            assert!(!inspected);
            assert_eq!(error_kind(&result.unwrap_err()), expected);
        }
    }
    assert!(completed_query(Err(Error::Validation("known malformed query".into())), 2, 3).is_ok());
    assert!(completed_query(
        Err(Error::Validation("unexpected earlier failure".into())),
        0,
        3
    )
    .is_err());
}

#[test]
fn native_search_index_snapshot_rejects_residue_and_detects_same_byte_replacement() {
    let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let cache = root.path().join("lucene");
    fs::create_dir_all(cache.join("index")).unwrap();
    fs::write(cache.join("coordinator.lock"), b"").unwrap();
    fs::write(root.path().join("lucene-revision.txt"), b"3").unwrap();
    fs::write(cache.join("index/segments_1"), b"synthetic").unwrap();
    let before = cache_snapshot(&cache, 3).unwrap();
    fs::write(cache.join(INTENT), b"retained").unwrap();
    assert!(cache_snapshot(&cache, 3).is_err());
    fs::remove_file(cache.join(INTENT)).unwrap();
    // Keep the old inode alive so this tests identity, without relying on allocator reuse.
    fs::rename(
        cache.join("index/segments_1"),
        root.path().join("old-segments"),
    )
    .unwrap();
    fs::write(cache.join("index/segments_1"), b"synthetic").unwrap();
    assert_ne!(before, cache_snapshot(&cache, 3).unwrap());
    std::os::unix::fs::symlink(root.path().join("old-segments"), cache.join("index/link")).unwrap();
    assert!(cache_snapshot(&cache, 3).is_err());
}
