//! Fixed debug-only canonical review specimens; no arbitrary worker-result acceptance interface.
use super::*;
impl Workspace {
    pub fn seed_pdf_processing_review(&mut self) -> Result<()> {
        let records: u64 = self
            .conn
            .query_row("SELECT count(*) FROM records", [], |row| row.get(0))?;
        require(
            self.revision()? == 0 && records == 0,
            "PDF processing fixtures require a fresh empty workspace",
        )?;
        for mode in [
            "recognized",
            "empty",
            "encrypted",
            "unsupported",
            "failed",
            "quota",
            "cancelled",
            "retry",
            "running",
            "cancel_requested",
            "queued",
        ] {
            let mut bytes = pdf_fixtures::PDF.to_vec();
            bytes.extend_from_slice(
                format!("\n% Fixed synthetic review specimen: {mode}\n").as_bytes(),
            );
            let source = self.import(&format!("pdf-review-{mode}.pdf"), &bytes)?;
            let job = self.queue_pdf_page_ocr(&source, &id(), 2, 144)?;
            if mode == "queued" {
                continue;
            }
            let prepared = self
                .claim_processing_job()?
                .ok_or_else(|| Error::Validation("Synthetic PDF claim unavailable".into()))?;
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
            let output = ProcessingOutput::Pdf(Box::new(pdf_fixtures::outcome(
                fixture_mode,
                &bytes,
                2,
                144,
            )));
            self.finish_processing_job(&prepared.ticket, Ok(&output))?;
            if mode == "retry" {
                self.retry_processing_job(
                    &job.id,
                    job.attempt,
                    "Fixed synthetic PDF retry; no worker ran",
                )?;
                let next = self
                    .claim_processing_job()?
                    .ok_or_else(|| Error::Validation("Synthetic PDF retry unavailable".into()))?;
                let output =
                    ProcessingOutput::Pdf(Box::new(pdf_fixtures::recognized(&bytes, 2, 144)));
                self.finish_processing_job(&next.ticket, Ok(&output))?;
            }
        }
        Ok(())
    }
}
