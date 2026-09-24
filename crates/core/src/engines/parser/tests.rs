use super::*;
fn valid() -> ParseResult {
    ParseResult {
        protocol_version: 1,
        job_id: uuid::Uuid::new_v4().to_string(),
        content_sha256: "0".repeat(64),
        source_bytes: 8,
        parser: "utf8-v1".into(),
        media_type: "text/plain".into(),
        status: ParseStatus::Complete,
        text: "synthetic".into(),
        metadata: BTreeMap::new(),
        limitations: vec![ParseLimitation::NoSourceAnchors],
        error: None,
    }
}
#[test]
fn parser_results_require_source_binding_and_honest_status() {
    let source = valid();
    validate_result(&source, &"0".repeat(64), 8).unwrap();
    for change in 0..9 {
        let mut result = source.clone();
        match change {
            0 => result.protocol_version = 2,
            1 => result.content_sha256 = "1".repeat(64),
            2 => result.source_bytes += 1,
            3 => result.parser = "downloaded-plugin".into(),
            4 => result.text = "x".repeat(MAX_TEXT_BYTES + 1),
            5 => result.limitations.clear(),
            6 => result.status = ParseStatus::Unsupported,
            7 => result.error = Some(ParseFailure::MalformedDocument),
            _ => result
                .metadata
                .insert("unbounded".into(), vec!["x".repeat(4097)])
                .map(|_| ())
                .unwrap_or(()),
        }
        assert!(
            validate_result(&result, &"0".repeat(64), 8).is_err(),
            "case {change}"
        );
    }
    let mut wire = serde_json::to_value(source).unwrap();
    wire["page_anchor"] = serde_json::json!(1);
    assert!(serde_json::from_value::<ParseResult>(wire).is_err());
}
#[test]
fn cancelled_parser_never_launches_or_stages_input() {
    let root = tempfile::tempdir().unwrap();
    let scratch = root.path().join("scratch");
    let token = CancellationToken::default();
    token.clone().cancel();
    let runtime = Runtime {
        root: root.path().join("missing"),
    };
    assert!(runtime
        .parse_with_cancel(&scratch, b"synthetic", &token)
        .is_err());
    assert!(!scratch.exists());
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires staged parser and search runtimes; run scripts/test_parser_workers.py"]
fn native_parser_extracts_supported_formats_and_preserves_limitations() {
    use sha2::{Digest, Sha256};
    use std::{fs, path::PathBuf};
    let root = tempfile::tempdir().unwrap();
    let runtime = Runtime {
        root: PathBuf::from(std::env::var_os("WORKBENCH_TEST_RUNTIME").expect("staged runtime")),
    };
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/parser");
    let mut jobs = std::collections::HashSet::new();
    for (name, status) in [
        ("notice.txt", ParseStatus::Complete),
        ("notice.pdf", ParseStatus::Partial),
        ("notice.docx", ParseStatus::Partial),
    ] {
        let original = fs::read(fixtures.join(name)).unwrap();
        let result = runtime.parse(root.path(), &original).unwrap();
        assert_eq!(result.status, status, "{name}: {result:?}");
        assert!(result.text.contains("Rowan Ellis"), "{name}: {result:?}");
        assert_eq!(
            result.content_sha256,
            format!("{:x}", Sha256::digest(&original))
        );
        assert!(jobs.insert(result.job_id));
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }
    let result = runtime
        .parse(
            root.path(),
            &fs::read(fixtures.join("no-text.pdf")).unwrap(),
        )
        .unwrap();
    assert_eq!(result.status, ParseStatus::Partial);
    assert!(result.text.trim().is_empty());
    assert!(result
        .limitations
        .contains(&ParseLimitation::OcrNotPerformed));
    let unsupported = runtime.parse(root.path(), &[0, 255, 0, 128]).unwrap();
    assert_eq!(unsupported.status, ParseStatus::Unsupported);
    let malformed = runtime
        .parse(root.path(), b"%PDF-broken synthetic input")
        .unwrap();
    assert_eq!(malformed.status, ParseStatus::Failed);
    assert_eq!(malformed.error, Some(ParseFailure::MalformedDocument));
    let traversal = runtime
        .parse(
            root.path(),
            &fs::read(fixtures.join("traversal.zip")).unwrap(),
        )
        .unwrap();
    assert_eq!(traversal.status, ParseStatus::Failed);
    assert_eq!(traversal.error, Some(ParseFailure::ArchiveLimits));
    let text = runtime.parse(root.path(), &vec![b'x'; 130_000]).unwrap();
    assert_eq!(text.status, ParseStatus::Partial);
    assert!(text.limitations.contains(&ParseLimitation::TextLimit));
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}
