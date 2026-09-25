//! Small, revision-bound reconciliation decorations for selected ledger rows.
use crate::{require, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MAX_BALANCE_ROWS: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransactionBalanceKey {
    pub id: String,
    pub expected_version: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransactionBalancesRequest {
    pub rows: Vec<TransactionBalanceKey>,
}
impl TransactionBalancesRequest {
    pub fn validate(&self) -> Result<()> {
        require(
            (1..=MAX_BALANCE_ROWS).contains(&self.rows.len()),
            "Balance request must contain between 1 and 200 rows",
        )?;
        let mut seen = BTreeSet::new();
        for row in &self.rows {
            identifier(&row.id)?;
            require(
                row.expected_version > 0,
                "Balance row version must be positive",
            )?;
            require(seen.insert(&row.id), "Duplicate balance row identifier")?;
        }
        Ok(())
    }
}
pub(crate) fn identifier(value: &str) -> Result<()> {
    require(
        !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control),
        "Balance row identifier must contain 1 to 256 bytes without controls",
    )
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum TransactionBalanceState {
    /// This row has no retained available balance; it can still contribute movement.
    NoBalance,
    /// This is the first retained balance for its source/account/currency group.
    NoPriorBalance,
    Checked {
        previous_id: String,
        previous_version: u32,
        contributing_row_count: u64,
        difference: String,
        reconciled: bool,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransactionBalanceRow {
    pub id: String,
    pub version: u32,
    pub balance: TransactionBalanceState,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransactionBalances {
    pub schema_version: u32,
    pub workspace_revision: u64,
    /// Exact requested set in request order. No partial successful result.
    pub rows: Vec<TransactionBalanceRow>,
}
