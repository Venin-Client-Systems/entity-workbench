use super::*;
use image_fixtures::{outcome, recognized, GIF, PNG};

fn workspace() -> (tempfile::TempDir, Workspace, String) {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut workspace = Workspace::open(temp.path().join("case")).unwrap();
    let evidence = workspace.import("synthetic.png", PNG).unwrap();
    (temp, workspace, evidence)
}

fn queue(workspace: &mut Workspace, evidence: &str) -> ProcessingJob {
    workspace.queue_image_ocr(evidence, &id()).unwrap()
}

#[test]
fn image_request_keys_bind_operation_and_preserve_parse_v1_schema() {
    let (_temp, mut workspace, evidence) = workspace();
    let key = id();
    let image = workspace.queue_image_ocr(&evidence, &key).unwrap();
    assert_eq!(image.schema_version, 2);
    let before = workspace.revision().unwrap();
    assert_eq!(
        workspace.queue_image_ocr(&evidence, &key).unwrap().id,
        image.id
    );
    assert_eq!(workspace.revision().unwrap(), before);
    assert!(matches!(
        workspace.queue_document_parse(&evidence, &key),
        Err(Error::Conflict(_))
    ));
    assert!(workspace.dispatch(serde_json::from_value(serde_json::json!({"action":"queue_image_ocr","evidence_id":evidence,"request_key":"invalid"})).unwrap()).is_err());
    assert!(serde_json::from_value::<Command>(
        serde_json::json!({"action":"finish_image_job","job_id":image.id})
    )
    .is_err());
    let current = serde_json::to_value(schemars::schema_for!(ExtractionRecord)).unwrap();
    let saved: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../schemas/extraction.v1.schema.json"
    ))
    .unwrap();
    assert_eq!(
        current, saved,
        "Parse extraction v1 remains byte-for-byte schema-compatible"
    );
    let mut parse = workspace.queue_document_parse(&evidence, &id()).unwrap();
    parse.schema_version = 1;
    put(&workspace.conn, "processing_job", &parse.id, &parse).unwrap();
    assert_eq!(
        workspace.processing_job(&parse.id).unwrap().schema_version,
        1
    );
    assert!(serde_json::from_value::<ParseDocumentInput>(serde_json::json!({"operation":"image_ocr","evidence_id":evidence,"sha256":hash(PNG),"bytes":PNG.len()})).is_err());
}

#[test]
fn image_publication_is_atomic_replayable_and_keeps_original_text_unmodified() {
    let (_temp, mut workspace, evidence) = workspace();
    let job = queue(&mut workspace, &evidence);
    let prepared = workspace.claim_processing_job().unwrap().unwrap();
    let output = ProcessingOutput::Image(Box::new(recognized()));
    workspace.conn.execute_batch("CREATE TRIGGER reject_image_finish BEFORE UPDATE ON records WHEN NEW.kind='processing_job' AND json_extract(NEW.body,'$.state')='completed' BEGIN SELECT RAISE(ABORT,'synthetic image publication failure'); END;").unwrap();
    let revision = workspace.revision().unwrap();
    assert!(workspace
        .finish_processing_job(&prepared.ticket, Ok(&output))
        .is_err());
    assert_eq!(workspace.revision().unwrap(), revision);
    assert!(
        all::<ImageExtractionRecord>(&workspace.conn, "image_extraction")
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        workspace.processing_job(&job.id).unwrap().state,
        ProcessingState::Running
    );
    workspace
        .conn
        .execute_batch("DROP TRIGGER reject_image_finish;")
        .unwrap();
    let finished = workspace
        .finish_processing_job(&prepared.ticket, Ok(&output))
        .unwrap();
    let revision = workspace.revision().unwrap();
    assert_eq!(
        workspace
            .finish_processing_job(&prepared.ticket, Ok(&output))
            .unwrap()
            .result_ids,
        finished.result_ids
    );
    assert_eq!(workspace.revision().unwrap(), revision);
    let record = workspace.image_extraction(&finished.result_ids[0]).unwrap();
    assert!(!record.result.raster_retained);
    assert_eq!(record.result.decoder.original_sha256, hash(PNG));
    assert_eq!(
        record.result.recognition.as_ref().unwrap().raster_sha256,
        record.result.decoder.raster.as_ref().unwrap().sha256
    );
    assert_eq!(
        hash(&serde_json::to_vec(&record.result).unwrap()),
        record.result_sha256
    );
    let encoded = serde_json::to_value(record).unwrap();
    assert!(encoded
        .pointer("/result/decoder/raster")
        .unwrap()
        .is_object());
    assert!(serde_json::to_vec(&encoded).unwrap().len() < 10_000);
    let source: Evidence = get(&workspace.conn, "evidence", &evidence).unwrap();
    assert!(source.text.is_none());
    workspace.verify_original(&source).unwrap();
    assert!(workspace.view().unwrap().observations.is_empty());
    let mut different = recognized();
    different.ocr.as_mut().unwrap().text = "Different replay".into();
    assert!(workspace
        .finish_processing_job(
            &prepared.ticket,
            Ok(&ProcessingOutput::Image(Box::new(different)))
        )
        .is_err());
}

#[test]
fn canonical_image_acceptance_rejects_wrong_raster_recognition_and_operation() {
    let (_temp, mut workspace, evidence) = workspace();
    for mutation in 0..8 {
        let job = queue(&mut workspace, &evidence);
        let prepared = workspace.claim_processing_job().unwrap().unwrap();
        let mut image = recognized();
        match mutation {
            0 => image.image.result.original_sha256 = "0".repeat(64),
            1 => *image.image.raster.as_mut().unwrap().last_mut().unwrap() ^= 1,
            2 => image.ocr.as_mut().unwrap().raster_sha256 = "0".repeat(64),
            3 => image.ocr = None,
            4 => image.ocr.as_mut().unwrap().job_id = image.image.result.job_id.clone(),
            5 => image.ocr.as_mut().unwrap().text = "\0".into(),
            6 => {
                image = outcome("failed");
                image.ocr = recognized().ocr;
            }
            _ => {
                image
                    .image
                    .result
                    .raster
                    .as_mut()
                    .unwrap()
                    .source_image_index = 1
            }
        }
        let failed = workspace
            .finish_processing_job(
                &prepared.ticket,
                Ok(&ProcessingOutput::Image(Box::new(image))),
            )
            .unwrap();
        assert_eq!(
            failed.failure,
            Some(ProcessingFailure::InvalidResult),
            "mutation {mutation}"
        );
        assert!(workspace
            .processing_job(&job.id)
            .unwrap()
            .result_ids
            .is_empty());
    }
    let source = workspace
        .import("synthetic.txt", b"Synthetic parser input")
        .unwrap();
    workspace.queue_document_parse(&source, &id()).unwrap();
    let prepared = workspace.claim_processing_job().unwrap().unwrap();
    assert_eq!(
        workspace
            .finish_processing_job(
                &prepared.ticket,
                Ok(&ProcessingOutput::Image(Box::new(recognized())))
            )
            .unwrap()
            .failure,
        Some(ProcessingFailure::InvalidResult)
    );
    assert!(
        all::<ImageExtractionRecord>(&workspace.conn, "image_extraction")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn typed_image_outcomes_and_retry_history_survive_backup_without_rasters() {
    let (temp, mut workspace, evidence) = workspace();
    let unsupported = workspace.import("synthetic.gif", GIF).unwrap();
    for (mode, expected) in [
        ("recognized", ProcessingState::Completed),
        ("empty", ProcessingState::Completed),
        ("unsupported", ProcessingState::Blocked),
        ("failed", ProcessingState::Failed),
        ("quota", ProcessingState::QuotaExhausted),
    ] {
        let job = queue(
            &mut workspace,
            if mode == "unsupported" {
                &unsupported
            } else {
                &evidence
            },
        );
        let prepared = workspace.claim_processing_job().unwrap().unwrap();
        let finished = workspace
            .finish_processing_job(
                &prepared.ticket,
                Ok(&ProcessingOutput::Image(Box::new(outcome(mode)))),
            )
            .unwrap();
        assert_eq!(finished.state, expected);
        let previous = workspace.image_extraction(&finished.result_ids[0]).unwrap();
        if mode == "empty" {
            assert_eq!(
                previous.result.recognition.as_ref().unwrap().status,
                crate::engines::ocr::OcrStatus::NoTextRecognized
            );
            assert_eq!(
                previous.result.recognition.as_ref().unwrap().text,
                "\n\x0c",
                "Canonical no-text output retains its original whitespace"
            );
            assert!(finished.detail.contains("without recognized text"));
        }
        if mode == "failed" {
            workspace
                .retry_processing_job(&job.id, 1, "Synthetic explicit image retry")
                .unwrap();
            let retry = workspace.claim_processing_job().unwrap().unwrap();
            let completed = workspace
                .finish_processing_job(
                    &retry.ticket,
                    Ok(&ProcessingOutput::Image(Box::new(recognized()))),
                )
                .unwrap();
            assert_eq!(completed.result_ids.len(), 2);
            assert_eq!(
                workspace
                    .image_extraction(&completed.result_ids[0])
                    .unwrap()
                    .result_sha256,
                previous.result_sha256
            );
        }
    }
    let records = all::<ImageExtractionRecord>(&workspace.conn, "image_extraction").unwrap();
    assert_eq!(records.len(), 6);
    let backup = workspace.backup().unwrap();
    let restored = Workspace::restore(&backup, &temp.path().join("restored")).unwrap();
    for record in records {
        let after = restored.image_extraction(&record.id).unwrap();
        assert_eq!(
            serde_json::to_value(after).unwrap(),
            serde_json::to_value(record).unwrap()
        );
    }
    assert_eq!(
        fs::read_dir(restored.root.join("scratch")).unwrap().count(),
        0
    );
    for source in restored.view().unwrap().evidence {
        restored.verify_original(&source).unwrap();
        assert!(source.text.is_none());
    }
}

#[test]
fn image_cancellation_and_unverified_exit_keep_shared_attempt_and_suspension_rules() {
    let (_temp, mut workspace, evidence) = workspace();
    let job = queue(&mut workspace, &evidence);
    let prepared = workspace.claim_processing_job().unwrap().unwrap();
    workspace.cancel_processing_job(&job.id, 1).unwrap();
    let cancelled = workspace
        .finish_processing_job(
            &prepared.ticket,
            Ok(&ProcessingOutput::Image(Box::new(recognized()))),
        )
        .unwrap();
    assert_eq!(cancelled.state, ProcessingState::Cancelled);
    assert!(cancelled.result_ids.is_empty());
    workspace
        .retry_processing_job(&job.id, 1, "Synthetic image retry")
        .unwrap();
    assert!(workspace.cancel_processing_job(&job.id, 1).is_err());
    let retry = workspace.claim_processing_job().unwrap().unwrap();
    assert!(workspace
        .finish_processing_job(
            &prepared.ticket,
            Ok(&ProcessingOutput::Image(Box::new(recognized())))
        )
        .is_err());
    let pending_image = queue(&mut workspace, &evidence);
    let pending_parse = workspace.queue_document_parse(&evidence, &id()).unwrap();
    workspace.cancel_processing_job(&job.id, 2).unwrap();
    let failed = workspace
        .finish_processing_job(
            &retry.ticket,
            Err(Error::TerminationUnverified(
                "Synthetic exit uncertainty".into(),
            )),
        )
        .unwrap();
    assert_eq!(failed.state, ProcessingState::Failed);
    assert_eq!(
        failed.failure,
        Some(ProcessingFailure::WorkerExitUnverified)
    );
    for pending in [pending_image, pending_parse] {
        assert_eq!(
            workspace.processing_job(&pending.id).unwrap().failure,
            Some(ProcessingFailure::RecoveryRequired)
        );
    }
    assert!(workspace.queue_image_ocr(&evidence, &id()).is_err());
    assert!(workspace.queue_document_parse(&evidence, &id()).is_err());
    assert!(workspace.claim_processing_job().is_err());
}

#[cfg(debug_assertions)]
#[test]
fn fixed_image_review_fixture_is_canonical_and_never_overwrites() {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut workspace = Workspace::open(temp.path().join("case")).unwrap();
    workspace.seed_image_processing_review().unwrap();
    assert_eq!(workspace.processing_jobs().unwrap().total, 10);
    assert_eq!(
        all::<ImageExtractionRecord>(&workspace.conn, "image_extraction")
            .unwrap()
            .len(),
        7
    );
    let before = workspace.revision().unwrap();
    assert!(workspace.seed_image_processing_review().is_err());
    assert_eq!(workspace.revision().unwrap(), before);
    assert!(workspace.view().unwrap().observations.is_empty());
}
