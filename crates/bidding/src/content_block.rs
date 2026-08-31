//! Closed ContentBlockV1 domain representation.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

/// Validates the closed link contract shared by persisted content and renderers.
pub fn validate_http_link(value: &str) -> Result<(), &'static str> {
    if value.is_empty()
        || value.chars().count() > 2_048
        || !(value.starts_with("http://") || value.starts_with("https://"))
        || matches!(value, "http://" | "https://")
        || value
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err("link is invalid");
    }
    let authority = value
        .strip_prefix("http://")
        .or_else(|| value.strip_prefix("https://"))
        .and_then(|suffix| suffix.split(['/', '?', '#']).next())
        .ok_or("link is invalid")?;
    if authority.is_empty() {
        return Err("link is invalid");
    }
    let parsed = url::Url::parse(value).map_err(|_| "link is invalid")?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err("link is invalid");
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockKind {
    RichText,
    Table,
    Image,
    AttachmentRef,
    StructuredForm,
    PageBreak,
    SignaturePlaceholder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockOrigin {
    Human,
    AgentCandidate,
    Deterministic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TextMark {
    Bold,
    Italic,
    Underline,
    Strike,
    Code,
    Link {
        href: String,
    },
    EvidenceRef {
        evidence_bundle_id: Uuid,
        evidence_item_id: Uuid,
        quote_start_offset: u64,
        quote_end_offset: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Inline {
    Text {
        text: String,
        #[serde(default)]
        marks: Vec<TextMark>,
    },
    HardBreak,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Paragraph {
    Paragraph { content: Vec<Inline> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ListItem {
    ListItem { content: Vec<Paragraph> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RichNode {
    Paragraph { content: Vec<Inline> },
    BulletList { content: Vec<ListItem> },
    OrderedList { content: Vec<ListItem> },
    Blockquote { content: Vec<Paragraph> },
    CodeBlock { language: String, text: String },
    HorizontalRule,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TableCell {
    pub row: usize,
    pub column: usize,
    pub rowspan: usize,
    pub colspan: usize,
    pub content: Vec<RichNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Crop {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageAlignment {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentRenderMode {
    EmbeddedPages,
    FileReference,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormFieldValue {
    pub field_id: String,
    pub value: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignatureKind {
    Signature,
    Seal,
    Date,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum BlockContent {
    RichText {
        nodes: Vec<RichNode>,
    },
    Table {
        row_count: usize,
        column_count: usize,
        cells: Vec<TableCell>,
        widths_mm: Vec<f64>,
        repeat_header_rows: usize,
    },
    Image {
        asset_revision_id: Uuid,
        width_mm: f64,
        alignment: ImageAlignment,
        crop: Crop,
        #[serde(default)]
        caption: Option<String>,
        alt: String,
    },
    AttachmentRef {
        asset_revision_id: Uuid,
        #[serde(default)]
        preparation_revision_id: Option<Uuid>,
        render_mode: AttachmentRenderMode,
        start_new_page: bool,
    },
    StructuredForm {
        form_definition_revision_id: Uuid,
        field_values: Vec<FormFieldValue>,
    },
    PageBreak,
    SignaturePlaceholder {
        signature_kind: SignatureKind,
        width_mm: f64,
        height_mm: f64,
        label: String,
    },
}

impl BlockContent {
    pub const fn kind(&self) -> BlockKind {
        match self {
            Self::RichText { .. } => BlockKind::RichText,
            Self::Table { .. } => BlockKind::Table,
            Self::Image { .. } => BlockKind::Image,
            Self::AttachmentRef { .. } => BlockKind::AttachmentRef,
            Self::StructuredForm { .. } => BlockKind::StructuredForm,
            Self::PageBreak => BlockKind::PageBreak,
            Self::SignaturePlaceholder { .. } => BlockKind::SignaturePlaceholder,
        }
    }

    pub fn sha256(&self) -> Result<String, serde_json::Error> {
        Ok(hex::encode(Sha256::digest(serde_json::to_vec(self)?)))
    }

    fn validate_rich(nodes: &[RichNode]) -> Result<(), &'static str> {
        if nodes.len() > 10_000 {
            return Err("rich content exceeds node limit");
        }
        for node in nodes {
            match node {
                RichNode::Paragraph { content } => Self::validate_inline(content)?,
                RichNode::HorizontalRule => {}
                RichNode::CodeBlock { language, text } => {
                    if language.len() > 64 || text.len() > 65_536 {
                        return Err("code block exceeds limit");
                    }
                }
                RichNode::Blockquote { content } => {
                    if content.len() > 1_000 {
                        return Err("blockquote length is invalid");
                    }
                    for paragraph in content {
                        let Paragraph::Paragraph { content } = paragraph;
                        Self::validate_inline(content)?;
                    }
                }
                RichNode::BulletList { content } | RichNode::OrderedList { content } => {
                    if content.is_empty() || content.len() > 1_000 {
                        return Err("list length is invalid");
                    }
                    for item in content {
                        let ListItem::ListItem { content } = item;
                        if content.is_empty() || content.len() > 1_000 {
                            return Err("list item length is invalid");
                        }
                        for paragraph in content {
                            let Paragraph::Paragraph { content } = paragraph;
                            Self::validate_inline(content)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn validate_inline(content: &[Inline]) -> Result<(), &'static str> {
        if content.len() > 10_000 {
            return Err("inline content exceeds limit");
        }
        for inline in content {
            if let Inline::Text { text, marks } = inline {
                if text.len() > 65_536 || marks.len() > 16 {
                    return Err("inline text or marks exceed limit");
                }
                for mark in marks {
                    match mark {
                        TextMark::Link { href } if validate_http_link(href).is_err() => {
                            return Err("link is invalid");
                        }
                        TextMark::EvidenceRef {
                            quote_start_offset,
                            quote_end_offset,
                            ..
                        } if quote_end_offset <= quote_start_offset => {
                            return Err("evidence quote range is invalid");
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::RichText { nodes } => Self::validate_rich(nodes),
            Self::Table {
                row_count,
                column_count,
                cells,
                widths_mm,
                repeat_header_rows,
            } => {
                if !(1..=10_000).contains(row_count)
                    || !(1..=256).contains(column_count)
                    || cells.is_empty()
                    || cells.len() > 100_000
                    || widths_mm.len() != *column_count
                    || *repeat_header_rows > *row_count
                    || widths_mm
                        .iter()
                        .any(|width| !width.is_finite() || *width <= 0.0 || *width > 200.0)
                    || widths_mm.iter().sum::<f64>() > 200.0
                {
                    return Err("table dimensions are invalid");
                }
                let slots = row_count
                    .checked_mul(*column_count)
                    .ok_or("table dimensions overflow")?;
                let mut cover = vec![false; slots];
                for cell in cells {
                    let row_end = cell.row.checked_add(cell.rowspan);
                    let column_end = cell.column.checked_add(cell.colspan);
                    if cell.rowspan == 0
                        || cell.colspan == 0
                        || row_end.is_none_or(|end| end > *row_count)
                        || column_end.is_none_or(|end| end > *column_count)
                        || (cell.row < *repeat_header_rows
                            && row_end.is_none_or(|end| end > *repeat_header_rows))
                    {
                        return Err("table cell is out of bounds or crosses the header boundary");
                    }
                    let row_end = row_end.expect("checked above");
                    let column_end = column_end.expect("checked above");
                    Self::validate_rich(&cell.content)?;
                    for row in cell.row..row_end {
                        for column in cell.column..column_end {
                            let slot = row * column_count + column;
                            if cover[slot] {
                                return Err("table cells overlap");
                            }
                            cover[slot] = true;
                        }
                    }
                }
                if cover.iter().any(|covered| !covered) {
                    return Err("table grid has uncovered cells");
                }
                Ok(())
            }
            Self::Image {
                width_mm,
                crop,
                caption,
                alt,
                ..
            } => {
                if !width_mm.is_finite()
                    || *width_mm <= 0.0
                    || *width_mm > 1_000.0
                    || [crop.left, crop.top, crop.right, crop.bottom]
                        .iter()
                        .any(|v| !v.is_finite() || *v < 0.0 || *v >= 1.0)
                    || crop.left + crop.right >= 1.0
                    || crop.top + crop.bottom >= 1.0
                    || caption.as_ref().is_some_and(|value| value.len() > 4_096)
                    || alt.len() > 4_096
                {
                    return Err("image layout is invalid");
                }
                Ok(())
            }
            Self::StructuredForm { field_values, .. } => {
                if field_values.len() > 10_000
                    || field_values.iter().any(|field| {
                        field.field_id.is_empty()
                            || field.field_id.len() > 256
                            || field.value.len() > 65_536
                    })
                {
                    return Err("structured form values are invalid");
                }
                Ok(())
            }
            Self::SignaturePlaceholder {
                width_mm,
                height_mm,
                label,
                ..
            } => {
                if !width_mm.is_finite()
                    || !height_mm.is_finite()
                    || *width_mm <= 0.0
                    || *height_mm <= 0.0
                    || *width_mm > 1_000.0
                    || *height_mm > 1_000.0
                    || label.is_empty()
                    || label.len() > 65_536
                {
                    return Err("signature placeholder is invalid");
                }
                Ok(())
            }
            Self::AttachmentRef { .. } | Self::PageBreak => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentBlockV1 {
    pub schema_version: u8,
    pub block_revision_id: Uuid,
    pub lineage_id: Uuid,
    pub revision: u64,
    pub kind: BlockKind,
    pub content: BlockContent,
    pub origin: BlockOrigin,
    pub content_sha256: String,
}

impl ContentBlockV1 {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema_version != 1 || self.revision == 0 {
            return Err("content block identity is invalid");
        }
        if self.kind != self.content.kind() {
            return Err("content block kind does not match content type");
        }
        if !is_sha256(&self.content_sha256) {
            return Err("content block digest is invalid");
        }
        self.content.validate()?;
        let digest = self
            .content
            .sha256()
            .map_err(|_| "content serialization failed")?;
        if digest != self.content_sha256 {
            return Err("content block digest mismatch");
        }
        Ok(())
    }
}

pub fn validate_content_block(value: &serde_json::Value) -> Result<(), String> {
    let block: ContentBlockV1 = serde_json::from_value(value.clone())
        .map_err(|error| format!("content block schema invalid: {error}"))?;
    block.validate().map_err(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn block(content: serde_json::Value, kind: &str) -> serde_json::Value {
        let typed: BlockContent = serde_json::from_value(content).unwrap();
        json!({
            "schema_version":1,"block_revision_id":Uuid::new_v4(),"lineage_id":Uuid::new_v4(),
            "revision":1,"kind":kind,"content":typed,"origin":"human",
            "content_sha256":typed.sha256().unwrap()
        })
    }

    #[test]
    fn closed_rich_text_is_valid() {
        validate_content_block(&block(
            json!({"type":"rich_text","nodes":[{"kind":"paragraph","content":[]}]}),
            "rich_text",
        ))
        .unwrap();
    }

    #[test]
    fn links_match_the_closed_http_uri_contract() {
        for invalid in [
            "https://example.com bad",
            "https://example.com/line\nbreak",
            "ftp://example.com/file",
            "https:///missing-host",
            "https://user:secret@example.com/path",
        ] {
            assert!(validate_http_link(invalid).is_err(), "accepted {invalid:?}");
        }
        for valid in [
            "https://example.com/a%20b?x=1&y=%E4%B8%AD",
            "http://example.com:8080/path#fragment",
        ] {
            validate_http_link(valid).unwrap();
        }
    }

    #[test]
    fn table_span_arithmetic_overflow_is_rejected_without_panicking() {
        let table = BlockContent::Table {
            row_count: 1,
            column_count: 1,
            cells: vec![TableCell {
                row: usize::MAX,
                column: usize::MAX,
                rowspan: 2,
                colspan: 2,
                content: vec![],
            }],
            widths_mm: vec![100.0],
            repeat_header_rows: 0,
        };
        assert!(table.validate().is_err());
    }

    #[test]
    fn unknown_fields_and_overlapping_tables_are_rejected() {
        let mut rich = block(json!({"type":"rich_text","nodes":[]}), "rich_text");
        rich.as_object_mut()
            .unwrap()
            .insert("unknown".into(), json!(true));
        assert!(validate_content_block(&rich).is_err());
        let table = block(
            json!({"type":"table","row_count":1,"column_count":1,
            "cells":[{"row":0,"column":0,"rowspan":1,"colspan":1,"content":[]},
                     {"row":0,"column":0,"rowspan":1,"colspan":1,"content":[]}],
            "widths_mm":[100.0],"repeat_header_rows":0}),
            "table",
        );
        assert!(validate_content_block(&table).is_err());
    }
}
