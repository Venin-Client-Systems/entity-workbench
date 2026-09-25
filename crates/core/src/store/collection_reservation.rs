//! One consumed, replay-verified v4 state authorizes at most one new reservation.
use super::replay::{capture_run, CapturedRun, PreparedRun};
use super::*;

pub(crate) struct CollectionReservationCapture {
    run: CapturedRun,
    execution: CollectionTicket,
}
pub(crate) struct PreparedCollectionReservation {
    run: PreparedRun,
    execution: CollectionTicket,
}
impl CollectionReservationCapture {
    pub(crate) fn prepare(self) -> Result<PreparedCollectionReservation> {
        Ok(PreparedCollectionReservation {
            run: self.run.prepare()?,
            execution: self.execution,
        })
    }
}
impl Workspace {
    pub(crate) fn capture_collection_reservation(
        &self,
        execution: &CollectionTicket,
        owner: &CollectionOwnership,
        protocol: CollectionProtocol,
        input: &CollectionInput,
        deadline: i64,
    ) -> Result<CollectionReservationCapture> {
        self.collection_owner(owner)?;
        self.collection_transport_available()?;
        require(
            protocol.version() == 4,
            "Prepared reservation requires attached v4 policy",
        )?;
        let run = capture_run(self, &execution.job_id, owner)?
            .ok_or_else(|| Error::Validation("Attached collection protocol changed".into()))?;
        require(
            protocol.matches(&run.job),
            "Attached collection mode or policy changed",
        )?;
        check_ticket(&run.job, execution)?;
        require(
            run.job.input == *input && run.job.checkpoint.deadline_at_ms == Some(deadline),
            "Collection driver input or first deadline changed",
        )?;
        Ok(CollectionReservationCapture {
            run,
            execution: execution.clone(),
        })
    }
    pub(crate) fn reserve_prepared_collection(
        &mut self,
        prepared: PreparedCollectionReservation,
        execution: &CollectionTicket,
        owner: &CollectionOwnership,
        cancelled: bool,
        at_ms: i64,
    ) -> Result<Option<RequestTicket>> {
        // A held quarantine permits settlement only, never another reservation.
        self.collection_owner(owner)?;
        require(
            prepared.execution == *execution,
            "Prepared execution ticket changed",
        )?;
        let (mut loaded, _) = prepared.run.revalidate(self, owner)?;
        self.collection_transport_available()?;
        check_ticket(&loaded.job, execution)?;
        if cancelled && !loaded.job.checkpoint.cancellation_requested {
            // Preserve the existing separate Cancel write and strict new-event
            // time validation. Never clamp a fresh reservation/control timestamp.
            append(&mut loaded, CollectionEvent::Cancel { at_ms }, None)?;
            self.collection_owner(owner)?;
            self.publish_collection(&loaded, None, None)?;
            // change() committed exactly one revision. Do not adopt a different
            // writer's revision for the subsequent Advance publication.
            loaded.revision = loaded
                .revision
                .checked_add(1)
                .ok_or_else(|| Error::Validation("Collection revision overflow".into()))?;
        }
        append(&mut loaded, CollectionEvent::Advance { at_ms }, None)?;
        self.collection_owner(owner)?;
        self.publish_collection(&loaded, None, None)?;
        Ok(loaded
            .job
            .checkpoint
            .requests
            .last()
            .filter(|r| matches!(r.progress, RequestProgress::Reserved))
            .map(|r| RequestTicket {
                run: execution.clone(),
                sequence: r.sequence,
                url: r.entry.url.clone(),
            }))
    }
}

#[cfg(test)]
#[path = "collection_reservation_tests.rs"]
mod tests;
