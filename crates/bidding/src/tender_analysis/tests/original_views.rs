use super::*;

#[tokio::test]
async fn requested_original_evidence_takes_priority_over_optional_assigned_candidate_recall() {
    let mut config = config();
    let journal = fresh_review_journal_config(&config).await;
    let mut state = journal.load().await.unwrap().unwrap();
    state.transcript.clear();
    for index in 0..6 {
        let id = format!("saved-{index}");
        let record = Record {
            id: id.clone(),
            sources: vec![span()],
            data: RecordData::Fact {
                name: "saved fact".into(),
                value: "already delivered content ".repeat(40),
                scope: "source".into(),
            },
        };
        let version = digest(&record).unwrap();
        state.analysis.records.insert(id.clone(), record);
        state
            .reviewer_coverage
            .candidate
            .insert(format!("record:{id}"), version);
    }
    state.reviewer_work.as_mut().unwrap().focus.references = vec!["record:saved-0".into()];
    let text = input().source_units[0].text.clone();
    let original =
        json!({"ok":true,"result":{"source_id":"source","start":0,"end":text.len(),"text":text}});
    state.transcript = vec![
        json!({"role":"assistant","tool_calls":[{"id":"read","type":"function","function":{"name":"read_source","arguments":"{}"}}]}),
        json!({"role":"tool","tool_call_id":"read","content":original.to_string()}),
    ];
    let mut pending = state.reviewer_coverage.clone();
    tools::cover(
        pending.text.entry("source".into()).or_default(),
        0,
        text.len(),
    );
    state.pending_coverage = Some(pending);
    let checkpoint = json!(state);
    let full = agent::request(&input(), &config, &mut state).await.unwrap();
    let recalled = |bytes: &[u8]| {
        let body: Value = serde_json::from_slice(bytes).unwrap();
        body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|message| {
                let content: Value = serde_json::from_str(message["content"].as_str()?).ok()?;
                content["retained_candidate_details"]["items"]
                    .as_array()
                    .cloned()
            })
            .flatten()
            .collect::<Vec<_>>()
    };
    let full_count = recalled(&full).len();
    assert!(full_count > 1);
    config.limits.max_context_bytes = full.len() - 1;
    let bounded = agent::request(&input(), &config, &mut state).await.unwrap();
    assert!(bounded.len() <= config.limits.max_context_bytes);
    let items = recalled(&bounded);
    assert!(items.len() > 1 && items.len() < full_count);
    assert_eq!(items[0]["reference"], "record:saved-0");
    let body: Value = serde_json::from_slice(&bounded).unwrap();
    let original_content = original.to_string();
    assert!(
        body["messages"].as_array().unwrap().iter().any(|message| {
            message["tool_call_id"] == "read"
                && message["content"].as_str() == Some(original_content.as_str())
        }),
        "new original result must be delivered verbatim"
    );
    assert_eq!(
        json!(state),
        checkpoint,
        "context selection cannot commit pending reads or alter counters"
    );
    state.reviewer_coverage = state.pending_coverage.clone().unwrap();
    let reread_checkpoint = json!(state);
    let reread = agent::request(&input(), &config, &mut state).await.unwrap();
    assert_eq!(recalled(&reread), items);
    assert_eq!(json!(state), reread_checkpoint);

    // Exact candidate results also take precedence over optional recall.
    // The cache must not make the very tool for fetching evidence unavailable.
    let result =
        json!({"ok":true,"result":{"view":"detail","items":[state.analysis.records["saved-5"]]}})
            .to_string();
    state.transcript[0]["tool_calls"][0]["function"]["name"] = json!("inspect_analysis");
    state.transcript[1]["content"] = json!(result);
    let candidate_checkpoint = json!(state);
    let lookup = agent::request(&input(), &config, &mut state).await.unwrap();
    assert!(recalled(&lookup).len() > 1);
    let body: Value = serde_json::from_slice(&lookup).unwrap();
    assert!(body["messages"].as_array().unwrap().iter().any(|m| {
        m["tool_call_id"] == "read" && m["content"].as_str() == Some(result.as_str())
    }));
    assert_eq!(json!(state), candidate_checkpoint);

    let mut mandatory: Checkpoint = serde_json::from_value(checkpoint.clone()).unwrap();
    mandatory.reviewer_work.as_mut().unwrap().focus.references =
        (0..6).map(|i| format!("record:saved-{i}")).collect();
    assert!(
        agent::request(&input(), &config, &mut mandatory)
            .await
            .is_err()
    );
    // A subsequent request can use the full cache again; exclusions are local
    // to request construction, not persisted reading or recovery state.
    let mut restored: Checkpoint = serde_json::from_value(checkpoint).unwrap();
    config.limits.max_context_bytes = full.len();
    let restored = agent::request(&input(), &config, &mut restored)
        .await
        .unwrap();
    assert_eq!(recalled(&restored).len(), full_count);
}

#[tokio::test]
async fn active_source_review_keeps_its_own_pixels_after_history_eviction_within_total_budget() {
    let mut config = config();
    config.limits.max_history_bytes = 128;
    let journal = fresh_review_journal_config(&config).await;
    let mut state = journal.load().await.unwrap().unwrap();
    let view = test_view();
    let id = view.id().unwrap();
    state.source_views.insert(id.clone(), view.clone());
    state
        .reviewer_coverage
        .views
        .insert(id.clone(), view.identity.clone());
    state
        .analysis
        .coverage
        .views
        .insert(id.clone(), view.identity.clone());
    state
        .reviewer_work
        .as_mut()
        .unwrap()
        .focus
        .source_spans
        .push(Span {
            source_id: view.identity.source_id.clone(),
            start: 0,
            end: 0,
            view_id: Some(id.clone()),
            grid_cell: None,
        });
    let assistant = |id: &str, name: &str| {
        json!({"role":"assistant","tool_calls":[
        {"id":id,"type":"function","function":{"name":name,"arguments":"{}"}}]})
    };
    let result =
        |id: &str| json!({"role":"tool","tool_call_id":id,"content":"{\"ok\":true,\"result\":{}}"});
    state.transcript = vec![
        assistant("view", "read_source_view"),
        result("view"),
        json!({"role":"user","source_view_refs":[id]}),
        assistant("latest", "inspect_analysis"),
        result("latest"),
    ];
    let before = state.clone();
    let body = agent::request(&input(), &config, &mut state).await.unwrap();
    let image_count = |bytes: &[u8]| {
        let body: Value = serde_json::from_slice(bytes).unwrap();
        body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|m| m["content"].as_array().into_iter().flatten())
            .filter(|item| item["type"] == "image_url")
            .count()
    };
    assert_eq!(
        image_count(&body),
        1,
        "active original must survive losing its old transcript group"
    );
    assert!(
        state
            .transcript
            .iter()
            .any(|m| m["tool_call_id"] == "latest")
    );
    assert_eq!(
        json!(state.reviewer_coverage),
        json!(before.reviewer_coverage)
    );
    assert_eq!(json!(state.source_views), json!(before.source_views));
    assert_eq!(
        json!(state.reviewer_progress),
        json!(before.reviewer_progress)
    );
    assert_eq!(state.read_bytes, before.read_bytes);
    let mut primary = before.clone();
    primary.role = Role::Main;
    primary.main_work = primary.reviewer_work.clone();
    primary.transcript.clear();
    primary.reviewer_coverage.views.clear();
    let primary_body = agent::request(&input(), &config, &mut primary)
        .await
        .unwrap();
    assert_eq!(
        image_count(&primary_body),
        1,
        "the main role also retains its own explicitly focused original"
    );
    let mut probe = before.clone();
    probe.transcript.pop(); // The latest candidate call has not returned yet.
    let pending_probe = probe.clone();
    let args = json!({"kind":"all","view":"detail","offset":0,"limit":1});
    let call = ChatToolCall {
        id: "latest".into(),
        name: "inspect_analysis".into(),
        arguments: args.to_string(),
    };
    let committed = probe.reviewer_coverage.clone();
    let page = agent::inspect_in_context(
        &input(),
        &config,
        &mut probe,
        &args,
        &[call],
        &[],
        &committed,
    )
    .await
    .unwrap();
    assert_eq!(
        page["items"].as_array().unwrap().len(),
        1,
        "moving the active pixels out of history must not falsely reject an exact candidate lookup"
    );
    assert_eq!(probe.transcript, pending_probe.transcript);
    assert_eq!(probe.read_bytes, pending_probe.read_bytes);
    assert_eq!(probe.turn, pending_probe.turn);
    let mut cache_only = pending_probe;
    cache_only.reviewer_coverage.views.clear();
    let before_failed_probe = cache_only.clone();
    let call = ChatToolCall {
        id: "latest".into(),
        name: "inspect_analysis".into(),
        arguments: args.to_string(),
    };
    let committed = cache_only.reviewer_coverage.clone();
    assert!(
        agent::inspect_in_context(
            &input(),
            &config,
            &mut cache_only,
            &args,
            &[call],
            &[],
            &committed
        )
        .await
        .unwrap_err()
        .contains("cannot fit together"),
        "cached pixels absent from the final request cannot satisfy retention"
    );
    assert_eq!(
        json!(cache_only),
        json!(before_failed_probe),
        "a failed sizing probe must restore all tentative state"
    );
    let mut unseen = state.clone();
    unseen.reviewer_coverage.views.clear();
    let without_image = agent::request(&input(), &config, &mut unseen)
        .await
        .unwrap();
    assert_eq!(
        image_count(&without_image),
        0,
        "the primary receipt and shared cache cannot grant independent reading"
    );
    let mut cross_input = input();
    let mut other_source = cross_input.source_units[0].clone();
    other_source.source_unit_revision_id = "other".into();
    other_source.ordinal += 1;
    cross_input.source_units.push(other_source);
    let mut cross = state.clone();
    cross.transcript.clear();
    cross.source_review = Some(source_review::initialize(&cross_input, &config).unwrap());
    source_review::select_next(&cross_input, &config, &mut cross).unwrap();
    cross
        .reviewer_work
        .as_mut()
        .unwrap()
        .source_scope
        .push("other".into());
    let mut other_view = view.clone();
    other_view.identity.source_id = "other".into();
    let other_id = other_view.id().unwrap();
    cross
        .reviewer_coverage
        .views
        .insert(other_id.clone(), other_view.identity.clone());
    cross.source_views.insert(other_id.clone(), other_view);
    cross
        .reviewer_work
        .as_mut()
        .unwrap()
        .focus
        .source_spans
        .push(Span {
            source_id: "other".into(),
            start: 0,
            end: 0,
            view_id: Some(other_id),
            grid_cell: None,
        });
    let coverage_before = json!(cross.reviewer_coverage);
    let both = agent::request(&cross_input, &config, &mut cross)
        .await
        .unwrap();
    assert_eq!(
        image_count(&both),
        2,
        "a cross-source original in the declared comparison scope must survive history eviction"
    );
    let cited_view = cross
        .reviewer_work
        .as_mut()
        .unwrap()
        .focus
        .source_spans
        .pop()
        .unwrap();
    cross.analysis.records.insert(
        "visual-claim".into(),
        Record {
            id: "visual-claim".into(),
            sources: vec![cited_view],
            data: RecordData::Fact {
                name: "visual claim".into(),
                value: "previously extracted value".into(),
                scope: "source".into(),
            },
        },
    );
    cross.reviewer_work.as_mut().unwrap().focus.references = vec!["record:visual-claim".into()];
    let implicit = agent::request(&cross_input, &config, &mut cross)
        .await
        .unwrap();
    assert_eq!(
        image_count(&implicit),
        1,
        "candidate citations alone must not pin every previously visited image; select a visual comparison explicitly"
    );
    cross
        .reviewer_work
        .as_mut()
        .unwrap()
        .source_scope
        .retain(|id| id != "other");
    cross
        .reviewer_work
        .as_mut()
        .unwrap()
        .focus
        .source_spans
        .retain(|span| span.source_id != "other");
    cross
        .reviewer_work
        .as_mut()
        .unwrap()
        .focus
        .references
        .clear();
    let one = agent::request(&cross_input, &config, &mut cross)
        .await
        .unwrap();
    assert_eq!(
        image_count(&one),
        1,
        "leaving the scope releases its pixels without erasing prior receipts"
    );
    assert_eq!(json!(cross.reviewer_coverage), coverage_before);
    config.limits.max_context_bytes = without_image.len() + view.jpeg_base64.len() / 2;
    assert!(
        agent::request(&input(), &config, &mut state).await.is_err(),
        "working images still obey the total ceiling; do not silently omit the active original"
    );
}

#[tokio::test]
async fn original_pixels_are_independently_viewed_and_survive_resume_without_rerender() {
    let journal = MemoryJournal::default();
    *journal.view.lock().unwrap() = Some(test_view());
    *journal.interrupt_after.lock().unwrap() = Some(2);
    let model = script();
    {
        let mut calls = model.calls.lock().unwrap();
        calls.insert(
            1,
            ("read_source_view".into(), json!({"source_id":"source"})),
        );
        // Both independent review rounds must actually receive the same pixels.
        for i in (0..calls.len()).rev().collect::<Vec<_>>() {
            if calls[i].0 == "request_review" {
                calls.insert(
                    i + 2,
                    ("read_source_view".into(), json!({"source_id":"source"})),
                );
            }
        }
    }
    assert_eq!(
        agent::run(
            &input(),
            &config(),
            &journal,
            &model,
            &CancellationToken::new()
        )
        .await
        .unwrap_err()
        .code,
        "INTERNAL"
    );
    let result = agent::run(
        &input(),
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(result.quality, "verified");
    assert_eq!(
        *journal.view_calls.lock().unwrap(),
        1,
        "resume/reviewer must reuse frozen pixels"
    );
    assert_eq!(result.source_views.len(), 1);
    assert_eq!(result.review.coverage.views, result.analysis.coverage.views);
    let bodies = model.bodies.lock().unwrap();
    assert!(
        bodies
            .iter()
            .filter(|b| b["messages"][0]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("INDEPENDENT"))
            .any(|b| b["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m["content"]
                    .as_array()
                    .is_some_and(|parts| parts.iter().any(|p| p["type"] == "image_url"))))
    );
    let id = result.source_views.keys().next().unwrap().clone();
    let mut span = Span {
        source_id: "source".into(),
        start: 0,
        end: 0,
        view_id: Some(id),
        grid_cell: None,
    };
    assert!(tools::validate_span(&input(), &result.review.coverage, &span).is_ok());
    assert!(tools::validate_span(&input(), &Coverage::default(), &span).is_err());
    span.end = 3;
    assert!(tools::validate_span(&input(), &result.review.coverage, &span).is_err());
}

#[tokio::test]
async fn a_failed_original_view_cannot_be_published_as_verified() {
    let journal = MemoryJournal::default();
    let model = script();
    model.calls.lock().unwrap().insert(
        1,
        ("read_source_view".into(), json!({"source_id":"source"})),
    );
    let result = agent::run(
        &input(),
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(result.quality, "needs_review");
    assert!(!result.analysis.coverage.view_failures.is_empty());
    assert!(result.source_views.is_empty());
}

#[tokio::test]
async fn invalid_or_oversized_pixels_never_establish_visual_coverage() {
    for oversize in [false, true] {
        let journal = MemoryJournal::default();
        let model = script();
        let mut config = config();
        let mut view = test_view();
        if oversize {
            config.limits.max_source_view_bytes = 1;
        } else {
            view.identity.image_sha256 = "0".repeat(64);
        }
        *journal.view.lock().unwrap() = Some(view);
        model.calls.lock().unwrap().insert(
            1,
            ("read_source_view".into(), json!({"source_id":"source"})),
        );
        let result = agent::run(
            &input(),
            &config,
            &journal,
            &model,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(result.quality, "needs_review");
        assert!(result.analysis.coverage.views.is_empty());
        assert!(result.source_views.is_empty());
        assert!(model.bodies.lock().unwrap().iter().all(|b| {
            !b["messages"]
                .to_string()
                .contains("data:image/jpeg;base64,")
        }));
    }
}

#[test]
fn primary_visual_coverage_is_not_reviewer_visual_coverage() {
    let input = input();
    let mut analysis = read_analysis(&input);
    let view = test_view();
    analysis
        .coverage
        .views
        .insert(view.id().unwrap(), view.identity);
    let mut reviewer = analysis.coverage.clone();
    reviewer.views.clear();
    assert!(tools::reading_gaps(&input, &reviewer).is_empty());
    assert!(
        tools::review_gaps(&input, &analysis, &reviewer)
            .iter()
            .any(|g| g["kind"] == "unreviewed_source_view")
    );
}

#[tokio::test]
async fn oversized_pending_image_batch_resumes_with_only_delivered_visual_coverage() {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use sha2::{Digest, Sha256};
    let journal = MemoryJournal::default();
    agent::run(
        &input(),
        &config(),
        &journal,
        &script(),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    let mut state = journal.state.lock().unwrap().clone().unwrap();
    state.role = Role::Reviewer;
    state.done = false;
    state.transcript.clear();
    state.pending_coverage = Some(state.reviewer_coverage.clone());
    let image = image::RgbImage::from_fn(160, 160, |x, y| {
        image::Rgb([
            (x.wrapping_mul(37) ^ y.wrapping_mul(71)) as u8,
            (x.wrapping_mul(17) ^ y.wrapping_mul(31)) as u8,
            (x ^ y) as u8,
        ])
    });
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 90)
        .encode_image(&image)
        .unwrap();
    let mut calls = vec![];
    let mut results = vec![];
    let mut refs = vec![];
    for i in 0..12 {
        let mut view = test_view();
        view.identity.source_id = format!("image-source-{i}");
        view.identity.page_ordinal = i;
        view.identity.width = 160;
        view.identity.height = 160;
        view.identity.image_sha256 = hex::encode(Sha256::digest(&bytes));
        view.jpeg_base64 = STANDARD.encode(&bytes);
        view.validate(&view.identity.source_id, 160, bytes.len())
            .unwrap();
        let id = view.id().unwrap();
        calls.push(
            json!({"id":format!("view-{i}"),"type":"function","function":{"name":"read_source_view",
            "arguments":json!({"source_id":view.identity.source_id}).to_string()}}),
        );
        results.push(
            json!({"role":"tool","tool_call_id":format!("view-{i}"),"content":json!({"ok":true,
            "result":{"view_id":id,"identity":view.identity}}).to_string()}),
        );
        state
            .pending_coverage
            .as_mut()
            .unwrap()
            .views
            .insert(id.clone(), view.identity.clone());
        state
            .analysis
            .coverage
            .views
            .insert(id.clone(), view.identity.clone());
        state.source_views.insert(id.clone(), view);
        refs.push(id);
    }
    state
        .transcript
        .push(json!({"role":"assistant","content":null,"tool_calls":calls}));
    state.transcript.extend(results);
    state
        .transcript
        .push(json!({"role":"user","source_view_refs":refs}));
    let before = state.clone();
    // Derive a partial-admission budget from the actual prompt and image
    // payloads. A fixed byte/token cap eventually fits no image as tools grow.
    let mut sizing_config = config();
    sizing_config.limits.max_context_bytes = 2_000_000;
    let mut sizing_state = before.clone();
    let mut sized: Value = serde_json::from_slice(
        &agent::request(&input(), &sizing_config, &mut sizing_state)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        sizing_state.pending_coverage.as_ref().unwrap().views.len(),
        refs.len()
    );
    let mut images = 0;
    sized["messages"].as_array_mut().unwrap().retain(|message| {
        let has_image = message["content"]
            .as_array()
            .is_some_and(|parts| parts.iter().any(|part| part["type"] == "image_url"));
        if has_image {
            images += 1;
        }
        !has_image || images <= refs.len() / 2
    });
    let byte_budget = serde_json::to_vec(&sized).unwrap().len();
    let token_budget = crate::agent_runtime::chat::estimate_input_tokens(
        &sized,
        sizing_config.limits.image_token_reserve,
        sizing_config.limits.token_safety_margin,
    )
    .unwrap()
        + sizing_config.provider.max_tokens as usize;
    for token_limited in [false, true] {
        let mut state = before.clone();
        let mut config = config();
        if token_limited {
            config.limits.max_context_tokens = token_budget;
            config.limits.max_context_bytes = sizing_config.limits.max_context_bytes;
        } else {
            config.limits.max_context_bytes = byte_budget;
        }
        let body = agent::request(&input(), &config, &mut state).await.unwrap();
        assert!(body.len() <= config.limits.max_context_bytes);
        let pending = state.pending_coverage.as_ref().unwrap();
        assert!(pending.views.len() < refs.len());
        assert!(!pending.views.is_empty());
        assert!(
            state.reviewer_coverage.views.is_empty(),
            "preparing a request is not delivery"
        );
        assert_eq!(
            state.analysis.coverage.views,
            before.analysis.coverage.views
        );
        assert_eq!(
            digest(&state.source_views).unwrap(),
            digest(&before.source_views).unwrap()
        );
        assert_eq!(state.read_bytes, before.read_bytes);
        assert_eq!(state.tool_calls, before.tool_calls);
        let request: Value = serde_json::from_slice(&body).unwrap();
        let messages = request["messages"].as_array().unwrap();
        let images = messages
            .iter()
            .filter(|m| {
                m["content"]
                    .as_array()
                    .is_some_and(|parts| parts.iter().any(|p| p["type"] == "image_url"))
            })
            .count();
        assert_eq!(images, pending.views.len());
        let errors = messages
            .iter()
            .filter(|m| m["role"] == "tool")
            .filter(|m| {
                let output: Value = serde_json::from_str(m["content"].as_str().unwrap()).unwrap();
                output["ok"] == false && output["error"].as_str().unwrap().contains("NOT delivered")
            })
            .count();
        assert_eq!(errors + images, refs.len());
        assert!(
            tools::review_gaps(&input(), &state.analysis, &state.reviewer_coverage)
                .iter()
                .any(|g| g["kind"] == "unreviewed_source_view")
        );
        // Resume must reserve identical provider bytes without resetting budgets.
        let mut replay: Checkpoint =
            serde_json::from_value(serde_json::to_value(&before).unwrap()).unwrap();
        assert_eq!(
            body,
            agent::request(&input(), &config, &mut replay)
                .await
                .unwrap()
        );
        assert_eq!(digest(&state).unwrap(), digest(&replay).unwrap());
    }
}
