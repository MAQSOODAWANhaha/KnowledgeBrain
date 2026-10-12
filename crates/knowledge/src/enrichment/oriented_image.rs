//! Decode OCR pixels in display orientation before regional cropping.
use image::ImageDecoder;

// ImageReader::into_decoder does not reserve the decoded output buffer as
// ImageReader::decode does. Preserve that reservation and also cover the copy
// needed by quarter-turn orientations before allocating either pixel buffer.
pub(super) fn decode(
    bytes: &[u8],
    mut limits: image::Limits,
) -> image::ImageResult<image::DynamicImage> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
    reader.limits(limits.clone());
    let mut decoder = reader.into_decoder()?;
    let orientation = decoder.orientation()?;
    limits.reserve(decoder.total_bytes())?;
    if matches!(
        orientation,
        image::metadata::Orientation::Rotate90
            | image::metadata::Orientation::Rotate270
            | image::metadata::Orientation::Rotate90FlipH
            | image::metadata::Orientation::Rotate270FlipH
    ) {
        limits.reserve(decoder.total_bytes())?;
    }
    decoder.set_limits(limits)?;
    let mut image = image::DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn exif_fixture(orientation: Option<u8>) -> Vec<u8> {
        use image::ImageEncoder;
        let pixels = image::RgbImage::from_fn(96, 64, |x, y| {
            image::Rgb(match (x < 48, y < 32) {
                (true, true) => [230, 20, 20],
                (false, true) => [20, 230, 20],
                (true, false) => [20, 20, 230],
                (false, false) => [230, 230, 20],
            })
        });
        let mut bytes = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 95);
        if let Some(tag) = orientation {
            // Little-endian TIFF with one SHORT Orientation entry.
            encoder
                .set_exif_metadata(vec![
                    b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 0x01, 3, 0, 1, 0, 0, 0, tag, 0, 0,
                    0, 0, 0, 0, 0,
                ])
                .unwrap();
        }
        encoder.encode_image(&pixels).unwrap();
        bytes
    }

    fn expected_pixels(bytes: &[u8], orientation: Option<u8>) -> image::DynamicImage {
        let raw = image::load_from_memory(bytes).unwrap();
        match orientation {
            Some(2) => raw.fliph(),
            Some(6) => raw.rotate90(),
            Some(8) => raw.rotate270(),
            _ => raw,
        }
    }

    #[tokio::test]
    async fn ocr_exif_is_applied_once_before_local_region_callback() {
        for orientation in [None, Some(2), Some(6), Some(8)] {
            let bytes = exif_fixture(orientation);
            let expected = expected_pixels(&bytes, orientation).to_rgb8();
            let image = decode(&bytes, image::Limits::default()).unwrap();
            assert_eq!(image.to_rgb8(), expected);
            let mut calls = 0;
            let result = super::super::regions::recognize_async(&image, 2, 8, |png| {
                calls += 1;
                let crop = image::load_from_memory(&png).unwrap().to_rgb8();
                assert_eq!(crop, expected);
                assert_eq!(
                    decode(&png, image::Limits::default()).unwrap().to_rgb8(),
                    crop
                );
                std::future::ready(Ok("synthetic local OCR".to_owned()))
            })
            .await;
            assert_eq!(calls, 1);
            assert!(result.complete);
            assert_eq!(result.fragments[0].right, expected.width());
            assert_eq!(result.fragments[0].bottom, expected.height());
        }
    }

    #[test]
    fn ocr_exif_decode_preserves_output_and_rotation_allocation_limits() {
        let raw = exif_fixture(None);
        let rotated = exif_fixture(Some(8));
        let bytes = 96 * 64 * 3;
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(bytes - 1);
        assert!(decode(&raw, limits.clone()).is_err());
        limits.max_alloc = Some(bytes * 2 - 1);
        assert!(decode(&rotated, limits).is_err());
        let mut limits = image::Limits::default();
        limits.max_image_height = Some(63);
        assert!(decode(&raw, limits).is_err());
    }
}
