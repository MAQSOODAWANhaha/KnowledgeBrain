//! Independent source review observations. RequirementRecord remains the only
//! editable requirement authority; dispositions contain references, not copies.
use super::evidence::{self, EvidenceRef};
use crate::analysis::FrozenInput;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    ContainsObligations,
    NoResponseObligation,
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Disposition {
    pub pack_id: String,
    pub version: String,
    pub evidence: Vec<EvidenceRef>,
    pub verdict: Verdict,
    pub requirement_ids: BTreeSet<String>,
    pub reason: String,
}

/// A host-derived pack projection, rebuilt from DiscoverWork on synchronization.
/// This is immutable coverage input, not another editable source/requirement list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub evidence: Vec<EvidenceRef>,
    pub structure_keys: BTreeSet<String>,
}

pub fn version(
    input: &FrozenInput,
    draft: &super::tools::Draft,
    pack: &str,
    scope: &Scope,
) -> Result<String, String> {
    super::canonical_sha256(&(
        evidence::input_digest(input)?,
        &draft.discovery_revision,
        draft.read_epoch,
        pack,
        scope,
    ))
}

pub fn validate(
    input: &FrozenInput,
    draft: &super::tools::Draft,
    scope: &Scope,
    review: &Disposition,
) -> Result<(), String> {
    if review.version != version(input, draft, &review.pack_id, scope)? {
        return Err("source disposition version is stale".into());
    }
    if review.reason.trim().is_empty() || (!scope.evidence.is_empty() && review.evidence.is_empty())
    {
        return Err("source disposition needs an explicit reason and bounded evidence".into());
    }
    evidence::validate_evidence(&review.evidence, input, &scope.evidence)?;
    evidence::validate_evidence(&review.evidence, input, &draft.check_reads.evidence)?;
    if scope.evidence.is_empty()
        && !scope
            .structure_keys
            .is_subset(&draft.check_reads.structure_keys)
    {
        return Err("empty source scope still requires delivered structure receipts".into());
    }
    let overlaps = |refs: &[EvidenceRef]| {
        refs.iter().any(|reference| {
            review.evidence.iter().any(|read| {
                reference.same_carrier(read)
                    && match (reference.range(), read.range()) {
                        (Some((start, end)), Some((other_start, other_end))) => {
                            start < other_end && other_start < end
                        }
                        (None, None) => true,
                        _ => false,
                    }
            })
        })
    };
    let matched: BTreeSet<_> = draft
        .requirements
        .iter()
        .filter(|(_, record)| overlaps(&record.evidence))
        .map(|(id, _)| id.clone())
        .collect();
    for id in &review.requirement_ids {
        let record = draft
            .requirements
            .get(id)
            .ok_or("source disposition references unknown requirement")?;
        if !overlaps(&record.evidence) {
            return Err("source disposition requirement is outside its source carrier".into());
        }
    }
    match review.verdict {
        Verdict::ContainsObligations if review.requirement_ids.is_empty() || !matched.is_subset(&review.requirement_ids) =>
            Err("source obligations need existing requirement references or an unresolved disposition".into()),
        Verdict::NoResponseObligation if !review.requirement_ids.is_empty()
            || draft.requirements.values().any(|record| overlaps(&record.evidence)) =>
            Err("no-obligation disposition conflicts with an existing requirement; resolve explicitly".into()),
        _ => Ok(()),
    }
}

pub fn reviewed_evidence(
    dispositions: &BTreeMap<String, Disposition>,
    pack: &str,
) -> Vec<EvidenceRef> {
    dispositions
        .values()
        .filter(|review| review.pack_id == pack && review.verdict != Verdict::Unresolved)
        .flat_map(|review| review.evidence.iter().cloned())
        .collect()
}

pub fn key(review: &Disposition) -> Result<String, String> {
    super::canonical_sha256(&(&review.pack_id, &review.evidence))
}

pub fn complete(
    input: &FrozenInput,
    draft: &super::tools::Draft,
    pack: &str,
) -> Result<bool, String> {
    let scope = draft
        .source_scopes
        .get(pack)
        .ok_or("source review scope missing")?;
    let rows: Vec<_> = draft
        .source_dispositions
        .values()
        .filter(|r| r.pack_id == pack)
        .collect();
    if rows.is_empty() {
        return Ok(false);
    }
    for row in &rows {
        validate(input, draft, scope, row)?;
        if row.verdict == Verdict::Unresolved {
            return Ok(false);
        }
    }
    let reviewed = reviewed_evidence(&draft.source_dispositions, pack);
    Ok(scope
        .structure_keys
        .is_subset(&draft.check_reads.structure_keys)
        && scope
            .evidence
            .iter()
            .all(|r| evidence::covered_by_union(r, &reviewed)))
}

pub fn ready(input: &FrozenInput, draft: &super::tools::Draft) -> Result<(), String> {
    if draft.source_scopes.keys().cloned().collect::<BTreeSet<_>>() != draft.required_pack_ids {
        return Err("required source review scopes are inconsistent".into());
    }
    for pack in &draft.required_pack_ids {
        if !complete(input, draft, pack)? {
            return Err("source dispositions are incomplete or unresolved".into());
        }
    }
    Ok(())
}
