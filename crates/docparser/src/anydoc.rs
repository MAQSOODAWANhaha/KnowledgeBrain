//! Brain `AnydocReader`: office → Markdown in this process. Not DocReader.

use std::collections::HashMap;

use crate::{ImageRef, ReadResult};

pub const ENGINE: &str = "anydoc";
pub const IMAGE_DIR: &str = "images/";

pub fn version() -> &'static str {
    "0.1.9"
}

const SUPPORTED: &[&str] = &[
    "csv", "doc", "docm", "docx", "epub", "odp", "ods", "odt", "pdf", "ppt", "pptm", "pptx", "rtf",
    "xls", "xlsm", "xlsx",
];

pub fn supported_file_types() -> &'static [&'static str] {
    SUPPORTED
}

pub fn supports(file_type: &str, file_name: &str) -> bool {
    format_for_file(file_type, file_name).is_some()
}

pub fn format_for_file(file_type: &str, file_name: &str) -> Option<anydoc::Format> {
    let ext = normalize_ext(file_type);
    let ext = if ext.is_empty() {
        file_name
            .rsplit('.')
            .next()
            .map(normalize_ext)
            .unwrap_or_default()
    } else {
        ext
    };
    anydoc::Format::from_extension(&ext)
}

pub fn pdf_needs_ocr(err: &anydoc::ConvertError) -> bool {
    let msg = err.to_string().to_ascii_lowercase();
    msg.contains("ocr is required") || msg.contains("no extractable text")
}

pub fn is_falsey(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "false" | "0" | "no" | "off"
    )
}

/// Brain `anydoc_extract_images` override, then `ANYDOC_EXTRACT_IMAGES`.
pub fn extract_images_enabled(overrides: &HashMap<String, String>) -> bool {
    if let Some(v) = overrides.get("anydoc_extract_images") {
        return !is_falsey(v);
    }
    !is_falsey(&std::env::var("ANYDOC_EXTRACT_IMAGES").unwrap_or_else(|_| "true".into()))
}

pub fn format_name(format: anydoc::Format) -> &'static str {
    match format {
        anydoc::Format::Doc => "doc",
        anydoc::Format::Docx => "docx",
        anydoc::Format::Odt => "odt",
        anydoc::Format::Pdf => "pdf",
        anydoc::Format::Ppt => "ppt",
        anydoc::Format::Pptx => "pptx",
        anydoc::Format::Rtf => "rtf",
        anydoc::Format::Epub => "epub",
        anydoc::Format::Excel => "xlsx",
        anydoc::Format::Ods => "ods",
        anydoc::Format::Odp => "odp",
        anydoc::Format::Csv => "csv",
    }
}

fn with_meta(mut result: ReadResult, format: anydoc::Format) -> ReadResult {
    result.metadata.insert("parser".into(), ENGINE.into());
    result
        .metadata
        .insert("anydoc_version".into(), version().into());
    result
        .metadata
        .insert("source_format".into(), format_name(format).into());
    result
}

/// Brain `AnydocReader.Read`. URLs are rejected. Scanned PDFs return `error`
/// so the convert router can fall back to builtin.
pub fn convert(
    file_name: &str,
    file_type: &str,
    bytes: &[u8],
    is_url: bool,
    extract_images: bool,
) -> ReadResult {
    if is_url && bytes.is_empty() {
        return ReadResult {
            error: "anydoc engine reads uploaded documents, not URLs".into(),
            ..ReadResult::default()
        };
    }
    if bytes.is_empty() {
        return ReadResult {
            error: "anydoc: empty document".into(),
            ..ReadResult::default()
        };
    }
    let Some(format) = format_for_file(file_type, file_name) else {
        return ReadResult {
            error: format!("anydoc engine does not support file type {file_type:?}"),
            ..ReadResult::default()
        };
    };
    let want_images = extract_images && format != anydoc::Format::Pdf;
    if want_images {
        return convert_with_assets(file_name, bytes, format);
    }
    match anydoc::to_markdown_bytes(bytes, Some(format)) {
        Ok(markdown) if format == anydoc::Format::Pdf && markdown.trim().is_empty() => {
            let mut r = ReadResult {
                error: "anydoc conversion failed: PDF has no extractable text; OCR is required"
                    .into(),
                ..ReadResult::default()
            };
            r.metadata
                .insert("source_format".into(), format_name(format).into());
            r
        }
        Ok(markdown) => with_meta(
            ReadResult {
                markdown,
                ..ReadResult::default()
            },
            format,
        ),
        Err(e) if format == anydoc::Format::Pdf && pdf_needs_ocr(&e) => ReadResult {
            error: format!("anydoc conversion failed for {file_name:?}: {e}"),
            ..ReadResult::default()
        },
        Err(e) => ReadResult {
            error: format!("anydoc conversion failed for {file_name:?}: {e}"),
            ..ReadResult::default()
        },
    }
}

/// Brain `ToMarkdownWithAssetLinks`: rewrite `ImageSource::Asset` to
/// `images/image-N.ext` then serialize so GFM keeps pictures in place.
fn convert_with_assets(file_name: &str, bytes: &[u8], format: anydoc::Format) -> ReadResult {
    match anydoc::to_document(bytes, Some(format)) {
        Ok(mut doc) => {
            let images = images_from_document(&doc);
            rewrite_asset_images(&mut doc);
            let markdown = document_to_gfm(&doc);
            with_meta(
                ReadResult {
                    markdown,
                    images,
                    ..ReadResult::default()
                },
                format,
            )
        }
        Err(e) => match anydoc::to_markdown_bytes(bytes, Some(format)) {
            Ok(markdown) => {
                let mut r = with_meta(
                    ReadResult {
                        markdown,
                        ..ReadResult::default()
                    },
                    format,
                );
                r.metadata
                    .insert("anydoc_assets_error".into(), e.to_string());
                r
            }
            Err(md_err) => ReadResult {
                error: format!("anydoc conversion failed for {file_name:?}: {md_err}"),
                ..ReadResult::default()
            },
        },
    }
}

fn images_from_document(doc: &anydoc::model::Document) -> Vec<ImageRef> {
    let mut images = Vec::new();
    for (i, asset) in doc.assets.iter().enumerate() {
        if asset.bytes.is_empty() || !asset.media_type.starts_with("image/") {
            continue;
        }
        let name = format!("image-{}{}", i + 1, extension_for(&asset.media_type));
        let original = format!("{IMAGE_DIR}{name}");
        images.push(ImageRef {
            filename: name,
            original_ref: original,
            mime_type: asset.media_type.clone(),
            storage_key: String::new(),
            data: asset.bytes.clone(),
        });
    }
    images
}

fn rewrite_asset_images(document: &mut anydoc::model::Document) {
    let urls: Vec<Option<String>> = document
        .assets
        .iter()
        .enumerate()
        .map(|(i, asset)| {
            if asset.bytes.is_empty() {
                None
            } else {
                Some(format!(
                    "{IMAGE_DIR}image-{}{}",
                    i + 1,
                    extension_for(&asset.media_type)
                ))
            }
        })
        .collect();
    rewrite_blocks(&mut document.blocks, &urls);
    for note in &mut document.notes {
        rewrite_blocks(&mut note.blocks, &urls);
    }
}

fn rewrite_blocks(blocks: &mut [anydoc::model::Block], urls: &[Option<String>]) {
    use anydoc::model::{Block, CellSlot, ImageSource, Inline};
    for block in blocks {
        match block {
            Block::Heading { content, .. } | Block::Paragraph(content) => {
                rewrite_inlines(content, urls);
            }
            Block::List(list) => {
                for item in &mut list.items {
                    rewrite_blocks(&mut item.blocks, urls);
                }
            }
            Block::Table(table) => {
                for row in &mut table.grid {
                    for slot in row {
                        if let CellSlot::Origin(cell) = slot {
                            rewrite_blocks(&mut cell.blocks, urls);
                        }
                    }
                }
            }
            Block::BlockQuote(inner) => rewrite_blocks(inner, urls),
            Block::CodeBlock { .. } | Block::Rule => {}
        }
    }
    fn rewrite_inlines(inlines: &mut [Inline], urls: &[Option<String>]) {
        for inline in inlines {
            match inline {
                Inline::Image { source, .. } => {
                    if let ImageSource::Asset(id) = source
                        && let Some(Some(url)) = urls.get(id.0)
                    {
                        *source = ImageSource::External(url.clone());
                    }
                }
                Inline::Link { content, .. } => rewrite_inlines(content, urls),
                _ => {}
            }
        }
    }
}

fn document_to_gfm(doc: &anydoc::model::Document) -> String {
    let mut parts = render_blocks(&doc.blocks);
    for note in &doc.notes {
        parts.extend(render_blocks(&note.blocks));
    }
    parts.join("\n\n")
}

fn render_blocks(blocks: &[anydoc::model::Block]) -> Vec<String> {
    blocks.iter().filter_map(render_block).collect()
}

fn render_block(block: &anydoc::model::Block) -> Option<String> {
    use anydoc::model::{Block, CellSlot, MarkerKind};
    match block {
        Block::Heading { level, content, .. } => {
            let text = render_inlines(content).trim().to_string();
            if text.is_empty() {
                return None;
            }
            let level = (*level).clamp(1, 6) as usize;
            Some(format!("{} {text}", "#".repeat(level)))
        }
        Block::Paragraph(inlines) => {
            let text = render_inlines(inlines);
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Block::List(list) => {
            if list.items.is_empty() {
                return None;
            }
            let mut lines = Vec::new();
            for (i, item) in list.items.iter().enumerate() {
                let n = list.start.saturating_add(i as u64);
                let label = item
                    .marker_label
                    .clone()
                    .unwrap_or_else(|| list.marker.label(n.max(1)));
                let prefix = if list.marker == MarkerKind::Bullet {
                    "-".to_string()
                } else {
                    label
                };
                let inner = render_blocks(&item.blocks);
                if inner.is_empty() {
                    lines.push(format!("{prefix} "));
                    continue;
                }
                lines.push(format!("{prefix} {}", inner[0]));
                for extra in inner.iter().skip(1) {
                    for line in extra.lines() {
                        lines.push(format!("  {line}"));
                    }
                }
            }
            Some(lines.join("\n"))
        }
        Block::Table(table) => {
            let mut rows: Vec<Vec<String>> = Vec::new();
            for row in &table.grid {
                let cells: Vec<String> = row
                    .iter()
                    .filter_map(|slot| match slot {
                        CellSlot::Origin(cell) => Some(
                            render_blocks(&cell.blocks)
                                .join(" ")
                                .replace('|', "\\|")
                                .replace('\n', " "),
                        ),
                        CellSlot::Covered { .. } => None,
                    })
                    .collect();
                if !cells.is_empty() {
                    rows.push(cells);
                }
            }
            if rows.is_empty() {
                return None;
            }
            let width = rows.iter().map(Vec::len).max().unwrap_or(0);
            if width == 0 {
                return None;
            }
            for row in &mut rows {
                row.resize(width, String::new());
            }
            let mut out = format!(
                "| {} |\n|{}|\n",
                rows[0].join(" | "),
                vec!["---"; width].join("|")
            );
            for row in rows.iter().skip(1) {
                out.push_str(&format!("| {} |\n", row.join(" | ")));
            }
            Some(out)
        }
        Block::BlockQuote(inner) => {
            let body = render_blocks(inner).join("\n\n");
            if body.is_empty() {
                return None;
            }
            Some(
                body.lines()
                    .map(|l| {
                        if l.is_empty() {
                            ">".to_string()
                        } else {
                            format!("> {l}")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
        }
        Block::CodeBlock { lang, text } => {
            let lang = lang.as_deref().unwrap_or("");
            let body = text.trim_end_matches('\n');
            Some(format!("```{lang}\n{body}\n```"))
        }
        Block::Rule => Some("---".to_string()),
    }
}

fn render_inlines(inlines: &[anydoc::model::Inline]) -> String {
    use anydoc::model::{ImageSource, Inline, LinkTarget};
    let mut out = String::new();
    for inline in inlines {
        match inline {
            Inline::Text { text, style } => {
                let mut s = text.clone();
                if style.code {
                    s = format!("`{s}`");
                }
                if style.bold {
                    s = format!("**{s}**");
                }
                if style.italic {
                    s = format!("*{s}*");
                }
                if style.strike {
                    s = format!("~~{s}~~");
                }
                out.push_str(&s);
            }
            Inline::Link { content, target } => {
                let label = render_inlines(content);
                let url = match target {
                    LinkTarget::External(u) | LinkTarget::Relative(u) | LinkTarget::Anchor(u) => {
                        u.as_str()
                    }
                };
                if url.is_empty() {
                    out.push_str(&label);
                } else {
                    out.push_str(&format!("[{label}]({url})"));
                }
            }
            Inline::Image { alt, source } => match source {
                ImageSource::External(url) => {
                    out.push_str(&format!("![{}]({url})", alt.trim()));
                }
                ImageSource::Asset(_) | ImageSource::Unavailable => {
                    out.push_str(alt.trim());
                }
            },
            Inline::LineBreak => out.push_str("\\\n"),
            Inline::Anchor(_) | Inline::NoteRef(_) => {}
        }
    }
    out
}

fn extension_for(media_type: &str) -> &'static str {
    match media_type.to_ascii_lowercase().as_str() {
        "image/jpeg" | "image/jpg" => ".jpg",
        "image/png" => ".png",
        "image/gif" => ".gif",
        "image/webp" => ".webp",
        "image/bmp" => ".bmp",
        "image/tiff" => ".tiff",
        "image/svg+xml" => ".svg",
        _ => ".bin",
    }
}

fn normalize_ext(s: &str) -> String {
    s.trim().trim_start_matches('.').to_ascii_lowercase()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn maps_brain_extensions() {
        assert_eq!(version(), "0.1.9");
        assert!(supports("docx", "a.docx"));
        assert!(supports("xlsx", "a.xlsx"));
        assert!(supports("xls", "legacy.xls"));
        assert!(supports("pptx", "deck.pptx"));
        assert!(supports("pdf", "x.pdf"));
        assert!(supports("csv", "t.csv"));
        assert!(!supports("md", "a.md"));
        assert!(!supports("html", "a.html"));
    }

    #[test]
    fn rejects_url() {
        let r = convert("a.docx", "docx", b"", true, true);
        assert!(r.error.contains("not URLs"), "{}", r.error);
    }

    #[test]
    fn converts_csv() {
        let r = convert("t.csv", "csv", b"a,b\n1,2\n", false, false);
        assert!(r.error.is_empty(), "{}", r.error);
        assert!(r.markdown.contains('a'), "{}", r.markdown);
        assert!(r.images.is_empty());
        assert_eq!(r.metadata.get("parser").map(String::as_str), Some("anydoc"));
        assert_eq!(
            r.metadata.get("anydoc_version").map(String::as_str),
            Some(version())
        );
        assert_eq!(
            r.metadata.get("source_format").map(String::as_str),
            Some("csv")
        );
    }

    #[test]
    fn url_with_bytes_still_converts() {
        let r = convert("t.csv", "csv", b"a,b\n1,2\n", true, false);
        assert!(r.error.is_empty(), "{}", r.error);
        assert_eq!(r.metadata.get("parser").map(String::as_str), Some("anydoc"));
    }

    #[test]
    fn extract_images_override_is_falsey() {
        let mut ov = HashMap::new();
        ov.insert("anydoc_extract_images".into(), "false".into());
        assert!(!extract_images_enabled(&ov));
        ov.insert("anydoc_extract_images".into(), "true".into());
        assert!(extract_images_enabled(&ov));
    }

    #[test]
    fn empty_is_error() {
        let r = convert("a.docx", "docx", b"", false, true);
        assert!(r.error.contains("empty"));
    }

    #[test]
    fn docx_keeps_embedded_image_in_place() {
        let bytes = sample_docx_with_image();
        let r = convert("report.docx", "docx", &bytes, false, true);
        assert!(r.error.is_empty(), "{}", r.error);
        assert_eq!(r.images.len(), 1, "{}", r.markdown);
        assert_eq!(r.images[0].filename, "image-1.png");
        assert_eq!(r.images[0].original_ref, "images/image-1.png");
        assert_eq!(r.images[0].data, one_pixel_png());
        let link = "![Shipping chart](images/image-1.png)";
        assert!(r.markdown.contains("# Quarterly report"), "{}", r.markdown);
        assert!(
            r.markdown.contains("Widgets shipped on time."),
            "{}",
            r.markdown
        );
        assert!(r.markdown.contains("Closing remarks."), "{}", r.markdown);
        assert!(r.markdown.contains(link), "{}", r.markdown);
        let before = r.markdown.find("Widgets shipped on time.").unwrap();
        let at = r.markdown.find(link).unwrap();
        let after = r.markdown.find("Closing remarks.").unwrap();
        assert!(before < at && at < after, "{}", r.markdown);
    }

    #[test]
    fn docx_table_renders_as_gfm() {
        let r = convert(
            "tender.docx",
            "docx",
            &sample_docx_with_table(),
            false,
            false,
        );
        assert!(r.error.is_empty(), "{}", r.error);
        assert_eq!(r.metadata.get("parser").map(String::as_str), Some(ENGINE));
        assert!(
            r.markdown.contains('|') && r.markdown.contains("hot-swap"),
            "{}",
            r.markdown
        );
        assert!(r.markdown.contains("validity"), "{}", r.markdown);
    }

    #[test]
    fn extract_images_off_does_not_emit_asset_link() {
        let bytes = sample_docx_with_image();
        let r = convert("report.docx", "docx", &bytes, false, false);
        assert!(r.error.is_empty(), "{}", r.error);
        assert!(r.images.is_empty());
        assert!(!r.markdown.contains("images/image-1.png"), "{}", r.markdown);
    }

    pub(crate) fn sample_docx_with_table() -> Vec<u8> {
        store_zip(&[
            (
                "[Content_Types].xml",
                br#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#,
            ),
            (
                "_rels/.rels",
                br#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#,
            ),
            (
                "word/document.xml",
                br#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p><w:r><w:t>Commercial terms</w:t></w:r></w:p>
    <w:tbl>
      <w:tblGrid><w:gridCol w:w="2000"/><w:gridCol w:w="4000"/></w:tblGrid>
      <w:tr>
        <w:tc><w:p><w:r><w:t>clause</w:t></w:r></w:p></w:tc>
        <w:tc><w:p><w:r><w:t>text</w:t></w:r></w:p></w:tc>
      </w:tr>
      <w:tr>
        <w:tc><w:p><w:r><w:t>validity</w:t></w:r></w:p></w:tc>
        <w:tc><w:p><w:r><w:t>90 days</w:t></w:r></w:p></w:tc>
      </w:tr>
      <w:tr>
        <w:tc><w:p><w:r><w:t>power</w:t></w:r></w:p></w:tc>
        <w:tc><w:p><w:r><w:t>hot-swap</w:t></w:r></w:p></w:tc>
      </w:tr>
    </w:tbl>
  </w:body>
</w:document>"#,
            ),
        ])
    }

    fn sample_docx_with_image() -> Vec<u8> {
        let png = one_pixel_png();
        store_zip(&[
            (
                "[Content_Types].xml",
                br#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Default Extension="png" ContentType="image/png"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
  <Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>
</Types>"#,
            ),
            (
                "_rels/.rels",
                br#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#,
            ),
            (
                "word/styles.xml",
                br#"<?xml version="1.0" encoding="UTF-8"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:style w:type="paragraph" w:styleId="Heading1">
    <w:name w:val="heading 1"/>
    <w:pPr><w:outlineLvl w:val="0"/></w:pPr>
  </w:style>
</w:styles>"#,
            ),
            (
                "word/_rels/document.xml.rels",
                br#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
  <Relationship Id="rId10" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.png"/>
</Relationships>"#,
            ),
            (
                "word/document.xml",
                br#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
            xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
            xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"
            xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
            xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture">
  <w:body>
    <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Quarterly report</w:t></w:r></w:p>
    <w:p><w:r><w:t>Widgets shipped on time.</w:t></w:r></w:p>
    <w:p><w:r><w:drawing><wp:inline><wp:docPr id="1" name="Chart" descr="Shipping chart"/>
      <a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture">
        <pic:pic><pic:nvPicPr><pic:cNvPr id="1" name="Chart"/><pic:cNvPicPr/></pic:nvPicPr>
          <pic:blipFill><a:blip r:embed="rId10"/></pic:blipFill>
          <pic:spPr/></pic:pic>
      </a:graphicData></a:graphic>
    </wp:inline></w:drawing></w:r></w:p>
    <w:p><w:r><w:t>Closing remarks.</w:t></w:r></w:p>
  </w:body>
</w:document>"#,
            ),
            ("word/media/image1.png", png.as_slice()),
        ])
    }

    fn one_pixel_png() -> Vec<u8> {
        vec![
            0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, b'I', b'H',
            b'D', b'R', 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0a, b'I', b'D', b'A', b'T', 0x78,
            0x9c, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00,
            0x00, 0x00, 0x00, b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82,
        ]
    }

    fn store_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut centrals = Vec::new();
        for (name, data) in files {
            let name_b = name.as_bytes();
            let crc = crc32(data);
            let offset = out.len() as u32;
            out.extend_from_slice(b"PK\x03\x04");
            out.extend_from_slice(&20u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name_b.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name_b);
            out.extend_from_slice(data);
            let mut c = Vec::new();
            c.extend_from_slice(b"PK\x01\x02");
            c.extend_from_slice(&20u16.to_le_bytes());
            c.extend_from_slice(&20u16.to_le_bytes());
            c.extend_from_slice(&0u16.to_le_bytes());
            c.extend_from_slice(&0u16.to_le_bytes());
            c.extend_from_slice(&0u16.to_le_bytes());
            c.extend_from_slice(&0u16.to_le_bytes());
            c.extend_from_slice(&crc.to_le_bytes());
            c.extend_from_slice(&(data.len() as u32).to_le_bytes());
            c.extend_from_slice(&(data.len() as u32).to_le_bytes());
            c.extend_from_slice(&(name_b.len() as u16).to_le_bytes());
            c.extend_from_slice(&0u16.to_le_bytes());
            c.extend_from_slice(&0u16.to_le_bytes());
            c.extend_from_slice(&0u16.to_le_bytes());
            c.extend_from_slice(&0u16.to_le_bytes());
            c.extend_from_slice(&0u32.to_le_bytes());
            c.extend_from_slice(&offset.to_le_bytes());
            c.extend_from_slice(name_b);
            centrals.push(c);
        }
        let cd_start = out.len() as u32;
        for c in &centrals {
            out.extend_from_slice(c);
        }
        let cd_len = (out.len() as u32) - cd_start;
        let n = centrals.len() as u16;
        out.extend_from_slice(b"PK\x05\x06");
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&cd_len.to_le_bytes());
        out.extend_from_slice(&cd_start.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &b in data {
            crc ^= u32::from(b);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }
}
