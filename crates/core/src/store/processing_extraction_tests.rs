use super::*;
use crate::engines::parser::{ParseFailure, ParseLimitation};
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn fixture() -> (tempfile::TempDir, Workspace, String) {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut workspace = Workspace::open(temp.path().join("case")).unwrap();
    let source = workspace
        .import(
            "synthetic.pdf",
            b"%PDF-Synthetic canonical extraction test only.",
        )
        .unwrap();
    (temp, workspace, source)
}
fn output(bytes: &[u8]) -> ParseResult {
    ParseResult {
        protocol_version: 1,
        job_id: id(),
        content_sha256: hash(bytes),
        source_bytes: bytes.len() as u64,
        parser: LOCAL_FONT_PDF_PARSER.into(),
        media_type: "application/pdf".into(),
        status: ParseStatus::Partial,
        text: "Fixed synthetic unreviewed text; no worker ran.".into(),
        metadata: BTreeMap::new(),
        limitations: vec![
            ParseLimitation::NoSourceAnchors,
            ParseLimitation::EmbeddedDocumentsExcluded,
            ParseLimitation::OcrNotPerformed,
            ParseLimitation::FontSubstituted,
            ParseLimitation::FontCoverageUnverified,
        ],
        error: None,
    }
}
fn publish(workspace: &mut Workspace, source: &str) -> (JobTicket, ParseResult, ExtractionRecord) {
    workspace.queue_document_parse(source, &id()).unwrap();
    let prepared = workspace.claim_processing_job().unwrap().unwrap();
    let result = output(&prepared.bytes);
    let job = workspace
        .finish_document_job(&prepared.ticket, Ok(result.clone()))
        .unwrap();
    let record = workspace.extraction(&job.result_ids[0]).unwrap();
    (prepared.ticket, result, record)
}
fn raw(workspace: &Workspace, kind: &str, key: &str) -> String {
    workspace
        .conn
        .query_row(
            "SELECT body FROM records WHERE kind=? AND id=?",
            params![kind, key],
            |row| row.get(0),
        )
        .unwrap()
}
fn replace(workspace: &Workspace, kind: &str, key: &str, body: &str) {
    assert_eq!(
        workspace
            .conn
            .execute(
                "UPDATE records SET body=? WHERE kind=? AND id=?",
                params![body, kind, key]
            )
            .unwrap(),
        1
    );
}

#[test]
fn extraction_v2_publication_is_atomic_and_replay_preserves_unreviewed_font_outcomes() {
    let (_temp, mut w, source) = fixture();
    w.queue_document_parse(&source, &id()).unwrap();
    let prepared = w.claim_processing_job().unwrap().unwrap();
    let result = output(&prepared.bytes);
    let revision = w.revision().unwrap();
    w.conn.execute_batch("CREATE TRIGGER reject_v2_finish BEFORE UPDATE ON records WHEN NEW.kind='processing_job' AND json_extract(NEW.body,'$.state')='partial' BEGIN SELECT RAISE(ABORT,'synthetic publication fault'); END;").unwrap();
    assert!(w
        .finish_document_job(&prepared.ticket, Ok(result.clone()))
        .is_err());
    assert_eq!(w.revision().unwrap(), revision);
    assert!(all::<ExtractionRecord>(&w.conn, "extraction")
        .unwrap()
        .is_empty());
    assert_eq!(
        w.processing_job(&prepared.ticket.job_id).unwrap().state,
        ProcessingState::Running
    );
    w.conn
        .execute_batch("DROP TRIGGER reject_v2_finish")
        .unwrap();
    let job = w
        .finish_document_job(&prepared.ticket, Ok(result.clone()))
        .unwrap();
    assert_eq!(job.state, ProcessingState::Partial);
    let record = w.extraction(&job.result_ids[0]).unwrap();
    assert_eq!(record.schema_version, 2);
    assert_eq!(record.result.protocol_version, 1);
    let before = raw(&w, "extraction", &record.id);
    let revision = w.revision().unwrap();
    w.finish_document_job(&prepared.ticket, Ok(result)).unwrap();
    assert_eq!(w.revision().unwrap(), revision);
    assert_eq!(raw(&w, "extraction", &record.id), before);
    w.retry_processing_job(&job.id, 1, "Synthetic font repair review")
        .unwrap();
    let next = w.claim_processing_job().unwrap().unwrap();
    let mut failed = output(&next.bytes);
    failed.status = ParseStatus::Failed;
    failed.error = Some(ParseFailure::FontAssetUnavailable);
    failed.text.clear();
    let job = w.finish_document_job(&next.ticket, Ok(failed)).unwrap();
    assert_eq!(job.state, ProcessingState::Failed);
    let failure = w.extraction(job.result_ids.last().unwrap()).unwrap();
    assert_eq!(failure.schema_version, 2);
    assert_eq!(
        failure.result.error,
        Some(ParseFailure::FontAssetUnavailable)
    );
    assert!(failure.result.text.is_empty() && failure.result.metadata.is_empty());
    assert_eq!(raw(&w, "extraction", &record.id), before);
    assert_eq!(w.extraction(&record.id).unwrap().attempt, 1);
    assert!(w.view().unwrap().observations.is_empty());
}

#[test]
fn saved_v1_prior_attempt_remains_byte_identical_through_retry_reopen_backup_and_restore() {
    assert_legacy_extraction_schema();
    assert_eq!(
        hash(include_bytes!(
            "../../../../fixtures/processing/extraction-v1.json"
        )),
        "85eb5f3b62613d5e6dd25bdd598e6e95435a2f4a0269614cacdb7174af6583fb"
    );
    let saved: Value = serde_json::from_str(include_str!(
        "../../../../fixtures/processing/extraction-v1.json"
    ))
    .unwrap();
    let (temp, mut w, _) = fixture();
    let source = w
        .import(
            saved["original_name"].as_str().unwrap(),
            saved["original_utf8"].as_str().unwrap().as_bytes(),
        )
        .unwrap();
    let record: ExtractionRecord = serde_json::from_value(saved["extraction"].clone()).unwrap();
    let job: ProcessingJob = serde_json::from_value(saved["job"].clone()).unwrap();
    assert_eq!(source, record.result.content_sha256);
    for (kind, key, body) in [
        (
            "extraction",
            &record.id,
            saved["extraction_json"].as_str().unwrap(),
        ),
        (
            "processing_job",
            &job.id,
            saved["job_json"].as_str().unwrap(),
        ),
    ] {
        w.conn
            .execute(
                "INSERT INTO records(kind,id,body) VALUES(?,?,?)",
                params![kind, key, body],
            )
            .unwrap();
    }
    let before = raw(&w, "extraction", &record.id);
    assert_eq!(w.extraction(&record.id).unwrap().schema_version, 1);
    w.retry_processing_job(&job.id, 1, "Synthetic legacy retry")
        .unwrap();
    let next = w.claim_processing_job().unwrap().unwrap();
    let newer = w
        .finish_document_job(&next.ticket, Ok(output(&next.bytes)))
        .unwrap();
    let newest_id = newer.result_ids.last().unwrap();
    let newest_body = raw(&w, "extraction", newest_id);
    assert_eq!(w.extraction(&record.id).unwrap().schema_version, 1);
    assert_eq!(raw(&w, "extraction", &record.id), before);
    let path = w.root.clone();
    drop(w);
    let mut reopened = Workspace::open(&path).unwrap();
    assert_eq!(raw(&reopened, "extraction", &record.id), before);
    assert_eq!(
        reopened.extraction(&record.id).unwrap().result.parser,
        "pdfbox-3.0.8"
    );
    let backup = reopened.backup().unwrap();
    let restored = Workspace::restore(&backup, &temp.path().join("restored")).unwrap();
    assert_eq!(raw(&restored, "extraction", &record.id), before);
    assert_eq!(restored.extraction(&record.id).unwrap().schema_version, 1);
    assert_eq!(restored.extraction(newest_id).unwrap().schema_version, 2);
    assert_eq!(raw(&restored, "extraction", newest_id), newest_body);
    assert!(restored.view().unwrap().observations.is_empty());
}

#[test]
fn extraction_inspection_rejects_forged_versions_result_digests_and_bindings_without_rewrite() {
    let (_temp, mut w, source) = fixture();
    let (_, _, record) = publish(&mut w, &source);
    let body = raw(&w, "extraction", &record.id);
    let revision = w.revision().unwrap();
    for case in 0..10 {
        let mut altered: Value = serde_json::from_str(&body).unwrap();
        match case {
            0 => altered["schema_version"] = json!(1),
            1 => altered["schema_version"] = json!(3),
            2 => altered["id"] = json!("0".repeat(64)),
            3 => altered["attempt"] = json!(0),
            4 => altered["job_id"] = json!(id()),
            5 => altered["result"]["text"] = json!("tampered text"),
            6 => altered["result_sha256"] = json!("0".repeat(64)),
            7 => altered["input"]["evidence_id"] = json!("0".repeat(64)),
            8 => altered["result"]["content_sha256"] = json!("0".repeat(64)),
            _ => altered["input"]["bytes"] = json!(999),
        }
        let encoded = serde_json::to_string(&altered).unwrap();
        replace(&w, "extraction", &record.id, &encoded);
        assert!(w.extraction(&record.id).is_err(), "case {case}");
        assert_eq!(raw(&w, "extraction", &record.id), encoded);
        assert_eq!(w.revision().unwrap(), revision);
    }
    replace(&w, "extraction", &record.id, &body);
    assert!(w.extraction(&record.id).is_ok());
    assert!(w.extraction("invalid").is_err());
    assert!(w.extraction(&"0".repeat(64)).is_err());
}

#[test]
fn extraction_inspection_refuses_whole_valid_record_or_owner_substitution_and_source_loss() {
    let (_temp, mut w, source) = fixture();
    let (_, _, a) = publish(&mut w, &source);
    let (_, _, b) = publish(&mut w, &source);
    let original = raw(&w, "extraction", &a.id);
    replace(&w, "extraction", &a.id, &raw(&w, "extraction", &b.id));
    assert!(w.extraction(&a.id).is_err());
    assert!(w.extraction(&b.id).is_ok());
    replace(&w, "extraction", &a.id, &original);
    let owner = raw(&w, "processing_job", &a.job_id);
    replace(
        &w,
        "processing_job",
        &a.job_id,
        &raw(&w, "processing_job", &b.job_id),
    );
    assert!(w.extraction(&a.id).is_err());
    replace(&w, "processing_job", &a.job_id, &owner);
    let other = w.import("other.txt", b"Other synthetic original").unwrap();
    let evidence = raw(&w, "evidence", &source);
    replace(&w, "evidence", &source, &raw(&w, "evidence", &other));
    assert!(w.extraction(&a.id).is_err());
    replace(&w, "evidence", &source, &evidence);
    let file = w.root.join("originals").join(&source);
    fs::remove_file(&file).unwrap();
    assert!(w.extraction(&a.id).is_err());
    fs::write(&file, b"Altered synthetic original").unwrap();
    assert!(w.extraction(&a.id).is_err());
    assert_eq!(raw(&w, "extraction", &a.id), original);
}

#[test]
fn extraction_inspection_preflights_record_and_owner_before_decoding_oversized_bodies() {
    let (_temp, mut w, source) = fixture();
    let (_, _, record) = publish(&mut w, &source);
    for (kind, key) in [
        ("extraction", &record.id),
        ("processing_job", &record.job_id),
    ] {
        let before = raw(&w, kind, key);
        replace(
            &w,
            kind,
            key,
            &serde_json::to_string(&"x".repeat(MAX_RESULT_BYTES as usize + 1)).unwrap(),
        );
        let error = w.extraction(&record.id).unwrap_err().to_string();
        assert!(error.contains("retained record limit"), "{error}");
        replace(&w, kind, key, &before);
    }
    assert!(w.extraction(&record.id).is_ok());
}

#[test]
fn extraction_inspection_pins_record_owner_and_source_in_one_read_snapshot() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let (_temp, mut w, source) = fixture();
    let (_, _, record) = publish(&mut w, &source);
    let writer = Workspace::open(&w.root).unwrap();
    w.conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    let reads = Arc::new(AtomicUsize::new(0));
    let changed = Arc::new(AtomicUsize::new(0));
    let observed = reads.clone();
    let committed = changed.clone();
    let job_id = record.job_id.clone();
    w.conn.authorizer(Some(move |context:AuthContext<'_>| {
        if matches!(context.action,AuthAction::Read{table_name:"records",column_name:"body",..})
            && observed.fetch_add(1,Ordering::SeqCst)==2 {
            // A real second connection corrupts ownership after the extraction's
            // checked body has been read, before its owner is queried.
            writer.conn.execute("UPDATE records SET body=json_set(body,'$.result_ids',json('[]')) WHERE kind='processing_job' AND id=?",[&job_id]).unwrap();
            committed.fetch_add(1,Ordering::SeqCst);
        }
        Authorization::Allow
    }));
    assert_eq!(w.extraction(&record.id).unwrap().id, record.id);
    assert_eq!(changed.load(Ordering::SeqCst), 1);
    w.conn
        .authorizer(None::<fn(AuthContext<'_>) -> Authorization>);
    assert!(w.extraction(&record.id).is_err());
}

#[test]
fn extraction_record_requires_a_bounded_timestamp_and_canonical_owner_uuid() {
    let (_temp, mut w, source) = fixture();
    let (_, _, record) = publish(&mut w, &source);
    for timestamp in [String::new(), "not-a-timestamp".into(), "x".repeat(65)] {
        let mut invalid = record.clone();
        invalid.created_at = timestamp;
        assert!(validate_record(&invalid, &invalid.id).is_err());
    }
    for owner in [
        "AAAAAAAA-1111-4111-8111-111111111111",
        "aaaaaaaa111141118111111111111111",
    ] {
        let mut invalid = record.clone();
        invalid.job_id = owner.into();
        invalid.id = hash(format!("{}:{}", invalid.job_id, invalid.attempt).as_bytes());
        assert!(validate_record(&invalid, &invalid.id).is_err());
    }
}

#[test]
fn both_extraction_versions_reject_duplicate_metadata_even_when_last_value_digest_matches() {
    let (_temp, mut w, source) = fixture();
    let (_, _, record) = publish(&mut w, &source);
    for version in [1, 2] {
        let mut altered = record.clone();
        altered.schema_version = version;
        if version == 1 {
            altered.result.parser = "pdfbox-3.0.8".into();
            altered.result.limitations.retain(|limit| {
                !matches!(
                    limit,
                    ParseLimitation::FontSubstituted | ParseLimitation::FontCoverageUnverified
                )
            });
        }
        altered
            .result
            .metadata
            .insert("title".into(), vec!["last".into()]);
        altered.result_sha256 = hash(&serde_json::to_vec(&altered.result).unwrap());
        let valid = serde_json::to_string(&altered).unwrap();
        replace(&w, "extraction", &record.id, &valid);
        assert_eq!(w.extraction(&record.id).unwrap().schema_version, version);
        let duplicate = valid.replace(
            "\"title\":[\"last\"]",
            "\"title\":[\"discarded\"],\"title\":[\"last\"]",
        );
        assert_ne!(duplicate, valid);
        replace(&w, "extraction", &record.id, &duplicate);
        let error = w.extraction(&record.id).unwrap_err().to_string();
        assert!(
            error.contains("Duplicate extraction metadata key"),
            "{error}"
        );
        assert_eq!(raw(&w, "extraction", &record.id), duplicate);
    }
}
