//! Fixed canonical UI fixtures, never a worker execution or public publication API.
use super::*;
use crate::engines::parser::{ParseFailure, ParseLimitation};
use std::collections::BTreeMap;

impl Workspace {
    /// Separate fatal-state specimen: never starts an OS worker.
    pub fn seed_processing_recovery_review(&mut self) -> Result<()> {
        let records: u64 = self
            .conn
            .query_row("SELECT count(*) FROM records", [], |r| r.get(0))?;
        require(
            self.revision()? == 0 && records == 0,
            "Processing recovery fixtures require a fresh empty workspace",
        )?;
        let source = self.import(
            "synthetic-unverified-exit.txt",
            b"Fixed synthetic unverified worker-exit specimen. No worker ran.",
        )?;
        let job = self.queue_document_parse(&source, &id())?;
        let prepared = self
            .claim_processing_job()?
            .ok_or_else(|| Error::Validation("Synthetic claim unavailable".into()))?;
        require(
            prepared.ticket.job_id == job.id,
            "Synthetic claim selected another job",
        )?;
        let pending = self.import(
            "synthetic-recovery-required.txt",
            b"Fixed synthetic pending work suspended by unverified exit.",
        )?;
        self.queue_document_parse(&pending, &id())?;
        self.cancel_processing_job(&job.id, job.attempt)?;
        self.finish_document_job(
            &prepared.ticket,
            Err(Error::TerminationUnverified(
                "Fixed synthetic unverified exit; no OS process was launched".into(),
            )),
        )?;
        Ok(())
    }

    /// Development CLI only. Refuses every nonempty workspace and accepts no result payload.
    pub fn seed_processing_review(&mut self) -> Result<()> {
        let records: u64 = self
            .conn
            .query_row("SELECT count(*) FROM records", [], |r| r.get(0))?;
        require(
            self.revision()? == 0 && records == 0,
            "Processing review fixtures require a fresh empty workspace",
        )?;
        for (name, mode) in [
            ("synthetic-complete.txt", "complete"),
            ("synthetic-partial.pdf", "partial"),
            ("synthetic-unsupported.bin", "unsupported"),
            ("synthetic-malformed.pdf", "malformed"),
            ("synthetic-cleanup.txt", "cleanup"),
            ("synthetic-blocked.txt", "blocked"),
            ("synthetic-limits.txt", "limits"),
            ("synthetic-interrupted.txt", "interrupted"),
            ("synthetic-running.txt", "running"),
            ("synthetic-cancel-requested.txt", "cancel"),
            ("synthetic-queued.txt", "queued"),
        ] {
            let bytes = format!("Fixed synthetic document: {name}\n<script>window.extractionExecuted = true</script>\n<img src=\"https://unrequested.example/image\">\n").into_bytes();
            let evidence = self.import(name, &bytes)?;
            let job = self.queue_document_parse(&evidence, &id())?;
            if mode == "queued" {
                continue;
            }
            let prepared = self
                .claim_processing_job()?
                .ok_or_else(|| Error::Validation("Synthetic claim unavailable".into()))?;
            require(
                prepared.ticket.job_id == job.id,
                "Synthetic claim selected another job",
            )?;
            if mode == "running" {
                continue;
            }
            if mode == "cancel" {
                self.cancel_processing_job(&job.id, job.attempt)?;
                continue;
            }
            let mut result = ParseResult {
                protocol_version: 1,
                job_id: id(),
                content_sha256: hash(&bytes),
                source_bytes: bytes.len() as u64,
                parser: "utf8-v1".into(),
                media_type: "text/plain".into(),
                status: ParseStatus::Complete,
                text: String::from_utf8(bytes).unwrap(),
                metadata: BTreeMap::new(),
                limitations: vec![ParseLimitation::NoSourceAnchors],
                error: None,
            };
            let outcome = match mode {
                "partial" => {
                    result.parser = "pdfbox-3.0.8".into();
                    result.media_type = "application/pdf".into();
                    result.status = ParseStatus::Partial;
                    result.limitations.extend([
                        ParseLimitation::EmbeddedDocumentsExcluded,
                        ParseLimitation::OcrNotPerformed,
                    ]);
                    result
                        .metadata
                        .insert("Synthetic page count".into(), vec!["2".into()]);
                    result.metadata.insert(
                        "Synthetic markup".into(),
                        vec!["<svg onload=\"window.metadataExecuted=true\">".into()],
                    );
                    Ok(result)
                }
                "unsupported" => {
                    result.parser = "unsupported-v1".into();
                    result.media_type = "application/octet-stream".into();
                    result.status = ParseStatus::Unsupported;
                    result.text.clear();
                    Ok(result)
                }
                "malformed" => {
                    result.parser = "pdfbox-3.0.8".into();
                    result.media_type = "application/pdf".into();
                    result.status = ParseStatus::Failed;
                    result.text.clear();
                    result.error = Some(ParseFailure::MalformedDocument);
                    Ok(result)
                }
                "cleanup" => Err(Error::Cleanup("Fixed synthetic cleanup failure".into())),
                "blocked" => Err(Error::Blocked("Fixed synthetic unavailable runtime".into())),
                "limits" => Err(Error::QuotaExhausted("Fixed synthetic worker limit".into())),
                "interrupted" => Err(Error::Interrupted(
                    "Fixed synthetic coordinator interruption".into(),
                )),
                _ => Ok(result),
            };
            self.finish_document_job(&prepared.ticket, outcome)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_recovery_fixture_suspends_canonical_execution_without_overwriting() {
        let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let mut workspace = Workspace::open(temp.path()).unwrap();
        workspace.seed_processing_recovery_review().unwrap();
        let jobs = workspace.processing_jobs().unwrap().jobs;
        assert_eq!(jobs.len(), 2);
        assert!(jobs.iter().any(|job| job.failure
            == Some(ProcessingFailure::WorkerExitUnverified)
            && job.state == ProcessingState::Failed));
        assert!(jobs.iter().any(
            |job| job.failure == Some(ProcessingFailure::RecoveryRequired)
                && job.state == ProcessingState::Blocked
        ));
        assert!(jobs.iter().all(|job| job.result_ids.is_empty()));
        let revision = workspace.revision().unwrap();
        assert!(workspace.seed_processing_recovery_review().is_err());
        assert_eq!(revision, workspace.revision().unwrap());
    }
    #[test]
    fn fixed_processing_fixtures_are_canonical_and_do_not_overwrite_workspaces() {
        let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let mut workspace = Workspace::open(temp.path()).unwrap();
        workspace.seed_processing_review().unwrap();
        let page = workspace.processing_jobs().unwrap();
        assert_eq!(page.total, 11);
        assert_eq!(
            page.jobs
                .iter()
                .filter(|j| j.state == ProcessingState::Running)
                .count(),
            2
        );
        assert_eq!(page.jobs.iter().flat_map(|j| &j.result_ids).count(), 4);
        for job in page.jobs {
            for key in job.result_ids {
                let extraction = workspace.extraction(&key).unwrap();
                let ParseDocumentInput::ParseDocument { sha256, bytes, .. } = &extraction.input;
                validate_result(&extraction.result, sha256, *bytes).unwrap();
            }
        }
        let revision = workspace.revision().unwrap();
        assert!(workspace.seed_processing_review().is_err());
        assert_eq!(workspace.revision().unwrap(), revision);
        assert!(workspace.view().unwrap().observations.is_empty());
    }
}
