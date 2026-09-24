//! Retained image word-region results; filesystem content precedes atomic canonical references.
use super::derivative_files as files;
use super::*;
use crate::{
    engines::{image_regions, ocr_regions::RegionsStatus},
    processing::*,
};

pub(super) fn summary(output: &image_regions::ImageOcrRegions) -> ImageRegionResult {
    ImageRegionResult {
        schema_version: 1,
        decoder: output.image.result.clone(),
        recognition: output.ocr.as_ref().map(|value| value.result.clone()),
    }
}
pub(super) fn build(
    job: &mut ProcessingJob,
    original: &[u8],
    output: &image_regions::ImageOcrRegions,
    key: String,
) -> Result<(ImageRegionExtractionRecord, Vec<u8>)> {
    image_regions::validate_result(
        &output.image.result,
        original,
        output.image.raster.as_deref(),
        output.ocr.as_ref().map(|value| &value.result),
        output.ocr.as_ref().map(|value| value.tsv.as_slice()),
    )?;
    let ProcessingInput::ImageOcrRegions {
        evidence_id,
        sha256,
        bytes,
    } = &job.input
    else {
        return Err(Error::Validation(
            "Word-region output does not match its operation".into(),
        ));
    };
    let bytes_result = serde_json::to_vec(&summary(output))?;
    let record = ImageRegionExtractionRecord {
        schema_version: 1,
        id: key,
        job_id: job.id.clone(),
        attempt: job.attempt,
        input: ImageOcrRegionsInput::ImageOcrRegions {
            evidence_id: evidence_id.clone(),
            sha256: sha256.clone(),
            bytes: *bytes,
        },
        created_at: now(),
        result: files::reference(DerivativeKind::ImageRegionResultJsonV1, &bytes_result)?,
        raster: output
            .image
            .raster
            .as_ref()
            .map(|bytes| files::reference(DerivativeKind::CanonicalPgmV1, bytes))
            .transpose()?,
        tsv: output
            .ocr
            .as_ref()
            .map(|value| files::reference(DerivativeKind::OcrTsvV1, &value.tsv))
            .transpose()?,
    };
    let empty = output
        .ocr
        .as_ref()
        .is_some_and(|value| value.result.status == RegionsStatus::NoTextRecognized);
    processing::image_outcome(job, &output.image.result.status, empty, true);
    Ok((record, bytes_result))
}
pub(super) fn refs(record: &ImageRegionExtractionRecord) -> impl Iterator<Item = &DerivativeRef> {
    std::iter::once(&record.result)
        .chain(record.raster.as_ref())
        .chain(record.tsv.as_ref())
}
pub(super) fn retain(
    root: &Path,
    conn: &Connection,
    record: &ImageRegionExtractionRecord,
    json: &[u8],
    output: &image_regions::ImageOcrRegions,
) -> Result<()> {
    if let (Some(reference), Some(bytes)) = (&record.raster, &output.image.raster) {
        retain_checked(root, conn, reference, bytes)?;
    }
    if let (Some(reference), Some(ocr)) = (&record.tsv, &output.ocr) {
        retain_checked(root, conn, reference, &ocr.tsv)?;
    }
    retain_checked(root, conn, &record.result, json)
}
fn retain_checked(
    root: &Path,
    conn: &Connection,
    reference: &DerivativeRef,
    bytes: &[u8],
) -> Result<()> {
    let published: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM derivative_objects WHERE sha256=?)",
        [&reference.sha256],
        |row| row.get(0),
    )?;
    let outcome = if published {
        files::verify_catalog(conn, reference)
            .and_then(|()| files::read(root, reference).map(|_| ()))
    } else {
        files::retain(root, reference, bytes)
    };
    outcome.map_err(|error| match error {
        Error::Cleanup(_) => error,
        _ => Error::DerivativeUnavailable { published },
    })
}

impl Workspace {
    /// Returns a small record plus verified typed result; no binary payload or filesystem path.
    pub fn inspect_image_region_extraction(
        &self,
        extraction_id: &str,
    ) -> Result<ImageRegionInspection> {
        let (inspection, _) = self.verify_image_regions(extraction_id)?;
        Ok(inspection)
    }
    /// Future display adapters receive bounded inert raster bytes, never an arbitrary path/URL.
    pub fn read_image_region_raster(&self, extraction_id: &str) -> Result<Vec<u8>> {
        let (_, raster) = self.verify_image_regions(extraction_id)?;
        raster.ok_or_else(|| Error::Blocked("This result contains no retained raster".into()))
    }
    fn verify_image_regions(
        &self,
        extraction_id: &str,
    ) -> Result<(ImageRegionInspection, Option<Vec<u8>>)> {
        let record: ImageRegionExtractionRecord =
            get(&self.conn, "image_region_extraction", extraction_id)?;
        require(
            record.schema_version == 1
                && record.id == extraction_id
                && record.attempt > 0
                && record.id == hash(format!("{}:{}", record.job_id, record.attempt).as_bytes()),
            "Invalid image-region extraction identity",
        )?;
        require(
            record.result.kind == DerivativeKind::ImageRegionResultJsonV1
                && record
                    .raster
                    .as_ref()
                    .is_none_or(|r| r.kind == DerivativeKind::CanonicalPgmV1)
                && record
                    .tsv
                    .as_ref()
                    .is_none_or(|r| r.kind == DerivativeKind::OcrTsvV1),
            "Incorrect image-region artifact types",
        )?;
        let job = self.processing_job(&record.job_id)?;
        let ImageOcrRegionsInput::ImageOcrRegions {
            evidence_id,
            sha256,
            bytes,
        } = &record.input;
        let input = ProcessingInput::ImageOcrRegions {
            evidence_id: evidence_id.clone(),
            sha256: sha256.clone(),
            bytes: *bytes,
        };
        require(
            job.input == input
                && job.attempt >= record.attempt
                && job.result_ids.contains(&record.id),
            "Extraction is not owned by this processing attempt",
        )?;
        let original = self.verify_processing_input(&input)?;
        for reference in refs(&record) {
            files::validate_ref(reference)?;
            files::verify_catalog(&self.conn, reference)?;
        }
        let json = files::read(&self.root, &record.result)?;
        let result: ImageRegionResult = serde_json::from_slice(&json)?;
        require(
            result.schema_version == 1 && serde_json::to_vec(&result)? == json,
            "Invalid or noncanonical image-region JSON",
        )?;
        let raster = record
            .raster
            .as_ref()
            .map(|r| files::read(&self.root, r))
            .transpose()?;
        let tsv = record
            .tsv
            .as_ref()
            .map(|r| files::read(&self.root, r))
            .transpose()?;
        image_regions::validate_result(
            &result.decoder,
            &original,
            raster.as_deref(),
            result.recognition.as_ref(),
            tsv.as_deref(),
        )?;
        Ok((
            ImageRegionInspection {
                extraction: record,
                result,
            },
            raster,
        ))
    }
}
