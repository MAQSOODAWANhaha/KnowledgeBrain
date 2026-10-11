// Deterministic fixture oracle. No external model or OCR accuracy claim.
struct MixedCycleModel {
    discover: MixedNativeDiscoveryModel,
    organize_step: AtomicUsize,
    compared: Mutex<BTreeSet<String>>,
}
#[async_trait]
impl Model for MixedCycleModel {
    async fn turn(&self, config: &Config, bytes: &[u8]) -> Result<ChatTurn, AgentError> {
        let wire: Value = serde_json::from_slice(bytes).unwrap();
        if wire["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["function"]["name"] == "submit_pack")
        {
            return self.discover.turn(config, bytes).await;
        }
        let payloads = wire["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|m| m["content"].as_str())
            .filter_map(|s| serde_json::from_str::<Value>(s).ok())
            .collect::<Vec<_>>();
        for payload in &payloads {
            assert!(
                payload["ok"] != false,
                "mixed fixture wire error: {payload}"
            );
        }
        let host = payloads
            .iter()
            .rev()
            .find(|p| p["wire_scope"].is_string())
            .unwrap();
        let requirements = host["requirements"]["items"].as_array().unwrap();
        assert_eq!(
            host["requirements"]["omitted"], 0,
            "fixture must use delivered requirement identities"
        );
        let results = payloads
            .iter()
            .filter_map(|p| p.get("result"))
            .collect::<Vec<_>>();
        let ids = requirements
            .iter()
            .map(|r| r["requirement_id"].clone())
            .collect::<Vec<_>>();
        let step = self.organize_step.load(Ordering::SeqCst);
        let (name, mut args) = if step < 7 {
            self.organize_step.fetch_add(1, Ordering::SeqCst);
            match step {
                0 => (
                    "put_chapters",
                    json!({"mode":"replace","chapters":[
                    {"id":"mixed-root","parent_id":null,"order":0,"title":"Fixture tender response","purpose":"group","requirement_ids":[]},
                    {"id":"mixed-group","parent_id":"mixed-root","order":0,"title":"Prescribed response materials","purpose":"group","requirement_ids":[]},
                    {"id":"mixed-response","parent_id":"mixed-group","order":0,"title":"Signed response","purpose":"response","requirement_ids":ids}]}),
                ),
                1 => {
                    let word = requirements
                        .iter()
                        .find(|r| {
                            r["requirement"]["description"]
                                .as_str()
                                .is_some_and(|d| d.contains("Keep this fixed wording."))
                        })
                        .unwrap();
                    (
                        "read_evidence",
                        json!({"refs":word["requirement"]["evidence"]}),
                    )
                }
                2 => {
                    let mut slots=ids.iter().enumerate().map(|(i,_)|json!({"slot_id":format!("mixed-field-{i}"),"chapter_id":"mixed-response","content":{"type":"editable_blank"},"blank_kind":"bidder_blank","match_query":format!("Fixture response field {i}")})).collect::<Vec<_>>();
                    let excerpt = results
                        .iter()
                        .flat_map(|r| r["excerpts"].as_array().into_iter().flatten())
                        .find(|e| {
                            e["quote"]
                                .as_str()
                                .is_some_and(|q| q.contains("Keep this fixed wording."))
                        })
                        .unwrap();
                    slots.push(json!({"slot_id":"mixed-fixed","chapter_id":"mixed-response","content":{"type":"source_copy","purpose":"mandated_response","refs":[excerpt["evidence"]]}}));
                    ("put_slots", json!({"mode":"replace","slots":slots}))
                }
                3 => (
                    "put_fulfillments",
                    json!({"fulfillments":requirements.iter().enumerate().map(|(i,r)|{
                    let mut targets=vec![json!({"type":"text_slot","slot_id":format!("mixed-field-{i}")})];
                    if r["requirement"]["description"].as_str().is_some_and(|d|d.contains("Keep this fixed wording.")){targets.push(json!({"type":"text_slot","slot_id":"mixed-fixed"}));}
                    json!({"requirement_id":r["requirement_id"],"primary_response_chapter_id":"mixed-response","target_refs":targets})
                }).collect::<Vec<_>>()}),
                ),
                4 => ("read_outline", json!({"mode":"forms"})),
                5 => {
                    let forms = results
                        .iter()
                        .flat_map(|r| r["items"].as_array().into_iter().flatten())
                        .filter(|r| r["form_id"].is_string());
                    (
                        "bind_forms",
                        json!({"mode":"replace","bindings":forms.map(|f|json!({"form_id":f["form_id"],"chapter_id":"mixed-response"})).collect::<Vec<_>>()}),
                    )
                }
                6 => ("read_requirements", json!({"mode":"packs"})),
                _ => unreachable!(),
            }
        } else {
            let rows = results
                .iter()
                .flat_map(|r| r["items"].as_array().into_iter().flatten())
                .filter(|r| r["pack_id"].is_string())
                .collect::<Vec<_>>();
            let mut reviewed = results
                .iter()
                .flat_map(|r| r["excerpts"].as_array().into_iter().flatten())
                .filter(|e| e["review_evidence_key"].is_string())
                .collect::<Vec<_>>();
            reviewed.extend(
                results
                    .iter()
                    .copied()
                    .filter(|r| r["review_evidence_key"].is_string()),
            );
            let work = &host["check_work"];
            if work["source_scope"].is_object() {
                let source = &work["source_scope"];
                let own = rows
                    .iter()
                    .copied()
                    .filter(|r| r["pack_id"] == source["pack_id"] && r["evidence"].is_object())
                    .collect::<Vec<_>>();
                assert!(!own.is_empty(), "source directory omitted fixture pack");
                if let Some(row) = own
                    .iter()
                    .find(|row| !reviewed.iter().any(|e| e["evidence"] == row["evidence"]))
                {
                    let instruction = &row["read"];
                    (
                        instruction["tool"].as_str().unwrap(),
                        instruction["args"].clone(),
                    )
                } else {
                    let linked = requirements
                        .iter()
                        .filter(|r| {
                            r["requirement"]["evidence"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .any(|e| own.iter().any(|row| row["evidence"] == *e))
                        })
                        .map(|r| r["requirement_id"].clone())
                        .collect::<Vec<_>>();
                    let keys = reviewed
                        .iter()
                        .filter(|e| own.iter().any(|row| row["evidence"] == e["evidence"]))
                        .map(|e| json!({"review_evidence_key":e["review_evidence_key"]}))
                        .collect::<Vec<_>>();
                    (
                        "submit_review",
                        json!({"requirement_ids":[],"pack_ids":[source["pack_id"]],"inspected_evidence":keys,"issues":[],"template_reviews":[],
                        "source_dispositions":[{"pack_id":source["pack_id"],"version":source["source_review_version"],"evidence":keys,"verdict":if linked.is_empty(){"no_response_obligation"}else{"contains_obligations"},"requirement_ids":linked,"reason":"Bounded synthetic source oracle; not model semantic acceptance"}]}),
                    )
                }
            } else if let Some(id) = work["requirement_id"].as_str() {
                let compared = self.compared.lock().unwrap().contains(id);
                if !compared {
                    if let Some(unit) = results.iter().rev().find(|r| {
                        r["requirement_id"] == id && r["original_requirement"].is_string()
                    }) {
                        let handles = unit["items"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|i| i["quote_handle"].clone())
                            .collect::<Vec<_>>();
                        self.compared.lock().unwrap().insert(id.into());
                        (
                            "submit_claim_comparison",
                            json!({"requirement_id":id,"version":unit["version"],"declared_claims":[{"claim_handle":"fixture-claim","obligation":unit["original_requirement"],"applicability":"synthetic native fixture","original_fragments":[unit["original_requirement"]],"primary_handles":handles,"support_handles":[]}],"observations":handles.iter().map(|h|json!({"quote_handle":h,"polarity":"required","applicability":"relevant","condition":"fixture requirement"})).collect::<Vec<_>>(),"decisions":[{"claim_handle":"fixture-claim","evidence_handles":handles,"verdict":"supports","action":"retain","resulting_claim":unit["original_requirement"]}]}),
                        )
                    } else {
                        ("read_claim_evidence", json!({"requirement_id":id}))
                    }
                } else if work["next_unread_slot"].is_object() {
                    ("read_outline", work["next_unread_slot"]["args"].clone())
                } else {
                    let requirement = requirements
                        .iter()
                        .find(|r| r["requirement_id"] == id)
                        .unwrap();
                    let keys = reviewed
                        .iter()
                        .filter(|e| {
                            requirement["requirement"]["evidence"]
                                .as_array()
                                .unwrap()
                                .contains(&e["evidence"])
                        })
                        .map(|e| json!({"review_evidence_key":e["review_evidence_key"]}))
                        .collect::<Vec<_>>();
                    let fields=work["target_refs"].as_array().unwrap().iter().filter_map(|target|target["slot_id"].as_str()).map(|slot|json!({"type":"slot","slot_id":slot,"label":if slot=="mixed-fixed" { "Keep this fixed wording.".to_string() } else { format!("Fixture response field {}",slot.strip_prefix("mixed-field-").unwrap()) }})).collect::<Vec<_>>();
                    (
                        "submit_review",
                        json!({"requirement_ids":[id],"pack_ids":[],"inspected_evidence":keys,"issues":[],"source_dispositions":[],"template_reviews":[{"requirement_id":id,"version":work["template_review_version"],"claims":[{"claim_handle":"fixture-claim","verdict":"template_ready","reason":"Synthetic fixture checks actual bound response fields and original fixed text","fields":fields}]}]}),
                    )
                }
            } else {
                ("finish_outline", json!({}))
            }
        };
        args["wire_scope"] = host["wire_scope"].clone();
        Ok(ChatTurn {
            content: String::new(),
            finish_reason: "tool_calls".into(),
            usage: None,
            tool_calls: vec![knowledge::models::ChatToolCall {
                id: format!("mixed-{}", host["progress"]["turn"]),
                name: name.into(),
                arguments: args.to_string(),
            }],
        })
    }
}

#[tokio::test]
async fn mixed_native_complete_organize_and_fresh_check_with_original_pixels() {
    super::super::profile::reset();
    let profile_started = std::time::Instant::now();
    let (input, config, state, expected_image_urls) = mixed_native_runtime_fixture();
    let journal = CycleJournal {
        state: Mutex::new(state),
        stop_check: AtomicBool::new(false),
        stop_repair: AtomicBool::new(false),
    };
    let model = MixedCycleModel {
        discover: MixedNativeDiscoveryModel {
            fallback: Delayed::default(),
            image_requests: AtomicUsize::new(0),
            word_grid_requests: AtomicUsize::new(0),
            expected_image_urls,
        },
        organize_step: AtomicUsize::new(0),
        compared: Mutex::new(BTreeSet::new()),
    };
    super::super::run(&input, &config, &journal, &model, &CancellationToken::new())
        .await
        .unwrap();
    eprintln!(
        "MIXED_PROFILE {}",
        json!({"wall_ms":profile_started.elapsed().as_millis(),"inclusive_count_micros":super::super::profile::finish()})
    );
    let saved = journal.load().await.unwrap().unwrap();
    assert!(saved.done && saved.outline_run.tool_draft.finished);
    assert!(
        saved
            .outline_run
            .tool_draft
            .slots
            .iter()
            .any(|slot| slot.text.contains("Keep this fixed wording."))
    );
    let images = saved
        .outline_run
        .tool_draft
        .check_reads
        .evidence
        .iter()
        .filter(|r| matches!(r, crate::outline::evidence::EvidenceRef::ImageRegion { .. }))
        .count();
    assert_eq!(
        images, 2,
        "both native image occurrences need independent Check pixels"
    );
    // Re-reading image claim metadata cannot recreate actual Check pixel credit.
    let (image_id, comparison) = saved
        .outline_run
        .tool_draft
        .claim_comparisons
        .iter()
        .find(|(id, _)| {
            saved.outline_run.tool_draft.requirements[*id]
                .evidence
                .iter()
                .any(|r| matches!(r, crate::outline::evidence::EvidenceRef::ImageRegion { .. }))
        })
        .unwrap();
    let mut metadata_only = saved.clone();
    metadata_only.done = false;
    metadata_only.outline_run.tool_draft.finished = false;
    metadata_only
        .outline_run
        .tool_draft
        .check_reads
        .evidence
        .retain(|r| !matches!(r, crate::outline::evidence::EvidenceRef::ImageRegion { .. }));
    let args = json!({"requirement_id":image_id});
    let call = knowledge::models::ChatToolCall {
        id: "image-claim-metadata-only".into(),
        name: "read_claim_evidence".into(),
        arguments: args.to_string(),
    };
    let page = super::super::read_evidence_in_context(
        &input,
        &config,
        &mut metadata_only,
        &args,
        &[call],
        &[],
        crate::outline::agent::Duty::Check,
    )
    .await
    .unwrap();
    assert!(
        page["items"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|item| item["excerpts"].as_array().unwrap())
            .any(|e| e["requires_original_pixels"] == true
                && e["read"]["tool"] == "read_source_view")
    );
    assert!(
        !metadata_only
            .outline_run
            .tool_draft
            .check_reads
            .evidence
            .iter()
            .any(|r| matches!(r, crate::outline::evidence::EvidenceRef::ImageRegion { .. }))
    );
    assert!(
        !metadata_only
            .outline_run
            .tool_draft
            .check_reads
            .pending_evidence
            .iter()
            .any(|(_, r)| matches!(r, crate::outline::evidence::EvidenceRef::ImageRegion { .. }))
    );
    let before = metadata_only.outline_run.tool_draft.clone();
    let error = crate::outline::agent::apply_for_duty(
        &input,
        &mut metadata_only,
        "submit_claim_comparison",
        &serde_json::to_value(comparison).unwrap(),
        crate::outline::agent::Duty::Check,
    )
    .unwrap_err();
    assert!(error.contains("fresh complete source reads"), "{error}");
    assert_eq!(before, metadata_only.outline_run.tool_draft);
    let mut changed = input.clone();
    changed.document_set_id.push_str("-new-version");
    assert!(
        crate::outline::tools::template_ready(&changed, &saved.outline_run.tool_draft).is_err()
    );
}

#[test]
fn mixed_native_identical_pixels_never_credit_another_source_occurrence() {
    let (input, config, mut state, _) = mixed_native_runtime_fixture();
    let views = state.source_views.values().cloned().collect::<Vec<_>>();
    assert_eq!(views.len(), 2);
    assert_eq!(views[0].jpeg_base64, views[1].jpeg_base64);
    assert_ne!(views[0].identity.source_id, views[1].identity.source_id);
    super::super::confirm_outline_visual_delivery(
        &input,
        &config,
        &mut state,
        &json!({"messages":[views[0].message()]}),
    )
    .unwrap();
    let references = &state.outline_run.tool_draft.delivered_evidence;
    assert_eq!(references.len(), 1);
    assert!(
        matches!(&references[0],crate::outline::evidence::EvidenceRef::ImageRegion {image_id,region,..} if image_id==&views[0].identity.source_id && region=="original")
    );
    assert!(
        state.outline_run.tool_draft.check_reads.evidence.is_empty(),
        "Discover image delivery never grants Check credit"
    );
}
