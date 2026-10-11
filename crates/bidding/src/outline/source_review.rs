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

/// A scope remains active through source disposition, linked claim and target
/// review. Delivery and source-only completion cannot advance the work cursor.
pub fn next_scope<'a>(input: &FrozenInput, draft: &'a super::tools::Draft) -> Option<&'a String> {
    draft.required_pack_ids.iter().find(|pack| {
        !execution_complete(input, draft, pack).unwrap_or(false)
            || linked_requirements(draft, pack)
                .iter()
                .filter(|id| !has_document_issue(input, draft, id))
                .any(|id| {
                    !draft.reviewed_requirement_ids.contains(id)
                        || draft.template_reviews.get(id).is_none_or(|review| {
                            !super::template_review::validate(input, draft, review).unwrap_or(false)
                        })
                })
    })
}

pub fn linked_requirements(draft: &super::tools::Draft, pack: &str) -> BTreeSet<String> {
    let Some(scope) = draft.source_scopes.get(pack) else {
        return BTreeSet::new();
    };
    draft
        .requirements
        .iter()
        .filter(|(_, record)| {
            record.evidence.iter().any(|reference| {
                scope
                    .evidence
                    .iter()
                    .any(|source| overlaps(source, reference))
            })
        })
        .map(|(id, _)| id.clone())
        .collect()
}

/// Explicit model classification; never infer document defects from API errors
/// or descriptive keywords. System findings remain repair obligations.
pub fn is_document_issue(issue: &super::ReviewIssue) -> bool {
    matches!(
        issue.code.as_str(),
        "document_missing" | "document_conflict" | "document_ambiguous" | "document_unreadable"
    )
}

fn overlaps(left: &EvidenceRef, right: &EvidenceRef) -> bool {
    left.same_carrier(right)
        && match (left.range(), right.range()) {
            (Some((start, end)), Some((other_start, other_end))) => {
                start < other_end && other_start < end
            }
            (None, None) => true,
            _ => false,
        }
}

fn current_document_evidence(
    input: &FrozenInput,
    draft: &super::tools::Draft,
    issue: &super::ReviewIssue,
) -> bool {
    if !is_document_issue(issue)
        || issue.evidence.is_empty()
        || evidence::validate_evidence(&issue.evidence, input, &draft.check_reads.evidence).is_err()
    {
        return false;
    }
    // Issues have no independent version field. Only a current, freshly read
    // source disposition can make their evidence actionable in this epoch.
    let current = draft
        .source_dispositions
        .values()
        .filter(|review| {
            draft
                .source_scopes
                .get(&review.pack_id)
                .is_some_and(|scope| validate(input, draft, scope, review).is_ok())
        })
        .flat_map(|review| review.evidence.iter().cloned())
        .collect::<Vec<_>>();
    issue
        .evidence
        .iter()
        .all(|reference| evidence::covered_by_union(reference, &current))
}

pub fn has_document_issue(
    input: &FrozenInput,
    draft: &super::tools::Draft,
    requirement_id: &str,
) -> bool {
    let Some(record) = draft.requirements.get(requirement_id) else {
        return false;
    };
    let covered = draft
        .review_issues
        .iter()
        .filter(|issue| {
            issue.requirement_ids.iter().any(|id| id == requirement_id)
                && current_document_evidence(input, draft, issue)
        })
        .flat_map(|issue| issue.evidence.iter().cloned())
        .collect::<Vec<_>>();
    !record.evidence.is_empty()
        && record
            .evidence
            .iter()
            .all(|reference| evidence::covered_by_union(reference, &covered))
}

/// Processing can finish an explicitly documented source problem while clean
/// readiness remains blocked by `complete`/`ready`. This grants no semantic credit.
pub fn execution_complete(
    input: &FrozenInput,
    draft: &super::tools::Draft,
    pack: &str,
) -> Result<bool, String> {
    if complete(input, draft, pack)? {
        return Ok(true);
    }
    let scope = draft
        .source_scopes
        .get(pack)
        .ok_or("source scope missing")?;
    if !scope
        .structure_keys
        .is_subset(&draft.check_reads.structure_keys)
    {
        return Ok(false);
    }
    let mut accounted = reviewed_evidence(&draft.source_dispositions, pack);
    for issue in draft
        .review_issues
        .iter()
        .filter(|issue| current_document_evidence(input, draft, issue))
    {
        evidence::validate_evidence(&issue.evidence, input, &draft.check_reads.evidence)?;
        accounted.extend(
            issue
                .evidence
                .iter()
                .filter(|reference| evidence::covered_by_union(reference, &scope.evidence))
                .cloned(),
        );
    }
    Ok(scope
        .evidence
        .iter()
        .all(|reference| evidence::covered_by_union(reference, &accounted)))
}

/// Derived control-stop predicate, not publication or semantic success. Every
/// independent source and requirement must be processed; only explicit document
/// findings may remain. API failures never create these domain findings.
pub fn document_issue_report_ready(input: &FrozenInput, draft: &super::tools::Draft) -> bool {
    // During Discover, committed findings may already be visible while the
    // canonical source inventory has not yet been synchronized. Empty sets must
    // not vacuously authorize the runtime's document-issues control stop.
    if draft.discovery_revision.is_none()
        || draft.required_pack_ids.is_empty()
        || draft.source_scopes.is_empty()
    {
        return false;
    }
    if !draft.review_issues.iter().any(is_document_issue)
        || draft.review_issues.iter().any(|issue| {
            !is_document_issue(issue)
                && !(issue.code == "claim_evidence_unresolved"
                    && issue.id.starts_with("claim-comparison:")
                    && !issue.requirement_ids.is_empty()
                    && issue
                        .requirement_ids
                        .iter()
                        .all(|id| has_document_issue(input, draft, id)))
        })
    {
        return false;
    }
    if draft.source_scopes.keys().cloned().collect::<BTreeSet<_>>() != draft.required_pack_ids
        || draft
            .required_pack_ids
            .iter()
            .any(|pack| !execution_complete(input, draft, pack).unwrap_or(false))
    {
        return false;
    }
    draft.required_requirement_ids.iter().all(|id| {
        has_document_issue(input, draft, id)
            || (draft.reviewed_requirement_ids.contains(id)
                && draft.template_reviews.get(id).is_some_and(|review| {
                    super::template_review::validate(input, draft, review).unwrap_or(false)
                }))
    })
}

#[cfg(test)]
mod scope_boundary_tests {
    use super::*;

    fn halves(reference: &EvidenceRef) -> (EvidenceRef, EvidenceRef) {
        let EvidenceRef::Text {
            input_digest,
            unit_id,
            start_byte,
            end_byte,
        } = reference
        else {
            panic!("text fixture required")
        };
        let split = start_byte + (end_byte - start_byte) / 2;
        (
            EvidenceRef::Text {
                input_digest: input_digest.clone(),
                unit_id: unit_id.clone(),
                start_byte: *start_byte,
                end_byte: split,
            },
            EvidenceRef::Text {
                input_digest: input_digest.clone(),
                unit_id: unit_id.clone(),
                start_byte: split,
                end_byte: *end_byte,
            },
        )
    }

    #[test]
    fn disjoint_ranges_of_same_text_link_only_their_own_requirements() {
        let (_, state, _, refs) = crate::outline::fixture_organized(false);
        let mut draft = state.outline_run.tool_draft;
        let text = refs
            .iter()
            .find(|r| matches!(r, EvidenceRef::Text { .. }))
            .unwrap();
        let (left, right) = halves(text);
        let mut record = draft.requirements.values().next().unwrap().clone();
        draft.requirements.clear();
        record.evidence = vec![left.clone()];
        draft.requirements.insert("left".into(), record.clone());
        record.evidence = vec![right.clone()];
        draft.requirements.insert("right".into(), record);
        draft.source_scopes.insert(
            "left-pack".into(),
            Scope {
                evidence: vec![left],
                structure_keys: BTreeSet::new(),
            },
        );
        draft.source_scopes.insert(
            "right-pack".into(),
            Scope {
                evidence: vec![right],
                structure_keys: BTreeSet::new(),
            },
        );
        assert_eq!(
            linked_requirements(&draft, "left-pack"),
            BTreeSet::from(["left".into()])
        );
        assert_eq!(
            linked_requirements(&draft, "right-pack"),
            BTreeSet::from(["right".into()])
        );
    }

    #[test]
    fn document_issue_cannot_skip_uncovered_requirement_or_stale_epoch() {
        let (input, state, pack, refs) = crate::outline::fixture_organized(false);
        let mut draft = state.outline_run.tool_draft;
        let text = refs
            .iter()
            .find(|r| matches!(r, EvidenceRef::Text { .. }))
            .unwrap()
            .clone();
        let (id, mut record) = draft
            .requirements
            .iter()
            .next()
            .map(|(id, r)| (id.clone(), r.clone()))
            .unwrap();
        record.evidence = vec![text.clone()];
        draft.requirements = BTreeMap::from([(id.clone(), record)]);
        draft.check_reads.evidence = vec![text.clone()];
        let scope = Scope {
            evidence: vec![text.clone()],
            structure_keys: BTreeSet::new(),
        };
        let review = Disposition {
            pack_id: pack.clone(),
            version: version(&input, &draft, &pack, &scope).unwrap(),
            evidence: vec![text.clone()],
            verdict: Verdict::Unresolved,
            requirement_ids: BTreeSet::from([id.clone()]),
            reason: "Synthetic scope question".into(),
        };
        draft.source_scopes.insert(pack, scope);
        draft
            .source_dispositions
            .insert(key(&review).unwrap(), review);
        // Split on an actual UTF-8 boundary so partial evidence remains valid.
        let EvidenceRef::Text {
            start_byte,
            end_byte,
            ..
        } = &text
        else {
            unreachable!()
        };
        let quote = &input
            .source_units
            .iter()
            .find(|s| match &text {
                EvidenceRef::Text { unit_id, .. } => &s.source_unit_revision_id == unit_id,
                _ => false,
            })
            .unwrap()
            .text;
        let split = quote
            .char_indices()
            .map(|(i, _)| i)
            .find(|i| i > start_byte && i < end_byte)
            .unwrap();
        let mut partial = text.clone();
        if let EvidenceRef::Text { end_byte, .. } = &mut partial {
            *end_byte = split;
        }
        draft.review_issues.push(super::super::ReviewIssue {
            id: "partial-document".into(),
            code: "document_ambiguous".into(),
            description: "Synthetic partial ambiguity".into(),
            requirement_ids: vec![id.clone()],
            evidence: vec![partial],
            requested_information: "Clarify this cited range".into(),
        });
        assert!(!has_document_issue(&input, &draft, &id));
        draft.review_issues[0].evidence = vec![text];
        assert!(has_document_issue(&input, &draft, &id));
        draft.read_epoch += 1;
        assert!(
            !has_document_issue(&input, &draft, &id),
            "fresh reads alone cannot revive an old disposition"
        );
    }
}
