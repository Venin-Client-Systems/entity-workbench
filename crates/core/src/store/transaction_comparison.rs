use super::*;
use crate::transaction_comparison::{self, TransactionComparison, TransactionComparisonRequest};

impl Workspace {
    /// Both periods and transfer counterparts come from one read snapshot; no state is written.
    pub fn compare_transaction_periods(
        &self,
        request: &TransactionComparisonRequest,
        expected_revision: u64,
    ) -> Result<TransactionComparison> {
        request.validate()?;
        let transaction = self.conn.unchecked_transaction()?;
        let revision = self.revision()?;
        if revision != expected_revision {
            return Err(Error::Conflict(
                "Transaction comparison revision changed; refresh the workspace before recalculating".into(),
            ));
        }
        let rows = self.transaction_analysis_rows(|row| request.includes(row))?;
        let result = transaction_comparison::compare(&rows, revision, request)?;
        transaction.commit()?;
        Ok(result)
    }
}
