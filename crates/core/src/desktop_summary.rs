//! Opt-in response projection. This is not a replacement for canonical records.
use crate::{
    domain::*,
    statements::{StatementImport, StatementProfile},
    transaction_page::TransactionReviewCounts,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DesktopSummaryResponse {
    pub schema_version: u32,
    pub workspace: DesktopWorkspace,
    pub analysis: LedgerSummary,
}

/// Presentation metadata, without the complete transaction and generic decision arrays.
/// Evidence text and statement import transaction IDs are still retained.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DesktopWorkspace {
    pub schema_version: u32,
    pub revision: u64,
    pub review_decision_count: u64,
    pub entities: Vec<Entity>,
    pub evidence: Vec<Evidence>,
    pub observations: Vec<Observation>,
    pub assertions: Vec<Assertion>,
    pub addresses: Vec<AddressAssociation>,
    pub locations: Vec<MerchantLocation>,
    pub leads: Vec<Lead>,
    pub jobs: Vec<CollectionJob>,
    pub findings: Vec<Finding>,
    pub hypotheses: Vec<Hypothesis>,
    pub merges: Vec<MergeDecision>,
    pub identity_decisions: Vec<IdentityDecision>,
    pub reports: Vec<ReportMetadata>,
    pub statement_profiles: Vec<StatementProfile>,
    pub statement_imports: Vec<StatementImport>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LedgerSummary {
    pub transaction_count: u64,
    pub review_counts: TransactionReviewCounts,
    /// Number of rows with candidate duplicates, not distinct pairs or confirmed duplicates.
    pub duplicate_candidate_row_count: u64,
    /// Source-order checks, including rows in every review state.
    pub balance_check_count: u64,
    pub balance_discrepancy_count: u64,
    pub totals: Vec<ReviewedCurrencySummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReviewedCurrencySummary {
    pub currency: String,
    pub credits: String,
    pub debits: String,
    pub net: String,
    pub included_count: u64,
    pub excluded_transfer_count: u64,
}
