// Production PackHost lifecycle, synthetic requests/responses only. No HTTP.
fn auto_support_fixture(supports: usize) -> (FrozenInput, Config, Durable) {
    use crate::analysis::source_manifest::{
        DocumentRelation, DocumentRelationKind, DocumentRelationStatus, RelationEndpoint,
    };
    let (mut input, config, mut journal) = fixture(supports + 1);
    input.document_relations = (1..=supports)
        .map(|index| DocumentRelation {
            id: format!("synthetic-support-{index}"),
            from: RelationEndpoint {
                document_id: "synthetic-0".into(),
                unit_id: Some("section".into()),
            },
            to: Some(RelationEndpoint {
                document_id: format!("synthetic-{index}"),
                unit_id: Some("section".into()),
            }),
            kind: DocumentRelationKind::ExplicitReference,
            status: DocumentRelationStatus::Confirmed,
            required: true,
            basis: "Synthetic source sentence for transport admission testing.".into(),
            locator: input.source_units[0].locator.clone(),
        })
        .collect();
    let mut state = super::super::retirement::checkpoint(&input);
    state.turn = 0;
    state.config_sha256 = digest(&config).unwrap();
    state.outline_run.reading_packs = Some(
        crate::outline::discover::DiscoverWork::plan_with_budget(&input, &|sessions| {
            Ok(sessions
                .iter()
                .all(|session| session["pack"]["atoms"].as_array().unwrap().len() == 1))
        }),
    );
    assert_eq!(
        state
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .required_support_refs(&input, "pack-0")
            .unwrap()
            .len(),
        supports
    );
    journal.state = Mutex::new(state);
    journal.stop_barrier = false;
    (input, config, journal)
}
fn automatic_pages(request: &Request) -> Vec<Value> {
    let wire: Value = serde_json::from_slice(&request.body).unwrap();
    wire["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "user")
        .filter_map(|message| message["content"].as_str())
        .filter_map(|text| serde_json::from_str::<Value>(text).ok())
        .filter(|value| value.get("automatic_support_page").is_some())
        .collect()
}

#[tokio::test]
async fn automatic_user_page_confirms_before_bad_json_and_received_replay_is_idempotent() {
    let (input, config, journal) = auto_support_fixture(1);
    let mut state = journal.load().await.unwrap().unwrap();
    let cancel = CancellationToken::new();
    let mut host = PackHost {
        input: &input,
        config: &config,
        state: &mut state,
        journal: &journal,
        cancel: &cancel,
    };
    let request = host.next(&BTreeSet::new()).await.unwrap().unwrap();
    assert_eq!(automatic_pages(&request).len(), 1);
    assert!(
        host.state.outline_run.discover_workers["pack-0"]
            .delivered_related_evidence
            .is_empty()
    );
    assert!(
        host.state.outline_run.discover_workers["pack-0"].related_read_frames[0]
            .user_page
            .is_some()
    );
    let mut response = Delayed {
        delay_ms: 0,
        ..Default::default()
    }
    .turn(&config, &request.body)
    .await
    .unwrap();
    response.tool_calls[0].arguments = "{bad-json".into();
    host.reserve(&request).await.unwrap();
    host.received(&request, &response).await.unwrap();
    // Reload durable Received before applying any new operation.
    let mut restored = journal.load().await.unwrap().unwrap();
    let mut host = PackHost {
        input: &input,
        config: &config,
        state: &mut restored,
        journal: &journal,
        cancel: &cancel,
    };
    host.replay_received().await.unwrap();
    assert!(
        host.state
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .missing_required_support(&input, "pack-0")
            .unwrap()
            .is_empty()
    );
    assert!(
        matches!(&host.state.outline_run.discover_workers["pack-0"].pending, Some(Pending::Failed { code, .. }) if code == "BUSINESS_REJECTED")
    );
    let snapshot = json!(host.state);
    host.replay_received().await.unwrap();
    assert_eq!(json!(host.state), snapshot);
    let next = host.next(&BTreeSet::new()).await.unwrap().unwrap();
    assert!(
        automatic_pages(&next).is_empty(),
        "confirmed user payload is retired only from the request"
    );
    assert!(
        host.state.outline_run.discover_workers["pack-0"]
            .history
            .iter()
            .any(|message| message["content"]
                .as_str()
                .is_some_and(|text| text.contains("automatic_support_page"))),
        "raw page audit survives"
    );
}

#[tokio::test]
async fn automatic_nine_pages_then_cross_claim_redelivery_preserves_business_failure_memory() {
    let (input, config, journal) = auto_support_fixture(9);
    let mut state = journal.load().await.unwrap().unwrap();
    let cancel = CancellationToken::new();
    let mut failures = 0;
    let mut redelivery = false;
    let mut first_signature = BTreeSet::new();
    for ordinal in 0..40 {
        let mut host = PackHost {
            input: &input,
            config: &config,
            state: &mut state,
            journal: &journal,
            cancel: &cancel,
        };
        let request = host.next(&BTreeSet::new()).await.unwrap().unwrap();
        assert_eq!(request.scope.pack, "pack-0");
        let missing = host
            .state
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .missing_required_support(&input, "pack-0")
            .unwrap();
        let last_page = missing.len() <= 1;
        let mut response = if last_page {
            let mut response = stall_submission(&config, &request, &[false]).await;
            if failures == 1 {
                let mut args: Value =
                    serde_json::from_str(&response.tool_calls[0].arguments).unwrap();
                args["requirements"][0]["description"] = json!("Distinct synthetic correction");
                args["requirements"][0]["evidence"] = json!([]);
                response.tool_calls[0].arguments = args.to_string();
            }
            response
        } else {
            Delayed {
                delay_ms: 0,
                ..Default::default()
            }
            .turn(&config, &request.body)
            .await
            .unwrap()
        };
        response.tool_calls[0].id = format!("auto-step-{ordinal}");
        host.reserve(&request).await.unwrap();
        host.received(&request, &response).await.unwrap();
        host.commit(&request, response).await.unwrap();
        let worker = &host.state.outline_run.discover_workers["pack-0"];
        if last_page {
            failures += 1;
            if failures == 1 {
                first_signature = worker.failure_signatures.clone();
            }
            if failures == 2 {
                redelivery = true;
            }
            if failures == 3 {
                assert!(redelivery);
                assert!(
                    matches!(&worker.pending, Some(Pending::Failed { code, .. }) if code == "DISCOVER_NO_STATE_PROGRESS")
                );
                assert!(worker.rejected_turns > 6);
                return;
            }
        } else if redelivery {
            assert!(
                first_signature.is_subset(&worker.failure_signatures),
                "same-source reauthorization cannot erase prior business failure"
            );
        }
        assert!(
            !matches!(&worker.pending, Some(Pending::Failed { code, .. }) if code == "DISCOVER_NO_STATE_PROGRESS"),
            "necessary current-claim pages cannot be stopped by prior business errors"
        );
        state = journal.load().await.unwrap().unwrap();
    }
    panic!("synthetic auto delivery did not reach the repeated business failure");
}

#[tokio::test]
async fn malformed_json_fingerprints_and_trim_empty_failures_do_not_fake_progress() {
    for whitespace in [false, true] {
        let (input, config, mut journal) = fixture(1);
        journal.stop_barrier = false;
        let mut state = journal.load().await.unwrap().unwrap();
        let cancel = CancellationToken::new();
        for step in 0..3 {
            let mut host = PackHost {
                input: &input,
                config: &config,
                state: &mut state,
                journal: &journal,
                cancel: &cancel,
            };
            let request = host.next(&BTreeSet::new()).await.unwrap().unwrap();
            let mut response = stall_submission(&config, &request, &[false]).await;
            if whitespace {
                let mut args: Value =
                    serde_json::from_str(&response.tool_calls[0].arguments).unwrap();
                args["requirements"][0]["description"] =
                    json!(if step == 0 { " " } else { " \t\n " });
                response.tool_calls[0].arguments = args.to_string();
            } else {
                response.tool_calls[0].arguments =
                    if step == 1 { "{broken-B" } else { "{broken-A" }.into();
            }
            host.reserve(&request).await.unwrap();
            host.received(&request, &response).await.unwrap();
            host.commit(&request, response).await.unwrap();
            let terminal = matches!(&host.state.outline_run.discover_workers["pack-0"].pending, Some(Pending::Failed { code, .. }) if code == "DISCOVER_NO_STATE_PROGRESS");
            assert_eq!(terminal, if whitespace { step == 1 } else { step == 2 });
            if terminal {
                break;
            }
            state = journal.load().await.unwrap().unwrap();
        }
    }
}

#[tokio::test]
async fn queued_automatic_page_cancel_does_not_clear_business_memory_or_confirm_source() {
    let (input, config, journal) = auto_support_fixture(1);
    let mut state = journal.load().await.unwrap().unwrap();
    state.outline_run.discover_workers.insert(
        "pack-0".into(),
        Worker {
            failure_signatures: BTreeSet::from(["prior-business-failure".into()]),
            ..Default::default()
        },
    );
    let cancel = CancellationToken::new();
    let mut host = PackHost {
        input: &input,
        config: &config,
        state: &mut state,
        journal: &journal,
        cancel: &cancel,
    };
    let request = host.next(&BTreeSet::new()).await.unwrap().unwrap();
    assert_eq!(automatic_pages(&request).len(), 1);
    cancel.cancel();
    assert_eq!(
        host.reserve(&request).await.unwrap_err().code,
        "AGENT_CANCELLED"
    );
    host.cancelled(&BTreeSet::new()).await.unwrap();
    let saved = journal.load().await.unwrap().unwrap();
    let worker = &saved.outline_run.discover_workers["pack-0"];
    assert!(worker.failure_signatures.contains("prior-business-failure"));
    assert!(worker.progress_evidence.is_empty());
    assert!(worker.delivered_related_evidence.is_empty());
    assert_eq!(saved.journal.accounting.physical_calls, 0);
    assert!(
        !saved
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .missing_required_support(&input, "pack-0")
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn automatic_local_capacity_failure_keeps_independent_worker_runnable() {
    let (input, config, mut journal) = fixture(2);
    journal.stop_barrier = false;
    let mut state = journal.load().await.unwrap().unwrap();
    state.outline_run.discover_workers.insert("pack-0".into(), Worker {
        history: vec![json!({"role":"assistant", "content":"unique synthetic capacity payload ".repeat(100_000)})],
        ..Default::default()
    });
    let cancel = CancellationToken::new();
    let mut host = PackHost {
        input: &input,
        config: &config,
        state: &mut state,
        journal: &journal,
        cancel: &cancel,
    };
    let request = host.next(&BTreeSet::new()).await.unwrap().unwrap();
    assert_eq!(request.scope.pack, "pack-1");
    assert!(
        matches!(&host.state.outline_run.discover_workers["pack-0"].pending,
        Some(Pending::Failed { code, .. }) if code == "DISCOVER_READ_CAPACITY_EXCEEDED")
    );
    assert_eq!(host.state.journal.accounting.physical_calls, 0);
    let response = Delayed {
        delay_ms: 0,
        ..Default::default()
    }
    .turn(&config, &request.body)
    .await
    .unwrap();
    host.reserve(&request).await.unwrap();
    host.received(&request, &response).await.unwrap();
    host.commit(&request, response).await.unwrap();
    assert!(!host.all_committed());
    assert_eq!(
        host.state
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .pack_counts()
            .committed,
        1
    );
    let mut restored = journal.load().await.unwrap().unwrap();
    let mut host = PackHost {
        input: &input,
        config: &config,
        state: &mut restored,
        journal: &journal,
        cancel: &cancel,
    };
    assert!(host.next(&BTreeSet::new()).await.unwrap().is_none());
    assert_eq!(host.state.journal.accounting.physical_calls, 1);
    crate::outline::agent::synchronize_discovery(&input, host.state).unwrap();
    assert_eq!(
        crate::outline::agent::current(&input, host.state),
        crate::outline::agent::Duty::Check
    );
    let pack = "pack-1".to_string();
    let scope = host.state.outline_run.tool_draft.source_scopes[&pack].clone();
    let version = crate::outline::source_review::version(
        &input,
        &host.state.outline_run.tool_draft,
        &pack,
        &scope,
    )
    .unwrap();
    // Explicit receipt seam; actual Check submission/gates execute below.
    // This domain test does not claim a model saw a transport read response.
    host.state.outline_run.tool_draft.check_reads.evidence = scope.evidence.clone();
    host.state.outline_run.tool_draft.check_reads.structure_keys = scope.structure_keys.clone();
    let refs = scope
        .evidence
        .iter()
        .map(|reference| {
            json!({"review_evidence_key":
        crate::outline::agent::review_evidence_key(host.state, reference).unwrap()})
        })
        .collect::<Vec<_>>();
    let args = json!({"requirement_ids":[],"pack_ids":[pack],"inspected_evidence":refs,
        "issues":[],"template_reviews":[],"source_dispositions":[{"pack_id":pack,
        "version":version,"evidence":refs,"verdict":"no_response_obligation",
        "requirement_ids":[],"reason":"Synthetic protocol-only scope independently checked."}]});
    crate::outline::agent::apply_for_duty(
        &input,
        host.state,
        "submit_review",
        &args,
        crate::outline::agent::Duty::Check,
    )
    .unwrap();
    assert!(
        crate::outline::source_review::complete(&input, &host.state.outline_run.tool_draft, &pack)
            .unwrap()
    );
    host.save().await.unwrap();
    let reviewed = journal.load().await.unwrap().unwrap();
    assert!(
        crate::outline::source_review::complete(&input, &reviewed.outline_run.tool_draft, &pack)
            .unwrap()
    );
    assert!(!reviewed.done);
    assert!(
        !reviewed
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .complete()
    );
}

#[test]
fn automatic_header_projection_keeps_owned_text_and_removes_duplicate_support_text() {
    let mut session = json!({"pack":{"atoms":[{
        "cells":[{"anchor_row":1,"anchor_column":0,"start_byte":0,"end_byte":4}],
        "header_context":[{"anchor_row":0,"anchor_column":0,"start_byte":0,"end_byte":6}],
        "table_structure":{"anchors":{
            "0,0":{"header_role":"column_header","header_fragments":{"0,6":"HEADER"}},
            "1,0":{"header_role":"row_header","header_fragments":{"0,4":"BODY"}}
        }}
    }]}});
    let mut cross_atom = session.clone();
    cross_atom["pack"]["atoms"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "cells":[{"anchor_row":0,"anchor_column":0,"start_byte":0,"end_byte":6}],
            "table_structure":{"anchors":{"0,0":{"header_fragments":{"0,6":"HEADER"}}}}
        }));
    project_owned_pack_headers(&mut cross_atom);
    assert_eq!(
        cross_atom.to_string().matches("HEADER").count(),
        1,
        "context copy is removed while another atom retains primary header ownership"
    );
    project_owned_pack_headers(&mut session);
    let wire = json!({"messages":[{"role":"user","content":session.to_string()},
        {"role":"user","content":json!({"automatic_support_page":{"read":"HEADER"}}).to_string()}]});
    assert_eq!(wire.to_string().matches("HEADER").count(), 1);
    assert_eq!(wire.to_string().matches("BODY").count(), 1);
    assert_eq!(
        session["pack"]["atoms"][0]["table_structure"]["anchors"]["0,0"]["header_role"],
        "column_header"
    );
    assert!(session["pack"]["atoms"][0].get("header_context").is_none());
}

#[tokio::test]
async fn automatic_grid_header_exact_wire_once_and_no_credit_before_received() {
    let (mut input, config, mut journal) = fixture(1);
    journal.stop_barrier = false;
    input.source_units[0].text.clear();
    input.structured_forms = vec![
        json!({"source_unit_revision_id":input.source_units[0].source_unit_revision_id,
        "form_definition_revision_id":"synthetic-table","definition":{
        "row_count":2,"column_count":1,"cells":[
            {"row":0,"column":0,"text":"UNIQUE_SYNTHETIC_HEADER","header_role":"column_header"},
            {"row":1,"column":0,"text":"Synthetic response body","header_role":"none"}
        ]}}),
    ];
    let mut state = super::super::retirement::checkpoint(&input);
    state.turn = 0;
    state.config_sha256 = digest(&config).unwrap();
    let mut work = crate::outline::discover::DiscoverWork::plan_with_budget(&input, &|sessions| {
        Ok(sessions.iter().all(|session| {
            session["pack"]["atoms"]
                .as_array()
                .unwrap()
                .iter()
                .map(|atom| atom["cells"].as_array().map_or(0, Vec::len))
                .sum::<usize>()
                <= 1
        }))
    });
    assert!(work.planning_error().is_none());
    work.claim(usize::MAX);
    let pack = work
        .pack_ids()
        .into_iter()
        .find(|id| !work.required_support_refs(&input, id).unwrap().is_empty())
        .expect("body pack requires header");
    state.outline_run.reading_packs = Some(work);
    state
        .outline_run
        .discover_workers
        .insert(pack.clone(), Worker::default());
    let cancel = CancellationToken::new();
    let mut host = PackHost {
        input: &input,
        config: &config,
        state: &mut state,
        journal: &journal,
        cancel: &cancel,
    };
    let request = host.prepare(&pack).await.unwrap();
    assert_eq!(
        String::from_utf8_lossy(&request.body)
            .matches("UNIQUE_SYNTHETIC_HEADER")
            .count(),
        1
    );
    assert_eq!(automatic_pages(&request).len(), 1);
    assert!(
        host.state.outline_run.discover_workers[&pack]
            .delivered_related_evidence
            .is_empty()
    );
    assert!(
        !host
            .state
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .missing_required_support(&input, &pack)
            .unwrap()
            .is_empty()
    );
    let mut response = Delayed {
        delay_ms: 0,
        ..Default::default()
    }
    .turn(&config, &request.body)
    .await
    .unwrap();
    response.tool_calls[0].arguments = "{invalid-action".into();
    host.reserve(&request).await.unwrap();
    assert!(
        host.state.outline_run.discover_workers[&pack]
            .delivered_related_evidence
            .is_empty()
    );
    host.received(&request, &response).await.unwrap();
    host.commit(&request, response).await.unwrap();
    assert!(
        host.state
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .missing_required_support(&input, &pack)
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn automatic_user_page_response_boundary_and_wire_tamper() {
    for case in [
        "stop",
        "no_tool",
        "length",
        "old_scope",
        "unknown_tool",
        "multiple_tools",
        "tampered_wire",
    ] {
        let (input, config, journal) = auto_support_fixture(1);
        let mut state = journal.load().await.unwrap().unwrap();
        let cancel = CancellationToken::new();
        let mut host = PackHost {
            input: &input,
            config: &config,
            state: &mut state,
            journal: &journal,
            cancel: &cancel,
        };
        let mut request = host.next(&BTreeSet::new()).await.unwrap().unwrap();
        let mut response = Delayed {
            delay_ms: 0,
            ..Default::default()
        }
        .turn(&config, &request.body)
        .await
        .unwrap();
        match case {
            "stop" => response.finish_reason = "stop".into(),
            "length" => response.finish_reason = "length".into(),
            "no_tool" => response.tool_calls.clear(),
            "unknown_tool" => response.tool_calls[0].name = "unavailable_synthetic_tool".into(),
            "multiple_tools" => {
                let mut second = response.tool_calls[0].clone();
                second.id = "distinct-complete-call".into();
                response.tool_calls.push(second);
            }
            "old_scope" => {
                let mut args: Value =
                    serde_json::from_str(&response.tool_calls[0].arguments).unwrap();
                args["wire_scope"] = json!("obsolete-scope");
                response.tool_calls[0].arguments = args.to_string();
            }
            "tampered_wire" => {
                let mut body: Value = serde_json::from_slice(&request.body).unwrap();
                body["messages"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"role":"user","content":"synthetic tamper"}));
                request.body = crate::agent_runtime::canonical_request_bytes(&body).unwrap();
                // Exercise receipt seal independently of the journal equality
                // gate: even a persisted request cannot authorize altered bytes.
                host.state
                    .outline_run
                    .discover_workers
                    .get_mut("pack-0")
                    .unwrap()
                    .pending = Some(Pending::Prepared {
                    request: request.clone(),
                });
            }
            _ => unreachable!(),
        }
        host.reserve(&request).await.unwrap();
        host.received(&request, &response).await.unwrap();
        host.commit(&request, response).await.unwrap();
        let credited = host
            .state
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .missing_required_support(&input, "pack-0")
            .unwrap()
            .is_empty();
        assert_eq!(
            credited,
            matches!(case, "old_scope" | "unknown_tool" | "multiple_tools"),
            "{case}"
        );
        assert!(
            !host.all_committed(),
            "{case}: rejected action cannot commit"
        );
        assert!(matches!(
            &host.state.outline_run.discover_workers["pack-0"].pending,
            Some(Pending::Failed { .. })
        ));
    }
}

#[test]
fn assistant_json_observation_with_host_page_field_is_not_retired() {
    let observation = json!({"role":"assistant","content":json!({"automatic_support_page":{"id":"observation-reference"},"observation":"Unique synthetic reading note"}).to_string()});
    let page = json!({"role":"user","content":json!({"automatic_support_page":{"id":"delivered-host-page"}}).to_string()});
    let worker = Worker {
        history: vec![page, observation.clone()],
        ..Default::default()
    };
    assert_eq!(
        project_worker_request_history(&worker),
        vec![observation.clone()]
    );
    assert_eq!(
        worker.history.len(),
        2,
        "projection cannot erase the raw audit"
    );
}
