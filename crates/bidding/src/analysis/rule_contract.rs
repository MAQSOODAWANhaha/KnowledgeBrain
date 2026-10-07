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
    #[serde(default)]
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

/// Allocate only new blank identities. Explicit IDs refer to the previous saved
/// revision; reordering never reassigns them and deletion never resets the host counter.
pub fn assign_item_ids(
    items: &mut [RuleItem],
    previous: &[RuleItem],
    last_id: u64,
) -> Result<u64, String> {
    let mut next = previous
        .iter()
        .filter_map(|item| item.id.strip_prefix('i')?.parse::<u64>().ok())
        .fold(last_id, u64::max);
    let mut seen = std::collections::BTreeSet::new();
    for (index, item) in items.iter().enumerate() {
        if item.id.trim().is_empty() {
            continue;
        }
        if !previous.iter().any(|old| old.id == item.id) || !seen.insert(item.id.clone()) {
            return Err(super::tools::field_error(
                &format!("/data/items/{index}/id"),
                "use a saved item ID once, or an empty string to allocate a new identity",
            ));
        }
    }
    for item in items.iter_mut() {
        if item.id.trim().is_empty() {
            next = next
                .checked_add(1)
                .ok_or("rule item identity sequence exhausted")?;
            item.id = format!("i{next}");
        }
    }
    Ok(next)
}

pub fn contract_sha256() -> Result<String, String> {
    digest(&json!({"version":2,"keys":ANALYSIS_GLOBAL_CHECK_KEYS,
        "evidence":"role-local-current-candidates-and-originals",
        "finding_identity":"content-sha256"}))
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
    let mut seen = std::collections::BTreeSet::new();
    for check in checks {
        if !seen.insert(&check.key) {
            return Err(format!("duplicate global check {}", check.key));
        }
        validate_check(input, analysis, check, findings)?;
    }
    if seen.len() != ANALYSIS_GLOBAL_CHECK_KEYS.len() {
        return Err("global checks incomplete".into());
    }
    Ok(())
}

pub fn validate_check(
    input: &FrozenInput,
    analysis: &Analysis,
    check: &GlobalCheck,
    findings: &[Finding],
) -> Result<(), String> {
    if !ANALYSIS_GLOBAL_CHECK_KEYS.contains(&check.key.as_str()) {
        return Err(format!("unknown global check {}", check.key));
    }
    if check.scope_sha256 != scope_sha256(input, analysis)? {
        return Err(format!("stale global check scope {}", check.key));
    }
    for ids in [&check.record_ids, &check.finding_ids] {
        if ids.iter().collect::<std::collections::BTreeSet<_>>().len() != ids.len() {
            return Err("global check references must be distinct".into());
        }
    }
    for id in &check.record_ids {
        if !analysis.records.contains_key(id) {
            return Err(format!("unknown global check record {id}"));
        }
    }
    if (check.conclusion == GlobalCheckConclusion::Findings) != !check.finding_ids.is_empty() {
        return Err(
            "findings conclusion requires saved findings; other conclusions cannot cite findings"
                .into(),
        );
    }
    for id in &check.finding_ids {
        if !findings
            .iter()
            .any(|finding| digest(finding).ok().as_ref() == Some(id))
        {
            return Err(format!("unknown or changed global check finding {id}"));
        }
    }
    if check.conclusion == GlobalCheckConclusion::SourceLimited
        && !has_source_limitation(input, analysis, check)
    {
        return Err("source_limited requires a cited saved source uncertainty or a frozen unavailable document; missing graph work is not missing source".into());
    }
    Ok(())
}

fn has_source_limitation(input: &FrozenInput, analysis: &Analysis, check: &GlobalCheck) -> bool {
    input
        .documents
        .iter()
        .any(|d| d["disposition"].as_str().is_some_and(|s| s != "ready"))
        || check.grounds.iter().any(|span| {
            analysis
                .dispositions
                .get(&span.source_id)
                .is_some_and(|d| d.state == DispositionState::Unresolved)
        })
        || check
            .record_ids
            .iter()
            .any(|id| match &analysis.records[id].data {
                RecordData::Unresolved { .. } => true,
                RecordData::Requirement {
                    strength,
                    compliance,
                    applicability,
                    ..
                } => {
                    *strength == Strength::Unknown
                        || applicability.state == ApplicabilityState::Unknown
                        || compliance.iter().any(|c| c.policy == Compliance::Unknown)
                }
                RecordData::Rule { applicability, .. }
                | RecordData::Template { applicability, .. } => {
                    applicability.state == ApplicabilityState::Unknown
                }
                RecordData::Fact { .. } => false,
            })
}

/// Delivery is role-local; valid source locations alone do not establish it.
pub fn validate_evidence(
    input: &FrozenInput,
    analysis: &Analysis,
    coverage: &Coverage,
    check: &GlobalCheck,
) -> Result<(), String> {
    for span in &check.grounds {
        tools::validate_span(input, coverage, span)?;
    }
    if check.grounds.is_empty() {
        let frozen_missing = input
            .documents
            .iter()
            .any(|d| d["disposition"].as_str().is_some_and(|s| s != "ready"));
        if (!input.source_units.is_empty() || !analysis.records.is_empty())
            && check.key != "collection_consistency"
            && !(check.conclusion == GlobalCheckConclusion::SourceLimited && frozen_missing)
        {
            return Err("global check needs independently read original grounds".into());
        }
        if !input.documents.is_empty() {
            if !tools::contains(coverage.metadata.get("documents"), 0, input.documents.len()) {
                return Err(
                    "read the frozen document inventory before a metadata-only global check".into(),
                );
            }
        } else if !input.source_units.is_empty() || !analysis.records.is_empty() {
            return Err("global check needs independently read original grounds".into());
        }
    }
    for id in &check.record_ids {
        let record = analysis
            .records
            .get(id)
            .ok_or("unknown global check record")?;
        if coverage.candidate.get(&format!("record:{id}")) != Some(&digest(record)?) {
            return Err(format!(
                "independently inspect the current global check record:{id}"
            ));
        }
    }
    Ok(())
}

pub fn validate_review(
    input: &FrozenInput,
    analysis: &Analysis,
    review: &Review,
) -> Result<(), String> {
    if review.contract_sha256 != contract_sha256()? {
        return Err("analysis global check contract changed or missing".into());
    }
    if review.analysis_sha256 != digest(analysis)? {
        return Err("analysis global review belongs to another graph".into());
    }
    validate_inventory(input, analysis, &review.global_checks, &review.findings)?;
    for check in &review.global_checks {
        if analysis
            .review_global_checks
            .get(&check.key)
            .map(digest)
            .transpose()?
            != Some(digest(check)?)
        {
            return Err("global review does not match the saved reviewer conclusions".into());
        }
        validate_evidence(input, analysis, &review.coverage, check)?;
    }
    Ok(())
}
