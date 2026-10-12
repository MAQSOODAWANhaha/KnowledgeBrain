//! Independent regression probes for physical OCR overlap ownership.
#[path = "../src/enrichment/regions.rs"]
mod regions;

#[test]
fn repeated_lines_outside_blank_overlap_must_not_be_silently_deduplicated() {
    // The top and bottom crops each contain a different physical occurrence.
    // Their intersection strip is blank; equal strings are not shared identity.
    let image = image::DynamicImage::new_rgb8(100, 200);
    let mut calls = 0;
    let result = regions::recognize(&image, 1, 4, |bytes| {
        calls += 1;
        let crop = image::load_from_memory(bytes).unwrap();
        match crop.height() {
            200 => Err("incomplete length".into()),
            110 => Ok("签名".into()),
            20 => Ok(String::new()),
            height => panic!("unexpected OCR region height: {height}"),
        }
    });
    assert!(
        result.complete,
        "blank overlap should preserve both distinct occurrences"
    );
    assert_eq!(
        result.text.matches("签名").count(),
        2,
        "false complete OCR dropped a distinct physical line: {}",
        result.text
    );
    assert_eq!(
        calls, 4,
        "the actual geometric overlap must be checked independently"
    );
}
