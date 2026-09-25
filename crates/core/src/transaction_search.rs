//! Literal Unicode-lowercase search; source text and financial semantics stay intact.
use crate::{analytics, literal_search::*, require, transaction_page::*, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const MAX_SEARCH_QUERY_BYTES: usize = 1024;
pub const MAX_SEARCH_FIELD_BYTES: usize = 4000;
const MAX_LOWERED_QUERY_BYTES: usize = MAX_SEARCH_QUERY_BYTES * 4;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransactionSearchRequest {
    pub query: String,
    pub page: TransactionPageRequest,
}
impl TransactionSearchRequest {
    pub fn validate(&self) -> Result<()> {
        require(
            self.query.len() <= MAX_SEARCH_QUERY_BYTES,
            "Transaction search query exceeds 1024 UTF-8 bytes",
        )?;
        self.page.validate()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransactionSearchPage {
    pub schema_version: u32,
    pub matching: LiteralMatching,
    /// Text is an additional scope dimension before the page's review selection.
    pub page: TransactionPage,
}

pub(crate) fn matches_lowered(
    description: &str,
    account: &str,
    date: &str,
    lowered_query: &str,
) -> Result<bool> {
    // Check all borrowed fields before creating concatenated or lowered strings.
    require(
        (1..=MAX_SEARCH_FIELD_BYTES).contains(&description.len())
            && (1..=MAX_SEARCH_FIELD_BYTES).contains(&account.len())
            && date.len() == 10
            && lowered_query.len() <= MAX_LOWERED_QUERY_BYTES,
        "Transaction search text fields are empty or exceed their byte bounds",
    )?;
    require(
        !description.trim().is_empty() && !account.trim().is_empty(),
        "Transaction search description and account cannot be blank",
    )?;
    analytics::date(date)?;
    let mut text = String::with_capacity(description.len() + account.len() + date.len() + 2);
    text.push_str(description);
    text.push(' ');
    text.push_str(account);
    text.push(' ');
    text.push_str(date);
    Ok(matches_text(&text, lowered_query))
}
