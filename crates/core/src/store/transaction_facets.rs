//! Bounded selector metadata. This does not validate or analyse the source transactions.
use super::*;
use crate::transaction_facets::*;
use serde::Deserialize;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    schema_version: u32,
    query_sha256: String,
    // First canonical record for the preceding distinct value. Keeping the value
    // out of the cursor bounds its size even for long legacy account names.
    sequence: i64,
}
impl Cursor {
    fn encode(&self) -> Result<String> {
        Ok(serde_json::to_vec(self)?
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect())
    }
    fn decode(text: &str, query: &str) -> Result<Self> {
        require(
            text.len() <= MAX_FACET_CURSOR_BYTES
                && text.len().is_multiple_of(2)
                && text
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "Invalid transaction facet cursor encoding",
        )?;
        let bytes = (0..text.len())
            .step_by(2)
            .map(|i| {
                u8::from_str_radix(&text[i..i + 2], 16)
                    .map_err(|_| Error::Validation("Invalid transaction facet cursor".into()))
            })
            .collect::<Result<Vec<_>>>()?;
        let value: Self = serde_json::from_slice(&bytes)?;
        require(
            value.schema_version == 1 && value.query_sha256 == query && value.sequence > 0,
            "Transaction facet cursor belongs to another query or revision",
        )?;
        Ok(value)
    }
}

impl Workspace {
    pub fn page_transaction_facets(
        &self,
        request: &TransactionFacetRequest,
        expected_revision: u64,
    ) -> Result<TransactionFacetPage> {
        request.validate()?;
        let snapshot = self.conn.unchecked_transaction()?;
        let revision = self.revision()?;
        if revision != expected_revision {
            return Err(Error::Conflict(
                "Transaction facet revision changed; refresh and restart suggestions".into(),
            ));
        }
        let query_sha256 = hash(&serde_json::to_vec(&(
            "transaction-facets-v1",
            revision,
            request.facet,
            request.page_size,
        ))?);
        let cursor = request
            .cursor
            .as_ref()
            .map(|value| Cursor::decode(value, &query_sha256))
            .transpose()?;
        let path = request.facet.path();
        // Inspect type and length in SQLite, before copying any value. Invalid or
        // oversized entries fail explicitly rather than disappearing from counts.
        let invalid: bool = snapshot.query_row(
            "SELECT EXISTS(SELECT 1 FROM records WHERE kind='transaction' AND
                (coalesce(json_type(body,?1),'missing')!='text'
                 OR length(CAST(json_extract(body,?1) AS BLOB)) NOT BETWEEN 1 AND ?2))",
            params![path, MAX_FACET_VALUE_BYTES],
            |row| row.get(0),
        )?;
        require(
            !invalid,
            "Canonical transaction facet is missing, non-text or exceeds 4000 bytes",
        )?;
        let (transaction_count, distinct_count): (u64, u64) = snapshot.query_row(
            "SELECT count(*), count(DISTINCT json_extract(body,?1) COLLATE BINARY)
             FROM records WHERE kind='transaction'",
            [path],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let after: Option<String> = if let Some(cursor) = &cursor {
            let value: Option<String> = snapshot.query_row(
                "SELECT json_extract(body,?1) FROM records WHERE kind='transaction' AND sequence=?2
                 AND sequence=(SELECT min(other.sequence) FROM records other
                   WHERE other.kind='transaction' AND json_extract(other.body,?1)=json_extract(records.body,?1) COLLATE BINARY)",
                params![path, cursor.sequence], |row| row.get(0),
            ).optional()?;
            require(
                value.is_some(),
                "Transaction facet cursor no longer identifies a distinct value",
            )?;
            value
        } else {
            None
        };
        let mut metadata = snapshot.prepare(
            "SELECT min(sequence), count(*), length(CAST(json_extract(body,?1) AS BLOB))
             FROM records WHERE kind='transaction'
             AND (?2 IS NULL OR json_extract(body,?1) COLLATE BINARY > ?2)
             GROUP BY json_extract(body,?1) COLLATE BINARY
             ORDER BY json_extract(body,?1) COLLATE BINARY ASC LIMIT ?3",
        )?;
        let mut candidates = metadata.query(params![path, after, request.page_size + 1])?;
        let mut positions = Vec::new();
        let mut total_bytes = 0u64;
        let mut more = false;
        while let Some(row) = candidates.next()? {
            if positions.len() == request.page_size as usize {
                more = true;
                break;
            }
            let size: u64 = row.get(2)?;
            total_bytes = total_bytes
                .checked_add(size)
                .ok_or_else(|| Error::Validation("Transaction facet size overflow".into()))?;
            require(
                total_bytes <= MAX_FACET_PAGE_BYTES,
                "Transaction facet page exceeds 256 KiB of value text; request fewer rows",
            )?;
            positions.push((row.get::<_, i64>(0)?, row.get::<_, u64>(1)?, size));
        }
        drop(candidates);
        drop(metadata);
        let mut values = Vec::with_capacity(positions.len());
        for (sequence, count, size) in &positions {
            let value: String = snapshot.query_row(
                "SELECT json_extract(body,?1) FROM records WHERE kind='transaction' AND sequence=?2",
                params![path, sequence], |row| row.get(0),
            )?;
            require(
                value.len() as u64 == *size,
                "Transaction facet size changed inside snapshot",
            )?;
            match request.facet {
                TransactionFacetKind::Account => require(
                    !value.trim().is_empty() && !value.chars().any(char::is_control),
                    "Canonical account facet is empty or contains controls unsupported by ledger filters",
                )?,
                TransactionFacetKind::Currency => require(
                    value.len() == 3 && value.bytes().all(|b| b.is_ascii_uppercase()),
                    "Canonical currency facet must be three uppercase letters",
                )?,
            }
            values.push(TransactionFacetValue {
                value,
                transaction_count: *count,
            });
        }
        let next_cursor = if more {
            Some(
                Cursor {
                    schema_version: 1,
                    query_sha256: query_sha256.clone(),
                    sequence: positions
                        .last()
                        .ok_or_else(|| {
                            Error::Validation("Facet continuation has no preceding value".into())
                        })?
                        .0,
                }
                .encode()?,
            )
        } else {
            None
        };
        snapshot.commit()?;
        Ok(TransactionFacetPage {
            schema_version: 1,
            workspace_revision: revision,
            facet: request.facet,
            query_sha256,
            transaction_count,
            distinct_count,
            values,
            next_cursor,
        })
    }
}

#[cfg(test)]
#[path = "transaction_facets_tests.rs"]
mod tests;
