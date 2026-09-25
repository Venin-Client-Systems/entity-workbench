//! Owned publication attempt; borrowing authority never escapes this fixed graph lifecycle.
use super::*;
use crate::processing::ProcessingInput;
use crate::processing::ProcessingJob;
use crate::processing::ProcessingState;
use crate::{graph_jobs::*, processing::JobTicket};

pub(crate) struct GraphAttempt {
    captured: Option<CapturedGraph>,
    ticket: JobTicket,
    claim: ProcessingJob,
    requested_revision: u64,
    queued_revision: u64,
}
impl GraphAttempt {
    pub(in crate::store) fn capture_claim(
        root: &Path,
        owner: Uuid,
        conn: &Connection,
        job: &ProcessingJob,
        ticket: &JobTicket,
        revision: u64,
    ) -> Result<Self> {
        require(
            job.state == ProcessingState::Running
                && job.id == ticket.job_id
                && job.attempt == ticket.attempt
                && job.lease.as_deref() == Some(&ticket.lease),
            "Graph capture requires the canonical running claim",
        )?;
        let ProcessingInput::ShortestConnectionPath {
            source_id,
            target_id,
            requested_revision,
            queued_revision,
        } = &job.input
        else {
            return Err(Error::Validation("Graph capture operation mismatch".into()));
        };
        let captured = capture_in_transaction(root, owner, conn, revision, source_id, target_id)?;
        Ok(Self {
            captured: Some(captured),
            ticket: ticket.clone(),
            claim: job.clone(),
            requested_revision: *requested_revision,
            queued_revision: *queued_revision,
        })
    }
    pub(crate) fn worker_input(&self) -> &[u8] {
        self.captured
            .as_ref()
            .map_or(&[], CapturedGraph::worker_input)
    }
    pub(in crate::store) fn owns(&self, ticket: &JobTicket) -> bool {
        self.ticket.job_id == ticket.job_id
            && self.ticket.attempt == ticket.attempt
            && self.ticket.lease == ticket.lease
    }
    pub(in crate::store) fn owns_job(&self, job: &ProcessingJob) -> bool {
        job.schema_version == 5
            && job.id == self.claim.id
            && job.request_key == self.claim.request_key
            && job.input == self.claim.input
            && job.attempt == self.claim.attempt
            && job.lease == self.claim.lease
    }
    pub(in crate::store) fn terminal_committed(&mut self) {
        self.captured.take();
    }
    pub(in crate::store) fn record(
        &self,
        root: &Path,
        owner: Uuid,
        conn: &Connection,
        ticket: &JobTicket,
        bytes: &[u8],
    ) -> Result<GraphAnalysisRecord> {
        require(
            self.owns(ticket) && !conn.is_autocommit(),
            "Graph publication requires owned transaction",
        )?;
        let captured = self
            .captured
            .as_ref()
            .ok_or_else(|| Error::Conflict("Graph attempt already consumed".into()))?;
        let result = validate_borrowed(root, owner, conn, captured, bytes)?;
        let selection = &captured.selection;
        let edge = |(a, b): &(String, String), ids: &Vec<String>| GraphEdge {
            source_id: a.clone(),
            target_id: b.clone(),
            assertion_ids: ids.clone(),
        };
        let outcome = match result {
            ValidatedPath::Unreachable => GraphOutcome::Unreachable {},
            ValidatedPath::Path { nodes, hops } => GraphOutcome::Path {
                nodes,
                hops: hops
                    .into_iter()
                    .map(|hop| GraphEdge {
                        source_id: hop.from,
                        target_id: hop.to,
                        assertion_ids: hop.assertion_ids,
                    })
                    .collect(),
            },
        };
        let mut record = GraphAnalysisRecord {
            schema_version: 1,
            id: String::new(),
            job_id: ticket.job_id.clone(),
            request_key: self.claim.request_key.clone(),
            attempt: ticket.attempt,
            host_attempt_lease: self.ticket.lease.clone(),
            requested_revision: self.requested_revision,
            queued_revision: self.queued_revision,
            recipe: RECIPE.into(),
            policy: POLICY.into(),
            engine: ENGINE.into(),
            engine_version: ENGINE_VERSION.into(),
            runtime_manifest_sha256: RUNTIME.into(),
            source_id: captured.source_id.clone(),
            target_id: captured.target_id.clone(),
            captured_revision: captured.revision,
            published_revision: captured
                .revision
                .checked_add(1)
                .ok_or_else(|| Error::Validation("Graph revision overflow".into()))?,
            capture_nonce: captured.nonce.clone(),
            snapshot_sha256: captured.snapshot_sha256.clone(),
            request_sha256: hash(captured.worker_input()),
            result_sha256: hash(bytes),
            request_json: String::from_utf8(captured.worker_input().to_vec())
                .map_err(|_| invalid("Invalid captured encoding"))?,
            result_json: String::from_utf8(bytes.to_vec())
                .map_err(|_| invalid("Invalid result encoding"))?,
            frozen: FrozenGraph {
                assertion_reviews: GraphReviewCounts {
                    accepted: selection.assertion_reviews.accepted,
                    pending: selection.assertion_reviews.pending,
                    rejected: selection.assertion_reviews.rejected,
                    deferred: selection.assertion_reviews.deferred,
                },
                nodes: selection.nodes.iter().cloned().collect(),
                edges: selection
                    .edges
                    .iter()
                    .map(|(key, ids)| edge(key, ids))
                    .collect(),
                fingerprints: selection
                    .fingerprints
                    .iter()
                    .map(|((kind, id), sha256)| GraphFingerprint {
                        kind: kind.clone(),
                        id: id.clone(),
                        sha256: sha256.clone(),
                    })
                    .collect(),
                entities: selection.entities.values().cloned().collect(),
                assertions: selection.assertions.values().cloned().collect(),
                observations: selection.observations.values().cloned().collect(),
                evidence: selection
                    .evidence
                    .values()
                    .map(|value| value.frozen.clone())
                    .collect(),
            },
            outcome,
            limitation: LIMITATION.into(),
        };
        record.id = record_identity(&record)?;
        bounded_record(&record)?;
        Ok(record)
    }
}

pub(in crate::store) fn bounded_record(record: &GraphAnalysisRecord) -> Result<Vec<u8>> {
    bounded_json(record, MAX_GRAPH_RECORD_BYTES)
}
pub(in crate::store) fn record_identity(record: &GraphAnalysisRecord) -> Result<String> {
    let mut identity = record.clone();
    identity.id.clear();
    Ok(hash(&bounded_record(&identity)?))
}
