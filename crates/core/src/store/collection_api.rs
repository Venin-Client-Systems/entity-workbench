//! Bounded durable-run projections. Historical job/receipt records are not rewritten.
use super::*;
use crate::collection_api::*;
use crate::collection_receipt::AcquisitionMode;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    version: u32,
    query_sha256: String,
    sequence: i64,
    id: String,
}
impl Cursor {
    fn encode(&self) -> Result<String> {
        Ok(serde_json::to_vec(self)?
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect())
    }
    fn decode(value: &str, query: &str) -> Result<Self> {
        require(
            value.len() <= 1024
                && value.len().is_multiple_of(2)
                && value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "Invalid collection cursor encoding",
        )?;
        let bytes = (0..value.len())
            .step_by(2)
            .map(|i| {
                u8::from_str_radix(&value[i..i + 2], 16)
                    .map_err(|_| Error::Validation("Invalid collection cursor".into()))
            })
            .collect::<Result<Vec<_>>>()?;
        let cursor: Self = serde_json::from_slice(&bytes)?;
        require(
            cursor.version == 1
                && cursor.query_sha256 == query
                && cursor.sequence > 0
                && canonical_uuid(&cursor.id),
            "Collection cursor belongs to another scope, page size or revision",
        )?;
        Ok(cursor)
    }
}
fn summary(job: &DurableCollectionJob) -> CollectionRunSummary {
    let checkpoint = &job.checkpoint;
    CollectionRunSummary {
        id: job.id.clone(),
        request_key: job.request_key.clone(),
        record_version: job.schema_version,
        mode: if job.synthetic {
            AcquisitionMode::Synthetic
        } else {
            AcquisitionMode::Live
        },
        collector_policy: job.collector_policy.clone(),
        input: job.input.clone(),
        created_at_ms: job.created_at_ms,
        updated_at_ms: checkpoint.updated_at_ms,
        state: checkpoint.state,
        generation: checkpoint.generation,
        first_started_at_ms: checkpoint.first_started_at_ms,
        deadline_at_ms: checkpoint.deadline_at_ms,
        cancellation_requested: checkpoint.cancellation_requested,
        requests_used: checkpoint.requests_used(),
        pages_retained: checkpoint.pages_retained,
        frontier_remaining: checkpoint.frontier.len() as u32,
    }
}
fn original(progress: &RequestProgress) -> Option<CollectionOriginalRef> {
    let result = match progress {
        RequestProgress::Settled { result, .. } => result.clone(),
        RequestProgress::Observed { receipt } => receipt.fetch_record(),
        _ => return None,
    };
    if let FetchRecord::Complete { sha256, bytes, .. } = result {
        Some(CollectionOriginalRef {
            evidence_id: sha256.clone(),
            sha256,
            bytes,
        })
    } else {
        None
    }
}
pub(crate) fn response_bound(value: &impl Serialize) -> Result<()> {
    require(
        serde_json::to_vec(value)?.len() <= RESPONSE_BYTES,
        "Collection response exceeds the 2 MiB bound; no partial response returned",
    )
}
impl Workspace {
    pub fn inspect_collection_run(&self, id: &str) -> Result<CollectionRunInspection> {
        require(canonical_uuid(id), "Invalid collection identifier")?;
        let tx = self.conn.unchecked_transaction()?;
        let revision: u64 = tx.query_row("SELECT revision FROM meta", [], |r| r.get(0))?;
        let loaded = load(&self.root, &tx, id, revision)?;
        let limitations = vec![
            "Collection records access facts, not permission, relevance, source independence or accepted observations.".into(),
            "Static text is unreviewed; original response bytes remain authoritative. No accepted page-region anchors are created.".into(),
            "Identical original bytes share evidence metadata. Later supported media interpretation may replace its current text; this is not an immutable web-text derivative.".into(),
            "V4 HTML interpretation has bounded parser admission and extraction. A limit retains the complete original/receipt with quota status and no partial derivative; historical over-limit interpretation is refused without rewriting.".into(),
            "Historical v1/v2/v3 specimens remain read-only and cannot resume through public controls.".into(),
        ];
        let requests = loaded
            .job
            .checkpoint
            .requests
            .iter()
            .map(|r| CollectionRequestView {
                sequence: r.sequence,
                generation: r.generation,
                entry: r.entry.clone(),
                reserved_at_ms: r.reserved_at_ms,
                original: original(&r.progress),
                progress: r.progress.clone(),
            })
            .collect();
        let response = CollectionRunInspection {
            schema_version: 1,
            workspace_revision: revision,
            availability: CollectionAvailability::StandaloneUnavailable,
            native_execution_enabled: NATIVE_COLLECTION_ENABLED,
            run: summary(&loaded.job),
            execution: CollectionExecutionStatus {
                phase: CollectionExecutionPhase::Unavailable,
                request_sequence: None,
                publication_retries: 0,
                publication_retry_limit: 3,
            },
            controls: CollectionControls {
                can_cancel: false,
                can_resume: false,
                can_retry_settlement: false,
            },
            requests,
            limitations,
        };
        response_bound(&response)?;
        Ok(response)
    }
    pub fn page_collection_runs(
        &self,
        request: &CollectionRunPageRequest,
        expected_revision: Option<u64>,
    ) -> Result<CollectionRunPage> {
        request.validate()?;
        let tx = self.conn.unchecked_transaction()?;
        let revision: u64 = tx.query_row("SELECT revision FROM meta", [], |r| r.get(0))?;
        if expected_revision.is_some_and(|expected| expected != revision) {
            return Err(Error::Conflict(
                "Collection catalogue changed; refresh from the first page".into(),
            ));
        }
        let query = hash(&serde_json::to_vec(&(
            1u32,
            "collection_run",
            "canonical_sequence_ascending",
            revision,
            request.page_size,
        ))?);
        let cursor = request
            .cursor
            .as_deref()
            .map(|c| Cursor::decode(c, &query))
            .transpose()?;
        if let Some(cursor) = &cursor {
            let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM records WHERE kind='collection_run' AND sequence=? AND id=?)",
                params![cursor.sequence, cursor.id], |r| r.get(0))?;
            require(
                exists,
                "Collection cursor does not identify a canonical run",
            )?;
        }
        let scope_count = tx.query_row(
            "SELECT count(*) FROM records WHERE kind='collection_run'",
            [],
            |r| r.get(0),
        )?;
        let mut statement = tx.prepare("SELECT sequence, CASE WHEN length(CAST(id AS BLOB))=36 THEN id ELSE NULL END,
            length(CAST(body AS BLOB)) FROM records WHERE kind='collection_run' AND (?1 IS NULL OR sequence>?1)
            ORDER BY sequence ASC LIMIT ?2")?;
        let candidates = statement
            .query_map(
                params![cursor.as_ref().map(|c| c.sequence), request.page_size + 1],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, Option<String>>(1)?,
                        r.get::<_, u64>(2)?,
                    ))
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let more = candidates.len() > request.page_size as usize;
        let mut rows = Vec::new();
        let mut last = None;
        for (sequence, key, bytes) in candidates.into_iter().take(request.page_size as usize) {
            require(
                bytes <= MAX_RECORD_BYTES as u64,
                "Collection record exceeds read bound",
            )?;
            let key =
                key.ok_or_else(|| Error::Validation("Invalid collection record key".into()))?;
            require(canonical_uuid(&key), "Invalid collection record key")?;
            let loaded = load(&self.root, &tx, &key, revision)?;
            rows.push(summary(&loaded.job));
            response_bound(&rows)?;
            last = Some((sequence, key));
        }
        let next_cursor = if more {
            let (sequence, id) = last.expect("nonempty page before continuation");
            Some(
                Cursor {
                    version: 1,
                    query_sha256: query,
                    sequence,
                    id,
                }
                .encode()?,
            )
        } else {
            None
        };
        let response = CollectionRunPage {
            schema_version: 1,
            workspace_revision: revision,
            availability: CollectionAvailability::StandaloneUnavailable,
            native_execution_enabled: NATIVE_COLLECTION_ENABLED,
            scope_count,
            rows,
            next_cursor,
        };
        response_bound(&response)?;
        Ok(response)
    }
}
#[cfg(test)]
#[path = "collection_api_tests.rs"]
mod tests;
