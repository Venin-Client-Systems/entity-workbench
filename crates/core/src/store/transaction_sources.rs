//! Revision, size checks and rows share one canonical read snapshot.
use super::*;
use crate::transaction_sources::*;
use std::collections::BTreeSet;

impl Workspace {
    pub fn read_transaction_sources(
        &self,
        request: &TransactionSourcesRequest,
        expected_revision: u64,
    ) -> Result<TransactionSources> {
        request.validate()?;
        let snapshot = self.conn.unchecked_transaction()?;
        let revision = self.revision()?;
        if revision != expected_revision {
            return Err(Error::Conflict(
                "Transaction source revision changed; refresh the originating result".into(),
            ));
        }
        let mut sizes = Vec::with_capacity(request.rows.len());
        let mut total = 0u64;
        let mut metadata = snapshot.prepare(
            "SELECT length(CAST(body AS BLOB)) FROM records WHERE kind='transaction' AND id=?",
        )?;
        // Check the complete batch before allocating any retained row body.
        for key in &request.rows {
            let size: Option<u64> = metadata.query_row([&key.id], |row| row.get(0)).optional()?;
            let size = size.ok_or_else(|| {
                Error::Validation("A selected transaction source is unavailable".into())
            })?;
            total = total
                .checked_add(size)
                .ok_or_else(|| Error::Validation("Transaction source size overflow".into()))?;
            require(
                total <= MAX_SOURCE_BODY_BYTES,
                "Transaction source batch exceeds the 2 MiB retained-body bound",
            )?;
            sizes.push(size);
        }
        drop(metadata);
        let mut rows = Vec::with_capacity(request.rows.len());
        let mut body_query =
            snapshot.prepare("SELECT body FROM records WHERE kind='transaction' AND id=?")?;
        for (key, size) in request.rows.iter().zip(sizes) {
            let body: String = body_query.query_row([&key.id], |row| row.get(0))?;
            require(body.len() as u64 == size, "Transaction source size changed")?;
            let row: crate::domain::Transaction = serde_json::from_str(&body)?;
            require(
                row.id == key.id && row.version > 0,
                "Canonical transaction source identity is invalid",
            )?;
            if key
                .expected_version
                .is_some_and(|version| version != row.version)
            {
                return Err(Error::Conflict(
                    "Transaction source version changed; refresh the originating result".into(),
                ));
            }
            analytics::validate_transaction(&row)?;
            rows.push(row);
        }
        drop(body_query);
        // Shared statement originals are read and hashed only once per batch.
        let sources: BTreeSet<_> = rows.iter().map(|row| row.anchor.evidence_id()).collect();
        for key in sources {
            let evidence: Evidence = get(&snapshot, "evidence", key)?;
            require(evidence.id == key, "Canonical source identity is invalid")?;
            self.verify_original(&evidence)?;
        }
        snapshot.commit()?;
        Ok(TransactionSources {
            schema_version: 1,
            workspace_revision: revision,
            rows,
        })
    }
}

#[cfg(test)]
#[path = "transaction_sources_tests.rs"]
mod tests;
