//! Manifest-only DOCX/PDF renderer. Never reads live shot/part/object tables.

use std::collections::HashSet;
use std::io::Cursor;

use docx_rs::{Docx, Paragraph, Pic, Run, Table, TableCell, TableRow};
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
    ProceduralAttachmentOriginal {
        part_key: String,
        attachment_ordinal: u32,
        attachment_id: Uuid,
        kind: String,
    },
    ProceduralAttachmentPage {
        part_key: String,
        attachment_ordinal: u32,
        attachment_id: Uuid,
        page_ordinal: u32,
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
    lines: Vec<PreparedLine>,
    procedural_asset_indexes: Vec<usize>,
}

struct PreparedLine {
    text: String,
    markdown_asset_indexes: Vec<usize>,
}

struct PreparedAsset<'a> {
    source: &'a ManifestRenderAsset,
    image: Option<PreparedImage>,
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
            image: prepare_asset_image(asset)?,
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
    let procedural_asset_indexes: Vec<usize> = prepared_assets
        .iter()
        .enumerate()
        .filter_map(|(index, asset)| {
            matches!(
                asset.source.locator,
                ManifestRenderAssetLocator::ProceduralAttachmentOriginal { .. }
                    | ManifestRenderAssetLocator::ProceduralAttachmentPage { .. }
            )
            .then_some(index)
        })
        .collect();
    validate_procedural_attachments(parts, &prepared_assets, &procedural_asset_indexes)?;

    let mut used = vec![false; prepared_assets.len()];
    for index in &bid_shot_asset_indexes {
        used[*index] = true;
    }
    for index in &procedural_asset_indexes {
        used[*index] = true;
    }

    let mut prepared_parts = Vec::with_capacity(parts.len());
    for (part_key, markdown) in parts {
        let mut occurrence_ordinal = 0u32;
        let mut lines = Vec::new();
        for line in markdown.lines() {
            let mut markdown_asset_indexes = Vec::new();
            let mut rendered_text = String::with_capacity(line.len());
            let mut rendered_cursor = 0usize;
            for (source_range, object_ref) in
                crate::submission::parse_markdown_object_occurrences(line)
            {
                rendered_text.push_str(&line[rendered_cursor..source_range.start]);
                rendered_cursor = source_range.end;
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
            rendered_text.push_str(&line[rendered_cursor..]);
            lines.push(PreparedLine {
                text: rendered_text,
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
            procedural_asset_indexes: procedural_asset_indexes
                .iter()
                .copied()
                .filter(|index| procedural_asset_part_key(&prepared_assets[*index]) == part_key)
                .collect(),
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
                    .flat_map(|line| line.markdown_asset_indexes.iter().copied())
                    .chain(part.procedural_asset_indexes.iter().copied()),
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

fn procedural_asset_part_key<'a>(asset: &'a PreparedAsset<'_>) -> &'a str {
    match &asset.source.locator {
        ManifestRenderAssetLocator::ProceduralAttachmentOriginal { part_key, .. }
        | ManifestRenderAssetLocator::ProceduralAttachmentPage { part_key, .. } => part_key,
        _ => unreachable!("procedural asset index contains a non-procedural locator"),
    }
}

fn validate_procedural_attachments(
    parts: &[(String, String)],
    assets: &[PreparedAsset<'_>],
    indexes: &[usize],
) -> Result<(), String> {
    let mut expected_attachment_ordinal = 0u32;
    let mut current: Option<(&str, Uuid, u32, bool, u32)> = None;
    let mut seen = HashSet::new();

    for index in indexes {
        let asset = &assets[*index];
        match &asset.source.locator {
            ManifestRenderAssetLocator::ProceduralAttachmentOriginal {
                part_key,
                attachment_ordinal,
                attachment_id,
                kind,
            } => {
                if let Some((_, _, _, was_pdf, next_page)) = current.take()
                    && was_pdf
                    && next_page == 0
                {
                    return Err("PDF procedural attachment has no frozen render pages".into());
                }
                if !matches!(part_key.as_str(), "6:authorization" | "6:procedural")
                    || !parts.iter().any(|(key, _)| key == part_key)
                {
                    return Err(format!(
                        "procedural attachment requires its manifest part: {part_key}"
                    ));
                }
                if *attachment_ordinal != expected_attachment_ordinal {
                    return Err(format!(
                        "procedural attachment ordinal mismatch: expected {expected_attachment_ordinal}, got {attachment_ordinal}"
                    ));
                }
                if attachment_id.is_nil() || !seen.insert(*attachment_id) {
                    return Err(format!(
                        "invalid or duplicate procedural attachment id: {attachment_id}"
                    ));
                }
                if !matches!(
                    kind.as_str(),
                    "bid_bond" | "authorization_support" | "seal_sample" | "procedural_support"
                ) {
                    return Err(format!("invalid procedural attachment kind: {kind}"));
                }
                let is_pdf = asset.source.media_type == "application/pdf";
                if is_pdf == asset.image.is_some() {
                    return Err("procedural attachment original render type mismatch".into());
                }
                current = Some((part_key, *attachment_id, *attachment_ordinal, is_pdf, 0));
                expected_attachment_ordinal = expected_attachment_ordinal
                    .checked_add(1)
                    .ok_or_else(|| "procedural attachment ordinal overflow".to_string())?;
            }
            ManifestRenderAssetLocator::ProceduralAttachmentPage {
                part_key,
                attachment_ordinal,
                attachment_id,
                page_ordinal,
            } => {
                let Some((current_part, current_id, current_ordinal, is_pdf, next_page)) =
                    current.as_mut()
                else {
                    return Err("procedural attachment page precedes its original".into());
                };
                if !*is_pdf
                    || current_part != part_key
                    || *current_id != *attachment_id
                    || *current_ordinal != *attachment_ordinal
                    || *page_ordinal != *next_page
                    || asset.image.is_none()
                {
                    return Err("procedural attachment page identity or order mismatch".into());
                }
                *next_page = next_page
                    .checked_add(1)
                    .ok_or_else(|| "procedural attachment page ordinal overflow".to_string())?;
            }
            _ => unreachable!("indexes contain only procedural attachments"),
        }
    }
    if let Some((_, _, _, is_pdf, next_page)) = current
        && is_pdf
        && next_page == 0
    {
        return Err("PDF procedural attachment has no frozen render pages".into());
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

fn prepare_asset_image(asset: &ManifestRenderAsset) -> Result<Option<PreparedImage>, String> {
    if matches!(
        asset.locator,
        ManifestRenderAssetLocator::ProceduralAttachmentOriginal { .. }
    ) && asset.media_type == "application/pdf"
    {
        lopdf::Document::load_mem(&asset.bytes)
            .map_err(|error| format!("procedural attachment PDF decode failed: {error}"))?;
        return Ok(None);
    }
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
    Ok(Some(PreparedImage {
        png_bytes: png.into_inner(),
        width,
        height,
    }))
}

fn manifest_to_docx(title: &str, document: &PreparedDocument<'_>) -> Result<Vec<u8>, String> {
    let mut docx = Docx::new().add_paragraph(heading(title, 36));
    for part in &document.parts {
        docx = docx.add_paragraph(heading(part.part_key, 28));
        for asset_index in &part.bid_shot_asset_indexes {
            docx = docx.add_paragraph(image_paragraph(
                document.assets[*asset_index]
                    .image
                    .as_ref()
                    .expect("bid shots are validated images"),
            ));
        }
        let mut line_index = 0usize;
        while line_index < part.lines.len() {
            if part.part_key == "6:quote"
                && let Some((rows, consumed)) = markdown_table_at(&part.lines, line_index)
            {
                docx = docx.add_table(docx_table(&rows));
                line_index += consumed;
                continue;
            }
            let line = &part.lines[line_index];
            if !line.text.trim().is_empty() {
                if let Some(rest) = line.text.strip_prefix("# ") {
                    docx = docx.add_paragraph(heading(rest, 28));
                } else {
                    docx = docx.add_paragraph(paragraph(&line.text));
                }
            }
            for asset_index in &line.markdown_asset_indexes {
                docx = docx.add_paragraph(image_paragraph(
                    document.assets[*asset_index]
                        .image
                        .as_ref()
                        .expect("markdown assets are validated images"),
                ));
            }
            line_index += 1;
        }
        if !part.procedural_asset_indexes.is_empty() {
            docx = docx.add_paragraph(heading("已确认程序附件", 24));
            for asset_index in &part.procedural_asset_indexes {
                if let Some(image) = &document.assets[*asset_index].image {
                    docx = docx.add_paragraph(image_paragraph(image));
                }
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
    const MAX_WIDTH_PX: u32 = 560;
    const MAX_HEIGHT_PX: u32 = 870;
    let (draw_w, draw_h) =
        fit_image_dimensions(image.width, image.height, MAX_WIDTH_PX, MAX_HEIGHT_PX);
    Paragraph::new().add_run(Run::new().add_image(Pic::new_with_dimensions(
        image.png_bytes.clone(),
        draw_w,
        draw_h,
    )))
}

fn markdown_table_at(lines: &[PreparedLine], start: usize) -> Option<(Vec<Vec<String>>, usize)> {
    let header = parse_markdown_table_row(lines.get(start)?.text.as_str())?;
    let delimiter = parse_markdown_table_row(lines.get(start + 1)?.text.as_str())?;
    if header.is_empty()
        || header.len() != delimiter.len()
        || !delimiter.iter().all(|cell| {
            let value = cell.trim();
            value.contains('-')
                && value
                    .chars()
                    .all(|character| matches!(character, '-' | ':'))
        })
    {
        return None;
    }
    let mut rows = vec![header];
    let mut consumed = 2usize;
    while let Some(line) = lines.get(start + consumed) {
        let Some(row) = parse_markdown_table_row(&line.text) else {
            break;
        };
        if row.len() != rows[0].len() {
            break;
        }
        rows.push(row);
        consumed += 1;
    }
    (rows.len() > 1).then_some((rows, consumed))
}

fn parse_markdown_table_row(line: &str) -> Option<Vec<String>> {
    let value = line.trim();
    if !value.starts_with('|') || !value.ends_with('|') {
        return None;
    }
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut escaped = false;
    for character in value[1..value.len() - 1].chars() {
        if escaped {
            cell.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == '|' {
            cells.push(cell.trim().to_string());
            cell.clear();
        } else {
            cell.push(character);
        }
    }
    if escaped {
        cell.push('\\');
    }
    cells.push(cell.trim().to_string());
    Some(cells)
}

fn docx_table(rows: &[Vec<String>]) -> Table {
    let column_count = rows.first().map(Vec::len).unwrap_or(1).max(1);
    let grid_width = 9000usize / column_count;
    Table::new(
        rows.iter()
            .enumerate()
            .map(|(row_index, row)| {
                TableRow::new(
                    row.iter()
                        .map(|cell| {
                            let run = Run::new().add_text(cell).size(16);
                            let run = if row_index == 0 { run.bold() } else { run };
                            TableCell::new().add_paragraph(Paragraph::new().add_run(run))
                        })
                        .collect(),
                )
            })
            .collect(),
    )
    .set_grid(vec![grid_width; column_count])
}

fn fit_image_dimensions(width: u32, height: u32, max_width: u32, max_height: u32) -> (u32, u32) {
    let scale = (f64::from(max_width) / f64::from(width))
        .min(f64::from(max_height) / f64::from(height))
        .min(1.0);
    let scaled = |value: u32, maximum: u32| {
        (f64::from(value) * scale)
            .round()
            .clamp(1.0, f64::from(maximum)) as u32
    };
    (scaled(width, max_width), scaled(height, max_height))
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
        .insert(font.clone(), printpdf::font::PdfFont::new(parsed.clone()));
    let mut pages: Vec<Vec<Op>> = Vec::new();
    let mut ops: Vec<Op> = Vec::new();
    let mut y = PDF_PAGE_HEIGHT - PDF_MARGIN;
    write_pdf_line(&mut ops, &mut pages, &mut y, &font, &parsed, title, 18.0);
    for part in &document.parts {
        write_pdf_line(
            &mut ops,
            &mut pages,
            &mut y,
            &font,
            &parsed,
            part.part_key,
            14.0,
        );
        for asset_index in &part.bid_shot_asset_indexes {
            add_pdf_image(
                &mut pdf,
                &mut ops,
                &mut pages,
                &mut y,
                &document.assets[*asset_index],
            )?;
        }
        let mut line_index = 0usize;
        while line_index < part.lines.len() {
            if part.part_key == "6:quote"
                && let Some((rows, consumed)) = markdown_table_at(&part.lines, line_index)
            {
                add_pdf_table(&mut ops, &mut pages, &mut y, &font, &parsed, &rows);
                line_index += consumed;
                continue;
            }
            let line = &part.lines[line_index];
            if !line.text.trim().is_empty() {
                let size = if line.text.starts_with("# ") {
                    14.0
                } else {
                    11.0
                };
                let text = line.text.strip_prefix("# ").unwrap_or(&line.text);
                write_pdf_line(&mut ops, &mut pages, &mut y, &font, &parsed, text, size);
            }
            for asset_index in &line.markdown_asset_indexes {
                add_pdf_image(
                    &mut pdf,
                    &mut ops,
                    &mut pages,
                    &mut y,
                    &document.assets[*asset_index],
                )?;
            }
            line_index += 1;
        }
        if !part.procedural_asset_indexes.is_empty() {
            write_pdf_line(
                &mut ops,
                &mut pages,
                &mut y,
                &font,
                &parsed,
                "已确认程序附件",
                12.0,
            );
            for asset_index in &part.procedural_asset_indexes {
                if document.assets[*asset_index].image.is_some() {
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
    }
    if !ops.is_empty() {
        pages.push(ops);
    }
    let pdf_pages: Vec<PdfPage> = pages
        .into_iter()
        .map(|ops| PdfPage::new(Mm(PDF_PAGE_WIDTH), Mm(PDF_PAGE_HEIGHT), ops))
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

const PDF_PAGE_WIDTH: f32 = 210.0;
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
    parsed_font: &printpdf::ParsedFont,
    text: &str,
    size: f32,
) {
    use printpdf::{Mm, Op, PdfFontHandle, Point, Pt, TextItem};
    for wrapped_line in wrap_pdf_text(text, size, parsed_font) {
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
                items: vec![TextItem::Text(wrapped_line)],
            },
            Op::EndTextSection,
        ]);
        *y -= size * 0.45;
    }
}

fn wrap_pdf_text(text: &str, size_pt: f32, font: &printpdf::ParsedFont) -> Vec<String> {
    let max_width_mm = PDF_PAGE_WIDTH - PDF_MARGIN * 2.0;
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut line_width_mm = 0.0f32;
    for character in text.chars() {
        let width_mm = pdf_glyph_width_mm(character, size_pt, font);
        if !line.is_empty() && line_width_mm + width_mm > max_width_mm {
            lines.push(std::mem::take(&mut line));
            line_width_mm = 0.0;
        }
        line.push(character);
        line_width_mm += width_mm;
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

fn pdf_glyph_width_mm(character: char, size_pt: f32, font: &printpdf::ParsedFont) -> f32 {
    let units_per_em = font.font_metrics.units_per_em.max(1);
    let advance_em = font
        .lookup_glyph_index(character as u32)
        .map(|glyph| font.get_horizontal_advance(glyph))
        .filter(|width| *width > 0)
        .map(|width| f32::from(width) / f32::from(units_per_em))
        .unwrap_or_else(|| if character.is_ascii() { 0.6 } else { 1.0 });
    advance_em * size_pt * 25.4 / 72.0
}

fn add_pdf_table(
    ops: &mut Vec<printpdf::Op>,
    pages: &mut Vec<Vec<printpdf::Op>>,
    y: &mut f32,
    font: &printpdf::FontId,
    parsed_font: &printpdf::ParsedFont,
    rows: &[Vec<String>],
) {
    use printpdf::{Line, LinePoint, Mm, Op, PdfFontHandle, Point, Pt, TextItem};
    let column_count = rows.first().map(Vec::len).unwrap_or(1).max(1);
    let table_width = PDF_PAGE_WIDTH - PDF_MARGIN * 2.0;
    let column_width = table_width / column_count as f32;
    let font_size = 7.0f32;
    let line_height = 3.4f32;
    let wrap_cell = |value: &str| {
        let max_width = (column_width - 2.0).max(1.0);
        let mut wrapped = Vec::new();
        let mut line = String::new();
        let mut width = 0.0f32;
        for character in value.chars() {
            let glyph_width = pdf_glyph_width_mm(character, font_size, parsed_font);
            if !line.is_empty() && width + glyph_width > max_width {
                wrapped.push(std::mem::take(&mut line));
                width = 0.0;
            }
            line.push(character);
            width += glyph_width;
        }
        if !line.is_empty() || wrapped.is_empty() {
            wrapped.push(line);
        }
        wrapped
    };

    for row in rows {
        let wrapped = row.iter().map(|cell| wrap_cell(cell)).collect::<Vec<_>>();
        let line_count = wrapped.iter().map(Vec::len).max().unwrap_or(1);
        let full_row_height = line_count as f32 * line_height + 2.0;
        let page_content_height = PDF_PAGE_HEIGHT - PDF_MARGIN * 2.0;
        if full_row_height <= page_content_height && *y - full_row_height < PDF_MARGIN {
            if !ops.is_empty() {
                pages.push(std::mem::take(ops));
            }
            *y = PDF_PAGE_HEIGHT - PDF_MARGIN;
        }

        let mut first_line = 0usize;
        while first_line < line_count {
            if *y - (line_height + 2.0) < PDF_MARGIN {
                if !ops.is_empty() {
                    pages.push(std::mem::take(ops));
                }
                *y = PDF_PAGE_HEIGHT - PDF_MARGIN;
            }
            let available_lines = ((*y - PDF_MARGIN - 2.0) / line_height).floor() as usize;
            let rendered_lines = (line_count - first_line).min(available_lines.max(1));
            let row_height = rendered_lines as f32 * line_height + 2.0;
            let top = *y;
            let bottom = top - row_height;
            ops.push(Op::SetOutlineThickness { pt: Pt(0.5) });
            for column in 0..=column_count {
                let x = PDF_MARGIN + column as f32 * column_width;
                ops.push(Op::DrawLine {
                    line: Line {
                        points: vec![
                            LinePoint {
                                p: Point::new(Mm(x), Mm(top)),
                                bezier: false,
                            },
                            LinePoint {
                                p: Point::new(Mm(x), Mm(bottom)),
                                bezier: false,
                            },
                        ],
                        is_closed: false,
                    },
                });
            }
            for horizontal_y in [top, bottom] {
                ops.push(Op::DrawLine {
                    line: Line {
                        points: vec![
                            LinePoint {
                                p: Point::new(Mm(PDF_MARGIN), Mm(horizontal_y)),
                                bezier: false,
                            },
                            LinePoint {
                                p: Point::new(Mm(PDF_PAGE_WIDTH - PDF_MARGIN), Mm(horizontal_y)),
                                bezier: false,
                            },
                        ],
                        is_closed: false,
                    },
                });
            }
            for (column, cell_lines) in wrapped.iter().enumerate() {
                let x = PDF_MARGIN + column as f32 * column_width + 1.0;
                for (line_index, text) in cell_lines
                    .iter()
                    .skip(first_line)
                    .take(rendered_lines)
                    .enumerate()
                {
                    let text_y = top - 1.5 - line_index as f32 * line_height;
                    ops.extend([
                        Op::StartTextSection,
                        Op::SetTextCursor {
                            pos: Point::new(Mm(x), Mm(text_y)),
                        },
                        Op::SetFont {
                            font: PdfFontHandle::External(font.clone()),
                            size: Pt(font_size),
                        },
                        Op::ShowText {
                            items: vec![TextItem::Text(text.clone())],
                        },
                        Op::EndTextSection,
                    ]);
                }
            }
            first_line += rendered_lines;
            *y = bottom;
            if first_line < line_count {
                pages.push(std::mem::take(ops));
                *y = PDF_PAGE_HEIGHT - PDF_MARGIN;
            }
        }
    }
    *y -= 2.0;
}

fn add_pdf_image(
    pdf: &mut printpdf::PdfDocument,
    ops: &mut Vec<printpdf::Op>,
    pages: &mut Vec<Vec<printpdf::Op>>,
    y: &mut f32,
    asset: &PreparedAsset<'_>,
) -> Result<(), String> {
    use printpdf::{Mm, Op, RawImage, XObject, XObjectId, XObjectTransform};
    let image = asset
        .image
        .as_ref()
        .ok_or_else(|| "non-image manifest asset reached PDF image renderer".to_string())?;
    let mut warnings = Vec::new();
    let raw_image = RawImage::decode_from_bytes(&image.png_bytes, &mut warnings)
        .map_err(|error| format!("normalized manifest image decode failed: {error}"))?;
    let id = XObjectId(format!("kb-bid-image-v1-{}", asset.source.manifest_ordinal));
    let max_w_mm = PDF_PAGE_WIDTH - PDF_MARGIN * 2.0;
    let max_h_mm = PDF_PAGE_HEIGHT - PDF_MARGIN * 2.0;
    let nat_w = raw_image.width.max(1) as f32 * 25.4 / 150.0;
    let nat_h = raw_image.height.max(1) as f32 * 25.4 / 150.0;
    let scale = (max_w_mm / nat_w).min(max_h_mm / nat_h).min(1.0);
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
            ManifestRenderAssetLocator::ProceduralAttachmentOriginal {
                part_key,
                attachment_ordinal,
                attachment_id,
                kind,
            } => {
                hash.update([2]);
                hash_pdf_field(&mut hash, part_key.as_bytes());
                hash.update(attachment_ordinal.to_be_bytes());
                hash.update(attachment_id.as_bytes());
                hash_pdf_field(&mut hash, kind.as_bytes());
            }
            ManifestRenderAssetLocator::ProceduralAttachmentPage {
                part_key,
                attachment_ordinal,
                attachment_id,
                page_ordinal,
            } => {
                hash.update([3]);
                hash_pdf_field(&mut hash, part_key.as_bytes());
                hash.update(attachment_ordinal.to_be_bytes());
                hash.update(attachment_id.as_bytes());
                hash.update(page_ordinal.to_be_bytes());
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
        png_bytes_with_dimensions(2, 2, color)
    }

    fn png_bytes_with_dimensions(width: u32, height: u32, color: [u8; 4]) -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(width, height, image::Rgba(color));
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

    fn markdown_asset_with_dimensions(
        manifest_ordinal: u32,
        part_key: &str,
        occurrence_ordinal: u32,
        width: u32,
        height: u32,
    ) -> ManifestRenderAsset {
        let bytes = png_bytes_with_dimensions(width, height, [10, 20, 30, 255]);
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

    fn procedural_original(
        manifest_ordinal: u32,
        part_key: &str,
        attachment_ordinal: u32,
        attachment_id: Uuid,
        kind: &str,
        media_type: &str,
        bytes: Vec<u8>,
    ) -> ManifestRenderAsset {
        let digest = domain::sha256_hex(&bytes);
        ManifestRenderAsset {
            manifest_ordinal,
            locator: ManifestRenderAssetLocator::ProceduralAttachmentOriginal {
                part_key: part_key.into(),
                attachment_ordinal,
                attachment_id,
                kind: kind.into(),
            },
            object_ref: format!("objects/{digest}"),
            digest,
            media_type: media_type.into(),
            byte_length: bytes.len() as u64,
            bytes,
        }
    }

    fn procedural_page(
        manifest_ordinal: u32,
        part_key: &str,
        attachment_ordinal: u32,
        attachment_id: Uuid,
        page_ordinal: u32,
    ) -> ManifestRenderAsset {
        let bytes = png_bytes([90, 100, 110, 255]);
        let digest = domain::sha256_hex(&bytes);
        ManifestRenderAsset {
            manifest_ordinal,
            locator: ManifestRenderAssetLocator::ProceduralAttachmentPage {
                part_key: part_key.into(),
                attachment_ordinal,
                attachment_id,
                page_ordinal,
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
            format!(
                "说明 ![第一张]({})\n再次 ![第二张]({})",
                first.object_ref, second.object_ref
            ),
        )];
        let assets = vec![first, second];

        validate_manifest_render_assets(&parts, &assets).expect("valid assets");
        let docx = render_manifest_document(GateFormat::Docx, "投标文件", &parts, &assets)
            .expect("render DOCX");
        assert!(docx.starts_with(b"PK"));
    }

    #[test]
    fn freezes_and_renders_image_and_pdf_procedural_attachments() {
        let image_id = Uuid::from_u128(41);
        let pdf_id = Uuid::from_u128(42);
        let original_pdf = render_manifest_document(
            GateFormat::Pdf,
            "附件原件",
            &[("1".into(), "PDF 原件".into())],
            &[],
        )
        .expect("build valid PDF fixture");
        let assets = vec![
            procedural_original(
                0,
                "6:authorization",
                0,
                image_id,
                "authorization_support",
                "image/png",
                png_bytes([10, 20, 30, 255]),
            ),
            procedural_original(
                1,
                "6:procedural",
                1,
                pdf_id,
                "bid_bond",
                "application/pdf",
                original_pdf,
            ),
            procedural_page(2, "6:procedural", 1, pdf_id, 0),
        ];
        let parts = vec![
            ("6:authorization".into(), "授权材料".into()),
            ("6:procedural".into(), "程序材料".into()),
        ];

        validate_manifest_render_assets(&parts, &assets).expect("valid frozen attachments");
        let docx = render_manifest_document(GateFormat::Docx, "投标文件", &parts, &assets)
            .expect("render attachments to DOCX");
        let parsed = docx_rs::read_docx(&docx).expect("parse DOCX");
        assert_eq!(parsed.images.len(), 2, "image original and PDF page render");
        let pdf = render_manifest_document(GateFormat::Pdf, "投标文件", &parts, &assets)
            .expect("render attachments to PDF");
        assert!(pdf.starts_with(b"%PDF"));
    }

    #[test]
    fn rejects_pdf_attachment_without_frozen_pages() {
        let attachment_id = Uuid::from_u128(43);
        let original_pdf = render_manifest_document(
            GateFormat::Pdf,
            "附件原件",
            &[("1".into(), "PDF 原件".into())],
            &[],
        )
        .expect("build valid PDF fixture");
        let error = validate_manifest_render_assets(
            &[("6:procedural".into(), "程序材料".into())],
            &[procedural_original(
                0,
                "6:procedural",
                0,
                attachment_id,
                "bid_bond",
                "application/pdf",
                original_pdf,
            )],
        )
        .expect_err("PDF without frozen pages must fail");
        assert!(error.contains("no frozen render pages"));
    }

    #[test]
    fn quote_markdown_table_becomes_docx_table_and_pdf_grid() {
        let markdown = concat!(
            "# 报价表\n\n",
            "| 序号 | 说明 | 数量 | 含税金额 |\n",
            "| ---: | --- | ---: | ---: |\n",
            "| 1 | 核心设备\\|含安装 | 2 | 1200.00 |\n",
            "\n- 含税合计：1200.00\n"
        );
        let parts = vec![("6:quote".into(), markdown.into())];
        let docx = render_manifest_document(GateFormat::Docx, "投标文件", &parts, &[])
            .expect("render quote DOCX table");
        let parsed_docx = docx_rs::read_docx(&docx).expect("parse quote DOCX");
        let document_json = serde_json::to_string(&parsed_docx.document).expect("serialize DOCX");
        assert!(document_json.contains("核心设备|含安装"));
        assert!(
            document_json.contains("\"rows\""),
            "quote must render as a DOCX table"
        );

        let pdf = render_manifest_document(GateFormat::Pdf, "投标文件", &parts, &[])
            .expect("render quote PDF grid");
        let mut warnings = Vec::new();
        let parsed_pdf = printpdf::PdfDocument::parse(
            &pdf,
            &printpdf::PdfParseOptions::default(),
            &mut warnings,
        )
        .expect("parse quote PDF");
        assert!(
            parsed_pdf
                .pages
                .iter()
                .flat_map(|page| &page.ops)
                .any(|operation| matches!(operation, printpdf::Op::DrawLine { .. })),
            "quote must render with an explicit PDF grid"
        );
    }

    #[test]
    fn quote_pdf_grid_paginates_rows_taller_than_a4_content_height() {
        let description = format!("{}尾", "超长报价说明".repeat(800));
        let markdown = format!(
            "# 报价表\n\n| 序号 | 说明 | 数量 | 含税金额 |\n| ---: | --- | ---: | ---: |\n| 1 | {description} | 1 | 1200.00 |"
        );
        let pdf = render_manifest_document(
            GateFormat::Pdf,
            "投标文件",
            &[("6:quote".into(), markdown)],
            &[],
        )
        .expect("paginate a quote row taller than one PDF page");
        let mut warnings = Vec::new();
        let parsed = printpdf::PdfDocument::parse(
            &pdf,
            &printpdf::PdfParseOptions::default(),
            &mut warnings,
        )
        .expect("parse paginated quote PDF");

        assert!(parsed.pages.len() > 2, "the tall quote row must span pages");
        let minimum_y = printpdf::Mm(PDF_MARGIN).into_pt().0 - 0.01;
        for operation in parsed.pages.iter().flat_map(|page| &page.ops) {
            match operation {
                printpdf::Op::DrawLine { line } => {
                    assert!(
                        line.points.iter().all(|point| point.p.y.0 >= minimum_y),
                        "PDF grid must stay inside the A4 content box"
                    );
                }
                printpdf::Op::SetTextCursor { pos } => {
                    assert!(
                        pos.y.0 >= minimum_y,
                        "PDF table text must stay inside the A4 content box"
                    );
                }
                _ => {}
            }
        }
    }

    #[test]
    fn rejects_missing_and_unexpected_markdown_occurrences() {
        let expected = markdown_asset(0, "1", 0, [10, 20, 30, 255]);
        let parts = vec![("1".into(), format!("![]({})", expected.object_ref))];
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
            format!(
                "![]({})\n![]({})",
                duplicate[0].object_ref, duplicate[1].object_ref
            ),
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
        let parts = vec![("1".into(), format!("![]({})", expected.object_ref))];
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
        let parts = vec![(
            "3".into(),
            format!("产品图片 ![产品]({})", markdown.object_ref),
        )];
        let assets = vec![shot, markdown];

        let document = prepare_manifest(&parts, &assets).expect("prepare manifest");
        assert_eq!(document.parts[0].bid_shot_asset_indexes, vec![0]);
        assert_eq!(document.parts[0].lines[0].markdown_asset_indexes, vec![1]);
    }

    #[test]
    fn rejects_manifest_order_that_places_markdown_before_bid_shots() {
        let markdown = markdown_asset(0, "3", 0, [40, 50, 60, 255]);
        let shot = bid_shot(1, 0, [10, 20, 30, 255]);
        let parts = vec![(
            "3".into(),
            format!("产品图片 ![产品]({})", markdown.object_ref),
        )];

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
        let parts = vec![(
            "3".into(),
            format!("中文投标文件 ![产品]({})", markdown.object_ref),
        )];
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
            "62e3fdf66291fcb2b67add3c9bb620fc17303f7ff39529e0c7f284585d471e85"
        );
    }

    #[test]
    fn markdown_image_nodes_are_consumed_but_bare_refs_remain_text() {
        let asset = markdown_asset(0, "1", 0, [10, 20, 30, 255]);
        let parts = vec![(
            "1".into(),
            format!(
                "前文 ![证据图]({}) 后文；裸引用 {}",
                asset.object_ref, asset.object_ref
            ),
        )];
        let bytes = render_manifest_document(GateFormat::Docx, "投标文件", &parts, &[asset])
            .expect("render DOCX");
        let parsed = docx_rs::read_docx(&bytes).expect("parse rendered DOCX");
        let rendered = serde_json::to_string(&parsed.document).expect("serialize DOCX body");

        assert!(rendered.contains("前文 "));
        assert!(rendered.contains(" 后文；裸引用 objects/"));
        assert!(!rendered.contains("![证据图]"));
        assert_eq!(parsed.images.len(), 1, "image-only nodes must still render");
    }

    #[test]
    fn pdf_wraps_long_text_within_a4_content_width() {
        let long_line = "中".repeat(400);
        let bytes =
            render_manifest_document(GateFormat::Pdf, "投标文件", &[("1".into(), long_line)], &[])
                .expect("render wrapped PDF");
        let mut warnings = Vec::new();
        let parsed = printpdf::PdfDocument::parse(
            &bytes,
            &printpdf::PdfParseOptions::default(),
            &mut warnings,
        )
        .expect("parse rendered PDF");
        let shown_lines = parsed
            .pages
            .iter()
            .flat_map(|page| &page.ops)
            .filter(|op| matches!(op, printpdf::Op::ShowText { .. }))
            .count();

        assert!(
            shown_lines > 4,
            "long content must be emitted as wrapped lines"
        );
    }

    #[test]
    fn tall_images_fit_docx_and_pdf_pages_without_aspect_ratio_crop() {
        let asset = markdown_asset_with_dimensions(0, "1", 0, 20, 2_000);
        let parts = vec![("1".into(), format!("![]({})", asset.object_ref))];

        let docx = render_manifest_document(
            GateFormat::Docx,
            "投标文件",
            &parts,
            std::slice::from_ref(&asset),
        )
        .expect("render tall DOCX image");
        let parsed_docx = docx_rs::read_docx(&docx).expect("parse rendered DOCX");
        let picture = parsed_docx
            .document
            .children
            .iter()
            .filter_map(|child| match child {
                docx_rs::DocumentChild::Paragraph(paragraph) => Some(paragraph),
                _ => None,
            })
            .flat_map(|paragraph| &paragraph.children)
            .filter_map(|child| match child {
                docx_rs::ParagraphChild::Run(run) => Some(run),
                _ => None,
            })
            .flat_map(|run| &run.children)
            .find_map(|child| match child {
                docx_rs::RunChild::Drawing(drawing) => match &drawing.data {
                    Some(docx_rs::DrawingData::Pic(pic)) => Some(pic),
                    _ => None,
                },
                _ => None,
            })
            .expect("DOCX picture");
        const EMU_PER_PIXEL: u32 = 9_525;
        assert!(picture.size.0 <= 560 * EMU_PER_PIXEL);
        assert!(picture.size.1 <= 870 * EMU_PER_PIXEL);
        assert!(
            (u64::from(picture.size.0) * 2_000).abs_diff(u64::from(picture.size.1) * 20)
                <= u64::from(picture.size.1)
        );

        let pdf = render_manifest_document(GateFormat::Pdf, "投标文件", &parts, &[asset])
            .expect("render tall PDF image");
        let mut warnings = Vec::new();
        let parsed_pdf = printpdf::PdfDocument::parse(
            &pdf,
            &printpdf::PdfParseOptions::default(),
            &mut warnings,
        )
        .expect("parse rendered PDF");
        let [draw_width_pt, skew_x, skew_y, draw_height_pt, _, _] = parsed_pdf
            .pages
            .iter()
            .flat_map(|page| &page.ops)
            .find_map(|op| match op {
                printpdf::Op::SetTransformationMatrix { matrix } => Some(matrix.as_array()),
                _ => None,
            })
            .expect("PDF image output matrix");
        let max_width_pt = printpdf::Mm(PDF_PAGE_WIDTH - PDF_MARGIN * 2.0).into_pt().0;
        let max_height_pt = printpdf::Mm(PDF_PAGE_HEIGHT - PDF_MARGIN * 2.0).into_pt().0;
        assert!(skew_x.abs() < f32::EPSILON && skew_y.abs() < f32::EPSILON);
        assert!(draw_width_pt <= max_width_pt + 0.01);
        assert!(draw_height_pt <= max_height_pt + 0.01);
        assert!(
            (draw_width_pt * 2_000.0 - draw_height_pt * 20.0).abs() < 0.01,
            "PDF output must preserve the image aspect ratio"
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
