use super::*;
#[test]
fn regions_profile_grants_only_fixed_text_and_tsv_outputs() {
    let text = profile(
        Path::new("/bundle/ocr/bin/tesseract"),
        Path::new("/bundle/ocr"),
        Path::new("/job"),
        Recipe::WordRegions,
    )
    .unwrap();
    assert!(text.contains("(allow file-write* (literal \"/job/result.txt\") (literal \"/job/result.tsv\") (subpath \"/job/scratch\"))"));
    assert!(text.contains("(deny process-fork)"));
    assert!(!text.contains("allow network"));
    assert!(!text.contains("java"));
    assert!(!text.contains("(allow file-write* (literal \"/job/input.pgm\")"));
    let original = profile(
        Path::new("/bundle/ocr/bin/tesseract"),
        Path::new("/bundle/ocr"),
        Path::new("/job"),
        Recipe::Text,
    )
    .unwrap();
    assert!(!original.contains("result.tsv"));
}

#[test]
#[ignore = "requires compiled synthetic native OCR probe"]
fn native_regions_recipe_denies_outside_io_network_and_bounds_output_and_cancel() {
    super::tests::native_recipe_boundaries(Recipe::WordRegions);
}
