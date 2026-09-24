use super::*;
use crate::engines::{ocr::OcrStatus, pdf_render::*};
use pdf_fixtures::{outcome, recognized, PDF};
fn workspace() -> (tempfile::TempDir, Workspace, String) {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut workspace = Workspace::open(temp.path().join("case")).unwrap();
    let source = workspace.import("synthetic-scan.pdf", PDF).unwrap();
    (temp, workspace, source)
}
fn queue(workspace: &mut Workspace, source: &str) -> ProcessingJob {
    workspace.queue_pdf_page_ocr(source, &id(), 2, 144).unwrap()
}
fn output() -> ProcessingOutput {
    ProcessingOutput::Pdf(Box::new(recognized(PDF, 2, 144)))
}
#[test]
fn pdf_request_identity_binds_page_dpi_operation_and_preserves_legacy_versions() {
    let (_temp, mut workspace, source) = workspace();
    let request = id();
    let job = workspace
        .queue_pdf_page_ocr(&source, &request, 2, 144)
        .unwrap();
    assert_eq!(job.schema_version, 3);
    let revision = workspace.revision().unwrap();
    assert_eq!(
        workspace
            .queue_pdf_page_ocr(&source, &request, 2, 144)
            .unwrap()
            .id,
        job.id
    );
    assert_eq!(workspace.revision().unwrap(), revision);
    for (page, dpi) in [(1, 144), (2, 72)] {
        assert!(matches!(
            workspace.queue_pdf_page_ocr(&source, &request, page, dpi),
            Err(Error::Conflict(_))
        ));
    }
    assert!(matches!(
        workspace.queue_document_parse(&source, &request),
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        workspace.queue_image_ocr(&source, &request),
        Err(Error::Conflict(_))
    ));
    for (page, dpi) in [(0, 144), (1001, 144), (2, 71), (2, 301)] {
        assert!(workspace
            .queue_pdf_page_ocr(&source, &id(), page, dpi)
            .is_err());
    }
    assert!(workspace
        .queue_pdf_page_ocr(&source, "invalid", 2, 144)
        .is_err());
    assert_eq!(workspace.revision().unwrap(), revision);
    assert!(
        serde_json::from_value::<Command>(json!({"action":"finish_pdf_job","job_id":job.id}))
            .is_err()
    );
    for version in [1, 2, 4] {
        let mut invalid = job.clone();
        invalid.schema_version = version;
        assert!(supported_job(&invalid).is_err());
    }
    let mut parse = workspace.queue_document_parse(&source, &id()).unwrap();
    for version in [1, 2] {
        parse.schema_version = version;
        put(&workspace.conn, "processing_job", &parse.id, &parse).unwrap();
        assert_eq!(
            workspace.processing_job(&parse.id).unwrap().schema_version,
            version
        );
    }
    let mut image = workspace.queue_image_ocr(&source, &id()).unwrap();
    image.schema_version = 2;
    put(&workspace.conn, "processing_job", &image.id, &image).unwrap();
    assert_eq!(
        workspace.processing_job(&image.id).unwrap().schema_version,
        2
    );
    for (current, saved) in [
        (
            serde_json::to_value(schemars::schema_for!(ExtractionRecord)).unwrap(),
            include_str!("../../../../schemas/extraction.v1.schema.json"),
        ),
        (
            serde_json::to_value(schemars::schema_for!(ImageExtractionRecord)).unwrap(),
            include_str!("../../../../schemas/image-extraction.v1.schema.json"),
        ),
    ] {
        assert_eq!(current, serde_json::from_str::<Value>(saved).unwrap());
    }
}
#[test]
fn pdf_publication_is_atomic_replayable_and_keeps_original_text_and_raster_policy() {
    let (_temp, mut workspace, source) = workspace();
    let job = queue(&mut workspace, &source);
    let prepared = workspace.claim_processing_job().unwrap().unwrap();
    let output = output();
    workspace.conn.execute_batch("CREATE TRIGGER reject_pdf_finish BEFORE UPDATE ON records WHEN NEW.kind='processing_job' AND json_extract(NEW.body,'$.state')='completed' BEGIN SELECT RAISE(ABORT,'synthetic PDF publication failure'); END;").unwrap();
    let revision = workspace.revision().unwrap();
    assert!(workspace
        .finish_processing_job(&prepared.ticket, Ok(&output))
        .is_err());
    assert_eq!(workspace.revision().unwrap(), revision);
    assert!(
        all::<PdfExtractionRecord>(&workspace.conn, "pdf_extraction")
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        workspace.processing_job(&job.id).unwrap().state,
        ProcessingState::Running
    );
    workspace
        .conn
        .execute_batch("DROP TRIGGER reject_pdf_finish;")
        .unwrap();
    let finished = workspace
        .finish_processing_job(&prepared.ticket, Ok(&output))
        .unwrap();
    let revision = workspace.revision().unwrap();
    assert_eq!(finished.state, ProcessingState::Completed);
    assert_eq!(
        workspace
            .finish_processing_job(&prepared.ticket, Ok(&output))
            .unwrap()
            .result_ids,
        finished.result_ids
    );
    assert_eq!(workspace.revision().unwrap(), revision);
    let record = workspace.pdf_extraction(&finished.result_ids[0]).unwrap();
    assert_eq!(
        record.result_sha256,
        hash(&serde_json::to_vec(&record.result).unwrap())
    );
    assert!(!record.result.raster_retained);
    assert_eq!(record.result.render.page_number, 2);
    assert_eq!(record.result.render.dpi, 144);
    assert_eq!(
        record.result.recognition.as_ref().unwrap().raster_sha256,
        record.result.render.raster.as_ref().unwrap().sha256
    );
    let inspected = workspace
        .dispatch(Command::InspectPdfExtraction {
            extraction_id: record.id.clone(),
        })
        .unwrap();
    assert_eq!(inspected, serde_json::to_value(record).unwrap());
    let view = workspace.view().unwrap();
    assert!(view
        .evidence
        .iter()
        .find(|e| e.id == source)
        .unwrap()
        .text
        .is_none());
    assert!(view.observations.is_empty());
    assert_eq!(
        fs::read(workspace.root.join("originals").join(hash(PDF))).unwrap(),
        PDF
    );
    assert_eq!(
        all::<PdfExtractionRecord>(&workspace.conn, "pdf_extraction")
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn pdf_acceptance_rejects_page_dpi_raster_recognition_and_operation_tampering() {
    for mutate in [
        |p: &mut PdfOcr| p.render.result.original_sha256 = "0".repeat(64),
        |p: &mut PdfOcr| p.render.result.page_number = 1,
        |p: &mut PdfOcr| p.render.result.dpi = 72,
        |p: &mut PdfOcr| p.render.result.geometry.as_mut().unwrap().rotation_degrees = 90,
        |p: &mut PdfOcr| p.render.raster.as_mut().unwrap().pop().map(|_| ()).unwrap(),
        |p: &mut PdfOcr| p.render.result.raster.as_mut().unwrap().sha256 = "0".repeat(64),
        |p: &mut PdfOcr| p.ocr.as_mut().unwrap().raster_sha256 = "0".repeat(64),
        |p: &mut PdfOcr| p.ocr.as_mut().unwrap().job_id = p.render.result.job_id.clone(),
        |p: &mut PdfOcr| p.ocr.as_mut().unwrap().status = OcrStatus::NoTextRecognized,
        |p: &mut PdfOcr| p.ocr = None,
        |p: &mut PdfOcr| {
            p.render.result.status = RenderStatus::Unsupported;
            p.render.result.failure = Some(RenderFailure::UnsupportedFeature);
        },
    ] {
        let (_temp, mut workspace, source) = workspace();
        let job = queue(&mut workspace, &source);
        let prepared = workspace.claim_processing_job().unwrap().unwrap();
        let mut value = recognized(PDF, 2, 144);
        mutate(&mut value);
        let output = ProcessingOutput::Pdf(Box::new(value));
        let finished = workspace
            .finish_processing_job(&prepared.ticket, Ok(&output))
            .unwrap();
        assert_eq!(finished.failure, Some(ProcessingFailure::InvalidResult));
        assert!(finished.result_ids.is_empty());
        assert_eq!(
            workspace.processing_job(&job.id).unwrap().state,
            ProcessingState::Failed
        );
        assert!(
            all::<PdfExtractionRecord>(&workspace.conn, "pdf_extraction")
                .unwrap()
                .is_empty()
        );
    }
    let (_temp, mut workspace, source) = workspace();
    queue(&mut workspace, &source);
    let prepared = workspace.claim_processing_job().unwrap().unwrap();
    let wrong = ProcessingOutput::Image(Box::new(image_fixtures::recognized()));
    assert_eq!(
        workspace
            .finish_processing_job(&prepared.ticket, Ok(&wrong))
            .unwrap()
            .failure,
        Some(ProcessingFailure::InvalidResult)
    );
}
#[test]
fn pdf_typed_outcomes_and_prior_attempts_survive_backup_without_raster_bytes() {
    let (temp, mut workspace, source) = workspace();
    let mut snapshots = Vec::new();
    for (mode, state, failure) in [
        ("recognized", ProcessingState::Completed, None),
        ("empty", ProcessingState::Completed, None),
        (
            "encrypted",
            ProcessingState::Blocked,
            Some(ProcessingFailure::EncryptedDocument),
        ),
        (
            "unsupported",
            ProcessingState::Blocked,
            Some(ProcessingFailure::UnsupportedFormat),
        ),
        (
            "failed",
            ProcessingState::Failed,
            Some(ProcessingFailure::PdfRenderFailed),
        ),
        (
            "quota",
            ProcessingState::QuotaExhausted,
            Some(ProcessingFailure::WorkerFailed),
        ),
    ] {
        let job = queue(&mut workspace, &source);
        let prepared = workspace.claim_processing_job().unwrap().unwrap();
        let value = ProcessingOutput::Pdf(Box::new(outcome(mode, PDF, 2, 144)));
        let finished = workspace
            .finish_processing_job(&prepared.ticket, Ok(&value))
            .unwrap();
        assert_eq!(finished.state, state);
        assert_eq!(finished.failure, failure);
        assert_eq!(finished.result_ids.len(), 1);
        let record = workspace.pdf_extraction(&finished.result_ids[0]).unwrap();
        if mode == "empty" {
            assert_eq!(record.result.recognition.as_ref().unwrap().text, "\n\x0c");
            assert_eq!(
                record.result.recognition.as_ref().unwrap().status,
                OcrStatus::NoTextRecognized
            );
        }
        snapshots.push(record.clone());
        if mode == "failed" {
            workspace
                .retry_processing_job(&job.id, 1, "Inspect another bounded attempt")
                .unwrap();
            let next = workspace.claim_processing_job().unwrap().unwrap();
            assert!(workspace
                .finish_processing_job(&prepared.ticket, Ok(&value))
                .is_err());
            let second = workspace
                .finish_processing_job(&next.ticket, Ok(&output()))
                .unwrap();
            assert_eq!(second.attempt, 2);
            assert_eq!(second.result_ids.len(), 2);
            assert_eq!(
                serde_json::to_value(workspace.pdf_extraction(&record.id).unwrap()).unwrap(),
                serde_json::to_value(&record).unwrap()
            );
            snapshots.push(workspace.pdf_extraction(&second.result_ids[1]).unwrap());
        }
    }
    let backup = workspace.backup().unwrap();
    let restored = Workspace::restore(&backup, &temp.path().join("restored")).unwrap();
    for record in snapshots {
        let after = restored.pdf_extraction(&record.id).unwrap();
        assert_eq!(
            serde_json::to_value(&after).unwrap(),
            serde_json::to_value(&record).unwrap()
        );
        assert!(!after.result.raster_retained);
        assert!(restored
            .processing_job(&record.job_id)
            .unwrap()
            .result_ids
            .contains(&record.id));
    }
    assert_eq!(
        fs::read(restored.root.join("originals").join(hash(PDF))).unwrap(),
        PDF
    );
}
#[test]
fn pdf_cancellation_stale_leases_and_unverified_exit_preserve_shared_precedence() {
    for failure in [None, Some("cleanup"), Some("exit")] {
        let (_temp, mut workspace, source) = workspace();
        let job = queue(&mut workspace, &source);
        let prepared = workspace.claim_processing_job().unwrap().unwrap();
        let queued = queue(&mut workspace, &source);
        workspace.cancel_processing_job(&job.id, 1).unwrap();
        let output = output();
        let result = match failure {
            Some("cleanup") => Err(Error::Cleanup("synthetic cleanup".into())),
            Some("exit") => Err(Error::TerminationUnverified(
                "synthetic unknown exit".into(),
            )),
            _ => Ok(&output),
        };
        let finished = workspace
            .finish_processing_job(&prepared.ticket, result)
            .unwrap();
        assert!(finished.result_ids.is_empty());
        if failure == Some("exit") {
            assert_eq!(finished.state, ProcessingState::Failed);
            assert_eq!(
                finished.failure,
                Some(ProcessingFailure::WorkerExitUnverified)
            );
            assert_eq!(
                workspace.processing_job(&queued.id).unwrap().failure,
                Some(ProcessingFailure::RecoveryRequired)
            );
            assert!(workspace
                .queue_pdf_page_ocr(&source, &id(), 2, 144)
                .is_err());
            assert!(workspace
                .retry_processing_job(&job.id, 1, "Unsafe retry")
                .is_err());
        } else {
            assert_eq!(finished.state, ProcessingState::Cancelled);
            assert_eq!(
                finished.failure,
                Some(if failure.is_some() {
                    ProcessingFailure::CleanupFailed
                } else {
                    ProcessingFailure::CancelledByAnalyst
                })
            );
            workspace.cancel_processing_job(&queued.id, 1).unwrap();
            workspace
                .retry_processing_job(&job.id, 1, "Explicit retry")
                .unwrap();
            let next = workspace.claim_processing_job().unwrap().unwrap();
            assert_eq!(next.ticket.attempt, 2);
            assert!(workspace.cancel_processing_job(&job.id, 1).is_err());
            assert!(workspace
                .finish_processing_job(&prepared.ticket, Ok(&output))
                .is_err());
            let mut forged = next.ticket.clone();
            forged.lease = id();
            assert!(workspace
                .finish_processing_job(&forged, Ok(&output))
                .is_err());
            assert_eq!(
                workspace
                    .finish_processing_job(&next.ticket, Ok(&output))
                    .unwrap()
                    .state,
                ProcessingState::Completed
            );
        }
    }
}
#[test]
fn pdf_changed_original_never_publishes_and_fixed_review_seed_never_overwrites() {
    let (_temp, mut workspace, source) = workspace();
    queue(&mut workspace, &source);
    let prepared = workspace.claim_processing_job().unwrap().unwrap();
    let original = workspace.root.join("originals").join(hash(PDF));
    private_file(&original, 0o600).unwrap();
    fs::write(&original, b"%PDF-synthetic changed").unwrap();
    assert_eq!(
        workspace
            .finish_processing_job(&prepared.ticket, Ok(&output()))
            .unwrap()
            .failure,
        Some(ProcessingFailure::InputUnavailable)
    );
    assert!(
        all::<PdfExtractionRecord>(&workspace.conn, "pdf_extraction")
            .unwrap()
            .is_empty()
    );
    #[cfg(debug_assertions)]
    {
        let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let mut workspace = Workspace::open(temp.path()).unwrap();
        workspace.seed_pdf_processing_review().unwrap();
        let jobs = workspace.processing_jobs().unwrap();
        assert_eq!(jobs.total, 11);
        assert_eq!(workspace.view().unwrap().evidence.len(), 11);
        let records = all::<PdfExtractionRecord>(&workspace.conn, "pdf_extraction").unwrap();
        assert_eq!(records.len(), 8);
        assert!(records.iter().all(|r| r.result.render.page_number == 2
            && r.result.render.dpi == 144
            && !r.result.raster_retained));
        assert!(records.iter().any(|r| r
            .result
            .recognition
            .as_ref()
            .is_some_and(|o| o.text == "\n\x0c")));
        let revision = workspace.revision().unwrap();
        assert!(workspace.seed_pdf_processing_review().is_err());
        assert_eq!(workspace.revision().unwrap(), revision);
    }
}
