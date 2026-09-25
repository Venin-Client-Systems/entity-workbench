//! Private two-phase v4 settlement. No parser runs in capture or commit.
use super::*;
use crate::{
    collection_settlement::TransportReceipt,
    collection_transport::{Observation, Outcome},
};
use std::collections::BTreeMap;

const SOURCE_METADATA_BYTES: u64 = 16 * 1024 * 1024;
struct Source {
    evidence: Evidence,
    metadata_sha256: String,
}
/// Owned, bounded canonical input. No live SQLite transaction/connection escapes.
/// This type is neither deserializable nor cloneable.
pub(crate) struct CollectionCapture {
    root: PathBuf,
    ownership_lifetime: String,
    job: DurableCollectionJob,
    record_sha256: String,
    revision: u64,
    sources: BTreeMap<String, Source>,
    request: RequestTicket,
    receipt: TransportReceipt,
}
/// Only replay/interpretation below can construct this consumed publication proof.
pub(crate) struct PreparedCollectionSettlement {
    capture: CollectionCapture,
    before: Machine,
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
        self.collection_publication_owner(owner)?;
        let tx = self.conn.unchecked_transaction()?;
        let revision = tx.query_row("SELECT revision FROM meta", [], |row| row.get(0))?;
        let (job, raw) = read_job(&tx, &request.run.job_id)?;
        if job.schema_version != 4 {
            return Ok(None);
        }
        job.protocol()?;
        require(
            job.events.len() <= MAX_EVENTS
                && canonical_uuid(&job.id)
                && canonical_uuid(&job.request_key),
            "Malformed collection capture",
        )?;
        // Cheap validation only. Full canonical replay occurs after unlocking.
        Machine::new_version(&job.input, job.created_at_ms, 4, true)?;
        let receipt = TransportReceipt::from_observation(observation);
        receipt.validate_mode(job.synthetic)?;
        receipt.validate(&request.url, &job.input, body(observation))?;
        charged(&job, request)?;
        let mut references = BTreeMap::new();
        for event in &job.events {
            if let CollectionEvent::TransportObserved { receipt, .. } = event {
                if let FetchRecord::Complete { sha256, bytes, .. } = receipt.fetch_record() {
                    valid_response_reference(&sha256, bytes)?;
                    if let Some(previous) = references.insert(sha256, bytes) {
                        require(previous == bytes, "Conflicting captured original lengths")?;
                    }
                }
            }
        }
        require(
            references.len() <= 50,
            "Collection source capture exceeds request limit",
        )?;
        // Bound all metadata before copying any source body across SQLite.
        let mut metadata_bytes = 0u64;
        for key in references.keys() {
            let bytes: u64 = tx.query_row(
                "SELECT length(CAST(body AS BLOB)) FROM records WHERE kind='evidence' AND id=?",
                [key],
                |row| row.get(0),
            )?;
            require(
                bytes <= MAX_RECORD_BYTES as u64,
                "Response evidence metadata exceeds collection read bound",
            )?;
            metadata_bytes = metadata_bytes
                .checked_add(bytes)
                .ok_or_else(|| Error::Validation("Collection capture size overflow".into()))?;
            require(
                metadata_bytes <= SOURCE_METADATA_BYTES,
                "Collection preparation source metadata exceeds 16 MiB; pending receipt retained",
            )?;
        }
        let mut sources = BTreeMap::new();
        for (key, bytes) in references {
            let (raw, evidence) = bounded_evidence_raw(&tx, &key)?
                .ok_or_else(|| Error::Validation("Retained response evidence is missing".into()))?;
            require(
                evidence.id == key && evidence.sha256 == key && evidence.bytes == bytes,
                "Captured evidence identity changed",
            )?;
            sources.insert(
                key,
                Source {
                    evidence,
                    metadata_sha256: hash(raw.as_bytes()),
                },
            );
        }
        Ok(Some(CollectionCapture {
            root: self.root.clone(),
            ownership_lifetime: owner.lifetime().into(),
            job,
            record_sha256: hash(raw.as_bytes()),
            revision,
            sources,
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
            capture,
            before,
            after,
            promotion,
        } = prepared;
        require(
            self.root == capture.root
                && owner.lifetime() == capture.ownership_lifetime
                && request == &capture.request
                && TransportReceipt::from_observation(observation) == capture.receipt,
            "Prepared settlement ownership, workspace or receipt changed",
        )?;
        let tx = self.conn.unchecked_transaction()?;
        let revision = tx.query_row("SELECT revision FROM meta", [], |row| row.get(0))?;
        let (current, raw) = read_job(&tx, &request.run.job_id)?;
        // An unrelated canonical revision may advance. Every captured source row
        // must still be exact; originals are rehashed through the bounded reader.
        for (key, source) in &capture.sources {
            let (raw, _) = bounded_evidence_raw(&tx, key)?
                .ok_or_else(|| Error::Validation("Captured source disappeared".into()))?;
            require(
                hash(raw.as_bytes()) == source.metadata_sha256,
                "Captured source metadata changed; prepare settlement again",
            )?;
            read_original(&self.root, &source.evidence)?;
        }
        let exact = hash(raw.as_bytes()) == capture.record_sha256;
        let mut loaded = Loaded {
            job: capture.job,
            machine: before,
            revision,
        };
        if !exact {
            // The ONLY accepted run drift is one appended canonical Cancel.
            // Reapply the existing rule and require the WHOLE record to match;
            // no caller-controlled checkpoint/frontier or arbitrary suffix wins.
            require(
                current.events.len() == loaded.job.events.len() + 1
                    && current.events[..loaded.job.events.len()] == loaded.job.events,
                "Prepared collection run changed; prepare settlement again",
            )?;
            let event = current.events.last().expect("one appended event").clone();
            require(
                matches!(event, CollectionEvent::Cancel { .. }),
                "Prepared collection run has a non-cancellation suffix",
            )?;
            require(
                loaded.job.events.len() < MAX_EVENTS,
                "Collection event bound reached",
            )?;
            loaded.machine.replay_cancel_suffix(&event)?;
            loaded.job.events.push(event);
            loaded.job.checkpoint = loaded.machine.checkpoint.clone();
            bounded(&loaded.job)?;
            require(
                loaded.job == current,
                "Prepared cancellation checkpoint changed",
            )?;
        }
        drop(tx);
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
                receipt: capture.receipt.clone(),
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
            capture.receipt.observed_wall_ms,
            &capture.receipt.fetch_record(),
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
        let before = collection_machine::replay(&self.job, |request, result| {
            if let FetchRecord::Complete { sha256, bytes, .. } = result {
                let source = self.sources.get(sha256).ok_or_else(|| {
                    Error::Validation("Original absent from collection capture".into())
                })?;
                validate_acquisition(&self.job, request, sha256, *bytes, &source.evidence)?;
                // Read one original at a time; never retain the full body history.
                Ok(Some(read_original(&self.root, &source.evidence)?))
            } else {
                Ok(None)
            }
        })?;
        let request = charged(&self.job, &self.request)?;
        if let RequestProgress::Observed { receipt } = &request.progress {
            require(
                receipt == &self.receipt,
                "Conflicting prepared receipt replay",
            )?;
            return Ok(PreparedCollectionSettlement {
                capture: self,
                before,
                after: None,
                promotion: None,
            });
        }
        check_ticket(&self.job, &self.request.run)?;
        require(
            matches!(request.progress, RequestProgress::Reserved),
            "Prepared request is not reserved",
        )?;
        before_interpret();
        let mut after = Loaded {
            job: self.job.clone(),
            machine: before.clone(),
            revision: self.revision,
        };
        let event = CollectionEvent::TransportObserved {
            clock_anchor_ms: after.job.checkpoint.updated_at_ms,
            sequence: self.request.sequence,
            receipt: self.receipt.clone(),
        };
        let promotion = append(&mut after, event, body(observation))?;
        Ok(PreparedCollectionSettlement {
            capture: self,
            before,
            after: Some(after),
            promotion,
        })
    }
}

#[cfg(test)]
#[path = "collection_preparation_tests.rs"]
mod tests;
