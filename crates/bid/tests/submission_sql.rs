//! Submission manifest SQL contract tests against a migrated V1 database.

use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::io::Cursor;
use uuid::Uuid;

struct SubmissionSeed {
    project_id: Uuid,
    actor: String,
}

async fn connect_test_pool() -> Result<PgPool, sqlx::Error> {
    let database_url =
        storage::database_url().map_err(|error| sqlx::Error::Configuration(Box::new(error)))?;
    PgPoolOptions::new()
        .max_connections(4)
        .connect(&database_url)
        .await
}

async fn live_test_pool() -> Option<PgPool> {
    match connect_test_pool().await {
        Ok(pool) => Some(pool),
        Err(error) if std::env::var_os("DATABASE_URL").is_some() => {
            panic!("connect live Submission contract database: {error}")
        }
        Err(_) => {
            eprintln!("skipped live Submission contract: database unavailable");
            None
        }
    }
}

async fn final_submission_schema_is_ready(pool: &PgPool) -> bool {
    sqlx::query_scalar(
        "SELECT to_regprocedure(
           'kb_bid_publish_submission_output(uuid,uuid,uuid,uuid,kb_object_ref,kb_sha256,bigint)'
         ) IS NOT NULL
         AND to_regprocedure(
           'kb_bid_create_submission_manifest(uuid,uuid,text,kb_actor_identity,text,bytea,kb_sha256)'
         ) IS NOT NULL
         AND to_regprocedure(
           'kb_bid_manifest_render_input(uuid,uuid)'
         ) IS NOT NULL
         AND to_regprocedure(
           'kb_bid_schedule_submission_render(uuid,uuid,uuid,kb_sha256,kb_actor_identity,text,bytea,kb_sha256)'
         ) IS NOT NULL
         AND to_regprocedure(
           'kb_bid_get_submission_render_job(uuid,uuid)'
         ) IS NOT NULL
         AND to_regprocedure(
           'kb_bid_claim_submission_render(uuid,uuid)'
         ) IS NOT NULL
         AND to_regprocedure(
           'kb_bid_heartbeat_submission_render(uuid,uuid)'
         ) IS NOT NULL
         AND to_regprocedure(
           'kb_bid_reap_submission_renders()'
         ) IS NOT NULL
         AND to_regprocedure(
           'kb_bid_upload_shot_artifact(uuid,uuid,uuid,kb_object_ref,kb_sha256,text,bigint,integer,integer,kb_actor_identity,text,bytea,kb_sha256)'
         ) IS NOT NULL
         AND to_regprocedure(
           'kb_bid_upload_attachment(uuid,uuid,uuid,text,kb_object_ref,kb_sha256,text,bigint,integer,integer,kb_actor_identity,text,bytea,kb_sha256)'
         ) IS NOT NULL",
    )
    .fetch_one(pool)
    .await
    .expect("probe final Submission schema")
}

async fn seed_project(pool: &PgPool) -> SubmissionSeed {
    let user_id = Uuid::new_v4();
    let project_id = Uuid::new_v4();
    let actor = format!("user:{user_id}");
    let mut tx = pool.begin().await.unwrap();

    sqlx::query("INSERT INTO users(id,email) VALUES($1,$2)")
        .bind(user_id)
        .bind(format!("{user_id}@submission-sql.invalid"))
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO bid_projects
         (id,title,owner_user_id,ends_at,fact_sha256,ceiling_identity_sha256,created_by)
         VALUES($1,'Submission SQL contract',$2,clock_timestamp()+interval '30 days',
           repeat('0',64),repeat('1',64),$3)",
    )
    .bind(project_id)
    .bind(user_id)
    .bind(&actor)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO bid_clause_set_identities(project_id,set_kind,revision,content_sha256,updated_at)
         VALUES($1,'pricing',0,repeat('2',64),clock_timestamp())",
    )
    .bind(project_id)
    .execute(&mut *tx)
    .await
    .unwrap();

    tx.commit().await.unwrap();
    SubmissionSeed { project_id, actor }
}

fn request_identity(payload: &Value) -> (Vec<u8>, String) {
    let bytes = serde_json::to_vec(payload).unwrap();
    let sha256 = domain::sha256_hex(&bytes);
    (bytes, sha256)
}

fn unique_png(seed: Uuid) -> Vec<u8> {
    let image = image::RgbaImage::from_raw(2, 2, seed.as_bytes().to_vec()).unwrap();
    let mut bytes = Cursor::new(Vec::new());
    image
        .write_to(&mut bytes, image::ImageFormat::Png)
        .expect("encode fixture PNG");
    bytes.into_inner()
}

async fn stage_object(
    pool: &PgPool,
    object_ref: &str,
    digest: &str,
    media_type: &str,
    byte_length: i64,
    actor: &str,
) -> Uuid {
    let staging_id = Uuid::new_v4();
    sqlx::query(
        "SELECT kb_object_upload_stage(
           $1,$2::kb_object_ref,$3::kb_sha256,$4,$5,$6::kb_actor_identity
         )",
    )
    .bind(staging_id)
    .bind(object_ref)
    .bind(digest)
    .bind(media_type)
    .bind(byte_length)
    .bind(actor)
    .execute(pool)
    .await
    .expect("stage object identity");
    staging_id
}

async fn assert_manifest_attempt_rolled_back(
    pool: &PgPool,
    manifest_id: Uuid,
    actor: &str,
    idempotency_key: &str,
) {
    let manifest_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM bid_submission_manifests WHERE id=$1")
            .bind(manifest_id)
            .fetch_one(pool)
            .await
            .unwrap();
    let reference_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM object_owner_references
          WHERE owner_kind='bid_manifest_asset' AND owner_id=$1",
    )
    .bind(manifest_id)
    .fetch_one(pool)
    .await
    .unwrap();
    let idempotency_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM idempotency_requests
          WHERE actor_identity=$1 AND operation='bid.submission.create_manifest'
            AND idempotency_key=$2",
    )
    .bind(actor)
    .bind(idempotency_key)
    .fetch_one(pool)
    .await
    .unwrap();

    assert_eq!(manifest_count, 0);
    assert_eq!(reference_count, 0);
    assert_eq!(idempotency_count, 0);
}

async fn create_manifest(pool: &PgPool, seed: &SubmissionSeed, format: &str) -> Value {
    let manifest_id = Uuid::new_v4();
    let request = json!({
        "manifest_id": manifest_id,
        "project_id": seed.project_id,
        "format": format,
    });
    let (request_bytes, request_sha256) = request_identity(&request);
    sqlx::query_scalar("SELECT kb_bid_create_submission_manifest($1,$2,$3,$4,$5,$6,$7)")
        .bind(manifest_id)
        .bind(seed.project_id)
        .bind(format)
        .bind(&seed.actor)
        .bind(format!("manifest-{manifest_id}"))
        .bind(request_bytes)
        .bind(request_sha256)
        .fetch_one(pool)
        .await
        .expect("create submission manifest")
}

async fn schedule_and_claim_render(
    pool: &PgPool,
    seed: &SubmissionSeed,
    manifest: &Value,
) -> (Uuid, Uuid, storage::bid_submission::SubmissionRenderClaim) {
    let manifest_id: Uuid = manifest["manifest_id"].as_str().unwrap().parse().unwrap();
    let manifest_sha256 = manifest["content_sha256"].as_str().unwrap();
    let render_job_id = Uuid::new_v4();
    let context = storage::bidding::MutationContext::new(
        seed.actor.clone(),
        format!("render-{render_job_id}"),
        &json!({"expected_manifest_sha256":manifest_sha256}),
    )
    .unwrap();
    let scheduled = storage::bid_submission::schedule_submission_render(
        pool,
        render_job_id,
        seed.project_id,
        manifest_id,
        manifest_sha256,
        &context,
    )
    .await
    .expect("schedule submission render");
    assert_eq!(scheduled["status"], "pending");
    let claim_token = Uuid::new_v4();
    let claim = storage::bid_submission::claim_submission_render(pool, render_job_id, claim_token)
        .await
        .expect("claim submission render")
        .expect("pending render must be claimable");
    (render_job_id, claim_token, claim)
}

async fn upload_shot_artifact(pool: &PgPool, seed: &SubmissionSeed, bytes: &[u8]) -> String {
    let shot_id = Uuid::new_v4();
    let digest = domain::sha256_hex(bytes);
    let object_ref = format!("objects/{digest}");
    let request = json!({
        "shot_artifact_id": shot_id,
        "project_id": seed.project_id,
        "object_ref": object_ref,
        "digest": digest,
        "media_type": "image/png",
        "byte_length": bytes.len(),
        "pixel_width": 2,
        "pixel_height": 2,
    });
    let (request_bytes, request_sha256) = request_identity(&request);
    let staging_id = stage_object(
        pool,
        &object_ref,
        &digest,
        "image/png",
        bytes.len() as i64,
        &seed.actor,
    )
    .await;
    let _: Value = sqlx::query_scalar(
        "SELECT kb_bid_upload_shot_artifact(
           $1,$2,$3,$4,$5,'image/png',$6,2,2,$7,$8,$9,$10)",
    )
    .bind(staging_id)
    .bind(shot_id)
    .bind(seed.project_id)
    .bind(&object_ref)
    .bind(&digest)
    .bind(bytes.len() as i64)
    .bind(&seed.actor)
    .bind(format!("shot-{shot_id}"))
    .bind(request_bytes)
    .bind(request_sha256)
    .fetch_one(pool)
    .await
    .expect("upload shot artifact identity");
    object_ref
}

#[tokio::test]
async fn upload_replay_returns_first_receipt_and_consumes_second_staging_reference() {
    let Some(pool) = live_test_pool().await else {
        return;
    };
    if !final_submission_schema_is_ready(&pool).await {
        eprintln!("skipped live Submission contract: final V1 schema unavailable");
        return;
    }
    let seed = seed_project(&pool).await;
    let bytes = unique_png(Uuid::new_v4());
    let digest = domain::sha256_hex(&bytes);
    let object_ref = format!("objects/{digest}");
    let payload = json!({
        "project_id": seed.project_id,
        "digest": digest,
        "media_type": "image/png",
        "byte_length": bytes.len(),
        "pixel_width": 2,
        "pixel_height": 2,
    });
    let (request_bytes, request_sha256) = request_identity(&payload);
    let idempotency_key = format!("shot-replay-{}", Uuid::new_v4());
    let first_id = Uuid::new_v4();
    let first_staging = stage_object(
        &pool,
        &object_ref,
        &digest,
        "image/png",
        bytes.len() as i64,
        &seed.actor,
    )
    .await;
    let first: Value = sqlx::query_scalar(
        "SELECT kb_bid_upload_shot_artifact(
           $1,$2,$3,$4,$5,'image/png',$6,2,2,$7,$8,$9,$10)",
    )
    .bind(first_staging)
    .bind(first_id)
    .bind(seed.project_id)
    .bind(&object_ref)
    .bind(&digest)
    .bind(bytes.len() as i64)
    .bind(&seed.actor)
    .bind(&idempotency_key)
    .bind(&request_bytes)
    .bind(&request_sha256)
    .fetch_one(&pool)
    .await
    .expect("first shot upload");

    let second_id = Uuid::new_v4();
    let second_staging = stage_object(
        &pool,
        &object_ref,
        &digest,
        "image/png",
        bytes.len() as i64,
        &seed.actor,
    )
    .await;
    let replay: Value = sqlx::query_scalar(
        "SELECT kb_bid_upload_shot_artifact(
           $1,$2,$3,$4,$5,'image/png',$6,2,2,$7,$8,$9,$10)",
    )
    .bind(second_staging)
    .bind(second_id)
    .bind(seed.project_id)
    .bind(&object_ref)
    .bind(&digest)
    .bind(bytes.len() as i64)
    .bind(&seed.actor)
    .bind(&idempotency_key)
    .bind(&request_bytes)
    .bind(&request_sha256)
    .fetch_one(&pool)
    .await
    .expect("idempotent shot replay");

    assert_eq!(replay, first);
    assert_eq!(replay["shot_artifact_id"], first_id.to_string());
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM bid_shot_artifacts WHERE id=ANY($1)")
        .bind(vec![first_id, second_id])
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 1);
    let staging_rows: i64 =
        sqlx::query_scalar("SELECT count(*) FROM object_upload_staging WHERE id=$1")
            .bind(second_staging)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(staging_rows, 0, "replay must consume temporary staging");
    pool.close().await;
}

#[tokio::test]
async fn rejected_upload_is_domain_zero_write_and_platform_abandon_is_tracked() {
    let Some(pool) = live_test_pool().await else {
        return;
    };
    if !final_submission_schema_is_ready(&pool).await {
        eprintln!("skipped live Submission contract: final V1 schema unavailable");
        return;
    }
    let seed = seed_project(&pool).await;
    let attachment_id = Uuid::new_v4();
    let bytes = format!("invalid attachment {attachment_id}").into_bytes();
    let digest = domain::sha256_hex(&bytes);
    let object_ref = format!("objects/{digest}");
    let staging_id = stage_object(
        &pool,
        &object_ref,
        &digest,
        "application/pdf",
        bytes.len() as i64,
        &seed.actor,
    )
    .await;
    let payload = json!({
        "project_id": seed.project_id,
        "kind": "not-a-kind",
        "digest": digest,
        "media_type": "application/pdf",
        "byte_length": bytes.len(),
        "pixel_width": null,
        "pixel_height": null,
    });
    let (request_bytes, request_sha256) = request_identity(&payload);
    let key = format!("attachment-reject-{attachment_id}");
    let result: Result<Value, sqlx::Error> = sqlx::query_scalar(
        "SELECT kb_bid_upload_attachment(
           $1,$2,$3,'not-a-kind',$4,$5,'application/pdf',$6,NULL,NULL,$7,$8,$9,$10)",
    )
    .bind(staging_id)
    .bind(attachment_id)
    .bind(seed.project_id)
    .bind(&object_ref)
    .bind(&digest)
    .bind(bytes.len() as i64)
    .bind(&seed.actor)
    .bind(&key)
    .bind(request_bytes)
    .bind(request_sha256)
    .fetch_one(&pool)
    .await;
    assert_database_error(
        result.expect_err("invalid attachment kind must reject"),
        "ATTACHMENT_KIND_INVALID",
    );

    let domain_rows: i64 =
        sqlx::query_scalar("SELECT count(*) FROM bid_procedural_attachments WHERE id=$1")
            .bind(attachment_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let final_refs: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM object_owner_references
          WHERE owner_kind='bid_attachment' AND owner_id=$1",
    )
    .bind(attachment_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let receipts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM idempotency_requests
          WHERE actor_identity=$1 AND operation='bid.attachment.upload' AND idempotency_key=$2",
    )
    .bind(&seed.actor)
    .bind(&key)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((domain_rows, final_refs, receipts), (0, 0, 0));

    assert!(
        storage::abandon_object_upload(&pool, staging_id, &seed.actor)
            .await
            .unwrap(),
        "unowned rejected upload must schedule retention"
    );
    let lifecycle: (String, i64) = sqlx::query_as(
        "SELECT registry.state,count(outbox.object_ref)
           FROM object_registry registry
           LEFT JOIN object_retention_outbox outbox USING(object_ref)
          WHERE registry.object_ref=$1
          GROUP BY registry.state",
    )
    .bind(&object_ref)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(lifecycle, ("deleting".into(), 1));
    pool.close().await;
}

async fn seed_current_part_with_markdown(
    pool: &PgPool,
    seed: &SubmissionSeed,
    part_key: &str,
    markdown: &str,
) {
    let content_id = Uuid::new_v4();
    let dependency_id = Uuid::new_v4();
    let mut tx = pool.begin().await.unwrap();

    sqlx::query(
        "INSERT INTO bid_part_content_artifacts
         (id,project_id,part_key,revision,canonical_markdown_utf8,content_sha256,created_by)
         VALUES($1,$2,$3,1,$4,
           encode(public.digest($4::bytea,'sha256'),'hex'),$5)",
    )
    .bind(content_id)
    .bind(seed.project_id)
    .bind(part_key)
    .bind(markdown.as_bytes())
    .bind(&seed.actor)
    .execute(&mut *tx)
    .await
    .unwrap();

    sqlx::query(
        "WITH input AS MATERIALIZED (
           SELECT template.slot AS template_slot,template.version AS template_version,
                  content.revision AS content_revision,content.content_sha256,
                  kb_bid_current_part_input_identities($1,$2) AS typed_input_identities,
                  clock_timestamp() AS generated_at
             FROM bid_template_contract_current template
             JOIN bid_part_content_artifacts content ON content.id=$3
            WHERE template.slot=kb_bid_template_slot($2)
         ), payload AS (
           SELECT input.*,jsonb_build_object(
             'schema_version',1,'project_id',$1::uuid,'part_key',$2::text,
             'template_slot',input.template_slot,'template_version',input.template_version,
             'input_identities',input.typed_input_identities,
             'part_content_revision',input.content_revision,
             'part_content_sha256',input.content_sha256,
             'generated_at',kb_bid_utc_json_time(input.generated_at)) AS canonical
             FROM input
         )
         INSERT INTO bid_part_dependency_artifacts
           (id,project_id,part_key,template_slot,template_version,part_content_artifact_id,
            schema_version,typed_input_identities,canonical_payload,content_sha256,generated_at)
         SELECT $4,$1,$2,payload.template_slot,payload.template_version,$3,1,
                payload.typed_input_identities,convert_to(payload.canonical::text,'UTF8'),
                encode(public.digest(convert_to(payload.canonical::text,'UTF8'),'sha256'),'hex'),
                payload.generated_at
           FROM payload",
    )
    .bind(seed.project_id)
    .bind(part_key)
    .bind(content_id)
    .bind(dependency_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO bid_current_parts
         (project_id,part_key,content_artifact_id,dependency_artifact_id,stale,stale_reason_codes)
         VALUES($1,$2,$3,$4,false,'{}')",
    )
    .bind(seed.project_id)
    .bind(part_key)
    .bind(content_id)
    .bind(dependency_id)
    .execute(&mut *tx)
    .await
    .unwrap();

    tx.commit().await.expect("part dependency contract fixture");
}

fn assert_database_error(error: sqlx::Error, expected: &str) {
    let message = error
        .as_database_error()
        .map(|error| error.message().to_string())
        .unwrap_or_else(|| error.to_string());
    assert!(
        message.contains(expected),
        "expected database error {expected}, got {message}"
    );
}

#[tokio::test]
async fn publishing_a_manifest_rejects_when_current_identity_changed() {
    let Some(pool) = live_test_pool().await else {
        return;
    };
    if !final_submission_schema_is_ready(&pool).await {
        eprintln!("skipped live Submission contract: final V1 schema unavailable");
        return;
    }
    let seed = seed_project(&pool).await;
    let manifest = create_manifest(&pool, &seed, "docx").await;
    let (render_job_id, claim_token, _) = schedule_and_claim_render(&pool, &seed, &manifest).await;

    sqlx::query(
        "UPDATE bid_clause_set_identities
            SET revision=revision+1,content_sha256=repeat('9',64),updated_at=clock_timestamp()
          WHERE project_id=$1 AND set_kind='pricing'",
    )
    .bind(seed.project_id)
    .execute(&pool)
    .await
    .unwrap();

    let output_id = Uuid::new_v4();
    let output_bytes = b"old manifest output";
    let output_sha256 = domain::sha256_hex(output_bytes);
    let output_ref = format!("objects/{output_sha256}");
    let staging_id = stage_object(
        &pool,
        &output_ref,
        &output_sha256,
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        output_bytes.len() as i64,
        &seed.actor,
    )
    .await;
    let result: Result<Value, sqlx::Error> =
        sqlx::query_scalar("SELECT kb_bid_publish_submission_output($1,$2,$3,$4,$5,$6,$7)")
            .bind(staging_id)
            .bind(output_id)
            .bind(render_job_id)
            .bind(claim_token)
            .bind(output_ref)
            .bind(output_sha256)
            .bind(output_bytes.len() as i64)
            .fetch_one(&pool)
            .await;

    assert_database_error(
        result.expect_err("old manifest must not publish"),
        "SUBMISSION_END_STATE_CHANGED",
    );
    storage::abandon_object_upload(&pool, staging_id, &seed.actor)
        .await
        .unwrap();
    pool.close().await;
}

#[tokio::test]
async fn submission_render_job_is_idempotent_fenced_and_terminally_observable() {
    let Some(pool) = live_test_pool().await else {
        return;
    };
    if !final_submission_schema_is_ready(&pool).await {
        eprintln!("skipped live Submission contract: final V1 schema unavailable");
        return;
    }
    let seed = seed_project(&pool).await;
    let manifest = create_manifest(&pool, &seed, "docx").await;
    let manifest_id: Uuid = manifest["manifest_id"].as_str().unwrap().parse().unwrap();
    let manifest_sha256 = manifest["content_sha256"].as_str().unwrap();
    let render_job_id = Uuid::new_v4();
    let replay_candidate = Uuid::new_v4();
    let context = storage::bidding::MutationContext::new(
        seed.actor.clone(),
        format!("render-lifecycle-{manifest_id}"),
        &json!({"expected_manifest_sha256":manifest_sha256}),
    )
    .unwrap();

    let first = storage::bid_submission::schedule_submission_render(
        &pool,
        render_job_id,
        seed.project_id,
        manifest_id,
        manifest_sha256,
        &context,
    )
    .await
    .expect("schedule render job");
    let replay = storage::bid_submission::schedule_submission_render(
        &pool,
        replay_candidate,
        seed.project_id,
        manifest_id,
        manifest_sha256,
        &context,
    )
    .await
    .expect("replay render schedule");
    assert_eq!(first["render_job_id"], render_job_id.to_string());
    assert_eq!(replay["render_job_id"], render_job_id.to_string());
    assert_eq!(first["status"], "pending");

    let first_claim_token = Uuid::new_v4();
    let first_claim =
        storage::bid_submission::claim_submission_render(&pool, render_job_id, first_claim_token)
            .await
            .unwrap()
            .expect("pending job must be claimable");
    assert_eq!(first_claim.attempt_count, 1);
    assert!(
        storage::bid_submission::claim_submission_render(&pool, render_job_id, Uuid::new_v4())
            .await
            .unwrap()
            .is_none(),
        "running job must reject a concurrent claim"
    );
    assert!(
        !storage::bid_submission::heartbeat_submission_render(
            &pool,
            render_job_id,
            Uuid::new_v4(),
        )
        .await
        .unwrap(),
        "wrong claim token must be fenced"
    );
    assert_eq!(
        storage::bid_submission::fail_submission_render(
            &pool,
            render_job_id,
            first_claim_token,
            "OBJECT_STORE_UNAVAILABLE",
            "temporary object store failure",
            true,
        )
        .await
        .unwrap()
        .as_deref(),
        Some("pending")
    );
    assert!(
        storage::bid_submission::pending_submission_renders(&pool)
            .await
            .unwrap()
            .contains(&render_job_id)
    );

    let second_claim_token = Uuid::new_v4();
    let second_claim =
        storage::bid_submission::claim_submission_render(&pool, render_job_id, second_claim_token)
            .await
            .unwrap()
            .expect("retryable render must become claimable again");
    assert_eq!(second_claim.attempt_count, 2);

    let output_id = Uuid::new_v4();
    let output_bytes = b"durable render output";
    let output_sha256 = domain::sha256_hex(output_bytes);
    let output_ref = format!("objects/{output_sha256}");
    let staging_id = stage_object(
        &pool,
        &output_ref,
        &output_sha256,
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        output_bytes.len() as i64,
        &seed.actor,
    )
    .await;
    let fenced: Result<Value, sqlx::Error> =
        sqlx::query_scalar("SELECT kb_bid_publish_submission_output($1,$2,$3,$4,$5,$6,$7)")
            .bind(staging_id)
            .bind(output_id)
            .bind(render_job_id)
            .bind(first_claim_token)
            .bind(&output_ref)
            .bind(&output_sha256)
            .bind(output_bytes.len() as i64)
            .fetch_one(&pool)
            .await;
    assert_database_error(
        fenced.expect_err("stale render claim must not publish"),
        "SUBMISSION_RENDER_CLAIM_LOST",
    );
    let published = storage::bid_submission::publish_submission_output(
        &pool,
        storage::bid_submission::PublishSubmissionOutput {
            staging_id,
            id: output_id,
            render_job_id,
            claim_token: second_claim_token,
            object_ref: &output_ref,
            digest: &output_sha256,
            byte_length: output_bytes.len() as i64,
        },
    )
    .await
    .expect("current claim publishes and completes atomically");
    assert_eq!(published["output_id"], output_id.to_string());
    let completed =
        storage::bid_submission::get_submission_render_job(&pool, seed.project_id, render_job_id)
            .await
            .unwrap()
            .expect("completed render job remains observable");
    assert_eq!(completed["status"], "completed");
    assert_eq!(completed["output_id"], output_id.to_string());

    let failed_manifest = create_manifest(&pool, &seed, "docx").await;
    let (failed_job_id, failed_claim_token, _) =
        schedule_and_claim_render(&pool, &seed, &failed_manifest).await;
    assert_eq!(
        storage::bid_submission::fail_submission_render(
            &pool,
            failed_job_id,
            failed_claim_token,
            "SUBMISSION_END_STATE_CHANGED",
            "manifest dependencies changed",
            false,
        )
        .await
        .unwrap()
        .as_deref(),
        Some("failed")
    );
    let failed =
        storage::bid_submission::get_submission_render_job(&pool, seed.project_id, failed_job_id)
            .await
            .unwrap()
            .expect("failed render job remains observable");
    assert_eq!(failed["status"], "failed");
    assert_eq!(failed["error_code"], "SUBMISSION_END_STATE_CHANGED");
    assert!(
        storage::bid_submission::get_submission_render_job(&pool, Uuid::new_v4(), failed_job_id,)
            .await
            .unwrap()
            .is_none(),
        "render job lookup must stay project scoped"
    );

    let exhausted_manifest = create_manifest(&pool, &seed, "docx").await;
    let exhausted_manifest_id: Uuid = exhausted_manifest["manifest_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let exhausted_manifest_sha256 = exhausted_manifest["content_sha256"].as_str().unwrap();
    let exhausted_job_id = Uuid::new_v4();
    let exhausted_context = storage::bidding::MutationContext::new(
        seed.actor.clone(),
        format!("render-exhausted-{exhausted_manifest_id}"),
        &json!({"expected_manifest_sha256":exhausted_manifest_sha256}),
    )
    .unwrap();
    storage::bid_submission::schedule_submission_render(
        &pool,
        exhausted_job_id,
        seed.project_id,
        exhausted_manifest_id,
        exhausted_manifest_sha256,
        &exhausted_context,
    )
    .await
    .expect("schedule render whose retries will exhaust");
    for attempt in 1..=4 {
        let claim_token = Uuid::new_v4();
        let claim =
            storage::bid_submission::claim_submission_render(&pool, exhausted_job_id, claim_token)
                .await
                .unwrap()
                .expect("retryable render must remain claimable before exhaustion");
        assert_eq!(claim.attempt_count, attempt);
        let status = storage::bid_submission::fail_submission_render(
            &pool,
            exhausted_job_id,
            claim_token,
            "SUBMISSION_RENDER_FAILED",
            "temporary object store timeout",
            true,
        )
        .await
        .unwrap();
        assert_eq!(
            status.as_deref(),
            Some(if attempt < 4 { "pending" } else { "failed" })
        );
    }
    let exhausted = storage::bid_submission::get_submission_render_job(
        &pool,
        seed.project_id,
        exhausted_job_id,
    )
    .await
    .unwrap()
    .expect("exhausted render job remains observable");
    assert_eq!(exhausted["status"], "failed");
    assert_eq!(exhausted["attempt_count"], 4);
    assert_eq!(exhausted["error_code"], "SUBMISSION_RENDER_FAILED");
    pool.close().await;
}

#[tokio::test]
async fn submission_render_reaper_is_concurrent_idempotent_and_fences_old_claim() {
    let Some(pool) = live_test_pool().await else {
        return;
    };
    if !final_submission_schema_is_ready(&pool).await {
        eprintln!("skipped live Submission contract: final V1 schema unavailable");
        return;
    }
    let seed = seed_project(&pool).await;
    let manifest = create_manifest(&pool, &seed, "docx").await;
    let (render_job_id, old_claim_token, first_claim) =
        schedule_and_claim_render(&pool, &seed, &manifest).await;
    assert_eq!(first_claim.attempt_count, 1);

    let fresh_manifest = create_manifest(&pool, &seed, "docx").await;
    let (fresh_job_id, fresh_claim_token, fresh_claim) =
        schedule_and_claim_render(&pool, &seed, &fresh_manifest).await;
    assert_eq!(fresh_claim.attempt_count, 1);

    let aged = sqlx::query(
        "UPDATE bid_submission_render_jobs
            SET heartbeat_at=clock_timestamp()
              - make_interval(secs => claim_lease_ms::double precision / 1000.0)
              - interval '1 second'
          WHERE id=$1 AND status='running' AND claim_token=$2",
    )
    .bind(render_job_id)
    .bind(old_claim_token)
    .execute(&pool)
    .await
    .expect("age the target render claim");
    assert_eq!(aged.rows_affected(), 1);

    let left_pool = pool.clone();
    let right_pool = pool.clone();
    let (left_reaped, right_reaped) = tokio::join!(
        storage::bid_submission::reap_submission_renders(&left_pool),
        storage::bid_submission::reap_submission_renders(&right_pool),
    );
    assert!(
        left_reaped.unwrap() + right_reaped.unwrap() >= 1,
        "one concurrent reaper must recover the expired target"
    );

    let target: (String, i32, bool, bool, Option<String>) = sqlx::query_as(
        "SELECT status,attempt_count,claim_token IS NULL,heartbeat_at IS NULL,error_code
           FROM bid_submission_render_jobs WHERE id=$1",
    )
    .bind(render_job_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        target,
        (
            "pending".into(),
            1,
            true,
            true,
            Some("CLAIM_LEASE_EXPIRED".into())
        )
    );
    let fresh: (String, i32, Option<Uuid>) = sqlx::query_as(
        "SELECT status,attempt_count,claim_token
           FROM bid_submission_render_jobs WHERE id=$1",
    )
    .bind(fresh_job_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(fresh, ("running".into(), 1, Some(fresh_claim_token)));
    assert!(
        !storage::bid_submission::heartbeat_submission_render(
            &pool,
            render_job_id,
            old_claim_token,
        )
        .await
        .unwrap(),
        "the reaped claim token must not regain the lease"
    );

    let output_id = Uuid::new_v4();
    let output_bytes = b"stale reaper output";
    let output_sha256 = domain::sha256_hex(output_bytes);
    let output_ref = format!("objects/{output_sha256}");
    let staging_id = stage_object(
        &pool,
        &output_ref,
        &output_sha256,
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        output_bytes.len() as i64,
        &seed.actor,
    )
    .await;
    let stale_publish: Result<Value, sqlx::Error> =
        sqlx::query_scalar("SELECT kb_bid_publish_submission_output($1,$2,$3,$4,$5,$6,$7)")
            .bind(staging_id)
            .bind(output_id)
            .bind(render_job_id)
            .bind(old_claim_token)
            .bind(&output_ref)
            .bind(&output_sha256)
            .bind(output_bytes.len() as i64)
            .fetch_one(&pool)
            .await;
    assert_database_error(
        stale_publish.expect_err("reaped claim must not publish"),
        "SUBMISSION_RENDER_CLAIM_LOST",
    );
    assert!(
        storage::abandon_object_upload(&pool, staging_id, &seed.actor)
            .await
            .unwrap()
    );

    let second_claim_token = Uuid::new_v4();
    let second_claim =
        storage::bid_submission::claim_submission_render(&pool, render_job_id, second_claim_token)
            .await
            .unwrap()
            .expect("reaped render must be claimable again");
    assert_eq!(second_claim.attempt_count, 2);
    assert_eq!(
        storage::bid_submission::fail_submission_render(
            &pool,
            render_job_id,
            second_claim_token,
            "TEST_COMPLETE",
            "reaper contract fixture cleanup",
            false,
        )
        .await
        .unwrap()
        .as_deref(),
        Some("failed")
    );
    assert_eq!(
        storage::bid_submission::fail_submission_render(
            &pool,
            fresh_job_id,
            fresh_claim_token,
            "TEST_COMPLETE",
            "fresh claim contract fixture cleanup",
            false,
        )
        .await
        .unwrap()
        .as_deref(),
        Some("failed")
    );
    pool.close().await;
}

#[tokio::test]
async fn manifest_freezes_global_occurrences_across_parts_and_repeated_object() {
    let Some(pool) = live_test_pool().await else {
        return;
    };
    if !final_submission_schema_is_ready(&pool).await {
        eprintln!("skipped live Submission contract: final V1 schema unavailable");
        return;
    }
    let seed = seed_project(&pool).await;
    let object_ref = upload_shot_artifact(&pool, &seed, b"shared image bytes").await;
    seed_current_part_with_markdown(
        &pool,
        &seed,
        "1",
        &format!("first {object_ref}\nrepeated {object_ref}"),
    )
    .await;
    seed_current_part_with_markdown(&pool, &seed, "4", &format!("other {object_ref}")).await;

    let manifest = create_manifest(&pool, &seed, "docx").await;
    let manifest_id: Uuid = manifest["manifest_id"].as_str().unwrap().parse().unwrap();
    let input: Value = sqlx::query_scalar("SELECT kb_bid_manifest_render_input($1,$2)")
        .bind(seed.project_id)
        .bind(manifest_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let assets = input["assets"].as_array().unwrap();

    assert_eq!(assets.len(), 3);
    assert_eq!(
        assets
            .iter()
            .map(|asset| asset["manifest_ordinal"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    assert!(
        assets
            .iter()
            .all(|asset| asset["digest"] == assets[0]["digest"])
    );
    assert_eq!(
        assets[0]["source_locator"],
        json!({"part_key":"1","occurrence":0})
    );
    assert_eq!(
        assets[1]["source_locator"],
        json!({"part_key":"1","occurrence":1})
    );
    assert_eq!(
        assets[2]["source_locator"],
        json!({"part_key":"4","occurrence":0})
    );
    pool.close().await;
}

#[tokio::test]
async fn docx_without_eligible_quote_freezes_fixed_placeholder() {
    let Some(pool) = live_test_pool().await else {
        return;
    };
    if !final_submission_schema_is_ready(&pool).await {
        eprintln!("skipped live Submission contract: final V1 schema unavailable");
        return;
    }
    let seed = seed_project(&pool).await;
    let manifest = create_manifest(&pool, &seed, "docx").await;
    let manifest_id: Uuid = manifest["manifest_id"].as_str().unwrap().parse().unwrap();
    let input: Value = sqlx::query_scalar("SELECT kb_bid_manifest_render_input($1,$2)")
        .bind(seed.project_id)
        .bind(manifest_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let quote = input["parts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|part| part["part_key"] == "6:quote")
        .expect("quote part is required");

    assert_eq!(manifest["gate_status"], "warning");
    assert_eq!(quote["is_placeholder"], true);
    assert_eq!(quote["markdown"], "> [报价尚未最终确认]");
    assert_eq!(
        quote["content_sha256"],
        domain::sha256_hex("> [报价尚未最终确认]".as_bytes())
    );
    pool.close().await;
}

#[tokio::test]
async fn attachment_validation_rejects_unavailable_frozen_object_identity() {
    let Some(pool) = live_test_pool().await else {
        return;
    };
    if !final_submission_schema_is_ready(&pool).await {
        eprintln!("skipped live Submission contract: final V1 schema unavailable");
        return;
    }
    let seed = seed_project(&pool).await;
    let attachment_id = Uuid::new_v4();
    let attachment_bytes = format!("attachment bytes {attachment_id}").into_bytes();
    let digest = domain::sha256_hex(&attachment_bytes);
    let object_ref = format!("objects/{digest}");
    let upload = json!({
        "attachment_id": attachment_id,
        "project_id": seed.project_id,
        "kind": "bid_bond",
        "object_ref": object_ref,
        "digest": digest,
        "media_type": "application/pdf",
        "byte_length": attachment_bytes.len(),
    });
    let (upload_bytes, upload_sha256) = request_identity(&upload);
    let staging_id = stage_object(
        &pool,
        &object_ref,
        &digest,
        "application/pdf",
        attachment_bytes.len() as i64,
        &seed.actor,
    )
    .await;
    let _: Value = sqlx::query_scalar(
        "SELECT kb_bid_upload_attachment(
           $1,$2,$3,'bid_bond',$4,$5,'application/pdf',$6,NULL,NULL,$7,$8,$9,$10)",
    )
    .bind(staging_id)
    .bind(attachment_id)
    .bind(seed.project_id)
    .bind(&object_ref)
    .bind(&digest)
    .bind(attachment_bytes.len() as i64)
    .bind(&seed.actor)
    .bind(format!("attachment-{attachment_id}"))
    .bind(upload_bytes)
    .bind(upload_sha256)
    .fetch_one(&pool)
    .await
    .expect("upload attachment identity");

    let removed: bool =
        sqlx::query_scalar("SELECT kb_object_reference_remove($1,'bid_attachment',$2,'original')")
            .bind(&object_ref)
            .bind(attachment_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(removed, "fixture must make the frozen object unavailable");

    let validate = json!({
        "attachment_id": attachment_id,
        "action": "validate",
        "expected_revision": 1,
    });
    let (validate_bytes, validate_sha256) = request_identity(&validate);
    let result: Result<Value, sqlx::Error> =
        sqlx::query_scalar("SELECT kb_bid_mutate_attachment($1,$2,'validate',1,NULL,$3,$4,$5,$6)")
            .bind(seed.project_id)
            .bind(attachment_id)
            .bind(&seed.actor)
            .bind(format!("validate-{attachment_id}"))
            .bind(validate_bytes)
            .bind(validate_sha256)
            .fetch_one(&pool)
            .await;

    assert_database_error(
        result.expect_err("unavailable attachment identity must not validate"),
        "ATTACHMENT_VALIDATION_IDENTITY_MISMATCH",
    );
    pool.close().await;
}

#[tokio::test]
async fn manifest_creation_rolls_back_for_missing_and_corrupt_physical_blob() {
    let Some(pool) = live_test_pool().await else {
        return;
    };
    if !final_submission_schema_is_ready(&pool).await {
        eprintln!("skipped live Submission contract: final V1 schema unavailable");
        return;
    }
    let seed = seed_project(&pool).await;
    let expected_bytes = unique_png(Uuid::new_v4());
    let object_ref = upload_shot_artifact(&pool, &seed, &expected_bytes).await;
    let digest = object_ref.strip_prefix("objects/").unwrap();
    assert!(
        !storage::blob_exists(digest),
        "unique missing-blob fixture must not already exist"
    );
    seed_current_part_with_markdown(&pool, &seed, "1", &object_ref).await;

    let missing_manifest_id = Uuid::new_v4();
    let missing_key = format!("missing-blob-{missing_manifest_id}");
    let missing_context = storage::bidding::MutationContext::new(
        seed.actor.clone(),
        missing_key.clone(),
        &json!({"manifest_id":missing_manifest_id,"project_id":seed.project_id,"format":"docx"}),
    )
    .unwrap();
    let missing_error = storage::bid_submission::create_submission_manifest(
        &pool,
        missing_manifest_id,
        seed.project_id,
        "docx",
        &missing_context,
    )
    .await
    .expect_err("missing physical blob must reject manifest creation");
    assert_database_error(missing_error, "MANIFEST_ASSET_BYTES_MISSING");
    assert_manifest_attempt_rolled_back(&pool, missing_manifest_id, &seed.actor, &missing_key)
        .await;

    std::fs::create_dir_all(storage::object_dir()).unwrap();
    std::fs::write(storage::blob_path(digest), unique_png(Uuid::new_v4())).unwrap();
    let corrupt_manifest_id = Uuid::new_v4();
    let corrupt_key = format!("corrupt-blob-{corrupt_manifest_id}");
    let corrupt_context = storage::bidding::MutationContext::new(
        seed.actor.clone(),
        corrupt_key.clone(),
        &json!({"manifest_id":corrupt_manifest_id,"project_id":seed.project_id,"format":"docx"}),
    )
    .unwrap();
    let corrupt_error = storage::bid_submission::create_submission_manifest(
        &pool,
        corrupt_manifest_id,
        seed.project_id,
        "docx",
        &corrupt_context,
    )
    .await
    .expect_err("corrupt physical blob must reject manifest creation");
    let _ = std::fs::remove_file(storage::blob_path(digest));
    assert_database_error(corrupt_error, "MANIFEST_ASSET_IDENTITY_MISMATCH");
    assert_manifest_attempt_rolled_back(&pool, corrupt_manifest_id, &seed.actor, &corrupt_key)
        .await;

    pool.close().await;
}
