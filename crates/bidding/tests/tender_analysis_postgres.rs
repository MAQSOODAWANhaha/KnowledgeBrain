//! Run explicitly against a disposable database with the three fresh baselines.
//! Model turns are scripted: this validates persistence, not semantic accuracy.
use async_trait::async_trait;
use bidding::{
    agent_error::AgentError,
    authoring_runtime::AuthoringRuntimeContractV1,
    tender_analysis::{
        FrozenInput,
        agent::{Config, Limits, Model},
        postgres,
    },
};
use knowledge::models::{ChatToolCall, ChatTurn};
use platform::BidAuthoringRequestIdentityV2;
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{collections::VecDeque, sync::Mutex};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[path = "support/global_review_publication.rs"]
mod global_review_publication;

fn runtime_pool_options(
    base: &sqlx::postgres::PgConnectOptions,
    role: &str,
) -> sqlx::postgres::PgConnectOptions {
    let password_key = match role {
        "kb_runtime_api" => "KNOWLEDGEBRAIN_API_DB_PASSWORD",
        "kb_runtime_worker" => "KNOWLEDGEBRAIN_WORKER_DB_PASSWORD",
        _ => panic!("unsupported runtime test role"),
    };
    let options = base.clone().username(role);
    match std::env::var(password_key) {
        Ok(password) => options.password(&password),
        Err(_) => options,
    }
}

fn config() -> Config {
    let provider: AuthoringRuntimeContractV1 = serde_json::from_value(json!({
        "schema_version":1,"base_url":"https://model.example.invalid/v1",
        "endpoint":"https://model.example.invalid/v1/chat/completions",
        "protocol":"openai_chat_completions_sse","model_id":"scripted-test",
        "credential_ref":"env:LLM_API_KEY","stream":true,"max_tokens":4096,"timeout_ms":90000,
        "response_mode":"tool_calls","transport_retries":0,"temperature":null,"reasoning_effort":"medium"
    })).unwrap();
    Config::with_provider(
        provider,
        Limits {
            max_no_progress_turns: 6,
            max_focus_turns: 24,
            max_focus_replans: 2,
            max_turns: 40,
            max_tool_calls: 50,
            max_read_bytes: 1_000_000,
            max_context_bytes: 200_000,
            max_history_bytes: 64000,
            max_context_tokens: 1_000_000,
            image_token_reserve: 16000,
            token_safety_margin: 2048,
            max_tool_result_bytes: 16_000,
            max_review_rounds: 3,
            max_source_view_edge: 1600,
            max_source_view_bytes: 16000,
            reviewer_reserve: 0,
            pack_max_units: 1,
            pack_max_chars: 0,
            pack_max_turns: 0,
            draft_path: false,
            draft_bind_terms: vec![],
            max_draft_docx_bytes: bidding::tender_analysis::draft::DRAFT_MAX_DOCX_BYTES,
        },
    )
    .unwrap()
}

async fn seed(pool: &PgPool) -> (BidAuthoringRequestIdentityV2, FrozenInput) {
    seed_original(pool, None).await
}

async fn seed_original(
    pool: &PgPool,
    original: Option<&[u8]>,
) -> (BidAuthoringRequestIdentityV2, FrozenInput) {
    let mut tx = pool.begin().await.unwrap();
    if let Some(bytes) = original {
        sqlx::query("SELECT set_config('kb_test.original_hex',$1,true),set_config('kb_test.media_type','image/png',true)")
            .bind(hex::encode(bytes)).execute(&mut *tx).await.unwrap();
    }
    sqlx::query("SELECT set_config('kb_test.runtime',$1,true)")
        .bind(serde_json::to_string(&config()).unwrap())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::raw_sql(include_str!("sql/tender_analysis_seed.sql"))
        .execute(&mut *tx)
        .await
        .unwrap();
    let value: Value = sqlx::query_scalar("SELECT current_setting('kb_test.request')::jsonb")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    let request:BidAuthoringRequestIdentityV2=serde_json::from_value(json!({
        "request_artifact_id":value["request_artifact_id"],"request_revision":value["request_revision"],
        "frozen_input_sha256":value["frozen_input_sha256"]
    })).unwrap();
    let bundle: Value =
        sqlx::query_scalar("SELECT kb_bid_v2_load_tender_analysis_input($1,$2,$3::kb_sha256)")
            .bind(request.request_artifact_id)
            .bind(request.request_revision)
            .bind(&request.frozen_input_sha256)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    tx.commit().await.unwrap();
    (
        request,
        serde_json::from_value(bundle["input"].clone()).unwrap(),
    )
}

struct Script {
    turns: Mutex<VecDeque<(&'static str, Value)>>,
    bodies: Mutex<Vec<Vec<u8>>>,
}
#[async_trait]
impl Model for Script {
    async fn turn(&self, _: &Config, body: &[u8]) -> Result<ChatTurn, AgentError> {
        self.bodies.lock().unwrap().push(body.to_vec());
        let (name, mut args) = self
            .turns
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected model call");
        if name == "put_relation" {
            let request: Value = serde_json::from_slice(body).unwrap();
            let record_id = request["messages"]
                .as_array()
                .unwrap()
                .iter()
                .rev()
                .filter(|m| m["role"] == "tool")
                .filter_map(|m| serde_json::from_str::<Value>(m["content"].as_str()?).ok())
                .find_map(|v| {
                    v["result"]["relations_to_recheck"]
                        .is_array()
                        .then(|| v["result"]["id"].clone())
                })
                .expect("script must have received put_record result");
            args["from"] = record_id.clone();
            args["to"] = record_id;
        }
        if name == "delete_review_finding" {
            let request: Value = serde_json::from_slice(body).unwrap();
            args["id"] = request["messages"]
                .as_array()
                .unwrap()
                .iter()
                .rev()
                .filter(|m| m["role"] == "tool")
                .filter_map(|m| serde_json::from_str::<Value>(m["content"].as_str()?).ok())
                .find_map(|v| v["result"]["items"][0]["id"].as_str().map(str::to_owned))
                .expect("review draft must have been delivered")
                .into();
        }
        let mut tool_calls = Vec::new();
        if name == "put_analysis_check" {
            let body: Value = serde_json::from_slice(body).unwrap();
            let packet: Value = serde_json::from_str(
                body["messages"].as_array().unwrap().last().unwrap()["content"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap();
            let global = &packet["global_analysis_checks"];
            for key in global["pending_keys"].as_array().unwrap() {
                tool_calls.push(ChatToolCall {
                    id: Uuid::new_v4().to_string(),
                    name: name.into(),
                    arguments: json!({"key":key,
                        "expected_scope_sha256":global["expected_scope_sha256"],
                        "conclusion":"pass","grounds":args["grounds"],
                        "record_ids":[],"finding_ids":[]})
                    .to_string(),
                });
            }
            assert!(
                !tool_calls.is_empty(),
                "script expected pending global checks"
            );
            return Ok(ChatTurn {
                usage: None,
                content: String::new(),
                finish_reason: "tool_calls".into(),
                tool_calls,
            });
        }
        if name == "put_source_review" {
            let body: Value = serde_json::from_slice(body).unwrap();
            let packet: Value = serde_json::from_str(
                body["messages"].as_array().unwrap().last().unwrap()["content"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap();
            let current = &packet["source_review"]["current"];
            let task = &current["task"];
            let sources = json!([{"source_id":task["source_id"],"start":task["region"]["start"],"end":task["region"]["end"]}]);
            for reference in current["pending_candidate_refs"]["items"]
                .as_array()
                .unwrap()
            {
                tool_calls.push(ChatToolCall {id:Uuid::new_v4().to_string(),name:"complete_review_check".into(),
                    arguments:json!({"reference":reference,"summary":"Scripted comparison against the isolated fixture source.","sources":sources}).to_string()});
            }
            let boundary = json!({"state":"complete","reason":"Complete isolated fixture clause.","sources":sources});
            args = json!({"task_id":task["id"],"expected_version":current["expected_version"],"status":"checked",
                "summary":"The isolated fixture has no unreported omissions or boundary issues.","sources":sources,
                "candidate_refs":current["pending_candidate_refs"]["items"],"template_mappings":[],"boundaries":{"before":boundary,"after":boundary},
                "finding_ids":[],"evidence_requests":[]});
        }
        tool_calls.push(ChatToolCall {
            id: Uuid::new_v4().to_string(),
            name: name.into(),
            arguments: args.to_string(),
        });
        Ok(ChatTurn {
            usage: None,
            content: String::new(),
            finish_reason: "tool_calls".into(),
            tool_calls,
        })
    }
}
fn script(input: &FrozenInput) -> Script {
    let source = &input.source_units[0];
    let span =
        json!({"source_id":source.source_unit_revision_id,"start":0,"end":source.text.len()});
    let read = (
        "read_source",
        json!({"source_id":source.source_unit_revision_id,"start":0,"max_bytes":1024}),
    );
    let metadata = (
        "collection_index",
        json!({"kind":"documents","offset":0,"limit":10}),
    );
    let requirement = json!({"id":null,"sources":[span],"data":{
        "kind":"requirement","text":source.text,"categories":["format","attachment"],"strength":"mandatory",
        "compliance":[{"policy":"must_comply","condition":"提交要求","grounds":[span]},
            {"policy":"explicit_response","condition":"提交要求","grounds":[span]}],
        "applicability":{"state":"applicable","scope":"本次投标","condition":"提交要求","grounds":[span]},
        "response":[{"channel":"response_table","description":"响应表","condition":"提交时","grounds":[span]},
            {"channel":"evidence_attachment","description":"证明材料","condition":"提交时","grounds":[span]}],
        "scoring_rule":null,"proofs":[],"criteria":[]
    }});
    let work = (
        "set_work_note",
        json!({"source_scope":[source.source_unit_revision_id],
        "objective":"核对测试来源及要求","focus":{"action":"locate","source_spans":[],"references":[]},"status":"active","note":"独立读取原文"}),
    );
    let turns = vec![
        work.clone(),
        metadata.clone(),
        read.clone(),
        ("put_record", requirement),
        (
            "put_relation",
            json!({"id":null,"from":null,"to":null,
            "from_target":{"kind":"response","index":0},"to_target":{"kind":"response","index":1},
            "kind":"references","state":"explicit","scope":"本次投标",
            "explanation":"表格响应对应证明材料响应位置","grounds":[span]}),
        ),
        (
            "set_disposition",
            json!({"source_id":source.source_unit_revision_id,"state":"requirement","reason":"提交义务"}),
        ),
        ("put_analysis_check", json!({"grounds":[span]})),
        ("request_review", json!({})),
        work,
        metadata,
        read,
        (
            "inspect_analysis",
            json!({"view":"detail","kind":"all","offset":0,"limit":10}),
        ),
        (
            "inspect_analysis",
            json!({"view":"detail","kind":"disposition","offset":0,"limit":10}),
        ),
        (
            "inspect_analysis",
            json!({"view":"detail","kind":"relation","offset":0,"limit":10}),
        ),
        ("put_analysis_check", json!({"grounds":[span]})),
        ("put_source_review", json!({})),
    ];
    Script {
        turns: Mutex::new(turns.into()),
        bodies: Mutex::new(vec![]),
    }
}

struct LostReviewAck<'a> {
    journal: postgres::PgJournal<'a>,
    fail_turn: usize,
    fail_sequence: Option<usize>,
    reject_prepared: bool,
}

#[async_trait]
impl bidding::tender_analysis::agent::Journal for LostReviewAck<'_> {
    async fn load(
        &self,
    ) -> Result<Option<bidding::tender_analysis::agent::Checkpoint>, AgentError> {
        self.journal.load().await
    }

    async fn reserve(
        &self,
        state: &bidding::tender_analysis::agent::Checkpoint,
        body: &[u8],
    ) -> Result<Option<usize>, AgentError> {
        let mut state = state.clone();
        if self.reject_prepared {
            state.tool_calls += 1;
        }
        let reservation = self.journal.reserve(&state, body).await?;
        if self.fail_sequence == Some(state.journal.sequence) {
            return Err(AgentError::new("INTERNAL", "lost prepared ACK"));
        }
        Ok(reservation)
    }

    async fn save(
        &self,
        state: &bidding::tender_analysis::agent::Checkpoint,
        progress: &Value,
    ) -> Result<(), AgentError> {
        self.journal.save(state, progress).await?;
        if (state.turn == self.fail_turn && state.journal.pending.is_none())
            || self.fail_sequence == Some(state.journal.sequence)
        {
            return Err(AgentError::new(
                "INTERNAL",
                "committed review checkpoint response lost",
            ));
        }
        Ok(())
    }
}

#[tokio::test]
#[ignore = "requires KB_TENDER_AGENT_TEST_DATABASE_URL pointing to a fresh owned test database"]
async fn review_draft_survives_committed_checkpoint_ack_loss() {
    use bidding::tender_analysis::agent::{self, Journal};
    let url =
        std::env::var("KB_TENDER_AGENT_TEST_DATABASE_URL").expect("dedicated test URL required");
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .unwrap();
    let db: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        db.starts_with("knowledgebrain_test_"),
        "refuse non-test database"
    );
    let (request, input) = seed(&pool).await;
    let owner = composition_claim(&pool, &request).await;
    let model = script(&input);
    let source = &input.source_units[0];
    let fail_turn = {
        let mut turns = model.turns.lock().unwrap();
        let end = turns.len() - 1;
        turns.insert(end, ("put_review_finding", json!({"id":null,"finding":{
            "code":"FIELD_RECHECK","message":"核对响应位置与原文的对应关系",
            "correction":"核对原文和具体响应位置", "affected":[],"sources":[{"source_id":source.source_unit_revision_id,"start":0,"end":source.text.len()}]
        }})));
        turns.insert(end + 1, ("inspect_review", json!({"offset":0,"limit":1})));
        turns.insert(end + 2, ("delete_review_finding", json!({"id":null})));
        end + 1
    };
    let journal = LostReviewAck {
        journal: postgres::PgJournal {
            pool: &pool,
            request: &request,
            owner: &owner,
            source_reader: None,
        },
        fail_turn,
        fail_sequence: None,
        reject_prepared: false,
    };
    let failed = agent::run(
        &input,
        &config(),
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(failed.code, "INTERNAL");
    let saved = journal.load().await.unwrap().unwrap();
    assert_eq!(saved.turn, fail_turn);
    assert_eq!(saved.review_draft.len(), 1);
    assert_eq!(
        saved.review_draft.values().next().unwrap().code,
        "FIELD_RECHECK"
    );
    let result = agent::run(
        &input,
        &config(),
        &journal.journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(result.quality, "verified");
    assert!(result.review.findings.is_empty());
    assert!(
        journal
            .load()
            .await
            .unwrap()
            .unwrap()
            .review_draft
            .is_empty()
    );
    assert_eq!(
        model.bodies.lock().unwrap().len(),
        fail_turn + 3,
        "resume must not repeat the saved finding turn"
    );
    pool.close().await;
}

#[tokio::test]
#[ignore = "requires KB_TENDER_AGENT_TEST_DATABASE_URL pointing to a fresh owned test database"]
async fn analysis_publishes_constraints_without_fabricating_submission_needs() {
    let url =
        std::env::var("KB_TENDER_AGENT_TEST_DATABASE_URL").expect("dedicated test URL required");
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .unwrap();
    let db: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        db.starts_with("knowledgebrain_test_"),
        "refuse non-test database"
    );
    let empty = json!({"kind":"all_of","children":[]});
    for (expr, valid) in [
        (empty.clone(), true),
        (json!({"kind":"any_of","children":[]}), false),
        (json!({"kind":"all_of","children":[empty]}), false),
        (json!({"kind":"all_of","children":[],"extra":true}), false),
    ] {
        let actual: bool = sqlx::query_scalar("SELECT kb_bid_v2_fulfillment_expr_valid($1)")
            .bind(expr)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(actual, valid);
    }
    let (request, input) = seed(&pool).await;
    let model = script(&input);
    // Exercise the storage shape, not the scripted model's semantic interpretation.
    {
        let mut turns = model.turns.lock().unwrap();
        turns.retain(|(name, _)| *name != "put_relation");
        let (_, record) = turns
            .iter_mut()
            .find(|(name, _)| *name == "put_record")
            .unwrap();
        record["data"]["response"] = json!([]);
        record["data"]["compliance"]
            .as_array_mut()
            .unwrap()
            .truncate(1);
        let mut second = record["data"]["compliance"][0].clone();
        second["condition"] = json!("另一个适用条件");
        record["data"]["compliance"]
            .as_array_mut()
            .unwrap()
            .push(second);
    }
    let result = postgres::execute_with_model(&pool, &request, &CancellationToken::new(), &model)
        .await
        .unwrap();
    assert_eq!(result["analysis_quality"], "verified", "{result}");
    let stored: Value = sqlx::query_scalar("SELECT convert_from(canonical_payload,'UTF8')::jsonb FROM bid_requirement_revision_artifacts WHERE project_id=$1")
        .bind(Uuid::parse_str(&input.project_id).unwrap()).fetch_one(&pool).await.unwrap();
    assert_eq!(stored["response_needs"], json!([]));
    assert_eq!(
        stored["fulfillment_expr"],
        json!({"kind":"all_of","children":[]})
    );
    assert_eq!(stored["compliance_policy"], "must_comply");
    pool.close().await;
}

#[tokio::test]
#[ignore = "requires KB_TENDER_AGENT_TEST_DATABASE_URL pointing to a fresh owned test database"]
async fn analysis_publication_replay_fencing_and_physical_budget() {
    let url =
        std::env::var("KB_TENDER_AGENT_TEST_DATABASE_URL").expect("dedicated test URL required");
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .unwrap();
    let db: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        db.starts_with("knowledgebrain_test_"),
        "refuse non-test database"
    );
    let (request, input) = seed(&pool).await;
    let model = script(&input);
    let result = postgres::execute_with_model(&pool, &request, &CancellationToken::new(), &model)
        .await
        .unwrap();
    let diagnostic:Option<String>=sqlx::query_scalar("SELECT last_error_message FROM bid_tender_agent_run_artifacts WHERE request_artifact_id=$1 ORDER BY attempt DESC LIMIT 1")
        .bind(request.request_artifact_id).fetch_one(&pool).await.unwrap();
    assert_eq!(
        result["analysis_quality"], "verified",
        "{result}; {diagnostic:?}"
    );
    let payload:Value=sqlx::query_scalar("SELECT convert_from(canonical_payload,'UTF8')::jsonb FROM bid_requirement_set_artifacts WHERE project_id=$1 ORDER BY revision DESC LIMIT 1")
        .bind(Uuid::parse_str(&input.project_id).unwrap()).fetch_one(&pool).await.unwrap();
    assert_eq!(
        payload["analysis_result"]["analysis"]["records"]
            .as_object()
            .unwrap()
            .len(),
        1
    );
    let analysis: bidding::tender_analysis::AnalysisResult =
        serde_json::from_value(payload["analysis_result"].clone()).unwrap();
    assert_eq!(analysis.analysis.relations.len(), 1);
    let relation = analysis.analysis.relations.values().next().unwrap();
    bidding::tender_analysis::tools::validate_relation(&input, &analysis.analysis, relation)
        .unwrap();
    let set_id: Uuid = sqlx::query_scalar("SELECT id FROM bid_requirement_set_artifacts WHERE project_id=$1 ORDER BY revision DESC LIMIT 1")
        .bind(Uuid::parse_str(&input.project_id).unwrap()).fetch_one(&pool).await.unwrap();
    let actor: String =
        sqlx::query_scalar("SELECT 'user:'||owner_user_id FROM bid_projects WHERE id=$1")
            .bind(Uuid::parse_str(&input.project_id).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    let page: Value = sqlx::query_scalar(
        "SELECT kb_bid_v2_get_tender_analysis($1,$2,$3::kb_actor_identity,'relation',0,10)",
    )
    .bind(Uuid::parse_str(&input.project_id).unwrap())
    .bind(set_id)
    .bind(actor)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        page["items"][0]["value"],
        serde_json::to_value(relation).unwrap()
    );
    let fulfillment: Value = sqlx::query_scalar(
        "SELECT fulfillment_expr FROM bid_requirement_revision_artifacts WHERE project_id=$1",
    )
    .bind(Uuid::parse_str(&input.project_id).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(fulfillment["children"][0]["channel"], "response_table");
    assert_eq!(fulfillment["children"][1]["channel"], "evidence_attachment");
    let calls = model.bodies.lock().unwrap().len();
    postgres::execute_with_model(&pool, &request, &CancellationToken::new(), &model)
        .await
        .unwrap();
    assert_eq!(
        model.bodies.lock().unwrap().len(),
        calls,
        "completed delivery must not call model again"
    );

    let (request, input) = seed(&pool).await;
    let claim: Value =
        sqlx::query_scalar("SELECT kb_bid_v2_tender_agent_claim($1,$2,$3::kb_sha256)")
            .bind(request.request_artifact_id)
            .bind(request.request_revision)
            .bind(&request.frozen_input_sha256)
            .fetch_one(&pool)
            .await
            .unwrap();
    let unused = script(&input);
    let live = postgres::execute_with_model(&pool, &request, &CancellationToken::new(), &unused)
        .await
        .unwrap();
    assert_eq!(live["disposition"], "live_owner");
    assert!(unused.bodies.lock().unwrap().is_empty());
    let first_body: Value = serde_json::from_slice(&model.bodies.lock().unwrap()[0]).unwrap();
    assert_eq!(first_body["max_tokens"], 4096);
    assert_eq!(first_body["reasoning_effort"], "medium");
    assert_eq!(first_body["stream_options"], json!({"include_usage":true}));
    let mut without_usage = first_body.clone();
    without_usage
        .as_object_mut()
        .unwrap()
        .remove("stream_options");
    let rejected =
        sqlx::query("SELECT kb_bid_v2_tender_agent_reserve($1,$2::kb_sha256,$3,$4,0,'main',$5)")
            .bind(request.request_artifact_id)
            .bind(&request.frozen_input_sha256)
            .bind(claim["attempt"].as_i64().unwrap() as i32)
            .bind(Uuid::parse_str(claim["execution_owner_token"].as_str().unwrap()).unwrap())
            .bind(serde_json_canonicalizer::to_vec(&without_usage).unwrap())
            .execute(&pool)
            .await
            .unwrap_err();
    assert!(rejected.to_string().contains("provider contract changed"));
    for effort in [Some("high"), None] {
        let mut drift = first_body.clone();
        drift.as_object_mut().unwrap().remove("reasoning_effort");
        if let Some(effort) = effort {
            drift["reasoning_effort"] = json!(effort);
        }
        let rejected = sqlx::query(
            "SELECT kb_bid_v2_tender_agent_reserve($1,$2::kb_sha256,$3,$4,0,'main',$5)",
        )
        .bind(request.request_artifact_id)
        .bind(&request.frozen_input_sha256)
        .bind(claim["attempt"].as_i64().unwrap() as i32)
        .bind(Uuid::parse_str(claim["execution_owner_token"].as_str().unwrap()).unwrap())
        .bind(serde_json_canonicalizer::to_vec(&drift).unwrap())
        .execute(&pool)
        .await
        .unwrap_err();
        assert!(rejected.to_string().contains("provider contract changed"));
    }
    let reservations: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM bid_tender_agent_call_attempts WHERE request_artifact_id=$1",
    )
    .bind(request.request_artifact_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        reservations, 0,
        "changed tuning cannot consume a reservation"
    );
    let scoped: Value =
        sqlx::query_scalar("SELECT kb_bid_v2_tender_source_view_input($1,$2::kb_sha256,$3,$4,$5)")
            .bind(request.request_artifact_id)
            .bind(&request.frozen_input_sha256)
            .bind(claim["attempt"].as_i64().unwrap() as i32)
            .bind(Uuid::parse_str(claim["execution_owner_token"].as_str().unwrap()).unwrap())
            .bind(Uuid::parse_str(&input.source_units[0].source_unit_revision_id).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(scoped["document_id"], input.source_units[0].document_id);
    let foreign =
        sqlx::query("SELECT kb_bid_v2_tender_source_view_input($1,$2::kb_sha256,$3,$4,$5)")
            .bind(request.request_artifact_id)
            .bind(&request.frozen_input_sha256)
            .bind(claim["attempt"].as_i64().unwrap() as i32)
            .bind(Uuid::parse_str(claim["execution_owner_token"].as_str().unwrap()).unwrap())
            .bind(Uuid::new_v4())
            .execute(&pool)
            .await
            .unwrap_err();
    assert!(foreign.to_string().contains("not in frozen collection"));
    let rejected = sqlx::query("SELECT kb_bid_v2_tender_agent_heartbeat($1,$2::kb_sha256,$3,$4)")
        .bind(request.request_artifact_id)
        .bind(&request.frozen_input_sha256)
        .bind(claim["attempt"].as_i64().unwrap() as i32)
        .bind(Uuid::new_v4())
        .execute(&pool)
        .await
        .unwrap_err();
    assert!(rejected.to_string().contains("REQUEST_ATTEMPT_SUPERSEDED"));

    struct Unavailable(Mutex<Vec<Vec<u8>>>);
    #[async_trait]
    impl Model for Unavailable {
        async fn turn(&self, _: &Config, body: &[u8]) -> Result<ChatTurn, AgentError> {
            self.0.lock().unwrap().push(body.to_vec());
            Err(AgentError::new(
                "INTERNAL",
                "injected infrastructure interruption",
            ))
        }
    }
    let (request, _) = seed(&pool).await;
    let unavailable = Unavailable(Mutex::new(vec![]));
    assert!(
        postgres::execute_with_model(&pool, &request, &CancellationToken::new(), &unavailable)
            .await
            .is_err()
    );
    let result =
        postgres::execute_with_model(&pool, &request, &CancellationToken::new(), &unavailable)
            .await
            .unwrap();
    assert_eq!(result["status"], "failed", "{result}");
    let bodies = unavailable.0.lock().unwrap();
    assert_eq!(
        bodies.len(),
        3,
        "retry attempt cannot reset physical-call budget"
    );
    assert!(bodies.windows(2).all(|pair| pair[0] == pair[1]));
}

#[tokio::test]
#[ignore = "requires owned PostgreSQL and authenticated Python docreader at DOCREADER_ADDR"]
async fn source_views_cross_real_python_rpc_and_publish_frozen_pixels() {
    use bidding::tender_process::{TenderDocumentProcessError, TenderObjectReader};
    use std::sync::atomic::{AtomicUsize, Ordering};
    assert!(
        std::env::var("DOCREADER_ADDR").is_ok(),
        "real Python service required"
    );
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&std::env::var("KB_TENDER_AGENT_TEST_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let db: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(db.starts_with("knowledgebrain_test_"));
    let image = image::RgbImage::from_pixel(24, 12, image::Rgb([80, 160, 240]));
    let mut original = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut original, image::ImageFormat::Png)
        .unwrap();
    let original = original.into_inner();
    struct Reader {
        bytes: Vec<u8>,
        calls: AtomicUsize,
    }
    #[async_trait]
    impl TenderObjectReader for Reader {
        async fn read(
            &self,
            _: &str,
            _: &CancellationToken,
        ) -> Result<Vec<u8>, TenderDocumentProcessError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.bytes.clone())
        }
    }
    for corrupt in [false, true] {
        let (request, input) = seed_original(&pool, Some(&original)).await;
        let reader = Reader {
            bytes: if corrupt {
                b"changed original".to_vec()
            } else {
                original.clone()
            },
            calls: AtomicUsize::new(0),
        };
        let model = script(&input);
        {
            let mut turns = model.turns.lock().unwrap();
            let view = (
                "read_source_view",
                json!({"source_id":input.source_units[0].source_unit_revision_id}),
            );
            turns.insert(2, view.clone());
            let last = turns.len() - 1;
            turns.insert(last, view);
        }
        let result = postgres::execute_with_model_and_reader(
            &pool,
            &request,
            &CancellationToken::new(),
            &model,
            Some(&reader),
        )
        .await
        .unwrap();
        let error:Option<String>=sqlx::query_scalar("SELECT last_error_message FROM bid_tender_agent_run_artifacts WHERE request_artifact_id=$1 ORDER BY attempt DESC LIMIT 1")
            .bind(request.request_artifact_id).fetch_one(&pool).await.unwrap();
        let tool_errors: std::collections::BTreeSet<String> = model
            .bodies
            .lock()
            .unwrap()
            .iter()
            .map(|b| serde_json::from_slice::<Value>(b).unwrap())
            .flat_map(|b| b["messages"].as_array().unwrap().clone())
            .filter(|m| m["role"] == "tool")
            .filter_map(|m| m["content"].as_str().map(str::to_owned))
            .filter(|s| serde_json::from_str::<Value>(s).unwrap()["ok"] == false)
            .collect();
        assert_eq!(
            result["analysis_quality"],
            if corrupt { "needs_review" } else { "verified" },
            "{result};{error:?};{tool_errors:?}"
        );
        let payload:Value=sqlx::query_scalar("SELECT convert_from(canonical_payload,'UTF8')::jsonb FROM bid_requirement_set_artifacts WHERE project_id=$1 ORDER BY revision DESC LIMIT 1")
            .bind(Uuid::parse_str(&input.project_id).unwrap()).fetch_one(&pool).await.unwrap();
        let views = payload["analysis_result"]["source_views"]
            .as_object()
            .unwrap();
        if corrupt {
            assert!(views.is_empty());
        } else {
            assert_eq!(views.len(), 1);
            assert_eq!(reader.calls.load(Ordering::SeqCst), 1);
            let stored = views.values().next().unwrap();
            assert!(!stored["jpeg_base64"].as_str().unwrap().is_empty());
            assert_eq!(stored["identity"]["width"], 24);
            let bodies = model.bodies.lock().unwrap();
            let review_contains_pixels = bodies
                .iter()
                .map(|b| serde_json::from_slice::<Value>(b).unwrap())
                .filter(|b| {
                    b["messages"][0]["content"][0]["text"]
                        .as_str()
                        .unwrap()
                        .contains("INDEPENDENT")
                })
                .any(|b| {
                    b["messages"]
                        .to_string()
                        .contains("data:image/jpeg;base64,")
                });
            assert!(
                review_contains_pixels,
                "reviewer must receive actual pixels from Python RPC"
            );
        }
    }
}

#[path = "support/diagnostic_messages.rs"]
mod diagnostic_messages;

#[tokio::test]
#[ignore = "requires KB_TENDER_AGENT_TEST_DATABASE_URL pointing to a fresh owned test database"]
async fn analysis_diagnostics_preserve_utf8_and_reject_foreign_owners() {
    let url =
        std::env::var("KB_TENDER_AGENT_TEST_DATABASE_URL").expect("dedicated test URL required");
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .unwrap();
    let db: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        db.starts_with("knowledgebrain_test_"),
        "refuse non-test database"
    );
    let (request, _) = seed(&pool).await;
    let retired: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_proc WHERE pronamespace='public'::regnamespace AND
         (proname LIKE 'kb_bid_v21_%' OR proname='kb_bid_v2_create_outline_candidate')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(retired, 0, "retired generation must not remain callable");
    let before: Value = sqlx::query_scalar(
        "SELECT to_jsonb(r) FROM bid_async_request_snapshot_artifacts r WHERE id=$1",
    )
    .bind(request.request_artifact_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    for operation in ["fail", "yield"] {
        let sql = if operation == "fail" {
            "SELECT kb_bid_v2_tender_agent_fail($1,$2::kb_sha256,$3,$4,'AGENT_OUTPUT_INVALID',$5)"
        } else {
            "SELECT kb_bid_v2_tender_agent_yield_for_retry($1,$2::kb_sha256,$3,$4,'INTERNAL',$5)"
        };
        for (label, message) in diagnostic_messages::cases() {
            for foreign in [false, true] {
                let mut tx = pool.begin().await.unwrap();
                let claim: Value =
                    sqlx::query_scalar("SELECT kb_bid_v2_tender_agent_claim($1,$2,$3::kb_sha256)")
                        .bind(request.request_artifact_id)
                        .bind(request.request_revision)
                        .bind(&request.frozen_input_sha256)
                        .fetch_one(&mut *tx)
                        .await
                        .unwrap();
                assert_eq!(claim["disposition"], "claimed");
                let token = if foreign {
                    Uuid::new_v4()
                } else {
                    Uuid::parse_str(claim["execution_owner_token"].as_str().unwrap()).unwrap()
                };
                let result = sqlx::query(sql)
                    .bind(request.request_artifact_id)
                    .bind(&request.frozen_input_sha256)
                    .bind(claim["attempt"].as_i64().unwrap() as i32)
                    .bind(token)
                    .bind(message.as_deref())
                    .execute(&mut *tx)
                    .await;
                if foreign {
                    let error = result.expect_err("foreign owner must be rejected");
                    assert!(
                        error.to_string().contains("REQUEST_ATTEMPT_SUPERSEDED"),
                        "{operation}/{label}: {error}"
                    );
                } else if operation == "fail" && message.is_none() {
                    let error =
                        result.expect_err("terminal NULL diagnostic must still be rejected");
                    assert_eq!(
                        error.as_database_error().unwrap().code().as_deref(),
                        Some("23514")
                    );
                } else {
                    result.unwrap_or_else(|error| panic!("{operation}/{label}: {error}"));
                    let state: Value = sqlx::query_scalar("SELECT to_jsonb(r) FROM bid_tender_agent_run_artifacts r WHERE request_artifact_id=$1 ORDER BY attempt DESC LIMIT 1")
                        .bind(request.request_artifact_id).fetch_one(&mut *tx).await.unwrap();
                    let expected = message.as_deref().unwrap_or("");
                    diagnostic_messages::assert_message(
                        &state["last_error_message"],
                        Some(expected),
                    );
                    assert_eq!(
                        state["status"],
                        if operation == "fail" {
                            "failed"
                        } else {
                            "retry_yielded"
                        }
                    );
                    let status: String = sqlx::query_scalar(
                        "SELECT status FROM bid_async_request_snapshot_artifacts WHERE id=$1",
                    )
                    .bind(request.request_artifact_id)
                    .fetch_one(&mut *tx)
                    .await
                    .unwrap();
                    assert_eq!(
                        status,
                        if operation == "fail" {
                            "failed"
                        } else {
                            "pending"
                        }
                    );
                    if operation == "yield" {
                        diagnostic_messages::assert_message(
                            &state["progress_detail"]["last_error_message"],
                            Some(expected),
                        );
                    }
                }
                tx.rollback().await.unwrap();
                let after: Value = sqlx::query_scalar(
                    "SELECT to_jsonb(r) FROM bid_async_request_snapshot_artifacts r WHERE id=$1",
                )
                .bind(request.request_artifact_id)
                .fetch_one(&pool)
                .await
                .unwrap();
                assert_eq!(after, before, "rolled-back probe changed request");
            }
        }
    }
}

#[tokio::test]
#[ignore = "requires KB_TENDER_AGENT_TEST_DATABASE_URL pointing to a fresh owned test database"]
async fn composition_source_freezes_published_analysis_and_restores_history() {
    use bidding::{
        docx_composition::{agent as compose, postgres as composition},
        docx_round,
    };
    let url =
        std::env::var("KB_TENDER_AGENT_TEST_DATABASE_URL").expect("dedicated test URL required");
    let pool = PgPool::connect(&url).await.unwrap();
    let database: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(database.starts_with("knowledgebrain_test_"));
    let (request, input) = seed(&pool).await;
    let project_id = Uuid::parse_str(&input.project_id).unwrap();
    let (workspace, actor): (Uuid, String) = sqlx::query_as(
        "SELECT w.id,'user:'||p.owner_user_id FROM bid_submission_workspaces w JOIN bid_projects p ON p.id=w.project_id WHERE p.id=$1")
        .bind(project_id).fetch_one(&pool).await.unwrap();
    let composition_config = compose::Config {
        provider: config().provider,
        limits: compose::Limits {
            max_no_progress_turns: 6,
            max_focus_turns: 24,
            max_focus_replans: 2,
            max_turns: 20,
            max_tool_calls: 100,
            max_physical_calls: 30,
            max_read_bytes: 1000000,
            max_context_bytes: 200000,
            max_context_tokens: 131072,
            image_token_reserve: 16384,
            token_safety_margin: 4096,
            max_tool_result_bytes: 16000,
            max_review_rounds: 3,
            max_docx_bytes: 1000000,
        },
    };
    let result =
        postgres::execute_with_model(&pool, &request, &CancellationToken::new(), &script(&input))
            .await
            .unwrap();
    assert_eq!(result["analysis_quality"], "verified");
    let basis = docx_round::get_docx_round_basis(&pool, workspace, &actor)
        .await
        .unwrap()
        .unwrap();
    // Exercise grants through a real runtime role, not only a database owner.
    let options: sqlx::postgres::PgConnectOptions = url.parse().unwrap();
    let api_pool = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(runtime_pool_options(&options, "kb_runtime_api"))
        .await
        .unwrap();
    let prepared = composition::prepare(
        &api_pool,
        workspace,
        basis.clone(),
        None,
        &actor,
        composition_config.clone(),
    )
    .await
    .unwrap();
    assert_eq!(prepared.request.source_request, request);
    assert_eq!(
        serde_json::to_value(&prepared.input).unwrap(),
        serde_json::to_value(&input).unwrap()
    );
    assert_eq!(
        prepared.request.analysis_sha256,
        bidding::tender_analysis::digest(&prepared.analysis).unwrap()
    );
    // Owning both projects does not authorize mixing their source identities;
    // an empty initial requirement set is also not an independently reviewed analysis.
    let other_project = Uuid::new_v4();
    let owner = Uuid::parse_str(actor.strip_prefix("user:").unwrap()).unwrap();
    sqlx::query("SELECT kb_bid_v2_create_project($1,'composition empty project',$2,$3::kb_actor_identity,$4,$5,$6::kb_sha256)")
        .bind(other_project).bind(owner).bind(&actor).bind(Uuid::new_v4().to_string())
        .bind(other_project.as_bytes().to_vec()).bind(hex::encode(Sha256::digest(other_project.as_bytes()))).execute(&api_pool).await.unwrap();
    let other_workspace: Uuid =
        sqlx::query_scalar("SELECT id FROM bid_submission_workspaces WHERE project_id=$1")
            .bind(other_project)
            .fetch_one(&pool)
            .await
            .unwrap();
    let error = composition::prepare(
        &api_pool,
        other_workspace,
        basis.clone(),
        None,
        &actor,
        composition_config.clone(),
    )
    .await
    .err()
    .unwrap();
    assert_eq!(error.code, "FROZEN_INPUT_DIGEST_MISMATCH");
    let empty_basis = docx_round::get_docx_round_basis(&api_pool, other_workspace, &actor)
        .await
        .unwrap()
        .unwrap();
    let error = composition::prepare(
        &api_pool,
        other_workspace,
        empty_basis,
        None,
        &actor,
        composition_config.clone(),
    )
    .await
    .err()
    .unwrap();
    assert_eq!(error.code, "FROZEN_INPUT_MISSING");
    let request_sha = prepared.request.sha256().unwrap();
    let snapshot: composition::FrozenCompositionRequest =
        serde_json::from_value(serde_json::to_value(&prepared.request).unwrap()).unwrap();
    // The snapshot contains immutable identities and config, not another copy of
    // all source text/page images or the previous bid's body.
    let snapshot_json = serde_json::to_value(&snapshot).unwrap();
    assert!(snapshot_json.get("input").is_none() && snapshot_json.get("analysis").is_none());
    assert!(
        composition::prepare(
            &api_pool,
            workspace,
            basis.clone(),
            None,
            &format!("user:{}", Uuid::new_v4()),
            composition_config.clone()
        )
        .await
        .is_err()
    );
    let mut wrong_basis = basis.clone();
    wrong_basis.requirement_set_sha256 = "f".repeat(64);
    assert!(
        composition::prepare(
            &api_pool,
            workspace,
            wrong_basis,
            None,
            &actor,
            composition_config.clone()
        )
        .await
        .is_err()
    );
    let stale_version = docx_round::DocxVersionIdentity {
        version_id: Uuid::new_v4(),
        docx_sha256: "a".repeat(64),
    };
    let error = composition::prepare(
        &api_pool,
        workspace,
        basis.clone(),
        Some(stale_version),
        &actor,
        composition_config.clone(),
    )
    .await
    .err()
    .unwrap();
    assert_eq!(error.code, "WORKSPACE_CAS_CONFLICT");
    assert_eq!(
        error.disposition,
        bidding::agent_error::RetryDisposition::Deterministic
    );
    let mut changed = snapshot.clone();
    changed.config.limits.max_turns += 1;
    assert_eq!(
        composition::restore(&api_pool, changed, &request_sha)
            .await
            .err()
            .unwrap()
            .code,
        "FROZEN_INPUT_DIGEST_MISMATCH"
    );
    let mut changed = snapshot.clone();
    changed.contract_sha256 = "0".repeat(64);
    let changed_sha = changed.sha256().unwrap();
    assert_eq!(
        composition::restore(&api_pool, changed, &changed_sha)
            .await
            .err()
            .unwrap()
            .code,
        "FROZEN_INPUT_DIGEST_MISMATCH"
    );

    // Start a new source round using the real freeze operation. Restore must
    // still load the original decisions/input; new preparation must reject it.
    let docs: Vec<Uuid> = input
        .documents
        .iter()
        .map(|d| Uuid::parse_str(d["document_id"].as_str().unwrap()).unwrap())
        .collect();
    let bytes = b"composition source regression new round".to_vec();
    use sha2::{Digest, Sha256};
    let bytes_sha = hex::encode(Sha256::digest(&bytes));
    let new_round: Value = sqlx::query_scalar("SELECT kb_bid_v2_freeze_document_set($1,$2,$3,$4::kb_sha256,$5,$6::kb_actor_identity,$7,$8,$9::kb_sha256,$10)")
        .bind(project_id).bind(docs).bind(basis.document_set_id).bind(&basis.document_set_sha256)
        .bind(Uuid::new_v4()).bind(&actor).bind(Uuid::new_v4().to_string()).bind(bytes).bind(bytes_sha)
        .bind(serde_json::to_value(config()).unwrap()).fetch_one(&api_pool).await.unwrap();
    assert_ne!(new_round["artifact_id"], basis.document_set_id.to_string());
    let error = composition::prepare(
        &api_pool,
        workspace,
        basis,
        None,
        &actor,
        snapshot.config.clone(),
    )
    .await
    .err()
    .unwrap();
    assert_eq!(error.code, "WORKSPACE_CAS_CONFLICT");
    let restored = composition::restore(&api_pool, snapshot, &request_sha)
        .await
        .unwrap();
    assert_eq!(restored.request.sha256().unwrap(), request_sha);
    assert_eq!(
        serde_json::to_value(&restored.input).unwrap(),
        serde_json::to_value(&prepared.input).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&restored.analysis).unwrap(),
        serde_json::to_value(&prepared.analysis).unwrap()
    );
    assert!(
        docx_round::get_current_docx(&api_pool, workspace, &actor)
            .await
            .unwrap()
            .is_none(),
        "source preparation must not create a DOCX"
    );
    api_pool.close().await;
    pool.close().await;
}

struct CompositionScript {
    turns: Mutex<VecDeque<(&'static str, Value)>>,
    bodies: Mutex<Vec<Vec<u8>>>,
}
#[async_trait]
impl bidding::docx_composition::agent::Model for CompositionScript {
    async fn turn(
        &self,
        _: &bidding::docx_composition::agent::Config,
        body: &[u8],
    ) -> Result<ChatTurn, bidding::agent_error::AgentError> {
        self.bodies.lock().unwrap().push(body.to_vec());
        let request: Value = serde_json::from_slice(body).unwrap();
        for entry in request["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|m| m["role"] == "tool")
        {
            let result: Value = serde_json::from_str(entry["content"].as_str().unwrap()).unwrap();
            assert_eq!(
                result["ok"], true,
                "unexpected composition tool rejection: {result}"
            );
        }
        let (name, mut args) = self
            .turns
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected composition call");
        assert!(
            request["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| tool["function"]["name"] == name),
            "scripted composition tool must be advertised in the current role: {name}"
        );
        let context: Value =
            serde_json::from_str(request["messages"][1]["content"].as_str().unwrap()).unwrap();
        if matches!(
            name,
            "set_presentation" | "put_section" | "put_composition_plan_item"
        ) {
            args["expected_draft_sha256"] = context["draft_sha256"].clone();
        }
        if name == "inspect_rendered_cells" {
            let bookmark = request["messages"]
                .as_array()
                .unwrap()
                .iter()
                .rev()
                .filter(|v| v["role"] == "tool")
                .filter_map(|v| serde_json::from_str::<Value>(v["content"].as_str()?).ok())
                .find_map(|v| {
                    v["result"]["items"]
                        .as_array()?
                        .iter()
                        .find(|r| r["value"]["table"].is_object())
                        .map(|r| r["value"]["bookmark"].clone())
                })
                .expect("reviewer must receive rendered table before reading its cells");
            args["bookmark"] = bookmark;
        }
        Ok(ChatTurn {
            usage: None,
            content: String::new(),
            finish_reason: "tool_calls".into(),
            tool_calls: vec![ChatToolCall {
                id: Uuid::new_v4().to_string(),
                name: name.into(),
                arguments: args.to_string(),
            }],
        })
    }
}
fn composition_script(
    input: &FrozenInput,
    analysis: &bidding::tender_analysis::AnalysisResult,
) -> CompositionScript {
    let source = &input.source_units[0];
    let id = analysis.analysis.records.keys().next().unwrap();
    let section_id = Uuid::new_v4().to_string();
    let obligations: Vec<_> = bidding::docx_composition::obligation_inventory(analysis)
        .iter()
        .map(|reference| bidding::docx_composition::reference_key(reference).unwrap())
        .collect();
    let span =
        json!({"source_id":source.source_unit_revision_id,"start":0,"end":source.text.len()});
    let mut turns = vec![
        (
            "read_source",
            json!({"source_id":source.source_unit_revision_id,"start":0,"max_bytes":1000}),
        ),
        (
            "put_composition_plan_item",
            json!({"id":section_id,"kind":"section","parent":null,"order":0,
                "title":"响应及证明材料","prescribed":true,"grounds":[span],"obligation_refs":obligations}),
        ),
        (
            "set_composition_work",
            json!({"source_scope":[source.source_unit_revision_id],"section_scope":[section_id],
                "plan_item_id":section_id,"action":"compose","objective":"落实当前计划章节",
                "note":"已读取独立来源并保存章节计划","status":"active"}),
        ),
        (
            "set_presentation",
            json!({"title":"测试投标模板","toc_title":"目录","style":{"width_mm":210,"height_mm":297,
            "top_mm":20,"right_mm":20,"bottom_mm":20,"left_mm":20,"font_family":"Test Font","body_font_pt":10.5,"line_spacing":1.5},
            "grounds":[span],"explanation":"测试中明确配置页面，招标未指定排版，不伪称原文格式"}),
        ),
        (
            "put_section",
            json!({"id":section_id,"parent":null,"order":0,"title":"响应及证明材料","grounds":[span],"content":[
            {"kind":"response_table","needs":[{"record_id":id,"target":{"kind":"response","index":0}}],"columns":["项目","待填响应"],"blank_rows":1},
            {"kind":"placeholder","needs":[{"record_id":id,"target":{"kind":"response","index":1}}]}]}),
        ),
        (
            "collection_index",
            json!({"kind":"documents","offset":0,"limit":100}),
        ),
        (
            "read_source",
            json!({"source_id":source.source_unit_revision_id,"start":0,"max_bytes":1000}),
        ),
    ];
    for kind in ["all", "relation", "disposition"] {
        turns.push((
            "inspect_analysis",
            json!({"view":"detail","kind":kind,"offset":0,"limit":100}),
        ));
    }
    for name in [
        "inspect_composition",
        "inspect_rendered",
        "inspect_placements",
    ] {
        turns.push((name, json!({"offset":0,"limit":100})));
    }
    turns.push((
        "inspect_rendered_cells",
        json!({"bookmark":null,"offset":0,"limit":100}),
    ));
    turns.push((
        "put_composition_review",
        json!({"item_id":section_id,"conclusion":"pass","grounds":[span],"finding_ids":[]}),
    ));
    turns.push(("submit_composition_review", json!({"findings":[]})));
    CompositionScript {
        turns: Mutex::new(turns.into()),
        bodies: Mutex::new(vec![]),
    }
}

struct LostCompositionAck<'a> {
    journal: bidding::docx_composition::postgres::PgJournal<'a>,
    fail_turn: usize,
    fail_sequence: Option<usize>,
    reject_prepared: bool,
}
#[async_trait]
impl bidding::docx_composition::agent::Journal for LostCompositionAck<'_> {
    async fn load(
        &self,
    ) -> Result<
        Option<bidding::docx_composition::agent::Checkpoint>,
        bidding::agent_error::AgentError,
    > {
        self.journal.load().await
    }
    async fn reserve(
        &self,
        state: &bidding::docx_composition::agent::Checkpoint,
        body: &[u8],
    ) -> Result<usize, bidding::agent_error::AgentError> {
        let mut state = state.clone();
        if self.reject_prepared {
            state.tool_calls += 1;
        }
        let reservation = self.journal.reserve(&state, body).await?;
        if self.fail_sequence == Some(state.journal.sequence) {
            return Err(AgentError::new("INTERNAL", "lost prepared ACK"));
        }
        Ok(reservation)
    }
    async fn save(
        &self,
        state: &bidding::docx_composition::agent::Checkpoint,
    ) -> Result<(), bidding::agent_error::AgentError> {
        self.journal.save(state).await?;
        if (state.turn == self.fail_turn && state.journal.pending.is_none())
            || self.fail_sequence == Some(state.journal.sequence)
        {
            return Err(bidding::agent_error::AgentError::new(
                "INTERNAL",
                "injected committed checkpoint response loss",
            ));
        }
        Ok(())
    }
}
async fn composition_claim(
    pool: &PgPool,
    request: &BidAuthoringRequestIdentityV2,
) -> bidding::bid_authoring_v2::AgentRunLease {
    let value: Value =
        sqlx::query_scalar("SELECT kb_bid_v2_tender_agent_claim($1,$2,$3::kb_sha256)")
            .bind(request.request_artifact_id)
            .bind(request.request_revision)
            .bind(&request.frozen_input_sha256)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(value["disposition"], "claimed");
    bidding::bid_authoring_v2::AgentRunLease {
        attempt: value["attempt"].as_i64().unwrap() as i32,
        max_attempts: value["max_attempts"].as_i64().unwrap() as i32,
        execution_owner_token: Uuid::parse_str(value["execution_owner_token"].as_str().unwrap())
            .unwrap(),
    }
}
fn composition_identity(value: &Value) -> BidAuthoringRequestIdentityV2 {
    BidAuthoringRequestIdentityV2 {
        request_artifact_id: Uuid::parse_str(value["request_artifact_id"].as_str().unwrap())
            .unwrap(),
        request_revision: value["request_revision"].as_i64().unwrap(),
        frozen_input_sha256: value["frozen_input_sha256"].as_str().unwrap().into(),
    }
}

#[tokio::test]
#[ignore = "requires KB_TENDER_AGENT_TEST_DATABASE_URL pointing to a fresh owned test database"]
async fn composition_durable_journal_resumes_review_and_enforces_owner_and_call_budgets() {
    use bidding::docx_composition::{agent as compose, postgres as composition};
    use compose::Journal;
    let url =
        std::env::var("KB_TENDER_AGENT_TEST_DATABASE_URL").expect("dedicated test URL required");
    let pool = PgPool::connect(&url).await.unwrap();
    let db: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(db.starts_with("knowledgebrain_test_"));
    let (analysis_request, input) = seed(&pool).await;
    let result = postgres::execute_with_model(
        &pool,
        &analysis_request,
        &CancellationToken::new(),
        &script(&input),
    )
    .await
    .unwrap();
    assert_eq!(result["analysis_quality"], "verified");
    let (workspace,actor):(Uuid,String)=sqlx::query_as("SELECT w.id,'user:'||p.owner_user_id FROM bid_submission_workspaces w JOIN bid_projects p ON p.id=w.project_id WHERE p.id=$1")
        .bind(Uuid::parse_str(&input.project_id).unwrap()).fetch_one(&pool).await.unwrap();
    let options: sqlx::postgres::PgConnectOptions = url.parse().unwrap();
    let api_pool = PgPool::connect_with(runtime_pool_options(&options, "kb_runtime_api"))
        .await
        .unwrap();
    let worker_pool = PgPool::connect_with(runtime_pool_options(&options, "kb_runtime_worker"))
        .await
        .unwrap();
    let basis = bidding::docx_round::get_docx_round_basis(&api_pool, workspace, &actor)
        .await
        .unwrap()
        .unwrap();
    let config = compose::Config {
        provider: config().provider,
        limits: compose::Limits {
            max_no_progress_turns: 6,
            max_focus_turns: 24,
            max_focus_replans: 2,
            max_turns: 30,
            max_tool_calls: 40,
            max_physical_calls: 40,
            max_read_bytes: 1000000,
            max_context_bytes: 500000,
            max_context_tokens: 131072,
            image_token_reserve: 16384,
            token_safety_margin: 4096,
            max_tool_result_bytes: 100000,
            max_review_rounds: 3,
            max_docx_bytes: 1000000,
        },
    };
    let prepared = composition::prepare(&api_pool, workspace, basis, None, &actor, config)
        .await
        .unwrap();
    // Exercise SQL directly so malformed frozen budgets cannot bypass the Rust gate.
    for variant in 0..3 {
        let mut snapshot = json!(prepared.request);
        let mut contract = prepared.request.config.contract_definition();
        match variant {
            0 => {
                snapshot["config"]["limits"]
                    .as_object_mut()
                    .unwrap()
                    .remove("max_context_tokens");
            }
            1 => snapshot["config"]["limits"]["image_token_reserve"] = json!(0),
            _ => {
                snapshot["config"]["limits"]["max_context_tokens"] =
                    json!(prepared.request.config.provider.max_tokens)
            }
        }
        contract["config"] = snapshot["config"].clone();
        snapshot["contract_sha256"] = json!(bidding::tender_analysis::digest(&contract).unwrap());
        let error = sqlx::query_scalar::<_, Value>(
            "SELECT kb_bid_v2_create_docx_composition_request($1,$2,$3,$4::kb_actor_identity,$5)",
        )
        .bind(Uuid::new_v4())
        .bind(snapshot)
        .bind(contract)
        .bind(&actor)
        .bind(Uuid::new_v4().to_string())
        .fetch_one(&api_pool)
        .await
        .unwrap_err();
        assert!(error.to_string().contains("DOCX_COMPOSITION_INPUT_INVALID"));
    }
    let key = Uuid::new_v4().to_string();
    let receipt = composition::create_request(&api_pool, &prepared.request, &key)
        .await
        .unwrap();
    assert_eq!(
        receipt,
        composition::create_request(&api_pool, &prepared.request, &key)
            .await
            .unwrap()
    );
    let identity = composition_identity(&receipt);
    let payload: Value =
        sqlx::query_scalar("SELECT kb_bid_v2_load_authoring_job_payload($1,$2,$3::kb_sha256)")
            .bind(identity.request_artifact_id)
            .bind(identity.request_revision)
            .bind(&identity.frozen_input_sha256)
            .fetch_one(&api_pool)
            .await
            .unwrap();
    assert_eq!(payload["job_payload"]["job_kind"], "docx_compose");
    assert_eq!(
        payload["job_payload"]["workspace_id"],
        workspace.to_string()
    );
    let loaded = composition::load_request(&worker_pool, &identity)
        .await
        .unwrap();
    assert_eq!(
        loaded.request.sha256().unwrap(),
        prepared.request.sha256().unwrap()
    );
    let owner = composition_claim(&worker_pool, &identity).await;
    let live: Value =
        sqlx::query_scalar("SELECT kb_bid_v2_tender_agent_claim($1,$2,$3::kb_sha256)")
            .bind(identity.request_artifact_id)
            .bind(identity.request_revision)
            .bind(&identity.frozen_input_sha256)
            .fetch_one(&worker_pool)
            .await
            .unwrap();
    assert_eq!(live["disposition"], "live_owner");
    let journal = composition::PgJournal {
        pool: &worker_pool,
        request: &identity,
        owner: &owner,
    };
    let model = composition_script(&loaded.input, &loaded.analysis);
    let fail_turn = model
        .turns
        .lock()
        .unwrap()
        .iter()
        .position(|(name, _)| *name == "put_section")
        .unwrap()
        + 1;
    let failing = LostCompositionAck {
        journal,
        fail_turn,
        fail_sequence: None,
        reject_prepared: false,
    };
    let error = compose::run(
        &loaded.input,
        &loaded.analysis,
        &loaded.request.config,
        &failing,
        &model,
        &CancellationToken::new(),
    )
    .await
    .err()
    .unwrap();
    assert_eq!(error.code, "INTERNAL", "{error:?}");
    let saved = failing.load().await.unwrap().unwrap();
    assert_eq!(saved.turn, fail_turn);
    assert_eq!(saved.workspace.draft.sections.len(), 1);
    assert!(
        saved.workspace.reviewing,
        "a complete Main batch must durably enter independent review"
    );
    assert_eq!(
        saved.main_work.as_ref().unwrap().plan_item_id.as_ref(),
        saved.workspace.draft.plan.keys().next()
    );
    failing.journal.save(&saved).await.unwrap();
    let mut divergent = saved.clone();
    divergent.main_progress.watch.focus_turns += 1;
    assert_eq!(
        failing.journal.save(&divergent).await.err().unwrap().code,
        "FROZEN_INPUT_DIGEST_MISMATCH"
    );
    let analysis_checkpoint: Option<Value> =
        sqlx::query_scalar("SELECT kb_bid_v2_tender_agent_checkpoint_get($1,$2::kb_sha256)")
            .bind(identity.request_artifact_id)
            .bind(&identity.frozen_input_sha256)
            .fetch_one(&worker_pool)
            .await
            .unwrap();
    assert!(
        analysis_checkpoint.is_none(),
        "composition must not share extraction checkpoint stages"
    );
    sqlx::query("SELECT kb_bid_v2_tender_agent_yield_for_retry($1,$2::kb_sha256,$3,$4,'INTERNAL','checkpoint response lost')")
        .bind(identity.request_artifact_id).bind(&identity.frozen_input_sha256).bind(owner.attempt).bind(owner.execution_owner_token).execute(&worker_pool).await.unwrap();
    let next = composition_claim(&worker_pool, &identity).await;
    assert_eq!(next.attempt, owner.attempt + 1);
    assert_eq!(
        failing.journal.save(&saved).await.err().unwrap().code,
        "REQUEST_ATTEMPT_SUPERSEDED"
    );
    let first_body = model.bodies.lock().unwrap()[0].clone();
    assert_eq!(
        failing
            .journal
            .reserve(&saved, &first_body)
            .await
            .err()
            .unwrap()
            .code,
        "REQUEST_ATTEMPT_SUPERSEDED"
    );
    let resumed = composition::PgJournal {
        pool: &worker_pool,
        request: &identity,
        owner: &next,
    };
    let artifact = compose::run(
        &loaded.input,
        &loaded.analysis,
        &loaded.request.config,
        &resumed,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(artifact.manifest.status, "reviewed_template");
    let calls = model.bodies.lock().unwrap().len();
    let replay = compose::run(
        &loaded.input,
        &loaded.analysis,
        &loaded.request.config,
        &resumed,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(replay.docx_base64, artifact.docx_base64);
    assert_eq!(model.bodies.lock().unwrap().len(), calls);
    let persisted_calls: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM bid_tender_agent_call_attempts WHERE request_artifact_id=$1",
    )
    .bind(identity.request_artifact_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(persisted_calls as usize, calls);
    let completed = resumed.load().await.unwrap().unwrap();
    assert!(completed.workspace.done);
    assert!(resumed.reserve(&completed, &first_body).await.is_err());
    let status: String =
        sqlx::query_scalar("SELECT status FROM bid_async_request_snapshot_artifacts WHERE id=$1")
            .bind(identity.request_artifact_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        status, "pending",
        "a reviewed checkpoint alone must not claim DOCX publication"
    );
    // Reuse a production-created prepared SDK state, not a manually forged
    // Journal that omits the active conversation required by contract v3.
    let initial_payload: Vec<u8> = sqlx::query_scalar("SELECT canonical_payload FROM bid_tender_agent_checkpoint_artifacts WHERE request_artifact_id=$1 AND stage_kind='composition_checkpoint' AND batch_ordinal=1")
        .bind(identity.request_artifact_id).fetch_one(&pool).await.unwrap();
    let initial: compose::Checkpoint = serde_json::from_slice(&initial_payload).unwrap();
    let prepared_state = |config: &compose::Config, reviewing: bool| {
        let mut workspace =
            bidding::docx_composition::tools::Workspace::new(&loaded.input, &loaded.analysis)
                .unwrap();
        workspace.reviewing = reviewing;
        let mut journal = initial.journal.clone();
        journal.pending.as_mut().unwrap().role = if reviewing { "reviewer" } else { "main" }.into();
        compose::Checkpoint {
            pending_delivery: None,
            journal,
            contract_sha256: config.contract_sha256().unwrap(),
            workspace,
            turn: 0,
            tool_calls: 0,
            read_bytes: 0,
            transcript: vec![],
            main_work: None,
            review_work: None,
            main_progress: Default::default(),
            review_progress: Default::default(),
        }
    };
    // Global physical budget is reserved before the provider and survives attempts.
    let mut limited = prepared.request.clone();
    limited.config.limits.max_physical_calls = 1;
    limited.contract_sha256 = limited.config.contract_sha256().unwrap();
    let budget_receipt =
        composition::create_request(&api_pool, &limited, &Uuid::new_v4().to_string())
            .await
            .unwrap();
    let budget_id = composition_identity(&budget_receipt);
    let budget_owner = composition_claim(&worker_pool, &budget_id).await;
    let budget_journal = composition::PgJournal {
        pool: &worker_pool,
        request: &budget_id,
        owner: &budget_owner,
    };
    assert_eq!(
        budget_journal
            .reserve(&prepared_state(&limited.config, false), &first_body)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        budget_journal
            .reserve(&prepared_state(&limited.config, false), &first_body)
            .await
            .err()
            .unwrap()
            .code,
        "AGENT_TURN_BUDGET_EXCEEDED"
    );
    sqlx::query("SELECT kb_bid_v2_tender_agent_yield_for_retry($1,$2::kb_sha256,$3,$4,'INTERNAL','provider response lost')")
        .bind(budget_id.request_artifact_id).bind(&budget_id.frozen_input_sha256).bind(budget_owner.attempt)
        .bind(budget_owner.execution_owner_token).execute(&worker_pool).await.unwrap();
    let budget_next = composition_claim(&worker_pool, &budget_id).await;
    let budget_retry = composition::PgJournal {
        pool: &worker_pool,
        request: &budget_id,
        owner: &budget_next,
    };
    assert_eq!(
        budget_retry
            .reserve(&prepared_state(&limited.config, false), &first_body)
            .await
            .err()
            .unwrap()
            .code,
        "AGENT_TURN_BUDGET_EXCEEDED"
    );
    // Each unsaved model boundary has one shared three-call retry allowance.
    let boundary_receipt =
        composition::create_request(&api_pool, &prepared.request, &Uuid::new_v4().to_string())
            .await
            .unwrap();
    let boundary_id = composition_identity(&boundary_receipt);
    let boundary_owner = composition_claim(&worker_pool, &boundary_id).await;
    let boundary = composition::PgJournal {
        pool: &worker_pool,
        request: &boundary_id,
        owner: &boundary_owner,
    };
    assert!(
        boundary
            .reserve(&prepared_state(&prepared.request.config, true), &first_body)
            .await
            .is_err()
    );
    let captured: Value = serde_json::from_slice(&first_body).unwrap();
    assert_eq!(captured["max_tokens"], 4096);
    assert_eq!(captured["reasoning_effort"], "medium");
    for effort in [Some("high"), None] {
        let mut drift = captured.clone();
        drift.as_object_mut().unwrap().remove("reasoning_effort");
        if let Some(effort) = effort {
            drift["reasoning_effort"] = json!(effort);
        }
        assert_eq!(
            boundary
                .reserve(
                    &prepared_state(&prepared.request.config, false),
                    &serde_json_canonicalizer::to_vec(&drift).unwrap()
                )
                .await
                .unwrap_err()
                .code,
            "FROZEN_INPUT_DIGEST_MISMATCH"
        );
    }
    assert_eq!(
        boundary
            .reserve(
                &prepared_state(&prepared.request.config, false),
                &first_body
            )
            .await
            .unwrap(),
        1
    );
    let mut bad: Value = serde_json::from_slice(&first_body).unwrap();
    bad["messages"][1]["content"] = json!("changed replay");
    assert_eq!(
        boundary
            .reserve(
                &prepared_state(&prepared.request.config, false),
                &serde_json_canonicalizer::to_vec(&bad).unwrap()
            )
            .await
            .err()
            .unwrap()
            .code,
        "FROZEN_INPUT_DIGEST_MISMATCH"
    );
    for count in 2..=3 {
        assert_eq!(
            boundary
                .reserve(
                    &prepared_state(&prepared.request.config, false),
                    &first_body
                )
                .await
                .unwrap(),
            count
        );
    }
    assert_eq!(
        boundary
            .reserve(
                &prepared_state(&prepared.request.config, false),
                &first_body
            )
            .await
            .err()
            .unwrap()
            .code,
        "AGENT_PROVIDER_UNAVAILABLE"
    );
    for boundary in [0, 1, 2] {
        let receipt =
            composition::create_request(&api_pool, &prepared.request, &Uuid::new_v4().to_string())
                .await
                .unwrap();
        let request = composition_identity(&receipt);
        let owner = composition_claim(&worker_pool, &request).await;
        let journal = LostCompositionAck {
            journal: composition::PgJournal {
                pool: &worker_pool,
                request: &request,
                owner: &owner,
            },
            fail_turn: usize::MAX,
            fail_sequence: Some(boundary),
            reject_prepared: boundary == 0,
        };
        let model = composition_script(&loaded.input, &loaded.analysis);
        let expected_calls = model.turns.lock().unwrap().len();
        let error = compose::run(
            &loaded.input,
            &loaded.analysis,
            &loaded.request.config,
            &journal,
            &model,
            &CancellationToken::new(),
        )
        .await
        .err()
        .unwrap();
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM bid_tender_agent_call_attempts WHERE request_artifact_id=$1",
        )
        .bind(request.request_artifact_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        if boundary == 0 {
            assert_eq!(error.code, "FROZEN_INPUT_DIGEST_MISMATCH");
            assert_eq!(
                count, 0,
                "prepared checkpoint failure rolls back physical reservation"
            );
            assert!(journal.load().await.unwrap().is_none());
            assert!(model.bodies.lock().unwrap().is_empty());
            continue;
        }
        assert_eq!(error.code, "INTERNAL", "{error:?}");
        assert_eq!(count, 1);
        let saved = journal.load().await.unwrap().unwrap();
        assert_eq!(saved.turn, 0);
        assert_eq!(saved.journal.sequence, boundary);
        assert_eq!(saved.journal.response().is_some(), boundary == 2);
        assert_eq!(saved.tool_calls, 0);
        journal.journal.save(&saved).await.unwrap();
        let mut forged = saved.clone();
        forged.journal.sequence += 1;
        forged.turn += 1;
        assert!(journal.journal.save(&forged).await.is_err());
        if boundary == 2 {
            assert!(
                journal
                    .journal
                    .reserve(&saved, saved.journal.body().unwrap())
                    .await
                    .is_err()
            );
        }
        let artifact = compose::run(
            &loaded.input,
            &loaded.analysis,
            &loaded.request.config,
            &journal.journal,
            &model,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(artifact.manifest.status, "reviewed_template");
        assert_eq!(model.bodies.lock().unwrap().len(), expected_calls);
        let completed = journal.load().await.unwrap().unwrap();
        assert!(completed.journal.pending.is_none());
        assert!(completed.journal.sequence > completed.turn);
    }
    api_pool.close().await;
    worker_pool.close().await;
    pool.close().await;
}

#[tokio::test]
#[ignore = "requires a dedicated disposable PostgreSQL and isolated OBJECT_DIR"]
async fn composition_publication_is_atomic_and_replay_verifies_both_files() {
    use bidding::docx_composition::{agent as compose, postgres as composition};
    use bidding::docx_round;
    use compose::Journal;
    let object_dir = std::path::PathBuf::from(
        std::env::var("OBJECT_DIR").expect("isolated OBJECT_DIR required"),
    );
    assert!(object_dir.is_absolute() && object_dir.starts_with(std::env::temp_dir()));
    assert!(
        std::env::var("KNOWLEDGEBRAIN_S3_BUCKET")
            .unwrap_or_default()
            .is_empty()
    );
    let url =
        std::env::var("KB_TENDER_AGENT_TEST_DATABASE_URL").expect("dedicated test URL required");
    let pool = PgPool::connect(&url).await.unwrap();
    let db: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(db.starts_with("knowledgebrain_test_"));
    let (analysis_request, input) = seed(&pool).await;
    let result = postgres::execute_with_model(
        &pool,
        &analysis_request,
        &CancellationToken::new(),
        &script(&input),
    )
    .await
    .unwrap();
    assert_eq!(result["analysis_quality"], "verified");
    let (workspace,actor):(Uuid,String)=sqlx::query_as("SELECT w.id,'user:'||p.owner_user_id FROM bid_submission_workspaces w JOIN bid_projects p ON p.id=w.project_id WHERE p.id=$1")
        .bind(Uuid::parse_str(&input.project_id).unwrap()).fetch_one(&pool).await.unwrap();
    let options: sqlx::postgres::PgConnectOptions = url.parse().unwrap();
    let api_pool = PgPool::connect_with(runtime_pool_options(&options, "kb_runtime_api"))
        .await
        .unwrap();
    let worker_pool = PgPool::connect_with(runtime_pool_options(&options, "kb_runtime_worker"))
        .await
        .unwrap();
    let basis = bidding::docx_round::get_docx_round_basis(&api_pool, workspace, &actor)
        .await
        .unwrap()
        .unwrap();
    let config = compose::Config {
        provider: config().provider,
        limits: compose::Limits {
            max_no_progress_turns: 6,
            max_focus_turns: 24,
            max_focus_replans: 2,
            max_turns: 30,
            max_tool_calls: 40,
            max_physical_calls: 40,
            max_read_bytes: 1000000,
            max_context_bytes: 500000,
            max_context_tokens: 131072,
            image_token_reserve: 16384,
            token_safety_margin: 4096,
            max_tool_result_bytes: 100000,
            max_review_rounds: 3,
            max_docx_bytes: 1000000,
        },
    };
    let prepared = composition::prepare(&api_pool, workspace, basis, None, &actor, config)
        .await
        .unwrap();

    let receipt =
        composition::create_request(&api_pool, &prepared.request, &Uuid::new_v4().to_string())
            .await
            .unwrap();
    let identity = composition_identity(&receipt);
    let owner = composition_claim(&worker_pool, &identity).await;
    assert!(
        composition::prepare_publication(&worker_pool, &identity)
            .await
            .is_err()
    );
    assert!(
        composition::replay_publication(&worker_pool, &identity)
            .await
            .unwrap()
            .is_none()
    );
    let journal = composition::PgJournal {
        pool: &worker_pool,
        request: &identity,
        owner: &owner,
    };
    let model = composition_script(&prepared.input, &prepared.analysis);
    compose::run(
        &prepared.input,
        &prepared.analysis,
        &prepared.request.config,
        &journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    let checkpoint = journal.load().await.unwrap().unwrap();
    global_review_publication::reject_invalid_composition_plan_publication(
        &pool,
        &identity,
        &owner,
        &checkpoint,
    )
    .await;
    for variant in 0..4 {
        let mut changed = checkpoint.clone();
        match variant {
            0 => changed.workspace.done = false,
            1 => changed.workspace.reviewing = true,
            2 => changed
                .workspace
                .artifact
                .as_mut()
                .unwrap()
                .manifest
                .sections
                .clear(),
            _ => changed.contract_sha256 = "0".repeat(64),
        }
        assert!(
            compose::reviewed_artifact(
                &prepared.input,
                &prepared.analysis,
                &prepared.request.config,
                &changed
            )
            .is_err()
        );
    }
    let publication = composition::prepare_publication(&worker_pool, &identity)
        .await
        .unwrap();
    let docx_stage = Uuid::new_v4();
    let manifest_stage = Uuid::new_v4();
    let docx_media = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
    platform::stage_object_upload(
        &worker_pool,
        docx_stage,
        &publication.docx.object_ref(),
        publication.docx.sha256(),
        docx_media,
        publication.docx.bytes().len() as i64,
        &actor,
    )
    .await
    .unwrap();
    platform::stage_object_upload(
        &worker_pool,
        manifest_stage,
        &platform::object_ref(&publication.manifest_sha256),
        &publication.manifest_sha256,
        "application/json",
        publication.manifest.len() as i64,
        &actor,
    )
    .await
    .unwrap();
    // Physical files are required; staged metadata alone is insufficient.
    assert!(
        composition::publish_staged(&worker_pool, &identity, &owner, docx_stage, manifest_stage)
            .await
            .is_err()
    );
    platform::write_blob_async(publication.docx.sha256(), publication.docx.bytes())
        .await
        .unwrap();
    platform::write_blob_async(&publication.manifest_sha256, &publication.manifest)
        .await
        .unwrap();
    let mut wrong_owner = owner.clone();
    wrong_owner.execution_owner_token = Uuid::new_v4();
    assert_eq!(
        composition::publish_staged(
            &worker_pool,
            &identity,
            &wrong_owner,
            docx_stage,
            manifest_stage
        )
        .await
        .err()
        .unwrap()
        .code,
        "REQUEST_ATTEMPT_SUPERSEDED"
    );
    // Second commit fails AFTER the first object and round were inserted.
    assert!(
        composition::publish_staged(&worker_pool, &identity, &owner, docx_stage, Uuid::new_v4())
            .await
            .is_err()
    );
    assert!(
        docx_round::get_current_docx(&api_pool, workspace, &actor)
            .await
            .unwrap()
            .is_none()
    );
    let rollback: (i64,i64,i64,String) = sqlx::query_as("SELECT (SELECT count(*) FROM bid_docx_round_artifacts WHERE workspace_id=$1),(SELECT count(*) FROM object_upload_staging WHERE id IN ($2,$3)),(SELECT count(*) FROM bid_async_stage_receipts WHERE request_artifact_id=$4 AND stage_kind='object_commit'),(SELECT status FROM bid_async_request_snapshot_artifacts WHERE id=$4)")
        .bind(workspace).bind(docx_stage).bind(manifest_stage).bind(identity.request_artifact_id).fetch_one(&pool).await.unwrap();
    assert_eq!(rollback, (0, 2, 0, "pending".into()));
    let published =
        composition::publish_staged(&worker_pool, &identity, &owner, docx_stage, manifest_stage)
            .await
            .unwrap();
    let version = Uuid::parse_str(published["version_id"].as_str().unwrap()).unwrap();
    assert_eq!(published["status"], "succeeded");
    assert_eq!(published["manifest"]["sha256"], publication.manifest_sha256);
    let owners: Vec<String> = sqlx::query_scalar("SELECT occurrence FROM object_owner_references WHERE owner_kind='bid_docx_version' AND owner_id=$1 ORDER BY occurrence")
        .bind(version).fetch_all(&pool).await.unwrap();
    assert_eq!(owners, vec!["composition_manifest", "document"]);
    let terminal: (String,String,i64) = sqlx::query_as("SELECT r.status,a.status,(SELECT count(*) FROM object_upload_staging WHERE id IN ($2,$3)) FROM bid_async_request_snapshot_artifacts r JOIN bid_tender_agent_run_artifacts a ON a.request_artifact_id=r.id AND a.attempt=r.current_attempt WHERE r.id=$1")
        .bind(identity.request_artifact_id).bind(docx_stage).bind(manifest_stage).fetch_one(&pool).await.unwrap();
    assert_eq!(terminal, ("succeeded".into(), "succeeded".into(), 0));
    let manifest_identity =
        composition::get_manifest_identity(&api_pool, workspace, version, &actor)
            .await
            .unwrap()
            .unwrap();
    assert_eq!(manifest_identity["sha256"], publication.manifest_sha256);
    assert!(
        composition::get_manifest_identity(
            &api_pool,
            workspace,
            version,
            &format!("user:{}", Uuid::new_v4())
        )
        .await
        .is_err()
    );
    assert!(
        composition::get_manifest_identity(&api_pool, Uuid::new_v4(), version, &actor)
            .await
            .is_err()
    );
    let before = std::fs::metadata(
        platform::blob_path(publication.docx.sha256()).expect("valid configured test object path"),
    )
    .unwrap()
    .modified()
    .unwrap();
    // Simulate lost SQL ACK: replay with no active owner, no stages or writes.
    assert_eq!(
        composition::replay_publication(&worker_pool, &identity)
            .await
            .unwrap()
            .unwrap(),
        published
    );
    assert_eq!(
        std::fs::metadata(
            platform::blob_path(publication.docx.sha256())
                .expect("valid configured test object path")
        )
        .unwrap()
        .modified()
        .unwrap(),
        before
    );
    for (sha, bytes) in [
        (publication.docx.sha256(), publication.docx.bytes()),
        (
            publication.manifest_sha256.as_str(),
            publication.manifest.as_slice(),
        ),
    ] {
        std::fs::write(
            platform::blob_path(sha).expect("valid configured test object path"),
            b"corrupt",
        )
        .unwrap();
        assert_eq!(
            composition::replay_publication(&worker_pool, &identity)
                .await
                .err()
                .unwrap()
                .code,
            "AGENT_OUTPUT_INVALID"
        );
        assert_eq!(
            std::fs::read(platform::blob_path(sha).expect("valid configured test object path"))
                .unwrap(),
            b"corrupt"
        );
        std::fs::remove_file(platform::blob_path(sha).expect("valid configured test object path"))
            .unwrap();
        assert_eq!(
            composition::replay_publication(&worker_pool, &identity)
                .await
                .err()
                .unwrap()
                .code,
            "INTERNAL"
        );
        platform::write_blob_async(sha, bytes).await.unwrap();
    }
    // Freeze a second composition, then save a user's edited version while it runs.
    let expected = docx_round::DocxVersionIdentity {
        version_id: version,
        docx_sha256: publication.docx.sha256().into(),
    };
    let next_prepared = composition::prepare(
        &api_pool,
        workspace,
        prepared.request.basis.clone(),
        Some(expected),
        &actor,
        prepared.request.config.clone(),
    )
    .await
    .unwrap();
    let next_receipt = composition::create_request(
        &api_pool,
        &next_prepared.request,
        &Uuid::new_v4().to_string(),
    )
    .await
    .unwrap();
    let next_id = composition_identity(&next_receipt);
    let next_owner = composition_claim(&worker_pool, &next_id).await;
    let next_journal = composition::PgJournal {
        pool: &worker_pool,
        request: &next_id,
        owner: &next_owner,
    };
    compose::run(
        &next_prepared.input,
        &next_prepared.analysis,
        &next_prepared.request.config,
        &next_journal,
        &composition_script(&next_prepared.input, &next_prepared.analysis),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    let editor = docx_round::editor_command(
        &api_pool,
        workspace,
        "open",
        &json!({"expected_version_id":version,"expected_docx_sha256":publication.docx.sha256()}),
        &actor,
        &Uuid::new_v4().to_string(),
    )
    .await
    .unwrap();
    let mut buffer = std::io::Cursor::new(Vec::new());
    docx_rs::Docx::new()
        .add_paragraph(
            docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text("用户保留的修改")),
        )
        .build()
        .pack(&mut buffer)
        .unwrap();
    let edited = docx_round::InitialDocx::new(buffer.into_inner()).unwrap();
    let edit_stage = Uuid::new_v4();
    platform::stage_object_upload(
        &api_pool,
        edit_stage,
        &edited.object_ref(),
        edited.sha256(),
        docx_media,
        edited.bytes().len() as i64,
        &actor,
    )
    .await
    .unwrap();
    platform::write_blob_async(edited.sha256(), edited.bytes())
        .await
        .unwrap();
    let saved = docx_round::editor_command(&api_pool,workspace,"save",&json!({"editor_key":editor["editor_key"],"save_id":null,"final":true,"callback_sha256":edited.sha256(),"docx_sha256":edited.sha256(),"byte_length":edited.bytes().len(),"staging_id":edit_stage}),&actor,&Uuid::new_v4().to_string()).await.unwrap();
    let saved_version = Uuid::parse_str(saved["version_id"].as_str().unwrap()).unwrap();
    assert!(
        composition::get_manifest_identity(&api_pool, workspace, saved_version, &actor)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        composition::get_manifest_identity(&api_pool, workspace, version, &actor)
            .await
            .unwrap()
            .unwrap(),
        manifest_identity
    );
    let next_publication = composition::prepare_publication(&worker_pool, &next_id)
        .await
        .unwrap();
    platform::write_blob_async(
        next_publication.docx.sha256(),
        next_publication.docx.bytes(),
    )
    .await
    .unwrap();
    platform::write_blob_async(
        &next_publication.manifest_sha256,
        &next_publication.manifest,
    )
    .await
    .unwrap();
    let next_docx_stage = Uuid::new_v4();
    let next_manifest_stage = Uuid::new_v4();
    platform::stage_object_upload(
        &worker_pool,
        next_docx_stage,
        &next_publication.docx.object_ref(),
        next_publication.docx.sha256(),
        docx_media,
        next_publication.docx.bytes().len() as i64,
        &actor,
    )
    .await
    .unwrap();
    platform::stage_object_upload(
        &worker_pool,
        next_manifest_stage,
        &platform::object_ref(&next_publication.manifest_sha256),
        &next_publication.manifest_sha256,
        "application/json",
        next_publication.manifest.len() as i64,
        &actor,
    )
    .await
    .unwrap();
    assert_eq!(
        composition::publish_staged(
            &worker_pool,
            &next_id,
            &next_owner,
            next_docx_stage,
            next_manifest_stage
        )
        .await
        .err()
        .unwrap()
        .code,
        "WORKSPACE_CAS_CONFLICT"
    );
    assert_eq!(
        docx_round::get_current_docx(&api_pool, workspace, &actor)
            .await
            .unwrap()
            .unwrap()["version_id"],
        saved["version_id"]
    );
    assert_eq!(
        platform::read_blob(edited.sha256()).unwrap(),
        edited.bytes()
    );
    assert_eq!(
        composition::replay_publication(&worker_pool, &identity)
            .await
            .unwrap()
            .unwrap(),
        published
    );
    assert!(
        composition::replay_publication(&worker_pool, &next_id)
            .await
            .unwrap()
            .is_none()
    );
    // A later source collection also prevents publication, independently of DOCX CAS.
    let source_prepared = composition::prepare(
        &api_pool,
        workspace,
        prepared.request.basis.clone(),
        Some(docx_round::DocxVersionIdentity {
            version_id: saved_version,
            docx_sha256: edited.sha256().into(),
        }),
        &actor,
        prepared.request.config.clone(),
    )
    .await
    .unwrap();
    let source_receipt = composition::create_request(
        &api_pool,
        &source_prepared.request,
        &Uuid::new_v4().to_string(),
    )
    .await
    .unwrap();
    let source_id = composition_identity(&source_receipt);
    let source_owner = composition_claim(&worker_pool, &source_id).await;
    let source_journal = composition::PgJournal {
        pool: &worker_pool,
        request: &source_id,
        owner: &source_owner,
    };
    compose::run(
        &source_prepared.input,
        &source_prepared.analysis,
        &source_prepared.request.config,
        &source_journal,
        &composition_script(&source_prepared.input, &source_prepared.analysis),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    let source_publication = composition::prepare_publication(&worker_pool, &source_id)
        .await
        .unwrap();
    let source_docx_stage = Uuid::new_v4();
    let source_manifest_stage = Uuid::new_v4();
    platform::stage_object_upload(
        &worker_pool,
        source_docx_stage,
        &source_publication.docx.object_ref(),
        source_publication.docx.sha256(),
        docx_media,
        source_publication.docx.bytes().len() as i64,
        &actor,
    )
    .await
    .unwrap();
    platform::stage_object_upload(
        &worker_pool,
        source_manifest_stage,
        &platform::object_ref(&source_publication.manifest_sha256),
        &source_publication.manifest_sha256,
        "application/json",
        source_publication.manifest.len() as i64,
        &actor,
    )
    .await
    .unwrap();
    platform::write_blob_async(
        source_publication.docx.sha256(),
        source_publication.docx.bytes(),
    )
    .await
    .unwrap();
    platform::write_blob_async(
        &source_publication.manifest_sha256,
        &source_publication.manifest,
    )
    .await
    .unwrap();
    let docs: Vec<Uuid> = input
        .documents
        .iter()
        .map(|d| Uuid::parse_str(d["document_id"].as_str().unwrap()).unwrap())
        .collect();
    let freeze_bytes = b"source changed during composition publication".to_vec();
    use sha2::{Digest, Sha256};
    let freeze_sha = hex::encode(Sha256::digest(&freeze_bytes));
    let _:Value=sqlx::query_scalar("SELECT kb_bid_v2_freeze_document_set($1,$2,$3,$4::kb_sha256,$5,$6::kb_actor_identity,$7,$8,$9::kb_sha256,$10)")
        .bind(Uuid::parse_str(&input.project_id).unwrap()).bind(docs).bind(prepared.request.basis.document_set_id).bind(&prepared.request.basis.document_set_sha256)
        .bind(Uuid::new_v4()).bind(&actor).bind(Uuid::new_v4().to_string()).bind(freeze_bytes).bind(freeze_sha)
        .bind(serde_json::to_value(self::config()).unwrap()).fetch_one(&api_pool).await.unwrap();
    assert_eq!(
        composition::publish_staged(
            &worker_pool,
            &source_id,
            &source_owner,
            source_docx_stage,
            source_manifest_stage
        )
        .await
        .err()
        .unwrap()
        .code,
        "WORKSPACE_CAS_CONFLICT"
    );
    assert_eq!(
        docx_round::get_current_docx(&api_pool, workspace, &actor)
            .await
            .unwrap()
            .unwrap()["version_id"],
        saved["version_id"]
    );
    assert_eq!(
        composition::replay_publication(&worker_pool, &identity)
            .await
            .unwrap()
            .unwrap(),
        published
    );
    let pending_stages: i64 =
        sqlx::query_scalar("SELECT count(*) FROM object_upload_staging WHERE id=ANY($1)")
            .bind(vec![
                next_docx_stage,
                next_manifest_stage,
                source_docx_stage,
                source_manifest_stage,
            ])
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        pending_stages, 4,
        "read-only replay must not consume unrelated staging"
    );
    let rounds: i64 =
        sqlx::query_scalar("SELECT count(*) FROM bid_docx_round_artifacts WHERE workspace_id=$1")
            .bind(workspace)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(rounds, 1);
    api_pool.close().await;
    worker_pool.close().await;
    pool.close().await;
}

struct CompositionTestObjects {
    writes: std::sync::atomic::AtomicUsize,
    fail_write: usize,
}
#[async_trait]
impl bidding::docx_composition::runtime::ObjectIo for CompositionTestObjects {
    async fn read(
        &self,
        sha: &str,
        _: usize,
        _: &CancellationToken,
    ) -> Result<Vec<u8>, bidding::agent_error::AgentError> {
        std::fs::read(platform::blob_path(sha).expect("valid configured test object path"))
            .map_err(|e| bidding::agent_error::AgentError::new("INTERNAL", e.to_string()))
    }
    async fn write(
        &self,
        sha: &str,
        bytes: &[u8],
        _: &CancellationToken,
    ) -> Result<(), bidding::agent_error::AgentError> {
        let count = self
            .writes
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        if count == self.fail_write {
            return Err(bidding::agent_error::AgentError::new(
                "INTERNAL",
                "injected object write failure",
            ));
        }
        std::fs::write(
            platform::blob_path(sha).expect("valid configured test object path"),
            bytes,
        )
        .map_err(|e| bidding::agent_error::AgentError::new("INTERNAL", e.to_string()))
    }
}
struct WaitingCompositionModel {
    entered: tokio::sync::Notify,
}
#[async_trait]
impl bidding::docx_composition::agent::Model for WaitingCompositionModel {
    async fn turn(
        &self,
        _: &bidding::docx_composition::agent::Config,
        _: &[u8],
    ) -> Result<ChatTurn, bidding::agent_error::AgentError> {
        self.entered.notify_one();
        std::future::pending().await
    }
}
struct InvalidCompositionModel;
#[async_trait]
impl bidding::docx_composition::agent::Model for InvalidCompositionModel {
    async fn turn(
        &self,
        _: &bidding::docx_composition::agent::Config,
        _: &[u8],
    ) -> Result<ChatTurn, bidding::agent_error::AgentError> {
        Err(bidding::agent_error::AgentError::new(
            "AGENT_OUTPUT_INVALID",
            "synthetic invalid composition",
        ))
    }
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL, Redis and isolated OBJECT_DIR"]
async fn composition_runtime_dispatch_resume_cleanup_heartbeat_and_failure() {
    use bidding::docx_composition::{agent as compose, postgres as composition, runtime};
    use std::sync::atomic::Ordering;
    let object_dir = std::path::PathBuf::from(
        std::env::var("OBJECT_DIR").expect("isolated OBJECT_DIR required"),
    );
    assert!(object_dir.is_absolute() && object_dir.starts_with(std::env::temp_dir()));
    assert!(
        std::env::var("KNOWLEDGEBRAIN_S3_BUCKET")
            .unwrap_or_default()
            .is_empty()
    );
    let url =
        std::env::var("KB_TENDER_AGENT_TEST_DATABASE_URL").expect("dedicated test URL required");
    let pool = PgPool::connect(&url).await.unwrap();
    let db: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(db.starts_with("knowledgebrain_test_"));
    let (analysis_request, input) = seed(&pool).await;
    let result = postgres::execute_with_model(
        &pool,
        &analysis_request,
        &CancellationToken::new(),
        &script(&input),
    )
    .await
    .unwrap();
    assert_eq!(result["analysis_quality"], "verified");
    let (workspace,actor):(Uuid,String)=sqlx::query_as("SELECT w.id,'user:'||p.owner_user_id FROM bid_submission_workspaces w JOIN bid_projects p ON p.id=w.project_id WHERE p.id=$1")
        .bind(Uuid::parse_str(&input.project_id).unwrap()).fetch_one(&pool).await.unwrap();
    let options: sqlx::postgres::PgConnectOptions = url.parse().unwrap();
    let api_pool = PgPool::connect_with(runtime_pool_options(&options, "kb_runtime_api"))
        .await
        .unwrap();
    let worker_pool = PgPool::connect_with(runtime_pool_options(&options, "kb_runtime_worker"))
        .await
        .unwrap();
    let basis = bidding::docx_round::get_docx_round_basis(&api_pool, workspace, &actor)
        .await
        .unwrap()
        .unwrap();
    let config = compose::Config {
        provider: config().provider,
        limits: compose::Limits {
            max_no_progress_turns: 6,
            max_focus_turns: 24,
            max_focus_replans: 2,
            max_turns: 30,
            max_tool_calls: 40,
            max_physical_calls: 40,
            max_read_bytes: 1000000,
            max_context_bytes: 500000,
            max_context_tokens: 131072,
            image_token_reserve: 16384,
            token_safety_margin: 4096,
            max_tool_result_bytes: 100000,
            max_review_rounds: 3,
            max_docx_bytes: 1000000,
        },
    };
    let prepared = composition::prepare(&api_pool, workspace, basis, None, &actor, config)
        .await
        .unwrap();

    assert!(
        std::env::var("REDIS_URL")
            .unwrap()
            .starts_with("redis://127.0.0.1:")
    );
    let storage = platform::oxana_connect().unwrap();
    let mut jobs = Vec::new();
    for _ in 0..3 {
        let receipt =
            composition::create_request(&api_pool, &prepared.request, &Uuid::new_v4().to_string())
                .await
                .unwrap();
        let identity = composition_identity(&receipt);
        jobs.push(platform::DocxComposeJobV2 {
            request: identity,
            project_id: Uuid::parse_str(&input.project_id).unwrap(),
            workspace_id: workspace,
        });
    }
    let job = &jobs[0];
    let payload = platform::BidAuthoringJobPayloadV2::DocxCompose {
        request: job.request.clone(),
        project_id: job.project_id,
        workspace_id: workspace,
    };
    let before = storage
        .enqueued_count(platform::BidAuthoringV2Queue)
        .await
        .unwrap();
    let envelope_id = platform::enqueue_bid_authoring_v2(payload.clone())
        .await
        .unwrap()
        .unwrap();
    platform::enqueue_bid_authoring_v2(payload).await.unwrap();
    assert_eq!(
        storage
            .enqueued_count(platform::BidAuthoringV2Queue)
            .await
            .unwrap(),
        before + 1
    );
    let envelope = storage.get_job(&envelope_id).await.unwrap().unwrap();
    let delivered: platform::DocxComposeJobV2 = serde_json::from_value(envelope.job.args).unwrap();
    assert_eq!(&delivered, job);
    let io = CompositionTestObjects {
        writes: 0.into(),
        fail_write: 2,
    };
    let cleanup = platform::StagedObjectCleanupTracker::new(&worker_pool, &actor);
    let model = composition_script(&prepared.input, &prepared.analysis);
    let cancel = CancellationToken::new();
    let mut forged = job.clone();
    forged.workspace_id = Uuid::new_v4();
    assert_eq!(
        runtime::execute_with_model(&worker_pool, &forged, &cancel, &io, &cleanup, &model)
            .await
            .err()
            .unwrap()
            .code,
        "FROZEN_INPUT_DIGEST_MISMATCH"
    );
    let attempts: i32 = sqlx::query_scalar(
        "SELECT current_attempt FROM bid_async_request_snapshot_artifacts WHERE id=$1",
    )
    .bind(job.request.request_artifact_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(attempts, 0);
    let owner = composition_claim(&worker_pool, &job.request).await;
    assert_eq!(
        runtime::execute_with_model(&worker_pool, job, &cancel, &io, &cleanup, &model)
            .await
            .unwrap()["disposition"],
        "live_owner"
    );
    assert!(model.bodies.lock().unwrap().is_empty());
    sqlx::query("SELECT kb_bid_v2_tender_agent_yield_for_retry($1,$2::kb_sha256,$3,$4,'INTERNAL','release live-owner fixture')")
        .bind(job.request.request_artifact_id).bind(&job.request.frozen_input_sha256).bind(owner.attempt).bind(owner.execution_owner_token).execute(&worker_pool).await.unwrap();
    assert_eq!(
        runtime::execute_with_model(&worker_pool, &delivered, &cancel, &io, &cleanup, &model)
            .await
            .err()
            .unwrap()
            .code,
        "INTERNAL"
    );
    assert_eq!(cleanup.pending_count(), 2);
    let state:(String,String)=sqlx::query_as("SELECT r.status,a.status FROM bid_async_request_snapshot_artifacts r JOIN bid_tender_agent_run_artifacts a ON a.request_artifact_id=r.id AND a.attempt=r.current_attempt WHERE r.id=$1").bind(job.request.request_artifact_id).fetch_one(&pool).await.unwrap();
    assert_eq!(state, ("pending".into(), "retry_yielded".into()));
    assert!(
        bidding::docx_round::get_current_docx(&api_pool, workspace, &actor)
            .await
            .unwrap()
            .is_none()
    );
    let stages = cleanup.pending_staging_ids();
    let retained_before = storage
        .enqueued_count(platform::RetentionQueue)
        .await
        .unwrap();
    cleanup.cleanup_pending().await.unwrap();
    assert!(!cleanup.has_pending());
    assert_eq!(
        storage
            .enqueued_count(platform::RetentionQueue)
            .await
            .unwrap(),
        retained_before + 2
    );
    let calls = model.bodies.lock().unwrap().len();
    let published = runtime::execute_with_model(&worker_pool, job, &cancel, &io, &cleanup, &model)
        .await
        .unwrap();
    assert_eq!(published["status"], "succeeded");
    assert!(!cleanup.has_pending());
    assert_eq!(
        model.bodies.lock().unwrap().len(),
        calls,
        "object retry must restore reviewed checkpoint without more model calls"
    );
    assert_eq!(io.writes.load(Ordering::SeqCst), 4);
    assert_eq!(
        runtime::execute_with_model(&worker_pool, job, &cancel, &io, &cleanup, &model)
            .await
            .unwrap(),
        published
    );
    assert_eq!(
        io.writes.load(Ordering::SeqCst),
        4,
        "successful replay must not rewrite objects"
    );
    // Cleanup redelivery after successful retry cannot delete newly owned bytes.
    for stage in stages {
        let _: Value = sqlx::query_scalar("SELECT kb_object_upload_expire_one($1)")
            .bind(stage)
            .fetch_one(&pool)
            .await
            .unwrap();
    }
    assert_eq!(
        runtime::execute_with_model(&worker_pool, job, &cancel, &io, &cleanup, &model)
            .await
            .unwrap(),
        published
    );
    let waiting = WaitingCompositionModel {
        entered: tokio::sync::Notify::new(),
    };
    let cancel_wait = CancellationToken::new();
    let waiting_cleanup = platform::StagedObjectCleanupTracker::new(&worker_pool, &actor);
    let control = async {
        waiting.entered.notified().await;
        let first:chrono::DateTime<chrono::Utc>=sqlx::query_scalar("SELECT heartbeat_at FROM bid_tender_agent_run_artifacts WHERE request_artifact_id=$1 ORDER BY attempt DESC LIMIT 1").bind(jobs[1].request.request_artifact_id).fetch_one(&pool).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_secs(6)).await;
        let renewed:bool=sqlx::query_scalar("SELECT heartbeat_at>$2 AND lease_expires_at>clock_timestamp() FROM bid_tender_agent_run_artifacts WHERE request_artifact_id=$1 ORDER BY attempt DESC LIMIT 1").bind(jobs[1].request.request_artifact_id).bind(first).fetch_one(&pool).await.unwrap();
        assert!(renewed, "live provider work must renew execution ownership");
        cancel_wait.cancel();
    };
    let (outcome, ()) = tokio::join!(
        runtime::execute_with_model(
            &worker_pool,
            &jobs[1],
            &cancel_wait,
            &io,
            &waiting_cleanup,
            &waiting
        ),
        control
    );
    assert_eq!(outcome.err().unwrap().code, "INTERNAL");
    assert!(!waiting_cleanup.has_pending());
    let status:String=sqlx::query_scalar("SELECT status FROM bid_tender_agent_run_artifacts WHERE request_artifact_id=$1 ORDER BY attempt DESC LIMIT 1").bind(jobs[1].request.request_artifact_id).fetch_one(&pool).await.unwrap();
    assert_eq!(status, "retry_yielded");
    // Its original expected DOCX is now stale. Resume still uses the original
    // source, but final CAS must fail and persist a terminal conflict.
    let stale = runtime::execute_with_model(
        &worker_pool,
        &jobs[1],
        &cancel,
        &io,
        &waiting_cleanup,
        &composition_script(&prepared.input, &prepared.analysis),
    )
    .await
    .unwrap();
    assert_eq!(stale["status"], "failed");
    assert_eq!(stale["error_code"], "WORKSPACE_CAS_CONFLICT");
    assert_eq!(waiting_cleanup.pending_count(), 2);
    waiting_cleanup.cleanup_pending().await.unwrap();
    assert_eq!(
        bidding::docx_round::get_current_docx(&api_pool, workspace, &actor)
            .await
            .unwrap()
            .unwrap()["version_id"],
        published["version_id"]
    );
    let failed = runtime::execute_with_model(
        &worker_pool,
        &jobs[2],
        &cancel,
        &io,
        &cleanup,
        &InvalidCompositionModel,
    )
    .await
    .unwrap();
    assert_eq!(failed["status"], "failed");
    assert_eq!(failed["error_code"], "AGENT_OUTPUT_INVALID");
    let terminal: (String, Option<String>) = sqlx::query_as(
        "SELECT status,error_code FROM bid_async_request_snapshot_artifacts WHERE id=$1",
    )
    .bind(jobs[2].request.request_artifact_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        terminal,
        ("failed".into(), Some("AGENT_OUTPUT_INVALID".into()))
    );
    assert_eq!(io.writes.load(Ordering::SeqCst), 6);
    api_pool.close().await;
    worker_pool.close().await;
    pool.close().await;
}

#[tokio::test]
#[ignore = "exports a scripted composition fixture only in a disposable HTTP test environment"]
async fn export_composition_http_fixture() {
    use bidding::docx_composition::{agent as compose, postgres as composition};
    let object_dir = std::path::PathBuf::from(
        std::env::var("OBJECT_DIR").expect("isolated OBJECT_DIR required"),
    );
    assert!(object_dir.is_absolute() && object_dir.starts_with(std::env::temp_dir()));
    assert!(
        std::env::var("KNOWLEDGEBRAIN_S3_BUCKET")
            .unwrap_or_default()
            .is_empty()
    );
    let url =
        std::env::var("KB_TENDER_AGENT_TEST_DATABASE_URL").expect("dedicated test URL required");
    let pool = PgPool::connect(&url).await.unwrap();
    let db: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(db.starts_with("knowledgebrain_test_"));
    let schema_identity = std::env::var_os("KB_RELEASE_DESCRIPTOR_PATH")
        .map(|_| platform::SchemaRuntimeIdentity::load_from_env().unwrap());
    if let Some(identity) = &schema_identity {
        platform::verify_runtime_schema(&pool, identity)
            .await
            .unwrap();
        // Verify the selected baseline rows still fail closed when corrupted or
        // absent. Only this disposable admin transaction bypasses append-only
        // triggers; rollback restores every byte, with no receipt rewrite.
        let spec = platform::load_frozen_seed_spec().unwrap();
        let keys = spec
            .tables
            .iter()
            .find(|table| table.table == "bid_authoring_contract_artifacts")
            .unwrap()
            .primary_key_values
            .as_ref()
            .unwrap();
        let id = Uuid::parse_str(&keys[0][0]).unwrap();
        for statement in [
            "UPDATE bid_authoring_contract_artifacts SET canonical_payload=canonical_payload||decode('20','hex'),content_sha256=kb_bid_v2_sha256_bytes(canonical_payload||decode('20','hex')) WHERE id=$1",
            "DELETE FROM bid_authoring_contract_artifacts WHERE id=$1",
        ] {
            let mut tx = pool.begin().await.unwrap();
            sqlx::query("SET LOCAL session_replication_role=replica")
                .execute(&mut *tx)
                .await
                .unwrap();
            assert_eq!(
                sqlx::query(statement)
                    .bind(id)
                    .execute(&mut *tx)
                    .await
                    .unwrap()
                    .rows_affected(),
                1
            );
            assert!(
                platform::verify_runtime_schema_on_connection(&mut tx, identity)
                    .await
                    .is_err()
            );
            tx.rollback().await.unwrap();
        }
        platform::verify_runtime_schema(&pool, identity)
            .await
            .unwrap();
    }
    let (analysis_request, input) = seed(&pool).await;
    let result = postgres::execute_with_model(
        &pool,
        &analysis_request,
        &CancellationToken::new(),
        &script(&input),
    )
    .await
    .unwrap();
    assert_eq!(result["analysis_quality"], "verified");
    let (workspace,actor):(Uuid,String)=sqlx::query_as("SELECT w.id,'user:'||p.owner_user_id FROM bid_submission_workspaces w JOIN bid_projects p ON p.id=w.project_id WHERE p.id=$1")
        .bind(Uuid::parse_str(&input.project_id).unwrap()).fetch_one(&pool).await.unwrap();
    let options: sqlx::postgres::PgConnectOptions = url.parse().unwrap();
    let api_pool = PgPool::connect_with(runtime_pool_options(&options, "kb_runtime_api"))
        .await
        .unwrap();
    let worker_pool = PgPool::connect_with(runtime_pool_options(&options, "kb_runtime_worker"))
        .await
        .unwrap();
    let basis = bidding::docx_round::get_docx_round_basis(&api_pool, workspace, &actor)
        .await
        .unwrap()
        .unwrap();
    let config = compose::Config {
        provider: config().provider,
        limits: compose::Limits {
            max_no_progress_turns: 6,
            max_focus_turns: 24,
            max_focus_replans: 2,
            max_turns: 30,
            max_tool_calls: 40,
            max_physical_calls: 40,
            max_read_bytes: 1000000,
            max_context_bytes: 500000,
            max_context_tokens: 131072,
            image_token_reserve: 16384,
            token_safety_margin: 4096,
            max_tool_result_bytes: 100000,
            max_review_rounds: 3,
            max_docx_bytes: 1000000,
        },
    };
    let prepared = composition::prepare(&api_pool, workspace, basis, None, &actor, config)
        .await
        .unwrap();

    let destination = std::path::PathBuf::from(
        std::env::var("KB_COMPOSITION_HTTP_FIXTURE").expect("fixture output required"),
    );
    assert!(destination.is_absolute() && destination.starts_with(&object_dir));
    let receipt =
        composition::create_request(&api_pool, &prepared.request, &Uuid::new_v4().to_string())
            .await
            .unwrap();
    let id = composition_identity(&receipt);
    let owner = composition_claim(&worker_pool, &id).await;
    let journal = composition::PgJournal {
        pool: &worker_pool,
        request: &id,
        owner: &owner,
    };
    compose::run(
        &prepared.input,
        &prepared.analysis,
        &prepared.request.config,
        &journal,
        &composition_script(&prepared.input, &prepared.analysis),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    let publication = composition::prepare_publication(&worker_pool, &id)
        .await
        .unwrap();
    let docx_stage = Uuid::new_v4();
    let manifest_stage = Uuid::new_v4();
    for (stage, sha, media, bytes) in [
        (
            docx_stage,
            publication.docx.sha256(),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            publication.docx.bytes(),
        ),
        (
            manifest_stage,
            publication.manifest_sha256.as_str(),
            "application/json",
            publication.manifest.as_slice(),
        ),
    ] {
        platform::stage_object_upload(
            &worker_pool,
            stage,
            &platform::object_ref(sha),
            sha,
            media,
            bytes.len() as i64,
            &actor,
        )
        .await
        .unwrap();
        platform::write_blob_async(sha, bytes).await.unwrap();
    }
    let published =
        composition::publish_staged(&worker_pool, &id, &owner, docx_stage, manifest_stage)
            .await
            .unwrap();
    if let Some(identity) = &schema_identity {
        // Normal upload registers a converter contract. The same original
        // release receipt must remain valid for all runtime roles afterwards.
        let runtime_contracts: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM bid_authoring_contract_artifacts WHERE contract_kind='converter'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(runtime_contracts > 0);
        for runtime_pool in [&pool, &api_pool, &worker_pool] {
            platform::verify_runtime_schema(runtime_pool, identity)
                .await
                .unwrap();
        }
    }
    let model_steps = composition_script(&prepared.input, &prepared.analysis)
        .turns
        .into_inner()
        .unwrap();
    std::fs::write(destination,serde_json::to_vec(&json!({"workspace":workspace,"actor":actor,"basis":prepared.request.basis,"completed":id,
        "current":published,"config":prepared.request.config,"documents":input.documents,"tender_runtime":self::config(),
        "project":input.project_id,"object_directory":platform::blob_path(publication.docx.sha256()).unwrap().parent().unwrap(),
        "model_steps":model_steps})).unwrap()).unwrap();
    api_pool.close().await;
    worker_pool.close().await;
    pool.close().await;
}

#[tokio::test]
#[ignore = "requires KB_TENDER_AGENT_TEST_DATABASE_URL pointing to a fresh owned test database"]
async fn analysis_three_boundaries_are_atomic_and_resume_received_responses() {
    use bidding::tender_analysis::agent::{self, Journal};
    let url =
        std::env::var("KB_TENDER_AGENT_TEST_DATABASE_URL").expect("dedicated test URL required");
    let pool = PgPool::connect(&url).await.unwrap();
    let db: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        db.starts_with("knowledgebrain_test_"),
        "refuse non-test database"
    );
    for boundary in [0, 1, 2, 3] {
        let (request, input) = seed(&pool).await;
        let owner = composition_claim(&pool, &request).await;
        let journal = LostReviewAck {
            journal: postgres::PgJournal {
                pool: &pool,
                request: &request,
                owner: &owner,
                source_reader: None,
            },
            fail_turn: usize::MAX,
            fail_sequence: Some(boundary),
            reject_prepared: boundary == 0,
        };
        let model = script(&input);
        let expected_calls = model.turns.lock().unwrap().len();
        let error = agent::run(
            &input,
            &config(),
            &journal,
            &model,
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM bid_tender_agent_call_attempts WHERE request_artifact_id=$1",
        )
        .bind(request.request_artifact_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        if boundary == 0 {
            assert_eq!(error.code, "FROZEN_INPUT_DIGEST_MISMATCH");
            assert_eq!(count, 0, "failed checkpoint must roll back reservation");
            assert!(journal.load().await.unwrap().is_none());
            assert!(model.bodies.lock().unwrap().is_empty());
            continue;
        }
        assert_eq!(error.code, "INTERNAL", "{error:?}");
        assert_eq!(count, 1);
        let saved = journal.load().await.unwrap().unwrap();
        assert_eq!(saved.turn, usize::from(boundary == 3));
        assert_eq!(saved.journal.sequence, boundary);
        assert_eq!(saved.journal.response().is_some(), boundary == 2);
        assert_eq!(saved.tool_calls, usize::from(boundary == 3));
        journal.journal.save(&saved, &json!({})).await.unwrap();
        if boundary == 3 {
            let value = json!(saved);
            let owner = value["dispatch"]["active"]["id"].as_str().unwrap();
            for (case, expected) in [
                ("refund", "Main root cost decreased"),
                ("owner", "Main dispatch detached owner"),
                ("watch", "Main dispatch detached owner"),
                ("identity", "Main root shape or identity"),
            ] {
                let mut forged = value.clone();
                forged["journal"]["sequence"] = json!(boundary + 1);
                match case {
                    "refund" => forged["dispatch"]["entries"][owner]["spent_batches"] = json!(0),
                    "owner" => forged["dispatch"]["active"]["id"] = json!("f".repeat(64)),
                    "watch" => forged["main_progress"]["watch"]["replans"] = json!(1),
                    "identity" => {
                        forged["dispatch"]["entries"][owner]["source_id"] = json!("foreign-source")
                    }
                    _ => unreachable!(),
                }
                let forged: agent::Checkpoint = serde_json::from_value(forged).unwrap();
                let error = journal.journal.save(&forged, &json!({})).await.unwrap_err();
                assert!(error.message.contains(expected), "{case}: {error:?}");
                assert_eq!(json!(journal.load().await.unwrap().unwrap()), value);
            }
        }
        let mut forged = saved.clone();
        forged.journal.sequence += 1;
        forged.turn += 1;
        // A direct commit from prepared, or a response-state turn change, is rejected.
        assert!(journal.journal.save(&forged, &json!({})).await.is_err());
        if boundary == 2 {
            assert!(
                journal
                    .journal
                    .reserve(&saved, saved.journal.body().unwrap())
                    .await
                    .is_err(),
                "received response cannot reserve another model call"
            );
        }
        // New owner must recover the same request/response; stale owner is fenced.
        sqlx::query("SELECT kb_bid_v2_tender_agent_yield_for_retry($1,$2::kb_sha256,$3,$4,'INTERNAL','boundary ACK lost')")
            .bind(request.request_artifact_id).bind(&request.frozen_input_sha256).bind(owner.attempt).bind(owner.execution_owner_token)
            .execute(&pool).await.unwrap();
        let next = composition_claim(&pool, &request).await;
        assert_eq!(
            journal
                .journal
                .save(&saved, &json!({}))
                .await
                .unwrap_err()
                .code,
            "REQUEST_ATTEMPT_SUPERSEDED"
        );
        let resumed = postgres::PgJournal {
            pool: &pool,
            request: &request,
            owner: &next,
            source_reader: None,
        };
        let result = agent::run(
            &input,
            &config(),
            &resumed,
            &model,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(result.quality, "verified");
        assert_eq!(model.bodies.lock().unwrap().len(), expected_calls);
        let completed = resumed.load().await.unwrap().unwrap();
        let sequence: i32 = sqlx::query_scalar("SELECT max(batch_ordinal) FROM bid_tender_agent_checkpoint_artifacts WHERE request_artifact_id=$1 AND stage_kind='analysis_checkpoint'")
            .bind(request.request_artifact_id).fetch_one(&pool).await.unwrap();
        assert_eq!(sequence as usize, completed.journal.sequence);
        assert!(completed.journal.sequence > completed.turn);
    }
    pool.close().await;
}

struct PreloadedPgModel {
    first_record: Mutex<Option<Value>>,
    rest: Script,
    bodies: Mutex<Vec<Vec<u8>>>,
}

impl PreloadedPgModel {
    fn new(input: &FrozenInput) -> Self {
        let rest = script(input);
        let first_record = {
            let mut turns = rest.turns.lock().unwrap();
            let first: Vec<_> = turns.drain(..4).collect();
            assert_eq!(first[3].0, "put_record");
            // Metadata can need its own page; source delivery itself comes
            // from the first reserved request, with no preliminary read turn.
            turns.push_front(first[1].clone());
            first[3].1.clone()
        };
        Self {
            first_record: Mutex::new(Some(first_record)),
            rest,
            bodies: Mutex::new(vec![]),
        }
    }
}

#[async_trait]
impl Model for PreloadedPgModel {
    async fn turn(&self, config: &Config, bytes: &[u8]) -> Result<ChatTurn, AgentError> {
        self.bodies.lock().unwrap().push(bytes.to_vec());
        let record = self.first_record.lock().unwrap().take();
        if let Some(record) = record {
            let body: Value = serde_json::from_slice(bytes).unwrap();
            let packet: Value = serde_json::from_str(
                body["messages"].as_array().unwrap().last().unwrap()["content"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap();
            let source_id = &record["sources"][0]["source_id"];
            assert_eq!(
                packet["preloaded_evidence"]["main_work"]["source_scope"],
                json!([source_id])
            );
            assert!(
                packet["preloaded_evidence"]["assigned_evidence"]["boundary_evidence"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|entry| entry["tool"] == "read_source"
                        && entry["source"]["source_id"] == *source_id
                        && entry["source"]["start"] == 0
                        && entry["source"]["end"] == record["sources"][0]["end"])
            );
            return Ok(ChatTurn {
                content: String::new(),
                finish_reason: "tool_calls".into(),
                usage: None,
                tool_calls: vec![ChatToolCall {
                    id: Uuid::new_v4().to_string(),
                    name: "put_record".into(),
                    arguments: record.to_string(),
                }],
            });
        }
        self.rest.turn(config, bytes).await
    }
}

#[tokio::test]
#[ignore = "requires KB_TENDER_AGENT_TEST_DATABASE_URL pointing to a fresh owned test database"]
async fn main_and_reviewer_preloads_recover_all_postgres_boundaries_without_early_or_double_receipts()
 {
    use bidding::tender_analysis::{
        agent::{self, Journal, Role},
        digest,
    };
    let pool = PgPool::connect(
        &std::env::var("KB_TENDER_AGENT_TEST_DATABASE_URL").expect("dedicated test URL required"),
    )
    .await
    .unwrap();
    let database: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        database.starts_with("knowledgebrain_test_"),
        "refuse non-test database"
    );
    for reviewer in [false, true] {
        for boundary in [1, 2, 3] {
            let (request, input) = seed(&pool).await;
            let owner = composition_claim(&pool, &request).await;
            let model = PreloadedPgModel::new(&input);
            let mut expected_calls = 1 + model.rest.turns.lock().unwrap().len();
            let mut prior = None;
            if reviewer {
                let handoff_turn = 2 + model
                    .rest
                    .turns
                    .lock()
                    .unwrap()
                    .iter()
                    .position(|(name, _)| *name == "put_analysis_check")
                    .unwrap();
                let setup = LostReviewAck {
                    journal: postgres::PgJournal {
                        pool: &pool,
                        request: &request,
                        owner: &owner,
                        source_reader: None,
                    },
                    fail_turn: handoff_turn,
                    fail_sequence: None,
                    reject_prepared: false,
                };
                let error =
                    agent::run(&input, &config(), &setup, &model, &CancellationToken::new())
                        .await
                        .unwrap_err();
                assert_eq!(error.code, "INTERNAL", "review setup: {error:?}");
                let state = setup.load().await.unwrap().unwrap();
                assert_eq!(state.role, Role::Reviewer);
                assert!(state.reviewer_coverage.text.is_empty());
                assert!(state.reviewer_coverage.candidate.is_empty());
                // The v2 automatic handoff follows the global-check inventory;
                // a local source disposition alone cannot enter Reviewer.
                assert_eq!(
                    model.rest.turns.lock().unwrap().pop_front().unwrap().0,
                    "request_review"
                );
                expected_calls -= 1;
                prior = Some(state);
            }
            let turn = prior.as_ref().map_or(0, |state| state.turn);
            let sequence = prior.as_ref().map_or(0, |state| state.journal.sequence);
            let calls_before = model.bodies.lock().unwrap().len();
            let interrupted = LostReviewAck {
                journal: postgres::PgJournal {
                    pool: &pool,
                    request: &request,
                    owner: &owner,
                    source_reader: None,
                },
                fail_turn: usize::MAX,
                fail_sequence: Some(sequence + boundary),
                reject_prepared: false,
            };
            let error = agent::run(
                &input,
                &config(),
                &interrupted,
                &model,
                &CancellationToken::new(),
            )
            .await
            .unwrap_err();
            assert_eq!(
                error.code, "INTERNAL",
                "reviewer={reviewer}, boundary={boundary}: {error:?}"
            );
            let saved = interrupted.load().await.unwrap().unwrap();
            assert_eq!(saved.journal.sequence, sequence + boundary);
            let prepared_bytes:Vec<u8> = sqlx::query_scalar("SELECT canonical_payload FROM bid_tender_agent_checkpoint_artifacts WHERE request_artifact_id=$1 AND stage_kind='analysis_checkpoint' AND batch_ordinal=$2")
                .bind(request.request_artifact_id).bind((sequence + 1) as i32).fetch_one(&pool).await.unwrap();
            let prepared: agent::Checkpoint = serde_json::from_slice(&prepared_bytes).unwrap();
            let reserved = prepared.journal.body().unwrap().to_vec();
            let body: Value = serde_json::from_slice(&reserved).unwrap();
            let packet: Value = serde_json::from_str(
                body["messages"].as_array().unwrap().last().unwrap()["content"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap();
            let sent = &packet["preloaded_evidence"];
            assert!(
                sent["assigned_evidence"].is_object(),
                "both roles must actually reserve original evidence"
            );
            assert_eq!(prepared.turn, turn);
            assert_eq!(
                prepared.read_bytes,
                prior.as_ref().map_or(0, |state| state.read_bytes)
            );
            if reviewer {
                assert_eq!(
                    json!(prepared.analysis.coverage),
                    json!(prior.as_ref().unwrap().analysis.coverage)
                );
                assert!(prepared.reviewer_coverage.text.is_empty());
                assert!(prepared.reviewer_coverage.candidate.is_empty());
                assert!(
                    !sent["assigned_evidence"]["candidates"]
                        .as_array()
                        .unwrap()
                        .is_empty()
                );
            } else {
                assert!(prepared.main_work.is_none());
                assert!(prepared.analysis.coverage.text.is_empty());
                assert!(prepared.analysis.records.is_empty());
            }
            if boundary < 3 {
                assert_eq!(saved.turn, turn);
                assert_eq!(saved.read_bytes, prepared.read_bytes);
                assert_eq!(saved.tool_calls, prepared.tool_calls);
                assert_eq!(json!(saved.analysis), json!(prepared.analysis));
                assert_eq!(
                    json!(saved.reviewer_coverage),
                    json!(prepared.reviewer_coverage)
                );
                assert_eq!(json!(saved.main_work), json!(prepared.main_work));
                assert_eq!(saved.journal.response().is_some(), boundary == 2);
                assert_eq!(saved.journal.body().unwrap(), reserved);
                let stop_after_commit = LostReviewAck {
                    journal: postgres::PgJournal {
                        pool: &pool,
                        request: &request,
                        owner: &owner,
                        source_reader: None,
                    },
                    fail_turn: turn + 1,
                    fail_sequence: None,
                    reject_prepared: false,
                };
                let error = agent::run(
                    &input,
                    &config(),
                    &stop_after_commit,
                    &model,
                    &CancellationToken::new(),
                )
                .await
                .unwrap_err();
                assert_eq!(error.code, "INTERNAL");
            }
            let committed = interrupted.load().await.unwrap().unwrap();
            assert_eq!(committed.turn, turn + 1);
            assert_eq!(committed.tool_calls, prepared.tool_calls + 1);
            assert_eq!(committed.journal.sequence, sequence + 3);
            assert!(committed.journal.pending.is_none());
            assert_eq!(
                model.bodies.lock().unwrap().len(),
                calls_before + 1,
                "received/committed restoration must not recall the tested model turn"
            );
            assert_eq!(model.bodies.lock().unwrap()[calls_before], reserved);
            let last_assistant = committed
                .transcript
                .iter()
                .rposition(|message| message["role"] == "assistant")
                .unwrap();
            let tool_bytes: usize = committed.transcript[last_assistant..]
                .iter()
                .filter(|message| message["role"] == "tool")
                .map(|message| message["content"].as_str().unwrap().len())
                .sum();
            assert_eq!(
                committed.read_bytes,
                prepared.read_bytes + serde_json::to_vec(sent).unwrap().len() + tool_bytes,
                "one exact evidence delivery and one executed tool output must be charged"
            );
            let source = &input.source_units[0];
            let citation = serde_json::from_value(json!({"source_id":source.source_unit_revision_id,"start":0,"end":source.text.len()})).unwrap();
            if reviewer {
                bidding::tender_analysis::tools::validate_span(
                    &input,
                    &committed.reviewer_coverage,
                    &citation,
                )
                .unwrap();
                for candidate in sent["assigned_evidence"]["candidates"].as_array().unwrap() {
                    assert_eq!(
                        committed
                            .reviewer_coverage
                            .candidate
                            .get(candidate["reference"].as_str().unwrap())
                            .map(String::as_str),
                        candidate["sha256"].as_str()
                    );
                }
                assert_eq!(json!(committed.analysis), json!(prepared.analysis));
                assert!(
                    committed.source_review.as_ref().unwrap().results.is_empty(),
                    "delivery is not semantic review approval"
                );
            } else {
                bidding::tender_analysis::tools::validate_span(
                    &input,
                    &committed.analysis.coverage,
                    &citation,
                )
                .unwrap();
                assert_eq!(
                    committed.analysis.records.len(),
                    1,
                    "received replay cannot duplicate the first record"
                );
                assert!(committed.reviewer_coverage.text.is_empty());
            }
            let attempts:Vec<Vec<u8>> = sqlx::query_scalar("SELECT provider_body FROM bid_tender_agent_call_attempts WHERE request_artifact_id=$1 AND batch_ordinal=$2 ORDER BY call_ordinal")
                .bind(request.request_artifact_id).bind(turn as i32).fetch_all(&pool).await.unwrap();
            assert_eq!(attempts.len(), if boundary == 1 { 2 } else { 1 });
            assert!(attempts.iter().all(|body| body == &reserved));
            let committed_digest = digest(&committed).unwrap();
            interrupted
                .journal
                .save(&committed, &json!({}))
                .await
                .unwrap();
            assert_eq!(
                digest(&interrupted.load().await.unwrap().unwrap()).unwrap(),
                committed_digest,
                "committed replay is idempotent in PostgreSQL"
            );
            let result = agent::run(
                &input,
                &config(),
                &interrupted.journal,
                &model,
                &CancellationToken::new(),
            )
            .await
            .unwrap();
            assert_eq!(result.quality, "verified");
            assert_eq!(model.bodies.lock().unwrap().len(), expected_calls);
            let completed = interrupted.load().await.unwrap().unwrap();
            assert_eq!(completed.review_rounds, 1);
            assert!(completed.done);
        }
    }
    pool.close().await;
}

#[tokio::test]
#[ignore = "requires KB_TENDER_AGENT_TEST_DATABASE_URL pointing to a fresh owned test database"]
async fn source_review_final_batch_recovers_without_extra_model_calls_or_double_finalization() {
    use bidding::tender_analysis::agent::{self, Journal};
    let url =
        std::env::var("KB_TENDER_AGENT_TEST_DATABASE_URL").expect("dedicated test URL required");
    let pool = PgPool::connect(&url).await.unwrap();
    let db: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        db.starts_with("knowledgebrain_test_"),
        "refuse non-test database"
    );
    for boundary in [1, 2, 3] {
        let (request, input) = seed(&pool).await;
        let owner = composition_claim(&pool, &request).await;
        let model = script(&input);
        let expected_calls = model.turns.lock().unwrap().len();
        let sequence = (expected_calls - 1) * 3 + boundary;
        let journal = LostReviewAck {
            journal: postgres::PgJournal {
                pool: &pool,
                request: &request,
                owner: &owner,
                source_reader: None,
            },
            fail_turn: usize::MAX,
            fail_sequence: Some(sequence),
            reject_prepared: false,
        };
        let error = agent::run(
            &input,
            &config(),
            &journal,
            &model,
            &CancellationToken::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "INTERNAL", "{error:?}");
        let saved = journal.load().await.unwrap().unwrap();
        assert_eq!(saved.journal.sequence, sequence);
        assert_eq!(saved.done, boundary == 3);
        assert_eq!(saved.review_rounds, usize::from(boundary == 3));
        assert_eq!(
            saved.source_review.as_ref().unwrap().results.is_empty(),
            boundary != 3
        );
        if boundary != 3 {
            assert!(
                saved.review.is_none(),
                "received response is not committed semantic approval"
            );
        }
        let resumed = agent::run(
            &input,
            &config(),
            &journal.journal,
            &model,
            &CancellationToken::new(),
        )
        .await;
        let result = resumed.unwrap_or_else(|error| {
            let path = std::env::temp_dir().join(format!(
                "kb-source-review-boundary-{boundary}-{}.json",
                Uuid::new_v4()
            ));
            std::fs::write(&path, serde_json::to_vec_pretty(&saved).unwrap()).unwrap();
            panic!(
                "boundary {boundary}: {error:?}; isolated fixture checkpoint: {}",
                path.display()
            );
        });
        assert_eq!(result.quality, "verified");
        assert_eq!(
            model.bodies.lock().unwrap().len(),
            expected_calls,
            "finalization cannot ask the model for an empty submission"
        );
        let completed = journal.load().await.unwrap().unwrap();
        assert_eq!(completed.review_rounds, 1);
        assert!(completed.done);
        assert!(completed.journal.pending.is_none());
        assert_eq!(
            completed
                .source_review
                .as_ref()
                .unwrap()
                .completed_analysis_sha256
                .as_ref(),
            Some(&bidding::tender_analysis::digest(&completed.analysis).unwrap())
        );
    }
    pool.close().await;
}

#[tokio::test]
#[ignore = "requires KB_TENDER_AGENT_TEST_DATABASE_URL pointing to a fresh owned test database"]
async fn repair_checkpoint_rejects_malformed_dispositions_without_changing_saved_state() {
    use bidding::tender_analysis::agent::{self, Journal};
    let url =
        std::env::var("KB_TENDER_AGENT_TEST_DATABASE_URL").expect("dedicated test URL required");
    let pool = PgPool::connect(&url).await.unwrap();
    let db: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        db.starts_with("knowledgebrain_test_"),
        "refuse non-test database"
    );
    let (request, input) = seed(&pool).await;
    let owner = composition_claim(&pool, &request).await;
    let journal = LostReviewAck {
        journal: postgres::PgJournal {
            pool: &pool,
            request: &request,
            owner: &owner,
            source_reader: None,
        },
        fail_turn: usize::MAX,
        fail_sequence: Some(1),
        reject_prepared: false,
    };
    let model = script(&input);
    assert_eq!(
        agent::run(
            &input,
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
    let saved = journal.load().await.unwrap().unwrap();
    let empty_tasks = json!(saved.repair.tasks);
    assert_eq!(empty_tasks["entries"], json!({}));
    assert_eq!(empty_tasks["aliases"], json!({}));
    assert_eq!(
        json!(saved.repair),
        json!({"feedback_sha256":null,"baseline":{},"results":{},"tasks":empty_tasks})
    );
    for (index,repair) in [
        json!(null),
        json!({"feedback_sha256":null,"baseline":{},"results":{},"approve":true}),
        json!({"feedback_sha256":"bad","baseline":{},"results":{}}),
        json!({"feedback_sha256":null,"baseline":{"record:invented":"a".repeat(64)},"results":{}}),
        json!({"feedback_sha256":"a".repeat(64),"baseline":{},"results":{ "bad":{"conclusion":"verified"}}}),
        json!({"feedback_sha256":"a".repeat(64),"baseline":{},"results":{
            "b".repeat(64):{"conclusion":"disputed","summary":"","sources":[],"candidate_versions":{}}}}),
    ].into_iter().enumerate() {
        let mut invalid = json!(saved);
        invalid["journal"]["sequence"] = json!(saved.journal.sequence + 1);
        let mut repair = repair;
        if let Some(object) = repair.as_object_mut() {
            object.insert("tasks".into(), empty_tasks.clone());
        }
        invalid["repair"] = repair;
        let error = sqlx::query(
            "SELECT kb_bid_v2_tender_agent_checkpoint_put($1,$2::kb_sha256,$3,$4,$5,$6)",
        )
        .bind(request.request_artifact_id)
        .bind(&request.frozen_input_sha256)
        .bind(owner.attempt)
        .bind(owner.execution_owner_token)
        .bind(invalid)
        .bind(json!({}))
        .execute(&pool)
        .await
        .unwrap_err();
        assert_eq!(error.as_database_error().unwrap().message(), if index < 3 {
            "FROZEN_INPUT_DIGEST_MISMATCH: checkpoint sequence, identity or budget"
        } else { "FROZEN_INPUT_DIGEST_MISMATCH: repair disposition shape" }, "{error}");
        assert_eq!(json!(journal.load().await.unwrap().unwrap()), json!(saved));
    }
    // The rejected writes do not strand the original durable reservation.
    let result = agent::run(
        &input,
        &config(),
        &journal.journal,
        &model,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(result.quality, "verified");
    pool.close().await;
}

// Synthetic ledger transitions isolate SQL persistence guarantees. They do not
// claim that the scripted model has extracted or independently approved a tender.
#[tokio::test]
#[ignore = "requires KB_TENDER_AGENT_TEST_DATABASE_URL pointing to a fresh owned test database"]
async fn repair_task_checkpoint_preserves_accounting_and_boundary_atomicity() {
    use bidding::tender_analysis::agent::{self, Journal};

    async fn put(journal: &postgres::PgJournal<'_>, value: &Value) -> Result<(), sqlx::Error> {
        sqlx::query("SELECT kb_bid_v2_tender_agent_checkpoint_put($1,$2::kb_sha256,$3,$4,$5,$6)")
            .bind(journal.request.request_artifact_id)
            .bind(&journal.request.frozen_input_sha256)
            .bind(journal.owner.attempt)
            .bind(journal.owner.execution_owner_token)
            .bind(value)
            .bind(json!({}))
            .execute(journal.pool)
            .await
            .map(|_| ())
    }
    fn digest(value: &Value) -> String {
        use sha2::{Digest, Sha256};
        hex::encode(Sha256::digest(
            serde_json_canonicalizer::to_vec(value).unwrap(),
        ))
    }
    let url =
        std::env::var("KB_TENDER_AGENT_TEST_DATABASE_URL").expect("dedicated test URL required");
    let pool = PgPool::connect(&url).await.unwrap();
    let db: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        db.starts_with("knowledgebrain_test_"),
        "refuse non-test database"
    );
    let (request, input) = seed(&pool).await;
    let owner = composition_claim(&pool, &request).await;
    let journal = LostReviewAck {
        journal: postgres::PgJournal {
            pool: &pool,
            request: &request,
            owner: &owner,
            source_reader: None,
        },
        fail_turn: usize::MAX,
        fail_sequence: Some(20),
        reject_prepared: false,
    };
    let error = agent::run(
        &input,
        &config(),
        &journal,
        &script(&input),
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "INTERNAL", "{error:?}");
    let mut committed = journal.load().await.unwrap().unwrap();
    let body = committed.journal.body().unwrap().to_vec();
    let response = committed.journal.response().unwrap().clone();
    committed.journal.committed().unwrap();
    committed.turn += 1;
    committed.tool_calls += 1;
    let mut committed = json!(committed);
    let source = &input.source_units[0];
    let finding = json!({"code":"FIELD_RECHECK","message":"Check source correspondence",
        "correction":"Compare the cited source", "affected":[],
        "sources":[{"source_id":source.source_unit_revision_id,"start":0,"end":source.text.len()}]});
    let finding_sha = digest(&finding);
    committed["review_draft"] = json!({"a":finding,"b":finding});
    let feedback_sha = digest(&json!([committed["review_rounds"], [finding, finding]]));
    let entry = |spent| {
        json!({"finding_sha256":finding_sha,"committed_turns":spent,
        "watch":{"no_progress_turns":0,"focus_turns":0,"replans":1,"recovery":"running"},
        "attempted_dependencies":["d".repeat(64)],"inherited_blocked":false})
    };
    committed["repair"]["tasks"] = json!({"active":null,"aliases":{"a":"a","b":"b"},
        "entries":{"a":entry(0),"b":entry(0)},"feedback_sha256":feedback_sha,
        "last_committed_turn":null});
    put(&journal.journal, &committed).await.unwrap();
    // Establish historical spend through actual three-boundary writes.
    for id in ["a", "b", "b"] {
        let mut next: agent::Checkpoint = serde_json::from_value(committed.clone()).unwrap();
        next.journal.prepare(next.turn, "main", &body).unwrap();
        let mut next = json!(next);
        next["repair"]["tasks"]["active"] = json!(id);
        next["main_progress"]["watch"] = next["repair"]["tasks"]["entries"][id]["watch"].clone();
        let next: agent::Checkpoint = serde_json::from_value(next).unwrap();
        journal.journal.reserve(&next, &body).await.unwrap();
        let mut next = next;
        next.journal.responded(response.clone()).unwrap();
        journal.journal.save(&next, &json!({})).await.unwrap();
        next.journal.committed().unwrap();
        next.turn += 1;
        next.tool_calls += 1;
        let mut next = json!(next);
        next["repair"]["tasks"]["entries"][id]["committed_turns"] = json!(
            next["repair"]["tasks"]["entries"][id]["committed_turns"]
                .as_u64()
                .unwrap()
                + 1
        );
        next["repair"]["tasks"]["last_committed_turn"] = next["turn"].clone();
        put(&journal.journal, &next).await.unwrap();
        committed = next;
    }
    // Repeating the exact committed payload is idempotent, including spent turns.
    put(&journal.journal, &committed).await.unwrap();
    assert_eq!(json!(journal.load().await.unwrap().unwrap()), committed);

    let mut prepared: agent::Checkpoint = serde_json::from_value(committed.clone()).unwrap();
    prepared
        .journal
        .prepare(prepared.turn, "main", &body)
        .unwrap();
    let prepared = json!(prepared);
    sqlx::query("SELECT kb_bid_v2_tender_agent_reserve($1,$2::kb_sha256,$3,$4,$5,$6,$7)")
        .bind(request.request_artifact_id)
        .bind(&request.frozen_input_sha256)
        .bind(owner.attempt)
        .bind(owner.execution_owner_token)
        .bind(committed["turn"].as_i64().unwrap() as i32)
        .bind("main")
        .bind(&body)
        .execute(&pool)
        .await
        .unwrap();
    let tasks_path = "/repair/tasks";
    for (path, replacement, message) in [
        (
            "/repair/tasks/active",
            json!("missing"),
            "repair task alias",
        ),
        (
            "/repair/tasks/aliases/b",
            json!("missing"),
            "repair task alias",
        ),
        (
            "/repair/tasks/entries/a/committed_turns",
            json!(-1),
            "repair task shape",
        ),
        (
            "/repair/tasks/entries/a/committed_turns",
            json!(0),
            "repair task accounting",
        ),
        (
            "/repair/tasks/entries/a/watch/replans",
            json!(0),
            "repair task accounting",
        ),
        (
            "/repair/tasks/entries/a/attempted_dependencies",
            json!([]),
            "repair task accounting",
        ),
        (
            "/repair/tasks/last_committed_turn",
            json!(0),
            "repair task accounting",
        ),
        (
            "/repair/tasks/entries/a/finding_sha256",
            json!("f".repeat(64)),
            "repair task finding identity",
        ),
        (
            "/repair/tasks/feedback_sha256",
            json!("f".repeat(64)),
            "repair task feedback identity",
        ),
        (
            "/repair/tasks/entries/a/attempted_dependencies",
            json!(["bad"]),
            "repair task shape",
        ),
        (
            "/repair/tasks/entries/a/attempted_dependencies",
            json!(["d".repeat(64), "d".repeat(64)]),
            "repair task shape",
        ),
        (
            "/repair/tasks/entries/a/committed_turns",
            json!(2),
            "preparation charged repair task",
        ),
        (
            "/main_progress/watch/focus_turns",
            json!(99),
            "preparation detached task watch",
        ),
        (
            "/analysis/dispositions",
            json!({"invented":{}}),
            "preparation changed business state",
        ),
        (
            "/main_progress/seen",
            json!(["invented"]),
            "preparation changed business state",
        ),
        (
            "/repair/baseline",
            json!({"record:invented":"a".repeat(64)}),
            "repair disposition shape",
        ),
    ] {
        let mut invalid = prepared.clone();
        *invalid.pointer_mut(path).unwrap() = replacement;
        let error = put(&journal.journal, &invalid).await.unwrap_err();
        assert!(
            error
                .as_database_error()
                .unwrap()
                .message()
                .ends_with(message),
            "{path}: {error}"
        );
        assert_eq!(json!(journal.load().await.unwrap().unwrap()), committed);
    }
    let mut detached = prepared.clone();
    detached["repair"]["tasks"]["active"] = json!(null);
    detached["main_progress"]["watch"]["focus_turns"] = json!(99);
    let error = put(&journal.journal, &detached).await.unwrap_err();
    assert!(
        error
            .to_string()
            .contains("preparation detached task watch")
    );
    assert_eq!(json!(journal.load().await.unwrap().unwrap()), committed);
    for key in ["entries", "aliases"] {
        let mut invalid = prepared.clone();
        invalid.pointer_mut(tasks_path).unwrap()[key]
            .as_object_mut()
            .unwrap()
            .remove("b");
        assert!(put(&journal.journal, &invalid).await.is_err());
        assert_eq!(json!(journal.load().await.unwrap().unwrap()), committed);
    }
    let mut merged = prepared;
    merged["repair"]["tasks"]["aliases"]["b"] = json!("a");
    merged["repair"]["tasks"]["active"] = json!("a");
    let undercounted = put(&journal.journal, &merged).await.unwrap_err();
    assert!(undercounted.to_string().contains("merge lost accounting"));
    merged["repair"]["tasks"]["entries"]["a"]["committed_turns"] = json!(3);
    // A retired entry remains byte-for-byte; only its alias is redirected.
    assert_eq!(
        merged["repair"]["tasks"]["entries"]["b"],
        committed["repair"]["tasks"]["entries"]["b"]
    );
    put(&journal.journal, &merged).await.unwrap();
    let mut received: agent::Checkpoint = serde_json::from_value(merged.clone()).unwrap();
    received.journal.responded(response.clone()).unwrap();
    let received = json!(received);
    let mut forged = received.clone();
    forged["repair"]["tasks"]["active"] = json!(null);
    let error = put(&journal.journal, &forged).await.unwrap_err();
    assert!(
        error
            .to_string()
            .contains("response boundary changed prepared state")
    );
    assert_eq!(json!(journal.load().await.unwrap().unwrap()), merged);
    put(&journal.journal, &received).await.unwrap();
    let mut final_state: agent::Checkpoint = serde_json::from_value(received).unwrap();
    final_state.journal.committed().unwrap();
    final_state.turn += 1;
    final_state.tool_calls += 1;
    let mut final_state = json!(final_state);
    final_state["repair"]["tasks"]["last_committed_turn"] = final_state["turn"].clone();
    final_state["repair"]["tasks"]["entries"]["a"]["committed_turns"] = json!(4);
    for (a_spent, b_spent, last_turn) in [
        (3, 2, final_state["turn"].clone()),
        (3, 3, final_state["turn"].clone()),
        (4, 2, merged["turn"].clone()),
    ] {
        let mut invalid = final_state.clone();
        invalid["repair"]["tasks"]["entries"]["a"]["committed_turns"] = json!(a_spent);
        invalid["repair"]["tasks"]["entries"]["b"]["committed_turns"] = json!(b_spent);
        invalid["repair"]["tasks"]["last_committed_turn"] = last_turn;
        let error = put(&journal.journal, &invalid).await.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("committed batch repair task charge"),
            "{error}"
        );
    }
    put(&journal.journal, &final_state).await.unwrap();
    put(&journal.journal, &final_state).await.unwrap();
    assert_eq!(json!(journal.load().await.unwrap().unwrap()), final_state);
    assert_eq!(
        final_state["repair"]["tasks"]["entries"]["a"]["committed_turns"],
        json!(4)
    );
    // If the charged canonical task retires in this same commit, both its
    // history and the winning ledger retain the charged turn.
    let mut retiring: agent::Checkpoint = serde_json::from_value(final_state).unwrap();
    retiring
        .journal
        .prepare(retiring.turn, "main", &body)
        .unwrap();
    journal.journal.reserve(&retiring, &body).await.unwrap();
    retiring.journal.responded(response).unwrap();
    journal.journal.save(&retiring, &json!({})).await.unwrap();
    retiring.journal.committed().unwrap();
    retiring.turn += 1;
    retiring.tool_calls += 1;
    let mut retiring = json!(retiring);
    retiring["repair"]["tasks"]["last_committed_turn"] = retiring["turn"].clone();
    retiring["repair"]["tasks"]["active"] = json!("b");
    retiring["repair"]["tasks"]["aliases"] = json!({"a":"b","b":"b"});
    retiring["repair"]["tasks"]["entries"]["a"]["committed_turns"] = json!(5);
    retiring["repair"]["tasks"]["entries"]["b"]["committed_turns"] = json!(5);
    for missed in ["a", "b"] {
        let mut invalid = retiring.clone();
        invalid["repair"]["tasks"]["entries"][missed]["committed_turns"] = json!(4);
        let error = put(&journal.journal, &invalid).await.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("committed batch repair task charge"),
            "{error}"
        );
    }
    put(&journal.journal, &retiring).await.unwrap();
    put(&journal.journal, &retiring).await.unwrap();
    assert_eq!(json!(journal.load().await.unwrap().unwrap()), retiring);
    pool.close().await;
}
