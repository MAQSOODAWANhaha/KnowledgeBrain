use super::*;
use knowledge::models::{ChatToolCall, ChatTurn};

fn turn(calls: Vec<(&str, Value)>) -> ChatTurn {
    ChatTurn {
        content: String::new(),
        finish_reason: "tool_calls".into(),
        usage: None,
        tool_calls: calls
            .into_iter()
            .enumerate()
            .map(|(index, (name, args))| ChatToolCall {
                id: format!("batch-{index}"),
                name: name.into(),
                arguments: args.to_string(),
            })
            .collect(),
    }
}

#[tokio::test]
async fn complete_draft_batch_compiles_and_enters_independent_review_without_transition_calls() {
    let (input, result) = fixture();
    let mut state = work_checkpoint(ready(&input, &result));
    agent::execute_turn(
        &input,
        &result,
        &config(),
        &mut state,
        turn(vec![(
            "composition_coverage",
            json!({"offset":0,"limit":100}),
        )]),
        Default::default(),
        &tokio_util::sync::CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(
        state.workspace.artifact.is_some(),
        "host must compile the complete draft"
    );
    assert!(
        state.workspace.reviewing,
        "host must enter independent review"
    );
    assert!(state.workspace.review_coverage.text.is_empty());
}

#[tokio::test]
async fn repeated_delivered_source_does_not_require_an_empty_receipt_turn() {
    let (input, result) = fixture();
    let mut state = work_checkpoint(ready(&input, &result));
    let before_coverage = json!(state.workspace.source_coverage);
    let output = step(
        &input,
        &result,
        &mut state,
        vec![(
            "read_source",
            json!({"source_id":"s0","start":0,"max_bytes":10000}),
        )],
    )
    .await;
    let read: Value = serde_json::from_str(output[0]["content"].as_str().unwrap()).unwrap();
    assert_eq!(read["ok"], true);
    assert!(
        state.read_bytes > 0,
        "repeated reads still consume the read budget"
    );
    assert_eq!(json!(state.workspace.source_coverage), before_coverage);
    assert!(
        state.pending_delivery.is_none(),
        "no qualification was added"
    );
    assert!(
        state.workspace.reviewing,
        "the host should advance without a receipt-only model call"
    );
    assert!(state.workspace.review_coverage.text.is_empty());
}

#[tokio::test]
async fn repeated_read_does_not_discard_a_fresh_read_in_the_same_batch() {
    let (input, result) = fixture();
    let mut state = work_checkpoint(ready(&input, &result));
    state.workspace.source_coverage.text.remove("s1");
    step(
        &input,
        &result,
        &mut state,
        vec![
            (
                "read_source",
                json!({"source_id":"s1","start":0,"max_bytes":10000}),
            ),
            (
                "read_source",
                json!({"source_id":"s0","start":0,"max_bytes":10000}),
            ),
        ],
    )
    .await;
    assert!(!state.workspace.source_coverage.text.contains_key("s1"));
    let pending = state.pending_delivery.as_ref().unwrap();
    assert!(pending.coverage.text.contains_key("s1"));
    assert_eq!(pending.messages.len(), 2);
    assert!(!state.workspace.reviewing);
    reserve_delivery(&input, &result, &mut state).await;
    step(
        &input,
        &result,
        &mut state,
        vec![("composition_coverage", json!({"offset":0,"limit":100}))],
    )
    .await;
    assert!(state.workspace.source_coverage.text.contains_key("s1"));
    assert!(state.workspace.reviewing);
}

#[tokio::test]
async fn same_batch_original_read_cannot_authorize_a_plan_write() {
    let (input, result) = fixture();
    let mut state = work_checkpoint(Workspace::new(&input, &result).unwrap());
    let mut plan = plan_for_section(&result, &section(&input, 0));
    plan["expected_draft_sha256"] = json!(digest(&state.workspace.draft).unwrap());
    let output = agent::execute_turn(
        &input,
        &result,
        &config(),
        &mut state,
        turn(vec![
            (
                "read_source",
                json!({"source_id":"s0","start":0,"max_bytes":10000}),
            ),
            ("put_composition_plan_item", plan),
        ]),
        Default::default(),
        &tokio_util::sync::CancellationToken::new(),
    )
    .await
    .unwrap();
    let saved: Value = serde_json::from_str(output[1]["content"].as_str().unwrap()).unwrap();
    assert_eq!(
        saved["ok"], false,
        "a read result has not reached the model yet"
    );
    assert!(state.workspace.draft.plan.is_empty());
}

async fn step(
    input: &FrozenInput,
    result: &AnalysisResult,
    state: &mut agent::Checkpoint,
    calls: Vec<(&str, Value)>,
) -> Vec<Value> {
    agent::execute_turn(
        input,
        result,
        &config(),
        state,
        turn(calls),
        Default::default(),
        &tokio_util::sync::CancellationToken::new(),
    )
    .await
    .unwrap()
}

async fn reserve_delivery(
    input: &FrozenInput,
    result: &AnalysisResult,
    state: &mut agent::Checkpoint,
) {
    let body = agent::request(input, state, result, &config())
        .await
        .unwrap();
    state
        .journal
        .prepare(
            state.turn,
            if state.workspace.reviewing {
                "reviewer"
            } else {
                "main"
            },
            &body,
        )
        .unwrap();
}

#[tokio::test]
async fn errors_and_pending_reads_prevent_automatic_phase_change() {
    let (input, result) = fixture();
    let mut state = work_checkpoint(ready(&input, &result));
    step(
        &input,
        &result,
        &mut state,
        vec![
            ("missing_tool", json!({})),
            ("composition_coverage", json!({"offset":0,"limit":100})),
        ],
    )
    .await;
    assert!(!state.workspace.reviewing);
    state.workspace.source_coverage.text.remove("s0");
    step(
        &input,
        &result,
        &mut state,
        vec![(
            "read_source",
            json!({"source_id":"s0","start":0,"max_bytes":10000}),
        )],
    )
    .await;
    assert!(state.pending_delivery.is_some());
    assert!(!state.workspace.reviewing);
    reserve_delivery(&input, &result, &mut state).await;
    step(
        &input,
        &result,
        &mut state,
        vec![("composition_coverage", json!({"offset":0,"limit":100}))],
    )
    .await;
    assert!(state.workspace.reviewing);
    assert!(state.pending_delivery.is_none());
    assert!(
        state.workspace.review_coverage.text.is_empty(),
        "Main receipts do not grant reviewer evidence"
    );
}

#[tokio::test]
async fn same_batch_inspection_does_not_authorize_review_judgment() {
    let (input, result) = fixture();
    let mut workspace = review_workspace(&input, &result);
    workspace.inspected.clear();
    let mut state = work_checkpoint(workspace);
    let output = step(
        &input,
        &result,
        &mut state,
        vec![
            ("inspect_composition", json!({"offset":0,"limit":100})),
            ("put_composition_review", plan_judgment(&input, 0)),
        ],
    )
    .await;
    let judgment: Value = serde_json::from_str(output[1]["content"].as_str().unwrap()).unwrap();
    assert_eq!(judgment["ok"], false);
    assert!(state.workspace.plan_reviews.is_empty());
    assert!(state.workspace.inspected.is_empty());
    assert!(state.pending_delivery.is_some());
    reserve_delivery(&input, &result, &mut state).await;
    let output = step(
        &input,
        &result,
        &mut state,
        vec![("put_composition_review", plan_judgment(&input, 0))],
    )
    .await;
    let judgment: Value = serde_json::from_str(output[0]["content"].as_str().unwrap()).unwrap();
    assert_eq!(judgment["ok"], true, "{judgment}");
}

#[tokio::test]
async fn pending_receipts_require_the_exact_frozen_body_and_survive_capacity_failure() {
    let (input, result) = fixture();
    let mut state = work_checkpoint(Workspace::new(&input, &result).unwrap());
    step(
        &input,
        &result,
        &mut state,
        vec![(
            "read_source",
            json!({"source_id":"s0","start":0,"max_bytes":10000}),
        )],
    )
    .await;
    assert!(state.workspace.source_coverage.text.is_empty());
    let before = json!(&state.pending_delivery);
    let body = agent::request(&input, &state, &result, &config())
        .await
        .unwrap();
    let mut small = config();
    small.limits.max_context_bytes = body.len() - 1;
    let error = agent::request(&input, &state, &result, &small)
        .await
        .unwrap_err();
    assert_eq!(error.code, "AGENT_TURN_BUDGET_EXCEEDED");
    assert_eq!(json!(&state.pending_delivery), before);
    assert!(state.workspace.source_coverage.text.is_empty());
    let mut clipped: Value = serde_json::from_slice(&body).unwrap();
    clipped["messages"]
        .as_array_mut()
        .unwrap()
        .retain(|message| message["role"] != "tool");
    state
        .journal
        .prepare(
            state.turn,
            "main",
            &serde_json_canonicalizer::to_vec(&clipped).unwrap(),
        )
        .unwrap();
    assert!(
        super::super::agent_work::accept_pending(&mut state, &result)
            .unwrap_err()
            .contains("not delivered")
    );
    assert_eq!(json!(&state.pending_delivery), before);
    assert!(state.workspace.source_coverage.text.is_empty());
    state.journal.pending = None;
    state.journal.prepare(state.turn, "main", &body).unwrap();
    let mut restored: agent::Checkpoint = serde_json::from_value(json!(state)).unwrap();
    super::super::agent_work::accept_pending(&mut restored, &result).unwrap();
    assert!(restored.pending_delivery.is_none());
    assert!(restored.workspace.source_coverage.text.contains_key("s0"));
    assert!(restored.workspace.review_coverage.text.is_empty());
}

#[tokio::test]
async fn compile_failure_commits_feedback_and_retries_only_after_draft_change() {
    let (input, result) = fixture();
    let mut state = work_checkpoint(ready(&input, &result));
    let mut small = config();
    small.limits.max_docx_bytes = 1;
    agent::execute_turn(
        &input,
        &result,
        &small,
        &mut state,
        turn(vec![(
            "composition_coverage",
            json!({"offset":0,"limit":100}),
        )]),
        Default::default(),
        &tokio_util::sync::CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(
        state.turn, 1,
        "correctable compilation errors commit the model response"
    );
    let feedback = json!(&state.workspace.compile_feedback);
    assert!(feedback["error"].is_string());
    assert!(!state.workspace.reviewing);
    let mut restored: agent::Checkpoint = serde_json::from_value(json!(state)).unwrap();
    // Even a larger test limit cannot silently retry the already failed draft.
    step(
        &input,
        &result,
        &mut restored,
        vec![("composition_coverage", json!({"offset":0,"limit":100}))],
    )
    .await;
    assert_eq!(json!(&restored.workspace.compile_feedback), feedback);
    assert!(restored.workspace.artifact.is_none());
    restored
        .workspace
        .draft
        .presentation
        .as_mut()
        .unwrap()
        .explanation
        .push_str("；修订稿");
    step(
        &input,
        &result,
        &mut restored,
        vec![("composition_coverage", json!({"offset":0,"limit":100}))],
    )
    .await;
    assert!(restored.workspace.reviewing);
    assert!(restored.workspace.compile_feedback.is_none());
}

#[tokio::test]
async fn received_replay_repeats_host_transition_without_another_model_call() {
    for fail_compile in [false, true] {
        let (input, result) = fixture();
        let mut settings = config();
        if fail_compile {
            settings.limits.max_docx_bytes = 1;
        }
        let journal = Journal::default();
        let mut initial = work_checkpoint(ready(&input, &result));
        // A ready persisted draft needs one ordinary successful response, not a
        // generated compile/request_review pair, to perform the transition.
        initial.contract_sha256 = settings.contract_sha256().unwrap();
        initial.turn = 1;
        initial.journal.sequence = 3;
        *journal.state.lock().unwrap() = Some(json!(initial));
        *journal.fail_boundary_ack.lock().unwrap() = Some(5);
        let model = Model {
            turns: Mutex::new(
                vec![("composition_coverage", json!({"offset":0,"limit":100}))].into(),
            ),
            invalid_replies: Mutex::new(0),
        };
        let error = agent::run(
            &input,
            &result,
            &settings,
            &journal,
            &model,
            &tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "INTERNAL");
        let received: agent::Checkpoint =
            serde_json::from_value(journal.state.lock().unwrap().clone().unwrap()).unwrap();
        assert!(received.journal.response().is_some());
        assert!(received.workspace.artifact.is_none());
        *journal.fail_ack_at.lock().unwrap() = Some(2);
        let error = agent::run(
            &input,
            &result,
            &settings,
            &journal,
            &model,
            &tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "INTERNAL");
        let committed: agent::Checkpoint =
            serde_json::from_value(journal.state.lock().unwrap().clone().unwrap()).unwrap();
        assert_eq!(committed.workspace.reviewing, !fail_compile);
        assert_eq!(committed.workspace.artifact.is_some(), !fail_compile);
        assert_eq!(committed.workspace.compile_feedback.is_some(), fail_compile);
        assert!(committed.journal.pending.is_none());
        assert_eq!(*journal.calls.lock().unwrap(), 1);
    }
}

#[tokio::test]
async fn original_view_receipt_requires_pixels_and_pending_images_cannot_be_deferred() {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use sha2::{Digest, Sha256};
    let (input, mut result) = fixture();
    let image = image::RgbImage::from_pixel(8, 8, image::Rgb([255, 255, 255]));
    let mut bytes = vec![];
    image::codecs::jpeg::JpegEncoder::new(&mut bytes)
        .encode_image(&image)
        .unwrap();
    let view = analysis::views::SourceView {
        identity: analysis::views::ViewIdentity {
            source_id: "s0".into(),
            original_sha256: "a".repeat(64),
            image_sha256: hex::encode(Sha256::digest(&bytes)),
            page_ordinal: 0,
            width: 8,
            height: 8,
            renderer: "docreader-source-view-v1/test".into(),
        },
        jpeg_base64: STANDARD.encode(&bytes),
    };
    let id = view.id().unwrap();
    result.source_views.insert(id.clone(), view);
    let mut state = work_checkpoint(Workspace::new(&input, &result).unwrap());
    step(
        &input,
        &result,
        &mut state,
        vec![("read_source_view", json!({"source_id":"s0"}))],
    )
    .await;
    let body = agent::request(&input, &state, &result, &config())
        .await
        .unwrap();
    let mut small = config();
    small.limits.max_context_bytes = body.len() - 1;
    assert_eq!(
        agent::request(&input, &state, &result, &small)
            .await
            .unwrap_err()
            .code,
        "AGENT_TURN_BUDGET_EXCEEDED"
    );
    assert!(state.transcript.iter().any(|message| {
        message["source_view_refs"]
            .as_array()
            .is_some_and(|ids| ids.contains(&json!(id)))
    }));
    let mut no_pixels: Value = serde_json::from_slice(&body).unwrap();
    no_pixels["messages"]
        .as_array_mut()
        .unwrap()
        .retain(|message| !message["content"].is_array());
    state
        .journal
        .prepare(
            state.turn,
            "main",
            &serde_json_canonicalizer::to_vec(&no_pixels).unwrap(),
        )
        .unwrap();
    assert!(
        super::super::agent_work::accept_pending(&mut state, &result)
            .unwrap_err()
            .contains("pixels")
    );
    assert!(state.workspace.source_coverage.views.is_empty());
    assert!(state.pending_delivery.is_some());
    state.journal.pending = None;
    state.journal.prepare(state.turn, "main", &body).unwrap();
    super::super::agent_work::accept_pending(&mut state, &result).unwrap();
    assert!(state.workspace.source_coverage.views.contains_key(&id));
    assert!(state.workspace.review_coverage.views.is_empty());
    step(
        &input,
        &result,
        &mut state,
        vec![("read_source_view", json!({"source_id":"s0"}))],
    )
    .await;
    assert!(
        state.pending_delivery.is_none(),
        "the exact Main image was already delivered"
    );
    state.workspace.reviewing = true;
    step(
        &input,
        &result,
        &mut state,
        vec![("read_source_view", json!({"source_id":"s0"}))],
    )
    .await;
    assert!(
        state.pending_delivery.is_some(),
        "Main image qualification cannot cancel Reviewer delivery"
    );
    assert!(state.workspace.review_coverage.views.is_empty());
}
