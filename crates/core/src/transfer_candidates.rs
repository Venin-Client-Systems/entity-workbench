//! Broad, analyst-filtered counterpart suggestions; canonical matching is authoritative.
use crate::{
    domain::ReviewState, literal_search::LiteralMatching, require, transaction_page::*, Result,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransferCandidateFilter {
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub account: Option<String>,
    pub currency: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransferCandidatesRequest {
    pub target_id: String,
    pub expected_target_version: u32,
    pub query: String,
    pub filter: TransferCandidateFilter,
    pub order: TransactionPageOrder,
    pub page_size: u32,
    pub cursor: Option<String>,
}
impl TransferCandidatesRequest {
    pub fn validate(&self) -> Result<()> {
        require(
            !self.target_id.is_empty()
                && self.target_id.len() <= 256
                && !self.target_id.chars().any(char::is_control),
            "Transfer target identifier must contain 1 to 256 bytes without controls",
        )?;
        require(
            self.expected_target_version > 0,
            "Transfer target version must be positive",
        )?;
        require(
            self.query.len() <= crate::transaction_search::MAX_SEARCH_QUERY_BYTES,
            "Transfer query exceeds 1024 UTF-8 bytes",
        )?;
        // Borrowed scope bounds must precede construction of the shared request.
        crate::transaction_analysis::validate_scope(
            self.filter.date_from.as_deref(),
            self.filter.date_to.as_deref(),
            self.filter.account.as_deref(),
            self.filter.currency.as_deref(),
        )?;
        crate::transaction_page::validate_window(self.page_size, self.cursor.as_deref())?;
        self.page_request().validate()
    }
    pub(crate) fn page_request(&self) -> TransactionPageRequest {
        TransactionPageRequest {
            filter: TransactionPageFilter {
                date_from: self.filter.date_from.clone(),
                date_to: self.filter.date_to.clone(),
                account: self.filter.account.clone(),
                currency: self.filter.currency.clone(),
                review: Some(ReviewState::Accepted),
            },
            order: self.order,
            page_size: self.page_size,
            cursor: self.cursor.clone(),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransferCandidatesPage {
    pub schema_version: u32,
    pub target_id: String,
    pub target_version: u32,
    pub matching: LiteralMatching,
    /// Other-account scope before review selection; selected_count is accepted rows.
    pub page: TransactionPage,
}
