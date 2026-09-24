//! Bounded local parsing and image OCR execution. The workspace mutex is released while a worker runs.
use crate::{
    domain::Command,
    engines::{CancellationToken, Runtime},
    processing::{ProcessingInput, ProcessingOutput},
    store::Workspace,
    Error, Result,
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs::File,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

type Executor = dyn Fn(
        Option<Runtime>,
        &Path,
        &ProcessingInput,
        &[u8],
        &CancellationToken,
    ) -> Result<ProcessingOutput>
    + Send
    + Sync;
struct Shared {
    workspace: Mutex<Workspace>,
    active: Mutex<BTreeMap<String, CancellationToken>>,
    stopping: AtomicBool,
    wake: Condvar,
    executor: Arc<Executor>,
}

/// One coordinator per workspace, one or two disposable worker processes at a time.
pub struct JobCoordinator {
    exports: crate::store::local_exports::NativeExports,
    shared: Arc<Shared>,
    workers: Mutex<Vec<JoinHandle<()>>>,
    ownership: Mutex<Option<File>>,
}

impl JobCoordinator {
    pub fn start(workspace: Workspace, concurrency: usize) -> Result<Self> {
        Self::with_executor(
            workspace,
            concurrency,
            Arc::new(|runtime, scratch, input, bytes, token| {
                let runtime = runtime.ok_or_else(|| {
                    Error::Blocked("Packaged processing runtime is unavailable".into())
                })?;
                match input {
                    ProcessingInput::ParseDocument { .. } => runtime
                        .parse_with_cancel(scratch, bytes, token)
                        .map(ProcessingOutput::Document),
                    ProcessingInput::PdfPageOcr {
                        page_number, dpi, ..
                    } => runtime
                        .ocr_pdf_page_with_cancel(scratch, bytes, *page_number, *dpi, token)
                        .map(|output| ProcessingOutput::Pdf(Box::new(output))),
                    ProcessingInput::ImageOcrRegions { .. } => runtime
                        .ocr_image_regions_with_cancel(scratch, bytes, token)
                        .map(|output| ProcessingOutput::ImageRegions(Box::new(output))),
                    ProcessingInput::ImageOcr { .. } => runtime
                        .ocr_image_with_cancel(scratch, bytes, token)
                        .map(|output| ProcessingOutput::Image(Box::new(output))),
                }
            }),
        )
    }

    fn with_executor(
        mut workspace: Workspace,
        concurrency: usize,
        executor: Arc<Executor>,
    ) -> Result<Self> {
        crate::require(
            (1..=2).contains(&concurrency),
            "Processing concurrency must be one or two",
        )?;
        let ownership = workspace.lock_processing()?;
        workspace.recover_processing_jobs()?;
        let exports = workspace.start_native_exports()?;
        let shared = Arc::new(Shared {
            workspace: Mutex::new(workspace),
            active: Mutex::new(BTreeMap::new()),
            stopping: AtomicBool::new(false),
            wake: Condvar::new(),
            executor,
        });
        let mut coordinator = Self {
            exports,
            shared,
            workers: Mutex::new(Vec::new()),
            ownership: Mutex::new(Some(ownership)),
        };
        for slot in 0..concurrency {
            let shared = coordinator.shared.clone();
            // If spawning a later thread fails, Drop stops and joins the threads already started.
            coordinator
                .workers
                .get_mut()
                .map_err(|_| Error::Blocked("Worker registry is unavailable".into()))?
                .push(
                    thread::Builder::new()
                        .name(format!("processing-worker-{slot}"))
                        .spawn(move || work(shared))?,
                );
        }
        Ok(coordinator)
    }

    pub fn dispatch(&self, command: Command) -> Result<Value> {
        self.dispatch_with(command, Workspace::dispatch)
    }

    pub fn dispatch_presentation(&self, command: Command) -> Result<Value> {
        self.dispatch_with(command, Workspace::dispatch_presentation)
    }

    /// Shares canonical mutation, cancellation and wakeup ownership with other modes.
    pub fn dispatch_summary(&self, command: Command) -> Result<serde_json::Value> {
        self.dispatch_with(command, Workspace::dispatch_summary)
    }
    fn dispatch_with(
        &self,
        command: Command,
        dispatch: impl FnOnce(&mut Workspace, Command) -> Result<Value>,
    ) -> Result<Value> {
        if self.shared.stopping.load(Ordering::Acquire) {
            return Err(Error::Blocked("Workspace coordinator is stopping".into()));
        }
        let cancel = match &command {
            Command::CancelProcessingJob { job_id, .. } => Some(job_id.clone()),
            _ => None,
        };
        let mut workspace = self
            .shared
            .workspace
            .lock()
            .map_err(|_| Error::Blocked("Workspace coordinator is unavailable".into()))?;
        if self.shared.stopping.load(Ordering::Acquire) {
            return Err(Error::Blocked("Workspace coordinator is stopping".into()));
        }
        let result = dispatch(&mut workspace, command)?;
        // Same lock order as claim: workspace then active. No cancellation can fall between them.
        if let Some(key) = cancel {
            if let Some(token) = self
                .shared
                .active
                .lock()
                .map_err(|_| Error::Blocked("Worker cancellation is unavailable".into()))?
                .get(&key)
            {
                token.cancel();
            }
        }
        drop(workspace);
        self.shared.wake.notify_all();
        Ok(result)
    }

    /// Export registry is always acquired before the workspace; commit/cleanup never acquire it.
    pub fn prepare_native_export(
        &self,
        request: crate::local_export::NativeExportRequest,
    ) -> Result<crate::local_export::PreparedExport> {
        self.exports.prepare(|| {
            let workspace = self
                .shared
                .workspace
                .lock()
                .map_err(|_| Error::Blocked("Workspace coordinator is unavailable".into()))?;
            if self.shared.stopping.load(Ordering::Acquire) {
                return Err(Error::Blocked("Workspace coordinator is stopping".into()));
            }
            workspace.native_export_content(request)
        })
    }
    pub fn commit_native_export(
        &self,
        ticket: &str,
        sha256: &str,
        bytes: u64,
    ) -> Result<crate::local_export::SavedExportReceipt> {
        self.exports.commit(ticket, sha256, bytes)
    }
    pub fn discard_native_export(
        &self,
        ticket: &str,
    ) -> Result<crate::local_export::DiscardedExport> {
        self.exports.discard(ticket)
    }

    /// Read only: full canonical original/raster/TSV/result verification precedes binary display.
    pub fn read_image_region_raster(&self, extraction_id: &str) -> Result<Vec<u8>> {
        if self.shared.stopping.load(Ordering::Acquire) {
            return Err(Error::Blocked("Workspace coordinator is stopping".into()));
        }
        let workspace = self
            .shared
            .workspace
            .lock()
            .map_err(|_| Error::Blocked("Workspace coordinator is unavailable".into()))?;
        if self.shared.stopping.load(Ordering::Acquire) {
            return Err(Error::Blocked("Workspace coordinator is stopping".into()));
        }
        workspace.read_image_region_raster(extraction_id)
    }

    /// Must be called by the desktop exit callback: its runtime may exit without running Drop.
    /// Safe to call repeatedly or concurrently. Return only after every executor has joined.
    pub fn shutdown(&self) -> Result<()> {
        self.shared.stopping.store(true, Ordering::Release);
        // Even if an earlier task panicked while holding a registry, shutdown must still
        // cancel known tokens and join every thread before releasing workspace ownership.
        let active = self
            .shared
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for token in active.values() {
            token.cancel();
        }
        drop(active);
        self.shared.wake.notify_all();
        let export_cleanup = self.exports.shutdown();
        let mut workers = self
            .workers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut failed = false;
        for worker in workers.drain(..) {
            failed |= worker.join().is_err();
        }
        // Unlock explicitly: closing our descriptor alone may leave a Unix flock
        // held by a descriptor inherited by an unrelated concurrent fork. Keep
        // the worker registry locked until ownership is released, so concurrent
        // shutdown calls cannot release it while this call is still joining.
        let mut ownership = self
            .ownership
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(file) = ownership.as_ref() {
            file.unlock()?;
            // A repeated shutdown must never unlock again after another
            // coordinator has acquired its own handle to the ownership file.
            *ownership = None;
        }
        export_cleanup?;
        if failed {
            return Err(Error::Interrupted(
                "A processing executor stopped unexpectedly".into(),
            ));
        }
        Ok(())
    }
}

fn work(shared: Arc<Shared>) {
    loop {
        let Ok(mut workspace) = shared.workspace.lock() else {
            return;
        };
        if shared.stopping.load(Ordering::Acquire) {
            return;
        }
        let prepared = match workspace.claim_processing_job() {
            Ok(Some(prepared)) => prepared,
            Ok(None) | Err(_) => {
                // Periodic wake also sees requests queued by another application view.
                let _guard = shared
                    .wake
                    .wait_timeout(workspace, Duration::from_millis(100));
                continue;
            }
        };
        let token = CancellationToken::default();
        let Ok(mut active) = shared.active.lock() else {
            return;
        };
        // A shutdown may have cancelled an empty registry after the initial loop check.
        // Check while holding the same registry lock used by shutdown, before launch.
        if shared.stopping.load(Ordering::Acquire) {
            token.cancel();
        }
        active.insert(prepared.ticket.job_id.clone(), token.clone());
        let runtime = workspace.processing_runtime();
        let scratch = workspace.processing_scratch();
        drop(active);
        drop(workspace);
        let result = if token.is_cancelled() {
            Err(Error::Interrupted(
                "Coordinator stopped before worker launch".into(),
            ))
        } else {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                (shared.executor)(runtime, &scratch, &prepared.input, &prepared.bytes, &token)
            }))
            .unwrap_or_else(|_| {
                Err(Error::TerminationUnverified(
                    "Processing adapter panicked; worker exit is unverified".into(),
                ))
            })
        };
        // Publication is retried without re-running the engine. No duplicate work or side effects.
        loop {
            let Ok(mut workspace) = shared.workspace.lock() else {
                return;
            };
            // Result values are bounded by the parser contract. Errors contain no worker output.
            let completion = if matches!(result, Err(Error::TerminationUnverified(_))) {
                Err(Error::TerminationUnverified(
                    "Worker exit unverified".into(),
                ))
            } else if matches!(result, Err(Error::Cleanup(_))) {
                Err(Error::Cleanup("Scratch cleanup failed".into()))
            } else if shared.stopping.load(Ordering::Acquire) {
                Err(Error::Interrupted("Coordinator stopped".into()))
            } else {
                match &result {
                    Ok(value) => Ok(value),
                    Err(Error::Cleanup(_)) => Err(Error::Cleanup("Scratch cleanup failed".into())),
                    Err(Error::Blocked(_)) => {
                        Err(Error::Blocked("Confined runtime unavailable".into()))
                    }
                    Err(Error::QuotaExhausted(_)) => {
                        Err(Error::QuotaExhausted("Worker resource limit".into()))
                    }
                    Err(Error::InvalidWorkerResult(_)) => Err(Error::InvalidWorkerResult(
                        "Worker result failed its protocol or source binding".into(),
                    )),
                    Err(_) => Err(Error::Validation("Document worker failed".into())),
                }
            };
            let finished = workspace.finish_processing_job(&prepared.ticket, completion);
            if finished.is_ok() || shared.stopping.load(Ordering::Acquire) {
                if let Ok(mut active) = shared.active.lock() {
                    active.remove(&prepared.ticket.job_id);
                }
                break;
            }
            // Keep the claim visible as running if storage is temporarily unavailable. A shutdown
            // leaves worker exit unverified at recovery; it never invents a successful completion.
            let _guard = shared
                .wake
                .wait_timeout(workspace, Duration::from_millis(100));
        }
    }
}

impl Drop for JobCoordinator {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[cfg(test)]
#[path = "coordinator_image_tests.rs"]
mod image_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        engines::parser::{ParseLimitation, ParseResult, ParseStatus},
        processing::{ProcessingJob, ProcessingState},
        store::hash,
    };
    use std::sync::atomic::AtomicUsize;
    use std::time::Instant;

    fn result(bytes: &[u8]) -> ParseResult {
        ParseResult {
            protocol_version: 1,
            job_id: uuid::Uuid::new_v4().to_string(),
            content_sha256: hash(bytes),
            source_bytes: bytes.len() as u64,
            parser: "utf8-v1".into(),
            media_type: "text/plain".into(),
            status: ParseStatus::Complete,
            text: "Synthetic worker output".into(),
            metadata: BTreeMap::new(),
            limitations: vec![ParseLimitation::NoSourceAnchors],
            error: None,
        }
    }
    fn until(mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !condition() {
            assert!(
                Instant::now() < deadline,
                "Synthetic worker deadline exceeded"
            );
            thread::sleep(Duration::from_millis(5));
        }
    }
    fn inspect(coordinator: &JobCoordinator, id: &str) -> ProcessingJob {
        serde_json::from_value(
            coordinator
                .dispatch(Command::InspectProcessingJob { job_id: id.into() })
                .unwrap(),
        )
        .unwrap()
    }
    #[test]
    #[cfg(debug_assertions)]
    fn raster_accessor_verifies_bytes_without_mutation_and_refuses_shutdown() {
        let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let mut workspace = Workspace::open(temp.path()).unwrap();
        workspace.seed_image_region_review().unwrap();
        let key = workspace
            .processing_jobs()
            .unwrap()
            .jobs
            .into_iter()
            .flat_map(|job| job.result_ids)
            .find(|key| workspace.read_image_region_raster(key).is_ok())
            .unwrap();
        let expected = workspace.read_image_region_raster(&key).unwrap();
        let coordinator = JobCoordinator::start(workspace, 1).unwrap();
        until(|| {
            let page: crate::processing::ProcessingJobPage = serde_json::from_value(
                coordinator
                    .dispatch(Command::ListProcessingJobs {})
                    .unwrap(),
            )
            .unwrap();
            page.jobs.iter().all(|job| {
                !matches!(
                    job.state,
                    ProcessingState::Queued | ProcessingState::Running
                )
            })
        });
        let revision = coordinator
            .shared
            .workspace
            .lock()
            .unwrap()
            .revision()
            .unwrap();
        assert_eq!(
            coordinator.read_image_region_raster(&key).unwrap(),
            expected
        );
        assert!(coordinator
            .read_image_region_raster("../workspace.db")
            .is_err());
        assert_eq!(
            coordinator
                .shared
                .workspace
                .lock()
                .unwrap()
                .revision()
                .unwrap(),
            revision
        );
        coordinator.shutdown().unwrap();
        assert!(matches!(
            coordinator.read_image_region_raster(&key),
            Err(Error::Blocked(_))
        ));
    }

    #[test]
    fn malformed_worker_results_stay_invalid_without_publishing_in_both_dispatch_modes() {
        for summary in [false, true] {
            let dir = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
            let mut workspace = Workspace::open(dir.path()).unwrap();
            let source = workspace
                .import("unreviewed.source", b"Synthetic unaccepted source")
                .unwrap();
            let coordinator = JobCoordinator::with_executor(
                workspace,
                1,
                Arc::new(|_, _, _, _, _| {
                    Err(Error::InvalidWorkerResult(
                        "Synthetic duplicate field or wrong request".into(),
                    ))
                }),
            )
            .unwrap();
            let request_key = uuid::Uuid::new_v4().to_string();
            let command = Command::QueueDocumentParse {
                evidence_id: source,
                request_key,
            };
            let response = if summary {
                coordinator.dispatch_summary(command.clone())
            } else {
                coordinator.dispatch(command.clone())
            }
            .unwrap();
            let job: ProcessingJob = serde_json::from_value(response).unwrap();
            until(|| inspect(&coordinator, &job.id).state == ProcessingState::Failed);
            let finished = inspect(&coordinator, &job.id);
            assert_eq!(
                finished.failure,
                Some(crate::processing::ProcessingFailure::InvalidResult)
            );
            assert!(finished.result_ids.is_empty());
            let revision = coordinator
                .shared
                .workspace
                .lock()
                .unwrap()
                .revision()
                .unwrap();
            let replay = if summary {
                coordinator.dispatch_summary(command)
            } else {
                coordinator.dispatch(command)
            }
            .unwrap();
            assert_eq!(replay["id"], job.id);
            assert_eq!(
                coordinator
                    .shared
                    .workspace
                    .lock()
                    .unwrap()
                    .revision()
                    .unwrap(),
                revision
            );
            coordinator.shutdown().unwrap();
            let reopened = Workspace::open(dir.path()).unwrap();
            assert_eq!(
                serde_json::to_value(reopened.processing_job(&job.id).unwrap()).unwrap(),
                serde_json::to_value(finished).unwrap()
            );
            assert_eq!(reopened.revision().unwrap(), revision);
            let connection = rusqlite::Connection::open(dir.path().join("workspace.db")).unwrap();
            let stored: i64 = connection
                .query_row(
                    "SELECT count(*) FROM records WHERE kind = 'extraction'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(stored, 0);
        }
    }

    #[test]
    fn responsive_cancellation_stops_execution_before_terminal_state_and_suppresses_success() {
        let dir = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let mut workspace = Workspace::open(dir.path()).unwrap();
        let source = workspace
            .import("synthetic.txt", b"Synthetic input")
            .unwrap();
        let job = workspace
            .queue_document_parse(&source, &uuid::Uuid::new_v4().to_string())
            .unwrap();
        let entered = Arc::new(AtomicBool::new(false));
        let left = Arc::new(AtomicBool::new(false));
        let entered_worker = entered.clone();
        let left_worker = left.clone();
        let coordinator = JobCoordinator::with_executor(
            workspace,
            1,
            Arc::new(move |_, _, _, bytes, token| {
                entered_worker.store(true, Ordering::Release);
                while !token.is_cancelled() {
                    thread::sleep(Duration::from_millis(5));
                }
                left_worker.store(true, Ordering::Release);
                // Deliberately race a valid success against cancellation.
                Ok(ProcessingOutput::Document(result(bytes)))
            }),
        )
        .unwrap();
        until(|| entered.load(Ordering::Acquire));
        assert_eq!(
            inspect(&coordinator, &job.id).state,
            ProcessingState::Running
        );
        let cancellation: ProcessingJob = serde_json::from_value(
            coordinator
                .dispatch(Command::CancelProcessingJob {
                    job_id: job.id.clone(),
                    expected_attempt: 1,
                })
                .unwrap(),
        )
        .unwrap();
        assert_eq!(cancellation.state, ProcessingState::Running);
        assert!(cancellation.cancellation_requested);
        until(|| inspect(&coordinator, &job.id).state == ProcessingState::Cancelled);
        assert!(left.load(Ordering::Acquire));
        assert!(inspect(&coordinator, &job.id).result_ids.is_empty());
    }
    #[test]
    fn summary_dispatch_wakes_once_and_signals_the_same_cancellation_owner() {
        let dir = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let workspace = Workspace::open(dir.path()).unwrap();
        let entered = Arc::new(AtomicUsize::new(0));
        let left = Arc::new(AtomicBool::new(false));
        let worker_entered = entered.clone();
        let worker_left = left.clone();
        let coordinator = JobCoordinator::with_executor(
            workspace,
            1,
            Arc::new(move |_, _, _, bytes, token| {
                worker_entered.fetch_add(1, Ordering::AcqRel);
                while !token.is_cancelled() {
                    thread::sleep(Duration::from_millis(5));
                }
                worker_left.store(true, Ordering::Release);
                Ok(ProcessingOutput::Document(result(bytes)))
            }),
        )
        .unwrap();
        let imported = coordinator
            .dispatch_summary(Command::Import {
                name: "summary-job.txt".into(),
                bytes: b"Synthetic summary job".to_vec(),
            })
            .unwrap();
        assert_eq!(imported["workspace"]["revision"], 1);
        assert!(imported["workspace"].get("transactions").is_none());
        let command = Command::QueueDocumentParse {
            evidence_id: imported["workspace"]["evidence"][0]["id"]
                .as_str()
                .unwrap()
                .into(),
            request_key: uuid::Uuid::new_v4().to_string(),
        };
        let job: ProcessingJob =
            serde_json::from_value(coordinator.dispatch_summary(command.clone()).unwrap()).unwrap();
        until(|| entered.load(Ordering::Acquire) == 1);
        assert_eq!(
            inspect(&coordinator, &job.id).state,
            ProcessingState::Running
        );
        let replay = coordinator.dispatch_summary(command).unwrap();
        assert_eq!(replay["id"], job.id);
        let cancellation: ProcessingJob = serde_json::from_value(
            coordinator
                .dispatch_summary(Command::CancelProcessingJob {
                    job_id: job.id.clone(),
                    expected_attempt: 1,
                })
                .unwrap(),
        )
        .unwrap();
        assert!(cancellation.cancellation_requested);
        until(|| inspect(&coordinator, &job.id).state == ProcessingState::Cancelled);
        assert!(left.load(Ordering::Acquire));
        assert_eq!(entered.load(Ordering::Acquire), 1);
        assert!(inspect(&coordinator, &job.id).result_ids.is_empty());
        let jobs = coordinator
            .dispatch_summary(Command::ListProcessingJobs {})
            .unwrap();
        assert_eq!(jobs["jobs"].as_array().unwrap().len(), 1);
        let summary = coordinator.dispatch_summary(Command::View {}).unwrap();
        assert_eq!(
            summary["workspace"]["evidence"].as_array().unwrap().len(),
            1
        );
        coordinator.shutdown().unwrap();
        assert!(coordinator.dispatch_summary(Command::View {}).is_err());
    }

    #[test]
    fn worker_pool_is_bounded_and_shutdown_reaps_all_active_executors() {
        let dir = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let mut workspace = Workspace::open(dir.path()).unwrap();
        let source = workspace
            .import("synthetic.txt", b"Synthetic input")
            .unwrap();
        let jobs: Vec<_> = (0..3)
            .map(|_| {
                workspace
                    .queue_document_parse(&source, &uuid::Uuid::new_v4().to_string())
                    .unwrap()
            })
            .collect();
        let active = Arc::new(AtomicUsize::new(0));
        let active_worker = active.clone();
        let coordinator = JobCoordinator::with_executor(
            workspace,
            2,
            Arc::new(move |_, _, _, _, token| {
                active_worker.fetch_add(1, Ordering::AcqRel);
                while !token.is_cancelled() {
                    thread::sleep(Duration::from_millis(5));
                }
                active_worker.fetch_sub(1, Ordering::AcqRel);
                Err(Error::Blocked("Synthetic cancellation".into()))
            }),
        )
        .unwrap();
        until(|| active.load(Ordering::Acquire) == 2);
        assert_eq!(
            jobs.iter()
                .filter(|job| inspect(&coordinator, &job.id).state == ProcessingState::Running)
                .count(),
            2
        );
        assert_eq!(
            inspect(&coordinator, &jobs[2].id).state,
            ProcessingState::Queued
        );
        let another = Workspace::open(dir.path()).unwrap();
        assert!(JobCoordinator::start(another, 1).is_err());
        drop(coordinator);
        assert_eq!(active.load(Ordering::Acquire), 0);
        let reopened = Workspace::open(dir.path()).unwrap();
        assert_eq!(
            reopened.processing_job(&jobs[2].id).unwrap().state,
            ProcessingState::Queued
        );
        for job in &jobs[..2] {
            assert_eq!(
                reopened.processing_job(&job.id).unwrap().failure,
                Some(crate::processing::ProcessingFailure::Interrupted)
            );
        }
    }

    #[test]
    fn shutdown_between_claim_and_registration_does_not_launch_a_worker() {
        let dir = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let mut workspace = Workspace::open(dir.path()).unwrap();
        let source = workspace
            .import("synthetic.txt", b"Synthetic input")
            .unwrap();
        let launches = Arc::new(AtomicUsize::new(0));
        let worker_launches = launches.clone();
        let coordinator = Arc::new(
            JobCoordinator::with_executor(
                workspace,
                1,
                Arc::new(move |_, _, _, bytes, _| {
                    worker_launches.fetch_add(1, Ordering::AcqRel);
                    Ok(ProcessingOutput::Document(result(bytes)))
                }),
            )
            .unwrap(),
        );
        // Pause registration after the canonical claim, without relying on scheduler timing.
        let registry = coordinator.shared.active.lock().unwrap();
        let job: ProcessingJob = serde_json::from_value(
            coordinator
                .dispatch(Command::QueueDocumentParse {
                    evidence_id: source,
                    request_key: uuid::Uuid::new_v4().to_string(),
                })
                .unwrap(),
        )
        .unwrap();
        let observer = Workspace::open(dir.path()).unwrap();
        until(|| observer.processing_job(&job.id).unwrap().state == ProcessingState::Running);
        let stopping = coordinator.clone();
        let shutdown = thread::spawn(move || stopping.shutdown());
        until(|| coordinator.shared.stopping.load(Ordering::Acquire));
        drop(registry);
        shutdown.join().unwrap().unwrap();
        assert_eq!(launches.load(Ordering::Acquire), 0);
        assert_eq!(
            observer.processing_job(&job.id).unwrap().failure,
            Some(crate::processing::ProcessingFailure::Interrupted)
        );
        assert!(coordinator.dispatch(Command::View {}).is_err());
        coordinator.shutdown().unwrap();
    }

    #[test]
    #[cfg(target_os = "macos")]
    #[ignore = "requires staged native Java parser and development confinement"]
    fn native_coordinator_publishes_and_restores_pdf_derivative() {
        let runtime = std::env::var_os("WORKBENCH_TEST_RUNTIME").expect("Staged runtime required");
        let dir = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let root = dir.path().join("workspace");
        let mut workspace = Workspace::open(&root).unwrap();
        workspace.attach_runtime(Runtime {
            root: runtime.into(),
        });
        let original = include_bytes!("../../../fixtures/parser/notice.pdf");
        let source = workspace.import("synthetic-notice.pdf", original).unwrap();
        let job = workspace
            .queue_document_parse(&source, &uuid::Uuid::new_v4().to_string())
            .unwrap();
        let coordinator = JobCoordinator::start(workspace, 1).unwrap();
        let deadline = Instant::now() + Duration::from_secs(45);
        let finished = loop {
            let job = inspect(&coordinator, &job.id);
            if !matches!(
                job.state,
                ProcessingState::Queued | ProcessingState::Running
            ) {
                break job;
            }
            assert!(
                Instant::now() < deadline,
                "Native document job deadline exceeded"
            );
            thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(finished.state, ProcessingState::Partial);
        assert_eq!(finished.result_ids.len(), 1);
        let extraction: crate::processing::ExtractionRecord = serde_json::from_value(
            coordinator
                .dispatch(Command::InspectExtraction {
                    extraction_id: finished.result_ids[0].clone(),
                })
                .unwrap(),
        )
        .unwrap();
        assert_eq!(extraction.result.content_sha256, hash(original));
        assert!(!extraction.result.text.is_empty());
        assert!(extraction
            .result
            .limitations
            .contains(&ParseLimitation::NoSourceAnchors));
        drop(coordinator);
        let mut reopened = Workspace::open(&root).unwrap();
        let backup = reopened.backup().unwrap();
        let restored = Workspace::restore(&backup, &dir.path().join("restored")).unwrap();
        assert_eq!(
            restored.processing_job(&job.id).unwrap().result_ids,
            finished.result_ids
        );
        assert_eq!(
            restored
                .extraction(&finished.result_ids[0])
                .unwrap()
                .result_sha256,
            extraction.result_sha256
        );
        let source = restored
            .view()
            .unwrap()
            .evidence
            .into_iter()
            .find(|item| item.id == source)
            .unwrap();
        assert!(
            source.text.is_none(),
            "Unreviewed derivative must not replace accepted source text"
        );
    }
}

#[cfg(test)]
#[path = "coordinator_pdf_tests.rs"]
mod pdf_tests;

#[cfg(test)]
#[path = "coordinator_region_tests.rs"]
mod region_tests;

#[cfg(test)]
#[path = "coordinator_ownership_tests.rs"]
mod ownership_tests;
