//! Old scan and fill application. The six outline tools live in
//! [`crate::outline::agent`]. The two response tools live in
//! [`crate::response::agent`].
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DraftStage {
    #[default]
    None,
    Outline,
    Fill,
    Published,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DraftStatus {
    Pending,
    Filled,
    Omitted,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChapterPurpose {
    Group,
    Response,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BodyStatus {
    Empty,
    User,
    Generated,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OmitReason {
    BindFailed,
    Deadline,
    NotApplicable,
    ParseFailed,
    BlankRule,
    WindowExceeded,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DraftPlanItem {
    #[serde(default)]
    pub grounds: Vec<Span>,
    #[serde(default)]
    pub requirement_ids: Vec<String>,
    pub id: String,
    pub parent: Option<String>,
    pub order: usize,
    pub title: String,
    pub prescribed: bool,
    #[serde(default)]
    pub source_ids: Vec<String>,
    #[serde(default)]
    pub windows: Vec<Vec<String>>,
    #[serde(default)]
    pub window_index: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_id: Option<String>,
    pub status: DraftStatus,
    pub purpose: ChapterPurpose,
    pub format_refs: Vec<Span>,
    pub body_status: BodyStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub omit_reason: Option<OmitReason>,
    /// 回读到的用户正文，原样重排。只有填充 run 的种子会带它：阶段一的大纲里
    /// 没有正文，官方编制也不认这条路径（`Content::Preserved` 是草稿专用）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preserved: Vec<super::readback::Preserved>,
}

pub fn plan_ready(plan: &[DraftPlanItem]) -> bool {
    !plan.is_empty()
}

pub fn has_open_chapter(plan: &[DraftPlanItem]) -> bool {
    plan.iter().any(|item| item.status != DraftStatus::Omitted)
}

fn heading_parts(source: &Source) -> Vec<String> {
    source.locator["heading_path"]
        .as_str()
        .unwrap_or("")
        .split(" > ")
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

/// 「已填」必须真有正文可编译：要么有模板 record，要么带回读来的用户正文。
pub fn filled_templates_present(analysis: &Analysis) -> bool {
    analysis.draft_plan.iter().all(|item| match item.status {
        DraftStatus::Filled => {
            !item.preserved.is_empty()
                || item
                    .template_id
                    .as_ref()
                    .is_some_and(|id| analysis.records.contains_key(id))
        }
        _ => true,
    })
}

fn folded_contains(haystack: &str, needle: &str) -> bool {
    let compact = |text: &str| {
        text.chars()
            .filter(|ch| !ch.is_whitespace())
            .collect::<String>()
    };
    let needle = compact(needle);
    !needle.is_empty() && compact(haystack).contains(&needle)
}

fn title_core(title: &str) -> &str {
    let title = title.trim();
    match title.find(['(', '（']) {
        Some(index) if index > 0 => title[..index].trim(),
        _ => title,
    }
}

fn title_needles(title: &str, extra_terms: &[String]) -> Vec<String> {
    let needle = title.trim();
    let mut terms = Vec::new();
    if !needle.is_empty() {
        terms.push(needle.to_string());
        let core = title_core(needle);
        if core != needle && core.chars().count() >= 2 {
            terms.push(core.to_string());
        }
    }
    terms.extend(
        extra_terms
            .iter()
            .map(|term| term.trim().to_string())
            .filter(|term| !term.is_empty()),
    );
    terms
}

fn source_matches_terms(input: &FrozenInput, source: &Source, terms: &[String]) -> bool {
    terms.iter().any(|term| folded_contains(&source.text, term))
        || form_titles(input, &source.source_unit_revision_id)
            .iter()
            .any(|name| terms.iter().any(|term| folded_contains(name, term)))
}

/// 命中来源的优先级：表名包含 > 解析器给出的 `heading_path` 同名 > 只在正文出现。
/// 不使用组成／格式词表抬优先级。
fn bind_rank(input: &FrozenInput, source: &Source, terms: &[String]) -> u8 {
    if form_titles(input, &source.source_unit_revision_id)
        .iter()
        .any(|name| terms.iter().any(|term| folded_contains(name, term)))
    {
        return 0;
    }
    if heading_parts(source)
        .iter()
        .any(|part| terms.iter().any(|term| folded_contains(part, term)))
    {
        return 2;
    }
    3
}

/// 按 title（及可选配置词）在冻结正文/表名中包含匹配；空白/换行不拆词。失败不猜页。
pub fn bind_source_ids(
    input: &FrozenInput,
    title: &str,
    extra_terms: &[String],
) -> Result<Vec<String>, OmitReason> {
    let terms = title_needles(title, extra_terms);
    if terms.is_empty() {
        return Err(OmitReason::BindFailed);
    }
    let mut hits: Vec<(u8, usize, String)> = input
        .source_units
        .iter()
        .filter(|source| source_matches_terms(input, source, &terms))
        .map(|source| {
            (
                bind_rank(input, source, &terms),
                source.ordinal,
                source.source_unit_revision_id.clone(),
            )
        })
        .collect();
    if hits.is_empty() {
        return Err(OmitReason::BindFailed);
    }
    hits.sort();
    hits.dedup();
    Ok(hits.into_iter().map(|(_, _, id)| id).collect())
}

fn specialize_pending_windows(input: &FrozenInput, plan: &mut [DraftPlanItem]) {
    for item in plan
        .iter_mut()
        .filter(|item| item.status == DraftStatus::Pending)
    {
        if item.omit_reason.is_some() || !item.source_ids.is_empty() {
            continue;
        }
        let mut from_grounds: Vec<String> = item
            .grounds
            .iter()
            .map(|span| span.source_id.clone())
            .collect();
        from_grounds.sort();
        from_grounds.dedup();
        if !from_grounds.is_empty() {
            item.source_ids = from_grounds;
            item.windows = vec![item.source_ids.clone()];
            item.omit_reason = None;
            continue;
        }
        item.windows.clear();
        item.window_index = 0;
        match bind_source_ids(input, &item.title, &[]) {
            Ok(hits) => {
                item.source_ids = hits;
                item.windows = vec![item.source_ids.clone()];
                item.omit_reason = None;
            }
            Err(reason) => {
                item.source_ids.clear();
                item.omit_reason = Some(reason);
            }
        }
    }
}

fn form_titles(input: &FrozenInput, source_id: &str) -> Vec<String> {
    input
        .structured_forms
        .iter()
        .filter(|form| form["source_unit_revision_id"] == source_id)
        .filter_map(|form| {
            form["definition"]["title"]
                .as_str()
                .or_else(|| form["title"].as_str())
                .map(str::to_string)
        })
        .collect()
}

pub fn current_window(item: &DraftPlanItem) -> &[String] {
    item.windows
        .get(item.window_index)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

pub fn merge_regions(
    existing: &[TemplateRegion],
    incoming: Vec<TemplateRegion>,
) -> Vec<TemplateRegion> {
    let mut out = existing.to_vec();
    for region in incoming {
        let key = (
            region.form_id.clone(),
            region.cells.clone(),
            region.source.source_id.clone(),
            region.source.start,
            region.source.end,
        );
        if let Some(prior) = out.iter_mut().find(|old| {
            (
                old.form_id.clone(),
                old.cells.clone(),
                old.source.source_id.clone(),
                old.source.start,
                old.source.end,
            ) == key
        }) {
            *prior = region;
        } else {
            out.push(region);
        }
    }
    out
}

/// 非空原文不得被 bidder_blank 整段抹掉；未选中的字必须保留。
pub fn reject_blank_erasing_source(
    input: &FrozenInput,
    regions: &[TemplateRegion],
) -> Result<(), String> {
    for region in regions {
        if region.role != RegionRole::BidderBlank {
            continue;
        }
        let text = region_source_text(input, region)?;
        if text.trim().is_empty() {
            continue;
        }
        let erased = if region.blank_ranges.is_empty() {
            true
        } else {
            let mut kept = text.clone();
            let mut ranges: Vec<(usize, usize)> = region
                .blank_ranges
                .iter()
                .map(|range| char_range(&text, range.start, range.end))
                .collect::<Result<_, _>>()?;
            ranges.retain(|(start, end)| start < end);
            ranges.sort_by_key(|range| range.0);
            for (start, end) in ranges.into_iter().rev() {
                kept.replace_range(start..end, "");
            }
            kept.trim().is_empty()
        };
        if erased {
            return Err("bidder_blank 不得抹掉非空原文的全部可见文字".into());
        }
    }
    Ok(())
}

fn region_source_text(input: &FrozenInput, region: &TemplateRegion) -> Result<String, String> {
    if let Some(form_id) = &region.form_id
        && let Some(cell) = region.cells.first()
    {
        let form = input
            .structured_forms
            .iter()
            .find(|form| form["form_definition_revision_id"] == *form_id)
            .ok_or("unknown form")?;
        let text = form["definition"]["cells"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|entry| entry["row"] == cell.row && entry["column"] == cell.column)
            .and_then(|entry| entry["text"].as_str())
            .unwrap_or("");
        return Ok(text.to_string());
    }
    let source = input
        .source_units
        .iter()
        .find(|source| source.source_unit_revision_id == region.source.source_id)
        .ok_or("unknown source")?;
    let (start, end) = char_range(&source.text, region.source.start, region.source.end)?;
    Ok(source.text[start..end].to_string())
}

fn char_range(text: &str, start: usize, end: usize) -> Result<(usize, usize), String> {
    let start = start.min(text.len());
    let end = end.min(text.len());
    if !text.is_char_boundary(start) || !text.is_char_boundary(end) {
        return Err(
            "text range is not a UTF-8 boundary; use read_source line_spans for exact offsets"
                .into(),
        );
    }
    Ok((start, end))
}

pub fn mark_window_coverage(input: &FrozenInput, coverage: &mut Coverage, window: &[String]) {
    for id in window {
        if let Some(source) = input
            .source_units
            .iter()
            .find(|source| source.source_unit_revision_id == *id)
        {
            coverage
                .text
                .entry(id.clone())
                .or_default()
                .push((0, source.text.len()));
        }
        for form in &input.structured_forms {
            if form["source_unit_revision_id"] == *id
                && let Some(form_id) = form["form_definition_revision_id"].as_str()
            {
                let n = form["definition"]["cells"]
                    .as_array()
                    .map(Vec::len)
                    .unwrap_or(0);
                coverage
                    .form_cells
                    .entry(form_id.to_string())
                    .or_default()
                    .push((0, n));
            }
        }
    }
}

/// Recompute provenance from saved obligations and the final tree, never titles.
///
/// Restored: still called by `outline_flow.rs` after requirement-ID remapping.
/// (B3 removed the old fill tool cluster around it, but this helper is live.)
pub(super) fn refresh_outline_basis(
    _input: &FrozenInput,
    state: &mut super::agent::Checkpoint,
) -> Result<(), String> {
    super::outline_flow::tree_valid(&state.analysis.draft_plan)?;
    let positions: std::collections::BTreeMap<_, _> = state
        .analysis
        .draft_plan
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id.clone(), index))
        .collect();
    let mut children = vec![Vec::new(); positions.len()];
    let mut parents = vec![None; positions.len()];
    for (index, node) in state.analysis.draft_plan.iter().enumerate() {
        if let Some(parent) = &node.parent {
            let parent = positions[parent];
            children[parent].push(index);
            parents[index] = Some(parent);
        }
    }
    let mut pending: Vec<_> = children.iter().map(Vec::len).collect();
    let mut ready: Vec<_> = pending
        .iter()
        .enumerate()
        .filter_map(|(i, count)| (*count == 0).then_some(i))
        .collect();
    while let Some(index) = ready.pop() {
        let node = &state.analysis.draft_plan[index];
        let mut grounds = std::collections::BTreeMap::new();
        let mut formats = std::collections::BTreeMap::new();
        for id in &node.requirement_ids {
            let need = state
                .analysis
                .outline
                .requirements
                .get(id)
                .ok_or("unknown chapter requirement")?;
            if need.applicability == super::outline_flow::Applicability::NotApplicable {
                continue;
            }
            for span in &need.grounds {
                grounds.insert(serde_json::to_string(span).unwrap(), span.clone());
            }
            for span in &need.format_grounds {
                formats.insert(serde_json::to_string(span).unwrap(), span.clone());
            }
        }
        if node.purpose == ChapterPurpose::Group {
            for child in &children[index] {
                let child = &state.analysis.draft_plan[*child];
                if child.status != DraftStatus::Omitted {
                    for span in &child.grounds {
                        grounds.insert(serde_json::to_string(span).unwrap(), span.clone());
                    }
                    for span in &child.format_refs {
                        formats.insert(serde_json::to_string(span).unwrap(), span.clone());
                    }
                }
            }
        }
        let node = &mut state.analysis.draft_plan[index];
        node.requirement_ids.sort();
        node.requirement_ids.dedup();
        node.grounds = grounds.into_values().collect();
        node.format_refs = formats.into_values().collect();
        let spans = if node.format_refs.is_empty() {
            &node.grounds
        } else {
            &node.format_refs
        };
        node.source_ids = spans
            .iter()
            .map(|span| span.source_id.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        node.windows = vec![node.source_ids.clone()];
        node.window_index = 0;
        if let Some(parent) = parents[index] {
            pending[parent] -= 1;
            if pending[parent] == 0 {
                ready.push(parent);
            }
        }
    }
    Ok(())
}

pub fn assign_next_chapter(input: &FrozenInput, state: &mut super::agent::Checkpoint) -> bool {
    specialize_pending_windows(input, &mut state.analysis.draft_plan);
    let next = state
        .analysis
        .draft_plan
        .iter()
        .filter(|item| {
            item.status == DraftStatus::Pending
                && item.omit_reason.is_none()
                && !item.source_ids.is_empty()
        })
        // Group nodes are structural only. Response parents with children still
        // carry their own body and must be filled; leaves always remain eligible.
        .filter(|item| item.purpose != ChapterPurpose::Group)
        .filter(|item| {
            item.parent.as_ref().is_none_or(|parent| {
                state
                    .analysis
                    .draft_plan
                    .iter()
                    .any(|node| node.id == *parent)
            })
        })
        .min_by_key(|item| (if item.parent.is_some() { 0 } else { 1 }, item.order))
        .map(|item| item.id.clone());
    let Some(id) = next else {
        state.draft_active_id = None;
        return false;
    };
    state.draft_active_id = Some(id.clone());
    // 整份填充要轮转很多章，停滞保护必须是 Job 级账本：只有「本章回合数」这个
    // 计时器随换章清零，累计的无进展轮次与 replan/blocked 状态留着。否则每换一
    // 章就把全局保护清零，一个卡住的 Job 可以把预算烧到底。
    state.main_progress.watch.focus_turns = 0;
    if let Some(item) = state.analysis.draft_plan.iter().find(|item| item.id == id) {
        let window = current_window(item).to_vec();
        state.main_work = Some(super::agent::WorkState {
            source_scope: window,
            deferred_sources: vec![],
            objective: format!("填写章节 {}", item.title),
            focus: Default::default(),
            output_refs: vec![],
            pending_refs: vec![],
            status: super::agent::context::WorkStatus::Active,
            note: String::new(),
        });
    }
    true
}

pub fn after_batch(
    input: &FrozenInput,
    state: &mut super::agent::Checkpoint,
    batch_failed: bool,
    stop: bool,
) -> Result<(), String> {
    if batch_failed {
        return Ok(());
    }
    match state.draft_stage {
        DraftStage::None | DraftStage::Outline => {
            use super::outline_flow::Phase;
            state.draft_stage = DraftStage::Outline;
            let packs_done = state
                .outline_run
                .reading_packs
                .as_ref()
                .is_some_and(|work| work.complete());
            if state.analysis.outline.phase == Phase::Discover && packs_done {
                state.analysis.outline.phase = Phase::Outline;
                state.outline_run.phase = Phase::Outline;
                state.main_work = None;
                // Preserve repair evidence and failed-call identities; only
                // first discovery hands off with a fresh conversation.
                // Clears only after every pack is committed.
                if state.analysis.outline.checks.is_empty() {
                    state.transcript.clear();
                }
            }
            let sha = super::digest(input)?;
            if crate::outline::project_draft(input, &sha, &state.outline_run.tool_draft).is_ok() {
                state.draft_stage = DraftStage::Published;
                state.done = true;
            }
        }
        DraftStage::Fill => {
            let active_done = state.draft_active_id.as_ref().is_none_or(|id| {
                state.analysis.draft_plan.iter().any(|item| {
                    item.id == *id
                        && (matches!(item.status, DraftStatus::Filled | DraftStatus::Omitted)
                            || item.omit_reason.is_some())
                })
            });
            // 用户要停：当前章收尾后就不再派下一章，走正常收尾编译出稿。剩下的
            // 章仍是 Pending，在稿子里是空 heading，下一次填章接着往下写。
            if active_done && (stop || !assign_next_chapter(input, state)) {
                state.draft_stopped = stop;
                state.draft_stage = DraftStage::Published;
                state.done = true;
            }
        }
        DraftStage::Published => state.done = true,
    }
    Ok(())
}
