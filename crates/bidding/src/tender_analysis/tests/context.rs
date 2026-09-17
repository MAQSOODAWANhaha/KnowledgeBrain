use super::*;

#[tokio::test]
async fn invalid_token_configuration_cannot_reserve_or_call_a_model() {
    for invalid_case in 0..4 {
        let mut config = config();
        match invalid_case {
            0 => {
                config.limits.max_context_tokens =
                    config.provider.max_tokens as usize + config.limits.token_safety_margin
            }
            1 => config.limits.token_safety_margin = 0,
            2 => config.limits.image_token_reserve = 0,
            _ => config.limits.token_safety_margin = usize::MAX,
        }
        let journal = MemoryJournal::default();
        let model = script();
        assert!(
            agent::run(
                &input(),
                &config,
                &journal,
                &model,
                &CancellationToken::new()
            )
            .await
            .is_err()
        );
        assert!(journal.reservations.lock().unwrap().is_empty());
        assert!(model.bodies.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn token_budget_trims_both_roles_and_reserves_output_below_the_byte_ceiling() {
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
    let saved = journal.load().await.unwrap().unwrap();
    for role in [Role::Main, Role::Reviewer] {
        let mut state = saved.clone();
        state.role = role;
        state.transcript.clear();
        let mut config = config();
        let base_bytes = agent::request(&input(), &config, &mut state)
            .await
            .unwrap()
            .len();
        config.limits.max_context_tokens = base_bytes
            + config.provider.max_tokens as usize
            + config.limits.token_safety_margin
            + 6000;
        config.limits.max_history_bytes = 90000;
        for i in 0..8 {
            state.transcript.extend([
                json!({"role":"assistant","content":null,"tool_calls":[{"id":format!("read-{i}"),
                    "type":"function","function":{"name":"read_source","arguments":"{}"}}]}),
                json!({"role":"tool","tool_call_id":format!("read-{i}"),
                    "content":format!("range-{i}:{}", "中文😀".repeat(300))}),
            ]);
        }
        state.pending_coverage = Some(Coverage::default());
        let before = state.clone();
        let bytes = agent::request(&input(), &config, &mut state).await.unwrap();
        assert!(
            bytes.len() + config.limits.token_safety_margin + config.provider.max_tokens as usize
                <= config.limits.max_context_tokens
        );
        assert!(bytes.len() < config.limits.max_context_bytes);
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(!text.contains("range-0:"));
        assert!(text.contains("range-7:"));
        assert_eq!(
            digest(&before.pending_coverage).unwrap(),
            digest(&state.pending_coverage).unwrap()
        );
        let mut replay = before;
        assert_eq!(
            bytes,
            agent::request(&input(), &config, &mut replay)
                .await
                .unwrap()
        );
        // Increasing output reservation can make even the last group impossible.
        config.provider.max_tokens =
            (config.limits.max_context_tokens - config.limits.token_safety_margin - base_bytes + 1)
                as u32;
        assert_eq!(
            agent::request(&input(), &config, &mut state)
                .await
                .unwrap_err()
                .code,
            "AGENT_TURN_BUDGET_EXCEEDED"
        );
        assert!(
            state.transcript.last().unwrap()["content"]
                .as_str()
                .unwrap()
                .contains("range-7:")
        );
        assert!(state.pending_coverage.is_some());
    }
}

#[tokio::test]
async fn both_extraction_roles_send_only_frozen_optional_reasoning() {
    for effort in [None, Some("high")] {
        let mut config = config();
        config.provider.reasoning_effort = effort.map(String::from);
        config.provider.max_tokens = 4096;
        config.provider.timeout_ms = 90000;
        let model = script();
        let journal = MemoryJournal::default();
        agent::run(
            &input(),
            &config,
            &journal,
            &model,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        let bodies = model.bodies.lock().unwrap();
        assert!(
            bodies
                .iter()
                .any(|b| b["messages"][0]["content"] != bodies[0]["messages"][0]["content"])
        );
        for body in bodies.iter() {
            assert_eq!(body["max_tokens"], 4096);
            assert_eq!(body.get("reasoning_effort").and_then(Value::as_str), effort);
            assert_eq!(body.get("reasoning_effort").is_some(), effort.is_some());
        }
    }
}

#[tokio::test]
async fn bounded_history_keeps_latest_unseen_group_and_replay_bytes_for_both_roles() {
    let mut input = input();
    let chunks: Vec<_> = (0..12)
        .map(|i| format!("range-{i:02}:中文😀证据。").repeat(16))
        .collect();
    input.source_units[0].text = chunks.concat();
    let mut calls = vec![("set_work_note", active_work("source"))];
    let mut start = 0;
    for chunk in &chunks {
        calls.push((
            "read_source",
            json!({"source_id":"source","start":start,"max_bytes":chunk.len()}),
        ));
        start += chunk.len();
    }
    let mut config = config();
    config.limits.max_history_bytes = 256;
    let model = work_script(calls);
    let journal = MemoryJournal::default();
    *journal.interrupt_after.lock().unwrap() = Some(13);
    agent::run(&input, &config, &journal, &model, &CancellationToken::new())
        .await
        .unwrap_err();
    let saved = journal.load().await.unwrap().unwrap();
    {
        let bodies = model.bodies.lock().unwrap();
        assert!(bodies.last().unwrap().to_string().contains("range-10:"));
        assert!(!bodies.last().unwrap().to_string().contains("range-00:"));
        let stable = &bodies[0]["messages"][1];
        assert!(
            bodies.iter().all(|b| &b["messages"][1] == stable),
            "dynamic progress must follow the stable prefix"
        );
    }
    for role in [Role::Main, Role::Reviewer] {
        let mut state = saved.clone();
        state.role = role;
        state.source_review = Some(source_review::initialize(&input, &config).unwrap());
        state.reviewer_work = state.main_work.clone();
        state.reviewer_coverage = state.analysis.coverage.clone();
        let before = state.clone();
        let bytes = agent::request(&input, &config, &mut state).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(
            body.to_string().contains("range-11:"),
            "latest undelivered result cannot be dropped"
        );
        assert!(!body.to_string().contains("range-00:"));
        assert_eq!(
            digest(&state.pending_coverage).unwrap(),
            digest(&before.pending_coverage).unwrap()
        );
        assert_eq!(state.turn, before.turn);
        assert_eq!(state.read_bytes, before.read_bytes);
        let mut replay: Checkpoint =
            serde_json::from_value(serde_json::to_value(&before).unwrap()).unwrap();
        assert_eq!(
            bytes,
            agent::request(&input, &config, &mut replay).await.unwrap()
        );
        let messages = body["messages"].as_array().unwrap();
        let calls: Vec<_> = messages
            .iter()
            .filter_map(|m| m["tool_calls"].as_array())
            .flatten()
            .map(|c| c["id"].clone())
            .collect();
        let results: Vec<_> = messages
            .iter()
            .filter(|m| m["role"] == "tool")
            .map(|m| m["tool_call_id"].clone())
            .collect();
        assert_eq!(calls, results, "trim only complete protocol groups");
    }
}

#[tokio::test]
#[ignore = "requires explicit frozen input, recorded requests, report path and configured runtime; no model calls"]
async fn replay_recorded_requests_with_bounded_history() {
    use std::{fs, path::PathBuf};
    let path = |key| PathBuf::from(std::env::var(key).expect("explicit replay path required"));
    let input: FrozenInput =
        serde_json::from_slice(&fs::read(path("KB_TENDER_REPLAY_INPUT")).unwrap()).unwrap();
    tools::validate_input(&input).unwrap();
    let config = Config::from_environment().unwrap();
    let mut requests: Vec<_> = fs::read_dir(path("KB_TENDER_REPLAY_REQUESTS"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter_map(|file| {
            let name = file.file_name()?.to_str()?;
            let turn = name
                .strip_prefix("turn-")?
                .split('-')
                .next()?
                .parse::<usize>()
                .ok()?;
            name.ends_with(".json").then_some((turn, file))
        })
        .collect();
    requests.sort_by_key(|(turn, _)| *turn);
    assert!(requests.len() > 1, "a trajectory needs multiple requests");
    let mut report = Vec::new();
    for (turn, file) in requests {
        let original = fs::read(&file).unwrap();
        let recorded: Value = serde_json::from_slice(&original).unwrap();
        let messages = recorded["messages"].as_array().unwrap();
        let first = messages.iter().position(|m| m["role"] == "assistant");
        let transcript = first.map_or_else(Vec::new, |i| messages[i..].to_vec());
        for role in [Role::Main, Role::Reviewer] {
            // Project historical protocol groups into the new request builder.
            // This is not a resumed old contract or a semantic extraction run.
            let mut state = Checkpoint {
                journal: Default::default(),
                input_sha256: digest(&input).unwrap(),
                config_sha256: digest(&config).unwrap(),
                turn,
                tool_calls: 0,
                read_bytes: 0,
                review_rounds: 0,
                role: role.clone(),
                analysis: Analysis::default(),
                review: None,
                review_draft: BTreeMap::new(),
                source_review: None,
                repair: Default::default(),
                dispatch: Default::default(),
                reviewer_coverage: Coverage::default(),
                pending_coverage: None,
                transcript: transcript.clone(),
                main_progress: Default::default(),
                reviewer_progress: Default::default(),
                main_work: None,
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
            let before = state.clone();
            let bytes = agent::request(&input, &config, &mut state).await.unwrap();
            assert!(bytes.len() <= config.limits.max_context_bytes);
            assert_eq!(
                digest(&state.analysis).unwrap(),
                digest(&before.analysis).unwrap()
            );
            assert!(state.pending_coverage.is_none());
            let mut replay = before.clone();
            assert_eq!(
                bytes,
                agent::request(&input, &config, &mut replay).await.unwrap()
            );
            let body: Value = serde_json::from_slice(&bytes).unwrap();
            let sent = body["messages"].as_array().unwrap();
            if let Some(last) = transcript.iter().rposition(|m| m["role"] == "assistant") {
                let latest = &transcript[last..];
                assert_eq!(
                    &sent[sent.len() - 1 - latest.len()..sent.len() - 1],
                    latest,
                    "the latest pending group must survive unchanged"
                );
            }
            let call_ids: Vec<_> = sent
                .iter()
                .filter_map(|m| m["tool_calls"].as_array())
                .flatten()
                .map(|c| c["id"].clone())
                .collect();
            let result_ids: Vec<_> = sent
                .iter()
                .filter(|m| m["role"] == "tool")
                .map(|m| m["tool_call_id"].clone())
                .collect();
            assert_eq!(
                call_ids, result_ids,
                "no orphan tool results after trimming"
            );
            report.push(
                json!({"turn":turn,"role":role,"original_bytes":original.len(),
                "bounded_bytes":bytes.len(),"original_messages":transcript.len(),
                "retained_messages":state.transcript.len()}),
            );
        }
    }
    assert!(
        report
            .iter()
            .any(|r| r["retained_messages"].as_u64() < r["original_messages"].as_u64())
    );
    fs::write(
        path("KB_TENDER_REPLAY_REPORT"),
        serde_json::to_vec_pretty(&json!({
            "input_sha256":digest(&input).unwrap(),"config_sha256":digest(&config).unwrap(),
            "scope":"recorded protocol projection only; no model or semantic acceptance",
            "requests":report
        }))
        .unwrap(),
    )
    .unwrap();
}

#[tokio::test]
async fn global_budget_exhaustion_retains_partial_result_without_approval() {
    let journal = MemoryJournal::default();
    let model = script();
    let mut config = config();
    config.limits.max_turns = 2;
    let error = agent::run(
        &input(),
        &config,
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "AGENT_TURN_BUDGET_EXCEEDED");
    let state = journal.state.lock().unwrap();
    let state = state.as_ref().unwrap();
    assert_eq!(state.turn, 2);
    assert!(!state.done);
    assert!(state.review.is_none());
}

#[tokio::test]
async fn review_replan_compacts_redundant_history_before_the_budget_is_full() {
    let config = config();
    let journal = fresh_review_journal().await;
    let mut state = journal.load().await.unwrap().unwrap();
    let record = state.analysis.records.values().next().unwrap().clone();
    let reference = format!("record:{}", record.id);
    state.reviewer_work.as_mut().unwrap().focus.references = vec![reference.clone()];
    state
        .reviewer_coverage
        .candidate
        .insert(reference.clone(), digest(&record).unwrap());
    let text = input().source_units[0].text.clone();
    tools::cover(
        state
            .reviewer_coverage
            .text
            .entry("source".into())
            .or_default(),
        0,
        text.len(),
    );
    let group = |id: &str, name: &str, result: serde_json::Value| {
        vec![
            json!({"role":"assistant","content":"I will inventory the remaining items before comparing them.","tool_calls":[{"id":id,"type":"function","function":{"name":name,"arguments":"{}"}}]}),
            json!({"role":"tool","tool_call_id":id,"content":json!({"ok":true,"result":result}).to_string()}),
        ]
    };
    let latest = group(
        "pending",
        "check_gaps",
        json!({"items":[],"next":0,"total":0}),
    );
    state.transcript = [
        group(
            "old-index",
            "source_index",
            json!({"items":[{"id":"source"}]}),
        ),
        group(
            "original",
            "read_source",
            json!({"source_id":"source","start":0,"end":text.len(),"text":text}),
        ),
        group(
            "candidate",
            "inspect_analysis",
            json!({"view":"detail","items":[record]}),
        ),
        latest.clone(),
    ]
    .concat();
    let normal: serde_json::Value =
        serde_json::from_slice(&agent::request(&input(), &config, &mut state).await.unwrap())
            .unwrap();
    assert!(
        normal["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["tool_call_id"] == "old-index")
    );
    let original = normal["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["tool_call_id"] == "original")
        .unwrap()
        .clone();
    state.reviewer_progress.watch.recovery = crate::agent_runtime::progress::Recovery::Replan;
    state.reviewer_progress.watch.replans = 1;
    let mut newly_read = state.clone();
    newly_read.reviewer_progress.watch.recovery = crate::agent_runtime::progress::Recovery::Running;
    let before = json!([
        state.analysis,
        state.reviewer_coverage,
        state.main_progress,
        state.reviewer_progress,
        state.pending_coverage,
        state.journal,
        state.turn,
        state.tool_calls,
        state.read_bytes
    ]);
    let recovered: serde_json::Value =
        serde_json::from_slice(&agent::request(&input(), &config, &mut state).await.unwrap())
            .unwrap();
    let messages = recovered["messages"].as_array().unwrap();
    assert!(!messages.iter().any(|m| m["tool_call_id"] == "old-index"));
    assert!(messages.contains(&original));
    assert!(state.transcript.ends_with(&latest));
    assert!(
        messages
            .iter()
            .any(|m| m["content"].as_str().is_some_and(|content| {
                serde_json::from_str::<serde_json::Value>(content)
                    .ok()
                    .is_some_and(|value| {
                        value["retained_candidate_details"]["items"]
                            .as_array()
                            .is_some_and(|items| {
                                items.iter().any(|item| {
                                    item["reference"] == reference && item["value"] == json!(record)
                                })
                            })
                    })
            }))
    );
    assert_eq!(
        json!([
            state.analysis,
            state.reviewer_coverage,
            state.main_progress,
            state.reviewer_progress,
            state.pending_coverage,
            state.journal,
            state.turn,
            state.tool_calls,
            state.read_bytes
        ]),
        before
    );
    let packet: serde_json::Value =
        serde_json::from_str(messages.last().unwrap()["content"].as_str().unwrap()).unwrap();
    assert!(
        packet["execution"]["next_action"]
            .as_str()
            .unwrap()
            .contains("single unfinished")
    );
    let stable = json!(state);
    agent::request(&input(), &config, &mut state).await.unwrap();
    assert_eq!(
        json!(state),
        stable,
        "repeated construction must not spend or reset recovery state"
    );
    let watch = json!(newly_read.reviewer_progress.watch);
    let renewed: Value = serde_json::from_slice(
        &agent::request(&input(), &config, &mut newly_read)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(
        !renewed["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["tool_call_id"] == "old-index")
    );
    assert_eq!(json!(newly_read.reviewer_progress.watch), watch);
}

#[test]
fn omitted_progress_limits_freeze_central_defaults() {
    let config = config();
    let mut value = json!(config.limits);
    for key in [
        "max_no_progress_turns",
        "max_focus_turns",
        "max_focus_replans",
    ] {
        value.as_object_mut().unwrap().remove(key);
    }
    let limits: Limits = serde_json::from_value(value).unwrap();
    assert_eq!(limits.max_no_progress_turns, 6);
    assert_eq!(limits.max_focus_turns, 24);
    assert_eq!(limits.max_focus_replans, 2);
    let frozen = Config::with_provider(config.provider, limits).unwrap();
    assert!(json!(frozen)["limits"]["max_no_progress_turns"].is_number());
}
