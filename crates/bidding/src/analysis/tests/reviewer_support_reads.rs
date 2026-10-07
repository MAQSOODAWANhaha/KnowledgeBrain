use super::super::agent::context;
use super::*;

async fn fixture() -> (FrozenInput, MemoryJournal, Checkpoint) {
    let journal = fresh_review_journal().await;
    let mut state = journal.load().await.unwrap().unwrap();
    let mut input = input();
    input.source_units.push(Source {
        source_unit_revision_id: "support".into(),
        document_id: "support-document".into(),
        ordinal: 1,
        text: "Cross-reference original: retain this exact evidence.".into(),
        locator: json!({}),
    });
    input.structured_forms.push(json!({
        "form_definition_revision_id":"support-grid","source_unit_revision_id":"support",
        "definition":{"schema_version":3,"kind":"grid","row_count":1,"column_count":1,
            "cells":[{"row":0,"column":0,"row_span":1,"col_span":1,"text":"Exact supporting cell"}]}
    }));
    state.input_sha256 = digest(&input).unwrap();
    state.journal = Default::default();
    state.turn = 0;
    state.tool_calls = 0;
    state.read_bytes = 0;
    state.review_rounds = 0;
    state.pending_coverage = Some(state.reviewer_coverage.clone());
    state.source_review = Some(source_review::initialize(&input, &config()).unwrap());
    source_review::select_next(&input, &config(), &mut state).unwrap();
    *journal.state.lock().unwrap() = Some(state.clone());
    journal.reservations.lock().unwrap().clear();
    let mut view = test_view();
    view.identity.source_id = "support".into();
    *journal.view.lock().unwrap() = Some(view);
    (input, journal, state)
}

fn reads() -> Vec<ChatToolCall> {
    [
        (
            "text",
            "read_source",
            json!({"source_id":"support","start":0,"max_bytes":1024}),
        ),
        (
            "grid",
            "read_form",
            json!({"form_id":"support-grid","offset":0,"limit":1}),
        ),
        ("pixels", "read_source_view", json!({"source_id":"support"})),
    ]
    .into_iter()
    .map(|(id, name, args)| ChatToolCall {
        id: id.into(),
        name: name.into(),
        arguments: args.to_string(),
    })
    .collect()
}

struct ReadSupport;
#[async_trait]
impl Model for ReadSupport {
    async fn turn(&self, _: &Config, _: &[u8]) -> Result<ChatTurn, AgentError> {
        Ok(ChatTurn {
            tool_calls: reads(),
            finish_reason: "tool_calls".into(),
            ..Default::default()
        })
    }
}

fn output(state: &Checkpoint, id: &str) -> Value {
    state
        .transcript
        .iter()
        .find(|m| m["role"] == "tool" && m["tool_call_id"] == id)
        .map(|m| serde_json::from_str(m["content"].as_str().unwrap()).unwrap())
        .unwrap()
}

#[tokio::test]
async fn reviewer_known_support_reads_do_not_expand_work_or_main_permissions() {
    let (input, _, state) = fixture().await;
    let before = json!(state);
    for call in reads() {
        let args: Value = serde_json::from_str(&call.arguments).unwrap();
        context::check_read_scope(&input, &state, &call.name, &args).unwrap();
        let mut main = state.clone();
        main.role = Role::Main;
        main.main_work = main.reviewer_work.clone();
        assert!(context::check_read_scope(&input, &main, &call.name, &args).is_err());
    }
    for (name, args) in [
        ("read_source", json!({"source_id":"unknown"})),
        ("read_source_view", json!({"source_id":"unknown"})),
        ("read_form", json!({"form_id":"unknown"})),
    ] {
        assert!(context::check_read_scope(&input, &state, name, &args).is_err());
    }
    assert_eq!(json!(state), before);
    let mut idle = state.clone();
    idle.reviewer_work = None;
    assert!(
        context::check_read_scope(
            &input,
            &idle,
            "read_source",
            &json!({"source_id":"support"})
        )
        .is_err()
    );
}

async fn pending_support() -> (FrozenInput, MemoryJournal, Checkpoint, Checkpoint) {
    let (input, journal, before) = fixture().await;
    *journal.interrupt_after.lock().unwrap() = Some(1);
    let error = agent::run(
        &input,
        &config(),
        &journal,
        &ReadSupport,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "INTERNAL", "{error:?}");
    let state = journal.load().await.unwrap().unwrap();
    for call in reads() {
        let result = output(&state, &call.id);
        assert_eq!(result["ok"], true, "{result}");
    }
    assert_eq!(json!(state.reviewer_work), json!(before.reviewer_work));
    assert_eq!(json!(state.analysis), json!(before.analysis));
    assert_eq!(
        state.source_review.as_ref().unwrap().active_task,
        before.source_review.as_ref().unwrap().active_task
    );
    assert!(!state.reviewer_coverage.text.contains_key("support"));
    assert!(
        !state
            .reviewer_coverage
            .form_cells
            .contains_key("support-grid")
    );
    assert!(state.reviewer_coverage.views.is_empty());
    let pending = state.pending_coverage.as_ref().unwrap();
    assert!(pending.text.contains_key("support"));
    assert!(pending.form_cells.contains_key("support-grid"));
    assert_eq!(pending.views.len(), 1);
    assert!(pending.candidate.is_empty());
    assert!(state.read_bytes > before.read_bytes);
    (input, journal, before, state)
}

fn assert_exact_evidence(body: &[u8], state: &Checkpoint) {
    let body: Value = serde_json::from_slice(body).unwrap();
    let messages = body["messages"].as_array().unwrap();
    for id in ["text", "grid"] {
        let original = output(state, id);
        let message = messages
            .iter()
            .find(|message| message["tool_call_id"] == id)
            .unwrap_or_else(|| panic!("support {id} result was evicted"));
        let mut actual: Value = serde_json::from_str(message["content"].as_str().unwrap()).unwrap();
        // Older delivered reads may gain deterministic line-span annotations;
        // the original bytes, ranges, grid cells and all other fields stay exact.
        if original["result"].get("line_spans").is_none() {
            actual["result"]
                .as_object_mut()
                .unwrap()
                .remove("line_spans");
        }
        assert_eq!(actual, original, "support {id} result changed");
    }
    let view = state.source_views.values().next().unwrap();
    assert!(
        messages.contains(&view.message()),
        "exact identity and pixels must be in the request"
    );
}

#[tokio::test]
async fn support_reads_survive_request_and_all_delivery_resume_boundaries() {
    for boundary in 1..=3 {
        let (input, journal, _, mut pending) = pending_support().await;
        let before = json!(pending);
        let body = agent::request(&input, &config(), &mut pending)
            .await
            .unwrap();
        assert_exact_evidence(&body, &pending);
        assert_eq!(
            json!(pending),
            before,
            "request preparation grants no reading"
        );
        let visible = context::visible_work_evidence(&pending, &pending.transcript);
        assert!(visible.contains_key("text:support"));
        assert!(visible.contains_key("form:support-grid"));
        assert!(visible.keys().any(|key| key.starts_with("view:")));
        let model = work_script(vec![("source_index", json!({"offset":0,"limit":1}))]);
        *journal.fail_boundary_ack.lock().unwrap() = Some(pending.journal.sequence + boundary);
        *journal.interrupt_after.lock().unwrap() = Some(pending.turn + 1);
        let error = agent::run(
            &input,
            &config(),
            &journal,
            &model,
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "INTERNAL");
        let interrupted = journal.load().await.unwrap().unwrap();
        if boundary < 3 {
            assert_eq!(
                json!(interrupted.reviewer_coverage),
                json!(pending.reviewer_coverage)
            );
            let error = agent::run(
                &input,
                &config(),
                &journal,
                &model,
                &CancellationToken::new(),
            )
            .await
            .unwrap_err();
            assert_eq!(error.code, "INTERNAL");
        }
        let delivered = journal.load().await.unwrap().unwrap();
        assert!(delivered.reviewer_coverage.text.contains_key("support"));
        assert!(
            delivered
                .reviewer_coverage
                .form_cells
                .contains_key("support-grid")
        );
        assert_eq!(delivered.reviewer_coverage.views.len(), 1);
        assert!(
            !delivered
                .reviewer_coverage
                .candidate
                .contains_key("disposition:support")
        );
        assert_eq!(json!(delivered.analysis), json!(pending.analysis));
        let new_output_bytes = delivered
            .transcript
            .iter()
            .filter(|message| message["role"] == "tool")
            .filter(|message| {
                !["text", "grid", "pixels"]
                    .iter()
                    .any(|id| message["tool_call_id"] == *id)
            })
            .map(|message| message["content"].as_str().unwrap().len())
            .sum::<usize>();
        assert_eq!(
            delivered.read_bytes,
            pending.read_bytes + new_output_bytes,
            "only the new tool result is charged; resuming cannot charge support twice"
        );
        assert_eq!(model.bodies.lock().unwrap().len(), 1);
        let reserved = journal.reservations.lock().unwrap()[&pending.turn]
            .0
            .clone();
        assert_exact_evidence(&reserved, &pending);
    }
}

#[tokio::test]
async fn committed_support_pixels_survive_image_history_compaction_without_expanding_scope() {
    let (input, _, _, mut state) = pending_support().await;
    state.reviewer_coverage = state.pending_coverage.take().unwrap();
    let id = state.reviewer_coverage.views.keys().next().unwrap().clone();
    for message in &mut state.transcript {
        if message.get("source_view_refs").is_some() {
            *message = json!({"role":"user","content":json!({"history_omitted_source_views":[id]}).to_string()});
        }
    }
    let before = json!(state);
    let body = agent::request(&input, &config(), &mut state).await.unwrap();
    assert_exact_evidence(&body, &state);
    assert_eq!(json!(state), before);
    let mut unseen = state.clone();
    unseen.reviewer_coverage.views.clear();
    let body: Value = serde_json::from_slice(
        &agent::request(&input, &config(), &mut unseen)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(
        !body["messages"]
            .as_array()
            .unwrap()
            .contains(&state.source_views[&id].message()),
        "the other role's receipt or shared cache never confers reviewer reading"
    );
    let mut constrained = config();
    state.read_bytes = constrained.limits.max_read_bytes;
    let exact = agent::request(&input, &constrained, &mut state)
        .await
        .unwrap();
    let mut without_pixels: Value = serde_json::from_slice(&exact).unwrap();
    without_pixels["messages"]
        .as_array_mut()
        .unwrap()
        .retain(|message| message != &state.source_views[&id].message());
    constrained.limits.max_context_bytes = serde_json::to_vec(&without_pixels).unwrap().len();
    let error = agent::request(&input, &constrained, &mut state)
        .await
        .expect_err("support pixels must not silently disappear to fit the smaller budget");
    assert_eq!(
        error.code, "AGENT_TURN_BUDGET_EXCEEDED",
        "support image cannot silently vanish to fit"
    );
}

#[tokio::test]
async fn real_history_compaction_keeps_support_text_grid_and_exact_pixels() {
    let (input, _, _, mut state) = pending_support().await;
    state.reviewer_coverage = state.pending_coverage.take().unwrap();
    state.transcript.extend([
        json!({"role":"assistant","tool_calls":[{"id":"next","type":"function","function":{"name":"source_index","arguments":"{}"}}]}),
        json!({"role":"tool","tool_call_id":"next","content":"{\"ok\":true,\"result\":{}}"}),
    ]);
    let original = state.clone();
    let mut config = config();
    state.read_bytes = config.limits.max_read_bytes;
    let full: Value =
        serde_json::from_slice(&agent::request(&input, &config, &mut state).await.unwrap())
            .unwrap();
    let messages = full["messages"].as_array().unwrap();
    let next = messages
        .iter()
        .position(|message| message["tool_calls"][0]["id"] == "next")
        .unwrap();
    // Size the boundary from the actual SDK envelope, not raw checkpoint JSON.
    config.limits.max_history_bytes = serde_json::to_vec(&messages[2..next]).unwrap().len() - 1;
    let body = agent::request(&input, &config, &mut state).await.unwrap();
    assert_exact_evidence(&body, &original);
    assert!(
        !state
            .transcript
            .iter()
            .any(|message| message.get("source_view_refs").is_some()),
        "exercise actual image-history compaction rather than an unconstrained request"
    );
    assert_eq!(
        json!(state.reviewer_coverage),
        json!(original.reviewer_coverage)
    );
    assert_eq!(json!(state.reviewer_work), json!(original.reviewer_work));
}

#[tokio::test]
async fn supporting_view_identity_and_read_budget_still_fail_closed() {
    let (input, journal, _) = fixture().await;
    *journal.view.lock().unwrap() = Some(test_view()); // Wrong frozen source identity.
    *journal.interrupt_after.lock().unwrap() = Some(1);
    let error = agent::run(
        &input,
        &config(),
        &journal,
        &ReadSupport,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "INTERNAL");
    let saved = journal.load().await.unwrap().unwrap();
    assert_eq!(output(&saved, "pixels")["ok"], false);
    assert!(saved.source_views.is_empty());
    assert!(saved.pending_coverage.as_ref().unwrap().views.is_empty());
    assert!(saved.reviewer_coverage.views.is_empty());

    let (input, journal, mut before) = fixture().await;
    let mut constrained = config();
    constrained.limits.max_read_bytes = 1;
    before.config_sha256 = digest(&constrained).unwrap();
    *journal.state.lock().unwrap() = Some(before.clone());
    let error = agent::run(
        &input,
        &constrained,
        &journal,
        &ReadSupport,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "AGENT_TURN_BUDGET_EXCEEDED");
    let saved = journal.load().await.unwrap().unwrap();
    assert_eq!(
        json!(saved.reviewer_coverage),
        json!(before.reviewer_coverage)
    );
    assert_eq!(json!(saved.analysis), json!(before.analysis));
}
