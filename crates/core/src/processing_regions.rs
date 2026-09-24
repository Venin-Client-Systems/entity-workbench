//! Small immutable metadata for retained, unreviewed image word-region derivatives.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DerivativeKind {
    CanonicalPgmV1,
    OcrTsvV1,
    ImageRegionResultJsonV1,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DerivativeRef {
    pub sha256: String,
    pub bytes: u64,
    pub kind: DerivativeKind,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum ImageOcrRegionsInput {
    ImageOcrRegions {
        evidence_id: String,
        sha256: String,
        bytes: u64,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImageRegionExtractionRecord {
    pub schema_version: u32,
    pub id: String,
    pub job_id: String,
    pub attempt: u32,
    pub input: ImageOcrRegionsInput,
    pub created_at: String,
    pub result: DerivativeRef,
    pub raster: Option<DerivativeRef>,
    pub tsv: Option<DerivativeRef>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImageRegionResult {
    pub schema_version: u32,
    pub decoder: crate::engines::image::ImageDecodeResult,
    pub recognition: Option<crate::engines::ocr_regions::OcrRegionsResult>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImageRegionInspection {
    pub extraction: ImageRegionExtractionRecord,
    pub result: ImageRegionResult,
}
