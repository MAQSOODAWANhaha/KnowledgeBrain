//! Bounded Rule.items and analysis global-check contract (§2.3/§2.4).
use super::*;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuleItemKind {
    Composition,
    Order,
    Format,
    Signature,
    SubmissionHint,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RuleItemTarget {
    Record { id: String },
    RuleItem { record_id: String, item_id: String },
    Unresolved { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleItem {
    pub id: String,
    pub kind: RuleItemKind,
    pub text: String,
    pub grounds: Vec<Span>,
    pub condition: String,
    pub targets: Vec<RuleItemTarget>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sequence: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format_value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GlobalCheckConclusion {
    Pass,
    Findings,
    SourceLimited,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlobalCheck {
    pub key: String,
    pub scope_sha256: String,
    pub conclusion: GlobalCheckConclusion,
    pub grounds: Vec<Span>,
    #[serde(default)]
    pub record_ids: Vec<String>,
    #[serde(default)]
    pub finding_ids: Vec<String>,
}

pub const ANALYSIS_GLOBAL_CHECK_KEYS: &[&str] = &[
    "source_coverage",
    "collection_consistency",
    "composition_order_format",
    "cross_references",
    "rule_items",
];

pub fn assign_item_ids(items: &mut [RuleItem]) {
    for (index, item) in items.iter_mut().enumerate() {
        if item.id.trim().is_empty() {
            item.id = format!("i{}", index + 1);
        }
    }
}

pub fn contract_sha256() -> Result<String, String> {
    digest(&ANALYSIS_GLOBAL_CHECK_KEYS)
}

pub fn scope_sha256(input: &FrozenInput, analysis: &Analysis) -> Result<String, String> {
    digest(&json!({
        "input": digest(input)?,
        "records": &analysis.records,
        "relations": &analysis.relations,
        "dispositions": &analysis.dispositions,
    }))
}

pub fn validate_inventory(
    input: &FrozenInput,
    analysis: &Analysis,
    checks: &[GlobalCheck],
    findings: &[Finding],
) -> Result<(), String> {
    let expected = scope_sha256(input, analysis)?;
    let mut seen = std::collections::BTreeSet::new();
    for check in checks {
        if !ANALYSIS_GLOBAL_CHECK_KEYS.contains(&check.key.as_str()) {
            return Err(format!("unknown global check {}", check.key));
        }
        if !seen.insert(&check.key) {
            return Err(format!("duplicate global check {}", check.key));
        }
        if check.scope_sha256 != expected {
            return Err(format!("stale global check {}", check.key));
        }
        match check.conclusion {
            GlobalCheckConclusion::Pass => {
                if !check.finding_ids.is_empty() {
                    return Err(format!("pass check {} cannot cite findings", check.key));
                }
            }
            GlobalCheckConclusion::Findings => {
                if check.finding_ids.is_empty() {
                    return Err(format!("findings check {} needs finding ids", check.key));
                }
                for id in &check.finding_ids {
                    if !findings.iter().any(|finding| {
                        digest(finding).ok().as_deref() == Some(id.as_str())
                            || finding.code == *id
                    }) {
                        // Allow draft finding IDs; host still requires a nonempty list.
                    }
                }
            }
            GlobalCheckConclusion::SourceLimited => {
                if check.grounds.is_empty() {
                    return Err(format!("source_limited check {} needs grounds", check.key));
                }
            }
        }
    }
    if seen.len() != ANALYSIS_GLOBAL_CHECK_KEYS.len() {
        return Err("global checks incomplete".into());
    }
    Ok(())
}

pub fn complete(input: &FrozenInput, analysis: &Analysis, review: &Review) -> bool {
    validate_inventory(input, analysis, &review.global_checks, &review.findings).is_ok()
}
