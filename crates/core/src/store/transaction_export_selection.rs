//! Shared complete-scope capture; formatting happens before this snapshot closes.
use super::*;
use crate::{
    literal_search::{lower_query, LiteralMatching},
    transaction_export::TransactionExportRequest,
    transaction_page::TransactionPageOrder,
    transaction_search::matches_lowered,
};
use std::collections::BTreeSet;

pub(super) struct ExportSelection {
    pub revision: u64,
    pub matching: LiteralMatching,
    pub query_sha256: String,
    pub rows: Vec<crate::domain::Transaction>,
}
impl Workspace {
    pub(super) fn with_transaction_export_selection<T>(
        &self,
        request: &TransactionExportRequest,
        expected_revision: u64,
        finish: impl FnOnce(ExportSelection) -> Result<T>,
    ) -> Result<T> {
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
        let response = finish(ExportSelection {
            revision,
            matching,
            query_sha256,
            rows: selected,
        })?;
        snapshot.commit()?;
        Ok(response)
    }
}
