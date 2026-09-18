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
    let owner = claim_agent_run(&pool, &request).await;
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
        let owner = claim_agent_run(&pool, &request).await;
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
        let next = claim_agent_run(&pool, &request).await;
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
            let owner = claim_agent_run(&pool, &request).await;
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
        let owner = claim_agent_run(&pool, &request).await;
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
    let owner = claim_agent_run(&pool, &request).await;
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
    let owner = claim_agent_run(&pool, &request).await;
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

async fn claim_agent_run(
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
