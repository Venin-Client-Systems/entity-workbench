use super::*;
use crate::transaction_csv::*;
impl Workspace {
    pub fn export_transaction_csv(
        &self,
        request: &TransactionCsvRequest,
        expected_revision: u64,
    ) -> Result<TransactionCsvExport> {
        self.with_transaction_export_selection(&request.selection, expected_revision, |selection| {
            let csv = encode(
                &selection.rows,
                selection.revision,
                request.non_accepted,
                MAX_CSV_BYTES,
            )?;
            let format = TransactionCsvFormat::TypedLiteralV1;
            let dictionary = dictionary();
            // Hash the sorted-key JSON Value, independent of DTO/IPC object field ordering.
            let format_sha256 = hash(&serde_json::to_vec(&serde_json::to_value((
                format,
                &dictionary,
            ))?)?);
            Ok(TransactionCsvExport {
                schema_version: 1,
                workspace_revision: selection.revision,
                request: request.clone(),
                matching: selection.matching,
                selection_sha256: selection.query_sha256,
                format,
                format_sha256,
                dictionary,
                row_count: selection.rows.len() as u64,
                bytes: csv.len() as u64,
                sha256: hash(csv.as_bytes()),
                csv,
            })
        })
    }
}
#[cfg(test)]
#[path = "transaction_csv_tests.rs"]
mod tests;
