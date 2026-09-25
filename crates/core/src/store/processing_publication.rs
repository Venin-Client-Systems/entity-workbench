//! Validate bounded worker results and publish immutable canonical references.
use super::super::processing_regions as regions;
use super::*;
use crate::engines::{image, ocr};
#[path = "processing_pdf_publication.rs"]
mod pdf;

pub(super) enum Derivative {
    Document(Box<ExtractionRecord>),
    Image(Box<ImageExtractionRecord>),
    Pdf(Box<PdfExtractionRecord>),
    ImageRegions(Box<ImageRegionExtractionRecord>, Vec<u8>),
}

impl Derivative {
    pub(super) fn id(&self) -> &str {
        match self {
            Self::Document(record) => &record.id,
            Self::Image(record) => &record.id,
            Self::Pdf(record) => &record.id,
            Self::ImageRegions(record, _) => &record.id,
        }
    }
    pub(super) fn prepare_files(
        &self,
        root: &Path,
        conn: &Connection,
        output: &ProcessingOutput,
    ) -> Result<()> {
        if let Self::ImageRegions(record, json) = self {
            let ProcessingOutput::ImageRegions(output) = output else {
                return Err(Error::Validation(
                    "Image-region preparation operation mismatch".into(),
                ));
            };
            regions::retain(root, conn, record, json, output)?;
        }
        Ok(())
    }
    pub(super) fn publish(&self, conn: &Connection) -> Result<()> {
        match self {
            Self::Document(record) => put(conn, "extraction", &record.id, record),
            Self::Image(record) => put(conn, "image_extraction", &record.id, record),
            Self::Pdf(record) => put(conn, "pdf_extraction", &record.id, record),
            Self::ImageRegions(record, _) => {
                for reference in regions::refs(record) {
                    super::super::derivative_files::catalog(conn, reference)?;
                }
                put(conn, "image_region_extraction", &record.id, record)
            }
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
            (ProcessingInput::ImageOcrRegions { .. }, ProcessingOutput::ImageRegions(output)) => {
                let original = self.verify_processing_input(&job.input)?;
                crate::engines::image_regions::validate_result(
                    &output.image.result,
                    &original,
                    output.image.raster.as_deref(),
                    output.ocr.as_ref().map(|value| &value.result),
                    output.ocr.as_ref().map(|value| value.tsv.as_slice()),
                )?;
                let previous = self.inspect_image_region_extraction(key)?;
                Ok(previous.extraction.attempt == ticket.attempt
                    && previous.extraction.result.sha256
                        == hash(&serde_json::to_vec(&regions::summary(output))?))
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
    let (_, sha256, bytes) = job.input.source().ok_or_else(|| {
        Error::InvalidWorkerResult("Graph output requires its private capture".into())
    })?;
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
            let record = ExtractionRecord {
                schema_version: 2,
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
            extraction::validate_record(&record, &record.id)?;
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
            let empty = output
                .ocr
                .as_ref()
                .is_some_and(|value| value.status == ocr::OcrStatus::NoTextRecognized);
            image_outcome(job, &output.image.result.status, empty, false);
            Ok(Derivative::Image(Box::new(record)))
        }
        (ProcessingInput::ImageOcrRegions { .. }, ProcessingOutput::ImageRegions(output)) => {
            let (record, json) = regions::build(job, original, output, key)?;
            Ok(Derivative::ImageRegions(Box::new(record), json))
        }
        (ProcessingInput::PdfPageOcr { .. }, ProcessingOutput::Pdf(output)) => {
            pdf::validate(job, original, output, key)
        }
        _ => Err(Error::Validation(
            "Worker result does not match the queued operation".into(),
        )),
    }
}
