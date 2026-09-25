//! Private coordinator arbitration; canonical sequence is shared across both queues.
use super::*;
use crate::collection_jobs::CollectionProtocol;

impl Workspace {
    pub(crate) fn processing_queue_head(&self) -> Result<Option<(u64, ProcessingJob)>> {
        let row: Option<(u64, String, String)> = self.conn.query_row(
            "SELECT sequence,id,body FROM records WHERE kind='processing_job' AND json_extract(body,'$.state')='queued' ORDER BY sequence LIMIT 1",
            [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        row.map(|(sequence, key, body)| {
            let job: ProcessingJob = serde_json::from_str(&body)?;
            supported_job(&job)?;
            require(job.id == key, "Queued processing identity differs")?;
            Ok((sequence, job))
        })
        .transpose()
    }
    pub(crate) fn collection_queue_sequence(
        &self,
        protocol: CollectionProtocol,
    ) -> Result<Option<u64>> {
        Ok(self.conn.query_row(
            "SELECT sequence FROM records WHERE kind='collection_run' AND json_extract(body,'$.schema_version')=? AND json_extract(body,'$.synthetic')=? AND json_extract(body,'$.checkpoint.state')='queued' ORDER BY sequence LIMIT 1",
            params![protocol.version(),protocol.synthetic()], |r|r.get(0)).optional()?)
    }
    pub(crate) fn block_graph_before_claim(
        &mut self,
        expected: &ProcessingJob,
        detail: &str,
    ) -> Result<()> {
        let revision = self.revision()?;
        let mut job = self.processing_job(&expected.id)?;
        require(
            serde_json::to_vec(&job)? == serde_json::to_vec(expected)?
                && matches!(job.input, ProcessingInput::ShortestConnectionPath { .. })
                && job.state == ProcessingState::Queued,
            "Queued graph identity changed",
        )?;
        terminal(
            &mut job,
            ProcessingState::Blocked,
            Some(ProcessingFailure::SchedulingOrRuntimeUnavailable),
            detail,
        );
        self.change(
            Some(revision),
            "processing.graph_admission_blocked",
            false,
            |conn| put(conn, "processing_job", &job.id, &job),
        )
    }
}
