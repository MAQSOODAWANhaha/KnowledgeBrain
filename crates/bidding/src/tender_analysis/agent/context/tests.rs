use super::*;

#[tokio::test]
#[ignore = "requires KB_TENDER_CONTEXT_REPLAY_DIR and KB_TENDER_CONTEXT_REPORT; offline scope transition only"]
async fn archived_completed_scope_opening_preserves_new_navigation_in_request() {
    let root = std::path::PathBuf::from(std::env::var("KB_TENDER_CONTEXT_REPLAY_DIR").unwrap());
    let checkpoint = std::fs::read(root.join("checkpoint.json")).unwrap();
    let mut state: Checkpoint = serde_json::from_slice(&checkpoint).unwrap();
    let input: FrozenInput =
        serde_json::from_slice(&std::fs::read(root.join("frozen-input.json")).unwrap()).unwrap();
    let config: Config =
        serde_json::from_slice(&std::fs::read(root.join("runtime.json")).unwrap()).unwrap();
    let current = Config::with_provider(config.provider.clone(), config.limits.clone()).unwrap();
    assert_eq!(
        json!(current),
        json!(config),
        "archived runtime still matches current contract"
    );
    assert_eq!(state.input_sha256, digest(&input).unwrap());
    assert_eq!(state.config_sha256, digest(&current).unwrap());
    state
        .journal
        .validate(
            state.turn,
            if state.role == Role::Main {
                "main"
            } else {
                "reviewer"
            },
        )
        .unwrap();
    let before: Value =
        serde_json::from_slice(&std::fs::read(root.join("request-before.json")).unwrap()).unwrap();
    let open: Value =
        serde_json::from_slice(&std::fs::read(root.join("open-call.json")).unwrap()).unwrap();
    let messages = before["messages"].as_array().unwrap();
    let prior_note = messages
        .iter()
        .rev()
        .flat_map(|m| m["tool_calls"].as_array().into_iter().flatten())
        .find(|c| c["function"]["name"] == "set_work_note")
        .unwrap();
    state.role = Role::Main;
    state.main_work =
        Some(serde_json::from_str(prior_note["function"]["arguments"].as_str().unwrap()).unwrap());
    assert_eq!(state.work().unwrap().status, WorkStatus::Complete);
    state.transcript = messages[2..messages.len() - 1].to_vec();
    state.pending_coverage = None;
    state.journal = Default::default();
    let retained = state.transcript.clone();
    let coverage = json!(state.coverage());
    let analysis = json!(state.analysis);
    let call = &open["tool_calls"][0];
    let args: Value =
        serde_json::from_str(call["function"]["arguments"].as_str().unwrap()).unwrap();
    state.transcript.push(open.clone());
    let result = super::apply(&input, &config, &mut state, "set_work_note", &args).unwrap();
    assert!(state.transcript.starts_with(&retained));
    assert_eq!(result["handoff"], false);
    state
        .transcript
        .push(json!({"role":"tool","tool_call_id":call["id"],
        "content":json!({"ok":true,"result":result}).to_string()}));
    let body = super::request(&input, &config, &mut state).await.unwrap();
    let request: Value = serde_json::from_slice(&body).unwrap();
    let wire = request["messages"].as_array().unwrap();
    for message in retained.iter().filter(|m| m["role"] == "tool") {
        assert!(
            wire.iter()
                .any(|m| m["tool_call_id"] == message["tool_call_id"]
                    && m["content"] == message["content"]),
            "delivered navigation must survive actual request preparation"
        );
    }
    assert_eq!(json!(state.coverage()), coverage);
    assert_eq!(json!(state.analysis), analysis);
    assert!(body.len() <= config.limits.max_context_bytes);
    assert!(
        estimate_input_tokens(&request, &config.limits).unwrap()
            + config.provider.max_tokens as usize
            <= config.limits.max_context_tokens
    );
    assert_eq!(
        std::fs::read(root.join("checkpoint.json")).unwrap(),
        checkpoint
    );
    std::fs::write(std::env::var("KB_TENDER_CONTEXT_REPORT").unwrap(), serde_json::to_vec_pretty(&json!({
        "mode":"offline scope-transition projection, not Journal recovery or model rerun",
        "request_bytes":body.len(),"fresh_navigation_retained":true,"analysis_and_coverage_unchanged":true,
        "archived_checkpoint_unchanged":true,"configured_budgets_respected":true,
        "current_runtime_and_archived_journal_valid":true
    })).unwrap()).unwrap();
}

#[tokio::test]
#[ignore = "requires KB_TENDER_CONTEXT_REPLAY_DIR and KB_TENDER_CONTEXT_REPORT; saved response and cached images only"]
async fn saved_mixed_batch_defers_overflowing_writes_without_losing_fitted_comparisons() {
    struct NoIo;
    #[async_trait]
    impl Journal for NoIo {
        async fn load(&self) -> Result<Option<Checkpoint>, AgentError> {
            panic!("offline only")
        }
        async fn reserve(&self, _: &Checkpoint, _: &[u8]) -> Result<Option<usize>, AgentError> {
            panic!("offline only")
        }
        async fn save(&self, _: &Checkpoint, _: &Value) -> Result<(), AgentError> {
            panic!("offline only")
        }
    }
    let root = std::path::PathBuf::from(std::env::var("KB_TENDER_CONTEXT_REPLAY_DIR").unwrap());
    let original = std::fs::read(root.join("extraction/checkpoint.json")).unwrap();
    let saved: Checkpoint = serde_json::from_slice(&original).unwrap();
    let input: FrozenInput =
        serde_json::from_slice(&std::fs::read(root.join("input/frozen-input.json")).unwrap())
            .unwrap();
    let runtime: Value =
        serde_json::from_slice(&std::fs::read(root.join("extraction/runtime.json")).unwrap())
            .unwrap();
    let config = Config::with_provider(
        serde_json::from_value(runtime["provider"].clone()).unwrap(),
        serde_json::from_value(runtime["limits"].clone()).unwrap(),
    )
    .unwrap();
    let response = saved.journal.response().unwrap().clone();
    let mut state = saved.clone();
    let result = super::execute_turn(
        &input,
        &config,
        &mut state,
        &NoIo,
        response.clone(),
        BTreeMap::new(),
        &CancellationToken::new(),
    )
    .await;
    let mut checks = vec![];
    let mut bytes = None;
    if let Ok(outputs) = &result {
        for (call, output) in response.tool_calls.iter().zip(outputs) {
            if call.name != "complete_review_check" {
                continue;
            }
            let args: Value = serde_json::from_str(&call.arguments).unwrap();
            let key = args["reference"].as_str().unwrap();
            let out: Value = serde_json::from_str(output["content"].as_str().unwrap()).unwrap();
            let compared = has_review_outcome(&state, key).unwrap();
            checks.push(json!({"reference":key,"ok":out["ok"],"error":out["error"],"compared_after":compared,"compared_before":has_review_outcome(&saved,key).unwrap()}));
        }
        bytes = Some(
            super::request(&input, &config, &mut state)
                .await
                .unwrap()
                .len(),
        );
    }
    std::fs::write(std::env::var("KB_TENDER_CONTEXT_REPORT").unwrap(), serde_json::to_vec_pretty(&json!({"mode":"offline saved response replay; no model or persistence calls","saved_turn":saved.turn,"error":result.as_ref().err().map(|e|&e.message),"request_bytes":bytes,"checks":checks})).unwrap()).unwrap();
    assert_eq!(
        std::fs::read(root.join("extraction/checkpoint.json")).unwrap(),
        original
    );
    let outputs = result.unwrap();
    assert_eq!(outputs.len(), response.tool_calls.len());
    assert_eq!(state.turn, saved.turn + 1);
    assert_eq!(
        state.tool_calls,
        saved.tool_calls + response.tool_calls.len()
    );
    assert_eq!(json!(state.analysis), json!(saved.analysis));
    assert_eq!(state.journal.body().unwrap(), saved.journal.body().unwrap());
    assert!(
        checks
            .iter()
            .any(|c| c["ok"] == true && c["compared_after"] == true)
    );
    assert!(checks.iter().any(|c| c["ok"] == false));
    for check in checks.iter().filter(|c| c["ok"] == false) {
        assert_eq!(
            check["compared_after"], check["compared_before"],
            "rejected writes must not establish comparisons"
        );
    }
}

#[tokio::test]
#[ignore = "requires KB_TENDER_CONTEXT_REPLAY_DIR and KB_TENDER_CONTEXT_REPORT; cached images only, no I/O"]
async fn checkpoint_original_image_rereads_fit_without_new_coverage() {
    struct NoIo;
    #[async_trait]
    impl Journal for NoIo {
        async fn load(&self) -> Result<Option<Checkpoint>, AgentError> {
            panic!("offline only")
        }
        async fn reserve(&self, _: &Checkpoint, _: &[u8]) -> Result<Option<usize>, AgentError> {
            panic!("offline only")
        }
        async fn save(&self, _: &Checkpoint, _: &Value) -> Result<(), AgentError> {
            panic!("offline only")
        }
    }
    let root = std::path::PathBuf::from(std::env::var("KB_TENDER_CONTEXT_REPLAY_DIR").unwrap());
    let original = std::fs::read(root.join("extraction/checkpoint.json")).unwrap();
    let saved: Checkpoint = serde_json::from_slice(&original).unwrap();
    let input: FrozenInput =
        serde_json::from_slice(&std::fs::read(root.join("input/frozen-input.json")).unwrap())
            .unwrap();
    let runtime: Value =
        serde_json::from_slice(&std::fs::read(root.join("extraction/runtime.json")).unwrap())
            .unwrap();
    let config = Config::with_provider(
        serde_json::from_value(runtime["provider"].clone()).unwrap(),
        serde_json::from_value(runtime["limits"].clone()).unwrap(),
    )
    .unwrap();
    let mut reports = vec![];
    for (id, identity) in &saved.coverage().views {
        let mut state = saved.clone();
        state.journal = Default::default();
        if let Some(delivered) = state.pending_coverage.take() {
            state.replace_coverage(delivered);
        }
        let before = json!([
            state.analysis,
            state.coverage(),
            state.turn,
            state.tool_calls
        ]);
        let args = json!({"source_id":identity.source_id});
        check_read_scope(&input, &state, "read_source_view", &args).unwrap();
        state.transcript.push(json!({"role":"assistant","tool_calls":[{"id":"reread","type":"function","function":{"name":"read_source_view","arguments":args.to_string()}}]}));
        let result = super::read_source_view(
            &input,
            &config,
            &mut state,
            &NoIo,
            &args,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(result["view_id"], *id);
        state.pending_coverage = Some(state.coverage().clone());
        state.transcript.push(json!({"role":"tool","tool_call_id":"reread","content":json!({"ok":true,"result":result}).to_string()}));
        let fitted =
            super::fit_batch(&input, &config, &mut state, &[], std::slice::from_ref(id)).await;
        let mut bytes = None;
        if fitted.is_ok() {
            state
                .transcript
                .push(json!({"role":"user","source_view_refs":[id]}));
            let wire = super::prepare_request(&input, &config, &mut state, false)
                .await
                .unwrap();
            let body: Value = serde_json::from_slice(&wire).unwrap();
            let expected = state.source_views[id].message();
            assert!(body["messages"].as_array().unwrap().contains(&expected));
            bytes = Some(wire.len());
        }
        assert_eq!(
            json!([
                state.analysis,
                state.coverage(),
                state.turn,
                state.tool_calls
            ]),
            before
        );
        reports.push(json!({"source_id":identity.source_id,"view_id":id,"bytes":bytes,"error":fitted.err().map(|e|e.message)}));
    }
    assert!(!reports.is_empty());
    assert_eq!(
        std::fs::read(root.join("extraction/checkpoint.json")).unwrap(),
        original
    );
    std::fs::write(
        std::env::var("KB_TENDER_CONTEXT_REPORT").unwrap(),
        serde_json::to_vec_pretty(&json!({"mode":"offline cached image rereads, original checkpoint and receipts unchanged","turn":saved.turn,"reports":reports})).unwrap(),
    ).unwrap();
    assert!(reports.iter().all(|r| r["error"].is_null()), "{reports:?}");
}

#[test]
#[ignore = "requires KB_TENDER_WORK_GAP_REPLAY_DIR and KB_TENDER_WORK_GAP_REPORT; diagnostics only"]
fn recorded_work_gap_diagnostics_do_not_resume_or_rewrite_the_run() {
    use std::path::PathBuf;
    let root = PathBuf::from(std::env::var("KB_TENDER_WORK_GAP_REPLAY_DIR").unwrap());
    let checkpoint_path = root.join("extraction/checkpoint.json");
    let original = std::fs::read(&checkpoint_path).unwrap();
    let input: FrozenInput =
        serde_json::from_slice(&std::fs::read(root.join("input/frozen-input.json")).unwrap())
            .unwrap();
    let mut projection: Value = serde_json::from_slice(&original).unwrap();
    // An in-memory diagnostic projection of the old payload only. Its
    // input/config identity is never changed or passed to agent::run.
    if projection.get("review_draft").is_none() {
        projection["review_draft"] = json!({});
    }
    if projection.get("journal").is_none() {
        projection["journal"] = json!({"sequence":0,"pending":null});
    }
    let state: Checkpoint = serde_json::from_value(projection).unwrap();
    let before = digest(&state).unwrap();
    let work = state.work().unwrap();
    let config: Config =
        serde_json::from_slice(&std::fs::read(root.join("extraction/runtime.json")).unwrap())
            .unwrap();
    let rows = completion_gaps(&input, &state, work, config.limits.max_tool_result_bytes).unwrap();
    let global = tools::gaps(&input, &state.analysis);
    let global_scope_items = global
        .iter()
        .take(50)
        .filter(|g| {
            g["source_id"]
                .as_str()
                .is_some_and(|id| work.source_scope.iter().any(|s| s == id))
        })
        .count();
    assert_eq!(
        global_scope_items, 0,
        "fixture must reproduce a global page hiding local blockers"
    );
    let missing = work
        .source_scope
        .iter()
        .filter(|id| !state.analysis.dispositions.contains_key(*id))
        .count();
    assert!(missing > 0);
    assert_eq!(
        rows.iter()
            .filter(|g| g["kind"] == "missing_disposition")
            .count(),
        missing
    );
    // Exercise the tool's actual byte-bounded continuation, without an LLM.
    let mut found = Vec::new();
    let mut offset = 0;
    let mut pages = 0;
    while offset < rows.len() {
        let page = work_gaps(
            &input,
            &state,
            &json!({"scope":"work","offset":offset,"limit":rows.len()}),
            2048,
        )
        .unwrap();
        assert!(serde_json::to_vec(&page).unwrap().len() <= 2048);
        let next = page["next"].as_u64().unwrap() as usize;
        assert!(next > offset);
        found.extend(page["items"].as_array().unwrap().iter().cloned());
        offset = next;
        pages += 1;
    }
    assert_eq!(found, rows);
    assert!(pages > 1);
    assert_eq!(digest(&state).unwrap(), before);
    assert_eq!(std::fs::read(&checkpoint_path).unwrap(), original);
    let request_packet = request_work_state(&input, &state, 2048).unwrap();
    assert!(serde_json::to_vec(&request_packet).unwrap().len() <= 2048);
    assert_eq!(request_packet["gap_counts"]["missing_disposition"], missing);
    assert!(request_packet["blockers"]["next"].as_u64().unwrap() > 0);
    assert_eq!(std::fs::read(&checkpoint_path).unwrap(), original);
    let report = json!({"mode":"offline diagnostics only; no model call or run recovery", "request_packet":request_packet,
        "recorded_turn":state.turn,"active_sources":work.source_scope.len(),
        "global_gap_count":global.len(),"global_first_50_scope_items":global_scope_items,
        "local_gap_count":rows.len(),"missing_scope_dispositions":missing,
        "pages_at_2048_bytes":pages,"items":rows});
    std::fs::write(
        std::env::var("KB_TENDER_WORK_GAP_REPORT").unwrap(),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
}

#[tokio::test]
#[ignore = "requires checkpoint replay directory, inspection argument array and report via KB_TENDER_CONTEXT_*; offline only"]
async fn checkpoint_exact_inspection_preserves_working_evidence() {
    checkpoint_inspection_replay(true).await;
}

#[tokio::test]
#[ignore = "requires a cache-pressure checkpoint, queries and report via KB_TENDER_CONTEXT_*; offline only"]
async fn checkpoint_exact_inspection_preserves_required_evidence_with_partial_optional_cache() {
    checkpoint_inspection_replay(false).await;
}

async fn checkpoint_inspection_replay(require_full_cache: bool) {
    let root = std::path::PathBuf::from(std::env::var("KB_TENDER_CONTEXT_REPLAY_DIR").unwrap());
    let original = std::fs::read(root.join("extraction/checkpoint.json")).unwrap();
    let saved: Checkpoint = serde_json::from_slice(&original).unwrap();
    let input: FrozenInput =
        serde_json::from_slice(&std::fs::read(root.join("input/frozen-input.json")).unwrap())
            .unwrap();
    let runtime: Value =
        serde_json::from_slice(&std::fs::read(root.join("extraction/runtime.json")).unwrap())
            .unwrap();
    let config = Config::with_provider(
        serde_json::from_value(runtime["provider"].clone()).unwrap(),
        serde_json::from_value(runtime["limits"].clone()).unwrap(),
    )
    .unwrap();
    let queries: Vec<Value> = serde_json::from_slice(
        &std::fs::read(std::env::var("KB_TENDER_CONTEXT_REQUEST").unwrap()).unwrap(),
    )
    .unwrap();
    let mut reports = vec![];
    for args in queries {
        let mut state = saved.clone();
        state.journal = Default::default();
        if let Some(delivered) = state.pending_coverage.take() {
            state.replace_coverage(delivered);
        }
        let call = knowledge::models::ChatToolCall {
            id: "offline-inspection".into(),
            name: "inspect_analysis".into(),
            arguments: args.to_string(),
        };
        state.transcript.push(json!({"role":"assistant","content":null,"tool_calls":[{"id":call.id,"type":"function","function":{"name":call.name,"arguments":call.arguments}}]}));
        let committed = state.coverage().clone();
        let expected = focused_work_evidence(&state, &state.transcript);
        let result = super::inspect_in_context(
            &input,
            &config,
            &mut state,
            &args,
            std::slice::from_ref(&call),
            &[],
            &committed,
        )
        .await;
        let mut wire_report = Value::Null;
        if let Ok(page) = &result {
            state
                .transcript
                .push(json!({"role":"tool","tool_call_id":call.id,
                "content":json!({"ok":true,"result":page}).to_string()}));
            let bytes = super::request(&input, &config, &mut state).await.unwrap();
            let body: Value = serde_json::from_slice(&bytes).unwrap();
            let messages = body["messages"].as_array().unwrap();
            let actual = focused_work_evidence(&state, messages);
            let all_visible = visible_work_evidence(&state, messages);
            let available = candidate_recall_message(
                &state,
                config.limits.max_tool_result_bytes,
                &BTreeMap::new(),
            )
            .unwrap();
            let available: Value = available["content"]
                .as_str()
                .map(|content| serde_json::from_str(content).unwrap())
                .unwrap_or(Value::Null);
            let recall_candidates: Vec<_> = available["retained_candidate_details"]["items"]
                .as_array()
                .into_iter()
                .flatten()
                .collect();
            let missing_recall: Vec<_> = recall_candidates
                .iter()
                .filter(|item| {
                    !all_visible.contains_key(&format!(
                        "candidate:{}:{}",
                        item["reference"].as_str().unwrap(),
                        digest(&item["value"]).unwrap()
                    ))
                })
                .map(|item| &item["reference"])
                .collect();
            let missing: Vec<_> = expected
                .iter()
                .filter(|(key, ranges)| {
                    !ranges
                        .iter()
                        .all(|&(a, b)| tools::contains(actual.get(*key), a, b))
                        && !key.strip_prefix("view:").is_some_and(|id| {
                            state
                                .source_views
                                .get(id)
                                .is_some_and(|v| messages.contains(&v.message()))
                        })
                })
                .map(|(key, _)| key)
                .collect();
            let focus_candidates = recall_candidates
                .iter()
                .filter(|item| {
                    state.work().is_some_and(|work| {
                        work.focus
                            .references
                            .iter()
                            .any(|key| item["reference"] == *key)
                    })
                })
                .count();
            wire_report = json!({"bytes":bytes.len(),"missing_evidence":missing,"focus_candidates":focus_candidates,
                "recall_candidates":recall_candidates.len(),"missing_recall_candidates":missing_recall});
        }
        reports.push(json!({"query":args,"expected_evidence":expected,
            "returned_items":result.as_ref().ok().map(|v|&v["items"]),"wire":wire_report,"error":result.err()}));
    }
    std::fs::write(std::env::var("KB_TENDER_CONTEXT_REPORT").unwrap(), serde_json::to_vec_pretty(&json!({
        "mode":"offline projection of exact lookups after delivery of the saved request; no model, reservation, checkpoint or business writes",
        "turn":saved.turn,"require_full_cache":require_full_cache,"reports":reports
    })).unwrap()).unwrap();
    assert_eq!(
        std::fs::read(root.join("extraction/checkpoint.json")).unwrap(),
        original
    );
    assert!(
        reports.iter().all(|r| r["error"].is_null()),
        "exact candidate lookup rejected; inspect offline report"
    );
    assert!(reports.iter().all(|r| {
        r["wire"]["bytes"].as_u64().unwrap() <= config.limits.max_context_bytes as u64
            && r["wire"]["missing_evidence"].as_array().unwrap().is_empty()
    }));
    if require_full_cache {
        assert!(
            reports
                .iter()
                .all(|r| r["wire"]["missing_recall_candidates"]
                    .as_array()
                    .unwrap()
                    .is_empty()),
            "all candidate versions reserved by the recall budget must coexist; inspect offline report"
        );
    } else {
        for report in &reports {
            let wire = &report["wire"];
            let omitted = wire["missing_recall_candidates"].as_array().unwrap().len() as u64;
            assert!(omitted > 0, "fixture must exercise partial-cache pressure");
            assert!(
                wire["recall_candidates"].as_u64().unwrap() - omitted
                    > wire["focus_candidates"].as_u64().unwrap(),
                "retain optional evidence that still fits, rather than discarding the entire cache"
            );
        }
    }
}

#[tokio::test]
#[ignore = "requires KB_TENDER_CONTEXT_REPLAY_DIR, KB_TENDER_CONTEXT_REQUEST and KB_TENDER_CONTEXT_REPORT; offline only"]
async fn scoped_candidate_retrieval_keeps_work_evidence_in_the_recorded_window() {
    use std::path::PathBuf;
    let root = PathBuf::from(std::env::var("KB_TENDER_CONTEXT_REPLAY_DIR").unwrap());
    let checkpoint_path = root.join("extraction/checkpoint.json");
    let original = std::fs::read(&checkpoint_path).unwrap();
    let saved: Checkpoint = serde_json::from_slice(&original).unwrap();
    let input: FrozenInput =
        serde_json::from_slice(&std::fs::read(root.join("input/frozen-input.json")).unwrap())
            .unwrap();
    let old: Value =
        serde_json::from_slice(&std::fs::read(root.join("extraction/runtime.json")).unwrap())
            .unwrap();
    // Offline projection uses only the recorded tuning, not its retired
    // runtime implementation identity. It never resumes that checkpoint.
    let config = Config::with_provider(
        serde_json::from_value(old["provider"].clone()).unwrap(),
        serde_json::from_value(old["limits"].clone()).unwrap(),
    )
    .unwrap();
    let request: Value = serde_json::from_slice(
        &std::fs::read(std::env::var("KB_TENDER_CONTEXT_REQUEST").unwrap()).unwrap(),
    )
    .unwrap();
    let messages = request["messages"].as_array().unwrap();
    let dynamic: Value =
        serde_json::from_str(messages.last().unwrap()["content"].as_str().unwrap()).unwrap();
    let work: WorkState = serde_json::from_value(dynamic["work"].clone()).unwrap();
    let evidence = |transcript: &[Value]| {
        let mut ranges = BTreeMap::<String, Vec<(usize, usize)>>::new();
        let mut cells = BTreeSet::new();
        for message in transcript.iter().filter(|m| m["role"] == "tool") {
            let output: Value = serde_json::from_str(message["content"].as_str().unwrap()).unwrap();
            if output["ok"] != true {
                continue;
            }
            let result = &output["result"];
            if result["text"].is_string()
                && result["source_id"]
                    .as_str()
                    .is_some_and(|id| work.source_scope.iter().any(|s| s == id))
            {
                tools::cover(
                    ranges
                        .entry(result["source_id"].as_str().unwrap().to_owned())
                        .or_default(),
                    result["start"].as_u64().unwrap() as usize,
                    result["end"].as_u64().unwrap() as usize,
                );
            }
            if result["form_id"].is_string()
                && input.structured_forms.iter().any(|f| {
                    f["form_definition_revision_id"] == result["form_id"]
                        && work
                            .source_scope
                            .iter()
                            .any(|s| f["source_unit_revision_id"] == *s)
                })
            {
                for cell in result["cells"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|c| !c.is_null())
                {
                    cells.insert((
                        result["form_id"].as_str().unwrap().to_owned(),
                        cell["row"].as_u64().unwrap(),
                        cell["column"].as_u64().unwrap(),
                    ));
                }
            }
        }
        (ranges, cells)
    };
    let mut base = saved;
    base.role = Role::Main;
    base.turn = dynamic["progress"]["turn"].as_u64().unwrap() as usize;
    base.tool_calls = dynamic["progress"]["tool_calls"].as_u64().unwrap() as usize;
    base.read_bytes = dynamic["progress"]["read_bytes"].as_u64().unwrap() as usize;
    base.main_work = Some(work.clone());
    base.transcript = messages[2..messages.len() - 1].to_vec();
    base.pending_coverage = None;
    base.journal = Default::default();
    let expected = evidence(&base.transcript);
    assert!(!expected.1.is_empty());
    let args = json!({"view":"detail","kind":"all","offset":0,"limit":50});
    let mut reports = vec![];
    for scoped in [false, true] {
        let mut state = base.clone();
        let call = knowledge::models::ChatToolCall {
            id: "diagnostic-inspection".into(),
            name: "inspect_analysis".into(),
            arguments: args.to_string(),
        };
        state.transcript.push(json!({"role":"assistant","content":null,"tool_calls":[{"id":call.id,"type":"function","function":{"name":call.name,"arguments":call.arguments}}]}));
        let page = if scoped {
            let committed = state.coverage().clone();
            super::inspect_in_context(
                &input,
                &config,
                &mut state,
                &args,
                std::slice::from_ref(&call),
                &[],
                &committed,
            )
            .await
            .unwrap()
        } else {
            tools::inspect_analysis(
                &input,
                &state.analysis,
                &mut Coverage::default(),
                &Coverage::default(),
                &args,
                config.limits.max_tool_result_bytes,
                None,
            )
            .unwrap()
        };
        state.transcript.push(json!({"role":"tool","tool_call_id":call.id,"content":json!({"ok":true,"result":page}).to_string()}));
        let body = super::request(&input, &config, &mut state).await.unwrap();
        let actual = evidence(&state.transcript);
        reports.push(json!({"scoped":scoped,"candidate_count":page["total"],"returned_candidates":page["items"].as_array().unwrap().len(),"next":page["next"],"request_bytes":body.len(),
            "tool_result_bytes":serde_json::to_vec(&page).unwrap().len(),"retained_anchors":actual.1.len(),"expected_anchors":expected.1.len(),
            "all_prior_work_evidence_retained":actual == expected}));
        if scoped {
            let total = page["total"].as_u64().unwrap();
            let mut next = page["next"].as_u64().unwrap();
            let mut ids: BTreeSet<_> = page["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v["id"].as_str().unwrap().to_owned())
                .collect();
            let mut pages = 1;
            while next < total {
                let args = json!({"view":"detail","kind":"all","offset":next,"limit":50});
                let call = knowledge::models::ChatToolCall {
                    id: format!("diagnostic-page-{next}"),
                    name: "inspect_analysis".into(),
                    arguments: args.to_string(),
                };
                state.transcript.push(json!({"role":"assistant","content":null,"tool_calls":[{"id":call.id,"type":"function","function":{"name":call.name,"arguments":call.arguments}}]}));
                let committed = state.coverage().clone();
                let page = super::inspect_in_context(
                    &input,
                    &config,
                    &mut state,
                    &args,
                    std::slice::from_ref(&call),
                    &[],
                    &committed,
                )
                .await
                .unwrap();
                state
                    .transcript
                    .push(json!({"role":"tool","tool_call_id":call.id,
                    "content":json!({"ok":true,"result":page}).to_string()}));
                super::request(&input, &config, &mut state).await.unwrap();
                assert_eq!(
                    evidence(&state.transcript),
                    expected,
                    "pagination evicted required source evidence"
                );
                for row in page["items"].as_array().unwrap() {
                    assert!(ids.insert(row["id"].as_str().unwrap().to_owned()));
                }
                let cursor = page["next"].as_u64().unwrap();
                assert!(cursor > next);
                next = cursor;
                pages += 1;
            }
            assert_eq!(ids.len() as u64, total);
            reports.last_mut().unwrap()["pages_to_retrieve_all_candidates"] = json!(pages);
            reports.last_mut().unwrap()["all_candidates_retrieved_without_evidence_eviction"] =
                json!(true);
        }
    }
    std::fs::write(
        std::env::var("KB_TENDER_CONTEXT_REPORT").unwrap(),
        serde_json::to_vec_pretty(&reports).unwrap(),
    )
    .unwrap();
    assert_eq!(std::fs::read(checkpoint_path).unwrap(), original);
    assert_eq!(
        reports[0]["all_prior_work_evidence_retained"], false,
        "fixture must reproduce evidence eviction"
    );
    assert_eq!(reports[1]["all_prior_work_evidence_retained"], true);
}

#[tokio::test]
#[ignore = "requires KB_TENDER_CONTEXT_REPLAY_DIR, KB_TENDER_CONTEXT_REQUEST and KB_TENDER_CONTEXT_REPORT; offline only"]
async fn recorded_overlapping_inspections_fit_without_losing_source_evidence() {
    use std::path::PathBuf;
    let root = PathBuf::from(std::env::var("KB_TENDER_CONTEXT_REPLAY_DIR").unwrap());
    let original = std::fs::read(root.join("extraction/checkpoint.json")).unwrap();
    let mut state: Checkpoint = serde_json::from_slice(&original).unwrap();
    let input: FrozenInput =
        serde_json::from_slice(&std::fs::read(root.join("input/frozen-input.json")).unwrap())
            .unwrap();
    let runtime: Value =
        serde_json::from_slice(&std::fs::read(root.join("extraction/runtime.json")).unwrap())
            .unwrap();
    let config = Config::with_provider(
        serde_json::from_value(runtime["provider"].clone()).unwrap(),
        serde_json::from_value(runtime["limits"].clone()).unwrap(),
    )
    .unwrap();
    let recorded: Value = serde_json::from_slice(
        &std::fs::read(std::env::var("KB_TENDER_CONTEXT_REQUEST").unwrap()).unwrap(),
    )
    .unwrap();
    let messages = recorded["messages"].as_array().unwrap();
    let dynamic: Value =
        serde_json::from_str(messages.last().unwrap()["content"].as_str().unwrap()).unwrap();
    let last = messages
        .iter()
        .rposition(|m| m["role"] == "assistant")
        .unwrap();
    let calls: Vec<knowledge::models::ChatToolCall> = messages[last]["tool_calls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| knowledge::models::ChatToolCall {
            id: c["id"].as_str().unwrap().into(),
            name: c["function"]["name"].as_str().unwrap().into(),
            arguments: c["function"]["arguments"].as_str().unwrap().into(),
        })
        .collect();
    assert!(calls.len() > 1 && calls.iter().all(|c| c.name == "inspect_analysis"));
    state.role = Role::Main;
    state.main_work = Some(serde_json::from_value(dynamic["work"].clone()).unwrap());
    state.turn = dynamic["progress"]["turn"].as_u64().unwrap() as usize - 1;
    state.tool_calls = dynamic["progress"]["tool_calls"].as_u64().unwrap() as usize - calls.len();
    state.read_bytes = dynamic["progress"]["read_bytes"].as_u64().unwrap() as usize;
    state.transcript = messages[2..=last].to_vec();
    state.pending_coverage = None;
    state.journal = Default::default();
    let expected = visible_work_evidence(&state, &state.transcript);
    assert!(!expected.is_empty());
    let mut source_window = state.clone();
    source_window.transcript.pop(); // Replace only the recorded last query batch.
    let mut report = vec![];
    for (index, call) in calls.iter().enumerate() {
        let args: Value = serde_json::from_str(&call.arguments).unwrap();
        let committed = state.coverage().clone();
        let result = super::inspect_in_context(
            &input,
            &config,
            &mut state,
            &args,
            &calls[index..],
            &[],
            &committed,
        )
        .await;
        let output = match result {
            Ok(page) => json!({"ok":true,"result":page}),
            Err(error) => json!({"ok":false,"error":error}),
        };
        report.push(json!({"args":args,"output":output}));
        state
            .transcript
            .push(json!({"role":"tool","tool_call_id":call.id,"content":output.to_string()}));
        super::fit_batch(&input, &config, &mut state, &calls[index + 1..], &[])
            .await
            .unwrap();
    }
    let body = super::request(&input, &config, &mut state).await.unwrap();
    let retained = visible_work_evidence(&state, &state.transcript) == expected;
    let ids: BTreeSet<_> = report
        .iter()
        .flat_map(|r| {
            r["output"]["result"]["items"]
                .as_array()
                .into_iter()
                .flatten()
        })
        .filter_map(|row| row["id"].as_str())
        .collect();
    let mut detail_count = 0;
    for id in ids {
        let mut state = source_window.clone();
        let args = json!({"view":"detail","kind":"all","ids":[id],"offset":0,"limit":1});
        let call = knowledge::models::ChatToolCall {
            id: "offline-single-detail".into(),
            name: "inspect_analysis".into(),
            arguments: args.to_string(),
        };
        state.transcript.push(json!({"role":"assistant","content":null,"tool_calls":[{"id":call.id,"type":"function","function":{"name":call.name,"arguments":call.arguments}}]}));
        let committed = state.coverage().clone();
        let page = super::inspect_in_context(
            &input,
            &config,
            &mut state,
            &args,
            std::slice::from_ref(&call),
            &[],
            &committed,
        )
        .await
        .unwrap();
        assert_eq!(page["items"][0], json!(state.analysis.records[id]));
        state.transcript.push(json!({"role":"tool","tool_call_id":call.id,"content":json!({"ok":true,"result":page}).to_string()}));
        super::request(&input, &config, &mut state).await.unwrap();
        assert_eq!(visible_work_evidence(&state, &state.transcript), expected);
        detail_count += 1;
    }
    std::fs::write(
        std::env::var("KB_TENDER_CONTEXT_REPORT").unwrap(),
        serde_json::to_vec_pretty(&json!({
            "mode":"offline projection only; no model call or checkpoint recovery",
            "request_bytes":body.len(),"all_source_evidence_retained":retained,"single_complete_details_retrieved":detail_count,"calls":report,
        }))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        std::fs::read(root.join("extraction/checkpoint.json")).unwrap(),
        original
    );
    assert!(retained);
    assert!(
        report.iter().all(|r| r["output"]["ok"] == true),
        "overlapping inventory queries must fit alongside original evidence"
    );
}

#[tokio::test]
#[ignore = "requires KB_TENDER_CONTEXT_REPLAY_DIR and KB_TENDER_CONTEXT_REPORT; offline only"]
async fn recorded_reviewer_batch_retains_sources_and_focused_candidates() {
    let root = std::path::PathBuf::from(std::env::var("KB_TENDER_CONTEXT_REPLAY_DIR").unwrap());
    let original = std::fs::read(root.join("extraction/checkpoint.json")).unwrap();
    let mut state: Checkpoint = serde_json::from_slice(&original).unwrap();
    assert_eq!(state.role, Role::Reviewer);
    let input: FrozenInput =
        serde_json::from_slice(&std::fs::read(root.join("input/frozen-input.json")).unwrap())
            .unwrap();
    let runtime: Value =
        serde_json::from_slice(&std::fs::read(root.join("extraction/runtime.json")).unwrap())
            .unwrap();
    let config = Config::with_provider(
        serde_json::from_value(runtime["provider"].clone()).unwrap(),
        serde_json::from_value(runtime["limits"].clone()).unwrap(),
    )
    .unwrap();
    let request: Value =
        serde_json::from_slice(&std::fs::read(root.join("request.json")).unwrap()).unwrap();
    let messages = request["messages"].as_array().unwrap();
    let dynamic: Value =
        serde_json::from_str(messages.last().unwrap()["content"].as_str().unwrap()).unwrap();
    let mut work = dynamic["work"].clone();
    work.as_object_mut().unwrap().remove("saved_outcome_count");
    work.as_object_mut()
        .unwrap()
        .remove("pending_outcome_count");
    state.reviewer_work = Some(serde_json::from_value(work).unwrap());
    let last = messages
        .iter()
        .rposition(|m| m["role"] == "assistant")
        .unwrap();
    let calls: Vec<knowledge::models::ChatToolCall> = messages[last]["tool_calls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| knowledge::models::ChatToolCall {
            id: c["id"].as_str().unwrap().into(),
            name: c["function"]["name"].as_str().unwrap().into(),
            arguments: c["function"]["arguments"].as_str().unwrap().into(),
        })
        .collect();
    assert!(
        calls.len() > 1
            && calls
                .iter()
                .all(|c| matches!(c.name.as_str(), "inspect_analysis" | "inspect_review"))
    );
    state.transcript = messages[2..=last].to_vec();
    for message in &mut state.transcript {
        if let Some(parts) = message["content"].as_array() {
            let ids: Vec<_> = parts
                .iter()
                .filter_map(|p| p["image_url"]["url"].as_str())
                .map(|url| {
                    state
                        .source_views
                        .iter()
                        .find_map(|(id, v)| {
                            (url.strip_prefix("data:image/jpeg;base64,")
                                == Some(v.jpeg_base64.as_str()))
                            .then_some(id.clone())
                        })
                        .expect("original image must match cache")
                })
                .collect();
            if !ids.is_empty() {
                *message = json!({"role":"user","source_view_refs":ids});
            }
        }
    }
    state.turn = dynamic["progress"]["turn"].as_u64().unwrap() as usize - 1;
    state.tool_calls = dynamic["progress"]["tool_calls"].as_u64().unwrap() as usize - calls.len();
    state.read_bytes = dynamic["progress"]["read_bytes"].as_u64().unwrap() as usize;
    state.pending_coverage = None;
    state.journal = Default::default();
    let expected = focused_work_evidence(&state, &state.transcript);
    assert!(expected.keys().any(|k| k.starts_with("candidate:")));
    assert!(
        expected
            .keys()
            .any(|k| k.starts_with("form:") || k.starts_with("text:"))
    );
    let mut report = vec![];
    for (index, call) in calls.iter().enumerate() {
        let args: Value = serde_json::from_str(&call.arguments).unwrap();
        let committed = state.coverage().clone();
        let result = super::inspect_in_context(
            &input,
            &config,
            &mut state,
            &args,
            &calls[index..],
            &[],
            &committed,
        )
        .await;
        let output = match result {
            Ok(page) => json!({"ok":true,"result":page}),
            Err(error) => json!({"ok":false,"error":error}),
        };
        report.push(json!({"args":args,"output":output}));
        state
            .transcript
            .push(json!({"role":"tool","tool_call_id":call.id,"content":output.to_string()}));
        super::fit_batch(&input, &config, &mut state, &calls[index + 1..], &[])
            .await
            .unwrap();
    }
    let body = super::request(&input, &config, &mut state).await.unwrap();
    let wire: Value = serde_json::from_slice(&body).unwrap();
    let actual = focused_work_evidence(&state, wire["messages"].as_array().unwrap());
    let missing: Vec<_> = expected
        .iter()
        .filter(|(key, ranges)| {
            !ranges
                .iter()
                .all(|&(a, b)| tools::contains(actual.get(*key), a, b))
                && !key.strip_prefix("view:").is_some_and(|id| {
                    state.source_views.get(id).is_some_and(|view| {
                        wire["messages"]
                            .as_array()
                            .unwrap()
                            .contains(&view.message())
                    })
                })
        })
        .map(|(k, v)| json!({"key":k,"ranges":v}))
        .collect();
    std::fs::write(std::env::var("KB_TENDER_CONTEXT_REPORT").unwrap(),serde_json::to_vec_pretty(&json!({"mode":"offline current-checkpoint/request-window projection; no model or journal I/O","request_bytes":body.len(),"calls":report,"missing_focused_evidence":missing,"expected":expected,"actual":actual})).unwrap()).unwrap();
    assert_eq!(
        std::fs::read(root.join("extraction/checkpoint.json")).unwrap(),
        original
    );
    assert!(
        missing.is_empty(),
        "focused candidates and source evidence must coexist"
    );
    assert!(
        report.iter().all(|r| r["output"]["ok"] == true),
        "recorded exact candidate lookups must not be rejected"
    );
}

#[tokio::test]
#[ignore = "requires KB_TENDER_CONTEXT_REPLAY_DIR and KB_TENDER_CONTEXT_REPORT; offline only"]
async fn recorded_scope_split_keeps_a_complete_grid_and_original_view_together() {
    use std::path::PathBuf;
    struct NoIo;
    #[async_trait]
    impl Journal for NoIo {
        async fn load(&self) -> Result<Option<Checkpoint>, AgentError> {
            panic!("offline only")
        }
        async fn reserve(&self, _: &Checkpoint, _: &[u8]) -> Result<Option<usize>, AgentError> {
            panic!("offline only")
        }
        async fn save(&self, _: &Checkpoint, _: &Value) -> Result<(), AgentError> {
            panic!("offline only")
        }
    }
    let root = PathBuf::from(std::env::var("KB_TENDER_CONTEXT_REPLAY_DIR").unwrap());
    let original = std::fs::read(root.join("extraction/checkpoint.json")).unwrap();
    let mut state: Checkpoint = serde_json::from_slice(&original).unwrap();
    let input: FrozenInput =
        serde_json::from_slice(&std::fs::read(root.join("input/frozen-input.json")).unwrap())
            .unwrap();
    let old: Value =
        serde_json::from_slice(&std::fs::read(root.join("extraction/runtime.json")).unwrap())
            .unwrap();
    let config = Config::with_provider(
        serde_json::from_value(old["provider"].clone()).unwrap(),
        serde_json::from_value(old["limits"].clone()).unwrap(),
    )
    .unwrap();
    state.journal = Default::default();
    state.pending_coverage = None;
    let before = digest(&state.analysis).unwrap();
    let mut work = state.work().unwrap().clone();
    let prior_scope = work.source_scope.clone();
    let form = input
        .structured_forms
        .iter()
        .find(|f| {
            let id = f["source_unit_revision_id"].as_str().unwrap();
            prior_scope.iter().any(|s| s == id)
                && state
                    .source_views
                    .values()
                    .any(|v| v.identity.source_id == id)
        })
        .unwrap();
    let source_id = form["source_unit_revision_id"].as_str().unwrap();
    work.source_scope = vec![source_id.into()];
    work.deferred_sources = prior_scope
        .iter()
        .filter(|id| *id != source_id)
        .cloned()
        .collect();
    assert!(!work.deferred_sources.is_empty());
    for key in scope_references(&state.analysis, &work.deferred_sources) {
        let refs = if unresolved(&state.analysis, &key) {
            &mut work.pending_refs
        } else {
            &mut work.output_refs
        };
        if !refs.contains(&key) {
            refs.push(key);
        }
    }
    let assistant = |calls: Vec<(&str, &str, Value)>| json!({"role":"assistant","content":null,"tool_calls":calls.into_iter().map(|(id,name,args)| json!({"id":id,"type":"function","function":{"name":name,"arguments":args.to_string()}})).collect::<Vec<_>>()});
    let response = |id: &str, page: &Value| json!({"role":"tool","tool_call_id":id,"content":json!({"ok":true,"result":page}).to_string()});
    state
        .transcript
        .push(assistant(vec![("split", "set_work_note", json!(work))]));
    let mut work_input = json!(work);
    work_input.as_object_mut().unwrap().remove("output_refs");
    work_input.as_object_mut().unwrap().remove("pending_refs");
    let split = super::apply(&input, &config, &mut state, "set_work_note", &work_input).unwrap();
    assert_eq!(split["handoff"], true);
    state.transcript.push(response("split", &split));
    assert_eq!(
        state.transcript.len(),
        2,
        "split must release the old delivered window"
    );
    let grid_args = json!({"form_id":form["form_definition_revision_id"],"offset":0,"limit":config.limits.max_tool_result_bytes});
    let view_args = json!({"source_id":source_id});
    state.transcript.push(assistant(vec![
        ("grid", "read_form", grid_args.clone()),
        ("view", "read_source_view", view_args.clone()),
    ]));
    let grid = tools::invoke(
        &input,
        &mut state.analysis.clone(),
        &mut state.coverage().clone(),
        false,
        "read_form",
        &grid_args,
        config.limits.max_tool_result_bytes,
    )
    .unwrap();
    assert_eq!(grid["next"], grid["total_cells"]);
    let view = super::read_source_view(
        &input,
        &config,
        &mut state,
        &NoIo,
        &view_args,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    state.transcript.push(response("grid", &grid));
    state.transcript.push(response("view", &view));
    state
        .transcript
        .push(json!({"role":"user","source_view_refs":[view["view_id"]]}));
    let expected = visible_work_evidence(&state, &state.transcript);
    let body = super::request(&input, &config, &mut state).await.unwrap();
    assert_eq!(visible_work_evidence(&state, &state.transcript), expected);
    assert_eq!(
        expected.len(),
        2,
        "complete grid plus its actual original image"
    );
    assert_eq!(digest(&state.analysis).unwrap(), before);
    let restored: Checkpoint = serde_json::from_value(json!(state)).unwrap();
    assert_eq!(
        restored.work().unwrap().deferred_sources,
        work.deferred_sources
    );
    assert_eq!(
        std::fs::read(root.join("extraction/checkpoint.json")).unwrap(),
        original
    );
    std::fs::write(std::env::var("KB_TENDER_CONTEXT_REPORT").unwrap(), serde_json::to_vec_pretty(&json!({
        "mode":"offline projection only; no model or journal I/O", "prior_scope_sources":prior_scope.len(),
        "active_sources":work.source_scope,"deferred_sources":work.deferred_sources,"request_bytes":body.len(),
        "complete_grid_cells":grid["total_cells"],"original_view_retained":true,"business_analysis_unchanged":true
    })).unwrap()).unwrap();
}

#[tokio::test]
#[ignore = "requires KB_TENDER_CONTEXT_REPLAY_DIR and KB_TENDER_CONTEXT_REPORT; offline only"]
async fn recorded_history_eviction_preserves_current_parsed_evidence() {
    replay_history_eviction(true).await;
}

#[tokio::test]
#[ignore = "requires KB_TENDER_CONTEXT_REPLAY_DIR and KB_TENDER_CONTEXT_REPORT; offline only"]
async fn recorded_active_original_and_focused_candidates_coexist() {
    replay_history_eviction(false).await;
}

async fn replay_history_eviction(require_all_candidates: bool) {
    use std::path::PathBuf;
    let root = PathBuf::from(std::env::var("KB_TENDER_CONTEXT_REPLAY_DIR").unwrap());
    let original = std::fs::read(root.join("extraction/checkpoint.json")).unwrap();
    let mut state: Checkpoint = serde_json::from_slice(&original).unwrap();
    let input: FrozenInput =
        serde_json::from_slice(&std::fs::read(root.join("input/frozen-input.json")).unwrap())
            .unwrap();
    let runtime: Value =
        serde_json::from_slice(&std::fs::read(root.join("extraction/runtime.json")).unwrap())
            .unwrap();
    let config = Config::with_provider(
        serde_json::from_value(runtime["provider"].clone()).unwrap(),
        serde_json::from_value(runtime["limits"].clone()).unwrap(),
    )
    .unwrap();
    let request: Value =
        serde_json::from_slice(&std::fs::read(root.join("request.json")).unwrap()).unwrap();
    let messages = request["messages"].as_array().unwrap();
    let dynamic: Value =
        serde_json::from_str(messages.last().unwrap()["content"].as_str().unwrap()).unwrap();
    state.transcript = messages[2..messages.len() - 1].to_vec();
    // Archived wire requests contain pixels; checkpoints reference cached
    // views. Restore those references only in this offline projection.
    for message in &mut state.transcript {
        let Some(parts) = message["content"].as_array() else {
            continue;
        };
        let ids: Vec<_> = parts
            .iter()
            .filter_map(|part| part["image_url"]["url"].as_str())
            .map(|url| {
                state
                    .source_views
                    .iter()
                    .find_map(|(id, view)| {
                        (url.strip_prefix("data:image/jpeg;base64,") == Some(&view.jpeg_base64))
                            .then_some(id.clone())
                    })
                    .expect("recorded image must match a cached original")
            })
            .collect();
        if !ids.is_empty() {
            *message = json!({"role":"user","source_view_refs":ids});
        }
    }
    let mut declared = dynamic["work"].clone();
    declared
        .as_object_mut()
        .unwrap()
        .remove("saved_outcome_count");
    declared
        .as_object_mut()
        .unwrap()
        .remove("pending_outcome_count");
    let mut work: WorkState = serde_json::from_value(declared).unwrap();
    retain_outcomes(&state.analysis, &mut work, None);
    let work = Some(work);
    match state.role {
        Role::Main => state.main_work = work,
        Role::Reviewer => state.reviewer_work = work,
    }
    state.turn = dynamic["progress"]["turn"].as_u64().unwrap() as usize;
    state.tool_calls = dynamic["progress"]["tool_calls"].as_u64().unwrap() as usize;
    state.read_bytes = dynamic["progress"]["read_bytes"].as_u64().unwrap() as usize;
    state.journal = Default::default();
    state.pending_coverage = None;
    let response: Value =
        serde_json::from_slice(&std::fs::read(root.join("response-request.json")).unwrap())
            .unwrap();
    let response_messages = response["messages"].as_array().unwrap();
    let last = response_messages
        .iter()
        .rposition(|m| m["role"] == "assistant")
        .unwrap();
    let current_group = response_messages[last..response_messages.len() - 1].to_vec();
    assert!(
        !current_group[0]["tool_calls"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    state.transcript.extend(current_group.clone());
    let completed: Value = serde_json::from_str(
        response_messages.last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    state.turn = completed["progress"]["turn"].as_u64().unwrap() as usize;
    state.tool_calls = completed["progress"]["tool_calls"].as_u64().unwrap() as usize;
    state.read_bytes = completed["progress"]["read_bytes"].as_u64().unwrap() as usize;
    let expected = visible_work_evidence(&state, &state.transcript);
    let focused = focused_work_evidence(&state, &state.transcript);
    if !require_all_candidates {
        assert!(focused.keys().any(|key| key.starts_with("candidate:")));
    }
    let body = super::request(&input, &config, &mut state).await.unwrap();
    let actual = visible_work_evidence(&state, &state.transcript);
    let wire: Value = serde_json::from_slice(&body).unwrap();
    let image_urls: Vec<_> = wire["messages"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|m| m["content"].as_array().into_iter().flatten())
        .filter_map(|item| item["image_url"]["url"].as_str())
        .collect();
    let current_source = dynamic["source_review"]["current"]["task"]["source_id"].as_str();
    let active_images: Vec<_> = state
        .reviewer_coverage
        .views
        .iter()
        .filter(|(_, view)| Some(view.source_id.as_str()) == current_source)
        .map(|(id, _)| &state.source_views[id])
        .collect();
    if !require_all_candidates {
        assert!(
            !active_images.is_empty(),
            "this replay must exercise a previously read active original"
        );
    }
    let active_images_retained = active_images.iter().all(|view| {
        image_urls
            .iter()
            .any(|url| url.strip_prefix("data:image/jpeg;base64,") == Some(&view.jpeg_base64))
    });
    let retained = expected
        .iter()
        .filter(|(key, _)| !key.starts_with("view:"))
        .all(|(key, ranges)| {
            ranges
                .iter()
                .all(|&(a, b)| tools::contains(actual.get(key), a, b))
        });
    let source_ranges_retained = expected
        .iter()
        .filter(|(key, _)| !key.starts_with("candidate:") && !key.starts_with("view:"))
        .all(|(key, ranges)| {
            ranges
                .iter()
                .all(|&(a, b)| tools::contains(actual.get(key), a, b))
        });
    let focus_retained = focused
        .iter()
        .filter(|(key, _)| !key.starts_with("view:"))
        .all(|(key, ranges)| {
            ranges
                .iter()
                .all(|&(a, b)| tools::contains(actual.get(key), a, b))
        });
    std::fs::write(std::env::var("KB_TENDER_CONTEXT_REPORT").unwrap(), serde_json::to_vec_pretty(&json!({
        "mode":"offline projection; no model or journal I/O", "request_bytes":body.len(),
        "expected_evidence":expected,"retained_evidence":actual,"all_parsed_evidence_retained":retained,
        "request_image_count":image_urls.len(),"active_source_images_expected":active_images.len(),
        "active_source_images_retained":active_images_retained,
        "require_all_candidates":require_all_candidates,
        "all_source_ranges_retained":source_ranges_retained,"all_focus_evidence_retained":focus_retained,
        "expected_focus_evidence":focused,
        "latest_tool_calls":current_group[0]["tool_calls"],
        "messages":state.transcript
    })).unwrap()).unwrap();
    assert_eq!(
        std::fs::read(root.join("extraction/checkpoint.json")).unwrap(),
        original
    );
    assert!(
        state.transcript.ends_with(&current_group),
        "new tool outputs must remain verbatim"
    );
    assert!(
        if require_all_candidates {
            retained
        } else {
            source_ranges_retained && focus_retained
        },
        "oversized delivered groups must yield before smaller current parsed evidence"
    );
    assert!(
        active_images_retained,
        "the current reviewer original must remain visible in the wire request"
    );
}

#[test]
fn delivered_line_annotations_preserve_receipts_errors_and_pending_results() {
    let mut state: Checkpoint = serde_json::from_value(json!({
        "dispatch":{"active":null,"entries":{},"last_committed_turn":null},"journal":{"sequence":0,"pending":null,"session":null},
        "input_sha256":"","config_sha256":"","turn":0,"tool_calls":0,"read_bytes":0,
        "review_rounds":0,"role":"main","analysis":Analysis::default(),"review":null,
        "review_draft":{},"reviewer_coverage":Coverage::default(),"pending_coverage":null,
        "transcript":[],"main_work":null,"reviewer_work":null,"done":false,"source_views":{}
    }))
    .unwrap();
    let group = |id: &str, result: Value| {
        vec![
            json!({"role":"assistant","tool_calls":[{"id":id,"function":{"name":"read_source","arguments":"{}"}}]}),
            json!({"role":"tool","tool_call_id":id,"content":result.to_string()}),
        ]
    };
    let source =
        json!({"ok":true,"result":{"source_id":"source","start":6,"end":13,"text":"甲\n乙"}});
    let latest = group("pending", source.clone());
    state.transcript = [
        group("old", source.clone()),
        group("failed", json!({"ok":false,"error":"unread"})),
        latest.clone(),
    ]
    .concat();
    let before = state.clone();
    annotate_delivered_source_lines(&mut state, 1024).unwrap();
    let mut delivered: Value =
        serde_json::from_str(state.transcript[1]["content"].as_str().unwrap()).unwrap();
    assert_eq!(
        delivered["result"]["line_spans"],
        json!([
            {"start":6,"end":10,"text":"甲\n"},{"start":10,"end":13,"text":"乙"}
        ])
    );
    delivered["result"]
        .as_object_mut()
        .unwrap()
        .remove("line_spans");
    assert_eq!(delivered, source, "original evidence remains verbatim");
    assert_eq!(
        state.transcript[2..],
        before.transcript[2..],
        "errors and pending tool group are unchanged"
    );
    assert!(state.transcript.ends_with(&latest));
    let once = state.transcript.clone();
    annotate_delivered_source_lines(&mut state, 1024).unwrap();
    assert_eq!(state.transcript, once);
    state.transcript = before.transcript.clone();
    annotate_delivered_source_lines(&mut state, 1).unwrap();
    assert_eq!(
        json!(state),
        json!(before),
        "annotation cannot change receipts, journals or exceed its result budget"
    );
}

#[test]
fn oversized_delivered_images_release_pixels_without_losing_their_batch_candidates() {
    let mut state = Checkpoint {
        journal: Default::default(),
        input_sha256: String::new(),
        config_sha256: String::new(),
        turn: 3,
        tool_calls: 3,
        read_bytes: 2048,
        review_rounds: 0,
        role: Role::Main,
        analysis: Analysis::default(),
        review: None,
        review_draft: BTreeMap::new(),
        source_review: None,
        repair: Default::default(),
        dispatch: Default::default(),
        reviewer_coverage: Coverage::default(),
        pending_coverage: Some(Coverage::default()),
        transcript: vec![],
        main_progress: Default::default(),
        reviewer_progress: Default::default(),
        main_work: Some(WorkState {
            source_scope: vec!["source".into()],
            deferred_sources: vec![],
            objective: "Compare the grid with its original page".into(),
            focus: Focus::default(),
            output_refs: vec![],
            pending_refs: vec![],
            status: WorkStatus::Active,
            note: String::new(),
        }),
        reviewer_work: None,
        done: false,
        source_views: BTreeMap::new(),
        draft_stage: Default::default(),
        draft_active_id: None,
        draft_compile_object_id: None,
        draft_docx_base64: None,
        outline_config_sha256: None,
        fill_config_sha256: None,
    };
    for id in ["first", "second"] {
        let view = views::SourceView {
            identity: views::ViewIdentity {
                source_id: "source".into(),
                original_sha256: "0".repeat(64),
                image_sha256: id.into(),
                page_ordinal: 0,
                width: 1,
                height: 1,
                renderer: "test-only-sizing".into(),
            },
            // Sizing test only; no decoding or provider I/O.
            jpeg_base64: "A".repeat(1024),
        };
        state
            .analysis
            .coverage
            .views
            .insert(id.into(), view.identity.clone());
        state.source_views.insert(id.into(), view);
        state
            .main_work
            .as_mut()
            .unwrap()
            .focus
            .source_spans
            .push(Span {
                source_id: "source".into(),
                start: 0,
                end: 0,
                view_id: Some(id.into()),
                grid_cell: None,
            });
    }
    let assistant = |id: &str, name: &str| {
        json!({"role":"assistant","tool_calls":[
            {"id":id,"type":"function","function":{"name":name,"arguments":"{}"}}
        ]})
    };
    let result = |id: &str, value: Value| {
        json!({"role":"tool","tool_call_id":id,
        "content":json!({"ok":true,"result":value}).to_string()})
    };
    let grid = vec![
        assistant("grid", "read_form"),
        result(
            "grid",
            json!({
                "source_id":"source","form_id":"grid","offset":0,"next":2,"cells":["条款","完整条件"]
            }),
        ),
    ];
    let candidate = Record {
        id: "candidate".into(),
        sources: vec![Span {
            source_id: "source".into(),
            start: 0,
            end: 3,
            view_id: None,
            grid_cell: None,
        }],
        data: RecordData::Fact {
            name: "项目".into(),
            value: "原文事实".into(),
            scope: "本项目".into(),
        },
    };
    state
        .analysis
        .records
        .insert(candidate.id.clone(), candidate.clone());
    let mut image_calls = assistant("image", "read_source_view");
    image_calls["tool_calls"]
        .as_array_mut()
        .unwrap()
        .push(assistant("candidate", "inspect_analysis")["tool_calls"][0].clone());
    let images = vec![
        image_calls,
        result("image", json!({"view_id":"first"})),
        result("candidate", json!({"view":"detail","items":[candidate]})),
        json!({"role":"user","source_view_refs":["first","second"]}),
    ];
    let latest = vec![
        assistant("write", "put_record"),
        result("write", json!({"id":"saved"})),
    ];
    state.transcript = [grid.clone(), images.clone(), latest.clone()].concat();
    let before = state.clone();
    // Each image fits alone; together their payloads cannot fit history.
    assert!(evict_delivered_group(&mut state, 1500, false));
    assert!(state.transcript.starts_with(&grid));
    assert!(state.transcript.ends_with(&latest));
    assert_eq!(
        state
            .transcript
            .iter()
            .find(|m| m["tool_call_id"] == "candidate"),
        Some(&images[2])
    );
    assert_eq!(
        &state.transcript[grid.len()..grid.len() + 3],
        &images[..3],
        "delivered pixels must not evict the current candidate and its complete tool protocol"
    );
    assert!(
        !state
            .transcript
            .iter()
            .any(|m| m.get("source_view_refs").is_some())
    );
    assert_eq!(json!(state.analysis), json!(before.analysis));
    assert_eq!(
        json!(state.reviewer_coverage),
        json!(before.reviewer_coverage)
    );
    assert_eq!(
        json!(state.pending_coverage),
        json!(before.pending_coverage)
    );
    assert_eq!(json!(state.source_views), json!(before.source_views));
    assert_eq!(state.read_bytes, before.read_bytes);

    // Even a smaller old image must yield before unique parsed evidence
    // when the caller has exhausted lossless history compaction.
    state = before.clone();
    assert!(!evict_delivered_group(&mut state, 4096, false));
    assert_eq!(json!(state), json!(before));
    assert!(evict_delivered_group(&mut state, 4096, true));
    assert!(state.transcript.starts_with(&grid));
    assert_eq!(&state.transcript[grid.len()..grid.len() + 3], &images[..3]);
    assert!(
        !state
            .transcript
            .iter()
            .any(|m| m.get("source_view_refs").is_some())
    );
    assert_eq!(json!(state.source_views), json!(before.source_views));
    assert_eq!(
        json!(state.analysis.coverage),
        json!(before.analysis.coverage)
    );
    state = before.clone();
    state.main_work.as_mut().unwrap().focus.source_spans.clear();
    assert!(evict_delivered_group(&mut state, 4096, true));
    assert!(
        state.transcript.starts_with(&grid),
        "incidental old images must yield before focused parsed evidence"
    );

    // An oversized latest image batch is pending delivery, never an old
    // group to evict. The caller handles its separate backpressure.
    state.transcript = [grid, images.clone()].concat();
    assert!(evict_delivered_group(&mut state, 1500, true));
    assert_eq!(state.transcript, images);
    assert!(!evict_delivered_group(&mut state, 1500, true));
}

#[test]
fn navigation_compaction_preserves_sources_details_metadata_and_pending_delivery() {
    let output = |result: Value| json!({"ok":true,"result":result}).to_string();
    let large = "navigation metadata ".repeat(100);
    let calls = [
        ("source", "read_form"),
        ("index", "source_index"),
        ("metadata", "collection_index"),
        ("detail", "inspect_analysis"),
        ("candidates", "inspect_analysis"),
        ("failed", "search_sources"),
    ];
    let mut transcript = vec![
        json!({"role":"assistant","tool_calls":calls.iter().map(|(id,name)|json!({"id":id,"function":{"name":name,"arguments":"{}"}})).collect::<Vec<_>>()}),
    ];
    for (id, result) in [
        ("source", output(json!({"cells":[large]}))),
        ("index", output(json!({"items":[large]}))),
        ("metadata", output(json!({"items":[large]}))),
        ("detail", output(json!({"view":"detail","items":[large]}))),
        (
            "candidates",
            output(json!({"view":"index","items":[large]})),
        ),
        ("failed", json!({"ok":false,"error":large}).to_string()),
    ] {
        transcript.push(json!({"role":"tool","tool_call_id":id,"content":result}));
    }
    transcript.push(json!({"role":"assistant","tool_calls":[{"id":"pending","function":{"name":"source_index","arguments":"{}"}}]}));
    transcript.push(
        json!({"role":"tool","tool_call_id":"pending","content":output(json!({"items":[large]}))}),
    );
    let before = transcript.clone();
    assert!(compact_delivered_navigation(&mut transcript));
    assert!(compact_delivered_navigation(&mut transcript));
    assert!(!compact_delivered_navigation(&mut transcript));
    for (old, new) in before.iter().zip(&transcript) {
        if matches!(new["tool_call_id"].as_str(), Some("index" | "candidates")) {
            let value: Value = serde_json::from_str(new["content"].as_str().unwrap()).unwrap();
            assert_eq!(value["result"]["history_omitted"], true);
            assert_eq!(old["tool_call_id"], new["tool_call_id"]);
        } else {
            assert_eq!(
                old, new,
                "evidence, errors and the latest batch must stay verbatim"
            );
        }
    }
}

#[test]
#[ignore = "requires KB_AGENT_LOOP_FIXTURE_DIR and KB_AGENT_LOOP_REPORT; offline archived regression"]
fn archived_stall_and_four_ready_relationships_regress_without_resuming_old_run() {
    use std::{fs, path::PathBuf};
    let root = PathBuf::from(std::env::var("KB_AGENT_LOOP_FIXTURE_DIR").unwrap());
    let run = root.join("real-run-v13-resume2");
    let original = fs::read(run.join("extraction/checkpoint.json")).unwrap();
    let input: FrozenInput =
        serde_json::from_slice(&fs::read(run.join("source/frozen-input.json")).unwrap()).unwrap();
    let runtime: Value =
        serde_json::from_slice(&fs::read(run.join("extraction/runtime.json")).unwrap()).unwrap();
    let config = Config::with_provider(
        serde_json::from_value(runtime["provider"].clone()).unwrap(),
        serde_json::from_value(runtime["limits"].clone()).unwrap(),
    )
    .unwrap();
    let mut state: Checkpoint = serde_json::from_slice(&original).unwrap();
    let analysis_before = digest(&state.analysis).unwrap();
    synchronize_outcomes(&mut state);
    let rows = completion_gaps(
        &input,
        &state,
        state.work().unwrap(),
        config.limits.max_tool_result_bytes,
    )
    .unwrap();
    assert!(!rows.iter().any(|g| matches!(
        g["kind"].as_str(),
        Some("unretained_outcome" | "unresolved_outcome")
    )));
    for _ in 0..28 {
        observe_progress(&mut state, &Role::Main, None, &config.limits).unwrap();
    }
    assert_eq!(state.main_progress.watch.recovery, Recovery::Blocked);
    assert_eq!(
        digest(&state.analysis).unwrap(),
        analysis_before,
        "stagnation is not source uncertainty"
    );
    let pairs: Vec<Value> = serde_json::from_slice(
        &fs::read(root.join("inspection-loop-diagnosis/pairs.json")).unwrap(),
    )
    .unwrap();
    let mut state: Checkpoint = serde_json::from_slice(&original).unwrap();
    let mut checks = vec![];
    for pair in pairs {
        let mut work = state.work().unwrap().clone();
        work.focus = Focus {
            action: FocusAction::Link,
            source_spans: vec![],
            references: vec![
                format!("record:{}", pair["from"].as_str().unwrap()),
                format!("record:{}", pair["to"].as_str().unwrap()),
            ],
        };
        super::apply(
            &input,
            &config,
            &mut state,
            "set_work_note",
            &work_input(&work).unwrap(),
        )
        .unwrap();
        let args = json!({"id":null,"from":pair["from"],"to":pair["to"],"from_target":{"kind":"record"},"to_target":{"kind":"record"},"kind":"references","state":"explicit","scope":"offline archived regression","explanation":"正文明确指向前附表，对照已保存的正文记录和前附表事实。","grounds":pair["grounds"]});
        let out = super::apply(&input, &config, &mut state, "put_relation", &args).unwrap();
        let completion = focused_completion(&state, "put_relation", &out).unwrap();
        assert!(completion.is_some());
        observe_progress(&mut state, &Role::Main, completion, &config.limits).unwrap();
        assert_eq!(state.main_progress.watch.focus_turns, 0);
        assert!(
            state
                .work()
                .unwrap()
                .output_refs
                .contains(&format!("relation:{}", out["id"].as_str().unwrap()))
        );
        checks.push(json!({"label":pair["label"],"validated_write":true,"automatic_reference":true,"focused_action_completed":true}));
    }
    assert_eq!(checks.len(), 4);
    assert_eq!(
        fs::read(run.join("extraction/checkpoint.json")).unwrap(),
        original
    );
    fs::write(std::env::var("KB_AGENT_LOOP_REPORT").unwrap(),serde_json::to_vec_pretty(&json!({"checks":checks,"twenty_eight_turn_stall":"execution_blocked","old_checkpoint_unchanged":true,"model_calls":0,"semantic_acceptance":"not assessed; fixture-selected diagnostic relationships are not an Agent result"})).unwrap()).unwrap();
}

#[test]
fn unique_candidate_versions_are_not_navigation_and_focused_pairs_survive() {
    let analysis = Analysis::default();
    let mut state: Checkpoint = serde_json::from_value(json!({
        "dispatch":{"active":null,"entries":{},"last_committed_turn":null},"journal":{"sequence":0,"pending":null,"session":null},"input_sha256":"","config_sha256":"",
        "turn":0,"tool_calls":0,"read_bytes":0,"review_rounds":0,"role":"main","analysis":analysis,
        "review":null,"review_draft":{},"reviewer_coverage":Coverage::default(),"pending_coverage":null,
        "transcript":[],"main_work":{"source_scope":["source"],"objective":"compare endpoints",
            "focus":{"action":"link","source_spans":[],"references":["record:left","record:right"]},"status":"active","note":""},
        "reviewer_work":null,"done":false,"source_views":{}
    })).unwrap();
    for id in ["left", "right"] {
        state.analysis.records.insert(
            id.into(),
            Record {
                id: id.into(),
                sources: vec![Span {
                    source_id: "source".into(),
                    start: 0,
                    end: 3,
                    view_id: None,
                    grid_cell: None,
                }],
                data: RecordData::Unresolved {
                    problem: id.into(),
                    affected: vec![],
                    candidates: vec![],
                },
            },
        );
    }
    let group = |id: &str, result: Value| {
        vec![
            json!({"role":"assistant","tool_calls":[{"id":id,"type":"function","function":{"name":"inspect_analysis","arguments":"{}"}}]}),
            json!({"role":"tool","tool_call_id":id,"content":json!({"ok":true,"result":result}).to_string()}),
        ]
    };
    let mut recall = state.clone();
    assert!(
        retained_candidate_message(&recall, 4096).unwrap().is_null(),
        "stored candidate data is not a role-local reading receipt"
    );
    let left = reference(&recall.analysis, "record:left").unwrap();
    recall
        .analysis
        .coverage
        .candidate
        .insert("record:left".into(), digest(&left).unwrap());
    let before = digest(&recall).unwrap();
    let message = retained_candidate_message(&recall, 4096).unwrap();
    let payload: Value = serde_json::from_str(message["content"].as_str().unwrap()).unwrap();
    assert_eq!(
        payload["retained_candidate_details"]["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        payload["retained_candidate_details"]["items"][0]["value"],
        left
    );
    assert_eq!(
        digest(&recall).unwrap(),
        before,
        "recall creates no state, receipt or comparison"
    );
    assert!(message["content"].as_str().unwrap().len() <= 4096);
    let mut legacy = payload.clone();
    legacy["retained_candidate_details"]["items"][0]["sha256"] = json!(digest(&left).unwrap());
    let legacy_message = |value: &Value| json!({"role":"user","content":value.to_string()});
    assert_eq!(
        visible_work_evidence(&recall, &[legacy_message(&legacy)]).len(),
        1
    );
    legacy["retained_candidate_details"]["items"][0]["sha256"] = json!("wrong");
    assert!(visible_work_evidence(&recall, &[legacy_message(&legacy)]).is_empty());
    let mut altered = payload.clone();
    altered["retained_candidate_details"]["items"][0]["value"]["data"]["problem"] =
        json!("altered");
    assert!(
        visible_work_evidence(&recall, &[legacy_message(&altered)]).is_empty(),
        "omitting a redundant sent digest must not accept changed candidate data"
    );
    assert!(retained_candidate_message(&recall, 1).unwrap().is_null());
    recall.main_work.as_mut().unwrap().status = WorkStatus::Complete;
    assert!(
        retained_candidate_message(&recall, 4096).unwrap().is_null(),
        "completed work must release recalled details"
    );
    recall.main_work.as_mut().unwrap().status = WorkStatus::Active;
    recall.pending_coverage = Some(recall.analysis.coverage.clone());
    let received = recall.analysis.coverage.clone();
    recall.analysis.coverage.candidate.clear();
    assert!(
        retained_candidate_message(&recall, 4096).unwrap().is_null(),
        "pending delivery alone cannot authorize recall"
    );
    recall.analysis.coverage = received;
    recall.pending_coverage = None;
    recall.reviewer_work = recall.main_work.clone();
    recall.role = Role::Reviewer;
    assert!(
        retained_candidate_message(&recall, 4096).unwrap().is_null(),
        "a reviewer cannot recall the main role's evidence"
    );
    let mut assigned = recall.clone();
    assigned.reviewer_work.as_mut().unwrap().focus.references = vec!["record:left".into()];
    assigned.reviewer_coverage = assigned.analysis.coverage.clone();
    assigned.analysis.records.get_mut("right").unwrap().sources[0].source_id = "other".into();
    let right = reference(&assigned.analysis, "record:right").unwrap();
    assigned
        .reviewer_coverage
        .candidate
        .insert("record:right".into(), digest(&right).unwrap());
    assigned.source_review = Some(
        serde_json::from_value(json!({
            "schema_version":1,"manifest_sha256":"fixture","results":{},"active_task":"task",
            "dependencies":{"source_ids":["source"],"references":["record:right"],"global":false},
            "finding_revisions":{},"candidate_revisions":{},"completed_analysis_sha256":null
        }))
        .unwrap(),
    );
    let before = digest(&assigned).unwrap();
    let recalled = retained_candidate_message(&assigned, 4096).unwrap();
    let payload: Value = serde_json::from_str(recalled["content"].as_str().unwrap()).unwrap();
    assert!(
        payload["retained_candidate_details"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["reference"] == "record:right"),
        "already-delivered assigned endpoints must survive manual focus changes"
    );
    assert_eq!(
        digest(&assigned).unwrap(),
        before,
        "recall grants no source access or evidence receipt"
    );
    assigned.source_review.as_mut().unwrap().active_task = None;
    let recalled = retained_candidate_message(&assigned, 4096).unwrap();
    let payload: Value = serde_json::from_str(recalled["content"].as_str().unwrap()).unwrap();
    assert_eq!(
        payload["retained_candidate_details"]["total"], 1,
        "inactive assignments release their candidate details"
    );
    assigned.source_review.as_mut().unwrap().active_task = Some("task".into());
    assigned.transcript = group(
        "old-candidates",
        json!({"view":"detail","items":[left, right]}),
    );
    assigned.transcript[0]["tool_calls"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "id":"original","type":"function","function":{"name":"read_source","arguments":"{}"}
        }));
    let original = json!({"role":"tool","tool_call_id":"original","content":json!({
        "ok":true,"result":{"source_id":"source","start":0,"end":3,"text":"原"}
    }).to_string()});
    assigned.transcript.push(original.clone());
    let latest = group("latest", json!({"view":"index","items":[]}));
    assigned.transcript.extend(latest.clone());
    let coverage = digest(&assigned.reviewer_coverage).unwrap();
    let before = assigned.transcript.clone();
    assert!(!compact_recallable_candidate_details(&mut assigned, 1));
    assert_eq!(
        assigned.transcript, before,
        "no recall capacity must preserve focused evidence"
    );
    assert!(compact_recallable_candidate_details(&mut assigned, 4096));
    assert!(
        assigned.transcript.contains(&original),
        "candidate recall must preserve the original sharing its batch"
    );
    assert!(
        assigned.transcript.ends_with(&latest),
        "pending tool results stay verbatim"
    );
    let recalled = retained_candidate_message(&assigned, 4096).unwrap();
    let payload: Value = serde_json::from_str(recalled["content"].as_str().unwrap()).unwrap();
    assert_eq!(payload["retained_candidate_details"]["total"], 2);
    assert_eq!(digest(&assigned.reviewer_coverage).unwrap(), coverage);
    recall.role = Role::Main;
    recall.transcript = group("visible", json!({"view":"detail","items":[left]}));
    assert!(
        retained_candidate_message(&recall, 4096).unwrap().is_null(),
        "visible detail should not be duplicated"
    );
    recall.transcript.clear();
    recall.analysis.records.get_mut("left").unwrap().data = RecordData::Unresolved {
        problem: "revised".into(),
        affected: vec![],
        candidates: vec![],
    };
    assert!(
        retained_candidate_message(&recall, 4096).unwrap().is_null(),
        "a stale receipt cannot recall an unread new version"
    );
    recall.analysis.records.remove("left");
    assert!(
        retained_candidate_message(&recall, 4096).unwrap().is_null(),
        "deleted candidates cannot be resurrected from old receipts"
    );
    let mut mixed = state.clone();
    let mut dormant = mixed.analysis.records["left"].clone();
    dormant.id = "dormant".into();
    dormant.data = RecordData::Unresolved {
        problem: "earlier independent candidate ".repeat(100),
        affected: vec![],
        candidates: vec![],
    };
    mixed
        .analysis
        .records
        .insert(dormant.id.clone(), dormant.clone());
    mixed.transcript.extend(group(
        "mixed",
        json!({"view":"detail","total":3,"next":2,
        "items":[mixed.analysis.records["left"],dormant]}),
    ));
    let source = json!({"role":"tool","tool_call_id":"source","content":json!({"ok":true,"result":{"source_id":"source","start":0,"end":3,"text":"原"}}).to_string()});
    mixed.transcript[0]["tool_calls"].as_array_mut().unwrap().push(json!({"id":"source","type":"function","function":{"name":"read_source","arguments":"{}"}}));
    mixed.transcript.push(source.clone());
    mixed.transcript.extend(group(
        "latest",
        json!({"view":"detail","items":[mixed.analysis.records["right"],dormant]}),
    ));
    let before_mixed = mixed.clone();
    let expected = focused_work_evidence(&mixed, &mixed.transcript);
    assert!(evict_delivered_group(&mut mixed, 65536, true));
    assert_eq!(focused_work_evidence(&mixed, &mixed.transcript), expected);
    assert_eq!(mixed.transcript.len(), before_mixed.transcript.len());
    assert_eq!(mixed.transcript[0], before_mixed.transcript[0]);
    assert_eq!(mixed.transcript[2], source);
    assert_eq!(
        &mixed.transcript[3..],
        &before_mixed.transcript[3..],
        "latest pending details must remain exact"
    );
    let compact: Value =
        serde_json::from_str(mixed.transcript[1]["content"].as_str().unwrap()).unwrap();
    assert_eq!(
        compact["result"]["items"],
        json!([mixed.analysis.records["left"]])
    );
    assert_eq!(compact["result"]["history_omitted_items"], 1);
    assert_eq!(compact["result"]["total"], 3);
    assert_eq!(compact["result"]["next"], 2);
    assert_eq!(json!(mixed.analysis), json!(before_mixed.analysis));
    assert_eq!(
        json!(mixed.reviewer_coverage),
        json!(before_mixed.reviewer_coverage)
    );
    state.transcript.extend(group(
        "left",
        json!({"view":"detail","items":[state.analysis.records["left"]]}),
    ));
    state
        .transcript
        .extend(group("navigation", json!({"view":"index","items":[]})));
    state.transcript.extend(group(
        "right",
        json!({"view":"detail","items":[state.analysis.records["right"]]}),
    ));
    state
        .transcript
        .extend(group("pending", json!({"view":"index","items":[]})));
    let pending = state.transcript[state.transcript.len() - 2..].to_vec();
    let evidence = focused_work_evidence(&state, &state.transcript);
    assert_eq!(evidence.len(), 2);
    assert!(evict_delivered_group(&mut state, 65536, true));
    assert_eq!(focused_work_evidence(&state, &state.transcript), evidence);
    assert!(
        !state
            .transcript
            .iter()
            .any(|m| m["tool_call_id"] == "navigation")
    );
    assert_eq!(
        &state.transcript[state.transcript.len() - 2..],
        pending.as_slice()
    );
    state.analysis.records.get_mut("left").unwrap().data = RecordData::Unresolved {
        problem: "changed version".into(),
        affected: vec![],
        candidates: vec![],
    };
    assert_eq!(
        visible_work_evidence(&state, &state.transcript).len(),
        1,
        "old candidate bodies cannot stand in for current versions"
    );
    assert!(evict_delivered_group(&mut state, 65536, true));
    assert!(!state.transcript.iter().any(|m| m["tool_call_id"] == "left"));
}

#[test]
fn token_estimate_counts_images_separately_and_preserves_utf8_and_tools() {
    let limits = crate::tender_analysis::tests::config().limits;
    let mut body = json!({"messages":[{"role":"user","content":[
        {"type":"text","text":"中文😀"},
        {"type":"image_url","image_url":{"url":"data:image/jpeg;base64,AAAA","detail":"high"}}
    ]}],"tools":[{"type":"function","function":{"name":"read_source"}}]});
    let before = body.clone();
    let estimate = estimate_input_tokens(&body, &limits).unwrap();
    assert_eq!(body, before, "sizing must not replace transmitted pixels");
    body["messages"][0]["content"][1]["image_url"]["url"] = json!("A".repeat(200000));
    assert_eq!(estimate_input_tokens(&body, &limits).unwrap(), estimate);
    body["messages"][0]["content"][0]["text"] = json!("中文😀中文😀");
    assert_eq!(
        estimate_input_tokens(&body, &limits).unwrap(),
        estimate + "中文😀".len()
    );
    body["tools"][0]["function"]["description"] = json!("真实工具定义");
    assert!(estimate_input_tokens(&body, &limits).unwrap() > estimate + "中文😀".len());
    let image = body["messages"][0]["content"][1].clone();
    let single = estimate_input_tokens(&body, &limits).unwrap();
    body["messages"][0]["content"]
        .as_array_mut()
        .unwrap()
        .push(image);
    assert!(estimate_input_tokens(&body, &limits).unwrap() >= single + limits.image_token_reserve);
}
