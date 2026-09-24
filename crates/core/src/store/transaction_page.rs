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
pub(super) enum PageContext<'a> {
    Ledger,
    Transfer {
        target_id: &'a str,
        expected_version: u32,
    },
}
fn page_params<'a>(
    base: &[&'a dyn rusqlite::ToSql],
    text: Option<&'a String>,
    target: Option<&'a crate::domain::Transaction>,
) -> Vec<&'a dyn rusqlite::ToSql> {
    let mut values = base.to_vec();
    if text.is_some() || target.is_some() {
        // Fixed text and transfer context follow all eight original parameters.
        // Unused positions remain NULL, never caller-controlled SQL fragments.
        values.resize(9, &rusqlite::types::Null);
        if let Some(text) = text {
            values[8] = text;
        }
    }
    if let Some(target) = target {
        values.push(&target.id);
        values.push(&target.account);
    }
    values
}
impl Workspace {
    pub fn page_transactions(
        &self,
        request: &TransactionPageRequest,
        expected_revision: u64,
    ) -> Result<TransactionPage> {
        self.transaction_page_with_search(request, expected_revision, None)
    }

    pub(super) fn transaction_page_with_search(
        &self,
        request: &TransactionPageRequest,
        expected_revision: u64,
        search: Option<(&String, &crate::literal_search::LiteralMatching)>,
    ) -> Result<TransactionPage> {
        self.transaction_page_with_context(request, expected_revision, search, PageContext::Ledger)
    }

    pub(super) fn transaction_page_with_context(
        &self,
        request: &TransactionPageRequest,
        expected_revision: u64,
        search: Option<(&String, &crate::literal_search::LiteralMatching)>,
        context: PageContext<'_>,
    ) -> Result<TransactionPage> {
        request.validate()?;
        let snapshot = self.conn.unchecked_transaction()?;
        let revision = self.revision()?;
        if revision != expected_revision {
            return Err(Error::Conflict(
                "Transaction page revision changed; refresh and restart pagination".into(),
            ));
        }
        let target = match context {
            PageContext::Ledger => None,
            PageContext::Transfer {
                target_id,
                expected_version,
            } => {
                require(
                    request.filter.review == Some(ReviewState::Accepted),
                    "Transfer candidates require accepted selection",
                )?;
                Some(super::transfer_candidates::resolve_target(
                    &snapshot,
                    target_id,
                    expected_version,
                )?)
            }
        };
        let mut verified_sources = BTreeSet::new();
        if let Some(target) = &target {
            let key = target.anchor.evidence_id();
            let evidence: Evidence = get(&snapshot, "evidence", key)?;
            require(
                evidence.id == key && evidence.sha256 == key,
                "Canonical transfer target source identity is invalid",
            )?;
            self.verify_original(&evidence)?;
            verified_sources.insert(key.to_string());
        }
        let base_scope = if target.is_some() {
            format!(
                "{SCOPE} AND id<>?10 AND (json_type(body,'$.account') IS NOT 'text'
                OR json_extract(body,'$.account')<>?11)"
            )
        } else {
            SCOPE.to_owned()
        };
        let text = search.map(|(text, _)| text).filter(|text| !text.is_empty());
        let scope_sql = if text.is_some() {
            super::transaction_search::register_matcher(&self.conn)?;
            // CASE fixes evaluation scope: malformed/oversized text in the base
            // date/account/currency scope must fail, even if it would not match.
            format!(
                "CASE WHEN {base_scope} THEN ew_transaction_text_match_v1(
                CASE WHEN json_type(body,'$.description')='text'
                    THEN json_extract(body,'$.description') END,
                CASE WHEN json_type(body,'$.account')='text'
                    THEN json_extract(body,'$.account') END,
                CASE WHEN json_type(body,'$.date')='text'
                    THEN json_extract(body,'$.date') END,?9) ELSE 0 END"
            )
        } else {
            base_scope
        };
        let query_sha256 = if let Some((text, matching)) = search {
            hash(&serde_json::to_vec(&(
                "transaction-search-v1",
                query_hash(request, revision)?,
                matching,
                text,
            ))?)
        } else {
            query_hash(request, revision)?
        };
        let query_sha256 = if let Some(target) = &target {
            hash(&serde_json::to_vec(&(
                "transfer-candidates-v1",
                query_sha256,
                &target.id,
                target.version,
            ))?)
        } else {
            query_sha256
        };
        let cursor = request
            .cursor
            .as_ref()
            .map(|value| Cursor::decode(value, &query_sha256))
            .transpose()?;
        let f = &request.filter;
        let review = f.review_name();
        let mut counts = TransactionReviewCounts::default();
        let mut scope_count = 0u64;
        let review_sql = "CASE json_extract(body,'$.review')
                WHEN 'accepted' THEN 'accepted' WHEN 'pending' THEN 'pending'
                WHEN 'rejected' THEN 'rejected' WHEN 'deferred' THEN 'deferred'
                ELSE 'invalid' END";
        let review_sql = if target.is_some() {
            // Missing/nontext account cannot establish that a row shares the
            // target account. Reject it in this scope instead of silently losing it.
            format!(
                "CASE WHEN json_type(body,'$.account') IS NOT 'text'
                THEN 'invalid_account' ELSE {review_sql} END"
            )
        } else {
            review_sql.to_owned()
        };
        let mut count_query = snapshot.prepare(&format!(
            "SELECT {review_sql} AS review_state, count(*)
             FROM records WHERE {scope_sql} GROUP BY review_state"
        ))?;
        let groups = count_query.query_map(
            rusqlite::params_from_iter(page_params(
                params![f.date_from, f.date_to, f.account, f.currency],
                text,
                target.as_ref(),
            )),
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?)),
        )?;
        for group in groups {
            let (state, count) = group?;
            let target = match state.as_str() {
                "accepted" => &mut counts.accepted,
                "pending" => &mut counts.pending,
                "rejected" => &mut counts.rejected,
                "deferred" => &mut counts.deferred,
                "invalid_account" => {
                    return Err(Error::Validation(
                        "Canonical transfer candidate account is missing or not text".into(),
                    ))
                }
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
                    WHERE {scope_sql} AND {REVIEW} AND sequence=?6
                    AND length(CAST(json_extract(body,'$.date') AS BLOB))=10"
                    ),
                    rusqlite::params_from_iter(page_params(
                        params![
                            f.date_from,
                            f.date_to,
                            f.account,
                            f.currency,
                            review,
                            cursor.sequence
                        ],
                        text,
                        target.as_ref(),
                    )),
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
        let sql = format!("SELECT sequence,id,length(CAST(body AS BLOB)),length(CAST(id AS BLOB)) FROM records WHERE {scope_sql} AND {REVIEW}
            AND (?6 IS NULL OR json_extract(body,'$.date') {comparison} ?6 OR (json_extract(body,'$.date')=?6 AND sequence>?7))
            ORDER BY json_extract(body,'$.date') {direction}, sequence ASC LIMIT ?8");
        let mut statement = snapshot.prepare(&sql)?;
        let mut matches = statement.query(rusqlite::params_from_iter(page_params(
            params![
                f.date_from,
                f.date_to,
                f.account,
                f.currency,
                review,
                cursor.as_ref().map(|c| c.date.as_str()),
                cursor.as_ref().map(|c| c.sequence),
                request.page_size + 1
            ],
            text,
            target.as_ref(),
        )))?;
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
            let text_matches = text
                .map(|query| {
                    crate::transaction_search::matches_lowered(
                        &value.description,
                        &value.account,
                        &value.date,
                        query,
                    )
                })
                .transpose()?
                .unwrap_or(true);
            require(
                value.id == key
                    && value.version > 0
                    && scope.includes(&value)
                    && text_matches
                    && target.as_ref().is_none_or(|target| {
                        value.id != target.id && value.account != target.account
                    })
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
            if !verified_sources.insert(source.to_string()) {
                continue;
            }
            let evidence: Evidence = get(&snapshot, "evidence", source)?;
            require(
                evidence.id == source && evidence.sha256 == source,
                "Canonical transaction source identity is invalid",
            )?;
            self.verify_original(&evidence)?;
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
