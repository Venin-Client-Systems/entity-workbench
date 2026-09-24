use super::*;
#[cfg(target_os = "macos")]
use crate::processing::ImageRegionInspection;
use crate::processing::{ProcessingFailure, ProcessingJob, ProcessingState};
use std::time::Instant;

fn job(coordinator: &JobCoordinator, id: &str) -> ProcessingJob {
    serde_json::from_value(
        coordinator
            .dispatch(Command::InspectProcessingJob { job_id: id.into() })
            .unwrap(),
    )
    .unwrap()
}
fn terminal(coordinator: &JobCoordinator, id: &str) -> ProcessingJob {
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let current = job(coordinator, id);
        if !matches!(
            current.state,
            ProcessingState::Queued | ProcessingState::Running
        ) {
            return current;
        }
        assert!(
            Instant::now() < deadline,
            "Synthetic region job deadline exceeded"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn region_missing_runtime_is_visible_and_worker_pool_remains_bounded() {
    use std::sync::atomic::AtomicUsize;
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut workspace = Workspace::open(temp.path().join("case")).unwrap();
    let source = workspace
        .import(
            "synthetic.png",
            include_bytes!("../../../fixtures/images/synthetic.png"),
        )
        .unwrap();
    let missing = workspace
        .queue_image_ocr_regions(&source, &uuid::Uuid::new_v4().to_string())
        .unwrap();
    let coordinator = JobCoordinator::start(workspace, 2).unwrap();
    let done = terminal(&coordinator, &missing.id);
    assert_eq!(done.state, ProcessingState::Blocked);
    assert_eq!(done.failure, Some(ProcessingFailure::RuntimeUnavailable));
    assert!(done.result_ids.is_empty());
    coordinator.shutdown().unwrap();
    drop(coordinator);
    let mut workspace = Workspace::open(temp.path().join("case")).unwrap();
    let mut queued = Vec::new();
    for _ in 0..3 {
        queued.push(
            workspace
                .queue_image_ocr_regions(&source, &uuid::Uuid::new_v4().to_string())
                .unwrap(),
        );
    }
    let active = Arc::new(AtomicUsize::new(0));
    let counter = active.clone();
    let coordinator = JobCoordinator::with_executor(
        workspace,
        2,
        Arc::new(move |_, _, input, _, token| {
            assert!(matches!(input, ProcessingInput::ImageOcrRegions { .. }));
            assert!(counter.fetch_add(1, Ordering::AcqRel) < 2);
            while !token.is_cancelled() {
                thread::sleep(Duration::from_millis(5));
            }
            counter.fetch_sub(1, Ordering::AcqRel);
            Err(Error::Blocked("Synthetic cancelled executor".into()))
        }),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while active.load(Ordering::Acquire) != 2 {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        job(&coordinator, &queued[2].id).state,
        ProcessingState::Queued
    );
    coordinator.shutdown().unwrap();
    assert_eq!(active.load(Ordering::Acquire), 0);
}

#[test]
#[cfg(target_os = "macos")]
#[ignore = "requires actual confined image and English word-region OCR runtimes"]
fn native_image_regions_retain_exact_provenance_raster_tsv_and_restore() {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let root = temp.path().join("case");
    let mut workspace = Workspace::open(&root).unwrap();
    workspace.attach_runtime(Runtime {
        root: std::env::var_os("WORKBENCH_TEST_IMAGE_RUNTIME")
            .expect("Image runtime required")
            .into(),
    });
    let mut queued = Vec::new();
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
        let evidence = workspace.import(name, bytes).unwrap();
        let pending = workspace
            .queue_image_ocr_regions(&evidence, &uuid::Uuid::new_v4().to_string())
            .unwrap();
        queued.push((bytes, pending));
    }
    let coordinator = JobCoordinator::start(workspace, 2).unwrap();
    let mut records = Vec::new();
    for (original, pending) in queued {
        let done = terminal(&coordinator, &pending.id);
        assert_eq!(done.state, ProcessingState::Completed, "{}", done.detail);
        let inspection: ImageRegionInspection = serde_json::from_value(
            coordinator
                .dispatch(Command::InspectImageRegionExtraction {
                    extraction_id: done.result_ids[0].clone(),
                })
                .unwrap(),
        )
        .unwrap();
        let recognition = inspection.result.recognition.as_ref().unwrap();
        assert_eq!(
            inspection.result.decoder.original_sha256,
            crate::store::hash(original)
        );
        assert!(recognition.text.contains("SYNTHETIC OCR TEST"));
        assert!(recognition.text.contains("REFERENCE 0042 AMOUNT 123.45"));
        let words: Vec<_> = recognition
            .regions
            .iter()
            .filter(|r| r.level == crate::engines::ocr_regions::RegionLevel::Word)
            .collect();
        assert_eq!(
            words
                .iter()
                .map(|word| word.text.as_str())
                .collect::<Vec<_>>(),
            [
                "SYNTHETIC",
                "OCR",
                "TEST",
                "REFERENCE",
                "0042",
                "AMOUNT",
                "123.45"
            ]
        );
        assert!(words
            .iter()
            .all(|r| r.engine_confidence.is_some() && r.bounds.width > 0 && r.bounds.height > 0));
        assert_eq!(
            inspection.extraction.raster.as_ref().unwrap().sha256,
            recognition.raster_sha256
        );
        assert_eq!(
            inspection.extraction.tsv.as_ref().unwrap().sha256,
            recognition.tsv_sha256
        );
        assert_ne!(inspection.result.decoder.job_id, recognition.job_id);
        records.push(inspection);
    }
    coordinator.shutdown().unwrap();
    drop(coordinator);
    let mut workspace = Workspace::open(&root).unwrap();
    assert_eq!(std::fs::read_dir(root.join("scratch")).unwrap().count(), 0);
    assert!(workspace
        .view()
        .unwrap()
        .evidence
        .iter()
        .all(|e| e.text.is_none()));
    assert!(workspace.view().unwrap().observations.is_empty());
    let backup = workspace.backup().unwrap();
    let restored = Workspace::restore(&backup, &temp.path().join("restored")).unwrap();
    for before in records {
        let raster = workspace
            .read_image_region_raster(&before.extraction.id)
            .unwrap();
        let after_raster = restored
            .read_image_region_raster(&before.extraction.id)
            .unwrap();
        assert_eq!(raster, after_raster);
        assert_eq!(
            crate::engines::ocr::raster_dimensions(&raster).unwrap(),
            (1200, 230)
        );
        let after = restored
            .inspect_image_region_extraction(&before.extraction.id)
            .unwrap();
        assert_eq!(
            serde_json::to_value(before).unwrap(),
            serde_json::to_value(after).unwrap()
        );
    }
}
