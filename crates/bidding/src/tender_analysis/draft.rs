//! 草稿通道：大纲与按章模板。不走 source_unit 抽取或独立 Rechecker。
use super::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// 阶段一的**兜底**回合上限，防跑飞用，不是验收门。真正决定阶段一多快的是按窗数
/// 推导的帽（`outline_turn_cap`，通常十来回合）；这里留够余量，宁可让难文档多跑几
/// 轮把大纲写全，也不为凑一个数字把它砍成半张大纲。整体填充是另一次请求、另一套
/// 账（`fill_turn_cap`），不共用这个上限。
pub const OUTLINE_MAX_TURNS: usize = 80;
/// 阶段一的**目标**回合数：出骨架应该在这个量级内完成。超了不失败、不截断，只是
/// 说明该优化投递与绑定，让 `outline_turn_cap` 更早闭合。
pub const OUTLINE_TURN_TARGET: usize = 20;
/// 每章的回合额度：读表 / 写模板 / 一次返工。
pub const FILL_TURNS_PER_CHAPTER: usize = 4;
/// 一次整体填充的回合上限。章多也不许无限跑；到点按已填章出稿。
pub const FILL_MAX_TURNS: usize = 200;
/// 一窗的**字节**上限（不是字数：中文约 3 字节/字）。
pub const DRAFT_WINDOW_BYTES: usize = 8000;
/// 历史 8 窗上限已删除；保留常数仅作回归对照，不再截断扫描。
pub const OUTLINE_MAX_WINDOWS: usize = 8;
/// 一个标题最多绑这么多来源，其余留给定向补读。
pub const BIND_MAX_SOURCES: usize = 8;
/// 连续这么多轮修补都没减少缺口就结束自动执行，不空转、不出部分骨架。
pub const OUTLINE_MAX_STALLED_ROUNDS: usize = 2;
/// 阶段一墙钟保护额度（非完整性定义）；到点保持未完成，不编译部分骨架。
pub const DRAFT_DEADLINE_SECS: u64 = 46 * 60;
/// 阶段一的墙钟**目标**：正常文档应该几分钟出骨架。
pub const OUTLINE_DEADLINE_TARGET_SECS: u64 = 5 * 60;
pub const OFFICIAL_DEADLINE_SECS: u64 = 45 * 60;
pub const DRAFT_MAX_ATTEMPTS: i32 = 3;
/// 整体填充的**写作窗**：到点停止派章，把已填的章编译出稿。
pub const FILL_DEADLINE_SECS: u64 = 40 * 60;
/// 整体填充的**信封窗**：写作窗之后还留 5 分钟给编译、登记与入稿，且必须短于
/// SQL 的 46 分钟硬租约（SQL 只作兜底，不在那边再调一遍）。
pub const FILL_ENVELOPE_DEADLINE_SECS: u64 = 45 * 60;

// 写作窗必须短于信封窗，信封窗必须短于 SQL 硬租约。
const _: () = assert!(
    OUTLINE_MAX_TURNS > OUTLINE_TURN_TARGET,
    "兜底上限必须宽于目标：时间是要压的目标，不是砍掉大纲的理由"
);
const _: () = assert!(
    FILL_DEADLINE_SECS < FILL_ENVELOPE_DEADLINE_SECS && FILL_ENVELOPE_DEADLINE_SECS < 46 * 60,
    "写作窗 < 信封窗 < SQL 硬租约，到期才有时间把已填的章编译出稿"
);
/// Default compile ceiling for the draft channel. The agent and the publication
/// path must read one configured value or the agent stages bytes the
/// publication rejects. A whole filled bid can outgrow this, so the ceiling is
/// configurable per stage (`Limits::max_draft_docx_bytes`) and an over-budget
/// document degrades to fewer bodies instead of failing the run.
pub const DRAFT_MAX_DOCX_BYTES: usize = 2_000_000;
/// 整份填满的投标文件远大于骨架，填章请求按这个上限编译；仍超限就降级出稿。
pub const FILL_MAX_DOCX_BYTES: usize = 8_000_000;

pub fn default_draft_docx_bytes() -> usize {
    DRAFT_MAX_DOCX_BYTES
}

/// Compile result. `degraded` stays empty: over-budget compile fails instead of
/// dropping filled bodies.
pub struct DraftCompile {
    pub compiled: crate::docx_composition::compiler::Compiled,
    pub degraded: Vec<String>,
}

/// Compile the current draft plan. Over-budget compilation fails without dropping
/// filled bodies or user-preserved content; callers must raise the budget or stop.
pub fn compile_draft(
    input: &FrozenInput,
    result: &super::AnalysisResult,
    max_docx_bytes: usize,
) -> Result<DraftCompile, String> {
    let composed = crate::docx_composition::synthesize_draft_document(input, result)?;
    let compiled =
        crate::docx_composition::compiler::compile(input, result, &composed, max_docx_bytes)?;
    Ok(DraftCompile {
        compiled,
        degraded: vec![],
    })
}

pub fn draft_claim_exhausted(draft_path: bool, attempt: i32) -> bool {
    draft_path && attempt > DRAFT_MAX_ATTEMPTS
}

/// 整体填充按**待填章数**记账，与阶段一的 20 回合门无关：一份 40 章的投标文件
/// 不可能在 20 回合里写完，而 3 章的补填也不该拿到 200 回合。
pub fn fill_turn_cap(pending_chapters: usize) -> usize {
    pending_chapters
        .saturating_mul(FILL_TURNS_PER_CHAPTER)
        .saturating_add(4)
        .clamp(FILL_TURNS_PER_CHAPTER + 4, FILL_MAX_TURNS)
}

/// 把运行期额度换成这次填充的额度并冻进请求。工具调用与读字节随回合等比放开，
/// 否则回合还没用完就先撞上工具帽；编译上限按整份正文放大（骨架用小值）。
pub fn fill_limits(
    mut limits: super::agent::Limits,
    pending_chapters: usize,
) -> super::agent::Limits {
    let turns = fill_turn_cap(pending_chapters);
    limits.max_turns = turns;
    limits.max_tool_calls = limits.max_tool_calls.max(turns * 3);
    limits.max_read_bytes = limits.max_read_bytes.max(turns * DRAFT_WINDOW_BYTES * 4);
    limits.max_draft_docx_bytes = limits.max_draft_docx_bytes.max(FILL_MAX_DOCX_BYTES);
    limits
}

pub fn analysis_deadline_secs(draft_path: bool) -> u64 {
    if draft_path {
        DRAFT_DEADLINE_SECS
    } else {
        OFFICIAL_DEADLINE_SECS
    }
}

/// Seconds until a frozen absolute deadline. `None` is the first claim, which
/// still uses the initial budget; a later claim must pass the same deadline.
pub fn handler_budget_secs(
    deadline_unix: Option<i64>,
    now_unix: i64,
    first_claim_secs: u64,
) -> u64 {
    match deadline_unix {
        None => first_claim_secs,
        Some(deadline) => (deadline - now_unix).max(0) as u64,
    }
}

pub const PUBLISH_RESERVE_SECS: u64 = 300;

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

/// 大纲阶段的回合帽：索引 + 逐窗投递 + 修补轮，按窗数推导。
pub fn outline_turn_cap(input: &FrozenInput) -> usize {
    outline_chunks(input)
        .len()
        .saturating_mul(4)
        .saturating_add(12)
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
/// 命中按优先级取前 `BIND_MAX_SOURCES` 个：全部命中都绑上等于让一章拖着全书正文，
/// 剩下的留给定向补读。
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
    hits.truncate(BIND_MAX_SOURCES);
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
            item.windows = split_windows(input, &item.source_ids);
            item.omit_reason = None;
            continue;
        }
        item.windows.clear();
        item.window_index = 0;
        match bind_source_ids(input, &item.title, &[]) {
            Ok(hits) => {
                item.source_ids = hits;
                item.windows = split_windows(input, &item.source_ids);
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

pub fn split_windows(input: &FrozenInput, source_ids: &[String]) -> Vec<Vec<String>> {
    let mut windows = Vec::new();
    let mut current = Vec::new();
    let mut bytes = 0usize;
    let mut document: Option<String> = None;
    for id in source_ids {
        let source = input
            .source_units
            .iter()
            .find(|source| source.source_unit_revision_id == *id);
        let extra = source.map(|source| source.text.len()).unwrap_or(0);
        let owner = source.map(|source| source.document_id.clone());
        // 跨文档不混窗；同一文档内按顺序累加到窗字节上限。
        let split = !current.is_empty()
            && (owner != document
                || (extra > 0 && bytes.saturating_add(extra) > DRAFT_WINDOW_BYTES));
        if split {
            windows.push(std::mem::take(&mut current));
            bytes = 0;
        }
        document = owner;
        current.push(id.clone());
        bytes = bytes.saturating_add(extra);
    }
    if !current.is_empty() {
        windows.push(current);
    }
    if windows.is_empty() {
        windows.push(Vec::new());
    }
    windows
}

/// 大纲阶段的窗序列：全部有效来源按原文顺序分窗，不截断尾部。
pub fn outline_windows(input: &FrozenInput) -> Vec<Vec<String>> {
    let ids = input
        .source_units
        .iter()
        .map(|source| source.source_unit_revision_id.clone())
        .collect::<Vec<_>>();
    split_windows(input, &ids)
}

/// 续读重叠只作上下文，不进入扫描记账区间。
pub const OUTLINE_RANGE_OVERLAP: usize = 256;
pub const OUTLINE_FORM_BODY_ROWS: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct OutlineChunk {
    pub source_id: String,
    pub kind: String,
    pub form_id: String,
    pub account_start: usize,
    pub account_end: usize,
    pub deliver_start: usize,
    pub header_cells: usize,
}

/// 不相交的 UTF-8 记账区间。切点落在字符边界上。
pub fn utf8_account_ranges(text: &str, window: usize) -> Vec<(usize, usize)> {
    if text.is_empty() {
        return Vec::new();
    }
    let window = window.max(1);
    let mut ranges = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut end = (start + window).min(text.len());
        while end > start && !text.is_char_boundary(end) {
            end -= 1;
        }
        if end == start {
            end = text[start..]
                .chars()
                .next()
                .map(|ch| start + ch.len_utf8())
                .unwrap_or(text.len());
        }
        ranges.push((start, end));
        if end >= text.len() {
            break;
        }
        start = end;
    }
    ranges
}

fn delivery_start(text: &str, account_start: usize) -> usize {
    if account_start == 0 {
        return 0;
    }
    let mut start = account_start.saturating_sub(OUTLINE_RANGE_OVERLAP);
    while start < account_start && !text.is_char_boundary(start) {
        start += 1;
    }
    start
}

/// 表头单元格不重复记账；续段的 `header_cells` 是投递时必须带上的前缀。
pub fn form_account_slices(
    definition: &serde_json::Value,
    body_rows: usize,
) -> Vec<(usize, usize, usize)> {
    let Some((rows, columns)) = super::relations::form_dimensions(definition) else {
        return Vec::new();
    };
    if columns == 0 || rows == 0 {
        return Vec::new();
    }
    let total = rows * columns;
    let header = if rows > 1 { columns } else { 0 };
    if header >= total {
        return vec![(0, total, 0)];
    }
    let step = columns.saturating_mul(body_rows.max(1)).max(columns);
    let mut start = 0;
    let mut slices = Vec::new();
    while start < total {
        let end = (start + step + if start == 0 { header } else { 0 }).min(total);
        slices.push((start, end, header));
        start = end;
    }
    slices
}

pub fn outline_chunks(input: &FrozenInput) -> Vec<OutlineChunk> {
    let mut chunks = Vec::new();
    for source in &input.source_units {
        if source.text.is_empty() {
            chunks.push(OutlineChunk {
                source_id: source.source_unit_revision_id.clone(),
                kind: "empty".into(),
                form_id: String::new(),
                account_start: 0,
                account_end: 0,
                deliver_start: 0,
                header_cells: 0,
            });
        }
        for (start, end) in utf8_account_ranges(&source.text, DRAFT_WINDOW_BYTES) {
            chunks.push(OutlineChunk {
                source_id: source.source_unit_revision_id.clone(),
                kind: "text".into(),
                form_id: String::new(),
                account_start: start,
                account_end: end,
                deliver_start: delivery_start(&source.text, start),
                header_cells: 0,
            });
        }
        for form in input
            .structured_forms
            .iter()
            .filter(|form| form["source_unit_revision_id"] == source.source_unit_revision_id)
        {
            let Some(form_id) = form["form_definition_revision_id"].as_str() else {
                continue;
            };
            for (start, end, header) in
                form_account_slices(&form["definition"], OUTLINE_FORM_BODY_ROWS)
            {
                chunks.push(OutlineChunk {
                    source_id: source.source_unit_revision_id.clone(),
                    kind: "form".into(),
                    form_id: form_id.into(),
                    account_start: start,
                    account_end: end,
                    deliver_start: if header > 0 { 0 } else { start },
                    header_cells: header,
                });
            }
        }
    }
    for (kind, values) in [
        ("documents", &input.documents),
        ("document_relations", &input.document_relations),
        ("decisions", &input.decisions),
    ] {
        for index in 0..values.len() {
            chunks.push(OutlineChunk {
                source_id: kind.into(),
                kind: "metadata".into(),
                form_id: String::new(),
                account_start: index,
                account_end: index + 1,
                deliver_start: index,
                header_cells: 0,
            });
        }
    }
    chunks
}

fn chunk_scanned(
    input: &FrozenInput,
    outline: &super::outline_flow::OutlineState,
    chunk: &OutlineChunk,
) -> bool {
    let _ = input;
    if chunk.kind == "metadata" {
        super::tools::contains(
            outline.scanned.metadata.get(&chunk.source_id),
            chunk.account_start,
            chunk.account_end,
        )
    } else if chunk.kind == "empty" {
        outline.scanned.metadata.contains_key(&chunk.source_id)
    } else if chunk.kind == "form" {
        super::tools::contains(
            outline.scanned.form_cells.get(&chunk.form_id),
            chunk.account_start,
            chunk.account_end,
        )
    } else {
        super::tools::contains(
            outline.scanned.text.get(&chunk.source_id),
            chunk.account_start,
            chunk.account_end,
        )
    }
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

/// 大纲合同：写工具 + 常驻读工具。全书装得进一窗时也是同一套工具，只是窗数为 1。
pub fn outline_schemas() -> Vec<Value> {
    let mut tools: Vec<Value> = serde_json::from_str(include_str!(
        "../../schemas/tender-draft-outline-tools-v1.schema.json"
    ))
    .expect("draft outline schemas");
    tools.extend(read_schemas());
    tools.extend(super::outline_flow::schemas());
    tools
}

/// 填章合同：填章工具 + 常驻读工具。
pub fn fill_schemas() -> Vec<Value> {
    let mut tools: Vec<Value> = serde_json::from_str(include_str!(
        "../../schemas/tender-draft-fill-tools-v1.schema.json"
    ))
    .expect("draft fill schemas");
    tools.extend(read_schemas());
    tools
}

/// Bounded navigation and search over the complete frozen collection.
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
                Some(
                    "collection_index"
                        | "search_sources"
                        | "source_index"
                        | "read_source"
                        | "read_form"
                        | "read_form_cell"
                        | "read_source_view"
                )
            )
        })
        .collect()
}

pub fn apply(
    input: &FrozenInput,
    config: &super::agent::Config,
    state: &mut super::agent::Checkpoint,
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    if name == "put_outline_items" {
        return super::outline_flow::apply_chapter_batch(input, config, state, args);
    }
    apply_validated(input, config, state, name, args)
}

pub(super) fn apply_validated(
    input: &FrozenInput,
    config: &super::agent::Config,
    state: &mut super::agent::Checkpoint,
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    if matches!(
        name,
        "submit_outline_scan"
            | "read_outline"
            | "read_outline_fragment"
            | "submit_outline_check"
            | "assign_outline_fragments"
            | "finish_outline"
    ) {
        return super::outline_flow::apply(
            input,
            state,
            name,
            args,
            config.limits.max_tool_result_bytes,
        );
    }
    if name == "put_outline_items" {
        if !organization_allowed(input, state) {
            return Err("chapter organization requires completed discovery".into());
        }
        let items = args["items"].as_array().ok_or("items required")?;
        let mut next = state.clone();
        let mut items = items.clone();
        for (index, item) in items.iter_mut().enumerate() {
            if item.get("grounds").is_some() || item.get("format_refs").is_some() {
                return Err(format!(
                    "items[{index}]: grounds and format_refs are host-derived; supply requirement_ids only"
                ));
            }
            let ids: Vec<String> = serde_json::from_value(item["requirement_ids"].clone())
                .map_err(|error| format!("items[{index}].requirement_ids: {error}"))?;
            let purpose: ChapterPurpose = serde_json::from_value(item["purpose"].clone())
                .map_err(|error| format!("items[{index}].purpose: {error}"))?;
            let mut grounds = Vec::new();
            let mut formats = Vec::new();
            for id in &ids {
                let need = state
                    .analysis
                    .outline
                    .requirements
                    .get(id)
                    .ok_or_else(|| format!("items[{index}].requirement_ids: unknown {id}"))?;
                let valid = match purpose {
                    ChapterPurpose::Response => need.needs_chapter(),
                    ChapterPurpose::Group => {
                        need.kind == super::outline_flow::NeedKind::StructureConstraint
                            && need.applicability
                                != super::outline_flow::Applicability::NotApplicable
                    }
                };
                if !valid {
                    return Err(format!(
                        "items[{index}].requirement_ids: {id} does not belong on a {purpose:?} node"
                    ));
                }
                grounds.extend(need.grounds.clone());
                formats.extend(need.format_grounds.clone());
            }
            if purpose == ChapterPurpose::Response && ids.is_empty() {
                return Err(format!(
                    "items[{index}].requirement_ids: response material requires saved obligations"
                ));
            }
            item["grounds"] = json!(grounds);
            item["format_refs"] = json!(formats);
        }
        let mut id_map = std::collections::BTreeMap::new();
        let mut seen_ids = BTreeSet::new();
        for (index, item) in items.iter_mut().enumerate() {
            let supplied = item["id"]
                .as_str()
                .filter(|id| !id.is_empty())
                .map(str::to_owned);
            if let Some(id) = &supplied
                && !seen_ids.insert(id.clone())
            {
                return Err(format!("items[{index}].id: duplicate chapter ID {id}"));
            }
            if supplied
                .as_ref()
                .is_none_or(|id| !state.analysis.draft_plan.iter().any(|node| &node.id == id))
            {
                if !supplied
                    .as_ref()
                    .is_some_and(|id| id.starts_with("tmp-") && id.len() > 4)
                {
                    return Err(format!(
                        "items[{index}].id: unknown chapter {:?}; new chapters require tmp- IDs",
                        supplied
                    ));
                }
                let id = loop {
                    next.analysis.outline.id_sequences.chapter += 1;
                    let id = format!("chapter-{}", next.analysis.outline.id_sequences.chapter);
                    if !state.analysis.draft_plan.iter().any(|node| node.id == id) {
                        break id;
                    }
                };
                if let Some(alias) = supplied {
                    id_map.insert(alias, id.clone());
                }
                item["id"] = json!(id);
            }
        }
        for item in &mut items {
            if let Some(parent) = item["parent"].as_str().and_then(|id| id_map.get(id)) {
                item["parent"] = json!(parent);
            }
        }
        let removed: Vec<String> =
            serde_json::from_value(args["remove_ids"].clone()).map_err(|e| e.to_string())?;
        if items
            .iter()
            .any(|item| removed.iter().any(|id| item["id"] == *id))
        {
            return Err("cannot update and remove the same chapter".into());
        }
        // Remove replaced/deleted nodes before checking sibling order; validate the final tree below.
        next.analysis.draft_plan.retain(|node| {
            !removed.contains(&node.id) && !items.iter().any(|item| item["id"] == node.id)
        });
        let mut saved = Vec::new();
        let mut composition_changed = false;
        let mut volume_touch = BTreeSet::new();
        for (index, item) in items.iter().enumerate() {
            let before_parent = item.get("id").and_then(Value::as_str).and_then(|id| {
                state
                    .analysis
                    .draft_plan
                    .iter()
                    .find(|node| node.id == id)
                    .and_then(|node| node.parent.clone())
            });
            let parent = item
                .get("parent")
                .and_then(Value::as_str)
                .map(str::to_string);
            if parent.is_none() || before_parent.is_none() {
                composition_changed = true;
            }
            if let Some(volume) = parent.clone().or(before_parent) {
                volume_touch.insert(volume);
            }
            saved.push(
                put_outline_item(input, config, &mut next, item)
                    .map_err(|error| format!("items[{index}] ({}): {error}", item["id"]))?,
            );
        }
        if !removed.is_empty() {
            composition_changed = true;
            for id in &removed {
                if let Some(parent) = next
                    .analysis
                    .draft_plan
                    .iter()
                    .find(|node| node.id == *id)
                    .and_then(|node| node.parent.clone())
                {
                    volume_touch.insert(parent);
                }
            }
        }
        next.analysis
            .draft_plan
            .retain(|node| !removed.contains(&node.id));
        super::outline_flow::tree_valid(&next.analysis.draft_plan)?;
        refresh_outline_basis(input, &mut next)?;
        let unmapped = next.analysis.outline.requirements.iter().any(|(id, need)| {
            need.needs_chapter()
                && state.analysis.draft_plan.iter().any(|node| {
                    node.status != DraftStatus::Omitted && node.requirement_ids.contains(id)
                })
                && !next.analysis.draft_plan.iter().any(|node| {
                    node.status != DraftStatus::Omitted && node.requirement_ids.contains(id)
                })
        });
        if unmapped {
            return Err("batch would leave a required or conditional requirement unmapped".into());
        }
        let volumes: Vec<String> = volume_touch.into_iter().collect();
        super::outline_flow::invalidate_checks(&mut next, composition_changed, &volumes);
        *state = next;
        return Ok(json!({"saved": saved, "id_map": id_map}));
    }
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
    input: &FrozenInput,
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
        node.windows = split_windows(input, &node.source_ids);
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
        crate::tender_analysis::tools::validate_span(input, state.coverage(), span)?;
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
        crate::tender_analysis::tools::validate_span(input, state.coverage(), span)?;
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
    let windows = split_windows(input, &source_ids);
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
        crate::tender_analysis::tools::validate_span(input, state.coverage(), &region.source)?;
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
        crate::tender_analysis::tools::validate_span(input, state.coverage(), &span)?;
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
            use super::outline_flow::{self, Phase};
            state.draft_stage = DraftStage::Outline;
            if state.outline_run.no_progress_rounds > 2 {
                return Err("outline semantic repair exhausted; checkpoint retained".into());
            }
            if state.analysis.outline.phase == Phase::Discover {
                if outline_flow::scan_complete(input, &state.analysis.outline) {
                    state.analysis.outline.phase = Phase::Outline;
                    state.outline_run.phase = Phase::Outline;
                    state.outline_run.chunk_cursor = outline_chunks(input).len();
                    state.main_work = None;
                    // Preserve repair evidence and failed-call identities; only
                    // first discovery hands off with a fresh conversation.
                    if state.analysis.outline.checks.is_empty() {
                        state.transcript.clear();
                    }
                } else if !outline_flow::scan_complete(input, &state.analysis.outline) {
                    preload_outline_window(input, state);
                }
            }
            // A repaired outline must return through the same publication
            // blockers and packet construction as an explicit finish call.
            if state.analysis.outline.phase == Phase::Outline
                && !state.analysis.outline.checks.is_empty()
                && state
                    .analysis
                    .outline
                    .issues
                    .values()
                    .all(|issue| issue.status != outline_flow::IssueStatus::Open)
            {
                let mut candidate = state.clone();
                if outline_flow::apply(input, &mut candidate, "finish_outline", &json!({}), 8192)
                    .is_ok()
                {
                    *state = candidate;
                }
            }
            if outline_flow::checked(input, state) {
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

/// Select unscanned sources independently of the 8KB accounting chunks.
pub(super) fn outline_reading_scope(
    input: &FrozenInput,
    state: &super::agent::Checkpoint,
    budget: usize,
) -> Vec<String> {
    let mut ids = Vec::new();
    let mut estimated = 0usize;
    for chunk in outline_chunks(input) {
        if chunk.kind == "metadata"
            || chunk_scanned(input, &state.analysis.outline, &chunk)
            || ids.contains(&chunk.source_id)
        {
            continue;
        }
        let bytes = input
            .source_units
            .iter()
            .find(|source| source.source_unit_revision_id == chunk.source_id)
            .map_or(0, |source| {
                let read = state
                    .analysis
                    .coverage
                    .text
                    .get(&chunk.source_id)
                    .into_iter()
                    .flatten()
                    .map(|(start, end)| end.saturating_sub(*start))
                    .sum::<usize>();
                source.text.len().saturating_sub(read).min(budget)
            });
        let form_bytes = input
            .structured_forms
            .iter()
            .filter(|form| form["source_unit_revision_id"] == chunk.source_id)
            .fold(0usize, |sum, form| {
                let total = super::relations::form_total(&form["definition"]).unwrap_or(0);
                let read = form["form_definition_revision_id"]
                    .as_str()
                    .and_then(|id| state.analysis.coverage.form_cells.get(id))
                    .into_iter()
                    .flatten()
                    .map(|(start, end)| end.saturating_sub(*start))
                    .sum::<usize>();
                let remaining = total.saturating_sub(read);
                let size =
                    serde_json::to_vec(&form["definition"]).map_or(budget, |bytes| bytes.len());
                sum.saturating_add(size.saturating_mul(remaining) / total.max(1))
            });
        // Reserve navigation/identity overhead; serialized evidence is checked again.
        let cost = bytes
            .saturating_add(form_bytes)
            .min(budget)
            .saturating_add(512);
        if !ids.is_empty() && estimated.saturating_add(cost) > budget {
            break;
        }
        estimated = estimated.saturating_add(cost);
        ids.push(chunk.source_id);
    }
    ids
}

/// 保存大纲发现游标。阅读包大小由请求组包层决定。首次建立后冻结分块摘要，
/// 游标指向下一个未记账的 UTF-8／表格区间，不因重试改计划。
pub fn preload_outline_window(input: &FrozenInput, state: &mut super::agent::Checkpoint) {
    let chunks = outline_chunks(input);
    let plan_sha = super::digest(&chunks).expect("outline chunk plan digest");
    if state.outline_run.chunk_plan_sha256.is_empty() {
        state.outline_run.chunk_plan_sha256 = plan_sha;
    }
    let index = chunks
        .iter()
        .position(|chunk| !chunk_scanned(input, &state.analysis.outline, chunk))
        .unwrap_or(chunks.len().saturating_sub(1));
    state.outline_run.chunk_cursor = index;
    let windows = outline_windows(input);
    let active = chunks.get(index);
    let source_index = active
        .and_then(|chunk| {
            windows
                .iter()
                .position(|window| window.iter().any(|id| id == &chunk.source_id))
        })
        .unwrap_or(windows.len().saturating_sub(1));
    state.draft_outline_window = source_index;
    // Only the recovery cursor lives here. The request builder sizes the
    // reading package from the frozen model configuration.
    let window = active
        .filter(|chunk| chunk.kind != "metadata")
        .map(|chunk| vec![chunk.source_id.clone()])
        .unwrap_or_default();
    let note = format!(
        "Inspect all actually delivered ranges and submit them together with submit_outline_scan. Chunk cursor {}/{} is accounting progress, not a one-chunk-per-turn restriction. Do not confirm navigation-only, unsent or overlapping context ranges.",
        index + 1,
        chunks.len().max(1)
    );
    state.main_work = Some(super::agent::WorkState {
        source_scope: window,
        deferred_sources: vec![],
        objective: format!(
            "按已投递原文写出投标文件组成大纲（第 {}/{} 块；缺依据的来源按索引 id 定向补读）",
            index + 1,
            chunks.len().max(1)
        ),
        focus: Default::default(),
        output_refs: vec![],
        pending_refs: vec![],
        status: super::agent::context::WorkStatus::Active,
        note,
    });
}

/// S1 结构索引：全书标题树与来源清单投影，不含正文。draft 路径原先根本不投递
/// `documents` 元数据，把最便宜、最结构化的信息藏起来却给了自由检索。
pub fn outline_index(
    input: &FrozenInput,
    state: &super::agent::Checkpoint,
    max_bytes: usize,
) -> Value {
    let rows: Vec<Value> = input.source_units.iter().map(|source| json!({
        "source_id":source.source_unit_revision_id,"document_id":source.document_id,
        "ordinal":source.ordinal,"bytes":source.text.len(),"locator":source.locator,
        "forms":input.structured_forms.iter().filter(|f| f["source_unit_revision_id"]==source.source_unit_revision_id)
            .map(|f| &f["form_definition_revision_id"]).collect::<Vec<_>>()
    })).collect();
    json!({"current_window":state.draft_outline_window,"chunk_cursor":state.outline_run.chunk_cursor,
        "chunk_plan_sha256":state.outline_run.chunk_plan_sha256,"total_sources":rows.len(),
        "sources":super::tools::bounded_page(&rows,0,rows.len().max(1),max_bytes/2).unwrap_or_else(|e| json!({"error":e})),
        "documents":super::tools::bounded_page(&input.documents,0,input.documents.len().max(1),max_bytes/4).unwrap_or_else(|e|json!({"error":e})),
        "instruction":"Use source_index(offset=next,limit=...) to continue. Tables are not automatically prescribed bid forms. Use collection_index for document relations and decisions."})
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
