//! Stable chapter identity and attachment-table mapping.
//!
//! A chapter id is the identity. Titles may be renamed without moving an
//! attachment table. Each attachment form binds to exactly one chapter through
//! a template `form_id` or a format-reference grid cell.
//!
//! 禁止硬编码: no document-specific strings, keyword lists, or sample
//! special-cases. Depth below is two documented constants, not a title list.

use crate::analysis::draft::{DraftPlanItem, DraftStatus};
use crate::analysis::{FrozenInput, Record, RecordData};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttachmentBinding {
    pub form_id: String,
    pub chapter_id: String,
}

/// Response leaves sit under a mid-level group, which sits under a root group.
/// Used when the tender has at least [`DEPTH_CHAIN_MIN`] attachment chains.
/// One chain may stay on a shallower tree. Documented in `docs/bidding/outline.md`.
pub const RESPONSE_LEAF_DEPTH: usize = 3;

/// Distinct attachment chains before [`RESPONSE_LEAF_DEPTH`] is required.
/// Documented in `docs/bidding/outline.md`.
pub const DEPTH_CHAIN_MIN: usize = 2;

/// Attachment tables in the frozen tender, in form id order.
pub fn attachment_form_ids(input: &FrozenInput) -> Vec<String> {
    let mut ids: Vec<_> = attachment_chains(input).into_iter().flatten().collect();
    ids.sort();
    ids.dedup();
    ids
}

/// One chain per run of continuation tables. A later table continues when it
/// repeats the header or its header row is empty, under the same column count,
/// and it stays in the same resolved heading with no prose between the grids.
/// A new heading, or any non-empty source line between the two tables, starts
/// another chain. Titles are not read.
pub fn attachment_chains(input: &FrozenInput) -> Vec<Vec<String>> {
    let index = SectionIndex::build(input);
    let mut placed = Vec::new();
    for form in &input.structured_forms {
        let Some(id) = form["form_definition_revision_id"].as_str() else {
            continue;
        };
        let source_id = form["source_unit_revision_id"].as_str().unwrap_or("");
        let source = input
            .source_units
            .iter()
            .find(|source| source.source_unit_revision_id == source_id);
        placed.push(PlacedForm {
            id,
            document_id: source
                .map(|source| source.document_id.as_str())
                .unwrap_or(""),
            ordinal: source.map(|source| source.ordinal).unwrap_or(0),
            heading: index
                .placed
                .get(source_id)
                .map(|(_, heading)| heading.as_str())
                .unwrap_or(""),
            columns: column_count(&form["definition"]),
            header: header_texts(&form["definition"]),
            own: is_attachment_form(form, &index),
            blocked: chain_blocked(&form["definition"]),
        });
    }
    placed.sort_by(|left, right| {
        (left.document_id, left.ordinal, left.id).cmp(&(right.document_id, right.ordinal, right.id))
    });
    let mut chains = Vec::new();
    let mut index = 0;
    while index < placed.len() {
        let mut end = index;
        while end + 1 < placed.len() && continues(input, &placed[end], &placed[end + 1]) {
            end += 1;
        }
        if placed[index..=end].iter().any(|form| form.own) {
            let chain: Vec<String> = placed[index..=end]
                .iter()
                .filter(|form| form.own || !form.blocked)
                .map(|form| form.id.to_string())
                .collect();
            if !chain.is_empty() {
                chains.push(chain);
            }
        }
        index = end + 1;
    }
    chains
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

struct PlacedForm<'a> {
    id: &'a str,
    document_id: &'a str,
    ordinal: usize,
    /// Heading the table hangs on, including a page table with an empty path.
    heading: &'a str,
    columns: usize,
    header: Vec<String>,
    own: bool,
    /// Numbered parameter grids and long requirement rows do not inherit
    /// attachment status from a neighboring continuation.
    blocked: bool,
}

fn continues(input: &FrozenInput, prev: &PlacedForm<'_>, next: &PlacedForm<'_>) -> bool {
    prev.document_id == next.document_id
        && prev.heading == next.heading
        && prev.columns >= 2
        && prev.columns == next.columns
        && prev.header.iter().any(|cell| !cell.is_empty())
        && (prev.header == next.header || next.header.iter().all(String::is_empty))
        && !prose_between(input, prev, next)
}

/// A non-empty source line between two grids is a new table, not a continuation.
fn prose_between(input: &FrozenInput, prev: &PlacedForm<'_>, next: &PlacedForm<'_>) -> bool {
    input.source_units.iter().any(|source| {
        source.document_id == prev.document_id
            && source.ordinal > prev.ordinal
            && source.ordinal < next.ordinal
            && !source.text.trim().is_empty()
    })
}

/// A table the bidder is meant to fill in.
///
/// The decision uses the grid and where the table sits in document order.
/// Titles and heading words are not read. A continuation that repeats the
/// preceding table's header, or that has an empty header under the same
/// column count, shares that table's attachment status.
fn is_attachment_form(form: &Value, index: &SectionIndex) -> bool {
    let Some(stats) = grid_stats(&form["definition"]) else {
        return false;
    };
    if fill_in_grid(&stats) || fill_in_columns(&form["definition"]) {
        return true;
    }
    caption_section_form(form, index, &stats)
}

/// Columns whose header cell has text and whose body is mostly empty.
/// Two such columns are a fill-in grid even when the rest of the table is
/// numbered. One such column still has to look like labels, not a parameter
/// grid or a requirement paragraph.
fn fill_in_columns(definition: &Value) -> bool {
    let Some(columns) = blank_header_columns(definition) else {
        return false;
    };
    if columns >= 2 {
        return true;
    }
    if columns == 0 {
        return false;
    }
    let Some(stats) = grid_stats(definition) else {
        return false;
    };
    if stats.body_max > SHORT_LABEL_CHARS {
        return false;
    }
    !(stats.body_substantive > 0 && stats.digit_body * 3 >= stats.body_substantive)
}

fn chain_blocked(definition: &Value) -> bool {
    let Some(stats) = grid_stats(definition) else {
        return false;
    };
    // `body_max` already ignores one lone long note and full-width footers.
    stats.body_max > SHORT_LABEL_CHARS
        || (stats.body_substantive > 0 && stats.digit_body * 3 >= stats.body_substantive)
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

/// One anchor covering every column. Used for a caption above the header and
/// for a footer under the body. The footer is not a value in each column.
fn full_width_rows(cells: &[Value], columns: usize) -> BTreeSet<usize> {
    if columns < 2 {
        return BTreeSet::new();
    }
    let mut per_row: BTreeMap<usize, (usize, usize)> = BTreeMap::new();
    for cell in cells {
        let row = cell["row"].as_u64().unwrap_or(0) as usize;
        let span = cell["col_span"].as_u64().unwrap_or(1).max(1) as usize;
        let entry = per_row.entry(row).or_insert((0, 0));
        entry.0 += 1;
        entry.1 = entry.1.max(span);
    }
    per_row
        .into_iter()
        .filter(|(_row, (anchors, span))| *anchors == 1 && *span >= columns)
        .map(|(row, _)| row)
        .collect()
}

fn grid_stats(definition: &Value) -> Option<GridStats> {
    let cells = definition.get("cells")?.as_array()?;
    if cells.is_empty() {
        return None;
    }
    let columns = column_count(definition);
    let wide = full_width_rows(cells, columns);
    let first = cells
        .iter()
        .filter_map(|cell| cell["row"].as_u64())
        .min()
        .unwrap_or(0) as usize;
    let mut by_row: BTreeMap<usize, Vec<CellKind>> = BTreeMap::new();
    let mut blank = 0;
    let mut inline = 0;
    let mut substantive = 0;
    let mut placed_cells: Vec<(usize, CellKind, usize, bool)> = Vec::new();
    let mut counted = 0;
    for cell in cells {
        let row = cell["row"].as_u64().unwrap_or(0) as usize;
        // A full-width footer is one sentence under the grid, not a body cell.
        if row != first && wide.contains(&row) {
            continue;
        }
        let text = cell["text"].as_str().unwrap_or("");
        let kind = classify_cell(text);
        by_row.entry(row).or_default().push(kind);
        let digits = text.chars().any(|ch| ch.is_ascii_digit());
        placed_cells.push((row, kind, text.chars().count(), digits));
        counted += 1;
        match kind {
            CellKind::Blank => blank += 1,
            CellKind::InlineBlank => inline += 1,
            CellKind::Substantive => substantive += 1,
        }
    }
    if counted == 0 {
        return None;
    }
    let mut body_total = 0;
    let mut body_fill = 0;
    let mut body_substantive = 0;
    let mut digit_body = 0;
    let mut header_substantive = 0;
    let mut body_lengths: Vec<usize> = Vec::new();
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
        body_lengths.push(chars);
        if digits {
            digit_body += 1;
        }
    }
    let header_body = header_substantive >= 1 && body_total >= 2 && body_fill * 2 >= body_total;
    Some(GridStats {
        total: counted,
        blank,
        inline,
        substantive,
        header_body,
        body_total,
        body_fill,
        body_max: blocking_body_max(&body_lengths),
        body_substantive,
        digit_body,
    })
}

/// Length used to reject requirement prose. One body cell past the short-label
/// bound is a note beside the row, not a column of requirements. Two or more
/// long cells still count.
fn blocking_body_max(body: &[usize]) -> usize {
    let long = body
        .iter()
        .filter(|chars| **chars > SHORT_LABEL_CHARS)
        .count();
    if long == 1 {
        return body
            .iter()
            .copied()
            .filter(|chars| *chars <= SHORT_LABEL_CHARS)
            .max()
            .unwrap_or(0);
    }
    body.iter().copied().max().unwrap_or(0)
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

fn column_count(definition: &Value) -> usize {
    definition["column_count"]
        .as_u64()
        .map(|count| count as usize)
        .filter(|count| *count > 0)
        .unwrap_or_else(|| {
            definition["cells"]
                .as_array()
                .map(|cells| {
                    cells
                        .iter()
                        .map(|cell| {
                            cell["column"].as_u64().unwrap_or(0) as usize
                                + cell["col_span"].as_u64().unwrap_or(1) as usize
                        })
                        .max()
                        .unwrap_or(0)
                })
                .unwrap_or(0)
        })
}

fn row_count(definition: &Value) -> usize {
    definition["row_count"]
        .as_u64()
        .map(|count| count as usize)
        .filter(|count| *count > 0)
        .unwrap_or_else(|| {
            definition["cells"]
                .as_array()
                .map(|cells| {
                    cells
                        .iter()
                        .map(|cell| cell["row"].as_u64().unwrap_or(0) as usize + 1)
                        .max()
                        .unwrap_or(0)
                })
                .unwrap_or(0)
        })
}

/// Header row under an optional spanning caption. A caption is one cell that
/// covers every column; the next row is then the header.
fn header_row(definition: &Value) -> Option<usize> {
    let cells = definition.get("cells")?.as_array()?;
    if cells.is_empty() {
        return None;
    }
    let columns = column_count(definition);
    let first = cells
        .iter()
        .filter_map(|cell| cell["row"].as_u64())
        .min()
        .unwrap_or(0) as usize;
    let spans_table = cells.iter().any(|cell| {
        cell["row"].as_u64().unwrap_or(0) as usize == first
            && cell["col_span"].as_u64().unwrap_or(1) as usize >= columns.max(1)
            && !cell["text"].as_str().unwrap_or("").trim().is_empty()
    });
    if spans_table {
        let next = cells
            .iter()
            .filter_map(|cell| cell["row"].as_u64())
            .map(|row| row as usize)
            .filter(|row| *row > first)
            .min();
        return next.or(Some(first));
    }
    Some(first)
}

fn header_texts(definition: &Value) -> Vec<String> {
    let columns = column_count(definition);
    if columns == 0 {
        return Vec::new();
    }
    let Some(header) = header_row(definition) else {
        return vec![String::new(); columns];
    };
    let mut texts = vec![String::new(); columns];
    let Some(cells) = definition["cells"].as_array() else {
        return texts;
    };
    for cell in cells {
        if cell["row"].as_u64().unwrap_or(0) as usize != header {
            continue;
        }
        let column = cell["column"].as_u64().unwrap_or(0) as usize;
        let text = cell["text"].as_str().unwrap_or("").trim().to_string();
        if column < texts.len() && texts[column].is_empty() {
            texts[column] = text;
        }
    }
    texts
}

fn blank_header_columns(definition: &Value) -> Option<usize> {
    let cells = definition.get("cells")?.as_array()?;
    if cells.is_empty() {
        return None;
    }
    let columns = column_count(definition);
    let rows = row_count(definition);
    let header = header_row(definition)?;
    if columns < 2 || rows < 2 {
        return None;
    }
    let wide = full_width_rows(cells, columns);
    let mut header_substantive = vec![false; columns];
    let mut body_blank = vec![0usize; columns];
    let mut body_substantive = vec![0usize; columns];
    let mut seen = vec![vec![false; columns]; rows];
    for cell in cells {
        let row = cell["row"].as_u64().unwrap_or(0) as usize;
        let column = cell["column"].as_u64().unwrap_or(0) as usize;
        let span = cell["col_span"].as_u64().unwrap_or(1).max(1) as usize;
        if row >= rows || column >= columns || (row != header && wide.contains(&row)) {
            continue;
        }
        let kind = classify_cell(cell["text"].as_str().unwrap_or(""));
        for offset in 0..span {
            let column = column + offset;
            if column >= columns || row >= seen.len() {
                break;
            }
            seen[row][column] = true;
            if row == header {
                if kind == CellKind::Substantive {
                    header_substantive[column] = true;
                }
            } else if row > header {
                if kind == CellKind::Substantive {
                    body_substantive[column] += 1;
                } else {
                    body_blank[column] += 1;
                }
            }
        }
    }
    for (row, row_seen) in seen.iter().enumerate().skip(header + 1) {
        if wide.contains(&row) {
            continue;
        }
        for (column, marked) in row_seen.iter().enumerate() {
            if !marked {
                body_blank[column] += 1;
            }
        }
    }
    Some(
        header_substantive
            .iter()
            .enumerate()
            .filter(|(column, substantive)| {
                let body = body_blank[*column] + body_substantive[*column];
                **substantive && body > 0 && body_blank[*column] > body_substantive[*column]
            })
            .count(),
    )
}

/// Structural context for the forms the model still has to bind.
///
/// Each card carries the stored caption, the header row, where the table
/// sits, and the continuation `chain` index. Forms that share a chain bind
/// to one leaf. The source index page does not have to reach that ordinal.
pub fn form_cards(input: &FrozenInput, form_ids: &[String]) -> Vec<Value> {
    let index = SectionIndex::build(input);
    let chains = attachment_chains(input);
    let mut cards = Vec::new();
    for id in form_ids {
        let Some(form) = input
            .structured_forms
            .iter()
            .find(|form| form["form_definition_revision_id"].as_str() == Some(id))
        else {
            continue;
        };
        let source_id = form["source_unit_revision_id"].as_str().unwrap_or("");
        let source = input
            .source_units
            .iter()
            .find(|source| source.source_unit_revision_id == source_id);
        let heading = index
            .placed
            .get(source_id)
            .map(|(_, heading)| heading.clone())
            .unwrap_or_default();
        let page = source.and_then(|source| source.locator["page_ordinal"].as_u64());
        let chain = chains
            .iter()
            .position(|chain| chain.iter().any(|form_id| form_id == id));
        cards.push(json!({
            "form_id": id,
            "title": form["definition"]["title"].as_str().unwrap_or(""),
            "header": header_texts(&form["definition"]),
            "source_id": source_id,
            "ordinal": source.map(|source| source.ordinal).unwrap_or(0),
            "page": page,
            "heading": heading,
            "chain": chain,
        }));
    }
    cards.sort_by(|left, right| {
        (
            left["ordinal"].as_u64().unwrap_or(0),
            left["form_id"].as_str().unwrap_or(""),
        )
            .cmp(&(
                right["ordinal"].as_u64().unwrap_or(0),
                right["form_id"].as_str().unwrap_or(""),
            ))
    });
    cards
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
            schema_version: 2,
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

    #[test]
    fn a_blank_column_under_the_header_is_a_form_below_half_fill() {
        let mut cells = Vec::new();
        for column in 0..4 {
            cells.push(["项目", "说明", "数量", "填写"][column]);
        }
        for _row in 1..6 {
            for column in 0..4 {
                cells.push(if column == 3 {
                    ""
                } else if column == 0 {
                    "名称"
                } else {
                    "全称"
                });
            }
        }
        let input = frozen(
            vec![source(
                "wide",
                40,
                "这一段说明很长，后面还有另一张表，不能靠章节里只有一张表来判断。",
                "商务文件",
                "document",
            )],
            vec![form("wide", "wide", grid(&cells, 4))],
        );
        assert_eq!(attachment_form_ids(&input), vec!["wide".to_string()]);
    }

    #[test]
    fn two_blank_columns_are_a_form_even_when_other_cells_are_numbered() {
        let mut cells = Vec::new();
        for column in 0..6 {
            cells.push(["序号", "名称", "参数", "单价", "总价", "备注"][column]);
        }
        for _row in 1..5 {
            for column in 0..6 {
                cells.push(match column {
                    0 => "1",
                    1 => "设备",
                    2 => "220V",
                    3 => "2",
                    _ => "",
                });
            }
        }
        let input = frozen(
            vec![source("price", 492, "", "报价", "page_table")],
            vec![form("price", "price", grid(&cells, 6))],
        );
        assert_eq!(attachment_form_ids(&input), vec!["price".to_string()]);
    }

    #[test]
    fn a_continuation_inherits_its_head_and_a_parameter_grid_does_not() {
        let head = grid(
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
                "电话",
                "号码",
                "",
            ],
            3,
        );
        let mut tail_cells = vec!["字段", "说明", "填写"];
        tail_cells.extend(["联系人", "姓名", "张三", "传真", "号码", "010"]);
        let tail = grid(&tail_cells, 3);
        let spec = grid(
            &[
                "序号", "项目", "参数", "响应", "1", "电压", "220V", "", "2", "功率", "5kW", "",
                "3", "重量", "30kg", "",
            ],
            4,
        );
        let mut spec_tail_cells = vec!["序号", "项目", "参数", "响应"];
        spec_tail_cells.extend(["", "", "", "", "", "", "", ""]);
        let spec_tail = grid(&spec_tail_cells, 4);
        let input = frozen(
            vec![
                source("head-src", 10, "", "格式", "page_table"),
                source("tail-src", 11, "", "", "page_table"),
                source("spec-src", 12, "", "技术规格", "page_table"),
                source("spec-tail-src", 13, "", "", "page_table"),
            ],
            vec![
                form("head", "head-src", head),
                form("tail", "tail-src", tail),
                form("spec", "spec-src", spec),
                form("spec-tail", "spec-tail-src", spec_tail),
            ],
        );
        let ids = attachment_form_ids(&input);
        assert!(ids.contains(&"head".to_string()), "{ids:?}");
        assert!(ids.contains(&"tail".to_string()), "{ids:?}");
        assert!(!ids.contains(&"spec".to_string()), "{ids:?}");
    }

    #[test]
    fn form_cards_carry_header_ordinal_and_heading() {
        let mut table = source("table", 449, "", "", "page_table");
        table.locator["page_ordinal"] = json!(12);
        let earlier = source("earlier", 448, "见下表", "投标文件格式 > 报价", "document");
        let definition = grid(&["名称", "单价", "总价", "", "", ""], 3);
        let input = frozen(
            vec![earlier, table],
            vec![form("late", "table", definition)],
        );
        let cards = form_cards(&input, &["late".to_string()]);
        assert_eq!(cards[0]["form_id"], "late");
        assert_eq!(cards[0]["header"], json!(["名称", "单价", "总价"]));
        assert_eq!(cards[0]["ordinal"], 449);
        assert_eq!(cards[0]["page"], 12);
        assert_eq!(cards[0]["heading"], "投标文件格式 > 报价");
        assert_eq!(cards[0]["source_id"], "table");
        assert_eq!(cards[0]["chain"], 0);
    }

    #[test]
    fn a_continuation_stops_at_a_new_heading_or_at_prose() {
        let cells = grid(&["名称", "内容", "", ""], 2);
        let same = frozen(
            vec![
                source("left", 0, "", "格式", "page_table"),
                source("right", 1, "", "", "page_table"),
            ],
            vec![
                form("left", "left", cells.clone()),
                form("right", "right", cells.clone()),
            ],
        );
        assert_eq!(
            attachment_chains(&same),
            vec![vec!["left".to_string(), "right".to_string()]]
        );
        let headed = frozen(
            vec![
                source("left", 0, "", "格式 > 甲", "page_table"),
                source("right", 1, "", "格式 > 乙", "page_table"),
            ],
            vec![
                form("left", "left", cells.clone()),
                form("right", "right", cells.clone()),
            ],
        );
        assert_eq!(attachment_chains(&headed).len(), 2);
        let prose = frozen(
            vec![
                source("left", 0, "", "格式", "page_table"),
                source("note", 1, "另起一表", "格式", "document"),
                source("right", 2, "", "格式", "page_table"),
            ],
            vec![
                form("left", "left", cells.clone()),
                form("right", "right", cells),
            ],
        );
        assert_eq!(attachment_chains(&prose).len(), 2);
    }

    fn spanned(row: usize, column: usize, col_span: usize, text: &str) -> Value {
        json!({
            "row": row,
            "column": column,
            "row_span": 1,
            "col_span": col_span,
            "text": text
        })
    }

    #[test]
    fn a_full_width_footer_does_not_hide_blank_columns() {
        let mut cells = Vec::new();
        for (column, text) in ["序号", "名称", "参数", "品牌", "单价", "总价"]
            .iter()
            .enumerate()
        {
            cells.push(spanned(0, column, 1, text));
        }
        for (column, text) in ["1", "设备", "规格", "甲", "", ""].iter().enumerate() {
            cells.push(spanned(1, column, 1, text));
        }
        cells.push(spanned(
            2,
            0,
            6,
            "表尾横跨各列的一句说明，不应当把空白列涂成已填写。",
        ));
        let input = frozen(
            vec![source("price", 40, &long_prose(), "报价", "document")],
            vec![form("price", "price", table_definition(6, 3, cells))],
        );
        assert_eq!(attachment_form_ids(&input), vec!["price".to_string()]);
    }

    #[test]
    fn a_full_width_footer_does_not_tip_the_body_fill() {
        let mut cells = Vec::new();
        for (column, text) in ["名称", "规格", "", ""].iter().enumerate() {
            cells.push(spanned(0, column, 1, text));
        }
        for (column, text) in ["设备", "参数", "", ""].iter().enumerate() {
            cells.push(spanned(1, column, 1, text));
        }
        cells.push(spanned(
            2,
            0,
            4,
            "表尾横跨各列的一句说明，多算一个已填格子就会差一格。",
        ));
        let input = frozen(
            vec![source("fill", 41, &long_prose(), "报价", "document")],
            vec![form("fill", "fill", table_definition(4, 3, cells))],
        );
        assert_eq!(attachment_form_ids(&input), vec!["fill".to_string()]);
    }

    #[test]
    fn a_filled_grid_with_a_full_width_footer_stays_out() {
        let mut cells = Vec::new();
        for (column, text) in ["项目", "要求", "参数", "响应"].iter().enumerate() {
            cells.push(spanned(0, column, 1, text));
        }
        cells.push(spanned(1, 0, 1, "电压"));
        cells.push(spanned(1, 1, 1, "额定"));
        cells.push(spanned(1, 2, 1, "220V"));
        cells.push(spanned(1, 3, 1, "满足"));
        cells.push(spanned(2, 0, 4, "表尾横跨各列的一句说明，正文都已经写满。"));
        let input = frozen(
            vec![source("spec", 3, "", "技术规格", "page_table")],
            vec![form(
                "spec",
                "spec",
                json!({
                    "title": "source_unit:form",
                    "row_count": 3,
                    "column_count": 4,
                    "cells": cells
                }),
            )],
        );
        assert!(attachment_form_ids(&input).is_empty());
    }

    #[test]
    fn one_long_note_does_not_block_a_blank_column_or_a_continuation() {
        let header = ["字段", "说明", "填写", "注记"];
        let mut head_cells = Vec::new();
        for (column, text) in header.iter().enumerate() {
            head_cells.push(spanned(0, column, 1, text));
        }
        head_cells.push(spanned(1, 0, 1, "名称"));
        head_cells.push(spanned(1, 1, 1, "全称"));
        head_cells.push(spanned(1, 2, 1, ""));
        head_cells.push(spanned(1, 3, 1, ""));
        head_cells.push(spanned(2, 0, 1, "地址"));
        head_cells.push(spanned(2, 1, 1, "注册"));
        head_cells.push(spanned(2, 2, 1, ""));
        head_cells.push(spanned(2, 3, 1, ""));
        let note = "这一格单独写了一句比较长的说明文字";
        let mut note_cells = Vec::new();
        for (column, text) in header.iter().enumerate() {
            note_cells.push(spanned(0, column, 1, text));
        }
        note_cells.push(spanned(1, 0, 1, "甲"));
        note_cells.push(spanned(1, 1, 1, "乙"));
        note_cells.push(spanned(1, 2, 1, "丙"));
        note_cells.push(spanned(1, 3, 1, note));
        let mut blank_note = note_cells.clone();
        blank_note[6]["text"] = json!("");
        let mut numbered = Vec::new();
        for (column, text) in header.iter().enumerate() {
            numbered.push(spanned(0, column, 1, text));
        }
        numbered.push(spanned(1, 0, 1, "1"));
        numbered.push(spanned(1, 1, 1, "220V"));
        numbered.push(spanned(1, 2, 1, "5kW"));
        numbered.push(spanned(1, 3, 1, note));
        let chain = frozen(
            vec![
                source("head-src", 10, "", "格式", "page_table"),
                source("note-src", 11, "", "", "page_table"),
                source("num-src", 12, "", "技术规格", "page_table"),
            ],
            vec![
                form("head", "head-src", table_definition(4, 3, head_cells)),
                form("note", "note-src", table_definition(4, 2, note_cells)),
                form("numbered", "num-src", table_definition(4, 2, numbered)),
            ],
        );
        let ids = attachment_form_ids(&chain);
        assert!(ids.contains(&"head".to_string()), "{ids:?}");
        assert!(ids.contains(&"note".to_string()), "{ids:?}");
        assert!(!ids.contains(&"numbered".to_string()), "{ids:?}");
        let alone = frozen(
            vec![source("alone", 20, &long_prose(), "商务", "document")],
            vec![form("alone", "alone", table_definition(4, 2, blank_note))],
        );
        assert_eq!(attachment_form_ids(&alone), vec!["alone".to_string()]);
    }

    fn long_prose() -> String {
        "这一段说明写得很长，用来挡住只靠标题下只有一张表才成立的那条规则。".repeat(3)
    }

    fn table_definition(columns: usize, rows: usize, cells: Vec<Value>) -> Value {
        json!({
            "title": "source_unit:form",
            "row_count": rows,
            "column_count": columns,
            "cells": cells
        })
    }
}
