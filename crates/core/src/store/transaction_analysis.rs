use super::*;
use crate::transaction_analysis::{self, TransactionAnalysis, TransactionAnalysisRequest};
use std::collections::BTreeSet;

impl Workspace {
    /// Read one SQLite snapshot; analysis never writes classifications, observations or reports.
    pub fn analyze_transactions(
        &self,
        request: &TransactionAnalysisRequest,
        expected_revision: u64,
    ) -> Result<TransactionAnalysis> {
        request.validate()?;
        let transaction = self.conn.unchecked_transaction()?;
        let revision = self.revision()?;
        if revision != expected_revision {
            return Err(Error::Conflict(
                "Transaction analysis revision changed; refresh the workspace before recalculating"
                    .into(),
            ));
        }
        let count: usize = self.conn.query_row(
            "SELECT count(*) FROM records WHERE kind='transaction'",
            [],
            |row| row.get(0),
        )?;
        require(
            count <= transaction_analysis::MAX_ANALYSIS_ROWS,
            "Transaction analysis exceeds the 100,000-row bound; analytical pagination is required",
        )?;
        let rows = all::<crate::domain::Transaction>(&self.conn, "transaction")?;
        // Include recorded counterpart sources: a filtered-out peer still supports an exclusion.
        let peers: BTreeSet<_> = rows
            .iter()
            .filter(|t| request.includes(t))
            .filter_map(|t| t.transfer_peer.as_deref())
            .collect();
        let sources: BTreeSet<_> = rows
            .iter()
            .filter(|t| request.includes(t) || peers.contains(t.id.as_str()))
            .map(|t| t.anchor.evidence_id())
            .collect();
        for key in sources {
            self.verify_original(&get::<Evidence>(&self.conn, "evidence", key)?)?;
        }
        let result = transaction_analysis::analyze(&rows, revision, request)?;
        transaction.commit()?;
        Ok(result)
    }
}
