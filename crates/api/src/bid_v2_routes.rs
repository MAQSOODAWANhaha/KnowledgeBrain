use crate::AppState;
use crate::err::{fail, forbidden, not_found, validation};
use crate::routes::{
    ApiErr, actor_from, durable_human_actor, require_bid_pool, required_idempotency_key,
};
use axum::body::Body;
use axum::extract::{Multipart, Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use platform::{
    BidAuthoringJobPayloadV2, BidAuthoringRequestIdentityV2, ContentGenerateOperationV2,
    SubmissionOutputModeV2,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v2/bid-projects",
            get(list_projects).post(create_project),
        )
        .route("/api/v2/bid-projects/{id}", get(get_project))
        .route("/api/v2/bid-projects/{id}/end", post(end_project))
        .route(
            "/api/v2/bid-projects/{id}/quote-snapshots",
            get(list_quote_snapshots).post(publish_quote_snapshot),
        )
        .route(
            "/api/v2/bid-projects/{id}/quote-snapshots/{snapshot_id}",
            get(get_quote_snapshot),
        )
        .route(
            "/api/v2/bid-projects/{id}/workspace",
            get(get_project_workspace),
        )
        .route(
            "/api/v2/bid-projects/{id}/tender-documents",
            get(list_tender_documents).post(upload_tender_document),
        )
        .route(
            "/api/v2/bid-projects/{id}/tender-documents/{document_id}/retry",
            post(retry_tender_document),
        )
        .route(
            "/api/v2/bid-projects/{id}/tender-documents/{document_id}/role",
            patch(patch_document_role),
        )
        .route(
            "/api/v2/bid-projects/{id}/tender-document-relations",
            get(list_document_relations).post(upsert_document_relation),
        )
        .route(
            "/api/v2/bid-projects/{id}/tender-document-relations/{relation_id}",
            patch(patch_document_relation),
        )
        .route(
            "/api/v2/bid-projects/{id}/document-set-revisions",
            get(list_document_sets).post(freeze_document_set),
        )
        .route(
            "/api/v2/bid-projects/{id}/document-set-revisions/{revision_id}",
            get(get_document_set),
        )
        .route(
            "/api/v2/bid-projects/{id}/source-units",
            get(list_source_units),
        )
        .route(
            "/api/v2/bid-projects/{id}/structured-forms",
            get(list_structured_forms),
        )
        .route(
            "/api/v2/bid-projects/{id}/source-unit-disposition-sets",
            post(publish_disposition_set),
        )
        .route(
            "/api/v2/bid-projects/{id}/requirements",
            get(list_requirements),
        )
        .route(
            "/api/v2/bid-projects/{id}/requirements/{requirement_id}",
            patch(patch_requirement),
        )
        .route(
            "/api/v2/bid-projects/{id}/requirement-supersessions",
            post(publish_requirement_supersession),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}",
            get(get_workspace),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/mutations",
            post(mutate_workspace),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/outline-candidates",
            post(create_outline_candidate),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/content-candidates",
            post(create_content_candidate),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/fulfillment-bindings",
            post(create_fulfillment_binding),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/fulfillment-bindings/{binding_lineage_id}",
            patch(patch_fulfillment_binding).delete(delete_fulfillment_binding),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/nodes/{node_lineage_id}/evidence-matches",
            post(match_evidence),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/nodes/{node_lineage_id}/evidence",
            get(get_node_evidence),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/nodes/{node_lineage_id}/evidence-pick-set",
            put(put_node_evidence_pick_set),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/evidence-overview",
            get(get_evidence_overview),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/assessments/current",
            get(get_current_assessments),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/preview",
            get(get_preview_html),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/outline-checkpoints",
            post(create_outline_checkpoint),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/assets",
            get(list_workspace_assets).post(upload_workspace_asset),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/assets/{asset_revision_id}",
            delete(delete_workspace_asset),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/assets/{asset_revision_id}/attachment-preparations",
            post(prepare_workspace_attachment),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/document-settings",
            get(get_document_settings).patch(patch_document_settings),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/requirement-projection",
            get(get_requirement_projection).patch(refresh_requirement_projection),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/exports",
            get(list_submission_exports).post(create_submission_export),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/exports/{export_id}",
            get(get_submission_export),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/exports/{export_id}/download",
            get(download_submission_export),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/exports/{export_id}/assessment-report",
            get(get_submission_assessment_report),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/requests",
            get(list_async_requests),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/requests/{request_id}",
            get(get_async_request),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/candidates/{candidate_id}",
            get(get_candidate),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/candidates/{candidate_id}/accept",
            post(accept_candidate),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/candidates/{candidate_id}/reject",
            post(reject_candidate),
        )
}

fn map_sql(error: sqlx::Error) -> ApiErr {
    let message = error.to_string();
    let code = error
        .as_database_error()
        .and_then(|value| value.code())
        .map(|value| value.into_owned());
    match code.as_deref() {
        Some("P0002") => not_found("bidding V2 resource"),
        Some("42501") => forbidden(),
        Some("40001") if message.contains("QUOTE_REVISION_CONFLICT") => {
            fail(StatusCode::CONFLICT, "QUOTE_REVISION_CONFLICT", message)
        }
        Some("40001") => fail(StatusCode::CONFLICT, "WORKSPACE_CAS_CONFLICT", message),
        Some("23505") if message.contains("IDEMPOTENCY_PAYLOAD_MISMATCH") => fail(
            StatusCode::CONFLICT,
            "IDEMPOTENCY_PAYLOAD_MISMATCH",
            message,
        ),
        Some("23514") if message.contains("TENDER_DOCUMENT_DUPLICATE") => fail(
            StatusCode::CONFLICT,
            "TENDER_DOCUMENT_DUPLICATE",
            "本项目已上传过这份文件",
        ),
        Some("23505") | Some("23514") | Some("22023") => validation(&message),
        Some("55000") => fail(StatusCode::CONFLICT, "PROJECT_ENDED", message),
        _ => {
            tracing::error!(%error, "bidding V2 SQL operation failed");
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL",
                "bidding V2 operation failed",
            )
        }
    }
}

async fn human_actor(headers: &HeaderMap, state: &AppState) -> Result<(Uuid, String), ApiErr> {
    let actor = actor_from(headers, state).await?;
    let durable = durable_human_actor(&actor)?;
    let user_id = durable
        .strip_prefix("user:")
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(forbidden)?;
    Ok((user_id, durable))
}

async fn enqueue(payload: BidAuthoringJobPayloadV2) -> Result<(), ApiErr> {
    match platform::enqueue_bid_authoring_v2(payload).await {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(fail(
            StatusCode::SERVICE_UNAVAILABLE,
            "QUEUE_UNAVAILABLE",
            "authoring queue unavailable; retry with the same Idempotency-Key",
        )),
        Err(error) => Err(fail(
            StatusCode::SERVICE_UNAVAILABLE,
            "QUEUE_UNAVAILABLE",
            format!("authoring queue unavailable: {error}"),
        )),
    }
}

async fn enqueue_if_pending(
    pool: &sqlx::PgPool,
    request: &BidAuthoringRequestIdentityV2,
    payload: BidAuthoringJobPayloadV2,
) -> Result<(), ApiErr> {
    let status =
        bidding::bid_authoring_v2::async_request_status_v2(pool, request.request_artifact_id)
            .await
            .map_err(map_sql)?;
    if status.as_deref() == Some("pending") {
        enqueue(payload).await
    } else {
        Ok(())
    }
}

fn request_identity(value: &Value) -> Result<BidAuthoringRequestIdentityV2, ApiErr> {
    let request_artifact_id = value
        .get("request_artifact_id")
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| validation("request_artifact_id missing"))?;
    let request_revision = value
        .get("request_revision")
        .and_then(Value::as_i64)
        .ok_or_else(|| validation("request_revision missing"))?;
    let frozen_input_sha256 = value
        .get("frozen_input_sha256")
        .and_then(Value::as_str)
        .ok_or_else(|| validation("frozen_input_sha256 missing"))?
        .to_owned();
    Ok(BidAuthoringRequestIdentityV2 {
        request_artifact_id,
        request_revision,
        frozen_input_sha256,
    })
}

async fn list_projects(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiErr> {
    let (user_id, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    bidding::bid_authoring_v2::list_projects_v2(&pool, user_id, &actor)
        .await
        .map(Json)
        .map_err(map_sql)
}

#[derive(Debug, Serialize, Deserialize)]
struct CreateProjectBody {
    title: String,
    #[serde(default)]
    ends_at: Option<String>,
}

async fn create_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateProjectBody>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    let (user_id, actor) = human_actor(&headers, &state).await?;
    let title = body.title.trim();
    if title.is_empty() {
        return Err(validation("title required"));
    }
    let context = bidding::MutationContext::new(actor, required_idempotency_key(&headers)?, &body)
        .map_err(|error| validation(&error.to_string()))?;
    let pool = require_bid_pool().await?;
    let value = bidding::bid_authoring_v2::create_project_v2(
        &pool,
        Uuid::new_v4(),
        title,
        user_id,
        &context,
    )
    .await
    .map_err(map_sql)?;
    Ok((StatusCode::CREATED, Json(value)))
}

async fn get_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    bidding::bid_authoring_v2::get_project_v2(&pool, id, &actor)
        .await
        .map_err(map_sql)?
        .map(Json)
        .ok_or_else(|| not_found("bid project"))
}

async fn end_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let context = bidding::MutationContext::new(actor, required_idempotency_key(&headers)?, &body)
        .map_err(|error| validation(&error.to_string()))?;
    let pool = require_bid_pool().await?;
    bidding::bid_authoring_v2::end_project_v2(&pool, id, &context)
        .await
        .map(Json)
        .map_err(map_sql)
}

async fn list_quote_snapshots(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    bidding::bid_authoring_v2::list_quote_snapshots_v2(&pool, id, &actor)
        .await
        .map(Json)
        .map_err(map_sql)
}

async fn get_quote_snapshot(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, snapshot_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    bidding::bid_authoring_v2::get_quote_snapshot_v2(&pool, id, snapshot_id, &actor)
        .await
        .map(Json)
        .map_err(map_sql)
}

async fn publish_quote_snapshot(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<bidding::quote_snapshot::FinalizeQuoteSnapshotV1>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    let (user_id, actor) = human_actor(&headers, &state).await?;
    let context =
        bidding::MutationContext::new(actor.clone(), required_idempotency_key(&headers)?, &body)
            .map_err(|error| validation(&error.to_string()))?;
    let pool = require_bid_pool().await?;
    let next = bidding::bid_authoring_v2::next_quote_snapshot_revision_v2(&pool, id, &actor)
        .await
        .map_err(map_sql)?;
    let revision = next
        .get("next_revision")
        .and_then(Value::as_i64)
        .ok_or_else(|| validation("quote revision identity missing"))?;
    let quote_id = next
        .get("quote_id")
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| validation("quote aggregate identity missing"))?;
    let built = bidding::quote_snapshot::build_quote_snapshot_v1(
        id,
        quote_id,
        revision,
        user_id,
        chrono::Utc::now(),
        &body,
    )
    .map_err(|error| validation(&error))?;
    let object_ref = platform::object_ref(&built.content_sha256);
    let staging_id = Uuid::new_v4();
    stage_upload(
        &pool,
        staging_id,
        &object_ref,
        &built.content_sha256,
        "application/json",
        &built.canonical_payload,
        &actor,
    )
    .await?;
    let snapshot_id = Uuid::new_v4();
    let result = bidding::bid_authoring_v2::publish_quote_snapshot_v2(
        &pool,
        bidding::bid_authoring_v2::PublishQuoteSnapshotV2 {
            project_id: id,
            snapshot_id,
            expected_revision: revision,
            staging_id,
            object_ref: &object_ref,
            content_sha256: &built.content_sha256,
            canonical_payload: &built.canonical_payload,
        },
        &context,
    )
    .await;
    let value = match result {
        Ok(value) => value,
        Err(error) => {
            let _ = platform::abandon_object_upload(&pool, staging_id, &actor).await;
            return Err(map_sql(error));
        }
    };
    let persisted_snapshot_id = value
        .get("quote_snapshot_id")
        .and_then(Value::as_str)
        .and_then(|raw| Uuid::parse_str(raw).ok());
    if persisted_snapshot_id != Some(snapshot_id) {
        let _ = platform::abandon_object_upload(&pool, staging_id, &actor).await;
    }
    Ok((StatusCode::CREATED, Json(value)))
}

async fn stage_upload(
    pool: &sqlx::PgPool,
    staging_id: Uuid,
    object_ref: &str,
    digest: &str,
    media_type: &str,
    bytes: &[u8],
    actor: &str,
) -> Result<(), ApiErr> {
    let byte_length = i64::try_from(bytes.len()).map_err(|_| validation("file too large"))?;
    platform::stage_object_upload(
        pool,
        staging_id,
        object_ref,
        digest,
        media_type,
        byte_length,
        actor,
    )
    .await
    .map_err(map_sql)?;
    if let Err(error) = platform::write_blob_async(digest, bytes).await {
        let _ = platform::abandon_object_upload(pool, staging_id, actor).await;
        tracing::error!(%error, %object_ref, "bidding V2 object write failed");
        return Err(fail(
            StatusCode::INTERNAL_SERVER_ERROR,
            "OBJECT_WRITE_FAILED",
            "file write failed",
        ));
    }
    Ok(())
}

async fn list_tender_documents(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    bidding::bid_authoring_v2::list_tender_documents_v2(&pool, id, &actor)
        .await
        .map(Json)
        .map_err(map_sql)
}

async fn upload_tender_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let mut file_name = String::new();
    let mut declared_media_type = None;
    let mut bytes = Vec::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| validation(&error.to_string()))?
    {
        if field.name() == Some("file") {
            file_name = field.file_name().unwrap_or("tender.pdf").to_owned();
            declared_media_type = field.content_type().map(str::to_owned);
            bytes = field
                .bytes()
                .await
                .map_err(|error| validation(&error.to_string()))?
                .to_vec();
        }
    }
    if bytes.is_empty() {
        return Err(validation("file required"));
    }
    let validated = bidding::tender_upload::validate_tender_upload(
        &file_name,
        declared_media_type.as_deref(),
        &bytes,
    )
    .map_err(|error| validation(&error.to_string()))?;
    let digest = platform::sha256_hex(&bytes);
    let object_ref = platform::object_ref(&digest);
    let context_payload = json!({
        "project_id": id,
        "file_name": file_name,
        "media_type": validated.media_type,
        "byte_length": bytes.len(),
        "original_sha256": digest,
    });
    let context = bidding::MutationContext::new(
        actor.clone(),
        required_idempotency_key(&headers)?,
        &context_payload,
    )
    .map_err(|error| validation(&error.to_string()))?;
    let pool = require_bid_pool().await?;
    let staging_id = Uuid::new_v4();
    stage_upload(
        &pool,
        staging_id,
        &object_ref,
        &digest,
        validated.media_type,
        &bytes,
        &actor,
    )
    .await?;
    let document_id = Uuid::new_v4();
    let request_artifact_id = Uuid::new_v4();
    let result = bidding::bid_authoring_v2::upload_tender_document_v2(
        &pool,
        bidding::bid_authoring_v2::UploadTenderDocumentV2 {
            staging_id,
            document_id,
            request_artifact_id,
            project_id: id,
            file_name: &file_name,
            media_type: validated.media_type,
            byte_length: bytes.len() as i64,
            object_ref: &object_ref,
            original_sha256: &digest,
        },
        &context,
    )
    .await;
    if result.is_err() {
        let _ = platform::abandon_object_upload(&pool, staging_id, &actor).await;
    }
    let value = result.map_err(map_sql)?;
    let request = request_identity(&value)?;
    let persisted_document_id = value
        .get("id")
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| validation("document id missing"))?;
    if persisted_document_id != document_id {
        let _ = platform::abandon_object_upload(&pool, staging_id, &actor).await;
    }
    enqueue_if_pending(
        &pool,
        &request,
        BidAuthoringJobPayloadV2::TenderDocumentProcess {
            request: request.clone(),
            project_id: id,
            document_revision_id: persisted_document_id,
        },
    )
    .await?;
    Ok((StatusCode::CREATED, Json(value)))
}

#[derive(Debug, Serialize, Deserialize)]
struct RetryTenderDocumentBody {
    expected_generation: i64,
}

async fn retry_tender_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, document_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<RetryTenderDocumentBody>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    if body.expected_generation <= 0 {
        return Err(validation("expected_generation must be positive"));
    }
    let context = bidding::MutationContext::new(actor, required_idempotency_key(&headers)?, &body)
        .map_err(|error| validation(&error.to_string()))?;
    let pool = require_bid_pool().await?;
    let value = bidding::bid_authoring_v2::retry_tender_document_v2(
        &pool,
        id,
        document_id,
        Uuid::new_v4(),
        body.expected_generation,
        &context,
    )
    .await
    .map_err(map_sql)?;
    let request = request_identity(&value)?;
    enqueue_if_pending(
        &pool,
        &request,
        BidAuthoringJobPayloadV2::TenderDocumentProcess {
            request: request.clone(),
            project_id: id,
            document_revision_id: document_id,
        },
    )
    .await?;
    Ok((StatusCode::ACCEPTED, Json(value)))
}

#[derive(Debug, Serialize, Deserialize)]
struct PatchRoleBody {
    document_role: String,
    expected_artifact_id: Uuid,
    expected_sha256: String,
}

async fn patch_document_role(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, document_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<PatchRoleBody>,
) -> Result<Json<Value>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let context = bidding::MutationContext::new(actor, required_idempotency_key(&headers)?, &body)
        .map_err(|error| validation(&error.to_string()))?;
    let pool = require_bid_pool().await?;
    bidding::bid_authoring_v2::patch_document_role_v2(
        &pool,
        id,
        document_id,
        &body.document_role,
        body.expected_artifact_id,
        &body.expected_sha256,
        &context,
    )
    .await
    .map(Json)
    .map_err(map_sql)
}

async fn list_document_relations(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    bidding::bid_authoring_v2::list_document_relations_v2(&pool, id, &actor)
        .await
        .map(Json)
        .map_err(map_sql)
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpsertRelationBody {
    #[serde(default)]
    lineage_id: Option<Uuid>,
    from_document_id: Uuid,
    to_document_id: Uuid,
    relation_kind: String,
    #[serde(default)]
    applicability: Value,
    #[serde(default)]
    expected_artifact_id: Option<Uuid>,
    #[serde(default)]
    expected_sha256: Option<String>,
}

async fn upsert_document_relation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<UpsertRelationBody>,
) -> Result<Json<Value>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let context = bidding::MutationContext::new(actor, required_idempotency_key(&headers)?, &body)
        .map_err(|error| validation(&error.to_string()))?;
    let pool = require_bid_pool().await?;
    bidding::bid_authoring_v2::upsert_document_relation_v2(
        &pool,
        bidding::bid_authoring_v2::UpsertDocumentRelationV2 {
            project_id: id,
            lineage_id: body.lineage_id.unwrap_or_else(Uuid::new_v4),
            from_document_id: body.from_document_id,
            to_document_id: body.to_document_id,
            relation_kind: &body.relation_kind,
            applicability: &body.applicability,
            expected_artifact_id: body.expected_artifact_id,
            expected_sha256: body.expected_sha256.as_deref(),
        },
        &context,
    )
    .await
    .map(Json)
    .map_err(map_sql)
}

async fn patch_document_relation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, relation_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpsertRelationBody>,
) -> Result<Json<Value>, ApiErr> {
    if body.lineage_id.is_some_and(|value| value != relation_id) {
        return Err(validation("relation identity mismatch"));
    }
    let (_, actor) = human_actor(&headers, &state).await?;
    let context = bidding::MutationContext::new(actor, required_idempotency_key(&headers)?, &body)
        .map_err(|error| validation(&error.to_string()))?;
    let pool = require_bid_pool().await?;
    bidding::bid_authoring_v2::upsert_document_relation_v2(
        &pool,
        bidding::bid_authoring_v2::UpsertDocumentRelationV2 {
            project_id: id,
            lineage_id: relation_id,
            from_document_id: body.from_document_id,
            to_document_id: body.to_document_id,
            relation_kind: &body.relation_kind,
            applicability: &body.applicability,
            expected_artifact_id: body.expected_artifact_id,
            expected_sha256: body.expected_sha256.as_deref(),
        },
        &context,
    )
    .await
    .map(Json)
    .map_err(map_sql)
}

async fn list_document_sets(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    bidding::bid_authoring_v2::list_document_sets_v2(&pool, id, &actor)
        .await
        .map(Json)
        .map_err(map_sql)
}

async fn get_document_set(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, revision_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    bidding::bid_authoring_v2::get_document_set_v2(&pool, id, revision_id, &actor)
        .await
        .map_err(map_sql)?
        .map(Json)
        .ok_or_else(|| not_found("document set revision"))
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FreezeDocumentSetBody {
    document_ids: Vec<Uuid>,
    #[serde(default)]
    expected_artifact_id: Option<Uuid>,
    #[serde(default)]
    expected_sha256: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PublishDispositionSetBody {
    document_set_revision_id: Uuid,
    expected_artifact_id: Uuid,
    expected_sha256: String,
    items: Value,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PatchRequirementBody {
    expected_requirement_set_id: Uuid,
    expected_requirement_set_sha256: String,
    requirement_kind: String,
    requiredness: String,
    compliance_policy: String,
    lifecycle: String,
    text: String,
    fulfillment_expr: Value,
    applicability: Value,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PublishRequirementSupersessionBody {
    lineage_id: Uuid,
    old_requirement_revision_id: Uuid,
    new_requirement_revision_id: Uuid,
    applicability: Value,
    #[serde(default)]
    tombstone: bool,
    #[serde(default)]
    expected_artifact_id: Option<Uuid>,
    #[serde(default)]
    expected_sha256: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OutlineGenerateBody {
    expected_workspace_revision_id: Uuid,
    document_set_revision_id: Uuid,
    document_set_sha256: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContentGenerateBody {
    target: String,
    #[serde(default)]
    node_lineage_id: Option<Uuid>,
    fill_policy: String,
    #[serde(default)]
    insertion_anchor: Option<Value>,
    selection_mode: String,
    #[serde(default)]
    pick_set_artifact_id: Option<Uuid>,
    expected_workspace_revision_id: Uuid,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceMatchBody {
    expected_workspace_revision_id: Uuid,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateEvidencePickSetBody {
    matching_report_id: Uuid,
    selected_evidence_item_ids: Vec<Uuid>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingTargetBody {
    kind: String,
    #[serde(default)]
    node_lineage_id: Option<Uuid>,
    #[serde(default)]
    block_lineage_id: Option<Uuid>,
    #[serde(default)]
    form_definition_revision_id: Option<Uuid>,
    #[serde(default)]
    quote_snapshot_id: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FulfillmentBindingBody {
    need_occurrence_id: Uuid,
    requirement_projection_revision_id: Uuid,
    channel: String,
    target: BindingTargetBody,
    reason: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RefreshRequirementProjectionBody {
    expected_artifact_id: Uuid,
    expected_sha256: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AcceptCandidateBody {
    expected_workspace_revision_id: Uuid,
    expected_workspace_sha256: String,
    #[serde(default)]
    operation_indexes: Vec<usize>,
    #[serde(default)]
    client_node_refs: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateOutlineCheckpointBody {
    expected_workspace_revision_id: Uuid,
    expected_workspace_sha256: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PatchDocumentSettingsBody {
    settings: Value,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrepareAttachmentBody {
    #[serde(default)]
    page_asset_revision_ids: Vec<Uuid>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SubmissionExportBody {
    mode: String,
    format: String,
    expected_workspace_revision_id: Uuid,
    #[serde(default)]
    watermark: Option<Value>,
    #[serde(default)]
    include_risk_notices: Option<bool>,
    #[serde(default)]
    include_knowledge_provenance: Option<bool>,
}

async fn freeze_document_set(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<FreezeDocumentSetBody>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let context = bidding::MutationContext::new(actor, required_idempotency_key(&headers)?, &body)
        .map_err(|error| validation(&error.to_string()))?;
    let pool = require_bid_pool().await?;
    let value = bidding::bid_authoring_v2::freeze_document_set_v2(
        &pool,
        id,
        &body.document_ids,
        body.expected_artifact_id,
        body.expected_sha256.as_deref(),
        Uuid::new_v4(),
        &context,
    )
    .await
    .map_err(map_sql)?;
    let request = request_identity(&value)?;
    let document_set_revision_id = value
        .get("artifact_id")
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| validation("document set artifact id missing"))?;
    let disposition_set_revision_id = value
        .get("disposition_set_artifact_id")
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| validation("disposition set artifact id missing"))?;
    enqueue_if_pending(
        &pool,
        &request,
        BidAuthoringJobPayloadV2::RequirementSetCompile {
            request: request.clone(),
            project_id: id,
            document_set_revision_id,
            disposition_set_revision_id,
        },
    )
    .await?;
    Ok((StatusCode::CREATED, Json(value)))
}

async fn publish_disposition_set(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<PublishDispositionSetBody>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let context = bidding::MutationContext::new(actor, required_idempotency_key(&headers)?, &body)
        .map_err(|error| validation(&error.to_string()))?;
    let pool = require_bid_pool().await?;
    let value = bidding::bid_authoring_v2::publish_disposition_set_v2(
        &pool,
        id,
        body.document_set_revision_id,
        &body.items,
        (body.expected_artifact_id, &body.expected_sha256),
        Uuid::new_v4(),
        &context,
    )
    .await
    .map_err(map_sql)?;
    let request = request_identity(&value)?;
    let disposition_set_revision_id = value
        .get("artifact_id")
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| validation("disposition set artifact id missing"))?;
    enqueue_if_pending(
        &pool,
        &request,
        BidAuthoringJobPayloadV2::RequirementSetCompile {
            request: request.clone(),
            project_id: id,
            document_set_revision_id: body.document_set_revision_id,
            disposition_set_revision_id,
        },
    )
    .await?;
    Ok((StatusCode::CREATED, Json(value)))
}

async fn list_source_units(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    bidding::bid_authoring_v2::list_source_units_v2(&pool, id, &actor)
        .await
        .map(Json)
        .map_err(map_sql)
}

async fn list_structured_forms(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    bidding::bid_authoring_v2::list_structured_forms_v2(&pool, id, &actor)
        .await
        .map(Json)
        .map_err(map_sql)
}

async fn list_requirements(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    bidding::bid_authoring_v2::list_requirements_v2(&pool, id, &actor)
        .await
        .map(Json)
        .map_err(map_sql)
}

async fn patch_requirement(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, requirement_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<PatchRequirementBody>,
) -> Result<Json<Value>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let context = bidding::MutationContext::new(actor, required_idempotency_key(&headers)?, &body)
        .map_err(|error| validation(&error.to_string()))?;
    let pool = require_bid_pool().await?;
    bidding::bid_authoring_v2::patch_requirement_v2(
        &pool,
        bidding::bid_authoring_v2::PatchRequirementV2 {
            project_id: id,
            requirement_revision_id: requirement_id,
            expected_set_id: body.expected_requirement_set_id,
            expected_set_sha256: &body.expected_requirement_set_sha256,
            requirement_kind: &body.requirement_kind,
            requiredness: &body.requiredness,
            compliance_policy: &body.compliance_policy,
            lifecycle: &body.lifecycle,
            text: &body.text,
            fulfillment_expr: &body.fulfillment_expr,
            applicability: &body.applicability,
        },
        &context,
    )
    .await
    .map(Json)
    .map_err(map_sql)
}

async fn publish_requirement_supersession(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<PublishRequirementSupersessionBody>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let context = bidding::MutationContext::new(actor, required_idempotency_key(&headers)?, &body)
        .map_err(|error| validation(&error.to_string()))?;
    let pool = require_bid_pool().await?;
    let value = bidding::bid_authoring_v2::publish_requirement_supersession_v2(
        &pool,
        bidding::bid_authoring_v2::PublishRequirementSupersessionV2 {
            project_id: id,
            lineage_id: body.lineage_id,
            old_requirement_revision_id: body.old_requirement_revision_id,
            new_requirement_revision_id: body.new_requirement_revision_id,
            applicability: &body.applicability,
            tombstone: body.tombstone,
            expected_artifact_id: body.expected_artifact_id,
            expected_sha256: body.expected_sha256.as_deref(),
        },
        &context,
    )
    .await
    .map_err(map_sql)?;
    Ok((StatusCode::CREATED, Json(value)))
}

async fn get_project_workspace(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    let project = bidding::bid_authoring_v2::get_project_v2(&pool, id, &actor)
        .await
        .map_err(map_sql)?
        .ok_or_else(|| not_found("bid project"))?;
    let workspace_id = project
        .get("workspace_id")
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| validation("workspace identity missing"))?;
    bidding::bid_authoring_v2::load_workspace_v2(&pool, workspace_id, &actor)
        .await
        .map_err(map_sql)?
        .map(Json)
        .ok_or_else(|| not_found("submission workspace"))
}

async fn get_workspace(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<Uuid>,
) -> Result<(HeaderMap, Json<Value>), ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    let workspace = bidding::bid_authoring_v2::load_workspace_v2(&pool, workspace_id, &actor)
        .await
        .map_err(map_sql)?
        .ok_or_else(|| not_found("submission workspace"))?;
    workspace_response(workspace)
}

fn workspace_response(workspace: Value) -> Result<(HeaderMap, Json<Value>), ApiErr> {
    let sha256 = workspace
        .get("sha256")
        .and_then(Value::as_str)
        .ok_or_else(|| validation("workspace sha256 missing"))?;
    let mut headers = HeaderMap::new();
    let etag = HeaderValue::from_str(&format!("\"{sha256}\""))
        .map_err(|_| validation("workspace sha256 invalid"))?;
    headers.insert("etag", etag);
    Ok((headers, Json(workspace)))
}

async fn mutate_workspace(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<Uuid>,
    Json(body): Json<bidding::workspace::WorkspaceMutationRequestV1>,
) -> Result<(HeaderMap, Json<Value>), ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    if body.workspace_id != workspace_id {
        return Err(validation("workspace path and body identity differ"));
    }
    let if_match = headers
        .get("if-match")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim_matches('"'))
        .ok_or_else(|| {
            fail(
                StatusCode::PRECONDITION_REQUIRED,
                "IF_MATCH_REQUIRED",
                "If-Match required",
            )
        })?;
    if if_match != body.expected_workspace_sha256 {
        return Err(fail(
            StatusCode::CONFLICT,
            "WORKSPACE_CAS_CONFLICT",
            "If-Match does not match expected_workspace_sha256",
        ));
    }
    let pool = require_bid_pool().await?;
    let current = bidding::bid_authoring_v2::load_workspace_v2(&pool, workspace_id, &actor)
        .await
        .map_err(map_sql)?
        .ok_or_else(|| not_found("submission workspace"))?;
    // A byte-identical retry reaches the idempotency receipt before SQL performs CAS.
    // Passing the current snapshot on a stale head is safe: a non-replay request is
    // rejected by kb_bid_v2_commit_workspace_mutation before the snapshot is read.
    let snapshot = match bidding::workspace::apply_workspace_operations(&current, &body) {
        Ok(snapshot) => snapshot,
        Err(bidding::workspace::WorkspaceMutationError::WorkspaceCasMismatch) => current.clone(),
        Err(error) => return Err(validation(&error.to_string())),
    };
    let context = bidding::MutationContext::new(actor, required_idempotency_key(&headers)?, &body)
        .map_err(|error| validation(&error.to_string()))?;
    let workspace = bidding::bid_authoring_v2::commit_workspace_mutation_v2(
        &pool,
        workspace_id,
        body.expected_workspace_revision_id,
        &body.expected_workspace_sha256,
        &snapshot,
        &context,
    )
    .await
    .map_err(map_sql)?;
    workspace_response(workspace)
}

fn required_if_match<'a>(headers: &'a HeaderMap, expected: &str) -> Result<&'a str, ApiErr> {
    let value = headers
        .get("if-match")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim_matches('"'))
        .ok_or_else(|| {
            fail(
                StatusCode::PRECONDITION_REQUIRED,
                "IF_MATCH_REQUIRED",
                "If-Match required",
            )
        })?;
    if value != expected {
        return Err(fail(
            StatusCode::CONFLICT,
            "WORKSPACE_CAS_CONFLICT",
            "If-Match does not match the expected workspace sha256",
        ));
    }
    Ok(value)
}

async fn create_outline_candidate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<Uuid>,
    Json(body): Json<OutlineGenerateBody>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let expected_sha256 = headers
        .get("if-match")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim_matches('"'))
        .ok_or_else(|| {
            fail(
                StatusCode::PRECONDITION_REQUIRED,
                "IF_MATCH_REQUIRED",
                "If-Match required",
            )
        })?;
    let context = bidding::MutationContext::new(actor, required_idempotency_key(&headers)?, &body)
        .map_err(|error| validation(&error.to_string()))?;
    let pool = require_bid_pool().await?;
    let request = bidding::bid_authoring_v2::create_outline_candidate_v2(
        &pool,
        workspace_id,
        body.expected_workspace_revision_id,
        expected_sha256,
        body.document_set_revision_id,
        &body.document_set_sha256,
        &context,
    )
    .await
    .map_err(map_sql)?;
    let identity = request_identity(&request)?;
    enqueue_if_pending(
        &pool,
        &identity,
        BidAuthoringJobPayloadV2::OutlineGenerate {
            request: identity.clone(),
            project_id: value_uuid(&request, "project_id")?,
            workspace_id: value_uuid(&request, "workspace_id")?,
            base_workspace_revision_id: value_uuid(&request, "base_workspace_revision_id")?,
        },
    )
    .await?;
    Ok((StatusCode::ACCEPTED, Json(request)))
}

fn value_uuid(value: &Value, key: &str) -> Result<Uuid, ApiErr> {
    value
        .get(key)
        .and_then(Value::as_str)
        .and_then(|raw| Uuid::parse_str(raw).ok())
        .ok_or_else(|| validation("required request identity missing"))
}

async fn enqueue_content_request(
    pool: &sqlx::PgPool,
    request: &Value,
    operation: ContentGenerateOperationV2,
) -> Result<(), ApiErr> {
    let identity = request_identity(request)?;
    enqueue_if_pending(
        pool,
        &identity,
        BidAuthoringJobPayloadV2::ContentGenerate {
            request: identity.clone(),
            project_id: value_uuid(request, "project_id")?,
            workspace_id: value_uuid(request, "workspace_id")?,
            base_workspace_revision_id: value_uuid(request, "base_workspace_revision_id")?,
            operation,
        },
    )
    .await
}

async fn create_content_candidate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<Uuid>,
    Json(body): Json<ContentGenerateBody>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let expected_sha256 = headers
        .get("if-match")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim_matches('"'))
        .ok_or_else(|| {
            fail(
                StatusCode::PRECONDITION_REQUIRED,
                "IF_MATCH_REQUIRED",
                "If-Match required",
            )
        })?;
    let context = bidding::MutationContext::new(actor, required_idempotency_key(&headers)?, &body)
        .map_err(|error| validation(&error.to_string()))?;
    let pool = require_bid_pool().await?;
    let request = bidding::bid_authoring_v2::create_content_request_v2(
        &pool,
        bidding::bid_authoring_v2::CreateContentRequestV2 {
            workspace_id,
            expected_revision_id: body.expected_workspace_revision_id,
            expected_sha256,
            operation: "generate",
            target_kind: &body.target,
            target_node_lineage_id: body.node_lineage_id,
            fill_policy: &body.fill_policy,
            insertion_anchor: body.insertion_anchor.as_ref(),
            evidence_selection_mode: &body.selection_mode,
            pick_set_artifact_id: body.pick_set_artifact_id,
        },
        &context,
    )
    .await
    .map_err(map_sql)?;
    enqueue_content_request(&pool, &request, ContentGenerateOperationV2::Generate).await?;
    Ok((StatusCode::ACCEPTED, Json(request)))
}

fn binding_target_value(body: &BindingTargetBody) -> Result<Value, ApiErr> {
    let identity = match body.kind.as_str() {
        "outline_node"
            if body.block_lineage_id.is_none()
                && body.form_definition_revision_id.is_none()
                && body.quote_snapshot_id.is_none() =>
        {
            body.node_lineage_id.map(|id| ("node_lineage_id", id))
        }
        "response_table"
            if body.node_lineage_id.is_none()
                && body.form_definition_revision_id.is_none()
                && body.quote_snapshot_id.is_none() =>
        {
            body.block_lineage_id.map(|id| ("block_lineage_id", id))
        }
        "structured_form"
            if body.node_lineage_id.is_none()
                && body.block_lineage_id.is_none()
                && body.quote_snapshot_id.is_none() =>
        {
            body.form_definition_revision_id
                .map(|id| ("form_definition_revision_id", id))
        }
        "quote"
            if body.node_lineage_id.is_none()
                && body.block_lineage_id.is_none()
                && body.form_definition_revision_id.is_none() =>
        {
            body.quote_snapshot_id.map(|id| ("quote_snapshot_id", id))
        }
        _ => None,
    }
    .ok_or_else(|| validation("binding target tagged identity is invalid"))?;
    Ok(json!({"kind":body.kind,identity.0:identity.1}))
}

fn workspace_resource_cas(headers: &HeaderMap, current: &Value) -> Result<(Uuid, String), ApiErr> {
    let revision = current
        .get("revision_id")
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| validation("workspace revision identity missing"))?;
    let expected = headers
        .get("if-match")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim_start_matches("W/").trim_matches('"').to_owned())
        .ok_or_else(|| {
            fail(
                StatusCode::PRECONDITION_REQUIRED,
                "IF_MATCH_REQUIRED",
                "If-Match required",
            )
        })?;
    // Do not reject a stale ETag before SQL: a byte-identical retry must reach
    // the idempotency receipt before aggregate CAS is evaluated.
    Ok((revision, expected))
}

async fn commit_resource_workspace(
    pool: &sqlx::PgPool,
    headers: &HeaderMap,
    workspace_id: Uuid,
    actor: String,
    current: &Value,
    snapshot: &Value,
    request: &Value,
) -> Result<(HeaderMap, Json<Value>), ApiErr> {
    let (revision, sha) = workspace_resource_cas(headers, current)?;
    let context = bidding::MutationContext::new(actor, required_idempotency_key(headers)?, request)
        .map_err(|error| validation(&error.to_string()))?;
    let workspace = bidding::bid_authoring_v2::commit_workspace_mutation_v2(
        pool,
        workspace_id,
        revision,
        &sha,
        snapshot,
        &context,
    )
    .await
    .map_err(map_sql)?;
    workspace_response(workspace)
}

async fn create_fulfillment_binding(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<Uuid>,
    Json(body): Json<FulfillmentBindingBody>,
) -> Result<(HeaderMap, Json<Value>), ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    let current = bidding::bid_authoring_v2::load_workspace_v2(&pool, workspace_id, &actor)
        .await
        .map_err(map_sql)?
        .ok_or_else(|| not_found("submission workspace"))?;
    let mut snapshot = current.clone();
    let bindings = snapshot
        .get_mut("bindings")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| validation("workspace bindings missing"))?;
    let target = binding_target_value(&body.target)?;
    bindings.push(json!({"binding_lineage_id":Uuid::new_v4(),"binding_revision_id":Uuid::new_v4(),"revision":1,
        "need_occurrence_id":body.need_occurrence_id,"requirement_projection_revision_id":body.requirement_projection_revision_id,
        "channel":body.channel,"target":target,"state":"bound","reason":body.reason}));
    let request = json!({"action":"create_fulfillment_binding","body":body});
    commit_resource_workspace(
        &pool,
        &headers,
        workspace_id,
        actor,
        &current,
        &snapshot,
        &request,
    )
    .await
}

async fn patch_fulfillment_binding(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, binding_lineage_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<FulfillmentBindingBody>,
) -> Result<(HeaderMap, Json<Value>), ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    let current = bidding::bid_authoring_v2::load_workspace_v2(&pool, workspace_id, &actor)
        .await
        .map_err(map_sql)?
        .ok_or_else(|| not_found("submission workspace"))?;
    let mut snapshot = current.clone();
    let bindings = snapshot
        .get_mut("bindings")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| validation("workspace bindings missing"))?;
    let index = bindings
        .iter()
        .position(|value| {
            value.get("binding_lineage_id").and_then(Value::as_str)
                == Some(binding_lineage_id.to_string().as_str())
        })
        .ok_or_else(|| not_found("fulfillment binding"))?;
    let revision = bindings[index]
        .get("revision")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        + 1;
    let target = binding_target_value(&body.target)?;
    bindings[index] = json!({"binding_lineage_id":binding_lineage_id,"binding_revision_id":Uuid::new_v4(),"revision":revision,
        "need_occurrence_id":body.need_occurrence_id,"requirement_projection_revision_id":body.requirement_projection_revision_id,
        "channel":body.channel,"target":target,"state":"bound","reason":body.reason});
    let request = json!({"action":"patch_fulfillment_binding","binding_lineage_id":binding_lineage_id,"body":body});
    commit_resource_workspace(
        &pool,
        &headers,
        workspace_id,
        actor,
        &current,
        &snapshot,
        &request,
    )
    .await
}

async fn delete_fulfillment_binding(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, binding_lineage_id)): Path<(Uuid, Uuid)>,
) -> Result<(HeaderMap, Json<Value>), ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    let current = bidding::bid_authoring_v2::load_workspace_v2(&pool, workspace_id, &actor)
        .await
        .map_err(map_sql)?
        .ok_or_else(|| not_found("submission workspace"))?;
    let mut snapshot = current.clone();
    let bindings = snapshot
        .get_mut("bindings")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| validation("workspace bindings missing"))?;
    let index = bindings
        .iter()
        .position(|value| {
            value.get("binding_lineage_id").and_then(Value::as_str)
                == Some(binding_lineage_id.to_string().as_str())
        })
        .ok_or_else(|| not_found("fulfillment binding"))?;
    let revision = bindings[index]
        .get("revision")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        + 1;
    let object = bindings[index]
        .as_object_mut()
        .ok_or_else(|| validation("fulfillment binding is invalid"))?;
    object.insert("binding_revision_id".into(), json!(Uuid::new_v4()));
    object.insert("revision".into(), json!(revision));
    object.insert("state".into(), json!("unbound"));
    object.insert("reason".into(), json!("user_unbound"));
    let request =
        json!({"action":"delete_fulfillment_binding","binding_lineage_id":binding_lineage_id});
    commit_resource_workspace(
        &pool,
        &headers,
        workspace_id,
        actor,
        &current,
        &snapshot,
        &request,
    )
    .await
}

async fn match_evidence(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, node_lineage_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<EvidenceMatchBody>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let expected_sha256 = headers
        .get("if-match")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim_matches('"'))
        .ok_or_else(|| {
            fail(
                StatusCode::PRECONDITION_REQUIRED,
                "IF_MATCH_REQUIRED",
                "If-Match required",
            )
        })?;
    let context = bidding::MutationContext::new(actor, required_idempotency_key(&headers)?, &body)
        .map_err(|error| validation(&error.to_string()))?;
    let pool = require_bid_pool().await?;
    let request = bidding::bid_authoring_v2::create_content_request_v2(
        &pool,
        bidding::bid_authoring_v2::CreateContentRequestV2 {
            workspace_id,
            expected_revision_id: body.expected_workspace_revision_id,
            expected_sha256,
            operation: "match_only",
            target_kind: "node",
            target_node_lineage_id: Some(node_lineage_id),
            fill_policy: "missing_requirements_only",
            insertion_anchor: None,
            evidence_selection_mode: "system_proposed",
            pick_set_artifact_id: None,
        },
        &context,
    )
    .await
    .map_err(map_sql)?;
    enqueue_content_request(&pool, &request, ContentGenerateOperationV2::MatchOnly).await?;
    Ok((StatusCode::ACCEPTED, Json(request)))
}

async fn put_node_evidence_pick_set(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, node_lineage_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<CreateEvidencePickSetBody>,
) -> Result<Json<Value>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    let context = bidding::MutationContext::new(actor, required_idempotency_key(&headers)?, &body)
        .map_err(|error| validation(&error.to_string()))?;
    bidding::bid_authoring_v2::create_node_evidence_pick_set_v2(
        &pool,
        workspace_id,
        node_lineage_id,
        body.matching_report_id,
        &body.selected_evidence_item_ids,
        &context,
    )
    .await
    .map(Json)
    .map_err(map_sql)
}

async fn get_node_evidence(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, node_lineage_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    bidding::bid_authoring_v2::get_node_evidence_v2(&pool, workspace_id, node_lineage_id, &actor)
        .await
        .map(Json)
        .map_err(map_sql)
}

async fn get_evidence_overview(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    bidding::bid_authoring_v2::get_evidence_overview_v2(&pool, workspace_id, &actor)
        .await
        .map(Json)
        .map_err(map_sql)
}

async fn get_current_assessments(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    bidding::bid_authoring_v2::get_current_assessments_v2(&pool, workspace_id, &actor)
        .await
        .map(Json)
        .map_err(map_sql)
}

async fn get_preview_html(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<Uuid>,
) -> Result<Response, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    let (html, etag) = bidding::bid_authoring_v2::get_preview_html_v2(&pool, workspace_id, &actor)
        .await
        .map_err(map_sql)?;
    let bytes = serde_json::to_vec(&json!({"html":html})).map_err(|error| {
        fail(
            StatusCode::INTERNAL_SERVER_ERROR,
            "RESPONSE_BUILD_FAILED",
            error.to_string(),
        )
    })?;
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .header("etag", format!("\"{etag}\""))
        .body(Body::from(bytes))
        .map_err(|error| {
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "RESPONSE_BUILD_FAILED",
                error.to_string(),
            )
        })
}

async fn create_submission_export(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<Uuid>,
    Json(body): Json<SubmissionExportBody>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let expected_sha256 = headers
        .get("if-match")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim_start_matches("W/").trim_matches('"'))
        .ok_or_else(|| {
            fail(
                StatusCode::PRECONDITION_REQUIRED,
                "IF_MATCH_REQUIRED",
                "If-Match required",
            )
        })?;
    let output_mode = match body.mode.as_str() {
        "review" | "review_draft" => "review_draft",
        "submission" => "submission",
        _ => return Err(validation("export mode must be review or submission")),
    };
    let queue_mode = match output_mode {
        "review_draft" => SubmissionOutputModeV2::ReviewDraft,
        _ => SubmissionOutputModeV2::Submission,
    };
    if !matches!(body.format.as_str(), "docx" | "pdf") {
        return Err(validation("export format must be docx or pdf"));
    }
    let watermark = body
        .watermark
        .as_ref()
        .and_then(|value| value.get("text").and_then(Value::as_str).map(str::to_owned));
    let mode_options = json!({
        "watermark":watermark,
        "include_assessment_notices":body.include_risk_notices.unwrap_or(output_mode=="review_draft"),
        "include_knowledge_sources":body.include_knowledge_provenance.unwrap_or(false),
    });
    let context = bidding::MutationContext::new(
        actor,
        required_idempotency_key(&headers)?,
        &json!({"workspace_id":workspace_id,"request":body,"mode_options":mode_options}),
    )
    .map_err(|error| validation(&error.to_string()))?;
    let pool = require_bid_pool().await?;
    let value = bidding::bid_authoring_v2::create_submission_export_request_v2(
        &pool,
        workspace_id,
        (body.expected_workspace_revision_id, expected_sha256),
        output_mode,
        &body.format,
        &mode_options,
        &context,
    )
    .await
    .map_err(map_sql)?;
    let request = request_identity(&value)?;
    let project_id = value
        .get("project_id")
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| validation("export project identity missing"))?;
    enqueue_if_pending(
        &pool,
        &request,
        BidAuthoringJobPayloadV2::SubmissionExport {
            request: request.clone(),
            project_id,
            workspace_id,
            workspace_revision_id: body.expected_workspace_revision_id,
            output_mode: queue_mode,
        },
    )
    .await?;
    Ok((StatusCode::ACCEPTED, Json(value)))
}

async fn list_submission_exports(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    bidding::bid_authoring_v2::list_submission_exports_v2(&pool, workspace_id, &actor)
        .await
        .map(Json)
        .map_err(map_sql)
}

async fn get_submission_export(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, export_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    bidding::bid_authoring_v2::get_submission_export_v2(&pool, workspace_id, export_id, &actor)
        .await
        .map(Json)
        .map_err(map_sql)
}

async fn get_submission_assessment_report(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, export_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    bidding::bid_authoring_v2::get_submission_assessment_report_v2(
        &pool,
        workspace_id,
        export_id,
        &actor,
    )
    .await
    .map(Json)
    .map_err(map_sql)
}

async fn download_submission_export(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, export_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    let metadata = bidding::bid_authoring_v2::get_submission_export_object_v2(
        &pool,
        workspace_id,
        export_id,
        &actor,
    )
    .await
    .map_err(map_sql)?
    .ok_or_else(|| not_found("export not found"))?;
    let object_ref = metadata
        .get("object_ref")
        .and_then(Value::as_str)
        .ok_or_else(|| validation("export object identity missing"))?;
    let digest = metadata
        .get("sha256")
        .and_then(Value::as_str)
        .ok_or_else(|| validation("export object digest missing"))?;
    if object_ref != format!("objects/{digest}") {
        return Err(validation("export object identity mismatch"));
    }
    let media_type = metadata
        .get("media_type")
        .and_then(Value::as_str)
        .ok_or_else(|| validation("export media type missing"))?;
    let file_name = metadata
        .get("file_name")
        .and_then(Value::as_str)
        .unwrap_or("submission.bin");
    let bytes = platform::read_blob(digest).map_err(|error| {
        fail(
            StatusCode::SERVICE_UNAVAILABLE,
            "EXPORT_OBJECT_UNAVAILABLE",
            error.to_string(),
        )
    })?;
    if platform::sha256_hex(&bytes) != digest {
        return Err(fail(
            StatusCode::UNPROCESSABLE_ENTITY,
            "ASSET_DIGEST_MISMATCH",
            "export bytes do not match immutable output digest",
        ));
    }
    if metadata.get("byte_length").and_then(Value::as_i64) != Some(bytes.len() as i64) {
        return Err(fail(
            StatusCode::UNPROCESSABLE_ENTITY,
            "ASSET_DIGEST_MISMATCH",
            "export byte length mismatch",
        ));
    }
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", media_type)
        .header(
            "content-disposition",
            format!("attachment; filename=\"{file_name}\""),
        )
        .body(Body::from(bytes))
        .map_err(|error| {
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "RESPONSE_BUILD_FAILED",
                error.to_string(),
            )
        })
}

async fn list_async_requests(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    bidding::bid_authoring_v2::list_workspace_async_requests_v2(&pool, workspace_id, &actor)
        .await
        .map(Json)
        .map_err(map_sql)
}

async fn get_async_request(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, request_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    bidding::bid_authoring_v2::get_async_request_v2(&pool, workspace_id, request_id, &actor)
        .await
        .map_err(map_sql)?
        .map(Json)
        .ok_or_else(|| not_found("authoring request"))
}

async fn get_candidate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, candidate_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    bidding::bid_authoring_v2::get_candidate_v2(&pool, workspace_id, candidate_id, &actor)
        .await
        .map_err(map_sql)?
        .map(Json)
        .ok_or_else(|| not_found("authoring candidate"))
}

fn candidate_operations(
    candidate: &Value,
    current: &Value,
    body: &AcceptCandidateBody,
) -> Result<(Vec<Value>, Vec<i32>), ApiErr> {
    match candidate.get("kind").and_then(Value::as_str) {
        Some("outline") => {
            let selected: std::collections::HashSet<&str> =
                body.client_node_refs.iter().map(String::as_str).collect();
            if selected.is_empty() {
                return Err(validation("at least one outline node must be selected"));
            }
            let nodes = candidate
                .get("nodes")
                .and_then(Value::as_array)
                .ok_or_else(|| validation("outline candidate nodes missing"))?;
            let mut by_ref = std::collections::HashMap::new();
            for (index, node) in nodes.iter().enumerate() {
                let reference = node
                    .get("client_node_ref")
                    .and_then(Value::as_str)
                    .ok_or_else(|| validation("candidate client_node_ref missing"))?;
                if by_ref.insert(reference, (index, node)).is_some() {
                    return Err(validation("candidate client_node_ref duplicated"));
                }
            }
            if selected
                .iter()
                .any(|reference| !by_ref.contains_key(reference))
            {
                return Err(validation("selected outline node does not exist"));
            }
            let identities = selected
                .iter()
                .map(|reference| (*reference, Uuid::new_v4()))
                .collect::<std::collections::HashMap<_, _>>();
            let mut pending = selected.clone();
            let mut inserted = std::collections::HashSet::new();
            let mut operations = Vec::new();
            let mut ordinals = Vec::new();
            while !pending.is_empty() {
                let before = pending.len();
                let references = pending.iter().copied().collect::<Vec<_>>();
                for reference in references {
                    let (index, node) = by_ref[reference];
                    let selected_parent = node
                        .get("parent_client_node_ref")
                        .and_then(Value::as_str)
                        .filter(|parent| selected.contains(parent));
                    if selected_parent.is_some_and(|parent| !inserted.contains(parent)) {
                        continue;
                    }
                    operations.push(json!({
                        "kind":"insert_node",
                        "lineage_id":identities[reference],
                        "revision_id":Uuid::new_v4(),
                        "parent_lineage_id":selected_parent.map(|parent| identities[parent]),
                        "ordinal":node.get("ordinal").and_then(Value::as_u64).unwrap_or(index as u64),
                        "title":node.get("title").and_then(Value::as_str).unwrap_or("未命名章节"),
                        "semantic_role":node.get("semantic_role").and_then(Value::as_str).unwrap_or("other"),
                        "render_role":node.get("render_role").and_then(Value::as_str).unwrap_or("section")
                    }));
                    ordinals.push(
                        i32::try_from(index)
                            .map_err(|_| validation("candidate ordinal overflow"))?,
                    );
                    inserted.insert(reference);
                    pending.remove(reference);
                }
                if pending.len() == before {
                    return Err(validation("selected outline graph is cyclic"));
                }
            }
            let projection_id = current
                .get("requirement_projection_revision_id")
                .and_then(Value::as_str)
                .ok_or_else(|| validation("requirement projection missing"))?;
            for binding in candidate
                .get("bindings")
                .and_then(Value::as_array)
                .unwrap_or(&Vec::new())
            {
                let Some(target_ref) = binding
                    .get("target_client_node_ref")
                    .and_then(Value::as_str)
                else {
                    continue;
                };
                if !selected.contains(target_ref) {
                    continue;
                }
                operations.push(json!({
                    "kind":"bind_fulfillment",
                    "need_occurrence_id":binding.get("need_occurrence_id"),
                    "requirement_projection_revision_id":projection_id,
                    "channel":binding.get("channel"),
                    "target":{"kind":"outline_node","node_lineage_id":identities[target_ref]},
                    "state":"bound",
                    "reason":"accepted_outline_candidate"
                }));
            }
            Ok((operations, ordinals))
        }
        Some("content") => {
            let requested: std::collections::HashSet<usize> =
                body.operation_indexes.iter().copied().collect();
            if requested.is_empty() {
                return Err(validation(
                    "at least one content operation must be selected",
                ));
            }
            let source = candidate
                .get("operations")
                .and_then(Value::as_array)
                .ok_or_else(|| validation("content candidate operations missing"))?;
            let mut operations = Vec::new();
            let mut ordinals = Vec::new();
            for (index, operation) in source.iter().enumerate() {
                if !requested.contains(&index) {
                    continue;
                }
                if operation.get("kind").and_then(Value::as_str) != Some("insert_block") {
                    return Err(validation("unsupported content candidate operation"));
                }
                operations.push(json!({
                    "kind":"insert_block",
                    "node_lineage_id":operation.get("target_node_lineage_id"),
                    "ordinal":operation.get("ordinal"),
                    "block":operation.get("block")
                }));
                ordinals.push(
                    i32::try_from(index).map_err(|_| validation("candidate ordinal overflow"))?,
                );
            }
            if operations.len() != requested.len() {
                return Err(validation("selected content operation does not exist"));
            }
            Ok((operations, ordinals))
        }
        _ => Err(validation("candidate kind invalid")),
    }
}

async fn accept_candidate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, candidate_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<AcceptCandidateBody>,
) -> Result<(HeaderMap, Json<Value>), ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    required_if_match(&headers, &body.expected_workspace_sha256)?;
    let pool = require_bid_pool().await?;
    let candidate =
        bidding::bid_authoring_v2::get_candidate_v2(&pool, workspace_id, candidate_id, &actor)
            .await
            .map_err(map_sql)?
            .ok_or_else(|| not_found("authoring candidate"))?;
    let current = bidding::bid_authoring_v2::load_workspace_v2(&pool, workspace_id, &actor)
        .await
        .map_err(map_sql)?
        .ok_or_else(|| not_found("submission workspace"))?;
    let context = bidding::MutationContext::new(actor, required_idempotency_key(&headers)?, &body)
        .map_err(|error| validation(&error.to_string()))?;
    if candidate.get("status").and_then(Value::as_str) == Some("accepted") {
        let receipt = bidding::bid_authoring_v2::accept_candidate_v2(
            &pool,
            workspace_id,
            candidate_id,
            (
                body.expected_workspace_revision_id,
                &body.expected_workspace_sha256,
            ),
            &json!({}),
            &[],
            &context,
        )
        .await
        .map_err(map_sql)?;
        return workspace_response(receipt);
    }
    if candidate.get("base_workspace_revision_id") != current.get("revision_id")
        || candidate.get("base_workspace_sha256") != current.get("sha256")
    {
        let obsolete = bidding::bid_authoring_v2::accept_candidate_v2(
            &pool,
            workspace_id,
            candidate_id,
            (
                body.expected_workspace_revision_id,
                &body.expected_workspace_sha256,
            ),
            &json!({}),
            &[],
            &context,
        )
        .await
        .map_err(map_sql)?;
        debug_assert_eq!(
            obsolete.get("error_code").and_then(Value::as_str),
            Some("CANDIDATE_OBSOLETE")
        );
        return Err(fail(
            StatusCode::CONFLICT,
            "CANDIDATE_OBSOLETE",
            "candidate base workspace is obsolete; review a new candidate",
        ));
    }
    let (operations, ordinals) = candidate_operations(&candidate, &current, &body)?;
    let mutation = bidding::workspace::WorkspaceMutationRequestV1 {
        schema_version: 1,
        workspace_id,
        expected_workspace_revision_id: body.expected_workspace_revision_id,
        expected_workspace_sha256: body.expected_workspace_sha256.clone(),
        operations,
    };
    let snapshot = bidding::workspace::apply_workspace_operations(&current, &mutation)
        .map_err(|error| validation(&error.to_string()))?;
    let workspace = bidding::bid_authoring_v2::accept_candidate_v2(
        &pool,
        workspace_id,
        candidate_id,
        (
            body.expected_workspace_revision_id,
            &body.expected_workspace_sha256,
        ),
        &snapshot,
        &ordinals,
        &context,
    )
    .await
    .map_err(map_sql)?;
    if workspace.get("error_code").and_then(Value::as_str) == Some("CANDIDATE_OBSOLETE") {
        return Err(fail(
            StatusCode::CONFLICT,
            "CANDIDATE_OBSOLETE",
            "candidate base workspace is obsolete; review a new candidate",
        ));
    }
    workspace_response(workspace)
}

async fn reject_candidate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, candidate_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let receipt_body = json!({"workspace_id":workspace_id,"candidate_id":candidate_id});
    let context =
        bidding::MutationContext::new(actor, required_idempotency_key(&headers)?, &receipt_body)
            .map_err(|error| validation(&error.to_string()))?;
    let pool = require_bid_pool().await?;
    bidding::bid_authoring_v2::reject_candidate_v2(&pool, workspace_id, candidate_id, &context)
        .await
        .map(Json)
        .map_err(map_sql)
}

async fn create_outline_checkpoint(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<Uuid>,
    Json(body): Json<CreateOutlineCheckpointBody>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let context = bidding::MutationContext::new(
        actor,
        required_idempotency_key(&headers)?,
        &json!({"workspace_id":workspace_id,"request":body}),
    )
    .map_err(|error| validation(&error.to_string()))?;
    let pool = require_bid_pool().await?;
    let value = bidding::bid_authoring_v2::create_outline_checkpoint_v2(
        &pool,
        workspace_id,
        body.expected_workspace_revision_id,
        &body.expected_workspace_sha256,
        Uuid::new_v4(),
        &context,
    )
    .await
    .map_err(map_sql)?;
    Ok((StatusCode::CREATED, Json(value)))
}

async fn get_document_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<Uuid>,
) -> Result<(HeaderMap, Json<Value>), ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    let workspace = bidding::bid_authoring_v2::load_workspace_v2(&pool, workspace_id, &actor)
        .await
        .map_err(map_sql)?
        .ok_or_else(|| not_found("submission workspace"))?;
    let sha = workspace
        .get("sha256")
        .and_then(Value::as_str)
        .ok_or_else(|| validation("workspace digest missing"))?;
    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        "etag",
        HeaderValue::from_str(&format!("\"{sha}\""))
            .map_err(|_| validation("workspace digest invalid"))?,
    );
    Ok((
        response_headers,
        Json(json!({"workspace_revision_id":workspace.get("revision_id"),
        "workspace_sha256":sha,"settings":workspace.get("document_settings")})),
    ))
}

async fn get_requirement_projection(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    bidding::bid_authoring_v2::get_requirement_projection_v2(&pool, workspace_id, &actor)
        .await
        .map(Json)
        .map_err(map_sql)
}

async fn refresh_requirement_projection(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<Uuid>,
    Json(body): Json<RefreshRequirementProjectionBody>,
) -> Result<(HeaderMap, Json<Value>), ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    let context = bidding::MutationContext::new(actor, required_idempotency_key(&headers)?, &body)
        .map_err(|error| validation(&error.to_string()))?;
    let workspace = bidding::bid_authoring_v2::refresh_requirement_projection_v2(
        &pool,
        workspace_id,
        body.expected_artifact_id,
        &body.expected_sha256,
        &context,
    )
    .await
    .map_err(map_sql)?;
    workspace_response(workspace)
}

async fn list_workspace_assets(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    bidding::bid_authoring_v2::list_workspace_assets_v2(&pool, workspace_id, &actor)
        .await
        .map(Json)
        .map_err(map_sql)
}

async fn upload_workspace_asset(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<Uuid>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let mut file_name = String::new();
    let mut declared_media_type = None;
    let mut bytes = Vec::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| validation(&error.to_string()))?
    {
        if field.name() == Some("file") {
            file_name = field.file_name().unwrap_or("asset.bin").to_owned();
            declared_media_type = field.content_type().map(str::to_owned);
            bytes = field
                .bytes()
                .await
                .map_err(|error| validation(&error.to_string()))?
                .to_vec();
        }
    }
    if bytes.is_empty() {
        return Err(validation("file required"));
    }
    let validated = bidding::tender_upload::validate_tender_upload(
        &file_name,
        declared_media_type.as_deref(),
        &bytes,
    )
    .map_err(|error| validation(&error.to_string()))?;
    let digest = platform::sha256_hex(&bytes);
    let object_ref = platform::object_ref(&digest);
    let context = bidding::MutationContext::new(
        actor.clone(),
        required_idempotency_key(&headers)?,
        &json!({
            "workspace_id":workspace_id,"file_name":file_name,"media_type":validated.media_type,
            "byte_length":bytes.len(),"content_sha256":digest
        }),
    )
    .map_err(|error| validation(&error.to_string()))?;
    let pool = require_bid_pool().await?;
    let staging_id = Uuid::new_v4();
    stage_upload(
        &pool,
        staging_id,
        &object_ref,
        &digest,
        validated.media_type,
        &bytes,
        &actor,
    )
    .await?;
    let asset_id = Uuid::new_v4();
    let result = bidding::bid_authoring_v2::upload_workspace_asset_v2(
        &pool,
        bidding::bid_authoring_v2::UploadWorkspaceAssetV2 {
            workspace_id,
            asset_id,
            staging_id,
            file_name: &file_name,
            media_type: validated.media_type,
            byte_length: bytes.len() as i64,
            object_ref: &object_ref,
            content_sha256: &digest,
        },
        &context,
    )
    .await;
    let value = match result {
        Ok(value) => value,
        Err(error) => {
            let _ = platform::abandon_object_upload(&pool, staging_id, &actor).await;
            return Err(map_sql(error));
        }
    };
    let persisted_asset_id = value
        .get("asset_revision_id")
        .and_then(Value::as_str)
        .and_then(|raw| Uuid::parse_str(raw).ok());
    if persisted_asset_id != Some(asset_id) {
        let _ = platform::abandon_object_upload(&pool, staging_id, &actor).await;
    }
    Ok((StatusCode::CREATED, Json(value)))
}

async fn prepare_workspace_attachment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, asset_revision_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<PrepareAttachmentBody>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    let assets = bidding::bid_authoring_v2::list_workspace_assets_v2(&pool, workspace_id, &actor)
        .await
        .map_err(map_sql)?;
    let values = assets.as_array().ok_or_else(|| {
        fail(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL",
            "asset list invalid",
        )
    })?;
    let source = values
        .iter()
        .find(|value| {
            value
                .get("asset_revision_id")
                .and_then(Value::as_str)
                .and_then(|value| Uuid::parse_str(value).ok())
                == Some(asset_revision_id)
        })
        .ok_or_else(|| {
            fail(
                StatusCode::NOT_FOUND,
                "NOT_FOUND",
                "workspace asset not found",
            )
        })?;
    let source_media = source
        .get("media_type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if source_media == "application/pdf" {
        return Err(validation(
            "PDF attachment pages are prepared by the SubmissionExport worker",
        ));
    }
    if !matches!(source_media, "image/png" | "image/jpeg" | "image/webp") {
        return Err(validation("attachment media type is unsupported"));
    }
    let page_ids = if body.page_asset_revision_ids.is_empty() {
        vec![asset_revision_id]
    } else {
        body.page_asset_revision_ids.clone()
    };
    if page_ids.as_slice() != [asset_revision_id] {
        return Err(validation(
            "image attachment preparation must use the source image itself",
        ));
    }
    let mut widths = Vec::with_capacity(page_ids.len());
    let mut heights = Vec::with_capacity(page_ids.len());
    for page_id in &page_ids {
        let page = values
            .iter()
            .find(|value| {
                value
                    .get("asset_revision_id")
                    .and_then(Value::as_str)
                    .and_then(|value| Uuid::parse_str(value).ok())
                    == Some(*page_id)
            })
            .ok_or_else(|| validation("attachment page asset not found"))?;
        let digest = page
            .get("content_sha256")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                fail(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL",
                    "page asset digest missing",
                )
            })?;
        let bytes = platform::read_blob(digest).map_err(|error| {
            fail(
                StatusCode::FAILED_DEPENDENCY,
                "ASSET_MISSING",
                error.to_string(),
            )
        })?;
        if platform::sha256_hex(&bytes) != digest {
            return Err(fail(
                StatusCode::UNPROCESSABLE_ENTITY,
                "ASSET_DIGEST_MISMATCH",
                "workspace page asset bytes do not match frozen digest",
            ));
        }
        let (width, height) = bidding::render_v2::frozen_image_dimensions(&bytes)
            .map_err(|error| validation(&error))?;
        widths
            .push(i32::try_from(width).map_err(|_| validation("attachment width exceeds limit"))?);
        heights.push(
            i32::try_from(height).map_err(|_| validation("attachment height exceeds limit"))?,
        );
    }
    let request = json!({"workspace_id":workspace_id,"asset_revision_id":asset_revision_id,
        "page_asset_revision_ids":page_ids,"operation":"prepare_attachment"});
    let context =
        bidding::MutationContext::new(actor, required_idempotency_key(&headers)?, &request)
            .map_err(|error| validation(&error.to_string()))?;
    bidding::bid_authoring_v2::prepare_workspace_attachment_v2(
        &pool,
        workspace_id,
        asset_revision_id,
        &page_ids,
        &widths,
        &heights,
        &context,
    )
    .await
    .map(|value| (StatusCode::CREATED, Json(value)))
    .map_err(map_sql)
}

async fn delete_workspace_asset(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, asset_revision_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    let request = json!({"workspace_id":workspace_id,"asset_revision_id":asset_revision_id,"reason":"user_removed"});
    let context =
        bidding::MutationContext::new(actor, required_idempotency_key(&headers)?, &request)
            .map_err(|error| validation(&error.to_string()))?;
    bidding::bid_authoring_v2::retire_workspace_asset_v2(
        &pool,
        workspace_id,
        asset_revision_id,
        "user_removed",
        &context,
    )
    .await
    .map(Json)
    .map_err(map_sql)
}

async fn patch_document_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<Uuid>,
    Json(body): Json<PatchDocumentSettingsBody>,
) -> Result<(HeaderMap, Json<Value>), ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let if_match = headers
        .get("if-match")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim_start_matches("W/").trim_matches('"').to_owned())
        .ok_or_else(|| {
            fail(
                StatusCode::PRECONDITION_REQUIRED,
                "IF_MATCH_REQUIRED",
                "If-Match required",
            )
        })?;
    bidding::workspace::validate_document_settings(&body.settings)
        .map_err(|error| validation(&error.to_string()))?;
    let pool = require_bid_pool().await?;
    let current = bidding::bid_authoring_v2::load_workspace_v2(&pool, workspace_id, &actor)
        .await
        .map_err(map_sql)?
        .ok_or_else(|| not_found("submission workspace"))?;
    let current_sha = current
        .get("sha256")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let current_revision = current
        .get("revision_id")
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| validation("workspace revision identity missing"))?;
    let expected_revision = if current_sha == if_match {
        current_revision
    } else {
        Uuid::nil()
    };
    let request = bidding::workspace::WorkspaceMutationRequestV1 {
        schema_version: 1,
        workspace_id,
        expected_workspace_revision_id: expected_revision,
        expected_workspace_sha256: if_match.clone(),
        operations: vec![json!({"kind":"update_document_settings","settings":body.settings})],
    };
    let snapshot = if current_sha == if_match {
        bidding::workspace::apply_workspace_operations(&current, &request)
            .map_err(|error| validation(&error.to_string()))?
    } else {
        current.clone()
    };
    let context = bidding::MutationContext::new(
        actor,
        required_idempotency_key(&headers)?,
        &json!({"workspace_id":workspace_id,"if_match":if_match,"settings":body.settings}),
    )
    .map_err(|error| validation(&error.to_string()))?;
    let workspace = bidding::bid_authoring_v2::commit_workspace_mutation_v2(
        &pool,
        workspace_id,
        expected_revision,
        &request.expected_workspace_sha256,
        &snapshot,
        &context,
    )
    .await
    .map_err(map_sql)?;
    workspace_response(workspace)
}
