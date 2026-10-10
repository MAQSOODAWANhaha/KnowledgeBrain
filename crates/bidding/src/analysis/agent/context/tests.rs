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
    assert!(body.len() <= config.limits.context_wire_ceiling());
    assert!(
        estimate_input_tokens(&request, &config.limits).unwrap()
            + config.provider.output_token_reserve as usize
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
    let grid_args = json!({"form_id":form["form_definition_revision_id"],"offset":0,"limit":config.limits.context_wire_ceiling()});
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
        config.limits.context_wire_ceiling(),
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

#[test]
fn delivered_line_annotations_preserve_receipts_errors_and_pending_results() {
    let mut state: Checkpoint = serde_json::from_value(json!({
        "journal":crate::agent_runtime::TurnJournal::default(),
        "input_sha256":"","config_sha256":"","turn":0,"tool_calls":0,"read_bytes":0,
        "review_rounds":0,"role":"main","analysis":Analysis::default(),"review":null,
        "pending_coverage":null,
        "transcript":[],"main_work":null,"done":false,"source_views":{},
        "outline_run":{"phase":"discover","chunk_plan_sha256":"","chunk_cursor":0,"active_check_packet":null,"repair_signatures":{},"no_progress_rounds":0,"discover_workers":{}}
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
        pending_coverage: Some(Coverage::default()),
        transcript: vec![],
        main_progress: Default::default(),
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
        done: false,
        source_views: BTreeMap::new(),
        draft_stage: Default::default(),
        draft_outline_gaps: None,
        draft_outline_stalls: 0,
        draft_outline_window: 0,
        outline_config_sha256: None,

        outline_run: Default::default(),
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
fn unique_candidate_versions_are_not_navigation_and_focused_pairs_survive() {
    let analysis = Analysis::default();
    let mut state: Checkpoint = serde_json::from_value(json!({
        "journal":crate::agent_runtime::TurnJournal::default(),"input_sha256":"","config_sha256":"",
        "turn":0,"tool_calls":0,"read_bytes":0,"review_rounds":0,"role":"main","analysis":analysis,
        "review":null,"pending_coverage":null,
        "transcript":[],"main_work":{"source_scope":["source"],"objective":"compare endpoints",
            "focus":{"action":"link","source_spans":[],"references":["record:left","record:right"]},"status":"active","note":""},
        "done":false,"source_views":{},
        "outline_run":{"phase":"discover","chunk_plan_sha256":"","chunk_cursor":0,"active_check_packet":null,"repair_signatures":{},"no_progress_rounds":0,"discover_workers":{}}
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
    let limits = crate::analysis::tests::config().limits;
    let mut body = json!({"model":limits.tokenizer.model_id,"messages":[{"role":"user","content":[
        {"type":"text","text":"中文😀"},
        {"type":"image_url","image_url":{"url":"data:image/jpeg;base64,AAAA","detail":"high"}}
    ]}],"tools":[{"type":"function","function":{"name":"read_source"}}]});
    let before = body.clone();
    let estimate = estimate_input_tokens(&body, &limits).unwrap();
    assert_eq!(body, before, "sizing must not replace transmitted pixels");
    body["messages"][0]["content"][1]["image_url"]["url"] = json!("A".repeat(200000));
    assert_eq!(estimate_input_tokens(&body, &limits).unwrap(), estimate);
    body["messages"][0]["content"][0]["text"] = json!("中文😀中文😀");
    assert!(estimate_input_tokens(&body, &limits).unwrap() > estimate);
    body["tools"][0]["function"]["description"] = json!("真实工具定义");
    assert!(estimate_input_tokens(&body, &limits).unwrap() > estimate);
    let image = body["messages"][0]["content"][1].clone();
    let single = estimate_input_tokens(&body, &limits).unwrap();
    body["messages"][0]["content"]
        .as_array_mut()
        .unwrap()
        .push(image);
    assert!(estimate_input_tokens(&body, &limits).unwrap() >= single + limits.image_token_reserve);
}

#[test]
fn over_budget_eviction_drops_only_committed_pack_turns() {
    use crate::outline::discover::{DiscoverWork, PackRequirement, PackSubmit};
    let source = |id: &str, ordinal: usize| crate::analysis::Source {
        source_unit_revision_id: id.into(),
        document_id: "doc".into(),
        text: "X".into(),
        locator: json!({"heading_path": id}),
        ordinal,
    };
    let input = FrozenInput {
        schema_version: 2,
        project_id: "project".into(),
        document_set_id: "set".into(),
        documents: vec![],
        document_relations: vec![],
        source_units: vec![source("a", 0), source("b", 1), source("c", 2)],
        structured_forms: vec![],
        decisions: vec![],
    };
    let mut work = DiscoverWork::plan_with_budget(&input, &|sessions| {
        Ok(sessions.iter().all(|session| {
            session["pack"]["atoms"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|atom| atom["context_only"] == false)
                .count()
                <= 1
        }) && crate::outline::discover::test_sessions_fit(sessions, 8192)?)
    });
    let packs = work.claim(10);
    work.submit(
        &input,
        "pack-0",
        PackSubmit {
            call_id: "ok".into(),
            claim_token: packs[0].claim_token.clone(),
            pack_revision: packs[0].pack_revision,
            requirements: vec![],
            no_requirement_reason: Some("Only explanatory text".into()),
            inspected_atom_ids: packs[0].atoms.iter().map(|a| a.id.clone()).collect(),
        },
    )
    .unwrap();
    assert!(
        work.submit(
            &input,
            "pack-1",
            PackSubmit {
                call_id: "bad".into(),
                claim_token: packs[1].claim_token.clone(),
                pack_revision: packs[1].pack_revision,
                no_requirement_reason: None,
                inspected_atom_ids: vec![],
                requirements: vec![PackRequirement {
                    condition_support: Vec::new(),
                    obligation_strength: "mandatory".into(),
                    extraction_quality: "explicit".into(),
                    description: "越界".into(),
                    source_section_id: packs[1].atoms[0].section_id.clone(),
                    kind: "material".into(),
                    evidence: vec![crate::outline::evidence::EvidenceRef::Text {
                        input_digest: crate::outline::evidence::input_digest(&input).unwrap(),
                        unit_id: "b".into(),
                        start_byte: 0,
                        end_byte: 2
                    }],
                }],
            },
        )
        .is_err()
    );
    let turn = |pack_id: &str| {
        json!({"role":"assistant","tool_calls":[{
            "function": {
                "name": "submit_pack",
                "arguments": json!({"pack_id": pack_id}).to_string()
            }
        }]})
    };
    let mut state = Checkpoint {
        journal: Default::default(),
        input_sha256: String::new(),
        config_sha256: String::new(),
        turn: 4,
        tool_calls: 3,
        read_bytes: 0,
        review_rounds: 0,
        role: Role::Main,
        analysis: Analysis::default(),
        review: None,
        pending_coverage: None,
        transcript: vec![
            turn("pack-0"),
            turn("pack-1"),
            turn("pack-2"),
            json!({"role":"assistant","content":"latest"}),
        ],
        main_progress: Default::default(),
        main_work: None,
        done: false,
        source_views: BTreeMap::new(),
        draft_stage: Default::default(),
        draft_outline_gaps: None,
        draft_outline_stalls: 0,
        draft_outline_window: 0,
        outline_config_sha256: None,

        outline_run: Default::default(),
    };
    // Fully scanned running and failed sources. Groups carry no evidence ranges,
    // so a scanned-cursor eviction would treat them as complete and drop them.
    state
        .analysis
        .outline
        .scanned
        .text
        .insert("b".into(), vec![(0, 1)]);
    state
        .analysis
        .outline
        .scanned
        .text
        .insert("c".into(), vec![(0, 1)]);
    state.outline_run.reading_packs = Some(work);
    let pack_ids = |state: &Checkpoint| -> Vec<String> {
        state
            .transcript
            .iter()
            .filter_map(|message| {
                message["tool_calls"][0]["function"]["arguments"]
                    .as_str()
                    .and_then(|text| serde_json::from_str::<Value>(text).ok())
                    .and_then(|args| args["pack_id"].as_str().map(str::to_string))
            })
            .collect()
    };
    assert!(evict_completed_discovery_history(&mut state, 0));
    assert_eq!(
        pack_ids(&state),
        ["pack-1".to_string(), "pack-2".to_string()]
    );
    assert!(!evict_completed_discovery_history(&mut state, 0));
    assert_eq!(
        pack_ids(&state),
        ["pack-1".to_string(), "pack-2".to_string()]
    );
    assert!(state.transcript.last().unwrap()["content"] == "latest");
    let mut successful = turn("pack-0");
    successful["tool_calls"][0]["id"] = json!("ack-0");
    state.transcript = vec![
        successful.clone(),
        json!({"role":"tool","tool_call_id":"ack-0","content":json!({"ok":false,"error":"repair feedback must survive"}).to_string()}),
    ];
    assert!(!evict_completed_discovery_history(&mut state, 0));
    state.transcript[1]["content"] =
        json!(json!({"ok":true,"result":{"status":"committed"}}).to_string());
    assert!(evict_completed_discovery_history(&mut state, 0));
    assert!(state.transcript.is_empty());
}

fn outline_checkpoint() -> Checkpoint {
    Checkpoint {
        journal: Default::default(),
        input_sha256: String::new(),
        config_sha256: String::new(),
        turn: 0,
        tool_calls: 0,
        read_bytes: 0,
        review_rounds: 0,
        role: Role::Main,
        analysis: Analysis::default(),
        review: None,
        pending_coverage: None,
        transcript: Vec::new(),
        main_progress: Default::default(),
        main_work: None,
        done: false,
        source_views: BTreeMap::new(),
        draft_stage: crate::analysis::draft::DraftStage::Outline,
        draft_outline_gaps: None,
        draft_outline_stalls: 0,
        draft_outline_window: 0,
        outline_config_sha256: None,

        outline_run: Default::default(),
    }
}

fn pack_input(count: usize, text: &str) -> FrozenInput {
    FrozenInput {
        schema_version: 2,
        project_id: "project".into(),
        document_set_id: "set".into(),
        documents: vec![],
        document_relations: vec![],
        source_units: (0..count)
            .map(|ordinal| crate::analysis::Source {
                source_unit_revision_id: format!("s{ordinal}"),
                document_id: "doc".into(),
                text: text.into(),
                locator: json!({"heading_path": format!("章{ordinal}")}),
                ordinal,
            })
            .collect(),
        structured_forms: vec![],
        decisions: vec![],
    }
}

#[tokio::test]
async fn sizing_does_not_stick_claims_and_an_oversized_brief_releases_packs() {
    let input = pack_input(6, &"甲乙丙丁".repeat(2_000));
    let mut config = crate::analysis::tests::config();
    config.limits.max_context_tokens = 32_000;
    let mut state = outline_checkpoint();
    let before = state.outline_run.reading_packs.clone();
    super::super::prepare_request(&input, &config, &mut state, false)
        .await
        .unwrap();
    assert_eq!(state.outline_run.reading_packs, before);
    let bytes = super::super::prepare_request(&input, &config, &mut state, true)
        .await
        .unwrap();
    let counts = state
        .outline_run
        .reading_packs
        .as_ref()
        .unwrap()
        .pack_counts();
    assert!(
        counts.running >= 1 && counts.running <= crate::outline::discover::DEFAULT_PACK_CONCURRENCY
    );
    assert!(counts.running < counts.total);
    assert!(counts.pending > 0);
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    let brief: Value =
        serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
    let packs = brief["reading_packs"].as_array().unwrap();
    assert_eq!(packs.len(), counts.running);
    let decoded = state.outline_run.tool_draft.model_wire.decode(json!({"wire_scope":serde_json::from_str::<Value>(body["messages"].as_array().unwrap().last().unwrap()["content"].as_str().unwrap()).unwrap()["wire_scope"],"pack_id":packs[0]["pack"]["id"]}),false).unwrap();
    assert_eq!(decoded["pack_id"], "pack-0");
    let running = counts.running;
    super::super::prepare_request(&input, &config, &mut state, true)
        .await
        .unwrap();
    assert_eq!(
        state
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .pack_counts()
            .running,
        running
    );
}

#[test]
fn pack_commits_are_discover_progress_and_later_phases_can_block() {
    use crate::outline::discover::{DEFAULT_PACK_CONCURRENCY, DiscoverWork, PackSubmit};
    let input = pack_input(3, "A");
    let mut state = outline_checkpoint();
    state.outline_run.reading_packs = Some(DiscoverWork::plan_with_budget(&input, &|sessions| {
        Ok(sessions.iter().all(|session| {
            session["pack"]["atoms"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|atom| atom["context_only"] == false)
                .count()
                <= 1
        }) && crate::outline::discover::test_sessions_fit(sessions, 8192)?)
    }));
    let packs = state
        .outline_run
        .reading_packs
        .as_mut()
        .unwrap()
        .claim(DEFAULT_PACK_CONCURRENCY);
    observe_progress(
        &mut state,
        &Role::Main,
        None,
        &crate::analysis::tests::config().limits,
    )
    .unwrap();
    state
        .analysis
        .outline
        .scanned
        .text
        .insert("s0".into(), vec![(0, 1)]);
    observe_progress(
        &mut state,
        &Role::Main,
        None,
        &crate::analysis::tests::config().limits,
    )
    .unwrap();
    assert_eq!(state.main_progress.watch.no_progress_turns, 1);
    state
        .outline_run
        .reading_packs
        .as_mut()
        .unwrap()
        .submit(
            &input,
            "pack-0",
            PackSubmit {
                call_id: "ok".into(),
                claim_token: packs[0].claim_token.clone(),
                pack_revision: packs[0].pack_revision,
                requirements: vec![],
                no_requirement_reason: Some("Only explanatory text".into()),
                inspected_atom_ids: packs[0].atoms.iter().map(|a| a.id.clone()).collect(),
            },
        )
        .unwrap();
    observe_progress(
        &mut state,
        &Role::Main,
        None,
        &crate::analysis::tests::config().limits,
    )
    .unwrap();
    assert_eq!(state.main_progress.watch.no_progress_turns, 0);
    assert_eq!(state.main_progress.watch.recovery, Recovery::Running);

    state.main_progress = Default::default();
    state.analysis.outline.phase = crate::analysis::outline_flow::Phase::Check;
    let limits = crate::analysis::tests::config().limits;
    observe_progress(&mut state, &Role::Main, None, &limits).unwrap();
    observe_progress(&mut state, &Role::Main, None, &limits).unwrap();
    assert_eq!(state.main_progress.watch.no_progress_turns, 1);
    state.outline_run.tool_draft.slots_submitted = true;
    observe_progress(&mut state, &Role::Main, None, &limits).unwrap();
    assert_eq!(state.main_progress.watch.no_progress_turns, 0);
    state.main_progress = Default::default();
    // The first observation records the completion key and resets the watch.
    // Each later window is one no-progress or focus allowance, repeated once
    // per replan plus the window that blocks.
    let window = limits.max_no_progress_turns.min(limits.max_focus_turns);
    let stalls = window
        .saturating_mul(limits.max_focus_replans.saturating_add(1))
        .saturating_add(1);
    for _ in 0..stalls {
        observe_progress(&mut state, &Role::Main, None, &limits).unwrap();
    }
    assert_eq!(state.main_progress.watch.recovery, Recovery::Blocked);
    assert!(super::super::outline_execution_blocked(&state));
    let stalled = super::super::outline_stall_message(&state);
    assert!(
        stalled.contains("phase check"),
        "stall error names the phase: {stalled}"
    );
    state.analysis.outline.phase = crate::analysis::outline_flow::Phase::Discover;
    let stalled = super::super::outline_stall_message(&state);
    assert!(
        stalled.contains("phase discover"),
        "stall error names the phase: {stalled}"
    );
    let mut scanned = outline_checkpoint();
    observe_progress(&mut scanned, &Role::Main, None, &limits).unwrap();
    observe_progress(&mut scanned, &Role::Main, None, &limits).unwrap();
    assert_eq!(scanned.main_progress.watch.no_progress_turns, 1);
    scanned
        .analysis
        .outline
        .scanned
        .text
        .insert("s0".into(), vec![(0, 1)]);
    observe_progress(&mut scanned, &Role::Main, None, &limits).unwrap();
    assert_eq!(scanned.main_progress.watch.no_progress_turns, 0);
    scanned.main_progress.watch.recovery = Recovery::Blocked;
    scanned.analysis.outline.phase = crate::analysis::outline_flow::Phase::Check;
    assert!(!super::super::outline_execution_blocked(&scanned));
    scanned.analysis.outline.phase = crate::analysis::outline_flow::Phase::Discover;
    assert!(super::super::outline_execution_blocked(&scanned));
}

#[test]
fn outline_visual_receipts_require_actual_pixels_and_original_object_identity() {
    use sha2::{Digest, Sha256};
    let mut input = pack_input(1, "");
    let mut png = Vec::new();
    image::DynamicImage::new_rgb8(2, 2)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let digest = hex::encode(Sha256::digest(&png));
    input.source_units[0].locator = json!({
        "locator_kind":"image", "image_available":true, "vision_required":true,
        "image_ref":format!("objects/{digest}"), "completeness":"complete", "heading_path":"Image"
    });
    let source_id = input.source_units[0].source_unit_revision_id.clone();
    let view =
        crate::outline::visual::render_frozen_image_view(&source_id, &digest, 0, &png, 1600, 16000)
            .unwrap();
    view.validate(&source_id, 1600, 16000).unwrap();
    assert!(view.validate(&source_id, 1600, 1).is_err());
    let mut altered = view.clone();
    altered.identity.image_sha256 = "0".repeat(64);
    assert!(altered.validate(&source_id, 1600, 16000).is_err());
    let mut state = outline_checkpoint();
    let mut packs = crate::outline::discover::DiscoverWork::plan(&input, 8000);
    packs.claim(1);
    state.outline_run.reading_packs = Some(packs);
    state.source_views.insert(view.id().unwrap(), view.clone());
    let config = crate::analysis::tests::config();
    super::super::confirm_outline_visual_delivery(
        &input,
        &config,
        &mut state,
        &json!({"messages":[{"role":"user","content":"cached metadata only"}]}),
    )
    .unwrap();
    assert!(state.outline_run.tool_draft.delivered_evidence.is_empty());
    super::super::confirm_outline_visual_delivery(
        &input,
        &config,
        &mut state,
        &json!({"messages":[view.message()]}),
    )
    .unwrap();
    assert!(state.outline_run.tool_draft.delivered_evidence.iter().any(|reference|matches!(reference,crate::outline::evidence::EvidenceRef::ImageRegion{image_id,..} if image_id==&source_id)));
    assert!(
        state.outline_run.tool_draft.check_reads.evidence.is_empty(),
        "Discover pixels are not independent Check receipts"
    );
    let mut no_vision = config.clone();
    no_vision.limits.vision_enabled = false;
    assert!(
        super::super::confirm_outline_visual_delivery(
            &input,
            &no_vision,
            &mut state,
            &json!({"messages":[view.message()]})
        )
        .is_err()
    );
    for cached in state.source_views.values_mut() {
        cached.identity.original_sha256 = "f".repeat(64);
    }
    assert!(
        super::super::confirm_outline_visual_delivery(
            &input,
            &config,
            &mut state,
            &json!({"messages":[view.message()]})
        )
        .unwrap_err()
        .contains("original hash")
    );
}

#[tokio::test]
async fn discovery_request_over_128k_bytes_fits_without_splitting_when_tokens_fit() {
    let text = "中文 evidence 😀\n".repeat(8_000);
    let input = pack_input(1, &text);
    let config = crate::analysis::tests::config();
    let mut state = outline_checkpoint();
    let bytes = super::super::prepare_request(&input, &config, &mut state, true)
        .await
        .unwrap();
    assert!(
        bytes.len() > 131_072,
        "fixture must exceed the removed byte cap"
    );
    assert!(
        crate::analysis::tests::total_request_tokens(&bytes, &config)
            <= config.limits.max_context_tokens
    );
    let work = state.outline_run.reading_packs.as_ref().unwrap();
    assert_eq!(
        work.pack_counts().total,
        1,
        "byte length cannot split fitting original text"
    );
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    let brief: Value =
        serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
    assert_eq!(
        brief["reading_packs"][0]["pack"]["atoms"][0]["excerpts"][0]["quote"],
        text
    );
}

#[tokio::test]
async fn discovery_planning_includes_post_plan_progress_at_the_token_boundary() {
    let input = pack_input(1, &"采购技术要求与交付标准。".repeat(3_000));
    let config = crate::analysis::tests::config();
    let mut wide = outline_checkpoint();
    let bytes = super::super::prepare_request(&input, &config, &mut wide, true)
        .await
        .unwrap();
    assert_eq!(
        wide.outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .pack_counts()
            .total,
        1
    );
    let mut body: Value = serde_json::from_slice(&bytes).unwrap();
    let actual_tokens = crate::analysis::tests::request_tokens(&body, &config);
    let host_message = body["messages"].as_array_mut().unwrap().last_mut().unwrap();
    let mut host: Value = serde_json::from_str(host_message["content"].as_str().unwrap()).unwrap();
    for key in [
        "outline_pack_total",
        "outline_pack_pending",
        "outline_pack_running",
        "outline_pack_failed",
        "outline_pack_committed",
    ] {
        host["progress"].as_object_mut().unwrap().remove(key);
    }
    host_message["content"] = json!(host.to_string());
    let pre_plan_tokens = crate::analysis::tests::request_tokens(&body, &config);
    assert!(pre_plan_tokens < actual_tokens);

    // This exact boundary admitted the entire chapter before planning, then
    // rejected its real request once the five progress fields were introduced.
    let mut constrained = config.clone();
    constrained.limits.max_context_tokens =
        pre_plan_tokens + constrained.provider.output_token_reserve as usize;
    let mut state = outline_checkpoint();
    let bytes = super::super::prepare_request(&input, &constrained, &mut state, true)
        .await
        .unwrap();
    assert!(
        crate::analysis::tests::total_request_tokens(&bytes, &constrained)
            <= constrained.limits.max_context_tokens
    );
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
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    let host: Value = serde_json::from_str(
        body["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(host["progress"]["turn"], 0);
    assert_eq!(
        host["progress"]["outline_pack_total"],
        state
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .pack_counts()
            .total
    );
}

#[tokio::test]
async fn discovery_failed_feedback_near_context_boundary_can_be_repaired() {
    failed_feedback_repair_scenario(2).await;
}
#[tokio::test]
async fn preferred_handles_failed_receipt_is_retained_evicted_and_repaired() {
    failed_feedback_repair_scenario(4).await;
}
async fn failed_feedback_repair_scenario(attempts: usize) {
    let input = pack_input(1, &"采购技术要求与交付标准。".repeat(3_000));
    let config = crate::analysis::tests::config();
    let mut state = outline_checkpoint();
    let bytes = super::super::prepare_request(&input, &config, &mut state, true)
        .await
        .unwrap();
    let mut constrained = config.clone();
    constrained.limits.max_context_tokens =
        crate::analysis::tests::total_request_tokens(&bytes, &config)
            + constrained.provider.output_token_reserve as usize
            + super::super::DISCOVERY_FEEDBACK_RESERVE_TOKENS
            + 512;
    state = outline_checkpoint();
    super::super::prepare_request(&input, &constrained, &mut state, true)
        .await
        .unwrap();
    let work = state.outline_run.reading_packs.as_ref().unwrap();
    assert_eq!(work.pack_counts().total, 1);
    let session = work.session(&input, "pack-0").unwrap();
    let invalid_identity = json!({"pack_id":"pack-0","call_id":"wrong-identity",
        "claim_token":"wrong-claim","pack_revision":session["pack"]["pack_revision"],"repair":false,
        "requirements":[],"no_requirement_reason":"No requirements","inspected_atom_ids":[]});
    let unchanged = work.clone();
    let rejection =
        crate::outline::agent::apply(&input, &mut state, "submit_pack", &invalid_identity).unwrap();
    assert_eq!(rejection["ok"], false);
    assert_eq!(
        state.outline_run.reading_packs.as_ref().unwrap(),
        &unchanged
    );
    assert!(!unchanged.retains_submission(&invalid_identity));
    state.transcript.push(json!({"role":"assistant","content":null,"tool_calls":[{
        "id":"wrong-identity","type":"function","function":{"name":"submit_pack","arguments":invalid_identity.to_string()}}]}));
    state
        .transcript
        .push(json!({"role":"tool","tool_call_id":"wrong-identity",
        "content":json!({"ok":true,"result":rejection}).to_string()}));
    super::super::prepare_request(&input, &constrained, &mut state, true)
        .await
        .unwrap();
    assert_eq!(
        state.transcript.len(),
        2,
        "identity rejection has no durable receipt to deduplicate"
    );
    for attempt in 0..attempts {
        let session = state
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .session(&input, "pack-0")
            .unwrap();
        let pack = &session["pack"];
        let requirement = json!({
            "obligation_strength":"mandatory", "extraction_quality":"explicit",
            "description":"A requirement", "kind":"technical",
            "source_section_id":pack["atoms"][0]["section_id"],
            "evidence":[{"kind":"text","input_digest":pack["input_digest"],
                "unit_id":"foreign-source","start_byte":0,"end_byte":1}]
        });
        let mut args = json!({"pack_id":"pack-0","call_id":session["submission_operation_id"],
            "claim_token":pack["claim_token"],"pack_revision":pack["pack_revision"],"repair":attempt > 0,
            "requirements":vec![requirement;if attempt == 0 {1} else {24}],
            "no_requirement_reason":if attempt == 0 { json!("No requirements, but missing inspection IDs") } else { Value::Null },
            "inspected_atom_ids":[]});
        {
            for requirement in args["requirements"].as_array_mut().unwrap() {
                requirement["source_section_id"] = json!({"atom_key":pack["atoms"][0]["atom_key"]});
                requirement["evidence"] =
                    json!([{"evidence_key":pack["atoms"][0]["excerpts"][0]["evidence_key"]}]);
            }
            args["inspected_atom_ids"] = json!([{"atom_key":pack["atoms"][0]["atom_key"]}]);
            // Domain failure after valid handle decoding, hence a durable receipt.
            args["no_requirement_reason"] = json!("contradictory positive and negative result");
        }
        assert!(
            constrained
                .limits
                .tokenizer
                .count_text_tokens(&args.to_string())
                .unwrap()
                < constrained.provider.output_token_reserve as usize
        );
        let feedback =
            crate::outline::agent::apply(&input, &mut state, "submit_pack", &args).unwrap();
        assert_eq!(feedback["ok"], false);
        let work = state.outline_run.reading_packs.as_ref().unwrap();
        assert_eq!(work.pack_counts().failed, 1);
        assert!(work.retains_submission(&args));
        if attempt > 0 {
            // Durable history may resolve old keys, but a new model submission
            // carrying those same keys must fail against the rotated live claim.
            let current_pack = work.session(&input, "pack-0").unwrap();
            let mut replay = args.clone();
            replay["call_id"] = current_pack["submission_operation_id"].clone();
            replay["claim_token"] = current_pack["pack"]["claim_token"].clone();
            replay["pack_revision"] = current_pack["pack"]["pack_revision"].clone();
            let mut rejected = state.clone();
            let before = json!(rejected.outline_run);
            let decoded =
                crate::outline::source_wire::resolve(&rejected, "submit_pack", replay).unwrap();
            assert!(
                crate::outline::agent::apply(&input, &mut rejected, "submit_pack", &decoded)
                    .is_err()
            );
            assert_eq!(
                json!(rejected.outline_run),
                before,
                "historical receipt lookup cannot authorize stale live keys"
            );
        }
        let mut stale = args.clone();
        stale["claim_token"] = json!("wrong-claim");
        assert!(!work.retains_submission(&stale));
        if attempt > 0 {
            for (field, value) in [
                ("pack_id", json!("another-pack")),
                ("evidence_index", json!(999)),
                ("unadvertised", json!(true)),
            ] {
                let mut altered = args.clone();
                altered["requirements"][0]["evidence"][0][field] = value;
                assert!(!work.retains_submission(&altered));
            }
        }
        let durable = work.session(&input, "pack-0").unwrap()["feedback"].clone();
        let id = format!("tool-{attempt}");
        state.transcript.push(json!({"role":"assistant","content":null,"tool_calls":[{
            "id":id,"type":"function","function":{"name":"submit_pack","arguments":args.to_string()}}]}));
        state
            .transcript
            .push(json!({"role":"tool","tool_call_id":id,
            "content":json!({"ok":false,"result":feedback}).to_string()}));
        state.turn += 1;
        if attempt > 0 {
            // Compact handles may naturally fit without eviction. Explicitly
            // exercise the durable-receipt eviction branch under pressure.
            assert!(evict_completed_discovery_history(&mut state, 0));
        }
        let bytes = super::super::prepare_request(&input, &constrained, &mut state, true)
            .await
            .unwrap();
        assert!(
            crate::analysis::tests::total_request_tokens(&bytes, &constrained)
                <= constrained.limits.max_context_tokens
        );
        assert_eq!(
            state
                .outline_run
                .reading_packs
                .as_ref()
                .unwrap()
                .session(&input, "pack-0")
                .unwrap()["feedback"],
            durable
        );
    }
    assert!(
        state.transcript.len() < 8,
        "repeated failed groups must not accumulate forever"
    );
    let session = state
        .outline_run
        .reading_packs
        .as_ref()
        .unwrap()
        .session(&input, "pack-0")
        .unwrap();
    let pack = &session["pack"];
    let repaired = crate::outline::agent::apply(&input, &mut state, "submit_pack", &json!({
        "pack_id":"pack-0","call_id":session["submission_operation_id"], "claim_token":pack["claim_token"],
        "pack_revision":pack["pack_revision"],"repair":true,"requirements":[],
        "no_requirement_reason":"All source atoms inspected; no obligation found in this fixture",
        "inspected_atom_ids":pack["atoms"].as_array().unwrap().iter().map(|atom|json!({"atom_key":atom["atom_key"]})).collect::<Vec<_>>()
    })).unwrap();
    assert_eq!(repaired["ok"], true);
    assert!(state.outline_run.reading_packs.as_ref().unwrap().complete());
}

#[test]
fn discovery_feedback_projection_keeps_complete_durable_errors() {
    let session = json!({"feedback":{"total":8,"errors":(0..8).map(|i|json!({"path":format!("/requirements/{i}"),"message":"Correct this field"})).collect::<Vec<_>>(),"truncated":false}});
    let mut projected = vec![session.clone()];
    for shown in [4, 2, 1] {
        assert!(super::super::shrink_discovery_feedback(&mut projected));
        assert_eq!(
            projected[0]["feedback"]["errors"].as_array().unwrap().len(),
            shown
        );
        assert_eq!(projected[0]["feedback"]["total"], 8);
        assert_eq!(projected[0]["feedback"]["omitted_errors"], 8 - shown);
        assert_eq!(projected[0]["feedback"]["truncated"], true);
    }
    assert!(!super::super::shrink_discovery_feedback(&mut projected));
    assert_eq!(session["feedback"]["errors"].as_array().unwrap().len(), 8);
}

#[tokio::test]
async fn discovery_planning_reserves_uncached_and_cached_images_without_delivering_placeholders() {
    use sha2::{Digest, Sha256};
    for cache_images in [false, true] {
        let mut input = pack_input(2, "");
        let mut state = outline_checkpoint();
        for (index, source) in input.source_units.iter_mut().enumerate() {
            let pixels =
                image::ImageBuffer::from_pixel(2, 2, image::Rgb([index as u8 * 127, 0, 0]));
            let mut png = Vec::new();
            image::DynamicImage::ImageRgb8(pixels)
                .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                .unwrap();
            let hash = hex::encode(Sha256::digest(&png));
            source.locator = json!({"locator_kind":"image","image_available":true,"vision_required":true,
                "image_ref":format!("objects/{hash}"),"completeness":"complete"});
            if cache_images {
                let view = crate::outline::visual::render_frozen_image_view(
                    &source.source_unit_revision_id,
                    &hash,
                    0,
                    &png,
                    1600,
                    16000,
                )
                .unwrap();
                state.source_views.insert(view.id().unwrap(), view);
            }
        }
        let config = crate::analysis::tests::config();
        let untouched = json!(state);
        let mut wide = state.clone();
        let bytes = super::super::prepare_request(&input, &config, &mut wide, true)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        let sessions = wide
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .inflight_sessions(&input);
        assert_eq!(
            wide.outline_run
                .reading_packs
                .as_ref()
                .unwrap()
                .pack_counts()
                .total,
            1
        );
        let mut planned = body.clone();
        super::super::reserve_pack_image_messages(&input, &config, &state, &sessions, &mut planned)
            .unwrap();
        let base = crate::analysis::tests::request_tokens(&body, &config);
        assert!(
            crate::analysis::tests::request_tokens(&planned, &config)
                >= base + 2 * config.limits.image_token_reserve
        );
        assert_eq!(
            json!(state),
            untouched,
            "sizing cannot grant visual receipts or cache fake views"
        );
        let mut constrained = config.clone();
        constrained.limits.max_context_tokens = base
            + config.limits.image_token_reserve
            + 2048
            + config.provider.output_token_reserve as usize;
        let bytes = super::super::prepare_request(&input, &constrained, &mut state, true)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            state
                .outline_run
                .reading_packs
                .as_ref()
                .unwrap()
                .pack_counts()
                .total,
            2,
            "future image token cost must split otherwise-small image metadata"
        );
        assert!(
            body["messages"].as_array().unwrap().iter().all(|message| {
                message["content"]
                    .as_array()
                    .is_none_or(|parts| parts.iter().all(|part| part["type"] != "image_url"))
            }),
            "planning placeholders never reach the actual provider request"
        );
        assert!(state.outline_run.tool_draft.delivered_evidence.is_empty());
        assert!(
            crate::analysis::tests::total_request_tokens(&bytes, &constrained)
                <= constrained.limits.max_context_tokens
        );
    }
}

#[tokio::test]
async fn malformed_handle_near_output_limit_keeps_schema_feedback_repairable() {
    let input = pack_input(1, &"采购技术要求与交付标准。".repeat(3000));
    for schema_failure in [true, false] {
        let config = crate::analysis::tests::config();
        let mut state = outline_checkpoint();
        let bytes = super::super::prepare_request(&input, &config, &mut state, true)
            .await
            .unwrap();
        let mut constrained = config.clone();
        constrained.limits.max_context_tokens =
            crate::analysis::tests::total_request_tokens(&bytes, &config)
                + constrained.provider.output_token_reserve as usize;
        // Replan through the real policy under this constrained window. The
        // response+feedback allowance must be secured before the first send.
        state = outline_checkpoint();
        super::super::prepare_request(&input, &constrained, &mut state, true)
            .await
            .unwrap();
        let session = state
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .session(&input, "pack-0")
            .unwrap();
        let pack = &session["pack"];
        let mut args = json!({"pack_id":"pack-0","call_id":"malformed-near-output-limit","claim_token":pack["claim_token"],
            "pack_revision":pack["pack_revision"],"repair":false,"requirements":[{"description":"A requirement",
            "kind":"technical","obligation_strength":"mandatory","extraction_quality":"explicit",
            "source_section_id":{"pack_id":"pack-0","atom_ref":0},"evidence":[{"pack_id":"pack-0","atom_ref":999,"evidence_index":0}]}],
            "no_requirement_reason":null,"inspected_atom_ids":[{"pack_id":"pack-0","atom_ref":0}]});
        if schema_failure {
            args["requirements"][0]["evidence"][0]["evidence_index"] = json!("not-an-integer");
        }
        // Count the complete assistant envelope, not just its description.
        let assistant = |args: &Value| json!({"role":"assistant","content":null,"tool_calls":[{"id":"near-output","type":"function","function":{"name":"submit_pack","arguments":args.to_string()}}]});
        let mut pad = String::new();
        loop {
            let next = pad.clone() + &"x ".repeat(16);
            args["unadvertised_padding"] = json!(next);
            let tokens = config
                .limits
                .tokenizer
                .count_text_tokens(&assistant(&args).to_string())
                .unwrap();
            if tokens >= config.provider.output_token_reserve as usize - 32 {
                break;
            }
            pad.push_str(&"x ".repeat(16));
        }
        args["unadvertised_padding"] = json!(pad);
        // For the invalid-handle case keep schema valid: put padding in assistant content.
        let content = args
            .as_object_mut()
            .unwrap()
            .remove("unadvertised_padding")
            .unwrap();
        let mut message = assistant(&args);
        message["content"] = content;
        assert!(
            config
                .limits
                .tokenizer
                .count_text_tokens(&message.to_string())
                .unwrap()
                < config.provider.output_token_reserve as usize
        );
        let before = state.outline_run.reading_packs.clone();
        let error =
            crate::outline::agent::apply(&input, &mut state, "submit_pack", &args).unwrap_err();
        assert_eq!(state.outline_run.reading_packs, before);
        assert!(
            !state
                .outline_run
                .reading_packs
                .as_ref()
                .unwrap()
                .retains_submission(&args)
        );
        state.transcript.push(message);
        state.transcript.push(json!({"role":"tool","tool_call_id":"near-output","content":json!({"ok":false,"error":error}).to_string()}));
        let request = super::super::prepare_request(&input, &constrained, &mut state, true)
            .await
            .unwrap();
        assert!(
            crate::analysis::tests::total_request_tokens(&request, &constrained)
                <= constrained.limits.max_context_tokens
        );
        assert_eq!(
            state.transcript.len(),
            2,
            "non-durable latest feedback must not be silently dropped"
        );
    }
}

#[tokio::test]
#[ignore = "requires private paused run; read-only compatibility and actual request admission"]
async fn private_paused_handle_run_fits_current_code_without_hash_changes() {
    let root = std::path::PathBuf::from(std::env::var("KB_PRIVATE_PROJECTION_DIR").unwrap());
    let run = root.join("handles-full-live-run");
    let read = |path: std::path::PathBuf| -> Value {
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    };
    let input: FrozenInput =
        serde_json::from_value(read(root.join("actual-frozen/frozen-input.json"))).unwrap();
    let archived: Config = serde_json::from_value(read(run.join("runtime.json"))).unwrap();
    let current = Config::with_provider_for(
        archived.provider.clone(),
        archived.limits.clone(),
        Some(&input),
    )
    .unwrap();
    assert_eq!(json!(current), json!(archived));
    let original: Checkpoint = serde_json::from_value(read(run.join("checkpoint.json"))).unwrap();
    assert_eq!(original.config_sha256, digest(&current).unwrap());
    assert_eq!(original.input_sha256, digest(&input).unwrap());
    original.journal.validate(original.turn, "main").unwrap();
    let mut probe = original.clone();
    let bytes = super::super::prepare_request(&input, &current, &mut probe, false)
        .await
        .unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    let host: Value = serde_json::from_str(
        value["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert!(host.get("sources").is_none() && host.get("outline").is_none());
    assert_eq!(
        original
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .requirement_records(),
        probe
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .requirement_records()
    );
    let accounting = crate::agent_runtime::chat::estimate_request_tokens(
        &value,
        &current.limits.tokenizer,
        current.limits.image_token_reserve,
        current.limits.token_safety_margin,
    )
    .unwrap();
    assert!(accounting.total_context_tokens <= current.limits.max_context_tokens);
    let retained = original.journal.body().unwrap();
    let retained_value: Value = serde_json::from_slice(retained).unwrap();
    let retained_accounting = crate::agent_runtime::chat::estimate_request_tokens(
        &retained_value,
        &current.limits.tokenizer,
        current.limits.image_token_reserve,
        current.limits.token_safety_margin,
    )
    .unwrap();
    assert!(retained_accounting.total_context_tokens <= current.limits.max_context_tokens);
    let original_again: Checkpoint =
        serde_json::from_value(read(run.join("checkpoint.json"))).unwrap();
    assert_eq!(
        json!(original),
        json!(original_again),
        "offline fit must not mutate checkpoint"
    );
    std::fs::write(root.join("paused-run-compatibility.json"),serde_json::to_vec_pretty(&json!({"config_matches":true,"input_matches":true,
        "checkpoint_unchanged":true,"config_sha256":original.config_sha256,"tools_sha256":current.tools_sha256,
        "same_pending_request_bytes":bytes==retained,"current_preparation_accounting":accounting,"pending_request_accounting":retained_accounting,
        "committed_packs":original.outline_run.reading_packs.as_ref().unwrap().pack_counts().committed,"resume_will_use_exact_pending_bytes":true})).unwrap()).unwrap();
}

#[test]
fn outline_recovery_keeps_unique_pages_but_releases_exact_duplicates() {
    let assistant = |id: &str, name: &str| json!({"role":"assistant","tool_calls":[{"id":id,"type":"function","function":{"name":name,"arguments":"{}"}}]});
    let result = |id: &str, value: Value| json!({"role":"tool","tool_call_id":id,"content":json!({"ok":true,"result":value}).to_string()});
    let input = pack_input(1, "source");
    let mut state = super::super::retirement::checkpoint(&input);
    for name in ["read_requirements", "read_outline", "read_evidence"] {
        let page = vec![
            assistant("page", name),
            result(
                "page",
                json!({"version":"v1","items":[{"requirement_id":"r1","description":"complete required response"}],"next_cursor":1}),
            ),
        ];
        let latest = vec![
            assistant("latest", "read_outline"),
            result(
                "latest",
                json!({"version":"v2","items":[],"next_cursor":null}),
            ),
        ];
        state.transcript = [page.clone(), latest.clone()].concat();
        let before = state.transcript.clone();
        assert!(
            !evict_delivered_group(&mut state, usize::MAX, false),
            "{name}: unique page was lost during recovery"
        );
        assert_eq!(state.transcript, before);
        state.transcript = [page.clone(), page.clone(), latest.clone()].concat();
        assert!(evict_delivered_group(&mut state, usize::MAX, false));
        assert_eq!(state.transcript, [page, latest].concat());
    }
}

#[test]
fn outline_pages_allow_bounded_eviction_and_exact_cursor_recovery() {
    let input = pack_input(1, "source");
    let mut state = super::super::retirement::checkpoint(&input);
    let page = |id: &str, cursor: usize| {
        vec![
            json!({"role":"assistant","tool_calls":[{"id":id,"type":"function","function":{"name":"read_requirements","arguments":json!({"cursor":cursor,"version":"v1"}).to_string()}}]}),
            json!({"role":"tool","tool_call_id":id,"content":json!({"ok":true,"result":{"version":"v1","items":[{"requirement_id":format!("r{cursor}")}],"next_cursor":cursor+1}}).to_string()}),
        ]
    };
    let first = page("a", 0);
    let last = page("b", 1);
    state.transcript = [first.clone(), last.clone()].concat();
    assert!(!evict_delivered_group(&mut state, 1, false));
    assert!(
        evict_delivered_group(&mut state, 1, true),
        "genuine context pressure may reclaim unique data"
    );
    assert_eq!(state.transcript, last);
    let recovered = page("c", 0);
    assert_eq!(
        visible_work_evidence(&state, &first),
        visible_work_evidence(&state, &recovered)
    );
    let result: Value = serde_json::from_str(recovered[1]["content"].as_str().unwrap()).unwrap();
    assert_eq!(result["result"]["version"], "v1");
    assert_eq!(result["result"]["next_cursor"], 1);
}

#[tokio::test]
#[ignore = "offline private recorded Organize recovery reproduction"]
async fn private_organize_recovery_preserves61_visible_requirements() {
    let root = std::path::PathBuf::from(std::env::var("KB_PRIVATE_PROJECTION_DIR").unwrap());
    let mut state: Checkpoint = serde_json::from_slice(
        &std::fs::read(root.join("handles-full-live-run/checkpoint.json")).unwrap(),
    )
    .unwrap();
    let request: Value = serde_json::from_slice(
        &std::fs::read(root.join("handles-full-live-run/turn-20-main.json")).unwrap(),
    )
    .unwrap();
    state.transcript = request["messages"].as_array().unwrap()
        [2..request["messages"].as_array().unwrap().len() - 1]
        .to_vec();
    for m in &mut state.transcript {
        if let Some(parts) = m["content"].as_array() {
            m["content"] = json!(
                parts
                    .iter()
                    .filter_map(|p| p["text"].as_str())
                    .collect::<String>()
            );
        }
    }
    let before = visible_work_evidence(&state, &state.transcript);
    while evict_delivered_group(&mut state, usize::MAX, false) {}
    let after = visible_work_evidence(&state, &state.transcript);
    assert_eq!(before, after, "unique pages lost by recovery");
    assert!(
        after
            .keys()
            .filter(|k| k.starts_with("outline-page:read_requirements:"))
            .count()
            >= 7
    );
    std::fs::write(root.join("organize61-recovery-result.json"),serde_json::to_vec_pretty(&json!({"unique_payloads_before":before.len(),"unique_payloads_after":after.len(),"checkpoint_unchanged":true,"http_calls":0})).unwrap()).unwrap();
}

#[tokio::test]
#[ignore = "offline actual response through execute_turn; requires KB_PRIVATE_PROJECTION_DIR"]
async fn private_runtime_check_repair_transition_replays_saved_response() {
    struct NoIo;
    #[async_trait]
    impl Journal for NoIo {
        async fn load(&self) -> Result<Option<Checkpoint>, AgentError> {
            panic!("offline")
        }
        async fn reserve(&self, _: &Checkpoint, _: &[u8]) -> Result<Option<usize>, AgentError> {
            panic!("offline")
        }
        async fn save(&self, _: &Checkpoint, _: &Value) -> Result<(), AgentError> {
            panic!("offline")
        }
    }
    let root = std::path::PathBuf::from(std::env::var("KB_PRIVATE_PROJECTION_DIR").unwrap());
    let run = root.join("production-claim-cycle-v2");
    let read = |path: std::path::PathBuf| -> Value {
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    };
    let input: FrozenInput =
        serde_json::from_value(read(run.join("frozen-scoped-input.json"))).unwrap();
    let mut state: Checkpoint =
        serde_json::from_value(read(run.join("checkpoint-6.json"))).unwrap();
    // Old handler fixture increments before dispatch; runtime increments afterwards.
    // Align the saved completed-read boundary, without inventing reading receipts.
    state.turn += 1;
    let limits = read(root.join("handles-full-live-limits.json"));
    let config = Config::with_provider_for(
        crate::authoring_runtime::AuthoringRuntimeContractV1::resolve_tools_from_environment()
            .unwrap(),
        serde_json::from_value(limits["extraction"].clone()).unwrap(),
        Some(&input),
    )
    .unwrap();
    let body = serde_json_canonicalizer::to_vec(&read(run.join("request-7.json"))).unwrap();
    state.journal = Default::default();
    state.journal.sequence = state.turn * 3;
    state
        .journal
        .prepare_session(
            &body,
            crate::agent_runtime::SESSION_PREFIX,
            crate::agent_runtime::ANALYSIS_SESSION_SUFFIX,
            config.limits.context_wire_ceiling(),
        )
        .unwrap();
    state.journal.prepare(state.turn, "main", &body).unwrap();
    let response: ChatTurn =
        serde_json::from_value(read(run.join("response-7.json"))["response"].clone()).unwrap();
    state.journal.responded(response.clone()).unwrap();
    state.journal.validate(state.turn, "main").unwrap();
    let before = state.clone();
    let results = super::execute_turn(
        &input,
        &config,
        &mut state,
        &NoIo,
        response.clone(),
        BTreeMap::new(),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        crate::outline::agent::current(&input, &state),
        crate::outline::agent::Duty::Discover
    );
    assert_eq!(state.outline_run.repair_events.len(), 1);
    assert_eq!(state.outline_run.repair_events[0]["status"], "reopened");
    assert!(state.outline_run.tool_draft.chapters.is_empty());
    assert!(state.outline_run.tool_draft.check_reads.evidence.is_empty());
    assert!(!state.done);
    assert!(!state.outline_run.tool_draft.finished);
    let mut recovered: Checkpoint = serde_json::from_value(json!(before)).unwrap();
    super::execute_turn(
        &input,
        &config,
        &mut recovered,
        &NoIo,
        response,
        BTreeMap::new(),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        json!(state.outline_run),
        json!(recovered.outline_run),
        "same saved batch recovery is deterministic"
    );
    let rounds = state.outline_run.repair_events.len();
    assert!(!crate::outline::agent::finish_check_repair_batch(&input, &mut state).unwrap());
    assert_eq!(state.outline_run.repair_events.len(), rounds);
    std::fs::write(root.join("runtime-transition-replay-result.json"),serde_json::to_vec_pretty(&json!({"runtime_execute_turn":true,"saved_response":7,"no_provider_calls":true,"transition":"Check -> Discover","draft_invalidated":true,"fresh_receipts_required":true,"saved_response_recovery_deterministic":true,"tool_results":results,"repair_events":state.outline_run.repair_events})).unwrap()).unwrap();
    std::fs::write(
        root.join("runtime-transition-reopened-checkpoint.json"),
        serde_json::to_vec_pretty(&state).unwrap(),
    )
    .unwrap();
}

#[tokio::test]
#[ignore = "offline deterministic runtime full cycle; not model semantics; requires KB_PRIVATE_PROJECTION_DIR"]
async fn private_runtime_repair_organize_fresh_check_cycle() {
    struct NoIo;
    #[async_trait]
    impl Journal for NoIo {
        async fn load(&self) -> Result<Option<Checkpoint>, AgentError> {
            panic!("offline")
        }
        async fn reserve(&self, _: &Checkpoint, _: &[u8]) -> Result<Option<usize>, AgentError> {
            panic!("offline")
        }
        async fn save(&self, _: &Checkpoint, _: &Value) -> Result<(), AgentError> {
            panic!("offline")
        }
    }
    async fn step(
        input: &FrozenInput,
        config: &Config,
        state: &mut Checkpoint,
        name: &str,
        args: Value,
        ok: bool,
    ) -> Value {
        assert!(state.turn < 80, "bounded offline fixture");
        let body = super::request(input, config, state).await.unwrap();
        let args =
            crate::outline::source_wire::fixture_arguments(input, state, name, args).unwrap();
        let args = state
            .outline_run
            .tool_draft
            .model_wire
            .fixture_encode(args)
            .unwrap();
        state
            .journal
            .prepare_session(
                &body,
                crate::agent_runtime::SESSION_PREFIX,
                crate::agent_runtime::ANALYSIS_SESSION_SUFFIX,
                config.limits.context_wire_ceiling(),
            )
            .unwrap();
        state.journal.prepare(state.turn, "main", &body).unwrap();
        let response:ChatTurn=serde_json::from_value(json!({"content":"","finish_reason":"tool_calls","usage":null,"tool_calls":[{"id":format!("offline-{}",state.turn),"name":name,"arguments":args.to_string()}]})).unwrap();
        state.journal.responded(response.clone()).unwrap();
        state.journal.validate(state.turn, "main").unwrap();
        let suppressed = state.journal.fixture_sdk_tools(&body, &response).unwrap();
        let results = super::execute_turn(
            input,
            config,
            state,
            &NoIo,
            response,
            suppressed,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        let output: Value = serde_json::from_str(results[0]["content"].as_str().unwrap()).unwrap();
        assert_eq!(
            output["ok"] == true && output["result"]["ok"] != false,
            ok,
            "{name}: {output}"
        );
        state.journal.fixture_sdk_finish(results).unwrap();
        state.journal.committed().unwrap();
        state.journal.validate(state.turn, "main").unwrap();
        let restored: Checkpoint = serde_json::from_value(json!(state)).unwrap();
        assert_eq!(json!(restored.outline_run), json!(state.outline_run));
        output["result"].clone()
    }
    async fn read_all(
        input: &FrozenInput,
        config: &Config,
        state: &mut Checkpoint,
        name: &str,
        mut args: Value,
    ) {
        loop {
            let page = step(input, config, state, name, args, true).await;
            let Some(cursor) = page["next_cursor"].as_str() else {
                assert_eq!(page["selection_complete"], true);
                break;
            };
            args = json!({"cursor":cursor});
        }
    }
    let root = std::path::PathBuf::from(std::env::var("KB_PRIVATE_PROJECTION_DIR").unwrap());
    let run = root.join("production-claim-cycle-v2");
    let read = |p: std::path::PathBuf| -> Value {
        serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
    };
    let input: FrozenInput =
        serde_json::from_value(read(run.join("frozen-scoped-input.json"))).unwrap();
    let mut state: Checkpoint = serde_json::from_value(read(
        root.join("runtime-transition-reopened-checkpoint.json"),
    ))
    .unwrap();
    state.journal.committed().unwrap();
    state.journal.session = None;
    let config = Config::with_provider_for(
        crate::authoring_runtime::AuthoringRuntimeContractV1::resolve_tools_from_environment()
            .unwrap(),
        serde_json::from_value(
            read(root.join("handles-full-live-limits.json"))["extraction"].clone(),
        )
        .unwrap(),
        Some(&input),
    )
    .unwrap();
    use crate::outline::{agent as outline, claim_review};
    // This archived object supplies only a stale read key. Its obsolete
    // semantic judgments are not migrated into the new protocol or accepted.
    let mut old_value = read(run.join("checkpoint-6.json"));
    if let Some(draft) = old_value["outline_run"].get_mut("tool_draft") {
        draft["claim_comparisons"] = json!({});
    }
    if let Some(draft) = old_value["outline_run"].get_mut("draft") {
        draft["claim_comparisons"] = json!({});
    }
    let old: Checkpoint = serde_json::from_value(old_value).unwrap();
    let oldref = old.outline_run.tool_draft.check_reads.evidence[0].clone();
    let oldkey = outline::review_evidence_key(&old, &oldref).unwrap();
    let initial_repairs = state.outline_run.repair_events.len();
    for synthetic_cycle in 0..2 {
        let work = state.outline_run.reading_packs.as_ref().unwrap();
        let original = work.requirement_records()["pack-1:0"].clone();
        let session = work.session(&input, "pack-1").unwrap();
        let support = session["pack"]["condition_support_options"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| json!({"support_key":s["support_key"]}))
            .collect::<Vec<_>>();
        assert!(!support.is_empty());
        let submit = json!({"pack_id":"pack-1","call_id":session["submission_operation_id"],"claim_token":session["pack"]["claim_token"],"pack_revision":session["pack"]["pack_revision"],"repair":false,"requirements":[{"description":"Synthetic fixture requires encrypted upload and excludes the optional paper artifact.","kind":"format","obligation_strength":"mandatory","extraction_quality":"explicit","source_section_id":original.source_section_id,"evidence":original.evidence,"condition_support":support}],"inspected_atom_ids":[],"no_requirement_reason":null});
        step(&input, &config, &mut state, "submit_pack", submit, true).await;
        assert_eq!(outline::current(&input, &state), outline::Duty::Organize);
        let records = state
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .requirement_records()
            .clone();
        let ids = records.keys().cloned().collect::<Vec<_>>();
        let refs = records
            .values()
            .flat_map(|r| {
                r.evidence
                    .iter()
                    .chain(r.condition_support.iter().flat_map(|s| s.evidence.iter()))
            })
            .cloned()
            .collect::<Vec<_>>();
        step(&input,&config,&mut state,"put_chapters",json!({"mode":"replace","chapters":[{"id":"response","parent_id":null,"order":0,"title":"递交要求响应","purpose":"response","requirement_ids":ids}]}),true).await;
        read_all(
            &input,
            &config,
            &mut state,
            "read_evidence",
            json!({"refs":refs}),
        )
        .await;
        step(&input,&config,&mut state,"put_slots",json!({"mode":"replace","slots":[{"slot_id":"response-body","chapter_id":"response","content":{"type":"generated_explanation","supporting_refs":refs},"text":"Synthetic fixture response: use the configured credential for encrypted upload before the fixture deadline; omit the optional paper artifact."}]}),true).await;
        let electronic = original.evidence[0].clone();
        let selected = records["pack-0:0"].evidence[2].clone();
        step(&input,&config,&mut state,"put_slots",json!({"mode":"upsert","slots":[{"slot_id":"electronic-source","chapter_id":"response","content":{"type":"source_copy","refs":[electronic]}},{"slot_id":"project-selection-source","chapter_id":"response","content":{"type":"source_copy","refs":[selected]}}]}),true).await;
        step(&input,&config,&mut state,"put_fulfillments",json!({"fulfillments":ids.iter().map(|id|json!({"requirement_id":id,"primary_response_chapter_id":"response","target_refs":[{"type":"text_slot","slot_id":"response-body"},{"type":"text_slot","slot_id":"electronic-source"},{"type":"text_slot","slot_id":"project-selection-source"}]})).collect::<Vec<_>>()}),true).await;
        assert_eq!(outline::current(&input, &state), outline::Duty::Check);
        assert!(state.outline_run.tool_draft.check_reads.evidence.is_empty());
        step(&input,&config,&mut state,"submit_review",json!({"requirement_ids":ids,"pack_ids":[],"inspected_evidence":[{"review_evidence_key":oldkey}],"issues":[]}),false).await;
        for slot in [
            "response-body",
            "electronic-source",
            "project-selection-source",
        ] {
            read_all(
                &input,
                &config,
                &mut state,
                "read_outline",
                json!({"mode":"slot_body","slot_id":slot}),
            )
            .await;
        }
        read_all(
            &input,
            &config,
            &mut state,
            "read_requirements",
            json!({"mode":"packs"}),
        )
        .await;
        let work = state.outline_run.reading_packs.as_ref().unwrap();
        let packids = work.pack_ids();
        let allrefs = packids
            .iter()
            .flat_map(|id| work.pack_evidence(&input, id).unwrap())
            .collect::<Vec<_>>();
        read_all(
            &input,
            &config,
            &mut state,
            "read_evidence",
            json!({"refs":allrefs}),
        )
        .await;
        if synthetic_cycle == 0 {
            let id = "pack-1:0";
            let unit = claim_review::build(&input, id, &records[id]).unwrap();
            read_all(
                &input,
                &config,
                &mut state,
                "read_claim_evidence",
                json!({"requirement_id":id}),
            )
            .await;
            let handles = unit
                .evidence_units
                .iter()
                .map(|u| u.quote_handle.clone())
                .collect::<Vec<_>>();
            step(&input,&config,&mut state,"submit_claim_comparison",json!({
            "requirement_id":id,"version":unit.version,
            "declared_claims":[{"claim_handle":"requirement","obligation":unit.original_requirement.description,
                "applicability":"synthetic protocol-only unresolved condition","original_fragments":[unit.original_requirement.description],
                "primary_handles":[handles[0]],"support_handles":&handles[1..]}],
            "observations":handles.iter().map(|h|json!({"quote_handle":h,"polarity":"mixed","applicability":"relevant","condition":"synthetic unresolved fixture; no semantic assessment"})).collect::<Vec<_>>(),
            "decisions":[{"claim_handle":"requirement","evidence_handles":handles,"verdict":"uncertain","action":"manual_review","resulting_claim":"Synthetic fixture requires a fresh repair and Check"}]
        }),true).await;
            assert_eq!(outline::current(&input, &state), outline::Duty::Discover);
            assert_eq!(state.outline_run.repair_events.len(), initial_repairs + 1);
            assert!(state.outline_run.tool_draft.chapters.is_empty());
            assert!(state.outline_run.tool_draft.check_reads.evidence.is_empty());
            continue;
        }
        for (id, record) in &records {
            let unit = claim_review::build(&input, id, record).unwrap();
            read_all(
                &input,
                &config,
                &mut state,
                "read_claim_evidence",
                json!({"requirement_id":id}),
            )
            .await;
            let handles = unit
                .evidence_units
                .iter()
                .map(|u| u.quote_handle.clone())
                .collect::<Vec<_>>();
            step(&input,&config,&mut state,"submit_claim_comparison",json!({"requirement_id":id,"version":unit.version,"declared_claims":[{"claim_handle":"requirement","obligation":unit.original_requirement.description,"applicability":"scripted protocol fixture scope","original_fragments":[unit.original_requirement.description],"primary_handles":[handles[0]],"support_handles":&handles[1..]}],"observations":handles.iter().map(|h|json!({"quote_handle":h,"polarity":"mixed","applicability":"relevant","condition":"deterministic protocol fixture, not model judgment"})).collect::<Vec<_>>(),"decisions":unit.claims.iter().map(|c|json!({"claim_handle":c.claim_handle,"evidence_handles":handles,"verdict":"supports","action":"retain","resulting_claim":c.original_text})).collect::<Vec<_>>()}),true).await;
        }
        let keys = state
            .outline_run
            .tool_draft
            .check_reads
            .evidence
            .iter()
            .map(|e| json!({"review_evidence_key":outline::review_evidence_key(&state,e).unwrap()}))
            .collect::<Vec<_>>();
        step(
            &input,
            &config,
            &mut state,
            "submit_review",
            json!({"requirement_ids":ids,"pack_ids":packids,"inspected_evidence":keys,"issues":[]}),
            true,
        )
        .await;
        step(
            &input,
            &config,
            &mut state,
            "finish_outline",
            json!({}),
            true,
        )
        .await;
        assert!(state.outline_run.tool_draft.finished);
    }
    assert_eq!(state.outline_run.repair_events.len(), initial_repairs + 1);
    std::fs::write(root.join("runtime-complete-cycle-result.json"),serde_json::to_vec_pretty(&json!({"status":"protocol_pass_only","no_provider_calls":true,"scripted_semantics":true,"runtime_execute_turn_every_step":true,"old_key_rejected":true,"substantive_response_body":true,"fresh_check_finished":true,"current_protocol_synthetic_check_reopen":true,"journal_valid_each_commit":true,"turn":state.turn})).unwrap()).unwrap();
    std::fs::write(
        root.join("runtime-complete-cycle-checkpoint.json"),
        serde_json::to_vec_pretty(&state).unwrap(),
    )
    .unwrap();
}

#[tokio::test]
#[ignore = "private runtime oversized metadata continuation; requires KB_PRIVATE_PROJECTION_DIR"]
async fn private_runtime_oversized_metadata_delivery() {
    struct NoIo;
    #[async_trait]
    impl Journal for NoIo {
        async fn load(&self) -> Result<Option<Checkpoint>, AgentError> {
            panic!("offline")
        }
        async fn reserve(&self, _: &Checkpoint, _: &[u8]) -> Result<Option<usize>, AgentError> {
            panic!("offline")
        }
        async fn save(&self, _: &Checkpoint, _: &Value) -> Result<(), AgentError> {
            panic!("offline")
        }
    }
    async fn step(
        input: &FrozenInput,
        config: &Config,
        state: &mut Checkpoint,
        name: &str,
        args: Value,
        ok: bool,
    ) -> Value {
        assert!(state.turn < 80, "bounded offline fixture");
        let body = super::request(input, config, state).await.unwrap();
        state
            .journal
            .prepare_session(
                &body,
                crate::agent_runtime::SESSION_PREFIX,
                crate::agent_runtime::ANALYSIS_SESSION_SUFFIX,
                config.limits.context_wire_ceiling(),
            )
            .unwrap();
        state.journal.prepare(state.turn, "main", &body).unwrap();
        let response:ChatTurn=serde_json::from_value(json!({"content":"","finish_reason":"tool_calls","usage":null,"tool_calls":[{"id":format!("offline-{}",state.turn),"name":name,"arguments":args.to_string()}]})).unwrap();
        state.journal.responded(response.clone()).unwrap();
        state.journal.validate(state.turn, "main").unwrap();
        let suppressed = state.journal.fixture_sdk_tools(&body, &response).unwrap();
        let audit_root =
            std::path::PathBuf::from(std::env::var("KB_PRIVATE_PROJECTION_DIR").unwrap());
        std::fs::write(audit_root.join(format!("metadata-wire-before-{}.json",state.turn)),serde_json::to_vec(&json!({"body":serde_json::from_slice::<Value>(&body).unwrap(),"frames":state.outline_run.tool_draft.read_frames})).unwrap()).unwrap();
        let results = super::execute_turn(
            input,
            config,
            state,
            &NoIo,
            response,
            suppressed,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        std::fs::write(audit_root.join(format!("metadata-state-after-{}.json",state.turn)),serde_json::to_vec(&json!({"records":state.outline_run.tool_draft.metadata_records,"frames":state.outline_run.tool_draft.read_frames})).unwrap()).unwrap();
        let output: Value = serde_json::from_str(results[0]["content"].as_str().unwrap()).unwrap();
        assert_eq!(
            output["ok"] == true && output["result"]["ok"] != false,
            ok,
            "{name}: {output}"
        );
        state.journal.fixture_sdk_finish(results).unwrap();
        state.journal.committed().unwrap();
        state.journal.validate(state.turn, "main").unwrap();
        let restored: Checkpoint = serde_json::from_value(json!(state)).unwrap();
        assert_eq!(json!(restored.outline_run), json!(state.outline_run));
        output["result"].clone()
    }

    let root = std::path::PathBuf::from(std::env::var("KB_PRIVATE_PROJECTION_DIR").unwrap());
    let run = root.join("production-claim-cycle-v2");
    let read = |p: std::path::PathBuf| -> Value {
        serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
    };
    let input: FrozenInput =
        serde_json::from_value(read(run.join("frozen-scoped-input.json"))).unwrap();
    let mut raw = read(run.join("seeded-checkpoint.json"));
    let oversized = "完整条款😀".repeat(40000);
    raw["outline_run"]["reading_packs"]["requirements"]["pack-0:0"]["description"] =
        json!(oversized);
    let mut state: Checkpoint = serde_json::from_value(raw).unwrap();
    state.transcript.clear();
    state.journal = Default::default();
    // A new synthetic Check fixture uses the current completed outline as data,
    // not as a resumable historical journal. No provider judgment is inferred.
    state.turn = 0;
    state.done = false;
    state.draft_stage = crate::analysis::draft::DraftStage::Outline;
    state.outline_run.tool_draft.finished = false;

    let config = Config::with_provider_for(
        crate::authoring_runtime::AuthoringRuntimeContractV1::resolve_tools_from_environment()
            .unwrap(),
        serde_json::from_value(
            read(root.join("handles-full-live-limits.json"))["extraction"].clone(),
        )
        .unwrap(),
        Some(&input),
    )
    .unwrap();
    assert_eq!(
        crate::outline::agent::current(&input, &state),
        crate::outline::agent::Duty::Check
    );
    let canonical_before = serde_json::to_value(
        state
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .requirement_records(),
    )
    .unwrap();
    let probe: Value =
        serde_json::from_slice(&super::request(&input, &config, &mut state).await.unwrap())
            .unwrap();
    let host = probe["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| {
            m["content"]
                .as_str()
                .and_then(|s| serde_json::from_str::<Value>(s).ok())
        })
        .find(|v| v["requirements"]["preview_only"] == true)
        .unwrap();
    assert!(host["requirements"]["items"].as_array().unwrap().is_empty());
    assert_eq!(
        host["requirements"]["total"].as_u64().unwrap() as usize,
        canonical_before.as_object().unwrap().len()
    );
    assert_eq!(
        host["requirements"]["omitted"],
        host["requirements"]["total"]
    );
    assert_eq!(
        host["requirements"]["navigation"]["tool"],
        "read_requirements"
    );
    assert_eq!(
        canonical_before,
        serde_json::to_value(
            state
                .outline_run
                .reading_packs
                .as_ref()
                .unwrap()
                .requirement_records()
        )
        .unwrap()
    );
    assert!(
        super::estimate_input_tokens(&probe, &config.limits).unwrap()
            + config.provider.output_token_reserve as usize
            <= config.limits.max_context_tokens
    );
    let mut args = json!({});
    let mut pages = 0;
    let mut record_key = None;
    loop {
        let page = step(&input, &config, &mut state, "read_requirements", args, true).await;
        pages += 1;
        assert!(pages < 16);
        if let Some(key) = page["items"][0]["record_key"].as_str() {
            record_key = Some(key.to_owned());
        }
        if pages == 1 {
            assert!(
                state.transcript.iter().any(|m| m["role"] == "tool"),
                "entering Check must retain undelivered first page"
            );
            let key = record_key
                .as_ref()
                .expect("single oversized record must fragment");
            assert!(
                !state.outline_run.tool_draft.metadata_records[key].complete(),
                "issuance is not delivery"
            );
        }
        let Some(cursor) = page["next_cursor"].as_str() else {
            assert_eq!(page["selection_complete"], true);
            break;
        };
        args = json!({"cursor":cursor});
    }
    // The final read result only becomes delivered in the following completed request.
    step(
        &input,
        &config,
        &mut state,
        "read_requirements",
        json!({"mode":"packs"}),
        true,
    )
    .await;
    let record = &state.outline_run.tool_draft.metadata_records[record_key.as_ref().unwrap()];
    std::fs::write(root.join("metadata-runtime-debug.json"),serde_json::to_vec_pretty(&json!({"pages":pages,"record":record,"frames":state.outline_run.tool_draft.read_frames})).unwrap()).unwrap();
    assert_eq!(
        record.restored().unwrap()["requirement"]["description"],
        oversized
    );
    assert!(pages > 1);
    std::fs::write(root.join("metadata-runtime-result.json"),serde_json::to_vec_pretty(&json!({"pages":pages,"complete_exact_reconstruction":true,"provider_calls":0,"formal_runtime":true,"empty_preview_preserves_total_and_navigation":true,"canonical_records_unchanged":true})).unwrap()).unwrap();
}

#[test]
#[ignore = "audits saved synthetic oversized runtime pages; requires KB_PRIVATE_PROJECTION_DIR; no model calls"]
fn private_oversized_canonical_request_and_hash_audit() {
    use sha2::{Digest, Sha256};
    let root = std::path::PathBuf::from(std::env::var("KB_PRIVATE_PROJECTION_DIR").unwrap());
    let read = |path| -> Value { serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap() };
    let limits: Limits = serde_json::from_value(
        read(root.join("handles-full-live-limits.json"))["extraction"].clone(),
    )
    .unwrap();
    let mut requests = Vec::new();
    for entry in std::fs::read_dir(&root).unwrap() {
        let path = entry.unwrap().path();
        if !path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("metadata-wire-before-")
        {
            continue;
        }
        let data = read(path);
        let body = &data["body"];
        let output = body["max_tokens"]
            .as_u64()
            .or_else(|| body["max_completion_tokens"].as_u64())
            .expect("actual output reservation") as usize;
        let input = super::estimate_input_tokens(body, &limits).unwrap();
        assert!(input + output <= limits.max_context_tokens);
        requests.push(json!({"input_tokens":input,"output_reserve":output,"total":input+output,"context":limits.max_context_tokens}));
    }
    assert!(requests.len() > 1);
    let debug = read(root.join("metadata-runtime-debug.json"));
    let record: crate::outline::metadata_fragments::Record =
        serde_json::from_value(debug["record"].clone()).unwrap();
    let restored = record.restored().unwrap();
    assert_eq!(restored, record.original);
    let text = restored["requirement"]["description"].as_str().unwrap();
    let actual = hex::encode(Sha256::digest(text.as_bytes()));
    let expected = hex::encode(Sha256::digest("完整条款😀".repeat(40000).as_bytes()));
    assert_eq!(actual, expected);
    std::fs::write(root.join("metadata-request-hash-audit.json"),serde_json::to_vec_pretty(&json!({"all_actual_requests_fit":true,"requests":requests,"record_complete_after_delivery":record.complete(),"expected_text_sha256":expected,"restored_text_sha256":actual,"bytes":text.len(),"provider_calls":0})).unwrap()).unwrap();
}

#[tokio::test]
async fn host_navigation_continuation_reaches_real_handler_and_rejects_filter_tampering() {
    use crate::outline::agent as outline;
    let (input, state, _, _) = crate::outline::fixture_discovered(false);
    let id = state
        .outline_run
        .reading_packs
        .as_ref()
        .unwrap()
        .requirement_records()
        .keys()
        .next()
        .unwrap()
        .clone();
    let mut raw = serde_json::to_value(state).unwrap();
    raw["outline_run"]["reading_packs"]["requirements"][&id]["description"] =
        json!("合成要求".repeat(10000));
    let mut state: Checkpoint = serde_json::from_value(raw).unwrap();
    let mut config = crate::analysis::tests::config();
    config.limits.max_context_tokens = 32768;
    let navigation = outline::requirements_navigation();
    let first = navigation["first_arguments"].clone();
    let call = |args: &Value| knowledge::models::ChatToolCall {
        id: "navigation".into(),
        name: "read_requirements".into(),
        arguments: args.to_string(),
    };
    state.transcript = vec![
        json!({"role":"assistant","tool_calls":[{"id":"navigation","type":"function","function":{"name":"read_requirements","arguments":first.to_string()}}]}),
    ];
    let page = super::super::read_projection_in_context(
        &input,
        &config,
        &mut state,
        &first,
        &[call(&first)],
        &[],
        outline::Duty::Organize,
    )
    .await
    .unwrap();
    let cursor = page["next_cursor"]
        .as_str()
        .expect("oversized selection must produce a real host cursor");
    assert!(navigation.get("continuation_arguments").is_none());
    let mut wire = json!({"messages":[
        {"role":"user","content":json!({"requirements_navigation":navigation}).to_string()},
        {"role":"tool","tool_call_id":"navigation","content":page.to_string()},
        {"role":"user","content":"{}"}],"tools":outline::schemas_for(outline::Duty::Organize)});
    let registry = crate::outline::model_wire::project(&state, &mut wire).unwrap();
    assert!(!wire.to_string().contains("<read_requirements.next_cursor>"));
    let wire_page: Value =
        serde_json::from_str(wire["messages"][1]["content"].as_str().unwrap()).unwrap();
    let host: Value =
        serde_json::from_str(wire["messages"][2]["content"].as_str().unwrap()).unwrap();
    let wire_args = json!({"cursor":wire_page["next_cursor"],"wire_scope":host["wire_scope"]});
    let spec = wire["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["function"]["name"] == "read_requirements")
        .unwrap();
    let schema = jsonschema::JSONSchema::compile(&spec["function"]["parameters"]).unwrap();
    assert!(schema.is_valid(&wire_args));
    assert!(!schema.is_valid(&json!({"cursor":wire_page["next_cursor"]})));
    let next = registry.decode(wire_args, false).unwrap();
    assert_eq!(next, json!({"cursor":cursor}));
    state.outline_run.tool_draft.model_wire = registry;
    assert_eq!(next.as_object().unwrap().len(), 1);
    state = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
    let before = serde_json::to_value(&state).unwrap();
    for (key, value) in [
        ("mode", json!("requirements")),
        ("source_section_id", json!("changed-filter")),
    ] {
        let mut tampered = next.clone();
        tampered[key] = value;
        let error = super::super::read_projection_in_context(
            &input,
            &config,
            &mut state,
            &tampered,
            &[call(&tampered)],
            &[],
            outline::Duty::Organize,
        )
        .await
        .unwrap_err();
        assert_eq!(error, "continuation accepts only the host cursor");
        assert_eq!(serde_json::to_value(&state).unwrap(), before);
    }
    state.transcript = vec![
        json!({"role":"assistant","tool_calls":[{"id":"navigation","type":"function","function":{"name":"read_requirements","arguments":next.to_string()}}]}),
    ];
    let continued = super::super::read_projection_in_context(
        &input,
        &config,
        &mut state,
        &next,
        &[call(&next)],
        &[],
        outline::Duty::Organize,
    )
    .await
    .unwrap();
    assert!(
        !state
            .outline_run
            .tool_draft
            .evidence_continuations
            .contains_key(cursor)
    );
    assert!(
        continued["items"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );
}

#[test]
fn claiming_or_failing_without_committing_is_not_discovery_progress() {
    let input = pack_input(1, "Submit a declaration.");
    let mut state = outline_checkpoint();
    state.outline_run.reading_packs =
        Some(crate::outline::discover::DiscoverWork::plan(&input, 8192));
    let before = one_shot_progress_marker(&state).unwrap();
    state.outline_run.reading_packs.as_mut().unwrap().claim(1);
    assert_eq!(one_shot_progress_marker(&state).unwrap(), before);
    observe_progress(
        &mut state,
        &Role::Main,
        None,
        &crate::analysis::tests::config().limits,
    )
    .unwrap();
    observe_progress(
        &mut state,
        &Role::Main,
        None,
        &crate::analysis::tests::config().limits,
    )
    .unwrap();
    assert!(state.main_progress.watch.no_progress_turns > 0);
}

#[test]
fn discovery_summary_reports_canonical_count_separately_from_draft() {
    let (input, mut state, _, _) = crate::outline::fixture_discovered(false);
    let work = state.outline_run.reading_packs.as_mut().unwrap();
    let id = work.pack_ids().into_iter().next().unwrap();
    work.reopen_committed(&input, &id, 1, "reopen-count-fixture")
        .unwrap();
    state.outline_run.tool_draft = Default::default();
    state.analysis.outline.phase = crate::analysis::outline_flow::Phase::Discover;
    let result = crate::outline::agent::apply_for_duty(
        &input,
        &mut state,
        "read_outline",
        &json!({"mode":"summary"}),
        crate::outline::agent::Duty::Discover,
    )
    .unwrap();
    assert_eq!(result["discovery_requirement_count"], 1);
    assert_eq!(result["requirement_count"], 1);
    assert_eq!(result["draft_requirement_count"], 0);
    assert_eq!(result["requirement_count_scope"], "canonical_discovery");
}

#[tokio::test]
async fn fresh_wire_claim_read_allows_multiple_model_owned_declarations() {
    struct NoIo;
    #[async_trait]
    impl Journal for NoIo {
        async fn load(&self) -> Result<Option<Checkpoint>, AgentError> {
            panic!("offline")
        }
        async fn reserve(&self, _: &Checkpoint, _: &[u8]) -> Result<Option<usize>, AgentError> {
            panic!("offline")
        }
        async fn save(&self, _: &Checkpoint, _: &Value) -> Result<(), AgentError> {
            panic!("offline")
        }
    }
    async fn step(
        input: &FrozenInput,
        config: &Config,
        state: &mut Checkpoint,
        name: &str,
        args: Value,
    ) -> Value {
        let body = super::request(input, config, state).await.unwrap();
        let args =
            crate::outline::source_wire::fixture_arguments(input, state, name, args).unwrap();
        let args = state
            .outline_run
            .tool_draft
            .model_wire
            .fixture_encode(args)
            .unwrap();
        state
            .journal
            .prepare_session(
                &body,
                crate::agent_runtime::SESSION_PREFIX,
                crate::agent_runtime::ANALYSIS_SESSION_SUFFIX,
                config.limits.context_wire_ceiling(),
            )
            .unwrap();
        state.journal.prepare(state.turn, "main", &body).unwrap();
        crate::outline::read_receipts::confirm(state, &serde_json::from_slice(&body).unwrap())
            .unwrap();
        let response:ChatTurn=serde_json::from_value(json!({"content":"","finish_reason":"tool_calls","usage":null,"tool_calls":[{"id":format!("claims-{}",state.turn),"name":name,"arguments":args.to_string()}]})).unwrap();
        state.journal.responded(response.clone()).unwrap();
        let suppressed = state.journal.fixture_sdk_tools(&body, &response).unwrap();
        let results = super::execute_turn(
            input,
            config,
            state,
            &NoIo,
            response,
            suppressed,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        let out: Value = serde_json::from_str(results[0]["content"].as_str().unwrap()).unwrap();
        assert_eq!(out["ok"], true, "{out}");
        state.journal.fixture_sdk_finish(results).unwrap();
        state.journal.committed().unwrap();
        let restored: Checkpoint = serde_json::from_value(json!(&state)).unwrap();
        *state = restored;
        out["result"].clone()
    }
    let (input, mut state, _, _) = crate::outline::fixture_organized(false);
    let config = crate::analysis::tests::config();
    let id = state
        .outline_run
        .reading_packs
        .as_ref()
        .unwrap()
        .requirement_ids()
        .into_iter()
        .next()
        .unwrap();
    let record = state
        .outline_run
        .reading_packs
        .as_ref()
        .unwrap()
        .requirement_records()[&id]
        .clone();
    let unit = crate::outline::claim_review::build(&input, &id, &record).unwrap();
    let before_read = one_shot_progress_marker(&state).unwrap();
    let mut args = json!({"requirement_id":id});
    loop {
        let page = step(&input, &config, &mut state, "read_claim_evidence", args).await;
        if let Some(cursor) = page["next_cursor"].as_str() {
            args = json!({"cursor":cursor});
        } else {
            break;
        }
    }
    assert_eq!(one_shot_progress_marker(&state).unwrap(), before_read);
    let quotes = unit
        .evidence_units
        .iter()
        .map(|item| item.quote_handle.clone())
        .collect::<Vec<_>>();
    let fragments = [
        ("materials", "提供资格证明"),
        ("no-subcontract", "并遵守禁止转包条款"),
    ];
    let comparison = json!({"requirement_id":id,"version":unit.version,
        "declared_claims":fragments.iter().map(|(key,text)|json!({"claim_handle":key,"obligation":text,"applicability":"fixture","original_fragments":[text],"primary_handles":[quotes[0]],"support_handles":&quotes[1..]})).collect::<Vec<_>>(),
        "observations":quotes.iter().map(|key|json!({"quote_handle":key,"polarity":"required","applicability":"relevant","condition":"fixture"})).collect::<Vec<_>>(),
        "decisions":fragments.iter().map(|(key,text)|json!({"claim_handle":key,"evidence_handles":quotes,"verdict":"supports","action":"retain","resulting_claim":text})).collect::<Vec<_>>()});
    step(
        &input,
        &config,
        &mut state,
        "submit_claim_comparison",
        comparison,
    )
    .await;
    assert_ne!(one_shot_progress_marker(&state).unwrap(), before_read);
    assert_eq!(
        state.outline_run.tool_draft.claim_comparisons[&id]
            .declared_claims
            .len(),
        2
    );
}

#[tokio::test]
async fn readonly_wire_scopes_do_not_complete_organize_or_check_work() {
    for check in [false, true] {
        let (input, mut state, _, _) = if check {
            crate::outline::fixture_organized(false)
        } else {
            crate::outline::fixture_discovered(false)
        };
        let config = crate::analysis::tests::config();
        let before = one_shot_progress_marker(&state).unwrap();
        for _ in 0..3 {
            super::request(&input, &config, &mut state).await.unwrap();
            crate::outline::agent::full_read_projection(
                &input,
                &state,
                "read_outline",
                &json!({"mode":"summary"}),
            )
            .unwrap();
            state.turn += 1;
            state.outline_run.tool_draft.read_epoch += 1;
            assert_eq!(one_shot_progress_marker(&state).unwrap(), before);
            observe_progress(&mut state, &Role::Main, None, &config.limits).unwrap();
        }
        assert!(state.main_progress.watch.no_progress_turns >= 2);
        if check {
            state
                .outline_run
                .tool_draft
                .reviewed_requirement_ids
                .insert("fixture-review".into());
        } else {
            crate::outline::agent::apply(&input,&mut state,"put_chapters",&json!({"mode":"replace","chapters":[{"id":"response","parent_id":null,"order":0,"title":"Response","purpose":"response","requirement_ids":[]}]})).unwrap();
        }
        assert_ne!(one_shot_progress_marker(&state).unwrap(), before);
    }
}

#[tokio::test]
async fn check_reload_retains_unextracted_source_scope_without_requirements() {
    use crate::outline::agent as outline;
    let (input, mut state, pack, refs) = crate::outline::fixture_organized(true);
    assert!(
        state
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .requirement_records()
            .is_empty()
    );
    state.transcript.clear();
    state = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
    assert_eq!(outline::current(&input, &state), outline::Duty::Check);
    let config = crate::analysis::tests::config();
    let bytes = super::super::prepare_request(&input, &config, &mut state, false)
        .await
        .unwrap();
    assert!(
        crate::analysis::tests::total_request_tokens(&bytes, &config)
            <= config.limits.max_context_tokens
    );
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    let host: Value = serde_json::from_str(
        body["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(host["check_work"]["remaining_comparisons"], 0);
    assert_eq!(host["review"]["remaining_packs"], 1);
    let args = json!({"mode":"packs"});
    let call = knowledge::models::ChatToolCall {
        id: "unclaimed-read".into(),
        name: "read_requirements".into(),
        arguments: args.to_string(),
    };
    state.transcript = vec![
        json!({"role":"assistant","tool_calls":[{"id":call.id,"type":"function","function":{"name":call.name,"arguments":call.arguments}}]}),
    ];
    let page = super::super::read_projection_in_context(
        &input,
        &config,
        &mut state,
        &args,
        &[call],
        &[],
        outline::Duty::Check,
    )
    .await
    .unwrap();
    assert_eq!(page["selection_complete"], true);
    let evidence = page["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|row| row.get("evidence"))
        .cloned()
        .collect::<Vec<_>>();
    assert!(!evidence.is_empty());
    for reference in &refs {
        assert!(
            evidence.contains(&json!(reference)),
            "unextracted source must remain in independent Check"
        );
    }
    assert!(
        !state
            .outline_run
            .tool_draft
            .reviewed_pack_ids
            .contains(&pack)
    );
    assert!(state.outline_run.tool_draft.check_reads.evidence.is_empty());
    assert!(!state.done);
}

#[tokio::test]
async fn oversized_unconsumed_unicode_read_probe_rejects_without_mutation() {
    let (input, mut state, _, _) = crate::outline::fixture_discovered(false);
    let config = crate::analysis::tests::config();
    let original_text = "合成条款🙂e\u{301}𠀀".repeat(20000);
    state.transcript = vec![
        json!({"role":"assistant","tool_calls":[{"id":"unconsumed","type":"function","function":{"name":"read_requirements","arguments":"{\"mode\":\"requirements\"}"}}]}),
        json!({"role":"tool","tool_call_id":"unconsumed","content":json!({"ok":true,"result":{"items":[{"description":original_text}],"selection_complete":false,"next_cursor":"host-preserved"}}).to_string()}),
    ];
    let before = serde_json::to_value(&state).unwrap();
    let error = super::super::prepare_request(&input, &config, &mut state, false)
        .await
        .unwrap_err();
    assert_eq!(error.code, "AGENT_TURN_BUDGET_EXCEEDED");
    assert!(error.message.contains("unconsumed read group"));
    assert_eq!(
        serde_json::to_value(&state).unwrap(),
        before,
        "rejected probe preserves complete Unicode payload, history and receipts"
    );
    assert!(state.outline_run.tool_draft.check_reads.evidence.is_empty());
}

#[test]
fn delivered_check_pages_advance_reading_without_completing_semantic_work() {
    let (_, mut state, _, _) = crate::outline::fixture_organized(false);
    let limits = crate::analysis::tests::config().limits;
    observe_progress(&mut state, &Role::Main, None, &limits).unwrap();
    let completed = state.main_progress.completions.clone();
    for (page, idle) in [("pack-a:atom", 0), ("pack-b:atom", 0), ("pack-a:atom", 1)] {
        state
            .outline_run
            .tool_draft
            .check_reads
            .structure_keys
            .insert(page.into());
        observe_progress(&mut state, &Role::Main, None, &limits).unwrap();
        assert_eq!(state.main_progress.watch.no_progress_turns, idle);
        assert_eq!(state.main_progress.completions, completed);
    }
    for page in 0..limits.max_no_progress_turns + 2 {
        state
            .outline_run
            .tool_draft
            .check_reads
            .structure_keys
            .insert(format!("source-page-{page}"));
        observe_progress(&mut state, &Role::Main, None, &limits).unwrap();
        assert_eq!(state.main_progress.watch.no_progress_turns, 0);
        assert_eq!(state.main_progress.watch.replans, 0);
        assert_eq!(state.main_progress.completions, completed);
        state = serde_json::from_value(json!(&state)).unwrap();
    }
    let reads = state
        .outline_run
        .tool_draft
        .check_reads
        .structure_keys
        .clone();
    for _ in 0..limits.max_no_progress_turns {
        state
            .outline_run
            .tool_draft
            .check_reads
            .structure_keys
            .extend(reads.clone());
        state.outline_run.tool_draft.read_epoch += 1;
        observe_progress(&mut state, &Role::Main, None, &limits).unwrap();
    }
    assert_eq!(state.main_progress.watch.replans, 1);
    assert_eq!(state.main_progress.completions, completed);
    assert!(state.outline_run.tool_draft.claim_comparisons.is_empty());
    assert!(
        state
            .outline_run
            .tool_draft
            .reviewed_requirement_ids
            .is_empty()
    );
}

#[test]
fn check_read_progress_requires_delivered_new_ranges_not_overlap_or_pending() {
    use crate::outline::evidence::EvidenceRef;
    let (_, mut state, _, _) = crate::outline::fixture_organized(false);
    let limits = crate::analysis::tests::config().limits;
    let reference = |start_byte, end_byte| EvidenceRef::Text {
        input_digest: "synthetic-input".into(),
        unit_id: "unicode-source".into(),
        start_byte,
        end_byte,
    };
    observe_progress(&mut state, &Role::Main, None, &limits).unwrap();
    let completed = state.main_progress.completions.clone();
    state
        .outline_run
        .tool_draft
        .check_reads
        .pending_evidence
        .push((0, reference(0, 6)));
    observe_progress(&mut state, &Role::Main, None, &limits).unwrap();
    assert_eq!(state.main_progress.watch.no_progress_turns, 1);
    state
        .outline_run
        .tool_draft
        .check_reads
        .evidence
        .push(reference(0, 6));
    observe_progress(&mut state, &Role::Main, None, &limits).unwrap();
    assert_eq!(state.main_progress.watch.no_progress_turns, 0);
    state
        .outline_run
        .tool_draft
        .check_reads
        .evidence
        .extend([reference(3, 6), reference(0, 3)]);
    observe_progress(&mut state, &Role::Main, None, &limits).unwrap();
    assert_eq!(state.main_progress.watch.no_progress_turns, 1);
    state
        .outline_run
        .tool_draft
        .check_reads
        .evidence
        .push(reference(6, 9));
    observe_progress(&mut state, &Role::Main, None, &limits).unwrap();
    assert_eq!(state.main_progress.watch.no_progress_turns, 0);
    state
        .outline_run
        .tool_draft
        .check_reads
        .slot_ranges
        .insert("response".into(), vec![(0, 6)]);
    observe_progress(&mut state, &Role::Main, None, &limits).unwrap();
    assert_eq!(state.main_progress.watch.no_progress_turns, 0);
    state
        .outline_run
        .tool_draft
        .check_reads
        .slot_ranges
        .get_mut("response")
        .unwrap()
        .extend([(3, 6), (0, 3)]);
    observe_progress(&mut state, &Role::Main, None, &limits).unwrap();
    assert_eq!(state.main_progress.watch.no_progress_turns, 1);
    assert_eq!(state.main_progress.completions, completed);
}

#[test]
fn completed_wire_read_progress_rejects_stale_and_tampered_frames() {
    use crate::outline::read_receipts;
    let (_, seed, pack, _) = crate::outline::fixture_organized(false);
    let limits = crate::analysis::tests::config().limits;
    let body =
        json!({"messages":[{"role":"tool","tool_call_id":"read-A","content":"exact page A"}]});
    for failure in ["epoch", "revision", "wire", "none"] {
        let mut state = seed.clone();
        observe_progress(&mut state, &Role::Main, None, &limits).unwrap();
        let before = state.outline_run.tool_draft.clone();
        let key = format!("{pack}:source-A");
        state
            .outline_run
            .tool_draft
            .check_reads
            .pending_structure_keys
            .push((0, key.clone()));
        read_receipts::queue(&mut state, &before, "read-A", true);
        read_receipts::seal(&mut state, &body).unwrap();
        state = serde_json::from_value(json!(&state)).unwrap();
        let mut delivered = body.clone();
        match failure {
            "epoch" => state.outline_run.tool_draft.read_epoch += 1,
            "revision" => state.outline_run.reading_packs.as_mut().unwrap().revision += 1,
            "wire" => delivered["messages"][0]["content"] = json!("different page B"),
            _ => (),
        }
        read_receipts::confirm(&mut state, &delivered).unwrap();
        observe_progress(&mut state, &Role::Main, None, &limits).unwrap();
        assert_eq!(
            state
                .outline_run
                .tool_draft
                .check_reads
                .structure_keys
                .contains(&key),
            failure == "none"
        );
        assert!(
            !state
                .outline_run
                .tool_draft
                .check_reads
                .structure_keys
                .contains("other-pack:source-A")
        );
        assert_eq!(
            state.main_progress.watch.no_progress_turns,
            usize::from(failure != "none")
        );
        read_receipts::confirm(&mut state, &delivered).unwrap();
        observe_progress(&mut state, &Role::Main, None, &limits).unwrap();
        assert!(state.main_progress.watch.no_progress_turns >= 1);
        assert!(state.outline_run.tool_draft.reviewed_pack_ids.is_empty());
    }
}

#[test]
fn organize_new_delivered_evidence_is_reading_not_completion() {
    let (_, mut state, _, refs) = crate::outline::fixture_discovered(false);
    let limits = crate::analysis::tests::config().limits;
    observe_progress(&mut state, &Role::Main, None, &limits).unwrap();
    let completed = state.main_progress.completions.clone();
    state
        .outline_run
        .tool_draft
        .delivered_evidence
        .push(refs[0].clone());
    observe_progress(&mut state, &Role::Main, None, &limits).unwrap();
    assert_eq!(state.main_progress.watch.no_progress_turns, 0);
    assert_eq!(state.main_progress.completions, completed);
    observe_progress(&mut state, &Role::Main, None, &limits).unwrap();
    assert_eq!(state.main_progress.watch.no_progress_turns, 1);
}

#[test]
fn check_task_retains_target_body_until_read_and_reviewed_after_reload() {
    let (input, mut state, _, _) = crate::outline::fixture_organized(false);
    let host = |state: &Checkpoint| {
        crate::outline::agent::host_packet(&input, state, 0, json!({}), json!({}), None)
    };
    let first = host(&state);
    let id = first["check_work"]["requirement_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let slot = first["check_work"]["next_unread_slot"]["slot_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        first["check_work"]["next_unread_slot"]["args"]["slot_id"],
        slot
    );
    state.outline_run.tool_draft.claim_comparisons.insert(
        id.clone(),
        crate::outline::claim_review::Comparison {
            requirement_id: id.clone(),
            version: "synthetic-comparison".into(),
            declared_claims: vec![],
            observations: vec![],
            decisions: vec![],
        },
    );
    state.transcript.clear();
    state = serde_json::from_value(json!(&state)).unwrap();
    let after = host(&state);
    assert_eq!(after["check_work"]["requirement_id"], id);
    assert_eq!(after["check_work"]["stage"], "review_sources");
    assert_eq!(after["check_work"]["next_unread_slot"]["slot_id"], slot);
    let target = state
        .outline_run
        .tool_draft
        .slots
        .iter()
        .find(|s| s.slot_id == slot)
        .unwrap();
    let bytes = target.text.len();
    state
        .outline_run
        .tool_draft
        .check_reads
        .slot_ranges
        .insert(slot.clone(), vec![(0, bytes)]);
    let after = host(&state);
    assert_ne!(after["check_work"]["next_unread_slot"]["slot_id"], slot);
    assert!(
        state
            .outline_run
            .tool_draft
            .reviewed_requirement_ids
            .is_empty()
    );
    assert!(!state.done);
}

#[test]
fn serial_check_selects_unreviewed_source_after_reload_without_history() {
    let (input, mut state, pack_id, refs) = crate::outline::fixture_organized(true);
    state.transcript.clear();
    state = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
    let packet = crate::outline::agent::host_packet(&input, &state, 0, json!({}), json!({}), None);
    assert_eq!(packet["check_work"]["source_scope"]["pack_id"], pack_id);
    assert_eq!(
        packet["check_work"]["source_scope"]["next_evidence"],
        json!(refs[0])
    );
    // Delivery alone does not discharge source review, including empty extraction.
    state.outline_run.tool_draft.check_reads.evidence = refs.clone();
    let after_read =
        crate::outline::agent::host_packet(&input, &state, 0, json!({}), json!({}), None);
    assert_eq!(
        packet["check_work"]["source_scope"],
        after_read["check_work"]["source_scope"]
    );
    // A persisted bounded review advances the scope even with no transcript.
    let draft = &mut state.outline_run.tool_draft;
    let disposition = crate::outline::source_review::Disposition {
        pack_id: pack_id.clone(),
        version: crate::outline::source_review::version(
            &input,
            draft,
            &pack_id,
            &draft.source_scopes[&pack_id],
        )
        .unwrap(),
        evidence: vec![refs[0].clone()],
        verdict: crate::outline::source_review::Verdict::NoResponseObligation,
        requirement_ids: Default::default(),
        reason: "Synthetic bounded review".into(),
    };
    draft.source_dispositions.insert(
        crate::outline::source_review::key(&disposition).unwrap(),
        disposition,
    );
    state = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
    let after_review =
        crate::outline::agent::host_packet(&input, &state, 0, json!({}), json!({}), None);
    assert_ne!(
        packet["check_work"]["source_scope"]["next_evidence"],
        after_review["check_work"]["source_scope"]["next_evidence"]
    );
    assert!(!state.done);
}
