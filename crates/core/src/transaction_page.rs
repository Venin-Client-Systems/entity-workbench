//! Revision-bound canonical ledger pages. No monetary aggregation or classifications.
use crate::{
    domain::{ReviewState, Transaction},
    require,
    transaction_analysis::TransactionAnalysisRequest,
    Result,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const MAX_PAGE_ROWS: u32 = 200;
pub const MAX_PAGE_BODY_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_CURSOR_BYTES: usize = 2048;

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TransactionPageFilter {
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub account: Option<String>,
    pub currency: Option<String>,
    pub review: Option<ReviewState>,
}
impl TransactionPageFilter {
    pub(crate) fn scope(&self) -> TransactionAnalysisRequest {
        TransactionAnalysisRequest {
            date_from: self.date_from.clone(),
            date_to: self.date_to.clone(),
            account: self.account.clone(),
            currency: self.currency.clone(),
            ..Default::default()
        }
    }
    pub(crate) fn review_name(&self) -> Option<&'static str> {
        self.review.as_ref().map(|state| match state {
            ReviewState::Accepted => "accepted",
            ReviewState::Pending => "pending",
            ReviewState::Rejected => "rejected",
            ReviewState::Deferred => "deferred",
        })
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransactionPageOrder {
    DateAscending,
    DateDescending,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransactionPageRequest {
    pub filter: TransactionPageFilter,
    pub order: TransactionPageOrder,
    pub page_size: u32,
    pub cursor: Option<String>,
}
impl Default for TransactionPageRequest {
    fn default() -> Self {
        Self {
            filter: Default::default(),
            order: TransactionPageOrder::DateAscending,
            page_size: 100,
            cursor: None,
        }
    }
}
impl TransactionPageRequest {
    pub fn validate(&self) -> Result<()> {
        crate::transaction_analysis::validate_scope(
            self.filter.date_from.as_deref(),
            self.filter.date_to.as_deref(),
            self.filter.account.as_deref(),
            self.filter.currency.as_deref(),
        )?;
        require(
            self.filter
                .account
                .as_ref()
                .is_none_or(|value| !value.chars().any(char::is_control)),
            "Page account cannot contain control characters",
        )?;
        validate_window(self.page_size, self.cursor.as_deref())
    }
}
pub(crate) fn validate_window(page_size: u32, cursor: Option<&str>) -> Result<()> {
    require(
        (1..=MAX_PAGE_ROWS).contains(&page_size),
        "Transaction page size must be between 1 and 200",
    )?;
    require(
        cursor.is_none_or(|cursor| !cursor.is_empty() && cursor.len() <= MAX_CURSOR_BYTES),
        "Transaction cursor is empty or exceeds its bound",
    )
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TransactionReviewCounts {
    pub accepted: u64,
    pub pending: u64,
    pub rejected: u64,
    pub deferred: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransactionPage {
    pub schema_version: u32,
    pub workspace_revision: u64,
    pub query_sha256: String,
    /// Date/account/currency scope, before the optional review-state selection.
    pub scope_count: u64,
    pub review_counts: TransactionReviewCounts,
    pub selected_count: u64,
    /// Exact canonical rows, including original decimal strings and source anchors.
    pub rows: Vec<Transaction>,
    pub next_cursor: Option<String>,
}
