//! Shared case mapping. Domain adapters must bound input before these allocations.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LiteralSearchAlgorithm {
    UnicodeDefaultLowercaseLiteralV1,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LiteralMatching {
    pub algorithm: LiteralSearchAlgorithm,
    pub unicode_version: [u8; 3],
}
impl Default for LiteralMatching {
    fn default() -> Self {
        let (major, minor, patch) = std::char::UNICODE_VERSION;
        Self {
            algorithm: LiteralSearchAlgorithm::UnicodeDefaultLowercaseLiteralV1,
            unicode_version: [major, minor, patch],
        }
    }
}

/// Do not trim or lowercase individual characters: whole-string casing includes
/// context-sensitive Greek sigma and the locale-independent expansion mappings.
pub(crate) fn lower_query(query: &str) -> String {
    query.to_lowercase()
}

/// The caller supplies an already-lowered query and a complete bounded string,
/// including its domain-specific field separators. No normalization or patterns.
pub(crate) fn matches_text(text: &str, lowered_query: &str) -> bool {
    text.to_lowercase().contains(lowered_query)
}
