//! 草稿通道：大纲与按章模板。不走 source_unit 抽取或独立 Rechecker。
use super::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const DRAFT_MAX_TURNS: usize = 80;
pub const DRAFT_OUTLINE_MAX_TURNS: usize = 5;
pub const DRAFT_WINDOW_CHARS: usize = 8000;
pub const DRAFT_DEADLINE_SECS: u64 = 20 * 60;
pub const OFFICIAL_DEADLINE_SECS: u64 = 45 * 60;
pub const DRAFT_MAX_ATTEMPTS: i32 = 2;

pub fn draft_claim_exhausted(draft_path: bool, attempt: i32) -> bool {
    draft_path && attempt > DRAFT_MAX_ATTEMPTS
}

pub fn analysis_deadline_secs(draft_path: bool) -> u64 {
    if draft_path {
        DRAFT_DEADLINE_SECS
    } else {
        OFFICIAL_DEADLINE_SECS
    }
}

pub fn draft_should_publish_partial(code: &str, _message: &str) -> bool {
    !matches!(
        code,
        "FROZEN_INPUT_DIGEST_MISMATCH" | "AGENT_OUTPUT_INVALID" | "AGENT_PROVIDER_UNAVAILABLE"
    )
}

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub omit_reason: Option<OmitReason>,
}

pub fn plan_ready(plan: &[DraftPlanItem]) -> bool {
    !plan.is_empty()
}

pub fn has_open_chapter(plan: &[DraftPlanItem]) -> bool {
    plan.iter().any(|item| item.status != DraftStatus::Omitted)
}

/// Large-file fill needs a real bid tree: every live volume has at least one
/// live child. Volume titles alone, or one expanded volume plus empty ones,
/// are not an outline.
pub fn outline_tree_ready(plan: &[DraftPlanItem]) -> bool {
    let live = |item: &DraftPlanItem| item.status != DraftStatus::Omitted;
    let roots: Vec<_> = plan
        .iter()
        .filter(|item| live(item) && item.parent.is_none())
        .collect();
    !roots.is_empty()
        && roots.iter().all(|root| {
            plan.iter().any(|item| {
                live(item) && item.parent.as_deref() == Some(root.id.as_str())
            })
        })
}

fn has_paren_group(text: &str) -> bool {
    (text.contains('(') && text.contains(')')) || (text.contains('（') && text.contains('）'))
}

fn concatenated_composition_title(title: &str) -> bool {
    let pauses = title.matches('、').count() + title.matches(", ").count();
    if pauses >= 2 {
        return true;
    }
    ["、", ", ", "及"].iter().any(|sep| {
        title.find(sep).is_some_and(|index| {
            has_paren_group(&title[..index]) && has_paren_group(&title[index + sep.len()..])
        })
    })
}

pub fn filled_templates_present(analysis: &Analysis) -> bool {
    analysis.draft_plan.iter().all(|item| match item.status {
        DraftStatus::Filled => item
            .template_id
            .as_ref()
            .is_some_and(|id| analysis.records.contains_key(id)),
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

fn numbered_heading_prefix(title: &str) -> Option<String> {
    let title = title.trim();
    let mut chars = title.chars().peekable();
    let mut prefix = String::new();
    if !chars.peek().is_some_and(|ch| ch.is_ascii_digit()) {
        return None;
    }
    while let Some(ch) = chars.peek().copied() {
        if ch.is_ascii_digit() {
            prefix.push(ch);
            chars.next();
            continue;
        }
        if matches!(ch, '.' | '．') && chars.clone().nth(1).is_some_and(|n| n.is_ascii_digit()) {
            prefix.push('.');
            chars.next();
            continue;
        }
        break;
    }
    (!prefix.is_empty() && prefix.chars().any(|ch| ch.is_ascii_digit())).then_some(prefix)
}

fn outline_child_fits_parent(parent_title: &str, child_title: &str) -> bool {
    let parent_num = numbered_heading_prefix(parent_title);
    let child_num = numbered_heading_prefix(child_title);
    if let (Some(parent_num), Some(child_num)) = (&parent_num, &child_num) {
        return child_num == parent_num
            || child_num.starts_with(&format!("{parent_num}."));
    }
    if parent_num.is_some() && child_num.is_none() {
        let core = title_core(parent_title);
        return folded_contains(child_title, core) || folded_contains(core, title_core(child_title));
    }
    true
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

fn sibling_density(text: &str, titles: &[String]) -> usize {
    titles
        .iter()
        .filter(|title| folded_contains(text, title) || folded_contains(text, title_core(title)))
        .count()
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
    let mut hits: Vec<String> = input
        .source_units
        .iter()
        .filter(|source| source_matches_terms(input, source, &terms))
        .map(|source| source.source_unit_revision_id.clone())
        .collect();
    if hits.is_empty() {
        return Err(OmitReason::BindFailed);
    }
    hits.sort();
    hits.dedup();
    Ok(hits)
}

fn prefer_format_sources(
    input: &FrozenInput,
    source_ids: &[String],
    titles: &[String],
) -> Vec<String> {
    let mut scored: Vec<(usize, usize, String)> = source_ids
        .iter()
        .filter_map(|id| {
            let source = input
                .source_units
                .iter()
                .find(|source| source.source_unit_revision_id == *id)?;
            Some((
                sibling_density(&source.text, titles),
                source.ordinal,
                id.clone(),
            ))
        })
        .collect();
    if scored.is_empty() {
        return source_ids.to_vec();
    }
    scored.sort_by_key(|(density, ordinal, _)| (*density, *ordinal));
    let best = scored[0].0;
    scored
        .into_iter()
        .filter(|(density, _, _)| *density == best)
        .map(|(_, _, id)| id)
        .collect()
}

fn specialize_pending_windows(input: &FrozenInput, plan: &mut [DraftPlanItem]) {
    let titles: Vec<String> = plan
        .iter()
        .filter(|item| item.status != DraftStatus::Omitted)
        .map(|item| item.title.clone())
        .collect();
    if titles.len() < 2 {
        return;
    }
    for item in plan.iter_mut() {
        if item.status != DraftStatus::Pending {
            continue;
        }
        let Ok(hits) = bind_source_ids(input, &item.title, &[]) else {
            continue;
        };
        let ranked = prefer_format_sources(input, &hits, &titles);
        if ranked.is_empty() {
            continue;
        }
        item.source_ids = ranked;
        item.windows = split_windows(input, &item.source_ids);
        item.window_index = 0;
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

pub fn split_windows(input: &FrozenInput, source_ids: &[String]) -> Vec<Vec<String>> {
    let mut windows = Vec::new();
    let mut current = Vec::new();
    let mut chars = 0usize;
    for id in source_ids {
        let extra = input
            .source_units
            .iter()
            .find(|source| source.source_unit_revision_id == *id)
            .map(|source| source.text.len())
            .unwrap_or(0);
        if !current.is_empty() && extra > 0 && chars.saturating_add(extra) > DRAFT_WINDOW_CHARS {
            windows.push(std::mem::take(&mut current));
            chars = 0;
        }
        current.push(id.clone());
        chars = chars.saturating_add(extra);
    }
    if !current.is_empty() {
        windows.push(current);
    }
    if windows.is_empty() {
        windows.push(Vec::new());
    }
    windows
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

pub fn omit_pending_deadline(plan: &mut [DraftPlanItem]) {
    for item in plan {
        if item.status == DraftStatus::Pending {
            item.status = DraftStatus::Omitted;
            item.omit_reason = Some(OmitReason::Deadline);
        }
    }
}

pub fn small_file(input: &FrozenInput) -> bool {
    input
        .source_units
        .iter()
        .map(|source| source.text.len())
        .sum::<usize>()
        <= DRAFT_WINDOW_CHARS
}

pub fn outline_schemas() -> Vec<Value> {
    serde_json::from_str(include_str!(
        "../../schemas/tender-draft-outline-tools-v1.schema.json"
    ))
    .expect("draft outline schemas")
}

pub fn fill_schemas() -> Vec<Value> {
    serde_json::from_str(include_str!(
        "../../schemas/tender-draft-fill-tools-v1.schema.json"
    ))
    .expect("draft fill schemas")
}

pub fn read_schemas() -> Vec<Value> {
    let tools: Vec<Value> = serde_json::from_str(include_str!(
        "../../schemas/tender-analysis-tools-v1.schema.json"
    ))
    .expect("analysis tools");
    tools
        .into_iter()
        .filter(|tool| {
            matches!(
                tool["function"]["name"].as_str(),
                Some("read_source" | "read_form" | "search_sources" | "read_source_view")
            )
        })
        .collect()
}

pub fn outline_schemas_for(input: &FrozenInput) -> Vec<Value> {
    let mut tools = outline_schemas();
    if !small_file(input) {
        tools.extend(read_schemas());
    }
    tools
}

pub fn fill_schemas_for(input: &FrozenInput) -> Vec<Value> {
    let mut tools = fill_schemas();
    if !small_file(input) {
        tools.extend(read_schemas());
    }
    tools
}

pub fn apply(
    input: &FrozenInput,
    config: &super::agent::Config,
    state: &mut super::agent::Checkpoint,
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    match name {
        "put_outline_item" => put_outline_item(input, config, state, args),
        "omit_outline_item" => omit_outline_item(input, config, state, args),
        "put_chapter_template" => put_chapter_template(input, config, state, args),
        "put_chapter_omission" => put_chapter_omission(state, args),
        _ => Err(format!("unknown draft tool {name}")),
    }
}

fn put_outline_item(
    input: &FrozenInput,
    config: &super::agent::Config,
    state: &mut super::agent::Checkpoint,
    args: &Value,
) -> Result<Value, String> {
    let title = args["title"].as_str().ok_or("title required")?.trim();
    if title.is_empty() {
        return Err("title required".into());
    }
    if concatenated_composition_title(title) {
        return Err(
            "one outline item cannot list multiple composition items; put each listed item as its own chapter"
                .into(),
        );
    }
    let order = args["order"].as_u64().ok_or("order required")? as usize;
    let prescribed = args["prescribed"].as_bool().ok_or("prescribed required")?;
    let parent = args["parent"].as_str().map(str::to_string);
    let grounds = parse_spans(&args["grounds"])?;
    if grounds.is_empty() {
        return Err("grounds required".into());
    }
    for span in &grounds {
        crate::tender_analysis::tools::validate_span(input, state.coverage(), span)?;
    }
    let id = args["id"]
        .as_str()
        .map(str::to_string)
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| format!("outline-{}", state.analysis.draft_plan.len() + 1));
    if sibling_order_taken(&state.analysis.draft_plan, parent.as_deref(), order, &id) {
        return Err(
            "sibling order must be unique under the same parent; keep the tender composition order"
                .into(),
        );
    }
    if let Some(parent_id) = parent.as_deref() {
        let parent_title = state
            .analysis
            .draft_plan
            .iter()
            .find(|item| item.id == parent_id)
            .map(|item| item.title.as_str())
            .ok_or("parent outline item missing")?;
        if !outline_child_fits_parent(parent_title, title) {
            return Err(
                "child title must belong under that parent: numbered headings keep descendant numbers, and technical chapter children cannot be unrelated exhibits"
                    .into(),
            );
        }
    }
    let source_ids = match bind_source_ids(input, title, &config.limits.draft_bind_terms) {
        Ok(ids) => ids,
        Err(reason) => {
            upsert_plan(
                state,
                DraftPlanItem {
                    id: id.clone(),
                    parent,
                    order,
                    title: title.into(),
                    prescribed,
                    source_ids: vec![],
                    windows: vec![],
                    window_index: 0,
                    template_id: None,
                    status: DraftStatus::Omitted,
                    omit_reason: Some(reason),
                },
            );
            return Ok(json!({"id":id,"saved":true,"status":"omitted"}));
        }
    };
    let windows = split_windows(input, &source_ids);
    upsert_plan(
        state,
        DraftPlanItem {
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
            omit_reason: None,
        },
    );
    Ok(json!({"id":id,"saved":true}))
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
        crate::tender_analysis::tools::validate_span(input, state.coverage(), span)?;
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
    let existing = state
        .analysis
        .draft_plan
        .iter()
        .find(|item| item.id == id);
    let parent = existing.and_then(|item| item.parent.clone());
    let order = existing.map(|item| item.order).unwrap_or_else(|| {
        next_sibling_order(&state.analysis.draft_plan, None)
    });
    upsert_plan(
        state,
        DraftPlanItem {
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
            omit_reason: Some(reason),
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
        crate::tender_analysis::tools::validate_span(input, state.coverage(), &region.source)?;
        if !window.is_empty() && !window.contains(&region.source.source_id) {
            return Err("region source is outside the assigned window".into());
        }
    }
    reject_blank_erasing_source(input, &regions)?;
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

fn put_chapter_omission(
    state: &mut super::agent::Checkpoint,
    args: &Value,
) -> Result<Value, String> {
    let chapter_id = args["chapter_id"].as_str().ok_or("chapter_id required")?;
    require_active_chapter(state, chapter_id)?;
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
    item.status = DraftStatus::Omitted;
    item.omit_reason = Some(reason);
    Ok(json!({"saved":true,"status":"omitted"}))
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

fn has_plan_children(plan: &[DraftPlanItem], id: &str) -> bool {
    plan.iter()
        .any(|item| item.parent.as_deref() == Some(id))
}

fn sibling_order_taken(
    plan: &[DraftPlanItem],
    parent: Option<&str>,
    order: usize,
    except_id: &str,
) -> bool {
    plan.iter().any(|item| {
        item.id != except_id && item.parent.as_deref() == parent && item.order == order
    })
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
        .filter(|item| item.status == DraftStatus::Pending)
        .filter(|item| !has_plan_children(&state.analysis.draft_plan, &item.id))
        .filter(|item| {
            let parent_ok = item.parent.as_ref().is_some_and(|parent| {
                state
                    .analysis
                    .draft_plan
                    .iter()
                    .any(|node| node.id == *parent)
            });
            if small_file(input) {
                parent_ok || item.parent.is_none()
            } else {
                parent_ok
            }
        })
        .min_by_key(|item| (if item.parent.is_some() { 0 } else { 1 }, item.order))
        .map(|item| item.id.clone());
    let Some(id) = next else {
        state.draft_active_id = None;
        return false;
    };
    state.draft_active_id = Some(id.clone());
    state.main_progress.watch = Default::default();
    if let Some(item) = state.analysis.draft_plan.iter().find(|item| item.id == id) {
        let window = current_window(item).to_vec();
        mark_window_coverage(input, &mut state.analysis.coverage, &window);
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
    analysis_mutated: bool,
    batch_failed: bool,
) -> Result<(), String> {
    if batch_failed {
        return Ok(());
    }
    match state.draft_stage {
        DraftStage::None | DraftStage::Outline => {
            if state.draft_stage == DraftStage::None {
                state.draft_stage = DraftStage::Outline;
                preload_outline_window(input, state);
            }
            let live = state
                .analysis
                .draft_plan
                .iter()
                .filter(|item| item.status != DraftStatus::Omitted)
                .count();
            if plan_ready(&state.analysis.draft_plan)
                && (small_file(input) || outline_tree_ready(&state.analysis.draft_plan))
                && (small_file(input)
                    || !analysis_mutated
                    || (state.turn + 1 >= DRAFT_OUTLINE_MAX_TURNS && live >= 2))
            {
                state.draft_stage = DraftStage::Fill;
                if !assign_next_chapter(input, state) {
                    state.draft_stage = DraftStage::Published;
                    state.done = true;
                }
            }
        }
        DraftStage::Fill => {
            let active_done = state.draft_active_id.as_ref().is_none_or(|id| {
                state.analysis.draft_plan.iter().any(|item| {
                    item.id == *id
                        && matches!(item.status, DraftStatus::Filled | DraftStatus::Omitted)
                })
            });
            if active_done && !assign_next_chapter(input, state) {
                state.draft_stage = DraftStage::Published;
                state.done = true;
            }
        }
        DraftStage::Published => state.done = true,
    }
    Ok(())
}

pub fn preload_outline_window(input: &FrozenInput, state: &mut super::agent::Checkpoint) {
    let ids: Vec<String> = input
        .source_units
        .iter()
        .map(|source| source.source_unit_revision_id.clone())
        .collect();
    if small_file(input) {
        mark_window_coverage(input, &mut state.analysis.coverage, &ids);
    }
    state.main_work = Some(super::agent::WorkState {
        source_scope: ids,
        deferred_sources: vec![],
        objective: if small_file(input) {
            "根据已预装原文写出投标文件大纲".into()
        } else {
            "检索冻结原文，写出投标文件组成大纲".into()
        },
        focus: Default::default(),
        output_refs: vec![],
        pending_refs: vec![],
        status: super::agent::context::WorkStatus::Active,
        note: String::new(),
    });
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
