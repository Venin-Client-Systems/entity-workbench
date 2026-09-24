//! One explicit export reads a single snapshot and never substitutes a visible page.
use super::*;
use crate::{
    literal_search::{lower_query, LiteralMatching},
    transaction_export::*,
    transaction_page::TransactionPageOrder,
    transaction_search::matches_lowered,
};
use std::collections::BTreeSet;
use std::io;

struct LimitedJson {
    bytes: Vec<u8>,
    limit: usize,
}
impl io::Write for LimitedJson {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        self.bytes
            .len()
            .checked_add(input.len())
            .filter(|count| *count <= self.limit)
            .ok_or_else(|| {
                io::Error::other(
                    "Complete transaction export exceeds its JSON byte limit; narrow the scope",
                )
            })?;
        self.bytes.extend_from_slice(input);
        Ok(input.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn encode(rows: &[crate::domain::Transaction], limit: usize) -> Result<String> {
    let mut writer = LimitedJson {
        bytes: Vec::new(),
        limit,
    };
    serde_json::to_writer_pretty(&mut writer, rows)?;
    String::from_utf8(writer.bytes)
        .map_err(|_| Error::Validation("Transaction JSON export is not UTF-8".into()))
}
impl Workspace {
    pub fn export_transactions(
        &self,
        request: &TransactionExportRequest,
        expected_revision: u64,
    ) -> Result<TransactionExport> {
        request.validate()?;
        let snapshot = self.conn.unchecked_transaction()?;
        let revision = self.revision()?;
        if revision != expected_revision {
            return Err(Error::Conflict(
                "Transaction export revision changed; refresh and apply the export scope again"
                    .into(),
            ));
        }
        let lowered = lower_query(&request.query);
        let matching = LiteralMatching::default();
        let query_sha256 = hash(&serde_json::to_vec(&(
            "transaction-export-v1",
            revision,
            &request.filter,
            request.order,
            &matching,
            &lowered,
        ))?);
        let scope = request.filter.scope();
        let mut statement = snapshot
            .prepare("SELECT id,body FROM records WHERE kind='transaction' ORDER BY sequence")?;
        let mut records = statement.query([])?;
        let mut selected = Vec::new();
        while let Some(record) = records.next()? {
            let key: String = record.get(0)?;
            let body: String = record.get(1)?;
            let row: crate::domain::Transaction = serde_json::from_str(&body)?;
            require(
                key == row.id && row.version > 0,
                "Canonical export transaction identity is invalid",
            )?;
            analytics::validate_transaction(&row)?;
            if !scope.includes(&row) {
                continue;
            }
            // Like search, text validation precedes review selection; a malformed
            // scoped field cannot be hidden by a nonmatching query/review filter.
            if !lowered.is_empty()
                && !matches_lowered(&row.description, &row.account, &row.date, &lowered)?
            {
                continue;
            }
            if request
                .filter
                .review
                .as_ref()
                .is_some_and(|state| state != &row.review)
            {
                continue;
            }
            selected.push(row);
        }
        drop(records);
        drop(statement);
        // Stable sorting retains canonical source insertion order for equal dates.
        selected.sort_by(|left, right| match request.order {
            TransactionPageOrder::DateAscending => left.date.cmp(&right.date),
            TransactionPageOrder::DateDescending => right.date.cmp(&left.date),
        });
        let sources: BTreeSet<_> = selected
            .iter()
            .map(|row| row.anchor.evidence_id())
            .collect();
        for key in sources {
            let evidence: Evidence = get(&snapshot, "evidence", key)?;
            require(
                evidence.id == key,
                "Canonical export source identity is invalid",
            )?;
            self.verify_original(&evidence)?;
        }
        let json = encode(&selected, MAX_EXPORT_JSON_BYTES)?;
        let response = TransactionExport {
            schema_version: 1,
            workspace_revision: revision,
            request: request.clone(),
            matching,
            query_sha256,
            row_count: selected.len() as u64,
            bytes: json.len() as u64,
            sha256: hash(json.as_bytes()),
            json,
        };
        snapshot.commit()?;
        Ok(response)
    }
}
#[cfg(test)]
#[path = "transaction_export_tests.rs"]
mod tests;
