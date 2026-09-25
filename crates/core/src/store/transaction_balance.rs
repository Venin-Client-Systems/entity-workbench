//! Retain full source-order reconciliation while projecting only selected states.
use super::*;
use crate::transaction_balance::*;
use std::collections::BTreeMap;

impl Workspace {
    pub fn read_transaction_balances(
        &self,
        request: &TransactionBalancesRequest,
        expected_revision: u64,
    ) -> Result<TransactionBalances> {
        request.validate()?;
        let snapshot = self.conn.unchecked_transaction()?;
        let revision = self.revision()?;
        if revision != expected_revision {
            return Err(Error::Conflict(
                "Balance revision changed; refresh the originating ledger page".into(),
            ));
        }
        // The legacy calculator requires every canonical row in insertion order.
        // This is an output projection, not bounded internal memory or SQL work.
        let mut query = snapshot
            .prepare("SELECT id,body FROM records WHERE kind='transaction' ORDER BY sequence")?;
        let mut records = query.query([])?;
        let mut transactions: Vec<crate::domain::Transaction> = Vec::new();
        let mut positions = BTreeMap::new();
        while let Some(record) = records.next()? {
            let key: String = record.get(0)?;
            let body: String = record.get(1)?;
            let row: crate::domain::Transaction = serde_json::from_str(&body)?;
            identifier(&key)?;
            require(
                key == row.id && row.version > 0,
                "Canonical balance row identity is invalid",
            )?;
            require(
                positions.insert(key, transactions.len()).is_none(),
                "Duplicate canonical balance row",
            )?;
            transactions.push(row);
        }
        drop(records);
        drop(query);
        // Resolve the entire request before constructing any returned states.
        let mut selected = Vec::with_capacity(request.rows.len());
        for key in &request.rows {
            let index = positions
                .get(&key.id)
                .ok_or_else(|| Error::Validation("A selected balance row is unavailable".into()))?;
            let row = &transactions[*index];
            if row.version != key.expected_version {
                return Err(Error::Conflict(
                    "Balance row version changed; refresh the originating ledger page".into(),
                ));
            }
            selected.push(row);
        }
        // Reuse exactly the established calculation: all review states, separate
        // source/account/currency windows, and every intervening movement.
        let calculated = analytics::analyse(&transactions)?;
        let checks: BTreeMap<_, _> = calculated
            .balance_checks
            .iter()
            .map(|check| (check.transaction_id.as_str(), check))
            .collect();
        let mut rows = Vec::with_capacity(selected.len());
        for row in selected {
            let balance = if let Some(check) = checks.get(row.id.as_str()) {
                let previous = positions.get(&check.previous_id).ok_or_else(|| {
                    Error::Validation("Balance predecessor is unavailable".into())
                })?;
                TransactionBalanceState::Checked {
                    previous_id: check.previous_id.clone(),
                    previous_version: transactions[*previous].version,
                    contributing_row_count: check.transaction_ids.len() as u64,
                    difference: check.difference.clone(),
                    reconciled: check.reconciled,
                }
            } else if row.balance.is_some() {
                TransactionBalanceState::NoPriorBalance
            } else {
                TransactionBalanceState::NoBalance
            };
            rows.push(TransactionBalanceRow {
                id: row.id.clone(),
                version: row.version,
                balance,
            });
        }
        snapshot.commit()?;
        Ok(TransactionBalances {
            schema_version: 1,
            workspace_revision: revision,
            rows,
        })
    }
}

#[cfg(test)]
#[path = "transaction_balance_tests.rs"]
mod tests;
