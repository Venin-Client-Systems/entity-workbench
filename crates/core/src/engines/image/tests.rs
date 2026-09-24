use super::*;
const PNG: &[u8] = include_bytes!("../../../../../fixtures/images/synthetic.png");
const JPEG: &[u8] = include_bytes!("../../../../../fixtures/images/synthetic.jpg");
fn sample() -> (ImageDecodeResult, Vec<u8>) {
    let raster = b"P5\n1 1\n255\n\xff".to_vec();
    (
        ImageDecodeResult {
            protocol_version: 1,
            job_id: uuid::Uuid::new_v4().to_string(),
            original_sha256: digest(PNG),
            original_bytes: PNG.len() as u64,
            decoder: "jdk-imageio-21-v1".into(),
            java_runtime: "21.0.12.1".into(),
            media_type: "image/png".into(),
            status: DecodeStatus::Decoded,
            failure: None,
            raster: Some(RasterBinding {
                path: "raster.pgm".into(),
                sha256: digest(&raster),
                bytes: raster.len() as u64,
                width: 1,
                height: 1,
                source_image_index: 0,
                pixel_mapping: PixelMapping::EncodedPixelsGrayWhiteAlphaV1,
            }),
            limitations: vec![
                DecodeLimitation::ExifOrientationNotApplied,
                DecodeLimitation::EmbeddedPreviewsExcluded,
                DecodeLimitation::MetadataNotExtracted,
                DecodeLimitation::ColorConvertedToGray,
                DecodeLimitation::NoDocumentPageMapping,
            ],
        },
        raster,
    )
}
#[test]
fn image_results_bind_original_raster_dimensions_and_honest_outcome() {
    let (mut result, raster) = sample();
    validate_result(&result, PNG, Some(&raster)).unwrap();
    assert!(validate_result(&result, JPEG, Some(&raster)).is_err());
    result.raster.as_mut().unwrap().source_image_index = 1;
    assert!(validate_result(&result, PNG, Some(&raster)).is_err());
    result.raster.as_mut().unwrap().source_image_index = 0;
    result.raster.as_mut().unwrap().width = 2;
    assert!(validate_result(&result, PNG, Some(&raster)).is_err());
    result.raster.as_mut().unwrap().width = 1;
    result.status = DecodeStatus::Failed;
    result.failure = Some(DecodeFailure::MalformedImage);
    assert!(validate_result(&result, PNG, Some(&raster)).is_err());
    result.raster = None;
    validate_result(&result, PNG, None).unwrap();
    result.failure = Some(DecodeFailure::PixelLimit);
    assert!(validate_result(&result, PNG, None).is_err());
    result.status = DecodeStatus::QuotaExhausted;
    validate_result(&result, PNG, None).unwrap();
    result.limitations.pop();
    assert!(validate_result(&result, PNG, None).is_err());
}
#[test]
fn cancelled_and_missing_image_runtime_fail_closed() {
    let root = tempfile::tempdir().unwrap();
    let runtime = Runtime {
        root: root.path().join("missing"),
    };
    let scratch = root.path().join("scratch");
    let token = CancellationToken::default();
    token.cancel();
    assert!(matches!(
        runtime.decode_image_with_cancel(&scratch, PNG, &token),
        Err(Error::Blocked(_))
    ));
    assert!(!scratch.exists());
    assert!(matches!(
        runtime.decode_image(&scratch, PNG),
        Err(Error::Blocked(_))
    ));
    if scratch.exists() {
        assert_eq!(std::fs::read_dir(&scratch).unwrap().count(), 0);
    }
}

#[test]
#[cfg(target_os = "macos")]
#[ignore = "requires staged confined image worker and OCR runtime"]
fn native_images_decode_and_ocr_with_exact_original_and_raster_binding() {
    let runtime = Runtime {
        root: std::env::var_os("WORKBENCH_TEST_IMAGE_RUNTIME")
            .expect("Staged image runtime required")
            .into(),
    };
    let scratch = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    for image in [
        PNG,
        JPEG,
        include_bytes!("../../../../../fixtures/images/exif-orientation.jpg").as_slice(),
    ] {
        let combined = runtime.ocr_image(scratch.path(), image).unwrap();
        assert_eq!(combined.image.result.status, DecodeStatus::Decoded);
        let binding = combined.image.result.raster.as_ref().unwrap();
        assert_eq!(
            (binding.width, binding.height, binding.source_image_index),
            (1200, 230, 0)
        );
        assert_eq!(combined.image.result.original_sha256, digest(image));
        let recognized = combined.ocr.unwrap();
        assert_eq!(recognized.raster_sha256, binding.sha256);
        assert!(recognized.text.contains("SYNTHETIC OCR TEST"));
        assert!(recognized.text.contains("REFERENCE 0042 AMOUNT 123.45"));
        validate_result(
            &combined.image.result,
            image,
            combined.image.raster.as_deref(),
        )
        .unwrap();
        assert_eq!(std::fs::read_dir(scratch.path()).unwrap().count(), 0);
    }
    let colors = runtime
        .decode_image(
            scratch.path(),
            include_bytes!("../../../../../fixtures/images/alpha.png"),
        )
        .unwrap();
    assert_eq!(colors.raster.unwrap(), b"P5\n2 2\n255\n\x4c\x96\x1d\xff");
}
#[test]
#[cfg(target_os = "macos")]
#[ignore = "requires staged confined image worker"]
fn native_images_reject_oversize_animation_malformed_and_unsupported_without_rasters() {
    let runtime = Runtime {
        root: std::env::var_os("WORKBENCH_TEST_IMAGE_RUNTIME")
            .expect("Staged image runtime required")
            .into(),
    };
    let scratch = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    for (bytes, status, failure) in [
        (
            include_bytes!("../../../../../fixtures/images/oversize.png").as_slice(),
            DecodeStatus::QuotaExhausted,
            DecodeFailure::PixelLimit,
        ),
        (
            include_bytes!("../../../../../fixtures/images/oversize.jpg").as_slice(),
            DecodeStatus::QuotaExhausted,
            DecodeFailure::PixelLimit,
        ),
        (
            include_bytes!("../../../../../fixtures/images/pixel-limit.png").as_slice(),
            DecodeStatus::QuotaExhausted,
            DecodeFailure::PixelLimit,
        ),
        (
            include_bytes!("../../../../../fixtures/images/chunk-limit.png").as_slice(),
            DecodeStatus::QuotaExhausted,
            DecodeFailure::ContainerLimit,
        ),
        (
            include_bytes!("../../../../../fixtures/images/animated.png").as_slice(),
            DecodeStatus::Unsupported,
            DecodeFailure::MultipleImages,
        ),
        (
            include_bytes!("../../../../../fixtures/images/multiple.jpg").as_slice(),
            DecodeStatus::Unsupported,
            DecodeFailure::MultipleImages,
        ),
        (
            include_bytes!("../../../../../fixtures/images/bad-crc.png").as_slice(),
            DecodeStatus::Failed,
            DecodeFailure::MalformedImage,
        ),
        (
            include_bytes!("../../../../../fixtures/images/unsupported.gif").as_slice(),
            DecodeStatus::Unsupported,
            DecodeFailure::UnsupportedFormat,
        ),
    ] {
        let image = runtime.ocr_image(scratch.path(), bytes).unwrap();
        assert_eq!(image.image.result.status, status);
        assert_eq!(image.image.result.failure, Some(failure));
        assert!(image.image.raster.is_none());
        assert!(image.ocr.is_none());
        assert_eq!(std::fs::read_dir(scratch.path()).unwrap().count(), 0);
    }
    let truncated = runtime
        .decode_image(
            scratch.path(),
            include_bytes!("../../../../../fixtures/images/truncated.jpg"),
        )
        .unwrap();
    assert_eq!(truncated.result.status, DecodeStatus::Failed);
    assert!(truncated.raster.is_none());
}
