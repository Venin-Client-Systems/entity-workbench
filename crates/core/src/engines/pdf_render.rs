//! Disposable, scan-focused PDF page renderer. No canonical writes or word-region claims.
use super::{ocr, CancellationToken, Runtime};
use crate::{require, Error, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
#[cfg(target_os = "macos")]
mod runtime;

pub const MAX_PAGES: u32 = 1000;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RenderStatus {
    Rendered,
    Encrypted,
    Unsupported,
    Failed,
    QuotaExhausted,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RenderFailure {
    EncryptedDocument,
    UnsupportedFormat,
    UnsupportedFeature,
    ActiveContent,
    ExternalResource,
    MalformedDocument,
    PageOutOfRange,
    PageLimit,
    PixelLimit,
    StructureLimit,
    StreamLimit,
    OperatorLimit,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RenderLimitation {
    ScanFocusedSubset,
    AnnotationsExcluded,
    ColorConvertedToGray,
    UnreviewedRaster,
    NoWordRegions,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageGeometry {
    /// Effective PDF CropBox, clipped to MediaBox: x0,y0,x1,y1, in points (UserUnit=1).
    pub crop_box: [f64; 4],
    pub rotation_degrees: u32,
    /// [a,b,c,d,e,f]: raster x=a*x+c*y+e, raster y=b*x+d*y+f.
    /// Raster origin is top-left; bounds are floored as in PDFBox, without resampling.
    pub pdf_to_raster: [f64; 6],
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RasterBinding {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PdfRenderResult {
    pub protocol_version: u32,
    pub job_id: String,
    pub original_sha256: String,
    pub original_bytes: u64,
    pub renderer: String,
    pub java_runtime: String,
    pub page_number: u32,
    pub dpi: u32,
    pub page_count: Option<u32>,
    pub status: RenderStatus,
    pub failure: Option<RenderFailure>,
    pub geometry: Option<PageGeometry>,
    pub raster: Option<RasterBinding>,
    pub limitations: Vec<RenderLimitation>,
}
#[derive(Debug)]
pub struct RenderedPdf {
    pub result: PdfRenderResult,
    pub raster: Option<Vec<u8>>,
}
#[derive(Debug)]
pub struct PdfOcr {
    pub render: RenderedPdf,
    pub ocr: Option<ocr::OcrResult>,
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn request_limits(original: &[u8], page: u32, dpi: u32) -> Result<()> {
    require(
        !original.is_empty() && original.len() <= crate::policy::MAX_IMPORT_BYTES,
        "PDF input byte limit",
    )?;
    require(
        (1..=MAX_PAGES).contains(&page) && (72..=300).contains(&dpi),
        "PDF page or DPI outside supported bounds",
    )
}
fn mapping(geometry: &PageGeometry, dpi: u32) -> Result<([f64; 6], u32, u32)> {
    let [x0, y0, x1, y1] = geometry.crop_box;
    require(
        geometry
            .crop_box
            .iter()
            .all(|v| v.is_finite() && v.abs() <= 1_000_000.0 && (*v as f32) as f64 == *v)
            && x1 > x0
            && y1 > y0,
        "Invalid PDF page geometry",
    )?;
    let s = (dpi as f32 / 72.0) as f64;
    let width = (((x1 as f32 - x0 as f32) * s as f32).floor().max(1.0)) as u32;
    let height = (((y1 as f32 - y0 as f32) * s as f32).floor().max(1.0)) as u32;
    require(
        width <= ocr::MAX_DIMENSION
            && height <= ocr::MAX_DIMENSION
            && u64::from(width) * u64::from(height) <= ocr::MAX_PIXELS as u64,
        "PDF raster pixel limit",
    )?;
    // PageDrawer translates by the float-rounded box size, then by its origin.
    // Do not simplify these offsets to x1/y1: float subtraction can round first.
    let right = s * x0 + s * (x1 as f32 - x0 as f32) as f64;
    let top = s * y0 + s * (y1 as f32 - y0 as f32) as f64;
    let (affine, w, h) = match geometry.rotation_degrees {
        0 => ([s, 0.0, 0.0, -s, -s * x0, top], width, height),
        90 => ([0.0, s, s, 0.0, -s * y0, -s * x0], height, width),
        180 => ([-s, 0.0, 0.0, s, right, -s * y0], width, height),
        270 => ([0.0, -s, -s, 0.0, top, right], height, width),
        _ => return Err(Error::Validation("Invalid PDF page rotation".into())),
    };
    Ok((affine, w, h))
}
/// Validate transport, selected page/settings, exact original/raster bytes and coordinate mapping.
/// Geometry is the worker's extraction from the PDF, not independent re-parsing by the coordinator.
pub fn validate_result(
    result: &PdfRenderResult,
    original: &[u8],
    page: u32,
    dpi: u32,
    raster: Option<&[u8]>,
) -> Result<()> {
    request_limits(original, page, dpi)?;
    require(
        result.protocol_version == 1
            && uuid::Uuid::parse_str(&result.job_id)
                .is_ok_and(|id| id.to_string() == result.job_id)
            && result.original_sha256 == digest(original)
            && result.original_bytes == original.len() as u64
            && result.page_number == page
            && result.dpi == dpi,
        "PDF result does not match assigned request",
    )?;
    require(
        result.renderer == "pdfbox-3.0.8-scan-v1"
            && result.java_runtime.starts_with("21.")
            && result.java_runtime.len() <= 128
            && !result.java_runtime.chars().any(char::is_control),
        "Unsupported PDF renderer identity",
    )?;
    require(
        result.limitations
            == [
                RenderLimitation::ScanFocusedSubset,
                RenderLimitation::AnnotationsExcluded,
                RenderLimitation::ColorConvertedToGray,
                RenderLimitation::UnreviewedRaster,
                RenderLimitation::NoWordRegions,
            ],
        "Incomplete PDF renderer limitations",
    )?;
    require(
        result.failure == Some(RenderFailure::UnsupportedFormat) || original.starts_with(b"%PDF-"),
        "PDF outcome requires a PDF input",
    )?;
    require(
        result.page_count.is_none_or(|count| count <= MAX_PAGES)
            || result.failure == Some(RenderFailure::PageLimit),
        "PDF page count exceeds its outcome bound",
    )?;
    if let Some(count) = result.page_count {
        require(count > 0, "Invalid PDF page count")?;
    }
    if result.status == RenderStatus::Rendered {
        require(
            original.starts_with(b"%PDF-")
                && result.failure.is_none()
                && result
                    .page_count
                    .is_some_and(|count| count >= page && count <= MAX_PAGES),
            "Invalid PDF rendering success",
        )?;
        let geometry = result
            .geometry
            .as_ref()
            .ok_or_else(|| Error::Validation("Missing PDF geometry".into()))?;
        let binding = result
            .raster
            .as_ref()
            .ok_or_else(|| Error::Validation("Missing PDF raster binding".into()))?;
        let raster = raster.ok_or_else(|| Error::Validation("Missing PDF raster".into()))?;
        let (expected, width, height) = mapping(geometry, dpi)?;
        require(
            geometry.pdf_to_raster == expected,
            "PDF orientation mapping is inconsistent",
        )?;
        require(
            ocr::raster_dimensions(raster)? == (width, height)
                && binding.width == width
                && binding.height == height
                && binding.path == "raster.pgm"
                && binding.sha256 == digest(raster)
                && binding.bytes == raster.len() as u64,
            "PDF raster binding is inconsistent",
        )?;
    } else {
        require(
            raster.is_none() && result.raster.is_none() && result.geometry.is_none(),
            "Rejected PDF published a raster or geometry",
        )?;
        let valid = match (&result.status, &result.failure) {
            (RenderStatus::Encrypted, Some(RenderFailure::EncryptedDocument)) => {
                result.page_count.is_none()
            }
            (RenderStatus::Unsupported, Some(RenderFailure::UnsupportedFormat)) => {
                !original.starts_with(b"%PDF-") && result.page_count.is_none()
            }
            (
                RenderStatus::Unsupported,
                Some(
                    RenderFailure::UnsupportedFeature
                    | RenderFailure::ActiveContent
                    | RenderFailure::ExternalResource,
                ),
            ) => original.starts_with(b"%PDF-"),
            (RenderStatus::Failed, Some(RenderFailure::MalformedDocument)) => {
                original.starts_with(b"%PDF-")
            }
            (RenderStatus::Failed, Some(RenderFailure::PageOutOfRange)) => result
                .page_count
                .is_some_and(|count| count < page && count <= MAX_PAGES),
            (RenderStatus::QuotaExhausted, Some(RenderFailure::PageLimit)) => {
                result.page_count.is_some_and(|count| count > MAX_PAGES)
            }
            (
                RenderStatus::QuotaExhausted,
                Some(
                    RenderFailure::PixelLimit
                    | RenderFailure::StructureLimit
                    | RenderFailure::StreamLimit
                    | RenderFailure::OperatorLimit,
                ),
            ) => original.starts_with(b"%PDF-"),
            _ => false,
        };
        require(valid, "PDF status and failure do not agree")?;
    }
    Ok(())
}
impl Runtime {
    pub fn render_pdf_page(
        &self,
        scratch: &Path,
        original: &[u8],
        page: u32,
        dpi: u32,
    ) -> Result<RenderedPdf> {
        self.render_pdf_page_with_cancel(
            scratch,
            original,
            page,
            dpi,
            &CancellationToken::default(),
        )
    }
    pub fn render_pdf_page_with_cancel(
        &self,
        scratch: &Path,
        original: &[u8],
        page: u32,
        dpi: u32,
        token: &CancellationToken,
    ) -> Result<RenderedPdf> {
        request_limits(original, page, dpi)?;
        if token.is_cancelled() {
            return Err(Error::Blocked("PDF rendering cancelled".into()));
        }
        self.render_pdf_confined(scratch, original, page, dpi, token)
    }
    pub fn ocr_pdf_page(
        &self,
        scratch: &Path,
        original: &[u8],
        page: u32,
        dpi: u32,
    ) -> Result<PdfOcr> {
        self.ocr_pdf_page_with_cancel(scratch, original, page, dpi, &CancellationToken::default())
    }
    pub fn ocr_pdf_page_with_cancel(
        &self,
        scratch: &Path,
        original: &[u8],
        page: u32,
        dpi: u32,
        token: &CancellationToken,
    ) -> Result<PdfOcr> {
        let render = self.render_pdf_page_with_cancel(scratch, original, page, dpi, token)?;
        let ocr = match render.raster.as_deref() {
            Some(bytes) => Some(self.ocr_with_cancel(scratch, bytes, token)?),
            None => None,
        };
        Ok(PdfOcr { render, ocr })
    }
    #[cfg(not(target_os = "macos"))]
    fn render_pdf_confined(
        &self,
        _scratch: &Path,
        _original: &[u8],
        _page: u32,
        _dpi: u32,
        _token: &CancellationToken,
    ) -> Result<RenderedPdf> {
        Err(Error::Blocked(
            "PDF renderer confinement is not verified on this platform".into(),
        ))
    }
    #[cfg(target_os = "macos")]
    fn render_pdf_confined(
        &self,
        scratch: &Path,
        original: &[u8],
        page: u32,
        dpi: u32,
        token: &CancellationToken,
    ) -> Result<RenderedPdf> {
        use super::supervision;
        runtime::validate(&self.root)?;
        use crate::policy::{WorkerLimits, WorkerOperation, WorkerRequest};
        use std::{fs, os::unix::fs::PermissionsExt, time::Duration};
        fs::create_dir_all(scratch)?;
        let scratch = scratch.canonicalize()?;
        fs::set_permissions(&scratch, fs::Permissions::from_mode(0o700))?;
        let job = tempfile::Builder::new()
            .prefix("pdf-render-")
            .tempdir_in(scratch)?;
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
                    output_bytes: 8192,
                    pages: MAX_PAGES,
                    pixels: ocr::MAX_PIXELS as u64,
                    archive_members: 1,
                    archive_depth: 0,
                    expanded_bytes: 32 * 1024 * 1024,
                },
            };
            let encoded = serde_json::to_vec(&request)?;
            crate::policy::validate_worker_request(&encoded)?;
            super::write_new(&path.join("request.json"), &encoded)?;
            supervision::run_pdf_java(
                &self.root,
                &path,
                "workbench.PdfRenderWorker",
                &[page.to_string(), dpi.to_string()],
                Duration::from_secs(30),
                token,
            )?;
            let result: PdfRenderResult = serde_json::from_slice(&supervision::read_result(
                &path.join("result.json"),
                8192,
            )?)?;
            require(
                result.job_id == request.job_id,
                "PDF result belongs to another job",
            )?;
            let raster = if result.status == RenderStatus::Rendered {
                Some(supervision::read_result(
                    &path.join("raster.pgm"),
                    (ocr::MAX_PIXELS + 32) as u64,
                )?)
            } else {
                require(
                    !path.join("raster.pgm").exists(),
                    "Rejected PDF left a raster",
                )?;
                None
            };
            validate_result(&result, original, page, dpi, raster.as_deref())?;
            if token.is_cancelled() {
                return Err(Error::Blocked("PDF rendering cancelled".into()));
            }
            Ok(RenderedPdf { result, raster })
        })();
        supervision::finish_job(job, outcome)
    }
}
#[cfg(test)]
mod tests;
