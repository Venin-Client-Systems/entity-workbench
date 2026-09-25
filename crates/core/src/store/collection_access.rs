//! Canonical synthetic fixture prefix, separate from ordinary collection readers.
//! Only the cfg(test) queue admits records. No coordinator or transport caller.
#![allow(dead_code)]
use super::collection_jobs::{acquired_evidence, prepare_collection_evidence, CollectionOwnership};
use super::*;
#[cfg(test)]
use crate::collection_profile::ValidatedPublisherAccessPlan;
use crate::{
    collection_access::{self as access, *},
    collection_access_machine::{self as machine, Machine},
    collection_jobs::{FetchRecord, RequestProgress, MAX_RECORD_BYTES},
    collection_machine::{self, Promotion},
    collection_settlement::TransportReceipt,
    collection_transport::{Observation, Outcome},
};
use std::collections::BTreeMap;

struct Loaded {
    job: Job,
    machine: Machine,
    revision: u64,
}
#[derive(Debug)]
pub(crate) struct AccessReview {
    pub revision: u64,
    pub review: Review,
}
impl Workspace {
    /// Fresh fixture admission only. This symbol does not exist in a production build.
    #[cfg(test)]
    pub(crate) fn queue_access_experiment(
        &mut self,
        profile: ValidatedPublisherAccessPlan,
        request_key: &str,
        owner: &CollectionOwnership,
        at_ms: i64,
    ) -> Result<Job> {
        self.collection_publication_owner(owner)?;
        require(
            crate::collection_jobs::canonical_uuid(request_key),
            "Invalid access request key",
        )?;
        let mapped: Option<String> = self.conn.query_row(
            "SELECT CASE WHEN length(CAST(body AS BLOB))<=80 THEN body ELSE 'null' END FROM records WHERE kind=? AND id=?",
            params![KEY_KIND, request_key], |r| r.get(0)).optional()?;
        if let Some(raw) = mapped {
            let key: String = serde_json::from_str(&raw)?;
            let loaded = self.load_access(&key)?;
            require(
                loaded.job.request_key == request_key
                    && loaded.job.plan_input == *profile.input()
                    && loaded.job.plan_sha256 == profile.plan_sha256(),
                "Access key belongs to another plan",
            )?;
            return Ok(loaded.job);
        }
        self.collection_owner(owner)?;
        self.collection_transport_available()?;
        collection_machine::valid_time(at_ms)?;
        let tx = self.conn.unchecked_transaction()?;
        let revision = tx.query_row("SELECT revision FROM meta", [], |r| r.get(0))?;
        let records: u64 = tx.query_row("SELECT count(*) FROM records", [], |r| r.get(0))?;
        require(
            records == 0 && revision == 0,
            "Experimental prefix requires a fresh synthetic workspace",
        )?;
        drop(tx);
        let machine = Machine::new(profile.clone(), at_ms, false)?;
        let job = Job {
            schema_version: 5,
            id: id(),
            request_key: request_key.into(),
            collector_policy: POLICY.into(),
            synthetic: true,
            plan_input: profile.input().clone(),
            plan_sha256: profile.plan_sha256().into(),
            created_at_ms: at_ms,
            events: Vec::new(),
            checkpoint: machine.checkpoint,
        };
        job.bounded()?;
        self.change(
            Some(revision),
            "collection.access_experiment.queue",
            false,
            |conn| {
                put(conn, KEY_KIND, request_key, &job.id)?;
                put(conn, KIND, &job.id, &job)
            },
        )?;
        Ok(job)
    }
    pub(crate) fn inspect_access_experiment(&self, key: &str) -> Result<Job> {
        Ok(self.load_access(key)?.job)
    }
    pub(crate) fn review_access_experiment(&self, key: &str) -> Result<AccessReview> {
        let loaded = self.load_access(key)?;
        Ok(AccessReview {
            revision: loaded.revision,
            review: access::review(&loaded.job)?,
        })
    }
    pub(crate) fn start_access_experiment(
        &mut self,
        key: &str,
        generation: u32,
        owner: &CollectionOwnership,
        at_ms: i64,
    ) -> Result<(Job, Option<Execution>)> {
        self.collection_owner(owner)?;
        self.collection_transport_available()?;
        let mut loaded = self.load_access(key)?;
        require(
            loaded.job.checkpoint.generation == generation,
            "Access generation changed",
        )?;
        let event = if loaded.job.checkpoint.state == State::Queued {
            Event::Start { at_ms, lease: id() }
        } else {
            Event::Resume { at_ms, lease: id() }
        };
        machine::append(&mut loaded.job, &mut loaded.machine, event, None, false)?;
        self.publish_access(&loaded, None, None)?;
        let execution = loaded.job.checkpoint.lease.as_ref().map(|lease| Execution {
            run_id: loaded.job.id.clone(),
            generation: loaded.job.checkpoint.generation,
            lease: lease.clone(),
            ownership_lifetime: owner.lifetime().into(),
        });
        Ok((loaded.job, execution))
    }
    pub(crate) fn advance_access_experiment(
        &mut self,
        execution_ticket: &Execution,
        owner: &CollectionOwnership,
        at_ms: i64,
    ) -> Result<Option<Ticket>> {
        self.collection_owner(owner)?;
        self.collection_transport_available()?;
        require(
            execution_ticket.ownership_lifetime == owner.lifetime(),
            "Access execution ownership changed",
        )?;
        let mut loaded = self.load_access(&execution_ticket.run_id)?;
        execution(
            &loaded.job,
            execution_ticket.generation,
            &execution_ticket.lease,
        )?;
        machine::append(
            &mut loaded.job,
            &mut loaded.machine,
            Event::Advance { at_ms },
            None,
            false,
        )?;
        self.publish_access(&loaded, None, None)?;
        Ok(loaded
            .job
            .checkpoint
            .requests
            .last()
            .filter(|r| r.progress == RequestProgress::Reserved)
            .map(|r| Ticket {
                ownership_lifetime: owner.lifetime().into(),
                run_id: loaded.job.id.clone(),
                generation: r.generation,
                lease: r.lease.clone(),
                sequence: r.sequence,
                url: r.url.clone(),
                purpose: r.purpose.clone(),
                plan_sha256: loaded.job.plan_sha256.clone(),
            }))
    }
    pub(crate) fn settle_access_experiment(
        &mut self,
        ticket: &Ticket,
        observation: &Observation,
        owner: &CollectionOwnership,
    ) -> Result<Job> {
        self.collection_publication_owner(owner)?;
        require(
            ticket.ownership_lifetime == owner.lifetime(),
            "Access settlement ownership changed",
        )?;
        if !observation.locally_quiescent {
            // Known uncertainty belongs to this lifetime even when subsequent
            // bounds, replay or receipt validation fails. Retain publication authority.
            owner.quarantine();
        }
        observation_bounds(observation)?;
        let mut loaded = self.load_access(&ticket.run_id)?;
        let request = request(&loaded.job, ticket)?.clone();
        let receipt = TransportReceipt::from_observation(observation);
        let body = match &observation.outcome {
            Outcome::Complete { body, .. } => Some(body.as_slice()),
            _ => None,
        };
        if let RequestProgress::Observed { receipt: previous } = &request.progress {
            require(*previous == receipt, "Conflicting access settlement retry")?;
            // Exact body verification also applies to an acknowledged retry.
            collection_machine::validate_fetch(&receipt.fetch_record(), body)?;
            return Ok(loaded.job);
        }
        execution(&loaded.job, ticket.generation, &ticket.lease)?;
        let event = Event::Observed {
            clock_anchor_ms: loaded.job.checkpoint.updated_at_ms,
            sequence: ticket.sequence,
            receipt: receipt.clone(),
        };
        let promotion = machine::append(&mut loaded.job, &mut loaded.machine, event, body, false)?;
        let evidence = self.retain_collection_response(
            &loaded.job.id,
            &ticket.url,
            receipt.observed_wall_ms,
            &receipt.fetch_record(),
            body,
        )?;
        self.publish_access(&loaded, evidence, promotion)?;
        Ok(loaded.job)
    }
    pub(crate) fn decide_access_experiment(
        &mut self,
        key: &str,
        expected_revision: u64,
        decision: Decision,
        owner: &CollectionOwnership,
        at_ms: i64,
    ) -> Result<Job> {
        self.collection_publication_owner(owner)?;
        decision.validate()?;
        let mut loaded = self.load_access(key)?;
        if let Some(previous) = &loaded.job.checkpoint.decision {
            require(*previous == decision, "Conflicting access decision retry")?;
            return Ok(loaded.job);
        }
        self.collection_owner(owner)?;
        self.collection_transport_available()?;
        if loaded.revision != expected_revision {
            return Err(Error::Conflict("Access review revision changed".into()));
        }
        machine::append(
            &mut loaded.job,
            &mut loaded.machine,
            Event::Decide { at_ms, decision },
            None,
            false,
        )?;
        self.publish_access(&loaded, None, None)?;
        Ok(loaded.job)
    }
    pub(crate) fn cancel_access_experiment(
        &mut self,
        key: &str,
        generation: u32,
        owner: &CollectionOwnership,
        at_ms: i64,
    ) -> Result<Job> {
        self.collection_publication_owner(owner)?;
        let mut loaded = self.load_access(key)?;
        require(
            loaded.job.checkpoint.generation == generation,
            "Access generation changed",
        )?;
        machine::append(
            &mut loaded.job,
            &mut loaded.machine,
            Event::Cancel { at_ms },
            None,
            false,
        )?;
        self.publish_access(&loaded, None, None)?;
        Ok(loaded.job)
    }
    /// Owned stop at the validated journal anchor, only for the exact reservation.
    pub(crate) fn stop_access_reservation(
        &mut self,
        ticket: &Ticket,
        owner: &CollectionOwnership,
    ) -> Result<Job> {
        self.collection_publication_owner(owner)?;
        require(
            ticket.ownership_lifetime == owner.lifetime(),
            "Access settlement ownership changed",
        )?;
        let mut loaded = self.load_access(&ticket.run_id)?;
        execution(&loaded.job, ticket.generation, &ticket.lease)?;
        require(
            request(&loaded.job, ticket)?.progress == RequestProgress::Reserved,
            "Anchored access stop requires a reservation",
        )?;
        if loaded.job.checkpoint.cancellation_requested {
            return Ok(loaded.job);
        }
        let event = Event::Cancel {
            at_ms: loaded.job.checkpoint.updated_at_ms,
        };
        // No externally supplied timestamp is accepted by this narrow path.
        machine::append(&mut loaded.job, &mut loaded.machine, event, None, true)?;
        self.publish_access(&loaded, None, None)?;
        Ok(loaded.job)
    }
    pub(crate) fn recover_access_experiment(
        &mut self,
        key: &str,
        owner: &CollectionOwnership,
        at_ms: i64,
    ) -> Result<Job> {
        self.collection_owner(owner)?;
        let mut loaded = self.load_access(key)?;
        machine::append(
            &mut loaded.job,
            &mut loaded.machine,
            Event::Recover { at_ms },
            None,
            false,
        )?;
        self.publish_access(&loaded, None, None)?;
        Ok(loaded.job)
    }
    fn publish_access(
        &mut self,
        loaded: &Loaded,
        mut evidence: Option<Evidence>,
        promotion: Option<Promotion>,
    ) -> Result<()> {
        loaded.job.bounded()?;
        prepare_collection_evidence(&mut evidence, promotion.as_ref())?;
        self.change(
            Some(loaded.revision),
            "collection.access_experiment.checkpoint",
            promotion.is_some(),
            |conn| {
                if let Some(evidence) = &evidence {
                    put(conn, "evidence", &evidence.id, evidence)?;
                }
                put(conn, KIND, &loaded.job.id, &loaded.job)
            },
        )
    }
    fn load_access(&self, key: &str) -> Result<Loaded> {
        require(
            crate::collection_jobs::canonical_uuid(key),
            "Invalid experimental run identifier",
        )?;
        let tx = self.conn.unchecked_transaction()?;
        let revision = tx.query_row("SELECT revision FROM meta", [], |r| r.get(0))?;
        let raw: Option<String> = tx.query_row("SELECT CASE WHEN length(CAST(body AS BLOB))<=? THEN body ELSE NULL END FROM records WHERE kind=? AND id=?",
            params![MAX_RECORD_BYTES, KIND, key], |r| r.get(0))?;
        let job: Job =
            serde_json::from_str(&raw.ok_or_else(|| {
                Error::Validation("Experimental run exceeds record limit".into())
            })?)?;
        require(job.id == key, "Experimental run key/body mismatch")?;
        job.profile()?;
        let mapped: String = tx.query_row("SELECT CASE WHEN length(CAST(body AS BLOB))<=80 THEN body ELSE 'null' END FROM records WHERE kind=? AND id=?", params![KEY_KIND, job.request_key], |r| r.get(0))?;
        require(
            serde_json::from_str::<String>(&mapped)? == key,
            "Experimental request key changed",
        )?;
        let mut expected = BTreeMap::<String, Evidence>::new();
        for event in &job.events {
            if let Event::Observed {
                sequence, receipt, ..
            } = event
            {
                if let FetchRecord::Complete { sha256, bytes, .. } = receipt.fetch_record() {
                    require(
                        bytes <= crate::collection::PAGE_BYTES as u64
                            && chrono::DateTime::from_timestamp_millis(receipt.observed_wall_ms)
                                .is_some()
                            && sha256.len() == 64
                            && sha256
                                .bytes()
                                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                        "Invalid experimental original identity",
                    )?;
                    let r = job
                        .checkpoint
                        .requests
                        .get(*sequence as usize)
                        .ok_or_else(|| Error::Validation("Missing access reservation".into()))?;
                    let next = acquired_evidence(
                        expected.remove(&sha256),
                        key,
                        &r.url,
                        receipt.observed_wall_ms,
                        &sha256,
                        bytes,
                    )?;
                    expected.insert(sha256, next);
                }
            }
        }
        require(
            expected.len() <= 50,
            "Experimental original count exceeds bound",
        )?;
        // The private fixture workspace cannot silently acquire a foreign corpus.
        let records: u64 = tx.query_row("SELECT count(*) FROM records", [], |r| r.get(0))?;
        require(
            records == expected.len() as u64 + 2,
            "Experimental workspace contains foreign records",
        )?;
        let mut metadata_bytes = 0u64;
        for sha in expected.keys() {
            let bytes: u64 = tx.query_row(
                "SELECT length(CAST(body AS BLOB)) FROM records WHERE kind='evidence' AND id=?",
                [sha],
                |r| r.get(0),
            )?;
            metadata_bytes = metadata_bytes
                .checked_add(bytes)
                .ok_or_else(|| Error::Validation("Access metadata overflow".into()))?;
            require(
                bytes <= MAX_RECORD_BYTES as u64 && metadata_bytes <= 16 * 1024 * 1024,
                "Experimental evidence metadata exceeds bound",
            )?;
        }
        let mut actual = BTreeMap::new();
        for (sha, reference) in &expected {
            let evidence = get_evidence(&tx, sha)?;
            require(
                evidence.sha256 == *sha && evidence.bytes == reference.bytes,
                "Experimental evidence differs from original",
            )?;
            actual.insert(sha.clone(), evidence);
        }
        let (machine, promotion) =
            machine::replay(&job, |_, receipt| match receipt.fetch_record() {
                FetchRecord::Complete { sha256, .. } => {
                    Ok(Some(read_original(&self.root, &actual[&sha256])?))
                }
                _ => Ok(None),
            })?;
        if let Some(promotion) = &promotion {
            let mut evidence = expected.remove(&promotion.sha256);
            prepare_collection_evidence(&mut evidence, Some(promotion))?;
            expected.insert(
                promotion.sha256.clone(),
                evidence.expect("promotion original"),
            );
        }
        for (sha, evidence) in expected {
            require(
                serde_json::to_vec(&evidence)? == serde_json::to_vec(&actual[&sha])?,
                "Experimental source text or acquisition provenance changed",
            )?;
        }
        Ok(Loaded {
            job,
            machine,
            revision,
        })
    }
}
/// Bound owned copies and hashing before converting a borrowed synthetic observation.
/// Semantic validation remains in the shared receipt validator.
fn observation_bounds(observation: &Observation) -> Result<()> {
    let head = match &observation.outcome {
        Outcome::Complete { head, body } => {
            require(
                body.len() <= crate::collection::PAGE_BYTES,
                "Experimental response exceeds body limit",
            )?;
            Some(head)
        }
        Outcome::Stopped { head, .. } => head.as_ref(),
    };
    if let Some(head) = head {
        require(
            head.media_type.as_ref().is_none_or(|s| s.len() <= 128)
                && head.redirect_url.as_ref().is_none_or(|s| s.len() <= 2048),
            "Experimental response metadata exceeds bound",
        )?;
    }
    require(
        observation
            .resolved
            .as_ref()
            .is_none_or(|r| r.addresses.len() <= 64 && r.method.len() <= 128)
            && observation
                .resolver_uncertainty
                .is_none_or(|r| r.method.len() <= 128),
        "Experimental resolver metadata exceeds bound",
    )
}
fn execution(job: &Job, generation: u32, lease: &str) -> Result<()> {
    require(
        job.checkpoint.state == State::Running
            && job.checkpoint.generation == generation
            && job.checkpoint.lease.as_deref() == Some(lease),
        "Experimental execution lease changed",
    )
}
fn request<'a>(job: &'a Job, ticket: &Ticket) -> Result<&'a Request> {
    let request = job
        .checkpoint
        .requests
        .get(ticket.sequence as usize)
        .ok_or_else(|| Error::Validation("Unknown experimental request".into()))?;
    require(
        job.id == ticket.run_id
            && job.plan_sha256 == ticket.plan_sha256
            && request.sequence == ticket.sequence
            && request.generation == ticket.generation
            && request.lease == ticket.lease
            && request.url == ticket.url
            && request.purpose == ticket.purpose,
        "Experimental request binding changed",
    )?;
    Ok(request)
}
#[cfg(test)]
#[path = "collection_access_tests.rs"]
mod tests;
