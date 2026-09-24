//! English OCR of a bounded, already decoded raster. No original-document anchors.
use super::{CancellationToken, Runtime};
use crate::{require, Error, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

pub const MAX_PIXELS: usize = 12_000_000;
pub const MAX_DIMENSION: u32 = 8192;
pub const MAX_TEXT_BYTES: u64 = 512_000;
const MODEL_SHA256: &str = "7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2";

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OcrStatus {
    Recognized,
    NoTextRecognized,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OcrLimitation {
    UnreviewedRecognition,
    NoWordRegions,
    NoOriginalDocumentMapping,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OcrResult {
    pub protocol_version: u32,
    pub job_id: String,
    /// The exact canonical PGM raster, never the original PDF or compressed image.
    pub raster_sha256: String,
    pub raster_bytes: u64,
    pub width: u32,
    pub height: u32,
    pub engine: String,
    pub language: String,
    pub model_sha256: String,
    pub runtime_manifest_sha256: String,
    pub status: OcrStatus,
    pub text: String,
    pub limitations: Vec<OcrLimitation>,
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn is_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}
/// Strict P5 subset: `P5\n{width} {height}\n255\n` followed by exactly width*height bytes.
/// Comments, alternate max values, multiple images and trailing data are rejected.
/// This is a raster interchange contract, not a general user-image decoder.
pub fn raster_dimensions(bytes: &[u8]) -> Result<(u32, u32)> {
    require(
        bytes.len() <= MAX_PIXELS + 32,
        "OCR raster exceeds input limit",
    )?;
    let mut lines = bytes.splitn(4, |byte| *byte == b'\n');
    require(
        lines.next() == Some(b"P5"),
        "Unsupported OCR raster encoding",
    )?;
    let dimensions = std::str::from_utf8(lines.next().unwrap_or_default())
        .map_err(|_| Error::Validation("Invalid OCR raster dimensions".into()))?;
    let (width, height) = dimensions
        .split_once(' ')
        .ok_or_else(|| Error::Validation("Invalid OCR raster dimensions".into()))?;
    let width: u32 = width
        .parse()
        .map_err(|_| Error::Validation("Invalid OCR width".into()))?;
    let height: u32 = height
        .parse()
        .map_err(|_| Error::Validation("Invalid OCR height".into()))?;
    require(
        dimensions == format!("{width} {height}"),
        "Non-canonical OCR dimensions",
    )?;
    require(
        (1..=MAX_DIMENSION).contains(&width) && (1..=MAX_DIMENSION).contains(&height),
        "OCR dimensions exceed policy",
    )?;
    let pixels = u64::from(width) * u64::from(height);
    require(
        pixels <= MAX_PIXELS as u64,
        "OCR pixel count exceeds policy",
    )?;
    require(
        lines.next() == Some(b"255"),
        "OCR requires 8-bit grayscale pixels",
    )?;
    require(
        lines.next().is_some_and(|body| body.len() as u64 == pixels),
        "OCR raster length does not match dimensions",
    )?;
    Ok((width, height))
}

/// Canonical acceptance can reuse these rules without trusting worker output.
pub fn validate_result(result: &OcrResult, raster: &[u8]) -> Result<()> {
    let (width, height) = raster_dimensions(raster)?;
    require(result.protocol_version == 1, "Unsupported OCR protocol")?;
    require(
        uuid::Uuid::parse_str(&result.job_id).is_ok_and(|id| id.to_string() == result.job_id),
        "Invalid OCR job ID",
    )?;
    require(
        result.raster_sha256 == digest(raster)
            && result.raster_bytes == raster.len() as u64
            && result.width == width
            && result.height == height,
        "OCR result does not match the assigned raster",
    )?;
    require(
        result.engine == "tesseract-5.5.2"
            && result.language == "eng"
            && result.model_sha256 == MODEL_SHA256
            && is_digest(&result.runtime_manifest_sha256),
        "Unsupported OCR runtime identity",
    )?;
    require(
        result.text.len() as u64 <= MAX_TEXT_BYTES
            && result.text.chars().count() <= 128_000
            && !result
                .text
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t' | '\x0c')),
        "Invalid or oversized OCR text",
    )?;
    require(
        result.limitations
            == [
                OcrLimitation::UnreviewedRecognition,
                OcrLimitation::NoWordRegions,
                OcrLimitation::NoOriginalDocumentMapping,
            ],
        "OCR limitations are incomplete",
    )?;
    require(
        (result.status == OcrStatus::NoTextRecognized) == result.text.trim().is_empty(),
        "OCR status conflicts with recognized text",
    )
}

impl Runtime {
    pub fn ocr(&self, scratch_root: &Path, raster: &[u8]) -> Result<OcrResult> {
        self.ocr_with_cancel(scratch_root, raster, &CancellationToken::default())
    }
    pub fn ocr_with_cancel(
        &self,
        scratch_root: &Path,
        raster: &[u8],
        cancellation: &CancellationToken,
    ) -> Result<OcrResult> {
        raster_dimensions(raster)?;
        if cancellation.is_cancelled() {
            return Err(Error::Blocked("OCR cancelled".into()));
        }
        self.ocr_confined(scratch_root, raster, cancellation)
    }
    #[cfg(not(target_os = "macos"))]
    fn ocr_confined(
        &self,
        _scratch_root: &Path,
        _raster: &[u8],
        _cancellation: &CancellationToken,
    ) -> Result<OcrResult> {
        Err(Error::Blocked(
            "OCR confinement is not verified on this platform".into(),
        ))
    }
    #[cfg(target_os = "macos")]
    fn ocr_confined(
        &self,
        scratch_root: &Path,
        raster: &[u8],
        cancellation: &CancellationToken,
    ) -> Result<OcrResult> {
        use super::supervision;
        use std::{fs, os::unix::fs::PermissionsExt, time::Duration};
        let runtime = self
            .root
            .join("ocr")
            .canonicalize()
            .map_err(|_| Error::Blocked("Packaged OCR runtime is unavailable".into()))?;
        let manifest = validate_runtime(&runtime)?;
        fs::create_dir_all(scratch_root)?;
        let scratch_root = scratch_root.canonicalize()?;
        fs::set_permissions(&scratch_root, fs::Permissions::from_mode(0o700))?;
        let job = tempfile::Builder::new()
            .prefix("ocr-")
            .tempdir_in(&scratch_root)?;
        let outcome = (|| {
            let job_path = job.path().canonicalize()?;
            super::write_new(&job_path.join("input.pgm"), raster)?;
            supervision::ocr::run(&runtime, &job_path, Duration::from_secs(30), cancellation)?;
            let bytes = supervision::read_result(&job_path.join("result.txt"), MAX_TEXT_BYTES)?;
            let text = String::from_utf8(bytes)
                .map_err(|_| Error::Validation("OCR returned invalid UTF-8".into()))?;
            let (width, height) = raster_dimensions(raster)?;
            let result = OcrResult {
                protocol_version: 1,
                job_id: uuid::Uuid::new_v4().to_string(),
                raster_sha256: digest(raster),
                raster_bytes: raster.len() as u64,
                width,
                height,
                engine: "tesseract-5.5.2".into(),
                language: "eng".into(),
                model_sha256: MODEL_SHA256.into(),
                runtime_manifest_sha256: manifest,
                status: if text.trim().is_empty() {
                    OcrStatus::NoTextRecognized
                } else {
                    OcrStatus::Recognized
                },
                text,
                limitations: vec![
                    OcrLimitation::UnreviewedRecognition,
                    OcrLimitation::NoWordRegions,
                    OcrLimitation::NoOriginalDocumentMapping,
                ],
            };
            validate_result(&result, raster)?;
            if cancellation.is_cancelled() {
                return Err(Error::Blocked("OCR cancelled".into()));
            }
            Ok(result)
        })();
        supervision::finish_job(job, outcome)
    }
}

#[cfg(target_os = "macos")]
fn validate_runtime(runtime: &Path) -> Result<String> {
    use std::{
        collections::{BTreeMap, BTreeSet},
        fs,
        path::Component,
    };
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Asset {
        bytes: u64,
        sha256: String,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Manifest {
        schema_version: u32,
        os: String,
        architecture: String,
        engine: String,
        language: String,
        model_sha256: String,
        files: BTreeMap<String, Asset>,
    }
    let bytes = super::supervision::read_result(&runtime.join("manifest.json"), 256_000)
        .map_err(|_| Error::Blocked("Packaged OCR manifest is unavailable".into()))?;
    let manifest: Manifest = serde_json::from_slice(&bytes)?;
    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x86_64"
    };
    require(
        manifest.schema_version == 1
            && manifest.os == "macos"
            && manifest.architecture == arch
            && manifest.engine == "tesseract-5.5.2"
            && manifest.language == "eng"
            && manifest.model_sha256 == MODEL_SHA256,
        "OCR runtime identity is unsupported",
    )?;
    require(
        (3..=128).contains(&manifest.files.len()),
        "OCR manifest file count exceeds policy",
    )?;
    for required in ["bin/tesseract", "tessdata/eng.traineddata", "NOTICE.json"] {
        require(
            manifest.files.contains_key(required),
            "Required OCR asset is absent from manifest",
        )?;
    }
    for (name, asset) in &manifest.files {
        require(
            !name.is_empty()
                && Path::new(&name)
                    .components()
                    .all(|part| matches!(part, Component::Normal(_)))
                && is_digest(&asset.sha256),
            "Unsafe OCR manifest entry",
        )?;
    }
    // The profile permits the lib/tessdata directories. Every file reachable
    // there must be inventoried, not merely the subset named by a manifest.
    let expected: BTreeSet<_> = manifest
        .files
        .keys()
        .cloned()
        .chain(std::iter::once("manifest.json".into()))
        .collect();
    let mut actual = BTreeSet::new();
    let mut pending = vec![(runtime.to_path_buf(), 0usize)];
    let mut entries = 0usize;
    while let Some((directory, depth)) = pending.pop() {
        require(depth <= 4, "OCR runtime directory depth exceeds policy")?;
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            entries += 1;
            require(entries <= 256, "OCR runtime directory count exceeds policy")?;
            let metadata = fs::symlink_metadata(entry.path())?;
            require(
                !metadata.file_type().is_symlink(),
                "Linked OCR asset rejected",
            )?;
            if metadata.is_dir() {
                pending.push((entry.path(), depth + 1));
            } else {
                require(metadata.is_file(), "Special OCR asset rejected")?;
                let path = entry.path();
                let relative = path
                    .strip_prefix(runtime)
                    .map_err(|_| Error::Validation("Invalid OCR runtime path".into()))?;
                let relative = relative
                    .to_str()
                    .ok_or_else(|| Error::Validation("Non-UTF8 OCR runtime path".into()))?;
                actual.insert(relative.to_owned());
            }
        }
    }
    if !expected.is_subset(&actual) {
        return Err(Error::Blocked("Packaged OCR asset is missing".into()));
    }
    require(
        actual == expected,
        "OCR runtime inventory differs from manifest",
    )?;
    let mut total = 0u64;
    for (name, asset) in manifest.files {
        total = total
            .checked_add(asset.bytes)
            .ok_or_else(|| Error::Validation("OCR asset size overflow".into()))?;
        require(
            asset.bytes <= 64 * 1024 * 1024 && total <= 256 * 1024 * 1024,
            "OCR runtime exceeds size policy",
        )?;
        let mut path = runtime.to_path_buf();
        for component in Path::new(&name).components() {
            path.push(component);
            require(
                !fs::symlink_metadata(&path)
                    .map_err(|_| Error::Blocked("Packaged OCR asset is missing".into()))?
                    .file_type()
                    .is_symlink(),
                "Linked OCR asset rejected",
            )?;
        }
        let content = super::supervision::read_result(&path, asset.bytes)?;
        require(
            content.len() as u64 == asset.bytes && digest(&content) == asset.sha256,
            "OCR runtime asset failed integrity verification",
        )?;
        if name == "tessdata/eng.traineddata" {
            require(asset.sha256 == MODEL_SHA256, "Unsupported OCR model")?;
        }
    }
    Ok(digest(&bytes))
}

#[cfg(test)]
mod tests;
