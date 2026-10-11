//! Synthetic transport oracle: exercises the production extraction loop, no HTTP.
use super::*;
use std::sync::Mutex;

struct Saved(Mutex<Checkpoint>);
#[async_trait]
impl Journal for Saved {
    async fn load(&self) -> Result<Option<Checkpoint>, AgentError> {
        Ok(Some(self.0.lock().unwrap().clone()))
    }
    async fn save(&self, state: &Checkpoint, _: &Value) -> Result<(), AgentError> {
        *self.0.lock().unwrap() =
            serde_json::from_slice(&serde_json::to_vec(state).unwrap()).unwrap();
        Ok(())
    }
    async fn reserve(&self, state: &Checkpoint, _: &[u8]) -> Result<Option<usize>, AgentError> {
        self.save(state, &json!({})).await?;
        Ok(Some(1))
    }
    async fn admit(&self, state: &Checkpoint, _: &[u8]) -> Result<(), AgentError> {
        assert!(
            state.journal.accounting.physical_calls < 24,
            "synthetic loop failed to stop"
        );
        Ok(())
    }
}
#[derive(Default)]
struct SourceOracle {
    business_rejection: bool,
    rejected_revisions: Mutex<Vec<u64>>,
    phases: Mutex<Vec<&'static str>>,
    observed: Mutex<Vec<String>>,
}
#[async_trait]
impl Model for SourceOracle {
    async fn turn(&self, _: &Config, body: &[u8]) -> Result<ChatTurn, AgentError> {
        let wire: Value = serde_json::from_slice(body).unwrap();
        let payloads = wire["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|m| m["content"].as_str())
            .filter_map(|s| serde_json::from_str::<Value>(s).ok())
            .collect::<Vec<_>>();
        let discover = wire["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["function"]["name"] == "submit_pack");
        let mut phases = self.phases.lock().unwrap();
        assert!(
            !discover || !phases.contains(&"Check"),
            "fallback sent new Discover request after source review"
        );
        phases.push(if discover { "Discover" } else { "Check" });
        let ordinal = phases.len();
        drop(phases);
        let host = payloads
            .iter()
            .rev()
            .find(|p| p["wire_scope"].is_string())
            .unwrap();
        let (name, mut args) = if discover {
            let session = &payloads
                .iter()
                .find(|p| p["reading_packs"].is_array())
                .unwrap()["reading_packs"][0];
            let pack = &session["pack"];
            if pack["order"] == 0 && !self.business_rejection {
                return Err(error(
                    "TEST_PROVIDER_FAILED",
                    "synthetic terminal transport failure",
                ));
            }
            if pack["order"] == 0 {
                self.rejected_revisions
                    .lock()
                    .unwrap()
                    .push(pack["pack_revision"].as_u64().unwrap());
            }
            let requirements = if pack["order"] == 0 {
                let atom = &pack["atoms"][0];
                json!([{"description":"The bidder must submit a signed declaration.","kind":"technical","obligation_strength":"mandatory","extraction_quality":"explicit","source_section_id":{"atom_key":atom["atom_key"]},"evidence":[{"evidence_key":atom["excerpts"][0]["evidence_key"]}]}])
            } else {
                json!([])
            };
            // Nonempty valid extraction plus a negative reason reaches the
            // domain rejection path and rotates the repair claim revision.
            (
                "submit_pack",
                json!({"pack_id":pack["id"],"call_id":session["submission_operation_id"],"claim_token":pack["claim_token"],"pack_revision":pack["pack_revision"],"repair":session["status"]=="failed","requirements":requirements,"issues":[],"no_requirement_reason":"Synthetic intentionally omitted obligation; source Check must catch it.","inspected_atom_ids":pack["atoms"].as_array().unwrap().iter().filter(|a|a["context_only"]!=true).map(|a|json!({"atom_key":a["atom_key"]})).collect::<Vec<_>>()}),
            )
        } else {
            assert_eq!(host["source_only_review"], true);
            let scope = &host["check_work"]["source_scope"];
            assert!(
                scope.is_object(),
                "source-only mode must not enter template completion"
            );
            let results = payloads
                .iter()
                .filter_map(|p| p.get("result"))
                .collect::<Vec<_>>();
            let rows = results
                .iter()
                .flat_map(|r| r["items"].as_array().into_iter().flatten())
                .filter(|r| r["pack_id"] == scope["pack_id"] && r["evidence"].is_object())
                .collect::<Vec<_>>();
            let reviewed = results
                .iter()
                .flat_map(|r| r["excerpts"].as_array().into_iter().flatten())
                .filter(|e| e["review_evidence_key"].is_string())
                .collect::<Vec<_>>();
            if rows.is_empty() {
                ("read_requirements", json!({"mode":"packs"}))
            } else if let Some(row) = rows
                .iter()
                .find(|row| !reviewed.iter().any(|e| e["evidence"] == row["evidence"]))
            {
                (
                    row["read"]["tool"].as_str().unwrap(),
                    row["read"]["args"].clone(),
                )
            } else {
                let keys = reviewed
                    .iter()
                    .filter(|e| rows.iter().any(|r| r["evidence"] == e["evidence"]))
                    .map(|e| json!({"review_evidence_key":e["review_evidence_key"]}))
                    .collect::<Vec<_>>();
                assert!(!keys.is_empty());
                self.observed
                    .lock()
                    .unwrap()
                    .push(scope["pack_id"].as_str().unwrap().into());
                (
                    "submit_review",
                    json!({"requirement_ids":[],"pack_ids":[scope["pack_id"]],"inspected_evidence":keys,"issues":[],"template_reviews":[],"source_dispositions":[{"pack_id":scope["pack_id"],"version":scope["source_review_version"],"evidence":keys,"verdict":"unresolved","requirement_ids":[],"reason":"Synthetic source explicitly requires a signed declaration but empty discovery omitted it."}]}),
                )
            }
        };
        args["wire_scope"] = host["wire_scope"].clone();
        Ok(ChatTurn {
            content: String::new(),
            finish_reason: "tool_calls".into(),
            usage: None,
            tool_calls: vec![knowledge::models::ChatToolCall {
                id: format!("source-only-{ordinal}"),
                name: name.into(),
                arguments: args.to_string(),
            }],
        })
    }
}
#[tokio::test]
async fn partial_source_runtime_reads_all_independent_scopes_then_reopens_and_stops() {
    exercise_source_fallback(false).await;
}
#[tokio::test]
async fn partial_source_runtime_exhausted_business_repairs_preserve_terminal_audit() {
    exercise_source_fallback(true).await;
}
async fn exercise_source_fallback(business_rejection: bool) {
    let input = super::token_transport_validation::synthetic_input_with_text(
        "The bidder must submit a signed declaration.".into(),
        3,
    );
    let mut config = crate::analysis::tests::config();
    config.limits.max_no_progress_turns = 2;
    let mut state = super::retirement::checkpoint(&input);
    state.turn = 0;
    state.config_sha256 = digest(&config).unwrap();
    state.outline_run.reading_packs = Some(
        crate::outline::discover::DiscoverWork::plan_with_budget(&input, &|sessions| {
            Ok(sessions
                .iter()
                .all(|s| s["pack"]["atoms"].as_array().unwrap().len() == 1))
        }),
    );
    assert_eq!(
        state
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .pack_counts()
            .total,
        3
    );
    let journal = Saved(Mutex::new(state));
    let model = SourceOracle {
        business_rejection,
        ..Default::default()
    };
    let result = run(&input, &config, &journal, &model, &CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(result.code, "AGENT_DISCOVER_BLOCKED");
    let saved = journal.load().await.unwrap().unwrap();
    assert_eq!(saved.outline_run.tool_draft.required_pack_ids.len(), 3);
    assert!(!saved.done);
    assert!(!saved.outline_run.reading_packs.as_ref().unwrap().complete());
    assert!(crate::outline::partial_source::repair_stopped(&saved));
    let observed = model
        .observed
        .lock()
        .unwrap()
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        observed.len(),
        2,
        "one omitted scope must not starve the other"
    );
    assert_eq!(
        saved.outline_run.repair_events.last().unwrap()["source_only_findings"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    if business_rejection {
        let revisions = model.rejected_revisions.lock().unwrap();
        assert_eq!(revisions.len(), 2);
        // The second request uses the first live repair claim; rotation happens
        // when its failed repair submission is processed, not before dispatch.
        assert_eq!(revisions[1], revisions[0]);
        let request = saved
            .outline_run
            .discover_workers
            .values()
            .find_map(|worker| match &worker.pending {
                Some(discover_coordinator::Pending::Failed { request, code })
                    if code == "DISCOVER_RETRY_EXHAUSTED" =>
                {
                    Some(request)
                }
                _ => None,
            })
            .expect("settled exhausted request audit retained");
        assert_eq!(request.scope.revision, revisions[1]);
        assert!(
            saved
                .outline_run
                .reading_packs
                .as_ref()
                .unwrap()
                .canonical_pack(&request.scope.pack)
                .unwrap()
                .pack_revision
                > request.scope.revision
        );
    }
    let calls = model.phases.lock().unwrap().len();
    assert_eq!(
        model
            .phases
            .lock()
            .unwrap()
            .iter()
            .filter(|p| **p == "Discover")
            .count(),
        if business_rejection { 4 } else { 3 }
    );
    // Reload uses the real Journal serialization path. A second run must stop
    // before model dispatch, retaining the same global failure and required IDs.
    let again = run(&input, &config, &journal, &model, &CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(again.code, "AGENT_DISCOVER_BLOCKED");
    assert_eq!(model.phases.lock().unwrap().len(), calls);
    assert_eq!(
        journal
            .load()
            .await
            .unwrap()
            .unwrap()
            .outline_run
            .tool_draft
            .required_pack_ids
            .len(),
        3
    );
}
