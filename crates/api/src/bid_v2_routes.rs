use crate::AppState;
use crate::err::{fail, forbidden, not_found, validation};
use crate::routes::{
    ApiErr, actor_from, durable_human_actor, require_bid_pool, required_idempotency_key,
};
use axum::extract::{Multipart, Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use platform::{BidAuthoringJobPayloadV2, BidAuthoringRequestIdentityV2};
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
            "/api/v2/bid-projects/{id}/workspace",
            get(get_project_workspace),
        )
        .route(
            "/api/v2/bid-projects/{id}/tender-documents",
            get(list_tender_documents).post(upload_tender_document),
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
            "/api/v2/bid-projects/{id}/document-set-revisions",
            post(freeze_document_set),
        )
        .route(
            "/api/v2/bid-projects/{id}/source-units",
            get(list_source_units),
        )
        .route(
            "/api/v2/bid-projects/{id}/requirements",
            get(list_requirements),
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
            "/api/v2/submission-workspaces/{workspace_id}/outline-generations",
            post(create_outline_candidate),
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
        Some("40001") => fail(StatusCode::CONFLICT, "CONFLICT", message),
        Some("23505") if message.contains("IDEMPOTENCY_PAYLOAD_MISMATCH") => fail(
            StatusCode::CONFLICT,
            "IDEMPOTENCY_PAYLOAD_MISMATCH",
            message,
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
    let context =
        bidding::bidding::MutationContext::new(actor, required_idempotency_key(&headers)?, &body)
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
    let context =
        bidding::bidding::MutationContext::new(actor, required_idempotency_key(&headers)?, &body)
            .map_err(|error| validation(&error.to_string()))?;
    let pool = require_bid_pool().await?;
    bidding::bid_authoring_v2::end_project_v2(&pool, id, &context)
        .await
        .map(Json)
        .map_err(map_sql)
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
    let context = bidding::bidding::MutationContext::new(
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
    enqueue(BidAuthoringJobPayloadV2::TenderDocumentProcess {
        request,
        project_id: id,
        document_revision_id: persisted_document_id,
    })
    .await?;
    Ok((StatusCode::CREATED, Json(value)))
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
    let context =
        bidding::bidding::MutationContext::new(actor, required_idempotency_key(&headers)?, &body)
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
    let context =
        bidding::bidding::MutationContext::new(actor, required_idempotency_key(&headers)?, &body)
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

#[derive(Debug, Serialize, Deserialize)]
struct FreezeDocumentSetBody {
    document_ids: Vec<Uuid>,
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
struct AcceptCandidateBody {
    expected_workspace_revision_id: Uuid,
    expected_workspace_sha256: String,
    #[serde(default)]
    operation_indexes: Vec<usize>,
    #[serde(default)]
    client_node_refs: Vec<String>,
}

async fn freeze_document_set(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<FreezeDocumentSetBody>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let context =
        bidding::bidding::MutationContext::new(actor, required_idempotency_key(&headers)?, &body)
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
    enqueue(BidAuthoringJobPayloadV2::RequirementSetCompile {
        request,
        project_id: id,
        document_set_revision_id,
        disposition_set_revision_id,
    })
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
    let snapshot = bidding::workspace::apply_workspace_operations(&current, &body).map_err(
        |error| match error {
            bidding::workspace::WorkspaceMutationError::WorkspaceCasMismatch => fail(
                StatusCode::CONFLICT,
                "WORKSPACE_CAS_CONFLICT",
                error.to_string(),
            ),
            _ => validation(&error.to_string()),
        },
    )?;
    let context =
        bidding::bidding::MutationContext::new(actor, required_idempotency_key(&headers)?, &body)
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
    let context =
        bidding::bidding::MutationContext::new(actor, required_idempotency_key(&headers)?, &body)
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
    Ok((StatusCode::ACCEPTED, Json(request)))
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
            let mut operations = Vec::new();
            let mut ordinals = Vec::new();
            for (index, node) in nodes.iter().enumerate() {
                let client_ref = node
                    .get("client_node_ref")
                    .and_then(Value::as_str)
                    .ok_or_else(|| validation("candidate client_node_ref missing"))?;
                if !selected.contains(client_ref) {
                    continue;
                }
                let parent = node
                    .get("parent_client_node_ref")
                    .and_then(Value::as_str)
                    .filter(|value| selected.contains(value));
                operations.push(json!({
                    "kind":"insert_node",
                    "client_node_ref":client_ref,
                    "parent_lineage_id":Value::Null,
                    "parent_client_node_ref":parent,
                    "ordinal":node.get("ordinal").and_then(Value::as_u64).unwrap_or(index as u64),
                    "title":node.get("title").and_then(Value::as_str).unwrap_or("未命名章节"),
                    "semantic_role":node.get("semantic_role").and_then(Value::as_str).unwrap_or("other"),
                    "render_role":node.get("render_role").and_then(Value::as_str).unwrap_or("section")
                }));
                ordinals.push(
                    i32::try_from(index).map_err(|_| validation("candidate ordinal overflow"))?,
                );
            }
            if operations.is_empty() {
                return Err(validation("selected outline nodes do not exist"));
            }
            Ok((operations, ordinals))
        }
        Some("content") => Err(validation(
            "content candidate acceptance is not available yet",
        )),
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
    let (operations, ordinals) = candidate_operations(&candidate, &body)?;
    let mutation = bidding::workspace::WorkspaceMutationRequestV1 {
        schema_version: 1,
        workspace_id,
        expected_workspace_revision_id: body.expected_workspace_revision_id,
        expected_workspace_sha256: body.expected_workspace_sha256.clone(),
        operations,
    };
    let snapshot = bidding::workspace::apply_workspace_operations(&current, &mutation)
        .map_err(|error| validation(&error.to_string()))?;
    let context =
        bidding::bidding::MutationContext::new(actor, required_idempotency_key(&headers)?, &body)
            .map_err(|error| validation(&error.to_string()))?;
    let workspace = bidding::bid_authoring_v2::accept_candidate_v2(
        &pool,
        workspace_id,
        candidate_id,
        body.expected_workspace_revision_id,
        &body.expected_workspace_sha256,
        &snapshot,
        &ordinals,
        &context,
    )
    .await
    .map_err(map_sql)?;
    workspace_response(workspace)
}

async fn reject_candidate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, candidate_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let receipt_body = json!({"workspace_id":workspace_id,"candidate_id":candidate_id});
    let context = bidding::bidding::MutationContext::new(
        actor,
        required_idempotency_key(&headers)?,
        &receipt_body,
    )
    .map_err(|error| validation(&error.to_string()))?;
    let pool = require_bid_pool().await?;
    bidding::bid_authoring_v2::reject_candidate_v2(&pool, workspace_id, candidate_id, &context)
        .await
        .map(Json)
        .map_err(map_sql)
}
