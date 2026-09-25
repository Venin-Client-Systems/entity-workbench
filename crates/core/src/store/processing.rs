use super::*;
#[cfg(any(test, debug_assertions))]
use crate::engines::parser::ParseResult;
use crate::{
    engines::parser::{validate_result, ParseStatus},
    processing::*,
};
use std::fs::File;

#[path = "processing_publication.rs"]
mod publication;
use publication::validated_derivative;
#[path = "processing_extraction.rs"]
mod extraction;
#[path = "processing_graph.rs"]
#[allow(dead_code)]
mod graph;
#[path = "processing_scheduling.rs"]
mod scheduling;

const MAX_PENDING: usize = 64;
const MAX_ATTEMPTS: u32 = 3;
const PAGE_LIMIT: u32 = 200;

pub(super) fn supported_job(job: &ProcessingJob) -> Result<()> {
    require(
        job.schema_version == 5
            && matches!(job.input, ProcessingInput::ShortestConnectionPath { .. })
            || (job.schema_version == 4
                && !matches!(job.input, ProcessingInput::ShortestConnectionPath { .. }))
            || (job.schema_version == 3
                && !matches!(
                    job.input,
                    ProcessingInput::ImageOcrRegions { .. }
                        | ProcessingInput::ShortestConnectionPath { .. }
                ))
            || (job.schema_version == 2
                && !matches!(
                    job.input,
                    ProcessingInput::PdfPageOcr { .. }
                        | ProcessingInput::ImageOcrRegions { .. }
                        | ProcessingInput::ShortestConnectionPath { .. }
                ))
            || (job.schema_version == 1
                && matches!(job.input, ProcessingInput::ParseDocument { .. })),
        "Unsupported processing job version or operation",
    )?;
    if let ProcessingInput::ShortestConnectionPath {
        requested_revision,
        queued_revision,
        ..
    } = job.input
    {
        require(
            requested_revision < queued_revision
                && (job.attempt != 1 || requested_revision.checked_add(1) == Some(queued_revision)),
            "Invalid graph queue revisions",
        )?;
    }
    if let ProcessingInput::PdfPageOcr {
        page_number, dpi, ..
    } = job.input
    {
        crate::engines::pdf_render::validate_settings(page_number, dpi)?;
    }
    Ok(())
}

fn attempt(job: &ProcessingJob, expected: u32) -> Result<()> {
    if job.attempt != expected {
        return Err(Error::Conflict(
            "Job attempt changed; reload before applying this action".into(),
        ));
    }
    Ok(())
}

pub(super) fn terminal(
    job: &mut ProcessingJob,
    state: ProcessingState,
    failure: Option<ProcessingFailure>,
    detail: &str,
) {
    job.state = state;
    job.failure = failure;
    job.detail = detail.into();
    job.updated_at = now();
    job.finished_at = Some(job.updated_at.clone());
}
pub(super) fn image_outcome(
    job: &mut ProcessingJob,
    status: &crate::engines::image::DecodeStatus,
    empty: bool,
    retained: bool,
) {
    use crate::engines::image::DecodeStatus;
    match status {
        DecodeStatus::Decoded => {
            let detail=match (retained,empty) {
                (true,true)=>"Image word-region OCR completed without recognized text. Validated raster, TSV and result retained; no facts were accepted",
                (true,false)=>"Unreviewed image word regions and validated raster, TSV and result retained; no facts were accepted",
                (false,true)=>"Image decoded and OCR completed without recognized text; no facts were accepted. Raster was not retained",
                (false,false)=>"Image OCR completed; unreviewed recognition and provenance retained. Raster was not retained",
            };
            terminal(job, ProcessingState::Completed, None, detail);
        }
        DecodeStatus::Unsupported => terminal(
            job,
            ProcessingState::Blocked,
            Some(ProcessingFailure::UnsupportedFormat),
            "The image decoder does not support this input; no OCR was run",
        ),
        DecodeStatus::Failed => terminal(
            job,
            ProcessingState::Failed,
            Some(ProcessingFailure::ImageDecodeFailed),
            "Image decoding failed; its typed outcome was retained and no OCR was run",
        ),
        DecodeStatus::QuotaExhausted => terminal(
            job,
            ProcessingState::QuotaExhausted,
            Some(ProcessingFailure::WorkerFailed),
            "Image decoding exceeded its limit; its typed outcome was retained and no OCR was run",
        ),
    }
}

fn pending_count(conn: &Connection) -> Result<usize> {
    Ok(conn.query_row("SELECT count(*) FROM records WHERE kind='processing_job' AND json_extract(body,'$.state') IN ('queued','running')", [], |row| row.get(0))?)
}

fn require_verified_worker_exit(conn: &Connection) -> Result<()> {
    if worker_exit_unverified(conn)? {
        return Err(Error::Blocked("Document execution is suspended because a previous worker's exit is unverified; verified process recovery is required".into()));
    }
    Ok(())
}

fn worker_exit_unverified(conn: &Connection) -> Result<bool> {
    Ok(conn.query_row("SELECT EXISTS(SELECT 1 FROM records WHERE kind='processing_job' AND json_extract(body,'$.failure')='worker_exit_unverified')", [], |row| row.get(0))?)
}

impl Workspace {
    pub(crate) fn processing_execution_suspended(&self) -> Result<bool> {
        worker_exit_unverified(&self.conn)
    }
}

fn suspend_queued_jobs(conn: &Connection) -> Result<()> {
    for mut queued in all::<ProcessingJob>(conn, "processing_job")? {
        if queued.state == ProcessingState::Queued {
            terminal(&mut queued, ProcessingState::Blocked, Some(ProcessingFailure::RecoveryRequired), "An earlier worker's exit is unverified. Document execution is suspended until verified process recovery");
            put(conn, "processing_job", &queued.id, &queued)?;
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum RequestedOperation {
    Parse,
    Image,
    ImageRegions,
    Pdf { page_number: u32, dpi: u32 },
}

impl Workspace {
    pub fn queue_document_parse(
        &mut self,
        evidence_id: &str,
        request_key: &str,
    ) -> Result<ProcessingJob> {
        self.queue_processing(evidence_id, request_key, RequestedOperation::Parse)
    }

    pub fn queue_image_ocr(
        &mut self,
        evidence_id: &str,
        request_key: &str,
    ) -> Result<ProcessingJob> {
        self.queue_processing(evidence_id, request_key, RequestedOperation::Image)
    }

    pub fn queue_image_ocr_regions(
        &mut self,
        evidence_id: &str,
        request_key: &str,
    ) -> Result<ProcessingJob> {
        self.queue_processing(evidence_id, request_key, RequestedOperation::ImageRegions)
    }

    pub fn queue_pdf_page_ocr(
        &mut self,
        evidence_id: &str,
        request_key: &str,
        page_number: u32,
        dpi: u32,
    ) -> Result<ProcessingJob> {
        crate::engines::pdf_render::validate_settings(page_number, dpi)?;
        self.queue_processing(
            evidence_id,
            request_key,
            RequestedOperation::Pdf { page_number, dpi },
        )
    }

    fn queue_processing(
        &mut self,
        evidence_id: &str,
        request_key: &str,
        operation: RequestedOperation,
    ) -> Result<ProcessingJob> {
        require(
            Uuid::parse_str(request_key).is_ok_and(|key| key.to_string() == request_key),
            "A canonical UUID request key is required",
        )?;
        let expected = self.revision()?;
        let evidence = get_evidence(&self.conn, evidence_id)?;
        let input = match operation {
            RequestedOperation::Parse => ProcessingInput::ParseDocument {
                evidence_id: evidence.id.clone(),
                sha256: evidence.sha256.clone(),
                bytes: evidence.bytes,
            },
            RequestedOperation::Image => ProcessingInput::ImageOcr {
                evidence_id: evidence.id.clone(),
                sha256: evidence.sha256.clone(),
                bytes: evidence.bytes,
            },
            RequestedOperation::ImageRegions => ProcessingInput::ImageOcrRegions {
                evidence_id: evidence.id.clone(),
                sha256: evidence.sha256.clone(),
                bytes: evidence.bytes,
            },
            RequestedOperation::Pdf { page_number, dpi } => ProcessingInput::PdfPageOcr {
                evidence_id: evidence.id.clone(),
                sha256: evidence.sha256.clone(),
                bytes: evidence.bytes,
                page_number,
                dpi,
            },
        };
        let existing: Option<String> = self
            .conn
            .query_row(
                "SELECT body FROM records WHERE kind='processing_request' AND id=?",
                [request_key],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(existing) = existing {
            let key: String = serde_json::from_str(&existing)?;
            let job = self.processing_job(&key)?;
            if job.input != input {
                return Err(Error::Conflict(
                    "Request key already belongs to another input".into(),
                ));
            }
            return Ok(job);
        }
        self.verify_original(&evidence)?;
        require_verified_worker_exit(&self.conn)?;
        require(
            evidence.bytes <= policy::MAX_IMPORT_BYTES as u64,
            "Document exceeds the import limit",
        )?;
        let at = now();
        let job = ProcessingJob {
            schema_version: 4,
            id: id(),
            request_key: request_key.into(),
            input,
            state: ProcessingState::Queued,
            attempt: 1,
            retry: RetryPolicy {
                automatic: false,
                max_attempts: MAX_ATTEMPTS,
            },
            cancellation_requested: false,
            created_at: at.clone(),
            updated_at: at,
            started_at: None,
            finished_at: None,
            failure: None,
            detail: match operation {
                RequestedOperation::Parse => "Queued for local document parsing",
                RequestedOperation::Image => "Queued for local image decoding and OCR",
                RequestedOperation::ImageRegions => {
                    "Queued for local image word regions and retained derivative files"
                }
                RequestedOperation::Pdf { .. } => {
                    "Queued for local rendering and OCR of the selected PDF page"
                }
            }
            .into(),
            result_ids: Vec::new(),
            lease: None,
        };
        self.change(Some(expected), "processing.queue", false, |conn| {
            require_verified_worker_exit(conn)?;
            require(pending_count(conn)? < MAX_PENDING, "Document queue is full")?;
            put(conn, "processing_request", request_key, &job.id)?;
            put(conn, "processing_job", &job.id, &job)
        })?;
        Ok(job)
    }

    pub fn processing_job(&self, job_id: &str) -> Result<ProcessingJob> {
        let job = get(&self.conn, "processing_job", job_id)?;
        supported_job(&job)?;
        Ok(job)
    }

    pub fn processing_jobs(&self) -> Result<ProcessingJobPage> {
        let total: u64 = self.conn.query_row(
            "SELECT count(*) FROM records WHERE kind='processing_job'",
            [],
            |row| row.get(0),
        )?;
        let mut stmt = self.conn.prepare(
            "SELECT body FROM records WHERE kind='processing_job' ORDER BY sequence DESC LIMIT ?",
        )?;
        let rows = stmt.query_map([PAGE_LIMIT], |row| row.get::<_, String>(0))?;
        let jobs = rows
            .map(|body| {
                let job = serde_json::from_str(&body?)?;
                supported_job(&job)?;
                Ok(job)
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(ProcessingJobPage {
            jobs,
            total,
            limit: PAGE_LIMIT,
        })
    }

    pub fn cancel_processing_job(
        &mut self,
        job_id: &str,
        expected_attempt: u32,
    ) -> Result<ProcessingJob> {
        self.cancel_job(job_id, expected_attempt, false)
    }

    pub(crate) fn cancel_graph_processing_job(
        &mut self,
        job_id: &str,
        expected_attempt: u32,
    ) -> Result<ProcessingJob> {
        self.cancel_job(job_id, expected_attempt, true)
    }

    fn cancel_job(
        &mut self,
        job_id: &str,
        expected_attempt: u32,
        graph_only: bool,
    ) -> Result<ProcessingJob> {
        let expected = self.revision()?;
        let mut job = if graph_only {
            super::graph_api::load(&self.conn, job_id)?
        } else {
            self.processing_job(job_id)?
        };
        attempt(&job, expected_attempt)?;
        if job.state == ProcessingState::Cancelled || job.cancellation_requested {
            return Ok(job);
        }
        require(
            matches!(
                job.state,
                ProcessingState::Queued | ProcessingState::Running
            ),
            "Only queued or running jobs can be cancelled",
        )?;
        job.cancellation_requested = true;
        job.updated_at = now();
        if job.state == ProcessingState::Queued {
            terminal(
                &mut job,
                ProcessingState::Cancelled,
                Some(ProcessingFailure::CancelledByAnalyst),
                "Cancelled before a worker started",
            );
        } else {
            job.detail = "Cancellation requested; waiting for the worker to stop".into();
        }
        self.change(Some(expected), "processing.cancel", false, |conn| {
            put(conn, "processing_job", job_id, &job)
        })?;
        Ok(job)
    }

    pub fn retry_processing_job(
        &mut self,
        job_id: &str,
        expected_attempt: u32,
        why: &str,
    ) -> Result<ProcessingJob> {
        reason(why)?;
        require_verified_worker_exit(&self.conn)?;
        let expected = self.revision()?;
        let mut job = self.processing_job(job_id)?;
        attempt(&job, expected_attempt)?;
        require(
            matches!(
                job.state,
                ProcessingState::Partial
                    | ProcessingState::Failed
                    | ProcessingState::Blocked
                    | ProcessingState::QuotaExhausted
                    | ProcessingState::Cancelled
            ),
            "This job cannot be retried in its current state",
        )?;
        require(job.attempt < MAX_ATTEMPTS, "Manual retry limit exhausted")?;
        if matches!(job.input, ProcessingInput::ShortestConnectionPath { .. }) {
            self.verify_graph_endpoints(&job.input)?;
        } else {
            self.verify_processing_input(&job.input)?;
        }
        if let ProcessingInput::ShortestConnectionPath {
            queued_revision, ..
        } = &mut job.input
        {
            *queued_revision = expected
                .checked_add(1)
                .ok_or_else(|| Error::Validation("Graph revision overflow".into()))?;
        }
        job.state = ProcessingState::Queued;
        job.attempt += 1;
        job.cancellation_requested = false;
        job.updated_at = now();
        job.started_at = None;
        job.finished_at = None;
        job.failure = None;
        job.lease = None;
        job.detail = "Queued by analyst for another local attempt".into();
        self.change(Some(expected), "processing.retry", false, |conn| {
            require_verified_worker_exit(conn)?;
            require(pending_count(conn)? < MAX_PENDING, "Document queue is full")?;
            record_decision(conn, job_id, ReviewState::Deferred, why)?;
            put(conn, "processing_job", job_id, &job)
        })?;
        Ok(job)
    }

    pub(super) fn verify_processing_input(&self, input: &ProcessingInput) -> Result<Vec<u8>> {
        let (evidence_id, sha256, bytes) = input
            .source()
            .ok_or_else(|| Error::Validation("Graph input is not an imported document".into()))?;
        let evidence: Evidence = get(&self.conn, "evidence", evidence_id)?;
        require(
            evidence.id == evidence_id
                && evidence.sha256 == sha256
                && evidence.bytes == bytes
                && bytes <= policy::MAX_IMPORT_BYTES as u64,
            "Job input no longer matches the retained evidence",
        )?;
        // Consume the single bounded buffer whose identity and digest were verified.
        read_original(&self.root, &evidence)
    }

    pub(crate) fn claim_processing_job(&mut self) -> Result<Option<PreparedProcessingJob>> {
        require_verified_worker_exit(&self.conn)?;
        let expected = self.revision()?;
        let body: Option<String> = self.conn.query_row("SELECT body FROM records WHERE kind='processing_job' AND json_extract(body,'$.state')='queued' ORDER BY sequence LIMIT 1", [], |row| row.get(0)).optional()?;
        let Some(body) = body else {
            return Ok(None);
        };
        let mut job: ProcessingJob = serde_json::from_str(&body)?;
        supported_job(&job)?;
        if matches!(job.input, ProcessingInput::ShortestConnectionPath { .. }) {
            terminal(&mut job, ProcessingState::Blocked,
                Some(ProcessingFailure::SchedulingOrRuntimeUnavailable),
                "Graph execution is disabled pending exclusive analytical scheduling and a verified application-local runtime; no capture or worker started");
            self.change(
                Some(expected),
                "processing.graph_unavailable",
                false,
                |conn| put(conn, "processing_job", &job.id, &job),
            )?;
            return Ok(None);
        }
        let bytes = match self.verify_processing_input(&job.input) {
            Ok(bytes) => bytes,
            Err(_) => {
                terminal(
                    &mut job,
                    ProcessingState::Blocked,
                    Some(ProcessingFailure::InputUnavailable),
                    "Retained input is missing, altered or invalid; restore it before retrying",
                );
                self.change(Some(expected), "processing.input_blocked", false, |conn| {
                    put(conn, "processing_job", &job.id, &job)
                })?;
                return Ok(None);
            }
        };
        require(
            (1..=MAX_ATTEMPTS).contains(&job.attempt) && !job.cancellation_requested,
            "Invalid queued job state",
        )?;
        let lease = id();
        job.state = ProcessingState::Running;
        job.lease = Some(lease.clone());
        job.started_at = Some(now());
        job.updated_at = now();
        job.detail = match job.input {
            ProcessingInput::ShortestConnectionPath { .. } => "Graph execution is disabled",
            ProcessingInput::ParseDocument { .. } => "Local document worker is running",
            ProcessingInput::PdfPageOcr { .. } => {
                "Local PDF page rendering and OCR workers are running in sequence"
            }
            ProcessingInput::ImageOcrRegions { .. } => {
                "Local image decoding and word-region OCR workers are running in sequence"
            }
            ProcessingInput::ImageOcr { .. } => {
                "Local image decoding and OCR workers are running in sequence"
            }
        }
        .into();
        self.change(Some(expected), "processing.claim", false, |conn| {
            require_verified_worker_exit(conn)?;
            put(conn, "processing_job", &job.id, &job)
        })?;
        let ticket = JobTicket {
            job_id: job.id,
            attempt: job.attempt,
            lease,
        };
        Ok(Some(PreparedProcessingJob {
            ticket,
            input: job.input,
            bytes,
        }))
    }

    /// The lock covers the entire coordinator lifetime. Opening a read/write view does not recover jobs.
    pub(crate) fn lock_processing(&self) -> Result<File> {
        let path = self.root.join("processing.lock");
        reject_link_ancestors(&path)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;
        require(file.metadata()?.is_file(), "Invalid processing lock file")?;
        private_file(&path, 0o600)?;
        file.try_lock().map_err(|_| {
            Error::Blocked("Another document coordinator owns this workspace".into())
        })?;
        Ok(file)
    }

    pub(crate) fn processing_scratch(&self) -> PathBuf {
        self.root.join("scratch")
    }
    pub(crate) fn processing_runtime(&self) -> Option<crate::engines::Runtime> {
        self.runtime.clone()
    }

    pub(crate) fn recover_processing_jobs(&mut self) -> Result<usize> {
        let expected = self.revision()?;
        let jobs: Vec<ProcessingJob> = all(&self.conn, "processing_job")?;
        for job in &jobs {
            supported_job(job)?;
        }
        let jobs: Vec<_> = jobs
            .into_iter()
            .filter(|job| {
                job.state == ProcessingState::Running
                    // Earlier development builds labelled orphan recovery Interrupted,
                    // clearing its lease. A joined, explicitly stopped attempt retains
                    // its lease and can still be retried safely under that category.
                    || (job.failure == Some(ProcessingFailure::Interrupted) && job.lease.is_none())
            })
            .collect();
        if jobs.is_empty() {
            return Ok(0);
        }
        let count = jobs.len();
        self.change(Some(expected), "processing.recover", false, |conn| {
            for mut job in jobs {
                terminal(&mut job, ProcessingState::Failed, Some(ProcessingFailure::WorkerExitUnverified), "Previous coordinator did not record a verified worker exit. Worker exit is unverified; document execution is suspended until verified process recovery. No new result was published");
                job.lease = None;
                put(conn, "processing_job", &job.id, &job)?;
            }
            suspend_queued_jobs(conn)?;
            Ok(())
        })?;
        Ok(count)
    }

    #[cfg(any(test, debug_assertions))]
    pub(crate) fn finish_document_job(
        &mut self,
        ticket: &JobTicket,
        result: Result<ParseResult>,
    ) -> Result<ProcessingJob> {
        match result {
            Ok(result) => {
                self.finish_processing_job(ticket, Ok(&ProcessingOutput::Document(result)))
            }
            Err(error) => self.finish_processing_job(ticket, Err(error)),
        }
    }

    /// Only the local coordinator has a claim ticket. Worker output is validated before publication.
    pub(crate) fn finish_processing_job(
        &mut self,
        ticket: &JobTicket,
        result: Result<&ProcessingOutput>,
    ) -> Result<ProcessingJob> {
        let expected = self.revision()?;
        let mut job = self.processing_job(&ticket.job_id)?;
        require(
            !matches!(job.input, ProcessingInput::ShortestConnectionPath { .. }),
            "Graph publication requires its private attempt",
        )?;
        attempt(&job, ticket.attempt)?;
        require(
            job.lease.as_deref() == Some(&ticket.lease),
            "Stale or unowned processing attempt",
        )?;
        if job.state != ProcessingState::Running {
            // A completion may be delivered again after acknowledgement is lost. Never publish twice.
            if let Ok(result) = &result {
                if self.is_processing_replay(&job, ticket, result)? {
                    return Ok(job);
                }
            }
            return Err(Error::Conflict(
                "Processing attempt already finished".into(),
            ));
        }
        let mut extraction = None;
        if matches!(result, Err(Error::TerminationUnverified(_))) {
            terminal(&mut job, ProcessingState::Failed, Some(ProcessingFailure::WorkerExitUnverified), "Worker exit could not be confirmed. Assignment files are retained and further document execution is suspended; no result was published");
        } else if job.cancellation_requested && matches!(result, Err(Error::Cleanup(_))) {
            terminal(&mut job, ProcessingState::Cancelled, Some(ProcessingFailure::CleanupFailed), "Worker stopped after cancellation; scratch cleanup failed and requires attention. No result was published");
        } else if matches!(result, Err(Error::Cleanup(_))) {
            terminal(
                &mut job,
                ProcessingState::Failed,
                Some(ProcessingFailure::CleanupFailed),
                "Worker scratch cleanup failed; no derivative was published",
            );
        } else if job.cancellation_requested {
            terminal(
                &mut job,
                ProcessingState::Cancelled,
                Some(ProcessingFailure::CancelledByAnalyst),
                "Worker stopped; no result from the cancelled attempt was published",
            );
        } else {
            let original = self.verify_processing_input(&job.input);
            match (original, &result) {
                (Err(_), _) => terminal(
                    &mut job,
                    ProcessingState::Blocked,
                    Some(ProcessingFailure::InputUnavailable),
                    "Retained input failed verification; the result was not published",
                ),
                (Ok(original), Ok(output)) => {
                    match validated_derivative(&mut job, &original, output) {
                        Ok(record) => {
                            job.result_ids.push(record.id().to_owned());
                            extraction = Some(record);
                        }
                        Err(_) => terminal(&mut job, ProcessingState::Failed, Some(ProcessingFailure::InvalidResult), "Worker result failed canonical validation; no derivative was published"),
                    }
                }
                (Ok(_), Err(Error::Cleanup(_))) => terminal(
                    &mut job,
                    ProcessingState::Failed,
                    Some(ProcessingFailure::CleanupFailed),
                    "Worker scratch cleanup failed; no derivative was published",
                ),
                (Ok(_), Err(Error::Interrupted(_))) => terminal(
                    &mut job,
                    ProcessingState::Failed,
                    Some(ProcessingFailure::Interrupted),
                    "Coordinator stopped the worker; an explicit manual retry is required",
                ),
                (Ok(_), Err(Error::Blocked(_))) => terminal(
                    &mut job,
                    ProcessingState::Blocked,
                    Some(ProcessingFailure::RuntimeUnavailable),
                    "The required confined local runtime is unavailable",
                ),
                (Ok(_), Err(Error::QuotaExhausted(_))) => terminal(
                    &mut job,
                    ProcessingState::QuotaExhausted,
                    Some(ProcessingFailure::WorkerFailed),
                    "Worker resource limit exhausted",
                ),
                (Ok(_), Err(Error::InvalidWorkerResult(_))) => terminal(
                    &mut job,
                    ProcessingState::Failed,
                    Some(ProcessingFailure::InvalidResult),
                    "Worker result failed its protocol or source binding; no derivative was published",
                ),
                (Ok(_), Err(_)) => terminal(
                    &mut job,
                    ProcessingState::Failed,
                    Some(ProcessingFailure::WorkerFailed),
                    "Worker failed; no derivative was published",
                ),
            }
        }
        if let (Some(record), Ok(output)) = (&extraction, &result) {
            match record.prepare_files(&self.root, &self.conn, output) {
                Ok(()) => {}
                Err(Error::DerivativeUnavailable { published }) => {
                    job.result_ids.pop();
                    extraction = None;
                    terminal(
                        &mut job,
                        ProcessingState::Failed,
                        Some(ProcessingFailure::DerivativeUnavailable),
                        if published {
                            "A previously published derivative is missing or altered; restore verified storage before retrying. No new result was published"
                        } else {
                            "Unreferenced derivative storage could not be prepared or verified. No new result was published"
                        },
                    );
                }
                Err(Error::Cleanup(_)) => {
                    job.result_ids.pop();
                    extraction = None;
                    terminal(&mut job, ProcessingState::Failed, Some(ProcessingFailure::CleanupFailed), "Derivative staging cleanup failed and requires attention. No new result was published");
                }
                Err(error) => return Err(error),
            }
        }
        self.change(Some(expected), "processing.finish", false, |conn| {
            if job.failure == Some(ProcessingFailure::WorkerExitUnverified) {
                suspend_queued_jobs(conn)?;
            }
            if let Some(record) = &extraction {
                record.publish(conn)?;
            }
            put(conn, "processing_job", &job.id, &job)
        })?;
        Ok(job)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::parser::ParseLimitation;
    use std::collections::BTreeMap;

    fn fixture() -> (tempfile::TempDir, Workspace, String) {
        let dir = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let mut workspace = Workspace::open(dir.path()).unwrap();
        let evidence = workspace
            .import("synthetic.txt", b"Synthetic source only.")
            .unwrap();
        (dir, workspace, evidence)
    }
    fn queue(workspace: &mut Workspace, evidence: &str) -> ProcessingJob {
        workspace.queue_document_parse(evidence, &id()).unwrap()
    }
    fn result(bytes: &[u8]) -> ParseResult {
        ParseResult {
            protocol_version: 1,
            job_id: id(),
            content_sha256: hash(bytes),
            source_bytes: bytes.len() as u64,
            parser: "utf8-v1".into(),
            media_type: "text/plain".into(),
            status: ParseStatus::Complete,
            text: "An unreviewed derivative.".into(),
            metadata: BTreeMap::new(),
            limitations: vec![ParseLimitation::NoSourceAnchors],
            error: None,
        }
    }
    #[test]
    fn request_keys_are_idempotent_and_do_not_alias_different_sources() {
        let (_dir, mut workspace, evidence) = fixture();
        let key = id();
        let first = workspace.queue_document_parse(&evidence, &key).unwrap();
        let revision = workspace.revision().unwrap();
        assert_eq!(
            workspace.queue_document_parse(&evidence, &key).unwrap().id,
            first.id
        );
        assert_eq!(workspace.revision().unwrap(), revision);
        let other = workspace
            .import("other.txt", b"Other synthetic source.")
            .unwrap();
        assert!(matches!(
            workspace.queue_document_parse(&other, &key),
            Err(Error::Conflict(_))
        ));
        assert_eq!(workspace.processing_jobs().unwrap().total, 1);
        assert!(workspace
            .queue_document_parse(&evidence, "not-a-uuid")
            .is_err());
    }
    #[test]
    fn publication_is_atomic_idempotent_and_preserves_original_text() {
        let (_dir, mut workspace, evidence) = fixture();
        let job = queue(&mut workspace, &evidence);
        let prepared = workspace.claim_processing_job().unwrap().unwrap();
        let result = result(&prepared.bytes);
        workspace.conn.execute_batch("CREATE TRIGGER reject_finish BEFORE UPDATE ON records WHEN NEW.kind='processing_job' AND json_extract(NEW.body,'$.state')='completed' BEGIN SELECT RAISE(ABORT,'synthetic publication failure'); END;").unwrap();
        let revision = workspace.revision().unwrap();
        assert!(workspace
            .finish_document_job(&prepared.ticket, Ok(result.clone()))
            .is_err());
        assert_eq!(
            all::<ExtractionRecord>(&workspace.conn, "extraction")
                .unwrap()
                .len(),
            0
        );
        assert_eq!(
            workspace.processing_job(&job.id).unwrap().state,
            ProcessingState::Running
        );
        assert_eq!(workspace.revision().unwrap(), revision);
        workspace
            .conn
            .execute_batch("DROP TRIGGER reject_finish;")
            .unwrap();
        let finished = workspace
            .finish_document_job(&prepared.ticket, Ok(result.clone()))
            .unwrap();
        let revision = workspace.revision().unwrap();
        assert_eq!(finished.state, ProcessingState::Completed);
        assert_eq!(finished.result_ids.len(), 1);
        assert_eq!(
            workspace
                .finish_document_job(&prepared.ticket, Ok(result.clone()))
                .unwrap()
                .result_ids,
            finished.result_ids
        );
        assert_eq!(workspace.revision().unwrap(), revision);
        let mut altered = result;
        altered.text = "Conflicting replay".into();
        assert!(workspace
            .finish_document_job(&prepared.ticket, Ok(altered))
            .is_err());
        let source: Evidence = get(&workspace.conn, "evidence", &evidence).unwrap();
        assert_eq!(source.text.as_deref(), Some("Synthetic source only."));
    }
    #[test]
    fn cancellation_wins_a_success_race_and_stale_actions_cannot_touch_retry() {
        let (_dir, mut workspace, evidence) = fixture();
        let job = queue(&mut workspace, &evidence);
        let prepared = workspace.claim_processing_job().unwrap().unwrap();
        assert!(workspace.cancel_processing_job(&job.id, 0).is_err());
        let pending = workspace.cancel_processing_job(&job.id, 1).unwrap();
        assert_eq!(pending.state, ProcessingState::Running);
        assert!(pending.cancellation_requested);
        let cancelled = workspace
            .finish_document_job(&prepared.ticket, Ok(result(&prepared.bytes)))
            .unwrap();
        assert_eq!(cancelled.state, ProcessingState::Cancelled);
        assert!(cancelled.result_ids.is_empty());
        workspace
            .retry_processing_job(&job.id, 1, "Synthetic manual retry")
            .unwrap();
        assert!(
            workspace.cancel_processing_job(&job.id, 1).is_err(),
            "A stale cancellation must not cancel a queued retry"
        );
        let next = workspace.claim_processing_job().unwrap().unwrap();
        assert_eq!(next.ticket.attempt, 2);
        assert!(workspace.cancel_processing_job(&job.id, 1).is_err());
        assert!(workspace
            .finish_document_job(&prepared.ticket, Ok(result(&prepared.bytes)))
            .is_err());
        assert_eq!(
            workspace.processing_job(&job.id).unwrap().state,
            ProcessingState::Running
        );
    }
    #[test]
    fn retries_are_manual_and_bounded_and_queued_cancel_starts_no_attempt() {
        let (_dir, mut workspace, evidence) = fixture();
        let job = queue(&mut workspace, &evidence);
        assert_eq!(
            workspace.cancel_processing_job(&job.id, 1).unwrap().state,
            ProcessingState::Cancelled
        );
        assert!(workspace.claim_processing_job().unwrap().is_none());
        let job = queue(&mut workspace, &evidence);
        for attempt in 1..=3 {
            let prepared = workspace.claim_processing_job().unwrap().unwrap();
            assert_eq!(prepared.ticket.attempt, attempt);
            let failed = workspace
                .finish_document_job(
                    &prepared.ticket,
                    Err(Error::Validation("Synthetic worker failure".into())),
                )
                .unwrap();
            assert_eq!(failed.state, ProcessingState::Failed);
            assert!(workspace.claim_processing_job().unwrap().is_none());
            let retry = workspace.retry_processing_job(&job.id, attempt, "Retry synthetic job");
            assert_eq!(retry.is_ok(), attempt < 3);
        }
    }
    #[test]
    fn prepared_processing_bytes_are_exact_and_altered_source_never_launches() {
        let (_temp, mut w, key) = fixture();
        let job = queue(&mut w, &key);
        let prepared = w.claim_processing_job().unwrap().unwrap();
        assert_eq!(prepared.bytes, b"Synthetic source only.");
        assert_eq!(hash(&prepared.bytes), key);
        w.finish_document_job(
            &prepared.ticket,
            Err(Error::Validation("synthetic failure".into())),
        )
        .unwrap();
        let next = queue(&mut w, &key);
        let path = w.root.join("originals").join(&key);
        fs::remove_file(&path).unwrap();
        fs::write(&path, b"Changed source bytes.").unwrap();
        assert!(w.claim_processing_job().unwrap().is_none());
        let blocked = w.processing_job(&next.id).unwrap();
        assert_eq!(blocked.state, ProcessingState::Blocked);
        assert_eq!(blocked.failure, Some(ProcessingFailure::InputUnavailable));
        assert!(blocked.lease.is_none());
        assert!(blocked.result_ids.is_empty());
        assert!(w.processing_job(&job.id).unwrap().result_ids.is_empty());
    }
    #[test]
    fn processing_input_refuses_evidence_key_retargeted_to_valid_other_original() {
        let (_temp, mut w, key) = fixture();
        let other = w
            .import("other.txt", b"Different synthetic source")
            .unwrap();
        let other_evidence: Evidence = get(&w.conn, "evidence", &other).unwrap();
        let fake = ProcessingInput::ParseDocument {
            evidence_id: key.clone(),
            sha256: other.clone(),
            bytes: other_evidence.bytes,
        };
        w.conn
            .execute(
                "UPDATE records SET body=? WHERE kind='evidence' AND id=?",
                params![serde_json::to_string(&other_evidence).unwrap(), key],
            )
            .unwrap();
        let revision = w.revision().unwrap();
        assert!(w.verify_processing_input(&fake).is_err());
        assert_eq!(w.revision().unwrap(), revision);
        assert_eq!(
            read_original(&w.root, &other_evidence).unwrap(),
            b"Different synthetic source"
        );
    }
    #[cfg(unix)]
    #[test]
    fn processing_claim_refuses_linked_original_even_when_target_bytes_match() {
        use std::os::unix::fs::symlink;
        let (temp, mut w, key) = fixture();
        let job = queue(&mut w, &key);
        let path = w.root.join("originals").join(&key);
        let outside = temp.path().join("outside");
        fs::write(&outside, b"Synthetic source only.").unwrap();
        fs::remove_file(&path).unwrap();
        symlink(&outside, &path).unwrap();
        assert!(w.claim_processing_job().unwrap().is_none());
        assert_eq!(
            w.processing_job(&job.id).unwrap().failure,
            Some(ProcessingFailure::InputUnavailable)
        );
        assert_eq!(fs::read(&outside).unwrap(), b"Synthetic source only.");
    }

    #[test]
    fn recovery_requires_exclusive_ownership_and_does_not_republish() {
        let (dir, mut workspace, evidence) = fixture();
        let job = queue(&mut workspace, &evidence);
        let ownership = workspace.lock_processing().unwrap();
        let prepared = workspace.claim_processing_job().unwrap().unwrap();
        let pending = queue(&mut workspace, &evidence);
        let mut another = Workspace::open(dir.path()).unwrap();
        assert!(another.lock_processing().is_err());
        assert_eq!(
            another.processing_job(&job.id).unwrap().state,
            ProcessingState::Running
        );
        drop(ownership);
        let _ownership = another.lock_processing().unwrap();
        assert_eq!(another.recover_processing_jobs().unwrap(), 1);
        assert_eq!(
            another.processing_job(&job.id).unwrap().failure,
            Some(ProcessingFailure::WorkerExitUnverified)
        );
        assert!(another
            .finish_document_job(&prepared.ticket, Ok(result(&prepared.bytes)))
            .is_err());
        assert!(another
            .retry_processing_job(&job.id, 1, "Unverified recovery retry")
            .is_err());
        assert!(another.claim_processing_job().is_err());
        assert!(another.queue_document_parse(&evidence, &id()).is_err());
        assert_eq!(
            another.processing_job(&pending.id).unwrap().failure,
            Some(ProcessingFailure::RecoveryRequired)
        );
        drop(another);
        let mut reopened = Workspace::open(dir.path()).unwrap();
        assert_eq!(reopened.recover_processing_jobs().unwrap(), 0);
        assert!(reopened
            .processing_job(&job.id)
            .unwrap()
            .result_ids
            .is_empty());
        assert!(reopened.claim_processing_job().is_err());
        assert_eq!(
            all::<ExtractionRecord>(&reopened.conn, "extraction")
                .unwrap()
                .len(),
            0
        );
    }
    #[test]
    fn bad_results_and_changed_originals_never_publish() {
        let (dir, mut workspace, evidence) = fixture();
        let job = queue(&mut workspace, &evidence);
        let prepared = workspace.claim_processing_job().unwrap().unwrap();
        let mut invalid = result(&prepared.bytes);
        invalid.content_sha256 = "0".repeat(64);
        assert_eq!(
            workspace
                .finish_document_job(&prepared.ticket, Ok(invalid))
                .unwrap()
                .failure,
            Some(ProcessingFailure::InvalidResult)
        );
        workspace
            .retry_processing_job(&job.id, 1, "Retry with verified synthetic input")
            .unwrap();
        let prepared = workspace.claim_processing_job().unwrap().unwrap();
        let input = workspace.processing_job(&job.id).unwrap().input;
        let (_, sha256, _) = input.source().unwrap();
        fs::rename(
            dir.path().join("originals").join(sha256),
            dir.path().join("scratch").join("removed-original"),
        )
        .unwrap();
        assert_eq!(
            workspace
                .finish_document_job(&prepared.ticket, Ok(result(&prepared.bytes)))
                .unwrap()
                .failure,
            Some(ProcessingFailure::InputUnavailable)
        );
        assert!(all::<ExtractionRecord>(&workspace.conn, "extraction")
            .unwrap()
            .is_empty());
    }
    #[test]
    fn backup_restores_jobs_derivatives_and_their_originals() {
        let (dir, mut workspace, evidence) = fixture();
        let job = queue(&mut workspace, &evidence);
        let prepared = workspace.claim_processing_job().unwrap().unwrap();
        let finished = workspace
            .finish_document_job(&prepared.ticket, Ok(result(&prepared.bytes)))
            .unwrap();
        let pending = queue(&mut workspace, &evidence);
        let backup = workspace.backup().unwrap();
        let restored = Workspace::restore(&backup, &dir.path().join("restored")).unwrap();
        assert_eq!(
            restored.processing_job(&job.id).unwrap().result_ids,
            finished.result_ids
        );
        assert_eq!(
            restored.processing_job(&pending.id).unwrap().state,
            ProcessingState::Queued
        );
        assert_eq!(
            restored
                .extraction(&finished.result_ids[0])
                .unwrap()
                .result
                .text,
            "An unreviewed derivative."
        );
        let source: Evidence = get(&restored.conn, "evidence", &evidence).unwrap();
        restored.verify_original(&source).unwrap();
    }
    #[test]
    fn pending_queue_is_bounded() {
        let (_dir, mut workspace, evidence) = fixture();
        for _ in 0..MAX_PENDING {
            queue(&mut workspace, &evidence);
        }
        assert!(workspace.queue_document_parse(&evidence, &id()).is_err());
        assert_eq!(
            workspace.processing_jobs().unwrap().total,
            MAX_PENDING as u64
        );
    }

    #[test]
    fn interrupted_cancellation_does_not_claim_verified_worker_exit() {
        let (_dir, mut workspace, evidence) = fixture();
        let job = queue(&mut workspace, &evidence);
        let _prepared = workspace.claim_processing_job().unwrap().unwrap();
        workspace.cancel_processing_job(&job.id, 1).unwrap();
        let _ownership = workspace.lock_processing().unwrap();
        workspace.recover_processing_jobs().unwrap();
        let recovered = workspace.processing_job(&job.id).unwrap();
        assert_eq!(recovered.state, ProcessingState::Failed);
        assert_eq!(
            recovered.failure,
            Some(ProcessingFailure::WorkerExitUnverified)
        );
        assert!(recovered.cancellation_requested);
        assert!(recovered.detail.contains("Worker exit is unverified"));
    }

    #[test]
    fn recovery_distinguishes_joined_shutdown_from_legacy_orphan_classification() {
        for legacy_orphan in [false, true] {
            let (_dir, mut workspace, evidence) = fixture();
            let job = queue(&mut workspace, &evidence);
            let prepared = workspace.claim_processing_job().unwrap().unwrap();
            let mut finished = workspace
                .finish_document_job(
                    &prepared.ticket,
                    Err(Error::Interrupted("Synthetic joined shutdown".into())),
                )
                .unwrap();
            if legacy_orphan {
                // Reproduce the previous development recovery representation,
                // whose cleared lease recorded no confirmed process termination.
                finished.lease = None;
                workspace
                    .change(None, "test.legacy_orphan", false, |conn| {
                        put(conn, "processing_job", &job.id, &finished)
                    })
                    .unwrap();
            }
            let _ownership = workspace.lock_processing().unwrap();
            assert_eq!(
                workspace.recover_processing_jobs().unwrap(),
                usize::from(legacy_orphan)
            );
            let retry = workspace.retry_processing_job(&job.id, 1, "Synthetic recovery review");
            assert_eq!(retry.is_ok(), !legacy_orphan);
            assert_eq!(workspace.claim_processing_job().is_ok(), !legacy_orphan);
        }
    }

    #[test]
    fn orphan_recovery_and_queue_suspension_commit_together() {
        let (_dir, mut workspace, evidence) = fixture();
        let running = queue(&mut workspace, &evidence);
        workspace.claim_processing_job().unwrap().unwrap();
        let queued = queue(&mut workspace, &evidence);
        let _ownership = workspace.lock_processing().unwrap();
        let revision = workspace.revision().unwrap();
        workspace.conn.execute_batch("CREATE TRIGGER reject_suspension BEFORE UPDATE ON records WHEN NEW.kind='processing_job' AND json_extract(NEW.body,'$.failure')='recovery_required' BEGIN SELECT RAISE(ABORT,'synthetic recovery failure'); END;").unwrap();
        assert!(workspace.recover_processing_jobs().is_err());
        assert_eq!(workspace.revision().unwrap(), revision);
        assert_eq!(
            workspace.processing_job(&running.id).unwrap().state,
            ProcessingState::Running
        );
        assert_eq!(
            workspace.processing_job(&queued.id).unwrap().state,
            ProcessingState::Queued
        );
        workspace
            .conn
            .execute_batch("DROP TRIGGER reject_suspension;")
            .unwrap();
        workspace.recover_processing_jobs().unwrap();
        assert_eq!(
            workspace.processing_job(&running.id).unwrap().failure,
            Some(ProcessingFailure::WorkerExitUnverified)
        );
        assert_eq!(
            workspace.processing_job(&queued.id).unwrap().failure,
            Some(ProcessingFailure::RecoveryRequired)
        );
    }

    #[test]
    fn cleanup_failure_survives_cancellation_and_missing_original() {
        for cancelled in [false, true] {
            let (dir, mut workspace, evidence) = fixture();
            let job = queue(&mut workspace, &evidence);
            let prepared = workspace.claim_processing_job().unwrap().unwrap();
            if cancelled {
                workspace.cancel_processing_job(&job.id, 1).unwrap();
            }
            let (_, sha256, _) = job.input.source().unwrap();
            fs::rename(
                dir.path().join("originals").join(sha256),
                dir.path().join("scratch").join("removed-original"),
            )
            .unwrap();
            let finished = workspace
                .finish_document_job(
                    &prepared.ticket,
                    Err(Error::Cleanup("Synthetic scratch failure".into())),
                )
                .unwrap();
            assert_eq!(finished.failure, Some(ProcessingFailure::CleanupFailed));
            assert_eq!(
                finished.state,
                if cancelled {
                    ProcessingState::Cancelled
                } else {
                    ProcessingState::Failed
                }
            );
            assert!(finished.result_ids.is_empty());
        }
    }

    #[test]
    fn unverified_exit_is_never_cancelled_and_suspends_further_execution() {
        for cancelled in [false, true] {
            let (dir, mut workspace, evidence) = fixture();
            let job = queue(&mut workspace, &evidence);
            let prepared = workspace.claim_processing_job().unwrap().unwrap();
            let pending = queue(&mut workspace, &evidence);
            if cancelled {
                workspace.cancel_processing_job(&job.id, 1).unwrap();
            }
            let finished = workspace
                .finish_document_job(
                    &prepared.ticket,
                    Err(Error::TerminationUnverified(
                        "Synthetic unknown exit".into(),
                    )),
                )
                .unwrap();
            assert_eq!(finished.state, ProcessingState::Failed);
            assert_eq!(
                finished.failure,
                Some(ProcessingFailure::WorkerExitUnverified)
            );
            assert!(finished.result_ids.is_empty());
            assert_eq!(
                workspace.processing_job(&pending.id).unwrap().state,
                ProcessingState::Blocked
            );
            assert_eq!(
                workspace.processing_job(&pending.id).unwrap().failure,
                Some(ProcessingFailure::RecoveryRequired)
            );
            assert!(workspace.queue_document_parse(&evidence, &id()).is_err());
            assert!(workspace
                .retry_processing_job(&job.id, 1, "Unsafe retry")
                .is_err());
            assert!(workspace.claim_processing_job().is_err());
            drop(workspace);
            let mut reopened = Workspace::open(dir.path()).unwrap();
            assert!(
                reopened.claim_processing_job().is_err(),
                "Restart must not erase unknown process state"
            );
        }
    }
}

#[cfg(debug_assertions)]
#[path = "processing_demo.rs"]
mod demo;

#[cfg(any(test, debug_assertions))]
#[path = "processing_image_fixtures.rs"]
mod image_fixtures;

#[cfg(debug_assertions)]
#[path = "processing_image_demo.rs"]
mod image_demo;

#[cfg(test)]
#[path = "processing_image_tests.rs"]
mod image_tests;

#[cfg(debug_assertions)]
#[path = "processing_pdf_demo.rs"]
mod pdf_demo;
#[cfg(any(test, debug_assertions))]
#[path = "processing_pdf_fixtures.rs"]
mod pdf_fixtures;
#[cfg(test)]
#[path = "processing_pdf_tests.rs"]
mod pdf_tests;

#[cfg(test)]
#[path = "processing_region_tests.rs"]
mod region_tests;

#[cfg(debug_assertions)]
#[path = "processing_region_demo.rs"]
mod region_demo;

#[cfg(test)]
#[path = "processing_graph_tests.rs"]
mod graph_tests;
