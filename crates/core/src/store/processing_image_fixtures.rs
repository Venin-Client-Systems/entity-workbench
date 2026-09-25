//! Fixed synthetic protocol values for state/UI tests. No OS worker runs here.
use super::*;
use crate::engines::{image::*, ocr::*};

pub(super) const PNG: &[u8] = include_bytes!("../../../../fixtures/images/synthetic.png");
pub(super) const GIF: &[u8] = include_bytes!("../../../../fixtures/images/unsupported.gif");

pub(super) fn recognized() -> ImageOcr {
    let raster = include_bytes!("../../../../fixtures/ocr/synthetic.pgm").to_vec();
    let (width, height) = raster_dimensions(&raster).unwrap();
    ImageOcr {
        image: DecodedImage {
            result: ImageDecodeResult {
                protocol_version: 1, job_id: id(), original_sha256: hash(PNG), original_bytes: PNG.len() as u64,
                decoder: "jdk-imageio-21-v1".into(), java_runtime: "21.synthetic-fixture".into(), media_type: "image/png".into(),
                status: DecodeStatus::Decoded, failure: None,
                raster: Some(RasterBinding { path: "raster.pgm".into(), sha256: hash(&raster), bytes: raster.len() as u64, width, height, source_image_index: 0, pixel_mapping: PixelMapping::EncodedPixelsGrayWhiteAlphaV1 }),
                limitations: vec![DecodeLimitation::ExifOrientationNotApplied, DecodeLimitation::EmbeddedPreviewsExcluded, DecodeLimitation::MetadataNotExtracted, DecodeLimitation::ColorConvertedToGray, DecodeLimitation::NoDocumentPageMapping],
            },
            raster: Some(raster.clone()),
        },
        ocr: Some(OcrResult {
            protocol_version: 1, job_id: id(), raster_sha256: hash(&raster), raster_bytes: raster.len() as u64, width, height,
            engine: "tesseract-5.5.2".into(), language: "eng".into(),
            model_sha256: "7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2".into(),
            runtime_manifest_sha256: "0".repeat(64), status: OcrStatus::Recognized,
            text: "FIXED SYNTHETIC UI SPECIMEN — NO WORKER RAN\nREFERENCE 0042 AMOUNT 123.45\n<script>window.imageRecognitionExecuted=true</script>".into(),
            limitations: vec![OcrLimitation::UnreviewedRecognition, OcrLimitation::NoWordRegions, OcrLimitation::NoOriginalDocumentMapping],
        }),
    }
}

pub(super) fn outcome(mode: &str) -> ImageOcr {
    let mut value = recognized();
    match mode {
        "recognized" => return value,
        "empty" => {
            let recognition = value.ocr.as_mut().unwrap();
            recognition.status = OcrStatus::NoTextRecognized;
            recognition.text = "\n\x0c".into();
            return value;
        }
        "unsupported" => {
            value.image.result.original_sha256 = hash(GIF);
            value.image.result.original_bytes = GIF.len() as u64;
            value.image.result.media_type = "application/octet-stream".into();
            value.image.result.status = DecodeStatus::Unsupported;
            value.image.result.failure = Some(DecodeFailure::UnsupportedFormat);
        }
        "failed" => {
            value.image.result.status = DecodeStatus::Failed;
            value.image.result.failure = Some(DecodeFailure::MalformedImage);
        }
        "quota" => {
            value.image.result.status = DecodeStatus::QuotaExhausted;
            value.image.result.failure = Some(DecodeFailure::PixelLimit);
        }
        _ => panic!("Unknown fixed synthetic outcome"),
    }
    value.image.result.raster = None;
    value.image.raster = None;
    value.ocr = None;
    value
}
