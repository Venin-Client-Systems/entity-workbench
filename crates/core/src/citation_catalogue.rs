//! Revision-bound citation metadata, separate from source content and analyst acceptance.
use crate::{
    domain::{ReviewState, SourceAnchor},
    literal_search::LiteralMatching,
    require, Result,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MAX_CITATION_IDS: usize = 100;
pub const MAX_CITATION_PAGE_ROWS: u32 = 50;
pub const MAX_CITATION_QUERY_BYTES: usize = 256;
pub const MAX_CITATION_PROJECTION_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_CITATION_CURSOR_BYTES: usize = 2048;

pub(crate) fn identifier(value: &str) -> Result<()> {
    require(
        !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control),
        "Citation identifier must contain 1 to 256 bytes without controls",
    )
}
fn ids(values: &[String], allow_empty: bool) -> Result<()> {
    require(
        (allow_empty || !values.is_empty()) && values.len() <= MAX_CITATION_IDS,
        "Citation ID set must contain at most 100 entries and a selected set cannot be empty",
    )?;
    let mut seen = BTreeSet::new();
    for value in values {
        identifier(value)?;
        require(seen.insert(value), "Duplicate citation identifier")?;
    }
    Ok(())
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CitationCatalogueRequest {
    pub query: String,
    pub excluded_ids: Vec<String>,
    pub page_size: u32,
    pub cursor: Option<String>,
}
impl Default for CitationCatalogueRequest {
    fn default() -> Self {
        Self {
            query: String::new(),
            excluded_ids: vec![],
            page_size: 50,
            cursor: None,
        }
    }
}
impl CitationCatalogueRequest {
    pub fn validate(&self) -> Result<()> {
        require(
            self.query.len() <= MAX_CITATION_QUERY_BYTES,
            "Citation search exceeds 256 UTF-8 bytes",
        )?;
        ids(&self.excluded_ids, true)?;
        require(
            (1..=MAX_CITATION_PAGE_ROWS).contains(&self.page_size),
            "Citation page size must be between 1 and 50",
        )?;
        require(
            self.cursor
                .as_ref()
                .is_none_or(|v| !v.is_empty() && v.len() <= MAX_CITATION_CURSOR_BYTES),
            "Citation cursor is empty or exceeds its bound",
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CitationSelectionsRequest {
    pub ids: Vec<String>,
}
impl CitationSelectionsRequest {
    pub fn validate(&self) -> Result<()> {
        ids(&self.ids, false)
    }
}
#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum CitationKind {
    Observation,
    Transaction,
    Evidence,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CitationEvidence {
    pub id: String,
    pub name: String,
    pub sha256: String,
    pub bytes: u64,
    pub media_type: String,
    pub origin_group: String,
    pub imported_at: String,
    pub extraction_status: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CitationSummary {
    Observation {
        id: String,
        entity_id: String,
        entity_name: Option<String>,
        field: String,
        value: String,
        review: ReviewState,
        anchor: SourceAnchor,
        source: CitationEvidence,
    },
    Transaction {
        id: String,
        description: String,
        amount: String,
        currency: String,
        date: String,
        account: String,
        review: ReviewState,
        version: u32,
        anchor: SourceAnchor,
        source: CitationEvidence,
    },
    Evidence {
        id: String,
        source: CitationEvidence,
    },
}
impl CitationSummary {
    pub fn id(&self) -> &str {
        match self {
            Self::Observation { id, .. }
            | Self::Transaction { id, .. }
            | Self::Evidence { id, .. } => id,
        }
    }
    pub fn kind(&self) -> CitationKind {
        match self {
            Self::Observation { .. } => CitationKind::Observation,
            Self::Transaction { .. } => CitationKind::Transaction,
            Self::Evidence { .. } => CitationKind::Evidence,
        }
    }
    pub fn source(&self) -> &CitationEvidence {
        match self {
            Self::Observation { source, .. }
            | Self::Transaction { source, .. }
            | Self::Evidence { source, .. } => source,
        }
    }
    pub(crate) fn validate(&self) -> Result<()> {
        identifier(self.id())?;
        identifier(&self.source().id)?;
        let source = self.source();
        require(
            source.id == source.sha256,
            "Citation source digest does not match its content-addressed identity",
        )?;
        require(
            source.sha256.len() == 64
                && source
                    .sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "Invalid citation source digest",
        )?;
        let expected_source = match self {
            Self::Observation {
                entity_id, anchor, ..
            } => {
                identifier(entity_id)?;
                anchor.evidence_id()
            }
            Self::Transaction {
                amount,
                date,
                version,
                anchor,
                ..
            } => {
                crate::analytics::amount(amount)?;
                crate::analytics::date(date)?;
                require(*version > 0, "Invalid citation transaction version")?;
                anchor.evidence_id()
            }
            Self::Evidence { id, .. } => id,
        };
        require(
            source.id == expected_source,
            "Citation source identity does not match its anchor",
        )
    }
    /// Same concatenated label/detail/source-name content as the original picker.
    /// Lower this complete string; per-field casing changes contextual mappings.
    pub(crate) fn search_text(&self) -> String {
        fn review(value: &ReviewState) -> &'static str {
            match value {
                ReviewState::Accepted => "accepted",
                ReviewState::Pending => "pending",
                ReviewState::Rejected => "rejected",
                ReviewState::Deferred => "deferred",
            }
        }
        match self {
            Self::Observation { entity_id, entity_name, field, value, review: state, source, .. } =>
                format!("{} · {field}: {value} Observation · {} {}", entity_name.as_deref().unwrap_or(entity_id), review(state), source.name),
            Self::Transaction { description, amount, currency, date, account, review: state, source, .. } =>
                format!("{description} · {amount} {currency} Transaction · {date} · account {account} · {} {}", review(state), source.name),
            Self::Evidence { source, .. } => format!("{} Whole source · {} {}", source.name, source.extraction_status, source.name),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CitationCataloguePage {
    pub schema_version: u32,
    pub workspace_revision: u64,
    pub matching: LiteralMatching,
    pub query_sha256: String,
    /// Literal-query matches after selected-ID exclusions, before pagination.
    pub scope_count: u64,
    pub rows: Vec<CitationSummary>,
    pub next_cursor: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CitationSelections {
    pub schema_version: u32,
    pub workspace_revision: u64,
    /// Exact requested set, in catalogue order; no partial success.
    pub rows: Vec<CitationSummary>,
}
