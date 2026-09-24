use super::*;
use crate::{
    engines::parser::{validate_result, ParseResult, ParseStatus},
    processing::*,
};
use std::fs::File;

const MAX_PENDING: usize = 64;
const MAX_ATTEMPTS: u32 = 3;
const PAGE_LIMIT: u32 = 200;

fn attempt(job: &ProcessingJob, expected: u32) -> Result<()> {
    if job.attempt != expected {
        return Err(Error::Conflict(
            "Job attempt changed; reload before applying this action".into(),
        ));
    }
    Ok(())
}

fn terminal(
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
fn pending_count(conn: &Connection) -> Result<usize> {
    Ok(conn.query_row("SELECT count(*) FROM records WHERE kind='processing_job' AND json_extract(body,'$.state') IN ('queued','running')", [], |row| row.get(0))?)
}

fn require_verified_worker_exit(conn: &Connection) -> Result<()> {
    let unverified: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM records WHERE kind='processing_job' AND json_extract(body,'$.failure')='worker_exit_unverified')", [], |row| row.get(0))?;
    if unverified {
        return Err(Error::Blocked("Document execution is suspended because a previous worker's exit is unverified; verified process recovery is required".into()));
    }
    Ok(())
}

impl Workspace {
    pub fn queue_document_parse(
        &mut self,
        evidence_id: &str,
        request_key: &str,
    ) -> Result<ProcessingJob> {
        require(
            Uuid::parse_str(request_key).is_ok_and(|key| key.to_string() == request_key),
            "A canonical UUID request key is required",
        )?;
        let expected = self.revision()?;
        let evidence: Evidence = get(&self.conn, "evidence", evidence_id)?;
        let input = ProcessingInput::ParseDocument {
            evidence_id: evidence.id.clone(),
            sha256: evidence.sha256.clone(),
            bytes: evidence.bytes,
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
            schema_version: 1,
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
            detail: "Queued for local document parsing".into(),
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
        get(&self.conn, "processing_job", job_id)
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
            .map(|body| Ok(serde_json::from_str(&body?)?))
            .collect::<Result<Vec<_>>>()?;
        Ok(ProcessingJobPage {
            jobs,
            total,
            limit: PAGE_LIMIT,
        })
    }

    pub fn extraction(&self, extraction_id: &str) -> Result<ExtractionRecord> {
        get(&self.conn, "extraction", extraction_id)
    }

    pub fn cancel_processing_job(
        &mut self,
        job_id: &str,
        expected_attempt: u32,
    ) -> Result<ProcessingJob> {
        let expected = self.revision()?;
        let mut job = self.processing_job(job_id)?;
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
        self.verify_processing_input(&job.input)?;
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

    fn verify_processing_input(&self, input: &ProcessingInput) -> Result<Vec<u8>> {
        let ProcessingInput::ParseDocument {
            evidence_id,
            sha256,
            bytes,
        } = input;
        let evidence: Evidence = get(&self.conn, "evidence", evidence_id)?;
        require(
            evidence.sha256 == *sha256
                && evidence.bytes == *bytes
                && *bytes <= policy::MAX_IMPORT_BYTES as u64,
            "Job input no longer matches the retained evidence",
        )?;
        self.verify_original(&evidence)?;
        // Check the bytes actually passed to the worker too, not only an earlier filesystem read.
        let content = fs::read(self.root.join("originals").join(sha256))?;
        require(
            content.len() as u64 == *bytes && hash(&content) == *sha256,
            "Job input changed while reading",
        )?;
        Ok(content)
    }

    pub(crate) fn claim_document_job(&mut self) -> Result<Option<PreparedDocumentJob>> {
        require_verified_worker_exit(&self.conn)?;
        let expected = self.revision()?;
        let body: Option<String> = self.conn.query_row("SELECT body FROM records WHERE kind='processing_job' AND json_extract(body,'$.state')='queued' ORDER BY sequence LIMIT 1", [], |row| row.get(0)).optional()?;
        let Some(body) = body else {
            return Ok(None);
        };
        let mut job: ProcessingJob = serde_json::from_str(&body)?;
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
        job.detail = "Local document worker is running".into();
        self.change(Some(expected), "processing.claim", false, |conn| {
            require_verified_worker_exit(conn)?;
            put(conn, "processing_job", &job.id, &job)
        })?;
        Ok(Some(PreparedDocumentJob {
            ticket: JobTicket {
                job_id: job.id,
                attempt: job.attempt,
                lease,
            },
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
        let jobs: Vec<_> = jobs
            .into_iter()
            .filter(|job| job.state == ProcessingState::Running)
            .collect();
        if jobs.is_empty() {
            return Ok(0);
        }
        let count = jobs.len();
        self.change(Some(expected), "processing.recover", false, |conn| {
            for mut job in jobs {
                if job.cancellation_requested {
                    terminal(&mut job, ProcessingState::Failed, Some(ProcessingFailure::Interrupted), "Previous coordinator stopped after cancellation was requested. Worker exit is unverified; no result was published. An explicit manual retry is required");
                } else {
                    terminal(&mut job, ProcessingState::Failed, Some(ProcessingFailure::Interrupted), "Previous coordinator stopped before publishing a result; an explicit manual retry is required");
                }
                job.lease = None;
                put(conn, "processing_job", &job.id, &job)?;
            }
            Ok(())
        })?;
        Ok(count)
    }

    /// Only the local coordinator has a claim ticket. Worker output is validated before publication.
    pub(crate) fn finish_document_job(
        &mut self,
        ticket: &JobTicket,
        result: Result<ParseResult>,
    ) -> Result<ProcessingJob> {
        let expected = self.revision()?;
        let mut job = self.processing_job(&ticket.job_id)?;
        attempt(&job, ticket.attempt)?;
        require(
            job.lease.as_deref() == Some(&ticket.lease),
            "Stale or unowned processing attempt",
        )?;
        if job.state != ProcessingState::Running {
            // A completion may be delivered again after acknowledgement is lost. Never publish twice.
            if let Ok(result) = &result {
                let result_hash = hash(&serde_json::to_vec(result)?);
                if let Some(key) = job.result_ids.last() {
                    let previous = self.extraction(key)?;
                    if previous.attempt == ticket.attempt && previous.result_sha256 == result_hash {
                        return Ok(job);
                    }
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
        } else if self.verify_processing_input(&job.input).is_err() {
            terminal(
                &mut job,
                ProcessingState::Blocked,
                Some(ProcessingFailure::InputUnavailable),
                "Retained input failed verification; the result was not published",
            );
        } else {
            match result {
                Ok(result) => {
                    let ProcessingInput::ParseDocument { sha256, bytes, .. } = &job.input;
                    if validate_result(&result, sha256, *bytes).is_err() {
                        terminal(&mut job, ProcessingState::Failed, Some(ProcessingFailure::InvalidResult), "Worker result failed canonical validation; no derivative was published");
                    } else {
                        let result_hash = hash(&serde_json::to_vec(&result)?);
                        let key = hash(format!("{}:{}", job.id, job.attempt).as_bytes());
                        match result.status {
                            ParseStatus::Complete => terminal(&mut job, ProcessingState::Completed, None, "Parsing completed; extraction awaits analyst review"),
                            ParseStatus::Partial => terminal(&mut job, ProcessingState::Partial, None, "Partial extraction retained with explicit limitations; review is required"),
                            ParseStatus::Unsupported => terminal(&mut job, ProcessingState::Blocked, Some(ProcessingFailure::UnsupportedFormat), "The packaged parser does not support this document"),
                            ParseStatus::Failed => terminal(&mut job, ProcessingState::Failed, Some(ProcessingFailure::DocumentFailed), "Document parsing failed; inspect the typed failure in its extraction record"),
                        }
                        extraction = Some(ExtractionRecord {
                            schema_version: 1,
                            id: key.clone(),
                            job_id: job.id.clone(),
                            attempt: job.attempt,
                            input: job.input.clone(),
                            created_at: now(),
                            result_sha256: result_hash,
                            result,
                        });
                        job.result_ids.push(key);
                    }
                }
                Err(Error::Cleanup(_)) => terminal(
                    &mut job,
                    ProcessingState::Failed,
                    Some(ProcessingFailure::CleanupFailed),
                    "Worker scratch cleanup failed; no derivative was published",
                ),
                Err(Error::Interrupted(_)) => terminal(
                    &mut job,
                    ProcessingState::Failed,
                    Some(ProcessingFailure::Interrupted),
                    "Coordinator stopped the worker; an explicit manual retry is required",
                ),
                Err(Error::Blocked(_)) => terminal(
                    &mut job,
                    ProcessingState::Blocked,
                    Some(ProcessingFailure::RuntimeUnavailable),
                    "The required confined local runtime is unavailable",
                ),
                Err(Error::QuotaExhausted(_)) => terminal(
                    &mut job,
                    ProcessingState::QuotaExhausted,
                    Some(ProcessingFailure::WorkerFailed),
                    "Worker resource limit exhausted",
                ),
                Err(_) => terminal(
                    &mut job,
                    ProcessingState::Failed,
                    Some(ProcessingFailure::WorkerFailed),
                    "Worker failed; no derivative was published",
                ),
            }
        }
        self.change(Some(expected), "processing.finish", false, |conn| {
            if job.failure == Some(ProcessingFailure::WorkerExitUnverified) {
                for mut queued in all::<ProcessingJob>(conn, "processing_job")? {
                    if queued.state == ProcessingState::Queued {
                        terminal(&mut queued, ProcessingState::Blocked, Some(ProcessingFailure::RecoveryRequired), "An earlier worker's exit is unverified. Document execution is suspended until verified process recovery");
                        put(conn, "processing_job", &queued.id, &queued)?;
                    }
                }
            }
            if let Some(record) = &extraction {
                put(conn, "extraction", &record.id, record)?;
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
        let prepared = workspace.claim_document_job().unwrap().unwrap();
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
        let prepared = workspace.claim_document_job().unwrap().unwrap();
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
        let next = workspace.claim_document_job().unwrap().unwrap();
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
        assert!(workspace.claim_document_job().unwrap().is_none());
        let job = queue(&mut workspace, &evidence);
        for attempt in 1..=3 {
            let prepared = workspace.claim_document_job().unwrap().unwrap();
            assert_eq!(prepared.ticket.attempt, attempt);
            let failed = workspace
                .finish_document_job(
                    &prepared.ticket,
                    Err(Error::Validation("Synthetic worker failure".into())),
                )
                .unwrap();
            assert_eq!(failed.state, ProcessingState::Failed);
            assert!(workspace.claim_document_job().unwrap().is_none());
            let retry = workspace.retry_processing_job(&job.id, attempt, "Retry synthetic job");
            assert_eq!(retry.is_ok(), attempt < 3);
        }
    }
    #[test]
    fn recovery_requires_exclusive_ownership_and_does_not_republish() {
        let (dir, mut workspace, evidence) = fixture();
        let job = queue(&mut workspace, &evidence);
        let ownership = workspace.lock_processing().unwrap();
        let prepared = workspace.claim_document_job().unwrap().unwrap();
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
            Some(ProcessingFailure::Interrupted)
        );
        assert!(another
            .finish_document_job(&prepared.ticket, Ok(result(&prepared.bytes)))
            .is_err());
        another
            .retry_processing_job(&job.id, 1, "Recover interrupted synthetic job")
            .unwrap();
        let retry = another.claim_document_job().unwrap().unwrap();
        let done = another
            .finish_document_job(&retry.ticket, Ok(result(&retry.bytes)))
            .unwrap();
        drop(another);
        let mut reopened = Workspace::open(dir.path()).unwrap();
        assert_eq!(reopened.recover_processing_jobs().unwrap(), 0);
        assert_eq!(
            reopened.processing_job(&job.id).unwrap().result_ids,
            done.result_ids
        );
        assert_eq!(
            all::<ExtractionRecord>(&reopened.conn, "extraction")
                .unwrap()
                .len(),
            1
        );
    }
    #[test]
    fn bad_results_and_changed_originals_never_publish() {
        let (dir, mut workspace, evidence) = fixture();
        let job = queue(&mut workspace, &evidence);
        let prepared = workspace.claim_document_job().unwrap().unwrap();
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
        let prepared = workspace.claim_document_job().unwrap().unwrap();
        let ProcessingInput::ParseDocument { sha256, .. } =
            workspace.processing_job(&job.id).unwrap().input;
        fs::rename(
            dir.path().join("originals").join(&sha256),
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
        let prepared = workspace.claim_document_job().unwrap().unwrap();
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
        let _prepared = workspace.claim_document_job().unwrap().unwrap();
        workspace.cancel_processing_job(&job.id, 1).unwrap();
        let _ownership = workspace.lock_processing().unwrap();
        workspace.recover_processing_jobs().unwrap();
        let recovered = workspace.processing_job(&job.id).unwrap();
        assert_eq!(recovered.state, ProcessingState::Failed);
        assert_eq!(recovered.failure, Some(ProcessingFailure::Interrupted));
        assert!(recovered.cancellation_requested);
        assert!(recovered.detail.contains("Worker exit is unverified"));
    }

    #[test]
    fn cleanup_failure_survives_cancellation_and_missing_original() {
        for cancelled in [false, true] {
            let (dir, mut workspace, evidence) = fixture();
            let job = queue(&mut workspace, &evidence);
            let prepared = workspace.claim_document_job().unwrap().unwrap();
            if cancelled {
                workspace.cancel_processing_job(&job.id, 1).unwrap();
            }
            let ProcessingInput::ParseDocument { sha256, .. } = job.input;
            fs::rename(
                dir.path().join("originals").join(&sha256),
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
            let prepared = workspace.claim_document_job().unwrap().unwrap();
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
            assert!(workspace.claim_document_job().is_err());
            drop(workspace);
            let mut reopened = Workspace::open(dir.path()).unwrap();
            assert!(
                reopened.claim_document_job().is_err(),
                "Restart must not erase unknown process state"
            );
        }
    }
}
