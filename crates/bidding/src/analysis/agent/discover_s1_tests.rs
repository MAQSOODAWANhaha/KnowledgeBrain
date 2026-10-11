//! Synthetic native-DOCX structural context: navigation, delivery, and Check authority.
use super::*;
use crate::outline::{
    claim_review,
    discover::{DiscoverWork, PackRequirement, PackSubmit, ParsePack},
    evidence::EvidenceRef,
    frozen::{self, FrozenBuildInput, PARSER_CONTRACT_VERSION, ParsedDocument},
};
use prost::Message;

fn fixture() -> (FrozenInput, DiscoverWork, ParsePack) {
    let parsed = docparser::ReadResult::try_from(
        docparser::proto::ReadResponse::decode(
            include_bytes!("../../../tests/fixtures/synthetic-s1-docx-wire.pb").as_slice(),
        )
        .unwrap(),
    )
    .unwrap();
    let input = frozen::build_frozen_input(
        FrozenBuildInput {
            project_id: "synthetic-s1-project".into(),
            document_set_id: "synthetic-s1-set".into(),
            documents: vec![ParsedDocument {
                role: crate::analysis::DocumentRole::Primary,
                document_id: "synthetic-s1-docx".into(),
                parsed,
            }],
            document_relations: vec![],
            decisions: vec![],
        },
        vec![],
        PARSER_CONTRACT_VERSION,
    )
    .unwrap();
    let mut work = DiscoverWork::plan_with_budget(&input, &|sessions| {
        Ok(serde_json::to_vec(sessions).unwrap().len() <= 20_000)
    });
    let pack = work
        .claim(256)
        .into_iter()
        .find(|pack| {
            work.pack_evidence(&input, &pack.id)
                .unwrap()
                .iter()
                .any(|r| {
                    matches!(
                        r,
                        EvidenceRef::GridCell {
                            anchor_row: 64,
                            anchor_column: 1,
                            ..
                        }
                    )
                })
        })
        .unwrap();
    (input, work, pack)
}

fn submission(input: &FrozenInput, work: &DiscoverWork, pack: &ParsePack) -> PackSubmit {
    let evidence = work
        .pack_evidence(input, &pack.id)
        .unwrap()
        .into_iter()
        .find(|r| {
            matches!(
                r,
                EvidenceRef::GridCell {
                    anchor_row: 64,
                    anchor_column: 1,
                    ..
                }
            )
        })
        .unwrap();
    PackSubmit {
        call_id: "s1-submit".into(),
        claim_token: pack.claim_token.clone(),
        pack_revision: pack.pack_revision,
        requirements: vec![PackRequirement {
            condition_support: vec![],
            obligation_strength: "mandatory".into(),
            extraction_quality: "explicit".into(),
            description:
                "If optional maintenance is selected, submit the signed response for item 064."
                    .into(),
            evidence: vec![evidence],
            source_section_id: pack.atoms[0].section_id.clone(),
            kind: "material".into(),
        }],
        issues: vec![],
        no_requirement_reason: None,
        inspected_atom_ids: vec![],
    }
}

#[test]
fn s1_native_context_requires_durable_delivery_and_never_transfers_ownership() {
    let (input, work, pack) = fixture();
    let id = &pack.id;
    let identity = work.pack_wire_identity(id).unwrap();
    let session = work.session(&input, id).unwrap();
    assert_eq!(session["pack"]["condition_support_options"], json!([]));
    let dependency = session["pack"]["read_dependencies"][0]["read"]["args"]["dependency_id"]
        .as_str()
        .unwrap();
    assert!(
        !work
            .dependency_selection(&input, id, dependency)
            .unwrap()
            .is_empty()
    );
    let refs = structural_refs(&input, &pack);
    let owned_before = work.pack_evidence(&input, id).unwrap();
    assert!(refs.iter().all(|r| !owned_before.contains(r)));
    let (page, options) = work.related_read(&input, id, &refs).unwrap();
    assert!(!options.is_empty());
    assert!(!work.related_read_already_delivered(id, &options));
    assert!(
        work.canonical_pack(id)
            .unwrap()
            .condition_support_options
            .is_empty()
    );
    let mut submit = submission(&input, &work, &pack);
    let rejected = work.clone().submit(&input, id, submit.clone()).unwrap_err();
    assert!(
        serde_json::to_string(&rejected)
            .unwrap()
            .contains("structural_context_not_delivered")
    );
    let mut empty = submit.clone();
    empty.requirements.clear();
    empty.no_requirement_reason = Some("No additional obligations in this synthetic scan.".into());
    empty.inspected_atom_ids = pack.atoms.iter().map(|atom| atom.id.clone()).collect();
    let rejected = work.clone().submit(&input, id, empty).unwrap_err();
    assert!(
        serde_json::to_string(&rejected)
            .unwrap()
            .contains("structural_context_not_delivered")
    );
    submit.requirements[0].condition_support = options.clone();
    assert!(work.clone().submit(&input, id, submit.clone()).is_err());

    let body = json!({"messages":[{"role":"tool","tool_call_id":"s1-original","content":page.to_string()}]});
    let request = Request {
        scope: Scope {
            run: identity["run"].as_str().unwrap().into(),
            pack: id.clone(),
            revision: pack.pack_revision,
            generation: 1,
            request: "s1-related-request".into(),
        },
        body: serde_json::to_vec(&body).unwrap(),
        input_reserve: 1,
        output_reserve: 1,
    };
    let frame = coordinator::RelatedReadFrame {
        directory_page: None,
        call_id: "s1-original".into(),
        pack_revision: pack.pack_revision,
        input_digest: crate::outline::evidence::input_digest(&input).unwrap(),
        options: options.clone(),
        sealed_sha256: crate::outline::read_receipts::wire_hash(&body, "s1-original").unwrap(),
    };
    let mut state = super::super::retirement::checkpoint(&input);
    state.outline_run.reading_packs = Some(work);
    state.outline_run.discover_workers.insert(id.clone(), Worker {
        pending: Some(Pending::Received {
            request: request.clone(),
            response: json!({"content":"","finish_reason":"tool_calls","usage":null,"tool_calls":[{"id":"submit","name":"submit_pack","arguments":"{}"}]}),
        }),
        related_read_frames: vec![frame],
        ..Default::default()
    });
    for mode in ["unsent", "stale", "directory"] {
        let mut rejected = state.clone();
        let worker = rejected.outline_run.discover_workers.get_mut(id).unwrap();
        match mode {
            "unsent" => {
                worker.pending = Some(Pending::Sending {
                    request: request.clone(),
                })
            }
            "stale" => worker.related_read_frames[0].pack_revision += 1,
            "directory" => {
                worker.related_read_frames[0].directory_page = Some("directory-only".into())
            }
            _ => unreachable!(),
        }
        let before = json!(rejected.outline_run.reading_packs);
        assert!(
            confirm_worker_related_reads(&input, &mut rejected, &request).is_err(),
            "{mode}"
        );
        assert_eq!(json!(rejected.outline_run.reading_packs), before, "{mode}");
    }
    confirm_worker_related_reads(&input, &mut state, &request).unwrap();
    let confirmed = json!(state.outline_run.reading_packs);
    confirm_worker_related_reads(&input, &mut state, &request).unwrap();
    assert_eq!(json!(state.outline_run.reading_packs), confirmed);
    let mut restored: Checkpoint = serde_json::from_value(json!(state)).unwrap();
    let work = restored.outline_run.reading_packs.as_mut().unwrap();
    assert!(work.related_read_already_delivered(id, &options));
    assert_eq!(work.pack_evidence(&input, id).unwrap(), owned_before);
    let mut invalid_primary = submit.clone();
    invalid_primary.requirements[0].evidence = refs;
    assert!(work.clone().submit(&input, id, invalid_primary).is_err());
    let mut unassociated = submit.clone();
    unassociated.requirements[0].condition_support.clear();
    work.clone().submit(&input, id, unassociated).unwrap();
    work.submit(&input, id, submit).unwrap();
    assert_eq!(
        work.requirement_records()
            .values()
            .next()
            .unwrap()
            .condition_support,
        options
    );
    work.reopen_committed(&input, id, pack.pack_revision, "s1-reopen")
        .unwrap();
    assert!(work.dependency_selection(&input, id, dependency).is_err());
    assert!(work.confirm_related_read(&input, id, &options).is_err());
    assert!(
        work.canonical_pack(id)
            .unwrap()
            .condition_support_options
            .is_empty()
    );
    assert_eq!(work.pack_evidence(&input, id).unwrap(), owned_before);
}

#[test]
fn s1_structural_candidates_exclude_other_documents_siblings_and_later_prose() {
    let (input, _, pack) = fixture();
    let owners = pack
        .atoms
        .iter()
        .map(|a| a.source_unit_revision_id.clone())
        .collect();
    let eligible = crate::outline::discover::section_context_sources(&input, &owners);
    let intro = eligible
        .iter()
        .find(|s| s.text.contains("Only if"))
        .unwrap();
    let intro_id = intro.source_unit_revision_id.clone();
    for mode in ["foreign", "sibling", "later"] {
        let mut changed = input.clone();
        let source = changed
            .source_units
            .iter_mut()
            .find(|s| s.source_unit_revision_id == intro_id)
            .unwrap();
        match mode {
            "foreign" => source.document_id = "other-document".into(),
            "sibling" => source.locator["section_id"] = json!("unrelated-sibling-section"),
            "later" => source.ordinal = usize::MAX,
            _ => unreachable!(),
        }
        assert!(
            crate::outline::discover::section_context_sources(&changed, &owners)
                .iter()
                .all(|s| s.source_unit_revision_id != intro_id),
            "{mode}"
        );
    }
}

#[test]
fn s1_check_candidate_requires_fresh_original_reads_and_semantic_applicability() {
    let (input, work, pack) = fixture();
    let row = submission(&input, &work, &pack).requirements.remove(0);
    let record = crate::outline::discover::RequirementRecord {
        condition_support: vec![],
        obligation_strength: row.obligation_strength,
        extraction_quality: row.extraction_quality,
        description: row.description,
        evidence: row.evidence,
        source_section_id: row.source_section_id,
        kind: row.kind,
        dedup_group: "s1".into(),
    };
    let unit = claim_review::build(&input, "s1", &record).unwrap();
    let context = unit
        .evidence_units
        .iter()
        .find(|u| {
            u.role == "related_section_context"
                && u.excerpts.iter().any(|e| e.quote.contains("Only if"))
        })
        .unwrap();
    let handles = unit
        .evidence_units
        .iter()
        .map(|u| u.quote_handle.clone())
        .collect::<Vec<_>>();
    let mut comparison = claim_review::Comparison {
        requirement_id: "s1".into(),
        version: unit.version.clone(),
        declared_claims: vec![claim_review::DeclaredClaim {
            claim_handle: "requirement".into(),
            obligation: record.description.clone(),
            applicability: "optional maintenance".into(),
            original_fragments: vec![record.description.clone()],
            primary_handles: vec![handles[0].clone()],
            support_handles: handles[1..].to_vec(),
        }],
        observations: unit
            .evidence_units
            .iter()
            .map(|u| claim_review::Observation {
                quote_handle: u.quote_handle.clone(),
                polarity: "mixed".into(),
                applicability: "relevant".into(),
                condition: "optional maintenance".into(),
            })
            .collect(),
        decisions: vec![claim_review::Decision {
            claim_handle: "requirement".into(),
            evidence_handles: handles,
            verdict: "supports".into(),
            action: "retain".into(),
            resulting_claim: record.description,
        }],
    };
    assert!(claim_review::condition_support_options(&input, &unit, &comparison, &[]).is_err());
    let reads = unit
        .evidence_units
        .iter()
        .flat_map(|u| u.excerpts.iter().map(|e| e.evidence.clone()))
        .collect::<Vec<_>>();
    let options =
        claim_review::condition_support_options(&input, &unit, &comparison, &reads).unwrap();
    assert!(
        options
            .iter()
            .any(|o| o.evidence.contains(&context.excerpts[0].evidence))
    );
    comparison
        .observations
        .iter_mut()
        .find(|o| o.quote_handle == context.quote_handle)
        .unwrap()
        .applicability = "not_relevant".into();
    assert!(
        claim_review::condition_support_options(&input, &unit, &comparison, &reads)
            .unwrap()
            .iter()
            .all(|o| !o.evidence.contains(&context.excerpts[0].evidence))
    );
}

fn structural_refs(input: &FrozenInput, pack: &ParsePack) -> Vec<EvidenceRef> {
    let owners = pack
        .atoms
        .iter()
        .map(|atom| atom.source_unit_revision_id.clone())
        .collect();
    crate::outline::discover::section_context_sources(input, &owners)
        .into_iter()
        .map(|source| EvidenceRef::Text {
            input_digest: pack.input_digest.clone(),
            unit_id: source.source_unit_revision_id.clone(),
            start_byte: 0,
            end_byte: source.text.len(),
        })
        .collect()
}

fn deliver_context_page(
    input: &FrozenInput,
    state: &mut Checkpoint,
    pack: &ParsePack,
    refs: &[EvidenceRef],
    call: &str,
) {
    let work = state.outline_run.reading_packs.as_ref().unwrap();
    let identity = work.pack_wire_identity(&pack.id).unwrap();
    let (page, options) = work.related_read(input, &pack.id, refs).unwrap();
    let body = json!({"messages":[{"role":"tool","tool_call_id":call,"content":page.to_string()}]});
    let request = Request {
        scope: Scope {
            run: identity["run"].as_str().unwrap().into(),
            pack: pack.id.clone(),
            revision: pack.pack_revision,
            generation: 1,
            request: call.into(),
        },
        body: serde_json::to_vec(&body).unwrap(),
        input_reserve: 1,
        output_reserve: 1,
    };
    state.outline_run.discover_workers.insert(pack.id.clone(), Worker {
        pending: Some(Pending::Received { request: request.clone(), response: json!({"content":"","finish_reason":"tool_calls","usage":null,"tool_calls":[{"id":"continue","name":"read_evidence","arguments":"{}"}]}) }),
        related_read_frames: vec![coordinator::RelatedReadFrame {
            directory_page: None, call_id: call.into(), pack_revision: pack.pack_revision,
            input_digest: pack.input_digest.clone(), options,
            sealed_sha256: crate::outline::read_receipts::wire_hash(&body, call).unwrap(),
        }], ..Default::default()
    });
    confirm_worker_related_reads(input, state, &request).unwrap();
}

#[test]
fn s1_partial_context_delivery_blocks_submission_until_full_union_survives_reload() {
    let (input, work, pack) = fixture();
    let submit = submission(&input, &work, &pack);
    let mut first = Vec::new();
    let mut rest = Vec::new();
    for reference in structural_refs(&input, &pack) {
        let EvidenceRef::Text {
            input_digest,
            unit_id,
            end_byte,
            ..
        } = reference
        else {
            unreachable!()
        };
        let source = input
            .source_units
            .iter()
            .find(|s| s.source_unit_revision_id == unit_id)
            .unwrap();
        let middle = source.text.char_indices().nth(1).unwrap().0;
        first.push(EvidenceRef::Text {
            input_digest: input_digest.clone(),
            unit_id: unit_id.clone(),
            start_byte: 0,
            end_byte: middle,
        });
        rest.push(EvidenceRef::Text {
            input_digest,
            unit_id,
            start_byte: middle,
            end_byte,
        });
    }
    let mut state = super::super::retirement::checkpoint(&input);
    state.outline_run.reading_packs = Some(work);
    deliver_context_page(&input, &mut state, &pack, &first, "s1-first-page");
    let partial = state.outline_run.reading_packs.as_ref().unwrap();
    let error = partial
        .clone()
        .submit(&input, &pack.id, submit.clone())
        .unwrap_err();
    assert!(
        serde_json::to_string(&error)
            .unwrap()
            .contains("structural_context_not_delivered")
    );
    let mut state: Checkpoint = serde_json::from_value(json!(state)).unwrap();
    deliver_context_page(&input, &mut state, &pack, &rest, "s1-last-page");
    let mut state: Checkpoint = serde_json::from_value(json!(state)).unwrap();
    let work = state.outline_run.reading_packs.as_mut().unwrap();
    work.submit(&input, &pack.id, submit).unwrap();
    assert!(
        work.requirement_records()
            .values()
            .next()
            .unwrap()
            .condition_support
            .is_empty(),
        "delivery does not force a semantic association"
    );
}
