// Synthetic occurrence identity, not an oracle for image meaning or placement.
fn s5_occurrences() -> (FrozenInput, Config, Checkpoint) {
    use crate::outline::frozen::{PARSER_CONTRACT_VERSION, build_frozen_input};
    let (mut build, mut receipts) = crate::outline::frozen::tests::mixed_native_build();
    let original = build
        .documents
        .iter()
        .find(|d| !d.parsed.images.is_empty())
        .unwrap()
        .clone();
    let template = original
        .parsed
        .structured_source_units
        .iter()
        .find(|u| u.kind == docparser::StructuredSourceUnitKind::ImageRegion)
        .unwrap()
        .clone();
    let original_contract: Value =
        serde_json::from_str(&original.parsed.metadata["source_contract"]).unwrap();
    let contract_unit = original_contract["units"]
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["unit_id"] == template.key)
        .unwrap()
        .clone();
    let receipt = receipts
        .iter()
        .find(|r| r.document_id == original.document_id && r.unit_id == template.key)
        .unwrap()
        .clone();
    let original_ref = match &template.locator {
        docparser::StructuredSourceLocator::Image { original_ref, .. } => original_ref,
        _ => unreachable!(),
    };
    let pixels = original
        .parsed
        .images
        .iter()
        .find(|i| &i.original_ref == original_ref)
        .unwrap()
        .clone();
    for (document_id, count) in [("pricing", 2), ("response", 1)] {
        let document = build
            .documents
            .iter_mut()
            .find(|d| d.document_id == document_id)
            .unwrap();
        let mut contract: Value =
            serde_json::from_str(&document.parsed.metadata["source_contract"]).unwrap();
        for occurrence in 0..count {
            let key = format!("s5-{document_id}-image-{occurrence}");
            let mut unit = template.clone();
            unit.key = key.clone();
            unit.ordinal = document
                .parsed
                .structured_source_units
                .iter()
                .map(|u| u.ordinal)
                .max()
                .unwrap_or(0)
                + 1;
            let mut image = pixels.clone();
            image.original_ref = format!("s5/{document_id}/{occurrence}.png");
            if let docparser::StructuredSourceLocator::Image {
                original_ref,
                page_ordinal,
                compound_parent,
                left,
                top,
                right,
                bottom,
                ..
            } = &mut unit.locator
            {
                *original_ref = image.original_ref.clone();
                *page_ordinal = None;
                *left = None;
                *top = None;
                *right = None;
                *bottom = None;
                *compound_parent = (document_id == "response").then_some(
                    docparser::CompoundImageParent::Paragraph {
                        section_ordinal: 0,
                        paragraph_ordinal: 0,
                    },
                );
            }
            let mut identity = contract_unit.clone();
            identity["unit_id"] = json!(key);
            identity["ordinal"] = json!(unit.ordinal);
            identity["section_id"] = Value::Null;
            identity["parent_section_id"] = Value::Null;
            identity["physical_locator"] = json!(unit.locator);
            identity["physical_path"] = json!(if document_id == "pricing" {
                format!("sheet:0/drawing:{occurrence}")
            } else {
                "body/p:0/drawing:0".into()
            });
            identity["rendered_spans"] = json!([]);
            contract["units"].as_array_mut().unwrap().push(identity);
            let mut completed = receipt.clone();
            completed.document_id = document_id.into();
            completed.unit_id = key;
            completed.document_revision = contract["document_revision"].as_str().unwrap().into();
            completed.parser_version = contract["parser_version"].as_str().unwrap().into();
            receipts.push(completed);
            document.parsed.structured_source_units.push(unit);
            document.parsed.images.push(image);
        }
        document
            .parsed
            .metadata
            .insert("source_contract".into(), contract.to_string());
    }
    let frozen = build_frozen_input(build.clone(), receipts, PARSER_CONTRACT_VERSION).unwrap();
    let config = crate::analysis::tests::config();
    let mut state = super::super::retirement::checkpoint(&frozen);
    state.config_sha256 = digest(&config).unwrap();
    state.turn = 0;
    state.outline_run.reading_packs = Some(
        crate::outline::discover::DiscoverWork::plan_with_budget(&frozen, &|sessions| {
            Ok(sessions
                .iter()
                .all(|session| session["pack"]["document_ids"].as_array().unwrap().len() == 1))
        }),
    );
    for source in frozen
        .source_units
        .iter()
        .filter(|s| s.locator["kind"] == "image_region")
    {
        let image = build
            .documents
            .iter()
            .find(|d| d.document_id == source.document_id)
            .unwrap()
            .parsed
            .images
            .iter()
            .find(|i| i.original_ref == source.locator["original_ref"])
            .unwrap();
        let view = crate::outline::visual::render_frozen_image_view(
            &source.source_unit_revision_id,
            source.locator["image_ref"]
                .as_str()
                .unwrap()
                .strip_prefix("objects/")
                .unwrap(),
            0,
            &image.data,
            config.limits.max_source_view_edge,
            config.limits.max_source_view_bytes,
        )
        .unwrap();
        state.source_views.insert(view.id().unwrap(), view);
    }
    (frozen, config, state)
}

#[tokio::test]
async fn s5_same_sheet_occurrences_and_compound_parent_survive_actual_wire_without_invented_anchor()
{
    let (input, config, state) = s5_occurrences();
    let ids = input
        .source_units
        .iter()
        .filter(|s| {
            s.locator["unit_id"]
                .as_str()
                .is_some_and(|id| id.starts_with("s5-"))
        })
        .collect::<Vec<_>>();
    assert_eq!(ids.len(), 3);
    for source in &ids {
        assert!(source.locator["left"].is_null());
        assert!(source.locator["row_ordinal"].is_null());
        if source.document_id == "pricing" {
            assert!(
                source.locator["physical_path"]
                    .as_str()
                    .unwrap()
                    .starts_with("sheet:0/")
            );
        } else {
            assert_eq!(
                source.locator["compound_parent"]["parent_kind"],
                "paragraph"
            );
        }
    }
    let journal = Durable {
        state: Mutex::new(state),
        stop_barrier: false,
        crash_received: AtomicBool::new(false),
        call_limit: 99,
    };
    let mut state = journal.load().await.unwrap().unwrap();
    let cancel = CancellationToken::new();
    let mut host = PackHost {
        input: &input,
        config: &config,
        state: &mut state,
        journal: &journal,
        cancel: &cancel,
    };
    // Claim every pack explicitly: one next() only represents one document scope.
    let packs = host
        .state
        .outline_run
        .reading_packs
        .as_mut()
        .unwrap()
        .claim(usize::MAX);
    let mut seen = BTreeSet::new();
    for pack in packs {
        host.state
            .outline_run
            .discover_workers
            .entry(pack.id.clone())
            .or_default();
        let request = host.prepare_inner(&pack.id, false).await.unwrap();
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        for source in &ids {
            if !pack
                .atoms
                .iter()
                .any(|atom| atom.source_unit_revision_id == source.source_unit_revision_id)
            {
                continue;
            }
            let view = host
                .state
                .source_views
                .values()
                .find(|v| v.identity.source_id == source.source_unit_revision_id)
                .unwrap();
            assert!(
                body["messages"]
                    .as_array()
                    .unwrap()
                    .contains(&view.message()),
                "owned occurrence must carry its own identity-bearing pixels"
            );
            seen.insert(source.source_unit_revision_id.clone());
        }
        let mut response = Delayed {
            delay_ms: 0,
            ..Default::default()
        }
        .turn(&config, &request.body)
        .await
        .unwrap();
        // No invented image interpretation: a malformed action cannot create
        // a requirement, while durable transport still confirms real pixels.
        response.tool_calls[0].arguments = "{bad-json".into();
        host.reserve(&request).await.unwrap();
        host.received(&request, &response).await.unwrap();
        assert!(matches!(
            host.state.outline_run.discover_workers[&pack.id].pending,
            Some(Pending::Received { .. })
        ));
        *host.state = journal.load().await.unwrap().unwrap();
        host.replay_received().await.unwrap();
        let once = json!(host.state.outline_run.reading_packs);
        let accounting = json!(host.state.journal.accounting);
        host.replay_received().await.unwrap();
        assert_eq!(json!(host.state.outline_run.reading_packs), once);
        assert_eq!(json!(host.state.journal.accounting), accounting);
        for source in &ids {
            if !pack
                .atoms
                .iter()
                .any(|atom| atom.source_unit_revision_id == source.source_unit_revision_id)
            {
                continue;
            }
            let work = host.state.outline_run.reading_packs.as_ref().unwrap();
            assert!(
                json!(work)["packs"][&pack.id]["visual_receipts"]
                    .to_string()
                    .contains(&source.source_unit_revision_id)
            );
        }
    }
    assert_eq!(
        seen.len(),
        ids.len(),
        "all document packs must be exercised"
    );
    assert_eq!(
        host.state
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .requirement_count(),
        0
    );
    assert!(
        host.state
            .outline_run
            .tool_draft
            .check_reads
            .evidence
            .is_empty()
    );
    assert!(!host.state.done);
}

#[test]
fn s5_same_pixels_fresh_check_is_per_occurrence_and_position_stays_unresolved() {
    let (input, _, mut state) = s5_occurrences();
    let sources = input
        .source_units
        .iter()
        .filter(|s| {
            s.locator["unit_id"]
                .as_str()
                .is_some_and(|id| id.starts_with("s5-"))
        })
        .collect::<Vec<_>>();
    let first = &sources[0].source_unit_revision_id;
    let second = &sources[1].source_unit_revision_id;
    let view = |id: &str| {
        state
            .source_views
            .values()
            .find(|v| v.identity.source_id == id)
            .unwrap()
    };
    assert_eq!(view(first).jpeg_base64, view(second).jpeg_base64);
    assert_ne!(view(first).message(), view(second).message());
    // Explicit fresh Check receipt seam; this does not assert semantic review.
    crate::outline::agent::note_visual_delivery(
        &input,
        &mut state,
        first,
        crate::outline::agent::Duty::Discover,
    )
    .unwrap();
    assert!(state.outline_run.tool_draft.check_reads.evidence.is_empty());
    crate::outline::agent::note_visual_delivery(
        &input,
        &mut state,
        first,
        crate::outline::agent::Duty::Check,
    )
    .unwrap();
    assert_eq!(state.outline_run.tool_draft.check_reads.evidence.len(), 1);
    assert!(!state.outline_run.tool_draft.check_reads.evidence.iter().any(|r| matches!(r,crate::outline::evidence::EvidenceRef::ImageRegion { image_id,.. } if image_id==second)));
    let restored: Checkpoint = serde_json::from_value(json!(state)).unwrap();
    assert_eq!(
        restored.outline_run.tool_draft.check_reads.evidence.len(),
        1
    );
    assert!(!restored.done);
    assert!(
        crate::outline::tools::template_ready(&input, &restored.outline_run.tool_draft).is_err()
    );
}

// Reuse the existing deterministic mixed-document oracle only to reach the
// production serial Check transport. Never publish its synthetic judgments.
struct S5CheckTransportOracle {
    inner: MixedCycleModel,
    cancel: CancellationToken,
    observed_finish: AtomicBool,
}
#[async_trait]
impl Model for S5CheckTransportOracle {
    async fn turn(&self, config: &Config, body: &[u8]) -> Result<ChatTurn, AgentError> {
        // The larger S5 fixture may have its host preview reduced to zero.
        // Reconstruct only from metadata actually present in this request scope,
        // never retain opaque IDs across requests or consult the checkpoint.
        let mut wire: Value = serde_json::from_slice(body).unwrap();
        let payloads = wire["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|m| m["content"].as_str())
            .filter_map(|s| serde_json::from_str::<Value>(s).ok())
            .collect::<Vec<_>>();
        if let Some(host) = payloads.iter().rev().find(|p| p["wire_scope"].is_string())
            && host["requirements"]["omitted"].as_u64().unwrap_or(0) > 0
        {
            let preview = host["requirements"]["items"].as_array().unwrap();
            let expected =
                preview.len() + host["requirements"]["omitted"].as_u64().unwrap() as usize;
            let mut rows = std::collections::BTreeMap::new();
            for row in preview {
                rows.insert(
                    row["requirement_id"].as_str().unwrap().to_owned(),
                    row.clone(),
                );
            }
            let mut cursor = None;
            for payload in &payloads {
                let result = &payload["result"];
                if let Some(items) = result["items"].as_array()
                    && items
                        .iter()
                        .any(|r| r["requirement_id"].is_string() && r["requirement"].is_object())
                {
                    for row in items
                        .iter()
                        .filter(|r| r["requirement_id"].is_string() && r["requirement"].is_object())
                    {
                        rows.insert(
                            row["requirement_id"].as_str().unwrap().to_owned(),
                            row.clone(),
                        );
                    }
                    cursor = result["next_cursor"].as_str().map(str::to_owned);
                }
            }
            if rows.len() < expected {
                let mut args = if let Some(cursor) = cursor {
                    json!({"cursor":cursor})
                } else {
                    json!({"mode":"requirements"})
                };
                args["wire_scope"] = host["wire_scope"].clone();
                return Ok(ChatTurn {
                    content: String::new(),
                    finish_reason: "tool_calls".into(),
                    usage: None,
                    tool_calls: vec![knowledge::models::ChatToolCall {
                        id: format!("s5-metadata-{}", host["progress"]["turn"]),
                        name: "read_requirements".into(),
                        arguments: args.to_string(),
                    }],
                });
            }
            assert_eq!(
                rows.len(),
                expected,
                "S5 metadata selection must cover exactly the current preview universe"
            );
            for message in wire["messages"].as_array_mut().unwrap().iter_mut().rev() {
                let Some(text) = message["content"].as_str() else {
                    continue;
                };
                let Ok(mut value) = serde_json::from_str::<Value>(text) else {
                    continue;
                };
                if value["wire_scope"] == host["wire_scope"] && value["requirements"].is_object() {
                    value["requirements"]["items"] = json!(rows.into_values().collect::<Vec<_>>());
                    value["requirements"]["omitted"] = json!(0);
                    message["content"] = json!(value.to_string());
                    break;
                }
            }
        }
        let response = self
            .inner
            .turn(config, &serde_json::to_vec(&wire).unwrap())
            .await?;
        if response
            .tool_calls
            .iter()
            .any(|call| call.name == "finish_outline")
        {
            self.observed_finish.store(true, Ordering::SeqCst);
            self.cancel.cancel();
            return Err(error(
                "S5_TRANSPORT_OBSERVED",
                "synthetic transport observation stops before publication",
            ));
        }
        Ok(response)
    }
}

#[tokio::test]
async fn s5_fresh_check_uses_serial_image_transport_for_each_occurrence_before_publication() {
    super::super::profile::reset();
    let profile_started = std::time::Instant::now();
    let (input, config, state) = s5_occurrences();
    let expected_image_urls = state
        .source_views
        .values()
        .map(|view| format!("data:image/jpeg;base64,{}", view.jpeg_base64))
        .collect();
    let journal = CycleJournal {
        state: Mutex::new(state),
        stop_check: AtomicBool::new(false),
        stop_repair: AtomicBool::new(false),
    };
    let cancel = CancellationToken::new();
    let model = S5CheckTransportOracle {
        cancel: cancel.clone(),
        observed_finish: AtomicBool::new(false),
        inner: MixedCycleModel {
            total_calls: AtomicUsize::new(0),
            repeat_actions: Mutex::new(std::collections::BTreeMap::new()),
            discover: MixedNativeDiscoveryModel {
                fallback: Delayed::default(),
                image_requests: AtomicUsize::new(0),
                word_grid_requests: AtomicUsize::new(0),
                expected_image_urls,
            },
            organize_step: AtomicUsize::new(0),
            compared: Mutex::new(BTreeSet::new()),
        },
    };
    let result = super::super::run(&input, &config, &journal, &model, &cancel)
        .await
        .unwrap_err();
    eprintln!(
        "S5_PROFILE {}",
        json!({"wall_ms":profile_started.elapsed().as_millis(),"model_calls":model.inner.total_calls.load(Ordering::SeqCst),"inclusive_count_micros":super::super::profile::finish(),"counters":super::super::profile::finish_counters()})
    );
    assert_eq!(result.code, "INTERNAL", "{}", result.message);
    assert_eq!(result.message, "Agent run cancelled");
    assert!(
        model.observed_finish.load(Ordering::SeqCst),
        "only the observed finish boundary may cancel this fixture"
    );
    let saved = journal.load().await.unwrap().unwrap();
    for source in input.source_units.iter().filter(|s| {
        s.locator["unit_id"]
            .as_str()
            .is_some_and(|id| id.starts_with("s5-"))
    }) {
        assert!(saved.outline_run.tool_draft.check_reads.evidence.iter().any(|reference|
            matches!(reference,crate::outline::evidence::EvidenceRef::ImageRegion{image_id,..} if image_id==&source.source_unit_revision_id)),
            "fresh Check transport must independently confirm each source occurrence");
        assert!(
            source.locator["left"].is_null(),
            "transport cannot invent placement"
        );
    }
    assert!(
        !saved.done && !saved.outline_run.tool_draft.finished,
        "transport fixtures do not approve meaning or publish an artifact"
    );
}
