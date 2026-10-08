//! Stable chapter identity and attachment-table mapping.
//!
//! A chapter id is the identity. Titles may be renamed without moving an
//! attachment table. Each attachment form binds to exactly one chapter through
//! a template `form_id` or a format-reference grid cell.

use crate::analysis::draft::{DraftPlanItem, DraftStatus};
use crate::analysis::{FrozenInput, Record, RecordData};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttachmentBinding {
    pub form_id: String,
    pub chapter_id: String,
}

/// Attachment tables in the frozen tender, in form id order.
pub fn attachment_form_ids(input: &FrozenInput) -> Vec<String> {
    let index = SectionIndex::build(input);
    let mut ids: Vec<_> = input
        .structured_forms
        .iter()
        .filter(|form| is_attachment_form(form, &index))
        .filter_map(|form| form["form_definition_revision_id"].as_str())
        .map(str::to_string)
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

/// Bind every attachment table to the chapter that carries its form id.
pub fn map_attachment_tables(
    input: &FrozenInput,
    plan: &[DraftPlanItem],
    records: &BTreeMap<String, Record>,
) -> Result<Vec<AttachmentBinding>, String> {
    let mut bindings: BTreeMap<String, String> = BTreeMap::new();
    for item in plan
        .iter()
        .filter(|item| item.status != DraftStatus::Omitted)
    {
        for form_id in chapter_form_ids(item, records) {
            if !attachment_form_ids(input).contains(&form_id) {
                continue;
            }
            if let Some(existing) = bindings.insert(form_id.clone(), item.id.clone())
                && existing != item.id
            {
                return Err(format!(
                    "attachment table {form_id} is mapped to both {existing} and {}",
                    item.id
                ));
            }
        }
    }
    Ok(bindings
        .into_iter()
        .map(|(form_id, chapter_id)| AttachmentBinding {
            form_id,
            chapter_id,
        })
        .collect())
}

pub fn unmapped_attachment_forms(
    input: &FrozenInput,
    plan: &[DraftPlanItem],
    records: &BTreeMap<String, Record>,
) -> Vec<String> {
    let Ok(bindings) = map_attachment_tables(input, plan, records) else {
        return attachment_form_ids(input);
    };
    let mapped: BTreeSet<_> = bindings
        .iter()
        .map(|binding| binding.form_id.as_str())
        .collect();
    attachment_form_ids(input)
        .into_iter()
        .filter(|id| !mapped.contains(id.as_str()))
        .collect()
}

fn chapter_form_ids(item: &DraftPlanItem, records: &BTreeMap<String, Record>) -> Vec<String> {
    let mut ids: Vec<_> = item
        .format_refs
        .iter()
        .filter_map(|span| span.grid_cell.as_ref().map(|cell| cell.form_id.clone()))
        .collect();
    if let Some(template_id) = item.template_id.as_deref()
        && let Some(record) = records.get(template_id)
        && let RecordData::Template { regions, .. } = &record.data
    {
        ids.extend(regions.iter().filter_map(|region| region.form_id.clone()));
    }
    ids.sort();
    ids.dedup();
    ids
}

/// A caption-sized section is one short line of prose above its only table.
const CAPTION_PROSE_CHARS: usize = 80;
/// Labels on a fill-in form are short. Requirement sentences are not.
const SHORT_LABEL_CHARS: usize = 12;

#[derive(Default)]
struct SectionShape {
    prose_chars: usize,
    forms: usize,
}

struct SectionIndex {
    placed: BTreeMap<String, (String, String)>,
    sections: BTreeMap<(String, String), SectionShape>,
}

impl SectionIndex {
    /// Headings follow document order. A table locator without `heading_path`
    /// hangs on the nearest earlier heading in the same document, the same way
    /// the document tree hangs a page table on the current section.
    fn build(input: &FrozenInput) -> Self {
        let mut sources: Vec<_> = input.source_units.iter().collect();
        sources.sort_by(|left, right| {
            (&left.document_id, left.ordinal).cmp(&(&right.document_id, right.ordinal))
        });
        let mut current: BTreeMap<String, String> = BTreeMap::new();
        let mut placed = BTreeMap::new();
        let mut sections: BTreeMap<(String, String), SectionShape> = BTreeMap::new();
        for source in sources {
            let own = source.locator["heading_path"].as_str().unwrap_or("").trim();
            if !own.is_empty() {
                current.insert(source.document_id.clone(), own.to_string());
            }
            let heading = if !own.is_empty() {
                own.to_string()
            } else {
                current
                    .get(&source.document_id)
                    .cloned()
                    .unwrap_or_default()
            };
            let key = (source.document_id.clone(), heading);
            let added = source.text.chars().count();
            let shape = sections.entry(key.clone()).or_default();
            shape.prose_chars = shape.prose_chars.saturating_add(added);
            placed.insert(source.source_unit_revision_id.clone(), key);
        }
        for form in &input.structured_forms {
            let Some(source_id) = form["source_unit_revision_id"].as_str() else {
                continue;
            };
            let Some(key) = placed.get(source_id) else {
                continue;
            };
            if let Some(shape) = sections.get_mut(key) {
                shape.forms = shape.forms.saturating_add(1);
            }
        }
        Self { placed, sections }
    }

    fn shape(&self, source_id: &str) -> Option<&SectionShape> {
        self.sections.get(self.placed.get(source_id)?)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CellKind {
    Blank,
    InlineBlank,
    Substantive,
}

struct GridStats {
    total: usize,
    blank: usize,
    inline: usize,
    substantive: usize,
    /// First row has a label, and at least half of the later anchors are empty.
    header_body: bool,
    /// Later anchors only. Zero when the grid is a single row.
    body_total: usize,
    body_fill: usize,
    /// Longest substantive label below the first row.
    body_max: usize,
    body_substantive: usize,
    digit_body: usize,
}

/// A table the bidder is meant to fill in.
///
/// The decision uses the grid and where the table sits in document order.
/// Titles and heading words are not read.
fn is_attachment_form(form: &Value, index: &SectionIndex) -> bool {
    let Some(stats) = grid_stats(&form["definition"]) else {
        return false;
    };
    if fill_in_grid(&stats) {
        return true;
    }
    caption_section_form(form, index, &stats)
}

fn fill_in_grid(stats: &GridStats) -> bool {
    if stats.total < 2 || stats.substantive == 0 {
        return false;
    }
    let fill = stats.blank + stats.inline;
    stats.inline >= 2 || (fill >= 2 && fill * 2 >= stats.total) || stats.header_body
}

/// The only table under a one-line heading, with a blank column and short labels.
/// Filled specification grids and numbered parameter grids stay out.
fn caption_section_form(form: &Value, index: &SectionIndex, stats: &GridStats) -> bool {
    if stats.body_total < 2 || stats.body_fill * 3 < stats.body_total {
        return false;
    }
    if stats.body_max > SHORT_LABEL_CHARS {
        return false;
    }
    if stats.body_substantive > 0 && stats.digit_body * 3 >= stats.body_substantive {
        return false;
    }
    let Some(source_id) = form["source_unit_revision_id"].as_str() else {
        return false;
    };
    let Some((_, heading)) = index.placed.get(source_id) else {
        return false;
    };
    if heading.is_empty() {
        return false;
    }
    let Some(shape) = index.shape(source_id) else {
        return false;
    };
    shape.forms == 1 && shape.prose_chars <= CAPTION_PROSE_CHARS
}

fn grid_stats(definition: &Value) -> Option<GridStats> {
    let cells = definition.get("cells")?.as_array()?;
    if cells.is_empty() {
        return None;
    }
    let mut by_row: BTreeMap<usize, Vec<CellKind>> = BTreeMap::new();
    let mut blank = 0;
    let mut inline = 0;
    let mut substantive = 0;
    let mut placed_cells: Vec<(usize, CellKind, usize, bool)> = Vec::new();
    for cell in cells {
        let text = cell["text"].as_str().unwrap_or("");
        let kind = classify_cell(text);
        let row = cell["row"].as_u64().unwrap_or(0) as usize;
        by_row.entry(row).or_default().push(kind);
        let digits = text.chars().any(|ch| ch.is_ascii_digit());
        placed_cells.push((row, kind, text.chars().count(), digits));
        match kind {
            CellKind::Blank => blank += 1,
            CellKind::InlineBlank => inline += 1,
            CellKind::Substantive => substantive += 1,
        }
    }
    let total = cells.len();
    let first = *by_row.keys().next()?;
    let mut body_total = 0;
    let mut body_fill = 0;
    let mut body_max = 0;
    let mut body_substantive = 0;
    let mut digit_body = 0;
    let mut header_substantive = 0;
    for (row, kinds) in &by_row {
        if *row == first {
            header_substantive = kinds
                .iter()
                .filter(|kind| **kind == CellKind::Substantive)
                .count();
            continue;
        }
        body_total += kinds.len();
        body_fill += kinds
            .iter()
            .filter(|kind| **kind != CellKind::Substantive)
            .count();
    }
    for (row, kind, chars, digits) in placed_cells {
        if row == first || kind != CellKind::Substantive {
            continue;
        }
        body_substantive += 1;
        body_max = body_max.max(chars);
        if digits {
            digit_body += 1;
        }
    }
    let header_body = header_substantive >= 1 && body_total >= 2 && body_fill * 2 >= body_total;
    Some(GridStats {
        total,
        blank,
        inline,
        substantive,
        header_body,
        body_total,
        body_fill,
        body_max,
        body_substantive,
        digit_body,
    })
}

fn classify_cell(text: &str) -> CellKind {
    if has_inline_blank(text) {
        return CellKind::InlineBlank;
    }
    if is_blank_cell(text) {
        CellKind::Blank
    } else {
        CellKind::Substantive
    }
}

fn is_placeholder(ch: char) -> bool {
    matches!(
        ch,
        '_' | '＿' | '.' | '．' | '·' | '…' | '-' | '—' | '–' | '~' | '～' | '□' | '☐'
    )
}

fn has_inline_blank(text: &str) -> bool {
    let mut run = 0;
    for ch in text.chars() {
        if is_placeholder(ch) {
            run += 1;
            if run >= 3 {
                return true;
            }
        } else if !ch.is_whitespace() {
            run = 0;
        }
    }
    false
}

fn is_blank_cell(text: &str) -> bool {
    let kept: String = text
        .chars()
        .filter(|ch| !ch.is_whitespace() && !is_placeholder(*ch))
        .collect();
    if kept.is_empty() {
        return true;
    }
    let tokens: Vec<&str> = text
        .split_whitespace()
        .filter(|token| token.chars().any(|ch| !is_placeholder(ch)))
        .collect();
    // Spaced single-character stubs ("年 月 日") are blanks. A lone token
    // such as a row number stays substantive.
    tokens.len() >= 2
        && tokens
            .iter()
            .all(|token| token.chars().filter(|ch| !is_placeholder(*ch)).count() <= 1)
        && kept.chars().count() <= 4
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::{FrozenInput, Source};
    use serde_json::json;

    fn source(id: &str, ordinal: usize, text: &str, heading: &str, locator_kind: &str) -> Source {
        let locator = if locator_kind == "page_table" {
            json!({
                "locator_kind": "page_table",
                "page_ordinal": 0,
                "table_ordinal": ordinal,
                "heading_path": heading
            })
        } else {
            json!({"locator_kind": "document", "heading_path": heading})
        };
        Source {
            source_unit_revision_id: id.into(),
            document_id: "doc".into(),
            text: text.into(),
            locator,
            ordinal,
        }
    }

    fn grid(cells: &[&str], columns: usize) -> Value {
        let rows = cells.len().div_ceil(columns);
        let mut anchors = Vec::new();
        for (index, text) in cells.iter().enumerate() {
            anchors.push(json!({
                "row": index / columns,
                "column": index % columns,
                "row_span": 1,
                "col_span": 1,
                "text": text
            }));
        }
        json!({
            "title": "source_unit:form",
            "row_count": rows,
            "column_count": columns,
            "cells": anchors
        })
    }

    fn frozen(units: Vec<Source>, forms: Vec<Value>) -> FrozenInput {
        FrozenInput {
            schema_version: 1,
            project_id: "project".into(),
            document_set_id: "set".into(),
            documents: vec![],
            document_relations: vec![],
            source_units: units,
            structured_forms: forms,
            decisions: vec![],
        }
    }

    fn form(id: &str, source_id: &str, definition: Value) -> Value {
        json!({
            "form_definition_revision_id": id,
            "source_unit_revision_id": source_id,
            "definition": definition
        })
    }

    #[test]
    fn fill_in_grids_are_attachment_forms_without_a_title_keyword() {
        let label_value = grid(&["名称", "", "地址", "", "日期", ""], 2);
        let price = grid(
            &[
                "序号", "名称", "数量", "单价", "合价", "", "", "", "", "", "", "", "", "", "",
            ],
            5,
        );
        let underscores = grid(&["名称：________", "地址：________", "签章"], 1);
        let date_stubs = grid(&["名称", "年 月 日", "地址", "年 月 日"], 2);
        let input = frozen(
            vec![
                source("labels", 0, "", "", "page_table"),
                source("price", 1, "", "", "page_table"),
                source("blanks", 2, "", "", "page_table"),
                source("dates", 3, "", "", "page_table"),
            ],
            vec![
                form("labels", "labels", label_value),
                form("price", "price", price),
                form("blanks", "blanks", underscores),
                form("dates", "dates", date_stubs),
            ],
        );
        assert_eq!(
            attachment_form_ids(&input),
            vec![
                "blanks".to_string(),
                "dates".to_string(),
                "labels".to_string(),
                "price".to_string()
            ]
        );
    }

    #[test]
    fn a_page_table_inherits_its_section_and_a_filled_spec_does_not_bind() {
        let prose = "按本表填写。";
        let form_grid = grid(
            &[
                "字段",
                "说明",
                "填写",
                "名称",
                "全称",
                "",
                "地址",
                "注册地",
                "",
            ],
            3,
        );
        let spec = grid(
            &[
                "序号", "项目", "参数", "响应", "1", "电压", "220V", "", "2", "功率", "5kW", "",
                "3", "重量", "30kg", "",
            ],
            4,
        );
        let input = frozen(
            vec![
                source("format", 0, prose, "格式章 > 填写表", "document"),
                source("table", 1, "", "", "page_table"),
                source(
                    "spec-text",
                    2,
                    "下列参数为采购需求，投标人按参数响应，不另成表。",
                    "技术规格",
                    "document",
                ),
                source("spec", 3, "", "", "page_table"),
            ],
            vec![form("fill", "table", form_grid), form("spec", "spec", spec)],
        );
        assert_eq!(attachment_form_ids(&input), vec!["fill".to_string()]);
    }

    #[test]
    fn spec_title_and_heading_do_not_promote_a_dense_grid() {
        let mut dense = grid(
            &[
                "项目",
                "招标要求",
                "电压",
                "额定电压应满足现场条件并提供检测报告",
                "功率",
                "额定功率不低于标书要求并附证明",
            ],
            2,
        );
        dense["title"] = json!("附件一 技术参数表");
        let input = frozen(
            vec![source(
                "dense",
                0,
                "技术要求正文很长，表格只是规格，不是待填表。",
                "附件 技术规格",
                "document",
            )],
            vec![form("dense", "dense", dense)],
        );
        assert!(attachment_form_ids(&input).is_empty());
    }

    #[test]
    fn long_requirement_text_under_a_short_heading_is_not_a_form() {
        let input = frozen(
            vec![
                source("intro", 0, "见下表。", "技术要求 > 电压", "document"),
                source("table", 1, "", "", "page_table"),
            ],
            vec![form(
                "spec",
                "table",
                grid(
                    &[
                        "项目",
                        "要求",
                        "响应",
                        "电压",
                        "额定电压应满足现场条件并提供检测报告",
                        "",
                        "功率",
                        "额定功率不低于标书要求并附证明材料",
                        "",
                    ],
                    3,
                ),
            )],
        );
        assert!(attachment_form_ids(&input).is_empty());
    }
}
