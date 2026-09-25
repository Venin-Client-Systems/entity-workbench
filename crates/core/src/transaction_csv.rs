//! Explicit reversible CSV presentation. Raw typed JSON remains a separate export.
use crate::{
    domain::{ReviewState, Transaction},
    literal_search::LiteralMatching,
    require,
    transaction_export::TransactionExportRequest,
    Error, Result,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};

pub const MAX_CSV_BYTES: usize = 256 * 1024 * 1024;
const BOM: &[u8] = b"\xef\xbb\xbf";
const HEADERS: [&str; 15] = [
    "workspace_revision",
    "id",
    "version",
    "account",
    "date",
    "posting_date",
    "description",
    "amount",
    "currency",
    "balance",
    "anchor",
    "review",
    "duplicate_candidates",
    "transfer_peer",
    "merchant",
];

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NonAcceptedCsvPolicy {
    Reject,
    AllowSelected,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransactionCsvFormat {
    TypedLiteralV1,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransactionCsvRequest {
    pub selection: TransactionExportRequest,
    pub non_accepted: NonAcceptedCsvPolicy,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CsvLogicalType {
    Text,
    ExactDecimalText,
    CalendarDateText,
    UnsignedInteger,
    SourceAnchorJson,
    StringArrayJson,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TransactionCsvColumn {
    pub name: String,
    pub logical_type: CsvLogicalType,
    pub prefix: String,
    pub nullable: bool,
    pub meaning: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TransactionCsvDictionary {
    pub schema_version: u32,
    pub encoding: String,
    pub delimiter: String,
    pub record_terminator: String,
    pub quote_policy: String,
    pub null_literal: String,
    pub columns: Vec<TransactionCsvColumn>,
    pub limitations: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TransactionCsvExport {
    pub schema_version: u32,
    pub workspace_revision: u64,
    pub request: TransactionCsvRequest,
    pub matching: LiteralMatching,
    /// Same selection identity as complete raw JSON, independent of CSV presentation.
    pub selection_sha256: String,
    pub format: TransactionCsvFormat,
    /// Versioned format and full dictionary, independent of selected row content.
    pub format_sha256: String,
    pub dictionary: TransactionCsvDictionary,
    pub row_count: u64,
    pub bytes: u64,
    /// Exact CSV bytes including UTF-8 BOM, doubled quotes and CRLF separators.
    pub sha256: String,
    pub csv: String,
}

pub(crate) fn dictionary() -> TransactionCsvDictionary {
    use CsvLogicalType::*;
    let definitions = [
        (
            UnsignedInteger,
            "uint:",
            false,
            "Workspace revision captured in the same canonical read snapshot.",
        ),
        (
            Text,
            "text:",
            false,
            "Stable canonical transaction identifier; never an account number.",
        ),
        (
            UnsignedInteger,
            "uint:",
            false,
            "Exact canonical transaction version.",
        ),
        (
            Text,
            "text:",
            false,
            "Original account text, including leading zeros.",
        ),
        (
            CalendarDateText,
            "date:",
            false,
            "Transaction calendar date YYYY-MM-DD; no time zone is implied.",
        ),
        (
            CalendarDateText,
            "date:",
            true,
            "Posting calendar date YYYY-MM-DD when available; no time zone is implied.",
        ),
        (
            Text,
            "text:",
            false,
            "Original transaction description; formulas remain literal text behind the visible prefix.",
        ),
        (
            ExactDecimalText,
            "decimal:",
            false,
            "Exact signed canonical amount string with up to eight decimal places; not a floating-point cell.",
        ),
        (
            Text,
            "text:",
            false,
            "Canonical three-letter currency; no currency conversion.",
        ),
        (
            ExactDecimalText,
            "decimal:",
            true,
            "Exact available balance string; null is distinct from zero.",
        ),
        (
            SourceAnchorJson,
            "json:",
            false,
            "Complete canonical SourceAnchor JSON; provenance reference, not an acceptance claim.",
        ),
        (
            Text,
            "text:",
            false,
            "Canonical review state: pending, accepted, rejected or deferred.",
        ),
        (
            StringArrayJson,
            "json:",
            false,
            "Exact duplicate candidate ID array; no repeated transactions are removed.",
        ),
        (
            Text,
            "text:",
            true,
            "Canonical transfer peer ID when present; export does not change or exclude transfers.",
        ),
        (
            Text,
            "text:",
            true,
            "Canonical merchant value when present; null and empty text remain distinct.",
        ),
    ];
    let columns = HEADERS
        .iter()
        .zip(definitions)
        .map(
            |(name, (logical_type, prefix, nullable, meaning))| TransactionCsvColumn {
                name: (*name).into(),
                logical_type,
                prefix: prefix.into(),
                nullable,
                meaning: meaning.into(),
            },
        )
        .collect();
    TransactionCsvDictionary {
        schema_version: 1,
        encoding: "utf-8-bom".into(),
        delimiter: ",".into(),
        record_terminator: "CRLF".into(),
        quote_policy: "all_fields_double_quoted_with_doubled_internal_quotes".into(),
        null_literal: "null".into(),
        columns,
        limitations:vec![
            "Visible alphabetic type prefixes make this a literal presentation; amounts and dates are not spreadsheet numeric/date cells.".into(),
            "Remove only the declared prefix when decoding. The exact unprefixed token null means missing; text:null and text: mean present text.".into(),
            "Raw typed JSON remains the machine-readable canonical export. CSV has no native type system; downstream edits, prefix removal and re-import settings can change interpretation.".into(),
            "No universal spreadsheet safety or tested Excel/LibreOffice behavior is claimed. Do not strip prefixes and then open untrusted values as formulas.".into(),
            "Literal text cells reject controls except tab, CR and LF. JSON cells preserve JSON escapes; any remaining unsupported literal control is also refused.".into(),
        ],
    }
}

fn cell(prefix: &str, raw: &str, limit: usize) -> Result<String> {
    require(
        prefix
            .len()
            .checked_add(raw.len())
            .is_some_and(|n| n <= limit),
        "CSV cell exceeds byte limit",
    )?;
    require(
        !raw.chars()
            .any(|c| c.is_control() && !matches!(c, '\t' | '\r' | '\n')),
        "CSV literal text contains an unsupported control character; use raw typed JSON",
    )?;
    let mut value = String::new();
    value
        .try_reserve_exact(prefix.len() + raw.len())
        .map_err(|_| Error::Validation("CSV cell allocation is unavailable".into()))?;
    value.push_str(prefix);
    value.push_str(raw);
    Ok(value)
}
fn optional_cell(prefix: &str, raw: Option<&str>, limit: usize) -> Result<String> {
    raw.map_or_else(|| Ok("null".into()), |v| cell(prefix, v, limit))
}
struct LimitedBytes {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for LimitedBytes {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        self.bytes
            .len()
            .checked_add(input.len())
            .filter(|n| *n <= self.limit)
            .ok_or_else(|| io::Error::other("CSV export byte limit exceeded; narrow the scope"))?;
        self.bytes.extend_from_slice(input);
        Ok(input.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn json_cell(value: &impl Serialize, limit: usize) -> Result<String> {
    let mut writer = LimitedBytes {
        bytes: Vec::new(),
        limit: limit.saturating_sub(5),
    };
    serde_json::to_writer(&mut writer, value)?;
    let value = String::from_utf8(writer.bytes)
        .map_err(|_| Error::Validation("CSV JSON cell is not UTF-8".into()))?;
    cell("json:", &value, limit)
}
/// Visit one owned cell at a time; never materialize a second complete row table.
fn visit_cells(
    row: &Transaction,
    revision: u64,
    limit: usize,
    mut write: impl FnMut(&str) -> Result<()>,
) -> Result<()> {
    write(&cell("uint:", &revision.to_string(), limit)?)?;
    write(&cell("text:", &row.id, limit)?)?;
    write(&cell("uint:", &row.version.to_string(), limit)?)?;
    write(&cell("text:", &row.account, limit)?)?;
    write(&cell("date:", &row.date, limit)?)?;
    write(&optional_cell("date:", row.posting_date.as_deref(), limit)?)?;
    write(&cell("text:", &row.description, limit)?)?;
    write(&cell("decimal:", &row.amount, limit)?)?;
    write(&cell("text:", &row.currency, limit)?)?;
    write(&optional_cell("decimal:", row.balance.as_deref(), limit)?)?;
    write(&json_cell(&row.anchor, limit)?)?;
    let review = match row.review {
        ReviewState::Pending => "pending",
        ReviewState::Accepted => "accepted",
        ReviewState::Rejected => "rejected",
        ReviewState::Deferred => "deferred",
    };
    write(&cell("text:", review, limit)?)?;
    write(&json_cell(&row.duplicate_candidates, limit)?)?;
    write(&optional_cell(
        "text:",
        row.transfer_peer.as_deref(),
        limit,
    )?)?;
    write(&optional_cell("text:", row.merchant.as_deref(), limit)?)
}
fn add_bytes(total: &mut usize, count: usize, limit: usize) -> Result<()> {
    *total = total
        .checked_add(count)
        .filter(|n| *n <= limit)
        .ok_or_else(|| {
            Error::Validation("Complete CSV export exceeds byte limit; narrow the scope".into())
        })?;
    Ok(())
}
fn field_bytes(raw: &str) -> Result<usize> {
    raw.len()
        .checked_add(raw.bytes().filter(|b| *b == b'"').count())
        .and_then(|n| n.checked_add(2))
        .ok_or_else(|| Error::Validation("CSV field size overflow".into()))
}
pub(crate) fn encode(
    rows: &[Transaction],
    revision: u64,
    policy: NonAcceptedCsvPolicy,
    limit: usize,
) -> Result<String> {
    require(
        policy == NonAcceptedCsvPolicy::AllowSelected
            || rows.iter().all(|row| row.review == ReviewState::Accepted),
        "CSV selection contains non-accepted records; explicitly allow the selected review states or narrow the scope",
    )?;
    // Exact preflight counts the chosen quote policy before allocating the final artifact.
    let mut expected = 0;
    add_bytes(&mut expected, BOM.len(), limit)?;
    for header in HEADERS {
        add_bytes(&mut expected, field_bytes(header)?, limit)?;
    }
    add_bytes(&mut expected, HEADERS.len() - 1 + 2, limit)?;
    for row in rows {
        visit_cells(row, revision, limit, |value| {
            add_bytes(&mut expected, field_bytes(value)?, limit)
        })?;
        add_bytes(&mut expected, HEADERS.len() - 1 + 2, limit)?;
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(expected)
        .map_err(|_| Error::Validation("CSV artifact allocation is unavailable".into()))?;
    let mut sink = LimitedBytes { bytes, limit };
    sink.write_all(BOM)?;
    let mut writer = csv::WriterBuilder::new()
        .quote_style(csv::QuoteStyle::Always)
        .terminator(csv::Terminator::CRLF)
        .double_quote(true)
        .from_writer(sink);
    writer.write_record(HEADERS)?;
    for row in rows {
        visit_cells(row, revision, limit, |value| {
            writer.write_field(value)?;
            Ok(())
        })?;
        writer.write_record(std::iter::empty::<&str>())?;
    }
    writer.flush()?;
    let sink = writer.into_inner().map_err(|e| Error::Io(e.into_error()))?;
    require(
        sink.bytes.len() == expected,
        "CSV serializer differs from byte preflight",
    )?;
    String::from_utf8(sink.bytes).map_err(|_| Error::Validation("CSV export is not UTF-8".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_sink_refuses_oversized_write_without_retaining_a_partial_input() {
        let mut sink = LimitedBytes {
            bytes: Vec::new(),
            limit: 8,
        };
        sink.write_all(b"abc").unwrap();
        assert!(sink.write_all(b"123456").is_err());
        assert_eq!(sink.bytes, b"abc");
        sink.write_all(b"12345").unwrap();
        assert_eq!(sink.bytes, b"abc12345");
        assert!(sink.write_all(b"x").is_err());
        assert_eq!(sink.bytes.len(), 8);
    }
}
