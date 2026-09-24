//! Canonical acceptance of exact selected-page rendering and recognition, without raster retention.
use super::*;
use crate::engines::pdf_render::{self, PdfOcr, RenderStatus};

pub(super) fn summary(output: &PdfOcr) -> PdfExtractionResult {
    PdfExtractionResult {
        render: output.render.result.clone(),
        recognition: output.ocr.clone(),
        raster_retained: false,
    }
}
impl Workspace {
    pub fn pdf_extraction(&self, extraction_id: &str) -> Result<PdfExtractionRecord> {
        get(&self.conn, "pdf_extraction", extraction_id)
    }
}
pub(super) fn validate(
    job: &mut ProcessingJob,
    original: &[u8],
    output: &PdfOcr,
    key: String,
) -> Result<Derivative> {
    let ProcessingInput::PdfPageOcr {
        evidence_id,
        sha256,
        bytes,
        page_number,
        dpi,
    } = &job.input
    else {
        return Err(Error::Validation(
            "PDF output does not match its assigned operation".into(),
        ));
    };
    pdf_render::validate_result(
        &output.render.result,
        original,
        *page_number,
        *dpi,
        output.render.raster.as_deref(),
    )?;
    match (
        &output.render.result.status,
        output.render.raster.as_deref(),
        &output.ocr,
    ) {
        (RenderStatus::Rendered, Some(raster), Some(recognition)) => {
            ocr::validate_result(recognition, raster)?;
            require(
                recognition.job_id != output.render.result.job_id,
                "PDF renderer and OCR must be separate worker jobs",
            )?;
        }
        (RenderStatus::Rendered, _, _) => {
            return Err(Error::Validation(
                "Rendered PDF has no validated recognition".into(),
            ))
        }
        (_, None, None) => {}
        _ => {
            return Err(Error::Validation(
                "Rejected PDF cannot publish recognition".into(),
            ))
        }
    }
    let result = summary(output);
    let record = PdfExtractionRecord {
        schema_version: 1,
        id: key,
        job_id: job.id.clone(),
        attempt: job.attempt,
        input: PdfPageOcrInput::PdfPageOcr {
            evidence_id: evidence_id.clone(),
            sha256: sha256.clone(),
            bytes: *bytes,
            page_number: *page_number,
            dpi: *dpi,
        },
        created_at: now(),
        result_sha256: hash(&serde_json::to_vec(&result)?),
        result,
    };
    let (state, failure, detail) = match output.render.result.status {
        RenderStatus::Rendered => {
            let empty = output.ocr.as_ref().is_some_and(|value| {
                value.status == ocr::OcrStatus::NoTextRecognized
            });
            let detail = if empty {
                "Selected PDF page rendered and OCR completed without recognized text. No facts were accepted; raster was not retained"
            } else {
                "Selected PDF page OCR completed; unreviewed recognition and page provenance retained. Raster was not retained"
            };
            (ProcessingState::Completed, None, detail)
        }
        RenderStatus::Encrypted => (
            ProcessingState::Blocked,
            Some(ProcessingFailure::EncryptedDocument),
            "PDF is encrypted; no page raster or recognition was published",
        ),
        RenderStatus::Unsupported => (
            ProcessingState::Blocked,
            Some(ProcessingFailure::UnsupportedFormat),
            "The PDF rendering profile does not support this input or feature; inspect its typed outcome. No OCR was run",
        ),
        RenderStatus::Failed => (
            ProcessingState::Failed,
            Some(ProcessingFailure::PdfRenderFailed),
            "Selected PDF page rendering failed; its typed outcome was retained. No OCR was run",
        ),
        RenderStatus::QuotaExhausted => (
            ProcessingState::QuotaExhausted,
            Some(ProcessingFailure::WorkerFailed),
            "PDF page rendering exceeded its bound; its typed outcome was retained. No OCR was run",
        ),
    };
    terminal(job, state, failure, detail);
    Ok(Derivative::Pdf(Box::new(record)))
}
