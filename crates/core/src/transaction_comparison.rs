//! Exact comparisons of two analyst-selected periods over one canonical ledger revision.
use crate::{
    analytics,
    domain::Transaction,
    require,
    transaction_analysis::{
        self, AnalysisRow, MoneyTotal, RowDisposition, TransactionAnalysisRequest,
        TransferTreatment,
    },
    Error, Result,
};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

fn date(value: &str) -> Result<NaiveDate> {
    analytics::date(value)?;
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| Error::Validation("Invalid comparison date".into()))
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DatePeriod {
    pub from: String,
    pub through: String,
}
impl DatePeriod {
    fn days(&self) -> Result<u32> {
        let from = date(&self.from)?;
        let through = date(&self.through)?;
        require(
            from <= through,
            "Period start date must not follow end date",
        )?;
        // Canonical four-digit years bound this positive duration to fewer than 4 million days.
        Ok((through - from).num_days() as u32 + 1)
    }
    fn includes(&self, row: &Transaction) -> bool {
        row.date >= self.from && row.date <= self.through
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TransactionComparisonRequest {
    pub baseline: DatePeriod,
    pub comparison: DatePeriod,
    pub account: Option<String>,
    pub currency: Option<String>,
    pub transfers: TransferTreatment,
}
impl TransactionComparisonRequest {
    pub fn validate(&self) -> Result<()> {
        self.baseline.days()?;
        self.comparison.days()?;
        require(
            self.baseline.through < self.comparison.from
                || self.comparison.through < self.baseline.from,
            "Comparison periods must not overlap; both endpoints are inclusive",
        )?;
        // Account, currency and transfer semantics share the existing analysis contract.
        self.analysis_request(&self.baseline).validate()
    }
    fn analysis_request(&self, period: &DatePeriod) -> TransactionAnalysisRequest {
        TransactionAnalysisRequest {
            date_from: Some(period.from.clone()),
            date_to: Some(period.through.clone()),
            account: self.account.clone(),
            currency: self.currency.clone(),
            transfers: self.transfers,
            ..Default::default()
        }
    }
    fn account_currency_includes(&self, row: &Transaction) -> bool {
        self.account
            .as_ref()
            .is_none_or(|value| row.account == *value)
            && self
                .currency
                .as_ref()
                .is_none_or(|value| row.currency == *value)
    }
    pub(crate) fn includes(&self, row: &Transaction) -> bool {
        self.account_currency_includes(row)
            && (self.baseline.includes(row) || self.comparison.includes(row))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TransactionVersion {
    pub transaction_id: String,
    pub version: u32,
}
impl From<&Transaction> for TransactionVersion {
    fn from(row: &Transaction) -> Self {
        Self {
            transaction_id: row.id.clone(),
            version: row.version,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PeriodAccountTotal {
    /// All source rows in this account/currency/period, including unaccepted and excluded rows.
    pub scope_count: usize,
    pub total: MoneyTotal,
    pub pending_ids: Vec<String>,
    pub rejected_ids: Vec<String>,
    pub deferred_ids: Vec<String>,
    pub excluded_transfer_ids: Vec<String>,
    /// Immutable revision-bound annotations, not a new review or classification decision.
    pub rows: Vec<AnalysisRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum RelativeChange {
    /// Exact ratio; a percentage is numerator / denominator * 100. No decimal division occurs.
    Defined {
        numerator: String,
        denominator: String,
    },
    ZeroBaseline {
        comparison_is_zero: bool,
    },
    /// A signed net loss baseline does not have an unambiguous conventional growth percentage.
    NegativeBaseline,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AmountChange {
    pub baseline: String,
    pub comparison: String,
    /// Comparison minus baseline, in the group's currency units.
    pub delta: String,
    pub relative_change: RelativeChange,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AccountCurrencyComparison {
    pub account: String,
    pub currency: String,
    pub baseline: PeriodAccountTotal,
    pub comparison: PeriodAccountTotal,
    pub credits: AmountChange,
    pub debits: AmountChange,
    pub net: AmountChange,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TransactionComparison {
    pub schema_version: u32,
    pub rules_version: String,
    pub workspace_revision: u64,
    pub request: TransactionComparisonRequest,
    pub baseline_days: u32,
    pub comparison_days: u32,
    pub unequal_duration: bool,
    /// Calendar days strictly between the periods, regardless of their selected order.
    pub gap_days: u32,
    pub workspace_transaction_count: usize,
    pub account_currency_transaction_count: usize,
    pub outside_account_currency_count: usize,
    pub baseline_transaction_count: usize,
    pub comparison_transaction_count: usize,
    /// Account/currency-matching rows before, between or after the selected periods.
    pub outside_period_rows: Vec<TransactionVersion>,
    /// Verified transfer counterparts supporting annotations, including out-of-scope peers.
    pub verified_transfer_peers: Vec<TransactionVersion>,
    pub groups: Vec<AccountCurrencyComparison>,
    pub limitations: Vec<String>,
}

fn change(baseline: &str, comparison: &str) -> Result<AmountChange> {
    // These are generated totals, not source fields: their exact string may exceed the source
    // field's 30-byte bound. Parse exactly without applying that unrelated input-length limit.
    let parse = |value: &str| {
        Decimal::from_str_exact(value)
            .map_err(|_| Error::Validation("Invalid exact comparison total".into()))
    };
    let base = parse(baseline)?;
    let compared = parse(comparison)?;
    let delta = analytics::exact_sub(compared, base)?.to_string();
    let relative_change = if base == Decimal::ZERO {
        RelativeChange::ZeroBaseline {
            comparison_is_zero: compared == Decimal::ZERO,
        }
    } else if base < Decimal::ZERO {
        RelativeChange::NegativeBaseline
    } else {
        RelativeChange::Defined {
            numerator: delta.clone(),
            denominator: baseline.into(),
        }
    };
    Ok(AmountChange {
        baseline: baseline.into(),
        comparison: comparison.into(),
        delta,
        relative_change,
    })
}

type RowLookup<'a> = BTreeMap<&'a str, &'a Transaction>;
type GroupRows = BTreeMap<(String, String), Vec<AnalysisRow>>;

fn grouped(rows: Vec<AnalysisRow>, lookup: &RowLookup<'_>) -> GroupRows {
    let mut groups: GroupRows = BTreeMap::new();
    // analyze() has already validated uniqueness, and produced only known transaction IDs.
    for row in rows {
        let source = lookup[row.transaction_id.as_str()];
        groups
            .entry((source.account.clone(), source.currency.clone()))
            .or_default()
            .push(row);
    }
    groups
}

fn period_total(rows: Vec<AnalysisRow>, lookup: &RowLookup<'_>) -> Result<PeriodAccountTotal> {
    let ids = |state| {
        rows.iter()
            .filter(|row| row.disposition == state)
            .map(|row| row.transaction_id.clone())
            .collect()
    };
    Ok(PeriodAccountTotal {
        scope_count: rows.len(),
        total: transaction_analysis::sum_transactions(
            rows.iter()
                .filter(|row| row.disposition == RowDisposition::Included)
                .map(|row| lookup[row.transaction_id.as_str()]),
        )?,
        pending_ids: ids(RowDisposition::Pending),
        rejected_ids: ids(RowDisposition::Rejected),
        deferred_ids: ids(RowDisposition::Deferred),
        excluded_transfer_ids: ids(RowDisposition::ReviewedTransferExcluded),
        rows,
    })
}

/// Compare one supplied ledger revision without mutating any source, review state or finding.
/// Workspace::compare_transaction_periods adds revision and original-integrity verification.
pub fn compare(
    transactions: &[Transaction],
    revision: u64,
    request: &TransactionComparisonRequest,
) -> Result<TransactionComparison> {
    request.validate()?;
    // Reuse authoritative classification, validation, transfer and scope rules for both sides.
    let baseline = transaction_analysis::analyze(
        transactions,
        revision,
        &request.analysis_request(&request.baseline),
    )?;
    let comparison = transaction_analysis::analyze(
        transactions,
        revision,
        &request.analysis_request(&request.comparison),
    )?;
    let lookup: RowLookup<'_> = transactions
        .iter()
        .map(|row| (row.id.as_str(), row))
        .collect();
    let peer_ids: BTreeSet<_> = baseline
        .rows
        .iter()
        .chain(&comparison.rows)
        .filter_map(|row| row.verified_transfer_peer.as_deref())
        .collect();
    let verified_transfer_peers = peer_ids.into_iter().map(|id| lookup[id].into()).collect();
    let baseline_transaction_count = baseline.scope_transaction_count;
    let comparison_transaction_count = comparison.scope_transaction_count;
    let mut baseline_groups = grouped(baseline.rows, &lookup);
    let mut comparison_groups = grouped(comparison.rows, &lookup);
    let keys: BTreeSet<_> = baseline_groups
        .keys()
        .chain(comparison_groups.keys())
        .cloned()
        .collect();
    let mut groups = Vec::with_capacity(keys.len());
    for key in keys {
        let baseline = period_total(baseline_groups.remove(&key).unwrap_or_default(), &lookup)?;
        let comparison = period_total(comparison_groups.remove(&key).unwrap_or_default(), &lookup)?;
        groups.push(AccountCurrencyComparison {
            account: key.0,
            currency: key.1,
            credits: change(&baseline.total.credits, &comparison.total.credits)?,
            debits: change(&baseline.total.debits, &comparison.total.debits)?,
            net: change(&baseline.total.net, &comparison.total.net)?,
            baseline,
            comparison,
        });
    }
    let baseline_days = request.baseline.days()?;
    let comparison_days = request.comparison.days()?;
    let (earlier, later) = if request.baseline.from < request.comparison.from {
        (&request.baseline, &request.comparison)
    } else {
        (&request.comparison, &request.baseline)
    };
    let gap_days = (date(&later.from)? - date(&earlier.through)?).num_days() as u32 - 1;
    let filtered: Vec<_> = transactions
        .iter()
        .filter(|row| request.account_currency_includes(row))
        .collect();
    Ok(TransactionComparison {
        schema_version: 1,
        rules_version: "transaction_period_comparison_v1".into(),
        workspace_revision: revision,
        request: request.clone(),
        baseline_days,
        comparison_days,
        unequal_duration: baseline_days != comparison_days,
        gap_days,
        workspace_transaction_count: transactions.len(),
        account_currency_transaction_count: filtered.len(),
        outside_account_currency_count: transactions.len() - filtered.len(),
        baseline_transaction_count,
        comparison_transaction_count,
        outside_period_rows: filtered.into_iter().filter(|row| !request.includes(row)).map(Into::into).collect(),
        verified_transfer_peers,
        groups,
        limitations: vec![
            "Periods use inclusive transaction dates, not posting dates. Comparison minus baseline is the direction even when the baseline is chronologically later. Unequal durations and gaps are not automatically normalized.".into(),
            "Only accepted source transactions enter totals. Pending, rejected, deferred and explicitly excluded reviewed transfers remain in each period's denominator and versioned source rows.".into(),
            "Accounts and currencies remain separate; no exchange conversion occurs. Duplicate candidates and legitimate repeated purchases remain counted. Transfer exclusion requires a reciprocal, accepted, equal-opposite same-currency pair on distinct accounts; out-of-scope peers can support that decision.".into(),
            "Zero accepted totals do not establish absent activity or complete statement coverage. Empty periods and account/currency pairs absent from one side remain distinguishable by their source and review denominators. Unimported statements and date gaps cannot be inferred from this ledger.".into(),
            "Relative change is an exact ratio with a positive baseline; multiply the ratio by 100 for percentage units. Zero and negative baselines have no percentage. Totals and differences that exceed exact decimal representability fail the complete comparison rather than round.".into(),
            "This calculation reuses transaction_patterns_v1 validation and review annotations, including its 100,000-row and 32 MiB description bounds and exact currency-total bounds. Heuristic cash/refund annotations are not new accepted classifications. Source drillthrough requires the recorded workspace revision and transaction version.".into(),
        ],
    })
}
