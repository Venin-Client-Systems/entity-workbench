//! Fixed canonical UI specimens, explicitly not an OCR execution interface.
use super::*;

impl Workspace {
    /// Development-only fresh-workspace fixture. Accepts no arbitrary worker result.
    pub fn seed_image_processing_review(&mut self) -> Result<()> {
        let records: u64 = self
            .conn
            .query_row("SELECT count(*) FROM records", [], |row| row.get(0))?;
        require(
            self.revision()? == 0 && records == 0,
            "Image processing fixtures require a fresh empty workspace",
        )?;
        for mode in [
            "recognized",
            "empty",
            "unsupported",
            "failed",
            "quota",
            "cancelled",
            "retry",
            "running",
            "cancel_requested",
            "queued",
        ] {
            let bytes = if mode == "unsupported" {
                image_fixtures::GIF
            } else {
                image_fixtures::PNG
            };
            // One retained original can support multiple explicitly named processing specimens.
            let source = self.import(
                if mode == "unsupported" {
                    "synthetic-unsupported.gif"
                } else {
                    "synthetic-image.png"
                },
                bytes,
            )?;
            let job = self.queue_image_ocr(&source, &id())?;
            if mode == "queued" {
                continue;
            }
            let prepared = self
                .claim_processing_job()?
                .ok_or_else(|| Error::Validation("Synthetic image claim unavailable".into()))?;
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
            let fixture_mode = match mode {
                "cancelled" => "recognized",
                "retry" => "failed",
                _ => mode,
            };
            let output = ProcessingOutput::Image(Box::new(image_fixtures::outcome(fixture_mode)));
            self.finish_processing_job(&prepared.ticket, Ok(&output))?;
            if mode == "retry" {
                self.retry_processing_job(
                    &job.id,
                    job.attempt,
                    "Fixed synthetic retry specimen; no worker ran",
                )?;
                let next = self
                    .claim_processing_job()?
                    .ok_or_else(|| Error::Validation("Synthetic retry unavailable".into()))?;
                let output = ProcessingOutput::Image(Box::new(image_fixtures::recognized()));
                self.finish_processing_job(&next.ticket, Ok(&output))?;
            }
        }
        Ok(())
    }
}
