//! Exact, revision-labelled transaction patterns. Heuristics never change canonical review.
use crate::{analytics, domain::*, require, Error, Result};
use chrono::{Days, Months, NaiveDate};
use rust_decimal::Decimal;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_ANALYSIS_ROWS: usize = 100_000;
const MAX_DESCRIPTION_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransferTreatment {
    Include,
    ExcludeReviewedPairs,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TransactionAnalysisRequest {
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub account: Option<String>,
    pub currency: Option<String>,
    pub transfers: TransferTreatment,
    pub minimum_occurrences: u32,
    pub date_tolerance_days: u32,
    /// Maximum absolute amount spread, applied separately in each currency's units.
    pub amount_tolerance: String,
}
impl Default for TransactionAnalysisRequest {
    fn default() -> Self {
        Self {
            date_from: None,
            date_to: None,
            account: None,
            currency: None,
            transfers: TransferTreatment::Include,
            minimum_occurrences: 3,
            date_tolerance_days: 2,
            amount_tolerance: "0".into(),
        }
    }
}
impl TransactionAnalysisRequest {
    pub fn validate(&self) -> Result<()> {
        validate_scope(
            self.date_from.as_deref(),
            self.date_to.as_deref(),
            self.account.as_deref(),
            self.currency.as_deref(),
        )?;
        require(
            (3..=12).contains(&self.minimum_occurrences),
            "Recurring candidates require between three and twelve minimum occurrences",
        )?;
        require(
            self.date_tolerance_days <= 3,
            "Recurring date tolerance cannot exceed three days",
        )?;
        require(
            analytics::amount(&self.amount_tolerance)? >= Decimal::ZERO,
            "Recurring amount tolerance must be nonnegative",
        )
    }
    pub fn includes(&self, t: &Transaction) -> bool {
        self.date_from.as_ref().is_none_or(|v| t.date >= *v)
            && self.date_to.as_ref().is_none_or(|v| t.date <= *v)
            && self.account.as_ref().is_none_or(|v| t.account == *v)
            && self.currency.as_ref().is_none_or(|v| t.currency == *v)
    }
}
/// Shared borrowed scope validation; callers can reject large values before cloning.
pub(crate) fn validate_scope(
    date_from: Option<&str>,
    date_to: Option<&str>,
    account: Option<&str>,
    currency: Option<&str>,
) -> Result<()> {
    for date in [date_from, date_to].into_iter().flatten() {
        analytics::date(date)?;
    }
    require(
        !matches!((date_from, date_to), (Some(a), Some(b)) if a > b),
        "Analysis start date must not follow end date",
    )?;
    require(
        account.is_none_or(|v| v.len() <= 4000 && !v.trim().is_empty()),
        "Analysis account must be nonempty and bounded",
    )?;
    require(
        currency.is_none_or(|v| v.len() == 3 && v.bytes().all(|c| c.is_ascii_uppercase())),
        "Analysis currency must be three uppercase letters",
    )
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MoneyTotal {
    pub credits: String,
    pub debits: String,
    pub net: String,
    pub transaction_ids: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CurrencyAnalysis {
    pub currency: String,
    pub scope_count: usize,
    pub total: MoneyTotal,
    pub pending_ids: Vec<String>,
    pub rejected_ids: Vec<String>,
    pub deferred_ids: Vec<String>,
    pub excluded_transfer_ids: Vec<String>,
    pub cash_candidates: MoneyTotal,
    pub refund_candidates: MoneyTotal,
    pub unclassified_ids: Vec<String>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RowDisposition {
    Included,
    Pending,
    Rejected,
    Deferred,
    ReviewedTransferExcluded,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CashRule {
    DebitWithAtmToken,
    DebitWithCashWithdrawalPhrase,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RefundRule {
    CreditWithRefundToken,
    CreditWithReversalToken,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AnalysisRow {
    pub transaction_id: String,
    pub version: u32,
    pub disposition: RowDisposition,
    pub verified_transfer_peer: Option<String>,
    pub unverified_transfer_match: bool,
    pub has_duplicate_candidates: bool,
    /// Rules are hints even when an unreviewed row is excluded from the totals.
    pub cash_rule: Option<CashRule>,
    pub refund_rule: Option<RefundRule>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MerchantGroup {
    pub id: String,
    pub currency: String,
    pub description_group: String,
    pub accounts: Vec<String>,
    pub total: MoneyTotal,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Cadence {
    Weekly,
    Fortnightly,
    Monthly,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RecurringCandidate {
    pub id: String,
    pub account: String,
    pub currency: String,
    pub description_group: String,
    pub cadence: Cadence,
    pub total: MoneyTotal,
    pub minimum_debit: String,
    pub maximum_debit: String,
    pub expected_dates: Vec<String>,
    pub actual_dates: Vec<String>,
    pub deviations_days: Vec<i64>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TransactionAnalysis {
    pub schema_version: u32,
    pub rules_version: String,
    pub workspace_revision: u64,
    pub request: TransactionAnalysisRequest,
    pub workspace_transaction_count: usize,
    pub scope_transaction_count: usize,
    pub rows: Vec<AnalysisRow>,
    pub currencies: Vec<CurrencyAnalysis>,
    pub merchant_groups: Vec<MerchantGroup>,
    pub recurring_candidates: Vec<RecurringCandidate>,
    pub recurrence_eligible_ids: Vec<String>,
    pub recurrence_unclassified_ids: Vec<String>,
    pub limitations: Vec<String>,
}

#[derive(Default)]
struct Accumulator {
    credits: Decimal,
    debits: Decimal,
    ids: Vec<String>,
}
impl Accumulator {
    fn add(&mut self, t: &Transaction, value: Decimal) -> Result<()> {
        let target = if value < Decimal::ZERO {
            &mut self.debits
        } else {
            &mut self.credits
        };
        *target = analytics::exact_add(*target, value.abs())?;
        self.ids.push(t.id.clone());
        Ok(())
    }
    fn finish(self) -> Result<MoneyTotal> {
        let net = analytics::exact_sub(self.credits, self.debits)?;
        Ok(MoneyTotal {
            credits: self.credits.to_string(),
            debits: self.debits.to_string(),
            net: net.to_string(),
            transaction_ids: self.ids,
        })
    }
}
/// Shared exact accumulation for already classified source rows. No review decisions are made here.
pub(crate) fn sum_transactions<'a>(
    rows: impl IntoIterator<Item = &'a Transaction>,
) -> Result<MoneyTotal> {
    let mut total = Accumulator::default();
    for row in rows {
        total.add(row, analytics::amount(&row.amount)?)?;
    }
    total.finish()
}

#[derive(Default)]
struct CurrencyBuilder {
    scope_count: usize,
    total: Accumulator,
    cash: Accumulator,
    refund: Accumulator,
    pending: Vec<String>,
    rejected: Vec<String>,
    deferred: Vec<String>,
    excluded: Vec<String>,
    unclassified: Vec<String>,
}
#[derive(Default)]
struct MerchantBuilder {
    total: Accumulator,
    accounts: BTreeSet<String>,
}

fn description_group(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_uppercase()
}
fn calendar(value: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| Error::Validation("Invalid transaction calendar date".into()))
}
fn identifier(parts: &[&str]) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(parts)?)))
}
fn hints(description: &str, value: Decimal) -> (Option<CashRule>, Option<RefundRule>) {
    let tokens: Vec<_> = description
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect();
    let cash = if value < Decimal::ZERO && tokens.contains(&"ATM") {
        Some(CashRule::DebitWithAtmToken)
    } else if value < Decimal::ZERO && tokens.windows(2).any(|pair| pair == ["CASH", "WITHDRAWAL"])
    {
        Some(CashRule::DebitWithCashWithdrawalPhrase)
    } else {
        None
    };
    let refund = if value > Decimal::ZERO && tokens.contains(&"REFUND") {
        Some(RefundRule::CreditWithRefundToken)
    } else if value > Decimal::ZERO && tokens.contains(&"REVERSAL") {
        Some(RefundRule::CreditWithReversalToken)
    } else {
        None
    };
    (cash, refund)
}
fn recurring(
    rows: &mut [&Transaction],
    request: &TransactionAnalysisRequest,
    label: &str,
) -> Result<Option<RecurringCandidate>> {
    if rows.len() < request.minimum_occurrences as usize {
        return Ok(None);
    }
    rows.sort_by(|a, b| (&a.date, &a.id).cmp(&(&b.date, &b.id)));
    // Repeated purchases are never deleted. Multiple same-day rows prevent this conservative candidate.
    if rows.windows(2).any(|p| p[0].date == p[1].date) {
        return Ok(None);
    }
    let dates = rows
        .iter()
        .map(|row| calendar(&row.date))
        .collect::<Result<Vec<_>>>()?;
    let amounts = rows
        .iter()
        .map(|row| analytics::amount(&row.amount).map(|a| a.abs()))
        .collect::<Result<Vec<_>>>()?;
    let minimum = *amounts
        .iter()
        .min()
        .ok_or_else(|| Error::Validation("Empty recurring group".into()))?;
    let maximum = *amounts
        .iter()
        .max()
        .ok_or_else(|| Error::Validation("Empty recurring group".into()))?;
    if analytics::exact_sub(maximum, minimum)? > analytics::amount(&request.amount_tolerance)? {
        return Ok(None);
    }
    for cadence in [Cadence::Weekly, Cadence::Fortnightly, Cadence::Monthly] {
        let expected: Option<Vec<_>> = (0..dates.len())
            .map(|index| match cadence {
                Cadence::Weekly => dates[0].checked_add_days(Days::new(index as u64 * 7)),
                Cadence::Fortnightly => dates[0].checked_add_days(Days::new(index as u64 * 14)),
                Cadence::Monthly => dates[0].checked_add_months(Months::new(index as u32)),
            })
            .collect();
        let Some(expected) = expected else {
            continue;
        };
        let deviations: Vec<_> = dates
            .iter()
            .zip(&expected)
            .map(|(a, b)| (*a - *b).num_days())
            .collect();
        if deviations
            .iter()
            .any(|d| d.unsigned_abs() > request.date_tolerance_days as u64)
        {
            continue;
        }
        let first = rows[0];
        let mut total = Accumulator::default();
        for row in rows.iter() {
            total.add(row, analytics::amount(&row.amount)?)?;
        }
        return Ok(Some(RecurringCandidate {
            id: identifier(&["recurring_v1", &first.account, &first.currency, label])?,
            account: first.account.clone(),
            currency: first.currency.clone(),
            description_group: label.into(),
            cadence,
            total: total.finish()?,
            minimum_debit: minimum.to_string(),
            maximum_debit: maximum.to_string(),
            expected_dates: expected.into_iter().map(|d| d.to_string()).collect(),
            actual_dates: rows.iter().map(|r| r.date.clone()).collect(),
            deviations_days: deviations,
        }));
    }
    Ok(None)
}

/// All output IDs resolve to the supplied immutable revision. No source row is silently deduplicated.
pub fn analyze(
    transactions: &[Transaction],
    revision: u64,
    request: &TransactionAnalysisRequest,
) -> Result<TransactionAnalysis> {
    request.validate()?;
    require(
        transactions.len() <= MAX_ANALYSIS_ROWS,
        "Transaction analysis exceeds the 100,000-row bound; analytical pagination is required",
    )?;
    let mut lookup = BTreeMap::new();
    let mut description_bytes = 0usize;
    for t in transactions {
        analytics::validate_transaction(t)?;
        require(
            !t.id.is_empty() && lookup.insert(t.id.as_str(), t).is_none(),
            "Transaction analysis requires unique nonempty row IDs",
        )?;
        description_bytes = description_bytes
            .checked_add(t.description.len())
            .ok_or_else(|| Error::Validation("Transaction description size overflow".into()))?;
        require(
            description_bytes <= MAX_DESCRIPTION_BYTES,
            "Transaction analysis exceeds the 32 MiB description bound",
        )?;
    }
    let mut output = TransactionAnalysis { schema_version: 1, rules_version: "transaction_patterns_v1".into(), workspace_revision: revision, request: request.clone(), workspace_transaction_count: transactions.len(), scope_transaction_count: 0, rows: vec![], currencies: vec![], merchant_groups: vec![], recurring_candidates: vec![], recurrence_eligible_ids: vec![], recurrence_unclassified_ids: vec![], limitations: vec![
        "Merchant totals group original descriptions by ASCII case and whitespace only. Merchant identity, branch and transaction channel are unconfirmed; digits and punctuation remain significant.".into(),
        "Cash/refund markers and cadence are heuristic candidates, not accepted classifications. Refunds are not paired to purchases or silently netted against another row.".into(),
        "Only accepted source transactions enter totals. Pending, rejected, deferred and opted-out reviewed transfer rows remain in the scope denominator and source list.".into(),
        "Duplicate candidates and repeated purchases remain counted. No currency conversion occurs. Scope uses transaction dates, not posting dates.".into(),
        "Recurrence requires the entire accepted debit group for one account, currency and description to fit an anchored weekly, fortnightly or calendar-month schedule. Missing periods, same-day repeats and mixed activity can suppress candidates; no subsequence search or subscription claim is made.".into(),
        "Calendar-month expectations clamp the first transaction's day to the last valid day of each target month. Amount tolerance is an absolute maximum debit spread in each currency's units.".into(),
    ] };
    let mut currencies: BTreeMap<String, CurrencyBuilder> = BTreeMap::new();
    let mut merchants: BTreeMap<(String, String), MerchantBuilder> = BTreeMap::new();
    let mut recurrent: BTreeMap<(String, String, String), Vec<&Transaction>> = BTreeMap::new();
    for t in transactions.iter().filter(|t| request.includes(t)) {
        output.scope_transaction_count += 1;
        let value = analytics::amount(&t.amount)?;
        let label = description_group(&t.description);
        let (cash_rule, refund_rule) = hints(&label, value);
        let peer = analytics::verified_transfer_peer(t, &lookup)?;
        let currency = currencies.entry(t.currency.clone()).or_default();
        currency.scope_count += 1;
        let disposition = match t.review {
            ReviewState::Pending => {
                currency.pending.push(t.id.clone());
                RowDisposition::Pending
            }
            ReviewState::Rejected => {
                currency.rejected.push(t.id.clone());
                RowDisposition::Rejected
            }
            ReviewState::Deferred => {
                currency.deferred.push(t.id.clone());
                RowDisposition::Deferred
            }
            ReviewState::Accepted
                if request.transfers == TransferTreatment::ExcludeReviewedPairs
                    && peer.is_some() =>
            {
                currency.excluded.push(t.id.clone());
                RowDisposition::ReviewedTransferExcluded
            }
            ReviewState::Accepted => RowDisposition::Included,
        };
        output.rows.push(AnalysisRow {
            transaction_id: t.id.clone(),
            version: t.version,
            disposition,
            verified_transfer_peer: peer.map(|p| p.id.clone()),
            unverified_transfer_match: t.transfer_peer.is_some() && peer.is_none(),
            has_duplicate_candidates: !t.duplicate_candidates.is_empty(),
            cash_rule,
            refund_rule,
        });
        if disposition != RowDisposition::Included {
            continue;
        }
        currency.total.add(t, value)?;
        if cash_rule.is_some() {
            currency.cash.add(t, value)?;
        }
        if refund_rule.is_some() {
            currency.refund.add(t, value)?;
        }
        if cash_rule.is_none() && refund_rule.is_none() {
            currency.unclassified.push(t.id.clone());
        }
        let merchant = merchants
            .entry((t.currency.clone(), label.clone()))
            .or_default();
        merchant.total.add(t, value)?;
        merchant.accounts.insert(t.account.clone());
        if value < Decimal::ZERO {
            output.recurrence_eligible_ids.push(t.id.clone());
            recurrent
                .entry((t.account.clone(), t.currency.clone(), label))
                .or_default()
                .push(t);
        }
    }
    for (currency, b) in currencies {
        output.currencies.push(CurrencyAnalysis {
            currency,
            scope_count: b.scope_count,
            total: b.total.finish()?,
            pending_ids: b.pending,
            rejected_ids: b.rejected,
            deferred_ids: b.deferred,
            excluded_transfer_ids: b.excluded,
            cash_candidates: b.cash.finish()?,
            refund_candidates: b.refund.finish()?,
            unclassified_ids: b.unclassified,
        });
    }
    for ((currency, label), b) in merchants {
        output.merchant_groups.push(MerchantGroup {
            id: identifier(&["description_v1", &currency, &label])?,
            currency,
            description_group: label,
            accounts: b.accounts.into_iter().collect(),
            total: b.total.finish()?,
        });
    }
    for ((_, _, label), mut rows) in recurrent {
        if let Some(candidate) = recurring(&mut rows, request, &label)? {
            output.recurring_candidates.push(candidate);
        } else {
            output
                .recurrence_unclassified_ids
                .extend(rows.into_iter().map(|t| t.id.clone()));
        }
    }
    Ok(output)
}
