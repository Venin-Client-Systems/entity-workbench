use super::*;
#[cfg(target_os = "macos")]
use crate::processing::ImageExtractionRecord;
use crate::processing::{ProcessingFailure, ProcessingJob, ProcessingState};
use std::{sync::atomic::AtomicUsize, time::Instant};

fn job(coordinator: &JobCoordinator, id: &str) -> ProcessingJob {
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
            "Synthetic image job deadline exceeded"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn image_and_parse_share_two_slots_and_joined_shutdown() {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut workspace = Workspace::open(temp.path().join("case")).unwrap();
    let source = workspace
        .import(
            "synthetic.png",
            include_bytes!("../../../fixtures/images/synthetic.png"),
        )
        .unwrap();
    let image = workspace
        .queue_image_ocr(&source, &uuid::Uuid::new_v4().to_string())
        .unwrap();
    let parse = workspace
        .queue_document_parse(&source, &uuid::Uuid::new_v4().to_string())
        .unwrap();
    let queued = workspace
        .queue_image_ocr(&source, &uuid::Uuid::new_v4().to_string())
        .unwrap();
    let active = Arc::new(AtomicUsize::new(0));
    let seen_image = Arc::new(AtomicBool::new(false));
    let seen_parse = Arc::new(AtomicBool::new(false));
    let counter = active.clone();
    let image_flag = seen_image.clone();
    let parse_flag = seen_parse.clone();
    let coordinator = JobCoordinator::with_executor(
        workspace,
        2,
        Arc::new(move |_, _, input, _, token| {
            match input {
                ProcessingInput::ImageOcr { .. } => image_flag.store(true, Ordering::Release),
                ProcessingInput::ParseDocument { .. } => parse_flag.store(true, Ordering::Release),
                ProcessingInput::ShortestConnectionPath { .. }
                | ProcessingInput::PdfPageOcr { .. }
                | ProcessingInput::ImageOcrRegions { .. } => {
                    panic!("Unexpected PDF job in image/parse fixture")
                }
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
    assert!(seen_image.load(Ordering::Acquire) && seen_parse.load(Ordering::Acquire));
    assert_eq!(job(&coordinator, &queued.id).state, ProcessingState::Queued);
    coordinator.shutdown().unwrap();
    assert_eq!(active.load(Ordering::Acquire), 0);
    let reopened = Workspace::open(temp.path().join("case")).unwrap();
    for value in [image, parse] {
        assert_eq!(
            reopened.processing_job(&value.id).unwrap().failure,
            Some(ProcessingFailure::Interrupted)
        );
    }
    assert_eq!(
        reopened.processing_job(&queued.id).unwrap().state,
        ProcessingState::Queued
    );
}

#[test]
fn executor_panic_cannot_claim_worker_exit_or_start_queued_work() {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut workspace = Workspace::open(temp.path().join("case")).unwrap();
    let source = workspace
        .import(
            "synthetic.png",
            include_bytes!("../../../fixtures/images/synthetic.png"),
        )
        .unwrap();
    let first = workspace
        .queue_image_ocr(&source, &uuid::Uuid::new_v4().to_string())
        .unwrap();
    let second = workspace
        .queue_document_parse(&source, &uuid::Uuid::new_v4().to_string())
        .unwrap();
    let coordinator = JobCoordinator::with_executor(
        workspace,
        1,
        Arc::new(|_, _, _, _, _| panic!("Synthetic executor panic")),
    )
    .unwrap();
    until(Duration::from_secs(5), || {
        job(&coordinator, &first.id).state == ProcessingState::Failed
    });
    assert_eq!(
        job(&coordinator, &first.id).failure,
        Some(ProcessingFailure::WorkerExitUnverified)
    );
    assert_eq!(
        job(&coordinator, &second.id).failure,
        Some(ProcessingFailure::RecoveryRequired)
    );
    assert!(coordinator
        .dispatch(Command::QueueImageOcr {
            evidence_id: source,
            request_key: uuid::Uuid::new_v4().to_string()
        })
        .is_err());
    assert!(coordinator.shutdown().is_err());
    assert!(JobCoordinator::start(Workspace::open(temp.path().join("case")).unwrap(), 1).is_err());
}

#[test]
#[cfg(target_os = "macos")]
#[ignore = "requires real confined PNG/JPEG and app-local OCR runtimes"]
fn native_coordinator_images_publish_and_restore_recognition_and_provenance() {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let root = temp.path().join("case");
    let mut workspace = Workspace::open(&root).unwrap();
    workspace.attach_runtime(Runtime {
        root: std::env::var_os("WORKBENCH_TEST_IMAGE_RUNTIME")
            .expect("Image runtime required")
            .into(),
    });
    let mut originals = Vec::new();
    for (name, bytes) in [
        (
            "synthetic.png",
            include_bytes!("../../../fixtures/images/synthetic.png").as_slice(),
        ),
        (
            "synthetic.jpg",
            include_bytes!("../../../fixtures/images/synthetic.jpg").as_slice(),
        ),
    ] {
        let source = workspace.import(name, bytes).unwrap();
        let queued = workspace
            .queue_image_ocr(&source, &uuid::Uuid::new_v4().to_string())
            .unwrap();
        originals.push((source, bytes, queued));
    }
    let coordinator = JobCoordinator::start(workspace, 2).unwrap();
    let mut snapshots = Vec::new();
    for (source, bytes, queued) in &originals {
        until(Duration::from_secs(70), || {
            !matches!(
                job(&coordinator, &queued.id).state,
                ProcessingState::Queued | ProcessingState::Running
            )
        });
        let finished = job(&coordinator, &queued.id);
        assert_eq!(
            finished.state,
            ProcessingState::Completed,
            "{}",
            finished.detail
        );
        assert_eq!(finished.result_ids.len(), 1);
        let extraction: ImageExtractionRecord = serde_json::from_value(
            coordinator
                .dispatch(Command::InspectImageExtraction {
                    extraction_id: finished.result_ids[0].clone(),
                })
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            extraction.result.decoder.original_sha256,
            crate::store::hash(bytes)
        );
        let recognition = extraction.result.recognition.as_ref().unwrap();
        assert!(recognition.text.contains("SYNTHETIC OCR TEST"));
        assert!(recognition.text.contains("REFERENCE 0042 AMOUNT 123.45"));
        let raster = extraction.result.decoder.raster.as_ref().unwrap();
        assert_eq!(
            (raster.width, raster.height, raster.source_image_index),
            (1200, 230, 0)
        );
        assert_eq!(recognition.raster_sha256, raster.sha256);
        assert!(!extraction.result.raster_retained);
        let view = coordinator.dispatch(Command::View {}).unwrap();
        let evidence: Vec<crate::domain::Evidence> =
            serde_json::from_value(view["workspace"]["evidence"].clone()).unwrap();
        assert!(evidence
            .iter()
            .find(|item| item.id == *source)
            .unwrap()
            .text
            .is_none());
        assert!(view["workspace"]["observations"]
            .as_array()
            .unwrap()
            .is_empty());
        snapshots.push(extraction);
    }
    coordinator.shutdown().unwrap();
    drop(coordinator);
    let mut workspace = Workspace::open(&root).unwrap();
    assert_eq!(std::fs::read_dir(root.join("scratch")).unwrap().count(), 0);
    let backup = workspace.backup().unwrap();
    let restored = Workspace::restore(&backup, &temp.path().join("restored")).unwrap();
    for snapshot in snapshots {
        let after = restored.image_extraction(&snapshot.id).unwrap();
        assert_eq!(
            serde_json::to_value(&snapshot).unwrap(),
            serde_json::to_value(after).unwrap()
        );
    }
    for (source, bytes, queued) in originals {
        assert_eq!(
            restored.processing_job(&queued.id).unwrap().state,
            ProcessingState::Completed
        );
        let evidence = restored
            .view()
            .unwrap()
            .evidence
            .into_iter()
            .find(|item| item.id == source)
            .unwrap();
        assert!(evidence.text.is_none());
        assert_eq!(
            std::fs::read(
                temp.path()
                    .join("restored")
                    .join("originals")
                    .join(evidence.sha256)
            )
            .unwrap(),
            bytes
        );
    }
}
