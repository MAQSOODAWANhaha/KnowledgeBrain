use super::*;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

struct Durable {
    state: Mutex<Checkpoint>,
    stop_barrier: bool,
    crash_received: AtomicBool,
    call_limit: u64,
}
#[async_trait]
impl Journal for Durable {
    async fn load(&self) -> Result<Option<Checkpoint>, AgentError> {
        Ok(Some(self.state.lock().unwrap().clone()))
    }
    async fn admit(&self, state: &Checkpoint, _: &[u8]) -> Result<(), AgentError> {
        if state.journal.accounting.physical_calls > self.call_limit {
            return Err(error("TEST_SHARED_BUDGET", "root budget exhausted"));
        }
        Ok(())
    }
    async fn reserve(&self, _: &Checkpoint, _: &[u8]) -> Result<Option<usize>, AgentError> {
        Err(error(
            "TEST_PHASE_BARRIER",
            "root serial provider must not start before test barrier",
        ))
    }
    async fn save(&self, state: &Checkpoint, _: &Value) -> Result<(), AgentError> {
        *self.state.lock().unwrap() =
            serde_json::from_slice(&serde_json::to_vec(state).unwrap()).unwrap();
        if state
            .outline_run
            .discover_workers
            .values()
            .any(|worker| matches!(worker.pending, Some(Pending::Received { .. })))
            && self.crash_received.swap(false, Ordering::SeqCst)
        {
            return Err(error(
                "TEST_CRASH_RECEIVED",
                "response persisted before simulated lost acknowledgement",
            ));
        }
        if self.stop_barrier
            && state
                .outline_run
                .reading_packs
                .as_ref()
                .is_some_and(crate::outline::discover::DiscoverWork::complete)
        {
            return Err(error(
                "TEST_ALL_PACKS_BARRIER",
                "all committed, Organize not sent",
            ));
        }
        Ok(())
    }
}
struct Delayed {
    calls: Mutex<Vec<Vec<u8>>>,
    active: Arc<AtomicUsize>,
    peak: AtomicUsize,
    order: Mutex<Vec<String>>,
    delay_ms: u64,
    fail_first: bool,
}
impl Default for Delayed {
    fn default() -> Self {
        Self {
            calls: Mutex::default(),
            active: Arc::default(),
            peak: AtomicUsize::new(0),
            order: Mutex::default(),
            delay_ms: 30,
            fail_first: false,
        }
    }
}
#[async_trait]
impl Model for Delayed {
    async fn turn(&self, _: &Config, body: &[u8]) -> Result<ChatTurn, AgentError> {
        let position = {
            let mut calls = self.calls.lock().unwrap();
            let position = calls.len();
            calls.push(body.to_vec());
            position
        };
        let n = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(n, Ordering::SeqCst);
        struct DropActive(Arc<AtomicUsize>);
        impl Drop for DropActive {
            fn drop(&mut self) {
                self.0.fetch_sub(1, Ordering::SeqCst);
            }
        }
        let _active = DropActive(self.active.clone());
        tokio::time::sleep(Duration::from_millis(if position == 0 {
            self.delay_ms
        } else {
            2
        }))
        .await;
        if self.fail_first && position == 0 {
            return Err(error("TEST_PROVIDER_FAILED", "one pack transport failure"));
        }
        let body: Value = serde_json::from_slice(body).unwrap();
        let messages = body["messages"].as_array().unwrap();
        let packet = messages
            .iter()
            .filter_map(|m| m["content"].as_str())
            .filter_map(|t| serde_json::from_str::<Value>(t).ok())
            .find(|v| v["reading_packs"].is_array())
            .unwrap();
        let scope = messages
            .iter()
            .filter_map(|m| m["content"].as_str())
            .filter_map(|t| serde_json::from_str::<Value>(t).ok())
            .find_map(|v| v["wire_scope"].as_str().map(str::to_owned))
            .unwrap();
        let session = &packet["reading_packs"][0];
        let pack = &session["pack"];
        self.order.lock().unwrap().push(format!("{position}"));
        Ok(ChatTurn{content:String::new(),finish_reason:"tool_calls".into(),usage:Some(knowledge::models::ChatUsage{prompt_tokens:Some(10),completion_tokens:Some(2),total_tokens:Some(12),..Default::default()}),tool_calls:vec![knowledge::models::ChatToolCall{id:format!("result-{position}"),name:"submit_pack".into(),arguments:json!({"wire_scope":scope,"pack_id":pack["id"],"call_id":session["submission_operation_id"],"claim_token":pack["claim_token"],"pack_revision":pack["pack_revision"],"repair":session["status"]=="failed","requirements":[],"issues":[],"no_requirement_reason":"Synthetic protocol fixture only; no semantic model result.","inspected_atom_ids":pack["atoms"].as_array().unwrap().iter().filter(|atom|atom["context_only"]!=true).map(|atom|json!({"atom_key":atom["atom_key"]})).collect::<Vec<_>>()}).to_string()}]})
    }
}
fn fixture(packs: usize) -> (FrozenInput, Config, Durable) {
    let input = super::super::token_transport_validation::synthetic_input(2, packs);
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
    assert_eq!(
        state
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .pack_counts()
            .total,
        packs
    );
    (
        input,
        config,
        Durable {
            state: Mutex::new(state),
            stop_barrier: true,
            crash_received: AtomicBool::new(false),
            call_limit: 99,
        },
    )
}
async fn invoke(
    input: &FrozenInput,
    config: &Config,
    journal: &Durable,
    model: &Delayed,
    cancel: &CancellationToken,
) -> AgentError {
    super::super::run(input, config, journal, model, cancel)
        .await
        .unwrap_err()
}
#[tokio::test]
async fn production_entry_overlaps_and_commits_out_of_order_with_shared_ledger() {
    let (input, config, journal) = fixture(2);
    {
        let mut saved = journal.state.lock().unwrap();
        saved.journal.accounting.physical_calls = 7;
        saved.journal.accounting.reserved_input_tokens = 100;
        saved.journal.accounting.reserved_output_tokens = 50;
    }
    let model = Delayed::default();
    assert_eq!(
        invoke(&input, &config, &journal, &model, &CancellationToken::new())
            .await
            .code,
        "TEST_ALL_PACKS_BARRIER"
    );
    assert_eq!(model.peak.load(Ordering::SeqCst), 2);
    assert_eq!(*model.order.lock().unwrap(), vec!["1", "0"]);
    let calls = model.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert_ne!(calls[0], calls[1]);
    let saved = journal.state.lock().unwrap();
    assert!(saved.outline_run.reading_packs.as_ref().unwrap().complete());
    assert_eq!(saved.journal.accounting.physical_calls, 9);
    assert_eq!(saved.journal.usage.total_tokens, 24);
    assert!(
        saved
            .outline_run
            .discover_workers
            .values()
            .all(|w| matches!(w.pending, Some(Pending::Committed { .. })))
    );
}
#[tokio::test]
async fn production_received_crash_replays_without_model_call() {
    let (input, config, journal) = fixture(1);
    journal.crash_received.store(true, Ordering::SeqCst);
    let first = Delayed::default();
    assert_eq!(
        invoke(&input, &config, &journal, &first, &CancellationToken::new())
            .await
            .code,
        "TEST_CRASH_RECEIVED"
    );
    let replay = Delayed::default();
    assert_eq!(
        invoke(
            &input,
            &config,
            &journal,
            &replay,
            &CancellationToken::new()
        )
        .await
        .code,
        "TEST_ALL_PACKS_BARRIER"
    );
    assert!(replay.calls.lock().unwrap().is_empty());
    let saved = journal.state.lock().unwrap();
    assert_eq!(saved.journal.accounting.physical_calls, 1);
    assert_eq!(saved.journal.usage.total_tokens, 12);
}
#[tokio::test]
async fn production_budget_denial_drains_and_preserves_first_response() {
    let (input, config, mut journal) = fixture(2);
    journal.call_limit = 1;
    let model = Delayed::default();
    assert_eq!(
        invoke(&input, &config, &journal, &model, &CancellationToken::new())
            .await
            .code,
        "TEST_SHARED_BUDGET"
    );
    assert_eq!(model.calls.lock().unwrap().len(), 1);
    let saved = journal.state.lock().unwrap();
    assert_eq!(saved.journal.accounting.physical_calls, 1);
    assert_eq!(
        saved
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .pack_counts()
            .committed,
        1
    );
}
#[tokio::test]
async fn production_cancel_retains_unknown_send_and_resume_does_not_repeat() {
    let (input, config, journal) = fixture(2);
    let model = Delayed {
        delay_ms: 1000,
        ..Default::default()
    };
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    let active = model.active.clone();
    let task = tokio::spawn(async move {
        while active.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
        trigger.cancel();
    });
    assert_eq!(
        invoke(&input, &config, &journal, &model, &cancel)
            .await
            .code,
        "AGENT_CANCELLED"
    );
    task.await.unwrap();
    assert_eq!(model.active.load(Ordering::SeqCst), 0);
    let charged = journal
        .state
        .lock()
        .unwrap()
        .journal
        .accounting
        .physical_calls;
    assert_eq!(charged, 2);
    let replay = Delayed::default();
    assert_eq!(
        invoke(
            &input,
            &config,
            &journal,
            &replay,
            &CancellationToken::new()
        )
        .await
        .code,
        "AGENT_SEND_OUTCOME_UNKNOWN"
    );
    assert!(replay.calls.lock().unwrap().is_empty());
    assert_eq!(
        journal
            .state
            .lock()
            .unwrap()
            .journal
            .accounting
            .physical_calls,
        charged
    );
}
#[tokio::test]
async fn production_failed_pack_does_not_discard_peer_commit() {
    let (input, config, journal) = fixture(2);
    let model = Delayed {
        fail_first: true,
        ..Default::default()
    };
    assert_eq!(
        invoke(&input, &config, &journal, &model, &CancellationToken::new())
            .await
            .code,
        "TEST_PHASE_BARRIER"
    );
    assert_eq!(
        journal
            .state
            .lock()
            .unwrap()
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .pack_counts()
            .committed,
        1
    );

    let mut saved = journal.state.lock().unwrap().clone();
    assert!(!saved.done);
    assert!(!saved.outline_run.reading_packs.as_ref().unwrap().complete());
    assert_eq!(model.calls.lock().unwrap().len(), 2);
    assert!(saved.outline_run.discover_workers.values().any(|worker| matches!(&worker.pending, Some(Pending::Failed { code, .. }) if code == "TEST_PROVIDER_FAILED")));
    crate::outline::agent::synchronize_discovery(&input, &mut saved).unwrap();
    assert_eq!(crate::outline::agent::current(&input, &saved), Duty::Check);
    assert_eq!(
        crate::outline::partial_source::eligible(&input, &saved)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(saved.outline_run.tool_draft.required_pack_ids.len(), 2);
}

#[tokio::test]
async fn production_targeted_reopen_only_sends_changed_pack() {
    let (input, config, journal) = fixture(2);
    let model = Delayed::default();
    assert_eq!(
        invoke(&input, &config, &journal, &model, &CancellationToken::new())
            .await
            .code,
        "TEST_ALL_PACKS_BARRIER"
    );
    {
        let mut saved = journal.state.lock().unwrap();
        let work = saved.outline_run.reading_packs.as_mut().unwrap();
        let id = work.pack_ids().into_iter().next().unwrap();
        work.reopen_committed(&input, &id, 1, "targeted-check-fixture")
            .unwrap();
    }
    let repair = Delayed::default();
    assert_eq!(
        invoke(
            &input,
            &config,
            &journal,
            &repair,
            &CancellationToken::new()
        )
        .await
        .code,
        "TEST_ALL_PACKS_BARRIER"
    );
    assert_eq!(repair.calls.lock().unwrap().len(), 1);
    assert_eq!(
        journal
            .state
            .lock()
            .unwrap()
            .journal
            .accounting
            .physical_calls,
        3
    );
}

struct CycleJournal {
    state: Mutex<Checkpoint>,
    stop_check: AtomicBool,
    stop_repair: AtomicBool,
}
#[async_trait]
impl Journal for CycleJournal {
    async fn load(&self) -> Result<Option<Checkpoint>, AgentError> {
        Ok(Some(
            serde_json::from_slice(&serde_json::to_vec(&*self.state.lock().unwrap()).unwrap())
                .unwrap(),
        ))
    }
    async fn reserve(&self, state: &Checkpoint, _: &[u8]) -> Result<Option<usize>, AgentError> {
        self.save(state, &json!({})).await?;
        Ok(Some(1))
    }
    async fn save(&self, state: &Checkpoint, _: &Value) -> Result<(), AgentError> {
        let _profile = super::super::profile::enter("fixture_checkpoint_serialize_save");
        *self.state.lock().unwrap() =
            serde_json::from_slice(&serde_json::to_vec(state).unwrap()).unwrap();
        if self.stop_repair.load(Ordering::SeqCst)
            && state.journal.pending.is_none()
            && !state.outline_run.repair_events.is_empty()
        {
            return Err(error(
                "TEST_AUTO_REPAIR_RELOAD",
                "automatic repair persisted",
            ));
        }
        if self.stop_check.load(Ordering::SeqCst)
            && state.journal.pending.is_none()
            && state.outline_run.tool_draft.reviewed_pack_ids.len() == 2
        {
            return Err(error(
                "TEST_CHECK_RELOAD",
                "committed checkpoint before publication",
            ));
        }
        Ok(())
    }
}
struct CycleModel {
    discover: Delayed,
    step: AtomicUsize,
    automatic: bool,
    saw_repair_wire: AtomicBool,
}
#[async_trait]
impl Model for CycleModel {
    async fn turn(&self, config: &Config, body: &[u8]) -> Result<ChatTurn, AgentError> {
        let wire: Value = serde_json::from_slice(body).unwrap();
        if wire["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["function"]["name"] == "submit_pack")
        {
            let mut response = self.discover.turn(config, body).await?;
            if self.automatic {
                let payloads = wire["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|m| m["content"].as_str())
                    .filter_map(|s| serde_json::from_str::<Value>(s).ok())
                    .collect::<Vec<_>>();
                let packet = payloads
                    .iter()
                    .find(|v| v["reading_packs"].is_array())
                    .unwrap();
                assert_eq!(
                    packet["semantic_contract"],
                    crate::outline::agent::semantic_guidance(Duty::Discover)
                );
                let session = &packet["reading_packs"][0];
                let atom = &session["pack"]["atoms"][0];
                let owner = payloads
                    .iter()
                    .find_map(|p| p["worker_pack"].as_str())
                    .unwrap();
                if !packet["repair_context"].is_null() {
                    assert_eq!(
                        packet["repair_context"]["pack_revision"],
                        session["pack"]["pack_revision"]
                    );
                    assert_eq!(packet["repair_context"]["pack_id"], session["pack"]["id"]);
                    assert!(
                        packet["repair_context"]["original_requirement"]["description"].is_string()
                    );
                    assert_eq!(
                        packet["repair_context"]["comparison"]["decisions"][0]["verdict"],
                        "uncertain"
                    );
                    self.saw_repair_wire.store(true, Ordering::SeqCst);
                }
                if owner == "pack-0" {
                    let mut args: Value =
                        serde_json::from_str(&response.tool_calls[0].arguments).unwrap();
                    args["requirements"] = json!([{"description":"Synthetic fixture response obligation","kind":"technical","obligation_strength":"mandatory","extraction_quality":"explicit","source_section_id":{"atom_key":atom["atom_key"]},"evidence":[{"evidence_key":atom["excerpts"][0]["evidence_key"]}],"condition_support":[]}]);
                    args["inspected_atom_ids"] = json!([]);
                    args["no_requirement_reason"] = Value::Null;
                    response.tool_calls[0].arguments = args.to_string();
                }
            }
            return Ok(response);
        }
        let payloads = wire["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|m| m["content"].as_str())
            .filter_map(|s| serde_json::from_str::<Value>(s).ok())
            .collect::<Vec<_>>();
        let host = payloads
            .iter()
            .rev()
            .find(|v| v["wire_scope"].is_string())
            .unwrap();
        let results = payloads
            .iter()
            .filter_map(|v| v.get("result"))
            .collect::<Vec<_>>();
        for value in &payloads {
            assert!(value["ok"] != false, "synthetic wire error: {value}");
        }
        let ids = host["requirements"]["items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| r.get("requirement_id"))
            .cloned()
            .collect::<Vec<_>>();
        let pack_rows = results
            .iter()
            .filter_map(|r| r["items"].as_array())
            .flatten()
            .filter(|r| r["pack_id"].is_string())
            .collect::<Vec<_>>();
        let packs = pack_rows
            .iter()
            .filter_map(|r| r["pack_id"].as_str())
            .collect::<BTreeSet<_>>();
        let refs = pack_rows
            .iter()
            .filter_map(|r| r.get("evidence"))
            .cloned()
            .collect::<Vec<_>>();
        let review_keys = results
            .iter()
            .filter_map(|r| r["excerpts"].as_array())
            .flatten()
            .filter_map(|e| e.get("review_evidence_key"))
            .map(|key| json!({"review_evidence_key":key}))
            .collect::<Vec<_>>();
        // Every identity/version/receipt below comes from the actual local mock
        // request. This model has no reference to the host checkpoint or Draft.
        let source_dispositions = packs.iter().map(|pack| {
            let rows: Vec<_> = pack_rows.iter().filter(|r| r["pack_id"] == **pack).collect();
            let source_refs: Vec<_> = rows.iter().filter_map(|r| r.get("evidence")).collect();
            let keys: Vec<_> = results.iter().filter_map(|r| r["excerpts"].as_array()).flatten()
                .filter(|e| source_refs.contains(&&e["evidence"]))
                .filter_map(|e| e.get("review_evidence_key"))
                .map(|key|json!({"review_evidence_key":key})).collect();
            let linked: Vec<_> = host["requirements"]["items"].as_array().into_iter().flatten()
                .filter(|r| r["requirement"]["evidence"].as_array().into_iter().flatten()
                    .any(|reference|source_refs.contains(&reference)))
                .map(|r|r["requirement_id"].clone()).collect();
            json!({"pack_id":pack,"version":rows[0]["source_review_version"],"evidence":keys,
                "verdict":if linked.is_empty(){"no_response_obligation"}else{"contains_obligations"},
                "requirement_ids":linked,"reason":"Local synthetic source review, not semantic model acceptance"})
        }).collect::<Vec<_>>();
        // A repaired empty-obligation pack may preserve a complete independent
        // response draft. Follow the advertised duty instead of replaying
        // Organize mutations that are no longer legal in Check.
        let advertised = wire["tools"].as_array().unwrap();
        let allows = |name: &str| {
            advertised
                .iter()
                .any(|tool| tool["function"]["name"] == name)
        };
        if allows("submit_review") && !allows("put_chapters") {
            let _ = self.step.compare_exchange(
                0,
                if self.automatic { 3 } else { 2 },
                Ordering::SeqCst,
                Ordering::SeqCst,
            );
        }
        let step = self.step.fetch_add(1, Ordering::SeqCst);
        let (name, mut args) = if self.automatic {
            let id = ids
                .first()
                .cloned()
                .or_else(|| {
                    results
                        .iter()
                        .rev()
                        .find_map(|r| r.get("requirement_id").cloned())
                })
                .expect("requirement must be visible in request");
            match step {
                0 => (
                    "put_chapters",
                    json!({"mode":"replace","chapters":[{"id":"synthetic","parent_id":null,"order":0,"title":"Synthetic response","purpose":"response","requirement_ids":ids}]}),
                ),
                1 => (
                    "put_slots",
                    json!({"mode":"replace","slots":[{"slot_id":"blank","chapter_id":"synthetic","content":{"type":"editable_blank"},"blank_kind":"bidder_blank","match_query":"Synthetic response"}]}),
                ),
                2 => (
                    "put_fulfillments",
                    json!({"fulfillments":[{"requirement_id":id,"primary_response_chapter_id":"synthetic","target_refs":[{"type":"text_slot","slot_id":"blank"}]}]}),
                ),
                3 => ("read_requirements", json!({"mode":"packs"})),
                4 => ("read_evidence", json!({"refs":refs})),
                5 => (
                    "read_outline",
                    json!({"mode":"slot_body","slot_id":"blank"}),
                ),
                6 => ("read_claim_evidence", json!({"requirement_id":id})),
                7 => {
                    let unit = results
                        .iter()
                        .rev()
                        .find(|r| r["original_requirement"].is_string())
                        .expect("claim evidence must be in actual request");
                    let handles = unit["items"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|u| u["quote_handle"].clone())
                        .collect::<Vec<_>>();
                    let repairing = !self.saw_repair_wire.load(Ordering::SeqCst);
                    (
                        "submit_claim_comparison",
                        json!({"requirement_id":unit["requirement_id"],"version":unit["version"],"declared_claims":[{"claim_handle":"requirement","obligation":unit["original_requirement"],"applicability":"synthetic fixture scope","original_fragments":[unit["original_requirement"]],"primary_handles":handles,"support_handles":[]}],"observations":handles.iter().map(|h|json!({"quote_handle":h,"polarity":"mixed","applicability":"relevant","condition":"synthetic condition requiring independent review"})).collect::<Vec<_>>(),"decisions":[{"claim_handle":"requirement","evidence_handles":handles,"verdict":if repairing {"uncertain"}else{"supports"},"action":if repairing {"manual_review"}else{"retain"},"resulting_claim":unit["original_requirement"]}]}),
                    )
                }
                8 => (
                    "submit_review",
                    json!({"requirement_ids":[id],"pack_ids":packs,"inspected_evidence":review_keys,"issues":[],
                        "source_dispositions":source_dispositions,
                        "template_reviews":[{"requirement_id":id,"version":host["check_work"]["template_review_version"],
                            "claims":[{"claim_handle":"requirement","verdict":"template_ready",
                                "reason":"Synthetic visible empty response field", "fields":[{"type":"slot","slot_id":"blank","label":"Synthetic response"}]}]}]}),
                ),
                9 => ("finish_outline", json!({})),
                _ => panic!("bounded automatic wire cycle exceeded"),
            }
        } else {
            match step {
                0 => (
                    "put_chapters",
                    json!({"mode":"replace","chapters":[{"id":"synthetic","parent_id":null,"order":0,"title":"Synthetic response","purpose":"response","requirement_ids":[]}]}),
                ),
                1 => (
                    "put_slots",
                    json!({"mode":"replace","slots":[{"slot_id":"blank","chapter_id":"synthetic","content":{"type":"editable_blank"},"blank_kind":"bidder_blank","match_query":"Synthetic response"}]}),
                ),
                2 => ("read_requirements", json!({"mode":"packs"})),
                3 => ("read_evidence", json!({"refs":refs})),
                4 => (
                    "submit_review",
                    json!({"requirement_ids":[],"pack_ids":packs,"inspected_evidence":review_keys,"issues":[],"source_dispositions":source_dispositions,"template_reviews":[]}),
                ),
                5 => ("finish_outline", json!({})),
                _ => panic!("bounded synthetic wire cycle exceeded"),
            }
        };
        args["wire_scope"] = host["wire_scope"].clone();
        Ok(ChatTurn {
            content: String::new(),
            finish_reason: "tool_calls".into(),
            usage: None,
            tool_calls: vec![knowledge::models::ChatToolCall {
                id: format!("cycle-{step}"),
                name: name.into(),
                arguments: args.to_string(),
            }],
        })
    }
}
#[tokio::test]
async fn production_parallel_organize_check_reopen_fresh_check_persist_reload() {
    let (input, config, initial) = fixture(2);
    let journal = CycleJournal {
        state: initial.state,
        stop_check: AtomicBool::new(true),
        stop_repair: AtomicBool::new(false),
    };
    let model = CycleModel {
        discover: Delayed::default(),
        step: AtomicUsize::new(0),
        automatic: false,
        saw_repair_wire: AtomicBool::new(false),
    };
    let err = super::super::run(&input, &config, &journal, &model, &CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(err.code, "TEST_CHECK_RELOAD");
    assert_eq!(model.discover.peak.load(Ordering::SeqCst), 2);
    let mut restored = journal.load().await.unwrap().unwrap();
    assert_eq!(restored.outline_run.tool_draft.reviewed_pack_ids.len(), 2);
    assert!(restored.journal.pending.is_none());
    let retained_chapters = restored.outline_run.tool_draft.chapters.clone();
    let retained_slots = restored.outline_run.tool_draft.slots.clone();
    let work = restored.outline_run.reading_packs.as_ref().unwrap();
    let id = work.pack_ids().into_iter().next().unwrap();
    let revision = work.session(&input, &id).unwrap()["pack"]["pack_revision"]
        .as_u64()
        .unwrap();
    crate::outline::agent::reopen_discovery_pack(
        &input,
        &mut restored,
        &id,
        revision,
        "synthetic-host-repair",
    )
    .unwrap();
    assert!(
        restored
            .outline_run
            .tool_draft
            .check_reads
            .evidence
            .is_empty()
    );
    restored.transcript.clear();
    journal.save(&restored, &json!({})).await.unwrap();
    journal.stop_check.store(false, Ordering::SeqCst);
    model.step.store(0, Ordering::SeqCst);
    super::super::run(&input, &config, &journal, &model, &CancellationToken::new())
        .await
        .unwrap();
    let final_state = journal.load().await.unwrap().unwrap();
    assert!(final_state.done);
    assert_eq!(
        final_state.outline_run.tool_draft.chapters,
        retained_chapters
    );
    assert_eq!(final_state.outline_run.tool_draft.slots, retained_slots);
    assert!(final_state.outline_run.tool_draft.finished);
    assert!(
        !final_state
            .outline_run
            .tool_draft
            .source_dispositions
            .is_empty()
    );
    assert_eq!(
        final_state.outline_run.tool_draft.reviewed_pack_ids.len(),
        2
    );
    assert_eq!(model.discover.calls.lock().unwrap().len(), 3);
    assert!(
        !final_state
            .outline_run
            .tool_draft
            .check_reads
            .evidence
            .is_empty()
    );
}

#[tokio::test]
async fn empty_business_draft_retains_issued_scope_on_serialization() {
    let (input, config, journal) = fixture(1);
    let mut state = journal.load().await.unwrap().unwrap();
    super::super::request(&input, &config, &mut state)
        .await
        .unwrap();
    assert!(state.outline_run.tool_draft.is_empty());
    let args = state
        .outline_run
        .tool_draft
        .model_wire
        .fixture_encode(json!({}))
        .unwrap();
    let restored: Checkpoint =
        serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
    assert_eq!(
        restored
            .outline_run
            .tool_draft
            .model_wire
            .decode(args, false)
            .unwrap(),
        json!({})
    );
}
#[tokio::test]
async fn production_claim_comparison_automatically_reopens_and_finishes_fresh_check() {
    let (input, config, initial) = fixture(2);
    let journal = CycleJournal {
        state: initial.state,
        stop_check: AtomicBool::new(false),
        stop_repair: AtomicBool::new(true),
    };
    let model = CycleModel {
        discover: Delayed::default(),
        step: AtomicUsize::new(0),
        automatic: true,
        saw_repair_wire: AtomicBool::new(false),
    };
    let err = super::super::run(&input, &config, &journal, &model, &CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(err.code, "TEST_AUTO_REPAIR_RELOAD");
    let mut restored = journal.load().await.unwrap().unwrap();
    assert_eq!(restored.outline_run.repair_events.len(), 1);
    assert_eq!(restored.outline_run.repair_events[0]["status"], "reopened");
    assert!(
        !restored.outline_run.tool_draft.chapters.is_empty(),
        "repair retains chapter structure while invalidating affected mappings"
    );
    assert!(
        restored
            .outline_run
            .tool_draft
            .check_reads
            .evidence
            .is_empty()
    );
    assert_eq!(
        crate::outline::agent::current(&input, &restored),
        crate::outline::agent::Duty::Discover
    );
    restored.transcript.clear();
    journal.stop_repair.store(false, Ordering::SeqCst);
    journal.save(&restored, &json!({})).await.unwrap();
    model.step.store(0, Ordering::SeqCst);
    super::super::run(&input, &config, &journal, &model, &CancellationToken::new())
        .await
        .unwrap();
    let final_state = journal.load().await.unwrap().unwrap();
    assert!(final_state.done);
    assert!(final_state.outline_run.tool_draft.finished);
    assert!(
        !final_state
            .outline_run
            .tool_draft
            .source_dispositions
            .is_empty()
    );
    assert_eq!(final_state.outline_run.repair_events.len(), 1);
    assert_eq!(model.discover.calls.lock().unwrap().len(), 3);
    assert_eq!(model.discover.peak.load(Ordering::SeqCst), 2);
    assert!(
        !final_state
            .outline_run
            .tool_draft
            .check_reads
            .evidence
            .is_empty()
    );
}

#[test]
fn worker_image_credit_validates_exact_bytes_atomically_and_requires_received() {
    let input = crate::outline::frozen::tests::python_fixture_input(0);
    let config = crate::analysis::tests::config();
    let mut state = super::super::retirement::checkpoint(&input);
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
            128,
            8192,
        )
        .unwrap();
        state.source_views.insert(view.id().unwrap(), view);
    }
    let mut work = crate::outline::discover::DiscoverWork::plan_with_budget(&input, &|s| {
        Ok(s.iter()
            .all(|s| s["pack"]["atoms"].as_array().unwrap().len() == 1))
    });
    work.claim(100);
    let (id,view)=work.pack_ids().into_iter().find_map(|id|state.source_views.values().find(|v|work.pack_evidence(&input,&id).unwrap().iter().any(|e|matches!(e,crate::outline::evidence::EvidenceRef::ImageRegion{image_id,..} if image_id==&v.identity.source_id))).map(|v|(id,v.clone()))).unwrap();
    let mut shared = json!(work);
    let mut peer = shared["packs"][&id].clone();
    peer["pack"]["id"] = json!("synthetic-shared-image-peer");
    shared["packs"]["synthetic-shared-image-peer"] = peer;
    work = serde_json::from_value(shared).unwrap();
    state.outline_run.reading_packs = Some(work);
    let request = Request {
        scope: Scope {
            run: "synthetic".into(),
            pack: id.clone(),
            revision: 1,
            generation: 1,
            request: "synthetic-image-request".into(),
        },
        body: serde_json::to_vec(&json!({"messages":[view.message()]})).unwrap(),
        input_reserve: 1,
        output_reserve: 1,
    };
    let response:ChatTurn=serde_json::from_value(json!({"content":"","finish_reason":"tool_calls","usage":null,"tool_calls":[{"id":"image","name":"submit_pack","arguments":"{}"}]})).unwrap();
    let worker = state
        .outline_run
        .discover_workers
        .entry(id.clone())
        .or_default();
    worker.images = vec![(
        view.identity.source_id.clone(),
        view.identity.image_sha256.clone(),
    )];
    worker.pending = Some(Pending::Received {
        request: request.clone(),
        response: json!(response),
    });
    let baseline = json!(state.outline_run.reading_packs);
    for mode in [
        "original",
        "bytes",
        "dimensions",
        "missing-wire",
        "unknown-send",
        "incomplete",
        "atomic-second",
    ] {
        let mut candidate = state.clone();
        let mut req = request.clone();
        let cached = candidate
            .source_views
            .values_mut()
            .find(|v| v.identity.source_id == view.identity.source_id)
            .unwrap();
        match mode {
            "original" => cached.identity.original_sha256 = "0".repeat(64),
            "bytes" => cached.jpeg_base64 = "invalid".into(),
            "dimensions" => cached.identity.width += 1,
            "missing-wire" => {
                req.body = serde_json::to_vec(&json!({"messages":[]})).unwrap();
                candidate
                    .outline_run
                    .discover_workers
                    .get_mut(&id)
                    .unwrap()
                    .pending = Some(Pending::Received {
                    request: req.clone(),
                    response: json!(response),
                });
            }
            "unknown-send" => {
                candidate
                    .outline_run
                    .discover_workers
                    .get_mut(&id)
                    .unwrap()
                    .pending = Some(Pending::Sending {
                    request: req.clone(),
                })
            }
            "incomplete" => {
                candidate
                    .outline_run
                    .discover_workers
                    .get_mut(&id)
                    .unwrap()
                    .pending = Some(Pending::Received {
                    request: req.clone(),
                    response: json!({"content":"","finish_reason":"length","usage":null,"tool_calls":[]}),
                })
            }
            "atomic-second" => candidate
                .outline_run
                .discover_workers
                .get_mut(&id)
                .unwrap()
                .images
                .push(("unknown-image".into(), "0".repeat(64))),
            _ => unreachable!(),
        }
        assert!(
            confirm_worker_images(&input, &config, &mut candidate, &req).is_err(),
            "{mode}"
        );
        assert_eq!(
            json!(candidate.outline_run.reading_packs),
            baseline,
            "no partial credit: {mode}"
        );
    }
    let peers = state
        .outline_run
        .reading_packs
        .as_ref()
        .unwrap()
        .pack_ids()
        .into_iter()
        .filter(|p| p != &id && p != "synthetic-shared-image-peer")
        .map(|p| {
            let v = state
                .outline_run
                .reading_packs
                .as_ref()
                .unwrap()
                .session(&input, &p)
                .unwrap();
            (p, v)
        })
        .collect::<Vec<_>>();
    confirm_worker_images(&input, &config, &mut state, &request).unwrap();
    let credited = json!(state.outline_run.reading_packs);
    assert_eq!(
        credited["packs"]["synthetic-shared-image-peer"]["visual_receipts"],
        json!({})
    );
    assert_eq!(
        credited["packs"][&id]["visual_receipts"][&view.identity.source_id],
        view.identity.image_sha256
    );
    for (peer, before) in peers {
        assert_eq!(
            state
                .outline_run
                .reading_packs
                .as_ref()
                .unwrap()
                .session(&input, &peer)
                .unwrap(),
            before
        );
    }
}

struct RelatedCycleModel {
    cycle: CycleModel,
    saw_attachment: AtomicBool,
    form_step: AtomicUsize,
}
#[async_trait]
impl Model for RelatedCycleModel {
    async fn turn(&self, config: &Config, bytes: &[u8]) -> Result<ChatTurn, AgentError> {
        let body: Value = serde_json::from_slice(bytes).unwrap();
        let payloads = body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|message| message["content"].as_str())
            .filter_map(|text| serde_json::from_str::<Value>(text).ok())
            .collect::<Vec<_>>();
        let scope = payloads
            .iter()
            .rev()
            .find_map(|value| value["wire_scope"].as_str())
            .unwrap();
        if let Some(packet) = payloads
            .iter()
            .find(|value| value["reading_packs"].is_array())
        {
            let session = &packet["reading_packs"][0];
            let dependency = &session["pack"]["read_dependencies"][0];
            if dependency.is_object() {
                let reads = payloads
                    .iter()
                    .filter(|value| value["result"]["status"] == "read_pending_delivery")
                    .collect::<Vec<_>>();
                let read = reads.last();
                if read.is_none() {
                    return Ok(ChatTurn {
                        content: String::new(),
                        finish_reason: "tool_calls".into(),
                        usage: None,
                        tool_calls: vec![knowledge::models::ChatToolCall {
                            id: "read-pricing".into(),
                            name: "read_evidence".into(),
                            arguments: json!({"wire_scope":scope,"dependency_id":dependency["relation"]["id"]}).to_string(),
                        }],
                    });
                }
                let read = &read.unwrap()["result"]["read"];
                let excerpts = read["excerpts"].as_array().unwrap();
                let observations = body["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|message| message["role"] == "assistant")
                    .map(|message| message["content"].to_string())
                    .collect::<Vec<_>>()
                    .join("\n");
                let percentage = observations.contains("Observed percentage: 15%")
                    || excerpts
                        .iter()
                        .any(|e| e["quote"].as_str().is_some_and(|q| q.contains("15%")));
                let currency = observations.contains("Observed currency: 1,234.50元")
                    || excerpts.iter().any(|e| {
                        e["quote"]
                            .as_str()
                            .is_some_and(|q| q.contains("1,234.50元"))
                    });
                let formula = observations.contains("Observed formula cache explicitly missing")
                    || payloads
                        .iter()
                        .flat_map(|payload| {
                            payload["table_structures"].as_array().into_iter().flatten()
                        })
                        .filter(|table| {
                            excerpts
                                .iter()
                                .any(|excerpt| excerpt["locator"]["table_id"] == table["table_id"])
                        })
                        .any(|table| {
                            table["physical_locator"]["cells"]
                                .as_array()
                                .is_some_and(|cells| {
                                    cells.iter().any(|cell| {
                                        cell["formula"] == "=C1*(1+A1)"
                                            && cell["display_incomplete_reason"]
                                                == "formula_cached_value_missing"
                                    })
                                })
                        });
                if percentage && currency && formula {
                    self.saw_attachment.store(true, Ordering::SeqCst);
                }
                let note = [
                    if percentage {
                        "Observed percentage: 15%."
                    } else {
                        ""
                    },
                    if currency {
                        "Observed currency: 1,234.50元."
                    } else {
                        ""
                    },
                    if formula {
                        "Observed formula cache explicitly missing."
                    } else {
                        ""
                    },
                ]
                .join(" ");
                if let Some(cursor) = read["next_cursor"].as_str() {
                    let generation = payloads
                        .iter()
                        .find_map(|value| value["generation"].as_u64())
                        .unwrap();
                    return Ok(ChatTurn {
                        content: note,
                        finish_reason: "tool_calls".into(),
                        usage: None,
                        tool_calls: vec![knowledge::models::ChatToolCall {
                            id: format!("read-pricing-{generation}"),
                            name: "read_evidence".into(),
                            arguments: json!({"wire_scope":scope,"cursor":cursor}).to_string(),
                        }],
                    });
                }
                assert_eq!(read["selection_complete"], true);
                assert!(
                    percentage && currency && formula,
                    "percentage={percentage} currency={currency} formula={formula}; read={read}; observations={observations}"
                );
                let supports = session["pack"]["condition_support_options"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .chain(read["condition_support_options"].as_array().unwrap().iter())
                    .map(|option| json!({"support_key":option["support_key"]}))
                    .collect::<Vec<_>>();
                let mut response = self.cycle.discover.turn(config, bytes).await?;
                let atom = session["pack"]["atoms"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|atom| {
                        atom["excerpts"]
                            .as_array()
                            .is_some_and(|excerpts| !excerpts.is_empty())
                    })
                    .unwrap();
                let mut args: Value =
                    serde_json::from_str(&response.tool_calls[0].arguments).unwrap();
                args["requirements"] = json!([{"description":"Complete the prescribed pricing schedule using 15% and the stated currency; preserve unavailable formula results.","kind":"technical","obligation_strength":"mandatory","extraction_quality":"explicit","source_section_id":{"atom_key":atom["atom_key"]},"evidence":[{"evidence_key":atom["excerpts"][0]["evidence_key"]}],"condition_support":supports}]);
                args["inspected_atom_ids"] = json!([]);
                args["no_requirement_reason"] = Value::Null;
                response.tool_calls[0].arguments = args.to_string();
                return Ok(response);
            }
            return self.cycle.discover.turn(config, bytes).await;
        }
        // Bind native forms using only the visible host's current identities.
        if self.cycle.step.load(Ordering::SeqCst) == 1 && self.form_step.load(Ordering::SeqCst) < 2
        {
            if self.form_step.fetch_add(1, Ordering::SeqCst) == 0 {
                return Ok(ChatTurn {
                    content: String::new(),
                    finish_reason: "tool_calls".into(),
                    usage: None,
                    tool_calls: vec![knowledge::models::ChatToolCall {
                        id: "read-pricing-forms".into(),
                        name: "read_outline".into(),
                        arguments: json!({"wire_scope":scope,"mode":"forms"}).to_string(),
                    }],
                });
            }
            let forms = payloads
                .iter()
                .rev()
                .find_map(|value| value["result"]["items"].as_array())
                .expect("form directory in actual request");
            return Ok(ChatTurn {content:String::new(),finish_reason:"tool_calls".into(),usage:None,
                tool_calls:vec![knowledge::models::ChatToolCall {id:"bind-pricing".into(),name:"bind_forms".into(),arguments:json!({"wire_scope":scope,"mode":"replace","bindings":forms.iter().map(|form|json!({"form_id":form["form_id"],"chapter_id":"synthetic"})).collect::<Vec<_>>()}).to_string()}]});
        }
        self.cycle.turn(config, bytes).await
    }
}

#[tokio::test]
async fn production_related_read_reaches_condition_support_and_fresh_check() {
    let input = crate::outline::frozen::tests::related_native_input();
    let config = crate::analysis::tests::config();
    let mut state = super::super::retirement::checkpoint(&input);
    state.turn = 0;
    state.config_sha256 = digest(&config).unwrap();
    state.outline_run.reading_packs = Some(
        crate::outline::discover::DiscoverWork::plan_with_budget(&input, &|sessions| {
            Ok(sessions
                .iter()
                .all(|session| session["pack"]["document_ids"].as_array().unwrap().len() == 1))
        }),
    );
    let journal = CycleJournal {
        state: Mutex::new(state),
        stop_check: AtomicBool::new(false),
        stop_repair: AtomicBool::new(false),
    };
    let model = RelatedCycleModel {
        cycle: CycleModel {
            discover: Delayed::default(),
            step: AtomicUsize::new(0),
            automatic: true,
            saw_repair_wire: AtomicBool::new(true),
        },
        saw_attachment: AtomicBool::new(false),
        form_step: AtomicUsize::new(0),
    };
    super::super::run(&input, &config, &journal, &model, &CancellationToken::new())
        .await
        .unwrap();
    let final_state = journal.load().await.unwrap().unwrap();
    assert!(model.saw_attachment.load(Ordering::SeqCst));
    assert!(final_state.done);
    assert!(final_state.outline_run.tool_draft.finished);
    let work = final_state.outline_run.reading_packs.as_ref().unwrap();
    assert_eq!(work.requirement_records().len(), 1);
    let requirement = work.requirement_records().values().next().unwrap();
    assert!(requirement.condition_support.len() > 1);
    assert!(matches!(
        requirement.condition_support[0].origin,
        crate::outline::discover::ConditionSupportOrigin::ConfirmedRelation { .. }
    ));
    assert!(
        !final_state
            .outline_run
            .tool_draft
            .check_reads
            .evidence
            .is_empty()
    );
    let mut changed_set = input.clone();
    changed_set.document_set_id.push_str("-replacement");
    assert!(
        crate::outline::tools::template_ready(&changed_set, &final_state.outline_run.tool_draft)
            .is_err()
    );
}

struct LongRelatedModel {
    submit: Delayed,
    pages: Mutex<Vec<(String, String)>>,
}
#[async_trait]
impl Model for LongRelatedModel {
    async fn turn(&self, config: &Config, bytes: &[u8]) -> Result<ChatTurn, AgentError> {
        let body: Value = serde_json::from_slice(bytes).unwrap();
        let count = crate::agent_runtime::chat::estimate_request_tokens_with_reserve(
            &body,
            &config.limits.tokenizer,
            config.limits.image_token_reserve,
            config.limits.token_safety_margin,
            config.provider.output_token_reserve as usize,
        )
        .unwrap();
        assert!(count.total_context_tokens <= config.limits.max_context_tokens);
        let payloads = body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|message| message["content"].as_str())
            .filter_map(|text| serde_json::from_str::<Value>(text).ok())
            .collect::<Vec<_>>();
        let scope = payloads
            .iter()
            .rev()
            .find_map(|value| value["wire_scope"].as_str())
            .unwrap();
        let generation = payloads
            .iter()
            .find_map(|value| value["generation"].as_u64())
            .unwrap();
        let session = &payloads
            .iter()
            .find(|value| value["reading_packs"].is_array())
            .unwrap()["reading_packs"][0];
        if session["pack"]["read_dependencies"]
            .as_array()
            .is_some_and(|values| !values.is_empty())
        {
            for message in body["messages"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|message| message["role"] == "tool")
            {
                let value: Value =
                    serde_json::from_str(message["content"].as_str().unwrap()).unwrap();
                assert!(value["ok"] != false, "read failed: {value}");
                if value["result"]["status"] == "read_pending_delivery" {
                    let id = message["tool_call_id"].as_str().unwrap().to_string();
                    let quote = value["result"]["read"]["excerpts"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|excerpt| excerpt["quote"].as_str().unwrap())
                        .collect::<String>();
                    let mut pages = self.pages.lock().unwrap();
                    if !pages.iter().any(|(seen, _)| seen == &id) {
                        pages.push((id, quote));
                    }
                }
            }
            let read = payloads
                .iter()
                .rev()
                .find(|value| value["result"]["status"] == "read_pending_delivery");
            let args = if let Some(read) = read {
                read["result"]["read"]["next_cursor"]
                    .as_str()
                    .map(|cursor| json!({"wire_scope":scope,"cursor":cursor}))
            } else {
                Some(
                    json!({"wire_scope":scope,"refs":[session["pack"]["read_dependencies"][0]["available_originals"][0]["evidence"]]}),
                )
            };
            if let Some(args) = args {
                return Ok(ChatTurn {
                    content: "Keep reading the linked source; only delivered exact ranges count."
                        .into(),
                    finish_reason: "tool_calls".into(),
                    usage: None,
                    tool_calls: vec![knowledge::models::ChatToolCall {
                        id: format!("long-read-{generation}"),
                        name: "read_evidence".into(),
                        arguments: args.to_string(),
                    }],
                });
            }
        }
        self.submit.turn(config, bytes).await
    }
}

#[tokio::test]
async fn production_dependency_over_small_token_window_preserves_every_utf8_byte() {
    use crate::analysis::source_manifest::*;
    let text =
        "条件😀：金额百分比和截止时间必须逐段保留。Use linked source synthetic-1.\n".repeat(700);
    let mut input =
        super::super::token_transport_validation::synthetic_input_with_text(text.clone(), 2);
    input.document_relations.push(DocumentRelation {
        id: "long-attachment".into(),
        from: RelationEndpoint {
            document_id: "synthetic-0".into(),
            unit_id: None,
        },
        to: Some(RelationEndpoint {
            document_id: "synthetic-1".into(),
            unit_id: None,
        }),
        kind: DocumentRelationKind::ExplicitReference,
        status: DocumentRelationStatus::Confirmed,
        required: true,
        basis: "Use linked source synthetic-1.".into(),
        locator: json!({"unit_id":"section"}),
    });
    let mut config = crate::analysis::tests::config();
    let mut state = super::super::retirement::checkpoint(&input);
    state.turn = 0;
    state.outline_run.reading_packs = Some(
        crate::outline::discover::DiscoverWork::plan_with_budget(&input, &|sessions| {
            Ok(sessions
                .iter()
                .all(|session| session["pack"]["document_ids"].as_array().unwrap().len() == 1))
        }),
    );
    state.outline_run.reading_packs.as_mut().unwrap().claim(2);
    state
        .outline_run
        .discover_workers
        .insert("pack-0".into(), Worker::default());
    let journal = Durable {
        state: Mutex::new(state.clone()),
        stop_barrier: true,
        crash_received: AtomicBool::new(false),
        call_limit: 50,
    };
    let cancel = CancellationToken::new();
    let mut probe_state = state.clone();
    let mut probe = PackHost {
        input: &input,
        config: &config,
        state: &mut probe_state,
        journal: &journal,
        cancel: &cancel,
    };
    let baseline = probe.prepare_inner("pack-0", false).await.unwrap();
    config.limits.max_context_tokens =
        baseline.input_reserve as usize + baseline.output_reserve as usize + 4096;
    state.config_sha256 = digest(&config).unwrap();
    *journal.state.lock().unwrap() = state;
    let model = LongRelatedModel {
        submit: Delayed::default(),
        pages: Mutex::new(Vec::new()),
    };
    let outcome = super::super::run(&input, &config, &journal, &model, &cancel)
        .await
        .unwrap_err();
    assert_eq!(outcome.code, "TEST_ALL_PACKS_BARRIER");
    {
        let pages = model.pages.lock().unwrap();
        assert!(
            pages.len() > 2,
            "fixture must cross multiple token-admitted pages"
        );
        assert_eq!(
            pages
                .iter()
                .map(|(_, quote)| quote.as_str())
                .collect::<String>(),
            text
        );
    }
    let restored = journal.load().await.unwrap().unwrap();
    assert!(
        restored
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .complete()
    );
    assert!(
        restored.outline_run.discover_workers["pack-0"]
            .related_continuations
            .is_empty()
    );
}

#[test]
fn related_read_credit_rejects_unknown_stale_foreign_and_modified_requests_atomically() {
    use crate::outline::evidence::{EvidenceRef, input_digest};
    let input = crate::outline::frozen::tests::related_native_input();
    let mut state = super::super::retirement::checkpoint(&input);
    let mut work = crate::outline::discover::DiscoverWork::plan_with_budget(&input, &|sessions| {
        Ok(sessions
            .iter()
            .all(|session| session["pack"]["document_ids"].as_array().unwrap().len() == 1))
    });
    work.claim(2);
    let id = "pack-0";
    let identity = work.pack_wire_identity(id).unwrap();
    let session = work.session(&input, id).unwrap();
    let reference: EvidenceRef = serde_json::from_value(
        session["pack"]["read_dependencies"][0]["available_originals"][0]["evidence"].clone(),
    )
    .unwrap();
    let (page, options) = work.related_read(&input, id, &[reference]).unwrap();
    let body = json!({"messages":[{"role":"tool","tool_call_id":"dependent-original","content":page.to_string()}]});
    let request = Request {
        scope: Scope {
            run: identity["run"].as_str().unwrap().into(),
            pack: id.into(),
            revision: 1,
            generation: 1,
            request: "related-request".into(),
        },
        body: serde_json::to_vec(&body).unwrap(),
        input_reserve: 1,
        output_reserve: 1,
    };
    let response = json!({"content":"","finish_reason":"tool_calls","usage":null,"tool_calls":[{"id":"submit","name":"submit_pack","arguments":"{}"}]});
    let frame = coordinator::RelatedReadFrame {
        directory_page: None,
        call_id: "dependent-original".into(),
        pack_revision: 1,
        input_digest: input_digest(&input).unwrap(),
        options: options.clone(),
        sealed_sha256: crate::outline::read_receipts::wire_hash(&body, "dependent-original")
            .unwrap(),
    };
    state.outline_run.reading_packs = Some(work);
    state.outline_run.discover_workers.insert(
        id.into(),
        Worker {
            pending: Some(Pending::Received {
                request: request.clone(),
                response: response.clone(),
            }),
            related_read_frames: vec![frame],
            ..Default::default()
        },
    );
    for mode in [
        "unknown",
        "incomplete",
        "modified",
        "missing",
        "run",
        "revision",
        "input",
        "foreign",
        "atomic",
    ] {
        let mut candidate = state.clone();
        let mut req = request.clone();
        match mode {
            "unknown" => {
                candidate
                    .outline_run
                    .discover_workers
                    .get_mut(id)
                    .unwrap()
                    .pending = Some(Pending::Sending {
                    request: req.clone(),
                })
            }
            "incomplete" => {
                candidate
                    .outline_run
                    .discover_workers
                    .get_mut(id)
                    .unwrap()
                    .pending = Some(Pending::Received {
                    request: req.clone(),
                    response: json!({"content":"","finish_reason":"length","usage":null,"tool_calls":[]}),
                })
            }
            "modified" | "missing" => {
                let mut value = body.clone();
                if mode == "modified" {
                    value["messages"][0]["content"] = json!("changed");
                } else {
                    value["messages"] = json!([]);
                }
                req.body = serde_json::to_vec(&value).unwrap();
                candidate
                    .outline_run
                    .discover_workers
                    .get_mut(id)
                    .unwrap()
                    .pending = Some(Pending::Received {
                    request: req.clone(),
                    response: response.clone(),
                });
            }
            "run" | "revision" => {
                if mode == "run" {
                    req.scope.run.push('x');
                } else {
                    req.scope.revision += 1;
                }
                candidate
                    .outline_run
                    .discover_workers
                    .get_mut(id)
                    .unwrap()
                    .pending = Some(Pending::Received {
                    request: req.clone(),
                    response: response.clone(),
                });
            }
            "input" => candidate
                .outline_run
                .discover_workers
                .get_mut(id)
                .unwrap()
                .related_read_frames[0]
                .input_digest
                .push('x'),
            "foreign" => {
                req.scope.pack = "pack-1".into();
                let mut worker = candidate.outline_run.discover_workers[id].clone();
                worker.pending = Some(Pending::Received {
                    request: req.clone(),
                    response: response.clone(),
                });
                candidate
                    .outline_run
                    .discover_workers
                    .insert("pack-1".into(), worker);
            }
            "atomic" => {
                let worker = candidate.outline_run.discover_workers.get_mut(id).unwrap();
                let mut invalid = worker.related_read_frames[0].clone();
                invalid.input_digest.push('x');
                worker.related_read_frames.push(invalid);
            }
            _ => unreachable!(),
        }
        let before = json!(candidate.outline_run.reading_packs);
        let result = confirm_worker_related_reads(&input, &mut candidate, &req);
        assert_eq!(
            json!(candidate.outline_run.reading_packs),
            before,
            "{mode}: unauthorized credit"
        );
        if !matches!(mode, "modified" | "missing") {
            assert!(result.is_err(), "{mode}");
        }
    }
    confirm_worker_related_reads(&input, &mut state, &request).unwrap();
    assert!(
        state
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .related_read_already_delivered(id, &options)
    );
    assert!(
        state.outline_run.discover_workers[id]
            .related_read_frames
            .is_empty()
    );
    assert!(
        !state
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .related_read_already_delivered("pack-1", &options)
    );
    assert!(state.outline_run.tool_draft.check_reads.evidence.is_empty());
}

#[tokio::test]
async fn dependency_cursor_cannot_change_worker_input_revision_or_selection() {
    let input = crate::outline::frozen::tests::related_native_input();
    let config = crate::analysis::tests::config();
    let mut state = super::super::retirement::checkpoint(&input);
    let mut work = crate::outline::discover::DiscoverWork::plan_with_budget(&input, &|sessions| {
        Ok(sessions
            .iter()
            .all(|session| session["pack"]["document_ids"].as_array().unwrap().len() == 1))
    });
    work.claim(2);
    let identity = work.pack_wire_identity("pack-0").unwrap();
    let selected = work
        .dependency_selection(&input, "pack-0", "pricing-reference")
        .unwrap();
    let continuation = coordinator::RelatedContinuation {
        run: identity["run"].as_str().unwrap().into(),
        pack: "pack-0".into(),
        pack_revision: 1,
        input_digest: crate::outline::evidence::input_digest(&input).unwrap(),
        refs: vec![selected[0].clone()],
    };
    let cursor = format!(
        "page_{}",
        crate::outline::canonical_sha256(&continuation).unwrap()
    );
    state.outline_run.reading_packs = Some(work);
    state.outline_run.discover_workers.insert(
        "pack-0".into(),
        Worker {
            generation: 1,
            related_continuations: std::collections::BTreeMap::from([(
                cursor.clone(),
                continuation,
            )]),
            ..Default::default()
        },
    );
    let mut request = Request {
        scope: Scope {
            run: identity["run"].as_str().unwrap().into(),
            pack: "pack-0".into(),
            revision: 1,
            generation: 1,
            request: "cursor-read".into(),
        },
        body: vec![],
        input_reserve: 1,
        output_reserve: 1,
    };
    request.scope.request =
        crate::outline::canonical_sha256(&(&identity, request.scope.generation, &request.body))
            .unwrap();
    validate_worker_scope(&input, &state, &request).unwrap();
    let response = ChatTurn {
        content: String::new(),
        finish_reason: "tool_calls".into(),
        usage: None,
        tool_calls: vec![knowledge::models::ChatToolCall {
            id: "cursor".into(),
            name: "read_evidence".into(),
            arguments: "{}".into(),
        }],
    };
    let journal = Durable {
        state: Mutex::new(state.clone()),
        stop_barrier: false,
        crash_received: AtomicBool::new(false),
        call_limit: 0,
    };
    let cancel = CancellationToken::new();
    let mut control = state.clone();
    let read = PackHost {
        input: &input,
        config: &config,
        state: &mut control,
        journal: &journal,
        cancel: &cancel,
    }
    .related_page(&request, &response, &json!({"cursor":cursor}))
    .await
    .unwrap();
    assert_eq!(read["status"], "read_pending_delivery");
    for mode in ["tampered-key", "run", "worker", "revision", "input"] {
        let mut candidate = state.clone();
        let mut key = cursor.clone();
        if mode == "tampered-key" {
            key.push('x');
        } else {
            let saved = candidate
                .outline_run
                .discover_workers
                .get_mut("pack-0")
                .unwrap()
                .related_continuations
                .get_mut(&key)
                .unwrap();
            match mode {
                "run" => saved.run.push('x'),
                "worker" => saved.pack = "pack-1".into(),
                "revision" => saved.pack_revision += 1,
                "input" => saved.input_digest.push('x'),
                _ => unreachable!(),
            }
        }
        let before = json!(candidate.outline_run.discover_workers);
        let mut host = PackHost {
            input: &input,
            config: &config,
            state: &mut candidate,
            journal: &journal,
            cancel: &cancel,
        };
        assert!(
            host.related_page(&request, &response, &json!({"cursor":key}))
                .await
                .is_err(),
            "{mode}"
        );
        assert_eq!(json!(candidate.outline_run.discover_workers), before);
    }
}

#[tokio::test]
async fn related_read_issued_keys_survive_peer_commit_but_not_input_change() {
    let input = crate::outline::frozen::tests::related_native_input();
    let config = crate::analysis::tests::config();
    let mut state = super::super::retirement::checkpoint(&input);
    state.turn = 0;
    state.config_sha256 = digest(&config).unwrap();
    state.outline_run.reading_packs = Some(
        crate::outline::discover::DiscoverWork::plan_with_budget(&input, &|sessions| {
            Ok(sessions
                .iter()
                .all(|s| s["pack"]["document_ids"].as_array().unwrap().len() == 1))
        }),
    );
    state.outline_run.reading_packs.as_mut().unwrap().claim(2);
    for id in ["pack-0", "pack-1"] {
        state
            .outline_run
            .discover_workers
            .insert(id.into(), Worker::default());
    }
    let journal = Durable {
        state: Mutex::new(state.clone()),
        stop_barrier: false,
        crash_received: AtomicBool::new(false),
        call_limit: 99,
    };
    let cancel = CancellationToken::new();
    let mut host = PackHost {
        input: &input,
        config: &config,
        state: &mut state,
        journal: &journal,
        cancel: &cancel,
    };
    let a = host.prepare_inner("pack-0", false).await.unwrap();
    let b = host.prepare_inner("pack-1", false).await.unwrap();
    host.reserve(&a).await.unwrap();
    host.reserve(&b).await.unwrap();
    let reader = RelatedCycleModel {
        cycle: CycleModel {
            discover: Delayed::default(),
            step: AtomicUsize::new(0),
            automatic: true,
            saw_repair_wire: AtomicBool::new(true),
        },
        saw_attachment: AtomicBool::new(false),
        form_step: AtomicUsize::new(0),
    };
    let response_a = reader.turn(&config, &a.body).await.unwrap();
    assert_eq!(response_a.tool_calls[0].name, "read_evidence");
    let response_b = Delayed::default().turn(&config, &b.body).await.unwrap();
    let old_revision = host
        .state
        .outline_run
        .reading_packs
        .as_ref()
        .unwrap()
        .revision;
    host.received(&b, &response_b).await.unwrap();
    host.commit(&b, response_b).await.unwrap();
    assert!(
        host.state
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .revision
            > old_revision
    );
    host.received(&a, &response_a).await.unwrap();
    let mut changed = host.state.clone();
    host.commit(&a, response_a.clone()).await.unwrap();
    let worker = &host.state.outline_run.discover_workers["pack-0"];
    assert!(worker.pending.is_none(), "{:?}", worker.pending);
    assert!(!worker.related_read_frames.is_empty());
    assert_eq!(worker.rejected_turns, 0);
    for dependency_change in ["source", "relation"] {
        let mut changed_input = input.clone();
        if dependency_change == "source" {
            let form = changed_input
                .structured_forms
                .iter_mut()
                .find(|form| {
                    form["definition"]["cells"]
                        .as_array()
                        .is_some_and(|cells| !cells.is_empty())
                })
                .unwrap();
            form["definition"]["cells"][0]["text"] = json!("different dependency source");
        } else {
            changed_input.document_relations[0].status =
                crate::analysis::source_manifest::DocumentRelationStatus::Unconfirmed;
        }
        let mut candidate = changed.clone();
        let mut stale = PackHost {
            input: &changed_input,
            config: &config,
            state: &mut candidate,
            journal: &journal,
            cancel: &cancel,
        };
        stale.commit(&a, response_a.clone()).await.unwrap();
        assert!(
            matches!(
                stale.state.outline_run.discover_workers["pack-0"].pending,
                Some(Pending::Failed { .. })
            ),
            "{dependency_change}"
        );
        assert!(
            stale.state.outline_run.discover_workers["pack-0"]
                .related_read_frames
                .is_empty()
        );
    }
    let mut reopened = changed.clone();
    let submission = Delayed::default().turn(&config, &a.body).await.unwrap();
    reopened
        .outline_run
        .discover_workers
        .get_mut("pack-0")
        .unwrap()
        .pending = Some(Pending::Received {
        request: a.clone(),
        response: serde_json::to_value(&submission).unwrap(),
    });
    {
        let mut owner = PackHost {
            input: &input,
            config: &config,
            state: &mut reopened,
            journal: &journal,
            cancel: &cancel,
        };
        owner.commit(&a, submission).await.unwrap();
    }
    reopened
        .outline_run
        .reading_packs
        .as_mut()
        .unwrap()
        .reopen_committed(
            &input,
            "pack-0",
            a.scope.revision,
            "reopen-owned-source-key",
        )
        .unwrap();
    reopened
        .outline_run
        .discover_workers
        .get_mut("pack-0")
        .unwrap()
        .pending = Some(Pending::Received {
        request: a.clone(),
        response: serde_json::to_value(&response_a).unwrap(),
    });
    {
        let mut owner = PackHost {
            input: &input,
            config: &config,
            state: &mut reopened,
            journal: &journal,
            cancel: &cancel,
        };
        owner.commit(&a, response_a.clone()).await.unwrap();
        assert!(matches!(
            owner.state.outline_run.discover_workers["pack-0"].pending,
            Some(Pending::Failed { .. })
        ));
    }
    changed.input_sha256.push('x');
    let mut stale_host = PackHost {
        input: &input,
        config: &config,
        state: &mut changed,
        journal: &journal,
        cancel: &cancel,
    };
    stale_host.commit(&a, response_a).await.unwrap();
    assert!(matches!(
        stale_host.state.outline_run.discover_workers["pack-0"].pending,
        Some(Pending::Failed { .. })
    ));
    assert!(
        stale_host.state.outline_run.discover_workers["pack-0"]
            .related_read_frames
            .is_empty()
    );
}

struct MixedNativeDiscoveryModel {
    fallback: Delayed,
    image_requests: AtomicUsize,
    word_grid_requests: AtomicUsize,
    expected_image_urls: BTreeSet<String>,
}
#[async_trait]
impl Model for MixedNativeDiscoveryModel {
    async fn turn(&self, config: &Config, bytes: &[u8]) -> Result<ChatTurn, AgentError> {
        let body: Value = serde_json::from_slice(bytes).unwrap();
        let packet = body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|m| m["content"].as_str())
            .filter_map(|s| serde_json::from_str::<Value>(s).ok())
            .find(|p| p["reading_packs"].is_array())
            .unwrap();
        let session = &packet["reading_packs"][0];
        let atoms = session["pack"]["atoms"].as_array().unwrap();
        let pixels = body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|m| m["content"].as_array())
            .flatten()
            .filter(|part| part["type"] == "image_url")
            .count();
        let mut requirements = Vec::new();
        for atom in atoms {
            let image = atom["carrier"]["kind"] == "image";
            let entries = if atom["cells"].is_array() {
                atom["cells"].as_array()
            } else {
                atom["excerpts"].as_array()
            };
            let Some(entries) = entries else { continue };
            let selected = if image {
                entries.first()
            } else {
                entries.iter().find(|entry| {
                    let text = entry["quote"]
                        .as_str()
                        .or_else(|| entry["text"].as_str())
                        .unwrap_or("");
                    text.contains("Tender requirements:")
                        || text.contains("Keep this fixed wording.")
                        || text.contains("Bidder name:")
                        || text.contains("Authorized signature:")
                        || (image && text.contains("Signed schedule required"))
                })
            };
            if let Some(entry) = selected {
                if image {
                    assert!(
                        pixels > 0,
                        "OCR fixture text must not substitute for original pixels"
                    );
                    let urls = body["messages"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter_map(|m| m["content"].as_array())
                        .flatten()
                        .filter_map(|part| part["image_url"]["url"].as_str());
                    assert!(
                        urls.into_iter()
                            .all(|url| self.expected_image_urls.contains(url))
                    );
                    self.image_requests.fetch_add(1, Ordering::SeqCst);
                }
                if atom["carrier"]["kind"] == "grid" {
                    self.word_grid_requests.fetch_add(1, Ordering::SeqCst);
                }
                // This mock response is a fixture oracle, not a visual/OCR accuracy claim.
                let quote = if image {
                    "Signed schedule required"
                } else {
                    entry["quote"]
                        .as_str()
                        .or_else(|| entry["text"].as_str())
                        .unwrap()
                };
                requirements.push(json!({"description":quote,"kind":"technical","obligation_strength":"mandatory","extraction_quality":"explicit",
                    "source_section_id":{"atom_key":atom["atom_key"]},"evidence":[{"evidence_key":entry["evidence_key"]}],"condition_support":[]}));
            }
        }
        let mut response = self.fallback.turn(config, bytes).await?;
        if !requirements.is_empty() {
            let mut args: Value = serde_json::from_str(&response.tool_calls[0].arguments).unwrap();
            args["requirements"] = json!(requirements);
            args["no_requirement_reason"] = Value::Null;
            args["inspected_atom_ids"] = json!([]);
            response.tool_calls[0].arguments = args.to_string();
        }
        Ok(response)
    }
}

fn mixed_native_runtime_fixture() -> (FrozenInput, Config, Checkpoint, BTreeSet<String>) {
    let input = crate::outline::frozen::tests::mixed_native_input();
    let (build, _) = crate::outline::frozen::tests::mixed_native_build();
    let config = crate::analysis::tests::config();
    let mut state = super::super::retirement::checkpoint(&input);
    state.turn = 0;
    state.config_sha256 = digest(&config).unwrap();
    state.outline_run.reading_packs = Some(
        crate::outline::discover::DiscoverWork::plan_with_budget(&input, &|sessions| {
            Ok(sessions
                .iter()
                .all(|s| s["pack"]["document_ids"].as_array().unwrap().len() == 1))
        }),
    );
    for source in input
        .source_units
        .iter()
        .filter(|s| s.locator["kind"] == "image_region")
    {
        let document = build
            .documents
            .iter()
            .find(|d| d.document_id == source.document_id)
            .unwrap();
        let image = document
            .parsed
            .images
            .iter()
            .find(|image| image.original_ref == source.locator["original_ref"])
            .unwrap();
        let digest = source.locator["image_ref"]
            .as_str()
            .unwrap()
            .strip_prefix("objects/")
            .unwrap();
        let view = crate::outline::visual::render_frozen_image_view(
            &source.source_unit_revision_id,
            digest,
            0,
            &image.data,
            config.limits.max_source_view_edge,
            config.limits.max_source_view_bytes,
        )
        .unwrap();
        state.source_views.insert(view.id().unwrap(), view);
    }
    let expected_image_urls = state
        .source_views
        .values()
        .map(|view| format!("data:image/jpeg;base64,{}", view.jpeg_base64))
        .collect();
    (input, config, state, expected_image_urls)
}

#[tokio::test]
async fn mixed_native_discovery_transports_word_grid_and_original_image_regions() {
    let (input, config, state, expected_image_urls) = mixed_native_runtime_fixture();
    let journal = Durable {
        state: Mutex::new(state),
        stop_barrier: true,
        crash_received: AtomicBool::new(false),
        call_limit: 20,
    };
    let model = MixedNativeDiscoveryModel {
        fallback: Delayed::default(),
        image_requests: AtomicUsize::new(0),
        word_grid_requests: AtomicUsize::new(0),
        expected_image_urls,
    };
    let outcome = super::super::run(&input, &config, &journal, &model, &CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(
        outcome.code, "TEST_ALL_PACKS_BARRIER",
        "{}",
        outcome.message
    );
    assert_eq!(model.image_requests.load(Ordering::SeqCst), 2);
    assert_eq!(model.word_grid_requests.load(Ordering::SeqCst), 1);
    let saved = journal.state.lock().unwrap();
    let records = saved
        .outline_run
        .reading_packs
        .as_ref()
        .unwrap()
        .requirement_records();
    let evidence = records
        .values()
        .flat_map(|record| &record.evidence)
        .collect::<Vec<_>>();
    assert_eq!(
        evidence
            .iter()
            .filter(|e| matches!(e, crate::outline::evidence::EvidenceRef::ImageRegion { .. }))
            .count(),
        2
    );
    assert!(
        evidence
            .iter()
            .any(|e| matches!(e, crate::outline::evidence::EvidenceRef::GridCell { .. }))
    );
}

#[tokio::test]
async fn mixed_native_related_image_requires_exact_pixels_identity_and_completed_request() {
    let (input, config, mut state, _) = mixed_native_runtime_fixture();
    state.outline_run.reading_packs.as_mut().unwrap().claim(1);
    state
        .outline_run
        .discover_workers
        .insert("pack-0".into(), Worker::default());
    let journal = Durable {
        state: Mutex::new(state.clone()),
        stop_barrier: false,
        crash_received: AtomicBool::new(false),
        call_limit: 20,
    };
    let cancel = CancellationToken::new();
    let mut host = PackHost {
        input: &input,
        config: &config,
        state: &mut state,
        journal: &journal,
        cancel: &cancel,
    };
    let mut request = host.prepare_inner("pack-0", false).await.unwrap();
    let (scope, dependency_id) = loop {
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        let payloads = body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|m| m["content"].as_str())
            .filter_map(|s| serde_json::from_str::<Value>(s).ok())
            .collect::<Vec<_>>();
        let scope = payloads
            .iter()
            .find_map(|p| p["wire_scope"].as_str())
            .unwrap()
            .to_owned();
        let directory = payloads
            .iter()
            .rev()
            .find_map(|p| {
                p["result"]["read"]["read_dependencies"]
                    .is_array()
                    .then_some(&p["result"]["read"])
            })
            .unwrap_or_else(|| {
                &payloads
                    .iter()
                    .find(|p| p["reading_packs"].is_array())
                    .unwrap()["reading_packs"][0]["pack"]
            });
        if let Some(row) = directory["read_dependencies"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| {
                row["available_originals"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|r| r["requires_original_pixels"] == true)
            })
        {
            break (scope, row["relation"]["id"].as_str().unwrap().to_owned());
        }
        let cursor = directory["next_dependency_cursor"]
            .as_str()
            .expect("image remains in directory");
        let turn = ChatTurn {
            content: String::new(),
            finish_reason: "tool_calls".into(),
            usage: None,
            tool_calls: vec![knowledge::models::ChatToolCall {
                id: format!("directory-{}", request.scope.generation),
                name: "read_evidence".into(),
                arguments: json!({"wire_scope":scope,"dependency_cursor":cursor}).to_string(),
            }],
        };
        host.reserve(&request).await.unwrap();
        host.received(&request, &turn).await.unwrap();
        host.commit(&request, turn).await.unwrap();
        request = host.prepare_inner("pack-0", false).await.unwrap();
    };
    let response = ChatTurn {
        content: "Request original fixture pixels; OCR stub is not visual evidence.".into(),
        finish_reason: "tool_calls".into(),
        usage: None,
        tool_calls: vec![knowledge::models::ChatToolCall {
            id: "read-original-image".into(),
            name: "read_evidence".into(),
            arguments: json!({"wire_scope":scope,"dependency_id":dependency_id}).to_string(),
        }],
    };
    host.reserve(&request).await.unwrap();
    host.received(&request, &response).await.unwrap();
    host.commit(&request, response).await.unwrap();
    assert!(
        host.state.outline_run.discover_workers["pack-0"]
            .pending
            .is_none()
    );
    let delivered = host.prepare_inner("pack-0", false).await.unwrap();
    assert!(
        host.state.outline_run.discover_workers["pack-0"]
            .images
            .is_empty(),
        "dependency does not grant ownership of target pack images"
    );
    let next_response = Delayed::default()
        .turn(&config, &delivered.body)
        .await
        .unwrap();
    host.reserve(&delivered).await.unwrap();
    host.received(&delivered, &next_response).await.unwrap();
    let received = host.state.clone();
    for mode in [
        "missing_pixels",
        "dimensions",
        "original_sha",
        "region",
        "unknown_response",
    ] {
        let mut candidate = received.clone();
        let mut altered = delivered.clone();
        match mode {
            "missing_pixels" => {
                let mut wire: Value = serde_json::from_slice(&altered.body).unwrap();
                wire["messages"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|m| !m["content"].is_array());
                altered.body = serde_json::to_vec(&wire).unwrap();
                candidate
                    .outline_run
                    .discover_workers
                    .get_mut("pack-0")
                    .unwrap()
                    .pending = Some(Pending::Received {
                    request: altered.clone(),
                    response: serde_json::to_value(&next_response).unwrap(),
                });
            }
            "dimensions" => {
                for view in candidate.source_views.values_mut() {
                    view.identity.width += 1;
                }
            }
            "original_sha" => {
                for view in candidate.source_views.values_mut() {
                    view.identity.original_sha256 = "a".repeat(64);
                }
            }
            "region" => {
                let frame = &mut candidate
                    .outline_run
                    .discover_workers
                    .get_mut("pack-0")
                    .unwrap()
                    .related_read_frames[0];
                if let crate::outline::evidence::EvidenceRef::ImageRegion { region, .. } =
                    &mut frame.options[0].evidence[0]
                {
                    *region = "cropped".into();
                }
            }
            "unknown_response" => {
                candidate
                    .outline_run
                    .discover_workers
                    .get_mut("pack-0")
                    .unwrap()
                    .pending = Some(Pending::Sending {
                    request: altered.clone(),
                });
            }
            _ => unreachable!(),
        }
        let before = json!(candidate.outline_run.reading_packs);
        assert!(
            confirm_worker_related_reads(&input, &mut candidate, &altered).is_err(),
            "{mode}"
        );
        assert_eq!(json!(candidate.outline_run.reading_packs), before, "{mode}");
    }
    let mut confirmed = received;
    confirm_worker_related_reads(&input, &mut confirmed, &delivered).unwrap();
    let session = confirmed
        .outline_run
        .reading_packs
        .as_ref()
        .unwrap()
        .session(&input, "pack-0")
        .unwrap();
    assert_eq!(
        session["pack"]["condition_support_options"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        confirmed
            .outline_run
            .tool_draft
            .check_reads
            .evidence
            .is_empty()
    );
}

include!("discover_mixed_cycle_tests.rs");

struct ColdNativeImages {
    views: Vec<views::SourceView>,
}
#[async_trait]
impl Journal for ColdNativeImages {
    async fn load(&self) -> Result<Option<Checkpoint>, AgentError> {
        Ok(None)
    }
    async fn reserve(&self, _: &Checkpoint, _: &[u8]) -> Result<Option<usize>, AgentError> {
        Ok(Some(0))
    }
    async fn save(&self, _: &Checkpoint, _: &Value) -> Result<(), AgentError> {
        Ok(())
    }
    async fn source_view(
        &self,
        source_id: &str,
        _: &Limits,
        _: &CancellationToken,
    ) -> Result<views::SourceView, AgentError> {
        self.views
            .iter()
            .find(|view| view.identity.source_id == source_id)
            .cloned()
            .ok_or_else(|| invalid("fixture image missing"))
    }
}

#[tokio::test]
async fn mixed_native_cold_image_cache_preserves_occurrences_and_exact_identity_delivery() {
    let (input, config, mut state, _) = mixed_native_runtime_fixture();
    let journal = ColdNativeImages {
        views: state.source_views.values().cloned().collect(),
    };
    assert_eq!(journal.views.len(), 2);
    assert_eq!(
        journal.views[0].identity.image_sha256,
        journal.views[1].identity.image_sha256
    );
    assert_ne!(
        journal.views[0].identity.source_id,
        journal.views[1].identity.source_id
    );
    state.source_views.clear();
    let mut work = crate::outline::discover::DiscoverWork::plan_with_budget(&input, &|_| Ok(true));
    work.claim(1);
    state.outline_run.reading_packs = Some(work);
    state.outline_run.discover_workers.clear();
    state
        .outline_run
        .discover_workers
        .insert("pack-0".into(), Worker::default());
    let cancel = CancellationToken::new();
    let request = PackHost {
        input: &input,
        config: &config,
        state: &mut state,
        journal: &journal,
        cancel: &cancel,
    }
    .prepare_inner("pack-0", false)
    .await
    .unwrap();
    assert_eq!(
        state.source_views.len(),
        2,
        "same pixels must not collapse native occurrences"
    );
    for view in &journal.views {
        assert!(state.source_views.contains_key(&view.id().unwrap()));
    }
    let response = json!({"content":"","finish_reason":"tool_calls","usage":null,"tool_calls":[{"id":"submit","name":"submit_pack","arguments":"{}"}]});
    state
        .outline_run
        .discover_workers
        .get_mut("pack-0")
        .unwrap()
        .pending = Some(Pending::Received {
        request: request.clone(),
        response: response.clone(),
    });
    for mode in ["identity_removed", "identity_changed", "unknown_response"] {
        let mut altered = request.clone();
        let mut candidate = state.clone();
        let mut body: Value = serde_json::from_slice(&altered.body).unwrap();
        if mode == "identity_removed" {
            body["messages"]
                .as_array_mut()
                .unwrap()
                .retain(|message| message != &journal.views[0].message());
        }
        if mode == "identity_changed" {
            let message = body["messages"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|message| **message == journal.views[0].message())
                .unwrap();
            message["content"][0]["text"] = json!("same image, wrong source identity");
        }
        altered.body = serde_json::to_vec(&body).unwrap();
        candidate
            .outline_run
            .discover_workers
            .get_mut("pack-0")
            .unwrap()
            .pending = Some(if mode == "unknown_response" {
            Pending::Sending {
                request: altered.clone(),
            }
        } else {
            Pending::Received {
                request: altered.clone(),
                response: response.clone(),
            }
        });
        let before = json!(candidate.outline_run.reading_packs);
        assert!(
            confirm_worker_images(&input, &config, &mut candidate, &altered).is_err(),
            "{mode}"
        );
        assert_eq!(json!(candidate.outline_run.reading_packs), before, "{mode}");
    }
    confirm_worker_images(&input, &config, &mut state, &request).unwrap();
}

#[test]
fn dependency_new_progress_uses_union_of_delivered_native_intervals() {
    use crate::outline::{discover::DiscoverWork, evidence::EvidenceRef};
    let input = crate::outline::frozen::tests::related_native_input();
    let mut work = DiscoverWork::plan_with_budget(&input, &|sessions| {
        Ok(sessions
            .iter()
            .all(|s| s["pack"]["document_ids"].as_array().unwrap().len() == 1))
    });
    work.claim(2);
    let relation =
        work.session(&input, "pack-0").unwrap()["pack"]["read_dependencies"][0]["relation"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
    let reference = work
        .dependency_selection(&input, "pack-0", &relation)
        .unwrap()
        .into_iter()
        .find(|r| r.range().is_some_and(|(a, b)| b - a >= 3))
        .unwrap();
    let quote =
        crate::outline::evidence::resolve_evidence(&input, std::slice::from_ref(&reference))
            .unwrap()[0]
            .quote
            .clone();
    let cut = quote.char_indices().nth(1).unwrap().0;
    let mut first = reference.clone();
    let mut second = reference.clone();
    match (&mut first, &mut second) {
        (
            EvidenceRef::Text {
                start_byte: a,
                end_byte: b,
                ..
            },
            EvidenceRef::Text { start_byte: c, .. },
        )
        | (
            EvidenceRef::GridCell {
                start_byte: a,
                end_byte: b,
                ..
            },
            EvidenceRef::GridCell { start_byte: c, .. },
        ) => {
            *b = *a + cut;
            *c = *b;
        }
        _ => unreachable!(),
    }
    let (_, whole) = work
        .related_read(&input, "pack-0", std::slice::from_ref(&reference))
        .unwrap();
    let (_, left) = work
        .related_read(&input, "pack-0", std::slice::from_ref(&first))
        .unwrap();
    let (_, right) = work
        .related_read(&input, "pack-0", std::slice::from_ref(&second))
        .unwrap();
    assert!(!work.related_read_already_delivered("pack-0", &left));
    work.confirm_related_read(&input, "pack-0", &left).unwrap();
    assert!(!work.related_read_already_delivered("pack-0", &whole));
    work.confirm_related_read(&input, "pack-0", &right).unwrap();
    assert!(
        work.related_read_already_delivered("pack-0", &whole),
        "adjacent completed reads cover the same original interval"
    );
    assert!(
        work.related_read_already_delivered("pack-0", &left),
        "overlap/subrange is not new progress"
    );
}

#[test]
fn dependency_directory_pages_do_not_expand_with_the_complete_relation_set() {
    use crate::outline::discover::DiscoverWork;
    let baseline = crate::outline::frozen::tests::related_native_input();
    let plan = |input: &FrozenInput| {
        let mut work = DiscoverWork::plan_with_budget(input, &|sessions| {
            Ok(sessions
                .iter()
                .all(|s| s["pack"]["document_ids"].as_array().unwrap().len() == 1))
        });
        work.claim(2);
        work
    };
    let original = plan(&baseline).session(&baseline, "pack-0").unwrap();
    let mut input = baseline.clone();
    let relation = input.document_relations[0].clone();
    input.document_relations = (0..128)
        .map(|index| {
            let mut relation = relation.clone();
            relation.id = format!("relation-{index}");
            relation
        })
        .collect();
    let work = plan(&input);
    let session = work.session(&input, "pack-0").unwrap();
    assert!(
        serde_json::to_vec(&session).unwrap().len()
            < serde_json::to_vec(&original).unwrap().len() + 1024,
        "metadata for 128 dependencies must not accompany every owned atom"
    );
    let mut directory = session["pack"].clone();
    let mut seen = BTreeSet::new();
    let mut old_cursor = None;
    loop {
        let rows = directory["read_dependencies"].as_array().unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0]["available_originals"].as_array().unwrap().len() <= 1);
        assert_eq!(rows[0]["originals_preview_only"], true);
        let id = rows[0]["relation"]["id"].as_str().unwrap();
        assert!(seen.insert(id.to_owned()));
        assert!(
            work.dependency_selection(&input, "pack-0", id)
                .unwrap()
                .len()
                > 1,
            "body selection is complete despite a one-range preview"
        );
        let Some(cursor) = directory["next_dependency_cursor"]
            .as_str()
            .map(str::to_owned)
        else {
            break;
        };
        old_cursor = Some(cursor.clone());
        directory = work
            .dependency_directory_page(&input, "pack-0", &cursor)
            .unwrap();
    }
    assert_eq!(seen.len(), 128);
    assert!(
        work.dependency_directory_page(&input, "pack-1", old_cursor.as_deref().unwrap())
            .is_err(),
        "cursor cannot cross workers"
    );
    assert!(
        work.dependency_directory_page(&baseline, "pack-0", old_cursor.as_deref().unwrap())
            .is_err(),
        "cursor cannot cross frozen inputs"
    );
}

#[test]
fn support_directory_retains_every_grant_without_repeating_original_bodies() {
    use crate::outline::discover::{ConditionSupport, ConditionSupportOrigin, DiscoverWork};
    let input = crate::outline::frozen::tests::related_native_input();
    let mut work = DiscoverWork::plan_with_budget(&input, &|sessions| {
        Ok(sessions
            .iter()
            .all(|s| s["pack"]["document_ids"].as_array().unwrap().len() == 1))
    });
    work.claim(2);
    let relation =
        work.session(&input, "pack-0").unwrap()["pack"]["read_dependencies"][0]["relation"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
    let refs = work
        .dependency_selection(&input, "pack-0", &relation)
        .unwrap();
    let grants = (0..128)
        .map(|index| ConditionSupport {
            origin: ConditionSupportOrigin::ReviewedRequirement {
                requirement_id: format!("reviewed-{index}"),
            },
            review_version: format!("version-{index}"),
            evidence: refs.clone(),
        })
        .collect::<Vec<_>>();
    work.bind_condition_support("pack-0", vec![grants[0].clone()])
        .unwrap();
    let first = work.session(&input, "pack-0").unwrap();
    work.bind_condition_support("pack-0", grants).unwrap();
    let session = work.session(&input, "pack-0").unwrap();
    assert!(
        serde_json::to_vec(&session).unwrap().len()
            < serde_json::to_vec(&first).unwrap().len() + 512
    );
    let mut page = session["pack"].clone();
    let mut keys = BTreeSet::new();
    let mut cursor_to_reject = None;
    loop {
        let rows = page["condition_support_options"].as_array().unwrap();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].get("excerpts").is_none());
        assert!(rows[0].get("delivered_evidence").is_none());
        let key = rows[0]["support_key"].as_str().unwrap();
        assert!(keys.insert(key.to_owned()));
        assert_eq!(
            work.dependency_selection(&input, "pack-0", key).unwrap(),
            refs,
            "all ranges remain selectable independently of preview visibility"
        );
        let Some(cursor) = page["next_support_cursor"].as_str().map(str::to_owned) else {
            break;
        };
        cursor_to_reject = Some(cursor.clone());
        page = work
            .dependency_directory_page(&input, "pack-0", &cursor)
            .unwrap();
    }
    assert_eq!(keys.len(), 128);
    assert!(
        work.dependency_directory_page(&input, "pack-1", cursor_to_reject.as_deref().unwrap())
            .is_err()
    );
    // These are directory/authorization views, never a fresh Check receipt.
    let (read, _) = work.related_read(&input, "pack-0", &refs[..1]).unwrap();
    assert_eq!(read["excerpts"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn dependency_selectors_validate_the_same_immutable_worker_scope_before_mutation() {
    let input = crate::outline::frozen::tests::related_native_input();
    let config = crate::analysis::tests::config();
    let mut state = super::super::retirement::checkpoint(&input);
    let mut work = crate::outline::discover::DiscoverWork::plan_with_budget(&input, &|sessions| {
        Ok(sessions
            .iter()
            .all(|s| s["pack"]["document_ids"].as_array().unwrap().len() == 1))
    });
    work.claim(2);
    state.outline_run.reading_packs = Some(work);
    state
        .outline_run
        .discover_workers
        .insert("pack-0".into(), Worker::default());
    state
        .outline_run
        .discover_workers
        .insert("pack-1".into(), Worker::default());
    let journal = Durable {
        state: Mutex::new(state.clone()),
        stop_barrier: false,
        crash_received: AtomicBool::new(false),
        call_limit: 99,
    };
    let cancel = CancellationToken::new();
    let request = PackHost {
        input: &input,
        config: &config,
        state: &mut state,
        journal: &journal,
        cancel: &cancel,
    }
    .prepare_inner("pack-0", false)
    .await
    .unwrap();
    validate_worker_scope(&input, &state, &request).unwrap();
    let response = ChatTurn {
        content: String::new(),
        finish_reason: "tool_calls".into(),
        usage: None,
        tool_calls: vec![knowledge::models::ChatToolCall {
            id: "read".into(),
            name: "read_evidence".into(),
            arguments: "{}".into(),
        }],
    };
    for mode in [
        "checkpoint_input",
        "source",
        "relation",
        "run",
        "pack",
        "revision",
        "generation",
        "hash",
        "body",
        "claim",
    ] {
        let mut changed_input = input.clone();
        let mut changed_state = state.clone();
        let mut altered = request.clone();
        match mode {
            "checkpoint_input" => changed_state.input_sha256.push('x'),
            "source" => changed_input.source_units[0].text.push('x'),
            "relation" => changed_input.document_relations[0].required = false,
            "run" => altered.scope.run.push('x'),
            "pack" => altered.scope.pack = "pack-1".into(),
            "revision" => altered.scope.revision += 1,
            "generation" => altered.scope.generation += 1,
            "hash" => altered.scope.request.push('x'),
            "body" => altered.body.push(b' '),
            "claim" => {
                let mut value = json!(changed_state.outline_run.reading_packs.as_ref().unwrap());
                value["packs"]["pack-0"]["pack"]["claim_token"] = json!("different-claim");
                changed_state.outline_run.reading_packs =
                    Some(serde_json::from_value(value).unwrap());
            }
            _ => unreachable!(),
        }
        for args in [
            json!({"dependency_id":"pricing-reference"}),
            json!({"dependency_cursor":"unissued"}),
            json!({"cursor":"unissued"}),
        ] {
            let before = json!(changed_state);
            let error = PackHost {
                input: &changed_input,
                config: &config,
                state: &mut changed_state,
                journal: &journal,
                cancel: &cancel,
            }
            .related_page(&altered, &response, &args)
            .await
            .unwrap_err();
            assert!(error.contains("stale or foreign worker"), "{mode}: {error}");
            assert_eq!(json!(changed_state), before, "{mode}");
        }
    }
}

#[tokio::test]
async fn reviewed_support_authorization_is_not_worker_delivery_progress() {
    use crate::outline::discover::{ConditionSupport, ConditionSupportOrigin, DiscoverWork};
    let input = crate::outline::frozen::tests::related_native_input();
    let mut config = crate::analysis::tests::config();
    config.limits.max_no_progress_turns = 2;
    let mut state = super::super::retirement::checkpoint(&input);
    let mut work = DiscoverWork::plan_with_budget(&input, &|sessions| {
        Ok(sessions
            .iter()
            .all(|s| s["pack"]["document_ids"].as_array().unwrap().len() == 1))
    });
    work.claim(2);
    let refs = work
        .dependency_selection(&input, "pack-0", "pricing-reference")
        .unwrap();
    work.bind_condition_support(
        "pack-0",
        vec![ConditionSupport {
            origin: ConditionSupportOrigin::ReviewedRequirement {
                requirement_id: "previously-reviewed".into(),
            },
            review_version: "version".into(),
            evidence: refs,
        }],
    )
    .unwrap();
    state.outline_run.reading_packs = Some(work);
    state
        .outline_run
        .discover_workers
        .insert("pack-0".into(), Worker::default());
    let journal = Durable {
        state: Mutex::new(state.clone()),
        stop_barrier: false,
        crash_received: AtomicBool::new(false),
        call_limit: 99,
    };
    let cancel = CancellationToken::new();
    let mut host = PackHost {
        input: &input,
        config: &config,
        state: &mut state,
        journal: &journal,
        cancel: &cancel,
    };
    let request = host.prepare_inner("pack-0", false).await.unwrap();
    let response = ChatTurn {
        content: String::new(),
        finish_reason: "tool_calls".into(),
        usage: None,
        tool_calls: vec![knowledge::models::ChatToolCall {
            id: "read-current-original".into(),
            name: "read_evidence".into(),
            arguments: "{}".into(),
        }],
    };
    let key = host
        .state
        .outline_run
        .reading_packs
        .as_ref()
        .unwrap()
        .session(&input, "pack-0")
        .unwrap()["pack"]["condition_support_options"][0]["support_key"]
        .as_str()
        .unwrap()
        .to_owned();
    let first = host
        .related_page(&request, &response, &json!({"dependency_id":key}))
        .await
        .unwrap();
    assert_eq!(
        first["read"]["new_coverage"], true,
        "authorized original not yet delivered is a new read"
    );
    let worker = host
        .state
        .outline_run
        .discover_workers
        .get_mut("pack-0")
        .unwrap();
    assert!(
        worker.delivered_related_evidence.is_empty(),
        "queued page is not delivery"
    );
    worker.history.push(json!({"role":"assistant","content":"","tool_calls":[{"id":"read-current-original","type":"function","function":{"name":"read_evidence","arguments":"{}"}}]}));
    worker.history.push(json!({"role":"tool","tool_call_id":"read-current-original","content":super::super::tool_result_envelope(first).to_string()}));
    let delivered = host.prepare_inner("pack-0", false).await.unwrap();
    host.reserve(&delivered).await.unwrap();
    host.received(&delivered, &response).await.unwrap();
    confirm_worker_related_reads(&input, host.state, &delivered).unwrap();
    assert_eq!(
        host.state.outline_run.discover_workers["pack-0"]
            .delivered_related_evidence
            .len(),
        1
    );
    let repeated = host
        .related_page(&delivered, &response, &json!({"dependency_id":key}))
        .await
        .unwrap();
    assert_eq!(
        repeated["read"]["new_coverage"], false,
        "only completed source delivery removes new progress"
    );
    let mut prefix =
        host.state.outline_run.discover_workers["pack-0"].delivered_related_evidence[0].clone();
    let quote = crate::outline::evidence::resolve_evidence(&input, std::slice::from_ref(&prefix))
        .unwrap()[0]
        .quote
        .clone();
    match &mut prefix {
        crate::outline::evidence::EvidenceRef::Text {
            start_byte,
            end_byte,
            ..
        }
        | crate::outline::evidence::EvidenceRef::GridCell {
            start_byte,
            end_byte,
            ..
        } => *end_byte = *start_byte + quote.chars().next().unwrap().len_utf8(),
        _ => unreachable!(),
    }
    let subrange = host
        .related_page(&delivered, &response, &json!({"refs":[prefix]}))
        .await
        .unwrap();
    assert_eq!(
        subrange["read"]["new_coverage"], false,
        "a delivered subrange is not progress"
    );
    assert!(
        host.state
            .outline_run
            .tool_draft
            .check_reads
            .evidence
            .is_empty()
    );
    // Exercise the actual durable response/commit route, rather than inferring
    // exhaustion from related_page metadata alone.
    let mut last = None;
    for ordinal in 0..2 {
        let request = host.prepare_inner("pack-0", false).await.unwrap();
        let wire: Value = serde_json::from_slice(&request.body).unwrap();
        let payloads = wire["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|m| m["content"].as_str())
            .filter_map(|s| serde_json::from_str::<Value>(s).ok())
            .collect::<Vec<_>>();
        let scope = payloads
            .iter()
            .rev()
            .find(|p| p["wire_scope"].is_string())
            .unwrap()["wire_scope"]
            .clone();
        let repeat = ChatTurn {
            content: String::new(),
            finish_reason: "tool_calls".into(),
            usage: None,
            tool_calls: vec![knowledge::models::ChatToolCall {
                id: format!("repeat-{ordinal}"),
                name: "read_evidence".into(),
                arguments: json!({"wire_scope":scope,"dependency_id":"pricing-reference"})
                    .to_string(),
            }],
        };
        host.reserve(&request).await.unwrap();
        host.received(&request, &repeat).await.unwrap();
        host.commit(&request, repeat).await.unwrap();
        let worker = &host.state.outline_run.discover_workers["pack-0"];
        let receipt: Value =
            serde_json::from_str(worker.history.last().unwrap()["content"].as_str().unwrap())
                .unwrap();
        assert_eq!(receipt["ok"], true);
        assert_eq!(receipt["result"]["status"], "read_pending_delivery");
        assert_eq!(receipt["result"]["read"]["new_coverage"], false);
        assert_eq!(worker.rejected_turns, ordinal + 1);
        assert!(
            matches!(&worker.pending,Some(Pending::Failed{request:saved,code}) if saved==&request && code=="BUSINESS_REJECTED")
        );
        last = Some(request);
    }
    let request = last.unwrap();
    // Unknown sending is never converted to an exhausted settled attempt.
    host.state
        .outline_run
        .discover_workers
        .get_mut("pack-0")
        .unwrap()
        .pending = Some(Pending::Sending {
        request: request.clone(),
    });
    assert!(host.next(&BTreeSet::new()).await.is_err());
    assert!(
        matches!(&host.state.outline_run.discover_workers["pack-0"].pending,Some(Pending::Sending{request:saved}) if saved==&request)
    );
    host.state
        .outline_run
        .discover_workers
        .get_mut("pack-0")
        .unwrap()
        .pending = Some(Pending::Failed {
        request: request.clone(),
        code: "BUSINESS_REJECTED".into(),
    });
    let next = host.next(&BTreeSet::new()).await.unwrap();
    assert!(next.is_none_or(|next| next.scope.pack != "pack-0"));
    assert!(
        matches!(&host.state.outline_run.discover_workers["pack-0"].pending,Some(Pending::Failed{request:saved,code}) if saved==&request && code=="DISCOVER_RETRY_EXHAUSTED")
    );
    assert!(
        !host
            .state
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .complete()
    );
    assert!(!host.state.done);
    host.save().await.unwrap();
    let reloaded = journal.load().await.unwrap().unwrap();
    assert!(
        matches!(&reloaded.outline_run.discover_workers["pack-0"].pending,Some(Pending::Failed{code,..}) if code=="DISCOVER_RETRY_EXHAUSTED")
    );
}

#[tokio::test]
async fn failed_repair_revision_retires_reads_without_resetting_no_progress_or_unknown_sends() {
    let (input, config, journal) = fixture(1);
    let mut state = journal.state.lock().unwrap().clone();
    let cancel = CancellationToken::new();
    let mut host = PackHost {
        input: &input,
        config: &config,
        state: &mut state,
        journal: &journal,
        cancel: &cancel,
    };
    let mut last_request = None;
    for _ in 0..2 {
        let request = host.next(&BTreeSet::new()).await.unwrap().unwrap();
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        let packet = body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|m| m["content"].as_str())
            .filter_map(|s| serde_json::from_str::<Value>(s).ok())
            .find(|p| p["reading_packs"].is_array())
            .unwrap();
        let atom = &packet["reading_packs"][0]["pack"]["atoms"][0];
        let mut response = Delayed::default()
            .turn(&config, &request.body)
            .await
            .unwrap();
        let mut args: Value = serde_json::from_str(&response.tool_calls[0].arguments).unwrap();
        args["requirements"] = json!([{"description":" ","kind":"technical","obligation_strength":"mandatory","extraction_quality":"explicit","source_section_id":{"atom_key":atom["atom_key"]},"evidence":[{"evidence_key":atom["excerpts"][0]["evidence_key"]}]}]);
        args["no_requirement_reason"] = Value::Null;
        response.tool_calls[0].arguments = args.to_string();
        host.reserve(&request).await.unwrap();
        host.received(&request, &response).await.unwrap();
        host.commit(&request, response).await.unwrap();
        assert_eq!(
            host.state
                .outline_run
                .reading_packs
                .as_ref()
                .unwrap()
                .status("pack-0"),
            Some(PackStatus::Failed)
        );
        last_request = Some(request);
    }
    let request = last_request.unwrap();
    let identity = host
        .state
        .outline_run
        .reading_packs
        .as_ref()
        .unwrap()
        .pack_wire_identity("pack-0")
        .unwrap();
    assert!(
        identity["revision"].as_u64().unwrap() > request.scope.revision,
        "actual failed repair advanced the pack revision"
    );
    let worker = host
        .state
        .outline_run
        .discover_workers
        .get_mut("pack-0")
        .unwrap();
    assert_eq!(worker.rejected_turns, 2);
    let reference = crate::outline::evidence::EvidenceRef::Text {
        input_digest: host.state.input_sha256.clone(),
        unit_id: input.source_units[0].source_unit_revision_id.clone(),
        start_byte: 0,
        end_byte: input.source_units[0].text.len(),
    };
    worker.delivered_related_evidence.push(reference.clone());
    worker
        .delivered_dependency_pages
        .insert("old-directory".into());
    worker
        .source_keys
        .insert("old-source-key".into(), reference);
    worker
        .history
        .push(json!({"role":"assistant","content":"old owned scope"}));
    worker
        .related_read_frames
        .push(coordinator::RelatedReadFrame {
            directory_page: Some("old-directory".into()),
            call_id: "old-read".into(),
            pack_revision: request.scope.revision,
            input_digest: host.state.input_sha256.clone(),
            options: vec![],
            sealed_sha256: None,
        });
    let snapshot = host.state.clone();
    let retained_audit =
        settled_submission_audit(&snapshot.outline_run.discover_workers["pack-0"].history);
    assert!(!retained_audit.is_empty());
    for pending_kind in [
        "failed",
        "prepared",
        "prepared_other_run",
        "committed",
        "sending",
        "received",
    ] {
        let mut candidate = snapshot.clone();
        candidate
            .outline_run
            .discover_workers
            .get_mut("pack-0")
            .unwrap()
            .pending = Some(match pending_kind {
            "failed" => Pending::Failed {
                request: request.clone(),
                code: "BUSINESS_REJECTED".into(),
            },
            "prepared" => Pending::Prepared {
                request: request.clone(),
            },
            "prepared_other_run" => {
                let mut old = request.clone();
                old.scope.revision = identity["revision"].as_u64().unwrap();
                old.scope.run.push('x');
                Pending::Prepared { request: old }
            }
            "committed" => Pending::Committed {
                request: request.clone(),
                receipt: json!({}),
            },
            "sending" => Pending::Sending {
                request: request.clone(),
            },
            "received" => Pending::Received {
                request: request.clone(),
                response: json!({}),
            },
            _ => unreachable!(),
        });
        let old_workers = json!(candidate.outline_run.discover_workers);
        let old_accounting = json!(candidate.journal.accounting);
        let result = PackHost {
            input: &input,
            config: &config,
            state: &mut candidate,
            journal: &journal,
            cancel: &cancel,
        }
        .next(&BTreeSet::new())
        .await;
        if matches!(pending_kind, "sending" | "received") {
            assert!(
                result
                    .unwrap_err()
                    .message
                    .contains("unresolved worker journal scope changed")
            );
            assert_eq!(json!(candidate.outline_run.discover_workers), old_workers);
            assert_eq!(json!(candidate.journal.accounting), old_accounting);
        } else {
            let fresh = result.unwrap().unwrap();
            assert_eq!(fresh.scope.revision, identity["revision"].as_u64().unwrap());
            let worker = &candidate.outline_run.discover_workers["pack-0"];
            assert!(worker.delivered_related_evidence.is_empty());
            assert!(worker.delivered_dependency_pages.is_empty());
            assert!(worker.related_read_frames.is_empty());
            assert_eq!(worker.history, retained_audit);
            assert!(
                !worker
                    .history
                    .iter()
                    .any(|message| message["content"] == "old owned scope")
            );
            assert!(!worker.source_keys.contains_key("old-source-key"));
            assert_eq!(
                worker.rejected_turns,
                if pending_kind == "committed" { 0 } else { 2 }
            );
        }
    }
    let mut exhausted = snapshot;
    exhausted
        .outline_run
        .discover_workers
        .get_mut("pack-0")
        .unwrap()
        .rejected_turns = config.limits.max_no_progress_turns;
    assert!(
        PackHost {
            input: &input,
            config: &config,
            state: &mut exhausted,
            journal: &journal,
            cancel: &cancel
        }
        .next(&BTreeSet::new())
        .await
        .unwrap()
        .is_none()
    );
    assert_eq!(
        exhausted.outline_run.discover_workers["pack-0"].rejected_turns,
        config.limits.max_no_progress_turns
    );
}

#[test]
fn failed_submission_projection_keeps_unconsumed_read_and_unsettled_groups() {
    let read = json!({"role":"assistant","tool_calls":[{"id":"read","function":{"name":"read_evidence","arguments":"{}"}}]});
    let read_result = json!({"role":"tool","tool_call_id":"read","content":"{\"ok\":true,\"result\":{\"status\":\"read_pending_delivery\"}}"});
    let pending = json!({"role":"assistant","tool_calls":[{"id":"pending","function":{"name":"submit_pack","arguments":"large unacknowledged body"}}]});
    let success = json!({"role":"assistant","tool_calls":[{"id":"success","function":{"name":"submit_pack","arguments":"successful body"}}]});
    let success_result = json!({"role":"tool","tool_call_id":"success","content":"{\"ok\":true}"});
    let mixed = json!({"role":"assistant","tool_calls":[{"id":"mixed-submit","function":{"name":"submit_pack","arguments":"mixed body"}},{"id":"mixed-read","function":{"name":"read_evidence","arguments":"{}"}}]});
    let mixed_result =
        json!({"role":"tool","tool_call_id":"mixed-submit","content":"{\"ok\":false}"});
    let history = vec![
        read,
        read_result,
        pending,
        success,
        success_result,
        mixed,
        mixed_result,
    ];
    assert_eq!(project_settled_submission_history(&history), history);
}

#[tokio::test]
async fn near_window_worker_repair_projects_failure_but_retains_full_audit_after_reload() {
    let (input, mut config, journal) = fixture(1);
    let mut state = journal.state.lock().unwrap().clone();
    let cancel = CancellationToken::new();
    let mut host = PackHost {
        input: &input,
        config: &config,
        state: &mut state,
        journal: &journal,
        cancel: &cancel,
    };
    let first = host.next(&BTreeSet::new()).await.unwrap().unwrap();
    let body: Value = serde_json::from_slice(&first.body).unwrap();
    let packet = body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["content"].as_str())
        .filter_map(|s| serde_json::from_str::<Value>(s).ok())
        .find(|p| p["reading_packs"].is_array())
        .unwrap();
    let atom = &packet["reading_packs"][0]["pack"]["atoms"][0];
    let mut failed = Delayed::default().turn(&config, &first.body).await.unwrap();
    let large = "synthetic failed submission audit ".repeat(10000);
    failed.content = large.clone();
    let mut args: Value = serde_json::from_str(&failed.tool_calls[0].arguments).unwrap();
    args["requirements"] = json!([{"description":large,"kind":"technical","obligation_strength":"mandatory","extraction_quality":"explicit","source_section_id":{"atom_key":atom["atom_key"]},"evidence":[{"evidence_key":atom["excerpts"][0]["evidence_key"]}]}]);
    // The valid positive extraction conflicts with the existing negative reason.
    failed.tool_calls[0].arguments = args.to_string();
    host.reserve(&first).await.unwrap();
    host.received(&first, &failed).await.unwrap();
    host.commit(&first, failed.clone()).await.unwrap();
    let audit = host.state.outline_run.discover_workers["pack-0"]
        .history
        .clone();
    assert_eq!(audit[0]["content"], failed.content);
    assert!(settled_submission_pair(&audit, 0).is_some());
    let second = host.next(&BTreeSet::new()).await.unwrap().unwrap();
    assert_eq!(second.scope.revision, first.scope.revision);
    let second_wire: Value = serde_json::from_slice(&second.body).unwrap();
    let second_packet = second_wire["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["content"].as_str())
        .filter_map(|s| serde_json::from_str::<Value>(s).ok())
        .find(|p| p["reading_packs"].is_array())
        .unwrap();
    let second_atom = &second_packet["reading_packs"][0]["pack"]["atoms"][0];
    let mut second_failure = Delayed::default()
        .turn(&config, &second.body)
        .await
        .unwrap();
    second_failure.content = failed.content.clone();
    let mut second_args: Value =
        serde_json::from_str(&second_failure.tool_calls[0].arguments).unwrap();
    assert_eq!(second_args["repair"], true);
    second_args["requirements"] = json!([{"description":failed.content,"kind":"technical","obligation_strength":"mandatory","extraction_quality":"explicit","source_section_id":{"atom_key":second_atom["atom_key"]},"evidence":[{"evidence_key":second_atom["excerpts"][0]["evidence_key"]}]}]);
    second_failure.tool_calls[0].arguments = second_args.to_string();
    host.reserve(&second).await.unwrap();
    host.received(&second, &second_failure).await.unwrap();
    host.commit(&second, second_failure).await.unwrap();
    let audit = host.state.outline_run.discover_workers["pack-0"]
        .history
        .clone();
    assert_eq!(audit.len(), 4);
    assert_eq!(audit[0]["content"], failed.content);
    assert_eq!(audit[2]["content"], failed.content);
    let repair = host.next(&BTreeSet::new()).await.unwrap().unwrap();
    assert!(repair.scope.revision > second.scope.revision);
    assert_eq!(
        host.state.outline_run.discover_workers["pack-0"].history,
        audit
    );
    assert!(!String::from_utf8_lossy(&repair.body).contains(&failed.content));
    assert!(String::from_utf8_lossy(&repair.body).contains("settled_submission_failure"));
    // Save the common generation/registry starting point for both probes.
    host.save().await.unwrap();
    let measured = host.prepare_inner("pack-0", false).await.unwrap();
    let measured_wire: Value = serde_json::from_slice(&measured.body).unwrap();
    let count = crate::agent_runtime::chat::estimate_request_tokens_with_reserve(
        &measured_wire,
        &config.limits.tokenizer,
        config.limits.image_token_reserve,
        config.limits.token_safety_margin,
        config.provider.output_token_reserve as usize,
    )
    .unwrap();
    let first_count = crate::agent_runtime::chat::estimate_request_tokens_with_reserve(
        &body,
        &config.limits.tokenizer,
        config.limits.image_token_reserve,
        config.limits.token_safety_margin,
        config.provider.output_token_reserve as usize,
    )
    .unwrap();
    assert!(first_count.total_context_tokens <= count.total_context_tokens);
    assert!(
        first_count.total_context_tokens * 5 >= count.total_context_tokens * 4,
        "fixture first wire must already occupy at least 80% of the measured repair window"
    );
    drop(host);
    // The actual compact wire fits at its measured context boundary; no extra
    // hypothetical future output is reserved. Original audit is far larger.
    config.limits.max_context_tokens = count.total_context_tokens;
    let mut restored = journal.load().await.unwrap().unwrap();
    let mut tight = PackHost {
        input: &input,
        config: &config,
        state: &mut restored,
        journal: &journal,
        cancel: &cancel,
    };
    let fitted = tight.prepare_inner("pack-0", false).await.unwrap();
    assert_eq!(
        fitted.body, measured.body,
        "same checkpoint generation must yield identical measured wire"
    );
    assert_eq!(
        tight.state.outline_run.discover_workers["pack-0"].history,
        audit
    );
    assert!(fitted.body.len() < serde_json::to_vec(&audit).unwrap().len());
    assert!(
        !tight
            .state
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .complete()
    );
    assert!(
        tight
            .state
            .outline_run
            .tool_draft
            .check_reads
            .evidence
            .is_empty()
    );
    drop(tight);
    config.limits.max_context_tokens = 1;
    let mut too_small = PackHost {
        input: &input,
        config: &config,
        state: &mut restored,
        journal: &journal,
        cancel: &cancel,
    };
    assert_eq!(
        too_small
            .prepare_inner("pack-0", false)
            .await
            .unwrap_err()
            .code,
        "AGENT_TURN_BUDGET_EXCEEDED"
    );
}
