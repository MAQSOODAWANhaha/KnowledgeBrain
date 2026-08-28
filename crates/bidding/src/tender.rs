//! Final V1 bounded tender segmentation, fact proposals, and KindRouter.

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, sync::LazyLock};
use crate::bidding::{
    CompleteDocumentConversion, ConvertedSourceImageUpload, MutationContext, PublishSection,
};
use uuid::Uuid;

pub const ROUTER_VERSION: &str = "kind-router-v1";
pub const POLICY_VERSION: &str = "requirement-span-v1+fact-suggestion-v1";
pub const PROMPT_VERSION: &str = "bounded-tender-publication-v1";

#[derive(Deserialize)]
struct TenderConfig {
    families: HashMap<String, TenderFamilyConfig>,
    skip_heading_hints: Vec<String>,
    outline: TenderOutlineConfig,
}

#[derive(Deserialize)]
struct TenderFamilyConfig {
    heading_hints: Vec<String>,
    signals: Vec<String>,
}

#[derive(Deserialize)]
struct TenderOutlineConfig {
    enumeration_prefix_terms: Vec<String>,
    numbered_heading_suffixes: Vec<String>,
    numbered_requirement_predicates: Vec<String>,
    table_predicates: Vec<String>,
}

static TENDER_CONFIG: LazyLock<TenderConfig> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../config/cn-tender-v2.json"))
        .expect("cn-tender-v2 config must be valid")
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClauseKind {
    Technical,
    Qualification,
    Service,
    Pricing,
    ScheduleDelivery,
    SchedulePayment,
    Evaluation,
    Procedural,
}

impl ClauseKind {
    pub const ALL: [Self; 8] = [
        Self::Technical,
        Self::Qualification,
        Self::Service,
        Self::Pricing,
        Self::ScheduleDelivery,
        Self::SchedulePayment,
        Self::Evaluation,
        Self::Procedural,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Technical => "technical",
            Self::Qualification => "qualification",
            Self::Service => "service",
            Self::Pricing => "pricing",
            Self::ScheduleDelivery => "schedule_delivery",
            Self::SchedulePayment => "schedule_payment",
            Self::Evaluation => "evaluation",
            Self::Procedural => "procedural",
        }
    }

    pub const fn family(self) -> Option<&'static str> {
        match self {
            Self::Technical => Some("technical"),
            Self::Qualification | Self::Service => Some("commercial"),
            Self::Pricing
            | Self::ScheduleDelivery
            | Self::SchedulePayment
            | Self::Evaluation
            | Self::Procedural => None,
        }
    }
}

impl std::str::FromStr for ClauseKind {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.as_str() == value)
            .ok_or("invalid clause kind")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutedKind {
    pub kind: ClauseKind,
    pub reason_code: &'static str,
}

/// The only automatic kind authority. It follows the frozen priority/veto
/// contract; extraction agents never produce kind or family.
pub fn route_kind(text: &str) -> RoutedKind {
    let payment_technical = contains_any(
        text,
        &["支付接口", "支付网关", "付款接口", "支付API", "支付密码"],
    );
    let technical_subject = contains_any(text, &["设备", "系统", "接口", "协议"])
        && contains_any(text, &["性能", "能力", "参数", "响应时间"]);
    if payment_technical || technical_subject {
        return RoutedKind {
            kind: ClauseKind::Technical,
            reason_code: "TECHNICAL_SUBJECT_PREDICATE",
        };
    }
    if contains_any(
        text,
        &[
            "许可证",
            "ISO",
            "等保",
            "资质",
            "软著",
            "业绩",
            "合同复印件",
            "合同佐证",
            "证书",
        ],
    ) {
        return RoutedKind {
            kind: ClauseKind::Qualification,
            reason_code: "QUALIFICATION_EVIDENCE",
        };
    }
    if contains_any(
        text,
        &[
            "保证金",
            "密封",
            "投标函",
            "授权委托",
            "法定代表人",
            "签章样式",
            "递交",
        ],
    ) {
        return RoutedKind {
            kind: ClauseKind::Procedural,
            reason_code: "PROCEDURAL_MATERIAL_OR_ACTION",
        };
    }
    if contains_any(text, &["付款", "结算", "支付"])
        && contains_any(text, &["比例", "金额", "节点", "账期", "验收", "主体"])
    {
        return RoutedKind {
            kind: ClauseKind::SchedulePayment,
            reason_code: "PAYMENT_ACTION_AND_TERM",
        };
    }
    if contains_any(
        text,
        &[
            "到货",
            "交货",
            "供货",
            "工期",
            "实施周期",
            "交付地点",
            "供货地点",
        ],
    ) {
        return RoutedKind {
            kind: ClauseKind::ScheduleDelivery,
            reason_code: "DELIVERY_TERM",
        };
    }
    let pricing = contains_any(text, &["分项报价", "计价口径", "单列价格", "报价明细"]);
    let evaluation = contains_any(text, &["评分项", "权重", "得分", "评分标准"]);
    if pricing {
        return RoutedKind {
            kind: ClauseKind::Pricing,
            reason_code: if evaluation {
                "PRICING_EVALUATION_CONFLICT"
            } else {
                "PRICING_STRUCTURE"
            },
        };
    }
    if evaluation {
        return RoutedKind {
            kind: ClauseKind::Evaluation,
            reason_code: "EVALUATION_SCORE",
        };
    }
    if contains_any(text, &["质保", "驻场", "培训", "应急", "7x24", "SLA"]) {
        return RoutedKind {
            kind: ClauseKind::Service,
            reason_code: "SERVICE_OBLIGATION",
        };
    }
    RoutedKind {
        kind: ClauseKind::Technical,
        reason_code: "BOUNDED_TECHNICAL_FALLBACK",
    }
}

fn contains_any(text: &str, terms: &[&str]) -> bool {
    terms.iter().any(|term| text.contains(term))
}

fn has_requirement_signal(text: &str) -> bool {
    contains_any(
        text,
        &["必须", "应当", "应", "须", "不得", "要求", "提供", "提交"],
    ) || contains_any(
        text,
        &[
            "设备",
            "系统",
            "接口",
            "协议",
            "许可证",
            "ISO",
            "等保",
            "资质",
            "质保",
            "驻场",
            "培训",
            "付款",
            "交货",
            "供货",
            "报价",
            "评分",
            "保证金",
            "密封",
            "投标函",
        ],
    ) || configured_requirement_signal(text)
        || numbered_title_has_strong_requirement(numbered_line_body(text).unwrap_or(text))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceSpanV2 {
    pub schema_version: u8,
    pub source_artifact_id: Uuid,
    pub section_artifact_id: Uuid,
    pub project_id: Uuid,
    pub document_id: Uuid,
    pub conversion_generation: i32,
    pub section_key: String,
    pub parent_start_offset: i64,
    pub parent_end_offset: i64,
    pub start_offset: i64,
    pub end_offset: i64,
    pub offset_unit: String,
    pub quote: String,
    pub quote_sha256: String,
    pub heading_path: Vec<String>,
}

/// Frozen identities used to verify a [`SourceSpanV2`] before it is consumed.
///
/// PostgreSQL repeats these checks when publishing. Keeping the verifier here
/// gives non-database consumers the same byte/scope boundary without falling
/// back to a live document pointer.
pub struct SourceSpanScope<'a> {
    pub source: &'a [u8],
    pub source_artifact_id: Uuid,
    pub section_artifact_id: Uuid,
    pub project_id: Uuid,
    pub document_id: Uuid,
    pub conversion_generation: i32,
    pub section_key: &'a str,
    pub parent_start_offset: usize,
    pub parent_end_offset: usize,
    pub heading_path: &'a [String],
}

pub fn verify_source_span_v2(
    span: &SourceSpanV2,
    scope: SourceSpanScope<'_>,
) -> Result<(), &'static str> {
    let source_utf8 =
        std::str::from_utf8(scope.source).map_err(|_| "frozen source is not valid UTF-8")?;
    if span.schema_version != 2 || span.offset_unit != "utf8_byte" {
        return Err("source span schema or offset unit invalid");
    }
    if span.source_artifact_id != scope.source_artifact_id
        || span.section_artifact_id != scope.section_artifact_id
        || span.project_id != scope.project_id
        || span.document_id != scope.document_id
        || span.conversion_generation != scope.conversion_generation
        || span.section_key != scope.section_key
        || span.heading_path != scope.heading_path
    {
        return Err("source span frozen scope mismatch");
    }

    let parent_start = usize::try_from(span.parent_start_offset)
        .map_err(|_| "source span parent bounds invalid")?;
    let parent_end =
        usize::try_from(span.parent_end_offset).map_err(|_| "source span parent bounds invalid")?;
    let start = usize::try_from(span.start_offset).map_err(|_| "source span bounds invalid")?;
    let end = usize::try_from(span.end_offset).map_err(|_| "source span bounds invalid")?;
    if parent_start != scope.parent_start_offset
        || parent_end != scope.parent_end_offset
        || parent_start >= parent_end
        || parent_end > scope.source.len()
        || !source_utf8.is_char_boundary(parent_start)
        || !source_utf8.is_char_boundary(parent_end)
        || start < parent_start
        || end > parent_end
    {
        return Err("source span parent scope mismatch");
    }
    if span.quote_sha256 != hex::encode(Sha256::digest(span.quote.as_bytes())) {
        return Err("source span quote digest mismatch");
    }
    verify_utf8_span(scope.source, start, end, &span.quote)
}

pub fn verify_utf8_span(
    source: &[u8],
    start: usize,
    end: usize,
    quote: &str,
) -> Result<(), &'static str> {
    if start >= end || end > source.len() {
        return Err("span bounds invalid");
    }
    if !source.is_char_boundary(start) || !source.is_char_boundary(end) {
        return Err("span does not use UTF-8 byte boundaries");
    }
    if &source[start..end] != quote.as_bytes() {
        return Err("span quote mismatch");
    }
    Ok(())
}

trait ByteCharBoundary {
    fn is_char_boundary(&self, index: usize) -> bool;
}

impl ByteCharBoundary for [u8] {
    fn is_char_boundary(&self, index: usize) -> bool {
        index == 0
            || index == self.len()
            || self
                .get(index)
                .is_some_and(|byte| byte & 0b1100_0000 != 0b1000_0000)
    }
}

#[derive(Debug, Clone)]
pub struct FrozenSection {
    pub key: String,
    pub heading_path: Vec<String>,
    pub parent_start_offset: usize,
    pub parent_end_offset: usize,
    pub segments: Vec<BoundedSegment>,
}

#[derive(Debug, Clone)]
pub struct BoundedSegment {
    pub start_offset: usize,
    pub end_offset: usize,
    pub quote: String,
    pub disposition: SegmentDisposition,
    pub facts: Vec<FactProposal>,
}

#[derive(Debug, Clone)]
pub enum SegmentDisposition {
    Clause {
        text: String,
        must: bool,
        routed: RoutedKind,
    },
    FactOnly,
    DeterministicNonRequirement,
    Ambiguous,
}

#[derive(Debug, Clone, Serialize)]
pub struct FactProposal {
    pub field: &'static str,
    pub typed_value: Value,
    pub confidence: String,
}

pub fn outline_and_route(markdown: &str) -> Result<Vec<FrozenSection>, String> {
    if markdown.is_empty() {
        return Err("converted source is empty".into());
    }
    let mut boundaries = vec![(0usize, Vec::<String>::new())];
    let mut heading_stack: Vec<String> = Vec::new();
    let mut byte_cursor = 0usize;
    for line in markdown.split_inclusive('\n') {
        let content = line.trim_end_matches(['\r', '\n']);
        if let Some((level, title)) = heading(content) {
            heading_stack.truncate(level.saturating_sub(1));
            heading_stack.push(title);
            if byte_cursor == 0 {
                boundaries[0].1 = heading_stack.clone();
            } else {
                boundaries.push((byte_cursor, heading_stack.clone()));
            }
        }
        byte_cursor += line.len();
    }
    boundaries.dedup_by_key(|item| item.0);
    let mut sections = Vec::new();
    for (ordinal, (start, path)) in boundaries.iter().enumerate() {
        let end = boundaries
            .get(ordinal + 1)
            .map(|item| item.0)
            .unwrap_or(markdown.len());
        if start == &end
            || markdown.as_bytes()[*start..end]
                .iter()
                .all(u8::is_ascii_whitespace)
        {
            continue;
        }
        let key = format!("section:{ordinal:04}:{}", stable_path_key(path));
        sections.push(FrozenSection {
            key,
            heading_path: path.clone(),
            parent_start_offset: *start,
            parent_end_offset: end,
            segments: segment_section(markdown, *start, end),
        });
    }
    if sections.is_empty() {
        return Err("converted source has no routable section".into());
    }
    Ok(sections)
}

fn stable_path_key(path: &[String]) -> String {
    if path.is_empty() {
        return "preamble".into();
    }
    let mut hasher = Sha256::new();
    for part in path {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part.as_bytes());
    }
    hex::encode(hasher.finalize())[..16].to_string()
}

static NUMERIC_NUMBERED_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?P<number>[0-9]{1,5}(?:\.[0-9]{1,5}){0,5})(?:[.、]\s*|\s+)(?P<title>\S.*)$")
        .expect("static regex")
});

static CHINESE_NUMBERED_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?P<prefix>(?:[（(][一二三四五六七八九十百千]+[）)]|[一二三四五六七八九十百千]+[、.．]))\s*(?P<title>\S.*)$",
    )
    .expect("static regex")
});

fn numbered_line_body(line: &str) -> Option<&str> {
    NUMERIC_NUMBERED_LINE
        .captures(line)
        .or_else(|| CHINESE_NUMBERED_LINE.captures(line))
        .and_then(|capture| capture.name("title"))
        .map(|title| title.as_str().trim())
}

fn has_subject_action_object(title: &str, actions: &[&String]) -> bool {
    const SUBJECT_TERMS: [&str; 15] = [
        "系统",
        "产品",
        "设备",
        "服务",
        "项目",
        "企业",
        "公司",
        "投标人",
        "供应商",
        "承包人",
        "申请人",
        "法定代表人",
        "投标文件",
        "响应文件",
        "报价",
    ];
    actions.iter().any(|action| {
        title.match_indices(action.as_str()).any(|(index, _)| {
            let subject = title[..index].trim();
            let object = title[index + action.len()..].trim();
            !subject.is_empty()
                && !object.is_empty()
                && SUBJECT_TERMS.iter().any(|term| subject.contains(term))
                && !(action.as_str() == "符合" && object.starts_with('性'))
        })
    })
}

fn numbered_title_has_strong_requirement(title: &str) -> bool {
    if TENDER_CONFIG
        .outline
        .numbered_requirement_predicates
        .iter()
        .filter(|predicate| {
            !["应", "需", "须"]
                .iter()
                .any(|modal| predicate.ends_with(modal))
        })
        .any(|predicate| title.starts_with(predicate))
        || ["必须", "不得"]
            .iter()
            .any(|predicate| title.contains(predicate))
    {
        return true;
    }
    let action_terms = TENDER_CONFIG
        .outline
        .table_predicates
        .iter()
        .filter(|term| term.chars().count() > 1 && term.as_str() != "应当")
        .collect::<Vec<_>>();
    if TENDER_CONFIG
        .outline
        .enumeration_prefix_terms
        .iter()
        .filter(|term| term.chars().count() > 1)
        .any(|term| title.starts_with(term))
    {
        return true;
    }
    if has_subject_action_object(title, &action_terms) {
        return true;
    }
    const EXTRA_ACTION_TERMS: [&str; 20] = [
        "完成", "符合", "遵守", "保证", "确保", "交付", "部署", "安装", "禁止", "限制", "签字",
        "盖章", "密封", "递交", "缴纳", "上传", "验收", "付款", "兼容", "为",
    ];
    ["应当", "应该", "应", "需要", "需", "须"]
        .iter()
        .any(|modal| {
            title.match_indices(modal).any(|(index, _)| {
                let rest = &title[index + modal.len()..];
                action_terms.iter().any(|action| rest.starts_with(*action))
                    || EXTRA_ACTION_TERMS
                        .iter()
                        .any(|action| rest.starts_with(action))
            })
        })
}

fn configured_heading_hint(title: &str) -> bool {
    TENDER_CONFIG
        .families
        .values()
        .flat_map(|family| &family.heading_hints)
        .chain(&TENDER_CONFIG.skip_heading_hints)
        .any(|hint| hint == title)
}

fn configured_requirement_signal(title: &str) -> bool {
    TENDER_CONFIG
        .families
        .values()
        .flat_map(|family| &family.signals)
        .any(|signal| title.contains(signal))
}

fn numbered_title_is_heading(title: &str) -> bool {
    if numbered_title_has_strong_requirement(title) {
        return false;
    }
    if configured_heading_hint(title) {
        return true;
    }
    !configured_requirement_signal(title)
        && TENDER_CONFIG
            .outline
            .numbered_heading_suffixes
            .iter()
            .any(|suffix| title.ends_with(suffix))
}

fn heading(line: &str) -> Option<(usize, String)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.len() > 256 || trimmed.contains(['。', '；', '！', '？']) {
        return None;
    }
    let hashes = trimmed.bytes().take_while(|byte| *byte == b'#').count();
    if (1..=6).contains(&hashes) && trimmed.as_bytes().get(hashes) == Some(&b' ') {
        return Some((hashes, trimmed[hashes..].trim().to_string()));
    }
    if trimmed.starts_with('第') && (trimmed.contains('章') || trimmed.contains('节')) {
        return Some((
            if trimmed.contains('章') { 1 } else { 2 },
            trimmed.to_string(),
        ));
    }
    if let Some(capture) = NUMERIC_NUMBERED_LINE.captures(trimmed) {
        let level = capture["number"].split('.').count();
        let title = capture["title"].trim();
        if numbered_title_is_heading(title) {
            return Some((level.min(6), trimmed.to_string()));
        }
        return None;
    }
    if let Some(capture) = CHINESE_NUMBERED_LINE.captures(trimmed) {
        if !numbered_title_is_heading(capture["title"].trim()) {
            return None;
        }
        let level = if capture["prefix"].starts_with(['（', '(']) {
            2
        } else {
            1
        };
        return Some((level, trimmed.to_string()));
    }
    None
}

fn segment_section(markdown: &str, parent_start: usize, parent_end: usize) -> Vec<BoundedSegment> {
    let bytes = markdown.as_bytes();
    let mut raw_ranges = Vec::new();
    let mut start = parent_start;
    let mut cursor = parent_start;
    while cursor < parent_end {
        let ch = markdown[cursor..]
            .chars()
            .next()
            .expect("valid UTF-8 source");
        cursor += ch.len_utf8();
        if matches!(ch, '。' | '；' | '！' | '？' | '\n') {
            raw_ranges.push((start, cursor));
            start = cursor;
        }
    }
    if start < parent_end {
        raw_ranges.push((start, parent_end));
    }
    raw_ranges
        .into_iter()
        .filter_map(|(start, end)| trim_utf8_range(markdown, start, end))
        .map(|(start, end)| {
            let quote = String::from_utf8(bytes[start..end].to_vec()).expect("source is UTF-8");
            let facts = extract_fact_proposals(&quote);
            let disposition = classify_segment(&quote, !facts.is_empty());
            BoundedSegment {
                start_offset: start,
                end_offset: end,
                quote,
                disposition,
                facts,
            }
        })
        .collect()
}

fn trim_utf8_range(source: &str, mut start: usize, mut end: usize) -> Option<(usize, usize)> {
    while start < end {
        let ch = source[start..end].chars().next()?;
        if !ch.is_whitespace() {
            break;
        }
        start += ch.len_utf8();
    }
    while start < end {
        let ch = source[start..end].chars().next_back()?;
        if !ch.is_whitespace() {
            break;
        }
        end -= ch.len_utf8();
    }
    (start < end).then_some((start, end))
}

fn classify_segment(text: &str, has_fact: bool) -> SegmentDisposition {
    let trimmed = text.trim();
    if heading(trimmed).is_some()
        || trimmed.starts_with("```")
        || trimmed.starts_with("![")
        || (trimmed.starts_with('|') && !contains_any(trimmed, &["必须", "应", "须", "要求"]))
    {
        return SegmentDisposition::DeterministicNonRequirement;
    }
    if has_requirement_signal(trimmed) {
        let routed = route_kind(trimmed);
        return SegmentDisposition::Clause {
            text: trimmed.to_string(),
            must: contains_any(trimmed, &["必须", "应当", "须", "不得"]),
            routed,
        };
    }
    if has_fact {
        SegmentDisposition::FactOnly
    } else if trimmed.chars().count() > 512 {
        SegmentDisposition::Ambiguous
    } else {
        SegmentDisposition::DeterministicNonRequirement
    }
}

fn extract_fact_proposals(text: &str) -> Vec<FactProposal> {
    let mut facts = Vec::new();
    const CNY_AMOUNT: &str =
        r"(?P<amount>(?:[1-9][0-9]{0,2}(?:,[0-9]{3})+|(?:0|[1-9][0-9]{0,17}))(?:\.[0-9]{1,2})?)";
    static BUDGET_AMOUNT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(&format!(
            r"预算(?:金额)?(?:为|是|[:：])?\s*(?:人民币\s*)?{CNY_AMOUNT}\s*(?:元|人民币)"
        ))
        .expect("static regex")
    });
    static CEILING_AMOUNT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(&format!(
            r"(?:最高限价|最高控制价|控制价|限价)(?:金额)?(?:为|是|[:：])?\s*(?:人民币\s*)?{CNY_AMOUNT}\s*(?:元|人民币)"
        ))
        .expect("static regex")
    });
    if let Some(amount) = capture_cny_amount(&BUDGET_AMOUNT, text) {
        facts.push(FactProposal {
            field: "budget_amount",
            typed_value: json!({"amount":amount,"currency_code":"CNY"}),
            confidence: "0.9500".into(),
        });
    }
    if let Some(amount) = capture_cny_amount(&CEILING_AMOUNT, text) {
        facts.push(FactProposal {
            field: "ceiling_price",
            typed_value: json!({"amount":amount,"currency_code":"CNY","basis":"unspecified"}),
            confidence: "0.9500".into(),
        });
    }
    let days = Regex::new(r"(?:有效期|有效)\s*(?P<days>[0-9]{1,4})\s*天").expect("static regex");
    if let Some(capture) = days.captures(text)
        && let Ok(days) = capture["days"].parse::<u16>()
        && (1..=3650).contains(&days)
    {
        facts.push(FactProposal {
            field: "bid_valid_days",
            typed_value: json!(days),
            confidence: "0.9000".into(),
        });
    }
    facts
}

fn capture_cny_amount(regex: &Regex, text: &str) -> Option<String> {
    normalize_cny(regex.captures(text)?.name("amount")?.as_str())
}

fn normalize_cny(value: &str) -> Option<String> {
    let canonical = value.replace(',', "");
    let mut parts = canonical.split('.');
    let whole = parts.next().unwrap_or("0").trim_start_matches('0');
    let whole = if whole.is_empty() { "0" } else { whole };
    if whole.len() > 18 {
        return None;
    }
    let fraction = parts.next().unwrap_or("");
    Some(format!("{whole}.{fraction:0<2}"))
}

impl FrozenSection {
    pub fn candidate_graph(&self) -> Value {
        Value::Array(
            self.segments
                .iter()
                .map(|segment| {
                    let (disposition, reason_code, clause) = match &segment.disposition {
                        SegmentDisposition::Clause { text, must, routed } => (
                            "clause",
                            "CLAUSE",
                            json!({
                                "text":text,
                                "must":must,
                                "kind":routed.kind.as_str(),
                                "router_reason_code":routed.reason_code
                            }),
                        ),
                        SegmentDisposition::FactOnly => {
                            ("non_requirement", "FACT_ONLY", Value::Null)
                        }
                        SegmentDisposition::DeterministicNonRequirement => (
                            "non_requirement",
                            "DETERMINISTIC_NON_REQUIREMENT",
                            Value::Null,
                        ),
                        SegmentDisposition::Ambiguous => ("unresolved", "AMBIGUOUS", Value::Null),
                    };
                    json!({
                        "start_offset":segment.start_offset,
                        "end_offset":segment.end_offset,
                        "quote":segment.quote,
                        "disposition":disposition,
                        "reason_code":reason_code,
                        "clause":clause,
                        "facts":segment.facts.iter().map(|fact| json!({
                            "field":fact.field,
                            "typed_value":fact.typed_value,
                            "confidence":fact.confidence
                        })).collect::<Vec<_>>()
                    })
                })
                .collect(),
        )
    }
}

#[derive(Serialize)]
struct PublicationRequest<'a> {
    target_id: Uuid,
    target_revision: i32,
    section_key: &'a str,
    parent_start_offset: usize,
    parent_end_offset: usize,
    expected_current_publication_id: Option<Uuid>,
    candidate_graph: &'a Value,
}

pub async fn convert_and_schedule_document(
    pool: &sqlx::PgPool,
    document_id: Uuid,
    target_revision: i64,
) -> Result<Option<(Uuid, i64)>, String> {
    let conversion_generation = i32::try_from(target_revision)
        .map_err(|_| "document conversion target revision is invalid".to_string())?;
    let Some(target) =
        crate::bidding::load_document_conversion(pool, document_id, conversion_generation)
            .await
            .map_err(|error| error.to_string())?
    else {
        return crate::bidding::document_conversion_successor(
            pool,
            document_id,
            conversion_generation,
        )
        .await
        .map_err(|error| error.to_string())
        .map(|successor| {
            successor.map(|successor| {
                (
                    successor.target_id,
                    i64::from(successor.extraction_generation),
                )
            })
        });
    };
    const CONVERSION_OBJECT_ACTOR: &str = "system:bid-convert-worker";
    let mut image_uploads: Vec<ConvertedSourceImageUpload> = Vec::new();
    let target_id = Uuid::new_v4();
    let conversion = async {
        let original_digest = target.object_ref.trim_start_matches("objects/");
        let bytes = platform::read_blob(original_digest).map_err(|error| error.to_string())?;
        let converted = docparser::convert_tender_source(&target.file_name, bytes)
            .await
            .map_err(|error| error.0)?;
        if !converted.error.is_empty() {
            return Err(converted.error);
        }
        let mut markdown = converted.markdown;
        let image_source_type = converted
            .metadata
            .get("image_source_type")
            .cloned()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| {
                knowledge::enrichment::image_source_type(&target.file_name, &markdown).to_string()
            });
        let language =
            knowledge::enrichment::infer_output_language(&format!("{}\n{markdown}", target.file_name));
        let mut image_digests = Vec::new();
        for image in converted.images {
            if image.data.is_empty() {
                continue;
            }
            let image_digest = hex::encode(Sha256::digest(&image.data));
            let image_ref = platform::object_ref(&image_digest);
            let media_type = image.mime_type.trim();
            if !media_type.starts_with("image/") {
                return Err("converted image media type is missing or invalid".into());
            }
            let staging_id = Uuid::new_v4();
            platform::stage_object_upload(
                pool,
                staging_id,
                &image_ref,
                &image_digest,
                media_type,
                image.data.len() as i64,
                CONVERSION_OBJECT_ACTOR,
            )
            .await
            .map_err(|error| error.to_string())?;
            image_uploads.push(ConvertedSourceImageUpload {
                staging_id,
                object_ref: image_ref.clone(),
                digest: image_digest.clone(),
                media_type: media_type.to_string(),
                byte_length: image.data.len() as i64,
                occurrence: format!("image:{}", image_uploads.len()),
            });
            platform::write_blob_off_runtime(&image_digest, &image.data)
                .map_err(|error| error.to_string())?;
            image_digests.push(image_digest);
            if knowledge::vlm_configured() {
                let (ocr, caption) =
                    knowledge::enrichment::describe_image(&image_ref, &image_source_type, &language)
                        .map_err(|error| format!("tender multimodal stage failed: {error}"))?;
                markdown = markdown.replacen(
                    &format!("]({})", image.original_ref),
                    &format!("]({image_ref})\n\n{ocr}\n\n{caption}\n"),
                    1,
                );
            } else if markdown.contains(&image.original_ref) {
                markdown = markdown.replace(&image.original_ref, &image_ref);
            }
        }
        image_digests.sort();
        let mut image_set_hasher = Sha256::new();
        image_set_hasher.update(b"ConvertedSourceArtifactV1:image-set:");
        for digest in image_digests {
            image_set_hasher.update(digest.as_bytes());
        }
        let image_asset_set_sha256 = hex::encode(image_set_hasher.finalize());
        let sections = outline_and_route(&markdown)?;
        let source_artifact_id = Uuid::new_v4();
        let completed = crate::bidding::complete_document_conversion(
            pool,
            CompleteDocumentConversion {
                document_id,
                conversion_generation,
                source_artifact_id,
                markdown: markdown.as_bytes(),
                converter_contract_version: "docparser-converted-source-v1",
                image_asset_set_sha256: &image_asset_set_sha256,
                image_assets: &image_uploads,
                extraction_target_id: target_id,
                expected_section_count: sections.len() as i32,
                policy_version: POLICY_VERSION,
                prompt_version: PROMPT_VERSION,
                actor: CONVERSION_OBJECT_ACTOR,
            },
        )
        .await
        .map_err(|error| error.to_string())?;
        if completed["source_artifact_id"] != source_artifact_id.to_string()
            || completed["extraction_target_id"] != target_id.to_string()
        {
            return Err("conversion publication identity mismatch".into());
        }
        let extraction_generation = completed
            .get("extraction_generation")
            .and_then(Value::as_i64)
            .ok_or_else(|| "conversion publication is missing extraction generation".to_string())?;
        Ok((target_id, extraction_generation))
    }
    .await;
    match conversion {
        Ok(successor) => Ok(Some(successor)),
        Err(error) => {
            for image in &image_uploads {
                let _ =
                    platform::abandon_object_upload(pool, image.staging_id, CONVERSION_OBJECT_ACTOR)
                        .await;
            }
            let retry = super::conversion_error_is_retryable(&error);
            let settled = crate::bidding::fail_document_conversion(
                pool,
                document_id,
                conversion_generation,
                if retry {
                    "CONVERSION_TRANSIENT"
                } else {
                    "CONVERSION_TERMINAL"
                },
                &error,
                retry,
            )
            .await
            .map_err(|settle_error| settle_error.to_string())?;
            if !settled {
                return crate::bidding::document_conversion_successor(
                    pool,
                    document_id,
                    conversion_generation,
                )
                .await
                .map_err(|successor_error| successor_error.to_string())
                .map(|successor| {
                    successor.map(|successor| {
                        (
                            successor.target_id,
                            i64::from(successor.extraction_generation),
                        )
                    })
                });
            }
            if retry { Err(error) } else { Ok(None) }
        }
    }
}

pub async fn run_extraction_target(
    pool: &sqlx::PgPool,
    target_id: Uuid,
    target_revision: i64,
) -> Result<(), String> {
    let extraction_generation = i32::try_from(target_revision)
        .map_err(|_| "extraction target revision is invalid".to_string())?;
    let Some(target) = crate::bidding::load_extraction(pool, target_id, extraction_generation)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(());
    };
    let extraction = async {
        let source = crate::bidding::extraction_source(pool, target.document_id)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "frozen extraction source missing".to_string())?;
        if source.source_artifact_id != target.source_artifact_id
            || source.conversion_generation != target.conversion_generation
            || hex::encode(Sha256::digest(&source.markdown)) != source.markdown_sha256
        {
            return Err("frozen extraction source identity mismatch".to_string());
        }
        let markdown = String::from_utf8(source.markdown)
            .map_err(|_| "converted source is not UTF-8".to_string())?;
        let sections = outline_and_route(&markdown)?;
        for section in &sections {
            let graph = section.candidate_graph();
            let expected_current_publication_id = crate::bidding::current_section_publication(
                pool,
                target.project_id,
                target.document_id,
                &section.key,
            )
            .await
            .map_err(|error| error.to_string())?;
            let request = PublicationRequest {
                target_id,
                target_revision: extraction_generation,
                section_key: &section.key,
                parent_start_offset: section.parent_start_offset,
                parent_end_offset: section.parent_end_offset,
                expected_current_publication_id,
                candidate_graph: &graph,
            };
            let context = MutationContext::new(
                "system:bid-extraction-worker",
                format!("{target_id}:{}", section.key),
                &request,
            )
            .map_err(|error| error.to_string())?;
            crate::bidding::publish_extraction_section(
                pool,
                PublishSection {
                    target_id,
                    extraction_generation,
                    section_key: &section.key,
                    heading_path: &json!(section.heading_path),
                    parent_start_offset: section.parent_start_offset as i64,
                    parent_end_offset: section.parent_end_offset as i64,
                    expected_current_publication_id,
                    candidate_graph: &graph,
                },
                &context,
            )
            .await
            .map_err(|error| error.to_string())?;
        }
        Ok::<(), String>(())
    }
    .await;
    match extraction {
        Ok(()) => Ok(()),
        Err(error) => {
            let retry = super::conversion_error_is_retryable(&error);
            let settled = crate::bidding::fail_extraction(
                pool,
                target_id,
                extraction_generation,
                if retry {
                    "EXTRACTION_TRANSIENT"
                } else {
                    "EXTRACTION_TERMINAL"
                },
                &error,
                retry,
            )
            .await
            .map_err(|settle_error| settle_error.to_string())?;
            if !settled {
                return Ok(());
            }
            if retry { Err(error) } else { Ok(()) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_router_golden_contract() {
        let cases = [
            ("系统应提供支付接口和密码认证", ClauseKind::Technical),
            ("验收后付款30%", ClauseKind::SchedulePayment),
            ("设备到货期为30天", ClauseKind::ScheduleDelivery),
            ("提交合同复印件并加盖公章", ClauseKind::Qualification),
            ("投标函应签字盖章", ClauseKind::Procedural),
            ("提供驻场7x24服务", ClauseKind::Service),
        ];
        for (text, expected) in cases {
            assert_eq!(route_kind(text).kind, expected, "{text}");
        }
        let conflict = route_kind("分项报价作为评分项，按得分权重计算");
        assert_eq!(conflict.kind, ClauseKind::Pricing);
        assert_eq!(conflict.reason_code, "PRICING_EVALUATION_CONFLICT");
    }

    #[test]
    fn chinese_offsets_are_utf8_bytes_and_replay_exactly() {
        let markdown = "# 技术要求\n系统必须支持国密协议。\n预算为100.00元。";
        let sections = outline_and_route(markdown).unwrap();
        let clause = sections
            .iter()
            .flat_map(|section| &section.segments)
            .find(|segment| segment.quote.contains("国密"))
            .unwrap();
        assert!(clause.start_offset > markdown[..clause.start_offset].chars().count());
        verify_utf8_span(
            markdown.as_bytes(),
            clause.start_offset,
            clause.end_offset,
            &clause.quote,
        )
        .unwrap();
        assert!(
            verify_utf8_span(
                markdown.as_bytes(),
                clause.start_offset + 1,
                clause.end_offset,
                &clause.quote
            )
            .is_err()
        );
    }

    #[test]
    fn dotted_numeric_headings_preserve_their_full_hierarchy() {
        let markdown =
            "1. 技术要求\n正文说明。\n1.2 服务要求\n服务正文。\n1.2.3 性能指标\n性能正文。";

        let sections = outline_and_route(markdown).unwrap();
        let heading_paths: Vec<_> = sections
            .iter()
            .map(|section| section.heading_path.as_slice())
            .collect();

        assert_eq!(
            heading_paths,
            vec![
                &["1. 技术要求".to_string()][..],
                &["1. 技术要求".to_string(), "1.2 服务要求".to_string()][..],
                &[
                    "1. 技术要求".to_string(),
                    "1.2 服务要求".to_string(),
                    "1.2.3 性能指标".to_string()
                ][..]
            ]
        );
    }

    #[test]
    fn numbered_requirements_and_real_headings_are_distinguished_in_both_numbering_styles() {
        let markdown = concat!(
            "1. 系统应提供双电源\n",
            "2. 提供营业执照复印件\n",
            "二、提交近三年审计报告\n",
            "5. 投标人须在截止日前提交营业执照\n",
            "6. 法定代表人须签字\n",
            "3. 应急预案\n",
            "3.1 需求分析\n",
            "4. 系统应用架构\n",
            "二、应对方案\n",
            "三、技术要求\n",
            "系统应支持国密协议。",
        );

        let sections = outline_and_route(markdown).unwrap();

        assert_eq!(sections.len(), 6);
        assert!(sections[0].heading_path.is_empty());
        let clause_quotes = sections[0]
            .segments
            .iter()
            .filter_map(|segment| match segment.disposition {
                SegmentDisposition::Clause { .. } => Some(segment.quote.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(clause_quotes.contains(&"1. 系统应提供双电源"));
        assert!(clause_quotes.contains(&"2. 提供营业执照复印件"));
        assert!(clause_quotes.contains(&"二、提交近三年审计报告"));
        assert!(clause_quotes.contains(&"5. 投标人须在截止日前提交营业执照"));
        assert!(clause_quotes.contains(&"6. 法定代表人须签字"));
        assert_eq!(sections[1].heading_path, vec!["3. 应急预案".to_string()]);
        assert_eq!(
            sections[2].heading_path,
            vec!["3. 应急预案".to_string(), "3.1 需求分析".to_string()]
        );
        assert_eq!(
            sections[3].heading_path,
            vec!["4. 系统应用架构".to_string()]
        );
        assert_eq!(sections[4].heading_path, vec!["二、应对方案".to_string()]);
        assert_eq!(sections[5].heading_path, vec!["三、技术要求".to_string()]);
    }

    #[test]
    fn numbered_action_requirements_are_preserved_as_clauses_instead_of_headings() {
        let markdown = concat!(
            "2. 投标文件格式符合招标要求\n",
            "3. 产品满足性能要求\n",
            "4. 系统达到性能要求\n",
            "5. 投标人应当遵守保密要求",
        );

        let sections = outline_and_route(markdown).unwrap();

        assert_eq!(sections.len(), 1);
        assert!(sections[0].heading_path.is_empty());
        let clause_quotes = sections[0]
            .segments
            .iter()
            .filter_map(|segment| match segment.disposition {
                SegmentDisposition::Clause { .. } => Some(segment.quote.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            clause_quotes,
            vec![
                "2. 投标文件格式符合招标要求",
                "3. 产品满足性能要求",
                "4. 系统达到性能要求",
                "5. 投标人应当遵守保密要求",
            ]
        );
    }

    #[test]
    fn numbered_elliptical_requirements_are_preserved_as_clauses_instead_of_headings() {
        let markdown = concat!(
            "2. ISO 9001认证资质\n",
            "3. 近三年类似项目业绩\n",
            "4. 需具备独立法人资格\n",
            "5. 具备独立法人资格\n",
            "6. 产品采用国产数据库",
        );

        let sections = outline_and_route(markdown).unwrap();

        assert_eq!(sections.len(), 1);
        assert!(sections[0].heading_path.is_empty());
        let clause_quotes = sections[0]
            .segments
            .iter()
            .filter_map(|segment| match segment.disposition {
                SegmentDisposition::Clause { .. } => Some(segment.quote.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            clause_quotes,
            vec![
                "2. ISO 9001认证资质",
                "3. 近三年类似项目业绩",
                "4. 需具备独立法人资格",
                "5. 具备独立法人资格",
                "6. 产品采用国产数据库",
            ]
        );
    }

    #[test]
    fn configured_numbered_heading_hints_define_outline_boundaries() {
        let markdown = concat!(
            "1. 采购需求\n",
            "2. 技术规格\n",
            "3. 技术参数\n",
            "4. 资格审查\n",
            "5. 商务条款\n",
            "6. 注册资本\n",
            "7. 类似项目\n",
            "8. 服务能力",
        );

        let sections = outline_and_route(markdown).unwrap();
        let heading_paths = sections
            .iter()
            .map(|section| section.heading_path.as_slice())
            .collect::<Vec<_>>();

        assert_eq!(
            heading_paths,
            vec![
                &["1. 采购需求".to_string()][..],
                &["2. 技术规格".to_string()][..],
                &["3. 技术参数".to_string()][..],
                &["4. 资格审查".to_string()][..],
                &["5. 商务条款".to_string()][..],
                &["6. 注册资本".to_string()][..],
                &["7. 类似项目".to_string()][..],
                &["8. 服务能力".to_string()][..],
            ]
        );
    }

    #[test]
    fn amount_facts_bind_to_their_own_field_context_and_accept_grouping() {
        let markdown = "预算1,000,000.00元，最高限价900,000.00元。";

        let sections = outline_and_route(markdown).unwrap();
        let facts = &sections[0].segments[0].facts;
        let budget = facts
            .iter()
            .find(|fact| fact.field == "budget_amount")
            .unwrap();
        let ceiling = facts
            .iter()
            .find(|fact| fact.field == "ceiling_price")
            .unwrap();

        assert_eq!(
            budget.typed_value,
            json!({"amount":"1000000.00","currency_code":"CNY"})
        );
        assert_eq!(
            ceiling.typed_value,
            json!({"amount":"900000.00","currency_code":"CNY","basis":"unspecified"})
        );
    }

    #[test]
    fn source_span_v2_rejects_scope_generation_digest_and_unknown_keys() {
        let source = "# 技术要求\n系统必须支持国密协议。";
        let quote = "系统必须支持国密协议。";
        let start = source.find(quote).unwrap();
        let end = start + quote.len();
        let source_artifact_id = Uuid::new_v4();
        let section_artifact_id = Uuid::new_v4();
        let project_id = Uuid::new_v4();
        let document_id = Uuid::new_v4();
        let heading_path = vec!["技术要求".to_string()];
        let span = SourceSpanV2 {
            schema_version: 2,
            source_artifact_id,
            section_artifact_id,
            project_id,
            document_id,
            conversion_generation: 3,
            section_key: "section:technical".into(),
            parent_start_offset: 0,
            parent_end_offset: source.len() as i64,
            start_offset: start as i64,
            end_offset: end as i64,
            offset_unit: "utf8_byte".into(),
            quote: quote.into(),
            quote_sha256: hex::encode(Sha256::digest(quote.as_bytes())),
            heading_path: heading_path.clone(),
        };
        let scope = || SourceSpanScope {
            source: source.as_bytes(),
            source_artifact_id,
            section_artifact_id,
            project_id,
            document_id,
            conversion_generation: 3,
            section_key: "section:technical",
            parent_start_offset: 0,
            parent_end_offset: source.len(),
            heading_path: &heading_path,
        };

        verify_source_span_v2(&span, scope()).unwrap();

        let mut wrong_generation = span.clone();
        wrong_generation.conversion_generation += 1;
        assert_eq!(
            verify_source_span_v2(&wrong_generation, scope()),
            Err("source span frozen scope mismatch")
        );

        let mut wrong_section = span.clone();
        wrong_section.section_artifact_id = Uuid::new_v4();
        assert_eq!(
            verify_source_span_v2(&wrong_section, scope()),
            Err("source span frozen scope mismatch")
        );

        let mut wrong_digest = span.clone();
        wrong_digest.quote_sha256 = "0".repeat(64);
        assert_eq!(
            verify_source_span_v2(&wrong_digest, scope()),
            Err("source span quote digest mismatch")
        );

        let mut encoded = serde_json::to_value(&span).unwrap();
        encoded
            .as_object_mut()
            .unwrap()
            .insert("live_document_id".into(), json!(Uuid::new_v4()));
        assert!(serde_json::from_value::<SourceSpanV2>(encoded).is_err());
    }

    #[test]
    fn every_routed_segment_has_one_disposition_and_fact_can_coexist() {
        let markdown = "# 商务要求\n最高限价1000.00元，报价不得超过限价。\n本段只是说明。";
        let sections = outline_and_route(markdown).unwrap();
        let segments: Vec<_> = sections
            .iter()
            .flat_map(|section| &section.segments)
            .collect();
        assert!(!segments.is_empty());
        assert!(segments.iter().any(|segment| {
            !segment.facts.is_empty()
                && matches!(segment.disposition, SegmentDisposition::Clause { .. })
        }));
        for section in sections {
            assert_eq!(
                section.candidate_graph().as_array().unwrap().len(),
                section.segments.len()
            );
        }
    }

    #[test]
    fn family_is_only_derived_from_kind() {
        assert_eq!(ClauseKind::Technical.family(), Some("technical"));
        assert_eq!(ClauseKind::Qualification.family(), Some("commercial"));
        assert_eq!(ClauseKind::Service.family(), Some("commercial"));
        for kind in [
            ClauseKind::Pricing,
            ClauseKind::ScheduleDelivery,
            ClauseKind::SchedulePayment,
            ClauseKind::Evaluation,
            ClauseKind::Procedural,
        ] {
            assert_eq!(kind.family(), None);
        }
    }
}
