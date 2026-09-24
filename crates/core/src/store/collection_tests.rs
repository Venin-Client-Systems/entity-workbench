//! Synthetic canonical-store tests; no network or worker runtime is involved.
use super::*;
use crate::collection_receipt::{AcquisitionMode, RequestPurpose};
use tempfile::TempDir;

fn workspace() -> (TempDir, Workspace) {
    let temp = TempDir::new_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let workspace = Workspace::open(temp.path().join("case")).unwrap();
    (temp, workspace)
}
fn scripted(responses: &[(&str, u16, &str, &[u8], bool)]) -> CollectionResult {
    let at = "2026-09-24T00:00:00Z".to_owned();
    let mut result = CollectionResult {
        pages: vec![],
        requests: responses.len() as u32,
        state: JobState::SuccessfulNoResults,
        notes: vec![],
        mode: AcquisitionMode::Synthetic,
        started_at: at.clone(),
        ended_at: at.clone(),
        elapsed_milliseconds: 0,
        trace: vec![],
        responses: vec![],
    };
    for (index, (path, status, media, bytes, searchable)) in responses.iter().enumerate() {
        let url = format!("https://example.com{path}");
        result.trace.push(RequestReceipt {
            sequence: index as u32,
            url: url.clone(),
            method: "GET".into(),
            purpose: if *path == "/robots.txt" {
                RequestPurpose::AccessReview
            } else {
                RequestPurpose::Seed
            },
            parent_request: None,
            hop: 0,
            started_at: at.clone(),
            ended_at: at.clone(),
            outcome: FetchOutcome::Fetched,
            http_status: Some(*status),
            media_type: Some((*media).into()),
            redirect_url: None,
            body_sha256: Some(hash(bytes)),
            body_bytes: Some(bytes.len() as u64),
            original_evidence_id: None,
        });
        result.responses.push(ResponseBody {
            sequence: index as u32,
            bytes: bytes.to_vec(),
        });
        if *searchable {
            result.pages.push(Page {
                request_sequence: index as u32,
                url,
                bytes: bytes.to_vec(),
                text: String::from_utf8(bytes.to_vec()).unwrap(),
            });
            result.state = JobState::Successful;
        }
    }
    result
}
fn finish(workspace: &mut Workspace, result: CollectionResult) -> String {
    let seeds = result
        .trace
        .iter()
        .filter(|r| r.purpose == RequestPurpose::Seed)
        .map(|r| r.url.clone())
        .collect::<Vec<_>>();
    let seeds = if seeds.is_empty() {
        vec!["https://example.com/start".into()]
    } else {
        seeds
    };
    let (job, revision) = workspace.start_collection(seeds, 2, 50, 600).unwrap();
    let key = job.id.clone();
    workspace.finish_collection(job, revision, result).unwrap();
    key
}

#[test]
fn empty_and_unsupported_responses_survive_without_search_promotion() {
    let (_temp, mut w) = workspace();
    let key = finish(
        &mut w,
        scripted(&[
            ("/robots.txt", 404, "text/plain", b"", false),
            ("/start", 204, "text/html", b"", false),
            ("/pdf", 200, "application/pdf", b"synthetic-pdf", false),
        ]),
    );
    let receipt = w.collection_receipt(&key).unwrap();
    assert!(receipt.retention_complete);
    assert_eq!(receipt.requests.len(), 3);
    assert_eq!(receipt.mode, AcquisitionMode::Synthetic);
    let evidence = w.view().unwrap().evidence;
    assert_eq!(evidence.len(), 2);
    assert!(evidence
        .iter()
        .all(|e| e.text.is_none() && e.extraction_status == "acquisition_only"));
    let empty = evidence.iter().find(|e| e.bytes == 0).unwrap();
    assert_eq!(empty.acquisitions.len(), 2);
    assert!(empty
        .acquisitions
        .iter()
        .all(|a| a.retrieved_at == "2026-09-24T00:00:00Z"));
    assert!(w.import("empty.txt", b"").is_err());
}

#[test]
fn identical_body_deduplicates_original_but_preserves_acquisition_context() {
    for indexed_first in [true, false] {
        let (_temp, mut w) = workspace();
        let body = b"Fictional shared source";
        let indexed = scripted(&[("/start", 200, "text/plain", body, true)]);
        let opaque = scripted(&[
            ("/robots.txt", 404, "text/plain", body, false),
            ("/error", 500, "application/octet-stream", body, false),
        ]);
        let (first, second) = if indexed_first {
            (indexed, opaque)
        } else {
            (opaque, indexed)
        };
        let a = finish(&mut w, first);
        let b = finish(&mut w, second);
        let evidence = w.view().unwrap().evidence;
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].acquisitions.len(), 3);
        assert_eq!(evidence[0].text.as_deref(), Some("Fictional shared source"));
        assert_eq!(evidence[0].origin_group, hash(body));
        assert!(w.collection_receipt(&a).is_ok());
        assert!(w.collection_receipt(&b).is_ok());
    }
}

#[test]
fn document_import_and_response_retention_preserve_each_others_history() {
    for imported_first in [true, false] {
        let (_temp, mut w) = workspace();
        let body = b"Fictional imported source";
        if imported_first {
            w.import("source.txt", body).unwrap();
        }
        let key = finish(
            &mut w,
            scripted(&[("/start", 500, "text/plain", body, false)]),
        );
        if !imported_first {
            w.import("source.txt", body).unwrap();
        }
        let evidence = w.view().unwrap().evidence;
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].name, "source.txt");
        assert_eq!(
            evidence[0].text.as_deref(),
            Some("Fictional imported source")
        );
        assert_eq!(evidence[0].acquisitions.len(), 1);
        w.collection_receipt(&key).unwrap();
    }
}

#[test]
fn retention_failure_cannot_publish_complete_receipt_or_export() {
    let (_temp, mut w) = workspace();
    let body = b"synthetic collision";
    fs::write(w.root.join("originals").join(hash(body)), b"corrupt").unwrap();
    let key = finish(
        &mut w,
        scripted(&[
            ("/robots.txt", 404, "text/plain", b"", false),
            ("/start", 200, "text/plain", body, true),
        ]),
    );
    let receipt = w.collection_receipt(&key).unwrap();
    assert!(!receipt.retention_complete);
    assert!(matches!(receipt.state, JobState::Failed));
    assert!(receipt.requests[0].original_evidence_id.is_some());
    assert!(receipt.requests[1].original_evidence_id.is_none());
    assert!(w.export_collection(&key).is_err());
}

#[test]
fn failed_final_transaction_leaves_job_unfinished_and_no_fabricated_receipt() {
    let (_temp, mut w) = workspace();
    let (job, revision) = w
        .start_collection(vec!["https://example.com/start".into()], 0, 2, 30)
        .unwrap();
    let key = job.id.clone();
    w.conn.execute_batch("CREATE TRIGGER fail_finish BEFORE UPDATE ON records WHEN NEW.kind='job' BEGIN SELECT RAISE(ABORT, 'synthetic publication failure'); END;").unwrap();
    assert!(w
        .finish_collection(
            job,
            revision,
            scripted(&[("/start", 200, "text/plain", b"Fictional source", true)])
        )
        .is_err());
    let job: CollectionJob = get(&w.conn, "job", &key).unwrap();
    assert!(matches!(job.state, JobState::Running));
    assert!(matches!(w.collection_receipt(&key), Err(Error::Blocked(_))));
    assert_eq!(w.view().unwrap().evidence.len(), 1);
}

#[test]
fn interrupted_and_legacy_jobs_do_not_invent_request_receipts() {
    let (_temp, mut w) = workspace();
    let (mut job, _) = w
        .start_collection(vec!["https://example.com/start".into()], 0, 2, 30)
        .unwrap();
    for state in [JobState::Running, JobState::Successful] {
        job.state = state;
        put(&w.conn, "job", &job.id, &job).unwrap();
        assert!(matches!(
            w.collection_receipt(&job.id),
            Err(Error::Blocked(_))
        ));
    }
}

#[test]
fn backup_restores_receipts_and_nonindexed_originals_and_rejects_corruption() {
    let (temp, mut w) = workspace();
    let key = finish(
        &mut w,
        scripted(&[
            ("/robots.txt", 404, "text/plain", b"", false),
            ("/start", 500, "text/plain", b"synthetic error", false),
        ]),
    );
    let before = serde_json::to_value(w.collection_receipt(&key).unwrap()).unwrap();
    let backup = w.backup().unwrap();
    let restored = Workspace::restore(&backup, &temp.path().join("restored")).unwrap();
    assert_eq!(
        before,
        serde_json::to_value(restored.collection_receipt(&key).unwrap()).unwrap()
    );
    assert!(restored
        .view()
        .unwrap()
        .evidence
        .iter()
        .all(|e| e.text.is_none()));
    let original = w.root.join("originals").join(hash(b"synthetic error"));
    fs::remove_file(original).unwrap();
    assert!(w.backup().is_err());
    assert!(w.export_collection(&key).is_err());
}

#[test]
fn exports_are_distinct_hash_bound_snapshots_that_do_not_change_on_later_import() {
    let (_temp, mut w) = workspace();
    let key = finish(
        &mut w,
        scripted(&[("/start", 200, "text/plain", b"Fictional source", true)]),
    );
    let first = w
        .dispatch(Command::ExportCollection {
            job_id: key.clone(),
        })
        .unwrap();
    let path = w.root.join(first["path"].as_str().unwrap());
    let bytes = fs::read(&path).unwrap();
    assert_eq!(hash(&bytes), first["sha256"].as_str().unwrap());
    let manifest: Value = serde_json::from_slice(&bytes).unwrap();
    let directory = path.parent().unwrap();
    for original in manifest["originals"].as_array().unwrap() {
        assert_eq!(
            hash(&fs::read(directory.join(original["path"].as_str().unwrap())).unwrap()),
            original["sha256"].as_str().unwrap()
        );
    }
    w.import("later.txt", b"Another fictional source").unwrap();
    let second = w.export_collection(&key).unwrap();
    assert_ne!(first["path"], second["path"]);
    assert_eq!(fs::read(path).unwrap(), bytes);
    w.dispatch(Command::InspectCollection { job_id: key })
        .unwrap();
}

#[test]
fn malformed_receipts_fail_closed_and_old_workspace_contract_stays_at_three() {
    let (_temp, mut w) = workspace();
    let key = finish(
        &mut w,
        scripted(&[("/start", 200, "text/plain", b"Fictional source", true)]),
    );
    let receipt = w.collection_receipt(&key).unwrap();
    for mutate in [0, 1, 2, 3, 4, 5] {
        let mut invalid = receipt.clone();
        match mutate {
            0 => invalid.requests[0].original_evidence_id = Some("../escape".into()),
            1 => invalid.requests_used += 1,
            2 => invalid.requests[0].url = "https://outside.example/start".into(),
            3 => invalid.requests[0].ended_at = "2026-09-23T00:00:00Z".into(),
            4 => invalid.schema_version = 99,
            _ => {
                invalid.requests[0].purpose = RequestPurpose::Link;
                invalid.requests[0].parent_request = Some(0);
            }
        }
        assert!(invalid.validate().is_err());
    }
    assert_eq!(
        w.conn
            .pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        3
    );
    let mut value = serde_json::to_value(&receipt).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<CollectionReceipt>(value).is_err());
}

#[cfg(unix)]
#[test]
fn symbolic_export_directory_is_rejected_without_touching_destination() {
    use std::os::unix::fs::symlink;
    let (temp, mut w) = workspace();
    let key = finish(
        &mut w,
        scripted(&[("/start", 200, "text/plain", b"Fictional source", true)]),
    );
    let outside = temp.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("sentinel"), b"preserve").unwrap();
    fs::remove_dir(w.root.join("exports")).unwrap();
    symlink(&outside, w.root.join("exports")).unwrap();
    assert!(w.export_collection(&key).is_err());
    assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"preserve");
    assert_eq!(fs::read_dir(outside).unwrap().count(), 1);
}

#[test]
fn receipts_reject_future_dates_and_make_observed_time_overruns_explicit() {
    let (_temp, mut w) = workspace();
    let key = finish(
        &mut w,
        scripted(&[("/start", 200, "text/plain", b"Fictional source", true)]),
    );
    let mut receipt = w.collection_receipt(&key).unwrap();
    let mut future = receipt.clone();
    future.started_at = "9999-01-01T00:00:00Z".into();
    future.ended_at = future.started_at.clone();
    future.requests[0].started_at = future.started_at.clone();
    future.requests[0].ended_at = future.started_at.clone();
    assert!(future.validate().is_err());
    receipt.max_seconds = 1;
    receipt.ended_at = "2026-09-24T00:00:02Z".into();
    receipt.elapsed_milliseconds = 2001;
    assert!(receipt.validate().is_err());
    receipt.time_limit_exceeded = true;
    assert!(receipt.validate().is_err());
    receipt.state = JobState::QuotaExhausted;
    receipt.validate().unwrap();
    receipt.elapsed_milliseconds = 5000;
    assert!(receipt.validate().is_err());
}

#[cfg(windows)]
#[test]
fn windows_export_junction_is_rejected_without_touching_destination() {
    let (temp, mut w) = workspace();
    let key = finish(
        &mut w,
        scripted(&[("/start", 200, "text/plain", b"Fictional source", true)]),
    );
    let outside = temp.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("sentinel"), b"preserve").unwrap();
    let exports = w.root.join("exports");
    fs::remove_dir(&exports).unwrap();
    let status = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&exports)
        .arg(&outside)
        .status()
        .unwrap();
    assert!(
        status.success(),
        "Junction fixture must be created for the negative test"
    );
    assert!(w.export_collection(&key).is_err());
    assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"preserve");
    assert_eq!(fs::read_dir(outside).unwrap().count(), 1);
    fs::remove_dir(exports).unwrap();
}

#[cfg(unix)]
#[test]
fn changed_original_directory_and_dangling_export_link_fail_closed() {
    use std::os::unix::fs::symlink;
    let (temp, mut w) = workspace();
    let key = finish(
        &mut w,
        scripted(&[("/start", 200, "text/plain", b"Fictional source", true)]),
    );
    let originals = w.root.join("originals");
    let moved = temp.path().join("moved");
    fs::rename(&originals, &moved).unwrap();
    symlink(&moved, &originals).unwrap();
    assert!(w.collection_receipt(&key).is_err());
    assert!(w.export_collection(&key).is_err());
    fs::remove_file(&originals).unwrap();
    fs::rename(moved, &originals).unwrap();
    let exports = w.root.join("exports");
    fs::remove_dir(&exports).unwrap();
    symlink(temp.path().join("absent"), &exports).unwrap();
    assert!(w.export_collection(&key).is_err());
    assert!(!temp.path().join("absent").exists());
}
