//! Submission storage adapter. Runtime callers execute only checked functions.

use image::GenericImageView;
use serde::Deserialize;
use serde_json::Value;
use sqlx::{Executor, PgPool, Postgres, Row};
use std::io::Cursor;
use uuid::Uuid;

use crate::bidding::MutationContext;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedUpload {
    pub media_type: &'static str,
    pub byte_length: i64,
    pub pixel_width: Option<i32>,
    pub pixel_height: Option<i32>,
}

#[derive(Debug)]
pub struct StoredManifestRenderAsset {
    pub object_ref: String,
    pub digest: String,
    pub media_type: String,
    pub byte_length: i64,
    pub pixel_width: Option<i32>,
    pub pixel_height: Option<i32>,
    pub source_kind: String,
    pub source_locator: Value,
    pub manifest_ordinal: i32,
    pub occurrence_ordinal: i32,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Deserialize)]
pub struct SubmissionRenderClaim {
    pub render_job_id: Uuid,
    pub project_id: Uuid,
    pub manifest_id: Uuid,
    pub expected_manifest_sha256: String,
    pub requested_by: String,
    pub idempotency_key: String,
    pub attempt_count: i32,
    pub max_attempts: i32,
    pub claim_lease_ms: i32,
}

#[derive(Debug, Deserialize)]
pub struct AttachmentPreparationClaim {
    pub preparation_job_id: Uuid,
    pub project_id: Uuid,
    pub attachment_id: Uuid,
    pub object_ref: String,
    pub content_sha256: String,
    pub byte_length: i64,
    pub attempt_count: i32,
    pub max_attempts: i32,
    pub claim_lease_ms: i32,
}

fn protocol(message: impl Into<String>) -> sqlx::Error {
    sqlx::Error::Protocol(message.into())
}

async fn load_validated_asset(digest: &str) -> Result<(Vec<u8>, ValidatedUpload), sqlx::Error> {
    let digest = digest.to_string();
    tokio::task::spawn_blocking(move || {
        let bytes = crate::read_blob(&digest).map_err(|_| "MANIFEST_ASSET_BYTES_MISSING")?;
        let metadata =
            validate_upload_bytes(&bytes, true).map_err(|_| "MANIFEST_ASSET_BYTES_INVALID")?;
        Ok::<_, &'static str>((bytes, metadata))
    })
    .await
    .map_err(|_| protocol("MANIFEST_ASSET_VALIDATION_TASK_FAILED"))?
    .map_err(protocol)
}

pub fn validate_upload_bytes(
    bytes: &[u8],
    allow_pdf: bool,
) -> Result<ValidatedUpload, sqlx::Error> {
    if bytes.is_empty() || bytes.len() > 20 * 1024 * 1024 {
        return Err(protocol("UPLOAD_SIZE_INVALID"));
    }
    if allow_pdf && bytes.starts_with(b"%PDF-") {
        let eof = bytes
            .windows(b"%%EOF".len())
            .rposition(|window| window == b"%%EOF")
            .ok_or_else(|| protocol("PDF_STRUCTURE_INVALID"))?;
        if bytes[eof + b"%%EOF".len()..]
            .iter()
            .any(|byte| !byte.is_ascii_whitespace())
        {
            return Err(protocol("PDF_STRUCTURE_INVALID"));
        }
        return Ok(ValidatedUpload {
            media_type: "application/pdf",
            byte_length: bytes.len() as i64,
            pixel_width: None,
            pixel_height: None,
        });
    }
    let format = image::guess_format(bytes).map_err(|_| protocol("IMAGE_MAGIC_INVALID"))?;
    let media_type = match format {
        image::ImageFormat::Png => "image/png",
        image::ImageFormat::Jpeg => "image/jpeg",
        image::ImageFormat::WebP => "image/webp",
        _ => return Err(protocol("IMAGE_MEDIA_TYPE_UNSUPPORTED")),
    };
    let (width, height) = image::ImageReader::with_format(Cursor::new(bytes), format)
        .into_dimensions()
        .map_err(|_| protocol("IMAGE_DIMENSIONS_INVALID"))?;
    if !(1..=20_000).contains(&width) || !(1..=20_000).contains(&height) {
        return Err(protocol("IMAGE_DIMENSIONS_INVALID"));
    }
    let pixels = u64::from(width) * u64::from(height);
    if pixels > 100_000_000 {
        return Err(protocol("IMAGE_PIXEL_QUOTA_EXCEEDED"));
    }
    let image = image::load_from_memory_with_format(bytes, format)
        .map_err(|_| protocol("IMAGE_DECODE_INVALID"))?;
    if image.dimensions() != (width, height) {
        return Err(protocol("IMAGE_DIMENSIONS_CHANGED_DURING_DECODE"));
    }
    Ok(ValidatedUpload {
        media_type,
        byte_length: bytes.len() as i64,
        pixel_width: Some(width as i32),
        pixel_height: Some(height as i32),
    })
}

pub struct UpdateCompanyProfile<'a> {
    pub project_id: Uuid,
    pub expected_revision: i64,
    pub legal_name: &'a str,
    pub uscc: &'a str,
    pub address: &'a str,
    pub legal_rep: &'a str,
    pub contact_name: &'a str,
    pub contact_phone: &'a str,
    pub contact_email: &'a str,
}

pub async fn update_company_profile(
    pool: &PgPool,
    input: UpdateCompanyProfile<'_>,
    context: &MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_bid_update_company_profile($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)",
    )
    .bind(input.project_id)
    .bind(input.expected_revision)
    .bind(input.legal_name)
    .bind(input.uscc)
    .bind(input.address)
    .bind(input.legal_rep)
    .bind(input.contact_name)
    .bind(input.contact_phone)
    .bind(input.contact_email)
    .bind(&context.actor)
    .bind(&context.idempotency_key)
    .bind(&context.request.bytes)
    .bind(&context.request.sha256)
    .fetch_one(pool)
    .await
}

pub struct UpdateSubmissionProfile<'a> {
    pub project_id: Uuid,
    pub expected_revision: i64,
    pub buyer_name: &'a str,
    pub project_code: &'a str,
    pub authorized_representative: &'a str,
    pub submission_date: chrono::NaiveDate,
    pub submission_place: &'a str,
    pub seal_confirmed: bool,
    pub signature_confirmed: bool,
}

pub async fn update_submission_profile(
    pool: &PgPool,
    input: UpdateSubmissionProfile<'_>,
    context: &MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_bid_update_submission_profile($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)",
    )
    .bind(input.project_id)
    .bind(input.expected_revision)
    .bind(input.buyer_name)
    .bind(input.project_code)
    .bind(input.authorized_representative)
    .bind(input.submission_date)
    .bind(input.submission_place)
    .bind(input.seal_confirmed)
    .bind(input.signature_confirmed)
    .bind(&context.actor)
    .bind(&context.idempotency_key)
    .bind(&context.request.bytes)
    .bind(&context.request.sha256)
    .fetch_one(pool)
    .await
}

pub async fn current_company_profile(
    pool: &PgPool,
    project_id: Uuid,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT to_jsonb(p) FROM bidding_current_company_profiles p WHERE project_id=$1",
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
}

pub async fn current_submission_profile(
    pool: &PgPool,
    project_id: Uuid,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT to_jsonb(p) FROM bidding_current_submission_profiles p WHERE project_id=$1",
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
}

pub async fn list_procedural_classifications(
    pool: &PgPool,
    project_id: Uuid,
) -> Result<Vec<Value>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT COALESCE(
            jsonb_agg(
              to_jsonb(classification) ORDER BY classification.segment_id
            ),
            '[]'::jsonb
          )
           FROM bidding_current_procedural_classifications classification
          WHERE classification.project_id=$1",
    )
    .bind(project_id)
    .fetch_one(pool)
    .await
    .map(|value: Value| value.as_array().cloned().unwrap_or_default())
}

pub async fn override_procedural_classification(
    pool: &PgPool,
    project_id: Uuid,
    classification_id: Uuid,
    effective_kind: &str,
    reason: &str,
    context: &MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_override_procedural_classification($1,$2,$3,$4,$5,$6,$7,$8)")
        .bind(project_id)
        .bind(classification_id)
        .bind(effective_kind)
        .bind(reason)
        .bind(&context.actor)
        .bind(&context.idempotency_key)
        .bind(&context.request.bytes)
        .bind(&context.request.sha256)
        .fetch_one(pool)
        .await
}

pub async fn resolve_procedural_requirement(
    pool: &PgPool,
    project_id: Uuid,
    classification_id: Uuid,
    resolution: &str,
    attachment_id: Option<Uuid>,
    reason: Option<&str>,
    context: &MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_resolve_procedural_requirement($1,$2,$3,$4,$5,$6,$7,$8,$9)")
        .bind(project_id)
        .bind(classification_id)
        .bind(resolution)
        .bind(attachment_id)
        .bind(reason)
        .bind(&context.actor)
        .bind(&context.idempotency_key)
        .bind(&context.request.bytes)
        .bind(&context.request.sha256)
        .fetch_one(pool)
        .await
}

pub struct UploadAttachment<'a> {
    pub staging_id: Uuid,
    pub id: Uuid,
    pub project_id: Uuid,
    pub kind: &'a str,
    pub object_ref: &'a str,
    pub digest: &'a str,
    pub media_type: &'a str,
    pub byte_length: i64,
    pub pixel_width: Option<i32>,
    pub pixel_height: Option<i32>,
}

pub async fn upload_attachment(
    pool: &PgPool,
    input: UploadAttachment<'_>,
    context: &MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_bid_upload_attachment($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)",
    )
    .bind(input.staging_id)
    .bind(input.id)
    .bind(input.project_id)
    .bind(input.kind)
    .bind(input.object_ref)
    .bind(input.digest)
    .bind(input.media_type)
    .bind(input.byte_length)
    .bind(input.pixel_width)
    .bind(input.pixel_height)
    .bind(&context.actor)
    .bind(&context.idempotency_key)
    .bind(&context.request.bytes)
    .bind(&context.request.sha256)
    .fetch_one(pool)
    .await
}

pub async fn claim_attachment_preparation(
    pool: &PgPool,
    preparation_job_id: Uuid,
    claim_token: Uuid,
) -> Result<Option<AttachmentPreparationClaim>, sqlx::Error> {
    let value: Option<Value> =
        sqlx::query_scalar("SELECT kb_bid_claim_attachment_preparation($1,$2)")
            .bind(preparation_job_id)
            .bind(claim_token)
            .fetch_one(pool)
            .await?;
    value
        .map(|value| serde_json::from_value(value).map_err(|error| protocol(error.to_string())))
        .transpose()
}

pub async fn heartbeat_attachment_preparation(
    pool: &PgPool,
    preparation_job_id: Uuid,
    claim_token: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_heartbeat_attachment_preparation($1,$2)")
        .bind(preparation_job_id)
        .bind(claim_token)
        .fetch_one(pool)
        .await
}

pub async fn publish_attachment_preparation(
    pool: &PgPool,
    preparation_job_id: Uuid,
    claim_token: Uuid,
    render_pages: &Value,
    actor: &str,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_publish_attachment_preparation($1,$2,$3,$4)")
        .bind(preparation_job_id)
        .bind(claim_token)
        .bind(render_pages)
        .bind(actor)
        .fetch_one(pool)
        .await
}

pub async fn fail_attachment_preparation(
    pool: &PgPool,
    preparation_job_id: Uuid,
    claim_token: Uuid,
    error_code: &str,
    error_detail: &str,
    retryable: bool,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_fail_attachment_preparation($1,$2,$3,$4,$5)")
        .bind(preparation_job_id)
        .bind(claim_token)
        .bind(error_code)
        .bind(error_detail)
        .bind(retryable)
        .fetch_one(pool)
        .await
}

pub async fn reap_attachment_preparations(pool: &PgPool) -> Result<i32, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_reap_attachment_preparations()")
        .fetch_one(pool)
        .await
}

pub async fn pending_attachment_preparations(pool: &PgPool) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar("SELECT COALESCE(kb_bid_pending_attachment_preparations(),'{}'::uuid[])")
        .fetch_one(pool)
        .await
}

pub async fn mutate_attachment(
    pool: &PgPool,
    project_id: Uuid,
    attachment_id: Uuid,
    action: &str,
    expected_revision: i32,
    reason: Option<&str>,
    context: &MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_mutate_attachment($1,$2,$3,$4,$5,$6,$7,$8,$9)")
        .bind(project_id)
        .bind(attachment_id)
        .bind(action)
        .bind(expected_revision)
        .bind(reason)
        .bind(&context.actor)
        .bind(&context.idempotency_key)
        .bind(&context.request.bytes)
        .bind(&context.request.sha256)
        .fetch_one(pool)
        .await
}

pub async fn attachment_validation_input(
    pool: &PgPool,
    project_id: Uuid,
    attachment_id: Uuid,
) -> Result<Option<(String, String, i64, Option<i32>, Option<i32>)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT content_sha256,media_type,byte_length,pixel_width,pixel_height
           FROM bidding_current_attachments WHERE project_id=$1 AND id=$2",
    )
    .bind(project_id)
    .bind(attachment_id)
    .fetch_optional(pool)
    .await
}

pub struct UploadShotArtifact<'a> {
    pub staging_id: Uuid,
    pub id: Uuid,
    pub project_id: Uuid,
    pub object_ref: &'a str,
    pub digest: &'a str,
    pub media_type: &'a str,
    pub byte_length: i64,
    pub pixel_width: i32,
    pub pixel_height: i32,
}

pub async fn upload_shot_artifact(
    pool: &PgPool,
    input: UploadShotArtifact<'_>,
    context: &MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_bid_upload_shot_artifact($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)",
    )
    .bind(input.staging_id)
    .bind(input.id)
    .bind(input.project_id)
    .bind(input.object_ref)
    .bind(input.digest)
    .bind(input.media_type)
    .bind(input.byte_length)
    .bind(input.pixel_width)
    .bind(input.pixel_height)
    .bind(&context.actor)
    .bind(&context.idempotency_key)
    .bind(&context.request.bytes)
    .bind(&context.request.sha256)
    .fetch_one(pool)
    .await
}

pub async fn replace_shot_set(
    pool: &PgPool,
    project_id: Uuid,
    expected_revision: i64,
    shot_artifact_ids: &[Uuid],
    context: &MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_replace_shot_set($1,$2,$3,$4,$5,$6,$7)")
        .bind(project_id)
        .bind(expected_revision)
        .bind(shot_artifact_ids)
        .bind(&context.actor)
        .bind(&context.idempotency_key)
        .bind(&context.request.bytes)
        .bind(&context.request.sha256)
        .fetch_one(pool)
        .await
}

pub async fn update_part(
    pool: &PgPool,
    project_id: Uuid,
    part_key: &str,
    expected_content_revision: i64,
    markdown: &[u8],
    context: &MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_update_part($1,$2,$3,$4,$5,$6,$7,$8)")
        .bind(project_id)
        .bind(part_key)
        .bind(expected_content_revision)
        .bind(markdown)
        .bind(&context.actor)
        .bind(&context.idempotency_key)
        .bind(&context.request.bytes)
        .bind(&context.request.sha256)
        .fetch_one(pool)
        .await
}

pub async fn regenerate_part(
    pool: &PgPool,
    project_id: Uuid,
    part_key: &str,
    expected_content_revision: i64,
    expected_dependency_sha256: Option<&str>,
    context: &MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_regenerate_part($1,$2,$3,$4,$5,$6,$7,$8)")
        .bind(project_id)
        .bind(part_key)
        .bind(expected_content_revision)
        .bind(expected_dependency_sha256)
        .bind(&context.actor)
        .bind(&context.idempotency_key)
        .bind(&context.request.bytes)
        .bind(&context.request.sha256)
        .fetch_one(pool)
        .await
}

pub async fn list_gate_issues(
    pool: &PgPool,
    project_id: Uuid,
    format: &str,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_list_gate_issues($1,$2)")
        .bind(project_id)
        .bind(format)
        .fetch_one(pool)
        .await
}

pub async fn manifest_render_input(
    pool: &PgPool,
    project_id: Uuid,
    manifest_id: Uuid,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_manifest_render_input($1,$2)")
        .bind(project_id)
        .bind(manifest_id)
        .fetch_one(pool)
        .await
}

pub async fn create_submission_manifest(
    pool: &PgPool,
    id: Uuid,
    project_id: Uuid,
    format: &str,
    context: &MutationContext,
) -> Result<Value, sqlx::Error> {
    let mut transaction = pool.begin().await?;
    let response: Value =
        sqlx::query_scalar("SELECT kb_bid_create_submission_manifest($1,$2,$3,$4,$5,$6,$7)")
            .bind(id)
            .bind(project_id)
            .bind(format)
            .bind(&context.actor)
            .bind(&context.idempotency_key)
            .bind(&context.request.bytes)
            .bind(&context.request.sha256)
            .fetch_one(&mut *transaction)
            .await?;
    let persisted_manifest_id = response
        .get("manifest_id")
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| protocol("SUBMISSION_MANIFEST_ID_INVALID"))?;
    let input: Value = sqlx::query_scalar("SELECT kb_bid_manifest_render_input($1,$2)")
        .bind(project_id)
        .bind(persisted_manifest_id)
        .fetch_one(&mut *transaction)
        .await?;
    let assets = input
        .get("assets")
        .and_then(Value::as_array)
        .ok_or_else(|| protocol("SUBMISSION_MANIFEST_ASSETS_INVALID"))?;
    for asset in assets {
        let asset_id = asset
            .get("id")
            .and_then(Value::as_str)
            .and_then(|value| Uuid::parse_str(value).ok())
            .ok_or_else(|| protocol("SUBMISSION_MANIFEST_ASSET_ID_INVALID"))?;
        fetch_manifest_render_asset(
            &mut *transaction,
            project_id,
            persisted_manifest_id,
            asset_id,
        )
        .await?;
    }
    transaction.commit().await?;
    Ok(response)
}

async fn fetch_manifest_render_asset<'e, E>(
    executor: E,
    project_id: Uuid,
    manifest_id: Uuid,
    asset_id: Uuid,
) -> Result<StoredManifestRenderAsset, sqlx::Error>
where
    E: Executor<'e, Database = Postgres>,
{
    let row = sqlx::query(
        "SELECT object_ref,digest,media_type,byte_length,pixel_width,pixel_height,source_kind,
                source_locator,manifest_ordinal,occurrence_ordinal
           FROM kb_bid_read_manifest_render_asset($1,$2,$3)",
    )
    .bind(project_id)
    .bind(manifest_id)
    .bind(asset_id)
    .fetch_one(executor)
    .await?;
    let object_ref: String = row.get("object_ref");
    let digest: String = row.get("digest");
    let (bytes, metadata) = load_validated_asset(&digest).await?;
    let pixel_width: Option<i32> = row.get("pixel_width");
    let pixel_height: Option<i32> = row.get("pixel_height");
    if object_ref != crate::object_ref(&digest)
        || domain::sha256_hex(&bytes) != digest
        || metadata.media_type != row.get::<String, _>("media_type")
        || metadata.byte_length != row.get::<i64, _>("byte_length")
        || metadata.pixel_width != pixel_width
        || metadata.pixel_height != pixel_height
    {
        return Err(protocol("MANIFEST_ASSET_IDENTITY_MISMATCH"));
    }
    Ok(StoredManifestRenderAsset {
        object_ref,
        digest,
        media_type: row.get("media_type"),
        byte_length: row.get("byte_length"),
        pixel_width,
        pixel_height,
        source_kind: row.get("source_kind"),
        source_locator: row.get("source_locator"),
        manifest_ordinal: row.get("manifest_ordinal"),
        occurrence_ordinal: row.get("occurrence_ordinal"),
        bytes,
    })
}

pub async fn read_manifest_render_asset(
    pool: &PgPool,
    project_id: Uuid,
    manifest_id: Uuid,
    asset_id: Uuid,
) -> Result<StoredManifestRenderAsset, sqlx::Error> {
    fetch_manifest_render_asset(pool, project_id, manifest_id, asset_id).await
}

pub async fn schedule_submission_render(
    pool: &PgPool,
    render_job_id: Uuid,
    project_id: Uuid,
    manifest_id: Uuid,
    expected_manifest_sha256: &str,
    context: &MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_schedule_submission_render($1,$2,$3,$4,$5,$6,$7,$8)")
        .bind(render_job_id)
        .bind(project_id)
        .bind(manifest_id)
        .bind(expected_manifest_sha256)
        .bind(&context.actor)
        .bind(&context.idempotency_key)
        .bind(&context.request.bytes)
        .bind(&context.request.sha256)
        .fetch_one(pool)
        .await
}

pub async fn get_submission_render_job(
    pool: &PgPool,
    project_id: Uuid,
    render_job_id: Uuid,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_get_submission_render_job($1,$2)")
        .bind(project_id)
        .bind(render_job_id)
        .fetch_one(pool)
        .await
}

pub async fn claim_submission_render(
    pool: &PgPool,
    render_job_id: Uuid,
    claim_token: Uuid,
) -> Result<Option<SubmissionRenderClaim>, sqlx::Error> {
    let value: Option<Value> = sqlx::query_scalar("SELECT kb_bid_claim_submission_render($1,$2)")
        .bind(render_job_id)
        .bind(claim_token)
        .fetch_one(pool)
        .await?;
    value
        .map(|value| serde_json::from_value(value).map_err(|error| protocol(error.to_string())))
        .transpose()
}

pub async fn heartbeat_submission_render(
    pool: &PgPool,
    render_job_id: Uuid,
    claim_token: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_heartbeat_submission_render($1,$2)")
        .bind(render_job_id)
        .bind(claim_token)
        .fetch_one(pool)
        .await
}

pub async fn fail_submission_render(
    pool: &PgPool,
    render_job_id: Uuid,
    claim_token: Uuid,
    error_code: &str,
    error_detail: &str,
    retryable: bool,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_fail_submission_render($1,$2,$3,$4,$5)")
        .bind(render_job_id)
        .bind(claim_token)
        .bind(error_code)
        .bind(error_detail)
        .bind(retryable)
        .fetch_one(pool)
        .await
}

pub async fn reap_submission_renders(pool: &PgPool) -> Result<i32, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_reap_submission_renders()")
        .fetch_one(pool)
        .await
}

pub async fn pending_submission_renders(pool: &PgPool) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar("SELECT COALESCE(kb_bid_pending_submission_renders(), '{}')")
        .fetch_one(pool)
        .await
}

pub struct PublishSubmissionOutput<'a> {
    pub staging_id: Uuid,
    pub id: Uuid,
    pub render_job_id: Uuid,
    pub claim_token: Uuid,
    pub object_ref: &'a str,
    pub digest: &'a str,
    pub byte_length: i64,
}

pub async fn publish_submission_output(
    pool: &PgPool,
    input: PublishSubmissionOutput<'_>,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_publish_submission_output($1,$2,$3,$4,$5,$6,$7)")
        .bind(input.staging_id)
        .bind(input.id)
        .bind(input.render_job_id)
        .bind(input.claim_token)
        .bind(input.object_ref)
        .bind(input.digest)
        .bind(input.byte_length)
        .fetch_one(pool)
        .await
}

pub async fn download_submission_output(
    pool: &PgPool,
    project_id: Uuid,
    output_id: Uuid,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_download_submission_output($1,$2)")
        .bind(project_id)
        .bind(output_id)
        .fetch_one(pool)
        .await
}

pub async fn required_part_keys(
    pool: &PgPool,
    project_id: Uuid,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_required_part_keys($1)")
        .bind(project_id)
        .fetch_one(pool)
        .await
}

pub async fn housekeep_end_expired(pool: &PgPool) -> Result<i32, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_housekeep_end_expired()")
        .fetch_one(pool)
        .await
}

pub async fn reclaim_stale_conversions(pool: &PgPool) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar("SELECT COALESCE(kb_bid_reclaim_stale_conversions(), '{}')")
        .fetch_one(pool)
        .await
}

pub async fn pending_conversions(pool: &PgPool) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar("SELECT COALESCE(kb_bid_pending_conversions(), '{}')")
        .fetch_one(pool)
        .await
}

pub async fn reclaim_stale_extractions(
    pool: &PgPool,
) -> Result<Vec<(Uuid, Uuid, Uuid)>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM kb_bid_reclaim_stale_extractions()")
        .fetch_all(pool)
        .await
}

pub async fn pending_extractions(pool: &PgPool) -> Result<Vec<(Uuid, Uuid, Uuid)>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM kb_bid_pending_extractions()")
        .fetch_all(pool)
        .await
}

pub async fn dirty_match_projects(pool: &PgPool) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar("SELECT COALESCE(kb_bid_dirty_match_projects(), '{}')")
        .fetch_one(pool)
        .await
}

pub async fn current_part_status(
    pool: &PgPool,
    project_id: Uuid,
) -> Result<Vec<Value>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(to_jsonb(p) ORDER BY p.part_key), '[]'::jsonb)
           FROM bidding_current_part_status p WHERE p.project_id=$1",
    )
    .bind(project_id)
    .fetch_one(pool)
    .await
    .map(|value: Value| value.as_array().cloned().unwrap_or_default())
}

pub async fn get_part(
    pool: &PgPool,
    project_id: Uuid,
    part_key: &str,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_get_part($1,$2)")
        .bind(project_id)
        .bind(part_key)
        .fetch_one(pool)
        .await
}

pub async fn list_attachments(pool: &PgPool, project_id: Uuid) -> Result<Vec<Value>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(to_jsonb(a) ORDER BY a.created_at,a.id), '[]'::jsonb)
           FROM bidding_current_attachments a WHERE a.project_id=$1",
    )
    .bind(project_id)
    .fetch_one(pool)
    .await
    .map(|value: Value| value.as_array().cloned().unwrap_or_default())
}

pub async fn list_outputs(pool: &PgPool, project_id: Uuid) -> Result<Vec<Value>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(to_jsonb(o) ORDER BY o.rendered_at DESC,o.id), '[]'::jsonb)
           FROM bidding_current_submission_outputs o WHERE o.project_id=$1",
    )
    .bind(project_id)
    .fetch_one(pool)
    .await
    .map(|value: Value| value.as_array().cloned().unwrap_or_default())
}

pub async fn current_shot_set(
    pool: &PgPool,
    project_id: Uuid,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query_scalar("SELECT to_jsonb(s) FROM bidding_current_shot_sets s WHERE s.project_id=$1")
        .bind(project_id)
        .fetch_optional(pool)
        .await
}

pub async fn clause_set_identities(
    pool: &PgPool,
    project_id: Uuid,
) -> Result<Vec<Value>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(to_jsonb(s) ORDER BY s.set_kind), '[]'::jsonb)
           FROM bidding_clause_set_identities s WHERE s.project_id=$1",
    )
    .bind(project_id)
    .fetch_one(pool)
    .await
    .map(|value: Value| value.as_array().cloned().unwrap_or_default())
}

pub async fn current_routes(pool: &PgPool, project_id: Uuid) -> Result<Vec<Value>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(to_jsonb(r) ORDER BY r.ordinal), '[]'::jsonb)
           FROM bidding_current_routes r WHERE r.project_id=$1",
    )
    .bind(project_id)
    .fetch_one(pool)
    .await
    .map(|value: Value| value.as_array().cloned().unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, Rgba, RgbaImage};

    fn tiny_png() -> Vec<u8> {
        let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(2, 3, Rgba([1, 2, 3, 255])));
        let mut output = Cursor::new(Vec::new());
        image
            .write_to(&mut output, image::ImageFormat::Png)
            .unwrap();
        output.into_inner()
    }

    #[test]
    fn upload_validation_derives_image_identity_from_bytes() {
        let bytes = tiny_png();
        let validated = validate_upload_bytes(&bytes, false).unwrap();
        assert_eq!(validated.media_type, "image/png");
        assert_eq!(validated.byte_length, bytes.len() as i64);
        assert_eq!(validated.pixel_width, Some(2));
        assert_eq!(validated.pixel_height, Some(3));
    }

    #[test]
    fn upload_validation_requires_a_complete_pdf_envelope() {
        let valid = validate_upload_bytes(b"%PDF-1.7\n%%EOF\n", true).unwrap();
        assert_eq!(valid.media_type, "application/pdf");
        assert!(
            validate_upload_bytes(b"%PDF-1.7\n", true)
                .unwrap_err()
                .to_string()
                .contains("PDF_STRUCTURE_INVALID")
        );
        assert!(
            validate_upload_bytes(b"%PDF-1.7\n%%EOF\ntrailing", true)
                .unwrap_err()
                .to_string()
                .contains("PDF_STRUCTURE_INVALID")
        );
    }

    #[test]
    fn upload_validation_does_not_accept_pdf_when_images_are_required() {
        assert!(validate_upload_bytes(b"%PDF-1.7\n%%EOF\n", false).is_err());
    }
}
