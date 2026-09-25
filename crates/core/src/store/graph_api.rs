//! Read projections are pinned to one SQLite snapshot; no worker publication authority.
use super::*;
use crate::{graph_api::*, graph_jobs::GraphAnalysisInspection, processing::*};
use serde::Deserialize;

const SCOPE: &str = "kind='processing_job' AND (json_extract(body,'$.schema_version')=5 OR json_extract(body,'$.input.operation')='shortest_connection_path')";

pub(super) fn load(conn: &Connection, key: &str) -> Result<ProcessingJob> {
    require(uuid(key), "Invalid graph job identifier")?;
    let mut statement =
        conn.prepare("SELECT body FROM records WHERE kind='processing_job' AND id=?")?;
    let mut rows = statement.query([key])?;
    let row = rows
        .next()?
        .ok_or_else(|| Error::Validation("Unknown graph job".into()))?;
    let raw = row
        .get_ref(0)?
        .as_str()
        .map_err(|_| Error::Validation("Invalid graph job storage".into()))?;
    require(
        raw.len() <= JOB_BYTES,
        "Graph job exceeds 64 KiB read bound",
    )?;
    let job: ProcessingJob = serde_json::from_str(raw)?;
    super::processing::supported_job(&job)?;
    let ProcessingInput::ShortestConnectionPath {
        source_id,
        target_id,
        ..
    } = &job.input
    else {
        return Err(Error::Validation("Not a graph job".into()));
    };
    endpoint(source_id)?;
    endpoint(target_id)?;
    require(
        job.id == key
            && uuid(&job.request_key)
            && source_id != target_id
            && (1..=3).contains(&job.attempt)
            && !job.retry.automatic
            && job.retry.max_attempts == 3
            && job.lease.as_deref().is_none_or(uuid)
            && job.result_ids.len() <= 3
            && job.result_ids.iter().all(|id| digest(id))
            && job.detail.len() <= 4096,
        "Invalid graph job identity or bounded metadata",
    )?;
    for at in [
        Some(&job.created_at),
        Some(&job.updated_at),
        job.started_at.as_ref(),
        job.finished_at.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        require(
            at.len() <= 64 && chrono::DateTime::parse_from_rfc3339(at).is_ok(),
            "Invalid graph job timestamp",
        )?;
    }
    Ok(job)
}

fn result_reference(
    conn: &Connection,
    job: &ProcessingJob,
    key: &str,
    revision: u64,
) -> Result<GraphResultReference> {
    // Closed scalar projections: SQLite may parse the stored document, but Rust never
    // allocates the frozen model or an unbounded metadata string for this lookup.
    let fields = [("id",64),("job_id",36),("request_key",36),("request_sha256",64),("result_sha256",64)]
        .map(|(field,max)| format!("CASE WHEN json_type(body,'$.{field}')='text' AND length(CAST(json_extract(body,'$.{field}') AS BLOB))={max} THEN json_extract(body,'$.{field}') ELSE NULL END"));
    let integers = ["schema_version","attempt","captured_revision","published_revision"]
        .map(|field| format!("CASE WHEN json_type(body,'$.{field}')='integer' THEN json_extract(body,'$.{field}') ELSE NULL END"));
    let query = format!(
        "SELECT length(CAST(body AS BLOB)),{},{} FROM records WHERE kind='graph_analysis' AND id=?",
        fields.join(","),
        integers.join(",")
    );
    let projected = conn
        .query_row(&query, [key], |row| {
            Ok((
                row.get::<_, u64>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<u32>>(6)?,
                row.get::<_, Option<u32>>(7)?,
                row.get::<_, Option<u64>>(8)?,
                row.get::<_, Option<u64>>(9)?,
            ))
        })
        .optional()?
        .ok_or_else(|| Error::Validation("Referenced graph result is missing".into()))?;
    let (
        bytes,
        Some(id),
        Some(job_id),
        Some(request_key),
        Some(request_sha256),
        Some(result_sha256),
        Some(version),
        Some(attempt),
        Some(captured_revision),
        Some(published_revision),
    ) = projected
    else {
        return Err(Error::Validation(
            "Invalid graph result reference metadata".into(),
        ));
    };
    require(
        bytes <= crate::graph_jobs::MAX_GRAPH_RECORD_BYTES as u64
            && version == 1
            && id == key
            && job_id == job.id
            && request_key == job.request_key
            && (1..=job.attempt).contains(&attempt)
            && digest(&request_sha256)
            && digest(&result_sha256)
            && captured_revision > 0
            && captured_revision.checked_add(1) == Some(published_revision)
            && published_revision <= revision,
        "Graph result reference identity or bounds differ",
    )?;
    Ok(GraphResultReference {
        id,
        request_sha256,
        result_sha256,
        captured_revision,
        published_revision,
    })
}

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
            .map(|byte| format!("{byte:02x}"))
            .collect())
    }
    fn decode(value: &str, query: &str) -> Result<Self> {
        require(
            !value.is_empty()
                && value.len() <= 1024
                && value.len().is_multiple_of(2)
                && value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "Invalid graph cursor encoding",
        )?;
        let bytes = (0..value.len())
            .step_by(2)
            .map(|i| {
                u8::from_str_radix(&value[i..i + 2], 16)
                    .map_err(|_| Error::Validation("Invalid graph cursor".into()))
            })
            .collect::<Result<Vec<_>>>()?;
        let cursor: Self = serde_json::from_slice(&bytes)?;
        require(
            cursor.version == 1 && cursor.sequence > 0 && uuid(&cursor.id),
            "Invalid graph cursor identity",
        )?;
        if cursor.query_sha256 != query {
            return Err(Error::Conflict("Graph cursor is stale or belongs to another page size; refresh from the first page".into()));
        }
        Ok(cursor)
    }
}
fn inspection(
    job: ProcessingJob,
    results: Vec<GraphResultReference>,
    revision: u64,
) -> GraphJobInspection {
    let can_cancel = matches!(
        job.state,
        ProcessingState::Queued | ProcessingState::Running
    ) && !job.cancellation_requested;
    GraphJobInspection {
        schema_version: 1, workspace_revision: revision,
        availability: GraphAvailability::StandaloneUnavailable,
        job, results, execution: None,
        controls: GraphControls { can_cancel, can_retry_publication: false },
        limitations: vec![
            "Queue acknowledgement retains the request; it does not mean an executable runtime is available or a worker has started.".into(),
            "Job metadata does not verify source originals. Inspect the immutable result for frozen provenance and current original integrity.".into(),
            "The fixed path policy uses accepted undirected relationships across retained dates; a path is not a causal or contemporaneous conclusion.".into(),
        ],
    }
}
impl Workspace {
    /// Exact request replay is read-only, including after later revisions or a terminal outcome.
    pub(crate) fn graph_request_job(&self, key: &str) -> Result<Option<ProcessingJob>> {
        require(uuid(key), "A canonical UUID request key is required")?;
        let tx = self.conn.unchecked_transaction()?;
        let mut statement =
            tx.prepare("SELECT body FROM records WHERE kind='processing_request' AND id=?")?;
        let mut rows = statement.query([key])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        let raw = row
            .get_ref(0)?
            .as_str()
            .map_err(|_| Error::Validation("Invalid graph request mapping".into()))?;
        require(raw.len() <= 128, "Graph request mapping exceeds bound")?;
        let id: String = serde_json::from_str(raw)?;
        let job = load(&tx, &id)?;
        require(
            job.request_key == key,
            "Graph request mapping identity differs",
        )?;
        Ok(Some(job))
    }
    pub fn queue_graph_job(
        &mut self,
        revision: u64,
        source: &str,
        target: &str,
        key: &str,
    ) -> Result<GraphJobInspection> {
        endpoint(source)?;
        endpoint(target)?;
        let job = self.queue_graph_path(revision, source, target, key)?;
        self.inspect_graph_job(&job.id)
    }
    pub fn inspect_graph_job(&self, key: &str) -> Result<GraphJobInspection> {
        let tx = self.conn.unchecked_transaction()?;
        let revision = self.revision()?;
        let job = load(&tx, key)?;
        let results = job
            .result_ids
            .iter()
            .map(|id| result_reference(&tx, &job, id, revision))
            .collect::<Result<Vec<_>>>()?;
        Ok(inspection(job, results, revision))
    }
    pub fn cancel_graph_job(&mut self, key: &str, attempt: u32) -> Result<GraphJobInspection> {
        // Validate the exact selected graph kind before delegating canonical cancellation.
        require(attempt > 0, "Invalid graph attempt")?;
        self.cancel_graph_processing_job(key, attempt)?;
        self.inspect_graph_job(key)
    }
    pub fn inspect_graph_result(
        &self,
        key: &str,
        request: &str,
        result: &str,
    ) -> Result<GraphAnalysisInspection> {
        require(
            digest(key) && digest(request) && digest(result),
            "Invalid graph result identity",
        )?;
        let inspection = self.inspect_graph_analysis(key)?;
        require(
            inspection.record.request_sha256 == request
                && inspection.record.result_sha256 == result,
            "Graph result digests differ from the selected snapshot",
        )?;
        Ok(inspection)
    }
    pub fn page_graph_jobs(
        &self,
        request: &GraphJobPageRequest,
        expected_revision: Option<u64>,
    ) -> Result<GraphJobPage> {
        request.validate()?;
        let tx = self.conn.unchecked_transaction()?;
        let revision = self.revision()?;
        if expected_revision.is_some_and(|r| r != revision) {
            return Err(Error::Conflict(
                "Graph catalogue changed; refresh from the first page".into(),
            ));
        }
        let query = hash(&serde_json::to_vec(&(
            1u32,
            "graph_jobs",
            "sequence_ascending",
            revision,
            request.page_size,
        ))?);
        let cursor = request
            .cursor
            .as_deref()
            .map(|c| Cursor::decode(c, &query))
            .transpose()?;
        if let Some(cursor) = &cursor {
            let exists: bool = tx.query_row(
                &format!(
                    "SELECT EXISTS(SELECT 1 FROM records WHERE {SCOPE} AND sequence=? AND id=?)"
                ),
                params![cursor.sequence, cursor.id],
                |r| r.get(0),
            )?;
            require(
                exists,
                "Graph cursor does not identify a canonical graph job",
            )?;
        }
        let total_count = tx.query_row(
            &format!("SELECT count(*) FROM records WHERE {SCOPE}"),
            [],
            |r| r.get(0),
        )?;
        let mut statement = tx.prepare(&format!("SELECT sequence,CASE WHEN length(CAST(id AS BLOB))=36 THEN id ELSE NULL END,length(CAST(body AS BLOB)) FROM records WHERE {SCOPE} AND (?1 IS NULL OR sequence>?1) ORDER BY sequence ASC LIMIT ?2"))?;
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
                sequence > 0 && bytes <= JOB_BYTES as u64,
                "Graph job metadata exceeds read bound",
            )?;
            let key = key.ok_or_else(|| Error::Validation("Invalid graph job key".into()))?;
            rows.push(GraphJobEntry {
                sequence: sequence as u64,
                job: load(&tx, &key)?,
            });
            last = Some((sequence, key));
        }
        let next_cursor = if more {
            let (sequence, id) = last.expect("nonempty bounded graph page");
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
        let response = GraphJobPage {
            schema_version: 1,
            workspace_revision: revision,
            availability: GraphAvailability::StandaloneUnavailable,
            total_count,
            rows,
            next_cursor,
        };
        require(
            serde_json::to_vec(&response)?.len() <= PAGE_BYTES,
            "Graph page exceeds 2 MiB; no partial response returned",
        )?;
        Ok(response)
    }
}

#[cfg(test)]
#[path = "graph_api_tests.rs"]
mod tests;
