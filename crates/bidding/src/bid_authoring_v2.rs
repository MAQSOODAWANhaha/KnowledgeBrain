//! Inactive V2 authoring repository seams.
//!
//! These methods call typed SECURITY DEFINER procedures from the inactive V2
//! baseline. They are not reachable from the active V1 router or worker.

use serde_json::Value;
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrozenTenderDocumentRow {
    pub request_artifact_id: Uuid,
    pub request_revision: i64,
    pub frozen_input_sha256: String,
    pub project_id: Uuid,
    pub document_id: Uuid,
    pub document_sha256: String,
    pub role_revision_id: Uuid,
    pub role_revision_sha256: String,
    pub converter_contract_id: Uuid,
    pub converter_contract_sha256: String,
    pub file_name: String,
    pub media_type: String,
    pub original_object_ref: String,
    pub byte_length: i64,
}

pub async fn load_tender_document_process_input_v2(
    pool: &PgPool,
    request_artifact_id: Uuid,
    request_revision: i64,
    frozen_input_sha256: &str,
) -> Result<Option<FrozenTenderDocumentRow>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT typed.request_artifact_id,typed.request_revision,typed.frozen_input_sha256,
                typed.project_id,typed.document_id,typed.document_sha256,
                typed.role_revision_id,typed.role_revision_sha256,
                typed.converter_contract_id,typed.converter_contract_sha256,
                document.file_name,document.media_type,document.original_object_ref,document.byte_length
           FROM bid_tender_document_process_request_identities typed
           JOIN bid_async_request_snapshot_artifacts request_value
             ON request_value.id=typed.request_artifact_id
            AND request_value.project_id=typed.project_id
            AND request_value.request_kind='tender_document_process'
            AND request_value.revision=typed.request_revision
            AND request_value.frozen_input_sha256=typed.frozen_input_sha256
           JOIN bid_documents document
             ON document.project_id=typed.project_id
            AND document.id=typed.document_id
            AND document.original_sha256=typed.document_sha256
           JOIN bid_document_role_revision_artifacts role_value
             ON role_value.project_id=typed.project_id
            AND role_value.document_id=typed.document_id
            AND role_value.id=typed.role_revision_id
            AND role_value.content_sha256=typed.role_revision_sha256
           JOIN bid_authoring_contract_artifacts converter
             ON converter.id=typed.converter_contract_id
            AND converter.content_sha256=typed.converter_contract_sha256
            AND converter.contract_kind='converter'
           JOIN object_registry object_value
             ON object_value.object_ref=document.original_object_ref
            AND object_value.digest=document.original_sha256
            AND object_value.media_type=document.media_type
            AND object_value.byte_length=document.byte_length
            AND object_value.state='available'
          WHERE typed.request_artifact_id=$1
            AND typed.request_revision=$2
            AND typed.frozen_input_sha256=$3::kb_sha256",
    )
    .bind(request_artifact_id)
    .bind(request_revision)
    .bind(frozen_input_sha256)
    .fetch_optional(pool)
    .await?;
    row.map(|row| {
        Ok(FrozenTenderDocumentRow {
            request_artifact_id: row.try_get("request_artifact_id")?,
            request_revision: row.try_get("request_revision")?,
            frozen_input_sha256: row.try_get("frozen_input_sha256")?,
            project_id: row.try_get("project_id")?,
            document_id: row.try_get("document_id")?,
            document_sha256: row.try_get("document_sha256")?,
            role_revision_id: row.try_get("role_revision_id")?,
            role_revision_sha256: row.try_get("role_revision_sha256")?,
            converter_contract_id: row.try_get("converter_contract_id")?,
            converter_contract_sha256: row.try_get("converter_contract_sha256")?,
            file_name: row.try_get("file_name")?,
            media_type: row.try_get("media_type")?,
            original_object_ref: row.try_get("original_object_ref")?,
            byte_length: row.try_get("byte_length")?,
        })
    })
    .transpose()
}

#[derive(Debug, Clone)]
pub struct PublishTenderDocumentProcessV2<'a> {
    pub request_artifact_id: Uuid,
    pub request_revision: i64,
    pub frozen_input_sha256: &'a str,
    pub project_id: Uuid,
    pub document_id: Uuid,
    pub document_sha256: &'a str,
    pub source: &'a Value,
    pub images: &'a Value,
    pub units: &'a Value,
    pub actor: &'a str,
}

pub async fn publish_tender_document_process_v2(
    pool: &PgPool,
    input: PublishTenderDocumentProcessV2<'_>,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_bid_v2_publish_tender_document_process(
           $1,$2,$3::kb_sha256,$4,$5,$6::kb_sha256,$7,$8,$9,$10::kb_actor_identity
         )",
    )
    .bind(input.request_artifact_id)
    .bind(input.request_revision)
    .bind(input.frozen_input_sha256)
    .bind(input.project_id)
    .bind(input.document_id)
    .bind(input.document_sha256)
    .bind(input.source)
    .bind(input.images)
    .bind(input.units)
    .bind(input.actor)
    .fetch_one(pool)
    .await
}

pub async fn create_project_v2(
    pool: &PgPool,
    id: Uuid,
    title: &str,
    owner_user_id: Uuid,
    context: &crate::mutation::MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_bid_v2_create_project($1,$2,$3,$4::kb_actor_identity,$5,$6,$7::kb_sha256)",
    )
    .bind(id)
    .bind(title)
    .bind(owner_user_id)
    .bind(&context.actor)
    .bind(&context.idempotency_key)
    .bind(&context.request.bytes)
    .bind(&context.request.sha256)
    .fetch_one(pool)
    .await
}

pub async fn list_projects_v2(
    pool: &PgPool,
    owner_user_id: Uuid,
    actor: &str,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_list_projects($1,$2::kb_actor_identity)")
        .bind(owner_user_id)
        .bind(actor)
        .fetch_one(pool)
        .await
}

pub async fn get_project_v2(
    pool: &PgPool,
    project_id: Uuid,
    actor: &str,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_get_project($1,$2::kb_actor_identity)")
        .bind(project_id)
        .bind(actor)
        .fetch_optional(pool)
        .await
        .map(Option::flatten)
}

pub async fn end_project_v2(
    pool: &PgPool,
    project_id: Uuid,
    context: &crate::mutation::MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_end_project($1,$2::kb_actor_identity,$3,$4,$5::kb_sha256)")
        .bind(project_id)
        .bind(&context.actor)
        .bind(&context.idempotency_key)
        .bind(&context.request.bytes)
        .bind(&context.request.sha256)
        .fetch_one(pool)
        .await
}

#[derive(Debug, Clone)]
pub struct UploadTenderDocumentV2<'a> {
    pub staging_id: Uuid,
    pub document_id: Uuid,
    pub request_artifact_id: Uuid,
    pub project_id: Uuid,
    pub file_name: &'a str,
    pub media_type: &'a str,
    pub byte_length: i64,
    pub object_ref: &'a str,
    pub original_sha256: &'a str,
}

pub async fn upload_tender_document_v2(
    pool: &PgPool,
    input: UploadTenderDocumentV2<'_>,
    context: &crate::mutation::MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_bid_v2_upload_tender_document(
          $1,$2,$3,$4,$5,$6,$7,$8::kb_object_ref,$9::kb_sha256,
          $10::kb_actor_identity,$11,$12,$13::kb_sha256)",
    )
    .bind(input.staging_id)
    .bind(input.document_id)
    .bind(input.request_artifact_id)
    .bind(input.project_id)
    .bind(input.file_name)
    .bind(input.media_type)
    .bind(input.byte_length)
    .bind(input.object_ref)
    .bind(input.original_sha256)
    .bind(&context.actor)
    .bind(&context.idempotency_key)
    .bind(&context.request.bytes)
    .bind(&context.request.sha256)
    .fetch_one(pool)
    .await
}

pub async fn list_tender_documents_v2(
    pool: &PgPool,
    project_id: Uuid,
    actor: &str,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_list_tender_documents($1,$2::kb_actor_identity)")
        .bind(project_id)
        .bind(actor)
        .fetch_one(pool)
        .await
}

pub async fn patch_document_role_v2(
    pool: &PgPool,
    project_id: Uuid,
    document_id: Uuid,
    role: &str,
    expected_artifact_id: Uuid,
    expected_sha256: &str,
    context: &crate::mutation::MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_bid_v2_patch_document_role(
          $1,$2,$3,$4,$5::kb_sha256,$6::kb_actor_identity,$7,$8,$9::kb_sha256)",
    )
    .bind(project_id)
    .bind(document_id)
    .bind(role)
    .bind(expected_artifact_id)
    .bind(expected_sha256)
    .bind(&context.actor)
    .bind(&context.idempotency_key)
    .bind(&context.request.bytes)
    .bind(&context.request.sha256)
    .fetch_one(pool)
    .await
}

#[derive(Debug, Clone)]
pub struct UpsertDocumentRelationV2<'a> {
    pub project_id: Uuid,
    pub lineage_id: Uuid,
    pub from_document_id: Uuid,
    pub to_document_id: Uuid,
    pub relation_kind: &'a str,
    pub applicability: &'a Value,
    pub expected_artifact_id: Option<Uuid>,
    pub expected_sha256: Option<&'a str>,
}

pub async fn upsert_document_relation_v2(
    pool: &PgPool,
    input: UpsertDocumentRelationV2<'_>,
    context: &crate::mutation::MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_bid_v2_upsert_document_relation(
          $1,$2,$3,$4,$5,$6,$7,$8::kb_sha256,$9::kb_actor_identity,$10,$11,$12::kb_sha256)",
    )
    .bind(input.project_id)
    .bind(input.lineage_id)
    .bind(input.from_document_id)
    .bind(input.to_document_id)
    .bind(input.relation_kind)
    .bind(input.applicability)
    .bind(input.expected_artifact_id)
    .bind(input.expected_sha256)
    .bind(&context.actor)
    .bind(&context.idempotency_key)
    .bind(&context.request.bytes)
    .bind(&context.request.sha256)
    .fetch_one(pool)
    .await
}

pub async fn list_document_relations_v2(
    pool: &PgPool,
    project_id: Uuid,
    actor: &str,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_list_document_relations($1,$2::kb_actor_identity)")
        .bind(project_id)
        .bind(actor)
        .fetch_one(pool)
        .await
}

pub async fn freeze_document_set_v2(
    pool: &PgPool,
    project_id: Uuid,
    document_ids: &[Uuid],
    expected_artifact_id: Option<Uuid>,
    expected_sha256: Option<&str>,
    request_artifact_id: Uuid,
    context: &crate::mutation::MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_bid_v2_freeze_document_set(
          $1,$2,$3,$4::kb_sha256,$5,$6::kb_actor_identity,$7,$8,$9::kb_sha256)",
    )
    .bind(project_id)
    .bind(document_ids)
    .bind(expected_artifact_id)
    .bind(expected_sha256)
    .bind(request_artifact_id)
    .bind(&context.actor)
    .bind(&context.idempotency_key)
    .bind(&context.request.bytes)
    .bind(&context.request.sha256)
    .fetch_one(pool)
    .await
}

pub async fn publish_disposition_set_v2(
    pool: &PgPool,
    project_id: Uuid,
    document_set_id: Uuid,
    items: &Value,
    expected_artifact_id: Uuid,
    expected_sha256: &str,
    request_artifact_id: Uuid,
    context: &crate::mutation::MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_bid_v2_publish_disposition_set(
          $1,$2,$3,$4,$5::kb_sha256,$6,$7::kb_actor_identity,$8,$9,$10::kb_sha256)",
    )
    .bind(project_id)
    .bind(document_set_id)
    .bind(items)
    .bind(expected_artifact_id)
    .bind(expected_sha256)
    .bind(request_artifact_id)
    .bind(&context.actor)
    .bind(&context.idempotency_key)
    .bind(&context.request.bytes)
    .bind(&context.request.sha256)
    .fetch_one(pool)
    .await
}

pub async fn list_source_units_v2(
    pool: &PgPool,
    project_id: Uuid,
    actor: &str,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_list_source_units($1,$2::kb_actor_identity)")
        .bind(project_id)
        .bind(actor)
        .fetch_one(pool)
        .await
}

pub async fn list_requirements_v2(
    pool: &PgPool,
    project_id: Uuid,
    actor: &str,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_list_requirements($1,$2::kb_actor_identity)")
        .bind(project_id)
        .bind(actor)
        .fetch_one(pool)
        .await
}

pub async fn load_workspace_v2(
    pool: &PgPool,
    workspace_id: Uuid,
    actor: &str,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_load_workspace_for_actor($1,$2::kb_actor_identity)")
        .bind(workspace_id)
        .bind(actor)
        .fetch_optional(pool)
        .await
        .map(Option::flatten)
}

pub async fn commit_workspace_mutation_v2(
    pool: &PgPool,
    workspace_id: Uuid,
    expected_revision_id: Uuid,
    expected_sha256: &str,
    snapshot: &Value,
    context: &crate::mutation::MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_bid_v2_commit_workspace_mutation_idempotent(
          $1,$2,$3::kb_sha256,$4,$5::kb_actor_identity,$6,$7,$8::kb_sha256)",
    )
    .bind(workspace_id)
    .bind(expected_revision_id)
    .bind(expected_sha256)
    .bind(snapshot)
    .bind(&context.actor)
    .bind(&context.idempotency_key)
    .bind(&context.request.bytes)
    .bind(&context.request.sha256)
    .fetch_one(pool)
    .await
}

pub async fn create_outline_candidate_v2(
    pool: &PgPool,
    workspace_id: Uuid,
    expected_revision_id: Uuid,
    expected_sha256: &str,
    document_set_id: Uuid,
    document_set_sha256: &str,
    context: &crate::mutation::MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_bid_v2_create_outline_candidate(
          $1,$2,$3::kb_sha256,$4,$5::kb_sha256,$6::kb_actor_identity,$7,$8,$9::kb_sha256)",
    )
    .bind(workspace_id)
    .bind(expected_revision_id)
    .bind(expected_sha256)
    .bind(document_set_id)
    .bind(document_set_sha256)
    .bind(&context.actor)
    .bind(&context.idempotency_key)
    .bind(&context.request.bytes)
    .bind(&context.request.sha256)
    .fetch_one(pool)
    .await
}

pub async fn get_async_request_v2(
    pool: &PgPool,
    workspace_id: Uuid,
    request_id: Uuid,
    actor: &str,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_get_async_request($1,$2,$3::kb_actor_identity)")
        .bind(workspace_id)
        .bind(request_id)
        .bind(actor)
        .fetch_optional(pool)
        .await
        .map(Option::flatten)
}

pub async fn get_candidate_v2(
    pool: &PgPool,
    workspace_id: Uuid,
    candidate_id: Uuid,
    actor: &str,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_get_candidate($1,$2,$3::kb_actor_identity)")
        .bind(workspace_id)
        .bind(candidate_id)
        .bind(actor)
        .fetch_optional(pool)
        .await
        .map(Option::flatten)
}

pub async fn accept_candidate_v2(
    pool: &PgPool,
    workspace_id: Uuid,
    candidate_id: Uuid,
    expected_revision_id: Uuid,
    expected_sha256: &str,
    snapshot: &Value,
    selected_ordinals: &[i32],
    context: &crate::mutation::MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_bid_v2_accept_candidate(
          $1,$2,$3,$4::kb_sha256,$5,$6,$7::kb_actor_identity,$8,$9,$10::kb_sha256)",
    )
    .bind(workspace_id)
    .bind(candidate_id)
    .bind(expected_revision_id)
    .bind(expected_sha256)
    .bind(snapshot)
    .bind(selected_ordinals)
    .bind(&context.actor)
    .bind(&context.idempotency_key)
    .bind(&context.request.bytes)
    .bind(&context.request.sha256)
    .fetch_one(pool)
    .await
}

pub async fn reject_candidate_v2(
    pool: &PgPool,
    workspace_id: Uuid,
    candidate_id: Uuid,
    context: &crate::mutation::MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_bid_v2_reject_candidate($1,$2,$3::kb_actor_identity,$4,$5,$6::kb_sha256)",
    )
    .bind(workspace_id)
    .bind(candidate_id)
    .bind(&context.actor)
    .bind(&context.idempotency_key)
    .bind(&context.request.bytes)
    .bind(&context.request.sha256)
    .fetch_one(pool)
    .await
}

pub async fn load_authoring_job_payload_v2(
    pool: &PgPool,
    request_artifact_id: Uuid,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT convert_from(request_payload,'UTF8')::jsonb
           FROM bid_async_request_snapshot_artifacts WHERE id=$1",
    )
    .bind(request_artifact_id)
    .fetch_optional(pool)
    .await
}

pub async fn compile_requirement_set_v2(
    pool: &PgPool,
    request_artifact_id: Uuid,
    request_revision: i64,
    frozen_input_sha256: &str,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_bid_v2_compile_requirement_set($1,$2,$3::kb_sha256,$4::kb_actor_identity)",
    )
    .bind(request_artifact_id)
    .bind(request_revision)
    .bind(frozen_input_sha256)
    .bind("system:requirement-set-compile-v2")
    .fetch_one(pool)
    .await
}

pub async fn mark_tender_document_failed_v2(
    pool: &PgPool,
    request_artifact_id: Uuid,
    error_code: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT kb_bid_v2_mark_tender_document_failed($1,$2)")
        .bind(request_artifact_id)
        .bind(error_code)
        .execute(pool)
        .await
        .map(|_| ())
}
