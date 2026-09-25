//! Bounded frozen report input. This adapter does not read or verify original files.
use crate::{analytics, domain::*, policy, require, Error, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Write},
};

pub const MAX_DOCUMENT_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_TEXT_BYTES: usize = 1024 * 1024;
pub const MAX_TRANSACTIONS: usize = 5_000;
pub const MAX_RECORDS: usize = 10_000;
pub const MAX_REFERENCES: usize = 10_000;
pub const TEMPLATE_VERSION: &str = "assessment-foundation-1";
pub const GENERATOR_VERSION: &str = "ooxml-foundation-3";
pub(crate) const LEGACY_GENERATOR_VERSION: &str = "ooxml-foundation-1";
pub(crate) const WRAPPING_GENERATOR_VERSION: &str = "ooxml-foundation-2";

pub(crate) fn supported_generator(value: &str) -> bool {
    matches!(
        value,
        GENERATOR_VERSION | LEGACY_GENERATOR_VERSION | WRAPPING_GENERATOR_VERSION
    )
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportDocument {
    pub schema_version: u32,
    pub report_id: String,
    pub workspace_revision: u64,
    pub created_at: String,
    pub template_version: String,
    pub generator_version: String,
    pub content: ReportContent,
    pub calculations: Calculations,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportContent {
    pub hypotheses: Vec<Hypothesis>,
    pub findings: Vec<Finding>,
    pub transactions: Vec<Transaction>,
    pub entities: Vec<Entity>,
    pub observations: Vec<Observation>,
    pub evidence: Vec<Evidence>,
    pub identity_decisions: Vec<IdentityDecision>,
    pub merges: Vec<MergeDecision>,
    pub decisions: Vec<ReviewDecision>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Calculations {
    pub totals: Vec<CurrencyTotal>,
    pub balances: Vec<BalanceCalculation>,
    pub accepted: usize,
    pub pending: usize,
    pub rejected: usize,
    pub deferred: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CurrencyTotal {
    pub currency: String,
    pub credits: String,
    pub debits: String,
    pub net: String,
    pub transaction_ids: Vec<String>,
    pub excluded_transfer_ids: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BalanceCalculation {
    pub transaction_id: String,
    pub previous_id: String,
    pub transaction_ids: Vec<String>,
    pub difference: String,
    pub reconciled: bool,
}
#[derive(Serialize)]
struct BorrowedContent<'a> {
    hypotheses: &'a [Hypothesis],
    findings: &'a [Finding],
    transactions: &'a [Transaction],
    entities: &'a [Entity],
    observations: &'a [Observation],
    evidence: &'a [Evidence],
    identity_decisions: &'a [IdentityDecision],
    merges: &'a [MergeDecision],
    decisions: &'a [ReviewDecision],
}
impl<'a> From<&'a ReportContent> for BorrowedContent<'a> {
    fn from(c: &'a ReportContent) -> Self {
        Self {
            hypotheses: &c.hypotheses,
            findings: &c.findings,
            transactions: &c.transactions,
            entities: &c.entities,
            observations: &c.observations,
            evidence: &c.evidence,
            identity_decisions: &c.identity_decisions,
            merges: &c.merges,
            decisions: &c.decisions,
        }
    }
}

pub fn capture<R>(
    view: &WorkspaceView<R>,
    report_id: &str,
    created_at: &str,
) -> Result<ReportDocument> {
    metadata(report_id, created_at)?;
    let borrowed = BorrowedContent {
        hypotheses: &view.hypotheses,
        findings: &view.findings,
        transactions: &view.transactions,
        entities: &view.entities,
        observations: &view.observations,
        evidence: &view.evidence,
        identity_decisions: &view.identity_decisions,
        merges: &view.merges,
        decisions: &view.decisions,
    };
    // Validate borrowed strings, collections and JSON expansion before cloning any records.
    validate_content(&borrowed)?;
    json_size(&borrowed)?;
    let calculations = calculations(borrowed.transactions)?;
    let document = ReportDocument {
        schema_version: 1,
        report_id: report_id.to_owned(),
        workspace_revision: view.revision,
        created_at: created_at.to_owned(),
        template_version: TEMPLATE_VERSION.into(),
        generator_version: GENERATOR_VERSION.into(),
        content: ReportContent {
            hypotheses: view.hypotheses.clone(),
            findings: view.findings.clone(),
            transactions: view.transactions.clone(),
            entities: view.entities.clone(),
            observations: view.observations.clone(),
            evidence: view.evidence.clone(),
            identity_decisions: view.identity_decisions.clone(),
            merges: view.merges.clone(),
            decisions: view.decisions.clone(),
        },
        calculations,
    };
    json_size(&document)?;
    Ok(document)
}
impl ReportDocument {
    pub fn validate(&self) -> Result<()> {
        require(
            self.schema_version == 1
                && self.template_version == TEMPLATE_VERSION
                && supported_generator(&self.generator_version),
            "Unsupported frozen report format",
        )?;
        metadata(&self.report_id, &self.created_at)?;
        validate_content(&BorrowedContent::from(&self.content))?;
        json_size(self)?;
        require(
            self.calculations == calculations(&self.content.transactions)?,
            "Frozen report calculations do not match the shared analytical engine",
        )
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let mut output = CappedWriter::new(Vec::new(), MAX_DOCUMENT_BYTES);
        serde_json::to_writer(&mut output, self)?;
        Ok(output.inner)
    }
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        limit(
            bytes.len() <= MAX_DOCUMENT_BYTES,
            "Frozen report JSON exceeds 16 MiB",
        )?;
        let document: Self = serde_json::from_slice(bytes)?;
        document.validate()?;
        Ok(document)
    }
}
fn metadata(id: &str, at: &str) -> Result<()> {
    require(
        id.len() == 36 && uuid::Uuid::parse_str(id).is_ok(),
        "Invalid report identifier",
    )?;
    require(
        at.len() <= 64 && chrono::DateTime::parse_from_rfc3339(at).is_ok(),
        "Invalid report creation time",
    )
}
pub(crate) fn xml_text(text: &str) -> Result<()> {
    limit(
        text.len() <= MAX_TEXT_BYTES,
        "Report text field exceeds 1 MiB",
    )?;
    require(text.chars().all(|c| matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')),
        "Report text contains an invalid XML character")
}
pub(crate) fn limit(ok: bool, message: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(Error::QuotaExhausted(message.into()))
    }
}
fn strings<'a>(values: impl IntoIterator<Item = &'a str>) -> Result<()> {
    for value in values {
        xml_text(value)?;
    }
    Ok(())
}
fn identifier(value: &str) -> Result<()> {
    require(
        !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control),
        "Invalid report record identifier",
    )?;
    xml_text(value)
}
fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn validate_content(c: &BorrowedContent<'_>) -> Result<()> {
    limit(
        c.transactions.len() <= MAX_TRANSACTIONS,
        "Report exceeds 5000 transaction rows",
    )?;
    let mut count = 0usize;
    for length in [
        c.hypotheses.len(),
        c.findings.len(),
        c.transactions.len(),
        c.entities.len(),
        c.observations.len(),
        c.evidence.len(),
        c.identity_decisions.len(),
        c.merges.len(),
        c.decisions.len(),
    ] {
        count = count
            .checked_add(length)
            .ok_or_else(|| Error::QuotaExhausted("Report record count overflow".into()))?;
    }
    limit(count <= MAX_RECORDS, "Report exceeds 10000 records")?;
    let mut ids = BTreeSet::new();
    for id in c
        .hypotheses
        .iter()
        .map(|x| &x.id)
        .chain(c.findings.iter().map(|x| &x.id))
        .chain(c.transactions.iter().map(|x| &x.id))
        .chain(c.entities.iter().map(|x| &x.id))
        .chain(c.observations.iter().map(|x| &x.id))
        .chain(c.evidence.iter().map(|x| &x.id))
        .chain(c.identity_decisions.iter().map(|x| &x.id))
        .chain(c.merges.iter().map(|x| &x.id))
        .chain(c.decisions.iter().map(|x| &x.id))
    {
        identifier(id)?;
        require(
            ids.insert(id.as_str()),
            "Duplicate or ambiguous report record identifier",
        )?;
    }
    let evidence: BTreeMap<_, _> = c.evidence.iter().map(|e| (e.id.as_str(), e)).collect();
    let entity: BTreeSet<_> = c.entities.iter().map(|e| e.id.as_str()).collect();
    let transaction: BTreeSet<_> = c.transactions.iter().map(|t| t.id.as_str()).collect();
    let observation: BTreeSet<_> = c.observations.iter().map(|o| o.id.as_str()).collect();
    let questions: BTreeSet<_> = c.hypotheses.iter().map(|h| h.id.as_str()).collect();
    let mut references = 0usize;
    let mut refs = |length: usize| -> Result<()> {
        references = references
            .checked_add(length)
            .ok_or_else(|| Error::QuotaExhausted("Report reference count overflow".into()))?;
        limit(
            references <= MAX_REFERENCES,
            "Report exceeds 10000 nested entries or references",
        )
    };
    for e in c.evidence {
        require(
            e.id == e.sha256 && digest(&e.sha256),
            "Invalid report evidence identity",
        )?;
        limit(
            e.bytes > 0 && e.bytes <= policy::MAX_IMPORT_BYTES as u64,
            "Report original exceeds supported import bounds",
        )?;
        strings([
            e.name.as_str(),
            &e.media_type,
            &e.origin_group,
            &e.imported_at,
            &e.extraction_status,
        ])?;
        if let Some(text) = &e.text {
            xml_text(text)?;
        }
        refs(e.acquisitions.len())?;
        for a in &e.acquisitions {
            strings([a.job_id.as_str(), &a.url, &a.retrieved_at])?;
        }
    }
    for e in c.entities {
        xml_text(&e.name)?;
        refs(e.identifiers.len())?;
        for i in &e.identifiers {
            strings([i.namespace.as_str(), &i.value])?;
        }
        if let Some(id) = &e.merged_into {
            require(entity.contains(id.as_str()), "Dangling merged entity")?;
        }
    }
    for h in c.hypotheses {
        strings([h.question.as_str(), &h.proposition])?;
        refs(h.alternatives.len())?;
        refs(h.gaps.len())?;
        strings(h.alternatives.iter().chain(&h.gaps).map(String::as_str))?;
    }
    for f in c.findings {
        strings([f.title.as_str(), &f.assessment, &f.limitations])?;
        refs(f.supporting_ids.len())?;
        refs(f.contradicting_ids.len())?;
        refs(f.hypothesis_ids.len())?;
        for id in f.supporting_ids.iter().chain(&f.contradicting_ids) {
            require(
                evidence.contains_key(id.as_str())
                    || observation.contains(id.as_str())
                    || transaction.contains(id.as_str()),
                "Dangling or unsupported finding citation",
            )?;
        }
        for id in &f.hypothesis_ids {
            require(questions.contains(id.as_str()), "Dangling finding question")?;
        }
    }
    for t in c.transactions {
        strings([
            t.account.as_str(),
            &t.date,
            &t.description,
            &t.amount,
            &t.currency,
        ])?;
        for value in [&t.posting_date, &t.balance, &t.merchant]
            .into_iter()
            .flatten()
        {
            xml_text(value)?;
        }
        analytics::validate_transaction(t)?;
        require(t.version > 0, "Invalid report transaction version")?;
        anchor(&t.anchor, &evidence)?;
        refs(t.duplicate_candidates.len())?;
        for id in t.duplicate_candidates.iter().chain(t.transfer_peer.iter()) {
            require(
                id != &t.id && transaction.contains(id.as_str()),
                "Dangling transaction relationship",
            )?;
        }
    }
    for o in c.observations {
        require(
            entity.contains(o.entity_id.as_str()),
            "Dangling observation entity",
        )?;
        strings([o.field.as_str(), &o.value])?;
        require(
            o.extraction_quality
                .is_none_or(|q| q.is_finite() && (0.0..=1.0).contains(&q)),
            "Invalid extraction quality",
        )?;
        anchor(&o.anchor, &evidence)?;
    }
    for d in c.identity_decisions {
        require(
            entity.contains(d.left_id.as_str()) && entity.contains(d.right_id.as_str()),
            "Dangling identity decision entity",
        )?;
        strings([d.reason.as_str(), &d.at])?;
    }
    for m in c.merges {
        require(
            entity.contains(m.source.as_str()) && entity.contains(m.target.as_str()),
            "Dangling merge entity",
        )?;
        xml_text(&m.reason)?;
    }
    // Historical review targets are displayed as literal IDs, not fabricated links.
    for d in c.decisions {
        strings([d.target_id.as_str(), &d.reason, &d.at])?;
    }
    Ok(())
}
fn anchor(anchor: &SourceAnchor, sources: &BTreeMap<&str, &Evidence>) -> Result<()> {
    let source = sources
        .get(anchor.evidence_id())
        .ok_or_else(|| Error::Validation("Dangling report source anchor".into()))?;
    match anchor {
        SourceAnchor::Text {
            line_start,
            line_end,
            ..
        } => require(
            *line_start > 0
                && line_end >= line_start
                && source
                    .text
                    .as_ref()
                    .is_some_and(|t| *line_end as usize <= t.lines().count()),
            "Report text anchor exceeds retained text",
        )?,
        SourceAnchor::Page { page, region, .. } => require(
            *page > 0 && region.is_none_or(|r| r.iter().all(|v| v.is_finite() && *v >= 0.0)),
            "Invalid retained page anchor",
        )?,
        SourceAnchor::Cell {
            sheet, row, column, ..
        } => {
            strings([sheet.as_str(), column.as_str()])?;
            require(
                *row > 0 && !sheet.is_empty() && !column.is_empty(),
                "Invalid retained cell anchor",
            )?;
        }
        SourceAnchor::Message { message_id, .. } => {
            xml_text(message_id)?;
            require(!message_id.is_empty(), "Empty message anchor")?;
        }
        SourceAnchor::Capture { selector, .. } => {
            xml_text(selector)?;
            require(!selector.is_empty(), "Empty capture anchor")?;
        }
    }
    Ok(())
}
fn calculations(rows: &[Transaction]) -> Result<Calculations> {
    let analysis = analytics::analyse(rows)?;
    Ok(Calculations {
        totals: analysis
            .totals
            .into_iter()
            .map(|t| CurrencyTotal {
                currency: t.currency,
                credits: t.credits,
                debits: t.debits,
                net: t.net,
                transaction_ids: t.transaction_ids,
                excluded_transfer_ids: t.excluded_transfer_ids,
            })
            .collect(),
        balances: analysis
            .balance_checks
            .into_iter()
            .map(|b| BalanceCalculation {
                transaction_id: b.transaction_id,
                previous_id: b.previous_id,
                transaction_ids: b.transaction_ids,
                difference: b.difference,
                reconciled: b.reconciled,
            })
            .collect(),
        accepted: rows
            .iter()
            .filter(|t| t.review == ReviewState::Accepted)
            .count(),
        pending: rows
            .iter()
            .filter(|t| t.review == ReviewState::Pending)
            .count(),
        rejected: rows
            .iter()
            .filter(|t| t.review == ReviewState::Rejected)
            .count(),
        deferred: rows
            .iter()
            .filter(|t| t.review == ReviewState::Deferred)
            .count(),
    })
}
fn json_size(value: &impl Serialize) -> Result<()> {
    serde_json::to_writer(CappedWriter::new(io::sink(), MAX_DOCUMENT_BYTES), value)?;
    Ok(())
}
pub(crate) struct CappedWriter<W> {
    pub inner: W,
    remaining: usize,
}
impl<W> CappedWriter<W> {
    pub fn new(inner: W, remaining: usize) -> Self {
        Self { inner, remaining }
    }
}
impl<W: Write> Write for CappedWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.remaining {
            return Err(io::Error::other("Report byte limit exceeded"));
        }
        let written = self.inner.write(bytes)?;
        self.remaining -= written;
        Ok(written)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
