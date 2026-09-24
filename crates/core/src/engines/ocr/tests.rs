use super::*;

const IMAGE: &[u8] = include_bytes!("../../../../../fixtures/ocr/synthetic.pgm");
#[test]
fn raster_rejects_expansion_ambiguity_and_unsupported_encodings() {
    assert_eq!(raster_dimensions(IMAGE).unwrap(), (1200, 230));
    for bad in [
        b"P5\n8193 1\n255\n".as_slice(),
        b"P5\n4000 4000\n255\n",
        b"P5\n1 1\n65535\n00",
        b"P5\n1 1\n255\n00",
        b"P5\n01 1\n255\n0",
        b"P5\n#comment\n1 1\n255\n0",
        b"P6\n1 1\n255\n000",
        b"P5\n0 1\n255\n",
        b"P5\n1 1\n255\n",
    ] {
        assert!(raster_dimensions(bad).is_err());
    }
    assert_eq!(raster_dimensions(b"P5\n1 1\n255\n\xff").unwrap(), (1, 1));
    let mut concatenated = IMAGE.to_vec();
    concatenated.extend_from_slice(IMAGE);
    assert!(raster_dimensions(&concatenated).is_err());
}
#[test]
fn missing_or_cancelled_ocr_fails_without_staging_input() {
    let root = tempfile::tempdir().unwrap();
    let runtime = Runtime {
        root: root.path().join("missing"),
    };
    let scratch = root.path().join("scratch");
    assert!(matches!(
        runtime.ocr(&scratch, IMAGE),
        Err(Error::Blocked(_))
    ));
    assert!(!scratch.exists());
    let token = CancellationToken::default();
    token.cancel();
    assert!(matches!(
        runtime.ocr_with_cancel(&scratch, IMAGE, &token),
        Err(Error::Blocked(_))
    ));
    assert!(!scratch.exists());
}
#[test]
fn ocr_acceptance_preserves_raster_binding_and_uncertainty() {
    let mut result = OcrResult {
        protocol_version: 1,
        job_id: uuid::Uuid::new_v4().to_string(),
        raster_sha256: digest(IMAGE),
        raster_bytes: IMAGE.len() as u64,
        width: 1200,
        height: 230,
        engine: "tesseract-5.5.2".into(),
        language: "eng".into(),
        model_sha256: MODEL_SHA256.into(),
        runtime_manifest_sha256: "0".repeat(64),
        status: OcrStatus::Recognized,
        text: "Unreviewed synthetic text".into(),
        limitations: vec![
            OcrLimitation::UnreviewedRecognition,
            OcrLimitation::NoWordRegions,
            OcrLimitation::NoOriginalDocumentMapping,
        ],
    };
    validate_result(&result, IMAGE).unwrap();
    result.width += 1;
    assert!(validate_result(&result, IMAGE).is_err());
    result.width -= 1;
    result.text = " ".into();
    assert!(validate_result(&result, IMAGE).is_err());
    result.status = OcrStatus::NoTextRecognized;
    validate_result(&result, IMAGE).unwrap();
    result.limitations.pop();
    assert!(validate_result(&result, IMAGE).is_err());
    result
        .limitations
        .push(OcrLimitation::NoOriginalDocumentMapping);
    result.text = "a".repeat(128_001);
    result.status = OcrStatus::Recognized;
    assert!(validate_result(&result, IMAGE).is_err());
    result.text = "invalid\0text".into();
    assert!(validate_result(&result, IMAGE).is_err());
}

#[test]
#[cfg(target_os = "macos")]
#[ignore = "requires staged native OCR runtime"]
fn native_ocr_reads_only_app_local_runtime_and_reports_empty_image() {
    let runtime = Runtime {
        root: std::env::var_os("WORKBENCH_TEST_OCR_RUNTIME")
            .expect("Staged OCR required")
            .into(),
    };
    let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let before = digest(IMAGE);
    let result = runtime.ocr(root.path(), IMAGE).unwrap();
    assert_eq!(result.status, OcrStatus::Recognized);
    assert!(
        result.text.contains("SYNTHETIC OCR TEST"),
        "Expected synthetic text was not recognized"
    );
    assert!(result.text.contains("REFERENCE 0042 AMOUNT 123.45"));
    assert_eq!(result.raster_sha256, before);
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    let mut blank = b"P5\n200 100\n255\n".to_vec();
    blank.extend(vec![255; 20_000]);
    let empty = runtime.ocr(root.path(), &blank).unwrap();
    assert_eq!(empty.status, OcrStatus::NoTextRecognized);
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
#[cfg(target_os = "macos")]
fn runtime_manifest_rejects_unreviewed_model_bytes() {
    use std::{collections::BTreeMap, fs};
    let root = tempfile::tempdir().unwrap();
    let bundle = root.path().join("ocr");
    fs::create_dir(&bundle).unwrap();
    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x86_64"
    };
    let mut files = BTreeMap::new();
    for name in ["bin/tesseract", "tessdata/eng.traineddata", "NOTICE.json"] {
        let path = bundle.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"synthetic").unwrap();
        files.insert(
            name,
            serde_json::json!({"bytes": 9, "sha256": digest(b"synthetic")}),
        );
    }
    let mut manifest = serde_json::json!({"schema_version": 1,"os":"macos","architecture":arch,"engine":"tesseract-5.5.2","language":"eng","model_sha256":MODEL_SHA256,"files":files});
    let save = |value: &serde_json::Value| {
        fs::write(
            bundle.join("manifest.json"),
            serde_json::to_vec(value).unwrap(),
        )
        .unwrap()
    };
    save(&manifest);
    assert!(
        validate_runtime(&bundle).is_err(),
        "An unreviewed model must fail"
    );
    manifest["files"]["tessdata/eng.traineddata"]["sha256"] = MODEL_SHA256.into();
    save(&manifest);
    assert!(validate_runtime(&bundle).is_err());
}

#[test]
#[cfg(target_os = "macos")]
#[ignore = "requires staged native OCR runtime"]
fn native_ocr_rejects_omitted_dylibs_and_unlisted_runtime_assets() {
    use std::{fs, os::unix::fs::symlink};
    let source = std::path::PathBuf::from(
        std::env::var_os("WORKBENCH_TEST_OCR_RUNTIME").expect("Staged OCR required"),
    )
    .join("ocr");
    validate_runtime(&source).unwrap();
    let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let bundle = root.path().join("ocr");
    let original = fs::read(source.join("manifest.json")).unwrap();
    let mut manifest: serde_json::Value = serde_json::from_slice(&original).unwrap();
    for name in manifest["files"].as_object().unwrap().keys() {
        let target = bundle.join(name);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::copy(source.join(name), target).unwrap();
    }
    fs::write(bundle.join("manifest.json"), &original).unwrap();
    validate_runtime(&bundle).unwrap();
    let omitted = manifest["files"]
        .as_object()
        .unwrap()
        .keys()
        .find(|name| name.starts_with("lib/"))
        .unwrap()
        .clone();
    manifest["files"].as_object_mut().unwrap().remove(&omitted);
    fs::write(
        bundle.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    assert!(bundle.join(omitted).is_file());
    let runtime = Runtime {
        root: root.path().to_owned(),
    };
    let scratch = root.path().join("scratch");
    assert!(
        runtime.ocr(&scratch, IMAGE).is_err(),
        "A loadable but unverified library must fail before launch"
    );
    assert!(!scratch.exists());
    fs::write(bundle.join("manifest.json"), &original).unwrap();
    validate_runtime(&bundle).unwrap();
    let rogue = bundle.join("lib/unlisted.dylib");
    fs::write(&rogue, b"synthetic unlisted asset").unwrap();
    assert!(validate_runtime(&bundle).is_err());
    fs::remove_file(&rogue).unwrap();
    validate_runtime(&bundle).unwrap();
    let outside = root.path().join("outside");
    fs::write(&outside, b"outside sentinel").unwrap();
    symlink(&outside, &rogue).unwrap();
    assert!(validate_runtime(&bundle).is_err());
    assert_eq!(fs::read(&outside).unwrap(), b"outside sentinel");
    fs::remove_file(&rogue).unwrap();
    validate_runtime(&bundle).unwrap();
    manifest = serde_json::from_slice(&original).unwrap();
    manifest["files"]["../outside"] =
        serde_json::json!({"bytes":16,"sha256":digest(b"outside sentinel")});
    fs::write(
        bundle.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    assert!(
        matches!(validate_runtime(&bundle), Err(Error::Validation(message)) if message == "Unsafe OCR manifest entry")
    );
    fs::write(bundle.join("manifest.json"), &original).unwrap();
    validate_runtime(&bundle).unwrap();
    fs::remove_file(bundle.join("bin/tesseract")).unwrap();
    assert!(matches!(validate_runtime(&bundle), Err(Error::Blocked(_))));
    assert_eq!(fs::read(&outside).unwrap(), b"outside sentinel");
}
