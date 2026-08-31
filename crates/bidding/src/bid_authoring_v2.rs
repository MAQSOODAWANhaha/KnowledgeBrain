//! V2 authoring repository seams.
//!
//! These methods call typed SECURITY DEFINER procedures from the active clean-slate
//! V2 baseline.

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
        "SELECT * FROM kb_bid_v2_load_tender_document_process_input(
           $1,$2,$3::kb_sha256)",
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

pub async fn retry_tender_document_v2(
    pool: &PgPool,
    project_id: Uuid,
    document_id: Uuid,
    request_artifact_id: Uuid,
    expected_generation: i64,
    context: &crate::mutation::MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_bid_v2_retry_tender_document($1,$2,$3,$4,$5::kb_actor_identity,$6,$7,$8::kb_sha256)",
    )
    .bind(project_id)
    .bind(document_id)
    .bind(request_artifact_id)
    .bind(expected_generation)
    .bind(&context.actor)
    .bind(&context.idempotency_key)
    .bind(&context.request.bytes)
    .bind(&context.request.sha256)
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

pub async fn next_quote_snapshot_revision_v2(
    pool: &PgPool,
    project_id: Uuid,
    actor: &str,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_next_quote_snapshot_revision($1,$2::kb_actor_identity)")
        .bind(project_id)
        .bind(actor)
        .fetch_one(pool)
        .await
}

pub struct PublishQuoteSnapshotV2<'a> {
    pub project_id: Uuid,
    pub snapshot_id: Uuid,
    pub expected_revision: i64,
    pub staging_id: Uuid,
    pub object_ref: &'a str,
    pub content_sha256: &'a str,
    pub canonical_payload: &'a [u8],
}

pub async fn publish_quote_snapshot_v2(
    pool: &PgPool,
    input: PublishQuoteSnapshotV2<'_>,
    context: &crate::mutation::MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_publish_quote_snapshot($1,$2,$3,$4,$5::kb_object_ref,$6::kb_sha256,$7,$8,$9::kb_actor_identity,$10,$11,$12::kb_sha256)")
        .bind(input.project_id).bind(input.snapshot_id).bind(input.expected_revision).bind(input.staging_id)
        .bind(input.object_ref).bind(input.content_sha256).bind(input.canonical_payload.len() as i64)
        .bind(input.canonical_payload).bind(&context.actor).bind(&context.idempotency_key)
        .bind(&context.request.bytes).bind(&context.request.sha256).fetch_one(pool).await
}

pub async fn list_quote_snapshots_v2(
    pool: &PgPool,
    project_id: Uuid,
    actor: &str,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_list_quote_snapshots($1,$2::kb_actor_identity)")
        .bind(project_id)
        .bind(actor)
        .fetch_one(pool)
        .await
}

pub async fn get_quote_snapshot_v2(
    pool: &PgPool,
    project_id: Uuid,
    snapshot_id: Uuid,
    actor: &str,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_get_quote_snapshot($1,$2,$3::kb_actor_identity)")
        .bind(project_id)
        .bind(snapshot_id)
        .bind(actor)
        .fetch_one(pool)
        .await
}

pub async fn list_document_sets_v2(
    pool: &PgPool,
    project_id: Uuid,
    actor: &str,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_list_document_sets($1,$2::kb_actor_identity)")
        .bind(project_id)
        .bind(actor)
        .fetch_one(pool)
        .await
}

pub async fn get_document_set_v2(
    pool: &PgPool,
    project_id: Uuid,
    document_set_id: Uuid,
    actor: &str,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_get_document_set($1,$2,$3::kb_actor_identity)")
        .bind(project_id)
        .bind(document_set_id)
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
    expected: (Uuid, &str),
    request_artifact_id: Uuid,
    context: &crate::mutation::MutationContext,
) -> Result<Value, sqlx::Error> {
    let (expected_artifact_id, expected_sha256) = expected;
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

pub async fn list_structured_forms_v2(
    pool: &PgPool,
    project_id: Uuid,
    actor: &str,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_list_structured_forms($1,$2::kb_actor_identity)")
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

#[derive(Debug)]
pub struct PatchRequirementV2<'a> {
    pub project_id: Uuid,
    pub requirement_revision_id: Uuid,
    pub expected_set_id: Uuid,
    pub expected_set_sha256: &'a str,
    pub requirement_kind: &'a str,
    pub requiredness: &'a str,
    pub compliance_policy: &'a str,
    pub lifecycle: &'a str,
    pub text: &'a str,
    pub fulfillment_expr: &'a Value,
    pub applicability: &'a Value,
}

pub async fn patch_requirement_v2(
    pool: &PgPool,
    input: PatchRequirementV2<'_>,
    context: &crate::mutation::MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_bid_v2_patch_requirement(
          $1,$2,$3,$4::kb_sha256,$5,$6,$7,$8,$9,$10,$11,
          $12::kb_actor_identity,$13,$14,$15::kb_sha256)",
    )
    .bind(input.project_id)
    .bind(input.requirement_revision_id)
    .bind(input.expected_set_id)
    .bind(input.expected_set_sha256)
    .bind(input.requirement_kind)
    .bind(input.requiredness)
    .bind(input.compliance_policy)
    .bind(input.lifecycle)
    .bind(input.text)
    .bind(input.fulfillment_expr)
    .bind(input.applicability)
    .bind(&context.actor)
    .bind(&context.idempotency_key)
    .bind(&context.request.bytes)
    .bind(&context.request.sha256)
    .fetch_one(pool)
    .await
}

#[derive(Debug)]
pub struct PublishRequirementSupersessionV2<'a> {
    pub project_id: Uuid,
    pub lineage_id: Uuid,
    pub old_requirement_revision_id: Uuid,
    pub new_requirement_revision_id: Uuid,
    pub applicability: &'a Value,
    pub tombstone: bool,
    pub expected_artifact_id: Option<Uuid>,
    pub expected_sha256: Option<&'a str>,
}

pub async fn publish_requirement_supersession_v2(
    pool: &PgPool,
    input: PublishRequirementSupersessionV2<'_>,
    context: &crate::mutation::MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_bid_v2_publish_requirement_supersession(
          $1,$2,$3,$4,$5,$6,$7,$8::kb_sha256,$9::kb_actor_identity,$10,$11,$12::kb_sha256)",
    )
    .bind(input.project_id)
    .bind(input.lineage_id)
    .bind(input.old_requirement_revision_id)
    .bind(input.new_requirement_revision_id)
    .bind(input.applicability)
    .bind(input.tombstone)
    .bind(input.expected_artifact_id)
    .bind(input.expected_sha256)
    .bind(&context.actor)
    .bind(&context.idempotency_key)
    .bind(&context.request.bytes)
    .bind(&context.request.sha256)
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

pub async fn load_outline_generation_input_v2(
    pool: &PgPool,
    request_artifact_id: Uuid,
    request_revision: i64,
    frozen_input_sha256: &str,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_load_outline_generation_input($1,$2,$3::kb_sha256)")
        .bind(request_artifact_id)
        .bind(request_revision)
        .bind(frozen_input_sha256)
        .fetch_one(pool)
        .await
}

pub async fn publish_outline_generation_v2(
    pool: &PgPool,
    request: &platform::BidAuthoringRequestIdentityV2,
    candidate: (Uuid, &[u8], &str),
    nodes: &Value,
) -> Result<Value, sqlx::Error> {
    let (candidate_id, candidate_payload, candidate_sha256) = candidate;
    sqlx::query_scalar(
        "SELECT kb_bid_v2_publish_outline_generation($1,$2,$3::kb_sha256,$4,$5,$6::kb_sha256,$7)",
    )
    .bind(request.request_artifact_id)
    .bind(request.request_revision)
    .bind(&request.frozen_input_sha256)
    .bind(candidate_id)
    .bind(candidate_payload)
    .bind(candidate_sha256)
    .bind(nodes)
    .fetch_one(pool)
    .await
}

pub async fn mark_outline_generation_failed_v2(
    pool: &PgPool,
    request_artifact_id: Uuid,
    request_revision: i64,
    frozen_input_sha256: &str,
    error_code: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT kb_bid_v2_mark_outline_generation_failed($1,$2,$3::kb_sha256,$4)")
        .bind(request_artifact_id)
        .bind(request_revision)
        .bind(frozen_input_sha256)
        .bind(error_code)
        .execute(pool)
        .await
        .map(|_| ())
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

pub async fn list_workspace_async_requests_v2(
    pool: &PgPool,
    workspace_id: Uuid,
    actor: &str,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_list_workspace_async_requests($1,$2::kb_actor_identity)")
        .bind(workspace_id)
        .bind(actor)
        .fetch_one(pool)
        .await
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
    expected: (Uuid, &str),
    snapshot: &Value,
    selected_ordinals: &[i32],
    context: &crate::mutation::MutationContext,
) -> Result<Value, sqlx::Error> {
    let (expected_revision_id, expected_sha256) = expected;
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

pub async fn get_requirement_projection_v2(
    pool: &PgPool,
    workspace_id: Uuid,
    actor: &str,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_get_requirement_projection($1,$2::kb_actor_identity)")
        .bind(workspace_id)
        .bind(actor)
        .fetch_one(pool)
        .await
}

pub async fn refresh_requirement_projection_v2(
    pool: &PgPool,
    workspace_id: Uuid,
    expected_artifact_id: Uuid,
    expected_sha256: &str,
    context: &crate::mutation::MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_refresh_requirement_projection($1,$2,$3::kb_sha256,$4::kb_actor_identity,$5,$6,$7::kb_sha256)")
        .bind(workspace_id).bind(expected_artifact_id).bind(expected_sha256).bind(&context.actor)
        .bind(&context.idempotency_key).bind(&context.request.bytes).bind(&context.request.sha256)
        .fetch_one(pool).await
}

pub async fn list_workspace_assets_v2(
    pool: &PgPool,
    workspace_id: Uuid,
    actor: &str,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_list_workspace_assets($1,$2::kb_actor_identity)")
        .bind(workspace_id)
        .bind(actor)
        .fetch_one(pool)
        .await
}

#[derive(Debug, Clone, Copy)]
pub struct UploadWorkspaceAssetV2<'a> {
    pub workspace_id: Uuid,
    pub asset_id: Uuid,
    pub staging_id: Uuid,
    pub file_name: &'a str,
    pub media_type: &'a str,
    pub byte_length: i64,
    pub object_ref: &'a str,
    pub content_sha256: &'a str,
}

pub async fn upload_workspace_asset_v2(
    pool: &PgPool,
    input: UploadWorkspaceAssetV2<'_>,
    context: &crate::mutation::MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_bid_v2_upload_workspace_asset(
          $1,$2,$3,$4,$5,$6,$7::kb_object_ref,$8::kb_sha256,
          $9::kb_actor_identity,$10,$11,$12::kb_sha256)",
    )
    .bind(input.workspace_id)
    .bind(input.asset_id)
    .bind(input.staging_id)
    .bind(input.file_name)
    .bind(input.media_type)
    .bind(input.byte_length)
    .bind(input.object_ref)
    .bind(input.content_sha256)
    .bind(&context.actor)
    .bind(&context.idempotency_key)
    .bind(&context.request.bytes)
    .bind(&context.request.sha256)
    .fetch_one(pool)
    .await
}

pub async fn prepare_workspace_attachment_v2(
    pool: &PgPool,
    workspace_id: Uuid,
    source_asset_revision_id: Uuid,
    page_source_asset_ids: &[Uuid],
    widths_px: &[i32],
    heights_px: &[i32],
    context: &crate::mutation::MutationContext,
) -> Result<Value, sqlx::Error> {
    let page_item_ids = (0..page_source_asset_ids.len())
        .map(|_| Uuid::new_v4())
        .collect::<Vec<_>>();
    sqlx::query_scalar(
        "SELECT kb_bid_v2_prepare_workspace_attachment(
        $1,$2,$3,$4,$5,$6,$7,$8::kb_actor_identity,$9,$10,$11::kb_sha256)",
    )
    .bind(workspace_id)
    .bind(source_asset_revision_id)
    .bind(Uuid::new_v4())
    .bind(page_source_asset_ids)
    .bind(&page_item_ids)
    .bind(widths_px)
    .bind(heights_px)
    .bind(&context.actor)
    .bind(&context.idempotency_key)
    .bind(&context.request.bytes)
    .bind(&context.request.sha256)
    .fetch_one(pool)
    .await
}

#[derive(Debug)]
pub struct PublishPdfAttachmentPreparationV2<'a> {
    pub request_artifact_id: Uuid,
    pub request_revision: i64,
    pub frozen_input_sha256: &'a str,
    pub source_asset_revision_id: Uuid,
    pub preparation_id: Uuid,
    pub page_item_ids: &'a [Uuid],
    pub staging_ids: &'a [Uuid],
    pub object_refs: &'a [String],
    pub content_sha256s: &'a [String],
    pub media_types: &'a [String],
    pub byte_lengths: &'a [i64],
    pub widths_px: &'a [i32],
    pub heights_px: &'a [i32],
}

pub async fn publish_pdf_attachment_preparation_v2(
    pool: &PgPool,
    input: PublishPdfAttachmentPreparationV2<'_>,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_bid_v2_publish_pdf_attachment_preparation(
        $1,$2,$3::kb_sha256,$4,$5,$6,$7,$8::kb_object_ref[],$9::kb_sha256[],$10,
        $11,$12,$13,$14::kb_actor_identity)",
    )
    .bind(input.request_artifact_id)
    .bind(input.request_revision)
    .bind(input.frozen_input_sha256)
    .bind(input.source_asset_revision_id)
    .bind(input.preparation_id)
    .bind(input.page_item_ids)
    .bind(input.staging_ids)
    .bind(input.object_refs)
    .bind(input.content_sha256s)
    .bind(input.media_types)
    .bind(input.byte_lengths)
    .bind(input.widths_px)
    .bind(input.heights_px)
    .bind("system:submission-export-v2")
    .fetch_one(pool)
    .await
}

pub async fn retire_workspace_asset_v2(
    pool: &PgPool,
    workspace_id: Uuid,
    asset_revision_id: Uuid,
    reason: &str,
    context: &crate::mutation::MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_retire_workspace_asset($1,$2,$3,$4::kb_actor_identity,$5,$6,$7::kb_sha256)")
        .bind(workspace_id).bind(asset_revision_id).bind(reason).bind(&context.actor)
        .bind(&context.idempotency_key).bind(&context.request.bytes).bind(&context.request.sha256)
        .fetch_one(pool).await
}

pub async fn create_outline_checkpoint_v2(
    pool: &PgPool,
    workspace_id: Uuid,
    expected_revision_id: Uuid,
    expected_sha256: &str,
    checkpoint_id: Uuid,
    context: &crate::mutation::MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_bid_v2_create_outline_checkpoint(
          $1,$2,$3::kb_sha256,$4,$5::kb_actor_identity,$6,$7,$8::kb_sha256)",
    )
    .bind(workspace_id)
    .bind(expected_revision_id)
    .bind(expected_sha256)
    .bind(checkpoint_id)
    .bind(&context.actor)
    .bind(&context.idempotency_key)
    .bind(&context.request.bytes)
    .bind(&context.request.sha256)
    .fetch_one(pool)
    .await
}

#[derive(Debug, Clone)]
pub struct CreateContentRequestV2<'a> {
    pub workspace_id: Uuid,
    pub expected_revision_id: Uuid,
    pub expected_sha256: &'a str,
    pub operation: &'a str,
    pub target_kind: &'a str,
    pub target_node_lineage_id: Option<Uuid>,
    pub fill_policy: &'a str,
    pub insertion_anchor: Option<&'a Value>,
    pub evidence_selection_mode: &'a str,
    pub pick_set_artifact_id: Option<Uuid>,
}

pub async fn create_content_request_v2(
    pool: &PgPool,
    input: CreateContentRequestV2<'_>,
    context: &crate::mutation::MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_bid_v2_create_content_request(
          $1,$2,$3::kb_sha256,$4,$5,$6,$7,$8,$9,$10,
          $11::kb_actor_identity,$12,$13,$14::kb_sha256)",
    )
    .bind(input.workspace_id)
    .bind(input.expected_revision_id)
    .bind(input.expected_sha256)
    .bind(input.operation)
    .bind(input.target_kind)
    .bind(input.target_node_lineage_id)
    .bind(input.fill_policy)
    .bind(input.insertion_anchor)
    .bind(input.evidence_selection_mode)
    .bind(input.pick_set_artifact_id)
    .bind(&context.actor)
    .bind(&context.idempotency_key)
    .bind(&context.request.bytes)
    .bind(&context.request.sha256)
    .fetch_one(pool)
    .await
}

pub async fn load_content_generation_input_v2(
    pool: &PgPool,
    request_artifact_id: Uuid,
    request_revision: i64,
    frozen_input_sha256: &str,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_load_content_generation_input($1,$2,$3::kb_sha256)")
        .bind(request_artifact_id)
        .bind(request_revision)
        .bind(frozen_input_sha256)
        .fetch_one(pool)
        .await
}

pub async fn publish_content_generation_v2(
    pool: &PgPool,
    request: &platform::BidAuthoringRequestIdentityV2,
    attestation: (Uuid, &str),
    matches: &Value,
    candidate: Option<(Uuid, &[u8], &str)>,
    operations: &Value,
) -> Result<Value, sqlx::Error> {
    let (attestation_id, attestation_sha256) = attestation;
    let (candidate_id, candidate_payload, candidate_sha256) = candidate
        .map(|(id, payload, sha256)| (Some(id), Some(payload), Some(sha256)))
        .unwrap_or((None, None, None));
    sqlx::query_scalar(
        "SELECT kb_bid_v2_publish_content_generation(
          $1,$2,$3::kb_sha256,$4,$5::kb_sha256,$6,$7,$8,$9::kb_sha256,$10)",
    )
    .bind(request.request_artifact_id)
    .bind(request.request_revision)
    .bind(&request.frozen_input_sha256)
    .bind(attestation_id)
    .bind(attestation_sha256)
    .bind(matches)
    .bind(candidate_id)
    .bind(candidate_payload)
    .bind(candidate_sha256)
    .bind(operations)
    .fetch_one(pool)
    .await
}

pub async fn mark_content_generation_failed_v2(
    pool: &PgPool,
    request_artifact_id: Uuid,
    request_revision: i64,
    frozen_input_sha256: &str,
    error_code: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT kb_bid_v2_mark_content_generation_failed($1,$2,$3::kb_sha256,$4)")
        .bind(request_artifact_id)
        .bind(request_revision)
        .bind(frozen_input_sha256)
        .bind(error_code)
        .execute(pool)
        .await
        .map(|_| ())
}

pub async fn create_evidence_pick_set_v2(
    pool: &PgPool,
    workspace_id: Uuid,
    matching_report_id: Uuid,
    selected_item_ids: &[Uuid],
    context: &crate::mutation::MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_create_evidence_pick_set($1,$2,$3,$4::kb_actor_identity,$5,$6,$7::kb_sha256)")
        .bind(workspace_id).bind(matching_report_id).bind(selected_item_ids)
        .bind(&context.actor).bind(&context.idempotency_key).bind(&context.request.bytes)
        .bind(&context.request.sha256).fetch_one(pool).await
}

pub async fn create_node_evidence_pick_set_v2(
    pool: &PgPool,
    workspace_id: Uuid,
    node_lineage_id: Uuid,
    matching_report_id: Uuid,
    selected_item_ids: &[Uuid],
    context: &crate::mutation::MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_create_node_evidence_pick_set($1,$2,$3,$4,$5::kb_actor_identity,$6,$7,$8::kb_sha256)")
        .bind(workspace_id).bind(node_lineage_id).bind(matching_report_id).bind(selected_item_ids)
        .bind(&context.actor).bind(&context.idempotency_key).bind(&context.request.bytes)
        .bind(&context.request.sha256).fetch_one(pool).await
}

pub async fn get_node_evidence_v2(
    pool: &PgPool,
    workspace_id: Uuid,
    node_lineage_id: Uuid,
    actor: &str,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_get_node_evidence($1,$2,$3::kb_actor_identity)")
        .bind(workspace_id)
        .bind(node_lineage_id)
        .bind(actor)
        .fetch_one(pool)
        .await
}

pub async fn list_evidence_pick_sets_v2(
    pool: &PgPool,
    workspace_id: Uuid,
    actor: &str,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_list_evidence_pick_sets($1,$2::kb_actor_identity)")
        .bind(workspace_id)
        .bind(actor)
        .fetch_one(pool)
        .await
}

pub async fn get_evidence_overview_v2(
    pool: &PgPool,
    workspace_id: Uuid,
    actor: &str,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_get_evidence_overview($1,$2::kb_actor_identity)")
        .bind(workspace_id)
        .bind(actor)
        .fetch_one(pool)
        .await
}

pub async fn get_current_assessments_v2(
    pool: &PgPool,
    workspace_id: Uuid,
    actor: &str,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_get_current_assessments($1,$2::kb_actor_identity)")
        .bind(workspace_id)
        .bind(actor)
        .fetch_one(pool)
        .await
}

pub async fn get_preview_html_v2(
    pool: &PgPool,
    workspace_id: Uuid,
    actor: &str,
) -> Result<(String, String), sqlx::Error> {
    let input: Value =
        sqlx::query_scalar("SELECT kb_bid_v2_load_preview_input($1,$2::kb_actor_identity)")
            .bind(workspace_id)
            .bind(actor)
            .fetch_one(pool)
            .await?;
    let workspace = input
        .get("workspace")
        .ok_or_else(|| sqlx::Error::Protocol("preview workspace missing".into()))?;
    let title = input
        .get("title")
        .and_then(Value::as_str)
        .ok_or_else(|| sqlx::Error::Protocol("preview title missing".into()))?;
    let etag = workspace
        .get("sha256")
        .and_then(Value::as_str)
        .ok_or_else(|| sqlx::Error::Protocol("preview workspace digest missing".into()))?
        .to_owned();
    let mut assets = Vec::new();
    for value in input
        .get("assets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let object_ref = value
            .get("object_ref")
            .and_then(Value::as_str)
            .ok_or_else(|| sqlx::Error::Protocol("preview asset object missing".into()))?;
        let digest = value
            .get("sha256")
            .and_then(Value::as_str)
            .ok_or_else(|| sqlx::Error::Protocol("preview asset digest missing".into()))?
            .to_owned();
        if object_ref != format!("objects/{digest}") {
            return Err(sqlx::Error::Protocol(
                "preview asset object identity mismatch".into(),
            ));
        }
        let media_type = value
            .get("media_type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        // Preview uses a PDF source only as attachment metadata; trusted prepared
        // page images are separate assets and remain byte-verified below.
        let bytes = if media_type == "application/pdf" {
            Vec::new()
        } else {
            let read_digest = digest.clone();
            let bytes = tokio::task::spawn_blocking(move || platform::read_blob(&read_digest))
                .await
                .map_err(|error| sqlx::Error::Protocol(format!("preview asset join: {error}")))?
                .map_err(|error| sqlx::Error::Protocol(format!("preview asset read: {error}")))?;
            if platform::sha256_hex(&bytes) != digest {
                return Err(sqlx::Error::Protocol(
                    "preview asset digest mismatch".into(),
                ));
            }
            bytes
        };
        assets.push(crate::render_v2::FrozenLayoutAssetV2 {
            asset_revision_id: value
                .get("asset_revision_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            sha256: digest,
            media_type,
            file_name: value
                .get("file_name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            bytes,
        });
    }
    let forms = input
        .get("forms")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let preparations = input
        .get("preparations")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let layout = crate::render_v2::layout_preview_from_workspace_with_resources(
        title,
        workspace,
        &assets,
        &forms,
        &preparations,
        None,
    )
    .map_err(sqlx::Error::Protocol)?;
    Ok((crate::render_v2::render_html(&layout), etag))
}

pub async fn create_submission_export_request_v2(
    pool: &PgPool,
    workspace_id: Uuid,
    expected: (Uuid, &str),
    output_mode: &str,
    format: &str,
    mode_options: &Value,
    context: &crate::mutation::MutationContext,
) -> Result<Value, sqlx::Error> {
    let (expected_revision_id, expected_sha256) = expected;
    sqlx::query_scalar(
        "SELECT kb_bid_v2_create_submission_export_request($1,$2,$3::kb_sha256,$4,$5,$6,$7::kb_actor_identity,$8,$9,$10::kb_sha256)",
    )
    .bind(workspace_id)
    .bind(expected_revision_id)
    .bind(expected_sha256)
    .bind(output_mode)
    .bind(format)
    .bind(mode_options)
    .bind(&context.actor)
    .bind(&context.idempotency_key)
    .bind(&context.request.bytes)
    .bind(&context.request.sha256)
    .fetch_one(pool)
    .await
}

pub async fn list_submission_exports_v2(
    pool: &PgPool,
    workspace_id: Uuid,
    actor: &str,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_list_submission_exports($1,$2::kb_actor_identity)")
        .bind(workspace_id)
        .bind(actor)
        .fetch_one(pool)
        .await
}

pub async fn get_submission_export_v2(
    pool: &PgPool,
    workspace_id: Uuid,
    output_id: Uuid,
    actor: &str,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_get_submission_export($1,$2,$3::kb_actor_identity)")
        .bind(workspace_id)
        .bind(output_id)
        .bind(actor)
        .fetch_one(pool)
        .await
}

pub async fn get_submission_assessment_report_v2(
    pool: &PgPool,
    workspace_id: Uuid,
    output_id: Uuid,
    actor: &str,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_bid_v2_get_submission_assessment_report($1,$2,$3::kb_actor_identity)",
    )
    .bind(workspace_id)
    .bind(output_id)
    .bind(actor)
    .fetch_one(pool)
    .await
}

pub async fn get_submission_export_object_v2(
    pool: &PgPool,
    workspace_id: Uuid,
    output_id: Uuid,
    actor: &str,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_get_submission_export_object($1,$2,$3::kb_actor_identity)")
        .bind(workspace_id)
        .bind(output_id)
        .bind(actor)
        .fetch_one(pool)
        .await
}

pub async fn load_submission_export_input_v2(
    pool: &PgPool,
    request_artifact_id: Uuid,
    request_revision: i64,
    frozen_input_sha256: &str,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_load_submission_export_input($1,$2,$3::kb_sha256)")
        .bind(request_artifact_id)
        .bind(request_revision)
        .bind(frozen_input_sha256)
        .fetch_one(pool)
        .await
}

pub struct SubmissionExportFontV2<'a> {
    pub staging_id: Uuid,
    pub object_ref: &'a str,
    pub sha256: &'a str,
    pub media_type: &'a str,
}

pub struct SubmissionExportOutputV2<'a> {
    pub staging_id: Uuid,
    pub artifact_id: Uuid,
    pub object_ref: &'a str,
    pub sha256: &'a str,
    pub media_type: &'a str,
    pub byte_length: i64,
}

pub async fn prepare_submission_export_v2(
    pool: &PgPool,
    request: &platform::BidAuthoringRequestIdentityV2,
    font: SubmissionExportFontV2<'_>,
    snapshot_id: Uuid,
    manifest_id: Uuid,
    actor: &str,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_bid_v2_prepare_submission_export($1,$2,$3::kb_sha256,$4,$5::kb_object_ref,$6::kb_sha256,$7,$8,$9,$10::kb_actor_identity)",
    )
    .bind(request.request_artifact_id)
    .bind(request.request_revision)
    .bind(&request.frozen_input_sha256)
    .bind(font.staging_id)
    .bind(font.object_ref)
    .bind(font.sha256)
    .bind(font.media_type)
    .bind(snapshot_id)
    .bind(manifest_id)
    .bind(actor)
    .fetch_one(pool)
    .await
}

pub async fn load_submission_manifest_render_input_v2(
    pool: &PgPool,
    manifest_id: Uuid,
    manifest_sha256: &str,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_load_submission_manifest_render_input($1,$2::kb_sha256)")
        .bind(manifest_id)
        .bind(manifest_sha256)
        .fetch_one(pool)
        .await
}

pub async fn publish_submission_export_v2(
    pool: &PgPool,
    request: &platform::BidAuthoringRequestIdentityV2,
    font: SubmissionExportFontV2<'_>,
    snapshot_id: Uuid,
    manifest_id: Uuid,
    output: SubmissionExportOutputV2<'_>,
    actor: &str,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_bid_v2_publish_submission_export($1,$2,$3::kb_sha256,$4,$5::kb_object_ref,$6::kb_sha256,$7,$8,$9,$10,$11,$12::kb_object_ref,$13::kb_sha256,$14,$15,$16::kb_actor_identity)",
    )
    .bind(request.request_artifact_id)
    .bind(request.request_revision)
    .bind(&request.frozen_input_sha256)
    .bind(font.staging_id)
    .bind(font.object_ref)
    .bind(font.sha256)
    .bind(font.media_type)
    .bind(snapshot_id)
    .bind(manifest_id)
    .bind(output.staging_id)
    .bind(output.artifact_id)
    .bind(output.object_ref)
    .bind(output.sha256)
    .bind(output.media_type)
    .bind(output.byte_length)
    .bind(actor)
    .fetch_one(pool)
    .await
}

pub async fn mark_submission_export_failed_v2(
    pool: &PgPool,
    request_artifact_id: Uuid,
    request_revision: i64,
    frozen_input_sha256: &str,
    error_code: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT kb_bid_v2_mark_submission_export_failed($1,$2,$3::kb_sha256,$4)")
        .bind(request_artifact_id)
        .bind(request_revision)
        .bind(frozen_input_sha256)
        .bind(error_code)
        .execute(pool)
        .await
        .map(|_| ())
}

pub async fn async_request_status_v2(
    pool: &PgPool,
    request_artifact_id: Uuid,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT status FROM bidding_v2_async_requests WHERE id=$1")
        .bind(request_artifact_id)
        .fetch_optional(pool)
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
    let input: Value = sqlx::query_scalar(
        "SELECT kb_bid_v2_load_requirement_set_compile_input_v3($1,$2,$3::kb_sha256)",
    )
    .bind(request_artifact_id)
    .bind(request_revision)
    .bind(frozen_input_sha256)
    .fetch_one(pool)
    .await?;
    let compiled = crate::requirement_compile::compile_requirement_input_v3(&input)
        .map_err(sqlx::Error::Protocol)?;
    sqlx::query_scalar(
        "SELECT kb_bid_v2_publish_requirement_set_v3($1,$2,$3::kb_sha256,$4,$5::kb_actor_identity)",
    )
    .bind(request_artifact_id)
    .bind(request_revision)
    .bind(frozen_input_sha256)
    .bind(compiled)
    .bind("system:requirement-set-compile-v3")
    .fetch_one(pool)
    .await
}

pub async fn mark_requirement_set_compile_failed_v2(
    pool: &PgPool,
    request_artifact_id: Uuid,
    request_revision: i64,
    frozen_input_sha256: &str,
    error_code: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT kb_bid_v2_mark_requirement_set_compile_failed($1,$2,$3::kb_sha256,$4)")
        .bind(request_artifact_id)
        .bind(request_revision)
        .bind(frozen_input_sha256)
        .bind(error_code)
        .execute(pool)
        .await?;
    Ok(())
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

pub async fn upsert_outline_agent_run_v2(
    pool: &PgPool,
    request: &platform::BidAuthoringRequestIdentityV2,
    attempt: i32,
    max_attempts: i32,
    stage: &str,
    detail: Value,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT kb_bid_v2_outline_run_upsert($1,$2::kb_sha256,$3,$4,$5,$6)")
        .bind(request.request_artifact_id)
        .bind(&request.frozen_input_sha256)
        .bind(attempt)
        .bind(max_attempts)
        .bind(stage)
        .bind(detail)
        .execute(pool)
        .await
        .map(|_| ())
}

pub async fn load_outline_map_batch_v2(
    pool: &PgPool,
    request: &platform::BidAuthoringRequestIdentityV2,
    batch_ordinal: i32,
    model_sha: &str,
    agent_sha: &str,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_bid_v2_outline_map_get($1,$2::kb_sha256,$3,$4::kb_sha256,$5::kb_sha256)",
    )
    .bind(request.request_artifact_id)
    .bind(&request.frozen_input_sha256)
    .bind(batch_ordinal)
    .bind(model_sha)
    .bind(agent_sha)
    .fetch_one(pool)
    .await
}

pub async fn store_outline_map_batch_v2(
    pool: &PgPool,
    request: &platform::BidAuthoringRequestIdentityV2,
    batch_ordinal: i32,
    model_sha: &str,
    agent_sha: &str,
    unit_ids: &[Uuid],
    payload: &Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "SELECT kb_bid_v2_outline_map_put($1,$2::kb_sha256,$3,$4::kb_sha256,$5::kb_sha256,$6,$7)",
    )
    .bind(request.request_artifact_id)
    .bind(&request.frozen_input_sha256)
    .bind(batch_ordinal)
    .bind(model_sha)
    .bind(agent_sha)
    .bind(unit_ids)
    .bind(payload)
    .execute(pool)
    .await
    .map(|_| ())
}

pub async fn load_outline_requirement_grouping_batch_v1(
    pool: &PgPool,
    request: &platform::BidAuthoringRequestIdentityV2,
    batch_ordinal: i32,
    model_sha: &str,
    agent_sha: &str,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_bid_v2_outline_grouping_get($1,$2::kb_sha256,$3,$4::kb_sha256,$5::kb_sha256)",
    )
    .bind(request.request_artifact_id)
    .bind(&request.frozen_input_sha256)
    .bind(batch_ordinal)
    .bind(model_sha)
    .bind(agent_sha)
    .fetch_one(pool)
    .await
}

pub async fn store_outline_requirement_grouping_batch_v1(
    pool: &PgPool,
    request: &platform::BidAuthoringRequestIdentityV2,
    batch_ordinal: i32,
    model_sha: &str,
    agent_sha: &str,
    need_ids: &[Uuid],
    payload: &Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "SELECT kb_bid_v2_outline_grouping_put($1,$2::kb_sha256,$3,$4::kb_sha256,$5::kb_sha256,$6,$7)",
    )
    .bind(request.request_artifact_id)
    .bind(&request.frozen_input_sha256)
    .bind(batch_ordinal)
    .bind(model_sha)
    .bind(agent_sha)
    .bind(need_ids)
    .bind(payload)
    .execute(pool)
    .await
    .map(|_| ())
}

pub struct OutlineSemanticGroupingBatchV4<'a> {
    pub batch_ordinal: i32,
    pub model_sha: &'a str,
    pub agent_sha: &'a str,
    pub need_ids: &'a [Uuid],
    pub structure_fragment_refs: &'a [String],
    pub payload: &'a Value,
}

pub async fn store_outline_semantic_grouping_batch_v4(
    pool: &PgPool,
    request: &platform::BidAuthoringRequestIdentityV2,
    batch: &OutlineSemanticGroupingBatchV4<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "SELECT kb_bid_v2_outline_semantic_grouping_put($1,$2::kb_sha256,$3,$4::kb_sha256,$5::kb_sha256,$6,$7::kb_sha256[],$8)",
    )
    .bind(request.request_artifact_id)
    .bind(&request.frozen_input_sha256)
    .bind(batch.batch_ordinal)
    .bind(batch.model_sha)
    .bind(batch.agent_sha)
    .bind(batch.need_ids)
    .bind(batch.structure_fragment_refs)
    .bind(batch.payload)
    .execute(pool)
    .await
    .map(|_| ())
}

pub async fn load_outline_reduce_plan_v3(
    pool: &PgPool,
    request: &platform::BidAuthoringRequestIdentityV2,
    map_evidence_set_sha: &str,
    grouping_evidence_set_sha: &str,
    reduce_contract_sha: &str,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_bid_v2_outline_reduce_get($1,$2::kb_sha256,$3::kb_sha256,$4::kb_sha256,$5::kb_sha256)",
    )
    .bind(request.request_artifact_id)
    .bind(&request.frozen_input_sha256)
    .bind(map_evidence_set_sha)
    .bind(grouping_evidence_set_sha)
    .bind(reduce_contract_sha)
    .fetch_one(pool)
    .await
}

pub async fn store_outline_reduce_plan_v3(
    pool: &PgPool,
    request: &platform::BidAuthoringRequestIdentityV2,
    map_evidence_set_sha: &str,
    grouping_evidence_set_sha: &str,
    reduce_contract_sha: &str,
    payload: &Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "SELECT kb_bid_v2_outline_reduce_put($1,$2::kb_sha256,$3::kb_sha256,$4::kb_sha256,$5::kb_sha256,$6)",
    )
    .bind(request.request_artifact_id)
    .bind(&request.frozen_input_sha256)
    .bind(map_evidence_set_sha)
    .bind(grouping_evidence_set_sha)
    .bind(reduce_contract_sha)
    .bind(payload)
    .execute(pool)
    .await
    .map(|_| ())
}

pub async fn store_outline_synthesis_packet_v3(
    pool: &PgPool,
    request: &platform::BidAuthoringRequestIdentityV2,
    reduce_plan_sha: &str,
    map_evidence_set_sha: &str,
    grouping_evidence_set_sha: &str,
    payload: &Value,
) -> Result<String, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_bid_v2_outline_synthesis_packet_append($1,$2::kb_sha256,$3::kb_sha256,$4::kb_sha256,$5::kb_sha256,$6)",
    )
    .bind(request.request_artifact_id)
    .bind(&request.frozen_input_sha256)
    .bind(reduce_plan_sha)
    .bind(map_evidence_set_sha)
    .bind(grouping_evidence_set_sha)
    .bind(payload)
    .fetch_one(pool)
    .await
}

pub struct OutlineToolTraceV2<'a> {
    pub attempt: i32,
    pub ordinal: i32,
    pub tool_name: &'a str,
    pub args: &'a str,
    pub result: &'a str,
    pub duration_ms: i32,
    pub ok: bool,
}

pub async fn append_outline_tool_trace_v2(
    pool: &PgPool,
    request: &platform::BidAuthoringRequestIdentityV2,
    trace: OutlineToolTraceV2<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT kb_bid_v2_outline_trace_append($1,$2::kb_sha256,$3,$4,$5,$6,$7,$8,$9)")
        .bind(request.request_artifact_id)
        .bind(&request.frozen_input_sha256)
        .bind(trace.attempt)
        .bind(trace.ordinal)
        .bind(trace.tool_name)
        .bind(trace.args)
        .bind(trace.result)
        .bind(trace.duration_ms)
        .bind(if trace.ok { "ok" } else { "error" })
        .execute(pool)
        .await
        .map(|_| ())
}

pub async fn store_outline_agent_checkpoint_v2(
    pool: &PgPool,
    request: &platform::BidAuthoringRequestIdentityV2,
    attempt: i32,
    checkpoint_ordinal: i32,
    phase: &str,
    payload: &Value,
) -> Result<String, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_outline_checkpoint_append($1,$2::kb_sha256,$3,$4,$5,$6)")
        .bind(request.request_artifact_id)
        .bind(&request.frozen_input_sha256)
        .bind(attempt)
        .bind(checkpoint_ordinal)
        .bind(phase)
        .bind(payload)
        .fetch_one(pool)
        .await
}

pub async fn load_latest_outline_agent_checkpoint_v2(
    pool: &PgPool,
    request: &platform::BidAuthoringRequestIdentityV2,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_outline_checkpoint_latest($1,$2::kb_sha256)")
        .bind(request.request_artifact_id)
        .bind(&request.frozen_input_sha256)
        .fetch_one(pool)
        .await
}

pub async fn fail_stale_outline_runs_v2(
    pool: &PgPool,
    stale_seconds: i32,
) -> Result<u64, sqlx::Error> {
    sqlx::query_scalar::<_, i64>("SELECT kb_bid_v2_fail_stale_outline_runs($1)")
        .bind(stale_seconds)
        .fetch_one(pool)
        .await
        .map(|count| count.max(0) as u64)
}

pub async fn outline_tool_search_units_v2(
    pool: &PgPool,
    request: &platform::BidAuthoringRequestIdentityV2,
    query: &str,
    limit: i32,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_outline_tool_search_units($1,$2,$3::kb_sha256,$4,$5)")
        .bind(request.request_artifact_id)
        .bind(request.request_revision)
        .bind(&request.frozen_input_sha256)
        .bind(query)
        .bind(limit)
        .fetch_one(pool)
        .await
}

pub async fn outline_tool_read_units_v2(
    pool: &PgPool,
    request: &platform::BidAuthoringRequestIdentityV2,
    ids: &[Uuid],
    offset: i64,
    limit: Option<i64>,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_outline_tool_read_units($1,$2,$3::kb_sha256,$4,$5,$6)")
        .bind(request.request_artifact_id)
        .bind(request.request_revision)
        .bind(&request.frozen_input_sha256)
        .bind(ids)
        .bind(offset)
        .bind(limit)
        .fetch_one(pool)
        .await
}

pub async fn outline_tool_read_requirements_v2(
    pool: &PgPool,
    request: &platform::BidAuthoringRequestIdentityV2,
    ids: &[Uuid],
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_outline_tool_read_requirements($1,$2,$3::kb_sha256,$4)")
        .bind(request.request_artifact_id)
        .bind(request.request_revision)
        .bind(&request.frozen_input_sha256)
        .bind(ids)
        .fetch_one(pool)
        .await
}

pub async fn outline_tool_read_forms_v2(
    pool: &PgPool,
    request: &platform::BidAuthoringRequestIdentityV2,
    ids: &[Uuid],
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_outline_tool_read_forms($1,$2,$3::kb_sha256,$4)")
        .bind(request.request_artifact_id)
        .bind(request.request_revision)
        .bind(&request.frozen_input_sha256)
        .bind(ids)
        .fetch_one(pool)
        .await
}

pub async fn outline_tool_read_images_v2(
    pool: &PgPool,
    request: &platform::BidAuthoringRequestIdentityV2,
    ids: &[Uuid],
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_outline_tool_read_images($1,$2,$3::kb_sha256,$4)")
        .bind(request.request_artifact_id)
        .bind(request.request_revision)
        .bind(&request.frozen_input_sha256)
        .bind(ids)
        .fetch_one(pool)
        .await
}
