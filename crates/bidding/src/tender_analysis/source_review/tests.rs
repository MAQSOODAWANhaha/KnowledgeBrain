use super::*;
use crate::agent_error::AgentError;
use crate::agent_runtime::progress::Progress;
use crate::tender_analysis::agent::{
    self, Journal, apply, apply_in_batch, execute_turn, finish_review_batch, request,
    review_complete,
};
use async_trait::async_trait;
use knowledge::models::ChatTurn;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

#[test]
fn source_task_neighbors_include_completed_ranges_without_crossing_documents_or_granting_reads() {
    let (mut input, config, mut state) = fixture();
    for (id, document) in [("next", "document"), ("other", "other-document")] {
        input.source_units.push(Source {
            source_unit_revision_id: id.into(),
            document_id: document.into(),
            text: "An unread original paragraph.".into(),
            locator: json!({}),
            ordinal: input.source_units.len(),
        });
    }
    state.source_review = Some(initialize(&input, &config).unwrap());
    select_next(&input, &config, &mut state).unwrap();
    let before = digest(&state).unwrap();
    let first = packet(&input, &config, &state).unwrap();
    assert!(first["current"]["neighbors"]["previous"].is_null());
    assert_eq!(first["current"]["neighbors"]["next"]["source_id"], "next");
    assert_eq!(
        first["current"]["neighbors"]["next"]["region"],
        json!({"kind":"text","start":0,"end":input.source_units[1].text.len()})
    );
    assert_eq!(digest(&state).unwrap(), before);
    let args = judgment(&input, &config, &state);
    put(&input, &config, &mut state, &args).unwrap();
    select_next(&input, &config, &mut state).unwrap();
    let before = digest(&state).unwrap();
    let next = packet(&input, &config, &state).unwrap();
    assert_eq!(next["current"]["task"]["source_id"], "next");
    assert_eq!(
        next["current"]["neighbors"]["previous"]["id"],
        first["current"]["task"]["id"]
    );
    assert!(next["current"]["neighbors"]["next"].is_null());
    assert_eq!(digest(&state).unwrap(), before);
    assert!(!state.reviewer_coverage.text.contains_key("next"));
    assert!(state.pending_coverage.is_none());
}

#[test]
fn reviewer_relationship_vocabulary_is_derived_from_the_writing_tool_without_write_access() {
    let (input, config, state) = fixture();
    let before = digest(&state).unwrap();
    let navigation = packet(&input, &config, &state).unwrap();
    let tools = tools::schemas(false);
    let writer = tools
        .iter()
        .find(|tool| tool["function"]["name"] == "put_relation")
        .unwrap();
    assert_eq!(
        navigation["relation_kind"],
        writer["function"]["parameters"]["properties"]["kind"]
    );
    assert!(
        navigation["relation_kind"]["enum"]
            .as_array()
            .unwrap()
            .contains(&json!("references"))
    );
    assert!(
        !navigation["relation_kind"]["enum"]
            .as_array()
            .unwrap()
            .contains(&json!("depends_on"))
    );
    assert!(
        !tools::schemas(true)
            .iter()
            .any(|tool| tool["function"]["name"] == "put_relation")
    );
    assert_eq!(digest(&state).unwrap(), before);
}

#[tokio::test]
#[ignore = "requires KB_TENDER_CONTEXT_REPLAY_DIR and KB_TENDER_CONTEXT_REPORT; offline only"]
async fn archived_source_review_costs_are_reported() {
    let root = std::path::PathBuf::from(std::env::var("KB_TENDER_CONTEXT_REPLAY_DIR").unwrap());
    let original = std::fs::read(root.join("extraction/checkpoint.json")).unwrap();
    let state: Checkpoint = serde_json::from_slice(&original).unwrap();
    let input: FrozenInput =
        serde_json::from_slice(&std::fs::read(root.join("source/frozen-input.json")).unwrap())
            .unwrap();
    let config: Config =
        serde_json::from_slice(&std::fs::read(root.join("extraction/runtime.json")).unwrap())
            .unwrap();
    let before = digest(&state).unwrap();
    let mut samples = Vec::new();
    for _ in 0..3 {
        let started = Instant::now();
        let inventory = tasks(&input, config.limits.max_tool_result_bytes).unwrap();
        let inventory_us = started.elapsed().as_micros();
        let started = Instant::now();
        let remaining = pending(&input, &config, &state).unwrap();
        let pending_us = started.elapsed().as_micros();
        let started = Instant::now();
        let packet = packet(&input, &config, &state).unwrap();
        let packet_us = started.elapsed().as_micros();
        let started = Instant::now();
        let evidence = evidence(&input, &config, &state).unwrap();
        let evidence_us = started.elapsed().as_micros();
        let mut projected = state.clone();
        let started = Instant::now();
        let request = agent::prepare_request(&input, &config, &mut projected, false)
            .await
            .unwrap();
        let request_us = started.elapsed().as_micros();
        samples.push(json!({"tasks":inventory.len(),"pending":remaining.len(),"inventory_us":inventory_us,"pending_us":pending_us,"packet_us":packet_us,"evidence_us":evidence_us,
            "request_us":request_us,"request_bytes":request.len(),"request_sha256":digest(&serde_json::from_slice::<Value>(&request).unwrap()).unwrap(),
            "packet_sha256":digest(&packet).unwrap(),"inventory_sha256":digest(&inventory).unwrap(),"pending_sha256":digest(&remaining).unwrap(),
            "evidence_sha256":digest(&evidence.map(|e|json!({"content":e.content,"coverage":e.coverage}))).unwrap()}));
    }
    assert_eq!(digest(&state).unwrap(), before);
    let report = json!({"mode":"offline immutable checkpoint profile, not model execution or semantic acceptance","checkpoint_sha256":digest(&serde_json::from_slice::<Value>(&original).unwrap()).unwrap(),"turn":state.turn,"samples":samples,"note":"Each duration measures a separate call. pending and packet each include inventory work; do not sum these as disjoint production stages."});
    std::fs::write(
        std::env::var("KB_TENDER_CONTEXT_REPORT").unwrap(),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
}

#[test]
fn completed_source_navigation_reopens_a_missing_candidate_comparison_before_aggregation() {
    let (input, config, mut state) = fixture();
    let args = judgment(&input, &config, &state);
    put(&input, &config, &mut state, &args).unwrap();
    assert!(pending(&input, &config, &state).unwrap().is_empty());
    // A historical source judgment may remain current while a local
    // comparison no longer matches the restored dependency version.
    let key = "disposition:source";
    let old_check = digest(&json!([
        "review_check",
        key,
        candidate_version(&state, key).unwrap()
    ]))
    .unwrap();
    assert!(state.reviewer_progress.seen.remove(&old_check));
    let source_receipts = json!(state.source_review.as_ref().unwrap().results);
    let coverage = json!(state.reviewer_coverage);
    let analysis = digest(&state.analysis).unwrap();
    assert!(!agent::review_complete(&input, &config, &state).unwrap());
    select_next(&input, &config, &mut state).unwrap();
    let navigation = packet(&input, &config, &state).unwrap();
    assert_eq!(navigation["remaining"], 1);
    assert_eq!(
        navigation["current"]["pending_candidate_refs"]["items"],
        json!([key])
    );
    assert!(state.source_review.as_ref().unwrap().active_task.is_some());
    assert_eq!(
        json!(state.source_review.as_ref().unwrap().results),
        source_receipts
    );
    assert_eq!(json!(state.reviewer_coverage), coverage);
    assert!(!context::has_review_outcome(&state, key).unwrap());
    compare(&input, &config, &mut state);
    assert!(pending(&input, &config, &state).unwrap().is_empty());
    agent::finish_review_batch(&input, &config, &mut state).unwrap();
    assert!(state.done);
    assert_eq!(state.review_rounds, 1);
    assert_eq!(digest(&state.analysis).unwrap(), analysis);
}

#[test]
fn assigned_candidate_comparison_can_use_delivered_boundary_evidence_without_expanding_work() {
    let (mut input, config, mut state) = fixture();
    for (id, ordinal) in [("next", 1), ("unread", 2)] {
        input.source_units.push(Source {
            source_unit_revision_id: id.into(),
            document_id: "document".into(),
            text: "续接的原文内容。".into(),
            locator: json!({}),
            ordinal,
        });
        state.analysis.dispositions.insert(
            id.into(),
            Disposition {
                state: DispositionState::NonRequirement,
                reason: "Synthetic original text.".into(),
            },
        );
    }
    let next = Span {
        source_id: "next".into(),
        start: 0,
        end: input.source_units[1].text.len(),
        view_id: None,
        grid_cell: None,
    };
    state.analysis.records.insert(
        "continued".into(),
        Record {
            id: "continued".into(),
            sources: vec![citation(&input), next.clone()],
            data: RecordData::Fact {
                name: "背景".into(),
                value: "跨页原文".into(),
                scope: "原文".into(),
            },
        },
    );
    state.input_sha256 = digest(&input).unwrap();
    state.source_review = Some(initialize(&input, &config).unwrap());
    select_next(&input, &config, &mut state).unwrap();
    let scope = state.work().unwrap().source_scope.clone();
    assert_eq!(scope, vec!["source"]);
    let args = json!({"reference":"record:continued","summary":"Compared both delivered original fragments.","sources":[citation(&input),next]});
    let bundle = evidence(&input, &config, &state).unwrap().unwrap();
    assert!(
        context::complete_review_check(
            &input,
            &mut state,
            &args,
            config.limits.max_tool_result_bytes
        )
        .unwrap_err()
        .contains("independently inspect")
    );
    // Only a subsequent model response makes the bundled source/candidate
    // receipts available. Boundary reading does not change work scope.
    state.reviewer_coverage = bundle.coverage;
    let mut unread_args = args.clone();
    unread_args["sources"][1]["source_id"] = json!("unread");
    let before = digest(&state).unwrap();
    assert!(
        context::complete_review_check(
            &input,
            &mut state,
            &unread_args,
            config.limits.max_tool_result_bytes
        )
        .is_err()
    );
    assert_eq!(digest(&state).unwrap(), before);
    context::complete_review_check(
        &input,
        &mut state,
        &args,
        config.limits.max_tool_result_bytes,
    )
    .unwrap();
    assert_eq!(state.work().unwrap().source_scope, scope);
    assert!(context::has_review_outcome(&state, "record:continued").unwrap());
    assert!(
        context::check_read_scope(
            &input,
            &state,
            "read_source",
            &json!({"source_id":"next","start":0,"max_bytes":100})
        )
        .is_err()
    );

    // Even a previously delivered neighbor candidate must be explicitly
    // selected before it can be compared; boundary text alone is not enough.
    let key = "disposition:next";
    state.reviewer_coverage.candidate.insert(
        key.into(),
        digest(&context::reference(&state.analysis, key).unwrap()).unwrap(),
    );
    let other =
        json!({"reference":key,"summary":"Unassigned neighbor.","sources":[args["sources"][1]]});
    assert!(
        context::complete_review_check(
            &input,
            &mut state,
            &other,
            config.limits.max_tool_result_bytes
        )
        .unwrap_err()
        .contains("active review focus")
    );
    assert!(state.source_review.as_ref().unwrap().results.is_empty());
}

#[test]
fn boundary_citation_does_not_require_comparing_unrelated_neighbor_candidates() {
    let (mut input, config, mut state) = fixture();
    input.source_units.push(Source {
        source_unit_revision_id: "next".into(),
        document_id: "document".into(),
        text: "下一段原文。".into(),
        locator: json!({}),
        ordinal: 1,
    });
    state.analysis.dispositions.insert(
        "next".into(),
        Disposition {
            state: DispositionState::NonRequirement,
            reason: "独立的下段处置".into(),
        },
    );
    state.input_sha256 = digest(&input).unwrap();
    state.source_review = Some(initialize(&input, &config).unwrap());
    select_next(&input, &config, &mut state).unwrap();
    compare(&input, &config, &mut state);
    // Simulate delivery of the exact bounded tool result, never granting
    // a receipt for the adjacent candidate which was not returned.
    state.reviewer_coverage = evidence(&input, &config, &state).unwrap().unwrap().coverage;
    assert!(
        !state
            .reviewer_coverage
            .candidate
            .contains_key("disposition:next")
    );
    let mut args = judgment(&input, &config, &state);
    args["boundaries"]["after"] = json!({"state":"continuation","reason":"当前段落接下一段，下一段的候选须由其自己的任务复核。",
        "sources":[{"source_id":"next","start":0,"end":input.source_units[1].text.len()}]});
    let continuation = args["boundaries"]["after"]["sources"][0].clone();
    args["sources"].as_array_mut().unwrap().push(continuation);
    let task = tasks(&input, config.limits.max_tool_result_bytes)
        .unwrap()
        .remove(0);
    put(&input, &config, &mut state, &args).unwrap();
    assert!(complete(&input, &state, &task).unwrap());
    assert_eq!(pending(&input, &config, &state).unwrap().len(), 1);
    state.analysis.dispositions.get_mut("next").unwrap().reason = "与边界原文无关的处置更正".into();
    assert!(complete(&input, &state, &task).unwrap());
    state
        .analysis
        .dispositions
        .get_mut("source")
        .unwrap()
        .reason = "实际比较对象发生更正".into();
    assert!(!complete(&input, &state, &task).unwrap());
}

#[test]
fn review_task_delivers_bounded_neighbor_evidence_without_approving_it() {
    let (mut input, config, mut state) = fixture();
    for (id, document) in [("next", "document"), ("other", "other-document")] {
        input.source_units.push(Source {
            source_unit_revision_id: id.into(),
            document_id: document.into(),
            text: "相邻原文，仍需要独立判断。".repeat(config.limits.max_tool_result_bytes),
            locator: json!({}),
            ordinal: input.source_units.len(),
        });
    }
    state.input_sha256 = digest(&input).unwrap();
    state.source_review = Some(initialize(&input, &config).unwrap());
    select_next(&input, &config, &mut state).unwrap();
    let before = digest(&state).unwrap();
    let bundle = evidence(&input, &config, &state).unwrap().unwrap();
    let adjacent = &bundle.content["assigned_evidence"]["boundary_evidence"];
    assert_eq!(adjacent.as_array().unwrap().len(), 1);
    assert_eq!(adjacent[0]["side"], "after");
    assert_eq!(adjacent[0]["source"]["source_id"], "next");
    assert_eq!(adjacent[0]["source"]["start"], 0);
    let end = adjacent[0]["source"]["end"].as_u64().unwrap() as usize;
    assert!(end > 0 && end < input.source_units[1].text.len());
    assert_eq!(
        adjacent[0]["source"]["text"],
        input.source_units[1].text[..end]
    );
    assert_eq!(bundle.coverage.text["next"], vec![(0, end)]);
    assert!(!bundle.coverage.text.contains_key("other"));
    assert!(!bundle.coverage.candidate.contains_key("disposition:next"));
    assert_eq!(digest(&state).unwrap(), before);
    assert!(state.source_review.as_ref().unwrap().results.is_empty());
    assert!(
        serde_json::to_vec(&bundle.content).unwrap().len() <= config.limits.max_tool_result_bytes
    );
    let message =
        json!({"role":"tool","content":json!({"ok":true,"result":bundle.content}).to_string()});
    let visible = context::visible_work_evidence(&state, &[message]);
    assert!(
        !visible.contains_key("text:next"),
        "adjacent reading does not expand the active work inventory"
    );
    assert_eq!(
        state.reviewer_work.as_ref().unwrap().source_scope,
        vec!["source"]
    );
    assert!(
        context::check_read_scope(&input, &state, "read_source", &json!({"source_id":"next"}))
            .is_err()
    );

    // On a split source, preceding evidence must include the true end of
    // the prior fragment even when read_source shrinks for annotations.
    let inventory = tasks(&input, config.limits.max_tool_result_bytes).unwrap();
    let fragments: Vec<_> = inventory
        .iter()
        .filter(|task| task.source_id == "next")
        .collect();
    let previous_end = match fragments[0].region {
        Region::Text { end, .. } => end,
        _ => panic!("text task"),
    };
    let review = state.source_review.as_mut().unwrap();
    review.active_task = Some(fragments[1].id.clone());
    review.dependencies = task_dependencies(fragments[1]);
    state.reviewer_work.as_mut().unwrap().source_scope = vec!["next".into()];
    let bundle = evidence(&input, &config, &state).unwrap().unwrap();
    let preceding = &bundle.content["assigned_evidence"]["boundary_evidence"][0];
    assert_eq!(preceding["side"], "before");
    assert_eq!(preceding["source"]["end"], previous_end);
    let start = preceding["source"]["start"].as_u64().unwrap() as usize;
    assert!(start < previous_end);
    assert_eq!(
        preceding["source"]["text"],
        input.source_units[1].text[start..previous_end]
    );
}

#[test]
fn review_task_evidence_preserves_complete_grid_cells_and_only_fitted_candidates() {
    let (mut input, config, mut state) = fixture();
    input.source_units[0].text.clear();
    input.structured_forms.push(
        json!({"form_definition_revision_id":"grid","source_unit_revision_id":"source",
        "definition":{"kind":"grid","row_count":2,"column_count":2,"widths_mm":[40,40],"cells":[
            {"row":0,"column":0,"row_span":1,"col_span":2,"text":"完整标题"},
            {"row":1,"column":0,"row_span":1,"col_span":1,"text":"原始字段"},
            {"row":1,"column":1,"row_span":1,"col_span":1,"text":""}]}}),
    );
    state.reviewer_coverage = Coverage::default();
    state.reviewer_progress = Progress::default();
    state.source_review = Some(initialize(&input, &config).unwrap());
    select_next(&input, &config, &mut state).unwrap();
    state.analysis.records.insert(
        "large".into(),
        Record {
            id: "large".into(),
            sources: vec![Span {
                source_id: "source".into(),
                start: 0,
                end: 0,
                view_id: None,
                grid_cell: Some(GridCitation {
                    form_id: "grid".into(),
                    row: 1,
                    column: 0,
                }),
            }],
            data: RecordData::Fact {
                name: "large".into(),
                value: "x".repeat(config.limits.max_tool_result_bytes),
                scope: "source".into(),
            },
        },
    );
    let before = digest(&state).unwrap();
    let bundle = evidence(&input, &config, &state).unwrap().unwrap();
    assert_eq!(digest(&state).unwrap(), before);
    let source = &bundle.content["assigned_evidence"]["source"];
    assert_eq!(source["next"], 4);
    assert_eq!(
        source["cells"][1],
        Value::Null,
        "covered merged position stays null"
    );
    assert_eq!(source["cells"][3]["text"], "", "empty anchor stays present");
    assert_eq!(source["citation_refs"][1], Value::Null);
    assert_eq!(bundle.coverage.form_cells["grid"], vec![(0, 4)]);
    assert!(bundle.coverage.views.is_empty());
    assert!(!bundle.coverage.candidate.contains_key("record:large"));
    assert!(bundle.coverage.candidate.contains_key("disposition:source"));
    assert!(
        serde_json::to_vec(&bundle.content).unwrap().len() <= config.limits.max_tool_result_bytes
    );
    let message =
        json!({"role":"tool","content":json!({"ok":true,"result":bundle.content}).to_string()});
    let visible = context::visible_work_evidence(&state, &[message]);
    assert_eq!(visible["form:grid"], vec![(0, 4)]);
    assert!(
        visible
            .keys()
            .any(|key| key.starts_with("candidate:disposition:source:"))
    );
    state.role = Role::Main;
    assert!(evidence(&input, &config, &state).unwrap().is_none());
    state.role = Role::Reviewer;
    state.reviewer_work.as_mut().unwrap().source_scope.clear();
    assert!(
        evidence(&input, &config, &state)
            .err()
            .unwrap()
            .contains("outside active work")
    );
}

fn fixture() -> (FrozenInput, Config, Checkpoint) {
    let input = FrozenInput {
        schema_version: 1,
        project_id: "test".into(),
        document_set_id: "test".into(),
        documents: vec![],
        document_relations: vec![],
        decisions: vec![],
        structured_forms: vec![],
        source_units: vec![Source {
            source_unit_revision_id: "source".into(),
            document_id: "document".into(),
            text: "项目背景介绍。".into(),
            locator: json!({}),
            ordinal: 0,
        }],
    };
    let config = crate::tender_analysis::tests::config();
    let mut analysis = Analysis::default();
    tools::cover(
        analysis.coverage.text.entry("source".into()).or_default(),
        0,
        input.source_units[0].text.len(),
    );
    analysis.dispositions.insert(
        "source".into(),
        Disposition {
            state: DispositionState::NonRequirement,
            reason: "背景介绍".into(),
        },
    );
    let mut coverage = analysis.coverage.clone();
    let value = context::reference(&analysis, "disposition:source").unwrap();
    coverage
        .candidate
        .insert("disposition:source".into(), digest(&value).unwrap());
    let mut state = Checkpoint {
        journal: Default::default(),
        input_sha256: digest(&input).unwrap(),
        config_sha256: digest(&config).unwrap(),
        turn: 0,
        tool_calls: 0,
        read_bytes: 0,
        review_rounds: 0,
        role: Role::Reviewer,
        analysis,
        review: None,
        review_draft: BTreeMap::new(),
        source_review: Some(initialize(&input, &config).unwrap()),
        repair: Default::default(),
        reviewer_coverage: coverage,
        pending_coverage: None,
        transcript: vec![],
        main_progress: Default::default(),
        reviewer_progress: Default::default(),
        main_work: None,
        reviewer_work: None,
        done: false,
        source_views: BTreeMap::new(),
    };
    select_next(&input, &config, &mut state).unwrap();
    compare(&input, &config, &mut state);
    (input, config, state)
}

#[test]
fn task_inventory_reuse_respects_input_budget_and_checkpoint_boundaries() {
    let (mut input, config, mut state) = fixture();
    input.source_units[0].text = "Synthetic original paragraph.\n".repeat(200);
    state.source_review = Some(initialize(&input, &config).unwrap());
    let before = digest(&state).unwrap();
    let wide = task_inventory(&input, &state, config.limits.max_tool_result_bytes).unwrap();
    assert_eq!(
        json!(wide),
        json!(tasks(&input, config.limits.max_tool_result_bytes).unwrap())
    );
    let cached = state
        .source_review
        .as_ref()
        .unwrap()
        .task_inventory
        .get()
        .unwrap()
        .clone();
    assert_eq!(
        json!(task_inventory(&input, &state, config.limits.max_tool_result_bytes).unwrap()),
        json!(wide)
    );
    assert!(std::sync::Arc::ptr_eq(
        &cached,
        state
            .source_review
            .as_ref()
            .unwrap()
            .task_inventory
            .get()
            .unwrap()
    ));

    let small_budget = 4096;
    let narrow = task_inventory(&input, &state, small_budget).unwrap();
    assert!(narrow.len() > wide.len());
    assert_eq!(json!(narrow), json!(tasks(&input, small_budget).unwrap()));
    input.source_units[0]
        .text
        .push_str("Changed original evidence.");
    let changed = task_inventory(&input, &state, config.limits.max_tool_result_bytes).unwrap();
    assert_ne!(json!(changed), json!(wide));
    assert_eq!(
        json!(changed),
        json!(tasks(&input, config.limits.max_tool_result_bytes).unwrap())
    );

    assert_eq!(digest(&state).unwrap(), before);
    let restored: Checkpoint = serde_json::from_value(json!(state)).unwrap();
    assert!(
        restored
            .source_review
            .as_ref()
            .unwrap()
            .task_inventory
            .get()
            .is_none()
    );
    assert_eq!(
        json!(task_inventory(&input, &restored, config.limits.max_tool_result_bytes).unwrap()),
        json!(changed)
    );
    assert_eq!(digest(&restored).unwrap(), before);
}

#[test]
fn reopened_task_delivers_prior_judgment_without_restoring_approval_or_displacing_evidence() {
    let (input, config, mut state) = fixture();
    let args = judgment(&input, &config, &state);
    put(&input, &config, &mut state, &args).unwrap();
    let prior = json!(
        state
            .source_review
            .as_ref()
            .unwrap()
            .results
            .values()
            .next()
            .unwrap()
            .judgment
    );
    state
        .analysis
        .dispositions
        .get_mut("source")
        .unwrap()
        .reason = "Updated candidate explanation requiring independent comparison.".into();
    select_next(&input, &config, &mut state).unwrap();
    let before = digest(&state).unwrap();
    let bundle = evidence(&input, &config, &state).unwrap().unwrap();
    assert_eq!(
        bundle.content["assigned_evidence"]["prior_judgment"]["judgment"],
        prior
    );
    assert!(!context::has_review_outcome(&state, "disposition:source").unwrap());
    assert_eq!(pending(&input, &config, &state).unwrap().len(), 1);
    assert_eq!(digest(&state).unwrap(), before);
    assert!(
        put(&input, &config, &mut state.clone(), &args)
            .unwrap_err()
            .contains("stale dependency version")
    );

    let mut without_history = state.clone();
    without_history
        .source_review
        .as_mut()
        .unwrap()
        .results
        .clear();
    let original = evidence(&input, &config, &without_history)
        .unwrap()
        .unwrap();
    assert_eq!(json!(bundle.coverage), json!(original.coverage));
    let message = |content: &Value| {
        vec![json!({"role":"tool","content":json!({"ok":true,"result":content}).to_string()})]
    };
    assert_eq!(
        context::visible_work_evidence(&state, &message(&bundle.content)),
        context::visible_work_evidence(&state, &message(&original.content)),
        "historical citations are not visible original evidence"
    );
    let mut current = bundle.content;
    current["assigned_evidence"]
        .as_object_mut()
        .unwrap()
        .remove("prior_judgment");
    assert_eq!(current, original.content);

    // A prior result near the old admission limit must not displace the
    // current source/candidates or be silently returned as a partial result.
    let prior = state
        .source_review
        .as_mut()
        .unwrap()
        .results
        .values_mut()
        .next()
        .unwrap();
    let old_size = serde_json::to_vec(&prior.judgment).unwrap().len();
    prior
        .judgment
        .summary
        .push_str(&"x".repeat(config.limits.max_tool_result_bytes - old_size));
    let bundle = evidence(&input, &config, &state).unwrap().unwrap();
    assert_eq!(bundle.content, original.content);
    assert_eq!(json!(bundle.coverage), json!(original.coverage));
    assert!(
        serde_json::to_vec(&bundle.content).unwrap().len() <= config.limits.max_tool_result_bytes
    );
}

#[test]
fn unrelated_execution_blocker_preserves_completed_source_guidance() {
    use crate::agent_runtime::progress::{ExecutionBlocker, ProgressWatch};

    let (input, config, mut state) = fixture();
    let focus = &mut state.reviewer_work.as_mut().unwrap().focus;
    focus.action = context::FocusAction::Review;
    focus.references = vec!["disposition:source".into()];
    state.reviewer_progress.watch = ProgressWatch {
        replans: 1,
        recovery: Recovery::Replan,
        ..Default::default()
    };
    state.reviewer_progress.blockers.push(ExecutionBlocker {
        scope: vec!["independent-source".into()],
        dependencies_sha256: digest(&Vec::<Value>::new()).unwrap(),
        watch: ProgressWatch {
            recovery: Recovery::Blocked,
            ..Default::default()
        },
    });
    let before = digest(&state).unwrap();
    let work = context::request_work_state(&input, &state, 4096).unwrap();
    assert_eq!(work["comparison_progress"]["remaining"], 0);
    assert_eq!(work["next_action"], "complete_source_review");
    let execution = context::execution_packet(&state, 4096).unwrap();
    assert_eq!(execution["blocker_count"], 1);
    assert!(
        execution["next_action"]
            .as_str()
            .unwrap()
            .contains("already has recorded outcomes")
    );
    assert_eq!(digest(&state).unwrap(), before);

    // A genuine local judgment may finish, but the unrelated execution
    // failure must still prevent whole-analysis acceptance.
    let args = judgment(&input, &config, &state);
    put(&input, &config, &mut state, &args).unwrap();
    assert!(pending(&input, &config, &state).unwrap().is_empty());
    assert!(!agent::review_complete(&input, &config, &state).unwrap());
    assert_eq!(state.reviewer_progress.blockers.len(), 1);
    assert!(!state.done);
    state.main_progress.blockers = std::mem::take(&mut state.reviewer_progress.blockers);
    assert!(!agent::review_complete(&input, &config, &state).unwrap());

    // An overlapping, unchanged blocker still suppresses completion
    // guidance. Dependency changes can make that local scope executable.
    let (input, _, mut state) = fixture();
    let focus = &mut state.reviewer_work.as_mut().unwrap().focus;
    focus.action = context::FocusAction::Review;
    focus.references = vec!["disposition:source".into()];
    let dependencies = digest(&vec![
        context::reference(&state.analysis, "disposition:source").unwrap(),
    ])
    .unwrap();
    state.reviewer_progress.blockers.push(ExecutionBlocker {
        scope: vec!["source".into()],
        dependencies_sha256: dependencies,
        watch: Default::default(),
    });
    let before = digest(&state).unwrap();
    assert!(context::check_blocked_scope(&state, &["source".into()]).is_err());
    let work = context::request_work_state(&input, &state, 4096).unwrap();
    assert_eq!(work["next_action"], "resolve_execution_blockers");
    let execution = context::execution_packet(&state, 4096).unwrap();
    assert!(
        !execution["next_action"]
            .as_str()
            .unwrap()
            .contains("already has recorded outcomes")
    );
    assert_eq!(digest(&state).unwrap(), before);
    state.reviewer_progress.blockers[0].dependencies_sha256 = "prior-version".into();
    assert!(context::check_blocked_scope(&state, &["source".into()]).is_ok());
    let work = context::request_work_state(&input, &state, 4096).unwrap();
    assert_eq!(work["next_action"], "complete_source_review");
    assert_eq!(state.reviewer_progress.blockers.len(), 1);
}

fn citation(input: &FrozenInput) -> Span {
    Span {
        source_id: "source".into(),
        start: 0,
        end: input.source_units[0].text.len(),
        view_id: None,
        grid_cell: None,
    }
}

fn compare(input: &FrozenInput, config: &Config, state: &mut Checkpoint) {
    context::complete_review_check(
        input,
        state,
        &json!({"reference":"disposition:source",
        "summary":"原文为项目背景，没有独立投标输出义务。","sources":[citation(input)]}),
        config.limits.max_tool_result_bytes,
    )
    .unwrap();
}

fn judgment(input: &FrozenInput, config: &Config, state: &Checkpoint) -> Value {
    let packet = packet(input, config, state).unwrap();
    let boundary = json!({"state":"complete","reason":"完整段落，与相邻内容无未解决的续接。","sources":[citation(input)]});
    json!({"task_id":packet["current"]["task"]["id"],"expected_version":packet["current"]["expected_version"],
        "status":"checked","summary":"逐段比较后未见遗漏的投标要求；来源处置与背景内容一致。",
        "sources":[citation(input)],"candidate_refs":["disposition:source"],
        "template_mappings":[],"boundaries":{"before":boundary,"after":boundary},"finding_ids":[],"evidence_requests":[]})
}

#[tokio::test]
async fn same_response_finding_withdrawal_preserves_version_and_evidence_checks() {
    let (input, config, mut state) = fixture();
    let finding = Finding {
        code: "source_issue".into(),
        message: "Synthetic suspected omission.".into(),
        correction: "Compare with the original background paragraph.".into(),
        affected: vec![],
        sources: vec![citation(&input)],
    };
    finding_changed(&mut state, None, Some(&finding)).unwrap();
    state.review_draft.insert("issue".into(), finding);
    let body: Value =
        serde_json::from_slice(&request(&input, &config, &mut state).await.unwrap()).unwrap();
    let batch = BatchVersion::capture(&state, &body).unwrap().unwrap();
    let args = judgment(&input, &config, &state);
    let mut prepared = state.clone();
    let response = ChatTurn {
        finish_reason: "tool_calls".into(),
        tool_calls: [
            ("delete_review_finding", json!({"id":"issue"})),
            (
                "complete_review_check",
                json!({"reference":"disposition:source",
                "summary":"The original paragraph is background.","sources":[citation(&input)]}),
            ),
            ("put_source_review", args.clone()),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (name, args))| knowledge::models::ChatToolCall {
            id: format!("batch-{index}"),
            name: name.into(),
            arguments: args.to_string(),
        })
        .collect(),
        ..Default::default()
    };
    prepared
        .journal
        .prepare(
            prepared.turn,
            "reviewer",
            &serde_json::to_vec(&body).unwrap(),
        )
        .unwrap();
    prepared.journal.responded(response).unwrap();
    let mut restored: Checkpoint = serde_json::from_value(json!(prepared)).unwrap();
    struct NoIo;
    #[async_trait]
    impl Journal for NoIo {
        async fn load(&self) -> Result<Option<Checkpoint>, AgentError> {
            panic!("no journal reads during tool execution")
        }
        async fn reserve(&self, _: &Checkpoint, _: &[u8]) -> Result<Option<usize>, AgentError> {
            panic!("no model reservation during replay")
        }
        async fn save(&self, _: &Checkpoint, _: &Value) -> Result<(), AgentError> {
            panic!("tool batch does not save partial state")
        }
    }
    let response = restored.journal.response().unwrap().clone();
    let results = execute_turn(
        &input,
        &config,
        &mut restored,
        &NoIo,
        response,
        BTreeMap::new(),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(results.len(), 3);
    for result in results {
        let result: Value = serde_json::from_str(result["content"].as_str().unwrap()).unwrap();
        assert_eq!(result["ok"], true, "{result}");
    }
    assert!(restored.review_draft.is_empty());
    assert_eq!(restored.source_review.as_ref().unwrap().results.len(), 1);
    let mut wrong_body = body.clone();
    let content = &mut wrong_body["messages"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap()["content"];
    let mut packet: Value = serde_json::from_str(content.as_str().unwrap()).unwrap();
    packet["source_review"]["current"]["expected_version"] = json!("older-turn");
    *content = json!(packet.to_string());
    assert!(
        BatchVersion::capture(&state, &wrong_body)
            .unwrap()
            .is_none()
    );

    apply_in_batch(
        &input,
        &config,
        &mut state,
        "delete_review_finding",
        &json!({"id":"issue"}),
        Some(&batch),
    )
    .unwrap();
    assert!(
        put(&input, &config, &mut state.clone(), &args)
            .unwrap_err()
            .contains("stale dependency version"),
        "reproduces the strict per-tool version conflict"
    );
    assert!(
        BatchVersion::capture(&state, &body).unwrap().is_none(),
        "a later response cannot reuse the old request snapshot"
    );
    let after_withdrawal = state.clone();
    assert!(
        put_in_batch(&input, &config, &mut state.clone(), &args, Some(&batch))
            .unwrap_err()
            .contains("record the current candidate comparison")
    );
    compare(&input, &config, &mut state);

    let mut stale = args.clone();
    stale["expected_version"] = json!("older-turn");
    assert!(
        put_in_batch(&input, &config, &mut state.clone(), &stale, Some(&batch))
            .unwrap_err()
            .contains("stale dependency version")
    );
    let mut changed = state.clone();
    changed
        .analysis
        .dispositions
        .get_mut("source")
        .unwrap()
        .reason
        .push_str(" changed");
    assert!(
        put_in_batch(&input, &config, &mut changed, &args, Some(&batch))
            .unwrap_err()
            .contains("stale dependency version")
    );
    let mut changed_scope = state.clone();
    changed_scope
        .source_review
        .as_mut()
        .unwrap()
        .dependencies
        .global = true;
    assert!(
        put_in_batch(&input, &config, &mut changed_scope, &args, Some(&batch))
            .unwrap_err()
            .contains("stale dependency version")
    );
    let mut changed_task = state.clone();
    changed_task.source_review.as_mut().unwrap().active_task = Some("another-task".into());
    assert!(
        put_in_batch(&input, &config, &mut changed_task, &args, Some(&batch))
            .unwrap_err()
            .contains("active source review task")
    );
    let mut unread = state.clone();
    unread.reviewer_coverage.text.clear();
    assert!(put_in_batch(&input, &config, &mut unread, &args, Some(&batch)).is_err());
    let mut unseen = state.clone();
    unseen.reviewer_coverage.candidate.clear();
    assert!(
        put_in_batch(&input, &config, &mut unseen, &args, Some(&batch))
            .unwrap_err()
            .contains("independently retrieve")
    );

    apply_in_batch(
        &input,
        &config,
        &mut state,
        "put_source_review",
        &args,
        Some(&batch),
    )
    .unwrap();
    let receipt = &state.source_review.as_ref().unwrap().results[args["task_id"].as_str().unwrap()];
    assert_eq!(receipt.judgment.expected_version, args["expected_version"]);
    assert_eq!(
        receipt.version,
        version(&state, &receipt.dependencies).unwrap()
    );
    assert_ne!(receipt.version, receipt.judgment.expected_version);
    assert!(pending(&input, &config, &state).unwrap().is_empty());

    // Newly saved findings still have to be included in the final judgment.
    let mut with_finding = after_withdrawal;
    let finding = Finding {
        code: "other_issue".into(),
        message: "Another suspected omission.".into(),
        correction: "Check the original.".into(),
        affected: vec![],
        sources: vec![citation(&input)],
    };
    finding_changed(&mut with_finding, None, Some(&finding)).unwrap();
    with_finding
        .review_draft
        .insert("new-issue".into(), finding);
    let error = put_in_batch(&input, &config, &mut with_finding, &args, Some(&batch)).unwrap_err();
    assert!(error.contains("/finding_ids"), "{error}");
    let mut findings_args = args;
    findings_args["status"] = json!("findings");
    findings_args["finding_ids"] = json!(["new-issue"]);
    put_in_batch(
        &input,
        &config,
        &mut with_finding,
        &findings_args,
        Some(&batch),
    )
    .unwrap();
}

fn relationship_fixture() -> (FrozenInput, Config, Checkpoint) {
    let (input, config, mut state) = fixture();
    state.analysis.records.insert(
        "rule".into(),
        Record {
            id: "rule".into(),
            sources: vec![citation(&input)],
            data: RecordData::Rule {
                text: "项目背景约定".into(),
                scope: "项目".into(),
                applicability: Applicability {
                    state: ApplicabilityState::Applicable,
                    condition: String::new(),
                    scope: "项目".into(),
                    grounds: vec![citation(&input)],
                },
            },
        },
    );
    let key = "record:rule";
    state.reviewer_coverage.candidate.insert(
        key.into(),
        digest(&context::reference(&state.analysis, key).unwrap()).unwrap(),
    );
    context::complete_review_check(&input, &mut state,
        &json!({"reference":key,"summary":"Synthetic rule text comparison.","sources":[citation(&input)]}),
        config.limits.max_tool_result_bytes).unwrap();
    compare(&input, &config, &mut state);
    (input, config, state)
}

fn relationship_judgment(input: &FrozenInput, config: &Config, state: &Checkpoint) -> Judgment {
    let mut value = judgment(input, config, state);
    value["relationship_checks"] = json!([{"record_id":"rule","status":"not_required",
        "related_record_ids":[],"relation_ids":[],"unresolved_record_ids":[],"finding_ids":[],
        "reason":"Synthetic background rule is standalone; no external condition or continuation.","sources":[citation(input)]}]);
    serde_json::from_value(value).unwrap()
}

#[test]
fn supporting_queries_change_dependencies_without_owning_other_source_judgments() {
    let (mut input, config, mut state) = relationship_fixture();
    for id in ["foreign", "other"] {
        input.source_units.push(Source {
            source_unit_revision_id: id.into(),
            document_id: "document".into(),
            text: "独立条款。".into(),
            locator: json!({}),
            ordinal: input.source_units.len(),
        });
        let source = Span {
            source_id: id.into(),
            start: 0,
            end: "独立条款。".len(),
            view_id: None,
            grid_cell: None,
        };
        state.analysis.records.insert(
            id.into(),
            Record {
                id: id.into(),
                sources: vec![source],
                data: RecordData::Unresolved {
                    problem: format!("Synthetic {id} uncertainty"),
                    affected: vec![],
                    candidates: vec![id.into()],
                },
            },
        );
        state.analysis.dispositions.insert(
            id.into(),
            Disposition {
                state: DispositionState::Unresolved,
                reason: "Synthetic candidate retained.".into(),
            },
        );
    }
    state.input_sha256 = digest(&input).unwrap();
    state.source_review = Some(initialize(&input, &config).unwrap());
    select_next(&input, &config, &mut state).unwrap();
    compare(&input, &config, &mut state);
    context::complete_review_check(&input,&mut state,&json!({"reference":"record:rule","summary":"Original background rule comparison.","sources":[citation(&input)]}),config.limits.max_tool_result_bytes).unwrap();
    let before = packet(&input, &config, &state).unwrap();
    let base_dependencies = state.source_review.as_ref().unwrap().dependencies.clone();
    let base_version = version(&state, &base_dependencies).unwrap();
    record_query(
        &input,
        &mut state,
        "inspect_analysis",
        &json!({"kind":"unresolved","ids":["foreign"],"view":"detail","offset":0,"limit":1}),
    );
    let after = packet(&input, &config, &state).unwrap();
    assert_ne!(
        after["current"]["expected_version"],
        before["current"]["expected_version"]
    );
    for field in [
        "records_requiring_relationship_judgment",
        "templates_requiring_mapping_judgment",
        "pending_candidate_refs",
        "comparison_total",
    ] {
        assert_eq!(
            after["current"][field], before["current"][field],
            "lookup grew {field}"
        );
    }
    let work =
        context::request_work_state(&input, &state, config.limits.max_tool_result_bytes).unwrap();
    assert_eq!(work["comparison_progress"]["remaining"], 0);
    let args = relationship_judgment(&input, &config, &state);
    put(&input, &config, &mut state, &json!(args)).unwrap();
    let task = task_inventory(&input, &state, config.limits.max_tool_result_bytes)
        .unwrap()
        .remove(0);
    assert!(complete(&input, &state, &task).unwrap());
    assert!(
        !agent::review_complete(&input, &config, &state).unwrap(),
        "foreign sources remain mandatory globally"
    );
    if let RecordData::Unresolved { problem, .. } =
        &mut state.analysis.records.get_mut("foreign").unwrap().data
    {
        problem.push_str(" revised");
    }
    assert_eq!(
        version(&state, &base_dependencies).unwrap(),
        base_version,
        "unqueried unrelated content does not affect the source"
    );
    assert!(
        !complete(&input, &state, &task).unwrap(),
        "queried support changes still invalidate the saved judgment"
    );

    for (id, from, to) in [
        ("direct", "rule", "foreign"),
        ("foreign-edge", "foreign", "other"),
    ] {
        state.analysis.relations.insert(
            id.into(),
            Relation {
                id: id.into(),
                from: from.into(),
                to: to.into(),
                from_target: RelationTarget::Record,
                to_target: RelationTarget::Record,
                from_record_sha256: digest(&state.analysis.records[from]).unwrap(),
                to_record_sha256: digest(&state.analysis.records[to]).unwrap(),
                kind: RelationKind::References,
                state: RelationState::Explicit,
                scope: "Synthetic clause reference".into(),
                explanation: "Synthetic source-backed relation".into(),
                grounds: vec![],
            },
        );
    }
    let owned = obligations(&input, &state, &task);
    assert!(owned.comparisons.contains("record:foreign"));
    assert!(owned.comparisons.contains("relation:direct"));
    assert!(!owned.subjects.contains("record:foreign"));
    assert!(!owned.comparisons.contains("relation:foreign-edge"));
    assert!(!owned.comparisons.contains("record:other"));
    let foreign_task = task_inventory(&input, &state, config.limits.max_tool_result_bytes)
        .unwrap()
        .into_iter()
        .find(|task| task.source_id == "foreign")
        .unwrap();
    assert!(
        obligations(&input, &state, &foreign_task)
            .subjects
            .contains("record:foreign")
    );
}

#[test]
fn relationship_ground_recall_requires_committed_reads_and_preserves_complete_text_and_cells() {
    let (mut input, config, mut state) = relationship_fixture();
    input.source_units.push(Source {
        source_unit_revision_id: "cross".into(),
        document_id: "document".into(),
        text: "另一段计费示例。".into(),
        locator: json!({}),
        ordinal: 1,
    });
    input.structured_forms.push(json!({"form_definition_revision_id":"grid","source_unit_revision_id":"cross",
        "definition":{"kind":"grid","row_count":1,"column_count":1,"cells":[{"row":0,"column":0,"row_span":1,"col_span":1,"text":"原始费率"}]}}));
    let text_span = Span {
        source_id: "cross".into(),
        start: 0,
        end: input.source_units[1].text.len(),
        view_id: None,
        grid_cell: None,
    };
    let grid_span = Span {
        source_id: "cross".into(),
        start: 0,
        end: 0,
        view_id: None,
        grid_cell: Some(GridCitation {
            form_id: "grid".into(),
            row: 0,
            column: 0,
        }),
    };
    state.analysis.records.get_mut("rule").unwrap().sources = vec![text_span.clone(), grid_span];
    let refs = BTreeSet::from(["record:rule".into()]);
    let task = tasks(&input, config.limits.max_tool_result_bytes)
        .unwrap()
        .remove(0);
    state
        .analysis
        .coverage
        .text
        .insert("cross".into(), vec![(0, text_span.end)]);
    state
        .analysis
        .coverage
        .form_cells
        .insert("grid".into(), vec![(0, 1)]);
    state.pending_coverage = Some(state.analysis.coverage.clone());
    assert!(
        recalled_relationship_sources(&input, &state, &task, &refs, 4096)
            .unwrap()
            .is_empty(),
        "main reads and unconfirmed current-batch reads cannot qualify"
    );
    state.reviewer_coverage = state.pending_coverage.take().unwrap();
    let before = digest(&state).unwrap();
    let rows = recalled_relationship_sources(&input, &state, &task, &refs, 4096).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["source"]["text"], input.source_units[1].text);
    assert_eq!(rows[0]["source"]["end"], text_span.end);
    assert_eq!(rows[1]["source"]["cells"][0]["text"], "原始费率");
    assert_eq!(rows[1]["source"]["next"], 1);
    assert_eq!(digest(&state).unwrap(), before);
    assert!(
        recalled_relationship_sources(&input, &state, &task, &refs, 128)
            .unwrap()
            .is_empty()
    );

    // A large text original cannot be truncated to make its citation fit,
    // and does not prevent a later complete cell from fitting.
    input.source_units[1].text = "完整原文。".repeat(1000);
    state.analysis.records.get_mut("rule").unwrap().sources[0].end =
        input.source_units[1].text.len();
    state
        .reviewer_coverage
        .text
        .insert("cross".into(), vec![(0, input.source_units[1].text.len())]);
    let rows = recalled_relationship_sources(&input, &state, &task, &refs, 4096).unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0]["source"]["form_id"].is_string());

    // Different fragments of one parsed source need the same treatment.
    input.source_units[1].text = "甲乙丙丁".into();
    state.analysis.records.get_mut("rule").unwrap().sources = vec![Span {
        source_id: "cross".into(),
        start: 6,
        end: 12,
        view_id: None,
        grid_cell: None,
    }];
    let task = Task {
        id: "fragment".into(),
        source_id: "cross".into(),
        region: Region::Text { start: 0, end: 6 },
    };
    let rows = recalled_relationship_sources(&input, &state, &task, &refs, 4096).unwrap();
    assert_eq!(rows[0]["source"]["text"], "丙丁");
}

#[test]
fn saved_candidate_findings_are_actionable_without_another_inventory_call() {
    let (input, config, mut state) = fixture();
    let finding = Finding {
        code: "source_issue".into(),
        message: "Synthetic source omission.".into(),
        correction: "Compare the omitted obligation with the original.".into(),
        affected: vec![],
        sources: vec![citation(&input)],
    };
    finding_changed(&mut state, None, Some(&finding)).unwrap();
    state.review_draft.insert("saved-issue".into(), finding);
    let before = digest(&state).unwrap();
    let navigation = packet(&input, &config, &state).unwrap();
    assert_eq!(
        navigation["current"]["candidates_with_findings"]["items"],
        json!([{"reference":"disposition:source","finding_ids":["saved-issue"]}])
    );
    assert_eq!(navigation["current"]["pending_candidate_refs"]["total"], 0);
    let error = context::complete_review_check(
        &input,
        &mut state,
        &json!({"reference":"disposition:source","summary":"No issue.","sources":[citation(&input)]}),
        config.limits.max_tool_result_bytes,
    )
    .unwrap_err();
    let feedback: Value =
        serde_json::from_str(error.strip_prefix("INVALID_FIELD /reference: ").unwrap()).unwrap();
    assert_eq!(feedback["reference"], "disposition:source");
    assert_eq!(feedback["recorded_outcome"], "findings");
    assert_eq!(feedback["finding_ids"]["items"], json!(["saved-issue"]));
    assert_eq!(
        digest(&state).unwrap(),
        before,
        "refusal grants no clean receipt"
    );

    // A finding cannot act as a comparison of an undelivered candidate.
    state.reviewer_coverage.candidate.clear();
    let navigation = packet(&input, &config, &state).unwrap();
    assert_eq!(
        navigation["current"]["candidates_with_findings"]["total"],
        0
    );
    assert_eq!(navigation["current"]["pending_candidate_refs"]["total"], 1);
    assert!(!context::has_review_outcome(&state, "disposition:source").unwrap());
}

#[test]
fn source_finding_feedback_identifies_the_exact_missing_and_unrelated_ids() {
    let (input, config, mut state) = fixture();
    for index in 0..8 {
        state.review_draft.insert(
            format!("issue-{index}"),
            Finding {
                code: "source_issue".into(),
                message: "Synthetic saved source issue.".into(),
                correction: "Compare and repair against the original source.".into(),
                affected: vec![],
                sources: vec![citation(&input)],
            },
        );
    }
    let mut args = judgment(&input, &config, &state);
    args["status"] = json!("findings");
    let kept: Vec<_> = state
        .review_draft
        .keys()
        .filter(|id| id.as_str() != "issue-4")
        .cloned()
        .collect();
    args["finding_ids"] = json!(kept);
    let before = digest(&state).unwrap();
    let detail = |error: String| -> Value {
        serde_json::from_str(error.strip_prefix("INVALID_FIELD /finding_ids: ").unwrap()).unwrap()
    };
    let error = detail(put(&input, &config, &mut state, &args).unwrap_err());
    assert_eq!(error["missing_finding_ids"]["items"], json!(["issue-4"]));
    assert_eq!(digest(&state).unwrap(), before);
    args["finding_ids"]
        .as_array_mut()
        .unwrap()
        .push(json!("not-in-scope"));
    let error = detail(put(&input, &config, &mut state, &args).unwrap_err());
    assert_eq!(
        error["unexpected_finding_ids"]["items"],
        json!(["not-in-scope"])
    );
    assert_eq!(digest(&state).unwrap(), before);
    args["finding_ids"] = json!(state.review_draft.keys().collect::<Vec<_>>());
    put(&input, &config, &mut state, &args).unwrap();
    assert_eq!(state.review_draft.len(), 8);
}

#[test]
#[ignore = "requires KB_TENDER_CONTEXT_REPLAY_DIR and KB_TENDER_CONTEXT_REPORT; offline only"]
fn archived_relationship_receipt_versions_are_reported() {
    let root = std::path::PathBuf::from(std::env::var("KB_TENDER_CONTEXT_REPLAY_DIR").unwrap());
    let state: Checkpoint =
        serde_json::from_slice(&std::fs::read(root.join("extraction/checkpoint.json")).unwrap())
            .unwrap();
    let mut receipts = Vec::new();
    for (task, receipt) in &state.source_review.as_ref().unwrap().results {
        if !receipt.judgment.relationship_checks.is_empty() {
            receipts.push(json!({"task_id":task,"current":receipt.version == version(&state,&receipt.dependencies).unwrap(),"stored_version":receipt.version,"current_version":version(&state,&receipt.dependencies).unwrap(),"record_ids":receipt.judgment.relationship_checks.iter().map(|c|&c.record_id).collect::<Vec<_>>(),"dependencies":receipt.dependencies}));
        }
    }
    assert!(
        !receipts.is_empty(),
        "the archive must contain saved relationship decisions"
    );
    std::fs::write(std::env::var("KB_TENDER_CONTEXT_REPORT").unwrap(),serde_json::to_vec_pretty(&json!({"turn":state.turn,"receipts":receipts,"source_checkpoint_unchanged":true,"semantic_acceptance":false})).unwrap()).unwrap();
}

#[test]
fn clean_candidate_cannot_replace_relationship_judgment() {
    let (input, config, mut state) = relationship_fixture();
    let args = judgment(&input, &config, &state);
    let before = digest(&state).unwrap();
    let error = put(&input, &config, &mut state, &args).unwrap_err();
    assert!(error.contains("/relationship_checks"), "{error}");
    assert!(
        error.contains("rule"),
        "missing record ID must be actionable: {error}"
    );
    assert_eq!(before, digest(&state).unwrap());
}

#[test]
fn relationship_receipt_reopens_after_edge_or_selected_target_changes() {
    let (input, config, mut state) = relationship_fixture();
    let check = relationship_judgment(&input, &config, &state);
    put(&input, &config, &mut state, &json!(check)).unwrap();
    assert!(pending(&input, &config, &state).unwrap().is_empty());
    // Restoring a clean legacy receipt cannot bypass the new explicit check.
    let mut legacy = state.clone();
    legacy
        .source_review
        .as_mut()
        .unwrap()
        .results
        .values_mut()
        .next()
        .unwrap()
        .judgment
        .relationship_checks
        .clear();
    assert_eq!(pending(&input, &config, &legacy).unwrap().len(), 1);
    state.analysis.records.insert(
        "target".into(),
        Record {
            id: "target".into(),
            sources: vec![],
            data: RecordData::Fact {
                name: "choice".into(),
                value: "selected".into(),
                scope: "project".into(),
            },
        },
    );
    let dependencies = state.source_review.as_ref().unwrap().dependencies.clone();
    let before = version(&state, &dependencies).unwrap();
    state.analysis.relations.insert(
        "edge".into(),
        Relation {
            id: "edge".into(),
            from: "rule".into(),
            to: "target".into(),
            from_target: RelationTarget::Record,
            to_target: RelationTarget::Record,
            from_record_sha256: digest(&state.analysis.records["rule"]).unwrap(),
            to_record_sha256: digest(&state.analysis.records["target"]).unwrap(),
            kind: RelationKind::References,
            state: RelationState::Explicit,
            scope: "project".into(),
            explanation: "selected target".into(),
            grounds: vec![],
        },
    );
    let added = version(&state, &dependencies).unwrap();
    assert_ne!(before, added);
    assert_eq!(pending(&input, &config, &state).unwrap().len(), 1);
    if let RecordData::Fact { value, .. } =
        &mut state.analysis.records.get_mut("target").unwrap().data
    {
        *value = "changed selection".into();
    }
    assert_ne!(added, version(&state, &dependencies).unwrap());
    state.analysis.relations.remove("edge");
    assert_eq!(before, version(&state, &dependencies).unwrap());
}

#[test]
fn relationship_resolution_requires_current_incident_edges_and_read_endpoints() {
    let (input, config, mut state) = relationship_fixture();
    let mut check = relationship_judgment(&input, &config, &state);
    let mut deps = state.source_review.as_ref().unwrap().dependencies.clone();
    check.relationship_checks[0].status = RelationshipStatus::Resolved;
    assert!(
        validate_relationship_checks(&input, &state, &check, &mut deps)
            .unwrap_err()
            .contains("resolved requires")
    );
    state.analysis.records.insert(
        "target".into(),
        Record {
            id: "target".into(),
            sources: vec![citation(&input)],
            data: RecordData::Fact {
                name: "choice".into(),
                value: "selected".into(),
                scope: "project".into(),
            },
        },
    );
    state.analysis.relations.insert(
        "edge".into(),
        Relation {
            id: "edge".into(),
            from: "rule".into(),
            to: "target".into(),
            from_target: RelationTarget::Record,
            to_target: RelationTarget::Record,
            from_record_sha256: digest(&state.analysis.records["rule"]).unwrap(),
            to_record_sha256: digest(&state.analysis.records["target"]).unwrap(),
            kind: RelationKind::References,
            state: RelationState::Explicit,
            scope: "project".into(),
            explanation: "selected target".into(),
            grounds: vec![citation(&input)],
        },
    );
    check.relationship_checks[0].relation_ids = vec!["edge".into()];
    check.relationship_checks[0].related_record_ids = vec!["target".into()];
    assert!(
        validate_relationship_checks(&input, &state, &check, &mut deps)
            .unwrap_err()
            .contains("independently inspect")
    );
    for key in ["record:target", "relation:edge"] {
        state.reviewer_coverage.candidate.insert(
            key.into(),
            digest(&context::reference(&state.analysis, key).unwrap()).unwrap(),
        );
    }
    validate_relationship_checks(&input, &state, &check, &mut deps).unwrap();
    state.analysis.relations.get_mut("edge").unwrap().state = RelationState::Unresolved;
    state.reviewer_coverage.candidate.insert(
        "relation:edge".into(),
        digest(&context::reference(&state.analysis, "relation:edge").unwrap()).unwrap(),
    );
    assert!(
        validate_relationship_checks(&input, &state, &check, &mut deps)
            .unwrap_err()
            .contains("resolved requires")
    );
    check.relationship_checks[0].status = RelationshipStatus::NotRequired;
    assert!(
        validate_relationship_checks(&input, &state, &check, &mut deps)
            .unwrap_err()
            .contains("not_required")
    );
}

#[test]
fn source_limited_requires_examining_available_continuation_without_forcing_resolution() {
    let (mut input, config, mut state) = relationship_fixture();
    let mut check = relationship_judgment(&input, &config, &state);
    input.source_units.push(Source {
        source_unit_revision_id: "continuation".into(),
        document_id: "document".into(),
        text: "Ambiguous continuation.".into(),
        locator: json!({}),
        ordinal: 1,
    });
    state.analysis.records.get_mut("rule").unwrap().data = RecordData::Unresolved {
        problem: "Unclear continuation target".into(),
        affected: vec![],
        candidates: vec!["continuation".into()],
    };
    state.reviewer_coverage.candidate.insert(
        "record:rule".into(),
        digest(&context::reference(&state.analysis, "record:rule").unwrap()).unwrap(),
    );
    check.relationship_checks = vec![RelationshipCheck {
        record_id: "rule".into(),
        status: RelationshipStatus::SourceLimited,
        related_record_ids: vec![],
        relation_ids: vec![],
        unresolved_record_ids: vec!["rule".into()],
        finding_ids: vec![],
        reason: "The referenced continuation still has two possible interpretations.".into(),
        sources: vec![citation(&input)],
    }];
    let mut deps = state.source_review.as_ref().unwrap().dependencies.clone();
    assert!(
        validate_relationship_checks(&input, &state, &check, &mut deps)
            .unwrap_err()
            .contains("frozen candidate source continuation")
    );
    check.relationship_checks[0].sources.push(Span {
        source_id: "continuation".into(),
        start: 0,
        end: input.source_units[1].text.len(),
        view_id: None,
        grid_cell: None,
    });
    assert!(
        validate_relationship_checks(&input, &state, &check, &mut deps).is_err(),
        "citation without independent delivery must fail"
    );
    tools::cover(
        state
            .reviewer_coverage
            .text
            .entry("continuation".into())
            .or_default(),
        0,
        input.source_units[1].text.len(),
    );
    validate_relationship_checks(&input, &state, &check, &mut deps).unwrap();
    assert!(!deps.source_ids.contains("continuation"));
    assert!(deps.references.contains("record:rule"));
    assert!(
        check.relationship_checks[0]
            .sources
            .iter()
            .any(|source| source.source_id == "continuation")
    );
    check.relationship_checks[0].status = RelationshipStatus::NotRequired;
    assert!(validate_relationship_checks(&input, &state, &check, &mut deps).is_err());
    check.relationship_checks.clear();
    check.status = JudgmentStatus::NeedsEvidence;
    validate_relationship_checks(&input, &state, &check, &mut deps).unwrap();
}

#[test]
fn relationship_review_can_cite_unresolved_evidence_without_an_affected_id() {
    let (input, config, mut state) = relationship_fixture();
    let unresolved = Record {
        id: "uncertain".into(),
        sources: vec![citation(&input)],
        data: RecordData::Unresolved {
            problem: "The source leaves this rule ambiguous.".into(),
            affected: vec![],
            candidates: vec![],
        },
    };
    state
        .analysis
        .records
        .insert(unresolved.id.clone(), unresolved);
    let key = "record:uncertain";
    state.reviewer_coverage.candidate.insert(
        key.into(),
        digest(&context::reference(&state.analysis, key).unwrap()).unwrap(),
    );
    let mut check = relationship_judgment(&input, &config, &state);
    check.relationship_checks[0].status = RelationshipStatus::SourceLimited;
    check.relationship_checks[0].unresolved_record_ids = vec!["uncertain".into()];
    let mut uncertainty = check.relationship_checks[0].clone();
    uncertainty.record_id = "uncertain".into();
    check.relationship_checks.push(uncertainty);
    let mut deps = state.source_review.as_ref().unwrap().dependencies.clone();
    validate_relationship_checks(&input, &state, &check, &mut deps).unwrap();
    // Shared evidence is required; unrelated unresolved items cannot be borrowed.
    state.analysis.records.get_mut("uncertain").unwrap().sources[0].source_id = "unrelated".into();
    state.reviewer_coverage.candidate.insert(
        key.into(),
        digest(&context::reference(&state.analysis, key).unwrap()).unwrap(),
    );
    let error = validate_relationship_checks(&input, &state, &check, &mut deps).unwrap_err();
    assert!(error.contains("/relationship_checks/0"));
}

#[test]
fn relationship_finding_can_use_cross_source_grounds_for_an_explicit_affected_field() {
    let (mut input, config, mut state) = relationship_fixture();
    let mut check = relationship_judgment(&input, &config, &state);
    input.source_units.push(Source {
        source_unit_revision_id: "selection".into(),
        document_id: "document".into(),
        text: "The selected condition governs the earlier rule.".into(),
        locator: json!({}),
        ordinal: 1,
    });
    let evidence = Span {
        source_id: "selection".into(),
        start: 0,
        end: input.source_units[1].text.len(),
        view_id: None,
        grid_cell: None,
    };
    tools::cover(
        state
            .reviewer_coverage
            .text
            .entry("selection".into())
            .or_default(),
        0,
        evidence.end,
    );
    state.review_draft.insert(
        "issue".into(),
        Finding {
            code: "wrong_selection".into(),
            message: "The rule does not incorporate the source selection.".into(),
            correction: "Apply the source selection.".into(),
            affected: vec![ReviewedField {
                id: "rule".into(),
                path: "/data/applicability".into(),
            }],
            sources: vec![evidence.clone()],
        },
    );
    check.status = JudgmentStatus::Findings;
    check.finding_ids = vec!["issue".into()];
    check.relationship_checks[0].status = RelationshipStatus::Findings;
    check.relationship_checks[0].finding_ids = vec!["issue".into()];
    let mut deps = state.source_review.as_ref().unwrap().dependencies.clone();
    validate_relationship_checks(&input, &state, &check, &mut deps).unwrap();
    assert!(!deps.source_ids.contains("selection"));
    assert!(deps.references.contains("record:rule"));
    state.analysis.records.insert(
        "other".into(),
        Record {
            id: "other".into(),
            sources: vec![evidence.clone()],
            data: RecordData::Fact {
                name: "Independent item on the cited page".into(),
                value: "Other source content".into(),
                scope: "selection".into(),
            },
        },
    );
    let unrelated = Finding {
        code: "other_issue".into(),
        message: "Separate issue on the cited page.".into(),
        correction: "Revise the other item.".into(),
        affected: vec![ReviewedField {
            id: "other".into(),
            path: "".into(),
        }],
        sources: vec![evidence],
    };
    state
        .review_draft
        .insert("unrelated".into(), unrelated.clone());
    let task = tasks(&input, config.limits.max_tool_result_bytes)
        .unwrap()
        .remove(0);
    assert_eq!(
        required_findings(
            &state,
            &task,
            &deps.source_ids,
            &references(&state.analysis, &deps)
        ),
        BTreeSet::from(["issue".into()]),
        "citing another page must not import its unrelated findings"
    );
    // A directly selected cross-source candidate still tracks finding
    // edits, even without querying its whole source collection.
    let selected = Dependencies {
        references: BTreeSet::from(["record:rule".into()]),
        ..Default::default()
    };
    let prior_version = version(&state, &selected).unwrap();
    let before = state.review_draft["issue"].clone();
    let mut after = before.clone();
    after
        .correction
        .push_str(" Preserve the selected condition.");
    finding_changed(&mut state, Some(&before), Some(&after)).unwrap();
    state.review_draft.insert("issue".into(), after);
    let changed_version = version(&state, &selected).unwrap();
    assert_ne!(prior_version, changed_version);
    finding_changed(&mut state, Some(&unrelated), None).unwrap();
    state.review_draft.remove("unrelated");
    assert_eq!(version(&state, &selected).unwrap(), changed_version);
    state
        .review_draft
        .get_mut("issue")
        .unwrap()
        .affected
        .clear();
    assert!(
        validate_relationship_checks(&input, &state, &check, &mut deps).is_err(),
        "unrelated evidence without an affected subject is not a relationship judgment"
    );
}

#[test]
fn relationship_finding_feedback_identifies_the_wrong_id_and_a_saved_subject_match() {
    let (mut input, config, mut state) = relationship_fixture();
    let mut check = relationship_judgment(&input, &config, &state);
    input.source_units.push(Source {
        source_unit_revision_id: "other".into(),
        document_id: "document".into(),
        text: "Unrelated source issue.".into(),
        locator: json!({}),
        ordinal: 1,
    });
    let other = Span {
        source_id: "other".into(),
        start: 0,
        end: input.source_units[1].text.len(),
        view_id: None,
        grid_cell: None,
    };
    tools::cover(
        state
            .reviewer_coverage
            .text
            .entry("other".into())
            .or_default(),
        0,
        other.end,
    );
    let correct = Finding {
        code: "source_issue".into(),
        message: "The rule is misinterpreted.".into(),
        correction: "Compare the source condition.".into(),
        affected: vec![ReviewedField {
            id: "rule".into(),
            path: "/data/applicability".into(),
        }],
        sources: vec![citation(&input)],
    };
    state.review_draft.insert("correct".into(), correct.clone());
    state.review_draft.insert(
        "wrong".into(),
        Finding {
            affected: vec![],
            sources: vec![other],
            ..correct
        },
    );
    check.status = JudgmentStatus::Findings;
    check.finding_ids = vec!["wrong".into(), "correct".into()];
    check.relationship_checks[0].status = RelationshipStatus::Findings;
    check.relationship_checks[0].finding_ids = vec!["wrong".into()];
    let mut deps = state.source_review.as_ref().unwrap().dependencies.clone();
    let before = digest(&state).unwrap();
    let error = validate_relationship_checks(&input, &state, &check, &mut deps).unwrap_err();
    let detail: Value = serde_json::from_str(
        error
            .strip_prefix("INVALID_FIELD /relationship_checks/0/finding_ids/0: ")
            .expect(&error),
    )
    .unwrap();
    assert_eq!(detail["record_id"], "rule");
    assert_eq!(detail["unrelated_finding_id"], "wrong");
    assert_eq!(detail["candidate_finding_id"], "correct");
    assert_eq!(before, digest(&state).unwrap());
    check.relationship_checks[0].finding_ids = vec!["correct".into()];
    validate_relationship_checks(&input, &state, &check, &mut deps).unwrap();
}

#[test]
fn relationship_findings_require_saved_subject_evidence_and_outer_retention() {
    let (input, config, mut state) = relationship_fixture();
    let mut check = relationship_judgment(&input, &config, &state);
    check.status = JudgmentStatus::Findings;
    check.relationship_checks[0].status = RelationshipStatus::Findings;
    check.relationship_checks[0].finding_ids = vec!["issue".into()];
    let mut deps = state.source_review.as_ref().unwrap().dependencies.clone();
    assert!(
        validate_relationship_checks(&input, &state, &check, &mut deps)
            .unwrap_err()
            .contains("save the relationship")
    );
    state.review_draft.insert(
        "issue".into(),
        Finding {
            code: "missing_reference".into(),
            message: "Missing source-required edge.".into(),
            correction: "Inspect the selected target and preserve the reference.".into(),
            affected: vec![ReviewedField {
                id: "rule".into(),
                path: String::new(),
            }],
            sources: vec![citation(&input)],
        },
    );
    assert!(
        validate_relationship_checks(&input, &state, &check, &mut deps)
            .unwrap_err()
            .contains("outer finding_ids")
    );
    check.finding_ids = vec!["issue".into()];
    validate_relationship_checks(&input, &state, &check, &mut deps).unwrap();
    state.review_draft.get_mut("issue").unwrap().sources[0].source_id = "unrelated".into();
    assert!(validate_relationship_checks(&input, &state, &check, &mut deps).is_err());
    // A content finding does not force a fabricated relationship problem.
    check.relationship_checks[0].status = RelationshipStatus::NotRequired;
    check.relationship_checks[0].finding_ids.clear();
    validate_relationship_checks(&input, &state, &check, &mut deps).unwrap();
}

#[test]
#[ignore = "requires KB_TENDER_CONTEXT_REPLAY_DIR and KB_TENDER_CONTEXT_REPORT; offline only"]
fn archived_clean_source_receipts_require_explicit_relationship_judgments() {
    let root = std::path::PathBuf::from(std::env::var("KB_TENDER_CONTEXT_REPLAY_DIR").unwrap());
    let original = std::fs::read(root.join("extraction/checkpoint.json")).unwrap();
    let state: Checkpoint = serde_json::from_slice(&original).unwrap();
    let input: FrozenInput =
        serde_json::from_slice(&std::fs::read(root.join("source/frozen-input.json")).unwrap())
            .unwrap();
    let config: Config =
        serde_json::from_slice(&std::fs::read(root.join("extraction/runtime.json")).unwrap())
            .unwrap();
    let mut reopened = Vec::new();
    for task in tasks(&input, config.limits.max_tool_result_bytes).unwrap() {
        let receipt = &state.source_review.as_ref().unwrap().results[&task.id];
        let mut dependencies = receipt.dependencies.clone();
        let required =
            relationship_records(&state.analysis, &references(&state.analysis, &dependencies));
        if required.is_empty() {
            continue;
        }
        let error =
            validate_relationship_checks(&input, &state, &receipt.judgment, &mut dependencies)
                .unwrap_err();
        assert!(error.contains("/relationship_checks"));
        assert!(!complete(&input, &state, &task).unwrap());
        reopened.push(json!({"task_id":task.id,"source_id":task.source_id,"required_record_ids":required,"error":error}));
    }
    assert!(!reopened.is_empty());
    assert_eq!(
        original,
        std::fs::read(root.join("extraction/checkpoint.json")).unwrap()
    );
    std::fs::write(std::env::var("KB_TENDER_CONTEXT_REPORT").unwrap(),serde_json::to_vec_pretty(&json!({
        "mode":"offline receipt gate projection; not a cross-contract checkpoint resume",
        "turn":state.turn,"records":state.analysis.records.len(),"relations":state.analysis.relations.len(),
        "reopened":reopened,"checkpoint_unchanged":true,"semantic_acceptance":false})).unwrap()).unwrap();
}

#[test]
#[ignore = "requires an archived source judgment replay directory"]
fn checkpoint_mapping_judgment_preserves_nested_source_evidence() {
    let directory = std::path::PathBuf::from(
        std::env::var("KB_TENDER_MAPPING_REPLAY_DIR").expect("explicit replay directory"),
    );
    let read = |name| std::fs::read(directory.join(name)).unwrap();
    let input: FrozenInput = serde_json::from_slice(&read("frozen-input.json")).unwrap();
    let state: Checkpoint = serde_json::from_slice(&read("checkpoint.json")).unwrap();
    let judgment: Judgment = serde_json::from_slice(&read("judgment.json")).unwrap();
    assert!(
        judgment.template_mappings.iter().any(|mapping| {
            mapping.finding_ids.iter().any(|id| {
                let finding = &state.review_draft[id];
                !finding
                    .sources
                    .iter()
                    .any(|source| mapping.sources.contains(source))
                    && finding.sources.iter().any(|source| {
                        mapping
                            .sources
                            .iter()
                            .any(|mapped| shared_mapping_evidence(source, mapped))
                    })
            })
        }),
        "the archived judgment must reproduce the exact-span false rejection"
    );
    let before = digest(&state).unwrap();
    let mut dependencies = state.source_review.as_ref().unwrap().dependencies.clone();
    validate_template_mappings(&input, &state, &judgment, &mut dependencies).unwrap();
    assert_eq!(digest(&state).unwrap(), before);
}

#[test]
#[ignore = "requires KB_TENDER_MAPPING_REPLAY_DIR; offline projection only"]
fn archived_cross_source_finding_admission() {
    let root = std::path::PathBuf::from(std::env::var("KB_TENDER_MAPPING_REPLAY_DIR").unwrap());
    let read = |name| std::fs::read(root.join(name)).unwrap();
    let original = read("checkpoint.json");
    let mut state: Checkpoint = serde_json::from_slice(&original).unwrap();
    let input: FrozenInput = serde_json::from_slice(&read("frozen-input.json")).unwrap();
    let config: Config = serde_json::from_slice(&read("runtime.json")).unwrap();
    let raw: Value = serde_json::from_slice(&read("judgment.json")).unwrap();
    let mut args = evidence_refs::expand(&input, &raw).unwrap();
    let mut judgment: Judgment = serde_json::from_value(args.clone()).unwrap();
    let task = task_inventory(&input, &state, config.limits.max_tool_result_bytes)
        .unwrap()
        .into_iter()
        .find(|task| task.id == judgment.task_id)
        .unwrap();
    // Reapply the archived model argument to a later candidate snapshot in
    // memory. Only the active task/dependency version is projected; no
    // original/candidate reading or comparison outcome is manufactured.
    let mut dependencies = task_dependencies(&task);
    dependencies
        .references
        .extend(judgment.candidate_refs.iter().cloned());
    expand_view_dependencies(&input, &state.reviewer_coverage, &mut dependencies);
    judgment.expected_version = version(&state, &dependencies).unwrap();
    args["expected_version"] = json!(judgment.expected_version);
    let review = state.source_review.as_mut().unwrap();
    review.active_task = Some(task.id.clone());
    review.dependencies = dependencies.clone();
    let owned = obligations(&input, &state, &task);
    let old_relevant = required_findings(&state, &task, &owned.sources, &owned.comparisons);
    let outer: BTreeSet<_> = judgment.finding_ids.iter().cloned().collect();
    let formerly_rejected: Vec<_> = outer.difference(&old_relevant).cloned().collect();
    assert!(!formerly_rejected.is_empty());
    validate_template_subjects(
        &input,
        &state,
        &judgment,
        &mut dependencies,
        &owned.subjects,
    )
    .unwrap();
    validate_relationship_subjects(
        &input,
        &state,
        &judgment,
        &mut dependencies,
        &owned.subjects,
    )
    .unwrap();
    let submitted = put(&input, &config, &mut state, &args).unwrap();
    assert!(complete(&input, &state, &task).unwrap());
    assert_eq!(read("checkpoint.json"), original);
    std::fs::write(root.join("archived-replay.json"), serde_json::to_vec_pretty(&json!({
        "mode":"archived model argument on later checkpoint; active task and expected version projected in memory",
        "checkpoint_turn":state.turn,"task_id":task.id,"formerly_rejected_findings":formerly_rejected,
        "result":submitted,"source_judgment_complete":true,"reading_receipts_unchanged":true,
        "original_checkpoint_unchanged":true,"provider_calls":0,"full_acceptance":false
    })).unwrap()).unwrap();
}

#[test]
#[ignore = "requires KB_TENDER_MAPPING_REPLAY_DIR; offline feedback projection only"]
fn archived_mapping_feedback_locates_saved_original_evidence() {
    let root = std::path::PathBuf::from(std::env::var("KB_TENDER_MAPPING_REPLAY_DIR").unwrap());
    let read = |name| std::fs::read(root.join(name)).unwrap();
    let original = read("checkpoint.json");
    let state: Checkpoint = serde_json::from_slice(&original).unwrap();
    let input: FrozenInput = serde_json::from_slice(&read("frozen-input.json")).unwrap();
    let config: Config = serde_json::from_slice(&read("runtime.json")).unwrap();
    let raw: Value = serde_json::from_slice(&read("judgment.json")).unwrap();
    let mut judgment: Judgment =
        serde_json::from_value(evidence_refs::expand(&input, &raw).unwrap()).unwrap();
    let task = task_inventory(&input, &state, config.limits.max_tool_result_bytes)
        .unwrap()
        .into_iter()
        .find(|task| task.id == judgment.task_id)
        .unwrap();
    let mut dependencies = task_dependencies(&task);
    dependencies
        .references
        .extend(judgment.candidate_refs.iter().cloned());
    expand_view_dependencies(&input, &state.reviewer_coverage, &mut dependencies);
    let owned = obligations(&input, &state, &task);
    let before = digest(&state).unwrap();
    let mut feedback = Vec::new();
    for _ in 0..=judgment.template_mappings.len() {
        let error = match validate_template_subjects(
            &input,
            &state,
            &judgment,
            &mut dependencies.clone(),
            &owned.subjects,
        ) {
            Ok(()) => break,
            Err(error) => error,
        };
        let (path, detail) = error.split_once(": ").unwrap();
        let detail: Value = serde_json::from_str(detail).unwrap();
        let id = detail["finding_id"].as_str().unwrap();
        let source: Span = serde_json::from_value(
            evidence_refs::expand(&input, &detail["finding_source_example"]).unwrap(),
        )
        .unwrap();
        assert_eq!(source, state.review_draft[id].sources[0]);
        tools::validate_span(&input, &state.reviewer_coverage, &source).unwrap();
        let index: usize = path.split('/').nth(2).unwrap().parse().unwrap();
        assert!(!judgment.template_mappings[index].sources.contains(&source));
        judgment.template_mappings[index].sources.push(source);
        feedback.push(detail);
    }
    assert!(!feedback.is_empty());
    validate_template_subjects(
        &input,
        &state,
        &judgment,
        &mut dependencies,
        &owned.subjects,
    )
    .unwrap();
    assert_eq!(digest(&state).unwrap(), before);
    assert_eq!(read("checkpoint.json"), original);
    std::fs::write(root.join("feedback-replay.json"), serde_json::to_vec_pretty(&json!({
        "mode":"offline projection: actual rejected model mapping arguments plus already-read saved source examples; no model repair or source judgment submitted",
        "checkpoint_turn":state.turn,"feedback":feedback,"mapping_validation_after_citation_projection":true,
        "checkpoint_and_reading_receipts_unchanged":true,"provider_calls":0,"full_acceptance":false
    })).unwrap()).unwrap();
}

#[test]
fn mapping_evidence_preserves_source_and_modality_boundaries() {
    let text = Span {
        start: 3,
        end: 9,
        ..citation(&fixture().0)
    };
    for other in [
        Span {
            source_id: "other".into(),
            ..text.clone()
        },
        Span {
            start: 0,
            end: 6,
            ..text.clone()
        },
        Span {
            start: 9,
            end: 12,
            ..text.clone()
        },
        Span {
            start: 3,
            end: 3,
            ..text.clone()
        },
    ] {
        assert!(!shared_mapping_evidence(&text, &other));
    }
    for evidence in [
        json!({"source_id":"source","start":0,"end":0,"grid_cell":{"form_id":"form","row":0,"column":0}}),
        json!({"source_id":"source","start":0,"end":0,"view_id":"view"}),
    ] {
        let span: Span = serde_json::from_value(evidence.clone()).unwrap();
        assert!(shared_mapping_evidence(&span, &span));
        assert!(!shared_mapping_evidence(&span, &text));
        let mut changed = evidence.clone();
        changed["source_id"] = json!("other");
        assert!(!shared_mapping_evidence(
            &span,
            &serde_json::from_value(changed).unwrap()
        ));
        let mut changed = evidence;
        if span.grid_cell.is_some() {
            changed["grid_cell"]["column"] = json!(1);
        } else {
            changed["view_id"] = json!("other-view");
        }
        assert!(!shared_mapping_evidence(
            &span,
            &serde_json::from_value(changed).unwrap()
        ));
    }
}

#[test]
#[ignore = "requires KB_TENDER_CONTEXT_REPLAY_DIR and KB_TENDER_CONTEXT_REPORT; offline only"]
fn archived_mixed_layout_receipts_require_original_pixels() {
    let root = std::path::PathBuf::from(std::env::var("KB_TENDER_CONTEXT_REPLAY_DIR").unwrap());
    let original = std::fs::read(root.join("extraction/checkpoint.json")).unwrap();
    let state: Checkpoint = serde_json::from_slice(&original).unwrap();
    let input: FrozenInput =
        serde_json::from_slice(&std::fs::read(root.join("source/frozen-input.json")).unwrap())
            .unwrap();
    let config: Config =
        serde_json::from_slice(&std::fs::read(root.join("extraction/runtime.json")).unwrap())
            .unwrap();
    let review = state.source_review.as_ref().unwrap();
    let mut reopened = Vec::new();
    for task in tasks(&input, config.limits.max_tool_result_bytes).unwrap() {
        let Some(receipt) = review.results.get(&task.id) else {
            continue;
        };
        if requires_layout_view(&input, &state, &task, &receipt.dependencies) {
            let error = validate_layout_view(
                &input,
                &state,
                &task,
                &receipt.dependencies,
                &receipt.judgment,
            )
            .unwrap_err();
            assert!(!complete(&input, &state, &task).unwrap());
            reopened.push(json!({"source_id":task.source_id,"prior_status":receipt.judgment.status,"error":error}));
        }
    }
    assert!(
        !reopened.is_empty(),
        "archive must reproduce missing mixed-layout evidence"
    );
    assert_eq!(
        std::fs::read(root.join("extraction/checkpoint.json")).unwrap(),
        original
    );
    std::fs::write(std::env::var("KB_TENDER_CONTEXT_REPORT").unwrap(),serde_json::to_vec_pretty(&json!({
        "mode":"offline gate projection only; not a checkpoint resumed under a new tool contract",
        "turn":state.turn,"reopened":reopened,"checkpoint_unchanged":true,"semantic_acceptance":false
    })).unwrap()).unwrap();
}

#[test]
fn whole_page_view_keeps_sibling_candidates_in_review_scope_after_restore() {
    let (mut input, config, mut state) = fixture();
    input.source_units[0].locator = json!({"page_ordinal":3});
    for (id, document, page) in [
        ("note", "document", 3),
        ("other-page", "document", 4),
        ("other-document", "different", 3),
    ] {
        input.source_units.push(Source {
            source_unit_revision_id: id.into(),
            document_id: document.into(),
            text: "表下注释".into(),
            locator: json!({"page_ordinal":page}),
            ordinal: input.source_units.len(),
        });
        state.analysis.records.insert(
            id.into(),
            Record {
                id: id.into(),
                sources: vec![Span {
                    source_id: id.into(),
                    start: 0,
                    end: "表下注释".len(),
                    view_id: None,
                    grid_cell: None,
                }],
                data: RecordData::Fact {
                    name: "说明".into(),
                    value: "原文已提取".into(),
                    scope: "表格".into(),
                },
            },
        );
    }
    state.source_review = Some(initialize(&input, &config).unwrap());
    select_next(&input, &config, &mut state).unwrap();
    compare(&input, &config, &mut state);
    let args = judgment(&input, &config, &state);
    put(&input, &config, &mut state, &args).unwrap();
    let old_task = tasks(&input, config.limits.max_tool_result_bytes)
        .unwrap()
        .remove(0);
    assert!(complete(&input, &state, &old_task).unwrap());
    let mut queried = state.clone();
    record_query(
        &input,
        &mut queried,
        "read_source_view",
        &json!({"source_id":"source"}),
    );
    assert!(
        queried
            .source_review
            .as_ref()
            .unwrap()
            .dependencies
            .source_ids
            .contains("note")
    );
    assert!(
        queried.reviewer_coverage.views.is_empty(),
        "query scope is not a delivered image receipt"
    );
    state.reviewer_coverage.views.insert(
        "page".into(),
        views::ViewIdentity {
            source_id: "source".into(),
            original_sha256: "a".repeat(64),
            image_sha256: "b".repeat(64),
            page_ordinal: 3,
            width: 1,
            height: 1,
            renderer: "docreader-source-view-v1/test".into(),
        },
    );
    // Simulate a restored old checkpoint whose view receipt is present but
    // dependencies still only name the table source, not the page note.
    state = serde_json::from_value(json!(state)).unwrap();
    let coverage = digest(&state.reviewer_coverage).unwrap();
    assert!(!complete(&input, &state, &old_task).unwrap());
    select_next(&input, &config, &mut state).unwrap();
    let current = packet(&input, &config, &state).unwrap();
    let pending = current["current"]["pending_candidate_refs"]["items"]
        .as_array()
        .unwrap();
    assert!(pending.contains(&json!("record:note")));
    assert!(!pending.contains(&json!("record:other-page")));
    assert!(!pending.contains(&json!("record:other-document")));
    assert_eq!(digest(&state.reviewer_coverage).unwrap(), coverage);
    assert!(!state.reviewer_coverage.text.contains_key("note"));
    assert!(
        !state
            .reviewer_coverage
            .candidate
            .contains_key("record:note")
    );
}

#[test]
fn split_same_page_templates_reopen_a_text_only_layout_receipt() {
    let (mut input, config, mut state) = fixture();
    input.source_units[0].locator = json!({"page_ordinal":7});
    input.source_units.push(Source {
        source_unit_revision_id: "grid-source".into(),
        document_id: "document".into(),
        text: String::new(),
        locator: json!({"page_ordinal":7}),
        ordinal: 1,
    });
    input.structured_forms.push(
        json!({"form_definition_revision_id":"grid","source_unit_revision_id":"grid-source",
        "definition":{"kind":"grid","row_count":1,"column_count":1,"widths_mm":[40],
            "cells":[{"row":0,"column":0,"row_span":1,"col_span":1,"text":"金额"}]}}),
    );
    state.source_review = Some(initialize(&input, &config).unwrap());
    select_next(&input, &config, &mut state).unwrap();
    let record: Record = serde_json::from_value(json!({"id":"title","sources":[citation(&input)],"data":{
        "kind":"template","label":"title","title":"Table heading and note","parent":null,"order":null,
        "purpose":"submission","applicability":{"state":"applicable","condition":"submission","scope":"bid","grounds":[citation(&input)]},
        "regions":[{"source":citation(&input),"role":"fixed_text","form_id":null,"cells":[],"instruction":"heading and note"}]}})).unwrap();
    state
        .reviewer_coverage
        .candidate
        .insert("record:title".into(), digest(&record).unwrap());
    state
        .analysis
        .records
        .insert(record.id.clone(), record.clone());
    context::complete_review_check(
        &input,
        &mut state,
        &json!({"reference":"record:title",
        "summary":"Compared current text template","sources":[citation(&input)]}),
        config.limits.max_tool_result_bytes,
    )
    .unwrap();
    compare(&input, &config, &mut state);
    let mut args = judgment(&input, &config, &state);
    args["template_mappings"] = json!([{"template_id":"title","requirement_ids":[],"relation_ids":[],
        "finding_ids":[],"reason":"Standalone text template","sources":[citation(&input)]}]);
    put(&input, &config, &mut state, &args).unwrap();
    let task = tasks(&input, config.limits.max_tool_result_bytes)
        .unwrap()
        .remove(0);
    assert!(complete(&input, &state, &task).unwrap());

    let grid_span = json!({"source_id":"grid-source","start":0,"end":0,
        "grid_cell":{"form_id":"grid","row":0,"column":0}});
    let mut grid = json!(record);
    grid["id"] = json!("table");
    grid["sources"] = json!([grid_span]);
    grid["data"]["regions"] = json!([{"source":grid_span,"role":"fixed_text","form_id":"grid",
        "cells":[{"row":0,"column":0}],"instruction":"table body"}]);
    state
        .analysis
        .records
        .insert("table".into(), serde_json::from_value(grid).unwrap());
    let dependencies = &state.source_review.as_ref().unwrap().dependencies;
    assert!(
        requires_layout_view(&input, &state, &task, dependencies),
        "separate text and table records on the same original page still need visual order checking"
    );
    assert!(
        !complete(&input, &state, &task).unwrap(),
        "a prior text-only receipt cannot approve the newly split mixed layout"
    );
    let before = digest(&state).unwrap();
    let judgment: Judgment = serde_json::from_value(args).unwrap();
    assert!(validate_layout_view(&input, &state, &task, dependencies, &judgment).is_err());
    assert_eq!(digest(&state).unwrap(), before);

    input.source_units[1].locator["page_ordinal"] = json!(8);
    assert!(!requires_layout_view(&input, &state, &task, dependencies));
    input.source_units[1].locator["page_ordinal"] = json!(7);
    input.source_units[1].document_id = "other-document".into();
    assert!(!requires_layout_view(&input, &state, &task, dependencies));
    input.source_units[1].document_id = "document".into();
    if let RecordData::Template { applicability, .. } =
        &mut state.analysis.records.get_mut("table").unwrap().data
    {
        applicability.state = ApplicabilityState::NotApplicable;
    }
    assert!(!requires_layout_view(
        &input,
        &state,
        &task,
        &state.source_review.as_ref().unwrap().dependencies
    ));
}

#[test]
fn mixed_layout_cannot_be_checked_without_independent_page_pixels() {
    let (mut input, config, mut state) = fixture();
    input.source_units[0].locator = json!({"page_ordinal":7});
    state.source_review = Some(initialize(&input, &config).unwrap());
    select_next(&input, &config, &mut state).unwrap();
    let record: Record = serde_json::from_value(json!({
        "id":"layout","sources":[citation(&input)],"data":{
            "kind":"template","label":"form","title":"Mixed layout","parent":null,
            "order":null,"purpose":"submission","applicability":{
                "state":"applicable","condition":"submission","scope":"bid","grounds":[citation(&input)]},
            "regions":[
                {"source":citation(&input),"role":"fixed_text","form_id":null,"cells":[],"instruction":"title"},
                {"source":citation(&input),"role":"fixed_text","form_id":"grid","cells":[],"instruction":"table"}
            ]}})).unwrap();
    state
        .reviewer_coverage
        .candidate
        .insert("record:layout".into(), digest(&record).unwrap());
    state.analysis.records.insert(record.id.clone(), record);
    context::complete_review_check(&input,&mut state,&json!({
        "reference":"record:layout","summary":"Compared parsed candidate","sources":[citation(&input)]
    }),config.limits.max_tool_result_bytes).unwrap();
    compare(&input, &config, &mut state);
    let mut args = judgment(&input, &config, &state);
    args["template_mappings"] = json!([{"template_id":"layout","requirement_ids":[],"relation_ids":[],
        "finding_ids":[],"reason":"Standalone original form","sources":[citation(&input)]}]);
    let before = digest(&state).unwrap();
    let error = put(&input, &config, &mut state, &args).unwrap_err();
    assert!(error.contains("original page pixels"), "{error}");
    assert_eq!(digest(&state).unwrap(), before);
    assert_eq!(
        packet(&input, &config, &state).unwrap()["current"]["layout_view"]["source_id"],
        "source"
    );
    let mut waiting = state.clone();
    let mut waiting_args = args.clone();
    waiting_args["status"] = json!("needs_evidence");
    waiting_args["evidence_requests"] =
        json!([{"question":"Original layout pixels unavailable","source_ids":["source"]}]);
    put(&input, &config, &mut waiting, &waiting_args).unwrap();
    assert_eq!(pending(&input, &config, &waiting).unwrap().len(), 1);

    // A primary receipt or an image merely cached by a read cannot satisfy
    // the independent review ledger, even when its geometry is correct.
    let view = views::ViewIdentity {
        source_id: "source".into(),
        original_sha256: "a".repeat(64),
        image_sha256: "b".repeat(64),
        page_ordinal: 7,
        width: 1,
        height: 1,
        renderer: "docreader-source-view-v1/test".into(),
    };
    state
        .analysis
        .coverage
        .views
        .insert("page".into(), view.clone());
    state.source_views.insert(
        "page".into(),
        views::SourceView {
            identity: view.clone(),
            jpeg_base64: String::new(),
        },
    );
    args["sources"]
        .as_array_mut()
        .unwrap()
        .push(json!({"source_id":"source","start":0,"end":0,"view_id":"page"}));
    assert!(put(&input, &config, &mut state, &args).is_err());
    state.reviewer_coverage.views.insert("page".into(), view);
    put(&input, &config, &mut state, &args).unwrap();
    assert!(pending(&input, &config, &state).unwrap().is_empty());
    let mut alternate_input = input.clone();
    let mut neighbor = input.source_units[0].clone();
    neighbor.source_unit_revision_id = "neighbor".into();
    alternate_input.source_units.push(neighbor);
    let mut alternate_state = state.clone();
    alternate_state
        .reviewer_coverage
        .views
        .get_mut("page")
        .unwrap()
        .source_id = "neighbor".into();
    let mut alternate_args = args.clone();
    alternate_args["sources"][1]["source_id"] = json!("neighbor");
    let alternate_judgment: Judgment = serde_json::from_value(alternate_args).unwrap();
    let task = tasks(&input, config.limits.max_tool_result_bytes)
        .unwrap()
        .remove(0);
    let dependencies = &state.source_review.as_ref().unwrap().dependencies;
    assert!(
        validate_layout_view(
            &alternate_input,
            &alternate_state,
            &task,
            dependencies,
            &alternate_judgment
        )
        .is_ok(),
        "the same original page may be read using a different parsed source identity"
    );
    alternate_input.source_units[1].document_id = "other-document".into();
    assert!(
        validate_layout_view(
            &alternate_input,
            &alternate_state,
            &task,
            dependencies,
            &alternate_judgment
        )
        .is_err()
    );
    alternate_input.source_units[1].document_id = input.source_units[0].document_id.clone();
    alternate_input.source_units[1].locator["page_ordinal"] = json!(8);
    assert!(
        validate_layout_view(
            &alternate_input,
            &alternate_state,
            &task,
            dependencies,
            &alternate_judgment
        )
        .is_err()
    );
    state.reviewer_coverage.views.clear();
    let restored: Checkpoint = serde_json::from_value(json!(state)).unwrap();
    assert_eq!(
        pending(&input, &config, &restored).unwrap().len(),
        1,
        "restored source receipts cannot bypass the required independent visual evidence"
    );
}

#[test]
fn source_packet_highlights_templates_without_a_recorded_requirement_mapping() {
    let (input, config, mut state) = fixture();
    let applicability = json!({"state":"applicable","condition":"submission","scope":"bid","grounds":[citation(&input)]});
    for value in [
        json!({"id":"requirement","sources":[citation(&input)],"data":{
            "kind":"requirement","text":"Use the prescribed form","categories":["format"],
            "strength":"mandatory","compliance":[],"applicability":applicability,
            "response":[{"channel":"structured_form","description":"Prescribed form","condition":"submission","grounds":[citation(&input)]}],
            "scoring_rule":null,"proofs":[],"criteria":[]}}),
        json!({"id":"template","sources":[citation(&input)],"data":{
            "kind":"template","label":"Form","title":"Prescribed form","parent":null,"order":null,
            "purpose":"submission","applicability":applicability,
            "regions":[{"source":citation(&input),"role":"fixed_text","form_id":null,"cells":[],"instruction":"Preserve wording"}]}}),
    ] {
        let record: Record = serde_json::from_value(value).unwrap();
        state.analysis.records.insert(record.id.clone(), record);
    }
    let before = digest(&state).unwrap();
    let work = packet(&input, &config, &state).unwrap();
    assert_eq!(
        work["current"]["templates_without_requirement_mapping"]["items"],
        json!(["record:template"])
    );
    assert_eq!(
        before,
        digest(&state).unwrap(),
        "navigation is not evidence or a finding"
    );
    let relation: Relation = serde_json::from_value(json!({
        "id":"mapping","from":"requirement","to":"template",
        "from_target":{"kind":"response","index":0},"to_target":{"kind":"record"},
        "from_record_sha256":digest(&state.analysis.records["requirement"]).unwrap(),
        "to_record_sha256":digest(&state.analysis.records["template"]).unwrap(),
        "kind":"requires_template","state":"explicit","scope":"submission",
        "explanation":"Use the prescribed form","grounds":[citation(&input)]
    }))
    .unwrap();
    state
        .analysis
        .relations
        .insert(relation.id.clone(), relation.clone());
    let mut queried = state.clone();
    record_query(
        &input,
        &mut queried,
        "inspect_analysis",
        &json!({
            "kind":"all","ids":["requirement","mapping","source"]
        }),
    );
    let refs = &queried
        .source_review
        .as_ref()
        .unwrap()
        .dependencies
        .references;
    assert!(refs.contains("record:requirement"));
    assert!(refs.contains("relation:mapping"));
    assert!(refs.contains("disposition:source"));
    assert!(!refs.contains("record:mapping") && !refs.contains("record:source"));
    assert_eq!(
        packet(&input, &config, &state).unwrap()["current"]["templates_without_requirement_mapping"]
            ["total"],
        0
    );
    state.analysis.relations.clear();
    assert_eq!(
        packet(&input, &config, &state).unwrap()["current"]["templates_without_requirement_mapping"]
            ["total"],
        1
    );
    assert!(!state.done && state.review_draft.is_empty());

    // Reading and comparing both records must not approve a missing edge
    // without an explicit source-grounded mapping judgment.
    for key in [
        "record:requirement",
        "record:template",
        "disposition:source",
    ] {
        state.reviewer_coverage.candidate.insert(
            key.into(),
            digest(&context::reference(&state.analysis, key).unwrap()).unwrap(),
        );
        context::complete_review_check(
            &input,
            &mut state,
            &json!({
                "reference":key,"summary":"The existing record matches the source.",
                "sources":[citation(&input)]
            }),
            config.limits.max_tool_result_bytes,
        )
        .unwrap();
    }
    let args = judgment(&input, &config, &state);
    assert!(
        put(&input, &config, &mut state, &args)
            .unwrap_err()
            .contains("/template_mappings")
    );
    let mut judgment: Judgment = serde_json::from_value(args).unwrap();
    judgment.template_mappings.push(TemplateMapping {
        template_id: "template".into(),
        requirement_ids: vec!["requirement".into()],
        relation_ids: vec![],
        finding_ids: vec![],
        reason: "The original requires this output in the prescribed form.".into(),
        sources: vec![citation(&input)],
    });
    let dependencies = state.source_review.as_ref().unwrap().dependencies.clone();
    let validate = |state: &Checkpoint, judgment: &Judgment| {
        validate_template_mappings(&input, state, judgment, &mut dependencies.clone())
    };
    assert!(
        validate(&state, &judgment)
            .unwrap_err()
            .contains("mapping is absent")
    );

    // A missing edge can be reported, but cannot silently become checked.
    state.review_draft.insert(
        "missing".into(),
        Finding {
            code: "MISSING_MAPPING".into(),
            message: "Required mapping absent".into(),
            correction: "Map the source requirement to its prescribed template".into(),
            affected: vec![],
            sources: vec![citation(&input)],
        },
    );
    judgment.finding_ids.push("missing".into());
    judgment.template_mappings[0]
        .finding_ids
        .push("missing".into());
    validate(&state, &judgment).unwrap();
    // A finding may quote the exact clause while the mapping includes its
    // surrounding paragraph. Both cite the same independently read text.
    state.review_draft.get_mut("missing").unwrap().sources[0].end = 6;
    validate(&state, &judgment).unwrap();
    judgment.template_mappings[0].sources[0].start = 6;
    assert!(
        validate(&state, &judgment).is_err(),
        "adjacent clauses are not shared evidence"
    );
    judgment.template_mappings[0].sources[0] = citation(&input);
    state.review_draft.get_mut("missing").unwrap().sources[0] = citation(&input);
    judgment.template_mappings[0].sources[0].end = 6;
    validate(&state, &judgment).unwrap();
    judgment.template_mappings[0].sources[0] = citation(&input);
    judgment.finding_ids.clear();
    assert!(validate(&state, &judgment).is_err());
    judgment.template_mappings[0].finding_ids.clear();
    state.review_draft.clear();

    state
        .analysis
        .relations
        .insert(relation.id.clone(), relation.clone());
    judgment.template_mappings[0]
        .relation_ids
        .push("mapping".into());
    assert!(
        validate(&state, &judgment)
            .unwrap_err()
            .contains("independently inspect")
    );
    state.reviewer_coverage.candidate.insert(
        "relation:mapping".into(),
        digest(&context::reference(&state.analysis, "relation:mapping").unwrap()).unwrap(),
    );
    validate(&state, &judgment).unwrap();

    // A correct mapping can coexist with an unrelated template-content
    // finding. Misfiling that issue must not ask for an invented mapping
    // problem when removing it from this nested list is sufficient.
    let mut saved = state.clone();
    for key in [
        "record:requirement",
        "record:template",
        "relation:mapping",
        "disposition:source",
    ] {
        context::complete_review_check(&input, &mut saved, &json!({
            "reference":key,"summary":"Synthetic original and mapping comparison.","sources":[citation(&input)]
        }),config.limits.max_tool_result_bytes).unwrap();
    }
    let mut complete_judgment = judgment.clone();
    complete_judgment.expected_version =
        version(&saved, &saved.source_review.as_ref().unwrap().dependencies).unwrap();
    put(&input, &config, &mut saved, &json!(complete_judgment)).unwrap();
    let task = task_inventory(&input, &saved, config.limits.max_tool_result_bytes)
        .unwrap()
        .remove(0);
    assert!(complete(&input, &saved, &task).unwrap());
    saved
        .source_review
        .as_mut()
        .unwrap()
        .results
        .get_mut(&task.id)
        .unwrap()
        .judgment
        .template_mappings
        .clear();
    assert!(
        !complete(&input, &saved, &task).unwrap(),
        "a historical receipt without its required mapping judgment cannot finish the source"
    );

    let mut content_source = citation(&input);
    content_source.start = 6;
    state.review_draft.insert(
        "content".into(),
        Finding {
            code: "TEMPLATE_CONTENT".into(),
            message: "The template removes fixed source wording".into(),
            correction: "Preserve the original fixed wording".into(),
            affected: vec![],
            sources: vec![content_source],
        },
    );
    judgment.finding_ids.push("content".into());
    judgment.template_mappings[0].sources[0].end = 6;
    validate(&state, &judgment).unwrap();
    judgment.template_mappings[0]
        .finding_ids
        .push("content".into());
    let error = validate(&state, &judgment).unwrap_err();
    assert!(error.contains("/template_mappings/0/finding_ids/0"));
    assert!(error.contains("finding_ids=[]"));
    let detail: Value = serde_json::from_str(error.split_once(": ").unwrap().1).unwrap();
    assert_eq!(detail["finding_id"], "content");
    assert_eq!(
        detail["finding_source_example"],
        evidence_refs::compact(&input, &state.review_draft["content"].sources[0]).unwrap()
    );
    let recalled = agent::inspect_review(
        &state,
        &detail["inspect_review"],
        config.limits.max_tool_result_bytes,
    )
    .unwrap();
    assert_eq!(
        recalled["items"][0]["finding"],
        json!(state.review_draft["content"])
    );
    judgment.template_mappings[0].finding_ids.clear();
    validate(&state, &judgment).unwrap();
    assert_eq!(judgment.finding_ids, ["content"]);
    assert!(state.review_draft.contains_key("content"));
    judgment.finding_ids.clear();
    state.review_draft.clear();
    judgment.template_mappings[0].sources[0] = citation(&input);

    state.analysis.relations.get_mut("mapping").unwrap().kind = RelationKind::References;
    assert!(
        validate(&state, &judgment)
            .unwrap_err()
            .contains("relation must map")
    );

    // Standalone material remains legal; no edge is manufactured to satisfy
    // a fixed count. This semantic assertion still needs read evidence.
    judgment.template_mappings[0].requirement_ids.clear();
    judgment.template_mappings[0].relation_ids.clear();
    judgment.template_mappings[0].reason =
        "Source material has no separate output obligation.".into();
    validate(&state, &judgment).unwrap();
    let mut unread = state.clone();
    unread.reviewer_coverage.text.clear();
    assert!(validate(&unread, &judgment).is_err());
    judgment
        .template_mappings
        .push(judgment.template_mappings[0].clone());
    assert!(validate(&state, &judgment).unwrap_err().contains("once"));
}

#[test]
fn cross_source_nested_findings_can_finish_the_assigned_judgment() {
    let (mut input, config, mut state) = fixture();
    input.source_units.push(Source {
        source_unit_revision_id: "governing".into(),
        document_id: "document".into(),
        text: "Submit the prescribed form. Separate unrelated clause.".into(),
        locator: json!({}),
        ordinal: 1,
    });
    let governing = Span {
        source_id: "governing".into(),
        start: 0,
        end: 26,
        view_id: None,
        grid_cell: None,
    };
    let unrelated = Span {
        start: 27,
        end: input.source_units[1].text.len(),
        ..governing.clone()
    };
    let applicability =
        json!({"state":"applicable","condition":"submission","scope":"bid","grounds":[governing]});
    for value in [
        json!({"id":"requirement","sources":[governing],"data":{
            "kind":"requirement","text":"Use the prescribed form","categories":["format"],
            "strength":"mandatory","compliance":[],"applicability":applicability,
            "response":[{"channel":"structured_form","description":"Prescribed form","condition":"submission","grounds":[governing]}],
            "scoring_rule":null,"proofs":[],"criteria":[]}}),
        json!({"id":"template","sources":[citation(&input),governing],"data":{
            "kind":"template","label":"Form","title":"Prescribed form","parent":null,"order":null,
            "purpose":"submission","applicability":applicability,
            "regions":[{"source":citation(&input),"role":"fixed_text","form_id":null,"cells":[],"instruction":"Preserve wording"}]}}),
    ] {
        let record: Record = serde_json::from_value(value).unwrap();
        state.analysis.records.insert(record.id.clone(), record);
    }
    state.input_sha256 = digest(&input).unwrap();
    state.source_review = Some(initialize(&input, &config).unwrap());
    select_next(&input, &config, &mut state).unwrap();
    tools::cover(
        state
            .reviewer_coverage
            .text
            .entry("governing".into())
            .or_default(),
        0,
        unrelated.end,
    );
    for key in ["record:requirement", "record:template"] {
        state.reviewer_coverage.candidate.insert(
            key.into(),
            digest(&context::reference(&state.analysis, key).unwrap()).unwrap(),
        );
        record_query(
            &input,
            &mut state,
            "inspect_analysis",
            &json!({"kind":"all","ids":[key.split_once(':').unwrap().1]}),
        );
        context::complete_review_check(&input, &mut state, &json!({"reference":key,
            "summary":"Compared the current record with its original clauses.","sources":[citation(&input),governing]}),config.limits.max_tool_result_bytes).unwrap();
    }
    for (id, source) in [("missing", governing.clone()), ("unrelated", unrelated)] {
        let finding = Finding {
            code: "missing_mapping".into(),
            message: "A source requirement has no template edge.".into(),
            correction: "Add the source-backed mapping.".into(),
            affected: vec![],
            sources: vec![source],
        };
        finding_changed(&mut state, None, Some(&finding)).unwrap();
        state.review_draft.insert(id.into(), finding);
    }
    let task = task_inventory(&input, &state, config.limits.max_tool_result_bytes)
        .unwrap()
        .remove(0);
    let owned = obligations(&input, &state, &task);
    assert!(required_findings(&state, &task, &owned.sources, &owned.comparisons).is_empty());
    compare(&input, &config, &mut state);
    let mut args = judgment(&input, &config, &state);
    args["status"] = json!("findings");
    args["finding_ids"] = json!(["missing"]);
    args["candidate_refs"] = json!([
        "disposition:source",
        "record:requirement",
        "record:template"
    ]);
    args["template_mappings"] = json!([{"template_id":"template","requirement_ids":["requirement"],
        "relation_ids":[],"finding_ids":["missing"],"reason":"The governing clause requires this continued form.","sources":[citation(&input),governing]}]);
    args["relationship_checks"] = json!([]);
    let before = state.clone();
    put(&input, &config, &mut state, &args).unwrap();
    assert!(complete(&input, &state, &task).unwrap());
    let receipt = &state.source_review.as_ref().unwrap().results[&task.id];
    assert!(!receipt.dependencies.source_ids.contains("governing"));
    assert!(
        !receipt
            .dependencies
            .references
            .contains("disposition:governing")
    );
    assert_eq!(receipt.judgment.finding_ids, ["missing"]);
    let mut restored: Checkpoint = serde_json::from_value(json!(state)).unwrap();
    restored.review_draft.remove("missing");
    assert!(!complete(&input, &restored, &task).unwrap());
    for fault in [
        "unrelated_outer",
        "unrelated_nested",
        "missing_outer",
        "unread_original",
        "stale_candidate",
    ] {
        let mut invalid = before.clone();
        let mut check = args.clone();
        match fault {
            "unrelated_outer" => check["finding_ids"] = json!(["missing", "unrelated"]),
            "unrelated_nested" => {
                check["finding_ids"] = json!(["unrelated"]);
                check["template_mappings"][0]["finding_ids"] = json!(["unrelated"]);
            }
            "missing_outer" => check["finding_ids"] = json!([]),
            "unread_original" => {
                invalid.reviewer_coverage.text.remove("governing");
            }
            "stale_candidate" => {
                invalid
                    .reviewer_coverage
                    .candidate
                    .remove("record:requirement");
            }
            _ => unreachable!(),
        }
        assert!(
            put(&input, &config, &mut invalid, &check).is_err(),
            "{fault}"
        );
        assert!(invalid.source_review.as_ref().unwrap().results.is_empty());
    }
    // The same source-only issue can support an explicit relationship
    // judgment without importing every finding from the governing page.
    args["template_mappings"][0]["requirement_ids"] = json!([]);
    args["template_mappings"][0]["finding_ids"] = json!([]);
    args["relationship_checks"] = json!([{"record_id":"template","status":"findings",
        "related_record_ids":["requirement"],"relation_ids":[],"unresolved_record_ids":[],"finding_ids":["missing"],
        "reason":"The required relationship is absent.","sources":[citation(&input),governing]}]);
    let mut related = before;
    put(&input, &config, &mut related, &args).unwrap();
    assert!(complete(&input, &related, &task).unwrap());
}

#[test]
fn candidate_comparisons_do_not_replace_source_judgment_and_host_finishes_without_submission() {
    let (input, config, mut state) = fixture();
    let task = packet(&input, &config, &state).unwrap();
    assert_eq!(task["current"]["comparison_total"], 1);
    assert_eq!(task["current"]["pending_candidate_refs"]["total"], 0);
    assert!(
        task["current"].get("candidate_refs").is_none(),
        "do not present completed comparisons as a new work roster"
    );
    assert!(tools::review_gaps(&input, &state.analysis, &state.reviewer_coverage).is_empty());
    finish_review_batch(&input, &config, &mut state).unwrap();
    assert!(!state.done);
    let args = judgment(&input, &config, &state);
    put(&input, &config, &mut state, &args).unwrap();
    assert!(
        !state.done,
        "individual tools must not finalize before the batch ends"
    );
    finish_review_batch(&input, &config, &mut state).unwrap();
    assert!(state.done);
    assert_eq!(state.review_rounds, 1);
    finish_review_batch(&input, &config, &mut state).unwrap();
    assert_eq!(
        state.review_rounds, 1,
        "replay cannot count the completed review twice"
    );
}

#[test]
fn late_finding_and_withdrawal_invalidate_old_receipts_without_resurrecting_clean_checks() {
    let (input, config, mut state) = fixture();
    let args = judgment(&input, &config, &state);
    put(&input, &config, &mut state, &args).unwrap();
    let finding = Finding {
        code: "MISSED_CONDITION".into(),
        message: "发现尚未处理的限定条件".into(),
        correction: "补全对应条件并复核".into(),
        affected: vec![],
        sources: vec![citation(&input)],
    };
    finding_changed(&mut state, None, Some(&finding)).unwrap();
    state.review_draft.insert("finding".into(), finding.clone());
    finish_review_batch(&input, &config, &mut state).unwrap();
    assert!(
        !state.done,
        "a later tool in the same batch invalidates earlier completion"
    );
    finding_changed(&mut state, Some(&finding), None).unwrap();
    state.review_draft.clear();
    assert_eq!(pending(&input, &config, &state).unwrap().len(), 1);
    assert!(!context::has_review_outcome(&state, "disposition:source").unwrap());
    assert!(
        put(&input, &config, &mut state, &args)
            .unwrap_err()
            .contains("stale")
    );
    compare(&input, &config, &mut state);
    let args = judgment(&input, &config, &state);
    put(&input, &config, &mut state, &args).unwrap();
    finish_review_batch(&input, &config, &mut state).unwrap();
    assert!(state.done);
}

#[test]
#[ignore = "requires archived before/after analyses; offline dependency projection only"]
fn archived_repair_invalidates_only_dependent_candidate_versions() {
    let directory =
        std::path::PathBuf::from(std::env::var("KB_TENDER_DEPENDENCY_REPLAY_DIR").unwrap());
    let read = |name| std::fs::read(directory.join(name)).unwrap();
    let original = read("checkpoint.json");
    let mut before: Checkpoint = serde_json::from_slice(&original).unwrap();
    let mut after = before.clone();
    before.analysis = serde_json::from_slice(&read("before-analysis.json")).unwrap();
    after.analysis = serde_json::from_slice(&read("after-analysis.json")).unwrap();
    // Compare the same new contract on both analyses. This is not a
    // migration or replay of archived comparison receipts.
    let refs = context::scope_references(
        &before.analysis,
        &before
            .analysis
            .dispositions
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
    );
    let mut retained = Vec::new();
    let mut invalidated = Vec::new();
    for key in refs {
        if candidate_version(&before, &key).unwrap() == candidate_version(&after, &key).unwrap() {
            retained.push(key);
        } else {
            invalidated.push(key);
        }
    }
    let expected: Vec<String> = serde_json::from_slice(&read("expected-invalidated.json")).unwrap();
    std::fs::write(directory.join("projection.json"), serde_json::to_vec_pretty(&json!({
        "mode":"offline new-contract dependency projection; no old receipts reused or model calls",
        "retained":retained,"invalidated":invalidated,
        "before_analysis_sha256":digest(&before.analysis).unwrap(),
        "after_analysis_sha256":digest(&after.analysis).unwrap(),
    })).unwrap()).unwrap();
    assert_eq!(
        invalidated.into_iter().collect::<BTreeSet<_>>(),
        expected.into_iter().collect()
    );
    assert!(
        !retained.is_empty(),
        "unrelated comparisons must survive a local repair"
    );
    assert_eq!(read("checkpoint.json"), original);
}

#[test]
fn unrelated_same_source_edit_preserves_candidate_check_but_reopens_source_judgment() {
    let (input, config, mut state) = fixture();
    for id in ["changed", "unrelated"] {
        state.analysis.records.insert(
            id.into(),
            Record {
                id: id.into(),
                sources: vec![citation(&input)],
                data: RecordData::Fact {
                    name: id.into(),
                    value: "背景信息".into(),
                    scope: "项目".into(),
                },
            },
        );
    }
    for key in ["record:changed", "record:unrelated", "disposition:source"] {
        state.reviewer_coverage.candidate.insert(
            key.into(),
            digest(&context::reference(&state.analysis, key).unwrap()).unwrap(),
        );
        context::complete_review_check(
            &input,
            &mut state,
            &json!({"reference":key,"summary":"已与原文逐项比较。","sources":[citation(&input)]}),
            config.limits.max_tool_result_bytes,
        )
        .unwrap();
    }
    let args = judgment(&input, &config, &state);
    put(&input, &config, &mut state, &args).unwrap();
    assert!(pending(&input, &config, &state).unwrap().is_empty());
    let RecordData::Fact { value, .. } =
        &mut state.analysis.records.get_mut("changed").unwrap().data
    else {
        panic!("fact");
    };
    *value = "修订后的背景信息".into();
    assert!(
        context::has_review_outcome(&state, "record:unrelated").unwrap(),
        "sharing a source does not make unrelated candidate checks stale"
    );
    assert!(!context::has_review_outcome(&state, "record:changed").unwrap());
    assert!(!context::has_review_outcome(&state, "disposition:source").unwrap());
    assert_eq!(pending(&input, &config, &state).unwrap().len(), 1);
}

#[test]
fn candidate_checks_track_relation_membership_endpoints_and_finding_withdrawal() {
    let (input, _, mut state) = fixture();
    for id in ["local", "endpoint", "other"] {
        state.analysis.records.insert(
            id.into(),
            Record {
                id: id.into(),
                sources: vec![Span {
                    source_id: id.into(),
                    ..citation(&input)
                }],
                data: RecordData::Fact {
                    name: id.into(),
                    value: "背景".into(),
                    scope: "项目".into(),
                },
            },
        );
    }
    let without_link = candidate_version(&state, "record:local").unwrap();
    let link = Relation {
        id: "link".into(),
        from: "local".into(),
        to: "endpoint".into(),
        from_target: RelationTarget::Record,
        to_target: RelationTarget::Record,
        from_record_sha256: digest(&state.analysis.records["local"]).unwrap(),
        to_record_sha256: digest(&state.analysis.records["endpoint"]).unwrap(),
        kind: RelationKind::References,
        state: RelationState::Explicit,
        scope: "项目".into(),
        explanation: "直接引用".into(),
        grounds: vec![citation(&input)],
    };
    state
        .analysis
        .relations
        .insert(link.id.clone(), link.clone());
    let linked = candidate_version(&state, "record:local").unwrap();
    let relation = candidate_version(&state, "relation:link").unwrap();
    assert_ne!(
        without_link, linked,
        "new incident relation invalidates the check"
    );
    let original_endpoint = state.analysis.records["endpoint"].clone();
    state.analysis.records.get_mut("endpoint").unwrap().sources[0].start = 1;
    assert_ne!(linked, candidate_version(&state, "record:local").unwrap());
    assert_ne!(
        relation,
        candidate_version(&state, "relation:link").unwrap()
    );
    state
        .analysis
        .records
        .insert("endpoint".into(), original_endpoint);
    state.analysis.relations.get_mut("link").unwrap().to = "other".into();
    assert_ne!(linked, candidate_version(&state, "record:local").unwrap());
    state.analysis.relations.remove("link");
    assert_ne!(linked, candidate_version(&state, "record:local").unwrap());
    state.analysis.relations.insert("link".into(), link);
    let finding = Finding {
        code: "ENDPOINT_MISMATCH".into(),
        message: "端点需复核".into(),
        correction: "对照原文".into(),
        affected: vec![serde_json::from_value(json!({"id":"endpoint","path":"/sources"})).unwrap()],
        sources: vec![citation(&input)],
    };
    finding_changed(&mut state, None, Some(&finding)).unwrap();
    finding_changed(&mut state, Some(&finding), None).unwrap();
    assert_ne!(
        linked,
        candidate_version(&state, "record:local").unwrap(),
        "withdrawing an endpoint finding must not resurrect a dependent clean check"
    );
}

#[test]
#[ignore = "requires KB_TENDER_CONTEXT_REPLAY_DIR and KB_TENDER_CONTEXT_REPORT; offline only"]
fn archived_queries_do_not_expand_source_task_obligations() {
    let root = std::path::PathBuf::from(std::env::var("KB_TENDER_CONTEXT_REPLAY_DIR").unwrap());
    let path = root.join("extraction/checkpoint.json");
    let original = std::fs::read(&path).unwrap();
    let state: Checkpoint = serde_json::from_slice(&original).unwrap();
    let input: FrozenInput =
        serde_json::from_slice(&std::fs::read(root.join("source/frozen-input.json")).unwrap())
            .unwrap();
    let config: Config =
        serde_json::from_slice(&std::fs::read(root.join("extraction/runtime.json")).unwrap())
            .unwrap();
    let body: Value = serde_json::from_slice(state.journal.body().unwrap()).unwrap();
    let old: Value = serde_json::from_str(
        body["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let before = digest(&state).unwrap();
    let current = packet(&input, &config, &state).unwrap();
    let work =
        context::request_work_state(&input, &state, config.limits.max_tool_result_bytes).unwrap();
    assert_eq!(
        current["current"]["task"]["id"],
        old["source_review"]["current"]["task"]["id"]
    );
    assert_eq!(
        current["current"]["expected_version"], old["source_review"]["current"]["expected_version"],
        "dependency algorithm and frozen request version stay unchanged"
    );
    assert_eq!(
        work["comparison_progress"]["total"],
        current["current"]["comparison_total"]
    );
    assert_eq!(
        work["comparison_progress"]["remaining"],
        current["current"]["pending_candidate_refs"]["total"]
    );
    let mut unqueried = state.clone();
    let review = unqueried.source_review.as_mut().unwrap();
    assert!(!review.dependencies.references.is_empty());
    review.dependencies.references.clear();
    review.dependencies.global = false;
    let without_queries = packet(&input, &config, &unqueried).unwrap();
    assert_ne!(
        current["current"]["expected_version"],
        without_queries["current"]["expected_version"]
    );
    for field in [
        "records_requiring_relationship_judgment",
        "templates_requiring_mapping_judgment",
        "pending_candidate_refs",
        "comparison_total",
    ] {
        assert_eq!(
            current["current"][field], without_queries["current"][field],
            "query-dependent obligations: {field}"
        );
    }
    assert!(
        current["current"]["records_requiring_relationship_judgment"]["total"]
            .as_u64()
            .unwrap()
            < old["source_review"]["current"]["records_requiring_relationship_judgment"]["total"]
                .as_u64()
                .unwrap()
    );
    assert_eq!(digest(&state).unwrap(), before);
    assert_eq!(std::fs::read(&path).unwrap(), original);
    std::fs::write(std::env::var("KB_TENDER_CONTEXT_REPORT").unwrap(),serde_json::to_vec_pretty(&json!({
        "mode":"offline projection of paused checkpoint; no resumed model call or checkpoint transformation",
        "turn":state.turn,"old_relationship_subjects":old["source_review"]["current"]["records_requiring_relationship_judgment"],
        "new_relationship_subjects":current["current"]["records_requiring_relationship_judgment"],
        "old_comparison_total":old["source_review"]["current"]["comparison_total"],"new_comparison_total":current["current"]["comparison_total"],
        "pending_comparisons":current["current"]["pending_candidate_refs"],"queries_change_version_not_obligations":true,
        "work_navigation_matches_source_roster":true,"frozen_request_version_unchanged":true,"original_checkpoint_unchanged":true,
        "saved_source_judgments":state.source_review.as_ref().unwrap().results.len(),"provider_calls":0,"full_acceptance":false
    })).unwrap()).unwrap();
}

#[test]
#[ignore = "requires KB_TENDER_CONTEXT_REPLAY_DIR and KB_TENDER_CONTEXT_REPORT; offline only"]
fn archived_same_response_judgment_reports_remaining_validation() {
    use sha2::Digest;
    let root = std::path::PathBuf::from(std::env::var("KB_TENDER_CONTEXT_REPLAY_DIR").unwrap());
    let checkpoint_path = root.join("extraction/checkpoint.json");
    let checkpoint_bytes = std::fs::read(&checkpoint_path).unwrap();
    let state: Checkpoint = serde_json::from_slice(&checkpoint_bytes).unwrap();
    let input: FrozenInput =
        serde_json::from_slice(&std::fs::read(root.join("source/frozen-input.json")).unwrap())
            .unwrap();
    let config: Config =
        serde_json::from_slice(&std::fs::read(root.join("extraction/runtime.json")).unwrap())
            .unwrap();
    let body_bytes = std::fs::read(std::env::var("KB_TENDER_CONTEXT_REQUEST").unwrap()).unwrap();
    let body: Value = serde_json::from_slice(&body_bytes).unwrap();
    let calls = state
        .transcript
        .iter()
        .rev()
        .find(|message| message["role"] == "assistant")
        .unwrap()["tool_calls"]
        .as_array()
        .unwrap();
    let put_index = calls
        .iter()
        .position(|call| call["function"]["name"] == "put_source_review")
        .unwrap();
    let args: Value =
        serde_json::from_str(calls[put_index]["function"]["arguments"].as_str().unwrap()).unwrap();
    let packet: Value = serde_json::from_str(
        body["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        args["task_id"],
        packet["source_review"]["current"]["task"]["id"]
    );
    assert_eq!(
        args["expected_version"],
        packet["source_review"]["current"]["expected_version"]
    );
    // Reconstruct only the pre-withdrawal revision counters in memory.
    // The archived batch has no candidate writes or scope-changing queries
    // before its judgment. Exact request-version equality verifies this
    // projection; it is not a resumable or rewritten checkpoint.
    fn find(value: &Value, id: &str) -> Option<Finding> {
        match value {
            Value::Object(fields) => {
                if value["id"] == id && value.get("finding").is_some() {
                    return Some(serde_json::from_value(value["finding"].clone()).unwrap());
                }
                fields.values().find_map(|value| find(value, id))
            }
            Value::Array(values) => values.iter().find_map(|value| find(value, id)),
            _ => None,
        }
    }
    let mut delta = state.clone();
    let review = delta.source_review.as_mut().unwrap();
    review.finding_revisions.clear();
    review.candidate_revisions.clear();
    let mut withdrawals = 0;
    for call in &calls[..put_index] {
        match call["function"]["name"].as_str().unwrap() {
            "delete_review_finding" => {
                let args: Value =
                    serde_json::from_str(call["function"]["arguments"].as_str().unwrap()).unwrap();
                let finding = body["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find_map(|message| {
                        let value: Value =
                            serde_json::from_str(message["content"].as_str()?).ok()?;
                        find(&value, args["id"].as_str().unwrap())
                    })
                    .expect("withdrawn finding must be visible in the archived request");
                finding_changed(&mut delta, Some(&finding), None).unwrap();
                withdrawals += 1;
            }
            "complete_review_check" => {}
            name => panic!("unmodeled pre-judgment operation: {name}"),
        }
    }
    assert!(withdrawals > 0);
    let mut before = state.clone();
    let review = before.source_review.as_mut().unwrap();
    let delta = delta.source_review.as_ref().unwrap();
    for (revisions, increments) in [
        (&mut review.finding_revisions, &delta.finding_revisions),
        (&mut review.candidate_revisions, &delta.candidate_revisions),
    ] {
        for (key, increment) in increments {
            let revision = revisions.get_mut(key).unwrap();
            *revision = revision.checked_sub(*increment).unwrap();
        }
        revisions.retain(|_, revision| *revision > 0);
    }
    let batch = BatchVersion::capture(&before, &body)
        .unwrap()
        .expect("reconstructed version must exactly match the frozen request");
    let expanded = evidence_refs::expand(&input, &args).unwrap();
    let judgment: Judgment = serde_json::from_value(expanded.clone()).unwrap();
    let before_read = digest(&state).unwrap();
    let bundle = evidence(&input, &config, &state).unwrap().unwrap();
    let recalled = bundle.content["assigned_evidence"]["subject_evidence"]["items"]
        .as_array()
        .expect("cross-source subject originals are returned");
    let mut recalled_missing_subjects = Vec::new();
    for check in &judgment.relationship_checks {
        let record = &state.analysis.records[&check.record_id];
        if record.sources.iter().any(|source| {
            check
                .sources
                .iter()
                .any(|span| shared_mapping_evidence(source, span))
        }) {
            continue;
        }
        let row = recalled
            .iter()
            .find(|row| row["record_id"] == check.record_id)
            .expect("previously omitted subject original is now available");
        let span = record
            .sources
            .iter()
            .find(|span| span.grid_cell.is_none() && span.view_id.is_none())
            .unwrap();
        let source = input
            .source_units
            .iter()
            .find(|source| source.source_unit_revision_id == span.source_id)
            .unwrap();
        assert_eq!(row["source"]["text"], &source.text[span.start..span.end]);
        assert_eq!(row["source"]["start"], span.start);
        assert_eq!(row["source"]["end"], span.end);
        recalled_missing_subjects.push(json!({"record_id":check.record_id,"original_bytes":span.end-span.start,"citation":row["citation"]}));
    }
    assert!(!recalled_missing_subjects.is_empty());
    let bundle_bytes = serde_json::to_vec(&bundle.content).unwrap().len();
    assert!(bundle_bytes <= config.limits.max_tool_result_bytes);
    assert_eq!(digest(&state).unwrap(), before_read);
    let strict_error = put(&input, &config, &mut state.clone(), &expanded).unwrap_err();
    assert!(strict_error.contains("stale dependency version"));
    let mut admitted = state.clone();
    let remaining_error = apply_in_batch(
        &input,
        &config,
        &mut admitted,
        "put_source_review",
        &args,
        Some(&batch),
    )
    .unwrap_err();
    assert!(remaining_error.contains("/relationship_checks/1/sources"));
    assert!(remaining_error.contains("subject_source_example"));
    assert_eq!(
        digest(&admitted.analysis).unwrap(),
        digest(&state.analysis).unwrap()
    );
    assert_eq!(json!(admitted.review_draft), json!(state.review_draft));
    assert_eq!(std::fs::read(&checkpoint_path).unwrap(), checkpoint_bytes);
    std::fs::write(std::env::var("KB_TENDER_CONTEXT_REPORT").unwrap(), serde_json::to_vec_pretty(&json!({
        "mode":"offline final-state judgment replay with exact request-version reconstruction; not a resumed run or semantic acceptance",
        "turn":state.turn,"withdrawals":withdrawals,"request_sha256":hex::encode(sha2::Sha256::digest(&body_bytes)),
        "expected_version":args["expected_version"],"request_version_reconstructed":true,
        "old_error":strict_error,"remaining_error":remaining_error,"judgment_accepted":false,"candidate_content_unchanged":true,
        "recalled_missing_subjects":recalled_missing_subjects,"tool_result_bytes":bundle_bytes,"tool_result_limit":config.limits.max_tool_result_bytes,
        "findings_unchanged":true,"checkpoint_bytes_unchanged":true,"provider_calls":0,"full_acceptance":false
    })).unwrap()).unwrap();
}

#[test]
#[ignore = "requires KB_TENDER_CONTEXT_REPLAY_DIR and KB_TENDER_CONTEXT_REPORT; offline only"]
fn archived_review_checkpoint_uses_unchanged_runtime_contract() {
    let root = std::path::PathBuf::from(std::env::var("KB_TENDER_CONTEXT_REPLAY_DIR").unwrap());
    let state: Checkpoint =
        serde_json::from_slice(&std::fs::read(root.join("extraction/checkpoint.json")).unwrap())
            .unwrap();
    let config: Config =
        serde_json::from_slice(&std::fs::read(root.join("extraction/runtime.json")).unwrap())
            .unwrap();
    let input: FrozenInput =
        serde_json::from_slice(&std::fs::read(root.join("source/frozen-input.json")).unwrap())
            .unwrap();
    let current = Config::with_provider(config.provider.clone(), config.limits.clone()).unwrap();
    assert_eq!(json!(current), json!(config));
    assert_eq!(state.config_sha256, digest(&current).unwrap());
    assert_eq!(state.input_sha256, digest(&input).unwrap());
    let role = if state.role == Role::Reviewer {
        "reviewer"
    } else {
        "main"
    };
    state.journal.validate(state.turn, role).unwrap();
    let before = digest(&state).unwrap();
    let navigation = packet(&input, &current, &state).unwrap();
    assert_eq!(digest(&state).unwrap(), before);
    std::fs::write(std::env::var("KB_TENDER_CONTEXT_REPORT").unwrap(), serde_json::to_vec_pretty(&json!({
        "mode":"offline current-contract validation; no checkpoint transformation or model call",
        "checkpoint_sha256":before,"input_sha256":state.input_sha256,"config_sha256":state.config_sha256,
        "runtime_unchanged":true,"journal_valid":true,"turn":state.turn,"role":role,"saved_source_receipts":state.source_review.as_ref().unwrap().results.len(),
        "original_active_task":state.source_review.as_ref().unwrap().active_task,
        "remaining_source_tasks":navigation["remaining"],"current_task":navigation["current"]["task"]["id"],
        "pending_candidate_refs":navigation["current"]["pending_candidate_refs"]
    })).unwrap()).unwrap();
}

#[test]
#[ignore = "requires KB_TENDER_CONTEXT_REPLAY_DIR and KB_TENDER_CONTEXT_REPORT; offline only"]
fn archived_rule_finding_invalidation_is_reported() {
    let root = std::path::PathBuf::from(std::env::var("KB_TENDER_CONTEXT_REPLAY_DIR").unwrap());
    let state: Checkpoint =
        serde_json::from_slice(&std::fs::read(root.join("extraction/checkpoint.json")).unwrap())
            .unwrap();
    let original = digest(&state).unwrap();
    let keys: Vec<_> = state
        .analysis
        .records
        .keys()
        .map(|id| format!("record:{id}"))
        .chain(
            state
                .analysis
                .relations
                .keys()
                .map(|id| format!("relation:{id}")),
        )
        .chain(
            state
                .analysis
                .dispositions
                .keys()
                .map(|id| format!("disposition:{id}")),
        )
        .collect();
    let versions: BTreeMap<_, _> = keys
        .iter()
        .map(|key| (key.clone(), candidate_version(&state, key).unwrap()))
        .collect();
    let mut trials = Vec::new();
    for (id, finding) in &state.review_draft {
        if !finding.affected.iter().any(|field| {
            state
                .analysis
                .records
                .get(&field.id)
                .is_some_and(|record| matches!(record.data, RecordData::Rule { .. }))
        }) {
            continue;
        }
        let mut trial = state.clone();
        finding_changed(&mut trial, Some(finding), None).unwrap();
        trial.review_draft.remove(id);
        let changed: Vec<_> = keys
            .iter()
            .filter(|key| candidate_version(&trial, key).unwrap() != versions[*key])
            .cloned()
            .collect();
        assert_eq!(
            digest(&trial.analysis).unwrap(),
            digest(&state.analysis).unwrap()
        );
        trials.push(json!({"finding_id":id,"affected":finding.affected,"changed_candidate_count":changed.len(),"changed_candidate_refs":changed}));
    }
    assert!(!trials.is_empty());
    assert_eq!(digest(&state).unwrap(), original);
    std::fs::write(std::env::var("KB_TENDER_CONTEXT_REPORT").unwrap(), serde_json::to_vec_pretty(&json!({
        "mode":"offline hypothetical withdrawal; original checkpoint and all candidate values unchanged; not semantic acceptance",
        "checkpoint_sha256":original,"turn":state.turn,"candidate_count":keys.len(),"trials":trials
    })).unwrap()).unwrap();
}

#[test]
fn a_rule_finding_does_not_reopen_unrelated_checks_until_rule_content_changes() {
    let (input, config, mut state) = relationship_fixture();
    let key = "disposition:source";
    // Put the rule on another source, outside this disposition's actual
    // candidate dependencies. Its content still participates globally.
    let rule = state.analysis.records.get_mut("rule").unwrap();
    rule.sources[0].source_id = "other".into();
    if let RecordData::Rule { applicability, .. } = &mut rule.data {
        applicability.grounds[0].source_id = "other".into();
    }
    compare(&input, &config, &mut state);
    let original = candidate_version(&state, key).unwrap();
    let related = candidate_version(&state, "record:rule").unwrap();
    let finding = Finding {
        code: "rule_issue".into(),
        message: "Synthetic finding on another rule.".into(),
        correction: "Verify the rule against its original source.".into(),
        affected: vec![ReviewedField {
            id: "rule".into(),
            path: "/data/text".into(),
        }],
        sources: vec![Span {
            source_id: "other".into(),
            ..citation(&input)
        }],
    };
    finding_changed(&mut state, None, Some(&finding)).unwrap();
    state
        .review_draft
        .insert("rule-issue".into(), finding.clone());
    assert_eq!(original, candidate_version(&state, key).unwrap());
    assert!(context::has_review_outcome(&state, key).unwrap());
    assert_ne!(related, candidate_version(&state, "record:rule").unwrap());
    finding_changed(&mut state, Some(&finding), None).unwrap();
    state.review_draft.remove("rule-issue");
    assert_eq!(original, candidate_version(&state, key).unwrap());

    if let RecordData::Rule { text, .. } = &mut state.analysis.records.get_mut("rule").unwrap().data
    {
        text.push_str(" Corrected original rule.");
    }
    assert_ne!(original, candidate_version(&state, key).unwrap());
    assert!(!context::has_review_outcome(&state, key).unwrap());
}

#[test]
fn candidate_checks_track_explicit_template_parent_and_global_rules() {
    let (input, _, mut state) = fixture();
    let applicability = Applicability {
        state: ApplicabilityState::Applicable,
        condition: "".into(),
        scope: "项目".into(),
        grounds: vec![citation(&input)],
    };
    for (id, parent) in [("parent", None), ("child", Some("parent"))] {
        state.analysis.records.insert(
            id.into(),
            Record {
                id: id.into(),
                sources: vec![Span {
                    source_id: id.into(),
                    ..citation(&input)
                }],
                data: RecordData::Template {
                    label: id.into(),
                    title: "模板".into(),
                    parent: parent.map(str::to_owned),
                    order: None,
                    purpose: "提交".into(),
                    applicability: applicability.clone(),
                    regions: vec![],
                },
            },
        );
    }
    let original = candidate_version(&state, "record:child").unwrap();
    let local = candidate_reference_values(&state, "record:child").unwrap();
    assert_eq!(
        local["record:parent"],
        json!(state.analysis.records["parent"])
    );
    state.analysis.records.get_mut("parent").unwrap().sources[0].start = 1;
    let changed_parent = candidate_version(&state, "record:child").unwrap();
    assert_ne!(original, changed_parent);
    state.analysis.records.remove("parent");
    assert!(candidate_reference_values(&state, "record:child").unwrap()["record:parent"].is_null());
    assert_ne!(
        changed_parent,
        candidate_version(&state, "record:child").unwrap()
    );
    let no_rule = candidate_version(&state, "record:child").unwrap();
    let local_without_rule = candidate_reference_values(&state, "record:child").unwrap();
    state.analysis.records.insert(
        "rule".into(),
        Record {
            id: "rule".into(),
            sources: vec![citation(&input)],
            data: RecordData::Rule {
                text: "统一约定".into(),
                scope: "项目".into(),
                applicability,
            },
        },
    );
    let with_rule = candidate_version(&state, "record:child").unwrap();
    assert_ne!(no_rule, with_rule);
    assert_eq!(
        local_without_rule,
        candidate_reference_values(&state, "record:child").unwrap()
    );
    state.analysis.records.get_mut("rule").unwrap().sources[0].start = 1;
    assert_ne!(
        with_rule,
        candidate_version(&state, "record:child").unwrap()
    );
    state.analysis.records.remove("rule");
    assert_eq!(no_rule, candidate_version(&state, "record:child").unwrap());
}

#[test]
fn local_candidate_values_include_explicit_and_related_rules_but_not_review_revisions() {
    let (input, _, mut state) = relationship_fixture();
    state.analysis.records.insert(
        "local".into(),
        Record {
            id: "local".into(),
            sources: vec![citation(&input)],
            data: RecordData::Fact {
                name: "项目事实".into(),
                value: "原文值".into(),
                scope: "项目".into(),
            },
        },
    );
    let local = candidate_reference_values(&state, "record:local").unwrap();
    assert_eq!(
        local.keys().cloned().collect::<Vec<_>>(),
        vec!["record:local"]
    );
    let explicit = candidate_reference_values(&state, "record:rule").unwrap();
    assert_eq!(
        explicit["record:rule"],
        json!(state.analysis.records["rule"])
    );
    state.analysis.relations.insert(
        "link".into(),
        Relation {
            id: "link".into(),
            from: "local".into(),
            to: "rule".into(),
            from_target: RelationTarget::Record,
            to_target: RelationTarget::Record,
            from_record_sha256: digest(&state.analysis.records["local"]).unwrap(),
            to_record_sha256: digest(&state.analysis.records["rule"]).unwrap(),
            kind: RelationKind::References,
            state: RelationState::Explicit,
            scope: "项目".into(),
            explanation: "原文引用约定".into(),
            grounds: vec![citation(&input)],
        },
    );
    let linked = candidate_reference_values(&state, "record:local").unwrap();
    assert_eq!(linked["record:rule"], json!(state.analysis.records["rule"]));
    assert_eq!(
        linked["relation:link"],
        json!(state.analysis.relations["link"])
    );
    assert_eq!(
        linked,
        candidate_reference_values(&state, "relation:link").unwrap()
    );
    let original_review = candidate_version(&state, "record:local").unwrap();
    let finding = Finding {
        code: "rule_issue".into(),
        message: "核对约定原文".into(),
        correction: "对照已引用约定".into(),
        affected: vec![ReviewedField {
            id: "rule".into(),
            path: "/data/text".into(),
        }],
        sources: vec![citation(&input)],
    };
    finding_changed(&mut state, None, Some(&finding)).unwrap();
    assert_ne!(
        original_review,
        candidate_version(&state, "record:local").unwrap()
    );
    assert_eq!(
        linked,
        candidate_reference_values(&state, "record:local").unwrap()
    );
    state.analysis.records.get_mut("rule").unwrap().sources[0].start = 1;
    assert_ne!(
        linked,
        candidate_reference_values(&state, "record:local").unwrap()
    );
    state.analysis.relations.remove("link");
    assert_eq!(
        local,
        candidate_reference_values(&state, "record:local").unwrap()
    );
}

#[test]
#[ignore = "requires KB_SOURCE_REVIEW_LEGACY_CHECKPOINT and KB_SOURCE_REVIEW_LEGACY_REPORT; checks saved legacy hashes without modifying the archive"]
fn archived_candidate_versions_retain_saved_legacy_repair_hashes() {
    let path =
        std::path::PathBuf::from(std::env::var("KB_SOURCE_REVIEW_LEGACY_CHECKPOINT").unwrap());
    let report = std::path::PathBuf::from(std::env::var("KB_SOURCE_REVIEW_LEGACY_REPORT").unwrap());
    let original = std::fs::read(&path).unwrap();
    let state: Checkpoint = serde_json::from_slice(&original).unwrap();
    let mut matched = 0;
    let mut stale = Vec::new();
    for (id, receipt) in &state.repair.results {
        let differences: Vec<_> = receipt
            .candidate_versions
            .iter()
            .filter_map(|(key, saved)| {
                let current = context::reference(&state.analysis, key)
                    .ok()
                    .map(|_| candidate_version(&state, key).unwrap());
                (current != *saved)
                    .then(|| json!({"reference":key,"saved":saved,"current":current}))
            })
            .collect();
        if differences.is_empty() {
            matched += 1;
        } else {
            stale.push(json!({"finding_sha256":id,"differences":differences}));
        }
    }
    // These are saved pre-refactor hashes from the fixed turn-456 archive,
    // not expectations recomputed from the new local repair implementation.
    assert_eq!(state.turn, 456);
    assert_eq!(matched, 22);
    assert_eq!(stale.len(), 1);
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert!(!report.exists());
    std::fs::write(report, serde_json::to_vec_pretty(&json!({
        "scope":"Legacy independent-review candidate_version compared with saved pre-refactor hashes; no model calls or state edits",
        "checkpoint":path,"turn":state.turn,"matching_saved_receipts":matched,
        "previously_stale_receipts":stale,"archive_unchanged":true,"full_acceptance":false
    })).unwrap()).unwrap();
}

#[test]
fn navigation_keeps_cross_source_endpoints_required_by_the_assigned_task_pending() {
    let (mut input, config, mut state) = fixture();
    input.source_units.push(Source {
        source_unit_revision_id: "other".into(),
        document_id: "document".into(),
        text: "另一段项目背景。".into(),
        locator: json!({}),
        ordinal: 1,
    });
    for (id, source_id) in [("local", "source"), ("endpoint", "other")] {
        let record = Record {
            id: id.into(),
            sources: vec![Span {
                source_id: source_id.into(),
                ..citation(&input)
            }],
            data: RecordData::Fact {
                name: "背景".into(),
                value: "项目背景".into(),
                scope: "项目".into(),
            },
        };
        state.analysis.records.insert(id.into(), record);
    }
    state.analysis.relations.insert(
        "link".into(),
        Relation {
            id: "link".into(),
            from: "local".into(),
            to: "endpoint".into(),
            from_target: RelationTarget::Record,
            to_target: RelationTarget::Record,
            from_record_sha256: digest(&state.analysis.records["local"]).unwrap(),
            to_record_sha256: digest(&state.analysis.records["endpoint"]).unwrap(),
            kind: RelationKind::References,
            state: RelationState::Explicit,
            scope: "项目".into(),
            explanation: "本段引用另一段背景。".into(),
            grounds: vec![citation(&input)],
        },
    );
    state.input_sha256 = digest(&input).unwrap();
    state.source_review = Some(initialize(&input, &config).unwrap());
    select_next(&input, &config, &mut state).unwrap();
    for key in ["record:local", "relation:link", "disposition:source"] {
        state.reviewer_coverage.candidate.insert(
            key.into(),
            digest(&context::reference(&state.analysis, key).unwrap()).unwrap(),
        );
        context::complete_review_check(
            &input,
            &mut state,
            &json!({"reference":key,"summary":"与本段证据相符。","sources":[citation(&input)]}),
            config.limits.max_tool_result_bytes,
        )
        .unwrap();
    }
    let source = packet(&input, &config, &state).unwrap();
    assert!(
        source["current"]["pending_candidate_refs"]["items"]
            .as_array()
            .unwrap()
            .contains(&json!("record:endpoint"))
    );
    let work =
        context::request_work_state(&input, &state, config.limits.max_tool_result_bytes).unwrap();
    assert_eq!(work["comparison_progress"]["remaining"], 1);
    assert_eq!(
        work["comparison_progress"]["next_reference"],
        "record:endpoint"
    );
    assert_ne!(work["next_action"], "complete_source_review");

    let mut focus = state.reviewer_work.clone().unwrap();
    focus.focus.action = context::FocusAction::Review;
    focus.focus.references = vec!["record:endpoint".into()];
    context::validate(&input, &state, &focus, config.limits.max_tool_result_bytes).unwrap();
    assert!(
        context::check_read_scope(&input, &state, "read_source", &json!({"source_id":"other"}))
            .is_err(),
        "a comparison focus does not grant cross-source reading permission"
    );
    let mut unrelated = state.clone();
    let mut record = unrelated.analysis.records["endpoint"].clone();
    record.id = "unrelated".into();
    unrelated.analysis.records.insert(record.id.clone(), record);
    focus.focus.references = vec!["record:unrelated".into()];
    assert!(
        context::validate(
            &input,
            &unrelated,
            &focus,
            config.limits.max_tool_result_bytes
        )
        .is_err()
    );

    // The other endpoint is a genuine comparison obligation. After it is
    // checked, changing its meaning must reopen the original source too.
    state
        .reviewer_work
        .as_mut()
        .unwrap()
        .source_scope
        .push("other".into());
    state.reviewer_coverage.candidate.insert(
        "record:endpoint".into(),
        digest(&state.analysis.records["endpoint"]).unwrap(),
    );
    let endpoint_span = Span {
        source_id: "other".into(),
        ..citation(&input)
    };
    tools::cover(
        state
            .reviewer_coverage
            .text
            .entry("other".into())
            .or_default(),
        endpoint_span.start,
        endpoint_span.end,
    );
    context::complete_review_check(
        &input,
        &mut state,
        &json!({"reference":"record:endpoint","summary":"已对照另一段背景。","sources":[endpoint_span]}),
        config.limits.max_tool_result_bytes,
    ).unwrap();
    assert_eq!(
        packet(&input, &config, &state).unwrap()["current"]["pending_candidate_refs"]["total"],
        0
    );
    let args = judgment(&input, &config, &state);
    put(&input, &config, &mut state, &args).unwrap();
    let completed_task = args["task_id"].as_str().unwrap();
    assert!(
        !pending(&input, &config, &state)
            .unwrap()
            .iter()
            .any(|t| t.id == completed_task)
    );
    let RecordData::Fact { value, .. } =
        &mut state.analysis.records.get_mut("endpoint").unwrap().data
    else {
        panic!("fact endpoint");
    };
    *value = "修订后的项目背景".into();
    let reopened = packet(&input, &config, &state).unwrap();
    assert!(
        reopened["current"]["pending_candidate_refs"]["items"]
            .as_array()
            .unwrap()
            .contains(&json!("record:endpoint")),
        "a changed endpoint must reappear in the unfinished roster"
    );
    assert!(
        pending(&input, &config, &state)
            .unwrap()
            .iter()
            .any(|t| t.id == completed_task)
    );
    assert!(!context::has_review_outcome(&state, "record:local").unwrap());
}

#[test]
fn adding_deleting_and_moving_candidates_reopens_source_results() {
    let (input, config, mut state) = fixture();
    let args = judgment(&input, &config, &state);
    put(&input, &config, &mut state, &args).unwrap();
    let record = Record {
        id: "new".into(),
        sources: vec![citation(&input)],
        data: RecordData::Fact {
            name: "项目".into(),
            value: "背景".into(),
            scope: "本项目".into(),
        },
    };
    state
        .analysis
        .records
        .insert(record.id.clone(), record.clone());
    assert_eq!(pending(&input, &config, &state).unwrap().len(), 1);
    let changed = version(&state, &state.source_review.as_ref().unwrap().dependencies).unwrap();
    state.analysis.records.remove("new");
    assert_ne!(
        changed,
        version(&state, &state.source_review.as_ref().unwrap().dependencies).unwrap()
    );
    state.analysis.records.insert(record.id.clone(), record);
    state
        .analysis
        .records
        .get_mut("new")
        .unwrap()
        .sources
        .clear();
    assert_ne!(
        changed,
        version(&state, &state.source_review.as_ref().unwrap().dependencies).unwrap()
    );
}

#[test]
fn local_dependencies_survive_unrelated_edits_but_global_candidate_queries_do_not() {
    let (input, config, mut state) = fixture();
    let local = version(&state, &state.source_review.as_ref().unwrap().dependencies).unwrap();
    state.reviewer_work = None;
    record_query(
        &input,
        &mut state,
        "inspect_analysis",
        &json!({"kind":"all","offset":0,"limit":10}),
    );
    let global = version(&state, &state.source_review.as_ref().unwrap().dependencies).unwrap();
    state.analysis.records.insert(
        "elsewhere".into(),
        Record {
            id: "elsewhere".into(),
            sources: vec![Span {
                source_id: "other".into(),
                ..citation(&input)
            }],
            data: RecordData::Fact {
                name: "别处".into(),
                value: "新增内容".into(),
                scope: "其他范围".into(),
            },
        },
    );
    assert_eq!(
        local,
        version(
            &state,
            &task_dependencies(&tasks(&input, config.limits.max_tool_result_bytes).unwrap()[0])
        )
        .unwrap()
    );
    assert_ne!(
        global,
        version(&state, &state.source_review.as_ref().unwrap().dependencies).unwrap()
    );
}

#[test]
fn immutable_source_reads_and_searches_do_not_reopen_unrelated_candidate_or_finding_changes() {
    for query in ["项目", "not present in the original"] {
        let (mut input, config, mut state) = fixture();
        input.source_units.push(Source {
            source_unit_revision_id: "other".into(),
            document_id: "document".into(),
            text: "Unrelated original source material.".into(),
            locator: json!({}),
            ordinal: 1,
        });
        state.input_sha256 = digest(&input).unwrap();
        state.source_review = Some(initialize(&input, &config).unwrap());
        select_next(&input, &config, &mut state).unwrap();
        compare(&input, &config, &mut state);
        let source_pending = |state: &Checkpoint| {
            pending(&input, &config, state)
                .unwrap()
                .iter()
                .any(|task| task.source_id == "source")
        };
        let args = json!({"query":query,"offset":0,"limit":10});
        let coverage_before = digest(&state.reviewer_coverage).unwrap();
        let found = tools::invoke(
            &input,
            &mut state.analysis,
            &mut state.reviewer_coverage,
            true,
            "search_sources",
            &args,
            config.limits.max_tool_result_bytes,
        )
        .unwrap();
        assert_eq!(found["total"], if query == "项目" { 1 } else { 0 });
        assert_eq!(digest(&state.reviewer_coverage).unwrap(), coverage_before);
        record_query(&input, &mut state, "search_sources", &args);
        let read_args =
            json!({"source_id":"other","start":0,"max_bytes":input.source_units[1].text.len()});
        tools::invoke(
            &input,
            &mut state.analysis,
            &mut state.reviewer_coverage,
            true,
            "read_source",
            &read_args,
            config.limits.max_tool_result_bytes,
        )
        .unwrap();
        record_query(&input, &mut state, "read_source", &read_args);
        assert!(
            !state
                .source_review
                .as_ref()
                .unwrap()
                .dependencies
                .source_ids
                .contains("other")
        );
        let judgment = judgment(&input, &config, &state);
        put(&input, &config, &mut state, &judgment).unwrap();
        assert!(!source_pending(&state));
        state.analysis.records.insert(
            "elsewhere".into(),
            Record {
                id: "elsewhere".into(),
                sources: vec![Span {
                    source_id: "other".into(),
                    ..citation(&input)
                }],
                data: RecordData::Fact {
                    name: "Unrelated fact".into(),
                    value: "Updated candidate".into(),
                    scope: "Other source".into(),
                },
            },
        );
        let finding = Finding {
            code: "unrelated".into(),
            message: "Different source issue".into(),
            correction: "Check that source".into(),
            affected: vec![],
            sources: vec![Span {
                source_id: "other".into(),
                ..citation(&input)
            }],
        };
        finding_changed(&mut state, None, Some(&finding)).unwrap();
        state.review_draft.insert("elsewhere".into(), finding);
        let after = tools::invoke(
            &input,
            &mut state.analysis,
            &mut state.reviewer_coverage,
            true,
            "search_sources",
            &args,
            config.limits.max_tool_result_bytes,
        )
        .unwrap();
        assert_eq!(
            found, after,
            "source query is independent of mutable analysis and findings"
        );
        assert!(
            !source_pending(&state),
            "unchanged original search must not reopen a completed unrelated source review"
        );
        state
            .analysis
            .dispositions
            .get_mut("source")
            .unwrap()
            .reason
            .push_str(" changed");
        assert!(
            source_pending(&state),
            "changes to the actually compared source candidates still invalidate review"
        );
    }
}

#[test]
fn unresolved_boundary_feedback_preserves_evidence_work_without_approving() {
    let (input, config, mut state) = fixture();
    let mut args = judgment(&input, &config, &state);
    args["boundaries"]["after"]["state"] = json!("unresolved");
    args["boundaries"]["after"]["reason"] =
        json!("The continuation is not in the available fragment.");
    let error = put(&input, &config, &mut state, &args).unwrap_err();
    assert!(error.contains("/boundaries/after/state"));
    assert!(error.contains("needs_evidence") && error.contains("put_review_finding"));
    assert!(state.source_review.as_ref().unwrap().results.is_empty());

    args["status"] = json!("needs_evidence");
    args["evidence_requests"] = json!([{
        "question":"Locate the continuation or establish that the frozen collection lacks it.",
        "source_ids":["source"]
    }]);
    put(&input, &config, &mut state, &args).unwrap();
    assert_eq!(pending(&input, &config, &state).unwrap().len(), 1);
    finish_review_batch(&input, &config, &mut state).unwrap();
    assert!(!state.done);
    assert_eq!(state.review_rounds, 0);
}

#[test]
fn reviewed_source_gap_can_remain_open_without_an_outstanding_extraction_error() {
    let (mut input, config, mut state) = fixture();
    input.source_units[0].text = "合同到期后应".into();
    state.input_sha256 = digest(&input).unwrap();
    state
        .analysis
        .coverage
        .text
        .insert("source".into(), vec![(0, input.source_units[0].text.len())]);
    state.analysis.dispositions.get_mut("source").unwrap().state = DispositionState::Unresolved;
    state
        .analysis
        .dispositions
        .get_mut("source")
        .unwrap()
        .reason = "原文句子不完整，冻结集合缺少续文。".into();
    state.analysis.records.insert(
        "gap".into(),
        Record {
            id: "gap".into(),
            sources: vec![citation(&input)],
            data: RecordData::Unresolved {
                problem: "句子在应字之后截断；冻结集合无续文，不补写后续义务。".into(),
                affected: vec![],
                candidates: vec![],
            },
        },
    );
    state.reviewer_coverage = state.analysis.coverage.clone();
    state.reviewer_progress = Default::default();
    state.reviewer_work = None;
    state.source_review = Some(initialize(&input, &config).unwrap());
    select_next(&input, &config, &mut state).unwrap();
    for key in ["disposition:source", "record:gap"] {
        state.reviewer_coverage.candidate.insert(
            key.into(),
            digest(&context::reference(&state.analysis, key).unwrap()).unwrap(),
        );
        context::complete_review_check(
            &input,
            &mut state,
            &json!({"reference":key,
            "summary":"已读原文与当前候选；候选准确保留缺少续文的限制，没有补造条款。",
            "sources":[citation(&input)]}),
            config.limits.max_tool_result_bytes,
        )
        .unwrap();
    }
    let mut args = judgment(&input, &config, &state);
    args["candidate_refs"] = json!(["disposition:source", "record:gap"]);
    args["boundaries"]["after"]["state"] = json!("unresolved");
    args["boundaries"]["after"]["reason"] =
        json!("原文截断；已独立核对保存的来源缺口，不能标为已续接。");
    args["relationship_checks"] = json!([{"record_id":"gap","status":"source_limited",
        "related_record_ids":[],"relation_ids":[],"unresolved_record_ids":["gap"],"finding_ids":[],
        "reason":"冻结集合中没有续文，当前未决记录准确表达原文范围。","sources":[citation(&input)]}]);
    let before = state.clone();
    put(&input, &config, &mut state, &args).unwrap();
    finish_review_batch(&input, &config, &mut state).unwrap();
    assert!(state.done);
    assert!(state.review.as_ref().unwrap().findings.is_empty());
    let result = AnalysisResult {
        schema_version: 1,
        frozen_input_sha256: digest(&input).unwrap(),
        analysis: state.analysis.clone(),
        review: state.review.clone().unwrap(),
        quality: "needs_review".into(),
        source_views: BTreeMap::new(),
    };
    assert!(!result.open_items(&input).is_empty());
    assert_eq!(result.expected_quality(&input), "needs_review");
    crate::docx_composition::validate_basis(&input, &result).unwrap();
    for fault in [
        "missing_judgment",
        "missing_original",
        "stale_candidate",
        "unread_original",
    ] {
        let mut invalid = before.clone();
        let mut check = args.clone();
        match fault {
            "missing_judgment" => check["relationship_checks"] = json!([]),
            "missing_original" => check["boundaries"]["after"]["sources"] = json!([]),
            "stale_candidate" => {
                if let RecordData::Unresolved { problem, .. } =
                    &mut invalid.analysis.records.get_mut("gap").unwrap().data
                {
                    problem.push_str("changed");
                }
            }
            "unread_original" => invalid.reviewer_coverage.text.clear(),
            _ => unreachable!(),
        }
        assert!(
            put(&input, &config, &mut invalid, &check).is_err(),
            "{fault}"
        );
        assert!(invalid.source_review.as_ref().unwrap().results.is_empty());
    }
}

#[test]
fn continuation_requires_evidence_beyond_the_current_fragment_even_after_restore() {
    let (input, config, mut state) = fixture();
    let mut args = judgment(&input, &config, &state);
    args["boundaries"]["after"]["state"] = json!("continuation");
    assert!(
        put(&input, &config, &mut state, &args)
            .unwrap_err()
            .contains("/boundaries/after")
    );
    assert!(state.source_review.as_ref().unwrap().results.is_empty());

    let args = judgment(&input, &config, &state);
    put(&input, &config, &mut state, &args).unwrap();
    // A receipt written by an older validator cannot bypass the same
    // boundary requirement when restored and considered for finalization.
    state
        .source_review
        .as_mut()
        .unwrap()
        .results
        .values_mut()
        .next()
        .unwrap()
        .judgment
        .boundaries
        .after
        .state = BoundaryStatus::Continuation;
    assert_eq!(pending(&input, &config, &state).unwrap().len(), 1);
    assert!(
        packet(&input, &config, &state).unwrap()["current"]["prior_boundary_error"]
            .as_str()
            .unwrap()
            .contains("/boundaries/after")
    );
    finish_review_batch(&input, &config, &mut state).unwrap();
    assert!(!state.done);
}

#[test]
fn continuation_requires_a_read_distinct_source_and_retains_frozen_provenance() {
    let (mut input, config, mut state) = fixture();
    let mut next = input.source_units[0].clone();
    next.source_unit_revision_id = "next".into();
    next.ordinal = 1;
    input.source_units.push(next);
    state.input_sha256 = digest(&input).unwrap();
    state.source_review = Some(initialize(&input, &config).unwrap());
    select_next(&input, &config, &mut state).unwrap();
    compare(&input, &config, &mut state);
    let mut args = judgment(&input, &config, &state);
    let target = Span {
        source_id: "next".into(),
        ..citation(&input)
    };
    args["boundaries"]["after"]["state"] = json!("continuation");
    args["boundaries"]["after"]["sources"] = json!([target]);
    assert!(
        put(&input, &config, &mut state, &args)
            .unwrap_err()
            .contains("read the cited source")
    );
    tools::cover(
        state
            .reviewer_coverage
            .text
            .entry("next".into())
            .or_default(),
        target.start,
        target.end,
    );
    put(&input, &config, &mut state, &args).unwrap();
    let receipt = &state.source_review.as_ref().unwrap().results[args["task_id"].as_str().unwrap()];
    assert_eq!(receipt.judgment.boundaries.after.sources, vec![target]);
    assert!(!receipt.dependencies.source_ids.contains("next"));
    let dependencies = receipt.dependencies.clone();
    let prior_version = receipt.version.clone();
    assert_eq!(pending(&input, &config, &state).unwrap().len(), 1);
    input.source_units[1].text.push_str("来源已改变");
    state.input_sha256 = digest(&input).unwrap();
    assert_ne!(version(&state, &dependencies).unwrap(), prior_version);
}

#[test]
fn same_source_continuation_requires_the_correct_side_and_exact_geometry() {
    let (mut input, config, state) = fixture();
    let ordinary: Judgment = serde_json::from_value(judgment(&input, &config, &state)).unwrap();
    input.structured_forms.push(json!({"form_definition_revision_id":"grid",
        "source_unit_revision_id":"source","definition":{"kind":"grid","row_count":4,"column_count":2}}));
    let task = Task {
        id: "fragment".into(),
        source_id: "source".into(),
        region: Region::Text { start: 6, end: 12 },
    };
    let before = Span {
        start: 0,
        end: 6,
        ..citation(&input)
    };
    let after = Span {
        start: 12,
        end: 18,
        ..citation(&input)
    };
    assert!(continuation_target(&input, &task, &before, true));
    assert!(!continuation_target(&input, &task, &before, false));
    assert!(continuation_target(&input, &task, &after, false));
    assert!(!continuation_target(&input, &task, &after, true));
    assert!(!continuation_target(
        &input,
        &task,
        &citation(&input),
        false
    ));
    let view = Span {
        start: 0,
        end: 0,
        view_id: Some("whole-page".into()),
        ..citation(&input)
    };
    assert!(!continuation_target(&input, &task, &view, false));
    let grid = Task {
        region: Region::Grid {
            form_id: "grid".into(),
            start: 2,
            end: 6,
        },
        ..task
    };
    let cell = |row| Span {
        start: 0,
        end: 0,
        grid_cell: Some(GridCitation {
            form_id: "grid".into(),
            row,
            column: 0,
        }),
        ..citation(&input)
    };
    assert!(continuation_target(&input, &grid, &cell(0), true));
    assert!(!continuation_target(&input, &grid, &cell(0), false));
    assert!(!continuation_target(&input, &grid, &cell(2), false));
    assert!(continuation_target(&input, &grid, &cell(3), false));
    assert!(!continuation_target(&input, &grid, &view, false));
    // An ordinary complete boundary remains valid without a target.
    validate_boundaries(&input, &state, &grid, &ordinary).unwrap();
}

#[test]
fn completed_main_repair_navigates_to_independent_review_without_self_approval() {
    let (input, config, mut state) = fixture();
    state.role = Role::Main;
    state.main_work = state.reviewer_work.take();
    state.main_work.as_mut().unwrap().status = WorkStatus::Complete;
    state.review = Some(Review {
        analysis_sha256: digest(&state.analysis).unwrap(),
        coverage: state.reviewer_coverage.clone(),
        findings: vec![],
    });
    state
        .analysis
        .dispositions
        .get_mut("source")
        .unwrap()
        .reason = "经原文核对的背景说明".into();
    let packet =
        context::request_work_state(&input, &state, config.limits.max_tool_result_bytes).unwrap();
    assert_eq!(packet["next_action"], "request_review");
    assert!(!state.done);
    assert_eq!(state.role, Role::Main);
    state.review.as_mut().unwrap().analysis_sha256 = digest(&state.analysis).unwrap();
    let packet =
        context::request_work_state(&input, &state, config.limits.max_tool_result_bytes).unwrap();
    assert_ne!(
        packet["next_action"], "request_review",
        "unchanged analysis must not be presented as repaired"
    );
    state
        .analysis
        .dispositions
        .get_mut("source")
        .unwrap()
        .reason = "再次修订的背景说明".into();
    state.analysis.coverage.text.clear();
    let packet =
        context::request_work_state(&input, &state, config.limits.max_tool_result_bytes).unwrap();
    assert_ne!(
        packet["next_action"], "request_review",
        "scope completion cannot hide global unread sources"
    );
}

#[test]
fn source_judgment_reports_the_required_saved_findings_for_correction() {
    let (input, config, mut state) = fixture();
    let finding = Finding {
        code: "OMISSION".into(),
        message: "原文限定条件待补全".into(),
        correction: "补全原文限定条件".into(),
        affected: vec![],
        sources: vec![citation(&input)],
    };
    finding_changed(&mut state, None, Some(&finding)).unwrap();
    state.review_draft.insert("saved-omission".into(), finding);
    let before_packet = json!(state);
    let guidance = packet(&input, &config, &state).unwrap();
    assert_eq!(
        guidance["current"]["completion"]["required_finding_ids"]["items"],
        json!(["saved-omission"])
    );
    assert_eq!(
        guidance["current"]["completion"]["status_if_evidence_complete"],
        "findings"
    );
    assert_eq!(
        json!(state),
        before_packet,
        "navigation must not sign a judgment"
    );
    let mut args = judgment(&input, &config, &state);
    let error = put(&input, &config, &mut state, &args).unwrap_err();
    assert!(error.contains("saved-omission"));
    assert!(error.contains("expected_status"));
    assert!(state.source_review.as_ref().unwrap().results.is_empty());
    args["status"] = json!("findings");
    args["finding_ids"] = json!(["saved-omission"]);
    put(&input, &config, &mut state, &args).unwrap();
    assert!(pending(&input, &config, &state).unwrap().is_empty());
}

#[test]
fn evidence_requests_and_unresolved_boundaries_never_sign_a_clean_result() {
    let (input, config, mut state) = fixture();
    let mut args = judgment(&input, &config, &state);
    args["boundaries"]["after"]["state"] = json!("unresolved");
    assert!(put(&input, &config, &mut state, &args).is_err());
    args["status"] = json!("needs_evidence");
    args["evidence_requests"] =
        json!([{"question":"该段是否还有后续限定？","source_ids":["source"]}]);
    let result = put(&input, &config, &mut state, &args).unwrap();
    assert_eq!(result["new_completion"], false);
    assert_eq!(pending(&input, &config, &state).unwrap().len(), 1);
    finish_review_batch(&input, &config, &mut state).unwrap();
    assert!(!state.done);
}

#[test]
fn source_fragments_partition_utf8_and_preserve_all_grid_slots_without_fixed_pages() {
    let (mut input, mut config, _) = fixture();
    config.limits.max_tool_result_bytes = 2048;
    input.source_units[0].text = "中文😀条款\n".repeat(400);
    input.structured_forms.push(
        json!({"form_definition_revision_id":"grid","source_unit_revision_id":"source",
        "definition":{"kind":"grid","row_count":2,"column_count":2,"widths_mm":[40,40],
            "cells":[{"row":0,"column":0,"row_span":1,"col_span":2,"text":"固定标题"},
                {"row":1,"column":0,"row_span":1,"col_span":1,"text":"名称："},
                {"row":1,"column":1,"row_span":1,"col_span":1,"text":""}]}}),
    );
    let tasks = tasks(&input, config.limits.max_tool_result_bytes).unwrap();
    let (mut text_end, mut cell_end) = (0, 0);
    for task in tasks {
        match task.region {
            Region::Text { start, end } => {
                assert_eq!(start, text_end);
                assert!(input.source_units[0].text.is_char_boundary(end));
                text_end = end;
            }
            Region::Grid { start, end, .. } => {
                assert_eq!(start, cell_end);
                cell_end = end;
            }
            Region::Empty => panic!("nonempty source"),
        }
    }
    assert_eq!(text_end, input.source_units[0].text.len());
    assert_eq!(cell_end, 4);
}

#[test]
fn current_fragment_navigation_does_not_require_rereading_the_whole_long_source() {
    let (mut input, mut config, mut state) = fixture();
    config.limits.max_tool_result_bytes = 2048;
    input.source_units[0].text = "完整条款及条件。\n".repeat(400);
    state.input_sha256 = digest(&input).unwrap();
    state.source_review = Some(initialize(&input, &config).unwrap());
    state.reviewer_coverage.text.clear();
    select_next(&input, &config, &mut state).unwrap();
    let tasks = tasks(&input, config.limits.max_tool_result_bytes).unwrap();
    assert!(tasks.len() > 1);
    let Region::Text { start, end } = tasks[0].region else {
        panic!("text task")
    };
    let gaps = reading_gaps(
        &input,
        &state,
        &state.reviewer_coverage,
        config.limits.max_tool_result_bytes,
    )
    .unwrap()
    .unwrap();
    assert_eq!(gaps.len(), 1);
    assert_eq!(gaps[0]["start"], start);
    assert_eq!(gaps[0]["end"], end);
    tools::cover(
        state
            .reviewer_coverage
            .text
            .entry("source".into())
            .or_default(),
        start,
        end,
    );
    assert!(
        reading_gaps(
            &input,
            &state,
            &state.reviewer_coverage,
            config.limits.max_tool_result_bytes
        )
        .unwrap()
        .unwrap()
        .is_empty()
    );
    assert!(!tools::reading_gaps(&input, &state.reviewer_coverage).is_empty());
    assert_eq!(
        pending(&input, &config, &state).unwrap().len(),
        tasks.len(),
        "reading the assigned fragment grants no semantic completion"
    );
}

#[test]
#[ignore = "requires KB_TENDER_CONTEXT_REPLAY_DIR and KB_TENDER_CONTEXT_REPORT; offline only"]
fn archived_deleted_candidate_keeps_findings_and_builds_review_navigation() {
    let root = std::path::PathBuf::from(std::env::var("KB_TENDER_CONTEXT_REPLAY_DIR").unwrap());
    let path = root.join("extraction/checkpoint.json");
    let original = std::fs::read(&path).unwrap();
    let state: Checkpoint = serde_json::from_slice(&original).unwrap();
    let input: FrozenInput =
        serde_json::from_slice(&std::fs::read(root.join("source/frozen-input.json")).unwrap())
            .unwrap();
    let config: Config =
        serde_json::from_slice(&std::fs::read(root.join("extraction/runtime.json")).unwrap())
            .unwrap();
    let dependencies = &state.source_review.as_ref().unwrap().dependencies;
    let missing: Vec<_> = dependency_references(&state.analysis, dependencies)
        .into_iter()
        .filter(|key| context::reference(&state.analysis, key).is_err())
        .collect();
    assert!(
        !missing.is_empty(),
        "archive must contain a deleted dependency"
    );
    let before = digest(&state).unwrap();
    let navigation = packet(&input, &config, &state).unwrap();
    let required = &navigation["current"]["completion"]["required_finding_ids"];
    assert!(required["total"].as_u64().unwrap() > 0);
    assert_eq!(
        navigation["current"]["completion"]["status_if_evidence_complete"],
        "findings"
    );
    assert_eq!(digest(&state).unwrap(), before);
    assert_eq!(std::fs::read(path).unwrap(), original);
    std::fs::write(
        std::env::var("KB_TENDER_CONTEXT_REPORT").unwrap(),
        serde_json::to_vec_pretty(&json!({"mode":"offline archived navigation; no provider or journal writes",
            "turn":state.turn,"deleted_dependencies":missing,"required_findings":required,
            "comparison_total":navigation["current"]["comparison_total"],"checkpoint_unchanged":true})).unwrap(),
    ).unwrap();
}

#[test]
#[ignore = "requires KB_TENDER_CONTEXT_REPLAY_DIR and KB_TENDER_CONTEXT_REPORT; offline only"]
fn archived_reviewed_uncertainty_can_be_retired_without_erasing_review() {
    let root = std::path::PathBuf::from(std::env::var("KB_TENDER_CONTEXT_REPLAY_DIR").unwrap());
    let path = root.join("extraction/checkpoint.json");
    let original = std::fs::read(&path).unwrap();
    let mut state: Checkpoint = serde_json::from_slice(&original).unwrap();
    let input: FrozenInput =
        serde_json::from_slice(&std::fs::read(root.join("source/frozen-input.json")).unwrap())
            .unwrap();
    let config: Config =
        serde_json::from_slice(&std::fs::read(root.join("extraction/runtime.json")).unwrap())
            .unwrap();
    assert!(state.review_rounds > 0);
    let review = json!(state.review);
    let draft = json!(state.review_draft);
    let pending: Vec<_> = state
        .analysis
        .records
        .values()
        .filter(|r| matches!(r.data, RecordData::Unresolved { .. }))
        .map(|r| r.id.clone())
        .collect();
    // Project the actual archived candidate into primary repair in memory.
    // This is neither journal recovery nor a model-authored repair.
    state.role = Role::Main;
    let mut retired = vec![];
    let mut protected = vec![];
    for id in pending {
        let result = apply(
            &input,
            &config,
            &mut state,
            "delete_record",
            &json!({"id":id}),
        );
        if let Err(error) = result {
            assert!(error.contains("resolve the pending outcome"));
            assert!(state.analysis.records.contains_key(&id));
            protected.push(id);
        } else {
            assert!(!state.analysis.records.contains_key(&id));
            for work in [&state.main_work, &state.reviewer_work]
                .into_iter()
                .flatten()
            {
                assert!(!work.pending_refs.contains(&format!("record:{id}")));
            }
            retired.push(id);
        }
    }
    assert!(!retired.is_empty());
    assert!(!protected.is_empty());
    assert_eq!(json!(state.review), review);
    assert_eq!(json!(state.review_draft), draft);
    assert!(!state.done);
    assert_eq!(std::fs::read(path).unwrap(), original);
    std::fs::write(
        std::env::var("KB_TENDER_CONTEXT_REPORT").unwrap(),
        serde_json::to_vec_pretty(&json!({
            "mode":"offline primary-role projection; no provider, journal writes or live acceptance",
            "retired":retired,"protected":protected,"review_preserved":true,
            "draft_preserved":true,"original_checkpoint_unchanged":true,"done":state.done
        })).unwrap(),
    ).unwrap();
}

#[test]
fn one_primary_edit_does_not_navigate_past_remaining_review_findings() {
    let (input, _, mut state) = fixture();
    state.role = Role::Main;
    state.main_work = state.reviewer_work.clone();
    state.main_work.as_mut().unwrap().status = context::WorkStatus::Complete;
    state.review = Some(Review {
        analysis_sha256: digest(&state.analysis).unwrap(),
        coverage: state.reviewer_coverage.clone(),
        findings: vec![Finding {
            code: "omission".into(),
            message: "Synthetic omitted obligation.".into(),
            correction: "Reconcile the missing obligation and its relations.".into(),
            affected: vec![],
            sources: vec![citation(&input)],
        }],
    });
    state
        .analysis
        .dispositions
        .get_mut("source")
        .unwrap()
        .reason
        .push_str(" revised");
    let before = digest(&state).unwrap();
    let packet = context::request_work_state(&input, &state, 2048).unwrap();
    assert_eq!(packet["next_action"], "verify_review_findings");
    assert_eq!(packet["finding_count"], 1);
    assert!(serde_json::to_vec(&packet).unwrap().len() <= 2048);
    assert_eq!(
        digest(&state).unwrap(),
        before,
        "guidance cannot clear findings or grant repair credit"
    );
    state.review.as_mut().unwrap().findings.clear();
    assert_eq!(
        context::request_work_state(&input, &state, 2048).unwrap()["next_action"],
        "request_review"
    );
}

#[test]
fn completed_main_scope_exposes_bounded_pending_work_without_new_receipts() {
    let (mut input, config, mut state) = fixture();
    state.role = Role::Main;
    state.main_work = state.reviewer_work.clone();
    state.main_work.as_mut().unwrap().status = context::WorkStatus::Complete;
    input.source_units.push(Source {
        source_unit_revision_id: "next".into(),
        document_id: "document".into(),
        text: "下一段原文。".into(),
        locator: json!({"page_ordinal":1}),
        ordinal: 1,
    });
    let before = json!(state);
    let packet = context::request_work_state(&input, &state, 2048).unwrap();
    assert_eq!(packet["next_action"], "select_next_scope");
    assert_eq!(packet["next_source"]["source_id"], "next");
    assert_eq!(
        packet["next_source"]["bytes"],
        input.source_units[1].text.len()
    );
    assert_eq!(packet["blockers"]["items"][0]["kind"], "unread_source");
    assert_eq!(packet["blockers"]["next"], 1);
    assert!(serde_json::to_vec(&packet).unwrap().len() <= 2048);
    assert_eq!(
        json!(state),
        before,
        "navigation cannot deliver evidence or change progress"
    );

    state.main_work.as_mut().unwrap().deferred_sources = vec!["next".into()];
    let packet = context::request_work_state(&input, &state, 2048).unwrap();
    assert_eq!(packet["next_action"], "resume_deferred_scope");
    assert_eq!(packet["next_source"]["source_id"], "next");
    state.main_work.as_mut().unwrap().deferred_sources.clear();

    input.source_units[1].text.clear();
    input.structured_forms.push(json!({
        "form_definition_revision_id":"next-grid","source_unit_revision_id":"next",
        "definition":{"kind":"grid","row_count":1,"column_count":1,
            "cells":[{"row":0,"column":0,"row_span":1,"col_span":1,"text":"条件"}]}
    }));
    let packet = context::request_work_state(&input, &state, 2048).unwrap();
    assert_eq!(packet["next_source"]["source_id"], "next");
    assert_eq!(packet["blockers"]["items"][0]["kind"], "unread_grid");
    assert_eq!(packet["blockers"]["items"][0]["form_id"], "next-grid");
    assert!(
        context::check_read_scope(&input, &state, "read_form", &json!({"form_id":"next-grid"}))
            .is_err()
    );

    input.source_units.push(Source {
        source_unit_revision_id: "later-text".into(),
        document_id: "document".into(),
        text: "后续正文不能让前面的表格一直推迟。".into(),
        locator: json!({"page_ordinal":2}),
        ordinal: 2,
    });
    let before = json!(state);
    let packet = context::request_work_state(&input, &state, 2048).unwrap();
    assert_eq!(packet["next_source"]["source_id"], "next");
    assert_eq!(packet["blockers"]["items"][0]["form_id"], "next-grid");
    assert_eq!(json!(state), before);
    assert!(
        context::check_read_scope(&input, &state, "read_form", &json!({"form_id":"next-grid"}))
            .is_err()
    );
    input.source_units.pop();
    input.source_units.pop();
    input.structured_forms.clear();
    let packet =
        context::request_work_state(&input, &state, config.limits.max_tool_result_bytes).unwrap();
    assert_eq!(
        packet["next_action"], "request_review",
        "existing full-collection gate still applies"
    );

    input.documents.push(json!({"document_id":"metadata-only"}));
    let before = json!(state);
    let packet = context::request_work_state(&input, &state, 2048).unwrap();
    assert_eq!(packet["next_action"], "select_next_scope");
    assert!(packet["next_source"].is_null());
    assert_eq!(packet["blockers"]["items"][0]["kind"], "unread_metadata");
    assert_eq!(json!(state), before);

    state
        .main_progress
        .blockers
        .push(crate::agent_runtime::progress::ExecutionBlocker {
            scope: vec!["source".into()],
            dependencies_sha256: "test-dependencies".into(),
            watch: Default::default(),
        });
    let before = json!(state);
    let packet = context::request_work_state(&input, &state, 2048).unwrap();
    assert_eq!(packet["next_action"], "resolve_execution_blockers");
    assert_eq!(
        json!(state),
        before,
        "navigation must not clear execution blockers"
    );
}

#[test]
fn opening_after_completed_scope_preserves_fresh_navigation_without_granting_reading() {
    for role in [Role::Main, Role::Reviewer] {
        let (mut input, config, mut state) = fixture();
        input.source_units.push(Source {
            source_unit_revision_id: "next".into(),
            document_id: "document".into(),
            text: "下一段原文。".into(),
            locator: json!({}),
            ordinal: 1,
        });
        state.input_sha256 = digest(&input).unwrap();
        state.source_review = Some(initialize(&input, &config).unwrap());
        select_next(&input, &config, &mut state).unwrap();
        compare(&input, &config, &mut state);
        state.role = role.clone();
        if role == Role::Main {
            state.main_work = state.reviewer_work.clone();
        }
        let note = |source: &str, status: &str| {
            json!({
                "source_scope":[source],"objective":"Read and account for this source",
                "status":status,"note":"","deferred_sources":[],
                "focus":{"action":"locate","source_spans":[],"references":[]}
            })
        };
        let assistant = |id: &str, name: &str, args: &Value| {
            json!({
                "role":"assistant","tool_calls":[{"id":id,"type":"function",
                    "function":{"name":name,"arguments":args.to_string()}}]
            })
        };
        let complete = note("source", "complete");
        state.transcript = vec![
            json!({"role":"user","content":"old source working context"}),
            assistant("complete", "set_work_note", &complete),
        ];
        let result = apply(&input, &config, &mut state, "set_work_note", &complete).unwrap();
        assert_eq!(result["handoff"], true);
        assert_eq!(
            state.transcript.len(),
            1,
            "completed source context is released"
        );
        state
            .transcript
            .push(json!({"role":"tool","tool_call_id":"complete",
            "content":json!({"ok":true,"result":result}).to_string()}));
        let query = json!({"offset":1,"limit":1});
        state
            .transcript
            .push(assistant("index", "source_index", &query));
        let index = apply(&input, &config, &mut state, "source_index", &query).unwrap();
        assert_eq!(index["items"][0]["source_id"], "next");
        state
            .transcript
            .push(json!({"role":"tool","tool_call_id":"index",
            "content":json!({"ok":true,"result":index}).to_string()}));
        let next = note("next", "active");
        state
            .transcript
            .push(assistant("open", "set_work_note", &next));
        let expected = state.transcript.clone();
        let before_coverage = json!(state.coverage());
        let before_analysis = json!(state.analysis);
        let result = apply(&input, &config, &mut state, "set_work_note", &next).unwrap();
        assert_eq!(
            state.transcript, expected,
            "opening must not discard just-delivered navigation"
        );
        assert_eq!(result["handoff"], false);
        assert_eq!(json!(state.coverage()), before_coverage);
        assert_eq!(json!(state.analysis), before_analysis);
        assert!(
            apply(
                &input,
                &config,
                &mut state,
                "set_work_note",
                &note("next", "complete")
            )
            .is_err()
        );
        assert!(
            context::check_read_scope(
                &input,
                &state,
                "read_source",
                &json!({"source_id":"source","start":0,"max_bytes":100})
            )
            .is_err()
        );
        assert!(
            apply(
                &input,
                &config,
                &mut state,
                "set_work_note",
                &note("source", "active")
            )
            .is_err(),
            "unfinished source cannot be abandoned without deferred work"
        );
        let mut split = note("source", "active");
        split["deferred_sources"] = json!(["next"]);
        state
            .transcript
            .push(assistant("split", "set_work_note", &split));
        let result = apply(&input, &config, &mut state, "set_work_note", &split).unwrap();
        assert_eq!(result["handoff"], true);
        assert_eq!(
            state.transcript.len(),
            1,
            "active-scope split still releases old context"
        );
        assert_eq!(state.work().unwrap().deferred_sources, vec!["next"]);
    }
}

#[test]
fn unread_grid_focus_is_a_plan_not_delivered_evidence() {
    for role in [Role::Main, Role::Reviewer] {
        for action in ["locate", "extract", "review"] {
            let (mut input, config, mut state) = fixture();
            input.structured_forms.push(json!({
                "form_definition_revision_id":"grid","source_unit_revision_id":"source",
                "definition":{"kind":"grid","row_count":2,"column_count":2,
                    "cells":[{"row":0,"column":0,"row_span":1,"col_span":2,"text":"条件"},
                        {"row":1,"column":0,"row_span":1,"col_span":1,"text":"响应"},
                        {"row":1,"column":1,"row_span":1,"col_span":1,"text":"证明"}]}
            }));
            state.role = role.clone();
            let span = json!({"source_id":"source","start":0,"end":0,
                "grid_cell":{"form_id":"grid","row":0,"column":0}});
            let work = json!({"source_scope":["source"],"objective":"核对计划中的网格",
                "focus":{"action":action,"source_spans":[span],"references":[]},
                "status":"active","note":"计划不是阅读回执"});
            let before = json!(state.coverage());
            apply(&input, &config, &mut state, "set_work_note", &work).unwrap();
            context::check_read_scope(&input, &state, "read_form", &json!({"form_id":"grid"}))
                .unwrap();
            assert_eq!(json!(state.coverage()), before);
            assert!(
                tools::validate_span(
                    &input,
                    state.coverage(),
                    &serde_json::from_value(span.clone()).unwrap()
                )
                .unwrap_err()
                .contains("read the cited grid cell"),
                "a plan cannot authorize a primary citation or independent review"
            );
            let (tool, args) = if role == Role::Main {
                (
                    "put_record",
                    json!({"id":null,"sources":[span],
                    "data":{"kind":"fact","name":"计划中的条件","value":"未读取","scope":"来源"}}),
                )
            } else {
                (
                    "put_review_finding",
                    json!({"id":null,"finding":{
                    "code":"unread","message":"未读取的条件","correction":"核对原文",
                    "sources":[span],"affected":[]}}),
                )
            };
            assert!(
                apply(&input, &config, &mut state, tool, &args)
                    .unwrap_err()
                    .contains("read the cited grid cell")
            );
            let saved = json!(state.work());
            for (field, value) in [
                ("/focus/source_spans/0/grid_cell/column", json!(1)),
                ("/focus/source_spans/0/grid_cell/row", json!(2)),
                ("/focus/source_spans/0/grid_cell/form_id", json!("unknown")),
                ("/focus/source_spans/0/source_id", json!("foreign")),
                ("/focus/source_spans/0/start", json!(1)),
            ] {
                let mut invalid = work.clone();
                *invalid.pointer_mut(field).unwrap() = value;
                assert!(apply(&input, &config, &mut state, "set_work_note", &invalid).is_err());
                assert_eq!(json!(state.work()), saved);
            }
            let mut invalid = work.clone();
            invalid["focus"]["source_spans"][0]["view_id"] = json!("undelivered");
            assert!(apply(&input, &config, &mut state, "set_work_note", &invalid).is_err());
            assert_eq!(json!(state.coverage()), before);
        }
    }
}

#[tokio::test]
async fn main_source_backed_dispute_requires_independent_reconsideration() {
    async fn handoff(input: &FrozenInput, config: &Config, state: &mut Checkpoint) {
        struct NoIo;
        #[async_trait]
        impl Journal for NoIo {
            async fn load(&self) -> Result<Option<Checkpoint>, AgentError> {
                panic!("no journal reads during tool replay")
            }
            async fn reserve(&self, _: &Checkpoint, _: &[u8]) -> Result<Option<usize>, AgentError> {
                panic!("no provider call during tool replay")
            }
            async fn save(&self, _: &Checkpoint, _: &Value) -> Result<(), AgentError> {
                panic!("no partial batch save")
            }
        }
        // Deliver actual current feedback in the request being completed;
        // no saved reading/repair receipt is manufactured by this fixture.
        let query = json!({"offset":0,"limit":usize::MAX});
        let feedback =
            agent::inspect_review(state, &query, config.limits.max_tool_result_bytes).unwrap();
        state.transcript.push(json!({"role":"assistant","content":null,"tool_calls":[{
            "id":"feedback","type":"function","function":{"name":"inspect_review","arguments":query.to_string()}
        }]}));
        state
            .transcript
            .push(json!({"role":"tool","tool_call_id":"feedback",
            "content":json!({"ok":true,"result":feedback}).to_string()}));
        let disposition = json!({"finding_sha256":digest(state.findings_for_repair()[0]).unwrap(),
            "conclusion":"disputed", "summary":"The synthetic source limitation is already recorded; request independent verification of the unchanged outcome.",
            "sources":[citation(input)],"candidate_refs":[]});
        for (name, arguments) in [
            ("put_repair_result", disposition),
            ("request_review", json!({})),
        ] {
            // Match the real pre-reservation dispatcher; request() alone is
            // intentionally a read-only sizing/projection path.
            agent::repair_task_host::schedule(state, &config.limits).unwrap();
            let body = request(input, config, state).await.unwrap();
            let response = ChatTurn {
                finish_reason: "tool_calls".into(),
                tool_calls: vec![knowledge::models::ChatToolCall {
                    id: name.into(),
                    name: name.into(),
                    arguments: arguments.to_string(),
                }],
                ..Default::default()
            };
            state.journal.prepare(state.turn, "main", &body).unwrap();
            state.journal.responded(response).unwrap();
            // Exercise restored complete responses and separate transition turns.
            *state = serde_json::from_value(json!(state)).unwrap();
            let response = state.journal.response().unwrap().clone();
            let results = execute_turn(
                input,
                config,
                state,
                &NoIo,
                response,
                BTreeMap::new(),
                &CancellationToken::new(),
            )
            .await
            .unwrap();
            let result: Value =
                serde_json::from_str(results[0]["content"].as_str().unwrap()).unwrap();
            assert_eq!(result["ok"], true, "{result}");
            state.journal.committed().unwrap();
        }
    }
    let (input, config, mut state) = fixture();
    let finding = Finding {
        code: "missing_continuation".into(),
        message: "The frozen source lacks the continuation.".into(),
        correction: "Obtain the missing original before completing the composition.".into(),
        affected: vec![],
        sources: vec![citation(&input)],
    };
    finding_changed(&mut state, None, Some(&finding)).unwrap();
    state.review_draft.insert("missing".into(), finding);
    let mut args = judgment(&input, &config, &state);
    args["status"] = json!("findings");
    args["finding_ids"] = json!(["missing"]);
    put(&input, &config, &mut state, &args).unwrap();
    finish_review_batch(&input, &config, &mut state).unwrap();
    assert_eq!(state.role, Role::Main);
    assert!(!state.done);
    let previous = json!(state.review);
    let receipts = json!(state.source_review.as_ref().unwrap().results);
    let semantic = json!([
        state.analysis.records,
        state.analysis.relations,
        state.analysis.dispositions
    ]);
    let received = state.analysis.coverage.clone();
    let mut delivered = received.clone();
    tools::inspect_analysis(
        &input,
        &state.analysis,
        &mut delivered,
        &received,
        &json!({"kind":"all","view":"detail","offset":0,"limit":20}),
        config.limits.max_tool_result_bytes,
        None,
    )
    .unwrap();
    state.replace_coverage(delivered);
    assert_ne!(
        digest(&state.analysis).unwrap(),
        previous["analysis_sha256"]
    );
    assert_eq!(
        json!(state.review),
        previous,
        "reading cannot revise a review"
    );
    assert_eq!(
        json!([
            state.analysis.records,
            state.analysis.relations,
            state.analysis.dispositions
        ]),
        semantic
    );
    assert!(review_complete(&input, &config, &state).unwrap());

    for defect in [
        "disposition",
        "source_judgment",
        "candidate_read",
        "boundary",
    ] {
        let mut changed = state.clone();
        match defect {
            "disposition" => {
                changed
                    .analysis
                    .dispositions
                    .get_mut("source")
                    .unwrap()
                    .reason = "A revised interpretation of the original".into();
            }
            "source_judgment" => changed.source_review.as_mut().unwrap().results.clear(),
            "candidate_read" => changed.reviewer_coverage.candidate.clear(),
            "boundary" => {
                changed
                    .source_review
                    .as_mut()
                    .unwrap()
                    .results
                    .values_mut()
                    .next()
                    .unwrap()
                    .judgment
                    .boundaries
                    .after
                    .state = BoundaryStatus::Continuation;
            }
            _ => unreachable!(),
        }
        handoff(&input, &config, &mut changed).await;
        assert!(!changed.done, "{defect} still requires independent work");
        assert_eq!(json!(changed.review), previous);
    }

    handoff(&input, &config, &mut state).await;
    assert!(
        !state.done,
        "a new source-backed dispute must be considered by the independent reviewer"
    );
    assert_eq!(state.role, Role::Reviewer);
    assert_eq!(
        json!(state.source_review.as_ref().unwrap().results),
        receipts
    );
    let review = state.review.as_ref().unwrap();
    assert_eq!(
        json!(review),
        previous,
        "main dispute must not revise the old independent report"
    );
    assert!(!pending(&input, &config, &state).unwrap().is_empty());
    let page = agent::inspect_review(
        &state,
        &json!({"offset":0,"limit":1}),
        config.limits.max_tool_result_bytes,
    )
    .unwrap();
    assert_eq!(page["items"][0]["main_repair"]["conclusion"], "disputed");
    assert_eq!(
        page["items"][0]["main_repair"]["independent_approval"],
        false
    );
    assert_eq!(json!(review.findings), previous["findings"]);
    assert!(
        !review.findings.is_empty(),
        "termination must not approve missing evidence"
    );
    let result = AnalysisResult {
        schema_version: 1,
        frozen_input_sha256: digest(&input).unwrap(),
        analysis: state.analysis.clone(),
        review: review.clone(),
        quality: "needs_review".into(),
        source_views: BTreeMap::new(),
    };
    assert_eq!(result.expected_quality(&input), "needs_review");
    assert!(crate::docx_composition::validate_basis(&input, &result).is_err());
}

#[test]
fn independent_finding_allows_retiring_a_wrong_unresolved_item_without_clearing_review() {
    let (input, config, mut state) = fixture();
    state.role = Role::Main;
    state.main_work = state.reviewer_work.clone();
    state.main_work.as_mut().unwrap().pending_refs = vec!["record:uncertain".into()];
    state.analysis.records.insert(
        "uncertain".into(),
        Record {
            id: "uncertain".into(),
            sources: vec![citation(&input)],
            data: RecordData::Unresolved {
                problem: "Old uncertainty.".into(),
                affected: vec![],
                candidates: vec![],
            },
        },
    );
    let args = json!({"id":"uncertain"});
    assert!(
        apply(&input, &config, &mut state, "delete_record", &args)
            .unwrap_err()
            .contains("resolve the pending outcome")
    );
    let issue = Finding {
        code: "stale_uncertainty".into(),
        message: "Original evidence resolves this item.".into(),
        correction:
            "Retire the stale unresolved item after preserving the actual source requirements."
                .into(),
        affected: vec![ReviewedField {
            id: "uncertain".into(),
            path: "/data/problem".into(),
        }],
        sources: vec![citation(&input)],
    };
    state.review_rounds = 1;
    state.review = Some(Review {
        analysis_sha256: digest(&state.analysis).unwrap(),
        coverage: state.reviewer_coverage.clone(),
        findings: vec![issue.clone()],
    });
    state.review.as_mut().unwrap().findings[0].affected.clear();
    assert!(
        apply(&input, &config, &mut state, "delete_record", &args).is_err(),
        "a generic source finding must not authorize deleting any pending item"
    );
    state.review.as_mut().unwrap().findings[0] = issue.clone();
    state.review_draft.insert("issue".into(), issue);
    let review_before = json!(state.review);
    apply(&input, &config, &mut state, "delete_record", &args).unwrap();
    assert!(!state.analysis.records.contains_key("uncertain"));
    assert_eq!(json!(state.review), review_before);
    assert!(state.review_draft.contains_key("issue"));
    assert!(
        !state.done,
        "retirement is a primary repair, never independent acceptance"
    );
}

#[test]
fn deleting_an_affected_candidate_does_not_silently_drop_its_source_finding() {
    let (input, config, mut state) = fixture();
    let record = Record {
        id: "fact".into(),
        sources: vec![citation(&input)],
        data: RecordData::Fact {
            name: "背景".into(),
            value: "错误解释".into(),
            scope: "项目".into(),
        },
    };
    state
        .reviewer_coverage
        .candidate
        .insert("record:fact".into(), digest(&record).unwrap());
    state.analysis.records.insert(record.id.clone(), record);
    record_query(
        &input,
        &mut state,
        "inspect_analysis",
        &json!({"kind":"record","ids":["fact"]}),
    );
    let finding = Finding {
        code: "WRONG_VALUE".into(),
        message: "解释不对应背景".into(),
        correction: "按背景原文修正".into(),
        sources: vec![],
        affected: vec![ReviewedField {
            id: "fact".into(),
            path: "/data/value".into(),
        }],
    };
    finding_changed(&mut state, None, Some(&finding)).unwrap();
    state.review_draft.insert("finding".into(), finding.clone());
    compare(&input, &config, &mut state);
    let mut args = judgment(&input, &config, &state);
    args["status"] = json!("findings");
    args["finding_ids"] = json!(["finding"]);
    put(&input, &config, &mut state, &args).unwrap();
    finish_review_batch(&input, &config, &mut state).unwrap();
    assert_eq!(state.role, Role::Main);
    state.analysis.records.remove("fact");
    state.role = Role::Reviewer;
    // Real repairs can merge and delete a candidate inspected in an earlier
    // round. Restore the archived dependencies exactly, including that ID.
    state = serde_json::from_value(json!(state)).unwrap();
    select_next(&input, &config, &mut state).unwrap();
    let dependencies = &state.source_review.as_ref().unwrap().dependencies;
    assert!(dependencies.references.contains("record:fact"));
    let deleted_version = version(&state, dependencies).unwrap();
    let mut without_tombstone = dependencies.clone();
    without_tombstone.references.remove("record:fact");
    assert_ne!(
        deleted_version,
        version(&state, &without_tombstone).unwrap()
    );
    assert!(!references(&state.analysis, dependencies).contains("record:fact"));
    compare(&input, &config, &mut state);
    assert_eq!(
        packet(&input, &config, &state).unwrap()["current"]["completion"]["required_finding_ids"]["items"],
        json!(["finding"])
    );
    let mut args = judgment(&input, &config, &state);
    let error = put(&input, &config, &mut state, &args).unwrap_err();
    let detail: Value =
        serde_json::from_str(error.strip_prefix("INVALID_FIELD /finding_ids: ").unwrap()).unwrap();
    assert_eq!(detail["required_finding_ids"]["items"], json!(["finding"]));
    assert_eq!(detail["expected_status"], "findings");
    args["status"] = json!("findings");
    args["finding_ids"] = json!(["finding"]);
    assert!(
        put(&input, &config, &mut state, &args)
            .unwrap_err()
            .contains("revised or explicitly withdrawn")
    );
    assert!(!state.done);
    assert_eq!(pending(&input, &config, &state).unwrap().len(), 1);
    finding_changed(&mut state, Some(&finding), None).unwrap();
    state.review_draft.clear();
    let current = packet(&input, &config, &state).unwrap();
    assert_eq!(
        current["current"]["completion"]["required_finding_ids"]["total"],
        0
    );
    assert_eq!(
        current["current"]["completion"]["status_if_evidence_complete"],
        "checked"
    );
    let args = judgment(&input, &config, &state);
    put(&input, &config, &mut state, &args).unwrap();
    finish_review_batch(&input, &config, &mut state).unwrap();
    assert!(state.done);
}
