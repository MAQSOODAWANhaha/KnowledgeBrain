use regex::Regex;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};
use std::sync::OnceLock;

const MAX_REQUIREMENT_BYTES: usize = 32_768;
const MAX_SEGMENTS_PER_UNIT: usize = 64;

fn numbered_clause_re() -> &'static Regex {
    static VALUE: OnceLock<Regex> = OnceLock::new();
    VALUE.get_or_init(|| {
        Regex::new(r"(?m)(?:^|[\s。；;])(?:第[一二三四五六七八九十百]+[章节部分]|\d+(?:\.\d+){1,4})[、.．]?\s*[^。；;\n]{0,96}")
            .expect("numbered requirement clause regex")
    })
}

fn contains_any(text: &str, values: &[&str]) -> bool {
    values.iter().any(|value| text.contains(value))
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

fn atomic_segments(text: &str) -> Vec<String> {
    let normalized = text
        .replace('\u{00a0}', " ")
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let mut boundaries = numbered_clause_re()
        .find_iter(&normalized)
        .map(|matched| {
            let raw = &normalized[matched.start()..matched.end()];
            matched.start()
                + raw
                    .char_indices()
                    .find(|(_, character)| {
                        !character.is_whitespace() && !matches!(character, '。' | '；' | ';')
                    })
                    .map(|(offset, _)| offset)
                    .unwrap_or(0)
        })
        .collect::<Vec<_>>();
    boundaries.sort_unstable();
    boundaries.dedup();

    let mut values = Vec::new();
    if boundaries.len() >= 2 {
        if boundaries[0] > 0 {
            let prefix = normalized[..boundaries[0]].trim();
            if prefix.chars().count() >= 8 {
                values.push(prefix.to_owned());
            }
        }
        for (index, start) in boundaries.iter().copied().enumerate() {
            let end = boundaries
                .get(index + 1)
                .copied()
                .unwrap_or(normalized.len());
            let value = normalized[start..end].trim();
            if value.chars().count() >= 8 {
                values.push(value.to_owned());
            }
        }
    } else {
        values.extend(normalized.split('\n').flat_map(|line| {
            if line.len() > 4_096 {
                line.split_inclusive(['。', '；'])
                    .map(str::trim)
                    .filter(|value| value.chars().count() >= 8)
                    .map(ToOwned::to_owned)
                    .collect::<Vec<_>>()
            } else {
                let value = line.trim();
                if value.chars().count() >= 8 {
                    vec![value.to_owned()]
                } else {
                    Vec::new()
                }
            }
        }));
    }
    if values.is_empty() && !normalized.trim().is_empty() {
        values.push(normalized.trim().to_owned());
    }
    values
        .into_iter()
        .take(MAX_SEGMENTS_PER_UNIT)
        .map(|value| truncate_utf8(&value, MAX_REQUIREMENT_BYTES))
        .collect()
}

fn applicability(text: &str) -> (&'static str, Option<&'static str>) {
    if contains_any(text, &["本次不适用", "不适用", "无需提供"]) {
        ("not_applicable", Some("招标文件明确标记为不适用"))
    } else if contains_any(text, &["如有", "若有", "视情况", "如适用"]) {
        ("conditional", Some("招标文件将该义务标记为条件适用"))
    } else if contains_any(text, &["必须", "应当", "须", "不得", "必须提供", "需提供"])
    {
        ("required", None)
    } else {
        ("optional", None)
    }
}

fn requirement_kind(text: &str, has_form: bool) -> &'static str {
    if contains_any(text, &["报价", "价格", "价款", "税率", "开标价"]) {
        "pricing"
    } else if contains_any(text, &["评标", "评审", "评分", "得分"]) {
        "evaluation"
    } else if contains_any(
        text,
        &[
            "交付", "交货", "工期", "进度", "安装", "调试", "验收", "实施",
        ],
    ) {
        "delivery"
    } else if contains_any(
        text,
        &[
            "技术",
            "参数",
            "规格",
            "性能",
            "设备",
            "产品",
            "标准",
            "售后",
            "质保",
            "供货范围",
        ],
    ) {
        "technical"
    } else if contains_any(
        text,
        &[
            "资格", "资质", "证书", "业绩", "财务", "信用", "失信", "股权",
        ],
    ) {
        "qualification"
    } else if contains_any(
        text,
        &[
            "商务",
            "投标函",
            "授权委托",
            "保证金",
            "履约",
            "合同",
            "付款",
            "廉洁",
        ],
    ) {
        "commercial"
    } else if has_form {
        "format"
    } else {
        "other"
    }
}

fn channel(text: &str, has_form: bool) -> &'static str {
    if contains_any(text, &["偏差表", "偏差声明", "技术偏差", "商务偏差"]) {
        "deviation_statement"
    } else if contains_any(text, &["报价", "价格表", "开标价", "分项价格"]) {
        "quotation"
    } else if has_form {
        "structured_form"
    } else if contains_any(text, &["逐条响应", "响应表", "技术参数表", "条款响应"])
    {
        "response_table"
    } else if contains_any(
        text,
        &[
            "证明文件",
            "证书",
            "截图",
            "资质材料",
            "业绩证明",
            "财务报表",
        ],
    ) {
        "evidence_attachment"
    } else {
        "narrative_content"
    }
}

fn requirement_ref(
    source_unit_revision_id: &str,
    ordinal: usize,
    text: &str,
    channel: &str,
) -> String {
    let mut digest = Sha256::new();
    digest.update(b"requirement-compile-v3\0");
    digest.update(source_unit_revision_id.as_bytes());
    digest.update(b"\0");
    digest.update(ordinal.to_string().as_bytes());
    digest.update(b"\0");
    digest.update(channel.as_bytes());
    digest.update(b"\0");
    digest.update(text.as_bytes());
    hex::encode(digest.finalize())
}

pub(crate) fn compile_requirement_input_v3(input: &Value) -> Result<Value, String> {
    let source_units = input
        .get("source_units")
        .and_then(Value::as_array)
        .ok_or_else(|| "requirement compile source_units missing".to_owned())?;
    if source_units.is_empty() {
        return Err("requirement compile source_units empty".into());
    }
    let forms_by_source = input
        .get("structured_forms")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|form| {
            Some((
                form.get("source_unit_revision_id")?.as_str()?.to_owned(),
                form.get("form_definition_revision_id")?
                    .as_str()?
                    .to_owned(),
            ))
        })
        .fold(
            HashMap::<String, Vec<String>>::new(),
            |mut values, (source, form)| {
                values.entry(source).or_default().push(form);
                values
            },
        );

    let mut covered_sources = BTreeSet::new();
    let mut requirements = Vec::new();
    for source in source_units {
        let source_id = source
            .get("source_unit_revision_id")
            .and_then(Value::as_str)
            .ok_or_else(|| "requirement compile source id invalid".to_owned())?;
        let text = source
            .get("text")
            .and_then(Value::as_str)
            .ok_or_else(|| "requirement compile source text invalid".to_owned())?;
        covered_sources.insert(source_id.to_owned());
        let form_ids = forms_by_source.get(source_id).cloned().unwrap_or_default();
        let has_form = !form_ids.is_empty();
        let segments = if has_form {
            vec![truncate_utf8(text.trim(), MAX_REQUIREMENT_BYTES)]
        } else {
            atomic_segments(text)
        };
        for (ordinal, segment) in segments.into_iter().enumerate() {
            if segment.is_empty() {
                continue;
            }
            let (status, reason) = applicability(&segment);
            let requirement_channel = channel(&segment, has_form);
            let kind = requirement_kind(&segment, has_form);
            let requiredness = if status == "not_applicable" {
                "informational"
            } else if status == "conditional" {
                "optional"
            } else if status == "required" || has_form {
                "mandatory"
            } else {
                "informational"
            };
            let compliance_policy = if kind == "evaluation" {
                "scored"
            } else if requirement_channel == "deviation_statement" {
                "deviation_allowed"
            } else if requiredness == "mandatory" {
                "must_comply"
            } else {
                "explicit_response"
            };
            requirements.push(json!({
                "requirement_ref": requirement_ref(source_id, ordinal, &segment, requirement_channel),
                "requirement_kind": kind,
                "requiredness": requiredness,
                "compliance_policy": compliance_policy,
                "requirement_text": segment,
                "channel": requirement_channel,
                "applicability": {
                    "status": status,
                    "reason": reason,
                    "source_unit_revision_ids": [source_id]
                },
                "source_unit_revision_ids": [source_id],
                "structured_form_revision_ids": form_ids
            }));
        }
    }
    if requirements.is_empty() {
        return Err("requirement compile produced no atomic requirements".into());
    }
    Ok(json!({
        "schema_version": 3,
        "source_unit_revision_ids": covered_sources.into_iter().collect::<Vec<_>>(),
        "requirements": requirements,
        "notices": []
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(text: &str, forms: Value) -> Value {
        json!({
            "source_units": [{
                "source_unit_revision_id": "11111111-1111-1111-1111-111111111111",
                "text": text
            }],
            "structured_forms": forms
        })
    }

    #[test]
    fn composition_clause_is_split_into_atomic_requirements() {
        let value = compile_requirement_input_v3(&input(
            "3.1 投标文件组成 3.1.1 商务文件：必须提供投标函。 3.1.2 技术文件：应逐条响应技术参数表。 3.1.3 报价文件：须提供开标价格表。 3.1.4 其他附件（如有）。",
            json!([]),
        ))
        .unwrap();
        let requirements = value["requirements"].as_array().unwrap();
        assert!(requirements.len() >= 4);
        assert!(
            requirements
                .iter()
                .any(|item| item["channel"] == "quotation")
        );
        assert!(
            requirements
                .iter()
                .any(|item| item["channel"] == "response_table")
        );
    }

    #[test]
    fn forms_and_evidence_use_non_narrative_channels() {
        let source_id = "11111111-1111-1111-1111-111111111111";
        let value = compile_requirement_input_v3(&input(
            "资格审查表：应提供财务报表和资质证明文件",
            json!([{
                "source_unit_revision_id": source_id,
                "form_definition_revision_id": "22222222-2222-2222-2222-222222222222"
            }]),
        ))
        .unwrap();
        assert_eq!(value["requirements"][0]["channel"], "structured_form");
        assert_eq!(
            value["requirements"][0]["structured_form_revision_ids"][0],
            "22222222-2222-2222-2222-222222222222"
        );
    }

    #[test]
    fn not_applicable_requirement_is_informational() {
        let value =
            compile_requirement_input_v3(&input("附件5 联合体协议书（本次不适用）", json!([])))
                .unwrap();
        assert_eq!(
            value["requirements"][0]["applicability"]["status"],
            "not_applicable"
        );
        assert_eq!(value["requirements"][0]["requiredness"], "informational");
    }
}
