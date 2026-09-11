use super::*;
use std::collections::BTreeSet;

/// Explicit conservative estimate, not a provider tokenizer. Count all JSON
/// text bytes, replacing each image URL with the configured visual allowance.
/// Base64 is transport encoding, not text sent through the model tokenizer.
pub(super) fn estimate_input_tokens(body: &Value, limits: &Limits) -> Result<usize, AgentError> {
    crate::agent_runtime::chat::estimate_input_tokens(
        body,
        limits.image_token_reserve,
        limits.token_safety_margin,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let rows = completion_gaps(&input, &state, work).unwrap();
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
                let output: Value =
                    serde_json::from_str(message["content"].as_str().unwrap()).unwrap();
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
        state.tool_calls =
            dynamic["progress"]["tool_calls"].as_u64().unwrap() as usize - calls.len();
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
        assert!(calls.len() > 1 && calls.iter().all(|c| c.name == "inspect_analysis"));
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
        state.tool_calls =
            dynamic["progress"]["tool_calls"].as_u64().unwrap() as usize - calls.len();
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
        let split =
            super::apply(&input, &config, &mut state, "set_work_note", &work_input).unwrap();
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
            "journal":{"sequence":0,"pending":null,"session":null},
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
        transcript.push(json!({"role":"tool","tool_call_id":"pending","content":output(json!({"items":[large]}))}));
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
            serde_json::from_slice(&fs::read(run.join("source/frozen-input.json")).unwrap())
                .unwrap();
        let runtime: Value =
            serde_json::from_slice(&fs::read(run.join("extraction/runtime.json")).unwrap())
                .unwrap();
        let config = Config::with_provider(
            serde_json::from_value(runtime["provider"].clone()).unwrap(),
            serde_json::from_value(runtime["limits"].clone()).unwrap(),
        )
        .unwrap();
        let mut state: Checkpoint = serde_json::from_slice(&original).unwrap();
        let analysis_before = digest(&state.analysis).unwrap();
        synchronize_outcomes(&mut state);
        let rows = completion_gaps(&input, &state, state.work().unwrap()).unwrap();
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
            "journal":{"sequence":0,"pending":null,"session":null},"input_sha256":"","config_sha256":"",
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
        assigned.source_review = Some(serde_json::from_value(json!({
            "schema_version":1,"manifest_sha256":"fixture","results":{},"active_task":"task",
            "dependencies":{"source_ids":["source"],"references":["record:right"],"global":false},
            "finding_revisions":{},"candidate_revisions":{},"completed_analysis_sha256":null
        })).unwrap());
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
        assert!(
            estimate_input_tokens(&body, &limits).unwrap() >= single + limits.image_token_reserve
        );
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkStatus {
    Active,
    Complete,
    Blocked,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FocusAction {
    #[default]
    Locate,
    Extract,
    Link,
    Review,
    Handoff,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Focus {
    pub action: FocusAction,
    pub source_spans: Vec<Span>,
    pub references: Vec<String>,
}

/// Role-local continuation state. All referenced work remains in the analysis;
/// this is neither a second task queue nor evidence of semantic correctness.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkState {
    pub source_scope: Vec<String>,
    #[serde(default)]
    pub deferred_sources: Vec<String>,
    pub objective: String,
    #[serde(default)]
    pub focus: Focus,
    #[serde(default)]
    pub output_refs: Vec<String>,
    #[serde(default)]
    pub pending_refs: Vec<String>,
    pub status: WorkStatus,
    pub note: String,
}

pub(super) fn reference(analysis: &Analysis, key: &str) -> Result<Value, String> {
    let (kind, id) = key
        .split_once(':')
        .ok_or("use record:<id>, relation:<id>, or disposition:<source_id> for a work reference, not a bare ID")?;
    match kind {
        "record" => analysis.records.get(id).map(|v| json!(v)),
        "relation" => analysis.relations.get(id).map(|v| json!(v)),
        "disposition" => analysis
            .dispositions
            .get(id)
            .map(|v| json!({"source_id":id,"disposition":v})),
        _ => None,
    }
    .ok_or_else(|| format!("unknown work reference: {key}"))
}

fn unresolved(analysis: &Analysis, key: &str) -> bool {
    let Some((kind, id)) = key.split_once(':') else {
        return false;
    };
    match kind {
        "record" => analysis
            .records
            .get(id)
            .is_some_and(|r| matches!(r.data, RecordData::Unresolved { .. })),
        "relation" => analysis
            .relations
            .get(id)
            .is_some_and(|r| r.state == RelationState::Unresolved),
        "disposition" => analysis
            .dispositions
            .get(id)
            .is_some_and(|d| d.state == DispositionState::Unresolved),
        _ => false,
    }
}

pub(super) fn scope_references(analysis: &Analysis, scope: &[String]) -> Vec<String> {
    let record_in_scope = |r: &Record| r.sources.iter().any(|s| scope.contains(&s.source_id));
    let mut refs: Vec<_> = analysis
        .records
        .iter()
        .filter(|(_, r)| record_in_scope(r))
        .map(|(id, _)| format!("record:{id}"))
        .collect();
    refs.extend(
        analysis
            .relations
            .iter()
            .filter(|(_, r)| {
                r.grounds.iter().any(|s| scope.contains(&s.source_id))
                    || [&r.from, &r.to]
                        .iter()
                        .any(|id| analysis.records.get(*id).is_some_and(record_in_scope))
            })
            .map(|(id, _)| format!("relation:{id}")),
    );
    refs.extend(
        scope
            .iter()
            .filter(|id| analysis.dispositions.contains_key(*id))
            .map(|id| format!("disposition:{id}")),
    );
    refs
}

/// Source judgments also depend on cross-source endpoints and actual queries.
/// Navigation must not declare comparisons finished while those remain pending.
fn work_references(state: &Checkpoint, work: &WorkState) -> Vec<String> {
    let mut refs = scope_references(&state.analysis, &work.source_scope);
    if state.role == Role::Reviewer
        && let Some(review) = state
            .source_review
            .as_ref()
            .filter(|r| r.active_task.is_some())
    {
        for key in source_review::references(&state.analysis, &review.dependencies) {
            if !refs.contains(&key) {
                refs.push(key);
            }
        }
    }
    refs
}

/// Outcomes are host-maintained, including deferred work and unresolved cross-scope
/// dependencies. They are references only, never reading receipts or approval.
pub(super) fn retain_outcomes(
    analysis: &Analysis,
    work: &mut WorkState,
    prior: Option<&WorkState>,
) {
    let scope: Vec<_> = work
        .source_scope
        .iter()
        .chain(&work.deferred_sources)
        .cloned()
        .collect();
    let mut refs: BTreeSet<_> = scope_references(analysis, &scope).into_iter().collect();
    refs.extend(work.output_refs.iter().chain(&work.pending_refs).cloned());
    if let Some(prior) = prior {
        refs.extend(
            prior
                .pending_refs
                .iter()
                .filter(|key| unresolved(analysis, key))
                .cloned(),
        );
    }
    work.output_refs = refs
        .iter()
        .filter(|key| reference(analysis, key).is_ok() && !unresolved(analysis, key))
        .cloned()
        .collect();
    work.pending_refs = refs
        .into_iter()
        .filter(|key| unresolved(analysis, key))
        .collect();
}

pub(super) fn synchronize_outcomes(state: &mut Checkpoint) {
    for work in [&mut state.main_work, &mut state.reviewer_work]
        .into_iter()
        .flatten()
    {
        retain_outcomes(&state.analysis, work, None);
    }
}

fn work_input(work: &WorkState) -> Result<Value, String> {
    let mut value = serde_json::to_value(work).map_err(|e| e.to_string())?;
    let object = value.as_object_mut().ok_or("work object missing")?;
    object.remove("output_refs");
    object.remove("pending_refs");
    Ok(value)
}

pub(super) fn request_work(state: &Checkpoint) -> Result<Value, String> {
    let Some(work) = state.work() else {
        return Ok(Value::Null);
    };
    let mut value = work_input(work)?;
    value["saved_outcome_count"] = json!(work.output_refs.len());
    value["pending_outcome_count"] = json!(work.pending_refs.len());
    Ok(value)
}

pub(super) fn execution_gaps(
    state: &Checkpoint,
    args: &Value,
    max_bytes: usize,
) -> Result<Value, String> {
    if args.as_object().is_none_or(|o| {
        o.keys()
            .any(|key| !matches!(key.as_str(), "scope" | "offset" | "limit"))
    }) {
        return Err("gap query accepts only scope, offset and limit".into());
    }
    let number = |key: &str| {
        args[key]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| format!("{key} must be a nonnegative integer"))
    };
    let rows: Vec<_> = if args["scope"] == "pending" {
        state.work().into_iter().flat_map(|w|&w.pending_refs).map(|key|json!({"reference":key,"kind":"source_uncertainty","instruction":"inspect this saved outcome when its content is needed; it is not an execution failure"})).collect()
    } else {
        state.execution().blockers.iter().map(|b|json!({"source_scope":b.scope,"dependencies_sha256":b.dependencies_sha256,"kind":"execution_blocker"})).collect()
    };
    tools::bounded_page(&rows, number("offset")?, number("limit")?, max_bytes)
}

pub(super) fn validate(
    input: &FrozenInput,
    state: &Checkpoint,
    next: &WorkState,
    max_bytes: usize,
) -> Result<(), String> {
    if next.source_scope.is_empty()
        || next.objective.trim().is_empty()
        || serde_json::to_vec(&work_input(next)?)
            .map_err(|e| e.to_string())?
            .len()
            > max_bytes
    {
        return Err(
            "work needs a nonempty source scope/objective and must fit the work budget".into(),
        );
    }
    for list in [
        &next.source_scope,
        &next.deferred_sources,
        &next.output_refs,
        &next.pending_refs,
        &next.focus.references,
    ] {
        if list.iter().collect::<BTreeSet<_>>().len() != list.len() {
            return Err("work references and source IDs must be distinct".into());
        }
    }
    for id in next.source_scope.iter().chain(&next.deferred_sources) {
        if !input
            .source_units
            .iter()
            .any(|s| &s.source_unit_revision_id == id)
        {
            return Err(format!("unknown work source: {id}"));
        }
    }
    if next
        .deferred_sources
        .iter()
        .any(|id| next.source_scope.contains(id))
    {
        return Err("a source cannot be both active and deferred".into());
    }
    for key in next.output_refs.iter().chain(&next.pending_refs) {
        reference(&state.analysis, key)?;
    }
    for span in &next.focus.source_spans {
        if !next.source_scope.contains(&span.source_id) {
            return Err("focus evidence must belong to the permitted source scope".into());
        }
        // A work note plans where to read; it is not a submitted citation.
        // Writes and review outcomes still validate delivered role coverage.
        if span.grid_cell.is_some() {
            tools::validate_grid_span(input, span)?;
        } else if span.view_id.is_none() {
            tools::validate_text_span(input, span)?;
        } else {
            tools::validate_span(input, state.coverage(), span)?;
        }
    }
    let allowed_focus = work_references(state, next);
    for key in &next.focus.references {
        let value = reference(&state.analysis, key)?;
        // A reviewer may focus on endpoints assigned by its current source
        // task. This plans a candidate comparison, not access to other sources.
        if !allowed_focus.contains(key) {
            return Err(format!(
                "focus reference lies outside permitted source scope: {key}"
            ));
        }
        if serde_json::to_vec(&value).map_err(|e| e.to_string())?.len() > max_bytes {
            return Err(
                "focused candidate exceeds detail budget; split the candidate first".into(),
            );
        }
    }
    if next.status == WorkStatus::Active
        && next.focus.action == FocusAction::Link
        && next.focus.references.len() < 2
    {
        return Err("link focus requires the exact endpoint references to compare".into());
    }
    if next.status == WorkStatus::Active
        && next.focus.action != FocusAction::Locate
        && next.focus.action != FocusAction::Handoff
        && next.focus.references.is_empty()
        && next.focus.source_spans.is_empty()
    {
        return Err("focus needs valid planned source spans or saved candidate references".into());
    }
    if let Some(prior) = state.work() {
        let removed: Vec<_> = prior
            .source_scope
            .iter()
            .filter(|id| !next.source_scope.contains(id))
            .cloned()
            .collect();
        if prior.status == WorkStatus::Active
            && !removed.is_empty()
            && (next.status != WorkStatus::Active
                || removed.iter().any(|id| !next.deferred_sources.contains(id)))
        {
            return Err("finish the active scope before replacing it, or split it with status=active and retain every removed source in deferred_sources".into());
        }
        for id in &prior.deferred_sources {
            if !next.deferred_sources.contains(id) && !next.source_scope.contains(id) {
                return Err(format!(
                    "resume the deferred source before removing it: {id}"
                ));
            }
        }
    }
    if next.status != WorkStatus::Complete {
        return Ok(());
    }
    if let Some(gap) = completion_gaps(input, state, next)?.first() {
        return Err(format!(
            "{}; use check_gaps with scope=work for exact fields and references",
            gap["message"].as_str().unwrap_or("work gap")
        ));
    }
    Ok(())
}

/// The same delivered-evidence checks drive diagnostics and handoff acceptance.
/// These rows identify work; querying them does not establish reading coverage.
fn completion_gaps(
    input: &FrozenInput,
    state: &Checkpoint,
    work: &WorkState,
) -> Result<Vec<Value>, String> {
    // Completion depends on confirmed evidence, not whether a redundant read
    // happened in this batch. Unseen sources and candidate versions still have
    // their own concrete gaps below.
    scoped_gaps(input, state, work, state.coverage())
}

fn scoped_gaps(
    input: &FrozenInput,
    state: &Checkpoint,
    work: &WorkState,
    coverage: &Coverage,
) -> Result<Vec<Value>, String> {
    let mut gaps = Vec::new();
    for mut gap in tools::reading_gaps(input, coverage) {
        let source_id = gap["source_id"].as_str().or_else(|| {
            input
                .structured_forms
                .iter()
                .find(|f| f["form_definition_revision_id"] == gap["form_id"])
                .and_then(|f| f["source_unit_revision_id"].as_str())
        });
        if source_id.is_some_and(|id| work.source_scope.iter().any(|s| s == id)) {
            gap["source_id"] = json!(source_id);
            gap["field"] = json!("source_scope");
            gap["message"] = json!("work scope still has undelivered source ranges or grid cells");
            gaps.push(gap);
        }
    }
    for id in &work.source_scope {
        if !state.analysis.dispositions.contains_key(id) {
            gaps.push(
                json!({"kind":"missing_disposition","field":"source_scope","source_id":id,
                "message":"work scope needs source dispositions"}),
            );
        }
    }
    if state.role == Role::Reviewer {
        for (id, view) in &state.analysis.coverage.views {
            if work.source_scope.contains(&view.source_id) && coverage.views.get(id) != Some(view) {
                gaps.push(json!({"kind":"unreviewed_view","field":"source_scope",
                    "source_id":view.source_id,"view_id":id,
                    "message":"independently inspect the scope's original views before completing it"}));
            }
        }
    }
    // Retention is global; completion is local. Unrelated pending outcomes stay
    // in WorkState and the final review gate, but must not pull deferred work
    // into every local comparison. Related cross-scope relations remain here.
    for key in work_references(state, work) {
        if state.role == Role::Reviewer
            && coverage.candidate.get(&key) != Some(&digest(&reference(&state.analysis, &key)?)?)
        {
            gaps.push(
                json!({"kind":"unreviewed_scope_outcome","field":"source_scope","reference":key,
                "message":format!("independently inspect current scope outcome: {key}")}),
            );
        }
    }
    Ok(gaps)
}

pub(super) fn work_gaps(
    input: &FrozenInput,
    state: &Checkpoint,
    args: &Value,
    max_bytes: usize,
) -> Result<Value, String> {
    if args.as_object().is_none_or(|args| {
        args.keys()
            .any(|key| !matches!(key.as_str(), "scope" | "offset" | "limit"))
    }) {
        return Err("work gap query accepts only scope, offset and limit".into());
    }
    let number = |name: &str| {
        args[name]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| format!("{name} must be a nonnegative integer"))
    };
    let work = state
        .work()
        .ok_or("declare a source scope with set_work_note before querying work gaps")?;
    let rows = fragment_gaps(
        input,
        state,
        state.coverage(),
        completion_gaps(input, state, work)?,
        max_bytes,
    )?;
    tools::bounded_page(&rows, number("offset")?, number("limit")?, max_bytes)
}

fn fragment_gaps(
    input: &FrozenInput,
    state: &Checkpoint,
    coverage: &Coverage,
    mut rows: Vec<Value>,
    max_bytes: usize,
) -> Result<Vec<Value>, String> {
    if state.role == Role::Reviewer
        && let Some(gaps) = source_review::reading_gaps(input, state, coverage, max_bytes)?
    {
        rows.retain(|gap| gap["kind"] != "unread_source" && gap["kind"] != "unread_grid");
        rows.extend(gaps.into_iter().map(|mut gap| {
            gap["field"] = json!("source_review.current.task");
            gap["message"] =
                json!("independently read the assigned fragment and collection metadata");
            gap
        }));
    }
    Ok(rows)
}

pub(super) fn check_delete(state: &Checkpoint, name: &str, args: &Value) -> Result<(), String> {
    let kind = match name {
        "delete_record" => "record",
        "delete_relation" => "relation",
        _ => return Ok(()),
    };
    let key = format!(
        "{kind}:{}",
        args["id"].as_str().ok_or("delete needs an ID")?
    );
    // A completed independent review can identify the pending item itself as
    // wrong. Let the primary role retire it; the saved finding still requires
    // explicit independent rereview and is not cleared by record deletion.
    let reviewed_issue = state.role == Role::Main
        && state.review_rounds > 0
        && state.review.as_ref().is_some_and(|review| {
            review
                .findings
                .iter()
                .any(|finding| finding.affected.iter().any(|field| args["id"] == field.id))
        });
    if unresolved(&state.analysis, &key)
        && !reviewed_issue
        && [&state.main_work, &state.reviewer_work]
            .iter()
            .filter_map(|w| w.as_ref())
            .any(|w| w.pending_refs.contains(&key))
    {
        return Err("resolve the pending outcome before deleting it; deletion cannot erase a handoff obligation".into());
    }
    Ok(())
}

pub(super) fn check_read_scope(
    input: &FrozenInput,
    state: &Checkpoint,
    name: &str,
    args: &Value,
) -> Result<(), String> {
    let source_id = match name {
        "read_source" | "read_source_view" => args["source_id"].as_str(),
        "read_form" => input
            .structured_forms
            .iter()
            .find(|f| f["form_definition_revision_id"] == args["form_id"])
            .and_then(|f| f["source_unit_revision_id"].as_str()),
        _ => return Ok(()),
    }
    .ok_or("reading requires a known source or form identity")?;
    let work = state
        .work()
        .ok_or("declare an active source scope with set_work_note before reading")?;
    if work.status != WorkStatus::Active || !work.source_scope.iter().any(|id| id == source_id) {
        return Err("read lies outside active work; expand the scope for a cross-reference or complete its handoff first".into());
    }
    Ok(())
}

/// A bounded, derived work checklist accompanies every request. It is not an
/// evidence receipt. Pending reads are projected here only because that exact
/// last protocol group is being delivered in this request; the durable ledger
/// is still promoted only after a complete model response.
pub(super) fn request_work_state(
    input: &FrozenInput,
    state: &Checkpoint,
    max_bytes: usize,
) -> Result<Value, String> {
    let Some(work) = state.work() else {
        return Ok(Value::Null);
    };
    if state.role == Role::Main && work.status == WorkStatus::Complete {
        if work.deferred_sources.is_empty()
            && state.main_progress.blockers.is_empty()
            && state.reviewer_progress.blockers.is_empty()
            && tools::gaps(input, &state.analysis).is_empty()
        {
            let changed = state
                .review
                .as_ref()
                .map(|review| digest(&state.analysis).map(|sha| sha != review.analysis_sha256))
                .transpose()?
                .unwrap_or(true);
            return Ok(if changed {
                json!({"next_action":"request_review",
                    "instruction":"The declared scope is complete and the full collection has no structural gaps. Call request_review to independently check the current analysis. Previous review_findings remain the previous report until the reviewer rechecks; the main Agent does not clear them. This navigation is not semantic approval."})
            } else {
                json!({"next_action":"verify_review_findings",
                    "instruction":"The analysis is unchanged since the previous review. Recheck the findings against original sources before deciding a correction or requesting another review; unchanged data is not a completed repair."})
            });
        }
        return Ok(Value::Null);
    }
    if work.status != WorkStatus::Active {
        return Ok(Value::Null);
    }
    let coverage = state
        .pending_coverage
        .as_ref()
        .unwrap_or_else(|| state.coverage());
    let rows = fragment_gaps(
        input,
        state,
        coverage,
        scoped_gaps(input, state, work, coverage)?,
        max_bytes,
    )?;
    let mut counts = BTreeMap::<String, usize>::new();
    for gap in &rows {
        *counts
            .entry(
                gap["kind"]
                    .as_str()
                    .ok_or("work gap kind missing")?
                    .to_owned(),
            )
            .or_default() += 1;
    }
    let mut packet = json!({
        "gap_counts":counts,
        "next_action":if !state.main_progress.blockers.is_empty() || !state.reviewer_progress.blockers.is_empty() {
            "resolve_execution_blockers"
        } else if !rows.is_empty() {
            "resolve_work_gaps"
        } else if state.role == Role::Reviewer {
            "compare_then_judge_source"
        } else {
            "complete_scope"
        },
        "instruction":if state.role == Role::Reviewer {
            "Work gaps track evidence delivery, not semantic approval. Compare the focused original evidence and candidate now. Save a grounded mismatch with put_review_finding; if the comparison is correct, record it with complete_review_check instead of inventing a finding. Judge source omissions and boundaries with put_source_review for the assigned task. The host advances completed tasks and aggregates when all required judgments exist. Re-read only a specific missing comparison field, not the entire inventory. Execution blockers still prevent submission."
        } else {
            "Resolve the listed gaps, save the grounded results, then use set_work_note status=complete for the SAME source_scope. An empty gap list does not call for another inventory read."
        },
        "receipt_basis":"Committed evidence plus reads delivered in this request; confirmation requires your complete tool response. This checklist does not itself establish evidence.",
        "blockers":null
    });
    if state.role == Role::Reviewer {
        let refs = work_references(state, work);
        let mut remaining = Vec::new();
        for key in &refs {
            if !has_review_outcome(state, key)? {
                remaining.push(key);
            }
        }
        let mut focus_remaining = 0;
        let mut next_focused = None;
        for key in &work.focus.references {
            if !has_review_outcome(state, key)? {
                focus_remaining += 1;
                next_focused.get_or_insert(key);
            }
        }
        packet["comparison_progress"] = json!({"total":refs.len(),
            "with_recorded_outcome":refs.len()-remaining.len(),"remaining":remaining.len(),
            "focus_total":work.focus.references.len(),"focus_remaining":focus_remaining,
            "next_reference":next_focused.or_else(|| remaining.first().copied()),
            "instruction":"Save a finding or complete_review_check after comparing a candidate. This is local progress, not automatic approval; source-to-result omissions still need independent checking."});
        if review_focus_complete(state)?
            && state.main_progress.blockers.is_empty()
            && state.reviewer_progress.blockers.is_empty()
        {
            if !remaining.is_empty() {
                packet["next_action"] = json!("select_next_review_focus");
                packet["instruction"] = json!(
                    "The current review focus already has recorded outcomes. Use set_work_note to select unfinished exact references, starting with comparison_progress.next_reference; then retrieve only the missing evidence or candidate fields for that comparison. Do not repeat completed checks or re-read the completed focus. Preserve deferred sources and check source-to-result omissions before completing the scope."
                );
            } else if rows.is_empty() {
                packet["next_action"] = json!("complete_source_review");
                packet["instruction"] = json!(
                    "All scope candidates have recorded comparison outcomes. Independently check whether original source obligations were omitted from the candidate set; save any grounded findings, then call put_source_review with the current task version and explicit boundaries. Recorded candidate checks alone do not prove source completeness or approve the analysis."
                );
            }
        }
    }
    let overhead = serde_json::to_vec(&packet)
        .map_err(|e| e.to_string())?
        .len()
        - serde_json::to_vec(&Value::Null)
            .map_err(|e| e.to_string())?
            .len();
    let page_budget = max_bytes
        .checked_sub(overhead)
        .ok_or("work checklist exceeds input budget")?;
    packet["blockers"] = tools::bounded_page(&rows, 0, rows.len().max(1), page_budget)?;
    Ok(packet)
}

/// Inventory of source payloads actually retained in a request transcript.
/// It is used only for context selection, never to acknowledge reading.
pub(super) fn visible_work_evidence(
    state: &Checkpoint,
    messages: &[Value],
) -> BTreeMap<String, Vec<(usize, usize)>> {
    let scope = state
        .work()
        .filter(|work| work.status == WorkStatus::Active)
        .map(|work| work.source_scope.as_slice())
        .unwrap_or_default();
    let mut ranges = BTreeMap::<String, Vec<(usize, usize)>>::new();
    for message in messages {
        if let Some(ids) = message["source_view_refs"].as_array() {
            for id in ids.iter().filter_map(Value::as_str) {
                if state
                    .source_views
                    .get(id)
                    .is_some_and(|view| scope.contains(&view.identity.source_id))
                {
                    ranges.insert(format!("view:{id}"), vec![(0, 1)]);
                }
            }
        }
        let Some(output) = message["content"]
            .as_str()
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
        else {
            continue;
        };
        if message["role"] == "user" {
            for item in output["retained_candidate_details"]["items"]
                .as_array()
                .into_iter()
                .flatten()
            {
                let Some(key) = item["reference"].as_str() else {
                    continue;
                };
                let Ok(current) = reference(&state.analysis, key) else {
                    continue;
                };
                let Ok(version) = digest(&current) else {
                    continue;
                };
                if state
                    .coverage()
                    .candidate
                    .get(key)
                    .is_some_and(|saved| saved == &version)
                    && current == item["value"]
                    && item
                        .get("sha256")
                        .is_none_or(|sha| sha.as_str() == Some(version.as_str()))
                {
                    ranges.insert(format!("candidate:{key}:{version}"), vec![(0, 1)]);
                }
            }
        }
        if message["role"] != "tool" {
            continue;
        }
        let result = &output["result"];
        if output["ok"] == true && result["view"] == "detail" {
            for item in result["items"].as_array().into_iter().flatten() {
                let key = if item["data"].is_object() {
                    item["id"].as_str().map(|id| format!("record:{id}"))
                } else if item["from"].is_string() && item["to"].is_string() {
                    item["id"].as_str().map(|id| format!("relation:{id}"))
                } else if item["disposition"].is_object() {
                    item["source_id"]
                        .as_str()
                        .map(|id| format!("disposition:{id}"))
                } else {
                    None
                };
                if let Some(key) = key
                    && scope_references(&state.analysis, scope).contains(&key)
                    && let Ok(current) = reference(&state.analysis, &key)
                    && current == *item
                    && let Ok(version) = digest(item)
                {
                    ranges.insert(format!("candidate:{key}:{version}"), vec![(0, 1)]);
                }
            }
        }
        let Some(source_id) = result["source_id"].as_str() else {
            continue;
        };
        if output["ok"] != true || !scope.iter().any(|id| id == source_id) {
            continue;
        }
        let (key, start, end) = if let Some(form_id) = result["form_id"].as_str() {
            (
                format!("form:{form_id}"),
                result["offset"].as_u64(),
                result["next"].as_u64(),
            )
        } else if result["text"].is_string() {
            (
                format!("text:{source_id}"),
                result["start"].as_u64(),
                result["end"].as_u64(),
            )
        } else {
            continue;
        };
        if let (Some(start), Some(end)) = (
            start.and_then(|n| usize::try_from(n).ok()),
            end.and_then(|n| usize::try_from(n).ok()),
        ) {
            tools::cover(ranges.entry(key).or_default(), start, end);
        }
    }
    ranges
}

pub(super) fn focused_work_evidence(
    state: &Checkpoint,
    messages: &[Value],
) -> BTreeMap<String, Vec<(usize, usize)>> {
    let mut evidence = visible_work_evidence(state, messages);
    let refs = state.work().map(|w| &w.focus.references);
    let views = focused_view_ids(state);
    evidence.retain(|key, _| {
        if let Some(id) = key.strip_prefix("view:") {
            return views.contains(id);
        }
        !key.starts_with("candidate:")
            || refs.is_some_and(|refs| {
                refs.iter()
                    .any(|r| key.starts_with(&format!("candidate:{r}:")))
            })
    });
    evidence
}

pub(super) fn focused_view_ids(state: &Checkpoint) -> BTreeSet<String> {
    state
        .work()
        .filter(|work| work.status == WorkStatus::Active)
        .into_iter()
        .flat_map(|work| &work.focus.source_spans)
        .filter_map(|span| span.view_id.clone())
        .collect()
}

/// Recall current focused/assigned versions already delivered to this role. This
/// working message is separate from evictable tool history, but participates
/// in the same total request budget. It creates no receipt or semantic result.
pub(super) fn retained_candidate_message(
    state: &Checkpoint,
    budget: usize,
) -> Result<Value, String> {
    let visible = visible_work_evidence(state, &state.transcript);
    candidate_recall_message(state, budget, &visible)
}

pub(super) fn trim_optional_candidate_recall(
    state: &Checkpoint,
    message: &mut Value,
    excluded: &mut BTreeSet<String>,
    excess: usize,
) -> Result<bool, String> {
    let Some(raw) = message["content"].as_str() else {
        return Ok(false);
    };
    let mut content: Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    let items = content["retained_candidate_details"]["items"]
        .as_array_mut()
        .ok_or("candidate recall items missing")?;
    let mut removed_bytes = 0;
    for item in items.iter().rev() {
        if removed_bytes >= excess {
            break;
        }
        let key = item["reference"]
            .as_str()
            .ok_or("candidate recall reference missing")?;
        if !excluded.contains(key)
            && !state
                .work()
                .is_some_and(|work| work.focus.references.iter().any(|r| r == key))
        {
            excluded.insert(key.to_owned());
            removed_bytes += serde_json::to_vec(item).map_err(|e| e.to_string())?.len();
        }
    }
    items.retain(|item| {
        !item["reference"]
            .as_str()
            .is_some_and(|key| excluded.contains(key))
    });
    let count = items.len();
    if count == 0 {
        *message = Value::Null;
    } else {
        content["retained_candidate_details"]["next"] = json!(count);
        message["content"] = json!(content.to_string());
    }
    Ok(removed_bytes > 0)
}

fn candidate_recall_message(
    state: &Checkpoint,
    budget: usize,
    visible: &BTreeMap<String, Vec<(usize, usize)>>,
) -> Result<Value, String> {
    let Some(work) = state
        .work()
        .filter(|work| work.status == WorkStatus::Active)
    else {
        return Ok(Value::Null);
    };
    let mut keys = work.focus.references.clone();
    if state.role == Role::Reviewer
        && state
            .source_review
            .as_ref()
            .is_some_and(|r| r.active_task.is_some())
    {
        for key in work_references(state, work) {
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
    }
    let mut candidates = Vec::new();
    for key in &keys {
        let Ok(value) = reference(&state.analysis, key) else {
            continue;
        };
        let sha = digest(&value)?;
        if state.coverage().candidate.get(key) == Some(&sha)
            && !visible.contains_key(&format!("candidate:{key}:{sha}"))
        {
            candidates.push(json!({"reference":key,"value":value}));
        }
    }
    if candidates.is_empty() {
        return Ok(Value::Null);
    }
    let mut content = json!({"retained_candidate_details":null,
        "note":"Current focused or assigned source-task candidate versions previously delivered to this role, recalled after tool-history eviction. This is not a new read or a comparison result. Unlisted candidates remain retrievable by exact ID."});
    let overhead = serde_json::to_vec(&content)
        .map_err(|e| e.to_string())?
        .len();
    let Ok(page) = tools::bounded_page(&candidates, 0, usize::MAX, budget.saturating_sub(overhead))
    else {
        // Recall is optional if a wrapped candidate cannot fit its allowance.
        // Exact reading and ordinary focus backpressure remain available.
        return Ok(Value::Null);
    };
    if page["items"].as_array().is_none_or(Vec::is_empty) {
        return Ok(Value::Null);
    }
    content["retained_candidate_details"] = page;
    Ok(json!({"role":"user","content":content.to_string()}))
}

/// Supply the same deterministic locations for already-delivered source reads
/// when resuming an older compatible checkpoint. Never rewrite a reserved body
/// or the latest pending group, or count annotations as newly read evidence.
pub(super) fn annotate_delivered_source_lines(
    state: &mut Checkpoint,
    max_bytes: usize,
) -> Result<(), String> {
    let Some(latest) = state
        .transcript
        .iter()
        .rposition(|m| m["role"] == "assistant")
    else {
        return Ok(());
    };
    let reads: BTreeSet<_> = state.transcript[..latest]
        .iter()
        .flat_map(|m| m["tool_calls"].as_array().into_iter().flatten())
        .filter(|call| call["function"]["name"] == "read_source")
        .filter_map(|call| call["id"].as_str().map(str::to_owned))
        .collect();
    for message in &mut state.transcript[..latest] {
        if !message["tool_call_id"]
            .as_str()
            .is_some_and(|id| reads.contains(id))
        {
            continue;
        }
        let Some(mut output) = message["content"]
            .as_str()
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
        else {
            continue;
        };
        if output["ok"] != true || output["result"].get("line_spans").is_some() {
            continue;
        }
        let result = &mut output["result"];
        let (Some(text), Some(start)) = (
            result["text"].as_str(),
            result["start"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok()),
        ) else {
            continue;
        };
        result["line_spans"] = json!(tools::source_line_spans(text, start));
        if serde_json::to_vec(result).map_err(|e| e.to_string())?.len() <= max_bytes {
            message["content"] = json!(output.to_string());
        }
    }
    Ok(())
}

/// Replace one old navigation payload before evicting its mixed protocol group.
/// Navigation is not evidence. The latest group is still awaiting delivery and
/// must remain verbatim, as must all source and complete candidate results.
pub(super) fn compact_delivered_navigation(transcript: &mut [Value]) -> bool {
    let Some(latest) = transcript.iter().rposition(|m| m["role"] == "assistant") else {
        return false;
    };
    let names: BTreeMap<_, _> = transcript[..latest]
        .iter()
        .flat_map(|m| m["tool_calls"].as_array().into_iter().flatten())
        .filter_map(|call| {
            Some((
                call["id"].as_str()?.to_owned(),
                call["function"]["name"].as_str()?.to_owned(),
            ))
        })
        .collect();
    for message in &mut transcript[..latest] {
        let Some(name) = message["tool_call_id"]
            .as_str()
            .and_then(|id| names.get(id))
        else {
            continue;
        };
        if !matches!(
            name.as_str(),
            "source_index" | "search_sources" | "inspect_analysis"
        ) {
            continue;
        }
        let Some(content) = message["content"].as_str() else {
            continue;
        };
        let Ok(output) = serde_json::from_str::<Value>(content) else {
            continue;
        };
        if output["ok"] != true
            || output["result"]["history_omitted"] == true
            || (name == "inspect_analysis" && output["result"]["view"] != "index")
        {
            continue;
        }
        let compact = json!({"ok":true,"result":{"history_omitted":true,
            "note":"Previously delivered navigation omitted from history to retain source evidence; query this tool again only if needed."}}).to_string();
        if compact.len() < content.len() {
            message["content"] = json!(compact);
            return true;
        }
    }
    false
}

/// Evict a complete delivered protocol group, preserving unique active source
/// evidence when another group can be removed instead.
pub(super) fn evict_delivered_group(
    state: &mut Checkpoint,
    history_budget: usize,
    allow_unique: bool,
) -> bool {
    let starts: Vec<_> = state
        .transcript
        .iter()
        .enumerate()
        .filter(|(_, message)| message["role"] == "assistant")
        .map(|(index, _)| index)
        .collect();
    if starts.len() < 2 {
        return false;
    }
    let groups: Vec<_> = starts
        .iter()
        .enumerate()
        .map(|(i, &start)| {
            (
                if i == 0 { 0 } else { start },
                starts.get(i + 1).copied().unwrap_or(state.transcript.len()),
            )
        })
        .collect();
    let evidence: Vec<_> = groups
        .iter()
        .map(|&(start, end)| visible_work_evidence(state, &state.transcript[start..end]))
        .collect();
    let redundant = (0..groups.len() - 1).find(|&candidate| {
        let mut other = BTreeMap::<String, Vec<(usize, usize)>>::new();
        for (_, items) in evidence
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != candidate)
        {
            for (key, ranges) in items {
                for &(start, end) in ranges {
                    tools::cover(other.entry(key.clone()).or_default(), start, end);
                }
            }
        }
        evidence[candidate].iter().all(|(key, ranges)| {
            ranges.iter().all(|&(start, end)| {
                other
                    .get(key)
                    .is_some_and(|other| other.iter().any(|&(a, b)| a <= start && b >= end))
            })
        })
    });
    // Release oversized delivered images, or smaller ones before the fallback
    // sacrifices unique evidence. A mixed batch can still contain the exact
    // candidates and grids needed for the active comparison. Request assembly
    // separately retains the actual pixels of currently required views.
    // Use a lower bound from the actual cached payloads, not token estimates.
    let releasable_images = (0..groups.len() - 1).find(|&candidate| {
        let (start, end) = groups[candidate];
        let image_bytes = state.transcript[start..end]
            .iter()
            .filter_map(|message| message["source_view_refs"].as_array())
            .flatten()
            .filter_map(Value::as_str)
            .filter_map(|id| state.source_views.get(id))
            .fold(0usize, |bytes, view| {
                bytes.saturating_add(view.jpeg_base64.len())
            });
        image_bytes > history_budget || (allow_unique && image_bytes > 0)
    });
    if redundant.is_none()
        && let Some(candidate) = releasable_images
    {
        let (start, end) = groups[candidate];
        for message in &mut state.transcript[start..end] {
            if let Some(ids) = message.get("source_view_refs") {
                *message = json!({"role":"user","content":json!({
                    "history_omitted_source_views":ids,
                    "note":"Previously delivered image pixels omitted from history. The image receipt is not a new reading or semantic judgment. Use read_source_view for a specific visual comparison when needed."
                }).to_string()});
            }
        }
        return true;
    }
    if redundant.is_none() && !allow_unique {
        return false;
    }
    // Preserve the latest pending group and enforce the frozen ceiling even
    // when all remaining groups contain unique evidence.
    let unfocused = (0..groups.len() - 1).find(|&candidate| {
        let (start, end) = groups[candidate];
        focused_work_evidence(state, &state.transcript[start..end]).is_empty()
    });
    if redundant.or(unfocused).is_none()
        && compact_delivered_candidate_details(state, *starts.last().unwrap(), &[])
    {
        return true;
    }
    let (start, end) = groups[redundant.or(unfocused).unwrap_or(0)];
    state.transcript.drain(start..end);
    true
}

/// A delivered batch can mix current source evidence and focused candidates
/// with details no longer in focus. Release only the latter before sacrificing
/// the complete group. Latest outputs and all reading receipts stay untouched.
pub(super) fn compact_recallable_candidate_details(state: &mut Checkpoint, budget: usize) -> bool {
    let Some(latest) = state
        .transcript
        .iter()
        .rposition(|m| m["role"] == "assistant")
    else {
        return false;
    };
    // Reserve room for these versions even if every candidate history payload
    // disappears. Never release a focused value that cannot fit this recall.
    let Ok(message) = candidate_recall_message(state, budget, &BTreeMap::new()) else {
        return false;
    };
    let Some(content) = message["content"]
        .as_str()
        .and_then(|content| serde_json::from_str::<Value>(content).ok())
    else {
        return false;
    };
    let recallable: Vec<_> = content["retained_candidate_details"]["items"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| item["value"].clone())
        .collect();
    compact_delivered_candidate_details(state, latest, &recallable)
}

fn compact_delivered_candidate_details(
    state: &mut Checkpoint,
    latest: usize,
    recallable: &[Value],
) -> bool {
    let protected: Vec<_> = state
        .work()
        .into_iter()
        .flat_map(|work| &work.focus.references)
        .filter_map(|key| reference(&state.analysis, key).ok())
        .filter(|value| !recallable.contains(value))
        .collect();
    let inspections: BTreeSet<_> = state.transcript[..latest]
        .iter()
        .flat_map(|m| m["tool_calls"].as_array().into_iter().flatten())
        .filter(|call| call["function"]["name"] == "inspect_analysis")
        .filter_map(|call| call["id"].as_str().map(str::to_owned))
        .collect();
    for message in &mut state.transcript[..latest] {
        if message["role"] != "tool"
            || !message["tool_call_id"]
                .as_str()
                .is_some_and(|id| inspections.contains(id))
        {
            continue;
        }
        let Some(content) = message["content"].as_str() else {
            continue;
        };
        let Ok(mut output) = serde_json::from_str::<Value>(content) else {
            continue;
        };
        if output["ok"] != true || output["result"]["view"] != "detail" {
            continue;
        }
        let Some(items) = output["result"]["items"].as_array_mut() else {
            continue;
        };
        let before = items.len();
        items.retain(|item| protected.contains(item));
        let removed = before - items.len();
        if removed == 0 {
            continue;
        }
        output["result"]["history_omitted_items"] = json!(
            output["result"]["history_omitted_items"]
                .as_u64()
                .unwrap_or(0)
                + removed as u64
        );
        output["result"]["history_note"] = json!(
            "Previously delivered candidate details omitted when outside focus or retained in the bounded working message; original pagination counters are historical. Retrieve exact IDs when needed. This establishes no new reading or comparison."
        );
        let compact = output.to_string();
        if compact.len() < content.len() {
            message["content"] = json!(compact);
            return true;
        }
    }
    false
}

fn scope_dependencies(state: &Checkpoint, scope: &[String]) -> Result<String, String> {
    let values: Vec<_> = scope_references(&state.analysis, scope)
        .iter()
        .map(|key| reference(&state.analysis, key))
        .collect::<Result<_, _>>()?;
    digest(&values)
}

pub(super) fn check_blocked_scope(state: &Checkpoint, scope: &[String]) -> Result<(), String> {
    for blocker in &state.execution().blockers {
        if blocker.scope.iter().any(|id| scope.contains(id))
            && scope_dependencies(state, &blocker.scope)? == blocker.dependencies_sha256
        {
            return Err(
                "blocked source dependencies are unchanged; continue an independent scope instead"
                    .into(),
            );
        }
    }
    Ok(())
}

/// Only used before preparing a fresh boundary. Saved responses still execute,
/// and changed dependencies or an independent source retain the handoff path.
pub(super) fn check_independent_work(
    input: &FrozenInput,
    state: &Checkpoint,
) -> Result<(), AgentError> {
    if state.execution().watch.recovery != Recovery::Blocked {
        return Ok(());
    }
    let mut blocked = BTreeSet::new();
    for blocker in &state.execution().blockers {
        if scope_dependencies(state, &blocker.scope).map_err(invalid)?
            == blocker.dependencies_sha256
        {
            blocked.extend(&blocker.scope);
        }
    }
    if input
        .source_units
        .iter()
        .all(|source| blocked.contains(&source.source_unit_revision_id))
    {
        return Err(error(
            "AGENT_TURN_BUDGET_EXCEEDED",
            "no independent source scope remains; execution blockers and checkpoint retained",
        ));
    }
    Ok(())
}

pub(super) fn observe_progress(
    state: &mut Checkpoint,
    role: &Role,
    local_completion: Option<String>,
    limits: &Limits,
) -> Result<(), String> {
    let work = if *role == Role::Main {
        &state.main_work
    } else {
        &state.reviewer_work
    };
    let mut scope = work
        .as_ref()
        .map(|w| w.source_scope.clone())
        .unwrap_or_default();
    scope.sort();
    let dependencies = scope_dependencies(state, &scope)?;
    let coverage = if *role == Role::Main {
        &state.analysis.coverage
    } else {
        &state.reviewer_coverage
    };
    let mut versions = Vec::new();
    // Receipts are already confirmed at this complete response boundary.
    // Pending reads are excluded; evicting and re-reading cannot create novelty.
    for (kind, value) in serde_json::to_value(coverage)
        .map_err(|e| e.to_string())?
        .as_object()
        .ok_or("coverage object missing")?
    {
        if kind == "view_failures" {
            continue;
        }
        if let Some(entries) = value.as_object() {
            for (id, value) in entries {
                versions.push(digest(&json!([kind, id, value]))?);
            }
        } else {
            versions.push(digest(&json!([kind, value]))?);
        }
    }
    for key in scope_references(&state.analysis, &scope) {
        versions.push(digest(&reference(&state.analysis, &key)?)?);
    }
    for finding in state.review_draft.values() {
        versions.push(digest(finding)?);
    }
    let completed = work
        .as_ref()
        .is_some_and(|w| w.status == WorkStatus::Complete)
        || state.role != *role
        || state.done;
    let completion = completed
        .then(|| digest(&json!([scope, dependencies, state.role, state.done])))
        .transpose()?;
    let progress = if *role == Role::Main {
        &mut state.main_progress
    } else {
        &mut state.reviewer_progress
    };
    progress.observe(
        versions,
        completion.or(local_completion),
        &limits.progress(),
    );
    if completed {
        progress
            .blockers
            .retain(|b| !b.scope.iter().all(|id| scope.contains(id)));
    }
    progress.block(scope, dependencies);
    if progress.watch.recovery == Recovery::Blocked {
        let work = if *role == Role::Main {
            &mut state.main_work
        } else {
            &mut state.reviewer_work
        };
        if let Some(work) = work {
            work.status = WorkStatus::Blocked;
        }
    }
    Ok(())
}

pub(super) fn execution_packet(state: &Checkpoint, max_bytes: usize) -> Result<Value, String> {
    let progress = state.execution();
    let completed_focus = review_focus_complete(state)?
        && state.main_progress.blockers.is_empty()
        && state.reviewer_progress.blockers.is_empty();
    let mut packet = json!({"watch":progress.watch,"blocker_count":progress.blockers.len(),
    "next_action":if completed_focus {
        "The current review focus already has recorded outcomes. Follow work_state.next_action and comparison_progress.next_reference to select unfinished comparisons. Do not repeat the completed focus. If none remain, complete the assigned source-to-result judgment with put_source_review; the host advances and aggregates. This guidance neither grants approval nor resets recovery budgets."
    } else if state.role == Role::Reviewer && progress.watch.needs_replan_context() {
        "Replan the stalled comparison now. Select a single unfinished candidate or one exact source uncertainty and narrow focus with set_work_note; retain the assigned source task and permitted cross-reference scope. Compare that candidate with its original evidence and save put_review_finding or complete_review_check before loading another inventory page. If a particular field is missing, retrieve only that evidence. Do not wait to read every candidate before saving the first comparison. After the local comparisons, judge source omissions and boundaries with put_source_review; the host advances tasks and aggregates complete judgments. Execution blockers prevent submission. This neither approves the analysis nor renews recovery allowances."
    } else { match (&state.role, &progress.watch.recovery) {
        (Role::Reviewer, Recovery::Running) => "Compare the focused source and candidate now. Save actionable mismatches with put_review_finding; record a correct comparison with complete_review_check, without inventing a finding. Judge the assigned source fragment with put_source_review, including omissions and continuation boundaries. The host advances tasks and aggregates complete judgments. Repeated reads/notes are not progress; inspect only a specific missing field. Execution blockers prevent submission.",
        (_, Recovery::Running) => "Save the current grounded local result before expanding comparisons.",
        (_, Recovery::Replan) => "Repeated reads/notes are not progress. Narrow focus to an exact clause or endpoint pair and write its result; inspect only a specific missing field.",
        (_, Recovery::Blocked) => "Select an independent source scope. This execution failure prevents final acceptance and cannot be converted to source uncertainty."
    }}});
    packet["blockers"] = execution_gaps(
        state,
        &json!({"scope":"execution","offset":0,"limit":progress.blockers.len().max(1)}),
        max_bytes.saturating_sub(
            serde_json::to_vec(&packet)
                .map_err(|e| e.to_string())?
                .len()
                + 16,
        ),
    )?;
    Ok(packet)
}

/// A clean comparison is a local outcome too. Its receipt is keyed by the
/// candidate version, not wording, so repeated assertions cannot renew budgets.
fn review_check_key(reference: &str, version: &str) -> Result<String, String> {
    digest(&json!(["review_check", reference, version]))
}

fn has_review_finding(state: &Checkpoint, reference: &str) -> bool {
    reference.split_once(':').is_some_and(|(kind, id)| {
        state.review_draft.values().any(|f| {
            let affects = if kind == "disposition" {
                f.sources.iter().any(|s| s.source_id == id)
            } else {
                f.affected.iter().any(|a| a.id == id)
            };
            affects
                && self::reference(&state.analysis, reference)
                    .ok()
                    .and_then(|v| digest(&v).ok())
                    .as_ref()
                    == state.reviewer_coverage.candidate.get(reference)
        })
    })
}

pub(super) fn has_review_outcome(state: &Checkpoint, key: &str) -> Result<bool, String> {
    let version = source_review::candidate_version(state, key)?;
    Ok(state
        .reviewer_progress
        .seen
        .contains(&review_check_key(key, &version)?)
        || has_review_finding(state, key))
}

fn review_focus_complete(state: &Checkpoint) -> Result<bool, String> {
    let Some(work) = state.work().filter(|work| {
        state.role == Role::Reviewer
            && work.status == WorkStatus::Active
            && work.focus.action == FocusAction::Review
            && !work.focus.references.is_empty()
    }) else {
        return Ok(false);
    };
    for key in &work.focus.references {
        if !has_review_outcome(state, key)? {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn complete_review_check(
    input: &FrozenInput,
    state: &mut Checkpoint,
    args: &Value,
    max_bytes: usize,
) -> Result<Value, String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Check {
        reference: String,
        summary: String,
        sources: Vec<Span>,
    }
    let check: Check = serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
    let work = state.work().ok_or("declare a review focus first")?;
    let assigned = state
        .source_review
        .as_ref()
        .is_some_and(|r| r.active_task.is_some())
        && work_references(state, work).contains(&check.reference);
    if work.status != WorkStatus::Active
        || (!assigned
            && (work.focus.action != FocusAction::Review
                || !work.focus.references.contains(&check.reference)))
    {
        return Err(
            "clean review check must name an exact candidate in the active review focus".into(),
        );
    }
    if check.summary.trim().is_empty() || check.sources.is_empty() {
        return Err(
            "clean review check needs a comparison summary and original source citations".into(),
        );
    }
    let version = digest(&reference(&state.analysis, &check.reference)?)?;
    if state.reviewer_coverage.candidate.get(&check.reference) != Some(&version) {
        return Err(
            "independently inspect the current candidate before recording a clean comparison"
                .into(),
        );
    }
    if has_review_finding(state, &check.reference) {
        return Err("candidate has a saved review finding; inspect_review before asserting a clean comparison".into());
    }
    for source in &check.sources {
        if !work.source_scope.contains(&source.source_id) {
            return Err("comparison evidence lies outside the active source scope".into());
        }
        tools::validate_span(input, &state.reviewer_coverage, source)?;
    }
    let cited: Vec<_> = check.sources.iter().map(|s| s.source_id.clone()).collect();
    if !scope_references(&state.analysis, &cited).contains(&check.reference) {
        return Err("comparison citations must concern the referenced candidate".into());
    }
    let receipt = review_check_key(
        &check.reference,
        &source_review::candidate_version(state, &check.reference)?,
    )?;
    let fresh = !state.reviewer_progress.seen.contains(&receipt);
    let output = json!({"reference":check.reference,"candidate_sha256":version,
        "assessment":"no_issue_in_comparison","summary":check.summary,"sources":check.sources,
        "new_completion":fresh,"completion_sha256":receipt});
    if serde_json::to_vec(&output)
        .map_err(|e| e.to_string())?
        .len()
        > max_bytes
    {
        return Err("clean comparison exceeds result budget; shorten the summary".into());
    }
    // Every check in a multi-tool batch is retained. The last novel local
    // completion resets the turn watch once, at the normal commit boundary.
    state.reviewer_progress.seen.insert(receipt);
    Ok(output)
}

/// A validated focused write closes a local action without pretending the whole
/// source scope is complete. Rewriting an identical version cannot reset it.
pub(super) fn focused_completion(
    state: &Checkpoint,
    name: &str,
    output: &Value,
) -> Result<Option<String>, String> {
    if name == "put_source_review" {
        return Ok((output["new_completion"] == true)
            .then(|| output["completion_sha256"].as_str().map(str::to_owned))
            .flatten());
    }
    let Some(work) = state.work().filter(|w| w.status == WorkStatus::Active) else {
        return Ok(None);
    };
    if name == "complete_review_check" {
        return Ok((output["new_completion"] == true)
            .then(|| output["completion_sha256"].as_str().map(str::to_owned))
            .flatten());
    }
    let Some(id) = output["id"].as_str() else {
        return Ok(None);
    };
    match (&work.focus.action, name) {
        (FocusAction::Link, "put_relation") => {
            let relation = state
                .analysis
                .relations
                .get(id)
                .ok_or("saved relation missing")?;
            if [&relation.from, &relation.to]
                .iter()
                .all(|id| work.focus.references.contains(&format!("record:{id}")))
            {
                return digest(relation).map(Some);
            }
        }
        (FocusAction::Extract, "put_record") => {
            let record = state
                .analysis
                .records
                .get(id)
                .ok_or("saved record missing")?;
            if record.sources.iter().any(|source| {
                work.focus.source_spans.iter().any(|focus| {
                    focus.source_id == source.source_id
                        && focus.grid_cell == source.grid_cell
                        && focus.view_id == source.view_id
                        && focus.start <= source.start
                        && focus.end >= source.end
                })
            }) {
                return digest(record).map(Some);
            }
        }
        (FocusAction::Review, "put_review_finding") => {
            if let Some(finding) = state.review_draft.get(id) {
                return digest(finding).map(Some);
            }
        }
        _ => {}
    }
    Ok(None)
}
