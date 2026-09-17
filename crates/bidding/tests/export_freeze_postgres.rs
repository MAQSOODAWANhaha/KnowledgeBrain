//! Real storage and owner boundaries with synthetic file identities. No model or
//! parser quality claim is made by these persistence fixtures.
#[allow(dead_code)]
mod support;

use bidding::export_review::{FrozenExecution, agent::Config};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

struct Fixture {
    id: Uuid,
    sha: String,
    attempt: i32,
    token: Uuid,
    source: Value,
    inventory: Value,
    render: Value,
    pdf_stage: Uuid,
}

fn config() -> Config {
    serde_json::from_value(json!({
        "provider":{"schema_version":1,"base_url":"https://example.invalid/v1","endpoint":"https://example.invalid/v1/chat/completions","protocol":"openai_chat_completions_sse","model_id":"storage-fixture","credential_ref":"env:LLM_API_KEY","stream":true,"max_tokens":8192,"timeout_ms":180000,"response_mode":"tool_calls","transport_retries":0,"temperature":null,"reasoning_effort":null},
        "limits":{"max_turns":8,"max_tool_calls":20,"max_physical_calls":20,"max_read_bytes":200000,"max_context_bytes":200000,"max_tool_result_bytes":20000}
    })).unwrap()
}

async fn seed(pool: &PgPool, execution: Option<&FrozenExecution>) -> Fixture {
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('kb_test.export_context',$1,true)")
        .bind(
            json!({"analysis_identity":null,"execution_contract":execution,"layout_result":null})
                .to_string(),
        )
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::raw_sql(r#"
DO $$
DECLARE user_id uuid:=gen_random_uuid(); project_value uuid:=gen_random_uuid(); workspace_value uuid;
 actor kb_actor_identity:='user:'||user_id::text; stage uuid:=gen_random_uuid(); saved jsonb; request_value jsonb;
 doc_head bid_document_set_current%ROWTYPE; req_head bid_requirement_set_current%ROWTYPE;
 body bytea:=convert_to('{}','UTF8'); docx_sha kb_sha256:=kb_bid_v2_sha256_bytes(convert_to(user_id::text,'UTF8'));
BEGIN
 INSERT INTO users(id,email) VALUES(user_id,user_id::text||'@example.invalid');
 PERFORM kb_bid_v2_create_project(project_value,'export storage contract',user_id,actor,gen_random_uuid()::text,body,kb_bid_v2_sha256_bytes(body));
 SELECT id INTO STRICT workspace_value FROM bid_submission_workspaces WHERE project_id=project_value;
 SELECT * INTO STRICT doc_head FROM bid_document_set_current WHERE scope_id=project_value;
 SELECT * INTO STRICT req_head FROM bid_requirement_set_current WHERE scope_id=project_value;
 PERFORM kb_object_upload_stage(stage,'objects/'||docx_sha,docx_sha,'application/vnd.openxmlformats-officedocument.wordprocessingml.document',14,actor);
 saved:=kb_bid_v2_create_docx_round(workspace_value,stage,jsonb_build_object(
   'document_set_id',doc_head.artifact_id,'document_set_sha256',doc_head.artifact_sha256,
   'requirement_set_id',req_head.artifact_id,'requirement_set_sha256',req_head.artifact_sha256,
   'expected_version_id',NULL,'expected_docx_sha256',NULL,'docx_sha256',docx_sha,'byte_length',14),actor,gen_random_uuid()::text);
 request_value:=kb_bid_v2_create_submission_export_request(workspace_value,(saved->>'version_id')::uuid,docx_sha,actor,gen_random_uuid()::text,body,kb_bid_v2_sha256_bytes(body),current_setting('kb_test.export_context')::jsonb);
 PERFORM set_config('kb_test.export_request',request_value::text,true);
END $$;
"#).execute(&mut *tx).await.unwrap();
    let request: Value =
        sqlx::query_scalar("SELECT current_setting('kb_test.export_request')::jsonb")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    tx.commit().await.unwrap();
    let id = Uuid::parse_str(request["request_artifact_id"].as_str().unwrap()).unwrap();
    let sha = request["frozen_input_sha256"].as_str().unwrap().to_owned();
    assert_eq!(
        request["frozen_context"]["execution_contract"],
        json!(execution)
    );
    let claim: Value =
        sqlx::query_scalar("SELECT kb_bid_v2_tender_agent_claim($1,1,$2::kb_sha256)")
            .bind(id)
            .bind(&sha)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(claim["disposition"], "claimed");
    let other: Value =
        sqlx::query_scalar("SELECT kb_bid_v2_tender_agent_claim($1,1,$2::kb_sha256)")
            .bind(id)
            .bind(&sha)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(other["disposition"], "live_owner");
    let source = request["source"].clone();
    let pdf_sha = bidding::tender_analysis::digest(&json!(Uuid::new_v4().to_string())).unwrap();
    let pdf_stage = Uuid::new_v4();
    sqlx::query("SELECT kb_object_upload_stage($1,$2::kb_object_ref,$3::kb_sha256,'application/pdf',15,'system:submission-export-v2')")
        .bind(pdf_stage).bind(format!("objects/{pdf_sha}")).bind(&pdf_sha).execute(pool).await.unwrap();
    let render = json!({"schema_version":1,"source":source,"pdf":{"object_ref":format!("objects/{pdf_sha}"),"sha256":pdf_sha,"media_type":"application/pdf","byte_length":15}});
    let inventory = json!({"docx_sha256":source["docx_sha256"],"pdf_sha256":pdf_sha,"units":[],"images":{},"parser_manifests":[
        {"schema_version":1,"profile":"output_inventory_v1","file_sha256":source["docx_sha256"],"parser":"storage-fixture","config":{}},
        {"schema_version":1,"profile":"output_inventory_v1","file_sha256":pdf_sha,"parser":"storage-fixture","config":{}}
    ]});
    Fixture {
        id,
        sha,
        attempt: claim["attempt"].as_i64().unwrap() as i32,
        token: claim["execution_owner_token"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap(),
        source,
        inventory,
        render,
        pdf_stage,
    }
}

async fn render_put(
    pool: &PgPool,
    f: &Fixture,
    value: &Value,
    token: Uuid,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_bid_v2_submission_export_render_put($1,$2::kb_sha256,$3,$4,$5,$6)",
    )
    .bind(f.id)
    .bind(&f.sha)
    .bind(f.attempt)
    .bind(token)
    .bind(f.pdf_stage)
    .bind(value)
    .fetch_one(pool)
    .await
}
async fn snapshot_put(pool: &PgPool, f: &Fixture, inventory: &Value) -> Result<Value, sqlx::Error> {
    snapshot_put_stages(pool, f, inventory, &json!({})).await
}
async fn snapshot_put_stages(
    pool: &PgPool,
    f: &Fixture,
    inventory: &Value,
    stages: &Value,
) -> Result<Value, sqlx::Error> {
    let value = json!({"schema_version":2,"inventory":inventory,"inventory_sha256":bidding::tender_analysis::digest(inventory).unwrap()});
    sqlx::query_scalar(
        "SELECT kb_bid_v2_submission_export_snapshot_put($1,$2::kb_sha256,$3,$4,$5,$6)",
    )
    .bind(f.id)
    .bind(&f.sha)
    .bind(f.attempt)
    .bind(f.token)
    .bind(value)
    .bind(stages)
    .fetch_one(pool)
    .await
}
async fn checkpoint_put(pool: &PgPool, f: &Fixture, state: &Value) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT kb_bid_v2_export_review_checkpoint_put($1,$2::kb_sha256,$3,$4,$5)")
        .bind(f.id)
        .bind(&f.sha)
        .bind(f.attempt)
        .bind(f.token)
        .bind(state)
        .execute(pool)
        .await
        .map(|_| ())
}
async fn reserve(
    pool: &PgPool,
    f: &Fixture,
    contract: &str,
    body: &Value,
) -> Result<i64, sqlx::Error> {
    let bytes = serde_json_canonicalizer::to_vec(body).unwrap();
    sqlx::query_scalar(
        "SELECT kb_bid_v2_export_review_reserve($1,$2::kb_sha256,$3,$4,0,$5::kb_sha256,$6)",
    )
    .bind(f.id)
    .bind(&f.sha)
    .bind(f.attempt)
    .bind(f.token)
    .bind(contract)
    .bind(bytes)
    .fetch_one(pool)
    .await
}

#[tokio::test]
#[ignore = "requires an isolated fresh PostgreSQL baseline"]
async fn export_freezes_execution_pdf_inventory_and_three_journal_boundaries() {
    let pool = support::connect_postgres_contract("export frozen execution")
        .await
        .unwrap();
    let frozen = FrozenExecution::freeze(&config()).unwrap();
    let f = seed(&pool, Some(&frozen)).await;
    assert!(
        render_put(&pool, &f, &f.render, Uuid::new_v4())
            .await
            .unwrap_err()
            .to_string()
            .contains("REQUEST_ATTEMPT_SUPERSEDED")
    );
    assert_eq!(
        render_put(&pool, &f, &f.render, f.token).await.unwrap(),
        f.render
    );
    assert_eq!(
        render_put(&pool, &f, &f.render, f.token).await.unwrap(),
        f.render
    );
    let mut changed = f.render.clone();
    changed["pdf"]["byte_length"] = json!(99);
    assert!(
        render_put(&pool, &f, &changed, f.token)
            .await
            .unwrap_err()
            .to_string()
            .contains("PDF already frozen")
    );
    let snapshot = snapshot_put(&pool, &f, &f.inventory).await.unwrap();
    assert_eq!(
        snapshot_put(&pool, &f, &f.inventory).await.unwrap(),
        snapshot
    );
    let mut changed = f.inventory.clone();
    changed["units"] = json!([{"id":"extra"}]);
    assert!(
        snapshot_put(&pool, &f, &changed)
            .await
            .unwrap_err()
            .to_string()
            .contains("inventory already frozen")
    );
    let loaded: Value =
        sqlx::query_scalar("SELECT kb_bid_v2_load_submission_export_input($1,1,$2::kb_sha256)")
            .bind(f.id)
            .bind(&f.sha)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(loaded["render"], f.render);
    assert_eq!(loaded["render_snapshot"], snapshot);
    let body = json!({"model":config().provider.model_id,"stream":true,"stream_options":{"include_usage":true},"max_tokens":8192,"tool_choice":"required","tools":frozen.definition["tools"],"messages":[{"role":"system","content":frozen.definition["reviewer"]}]});
    let mut changed = body.clone();
    changed["model"] = json!("replacement");
    assert!(
        reserve(&pool, &f, &frozen.contract_sha256, &changed)
            .await
            .unwrap_err()
            .to_string()
            .contains("provider contract changed")
    );
    let mut state = json!({"journal":{"sequence":1,"pending":null,"session":null},"contract_sha256":frozen.contract_sha256,"inventory":f.inventory,"analysis":{},"obligations":{},"pending_delivery":null,
        "tender_coverage":bidding::tender_analysis::Coverage::default(),"output_coverage":{"units":{},"views":{}},"reviews":{},"turn":0,"tool_calls":0,"read_bytes":0,"transcript":[],"progress":bidding::agent_runtime::progress::Progress::default(),"done":true});
    assert!(
        checkpoint_put(&pool, &f, &state)
            .await
            .unwrap_err()
            .to_string()
            .contains("reserved exact request")
    );
    assert_eq!(
        reserve(&pool, &f, &frozen.contract_sha256, &body)
            .await
            .unwrap(),
        1
    );
    state["done"] = json!(false);
    state["journal"]["session"] = json!({"run":{},"prefix":2,"suffix":0});
    state["journal"]["pending"] = json!({"turn":0,"role":"reviewer","body":String::from_utf8(serde_json_canonicalizer::to_vec(&body).unwrap()).unwrap(),"response":null});
    checkpoint_put(&pool, &f, &state).await.unwrap();
    checkpoint_put(&pool, &f, &state).await.unwrap();
    let mut forged = state.clone();
    forged["journal"]["sequence"] = json!(2);
    forged["done"] = json!(true);
    forged["journal"]["pending"] = Value::Null;
    assert!(
        checkpoint_put(&pool, &f, &forged)
            .await
            .unwrap_err()
            .to_string()
            .contains("response boundary")
    );
    state["journal"]["sequence"] = json!(2);
    state["journal"]["pending"]["response"] = json!({"content":"","tool_calls":[{"id":"c0","name":"inspect_progress","arguments":"{}"}],"finish_reason":"tool_calls","usage":null});
    checkpoint_put(&pool, &f, &state).await.unwrap();
    assert!(
        reserve(&pool, &f, &frozen.contract_sha256, &body)
            .await
            .unwrap_err()
            .to_string()
            .contains("turn or contract changed")
    );
    state["journal"]["sequence"] = json!(3);
    state["journal"]["pending"] = Value::Null;
    state["turn"] = json!(1);
    state["tool_calls"] = json!(1);
    checkpoint_put(&pool, &f, &state).await.unwrap();
}

#[tokio::test]
#[ignore = "requires an isolated fresh PostgreSQL baseline"]
async fn unconfigured_manual_export_requires_owner_frozen_files_and_explicit_not_checked() {
    let pool = support::connect_postgres_contract("export publication")
        .await
        .unwrap();
    let mut f = seed(&pool, None).await;
    render_put(&pool, &f, &f.render, f.token).await.unwrap();
    // Real PNG bytes are written through the existing local object store.
    // DOCX/PDF remain synthetic storage fixtures: this is not model acceptance.
    use sha2::{Digest, Sha256};
    let random = Uuid::new_v4();
    let pixels = image::RgbImage::from_pixel(
        2,
        2,
        image::Rgb([
            random.as_bytes()[0],
            random.as_bytes()[1],
            random.as_bytes()[2],
        ]),
    );
    let mut encoded = std::io::Cursor::new(Vec::new());
    pixels
        .write_to(&mut encoded, image::ImageFormat::Png)
        .unwrap();
    let bytes = encoded.into_inner();
    let image_sha = hex::encode(Sha256::digest(&bytes));
    let image = bidding::export_review::OutputImage {
        sha256: image_sha.clone(),
        object_ref: format!("objects/{image_sha}"),
        media_type: "image/png".into(),
        byte_length: bytes.len() as u64,
        width: 2,
        height: 2,
    };
    image.validate_bytes(&bytes).unwrap();
    platform::write_blob_off_runtime(&image_sha, &bytes).unwrap();
    assert_eq!(platform::read_blob(&image_sha).unwrap(), bytes);
    let image_stage = Uuid::new_v4();
    sqlx::query("SELECT kb_object_upload_stage($1,$2::kb_object_ref,$3::kb_sha256,'image/png',$4,'system:submission-export-v2')")
        .bind(image_stage).bind(&image.object_ref).bind(&image_sha).bind(bytes.len() as i64).execute(&pool).await.unwrap();
    f.inventory["images"] = json!({image_sha.clone():image});
    f.inventory["parser_manifests"][1]["image_sha256"] = json!({"page-1":image_sha});
    f.inventory["units"] = json!([{"id":"page-1","file_sha256":f.inventory["pdf_sha256"],"image_sha256s":[image_sha]}]);
    assert!(
        snapshot_put(&pool, &f, &f.inventory)
            .await
            .unwrap_err()
            .to_string()
            .contains("image staging missing")
    );
    let mut wrong_png = std::io::Cursor::new(Vec::new());
    image::RgbImage::from_pixel(
        2,
        2,
        image::Rgb([
            random.as_bytes()[0] ^ 1,
            random.as_bytes()[1],
            random.as_bytes()[2],
        ]),
    )
    .write_to(&mut wrong_png, image::ImageFormat::Png)
    .unwrap();
    let wrong_png = wrong_png.into_inner();
    let wrong_sha = hex::encode(Sha256::digest(&wrong_png));
    platform::write_blob_off_runtime(&wrong_sha, &wrong_png).unwrap();
    let wrong_stage = Uuid::new_v4();
    sqlx::query("SELECT kb_object_upload_stage($1,$2::kb_object_ref,$3::kb_sha256,'image/png',$4,'system:submission-export-v2')")
        .bind(wrong_stage).bind(format!("objects/{wrong_sha}")).bind(&wrong_sha).bind(wrong_png.len() as i64).execute(&pool).await.unwrap();
    assert!(
        snapshot_put_stages(
            &pool,
            &f,
            &f.inventory,
            &json!({image_sha.clone():wrong_stage})
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("object upload staging owner mismatch"),
        "another real image object cannot replace the frozen pixels"
    );
    let mut wrong = f.inventory.clone();
    wrong["images"][&image_sha]["sha256"] = json!("a".repeat(64));
    assert!(
        snapshot_put_stages(&pool, &f, &wrong, &json!({image_sha.clone():image_stage}))
            .await
            .unwrap_err()
            .to_string()
            .contains("image object identity")
    );
    let snapshot = snapshot_put_stages(
        &pool,
        &f,
        &f.inventory,
        &json!({image_sha.clone():image_stage}),
    )
    .await
    .unwrap();
    assert_eq!(
        snapshot_put(&pool, &f, &f.inventory).await.unwrap(),
        snapshot,
        "replay uses the already-owned pixels without a new stage"
    );
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("DELETE FROM object_owner_references WHERE owner_kind='bid_submission_export_request' AND owner_id=$1 AND object_ref=$2")
        .bind(f.id).bind(&image.object_ref).execute(&mut *tx).await.unwrap();
    let lost = sqlx::query_scalar::<_, Value>(
        "SELECT kb_bid_v2_submission_export_snapshot_put($1,$2::kb_sha256,$3,$4,$5)",
    )
    .bind(f.id)
    .bind(&f.sha)
    .bind(f.attempt)
    .bind(f.token)
    .bind(&snapshot)
    .fetch_one(&mut *tx)
    .await
    .unwrap_err();
    assert!(lost.to_string().contains("image owner missing"));
    tx.rollback().await.unwrap();
    let docx_stage = Uuid::new_v4();
    let docx_id = Uuid::new_v4();
    let pdf_id = Uuid::new_v4();
    let package = Uuid::new_v4();
    let mime = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
    sqlx::query("SELECT kb_object_upload_stage($1,$2::kb_object_ref,$3::kb_sha256,$4,14,'system:submission-export-v2')")
        .bind(docx_stage).bind(f.source["object_ref"].as_str().unwrap()).bind(f.source["docx_sha256"].as_str().unwrap()).bind(mime).execute(&pool).await.unwrap();
    let docx = json!({"staging_id":docx_stage,"artifact_id":docx_id,"object_ref":f.source["object_ref"],"sha256":f.source["docx_sha256"],"media_type":mime,"byte_length":14});
    let mut pdf = f.render["pdf"].clone();
    pdf["staging_id"] = Value::Null;
    pdf["artifact_id"] = json!(pdf_id);
    let report = json!({"schema_version":2,"source":f.source,"outputs":{"docx":{"artifact_id":docx_id,"sha256":f.source["docx_sha256"],"byte_length":14},"pdf":{"artifact_id":pdf_id,"sha256":pdf["sha256"],"byte_length":15}},"output_images":f.inventory["images"],"output_inventory":{"inventory_sha256":snapshot["inventory_sha256"]},"export_review":null,"checks":[{"id":"export_review","status":"not_checked","detail":"No configured reviewer"}]});
    let publish = |token, report: Value| {
        let pool = &pool;
        let f = &f;
        let docx = &docx;
        let pdf = &pdf;
        async move {
            sqlx::query_scalar::<_,Value>("SELECT kb_bid_v2_publish_submission_export($1,1,$2::kb_sha256,$3,$4,$5,$6,'system:submission-export-v2',$7,$8)").bind(f.id).bind(&f.sha).bind(package).bind(docx).bind(pdf).bind(report).bind(f.attempt).bind(token).fetch_one(pool).await
        }
    };
    assert!(
        publish(Uuid::new_v4(), report.clone())
            .await
            .unwrap_err()
            .to_string()
            .contains("REQUEST_ATTEMPT_SUPERSEDED")
    );
    let mut forged = report.clone();
    forged["checks"][0]["status"] = json!("pass");
    assert!(
        publish(f.token, forged)
            .await
            .unwrap_err()
            .to_string()
            .contains("unfrozen semantic review")
    );
    let mut wrong = report.clone();
    wrong["output_inventory"]["inventory_sha256"] = json!("f".repeat(64));
    assert!(
        publish(f.token, wrong)
            .await
            .unwrap_err()
            .to_string()
            .contains("frozen files or inventory")
    );
    let mut wrong_image_report = report.clone();
    wrong_image_report["output_images"][&image_sha]["width"] = json!(99);
    assert!(
        publish(f.token, wrong_image_report)
            .await
            .unwrap_err()
            .to_string()
            .contains("frozen files or inventory")
    );
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("DELETE FROM object_owner_references WHERE owner_kind='bid_submission_export_request' AND owner_id=$1 AND object_ref=$2")
        .bind(f.id).bind(&image.object_ref).execute(&mut *tx).await.unwrap();
    let missing=sqlx::query_scalar::<_,Value>("SELECT kb_bid_v2_publish_submission_export($1,1,$2::kb_sha256,$3,$4,$5,$6,'system:submission-export-v2',$7,$8)")
        .bind(f.id).bind(&f.sha).bind(package).bind(&docx).bind(&pdf).bind(&report).bind(f.attempt).bind(f.token).fetch_one(&mut *tx).await.unwrap_err();
    assert!(missing.to_string().contains("image owner missing"));
    tx.rollback().await.unwrap();
    let published = publish(f.token, report.clone()).await.unwrap();
    assert_eq!(
        publish(Uuid::new_v4(), Value::Null).await.unwrap(),
        published,
        "lost publication ACK replays terminal result without another owner"
    );
    let status:String=sqlx::query_scalar("SELECT status FROM bid_tender_agent_run_artifacts WHERE request_artifact_id=$1 AND attempt=$2").bind(f.id).bind(f.attempt).fetch_one(&pool).await.unwrap();
    assert_eq!(status, "succeeded");
    let manifest:Value=sqlx::query_scalar("SELECT convert_from(canonical_payload,'UTF8')::jsonb FROM bid_submission_manifest_artifacts WHERE id=$1")
        .bind(package).fetch_one(&pool).await.unwrap();
    assert_eq!(manifest["output_images"], f.inventory["images"]);
    let report_id = Uuid::parse_str(published["assessment_report_id"].as_str().unwrap()).unwrap();
    let owner_count:i64=sqlx::query_scalar("SELECT count(*) FROM object_owner_references WHERE object_ref=$1 AND ((owner_kind='bid_submission_manifest' AND owner_id=$2) OR (owner_kind='bid_submission_assessment_report' AND owner_id=$3))")
        .bind(&image.object_ref).bind(package).bind(report_id).fetch_one(&pool).await.unwrap();
    assert_eq!(
        owner_count, 2,
        "both published artifacts retain their visual evidence"
    );
    let deletion:Option<Value>=sqlx::query_scalar("SELECT kb_object_reference_remove($1::kb_object_ref,'bid_submission_export_request',$2,$3,$4)")
        .bind(&image.object_ref).bind(f.id).bind(format!("inventory:image:{image_sha}")).bind(Uuid::new_v4()).fetch_one(&pool).await.unwrap();
    assert!(
        deletion.is_none(),
        "publication owners prevent evidence deletion"
    );
    assert_eq!(platform::read_blob(&image_sha).unwrap(), bytes);
    let mut wrong_bytes = bytes.clone();
    wrong_bytes[0] ^= 1;
    assert!(
        image.validate_bytes(&wrong_bytes).is_err(),
        "retrieved wrong pixels cannot satisfy frozen metadata"
    );
}
