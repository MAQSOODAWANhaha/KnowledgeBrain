//! Deterministic DOCX primitives for a source-backed composition plan.
//! No Workspace body, old outline, bidder facts or previous DOCX is an input.
use crate::content_block::BlockContent;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Write},
};

mod text_regions;
pub use text_regions::TextRegion;
pub(crate) use text_regions::{initial_text_fragment, region_bookmark_name, resolve_text_regions};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateStyle {
    pub width_mm: f64,
    pub height_mm: f64,
    pub top_mm: f64,
    pub right_mm: f64,
    pub bottom_mm: f64,
    pub left_mm: f64,
    pub font_family: String,
    pub body_font_pt: f64,
    pub line_spacing: f64,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TemplatePlan {
    pub title: String,
    pub toc_title: String,
    pub style: TemplateStyle,
    pub sections: Vec<TemplateSection>,
    pub excluded_sources: Vec<ExcludedSource>,
    pub excluded_forms: Vec<ExcludedForm>,
    pub notices: Vec<String>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateSection {
    pub title: String,
    #[serde(default, skip_serializing_if = "SectionPlacement::is_body")]
    pub placement: SectionPlacement,
    pub depth: usize,
    pub source_ids: Vec<String>,
    pub blocks: Vec<TemplateBlock>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SectionPlacement {
    FrontMatter,
    #[default]
    Body,
}
impl SectionPlacement {
    pub fn is_body(&self) -> bool {
        *self == Self::Body
    }
}

fn toc(plan: &TemplatePlan) -> Result<String, TemplateError> {
    let mut out = paragraph(&plan.toc_title, None)?;
    out += "<w:p><w:r><w:fldChar w:fldCharType=\"begin\" w:dirty=\"true\"/></w:r><w:r><w:instrText xml:space=\"preserve\"> TOC \\o &quot;1-9&quot; \\h \\z \\u </w:instrText></w:r><w:r><w:fldChar w:fldCharType=\"separate\"/></w:r></w:p>";
    for section in plan.sections.iter().filter(|s| s.placement.is_body()) {
        out += &paragraph(&section.title, None)?;
    }
    out += "<w:p><w:r><w:fldChar w:fldCharType=\"end\"/></w:r><w:r><w:br w:type=\"page\"/></w:r></w:p>";
    Ok(out)
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateBlock {
    pub kind: String,
    /// Compiler-owned, contiguous original text with independently located blanks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub text_regions: Vec<TextRegion>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_parts: Vec<SourcePart>,
    pub source_id: Option<String>,
    pub quote: Option<String>,
    /// Reviewed applicability explanation, distinct from a verbatim quotation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    pub form_id: Option<String>,
    pub header_rows: usize,
    pub blank_cells: Vec<CellRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blank_ranges: Vec<crate::template_grid::CellTextRange>,
    pub columns: Vec<String>,
    pub blank_rows: usize,
    /// 回读保留内容的收据 key，指向 `input["preserved_units"]` 里的单元。块本身
    /// 不带正文，和 `quote`/`source_excerpt` 一样只引用已验证的来源。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preserved_key: Option<String>,
}

/// Exact frozen evidence; there is deliberately no model-supplied text field.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourcePart {
    Text {
        source_id: String,
        start: usize,
        end: usize,
    },
    GridCell {
        source_id: String,
        form_id: String,
        row: usize,
        column: usize,
    },
}
impl SourcePart {
    pub fn source_id(&self) -> &str {
        match self {
            Self::Text { source_id, .. } | Self::GridCell { source_id, .. } => source_id,
        }
    }
    pub fn form_id(&self) -> Option<&str> {
        match self {
            Self::GridCell { form_id, .. } => Some(form_id),
            Self::Text { .. } => None,
        }
    }
}

pub fn resolve_source_parts(input: &Value, parts: &[SourcePart]) -> Result<String, TemplateError> {
    check(!parts.is_empty(), "source excerpt has no parts")?;
    let mut out = String::new();
    for part in parts {
        let source = input["source_units"]
            .as_array()
            .and_then(|sources| {
                sources
                    .iter()
                    .find(|s| s["source_unit_revision_id"] == part.source_id())
            })
            .ok_or_else(|| invalid("excerpt source missing"))?;
        let value = match part {
            SourcePart::Text { start, end, .. } => {
                check(start < end, "empty or reversed excerpt range")?;
                source["text"]
                    .as_str()
                    .and_then(|s| s.get(*start..*end))
                    .ok_or_else(|| invalid("excerpt must use valid UTF-8 byte boundaries"))?
            }
            SourcePart::GridCell {
                form_id,
                row,
                column,
                ..
            } => {
                let form = input["structured_forms"]
                    .as_array()
                    .and_then(|forms| {
                        forms.iter().find(|f| {
                            f["form_definition_revision_id"] == *form_id
                                && f["source_unit_revision_id"] == part.source_id()
                        })
                    })
                    .ok_or_else(|| invalid("excerpt grid does not belong to source"))?;
                check(
                    crate::tender_analysis::relations::grid_cell_is_anchor(
                        &form["definition"],
                        *row,
                        *column,
                    ),
                    "excerpt cell must be a merged-cell anchor",
                )?;
                form["definition"]["cells"]
                    .as_array()
                    .and_then(|cells| {
                        cells
                            .iter()
                            .find(|c| c["row"] == *row && c["column"] == *column)
                    })
                    .and_then(|c| c["text"].as_str())
                    .ok_or_else(|| invalid("excerpt cell text missing"))?
            }
        };
        check(!value.trim().is_empty(), "empty source excerpt part")?;
        out.push_str(value);
    }
    Ok(out)
}
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct CellRef {
    pub row: usize,
    pub column: usize,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExcludedSource {
    pub source_id: String,
    pub reason: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExcludedForm {
    pub form_id: String,
    pub reason: String,
}

#[derive(Debug, thiserror::Error)]
pub enum TemplateError {
    #[error("template output invalid: {0}")]
    Output(String),
}
fn invalid(message: impl Into<String>) -> TemplateError {
    TemplateError::Output(message.into())
}
fn check(ok: bool, message: &str) -> Result<(), TemplateError> {
    if ok { Ok(()) } else { Err(invalid(message)) }
}
fn text(value: &str) -> Result<String, TemplateError> {
    check(
        value.chars().all(|c| {
            matches!(c, '\t' | '\n' | '\r') || (c >= ' ' && c != '\u{fffe}' && c != '\u{ffff}')
        }),
        "invalid XML character",
    )?;
    Ok(value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;"))
}
fn paragraph(value: &str, style: Option<&str>) -> Result<String, TemplateError> {
    Ok(format!(
        "<w:p>{}<w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>",
        style
            .map(|s| format!("<w:pPr><w:pStyle w:val=\"{s}\"/></w:pPr>"))
            .unwrap_or_default(),
        text(value)?
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .replace('\n', "</w:t><w:br/><w:t xml:space=\"preserve\">")
            .replace('\t', "</w:t><w:tab/><w:t xml:space=\"preserve\">")
    ))
}
fn twips(mm: f64) -> usize {
    (mm * 1440.0 / 25.4).round() as usize
}

/// Produce only new OOXML bytes. Fixed text must occur in the frozen sources;
/// cells and source/form coverage are checked before any package is returned.
/// 回读保留内容只能从收据里取：块给 key，内容在 `input["preserved_units"]` 里，
/// 那份 map 由宿主从绑 `file_sha256` 的 docreader 收据构造。
fn preserved_unit<'a>(
    input: &'a Value,
    block: &TemplateBlock,
    kind: &str,
) -> Result<&'a Value, TemplateError> {
    let key = block
        .preserved_key
        .as_deref()
        .ok_or_else(|| invalid("preserved block has no receipt key"))?;
    let unit = input["preserved_units"]
        .get(key)
        .ok_or_else(|| invalid("preserved content is not in the readback receipt"))?;
    check(
        unit["kind"] == kind,
        "preserved receipt kind does not match the primitive",
    )?;
    Ok(unit)
}

pub fn compile_template(input: &Value, plan: &TemplatePlan) -> Result<Vec<u8>, TemplateError> {
    let sources = input["source_units"]
        .as_array()
        .ok_or_else(|| invalid("source registry missing"))?;
    let forms = input["structured_forms"]
        .as_array()
        .ok_or_else(|| invalid("form registry missing"))?;
    let sources: BTreeMap<_, _> = sources
        .iter()
        .map(|v| {
            Ok((
                v["source_unit_revision_id"]
                    .as_str()
                    .ok_or_else(|| invalid("source id"))?,
                v["text"].as_str().ok_or_else(|| invalid("source text"))?,
            ))
        })
        .collect::<Result<_, TemplateError>>()?;
    let forms: BTreeMap<_, _> = forms
        .iter()
        .map(|v| {
            Ok((
                v["form_definition_revision_id"]
                    .as_str()
                    .ok_or_else(|| invalid("form id"))?,
                v,
            ))
        })
        .collect::<Result<_, TemplateError>>()?;
    check(
        !sources.is_empty() && !plan.sections.is_empty(),
        "complete template requires sources and sections",
    )?;
    check(
        !plan.title.trim().is_empty() && !plan.toc_title.trim().is_empty(),
        "title required",
    )?;
    let s = &plan.style;
    check(
        [s.width_mm, s.height_mm, s.body_font_pt, s.line_spacing]
            .iter()
            .all(|v| v.is_finite() && *v > 0.0)
            && [s.top_mm, s.right_mm, s.bottom_mm, s.left_mm]
                .iter()
                .all(|v| v.is_finite() && *v >= 0.0)
            && s.width_mm > s.left_mm + s.right_mm
            && s.height_mm > s.top_mm + s.bottom_mm
            && twips(s.width_mm) <= 31680
            && twips(s.height_mm) <= 31680
            && s.body_font_pt <= 1638.0
            && s.line_spacing <= 100.0
            && !s.font_family.trim().is_empty(),
        "invalid page/font dimensions",
    )?;
    let printable = s.width_mm - s.left_mm - s.right_mm;
    let mut used_sources = BTreeSet::new();
    let mut used_forms = BTreeSet::new();
    let mut body = paragraph(&plan.title, Some("Title"))?;
    let mut toc_written = false;
    let mut last_depth = 0;
    let mut range_id = 0u32;
    let mut cells_used = 0usize;
    for (ordinal, section) in plan.sections.iter().enumerate() {
        if section.placement.is_body() && !toc_written {
            check(section.depth == 0, "body must start at root depth")?;
            if ordinal > 0 {
                body += "<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>";
            }
            body += &toc(plan)?;
            toc_written = true;
        }
        check(
            section.placement.is_body() || (!toc_written && section.depth == 0),
            "front matter must be root content before the body",
        )?;
        check(
            section.depth < 9
                && (ordinal != 0 || section.depth == 0)
                && section.depth <= last_depth + 1,
            "invalid chapter hierarchy",
        )?;
        check(
            !section.title.trim().is_empty()
                && !section.source_ids.is_empty()
                && !section.blocks.is_empty(),
            "empty chapter structure or provenance",
        )?;
        last_depth = section.depth;
        for id in &section.source_ids {
            check(sources.contains_key(id.as_str()), "foreign chapter source")?;
            used_sources.insert(id.as_str());
        }
        body += &bookmark(ordinal, None, true, range_id);
        body += &paragraph(
            &section.title,
            Some(&if section.placement.is_body() {
                format!("Heading{}", section.depth + 1)
            } else {
                "Title".into()
            }),
        )?;
        body += &bookmark(ordinal, None, false, range_id);
        range_id += 1;
        for (block_index, block) in section.blocks.iter().enumerate() {
            let block_range_id = range_id;
            check(
                block.kind == "condition" || block.condition.is_none(),
                "applicability annotation cannot be hidden in another block kind",
            )?;
            body += &bookmark(ordinal, Some(block_index), true, range_id);
            if let Some(id) = &block.source_id {
                check(
                    section.source_ids.contains(id),
                    "block source outside chapter",
                )?;
            }
            check(
                block.kind == "source_excerpt" || block.source_parts.is_empty(),
                "source parts require source_excerpt primitive",
            )?;
            check(
                block.kind == "table" || block.blank_ranges.is_empty(),
                "partial cell blanks require a frozen table",
            )?;
            check(
                block.kind == "text_regions" || block.text_regions.is_empty(),
                "text regions require their own primitive",
            )?;
            check(
                matches!(
                    block.kind.as_str(),
                    "preserved_paragraphs" | "preserved_table"
                ) == block.preserved_key.is_some(),
                "preserved content requires its own primitive and a receipt key",
            )?;
            match block.kind.as_str() {
                "text_regions" => {
                    check(
                        block.quote.is_none()
                            && block.form_id.is_none()
                            && block.header_rows == 0
                            && block.blank_cells.is_empty()
                            && block.columns.is_empty()
                            && block.blank_rows == 0,
                        "unexpected text region fields",
                    )?;
                    let fragments = resolve_text_regions(input, block)?;
                    body += "<w:p>";
                    for (index, fragment) in fragments.iter().enumerate() {
                        range_id = range_id
                            .checked_add(1)
                            .ok_or_else(|| invalid("too many document ranges"))?;
                        let name = region_bookmark_name(ordinal, block_index, index);
                        body +=
                            &format!("<w:bookmarkStart w:id=\"{range_id}\" w:name=\"{name}\"/>");
                        let run = paragraph(fragment, None)?;
                        body += run
                            .strip_prefix("<w:p>")
                            .and_then(|s| s.strip_suffix("</w:p>"))
                            .ok_or_else(|| invalid("paragraph carrier missing"))?;
                        body += &format!("<w:bookmarkEnd w:id=\"{range_id}\"/>");
                    }
                    body += "</w:p>";
                }
                "source_excerpt" => {
                    check(
                        block.source_id.is_none()
                            && block.quote.is_none()
                            && block.condition.is_none()
                            && block.form_id.is_none()
                            && block.columns.is_empty()
                            && block.blank_cells.is_empty()
                            && block.blank_rows == 0
                            && block.header_rows == 0,
                        "unexpected source excerpt fields",
                    )?;
                    for part in &block.source_parts {
                        check(
                            section.source_ids.iter().any(|id| id == part.source_id()),
                            "excerpt source outside chapter",
                        )?;
                        if let Some(id) = part.form_id() {
                            used_forms.insert(id);
                        }
                    }
                    body += &paragraph(&resolve_source_parts(input, &block.source_parts)?, None)?;
                }
                "condition" => {
                    check(
                        block
                            .source_id
                            .as_ref()
                            .is_some_and(|id| sources.contains_key(id.as_str()))
                            && block.quote.is_none()
                            && block.form_id.is_none()
                            && block.columns.is_empty()
                            && block.blank_cells.is_empty()
                            && block.blank_rows == 0
                            && block.header_rows == 0,
                        "invalid conditional template annotation",
                    )?;
                    let condition = block
                        .condition
                        .as_deref()
                        .filter(|s| !s.trim().is_empty())
                        .ok_or_else(|| invalid("conditional template annotation is empty"))?;
                    body += &paragraph(&format!("适用条件：{condition}"), None)?;
                }
                "quote" => {
                    let source = block
                        .source_id
                        .as_ref()
                        .and_then(|id| sources.get(id.as_str()))
                        .ok_or_else(|| invalid("quote source missing"))?;
                    let quote = block
                        .quote
                        .as_deref()
                        .filter(|t| !t.is_empty())
                        .ok_or_else(|| invalid("empty fixed wording"))?;
                    check(
                        source.contains(quote),
                        "fixed wording is not verbatim source text",
                    )?;
                    check(
                        block.form_id.is_none()
                            && block.columns.is_empty()
                            && block.blank_cells.is_empty()
                            && block.blank_rows == 0
                            && block.header_rows == 0,
                        "unexpected quote fields",
                    )?;
                    let (initial, _) = initial_text_fragment(quote, false, false, &mut false);
                    for line in initial.split('\n') {
                        body += &paragraph(line, None)?;
                    }
                }
                "blank" => {
                    check(
                        block.source_id.is_none()
                            && block.quote.is_none()
                            && block.form_id.is_none()
                            && block.columns.is_empty()
                            && block.blank_cells.is_empty()
                            && block.header_rows == 0
                            && block.blank_rows == 0,
                        "unexpected blank fields",
                    )?;
                    let (initial, _) = initial_text_fragment("", true, false, &mut false);
                    body += &paragraph(&initial, None)?;
                }
                "table" => {
                    check(
                        block.quote.is_none() && block.columns.is_empty() && block.blank_rows == 0,
                        "unexpected frozen table fields",
                    )?;
                    let id = block
                        .form_id
                        .as_deref()
                        .ok_or_else(|| invalid("form missing"))?;
                    let form = forms
                        .get(id)
                        .ok_or_else(|| invalid("foreign frozen form"))?;
                    check(
                        section
                            .source_ids
                            .iter()
                            .any(|id| form["source_unit_revision_id"] == *id),
                        "form outside chapter sources",
                    )?;
                    used_forms.insert(id);
                    let def = &form["definition"];
                    let columns = def["column_count"]
                        .as_u64()
                        .ok_or_else(|| invalid("column count"))?
                        as usize;
                    check(columns > 0 && columns <= 1000, "table capacity")?;
                    let policies = (0..columns)
                        .map(|column| json!({"column":column,"role":"copy_verbatim"}))
                        .collect::<Vec<_>>();
                    let table = crate::template_grid::table_block_from_grid(
                        def,
                        &policies,
                        block.header_rows,
                    )
                    .map_err(invalid)?;
                    let BlockContent::Table {
                        row_count,
                        column_count,
                        cells,
                        widths_mm,
                        repeat_header_rows,
                    } = table
                    else {
                        return Err(invalid("grid required"));
                    };
                    let blank: BTreeSet<_> = block.blank_cells.iter().collect();
                    check(
                        blank.len() == block.blank_cells.len()
                            && blank.iter().all(|p| {
                                p.row >= repeat_header_rows
                                    && cells.iter().any(|c| c.row == p.row && c.column == p.column)
                            }),
                        "invalid blank anchor or blank header",
                    )?;
                    check(
                        block.blank_ranges.iter().all(|r| {
                            r.row >= repeat_header_rows
                                && cells.iter().any(|c| c.row == r.row && c.column == r.column)
                                && !blank.contains(&CellRef {
                                    row: r.row,
                                    column: r.column,
                                })
                        }),
                        "partial blank must use a non-header anchor without a whole-cell blank",
                    )?;
                    let raw = def["cells"]
                        .as_array()
                        .ok_or_else(|| invalid("cells missing"))?;
                    let anchors = cells
                        .iter()
                        .map(|c| {
                            let value = raw
                                .iter()
                                .find(|v| v["row"] == c.row && v["column"] == c.column)
                                .and_then(|v| v["text"].as_str())
                                .ok_or_else(|| invalid("cell text missing"))?;
                            Ok((
                                c.row,
                                c.column,
                                c.rowspan,
                                c.colspan,
                                if blank.contains(&CellRef {
                                    row: c.row,
                                    column: c.column,
                                }) {
                                    String::new()
                                } else {
                                    crate::template_grid::blank_cell_text(
                                        value,
                                        c.row,
                                        c.column,
                                        &block.blank_ranges,
                                    )
                                    .map_err(invalid)?
                                },
                            ))
                        })
                        .collect::<Result<Vec<_>, TemplateError>>()?;
                    let anchors: Vec<_> = anchors
                        .iter()
                        .map(|(row, column, rowspan, colspan, value)| {
                            (*row, *column, *rowspan, *colspan, value.as_str())
                        })
                        .collect();
                    cells_used = cells_used
                        .checked_add(row_count * column_count)
                        .ok_or_else(|| invalid("table capacity"))?;
                    check(cells_used <= 100000, "aggregate table capacity")?;
                    body += &table_xml(
                        row_count,
                        column_count,
                        &anchors,
                        &widths_mm,
                        repeat_header_rows,
                        printable,
                    )?;
                }
                "response_table" => {
                    check(
                        block.source_id.is_some()
                            && block.form_id.is_none()
                            && block.quote.is_none()
                            && block.blank_cells.is_empty()
                            && block.header_rows == 1
                            && !block.columns.is_empty()
                            && block.columns.len() <= 1000
                            && block.blank_rows > 0
                            && block.blank_rows <= 1000
                            && !plan.notices.is_empty(),
                        "invalid proposed response table",
                    )?;
                    check(
                        block.columns.iter().all(|t| !t.trim().is_empty()),
                        "empty response header",
                    )?;
                    let count = block.columns.len();
                    let rows = block.blank_rows + 1;
                    cells_used += rows * count;
                    check(cells_used <= 100000, "aggregate table capacity")?;
                    let cells = (0..rows)
                        .flat_map(|row| {
                            block.columns.iter().enumerate().map(move |(col, title)| {
                                (row, col, 1, 1, if row == 0 { title.as_str() } else { "" })
                            })
                        })
                        .collect::<Vec<_>>();
                    body += &table_xml(
                        rows,
                        count,
                        &cells,
                        &vec![printable / count as f64; count],
                        1,
                        printable,
                    )?;
                }
                // 用户已经写好的正文原样回去。文本不来自模型，也不来自招标原文，
                // 而来自绑 file_sha256 的回读收据；这里只按 key 取，不接受内联文本。
                "preserved_paragraphs" => {
                    check(
                        block.source_id.is_none()
                            && block.quote.is_none()
                            && block.form_id.is_none()
                            && block.columns.is_empty()
                            && block.blank_cells.is_empty()
                            && block.header_rows == 0
                            && block.blank_rows == 0,
                        "unexpected preserved paragraph fields",
                    )?;
                    let unit = preserved_unit(input, block, "paragraphs")?;
                    let paragraphs = unit["paragraphs"]
                        .as_array()
                        .filter(|items| !items.is_empty() && items.len() <= 10000)
                        .ok_or_else(|| invalid("preserved paragraphs missing"))?;
                    let mut written = false;
                    for item in paragraphs {
                        let text = item
                            .as_str()
                            .ok_or_else(|| invalid("preserved paragraph is not text"))?;
                        written |= !text.trim().is_empty();
                        body += &paragraph(text, None)?;
                    }
                    check(written, "preserved paragraphs are all blank")?;
                }
                "preserved_table" => {
                    check(
                        block.source_id.is_none()
                            && block.quote.is_none()
                            && block.form_id.is_none()
                            && block.columns.is_empty()
                            && block.blank_cells.is_empty()
                            && block.blank_ranges.is_empty()
                            && block.header_rows == 0
                            && block.blank_rows == 0,
                        "unexpected preserved table fields",
                    )?;
                    let unit = preserved_unit(input, block, "table")?;
                    let rows = unit["row_count"].as_u64().unwrap_or(0) as usize;
                    let columns = unit["column_count"].as_u64().unwrap_or(0) as usize;
                    check(
                        rows > 0 && rows <= 10000 && columns > 0 && columns <= 1000,
                        "preserved table geometry",
                    )?;
                    let raw = unit["cells"]
                        .as_array()
                        .ok_or_else(|| invalid("preserved table cells missing"))?;
                    let mut anchors = Vec::new();
                    for cell in raw {
                        let row = cell["row"].as_u64().unwrap_or(u64::MAX) as usize;
                        let column = cell["column"].as_u64().unwrap_or(u64::MAX) as usize;
                        let row_span = cell["row_span"].as_u64().unwrap_or(0) as usize;
                        let col_span = cell["col_span"].as_u64().unwrap_or(0) as usize;
                        let text = cell["text"]
                            .as_str()
                            .ok_or_else(|| invalid("preserved cell is not text"))?;
                        check(
                            row < rows
                                && column < columns
                                && row_span >= 1
                                && col_span >= 1
                                && row + row_span <= rows
                                && column + col_span <= columns,
                            "preserved cell outside its own grid",
                        )?;
                        anchors.push((row, column, row_span, col_span, text));
                    }
                    check(!anchors.is_empty(), "preserved table has no cell")?;
                    cells_used = cells_used
                        .checked_add(rows * columns)
                        .ok_or_else(|| invalid("table capacity"))?;
                    check(cells_used <= 100000, "aggregate table capacity")?;
                    body += &table_xml(
                        rows,
                        columns,
                        &anchors,
                        &vec![printable / columns as f64; columns],
                        0,
                        printable,
                    )?;
                }
                _ => return Err(invalid("unsupported template block")),
            }
            body += &bookmark(ordinal, Some(block_index), false, block_range_id);
            range_id = range_id
                .checked_add(1)
                .filter(|v| *v <= i32::MAX as u32)
                .ok_or_else(|| invalid("too many document ranges"))?;
        }
    }
    check(toc_written, "document requires body chapters")?;
    for excluded in &plan.excluded_sources {
        check(
            sources.contains_key(excluded.source_id.as_str())
                && !excluded.reason.trim().is_empty()
                && used_sources.insert(&excluded.source_id),
            "duplicate/used/foreign excluded source",
        )?;
    }
    for excluded in &plan.excluded_forms {
        check(
            forms.contains_key(excluded.form_id.as_str())
                && !excluded.reason.trim().is_empty()
                && used_forms.insert(&excluded.form_id),
            "duplicate/used/foreign excluded form",
        )?;
    }
    check(
        used_sources.len() == sources.len() && used_forms.len() == forms.len(),
        "source/form coverage incomplete",
    )?;
    body += &format!(
        "<w:sectPr><w:pgSz w:w=\"{}\" w:h=\"{}\" w:orient=\"{}\"/><w:pgMar w:top=\"{}\" w:right=\"{}\" w:bottom=\"{}\" w:left=\"{}\"/></w:sectPr>",
        twips(s.width_mm),
        twips(s.height_mm),
        if s.width_mm > s.height_mm {
            "landscape"
        } else {
            "portrait"
        },
        twips(s.top_mm),
        twips(s.right_mm),
        twips(s.bottom_mm),
        twips(s.left_mm)
    );
    let ns = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:document xmlns:w=\"{ns}\"><w:body>{body}</w:body></w:document>"
    );
    let font = text(&s.font_family)?;
    let mut styles = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:styles xmlns:w=\"{ns}\"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"{font}\" w:hAnsi=\"{font}\" w:eastAsia=\"{font}\" w:cs=\"{font}\"/><w:sz w:val=\"{}\"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:line=\"{}\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/></w:style><w:style w:type=\"paragraph\" w:styleId=\"Title\"><w:name w:val=\"Title\"/><w:rPr><w:b/></w:rPr></w:style>",
        (s.body_font_pt * 2.0).round(),
        (s.line_spacing * 240.0).round()
    );
    for depth in 0..9 {
        styles += &format!(
            "<w:style w:type=\"paragraph\" w:styleId=\"Heading{}\"><w:name w:val=\"heading {}\"/><w:basedOn w:val=\"Normal\"/><w:pPr><w:keepNext/><w:outlineLvl w:val=\"{depth}\"/></w:pPr><w:rPr><w:b/></w:rPr></w:style>",
            depth + 1,
            depth + 1
        );
    }
    styles += "</w:styles>";
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let parts=[("[Content_Types].xml",r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/></Types>"#.to_owned()),
        ("_rels/.rels",r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="document" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#.to_owned()),
        ("word/_rels/document.xml.rels",r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="styles" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="settings" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml"/></Relationships>"#.to_owned()),
        ("word/document.xml",document),("word/styles.xml",styles),("word/settings.xml",format!("<w:settings xmlns:w=\"{ns}\"><w:updateFields w:val=\"true\"/></w:settings>"))];
    for (name, value) in parts {
        zip.start_file(
            name,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )
        .map_err(|e| invalid(e.to_string()))?;
        zip.write_all(value.as_bytes())
            .map_err(|e| invalid(e.to_string()))?;
    }
    let bytes = zip
        .finish()
        .map_err(|e| invalid(e.to_string()))?
        .into_inner();
    crate::tender_upload::validate_docx_document(&bytes).map_err(|e| invalid(e.to_string()))?;
    Ok(bytes)
}

type Anchor<'a> = (usize, usize, usize, usize, &'a str);
fn table_xml(
    rows: usize,
    columns: usize,
    cells: &[Anchor<'_>],
    widths: &[f64],
    headers: usize,
    printable: f64,
) -> Result<String, TemplateError> {
    check(
        widths.len() == columns
            && widths.iter().all(|w| w.is_finite() && *w > 0.0)
            && widths.iter().sum::<f64>() <= printable + 0.02,
        "table exceeds page width",
    )?;
    let mut out = String::from(
        "<w:tbl><w:tblPr><w:tblLayout w:type=\"fixed\"/><w:tblBorders><w:top w:val=\"single\"/><w:left w:val=\"single\"/><w:bottom w:val=\"single\"/><w:right w:val=\"single\"/><w:insideH w:val=\"single\"/><w:insideV w:val=\"single\"/></w:tblBorders></w:tblPr><w:tblGrid>",
    );
    for width in widths {
        out += &format!("<w:gridCol w:w=\"{}\"/>", twips(*width));
    }
    out += "</w:tblGrid>";
    for row in 0..rows {
        out += "<w:tr><w:trPr>";
        if row < headers {
            out += "<w:tblHeader/>";
        }
        out += "</w:trPr>";
        let mut col = 0;
        while col < columns {
            let &(r, c, rs, cs, value) = cells
                .iter()
                .find(|(r, c, rs, cs, _)| row >= *r && row < r + rs && col >= *c && col < c + cs)
                .ok_or_else(|| invalid("uncovered cell"))?;
            check(
                c == col && (r >= headers || r + rs <= headers),
                "merge crosses header",
            )?;
            out += &format!(
                "<w:tc><w:tcPr><w:tcW w:w=\"{}\" w:type=\"dxa\"/>",
                twips(widths[c..c + cs].iter().sum())
            );
            if cs > 1 {
                out += &format!("<w:gridSpan w:val=\"{cs}\"/>");
            }
            if rs > 1 {
                out += if row == r {
                    "<w:vMerge w:val=\"restart\"/>"
                } else {
                    "<w:vMerge/>"
                };
            }
            out += "</w:tcPr>";
            out += &paragraph(if row == r { value } else { "" }, None)?;
            out += "</w:tc>";
            col += cs;
        }
        out += "</w:tr>";
    }
    out += "</w:tbl>";
    Ok(out)
}

/// Stable within one compiled plan, scoped by the immutable DOCX digest.
pub fn bookmark_name(section: usize, block: Option<usize>) -> String {
    match block {
        Some(block) => format!("kb_s{section}_b{block}"),
        None => format!("kb_s{section}"),
    }
}
fn bookmark(section: usize, block: Option<usize>, start: bool, id: u32) -> String {
    if start {
        format!(
            "<w:bookmarkStart w:id=\"{id}\" w:name=\"{}\"/>",
            bookmark_name(section, block)
        )
    } else {
        format!("<w:bookmarkEnd w:id=\"{id}\"/>")
    }
}
