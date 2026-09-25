//! Fixed synthetic values for canonical state/UI tests. These functions do not execute workers.
use super::*;
use crate::engines::{ocr::*, pdf_render::*};
pub(super) const PDF: &[u8] = include_bytes!("../../../../fixtures/pdf-render/scan.pdf");

pub(super) fn recognized(original: &[u8], page_number: u32, dpi: u32) -> PdfOcr {
    let scale = f64::from(dpi as f32 / 72.0);
    // State fixtures use 72/144 DPI and a 600 × 115 point page. Actual native tests render independently.
    let width = (600.0 * scale).floor() as u32;
    let height = (115.0 * scale).floor() as u32;
    let raster = if dpi == 144 {
        include_bytes!("../../../../fixtures/ocr/synthetic.pgm").to_vec()
    } else {
        let mut bytes = format!("P5\n{width} {height}\n255\n").into_bytes();
        bytes.resize(bytes.len() + (width * height) as usize, 255);
        bytes
    };
    let render = PdfRenderResult {
        protocol_version: 1,
        job_id: id(),
        original_sha256: hash(original),
        original_bytes: original.len() as u64,
        renderer: "pdfbox-3.0.8-scan-v1".into(),
        java_runtime: "21.synthetic-fixture".into(),
        page_number,
        dpi,
        page_count: Some(2),
        status: RenderStatus::Rendered,
        failure: None,
        geometry: Some(PageGeometry {
            crop_box: [0.0, 0.0, 600.0, 115.0],
            rotation_degrees: 0,
            pdf_to_raster: [scale, 0.0, 0.0, -scale, 0.0, 115.0 * scale],
        }),
        raster: Some(RasterBinding {
            path: "raster.pgm".into(),
            sha256: hash(&raster),
            bytes: raster.len() as u64,
            width,
            height,
        }),
        limitations: vec![
            RenderLimitation::ScanFocusedSubset,
            RenderLimitation::AnnotationsExcluded,
            RenderLimitation::ColorConvertedToGray,
            RenderLimitation::UnreviewedRaster,
            RenderLimitation::NoWordRegions,
        ],
    };
    let recognition = OcrResult {
        protocol_version: 1,
        job_id: id(),
        raster_sha256: hash(&raster),
        raster_bytes: raster.len() as u64,
        width,
        height,
        engine: "tesseract-5.5.2".into(),
        language: "eng".into(),
        model_sha256: "7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2".into(),
        runtime_manifest_sha256: "0".repeat(64),
        status: OcrStatus::Recognized,
        text: concat!(
            "FIXED SYNTHETIC UI SPECIMEN — NO WORKER RAN\n",
            "REFERENCE 0042 AMOUNT 123.45\n",
            "<script>window.pdfRecognitionExecuted=true</script>",
        )
        .into(),
        limitations: vec![
            OcrLimitation::UnreviewedRecognition,
            OcrLimitation::NoWordRegions,
            OcrLimitation::NoOriginalDocumentMapping,
        ],
    };
    PdfOcr {
        render: RenderedPdf {
            result: render,
            raster: Some(raster),
        },
        ocr: Some(recognition),
    }
}

pub(super) fn outcome(mode: &str, original: &[u8], page: u32, dpi: u32) -> PdfOcr {
    let mut value = recognized(original, page, dpi);
    match mode {
        "recognized" => return value,
        "empty" => {
            let ocr = value.ocr.as_mut().unwrap();
            ocr.status = OcrStatus::NoTextRecognized;
            ocr.text = "\n\x0c".into();
            return value;
        }
        "encrypted" => {
            value.render.result.status = RenderStatus::Encrypted;
            value.render.result.failure = Some(RenderFailure::EncryptedDocument);
            value.render.result.page_count = None;
        }
        "unsupported" => {
            value.render.result.status = RenderStatus::Unsupported;
            value.render.result.failure = Some(RenderFailure::UnsupportedFeature);
        }
        "failed" => {
            value.render.result.status = RenderStatus::Failed;
            value.render.result.failure = Some(RenderFailure::MalformedDocument);
        }
        "quota" => {
            value.render.result.status = RenderStatus::QuotaExhausted;
            value.render.result.failure = Some(RenderFailure::PixelLimit);
        }
        _ => panic!("Unknown fixed PDF specimen"),
    }
    value.render.result.geometry = None;
    value.render.result.raster = None;
    value.render.raster = None;
    value.ocr = None;
    value
}
