//! Target resolution and the shared ledger paging engine use the same snapshot.
use super::*;
use crate::{
    literal_search::{lower_query, LiteralMatching},
    transfer_candidates::*,
};

pub(super) fn resolve_target(
    snapshot: &Connection,
    key: &str,
    version: u32,
) -> Result<crate::domain::Transaction> {
    let bytes: Option<u64> = snapshot
        .query_row(
            "SELECT length(CAST(body AS BLOB)) FROM records WHERE kind='transaction' AND id=?",
            [key],
            |row| row.get(0),
        )
        .optional()?;
    let bytes = bytes.ok_or_else(|| Error::Validation("Transfer target is unavailable".into()))?;
    require(
        bytes <= crate::transaction_page::MAX_PAGE_BODY_BYTES as u64,
        "Transfer target exceeds the 2 MiB retained-body bound",
    )?;
    let body: String = snapshot.query_row(
        "SELECT body FROM records WHERE kind='transaction' AND id=?",
        [key],
        |row| row.get(0),
    )?;
    require(
        body.len() as u64 == bytes,
        "Transfer target size changed inside snapshot",
    )?;
    let target: crate::domain::Transaction = serde_json::from_str(&body)?;
    require(
        target.id == key && target.version > 0,
        "Canonical transfer target identity is invalid",
    )?;
    if target.version != version {
        return Err(Error::Conflict(
            "Transfer target version changed; refresh the originating row".into(),
        ));
    }
    analytics::validate_transaction(&target)?;
    Ok(target)
}
impl Workspace {
    pub fn page_transfer_candidates(
        &self,
        request: &TransferCandidatesRequest,
        expected_revision: u64,
    ) -> Result<TransferCandidatesPage> {
        request.validate()?;
        let matching = LiteralMatching::default();
        let lowered = lower_query(&request.query);
        let page = self.transaction_page_with_context(
            &request.page_request(),
            expected_revision,
            Some((&lowered, &matching)),
            super::transaction_page::PageContext::Transfer {
                target_id: &request.target_id,
                expected_version: request.expected_target_version,
            },
        )?;
        Ok(TransferCandidatesPage {
            schema_version: 1,
            target_id: request.target_id.clone(),
            target_version: request.expected_target_version,
            matching,
            page,
        })
    }
}
#[cfg(test)]
#[path = "transfer_candidates_tests.rs"]
mod tests;
