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
        Ok(ChatTurn{content:String::new(),finish_reason:"tool_calls".into(),usage:Some(knowledge::models::ChatUsage{prompt_tokens:Some(10),completion_tokens:Some(2),total_tokens:Some(12),..Default::default()}),tool_calls:vec![knowledge::models::ChatToolCall{id:format!("result-{position}"),name:"submit_pack".into(),arguments:json!({"wire_scope":scope,"pack_id":pack["id"],"call_id":session["submission_operation_id"],"claim_token":pack["claim_token"],"pack_revision":pack["pack_revision"],"repair":session["status"]=="failed","requirements":[],"no_requirement_reason":"Synthetic protocol fixture only; no semantic model result.","inspected_atom_ids":pack["atoms"].as_array().unwrap().iter().filter(|atom|atom["context_only"]!=true).map(|atom|json!({"atom_key":atom["atom_key"]})).collect::<Vec<_>>()}).to_string()}]})
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
        "AGENT_DISCOVER_BLOCKED"
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
    assert!(restored.outline_run.tool_draft.chapters.is_empty());
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
                    let refs = dependency["available_originals"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|source| source["evidence"].clone())
                        .collect::<Vec<_>>();
                    assert!(!refs.is_empty());
                    return Ok(ChatTurn {
                        content: String::new(),
                        finish_reason: "tool_calls".into(),
                        usage: None,
                        tool_calls: vec![knowledge::models::ChatToolCall {
                            id: "read-pricing".into(),
                            name: "read_evidence".into(),
                            arguments: json!({"wire_scope":scope,"refs":refs}).to_string(),
                        }],
                    });
                }
                let read = &read.unwrap()["result"]["read"];
                let excerpts = read["excerpts"].as_array().unwrap();
                let numeric = excerpts.iter().any(|excerpt| {
                    excerpt["quote"]
                        .as_str()
                        .is_some_and(|text| text.contains("15%") && text.contains("1,234.50元"))
                });
                let formula = excerpts.iter().any(|excerpt| {
                    excerpt["locator"]["cells"].as_array().is_some_and(|cells| {
                        cells.iter().any(|cell| {
                            cell["formula"] == "=C1*(1+A1)"
                                && cell["display_incomplete_reason"]
                                    == "formula_cached_value_missing"
                        })
                    })
                });
                if numeric && formula {
                    self.saw_attachment.store(true, Ordering::SeqCst);
                }
                let note = if numeric && formula {
                    "Observed attachment: 15%, 1,234.50元; formula cache explicitly missing."
                } else {
                    "Continue the current dependency selection."
                };
                if let Some(cursor) = read["next_cursor"].as_str() {
                    let generation = payloads
                        .iter()
                        .find_map(|value| value["generation"].as_u64())
                        .unwrap();
                    return Ok(ChatTurn {
                        content: note.into(),
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
                    numeric && formula
                        || body["messages"].as_array().unwrap().iter().any(
                            |message| message["role"] == "assistant"
                                && message["content"]
                                    .to_string()
                                    .contains("Observed attachment: 15%")
                                && message["content"]
                                    .to_string()
                                    .contains("formula cache explicitly missing")
                        )
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
    let source = input
        .source_units
        .iter()
        .find(|source| source.document_id == "pricing" && source.text.contains("15%"))
        .unwrap();
    let reference = EvidenceRef::Text {
        input_digest: input_digest(&input).unwrap(),
        unit_id: source.source_unit_revision_id.clone(),
        start_byte: 0,
        end_byte: source.text.len(),
    };
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
    let source = input
        .source_units
        .iter()
        .find(|source| source.document_id == "pricing" && !source.text.is_empty())
        .unwrap();
    let continuation = coordinator::RelatedContinuation {
        run: identity["run"].as_str().unwrap().into(),
        pack: "pack-0".into(),
        pack_revision: 1,
        input_digest: crate::outline::evidence::input_digest(&input).unwrap(),
        refs: vec![crate::outline::evidence::EvidenceRef::Text {
            input_digest: crate::outline::evidence::input_digest(&input).unwrap(),
            unit_id: source.source_unit_revision_id.clone(),
            start_byte: 0,
            end_byte: source.text.len(),
        }],
    };
    let cursor = format!(
        "page_{}",
        crate::outline::canonical_sha256(&continuation).unwrap()
    );
    state.outline_run.reading_packs = Some(work);
    state.outline_run.discover_workers.insert(
        "pack-0".into(),
        Worker {
            related_continuations: std::collections::BTreeMap::from([(
                cursor.clone(),
                continuation,
            )]),
            ..Default::default()
        },
    );
    let request = Request {
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
