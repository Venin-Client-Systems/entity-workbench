//! One explicit export reads a single snapshot and never substitutes a visible page.
use super::*;
use crate::transaction_export::*;
#[cfg(test)]
use crate::{literal_search::LiteralMatching, transaction_page::TransactionPageOrder};
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
        self.with_transaction_export_selection(request, expected_revision, |selection| {
            let json = encode(&selection.rows, MAX_EXPORT_JSON_BYTES)?;
            Ok(TransactionExport {
                schema_version: 1,
                workspace_revision: selection.revision,
                request: request.clone(),
                matching: selection.matching,
                query_sha256: selection.query_sha256,
                row_count: selection.rows.len() as u64,
                bytes: json.len() as u64,
                sha256: hash(json.as_bytes()),
                json,
            })
        })
    }
}

#[cfg(test)]
#[path = "transaction_export_tests.rs"]
mod tests;
