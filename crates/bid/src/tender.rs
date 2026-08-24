//! Final V1 bounded tender segmentation, fact proposals, and KindRouter.

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use storage::bidding::{
    CompleteDocumentConversion, ConvertedSourceImageUpload, MutationContext, PublishSection,
};
use uuid::Uuid;

pub const ROUTER_VERSION: &str = "kind-router-v1";
pub const POLICY_VERSION: &str = "requirement-span-v1+fact-suggestion-v1";
pub const PROMPT_VERSION: &str = "bounded-tender-publication-v1";

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
    let numeric_prefix = trimmed
        .split_once(['.', '、'])
        .is_some_and(|(prefix, rest)| {
            !rest.trim().is_empty() && prefix.split('.').all(|p| p.parse::<u16>().is_ok())
        });
    if numeric_prefix {
        let level = trimmed
            .split_once(['.', '、'])
            .map(|(prefix, _)| prefix.matches('.').count() + 1)
            .unwrap_or(1);
        return Some((level.min(6), trimmed.to_string()));
    }
    let mut chars = trimmed.chars();
    if chars
        .next()
        .is_some_and(|first| "一二三四五六七八九十".contains(first))
        && chars.next() == Some('、')
    {
        return Some((1, trimmed.to_string()));
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
    let requirement_signal = contains_any(
        trimmed,
        &["必须", "应当", "应", "须", "不得", "要求", "提供", "提交"],
    ) || contains_any(
        trimmed,
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
    );
    if requirement_signal {
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
    let amount = Regex::new(r"(?P<amount>[0-9]{1,18}(?:\.[0-9]{1,2})?)\s*(?:元|人民币)")
        .expect("static regex");
    if let Some(capture) = amount.captures(text) {
        let amount = normalize_cny(&capture["amount"]);
        if text.contains("预算") {
            facts.push(FactProposal {
                field: "budget_amount",
                typed_value: json!({"amount":amount,"currency_code":"CNY"}),
                confidence: "0.9500".into(),
            });
        }
        if contains_any(text, &["最高限价", "控制价", "限价"]) {
            facts.push(FactProposal {
                field: "ceiling_price",
                typed_value: json!({"amount":amount,"currency_code":"CNY","basis":"unspecified"}),
                confidence: "0.9500".into(),
            });
        }
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

fn normalize_cny(value: &str) -> String {
    let mut parts = value.split('.');
    let whole = parts.next().unwrap_or("0").trim_start_matches('0');
    let whole = if whole.is_empty() { "0" } else { whole };
    let fraction = parts.next().unwrap_or("");
    format!("{whole}.{fraction:0<2}")
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
    attempt: i32,
    section_key: &'a str,
    parent_start_offset: usize,
    parent_end_offset: usize,
    expected_current_publication_id: Option<Uuid>,
    candidate_graph: &'a Value,
}

pub async fn convert_and_schedule_document(
    pool: &sqlx::PgPool,
    document_id: Uuid,
) -> Result<Option<Uuid>, String> {
    let claim_token = Uuid::new_v4();
    let Some(claim) = storage::bidding::claim_document_conversion(
        pool,
        document_id,
        claim_token,
        "bid-convert-v1",
    )
    .await
    .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let original_digest = claim.object_ref.trim_start_matches("objects/");
    let bytes = storage::read_blob(original_digest).map_err(|error| error.to_string())?;
    const CONVERSION_OBJECT_ACTOR: &str = "system:bid-convert-worker";
    let mut image_uploads: Vec<ConvertedSourceImageUpload> = Vec::new();
    let conversion = async {
        let converted = docparser::convert_to_markdown(&claim.file_name, bytes)
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
                enrichment::image_source_type(&claim.file_name, &markdown).to_string()
            });
        let language =
            enrichment::infer_output_language(&format!("{}\n{markdown}", claim.file_name));
        let mut image_digests = Vec::new();
        for image in converted.images {
            if image.data.is_empty() {
                continue;
            }
            if !storage::bidding::heartbeat_document_conversion(pool, document_id, claim_token)
                .await
                .map_err(|error| error.to_string())?
            {
                return Err("document conversion lease lost".into());
            }
            let image_digest = hex::encode(Sha256::digest(&image.data));
            let image_ref = storage::object_ref(&image_digest);
            let media_type = image.mime_type.trim();
            if !media_type.starts_with("image/") {
                return Err("converted image media type is missing or invalid".into());
            }
            let staging_id = Uuid::new_v4();
            storage::stage_object_upload(
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
            storage::write_blob_off_runtime(&image_digest, &image.data)
                .map_err(|error| error.to_string())?;
            image_digests.push(image_digest);
            if domain::vlm_configured() {
                let (ocr, caption) =
                    enrichment::describe_image(&image_ref, &image_source_type, &language)
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
        storage::bidding::complete_document_conversion(
            pool,
            CompleteDocumentConversion {
                document_id,
                claim_token,
                source_artifact_id,
                markdown: markdown.as_bytes(),
                converter_contract_version: "docparser-converted-source-v1",
                image_asset_set_sha256: &image_asset_set_sha256,
                image_assets: &image_uploads,
                actor: CONVERSION_OBJECT_ACTOR,
            },
        )
        .await
        .map_err(|error| error.to_string())?;
        let target_id = Uuid::new_v4();
        let schedule_payload = json!({
            "schema_version":1,
            "target_id":target_id,
            "document_id":document_id,
            "source_artifact_id":source_artifact_id,
            "expected_section_count":sections.len(),
            "policy_version":POLICY_VERSION,
            "prompt_version":PROMPT_VERSION
        });
        let context = MutationContext::new(
            "system:bid-convert-worker",
            format!("{document_id}:{}", claim.conversion_generation),
            &schedule_payload,
        )
        .map_err(|error| error.to_string())?;
        storage::bidding::schedule_extraction(
            pool,
            target_id,
            document_id,
            sections.len() as i32,
            POLICY_VERSION,
            PROMPT_VERSION,
            &context,
        )
        .await
        .map_err(|error| error.to_string())?;
        Ok(target_id)
    }
    .await;
    match conversion {
        Ok(target_id) => Ok(Some(target_id)),
        Err(error) => {
            for image in &image_uploads {
                let _ =
                    storage::abandon_object_upload(pool, image.staging_id, CONVERSION_OBJECT_ACTOR)
                        .await;
            }
            let retry = super::conversion_error_is_retryable(&error);
            let _ = storage::bidding::fail_document_conversion(
                pool,
                document_id,
                claim_token,
                if retry {
                    "CONVERSION_TRANSIENT"
                } else {
                    "CONVERSION_TERMINAL"
                },
                retry,
            )
            .await;
            Err(error)
        }
    }
}

pub async fn run_extraction_target(
    pool: &sqlx::PgPool,
    target_id: Uuid,
    expected_project_id: Uuid,
    expected_document_id: Option<Uuid>,
) -> Result<(), String> {
    let claim_token = Uuid::new_v4();
    let Some(claim) =
        storage::bidding::claim_extraction(pool, target_id, claim_token, "bid-extract-v1")
            .await
            .map_err(|error| error.to_string())?
    else {
        return Ok(());
    };
    if claim.project_id != expected_project_id
        || expected_document_id.is_some_and(|document_id| document_id != claim.document_id)
    {
        let _ = storage::bidding::fail_extraction(
            pool,
            target_id,
            claim.attempt,
            claim_token,
            "EXTRACTION_SCOPE_MISMATCH",
            false,
        )
        .await;
        return Err("extraction target scope mismatch".into());
    }
    let source = storage::bidding::extraction_source(pool, claim.document_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "frozen extraction source missing".to_string())?;
    if source.source_artifact_id != claim.source_artifact_id
        || source.conversion_generation != claim.conversion_generation
        || hex::encode(Sha256::digest(&source.markdown)) != source.markdown_sha256
    {
        return Err("frozen extraction source identity mismatch".into());
    }
    let markdown =
        String::from_utf8(source.markdown).map_err(|_| "converted source is not UTF-8")?;
    let sections = outline_and_route(&markdown)?;
    for section in &sections {
        if !storage::bidding::heartbeat_extraction(pool, target_id, claim_token, claim.attempt)
            .await
            .map_err(|error| error.to_string())?
        {
            return Err("extraction lease lost".into());
        }
        let graph = section.candidate_graph();
        let expected_current_publication_id = storage::bidding::current_section_publication(
            pool,
            claim.project_id,
            claim.document_id,
            &section.key,
        )
        .await
        .map_err(|error| error.to_string())?;
        let request = PublicationRequest {
            target_id,
            attempt: claim.attempt,
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
        storage::bidding::publish_extraction_section(
            pool,
            PublishSection {
                target_id,
                attempt: claim.attempt,
                claim_token,
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
    Ok(())
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
