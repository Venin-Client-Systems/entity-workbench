//! Small ordered reads for already selected transaction sources.
use crate::{domain::Transaction, require, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MAX_SOURCE_ROWS: usize = 25;
pub const MAX_SOURCE_BODY_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransactionSourceKey {
    pub id: String,
    /// Required for analysis-derived rows. None selects the current row at the
    /// independently required workspace revision, for example a finding citation.
    pub expected_version: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransactionSourcesRequest {
    pub rows: Vec<TransactionSourceKey>,
}
impl TransactionSourcesRequest {
    pub fn validate(&self) -> Result<()> {
        require(
            (1..=MAX_SOURCE_ROWS).contains(&self.rows.len()),
            "Transaction source request must contain between 1 and 25 rows",
        )?;
        let mut seen = BTreeSet::new();
        for row in &self.rows {
            require(
                !row.id.is_empty() && row.id.len() <= 256 && !row.id.chars().any(char::is_control),
                "Transaction source identifier must contain 1 to 256 bytes without controls",
            )?;
            require(
                seen.insert(&row.id),
                "Duplicate transaction source identifier",
            )?;
            require(
                row.expected_version.is_none_or(|version| version > 0),
                "Transaction source version must be positive",
            )?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransactionSources {
    pub schema_version: u32,
    pub workspace_revision: u64,
    /// Exact canonical rows in request order; a failure returns no partial batch.
    pub rows: Vec<Transaction>,
}
