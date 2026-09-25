//! Target resolution, counts, cursor and complete page use one SQLite read snapshot.
use super::*;
use crate::review_decision_page::*;
use serde::Deserialize;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    schema_version: u32,
    query_sha256: String,
    sequence: i64,
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
            value.len() <= MAX_DECISION_CURSOR_BYTES
                && value.len().is_multiple_of(2)
                && value
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
            "Invalid review decision cursor encoding",
        )?;
        let bytes = (0..value.len())
            .step_by(2)
            .map(|index| {
                u8::from_str_radix(&value[index..index + 2], 16)
                    .map_err(|_| Error::Validation("Invalid review decision cursor".into()))
            })
            .collect::<Result<Vec<_>>>()?;
        let cursor: Self = serde_json::from_slice(&bytes)?;
        require(
            cursor.schema_version == 1 && cursor.query_sha256 == query && cursor.sequence > 0,
            "Review decision cursor belongs to another target, size or revision",
        )?;
        Ok(cursor)
    }
}
fn query_hash(
    request: &ReviewDecisionPageRequest,
    kind: ReviewDecisionTargetKind,
    revision: u64,
) -> Result<String> {
    // Version one always orders by ascending canonical sequence. Cursor excluded.
    Ok(hash(&serde_json::to_vec(&(
        1u32,
        revision,
        &request.target_id,
        kind,
        request.page_size,
    ))?))
}
fn target_kind(conn: &Connection, key: &str) -> Result<ReviewDecisionTargetKind> {
    // These are exactly the current generic ReviewDecision writer targets. Other
    // decision families (identity, collection, etc.) retain their own contracts.
    // Evaluate body identity inside SQLite without loading the target's body.
    let mut statement = conn.prepare("SELECT kind,
        coalesce(json_type(body,'$.id')='text' AND json_extract(body,'$.id')=?1,0)
        FROM records WHERE id=?1 AND kind IN
        ('entity','observation','hypothesis','finding','transaction','processing_job','merge') LIMIT 2")?;
    let mut matches = statement.query([key])?;
    let first = matches
        .next()?
        .map(|row| -> rusqlite::Result<_> {
            Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?))
        })
        .transpose()?
        .ok_or_else(|| Error::Validation("Review target is unavailable or unsupported".into()))?;
    require(
        matches.next()?.is_none(),
        "Review target identity is ambiguous across record kinds",
    )?;
    require(first.1, "Canonical review target body identity is invalid")?;
    match first.0.as_str() {
        "entity" => Ok(ReviewDecisionTargetKind::Entity),
        "observation" => Ok(ReviewDecisionTargetKind::Observation),
        "hypothesis" => Ok(ReviewDecisionTargetKind::Hypothesis),
        "finding" => Ok(ReviewDecisionTargetKind::Finding),
        "transaction" => Ok(ReviewDecisionTargetKind::Transaction),
        "processing_job" => Ok(ReviewDecisionTargetKind::ProcessingJob),
        "merge" => Ok(ReviewDecisionTargetKind::Merge),
        _ => Err(Error::Validation("Unsupported review target kind".into())),
    }
}
impl Workspace {
    pub fn page_review_decisions(
        &self,
        request: &ReviewDecisionPageRequest,
        expected_revision: u64,
    ) -> Result<ReviewDecisionPage> {
        request.validate()?;
        let snapshot = self.conn.unchecked_transaction()?;
        let revision = self.revision()?;
        if revision != expected_revision {
            return Err(Error::Conflict(
                "Review decision revision changed; refresh and restart history".into(),
            ));
        }
        let kind = target_kind(&snapshot, &request.target_id)?;
        let query_sha256 = query_hash(request, kind, revision)?;
        let cursor = request
            .cursor
            .as_ref()
            .map(|v| Cursor::decode(v, &query_sha256))
            .transpose()?;
        let scope_count: u64 = snapshot.query_row(
            "SELECT count(*) FROM records
            WHERE kind='decision' AND json_extract(body,'$.target_id')=?",
            [&request.target_id],
            |row| row.get(0),
        )?;
        if let Some(cursor) = &cursor {
            let exists: bool = snapshot.query_row(
                "SELECT EXISTS(SELECT 1 FROM records
                WHERE kind='decision' AND sequence=?1 AND json_extract(body,'$.target_id')=?2)",
                params![cursor.sequence, request.target_id],
                |row| row.get(0),
            )?;
            require(
                exists,
                "Review decision cursor no longer identifies a row in this target scope",
            )?;
        }
        let mut metadata = snapshot.prepare(
            "SELECT sequence,length(CAST(id AS BLOB)),length(CAST(body AS BLOB))
            FROM records WHERE kind='decision' AND json_extract(body,'$.target_id')=?1
            AND (?2 IS NULL OR sequence>?2) ORDER BY sequence ASC LIMIT ?3",
        )?;
        let mut candidates = metadata.query(params![
            request.target_id,
            cursor.as_ref().map(|c| c.sequence),
            request.page_size + 1
        ])?;
        let mut positions = Vec::new();
        let mut total = 0u64;
        let mut more = false;
        while let Some(row) = candidates.next()? {
            if positions.len() == request.page_size as usize {
                more = true;
                break;
            }
            let size: u64 = row.get(2)?;
            total = total
                .checked_add(size)
                .ok_or_else(|| Error::Validation("Review decision size overflow".into()))?;
            require(
                total <= MAX_DECISION_PAGE_BODY_BYTES
                    && row.get::<_, u64>(1)? <= MAX_DECISION_PAGE_BODY_BYTES,
                "Review decision page exceeds the 1 MiB retained-body bound; no partial page returned",
            )?;
            positions.push((row.get::<_, i64>(0)?, size));
        }
        drop(candidates);
        drop(metadata);
        let mut body_query =
            snapshot.prepare("SELECT id,body FROM records WHERE sequence=? AND kind='decision'")?;
        let mut rows = Vec::with_capacity(positions.len());
        for (sequence, size) in &positions {
            let (key, body): (String, String) =
                body_query.query_row([sequence], |row| Ok((row.get(0)?, row.get(1)?)))?;
            require(
                body.len() as u64 == *size,
                "Review decision body size changed",
            )?;
            let decision: ReviewDecision = serde_json::from_str(&body)?;
            require(
                !decision.id.is_empty()
                    && decision.id == key
                    && decision.target_id == request.target_id,
                "Canonical review decision key or target identity is invalid",
            )?;
            // Historical reason/date strings are not normalized or revalidated
            // against today's writer limits. Strict typed decode preserves them.
            rows.push(decision);
        }
        drop(body_query);
        let next_cursor = if more {
            Some(
                Cursor {
                    schema_version: 1,
                    query_sha256: query_sha256.clone(),
                    sequence: positions
                        .last()
                        .ok_or_else(|| {
                            Error::Validation("Review continuation has no preceding row".into())
                        })?
                        .0,
                }
                .encode()?,
            )
        } else {
            None
        };
        let page = ReviewDecisionPage {
            schema_version: 1,
            workspace_revision: revision,
            target_id: request.target_id.clone(),
            resolved_target_kind: kind,
            scope_count,
            query_sha256,
            rows,
            next_cursor,
        };
        snapshot.commit()?;
        Ok(page)
    }
}

#[cfg(test)]
#[path = "review_decision_page_tests.rs"]
mod tests;
