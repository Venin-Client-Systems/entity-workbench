use super::*;
use crate::{collection_settlement::TransportReceipt, collection_transport::StopReason};

impl Machine {
    pub(super) fn observe_transport(
        &mut self,
        anchor: i64,
        sequence: u32,
        receipt: &TransportReceipt,
        body: Option<&[u8]>,
    ) -> Result<Option<Promotion>> {
        require(
            matches!(self.version, 2..=4) && anchor == self.checkpoint.updated_at_ms,
            "Transport observation has invalid version or clock anchor",
        )?;
        self.running()?;
        let request = self
            .checkpoint
            .requests
            .get(sequence as usize)
            .ok_or_else(|| {
                Error::Validation("No charged request for transport observation".into())
            })?
            .clone();
        require(
            request.sequence == sequence && matches!(request.progress, RequestProgress::Reserved),
            "Transport request is not reserved",
        )?;
        receipt.validate(&request.entry.url, &self.input, body)?;
        // Cancellation may have been journaled after the response was observed.
        // Compare the raw sample with its reservation, not that later event.
        let clock_changed = receipt.stopped_for(StopReason::ClockChanged)
            || receipt.observed_wall_ms < request.reserved_at_ms;
        let mut promotion = None;
        match receipt_stop_state(
            receipt,
            self.checkpoint.cancellation_requested,
            self.checkpoint.deadline_at_ms.expect("running deadline"),
            request.reserved_at_ms,
            self.input.max_seconds,
        ) {
            Some(CollectionState::RecoveryRequired) => {
                self.checkpoint.saw_unknown = true;
                self.finish(CollectionState::RecoveryRequired);
            }
            Some(CollectionState::Failed) => {
                self.checkpoint.saw_failed = true;
                self.finish(CollectionState::Failed);
            }
            Some(CollectionState::Cancelled) => {
                self.checkpoint.cancellation_requested = true;
                self.finish(CollectionState::Cancelled);
            }
            Some(CollectionState::QuotaExhausted) => {
                self.checkpoint.saw_quota = true;
                self.finish(CollectionState::QuotaExhausted);
            }
            Some(CollectionState::Blocked) => {
                self.checkpoint.saw_blocked = true;
                self.finish(CollectionState::Blocked);
            }
            Some(_) => unreachable!("closed transport stop classification"),
            None => {
                // Retain encoded bytes but never interpret them as robots or text.
                let result = if receipt.head().is_some_and(|head| !head.identity_encoding) {
                    FetchRecord::Failed {
                        reason: TransportFailure::Policy,
                    }
                } else {
                    receipt.fetch_record()
                };
                promotion = self.received(&request, &result, body, receipt.observed_wall_ms)?;
            }
        }
        self.checkpoint.requests[sequence as usize].progress = RequestProgress::Observed {
            receipt: receipt.clone(),
        };
        if !clock_changed {
            self.checkpoint.updated_at_ms = anchor.max(receipt.observed_wall_ms);
        }
        Ok(promotion)
    }
}
