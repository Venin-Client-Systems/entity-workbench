//! Confined PNG/JPEG decoding. Raster coordinates refer to encoded image pixels, not PDF pages.
use super::{ocr, CancellationToken, Runtime};
use crate::{require, Error, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DecodeStatus {
    Decoded,
    Unsupported,
    Failed,
    QuotaExhausted,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DecodeFailure {
    UnsupportedFormat,
    MultipleImages,
    MalformedImage,
    DecoderWarning,
    PixelLimit,
    ContainerLimit,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DecodeLimitation {
    ExifOrientationNotApplied,
    EmbeddedPreviewsExcluded,
    MetadataNotExtracted,
    ColorConvertedToGray,
    NoDocumentPageMapping,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PixelMapping {
    EncodedPixelsGrayWhiteAlphaV1,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RasterBinding {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
    pub source_image_index: u32,
    pub pixel_mapping: PixelMapping,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImageDecodeResult {
    pub protocol_version: u32,
    pub job_id: String,
    pub original_sha256: String,
    pub original_bytes: u64,
    pub decoder: String,
    pub java_runtime: String,
    pub media_type: String,
    pub status: DecodeStatus,
    pub failure: Option<DecodeFailure>,
    pub raster: Option<RasterBinding>,
    pub limitations: Vec<DecodeLimitation>,
}
/// Caller chooses how to retain this bounded derivative; workers never access canonical storage.
#[derive(Debug)]
pub struct DecodedImage {
    pub result: ImageDecodeResult,
    pub raster: Option<Vec<u8>>,
}
#[derive(Debug)]
pub struct ImageOcr {
    pub image: DecodedImage,
    pub ocr: Option<ocr::OcrResult>,
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn media_type(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        "image/jpeg"
    } else {
        "application/octet-stream"
    }
}
/// Shared canonical validation binds both exact original bytes and the returned raster.
pub fn validate_result(
    result: &ImageDecodeResult,
    original: &[u8],
    raster: Option<&[u8]>,
) -> Result<()> {
    require(
        original.len() <= crate::policy::MAX_IMPORT_BYTES,
        "Image input exceeds byte limit",
    )?;
    require(
        result.protocol_version == 1
            && uuid::Uuid::parse_str(&result.job_id)
                .is_ok_and(|id| id.to_string() == result.job_id),
        "Invalid image decoder protocol or job ID",
    )?;
    require(
        result.original_sha256 == digest(original)
            && result.original_bytes == original.len() as u64
            && result.media_type == media_type(original),
        "Decoded image is not bound to the assigned original",
    )?;
    require(
        result.decoder == "jdk-imageio-21-v1"
            && result.java_runtime.starts_with("21.")
            && result.java_runtime.len() <= 128
            && !result.java_runtime.chars().any(char::is_control),
        "Unsupported image decoder identity",
    )?;
    require(
        result.limitations
            == [
                DecodeLimitation::ExifOrientationNotApplied,
                DecodeLimitation::EmbeddedPreviewsExcluded,
                DecodeLimitation::MetadataNotExtracted,
                DecodeLimitation::ColorConvertedToGray,
                DecodeLimitation::NoDocumentPageMapping,
            ],
        "Image decoder limitations are incomplete",
    )?;
    match result.status {
        DecodeStatus::Decoded => {
            require(
                result.failure.is_none() && result.media_type != "application/octet-stream",
                "Invalid successful image decoding claim",
            )?;
            let binding = result
                .raster
                .as_ref()
                .ok_or_else(|| Error::Validation("Decoded image has no raster binding".into()))?;
            let bytes =
                raster.ok_or_else(|| Error::Validation("Decoded image has no raster".into()))?;
            let (width, height) = ocr::raster_dimensions(bytes)?;
            require(
                binding.path == "raster.pgm"
                    && binding.sha256 == digest(bytes)
                    && binding.bytes == bytes.len() as u64
                    && binding.width == width
                    && binding.height == height
                    && binding.source_image_index == 0,
                "Decoded raster does not match its source binding",
            )?;
        }
        _ => {
            require(
                result.raster.is_none() && raster.is_none(),
                "Failed or unsupported image cannot publish a raster",
            )?;
            let valid = match (&result.status, &result.failure) {
                (DecodeStatus::Unsupported, Some(DecodeFailure::UnsupportedFormat)) => {
                    result.media_type == "application/octet-stream"
                }
                (DecodeStatus::Unsupported, Some(DecodeFailure::MultipleImages)) => {
                    result.media_type != "application/octet-stream"
                }
                (
                    DecodeStatus::Failed,
                    Some(DecodeFailure::MalformedImage | DecodeFailure::DecoderWarning),
                ) => result.media_type != "application/octet-stream",
                (
                    DecodeStatus::QuotaExhausted,
                    Some(DecodeFailure::PixelLimit | DecodeFailure::ContainerLimit),
                ) => result.media_type != "application/octet-stream",
                _ => false,
            };
            require(valid, "Image status and failure do not agree")?;
        }
    }
    Ok(())
}
impl Runtime {
    pub fn decode_image(&self, scratch_root: &Path, original: &[u8]) -> Result<DecodedImage> {
        self.decode_image_with_cancel(scratch_root, original, &CancellationToken::default())
    }
    pub fn decode_image_with_cancel(
        &self,
        scratch_root: &Path,
        original: &[u8],
        cancellation: &CancellationToken,
    ) -> Result<DecodedImage> {
        require(
            original.len() <= crate::policy::MAX_IMPORT_BYTES,
            "Image input exceeds byte limit",
        )?;
        if cancellation.is_cancelled() {
            return Err(Error::Blocked("Image decoding cancelled".into()));
        }
        self.decode_image_confined(scratch_root, original, cancellation)
    }
    /// Each stage starts a separate disposable worker. Unsupported/failed images never enter OCR.
    pub fn ocr_image(&self, scratch_root: &Path, original: &[u8]) -> Result<ImageOcr> {
        self.ocr_image_with_cancel(scratch_root, original, &CancellationToken::default())
    }
    pub fn ocr_image_with_cancel(
        &self,
        scratch_root: &Path,
        original: &[u8],
        cancellation: &CancellationToken,
    ) -> Result<ImageOcr> {
        let image = self.decode_image_with_cancel(scratch_root, original, cancellation)?;
        let ocr = match image.raster.as_deref() {
            Some(raster) => Some(self.ocr_with_cancel(scratch_root, raster, cancellation)?),
            None => None,
        };
        Ok(ImageOcr { image, ocr })
    }
    #[cfg(not(target_os = "macos"))]
    fn decode_image_confined(
        &self,
        _scratch_root: &Path,
        _original: &[u8],
        _cancellation: &CancellationToken,
    ) -> Result<DecodedImage> {
        Err(Error::Blocked(
            "Image decoder confinement is not verified on this platform".into(),
        ))
    }
    #[cfg(target_os = "macos")]
    fn decode_image_confined(
        &self,
        scratch_root: &Path,
        original: &[u8],
        cancellation: &CancellationToken,
    ) -> Result<DecodedImage> {
        use super::supervision;
        use crate::policy::{WorkerLimits, WorkerOperation, WorkerRequest};
        use std::{fs, os::unix::fs::PermissionsExt, time::Duration};
        fs::create_dir_all(scratch_root)?;
        let scratch_root = scratch_root.canonicalize()?;
        fs::set_permissions(&scratch_root, fs::Permissions::from_mode(0o700))?;
        let job = tempfile::Builder::new()
            .prefix("image-")
            .tempdir_in(&scratch_root)?;
        let outcome = (|| {
            let path = job.path().canonicalize()?;
            super::write_new(&path.join("input.json"), original)?;
            let request = WorkerRequest {
                protocol_version: 1,
                job_id: uuid::Uuid::new_v4().to_string(),
                operation: WorkerOperation::Parse,
                inputs: vec!["input.json".into()],
                output: "result.json".into(),
                limits: WorkerLimits {
                    seconds: 30,
                    output_bytes: 4096,
                    pages: 1,
                    pixels: ocr::MAX_PIXELS as u64,
                    archive_members: 1,
                    archive_depth: 0,
                    expanded_bytes: (ocr::MAX_PIXELS + 32) as u64,
                },
            };
            let encoded = serde_json::to_vec(&request)?;
            crate::policy::validate_worker_request(&encoded)?;
            super::write_new(&path.join("request.json"), &encoded)?;
            supervision::run_image_java(
                &self.root,
                &path,
                "workbench.ImageWorker",
                &[],
                Duration::from_secs(30),
                cancellation,
            )?;
            let bytes = supervision::read_result(&path.join("result.json"), 4096)?;
            let result: ImageDecodeResult = serde_json::from_slice(&bytes)?;
            require(
                result.job_id == request.job_id,
                "Image decoding belongs to another job",
            )?;
            let raster = if result.status == DecodeStatus::Decoded {
                Some(supervision::read_result(
                    &path.join("raster.pgm"),
                    (ocr::MAX_PIXELS + 32) as u64,
                )?)
            } else {
                require(
                    !path.join("raster.pgm").exists(),
                    "Rejected image left an unexpected raster",
                )?;
                None
            };
            validate_result(&result, original, raster.as_deref())?;
            if cancellation.is_cancelled() {
                return Err(Error::Blocked("Image decoding cancelled".into()));
            }
            Ok(DecodedImage { result, raster })
        })();
        supervision::finish_job(job, outcome)
    }
}
#[cfg(test)]
mod tests;
