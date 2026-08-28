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
