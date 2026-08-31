//! Deterministic V2 layout rendering. Both DOCX and PDF consume the same frozen
//! layout model; this module never reads mutable database state.

use std::{
    cell::Cell,
    io::{Cursor, Read, Write},
    sync::Arc,
};

use crate::content_block::{
    Inline, ListItem, Paragraph as RichParagraph, RichNode, TextMark, validate_http_link,
};
use base64::Engine as _;
use docx_rs::{
    AlignmentType, BreakType, Docx, Footer, Header, Hyperlink, HyperlinkType, LineSpacing,
    LineSpacingType, PageMargin, PageNum, Paragraph, Pic, Run, RunFonts, Table, TableCell,
    TableLayoutType, TableRow, VMergeType, WidthType,
};
use image::GenericImageView;
use printpdf::{
    Actions, FontId, Line, LinePoint, LinkAnnotation, Mm, Op, ParsedFont, PdfDocument,
    PdfFontHandle, PdfPage, PdfSaveOptions, Point, Pt, RawImage, Rect, TextItem, TextMatrix,
    TextRenderingMode, XObject, XObjectId, XObjectTransform,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub const DOCX_RENDERER_CONTRACT: &str = "knowledgebrain.bid.v2.docx.1";
pub const PDF_RENDERER_CONTRACT: &str = "knowledgebrain.bid.v2.pdf.1";
pub const PDF_FONT_RESOURCE_ID: &str = "kb-bid-v2-font-1";
pub const PDF_FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/NotoSansJP-Regular.otf");
pub const PDF_FONT_SHA256: &str =
    "5d0df56f107605387e0de494b22dfc7fb05d8d79ffd981474e7be11dbe571882";
const PDF_PAGE_WIDTH: f32 = 210.0;
const PDF_PAGE_HEIGHT: f32 = 297.0;
pub const MAX_FROZEN_IMAGE_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_FROZEN_IMAGE_DIMENSION: u32 = 20_000;
pub const MAX_FROZEN_IMAGE_PIXELS: u64 = 100_000_000;
const MAX_RENDER_PLAN_BYTES: usize = 256 * 1024 * 1024;
const MAX_RENDER_PLAN_PIXELS: u64 = 300_000_000;
const MAX_RENDER_TABLE_CELLS: u64 = 100_000;
const MAX_RENDER_BLOCK_OCCURRENCES: usize = 100_000;

pub fn frozen_image_dimensions(bytes: &[u8]) -> Result<(u32, u32), String> {
    if bytes.is_empty() || bytes.len() > MAX_FROZEN_IMAGE_BYTES {
        return Err("attachment image exceeds the frozen byte budget".into());
    }
    let dimensions = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| format!("unsupported attachment image: {error}"))?
        .into_dimensions()
        .map_err(|error| format!("unsupported attachment image: {error}"))?;
    if dimensions.0 == 0
        || dimensions.1 == 0
        || dimensions.0 > MAX_FROZEN_IMAGE_DIMENSION
        || dimensions.1 > MAX_FROZEN_IMAGE_DIMENSION
        || u64::from(dimensions.0) * u64::from(dimensions.1) > MAX_FROZEN_IMAGE_PIXELS
    {
        return Err("attachment image exceeds the frozen dimension budget".into());
    }
    Ok(dimensions)
}

pub fn frozen_pdf_page_count(bytes: &[u8]) -> Result<u32, String> {
    if bytes.is_empty() || bytes.len() > 128 * 1024 * 1024 {
        return Err("PDF attachment exceeds the frozen byte budget".into());
    }
    let document = lopdf::Document::load_mem(bytes)
        .map_err(|error| format!("inspect frozen PDF page metadata: {error}"))?;
    let pages = u32::try_from(document.get_pages().len())
        .map_err(|_| "frozen PDF page count exceeds platform limits")?;
    if pages == 0 || pages > 1_000 {
        return Err("frozen PDF page count exceeds the export budget".into());
    }
    Ok(pages)
}

/// Returns the exact job-local 144-DPI raster geometry after applying inherited
/// CropBox/MediaBox and page rotation. Callers must run this before `pdftoppm`.
pub fn frozen_pdf_raster_geometry(bytes: &[u8]) -> Result<Vec<(u32, u32)>, String> {
    use lopdf::Object;

    fn number(value: &Object) -> Option<f64> {
        match value {
            Object::Integer(value) => Some(*value as f64),
            Object::Real(value) => Some(f64::from(*value)),
            _ => None,
        }
    }
    fn inherited_array(
        document: &lopdf::Document,
        page_id: lopdf::ObjectId,
        key: &[u8],
    ) -> Option<Vec<f64>> {
        let mut id = page_id;
        for _ in 0..32 {
            let dictionary = document.get_dictionary(id).ok()?;
            if let Ok(value) = dictionary.get(key) {
                let values = match value {
                    Object::Array(values) => Some(values),
                    Object::Reference(reference) => document
                        .get_object(*reference)
                        .ok()
                        .and_then(|value| value.as_array().ok()),
                    _ => None,
                }?;
                let values = values.iter().map(number).collect::<Option<Vec<_>>>()?;
                if values.len() >= 4 {
                    return Some(values);
                }
            }
            id = match dictionary.get(b"Parent") {
                Ok(Object::Reference(parent)) => *parent,
                _ => return None,
            };
        }
        None
    }
    fn inherited_rotation(document: &lopdf::Document, page_id: lopdf::ObjectId) -> i64 {
        let mut id = page_id;
        for _ in 0..32 {
            let Ok(dictionary) = document.get_dictionary(id) else {
                return 0;
            };
            if let Ok(Object::Integer(rotation)) = dictionary.get(b"Rotate") {
                return rotation.rem_euclid(360);
            }
            id = match dictionary.get(b"Parent") {
                Ok(Object::Reference(parent)) => *parent,
                _ => return 0,
            };
        }
        0
    }

    if bytes.is_empty() || bytes.len() > 128 * 1024 * 1024 {
        return Err("PDF attachment exceeds the frozen byte budget".into());
    }
    let document = lopdf::Document::load_mem(bytes)
        .map_err(|error| format!("inspect frozen PDF page geometry: {error}"))?;
    let pages = document.get_pages();
    if pages.is_empty() || pages.len() > 1_000 {
        return Err("frozen PDF page count exceeds the export budget".into());
    }
    let mut result = Vec::with_capacity(pages.len());
    for page_id in pages.values().copied() {
        let values = inherited_array(&document, page_id, b"CropBox")
            .or_else(|| inherited_array(&document, page_id, b"MediaBox"))
            .ok_or_else(|| "frozen PDF page box missing".to_owned())?;
        let width_points = (values[2] - values[0]).abs();
        let height_points = (values[3] - values[1]).abs();
        if !width_points.is_finite()
            || !height_points.is_finite()
            || width_points <= 0.0
            || height_points <= 0.0
        {
            return Err("frozen PDF page box invalid".into());
        }
        let (width_points, height_points) = match inherited_rotation(&document, page_id) {
            90 | 270 => (height_points, width_points),
            0 | 180 => (width_points, height_points),
            _ => return Err("frozen PDF page rotation invalid".into()),
        };
        let width = (width_points * 2.0).ceil();
        let height = (height_points * 2.0).ceil();
        if width > f64::from(u32::MAX) || height > f64::from(u32::MAX) {
            return Err("frozen PDF raster dimension overflow".into());
        }
        let dimensions = (width as u32, height as u32);
        if dimensions.0 == 0
            || dimensions.1 == 0
            || dimensions.0 > MAX_FROZEN_IMAGE_DIMENSION
            || dimensions.1 > MAX_FROZEN_IMAGE_DIMENSION
            || u64::from(dimensions.0) * u64::from(dimensions.1) > MAX_FROZEN_IMAGE_PIXELS
        {
            return Err("frozen PDF page exceeds the raster geometry budget".into());
        }
        result.push(dimensions);
    }
    Ok(result)
}

thread_local! { static DOCX_PARAGRAPH_ID:Cell<u32>=const { Cell::new(1) }; }
fn reset_docx_paragraph_ids() {
    DOCX_PARAGRAPH_ID.set(1);
}
fn docx_paragraph() -> Paragraph {
    let id = DOCX_PARAGRAPH_ID.get();
    DOCX_PARAGRAPH_ID.set(id + 1);
    Paragraph::new().id(format!("{id:08x}"))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LayoutMarginsV2 {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LayoutSettingsV2 {
    pub margins_mm: LayoutMarginsV2,
    pub body_font_pt: f32,
    pub line_spacing: f32,
    pub heading_numbering: String,
    pub header: String,
    pub footer: String,
    pub page_number: String,
    pub include_toc: bool,
}

impl Default for LayoutSettingsV2 {
    fn default() -> Self {
        Self {
            margins_mm: LayoutMarginsV2 {
                top: 25.4,
                right: 25.4,
                bottom: 25.4,
                left: 25.4,
            },
            body_font_pt: 12.0,
            line_spacing: 1.5,
            heading_numbering: "decimal".into(),
            header: String::new(),
            footer: String::new(),
            page_number: "footer_center".into(),
            include_toc: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LayoutDocumentV2 {
    pub title: String,
    pub sections: Vec<LayoutSectionV2>,
    pub watermark: Option<String>,
    pub settings: LayoutSettingsV2,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LayoutSectionV2 {
    pub title: String,
    pub depth: u32,
    pub blocks: Vec<LayoutBlockV2>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LayoutTextRunV2 {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub code: bool,
    pub link: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LayoutParagraphV2 {
    pub list_marker: Option<String>,
    pub runs: Vec<LayoutTextRunV2>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FrozenLayoutAssetV2 {
    pub asset_revision_id: String,
    pub sha256: String,
    pub media_type: String,
    pub file_name: String,
    pub bytes: Arc<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum LayoutBlockV2 {
    RichText(Vec<LayoutParagraphV2>),
    Table(LayoutTableV2),
    Image {
        caption: String,
        width_mm: f32,
        alignment: String,
        crop: LayoutCropV2,
        asset: FrozenLayoutAssetV2,
    },
    Attachment {
        label: String,
    },
    PreparedAttachment {
        label: String,
        pages: Vec<FrozenLayoutAssetV2>,
        start_new_page: bool,
    },
    StructuredForm(Vec<(String, String)>),
    PageBreak,
    Signature {
        label: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LayoutCropV2 {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Default for LayoutCropV2 {
    fn default() -> Self {
        Self {
            left: 0.0,
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LayoutTableCellV2 {
    pub row: usize,
    pub column: usize,
    pub rowspan: usize,
    pub colspan: usize,
    pub text: String,
    pub paragraphs: Vec<LayoutParagraphV2>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LayoutTableV2 {
    pub row_count: usize,
    pub column_count: usize,
    pub cells: Vec<LayoutTableCellV2>,
    pub widths_mm: Vec<f32>,
    pub repeat_header_rows: usize,
}

fn layout_runs(inlines: &[Inline]) -> Vec<LayoutTextRunV2> {
    inlines
        .iter()
        .map(|inline| match inline {
            Inline::HardBreak => LayoutTextRunV2 {
                text: "\n".into(),
                bold: false,
                italic: false,
                underline: false,
                strike: false,
                code: false,
                link: None,
            },
            Inline::Text { text, marks } => LayoutTextRunV2 {
                text: text.clone(),
                bold: marks.iter().any(|mark| matches!(mark, TextMark::Bold)),
                italic: marks.iter().any(|mark| matches!(mark, TextMark::Italic)),
                underline: marks.iter().any(|mark| matches!(mark, TextMark::Underline)),
                strike: marks.iter().any(|mark| matches!(mark, TextMark::Strike)),
                code: marks.iter().any(|mark| matches!(mark, TextMark::Code)),
                link: marks.iter().find_map(|mark| match mark {
                    TextMark::Link { href } => Some(href.clone()),
                    _ => None,
                }),
            },
        })
        .collect()
}

fn layout_paragraphs(nodes: &[RichNode]) -> Vec<LayoutParagraphV2> {
    let mut result = Vec::new();
    for node in nodes {
        match node {
            RichNode::Paragraph { content } => result.push(LayoutParagraphV2 {
                list_marker: None,
                runs: layout_runs(content),
            }),
            RichNode::Blockquote { content } => {
                for paragraph in content {
                    let RichParagraph::Paragraph { content } = paragraph;
                    result.push(LayoutParagraphV2 {
                        list_marker: Some("│ ".into()),
                        runs: layout_runs(content),
                    });
                }
            }
            RichNode::CodeBlock { text, .. } => result.push(LayoutParagraphV2 {
                list_marker: None,
                runs: vec![LayoutTextRunV2 {
                    text: text.clone(),
                    bold: false,
                    italic: false,
                    underline: false,
                    strike: false,
                    code: true,
                    link: None,
                }],
            }),
            RichNode::HorizontalRule => result.push(LayoutParagraphV2 {
                list_marker: None,
                runs: vec![LayoutTextRunV2 {
                    text: "────────".into(),
                    bold: false,
                    italic: false,
                    underline: false,
                    strike: false,
                    code: false,
                    link: None,
                }],
            }),
            RichNode::BulletList { content } | RichNode::OrderedList { content } => {
                let ordered = matches!(node, RichNode::OrderedList { .. });
                for (item_index, item) in content.iter().enumerate() {
                    let ListItem::ListItem { content } = item;
                    for (paragraph_index, paragraph) in content.iter().enumerate() {
                        let RichParagraph::Paragraph { content } = paragraph;
                        result.push(LayoutParagraphV2 {
                            list_marker: (paragraph_index == 0).then(|| {
                                if ordered {
                                    format!("{}. ", item_index + 1)
                                } else {
                                    "• ".into()
                                }
                            }),
                            runs: layout_runs(content),
                        });
                    }
                }
            }
        }
    }
    result
}

fn plain_paragraphs(paragraphs: &[LayoutParagraphV2]) -> String {
    paragraphs
        .iter()
        .map(|paragraph| {
            format!(
                "{}{}",
                paragraph.list_marker.as_deref().unwrap_or_default(),
                paragraph
                    .runs
                    .iter()
                    .map(|run| run.text.as_str())
                    .collect::<String>()
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn find_asset(
    assets: &[FrozenLayoutAssetV2],
    asset_id: &str,
) -> Result<FrozenLayoutAssetV2, String> {
    assets
        .iter()
        .find(|asset| asset.asset_revision_id == asset_id)
        .cloned()
        .ok_or_else(|| format!("frozen render asset {asset_id} missing"))
}

fn form_field_label(forms: &[Value], form_id: &str, field_id: &str) -> String {
    forms
        .iter()
        .find(|form| {
            form.get("form_definition_revision_id")
                .and_then(Value::as_str)
                == Some(form_id)
        })
        .and_then(|form| form.get("fields"))
        .and_then(Value::as_array)
        .and_then(|fields| {
            fields
                .iter()
                .find(|field| field.get("field_id").and_then(Value::as_str) == Some(field_id))
        })
        .and_then(|field| field.get("label"))
        .and_then(Value::as_str)
        .unwrap_or(field_id)
        .to_owned()
}

fn block_from_json(
    block: &Value,
    assets: &[FrozenLayoutAssetV2],
    forms: &[Value],
    preparations: &[Value],
    allow_unprepared_attachment: bool,
) -> Result<LayoutBlockV2, String> {
    let kind = block
        .get("kind")
        .and_then(Value::as_str)
        .ok_or("block kind missing")?;
    let content = block.get("content").ok_or("block content missing")?;
    match kind {
        "rich_text" => {
            let nodes = serde_json::from_value::<Vec<RichNode>>(
                content
                    .get("nodes")
                    .ok_or("rich text nodes missing")?
                    .clone(),
            )
            .map_err(|error| format!("rich text layout invalid: {error}"))?;
            Ok(LayoutBlockV2::RichText(layout_paragraphs(&nodes)))
        }
        "table" => {
            let row_count: usize = content
                .get("row_count")
                .and_then(Value::as_u64)
                .ok_or("table row_count missing")?
                .try_into()
                .map_err(|_| "table row_count exceeds platform limits")?;
            let column_count: usize = content
                .get("column_count")
                .and_then(Value::as_u64)
                .ok_or("table column_count missing")?
                .try_into()
                .map_err(|_| "table column_count exceeds platform limits")?;
            let grid_cells = row_count
                .checked_mul(column_count)
                .ok_or("table grid size overflow")?;
            if grid_cells == 0 || grid_cells as u64 > MAX_RENDER_TABLE_CELLS {
                return Err("table grid exceeds cell budget".into());
            }
            let widths_mm = content
                .get("widths_mm")
                .and_then(Value::as_array)
                .ok_or("table widths_mm missing")?
                .iter()
                .map(|value| {
                    value
                        .as_f64()
                        .map(|width| width as f32)
                        .ok_or("table width invalid")
                })
                .collect::<Result<Vec<_>, _>>()?;
            if widths_mm.len() != column_count {
                return Err("table width count mismatch".into());
            }
            let repeat_header_rows: usize = content
                .get("repeat_header_rows")
                .and_then(Value::as_u64)
                .ok_or("table repeat_header_rows missing")?
                .try_into()
                .map_err(|_| "table header count exceeds platform limits")?;
            let cell_values = content
                .get("cells")
                .and_then(Value::as_array)
                .ok_or("table cells missing")?;
            if cell_values.len() > grid_cells {
                return Err("table cell count exceeds declared grid".into());
            }
            let cells = cell_values
                .iter()
                .map(|cell| {
                    let nodes = serde_json::from_value::<Vec<RichNode>>(
                        cell.get("content")
                            .ok_or("table cell content missing")?
                            .clone(),
                    )
                    .map_err(|error| format!("table cell rich content invalid: {error}"))?;
                    let paragraphs = layout_paragraphs(&nodes);
                    let text = plain_paragraphs(&paragraphs);
                    Ok(LayoutTableCellV2 {
                        row: cell
                            .get("row")
                            .and_then(Value::as_u64)
                            .ok_or("table cell row missing")?
                            .try_into()
                            .map_err(|_| "table cell row exceeds platform limits")?,
                        column: cell
                            .get("column")
                            .and_then(Value::as_u64)
                            .ok_or("table cell column missing")?
                            .try_into()
                            .map_err(|_| "table cell column exceeds platform limits")?,
                        rowspan: cell
                            .get("rowspan")
                            .and_then(Value::as_u64)
                            .ok_or("table cell rowspan missing")?
                            .try_into()
                            .map_err(|_| "table cell rowspan exceeds platform limits")?,
                        colspan: cell
                            .get("colspan")
                            .and_then(Value::as_u64)
                            .ok_or("table cell colspan missing")?
                            .try_into()
                            .map_err(|_| "table cell colspan exceeds platform limits")?,
                        text,
                        paragraphs,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            if cells.is_empty()
                || row_count == 0
                || column_count == 0
                || repeat_header_rows > row_count
            {
                return Err("table dimensions or header count invalid".into());
            }
            let slots = row_count
                .checked_mul(column_count)
                .ok_or("table dimensions overflow")?;
            let mut covered = vec![false; slots];
            for cell in &cells {
                let row_end = cell.row.checked_add(cell.rowspan);
                let column_end = cell.column.checked_add(cell.colspan);
                if cell.rowspan == 0
                    || cell.colspan == 0
                    || row_end.is_none_or(|end| end > row_count)
                    || column_end.is_none_or(|end| end > column_count)
                    || (cell.row < repeat_header_rows
                        && row_end.is_none_or(|end| end > repeat_header_rows))
                {
                    return Err("table merge crosses bounds or the header boundary".into());
                }
                let row_end = row_end.expect("checked above");
                let column_end = column_end.expect("checked above");
                for row in cell.row..row_end {
                    for column in cell.column..column_end {
                        let slot = row * column_count + column;
                        if covered[slot] {
                            return Err("table cells overlap".into());
                        }
                        covered[slot] = true;
                    }
                }
            }
            if covered.iter().any(|covered| !covered) {
                return Err("table cells do not cover the declared grid".into());
            }
            Ok(LayoutBlockV2::Table(LayoutTableV2 {
                row_count,
                column_count,
                cells,
                widths_mm,
                repeat_header_rows,
            }))
        }
        "image" => {
            let asset_id = content
                .get("asset_revision_id")
                .and_then(Value::as_str)
                .ok_or("image asset identity missing")?;
            let crop = content.get("crop").ok_or("image crop missing")?;
            Ok(LayoutBlockV2::Image {
                caption: content
                    .get("caption")
                    .or_else(|| content.get("alt"))
                    .and_then(Value::as_str)
                    .unwrap_or("图片")
                    .to_owned(),
                width_mm: content
                    .get("width_mm")
                    .and_then(Value::as_f64)
                    .ok_or("image width missing")? as f32,
                alignment: content
                    .get("alignment")
                    .and_then(Value::as_str)
                    .ok_or("image alignment missing")?
                    .to_owned(),
                crop: LayoutCropV2 {
                    left: crop
                        .get("left")
                        .and_then(Value::as_f64)
                        .ok_or("image crop left missing")? as f32,
                    top: crop
                        .get("top")
                        .and_then(Value::as_f64)
                        .ok_or("image crop top missing")? as f32,
                    right: crop
                        .get("right")
                        .and_then(Value::as_f64)
                        .ok_or("image crop right missing")? as f32,
                    bottom: crop
                        .get("bottom")
                        .and_then(Value::as_f64)
                        .ok_or("image crop bottom missing")? as f32,
                },
                asset: find_asset(assets, asset_id)?,
            })
        }
        "attachment_ref" => {
            let source_id = content
                .get("asset_revision_id")
                .and_then(Value::as_str)
                .ok_or("attachment asset identity missing")?;
            let source = find_asset(assets, source_id)?;
            if content.get("render_mode").and_then(Value::as_str) == Some("file_reference") {
                Ok(LayoutBlockV2::Attachment {
                    label: source.file_name,
                })
            } else {
                let preparation_id = content
                    .get("preparation_revision_id")
                    .and_then(Value::as_str);
                let matching = preparations
                    .iter()
                    .filter(|value| {
                        preparation_id.map_or_else(
                            || {
                                value
                                    .get("source_asset_revision_id")
                                    .and_then(Value::as_str)
                                    == Some(source_id)
                            },
                            |expected| {
                                value
                                    .get("attachment_preparation_revision_id")
                                    .and_then(Value::as_str)
                                    == Some(expected)
                            },
                        )
                    })
                    .collect::<Vec<_>>();
                if matching.is_empty() && preparation_id.is_none() && allow_unprepared_attachment {
                    return Ok(LayoutBlockV2::Attachment {
                        label: format!("{}（导出时嵌入页面）", source.file_name),
                    });
                }
                if matching.len() != 1 {
                    return Err("embedded attachment has no unique frozen preparation".into());
                }
                let preparation = matching[0];
                let pages = preparation
                    .get("page_assets")
                    .and_then(Value::as_array)
                    .ok_or("attachment preparation pages missing")?
                    .iter()
                    .map(|page| {
                        let id = page
                            .get("page_asset_id")
                            .and_then(Value::as_str)
                            .ok_or("prepared page identity missing")?;
                        find_asset(assets, id)
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                if pages.is_empty() {
                    return Err("attachment preparation has no pages".into());
                }
                Ok(LayoutBlockV2::PreparedAttachment {
                    label: source.file_name,
                    pages,
                    start_new_page: content
                        .get("start_new_page")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                })
            }
        }
        "structured_form" => {
            let form_id = content
                .get("form_definition_revision_id")
                .and_then(Value::as_str)
                .ok_or("structured form definition identity missing")?;
            Ok(LayoutBlockV2::StructuredForm(
                content
                    .get("field_values")
                    .and_then(Value::as_array)
                    .unwrap_or(&Vec::new())
                    .iter()
                    .map(|field| {
                        let field_id = field
                            .get("field_id")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        (
                            form_field_label(forms, form_id, field_id),
                            field
                                .get("value")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_owned(),
                        )
                    })
                    .collect(),
            ))
        }
        "page_break" => Ok(LayoutBlockV2::PageBreak),
        "signature_placeholder" => Ok(LayoutBlockV2::Signature {
            label: content
                .get("label")
                .and_then(Value::as_str)
                .unwrap_or("签章处")
                .to_owned(),
        }),
        other => Err(format!("unsupported ContentBlockV1 kind {other}")),
    }
}

fn settings_from_workspace(workspace: &Value) -> LayoutSettingsV2 {
    let mut result = LayoutSettingsV2::default();
    let Some(settings) = workspace.get("document_settings") else {
        return result;
    };
    if let Some(margins) = settings.get("margins_mm") {
        result.margins_mm = LayoutMarginsV2 {
            top: margins.get("top").and_then(Value::as_f64).unwrap_or(25.4) as f32,
            right: margins.get("right").and_then(Value::as_f64).unwrap_or(25.4) as f32,
            bottom: margins
                .get("bottom")
                .and_then(Value::as_f64)
                .unwrap_or(25.4) as f32,
            left: margins.get("left").and_then(Value::as_f64).unwrap_or(25.4) as f32,
        };
    }
    result.body_font_pt = settings
        .get("body_font_pt")
        .and_then(Value::as_f64)
        .unwrap_or(12.0) as f32;
    result.line_spacing = settings
        .get("line_spacing")
        .and_then(Value::as_f64)
        .unwrap_or(1.5) as f32;
    result.heading_numbering = settings
        .get("heading_numbering")
        .and_then(Value::as_str)
        .unwrap_or("decimal")
        .to_owned();
    result.header = settings
        .get("header")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    result.footer = settings
        .get("footer")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    result.page_number = settings
        .get("page_number")
        .and_then(Value::as_str)
        .unwrap_or("footer_center")
        .to_owned();
    result.include_toc = settings
        .get("include_toc")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    result
}

fn printable_dimensions(settings: &LayoutSettingsV2) -> Result<(f32, f32), String> {
    let width = PDF_PAGE_WIDTH - settings.margins_mm.left - settings.margins_mm.right;
    let height = PDF_PAGE_HEIGHT - settings.margins_mm.top - settings.margins_mm.bottom;
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return Err("document settings leave no printable area".into());
    }
    Ok((width, height))
}

fn fitted_page_width(
    asset: &FrozenLayoutAssetV2,
    settings: &LayoutSettingsV2,
) -> Result<f32, String> {
    let image = cropped_image(asset, &LayoutCropV2::default())?;
    let (width_px, height_px) = image.dimensions();
    let (printable_width, printable_height) = printable_dimensions(settings)?;
    Ok(printable_width.min(printable_height * width_px as f32 / height_px as f32))
}

fn validate_paragraph_links(paragraphs: &[LayoutParagraphV2]) -> Result<(), String> {
    for link in paragraphs
        .iter()
        .flat_map(|paragraph| &paragraph.runs)
        .filter_map(|run| run.link.as_deref())
    {
        validate_http_link(link)
            .map_err(|_| "rich text link must use a safe HTTP(S) URI".to_owned())?;
    }
    Ok(())
}

fn validate_layout_dimensions(document: &LayoutDocumentV2) -> Result<(), String> {
    let (printable_width, printable_height) = printable_dimensions(&document.settings)?;
    for section in &document.sections {
        for block in &section.blocks {
            match block {
                LayoutBlockV2::RichText(paragraphs) => validate_paragraph_links(paragraphs)?,
                LayoutBlockV2::Table(table) => {
                    if table.repeat_header_rows > table.row_count {
                        return Err("table header count exceeds row count".into());
                    }
                    for cell in &table.cells {
                        validate_paragraph_links(&cell.paragraphs)?;
                        let row_end = cell
                            .row
                            .checked_add(cell.rowspan)
                            .ok_or("table row span overflow")?;
                        let column_end = cell
                            .column
                            .checked_add(cell.colspan)
                            .ok_or("table column span overflow")?;
                        if row_end > table.row_count || column_end > table.column_count {
                            return Err("table merge crosses bounds".into());
                        }
                        if cell.row < table.repeat_header_rows && row_end > table.repeat_header_rows
                        {
                            return Err("table merge crosses the header boundary".into());
                        }
                    }
                    let declared = table.widths_mm.iter().sum::<f32>();
                    if !declared.is_finite() || declared > printable_width + f32::EPSILON {
                        return Err("table exceeds printable width".into());
                    }
                }
                LayoutBlockV2::Image {
                    width_mm,
                    asset,
                    crop,
                    ..
                } => {
                    if !width_mm.is_finite()
                        || *width_mm <= 0.0
                        || *width_mm > printable_width + f32::EPSILON
                    {
                        return Err("image exceeds printable width".into());
                    }
                    let image = cropped_image(asset, crop)?;
                    let (width_px, height_px) = image.dimensions();
                    let height_mm = *width_mm * height_px as f32 / width_px as f32;
                    if height_mm > printable_height + f32::EPSILON {
                        return Err("image exceeds printable height".into());
                    }
                }
                LayoutBlockV2::PreparedAttachment { pages, .. } => {
                    for page in pages {
                        let _ = fitted_page_width(page, &document.settings)?;
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

pub fn layout_from_workspace(
    title: &str,
    workspace: &Value,
    watermark: Option<String>,
) -> Result<LayoutDocumentV2, String> {
    layout_from_workspace_with_resources(title, workspace, &[], &[], &[], watermark)
}

pub fn layout_from_workspace_with_resources(
    title: &str,
    workspace: &Value,
    assets: &[FrozenLayoutAssetV2],
    forms: &[Value],
    preparations: &[Value],
    watermark: Option<String>,
) -> Result<LayoutDocumentV2, String> {
    layout_from_workspace_with_resources_policy(
        title,
        workspace,
        assets,
        forms,
        preparations,
        watermark,
        false,
    )
}

pub fn layout_preview_from_workspace_with_resources(
    title: &str,
    workspace: &Value,
    assets: &[FrozenLayoutAssetV2],
    forms: &[Value],
    preparations: &[Value],
    watermark: Option<String>,
) -> Result<LayoutDocumentV2, String> {
    layout_from_workspace_with_resources_policy(
        title,
        workspace,
        assets,
        forms,
        preparations,
        watermark,
        true,
    )
}

fn checked_render_plan_budget(
    nodes: &[Value],
    blocks: &[Value],
    assets: &[FrozenLayoutAssetV2],
    preparations: &[Value],
) -> Result<(), String> {
    let mut bytes = 0usize;
    let mut pixels = 0u64;
    let mut cells = 0u64;
    let mut occurrences = 0usize;
    let mut charge_asset = |asset_id: &str| -> Result<(), String> {
        let asset = assets
            .iter()
            .find(|asset| asset.asset_revision_id == asset_id)
            .ok_or_else(|| format!("frozen render asset {asset_id} missing"))?;
        bytes = bytes
            .checked_add(asset.bytes.len())
            .ok_or("render plan byte budget overflow")?;
        if bytes > MAX_RENDER_PLAN_BYTES {
            return Err("render plan exceeds aggregate byte budget".into());
        }
        if asset.media_type.starts_with("image/") {
            let (width, height) = frozen_image_dimensions(&asset.bytes)?;
            pixels = pixels
                .checked_add(u64::from(width) * u64::from(height))
                .ok_or("render plan pixel budget overflow")?;
            if pixels > MAX_RENDER_PLAN_PIXELS {
                return Err("render plan exceeds aggregate pixel budget".into());
            }
        }
        Ok(())
    };
    for node in nodes {
        let lineage_ids = node
            .get("block_lineage_ids")
            .and_then(Value::as_array)
            .ok_or("node block identities missing")?;
        occurrences = occurrences
            .checked_add(lineage_ids.len())
            .ok_or("render block occurrence budget overflow")?;
        if occurrences > MAX_RENDER_BLOCK_OCCURRENCES {
            return Err("render plan has too many block occurrences".into());
        }
        for lineage_id in lineage_ids {
            let lineage_id = lineage_id
                .as_str()
                .ok_or("invalid block lineage identity")?;
            let block = blocks
                .iter()
                .find(|block| block.get("lineage_id").and_then(Value::as_str) == Some(lineage_id))
                .ok_or_else(|| format!("frozen block {lineage_id} missing"))?;
            let content = block.get("content").ok_or("block content missing")?;
            match block.get("kind").and_then(Value::as_str) {
                Some("image") => charge_asset(
                    content
                        .get("asset_revision_id")
                        .and_then(Value::as_str)
                        .ok_or("image asset identity missing")?,
                )?,
                Some("attachment_ref") => {
                    let source_id = content
                        .get("asset_revision_id")
                        .and_then(Value::as_str)
                        .ok_or("attachment asset identity missing")?;
                    charge_asset(source_id)?;
                    if content.get("render_mode").and_then(Value::as_str) == Some("embedded_pages")
                    {
                        let preparation_id = content
                            .get("preparation_revision_id")
                            .and_then(Value::as_str);
                        let matching = preparations
                            .iter()
                            .filter(|value| {
                                preparation_id.map_or_else(
                                    || {
                                        value
                                            .get("source_asset_revision_id")
                                            .and_then(Value::as_str)
                                            == Some(source_id)
                                    },
                                    |expected| {
                                        value
                                            .get("attachment_preparation_revision_id")
                                            .and_then(Value::as_str)
                                            == Some(expected)
                                    },
                                )
                            })
                            .collect::<Vec<_>>();
                        if matching.len() == 1 {
                            for page in matching[0]
                                .get("page_assets")
                                .and_then(Value::as_array)
                                .ok_or("attachment preparation pages missing")?
                            {
                                charge_asset(
                                    page.get("page_asset_id")
                                        .and_then(Value::as_str)
                                        .ok_or("prepared page identity missing")?,
                                )?;
                            }
                        }
                    }
                }
                Some("table") => {
                    let rows = content
                        .get("row_count")
                        .and_then(Value::as_u64)
                        .ok_or("table row_count missing")?;
                    let columns = content
                        .get("column_count")
                        .and_then(Value::as_u64)
                        .ok_or("table column_count missing")?;
                    cells = cells
                        .checked_add(
                            rows.checked_mul(columns)
                                .ok_or("table cell budget overflow")?,
                        )
                        .ok_or("table cell budget overflow")?;
                    if cells > MAX_RENDER_TABLE_CELLS {
                        return Err("render plan exceeds table cell budget".into());
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn layout_from_workspace_with_resources_policy(
    title: &str,
    workspace: &Value,
    assets: &[FrozenLayoutAssetV2],
    forms: &[Value],
    preparations: &[Value],
    watermark: Option<String>,
    allow_unprepared_attachment: bool,
) -> Result<LayoutDocumentV2, String> {
    let nodes = workspace
        .get("nodes")
        .and_then(Value::as_array)
        .ok_or("workspace nodes missing")?;
    let blocks = workspace
        .get("blocks")
        .and_then(Value::as_array)
        .ok_or("workspace blocks missing")?;
    checked_render_plan_budget(nodes, blocks, assets, preparations)?;
    let mut sections = Vec::with_capacity(nodes.len());
    for node in nodes {
        if matches!(
            node.get("render_role").and_then(Value::as_str),
            Some("hidden" | "front_matter" | "toc")
        ) || matches!(
            node.get("semantic_role").and_then(Value::as_str),
            Some("cover" | "toc")
        ) {
            continue;
        }
        let lineage_ids = node
            .get("block_lineage_ids")
            .and_then(Value::as_array)
            .ok_or("node block identities missing")?;
        let mut section_blocks = Vec::new();
        for lineage_id in lineage_ids {
            let lineage_id = lineage_id
                .as_str()
                .ok_or("invalid block lineage identity")?;
            let block = blocks
                .iter()
                .find(|block| block.get("lineage_id").and_then(Value::as_str) == Some(lineage_id))
                .ok_or_else(|| format!("frozen block {lineage_id} missing"))?;
            section_blocks.push(block_from_json(
                block,
                assets,
                forms,
                preparations,
                allow_unprepared_attachment,
            )?);
        }
        sections.push(LayoutSectionV2 {
            title: node
                .get("title")
                .and_then(Value::as_str)
                .ok_or("node title missing")?
                .to_owned(),
            depth: node.get("depth").and_then(Value::as_u64).unwrap_or(0) as u32,
            blocks: section_blocks,
        });
    }
    let document = LayoutDocumentV2 {
        title: title.to_owned(),
        sections,
        watermark,
        settings: settings_from_workspace(workspace),
    };
    validate_layout_dimensions(&document)?;
    Ok(document)
}

fn run_fonts(_settings: &LayoutSettingsV2) -> RunFonts {
    RunFonts::new()
        .ascii("Noto Sans JP")
        .hi_ansi("Noto Sans JP")
        .east_asia("Noto Sans JP")
        .cs("Noto Sans JP")
}

fn paragraph(text: &str, size: usize, settings: &LayoutSettingsV2) -> Paragraph {
    docx_paragraph().add_run(
        Run::new()
            .add_text(text)
            .size(size)
            .fonts(run_fonts(settings)),
    )
}

fn body_paragraph(text: &str, settings: &LayoutSettingsV2) -> Paragraph {
    paragraph(
        text,
        (settings.body_font_pt * 2.0).round() as usize,
        settings,
    )
    .line_spacing(
        LineSpacing::new()
            .line_rule(LineSpacingType::Auto)
            .line((settings.line_spacing * 240.0).round() as i32),
    )
}

fn rich_docx_paragraph(value: &LayoutParagraphV2, settings: &LayoutSettingsV2) -> Paragraph {
    let mut paragraph = docx_paragraph().line_spacing(
        LineSpacing::new()
            .line_rule(LineSpacingType::Auto)
            .line((settings.line_spacing * 240.0).round() as i32),
    );
    if let Some(marker) = &value.list_marker {
        paragraph = paragraph.add_run(
            Run::new()
                .add_text(marker)
                .size((settings.body_font_pt * 2.0).round() as usize)
                .fonts(run_fonts(settings)),
        );
    }
    for value in &value.runs {
        let mut run = Run::new()
            .add_text(&value.text)
            .size((settings.body_font_pt * 2.0).round() as usize)
            .fonts(run_fonts(settings));
        if value.bold {
            run = run.bold();
        }
        if value.italic {
            run = run.italic();
        }
        if value.underline || value.link.is_some() {
            run = run.underline("single");
        }
        if value.strike {
            run = run.strike();
        }
        if let Some(link) = &value.link {
            paragraph =
                paragraph.add_hyperlink(Hyperlink::new(link, HyperlinkType::External).add_run(run));
        } else {
            paragraph = paragraph.add_run(run);
        }
    }
    paragraph
}

fn chinese_ordinal(value: u32) -> String {
    const DIGITS: [&str; 10] = ["零", "一", "二", "三", "四", "五", "六", "七", "八", "九"];
    match value {
        0..=9 => DIGITS[value as usize].into(),
        10 => "十".into(),
        11..=19 => format!("十{}", DIGITS[(value % 10) as usize]),
        20..=99 if value.is_multiple_of(10) => format!("{}十", DIGITS[(value / 10) as usize]),
        20..=99 => format!(
            "{}十{}",
            DIGITS[(value / 10) as usize],
            DIGITS[(value % 10) as usize]
        ),
        _ => value.to_string(),
    }
}

fn numbered_section_titles(document: &LayoutDocumentV2) -> Vec<String> {
    if document.settings.heading_numbering == "none" {
        return document
            .sections
            .iter()
            .map(|section| section.title.clone())
            .collect();
    }
    let base_depth = document
        .sections
        .iter()
        .map(|section| section.depth)
        .min()
        .unwrap_or(0);
    let mut counters = [0u32; 8];
    document
        .sections
        .iter()
        .map(|section| {
            let depth = section
                .depth
                .saturating_sub(base_depth)
                .min((counters.len() - 1) as u32) as usize;
            counters[depth] += 1;
            counters
                .iter_mut()
                .skip(depth + 1)
                .for_each(|value| *value = 0);
            let prefix = if depth == 0 {
                format!("{}、", chinese_ordinal(counters[0]))
            } else if depth == 1 {
                format!("{}.", counters[1])
            } else {
                counters[1..=depth]
                    .iter()
                    .copied()
                    .filter(|value| *value > 0)
                    .map(|value| value.to_string())
                    .collect::<Vec<_>>()
                    .join(".")
            };
            if depth == 0 {
                format!("{prefix}{}", section.title)
            } else {
                format!("{prefix} {}", section.title)
            }
        })
        .collect()
}

fn table_slot_owners(table: &LayoutTableV2) -> Vec<usize> {
    let mut owners = vec![usize::MAX; table.row_count * table.column_count];
    for (index, cell) in table.cells.iter().enumerate() {
        for row in cell.row..cell.row + cell.rowspan {
            for column in cell.column..cell.column + cell.colspan {
                owners[row * table.column_count + column] = index;
            }
        }
    }
    owners
}

fn table_cells_by_row(table: &LayoutTableV2) -> Vec<Vec<usize>> {
    let mut rows = vec![Vec::new(); table.row_count];
    for (index, cell) in table.cells.iter().enumerate() {
        rows[cell.row].push(index);
    }
    for cells in &mut rows {
        cells.sort_by_key(|index| table.cells[*index].column);
    }
    rows
}

fn table_protected_row_ends(table: &LayoutTableV2, row_cells: &[Vec<usize>]) -> Vec<usize> {
    let mut ends = (0..table.row_count)
        .map(|row| {
            row_cells[row]
                .iter()
                .map(|index| table.cells[*index].row + table.cells[*index].rowspan)
                .max()
                .unwrap_or(row + 1)
        })
        .collect::<Vec<_>>();
    for row in (0..table.row_count).rev() {
        let mut cursor = row + 1;
        let mut end = ends[row];
        while cursor < end {
            end = end.max(ends[cursor]);
            cursor += 1;
        }
        ends[row] = end;
    }
    ends
}

fn docx_table(table: &LayoutTableV2, settings: &LayoutSettingsV2) -> Table {
    let owners = table_slot_owners(table);
    let rows = (0..table.row_count)
        .map(|row| {
            let mut cells = Vec::new();
            let mut column = 0;
            while column < table.column_count {
                let owner = owners[row * table.column_count + column];
                let source = &table.cells[owner];
                if source.row == row && source.column == column {
                    let width = table.widths_mm[column..column + source.colspan]
                        .iter()
                        .sum::<f32>();
                    let mut cell = TableCell::new();
                    if source.paragraphs.is_empty() {
                        cell = cell.add_paragraph(body_paragraph(&source.text, settings));
                    } else {
                        for paragraph in &source.paragraphs {
                            cell = cell.add_paragraph(rich_docx_paragraph(paragraph, settings));
                        }
                    }
                    cell = cell.width((width * 56.692_913).round() as usize, WidthType::Dxa);
                    if source.colspan > 1 {
                        cell = cell.grid_span(source.colspan);
                    }
                    if source.rowspan > 1 {
                        cell = cell.vertical_merge(VMergeType::Restart);
                    }
                    cells.push(cell);
                    column += source.colspan;
                } else if source.row < row && source.column == column {
                    let width = table.widths_mm[column..column + source.colspan]
                        .iter()
                        .sum::<f32>();
                    let mut cell = TableCell::new()
                        .add_paragraph(body_paragraph("", settings))
                        .width((width * 56.692_913).round() as usize, WidthType::Dxa)
                        .vertical_merge(VMergeType::Continue);
                    if source.colspan > 1 {
                        cell = cell.grid_span(source.colspan);
                    }
                    cells.push(cell);
                    column += source.colspan;
                } else {
                    column += 1;
                }
            }
            TableRow::new(cells).cant_split()
        })
        .collect();
    let grid = table
        .widths_mm
        .iter()
        .map(|width| (*width * 56.692_913).round() as usize)
        .collect::<Vec<_>>();
    let width = grid.iter().sum();
    Table::new(rows)
        .set_grid(grid)
        .width(width, WidthType::Dxa)
        .layout(TableLayoutType::Fixed)
}

fn docx_form_table(fields: &[(String, String)], settings: &LayoutSettingsV2) -> Table {
    Table::new(
        fields
            .iter()
            .map(|(key, value)| {
                TableRow::new(vec![
                    TableCell::new().add_paragraph(body_paragraph(key, settings)),
                    TableCell::new().add_paragraph(body_paragraph(value, settings)),
                ])
            })
            .collect(),
    )
}

fn cropped_image(
    asset: &FrozenLayoutAssetV2,
    crop: &LayoutCropV2,
) -> Result<image::DynamicImage, String> {
    let digest = hex::encode(Sha256::digest(asset.bytes.as_slice()));
    if asset.sha256.len() != 64 || digest != asset.sha256 {
        return Err(format!(
            "frozen image digest mismatch: {}",
            asset.asset_revision_id
        ));
    }
    let _ = frozen_image_dimensions(&asset.bytes)?;
    let image = image::load_from_memory(&asset.bytes)
        .map_err(|error| format!("decode frozen image {}: {error}", asset.asset_revision_id))?;
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return Err("frozen image has zero dimensions".into());
    }
    let x = (crop.left * width as f32).round() as u32;
    let y = (crop.top * height as f32).round() as u32;
    let right = (crop.right * width as f32).round() as u32;
    let bottom = (crop.bottom * height as f32).round() as u32;
    let crop_width = width.saturating_sub(x).saturating_sub(right);
    let crop_height = height.saturating_sub(y).saturating_sub(bottom);
    if crop_width == 0 || crop_height == 0 {
        return Err("frozen image crop has zero dimensions".into());
    }
    Ok(image.crop_imm(x, y, crop_width, crop_height))
}

fn docx_image(
    asset: &FrozenLayoutAssetV2,
    width_mm: f32,
    crop: &LayoutCropV2,
    alignment: &str,
) -> Result<Paragraph, String> {
    let image = cropped_image(asset, crop)?;
    let (width_px, height_px) = image.dimensions();
    let mut png = Cursor::new(Vec::new());
    image
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|error| error.to_string())?;
    let height_mm = width_mm * (height_px as f32 / width_px as f32);
    let pic = Pic::new_with_dimensions(png.into_inner(), width_px, height_px)
        .id(format!("rIdKb{}", &asset.sha256[..16]))
        .size((width_mm * 36_000.0) as u32, (height_mm * 36_000.0) as u32);
    let alignment = match alignment {
        "left" => AlignmentType::Left,
        "right" => AlignmentType::Right,
        _ => AlignmentType::Center,
    };
    Ok(docx_paragraph()
        .align(alignment)
        .add_run(Run::new().add_image(pic)))
}

fn repeated_docx_table_headers(document: &LayoutDocumentV2) -> Vec<usize> {
    document
        .sections
        .iter()
        .flat_map(|section| &section.blocks)
        .filter_map(|block| match block {
            LayoutBlockV2::Table(table) => Some(table.repeat_header_rows),
            LayoutBlockV2::StructuredForm(_) => Some(0),
            _ => None,
        })
        .collect()
}

fn mark_docx_table_headers(xml: &str, repeats: &[usize]) -> Result<String, String> {
    let mut output = String::with_capacity(xml.len() + repeats.iter().sum::<usize>() * 32);
    let mut cursor = 0;
    for repeat in repeats {
        let relative_start = xml[cursor..]
            .find("<w:tbl>")
            .ok_or("DOCX table sequence is incomplete")?;
        let table_start = cursor + relative_start;
        let relative_end = xml[table_start..]
            .find("</w:tbl>")
            .ok_or("DOCX table XML is unbalanced")?;
        let table_end = table_start + relative_end + "</w:tbl>".len();
        output.push_str(&xml[cursor..table_start]);
        let mut table = xml[table_start..table_end].to_owned();
        let mut row_cursor = 0;
        for _ in 0..*repeat {
            let row_start = row_cursor
                + table[row_cursor..]
                    .find("<w:tr>")
                    .ok_or("DOCX repeat-header row is missing")?;
            let child_start = row_start + "<w:tr>".len();
            if table[child_start..].starts_with("<w:trPr />") {
                table.replace_range(
                    child_start..child_start + "<w:trPr />".len(),
                    "<w:trPr><w:tblHeader /></w:trPr>",
                );
            } else if table[child_start..].starts_with("<w:trPr>") {
                let insertion = child_start + "<w:trPr>".len();
                table.insert_str(insertion, "<w:tblHeader />");
            } else {
                table.insert_str(child_start, "<w:trPr><w:tblHeader /></w:trPr>");
            }
            row_cursor = child_start + "<w:trPr><w:tblHeader /></w:trPr>".len();
        }
        output.push_str(&table);
        cursor = table_end;
    }
    output.push_str(&xml[cursor..]);
    Ok(output)
}

fn patch_docx_table_headers(
    bytes: Vec<u8>,
    document: &LayoutDocumentV2,
) -> Result<Vec<u8>, String> {
    let repeats = repeated_docx_table_headers(document);
    if repeats.iter().all(|repeat| *repeat == 0) {
        return Ok(bytes);
    }
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|error| error.to_string())?;
    let mut entries = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        let name = entry.name().to_owned();
        let is_dir = entry.is_dir();
        let compression = entry.compression();
        let mut value = Vec::new();
        entry
            .read_to_end(&mut value)
            .map_err(|error| error.to_string())?;
        if name == "word/document.xml" {
            let xml = String::from_utf8(value).map_err(|error| error.to_string())?;
            value = mark_docx_table_headers(&xml, &repeats)?.into_bytes();
        }
        entries.push((name, is_dir, compression, value));
    }
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, is_dir, compression, value) in entries {
        let options = zip::write::SimpleFileOptions::default().compression_method(compression);
        if is_dir {
            writer
                .add_directory(name, options)
                .map_err(|error| error.to_string())?;
        } else {
            writer
                .start_file(name, options)
                .map_err(|error| error.to_string())?;
            writer
                .write_all(&value)
                .map_err(|error| error.to_string())?;
        }
    }
    writer
        .finish()
        .map(|cursor| cursor.into_inner())
        .map_err(|error| error.to_string())
}

pub fn render_docx(document: &LayoutDocumentV2) -> Result<Vec<u8>, String> {
    validate_layout_dimensions(document)?;
    reset_docx_paragraph_ids();
    let mm_to_twips = |value: f32| (value * 56.692_913).round() as i32;
    let margins = &document.settings.margins_mm;
    let mut docx = Docx::new()
        .page_size(11_906, 16_838)
        .page_margin(
            PageMargin::new()
                .top(mm_to_twips(margins.top))
                .right(mm_to_twips(margins.right))
                .bottom(mm_to_twips(margins.bottom))
                .left(mm_to_twips(margins.left)),
        )
        .add_paragraph(paragraph(&document.title, 36, &document.settings));
    if !document.settings.header.is_empty() {
        docx = docx.header(Header::new().add_paragraph(paragraph(
            &document.settings.header,
            18,
            &document.settings,
        )));
    }
    if !document.settings.footer.is_empty() || document.settings.page_number != "none" {
        let footer_alignment = if document.settings.page_number == "footer_outside" {
            AlignmentType::Right
        } else {
            AlignmentType::Center
        };
        let mut footer = docx_paragraph().align(footer_alignment);
        if !document.settings.footer.is_empty() {
            footer = footer.add_run(
                Run::new()
                    .add_text(&document.settings.footer)
                    .size(18)
                    .fonts(run_fonts(&document.settings)),
            );
        }
        if document.settings.page_number != "none" {
            footer = footer
                .add_run(
                    Run::new()
                        .add_text("  第 ")
                        .size(18)
                        .fonts(run_fonts(&document.settings)),
                )
                .add_page_num(PageNum::new())
                .add_run(
                    Run::new()
                        .add_text(" 页")
                        .size(18)
                        .fonts(run_fonts(&document.settings)),
                );
        }
        docx = docx.footer(Footer::new().add_paragraph(footer));
    }
    if let Some(watermark) = &document.watermark {
        docx = docx.add_paragraph(paragraph(
            &format!("【{watermark}】"),
            20,
            &document.settings,
        ));
    }
    if document.settings.include_toc {
        docx = docx.add_paragraph(paragraph("目录", 30, &document.settings).style("Heading1"));
        for title in numbered_section_titles(document) {
            docx = docx.add_paragraph(paragraph(&title, 20, &document.settings));
        }
        docx = docx.add_paragraph(docx_paragraph().add_run(Run::new().add_break(BreakType::Page)));
    }
    let titles = numbered_section_titles(document);
    for (section, section_title) in document.sections.iter().zip(titles) {
        let size = 30usize
            .saturating_sub((section.depth as usize).saturating_mul(2))
            .max(22);
        let style = format!("Heading{}", (section.depth + 1).min(6));
        docx =
            docx.add_paragraph(paragraph(&section_title, size, &document.settings).style(&style));
        for block in &section.blocks {
            match block {
                LayoutBlockV2::RichText(paragraphs) => {
                    for paragraph in paragraphs {
                        docx =
                            docx.add_paragraph(rich_docx_paragraph(paragraph, &document.settings));
                    }
                }
                LayoutBlockV2::Table(table) => {
                    docx = docx.add_table(docx_table(table, &document.settings))
                }
                LayoutBlockV2::StructuredForm(fields) => {
                    docx = docx.add_table(docx_form_table(fields, &document.settings))
                }
                LayoutBlockV2::Image {
                    caption,
                    width_mm,
                    alignment,
                    crop,
                    asset,
                } => {
                    docx = docx
                        .add_paragraph(docx_image(asset, *width_mm, crop, alignment)?)
                        .add_paragraph(paragraph(caption, 20, &document.settings));
                }
                LayoutBlockV2::Attachment { label } => {
                    docx = docx.add_paragraph(paragraph(
                        &format!("[附件] {label}"),
                        20,
                        &document.settings,
                    ))
                }
                LayoutBlockV2::PreparedAttachment {
                    label,
                    pages,
                    start_new_page,
                } => {
                    if *start_new_page {
                        docx = docx.add_paragraph(
                            docx_paragraph().add_run(Run::new().add_break(BreakType::Page)),
                        );
                    }
                    docx = docx.add_paragraph(paragraph(
                        &format!("附件：{label}"),
                        22,
                        &document.settings,
                    ));
                    for page in pages {
                        docx = docx
                            .add_paragraph(
                                docx_paragraph().add_run(Run::new().add_break(BreakType::Page)),
                            )
                            .add_paragraph(docx_image(
                                page,
                                fitted_page_width(page, &document.settings)?,
                                &LayoutCropV2::default(),
                                "center",
                            )?);
                    }
                }
                LayoutBlockV2::PageBreak => {
                    docx = docx.add_paragraph(
                        docx_paragraph().add_run(Run::new().add_break(BreakType::Page)),
                    )
                }
                LayoutBlockV2::Signature { label } => {
                    docx = docx.add_paragraph(paragraph(
                        &format!("\n\n{label}：________________"),
                        22,
                        &document.settings,
                    ))
                }
            }
        }
    }
    let mut buffer = Cursor::new(Vec::new());
    docx.pack(&mut buffer).map_err(|error| error.to_string())?;
    patch_docx_table_headers(buffer.into_inner(), document)
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn render_html_rich(paragraphs: &[LayoutParagraphV2]) -> String {
    let mut html = String::new();
    for paragraph in paragraphs {
        html.push_str("<p>");
        if let Some(marker) = &paragraph.list_marker {
            html.push_str(&html_escape(marker));
        }
        for run in &paragraph.runs {
            let mut value = html_escape(&run.text).replace('\n', "<br>");
            if run.code {
                value = format!("<code>{value}</code>");
            }
            if run.bold {
                value = format!("<strong>{value}</strong>");
            }
            if run.italic {
                value = format!("<em>{value}</em>");
            }
            if run.underline {
                value = format!("<u>{value}</u>");
            }
            if run.strike {
                value = format!("<s>{value}</s>");
            }
            if let Some(link) = &run.link {
                value = format!("<a href=\"{}\">{value}</a>", html_escape(link));
            }
            html.push_str(&value);
        }
        html.push_str("</p>");
    }
    html
}

fn render_html_table(table: &LayoutTableV2) -> String {
    let owners = table_slot_owners(table);
    let mut html = String::from("<table><colgroup>");
    for width in &table.widths_mm {
        html.push_str(&format!("<col style=\"width:{}mm\">", width));
    }
    html.push_str("</colgroup>");
    if table.repeat_header_rows == 0 {
        html.push_str("<tbody>");
    }
    for row in 0..table.row_count {
        if row == 0 && table.repeat_header_rows > 0 {
            html.push_str("<thead>");
        }
        if row == table.repeat_header_rows && table.repeat_header_rows > 0 {
            html.push_str("</thead><tbody>");
        }
        html.push_str("<tr>");
        let mut column = 0;
        while column < table.column_count {
            let owner = owners[row * table.column_count + column];
            let cell = &table.cells[owner];
            if cell.row == row && cell.column == column {
                let tag = if row < table.repeat_header_rows {
                    "th"
                } else {
                    "td"
                };
                html.push_str(&format!(
                    "<{tag} rowspan=\"{}\" colspan=\"{}\">{}</{tag}>",
                    cell.rowspan,
                    cell.colspan,
                    if cell.paragraphs.is_empty() {
                        html_escape(&cell.text)
                    } else {
                        render_html_rich(&cell.paragraphs)
                    }
                ));
                column += cell.colspan;
            } else if cell.row < row && cell.column == column {
                column += cell.colspan;
            } else {
                column += 1;
            }
        }
        html.push_str("</tr>");
    }
    if table.repeat_header_rows == table.row_count && table.repeat_header_rows > 0 {
        html.push_str("</thead><tbody>");
    }
    html.push_str("</tbody></table>");
    html
}

pub fn render_html(document: &LayoutDocumentV2) -> Result<String, String> {
    validate_layout_dimensions(document)?;
    let margins = &document.settings.margins_mm;
    let mut html = format!(
        "<!doctype html><html lang=\"zh-CN\"><head><meta charset=\"utf-8\"><title>{}</title><style>@page{{size:A4;margin:{}mm {}mm {}mm {}mm}}body{{font-family:'{}','{}';font-size:{}pt;line-height:{}}}.page-break{{break-after:page}}table{{border-collapse:collapse;width:100%}}td{{border:1px solid #333;padding:4px}}figure img{{max-width:100%;height:auto}}code{{font-family:inherit}}</style></head><body>",
        html_escape(&document.title),
        margins.top,
        margins.right,
        margins.bottom,
        margins.left,
        "Noto Sans JP",
        "Noto Sans JP",
        document.settings.body_font_pt,
        document.settings.line_spacing
    );
    html.push_str(&format!(
        "<header>{}</header><h1>{}</h1>",
        html_escape(&document.settings.header),
        html_escape(&document.title)
    ));
    if let Some(watermark) = &document.watermark {
        html.push_str(&format!(
            "<p class=\"watermark\">【{}】</p>",
            html_escape(watermark)
        ));
    }
    if document.settings.include_toc {
        html.push_str("<nav><h2>目录</h2><ol>");
        for title in numbered_section_titles(document) {
            html.push_str(&format!("<li>{}</li>", html_escape(&title)));
        }
        html.push_str("</ol></nav>");
    }
    for (section, title) in document
        .sections
        .iter()
        .zip(numbered_section_titles(document))
    {
        let level = (section.depth + 2).min(6);
        html.push_str(&format!(
            "<section><h{level}>{}</h{level}>",
            html_escape(&title)
        ));
        for block in &section.blocks {
            match block {
                LayoutBlockV2::RichText(paragraphs) => html.push_str(&render_html_rich(paragraphs)),
                LayoutBlockV2::Table(table) => html.push_str(&render_html_table(table)),
                LayoutBlockV2::StructuredForm(fields) => {
                    html.push_str("<table class=\"structured-form\">");
                    for (label, value) in fields {
                        html.push_str(&format!(
                            "<tr><td>{}</td><td>{}</td></tr>",
                            html_escape(label),
                            html_escape(value)
                        ));
                    }
                    html.push_str("</table>");
                }
                LayoutBlockV2::Image {
                    caption,
                    width_mm,
                    alignment,
                    crop,
                    asset,
                } => {
                    let cropped = cropped_image(asset, crop)?;
                    let mut png = Cursor::new(Vec::new());
                    cropped
                        .write_to(&mut png, image::ImageFormat::Png)
                        .map_err(|error| format!("encode cropped HTML image: {error}"))?;
                    html.push_str(&format!(
                        "<figure data-asset-revision-id=\"{}\" style=\"text-align:{}\"><img alt=\"{}\" src=\"data:image/png;base64,{}\" style=\"width:{}mm;max-width:100%;height:auto\"><figcaption>{}</figcaption></figure>",
                        html_escape(&asset.asset_revision_id),html_escape(alignment),html_escape(caption),
                        base64::engine::general_purpose::STANDARD.encode(png.into_inner()),width_mm,
                        html_escape(caption)));
                }
                LayoutBlockV2::Attachment { label } => {
                    html.push_str(&format!("<p>[附件] {}</p>", html_escape(label)))
                }
                LayoutBlockV2::PreparedAttachment {
                    label,
                    pages,
                    start_new_page,
                } => {
                    if *start_new_page {
                        html.push_str("<hr class=\"page-break\">");
                    }
                    html.push_str(&format!("<p>附件：{}</p>", html_escape(label)));
                    for page in pages {
                        let style = fitted_page_width(page, &document.settings)
                            .map(|width_mm| {
                                format!(
                                    "display:block;width:{width_mm}mm;max-width:100%;height:auto"
                                )
                            })
                            .unwrap_or_else(|_| "display:block;max-width:100%;height:auto".into());
                        html.push_str(&format!("<img class=\"prepared-attachment-page page-break\" alt=\"{}\" src=\"data:{};base64,{}\" style=\"{}\">",
                            html_escape(&page.file_name),html_escape(&page.media_type),
                            base64::engine::general_purpose::STANDARD.encode(page.bytes.as_slice()),style));
                    }
                }
                LayoutBlockV2::PageBreak => html.push_str("<hr class=\"page-break\">"),
                LayoutBlockV2::Signature { label } => html.push_str(&format!(
                    "<p class=\"signature\">{}：________________</p>",
                    html_escape(label)
                )),
            }
        }
        html.push_str("</section>");
    }
    html.push_str(&format!(
        "<footer>{}</footer></body></html>",
        html_escape(&document.settings.footer)
    ));
    Ok(html)
}

fn block_lines(block: &LayoutBlockV2) -> Vec<String> {
    match block {
        LayoutBlockV2::RichText(paragraphs) => paragraphs
            .iter()
            .map(|paragraph| {
                format!(
                    "{}{}",
                    paragraph.list_marker.as_deref().unwrap_or_default(),
                    paragraph
                        .runs
                        .iter()
                        .map(|run| run.text.as_str())
                        .collect::<String>()
                )
            })
            .collect(),
        LayoutBlockV2::Table(table) => {
            let rows = table_cells_by_row(table);
            rows.into_iter()
                .map(|cells| {
                    cells
                        .into_iter()
                        .map(|index| table.cells[index].text.clone())
                        .collect::<Vec<_>>()
                        .join(" | ")
                })
                .collect()
        }
        LayoutBlockV2::StructuredForm(fields) => fields
            .iter()
            .map(|(key, value)| format!("{key}：{value}"))
            .collect(),
        LayoutBlockV2::Image { caption, .. } => vec![caption.clone()],
        LayoutBlockV2::Attachment { label } => vec![format!("[附件] {label}")],
        LayoutBlockV2::PreparedAttachment { label, .. } => vec![format!("附件：{label}")],
        LayoutBlockV2::PageBreak => Vec::new(),
        LayoutBlockV2::Signature { label } => vec![format!("{label}：________________")],
    }
}

fn glyph_width(character: char, size: f32, font: &ParsedFont) -> f32 {
    let units = font.font_metrics.units_per_em.max(1);
    let em = font
        .lookup_glyph_index(character as u32)
        .map(|glyph| font.get_horizontal_advance(glyph))
        .filter(|width| *width > 0)
        .map(|width| f32::from(width) / f32::from(units))
        .unwrap_or(if character.is_ascii() { 0.6 } else { 1.0 });
    em * size * 25.4 / 72.0
}

fn wrap(text: &str, size: f32, font: &ParsedFont, width_mm: f32) -> Vec<String> {
    let mut result = Vec::new();
    let mut line = String::new();
    let mut current = 0.0;
    for character in text.chars() {
        if character == '\n' {
            result.push(std::mem::take(&mut line));
            current = 0.0;
            continue;
        }
        let next = glyph_width(character, size, font);
        if !line.is_empty() && current + next > width_mm {
            result.push(std::mem::take(&mut line));
            current = 0.0;
        }
        line.push(character);
        current += next;
    }
    if !line.is_empty() || result.is_empty() {
        result.push(line);
    }
    result
}

fn fixed_pdf_text(ops: &mut Vec<Op>, font: &FontId, text: String, size: f32, x: f32, y: f32) {
    if text.is_empty() {
        return;
    }
    ops.extend([
        Op::StartTextSection,
        Op::SetTextCursor {
            pos: Point {
                x: Mm(x).into(),
                y: Mm(y).into(),
            },
        },
        Op::SetFont {
            font: PdfFontHandle::External(font.clone()),
            size: Pt(size),
        },
        Op::ShowText {
            items: vec![TextItem::Text(text)],
        },
        Op::EndTextSection,
    ]);
}

struct PdfFlow<'a> {
    ops: Vec<Op>,
    pages: Vec<Vec<Op>>,
    y: f32,
    font: &'a FontId,
    parsed: &'a ParsedFont,
    margins: &'a LayoutMarginsV2,
}

impl PdfFlow<'_> {
    fn page_break(&mut self) {
        if !self.ops.is_empty() {
            self.pages.push(std::mem::take(&mut self.ops));
        }
        self.y = PDF_PAGE_HEIGHT - self.margins.top;
    }
}

fn write_line_with_spacing(flow: &mut PdfFlow<'_>, text: &str, size: f32, line_spacing: f32) {
    let width = PDF_PAGE_WIDTH - flow.margins.left - flow.margins.right;
    let leading = size * 25.4 / 72.0 * line_spacing.max(1.0);
    for line in wrap(text, size, flow.parsed, width) {
        if flow.y < flow.margins.bottom + leading {
            flow.page_break();
        }
        flow.ops.extend([
            Op::StartTextSection,
            Op::SetTextCursor {
                pos: Point {
                    x: Mm(flow.margins.left).into(),
                    y: Mm(flow.y).into(),
                },
            },
            Op::SetFont {
                font: PdfFontHandle::External(flow.font.clone()),
                size: Pt(size),
            },
            Op::SetLineHeight {
                lh: Pt(size * line_spacing.max(1.0)),
            },
            Op::ShowText {
                items: vec![TextItem::Text(line)],
            },
            Op::EndTextSection,
        ]);
        flow.y -= leading;
    }
}

fn write_line(flow: &mut PdfFlow<'_>, text: &str, size: f32) {
    write_line_with_spacing(flow, text, size, 1.2)
}

fn pdf_run_style_eq(left: &LayoutTextRunV2, right: &LayoutTextRunV2) -> bool {
    left.bold == right.bold
        && left.italic == right.italic
        && left.underline == right.underline
        && left.strike == right.strike
        && left.code == right.code
        && left.link == right.link
}

fn append_pdf_character(line: &mut Vec<LayoutTextRunV2>, style: &LayoutTextRunV2, character: char) {
    if let Some(last) = line.last_mut()
        && pdf_run_style_eq(last, style)
    {
        last.text.push(character);
        return;
    }
    let mut segment = style.clone();
    segment.text = character.to_string();
    line.push(segment);
}

fn pdf_rich_lines(
    paragraph: &LayoutParagraphV2,
    size: f32,
    font: &ParsedFont,
    width_mm: f32,
) -> Vec<Vec<LayoutTextRunV2>> {
    let plain = LayoutTextRunV2 {
        text: String::new(),
        bold: false,
        italic: false,
        underline: false,
        strike: false,
        code: false,
        link: None,
    };
    let mut runs = Vec::new();
    if let Some(marker) = &paragraph.list_marker {
        let mut marker_run = plain;
        marker_run.text = marker.clone();
        runs.push(marker_run);
    }
    runs.extend(paragraph.runs.clone());

    let mut lines = Vec::new();
    let mut line = Vec::new();
    let mut line_width = 0.0;
    for run in runs {
        for character in run.text.chars() {
            if character == '\n' {
                lines.push(std::mem::take(&mut line));
                line_width = 0.0;
                continue;
            }
            let character_width = glyph_width(character, size, font);
            if line_width + character_width > width_mm && !line.is_empty() {
                lines.push(std::mem::take(&mut line));
                line_width = 0.0;
            }
            append_pdf_character(&mut line, &run, character);
            line_width += character_width;
        }
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

fn pdf_decoration_line(flow: &mut PdfFlow<'_>, x: f32, y: f32, width: f32) {
    flow.ops.extend([
        Op::SetOutlineThickness { pt: Pt(0.6) },
        Op::DrawLine {
            line: Line {
                points: vec![
                    LinePoint {
                        p: Point {
                            x: Mm(x).into(),
                            y: Mm(y).into(),
                        },
                        bezier: false,
                    },
                    LinePoint {
                        p: Point {
                            x: Mm(x + width).into(),
                            y: Mm(y).into(),
                        },
                        bezier: false,
                    },
                ],
                is_closed: false,
            },
        },
    ]);
}

fn draw_pdf_rich_segments(
    flow: &mut PdfFlow<'_>,
    line: Vec<LayoutTextRunV2>,
    size: f32,
    leading: f32,
    mut x: f32,
    y: f32,
) {
    for run in line {
        let run_width = run
            .text
            .chars()
            .map(|character| glyph_width(character, size, flow.parsed))
            .sum::<f32>();
        let x_pt: Pt = Mm(x).into();
        let y_pt: Pt = Mm(y).into();
        flow.ops.push(Op::StartTextSection);
        flow.ops.push(Op::SetFont {
            font: PdfFontHandle::External(flow.font.clone()),
            size: Pt(size),
        });
        flow.ops.push(Op::SetTextRenderingMode {
            mode: if run.bold {
                TextRenderingMode::FillStroke
            } else {
                TextRenderingMode::Fill
            },
        });
        if run.bold {
            flow.ops.push(Op::SetOutlineThickness { pt: Pt(0.35) });
        }
        flow.ops.push(if run.italic {
            Op::SetTextMatrix {
                matrix: TextMatrix::Raw([1.0, 0.0, 0.22, 1.0, x_pt.0, y_pt.0]),
            }
        } else {
            Op::SetTextCursor {
                pos: Point { x: x_pt, y: y_pt },
            }
        });
        flow.ops.push(Op::ShowText {
            items: vec![TextItem::Text(run.text.clone())],
        });
        flow.ops.push(Op::EndTextSection);
        flow.ops.push(Op::SetTextRenderingMode {
            mode: TextRenderingMode::Fill,
        });
        if run.underline || run.link.is_some() {
            pdf_decoration_line(flow, x, y - 0.8, run_width);
        }
        if run.strike {
            pdf_decoration_line(flow, x, y + leading * 0.28, run_width);
        }
        if let Some(link) = run.link {
            flow.ops.push(Op::LinkAnnotation {
                link: LinkAnnotation::new(
                    Rect::from_xywh(
                        x_pt,
                        Mm(y - 1.0).into(),
                        Mm(run_width).into(),
                        Pt(size * 1.2),
                    ),
                    Actions::uri(link),
                    None,
                    None,
                    None,
                ),
            });
        }
        x += run_width;
    }
}

fn write_pdf_rich_paragraph(
    flow: &mut PdfFlow<'_>,
    paragraph: &LayoutParagraphV2,
    size: f32,
    line_spacing: f32,
) {
    let width = PDF_PAGE_WIDTH - flow.margins.left - flow.margins.right;
    let leading = size * 25.4 / 72.0 * line_spacing.max(1.0);
    for line in pdf_rich_lines(paragraph, size, flow.parsed, width) {
        if flow.y < flow.margins.bottom + leading {
            flow.page_break();
        }
        draw_pdf_rich_segments(flow, line, size, leading, flow.margins.left, flow.y);
        flow.y -= leading;
    }
}

struct PdfTableRow<'a> {
    table: &'a LayoutTableV2,
    row: usize,
    y_top: f32,
    widths: &'a [f32],
    row_heights: &'a [f32],
    row_cells: &'a [Vec<usize>],
    font_size: f32,
    leading: f32,
}

fn draw_pdf_table_row(flow: &mut PdfFlow<'_>, row_input: PdfTableRow<'_>) -> Result<(), String> {
    let PdfTableRow {
        table,
        row,
        y_top,
        widths,
        row_heights,
        row_cells,
        font_size,
        leading,
    } = row_input;
    for index in &row_cells[row] {
        let cell = &table.cells[*index];
        let x = flow.margins.left + widths[..cell.column].iter().sum::<f32>();
        let width = widths[cell.column..cell.column + cell.colspan]
            .iter()
            .sum::<f32>();
        let height = row_heights[cell.row..cell.row + cell.rowspan]
            .iter()
            .sum::<f32>();
        let bottom = y_top - height;
        let points = [
            (x, y_top),
            (x + width, y_top),
            (x + width, bottom),
            (x, bottom),
        ]
        .into_iter()
        .map(|(x, y)| LinePoint {
            p: Point {
                x: Mm(x).into(),
                y: Mm(y).into(),
            },
            bezier: false,
        })
        .collect();
        flow.ops.extend([
            Op::SetOutlineThickness { pt: Pt(0.5) },
            Op::DrawLine {
                line: Line {
                    points,
                    is_closed: true,
                },
            },
        ]);
        let mut text_y = y_top - leading;
        if cell.paragraphs.is_empty() {
            for line in wrap(&cell.text, font_size, flow.parsed, (width - 2.0).max(1.0)) {
                if text_y < bottom + 1.0 {
                    return Err("table row measurement did not fit drawn text".into());
                }
                fixed_pdf_text(&mut flow.ops, flow.font, line, font_size, x + 1.0, text_y);
                text_y -= leading;
            }
        } else {
            for paragraph in &cell.paragraphs {
                for line in
                    pdf_rich_lines(paragraph, font_size, flow.parsed, (width - 2.0).max(1.0))
                {
                    if text_y < bottom + 1.0 {
                        return Err("table row measurement did not fit drawn rich text".into());
                    }
                    draw_pdf_rich_segments(flow, line, font_size, leading, x + 1.0, text_y);
                    text_y -= leading;
                }
            }
        }
    }
    Ok(())
}

fn write_pdf_table(
    flow: &mut PdfFlow<'_>,
    table: &LayoutTableV2,
    font_size: f32,
    line_spacing: f32,
) -> Result<(), String> {
    let widths = table.widths_mm.clone();
    let row_cells = table_cells_by_row(table);
    let leading = font_size * 25.4 / 72.0 * line_spacing;
    let mut row_heights = vec![(leading + 4.0).max(7.0); table.row_count];
    for cell in &table.cells {
        let width = widths[cell.column..cell.column + cell.colspan]
            .iter()
            .sum::<f32>();
        let line_count = if cell.paragraphs.is_empty() {
            wrap(&cell.text, font_size, flow.parsed, (width - 2.0).max(1.0)).len()
        } else {
            cell.paragraphs
                .iter()
                .map(|paragraph| {
                    pdf_rich_lines(paragraph, font_size, flow.parsed, (width - 2.0).max(1.0)).len()
                })
                .sum()
        };
        let required = line_count as f32 * leading + 3.0;
        let current = row_heights[cell.row..cell.row + cell.rowspan]
            .iter()
            .sum::<f32>();
        if required > current {
            row_heights[cell.row + cell.rowspan - 1] += required - current;
        }
    }
    let header_rows = table.repeat_header_rows.min(table.row_count);
    let header_height = row_heights[..header_rows].iter().sum::<f32>();
    let printable_height = PDF_PAGE_HEIGHT - flow.margins.top - flow.margins.bottom;
    if header_height > printable_height {
        return Err("table header is taller than the printable page".into());
    }
    let span_end_by_start = table_protected_row_ends(table, &row_cells);
    let mut row = 0;
    while row < table.row_count {
        let protected_end = span_end_by_start[row];
        let protected_height = row_heights[row..protected_end].iter().sum::<f32>();
        let printable_height = PDF_PAGE_HEIGHT - flow.margins.top - flow.margins.bottom;
        if protected_height > printable_height {
            return Err("table row is taller than the printable page".into());
        }
        if row >= header_rows
            && header_rows > 0
            && header_height + protected_height > printable_height
        {
            return Err(
                "repeated table header and protected row group exceed the printable page".into(),
            );
        }
        if flow.y - protected_height < flow.margins.bottom {
            flow.page_break();
            if row >= header_rows
                && header_rows > 0
                && flow.y - header_height >= flow.margins.bottom
            {
                for header in 0..header_rows {
                    draw_pdf_table_row(
                        flow,
                        PdfTableRow {
                            table,
                            row: header,
                            y_top: flow.y,
                            widths: &widths,
                            row_heights: &row_heights,
                            row_cells: &row_cells,
                            font_size,
                            leading,
                        },
                    )?;
                    flow.y -= row_heights[header];
                }
            }
        }
        draw_pdf_table_row(
            flow,
            PdfTableRow {
                table,
                row,
                y_top: flow.y,
                widths: &widths,
                row_heights: &row_heights,
                row_cells: &row_cells,
                font_size,
                leading,
            },
        )?;
        flow.y -= row_heights[row];
        row += 1;
    }
    flow.y -= 2.0;
    Ok(())
}

fn write_pdf_image(
    pdf: &mut PdfDocument,
    flow: &mut PdfFlow<'_>,
    asset: &FrozenLayoutAssetV2,
    width_mm: f32,
    crop: &LayoutCropV2,
    alignment: &str,
) -> Result<(), String> {
    let cropped = cropped_image(asset, crop)?;
    let mut png = Cursor::new(Vec::new());
    cropped
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|error| error.to_string())?;
    let bytes = png.into_inner();
    let mut warnings = Vec::new();
    let image = RawImage::decode_from_bytes(&bytes, &mut warnings)
        .map_err(|error| format!("decode frozen image {}: {error}", asset.asset_revision_id))?;
    if image.width == 0 || image.height == 0 {
        return Err("frozen image has zero dimensions".into());
    }
    let target_width = width_mm;
    let target_height = target_width * (image.height as f32 / image.width as f32);
    if flow.y - target_height < flow.margins.bottom {
        flow.page_break();
    }
    let dpi = 300.0;
    let natural_width_pt = image.width as f32 * 72.0 / dpi;
    let natural_height_pt = image.height as f32 * 72.0 / dpi;
    let id = XObjectId(format!(
        "kb-bid-v2-image-{}",
        hex::encode(Sha256::digest(&bytes))
    ));
    pdf.resources
        .xobjects
        .map
        .insert(id.clone(), XObject::Image(image));
    let available = PDF_PAGE_WIDTH - flow.margins.left - flow.margins.right;
    let x = match alignment {
        "right" => flow.margins.left + available - target_width,
        "center" => flow.margins.left + (available - target_width) / 2.0,
        _ => flow.margins.left,
    };
    flow.ops.push(Op::UseXobject {
        id,
        transform: XObjectTransform {
            translate_x: Some(Mm(x).into()),
            translate_y: Some(Mm(flow.y - target_height).into()),
            scale_x: Some(Pt::from(Mm(target_width)).0 / natural_width_pt),
            scale_y: Some(Pt::from(Mm(target_height)).0 / natural_height_pt),
            dpi: Some(dpi),
            ..Default::default()
        },
    });
    flow.y -= target_height + 3.0;
    Ok(())
}

pub fn render_pdf(document: &LayoutDocumentV2) -> Result<Vec<u8>, String> {
    validate_layout_dimensions(document)?;
    let actual = hex::encode(Sha256::digest(PDF_FONT_BYTES));
    if actual != PDF_FONT_SHA256 {
        return Err(format!("frozen PDF font digest mismatch: {actual}"));
    }
    let mut warnings = Vec::new();
    let parsed =
        ParsedFont::from_bytes(PDF_FONT_BYTES, 0, &mut warnings).ok_or("parse frozen PDF font")?;
    let document_id = hex::encode(Sha256::digest(
        [
            PDF_RENDERER_CONTRACT.as_bytes(),
            PDF_FONT_SHA256.as_bytes(),
            &serde_json::to_vec(document).map_err(|error| error.to_string())?,
        ]
        .concat(),
    ));
    let mut pdf = PdfDocument::new(&document.title);
    pdf.metadata.info.creator = PDF_RENDERER_CONTRACT.into();
    pdf.metadata.info.producer = format!("{PDF_RENDERER_CONTRACT};font-sha256={PDF_FONT_SHA256}");
    pdf.metadata.info.identifier = document_id.clone();
    let font = FontId(PDF_FONT_RESOURCE_ID.into());
    pdf.resources
        .fonts
        .map
        .insert(font.clone(), printpdf::font::PdfFont::new(parsed.clone()));
    let margins = &document.settings.margins_mm;
    let mut flow = PdfFlow {
        ops: Vec::new(),
        pages: Vec::new(),
        y: PDF_PAGE_HEIGHT - margins.top,
        font: &font,
        parsed: &parsed,
        margins,
    };
    write_line(&mut flow, &document.title, 18.0);
    if let Some(watermark) = &document.watermark {
        write_line(&mut flow, &format!("【{watermark}】"), 10.0);
    }
    if document.settings.include_toc {
        write_line(&mut flow, "目录", 15.0);
        for title in numbered_section_titles(document) {
            write_line(&mut flow, &title, 10.0);
        }
        flow.page_break();
    }
    let titles = numbered_section_titles(document);
    for (section, section_title) in document.sections.iter().zip(titles) {
        write_line(
            &mut flow,
            &section_title,
            (15.0 - section.depth as f32).max(11.0),
        );
        for block in &section.blocks {
            match block {
                LayoutBlockV2::PageBreak => flow.page_break(),
                LayoutBlockV2::Image {
                    caption,
                    width_mm,
                    alignment,
                    crop,
                    asset,
                } => {
                    write_pdf_image(&mut pdf, &mut flow, asset, *width_mm, crop, alignment)?;
                    write_line(&mut flow, caption, 10.0);
                }
                LayoutBlockV2::RichText(paragraphs) => {
                    for paragraph in paragraphs {
                        write_pdf_rich_paragraph(
                            &mut flow,
                            paragraph,
                            document.settings.body_font_pt,
                            document.settings.line_spacing,
                        );
                    }
                }
                LayoutBlockV2::Table(table) => write_pdf_table(
                    &mut flow,
                    table,
                    document.settings.body_font_pt,
                    document.settings.line_spacing,
                )?,
                LayoutBlockV2::PreparedAttachment {
                    label,
                    pages: attachment_pages,
                    start_new_page,
                } => {
                    if *start_new_page {
                        flow.page_break();
                    }
                    write_line(&mut flow, &format!("附件：{label}"), 11.0);
                    for page in attachment_pages {
                        flow.page_break();
                        write_pdf_image(
                            &mut pdf,
                            &mut flow,
                            page,
                            fitted_page_width(page, &document.settings)?,
                            &LayoutCropV2::default(),
                            "center",
                        )?;
                    }
                }
                _ => {
                    for line in block_lines(block) {
                        write_line_with_spacing(
                            &mut flow,
                            &line,
                            document.settings.body_font_pt,
                            document.settings.line_spacing,
                        );
                    }
                }
            }
        }
    }
    if !flow.ops.is_empty() {
        flow.pages.push(flow.ops);
    }
    let total_pages = flow.pages.len();
    let pages = flow
        .pages
        .into_iter()
        .enumerate()
        .map(|(index, mut ops)| {
            fixed_pdf_text(
                &mut ops,
                &font,
                document.settings.header.clone(),
                9.0,
                margins.left,
                PDF_PAGE_HEIGHT - margins.top / 2.0,
            );
            let footer = match document.settings.page_number.as_str() {
                "none" => document.settings.footer.clone(),
                _ if document.settings.footer.is_empty() => {
                    format!("第 {} / {} 页", index + 1, total_pages)
                }
                _ => format!(
                    "{}  第 {} / {} 页",
                    document.settings.footer,
                    index + 1,
                    total_pages
                ),
            };
            let footer_width = footer
                .chars()
                .map(|character| glyph_width(character, 9.0, &parsed))
                .sum::<f32>();
            let footer_x = match document.settings.page_number.as_str() {
                "footer_outside" if index % 2 == 0 => {
                    (PDF_PAGE_WIDTH - margins.right - footer_width).max(margins.left)
                }
                "footer_outside" => margins.left,
                _ => ((PDF_PAGE_WIDTH - footer_width) / 2.0).max(margins.left),
            };
            fixed_pdf_text(&mut ops, &font, footer, 9.0, footer_x, margins.bottom / 2.0);
            PdfPage::new(Mm(PDF_PAGE_WIDTH), Mm(PDF_PAGE_HEIGHT), ops)
        })
        .collect();
    let mut save_warnings = Vec::new();
    let mut output = pdf
        .with_pages(pages)
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
        .map_err(|error| error.to_string())?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn workspace() -> Value {
        json!({"nodes":[{"title":"技术方案","depth":0,"render_role":"section","block_lineage_ids":["b1"]}],"blocks":[{"lineage_id":"b1","kind":"rich_text","content":{"type":"rich_text","nodes":[{"kind":"paragraph","content":[{"kind":"text","text":"中文投标正文"}]}]}}]})
    }

    #[test]
    fn mixed_outline_numbering_resets_below_each_top_level() {
        let document = LayoutDocumentV2 {
            title: "投标文件".into(),
            sections: vec![
                LayoutSectionV2 {
                    title: "商务文件".into(),
                    depth: 1,
                    blocks: vec![],
                },
                LayoutSectionV2 {
                    title: "投标函".into(),
                    depth: 2,
                    blocks: vec![],
                },
                LayoutSectionV2 {
                    title: "授权委托书".into(),
                    depth: 3,
                    blocks: vec![],
                },
                LayoutSectionV2 {
                    title: "资格文件".into(),
                    depth: 2,
                    blocks: vec![],
                },
                LayoutSectionV2 {
                    title: "技术文件".into(),
                    depth: 1,
                    blocks: vec![],
                },
                LayoutSectionV2 {
                    title: "技术要求响应".into(),
                    depth: 2,
                    blocks: vec![],
                },
                LayoutSectionV2 {
                    title: "参数响应表".into(),
                    depth: 3,
                    blocks: vec![],
                },
            ],
            watermark: None,
            settings: LayoutSettingsV2::default(),
        };
        assert_eq!(
            numbered_section_titles(&document),
            vec![
                "一、商务文件",
                "1. 投标函",
                "1.1 授权委托书",
                "2. 资格文件",
                "二、技术文件",
                "1. 技术要求响应",
                "1.1 参数响应表",
            ]
        );
    }

    #[test]
    fn cover_and_toc_nodes_do_not_enter_body_numbering() {
        let value = json!({
            "nodes":[
                {"title":"投标文件","depth":0,"semantic_role":"cover","render_role":"front_matter","block_lineage_ids":[]},
                {"title":"目录","depth":1,"semantic_role":"toc","render_role":"toc","block_lineage_ids":[]},
                {"title":"商务文件","depth":1,"semantic_role":"commercial","render_role":"section","block_lineage_ids":["b1"]}
            ],
            "blocks":[{"lineage_id":"b1","kind":"rich_text","content":{"type":"rich_text","nodes":[{"kind":"paragraph","content":[{"kind":"text","text":"正文"}]}]}}]
        });
        let layout = layout_from_workspace("投标文件", &value, None).unwrap();
        assert_eq!(numbered_section_titles(&layout), vec!["一、商务文件"]);
    }

    #[test]
    fn frozen_layout_renders_real_docx_and_a4_pdf() {
        let layout =
            layout_from_workspace("投标文件", &workspace(), Some("评审稿".into())).unwrap();
        let docx = render_docx(&layout).unwrap();
        let pdf = render_pdf(&layout).unwrap();
        assert!(docx.starts_with(b"PK"));
        assert!(pdf.starts_with(b"%PDF-"));
        assert!(docx.len() > 1_000);
        assert!(pdf.len() > 10_000);
    }

    #[test]
    fn renderer_is_byte_replayable() {
        let layout = layout_from_workspace("投标文件", &workspace(), None).unwrap();
        let first = render_docx(&layout).unwrap();
        let second = render_docx(&layout).unwrap();
        if first != second {
            let entries = |bytes: &[u8]| {
                let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
                (0..archive.len())
                    .map(|index| {
                        let mut entry = archive.by_index(index).unwrap();
                        let name = entry.name().to_owned();
                        let mut body = Vec::new();
                        std::io::Read::read_to_end(&mut entry, &mut body).unwrap();
                        (name, hex::encode(Sha256::digest(body)))
                    })
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                entries(&first),
                entries(&second),
                "DOCX package entries differ"
            );
            panic!("DOCX ZIP envelope is not deterministic");
        }
        assert_eq!(render_pdf(&layout).unwrap(), render_pdf(&layout).unwrap());
    }

    #[test]
    fn canonical_table_preserves_grid_spans_widths_headers_and_typography() {
        use std::io::Read;
        let rich =
            |text: &str| json!([{"kind":"paragraph","content":[{"kind":"text","text":text}]}]);
        let workspace = json!({"nodes":[{"title":"报价表","depth":0,"render_role":"section","block_lineage_ids":["t1"]}],"blocks":[{
            "lineage_id":"t1","kind":"table","content":{"type":"table","row_count":2,"column_count":2,
                "cells":[{"row":0,"column":0,"rowspan":1,"colspan":2,"content":[{"kind":"paragraph","content":[{"kind":"text","text":"表头","marks":[{"kind":"bold"},{"kind":"italic"}]}]}]},
                    {"row":1,"column":0,"rowspan":1,"colspan":1,"content":rich("名称")},
                    {"row":1,"column":1,"rowspan":1,"colspan":1,"content":rich("金额")}],
                "widths_mm":[60.0,80.0],"repeat_header_rows":1}}
        ]});
        let layout = layout_from_workspace("投标文件", &workspace, None).unwrap();
        let LayoutBlockV2::Table(table) = &layout.sections[0].blocks[0] else {
            panic!("table layout missing")
        };
        assert_eq!(
            (
                table.row_count,
                table.column_count,
                table.repeat_header_rows
            ),
            (2, 2, 1)
        );
        assert_eq!(table.cells[0].colspan, 2);
        let html = render_html(&layout).unwrap();
        assert!(html.contains("<thead>"));
        assert!(html.contains("colspan=\"2\""));
        assert!(html.contains("width:60mm"));
        assert!(html.contains("<em><strong>表头</strong></em>"));
        let docx = render_docx(&layout).unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(docx)).unwrap();
        let mut xml = String::new();
        archive
            .by_name("word/document.xml")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        assert!(xml.contains("w:gridSpan"));
        assert!(xml.contains("<w:tblHeader />"));
        assert!(xml.contains("Noto Sans JP"));
        assert!(xml.contains("w:line=\"360\""));
        assert!(xml.contains("<w:b"));
        assert!(xml.contains("<w:i"));
        let pdf = render_pdf(&layout).unwrap();
        assert!(pdf.starts_with(b"%PDF-"));
        let parsed = lopdf::Document::load_mem(&pdf).unwrap();
        let operators = parsed
            .get_pages()
            .values()
            .flat_map(|page_id| {
                lopdf::content::Content::decode(&parsed.get_page_content(*page_id))
                    .unwrap()
                    .operations
            })
            .map(|operation| operation.operator)
            .collect::<Vec<_>>();
        assert!(operators.iter().any(|operator| operator == "Tr"));
        assert!(operators.iter().any(|operator| operator == "Tm"));
    }

    #[test]
    fn frozen_image_and_form_render_in_both_formats() {
        use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
        use zip::ZipArchive;
        let mut png = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(40, 20, Rgba([12, 80, 160, 255])))
            .write_to(&mut png, ImageFormat::Png)
            .unwrap();
        let bytes = png.into_inner();
        let digest = hex::encode(Sha256::digest(&bytes));
        let asset = FrozenLayoutAssetV2 {
            asset_revision_id: "00000000-0000-4000-8000-000000000901".into(),
            sha256: digest,
            media_type: "image/png".into(),
            file_name: "方案图.png".into(),
            bytes: Arc::new(bytes),
        };
        let workspace = json!({"nodes":[{"title":"技术方案","depth":0,"render_role":"section","block_lineage_ids":["b1","b2"]}],"blocks":[
            {"lineage_id":"b1","kind":"image","content":{"type":"image","asset_revision_id":asset.asset_revision_id,"width_mm":120,"alignment":"center","crop":{"left":0.25,"top":0,"right":0.25,"bottom":0},"alt":"架构图"}},
            {"lineage_id":"b2","kind":"structured_form","content":{"type":"structured_form","form_definition_revision_id":"00000000-0000-4000-8000-000000000902","field_values":[{"field_id":"company","value":"知识脑"}]}}
        ]});
        let forms = vec![
            json!({"form_definition_revision_id":"00000000-0000-4000-8000-000000000902","fields":[{"field_id":"company","label":"公司名称"}]}),
        ];
        let layout = layout_from_workspace_with_resources(
            "投标文件",
            &workspace,
            &[asset],
            &forms,
            &[],
            None,
        )
        .unwrap();
        let docx = render_docx(&layout).unwrap();
        let pdf = render_pdf(&layout).unwrap();
        let mut archive = ZipArchive::new(Cursor::new(&docx)).unwrap();
        assert!((0..archive.len()).any(|index| {
            archive
                .by_index(index)
                .unwrap()
                .name()
                .starts_with("word/media/")
        }));
        let parsed_pdf = lopdf::Document::load_mem(&pdf).unwrap();
        assert!(parsed_pdf.objects.values().any(|object| {
            match object {
                lopdf::Object::Stream(stream) => stream
                    .dict
                    .get(b"Subtype")
                    .ok()
                    .is_some_and(|value| value.as_name().ok() == Some(b"Image")),
                _ => false,
            }
        }));
        let html = render_html(&layout).unwrap();
        assert!(html.contains("data:image/png;base64,"));
        assert!(!html.contains("clip-path"));
        let encoded = html
            .split("data:image/png;base64,")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        let html_image = image::load_from_memory(
            &base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(html_image.dimensions(), (20, 20));
        assert!(!html.contains("class=\"frozen-image\""));
        assert_eq!(docx, render_docx(&layout).unwrap());
        assert_eq!(pdf, render_pdf(&layout).unwrap());
    }

    #[test]
    fn rich_text_marks_lists_and_links_survive_layout_and_rendering() {
        use std::io::Read as _;
        use zip::ZipArchive;

        let workspace = json!({
            "nodes":[{"title":"技术方案","depth":0,"render_role":"section","block_lineage_ids":["rich"]}],
            "blocks":[{"lineage_id":"rich","kind":"rich_text","content":{"type":"rich_text","nodes":[
                {"kind":"paragraph","content":[
                    {"kind":"text","text":"加粗","marks":[{"kind":"bold"}]},
                    {"kind":"text","text":"链接","marks":[{"kind":"link","href":"https://example.com"}]},
                    {"kind":"text","text":"修订","marks":[{"kind":"underline"},{"kind":"strike"},{"kind":"code"}]}
                ]},
                {"kind":"bullet_list","content":[{"kind":"list_item","content":[
                    {"kind":"paragraph","content":[{"kind":"text","text":"列表项","marks":[{"kind":"italic"}]}]}
                ]}]}
            ]}}]
        });
        let layout = layout_from_workspace("投标文件", &workspace, None).unwrap();
        let LayoutBlockV2::RichText(paragraphs) = &layout.sections[0].blocks[0] else {
            panic!("rich text layout was flattened");
        };
        assert!(paragraphs[0].runs[0].bold);
        assert_eq!(
            paragraphs[0].runs[1].link.as_deref(),
            Some("https://example.com")
        );
        assert_eq!(paragraphs[1].list_marker.as_deref(), Some("• "));
        let html = render_html(&layout).unwrap();
        assert!(html.contains("<strong>加粗</strong>"));
        assert!(html.contains("href=\"https://example.com\""));
        assert!(html.contains("<s><u><code>修订</code></u></s>"));
        assert!(html.contains("<em>列表项</em>"));

        let docx = render_docx(&layout).unwrap();
        let mut archive = ZipArchive::new(Cursor::new(docx)).unwrap();
        let mut document_xml = String::new();
        archive
            .by_name("word/document.xml")
            .unwrap()
            .read_to_string(&mut document_xml)
            .unwrap();
        assert!(document_xml.contains("<w:b"));
        assert!(document_xml.contains("<w:i"));
        assert!(document_xml.contains("<w:u"));
        assert!(document_xml.contains("<w:strike"));
        assert!(!document_xml.contains("Courier New"));
        assert!(document_xml.contains("<w:hyperlink"));

        let pdf = render_pdf(&layout).unwrap();
        let parsed = lopdf::Document::load_mem(&pdf).unwrap();
        let pages = parsed.get_pages();
        assert!(
            pages.values().any(|page_id| parsed
                .get_object(*page_id)
                .unwrap()
                .as_dict()
                .unwrap()
                .has(b"Annots")),
            "PDF link annotation missing"
        );
        let operators = pages
            .values()
            .flat_map(|page_id| {
                lopdf::content::Content::decode(&parsed.get_page_content(*page_id))
                    .unwrap()
                    .operations
            })
            .map(|operation| operation.operator)
            .collect::<Vec<_>>();
        assert!(operators.iter().any(|operator| operator == "Tr"));
        assert!(operators.iter().any(|operator| operator == "Tm"));
        assert!(operators.iter().any(|operator| operator == "l"));
    }

    #[test]
    fn unsafe_layout_links_and_missing_assets_fail_closed() {
        let workspace = json!({
            "nodes":[{"title":"技术方案","depth":0,"render_role":"section","block_lineage_ids":["rich"]}],
            "blocks":[{"lineage_id":"rich","kind":"rich_text","content":{"type":"rich_text","nodes":[
                {"kind":"paragraph","content":[{"kind":"text","text":"链接","marks":[{"kind":"link","href":"https://example.com"}]}]}
            ]}}]
        });
        let mut layout = layout_from_workspace("投标文件", &workspace, None).unwrap();
        let LayoutBlockV2::RichText(paragraphs) = &mut layout.sections[0].blocks[0] else {
            panic!("rich text layout missing")
        };
        paragraphs[0].runs[0].link = Some("javascript:alert(1)".into());
        assert!(render_html(&layout).is_err());
        assert!(render_docx(&layout).is_err());
        assert!(render_pdf(&layout).is_err());

        let missing = json!({
            "nodes":[{"title":"图片","depth":0,"render_role":"section","block_lineage_ids":["image"]}],
            "blocks":[{"lineage_id":"image","kind":"image","content":{"type":"image",
                "asset_revision_id":"00000000-0000-4000-8000-000000000999","width_mm":100,
                "alignment":"center","crop":{"left":0,"top":0,"right":0,"bottom":0},"alt":"缺失"}}]
        });
        assert!(layout_from_workspace("投标文件", &missing, None).is_err());
    }

    #[test]
    fn preview_uses_a_safe_placeholder_until_pdf_pages_are_prepared() {
        let asset = FrozenLayoutAssetV2 {
            asset_revision_id: "00000000-0000-4000-8000-000000000903".into(),
            sha256: "a".repeat(64),
            media_type: "application/pdf".into(),
            file_name: "资质附件.pdf".into(),
            bytes: Arc::new(b"%PDF-1.4 fixture".to_vec()),
        };
        let workspace = json!({
            "nodes":[{"title":"附件","depth":0,"render_role":"section","block_lineage_ids":["attachment"]}],
            "blocks":[{"lineage_id":"attachment","kind":"attachment_ref","content":{
                "type":"attachment_ref","asset_revision_id":asset.asset_revision_id,
                "preparation_revision_id":null,"render_mode":"embedded_pages","start_new_page":true
            }}]
        });
        assert!(
            layout_from_workspace_with_resources(
                "投标文件",
                &workspace,
                std::slice::from_ref(&asset),
                &[],
                &[],
                None,
            )
            .is_err()
        );
        let preview = layout_preview_from_workspace_with_resources(
            "投标文件",
            &workspace,
            &[asset],
            &[],
            &[],
            None,
        )
        .unwrap();
        let LayoutBlockV2::Attachment { label } = &preview.sections[0].blocks[0] else {
            panic!("unprepared preview did not use an attachment placeholder");
        };
        assert!(label.contains("导出时嵌入页面"));

        let mut page_bytes = Vec::new();
        image::DynamicImage::new_rgb8(10, 20)
            .write_to(&mut Cursor::new(&mut page_bytes), image::ImageFormat::Png)
            .unwrap();
        let mut prepared = preview;
        prepared.sections[0].blocks[0] = LayoutBlockV2::PreparedAttachment {
            label: "资质附件.pdf".into(),
            pages: vec![FrozenLayoutAssetV2 {
                asset_revision_id: "00000000-0000-4000-8000-000000000904".into(),
                sha256: platform::sha256_hex(&page_bytes),
                media_type: "image/png".into(),
                file_name: "page-1.png".into(),
                bytes: Arc::new(page_bytes),
            }],
            start_new_page: true,
        };
        let html = render_html(&prepared).unwrap();
        assert!(html.contains("class=\"prepared-attachment-page page-break\""));
        assert!(html.contains("style=\"display:block;width:123.1mm;max-width:100%;height:auto\""));
    }

    #[test]
    fn repeated_pdf_header_plus_body_group_must_fit_the_printable_page() {
        let paragraph = |text: String| LayoutParagraphV2 {
            list_marker: None,
            runs: vec![LayoutTextRunV2 {
                text,
                bold: false,
                italic: false,
                underline: false,
                strike: false,
                code: false,
                link: None,
            }],
        };
        let document = |rowspan: usize| LayoutDocumentV2 {
            title: "table".into(),
            sections: vec![LayoutSectionV2 {
                title: "section".into(),
                depth: 0,
                blocks: vec![LayoutBlockV2::Table(LayoutTableV2 {
                    row_count: 1 + rowspan,
                    column_count: 1,
                    cells: vec![
                        LayoutTableCellV2 {
                            row: 0,
                            column: 0,
                            rowspan: 1,
                            colspan: 1,
                            text: String::new(),
                            paragraphs: vec![paragraph("header\n".repeat(20))],
                        },
                        LayoutTableCellV2 {
                            row: 1,
                            column: 0,
                            rowspan,
                            colspan: 1,
                            text: String::new(),
                            paragraphs: vec![paragraph("body\n".repeat(20))],
                        },
                    ],
                    widths_mm: vec![150.0],
                    repeat_header_rows: 1,
                })],
            }],
            watermark: None,
            settings: LayoutSettingsV2 {
                include_toc: false,
                ..Default::default()
            },
        };
        for rowspan in [1, 2] {
            assert_eq!(
                render_pdf(&document(rowspan)).unwrap_err(),
                "repeated table header and protected row group exceed the printable page"
            );
        }
    }

    #[test]
    fn layout_table_span_overflow_is_rejected_without_panicking() {
        let document = LayoutDocumentV2 {
            title: "table".into(),
            sections: vec![LayoutSectionV2 {
                title: "section".into(),
                depth: 0,
                blocks: vec![LayoutBlockV2::Table(LayoutTableV2 {
                    row_count: 1,
                    column_count: 1,
                    cells: vec![LayoutTableCellV2 {
                        row: usize::MAX,
                        column: usize::MAX,
                        rowspan: 2,
                        colspan: 2,
                        text: String::new(),
                        paragraphs: vec![],
                    }],
                    widths_mm: vec![150.0],
                    repeat_header_rows: 0,
                })],
            }],
            watermark: None,
            settings: LayoutSettingsV2::default(),
        };
        assert!(validate_layout_dimensions(&document).is_err());
    }

    #[test]
    fn pdf_rich_table_counts_multiline_paragraphs_and_rejects_oversized_rows() {
        let run = |text: &str| LayoutTextRunV2 {
            text: text.into(),
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            code: false,
            link: None,
        };
        let table = |paragraphs: Vec<LayoutParagraphV2>| LayoutTableV2 {
            row_count: 1,
            column_count: 1,
            cells: vec![LayoutTableCellV2 {
                row: 0,
                column: 0,
                rowspan: 1,
                colspan: 1,
                text: String::new(),
                paragraphs,
            }],
            widths_mm: vec![150.0],
            repeat_header_rows: 0,
        };
        let document = |table| LayoutDocumentV2 {
            title: "table".into(),
            sections: vec![LayoutSectionV2 {
                title: "section".into(),
                depth: 0,
                blocks: vec![LayoutBlockV2::Table(table)],
            }],
            watermark: None,
            settings: LayoutSettingsV2 {
                include_toc: false,
                ..Default::default()
            },
        };
        let multiline = table(vec![
            LayoutParagraphV2 {
                list_marker: None,
                runs: vec![run("line one\nline two")],
            },
            LayoutParagraphV2 {
                list_marker: None,
                runs: vec![run("paragraph two")],
            },
        ]);
        assert!(
            render_pdf(&document(multiline))
                .unwrap()
                .starts_with(b"%PDF-")
        );

        let oversized = table(vec![LayoutParagraphV2 {
            list_marker: None,
            runs: vec![run(&"line\n".repeat(500))],
        }]);
        assert_eq!(
            render_pdf(&document(oversized)).unwrap_err(),
            "table row is taller than the printable page"
        );
    }

    fn pdf_with_page_box(width: u32, height: u32, rotation: Option<i32>) -> Vec<u8> {
        let rotate = rotation.map_or(String::new(), |value| format!(" /Rotate {value}"));
        let objects = [
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}]{rotate} /Resources << >> >>"
            )
            .into_bytes(),
        ];
        let mut result = b"%PDF-1.4\n".to_vec();
        let mut offsets = vec![0];
        for (index, value) in objects.iter().enumerate() {
            offsets.push(result.len());
            result.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
            result.extend_from_slice(value);
            result.extend_from_slice(b"\nendobj\n");
        }
        let xref = result.len();
        result.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
        );
        for offset in offsets.into_iter().skip(1) {
            result.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        result.extend_from_slice(
            format!(
                "trailer << /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        result
    }

    #[test]
    fn frozen_pdf_geometry_is_exact_and_rejects_oversized_media_box_before_raster() {
        assert_eq!(
            frozen_pdf_raster_geometry(&pdf_with_page_box(400, 200, None)).unwrap(),
            vec![(800, 400)]
        );
        assert_eq!(
            frozen_pdf_raster_geometry(&pdf_with_page_box(400, 200, Some(90))).unwrap(),
            vec![(400, 800)]
        );
        assert!(frozen_pdf_raster_geometry(&pdf_with_page_box(20_000, 20_000, None)).is_err());
    }

    #[test]
    fn staggered_rowspans_form_one_transitive_page_break_group() {
        let cell = |row, column, rowspan, colspan| LayoutTableCellV2 {
            row,
            column,
            rowspan,
            colspan,
            text: String::new(),
            paragraphs: vec![],
        };
        let table = LayoutTableV2 {
            row_count: 4,
            column_count: 2,
            cells: vec![
                cell(0, 0, 1, 2),
                cell(1, 0, 2, 1),
                cell(1, 1, 1, 1),
                cell(2, 1, 2, 1),
                cell(3, 0, 1, 1),
            ],
            widths_mm: vec![70.0, 70.0],
            repeat_header_rows: 1,
        };
        let rows = table_cells_by_row(&table);
        assert_eq!(table_protected_row_ends(&table, &rows), vec![1, 4, 4, 4]);
    }
}
