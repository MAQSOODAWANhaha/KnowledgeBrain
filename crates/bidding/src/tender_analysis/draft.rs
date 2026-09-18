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
/// 大纲阶段的窗数上限。锚点窗之外的顺序全扫只是 fallback，超限带缺口发布。
pub const OUTLINE_MAX_WINDOWS: usize = 8;
/// 一个标题最多绑这么多来源，其余留给定向补读。
pub const BIND_MAX_SOURCES: usize = 8;
/// 连续这么多轮修补都没减少缺口就收尾，不空转。
pub const OUTLINE_MAX_STALLED_ROUNDS: usize = 2;
/// 阶段一的墙钟兜底：到点把已有大纲编译成骨架出稿（不是失败，也不是「必须」20
/// 分钟写完）。目标是远快于此，见 `OUTLINE_DEADLINE_TARGET_SECS`。
pub const DRAFT_DEADLINE_SECS: u64 = 20 * 60;
/// 阶段一的墙钟**目标**：正常文档应该几分钟出骨架。
pub const OUTLINE_DEADLINE_TARGET_SECS: u64 = 5 * 60;
pub const OFFICIAL_DEADLINE_SECS: u64 = 45 * 60;
pub const DRAFT_MAX_ATTEMPTS: i32 = 2;
/// 整体填充的**写作窗**：到点停止派章，把已填的章编译出稿。
pub const FILL_DEADLINE_SECS: u64 = 40 * 60;
/// 整体填充的**信封窗**：写作窗之后还留 5 分钟给编译、登记与入稿，且必须短于
/// SQL 的 46 分钟硬租约（SQL 只作兜底，不在那边再调一遍）。
pub const FILL_ENVELOPE_DEADLINE_SECS: u64 = 45 * 60;

// 三条口径写成编译期断言，改常数时当场失败，不用等哪个测试恰好覆盖到。
const _: () = assert!(
    OUTLINE_MAX_WINDOWS + 4 <= OUTLINE_TURN_TARGET,
    "最坏窗数下的大纲帽也要落在目标回合内，否则目标是空话"
);
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

/// 编译产物 + 为了压进字节上限而被退回骨架的章。
pub struct DraftCompile {
    pub compiled: crate::docx_composition::compiler::Compiled,
    pub degraded: Vec<String>,
}

/// 按配置上限编译草稿，超限时降级出稿而不是让整轮白跑。
///
/// 一份填满的 106 页投标文件很容易超过骨架用的上限。用户宁愿拿到一份「前面
/// 有正文、后面留空标题」的 Word，也不要一个编译错误和零产物；被退回的章会
/// 报上来，由调用方告知用户。
pub fn compile_draft(
    input: &FrozenInput,
    result: &super::AnalysisResult,
    max_docx_bytes: usize,
) -> Result<DraftCompile, String> {
    let filled: Vec<String> = result
        .analysis
        .draft_plan
        .iter()
        .filter(|item| item.status == DraftStatus::Filled)
        .map(|item| item.id.clone())
        .collect();
    let mut keep = filled.len();
    loop {
        let mut attempt = result.clone();
        let degraded: Vec<String> = filled.iter().skip(keep).cloned().collect();
        for id in &degraded {
            if let Some(item) = attempt
                .analysis
                .draft_plan
                .iter_mut()
                .find(|item| &item.id == id)
            {
                item.status = DraftStatus::Pending;
            }
        }
        if !degraded.is_empty() {
            // The compiler checks the analysis digest, so a degraded plan has to
            // be a coherent analysis rather than an edited copy of another one.
            attempt.review.analysis_sha256 = crate::tender_analysis::digest(&attempt.analysis)?;
        }
        let composed = crate::docx_composition::synthesize_draft_document(input, &attempt)?;
        match crate::docx_composition::compiler::compile(input, &attempt, &composed, max_docx_bytes)
        {
            Ok(compiled) => return Ok(DraftCompile { compiled, degraded }),
            Err(error) if keep > 0 && error.contains("exceeds configured byte budget") => {
                keep /= 2;
            }
            Err(error) => return Err(error),
        }
    }
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

fn titles_match(left: &str, right: &str) -> bool {
    let left_core = title_core(left);
    let right_core = title_core(right);
    folded_contains(left, right_core)
        || folded_contains(right, left_core)
        || folded_contains(left_core, right_core)
        || folded_contains(right_core, left_core)
}

fn source_subheadings(input: &FrozenInput, title: &str) -> Vec<String> {
    let mut kids = BTreeSet::new();
    // 分母只取能当目录依据的来源：投标人须知、评标办法、技术规格的标题树是要求，
    // 不是投标目录，拿它们当分母就会把招标目录抄进投标文件。
    for source in input
        .source_units
        .iter()
        .filter(|source| is_catalog_source(input, source))
    {
        let parts = heading_parts(source);
        for (index, part) in parts.iter().enumerate() {
            if titles_match(part, title) && index + 1 < parts.len() {
                kids.insert(parts[index + 1].clone());
            }
        }
    }
    kids.into_iter().collect()
}

fn child_covers_subheading(child_title: &str, subheading: &str) -> bool {
    folded_contains(child_title, subheading)
        || folded_contains(subheading, title_core(child_title))
        || folded_contains(child_title, title_core(subheading))
}

/// 宿主完整性清单里的一条缺口：`owner` 为空表示缺的是组成条款/点名表单里
/// 列出的顶层项，否则是该节点在来源标题下还没写的子标题。
pub struct OutlineGap {
    pub owner: Option<String>,
    pub owner_title: String,
    pub missing: Vec<String>,
}

/// 组成条款定位标记。宿主只用它找到条款位置，条款里列了什么由原文决定，
/// 不靠内置行业词表猜标题。
const COMPOSITION_CLAUSE_MARKERS: [&str; 8] = [
    "投标文件组成",
    "投标文件的组成",
    "投标文件应包括",
    "投标文件由",
    "装订顺序",
    "须提交的格式",
    "由下列",
    "包括以下",
];

/// 「投标文件格式/附表」章的定位标记。与组成条款标记一起决定哪些来源能当目录依据。
const FORMAT_CHAPTER_MARKERS: [&str; 4] = ["投标文件格式", "响应文件格式", "投标文件组成", "附表"];

/// 能当目录依据的来源：组成/装订条款所在来源、「投标文件格式/附表」章、被点名的
/// 表单所在来源。其余章节是投标要求，不是投标目录。
pub fn is_catalog_source(input: &FrozenInput, source: &Source) -> bool {
    if !composition_clause_items(&source.text).is_empty() {
        return true;
    }
    if heading_parts(source).iter().any(|part| {
        FORMAT_CHAPTER_MARKERS
            .iter()
            .any(|m| folded_contains(part, m))
    }) {
        return true;
    }
    input
        .structured_forms
        .iter()
        .any(|form| form["source_unit_revision_id"] == source.source_unit_revision_id)
}

fn plausible_item_title(part: &str) -> bool {
    let count = part.chars().count();
    (2..=20).contains(&count)
        && !part.contains('。')
        && !COMPOSITION_CLAUSE_MARKERS
            .iter()
            .any(|marker| part.contains(marker))
}

fn strip_ordinal(part: &str) -> &str {
    part.trim()
        .trim_start_matches(|c: char| {
            c.is_ascii_digit() || matches!(c, '.' | '．' | '、' | ' ' | '①'..='⑳')
        })
        .trim()
}

/// 条款正文按枚举标记切条。列表既可能是「、」串联，也可能是编号或换行分行。
/// 没有冒号也没有分隔符的，说明标记后面只是普通句子，不是枚举，不取。
fn clause_items(body: &str) -> Vec<String> {
    if !body.contains(['、', '；', ';', '\n']) {
        return Vec::new();
    }
    body.split(|c: char| {
        matches!(
            c,
            '、' | '；' | ';' | '\n' | '，' | ',' | '(' | ')' | '（' | '）'
        )
    })
    .map(strip_ordinal)
    .filter(|part| plausible_item_title(part))
    .map(str::to_string)
    .collect()
}

fn composition_clause_items(text: &str) -> Vec<String> {
    let mut items = Vec::new();
    for marker in COMPOSITION_CLAUSE_MARKERS {
        let mut from = 0;
        while let Some(hit) = text[from..].find(marker) {
            let start = from + hit + marker.len();
            let end = text[start..]
                .find('。')
                .map(|index| start + index)
                .unwrap_or(text.len());
            let body = &text[start..end];
            // 「……由下列文件组成：投标函、……」冒号之后才是枚举本身。
            let body = match body.char_indices().rfind(|(_, c)| matches!(c, '：' | ':')) {
                Some((index, colon)) => &body[index + colon.len_utf8()..],
                None => body,
            };
            items.extend(clause_items(body));
            from = end.max(start);
        }
    }
    items
}

fn all_form_titles(input: &FrozenInput) -> Vec<String> {
    input
        .structured_forms
        .iter()
        .filter_map(|form| {
            form["definition"]["title"]
                .as_str()
                .or_else(|| form["title"].as_str())
                .map(str::to_string)
        })
        .filter(|title| plausible_item_title(title))
        .collect()
}

/// 期望清单：组成条款枚举 + 被点名的指定格式/附表。
fn expected_outline_items(input: &FrozenInput) -> Vec<String> {
    let mut expected: Vec<String> = Vec::new();
    let candidates = input
        .source_units
        .iter()
        .flat_map(|source| composition_clause_items(&source.text))
        .chain(all_form_titles(input));
    for item in candidates {
        if !expected.iter().any(|kept| titles_match(kept, &item)) {
            expected.push(item);
        }
    }
    expected
}

fn catalog_gaps(input: &FrozenInput, plan: &[DraftPlanItem]) -> Vec<OutlineGap> {
    let live = |item: &DraftPlanItem| item.status != DraftStatus::Omitted;
    let mut gaps = Vec::new();
    // 根节点同样要查：「只有分册名」正好是漏掉根节点时看不见的那一类。
    for item in plan.iter().filter(|item| live(item)) {
        let required = source_subheadings(input, &item.title);
        if required.is_empty() {
            continue;
        }
        let children: Vec<_> = plan
            .iter()
            .filter(|child| live(child) && child.parent.as_deref() == Some(item.id.as_str()))
            .collect();
        let missing: Vec<_> = required
            .into_iter()
            .filter(|sub| {
                !children
                    .iter()
                    .any(|child| child_covers_subheading(&child.title, sub))
            })
            .collect();
        if !missing.is_empty() {
            gaps.push(OutlineGap {
                owner: Some(item.id.clone()),
                owner_title: item.title.clone(),
                missing,
            });
        }
    }
    gaps
}

/// 宿主完整性清单。大纲只有在清单闭合时才算写完；到期是「带缺口发布」。
pub fn outline_gaps(input: &FrozenInput, plan: &[DraftPlanItem]) -> Vec<OutlineGap> {
    let mut gaps = Vec::new();
    // 有据 omitted 也算映射到了，所以这里连 omitted 节点一起认。
    let missing: Vec<String> = expected_outline_items(input)
        .into_iter()
        .filter(|item| !plan.iter().any(|node| titles_match(&node.title, item)))
        .collect();
    if !missing.is_empty() {
        gaps.push(OutlineGap {
            owner: None,
            owner_title: "投标文件组成条款".into(),
            missing,
        });
    }
    gaps.extend(catalog_gaps(input, plan));
    gaps
}

pub fn outline_ready(input: &FrozenInput, plan: &[DraftPlanItem]) -> bool {
    !plan.is_empty() && outline_gaps(input, plan).is_empty()
}



/// 大纲阶段的回合帽：索引 + 逐窗投递 + 修补轮，按窗数推导。
pub fn outline_turn_cap(input: &FrozenInput) -> usize {
    outline_windows(input).len().saturating_mul(4).saturating_add(12)
}

/// 帽到期或修补轮不再减少缺口时，把每条缺口记成 `omitted(bind_failed)`，
/// 让它出现在报告里而不是静默消失。


/// 顺序正确性：同一父下的 `order` 以组成条款枚举顺序为准；条款没列的节点保持
/// 相对次序排在其后。



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



/// 命中来源的优先级：表名精确 > 目录依据章 > `heading_path` 同名 > 只在正文出现。
fn bind_rank(input: &FrozenInput, source: &Source, terms: &[String]) -> u8 {
    if form_titles(input, &source.source_unit_revision_id)
        .iter()
        .any(|name| terms.iter().any(|term| folded_contains(name, term)))
    {
        return 0;
    }
    if is_catalog_source(input, source) {
        return 1;
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
    for item in plan.iter_mut().filter(|item| item.status == DraftStatus::Pending) {
        if !item.source_ids.is_empty() { continue; }
        item.windows.clear();
        item.window_index = 0;
        match bind_source_ids(input, &item.title, &[]) {
            Ok(hits) => { item.source_ids = hits; item.windows = split_windows(input, &item.source_ids); item.omit_reason = None; }
            Err(reason) => { item.source_ids.clear(); item.omit_reason = Some(reason); }
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

/// 大纲阶段的窗序列：先投锚点窗（组成条款与投标文件格式章），其余来源按顺序作为
/// fallback 排在后面，总窗数有上限。锚点由宿主用与完整性清单同一组条款标记确定，
/// 不再多要一次模型选点。
pub fn outline_windows(input: &FrozenInput) -> Vec<Vec<String>> {
    let ids = input.source_units.iter().map(|source| source.source_unit_revision_id.clone()).collect::<Vec<_>>();
    let windows = split_windows(input, &ids);
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

/// 常驻读工具。**没有 `search_sources`**：自由检索是回合失控的入口，读取只准
/// 按索引里的 id 取。工具集固定、不随文件大小变，所以冻结合同只有两份 hash。
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
                Some("collection_index" | "source_index" | "search_sources" | "read_source" | "read_form" | "read_source_view")
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
    if matches!(name, "submit_outline_scan" | "read_outline" | "submit_outline_check" | "finish_outline") {
        return super::outline_flow::apply(input, state, name, args, config.limits.max_tool_result_bytes);
    }
    if name == "put_outline_items" {
        if state.analysis.outline.phase != super::outline_flow::Phase::Outline { return Err("chapter organization requires completed discovery".into()); }
        let items = args["items"].as_array().ok_or("items required")?;
        let mut next = state.clone();
        let mut saved = Vec::new();
        for item in items { saved.push(put_outline_item(input, config, &mut next, item)?); }
        let removed: Vec<String> = serde_json::from_value(args["remove_ids"].clone()).map_err(|e| e.to_string())?;
        next.analysis.draft_plan.retain(|node| !removed.contains(&node.id));
        super::outline_flow::tree_valid(&next.analysis.draft_plan)?;
        next.analysis.outline.checked_sha256 = None;
        *state = next;
        return Ok(json!({"saved":saved}));
    }
    match name {
        "put_outline_item" => put_outline_item(input, config, state, args),
        "omit_outline_item" => omit_outline_item(input, config, state, args),
        "put_chapter_template" => put_chapter_template(input, config, state, args),
        "put_chapter_omission" => put_chapter_omission(input, state, args),
        _ => Err(format!("unknown draft tool {name}")),
    }
}

fn put_outline_item(
    input: &FrozenInput,
    _config: &super::agent::Config,
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
    let requirement_ids: Vec<String> = serde_json::from_value(args["requirement_ids"].clone()).map_err(|e| e.to_string())?;
    if requirement_ids.iter().any(|id| !state.analysis.outline.requirements.contains_key(id)) { return Err("unknown submission requirement".into()); }
    let mut source_ids: Vec<String> = requirement_ids.iter().flat_map(|id| state.analysis.outline.requirements[id].format_grounds.iter().map(|span| span.source_id.clone())).collect();
    if source_ids.is_empty() { source_ids = grounds.iter().map(|span| span.source_id.clone()).collect(); }
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
            purpose: ChapterPurpose::Response,
            format_refs: vec![],
            body_status: BodyStatus::Empty,
            omit_reason: None,
            preserved: vec![],
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

fn put_chapter_omission(
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
    plan.iter().any(|item| item.parent.as_deref() == Some(id))
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
        .filter(|item| item.status == DraftStatus::Pending && !item.source_ids.is_empty())
        .filter(|item| !has_plan_children(&state.analysis.draft_plan, &item.id))
        // 叶子章就是可填章：声明了父节点的必须真有父节点，根一级的叶子章同样可填。
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
            if state.outline_run.no_progress_rounds > 2 { return Err("outline semantic repair exhausted; checkpoint retained".into()); }
            if state.analysis.outline.phase == Phase::Discover {
                if outline_flow::scan_complete(input, &state.analysis.outline) {
                    state.analysis.outline.phase = Phase::Outline;
                    state.transcript.clear();
                } else {
                    preload_outline_window(input, state);
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
                        && matches!(item.status, DraftStatus::Filled | DraftStatus::Omitted)
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

/// 投递当前大纲窗。全书装得进一窗时只跑一轮，与今天的「小文件」行为一致，但走的是
/// 同一条代码路径。
pub fn preload_outline_window(input: &FrozenInput, state: &mut super::agent::Checkpoint) {
    let windows = outline_windows(input);
    let index = windows.iter().position(|window| window.iter().any(|id| !super::outline_flow::source_scanned(input, &state.analysis.outline, id))).unwrap_or(windows.len()-1);
    state.draft_outline_window = index;
    let window = windows[index].clone();
    state.main_work = Some(super::agent::WorkState {
        source_scope: window,
        deferred_sources: vec![],
        objective: format!(
            "按已投递原文写出投标文件组成大纲（第 {}/{} 窗；缺依据的来源按索引 id 定向补读）",
            index + 1,
            windows.len()
        ),
        focus: Default::default(),
        output_refs: vec![],
        pending_refs: vec![],
        status: super::agent::context::WorkStatus::Active,
        note: "Inspect the actual delivered ranges, then submit_outline_scan. Read remaining ranges before claiming a whole source.".into(),
    });
}

/// S1 结构索引：全书标题树与来源清单投影，不含正文。draft 路径原先根本不投递
/// `documents` 元数据，把最便宜、最结构化的信息藏起来却给了自由检索。
pub fn outline_index(input: &FrozenInput, state: &super::agent::Checkpoint, max_bytes: usize) -> Value {
    let rows: Vec<Value> = input.source_units.iter().map(|source| json!({
        "source_id":source.source_unit_revision_id,"document_id":source.document_id,
        "ordinal":source.ordinal,"bytes":source.text.len(),"locator":source.locator,
        "forms":input.structured_forms.iter().filter(|f| f["source_unit_revision_id"]==source.source_unit_revision_id)
            .map(|f| &f["form_definition_revision_id"]).collect::<Vec<_>>()
    })).collect();
    json!({"current_window":state.draft_outline_window,"total_sources":rows.len(),
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
