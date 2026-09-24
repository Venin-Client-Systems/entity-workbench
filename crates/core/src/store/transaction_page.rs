//! Fixed parameterized keyset queries over one SQLite snapshot.
use super::*;
use crate::transaction_page::*;
use serde::Deserialize;
use std::collections::BTreeSet;

// Scope matches TransactionAnalysisRequest::includes: canonical dates are ISO text,
// account/currency comparisons are exact, and neither applies a review exclusion.
const SCOPE: &str = "kind='transaction'
 AND (?1 IS NULL OR json_extract(body,'$.date') >= ?1)
 AND (?2 IS NULL OR json_extract(body,'$.date') <= ?2)
 AND (?3 IS NULL OR json_extract(body,'$.account') = ?3)
 AND (?4 IS NULL OR json_extract(body,'$.currency') = ?4)";
const REVIEW: &str = "(?5 IS NULL OR json_extract(body,'$.review') = ?5)";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    schema_version: u32,
    query_sha256: String,
    date: String,
    sequence: i64,
}
impl Cursor {
    fn encode(&self) -> Result<String> {
        let bytes = serde_json::to_vec(self)?;
        Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
    }
    fn decode(value: &str, query: &str) -> Result<Self> {
        require(
            value.len() <= MAX_CURSOR_BYTES
                && value.len().is_multiple_of(2)
                && value
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
            "Invalid transaction cursor encoding",
        )?;
        let bytes = (0..value.len())
            .step_by(2)
            .map(|index| {
                u8::from_str_radix(&value[index..index + 2], 16)
                    .map_err(|_| Error::Validation("Invalid transaction cursor".into()))
            })
            .collect::<Result<Vec<_>>>()?;
        let cursor: Self = serde_json::from_slice(&bytes)?;
        require(
            cursor.schema_version == 1 && cursor.query_sha256 == query && cursor.sequence > 0,
            "Transaction cursor belongs to another query or revision",
        )?;
        analytics::date(&cursor.date)?;
        Ok(cursor)
    }
}
fn query_hash(request: &TransactionPageRequest, revision: u64) -> Result<String> {
    // Cursor deliberately excluded: continuation must use the identical query.
    Ok(hash(&serde_json::to_vec(&(
        1u32,
        revision,
        &request.filter,
        request.order,
        request.page_size,
    ))?))
}
impl Workspace {
    pub fn page_transactions(
        &self,
        request: &TransactionPageRequest,
        expected_revision: u64,
    ) -> Result<TransactionPage> {
        request.validate()?;
        let snapshot = self.conn.unchecked_transaction()?;
        let revision = self.revision()?;
        if revision != expected_revision {
            return Err(Error::Conflict(
                "Transaction page revision changed; refresh and restart pagination".into(),
            ));
        }
        let query_sha256 = query_hash(request, revision)?;
        let cursor = request
            .cursor
            .as_ref()
            .map(|value| Cursor::decode(value, &query_sha256))
            .transpose()?;
        let f = &request.filter;
        let review = f.review_name();
        let mut counts = TransactionReviewCounts::default();
        let mut scope_count = 0u64;
        let mut count_query = snapshot.prepare(&format!(
            "SELECT CASE json_extract(body,'$.review')
                WHEN 'accepted' THEN 'accepted' WHEN 'pending' THEN 'pending'
                WHEN 'rejected' THEN 'rejected' WHEN 'deferred' THEN 'deferred'
                ELSE 'invalid' END AS review_state, count(*)
             FROM records WHERE {SCOPE} GROUP BY review_state"
        ))?;
        let groups = count_query.query_map(
            params![f.date_from, f.date_to, f.account, f.currency],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?)),
        )?;
        for group in groups {
            let (state, count) = group?;
            let target = match state.as_str() {
                "accepted" => &mut counts.accepted,
                "pending" => &mut counts.pending,
                "rejected" => &mut counts.rejected,
                "deferred" => &mut counts.deferred,
                _ => {
                    return Err(Error::Validation(
                        "Invalid canonical transaction review state".into(),
                    ))
                }
            };
            *target = count;
            scope_count = scope_count
                .checked_add(count)
                .ok_or_else(|| Error::Validation("Transaction count overflow".into()))?;
        }
        drop(count_query);
        let selected_count = match review {
            Some("accepted") => counts.accepted,
            Some("pending") => counts.pending,
            Some("rejected") => counts.rejected,
            Some("deferred") => counts.deferred,
            None => scope_count,
            _ => unreachable!(),
        };
        if let Some(cursor) = &cursor {
            let date: Option<String> = snapshot
                .query_row(
                    &format!(
                        "SELECT json_extract(body,'$.date') FROM records
                    WHERE {SCOPE} AND {REVIEW} AND sequence=?6
                    AND length(CAST(json_extract(body,'$.date') AS BLOB))=10"
                    ),
                    params![
                        f.date_from,
                        f.date_to,
                        f.account,
                        f.currency,
                        review,
                        cursor.sequence
                    ],
                    |row| row.get(0),
                )
                .optional()?;
            require(
                date.as_deref() == Some(cursor.date.as_str()),
                "Transaction cursor no longer identifies a row in the selected scope",
            )?;
        }
        // Counts and cursor validation share this snapshot. Once no selected
        // rows exist, a second scoped scan cannot contribute a row or source.
        // Keep this after cursor validation: empty selections must not turn an
        // invalid continuation into a successful empty response.
        if selected_count == 0 {
            let result = TransactionPage {
                schema_version: 1,
                workspace_revision: revision,
                query_sha256,
                scope_count,
                review_counts: counts,
                selected_count,
                rows: Vec::new(),
                next_cursor: None,
            };
            snapshot.commit()?;
            return Ok(result);
        }
        // Direction is a closed enum, never an analyst-supplied SQL fragment.
        // Equal-date ties remain sequence-ascending in both orders.
        let (direction, comparison) = match request.order {
            TransactionPageOrder::DateAscending => ("ASC", ">"),
            TransactionPageOrder::DateDescending => ("DESC", "<"),
        };
        let sql = format!("SELECT sequence,id,length(CAST(body AS BLOB)),length(CAST(id AS BLOB)) FROM records WHERE {SCOPE} AND {REVIEW}
            AND (?6 IS NULL OR json_extract(body,'$.date') {comparison} ?6 OR (json_extract(body,'$.date')=?6 AND sequence>?7))
            ORDER BY json_extract(body,'$.date') {direction}, sequence ASC LIMIT ?8");
        let mut statement = snapshot.prepare(&sql)?;
        let mut matches = statement.query(params![
            f.date_from,
            f.date_to,
            f.account,
            f.currency,
            review,
            cursor.as_ref().map(|c| c.date.as_str()),
            cursor.as_ref().map(|c| c.sequence),
            request.page_size + 1
        ])?;
        let mut rows = Vec::new();
        let mut body_bytes = 0usize;
        let mut last = None;
        let mut more = false;
        let scope = f.scope();
        while let Some(row) = matches.next()? {
            if rows.len() == request.page_size as usize {
                more = true;
                break;
            }
            let size: u64 = row.get(2)?;
            if size > MAX_PAGE_BODY_BYTES as u64 || body_bytes + size as usize > MAX_PAGE_BODY_BYTES
            {
                require(
                    !rows.is_empty(),
                    "A single transaction exceeds the 2 MiB page-body bound",
                )?;
                more = true;
                break;
            }
            // Do not allocate the body until both per-row and aggregate bounds pass.
            let sequence: i64 = row.get(0)?;
            let body: String = snapshot.query_row(
                "SELECT body FROM records WHERE sequence=?",
                [sequence],
                |record| record.get(0),
            )?;
            require(
                body.len() as u64 == size,
                "Transaction body size changed inside snapshot",
            )?;
            let value: crate::domain::Transaction = serde_json::from_str(&body)?;
            require(
                row.get::<_, u64>(3)? <= MAX_PAGE_BODY_BYTES as u64,
                "Canonical transaction key exceeds the page bound",
            )?;
            let key: String = row.get(1)?;
            require(
                value.id == key
                    && value.version > 0
                    && scope.includes(&value)
                    && f.review.as_ref().is_none_or(|state| *state == value.review),
                "Canonical transaction identity or scope mismatch",
            )?;
            analytics::validate_transaction(&value)?;
            last = Some(Cursor {
                schema_version: 1,
                query_sha256: query_sha256.clone(),
                date: value.date.clone(),
                sequence,
            });
            rows.push(value);
            body_bytes += size as usize;
        }
        drop(matches);
        drop(statement);
        let sources: BTreeSet<_> = rows.iter().map(|row| row.anchor.evidence_id()).collect();
        for source in sources {
            self.verify_original(&get::<Evidence>(&snapshot, "evidence", source)?)?;
        }
        let next_cursor = if more {
            Some(
                last.ok_or_else(|| {
                    Error::Validation("Transaction continuation has no preceding row".into())
                })?
                .encode()?,
            )
        } else {
            None
        };
        let result = TransactionPage {
            schema_version: 1,
            workspace_revision: revision,
            query_sha256,
            scope_count,
            review_counts: counts,
            selected_count,
            rows,
            next_cursor,
        };
        snapshot.commit()?;
        Ok(result)
    }
}

#[cfg(test)]
#[path = "transaction_page_tests.rs"]
mod tests;
