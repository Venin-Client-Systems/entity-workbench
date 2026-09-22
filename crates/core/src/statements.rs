//! Explicit, deterministic statement mappings. Original bytes are never rewritten.
use crate::{analytics, domain::*, policy, require, Error, Result};
use rust_decimal::Decimal;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Delimiter {
    Comma,
    Semicolon,
    Tab,
}
impl Delimiter {
    pub fn byte(self) -> u8 {
        match self {
            Self::Comma => b',',
            Self::Semicolon => b';',
            Self::Tab => b'\t',
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DateFormat {
    Iso,
    DayFirst,
    MonthFirst,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NumberFormat {
    DotDecimal,
    CommaDecimal,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RowOrder {
    OldestFirst,
    NewestFirst,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ValueMapping {
    Column { column: String },
    Constant { value: String },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AmountMapping {
    Signed {
        column: String,
        positive_is_debit: bool,
    },
    DebitCredit {
        debit: String,
        credit: String,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StatementMapping {
    pub delimiter: Delimiter,
    pub date_format: DateFormat,
    pub number_format: NumberFormat,
    pub row_order: RowOrder,
    pub date: String,
    pub posting_date: Option<String>,
    pub description: String,
    pub amount: AmountMapping,
    pub balance: Option<String>,
    pub account: ValueMapping,
    pub currency: ValueMapping,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StatementProfile {
    pub id: String,
    pub name: String,
    pub mapping: StatementMapping,
    pub created_at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StatementImport {
    pub evidence_id: String,
    pub mapping: StatementMapping,
    pub profile_id: Option<String>,
    pub transaction_ids: Vec<String>,
    pub imported_at: String,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct StatementSample {
    pub sha256: String,
    pub headers: Vec<String>,
    pub sample_rows: Vec<Vec<String>>,
    pub suggested_mapping: StatementMapping,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct StatementPreviewRow {
    pub source_row: u32,
    pub original_amounts: Vec<(String, String)>,
    pub transaction: Option<Transaction>,
    pub error: Option<String>,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct StatementIssue {
    pub source_row: u32,
    pub message: String,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct StatementPreview {
    pub workspace_revision: u64,
    pub sha256: String,
    pub preview_token: String,
    pub already_imported: bool,
    pub total_rows: usize,
    pub valid_rows: usize,
    pub invalid_rows: usize,
    pub balance_mismatches: usize,
    pub rows: Vec<StatementPreviewRow>,
    pub issues: Vec<StatementIssue>,
    pub rows_truncated: bool,
    pub issues_truncated: bool,
}
pub(crate) struct ParsedStatement {
    pub total_rows: usize,
    pub invalid_rows: usize,
    pub transactions: Vec<Transaction>,
    pub rows: Vec<StatementPreviewRow>,
    pub issues: Vec<StatementIssue>,
}

pub(crate) fn reader(bytes: &[u8], delimiter: Delimiter) -> Result<csv::Reader<&[u8]>> {
    require(
        !bytes.is_empty() && bytes.len() <= policy::MAX_IMPORT_BYTES,
        "Statement must contain 1 byte to 16 MiB",
    )?;
    let text = std::str::from_utf8(bytes)
        .map_err(|_| Error::Validation("Statement must be UTF-8 text".into()))?;
    Ok(csv::ReaderBuilder::new()
        .delimiter(delimiter.byte())
        .from_reader(text.trim_start_matches('\u{feff}').as_bytes()))
}
fn headers(reader: &mut csv::Reader<&[u8]>) -> Result<Vec<String>> {
    let headers: Vec<String> = reader.headers()?.iter().map(str::to_owned).collect();
    require(
        !headers.is_empty() && headers.len() <= 128,
        "Statement requires 1 to 128 header columns",
    )?;
    let mut seen = BTreeSet::new();
    for h in &headers {
        require(
            !h.trim().is_empty() && h.len() <= 200 && !h.chars().any(char::is_control),
            "Headers must be nonempty and at most 200 bytes, without control characters",
        )?;
        require(
            seen.insert(h),
            "Duplicate headers are ambiguous; use a source export with unique headers",
        )?;
    }
    Ok(headers)
}
pub(crate) fn suggested_mapping(headers: &[String], delimiter: Delimiter) -> StatementMapping {
    let optional = |name: &str| headers.iter().find(|h| *h == name).cloned();
    let required = |name: &str| optional(name).unwrap_or_default();
    let value = |name: &str| match optional(name) {
        Some(column) => ValueMapping::Column { column },
        None => ValueMapping::Constant {
            value: String::new(),
        },
    };
    StatementMapping {
        delimiter,
        date_format: DateFormat::Iso,
        number_format: NumberFormat::DotDecimal,
        row_order: RowOrder::OldestFirst,
        date: required("date"),
        posting_date: optional("posting_date"),
        description: required("description"),
        amount: AmountMapping::Signed {
            column: required("amount"),
            positive_is_debit: false,
        },
        balance: optional("balance"),
        account: value("account"),
        currency: value("currency"),
    }
}
pub fn sample(bytes: &[u8], delimiter: Delimiter) -> Result<StatementSample> {
    let mut reader = reader(bytes, delimiter)?;
    let headers = headers(&mut reader)?;
    let sample_rows = reader
        .records()
        .take(5)
        .map(|r| Ok(r?.iter().map(|s| s.chars().take(240).collect()).collect()))
        .collect::<Result<Vec<_>>>()?;
    Ok(StatementSample {
        sha256: crate::store::hash(bytes),
        suggested_mapping: suggested_mapping(&headers, delimiter),
        headers,
        sample_rows,
    })
}
fn column_index(headers: &[String], column: &str) -> Result<usize> {
    headers
        .iter()
        .position(|h| h == column)
        .ok_or_else(|| Error::Validation(format!("Select an existing column for '{column}'")))
}
fn validate_mapping(headers: &[String], mapping: &StatementMapping) -> Result<()> {
    let mut columns = vec![&mapping.date, &mapping.description];
    columns.extend(mapping.posting_date.iter());
    columns.extend(mapping.balance.iter());
    match &mapping.amount {
        AmountMapping::Signed { column, .. } => columns.push(column),
        AmountMapping::DebitCredit { debit, credit } => {
            require(
                debit != credit,
                "Debit and credit must use different columns",
            )?;
            columns.extend([debit, credit]);
        }
    }
    for value in [&mapping.account, &mapping.currency] {
        match value {
            ValueMapping::Column { column } => columns.push(column),
            ValueMapping::Constant { value } => require(
                !value.trim().is_empty()
                    && value.len() <= 300
                    && !value.chars().any(char::is_control),
                "Constant account/currency values must be nonempty text of at most 300 bytes",
            )?,
        }
    }
    for column in columns {
        column_index(headers, column)?;
    }
    Ok(())
}
fn mapped_date(value: &str, format: DateFormat) -> Result<String> {
    let value = value.trim();
    let pattern = match format {
        DateFormat::Iso => "%Y-%m-%d",
        DateFormat::DayFirst => "%d/%m/%Y",
        DateFormat::MonthFirst => "%m/%d/%Y",
    };
    require(
        value.len() == 10,
        "Date must match the selected ten-character format",
    )?;
    let parsed = chrono::NaiveDate::parse_from_str(value, pattern)
        .map_err(|_| Error::Validation("Invalid date for the selected format".into()))?;
    require(
        parsed.format(pattern).to_string() == value,
        "Date must match the selected format exactly",
    )?;
    Ok(parsed.format("%Y-%m-%d").to_string())
}
fn mapped_amount(value: &str, format: NumberFormat) -> Result<Decimal> {
    let value = value.trim();
    require(
        !value.is_empty() && value.len() <= 64,
        "Amount is empty or too long",
    )?;
    let (negative, unsigned) =
        if let Some(v) = value.strip_prefix('(').and_then(|v| v.strip_suffix(')')) {
            (true, v)
        } else if let Some(v) = value.strip_prefix('-') {
            (true, v)
        } else {
            (false, value.strip_prefix('+').unwrap_or(value))
        };
    let (decimal, grouping) = match format {
        NumberFormat::DotDecimal => ('.', ','),
        NumberFormat::CommaDecimal => (',', '.'),
    };
    let parts: Vec<_> = unsigned.split(decimal).collect();
    require(
        parts.len() <= 2 && !parts[0].is_empty(),
        "Invalid decimal separators",
    )?;
    let groups: Vec<_> = parts[0].split(grouping).collect();
    require(
        groups
            .iter()
            .all(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())),
        "Amount contains an unsupported character",
    )?;
    if groups.len() > 1 {
        require(
            groups[0].len() <= 3 && groups.iter().skip(1).all(|g| g.len() == 3),
            "Thousands separators must separate groups of three digits",
        )?;
    }
    let mut normalized = groups.join("");
    if parts.len() == 2 {
        require(
            !parts[1].is_empty() && parts[1].bytes().all(|b| b.is_ascii_digit()),
            "Invalid fractional amount",
        )?;
        normalized.push('.');
        normalized.push_str(parts[1]);
    }
    if negative {
        normalized.insert(0, '-');
    }
    analytics::amount(&normalized)
}
fn transaction(
    row: &csv::StringRecord,
    headers: &[String],
    mapping: &StatementMapping,
    digest: &str,
    source_row: u32,
) -> Result<Transaction> {
    let cell = |name: &str| -> Result<&str> {
        row.get(column_index(headers, name)?)
            .ok_or_else(|| Error::Validation("Source row is missing a mapped field".into()))
    };
    let value = |mapping: &ValueMapping| -> Result<String> {
        match mapping {
            ValueMapping::Column { column } => Ok(cell(column)?.into()),
            ValueMapping::Constant { value } => Ok(value.clone()),
        }
    };
    let number = |s: &str| mapped_amount(s, mapping.number_format);
    let (amount, source_column) = match &mapping.amount {
        AmountMapping::Signed {
            column,
            positive_is_debit,
        } => {
            let amount = number(cell(column)?)?;
            (if *positive_is_debit { -amount } else { amount }, column)
        }
        AmountMapping::DebitCredit { debit, credit } => {
            let optional = |c: &str| -> Result<Option<Decimal>> {
                let s = cell(c)?;
                if s.trim().is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(number(s)?))
                }
            };
            let (d, c) = (optional(debit)?, optional(credit)?);
            require(
                d.is_some() || c.is_some(),
                "Debit and credit are both empty",
            )?;
            let (dv, cv) = (d.unwrap_or_default(), c.unwrap_or_default());
            require(
                dv >= Decimal::ZERO && cv >= Decimal::ZERO,
                "Separate debit/credit columns must contain nonnegative values",
            )?;
            require(
                dv.is_zero() || cv.is_zero(),
                "Both debit and credit are nonzero; review this source row",
            )?;
            if !dv.is_zero() || c.is_none() {
                (-dv, debit)
            } else {
                (cv, credit)
            }
        }
    };
    let optional_date = match &mapping.posting_date {
        Some(c) if !cell(c)?.trim().is_empty() => Some(mapped_date(cell(c)?, mapping.date_format)?),
        _ => None,
    };
    let balance = match &mapping.balance {
        Some(c) if !cell(c)?.trim().is_empty() => Some(number(cell(c)?)?.to_string()),
        _ => None,
    };
    let t = Transaction {
        id: format!("{digest}:{source_row}"),
        account: value(&mapping.account)?,
        date: mapped_date(cell(&mapping.date)?, mapping.date_format)?,
        posting_date: optional_date,
        description: cell(&mapping.description)?.into(),
        amount: amount.to_string(),
        currency: value(&mapping.currency)?.trim().to_ascii_uppercase(),
        balance,
        anchor: SourceAnchor::Cell {
            evidence_id: digest.into(),
            sheet: "CSV".into(),
            row: source_row,
            column: source_column.clone(),
        },
        review: ReviewState::Pending,
        duplicate_candidates: vec![],
        transfer_peer: None,
        merchant: None,
        version: 1,
    };
    require(
        t.account.len() <= 300 && t.description.len() <= 4000,
        "Account or description exceeds the supported length",
    )?;
    analytics::validate_transaction(&t)?;
    Ok(t)
}
pub(crate) fn parse(bytes: &[u8], mapping: &StatementMapping) -> Result<ParsedStatement> {
    let mut reader = reader(bytes, mapping.delimiter)?;
    let headers = headers(&mut reader)?;
    validate_mapping(&headers, mapping)?;
    let digest = crate::store::hash(bytes);
    let mut result = ParsedStatement {
        total_rows: 0,
        invalid_rows: 0,
        transactions: vec![],
        rows: vec![],
        issues: vec![],
    };
    for (index, record) in reader.records().enumerate() {
        require(index < 100_000, "Statement row limit exceeded")?;
        let source_row = (index + 2) as u32;
        result.total_rows += 1;
        let mut original_amounts = vec![];
        let parsed = match record {
            Ok(row) => {
                let columns = match &mapping.amount {
                    AmountMapping::Signed { column, .. } => vec![column],
                    AmountMapping::DebitCredit { debit, credit } => vec![debit, credit],
                };
                for c in columns {
                    original_amounts.push((
                        c.clone(),
                        row.get(column_index(&headers, c)?)
                            .unwrap_or_default()
                            .chars()
                            .take(240)
                            .collect(),
                    ));
                }
                transaction(&row, &headers, mapping, &digest, source_row)
            }
            Err(e) => Err(e.into()),
        };
        let (transaction, error) = match parsed {
            Ok(t) => {
                result.transactions.push(t.clone());
                (Some(t), None)
            }
            Err(e) => {
                result.invalid_rows += 1;
                let message = e.to_string();
                if result.issues.len() < 100 {
                    result.issues.push(StatementIssue {
                        source_row,
                        message: message.clone(),
                    });
                }
                (None, Some(message))
            }
        };
        if result.rows.len() < 50 {
            result.rows.push(StatementPreviewRow {
                source_row,
                original_amounts,
                transaction,
                error,
            });
        }
    }
    require(result.total_rows > 0, "Statement contains no data rows")?;
    if mapping.row_order == RowOrder::NewestFirst {
        result.transactions.reverse();
    }
    Ok(result)
}
