//! One fixed, direct-only SQLite function; no analyst SQL or locale-dependent casing.
use super::*;
use crate::literal_search::{lower_query, LiteralMatching};
use crate::transaction_search::*;
use rusqlite::{functions::FunctionFlags, types::ValueRef};

pub(super) fn register_matcher(connection: &Connection) -> Result<()> {
    connection.create_scalar_function(
        "ew_transaction_text_match_v1",
        4,
        FunctionFlags::SQLITE_UTF8
            | FunctionFlags::SQLITE_DETERMINISTIC
            | FunctionFlags::SQLITE_DIRECTONLY,
        |context| {
            let text = |index| match context.get_raw(index) {
                ValueRef::Text(bytes) => std::str::from_utf8(bytes).map_err(|_| {
                    Error::Validation("Canonical search field is not UTF-8 text".into())
                }),
                _ => Err(Error::Validation(
                    "Canonical search field is missing or not text".into(),
                )),
            };
            let run = || matches_lowered(text(0)?, text(1)?, text(2)?, text(3)?);
            run().map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))
        },
    )?;
    Ok(())
}

impl Workspace {
    pub fn search_transactions(
        &self,
        request: &TransactionSearchRequest,
        expected_revision: u64,
    ) -> Result<TransactionSearchPage> {
        request.validate()?;
        let lowered = lower_query(&request.query);
        let matching = LiteralMatching::default();
        let page = self.transaction_page_with_search(
            &request.page,
            expected_revision,
            Some((&lowered, &matching)),
        )?;
        Ok(TransactionSearchPage {
            schema_version: 1,
            matching,
            page,
        })
    }
}

#[cfg(test)]
#[path = "transaction_search_tests.rs"]
mod tests;
