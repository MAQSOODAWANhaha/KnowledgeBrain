use super::evidence::EvidenceRef;
use super::tools::{Draft, apply_canonical as apply, validate_final_outline};
use super::*;
use crate::analysis::{FrozenInput, Source};
use crate::response::{NO_EVIDENCE_TEXT, ResponseStatus, match_queries, respond};
use serde_json::json;
use std::collections::BTreeSet;

pub(super) fn input() -> FrozenInput {
    FrozenInput {
        schema_version: crate::outline::frozen::FROZEN_SCHEMA_VERSION,
        project_id: "project".into(),
        document_set_id: "set".into(),
        documents: vec![],
        document_relations: vec![],
        source_units: vec![Source {
            source_unit_revision_id: "source".into(),
            document_id: "doc".into(),
            ordinal: 0,
            locator: json!({"heading_path":"资格","completeness":"complete"}),
            text: "不得转包；须提交资格证明。\n条件满足时提供附件。".into(),
        }],
        structured_forms: vec![],
        decisions: vec![],
    }
}
pub(super) fn evidence(input: &FrozenInput) -> EvidenceRef {
    EvidenceRef::Text {
        input_digest: evidence::input_digest(input).unwrap(),
        unit_id: "source".into(),
        start_byte: 0,
        end_byte: input.source_units[0].text.len(),
    }
}
pub(super) fn chapter(id: &str) -> ChapterOutline {
    ChapterOutline {
        id: id.into(),
        parent_id: None,
        order: 0,
        title: "资格响应".into(),
        purpose: ChapterPurpose::Response,
        requirement_ids: vec![],
    }
}
pub(super) fn blank(id: &str) -> TemplateContent {
    TemplateContent {
        slot_id: id.into(),
        chapter_id: "response".into(),
        kind: SlotKind::BidderBlank,
        content: TemplateBody::EditableBlank,
        text: String::new(),
        response_required: true,
        match_query: "资格证明".into(),
    }
}
fn record(input: &FrozenInput) -> super::discover::RequirementRecord {
    super::discover::RequirementRecord {
        condition_support: Vec::new(),
        obligation_strength: "mandatory".into(),
        extraction_quality: "explicit".into(),
        description: "资格义务".into(),
        evidence: vec![evidence(input)],
        source_section_id: "资格".into(),
        kind: "qualification".into(),
        dedup_group: "candidate".into(),
    }
}

pub(super) fn valid_draft() -> Draft {
    Draft {
        chapters: vec![chapter("response")],
        slots: vec![blank("blank")],
        slots_submitted: true,
        finished: true,
        ..Draft::default()
    }
}

#[test]
fn projection_and_response_preserve_host_source_copy() {
    let input = input();
    let mut draft = valid_draft();
    let reference = evidence(&input);
    draft.delivered_evidence = vec![reference.clone()];
    apply(&input,&mut draft,&BTreeSet::new(),"put_slots",&json!({"mode":"upsert","slots":[{"slot_id":"fixed","chapter_id":"response","content":{"type":"source_copy","refs":[reference]}}]})).unwrap();
    apply(
        &input,
        &mut draft,
        &BTreeSet::new(),
        "finish_outline",
        &json!({}),
    )
    .unwrap();
    let artifact = project_draft(&input, &evidence::input_digest(&input).unwrap(), &draft)
        .unwrap()
        .artifact;
    assert_eq!(artifact.templates[1].text, input.source_units[0].text);
    assert_eq!(match_queries(&artifact).unwrap().len(), 1);
    let answers = respond(&artifact, &[]).unwrap();
    assert_eq!(answers.responses[0].status, ResponseStatus::NoEvidence);
    assert_eq!(answers.responses[0].text, NO_EVIDENCE_TEXT);
    assert!(validate_publication(&input, &artifact, &[]).is_ok());
}

#[test]
fn model_fixed_wording_and_undelivered_sources_are_rejected() {
    let input = input();
    let mut draft = valid_draft();
    let prior = draft.clone();
    assert!(apply(&input,&mut draft,&BTreeSet::new(),"put_slots",&json!({"mode":"replace","slots":[{"slot_id":"fixed","chapter_id":"response","kind":"fixed_text","text":"允许转包"}]})).is_err());
    assert_eq!(draft, prior);
    assert!(apply(&input,&mut draft,&BTreeSet::new(),"put_slots",&json!({"mode":"replace","slots":[{"slot_id":"fixed","chapter_id":"response","content":{"type":"source_copy","refs":[evidence(&input)]}}]})).unwrap_err().contains("outside_scope"));
    assert_eq!(draft, prior);
}

#[test]
fn source_copy_must_have_explicit_paragraph_or_cell_boundaries() {
    let input = input();
    let reference = evidence(&input);
    assert!(
        tools::source_copy(&input, &[reference.clone(), reference])
            .unwrap_err()
            .contains("separate")
    );
}

#[test]
fn final_projection_and_publication_revalidate_tampered_source_and_roles() {
    let input = input();
    let mut draft = valid_draft();
    let reference = evidence(&input);
    draft.slots.push(TemplateContent {
        slot_id: "fixed".into(),
        chapter_id: "response".into(),
        kind: SlotKind::FixedText,
        content: TemplateBody::SourceCopy {
            refs: vec![reference],
        },
        text: input.source_units[0].text.clone(),
        response_required: false,
        match_query: String::new(),
    });
    let digest = evidence::input_digest(&input).unwrap();
    let mut artifact = project_draft(&input, &digest, &draft).unwrap().artifact;
    artifact.templates[1].text = "允许转包".into();
    assert!(
        validate_publication(&input, &artifact, &[])
            .unwrap_err()
            .contains("frozen source")
    );
    draft.chapters[0].purpose = ChapterPurpose::Group;
    assert!(project_draft(&input, &digest, &draft).is_err());
    draft.chapters[0].purpose = ChapterPurpose::Response;
    draft.slots[1].text = "允许转包".into();
    assert!(project_draft(&input, &digest, &draft).is_err());
}

#[test]
fn requirement_assignment_alone_never_counts_as_fulfillment() {
    let input = input();
    let mut draft = valid_draft();
    let reqs = BTreeSet::from(["r".into()]);
    draft.required_requirement_ids = reqs.clone();
    draft.requirements.insert("r".into(), record(&input));
    draft.chapters[0].requirement_ids = vec!["r".into()];
    assert!(
        validate_final_outline(&input, &reqs, &draft)
            .unwrap_err()
            .contains("actual fulfillment")
    );
    apply(&input,&mut draft,&reqs,"put_fulfillments",&json!({"fulfillments":[{"requirement_id":"r","primary_response_chapter_id":"response","target_refs":[{"type":"text_slot","slot_id":"blank"}]}]})).unwrap();
    assert!(validate_final_outline(&input, &reqs, &draft).is_ok());
    draft.slots.clear();
    assert!(
        validate_final_outline(&input, &reqs, &draft)
            .unwrap_err()
            .contains("target slot")
    );
}

#[test]
fn manual_work_is_visible_and_requires_review_status() {
    let input = input();
    let mut draft = valid_draft();
    draft.required_requirement_ids.insert("r".into());
    draft.requirements.insert("r".into(), record(&input));
    draft.reviewed_requirement_ids.insert("r".into());
    draft.chapters[0].requirement_ids = vec!["r".into()];
    draft.fulfillments = vec![Fulfillment {
        requirement_id: "r".into(),
        primary_response_chapter_id: "response".into(),
        target_refs: vec![TargetRef::ManualTask {
            task_id: "manual".into(),
            description: "核验原件资格证明".into(),
        }],
    }];
    assert!(super::tools::needs_semantic_review(&draft));
    assert!(project_draft(&input, &evidence::input_digest(&input).unwrap(), &draft).is_err());
    assert_eq!(draft.fulfillments[0].target_refs.len(), 1);
}

#[test]
fn old_artifact_schema_and_missing_content_contract_are_rejected() {
    let input = input();
    let artifact = project_draft(
        &input,
        &evidence::input_digest(&input).unwrap(),
        &valid_draft(),
    )
    .unwrap()
    .artifact;
    let mut old = artifact.clone();
    old.schema_version = 1;
    assert!(validate_artifact(&old).is_err());
    let mut value = serde_json::to_value(artifact).unwrap();
    value["templates"][0]
        .as_object_mut()
        .unwrap()
        .remove("content");
    assert!(serde_json::from_value::<OutlineArtifact>(value).is_err());
}

#[test]
fn oversized_slot_metadata_is_preserved_for_host_continuation() {
    let input = input();
    let mut draft = valid_draft();
    let previous = draft.slots.clone();
    let huge = "🙂".repeat(4096);
    apply(&input,&mut draft,&BTreeSet::new(),"put_slots",&json!({"mode":"upsert","slots":[{"slot_id":huge,"chapter_id":"response","content":{"type":"editable_blank"},"blank_kind":"bidder_blank","match_query":"合法查询"}]})).unwrap();
    assert!(previous.iter().all(|slot| draft.slots.contains(slot)));
    assert!(draft.slots.iter().any(|slot| slot.slot_id == huge));
    assert!(!draft.finished);
}

#[test]
fn instruction_only_target_cannot_fulfill_an_actual_requirement() {
    let input = input();
    let mut draft = valid_draft();
    draft.required_requirement_ids.insert("r".into());
    draft.requirements.insert("r".into(), record(&input));
    draft.chapters[0].requirement_ids = vec!["r".into()];
    draft.slots.push(TemplateContent {
        slot_id: "instruction".into(),
        chapter_id: "response".into(),
        kind: SlotKind::Instruction,
        content: TemplateBody::GeneratedExplanation {
            supporting_refs: vec![evidence(&input)],
        },
        text: "Please provide a certificate".into(),
        response_required: false,
        match_query: String::new(),
    });
    let prior = draft.clone();
    let reqs = draft.required_requirement_ids.clone();
    let args = json!({"fulfillments":[{"requirement_id":"r","primary_response_chapter_id":"response","target_refs":[{"type":"text_slot","slot_id":"instruction"}]}]});
    assert!(
        apply(&input, &mut draft, &reqs, "put_fulfillments", &args)
            .unwrap_err()
            .contains("only generated instructions")
    );
    assert_eq!(draft, prior);
    draft.fulfillments = serde_json::from_value(args["fulfillments"].clone()).unwrap();
    assert!(
        validate_final_outline(&input, &reqs, &draft)
            .unwrap_err()
            .contains("only generated instructions")
    );
}
