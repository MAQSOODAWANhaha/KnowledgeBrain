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
