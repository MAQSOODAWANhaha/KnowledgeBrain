use super::agent::{Checkpoint, Config, Journal, Limits, Model, Role};
use super::*;
use crate::{agent_error::AgentError, authoring_runtime::AuthoringRuntimeContractV1};
use async_trait::async_trait;
use knowledge::models::{ChatToolCall, ChatTurn};
use serde_json::json;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Mutex,
};
use tokio_util::sync::CancellationToken;

mod context;
mod id_navigation;
mod input;
mod original_views;
mod reading;
mod records;
mod relationships;
mod repair;
mod repair_dependencies;
mod repair_dispute;
mod repair_history;
mod repair_history_navigation;
mod repair_navigation;
mod repair_recovery;
mod repair_task_dispatch;
mod repair_task_packet;
mod resume;
mod review;
mod work;

fn input() -> FrozenInput {
    FrozenInput {
        schema_version: 1,
        project_id: "project".into(),
        document_set_id: "set".into(),
        documents: vec![],
        document_relations: vec![],
        decisions: vec![],
        structured_forms: vec![],
        source_units: vec![Source {
            source_unit_revision_id: "source".into(),
            document_id: "document".into(),
            text: "提交格式见附表甲。".into(),
            locator: json!({"heading_path":"须知"}),
            ordinal: 0,
        }],
    }
}
fn span() -> Span {
    Span {
        view_id: None,
        grid_cell: None,
        source_id: "source".into(),
        start: 0,
        end: input().source_units[0].text.len(),
    }
}

fn grid_citation_input() -> FrozenInput {
    let mut input = input();
    input.source_units[0].text.clear();
    input.structured_forms.push(
        json!({"form_definition_revision_id":"grid","source_unit_revision_id":"source",
        "definition":{"kind":"grid","row_count":2,"column_count":2,"widths_mm":[40,40],
            "cells":[{"row":0,"column":0,"row_span":1,"col_span":2,"text":"产品条件"},
                {"row":1,"column":0,"row_span":1,"col_span":1,"text":"投标时逐项响应"},
                {"row":1,"column":1,"row_span":1,"col_span":1,"text":"供货时提供证书"}]}}),
    );
    input
}

fn requirement() -> Value {
    json!({"id":null,"sources":[span()],"data":{"kind":"requirement","text":"提交规定格式",
        "categories":["format"],"strength":"mandatory","compliance":[{"policy":"explicit_response","condition":"按须知提交","grounds":[span()]}],"applicability":{"state":"applicable","condition":"按须知提交","scope":"本次投标","grounds":[span()]},
        "response":[{"channel":"structured_form","description":"按附表编制","condition":"按须知提交","grounds":[span()]}],"scoring_rule":null,"proofs":[],"criteria":[]}})
}

fn read_analysis(input: &FrozenInput) -> Analysis {
    let mut analysis = Analysis::default();
    for source in &input.source_units {
        tools::cover(
            analysis
                .coverage
                .text
                .entry(source.source_unit_revision_id.clone())
                .or_default(),
            0,
            source.text.len(),
        );
    }
    analysis
}

fn analysis_query_fixture() -> (FrozenInput, Analysis, Vec<String>) {
    let mut input = input();
    input.source_units.push(Source {
        source_unit_revision_id: "other-source".into(),
        document_id: "other-document".into(),
        ordinal: 1,
        ..input.source_units[0].clone()
    });
    let mut analysis = Analysis::default();
    let mut coverage = Coverage::default();
    let mut ids = Vec::new();
    for source_id in ["source", "other-source", "source"] {
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "read_source",
            &json!({"source_id":source_id,"start":0,"max_bytes":1024}),
            16000,
        )
        .unwrap();
        let record = tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "put_record",
            &json!({"id":null,"sources":[Span {source_id:source_id.into(),..span()}],
                "data":{"kind":"fact","name":"同名格式","value":source_id,"scope":"本文件"}}),
            16000,
        )
        .unwrap();
        ids.push(record["id"].as_str().unwrap().to_owned());
        tools::invoke(
            &input,
            &mut analysis,
            &mut coverage,
            false,
            "set_disposition",
            &json!({"source_id":source_id,"state":"non_requirement","reason":"测试事实来源"}),
            16000,
        )
        .unwrap();
    }
    let relation = tools::invoke(
        &input,
        &mut analysis,
        &mut coverage,
        false,
        "put_relation",
        &json!({"id":null,"from":ids[0],"to":ids[1],
            "from_target":{"kind":"record"},"to_target":{"kind":"record"},
            "kind":"references","state":"explicit","scope":"跨文件引用",
            "explanation":"依据原文引用","grounds":[span()]}),
        16000,
    )
    .unwrap();
    ids.push(relation["id"].as_str().unwrap().to_owned());
    (input, analysis, ids)
}

#[derive(Default)]
struct MemoryJournal {
    sdk_turns: Mutex<Vec<usize>>,
    fail_boundary_ack: Mutex<Option<usize>>,
    cancel_boundary: Mutex<Option<(usize, CancellationToken)>>,
    reject_reservation: Mutex<bool>,
    state: Mutex<Option<Checkpoint>>,
    reservations: Mutex<BTreeMap<usize, (Vec<u8>, usize)>>,
    interrupt_after: Mutex<Option<usize>>,
    view: Mutex<Option<views::SourceView>>,
    view_calls: Mutex<usize>,
}
#[async_trait]
impl Journal for MemoryJournal {
    async fn source_view(
        &self,
        _: &str,
        _: &Limits,
        _: &CancellationToken,
    ) -> Result<views::SourceView, AgentError> {
        *self.view_calls.lock().unwrap() += 1;
        self.view
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| AgentError::new("SOURCE_VIEW_UNAVAILABLE", "injected unavailable view"))
    }
    async fn load(&self) -> Result<Option<Checkpoint>, AgentError> {
        Ok(self.state.lock().unwrap().clone())
    }
    async fn reserve(&self, state: &Checkpoint, body: &[u8]) -> Result<Option<usize>, AgentError> {
        if *self.reject_reservation.lock().unwrap() {
            return Err(crate::agent_error::AgentError::new(
                "INTERNAL",
                "reservation transaction rejected",
            ));
        }
        self.sdk_turns
            .lock()
            .unwrap()
            .push(state.journal.session.as_ref().unwrap().turn());
        let mut rows = self.reservations.lock().unwrap();
        let row = rows.entry(state.turn).or_insert_with(|| (body.to_vec(), 0));
        assert_eq!(
            row.0, body,
            "same boundary must preserve exact provider body"
        );
        if row.1 == 3 {
            return Ok(None);
        }
        row.1 += 1;
        *self.state.lock().unwrap() = Some(state.clone());
        if let Some((sequence, token)) = &*self.cancel_boundary.lock().unwrap()
            && *sequence == state.journal.sequence
        {
            token.cancel();
        }
        let mut boundary = self.fail_boundary_ack.lock().unwrap();
        if *boundary == Some(state.journal.sequence) {
            *boundary = None;
            return Err(crate::agent_error::AgentError::new(
                "INTERNAL",
                "lost boundary acknowledgement",
            ));
        }
        Ok(Some(row.1))
    }
    async fn save(&self, state: &Checkpoint, _: &Value) -> Result<(), AgentError> {
        *self.state.lock().unwrap() = Some(state.clone());
        if let Some((sequence, token)) = &*self.cancel_boundary.lock().unwrap()
            && *sequence == state.journal.sequence
        {
            token.cancel();
        }
        let mut boundary = self.fail_boundary_ack.lock().unwrap();
        if *boundary == Some(state.journal.sequence) {
            *boundary = None;
            return Err(crate::agent_error::AgentError::new(
                "INTERNAL",
                "lost boundary acknowledgement",
            ));
        }
        let mut interrupt = self.interrupt_after.lock().unwrap();
        if *interrupt == Some(state.turn) && state.journal.pending.is_none() {
            *interrupt = None;
            return Err(AgentError::new(
                "INTERNAL",
                "simulated lost checkpoint acknowledgement",
            ));
        }
        Ok(())
    }
}

fn test_view() -> views::SourceView {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use sha2::{Digest, Sha256};
    let image = image::RgbImage::from_pixel(8, 8, image::Rgb([255, 255, 255]));
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut bytes)
        .encode_image(&image)
        .unwrap();
    views::SourceView {
        identity: views::ViewIdentity {
            source_id: "source".into(),
            original_sha256: "a".repeat(64),
            image_sha256: hex::encode(Sha256::digest(&bytes)),
            page_ordinal: 0,
            width: 8,
            height: 8,
            renderer: "docreader-source-view-v1/test-fixture".into(),
        },
        jpeg_base64: STANDARD.encode(bytes),
    }
}

struct Script {
    calls: Mutex<VecDeque<(String, Value)>>,
    bodies: Mutex<Vec<Value>>,
}
#[async_trait]
impl Model for Script {
    async fn turn(&self, _: &Config, body: &[u8]) -> Result<ChatTurn, AgentError> {
        let body: Value = serde_json::from_slice(body).unwrap();
        self.bodies.lock().unwrap().push(body.clone());
        let (name, mut args) = self
            .calls
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected model call");
        if name == "delete_review_finding" && args["id"] == "$fixture_finding" {
            args["id"] = body["messages"]
                .as_array()
                .unwrap()
                .iter()
                .rev()
                .find_map(|message| {
                    let result: Value = serde_json::from_str(message["content"].as_str()?).ok()?;
                    result["result"]["items"]
                        .as_array()?
                        .first()?
                        .get("id")
                        .cloned()
                })
                .expect("fixture explicitly retrieved its finding");
        }
        if name == "put_repair_result" && args["finding_sha256"] == "$fixture_repair" {
            let packet: Value = serde_json::from_str(
                body["messages"].as_array().unwrap().last().unwrap()["content"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap();
            args["finding_sha256"] =
                packet["review_findings"]["repair"]["next_finding"]["finding_sha256"].clone();
            if args["candidate_refs"] == json!(["$fixture_record"]) {
                let id = body["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .rev()
                    .find_map(|message| {
                        let result: Value =
                            serde_json::from_str(message["content"].as_str()?).ok()?;
                        let item = result["result"]["items"]
                            .as_array()?
                            .iter()
                            .find(|item| item.get("data").is_some())?;
                        item.get("id").cloned()
                    })
                    .expect("fixture explicitly inspected its repaired record");
                args["candidate_refs"] = json!([format!("record:{}", id.as_str().unwrap())]);
            }
        }
        let tools = fixture_review_calls(&body, &name, &args);
        Ok(ChatTurn {
            usage: None,
            content: String::new(),
            finish_reason: "tool_calls".into(),
            tool_calls: tools.unwrap_or_else(|| {
                vec![ChatToolCall {
                    id: format!("call-{}", self.calls.lock().unwrap().len()),
                    name,
                    arguments: args.to_string(),
                }]
            }),
        })
    }
}

/// Scripted semantic assertions for synthetic fixtures, using the real task
/// identity from the request. This does not run the completion reducer or
/// manufacture durable receipts; every assertion still traverses the tools.
fn fixture_review_calls(body: &Value, name: &str, args: &Value) -> Option<Vec<ChatToolCall>> {
    if name != "put_source_review" || !(args == &json!({}) || args.get("fixture_status").is_some())
    {
        return None;
    }
    let packet: Value =
        serde_json::from_str(body["messages"].as_array()?.last()?["content"].as_str()?).ok()?;
    let current = &packet["source_review"]["current"];
    let task = &current["task"];
    let source = task["source_id"].as_str()?;
    let region = &task["region"];
    if region["kind"] != "text" {
        return None;
    }
    let sources = json!([{"source_id":source,"start":region["start"],"end":region["end"]}]);
    let mut calls = Vec::new();
    let mut emit = |name: &str, args: Value| {
        calls.push(ChatToolCall {
            id: format!("fixture-{}", calls.len()),
            name: name.into(),
            arguments: args.to_string(),
        })
    };
    for reference in current["pending_candidate_refs"]["items"].as_array()? {
        emit(
            "complete_review_check",
            json!({"reference":reference,"summary":"Synthetic fixture comparison against the cited original.","sources":sources}),
        );
    }
    let mut finding_ids = vec![];
    if args["fixture_status"] == "findings" {
        for message in body["messages"].as_array()? {
            let Some(content) = message["content"].as_str() else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<Value>(content) else {
                continue;
            };
            if value["result"]["saved"] == true
                && let Some(id) = value["result"]["id"].as_str()
            {
                finding_ids.push(id.to_owned());
            }
        }
    }
    let boundary = json!({"state":"complete","reason":"The synthetic paragraph is complete at this boundary.","sources":sources});
    let template_mappings = if args["fixture_template_mappings"] == true {
        current["templates_requiring_mapping_judgment"]["items"].as_array()?.iter()
            .map(|id| json!({"template_id":id,"requirement_ids":[],"relation_ids":[],
                "finding_ids":[],"sources":sources,
                "reason":"Synthetic applicability-only fixture contains one standalone template and no separate requirement record."}))
            .collect::<Vec<_>>()
    } else {
        vec![]
    };
    let relationship_checks = current["records_requiring_relationship_judgment"]["items"]
        .as_array()?.iter().map(|id| {
            let unresolved = args["fixture_unresolved"] == true;
            json!({"record_id":id,"status":args.get("fixture_relationship_status").unwrap_or(&json!("not_required")),
                "related_record_ids":[],"relation_ids":[],"unresolved_record_ids":if unresolved {json!([id])} else {json!([])},
                "finding_ids":[],"reason":"Synthetic fixture: standalone bidder condition or documented unavailable evidence, without an external selected condition or continuation target.","sources":sources})
        }).collect::<Vec<_>>();
    emit(
        "put_source_review",
        json!({"task_id":task["id"],"expected_version":current["expected_version"],
        "status":if finding_ids.is_empty(){"checked"}else{"findings"},"summary":"Synthetic fixture source-to-result judgment, including omissions and boundaries.",
        "sources":sources,"candidate_refs":current["pending_candidate_refs"]["items"],"template_mappings":template_mappings,"relationship_checks":relationship_checks,"boundaries":{"before":boundary,"after":boundary},
        "finding_ids":finding_ids,"evidence_requests":[]}),
    );
    Some(calls)
}
pub(super) fn config() -> Config {
    // Test provider never makes network calls; production resolves this identity
    // from configuration and freezes it before execution.
    let provider:AuthoringRuntimeContractV1=serde_json::from_value(json!({"schema_version":1,"base_url":"https://llm.example/v1",
        "endpoint":"https://llm.example/v1/chat/completions","protocol":"openai_chat_completions_sse","model_id":"test-frozen-model",
        "credential_ref":"env:LLM_API_KEY","stream":true,"max_tokens":8192,"timeout_ms":180000,"response_mode":"tool_calls",
        "transport_retries":0,"temperature":null,"reasoning_effort":null})).unwrap();
    Config::with_provider(
        provider,
        Limits {
            max_no_progress_turns: 6,
            max_focus_turns: 24,
            max_focus_replans: 2,
            max_turns: 32,
            max_tool_calls: 40,
            max_read_bytes: 1000000,
            max_context_bytes: 100000,
            max_history_bytes: 32000,
            max_context_tokens: 1_000_000,
            image_token_reserve: 16000,
            token_safety_margin: 2048,
            max_tool_result_bytes: 16000,
            max_review_rounds: 3,
            max_source_view_edge: 1600,
            max_source_view_bytes: 16000,
        },
    )
    .unwrap()
}

fn active_work(source_id: &str) -> Value {
    json!({"source_scope":[source_id],"objective":"核对当前来源及其响应要求",
        "focus":{"action":"locate","source_spans":[],"references":[]},"status":"active","note":"从来源提取，保留跨范围引用"})
}

fn work_script(calls: Vec<(&str, Value)>) -> Script {
    Script {
        calls: Mutex::new(calls.into_iter().map(|(n, v)| (n.into(), v)).collect()),
        bodies: Mutex::new(vec![]),
    }
}

async fn fresh_review_journal() -> MemoryJournal {
    fresh_review_journal_config(&config()).await
}

async fn fresh_review_journal_config(config: &Config) -> MemoryJournal {
    let journal = MemoryJournal::default();
    agent::run(
        &input(),
        config,
        &journal,
        &script(),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    let mut state = journal.load().await.unwrap().unwrap();
    state.role = Role::Reviewer;
    state.done = false;
    state.review = None;
    state.reviewer_coverage = Coverage::default();
    state.reviewer_progress = Default::default();
    state.pending_coverage = None;
    state.reviewer_work = None;
    state.transcript.clear();
    state.source_review = Some(source_review::initialize(&input(), config).unwrap());
    source_review::select_next(&input(), config, &mut state).unwrap();
    *journal.state.lock().unwrap() = Some(state);
    journal
}

fn script() -> Script {
    let read = (
        "read_source",
        json!({"source_id":"source","start":0,"max_bytes":1024}),
    );
    let calls = vec![
        ("set_work_note", active_work("source")),
        read.clone(),
        (
            "set_disposition",
            json!({"source_id":"source","state":"non_requirement","reason":"incorrect initial interpretation"}),
        ),
        ("request_review", json!({})),
        ("set_work_note", active_work("source")),
        ("put_source_review", json!({})),
        read.clone(),
        (
            "inspect_analysis",
            json!({"view":"detail","kind":"disposition","offset":0,"limit":10}),
        ),
        (
            "put_review_finding",
            json!({"id":null,"finding":{"code":"OMITTED_REQUIREMENT","message":"遗漏附表提交义务","correction":"按所引原文补全并重新核对该字段", "affected":[],"sources":[span()]}}),
        ),
        ("put_source_review", json!({"fixture_status":"findings"})),
        ("inspect_review", json!({"offset":0,"limit":10})),
        ("put_record", requirement()),
        (
            "set_disposition",
            json!({"source_id":"source","state":"requirement","reason":"须知要求按附表提交"}),
        ),
        (
            "inspect_analysis",
            json!({"view":"detail","kind":"all","offset":0,"limit":10}),
        ),
        (
            "put_repair_result",
            json!({"finding_sha256":"$fixture_repair", "conclusion":"revised",
            "summary":"Added the previously omitted submission requirement from the cited fixture source.",
            "sources":[span()],"candidate_refs":["$fixture_record"]}),
        ),
        ("request_review", json!({})),
        ("set_work_note", active_work("source")),
        read,
        ("put_source_review", json!({})),
        (
            "inspect_analysis",
            json!({"view":"detail","kind":"disposition","offset":0,"limit":10}),
        ),
        (
            "inspect_analysis",
            json!({"view":"detail","kind":"all","offset":0,"limit":10}),
        ),
        ("inspect_review", json!({"offset":0,"limit":10})),
        ("delete_review_finding", json!({"id":"$fixture_finding"})),
        ("put_source_review", json!({})),
    ];
    Script {
        calls: Mutex::new(calls.into_iter().map(|(n, v)| (n.into(), v)).collect()),
        bodies: Mutex::new(vec![]),
    }
}

fn field_relation_fixture() -> (FrozenInput, Analysis, Value) {
    let mut input = input();
    input.structured_forms = vec![json!({
        "form_definition_revision_id":"form-a", "source_unit_revision_id":"source",
        "definition":{"kind":"grid","row_count":2,"column_count":2,"widths_mm":[40,40],"cells":[
            {"row":0,"column":0,"row_span":1,"col_span":2,"text":"同号附表"},
            {"row":1,"column":0,"row_span":1,"col_span":1,"text":"分项"},
            {"row":1,"column":1,"row_span":1,"col_span":1,"text":"合计"}
        ]}
    })];
    let mut analysis = read_analysis(&input);
    analysis
        .coverage
        .form_cells
        .insert("form-a".into(), vec![(0, 4)]);
    analysis.dispositions.insert(
        "source".into(),
        Disposition {
            state: DispositionState::Requirement,
            reason: "表格字段".into(),
        },
    );
    let record:Record=serde_json::from_value(json!({"id":"template-a","sources":[span()],"data":{
        "kind":"template","label":"附表","title":"同号附表","parent":null,"order":null,"purpose":"投标格式",
        "applicability":{"state":"applicable","scope":"本项目","condition":"原文指定","grounds":[span()]},
        "regions":[{"source":span(),"role":"fixed_text","form_id":"form-a","cells":[{"row":0,"column":0}],"instruction":"保留"},
                   {"source":span(),"role":"bidder_blank","form_id":"form-a","cells":[{"row":1,"column":0},{"row":1,"column":1}],"instruction":"后续填写"}]
    }})).unwrap();
    analysis.records.insert(record.id.clone(), record);
    let args = json!({"id":null,"from":"template-a","to":"template-a",
        "from_target":{"kind":"template_cell","form_id":"form-a","row":1,"column":0},
        "to_target":{"kind":"template_cell","form_id":"form-a","row":1,"column":1},
        "kind":"aggregates","state":"explicit","scope":"当前附件","explanation":"分项汇总到合计，保留原文条件，不执行计算","grounds":[span()]});
    (input, analysis, args)
}
