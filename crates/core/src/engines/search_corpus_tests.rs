use super::*;

fn evidence(id: &str, text: Option<&str>) -> Evidence {
    Evidence {
        id: id.into(),
        name: "Synthetic \"name\" \\ Δ\n".into(),
        sha256: id.into(),
        bytes: 0,
        media_type: "text/plain".into(),
        origin_group: "synthetic".into(),
        imported_at: "synthetic".into(),
        extraction_status: "complete".into(),
        text: text.map(str::to_owned),
        acquisitions: vec![],
    }
}

#[test]
fn search_corpus_exact_legacy_manifest_order_escaping_and_null_empty_semantics() {
    for rows in [
        vec![],
        vec![evidence("none", None)],
        vec![
            evidence("last-lexical", Some("\0\u{1f}\t\r\n\\\" é 漢 😀 <script>")),
            evidence("metadata-only", None),
            evidence("first-lexical", Some("")),
        ],
    ] {
        let expected = serde_json::to_vec(&serde_json::json!({"workspace_revision":31,
            "documents":rows.iter().filter_map(|e|e.text.as_ref().map(|text|
                serde_json::json!({"id":e.id,"name":e.name,"text":text}))).collect::<Vec<_>>() }))
        .unwrap();
        let mut builder = CorpusBuilder::new(31).unwrap();
        for row in &rows {
            builder.push(row).unwrap();
        }
        let corpus = builder.finish().unwrap();
        assert_eq!(corpus.manifest(), expected);
        assert_eq!(
            corpus.document_count(),
            rows.iter().filter(|r| r.text.is_some()).count() as u64
        );
        for row in rows {
            assert!(corpus.knows(&row.id));
        }
        assert!(!corpus.knows("unknown"));
    }
}

#[test]
fn search_corpus_document_and_total_row_limits_are_separate_and_poison_failure() {
    let mut builder = CorpusBuilder::new(1).unwrap();
    let empty = evidence("text", Some(""));
    for _ in 0..MAX_DOCUMENTS {
        builder.push(&empty).unwrap();
    }
    builder.push(&evidence("none", None)).unwrap();
    assert_eq!(builder.documents, MAX_DOCUMENTS);
    assert!(builder.push(&empty).is_err());
    assert!(builder.finish().is_err());

    let mut builder = CorpusBuilder::new(1).unwrap();
    let none = evidence("none", None);
    for _ in 0..MAX_EVIDENCE_ROWS {
        builder.push(&none).unwrap();
    }
    assert!(builder.push(&none).is_err());
    assert!(builder.finish().is_err());
}

#[test]
fn search_corpus_writer_refuses_before_growth_and_counts_escaped_json_bytes() {
    let mut writer = LimitedJson {
        bytes: vec![b'x'; MAX_MANIFEST_BYTES - 1],
    };
    let capacity = writer.bytes.capacity();
    assert!(writer.write_all(b"ab").is_err());
    assert_eq!(writer.bytes.len(), MAX_MANIFEST_BYTES - 1);
    assert_eq!(writer.bytes.capacity(), capacity);
    writer.write_all(b"a").unwrap();
    assert_eq!(writer.bytes.len(), MAX_MANIFEST_BYTES);

    let text = "\0".repeat(3 * 1024 * 1024);
    let mut builder = CorpusBuilder::new(1).unwrap();
    assert!(builder.push(&evidence("source", Some(&text))).is_err());
    assert!(builder.writer.bytes.len() <= MAX_MANIFEST_BYTES);
    assert!(builder.finish().is_err());
}

#[test]
fn search_corpus_keeps_exact_legacy_known_ids_without_authorizing_unknown_hits() {
    use crate::{engines::Runtime, policy::WorkerOperation};
    for (hit_id, accepted) in [("text", true), ("metadata-only", true), ("unknown", false)] {
        let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let runtime = Runtime {
            root: root.path().join("not-executed"),
        };
        let mut builder = CorpusBuilder::new(7).unwrap();
        builder.push(&evidence("text", Some("synthetic"))).unwrap();
        builder.push(&evidence("metadata-only", None)).unwrap();
        let corpus = builder.finish().unwrap();
        let result = runtime.search_corpus_with(
            &root.path().join("cache"),
            &corpus,
            "needle",
            |_, op, _| {
                Ok(match op {
                    WorkerOperation::Index => br#"{"workspace_revision":7,"indexed":1}"#.to_vec(),
                    WorkerOperation::Search => {
                        serde_json::to_vec(&serde_json::json!({"workspace_revision":"7",
                    "hits":[{"id":hit_id,"name":"legacy name","score":1.0}],"total":1}))?
                    }
                    _ => panic!("unexpected operation"),
                })
            },
        );
        assert_eq!(result.result.is_ok(), accepted);
        assert!(!root.path().join("cache/execution-intent.v1.json").exists());
    }
}

#[cfg(not(target_os = "macos"))]
#[test]
fn search_corpus_unsupported_host_refuses_without_cache_creation() {
    use crate::{engines::Runtime, Error};
    let root = tempfile::tempdir().unwrap();
    let runtime = Runtime {
        root: root.path().join("not-executed"),
    };
    let corpus = CorpusBuilder::new(1).unwrap().finish().unwrap();
    assert!(matches!(
        runtime
            .search_corpus_completed(&root.path().join("cache"), &corpus, "needle")
            .result,
        Err(Error::Blocked(_))
    ));
    assert!(matches!(
        runtime.search(&root.path().join("cache"), 1, &[], "needle"),
        Err(Error::Blocked(_))
    ));
    assert!(!root.path().join("cache").exists());
}
