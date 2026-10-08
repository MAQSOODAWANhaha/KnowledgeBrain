//! Old scan and fill application. The six outline tools live in
//! [`crate::outline::agent`]. The two response tools live in
//! [`crate::response::agent`].
use super::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
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

pub fn apply(
    input: &FrozenInput,
    config: &super::agent::Config,
    state: &mut super::agent::Checkpoint,
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    refuse_retired_outline_tool(name)?;
    apply_validated(input, config, state, name, args)
}

/// The one-shot run finishes through `outline::agent::apply`.
fn refuse_retired_outline_tool(name: &str) -> Result<(), String> {
    if matches!(
        name,
        "submit_outline_scan" | "put_outline_items" | "submit_outline_check" | "finish_outline"
    ) {
        return Err("retired outline tool is not on the product path".into());
    }
    Ok(())
}

pub(super) fn apply_validated(
    input: &FrozenInput,
    config: &super::agent::Config,
    state: &mut super::agent::Checkpoint,
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    refuse_retired_outline_tool(name)?;
    match name {
        "put_outline_item" => put_outline_item(input, config, state, args),
        "omit_outline_item" => omit_outline_item(input, config, state, args),
        "put_chapter_template" => put_chapter_template(input, config, state, args),
        "skip_chapter_content" => skip_chapter_content(input, state, args),
        _ => Err(format!("unknown draft tool {name}")),
    }
}

/// Recompute provenance from saved obligations and the final tree, never titles.
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

fn organization_allowed(input: &FrozenInput, state: &super::agent::Checkpoint) -> bool {
    use super::outline_flow::{Phase, scan_complete};
    state.analysis.outline.phase == Phase::Outline
        || (state.analysis.outline.phase == Phase::Discover
            && !state.analysis.outline.checks.is_empty()
            && scan_complete(input, &state.analysis.outline))
}

fn put_outline_item(
    input: &FrozenInput,
    _config: &super::agent::Config,
    state: &mut super::agent::Checkpoint,
    args: &Value,
) -> Result<Value, String> {
    if !organization_allowed(input, state) {
        return Err("chapter organization requires completed discovery".into());
    }
    let title = args["title"].as_str().ok_or("title required")?.trim();
    if title.is_empty() {
        return Err("title required".into());
    }
    let order = args["order"].as_u64().ok_or("order required")? as usize;
    let prescribed = args["prescribed"].as_bool().ok_or("prescribed required")?;
    let parent = args["parent"].as_str().map(str::to_string);
    let grounds = parse_spans(&args["grounds"])?;
    if grounds.is_empty() && args["purpose"] != "group" {
        return Err("grounds required".into());
    }
    for span in &grounds {
        crate::analysis::tools::validate_span(input, state.coverage(), span)?;
    }
    let requirement_ids = required_json_array(args, "requirement_ids")?;
    let requirement_ids: Vec<String> =
        serde_json::from_value(requirement_ids).map_err(|e| e.to_string())?;
    if requirement_ids
        .iter()
        .any(|id| !state.analysis.outline.requirements.contains_key(id))
    {
        return Err("unknown submission requirement".into());
    }
    let purpose = required_json_field(args, "purpose")?;
    let purpose: ChapterPurpose = serde_json::from_value(purpose).map_err(|e| e.to_string())?;
    let format_refs = required_json_array(args, "format_refs")?;
    let format_refs: Vec<Span> = serde_json::from_value(format_refs).map_err(|e| e.to_string())?;
    for span in &format_refs {
        crate::analysis::tools::validate_span(input, state.coverage(), span)?;
    }
    let id = match args
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
    {
        Some(id) => id.to_string(),
        None => {
            state.analysis.outline.id_sequences.chapter += 1;
            format!("chapter-{}", state.analysis.outline.id_sequences.chapter)
        }
    };
    if sibling_order_taken(&state.analysis.draft_plan, parent.as_deref(), order, &id) {
        return Err(
            "sibling order must be unique under the same parent; keep the tender composition order"
                .into(),
        );
    }
    let composition_changed = parent.is_none();
    let volume_touch: Vec<String> = parent.iter().cloned().collect();
    let mut source_ids: Vec<String> = requirement_ids
        .iter()
        .flat_map(|id| {
            state.analysis.outline.requirements[id]
                .format_grounds
                .iter()
                .map(|span| span.source_id.clone())
        })
        .collect();
    source_ids.extend(format_refs.iter().map(|span| span.source_id.clone()));
    if source_ids.is_empty() {
        source_ids = grounds.iter().map(|span| span.source_id.clone()).collect();
    }
    let mut seen = BTreeSet::new();
    source_ids.retain(|id| seen.insert(id.clone()));
    let windows = vec![source_ids.clone()];
    upsert_plan(
        state,
        DraftPlanItem {
            grounds,
            requirement_ids,
            id: id.clone(),
            parent,
            order,
            title: title.into(),
            prescribed,
            source_ids,
            windows,
            window_index: 0,
            template_id: None,
            status: DraftStatus::Pending,
            purpose,
            format_refs,
            body_status: BodyStatus::Empty,
            omit_reason: None,
            preserved: vec![],
        },
    );
    super::outline_flow::invalidate_checks(state, composition_changed, &volume_touch);
    Ok(json!({"id":id,"saved":true}))
}

fn required_json_field(args: &Value, key: &str) -> Result<Value, String> {
    match args.get(key) {
        None => Err(format!("{key} required")),
        Some(Value::Null) => Err(format!("{key} must not be null")),
        Some(value) => Ok(value.clone()),
    }
}

fn required_json_array(args: &Value, key: &str) -> Result<Value, String> {
    let value = required_json_field(args, key)?;
    if !value.is_array() {
        return Err(format!("{key} must be an array"));
    }
    Ok(value)
}

fn omit_outline_item(
    input: &FrozenInput,
    _config: &super::agent::Config,
    state: &mut super::agent::Checkpoint,
    args: &Value,
) -> Result<Value, String> {
    let title = args["title"].as_str().ok_or("title required")?.to_string();
    let grounds = parse_spans(&args["grounds"])?;
    if grounds.is_empty() {
        return Err("grounds required".into());
    }
    for span in &grounds {
        crate::analysis::tools::validate_span(input, state.coverage(), span)?;
    }
    let reason = match args["reason"].as_str().unwrap_or("") {
        "bind_failed" => OmitReason::BindFailed,
        "not_applicable" => OmitReason::NotApplicable,
        "parse_failed" => OmitReason::ParseFailed,
        _ => return Err("invalid omit reason".into()),
    };
    let id = args["id"]
        .as_str()
        .map(str::to_string)
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| format!("outline-{}", state.analysis.draft_plan.len() + 1));
    let existing = state.analysis.draft_plan.iter().find(|item| item.id == id);
    let parent = existing.and_then(|item| item.parent.clone());
    let order = existing
        .map(|item| item.order)
        .unwrap_or_else(|| next_sibling_order(&state.analysis.draft_plan, None));
    upsert_plan(
        state,
        DraftPlanItem {
            grounds: vec![],
            requirement_ids: vec![],
            id: id.clone(),
            parent,
            order,
            title,
            prescribed: true,
            source_ids: vec![],
            windows: vec![],
            window_index: 0,
            template_id: None,
            status: DraftStatus::Omitted,
            purpose: ChapterPurpose::Response,
            format_refs: vec![],
            body_status: BodyStatus::Empty,
            omit_reason: Some(reason),
            preserved: vec![],
        },
    );
    Ok(json!({"id":id,"saved":true,"status":"omitted"}))
}

fn put_chapter_template(
    input: &FrozenInput,
    _config: &super::agent::Config,
    state: &mut super::agent::Checkpoint,
    args: &Value,
) -> Result<Value, String> {
    if state.pending_coverage.is_some() {
        return Err("new reads in this turn cannot be used until the next request".into());
    }
    let chapter_id = args["chapter_id"].as_str().ok_or("chapter_id required")?;
    require_active_chapter(state, chapter_id)?;
    let index = state
        .analysis
        .draft_plan
        .iter()
        .position(|item| item.id == chapter_id)
        .ok_or("unknown chapter")?;
    let window: Vec<String> = current_window(&state.analysis.draft_plan[index]).to_vec();
    let regions = parse_regions(&args["regions"])?;
    for region in &regions {
        crate::analysis::tools::validate_span(input, state.coverage(), &region.source)?;
        if !window.is_empty() && !window.contains(&region.source.source_id) {
            return Err("region source is outside the assigned window".into());
        }
    }
    reject_blank_erasing_source(input, &regions)?;
    validate_fill_regions(input, &regions)?;
    let title = args["title"].as_str().unwrap_or("").to_string();
    let purpose = args["purpose"].as_str().unwrap_or("").to_string();
    let existing_id = state.analysis.draft_plan[index].template_id.clone();
    let id = args["id"]
        .as_str()
        .map(str::to_string)
        .filter(|id| !id.is_empty())
        .or(existing_id.clone())
        .unwrap_or_else(|| format!("tpl-{chapter_id}"));
    let mut regions = regions;
    if let Some(prior) = existing_id
        .as_ref()
        .and_then(|id| state.analysis.records.get(id))
        && let RecordData::Template { regions: old, .. } = &prior.data
    {
        regions = merge_regions(old, regions);
    }
    let span = regions
        .first()
        .map(|region| region.source.clone())
        .ok_or("regions required")?;
    state.analysis.records.insert(
        id.clone(),
        Record {
            id: id.clone(),
            sources: vec![span.clone()],
            data: RecordData::Template {
                label: title.clone(),
                title,
                parent: None,
                order: Some(state.analysis.draft_plan[index].order),
                purpose,
                applicability: Applicability {
                    state: ApplicabilityState::Applicable,
                    scope: chapter_id.into(),
                    condition: "原文指定".into(),
                    grounds: vec![span],
                },
                regions,
            },
        },
    );
    let item = &mut state.analysis.draft_plan[index];
    item.template_id = Some(id.clone());
    let last = item.window_index + 1 >= item.windows.len().max(1);
    if last {
        item.status = DraftStatus::Filled;
    } else {
        item.window_index += 1;
        item.status = DraftStatus::Pending;
    }
    Ok(json!({"id":id,"saved":true,"status":item.status}))
}

fn skip_chapter_content(
    input: &FrozenInput,
    state: &mut super::agent::Checkpoint,
    args: &Value,
) -> Result<Value, String> {
    let chapter_id = args["chapter_id"].as_str().ok_or("chapter_id required")?;
    require_active_chapter(state, chapter_id)?;
    // A dropped chapter is a claim about the tender, so it carries the same
    // burden of proof as a written one: a reason the reader can check.
    if args["summary"].as_str().unwrap_or("").trim().is_empty() {
        return Err("omission needs a summary".into());
    }
    let grounds = args["grounds"].as_array().ok_or("omission needs grounds")?;
    if grounds.is_empty() {
        return Err("omission needs grounds".into());
    }
    for ground in grounds {
        let span: Span =
            serde_json::from_value(unwrap_citation(ground)).map_err(|error| error.to_string())?;
        crate::analysis::tools::validate_span(input, state.coverage(), &span)?;
    }
    let reason = match args["reason"].as_str().unwrap_or("") {
        "bind_failed" => OmitReason::BindFailed,
        "deadline" => OmitReason::Deadline,
        "not_applicable" => OmitReason::NotApplicable,
        "parse_failed" => OmitReason::ParseFailed,
        "blank_rule" => OmitReason::BlankRule,
        "window_exceeded" => OmitReason::WindowExceeded,
        _ => return Err("invalid omit reason".into()),
    };
    let item = state
        .analysis
        .draft_plan
        .iter_mut()
        .find(|item| item.id == chapter_id)
        .ok_or("unknown chapter")?;
    let reason = if reason == OmitReason::BindFailed && !item.source_ids.is_empty() {
        OmitReason::WindowExceeded
    } else {
        reason
    };
    item.status = DraftStatus::Pending;
    item.omit_reason = Some(reason);
    Ok(json!({"saved":true,"status":"pending","content_skipped":true}))
}

fn upsert_plan(state: &mut super::agent::Checkpoint, item: DraftPlanItem) {
    if let Some(existing) = state
        .analysis
        .draft_plan
        .iter_mut()
        .find(|old| old.id == item.id)
    {
        *existing = item;
    } else {
        state.analysis.draft_plan.push(item);
    }
}

fn sibling_order_taken(
    plan: &[DraftPlanItem],
    parent: Option<&str>,
    order: usize,
    except_id: &str,
) -> bool {
    plan.iter()
        .any(|item| item.id != except_id && item.parent.as_deref() == parent && item.order == order)
}

fn next_sibling_order(plan: &[DraftPlanItem], parent: Option<&str>) -> usize {
    plan.iter()
        .filter(|item| item.parent.as_deref() == parent)
        .map(|item| item.order)
        .max()
        .map(|order| order + 1)
        .unwrap_or(0)
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

fn require_active_chapter(
    state: &super::agent::Checkpoint,
    chapter_id: &str,
) -> Result<(), String> {
    let assigned = state.draft_active_id.as_deref().unwrap_or("");
    if assigned != chapter_id {
        return Err(format!(
            "chapter_id must equal assigned draft_active_id \"{assigned}\"; do not send the title"
        ));
    }
    Ok(())
}

fn unwrap_citation(value: &Value) -> Value {
    if value.get("source_id").is_none()
        && let Some(inner) = value
            .get("citation_ref")
            .or_else(|| value.get("citation_refs"))
    {
        return unwrap_citation(inner);
    }
    value.clone()
}

fn parse_span(value: &Value) -> Result<Span, String> {
    serde_json::from_value(unwrap_citation(value)).map_err(|e| e.to_string())
}

fn parse_spans(value: &Value) -> Result<Vec<Span>, String> {
    let Value::Array(items) = value else {
        return Err("grounds must be an array".into());
    };
    items.iter().map(parse_span).collect()
}

/// §7.1 的填章校验清单，补上 `validate_record` 在填章路径缺失的那几项。
///
/// 窗成员、读取收据、引文非空与「空白不吞源」由调用方先行校验（`validate_span`
/// 本来就拒零长文本区间）；这里管 grid header policy 与 grid 锚点的覆盖与不重
/// 叠——少了它们，带表格的章要么编译不出来，要么把用户看不见的单元格悄悄写坏。
fn validate_fill_regions(input: &FrozenInput, regions: &[TemplateRegion]) -> Result<(), String> {
    let mut assigned: BTreeMap<(String, usize, usize), ()> = BTreeMap::new();
    for region in regions {
        if region.role == RegionRole::Instruction && region.instruction.trim().is_empty() {
            return Err("instruction region needs the instruction text".into());
        }
        let Some(form_id) = region.form_id.as_deref() else {
            if !region.cells.is_empty() {
                return Err("only a grid region can select cells".into());
            }
            continue;
        };
        let form = input
            .structured_forms
            .iter()
            .find(|form| form["form_definition_revision_id"] == *form_id)
            .ok_or("grid region names a form outside the frozen input")?;
        let definition = &form["definition"];
        let rows = definition["row_count"]
            .as_u64()
            .ok_or("grid rows missing")? as usize;
        let columns = definition["column_count"]
            .as_u64()
            .ok_or("grid columns missing")? as usize;
        let header_rows = region
            .header_rows
            .ok_or("grid region must state its header policy")?;
        if header_rows >= rows {
            return Err("grid header policy covers the whole form".into());
        }
        if region.cells.is_empty() {
            return Err("grid region must select at least one cell".into());
        }
        // Anchors come from the same renderer the compiler uses, so a region can
        // never name a cell that the merged grid does not actually expose.
        let policies: Vec<_> = (0..columns)
            .map(|column| json!({"column":column,"role":"copy_verbatim"}))
            .collect();
        let table =
            crate::template_grid::table_block_from_grid(definition, &policies, header_rows)?;
        let crate::content_block::BlockContent::Table { cells, .. } = table else {
            return Err("grid expected".into());
        };
        let anchors: BTreeSet<_> = cells.iter().map(|cell| (cell.row, cell.column)).collect();
        for cell in &region.cells {
            if !anchors.contains(&(cell.row, cell.column)) {
                return Err("grid region selects a foreign or covered cell".into());
            }
            if assigned
                .insert((form_id.to_string(), cell.row, cell.column), ())
                .is_some()
            {
                return Err("grid regions assign the same cell twice".into());
            }
        }
    }
    Ok(())
}

fn parse_regions(value: &Value) -> Result<Vec<TemplateRegion>, String> {
    let Value::Array(items) = value else {
        return Err("regions must be an array".into());
    };
    items
        .iter()
        .map(|item| {
            let mut item = item.clone();
            if let Some(source) = item.get("source").cloned() {
                item["source"] = unwrap_citation(&source);
            }
            serde_json::from_value(item).map_err(|e| e.to_string())
        })
        .collect()
}
