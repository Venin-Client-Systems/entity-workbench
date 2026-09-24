//! Exact whole-ledger selector values, independent of the currently displayed rows.
use crate::{require, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const MAX_FACET_ROWS: u32 = 100;
pub const MAX_FACET_VALUE_BYTES: u64 = 4000;
pub const MAX_FACET_PAGE_BYTES: u64 = 256 * 1024;
pub const MAX_FACET_CURSOR_BYTES: usize = 2048;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransactionFacetKind {
    Account,
    Currency,
}
impl TransactionFacetKind {
    pub(crate) fn path(self) -> &'static str {
        match self {
            Self::Account => "$.account",
            Self::Currency => "$.currency",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransactionFacetRequest {
    pub facet: TransactionFacetKind,
    pub page_size: u32,
    pub cursor: Option<String>,
}
impl TransactionFacetRequest {
    pub fn validate(&self) -> Result<()> {
        require(
            (1..=MAX_FACET_ROWS).contains(&self.page_size),
            "Transaction facet page size must be between 1 and 100",
        )?;
        require(
            self.cursor
                .as_ref()
                .is_none_or(|value| !value.is_empty() && value.len() <= MAX_FACET_CURSOR_BYTES),
            "Transaction facet cursor is empty or exceeds its bound",
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TransactionFacetValue {
    pub value: String,
    pub transaction_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransactionFacetPage {
    pub schema_version: u32,
    pub workspace_revision: u64,
    pub facet: TransactionFacetKind,
    pub query_sha256: String,
    /// Every transaction, irrespective of review state or applied ledger filters.
    pub transaction_count: u64,
    pub distinct_count: u64,
    /// Exact values in SQLite BINARY text order; no case or whitespace normalization.
    pub values: Vec<TransactionFacetValue>,
    pub next_cursor: Option<String>,
}
