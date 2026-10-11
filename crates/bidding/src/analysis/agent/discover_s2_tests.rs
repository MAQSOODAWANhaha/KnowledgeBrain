// S2: a later exception has no pre-bound dependency. Fixed oracle, no HTTP.
struct S2Model {
    cycle: CycleModel,
    observed_exception: AtomicBool,
}
#[async_trait]
impl Model for S2Model {
    async fn turn(&self, config: &Config, bytes: &[u8]) -> Result<ChatTurn, AgentError> {
        let wire: Value = serde_json::from_slice(bytes).unwrap();
        let payloads = wire["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|m| m["content"].as_str())
            .filter_map(|s| serde_json::from_str::<Value>(s).ok())
            .collect::<Vec<_>>();
        let mut response = self.cycle.turn(config, bytes).await?;
        for call in &mut response.tool_calls {
            assert_ne!(
                call.name, "finish_outline",
                "unresolved exception must not reach publication"
            );
            if call.name == "submit_pack" {
                let mut args: Value = serde_json::from_str(&call.arguments).unwrap();
                if let Some(requirement) = args["requirements"]
                    .as_array_mut()
                    .and_then(|rows| rows.first_mut())
                {
                    requirement["description"] =
                        json!("The bidder must submit a signed declaration.");
                }
                call.arguments = args.to_string();
            }
            if call.name == "submit_claim_comparison" {
                let unit = payloads
                    .iter()
                    .filter_map(|p| p.get("result"))
                    .rev()
                    .find(|r| r["original_requirement"].is_string())
                    .unwrap();
                let primary = unit["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|i| i["role"] == "requirement_source")
                    .map(|i| i["quote_handle"].clone())
                    .collect::<Vec<_>>();
                assert!(!primary.is_empty());
                let mut args: Value = serde_json::from_str(&call.arguments).unwrap();
                args["declared_claims"][0]["primary_handles"] = json!(primary);
                args["decisions"][0]["evidence_handles"] = json!(primary);
                call.arguments = args.to_string();
            }
            if call.name == "submit_review" {
                let mut args: Value = serde_json::from_str(&call.arguments).unwrap();
                let excerpt = payloads
                    .iter()
                    .filter_map(|p| p.get("result"))
                    .flat_map(|r| r["excerpts"].as_array().into_iter().flatten())
                    .find(|e| {
                        e["quote"].as_str().is_some_and(|q| q.contains("EXCEPTION"))
                            && e["review_evidence_key"].is_string()
                    })
                    .expect("later exception needs fresh Check source evidence");
                let row = payloads
                    .iter()
                    .filter_map(|p| p.get("result"))
                    .flat_map(|r| r["items"].as_array().into_iter().flatten())
                    .find(|r| r["evidence"] == excerpt["evidence"])
                    .unwrap();
                assert!(excerpt["quote"].as_str().unwrap().contains("EXCEPTION"));
                self.observed_exception.store(true, Ordering::SeqCst);
                let disposition = args["source_dispositions"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|d| d["pack_id"] == row["pack_id"])
                    .unwrap();
                disposition["verdict"] = json!("unresolved");
                disposition["requirement_ids"] = json!([]);
                disposition["reason"] = json!(
                    "The later EXCEPTION changes the earlier declaration obligation and was omitted from canonical extraction; re-extract its condition before readiness."
                );
                call.arguments = args.to_string();
            }
        }
        Ok(response)
    }
}
#[tokio::test]
async fn s2_later_unbound_exception_is_freshly_reviewed_and_reopens_without_readiness() {
    use crate::outline::frozen::{FrozenBuildInput, ParsedDocument, build_frozen_input};
    use sha2::{Digest, Sha256};
    let texts = [
        "The bidder must submit a signed declaration.",
        "EXCEPTION: bidders already certified need not submit that declaration.",
    ];
    let markdown = texts.join("\n");
    let hash = |text: &str| hex::encode(Sha256::digest(text.as_bytes()));
    let mut units = Vec::new();
    let mut originals = Vec::new();
    let mut offset = 0;
    for (ordinal, text) in texts.iter().enumerate() {
        let key = format!("section-{ordinal}");
        let heading = format!("Independent section {ordinal}");
        let locator = docparser::StructuredSourceLocator::Document {
            section_ordinal: ordinal as u32,
            table_ordinal: None,
            row_ordinal: None,
            form_ordinal: None,
            heading_path: heading.clone(),
        };
        units.push(json!({"unit_id":key,"ordinal":ordinal,"kind":"section","text_sha256":hash(text),"grid_sha256":null,"section_id":key,"parent_section_id":null,"heading_level":1,"heading_path":heading,"physical_locator":locator,"physical_path":format!("body/p:{ordinal}"),"physical_locator_unavailable_reason":null,"rendered_spans":[{"start_byte":offset,"end_byte":offset+text.len()}],"completeness":"complete","reasons":[],"table_id":null,"header_cells":[]}));
        originals.push(docparser::StructuredSourceUnit {
            key,
            ordinal: ordinal as u32,
            kind: docparser::StructuredSourceUnitKind::Section,
            text: (*text).into(),
            locator,
            grid: None,
        });
        offset += text.len() + 1;
    }
    let contract = json!({"schema_version":2,"document_revision":hash(&markdown),"parser_version":"synthetic-test-v2","markdown_sha256":hash(&markdown),"page_manifest":[],"units":units});
    let input = build_frozen_input(
        FrozenBuildInput {
            project_id: "s2-project".into(),
            document_set_id: "s2-set".into(),
            documents: vec![ParsedDocument {
                role: crate::analysis::DocumentRole::Unspecified,
                document_id: "s2-doc".into(),
                parsed: docparser::ReadResult {
                    markdown,
                    metadata: std::collections::HashMap::from([(
                        "source_contract".into(),
                        contract.to_string(),
                    )]),
                    structured_source_units: originals,
                    ..Default::default()
                },
            }],
            document_relations: vec![],
            decisions: vec![],
        },
        vec![],
        crate::outline::frozen::PARSER_CONTRACT_VERSION,
    )
    .unwrap();
    assert!(input.document_relations.is_empty());
    let config = crate::analysis::tests::config();
    let mut state = super::super::retirement::checkpoint(&input);
    state.turn = 0;
    state.config_sha256 = digest(&config).unwrap();
    state.outline_run.reading_packs = Some(
        crate::outline::discover::DiscoverWork::plan_with_budget(&input, &|sessions| {
            Ok(sessions
                .iter()
                .all(|s| s["pack"]["atoms"].as_array().unwrap().len() == 1))
        }),
    );
    let work = state.outline_run.reading_packs.as_ref().unwrap();
    assert_eq!(work.pack_counts().total, 2);
    for id in work.pack_ids() {
        assert!(work.required_support_refs(&input, &id).unwrap().is_empty());
    }
    let journal = CycleJournal {
        state: Mutex::new(state),
        stop_check: AtomicBool::new(false),
        stop_repair: AtomicBool::new(true),
    };
    let model = S2Model {
        cycle: CycleModel {
            discover: Delayed::default(),
            step: AtomicUsize::new(0),
            automatic: true,
            saw_repair_wire: AtomicBool::new(true),
        },
        observed_exception: AtomicBool::new(false),
    };
    let result = super::super::run(&input, &config, &journal, &model, &CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(result.code, "TEST_AUTO_REPAIR_RELOAD", "{}", result.message);
    assert!(model.observed_exception.load(Ordering::SeqCst));
    let saved = journal.load().await.unwrap().unwrap();
    assert!(!saved.done);
    assert!(!saved.outline_run.tool_draft.finished);
    assert!(crate::outline::tools::template_ready(&input, &saved.outline_run.tool_draft).is_err());
    assert!(!saved.outline_run.reading_packs.as_ref().unwrap().complete());
    assert!(
        saved
            .outline_run
            .repair_events
            .iter()
            .any(|e| e["finding_kind"] == "source_omission" && e["pack_id"] == "pack-1")
    );
    assert_eq!(
        model.cycle.discover.calls.lock().unwrap().len(),
        2,
        "test stops after durable repair before another extraction call"
    );
    let reloaded: Checkpoint = serde_json::from_value(json!(saved)).unwrap();
    assert_eq!(
        reloaded.outline_run.repair_events,
        saved.outline_run.repair_events
    );
    assert!(
        !reloaded
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .complete()
    );
    assert!(
        crate::outline::tools::template_ready(&input, &reloaded.outline_run.tool_draft).is_err()
    );
}
