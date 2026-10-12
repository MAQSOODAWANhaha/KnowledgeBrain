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
    let mut input = frozen::build_frozen_input(
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
    // Required context is a precise synthetic parser relation, not a prose heuristic.
    let intro = input
        .source_units
        .iter()
        .find(|s| s.text.contains("Only if"))
        .unwrap()
        .source_unit_revision_id
        .clone();
    let intro_source = input
        .source_units
        .iter()
        .find(|s| s.source_unit_revision_id == intro)
        .unwrap();
    let intro_doc = intro_source.document_id.clone();
    let intro_unit = intro_source.locator["unit_id"].as_str().unwrap().to_owned();
    for form in &input.structured_forms {
        let owner = input
            .source_units
            .iter()
            .find(|s| {
                Some(s.source_unit_revision_id.as_str()) == form["source_unit_revision_id"].as_str()
            })
            .unwrap();
        input.document_relations.push(serde_json::from_value(json!({"id":format!("s1-required-{}",owner.source_unit_revision_id),"from":{"document_id":owner.document_id,"unit_id":owner.locator["unit_id"]},"to":{"document_id":intro_doc,"unit_id":intro_unit},"kind":"explicit_reference","status":"confirmed","required":true,"basis":form["definition"]["cells"].as_array().unwrap().iter().find_map(|c|c["text"].as_str().filter(|text|!text.is_empty())).unwrap(),"locator":owner.locator})).unwrap());
    }
    let mut work = DiscoverWork::plan_with_budget(&input, &|sessions| {
        Ok(serde_json::to_vec(sessions).unwrap().len() <= 20_000)
    });
    assert!(
        work.planning_error().is_none(),
        "native S1 fixture planning failed: {:?}",
        work.planning_error()
    );
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
    let mut directory = session["pack"].clone();
    let dependency = loop {
        if let Some(row) = directory["read_dependencies"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["type"] == "structural_context_candidate")
        {
            break row["read"]["args"]["dependency_id"]
                .as_str()
                .unwrap()
                .to_owned();
        }
        let cursor = directory["next_dependency_cursor"]
            .as_str()
            .expect("fixture must contain an optional scoped structural candidate");
        directory = work.dependency_directory_page(&input, id, cursor).unwrap();
    };
    assert!(
        !work
            .dependency_selection(&input, id, &dependency)
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
        user_page: None,
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
    assert!(work.dependency_selection(&input, id, &dependency).is_err());
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
    use crate::outline::discover::PackCarrier;
    let owned = pack
        .atoms
        .iter()
        .filter(|a| !a.context_only)
        .flat_map(|atom| match &atom.carrier {
            PackCarrier::Text { evidence } | PackCarrier::Image { evidence, .. } => {
                vec![evidence.clone()]
            }
            PackCarrier::Grid {
                table_id, cells, ..
            } => cells
                .iter()
                .filter(|c| c.start_byte < c.end_byte)
                .map(|c| EvidenceRef::GridCell {
                    input_digest: pack.input_digest.clone(),
                    table_id: table_id.clone(),
                    anchor_row: c.anchor_row,
                    anchor_column: c.anchor_column,
                    start_byte: c.start_byte,
                    end_byte: c.end_byte,
                })
                .collect(),
            PackCarrier::Structure { .. } => vec![],
        })
        .collect::<Vec<_>>();
    crate::outline::source_support::select(input, &owned)
        .unwrap()
        .into_iter()
        .map(|s| s.evidence)
        .collect()
}

fn s1_wire_evidence(worker: &Worker, evidence: &Value) -> EvidenceRef {
    let decoded=worker.registry.decode(json!({"wire_scope":worker.registry.fixture_encode(json!({})).unwrap()["wire_scope"],"refs":[evidence]}),false).unwrap();
    let key = decoded["refs"][0]["source_key"]
        .as_str()
        .expect("actual wire must use an issued source key");
    worker
        .source_keys
        .get(key)
        .expect("source key must have been issued by this worker")
        .clone()
}

fn split_support_page(
    input: &FrozenInput,
    refs: &[EvidenceRef],
) -> (Vec<EvidenceRef>, Vec<EvidenceRef>) {
    let mut first = Vec::new();
    let mut rest = Vec::new();
    for reference in refs {
        let Some((start, end)) = reference.range() else {
            first.push(reference.clone());
            continue;
        };
        let excerpts =
            crate::outline::evidence::resolve_evidence(input, std::slice::from_ref(reference))
                .unwrap();
        let boundary = start + excerpts[0].quote.chars().next().unwrap().len_utf8();
        let mut left = serde_json::to_value(reference).unwrap();
        left["end_byte"] = json!(boundary);
        first.push(serde_json::from_value(left).unwrap());
        if boundary < end {
            let mut right = serde_json::to_value(reference).unwrap();
            right["start_byte"] = json!(boundary);
            rest.push(serde_json::from_value(right).unwrap());
        }
    }
    assert!(!rest.is_empty());
    (first, rest)
}

fn stage_context_page(
    input: &FrozenInput,
    state: &mut Checkpoint,
    pack: &ParsePack,
    refs: &[EvidenceRef],
    call: &str,
) -> Request {
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
    let worker = state
        .outline_run
        .discover_workers
        .entry(pack.id.clone())
        .or_default();
    worker.pending = Some(Pending::Received {
        request: request.clone(),
        response: json!({"content":"","finish_reason":"tool_calls","usage":null,"tool_calls":[{"id":"continue","name":"read_evidence","arguments":"{}"}]}),
    });
    worker
        .related_read_frames
        .push(coordinator::RelatedReadFrame {
            user_page: None,
            directory_page: None,
            call_id: call.into(),
            pack_revision: pack.pack_revision,
            input_digest: pack.input_digest.clone(),
            options,
            sealed_sha256: crate::outline::read_receipts::wire_hash(&body, call).unwrap(),
        });
    request
}

fn deliver_context_page(
    input: &FrozenInput,
    state: &mut Checkpoint,
    pack: &ParsePack,
    refs: &[EvidenceRef],
    call: &str,
) {
    let request = stage_context_page(input, state, pack, refs, call);
    confirm_worker_related_reads(input, state, &request).unwrap();
}

#[test]
fn s1_partial_context_delivery_blocks_submission_until_full_union_survives_reload() {
    let (input, work, pack) = fixture();
    let submit = submission(&input, &work, &pack);
    let (first, rest) = split_support_page(&input, &structural_refs(&input, &pack));
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

#[test]
fn s1_failed_claim_reads_and_repairs_on_the_same_state_without_terminal_or_foreign_credit() {
    use crate::outline::discover::PackStatus;
    let (input, work, pack) = fixture();
    let mut submit = submission(&input, &work, &pack);
    let mut state = super::super::retirement::checkpoint(&input);
    state.outline_run.reading_packs = Some(work);
    let rejected = state
        .outline_run
        .reading_packs
        .as_mut()
        .unwrap()
        .submit(&input, &pack.id, submit.clone())
        .unwrap_err();
    assert!(
        rejected
            .errors
            .iter()
            .any(|e| e.code == "structural_context_not_delivered")
    );
    assert_eq!(
        state
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .status(&pack.id),
        Some(PackStatus::Failed)
    );
    let mut state: Checkpoint = serde_json::from_value(json!(state)).unwrap();
    let refs = structural_refs(&input, &pack);
    let (first, rest) = split_support_page(&input, &refs);
    let request = stage_context_page(&input, &mut state, &pack, &first, "failed-first-page");
    for mode in ["terminal", "foreign", "stale_revision"] {
        let mut rejected = state.clone();
        let mut altered = request.clone();
        match mode {
            "terminal" => {
                rejected
                    .outline_run
                    .discover_workers
                    .get_mut(&pack.id)
                    .unwrap()
                    .pending = Some(Pending::Failed {
                    request: request.clone(),
                    code: "BUSINESS_REJECTED".into(),
                })
            }
            "foreign" => {
                let other = rejected
                    .outline_run
                    .reading_packs
                    .as_ref()
                    .unwrap()
                    .pack_ids()
                    .into_iter()
                    .find(|id| id != &pack.id)
                    .unwrap();
                altered.scope.pack = other.clone();
                let mut worker = rejected.outline_run.discover_workers[&pack.id].clone();
                if let Some(Pending::Received { request, .. }) = &mut worker.pending {
                    *request = altered.clone();
                }
                rejected.outline_run.discover_workers.insert(other, worker);
            }
            "stale_revision" => altered.scope.revision += 1,
            _ => unreachable!(),
        }
        let before = json!(rejected);
        assert!(
            confirm_worker_related_reads(&input, &mut rejected, &altered).is_err(),
            "{mode}"
        );
        assert_eq!(json!(rejected), before, "{mode}: no partial credit");
    }
    confirm_worker_related_reads(&input, &mut state, &request).unwrap();
    assert_eq!(
        state
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .status(&pack.id),
        Some(PackStatus::Failed),
        "reading does not change the repair state"
    );
    submit.call_id = "failed-partial-repair".into();
    let partial = state
        .outline_run
        .reading_packs
        .as_mut()
        .unwrap()
        .repair(&input, &pack.id, submit)
        .unwrap_err();
    assert!(
        partial
            .errors
            .iter()
            .any(|e| e.code == "structural_context_not_delivered")
    );
    let current = state
        .outline_run
        .reading_packs
        .as_ref()
        .unwrap()
        .canonical_pack(&pack.id)
        .unwrap()
        .clone();
    assert!(current.pack_revision > pack.pack_revision);
    assert!(
        current.condition_support_options.is_empty(),
        "repair rotates the claim and does not inherit old delivery"
    );
    let mut state: Checkpoint = serde_json::from_value(json!(state)).unwrap();
    // A previous durable request cannot authorize the repaired claim.
    let before = json!(state);
    assert!(confirm_worker_related_reads(&input, &mut state, &request).is_err());
    assert_eq!(json!(state), before);
    deliver_context_page(&input, &mut state, &current, &first, "repair-first-page");
    let mut state: Checkpoint = serde_json::from_value(json!(state)).unwrap();
    deliver_context_page(&input, &mut state, &current, &rest, "repair-last-page");
    assert!(
        state.outline_run.discover_workers[&pack.id]
            .related_read_frames
            .is_empty()
    );
    let work = state.outline_run.reading_packs.as_mut().unwrap();
    let mut repaired = submission(&input, work, &current);
    repaired.call_id = "failed-complete-repair".into();
    work.repair(&input, &pack.id, repaired).unwrap();
    let restored: Checkpoint = serde_json::from_value(json!(state)).unwrap();
    let work = restored.outline_run.reading_packs.as_ref().unwrap();
    assert_eq!(work.status(&pack.id), Some(PackStatus::Committed));
    assert_eq!(work.requirement_records().len(), 1);
    assert!(
        work.requirement_records()
            .values()
            .next()
            .unwrap()
            .condition_support
            .is_empty(),
        "delivery does not force applicability or grant Check credit"
    );
    assert!(
        restored
            .outline_run
            .tool_draft
            .check_reads
            .evidence
            .is_empty()
    );
}

struct S1Journal(std::sync::Mutex<Checkpoint>);
#[async_trait]
impl Journal for S1Journal {
    async fn load(&self) -> Result<Option<Checkpoint>, AgentError> {
        Ok(Some(self.0.lock().unwrap().clone()))
    }
    async fn reserve(&self, _: &Checkpoint, _: &[u8]) -> Result<Option<usize>, AgentError> {
        panic!("the worker must use reserve_discover_worker")
    }
    async fn save(&self, state: &Checkpoint, _: &Value) -> Result<(), AgentError> {
        *self.0.lock().unwrap() = serde_json::from_value(json!(state)).unwrap();
        Ok(())
    }
}

#[tokio::test]
async fn s1_actual_worker_rejects_reads_continuations_repairs_reloads_and_requires_fresh_check() {
    s1_worker_observation_lifecycle(false, false).await;
}

#[tokio::test]
async fn s1_unique_observations_stop_at_capacity_without_losing_notes_or_retrying_after_reload() {
    s1_worker_observation_lifecycle(true, true).await;
}

#[tokio::test]
async fn s1_repeated_observations_complete_at_original_narrow_window_with_full_durable_audit() {
    s1_worker_observation_lifecycle(false, true).await;
}

fn s1_plain_assistant_text(message: &Value) -> Option<String> {
    if message["role"] != "assistant" {
        return None;
    }
    if message
        .get("tool_calls")
        .is_some_and(|calls| !calls.is_null() && !calls.as_array().is_some_and(Vec::is_empty))
    {
        return None;
    }
    match &message["content"] {
        Value::String(text) => Some(text.clone()),
        Value::Array(blocks) => Some(
            blocks
                .iter()
                .map(|block| {
                    assert_eq!(
                        block["type"], "text",
                        "synthetic observation must contain only text blocks"
                    );
                    block["text"]
                        .as_str()
                        .expect("text block string")
                        .to_owned()
                })
                .collect::<String>(),
        ),
        other => panic!("unexpected synthetic assistant content shape: {other}"),
    }
}

async fn s1_worker_observation_lifecycle(unique_observations: bool, narrow_window: bool) {
    // The source is derived from the synthetic native DOCX, with a longer
    // synthetic introduction solely to force production source pagination.
    let (mut input, _, _) = fixture();
    let intro = input
        .source_units
        .iter_mut()
        .find(|source| source.text.contains("Only if"))
        .unwrap();
    intro.text = intro.text.repeat(if narrow_window { 128 } else { 1024 });
    let intro_id = intro.source_unit_revision_id.clone();
    let intro_text = intro.text.clone();
    assert_eq!(
        intro_text.len(),
        if narrow_window { 22_528 } else { 180_224 },
        "original narrow window stays unchanged; longer functional source separates paging cost from audit cost"
    );
    let mut work = DiscoverWork::plan_with_budget(&input, &|sessions| {
        Ok(serde_json::to_vec(sessions).unwrap().len() <= 20_000)
    });
    assert!(
        work.planning_error().is_none(),
        "{:?}",
        work.planning_error()
    );
    let packs = work.claim(usize::MAX);
    let target = packs
        .iter()
        .find(|pack| {
            work.pack_evidence(&input, &pack.id)
                .unwrap()
                .iter()
                .any(|reference| {
                    matches!(
                        reference,
                        EvidenceRef::GridCell {
                            anchor_row: 64,
                            anchor_column: 1,
                            ..
                        }
                    )
                })
        })
        .unwrap()
        .clone();
    // Unrelated scopes are explicit domain fixtures, not simulated worker
    // successes. Only the target below exercises the full worker lifecycle.
    for pack in packs.iter().filter(|pack| pack.id != target.id) {
        let refs = structural_refs(&input, pack);
        if !refs.is_empty() {
            let (_, options) = work.related_read(&input, &pack.id, &refs).unwrap();
            work.confirm_related_read(&input, &pack.id, &options)
                .unwrap();
        }
        work.submit(
            &input,
            &pack.id,
            PackSubmit {
                call_id: format!("s1-seed-{}", pack.id),
                claim_token: pack.claim_token.clone(),
                pack_revision: pack.pack_revision,
                requirements: vec![],
                issues: vec![],
                no_requirement_reason: Some(
                    "Synthetic protocol-only seed, not a semantic extraction assertion.".into(),
                ),
                inspected_atom_ids: pack
                    .atoms
                    .iter()
                    .filter(|atom| !atom.context_only)
                    .map(|atom| atom.id.clone())
                    .collect(),
            },
        )
        .unwrap();
    }
    let mut state = super::super::retirement::checkpoint(&input);
    state.turn = 0;
    state.outline_run.reading_packs = Some(work);
    state
        .outline_run
        .discover_workers
        .insert(target.id.clone(), Worker::default());
    let mut config = crate::analysis::tests::config();
    let cancel = CancellationToken::new();
    let probe_journal = S1Journal(std::sync::Mutex::new(state.clone()));
    let mut probe = state.clone();
    let baseline = PackHost {
        input: &input,
        config: &config,
        state: &mut probe,
        journal: &probe_journal,
        cancel: &cancel,
    }
    .prepare_candidate(&target.id, false)
    .await
    .unwrap();
    // The narrow regression retains its original window. The functional
    // fixture measures retained-candidate space and the actual full first
    // support request, then selects an explicit intermediate pagination window.
    // This is test-only sizing; production config and admission are unchanged.
    config.limits.max_context_tokens =
        baseline.input_reserve as usize + baseline.output_reserve as usize + 1300;
    if !narrow_window {
        let work = state.outline_run.reading_packs.as_ref().unwrap();
        let required = work.required_support_refs(&input, &target.id).unwrap();
        let mut envelope: Value = serde_json::from_slice(&baseline.body).unwrap();
        let (page, _) =
            crate::outline::discover::automatic_support_page(&input, &target, &required).unwrap();
        envelope["messages"]
            .as_array_mut()
            .unwrap()
            .push(json!({"role":"user","content":page.to_string()}));
        let packet = envelope["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|m| m["content"].as_str())
            .filter_map(|s| serde_json::from_str::<Value>(s).ok())
            .find(|p| p["reading_packs"].is_array())
            .unwrap();
        let session = &packet["reading_packs"][0];
        let pack = &session["pack"];
        let rejected = json!({"pack_id":pack["id"],"call_id":session["submission_operation_id"],"claim_token":pack["claim_token"],"pack_revision":pack["pack_revision"],"repair":true,"requirements":[],"issues":[],"no_requirement_reason":"Synthetic host protocol response only; no semantic extraction assertion.","inspected_atom_ids":pack["atoms"].as_array().unwrap().iter().filter(|a|a["context_only"]!=true).map(|a|json!({"atom_key":a["atom_key"]})).collect::<Vec<_>>()});
        for _ in 0..=required.len() {
            envelope["messages"].as_array_mut().unwrap().push(json!({"role":"user","content":json!({"rejected_submission_audit":{"original_arguments_text":rejected.to_string(),"settled_submission_failure":"Required structural support pages remain"}}).to_string()}));
        }
        // Measure the real first support request separately, at the unchanged
        // roomy synthetic config. The functional window must be below this
        // full-page size, otherwise this fixture never exercises pagination.
        let roomy_config = crate::analysis::tests::config();
        let mut full_probe = state.clone();
        let long_original = required
            .iter()
            .find(|r| matches!(r,EvidenceRef::Text{unit_id,..} if unit_id==&intro_id))
            .unwrap()
            .clone();
        let (full_page, full_options) =
            crate::outline::discover::automatic_support_page(&input, &target, &[long_original])
                .unwrap();
        full_probe
            .outline_run
            .discover_workers
            .get_mut(&target.id)
            .unwrap()
            .related_read_frames
            .push(coordinator::RelatedReadFrame {
                user_page: Some(full_page),
                directory_page: None,
                call_id: String::new(),
                pack_revision: target.pack_revision,
                input_digest: full_probe.input_sha256.clone(),
                options: full_options,
                sealed_sha256: None,
            });
        let full_request = PackHost {
            input: &input,
            config: &roomy_config,
            state: &mut full_probe,
            journal: &probe_journal,
            cancel: &cancel,
        }
        .prepare_candidate(&target.id, false)
        .await
        .unwrap();
        let full_total = (full_request.input_reserve + full_request.output_reserve) as usize;
        // Remove only the planned original page; the measured envelope keeps
        // complete rejected arguments. Thus the floor explicitly includes
        // audit space rather than reclaiming it by dropping failed candidates.
        let baseline_message_count: usize = serde_json::from_slice::<Value>(&baseline.body)
            .unwrap()["messages"]
            .as_array()
            .unwrap()
            .len();
        envelope["messages"]
            .as_array_mut()
            .unwrap()
            .remove(baseline_message_count);
        let audit_floor = crate::agent_runtime::chat::estimate_request_tokens_with_reserve(
            &envelope,
            &config.limits.tokenizer,
            config.limits.image_token_reserve,
            config.limits.token_safety_margin,
            config.provider.output_token_reserve as usize,
        )
        .unwrap()
        .total_context_tokens;
        assert!(
            audit_floor < full_total,
            "synthetic fixture needs room between retained-candidate floor and full source page: floor={audit_floor},full={full_total}"
        );
        config.limits.max_context_tokens = audit_floor + (full_total - audit_floor) * 3 / 4;
        eprintln!(
            "S1 functional measured audit_floor={audit_floor}, full_long_page={full_total}, window={}",
            config.limits.max_context_tokens
        );
        assert!(config.limits.max_context_tokens < full_total);
    }
    state.config_sha256 = digest(&config).unwrap();
    let journal = S1Journal(std::sync::Mutex::new(state.clone()));
    let mut continuation_calls = 0;
    let mut delivered_intro = String::new();
    let mut finished = false;
    let mut physical_calls = 0;
    let mut capacity_stopped = false;
    let mut saw_repeated_retired = false;
    let mut emitted_observations = Vec::<String>::new();
    let mut rejected_arguments = Vec::<String>::new();
    for step in 0..64 {
        // Recreate the borrowing adapter over the same persisted checkpoint;
        // no frame, request identity or worker status is manually injected.
        let mut host = PackHost {
            input: &input,
            config: &config,
            state: &mut state,
            journal: &journal,
            cancel: &cancel,
        };
        let Some(request) = host.next(&BTreeSet::new()).await.unwrap() else {
            assert!(
                narrow_window && unique_observations,
                "repeated-observation lifecycle must not accept capacity as success: calls={physical_calls}, intro_pages={continuation_calls}, window={}",
                config.limits.max_context_tokens
            );
            assert!(
                matches!(&host.state.outline_run.discover_workers[&target.id].pending,Some(Pending::Failed {code,..}) if code=="DISCOVER_READ_CAPACITY_EXCEEDED")
            );
            capacity_stopped = true;
            state = journal.load().await.unwrap().unwrap();
            break;
        };
        assert_eq!(request.scope.pack, target.id);
        assert!(
            request.input_reserve + request.output_reserve
                <= config.limits.max_context_tokens as u64
        );
        let wire: Value = serde_json::from_slice(&request.body).unwrap();
        if !unique_observations {
            let observation = "Synthetic offline lifecycle response.";
            let canonical_count = host.state.outline_run.discover_workers[&target.id]
                .history
                .iter()
                .filter(|message| {
                    message["role"] == "assistant" && message["content"] == observation
                })
                .count();
            saw_repeated_retired |= canonical_count > 1;
            let retired_count = wire["messages"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(s1_plain_assistant_text)
                .filter(|text| text == observation)
                .count();
            assert_eq!(
                retired_count,
                usize::from(canonical_count > 0),
                "actual wire must retain exactly one observation when canonical retired observations exist, step {step}"
            );
        }
        let payloads = wire["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|message| {
                let text = match &message["content"] {
                    Value::String(s) => s.clone(),
                    Value::Array(parts) => parts
                        .iter()
                        .filter_map(|p| p["text"].as_str())
                        .collect::<String>(),
                    _ => return None,
                };
                serde_json::from_str::<Value>(&text).ok()
            })
            .collect::<Vec<_>>();
        let wire_candidates = payloads
            .iter()
            .filter_map(|payload| {
                payload["rejected_submission_audit"]["original_arguments_text"].as_str()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            wire_candidates,
            rejected_arguments
                .last()
                .map(String::as_str)
                .into_iter()
                .collect::<Vec<_>>(),
            "wire must retain exactly the latest complete failed candidate"
        );
        let page = &payloads
            .iter()
            .find(|p| p["automatic_support_page"].is_object())
            .expect("every incomplete-support turn carries actual source")["automatic_support_page"]
            ["read"];
        let before_options = json!(
            host.state
                .outline_run
                .reading_packs
                .as_ref()
                .unwrap()
                .canonical_pack(&target.id)
                .unwrap()
                .condition_support_options
        );
        for excerpt in page["excerpts"].as_array().into_iter().flatten() {
            if matches!(s1_wire_evidence(&host.state.outline_run.discover_workers[&target.id], &excerpt["evidence"]), EvidenceRef::Text{unit_id,..} if unit_id==intro_id)
            {
                let quote = excerpt["quote"].as_str().unwrap();
                assert!(!quote.is_empty());
                delivered_intro.push_str(quote);
                continuation_calls += 1;
            }
        }
        let session = &payloads
            .iter()
            .find(|p| p["reading_packs"].is_array())
            .unwrap()["reading_packs"][0];
        let pack = &session["pack"];
        let scope = payloads
            .iter()
            .find_map(|p| p["wire_scope"].as_str())
            .unwrap();
        let name = "submit_pack";
        let args = json!({"wire_scope":scope,"pack_id":pack["id"],"call_id":session["submission_operation_id"],"claim_token":pack["claim_token"],"pack_revision":pack["pack_revision"],"repair":session["status"]=="failed","requirements":[],"issues":[],"no_requirement_reason":"Synthetic host protocol response only; no semantic extraction assertion.","inspected_atom_ids":pack["atoms"].as_array().unwrap().iter().filter(|a|a["context_only"]!=true).map(|a|json!({"atom_key":a["atom_key"]})).collect::<Vec<_>>()});
        let observation = if unique_observations {
            format!(
                "Unique observation {step}: {}",
                "Synthetic evidence observation retained in its original sequence. ".repeat(3)
            )
        } else {
            "Synthetic offline lifecycle response.".into()
        };
        emitted_observations.push(observation.clone());
        let response = ChatTurn {
            content: observation,
            finish_reason: "tool_calls".into(),
            usage: Some(knowledge::models::ChatUsage {
                prompt_tokens: Some(10),
                completion_tokens: Some(2),
                total_tokens: Some(12),
                ..Default::default()
            }),
            tool_calls: vec![knowledge::models::ChatToolCall {
                id: format!("s1-runtime-{step}"),
                name: name.into(),
                arguments: args.to_string(),
            }],
        };
        host.reserve(&request).await.unwrap();
        physical_calls += 1;
        assert!(matches!(
            journal
                .load()
                .await
                .unwrap()
                .unwrap()
                .outline_run
                .discover_workers[&target.id]
                .pending,
            Some(Pending::Sending { .. })
        ));
        host.received(&request, &response).await.unwrap();
        assert!(matches!(
            journal
                .load()
                .await
                .unwrap()
                .unwrap()
                .outline_run
                .discover_workers[&target.id]
                .pending,
            Some(Pending::Received { .. })
        ));
        assert_eq!(
            json!(
                host.state
                    .outline_run
                    .reading_packs
                    .as_ref()
                    .unwrap()
                    .canonical_pack(&target.id)
                    .unwrap()
                    .condition_support_options
            ),
            before_options,
            "a received response is not yet source credit"
        );
        let submitted_arguments = response.tool_calls[0].arguments.clone();
        host.commit(&request, response).await.unwrap();
        let worker = &host.state.outline_run.discover_workers[&target.id];
        let audit_observations = worker
            .history
            .iter()
            .filter(|message| message["role"] == "assistant")
            .map(|message| message["content"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            audit_observations, emitted_observations,
            "request projection must preserve every original audit observation in order"
        );
        let result: Value =
            serde_json::from_str(worker.history.last().unwrap()["content"].as_str().unwrap())
                .unwrap();
        if result["ok"] == true {
            assert_eq!(result["result"]["status"], "committed");
            finished = true;
        } else {
            rejected_arguments.push(submitted_arguments);
            assert!(
                result
                    .to_string()
                    .contains("Required structural support pages remain"),
                "step {step}: {result}"
            );
            assert!(
                matches!(&worker.pending,Some(Pending::Failed {code,..}) if code=="BUSINESS_REJECTED")
            );
        }
        let saved = journal.load().await.unwrap().unwrap();
        assert_eq!(json!(saved), json!(host.state));
        state = saved;
        if finished || capacity_stopped {
            break;
        }
    }
    assert!(
        !rejected_arguments.is_empty(),
        "lifecycle fixture must retain at least one failed candidate"
    );
    // Wire projection never removes durable rejected arguments; state here has
    // been reloaded after every commit in both success and capacity paths.
    let worker = &state.outline_run.discover_workers[&target.id];
    for original in &rejected_arguments {
        assert!(
            worker
                .history
                .iter()
                .any(|m| m["tool_calls"].as_array().is_some_and(|calls| calls
                    .iter()
                    .any(|c| c["function"]["arguments"] == *original))),
            "complete failed candidate must remain in canonical audit"
        );
    }
    if narrow_window && unique_observations {
        assert!(
            capacity_stopped && !finished,
            "unique observations must produce an explicit bounded host stop"
        );
        let work = state.outline_run.reading_packs.as_ref().unwrap();
        assert_ne!(work.status(&target.id), Some(PackStatus::Committed));
        let delivered = work
            .canonical_pack(&target.id)
            .unwrap()
            .condition_support_options
            .iter()
            .flat_map(|option| option.evidence.iter().cloned())
            .collect::<Vec<_>>();
        assert!(
            structural_refs(&input, &target).iter().any(|reference| {
                !crate::outline::evidence::covered_by_union(reference, &delivered)
            }),
            "capacity stop cannot grant unconsumed source coverage"
        );
        assert!(work.requirement_records().is_empty());
        assert!(!work.complete());
        assert!(state.outline_run.tool_draft.check_reads.evidence.is_empty());
        let audit = json!(state.outline_run.discover_workers[&target.id]);
        let accounting = json!(state.journal.accounting);
        let mut restored = journal.load().await.unwrap().unwrap();
        let mut host = PackHost {
            input: &input,
            config: &config,
            state: &mut restored,
            journal: &journal,
            cancel: &cancel,
        };
        host.replay_received().await.unwrap();
        assert!(
            host.next(&BTreeSet::new()).await.unwrap().is_none(),
            "capacity-stopped worker must not be selected after reload"
        );
        assert_eq!(
            json!(host.state.journal.accounting),
            accounting,
            "reload must not reserve another request"
        );
        assert_eq!(
            json!(host.state.outline_run.discover_workers[&target.id]),
            audit
        );
        assert_eq!(host.state.journal.accounting.physical_calls, physical_calls);
        assert!(!host.state.done);
        return;
    }
    assert!(finished, "bounded lifecycle must commit");
    if physical_calls > 2 {
        assert!(
            saw_repeated_retired,
            "third request must retire duplicate projected observations while preserving audit"
        );
    }
    assert!(
        continuation_calls > 1,
        "functional fixture must deliver multiple genuine source pages; >6-page scheduling is covered by automatic_nine_pages_then_cross_claim_redelivery"
    );
    assert_eq!(
        delivered_intro, intro_text,
        "every original byte must be returned exactly once"
    );
    assert_eq!(state.journal.accounting.physical_calls, physical_calls);
    assert!(
        state.outline_run.discover_workers[&target.id]
            .related_read_frames
            .is_empty()
    );
    assert!(matches!(
        state.outline_run.discover_workers[&target.id].pending,
        Some(Pending::Committed { .. })
    ));
    assert!(state.outline_run.tool_draft.check_reads.evidence.is_empty());
    crate::outline::agent::synchronize_discovery(&input, &mut state).unwrap();
    let scope = state.outline_run.tool_draft.source_scopes[&target.id].clone();
    let version = crate::outline::source_review::version(
        &input,
        &state.outline_run.tool_draft,
        &target.id,
        &scope,
    )
    .unwrap();
    let refs = scope.evidence.iter().map(|reference| json!({"review_evidence_key":crate::outline::agent::review_evidence_key(&state, reference).unwrap()})).collect::<Vec<_>>();
    let args = json!({"requirement_ids":[],"pack_ids":[target.id],"inspected_evidence":refs,"issues":[],"template_reviews":[],"source_dispositions":[{"pack_id":target.id,"version":version,"evidence":refs,"verdict":"no_response_obligation","requirement_ids":[],"reason":"Synthetic source disposition checks the host protocol, not semantic extraction."}]});
    let before = json!(state);
    assert!(
        crate::outline::agent::apply_for_duty(
            &input,
            &mut state,
            "submit_review",
            &args,
            Duty::Check
        )
        .is_err()
    );
    assert_eq!(
        json!(state),
        before,
        "Discover reads do not become Check receipts"
    );
    // Explicit Check receipt seam, as in the domain acceptance suite. This is
    // not a claim that a real provider performed the independent semantic review.
    state.outline_run.tool_draft.check_reads.evidence = scope.evidence;
    state.outline_run.tool_draft.check_reads.structure_keys = scope.structure_keys;
    crate::outline::agent::apply_for_duty(&input, &mut state, "submit_review", &args, Duty::Check)
        .unwrap();
    assert!(
        crate::outline::source_review::complete(&input, &state.outline_run.tool_draft, &target.id)
            .unwrap()
    );
    journal.save(&state, &json!({})).await.unwrap();
    let restored = journal.load().await.unwrap().unwrap();
    assert!(
        crate::outline::source_review::complete(
            &input,
            &restored.outline_run.tool_draft,
            &target.id
        )
        .unwrap()
    );
    assert!(!restored.done);
}

#[tokio::test]
async fn s1_two_contexts_auto_delivery_confirms_only_consumed_pages_and_reloads() {
    // Native synthetic document manifests retain real parser unit identities.
    let mut input = super::super::token_transport_validation::synthetic_input(1, 3);
    let source_ids = input
        .source_units
        .iter()
        .map(|s| s.source_unit_revision_id.clone())
        .collect::<Vec<_>>();
    for n in 0..2 {
        let owner = &input.source_units[2];
        let support = &input.source_units[n];
        input.document_relations.push(serde_json::from_value(json!({"id":format!("required-{n}"),"from":{"document_id":owner.document_id,"unit_id":owner.locator["unit_id"]},"to":{"document_id":support.document_id,"unit_id":support.locator["unit_id"]},"kind":"explicit_reference","status":"confirmed","required":true,"basis":owner.text.trim(),"locator":owner.locator})).unwrap());
    }
    input.validate_document_relations().unwrap();
    let work = DiscoverWork::plan_with_budget(&input, &|sessions| {
        Ok(sessions
            .iter()
            .all(|s| s["pack"]["atoms"].as_array().unwrap().len() == 1))
    });
    assert!(
        work.planning_error().is_none(),
        "{:?}",
        work.planning_error()
    );
    let target = work
        .pack_ids()
        .into_iter()
        .find(|id| {
            work.pack_evidence(&input, id)
                .unwrap()
                .iter()
                .any(|r| matches!(r,EvidenceRef::Text{unit_id,..} if unit_id==&source_ids[2]))
        })
        .unwrap();
    let mut state = super::super::retirement::checkpoint(&input);
    state.turn = 0;
    state.outline_run.reading_packs = Some(work);
    let config = crate::analysis::tests::config();
    state.config_sha256 = digest(&config).unwrap();
    let journal = S1Journal(std::sync::Mutex::new(state.clone()));
    let cancel = CancellationToken::new();
    let mut target_turns = 0;
    let mut calls = 0;
    let mut delivered = Vec::new();
    for step in 0..8 {
        let mut host = PackHost {
            input: &input,
            config: &config,
            state: &mut state,
            journal: &journal,
            cancel: &cancel,
        };
        let Some(request) = host.next(&BTreeSet::new()).await.unwrap() else {
            break;
        };
        let wire: Value = serde_json::from_slice(&request.body).unwrap();
        let payloads = wire["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|m| {
                let text = match &m["content"] {
                    Value::String(s) => s.clone(),
                    Value::Array(a) => a
                        .iter()
                        .filter_map(|v| v["text"].as_str())
                        .collect::<String>(),
                    _ => return None,
                };
                serde_json::from_str::<Value>(&text).ok()
            })
            .collect::<Vec<_>>();
        let session = &payloads
            .iter()
            .find(|p| p["reading_packs"].is_array())
            .unwrap()["reading_packs"][0];
        let pack = &session["pack"];
        let scope = payloads
            .iter()
            .find_map(|p| p["wire_scope"].as_str())
            .unwrap();
        let before = host
            .state
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .canonical_pack(&request.scope.pack)
            .unwrap()
            .condition_support_options
            .clone();
        let is_target = request.scope.pack == target;
        if is_target {
            assert_eq!(
                before.len(),
                target_turns,
                "pending automatic page cannot grant credit"
            );
            let page = &payloads
                .iter()
                .find(|p| p["automatic_support_page"].is_object())
                .unwrap()["automatic_support_page"]["read"];
            let refs = page["excerpts"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| {
                    match s1_wire_evidence(
                        &host.state.outline_run.discover_workers[&target],
                        &e["evidence"],
                    ) {
                        EvidenceRef::Text { unit_id, .. } => unit_id,
                        _ => panic!("synthetic support must be text"),
                    }
                })
                .collect::<Vec<_>>();
            assert_eq!(refs, vec![source_ids[target_turns].clone()]);
            delivered.extend(refs);
        }
        let args = json!({"wire_scope":scope,"pack_id":pack["id"],"call_id":session["submission_operation_id"],"claim_token":pack["claim_token"],"pack_revision":pack["pack_revision"],"repair":session["status"]=="failed","requirements":[],"issues":[],"no_requirement_reason":"Synthetic protocol response, not semantic extraction.","inspected_atom_ids":pack["atoms"].as_array().unwrap().iter().filter(|a|a["context_only"]!=true).map(|a|json!({"atom_key":a["atom_key"]})).collect::<Vec<_>>()});
        let response = ChatTurn {
            content: "Synthetic response.".into(),
            finish_reason: "tool_calls".into(),
            usage: Some(knowledge::models::ChatUsage {
                prompt_tokens: Some(10),
                completion_tokens: Some(2),
                total_tokens: Some(12),
                ..Default::default()
            }),
            tool_calls: vec![knowledge::models::ChatToolCall {
                id: format!("s1-auto-{step}"),
                name: "submit_pack".into(),
                arguments: args.to_string(),
            }],
        };
        host.reserve(&request).await.unwrap();
        host.received(&request, &response).await.unwrap();
        assert_eq!(
            host.state
                .outline_run
                .reading_packs
                .as_ref()
                .unwrap()
                .canonical_pack(&request.scope.pack)
                .unwrap()
                .condition_support_options,
            before,
            "Received is not confirmation"
        );
        host.commit(&request, response).await.unwrap();
        calls += 1;
        let worker = &host.state.outline_run.discover_workers[&request.scope.pack];
        let result: Value =
            serde_json::from_str(worker.history.last().unwrap()["content"].as_str().unwrap())
                .unwrap();
        if is_target && target_turns == 0 {
            assert_eq!(result["ok"], false);
            assert!(
                result
                    .to_string()
                    .contains("Required structural support pages remain")
            );
            assert!(
                matches!(&worker.pending,Some(Pending::Failed{code,..}) if code=="BUSINESS_REJECTED")
            );
        } else {
            assert_eq!(result["ok"], true, "{result}");
        }
        if is_target {
            target_turns += 1;
        }
        let saved = journal.load().await.unwrap().unwrap();
        assert_eq!(json!(saved), json!(host.state));
        state = saved;
    }
    assert_eq!(target_turns, 2);
    assert_eq!(delivered, source_ids[..2].to_vec());
    let work = state.outline_run.reading_packs.as_ref().unwrap();
    assert!(work.complete(), "no other pack was seeded successful");
    assert_eq!(work.status(&target), Some(PackStatus::Committed));
    assert!(
        work.pack_evidence(&input, &target)
            .unwrap()
            .iter()
            .all(|r| matches!(r,EvidenceRef::Text{unit_id,..} if unit_id==&source_ids[2]))
    );
    assert!(state.outline_run.tool_draft.check_reads.evidence.is_empty());
    assert_eq!(state.journal.accounting.physical_calls, calls);
    let before = json!(state.journal.accounting);
    let mut host = PackHost {
        input: &input,
        config: &config,
        state: &mut state,
        journal: &journal,
        cancel: &cancel,
    };
    host.replay_received().await.unwrap();
    assert!(host.next(&BTreeSet::new()).await.unwrap().is_none());
    assert_eq!(json!(host.state.journal.accounting), before);
}
