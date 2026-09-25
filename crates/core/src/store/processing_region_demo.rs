//! Fixed synthetic UI states. No worker runs and no supplied worker payload is accepted.
use super::*;
use crate::engines::{image_regions::ImageOcrRegions, ocr_regions::*};

fn outcome(mode: &str, original: &[u8]) -> ProcessingOutput {
    let mut image =
        image_fixtures::outcome(if matches!(mode, "unsupported" | "failed" | "quota") {
            mode
        } else {
            "recognized"
        })
        .image;
    image.result.original_sha256 = hash(original);
    image.result.original_bytes = original.len() as u64;
    let ocr = image.raster.as_ref().map(|raster| {
        let mut words: Vec<String> = if mode == "empty" { Vec::new() } else {
            "FIXED SYNTHETIC UI SPECIMEN — NO WORKER RAN REFERENCE 0042 AMOUNT 123.45 <script>window.regionExecuted=true</script>".split_whitespace().map(str::to_owned).collect()
        };
        if mode == "many" { words.extend((0..60).map(|i| format!("WORD{i:04}"))); }
        let mut regions = vec![OcrRegion { level: RegionLevel::Page, page_number: 1, block_number: 0, paragraph_number: 0, line_number: 0, word_number: 0, bounds: RasterBox { left: 0, top: 0, width: 1200, height: 230 }, engine_confidence: None, text: String::new() }];
        if !words.is_empty() {
            for (index, level) in [RegionLevel::Block, RegionLevel::Paragraph, RegionLevel::Line].into_iter().enumerate() {
                regions.push(OcrRegion { level, page_number: 1, block_number: 1, paragraph_number: u32::from(index >= 1), line_number: u32::from(index >= 2), word_number: 0, bounds: RasterBox { left: 0, top: 0, width: 1200, height: 230 }, engine_confidence: None, text: String::new() });
            }
            for (index, word) in words.iter().enumerate() {
                regions.push(OcrRegion { level: RegionLevel::Word, page_number: 1, block_number: 1, paragraph_number: 1, line_number: 1, word_number: index as u32 + 1, bounds: RasterBox { left: (index as u32 % 50) * 20, top: 20 + (index as u32 / 50) * 50, width: 18, height: 30 }, engine_confidence: Some(96.123456), text: word.clone() });
            }
        }
        let mut tsv = String::from("level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n");
        for region in &regions {
            let level = match region.level { RegionLevel::Page => 1, RegionLevel::Block => 2, RegionLevel::Paragraph => 3, RegionLevel::Line => 4, RegionLevel::Word => 5 };
            let b = &region.bounds;
            tsv.push_str(&format!("{level}\t1\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n", region.block_number, region.paragraph_number, region.line_number, region.word_number, b.left, b.top, b.width, b.height, if level == 5 { "96.123456" } else { "-1" }, region.text));
        }
        let tsv = tsv.into_bytes();
        OcrRegions { result: OcrRegionsResult { protocol_version: 1, job_id: id(), raster_sha256: hash(raster), raster_bytes: raster.len() as u64, width: 1200, height: 230, engine: "tesseract-5.5.2".into(), language: "eng".into(), model_sha256: "7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2".into(), runtime_manifest_sha256: "0".repeat(64), status: if words.is_empty() { RegionsStatus::NoTextRecognized } else { RegionsStatus::Recognized }, text: if words.is_empty() { "\n\x0c".into() } else { format!("{}\n", words.join(" ")) }, tsv_sha256: hash(&tsv), tsv_bytes: tsv.len() as u64, regions, limitations: vec![RegionsLimitation::UnreviewedRecognition, RegionsLimitation::UnreviewedWordRegions, RegionsLimitation::EngineConfidenceNotProbability, RegionsLimitation::NoOriginalDocumentMapping, RegionsLimitation::SingleUniformBlock] }, tsv }
    });
    ProcessingOutput::ImageRegions(Box::new(ImageOcrRegions { image, ocr }))
}
impl Workspace {
    pub fn seed_image_region_review(&mut self) -> Result<()> {
        let count: u64 = self
            .conn
            .query_row("SELECT count(*) FROM records", [], |row| row.get(0))?;
        require(
            self.revision()? == 0 && count == 0,
            "Image-region fixtures require a fresh empty workspace",
        )?;
        for mode in [
            "recognized",
            "many",
            "empty",
            "unsupported",
            "failed",
            "quota",
            "cancelled",
            "retry",
            "worker_failed",
            "running",
            "cancel_requested",
            "queued",
        ] {
            let mut original = if mode == "unsupported" {
                image_fixtures::GIF.to_vec()
            } else {
                image_fixtures::PNG.to_vec()
            };
            // Fixed trailing specimen marker distinguishes originals; these are state fixtures,
            // not a decoder fidelity test or evidence that this synthetic recognition ran.
            original.extend_from_slice(
                format!("\nFIXED SYNTHETIC UI SPECIMEN — NO WORKER RAN: {mode}\n").as_bytes(),
            );
            let source = self.import(
                &format!(
                    "region-review-{mode}.{}",
                    if mode == "unsupported" { "gif" } else { "png" }
                ),
                &original,
            )?;
            let job = self.queue_image_ocr_regions(&source, &id())?;
            if mode == "queued" {
                continue;
            }
            let prepared = self
                .claim_processing_job()?
                .ok_or_else(|| Error::Validation("Synthetic region claim unavailable".into()))?;
            require(
                prepared.ticket.job_id == job.id,
                "Synthetic claim selected another job",
            )?;
            if mode == "running" {
                continue;
            }
            if mode == "cancel_requested" {
                self.cancel_processing_job(&job.id, job.attempt)?;
                continue;
            }
            if mode == "cancelled" {
                self.cancel_processing_job(&job.id, job.attempt)?;
            }
            if mode == "worker_failed" {
                self.finish_processing_job(
                    &prepared.ticket,
                    Err(Error::Blocked(
                        "Fixed synthetic unavailable runtime; no worker ran".into(),
                    )),
                )?;
                continue;
            }
            let output = outcome(if mode == "retry" { "failed" } else { mode }, &original);
            self.finish_processing_job(&prepared.ticket, Ok(&output))?;
            if mode == "retry" {
                self.retry_processing_job(
                    &job.id,
                    job.attempt,
                    "Fixed synthetic retry specimen; no worker ran",
                )?;
                let next = self.claim_processing_job()?.ok_or_else(|| {
                    Error::Validation("Synthetic region retry unavailable".into())
                })?;
                self.finish_processing_job(&next.ticket, Ok(&outcome("recognized", &original)))?;
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_region_ui_states_verify_without_running_workers() {
        let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let mut workspace = Workspace::open(temp.path()).unwrap();
        workspace.seed_image_region_review().unwrap();
        assert!(workspace.seed_image_region_review().is_err());
        let jobs = workspace.processing_jobs().unwrap();
        assert_eq!(jobs.total, 12);
        let mut results = 0;
        for job in jobs.jobs {
            for key in job.result_ids {
                let value = workspace.inspect_image_region_extraction(&key).unwrap();
                if value.extraction.raster.is_some() {
                    assert!(!workspace.read_image_region_raster(&key).unwrap().is_empty());
                }
                results += 1;
            }
        }
        assert_eq!(results, 8);
        assert!(workspace.view().unwrap().observations.is_empty());
    }
}
