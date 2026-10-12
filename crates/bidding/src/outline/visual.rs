//! Bounded full-frame original image delivery for the configured authoring model.
//! Loading or caching this value grants no reading receipt.
use crate::analysis::{
    FrozenInput,
    views::{SourceView, ViewIdentity},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use image::ImageDecoder;
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

pub async fn load_frozen_image_view(
    input: &FrozenInput,
    source_id: &str,
    max_edge: u32,
    max_bytes: usize,
    cancel: &CancellationToken,
) -> Result<SourceView, String> {
    let source = input
        .source_units
        .iter()
        .find(|source| source.source_unit_revision_id == source_id)
        .ok_or("unknown frozen image source")?;
    if source.locator["locator_kind"] != "image" || source.locator["image_available"] != true {
        return Err("source is not a persisted frozen image".into());
    }
    let digest = source.locator["image_ref"]
        .as_str()
        .and_then(|value| value.strip_prefix("objects/"))
        .filter(|value| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        })
        .ok_or("frozen image has no canonical object reference")?
        .to_string();
    let source_id = source_id.to_string();
    let page = source.locator["page_ordinal"]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or(0);
    tokio::select! {
        _=cancel.cancelled()=>Err("original image loading cancelled".into()),
        result=tokio::task::spawn_blocking(move || {
            let bytes=platform::read_blob(&digest).map_err(|e|format!("original image blob unavailable: {e}"))?;
            render_frozen_image_view(&source_id,&digest,page,&bytes,max_edge,max_bytes)
        })=>result.map_err(|e|format!("original image worker failed: {e}"))?,
    }
}

pub(crate) fn render_frozen_image_view(
    source_id: &str,
    digest: &str,
    page: u32,
    bytes: &[u8],
    max_edge: u32,
    max_bytes: usize,
) -> Result<SourceView, String> {
    if max_edge == 0 || max_bytes == 0 || bytes.len() > 64 * 1024 * 1024 {
        return Err("original image exceeds decode/input budget".into());
    }
    if hex::encode(Sha256::digest(bytes)) != digest {
        return Err("original frozen image digest mismatch".into());
    }
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(128 * 1024 * 1024);
    limits.max_image_width = Some(32_768);
    limits.max_image_height = Some(32_768);
    let image = decode_oriented_image(bytes, limits)
        .map_err(|e| format!("original image cannot be decoded within budget: {e}"))?;
    let mut edge = max_edge.min(image.width().max(image.height()));
    for attempt in 0..8 {
        let pixels = image.thumbnail(edge, edge).to_rgb8();
        let quality = if attempt == 0 { 85 } else { 70 };
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, quality)
            .encode_image(&pixels)
            .map_err(|e| e.to_string())?;
        if jpeg.len() <= max_bytes {
            let view = SourceView {
                identity: ViewIdentity {
                    source_id: source_id.into(),
                    original_sha256: digest.into(),
                    image_sha256: hex::encode(Sha256::digest(&jpeg)),
                    page_ordinal: page,
                    width: pixels.width(),
                    height: pixels.height(),
                    renderer: format!(
                        "frozen-source-view-v2/full-frame/jpeg-q{quality}/edge{edge}"
                    ),
                },
                jpeg_base64: STANDARD.encode(jpeg),
            };
            view.validate(source_id, max_edge, max_bytes)?;
            return Ok(view);
        }
        if edge <= 1 {
            break;
        }
        edge = (edge / 2).max(1);
    }
    Err("original image cannot fit the configured image budget; no pixels were delivered".into())
}

// ImageReader::into_decoder does not reserve the decoded output buffer as
// ImageReader::decode does. Preserve that reservation and also cover the copy
// needed by quarter-turn orientations before allocating either pixel buffer.
fn decode_oriented_image(
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
    #[test]
    fn rendering_is_full_frame_bounded_and_binds_actual_transport_bytes() {
        let image = image::RgbImage::from_fn(320, 200, |x, y| {
            image::Rgb([(x % 255) as u8, (y % 255) as u8, 80])
        });
        let mut png = std::io::Cursor::new(Vec::new());
        image.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let bytes = png.into_inner();
        let digest = hex::encode(Sha256::digest(&bytes));
        let view = render_frozen_image_view("image", &digest, 3, &bytes, 128, 4096).unwrap();
        assert!(view.identity.width <= 128 && view.identity.height <= 128);
        assert!(STANDARD.decode(&view.jpeg_base64).unwrap().len() <= 4096);
        assert_eq!(view.identity.original_sha256, digest);
        assert!(
            view.message()["content"][1]["image_url"]["url"]
                .as_str()
                .unwrap()
                .starts_with("data:image/jpeg;base64,")
        );
        view.validate("image", 128, 4096).unwrap();
        assert!(render_frozen_image_view("image", &"0".repeat(64), 3, &bytes, 128, 4096).is_err());
        assert!(render_frozen_image_view("image", &digest, 3, &bytes, 128, 1).is_err());
    }
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

    #[test]
    fn frozen_exif_orientation_is_applied_once_and_original_hash_is_preserved() {
        for orientation in [None, Some(2), Some(6), Some(8)] {
            let bytes = exif_fixture(orientation);
            let expected = expected_pixels(&bytes, orientation).to_rgb8();
            let decoded = decode_oriented_image(&bytes, image::Limits::default()).unwrap();
            assert_eq!(decoded.to_rgb8(), expected);
            let digest = hex::encode(Sha256::digest(&bytes));
            let view = render_frozen_image_view("image", &digest, 0, &bytes, 128, 100_000).unwrap();
            let jpeg = STANDARD.decode(&view.jpeg_base64).unwrap();
            let rendered = image::load_from_memory(&jpeg).unwrap().to_rgb8();
            assert_eq!(rendered.dimensions(), expected.dimensions());
            for (x, y) in [
                (10, 10),
                (expected.width() - 11, 10),
                (10, expected.height() - 11),
                (expected.width() - 11, expected.height() - 11),
            ] {
                for channel in 0..3 {
                    assert!(
                        rendered.get_pixel(x, y)[channel]
                            .abs_diff(expected.get_pixel(x, y)[channel])
                            <= 20
                    );
                }
            }
            assert_eq!(view.identity.original_sha256, digest);
            assert_eq!(
                view.identity.image_sha256,
                hex::encode(Sha256::digest(&jpeg))
            );
            // Encoded output has no residual orientation causing a second rotation.
            assert_eq!(
                decode_oriented_image(&jpeg, image::Limits::default())
                    .unwrap()
                    .to_rgb8(),
                rendered
            );
            view.validate("image", 128, 100_000).unwrap();
        }
    }

    #[test]
    fn frozen_exif_decode_preserves_output_and_rotation_allocation_limits() {
        let raw = exif_fixture(None);
        let rotated = exif_fixture(Some(6));
        let bytes = 96 * 64 * 3;
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(bytes - 1);
        assert!(decode_oriented_image(&raw, limits.clone()).is_err());
        limits.max_alloc = Some(bytes * 2 - 1);
        assert!(decode_oriented_image(&rotated, limits).is_err());
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(95);
        assert!(decode_oriented_image(&raw, limits).is_err());
    }
}
