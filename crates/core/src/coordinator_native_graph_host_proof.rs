//! Explicit single-case native host/API proof. Never discovered or run at startup.
use super::*;
use crate::{
    engines::python_graph::TestObservation,
    graph_api::{GraphAvailability, GraphJobInspection, GraphJobPage, GraphJobPageRequest},
    graph_jobs::{GraphAnalysisInspection, GraphFreshness, GraphOriginalIntegrity, GraphOutcome},
    processing::ProcessingState,
    store::graph_analysis::{coordinator_fixture as fixture, probe_fixture},
};
use serde_json::json;
use std::{fs, io::Write, mem::ManuallyDrop, time::Instant};

fn check(ok: bool, message: &str) -> Result<()> {
    crate::require(ok, message)
}
fn raw(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    check(bytes.len() <= 64 * 1024 * 1024, "Host proof artifact bound")?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
fn write(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    raw(path, &serde_json::to_vec_pretty(value)?)
}
fn stopped(observation: &TestObservation) -> bool {
    let v = observation.receipt();
    v["execution_calls"] == 1
        && v["launch_count"] == 1
        && v["termination"] == "confirmed"
        && v["cleanup"] == "confirmed"
}
fn wait(c: &JobCoordinator, observation: &TestObservation) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(180);
    let mut active = c.shared.active.lock().unwrap();
    loop {
        let receipt = observation.receipt();
        check(
            receipt["termination"] != "unverified" && receipt["cleanup"] != "failed",
            "Host proof supervision unverified; no post reads",
        )?;
        if stopped(observation) && active.status().is_none() {
            return Ok(());
        }
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| Error::Validation("Host proof coordinator deadline; no retry".into()))?;
        active = c
            .shared
            .graph_wake
            .wait_timeout(active, remaining.min(Duration::from_millis(50)))
            .unwrap()
            .0;
    }
}
fn queue(r: u64, key: &str) -> Command {
    Command::QueueGraphPath {
        expected_revision: r,
        source_id: "a".into(),
        target_id: "c".into(),
        request_key: key.into(),
    }
}
fn inspect(reference: &crate::graph_api::GraphResultReference) -> Command {
    Command::InspectGraphAnalysis {
        id: reference.id.clone(),
        expected_request_sha256: reference.request_sha256.clone(),
        expected_result_sha256: reference.result_sha256.clone(),
    }
}
fn same(a: &impl serde::Serialize, b: &impl serde::Serialize) -> Result<bool> {
    Ok(serde_json::to_value(a)? == serde_json::to_value(b)?)
}

/// The public queue runs after startup. Compare the seeded state, not an invented
/// queued snapshot: exactly request mapping + job + result may be added.
fn delta(
    before: &fixture::Snapshot,
    after: &fixture::Snapshot,
    queued: &GraphJobInspection,
    finished: &GraphJobInspection,
    result: &GraphAnalysisInspection,
) -> Result<Value> {
    let b = &before.tables;
    let a = &after.tables;
    check(
        before.originals == after.originals,
        "Host proof original mutation",
    )?;
    for table in ["schema", "version", "derivative_objects"] {
        check(b[table] == a[table], "Host proof static table mutation")?;
    }
    check(
        b["meta"] == vec![vec![json!(1), json!(2)]] && a["meta"] == vec![vec![json!(1), json!(5)]],
        "Host proof R/P",
    )?;
    let old = &b["records"];
    let new = &a["records"];
    check(
        new.starts_with(old) && new.len() == old.len() + 3,
        "Host proof existing or excess canonical rows",
    )?;
    let inserted = &new[old.len()..];
    for (row, kind, key, body) in [
        (
            &inserted[0],
            "processing_request",
            queued.job.request_key.as_str(),
            json!(queued.job.id),
        ),
        (
            &inserted[1],
            "processing_job",
            queued.job.id.as_str(),
            serde_json::to_value(&finished.job)?,
        ),
        (
            &inserted[2],
            "graph_analysis",
            result.record.id.as_str(),
            serde_json::to_value(&result.record)?,
        ),
    ] {
        check(
            row[1] == kind
                && row[2] == key
                && serde_json::from_str::<Value>(row[3].as_str().unwrap_or(""))? == body,
            "Host proof detached canonical body",
        )?;
    }
    check(
        a["history"].starts_with(&b["history"]) && a["history"].len() == b["history"].len() + 2,
        "Host proof unexpected history",
    )?;
    let history = &a["history"][b["history"].len()..];
    check(
        history[0][1] == "processing_job"
            && history[0][2] == queued.job.id
            && history[0][4] == 4
            && history[1][1..3] == history[0][1..3]
            && history[1][4] == 5,
        "Host proof history identity",
    )?;
    check(
        serde_json::from_str::<Value>(history[0][3].as_str().unwrap_or(""))?
            == serde_json::to_value(&queued.job)?,
        "Host proof queue acknowledgement differs from history",
    )?;
    let running: crate::processing::ProcessingJob =
        serde_json::from_str(history[1][3].as_str().unwrap_or(""))?;
    check(
        running.state == ProcessingState::Running
            && running.id == finished.job.id
            && running.request_key == finished.job.request_key
            && running.lease == finished.job.lease
            && same(&running.input, &finished.job.input)?
            && running.attempt == 1,
        "Host proof running claim binding",
    )?;
    check(
        a["events"].starts_with(&b["events"]) && a["events"].len() == b["events"].len() + 3,
        "Host proof unexpected events",
    )?;
    let events = &a["events"][b["events"].len()..];
    for (i, action) in [
        "processing.graph_queue",
        "processing.graph_claim",
        "processing.graph_finish",
    ]
    .iter()
    .enumerate()
    {
        check(
            events[i][1] == i + 3 && events[i][2] == *action,
            "Host proof event revision/action",
        )?;
    }
    for (table, increment) in [("records", 5), ("events", 3), ("history", 2)] {
        let prior = b["sqlite_sequence"]
            .iter()
            .find(|row| row[0] == table)
            .map_or(0, |row| row[1].as_u64().unwrap());
        let actual = a["sqlite_sequence"]
            .iter()
            .find(|row| row[0] == table)
            .and_then(|row| row[1].as_u64());
        check(
            actual == Some(prior + increment),
            "Host proof sequence mutation",
        )?;
    }
    check(
        a["sqlite_sequence"].len() == 3,
        "Host proof unexpected sequence table",
    )?;
    Ok(
        json!({"before":before.identity()?,"after":after.identity()?,"all_tables_checked":true,"new_events":events}),
    )
}

fn run(root: &Path, observation: Arc<TestObservation>) -> Result<Value> {
    let resources = root.join("resources");
    let (w, _) = probe_fixture::seed_case(&root.join("workspace"))?;
    let before = fixture::snapshot(&w)?;
    write(&root.join("canonical-before.json"), &before)?;
    check(w.revision()? == 2, "Host proof fixture revision")?;
    // The actual public host constructor creates and verifies its capability.
    // No executor/capability injection; no graph is queued before observation.
    let c = ManuallyDrop::new(JobCoordinator::start_with_development_app_resources(
        w,
        1,
        &resources,
        &CancellationToken::default(),
    )?);
    let outcome = (|| -> Result<Value> {
        check(
            fixture::snapshot(&c.shared.workspace.lock().unwrap())?.identity()?
                == before.identity()?,
            "Host constructor changed canonical fixture",
        )?;
        match &c.shared.graph_executor {
            graph::GraphExecution::Configured(runtime) => {
                runtime.attach_observation(observation.clone())?
            }
            _ => {
                return Err(Error::Blocked(
                    "Actual host constructor did not configure graph runtime".into(),
                ))
            }
        }
        let key = uuid::Uuid::new_v4().to_string();
        let queued: GraphJobInspection = serde_json::from_value(c.dispatch(queue(2, &key))?)?;
        check(
            queued.availability == GraphAvailability::Ready
                && queued.workspace_revision == 3
                && queued.job.state == ProcessingState::Queued
                && queued.job.request_key == key,
            "Host queue acknowledgement differs",
        )?;
        // Do not read any output, DB or originals after admission until supervision
        // confirms termination + cleanup and the owned publication interval ends.
        wait(&c, &observation)?;
        let page: GraphJobPage = serde_json::from_value(c.dispatch(Command::PageGraphJobs {
            request: GraphJobPageRequest {
                page_size: 25,
                cursor: None,
            },
            expected_revision: Some(5),
        })?)?;
        check(
            page.availability == GraphAvailability::Ready
                && page.workspace_revision == 5
                && page.total_count == 1
                && page.rows.len() == 1
                && page.next_cursor.is_none()
                && page.rows[0].job.id == queued.job.id,
            "Host graph page differs",
        )?;
        let finished: GraphJobInspection =
            serde_json::from_value(c.dispatch(Command::InspectGraphJob {
                job_id: page.rows[0].job.id.clone(),
            })?)?;
        check(
            finished.availability == GraphAvailability::Ready
                && finished.workspace_revision == 5
                && finished.job.state == ProcessingState::Completed
                && finished.job.failure.is_none()
                && finished.results.len() == 1
                && finished.execution.is_none()
                && same(&page.rows[0].job, &finished.job)?,
            "Host completed inspection differs",
        )?;
        let reference = &finished.results[0];
        let inspected: GraphAnalysisInspection =
            serde_json::from_value(c.dispatch(inspect(reference))?)?;
        let record = &inspected.record;
        check(
            inspected.original_integrity == GraphOriginalIntegrity::Verified
                && inspected.freshness == Some(GraphFreshness::CurrentAtPublication)
                && inspected.compared_revision == 5
                && record.requested_revision == 2
                && record.queued_revision == 3
                && record.captured_revision == 4
                && record.published_revision == 5
                && reference.captured_revision == 4
                && reference.published_revision == 5
                && finished.job.result_ids == [record.id.clone()]
                && record.job_id == finished.job.id
                && record.request_key == key
                && Some(&record.host_attempt_lease) == finished.job.lease.as_ref(),
            "Host frozen identity/revisions",
        )?;
        check(
            matches!(&record.outcome, GraphOutcome::Path{nodes,hops} if nodes == &["a","b","c"]
            && hops.len()==2 && hops[0].assertion_ids==["r1","r1-parallel"] && hops[1].assertion_ids==["r2"]),
            "Host ordinary path/provenance differs",
        )?;
        let [request, wrapper, result] = observation.retained()?;
        check(
            record.request_json.as_bytes() == request && record.result_json.as_bytes() == result,
            "Host frozen raw bytes differ",
        )?;
        raw(&root.join("request.json"), &request)?;
        raw(&root.join("wrapper.json"), &wrapper)?;
        raw(&root.join("result.json"), &result)?;
        write(&root.join("record.json"), record)?;
        let after = fixture::snapshot(&c.shared.workspace.lock().unwrap())?;
        let delta = delta(&before, &after, &queued, &finished, &inspected)?;
        let replay: GraphJobInspection = serde_json::from_value(c.dispatch(queue(2, &key))?)?;
        check(
            same(&finished, &replay)?
                && fixture::snapshot(&c.shared.workspace.lock().unwrap())?.identity()?
                    == after.identity()?,
            "Exact public queue replay mutated or replaced job",
        )?;
        check(stopped(&observation), "Host repeated worker invocation")?;
        write(&root.join("canonical-after.json"), &after)?;
        Ok(
            json!({"queued":queued,"page":page,"finished":finished,"inspection":inspected,
            "replay":replay,"delta":delta,"replay_unchanged":true}),
        )
    })();
    // Do not let Drop join/clean/traverse an uncertain native assignment.
    if !stopped(&observation) {
        return outcome;
    }
    c.shutdown()?;
    drop(ManuallyDrop::into_inner(c));
    let mut result = outcome?;
    let mut reopened = Workspace::open(root.join("workspace"))?;
    let before_read = fixture::snapshot(&reopened)?;
    check(
        before_read.identity()? == result["delta"]["after"],
        "Host reopen changed canonical state",
    )?;
    let reference: crate::graph_api::GraphResultReference =
        serde_json::from_value(result["finished"]["results"][0].clone())?;
    let read = reopened.dispatch(inspect(&reference))?;
    check(
        read == result["inspection"]
            && fixture::snapshot(&reopened)?.identity()? == before_read.identity()?,
        "Host reopened immutable inspection differs",
    )?;
    check(
        fs::read_dir(reopened.processing_scratch())?
            .next()
            .is_none(),
        "Host scratch retained after successful cleanup",
    )?;
    result["joined_shutdown"] = json!(true);
    result["reopen_unchanged"] = json!(true);
    result["scratch_empty"] = json!(true);
    Ok(result)
}

#[test]
#[ignore = "explicit reviewed pre-staged host resource/public graph proof only"]
fn native_graph_host_api() {
    let observation = Arc::new(TestObservation::new(false));
    let run = (|| -> Result<()> {
        let root = std::env::var_os("WORKBENCH_TEST_GRAPH_HOST_ARTIFACTS")
            .map(std::path::PathBuf::from)
            .ok_or_else(|| Error::Validation("Explicit host proof directory required".into()))?;
        let campaign = root.file_name().and_then(|s| s.to_str()).unwrap_or("");
        check(
            uuid::Uuid::parse_str(campaign).is_ok_and(|id| id.to_string() == campaign)
                && root.is_absolute()
                && root.canonicalize()? == root,
            "Host proof canonical UUID directory required",
        )?;
        let mut entries = fs::read_dir(&root)?
            .map(|entry| entry.map(|e| e.file_name()))
            .collect::<std::io::Result<Vec<_>>>()?;
        entries.sort();
        check(
            entries
                == [
                    "build.log",
                    "native-test-binary",
                    "native-test.log",
                    "observation.json",
                    "resources",
                ]
                .map(std::ffi::OsString::from),
            "Host proof refuses prior workspaces, receipts or unknown artifacts",
        )?;
        write(
            &root.join("native-start.json"),
            &json!({"schema_version":1,"campaign_id":campaign,"complete_release":false}),
        )?;
        let result = run(&root, observation.clone());
        write(
            &root.join("receipt.json"),
            &json!({"schema_version":1,"campaign_id":campaign,
            "passed":result.is_ok(),"failure":result.as_ref().err().map(ToString::to_string),
            "observation":observation.receipt(),"result":result.as_ref().ok(),"complete_release":false}),
        )?;
        result?;
        Ok(())
    })();
    assert!(run.is_ok(), "{run:?}");
}

#[test]
fn host_receipt_delta_binds_real_public_queue_and_complete_canonical_mutation_set() {
    let (_root, mut workspace, _) = probe_fixture::specimen();
    let before = fixture::snapshot(&workspace).unwrap();
    let key = uuid::Uuid::new_v4().to_string();
    let queued: GraphJobInspection =
        serde_json::from_value(workspace.dispatch(queue(2, &key)).unwrap()).unwrap();
    let (ticket, mut attempt) = workspace.claim_graph_exclusive(&queued.job).unwrap();
    // Source-only closed response, never used by the ignored native case.
    let mut response: Value = serde_json::from_slice(attempt.worker_input()).unwrap();
    for key in ["nodes", "edges", "source_id", "target_id"] {
        response.as_object_mut().unwrap().remove(key);
    }
    response["outcome"] = json!({"state":"path","nodes":["a","b","c"]});
    workspace
        .finish_graph_processing_job(
            &ticket,
            &mut attempt,
            Ok(&ProcessingOutput::Graph(
                serde_json::to_vec(&response).unwrap(),
            )),
        )
        .unwrap();
    let finished: GraphJobInspection = serde_json::from_value(
        workspace
            .dispatch(Command::InspectGraphJob {
                job_id: queued.job.id.clone(),
            })
            .unwrap(),
    )
    .unwrap();
    let inspected: GraphAnalysisInspection =
        serde_json::from_value(workspace.dispatch(inspect(&finished.results[0])).unwrap()).unwrap();
    let after = fixture::snapshot(&workspace).unwrap();
    delta(&before, &after, &queued, &finished, &inspected).unwrap();
    for table in after.tables.keys() {
        let mut changed = after.clone();
        changed
            .tables
            .get_mut(table)
            .unwrap()
            .push(vec![json!("unexpected")]);
        assert!(
            delta(&before, &changed, &queued, &finished, &inspected).is_err(),
            "{table}"
        );
    }
    let mut changed = after.clone();
    changed.originals.clear();
    assert!(delta(&before, &changed, &queued, &finished, &inspected).is_err());
    let mut wrong = finished.clone();
    wrong.job.detail = "Detached different acknowledgement".into();
    assert!(delta(&before, &after, &queued, &wrong, &inspected).is_err());
}
