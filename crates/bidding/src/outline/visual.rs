//! Bounded full-frame original image delivery for the configured authoring model.
//! Loading or caching this value grants no reading receipt.
use crate::analysis::{
    FrozenInput,
    views::{SourceView, ViewIdentity},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
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
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(128 * 1024 * 1024);
    limits.max_image_width = Some(32_768);
    limits.max_image_height = Some(32_768);
    reader.limits(limits);
    let image = reader
        .decode()
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
}
