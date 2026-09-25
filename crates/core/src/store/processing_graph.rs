//! Fixed graph job settlement. This module exposes no worker-supplied publication capability.
use super::super::graph_analysis::{self, GraphAttempt};
use super::*;
use crate::graph_jobs::*;

fn graph_endpoints(conn: &Connection, source: &str, target: &str) -> Result<()> {
    require(source != target, "Graph endpoints must differ")?;
    for key in [source, target] {
        require(
            !key.is_empty()
                && key.len() <= 128
                && key.trim() == key
                && !key.chars().any(char::is_control),
            "Invalid graph endpoint",
        )?;
        let active: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM records WHERE kind='entity' AND id=? AND json_extract(body,'$.merged_into') IS NULL)", [key], |row|row.get(0))?;
        require(active, "Graph endpoint is missing or merged")?;
    }
    Ok(())
}
impl Workspace {
    pub(crate) fn queue_graph_path(
        &mut self,
        expected_revision: u64,
        source: &str,
        target: &str,
        request_key: &str,
    ) -> Result<ProcessingJob> {
        require(
            Uuid::parse_str(request_key).is_ok_and(|key| key.to_string() == request_key),
            "A canonical UUID request key is required",
        )?;
        let existing: Option<String> = self
            .conn
            .query_row(
                "SELECT body FROM records WHERE kind='processing_request' AND id=?",
                [request_key],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(existing) = existing {
            let mapped_id = serde_json::from_str::<String>(&existing)?;
            let job = self.processing_job(&mapped_id)?;
            require(
                job.id == mapped_id
                    && job.request_key == request_key
                    && matches!(&job.input, ProcessingInput::ShortestConnectionPath {source_id,target_id,requested_revision,..} if source_id==source && target_id==target && *requested_revision==expected_revision),
                "Request key already belongs to another input",
            )?;
            return Ok(job);
        }
        let queued = expected_revision
            .checked_add(1)
            .ok_or_else(|| Error::Validation("Graph revision overflow".into()))?;
        let at = now();
        let job = ProcessingJob {
            schema_version: 5,
            id: id(),
            request_key: request_key.into(),
            input: ProcessingInput::ShortestConnectionPath {
                source_id: source.into(),
                target_id: target.into(),
                requested_revision: expected_revision,
                queued_revision: queued,
            },
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
            detail: "Queued; automatic graph execution remains unavailable pending exclusive scheduling and runtime integration".into(),
            result_ids: vec![],
            lease: None,
        };
        self.change(
            Some(expected_revision),
            "processing.graph_queue",
            false,
            |conn| {
                require_verified_worker_exit(conn)?;
                require(
                    pending_count(conn)? < MAX_PENDING,
                    "Processing queue is full",
                )?;
                graph_endpoints(conn, source, target)?;
                put(conn, "processing_request", request_key, &job.id)?;
                put(conn, "processing_job", &job.id, &job)
            },
        )?;
        Ok(job)
    }
    pub(super) fn verify_graph_endpoints(&self, input: &ProcessingInput) -> Result<()> {
        let ProcessingInput::ShortestConnectionPath {
            source_id,
            target_id,
            ..
        } = input
        else {
            return Err(Error::Validation("Graph operation mismatch".into()));
        };
        graph_endpoints(&self.conn, source_id, target_id)
    }
    pub(crate) fn finish_graph_processing_job(
        &mut self,
        ticket: &JobTicket,
        attempt_owner: &mut GraphAttempt,
        result: Result<&ProcessingOutput>,
    ) -> Result<ProcessingJob> {
        require(
            attempt_owner.owns(ticket),
            "Graph attempt belongs to another claim",
        )?;
        let expected = self.revision()?;
        let mut job = self.processing_job(&ticket.job_id)?;
        attempt(&job, ticket.attempt)?;
        require(
            attempt_owner.owns_job(&job),
            "Stale or unowned graph attempt",
        )?;
        if job.state != ProcessingState::Running {
            if job.state == ProcessingState::Completed {
                if let Ok(ProcessingOutput::Graph(bytes)) = result {
                    if let Some(key) = job.result_ids.last() {
                        let record = self.graph_analysis_record(key)?;
                        if record.job_id == ticket.job_id
                            && record.host_attempt_lease == ticket.lease
                            && record.attempt == ticket.attempt
                            && record.result_sha256 == hash(bytes)
                        {
                            attempt_owner.terminal_committed();
                            return Ok(job);
                        }
                    }
                }
            }
            return Err(Error::Conflict("Graph attempt already finished".into()));
        }
        let root = self.root.clone();
        let owner = self.graph_capture_owner;
        let cancelled = job.cancellation_requested;
        self.change(Some(expected), "processing.graph_finish", false, |conn| {
            let mut record = None;
            match &result {
                Err(Error::TerminationUnverified(_)) => terminal(
                    &mut job, ProcessingState::Failed, Some(ProcessingFailure::WorkerExitUnverified),
                    "Worker exit unverified; assignments retained and execution suspended",
                ),
                Err(Error::Cleanup(_)) => terminal(
                    &mut job, if cancelled { ProcessingState::Cancelled } else { ProcessingState::Failed },
                    Some(ProcessingFailure::CleanupFailed),
                    "Worker stopped but scratch cleanup failed; no graph result published",
                ),
                _ if job.cancellation_requested => terminal(
                    &mut job, ProcessingState::Cancelled, Some(ProcessingFailure::CancelledByAnalyst),
                    "Worker stopped after cancellation; no graph result published",
                ),
                Ok(ProcessingOutput::Graph(bytes)) => match attempt_owner.record(&root, owner, conn, ticket, bytes) {
                    Ok(value) => {
                        job.result_ids.push(value.id.clone());
                        record = Some(value);
                        terminal(&mut job, ProcessingState::Completed, None,
                            "Validated immutable graph snapshot; no facts accepted");
                    }
                    // Roll back every write and retain the same owned attempt. Never truncate.
                    Err(error @ (Error::QuotaExhausted(_) | Error::Database(_))) => return Err(error),
                    Err(Error::Conflict(_)) => terminal(
                        &mut job, ProcessingState::Blocked, Some(ProcessingFailure::StaleGraphCapture),
                        "Captured revision or provenance changed; no automatic rebase or engine retry",
                    ),
                    Err(Error::InvalidWorkerResult(_)) => terminal(
                        &mut job, ProcessingState::Failed, Some(ProcessingFailure::InvalidResult),
                        "Graph worker result failed exact canonical validation",
                    ),
                    Err(_) => terminal(
                        &mut job, ProcessingState::Blocked, Some(ProcessingFailure::InputUnavailable),
                        "Captured original or provenance is unavailable; no graph result published",
                    ),
                },
                Ok(_) => terminal(
                    &mut job, ProcessingState::Failed, Some(ProcessingFailure::InvalidResult),
                    "Result does not match the fixed graph operation",
                ),
                Err(Error::Blocked(_)) => terminal(
                    &mut job, ProcessingState::Blocked, Some(ProcessingFailure::RuntimeUnavailable),
                    "A verified application-local confined Python graph runtime is unavailable",
                ),
                Err(Error::Interrupted(_)) => terminal(
                    &mut job, ProcessingState::Failed, Some(ProcessingFailure::Interrupted),
                    "Coordinator confirmed the worker stopped; manual retry is required",
                ),
                Err(Error::QuotaExhausted(_)) => terminal(
                    &mut job, ProcessingState::QuotaExhausted, Some(ProcessingFailure::WorkerFailed),
                    "Graph worker resource limit exhausted",
                ),
                Err(Error::InvalidWorkerResult(_)) => terminal(
                    &mut job, ProcessingState::Failed, Some(ProcessingFailure::InvalidResult),
                    "Graph worker result failed its protocol or assignment binding",
                ),
                Err(_) => terminal(
                    &mut job, ProcessingState::Failed, Some(ProcessingFailure::WorkerFailed),
                    "Graph worker failed; no result published",
                ),
            }
            if let Some(record) = record {
                put(conn, "graph_analysis", &record.id, &record)?;
            }
            if job.failure == Some(ProcessingFailure::WorkerExitUnverified) {
                suspend_queued_jobs(conn)?;
            }
            put(conn, "processing_job", &job.id, &job)
        })?;
        attempt_owner.terminal_committed();
        Ok(job)
    }
    pub(crate) fn graph_analysis_record(&self, key: &str) -> Result<GraphAnalysisRecord> {
        let mut statement = self
            .conn
            .prepare("SELECT body FROM records WHERE kind='graph_analysis' AND id=?")?;
        let mut rows = statement.query([key])?;
        let row = rows
            .next()?
            .ok_or_else(|| Error::Validation("Unknown graph analysis".into()))?;
        let raw = row
            .get_ref(0)?
            .as_str()
            .map_err(|_| Error::Validation("Invalid graph analysis storage".into()))?;
        require(
            raw.len() <= MAX_GRAPH_RECORD_BYTES,
            "Graph analysis record exceeds byte bound",
        )?;
        let record: GraphAnalysisRecord = serde_json::from_str(raw)?;
        require(
            record.schema_version == 1
                && record.id == key
                && graph_analysis::record_identity(&record)? == key
                && record.requested_revision < record.queued_revision
                && record.queued_revision < record.captured_revision
                && record.published_revision
                    == record
                        .captured_revision
                        .checked_add(1)
                        .ok_or_else(|| Error::Validation("Graph revision overflow".into()))?
                && record.request_sha256 == hash(record.request_json.as_bytes())
                && record.result_sha256 == hash(record.result_json.as_bytes()),
            "Graph analysis immutable identity differs",
        )?;
        Ok(record)
    }
    pub(crate) fn inspect_graph_analysis(&self, key: &str) -> Result<GraphAnalysisInspection> {
        let transaction = self.conn.unchecked_transaction()?;
        let revision = self.revision()?;
        let record = self.graph_analysis_record(key)?;
        let job = self.processing_job(&record.job_id)?;
        require(
            job.id == record.job_id
                && job.request_key == record.request_key
                && matches!(&job.input, ProcessingInput::ShortestConnectionPath {source_id,target_id,requested_revision,queued_revision} if source_id==&record.source_id && target_id==&record.target_id && requested_revision==&record.requested_revision && (job.attempt>record.attempt || *queued_revision==record.queued_revision))
                && (job.attempt > record.attempt
                    || job.lease.as_deref() == Some(&record.host_attempt_lease))
                && job.result_ids.contains(&record.id)
                && job.attempt >= record.attempt
                && revision >= record.published_revision,
            "Graph analysis canonical linkage differs",
        )?;
        // Frozen values remain readable after normal canonical corrections. Verify retained bytes,
        // never require today's mutable entity/observation JSON to equal historical fingerprints.
        let integrity = if record
            .frozen
            .evidence
            .iter()
            .all(|evidence| read_original(&self.root, evidence).is_ok())
        {
            GraphOriginalIntegrity::Verified
        } else {
            GraphOriginalIntegrity::Unavailable
        };
        transaction.commit()?;
        let published_revision = record.published_revision;
        Ok(GraphAnalysisInspection {
            record,
            compared_revision: revision,
            freshness: if integrity == GraphOriginalIntegrity::Unavailable {
                None
            } else {
                Some(if revision == published_revision {
                    GraphFreshness::CurrentAtPublication
                } else {
                    GraphFreshness::WorkspaceAdvanced
                })
            },
            original_integrity: integrity,
        })
    }
}

// Exclusive coordinator claim: Running, actual C, event and capture commit together.
// The coordinator holds workspace and admission ownership before this private call.
impl Workspace {
    pub(crate) fn claim_graph_exclusive(
        &mut self,
        expected_job: &ProcessingJob,
    ) -> Result<(JobTicket, GraphAttempt)> {
        require_verified_worker_exit(&self.conn)?;
        let expected = self.revision()?;
        let mut job = self.processing_job(&expected_job.id)?;
        require(
            serde_json::to_vec(&job)? == serde_json::to_vec(expected_job)?,
            "Graph queued identity changed",
        )?;
        supported_job(&job)?;
        require(
            job.state == ProcessingState::Queued && !job.cancellation_requested,
            "Graph exclusive claim must be queued",
        )?;
        let ticket = JobTicket {
            job_id: job.id.clone(),
            attempt: job.attempt,
            lease: id(),
        };
        let root = self.root.clone();
        let owner = self.graph_capture_owner;
        let conn = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current: u64 = conn.query_row("SELECT revision FROM meta", [], |row| row.get(0))?;
        require(
            current == expected,
            "Graph exclusive claim revision changed",
        )?;
        let captured_revision = current
            .checked_add(1)
            .ok_or_else(|| Error::Validation("Graph revision overflow".into()))?;
        job.state = ProcessingState::Running;
        job.lease = Some(ticket.lease.clone());
        job.started_at = Some(now());
        job.updated_at = now();
        job.detail = "Exclusive graph claim; fixed executor owns the attempt".into();
        put(&conn, "processing_job", &job.id, &job)?;
        conn.execute("UPDATE meta SET revision=?", [captured_revision])?;
        conn.execute(
            "INSERT INTO events(revision,action,at) VALUES(?,?,?)",
            params![captured_revision, "processing.graph_claim", now()],
        )?;
        let attempt =
            GraphAttempt::capture_claim(&root, owner, &conn, &job, &ticket, captured_revision)?;
        conn.commit()?;
        Ok((ticket, attempt))
    }
}

#[cfg(test)]
impl Workspace {
    pub(super) fn claim_graph_for_publication_test(
        &mut self,
        key: &str,
    ) -> Result<(JobTicket, GraphAttempt)> {
        let job = self.processing_job(key)?;
        self.claim_graph_exclusive(&job)
    }
}
