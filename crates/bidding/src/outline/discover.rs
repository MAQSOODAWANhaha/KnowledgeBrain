//! Reading packs follow the structured document.
//!
//! Adjacent sections merge while they fit in the byte budget. A section
//! that does not fit splits at its own paragraphs, clauses, and table rows.
//! Table bytes are the UTF-8 length of the cell text a pack sends, including
//! a repeated header. A continuation table repeats its header as context and
//! does not scan those cells again. A byte cut happens only inside one clause
//! that is still larger than the budget, and it stays on a UTF-8 character
//! boundary.

use crate::analysis::{FrozenInput, Source};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const DEFAULT_PACK_CONCURRENCY: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextSpan {
    pub source_id: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormSpan {
    pub form_id: String,
    pub start: usize,
    pub end: usize,
    /// Header cells repeated on a continuation slice. They are not inside
    /// `[start, end)` for that slice. Zero when this slice includes the header.
    #[serde(default)]
    pub header_cells: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParsePack {
    pub id: String,
    pub document_id: String,
    pub order: usize,
    /// Parent heading. Empty when the section has no parent.
    pub context_heading: String,
    pub heading: String,
    pub text: Vec<TextSpan>,
    pub forms: Vec<FormSpan>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackStatus {
    Pending,
    Running,
    Failed,
    Committed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldError {
    pub path: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackFeedback {
    pub pack_id: String,
    pub call_id: String,
    pub arguments_sha256: String,
    pub errors: Vec<FieldError>,
    pub total: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChapterBlock {
    document_id: String,
    section_key: String,
    heading: String,
    context_heading: String,
    text: Vec<TextSpan>,
    source_texts: Vec<String>,
    forms: Vec<FormSpan>,
    form_columns: Vec<usize>,
    /// Per-cell UTF-8 lengths, row-major, parallel to `forms`.
    form_cell_bytes: Vec<Vec<usize>>,
    bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PackRecord {
    pack: ParsePack,
    status: PackStatus,
    attempt: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    feedback: Option<PackFeedback>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoverWork {
    pub plan_sha256: String,
    packs: BTreeMap<String, PackRecord>,
    requirements: BTreeMap<String, RequirementRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequirementRecord {
    pub description: String,
    pub source_id: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackCounts {
    pub total: usize,
    pub pending: usize,
    pub running: usize,
    pub failed: usize,
    pub committed: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PackRequirement {
    pub description: String,
    pub source_id: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PackSubmit {
    pub call_id: String,
    pub requirements: Vec<PackRequirement>,
}

impl DiscoverWork {
    pub fn plan(input: &FrozenInput, soft_max_bytes: usize) -> Self {
        let packs = plan_packs(input, soft_max_bytes);
        let plan_sha256 = plan_sha(&packs);
        let packs = packs
            .into_iter()
            .map(|pack| {
                (
                    pack.id.clone(),
                    PackRecord {
                        pack,
                        status: PackStatus::Pending,
                        attempt: 0,
                        feedback: None,
                    },
                )
            })
            .collect();
        Self {
            plan_sha256,
            packs,
            requirements: BTreeMap::new(),
        }
    }

    /// Mark up to `limit` pending packs running. Packs already running stay running.
    pub fn claim(&mut self, limit: usize) -> Vec<ParsePack> {
        let mut claimed = Vec::new();
        for record in self.packs.values_mut() {
            if claimed.len() >= limit {
                break;
            }
            if record.status == PackStatus::Pending {
                record.status = PackStatus::Running;
                record.attempt = record.attempt.saturating_add(1);
                claimed.push(record.pack.clone());
            }
        }
        claimed
    }

    pub fn submit(&mut self, pack_id: &str, submit: PackSubmit) -> Result<(), PackFeedback> {
        let errors = self.validation_errors(pack_id, &submit);
        if !errors.is_empty() {
            let feedback = feedback(pack_id, &submit, errors);
            if let Some(record) = self.packs.get_mut(pack_id) {
                record.status = PackStatus::Failed;
                record.feedback = Some(feedback.clone());
            }
            return Err(feedback);
        }
        let record = self
            .packs
            .get_mut(pack_id)
            .expect("validation found the pack");
        record.status = PackStatus::Committed;
        record.feedback = None;
        for (index, requirement) in submit.requirements.iter().enumerate() {
            self.requirements.insert(
                format!("{pack_id}:{index}"),
                RequirementRecord {
                    description: requirement.description.clone(),
                    source_id: requirement.source_id.clone(),
                    start: requirement.start,
                    end: requirement.end,
                },
            );
        }
        Ok(())
    }

    pub fn session(&self, input: &FrozenInput, pack_id: &str) -> Result<serde_json::Value, String> {
        let record = self
            .packs
            .get(pack_id)
            .ok_or_else(|| format!("unknown reading pack {pack_id}"))?;
        Ok(serde_json::json!({
            "duty": "discover",
            "pack": materialize_pack(input, &record.pack)?,
            "status": record.status,
            "feedback": record.feedback,
        }))
    }

    /// Sessions for packs this discover turn still has to handle.
    pub fn inflight_sessions(&self, input: &FrozenInput) -> Vec<serde_json::Value> {
        self.packs
            .values()
            .filter(|record| matches!(record.status, PackStatus::Running | PackStatus::Failed))
            .map(|record| {
                self.session(input, &record.pack.id)
                    .expect("in-flight pack has a session")
            })
            .collect()
    }

    pub fn status(&self, pack_id: &str) -> Option<PackStatus> {
        self.packs.get(pack_id).map(|record| record.status)
    }

    pub fn requirement_count(&self) -> usize {
        self.requirements.len()
    }

    pub fn requirement(&self, id: &str) -> Option<&RequirementRecord> {
        self.requirements.get(id)
    }

    pub fn requirement_ids(&self) -> BTreeSet<String> {
        self.requirements.keys().cloned().collect()
    }

    pub fn requirement_packet(&self) -> Vec<serde_json::Value> {
        self.requirements
            .iter()
            .map(|(id, record)| {
                serde_json::json!({
                    "id": id,
                    "description": record.description,
                    "source_id": record.source_id,
                    "start": record.start,
                    "end": record.end,
                })
            })
            .collect()
    }

    pub fn pack_counts(&self) -> PackCounts {
        let mut counts = PackCounts {
            total: self.packs.len(),
            pending: 0,
            running: 0,
            failed: 0,
            committed: 0,
        };
        for record in self.packs.values() {
            match record.status {
                PackStatus::Pending => counts.pending += 1,
                PackStatus::Running => counts.running += 1,
                PackStatus::Failed => counts.failed += 1,
                PackStatus::Committed => counts.committed += 1,
            }
        }
        counts
    }

    /// A discovery turn can leave the window only when every pack it names is committed.
    pub fn turn_only_committed(&self, pack_ids: &[String]) -> bool {
        !pack_ids.is_empty()
            && pack_ids
                .iter()
                .all(|id| self.status(id) == Some(PackStatus::Committed))
    }

    /// True when every planned pack has been committed. An empty plan is done.
    pub fn complete(&self) -> bool {
        self.packs
            .values()
            .all(|record| record.status == PackStatus::Committed)
    }

    fn validation_errors(&self, pack_id: &str, submit: &PackSubmit) -> Vec<FieldError> {
        let Some(record) = self.packs.get(pack_id) else {
            return vec![field(
                "",
                "unknown_pack",
                "reading pack is not in the frozen plan",
            )];
        };
        if !matches!(record.status, PackStatus::Running | PackStatus::Failed) {
            return vec![field(
                "",
                "pack_not_running",
                "submit a pack that was claimed for this turn",
            )];
        }
        let mut errors = Vec::new();
        for (index, requirement) in submit.requirements.iter().enumerate() {
            let path = format!("/requirements/{index}");
            let Some(span) = record
                .pack
                .text
                .iter()
                .find(|span| span.source_id == requirement.source_id)
            else {
                errors.push(field(
                    &path,
                    "outside_pack",
                    "requirement cites a source outside this reading pack",
                ));
                continue;
            };
            if requirement.start >= requirement.end
                || requirement.start < span.start
                || requirement.end > span.end
            {
                errors.push(field(
                    &path,
                    "outside_slice",
                    "requirement must cite text inside the delivered heading slice",
                ));
            }
        }
        errors
    }
}

pub fn reading_budget(pack_max_chars: usize) -> usize {
    if pack_max_chars == 0 {
        8_000
    } else {
        pack_max_chars
    }
}

/// Plan once, claim up to `limit` pending packs, and return every pack this turn
/// still has to read: the ones just claimed, plus packs already `running` or
/// `failed` and waiting for repair.
pub fn claim_turn(
    slot: &mut Option<DiscoverWork>,
    input: &FrozenInput,
    budget: usize,
    limit: usize,
) -> Vec<serde_json::Value> {
    let work = slot.get_or_insert_with(|| DiscoverWork::plan(input, budget));
    work.claim(limit);
    work.inflight_sessions(input)
}

pub fn apply_pack_tool(
    slot: &mut Option<DiscoverWork>,
    input: &FrozenInput,
    budget: usize,
    name: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let pack_id = args["pack_id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or("pack_id is required")?;
    let work = slot.get_or_insert_with(|| DiscoverWork::plan(input, budget));
    if name == "repair_pack_scan" && work.status(pack_id) != Some(PackStatus::Failed) {
        return Err("repair_pack_scan requires a failed reading pack".into());
    }
    let submit = parse_submit(args)?;
    match work.submit(pack_id, submit) {
        Ok(()) => Ok(serde_json::json!({
            "ok": true,
            "pack_id": pack_id,
            "status": "committed",
        })),
        Err(feedback) => Ok(serde_json::json!({"ok": false, "feedback": feedback})),
    }
}

fn parse_submit(args: &serde_json::Value) -> Result<PackSubmit, String> {
    let call_id = args["call_id"].as_str().unwrap_or("").trim();
    if call_id.is_empty() {
        return Err("call_id is required".into());
    }
    let rows = args["requirements"]
        .as_array()
        .ok_or("requirements must be an array")?;
    let mut requirements = Vec::new();
    for row in rows {
        let description = row["description"].as_str().unwrap_or("").trim();
        let source_id = row["source_id"].as_str().unwrap_or("").trim();
        if description.is_empty() || source_id.is_empty() {
            return Err("requirement description and source_id are required".into());
        }
        let start = row["start"]
            .as_u64()
            .ok_or("requirement start must be a byte offset")? as usize;
        let end = row["end"]
            .as_u64()
            .ok_or("requirement end must be a byte offset")? as usize;
        requirements.push(PackRequirement {
            description: description.to_string(),
            source_id: source_id.to_string(),
            start,
            end,
        });
    }
    Ok(PackSubmit {
        call_id: call_id.to_string(),
        requirements,
    })
}

pub fn plan_packs(input: &FrozenInput, soft_max_bytes: usize) -> Vec<ParsePack> {
    let budget = soft_max_bytes.max(1);
    let mut pieces = Vec::new();
    for block in chapter_blocks(input) {
        pieces.extend(split_block(block, budget));
    }
    merge_pieces(pieces, budget)
}

fn merge_pieces(pieces: Vec<ChapterBlock>, budget: usize) -> Vec<ParsePack> {
    let mut groups: Vec<Vec<ChapterBlock>> = Vec::new();
    let mut bytes = 0usize;
    for piece in pieces {
        let same_document = groups.last().is_some_and(|group| {
            group
                .last()
                .is_some_and(|last| last.document_id == piece.document_id)
        });
        if !groups.is_empty() && same_document && bytes.saturating_add(piece.bytes) <= budget {
            bytes = bytes.saturating_add(piece.bytes);
            groups.last_mut().expect("group exists").push(piece);
        } else {
            bytes = piece.bytes;
            groups.push(vec![piece]);
        }
    }
    groups
        .into_iter()
        .enumerate()
        .map(|(order, group)| pack_from(order, &group))
        .collect()
}

fn split_block(block: ChapterBlock, budget: usize) -> Vec<ChapterBlock> {
    if block.bytes <= budget {
        return vec![block];
    }
    if block.text.len() + block.forms.len() > 1 {
        let mut pieces = Vec::new();
        for (span, source_text) in block.text.iter().zip(block.source_texts.iter()) {
            pieces.extend(split_block(
                text_piece(&block, span.clone(), span.end - span.start, source_text),
                budget,
            ));
        }
        for ((span, columns), cell_bytes) in block
            .forms
            .iter()
            .zip(block.form_columns.iter())
            .zip(block.form_cell_bytes.iter())
        {
            pieces.extend(split_form_piece(
                &block,
                span.clone(),
                *columns,
                cell_bytes,
                budget,
            ));
        }
        return pieces;
    }
    if let Some(span) = block.text.first() {
        let source_text = block.source_texts.first().map(String::as_str).unwrap_or("");
        return split_text_piece(&block, span, source_text, budget);
    }
    if let Some(span) = block.forms.first() {
        let columns = block.form_columns.first().copied().unwrap_or(0);
        let cell_bytes = block
            .form_cell_bytes
            .first()
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        return split_form_piece(&block, span.clone(), columns, cell_bytes, budget);
    }
    vec![block]
}

fn text_piece(
    block: &ChapterBlock,
    span: TextSpan,
    bytes: usize,
    source_text: &str,
) -> ChapterBlock {
    ChapterBlock {
        document_id: block.document_id.clone(),
        section_key: block.section_key.clone(),
        heading: block.heading.clone(),
        context_heading: block.context_heading.clone(),
        text: vec![span],
        source_texts: vec![source_text.to_string()],
        forms: Vec::new(),
        form_columns: Vec::new(),
        form_cell_bytes: Vec::new(),
        bytes,
    }
}

fn form_piece(block: &ChapterBlock, span: FormSpan, bytes: usize) -> ChapterBlock {
    ChapterBlock {
        document_id: block.document_id.clone(),
        section_key: block.section_key.clone(),
        heading: block.heading.clone(),
        context_heading: block.context_heading.clone(),
        text: Vec::new(),
        source_texts: Vec::new(),
        forms: vec![span],
        form_columns: Vec::new(),
        form_cell_bytes: Vec::new(),
        bytes,
    }
}

fn split_text_piece(
    block: &ChapterBlock,
    span: &TextSpan,
    source_text: &str,
    budget: usize,
) -> Vec<ChapterBlock> {
    let width = span.end - span.start;
    if width <= budget {
        return vec![text_piece(block, span.clone(), width, source_text)];
    }
    let slice = source_text.get(span.start..span.end).unwrap_or("");
    ranges_for_text(slice, budget)
        .into_iter()
        .map(|(start, end)| {
            text_piece(
                block,
                TextSpan {
                    source_id: span.source_id.clone(),
                    start: span.start + start,
                    end: span.start + end,
                },
                end - start,
                source_text,
            )
        })
        .collect()
}

fn split_form_piece(
    block: &ChapterBlock,
    span: FormSpan,
    columns: usize,
    cell_bytes: &[usize],
    budget: usize,
) -> Vec<ChapterBlock> {
    let width = span.end.saturating_sub(span.start);
    let billed = form_span_bytes(cell_bytes, &span);
    if columns == 0 || billed <= budget || !width.is_multiple_of(columns) {
        return vec![form_piece(block, span, billed)];
    }
    let mut pieces = Vec::new();
    let mut cursor = span.start;
    let mut first = true;
    while cursor < span.end {
        let header_cells = if first { span.header_cells } else { columns };
        let repeated = range_bytes(cell_bytes, 0, header_cells);
        let mut take = columns;
        let mut body = range_bytes(cell_bytes, cursor, cursor + columns);
        while cursor + take + columns <= span.end {
            let next_start = cursor + take;
            let next = range_bytes(cell_bytes, next_start, next_start + columns);
            if body.saturating_add(next).saturating_add(repeated) > budget {
                break;
            }
            take += columns;
            body = body.saturating_add(next);
        }
        let end = cursor + take;
        let piece = FormSpan {
            form_id: span.form_id.clone(),
            start: cursor,
            end,
            header_cells,
        };
        let bytes = form_span_bytes(cell_bytes, &piece);
        pieces.push(form_piece(block, piece, bytes));
        cursor = end;
        first = false;
    }
    pieces
}

fn form_span_bytes(cell_bytes: &[usize], span: &FormSpan) -> usize {
    range_bytes(cell_bytes, span.start, span.end).saturating_add(range_bytes(
        cell_bytes,
        0,
        span.header_cells,
    ))
}

fn range_bytes(cell_bytes: &[usize], start: usize, end: usize) -> usize {
    if start >= end || start >= cell_bytes.len() {
        return 0;
    }
    let end = end.min(cell_bytes.len());
    cell_bytes[start..end].iter().sum()
}

fn clause_atoms(text: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut start = 0usize;
    let mut index = 0usize;
    for ch in text.chars() {
        index += ch.len_utf8();
        if ch == '\n' || ch == '。' || ch == '；' {
            ranges.push((start, index));
            start = index;
        }
    }
    if start < text.len() {
        ranges.push((start, text.len()));
    }
    ranges
}

fn char_chunks(text: &str, start: usize, end: usize, budget: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut cursor = start;
    while cursor < end {
        let mut limit = cursor.saturating_add(budget).min(end);
        if limit < end {
            while limit > cursor && !text.is_char_boundary(limit) {
                limit -= 1;
            }
        }
        if limit == cursor {
            let ch = text[cursor..].chars().next().expect("cursor is in text");
            limit = cursor + ch.len_utf8();
        }
        out.push((cursor, limit));
        cursor = limit;
    }
    out
}

fn ranges_for_text(text: &str, budget: usize) -> Vec<(usize, usize)> {
    if text.len() <= budget {
        return vec![(0, text.len())];
    }
    let mut ranges = Vec::new();
    let mut cursor = 0usize;
    let mut packed = 0usize;
    for (start, end) in clause_atoms(text) {
        let len = end - start;
        if len > budget {
            if packed > cursor {
                ranges.push((cursor, packed));
            }
            ranges.extend(char_chunks(text, start, end, budget));
            cursor = end;
            packed = end;
            continue;
        }
        if packed > cursor && packed - cursor + len > budget {
            ranges.push((cursor, packed));
            cursor = start;
        }
        packed = end;
    }
    if packed > cursor {
        ranges.push((cursor, packed));
    }
    ranges
}

fn chapter_blocks(input: &FrozenInput) -> Vec<ChapterBlock> {
    let mut sources = input.source_units.iter().collect::<Vec<_>>();
    sources.sort_by(|left, right| {
        (&left.document_id, left.ordinal).cmp(&(&right.document_id, right.ordinal))
    });
    let mut blocks: Vec<ChapterBlock> = Vec::new();
    for source in sources {
        let key = section_key(source);
        let heading = display_heading(source);
        let same = match &key {
            Some(key) => blocks.last().is_some_and(|block| {
                block.document_id == source.document_id && block.section_key == *key
            }),
            None => blocks
                .last()
                .is_some_and(|block| block.document_id == source.document_id),
        };
        if !same {
            let heading = if key.is_some() {
                heading
            } else {
                String::new()
            };
            blocks.push(ChapterBlock {
                document_id: source.document_id.clone(),
                section_key: key.clone().unwrap_or_default(),
                heading: heading.clone(),
                context_heading: parent_heading(&heading),
                text: Vec::new(),
                source_texts: Vec::new(),
                forms: Vec::new(),
                form_columns: Vec::new(),
                form_cell_bytes: Vec::new(),
                bytes: 0,
            });
        }
        let block = blocks.last_mut().expect("block was just ensured");
        let end = source.text.len();
        block.bytes = block.bytes.saturating_add(end);
        block.text.push(TextSpan {
            source_id: source.source_unit_revision_id.clone(),
            start: 0,
            end,
        });
        block.source_texts.push(source.text.clone());
        for form in input
            .structured_forms
            .iter()
            .filter(|form| form["source_unit_revision_id"] == source.source_unit_revision_id)
        {
            let Some(form_id) = form["form_definition_revision_id"].as_str() else {
                continue;
            };
            let columns = form["definition"]["column_count"].as_u64().unwrap_or(0) as usize;
            let cell_bytes = form_cell_byte_lengths(&form["definition"]);
            let text_bytes = cell_bytes.iter().sum::<usize>();
            let cells = cell_bytes.len();
            block.forms.push(FormSpan {
                form_id: form_id.to_string(),
                start: 0,
                end: cells,
                header_cells: 0,
            });
            block.form_columns.push(columns);
            block.form_cell_bytes.push(cell_bytes);
            block.bytes = block.bytes.saturating_add(text_bytes);
        }
    }
    blocks
}

fn pack_from(order: usize, blocks: &[ChapterBlock]) -> ParsePack {
    let first = &blocks[0];
    ParsePack {
        id: format!("pack-{order}"),
        document_id: first.document_id.clone(),
        order,
        context_heading: first.context_heading.clone(),
        heading: blocks
            .iter()
            .map(|block| block.heading.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        text: blocks.iter().flat_map(|block| block.text.clone()).collect(),
        forms: blocks
            .iter()
            .flat_map(|block| block.forms.clone())
            .collect(),
    }
}

fn section_key(source: &Source) -> Option<String> {
    if let Some(section) = source.locator["section_ordinal"].as_u64() {
        return Some(format!("section:{section}"));
    }
    let heading = source.locator["heading_path"].as_str().unwrap_or("").trim();
    if !heading.is_empty() {
        return Some(format!("heading:{heading}"));
    }
    None
}

fn display_heading(source: &Source) -> String {
    source.locator["heading_path"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string()
}

fn parent_heading(heading: &str) -> String {
    heading
        .rsplit_once(" > ")
        .map(|(parent, _)| parent.to_string())
        .unwrap_or_default()
}

fn form_cell_count(definition: &serde_json::Value) -> usize {
    let rows = definition["row_count"].as_u64().unwrap_or(0) as usize;
    let columns = definition["column_count"].as_u64().unwrap_or(0) as usize;
    rows.saturating_mul(columns)
}

/// UTF-8 lengths of every grid cell, in the same order `materialize_form` sends.
fn form_cell_byte_lengths(definition: &serde_json::Value) -> Vec<usize> {
    let columns = definition["column_count"].as_u64().unwrap_or(0) as usize;
    (0..form_cell_count(definition))
        .map(|index| cell_text(Some(definition), columns, index).len())
        .collect()
}

fn plan_sha(packs: &[ParsePack]) -> String {
    let bytes = serde_json::to_vec(packs).unwrap_or_default();
    hex::encode(Sha256::digest(bytes))
}

fn field(path: &str, code: &str, message: &str) -> FieldError {
    FieldError {
        path: path.to_string(),
        code: code.to_string(),
        message: message.to_string(),
    }
}

fn materialize_pack(input: &FrozenInput, pack: &ParsePack) -> Result<serde_json::Value, String> {
    let mut text = Vec::with_capacity(pack.text.len());
    for span in &pack.text {
        let source = input
            .source_units
            .iter()
            .find(|source| source.source_unit_revision_id == span.source_id)
            .ok_or_else(|| format!("reading pack source {} is missing", span.source_id))?;
        let body = source.text.get(span.start..span.end).ok_or_else(|| {
            format!(
                "reading pack slice {}..{} is outside {}",
                span.start, span.end, span.source_id
            )
        })?;
        text.push(serde_json::json!({
            "source_id": span.source_id,
            "start": span.start,
            "end": span.end,
            "text": body,
        }));
    }
    let mut forms = Vec::with_capacity(pack.forms.len());
    for span in &pack.forms {
        forms.push(materialize_form(input, span));
    }
    Ok(serde_json::json!({
        "id": pack.id,
        "document_id": pack.document_id,
        "order": pack.order,
        "context_heading": pack.context_heading,
        "heading": pack.heading,
        "text": text,
        "forms": forms,
    }))
}

fn materialize_form(input: &FrozenInput, span: &FormSpan) -> serde_json::Value {
    let definition = input
        .structured_forms
        .iter()
        .find(|form| form["form_definition_revision_id"] == span.form_id)
        .map(|form| &form["definition"]);
    let columns = definition
        .and_then(|definition| definition["column_count"].as_u64())
        .unwrap_or(0) as usize;
    let cells: Vec<String> = (span.start..span.end)
        .map(|index| cell_text(definition, columns, index))
        .collect();
    let mut form = serde_json::json!({
        "form_id": span.form_id,
        "start": span.start,
        "end": span.end,
        "header_cells": span.header_cells,
        "cells": cells,
    });
    if span.header_cells > 0 {
        let header: Vec<String> = (0..span.header_cells)
            .map(|index| cell_text(definition, columns, index))
            .collect();
        form["header"] = serde_json::json!(header);
    }
    form
}

fn cell_text(definition: Option<&serde_json::Value>, columns: usize, index: usize) -> String {
    if columns == 0 {
        return String::new();
    }
    let row = index / columns;
    let column = index % columns;
    definition
        .and_then(|definition| definition["cells"].as_array())
        .and_then(|cells| {
            cells.iter().find(|cell| {
                cell["row"].as_u64() == Some(row as u64)
                    && cell["column"].as_u64() == Some(column as u64)
            })
        })
        .and_then(|cell| cell["text"].as_str())
        .unwrap_or("")
        .to_string()
}

fn feedback(pack_id: &str, submit: &PackSubmit, errors: Vec<FieldError>) -> PackFeedback {
    let total = errors.len();
    PackFeedback {
        pack_id: pack_id.to_string(),
        call_id: submit.call_id.clone(),
        arguments_sha256: hex::encode(Sha256::digest(
            serde_json::to_vec(submit).unwrap_or_default(),
        )),
        errors,
        total,
        truncated: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn source(id: &str, ordinal: usize, text: &str, heading: &str) -> Source {
        Source {
            source_unit_revision_id: id.into(),
            document_id: "doc".into(),
            text: text.into(),
            locator: json!({"heading_path": heading}),
            ordinal,
        }
    }

    fn input(sources: Vec<Source>, forms: Vec<serde_json::Value>) -> FrozenInput {
        FrozenInput {
            schema_version: 1,
            project_id: "project".into(),
            document_set_id: "set".into(),
            documents: vec![],
            document_relations: vec![],
            source_units: sources,
            structured_forms: forms,
            decisions: vec![],
        }
    }

    fn located(id: &str, ordinal: usize, text: &str, locator: serde_json::Value) -> Source {
        Source {
            source_unit_revision_id: id.into(),
            document_id: "doc".into(),
            text: text.into(),
            locator,
            ordinal,
        }
    }

    #[test]
    fn a_section_splits_on_clauses_before_characters() {
        let frozen = input(vec![source("s1", 0, "甲。乙", "第一章 > 投标函")], vec![]);
        let packs = plan_packs(&frozen, 6);
        assert_eq!(packs.len(), 2);
        assert_eq!(packs[0].text[0].start, 0);
        assert_eq!(packs[0].text[0].end, "甲。".len());
        assert_eq!(packs[1].text[0].start, "甲。".len());
        assert_eq!(packs[1].text[0].end, "甲。乙".len());
        assert_eq!(packs[0].context_heading, "第一章");
    }

    #[test]
    fn a_clause_larger_than_the_budget_splits_on_a_character_boundary() {
        let frozen = input(vec![source("s1", 0, "投标", "第一章")], vec![]);
        let packs = plan_packs(&frozen, 4);
        assert_eq!(
            packs
                .iter()
                .map(|pack| (pack.text[0].start, pack.text[0].end))
                .collect::<Vec<_>>(),
            vec![(0, "投".len()), ("投".len(), "投标".len())]
        );
    }

    #[test]
    fn adjacent_sections_merge_under_the_budget() {
        let frozen = input(
            vec![
                source("a", 0, "商务", "第一章 > 投标函"),
                source("b", 1, "技术", "第一章 > 技术方案"),
                source("c", 2, "报价表正文", "第二章 > 报价"),
            ],
            vec![json!({
                "form_definition_revision_id": "form-price",
                "source_unit_revision_id": "c",
                "definition": {
                    "row_count": 2,
                    "column_count": 2,
                    "cells": [{"row": 0, "column": 0, "text": "abcd"}]
                }
            })],
        );
        // "报价表正文" is 15 bytes and the cell text is 4. Together they fit in 20.
        let separate = plan_packs(&frozen, 20);
        assert_eq!(separate.len(), 2);
        let price = separate
            .iter()
            .find(|pack| pack.heading.contains("报价"))
            .unwrap();
        assert_eq!(
            price.forms,
            vec![FormSpan {
                form_id: "form-price".into(),
                start: 0,
                end: 4,
                header_cells: 0,
            }]
        );

        let merged = plan_packs(&frozen, 10_000);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].text.len(), 3);
        assert!(merged[0].text.iter().all(|span| span.start == 0));
    }

    #[test]
    fn a_wide_table_splits_by_body_row_and_does_not_rescan_the_header() {
        let frozen = input(
            vec![source("c", 0, "", "第二章 > 报价")],
            vec![json!({
                "form_definition_revision_id": "form-price",
                "source_unit_revision_id": "c",
                "definition": {
                    "row_count": 3,
                    "column_count": 2,
                    "cells": [
                        {"row": 0, "column": 0, "text": "甲"},
                        {"row": 0, "column": 1, "text": "甲"},
                        {"row": 1, "column": 0, "text": "甲"},
                        {"row": 1, "column": 1, "text": "甲"},
                        {"row": 2, "column": 0, "text": "甲"},
                        {"row": 2, "column": 1, "text": "甲"}
                    ]
                }
            })],
        );
        // Each cell is 3 bytes. Two rows are 12 bytes; six cells are not.
        let packs = plan_packs(&frozen, 12);
        let forms: Vec<_> = packs.iter().flat_map(|pack| pack.forms.clone()).collect();
        assert_eq!(
            forms,
            vec![
                FormSpan {
                    form_id: "form-price".into(),
                    start: 0,
                    end: 4,
                    header_cells: 0,
                },
                FormSpan {
                    form_id: "form-price".into(),
                    start: 4,
                    end: 6,
                    header_cells: 2,
                },
            ]
        );
    }

    #[test]
    fn many_empty_or_short_cells_do_not_exceed_the_byte_budget() {
        let mut cells = Vec::new();
        for column in 0..8 {
            cells.push(json!({"row": 0, "column": column, "text": "a"}));
        }
        for column in 0..4 {
            cells.push(json!({"row": 1, "column": column, "text": "ab"}));
        }
        let frozen = input(
            vec![source("c", 0, "", "第二章 > 报价")],
            vec![json!({
                "form_definition_revision_id": "form-price",
                "source_unit_revision_id": "c",
                "definition": {
                    "row_count": 8,
                    "column_count": 8,
                    "cells": cells
                }
            })],
        );
        // 64 cells hold 16 bytes. Counting cells would split this pack.
        let packs = plan_packs(&frozen, 16);
        assert_eq!(packs.len(), 1);
        assert_eq!(
            packs[0].forms,
            vec![FormSpan {
                form_id: "form-price".into(),
                start: 0,
                end: 64,
                header_cells: 0,
            }]
        );
    }

    #[test]
    fn a_few_long_cells_exceed_the_byte_budget_and_split_by_row() {
        let long = "甲".repeat(30);
        let frozen = input(
            vec![source("c", 0, "", "第二章 > 报价")],
            vec![json!({
                "form_definition_revision_id": "form-price",
                "source_unit_revision_id": "c",
                "definition": {
                    "row_count": 3,
                    "column_count": 2,
                    "cells": [
                        {"row": 0, "column": 0, "text": "标题"},
                        {"row": 0, "column": 1, "text": "说明"},
                        {"row": 1, "column": 0, "text": long},
                        {"row": 1, "column": 1, "text": ""},
                        {"row": 2, "column": 0, "text": "x"},
                        {"row": 2, "column": 1, "text": "y"}
                    ]
                }
            })],
        );
        // Six cells fit in 30. The 90-byte cell does not, so the split follows row text.
        let packs = plan_packs(&frozen, 30);
        let forms: Vec<_> = packs.iter().flat_map(|pack| pack.forms.clone()).collect();
        assert_eq!(
            forms,
            vec![
                FormSpan {
                    form_id: "form-price".into(),
                    start: 0,
                    end: 2,
                    header_cells: 0,
                },
                FormSpan {
                    form_id: "form-price".into(),
                    start: 2,
                    end: 4,
                    header_cells: 2,
                },
                FormSpan {
                    form_id: "form-price".into(),
                    start: 4,
                    end: 6,
                    header_cells: 2,
                },
            ]
        );
        let mut work = DiscoverWork::plan(&frozen, 30);
        work.claim(10);
        let mut sent = Vec::new();
        for session in work.inflight_sessions(&frozen) {
            for form in session["pack"]["forms"].as_array().unwrap() {
                let cells: usize = form["cells"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|cell| cell.as_str().unwrap().len())
                    .sum();
                let header: usize = form
                    .get("header")
                    .and_then(|header| header.as_array())
                    .map(|header| header.iter().map(|cell| cell.as_str().unwrap().len()).sum())
                    .unwrap_or(0);
                sent.push(cells + header);
            }
        }
        assert_eq!(sent, vec![12, 102, 14]);
    }

    #[test]
    fn the_same_section_continues_across_pages_and_keeps_its_table() {
        let frozen = input(
            vec![
                located(
                    "a",
                    0,
                    "甲",
                    json!({"section_ordinal": 0, "heading_path": "第一章", "page_ordinal": 0}),
                ),
                located("table", 1, "", json!({"page_ordinal": 0})),
                located(
                    "b",
                    2,
                    "乙",
                    json!({"section_ordinal": 0, "heading_path": "第一章", "page_ordinal": 1}),
                ),
            ],
            vec![],
        );
        let packs = plan_packs(&frozen, 10_000);
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].heading, "第一章");
        assert_eq!(packs[0].text.len(), 3);
        assert!(!packs[0].heading.contains("page:"));
    }

    #[test]
    fn a_failed_pack_does_not_commit_or_clear_another_pack() {
        let frozen = input(
            vec![
                source("a", 0, "A", "第一章 > 投标函"),
                source("b", 1, "B", "第二章 > 技术方案"),
            ],
            vec![],
        );
        let mut work = DiscoverWork::plan(&frozen, 1);
        let claimed = work.claim(DEFAULT_PACK_CONCURRENCY);
        assert_eq!(claimed.len(), 2);
        work.submit(
            "pack-0",
            PackSubmit {
                call_id: "call-1".into(),
                requirements: vec![PackRequirement {
                    description: "投标函".into(),
                    source_id: "a".into(),
                    start: 0,
                    end: 1,
                }],
            },
        )
        .unwrap();
        let err = work
            .submit(
                "pack-1",
                PackSubmit {
                    call_id: "call-2".into(),
                    requirements: vec![PackRequirement {
                        description: "错章".into(),
                        source_id: "a".into(),
                        start: 0,
                        end: 1,
                    }],
                },
            )
            .unwrap_err();
        assert_eq!(err.errors[0].code, "outside_pack");
        assert_eq!(err.total, 1);
        assert!(!err.truncated);
        assert_eq!(work.status("pack-0"), Some(PackStatus::Committed));
        assert_eq!(work.status("pack-1"), Some(PackStatus::Failed));
        assert_eq!(work.requirement_count(), 1);
        let session = work.session(&frozen, "pack-1").unwrap();
        assert!(session.get("feedback").is_some());
        assert_eq!(session["pack"]["text"][0]["text"], "B");
        assert!(!session.to_string().contains("投标函"));
        assert!(work.requirement("pack-1:0").is_none());
        let kept = work.requirement("pack-0:0").unwrap();
        assert_eq!(kept.source_id, "a");
        assert_eq!((kept.start, kept.end), (0, 1));
    }

    #[test]
    fn claim_turn_then_repair_keeps_a_committed_pack() {
        let frozen = input(
            vec![
                source("a", 0, "A", "第一章 > 投标函"),
                source("b", 1, "B", "第二章 > 技术方案"),
            ],
            vec![],
        );
        let mut slot = None;
        let sessions = claim_turn(&mut slot, &frozen, 1, DEFAULT_PACK_CONCURRENCY);
        assert_eq!(sessions.len(), 2);
        let bad = apply_pack_tool(
            &mut slot,
            &frozen,
            1,
            "submit_pack_scan",
            &json!({
                "pack_id": "pack-1",
                "call_id": "bad",
                "requirements": [{"description": "错", "source_id": "a", "start": 0, "end": 1}]
            }),
        )
        .unwrap();
        assert_eq!(bad["ok"], false);
        assert_eq!(bad["feedback"]["errors"][0]["code"], "outside_pack");
        let err = apply_pack_tool(
            &mut slot,
            &frozen,
            1,
            "repair_pack_scan",
            &json!({
                "pack_id": "pack-0",
                "call_id": "nope",
                "requirements": []
            }),
        )
        .unwrap_err();
        assert!(err.contains("failed reading pack"));
        apply_pack_tool(
            &mut slot,
            &frozen,
            1,
            "submit_pack_scan",
            &json!({
                "pack_id": "pack-0",
                "call_id": "ok",
                "requirements": [{"description": "投标函", "source_id": "a", "start": 0, "end": 1}]
            }),
        )
        .unwrap();
        apply_pack_tool(
            &mut slot,
            &frozen,
            1,
            "repair_pack_scan",
            &json!({
                "pack_id": "pack-1",
                "call_id": "fix",
                "requirements": [{"description": "技术", "source_id": "b", "start": 0, "end": 1}]
            }),
        )
        .unwrap();
        let work = slot.unwrap();
        assert_eq!(work.status("pack-0"), Some(PackStatus::Committed));
        assert_eq!(work.status("pack-1"), Some(PackStatus::Committed));
        assert_eq!(work.requirement_count(), 2);
        assert_eq!(work.requirement("pack-1:0").unwrap().source_id, "b");
    }

    #[test]
    fn outside_slice_does_not_store_the_requirement() {
        let frozen = input(vec![source("a", 0, "甲乙", "第一章")], vec![]);
        let mut work = DiscoverWork::plan(&frozen, 10_000);
        work.claim(1);
        let err = work
            .submit(
                "pack-0",
                PackSubmit {
                    call_id: "call".into(),
                    requirements: vec![PackRequirement {
                        description: "越界".into(),
                        source_id: "a".into(),
                        start: 0,
                        end: "甲乙".len() + 1,
                    }],
                },
            )
            .unwrap_err();
        assert_eq!(err.errors[0].code, "outside_slice");
        assert_eq!(work.status("pack-0"), Some(PackStatus::Failed));
        assert_eq!(work.requirement_count(), 0);
    }

    #[test]
    fn brief_packs_carry_slice_text_and_continuation_headers() {
        let prefix = "包外前";
        let middle = "条款内容超过预算必须切开";
        let suffix = "包外后";
        let body = format!("{prefix}。{middle}。{suffix}");
        let frozen = input(
            vec![
                located(
                    "later",
                    2,
                    "后文",
                    json!({"heading_path": "第三章", "section_ordinal": 2}),
                ),
                located(
                    "body",
                    0,
                    &body,
                    json!({"heading_path": "第一章 > 投标函", "section_ordinal": 0}),
                ),
                source("table", 1, "", "第二章 > 报价"),
            ],
            vec![json!({
                "form_definition_revision_id": "form-price",
                "source_unit_revision_id": "table",
                "definition": {
                    "title": "报价",
                    "row_count": 3,
                    "column_count": 2,
                    "cells": [
                        {"row": 0, "column": 0, "text": "序号"},
                        {"row": 0, "column": 1, "text": "项目"},
                        {"row": 1, "column": 0, "text": "1"},
                        {"row": 1, "column": 1, "text": "人工"},
                        {"row": 2, "column": 0, "text": "2"},
                        {"row": 2, "column": 1, "text": "材料"}
                    ]
                }
            })],
        );
        let mut work = DiscoverWork::plan(&frozen, 4);
        work.claim(100);
        let mut sessions = work.inflight_sessions(&frozen);
        sessions.sort_by_key(|session| session["pack"]["order"].as_u64().unwrap());
        let ids: Vec<_> = sessions
            .iter()
            .filter_map(|session| {
                session["pack"]["text"]
                    .as_array()
                    .and_then(|rows| rows.first())
                    .and_then(|row| row["source_id"].as_str())
                    .filter(|id| !id.is_empty())
            })
            .collect();
        assert!(
            ids.windows(2).all(|pair| {
                let left = frozen
                    .source_units
                    .iter()
                    .find(|source| source.source_unit_revision_id == pair[0])
                    .unwrap()
                    .ordinal;
                let right = frozen
                    .source_units
                    .iter()
                    .find(|source| source.source_unit_revision_id == pair[1])
                    .unwrap()
                    .ordinal;
                left <= right
            }),
            "packs follow frozen source order, got {ids:?}"
        );
        let sources = ["body", "later"];
        for source_id in sources {
            let source = frozen
                .source_units
                .iter()
                .find(|source| source.source_unit_revision_id == source_id)
                .unwrap();
            let mut covered = String::new();
            for session in &sessions {
                for span in session["pack"]["text"].as_array().unwrap() {
                    if span["source_id"] != source_id {
                        continue;
                    }
                    let start = span["start"].as_u64().unwrap() as usize;
                    let end = span["end"].as_u64().unwrap() as usize;
                    let text = span["text"].as_str().unwrap();
                    assert_eq!(text, &source.text[start..end]);
                    assert!(
                        !text.contains("人工"),
                        "table cell text stays out of prose packs"
                    );
                    covered.push_str(text);
                }
            }
            assert_eq!(covered, source.text);
        }
        let letter = sessions
            .iter()
            .find(|session| {
                session["pack"]["heading"]
                    .as_str()
                    .unwrap()
                    .contains("投标函")
            })
            .unwrap();
        assert_eq!(letter["pack"]["context_heading"], "第一章");
        let form = sessions
            .iter()
            .flat_map(|session| session["pack"]["forms"].as_array().unwrap().iter())
            .find(|form| form["cells"] == json!(["2", "材料"]))
            .expect("continuation slice");
        assert_eq!(form["header_cells"], 2);
        let start = form["start"].as_u64().unwrap() as usize;
        let end = form["end"].as_u64().unwrap() as usize;
        assert_eq!(form["header"], json!(["序号", "项目"]));
        assert_eq!(form["cells"], json!(["2", "材料"]));
        assert_eq!(form["cells"].as_array().unwrap().len(), end - start);
        assert!((0..2).all(|index| index < start || index >= end));
        assert!(!form["cells"].to_string().contains("序号"));
    }

    #[test]
    fn running_and_failed_packs_are_replayed_with_text() {
        let frozen = input(
            vec![
                source("a", 0, "A", "第一章 > 投标函"),
                source("b", 1, "B", "第二章 > 技术方案"),
                source("c", 2, "C", "第三章"),
            ],
            vec![],
        );
        let mut slot = None;
        let first = claim_turn(&mut slot, &frozen, 1, 1);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0]["pack"]["id"], "pack-0");
        assert_eq!(first[0]["pack"]["text"][0]["text"], "A");
        assert_eq!(first[0]["status"], "running");
        let again = claim_turn(&mut slot, &frozen, 1, 1);
        assert_eq!(again.len(), 2);
        assert_eq!(again[0]["pack"]["text"][0]["text"], "A");
        assert_eq!(again[1]["pack"]["id"], "pack-1");
        assert_eq!(again[1]["pack"]["text"][0]["text"], "B");
        apply_pack_tool(
            &mut slot,
            &frozen,
            1,
            "submit_pack_scan",
            &json!({
                "pack_id": "pack-0",
                "call_id": "bad",
                "requirements": [{"description": "越界", "source_id": "a", "start": 0, "end": 2}]
            }),
        )
        .unwrap();
        apply_pack_tool(
            &mut slot,
            &frozen,
            1,
            "submit_pack_scan",
            &json!({
                "pack_id": "pack-1",
                "call_id": "ok",
                "requirements": [{"description": "技术", "source_id": "b", "start": 0, "end": 1}]
            }),
        )
        .unwrap();
        let replay = claim_turn(&mut slot, &frozen, 1, 1);
        let ids: Vec<_> = replay
            .iter()
            .map(|session| session["pack"]["id"].as_str().unwrap())
            .collect();
        assert!(ids.contains(&"pack-0"), "failed pack is replayed: {ids:?}");
        assert!(!ids.contains(&"pack-1"), "committed pack leaves the brief");
        assert!(ids.contains(&"pack-2"));
        let failed = replay
            .iter()
            .find(|session| session["pack"]["id"] == "pack-0")
            .unwrap();
        assert_eq!(failed["status"], "failed");
        assert_eq!(failed["pack"]["text"][0]["text"], "A");
        assert!(failed["feedback"]["errors"][0]["code"] == "outside_slice");
    }
}
