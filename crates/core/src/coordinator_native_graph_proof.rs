//! Explicit ignored native campaign. No normal application configuration or public command.
use super::*;
use crate::{
    engines::python_graph::{TestObservation, VerifiedGraphRuntime},
    graph_jobs::{GraphOriginalIntegrity, GraphOutcome},
    processing::{ProcessingJob, ProcessingState},
    store::graph_analysis::{
        coordinator_fixture::{self as fixture},
        probe_fixture,
    },
};
use serde_json::json;
use std::{fs, io::Write, mem::ManuallyDrop, sync::atomic::AtomicUsize, time::Instant};

const CASES: [&str; 4] = [
    "ordinary_retry",
    "unreachable",
    "large_chain",
    "live_cancel",
];
fn check(ok: bool, message: &str) -> Result<()> {
    crate::require(ok, message)
}
fn write(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    check(
        bytes.len() <= 64 * 1024 * 1024,
        "Native proof retained JSON exceeds bound",
    )?;
    raw(path, &bytes)
}
fn raw(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
fn wait(
    c: &JobCoordinator,
    observation: &TestObservation,
    ready: impl Fn(&ProcessingActivity) -> bool,
) -> Result<()> {
    let until = Instant::now() + Duration::from_secs(180);
    let mut activity = c.shared.active.lock().unwrap();
    while !ready(&activity) {
        check(
            observation.receipt()["termination"] != "unverified",
            "Native proof termination unverified; no post reads",
        )?;
        let remaining = until
            .checked_duration_since(Instant::now())
            .ok_or_else(|| Error::Validation("Native proof coordinator deadline".into()))?;
        activity = c
            .shared
            .graph_wake
            .wait_timeout(activity, remaining.min(Duration::from_millis(50)))
            .unwrap()
            .0;
    }
    Ok(())
}
fn job(c: &JobCoordinator, key: &str) -> Result<ProcessingJob> {
    c.shared.workspace.lock().unwrap().processing_job(key)
}
fn stopped(observation: &TestObservation) -> Result<()> {
    let value = observation.receipt();
    check(
        value["execution_calls"] == 1
            && value["launch_count"] == 1
            && value["termination"] == "confirmed"
            && value["cleanup"] == "confirmed",
        "Native proof lacks single confirmed execution and cleanup",
    )
}
fn refused_collection() -> crate::collection_transport::Observation {
    use crate::collection_transport::*;
    Observation {
        outcome: Outcome::Stopped {
            reason: StopReason::Policy,
            head: None,
        },
        phase: Phase::BeforeRequest,
        elapsed_milliseconds: 0,
        observed_wall_ms: chrono::Utc::now().timestamp_millis(),
        resolved: None,
        resolver_uncertainty: None,
        stop_observed: None,
        locally_quiescent: true,
    }
}
fn prepare(root: &Path, case: &str) -> Result<(Workspace, String, String, String)> {
    check(CASES.contains(&case), "Unknown native graph case")?;
    let (w, evidence) = if case == "large_chain" {
        fixture::seed_chain(&root.join("workspace"))?
    } else {
        probe_fixture::seed_case(&root.join("workspace"))?
    };
    let (source, target) = if case == "large_chain" {
        (fixture::chain_id(0), fixture::chain_id(699))
    } else {
        (
            "a".into(),
            if case == "unreachable" { "f" } else { "c" }.into(),
        )
    };
    Ok((w, evidence, source, target))
}
fn run_case(
    root: &Path,
    engines: &Path,
    case: &str,
    observation: Arc<TestObservation>,
) -> Result<Value> {
    let runtime = VerifiedGraphRuntime::from_app_engines(engines, &CancellationToken::default())?
        .observe(observation.clone());
    let (mut w, evidence, source, target) = prepare(root, case)?;
    let r = w.revision()?;
    let key = uuid::Uuid::new_v4().to_string();
    let queued = w.queue_graph_path(r, &source, &target, &key)?;
    let mut mutable = vec![("processing_job".into(), queued.id.clone())];
    let mut document = None;
    let mut collection_job = None;
    let documents = Arc::new(AtomicUsize::new(0));
    let collections = Arc::new(AtomicUsize::new(0));
    let d = documents.clone();
    let x = collections.clone();
    let doc_executor: Arc<Executor> = Arc::new(move |_, _, _, _, _| {
        d.fetch_add(1, Ordering::SeqCst);
        Err(Error::Blocked(
            "Synthetic competing document; no native execution".into(),
        ))
    });
    let collection_executor: Arc<collection::CollectionExecutor> =
        Arc::new(move |_, _, _, _, _| {
            x.fetch_add(1, Ordering::SeqCst);
            refused_collection()
        });
    if case == "ordinary_retry" {
        let q = w.queue_document_parse(&evidence, &uuid::Uuid::new_v4().to_string())?;
        mutable.push(("processing_job".into(), q.id.clone()));
        document = Some(q.id);
        let q = w.queue_collection_protocol(
            crate::collection_jobs::CollectionInput {
                urls: vec!["https://collection.invalid/start".into()],
                max_hops: 0,
                max_requests: 2,
                max_seconds: 60,
            },
            &uuid::Uuid::new_v4().to_string(),
            chrono::Utc::now().timestamp_millis(),
            crate::collection_jobs::CollectionProtocol::SyntheticV4,
        )?;
        mutable.push(("collection_run".into(), q.id.clone()));
        collection_job = Some(q.id);
    }
    let before = fixture::snapshot(&w)?;
    write(&root.join("canonical-before.json"), &before)?;
    let db = root.join("workspace/workspace.db");
    if case == "ordinary_retry" {
        rusqlite::Connection::open(&db)?.execute_batch("CREATE TRIGGER reject_graph_finish BEFORE UPDATE ON records WHEN OLD.kind='processing_job' AND json_extract(OLD.body,'$.input.operation')='shortest_connection_path' AND json_extract(OLD.body,'$.state')='running' AND json_extract(NEW.body,'$.state')!='running' BEGIN SELECT RAISE(ABORT,'synthetic publication refusal'); END;")?;
    }
    // Persistent case directory plus ManuallyDrop: an uncertain child must not
    // trigger implicit shutdown, workspace cleanup, or a post-canonical read.
    let c = ManuallyDrop::new(JobCoordinator::with_graph_execution(
        w,
        2,
        doc_executor,
        collection_job.as_ref().map(|_| {
            (
                collection_executor,
                crate::collection_jobs::CollectionProtocol::SyntheticV4,
            )
        }),
        GraphExecution::Configured(runtime),
    )?);
    let outcome = (|| {
        let mut exclusive = Value::Null;
        if case == "live_cancel" {
            observation.wait_live(Duration::from_secs(180))?;
            c.dispatch(Command::CancelProcessingJob {
                job_id: queued.id.clone(),
                expected_attempt: queued.attempt,
            })?;
        }
        if case == "ordinary_retry" {
            wait(&c, &observation, |a| {
                a.status()
                    .is_some_and(|s| s.phase == GraphPhase::PublicationPending)
            })?;
            stopped(&observation)?;
            let state = c
                .graph_status()?
                .ok_or_else(|| Error::Validation("Pending graph status missing".into()))?;
            let w = c.shared.workspace.lock().unwrap();
            check(
                w.processing_job(document.as_ref().unwrap())?.state == ProcessingState::Queued
                    && w.inspect_durable_collection(collection_job.as_ref().unwrap())?
                        .checkpoint
                        .state
                        == crate::collection_jobs::CollectionState::Queued,
                "Competing job claimed inside exclusive graph interval",
            )?;
            check(
                documents.load(Ordering::SeqCst) == 0 && collections.load(Ordering::SeqCst) == 0,
                "Competing executor entered inside exclusive graph interval",
            )?;
            let running = w.processing_job(&queued.id)?;
            let pending_revision = w.revision()?;
            drop(w);
            let retained_before = observation.receipt();
            exclusive = json!({"pending_revision":pending_revision,"job":running,"request_sha256":state.request_sha256,"document_job":document,"collection_job":collection_job,"document_calls":0,"collection_calls":0,"publication_attempts_before_retry":c.shared.active.lock().unwrap().publication_attempts});
            rusqlite::Connection::open(&db)?.execute_batch("DROP TRIGGER reject_graph_finish")?;
            c.retry_graph_publication(
                &state.job_id,
                state.attempt,
                state.lease.as_deref().unwrap(),
                state.request_sha256.as_deref().unwrap(),
            )?;
            wait(&c, &observation, |a| a.status().is_none())?;
            check(
                observation.receipt() == retained_before,
                "Publication retry invoked or changed native assignment",
            )?;
        } else {
            wait(&c, &observation, |a| {
                a.publication_attempts >= 1 && a.status().is_none()
            })?;
        }
        stopped(&observation)?;
        let finished = job(&c, &queued.id)?;
        check(
            finished.state
                == if case == "live_cancel" {
                    ProcessingState::Cancelled
                } else {
                    ProcessingState::Completed
                },
            "Native graph terminal state differs",
        )?;
        let mut competitors = Value::Null;
        if case == "ordinary_retry" {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let w = c.shared.workspace.lock().unwrap();
                let document = w.processing_job(document.as_ref().unwrap())?;
                let collection = w.inspect_durable_collection(collection_job.as_ref().unwrap())?;
                if document.state == ProcessingState::Blocked
                    && collection.checkpoint.state
                        == crate::collection_jobs::CollectionState::Blocked
                {
                    competitors = json!({"document":document,"collection":collection});
                    break;
                }
                drop(w);
                check(Instant::now()<deadline,"Synthetic competitors did not reach expected terminal states after graph publication")?;
                thread::sleep(Duration::from_millis(20));
            }
            check(
                documents.load(Ordering::SeqCst) == 1 && collections.load(Ordering::SeqCst) == 1,
                "Synthetic competing execution repeated",
            )?;
        }
        // Join all automatic writers before snapshots; no graph child remains.
        c.shutdown()?;
        let observation_value = observation.receipt();
        let [request, wrapper, result] = observation.retained()?;
        raw(&root.join("request.json"), &request)?;
        raw(&root.join("wrapper.json"), &wrapper)?;
        raw(&root.join("result.json"), &result)?;
        let mut w = c.shared.workspace.lock().unwrap();
        let record = if case == "live_cancel" {
            check(
                finished.result_ids.is_empty() && wrapper.is_empty() && result.is_empty(),
                "Cancelled graph retained a result",
            )?;
            None
        } else {
            check(
                finished.result_ids.len() == 1,
                "Completed graph result count differs",
            )?;
            let inspected = w.inspect_graph_analysis(&finished.result_ids[0])?;
            check(
                inspected.original_integrity == GraphOriginalIntegrity::Verified,
                "Frozen graph originals unavailable",
            )?;
            let record = inspected.record;
            check(
                record.request_json.as_bytes() == request
                    && record.result_json.as_bytes() == result
                    && record.requested_revision == r
                    && record.queued_revision == r + 1
                    && record.published_revision == record.captured_revision + 1,
                "Frozen graph raw bytes or revisions differ",
            )?;
            match (&record.outcome, case) {
                (GraphOutcome::Unreachable {}, "unreachable") => {}
                (GraphOutcome::Path { nodes, hops }, "ordinary_retry") => check(
                    nodes == &["a", "b", "c"]
                        && hops.len() == 2
                        && hops[0].assertion_ids == ["r1", "r1-parallel"]
                        && hops[1].assertion_ids == ["r2"],
                    "Ordinary graph path/provenance differs",
                )?,
                (GraphOutcome::Path { nodes, hops }, "large_chain") => check(
                    nodes == &(0..700).map(fixture::chain_id).collect::<Vec<_>>()
                        && hops.len() == 699
                        && request.len() > 64 * 1024
                        && request.len() <= 1024 * 1024,
                    "Genuine large graph capture differs",
                )?,
                _ => return Err(Error::Validation("Unexpected frozen graph outcome".into())),
            }
            write(&root.join("record.json"), &record)?;
            Some(record)
        };
        let after = fixture::snapshot(&w)?;
        let delta = fixture::verify_delta(
            &before,
            &after,
            &mutable,
            record.as_ref().map(|r| r.id.as_str()),
        )?;
        write(&root.join("canonical-after.json"), &after)?;
        let replay = w.queue_graph_path(r, &source, &target, &key)?;
        check(
            serde_json::to_value(&replay)? == serde_json::to_value(&finished)?
                && fixture::snapshot(&w)?.identity()? == after.identity()?,
            "Completed delivery replay changed canonical state",
        )?;
        check(
            fs::read_dir(w.processing_scratch())?.next().is_none(),
            "Native graph scratch not empty after cleanup",
        )?;
        drop(w);
        Ok(
            json!({"case":case,"requested_revision":r,"queued":queued,"finished":finished,"record_id":record.as_ref().map(|r|&r.id),"exclusive":exclusive,"competitors":competitors,"observation":observation_value,"delta":delta,"replay_unchanged":true,"scratch_empty":true,"synthetic_document_calls":documents.load(Ordering::SeqCst),"synthetic_collection_calls":collections.load(Ordering::SeqCst)}),
        )
    })();
    // Unknown or incomplete supervision must preserve ownership and every path.
    // Only an already confirmed/cleaned assignment can take normal shutdown.
    if stopped(&observation).is_ok() {
        let shutdown = c.shutdown();
        drop(ManuallyDrop::into_inner(c));
        shutdown?;
    } else {
        return outcome;
    }
    let value = outcome?;
    let mut reopened = Workspace::open(root.join("workspace"))?;
    let after = fixture::snapshot(&reopened)?;
    check(
        after.identity()? == value["delta"]["after"],
        "Reopening changed canonical state",
    )?;
    let replay = reopened.queue_graph_path(r, &source, &target, &key)?;
    check(
        serde_json::to_value(&replay)? == value["finished"],
        "Reopened graph delivery differs",
    )?;
    if let Some(record) = value["record_id"].as_str() {
        let retained = reopened.inspect_graph_analysis(record)?;
        check(
            retained.original_integrity == GraphOriginalIntegrity::Verified,
            "Reopened graph originals unavailable",
        )?;
        check(
            serde_json::to_vec(&retained.record)?
                == serde_json::to_vec(&serde_json::from_slice::<
                    crate::graph_jobs::GraphAnalysisRecord,
                >(&fs::read(root.join("record.json"))?)?)?,
            "Reopened immutable graph differs",
        )?;
    }
    check(
        fixture::snapshot(&reopened)?.identity()? == after.identity()?,
        "Reopened replay wrote canonical state",
    )?;
    let mut value = value;
    value["reopen_unchanged"] = json!(true);
    Ok(value)
}

#[test]
#[ignore = "explicit reviewed four-case native graph coordinator campaign only"]
fn native_graph_coordinator_campaign() {
    let run = (|| -> Result<()> {
        let root = std::env::var_os("WORKBENCH_TEST_GRAPH_ARTIFACTS")
            .map(std::path::PathBuf::from)
            .ok_or_else(|| Error::Validation("Explicit campaign artifacts required".into()))?;
        let engines = std::env::var_os("WORKBENCH_TEST_GRAPH_ENGINES")
            .map(std::path::PathBuf::from)
            .ok_or_else(|| Error::Validation("Explicit verified app resources required".into()))?;
        let campaign = std::env::var("WORKBENCH_TEST_GRAPH_CAMPAIGN")
            .map_err(|_| Error::Validation("Explicit campaign UUID required".into()))?;
        check(
            uuid::Uuid::parse_str(&campaign).is_ok_and(|u| u.to_string() == campaign),
            "Canonical campaign UUID required",
        )?;
        write(
            &root.join("native-start.json"),
            &json!({"schema_version":1,"campaign_id":campaign,"cases":CASES,"complete_release":false}),
        )?;
        for case in CASES {
            let path = root.join(case);
            fs::create_dir(&path)?;
            let observation = Arc::new(TestObservation::new(case == "live_cancel"));
            let result = run_case(&path, &engines, case, observation.clone());
            let receipt = json!({"schema_version":1,"campaign_id":campaign,"case":case,"passed":result.is_ok(),"failure":result.as_ref().err().map(ToString::to_string),"observation":observation.receipt(),"result":result.as_ref().ok(),"complete_release":false});
            write(&path.join("receipt.json"), &receipt)?;
            result?;
        }
        write(
            &root.join("native-complete.json"),
            &json!({"schema_version":1,"campaign_id":campaign,"passed":true,"complete_release":false}),
        )?;
        Ok(())
    })();
    assert!(run.is_ok(), "{run:?}");
}
