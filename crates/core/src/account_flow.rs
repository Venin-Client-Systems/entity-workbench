//! Revision-bound internal account flows, derived only from verified reviewed transfer pairs.
use crate::{
    analytics,
    domain::{ReviewState, SourceAnchor, Transaction},
    require,
    transaction_analysis::{self, MoneyTotal, TransactionAnalysisRequest},
    transaction_comparison::TransactionVersion,
    transaction_page::TransactionReviewCounts,
    Result,
};
use rust_decimal::Decimal;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
};

pub const MAX_FLOW_NODES: usize = 1_000;
pub const MAX_FLOW_EDGES: usize = 10_000;
pub const MAX_FLOW_PAIRS: usize = 50_000;
pub const MAX_FLOW_BYTES: usize = 16 * 1024 * 1024;
const MAX_ANCHOR_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AccountFlowRequest {
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub account: Option<String>,
    pub currency: Option<String>,
}
impl AccountFlowRequest {
    pub fn validate(&self) -> Result<()> {
        transaction_analysis::validate_scope(
            self.date_from.as_deref(),
            self.date_to.as_deref(),
            self.account.as_deref(),
            self.currency.as_deref(),
        )
    }
    pub(crate) fn scope(&self) -> TransactionAnalysisRequest {
        TransactionAnalysisRequest {
            date_from: self.date_from.clone(),
            date_to: self.date_to.clone(),
            account: self.account.clone(),
            currency: self.currency.clone(),
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FlowSource {
    pub transaction_id: String,
    pub version: u32,
    pub account: String,
    pub currency: String,
    pub date: String,
    /// Exact canonical source spelling, not a newly rounded amount.
    pub amount: String,
    /// Retained location reference; quote/location validation remains source inspection's job.
    pub anchor: SourceAnchor,
    pub review: ReviewState,
    pub in_scope: bool,
    pub verified_transfer_peer: Option<TransactionVersion>,
    pub unverified_transfer_match: bool,
    pub has_duplicate_candidates: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AccountFlowNode {
    pub id: String,
    pub account: String,
    pub currency: String,
    pub support_only: bool,
    pub scope_count: usize,
    pub review_counts: TransactionReviewCounts,
    /// All accepted selected rows, including the selected endpoints of mapped transfers.
    pub accepted_ledger: MoneyTotal,
    /// Accepted selected rows without a verified reciprocal transfer counterpart.
    pub accepted_unmapped: MoneyTotal,
    pub pending_ids: Vec<String>,
    pub rejected_ids: Vec<String>,
    pub deferred_ids: Vec<String>,
    /// These two overlapping annotations count selected rows in any review state.
    pub unverified_transfer_count: usize,
    pub unverified_transfer_ids: Vec<String>,
    pub duplicate_candidate_count: usize,
    pub duplicate_candidate_ids: Vec<String>,
    /// Out-of-scope sources supporting included edges; never included in selected totals/counts.
    pub support_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AccountFlowPair {
    pub id: String,
    pub debit: TransactionVersion,
    pub credit: TransactionVersion,
    pub debit_date: String,
    pub credit_date: String,
    pub debit_in_scope: bool,
    pub credit_in_scope: bool,
    pub amount: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AccountFlowEdge {
    pub id: String,
    pub currency: String,
    pub debit_node_id: String,
    pub credit_node_id: String,
    /// Sum of positive amounts, once per verified pair, not selected ledger net activity.
    pub amount: String,
    pub pairs: Vec<AccountFlowPair>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AccountFlows {
    pub schema_version: u32,
    pub rules_version: String,
    pub workspace_revision: u64,
    pub request: AccountFlowRequest,
    pub workspace_transaction_count: usize,
    pub scope_transaction_count: usize,
    pub support_transaction_count: usize,
    pub nodes: Vec<AccountFlowNode>,
    pub edges: Vec<AccountFlowEdge>,
    pub sources: Vec<FlowSource>,
    pub limitations: Vec<String>,
}

struct SizeCounter {
    bytes: usize,
    limit: usize,
    exceeded: bool,
}
impl io::Write for SizeCounter {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        let next = self.bytes.checked_add(input.len());
        if next.is_none_or(|n| n > self.limit) {
            self.exceeded = true;
            return Err(io::Error::other(
                "Account-flow serialization exceeds its byte bound",
            ));
        }
        self.bytes = next.unwrap();
        Ok(input.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
/// Count actual JSON escaping without allocating an encoded output buffer.
fn bounded_size(value: &impl Serialize, limit: usize) -> Result<usize> {
    let mut count = SizeCounter {
        bytes: 0,
        limit,
        exceeded: false,
    };
    let result = serde_json::to_writer(&mut count, value);
    require(
        !count.exceeded,
        "Account-flow output exceeds its byte bound; narrow the scope",
    )?;
    result?;
    Ok(count.bytes)
}
fn identifier(parts: &[&str]) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(parts)?)))
}
fn valid_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}
fn validate_flow_row(row: &Transaction) -> Result<()> {
    require(
        valid_id(&row.id) && row.version > 0,
        "Account-flow source identity/version is invalid",
    )?;
    require(
        row.account.len() <= 4000,
        "Account-flow account exceeds its byte bound",
    )?;
    require(
        row.transfer_peer.as_deref().is_none_or(valid_id),
        "Account-flow peer identifier is invalid",
    )?;
    require(
        valid_id(row.anchor.evidence_id()),
        "Account-flow evidence identifier is invalid",
    )?;
    if let SourceAnchor::Page {
        region: Some(region),
        ..
    } = &row.anchor
    {
        require(
            region.iter().all(|v| v.is_finite()),
            "Account-flow region is nonfinite",
        )?;
    }
    bounded_size(&row.anchor, MAX_ANCHOR_BYTES)?;
    Ok(())
}

#[derive(Default)]
struct NodeRows<'a> {
    selected: Vec<&'a Transaction>,
    support: Vec<&'a Transaction>,
}
#[derive(Default)]
struct EdgeRows {
    amount: Decimal,
    pairs: Vec<AccountFlowPair>,
}

/// Pure calculation. Workspace::analyze_account_flows adds canonical key/original verification.
pub fn analyze(
    transactions: &[Transaction],
    revision: u64,
    request: &AccountFlowRequest,
) -> Result<AccountFlows> {
    request.validate()?;
    let lookup = transaction_analysis::validated_lookup(transactions)?;
    for row in transactions {
        validate_flow_row(row)?;
    }
    let scope = request.scope();
    let selected: BTreeSet<_> = lookup
        .values()
        .filter(|row| scope.includes(row))
        .map(|row| row.id.as_str())
        .collect();
    let mut included = selected.clone();
    let mut verified = BTreeMap::new();
    for key in &selected {
        if let Some(peer) = analytics::verified_transfer_peer(lookup[key], &lookup)? {
            included.insert(peer.id.as_str());
            verified.insert(*key, peer);
            verified.insert(peer.id.as_str(), lookup[key]);
        }
    }
    let mut nodes: BTreeMap<(&str, &str), NodeRows<'_>> = BTreeMap::new();
    let mut sources = Vec::with_capacity(included.len());
    for key in &included {
        let row = lookup[key];
        let in_scope = selected.contains(key);
        let node = nodes.entry((&row.account, &row.currency)).or_default();
        if in_scope {
            node.selected.push(row);
        } else {
            node.support.push(row);
        }
        require(
            nodes.len() <= MAX_FLOW_NODES,
            "Account-flow node bound exceeded; narrow the scope",
        )?;
        sources.push(FlowSource {
            transaction_id: row.id.clone(),
            version: row.version,
            account: row.account.clone(),
            currency: row.currency.clone(),
            date: row.date.clone(),
            amount: row.amount.clone(),
            anchor: row.anchor.clone(),
            review: row.review.clone(),
            in_scope,
            verified_transfer_peer: verified
                .get(key)
                .map(|peer| TransactionVersion::from(*peer)),
            unverified_transfer_match: row.transfer_peer.is_some() && !verified.contains_key(key),
            has_duplicate_candidates: !row.duplicate_candidates.is_empty(),
        });
    }
    let mut edge_rows: BTreeMap<(&str, &str, &str), EdgeRows> = BTreeMap::new();
    let mut pair_count = 0;
    for (key, credit) in &verified {
        let debit = lookup[key];
        if analytics::amount(&debit.amount)? >= Decimal::ZERO {
            continue;
        }
        pair_count += 1;
        require(
            pair_count <= MAX_FLOW_PAIRS,
            "Account-flow pair bound exceeded; narrow the scope",
        )?;
        let amount = analytics::amount(&credit.amount)?;
        let edge = edge_rows
            .entry((&debit.currency, &debit.account, &credit.account))
            .or_default();
        edge.amount = analytics::exact_add(edge.amount, amount)?;
        edge.pairs.push(AccountFlowPair {
            id: identifier(&["account_flow_pair_v1", &debit.id, &credit.id])?,
            debit: debit.into(),
            credit: (*credit).into(),
            debit_date: debit.date.clone(),
            credit_date: credit.date.clone(),
            debit_in_scope: selected.contains(debit.id.as_str()),
            credit_in_scope: selected.contains(credit.id.as_str()),
            amount: amount.to_string(),
        });
        require(
            edge_rows.len() <= MAX_FLOW_EDGES,
            "Account-flow edge bound exceeded; narrow the scope",
        )?;
    }
    let mut output_nodes = Vec::with_capacity(nodes.len());
    for ((account, currency), rows) in nodes {
        let ids = |state| {
            rows.selected
                .iter()
                .filter(|row| row.review == state)
                .map(|row| row.id.clone())
                .collect::<Vec<_>>()
        };
        let pending_ids = ids(ReviewState::Pending);
        let rejected_ids = ids(ReviewState::Rejected);
        let deferred_ids = ids(ReviewState::Deferred);
        let accepted_ledger = transaction_analysis::sum_transactions(
            rows.selected
                .iter()
                .copied()
                .filter(|row| row.review == ReviewState::Accepted),
        )?;
        let accepted_unmapped =
            transaction_analysis::sum_transactions(rows.selected.iter().copied().filter(|row| {
                row.review == ReviewState::Accepted && !verified.contains_key(row.id.as_str())
            }))?;
        let unverified_transfer_ids: Vec<_> = rows
            .selected
            .iter()
            .filter(|row| row.transfer_peer.is_some() && !verified.contains_key(row.id.as_str()))
            .map(|row| row.id.clone())
            .collect();
        let duplicate_candidate_ids: Vec<_> = rows
            .selected
            .iter()
            .filter(|row| !row.duplicate_candidates.is_empty())
            .map(|row| row.id.clone())
            .collect();
        output_nodes.push(AccountFlowNode {
            id: identifier(&["account_flow_node_v1", account, currency])?,
            account: account.into(),
            currency: currency.into(),
            support_only: rows.selected.is_empty(),
            scope_count: rows.selected.len(),
            review_counts: TransactionReviewCounts {
                accepted: accepted_ledger.transaction_ids.len() as u64,
                pending: pending_ids.len() as u64,
                rejected: rejected_ids.len() as u64,
                deferred: deferred_ids.len() as u64,
            },
            accepted_ledger,
            accepted_unmapped,
            pending_ids,
            rejected_ids,
            deferred_ids,
            unverified_transfer_count: unverified_transfer_ids.len(),
            unverified_transfer_ids,
            duplicate_candidate_count: duplicate_candidate_ids.len(),
            duplicate_candidate_ids,
            support_ids: rows.support.iter().map(|row| row.id.clone()).collect(),
        });
    }
    let mut edges = Vec::with_capacity(edge_rows.len());
    for ((currency, debit, credit), mut edge) in edge_rows {
        edge.pairs.sort_by(|a, b| {
            (
                &a.debit_date,
                &a.credit_date,
                &a.debit.transaction_id,
                &a.credit.transaction_id,
            )
                .cmp(&(
                    &b.debit_date,
                    &b.credit_date,
                    &b.debit.transaction_id,
                    &b.credit.transaction_id,
                ))
        });
        edges.push(AccountFlowEdge {
            id: identifier(&["account_flow_edge_v1", currency, debit, credit])?,
            currency: currency.into(),
            debit_node_id: identifier(&["account_flow_node_v1", debit, currency])?,
            credit_node_id: identifier(&["account_flow_node_v1", credit, currency])?,
            amount: edge.amount.to_string(),
            pairs: edge.pairs,
        });
    }
    let limitations = vec![
            "Only reciprocal accepted nonzero equal-opposite same-currency transfers between distinct exact account labels form edges. This is an internal reviewed ledger relationship, not inferred external counterparties or beneficial ownership.".into(),
            "A pair is included when either endpoint is selected and counted once from debit account to credit account. Out-of-scope support is identified separately and contributes no selected ledger activity. Edge sums therefore differ from selected ledger net totals.".into(),
            "Accepted ledger totals include mapped transfers. Accepted unmapped totals include every other accepted selected row, including refunds and unverified transfer markers. Pending, rejected and deferred rows remain in selected denominators; duplicate candidates and repeated rows are not removed.".into(),
            "Scope uses inclusive transaction dates and exact account/currency labels; posting dates, automatic period normalization, foreign-exchange conversion and merchant inference are not used.".into(),
            "Source IDs, versions and retained anchors belong to this revision. Original bytes are verified by the workspace reader; anchor locations and quotes require source inspection and are not newly accepted coordinates or findings.".into(),
            "The complete ledger is validated within fixed bounds. Narrowing scope does not bypass malformed or oversized retained inputs; no indexed latency or hard memory bound is claimed.".into(),
        ];
    let result = AccountFlows {
        schema_version: 1,
        rules_version: "reviewed_internal_account_flows_v1".into(),
        workspace_revision: revision,
        request: request.clone(),
        workspace_transaction_count: transactions.len(),
        scope_transaction_count: selected.len(),
        support_transaction_count: included.len() - selected.len(),
        nodes: output_nodes,
        edges,
        sources,
        limitations,
    };
    bounded_size(&result, MAX_FLOW_BYTES)?;
    Ok(result)
}

#[cfg(test)]
#[path = "account_flow_tests.rs"]
mod tests;
