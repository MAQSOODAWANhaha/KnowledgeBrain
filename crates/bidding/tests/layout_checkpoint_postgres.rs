//! Storage-only layout contract: synthetic typed identities use the real claim
//! and owner lock. This does not prove a product create route or editor port.
#[allow(dead_code)]
mod support;

use bidding::docx_layout::LayoutCheckpoint;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

struct Fixture {
    id: Uuid,
    sha: String,
    attempt: i32,
    token: Uuid,
    state: Value,
}

async fn seed(pool: &PgPool, kind: &str) -> Fixture {
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('kb_test.layout_kind',$1,true)")
        .bind(kind)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::raw_sql(r#"
DO $$
DECLARE owner_id uuid:=gen_random_uuid(); project_id_value uuid:=gen_random_uuid();
  actor kb_actor_identity:='user:'||owner_id::text; workspace_id_value uuid;
  request_id uuid:=gen_random_uuid(); round_id_value uuid; version_id_value uuid;
  staging_id uuid:=gen_random_uuid(); saved_docx jsonb;
  document_head bid_document_set_current%ROWTYPE; requirement_head bid_requirement_set_current%ROWTYPE;
  source_value jsonb; frozen_sha kb_sha256; request_bytes bytea:=convert_to('{}','UTF8');
  docx_sha kb_sha256:=kb_bid_v2_sha256_bytes(convert_to('synthetic saved layout source','UTF8'));
  kind text:=current_setting('kb_test.layout_kind');
BEGIN
  INSERT INTO users(id,email) VALUES(owner_id,owner_id::text||'@example.invalid');
  PERFORM kb_bid_v2_create_project(project_id_value,'layout storage contract',owner_id,
    actor,gen_random_uuid()::text,request_bytes,kb_bid_v2_sha256_bytes(request_bytes));
  SELECT id INTO STRICT workspace_id_value FROM bid_submission_workspaces WHERE project_id=project_id_value;
  SELECT * INTO STRICT document_head FROM bid_document_set_current WHERE scope_id=project_id_value;
  SELECT * INTO STRICT requirement_head FROM bid_requirement_set_current WHERE scope_id=project_id_value;
  PERFORM kb_object_upload_stage(staging_id,'objects/'||docx_sha,docx_sha,
    'application/vnd.openxmlformats-officedocument.wordprocessingml.document',octet_length('synthetic saved layout source'),actor);
  saved_docx:=kb_bid_v2_create_docx_round(workspace_id_value,staging_id,jsonb_build_object(
    'document_set_id',document_head.artifact_id,'document_set_sha256',document_head.artifact_sha256,
    'requirement_set_id',requirement_head.artifact_id,'requirement_set_sha256',requirement_head.artifact_sha256,
    'expected_version_id',NULL,'expected_docx_sha256',NULL,
    'docx_sha256',docx_sha,'byte_length',octet_length('synthetic saved layout source')),actor,gen_random_uuid()::text);
  round_id_value:=(saved_docx->>'round_id')::uuid;
  version_id_value:=(saved_docx->>'version_id')::uuid;
  source_value:=kb_bid_v2_submission_docx_source(workspace_id_value,version_id_value,docx_sha);
  frozen_sha:=kb_bid_v2_sha256_bytes(convert_to(source_value::text,'UTF8'));
  INSERT INTO bid_async_request_snapshot_artifacts(id,project_id,workspace_id,request_kind,revision,
    frozen_input_sha256,request_payload,request_sha256,status)
  VALUES(request_id,project_id_value,workspace_id_value,kind,1,frozen_sha,request_bytes,kb_bid_v2_sha256_bytes(request_bytes),'pending');
  INSERT INTO bid_submission_export_request_identities(request_artifact_id,project_id,workspace_id,request_kind,
    request_revision,request_sha256,frozen_input_sha256,round_id,version_id,docx_sha256,source)
  VALUES(request_id,project_id_value,workspace_id_value,kind,1,kb_bid_v2_sha256_bytes(request_bytes),
    frozen_sha,round_id_value,version_id_value,docx_sha,source_value);
  PERFORM set_config('kb_test.layout_request',jsonb_build_object('id',request_id,'sha',frozen_sha,'docx_sha',docx_sha)::text,true);
END $$;
"#).execute(&mut *tx).await.unwrap();
    let identity: Value =
        sqlx::query_scalar("SELECT current_setting('kb_test.layout_request')::jsonb")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    tx.commit().await.unwrap();
    let id = Uuid::parse_str(identity["id"].as_str().unwrap()).unwrap();
    let sha = identity["sha"].as_str().unwrap().to_owned();
    let claim: Value =
        sqlx::query_scalar("SELECT kb_bid_v2_tender_agent_claim($1,1,$2::kb_sha256)")
            .bind(id)
            .bind(&sha)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(claim["disposition"], "claimed", "real claim for {kind}");
    Fixture {
        id,
        sha,
        attempt: claim["attempt"].as_i64().unwrap() as i32,
        token: Uuid::parse_str(claim["execution_owner_token"].as_str().unwrap()).unwrap(),
        state: json!(LayoutCheckpoint::start(
            identity["docx_sha"].as_str().unwrap().into(),
            3
        )),
    }
}

async fn put(pool: &PgPool, f: &Fixture, state: &Value, token: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT kb_bid_v2_layout_checkpoint_put($1,$2::kb_sha256,$3,$4,$5)")
        .bind(f.id)
        .bind(&f.sha)
        .bind(f.attempt)
        .bind(token)
        .bind(state)
        .execute(pool)
        .await
        .map(|_| ())
}

async fn latest(pool: &PgPool, f: &Fixture) -> Value {
    sqlx::query_scalar("SELECT kb_bid_v2_layout_checkpoint_get($1,$2::kb_sha256)")
        .bind(f.id)
        .bind(&f.sha)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "requires KNOWLEDGEBRAIN_TEST_DATABASE_URL for an isolated fresh baseline"]
async fn layout_checkpoints_append_replay_and_fence_owner_kind_budget_and_terminal_state() {
    let pool = support::connect_postgres_contract("layout checkpoint")
        .await
        .unwrap();
    let f = seed(&pool, "docx_layout").await;
    sqlx::query("SELECT kb_bid_v2_tender_agent_heartbeat($1,$2::kb_sha256,$3,$4)")
        .bind(f.id)
        .bind(&f.sha)
        .bind(f.attempt)
        .bind(f.token)
        .execute(&pool)
        .await
        .unwrap();
    let wrong_owner = put(&pool, &f, &f.state, Uuid::new_v4()).await.unwrap_err();
    assert!(
        wrong_owner
            .to_string()
            .contains("REQUEST_ATTEMPT_SUPERSEDED")
    );
    put(&pool, &f, &f.state, f.token).await.unwrap();
    put(&pool, &f, &f.state, f.token).await.unwrap();
    assert_eq!(latest(&pool, &f).await, f.state);
    let mut next = f.state.clone();
    next["revision"] = json!(2);
    next["iteration"] = json!(1);
    put(&pool, &f, &next, f.token).await.unwrap();
    // A delayed identical ACK is idempotent, without rewinding latest.
    put(&pool, &f, &f.state, f.token).await.unwrap();
    assert_eq!(latest(&pool, &f).await, next);
    for (field, value) in [
        ("revision", json!(1)),
        ("revision", json!(4)),
        ("max_iterations", json!(4)),
        ("max_iterations", json!(2)),
        ("iteration", json!(0)),
        ("iteration", json!(4)),
        ("baseline_sha256", json!("0".repeat(64))),
    ] {
        let mut invalid = next.clone();
        invalid["revision"] = json!(3);
        invalid[field] = value;
        assert!(
            put(&pool, &f, &invalid, f.token).await.is_err(),
            "must reject {field}: {invalid}"
        );
        assert_eq!(latest(&pool, &f).await, next);
    }
    let mut divergent = next.clone();
    divergent["diagnosis"] = json!("different bytes at existing revision");
    assert!(put(&pool, &f, &divergent, f.token).await.is_err());
    let mut terminal = next.clone();
    terminal["revision"] = json!(3);
    terminal["state"] = json!("failed");
    terminal["diagnosis"] = json!("synthetic terminal diagnosis");
    put(&pool, &f, &terminal, f.token).await.unwrap();
    put(&pool, &f, &terminal, f.token).await.unwrap();
    let mut reopened = terminal.clone();
    reopened["revision"] = json!(4);
    reopened["state"] = json!("measure");
    assert!(put(&pool, &f, &reopened, f.token).await.is_err());
    let (count, contracts): (i64, bool) = sqlx::query_as("SELECT count(*),bool_and(contract_sha256=$2::kb_sha256) FROM bid_tender_agent_checkpoint_artifacts WHERE request_artifact_id=$1 AND stage_kind='layout_checkpoint'")
        .bind(f.id).bind(&f.sha).fetch_one(&pool).await.unwrap();
    assert_eq!(count, 3, "append only, identical retries add no rows");
    assert!(contracts, "each revision binds the frozen input");
    assert_eq!(latest(&pool, &f).await, terminal);
    let other = seed(&pool, "submission_export").await;
    let wrong_kind = put(&pool, &other, &other.state, other.token)
        .await
        .unwrap_err();
    assert!(
        wrong_kind
            .to_string()
            .contains("layout checkpoint identity")
    );
    // The shared owner lifecycle does not authorize the other stage's writes.
    let export_state = json!({
        "journal":{"sequence":1,"pending":null,"session":null},
        "contract_sha256":f.sha,"inventory":{"docx_sha256":f.state["baseline_sha256"]},
        "analysis":{},"tender_coverage":{},"output_coverage":{},"reviews":{},
        "turn":0,"tool_calls":0,"read_bytes":0,"transcript":[],"progress":{},"done":false
    });
    let cross_stage =
        sqlx::query("SELECT kb_bid_v2_export_review_checkpoint_put($1,$2::kb_sha256,$3,$4,$5)")
            .bind(f.id)
            .bind(&f.sha)
            .bind(f.attempt)
            .bind(f.token)
            .bind(export_state)
            .execute(&pool)
            .await
            .unwrap_err();
    assert!(
        cross_stage
            .to_string()
            .contains("export-review checkpoint identity")
    );
    let body = serde_json_canonicalizer::to_vec(&json!({
        "model":"scripted-layout-fence","stream":true,"stream_options":{"include_usage":true},
        "max_tokens":1,"tool_choice":"required","tools":[],
        "messages":[{"role":"system","content":"synthetic kind fence"}]
    }))
    .unwrap();
    let cross_reserve = sqlx::query(
        "SELECT kb_bid_v2_export_review_reserve($1,$2::kb_sha256,$3,$4,0,$2::kb_sha256,$5)",
    )
    .bind(f.id)
    .bind(&f.sha)
    .bind(f.attempt)
    .bind(f.token)
    .bind(body)
    .execute(&pool)
    .await
    .unwrap_err();
    assert!(
        cross_reserve
            .to_string()
            .contains("export-review turn or contract changed")
    );
}
