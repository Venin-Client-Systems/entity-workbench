//! Unreviewed raster-relative Tesseract hierarchy and word boxes. No canonical writes.
use super::{ocr, CancellationToken, Runtime};
use crate::{require, Error, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::Path;

mod tsv;
pub const MAX_TSV_BYTES: u64 = 2_000_000;
pub const MAX_REGIONS: usize = 20_000;
pub const MAX_WORDS: usize = 10_000;
pub const MAX_WORD_BYTES: usize = 2_000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RegionLevel {
    Page,
    Block,
    Paragraph,
    Line,
    Word,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RasterBox {
    pub left: u32,
    pub top: u32,
    pub width: u32,
    pub height: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct OcrRegion {
    pub level: RegionLevel,
    /// Tesseract's one-based raster page. This is never an original document page.
    pub page_number: u32,
    pub block_number: u32,
    pub paragraph_number: u32,
    pub line_number: u32,
    pub word_number: u32,
    /// Pixels, top-left origin, right/bottom edges excluded.
    pub bounds: RasterBox,
    /// Tesseract's finite 0–100 word score; structural rows carry None.
    /// This score is neither a calibrated probability nor analyst confidence.
    pub engine_confidence: Option<f64>,
    pub text: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RegionsStatus {
    Recognized,
    NoTextRecognized,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RegionsLimitation {
    UnreviewedRecognition,
    UnreviewedWordRegions,
    EngineConfidenceNotProbability,
    NoOriginalDocumentMapping,
    SingleUniformBlock,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OcrRegionsResult {
    pub protocol_version: u32,
    pub job_id: String,
    pub raster_sha256: String,
    pub raster_bytes: u64,
    pub width: u32,
    pub height: u32,
    pub engine: String,
    pub language: String,
    pub model_sha256: String,
    pub runtime_manifest_sha256: String,
    pub status: RegionsStatus,
    /// Companion text output, preserving the engine's original whitespace.
    pub text: String,
    pub tsv_sha256: String,
    pub tsv_bytes: u64,
    pub regions: Vec<OcrRegion>,
    pub limitations: Vec<RegionsLimitation>,
}
fn limitations() -> Vec<RegionsLimitation> {
    vec![
        RegionsLimitation::UnreviewedRecognition,
        RegionsLimitation::UnreviewedWordRegions,
        RegionsLimitation::EngineConfidenceNotProbability,
        RegionsLimitation::NoOriginalDocumentMapping,
        RegionsLimitation::SingleUniformBlock,
    ]
}

/// Revalidate the exact raster, TSV bytes and typed result before any future canonical use.
/// No source-page mapping or acceptance decision is established here.
pub fn validate_result(result: &OcrRegionsResult, raster: &[u8], tsv: &[u8]) -> Result<()> {
    let (width, height) = ocr::raster_dimensions(raster)?;
    require(
        tsv.len() < MAX_TSV_BYTES as usize && result.regions.len() <= MAX_REGIONS,
        "OCR region result exceeds policy",
    )?;
    require(
        result.protocol_version == 1,
        "Unsupported OCR regions protocol",
    )?;
    ocr::validate_identity(
        &result.job_id,
        &result.engine,
        &result.language,
        &result.model_sha256,
        &result.runtime_manifest_sha256,
    )?;
    require(
        result.raster_sha256 == ocr::digest(raster)
            && result.raster_bytes == raster.len() as u64
            && result.width == width
            && result.height == height,
        "OCR regions do not match the assigned raster",
    )?;
    require(
        result.tsv_bytes == tsv.len() as u64 && result.tsv_sha256 == ocr::digest(tsv),
        "OCR regions do not match the TSV output",
    )?;
    let expected = tsv::parse(tsv, &result.text, width, height)?;
    require(
        result.regions.len() <= MAX_REGIONS && result.regions == expected,
        "OCR region metadata differs from validated TSV",
    )?;
    require(
        (result.status == RegionsStatus::NoTextRecognized) == result.text.trim().is_empty(),
        "OCR region status conflicts with recognized text",
    )?;
    require(
        result.limitations == limitations(),
        "OCR region limitations are incomplete",
    )
}

/// TSV remains bounded transient validation material; no persistence is implied.
#[derive(Debug)]
pub struct OcrRegions {
    pub result: OcrRegionsResult,
    pub tsv: Vec<u8>,
}
impl Runtime {
    pub fn ocr_regions(&self, scratch_root: &Path, raster: &[u8]) -> Result<OcrRegions> {
        self.ocr_regions_with_cancel(scratch_root, raster, &CancellationToken::default())
    }
    pub fn ocr_regions_with_cancel(
        &self,
        scratch_root: &Path,
        raster: &[u8],
        cancellation: &CancellationToken,
    ) -> Result<OcrRegions> {
        ocr::raster_dimensions(raster)?;
        if cancellation.is_cancelled() {
            return Err(Error::Blocked("OCR regions cancelled".into()));
        }
        self.ocr_regions_confined(scratch_root, raster, cancellation)
    }
    #[cfg(not(target_os = "macos"))]
    fn ocr_regions_confined(
        &self,
        _scratch_root: &Path,
        _raster: &[u8],
        _cancellation: &CancellationToken,
    ) -> Result<OcrRegions> {
        Err(Error::Blocked(
            "OCR word-region confinement is not verified on this platform".into(),
        ))
    }
    #[cfg(target_os = "macos")]
    fn ocr_regions_confined(
        &self,
        scratch_root: &Path,
        raster: &[u8],
        cancellation: &CancellationToken,
    ) -> Result<OcrRegions> {
        use super::supervision;
        use std::{fs, os::unix::fs::PermissionsExt, time::Duration};
        let runtime = self
            .root
            .join("ocr")
            .canonicalize()
            .map_err(|_| Error::Blocked("Packaged OCR runtime is unavailable".into()))?;
        let manifest = ocr::validate_runtime(&runtime)?;
        fs::create_dir_all(scratch_root)?;
        let scratch_root = scratch_root.canonicalize()?;
        fs::set_permissions(&scratch_root, fs::Permissions::from_mode(0o700))?;
        let job = tempfile::Builder::new()
            .prefix("ocr-regions-")
            .tempdir_in(&scratch_root)?;
        let outcome = (|| {
            let path = job.path().canonicalize()?;
            super::write_new(&path.join("input.pgm"), raster)?;
            supervision::ocr::run_regions(&runtime, &path, Duration::from_secs(30), cancellation)?;
            let tsv = supervision::read_result(&path.join("result.tsv"), MAX_TSV_BYTES)?;
            let text = String::from_utf8(supervision::read_result(
                &path.join("result.txt"),
                ocr::MAX_TEXT_BYTES,
            )?)
            .map_err(|_| Error::Validation("OCR returned invalid UTF-8 text".into()))?;
            let (width, height) = ocr::raster_dimensions(raster)?;
            let regions = tsv::parse(&tsv, &text, width, height)?;
            let result = OcrRegionsResult {
                protocol_version: 1,
                job_id: uuid::Uuid::new_v4().to_string(),
                raster_sha256: ocr::digest(raster),
                raster_bytes: raster.len() as u64,
                width,
                height,
                engine: "tesseract-5.5.2".into(),
                language: "eng".into(),
                model_sha256: ocr::MODEL_SHA256.into(),
                runtime_manifest_sha256: manifest,
                status: if text.trim().is_empty() {
                    RegionsStatus::NoTextRecognized
                } else {
                    RegionsStatus::Recognized
                },
                text,
                tsv_sha256: ocr::digest(&tsv),
                tsv_bytes: tsv.len() as u64,
                regions,
                limitations: limitations(),
            };
            validate_result(&result, raster, &tsv)?;
            if cancellation.is_cancelled() {
                return Err(Error::Blocked("OCR regions cancelled".into()));
            }
            Ok(OcrRegions { result, tsv })
        })();
        supervision::finish_job(job, outcome)
    }
}
#[cfg(test)]
mod tests;
