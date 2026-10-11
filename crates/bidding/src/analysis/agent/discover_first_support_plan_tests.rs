// Production planner -> actual PackHost prepare, with synthetic native inputs.
// The context window is fixed for both modes; there is no provider call.
#[tokio::test]
async fn production_first_required_support_is_inside_planned_text_and_image_window() {
    use crate::analysis::source_manifest::{
        DocumentRelation, DocumentRelationKind, DocumentRelationStatus, RelationEndpoint,
    };
    for image_support in [false, true] {
        let mut input = super::super::token_transport_validation::synthetic_input(4096, 1);
        let mut auxiliary = if image_support {
            crate::outline::frozen::tests::python_fixture_input(0)
        } else {
            super::super::token_transport_validation::synthetic_input(32, 2)
        };
        if !image_support {
            auxiliary
                .documents
                .retain(|d| d.document_id == "synthetic-1");
            auxiliary
                .source_units
                .retain(|s| s.document_id == "synthetic-1");
        }
        let target = auxiliary
            .source_units
            .iter()
            .find(|s| !image_support || s.locator["image_available"] == true)
            .unwrap()
            .clone();
        input.documents.extend(auxiliary.documents);
        input.source_units.extend(auxiliary.source_units);
        input.structured_forms.extend(auxiliary.structured_forms);
        input.document_relations = vec![DocumentRelation {
            id: "first-support".into(),
            from: RelationEndpoint {
                document_id: "synthetic-0".into(),
                unit_id: Some("section".into()),
            },
            to: Some(RelationEndpoint {
                document_id: target.document_id.clone(),
                unit_id: Some(target.locator["unit_id"].as_str().unwrap().into()),
            }),
            kind: DocumentRelationKind::ExplicitReference,
            status: DocumentRelationStatus::Confirmed,
            required: true,
            basis: "Synthetic source sentence for transport admission testing.".into(),
            locator: input.source_units[0].locator.clone(),
        }];
        input.validate_document_relations().unwrap();
        let mut config = crate::analysis::tests::config();
        config.limits.max_context_tokens = 49_152;
        let mut state = super::super::retirement::checkpoint(&input);
        state.turn = 0;
        state.config_sha256 = digest(&config).unwrap();
        if image_support {
            let rows: Value = serde_json::from_str(include_str!(
                "../../../../docparser/tests/fixtures/python-source-contract-v2.json"
            ))
            .unwrap();
            for source in input
                .source_units
                .iter()
                .filter(|s| s.locator["image_available"] == true)
            {
                let image = rows[0]["images"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|i| i["original_ref"] == source.locator["original_ref"])
                    .unwrap();
                let bytes = hex::decode(image["hex"].as_str().unwrap()).unwrap();
                let view = crate::outline::visual::render_frozen_image_view(
                    &source.source_unit_revision_id,
                    source.locator["image_ref"]
                        .as_str()
                        .unwrap()
                        .strip_prefix("objects/")
                        .unwrap(),
                    source.locator["page_ordinal"].as_u64().unwrap() as u32,
                    &bytes,
                    config.limits.max_source_view_edge,
                    config.limits.max_source_view_bytes,
                )
                .unwrap();
                state.source_views.insert(view.id().unwrap(), view);
            }
        }
        // No precomputed/manual plan: exercise the production fits closure.
        super::super::request(&input, &config, &mut state)
            .await
            .unwrap();
        assert!(
            state
                .outline_run
                .reading_packs
                .as_ref()
                .unwrap()
                .pack_counts()
                .total
                > 1
        );
        let journal = CycleJournal {
            state: Mutex::new(state.clone()),
            stop_check: AtomicBool::new(false),
            stop_repair: AtomicBool::new(false),
        };
        let cancel = CancellationToken::new();
        let mut host = PackHost {
            input: &input,
            config: &config,
            state: &mut state,
            journal: &journal,
            cancel: &cancel,
        };
        let request = host.next(&BTreeSet::new()).await.unwrap().unwrap();
        assert_eq!(
            request.scope.pack, "pack-0",
            "prepare must not skip a capacity-failed first pack"
        );
        assert!(
            !matches!(&host.state.outline_run.discover_workers["pack-0"].pending,Some(Pending::Failed{code,..}) if code=="DISCOVER_READ_CAPACITY_EXCEEDED"||code=="AGENT_TURN_BUDGET_EXCEEDED")
        );
        assert_ne!(
            host.state
                .outline_run
                .reading_packs
                .as_ref()
                .unwrap()
                .status("pack-0"),
            Some(crate::outline::discover::PackStatus::Failed)
        );
        assert_eq!(
            automatic_pages(&request).len(),
            1,
            "first worker needs actual automatic support, image={image_support}"
        );
        let wire: Value = serde_json::from_slice(&request.body).unwrap();
        let tokens = crate::agent_runtime::chat::estimate_request_tokens_with_reserve(
            &wire,
            &config.limits.tokenizer,
            config.limits.image_token_reserve,
            config.limits.token_safety_margin,
            config.provider.output_token_reserve as usize,
        )
        .unwrap();
        assert!(tokens.total_context_tokens <= config.limits.max_context_tokens);
        let images = wire["messages"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|m| m["content"].as_array().into_iter().flatten())
            .filter(|part| part["type"] == "image_url")
            .count();
        assert_eq!(images > 0, image_support);
        assert_eq!(
            host.state.journal.accounting.physical_calls, 0,
            "prepare neither reserves nor calls provider"
        );
    }
}
