use super::*;
const IMAGE: &[u8] = include_bytes!("../../../../../fixtures/ocr/synthetic.pgm");
const HEADER: &str = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n";
fn valid_tsv() -> String {
    format!("{HEADER}1\t1\t0\t0\t0\t0\t0\t0\t1200\t230\t-1\t\n2\t1\t1\t0\t0\t0\t10\t10\t100\t40\t-1\t\n3\t1\t1\t1\t0\t0\t10\t10\t100\t40\t-1\t\n4\t1\t1\t1\t1\t0\t10\t10\t100\t40\t-1\t\n5\t1\t1\t1\t1\t1\t10\t10\t30\t40\t96.123456\t0042\n5\t1\t1\t1\t1\t2\t50\t10\t60\t40\t0.000000\t123.45\n")
}
fn result() -> OcrRegions {
    let tsv = valid_tsv().into_bytes();
    OcrRegions {
        result: OcrRegionsResult {
            protocol_version: 1,
            job_id: uuid::Uuid::new_v4().to_string(),
            raster_sha256: ocr::digest(IMAGE),
            raster_bytes: IMAGE.len() as u64,
            width: 1200,
            height: 230,
            engine: "tesseract-5.5.2".into(),
            language: "eng".into(),
            model_sha256: ocr::MODEL_SHA256.into(),
            runtime_manifest_sha256: "0".repeat(64),
            status: RegionsStatus::Recognized,
            text: "0042 123.45\n".into(),
            tsv_sha256: ocr::digest(&tsv),
            tsv_bytes: tsv.len() as u64,
            regions: tsv::parse(&tsv, "0042 123.45\n", 1200, 230).unwrap(),
            limitations: limitations(),
        },
        tsv,
    }
}
#[test]
fn regions_parse_actual_hierarchy_and_separate_engine_confidence() {
    let value = result();
    validate_result(&value.result, IMAGE, &value.tsv).unwrap();
    assert_eq!(value.result.regions.len(), 6);
    assert_eq!(value.result.regions[4].engine_confidence, Some(96.123456));
    assert_eq!(value.result.regions[5].engine_confidence, Some(0.0));
    assert_eq!(value.result.regions[3].engine_confidence, None);
    assert_eq!(value.result.regions[4].text, "0042");
    assert!(tsv::parse(&value.tsv, "0042 123.45", 1200, 230).is_err());
    let blank = format!("{HEADER}1\t1\t0\t0\t0\t0\t0\t0\t1200\t230\t-1\t\n");
    let regions = tsv::parse(blank.as_bytes(), "\n\x0c", 1200, 230).unwrap();
    assert_eq!(regions.len(), 1);
}
#[test]
fn regions_reject_truncation_hierarchy_confidence_boxes_and_oversize() {
    let good = valid_tsv();
    let mut failures = vec![
        String::new(),
        good.trim_end_matches('\n').into(),
        good.replace("page_num", "page"),
        good.replace("96.123456", "NaN"),
        good.replace("96.123456", "inf"),
        good.replace("96.123456", "100.000001"),
        good.replace("96.123456", "-1.000000"),
        good.replace("96.123456", "9.6e1"),
        good.replace("96.123456", "096.123456"),
        good.replace("5\t1\t1\t1\t1\t2", "5\t1\t1\t1\t1\t1"),
        good.replace("5\t1\t1\t1\t1\t2", "5\t1\t1\t1\t1\t3"),
        good.replace("5\t1\t1", "5\t2\t1"),
        good.replace("50\t10\t60", "4294967295\t10\t60"),
        good.replace("50\t10\t60", "50\t10\t61"),
        good.replace("50\t10\t60", "50\t10\t0"),
        good.replace("123.45", "bad\tvalue"),
        good.replace("123.45", "bad\0value"),
        good.replace("123.45", &"x".repeat(MAX_WORD_BYTES + 1)),
        good.replace("-1\t\n", "0\t\n"),
        good.replacen("2\t1\t1\t0\t0\t0\t10\t10\t100\t40\t-1\t\n", "", 1),
    ];
    let rows: Vec<_> = good.split_inclusive('\n').collect();
    failures.push(rows[..rows.len() - 1].concat()); // Complete-row truncation caught by companion text.
    failures.push(rows[..rows.len() - 2].concat()); // Empty line hierarchy cannot complete.
    failures.push(format!("{}{}", good, rows[1])); // Duplicate page.
    failures.push(format!("{}{}", good, rows.last().unwrap())); // Duplicate word ID.
    for bad in failures {
        assert!(
            tsv::parse(bad.as_bytes(), "0042 123.45\n", 1200, 230).is_err(),
            "accepted {bad:?}"
        );
    }
    assert!(tsv::parse(&[b'x'; MAX_TSV_BYTES as usize], "", 1200, 230).is_err());
    let invalid_utf8 = [HEADER.as_bytes(), b"\xff\n"].concat();
    assert!(tsv::parse(&invalid_utf8, "", 1200, 230).is_err());
}
#[test]
fn regions_reject_typed_result_binding_and_schema_tampering() {
    for mutate in [
        |v: &mut OcrRegionsResult| v.protocol_version = 2,
        |v: &mut OcrRegionsResult| v.raster_sha256 = "0".repeat(64),
        |v: &mut OcrRegionsResult| v.width += 1,
        |v: &mut OcrRegionsResult| v.tsv_bytes += 1,
        |v: &mut OcrRegionsResult| v.tsv_sha256 = "0".repeat(64),
        |v: &mut OcrRegionsResult| v.status = RegionsStatus::NoTextRecognized,
        |v: &mut OcrRegionsResult| {
            v.limitations.pop();
        },
        |v: &mut OcrRegionsResult| v.regions[4].bounds.left += 1,
        |v: &mut OcrRegionsResult| v.regions[4].engine_confidence = Some(f64::NAN),
        |v: &mut OcrRegionsResult| v.regions[4].text = "42".into(),
        |v: &mut OcrRegionsResult| v.text = "42 123.45".into(),
        |v: &mut OcrRegionsResult| v.language = "any".into(),
    ] {
        let mut value = result();
        mutate(&mut value.result);
        assert!(validate_result(&value.result, IMAGE, &value.tsv).is_err());
    }
    let mut json = serde_json::to_value(result().result).unwrap();
    json["analyst_confidence"] = serde_json::json!(1);
    assert!(serde_json::from_value::<OcrRegionsResult>(json).is_err());
}
#[test]
fn regions_missing_runtime_and_cancelled_input_fail_without_scratch() {
    let temp = tempfile::tempdir().unwrap();
    let runtime = Runtime {
        root: temp.path().join("missing"),
    };
    let scratch = temp.path().join("scratch");
    assert!(matches!(
        runtime.ocr_regions(&scratch, IMAGE),
        Err(Error::Blocked(_))
    ));
    let token = CancellationToken::default();
    token.cancel();
    assert!(matches!(
        runtime.ocr_regions_with_cancel(&scratch, IMAGE, &token),
        Err(Error::Blocked(_))
    ));
    assert!(!scratch.exists());
}
#[test]
#[cfg(target_os = "macos")]
#[ignore = "requires packaged English OCR runtime"]
fn native_regions_recognize_bounded_word_boxes_and_blank_raster() {
    let runtime = Runtime {
        root: std::env::var_os("WORKBENCH_TEST_OCR_RUNTIME")
            .expect("Packaged runtime required")
            .into(),
    };
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let value = runtime.ocr_regions(temp.path(), IMAGE).unwrap();
    validate_result(&value.result, IMAGE, &value.tsv).unwrap();
    assert_eq!(value.result.status, RegionsStatus::Recognized);
    let words: Vec<_> = value
        .result
        .regions
        .iter()
        .filter(|r| r.level == RegionLevel::Word)
        .collect();
    assert!(words.iter().any(|w| w.text == "0042"));
    assert!(words.iter().any(|w| w.text == "123.45"));
    assert!(words.iter().any(|w| w.text == "SYNTHETIC"));
    // Every actual word box covers ink in this synthetic raster, not a fabricated full-page box.
    let pixels = IMAGE.splitn(4, |b| *b == b'\n').nth(3).unwrap();
    for word in words {
        let b = &word.bounds;
        assert!(b.width < 1200 && b.height < 230);
        assert!((b.top..b.top + b.height)
            .any(|y| (b.left..b.left + b.width).any(|x| pixels[(y * 1200 + x) as usize] < 128)));
    }
    let mut blank = b"P5\n200 100\n255\n".to_vec();
    blank.extend(vec![255; 20_000]);
    let blank = runtime.ocr_regions(temp.path(), &blank).unwrap();
    assert_eq!(blank.result.status, RegionsStatus::NoTextRecognized);
    assert_eq!(blank.result.regions.len(), 1);
    assert!(blank.result.text.trim().is_empty());
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
}

#[test]
fn regions_enforce_word_and_row_caps_and_reject_duplicate_boxes() {
    let good = valid_tsv();
    let prefix: String = good.split_inclusive('\n').take(5).collect();
    let mut words = prefix.clone();
    let mut text = String::new();
    for word in 1..=MAX_WORDS {
        words.push_str(&format!(
            "5\t1\t1\t1\t1\t{word}\t10\t10\t30\t40\t0.000000\tw{word}\n"
        ));
        text.push_str(&format!("w{word} "));
    }
    assert_eq!(
        tsv::parse(words.as_bytes(), &format!("{text}\n"), 1200, 230)
            .unwrap()
            .len(),
        MAX_WORDS + 4
    );
    words.push_str("5\t1\t1\t1\t1\t10001\t10\t10\t30\t40\t0.000000\tw10001\n");
    text.push_str("w10001\n");
    assert!(tsv::parse(words.as_bytes(), &format!("{text}\n"), 1200, 230).is_err());

    let mut lines: String = good.split_inclusive('\n').take(4).collect();
    let mut text = String::new();
    for line in 1..=9998 {
        lines.push_str(&format!("4\t1\t1\t1\t{line}\t0\t10\t10\t100\t40\t-1\t\n"));
        lines.push_str(&format!(
            "5\t1\t1\t1\t{line}\t1\t10\t10\t30\t40\t0.000000\tw{line}\n"
        ));
        text.push_str(&format!("w{line} "));
    }
    assert_eq!(
        tsv::parse(lines.as_bytes(), &format!("{text}\n"), 1200, 230)
            .unwrap()
            .len(),
        MAX_REGIONS - 1
    );
    lines.push_str("4\t1\t1\t1\t9999\t0\t10\t10\t100\t40\t-1\t\n5\t1\t1\t1\t9999\t1\t10\t10\t30\t40\t0.000000\tw9999\n");
    text.push_str("w9999\n");
    assert!(tsv::parse(lines.as_bytes(), &format!("{text}\n"), 1200, 230).is_err());

    let duplicate = format!("{prefix}5\t1\t1\t1\t1\t1\t10\t10\t30\t40\t50.000000\t0042\n5\t1\t1\t1\t1\t2\t10\t10\t30\t40\t50.000000\t0042\n");
    assert!(tsv::parse(duplicate.as_bytes(), "0042 0042\n", 1200, 230).is_err());
    // Legitimate repeated text in distinct rectangles remains present.
    let separate = duplicate.replacen("1\t2\t10\t10\t30", "1\t2\t50\t10\t30", 1);
    assert_eq!(
        tsv::parse(separate.as_bytes(), "0042 0042\n", 1200, 230)
            .unwrap()
            .len(),
        6
    );
}
