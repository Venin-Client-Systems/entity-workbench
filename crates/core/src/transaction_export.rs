//! Explicit complete-scope JSON export, separate from a visible ledger page.
use crate::{
    literal_search::LiteralMatching, require, transaction_page::*,
    transaction_search::MAX_SEARCH_QUERY_BYTES, Result,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const MAX_EXPORT_JSON_BYTES: usize = 256 * 1024 * 1024;
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransactionExportRequest {
    pub query: String,
    pub filter: TransactionPageFilter,
    pub order: TransactionPageOrder,
}
impl TransactionExportRequest {
    pub fn validate(&self) -> Result<()> {
        require(
            self.query.len() <= MAX_SEARCH_QUERY_BYTES,
            "Transaction export query exceeds 1024 UTF-8 bytes",
        )?;
        TransactionPageRequest {
            filter: self.filter.clone(),
            order: self.order,
            ..Default::default()
        }
        .validate()
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransactionExport {
    pub schema_version: u32,
    pub workspace_revision: u64,
    pub request: TransactionExportRequest,
    pub matching: LiteralMatching,
    /// Export identity, distinct from a page's page-size-bound query identity.
    pub query_sha256: String,
    pub row_count: u64,
    pub bytes: u64,
    pub sha256: String,
    /// Complete pretty-printed array of exact canonical transactions, including all review states selected by the request.
    pub json: String,
}
