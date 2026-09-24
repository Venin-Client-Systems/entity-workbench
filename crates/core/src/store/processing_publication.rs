//! Validate and publish bounded immutable results without retaining raster bytes.
use super::*;
use crate::engines::{image, ocr};
#[path = "processing_pdf_publication.rs"]
mod pdf;

pub(super) enum Derivative {
    Document(Box<ExtractionRecord>),
    Image(Box<ImageExtractionRecord>),
    Pdf(Box<PdfExtractionRecord>),
}

impl Derivative {
    pub(super) fn id(&self) -> &str {
        match self {
            Self::Document(record) => &record.id,
            Self::Image(record) => &record.id,
            Self::Pdf(record) => &record.id,
        }
    }
    pub(super) fn publish(&self, conn: &Connection) -> Result<()> {
        match self {
            Self::Document(record) => put(conn, "extraction", &record.id, record),
            Self::Image(record) => put(conn, "image_extraction", &record.id, record),
            Self::Pdf(record) => put(conn, "pdf_extraction", &record.id, record),
        }
    }
}

fn image_summary(output: &image::ImageOcr) -> ImageExtractionResult {
    ImageExtractionResult {
        decoder: output.image.result.clone(),
        recognition: output.ocr.clone(),
        raster_retained: false,
    }
}

impl Workspace {
    pub fn image_extraction(&self, extraction_id: &str) -> Result<ImageExtractionRecord> {
        get(&self.conn, "image_extraction", extraction_id)
    }

    pub(super) fn is_processing_replay(
        &self,
        job: &ProcessingJob,
        ticket: &JobTicket,
        output: &ProcessingOutput,
    ) -> Result<bool> {
        let Some(key) = job.result_ids.last() else {
            return Ok(false);
        };
        match (&job.input, output) {
            (ProcessingInput::ParseDocument { .. }, ProcessingOutput::Document(result)) => {
                let previous = self.extraction(key)?;
                Ok(previous.attempt == ticket.attempt
                    && previous.result_sha256 == hash(&serde_json::to_vec(result)?))
            }
            (ProcessingInput::ImageOcr { .. }, ProcessingOutput::Image(result)) => {
                let previous = self.image_extraction(key)?;
                Ok(previous.attempt == ticket.attempt
                    && previous.result_sha256 == hash(&serde_json::to_vec(&image_summary(result))?))
            }
            (ProcessingInput::PdfPageOcr { .. }, ProcessingOutput::Pdf(result)) => {
                let previous = self.pdf_extraction(key)?;
                Ok(previous.attempt == ticket.attempt
                    && previous.result_sha256 == hash(&serde_json::to_vec(&pdf::summary(result))?))
            }
            _ => Ok(false),
        }
    }
}

pub(super) fn validated_derivative(
    job: &mut ProcessingJob,
    original: &[u8],
    output: &ProcessingOutput,
) -> Result<Derivative> {
    let (_, sha256, bytes) = job.input.source();
    require(
        hash(original) == sha256 && original.len() as u64 == bytes,
        "Processing source binding changed",
    )?;
    let key = hash(format!("{}:{}", job.id, job.attempt).as_bytes());
    match (&job.input, output) {
        (
            ProcessingInput::ParseDocument {
                evidence_id,
                sha256,
                bytes,
            },
            ProcessingOutput::Document(result),
        ) => {
            validate_result(result, sha256, *bytes)?;
            let record = ExtractionRecord {
                schema_version: 1,
                id: key,
                job_id: job.id.clone(),
                attempt: job.attempt,
                input: ParseDocumentInput::ParseDocument {
                    evidence_id: evidence_id.clone(),
                    sha256: sha256.clone(),
                    bytes: *bytes,
                },
                created_at: now(),
                result_sha256: hash(&serde_json::to_vec(result)?),
                result: result.clone(),
            };
            match result.status {
                ParseStatus::Complete => terminal(
                    job,
                    ProcessingState::Completed,
                    None,
                    "Parsing completed; extraction awaits analyst review",
                ),
                ParseStatus::Partial => terminal(
                    job,
                    ProcessingState::Partial,
                    None,
                    "Partial extraction retained with explicit limitations; review is required",
                ),
                ParseStatus::Unsupported => terminal(
                    job,
                    ProcessingState::Blocked,
                    Some(ProcessingFailure::UnsupportedFormat),
                    "The packaged parser does not support this document",
                ),
                ParseStatus::Failed => terminal(
                    job,
                    ProcessingState::Failed,
                    Some(ProcessingFailure::DocumentFailed),
                    "Document parsing failed; inspect the typed failure in its extraction record",
                ),
            }
            Ok(Derivative::Document(Box::new(record)))
        }
        (
            ProcessingInput::ImageOcr {
                evidence_id,
                sha256,
                bytes,
            },
            ProcessingOutput::Image(output),
        ) => {
            image::validate_result(
                &output.image.result,
                original,
                output.image.raster.as_deref(),
            )?;
            match (
                output.image.result.status.clone(),
                output.image.raster.as_deref(),
                &output.ocr,
            ) {
                (image::DecodeStatus::Decoded, Some(raster), Some(recognition)) => {
                    ocr::validate_result(recognition, raster)?;
                    require(
                        recognition.job_id != output.image.result.job_id,
                        "Decoder and OCR must be separate worker jobs",
                    )?;
                }
                (image::DecodeStatus::Decoded, _, _) => {
                    return Err(Error::Validation(
                        "Decoded image has no validated OCR result".into(),
                    ))
                }
                (_, None, None) => {}
                _ => {
                    return Err(Error::Validation(
                        "Rejected image cannot publish recognition".into(),
                    ))
                }
            }
            let result = image_summary(output);
            let record = ImageExtractionRecord {
                schema_version: 1,
                id: key,
                job_id: job.id.clone(),
                attempt: job.attempt,
                input: ImageOcrInput::ImageOcr {
                    evidence_id: evidence_id.clone(),
                    sha256: sha256.clone(),
                    bytes: *bytes,
                },
                created_at: now(),
                result_sha256: hash(&serde_json::to_vec(&result)?),
                result,
            };
            match output.image.result.status {
                image::DecodeStatus::Decoded => {
                    let empty = output.ocr.as_ref().is_some_and(|value| value.status == ocr::OcrStatus::NoTextRecognized);
                    terminal(job, ProcessingState::Completed, None, if empty { "Image decoded and OCR completed without recognized text; no facts were accepted. Raster was not retained" } else { "Image OCR completed; unreviewed recognition and provenance retained. Raster was not retained" });
                }
                image::DecodeStatus::Unsupported => terminal(job, ProcessingState::Blocked, Some(ProcessingFailure::UnsupportedFormat), "The image decoder does not support this input; no OCR was run"),
                image::DecodeStatus::Failed => terminal(job, ProcessingState::Failed, Some(ProcessingFailure::ImageDecodeFailed), "Image decoding failed; its typed outcome was retained and no OCR was run"),
                image::DecodeStatus::QuotaExhausted => terminal(job, ProcessingState::QuotaExhausted, Some(ProcessingFailure::WorkerFailed), "Image decoding exceeded its limit; its typed outcome was retained and no OCR was run"),
            }
            Ok(Derivative::Image(Box::new(record)))
        }
        (ProcessingInput::PdfPageOcr { .. }, ProcessingOutput::Pdf(output)) => {
            pdf::validate(job, original, output, key)
        }
        _ => Err(Error::Validation(
            "Worker result does not match the queued operation".into(),
        )),
    }
}
