//! Private two-phase v4 settlement. No parser runs in capture or commit.
use super::replay::{capture_run, CapturedRun, PreparedRun};
use super::*;
use crate::{
    collection_settlement::TransportReceipt,
    collection_transport::{Observation, Outcome},
};

pub(crate) struct CollectionCapture {
    run: CapturedRun,
    request: RequestTicket,
    receipt: TransportReceipt,
}
/// Only replay/interpretation below can construct this consumed publication proof.
pub(crate) struct PreparedCollectionSettlement {
    run: PreparedRun,
    request: RequestTicket,
    receipt: TransportReceipt,
    after: Option<Loaded>,
    promotion: Option<Promotion>,
}
fn body(observation: &Observation) -> Option<&[u8]> {
    match &observation.outcome {
        Outcome::Complete { body, .. } => Some(body),
        _ => None,
    }
}
fn charged<'a>(
    job: &'a DurableCollectionJob,
    request: &RequestTicket,
) -> Result<&'a ChargedRequest> {
    let charged = job
        .checkpoint
        .requests
        .get(request.sequence as usize)
        .ok_or_else(|| Error::Validation("No charged request for prepared settlement".into()))?;
    require(
        charged.sequence == request.sequence
            && charged.generation == request.run.generation
            && charged.lease == request.run.lease
            && charged.entry.url == request.url,
        "Prepared settlement request identity changed",
    )?;
    Ok(charged)
}
impl Workspace {
    pub(crate) fn capture_collection_settlement(
        &self,
        request: &RequestTicket,
        observation: &Observation,
        owner: &CollectionOwnership,
    ) -> Result<Option<CollectionCapture>> {
        let Some(run) = capture_run(self, &request.run.job_id, owner)? else {
            return Ok(None);
        };
        let receipt = TransportReceipt::from_observation(observation);
        receipt.validate_mode(run.job.synthetic)?;
        receipt.validate(&request.url, &run.job.input, body(observation))?;
        charged(&run.job, request)?;
        Ok(Some(CollectionCapture {
            run,
            request: request.clone(),
            receipt,
        }))
    }

    pub(crate) fn commit_prepared_collection(
        &mut self,
        prepared: PreparedCollectionSettlement,
        request: &RequestTicket,
        observation: &Observation,
        owner: &CollectionOwnership,
        stop_requested: bool,
        at_ms: i64,
    ) -> Result<DurableCollectionJob> {
        self.collection_publication_owner(owner)?;
        let PreparedCollectionSettlement {
            run,
            request: captured_request,
            receipt,
            after,
            promotion,
        } = prepared;
        require(
            request == &captured_request
                && TransportReceipt::from_observation(observation) == receipt,
            "Prepared settlement request or receipt changed",
        )?;
        let (mut loaded, exact) = run.revalidate(self, owner)?;
        let revision = loaded.revision;
        if after.is_none() {
            // Exact idempotent replay: preparation verified the already retained
            // receipt. No write, new acquisition or charge is permitted.
            require(exact, "Already-settled capture changed")?;
            return Ok(loaded.job);
        }
        check_ticket(&loaded.job, &request.run)?;
        charged(&loaded.job, request)?;
        if stop_requested && !loaded.job.checkpoint.cancellation_requested {
            if at_ms < loaded.job.checkpoint.updated_at_ms {
                // Preserve the verified journal anchor after wall-clock rollback.
                // The raw observation remains unchanged in its transport receipt.
                append_anchored_cancel(&mut loaded)?;
            } else {
                append(&mut loaded, CollectionEvent::Cancel { at_ms }, None)?;
            }
        }
        let promotion = if loaded.job.checkpoint.cancellation_requested {
            // Existing receipt precedence suppresses interpretation/promotion.
            // This apply cannot enter static_page: cancel is already canonical.
            let event = CollectionEvent::TransportObserved {
                clock_anchor_ms: loaded.job.checkpoint.updated_at_ms,
                sequence: request.sequence,
                receipt: receipt.clone(),
            };
            let result = append(&mut loaded, event, body(observation))?;
            require(
                result.is_none(),
                "Cancelled preparation unexpectedly promoted text",
            )?;
            None
        } else {
            require(exact, "Unrecognized prepared run change")?;
            loaded = after.expect("non-idempotent prepared settlement");
            loaded.revision = revision;
            promotion
        };
        let evidence = self.retain_collection_response(
            &loaded.job.id,
            &request.url,
            receipt.observed_wall_ms,
            &receipt.fetch_record(),
            body(observation),
        )?;
        self.publish_collection(&loaded, evidence, promotion)?;
        Ok(loaded.job)
    }
}
impl CollectionCapture {
    pub(crate) fn prepare(self, observation: &Observation) -> Result<PreparedCollectionSettlement> {
        self.prepare_inner(observation, || {})
    }
    #[cfg(test)]
    pub(crate) fn prepare_with_hook(
        self,
        observation: &Observation,
        hook: impl FnOnce(),
    ) -> Result<PreparedCollectionSettlement> {
        self.prepare_inner(observation, hook)
    }
    fn prepare_inner(
        self,
        observation: &Observation,
        before_interpret: impl FnOnce(),
    ) -> Result<PreparedCollectionSettlement> {
        require(
            TransportReceipt::from_observation(observation) == self.receipt,
            "Captured receipt changed before interpretation",
        )?;
        let CollectionCapture {
            run,
            request: captured_request,
            receipt,
        } = self;
        let run = run.prepare()?;
        let request = charged(&run.capture.job, &captured_request)?;
        if let RequestProgress::Observed { receipt: previous } = &request.progress {
            require(previous == &receipt, "Conflicting prepared receipt replay")?;
            return Ok(PreparedCollectionSettlement {
                run,
                request: captured_request,
                receipt,
                after: None,
                promotion: None,
            });
        }
        check_ticket(&run.capture.job, &captured_request.run)?;
        require(
            matches!(request.progress, RequestProgress::Reserved),
            "Prepared request is not reserved",
        )?;
        before_interpret();
        let mut after = Loaded {
            job: run.capture.job.clone(),
            machine: run.machine.clone(),
            revision: run.capture.revision,
        };
        let event = CollectionEvent::TransportObserved {
            clock_anchor_ms: after.job.checkpoint.updated_at_ms,
            sequence: captured_request.sequence,
            receipt: receipt.clone(),
        };
        let promotion = append(&mut after, event, body(observation))?;
        Ok(PreparedCollectionSettlement {
            run,
            request: captured_request,
            receipt,
            after: Some(after),
            promotion,
        })
    }
}

#[cfg(test)]
#[path = "collection_preparation_tests.rs"]
mod tests;
