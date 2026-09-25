//! One revision-bound snapshot; no canonical mutations or inferred transfer matches.
use super::*;
use crate::account_flow::{self, AccountFlowRequest, AccountFlows};

const MAX_FLOW_RECORD_BYTES: u64 = 2 * 1024 * 1024;
const MAX_FLOW_LEDGER_BYTES: u64 = 64 * 1024 * 1024;

impl Workspace {
    pub fn analyze_account_flows(
        &self,
        request: &AccountFlowRequest,
        expected_revision: u64,
    ) -> Result<AccountFlows> {
        request.validate()?;
        let snapshot = self.conn.unchecked_transaction()?;
        let revision = self.revision()?;
        if revision != expected_revision {
            return Err(Error::Conflict(
                "Account-flow revision changed; refresh before recalculating".into(),
            ));
        }
        let count: u64 = snapshot.query_row(
            "SELECT count(*) FROM records WHERE kind='transaction'",
            [],
            |row| row.get(0),
        )?;
        require(
            count <= crate::transaction_analysis::MAX_ANALYSIS_ROWS as u64,
            "Account-flow ledger exceeds the 100,000-row bound",
        )?;
        // Project lengths/types/equality before Rust allocates any retained transaction body.
        // The new flow contract is stricter without changing historical analysis readers.
        let mut statement = snapshot.prepare("SELECT length(CAST(id AS BLOB)),length(CAST(body AS BLOB)),json_type(body,'$.id'),length(CAST(json_extract(body,'$.id') AS BLOB)),CAST(id AS BLOB)=CAST(json_extract(body,'$.id') AS BLOB),json_type(body,'$.version'),json_extract(body,'$.version') FROM records WHERE kind='transaction' ORDER BY sequence")?;
        let mut records = statement.query([])?;
        let mut total = 0u64;
        while let Some(record) = records.next()? {
            let key_bytes: u64 = record.get(0)?;
            let body_bytes: u64 = record.get(1)?;
            let id_type: Option<String> = record.get(2)?;
            let id_bytes: Option<u64> = record.get(3)?;
            let equal: Option<bool> = record.get(4)?;
            let version_type: Option<String> = record.get(5)?;
            require(
                (1..=256).contains(&key_bytes)
                    && id_type.as_deref() == Some("text")
                    && id_bytes == Some(key_bytes)
                    && equal == Some(true)
                    && version_type.as_deref() == Some("integer"),
                "Canonical account-flow transaction identity is invalid",
            )?;
            let version: u64 = record.get(6)?;
            require(
                (1..=u32::MAX as u64).contains(&version),
                "Canonical account-flow transaction version is invalid",
            )?;
            require(
                body_bytes <= MAX_FLOW_RECORD_BYTES,
                "Account-flow retained transaction exceeds the 2 MiB bound",
            )?;
            total = total
                .checked_add(body_bytes)
                .ok_or_else(|| Error::Validation("Account-flow ledger size overflow".into()))?;
            require(
                total <= MAX_FLOW_LEDGER_BYTES,
                "Account-flow retained ledger exceeds the 64 MiB bound",
            )?;
        }
        drop(records);
        drop(statement);
        let scope = request.scope();
        // Includes out-of-scope recorded counterparts. Each distinct original is verified once.
        let rows = self.transaction_analysis_rows(|row| scope.includes(row))?;
        let result = account_flow::analyze(&rows, revision, request)?;
        snapshot.commit()?;
        Ok(result)
    }
}

#[cfg(test)]
#[path = "account_flow_tests.rs"]
mod tests;
