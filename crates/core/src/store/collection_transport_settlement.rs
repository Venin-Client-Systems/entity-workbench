use super::*;
use crate::{
    collection_settlement::TransportReceipt,
    collection_transport::{Observation, Outcome},
};

impl Workspace {
    pub(super) fn collection_transport_available(&self) -> Result<()> {
        let quarantined: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM records WHERE kind='collection_run' AND json_extract(body,'$.checkpoint.state')='recovery_required')",
            [], |row| row.get(0))?;
        require(
            !quarantined,
            "Collection transport requires verified process recovery",
        )
    }
    pub(crate) fn collection_execution_snapshot(
        &self,
        ticket: &CollectionTicket,
        owner: &CollectionOwnership,
    ) -> Result<DurableCollectionJob> {
        self.collection_owner(owner)?;
        self.collection_transport_available()?;
        let loaded = self.load_collection(&ticket.job_id)?;
        require(
            loaded.job.schema_version == 2 && loaded.job.synthetic,
            "Transport driver requires synthetic v2 run",
        )?;
        check_ticket(&loaded.job, ticket)?;
        Ok(loaded.job)
    }
    pub(crate) fn settle_collection_transport(
        &mut self,
        request: &RequestTicket,
        observation: &Observation,
        owner: &CollectionOwnership,
    ) -> Result<DurableCollectionJob> {
        self.collection_publication_owner(owner)?;
        let mut loaded = self.load_collection(&request.run.job_id)?;
        require(
            loaded.job.schema_version == 2 && loaded.job.synthetic,
            "Transport settlement requires synthetic v2 run",
        )?;
        let charged = loaded
            .job
            .checkpoint
            .requests
            .get(request.sequence as usize)
            .ok_or_else(|| Error::Validation("No charged request for transport settlement".into()))?
            .clone();
        require(
            charged.generation == request.run.generation
                && charged.lease == request.run.lease
                && charged.sequence == request.sequence
                && charged.entry.url == request.url,
            "Transport settlement ticket changed",
        )?;
        let receipt = TransportReceipt::from_observation(observation);
        let body = match &observation.outcome {
            Outcome::Complete { body, .. } => Some(body.as_slice()),
            _ => None,
        };
        receipt.validate(&request.url, &loaded.job.input, body)?;
        if let RequestProgress::Observed { receipt: previous } = &charged.progress {
            require(
                previous == &receipt,
                "Conflicting transport settlement replay",
            )?;
            return Ok(loaded.job);
        }
        check_ticket(&loaded.job, &request.run)?;
        let event = CollectionEvent::TransportObserved {
            clock_anchor_ms: loaded.job.checkpoint.updated_at_ms,
            sequence: request.sequence,
            receipt: receipt.clone(),
        };
        let promotion = append(&mut loaded, event, body)?;
        let evidence = self.retain_collection_response(
            &loaded.job.id,
            &request.url,
            receipt.observed_wall_ms,
            &receipt.fetch_record(),
            body,
        )?;
        self.publish_collection(&loaded, evidence, promotion)?;
        Ok(loaded.job)
    }
}

#[cfg(test)]
#[path = "collection_transport_settlement_tests.rs"]
mod tests;
