// S4 composition through production worker delivery and serial source Check.
// All documents, source text, responses and relations are synthetic. No HTTP.
struct S4Model {
    cycle: CycleModel,
    automatic_pages: AtomicUsize,
    submitted_issues: AtomicBool,
}
#[async_trait]
impl Model for S4Model {
    async fn turn(&self, config: &Config, bytes: &[u8]) -> Result<ChatTurn, AgentError> {
        let wire: Value = serde_json::from_slice(bytes).unwrap();
        let payloads = wire["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|m| m["content"].as_str())
            .filter_map(|s| serde_json::from_str::<Value>(s).ok())
            .collect::<Vec<_>>();
        let pages = payloads
            .iter()
            .filter(|p| p.get("automatic_support_page").is_some())
            .count();
        self.automatic_pages.fetch_add(pages, Ordering::SeqCst);
        let mut response = self.cycle.turn(config, bytes).await?;
        for call in &mut response.tool_calls {
            assert_ne!(
                call.name, "finish_outline",
                "document issues must stop before publication"
            );
            if call.name == "submit_review" {
                let mut args: Value = serde_json::from_str(&call.arguments).unwrap();
                let source = payloads
                    .iter()
                    .filter_map(|p| p.get("result"))
                    .flat_map(|r| r["items"].as_array().into_iter().flatten())
                    .find(|r| r["pack_id"].is_string() && r["evidence"].is_object())
                    .expect("the affected source scope must be read through actual tools");
                let key = payloads
                    .iter()
                    .filter_map(|p| p.get("result"))
                    .flat_map(|r| r["excerpts"].as_array().into_iter().flatten())
                    .find(|e| {
                        e["evidence"] == source["evidence"] && e["review_evidence_key"].is_string()
                    })
                    .expect("document issue must use fresh original evidence");
                args["issues"] = json!([
                    {"id":"s4-missing","code":"document_missing","description":"Referenced synthetic annex is unavailable.","requested_information":"Supply the missing annex.","requirement_ids":[],"evidence":[{"review_evidence_key":key["review_evidence_key"]}]},
                    {"id":"s4-unconfirmed","code":"document_ambiguous","description":"The other synthetic annex reference has not been confirmed.","requested_information":"Confirm which annex the reference designates.","requirement_ids":[],"evidence":[{"review_evidence_key":key["review_evidence_key"]}]}
                ]);
                call.arguments = args.to_string();
                self.submitted_issues.store(true, Ordering::SeqCst);
            }
        }
        Ok(response)
    }
}

#[tokio::test]
async fn s4_exact_required_endpoint_same_named_peer_missing_and_unconfirmed_share_run() {
    use crate::analysis::source_manifest::{
        DocumentRelation, DocumentRelationKind, DocumentRelationStatus, RelationEndpoint,
    };
    let (mut input, config, initial) = fixture(3);
    // All three native units deliberately have the same section ID, heading and
    // body. Only document+unit endpoint identity may select the annex.
    assert_eq!(
        input.source_units[1].locator["unit_id"],
        input.source_units[2].locator["unit_id"]
    );
    assert_eq!(
        input.source_units[1].locator["heading_path"],
        input.source_units[2].locator["heading_path"]
    );
    let relation = |id: &str, status, to| DocumentRelation {
        id: id.into(),
        from: RelationEndpoint {
            document_id: "synthetic-0".into(),
            unit_id: Some("section".into()),
        },
        to,
        kind: DocumentRelationKind::ExplicitReference,
        status,
        required: true,
        basis: "Synthetic source sentence for transport admission testing.".into(),
        locator: input.source_units[0].locator.clone(),
    };
    input.document_relations = vec![
        relation(
            "exact",
            DocumentRelationStatus::Confirmed,
            Some(RelationEndpoint {
                document_id: "synthetic-1".into(),
                unit_id: Some("section".into()),
            }),
        ),
        relation("absent", DocumentRelationStatus::Missing, None),
        relation(
            "uncertain",
            DocumentRelationStatus::Unconfirmed,
            Some(RelationEndpoint {
                document_id: "synthetic-2".into(),
                unit_id: Some("section".into()),
            }),
        ),
    ];
    input.validate_document_relations().unwrap();
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
    assert!(work.planning_error().is_none());
    assert_eq!(work.pack_counts().total, 3);
    let required = work.required_support_refs(&input, "pack-0").unwrap();
    assert_eq!(required.len(), 1);
    assert!(
        matches!(&required[0],crate::outline::evidence::EvidenceRef::Text{unit_id,..} if unit_id==&input.source_units[1].source_unit_revision_id)
    );
    assert!(
        work.required_support_refs(&input, "pack-1")
            .unwrap()
            .is_empty(),
        "required relation is directed"
    );
    assert!(
        work.required_support_refs(&input, "pack-2")
            .unwrap()
            .is_empty(),
        "same-name unrelated source stays independent"
    );
    let journal = CycleJournal {
        state: Mutex::new(state),
        stop_check: AtomicBool::new(false),
        stop_repair: AtomicBool::new(false),
    };
    drop(initial);
    let model = S4Model {
        cycle: CycleModel {
            discover: Delayed::default(),
            step: AtomicUsize::new(0),
            automatic: false,
            saw_repair_wire: AtomicBool::new(true),
        },
        automatic_pages: AtomicUsize::new(0),
        submitted_issues: AtomicBool::new(false),
    };
    let failure = super::super::run(&input, &config, &journal, &model, &CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(failure.code, "DOCUMENT_ISSUES_REPORTED");
    assert_eq!(
        model.automatic_pages.load(Ordering::SeqCst),
        1,
        "only the exact required annex is automatically delivered"
    );
    assert!(model.submitted_issues.load(Ordering::SeqCst));
    let saved = journal.load().await.unwrap().unwrap();
    assert!(!saved.done);
    assert!(!saved.outline_run.tool_draft.finished);
    assert!(saved.outline_run.reading_packs.as_ref().unwrap().complete());
    assert_eq!(
        saved.outline_run.tool_draft.reviewed_pack_ids.len(),
        3,
        "missing relation must not starve the independent peer"
    );
    assert!(crate::outline::tools::template_ready(&input, &saved.outline_run.tool_draft).is_err());
    assert_eq!(saved.outline_run.tool_draft.review_issues.len(), 2);
    assert_eq!(
        saved.outline_run.discover_workers["pack-0"].delivered_related_evidence,
        required
    );
    assert!(
        saved.outline_run.discover_workers["pack-2"]
            .delivered_related_evidence
            .is_empty()
    );
    let reloaded: Checkpoint = serde_json::from_value(json!(saved)).unwrap();
    assert_eq!(reloaded.review_report(&input), saved.review_report(&input));
    assert!(crate::outline::source_review::document_issue_report_ready(
        &input,
        &reloaded.outline_run.tool_draft
    ));
}
