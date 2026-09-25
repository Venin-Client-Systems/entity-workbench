use super::*;
use crate::processing::{ProcessingFailure, ProcessingJob, ProcessingState};
use std::{sync::atomic::AtomicUsize, time::Instant};
fn inspect(coordinator: &JobCoordinator, id: &str) -> ProcessingJob {
    serde_json::from_value(
        coordinator
            .dispatch(Command::InspectProcessingJob { job_id: id.into() })
            .unwrap(),
    )
    .unwrap()
}
fn until(timeout: Duration, mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + timeout;
    while !check() {
        assert!(
            Instant::now() < deadline,
            "PDF coordinator test deadline exceeded"
        );
        thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn pdf_and_legacy_jobs_share_two_slots_and_joined_shutdown() {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let root = temp.path().join("case");
    let mut workspace = Workspace::open(&root).unwrap();
    let source = workspace
        .import(
            "synthetic.pdf",
            include_bytes!("../../../fixtures/pdf-render/scan.pdf"),
        )
        .unwrap();
    let pdf = workspace
        .queue_pdf_page_ocr(&source, &uuid::Uuid::new_v4().to_string(), 2, 144)
        .unwrap();
    let parse = workspace
        .queue_document_parse(&source, &uuid::Uuid::new_v4().to_string())
        .unwrap();
    let pending = workspace
        .queue_image_ocr(&source, &uuid::Uuid::new_v4().to_string())
        .unwrap();
    let active = Arc::new(AtomicUsize::new(0));
    let seen = Arc::new(AtomicBool::new(false));
    let counter = active.clone();
    let flag = seen.clone();
    let coordinator = JobCoordinator::with_executor(
        workspace,
        2,
        Arc::new(move |_, _, input, _, token| {
            if let ProcessingInput::PdfPageOcr {
                page_number, dpi, ..
            } = input
            {
                assert_eq!((*page_number, *dpi), (2, 144));
                flag.store(true, Ordering::Release);
            }
            counter.fetch_add(1, Ordering::AcqRel);
            while !token.is_cancelled() {
                thread::sleep(Duration::from_millis(5));
            }
            counter.fetch_sub(1, Ordering::AcqRel);
            Err(Error::Blocked("Synthetic stopped executor".into()))
        }),
    )
    .unwrap();
    until(Duration::from_secs(5), || {
        active.load(Ordering::Acquire) == 2
    });
    assert!(seen.load(Ordering::Acquire));
    assert_eq!(
        inspect(&coordinator, &pending.id).state,
        ProcessingState::Queued
    );
    coordinator.shutdown().unwrap();
    assert_eq!(active.load(Ordering::Acquire), 0);
    let reopened = Workspace::open(&root).unwrap();
    for job in [pdf, parse] {
        assert_eq!(
            reopened.processing_job(&job.id).unwrap().failure,
            Some(ProcessingFailure::Interrupted)
        );
    }
    assert_eq!(
        reopened.processing_job(&pending.id).unwrap().state,
        ProcessingState::Queued
    );
}
#[test]
fn pdf_running_cancellation_and_panic_do_not_publish_or_hide_unknown_exit() {
    for panic in [false, true] {
        let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let mut workspace = Workspace::open(temp.path()).unwrap();
        let source = workspace
            .import(
                "synthetic.pdf",
                include_bytes!("../../../fixtures/pdf-render/scan.pdf"),
            )
            .unwrap();
        let job = workspace
            .queue_pdf_page_ocr(&source, &uuid::Uuid::new_v4().to_string(), 2, 144)
            .unwrap();
        let pending = workspace
            .queue_pdf_page_ocr(&source, &uuid::Uuid::new_v4().to_string(), 1, 72)
            .unwrap();
        let started = Arc::new(AtomicBool::new(false));
        let entered = started.clone();
        let coordinator = JobCoordinator::with_executor(
            workspace,
            1,
            Arc::new(move |_, _, _, _, token| {
                if panic {
                    panic!("Synthetic PDF executor panic");
                }
                entered.store(true, Ordering::Release);
                while !token.is_cancelled() {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(Error::Blocked("Synthetic cancelled PDF worker".into()))
            }),
        )
        .unwrap();
        if panic {
            until(Duration::from_secs(5), || {
                inspect(&coordinator, &job.id).state == ProcessingState::Failed
            });
            assert_eq!(
                inspect(&coordinator, &job.id).failure,
                Some(ProcessingFailure::WorkerExitUnverified)
            );
            assert_eq!(
                inspect(&coordinator, &pending.id).failure,
                Some(ProcessingFailure::RecoveryRequired)
            );
            assert!(coordinator
                .dispatch(Command::QueuePdfPageOcr {
                    evidence_id: source,
                    request_key: uuid::Uuid::new_v4().to_string(),
                    page_number: 2,
                    dpi: 144
                })
                .is_err());
        } else {
            until(Duration::from_secs(5), || started.load(Ordering::Acquire));
            coordinator
                .dispatch(Command::CancelProcessingJob {
                    job_id: job.id.clone(),
                    expected_attempt: 1,
                })
                .unwrap();
            until(Duration::from_secs(5), || {
                inspect(&coordinator, &job.id).state == ProcessingState::Cancelled
            });
        }
        assert!(inspect(&coordinator, &job.id).result_ids.is_empty());
        if panic {
            assert!(coordinator.shutdown().is_err());
            assert!(JobCoordinator::start(Workspace::open(temp.path()).unwrap(), 1).is_err());
        } else {
            coordinator.shutdown().unwrap();
        }
    }
}
#[test]
#[cfg(target_os = "macos")]
#[ignore = "requires real confined PDF renderer and packaged English OCR"]
fn native_pdf_jobs_publish_selected_pages_and_restore_exact_unreviewed_provenance() {
    use crate::{
        engines::{ocr::OcrStatus, pdf_render::RenderStatus},
        processing::PdfExtractionRecord,
    };
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let root = temp.path().join("case");
    let mut workspace = Workspace::open(&root).unwrap();
    workspace.attach_runtime(Runtime {
        root: std::env::var_os("WORKBENCH_TEST_PDF_RUNTIME")
            .expect("Staged PDF runtime required")
            .into(),
    });
    let mut sources = Vec::new();
    for (name, bytes, page, status, failure) in [
        (
            "scan.pdf",
            include_bytes!("../../../fixtures/pdf-render/scan.pdf").as_slice(),
            1,
            ProcessingState::Completed,
            None,
        ),
        (
            "scan-jpeg.pdf",
            include_bytes!("../../../fixtures/pdf-render/scan-jpeg.pdf").as_slice(),
            1,
            ProcessingState::Completed,
            None,
        ),
        (
            "scan.pdf",
            include_bytes!("../../../fixtures/pdf-render/scan.pdf").as_slice(),
            2,
            ProcessingState::Completed,
            None,
        ),
        (
            "encrypted.pdf",
            include_bytes!("../../../fixtures/pdf-render/encrypted.pdf").as_slice(),
            1,
            ProcessingState::Blocked,
            Some(ProcessingFailure::EncryptedDocument),
        ),
        (
            "font.pdf",
            include_bytes!("../../../fixtures/pdf-render/font.pdf").as_slice(),
            1,
            ProcessingState::Blocked,
            Some(ProcessingFailure::UnsupportedFormat),
        ),
        (
            "malformed.pdf",
            include_bytes!("../../../fixtures/pdf-render/malformed.pdf").as_slice(),
            1,
            ProcessingState::Failed,
            Some(ProcessingFailure::PdfRenderFailed),
        ),
        (
            "large-page.pdf",
            include_bytes!("../../../fixtures/pdf-render/large-page.pdf").as_slice(),
            1,
            ProcessingState::QuotaExhausted,
            Some(ProcessingFailure::WorkerFailed),
        ),
    ] {
        let source = workspace.import(name, bytes).unwrap();
        let job = workspace
            .queue_pdf_page_ocr(&source, &uuid::Uuid::new_v4().to_string(), page, 144)
            .unwrap();
        sources.push((source, bytes, page, status, failure, job));
    }
    let coordinator = JobCoordinator::start(workspace, 2).unwrap();
    let mut snapshots = Vec::new();
    for (source, bytes, page, status, failure, queued) in &sources {
        until(Duration::from_secs(120), || {
            !matches!(
                inspect(&coordinator, &queued.id).state,
                ProcessingState::Queued | ProcessingState::Running
            )
        });
        let finished = inspect(&coordinator, &queued.id);
        assert_eq!(&finished.state, status, "{}", finished.detail);
        assert_eq!(&finished.failure, failure);
        assert_eq!(finished.result_ids.len(), 1);
        let record: PdfExtractionRecord = serde_json::from_value(
            coordinator
                .dispatch(Command::InspectPdfExtraction {
                    extraction_id: finished.result_ids[0].clone(),
                })
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            record.result.render.original_sha256,
            crate::store::hash(bytes)
        );
        assert_eq!(record.result.render.page_number, *page);
        assert_eq!(record.result.render.dpi, 144);
        assert!(!record.result.raster_retained);
        if *status == ProcessingState::Completed {
            assert_eq!(record.result.render.status, RenderStatus::Rendered);
            let raster = record.result.render.raster.as_ref().unwrap();
            assert_eq!((raster.width, raster.height), (1200, 230));
            let recognized = record.result.recognition.as_ref().unwrap();
            assert_eq!(recognized.raster_sha256, raster.sha256);
            if *page == 2 {
                assert_eq!(recognized.status, OcrStatus::NoTextRecognized);
                assert!(recognized.text.trim().is_empty());
            } else {
                assert!(recognized.text.contains("SYNTHETIC OCR TEST"));
                assert!(recognized.text.contains("REFERENCE 0042 AMOUNT 123.45"));
            }
        } else {
            assert!(record.result.recognition.is_none());
            assert!(record.result.render.raster.is_none());
        }
        let view = coordinator.dispatch(Command::View {}).unwrap();
        let evidence: Vec<crate::domain::Evidence> =
            serde_json::from_value(view["workspace"]["evidence"].clone()).unwrap();
        assert!(evidence
            .iter()
            .find(|e| e.id == *source)
            .unwrap()
            .text
            .is_none());
        assert!(view["workspace"]["observations"]
            .as_array()
            .unwrap()
            .is_empty());
        snapshots.push(record);
    }
    coordinator.shutdown().unwrap();
    drop(coordinator);
    assert_eq!(std::fs::read_dir(root.join("scratch")).unwrap().count(), 0);
    let mut workspace = Workspace::open(&root).unwrap();
    let backup = workspace.backup().unwrap();
    let restored = Workspace::restore(&backup, &temp.path().join("restored")).unwrap();
    for record in snapshots {
        assert_eq!(
            serde_json::to_value(restored.pdf_extraction(&record.id).unwrap()).unwrap(),
            serde_json::to_value(&record).unwrap()
        );
        assert!(restored
            .processing_job(&record.job_id)
            .unwrap()
            .result_ids
            .contains(&record.id));
    }
    for (source, bytes, _, _, _, _) in sources {
        let evidence = restored
            .view()
            .unwrap()
            .evidence
            .into_iter()
            .find(|e| e.id == source)
            .unwrap();
        assert!(evidence.text.is_none());
        assert_eq!(
            std::fs::read(temp.path().join("restored/originals").join(evidence.sha256)).unwrap(),
            bytes
        );
    }
}
