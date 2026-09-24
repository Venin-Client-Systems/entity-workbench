//! Unactivated canonical durable-collection foundation. No public dispatch hooks.
#![allow(dead_code)]
use super::*;
use crate::{
    collection_jobs::*,
    collection_machine::{self, Machine, Promotion},
};
use std::fs::File;

/// Uses the existing coordinator ownership file. This development seam must not
/// become a second independent coordinator when live execution is integrated.
pub(crate) struct CollectionOwnership {
    root: PathBuf,
    file: File,
}
impl Drop for CollectionOwnership {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}
struct Loaded {
    job: DurableCollectionJob,
    machine: Machine,
    revision: u64,
}

impl Workspace {
    pub(crate) fn collection_ownership(&self) -> Result<CollectionOwnership> {
        Ok(CollectionOwnership {
            root: self.root.clone(),
            file: self.lock_processing()?,
        })
    }
    fn collection_owner(&self, owner: &CollectionOwnership) -> Result<()> {
        require(
            owner.root == self.root,
            "Collection ownership belongs to another workspace",
        )
    }
    pub(crate) fn queue_durable_collection(
        &mut self,
        input: CollectionInput,
        request_key: &str,
        at_ms: i64,
    ) -> Result<DurableCollectionJob> {
        require(
            canonical_uuid(request_key),
            "Canonical collection request UUID required",
        )?;
        let input = input.normalized()?;
        collection_machine::valid_time(at_ms)?;
        let tx = self.conn.unchecked_transaction()?;
        let revision: u64 = tx.query_row("SELECT revision FROM meta", [], |r| r.get(0))?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT CASE WHEN length(CAST(body AS BLOB))<=80 THEN body ELSE 'null' END FROM records WHERE kind='collection_run_key' AND id=?",
                [request_key],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(body) = existing {
            require(body.len() <= 80, "Malformed collection request mapping")?;
            let key: String = serde_json::from_str(&body)?;
            let loaded = load(&self.root, &tx, &key, revision)?;
            require(
                loaded.job.request_key == request_key && loaded.job.input == input,
                "Collection request key belongs to another input",
            )?;
            return Ok(loaded.job);
        }
        let pending: u64 = tx.query_row("SELECT count(*) FROM records WHERE kind='collection_run' AND json_extract(body,'$.checkpoint.state') IN ('queued','running','interrupted')", [], |r| r.get(0))?;
        require(pending < 8, "Durable collection pending-job limit reached")?;
        drop(tx);
        let machine = Machine::new(&input, at_ms)?;
        let job = DurableCollectionJob {
            schema_version: 1,
            id: id(),
            request_key: request_key.into(),
            collector_policy: "direct-https-durable-foundation-v1".into(),
            synthetic: true,
            input,
            created_at_ms: at_ms,
            events: Vec::new(),
            checkpoint: machine.checkpoint,
        };
        bounded(&job)?;
        self.change(Some(revision), "collection.durable.queue", false, |conn| {
            put(conn, "collection_run_key", request_key, &job.id)?;
            put(conn, "collection_run", &job.id, &job)
        })?;
        Ok(job)
    }
    fn load_collection(&self, job_id: &str) -> Result<Loaded> {
        require(
            canonical_uuid(job_id),
            "Invalid durable collection identifier",
        )?;
        let tx = self.conn.unchecked_transaction()?;
        let revision: u64 = tx.query_row("SELECT revision FROM meta", [], |r| r.get(0))?;
        load(&self.root, &tx, job_id, revision)
    }
    pub(crate) fn inspect_durable_collection(&self, job_id: &str) -> Result<DurableCollectionJob> {
        Ok(self.load_collection(job_id)?.job)
    }
    pub(crate) fn start_durable_collection(
        &mut self,
        job_id: &str,
        generation: u32,
        owner: &CollectionOwnership,
        at_ms: i64,
    ) -> Result<Option<CollectionTicket>> {
        self.collection_owner(owner)?;
        let mut loaded = self.load_collection(job_id)?;
        expected_generation(&loaded.job, generation)?;
        let running: bool = self.conn.query_row("SELECT EXISTS(SELECT 1 FROM records WHERE kind='collection_run' AND json_extract(body,'$.checkpoint.state')='running')", [], |r| r.get(0))?;
        require(!running, "Another durable collection is running")?;
        let event = match loaded.job.checkpoint.state {
            CollectionState::Queued => CollectionEvent::Start { at_ms, lease: id() },
            CollectionState::Interrupted => CollectionEvent::Resume { at_ms, lease: id() },
            _ => return Err(Error::Conflict("Collection cannot start or resume".into())),
        };
        append(&mut loaded, event, None)?;
        self.publish_collection(&loaded, None, None)?;
        // An expired resume is a committed quota outcome, not a new execution lease.
        Ok(loaded
            .job
            .checkpoint
            .lease
            .as_ref()
            .map(|lease| CollectionTicket {
                job_id: loaded.job.id.clone(),
                generation: loaded.job.checkpoint.generation,
                lease: lease.clone(),
            }))
    }
    pub(crate) fn advance_durable_collection(
        &mut self,
        execution: &CollectionTicket,
        owner: &CollectionOwnership,
        at_ms: i64,
    ) -> Result<Option<RequestTicket>> {
        self.collection_owner(owner)?;
        let mut loaded = self.load_collection(&execution.job_id)?;
        check_ticket(&loaded.job, execution)?;
        append(&mut loaded, CollectionEvent::Advance { at_ms }, None)?;
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
    pub(crate) fn complete_durable_collection(
        &mut self,
        request: &RequestTicket,
        response: &CollectionResponse,
        owner: &CollectionOwnership,
        at_ms: i64,
    ) -> Result<DurableCollectionJob> {
        self.collection_owner(owner)?;
        let mut loaded = self.load_collection(&request.run.job_id)?;
        let charged = loaded
            .job
            .checkpoint
            .requests
            .get(request.sequence as usize)
            .ok_or_else(|| Error::Validation("No charged collection request".into()))?
            .clone();
        require(
            charged.generation == request.run.generation
                && charged.lease == request.run.lease
                && charged.entry.url == request.url,
            "Completion ticket differs from charged request",
        )?;
        let (result, bytes) = response_record(&request.url, response)?;
        if let RequestProgress::Settled {
            ended_at_ms,
            result: previous,
        } = &charged.progress
        {
            require(
                previous == &result && *ended_at_ms == at_ms,
                "Conflicting collection completion replay",
            )?;
            return Ok(loaded.job);
        }
        check_ticket(&loaded.job, &request.run)?;
        let promotion = append(
            &mut loaded,
            CollectionEvent::Complete {
                at_ms,
                sequence: request.sequence,
                result: result.clone(),
            },
            bytes,
        )?;
        let evidence = if let FetchRecord::Complete {
            sha256,
            bytes: expected,
            ..
        } = &result
        {
            let existing = bounded_evidence(&self.conn, sha256)?;
            let mut evidence = existing.unwrap_or_else(|| Evidence {
                id: sha256.clone(),
                name: "http-response.bin".into(),
                sha256: sha256.clone(),
                bytes: *expected,
                media_type: "application/octet-stream".into(),
                origin_group: sha256.clone(),
                imported_at: stamp(at_ms),
                extraction_status: "acquisition_only".into(),
                text: None,
                acquisitions: Vec::new(),
            });
            require(
                evidence.id == *sha256 && evidence.sha256 == *sha256 && evidence.bytes == *expected,
                "Existing evidence differs from response identity",
            )?;
            retain_original(&self.root, &evidence, bytes.expect("complete response"))?;
            evidence.acquisitions.push(Acquisition {
                job_id: loaded.job.id.clone(),
                url: request.url.clone(),
                retrieved_at: stamp(at_ms),
            });
            Some(evidence)
        } else {
            None
        };
        self.publish_collection(&loaded, evidence, promotion)?;
        Ok(loaded.job)
    }
    pub(crate) fn cancel_durable_collection(
        &mut self,
        job_id: &str,
        generation: u32,
        at_ms: i64,
    ) -> Result<DurableCollectionJob> {
        let mut loaded = self.load_collection(job_id)?;
        expected_generation(&loaded.job, generation)?;
        if loaded.job.checkpoint.cancellation_requested {
            return Ok(loaded.job);
        }
        append(&mut loaded, CollectionEvent::Cancel { at_ms }, None)?;
        self.publish_collection(&loaded, None, None)?;
        Ok(loaded.job)
    }
    /// Internal quiescence acknowledgement. No production caller or stopped-transport claim yet.
    pub(crate) fn acknowledge_collection_stop(
        &mut self,
        execution: &CollectionTicket,
        owner: &CollectionOwnership,
        at_ms: i64,
    ) -> Result<DurableCollectionJob> {
        self.collection_owner(owner)?;
        let mut loaded = self.load_collection(&execution.job_id)?;
        check_ticket(&loaded.job, execution)?;
        append(
            &mut loaded,
            CollectionEvent::StopAcknowledged { at_ms },
            None,
        )?;
        self.publish_collection(&loaded, None, None)?;
        Ok(loaded.job)
    }
    pub(crate) fn recover_durable_collections(
        &mut self,
        owner: &CollectionOwnership,
        at_ms: i64,
    ) -> Result<usize> {
        self.collection_owner(owner)?;
        let keys: Vec<String> = self
            .conn
            .prepare("SELECT id FROM records WHERE kind='collection_run' AND json_extract(body,'$.checkpoint.state')='running' ORDER BY sequence LIMIT 9")?
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        require(
            keys.len() <= 8,
            "Durable collection pending-job limit is inconsistent",
        )?;
        let mut count = 0;
        for key in keys {
            let mut loaded = self.load_collection(&key)?;
            if loaded.job.checkpoint.state == CollectionState::Running {
                append(&mut loaded, CollectionEvent::Recover { at_ms }, None)?;
                self.publish_collection(&loaded, None, None)?;
                count += 1;
            }
        }
        Ok(count)
    }
    fn publish_collection(
        &mut self,
        loaded: &Loaded,
        mut evidence: Option<Evidence>,
        promotion: Option<Promotion>,
    ) -> Result<()> {
        bounded(&loaded.job)?;
        if let Some(promotion) = &promotion {
            let original = evidence
                .as_mut()
                .ok_or_else(|| Error::Validation("Derivative lacks acquired original".into()))?;
            require(
                original.id == promotion.sha256,
                "Derivative differs from acquired response",
            )?;
            original.text = Some(promotion.text.clone());
            original.media_type = promotion.media_type.clone();
            original.extraction_status = "static_text_only".into();
        }
        if let Some(original) = &evidence {
            require(
                serde_json::to_vec(original)?.len() <= MAX_RECORD_BYTES,
                "Response evidence metadata exceeds collection read bound",
            )?;
        }
        self.change(
            Some(loaded.revision),
            "collection.durable.checkpoint",
            promotion.is_some(),
            |conn| {
                if let Some(evidence) = &evidence {
                    put(conn, "evidence", &evidence.id, evidence)?;
                }
                put(conn, "collection_run", &loaded.job.id, &loaded.job)
            },
        )
    }
}
fn expected_generation(job: &DurableCollectionJob, generation: u32) -> Result<()> {
    if job.checkpoint.generation != generation {
        return Err(Error::Conflict("Collection generation changed".into()));
    }
    Ok(())
}
fn check_ticket(job: &DurableCollectionJob, execution: &CollectionTicket) -> Result<()> {
    expected_generation(job, execution.generation)?;
    require(
        job.id == execution.job_id
            && job.checkpoint.lease.as_ref() == Some(&execution.lease)
            && job.checkpoint.state == CollectionState::Running,
        "Collection execution lease is stale",
    )
}
fn append(
    loaded: &mut Loaded,
    event: CollectionEvent,
    body: Option<&[u8]>,
) -> Result<Option<Promotion>> {
    require(
        loaded.job.events.len() < MAX_EVENTS,
        "Collection event bound reached",
    )?;
    let promotion = loaded.machine.apply(&event, body)?;
    loaded.job.events.push(event);
    loaded.job.checkpoint = loaded.machine.checkpoint.clone();
    bounded(&loaded.job)?;
    Ok(promotion)
}
fn bounded(job: &DurableCollectionJob) -> Result<()> {
    require(
        serde_json::to_vec(job)?.len() <= MAX_RECORD_BYTES,
        "Durable collection record exceeds size bound",
    )
}
fn stamp(at_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(at_ms)
        .expect("validated time")
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
fn load(root: &Path, conn: &Connection, key: &str, revision: u64) -> Result<Loaded> {
    // Reject oversized bodies before copying them across the SQLite boundary.
    let body: Option<String> = conn.query_row("SELECT CASE WHEN length(CAST(body AS BLOB))<=? THEN body ELSE NULL END FROM records WHERE kind='collection_run' AND id=?", params![MAX_RECORD_BYTES, key], |r| r.get(0))?;
    let job: DurableCollectionJob = serde_json::from_str(&body.ok_or_else(|| {
        Error::Validation("Durable collection record exceeds size bound".into())
    })?)?;
    require(job.id == key, "Durable collection canonical key mismatch")?;
    let mapped: String = conn.query_row(
        "SELECT CASE WHEN length(CAST(body AS BLOB))<=80 THEN body ELSE 'null' END FROM records WHERE kind='collection_run_key' AND id=?",
        [&job.request_key],
        |r| r.get(0),
    )?;
    require(
        mapped.len() <= 80 && serde_json::from_str::<String>(&mapped)? == key,
        "Durable collection request-key binding mismatch",
    )?;
    let machine = collection_machine::replay(&job, |request, result| {
        if let FetchRecord::Complete { sha256, bytes, .. } = result {
            require(
                *bytes <= crate::collection::PAGE_BYTES as u64
                    && sha256.len() == 64
                    && sha256
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "Invalid retained response reference",
            )?;
            let evidence = bounded_evidence(conn, sha256)?
                .ok_or_else(|| Error::Validation("Retained response evidence is missing".into()))?;
            require(
                evidence.id == *sha256 && evidence.sha256 == *sha256 && evidence.bytes == *bytes,
                "Retained response evidence identity mismatch",
            )?;
            let ended = job
                .events
                .iter()
                .find_map(|event| match event {
                    CollectionEvent::Complete {
                        at_ms, sequence, ..
                    } if *sequence == request.sequence => Some(*at_ms),
                    _ => None,
                })
                .ok_or_else(|| Error::Validation("No settled acquisition time".into()))?;
            require(
                evidence.acquisitions.iter().any(|a| {
                    a.job_id == job.id
                        && a.url == request.entry.url
                        && a.retrieved_at == stamp(ended)
                }),
                "Retained response acquisition is missing",
            )?;
            Ok(Some(read_original(root, &evidence)?))
        } else {
            Ok(None)
        }
    })?;
    Ok(Loaded {
        job,
        machine,
        revision,
    })
}
fn bounded_evidence(conn: &Connection, key: &str) -> Result<Option<Evidence>> {
    let body: Option<Option<String>> = conn.query_row(
        "SELECT CASE WHEN length(CAST(body AS BLOB))<=? THEN body ELSE NULL END FROM records WHERE kind='evidence' AND id=?",
        params![MAX_RECORD_BYTES, key], |r| r.get(0),
    ).optional()?;
    match body {
        None => Ok(None),
        Some(None) => Err(Error::Validation(
            "Response evidence metadata exceeds collection read bound".into(),
        )),
        Some(Some(body)) => Ok(Some(serde_json::from_str(&body)?)),
    }
}

fn response_record<'a>(
    url: &str,
    response: &'a CollectionResponse,
) -> Result<(FetchRecord, Option<&'a [u8]>)> {
    let result = match response {
        CollectionResponse::Complete {
            status,
            content_type,
            location,
            body,
        } => {
            require(
                body.len() <= crate::collection::PAGE_BYTES,
                "Complete response exceeds body limit",
            )?;
            let base = policy::validate_https_url(url)?;
            let redirect_url = if [301, 302, 303, 307, 308].contains(status) {
                location
                    .as_ref()
                    .filter(|raw| raw.len() <= 2048)
                    .and_then(|raw| base.join(raw).ok())
                    .and_then(|u| policy::validate_https_url(u.as_str()).ok())
                    .map(|u| u.to_string())
            } else {
                None
            };
            (
                FetchRecord::Complete {
                    status: *status,
                    media_type: crate::collection::media_type(content_type),
                    redirect_url,
                    sha256: hash(body),
                    bytes: body.len() as u64,
                },
                Some(body.as_slice()),
            )
        }
        CollectionResponse::Incomplete {
            status,
            content_type,
        } => (
            FetchRecord::Incomplete {
                status: *status,
                media_type: crate::collection::media_type(content_type),
            },
            None,
        ),
        CollectionResponse::Failed(reason) => (FetchRecord::Failed { reason: *reason }, None),
    };
    collection_machine::validate_fetch(&result.0, result.1)?;
    Ok(result)
}

#[cfg(test)]
#[path = "collection_jobs_tests.rs"]
mod tests;
