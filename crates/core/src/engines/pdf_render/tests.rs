use super::*;
const SCAN: &[u8] = include_bytes!("../../../../../fixtures/pdf-render/scan.pdf");
fn sample() -> (PdfRenderResult, Vec<u8>) {
    let raster = b"P5\n2 1\n255\n\xff\0".to_vec();
    (
        PdfRenderResult {
            protocol_version: 1,
            job_id: uuid::Uuid::new_v4().to_string(),
            original_sha256: digest(SCAN),
            original_bytes: SCAN.len() as u64,
            renderer: "pdfbox-3.0.8-scan-v1".into(),
            java_runtime: "21.0.12.1".into(),
            page_number: 1,
            dpi: 72,
            page_count: Some(2),
            status: RenderStatus::Rendered,
            failure: None,
            geometry: Some(PageGeometry {
                crop_box: [10.0, 20.0, 12.0, 21.0],
                rotation_degrees: 0,
                pdf_to_raster: [1.0, 0.0, 0.0, -1.0, -10.0, 21.0],
            }),
            raster: Some(RasterBinding {
                path: "raster.pgm".into(),
                sha256: digest(&raster),
                bytes: raster.len() as u64,
                width: 2,
                height: 1,
            }),
            limitations: vec![
                RenderLimitation::ScanFocusedSubset,
                RenderLimitation::AnnotationsExcluded,
                RenderLimitation::ColorConvertedToGray,
                RenderLimitation::UnreviewedRaster,
                RenderLimitation::NoWordRegions,
            ],
        },
        raster,
    )
}
#[test]
fn pdf_result_rejects_original_page_dpi_geometry_raster_and_protocol_tampering() {
    // Java transports exact doubles promoted from PDFBox f32 coordinates. The
    // default serde_json fast parser can shift these by one ULP without float_roundtrip.
    let coordinates: [f64; 2] =
        serde_json::from_str("[-10.100000381469727,-20.200000762939453]").unwrap();
    assert_eq!(coordinates, [f64::from(-10.1_f32), f64::from(-20.2_f32)]);
    let (result, raster) = sample();
    validate_result(&result, SCAN, 1, 72, Some(&raster)).unwrap();
    assert!(validate_result(&result, b"%PDF-fake", 1, 72, Some(&raster)).is_err());
    assert!(validate_result(&result, SCAN, 2, 72, Some(&raster)).is_err());
    assert!(validate_result(&result, SCAN, 1, 73, Some(&raster)).is_err());
    for mutate in [
        |r: &mut PdfRenderResult| r.geometry.as_mut().unwrap().rotation_degrees = 90,
        |r: &mut PdfRenderResult| r.geometry.as_mut().unwrap().pdf_to_raster[0] = 0.0,
        |r: &mut PdfRenderResult| r.geometry.as_mut().unwrap().crop_box[0] = f64::NAN,
        |r: &mut PdfRenderResult| r.raster.as_mut().unwrap().sha256 = "0".repeat(64),
        |r: &mut PdfRenderResult| r.raster.as_mut().unwrap().width = 3,
        |r: &mut PdfRenderResult| r.raster.as_mut().unwrap().path = "outside.pgm".into(),
        |r: &mut PdfRenderResult| r.page_count = Some(0),
        |r: &mut PdfRenderResult| r.status = RenderStatus::Encrypted,
        |r: &mut PdfRenderResult| {
            r.limitations.pop();
        },
        |r: &mut PdfRenderResult| r.protocol_version = 2,
    ] {
        let mut changed = result.clone();
        mutate(&mut changed);
        assert!(validate_result(&changed, SCAN, 1, 72, Some(&raster)).is_err());
    }
    let encoded = serde_json::to_string(&result).unwrap();
    let duplicate = format!("{},\"protocol_version\":1}}", &encoded[..encoded.len() - 1]);
    assert!(serde_json::from_str::<PdfRenderResult>(&duplicate).is_err());
    assert!(serde_json::from_str::<PdfRenderResult>(&format!("{encoded}{{}}")).is_err());
    let mut json = serde_json::to_value(result).unwrap();
    json["unexpected"] = true.into();
    assert!(serde_json::from_value::<PdfRenderResult>(json).is_err());
}
#[test]
fn pdf_requests_and_unavailable_runtime_fail_closed() {
    let directory = tempfile::tempdir().unwrap();
    let scratch = directory.path().join("scratch");
    let runtime = Runtime {
        root: directory.path().join("absent"),
    };
    for (page, dpi) in [(0, 72), (1001, 72), (1, 71), (1, 301)] {
        assert!(runtime.render_pdf_page(&scratch, SCAN, page, dpi).is_err());
        assert!(!scratch.exists());
    }
    let token = CancellationToken::default();
    token.cancel();
    assert!(matches!(
        runtime.render_pdf_page_with_cancel(&scratch, SCAN, 1, 72, &token),
        Err(Error::Blocked(_))
    ));
    assert!(!scratch.exists());
    assert!(matches!(
        runtime.render_pdf_page(&scratch, SCAN, 1, 72),
        Err(Error::Blocked(_))
    ));
    if scratch.exists() {
        assert_eq!(std::fs::read_dir(scratch).unwrap().count(), 0);
    }
}
#[cfg(target_os = "macos")]
fn runtime() -> Runtime {
    Runtime {
        root: std::env::var_os("WORKBENCH_TEST_PDF_RUNTIME")
            .expect("Staged PDF renderer required")
            .into(),
    }
}
#[test]
#[cfg(target_os = "macos")]
#[ignore = "requires staged confined PDF renderer and OCR"]
fn native_pdf_scan_and_selected_page_render_then_ocr_with_bound_provenance() {
    let scratch = tempfile::tempdir().unwrap();
    let runtime = runtime();
    let output = runtime.ocr_pdf_page(scratch.path(), SCAN, 1, 144).unwrap();
    assert_eq!(output.render.result.status, RenderStatus::Rendered);
    let binding = output.render.result.raster.as_ref().unwrap();
    assert_eq!((binding.width, binding.height), (1200, 230));
    assert_eq!(output.render.result.page_count, Some(2));
    let ocr = output.ocr.unwrap();
    assert_eq!(ocr.raster_sha256, binding.sha256);
    assert!(ocr.text.contains("SYNTHETIC OCR TEST"));
    assert!(ocr.text.contains("REFERENCE 0042 AMOUNT 123.45"));
    validate_result(
        &output.render.result,
        SCAN,
        1,
        144,
        output.render.raster.as_deref(),
    )
    .unwrap();
    let jpeg = runtime
        .ocr_pdf_page(
            scratch.path(),
            include_bytes!("../../../../../fixtures/pdf-render/scan-jpeg.pdf"),
            1,
            144,
        )
        .unwrap();
    assert!(jpeg
        .ocr
        .unwrap()
        .text
        .contains("REFERENCE 0042 AMOUNT 123.45"));
    let second = runtime
        .render_pdf_page(scratch.path(), SCAN, 2, 72)
        .unwrap();
    assert_eq!(second.result.page_number, 2);
    assert_eq!(second.result.raster.as_ref().unwrap().width, 600);
    assert_ne!(
        second.result.raster.as_ref().unwrap().sha256,
        binding.sha256
    );
    assert_eq!(std::fs::read_dir(scratch.path()).unwrap().count(), 0);
}
#[test]
#[cfg(target_os = "macos")]
#[ignore = "requires staged confined PDF renderer"]
fn native_pdf_crop_and_all_quarter_turns_match_pixel_mapping() {
    let scratch = tempfile::tempdir().unwrap();
    let runtime = runtime();
    let cross_zero = runtime
        .render_pdf_page(
            scratch.path(),
            include_bytes!("../../../../../fixtures/pdf-render/crop-cross-zero.pdf"),
            1,
            123,
        )
        .unwrap();
    assert_eq!(cross_zero.result.status, RenderStatus::Rendered);
    let geometry = cross_zero.result.geometry.as_ref().unwrap();
    let binding = cross_zero.result.raster.as_ref().unwrap();
    let raster = cross_zero.raster.as_ref().unwrap();
    let [a, b, c, d, e, f] = geometry.pdf_to_raster;
    let header = format!("P5\n{} {}\n255\n", binding.width, binding.height).len();
    for (x, y, expected) in [(0.0, 10.0, 0), (50.0, -10.0, 255)] {
        let rx = (a * x + c * y + e) as usize;
        let ry = (b * x + d * y + f) as usize;
        assert_eq!(raster[header + ry * binding.width as usize + rx], expected);
    }
    for (rotation, dpi) in [0, 90, 180, 270]
        .into_iter()
        .flat_map(|r| [72, 123].map(|dpi| (r, dpi)))
    {
        let bytes = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../../fixtures/pdf-render/crop-{rotation}.pdf")),
        )
        .unwrap();
        let output = runtime
            .render_pdf_page(scratch.path(), &bytes, 1, dpi)
            .unwrap();
        assert_eq!(output.result.status, RenderStatus::Rendered);
        let geometry = output.result.geometry.as_ref().unwrap();
        assert_eq!(geometry.rotation_degrees, rotation);
        assert_eq!(geometry.crop_box, [10.25, 20.5, 91.0, 60.75]);
        let raster = output.raster.unwrap();
        let binding = output.result.raster.unwrap();
        let header = format!("P5\n{} {}\n255\n", binding.width, binding.height).len();
        // A black 20x20 square at PDF coordinates (10.25..30.25,40.75..60.75). Test its interior
        // and a distant white point through the published top-left raster mapping.
        let [a, b, c, d, e, f] = geometry.pdf_to_raster;
        for (x, y, expected) in [(20.0, 50.0, 0), (70.0, 30.0, 255)] {
            let rx = (a * x + c * y + e) as usize;
            let ry = (b * x + d * y + f) as usize;
            assert_eq!(
                raster[header + ry * binding.width as usize + rx],
                expected,
                "rotation {rotation}"
            );
        }
    }
}
#[test]
#[cfg(target_os = "macos")]
#[ignore = "requires staged confined PDF renderer"]
fn native_pdf_rejections_are_explicit_and_never_enter_ocr() {
    let scratch = tempfile::tempdir().unwrap();
    let runtime = runtime();
    for (name, status, failure) in [
        (
            "jpeg-dimensions",
            RenderStatus::Failed,
            RenderFailure::MalformedDocument,
        ),
        (
            "jpeg-pixel-limit",
            RenderStatus::QuotaExhausted,
            RenderFailure::PixelLimit,
        ),
        (
            "long-image",
            RenderStatus::Failed,
            RenderFailure::MalformedDocument,
        ),
        (
            "short-image",
            RenderStatus::Failed,
            RenderFailure::MalformedDocument,
        ),
        (
            "encrypted",
            RenderStatus::Encrypted,
            RenderFailure::EncryptedDocument,
        ),
        (
            "encrypted-empty",
            RenderStatus::Encrypted,
            RenderFailure::EncryptedDocument,
        ),
        (
            "malformed",
            RenderStatus::Failed,
            RenderFailure::MalformedDocument,
        ),
        (
            "missing-image",
            RenderStatus::Failed,
            RenderFailure::MalformedDocument,
        ),
        (
            "active",
            RenderStatus::Unsupported,
            RenderFailure::ActiveContent,
        ),
        (
            "external",
            RenderStatus::Unsupported,
            RenderFailure::ExternalResource,
        ),
        (
            "font",
            RenderStatus::Unsupported,
            RenderFailure::UnsupportedFeature,
        ),
        (
            "user-unit",
            RenderStatus::Unsupported,
            RenderFailure::UnsupportedFeature,
        ),
        (
            "invalid-rotation",
            RenderStatus::Unsupported,
            RenderFailure::UnsupportedFeature,
        ),
        (
            "unsupported-filter",
            RenderStatus::Unsupported,
            RenderFailure::UnsupportedFeature,
        ),
        (
            "unknown-operator",
            RenderStatus::Unsupported,
            RenderFailure::UnsupportedFeature,
        ),
        (
            "large-page",
            RenderStatus::QuotaExhausted,
            RenderFailure::PixelLimit,
        ),
        (
            "page-limit",
            RenderStatus::QuotaExhausted,
            RenderFailure::PageLimit,
        ),
        (
            "structure-limit",
            RenderStatus::QuotaExhausted,
            RenderFailure::StructureLimit,
        ),
        (
            "stream-limit",
            RenderStatus::QuotaExhausted,
            RenderFailure::StreamLimit,
        ),
        (
            "operator-limit",
            RenderStatus::QuotaExhausted,
            RenderFailure::OperatorLimit,
        ),
    ] {
        let bytes = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../../fixtures/pdf-render/{name}.pdf")),
        )
        .unwrap();
        let output = runtime.ocr_pdf_page(scratch.path(), &bytes, 1, 72).unwrap();
        assert_eq!(
            output.render.result.status, status,
            "{name}: {:?}",
            output.render.result
        );
        assert_eq!(output.render.result.failure, Some(failure), "{name}");
        assert!(output.render.raster.is_none() && output.ocr.is_none());
        assert_eq!(std::fs::read_dir(scratch.path()).unwrap().count(), 0);
    }
    let out = runtime
        .render_pdf_page(scratch.path(), SCAN, 3, 72)
        .unwrap();
    assert_eq!(out.result.failure, Some(RenderFailure::PageOutOfRange));
    let out = runtime
        .render_pdf_page(scratch.path(), b"synthetic not a PDF", 1, 72)
        .unwrap();
    assert_eq!(out.result.failure, Some(RenderFailure::UnsupportedFormat));
}
#[test]
#[cfg(target_os = "macos")]
#[ignore = "requires copied staged PDF runtime; mutates and restores its component"]
fn native_pdf_runtime_inventory_rejects_missing_unlisted_linked_or_modified_assets() {
    use std::{fs, os::unix::fs::symlink};
    let runtime = runtime();
    runtime::validate(&runtime.root).unwrap();
    let component = runtime.root.join("pdf-render");
    let manifest = component.join("manifest.json");
    let original = fs::read(&manifest).unwrap();
    let mut omitted: serde_json::Value = serde_json::from_slice(&original).unwrap();
    omitted["files"]
        .as_object_mut()
        .unwrap()
        .remove("lib/fontbox-3.0.8.jar");
    fs::write(&manifest, serde_json::to_vec(&omitted).unwrap()).unwrap();
    assert!(runtime::validate(&runtime.root).is_err());
    fs::write(&manifest, &original).unwrap();
    runtime::validate(&runtime.root).unwrap();
    let extra = component.join("lib/unlisted.jar");
    fs::write(&extra, "synthetic unlisted").unwrap();
    assert!(runtime::validate(&runtime.root).is_err());
    fs::remove_file(&extra).unwrap();
    runtime::validate(&runtime.root).unwrap();
    let target = component.join("lib/fontbox-3.0.8.jar");
    let bytes = fs::read(&target).unwrap();
    fs::write(&target, "changed").unwrap();
    assert!(runtime::validate(&runtime.root).is_err());
    fs::write(&target, &bytes).unwrap();
    runtime::validate(&runtime.root).unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();
    fs::write(outside.path(), "outside sentinel").unwrap();
    fs::remove_file(&target).unwrap();
    symlink(outside.path(), &target).unwrap();
    assert!(runtime::validate(&runtime.root).is_err());
    fs::remove_file(&target).unwrap();
    fs::write(&target, &bytes).unwrap();
    assert_eq!(
        fs::read_to_string(outside.path()).unwrap(),
        "outside sentinel"
    );
    runtime::validate(&runtime.root).unwrap();
}
