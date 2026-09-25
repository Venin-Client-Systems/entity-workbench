//! Native development campaign through canonical public commands. Synthetic only.
//! This example does not expose an engine override, job-completion command or
//! test cancellation hook to application code.
#[path = "windows_parser_probe/fixtures.rs"]
mod fixtures;
#[path = "windows_parser_probe/runtime_copy.rs"]
mod runtime_copy;
use fixtures::{Fixture, FIXTURES};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};
use workbench_core::{
    coordinator::JobCoordinator,
    domain::Command,
    engines::Runtime,
    processing::{ExtractionRecord, ProcessingFailure, ProcessingJob, ProcessingState},
    store::{hash, Workspace},
};

type ProbeResult<T> = Result<T, Failure>;
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Failure {
    NotStarted,
    Arguments,
    UnsupportedHost,
    Workspace,
    Command,
    Deadline,
    Fixture,
    Extraction,
    Publication,
    Recovery,
    RuntimeControl,
    Shutdown,
    Cleanup,
    Receipt,
    Unexpected,
}
fn check(ok: bool, failure: Failure) -> ProbeResult<()> {
    if ok {
        Ok(())
    } else {
        Err(failure)
    }
}
const CANCEL_BYTES: &[u8] = b"Synthetic source cancelled while queued, before any claim.\n";
const REQUIRED: [&str; 25] = [
    "fixture_notice.txt",
    "fixture_unreviewed.source",
    "fixture_notice.pdf",
    "fixture_notice.docx",
    "fixture_no-text.pdf",
    "fixture_malformed.pdf",
    "fixture_traversal.zip",
    "fixture_unsupported.bin",
    "fixture_font-corpus.pdf",
    "fixture_embedded-font.pdf",
    "queued_cancellation_unclaimed",
    "evidence_unchanged",
    "idempotent_queue",
    "one_result_per_job",
    "restart_unchanged",
    "frozen_backup_isolated",
    "full_backup_restored",
    "runtime_control_baseline",
    "missing_runtime_blocked",
    "altered_runtime_blocked",
    "wrong_role_runtime_blocked",
    "control_records_unchanged",
    "joined_shutdown",
    "scratch_cleanup",
    "staged_runtime_unchanged",
];
#[derive(Serialize)]
struct JobObservation {
    state: ProcessingState,
    failure: Option<ProcessingFailure>,
    attempt: u32,
    result_count: usize,
    started: bool,
}
#[derive(Serialize)]
struct Receipt {
    schema_version: u32,
    source_commit: String,
    build_source_commit: Option<&'static str>,
    nonce: String,
    complete_release: bool,
    passed: bool,
    phase: String,
    failure: Option<Failure>,
    checks: BTreeMap<String, bool>,
    fixtures: Vec<Value>,
    derivatives: Vec<Value>,
    joined_coordinators: u32,
    retained_workspace: bool,
    in_flight_cancellation_proven: bool,
    observed_job: Option<JobObservation>,
}
impl Receipt {
    fn new(source: String, nonce: String) -> Self {
        Self {
            schema_version: 2,
            source_commit: source,
            build_source_commit: option_env!("WORKBENCH_COORDINATOR_PROBE_SOURCE"),
            nonce,
            complete_release: false,
            passed: false,
            phase: "not_started".into(),
            failure: Some(Failure::NotStarted),
            checks: BTreeMap::new(),
            fixtures: FIXTURES
                .iter()
                .map(|f| json!({"name":f.name,"sha256":hash(f.bytes),"bytes":f.bytes.len()}))
                .collect(),
            derivatives: vec![],
            joined_coordinators: 0,
            retained_workspace: false,
            in_flight_cancellation_proven: false,
            observed_job: None,
        }
    }
    fn pass(&mut self, name: &str) -> ProbeResult<()> {
        check(
            REQUIRED.contains(&name) && self.checks.insert(name.into(), true).is_none(),
            Failure::Receipt,
        )
    }
    fn save(&self, path: &Path) -> ProbeResult<()> {
        let bytes = serde_json::to_vec_pretty(self).map_err(|_| Failure::Receipt)?;
        check(bytes.len() <= 128 * 1024, Failure::Receipt)?;
        fs::write(path, bytes).map_err(|_| Failure::Receipt)
    }
    fn phase(&mut self, phase: &str, path: &Path) -> ProbeResult<()> {
        self.phase = phase.into();
        self.observed_job = None;
        self.save(path)
    }
}

fn call<T: for<'de> Deserialize<'de>>(
    coordinator: &JobCoordinator,
    command: Command,
) -> ProbeResult<T> {
    serde_json::from_value(
        coordinator
            .dispatch(command)
            .map_err(|_| Failure::Command)?,
    )
    .map_err(|_| Failure::Command)
}
fn workspace_call<T: for<'de> Deserialize<'de>>(
    workspace: &mut Workspace,
    command: Command,
) -> ProbeResult<T> {
    serde_json::from_value(workspace.dispatch(command).map_err(|_| Failure::Command)?)
        .map_err(|_| Failure::Command)
}
fn queue(
    coordinator: &JobCoordinator,
    source: &str,
    request_key: &str,
) -> ProbeResult<ProcessingJob> {
    call(
        coordinator,
        Command::QueueDocumentParse {
            evidence_id: source.into(),
            request_key: request_key.into(),
        },
    )
}
fn wait(coordinator: &JobCoordinator, id: &str) -> ProbeResult<ProcessingJob> {
    // Preparation/canonical publication have an outer bound; the actual worker
    // recipe remains at its existing 30-second limit.
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let job: ProcessingJob = call(
            coordinator,
            Command::InspectProcessingJob { job_id: id.into() },
        )?;
        if !matches!(
            job.state,
            ProcessingState::Queued | ProcessingState::Running
        ) {
            return Ok(job);
        }
        check(Instant::now() < deadline, Failure::Deadline)?;
        thread::sleep(Duration::from_millis(25));
    }
}
fn with_coordinator<T>(
    workspace: Workspace,
    receipt: &mut Receipt,
    action: impl FnOnce(&JobCoordinator, &mut Receipt) -> ProbeResult<T>,
) -> ProbeResult<T> {
    let coordinator = JobCoordinator::start(workspace, 1).map_err(|_| Failure::Workspace)?;
    let outcome = action(&coordinator, receipt);
    // Always join, including after a failed assertion or timeout. A failed join
    // governs the outcome, and prevents any workspace cleanup/recovery reads.
    coordinator.shutdown().map_err(|_| Failure::Shutdown)?;
    check(
        coordinator.dispatch(Command::View {}).is_err(),
        Failure::Shutdown,
    )?;
    receipt.joined_coordinators += 1;
    outcome
}
fn empty_scratch(root: &Path) -> ProbeResult<()> {
    check(
        fs::read_dir(root.join("scratch"))
            .map_err(|_| Failure::Cleanup)?
            .next()
            .is_none(),
        Failure::Cleanup,
    )
}
fn result(
    coordinator: &JobCoordinator,
    job: &ProcessingJob,
    fixture: Fixture,
) -> ProbeResult<Value> {
    check(
        job.result_ids.len() == 1 && job.attempt == 1,
        Failure::Publication,
    )?;
    check(
        serde_json::to_value(&job.state).map_err(|_| Failure::Publication)? == fixture.state,
        Failure::Fixture,
    )?;
    let expected_failure = match fixture.state {
        "failed" => json!("document_failed"),
        "blocked" => json!("unsupported_format"),
        _ => Value::Null,
    };
    check(
        serde_json::to_value(&job.failure).map_err(|_| Failure::Publication)? == expected_failure,
        Failure::Fixture,
    )?;
    let value: Value = call(
        coordinator,
        Command::InspectExtraction {
            extraction_id: job.result_ids[0].clone(),
        },
    )?;
    let record: ExtractionRecord =
        serde_json::from_value(value.clone()).map_err(|_| Failure::Extraction)?;
    check(
        record.schema_version == 2
            && record.id == job.result_ids[0]
            && record.job_id == job.id
            && record.attempt == job.attempt,
        Failure::Extraction,
    )?;
    let input = serde_json::to_value(&record.input).map_err(|_| Failure::Extraction)?;
    check(
        input["operation"] == "parse_document"
            && input["evidence_id"] == hash(fixture.bytes)
            && input["sha256"] == hash(fixture.bytes)
            && input["bytes"] == fixture.bytes.len() as u64,
        Failure::Extraction,
    )?;
    workbench_core::engines::parser::validate_result(
        &record.result,
        &hash(fixture.bytes),
        fixture.bytes.len() as u64,
    )
    .map_err(|_| Failure::Extraction)?;
    check(
        record.result_sha256
            == hash(&serde_json::to_vec(&record.result).map_err(|_| Failure::Extraction)?),
        Failure::Extraction,
    )?;
    fixture.validate(&value["result"])?;
    Ok(value)
}
#[derive(Clone, PartialEq, Debug)]
struct Snapshot {
    view: Value,
    jobs: Value,
    extractions: BTreeMap<String, Value>,
}
fn capture(mut dispatch: impl FnMut(Command) -> ProbeResult<Value>) -> ProbeResult<Snapshot> {
    let view = dispatch(Command::View {})?;
    let jobs = dispatch(Command::ListProcessingJobs {})?;
    let list = jobs["jobs"].as_array().ok_or(Failure::Recovery)?;
    check(jobs["total"] == list.len() as u64, Failure::Recovery)?;
    let mut extractions = BTreeMap::new();
    for job in list {
        for id in job["result_ids"].as_array().ok_or(Failure::Recovery)? {
            let id = id.as_str().ok_or(Failure::Recovery)?;
            let value = dispatch(Command::InspectExtraction {
                extraction_id: id.into(),
            })?;
            check(
                extractions.insert(id.into(), value).is_none(),
                Failure::Publication,
            )?;
        }
    }
    Ok(Snapshot {
        view,
        jobs,
        extractions,
    })
}
fn backup(coordinator: &JobCoordinator, root: &Path) -> ProbeResult<PathBuf> {
    let response: Value = call(coordinator, Command::Backup {})?;
    let name = response["backup"].as_str().ok_or(Failure::Recovery)?;
    check(uuid::Uuid::parse_str(name).is_ok(), Failure::Recovery)?;
    Ok(root.join("backups").join(name))
}
fn read_bounded(path: &Path, maximum: u64) -> ProbeResult<Vec<u8>> {
    let file = fs::File::open(path).map_err(|_| Failure::Recovery)?;
    check(
        file.metadata().map_err(|_| Failure::Recovery)?.len() <= maximum,
        Failure::Recovery,
    )?;
    let mut bytes = vec![];
    file.take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Failure::Recovery)?;
    check(bytes.len() as u64 <= maximum, Failure::Recovery)?;
    Ok(bytes)
}
fn verify_originals(root: &Path, snapshot: &Snapshot) -> ProbeResult<()> {
    for source in snapshot.view["evidence"]
        .as_array()
        .ok_or(Failure::Recovery)?
    {
        let sha = source["sha256"].as_str().ok_or(Failure::Recovery)?;
        check(valid_sha(sha) && source["id"] == sha, Failure::Recovery)?;
        let bytes = read_bounded(&root.join("originals").join(sha), 16 * 1024 * 1024)?;
        check(
            hash(&bytes) == sha && source["bytes"] == bytes.len() as u64,
            Failure::Recovery,
        )?;
    }
    Ok(())
}
fn valid_sha(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn restore(snapshot_path: &Path, destination: &Path, expected: &Snapshot) -> ProbeResult<()> {
    let mut workspace =
        Workspace::restore(snapshot_path, destination).map_err(|_| Failure::Recovery)?;
    let actual = capture(|command| workspace_call(&mut workspace, command))?;
    check(&actual == expected, Failure::Recovery)?;
    verify_originals(destination, &actual)?;
    empty_scratch(destination)
}

fn main_campaign(
    runtime: &Path,
    area: &Path,
    receipt: &mut Receipt,
    report: &Path,
) -> ProbeResult<()> {
    let root = area.join("workspace");
    let mut workspace = Workspace::open(&root).map_err(|_| Failure::Workspace)?;
    workspace.attach_runtime(Runtime {
        root: runtime.into(),
    });
    let _: Value = workspace_call(
        &mut workspace,
        Command::Import {
            name: "cancel-before-start.source".into(),
            bytes: CANCEL_BYTES.to_vec(),
        },
    )?;
    let queued: ProcessingJob = workspace_call(
        &mut workspace,
        Command::QueueDocumentParse {
            evidence_id: hash(CANCEL_BYTES),
            request_key: uuid::Uuid::new_v4().to_string(),
        },
    )?;
    let cancelled: ProcessingJob = workspace_call(
        &mut workspace,
        Command::CancelProcessingJob {
            job_id: queued.id.clone(),
            expected_attempt: queued.attempt,
        },
    )?;
    check(
        queued.attempt == 1
            && cancelled.attempt == 1
            && cancelled.started_at.is_none()
            && cancelled.result_ids.is_empty()
            && cancelled.cancellation_requested
            && cancelled.state == ProcessingState::Cancelled
            && serde_json::to_value(&cancelled.failure).map_err(|_| Failure::Publication)?
                == "cancelled_by_analyst",
        Failure::Publication,
    )?;
    let (frozen_path, frozen, full_path, full) = with_coordinator(
        workspace,
        receipt,
        |coordinator, receipt| {
            let observed: ProcessingJob = call(
                coordinator,
                Command::InspectProcessingJob {
                    job_id: queued.id.clone(),
                },
            )?;
            check(
                serde_json::to_value(&observed).map_err(|_| Failure::Publication)?
                    == serde_json::to_value(&cancelled).map_err(|_| Failure::Publication)?,
                Failure::Publication,
            )?;
            empty_scratch(&root)?;
            receipt.pass("queued_cancellation_unclaimed")?;
            let mut baselines = BTreeMap::new();
            let mut frozen = None;
            for (index, fixture) in FIXTURES.iter().copied().enumerate() {
                receipt.phase(&format!("fixture_{}", fixture.name), report)?;
                let imported: Value = call(
                    coordinator,
                    Command::Import {
                        name: fixture.name.into(),
                        bytes: fixture.bytes.to_vec(),
                    },
                )?;
                let source = imported["evidence"]
                    .as_array()
                    .and_then(|sources| {
                        sources
                            .iter()
                            .find(|source| source["id"] == hash(fixture.bytes))
                    })
                    .ok_or(Failure::Publication)?
                    .clone();
                if fixture.name == "unreviewed.source" {
                    check(source["text"].is_null(), Failure::Publication)?;
                }
                baselines.insert(hash(fixture.bytes), source);
                let key = uuid::Uuid::new_v4().to_string();
                let job = queue(coordinator, &hash(fixture.bytes), &key)?;
                let job = wait(coordinator, &job.id)?;
                // Retain only domain enums and bounded counts, never job IDs,
                // source contents, paths or worker-provided diagnostic text.
                receipt.observed_job = Some(JobObservation {
                    state: job.state.clone(),
                    failure: job.failure.clone(),
                    attempt: job.attempt,
                    result_count: job.result_ids.len(),
                    started: job.started_at.is_some(),
                });
                receipt.save(report)?;
                let extraction = result(coordinator, &job, fixture)?;
                let revision: Value = call(coordinator, Command::View {})?;
                let repeated = queue(coordinator, &hash(fixture.bytes), &key)?;
                check(
                    serde_json::to_value(&repeated).map_err(|_| Failure::Publication)?
                        == serde_json::to_value(&job).map_err(|_| Failure::Publication)?,
                    Failure::Publication,
                )?;
                let after: Value = call(coordinator, Command::View {})?;
                check(revision == after, Failure::Publication)?;
                let again: Value = call(
                    coordinator,
                    Command::InspectExtraction {
                        extraction_id: job.result_ids[0].clone(),
                    },
                )?;
                check(extraction == again, Failure::Publication)?;
                receipt.derivatives.push(json!({"fixture":fixture.name,"source_sha256":hash(fixture.bytes),"record_sha256":hash(&serde_json::to_vec(&extraction).map_err(|_| Failure::Receipt)?),"result_sha256":extraction["result_sha256"],"text_sha256":hash(extraction["result"]["text"].as_str().ok_or(Failure::Extraction)?.as_bytes()),"schema_version":2,"state":fixture.state,"parser":fixture.parser}));
                receipt.pass(&format!("fixture_{}", fixture.name))?;
                empty_scratch(&root)?;
                if index == 0 {
                    let snapshot = capture(|command| call(coordinator, command))?;
                    frozen = Some((backup(coordinator, &root)?, snapshot));
                }
            }
            let snapshot = capture(|command| call(coordinator, command))?;
            check(
                snapshot.jobs["total"] == 11 && snapshot.extractions.len() == 10,
                Failure::Publication,
            )?;
            let sources = snapshot.view["evidence"]
                .as_array()
                .ok_or(Failure::Publication)?;
            for (key, baseline) in &baselines {
                check(
                    sources.iter().find(|source| source["id"] == key.as_str()) == Some(baseline),
                    Failure::Publication,
                )?;
            }
            check(
                snapshot.view["observations"] == json!([])
                    && snapshot.view["assertions"] == json!([]),
                Failure::Publication,
            )?;
            receipt.pass("evidence_unchanged")?;
            receipt.pass("idempotent_queue")?;
            receipt.pass("one_result_per_job")?;
            let final_backup = backup(coordinator, &root)?;
            check(
                capture(|command| call(coordinator, command))? == snapshot,
                Failure::Recovery,
            )?;
            let (frozen_path, frozen) = frozen.ok_or(Failure::Recovery)?;
            Ok((frozen_path, frozen, final_backup, snapshot))
        },
    )?;
    receipt.phase("restart_and_restore", report)?;
    let reopened = Workspace::open(&root).map_err(|_| Failure::Recovery)?;
    with_coordinator(reopened, receipt, |coordinator, _| {
        check(
            capture(|command| call(coordinator, command))? == full,
            Failure::Recovery,
        )
    })?;
    verify_originals(&root, &full)?;
    receipt.pass("restart_unchanged")?;
    restore(&frozen_path, &area.join("frozen-restore"), &frozen)?;
    check(
        frozen.jobs["total"] == 2
            && frozen.extractions.len() == 1
            && full.extractions.len() == 10
            && frozen != full,
        Failure::Recovery,
    )?;
    receipt.pass("frozen_backup_isolated")?;
    restore(&full_path, &area.join("full-restore"), &full)?;
    receipt.pass("full_backup_restored")?;
    empty_scratch(&root)
}

fn blocked_job(coordinator: &JobCoordinator) -> ProbeResult<()> {
    let job = queue(
        coordinator,
        &hash(FIXTURES[0].bytes),
        &uuid::Uuid::new_v4().to_string(),
    )?;
    let job = wait(coordinator, &job.id)?;
    check(
        job.state == ProcessingState::Blocked
            && serde_json::to_value(job.failure).map_err(|_| Failure::RuntimeControl)?
                == "runtime_unavailable"
            && job.result_ids.is_empty(),
        Failure::RuntimeControl,
    )
}
fn control_workspace(root: &Path, runtime: &Path) -> ProbeResult<Workspace> {
    let mut workspace = Workspace::open(root).map_err(|_| Failure::Workspace)?;
    workspace.attach_runtime(Runtime {
        root: runtime.into(),
    });
    let _: Value = workspace_call(
        &mut workspace,
        Command::Import {
            name: FIXTURES[0].name.into(),
            bytes: FIXTURES[0].bytes.to_vec(),
        },
    )?;
    Ok(workspace)
}
fn runtime_controls(
    runtime: &Path,
    area: &Path,
    receipt: &mut Receipt,
    report: &Path,
) -> ProbeResult<()> {
    receipt.phase("missing_runtime", report)?;
    let missing_root = area.join("missing-runtime-workspace");
    with_coordinator(
        control_workspace(&missing_root, &area.join("absent-runtime"))?,
        receipt,
        |coordinator, _| blocked_job(coordinator),
    )?;
    empty_scratch(&missing_root)?;
    receipt.pass("missing_runtime_blocked")?;
    receipt.phase("runtime_control_copy", report)?;
    let clone = area.join("control-runtime");
    fs::create_dir(&clone).map_err(|_| Failure::RuntimeControl)?;
    runtime_copy::copy(&runtime.join("parser"), &clone.join("parser"))?;
    let jar_path = clone.join("parser/worker.jar");
    let manifest_path = clone.join("parser/manifest.json");
    let jar = read_bounded(&jar_path, 16 * 1024 * 1024)?;
    let manifest = read_bounded(&manifest_path, 1024 * 1024)?;
    check(!jar.is_empty(), Failure::RuntimeControl)?;
    let source_identity = (
        hash(&read_bounded(
            &runtime.join("parser/worker.jar"),
            16 * 1024 * 1024,
        )?),
        hash(&read_bounded(
            &runtime.join("parser/manifest.json"),
            1024 * 1024,
        )?),
    );
    check(
        source_identity == (hash(&jar), hash(&manifest)),
        Failure::RuntimeControl,
    )?;
    let root = area.join("runtime-control-workspace");
    with_coordinator(
        control_workspace(&root, &clone)?,
        receipt,
        |coordinator, receipt| {
            receipt.phase("runtime_control_baseline", report)?;
            let job = queue(
                coordinator,
                &hash(FIXTURES[0].bytes),
                &uuid::Uuid::new_v4().to_string(),
            )?;
            let job = wait(coordinator, &job.id)?;
            let baseline = result(coordinator, &job, FIXTURES[0])?;
            receipt.pass("runtime_control_baseline")?;
            receipt.phase("altered_runtime", report)?;
            let mut altered = jar.clone();
            altered[0] ^= 1;
            fs::write(&jar_path, altered).map_err(|_| Failure::RuntimeControl)?;
            blocked_job(coordinator)?;
            receipt.pass("altered_runtime_blocked")?;
            fs::write(&jar_path, &jar).map_err(|_| Failure::RuntimeControl)?;
            check(
                hash(&read_bounded(&jar_path, 16 * 1024 * 1024)?) == source_identity.0,
                Failure::RuntimeControl,
            )?;
            // Real success after restoration prevents a lingering mutation from
            // falsely explaining the subsequent wrong-role rejection.
            let restored = queue(
                coordinator,
                &hash(FIXTURES[0].bytes),
                &uuid::Uuid::new_v4().to_string(),
            )?;
            result(coordinator, &wait(coordinator, &restored.id)?, FIXTURES[0])?;
            receipt.phase("wrong_role_runtime", report)?;
            let mut wrong: Value =
                serde_json::from_slice(&manifest).map_err(|_| Failure::RuntimeControl)?;
            wrong["role"] = json!("search");
            fs::write(
                &manifest_path,
                serde_json::to_vec(&wrong).map_err(|_| Failure::RuntimeControl)?,
            )
            .map_err(|_| Failure::RuntimeControl)?;
            blocked_job(coordinator)?;
            receipt.pass("wrong_role_runtime_blocked")?;
            fs::write(&manifest_path, &manifest).map_err(|_| Failure::RuntimeControl)?;
            let still: Value = call(
                coordinator,
                Command::InspectExtraction {
                    extraction_id: job.result_ids[0].clone(),
                },
            )?;
            check(still == baseline, Failure::Publication)?;
            let snapshot = capture(|command| call(coordinator, command))?;
            check(
                snapshot.jobs["total"] == 4 && snapshot.extractions.len() == 2,
                Failure::Publication,
            )?;
            receipt.pass("control_records_unchanged")
        },
    )?;
    check(
        source_identity
            == (
                hash(&read_bounded(
                    &runtime.join("parser/worker.jar"),
                    16 * 1024 * 1024,
                )?),
                hash(&read_bounded(
                    &runtime.join("parser/manifest.json"),
                    1024 * 1024,
                )?),
            ),
        Failure::RuntimeControl,
    )?;
    receipt.pass("staged_runtime_unchanged")?;
    empty_scratch(&root)?;
    receipt.pass("scratch_cleanup")?;
    check(receipt.joined_coordinators == 4, Failure::Shutdown)?;
    receipt.pass("joined_shutdown")
}

fn run(runtime: &Path, receipt: &mut Receipt, report: &Path) -> ProbeResult<()> {
    check(cfg!(windows), Failure::UnsupportedHost)?;
    check(
        receipt.build_source_commit == Some(receipt.source_commit.as_str()),
        Failure::Receipt,
    )?;
    let runtime = runtime
        .canonicalize()
        .map_err(|_| Failure::RuntimeControl)?;
    let parent = std::env::temp_dir()
        .canonicalize()
        .map_err(|_| Failure::Workspace)?;
    let area = tempfile::Builder::new()
        .prefix("ew-coordinator-probe-")
        .tempdir_in(parent)
        .map_err(|_| Failure::Workspace)?
        .keep();
    // Failed campaigns retain their synthetic trees. In particular, an unknown
    // worker exit must never cause a TempDir destructor to traverse assignments.
    receipt.retained_workspace = true;
    main_campaign(&runtime, &area, receipt, report)?;
    runtime_controls(&runtime, &area, receipt, report)?;
    check(
        receipt.checks.len() == REQUIRED.len()
            && REQUIRED
                .iter()
                .all(|name| receipt.checks.get(*name) == Some(&true)),
        Failure::Receipt,
    )?;
    receipt.phase("cleanup", report)?;
    fs::remove_dir_all(area).map_err(|_| Failure::Cleanup)?;
    receipt.retained_workspace = false;
    Ok(())
}
fn main() {
    let result = entry();
    if result.is_err() {
        eprintln!(
            "Windows canonical parser development probe failed; inspect its bounded receipt."
        );
        std::process::exit(1);
    }
}
fn entry() -> ProbeResult<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    check(
        args.len() == 8
            && args[0] == "--report"
            && args[2] == "--runtime"
            && args[4] == "--source"
            && args[6] == "--nonce",
        Failure::Arguments,
    )?;
    let report = PathBuf::from(&args[1]);
    let runtime = PathBuf::from(&args[3]);
    let source = args[5].to_str().ok_or(Failure::Arguments)?;
    check(
        source.len() == 40
            && source
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        Failure::Arguments,
    )?;
    let nonce = args[7].to_str().ok_or(Failure::Arguments)?;
    check(uuid::Uuid::parse_str(nonce).is_ok(), Failure::Arguments)?;
    let mut receipt = Receipt::new(source.into(), nonce.into());
    receipt.save(&report)?;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run(&runtime, &mut receipt, &report)
    }))
    .unwrap_or(Err(Failure::Unexpected));
    receipt.passed = result.is_ok();
    receipt.failure = result.as_ref().err().copied();
    if result.is_ok() {
        receipt.phase = "complete".into();
    }
    receipt.save(&report)?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    #[test]
    fn fixture_and_check_inventory_is_closed_and_independent() {
        assert_eq!(FIXTURES.len(), 10);
        assert_eq!(
            FIXTURES
                .iter()
                .map(|f| hash(f.bytes))
                .collect::<BTreeSet<_>>()
                .len(),
            10
        );
        assert_eq!(REQUIRED.into_iter().collect::<BTreeSet<_>>().len(), 25);
        for fixture in FIXTURES {
            assert!(REQUIRED.contains(&format!("fixture_{}", fixture.name).as_str()));
        }
    }
    #[test]
    fn receipts_start_failed_and_cannot_overwrite_an_observed_case() {
        let mut receipt = Receipt::new("0".repeat(40), uuid::Uuid::new_v4().to_string());
        assert!(
            !receipt.passed && !receipt.complete_release && !receipt.in_flight_cancellation_proven
        );
        receipt.pass(REQUIRED[0]).unwrap();
        assert!(receipt.pass(REQUIRED[0]).is_err());
        assert!(receipt.pass("unknown").is_err());
        assert_eq!(receipt.checks.len(), 1);
    }
}
