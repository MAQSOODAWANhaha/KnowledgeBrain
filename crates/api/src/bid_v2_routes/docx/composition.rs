use super::*;

mod report;

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/docx/versions/{version_id}/composition-report",
            get(report::download),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/docx-fills/basis",
            get(basis),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/docx-fills",
            post(create_fill),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/docx-fills/{request_id}/stop",
            post(stop_fill),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/docx-fills/latest",
            get(latest),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/docx-fills/{request_id}",
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
/// 用户触发的填章：填的是**当前这一版** Word，所以必须带 `expected`，并且在这里
/// 就把它回读一遍——读不出章的文档当场报错，而不是排队几分钟后再失败。回读出的
/// 章树摘要冻进请求，worker 用同一份字节重算并核对。
async fn create_fill(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workspace): Path<Uuid>,
    BidJson(body): BidJson<RoundMetadata>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let key = required_idempotency_key(&headers)?;
    let pool = require_bid_pool().await?;
    validate_metadata(&body)?;
    let expected = body
        .expected
        .clone()
        .ok_or_else(|| validation("filling requires the document version it fills"))?;
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
    let receipt =
        match replay {
            Some(receipt) => receipt,
            None => {
                let config =
                    bidding::tender_analysis::agent::Config::from_environment().map_err(|e| {
                        fail(
                            StatusCode::SERVICE_UNAVAILABLE,
                            "AGENT_PROVIDER_UNAVAILABLE",
                            e.message,
                        )
                    })?;
                let sha = expected.docx_sha256.clone();
                let docx = tokio::task::spawn_blocking(move || platform::read_blob(&sha))
                    .await
                    .map_err(|_| {
                        fail(
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "DOCX_READ_FAILED",
                            "document read task failed",
                        )
                    })?
                    .map_err(|error| {
                        tracing::error!(%error, "Draft fill source object read failed");
                        fail(
                            StatusCode::SERVICE_UNAVAILABLE,
                            "DOCX_UNAVAILABLE",
                            "document file is unavailable",
                        )
                    })?;
                let prepared = bidding::docx_composition::fill::prepare(
                    &pool,
                    bidding::docx_composition::fill::FillIntent {
                        workspace_id: workspace,
                        basis: body.basis,
                        expected: body.expected,
                        actor: actor.clone(),
                        docx,
                    },
                    config,
                    &tokio_util::sync::CancellationToken::new(),
                )
                .await
                .map_err(map_composition)?;
                if !prepared.seed.iter().any(|item| {
                    item.status == bidding::tender_analysis::draft::DraftStatus::Pending
                }) {
                    let current = bidding::docx_round::get_current_docx(&pool, workspace, &actor)
                        .await
                        .map_err(map_docx_sql)?;
                    if current.as_ref().is_none_or(|value| {
                        value["version_id"] != json!(expected.version_id)
                            || value["docx_sha256"] != json!(expected.docx_sha256)
                    }) {
                        return Err(fail(
                            StatusCode::CONFLICT,
                            "WORKSPACE_CAS_CONFLICT",
                            "document changed during fill preparation",
                        ));
                    }
                    return Ok((
                        StatusCode::OK,
                        Json(json!({"status":"unchanged", "current":expected})),
                    ));
                }
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

/// 停止填充：只记一次意向，填章 run 在当前章收尾后自己停下并照常出稿。这里不撤
/// 租约、不杀任务，所以重复点、或在任务已经结束后点，都只是拿回当前状态。
async fn stop_fill(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace, request)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    let value: Value =
        sqlx::query_scalar("SELECT kb_bid_v2_request_docx_fill_stop($1,$2,$3::kb_actor_identity)")
            .bind(workspace)
            .bind(request)
            .bind(actor)
            .fetch_one(&pool)
            .await
            .map_err(map_docx_sql)?;
    Ok(Json(value))
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
