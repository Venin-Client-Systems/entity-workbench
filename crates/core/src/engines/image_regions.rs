//! Opt-in image decode → word-region pipeline. Existing text-only OCR is unchanged.
use super::{image, ocr_regions, CancellationToken, Runtime};
use crate::{require, Error, Result};
use std::path::Path;
#[derive(Debug)]
pub struct ImageOcrRegions {
    pub image: image::DecodedImage,
    pub ocr: Option<ocr_regions::OcrRegions>,
}
pub fn validate_result(
    decoder: &image::ImageDecodeResult,
    original: &[u8],
    raster: Option<&[u8]>,
    recognition: Option<&ocr_regions::OcrRegionsResult>,
    tsv: Option<&[u8]>,
) -> Result<()> {
    image::validate_result(decoder, original, raster)?;
    match (&decoder.status, raster, recognition, tsv) {
        (image::DecodeStatus::Decoded, Some(raster), Some(recognition), Some(tsv)) => {
            ocr_regions::validate_result(recognition, raster, tsv)?;
            require(
                decoder.job_id != recognition.job_id,
                "Decoder and region OCR must be separate worker jobs",
            )
        }
        (image::DecodeStatus::Decoded, _, _, _) => Err(Error::Validation(
            "Decoded image has incomplete word-region output".into(),
        )),
        (_, None, None, None) => Ok(()),
        _ => Err(Error::Validation(
            "Rejected image has word-region output".into(),
        )),
    }
}
impl Runtime {
    pub fn ocr_image_regions_with_cancel(
        &self,
        scratch_root: &Path,
        original: &[u8],
        cancellation: &CancellationToken,
    ) -> Result<ImageOcrRegions> {
        let image = self.decode_image_with_cancel(scratch_root, original, cancellation)?;
        let ocr = match image.raster.as_deref() {
            Some(raster) => {
                Some(self.ocr_regions_with_cancel(scratch_root, raster, cancellation)?)
            }
            None => None,
        };
        validate_result(
            &image.result,
            original,
            image.raster.as_deref(),
            ocr.as_ref().map(|value| &value.result),
            ocr.as_ref().map(|value| value.tsv.as_slice()),
        )?;
        Ok(ImageOcrRegions { image, ocr })
    }
}
