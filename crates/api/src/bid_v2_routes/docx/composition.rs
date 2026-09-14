use super::*;
use bidding::docx_composition::{agent::Config, postgres};

mod report;

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/docx/versions/{version_id}/composition-report",
            get(report::download),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/docx-compositions/basis",
            get(basis),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/docx-compositions",
            post(create),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/docx-compositions/latest",
            get(latest),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/docx-compositions/{request_id}",
            get(status),
        )
}

fn map_composition(error: bidding::agent_error::AgentError) -> ApiErr {
    let status = match error.code.as_str() {
        "WORKSPACE_CAS_CONFLICT" => StatusCode::CONFLICT,
        "INTERNAL" | "AGENT_PROVIDER_UNAVAILABLE" => StatusCode::SERVICE_UNAVAILABLE,
        _ => StatusCode::UNPROCESSABLE_ENTITY,
    };
    fail(status, &error.code, error.message)
}
async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workspace): Path<Uuid>,
    BidJson(body): BidJson<RoundMetadata>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let key = required_idempotency_key(&headers)?;
    let pool = require_bid_pool().await?;
    validate_metadata(&body)?;
    let input = json!({"basis":body.basis,"expected":body.expected});
    let replay: Option<Value> = sqlx::query_scalar(
        "SELECT kb_bid_v2_replay_docx_composition_submission($1,$2,$3::kb_actor_identity,$4)",
    )
    .bind(workspace)
    .bind(&input)
    .bind(&actor)
    .bind(&key)
    .fetch_one(&pool)
    .await
    .map_err(map_docx_sql)?;
    let receipt = match replay {
        Some(receipt) => receipt,
        None => {
            let config = Config::from_environment().map_err(|e| {
                fail(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "AGENT_PROVIDER_UNAVAILABLE",
                    e.message,
                )
            })?;
            let prepared =
                postgres::prepare(&pool, workspace, body.basis, body.expected, &actor, config)
                    .await
                    .map_err(map_composition)?;
            sqlx::query_scalar(
                "SELECT kb_bid_v2_submit_docx_composition_request($1,$2,$3::kb_actor_identity,$4)",
            )
            .bind(sqlx::types::Json(&prepared.request))
            .bind(prepared.request.config.contract_definition())
            .bind(&actor)
            .bind(&key)
            .fetch_one(&pool)
            .await
            .map_err(map_docx_sql)?
        }
    };
    enqueue_if_pending(&pool, &receipt).await?;
    Ok((StatusCode::ACCEPTED, Json(receipt)))
}
async fn read_status(
    state: AppState,
    headers: HeaderMap,
    workspace: Uuid,
    request: Option<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    let value: Option<Value> = sqlx::query_scalar(
        "SELECT kb_bid_v2_get_docx_composition_request($1,$2,$3::kb_actor_identity)",
    )
    .bind(workspace)
    .bind(request)
    .bind(actor)
    .fetch_one(&pool)
    .await
    .map_err(map_docx_sql)?;
    match (value, request) {
        (Some(value), _) => Ok(Json(value)),
        (None, None) => Ok(Json(Value::Null)),
        (None, Some(_)) => Err(fail(
            StatusCode::NOT_FOUND,
            "DOCX_COMPOSITION_NOT_FOUND",
            "composition request not found",
        )),
    }
}
async fn latest(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workspace): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    read_status(state, headers, workspace, None).await
}
async fn status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace, request)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiErr> {
    read_status(state, headers, workspace, Some(request)).await
}

async fn basis(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workspace): Path<Uuid>,
) -> Result<Json<Option<DocxRoundBasis>>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    let value: Option<sqlx::types::Json<DocxRoundBasis>> =
        sqlx::query_scalar("SELECT kb_bid_v2_get_docx_composition_basis($1,$2::kb_actor_identity)")
            .bind(workspace)
            .bind(actor)
            .fetch_one(&pool)
            .await
            .map_err(map_docx_sql)?;
    Ok(Json(value.map(|v| v.0)))
}
