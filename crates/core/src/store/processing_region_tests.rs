use super::*;
use crate::engines::{image_regions::ImageOcrRegions, ocr_regions::*};
const PNG: &[u8] = include_bytes!("../../../../fixtures/images/synthetic.png");
fn workspace() -> (tempfile::TempDir, Workspace, String) {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut workspace = Workspace::open(temp.path().join("case")).unwrap();
    let source = workspace.import("synthetic.png", PNG).unwrap();
    (temp, workspace, source)
}
fn fixture() -> ProcessingOutput {
    let image = image_fixtures::recognized().image;
    let raster = image.raster.as_ref().unwrap();
    let mut rows=String::from("level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n");
    let mut regions = Vec::new();
    for (index, level) in [
        RegionLevel::Page,
        RegionLevel::Block,
        RegionLevel::Paragraph,
        RegionLevel::Line,
        RegionLevel::Word,
    ]
    .into_iter()
    .enumerate()
    {
        let word = level == RegionLevel::Word;
        let dims = if index == 0 { (1200, 230) } else { (100, 40) };
        let row = OcrRegion {
            level,
            page_number: 1,
            block_number: u32::from(index >= 1),
            paragraph_number: u32::from(index >= 2),
            line_number: u32::from(index >= 3),
            word_number: u32::from(word),
            bounds: RasterBox {
                left: 0,
                top: 0,
                width: dims.0,
                height: dims.1,
            },
            engine_confidence: word.then_some(95.0),
            text: if word {
                "SYNTHETIC".into()
            } else {
                String::new()
            },
        };
        rows.push_str(&format!(
            "{}\t1\t{}\t{}\t{}\t{}\t0\t0\t{}\t{}\t{}\t{}\n",
            index + 1,
            row.block_number,
            row.paragraph_number,
            row.line_number,
            row.word_number,
            dims.0,
            dims.1,
            if word { "95.000000" } else { "-1" },
            row.text
        ));
        regions.push(row);
    }
    let tsv = rows.into_bytes();
    let result = OcrRegionsResult {
        protocol_version: 1,
        job_id: id(),
        raster_sha256: hash(raster),
        raster_bytes: raster.len() as u64,
        width: 1200,
        height: 230,
        engine: "tesseract-5.5.2".into(),
        language: "eng".into(),
        model_sha256: "7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2".into(),
        runtime_manifest_sha256: "0".repeat(64),
        status: RegionsStatus::Recognized,
        text: "SYNTHETIC\n".into(),
        tsv_sha256: hash(&tsv),
        tsv_bytes: tsv.len() as u64,
        regions,
        limitations: vec![
            RegionsLimitation::UnreviewedRecognition,
            RegionsLimitation::UnreviewedWordRegions,
            RegionsLimitation::EngineConfidenceNotProbability,
            RegionsLimitation::NoOriginalDocumentMapping,
            RegionsLimitation::SingleUniformBlock,
        ],
    };
    ProcessingOutput::ImageRegions(Box::new(ImageOcrRegions {
        image,
        ocr: Some(OcrRegions { result, tsv }),
    }))
}
fn publish(
    workspace: &mut Workspace,
    source: &str,
) -> (ProcessingJob, ProcessingOutput, JobTicket) {
    workspace.queue_image_ocr_regions(source, &id()).unwrap();
    let prepared = workspace.claim_processing_job().unwrap().unwrap();
    let output = fixture();
    let job = workspace
        .finish_processing_job(&prepared.ticket, Ok(&output))
        .unwrap();
    (job, output, prepared.ticket)
}
fn overwrite(path: &Path, bytes: &[u8]) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    #[cfg(windows)]
    {
        let mut p = fs::metadata(path).unwrap().permissions();
        p.set_readonly(false);
        fs::set_permissions(path, p).unwrap();
    }
    fs::write(path, bytes).unwrap();
    private_file(path, 0o400).unwrap();
}
#[test]
fn region_jobs_opt_in_identity_and_immutable_files_survive_restore() {
    let (temp, mut workspace, source) = workspace();
    let key = id();
    let job = workspace.queue_image_ocr_regions(&source, &key).unwrap();
    assert_eq!(job.schema_version, 4);
    let revision = workspace.revision().unwrap();
    assert_eq!(
        workspace.queue_image_ocr_regions(&source, &key).unwrap().id,
        job.id
    );
    assert_eq!(workspace.revision().unwrap(), revision);
    assert!(workspace.queue_image_ocr(&source, &key).is_err());
    let prepared = workspace.claim_processing_job().unwrap().unwrap();
    let output = fixture();
    let done = workspace
        .finish_processing_job(&prepared.ticket, Ok(&output))
        .unwrap();
    assert_eq!(done.state, ProcessingState::Completed);
    let inspected = workspace
        .inspect_image_region_extraction(&done.result_ids[0])
        .unwrap();
    let json = serde_json::to_vec(&inspected.extraction).unwrap();
    assert!(
        json.len() < 1600,
        "Canonical record unexpectedly contains large result data"
    );
    assert_eq!(
        workspace
            .read_image_region_raster(&inspected.extraction.id)
            .unwrap(),
        include_bytes!("../../../../fixtures/ocr/synthetic.pgm")
    );
    let revision = workspace.revision().unwrap();
    workspace
        .finish_processing_job(&prepared.ticket, Ok(&output))
        .unwrap();
    assert_eq!(workspace.revision().unwrap(), revision);
    assert_eq!(
        workspace
            .conn
            .query_row::<u64, _, _>("SELECT count(*) FROM derivative_objects", [], |r| r.get(0))
            .unwrap(),
        3
    );
    assert!(workspace.view().unwrap().evidence[0].text.is_none());
    assert!(workspace.view().unwrap().observations.is_empty());
    let backup = workspace.backup().unwrap();
    let restored = Workspace::restore(&backup, &temp.path().join("restored")).unwrap();
    assert_eq!(
        serde_json::to_value(
            restored
                .inspect_image_region_extraction(&inspected.extraction.id)
                .unwrap()
        )
        .unwrap(),
        serde_json::to_value(inspected).unwrap()
    );
    assert_eq!(
        restored
            .read_image_region_raster(&done.result_ids[0])
            .unwrap(),
        workspace
            .read_image_region_raster(&done.result_ids[0])
            .unwrap()
    );
}
#[test]
fn region_publication_rollback_leaves_only_unreferenced_verified_objects() {
    let (_temp, mut workspace, source) = workspace();
    let job = workspace.queue_image_ocr_regions(&source, &id()).unwrap();
    let ticket = workspace.claim_processing_job().unwrap().unwrap().ticket;
    let output = fixture();
    workspace.conn.execute_batch("CREATE TRIGGER reject_region BEFORE UPDATE ON records WHEN NEW.kind='processing_job' AND json_extract(NEW.body,'$.state')='completed' BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").unwrap();
    let revision = workspace.revision().unwrap();
    assert!(workspace
        .finish_processing_job(&ticket, Ok(&output))
        .is_err());
    assert_eq!(workspace.revision().unwrap(), revision);
    assert_eq!(
        workspace.processing_job(&job.id).unwrap().state,
        ProcessingState::Running
    );
    assert!(
        all::<ImageRegionExtractionRecord>(&workspace.conn, "image_region_extraction")
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        workspace
            .conn
            .query_row::<u64, _, _>("SELECT count(*) FROM derivative_objects", [], |r| r.get(0))
            .unwrap(),
        0
    );
    let objects = workspace.root.join("derivatives/objects");
    assert_eq!(fs::read_dir(&objects).unwrap().count(), 3);
    let backup = workspace.backup().unwrap();
    assert!(!backup.join("derivatives/objects").exists());
    workspace
        .conn
        .execute_batch("DROP TRIGGER reject_region")
        .unwrap();
    let done = workspace
        .finish_processing_job(&ticket, Ok(&output))
        .unwrap();
    assert_eq!(done.state, ProcessingState::Completed);
    assert_eq!(fs::read_dir(objects).unwrap().count(), 3);
}
#[test]
fn region_files_and_original_tampering_block_reads_backup_and_restore() {
    for component in ["raster", "tsv", "result", "original"] {
        let (temp, mut workspace, source) = workspace();
        let (done, _, _) = publish(&mut workspace, &source);
        let inspected = workspace
            .inspect_image_region_extraction(&done.result_ids[0])
            .unwrap();
        let backup = workspace.backup().unwrap();
        let reference = match component {
            "raster" => inspected.extraction.raster.as_ref(),
            "tsv" => inspected.extraction.tsv.as_ref(),
            "result" => Some(&inspected.extraction.result),
            _ => None,
        };
        let relative = reference
            .map(|r| PathBuf::from("derivatives/objects").join(&r.sha256))
            .unwrap_or_else(|| PathBuf::from("originals").join(hash(PNG)));
        overwrite(&workspace.root.join(&relative), b"altered");
        assert!(workspace
            .inspect_image_region_extraction(&done.result_ids[0])
            .is_err());
        assert!(workspace
            .read_image_region_raster(&done.result_ids[0])
            .is_err());
        assert!(workspace.backup().is_err());
        overwrite(&backup.join(relative), b"altered");
        assert!(Workspace::restore(&backup, &temp.path().join("restored")).is_err());
        assert!(!temp.path().join("restored/workspace.db").exists());
    }
}
#[test]
fn region_cancellation_and_invalid_results_publish_no_objects() {
    for mode in [
        "cancel",
        "unverified",
        "raster",
        "tsv",
        "region",
        "original",
    ] {
        let (_temp, mut workspace, source) = workspace();
        let job = workspace.queue_image_ocr_regions(&source, &id()).unwrap();
        let ticket = workspace.claim_processing_job().unwrap().unwrap().ticket;
        let mut output = fixture();
        if mode == "cancel" {
            workspace.cancel_processing_job(&job.id, 1).unwrap();
        }
        let ProcessingOutput::ImageRegions(value) = &mut output else {
            unreachable!()
        };
        match mode {
            "raster" => {
                value.image.raster.as_mut().unwrap().pop();
            }
            "tsv" => {
                value.ocr.as_mut().unwrap().tsv.pop();
            }
            "region" => value.ocr.as_mut().unwrap().result.regions[4].bounds.left += 1,
            "original" => value.image.result.original_sha256 = "0".repeat(64),
            _ => {}
        }
        let result = if mode == "unverified" {
            Err(Error::TerminationUnverified("Synthetic".into()))
        } else {
            Ok(&output)
        };
        let done = workspace.finish_processing_job(&ticket, result).unwrap();
        assert!(done.result_ids.is_empty());
        assert!(!workspace.root.join("derivatives").exists());
        assert_eq!(
            done.failure,
            Some(match mode {
                "cancel" => ProcessingFailure::CancelledByAnalyst,
                "unverified" => ProcessingFailure::WorkerExitUnverified,
                _ => ProcessingFailure::InvalidResult,
            })
        );
    }
}

#[test]
fn region_existing_published_corruption_is_not_repaired_and_orphan_conflicts_are_distinct() {
    for published in [false, true] {
        for missing in [false, true] {
            if !published && missing {
                continue;
            }
            let (_temp, mut workspace, source) = workspace();
            let raster = include_bytes!("../../../../fixtures/ocr/synthetic.pgm");
            let target = workspace
                .root
                .join("derivatives/objects")
                .join(hash(raster));
            if published {
                publish(&mut workspace, &source);
            } else {
                private_dir(target.parent().unwrap()).unwrap();
                fs::write(&target, b"conflict").unwrap();
                private_file(&target, 0o400).unwrap();
            }
            if missing {
                #[cfg(windows)]
                {
                    let mut permissions = fs::metadata(&target).unwrap().permissions();
                    permissions.set_readonly(false);
                    fs::set_permissions(&target, permissions).unwrap();
                }
                fs::remove_file(&target).unwrap();
            } else if published {
                overwrite(&target, b"corrupt");
            }
            workspace.queue_image_ocr_regions(&source, &id()).unwrap();
            let ticket = workspace.claim_processing_job().unwrap().unwrap().ticket;
            let done = workspace
                .finish_processing_job(&ticket, Ok(&fixture()))
                .unwrap();
            assert_eq!(done.state, ProcessingState::Failed);
            assert_eq!(done.failure, Some(ProcessingFailure::DerivativeUnavailable));
            assert!(done.result_ids.is_empty());
            assert_eq!(
                done.detail.contains("previously published"),
                published,
                "{}",
                done.detail
            );
            if missing {
                assert!(!target.exists());
            } else {
                assert_ne!(fs::read(&target).unwrap(), raster);
            }
        }
    }
}

#[test]
fn region_empty_and_rejected_results_and_retry_history_remain_distinct() {
    let (temp, mut workspace, source) = workspace();
    let unsupported = workspace
        .import("unsupported.gif", image_fixtures::GIF)
        .unwrap();
    let mut snapshots = Vec::new();
    for (mode, expected) in [
        ("empty", ProcessingState::Completed),
        ("unsupported", ProcessingState::Blocked),
        ("failed", ProcessingState::Failed),
        ("quota", ProcessingState::QuotaExhausted),
    ] {
        let job = workspace
            .queue_image_ocr_regions(
                if mode == "unsupported" {
                    &unsupported
                } else {
                    &source
                },
                &id(),
            )
            .unwrap();
        let ticket = workspace.claim_processing_job().unwrap().unwrap().ticket;
        let mut output = fixture();
        let ProcessingOutput::ImageRegions(value) = &mut output else {
            unreachable!()
        };
        if mode == "empty" {
            let ocr = value.ocr.as_mut().unwrap();
            ocr.result.status = RegionsStatus::NoTextRecognized;
            ocr.result.text = "\n\x0c".into();
            ocr.result.regions.truncate(1);
            ocr.tsv = ocr
                .tsv
                .split_inclusive(|c| *c == b'\n')
                .take(2)
                .flatten()
                .copied()
                .collect();
            ocr.result.tsv_sha256 = hash(&ocr.tsv);
            ocr.result.tsv_bytes = ocr.tsv.len() as u64;
        } else {
            value.image = image_fixtures::outcome(mode).image;
            value.ocr = None;
        }
        let done = workspace
            .finish_processing_job(&ticket, Ok(&output))
            .unwrap();
        assert_eq!(done.state, expected, "{}", done.detail);
        let record = workspace
            .inspect_image_region_extraction(&done.result_ids[0])
            .unwrap();
        assert_eq!(record.extraction.raster.is_some(), mode == "empty");
        assert_eq!(record.extraction.tsv.is_some(), mode == "empty");
        if mode == "empty" {
            assert_eq!(record.result.recognition.as_ref().unwrap().text, "\n\x0c");
        } else {
            assert!(record.result.recognition.is_none());
            assert!(workspace
                .read_image_region_raster(&record.extraction.id)
                .is_err());
        }
        snapshots.push(record);
        if mode == "failed" {
            workspace
                .retry_processing_job(&job.id, 1, "Explicit synthetic retry")
                .unwrap();
            assert!(workspace.cancel_processing_job(&job.id, 1).is_err());
            let retry = workspace.claim_processing_job().unwrap().unwrap().ticket;
            assert!(workspace
                .finish_processing_job(&ticket, Ok(&fixture()))
                .is_err());
            let done = workspace
                .finish_processing_job(&retry, Ok(&fixture()))
                .unwrap();
            assert_eq!(done.result_ids.len(), 2);
            assert_eq!(done.attempt, 2);
            snapshots.push(
                workspace
                    .inspect_image_region_extraction(&done.result_ids[1])
                    .unwrap(),
            );
        }
    }
    let backup = workspace.backup().unwrap();
    let restored = Workspace::restore(&backup, &temp.path().join("restored")).unwrap();
    for before in snapshots {
        let after = restored
            .inspect_image_region_extraction(&before.extraction.id)
            .unwrap();
        assert_eq!(
            serde_json::to_value(before).unwrap(),
            serde_json::to_value(after).unwrap()
        );
    }
}

#[test]
fn region_completed_replay_rejects_changed_raster_and_tsv_bytes() {
    for component in ["raster", "tsv"] {
        let (_temp, mut workspace, source) = workspace();
        let (_, mut output, ticket) = publish(&mut workspace, &source);
        let revision = workspace.revision().unwrap();
        let ProcessingOutput::ImageRegions(value) = &mut output else {
            unreachable!()
        };
        if component == "raster" {
            value.image.raster.as_mut().unwrap()[20] ^= 1;
        } else {
            value.ocr.as_mut().unwrap().tsv.pop();
        }
        assert!(workspace
            .finish_processing_job(&ticket, Ok(&output))
            .is_err());
        assert_eq!(workspace.revision().unwrap(), revision);
    }
}

#[test]
#[cfg(unix)]
fn region_links_and_oversized_objects_fail_without_modifying_outside_sentinels() {
    use std::os::unix::fs::symlink;
    for mode in ["symlink", "hardlink", "directory", "oversized"] {
        let (temp, mut workspace, source) = workspace();
        let bytes = include_bytes!("../../../../fixtures/ocr/synthetic.pgm");
        let target = workspace.root.join("derivatives/objects").join(hash(bytes));
        private_dir(target.parent().unwrap()).unwrap();
        let sentinel = temp.path().join("outside");
        fs::write(&sentinel, bytes).unwrap();
        private_file(&sentinel, 0o400).unwrap();
        match mode {
            "symlink" => symlink(&sentinel, &target).unwrap(),
            "hardlink" => fs::hard_link(&sentinel, &target).unwrap(),
            "directory" => fs::create_dir(&target).unwrap(),
            _ => {
                fs::write(&target, [bytes.as_slice(), b"x"].concat()).unwrap();
                private_file(&target, 0o400).unwrap();
            }
        }
        workspace.queue_image_ocr_regions(&source, &id()).unwrap();
        let ticket = workspace.claim_processing_job().unwrap().unwrap().ticket;
        let done = workspace
            .finish_processing_job(&ticket, Ok(&fixture()))
            .unwrap();
        assert_eq!(done.failure, Some(ProcessingFailure::DerivativeUnavailable));
        assert!(done.result_ids.is_empty());
        assert_eq!(fs::read(&sentinel).unwrap(), bytes);
    }
}

#[test]
fn region_backup_uses_snapshot_catalog_despite_concurrent_publication() {
    let (temp, mut workspace, source) = workspace();
    let (first, _, _) = publish(&mut workspace, &source);
    let root = workspace.root.clone();
    let revision = workspace.revision().unwrap();
    let backup = workspace
        .backup_snapshot(|| {
            let mut other = Workspace::open(&root)?;
            publish(&mut other, &source);
            Ok(())
        })
        .unwrap();
    assert_eq!(
        all::<ImageRegionExtractionRecord>(&workspace.conn, "image_region_extraction")
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        workspace
            .conn
            .query_row::<u64, _, _>("SELECT count(*) FROM derivative_objects", [], |r| r.get(0))
            .unwrap(),
        4
    );
    let restored = Workspace::restore(&backup, &temp.path().join("restored")).unwrap();
    assert_eq!(restored.revision().unwrap(), revision);
    assert_eq!(
        all::<ImageRegionExtractionRecord>(&restored.conn, "image_region_extraction")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        restored
            .conn
            .query_row::<u64, _, _>("SELECT count(*) FROM derivative_objects", [], |r| r.get(0))
            .unwrap(),
        3
    );
    assert_eq!(
        fs::read_dir(backup.join("derivatives/objects"))
            .unwrap()
            .count(),
        3
    );
    restored
        .inspect_image_region_extraction(&first.result_ids[0])
        .unwrap();
}

#[test]
#[cfg(windows)]
fn region_replaced_directory_junction_cannot_write_outside() {
    let (temp, mut workspace, source) = workspace();
    let outside = temp.path().join("outside");
    fs::create_dir(&outside).unwrap();
    let derivatives = workspace.root.join("derivatives");
    let created = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&derivatives)
        .arg(&outside)
        .output()
        .unwrap();
    assert!(created.status.success());
    workspace.queue_image_ocr_regions(&source, &id()).unwrap();
    let ticket = workspace.claim_processing_job().unwrap().unwrap().ticket;
    let done = workspace
        .finish_processing_job(&ticket, Ok(&fixture()))
        .unwrap();
    assert_eq!(done.failure, Some(ProcessingFailure::DerivativeUnavailable));
    assert!(done.result_ids.is_empty());
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
    fs::remove_dir(derivatives).unwrap();
}

#[test]
fn region_incomplete_or_mismatched_backup_manifest_never_publishes_database() {
    for mode in ["incomplete", "references", "revision"] {
        let (temp, mut workspace, source) = workspace();
        publish(&mut workspace, &source);
        let backup = workspace.backup().unwrap();
        let path = backup.join("manifest.json");
        let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        match mode {
            "incomplete" => manifest["complete"] = json!(false),
            "references" => manifest["derivatives"] = json!([]),
            _ => manifest["revision"] = json!(0),
        }
        fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let destination = temp.path().join("restored");
        assert!(Workspace::restore(&backup, &destination).is_err());
        assert!(!destination.exists());
    }
}
