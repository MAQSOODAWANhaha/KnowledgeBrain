//! Fixed-source-set repair preserves independent domain work, never read credit.
use super::{TargetRef, discover::ConditionSupportOrigin, evidence::EvidenceRef, tools::Draft};
use std::collections::BTreeSet;

fn response_chapters(draft: &Draft, affected: &BTreeSet<String>) -> BTreeSet<String> {
    let mut chapters = draft
        .chapters
        .iter()
        .filter(|c| c.requirement_ids.iter().any(|id| affected.contains(id)))
        .map(|c| c.id.clone())
        .collect::<BTreeSet<_>>();
    for fulfillment in draft
        .fulfillments
        .iter()
        .filter(|f| affected.contains(&f.requirement_id))
    {
        chapters.insert(fulfillment.primary_response_chapter_id.clone());
        for target in &fulfillment.target_refs {
            match target {
                TargetRef::TextSlot { slot_id } => {
                    chapters.extend(
                        draft
                            .slots
                            .iter()
                            .filter(|s| s.slot_id == *slot_id)
                            .map(|s| s.chapter_id.clone()),
                    );
                }
                TargetRef::FormBinding { form_id } => {
                    chapters.extend(
                        draft
                            .bindings
                            .iter()
                            .filter(|b| b.form_id == *form_id)
                            .map(|b| b.chapter_id.clone()),
                    );
                }
                TargetRef::ManualTask { .. } => {}
            }
        }
    }
    chapters
}

pub(crate) fn retain_independent(
    draft: &mut Draft,
    mut affected: BTreeSet<String>,
    changed_sources: &[EvidenceRef],
) -> BTreeSet<String> {
    loop {
        let before = affected.len();
        let targets = draft
            .fulfillments
            .iter()
            .filter(|f| affected.contains(&f.requirement_id))
            .flat_map(|f| f.target_refs.iter())
            .collect::<Vec<_>>();
        let chapters = response_chapters(draft, &affected);
        let dependent_sources = draft
            .requirements
            .iter()
            .filter(|(id, _)| affected.contains(*id))
            .flat_map(|(_, r)| r.evidence.iter())
            .chain(changed_sources.iter())
            .collect::<Vec<_>>();
        let mut additions = BTreeSet::new();
        for (id, record) in &draft.requirements {
            let dependency = record.condition_support.iter().any(|support| {
                matches!(&support.origin, ConditionSupportOrigin::ReviewedRequirement { requirement_id } if affected.contains(requirement_id))
                    || support.evidence.iter().any(|r| dependent_sources.iter().any(|s| s.same_carrier(r)))
            });
            let shared_target = draft.fulfillments.iter().any(|f| {
                f.requirement_id == *id
                    && f.target_refs.iter().any(|target| targets.contains(&target))
            });
            // A requirement can point into an affected chapter through a target
            // even when its primary response chapter is elsewhere. Include it
            // before removing that chapter's slots/bindings.
            let shared_chapter = !response_chapters(draft, &BTreeSet::from([id.clone()]))
                .is_disjoint(&chapters);
            if dependency || shared_target || shared_chapter {
                additions.insert(id.clone());
            }
        }
        affected.extend(additions);
        if affected.len() == before {
            break;
        }
    }
    let targets = draft
        .fulfillments
        .iter()
        .filter(|f| affected.contains(&f.requirement_id))
        .flat_map(|f| f.target_refs.iter().cloned())
        .collect::<Vec<_>>();
    let affected_chapters = response_chapters(draft, &affected);
    let mut chapters = draft.chapters.clone();
    for chapter in &mut chapters {
        chapter.requirement_ids.retain(|id| !affected.contains(id));
    }
    let slots = draft
        .slots
        .iter()
        .filter(|slot| {
            !affected_chapters.contains(&slot.chapter_id)
                && !targets.contains(&TargetRef::TextSlot {
                    slot_id: slot.slot_id.clone(),
                })
        })
        .cloned()
        .collect::<Vec<_>>();
    let bindings = draft
        .bindings
        .iter()
        .filter(|binding| {
            !affected_chapters.contains(&binding.chapter_id)
                && !targets.contains(&TargetRef::FormBinding {
                    form_id: binding.form_id.clone(),
                })
        })
        .cloned()
        .collect();
    let slots_submitted = draft.slots_submitted && slots.len() == draft.slots.len();
    // All receipts and wire handles expire with this global revision/epoch.
    // Keep the canonical requirement set until Discover replaces it; no obligation
    // is removed merely to make readiness pass. Affected mappings must be rebuilt.
    *draft = Draft {
        chapters,
        slots,
        bindings,
        slots_submitted,
        fulfillments: draft
            .fulfillments
            .iter()
            .filter(|f| !affected.contains(&f.requirement_id))
            .cloned()
            .collect(),
        requirements: draft.requirements.clone(),
        required_requirement_ids: draft.required_requirement_ids.clone(),
        required_pack_ids: draft.required_pack_ids.clone(),
        read_epoch: draft.read_epoch.saturating_add(1),
        ..Draft::default()
    };
    affected
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::outline::{Fulfillment, discover::ConditionSupport};

    #[test]
    fn repair_closes_shared_targets_and_conditions_but_keeps_independent_draft() {
        let (input, state, _, refs) = crate::outline::fixture_organized(false);
        let mut draft = state.outline_run.tool_draft;
        let seed = draft
            .required_requirement_ids
            .iter()
            .next()
            .unwrap()
            .clone();
        let record = draft.requirements[&seed].clone();
        let mut independent_chapter = draft.chapters[0].clone();
        independent_chapter.id = "independent-response".into();
        independent_chapter.requirement_ids = vec!["independent:0".into()];
        draft.chapters.push(independent_chapter.clone());
        independent_chapter.id = "target-peer-response".into();
        independent_chapter.requirement_ids = vec!["target-peer:0".into()];
        draft.chapters.push(independent_chapter);
        for (id, target) in [
            ("peer:0", "blank"),
            ("dependent:0", "dependent"),
            ("target-peer:0", "target-peer"),
            ("independent:0", "independent"),
        ] {
            let mut requirement = record.clone();
            if id == "dependent:0" {
                requirement.condition_support.push(ConditionSupport {
                    origin: ConditionSupportOrigin::ReviewedRequirement {
                        requirement_id: "peer:0".into(),
                    },
                    review_version: "fixture".into(),
                    evidence: vec![],
                });
            }
            draft.requirements.insert(id.into(), requirement);
            draft.required_requirement_ids.insert(id.into());
            let chapter = if id == "target-peer:0" {
                "target-peer-response".to_string()
            } else if id == "independent:0" {
                "independent-response".to_string()
            } else {
                draft.chapters[0].requirement_ids.push(id.into());
                draft.chapters[0].id.clone()
            };
            draft.fulfillments.push(Fulfillment {
                requirement_id: id.into(),
                primary_response_chapter_id: chapter.clone(),
                target_refs: vec![TargetRef::TextSlot {
                    slot_id: target.into(),
                }],
            });
            if target != "blank" {
                let mut slot = draft
                    .slots
                    .iter()
                    .find(|s| s.slot_id == "blank")
                    .unwrap()
                    .clone();
                slot.slot_id = target.into();
                slot.chapter_id = if id == "target-peer:0" {
                    draft.chapters[0].id.clone()
                } else { chapter };
                draft.slots.push(slot);
            }
        }
        draft.check_reads.evidence = refs.clone();
        draft.delivered_evidence = refs.clone();
        draft.finished = true;
        let unrelated = draft
            .slots
            .iter()
            .find(|s| s.slot_id == "independent")
            .unwrap()
            .clone();
        let obligations = draft.requirements.clone();
        let epoch = draft.read_epoch;
        let affected = retain_independent(&mut draft, BTreeSet::from([seed.clone()]), &refs);
        assert_eq!(
            affected,
            BTreeSet::from([seed, "peer:0".into(), "dependent:0".into(), "target-peer:0".into()])
        );
        assert_eq!(draft.slots, vec![unrelated]);
        assert_eq!(draft.fulfillments.len(), 1);
        assert_eq!(draft.fulfillments[0].requirement_id, "independent:0");
        assert_eq!(
            draft.requirements, obligations,
            "repair cannot silently remove obligations"
        );
        assert!(!draft.chapters.is_empty());
        assert!(draft.check_reads.evidence.is_empty() && draft.delivered_evidence.is_empty());
        assert!(draft.claim_comparisons.is_empty() && draft.template_reviews.is_empty());
        assert_eq!(draft.read_epoch, epoch + 1);
        assert!(!draft.finished && !draft.slots_submitted);
        assert!(crate::outline::tools::template_ready(&input, &draft).is_err());
    }
}
