//! Submission manifest SQL contract tests against a migrated V1 database.

use serde_json::{Value, json};
use sqlx::{Acquire, PgPool};
use std::io::Cursor;
use uuid::Uuid;

mod support;

struct SubmissionSeed {
    project_id: Uuid,
    actor: String,
}

async fn live_test_pool() -> Option<PgPool> {
    support::connect_postgres_contract("Submission").await
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
         ) IS NOT NULL
         AND to_regprocedure(
           'kb_bid_claim_attachment_preparation(uuid,uuid)'
         ) IS NOT NULL
         AND to_regprocedure(
           'kb_bid_publish_attachment_preparation(uuid,uuid,jsonb,kb_actor_identity)'
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

struct TestBlobs(Vec<String>);

impl TestBlobs {
    fn persist(entries: &[(&str, &[u8])]) -> Self {
        std::fs::create_dir_all(storage::object_dir()).expect("create test object directory");
        for (digest, bytes) in entries {
            std::fs::write(storage::blob_path(digest), bytes).expect("persist test object bytes");
        }
        Self(
            entries
                .iter()
                .map(|(digest, _)| (*digest).to_string())
                .collect(),
        )
    }
}

impl Drop for TestBlobs {
    fn drop(&mut self) {
        for digest in &self.0 {
            let _ = std::fs::remove_file(storage::blob_path(digest));
        }
    }
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

async fn upload_pending_pdf_attachment(pool: &PgPool, seed: &SubmissionSeed) -> (Uuid, Uuid) {
    let attachment_id = Uuid::new_v4();
    let original = format!("%PDF-1.7\n% {attachment_id}\n%%EOF\n").into_bytes();
    let digest = domain::sha256_hex(&original);
    let object_ref = format!("objects/{digest}");
    let staging_id = stage_object(
        pool,
        &object_ref,
        &digest,
        "application/pdf",
        original.len() as i64,
        &seed.actor,
    )
    .await;
    let request = json!({
        "attachment_id": attachment_id,
        "project_id": seed.project_id,
        "kind": "bid_bond",
        "object_ref": object_ref,
        "digest": digest,
    });
    let (request_bytes, request_sha256) = request_identity(&request);
    let uploaded: Value = sqlx::query_scalar(
        "SELECT kb_bid_upload_attachment(
           $1,$2,$3,'bid_bond',$4,$5,'application/pdf',$6,NULL,NULL,$7,$8,$9,$10)",
    )
    .bind(staging_id)
    .bind(attachment_id)
    .bind(seed.project_id)
    .bind(&object_ref)
    .bind(&digest)
    .bind(original.len() as i64)
    .bind(&seed.actor)
    .bind(format!("upload-pending-pdf-{attachment_id}"))
    .bind(request_bytes)
    .bind(request_sha256)
    .fetch_one(pool)
    .await
    .expect("upload pending PDF attachment");
    assert_eq!(uploaded["preparation_status"], "pending");
    let preparation_job_id = uploaded["preparation_job_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    (attachment_id, preparation_job_id)
}

async fn stage_render_page(pool: &PgPool, page_ordinal: usize) -> (Uuid, Value) {
    let page = unique_png(Uuid::new_v4());
    let digest = domain::sha256_hex(&page);
    let object_ref = format!("objects/{digest}");
    let staging_id = stage_object(
        pool,
        &object_ref,
        &digest,
        "image/png",
        page.len() as i64,
        "system:bid-attachment-preparation",
    )
    .await;
    let descriptor = json!({
        "staging_id": staging_id,
        "page_ordinal": page_ordinal,
        "object_ref": object_ref,
        "digest": digest,
        "media_type": "image/png",
        "byte_length": page.len(),
        "pixel_width": 2,
        "pixel_height": 2,
    });
    (staging_id, descriptor)
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

#[tokio::test]
async fn procedural_segments_distinguish_numbered_items_from_decimal_amounts() {
    let Some(pool) = support::connect_postgres_contract("ProceduralSegmentV1").await else {
        return;
    };
    let ready: bool = sqlx::query_scalar(
        "SELECT to_regprocedure('kb_bid_split_procedural_segments(text)') IS NOT NULL",
    )
    .fetch_one(&pool)
    .await
    .expect("probe procedural segment splitter");
    if !support::require_final_schema("ProceduralSegmentV1", ready) {
        return;
    }

    let amount = "投标人应提交投标保证金 10.00 万元整，并上传缴纳回执。";
    let amount_segments: Vec<String> = sqlx::query_scalar(
        "SELECT convert_from(segment_utf8,'UTF8')
           FROM kb_bid_split_procedural_segments($1)
          ORDER BY start_offset",
    )
    .bind(amount)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(amount_segments, vec![amount]);

    let numbered = "投标材料包括 1. 提交授权委托书 2、上传保证金回执";
    let numbered_segments: Vec<String> = sqlx::query_scalar(
        "SELECT convert_from(segment_utf8,'UTF8')
           FROM kb_bid_split_procedural_segments($1)
          ORDER BY start_offset",
    )
    .bind(numbered)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        numbered_segments,
        vec!["投标材料包括", "1. 提交授权委托书", "2、上传保证金回执"]
    );

    let compact_numbered = "材料：（一）上传保证金回执（二）提交授权委托书";
    let compact_segments: Vec<String> = sqlx::query_scalar(
        "SELECT convert_from(segment_utf8,'UTF8')
           FROM kb_bid_split_procedural_segments($1)
          ORDER BY start_offset",
    )
    .bind(compact_numbered)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        compact_segments,
        vec!["材料：", "（一）上传保证金回执", "（二）提交授权委托书"]
    );
}

#[tokio::test]
async fn procedural_text_change_terminals_previous_classification_and_decision() {
    let Some(pool) = live_test_pool().await else {
        return;
    };
    if !support::require_final_schema("Submission", final_submission_schema_is_ready(&pool).await) {
        return;
    }
    let seed = seed_project(&pool).await;
    let clause_id = Uuid::new_v4();
    let create_context = storage::bidding::MutationContext::new(
        seed.actor.clone(),
        format!("procedural-text-create-{clause_id}"),
        &json!({"text":"投标函签字并盖章。","kind":"procedural","must":true}),
    )
    .unwrap();
    storage::bidding::create_clause(
        &pool,
        clause_id,
        seed.project_id,
        "投标函签字并盖章。",
        "procedural",
        true,
        &create_context,
    )
    .await
    .unwrap();
    let confirm_context = storage::bidding::MutationContext::new(
        seed.actor.clone(),
        format!("procedural-text-confirm-{clause_id}"),
        &json!({"action":"confirm","expected_revision":1}),
    )
    .unwrap();
    storage::bidding::mutate_clause(
        &pool,
        seed.project_id,
        clause_id,
        "confirm",
        &json!({}),
        1,
        &confirm_context,
    )
    .await
    .unwrap();
    let classification_id: Uuid = sqlx::query_scalar(
        "SELECT classification.id
           FROM bidding_current_procedural_classifications classification
           JOIN bid_procedural_segment_artifacts segment ON segment.id=classification.segment_id
          WHERE classification.project_id=$1 AND segment.clause_id=$2
            AND classification.effective_requirement_kind='confirmation'",
    )
    .bind(seed.project_id)
    .bind(clause_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let resolve_context = storage::bidding::MutationContext::new(
        seed.actor.clone(),
        format!("procedural-text-resolve-{classification_id}"),
        &json!({"resolution":"confirmed_by_user"}),
    )
    .unwrap();
    storage::bid_submission::resolve_procedural_requirement(
        &pool,
        seed.project_id,
        classification_id,
        "confirmed_by_user",
        None,
        None,
        &resolve_context,
    )
    .await
    .unwrap();

    let patch = json!({"text":"投标函签字并加盖公章。"});
    let patch_context = storage::bidding::MutationContext::new(
        seed.actor.clone(),
        format!("procedural-text-patch-{clause_id}"),
        &patch,
    )
    .unwrap();
    storage::bidding::mutate_clause(
        &pool,
        seed.project_id,
        clause_id,
        "patch",
        &patch,
        2,
        &patch_context,
    )
    .await
    .unwrap();

    let old_lifecycle: (String, Option<String>, String, Option<String>) = sqlx::query_as(
        "SELECT classification.lifecycle_status,classification.terminal_reason,
                decision.lifecycle_status,decision.terminal_reason
           FROM bid_procedural_classification_artifacts classification
           JOIN bid_procedural_decision_artifacts decision
             ON decision.classification_id=classification.id
          WHERE classification.id=$1",
    )
    .bind(classification_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        old_lifecycle,
        (
            "superseded".into(),
            Some("text_changed".into()),
            "superseded".into(),
            Some("text_changed".into())
        )
    );
    let current_segments: Vec<String> = sqlx::query_scalar(
        "SELECT classification.segment_text
           FROM bidding_current_procedural_classifications classification
           JOIN bid_procedural_segment_artifacts segment ON segment.id=classification.segment_id
          WHERE classification.project_id=$1 AND segment.clause_id=$2",
    )
    .bind(seed.project_id)
    .bind(clause_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(current_segments, vec!["投标函签字并加盖公章。".to_string()]);
}

#[tokio::test]
async fn manual_part_put_creates_and_keeps_a_current_exportable_revision() {
    let Some(pool) = live_test_pool().await else {
        return;
    };
    if !support::require_final_schema("Submission", final_submission_schema_is_ready(&pool).await) {
        return;
    }
    let seed = seed_project(&pool).await;

    for (expected_revision, markdown) in [(0_i64, "人工初稿"), (1_i64, "人工修订稿")] {
        let request = json!({
            "project_id": seed.project_id,
            "part_key": "1",
            "expected_content_revision": expected_revision,
            "markdown": markdown,
        });
        let (request_bytes, request_sha256) = request_identity(&request);
        let receipt: Value =
            sqlx::query_scalar("SELECT kb_bid_update_part($1,'1',$2,$3,$4,$5,$6,$7)")
                .bind(seed.project_id)
                .bind(expected_revision)
                .bind(markdown.as_bytes())
                .bind(&seed.actor)
                .bind(format!(
                    "manual-part-{expected_revision}-{}",
                    seed.project_id
                ))
                .bind(request_bytes)
                .bind(request_sha256)
                .fetch_one(&pool)
                .await
                .expect("manual part PUT must create a current dependency-bound revision");
        assert_eq!(receipt["revision"], expected_revision + 1);

        let current: (i64, Vec<u8>, bool, bool) = sqlx::query_as(
            "SELECT content.revision,content.canonical_markdown_utf8,current_value.stale,
                    dependency.part_content_artifact_id=current_value.content_artifact_id
               FROM bid_current_parts current_value
               JOIN bid_part_content_artifacts content ON content.id=current_value.content_artifact_id
               JOIN bid_part_dependency_artifacts dependency ON dependency.id=current_value.dependency_artifact_id
              WHERE current_value.project_id=$1 AND current_value.part_key='1'",
        )
        .bind(seed.project_id)
        .fetch_one(&pool)
        .await
        .expect("manual part must remain current after PUT");
        assert_eq!(
            current,
            (
                expected_revision + 1,
                markdown.as_bytes().to_vec(),
                false,
                true
            )
        );
    }
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
    if !support::require_final_schema("Submission", final_submission_schema_is_ready(&pool).await) {
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
}

#[tokio::test]
async fn rejected_upload_is_domain_zero_write_and_platform_abandon_is_tracked() {
    let Some(pool) = live_test_pool().await else {
        return;
    };
    if !support::require_final_schema("Submission", final_submission_schema_is_ready(&pool).await) {
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
    if !support::require_final_schema("Submission", final_submission_schema_is_ready(&pool).await) {
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
}

#[tokio::test]
async fn submission_render_job_is_idempotent_fenced_and_terminally_observable() {
    let Some(pool) = live_test_pool().await else {
        return;
    };
    if !support::require_final_schema("Submission", final_submission_schema_is_ready(&pool).await) {
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
}

#[tokio::test]
async fn submission_render_reaper_is_concurrent_idempotent_and_fences_old_claim() {
    let Some(pool) = live_test_pool().await else {
        return;
    };
    if !support::require_final_schema("Submission", final_submission_schema_is_ready(&pool).await) {
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
    assert!(
        !storage::bid_submission::heartbeat_submission_render(
            &pool,
            render_job_id,
            old_claim_token,
        )
        .await
        .unwrap(),
        "an expired claim must not be revived before the reaper runs"
    );
    assert_eq!(
        storage::bid_submission::fail_submission_render(
            &pool,
            render_job_id,
            old_claim_token,
            "STALE_OWNER",
            "expired owner must be fenced",
            true,
        )
        .await
        .unwrap(),
        None,
        "an expired claim must not settle the durable job"
    );
    let expired_output_id = Uuid::new_v4();
    let expired_output_bytes = b"expired owner output";
    let expired_output_sha256 = domain::sha256_hex(expired_output_bytes);
    let expired_output_ref = format!("objects/{expired_output_sha256}");
    let expired_staging_id = stage_object(
        &pool,
        &expired_output_ref,
        &expired_output_sha256,
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        expired_output_bytes.len() as i64,
        &seed.actor,
    )
    .await;
    let expired_publish: Result<Value, sqlx::Error> =
        sqlx::query_scalar("SELECT kb_bid_publish_submission_output($1,$2,$3,$4,$5,$6,$7)")
            .bind(expired_staging_id)
            .bind(expired_output_id)
            .bind(render_job_id)
            .bind(old_claim_token)
            .bind(&expired_output_ref)
            .bind(&expired_output_sha256)
            .bind(expired_output_bytes.len() as i64)
            .fetch_one(&pool)
            .await;
    assert_database_error(
        expired_publish.expect_err("expired claim must not publish before reaping"),
        "SUBMISSION_RENDER_CLAIM_LOST",
    );
    assert!(
        storage::abandon_object_upload(&pool, expired_staging_id, &seed.actor)
            .await
            .unwrap()
    );

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
}

#[tokio::test]
async fn manifest_freezes_global_occurrences_across_parts_and_repeated_object() {
    let Some(pool) = live_test_pool().await else {
        return;
    };
    if !support::require_final_schema("Submission", final_submission_schema_is_ready(&pool).await) {
        return;
    }
    let seed = seed_project(&pool).await;
    let object_ref = upload_shot_artifact(&pool, &seed, b"shared image bytes").await;
    seed_current_part_with_markdown(
        &pool,
        &seed,
        "1",
        &format!(
            "first ![shared]({object_ref})\nplain {object_ref}\nrepeated ![shared again]({object_ref})"
        ),
    )
    .await;
    seed_current_part_with_markdown(&pool, &seed, "4", &format!("other ![shared]({object_ref})"))
        .await;

    let missing_bare_ref = format!("plain objects/{}", "f".repeat(64));
    sqlx::query("SELECT kb_bid_validate_part_markdown_assets($1,$2)")
        .bind(seed.project_id)
        .bind(missing_bare_ref.as_bytes())
        .execute(&pool)
        .await
        .expect("a bare object ref is ordinary markdown text");

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
}

#[tokio::test]
async fn docx_without_eligible_quote_freezes_fixed_placeholder() {
    let Some(pool) = live_test_pool().await else {
        return;
    };
    if !support::require_final_schema("Submission", final_submission_schema_is_ready(&pool).await) {
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
}

#[tokio::test]
async fn pdf_manifest_creation_rejects_the_persisted_submission_gate() {
    let Some(pool) = live_test_pool().await else {
        return;
    };
    if !support::require_final_schema("Submission", final_submission_schema_is_ready(&pool).await) {
        return;
    }
    let seed = seed_project(&pool).await;
    let manifest_id = Uuid::new_v4();
    let request = json!({
        "manifest_id": manifest_id,
        "project_id": seed.project_id,
        "format": "pdf",
    });
    let (request_bytes, request_sha256) = request_identity(&request);
    let idempotency_key = format!("pdf-gate-{manifest_id}");
    let result: Result<Value, sqlx::Error> =
        sqlx::query_scalar("SELECT kb_bid_create_submission_manifest($1,$2,'pdf',$3,$4,$5,$6)")
            .bind(manifest_id)
            .bind(seed.project_id)
            .bind(&seed.actor)
            .bind(&idempotency_key)
            .bind(request_bytes)
            .bind(request_sha256)
            .fetch_one(&pool)
            .await;

    assert_database_error(
        result.expect_err("PDF must fail while the durable gate has hard issues"),
        "SUBMISSION_GATE_REJECTED",
    );
    assert_manifest_attempt_rolled_back(&pool, manifest_id, &seed.actor, &idempotency_key).await;
}

#[tokio::test]
async fn procedural_router_and_template_promotions_are_cas_fenced_and_durable() {
    let Some(pool) = live_test_pool().await else {
        return;
    };
    if !support::require_final_schema("Submission", final_submission_schema_is_ready(&pool).await) {
        return;
    }
    let seed = seed_project(&pool).await;
    seed_current_part_with_markdown(&pool, &seed, "1", "项目总览").await;

    let closed_request = json!({"target":"missing","expected_generation":0});
    let (closed_bytes, closed_sha256) = request_identity(&closed_request);
    let closed: Result<Value, sqlx::Error> = sqlx::query_scalar(
        "SELECT kb_bid_promote_procedural_router(
           'missing','procedural-router-v1',0,$1,$2,$3,$4)",
    )
    .bind(&seed.actor)
    .bind(format!("maintenance-required-{}", Uuid::new_v4()))
    .bind(closed_bytes)
    .bind(closed_sha256)
    .fetch_one(&pool)
    .await;
    assert_database_error(
        closed.expect_err("promotion outside maintenance must reject"),
        "MAINTENANCE_REQUIRED",
    );

    let initial_procedural: (String, i64) = sqlx::query_as(
        "SELECT version,promotion_generation FROM procedural_router_current WHERE singleton_key",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let initial_template: (String, i64) = sqlx::query_as(
        "SELECT version,promotion_generation
           FROM bid_template_contract_current WHERE slot='1'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(initial_procedural, ("procedural-router-v1".into(), 0));
    assert_eq!(initial_template, ("v1".into(), 0));

    let mut transaction = pool.begin().await.unwrap();
    sqlx::query(
        "UPDATE application_maintenance_gate
            SET mode='maintenance',generation=generation+1,updated_by=$1,updated_at=clock_timestamp()
          WHERE singleton_key",
    )
    .bind(&seed.actor)
    .execute(&mut *transaction)
    .await
    .unwrap();
    let clause_id = Uuid::new_v4();
    let segment_id = Uuid::new_v4();
    let classification_id = Uuid::new_v4();
    let decision_id = Uuid::new_v4();
    let segment_text = "投标函签字并盖章";
    let segment_bytes = segment_text.as_bytes();
    let segment_sha256 = domain::sha256_hex(segment_bytes);
    let stable_key = domain::sha256_hex(format!("{clause_id}:{segment_sha256}").as_bytes());
    sqlx::query(
        "INSERT INTO bid_clauses(
           id,project_id,provenance,status,kind,text,must,revision,created_by)
         VALUES($1,$2,'manual','confirmed','procedural',$3,true,2,$4)",
    )
    .bind(clause_id)
    .bind(seed.project_id)
    .bind(segment_text)
    .bind(&seed.actor)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO bid_procedural_segment_artifacts(
           id,project_id,clause_id,stable_key,segmentation_version,start_offset,end_offset,
           segment_utf8,segment_sha256,provenance)
         VALUES($1,$2,$3,$4,'procedural-segment-v1',0,$5,$6,$7,'manual')",
    )
    .bind(segment_id)
    .bind(seed.project_id)
    .bind(clause_id)
    .bind(stable_key)
    .bind(segment_bytes.len() as i64)
    .bind(segment_bytes)
    .bind(&segment_sha256)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO bid_procedural_classification_artifacts(
           id,project_id,segment_id,revision,router_contract_version,router_promotion_generation,
           router_result_status,router_requirement_kind,effective_requirement_kind,lifecycle_status)
         VALUES($1,$2,$3,1,'procedural-router-v1',0,'classified','confirmation','confirmation','current')",
    )
    .bind(classification_id)
    .bind(seed.project_id)
    .bind(segment_id)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO bid_procedural_decision_artifacts(
           id,project_id,classification_id,revision,resolution,actor_identity,decided_at,lifecycle_status)
         VALUES($1,$2,$3,1,'confirmed_by_user',$4,clock_timestamp(),'current')",
    )
    .bind(decision_id)
    .bind(seed.project_id)
    .bind(classification_id)
    .bind(&seed.actor)
    .execute(&mut *transaction)
    .await
    .unwrap();

    let suffix = Uuid::new_v4().simple().to_string();
    let procedural_versions = [
        (
            format!("procedural-router-{suffix}-v2"),
            json!({"status":"classified","kind":"confirmation"}),
        ),
        (
            format!("procedural-router-{suffix}-v3"),
            json!({"status":"classified","kind":"bid_bond"}),
        ),
    ];
    for (version, override_value) in &procedural_versions {
        let mut overrides = serde_json::Map::new();
        overrides.insert(segment_text.to_string(), override_value.clone());
        let contract = json!({
            "schema_version": 1,
            "version": version,
            "overrides": overrides,
        });
        let canonical_payload = serde_json::to_vec(&contract).unwrap();
        let content_sha256 = domain::sha256_hex(&canonical_payload);
        let request = json!({"version":version,"content_sha256":content_sha256});
        let (request_bytes, request_sha256) = request_identity(&request);
        let _: Value = sqlx::query_scalar(
            "SELECT kb_bid_register_procedural_router_contract($1,$2,$3,$4,$5,$6,$7)",
        )
        .bind(version)
        .bind(canonical_payload)
        .bind(content_sha256)
        .bind(&seed.actor)
        .bind(format!("register-{version}"))
        .bind(request_bytes)
        .bind(request_sha256)
        .fetch_one(&mut *transaction)
        .await
        .unwrap_or_else(|error| panic!("register {version}: {error}"));
    }

    let first_request = json!({"target_version":procedural_versions[0].0,"generation":0});
    let (first_bytes, first_sha256) = request_identity(&first_request);
    let first: Value = sqlx::query_scalar(
        "SELECT kb_bid_promote_procedural_router(
           $1,'procedural-router-v1',0,$2,$3,$4,$5)",
    )
    .bind(&procedural_versions[0].0)
    .bind(&seed.actor)
    .bind(format!("promote-{}", procedural_versions[0].0))
    .bind(first_bytes)
    .bind(first_sha256)
    .fetch_one(&mut *transaction)
    .await
    .expect("promote procedural router while preserving a compatible decision");
    assert_eq!(first["promotion_generation"], 1);
    sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
        .execute(&mut *transaction)
        .await
        .expect("verify first successor chain");
    let first_successor: (Uuid, i32, String, i64) = sqlx::query_as(
        "SELECT successor.id,successor.revision,successor.router_contract_version,
                successor.router_promotion_generation
           FROM bid_procedural_classification_artifacts predecessor
           JOIN bid_procedural_classification_artifacts successor
             ON successor.id=predecessor.successor_id
          WHERE predecessor.id=$1",
    )
    .bind(classification_id)
    .fetch_one(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(first_successor.1, 2);
    assert_eq!(first_successor.2, procedural_versions[0].0);
    assert_eq!(first_successor.3, 1);
    let first_decision_successor: (Uuid, Uuid, i32, String) = sqlx::query_as(
        "SELECT successor.id,successor.classification_id,successor.revision,predecessor.lifecycle_status
           FROM bid_procedural_decision_artifacts predecessor
           JOIN bid_procedural_decision_artifacts successor ON successor.id=predecessor.successor_id
          WHERE predecessor.id=$1",
    )
    .bind(decision_id)
    .fetch_one(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(first_decision_successor.1, first_successor.0);
    assert_eq!(first_decision_successor.2, 1);
    assert_eq!(first_decision_successor.3, "superseded");
    sqlx::query("SET CONSTRAINTS ALL DEFERRED")
        .execute(&mut *transaction)
        .await
        .unwrap();

    let second_request = json!({"target_version":procedural_versions[1].0,"generation":1});
    let (second_bytes, second_sha256) = request_identity(&second_request);
    let second: Value =
        sqlx::query_scalar("SELECT kb_bid_promote_procedural_router($1,$2,1,$3,$4,$5,$6)")
            .bind(&procedural_versions[1].0)
            .bind(&procedural_versions[0].0)
            .bind(&seed.actor)
            .bind(format!("promote-{}", procedural_versions[1].0))
            .bind(second_bytes)
            .bind(second_sha256)
            .fetch_one(&mut *transaction)
            .await
            .expect("promote procedural router while terminating an incompatible decision");
    assert_eq!(second["promotion_generation"], 2);
    assert!(second["blocked_decision_count"].as_i64().unwrap() >= 1);
    sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
        .execute(&mut *transaction)
        .await
        .expect("verify second successor and terminal contracts");
    let current_classification: (i32, String, i64, Option<String>) = sqlx::query_as(
        "SELECT revision,router_contract_version,router_promotion_generation,effective_requirement_kind
           FROM bid_procedural_classification_artifacts
          WHERE segment_id=$1 AND lifecycle_status='current'",
    )
    .bind(segment_id)
    .fetch_one(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(
        current_classification,
        (
            3,
            procedural_versions[1].0.clone(),
            2,
            Some("bid_bond".into())
        )
    );
    let terminated_decision: (String, Option<String>, bool, bool) = sqlx::query_as(
        "SELECT lifecycle_status,terminal_reason,terminal_at IS NOT NULL,terminal_actor IS NOT NULL
           FROM bid_procedural_decision_artifacts WHERE id=$1",
    )
    .bind(first_decision_successor.0)
    .fetch_one(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(
        terminated_decision,
        (
            "superseded".into(),
            Some("router_promoted".into()),
            true,
            true
        )
    );
    let current_decisions: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM bid_procedural_decision_artifacts decision
          JOIN bid_procedural_classification_artifacts classification
            ON classification.id=decision.classification_id
         WHERE classification.segment_id=$1 AND decision.lifecycle_status='current'",
    )
    .bind(segment_id)
    .fetch_one(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(current_decisions, 0);

    {
        let mut savepoint = transaction.begin().await.unwrap();
        let request = json!({"stale_expected_generation":0});
        let (request_bytes, request_sha256) = request_identity(&request);
        let stale: Result<Value, sqlx::Error> = sqlx::query_scalar(
            "SELECT kb_bid_promote_procedural_router($1,'procedural-router-v1',0,$2,$3,$4,$5)",
        )
        .bind(&procedural_versions[1].0)
        .bind(&seed.actor)
        .bind(format!("procedural-stale-cas-{suffix}"))
        .bind(request_bytes)
        .bind(request_sha256)
        .fetch_one(&mut *savepoint)
        .await;
        assert_database_error(
            stale.expect_err("stale procedural promotion CAS must reject"),
            "PROCEDURAL_ROUTER_PROMOTION_CAS_MISMATCH",
        );
        savepoint.rollback().await.unwrap();
    }

    let template_version = format!("template-{suffix}-v2");
    let template_contract = json!({
        "schema_version": 1,
        "slot": "1",
        "version": template_version,
    });
    let template_payload = serde_json::to_vec(&template_contract).unwrap();
    let template_sha256 = domain::sha256_hex(&template_payload);
    let template_request = json!({"slot":"1","version":template_version});
    let (template_bytes, template_request_sha256) = request_identity(&template_request);
    let _: Value =
        sqlx::query_scalar("SELECT kb_bid_register_template_contract('1',$1,$2,$3,$4,$5,$6,$7)")
            .bind(&template_version)
            .bind(template_payload)
            .bind(template_sha256)
            .bind(&seed.actor)
            .bind(format!("register-{template_version}"))
            .bind(template_bytes)
            .bind(template_request_sha256)
            .fetch_one(&mut *transaction)
            .await
            .expect("register target template contract");
    let expected_stale_part_count: i64 = sqlx::query_scalar(
        "SELECT count(*)
           FROM bid_current_parts current_part
           JOIN bid_part_dependency_artifacts dependency
             ON dependency.id=current_part.dependency_artifact_id
           JOIN bid_projects project_value ON project_value.id=current_part.project_id
          WHERE project_value.status='open'
            AND dependency.template_slot='1' AND dependency.template_version='v1'",
    )
    .fetch_one(&mut *transaction)
    .await
    .unwrap();
    let promote_template_request = json!({"slot":"1","target_version":template_version});
    let (promote_template_bytes, promote_template_sha256) =
        request_identity(&promote_template_request);
    let template: Value =
        sqlx::query_scalar("SELECT kb_bid_promote_template_contract('1',$1,'v1',0,$2,$3,$4,$5)")
            .bind(&template_version)
            .bind(&seed.actor)
            .bind(format!("promote-{template_version}"))
            .bind(promote_template_bytes)
            .bind(promote_template_sha256)
            .fetch_one(&mut *transaction)
            .await
            .expect("promote template and stale its current consumers");
    assert_eq!(template["promotion_generation"], 1);
    assert_eq!(
        template["stale_part_count"].as_i64(),
        Some(expected_stale_part_count)
    );
    let stale_part: (bool, Vec<String>) = sqlx::query_as(
        "SELECT stale,stale_reason_codes FROM bid_current_parts
          WHERE project_id=$1 AND part_key='1'",
    )
    .bind(seed.project_id)
    .fetch_one(&mut *transaction)
    .await
    .unwrap();
    assert!(stale_part.0);
    assert!(
        stale_part
            .1
            .contains(&"TEMPLATE_CONTRACT_PROMOTED".to_string())
    );

    {
        let mut savepoint = transaction.begin().await.unwrap();
        let request = json!({"stale_expected_generation":0});
        let (request_bytes, request_sha256) = request_identity(&request);
        let stale: Result<Value, sqlx::Error> = sqlx::query_scalar(
            "SELECT kb_bid_promote_template_contract('1',$1,'v1',0,$2,$3,$4,$5)",
        )
        .bind(&template_version)
        .bind(&seed.actor)
        .bind(format!("template-stale-cas-{suffix}"))
        .bind(request_bytes)
        .bind(request_sha256)
        .fetch_one(&mut *savepoint)
        .await;
        assert_database_error(
            stale.expect_err("stale template promotion CAS must reject"),
            "TEMPLATE_CONTRACT_PROMOTION_CAS_MISMATCH",
        );
        savepoint.rollback().await.unwrap();
    }

    for (operation, expected_count) in [
        ("bid.procedural_router.register", 2_i64),
        ("bid.procedural_router.promote", 2_i64),
        ("bid.template_contract.register", 1_i64),
        ("bid.template_contract.promote", 1_i64),
    ] {
        let audit_count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM audit_events
              WHERE operation=$1 AND actor_identity=$2",
        )
        .bind(operation)
        .bind(&seed.actor)
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
        assert_eq!(
            audit_count, expected_count,
            "maintenance contract mutation must append exactly one audit envelope"
        );
    }

    transaction.rollback().await.unwrap();
    let rolled_back_procedural: (String, i64) = sqlx::query_as(
        "SELECT version,promotion_generation FROM procedural_router_current WHERE singleton_key",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let rolled_back_template: (String, i64) = sqlx::query_as(
        "SELECT version,promotion_generation
           FROM bid_template_contract_current WHERE slot='1'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(rolled_back_procedural, initial_procedural);
    assert_eq!(rolled_back_template, initial_template);
}

#[tokio::test]
async fn pdf_attachment_preparation_requires_a_contiguous_frozen_page_set() {
    let Some(pool) = live_test_pool().await else {
        return;
    };
    if !support::require_final_schema("Submission", final_submission_schema_is_ready(&pool).await) {
        return;
    }
    let seed = seed_project(&pool).await;
    let attachment_id = Uuid::new_v4();
    let original = format!("%PDF-1.7\n% {attachment_id}\n%%EOF\n").into_bytes();
    let original_digest = domain::sha256_hex(&original);
    let original_ref = format!("objects/{original_digest}");
    let original_staging = stage_object(
        &pool,
        &original_ref,
        &original_digest,
        "application/pdf",
        original.len() as i64,
        &seed.actor,
    )
    .await;
    let request = json!({
        "attachment_id": attachment_id,
        "project_id": seed.project_id,
        "kind": "bid_bond",
    });
    let (request_bytes, request_sha256) = request_identity(&request);
    let uploaded: Value = sqlx::query_scalar(
        "SELECT kb_bid_upload_attachment(
           $1,$2,$3,'bid_bond',$4,$5,'application/pdf',$6,NULL,NULL,$7,$8,$9,$10)",
    )
    .bind(original_staging)
    .bind(attachment_id)
    .bind(seed.project_id)
    .bind(&original_ref)
    .bind(&original_digest)
    .bind(original.len() as i64)
    .bind(&seed.actor)
    .bind(format!("attachment-gap-{attachment_id}"))
    .bind(request_bytes)
    .bind(request_sha256)
    .fetch_one(&pool)
    .await
    .expect("upload PDF before preparing its frozen pages");
    assert_eq!(uploaded["preparation_status"], "pending");
    assert_eq!(uploaded["render_page_count"], 0);
    let preparation_job_id: Uuid = uploaded["preparation_job_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();

    let validate_request = json!({
        "attachment_id": attachment_id,
        "action": "validate",
        "expected_revision": 1,
    });
    let (validate_bytes, validate_sha256) = request_identity(&validate_request);
    let validation: Result<Value, sqlx::Error> =
        sqlx::query_scalar("SELECT kb_bid_mutate_attachment($1,$2,'validate',1,NULL,$3,$4,$5,$6)")
            .bind(seed.project_id)
            .bind(attachment_id)
            .bind(&seed.actor)
            .bind(format!("validate-pending-{attachment_id}"))
            .bind(validate_bytes)
            .bind(validate_sha256)
            .fetch_one(&pool)
            .await;
    assert_database_error(
        validation.expect_err("a PDF cannot validate while page preparation is pending"),
        "ATTACHMENT_PREPARATION_INCOMPLETE",
    );

    let worker_actor = "system:bid-attachment-preparation";
    let page = unique_png(Uuid::new_v4());
    let page_digest = domain::sha256_hex(&page);
    let page_ref = format!("objects/{page_digest}");
    let page_staging = stage_object(
        &pool,
        &page_ref,
        &page_digest,
        "image/png",
        page.len() as i64,
        worker_actor,
    )
    .await;
    let render_pages = json!([{
        "staging_id": page_staging,
        "page_ordinal": 1,
        "object_ref": page_ref,
        "digest": page_digest,
        "media_type": "image/png",
        "byte_length": page.len(),
        "pixel_width": 2,
        "pixel_height": 2,
    }]);
    let claim_token = Uuid::new_v4();
    let claim: Option<Value> =
        sqlx::query_scalar("SELECT kb_bid_claim_attachment_preparation($1,$2)")
            .bind(preparation_job_id)
            .bind(claim_token)
            .fetch_one(&pool)
            .await
            .expect("claim PDF attachment preparation");
    assert!(claim.is_some());
    let result: Result<Value, sqlx::Error> =
        sqlx::query_scalar("SELECT kb_bid_publish_attachment_preparation($1,$2,$3,$4)")
            .bind(preparation_job_id)
            .bind(claim_token)
            .bind(&render_pages)
            .bind(worker_actor)
            .fetch_one(&pool)
            .await;
    assert_database_error(
        result.expect_err("a PDF page set starting at ordinal one must reject"),
        "ATTACHMENT_RENDER_PAGE_SET_INVALID",
    );
    let page_rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM bid_attachment_render_pages WHERE attachment_id=$1",
    )
    .bind(attachment_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let page_owner_rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM object_owner_references
          WHERE owner_kind='bid_attachment_page' AND owner_id=$1",
    )
    .bind(attachment_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((page_rows, page_owner_rows), (0, 0));
    let failed: Option<String> = sqlx::query_scalar(
        "SELECT kb_bid_fail_attachment_preparation($1,$2,'INVALID_PAGE_SET',$3,true)",
    )
    .bind(preparation_job_id)
    .bind(claim_token)
    .bind("page ordinals must start at zero")
    .fetch_one(&pool)
    .await
    .expect("settle the rejected preparation claim");
    assert_eq!(failed.as_deref(), Some("pending"));
    assert!(
        storage::abandon_object_upload(&pool, page_staging, worker_actor)
            .await
            .unwrap()
    );

    let second_claim_token = Uuid::new_v4();
    let second_claim: Option<Value> =
        sqlx::query_scalar("SELECT kb_bid_claim_attachment_preparation($1,$2)")
            .bind(preparation_job_id)
            .bind(second_claim_token)
            .fetch_one(&pool)
            .await
            .expect("reclaim preparation after retryable page-set failure");
    assert!(second_claim.is_some());
    let (page_zero_staging, page_zero) = stage_render_page(&pool, 0).await;
    let unavailable_page = unique_png(Uuid::new_v4());
    let unavailable_digest = domain::sha256_hex(&unavailable_page);
    let page_one = json!({
        "staging_id": Uuid::new_v4(),
        "page_ordinal": 1,
        "object_ref": format!("objects/{unavailable_digest}"),
        "digest": unavailable_digest,
        "media_type": "image/png",
        "byte_length": unavailable_page.len(),
        "pixel_width": 2,
        "pixel_height": 2,
    });
    let partial_publish: Result<Value, sqlx::Error> =
        sqlx::query_scalar("SELECT kb_bid_publish_attachment_preparation($1,$2,$3,$4)")
            .bind(preparation_job_id)
            .bind(second_claim_token)
            .bind(json!([page_zero, page_one]))
            .bind(worker_actor)
            .fetch_one(&pool)
            .await;
    partial_publish.expect_err("missing second staging row must roll back the complete page set");
    let page_rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM bid_attachment_render_pages WHERE attachment_id=$1",
    )
    .bind(attachment_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let page_owner_rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM object_owner_references
          WHERE owner_kind='bid_attachment_page' AND owner_id=$1",
    )
    .bind(attachment_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let page_zero_staging_rows: i64 =
        sqlx::query_scalar("SELECT count(*) FROM object_upload_staging WHERE id=$1")
            .bind(page_zero_staging)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        (page_rows, page_owner_rows, page_zero_staging_rows),
        (0, 0, 1)
    );
    let failed: Option<String> = sqlx::query_scalar(
        "SELECT kb_bid_fail_attachment_preparation($1,$2,'STAGING_MISSING',$3,true)",
    )
    .bind(preparation_job_id)
    .bind(second_claim_token)
    .bind("a page staging row disappeared before publication")
    .fetch_one(&pool)
    .await
    .expect("settle atomic publication failure");
    assert_eq!(failed.as_deref(), Some("pending"));
    assert!(
        storage::abandon_object_upload(&pool, page_zero_staging, worker_actor)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn attachment_preparation_reaper_fences_expired_owner_and_allows_reclaim() {
    let Some(pool) = live_test_pool().await else {
        return;
    };
    if !support::require_final_schema("Submission", final_submission_schema_is_ready(&pool).await) {
        return;
    }
    let seed = seed_project(&pool).await;
    let (attachment_id, preparation_job_id) = upload_pending_pdf_attachment(&pool, &seed).await;
    let expired_token = Uuid::new_v4();
    let claim: Option<Value> =
        sqlx::query_scalar("SELECT kb_bid_claim_attachment_preparation($1,$2)")
            .bind(preparation_job_id)
            .bind(expired_token)
            .fetch_one(&pool)
            .await
            .expect("claim PDF preparation before simulating lease expiry");
    assert!(claim.is_some());
    sqlx::query(
        "UPDATE bid_attachment_preparation_jobs
            SET heartbeat_at=clock_timestamp()-make_interval(secs=>claim_lease_ms/1000.0)-interval '1 second'
          WHERE id=$1",
    )
    .bind(preparation_job_id)
    .execute(&pool)
    .await
    .unwrap();

    let renewed: bool = sqlx::query_scalar("SELECT kb_bid_heartbeat_attachment_preparation($1,$2)")
        .bind(preparation_job_id)
        .bind(expired_token)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(!renewed, "an expired lease must not be revived");
    let stale_publish: Result<Value, sqlx::Error> =
        sqlx::query_scalar("SELECT kb_bid_publish_attachment_preparation($1,$2,'[]'::jsonb,$3)")
            .bind(preparation_job_id)
            .bind(expired_token)
            .bind("system:bid-attachment-preparation")
            .fetch_one(&pool)
            .await;
    assert_database_error(
        stale_publish.expect_err("expired attachment preparation owner must be fenced"),
        "ATTACHMENT_PREPARATION_CLAIM_LOST",
    );

    let reaped: i32 = sqlx::query_scalar("SELECT kb_bid_reap_attachment_preparations()")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(reaped, 1);
    let reaped_state: (String, Option<Uuid>, String) = sqlx::query_as(
        "SELECT status,claim_token,error_code
           FROM bid_attachment_preparation_jobs WHERE id=$1",
    )
    .bind(preparation_job_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        reaped_state,
        ("pending".into(), None, "CLAIM_LEASE_EXPIRED".into())
    );

    let reclaim_token = Uuid::new_v4();
    let reclaim: Option<Value> =
        sqlx::query_scalar("SELECT kb_bid_claim_attachment_preparation($1,$2)")
            .bind(preparation_job_id)
            .bind(reclaim_token)
            .fetch_one(&pool)
            .await
            .expect("reclaim reaped PDF preparation");
    assert!(reclaim.is_some());
    let (_, page) = stage_render_page(&pool, 0).await;
    let prepared: Value =
        sqlx::query_scalar("SELECT kb_bid_publish_attachment_preparation($1,$2,$3,$4)")
            .bind(preparation_job_id)
            .bind(reclaim_token)
            .bind(json!([page]))
            .bind("system:bid-attachment-preparation")
            .fetch_one(&pool)
            .await
            .expect("new preparation owner publishes after reaping old lease");
    assert_eq!(prepared["attachment_id"], attachment_id.to_string());
    assert_eq!(prepared["status"], "completed");
}

#[tokio::test]
async fn rejecting_or_deleting_attachment_cancels_and_fences_running_preparation() {
    let Some(pool) = live_test_pool().await else {
        return;
    };
    if !support::require_final_schema("Submission", final_submission_schema_is_ready(&pool).await) {
        return;
    }
    let seed = seed_project(&pool).await;
    for (action, expected_error_code) in [
        ("reject", "ATTACHMENT_REJECTED"),
        ("delete", "ATTACHMENT_DELETED"),
    ] {
        let (attachment_id, preparation_job_id) = upload_pending_pdf_attachment(&pool, &seed).await;
        let claim_token = Uuid::new_v4();
        let claim: Option<Value> =
            sqlx::query_scalar("SELECT kb_bid_claim_attachment_preparation($1,$2)")
                .bind(preparation_job_id)
                .bind(claim_token)
                .fetch_one(&pool)
                .await
                .expect("claim preparation before attachment cancellation");
        assert!(claim.is_some());

        let request = json!({
            "attachment_id": attachment_id,
            "action": action,
            "expected_revision": 1,
        });
        let (request_bytes, request_sha256) = request_identity(&request);
        let _: Value =
            sqlx::query_scalar("SELECT kb_bid_mutate_attachment($1,$2,$3,1,NULL,$4,$5,$6,$7)")
                .bind(seed.project_id)
                .bind(attachment_id)
                .bind(action)
                .bind(&seed.actor)
                .bind(format!("{action}-running-preparation-{attachment_id}"))
                .bind(request_bytes)
                .bind(request_sha256)
                .fetch_one(&pool)
                .await
                .unwrap_or_else(|error| {
                    panic!("{action} attachment with running preparation: {error}")
                });
        let job_state: (String, Option<Uuid>, String) = sqlx::query_as(
            "SELECT status,claim_token,error_code
               FROM bid_attachment_preparation_jobs WHERE id=$1",
        )
        .bind(preparation_job_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            job_state,
            ("cancelled".into(), None, expected_error_code.into())
        );
        let heartbeat: bool =
            sqlx::query_scalar("SELECT kb_bid_heartbeat_attachment_preparation($1,$2)")
                .bind(preparation_job_id)
                .bind(claim_token)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(!heartbeat);
        let stale_publish: Result<Value, sqlx::Error> = sqlx::query_scalar(
            "SELECT kb_bid_publish_attachment_preparation($1,$2,'[]'::jsonb,$3)",
        )
        .bind(preparation_job_id)
        .bind(claim_token)
        .bind("system:bid-attachment-preparation")
        .fetch_one(&pool)
        .await;
        assert_database_error(
            stale_publish.expect_err("cancelled preparation owner must be fenced"),
            "ATTACHMENT_PREPARATION_CLAIM_LOST",
        );
    }
}

#[tokio::test]
async fn attachment_validation_rejects_unavailable_frozen_object_identity() {
    let Some(pool) = live_test_pool().await else {
        return;
    };
    if !support::require_final_schema("Submission", final_submission_schema_is_ready(&pool).await) {
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
        "media_type": "image/png",
        "byte_length": attachment_bytes.len(),
    });
    let (upload_bytes, upload_sha256) = request_identity(&upload);
    let staging_id = stage_object(
        &pool,
        &object_ref,
        &digest,
        "image/png",
        attachment_bytes.len() as i64,
        &seed.actor,
    )
    .await;
    let uploaded: Value = sqlx::query_scalar(
        "SELECT kb_bid_upload_attachment(
           $1,$2,$3,'bid_bond',$4,$5,'image/png',$6,1,1,$7,$8,$9,$10)",
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
    assert_eq!(uploaded["preparation_status"], "not_required");
    assert!(uploaded["preparation_job_id"].is_null());
    let preparation_jobs: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM bid_attachment_preparation_jobs WHERE attachment_id=$1",
    )
    .bind(attachment_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(preparation_jobs, 0);

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
}

#[tokio::test]
async fn confirmed_pdf_attachment_is_frozen_into_manifest_and_rendered_from_pages() {
    let Some(pool) = live_test_pool().await else {
        return;
    };
    if !support::require_final_schema("Submission", final_submission_schema_is_ready(&pool).await) {
        return;
    }
    let seed = seed_project(&pool).await;
    sqlx::query(
        "INSERT INTO bid_clause_set_identities(project_id,set_kind,revision,content_sha256,updated_at)
         VALUES($1,'procedural',0,
           encode(public.digest(convert_to('ClauseSetV1:procedural:','UTF8'),'sha256'),'hex'),
           clock_timestamp())",
    )
    .bind(seed.project_id)
    .execute(&pool)
    .await
    .unwrap();

    let original_pdf = bid::render_manifest_document(
        bid::submission::GateFormat::Pdf,
        "保证金附件原件",
        &[("1".into(), "已冻结 PDF 原件".into())],
        &[],
    )
    .expect("build valid PDF attachment fixture");
    let pages = [unique_png(Uuid::new_v4()), unique_png(Uuid::new_v4())];
    let original_digest = domain::sha256_hex(&original_pdf);
    let page_digests = pages
        .iter()
        .map(|page| domain::sha256_hex(page))
        .collect::<Vec<_>>();
    let _blobs = TestBlobs::persist(&[
        (&original_digest, &original_pdf),
        (&page_digests[0], &pages[0]),
        (&page_digests[1], &pages[1]),
    ]);
    let original_ref = format!("objects/{original_digest}");
    let original_staging = stage_object(
        &pool,
        &original_ref,
        &original_digest,
        "application/pdf",
        original_pdf.len() as i64,
        &seed.actor,
    )
    .await;
    let attachment_id = Uuid::new_v4();
    let upload_request = json!({
        "attachment_id": attachment_id,
        "project_id": seed.project_id,
        "kind": "bid_bond",
        "object_ref": original_ref,
        "digest": original_digest,
    });
    let (upload_bytes, upload_sha256) = request_identity(&upload_request);
    let uploaded: Value = sqlx::query_scalar(
        "SELECT kb_bid_upload_attachment(
           $1,$2,$3,'bid_bond',$4,$5,'application/pdf',$6,NULL,NULL,$7,$8,$9,$10)",
    )
    .bind(original_staging)
    .bind(attachment_id)
    .bind(seed.project_id)
    .bind(&original_ref)
    .bind(&original_digest)
    .bind(original_pdf.len() as i64)
    .bind(&seed.actor)
    .bind(format!("upload-pdf-{attachment_id}"))
    .bind(upload_bytes)
    .bind(upload_sha256)
    .fetch_one(&pool)
    .await
    .expect("upload PDF before preparing frozen pages");
    assert_eq!(uploaded["preparation_status"], "pending");
    assert_eq!(uploaded["render_page_count"], 0);
    let preparation_job_id: Uuid = uploaded["preparation_job_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();

    let pending_validation_request = json!({
        "attachment_id": attachment_id,
        "action": "validate",
        "expected_revision": 1,
    });
    let (pending_validation_bytes, pending_validation_sha256) =
        request_identity(&pending_validation_request);
    let pending_validation: Result<Value, sqlx::Error> =
        sqlx::query_scalar("SELECT kb_bid_mutate_attachment($1,$2,'validate',1,NULL,$3,$4,$5,$6)")
            .bind(seed.project_id)
            .bind(attachment_id)
            .bind(&seed.actor)
            .bind(format!("validate-pending-{attachment_id}"))
            .bind(pending_validation_bytes)
            .bind(pending_validation_sha256)
            .fetch_one(&pool)
            .await;
    assert_database_error(
        pending_validation.expect_err("pending PDF preparation must block validation"),
        "ATTACHMENT_PREPARATION_INCOMPLETE",
    );

    let worker_actor = "system:bid-attachment-preparation";
    let mut render_pages = Vec::new();
    for (page_ordinal, (page, digest)) in pages.iter().zip(&page_digests).enumerate() {
        let object_ref = format!("objects/{digest}");
        let staging_id = stage_object(
            &pool,
            &object_ref,
            digest,
            "image/png",
            page.len() as i64,
            worker_actor,
        )
        .await;
        render_pages.push(json!({
            "staging_id": staging_id,
            "page_ordinal": page_ordinal,
            "object_ref": object_ref,
            "digest": digest,
            "media_type": "image/png",
            "byte_length": page.len(),
            "pixel_width": 2,
            "pixel_height": 2,
        }));
    }
    let claim_token = Uuid::new_v4();
    let claim: Option<Value> =
        sqlx::query_scalar("SELECT kb_bid_claim_attachment_preparation($1,$2)")
            .bind(preparation_job_id)
            .bind(claim_token)
            .fetch_one(&pool)
            .await
            .expect("claim PDF attachment preparation");
    assert!(claim.is_some());
    let prepared: Value =
        sqlx::query_scalar("SELECT kb_bid_publish_attachment_preparation($1,$2,$3,$4)")
            .bind(preparation_job_id)
            .bind(claim_token)
            .bind(json!(render_pages))
            .bind(worker_actor)
            .fetch_one(&pool)
            .await
            .expect("publish the frozen PDF page set");
    assert_eq!(prepared["status"], "completed");
    assert_eq!(prepared["render_page_count"], 2);

    for (action, expected_revision) in [("validate", 1), ("confirm", 2)] {
        let request = json!({
            "attachment_id": attachment_id,
            "action": action,
            "expected_revision": expected_revision,
        });
        let (request_bytes, request_sha256) = request_identity(&request);
        let _: Value =
            sqlx::query_scalar("SELECT kb_bid_mutate_attachment($1,$2,$3,$4,NULL,$5,$6,$7,$8)")
                .bind(seed.project_id)
                .bind(attachment_id)
                .bind(action)
                .bind(expected_revision)
                .bind(&seed.actor)
                .bind(format!("{action}-{attachment_id}"))
                .bind(request_bytes)
                .bind(request_sha256)
                .fetch_one(&pool)
                .await
                .unwrap_or_else(|error| panic!("{action} frozen PDF attachment: {error}"));
    }

    let clause_id = Uuid::new_v4();
    let create_context = storage::bidding::MutationContext::new(
        seed.actor.clone(),
        format!("create-procedural-{clause_id}"),
        &json!({"text":"上传保证金缴纳回执","kind":"procedural"}),
    )
    .unwrap();
    storage::bidding::create_clause(
        &pool,
        clause_id,
        seed.project_id,
        "上传保证金缴纳回执",
        "procedural",
        true,
        &create_context,
    )
    .await
    .expect("create procedural clause");
    let confirm_context = storage::bidding::MutationContext::new(
        seed.actor.clone(),
        format!("confirm-procedural-{clause_id}"),
        &json!({"action":"confirm","expected_revision":1}),
    )
    .unwrap();
    storage::bidding::mutate_clause(
        &pool,
        seed.project_id,
        clause_id,
        "confirm",
        &json!({}),
        1,
        &confirm_context,
    )
    .await
    .expect("confirm procedural clause");
    let classification_id: Uuid = sqlx::query_scalar(
        "SELECT classification.id
           FROM bidding_current_procedural_classifications classification
           JOIN bid_procedural_segment_artifacts segment ON segment.id=classification.segment_id
          WHERE classification.project_id=$1 AND segment.clause_id=$2
            AND classification.effective_requirement_kind='bid_bond'",
    )
    .bind(seed.project_id)
    .bind(clause_id)
    .fetch_one(&pool)
    .await
    .expect("procedural clause must classify as bid bond");
    let resolve_context = storage::bidding::MutationContext::new(
        seed.actor.clone(),
        format!("resolve-bid-bond-{classification_id}"),
        &json!({"resolution":"satisfied_by_attachment","attachment_id":attachment_id}),
    )
    .unwrap();
    storage::bid_submission::resolve_procedural_requirement(
        &pool,
        seed.project_id,
        classification_id,
        "satisfied_by_attachment",
        Some(attachment_id),
        None,
        &resolve_context,
    )
    .await
    .expect("resolve requirement with confirmed PDF attachment");

    let manifest_id = Uuid::new_v4();
    let manifest_key = format!("pdf-attachment-manifest-{manifest_id}");
    let manifest_context = storage::bidding::MutationContext::new(
        seed.actor.clone(),
        manifest_key,
        &json!({"manifest_id":manifest_id,"project_id":seed.project_id,"format":"docx"}),
    )
    .unwrap();
    storage::bid_submission::create_submission_manifest(
        &pool,
        manifest_id,
        seed.project_id,
        "docx",
        &manifest_context,
    )
    .await
    .expect("freeze attachment into a manifest after verifying physical bytes");
    let input = storage::bid_submission::manifest_render_input(&pool, seed.project_id, manifest_id)
        .await
        .unwrap();
    let parts = input["parts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|part| {
            (
                part["part_key"].as_str().unwrap().to_string(),
                part["markdown"].as_str().unwrap().to_string(),
            )
        })
        .collect::<Vec<_>>();
    let mut assets = Vec::new();
    let mut page_asset_ids = Vec::new();
    for row in input["assets"].as_array().unwrap() {
        let asset_id: Uuid = row["id"].as_str().unwrap().parse().unwrap();
        let stored = storage::bid_submission::read_manifest_render_asset(
            &pool,
            seed.project_id,
            manifest_id,
            asset_id,
        )
        .await
        .expect("read only the frozen manifest asset");
        let manifest_ordinal = u32::try_from(stored.manifest_ordinal).unwrap();
        let locator = match stored.source_kind.as_str() {
            "procedural_attachment" => {
                bid::ManifestRenderAssetLocator::ProceduralAttachmentOriginal {
                    part_key: stored.source_locator["part_key"]
                        .as_str()
                        .unwrap()
                        .to_string(),
                    attachment_ordinal: u32::try_from(
                        stored.source_locator["attachment_ordinal"]
                            .as_u64()
                            .unwrap(),
                    )
                    .unwrap(),
                    attachment_id: stored.source_locator["attachment_id"]
                        .as_str()
                        .unwrap()
                        .parse()
                        .unwrap(),
                    kind: stored.source_locator["kind"].as_str().unwrap().to_string(),
                }
            }
            "procedural_attachment_page" => {
                let page_ordinal =
                    u32::try_from(stored.source_locator["page_ordinal"].as_u64().unwrap()).unwrap();
                page_asset_ids.push((page_ordinal, asset_id));
                bid::ManifestRenderAssetLocator::ProceduralAttachmentPage {
                    part_key: stored.source_locator["part_key"]
                        .as_str()
                        .unwrap()
                        .to_string(),
                    attachment_ordinal: u32::try_from(
                        stored.source_locator["attachment_ordinal"]
                            .as_u64()
                            .unwrap(),
                    )
                    .unwrap(),
                    attachment_id: stored.source_locator["attachment_id"]
                        .as_str()
                        .unwrap()
                        .parse()
                        .unwrap(),
                    page_ordinal,
                }
            }
            source_kind => panic!("unexpected formal attachment source kind: {source_kind}"),
        };
        assets.push(bid::ManifestRenderAsset {
            manifest_ordinal,
            locator,
            object_ref: stored.object_ref,
            digest: stored.digest,
            media_type: stored.media_type,
            byte_length: u64::try_from(stored.byte_length).unwrap(),
            bytes: stored.bytes,
        });
    }
    assert_eq!(
        input["assets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|asset| asset["source_kind"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec![
            "procedural_attachment",
            "procedural_attachment_page",
            "procedural_attachment_page",
        ]
    );
    assert_eq!(page_asset_ids.len(), 2);
    let docx = bid::render_manifest_document(
        bid::submission::GateFormat::Docx,
        "投标文件",
        &parts,
        &assets,
    )
    .expect("render manifest-only DOCX with both frozen PDF pages");
    let parsed_docx = docx_rs::read_docx(&docx).expect("parse rendered DOCX");
    assert_eq!(parsed_docx.images.len(), 2);
    let pdf = bid::render_manifest_document(
        bid::submission::GateFormat::Pdf,
        "投标文件",
        &parts,
        &assets,
    )
    .expect("render manifest-only PDF with both frozen PDF pages");
    assert!(pdf.starts_with(b"%PDF"));
    lopdf::Document::load_mem(&pdf).expect("rendered PDF is structurally valid");

    std::fs::write(
        storage::blob_path(&page_digests[0]),
        unique_png(Uuid::new_v4()),
    )
    .unwrap();
    let corrupt = storage::bid_submission::read_manifest_render_asset(
        &pool,
        seed.project_id,
        manifest_id,
        page_asset_ids
            .iter()
            .find(|(ordinal, _)| *ordinal == 0)
            .unwrap()
            .1,
    )
    .await
    .expect_err("corrupt frozen page bytes must fail closed");
    assert!(
        corrupt
            .to_string()
            .contains("MANIFEST_ASSET_IDENTITY_MISMATCH")
    );
    std::fs::write(storage::blob_path(&page_digests[0]), &pages[0]).unwrap();

    let page_one_ref = format!("objects/{}", page_digests[1]);
    let retention_scheduled: bool =
        sqlx::query_scalar("SELECT kb_object_reference_remove($1,'bid_attachment_page',$2,'1')")
            .bind(&page_one_ref)
            .bind(attachment_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        !retention_scheduled,
        "the already-frozen manifest reference must keep the object available"
    );
    let attachment_page_owners: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM object_owner_references
          WHERE object_ref=$1 AND owner_kind='bid_attachment_page'
            AND owner_id=$2 AND occurrence='1'",
    )
    .bind(&page_one_ref)
    .bind(attachment_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(attachment_page_owners, 0);
    let validate_request = json!({
        "attachment_id": attachment_id,
        "action": "validate",
        "expected_revision": 3,
    });
    let (validate_bytes, validate_sha256) = request_identity(&validate_request);
    let validation: Result<Value, sqlx::Error> =
        sqlx::query_scalar("SELECT kb_bid_mutate_attachment($1,$2,'validate',3,NULL,$3,$4,$5,$6)")
            .bind(seed.project_id)
            .bind(attachment_id)
            .bind(&seed.actor)
            .bind(format!("revalidate-missing-page-{attachment_id}"))
            .bind(validate_bytes)
            .bind(validate_sha256)
            .fetch_one(&pool)
            .await;
    assert_database_error(
        validation.expect_err("validation must detect a missing frozen page owner"),
        "ATTACHMENT_RENDER_PAGE_IDENTITY_MISMATCH",
    );

    let missing_manifest_id = Uuid::new_v4();
    let missing_key = format!("missing-page-{missing_manifest_id}");
    let missing_context = storage::bidding::MutationContext::new(
        seed.actor.clone(),
        missing_key.clone(),
        &json!({"manifest_id":missing_manifest_id,"project_id":seed.project_id,"format":"docx"}),
    )
    .unwrap();
    let missing = storage::bid_submission::create_submission_manifest(
        &pool,
        missing_manifest_id,
        seed.project_id,
        "docx",
        &missing_context,
    )
    .await
    .expect_err("manifest creation must reject a missing frozen page owner");
    assert_database_error(missing, "MANIFEST_ASSET_UNAVAILABLE_OR_INVALID");
    assert_manifest_attempt_rolled_back(&pool, missing_manifest_id, &seed.actor, &missing_key)
        .await;
}

#[tokio::test]
async fn manifest_creation_rolls_back_for_missing_and_corrupt_physical_blob() {
    let Some(pool) = live_test_pool().await else {
        return;
    };
    if !support::require_final_schema("Submission", final_submission_schema_is_ready(&pool).await) {
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
    seed_current_part_with_markdown(
        &pool,
        &seed,
        "1",
        &format!("![missing asset]({object_ref})"),
    )
    .await;

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
}
