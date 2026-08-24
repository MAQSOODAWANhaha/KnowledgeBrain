//! Manifest-only DOCX/PDF renderer. Never reads live shot/part/object tables.

use std::collections::HashSet;
use std::io::Cursor;

use docx_rs::{Docx, Paragraph, Pic, Run};
use image::{GenericImageView, ImageFormat};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::submission::GateFormat;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ManifestRenderAssetLocator {
    BidShot {
        placement_ordinal: u32,
        shot_artifact_id: Uuid,
    },
    MarkdownObject {
        part_key: String,
        occurrence_ordinal: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestRenderAsset {
    pub manifest_ordinal: u32,
    pub locator: ManifestRenderAssetLocator,
    pub object_ref: String,
    pub digest: String,
    pub media_type: String,
    pub byte_length: u64,
    pub bytes: Vec<u8>,
}

pub fn validate_manifest_render_assets(
    parts: &[(String, String)],
    assets: &[ManifestRenderAsset],
) -> Result<(), String> {
    prepare_manifest(parts, assets).map(|_| ())
}

pub fn render_manifest_document(
    format: GateFormat,
    title: &str,
    parts: &[(String, String)],
    assets: &[ManifestRenderAsset],
) -> Result<Vec<u8>, String> {
    let document = prepare_manifest(parts, assets)?;
    match format {
        GateFormat::Docx => manifest_to_docx(title, &document),
        GateFormat::Pdf => manifest_to_pdf(title, &document),
    }
}

pub fn renderer_contract_identity(format: GateFormat) -> Value {
    match format {
        GateFormat::Docx => json!({"version": DOCX_RENDERER_CONTRACT}),
        GateFormat::Pdf => json!({
            "version": PDF_RENDERER_CONTRACT,
            "font_sha256": PDF_FONT_SHA256,
        }),
    }
}

struct PreparedDocument<'a> {
    parts: Vec<PreparedPart<'a>>,
    assets: Vec<PreparedAsset<'a>>,
}

struct PreparedPart<'a> {
    part_key: &'a str,
    bid_shot_asset_indexes: Vec<usize>,
    lines: Vec<PreparedLine<'a>>,
}

struct PreparedLine<'a> {
    text: &'a str,
    markdown_asset_indexes: Vec<usize>,
}

struct PreparedAsset<'a> {
    source: &'a ManifestRenderAsset,
    image: PreparedImage,
}

struct PreparedImage {
    png_bytes: Vec<u8>,
    width: u32,
    height: u32,
}

fn prepare_manifest<'a>(
    parts: &'a [(String, String)],
    assets: &'a [ManifestRenderAsset],
) -> Result<PreparedDocument<'a>, String> {
    let mut part_keys = HashSet::new();
    for (part_key, _) in parts {
        if !part_keys.insert(part_key.as_str()) {
            return Err(format!("duplicate manifest part: {part_key}"));
        }
    }

    let mut locators = HashSet::new();
    let mut prepared_assets = Vec::with_capacity(assets.len());
    for (index, asset) in assets.iter().enumerate() {
        let expected_ordinal = u32::try_from(index)
            .map_err(|_| "manifest asset count exceeds u32 ordinal range".to_string())?;
        if asset.manifest_ordinal != expected_ordinal {
            return Err(format!(
                "manifest asset ordinal mismatch: expected {expected_ordinal}, got {}",
                asset.manifest_ordinal
            ));
        }
        if !locators.insert(asset.locator.clone()) {
            return Err(format!(
                "duplicate manifest asset locator: {:?}",
                asset.locator
            ));
        }
        validate_frozen_asset_identity(asset)?;
        prepared_assets.push(PreparedAsset {
            source: asset,
            image: prepare_image(asset)?,
        });
    }

    let bid_shot_asset_indexes: Vec<usize> = prepared_assets
        .iter()
        .enumerate()
        .filter_map(|(index, asset)| {
            matches!(
                asset.source.locator,
                ManifestRenderAssetLocator::BidShot { .. }
            )
            .then_some(index)
        })
        .collect();
    validate_bid_shots(parts, &prepared_assets, &bid_shot_asset_indexes)?;

    let mut used = vec![false; prepared_assets.len()];
    for index in &bid_shot_asset_indexes {
        used[*index] = true;
    }

    let mut prepared_parts = Vec::with_capacity(parts.len());
    for (part_key, markdown) in parts {
        let mut occurrence_ordinal = 0u32;
        let mut lines = Vec::new();
        for line in markdown.lines() {
            let mut markdown_asset_indexes = Vec::new();
            for (_, object_ref) in crate::submission::parse_markdown_object_occurrences(line) {
                let locator = ManifestRenderAssetLocator::MarkdownObject {
                    part_key: part_key.clone(),
                    occurrence_ordinal,
                };
                let Some(asset_index) = prepared_assets
                    .iter()
                    .position(|asset| asset.source.locator == locator)
                else {
                    return Err(format!(
                        "missing markdown render asset: part {part_key}, occurrence {occurrence_ordinal}"
                    ));
                };
                if prepared_assets[asset_index].source.object_ref != object_ref {
                    return Err(format!(
                        "markdown render asset object_ref mismatch: part {part_key}, occurrence {occurrence_ordinal}"
                    ));
                }
                used[asset_index] = true;
                markdown_asset_indexes.push(asset_index);
                occurrence_ordinal = occurrence_ordinal
                    .checked_add(1)
                    .ok_or_else(|| "markdown occurrence ordinal overflow".to_string())?;
            }
            lines.push(PreparedLine {
                text: line,
                markdown_asset_indexes,
            });
        }
        prepared_parts.push(PreparedPart {
            part_key,
            bid_shot_asset_indexes: if part_key == "3" {
                bid_shot_asset_indexes.clone()
            } else {
                Vec::new()
            },
            lines,
        });
    }

    if let Some(index) = used.iter().position(|used| !used) {
        return Err(format!(
            "unexpected manifest render asset: {:?}",
            prepared_assets[index].source.locator
        ));
    }

    let expected_asset_indexes: Vec<usize> = prepared_parts
        .iter()
        .flat_map(|part| {
            part.bid_shot_asset_indexes.iter().copied().chain(
                part.lines
                    .iter()
                    .flat_map(|line| line.markdown_asset_indexes.iter().copied()),
            )
        })
        .collect();
    if let Some((manifest_ordinal, asset_index)) = expected_asset_indexes
        .iter()
        .enumerate()
        .find(|(manifest_ordinal, asset_index)| manifest_ordinal != *asset_index)
    {
        return Err(format!(
            "manifest asset order does not match render order at ordinal {manifest_ordinal}: {:?}",
            prepared_assets[*asset_index].source.locator
        ));
    }

    Ok(PreparedDocument {
        parts: prepared_parts,
        assets: prepared_assets,
    })
}

fn validate_bid_shots(
    parts: &[(String, String)],
    assets: &[PreparedAsset<'_>],
    indexes: &[usize],
) -> Result<(), String> {
    if !indexes.is_empty() && !parts.iter().any(|(part_key, _)| part_key == "3") {
        return Err("bid shot render assets require manifest part 3".into());
    }
    let mut shot_ids = HashSet::new();
    for (expected_ordinal, index) in indexes.iter().enumerate() {
        let ManifestRenderAssetLocator::BidShot {
            placement_ordinal,
            shot_artifact_id,
        } = assets[*index].source.locator
        else {
            unreachable!("indexes contain only bid shots")
        };
        let expected_ordinal = u32::try_from(expected_ordinal)
            .map_err(|_| "bid shot count exceeds u32 ordinal range".to_string())?;
        if placement_ordinal != expected_ordinal {
            return Err(format!(
                "bid shot placement ordinal mismatch: expected {expected_ordinal}, got {placement_ordinal}"
            ));
        }
        if shot_artifact_id.is_nil() {
            return Err(format!(
                "bid shot artifact id is nil at placement {placement_ordinal}"
            ));
        }
        if !shot_ids.insert(shot_artifact_id) {
            return Err(format!(
                "duplicate bid shot artifact id: {shot_artifact_id}"
            ));
        }
    }
    Ok(())
}

fn validate_frozen_asset_identity(asset: &ManifestRenderAsset) -> Result<(), String> {
    if asset.digest.len() != 64
        || !asset
            .digest
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(format!("invalid manifest asset digest: {}", asset.digest));
    }
    if asset.object_ref != format!("objects/{}", asset.digest) {
        return Err(format!(
            "manifest asset object_ref does not match digest: {}",
            asset.object_ref
        ));
    }
    let actual_length = u64::try_from(asset.bytes.len())
        .map_err(|_| "manifest asset byte length exceeds u64 range".to_string())?;
    if asset.byte_length != actual_length {
        return Err(format!(
            "manifest asset byte length mismatch: expected {}, got {actual_length}",
            asset.byte_length
        ));
    }
    let actual_digest = domain::sha256_hex(&asset.bytes);
    if asset.digest != actual_digest {
        return Err(format!(
            "manifest asset digest mismatch: expected {}, got {actual_digest}",
            asset.digest
        ));
    }
    Ok(())
}

fn prepare_image(asset: &ManifestRenderAsset) -> Result<PreparedImage, String> {
    if !asset.media_type.starts_with("image/") {
        return Err(format!(
            "manifest asset MIME is not an image: {}",
            asset.media_type
        ));
    }
    let declared_format = ImageFormat::from_mime_type(&asset.media_type).ok_or_else(|| {
        format!(
            "manifest asset MIME is not a supported image: {}",
            asset.media_type
        )
    })?;
    let detected_format = image::guess_format(&asset.bytes)
        .map_err(|error| format!("manifest asset image signature is invalid: {error}"))?;
    if detected_format != declared_format {
        return Err(format!(
            "manifest asset MIME does not match image signature: {}",
            asset.media_type
        ));
    }
    let image = image::load_from_memory_with_format(&asset.bytes, detected_format)
        .map_err(|error| format!("manifest asset image decode failed: {error}"))?;
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return Err("manifest asset image has zero dimensions".into());
    }
    let mut png = Cursor::new(Vec::new());
    image
        .write_to(&mut png, ImageFormat::Png)
        .map_err(|error| format!("manifest asset image normalization failed: {error}"))?;
    Ok(PreparedImage {
        png_bytes: png.into_inner(),
        width,
        height,
    })
}

fn manifest_to_docx(title: &str, document: &PreparedDocument<'_>) -> Result<Vec<u8>, String> {
    let mut docx = Docx::new().add_paragraph(heading(title, 36));
    for part in &document.parts {
        docx = docx.add_paragraph(heading(part.part_key, 28));
        for asset_index in &part.bid_shot_asset_indexes {
            docx = docx.add_paragraph(image_paragraph(&document.assets[*asset_index].image));
        }
        for line in &part.lines {
            if line.text.trim().is_empty() {
                continue;
            }
            if let Some(rest) = line.text.strip_prefix("# ") {
                docx = docx.add_paragraph(heading(rest, 28));
            } else {
                docx = docx.add_paragraph(paragraph(line.text));
            }
            for asset_index in &line.markdown_asset_indexes {
                docx = docx.add_paragraph(image_paragraph(&document.assets[*asset_index].image));
            }
        }
    }
    let mut buf = Cursor::new(Vec::new());
    docx.pack(&mut buf).map_err(|error| error.to_string())?;
    Ok(buf.into_inner())
}

fn heading(text: &str, size: usize) -> Paragraph {
    Paragraph::new().add_run(Run::new().add_text(text).bold().size(size))
}

fn paragraph(text: &str) -> Paragraph {
    Paragraph::new().add_run(Run::new().add_text(text))
}

fn image_paragraph(image: &PreparedImage) -> Paragraph {
    let max_w = 480u32;
    let (draw_w, draw_h) = if image.width > max_w {
        let next_h = (u64::from(image.height) * u64::from(max_w) / u64::from(image.width)) as u32;
        (max_w, next_h.max(1))
    } else {
        (image.width, image.height)
    };
    Paragraph::new().add_run(Run::new().add_image(Pic::new_with_dimensions(
        image.png_bytes.clone(),
        draw_w,
        draw_h,
    )))
}

fn manifest_to_pdf(title: &str, document: &PreparedDocument<'_>) -> Result<Vec<u8>, String> {
    use printpdf::{FontId, Mm, Op, ParsedFont, PdfDocument, PdfPage, PdfSaveOptions};
    let font_bytes = cjk_font_bytes()?;
    let mut warnings = Vec::new();
    let parsed = ParsedFont::from_bytes(font_bytes, 0, &mut warnings)
        .ok_or_else(|| "parse frozen PDF font".to_string())?;
    let document_id = pdf_document_identifier(title, document);
    let mut pdf = PdfDocument::new(title);
    pdf.metadata.info.creator = PDF_RENDERER_CONTRACT.into();
    pdf.metadata.info.producer = format!("{PDF_RENDERER_CONTRACT};font-sha256={PDF_FONT_SHA256}");
    pdf.metadata.info.identifier = document_id.clone();
    let font = FontId(PDF_FONT_RESOURCE_ID.into());
    pdf.resources
        .fonts
        .map
        .insert(font.clone(), printpdf::font::PdfFont::new(parsed));
    const PAGE_W: f32 = 210.0;
    let mut pages: Vec<Vec<Op>> = Vec::new();
    let mut ops: Vec<Op> = Vec::new();
    let mut y = PDF_PAGE_HEIGHT - PDF_MARGIN;
    write_pdf_line(&mut ops, &mut pages, &mut y, &font, title, 18.0);
    for part in &document.parts {
        write_pdf_line(&mut ops, &mut pages, &mut y, &font, part.part_key, 14.0);
        for asset_index in &part.bid_shot_asset_indexes {
            add_pdf_image(
                &mut pdf,
                &mut ops,
                &mut pages,
                &mut y,
                &document.assets[*asset_index],
            )?;
        }
        for line in &part.lines {
            if line.text.trim().is_empty() {
                continue;
            }
            let size = if line.text.starts_with("# ") {
                14.0
            } else {
                11.0
            };
            let text = line.text.strip_prefix("# ").unwrap_or(line.text);
            write_pdf_line(&mut ops, &mut pages, &mut y, &font, text, size);
            for asset_index in &line.markdown_asset_indexes {
                add_pdf_image(
                    &mut pdf,
                    &mut ops,
                    &mut pages,
                    &mut y,
                    &document.assets[*asset_index],
                )?;
            }
        }
    }
    if !ops.is_empty() {
        pages.push(ops);
    }
    let pdf_pages: Vec<PdfPage> = pages
        .into_iter()
        .map(|ops| PdfPage::new(Mm(PAGE_W), Mm(PDF_PAGE_HEIGHT), ops))
        .collect();
    let mut save_warnings = Vec::new();
    let mut output = pdf
        .with_pages(pdf_pages)
        .to_lopdf_document(&PdfSaveOptions::default(), &mut save_warnings);
    let stable_id =
        lopdf::Object::String(document_id.into_bytes(), lopdf::StringFormat::Hexadecimal);
    output.trailer.set(
        "ID",
        lopdf::Object::Array(vec![stable_id.clone(), stable_id]),
    );
    let mut bytes = Vec::new();
    output
        .save_to(&mut bytes)
        .map_err(|error| format!("serialize deterministic PDF: {error}"))?;
    Ok(bytes)
}

const PDF_PAGE_HEIGHT: f32 = 297.0;
const PDF_MARGIN: f32 = 16.0;
const DOCX_RENDERER_CONTRACT: &str = "knowledgebrain.bid.docx.v1";
const PDF_RENDERER_CONTRACT: &str = "knowledgebrain.bid.pdf.v1";
const PDF_FONT_RESOURCE_ID: &str = "kb-bid-font-v1";
const PDF_FONT_SHA256: &str = "5d0df56f107605387e0de494b22dfc7fb05d8d79ffd981474e7be11dbe571882";
const PDF_FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/NotoSansJP-Regular.otf");

fn write_pdf_line(
    ops: &mut Vec<printpdf::Op>,
    pages: &mut Vec<Vec<printpdf::Op>>,
    y: &mut f32,
    font: &printpdf::FontId,
    text: &str,
    size: f32,
) {
    use printpdf::{Mm, Op, PdfFontHandle, Point, Pt, TextItem};
    if *y < PDF_MARGIN + size * 0.45 {
        if !ops.is_empty() {
            pages.push(std::mem::take(ops));
        }
        *y = PDF_PAGE_HEIGHT - PDF_MARGIN;
    }
    ops.extend([
        Op::StartTextSection,
        Op::SetTextCursor {
            pos: Point {
                x: Mm(PDF_MARGIN).into(),
                y: Mm(*y).into(),
            },
        },
        Op::SetFont {
            font: PdfFontHandle::External(font.clone()),
            size: Pt(size),
        },
        Op::SetLineHeight { lh: Pt(size + 2.0) },
        Op::ShowText {
            items: vec![TextItem::Text(text.to_string())],
        },
        Op::EndTextSection,
    ]);
    *y -= size * 0.45;
}

fn add_pdf_image(
    pdf: &mut printpdf::PdfDocument,
    ops: &mut Vec<printpdf::Op>,
    pages: &mut Vec<Vec<printpdf::Op>>,
    y: &mut f32,
    asset: &PreparedAsset<'_>,
) -> Result<(), String> {
    use printpdf::{Mm, Op, RawImage, XObject, XObjectId, XObjectTransform};
    let mut warnings = Vec::new();
    let raw_image = RawImage::decode_from_bytes(&asset.image.png_bytes, &mut warnings)
        .map_err(|error| format!("normalized manifest image decode failed: {error}"))?;
    let id = XObjectId(format!("kb-bid-image-v1-{}", asset.source.manifest_ordinal));
    let max_w_mm = 140.0_f32;
    let nat_w = raw_image.width.max(1) as f32 * 25.4 / 150.0;
    let nat_h = raw_image.height.max(1) as f32 * 25.4 / 150.0;
    let scale = (max_w_mm / nat_w).min(1.0);
    let draw_h = nat_h * scale;
    if *y - draw_h < PDF_MARGIN {
        if !ops.is_empty() {
            pages.push(std::mem::take(ops));
        }
        *y = PDF_PAGE_HEIGHT - PDF_MARGIN;
    }
    if pdf
        .resources
        .xobjects
        .map
        .insert(id.clone(), XObject::Image(raw_image))
        .is_some()
    {
        return Err(format!(
            "duplicate deterministic PDF image resource: {}",
            id.0
        ));
    }
    *y -= draw_h;
    ops.push(Op::UseXobject {
        id,
        transform: XObjectTransform {
            translate_x: Some(Mm(PDF_MARGIN).into()),
            translate_y: Some(Mm(*y).into()),
            scale_x: Some(scale),
            scale_y: Some(scale),
            dpi: Some(150.0),
            ..Default::default()
        },
    });
    *y -= 3.0;
    Ok(())
}

fn cjk_font_bytes() -> Result<&'static [u8], String> {
    let digest = domain::sha256_hex(PDF_FONT_BYTES);
    if digest != PDF_FONT_SHA256 {
        return Err(format!(
            "frozen PDF font digest mismatch: expected {PDF_FONT_SHA256}, got {digest}"
        ));
    }
    Ok(PDF_FONT_BYTES)
}

fn pdf_document_identifier(title: &str, document: &PreparedDocument<'_>) -> String {
    let mut hash = Sha256::new();
    hash_pdf_field(&mut hash, PDF_RENDERER_CONTRACT.as_bytes());
    hash_pdf_field(&mut hash, PDF_FONT_SHA256.as_bytes());
    hash_pdf_field(&mut hash, title.as_bytes());
    hash.update((document.parts.len() as u64).to_be_bytes());
    for part in &document.parts {
        hash_pdf_field(&mut hash, part.part_key.as_bytes());
        hash.update((part.lines.len() as u64).to_be_bytes());
        for line in &part.lines {
            hash_pdf_field(&mut hash, line.text.as_bytes());
        }
    }
    hash.update((document.assets.len() as u64).to_be_bytes());
    for asset in &document.assets {
        hash.update(asset.source.manifest_ordinal.to_be_bytes());
        match &asset.source.locator {
            ManifestRenderAssetLocator::BidShot {
                placement_ordinal,
                shot_artifact_id,
            } => {
                hash.update([0]);
                hash.update(placement_ordinal.to_be_bytes());
                hash.update(shot_artifact_id.as_bytes());
            }
            ManifestRenderAssetLocator::MarkdownObject {
                part_key,
                occurrence_ordinal,
            } => {
                hash.update([1]);
                hash_pdf_field(&mut hash, part_key.as_bytes());
                hash.update(occurrence_ordinal.to_be_bytes());
            }
        }
        hash_pdf_field(&mut hash, asset.source.object_ref.as_bytes());
        hash_pdf_field(&mut hash, asset.source.digest.as_bytes());
        hash_pdf_field(&mut hash, asset.source.media_type.as_bytes());
        hash.update(asset.source.byte_length.to_be_bytes());
    }
    hex::encode(hash.finalize())
}

fn hash_pdf_field(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_be_bytes());
    hash.update(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_bytes(color: [u8; 4]) -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(2, 2, image::Rgba(color));
        let mut bytes = Cursor::new(Vec::new());
        image
            .write_to(&mut bytes, ImageFormat::Png)
            .expect("encode test PNG");
        bytes.into_inner()
    }

    fn markdown_asset(
        manifest_ordinal: u32,
        part_key: &str,
        occurrence_ordinal: u32,
        color: [u8; 4],
    ) -> ManifestRenderAsset {
        let bytes = png_bytes(color);
        let digest = domain::sha256_hex(&bytes);
        ManifestRenderAsset {
            manifest_ordinal,
            locator: ManifestRenderAssetLocator::MarkdownObject {
                part_key: part_key.into(),
                occurrence_ordinal,
            },
            object_ref: format!("objects/{digest}"),
            digest,
            media_type: "image/png".into(),
            byte_length: bytes.len() as u64,
            bytes,
        }
    }

    fn bid_shot(
        manifest_ordinal: u32,
        placement_ordinal: u32,
        color: [u8; 4],
    ) -> ManifestRenderAsset {
        let bytes = png_bytes(color);
        let digest = domain::sha256_hex(&bytes);
        ManifestRenderAsset {
            manifest_ordinal,
            locator: ManifestRenderAssetLocator::BidShot {
                placement_ordinal,
                shot_artifact_id: Uuid::from_u128(u128::from(placement_ordinal) + 1),
            },
            object_ref: format!("objects/{digest}"),
            digest,
            media_type: "image/png".into(),
            byte_length: bytes.len() as u64,
            bytes,
        }
    }

    #[test]
    fn validates_and_renders_exact_markdown_occurrences() {
        let first = markdown_asset(0, "1", 0, [10, 20, 30, 255]);
        let second = markdown_asset(1, "1", 1, [10, 20, 30, 255]);
        let parts = vec![(
            "1".into(),
            format!("说明 {}\n再次 {}", first.object_ref, second.object_ref),
        )];
        let assets = vec![first, second];

        validate_manifest_render_assets(&parts, &assets).expect("valid assets");
        let docx = render_manifest_document(GateFormat::Docx, "投标文件", &parts, &assets)
            .expect("render DOCX");
        assert!(docx.starts_with(b"PK"));
    }

    #[test]
    fn rejects_missing_and_unexpected_markdown_occurrences() {
        let expected = markdown_asset(0, "1", 0, [10, 20, 30, 255]);
        let parts = vec![("1".into(), expected.object_ref.clone())];
        let missing = validate_manifest_render_assets(&parts, &[]).unwrap_err();
        assert!(missing.contains("missing markdown render asset"));

        let no_occurrences = vec![("1".into(), "plain text".into())];
        let extra = validate_manifest_render_assets(
            &no_occurrences,
            &[markdown_asset(0, "1", 0, [10, 20, 30, 255])],
        )
        .unwrap_err();
        assert!(extra.contains("unexpected manifest render asset"));
    }

    #[test]
    fn rejects_duplicate_locator_and_non_contiguous_manifest_ordinals() {
        let duplicate = vec![
            markdown_asset(0, "1", 0, [10, 20, 30, 255]),
            markdown_asset(1, "1", 0, [10, 20, 30, 255]),
        ];
        let parts = vec![(
            "1".into(),
            format!("{}\n{}", duplicate[0].object_ref, duplicate[1].object_ref),
        )];
        assert!(
            validate_manifest_render_assets(&parts, &duplicate)
                .unwrap_err()
                .contains("duplicate manifest asset locator")
        );

        let out_of_order = vec![markdown_asset(1, "1", 0, [10, 20, 30, 255])];
        assert!(
            validate_manifest_render_assets(&parts[..1], &out_of_order)
                .unwrap_err()
                .contains("manifest asset ordinal mismatch")
        );
    }

    #[test]
    fn rejects_wrong_object_ref_non_image_mime_and_damaged_image() {
        let expected = markdown_asset(0, "1", 0, [10, 20, 30, 255]);
        let parts = vec![("1".into(), expected.object_ref)];
        let wrong_ref = vec![markdown_asset(0, "1", 0, [40, 50, 60, 255])];
        assert!(
            validate_manifest_render_assets(&parts, &wrong_ref)
                .unwrap_err()
                .contains("object_ref mismatch")
        );

        let mut non_image = markdown_asset(0, "1", 0, [10, 20, 30, 255]);
        non_image.media_type = "application/octet-stream".into();
        assert!(
            validate_manifest_render_assets(&parts, &[non_image])
                .unwrap_err()
                .contains("MIME is not an image")
        );

        let mut damaged = markdown_asset(0, "1", 0, [10, 20, 30, 255]);
        damaged.bytes = b"not an image".to_vec();
        damaged.digest = domain::sha256_hex(&damaged.bytes);
        damaged.object_ref = format!("objects/{}", damaged.digest);
        damaged.byte_length = damaged.bytes.len() as u64;
        assert!(
            validate_manifest_render_assets(&parts, &[damaged])
                .unwrap_err()
                .contains("image signature is invalid")
        );
    }

    #[test]
    fn part_three_places_bid_shots_before_markdown_occurrences() {
        let shot = bid_shot(0, 0, [10, 20, 30, 255]);
        let markdown = markdown_asset(1, "3", 0, [40, 50, 60, 255]);
        let parts = vec![("3".into(), format!("产品图片 {}", markdown.object_ref))];
        let assets = vec![shot, markdown];

        let document = prepare_manifest(&parts, &assets).expect("prepare manifest");
        assert_eq!(document.parts[0].bid_shot_asset_indexes, vec![0]);
        assert_eq!(document.parts[0].lines[0].markdown_asset_indexes, vec![1]);
    }

    #[test]
    fn rejects_manifest_order_that_places_markdown_before_bid_shots() {
        let markdown = markdown_asset(0, "3", 0, [40, 50, 60, 255]);
        let shot = bid_shot(1, 0, [10, 20, 30, 255]);
        let parts = vec![("3".into(), format!("产品图片 {}", markdown.object_ref))];

        assert!(
            validate_manifest_render_assets(&parts, &[markdown, shot])
                .unwrap_err()
                .contains("order does not match render order")
        );
    }

    #[test]
    fn rejects_invalid_bid_shot_placement_and_shots_without_part_three() {
        let parts = vec![("3".into(), "产品".into())];
        let invalid_placement = vec![bid_shot(0, 1, [10, 20, 30, 255])];
        assert!(
            validate_manifest_render_assets(&parts, &invalid_placement)
                .unwrap_err()
                .contains("bid shot placement ordinal mismatch")
        );

        let no_part_three = vec![("1".into(), "商务".into())];
        assert!(
            validate_manifest_render_assets(&no_part_three, &[bid_shot(0, 0, [10, 20, 30, 255])])
                .unwrap_err()
                .contains("require manifest part 3")
        );
    }

    #[test]
    fn rejects_frozen_digest_and_length_mismatch() {
        let mut digest_mismatch = bid_shot(0, 0, [10, 20, 30, 255]);
        digest_mismatch.bytes[0] ^= 1;
        assert!(
            validate_manifest_render_assets(&[("3".into(), "产品".into())], &[digest_mismatch])
                .unwrap_err()
                .contains("digest mismatch")
        );

        let mut length_mismatch = bid_shot(0, 0, [10, 20, 30, 255]);
        length_mismatch.byte_length += 1;
        assert!(
            validate_manifest_render_assets(&[("3".into(), "产品".into())], &[length_mismatch])
                .unwrap_err()
                .contains("byte length mismatch")
        );
    }

    #[test]
    fn pdf_render_is_byte_replayable_with_cjk_and_images() {
        let shot = bid_shot(0, 0, [10, 20, 30, 255]);
        let markdown = markdown_asset(1, "3", 0, [40, 50, 60, 255]);
        let parts = vec![("3".into(), format!("中文投标文件 {}", markdown.object_ref))];
        let assets = vec![shot, markdown];

        let first = render_manifest_document(GateFormat::Pdf, "投标文件", &parts, &assets)
            .expect("first PDF render");
        let _interleaved = render_manifest_document(
            GateFormat::Pdf,
            "另一份文件",
            &[("1".into(), "不同内容".into())],
            &[],
        )
        .expect("interleaved PDF render");
        let replay = render_manifest_document(GateFormat::Pdf, "投标文件", &parts, &assets)
            .expect("replayed PDF render");

        assert!(first.starts_with(b"%PDF"));
        assert_eq!(first, replay);
        assert_eq!(
            domain::sha256_hex(&first),
            "c20630b93f6cdc67fe7a10985099b9d025f2e799e1fe8ea2d84c9f9c748c7f2a"
        );
    }

    #[test]
    fn frozen_pdf_font_matches_declared_identity() {
        const FONT_MANIFEST: &str = include_str!("../assets/fonts/NotoSansJP-Regular.toml");
        const FONT_LICENSE: &[u8] = include_bytes!("../assets/fonts/OFL.txt");

        assert_eq!(domain::sha256_hex(PDF_FONT_BYTES), PDF_FONT_SHA256);
        assert_eq!(cjk_font_bytes().expect("frozen PDF font"), PDF_FONT_BYTES);
        assert!(FONT_MANIFEST.contains(PDF_RENDERER_CONTRACT));
        assert!(FONT_MANIFEST.contains(PDF_FONT_SHA256));
        assert!(FONT_MANIFEST.contains(&domain::sha256_hex(FONT_LICENSE)));
    }

    #[test]
    fn renderer_contract_identity_freezes_format_specific_resources() {
        assert_eq!(
            renderer_contract_identity(GateFormat::Docx),
            serde_json::json!({"version":"knowledgebrain.bid.docx.v1"})
        );
        assert_eq!(
            renderer_contract_identity(GateFormat::Pdf),
            serde_json::json!({
                "version":"knowledgebrain.bid.pdf.v1",
                "font_sha256":PDF_FONT_SHA256,
            })
        );
    }
}
