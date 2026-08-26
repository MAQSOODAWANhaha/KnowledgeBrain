//! Final V1 bid HTTP contract: kind-only clauses, quote, submission, no family/export.

use crate::AppState;
use crate::err::{fail, not_found, validation};
use crate::routes::ApiErr;
use crate::routes::{
    Actor, actor_from, durable_human_actor, require_admin, require_bid_pool,
    required_idempotency_key,
};
use axum::extract::{Multipart, Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use uuid::Uuid;

pub fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/bids", get(list_bids).post(create_bid))
        .route("/api/v1/bids/{id}", get(get_bid).post(end_bid))
        .route(
            "/api/v1/bids/{id}/documents",
            get(list_documents).post(upload_document),
        )
        .route(
            "/api/v1/bids/{id}/documents/{did}/retry",
            post(retry_document),
        )
        .route(
            "/api/v1/bids/{id}/clauses",
            get(list_clauses).post(create_clause),
        )
        .route("/api/v1/bids/{id}/clauses/{cid}", patch(mutate_clause))
        .route("/api/v1/bids/{id}/facts", get(list_facts).post(mutate_fact))
        .route("/api/v1/bids/{id}/units", get(list_units))
        .route("/api/v1/bids/{id}/matching", get(get_matching))
        .route(
            "/api/v1/bids/{id}/matching/reports/{report_id}",
            get(get_matching_report),
        )
        .route("/api/v1/bids/{id}/matching/schedule", post(schedule_match))
        .route(
            "/api/v1/bids/{id}/matching/routes/{route_id}/pick-set",
            get(get_route_pick_set).put(replace_route_pick_set),
        )
        .route("/api/v1/bids/{id}/quote", get(get_quote).patch(patch_quote))
        .route("/api/v1/bids/{id}/quote/draft", post(create_quote_draft))
        .route(
            "/api/v1/bids/{id}/quote/lines/{line_id}",
            put(upsert_line).delete(delete_line),
        )
        .route("/api/v1/bids/{id}/quote/lines/reorder", post(reorder_lines))
        .route("/api/v1/bids/{id}/quote/preview", get(preview_quote))
        .route("/api/v1/bids/{id}/quote/finalize", post(finalize_quote))
        .route("/api/v1/bids/{id}/quote/reopen", post(reopen_quote))
        .route("/api/v1/bids/{id}/quote/snapshots/{sid}", get(get_snapshot))
        .route(
            "/api/v1/bids/{id}/company-profile",
            get(get_company_profile).put(update_company_profile),
        )
        .route(
            "/api/v1/bids/{id}/submission-profile",
            get(get_submission_profile).put(update_submission_profile),
        )
        .route(
            "/api/v1/bids/{id}/procedural-requirements",
            get(list_procedural),
        )
        .route(
            "/api/v1/bids/{id}/procedural-classifications/{cid}/override",
            post(override_classification),
        )
        .route(
            "/api/v1/bids/{id}/procedural-requirements/{cid}/resolve",
            post(resolve_requirement),
        )
        .route(
            "/api/v1/bids/{id}/attachments",
            get(list_attachments).post(upload_attachment),
        )
        .route(
            "/api/v1/bids/{id}/attachments/{aid}/{action}",
            post(mutate_attachment),
        )
        .route("/api/v1/bids/{id}/shots", get(get_shots).put(replace_shots))
        .route("/api/v1/bids/{id}/shots/artifacts", post(upload_shot))
        .route("/api/v1/bids/{id}/submission/outputs", get(list_outputs))
        .route("/api/v1/bids/{id}/parts", get(list_parts))
        .route(
            "/api/v1/bids/{id}/parts/{key}",
            get(get_part).put(update_part),
        )
        .route(
            "/api/v1/bids/{id}/parts/{key}/regenerate",
            post(regenerate_part),
        )
        .route("/api/v1/bids/{id}/gate-issues", get(list_gate_issues))
        .route(
            "/api/v1/bids/{id}/submission/manifests",
            post(create_manifest),
        )
        .route(
            "/api/v1/bids/{id}/submission/manifests/{mid}/render",
            post(render_manifest),
        )
        .route(
            "/api/v1/bids/{id}/submission/render-jobs/{jid}",
            get(get_render_job),
        )
        .route(
            "/api/v1/bids/{id}/submission/artifacts/{oid}",
            get(download_output),
        )
        .route(
            "/api/v1/maintenance/kind-router/register",
            post(register_kind_router),
        )
        .route(
            "/api/v1/maintenance/kind-router/promote",
            post(promote_kind_router),
        )
        .route(
            "/api/v1/maintenance/procedural-router/register",
            post(register_procedural_router),
        )
        .route(
            "/api/v1/maintenance/procedural-router/promote",
            post(promote_procedural_router),
        )
        .route(
            "/api/v1/maintenance/template-contracts/{slot}/register",
            post(register_template_contract),
        )
        .route(
            "/api/v1/maintenance/template-contracts/{slot}/promote",
            post(promote_template_contract),
        )
        .route_layer(axum::middleware::from_fn_with_state(
            state,
            require_bid_project_owner,
        ))
}

async fn require_bid_project_owner(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, ApiErr> {
    let Some(project_id) =
        bid_project_id_from_path(request.uri().path()).map_err(|()| not_found("bid"))?
    else {
        return Ok(next.run(request).await);
    };
    let actor = actor_from(request.headers(), &state).await?;
    let pool = require_bid_pool().await?;
    let project = storage::bidding::get_project(&pool, project_id)
        .await
        .map_err(map_sql)?
        .ok_or_else(|| not_found("bid"))?;
    if !matches!(actor, Actor::User(user_id) if user_id == project.owner_user_id) {
        return Err(not_found("bid"));
    }
    Ok(next.run(request).await)
}

fn bid_project_id_from_path(path: &str) -> Result<Option<Uuid>, ()> {
    let Some(remaining) = path.strip_prefix("/api/v1/bids/") else {
        return Ok(None);
    };
    let project_segment = remaining.split('/').next().ok_or(())?;
    if project_segment.is_empty() || project_segment.contains('%') {
        return Err(());
    }
    Uuid::parse_str(project_segment).map(Some).map_err(|_| ())
}

fn map_sql(error: sqlx::Error) -> ApiErr {
    let message = error.to_string();
    for code in [
        "SUBMISSION_MANIFEST_MISSING",
        "SUBMISSION_RENDER_JOB_MISSING",
        "SUBMISSION_OUTPUT_MISSING",
        "MANIFEST_ASSET_MISSING",
    ] {
        if message.contains(code) {
            return fail(StatusCode::NOT_FOUND, code, message);
        }
    }
    for code in [
        "QUOTE_EDIT_VERSION_MISMATCH",
        "FACT_REVISION_CAS_MISMATCH",
        "CEILING_IDENTITY_CAS_MISMATCH",
        "PRICING_IDENTITY_CAS_MISMATCH",
        "QUOTE_SNAPSHOT_CAS_MISMATCH",
        "PROFILE_REVISION_CAS_MISMATCH",
        "PART_CONTENT_CAS_MISMATCH",
        "PART_DEPENDENCY_CAS_MISMATCH",
        "ATTACHMENT_REVISION_CAS_MISMATCH",
        "SHOT_SET_REVISION_CAS_MISMATCH",
        "IDEMPOTENCY_PAYLOAD_MISMATCH",
        "SUBMISSION_END_STATE_CHANGED",
        "MANIFEST_SHA256_MISMATCH",
        "SUBMISSION_RENDER_IDENTITY_MISMATCH",
        "SUBMISSION_RENDER_CLAIM_LOST",
        "CURRENT_MATCHING_REPORT_MISMATCH",
        "ROUTE_PICK_REVISION_MISMATCH",
    ] {
        if message.contains(code) {
            return fail(StatusCode::CONFLICT, code, message);
        }
    }
    for code in [
        "QUOTE_AMOUNT_OVERFLOW",
        "CEILING_BASIS_UNSPECIFIED",
        "QUOTE_CEILING_EXCEEDED",
        "QUOTE_LINE_INCOMPLETE",
        "QUOTE_LINE_UNCONFIRMED",
        "QUOTE_EMPTY",
        "PROFILE_FIELD_MISSING",
        "SUBMISSION_GATE_REJECTED",
        "HUMAN_ACTOR_REQUIRED",
        "PROJECT_ENDED",
        "PART_KEY_INVALID",
        "ATTACHMENT_NOT_VALID",
        "ATTACHMENT_VALIDATION_INVALID",
        "ATTACHMENT_VALIDATION_IDENTITY_MISMATCH",
        "ATTACHMENT_RENDER_PAGE_SET_INVALID",
        "ATTACHMENT_RENDER_PAGE_IDENTITY_MISMATCH",
        "ATTACHMENT_RENDER_PAGE_QUOTA_EXCEEDED",
        "SHOT_VALIDATION_INVALID",
        "SHOT_SET_ARTIFACTS_INVALID",
        "MANIFEST_ASSET_UNAVAILABLE_OR_INVALID",
        "MANIFEST_ASSET_QUOTA_EXCEEDED",
        "PICK_ITEM_NOT_VISIBLE_SUPPORTED",
        "ROUTE_PICK_REQUIRES_TECHNICAL_REPORT",
    ] {
        if message.contains(code) {
            return fail(StatusCode::BAD_REQUEST, code, message);
        }
    }
    fail(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL", message)
}

const PDF_MEDIA_TYPE: &str = "application/pdf";
const DOCX_MEDIA_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

fn docx_structure_is_valid(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04")
        && bytes.windows(4).any(|window| window == b"PK\x01\x02")
        && bytes.windows(4).any(|window| window == b"PK\x05\x06")
        && bytes
            .windows(b"[Content_Types].xml".len())
            .any(|window| window == b"[Content_Types].xml")
        && bytes
            .windows(b"word/document.xml".len())
            .any(|window| window == b"word/document.xml")
}

fn tender_document_media_type(bytes: &[u8]) -> Result<&'static str, &'static str> {
    if bytes.starts_with(b"%PDF-") {
        let Some(eof_offset) = bytes
            .windows(b"%%EOF".len())
            .rposition(|window| window == b"%%EOF")
        else {
            return Err("TENDER_PDF_STRUCTURE_INVALID");
        };
        if bytes[eof_offset + b"%%EOF".len()..]
            .iter()
            .any(|byte| !byte.is_ascii_whitespace())
        {
            return Err("TENDER_PDF_STRUCTURE_INVALID");
        }
        return Ok(PDF_MEDIA_TYPE);
    }
    if bytes.starts_with(b"PK\x03\x04") {
        return docx_structure_is_valid(bytes)
            .then_some(DOCX_MEDIA_TYPE)
            .ok_or("TENDER_DOCX_STRUCTURE_INVALID");
    }
    Err("TENDER_DOCUMENT_TYPE_INVALID")
}

async fn validate_uploaded_bytes(
    bytes: Vec<u8>,
    allow_pdf: bool,
) -> Result<(Vec<u8>, storage::bid_submission::ValidatedUpload), ApiErr> {
    tokio::task::spawn_blocking(move || {
        storage::bid_submission::validate_upload_bytes(&bytes, allow_pdf)
            .map(|metadata| (bytes, metadata))
    })
    .await
    .map_err(|_| validation("upload validation task failed"))?
    .map_err(|error| validation(&error.to_string()))
}

async fn stage_uploaded_bytes(
    pool: &sqlx::PgPool,
    staging_id: Uuid,
    object_ref: &str,
    digest: &str,
    media_type: &str,
    bytes: &[u8],
    actor: &str,
) -> Result<(), ApiErr> {
    let byte_length = i64::try_from(bytes.len()).map_err(|_| validation("file too large"))?;
    storage::stage_object_upload(
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
    if let Err(error) = storage::write_blob_async(digest, bytes).await {
        tracing::error!(%error, %object_ref, %staging_id, "staged object write failed");
        let _ = storage::abandon_object_upload(pool, staging_id, actor).await;
        return Err(fail(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL",
            "file write failed",
        ));
    }
    Ok(())
}

async fn abandon_staged_upload(pool: &sqlx::PgPool, staging_id: Uuid, actor: &str) {
    if let Err(error) = storage::abandon_object_upload(pool, staging_id, actor).await {
        tracing::error!(%error, %staging_id, "failed to abandon object upload staging");
    }
}

async fn require_open(pool: &sqlx::PgPool, id: Uuid) -> Result<storage::bidding::Project, ApiErr> {
    let project = storage::bidding::get_project(pool, id)
        .await
        .map_err(map_sql)?
        .ok_or_else(|| not_found("bid"))?;
    if project.status != "open" {
        return Err(fail(StatusCode::CONFLICT, "ENDED", "project ended"));
    }
    Ok(project)
}

fn project_json(project: &storage::bidding::Project) -> Value {
    json!({
        "id": project.id,
        "title": project.title,
        "owner_user_id": project.owner_user_id,
        "ends_at": project.ends_at,
        "expires_at": project.expires_at,
        "status": project.status,
        "ended_at": project.ended_at,
        "fact_revision": project.fact_revision,
        "fact_sha256": project.fact_sha256,
        "budget_amount": project.budget_amount,
        "ceiling_price": project.ceiling_price,
        "ceiling_basis": project.ceiling_basis,
        "ceiling_revision": project.ceiling_revision,
        "ceiling_identity_sha256": project.ceiling_identity_sha256,
        "bid_open_at": project.bid_open_at,
        "bid_valid_until": project.bid_valid_until,
        "bid_valid_days": project.bid_valid_days
    })
}

async fn list_bids(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiErr> {
    let owner = match actor_from(&headers, &state).await? {
        Actor::User(owner) => owner,
        _ => {
            return Err(fail(
                StatusCode::FORBIDDEN,
                "BID_USER_REQUIRED",
                "bid access requires a user actor",
            ));
        }
    };
    let pool = require_bid_pool().await?;
    let projects = storage::bidding::list_projects(&pool, owner)
        .await
        .map_err(map_sql)?;
    Ok(Json(json!(
        projects.iter().map(project_json).collect::<Vec<_>>()
    )))
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct NewBid {
    title: String,
    ends_at: chrono::DateTime<chrono::Utc>,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

async fn create_bid(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<NewBid>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let actor_identity = durable_human_actor(&actor)?;
    let owner = match actor {
        Actor::User(id) => id,
        _ => return Err(validation("create bid requires a user actor")),
    };
    let pool = require_bid_pool().await?;
    let id = Uuid::new_v4();
    let context = storage::bidding::MutationContext::new(
        actor_identity,
        required_idempotency_key(&headers)?,
        &body,
    )
    .map_err(|error| validation(&error.to_string()))?;
    let created = storage::bidding::create_project(
        &pool,
        id,
        &body.title,
        owner,
        body.ends_at,
        body.expires_at,
        &context,
    )
    .await
    .map_err(map_sql)?;
    Ok((StatusCode::CREATED, Json(created)))
}

async fn get_bid(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    let project = storage::bidding::get_project(&pool, id)
        .await
        .map_err(map_sql)?
        .ok_or_else(|| not_found("bid"))?;
    let documents = storage::bidding::list_documents(&pool, id)
        .await
        .map_err(map_sql)?;
    let quote = storage::bid_quote::quote_state(&pool, id)
        .await
        .map_err(map_sql)?;
    let suggestions = storage::bidding::current_fact_suggestions(&pool, id)
        .await
        .map_err(map_sql)?;
    let clauses = storage::bidding::list_clauses(&pool, id, false)
        .await
        .map_err(map_sql)?;
    let matching = storage::bid_matching::matching_overview(&pool, id)
        .await
        .map_err(map_sql)?;
    let clause_sets = storage::bid_submission::clause_set_identities(&pool, id)
        .await
        .map_err(map_sql)?;
    let parts = storage::bid_submission::current_part_status(&pool, id)
        .await
        .map_err(map_sql)?;
    let outputs = storage::bid_submission::list_outputs(&pool, id)
        .await
        .map_err(map_sql)?;
    let files = documents.len() as i64;
    let ready = documents
        .iter()
        .filter(|document| document.parse_status == "completed")
        .count() as i64;
    let drafts = clauses
        .iter()
        .filter(|clause| clause.status == "draft")
        .count() as i64;
    let extract_running = documents.iter().any(|document| {
        document.parse_status == "pending" || document.parse_status == "processing"
    });
    let routes = matching
        .get("routes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let reports = matching
        .get("reports")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let match_running = !routes.is_empty() && reports.len() < routes.len();
    let has_picks = matching
        .get("project_pick_set")
        .and_then(|value| value.get("payload"))
        .and_then(|value| value.get("items"))
        .and_then(Value::as_array)
        .map(|items| !items.is_empty())
        .unwrap_or(false);
    Ok(Json(json!({
        "project": project_json(&project),
        "documents": documents,
        "quote": quote,
        "facts": {
            "revision": project.fact_revision,
            "sha256": project.fact_sha256,
            "budget_amount": project.budget_amount,
            "ceiling_price": project.ceiling_price,
            "ceiling_basis": project.ceiling_basis,
            "ceiling_revision": project.ceiling_revision,
            "ceiling_identity_sha256": project.ceiling_identity_sha256,
            "expires_at": project.expires_at,
            "bid_open_at": project.bid_open_at,
            "bid_valid_until": project.bid_valid_until,
            "bid_valid_days": project.bid_valid_days,
            "suggestions": suggestions
        },
        "clause_sets": clause_sets,
        "matching": matching,
        "parts": parts,
        "outputs": outputs,
        "derived": bid::derived_status(
            files,
            ready,
            drafts,
            i64::from(has_picks),
            0,
            extract_running,
            match_running,
        )
    })))
}

#[derive(Deserialize, Serialize)]
struct EndBid {
    expected_fact_revision: i64,
}

async fn end_bid(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<EndBid>,
) -> Result<Json<Value>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    let context = storage::bidding::MutationContext::new(
        durable_human_actor(&actor)?,
        required_idempotency_key(&headers)?,
        &body,
    )
    .map_err(|error| validation(&error.to_string()))?;
    Ok(Json(
        storage::bidding::end_project(&pool, id, body.expected_fact_revision, &context)
            .await
            .map_err(map_sql)?,
    ))
}

async fn list_documents(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    Ok(Json(json!({
        "documents": storage::bidding::list_documents(&pool, id).await.map_err(map_sql)?
    })))
}

async fn upload_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open(&pool, id).await?;
    let mut file_name = String::from("tender.pdf");
    let mut bytes = Vec::new();
    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name() == Some("file") {
            if let Some(name) = field.file_name() {
                file_name = name.to_string();
            }
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
    let media_type = tender_document_media_type(&bytes).map_err(validation)?;
    let actor = durable_human_actor(&actor)?;
    let digest = domain::sha256_hex(&bytes);
    let object_ref = storage::object_ref(&digest);
    let document_id = Uuid::new_v4();
    let payload = json!({
        "project_id": id,
        "file_name": file_name,
        "media_type": media_type,
        "byte_length": bytes.len(),
        "original_sha256": digest
    });
    let context = storage::bidding::MutationContext::new(
        actor.clone(),
        required_idempotency_key(&headers)?,
        &payload,
    )
    .map_err(|error| validation(&error.to_string()))?;
    let staging_id = Uuid::new_v4();
    stage_uploaded_bytes(
        &pool,
        staging_id,
        &object_ref,
        &digest,
        media_type,
        &bytes,
        &actor,
    )
    .await?;
    let created = storage::bidding::upload_document(
        &pool,
        staging_id,
        storage::bidding::UploadDocument {
            id: document_id,
            project_id: id,
            file_name: &file_name,
            media_type,
            byte_length: bytes.len() as i64,
            object_ref: &object_ref,
            original_sha256: &digest,
        },
        &context,
    )
    .await;
    if created.is_err() {
        abandon_staged_upload(&pool, staging_id, &actor).await;
    }
    let created = created.map_err(map_sql)?;
    let _ = runtime::enqueue_bid_convert(document_id).await;
    Ok((StatusCode::CREATED, Json(created)))
}

#[derive(Deserialize, Serialize)]
struct RetryDoc {
    expected_generation: i32,
}

async fn retry_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, did)): Path<(Uuid, Uuid)>,
    Json(body): Json<RetryDoc>,
) -> Result<Json<Value>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open(&pool, id).await?;
    let context = storage::bidding::MutationContext::new(
        durable_human_actor(&actor)?,
        required_idempotency_key(&headers)?,
        &body,
    )
    .map_err(|error| validation(&error.to_string()))?;
    let result = storage::bidding::retry_document_conversion(
        &pool,
        id,
        did,
        body.expected_generation,
        &context,
    )
    .await
    .map_err(map_sql)?;
    let _ = runtime::enqueue_bid_convert(did).await;
    Ok(Json(result))
}

async fn list_clauses(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    let history = query.get("include_history").map(String::as_str) == Some("true");
    let clauses = storage::bidding::list_clauses(&pool, id, history)
        .await
        .map_err(map_sql)?;
    Ok(Json(json!({ "clauses": clauses })))
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct NewClause {
    text: String,
    kind: String,
    must: bool,
}

async fn create_clause(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<NewClause>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    if body.kind.parse::<bid::tender::ClauseKind>().is_err() {
        return Err(validation("kind must be a server-owned clause kind"));
    }
    let actor = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open(&pool, id).await?;
    let clause_id = Uuid::new_v4();
    let context = storage::bidding::MutationContext::new(
        durable_human_actor(&actor)?,
        required_idempotency_key(&headers)?,
        &body,
    )
    .map_err(|error| validation(&error.to_string()))?;
    Ok((
        StatusCode::CREATED,
        Json(
            storage::bidding::create_clause(
                &pool, clause_id, id, &body.text, &body.kind, body.must, &context,
            )
            .await
            .map_err(map_sql)?,
        ),
    ))
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct MutateClause {
    action: String,
    expected_revision: i64,
    #[serde(default)]
    patch: Value,
}

async fn mutate_clause(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, cid)): Path<(Uuid, Uuid)>,
    Json(body): Json<MutateClause>,
) -> Result<Json<Value>, ApiErr> {
    if body.patch.get("family").is_some() {
        return Err(validation("client must not submit family"));
    }
    let actor = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open(&pool, id).await?;
    let context = storage::bidding::MutationContext::new(
        durable_human_actor(&actor)?,
        required_idempotency_key(&headers)?,
        &body,
    )
    .map_err(|error| validation(&error.to_string()))?;
    Ok(Json(
        storage::bidding::mutate_clause(
            &pool,
            id,
            cid,
            &body.action,
            &body.patch,
            body.expected_revision,
            &context,
        )
        .await
        .map_err(map_sql)?,
    ))
}

async fn list_facts(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    let project = storage::bidding::get_project(&pool, id)
        .await
        .map_err(map_sql)?
        .ok_or_else(|| not_found("bid"))?;
    Ok(Json(json!({
        "project_facts": {
            "revision": project.fact_revision,
            "sha256": project.fact_sha256,
            "budget_amount": project.budget_amount,
            "ceiling_price": project.ceiling_price,
            "ceiling_currency": project.ceiling_currency,
            "ceiling_basis": project.ceiling_basis,
            "ceiling_revision": project.ceiling_revision,
            "ceiling_identity_sha256": project.ceiling_identity_sha256,
            "expires_at": project.expires_at,
            "bid_open_at": project.bid_open_at,
            "bid_valid_until": project.bid_valid_until,
            "bid_valid_days": project.bid_valid_days
        },
        "suggestions": storage::bidding::current_fact_suggestions(&pool, id).await.map_err(map_sql)?,
        "history": storage::bidding::fact_suggestion_history(&pool, id).await.map_err(map_sql)?
    })))
}

#[derive(Deserialize, Serialize)]
struct MutateFact {
    action: String,
    expected_fact_revision: i64,
    candidate_id: Option<Uuid>,
    field: Option<String>,
    typed_value: Option<Value>,
    reason: Option<String>,
    override_reason: Option<String>,
}

async fn mutate_fact(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<MutateFact>,
) -> Result<Json<Value>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open(&pool, id).await?;
    let context = storage::bidding::MutationContext::new(
        durable_human_actor(&actor)?,
        required_idempotency_key(&headers)?,
        &body,
    )
    .map_err(|error| validation(&error.to_string()))?;
    Ok(Json(
        storage::bidding::mutate_fact(
            &pool,
            storage::bidding::FactMutation {
                project_id: id,
                action: &body.action,
                candidate_id: body.candidate_id,
                field: body.field.as_deref(),
                typed_value: body.typed_value.as_ref(),
                reason: body.reason.as_deref(),
                override_reason: body.override_reason.as_deref(),
                expected_fact_revision: body.expected_fact_revision,
            },
            &context,
        )
        .await
        .map_err(map_sql)?,
    ))
}

async fn list_units(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    Ok(Json(json!({
        "units": bid::list_match_units(&pool, id).await.map_err(|error| fail(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL", error))?
    })))
}

async fn schedule_match(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open(&pool, id).await?;
    let job = bid::schedule_dirty_and_enqueue(
        &pool,
        id,
        storage::bid_matching::ScheduleMutationContext::human(
            durable_human_actor(&actor)?,
            required_idempotency_key(&headers)?,
        ),
    )
    .await
    .map_err(|error| fail(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL", error))?;
    Ok(Json(json!({ "job_id": job })))
}

async fn get_matching(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    Ok(Json(
        storage::bid_matching::matching_overview(&pool, id)
            .await
            .map_err(map_sql)?,
    ))
}

async fn get_matching_report(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, report_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    storage::bid_matching::matching_report_artifact_json(&pool, project_id, report_id)
        .await
        .map_err(map_sql)?
        .ok_or_else(|| not_found("matching report"))
        .map(Json)
}

async fn get_route_pick_set(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, route_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    Ok(Json(
        storage::bid_matching::route_pick_set_json(&pool, project_id, route_id)
            .await
            .map_err(map_sql)?,
    ))
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReplaceRoutePickSetBody {
    source_report_artifact_id: Uuid,
    report_sha256: String,
    expected_revision: i64,
    items: Vec<storage::bid_matching::PickSelectionV1>,
}

async fn replace_route_pick_set(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, route_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<ReplaceRoutePickSetBody>,
) -> Result<Json<storage::bid_matching::PickSetReceiptV1>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open(&pool, project_id).await?;
    let context = storage::bidding::MutationContext::new(
        durable_human_actor(&actor)?,
        required_idempotency_key(&headers)?,
        &body,
    )
    .map_err(|error| validation(&error.to_string()))?;
    Ok(Json(
        storage::bid_matching::replace_route_pick_set(
            &pool,
            storage::bid_matching::ReplaceRoutePickSetV1 {
                project_id,
                route_id,
                source_report_artifact_id: body.source_report_artifact_id,
                report_sha256: body.report_sha256,
                expected_revision: body.expected_revision,
                selections: body.items,
            },
            &context,
        )
        .await
        .map_err(map_sql)?,
    ))
}

async fn get_quote(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    Ok(Json(
        storage::bid_quote::quote_state(&pool, id)
            .await
            .map_err(map_sql)?,
    ))
}

#[derive(Deserialize, Serialize)]
struct QuoteDraft {
    tax_mode: String,
    title: String,
    notes: Option<String>,
}

async fn create_quote_draft(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<QuoteDraft>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open(&pool, id).await?;
    let context = storage::bidding::MutationContext::new(
        durable_human_actor(&actor)?,
        required_idempotency_key(&headers)?,
        &body,
    )
    .map_err(|error| validation(&error.to_string()))?;
    Ok((
        StatusCode::CREATED,
        Json(
            storage::bid_quote::create_quote_draft(
                &pool,
                id,
                &body.tax_mode,
                &body.title,
                body.notes.as_deref(),
                &context,
            )
            .await
            .map_err(map_sql)?,
        ),
    ))
}

#[derive(Deserialize, Serialize)]
struct PatchQuote {
    expected_edit_version: i64,
    tax_mode: String,
    title: String,
    notes: Option<String>,
}

async fn patch_quote(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchQuote>,
) -> Result<Json<Value>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open(&pool, id).await?;
    let context = storage::bidding::MutationContext::new(
        durable_human_actor(&actor)?,
        required_idempotency_key(&headers)?,
        &body,
    )
    .map_err(|error| validation(&error.to_string()))?;
    Ok(Json(
        storage::bid_quote::patch_quote_header(
            &pool,
            id,
            body.expected_edit_version,
            &body.tax_mode,
            &body.title,
            body.notes.as_deref(),
            &context,
        )
        .await
        .map_err(map_sql)?,
    ))
}

#[derive(Deserialize, Serialize)]
struct UpsertLine {
    expected_edit_version: i64,
    ordinal: i32,
    description: String,
    pricing_mode: String,
    quantity: Option<String>,
    unit: Option<String>,
    unit_price: Option<String>,
    entered_amount: Option<String>,
    tax_rate: String,
    user_confirmed: bool,
}

async fn upsert_line(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, line_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpsertLine>,
) -> Result<Json<Value>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open(&pool, id).await?;
    let context = storage::bidding::MutationContext::new(
        durable_human_actor(&actor)?,
        required_idempotency_key(&headers)?,
        &body,
    )
    .map_err(|error| validation(&error.to_string()))?;
    if let Some(raw) = body.quantity.as_deref() {
        bid::quote::parse_decimal_string(raw, 6).map_err(|error| validation(&error.to_string()))?;
    }
    if let Some(raw) = body.unit_price.as_deref() {
        bid::quote::parse_decimal_string(raw, 6).map_err(|error| validation(&error.to_string()))?;
    }
    if let Some(raw) = body.entered_amount.as_deref() {
        bid::quote::parse_decimal_string(raw, 2).map_err(|error| validation(&error.to_string()))?;
    }
    bid::quote::parse_decimal_string(&body.tax_rate, 6)
        .map_err(|error| validation(&error.to_string()))?;
    Ok(Json(
        storage::bid_quote::upsert_quote_line(
            &pool,
            storage::bid_quote::UpsertQuoteLine {
                project_id: id,
                line_id,
                expected_edit_version: body.expected_edit_version,
                ordinal: body.ordinal,
                description: &body.description,
                pricing_mode: &body.pricing_mode,
                quantity: body.quantity.as_deref(),
                unit: body.unit.as_deref(),
                unit_price: body.unit_price.as_deref(),
                entered_amount: body.entered_amount.as_deref(),
                tax_rate: &body.tax_rate,
                user_confirmed: body.user_confirmed,
            },
            &context,
        )
        .await
        .map_err(map_sql)?,
    ))
}

#[derive(Deserialize, Serialize)]
struct Versioned {
    expected_edit_version: i64,
}

async fn delete_line(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, line_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<Versioned>,
) -> Result<Json<Value>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open(&pool, id).await?;
    let context = storage::bidding::MutationContext::new(
        durable_human_actor(&actor)?,
        required_idempotency_key(&headers)?,
        &body,
    )
    .map_err(|error| validation(&error.to_string()))?;
    Ok(Json(
        storage::bid_quote::delete_quote_line(
            &pool,
            id,
            line_id,
            body.expected_edit_version,
            &context,
        )
        .await
        .map_err(map_sql)?,
    ))
}

#[derive(Deserialize, Serialize)]
struct Reorder {
    expected_edit_version: i64,
    line_ids: Vec<Uuid>,
}

async fn reorder_lines(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<Reorder>,
) -> Result<Json<Value>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open(&pool, id).await?;
    let context = storage::bidding::MutationContext::new(
        durable_human_actor(&actor)?,
        required_idempotency_key(&headers)?,
        &body,
    )
    .map_err(|error| validation(&error.to_string()))?;
    Ok(Json(
        storage::bid_quote::reorder_quote_lines(
            &pool,
            id,
            body.expected_edit_version,
            &body.line_ids,
            &context,
        )
        .await
        .map_err(map_sql)?,
    ))
}

async fn preview_quote(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    Ok(Json(
        storage::bid_quote::preview_quote_totals(&pool, id)
            .await
            .map_err(map_sql)?,
    ))
}

#[derive(Deserialize, Serialize)]
struct Finalize {
    expected_edit_version: i64,
    expected_fact_revision: i64,
    expected_ceiling_revision: i64,
    expected_ceiling_identity_sha256: String,
    expected_pricing_revision: i64,
    expected_pricing_set_sha256: String,
    #[serde(default)]
    no_ceiling_reviewed: bool,
    no_ceiling_reason: Option<String>,
}

async fn finalize_quote(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<Finalize>,
) -> Result<Json<Value>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open(&pool, id).await?;
    let context = storage::bidding::MutationContext::new(
        durable_human_actor(&actor)?,
        required_idempotency_key(&headers)?,
        &body,
    )
    .map_err(|error| validation(&error.to_string()))?;
    Ok(Json(
        storage::bid_quote::finalize_quote(
            &pool,
            storage::bid_quote::FinalizeQuote {
                project_id: id,
                expected_edit_version: body.expected_edit_version,
                expected_fact_revision: body.expected_fact_revision,
                expected_ceiling_revision: body.expected_ceiling_revision,
                expected_ceiling_identity_sha256: &body.expected_ceiling_identity_sha256,
                expected_pricing_revision: body.expected_pricing_revision,
                expected_pricing_set_sha256: &body.expected_pricing_set_sha256,
                no_ceiling_reviewed: body.no_ceiling_reviewed,
                no_ceiling_reason: body.no_ceiling_reason.as_deref(),
            },
            &context,
        )
        .await
        .map_err(map_sql)?,
    ))
}

#[derive(Deserialize, Serialize)]
struct Reopen {
    expected_snapshot_id: Uuid,
    expected_fact_revision: i64,
    expected_pricing_revision: i64,
}

async fn reopen_quote(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<Reopen>,
) -> Result<Json<Value>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open(&pool, id).await?;
    let context = storage::bidding::MutationContext::new(
        durable_human_actor(&actor)?,
        required_idempotency_key(&headers)?,
        &body,
    )
    .map_err(|error| validation(&error.to_string()))?;
    Ok(Json(
        storage::bid_quote::reopen_quote(
            &pool,
            id,
            body.expected_snapshot_id,
            body.expected_fact_revision,
            body.expected_pricing_revision,
            &context,
        )
        .await
        .map_err(map_sql)?,
    ))
}

async fn get_snapshot(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, sid)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    storage::bid_quote::get_quote_snapshot(&pool, id, sid)
        .await
        .map_err(map_sql)?
        .ok_or_else(|| not_found("quote snapshot"))
        .map(Json)
}

async fn get_company_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    Ok(Json(
        storage::bid_submission::current_company_profile(&pool, id)
            .await
            .map_err(map_sql)?
            .unwrap_or(Value::Null),
    ))
}

#[derive(Deserialize, Serialize)]
struct CompanyProfileBody {
    expected_revision: i64,
    legal_name: String,
    unified_social_credit_code: String,
    registered_address: String,
    legal_representative: String,
    contact_name: String,
    contact_phone: String,
    contact_email: String,
}

async fn update_company_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<CompanyProfileBody>,
) -> Result<Json<Value>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open(&pool, id).await?;
    let context = storage::bidding::MutationContext::new(
        durable_human_actor(&actor)?,
        required_idempotency_key(&headers)?,
        &body,
    )
    .map_err(|error| validation(&error.to_string()))?;
    Ok(Json(
        storage::bid_submission::update_company_profile(
            &pool,
            storage::bid_submission::UpdateCompanyProfile {
                project_id: id,
                expected_revision: body.expected_revision,
                legal_name: &body.legal_name,
                uscc: &body.unified_social_credit_code,
                address: &body.registered_address,
                legal_rep: &body.legal_representative,
                contact_name: &body.contact_name,
                contact_phone: &body.contact_phone,
                contact_email: &body.contact_email,
            },
            &context,
        )
        .await
        .map_err(map_sql)?,
    ))
}

async fn get_submission_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    Ok(Json(
        storage::bid_submission::current_submission_profile(&pool, id)
            .await
            .map_err(map_sql)?
            .unwrap_or(Value::Null),
    ))
}

#[derive(Deserialize, Serialize)]
struct SubmissionProfileBody {
    expected_revision: i64,
    buyer_name: String,
    project_code: String,
    authorized_representative: String,
    submission_date: String,
    submission_place: String,
    seal_confirmed: bool,
    signature_confirmed: bool,
}

async fn update_submission_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<SubmissionProfileBody>,
) -> Result<Json<Value>, ApiErr> {
    let date = chrono::NaiveDate::parse_from_str(&body.submission_date, "%Y-%m-%d")
        .map_err(|_| validation("submission_date must be YYYY-MM-DD"))?;
    let actor = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open(&pool, id).await?;
    let context = storage::bidding::MutationContext::new(
        durable_human_actor(&actor)?,
        required_idempotency_key(&headers)?,
        &body,
    )
    .map_err(|error| validation(&error.to_string()))?;
    Ok(Json(
        storage::bid_submission::update_submission_profile(
            &pool,
            storage::bid_submission::UpdateSubmissionProfile {
                project_id: id,
                expected_revision: body.expected_revision,
                buyer_name: &body.buyer_name,
                project_code: &body.project_code,
                authorized_representative: &body.authorized_representative,
                submission_date: date,
                submission_place: &body.submission_place,
                seal_confirmed: body.seal_confirmed,
                signature_confirmed: body.signature_confirmed,
            },
            &context,
        )
        .await
        .map_err(map_sql)?,
    ))
}

async fn list_procedural(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    Ok(Json(json!({
        "classifications": storage::bid_submission::list_procedural_classifications(&pool, id).await.map_err(map_sql)?
    })))
}

#[derive(Deserialize, Serialize)]
struct OverrideBody {
    effective_kind: String,
    reason: String,
}

async fn override_classification(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, cid)): Path<(Uuid, Uuid)>,
    Json(body): Json<OverrideBody>,
) -> Result<Json<Value>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open(&pool, id).await?;
    let context = storage::bidding::MutationContext::new(
        durable_human_actor(&actor)?,
        required_idempotency_key(&headers)?,
        &body,
    )
    .map_err(|error| validation(&error.to_string()))?;
    Ok(Json(
        storage::bid_submission::override_procedural_classification(
            &pool,
            id,
            cid,
            &body.effective_kind,
            &body.reason,
            &context,
        )
        .await
        .map_err(map_sql)?,
    ))
}

#[derive(Deserialize, Serialize)]
struct ResolveBody {
    resolution: String,
    attachment_id: Option<Uuid>,
    reason: Option<String>,
}

async fn resolve_requirement(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, cid)): Path<(Uuid, Uuid)>,
    Json(body): Json<ResolveBody>,
) -> Result<Json<Value>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open(&pool, id).await?;
    let context = storage::bidding::MutationContext::new(
        durable_human_actor(&actor)?,
        required_idempotency_key(&headers)?,
        &body,
    )
    .map_err(|error| validation(&error.to_string()))?;
    Ok(Json(
        storage::bid_submission::resolve_procedural_requirement(
            &pool,
            id,
            cid,
            &body.resolution,
            body.attachment_id,
            body.reason.as_deref(),
            &context,
        )
        .await
        .map_err(map_sql)?,
    ))
}

async fn upload_attachment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open(&pool, id).await?;
    let mut kind = String::new();
    let mut bytes = Vec::new();
    while let Ok(Some(field)) = multipart.next_field().await {
        match field.name() {
            Some("kind") => kind = field.text().await.unwrap_or_default(),
            Some("file") => {
                bytes = field
                    .bytes()
                    .await
                    .map_err(|e| validation(&e.to_string()))?
                    .to_vec();
            }
            _ => {}
        }
    }
    if bytes.is_empty() || kind.is_empty() {
        return Err(validation("kind and file required"));
    }
    let (bytes, metadata) = validate_uploaded_bytes(bytes, true).await?;
    let actor = durable_human_actor(&actor)?;
    let digest = domain::sha256_hex(&bytes);
    let object_ref = storage::object_ref(&digest);
    let attachment_id = Uuid::new_v4();
    let payload = json!({"project_id":id,"kind":kind,"digest":digest,
        "media_type":metadata.media_type,"byte_length":metadata.byte_length,
        "pixel_width":metadata.pixel_width,"pixel_height":metadata.pixel_height});
    let context = storage::bidding::MutationContext::new(
        actor.clone(),
        required_idempotency_key(&headers)?,
        &payload,
    )
    .map_err(|error| validation(&error.to_string()))?;
    let staging_id = Uuid::new_v4();
    stage_uploaded_bytes(
        &pool,
        staging_id,
        &object_ref,
        &digest,
        metadata.media_type,
        &bytes,
        &actor,
    )
    .await?;
    let uploaded = storage::bid_submission::upload_attachment(
        &pool,
        storage::bid_submission::UploadAttachment {
            staging_id,
            id: attachment_id,
            project_id: id,
            kind: &kind,
            object_ref: &object_ref,
            digest: &digest,
            media_type: metadata.media_type,
            byte_length: metadata.byte_length,
            pixel_width: metadata.pixel_width,
            pixel_height: metadata.pixel_height,
        },
        &context,
    )
    .await;
    if uploaded.is_err() {
        abandon_staged_upload(&pool, staging_id, &actor).await;
    }
    let uploaded = uploaded.map_err(map_sql)?;
    if let Some(preparation_job_id) = uploaded
        .get("preparation_job_id")
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
    {
        match runtime::enqueue_bid_prepare_attachment_v1(preparation_job_id).await {
            Ok(Some(_)) => {}
            Ok(None) => tracing::warn!(%preparation_job_id,
                "attachment preparation queue unavailable; durable job remains pending"),
            Err(error) => tracing::warn!(%preparation_job_id,%error,
                "attachment preparation enqueue failed; durable job remains pending"),
        }
    }
    Ok((StatusCode::CREATED, Json(uploaded)))
}

#[derive(Deserialize, Serialize)]
struct AttachmentAction {
    expected_revision: i32,
    reason: Option<String>,
}

async fn mutate_attachment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, aid, action)): Path<(Uuid, Uuid, String)>,
    Json(body): Json<AttachmentAction>,
) -> Result<Json<Value>, ApiErr> {
    if !matches!(
        action.as_str(),
        "validate" | "invalidate" | "confirm" | "reject" | "delete"
    ) {
        return Err(validation("unknown attachment action"));
    }
    let actor = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open(&pool, id).await?;
    if action == "validate" {
        let (digest, media_type, byte_length, pixel_width, pixel_height) =
            storage::bid_submission::attachment_validation_input(&pool, id, aid)
                .await
                .map_err(map_sql)?
                .ok_or_else(|| not_found("attachment"))?;
        let digest_for_read = digest.clone();
        let bytes = tokio::task::spawn_blocking(move || storage::read_blob(&digest_for_read))
            .await
            .map_err(|_| validation("attachment validation task failed"))?
            .map_err(|_| validation("attachment bytes unavailable"))?;
        let actual = storage::bid_submission::validate_upload_bytes(&bytes, true)
            .map_err(|error| validation(&error.to_string()))?;
        if domain::sha256_hex(&bytes) != digest
            || actual.media_type != media_type
            || actual.byte_length != byte_length
            || actual.pixel_width != pixel_width
            || actual.pixel_height != pixel_height
        {
            return Err(validation("attachment validation identity mismatch"));
        }
    }
    let context = storage::bidding::MutationContext::new(
        durable_human_actor(&actor)?,
        required_idempotency_key(&headers)?,
        &body,
    )
    .map_err(|error| validation(&error.to_string()))?;
    Ok(Json(
        storage::bid_submission::mutate_attachment(
            &pool,
            id,
            aid,
            &action,
            body.expected_revision,
            body.reason.as_deref(),
            &context,
        )
        .await
        .map_err(map_sql)?,
    ))
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ShotSetBody {
    expected_revision: i64,
    shot_artifact_ids: Vec<Uuid>,
}

async fn upload_shot(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open(&pool, id).await?;
    let mut bytes = Vec::new();
    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name() == Some("file") {
            bytes = field
                .bytes()
                .await
                .map_err(|error| validation(&error.to_string()))?
                .to_vec();
        }
    }
    let (bytes, metadata) = validate_uploaded_bytes(bytes, false).await?;
    let actor = durable_human_actor(&actor)?;
    let digest = domain::sha256_hex(&bytes);
    let object_ref = storage::object_ref(&digest);
    let shot_id = Uuid::new_v4();
    let payload = json!({"project_id":id,"digest":digest,
        "media_type":metadata.media_type,"byte_length":metadata.byte_length,
        "pixel_width":metadata.pixel_width,"pixel_height":metadata.pixel_height});
    let context = storage::bidding::MutationContext::new(
        actor.clone(),
        required_idempotency_key(&headers)?,
        &payload,
    )
    .map_err(|error| validation(&error.to_string()))?;
    let pixel_width = metadata
        .pixel_width
        .ok_or_else(|| validation("image width missing"))?;
    let pixel_height = metadata
        .pixel_height
        .ok_or_else(|| validation("image height missing"))?;
    let staging_id = Uuid::new_v4();
    stage_uploaded_bytes(
        &pool,
        staging_id,
        &object_ref,
        &digest,
        metadata.media_type,
        &bytes,
        &actor,
    )
    .await?;
    let uploaded = storage::bid_submission::upload_shot_artifact(
        &pool,
        storage::bid_submission::UploadShotArtifact {
            staging_id,
            id: shot_id,
            project_id: id,
            object_ref: &object_ref,
            digest: &digest,
            media_type: metadata.media_type,
            byte_length: metadata.byte_length,
            pixel_width,
            pixel_height,
        },
        &context,
    )
    .await;
    if uploaded.is_err() {
        abandon_staged_upload(&pool, staging_id, &actor).await;
    }
    Ok((StatusCode::CREATED, Json(uploaded.map_err(map_sql)?)))
}

async fn replace_shots(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<ShotSetBody>,
) -> Result<Json<Value>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open(&pool, id).await?;
    let context = storage::bidding::MutationContext::new(
        durable_human_actor(&actor)?,
        required_idempotency_key(&headers)?,
        &body,
    )
    .map_err(|error| validation(&error.to_string()))?;
    Ok(Json(
        storage::bid_submission::replace_shot_set(
            &pool,
            id,
            body.expected_revision,
            &body.shot_artifact_ids,
            &context,
        )
        .await
        .map_err(map_sql)?,
    ))
}

async fn list_parts(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    Ok(Json(json!({
        "required_part_keys": storage::bid_submission::required_part_keys(&pool, id).await.map_err(map_sql)?,
        "parts": storage::bid_submission::current_part_status(&pool, id).await.map_err(map_sql)?
    })))
}

async fn get_part(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, key)): Path<(Uuid, String)>,
) -> Result<Json<Value>, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    storage::bid_submission::get_part(&pool, id, &key)
        .await
        .map_err(map_sql)?
        .ok_or_else(|| not_found("part"))
        .map(Json)
}

async fn list_attachments(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    Ok(Json(json!({
        "attachments": storage::bid_submission::list_attachments(&pool, id).await.map_err(map_sql)?
    })))
}

async fn get_shots(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    Ok(Json(json!({
        "shot_set": storage::bid_submission::current_shot_set(&pool, id).await.map_err(map_sql)?
    })))
}

async fn list_outputs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    Ok(Json(json!({
        "outputs": storage::bid_submission::list_outputs(&pool, id).await.map_err(map_sql)?
    })))
}

#[derive(Deserialize, Serialize)]
struct UpdatePart {
    expected_content_revision: i64,
    markdown: String,
}

async fn update_part(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, key)): Path<(Uuid, String)>,
    Json(body): Json<UpdatePart>,
) -> Result<Json<Value>, ApiErr> {
    if bid::submission::template_slot_for_part_key(&key).is_none() {
        return Err(validation("unknown part key"));
    }
    let actor = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open(&pool, id).await?;
    let context = storage::bidding::MutationContext::new(
        durable_human_actor(&actor)?,
        required_idempotency_key(&headers)?,
        &body,
    )
    .map_err(|error| validation(&error.to_string()))?;
    Ok(Json(
        storage::bid_submission::update_part(
            &pool,
            id,
            &key,
            body.expected_content_revision,
            body.markdown.as_bytes(),
            &context,
        )
        .await
        .map_err(map_sql)?,
    ))
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RegenPart {
    expected_content_revision: i64,
    expected_dependency_sha256: Option<String>,
}

async fn regenerate_part(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, key)): Path<(Uuid, String)>,
    Json(body): Json<RegenPart>,
) -> Result<Json<Value>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open(&pool, id).await?;
    let context = storage::bidding::MutationContext::new(
        durable_human_actor(&actor)?,
        required_idempotency_key(&headers)?,
        &body,
    )
    .map_err(|error| validation(&error.to_string()))?;
    Ok(Json(
        storage::bid_submission::regenerate_part(
            &pool,
            id,
            &key,
            body.expected_content_revision,
            body.expected_dependency_sha256.as_deref(),
            &context,
        )
        .await
        .map_err(map_sql)?,
    ))
}

async fn list_gate_issues(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    let format = query.get("format").map(String::as_str).unwrap_or("pdf");
    Ok(Json(
        storage::bid_submission::list_gate_issues(&pool, id, format)
            .await
            .map_err(map_sql)?,
    ))
}

#[derive(Deserialize, Serialize)]
struct CreateManifest {
    format: String,
}

async fn create_manifest(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<CreateManifest>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open(&pool, id).await?;
    let manifest_id = Uuid::new_v4();
    let context = storage::bidding::MutationContext::new(
        durable_human_actor(&actor)?,
        required_idempotency_key(&headers)?,
        &body,
    )
    .map_err(|error| validation(&error.to_string()))?;
    Ok((
        StatusCode::CREATED,
        Json(
            storage::bid_submission::create_submission_manifest(
                &pool,
                manifest_id,
                id,
                &body.format,
                &context,
            )
            .await
            .map_err(map_sql)?,
        ),
    ))
}

#[derive(Deserialize, Serialize)]
struct RenderBody {
    expected_manifest_sha256: String,
}

async fn render_manifest(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, mid)): Path<(Uuid, Uuid)>,
    Json(body): Json<RenderBody>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    require_open(&pool, id).await?;
    let actor = durable_human_actor(&actor)?;
    let idempotency_key = required_idempotency_key(&headers)?;
    let input = storage::bid_submission::manifest_render_input(&pool, id, mid)
        .await
        .map_err(map_sql)?;
    if input.get("content_sha256").and_then(Value::as_str)
        != Some(body.expected_manifest_sha256.as_str())
    {
        return Err(fail(
            StatusCode::CONFLICT,
            "MANIFEST_SHA256_MISMATCH",
            "manifest identity changed",
        ));
    }
    let format = match input.get("format").and_then(Value::as_str) {
        Some("pdf") => bid::submission::GateFormat::Pdf,
        Some("docx") => bid::submission::GateFormat::Docx,
        _ => return Err(validation("invalid manifest format")),
    };
    if input.get("renderer_contract") != Some(&bid::renderer_contract_identity(format)) {
        return Err(fail(
            StatusCode::CONFLICT,
            "RENDERER_CONTRACT_MISMATCH",
            "manifest renderer contract differs from the running binary",
        ));
    }
    let context = storage::bidding::MutationContext::new(actor, idempotency_key, &body)
        .map_err(|error| validation(&error.to_string()))?;
    let scheduled = storage::bid_submission::schedule_submission_render(
        &pool,
        Uuid::new_v4(),
        id,
        mid,
        &body.expected_manifest_sha256,
        &context,
    )
    .await
    .map_err(map_sql)?;
    let render_job_id = scheduled
        .get("render_job_id")
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| {
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "SUBMISSION_RENDER_JOB_INVALID",
                "durable render job did not return an id",
            )
        })?;
    match runtime::enqueue_bid_render_submission_v1(render_job_id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            tracing::warn!(%render_job_id, "render queue unavailable; durable job remains pending")
        }
        Err(error) => {
            tracing::warn!(%render_job_id, %error, "render enqueue failed; durable job remains pending")
        }
    }
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({
            "render_job_id": render_job_id,
            "manifest_id": mid,
            "status": "queued"
        })),
    ))
}

async fn get_render_job(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, jid)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    let job = storage::bid_submission::get_submission_render_job(&pool, id, jid)
        .await
        .map_err(map_sql)?
        .ok_or_else(|| not_found("submission render job"))?;
    Ok(Json(job))
}

async fn download_output(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, oid)): Path<(Uuid, Uuid)>,
) -> Result<impl IntoResponse, ApiErr> {
    let _ = actor_from(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    let meta = storage::bid_submission::download_submission_output(&pool, id, oid)
        .await
        .map_err(map_sql)?;
    let hash = meta
        .get("digest")
        .and_then(Value::as_str)
        .ok_or_else(|| not_found("submission artifact"))?
        .to_string();
    let object_ref = meta
        .get("object_ref")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let format = meta
        .get("format")
        .and_then(Value::as_str)
        .unwrap_or("pdf")
        .to_string();
    let bytes = storage::read_blob(&hash).map_err(|_| not_found("submission artifact bytes"))?;
    if object_ref != storage::object_ref(&hash) || domain::sha256_hex(&bytes) != hash {
        return Err(fail(
            StatusCode::INTERNAL_SERVER_ERROR,
            "SUBMISSION_ARTIFACT_IDENTITY_MISMATCH",
            "submission artifact bytes do not match frozen identity",
        ));
    }
    let mime = if format == "pdf" {
        "application/pdf"
    } else {
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
    };
    Ok((
        [
            (header::CONTENT_TYPE, mime.to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"submission-{oid}.{format}\""),
            ),
        ],
        bytes,
    ))
}

#[derive(Deserialize, Serialize)]
struct RegisterKindRouter {
    version: String,
    canonical_payload: String,
}

async fn register_kind_router(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<RegisterKindRouter>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    require_admin(&state, &actor)?;
    let pool = require_bid_pool().await?;
    let bytes = body.canonical_payload.as_bytes();
    let digest = domain::sha256_hex(bytes);
    let context = storage::bidding::MutationContext::new(
        durable_human_actor(&actor)?,
        required_idempotency_key(&headers)?,
        &body,
    )
    .map_err(|error| validation(&error.to_string()))?;
    Ok((
        StatusCode::CREATED,
        Json(
            storage::bidding::register_kind_router_contract(
                &pool,
                &body.version,
                bytes,
                &digest,
                &context,
            )
            .await
            .map_err(map_sql)?,
        ),
    ))
}

#[derive(Deserialize, Serialize)]
struct PromoteKindRouter {
    target_version: String,
    expected_current_version: String,
    expected_promotion_generation: i64,
}

async fn promote_kind_router(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<PromoteKindRouter>,
) -> Result<Json<Value>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    require_admin(&state, &actor)?;
    let pool = require_bid_pool().await?;
    let context = storage::bidding::MutationContext::new(
        durable_human_actor(&actor)?,
        required_idempotency_key(&headers)?,
        &body,
    )
    .map_err(|error| validation(&error.to_string()))?;
    Ok(Json(
        storage::bidding::promote_kind_router(
            &pool,
            &body.target_version,
            &body.expected_current_version,
            body.expected_promotion_generation,
            &context,
        )
        .await
        .map_err(map_sql)?,
    ))
}

async fn register_procedural_router(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<RegisterKindRouter>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    require_admin(&state, &actor)?;
    let pool = require_bid_pool().await?;
    let bytes = body.canonical_payload.as_bytes();
    let digest = domain::sha256_hex(bytes);
    let context = storage::bidding::MutationContext::new(
        durable_human_actor(&actor)?,
        required_idempotency_key(&headers)?,
        &body,
    )
    .map_err(|error| validation(&error.to_string()))?;
    Ok((
        StatusCode::CREATED,
        Json(
            storage::bidding::register_procedural_router_contract(
                &pool,
                &body.version,
                bytes,
                &digest,
                &context,
            )
            .await
            .map_err(map_sql)?,
        ),
    ))
}

async fn promote_procedural_router(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<PromoteKindRouter>,
) -> Result<Json<Value>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    require_admin(&state, &actor)?;
    let pool = require_bid_pool().await?;
    let context = storage::bidding::MutationContext::new(
        durable_human_actor(&actor)?,
        required_idempotency_key(&headers)?,
        &body,
    )
    .map_err(|error| validation(&error.to_string()))?;
    Ok(Json(
        storage::bidding::promote_procedural_router(
            &pool,
            &body.target_version,
            &body.expected_current_version,
            body.expected_promotion_generation,
            &context,
        )
        .await
        .map_err(map_sql)?,
    ))
}

#[derive(Deserialize, Serialize)]
struct RegisterTemplateContract {
    version: String,
    canonical_payload: String,
}

async fn register_template_contract(
    State(state): State<AppState>,
    Path(slot): Path<String>,
    headers: HeaderMap,
    Json(body): Json<RegisterTemplateContract>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    require_admin(&state, &actor)?;
    let pool = require_bid_pool().await?;
    let bytes = body.canonical_payload.as_bytes();
    let digest = domain::sha256_hex(bytes);
    let request = json!({
        "slot":&slot,
        "version":&body.version,
        "canonical_payload":&body.canonical_payload
    });
    let context = storage::bidding::MutationContext::new(
        durable_human_actor(&actor)?,
        required_idempotency_key(&headers)?,
        &request,
    )
    .map_err(|error| validation(&error.to_string()))?;
    Ok((
        StatusCode::CREATED,
        Json(
            storage::bidding::register_template_contract(
                &pool,
                &slot,
                &body.version,
                bytes,
                &digest,
                &context,
            )
            .await
            .map_err(map_sql)?,
        ),
    ))
}

async fn promote_template_contract(
    State(state): State<AppState>,
    Path(slot): Path<String>,
    headers: HeaderMap,
    Json(body): Json<PromoteKindRouter>,
) -> Result<Json<Value>, ApiErr> {
    let actor = actor_from(&headers, &state).await?;
    require_admin(&state, &actor)?;
    let pool = require_bid_pool().await?;
    let request = json!({
        "slot":&slot,
        "target_version":&body.target_version,
        "expected_current_version":&body.expected_current_version,
        "expected_promotion_generation":body.expected_promotion_generation
    });
    let context = storage::bidding::MutationContext::new(
        durable_human_actor(&actor)?,
        required_idempotency_key(&headers)?,
        &request,
    )
    .map_err(|error| validation(&error.to_string()))?;
    Ok(Json(
        storage::bidding::promote_template_contract(
            &pool,
            &slot,
            &body.target_version,
            &body.expected_current_version,
            body.expected_promotion_generation,
            &context,
        )
        .await
        .map_err(map_sql)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored_zip(names: &[&str]) -> Vec<u8> {
        let mut output = Vec::new();
        let mut central_entries = Vec::new();
        for name in names {
            let name = name.as_bytes();
            let local_offset = output.len() as u32;
            output.extend_from_slice(b"PK\x03\x04");
            output.extend_from_slice(&20u16.to_le_bytes());
            output.extend_from_slice(&[0; 20]);
            output.extend_from_slice(&(name.len() as u16).to_le_bytes());
            output.extend_from_slice(&0u16.to_le_bytes());
            output.extend_from_slice(name);

            let mut central = Vec::new();
            central.extend_from_slice(b"PK\x01\x02");
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&[0; 20]);
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&[0; 12]);
            central.extend_from_slice(&local_offset.to_le_bytes());
            central.extend_from_slice(name);
            central_entries.push(central);
        }
        let central_offset = output.len() as u32;
        for entry in central_entries {
            output.extend_from_slice(&entry);
        }
        let central_size = output.len() as u32 - central_offset;
        output.extend_from_slice(b"PK\x05\x06");
        output.extend_from_slice(&0u16.to_le_bytes());
        output.extend_from_slice(&0u16.to_le_bytes());
        output.extend_from_slice(&(names.len() as u16).to_le_bytes());
        output.extend_from_slice(&(names.len() as u16).to_le_bytes());
        output.extend_from_slice(&central_size.to_le_bytes());
        output.extend_from_slice(&central_offset.to_le_bytes());
        output.extend_from_slice(&0u16.to_le_bytes());
        output
    }

    #[test]
    fn tender_document_media_type_rejects_unrecognized_bytes() {
        assert_eq!(
            tender_document_media_type(b"not a PDF or DOCX"),
            Err("TENDER_DOCUMENT_TYPE_INVALID")
        );
    }

    #[test]
    fn tender_document_media_type_accepts_pdf_and_docx() {
        assert_eq!(
            tender_document_media_type(b"%PDF-1.7\n%%EOF\n"),
            Ok(PDF_MEDIA_TYPE)
        );
        assert_eq!(
            tender_document_media_type(&stored_zip(&["[Content_Types].xml", "word/document.xml"])),
            Ok(DOCX_MEDIA_TYPE)
        );
    }

    #[test]
    fn tender_document_media_type_rejects_invalid_pdf_and_non_docx_zip() {
        assert_eq!(
            tender_document_media_type(b"%PDF-1.7\n%%EOF\ntrailing"),
            Err("TENDER_PDF_STRUCTURE_INVALID")
        );
        assert_eq!(
            tender_document_media_type(&stored_zip(&["[Content_Types].xml", "xl/workbook.xml"])),
            Err("TENDER_DOCX_STRUCTURE_INVALID")
        );
    }

    #[test]
    fn bid_project_path_identity_is_fail_closed() {
        let project_id = Uuid::new_v4();
        assert_eq!(
            bid_project_id_from_path(&format!("/api/v1/bids/{project_id}/quote")),
            Ok(Some(project_id))
        );
        assert_eq!(bid_project_id_from_path("/api/v1/bids"), Ok(None));
        assert_eq!(
            bid_project_id_from_path("/api/v1/maintenance/kind-router/register"),
            Ok(None)
        );
        assert!(bid_project_id_from_path("/api/v1/bids/%31/quote").is_err());
        assert!(bid_project_id_from_path("/api/v1/bids/not-a-uuid/quote").is_err());
    }

    #[test]
    fn route_pick_domain_errors_have_stable_http_statuses() {
        for code in [
            "CURRENT_MATCHING_REPORT_MISMATCH",
            "ROUTE_PICK_REVISION_MISMATCH",
        ] {
            assert_eq!(
                map_sql(sqlx::Error::Protocol(code.into()))
                    .into_response()
                    .status(),
                StatusCode::CONFLICT,
                "{code}"
            );
        }
        for code in [
            "PICK_ITEM_NOT_VISIBLE_SUPPORTED",
            "ROUTE_PICK_REQUIRES_TECHNICAL_REPORT",
        ] {
            assert_eq!(
                map_sql(sqlx::Error::Protocol(code.into()))
                    .into_response()
                    .status(),
                StatusCode::BAD_REQUEST,
                "{code}"
            );
        }
    }
}
