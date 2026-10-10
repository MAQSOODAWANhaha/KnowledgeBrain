//! Version 2 Discover → Organize → evidence-backed Check → publication acceptance.
use super::agent::{Duty, current, host_packet};
use super::discover::{DiscoverWork, PackRequirement, PackSubmit};
use super::*;
use crate::analysis::agent::Checkpoint;
use crate::analysis::{Analysis, FrozenInput};
use serde_json::json;
use std::collections::BTreeMap;
// Domain acceptance harness: delivery is explicit and uses the production
// exact-wire receipt seam. This is not a simulated model or full runtime test.
trait FixtureDelivery {
    fn deliver_fixture_reads(&mut self);
}
impl FixtureDelivery for Checkpoint {
    fn deliver_fixture_reads(&mut self) {
        let mut body = json!({"messages":self.transcript});
        self.outline_run.tool_draft.source_keys = source_wire::request(self, &mut body).unwrap();
        read_receipts::seal(self, &body).unwrap();
        read_receipts::confirm(self, &body).unwrap();
        self.turn += 1;
    }
}
fn apply(
    input: &FrozenInput,
    state: &mut Checkpoint,
    name: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let before = state.outline_run.tool_draft.clone();
    let duty = current(input, state);
    let mut args = args.clone();
    if name == "submit_review" {
        for pointer in ["/inspected_evidence"] {
            if let Some(values) = args
                .pointer_mut(pointer)
                .and_then(serde_json::Value::as_array_mut)
            {
                for v in values {
                    if let Ok(reference) =
                        serde_json::from_value::<evidence::EvidenceRef>(v.clone())
                    {
                        *v = json!({"review_evidence_key":agent::review_evidence_key(state,&reference)?});
                    }
                }
            }
        }
        if let Some(issues) = args["issues"].as_array_mut() {
            for issue in issues {
                if let Some(values) = issue["evidence"].as_array_mut() {
                    for v in values {
                        if let Ok(reference) =
                            serde_json::from_value::<evidence::EvidenceRef>(v.clone())
                        {
                            *v = json!({"review_evidence_key":agent::review_evidence_key(state,&reference)?});
                        }
                    }
                }
            }
        }
    }
    let result = match name {
        "read_requirements" => {
            let page = agent::full_read_projection(input, state, name, &args)?;
            agent::queue_read_projection(state, name, &page, duty);
            page
        }
        "read_claim_evidence" => {
            let id = args["requirement_id"].as_str().ok_or("id missing")?;
            let record = state
                .outline_run
                .reading_packs
                .as_ref()
                .unwrap()
                .requirement_records()
                .get(id)
                .ok_or("unknown requirement")?;
            let unit = claim_review::build(input, id, record)?;
            for u in &unit.evidence_units {
                for excerpt in &u.excerpts {
                    state
                        .outline_run
                        .tool_draft
                        .check_reads
                        .pending_evidence
                        .push((state.turn, excerpt.evidence.clone()));
                }
            }
            json!({"items":unit.evidence_units,"next_cursor":null,"version":unit.version})
        }
        _ => agent::apply(input, state, name, &args)?,
    };
    if matches!(
        name,
        "read_requirements" | "read_outline" | "read_evidence" | "read_claim_evidence"
    ) {
        let id = format!("fixture-read-{}", state.transcript.len());
        state.transcript.push(json!({"role":"assistant","tool_calls":[{"id":id,"type":"function","function":{"name":name,"arguments":args.to_string()}}]}));
        state.transcript.push(json!({"role":"tool","tool_call_id":id,"content":json!({"ok":true,"result":result}).to_string()}));
        read_receipts::queue(state, &before, &id, duty == Duty::Check);
    }
    Ok(result)
}
pub(super) fn checkpoint(input: &FrozenInput) -> Checkpoint {
    Checkpoint {
        journal: Default::default(),
        input_sha256: crate::analysis::digest(input).unwrap(),
        config_sha256: String::new(),
        turn: 1,
        tool_calls: 0,
        read_bytes: 0,
        review_rounds: 0,
        role: crate::analysis::agent::Role::Main,
        analysis: Analysis::default(),
        review: None,
        pending_coverage: None,
        transcript: vec![json!({"role":"assistant","content":"发现对话里的要求：投标函"})],
        main_progress: Default::default(),
        main_work: None,
        done: false,
        source_views: BTreeMap::new(),
        draft_stage: crate::analysis::draft::DraftStage::Outline,
        draft_outline_gaps: None,
        draft_outline_stalls: 0,
        draft_outline_window: 0,
        outline_config_sha256: None,

        outline_run: Default::default(),
    }
}

pub(crate) fn discovered(
    negative: bool,
) -> (FrozenInput, Checkpoint, String, Vec<evidence::EvidenceRef>) {
    discovered_with_quality(negative, "explicit")
}
fn discovered_with_quality(
    negative: bool,
    quality: &str,
) -> (FrozenInput, Checkpoint, String, Vec<evidence::EvidenceRef>) {
    let input = super::tests::input();
    let mut state = checkpoint(&input);
    state.analysis.outline.phase = crate::analysis::outline_flow::Phase::Outline;
    let mut work = DiscoverWork::plan(&input, 4096);
    let packs = work.claim(4);
    assert_eq!(packs.len(), 1);
    let pack = &packs[0];
    let refs = work.pack_evidence(&input, &pack.id).unwrap();
    let requirements = if negative {
        vec![]
    } else {
        vec![PackRequirement {
            condition_support: Vec::new(),
            obligation_strength: "mandatory".into(),
            extraction_quality: quality.into(),
            description: "提供资格证明并遵守禁止转包条款".into(),
            evidence: refs.clone(),
            source_section_id: pack.atoms[0].section_id.clone(),
            kind: "qualification".into(),
        }]
    };
    let submit = PackSubmit {
        call_id: "submission".into(),
        claim_token: pack.claim_token.clone(),
        pack_revision: pack.pack_revision,
        requirements,
        no_requirement_reason: negative.then(|| "本包仅有说明，仍需独立复核".into()),
        inspected_atom_ids: if negative {
            pack.atoms
                .iter()
                .filter(|atom| !atom.context_only)
                .map(|atom| atom.id.clone())
                .collect()
        } else {
            vec![]
        },
    };
    let pack_id = pack.id.clone();
    // Domain setup uses canonical typed provenance; model-wire behavior is
    // exercised by runtime tests, not by accepting a legacy JSON envelope.
    work.submit(&input, &pack_id, submit).unwrap();
    state.outline_run.reading_packs = Some(work);
    (input, state, pack_id, refs)
}

pub(crate) fn organized(
    negative: bool,
) -> (FrozenInput, Checkpoint, String, Vec<evidence::EvidenceRef>) {
    organized_with_quality(negative, "explicit")
}
fn organized_with_quality(
    negative: bool,
    quality: &str,
) -> (FrozenInput, Checkpoint, String, Vec<evidence::EvidenceRef>) {
    let (input, mut state, pack_id, refs) = discovered_with_quality(negative, quality);
    assert_eq!(current(&input, &state), Duty::Organize);
    let ids = state
        .outline_run
        .reading_packs
        .as_ref()
        .unwrap()
        .requirement_ids()
        .into_iter()
        .collect::<Vec<_>>();
    apply(&input,&mut state,"put_chapters",&json!({"mode":"replace","chapters":[{"id":"response","parent_id":null,"order":0,"title":"资格响应","purpose":"response","requirement_ids":ids}]})).unwrap();
    apply(
        &input,
        &mut state,
        "read_evidence",
        &json!({"refs":refs,"max_bytes":4096}),
    )
    .unwrap();
    state.deliver_fixture_reads(); // The next complete model response has seen read_evidence.
    apply(&input,&mut state,"put_slots",&json!({"mode":"replace","slots":[{"slot_id":"fixed","chapter_id":"response","content":{"type":"source_copy","refs":[refs[0]]}},{"slot_id":"blank","chapter_id":"response","content":{"type":"editable_blank"},"blank_kind":"bidder_blank","match_query":"资格材料"}]})).unwrap();
    if !negative {
        assert_eq!(current(&input, &state), Duty::Organize);
        apply(&input,&mut state,"put_fulfillments",&json!({"fulfillments":[{"requirement_id":ids[0],"primary_response_chapter_id":"response","target_refs":[{"type":"text_slot","slot_id":"blank"},{"type":"text_slot","slot_id":"fixed"}]}]})).unwrap();
    }
    assert_eq!(current(&input, &state), Duty::Check);
    (input, state, pack_id, refs)
}

fn fresh_check_reads(input: &FrozenInput, state: &mut Checkpoint, refs: &[evidence::EvidenceRef]) {
    assert_eq!(current(input, state), Duty::Check);
    let text_refs = refs
        .iter()
        .filter(|reference| !matches!(reference, evidence::EvidenceRef::ImageRegion { .. }))
        .cloned()
        .collect::<Vec<_>>();
    apply(
        input,
        state,
        "read_evidence",
        &json!({"refs":text_refs,"max_bytes":16384}),
    )
    .unwrap();
    // Mock the verified-pixels host seam; runtime tests check actual body bytes.
    for reference in refs {
        if let evidence::EvidenceRef::ImageRegion { image_id, .. } = reference {
            agent::note_visual_delivery(input, state, image_id, Duty::Check).unwrap();
        }
    }
    let slots = state
        .outline_run
        .tool_draft
        .slots
        .iter()
        .map(|slot| slot.slot_id.clone())
        .collect::<Vec<_>>();
    for id in slots {
        let mut cursor = 0;
        let mut version = serde_json::Value::Null;
        loop {
            let mut args =
                json!({"mode":"slot_body","slot_id":id,"cursor":cursor,"max_bytes":2048});
            if cursor > 0 {
                args["version"] = version.clone();
            }
            let page = apply(input, state, "read_outline", &args).unwrap();
            version = page["version"].clone();
            match page["next_cursor"].as_u64() {
                Some(next) => cursor = next,
                None => break,
            }
        }
    }
    let mut cursor = 0;
    let mut version = serde_json::Value::Null;
    loop {
        let mut args = json!({"mode":"packs","cursor":cursor,"max_bytes":16384});
        if cursor > 0 {
            args["version"] = version.clone();
        }
        let page = apply(input, state, "read_requirements", &args).unwrap();
        version = page["version"].clone();
        match page["next_cursor"].as_u64() {
            Some(next) => cursor = next,
            None => break,
        }
    }
    state.deliver_fixture_reads();
    deterministic_claim_comparisons(input, state);
}

// Deterministic protocol fixture only: these scripted judgments are never
// reported as actual model correctness. Every read uses the production route.
fn deterministic_claim_comparisons(input: &FrozenInput, state: &mut Checkpoint) {
    let records = state
        .outline_run
        .reading_packs
        .as_ref()
        .unwrap()
        .requirement_records()
        .clone();
    for (id, record) in records {
        let unit = claim_review::build(input, &id, &record).unwrap();
        let mut cursor = 0;
        loop {
            let args = json!({"requirement_id":id,"cursor":cursor,"version":unit.version,"max_bytes":16384});
            let page = apply(input, state, "read_claim_evidence", &args).unwrap();
            state.deliver_fixture_reads();
            match page["next_cursor"].as_u64() {
                Some(next) => cursor = next,
                None => break,
            }
        }
        let handles = unit
            .evidence_units
            .iter()
            .map(|u| u.quote_handle.clone())
            .collect::<Vec<_>>();
        let comparison = json!({"requirement_id":id,"version":unit.version,"declared_claims":[{"claim_handle":"requirement","obligation":record.description,"applicability":"explicit fixture scope","original_fragments":[record.description],"primary_handles":[handles[0]],"support_handles":&handles[1..]}],
            "observations":handles.iter().map(|h|json!({"quote_handle":h,"polarity":"mixed","applicability":"relevant","condition":"deterministic fixture source conditions"})).collect::<Vec<_>>(),
            "decisions":unit.claims.iter().map(|c|json!({"claim_handle":c.claim_handle,"evidence_handles":handles,"verdict":"supports","action":"retain","resulting_claim":c.original_text})).collect::<Vec<_>>()});
        apply(input, state, "submit_claim_comparison", &comparison).unwrap();
    }
}

#[test]
fn discovery_transcript_is_not_needed_for_exact_source_copy_and_publication() {
    let (input, mut state, pack, refs) = organized(false);
    state.transcript.clear();
    fresh_check_reads(&input, &mut state, &refs);
    let ids = state
        .outline_run
        .reading_packs
        .as_ref()
        .unwrap()
        .requirement_ids();
    apply(
        &input,
        &mut state,
        "submit_review",
        &json!({"requirement_ids":ids,"pack_ids":[pack],"inspected_evidence":refs,"issues":[]}),
    )
    .unwrap();
    apply(&input, &mut state, "finish_outline", &json!({})).unwrap();
    let published =
        project_draft(&input, &state.input_sha256, &state.outline_run.tool_draft).unwrap();
    assert_eq!(
        published.artifact.templates[0].text,
        input.source_units[0].text
    );
    assert!(!published.artifact.needs_review);
    assert_eq!(published.artifact.fulfillments.len(), 1);
    validate_publication(&input, &published.artifact, &published.bindings).unwrap();
}

#[test]
fn empty_extraction_is_reviewed_again_and_evidence_backed_issues_survive() {
    let (input, mut state, pack, refs) = organized(true);
    assert!(
        apply(&input, &mut state, "finish_outline", &json!({}))
            .unwrap_err()
            .contains("semantic review")
    );
    assert_eq!(current(&input, &state), Duty::Check);
    let sources = apply(
        &input,
        &mut state,
        "read_requirements",
        &json!({"mode":"packs","max_bytes":4096}),
    )
    .unwrap();
    assert_eq!(sources["items"][0]["pack_id"], pack);
    assert!(sources["items"][0]["no_requirement_reason"].is_string());
    fresh_check_reads(&input, &mut state, &refs);
    apply(&input,&mut state,"submit_review",&json!({"requirement_ids":[],"pack_ids":[pack],"inspected_evidence":refs,"issues":[{"id":"missed","code":"possible_missing_obligation","description":"源中有资格义务，空提取需人工核实","requirement_ids":[],"evidence":refs}]})).unwrap();
    apply(&input, &mut state, "finish_outline", &json!({})).unwrap();
    let artifact = project_draft(&input, &state.input_sha256, &state.outline_run.tool_draft)
        .unwrap()
        .artifact;
    assert!(artifact.needs_review);
    assert_eq!(artifact.review_issues.len(), 1);
}

#[test]
fn check_rejects_unsourced_issues_and_cannot_mutate_the_outline() {
    let (input, mut state, pack, refs) = organized(false);
    let prior = state.outline_run.tool_draft.clone();
    assert!(apply(&input,&mut state,"put_chapters",&json!({"mode":"replace","chapters":[{"id":"g","parent_id":null,"order":0,"title":"g","purpose":"group","requirement_ids":[]}]})).is_err());
    assert_eq!(state.outline_run.tool_draft, prior);
    assert!(apply(&input,&mut state,"submit_review",&json!({"requirement_ids":[],"pack_ids":[pack],"inspected_evidence":refs,"issues":[{"id":"issue","code":"missing","description":"无来源","requirement_ids":[],"evidence":[]}]})).is_err());
    assert_eq!(state.outline_run.tool_draft, prior);
}

#[test]
fn rejected_model_byte_budget_argument_does_not_grant_receipts() {
    let (input, state, _, refs) = discovered(false);
    let prior = state.outline_run.tool_draft.delivered_evidence.clone();
    assert!(
        source_wire::resolve(
            &state,
            "read_evidence",
            json!({"refs":refs,"max_bytes":256})
        )
        .is_err()
    );
    assert_eq!(state.outline_run.tool_draft.delivered_evidence, prior);
    let packet = host_packet(&input, &state, 8192, json!({}), json!({}), None);
    assert!(packet["requirements"]["items"].is_array());
    assert!(packet["outline"]["slots"].is_null());
}

#[test]
fn every_advertised_tool_has_one_schema_handler_and_duty_gate() {
    let registry = agent::registry();
    let mut names = std::collections::BTreeSet::new();
    for spec in registry {
        assert!(names.insert(spec.name));
        assert!(agent::handles(spec.name));
        for duty in [Duty::Discover, Duty::Organize, Duty::Check] {
            assert_eq!(
                agent::deny(duty, spec.name, false).is_none(),
                spec.allowed_duties.contains(&duty)
            );
            assert_eq!(
                agent::schemas_for(duty)
                    .iter()
                    .any(|s| s["function"]["name"] == spec.name),
                spec.allowed_duties.contains(&duty)
            );
        }
    }
    assert!(!agent::handles("bind_forms_append"));
    assert!(!agent::handles("put_slots_append"));
    assert!(agent::validate_arguments("read_outline", &json!({"unexpected":true})).is_err());
    assert!(agent::validate_arguments("put_slots",&json!({"mode":"replace","slots":[{"slot_id":"x","chapter_id":"x","kind":"fixed_text","text":"伪造","match_query":""}]})).is_err());
}

#[test]
fn real_python_pdf_fixture_runs_through_freeze_discovery_targets_and_publication_validation() {
    // Python produced the inventory fixture; OCR results are explicitly mocked
    // complete receipts. This tests the cross-language host contract, not recall.
    for input in [
        super::frozen::tests::python_fixture_input(0),
        super::frozen::tests::complete_grid_fixture_input(),
        super::frozen::tests::native_office_fixture_input(0),
        super::frozen::tests::native_office_fixture_input(1),
    ] {
        let mut state = checkpoint(&input);
        state.analysis.outline.phase = crate::analysis::outline_flow::Phase::Outline;
        let mut work = DiscoverWork::plan(&input, 4096);
        let mut all_evidence = Vec::new();
        let mut empty_packs = Vec::new();
        while !work.complete() {
            let packs = work.claim(4);
            assert!(!packs.is_empty());
            for pack in packs {
                let refs = work.pack_evidence(&input, &pack.id).unwrap();
                for reference in &refs {
                    if let evidence::EvidenceRef::ImageRegion { image_id, .. } = reference {
                        work.confirm_visual_delivery(&input, image_id, &"a".repeat(64))
                            .unwrap();
                    }
                }
                // Native office fixtures prove table-only obligations reach a
                // response target; surrounding prose is not their evidence.
                let requirements = refs
                    .iter()
                    .find(|reference| {
                        matches!(
                            reference,
                            evidence::EvidenceRef::GridCell {
                                anchor_row: 1,
                                anchor_column: 0,
                                ..
                            }
                        )
                    })
                    .or_else(|| {
                        refs.iter().find(|reference| {
                            matches!(reference, evidence::EvidenceRef::GridCell { .. })
                        })
                    })
                    .or_else(|| refs.first())
                    .map(|reference| {
                        vec![PackRequirement {
                            condition_support: Vec::new(),
                            obligation_strength: "mandatory".into(),
                            extraction_quality: "explicit".into(),
                            description: "Fixture obligation requiring a response".into(),
                            evidence: vec![reference.clone()],
                            source_section_id: pack.atoms[0].section_id.clone(),
                            kind: "technical".into(),
                        }]
                    })
                    .unwrap_or_default();
                let negative = requirements.is_empty();
                if negative {
                    empty_packs.push(pack.id.clone());
                }
                work.submit(
                    &input,
                    &pack.id,
                    PackSubmit {
                        call_id: format!("fixture:{}", pack.id),
                        claim_token: pack.claim_token.clone(),
                        pack_revision: pack.pack_revision,
                        requirements,
                        no_requirement_reason: negative
                            .then(|| "Explicitly empty structural carrier".into()),
                        inspected_atom_ids: if negative {
                            pack.atoms
                                .iter()
                                .filter(|atom| !atom.context_only)
                                .map(|atom| atom.id.clone())
                                .collect()
                        } else {
                            vec![]
                        },
                    },
                )
                .unwrap();
                all_evidence.extend(refs);
            }
        }
        super::evidence::resolve_evidence(&input, &all_evidence).unwrap();
        let ids = work.requirement_ids();
        let packs = work.pack_ids();
        state.outline_run.reading_packs = Some(work);
        apply(&input,&mut state,"put_chapters",&json!({"mode":"replace","chapters":[
        {"id":"root","parent_id":null,"order":0,"title":"Root","purpose":"group","requirement_ids":[]},
        {"id":"group","parent_id":"root","order":0,"title":"Group","purpose":"group","requirement_ids":[]},
        {"id":"response","parent_id":"group","order":0,"title":"Response","purpose":"response","requirement_ids":ids}]})).unwrap();
        let bindings = super::chapters::attachment_chains(&input)
            .into_iter()
            .flatten()
            .map(|id| json!({"form_id":id,"chapter_id":"response"}))
            .collect::<Vec<_>>();
        apply(
            &input,
            &mut state,
            "bind_forms",
            &json!({"mode":"replace","bindings":bindings}),
        )
        .unwrap();
        apply(
            &input,
            &mut state,
            "read_evidence",
            &json!({"refs":all_evidence.iter().filter(|reference|!matches!(reference,evidence::EvidenceRef::ImageRegion{..})).collect::<Vec<_>>(),"max_bytes":16384}),
        )
        .unwrap();
        if !empty_packs.is_empty() {
            apply(
                &input,
                &mut state,
                "read_requirements",
                &json!({"mode":"packs","max_bytes":16384}),
            )
            .unwrap();
        }
        state.deliver_fixture_reads();
        let first = all_evidence
            .iter()
            .find(|reference| !matches!(reference, evidence::EvidenceRef::ImageRegion { .. }))
            .unwrap();
        apply(&input,&mut state,"put_slots",&json!({"mode":"replace","slots":[
        {"slot_id":"source-copy","chapter_id":"response","content":{"type":"source_copy","refs":[first]}},
        {"slot_id":"response-slot","chapter_id":"response","content":{"type":"editable_blank"},"blank_kind":"bidder_blank","match_query":"Fixture response evidence"}]})).unwrap();
        let fulfillments=ids.iter().map(|id|json!({"requirement_id":id,"primary_response_chapter_id":"response","target_refs":[{"type":"text_slot","slot_id":"response-slot"}]})).collect::<Vec<_>>();
        apply(
            &input,
            &mut state,
            "put_fulfillments",
            &json!({"fulfillments":fulfillments}),
        )
        .unwrap();
        fresh_check_reads(&input, &mut state, &all_evidence);
        apply(&input,&mut state,"submit_review",&json!({"requirement_ids":ids,"pack_ids":packs,"inspected_evidence":all_evidence,"issues":[]})).unwrap();
        apply(&input, &mut state, "finish_outline", &json!({})).unwrap();
        let projected =
            project_draft(&input, &state.input_sha256, &state.outline_run.tool_draft).unwrap();
        assert_eq!(
            projected.artifact.templates[0].text,
            super::evidence::resolve_evidence(&input, std::slice::from_ref(first)).unwrap()[0]
                .quote
        );
        assert!(!projected.artifact.requirements.is_empty());
        if input
            .source_units
            .iter()
            .all(|source| source.text.is_empty())
        {
            assert!(
                projected
                    .artifact
                    .requirements
                    .values()
                    .flat_map(|record| &record.evidence)
                    .any(|evidence| matches!(
                        evidence,
                        super::evidence::EvidenceRef::GridCell { .. }
                    ))
            );
            assert!(projected.artifact.fulfillments.iter().all(|row| {
                row.target_refs
                    .iter()
                    .any(|target| matches!(target, TargetRef::TextSlot { .. }))
            }));
        }
        validate_publication(&input, &projected.artifact, &projected.bindings).unwrap();
    }
}

#[test]
fn reads_in_same_model_response_do_not_authorize_source_copy_or_review() {
    let (input, mut state, _, refs) = discovered(false);
    let ids = state
        .outline_run
        .reading_packs
        .as_ref()
        .unwrap()
        .requirement_ids();
    apply(&input,&mut state,"put_chapters",&json!({"mode":"replace","chapters":[{"id":"response","parent_id":null,"order":0,"title":"Response","purpose":"response","requirement_ids":ids}]})).unwrap();
    apply(&input, &mut state, "read_evidence", &json!({"refs":refs})).unwrap();
    assert!(state.outline_run.tool_draft.delivered_evidence.is_empty());
    assert!(apply(&input,&mut state,"put_slots",&json!({"mode":"replace","slots":[{"slot_id":"copy","chapter_id":"response","content":{"type":"source_copy","refs":refs}}]})).unwrap_err().contains("outside_scope"));
}

#[test]
fn check_cannot_reuse_organize_receipts_after_transcript_clear() {
    let (input, mut state, pack, refs) = organized(false);
    state.transcript.clear();
    let ids = state
        .outline_run
        .reading_packs
        .as_ref()
        .unwrap()
        .requirement_ids();
    let review =
        json!({"requirement_ids":ids,"pack_ids":[pack],"inspected_evidence":refs,"issues":[]});
    assert!(!state.outline_run.tool_draft.delivered_evidence.is_empty());
    assert!(
        apply(&input, &mut state, "submit_review", &review)
            .unwrap_err()
            .contains("not-yet-delivered review evidence key")
    );
    apply(&input, &mut state, "read_evidence", &json!({"refs":refs})).unwrap();
    assert!(
        apply(&input, &mut state, "submit_review", &review)
            .unwrap_err()
            .contains("not-yet-delivered review evidence key")
    );
    state.deliver_fixture_reads();
    assert!(
        apply(&input, &mut state, "submit_review", &review)
            .unwrap_err()
            .contains("slot body")
    );
    fresh_check_reads(&input, &mut state, &refs);
    apply(&input, &mut state, "submit_review", &review).unwrap();
}

#[test]
fn check_reads_generated_prose_in_bounded_utf8_pages_before_finishing() {
    let (input, mut state, pack, refs) = organized(false);
    let generated = "生成说明：必须核实原文中的条件。🙂".repeat(120);
    agent::apply_for_duty(&input,&mut state,"put_slots",&json!({"mode":"upsert","slots":[{"slot_id":"explanation","chapter_id":"response","content":{"type":"generated_explanation","supporting_refs":refs},"text":generated}]}),Duty::Organize).unwrap();
    state.transcript.clear();
    apply(&input, &mut state, "read_evidence", &json!({"refs":refs})).unwrap();
    for id in ["fixed", "blank"] {
        apply(
            &input,
            &mut state,
            "read_outline",
            &json!({"mode":"slot_body","slot_id":id,"max_bytes":2048}),
        )
        .unwrap();
    }
    apply(
        &input,
        &mut state,
        "read_requirements",
        &json!({"mode":"packs","max_bytes":16384}),
    )
    .unwrap();
    state.deliver_fixture_reads();
    deterministic_claim_comparisons(&input, &mut state);
    let ids = state
        .outline_run
        .reading_packs
        .as_ref()
        .unwrap()
        .requirement_ids();
    apply(
        &input,
        &mut state,
        "submit_review",
        &json!({"requirement_ids":ids,"pack_ids":[pack],"inspected_evidence":refs,"issues":[]}),
    )
    .unwrap();
    assert!(
        apply(&input, &mut state, "finish_outline", &json!({}))
            .unwrap_err()
            .contains("generated slot body")
    );
    let mut cursor = 0;
    let mut version = serde_json::Value::Null;
    let mut actual = String::new();
    let mut pages = 0;
    loop {
        let mut args =
            json!({"mode":"slot_body","slot_id":"explanation","cursor":cursor,"max_bytes":512});
        if cursor > 0 {
            args["version"] = version.clone();
        }
        let page = apply(&input, &mut state, "read_outline", &args).unwrap();
        assert!(serde_json::to_vec(&page).unwrap().len() <= 512);
        actual.push_str(page["text"].as_str().unwrap());
        pages += 1;
        version = page["version"].clone();
        match page["next_cursor"].as_u64() {
            Some(next) => cursor = next,
            None => break,
        }
    }
    assert!(pages > 1);
    assert_eq!(actual, generated);
    assert!(apply(&input, &mut state, "finish_outline", &json!({})).is_err());
    state.deliver_fixture_reads();
    apply(&input, &mut state, "finish_outline", &json!({})).unwrap();
}

#[test]
fn host_reopen_invalidates_downstream_once_and_is_not_an_llm_tool() {
    let (input, mut state, pack, _) = organized(false);
    let revision = state
        .outline_run
        .reading_packs
        .as_ref()
        .unwrap()
        .session(&input, &pack)
        .unwrap()["pack"]["pack_revision"]
        .as_u64()
        .unwrap();
    let claim =
        agent::reopen_discovery_pack(&input, &mut state, &pack, revision, "authorized-reopen")
            .unwrap();
    assert!(state.outline_run.tool_draft.is_empty());
    assert_eq!(current(&input, &state), Duty::Discover);
    state
        .outline_run
        .tool_draft
        .chapters
        .push(super::tests::chapter("new"));
    let replay =
        agent::reopen_discovery_pack(&input, &mut state, &pack, revision, "authorized-reopen")
            .unwrap();
    assert_eq!(claim, replay);
    assert_eq!(state.outline_run.tool_draft.chapters.len(), 1);
    assert!(!agent::handles("reopen_discovery_pack"));
}

#[test]
fn mixed_text_and_empty_grid_pack_requires_every_structural_receipt() {
    let mut input = super::tests::input();
    input.source_units.push(crate::analysis::Source {
        source_unit_revision_id: "empty-table".into(),
        document_id: "doc".into(),
        ordinal: 1,
        text: String::new(),
        locator: json!({"heading_path":"资格","completeness":"complete"}),
    });
    input.structured_forms.push(json!({"form_definition_revision_id":"empty-table","source_unit_revision_id":"empty-table","definition":{"row_count":1,"column_count":1,"completeness":"complete","cells":[{"row":0,"column":0,"row_span":1,"column_span":1,"text":""}]}}));
    let mut work = DiscoverWork::plan(&input, 4096);
    let packs = work.claim(4);
    assert_eq!(packs.len(), 1);
    let pack = &packs[0];
    let refs = work.pack_evidence(&input, &pack.id).unwrap();
    assert!(!refs.is_empty());
    work.submit(
        &input,
        &pack.id,
        PackSubmit {
            call_id: "mixed-empty".into(),
            claim_token: pack.claim_token.clone(),
            pack_revision: pack.pack_revision,
            requirements: vec![],
            no_requirement_reason: Some("Contract-test negative result".into()),
            inspected_atom_ids: pack
                .atoms
                .iter()
                .filter(|atom| !atom.context_only)
                .map(|atom| atom.id.clone())
                .collect(),
        },
    )
    .unwrap();
    let mut state = checkpoint(&input);
    state.outline_run.reading_packs = Some(work);
    apply(&input,&mut state,"put_chapters",&json!({"mode":"replace","chapters":[{"id":"response","parent_id":null,"order":0,"title":"Response","purpose":"response","requirement_ids":[]}]})).unwrap();
    apply(&input,&mut state,"put_slots",&json!({"mode":"replace","slots":[{"slot_id":"blank","chapter_id":"response","content":{"type":"editable_blank"},"blank_kind":"bidder_blank","match_query":"Response"}]})).unwrap();
    apply(&input, &mut state, "read_evidence", &json!({"refs":refs})).unwrap();
    state.deliver_fixture_reads();
    let review =
        json!({"requirement_ids":[],"pack_ids":[pack.id],"inspected_evidence":refs,"issues":[]});
    apply(&input, &mut state, "submit_review", &review).unwrap();
    assert!(
        !state
            .outline_run
            .tool_draft
            .reviewed_pack_ids
            .contains(&pack.id)
    );
    assert!(apply(&input, &mut state, "finish_outline", &json!({})).is_err());
    let page = apply(
        &input,
        &mut state,
        "read_requirements",
        &json!({"mode":"packs","max_bytes":16384}),
    )
    .unwrap();
    assert!(
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["cell"]["start_byte"] == 0 && row["cell"]["end_byte"] == 0)
    );
    state.deliver_fixture_reads();
    apply(&input, &mut state, "submit_review", &review).unwrap();
    assert!(
        state
            .outline_run
            .tool_draft
            .reviewed_pack_ids
            .contains(&pack.id)
    );
    apply(&input, &mut state, "finish_outline", &json!({})).unwrap();
}

#[test]
fn image_metadata_never_grants_pixels_and_check_needs_its_own_visual_delivery() {
    let input = super::frozen::tests::python_fixture_input(0);
    let image_id = input
        .source_units
        .iter()
        .find(|source| source.locator["locator_kind"] == "image")
        .unwrap()
        .source_unit_revision_id
        .clone();
    let reference = super::evidence::EvidenceRef::ImageRegion {
        input_digest: super::evidence::input_digest(&input).unwrap(),
        image_id: image_id.clone(),
        region: "original".into(),
    };
    let mut state = checkpoint(&input);
    assert!(
        agent::apply_for_duty(
            &input,
            &mut state,
            "read_evidence",
            &json!({"refs":[reference]}),
            Duty::Organize
        )
        .unwrap_err()
        .contains("actual pixel delivery")
    );
    assert!(state.outline_run.tool_draft.delivered_evidence.is_empty());
    // This host-only call stands for independently verified actual image_url
    // delivery. The outer runtime separately tests the body-byte confirmation.
    agent::note_visual_delivery(&input, &mut state, &image_id, Duty::Organize).unwrap();
    assert!(
        state
            .outline_run
            .tool_draft
            .delivered_evidence
            .contains(&reference)
    );
    assert!(state.outline_run.tool_draft.check_reads.evidence.is_empty());
    state.transcript.clear();
    agent::note_visual_delivery(&input, &mut state, &image_id, Duty::Check).unwrap();
    assert!(
        state
            .outline_run
            .tool_draft
            .check_reads
            .evidence
            .contains(&reference)
    );
    assert!(
        agent::schemas_for(Duty::Check)
            .iter()
            .any(|tool| tool["function"]["name"] == "read_source_view")
    );
}

#[test]
fn full_duty_chain_never_marks_uncertain_or_contradicted_claim_ready() {
    for (quality, contradiction) in [("uncertain", false), ("explicit", true)] {
        let (input, mut state, pack, refs) = organized_with_quality(false, quality);
        assert_eq!(current(&input, &state), Duty::Check);
        fresh_check_reads(&input, &mut state, &refs);
        let ids = state
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .requirement_ids();
        let issues = if contradiction {
            json!([{"id":"conflict","code":"contradicted_obligation","description":"Source-bound requirement conflicts with its quoted condition; unresolved","requirement_ids":ids,"evidence":refs}])
        } else {
            json!([])
        };
        apply(&input,&mut state,"submit_review",&json!({"requirement_ids":ids,"pack_ids":[pack],"inspected_evidence":refs,"issues":issues})).unwrap();
        let finished = apply(&input, &mut state, "finish_outline", &json!({})).unwrap();
        assert_eq!(finished["needs_review"], true);
        assert_eq!(finished["semantic_ready"], false);
        let artifact = project_draft(&input, &state.input_sha256, &state.outline_run.tool_draft)
            .unwrap()
            .artifact;
        assert!(artifact.needs_review);
        assert!(!artifact.review_issues.is_empty());
    }
}

#[tokio::test]
#[ignore = "authorized bounded real-provider production duty regression"]
async fn private_real_production_claim_repair_cycle() {
    use crate::analysis::agent::{self as runtime, Model};
    use crate::authoring_runtime::AuthoringRuntimeContractV1;
    use serde_json::Value;
    assert_eq!(std::env::var("KB_ALLOW_PRIVATE_LIVE").as_deref(), Ok("1"));
    let root = std::path::PathBuf::from(std::env::var("KB_PRIVATE_PROJECTION_DIR").unwrap());
    let dry = std::env::var("KB_PREPARE_ONLY").as_deref() == Ok("1");
    let out = root.join(if dry {
        "production-claim-cycle-v2-preflight"
    } else {
        "production-claim-cycle-v2"
    });
    std::fs::create_dir(&out).expect("never replay billed run");
    let read = |p: std::path::PathBuf| -> Value {
        serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
    };
    let save = |name: &str, v: &Value| {
        std::fs::write(out.join(name), serde_json::to_vec_pretty(v).unwrap()).unwrap()
    };
    let full: FrozenInput =
        serde_json::from_value(read(root.join("actual-frozen/frozen-input.json"))).unwrap();
    let prior: Checkpoint =
        serde_json::from_value(read(root.join("handles-full-live-run/checkpoint.json"))).unwrap();
    let selection = read(root.join("semantic-selection.json"));
    let original = prior
        .outline_run
        .reading_packs
        .as_ref()
        .unwrap()
        .requirement_records()[selection["requirement_id"].as_str().unwrap()]
    .clone();
    let extra = read(root.join("cross-pack-project-selection-evidence.json"));
    let table = extra["row"][2]["reference"]["table_id"].as_str().unwrap();
    let form = full
        .structured_forms
        .iter()
        .find(|f| f["form_definition_revision_id"] == table)
        .unwrap()
        .clone();
    let mut ids = std::collections::BTreeSet::new();
    for e in &original.evidence {
        if let evidence::EvidenceRef::Text { unit_id, .. } = e {
            ids.insert(unit_id.clone());
        }
    }
    ids.insert(form["source_unit_revision_id"].as_str().unwrap().into());
    loop {
        let before = ids.len();
        let parents = full
            .source_units
            .iter()
            .filter(|s| ids.contains(&s.source_unit_revision_id))
            .filter_map(|s| s.locator["parent_section_id"].as_str())
            .collect::<Vec<_>>();
        for s in &full.source_units {
            if s.locator["section_id"]
                .as_str()
                .is_some_and(|id| parents.contains(&id))
            {
                ids.insert(s.source_unit_revision_id.clone());
            }
        }
        if before == ids.len() {
            break;
        }
    }
    let mut input = full.clone();
    input
        .source_units
        .retain(|s| ids.contains(&s.source_unit_revision_id));
    input.structured_forms = vec![form];
    crate::analysis::tools::validate_input(&input).unwrap();
    save("frozen-scoped-input.json", &json!(input));
    save(
        "scope.json",
        &json!({"kind":"real-provider production duty routing on derived carrier-complete subset; seeded actual previous extraction error; not whole-document acceptance","source_units":input.source_units.len(),"native_tables":1,"original_input_digest":evidence::input_digest(&full).unwrap(),"scoped_input_digest":evidence::input_digest(&input).unwrap(),"original_checkpoint_unchanged":true,"max_calls":12}),
    );
    let limits = read(root.join("handles-full-live-limits.json"));
    let config = runtime::Config::with_provider_for(
        AuthoringRuntimeContractV1::resolve_tools_from_environment().unwrap(),
        serde_json::from_value(limits["extraction"].clone()).unwrap(),
        Some(&input),
    )
    .unwrap();
    let mut state = checkpoint(&input);
    state.transcript.clear();
    let original_ids = original
        .evidence
        .iter()
        .filter_map(|e| {
            if let evidence::EvidenceRef::Text { unit_id, .. } = e {
                Some(unit_id.clone())
            } else {
                None
            }
        })
        .collect::<std::collections::BTreeSet<_>>();
    let mut work = DiscoverWork::plan_with_budget(&input, &|sessions| {
        for session in sessions {
            let atoms = session["pack"]["atoms"].as_array().unwrap();
            let grid = atoms.iter().any(|a| a["carrier"]["kind"] == "grid");
            let original_text = atoms.iter().any(|a| {
                a["carrier"]["evidence"]["unit_id"]
                    .as_str()
                    .is_some_and(|id| original_ids.contains(id))
            });
            if grid && original_text {
                return Ok(false);
            }
        }
        Ok(true)
    });
    assert!(work.planning_error().is_none());
    let claimed = work.claim(2);
    assert_eq!(
        work.pack_counts().total,
        2,
        "fixture must keep front table and original clause in separate normal planned packs"
    );
    let front = claimed
        .iter()
        .find(|p| {
            p.atoms
                .iter()
                .any(|a| matches!(a.carrier, discover::PackCarrier::Grid { .. }))
        })
        .unwrap()
        .clone();
    let pack = claimed.iter().find(|p| p.id != front.id).unwrap().clone();
    assert_eq!(front.id, "pack-0");
    assert_eq!(pack.id, "pack-1");
    let mut seeded = original.clone();
    for e in &mut seeded.evidence {
        match e {
            evidence::EvidenceRef::Text { input_digest, .. }
            | evidence::EvidenceRef::GridCell { input_digest, .. }
            | evidence::EvidenceRef::ImageRegion { input_digest, .. } => {
                *input_digest = pack.input_digest.clone()
            }
        }
    }
    state.outline_run.reading_packs = Some(work);
    let front_session = state
        .outline_run
        .reading_packs
        .as_ref()
        .unwrap()
        .session(&input, &front.id)
        .unwrap();
    let mut front_refs = extra["row"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["reference"].clone())
        .collect::<Vec<_>>();
    for reference in &mut front_refs {
        reference["input_digest"] = json!(front.input_digest);
    }
    let front_atom = front
        .atoms
        .iter()
        .find(|a| matches!(a.carrier, discover::PackCarrier::Grid { .. }))
        .unwrap();
    let result=apply(&input,&mut state,"submit_pack",&json!({"pack_id":front.id,"call_id":front_session["submission_operation_id"],"pack_revision":front.pack_revision,"claim_token":front.claim_token,"repair":false,"requirements":[{"description":"Synthetic fixture excludes the optional paper artifact.","kind":"format","obligation_strength":"informational","extraction_quality":"explicit","source_section_id":front_atom.section_id,"evidence":front_refs}],"no_requirement_reason":null,"inspected_atom_ids":[]})).unwrap();
    assert_ne!(result["ok"], false, "{result}");
    let operation = state
        .outline_run
        .reading_packs
        .as_ref()
        .unwrap()
        .session(&input, &pack.id)
        .unwrap()["submission_operation_id"]
        .clone();
    let seeded_submit = json!({"pack_id":pack.id,"call_id":operation,"pack_revision":pack.pack_revision,"claim_token":pack.claim_token,"repair":false,"requirements":[{"description":seeded.description,"kind":seeded.kind,"obligation_strength":seeded.obligation_strength,"extraction_quality":seeded.extraction_quality,"source_section_id":seeded.source_section_id,"evidence":seeded.evidence}],"no_requirement_reason":null,"inspected_atom_ids":[]});
    let result = apply(&input, &mut state, "submit_pack", &seeded_submit).unwrap();
    assert_ne!(result["ok"], false, "{result}");
    assert_eq!(current(&input, &state), Duty::Organize);
    let all_ids = state
        .outline_run
        .reading_packs
        .as_ref()
        .unwrap()
        .requirement_ids();
    apply(&input,&mut state,"put_chapters",&json!({"mode":"replace","chapters":[{"id":"response","parent_id":null,"order":0,"title":"投标文件递交响应","purpose":"response","requirement_ids":all_ids}]})).unwrap();
    apply(&input,&mut state,"put_slots",&json!({"mode":"replace","slots":[{"slot_id":"response-blank","chapter_id":"response","content":{"type":"editable_blank"},"blank_kind":"bidder_blank","match_query":seeded.description}]})).unwrap();
    apply(&input,&mut state,"put_fulfillments",&json!({"fulfillments":all_ids.iter().map(|id|json!({"requirement_id":id,"primary_response_chapter_id":"response","target_refs":[{"type":"text_slot","slot_id":"response-blank"}]})).collect::<Vec<_>>()})).unwrap();
    assert_eq!(current(&input, &state), Duty::Check);
    save("seeded-checkpoint.json", &json!(state));
    save(
        "fixture-evidence.json",
        &json!({"front_pack":front.id,"clause_pack":pack.id,"packs":2,"seeded_by_host":true,"full_document_acceptance":false}),
    );
    if dry {
        return;
    }
    let mut reopened = false;
    let mut last_error = String::new();
    let mut repeated_errors = 0usize;
    let mut stop_reason = "call_limit";
    for index in 1..=12 {
        if out.join("operator-pause").exists() {
            stop_reason = "operator_pause";
            break;
        }
        let duty = current(&input, &state);
        let mut brief = json!({"scope":"bounded production Check-repair-freshCheck regression; independently examine evidence, select tools freely; do not assume the seeded requirement is correct","input_digest":evidence::input_digest(&input).unwrap()});
        if duty == Duty::Discover {
            let work = state.outline_run.reading_packs.as_mut().unwrap();
            work.claim(1);
            brief["reading_packs"] = json!(work.inflight_sessions(&input));
        }
        let host = host_packet(
            &input,
            &state,
            48000,
            state.progress(&input),
            Value::Null,
            None,
        );
        let mut messages = vec![
            json!({"role":"system","content":agent::system_prompt(duty)}),
            json!({"role":"user","content":brief.to_string()}),
        ];
        messages.extend(state.transcript.clone());
        messages.push(json!({"role":"user","content":host.to_string()}));
        let bytes = crate::agent_runtime::chat::prepare(
            &config.provider,
            messages,
            agent::schemas_for(duty),
        )
        .await
        .unwrap();
        let mut body: Value = serde_json::from_slice(&bytes).unwrap();
        discover::compact_request(&mut body).unwrap();
        let accounting = crate::agent_runtime::chat::estimate_request_tokens(
            &body,
            &config.limits.tokenizer,
            config.limits.image_token_reserve,
            config.limits.token_safety_margin,
        )
        .unwrap();
        assert!(accounting.total_context_tokens <= 131072);
        save(&format!("request-{index}.json"), &body);
        save(&format!("accounting-{index}.json"), &json!(accounting));
        let started = std::time::Instant::now();
        let result = runtime::ConfiguredModel
            .turn(&config, &serde_json::to_vec(&body).unwrap())
            .await;
        let response = match result {
            Ok(r) => r,
            Err(e) => {
                save(
                    &format!("response-{index}.json"),
                    &json!({"error":e.to_string(),"seconds":started.elapsed().as_secs_f64()}),
                );
                break;
            }
        };
        save(
            &format!("response-{index}.json"),
            &json!({"duty":format!("{duty:?}"),"response":response,"seconds":started.elapsed().as_secs_f64()}),
        );
        state.deliver_fixture_reads();
        let calls=response.tool_calls.iter().map(|c|json!({"id":c.id,"type":"function","function":{"name":c.name,"arguments":c.arguments}})).collect::<Vec<_>>();
        state
            .transcript
            .push(json!({"role":"assistant","tool_calls":calls}));
        for call in response.tool_calls {
            let args: Value = serde_json::from_str(&call.arguments).unwrap();
            let result = apply(&input, &mut state, &call.name, &args);
            let output = match result {
                Ok(v) if v["ok"] == false => v,
                Ok(v) => json!({"ok":true,"result":v}),
                Err(e) => json!({"ok":false,"error":e}),
            };
            if output["ok"] == false {
                let error = output.to_string();
                if error == last_error {
                    repeated_errors += 1
                } else {
                    last_error = error;
                    repeated_errors = 1
                }
            } else {
                last_error.clear();
                repeated_errors = 0
            }
            state
                .transcript
                .push(json!({"role":"tool","tool_call_id":call.id,"content":output.to_string()}));
        }
        save(&format!("checkpoint-{index}.json"), &json!(state));
        if duty == Duty::Check
            && !reopened
            && state
                .outline_run
                .tool_draft
                .claim_comparisons
                .get(&format!("{}:0", pack.id))
                .is_some_and(|c| c.decisions.iter().any(|d| d.verdict != "supports"))
        {
            let issue = state.outline_run.tool_draft.review_issues.clone();
            let revision = state
                .outline_run
                .reading_packs
                .as_ref()
                .unwrap()
                .session(&input, &pack.id)
                .unwrap()["pack"]["pack_revision"]
                .as_u64()
                .unwrap();
            agent::reopen_discovery_pack(
                &input,
                &mut state,
                &pack.id,
                revision,
                "evidence-bound-reopen-1",
            )
            .unwrap();
            reopened = true;
            state.transcript.clear();
            state.transcript.push(json!({"role":"user","content":json!({"host_reopened_for_evidence_bound_review_issues":issue,"instruction":"Read the complete reopened pack and correct only evidence-supported obligations. Retain valid distinct claims; a rejected positive obligation may be removed or represented as explicitly not applicable. Use the host submission_operation_id and current session state."}).to_string()}));
        }
        save(
            "status.json",
            &json!({"calls_completed":index,"duty_next":format!("{:?}",current(&input,&state)),"reopened":reopened,"requirements":state.outline_run.reading_packs.as_ref().unwrap().requirement_records(),"comparisons":state.outline_run.tool_draft.claim_comparisons.len(),"issues":state.outline_run.tool_draft.review_issues,"finished":state.outline_run.tool_draft.finished}),
        );
        if repeated_errors >= 3 {
            stop_reason = "three_identical_tool_rejections";
            break;
        }
        if reopened && state.outline_run.tool_draft.finished {
            stop_reason = "finished";
            break;
        }
    }
    save("final-checkpoint.json", &json!(state));
    save(
        "termination.json",
        &json!({"stop_reason":stop_reason,"business_success":reopened&&state.outline_run.tool_draft.finished,"repeated_errors":repeated_errors}),
    );
    assert!(
        reopened && state.outline_run.tool_draft.finished,
        "scoped production acceptance incomplete: {stop_reason}"
    );
}

#[test]
#[ignore = "offline private two-pack checkpoint replay; requires KB_PRIVATE_PROJECTION_DIR"]
fn private_replay_crosspack_unresolved_reopen() {
    let root = std::path::PathBuf::from(std::env::var("KB_PRIVATE_PROJECTION_DIR").unwrap())
        .join("production-claim-cycle-v2");
    let input: FrozenInput =
        serde_json::from_slice(&std::fs::read(root.join("frozen-scoped-input.json")).unwrap())
            .unwrap();
    let mut state: Checkpoint =
        serde_json::from_slice(&std::fs::read(root.join("checkpoint-8.json")).unwrap()).unwrap();
    let pack_id = "pack-1";
    assert_eq!(current(&input, &state), Duty::Check);
    let comparison = &state.outline_run.tool_draft.claim_comparisons["pack-1:0"];
    assert!(
        comparison
            .decisions
            .iter()
            .any(|d| d.verdict == "uncertain")
    );
    let work = state.outline_run.reading_packs.as_ref().unwrap();
    let revision = work.session(&input, pack_id).unwrap()["pack"]["pack_revision"]
        .as_u64()
        .unwrap();
    let front = work.session(&input, "pack-0").unwrap();
    let owned = work.pack_evidence(&input, pack_id).unwrap();
    let mut unread = state.clone();
    unread.outline_run.tool_draft.check_reads.evidence.clear();
    let before = json!(unread);
    assert!(
        agent::reopen_discovery_pack(&input, &mut unread, pack_id, revision, "unread-probe")
            .is_err()
    );
    assert_eq!(json!(unread), before);
    agent::reopen_discovery_pack(
        &input,
        &mut state,
        pack_id,
        revision,
        "offline-unresolved-reopen",
    )
    .unwrap();
    assert_eq!(current(&input, &state), Duty::Discover);
    let work = state.outline_run.reading_packs.as_ref().unwrap();
    assert_eq!(work.session(&input, "pack-0").unwrap(), front);
    assert_eq!(work.pack_evidence(&input, pack_id).unwrap(), owned);
    let session = work.session(&input, pack_id).unwrap();
    let options = session["pack"]["condition_support_options"]
        .as_array()
        .unwrap();
    assert!(!options.is_empty());
    for option in options {
        assert_eq!(option["type"], "external_condition_support");
        assert!(option["support_key"].as_str().unwrap().starts_with("cs_"));
        for excerpt in option["excerpts"].as_array().unwrap() {
            let reference: evidence::EvidenceRef =
                serde_json::from_value(excerpt["evidence"].clone()).unwrap();
            assert!(!evidence::covered_by_union(&reference, &owned));
        }
    }
}

#[test]
#[ignore = "offline private Check evidence-key replay; requires KB_PRIVATE_PROJECTION_DIR"]
fn private_replay_review_keys_preserve_complete_coverage() {
    let root = std::path::PathBuf::from(std::env::var("KB_PRIVATE_PROJECTION_DIR").unwrap())
        .join("production-claim-cycle-v2");
    let input: FrozenInput =
        serde_json::from_slice(&std::fs::read(root.join("frozen-scoped-input.json")).unwrap())
            .unwrap();
    let mut state: Checkpoint =
        serde_json::from_slice(&std::fs::read(root.join("checkpoint-11.json")).unwrap()).unwrap();
    state.deliver_fixture_reads();
    let records = state
        .outline_run
        .reading_packs
        .as_ref()
        .unwrap()
        .requirement_records();
    let missing = records["pack-0:0"].evidence[..2].to_vec();
    let reads = state.outline_run.tool_draft.check_reads.evidence.clone();
    let keys = reads
        .iter()
        .map(|e| json!({"review_evidence_key":agent::review_evidence_key(&state,e).unwrap()}))
        .collect::<Vec<_>>();
    let mut args = json!({"requirement_ids":["pack-0:0","pack-1:0"],"pack_ids":[],"inspected_evidence":keys,"issues":[]});
    args["inspected_evidence"] = json!(
        reads
            .iter()
            .filter(|r| !missing.contains(r))
            .map(|e| json!({"review_evidence_key":agent::review_evidence_key(&state,e).unwrap()}))
            .collect::<Vec<_>>()
    );
    let error = apply(&input, &mut state.clone(), "submit_review", &args).unwrap_err();
    for e in &missing {
        assert!(
            error.contains(&agent::review_evidence_key(&state, e).unwrap()),
            "missing cell must return its selectable key: {error}"
        );
    }
    args["inspected_evidence"] = json!(keys);
    let mut forged = args.clone();
    forged["inspected_evidence"][0] = json!({"review_evidence_key":"rv_000000000000000000000000"});
    assert!(apply(&input, &mut state.clone(), "submit_review", &forged).is_err());
    let result = apply(&input, &mut state, "submit_review", &args).unwrap();
    assert_ne!(result["ok"], false, "{result}");
    assert!(!state.outline_run.tool_draft.finished);
}

#[test]
#[ignore = "offline private read transaction boundary; requires KB_PRIVATE_PROJECTION_DIR"]
fn private_claim_projection_error_grants_no_receipts() {
    let root = std::path::PathBuf::from(std::env::var("KB_PRIVATE_PROJECTION_DIR").unwrap())
        .join("production-claim-cycle-v2");
    let input: FrozenInput =
        serde_json::from_slice(&std::fs::read(root.join("frozen-scoped-input.json")).unwrap())
            .unwrap();
    let state: Checkpoint =
        serde_json::from_slice(&std::fs::read(root.join("seeded-checkpoint.json")).unwrap())
            .unwrap();
    let mut found = false;
    for max in (1024..=16384).step_by(17) {
        let mut probe = state.clone();
        let before = json!(probe);
        let result = apply(
            &input,
            &mut probe,
            "read_claim_evidence",
            &json!({"requirement_id":"pack-1:0","max_bytes":max}),
        );
        if let Err(error) = result {
            assert_eq!(json!(probe), before, "read error changed checkpoint");
            if error.contains("with receipt keys exceeds") {
                found = true;
                probe.turn += 1;
                apply(&input, &mut probe, "read_requirements", &json!({})).unwrap();
                assert!(probe.outline_run.tool_draft.check_reads.evidence.is_empty());
                assert!(
                    probe
                        .outline_run
                        .tool_draft
                        .check_reads
                        .pending_evidence
                        .is_empty()
                );
                break;
            }
        }
    }
    assert!(
        found,
        "fixture must exercise post-decoration failure after source projection"
    );
}
