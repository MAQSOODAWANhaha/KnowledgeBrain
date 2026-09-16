//! ONLYOFFICE transport. No defaults for deployments, keys or trust boundaries.
use super::*;
use axum::extract::{OriginalUri, Query};
use axum::http::header::HOST;
use bidding::onlyoffice_conversion::{Config, Error as TransportError, ErrorKind, bounded_body};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use reqwest::Url;
use serde::Serialize;

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/docx/editor",
            post(open),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/docx/editor/save",
            post(force_save),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/docx/editor/{editor_key}/saves/{save_id}",
            get(saved_receipt),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/docx/editor/{editor_key}/source",
            get(source),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/docx/editor/{editor_key}/callback",
            post(callback),
        )
}

fn map_transport(error: TransportError) -> ApiErr {
    let status = match error.kind {
        ErrorKind::Invalid => StatusCode::BAD_REQUEST,
        ErrorKind::Unauthorized => StatusCode::UNAUTHORIZED,
        ErrorKind::Forbidden => StatusCode::FORBIDDEN,
        ErrorKind::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
    };
    fail(status, error.code, error.message)
}

fn transport_error(code: &str, message: &str) -> ApiErr {
    fail(StatusCode::SERVICE_UNAVAILABLE, code, message)
}

fn hostname_from_host(host: &str) -> &str {
    let host = host.trim();
    if let Some(rest) = host.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else {
        match host.rsplit_once(':') {
            Some((name, port)) if port.chars().all(|c| c.is_ascii_digit()) => name,
            _ => host,
        }
    }
}

fn browser_document_server(server: &Url, headers: &HeaderMap) -> Result<Url, ApiErr> {
    let raw = headers
        .get("x-forwarded-host")
        .or_else(|| headers.get(HOST))
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let hostname = hostname_from_host(raw.split(',').next().unwrap_or("").trim());
    if hostname.is_empty() {
        return Ok(server.clone());
    }
    let mut url = server.clone();
    url.set_host(Some(hostname))
        .map_err(|_| validation("invalid ONLYOFFICE origin"))?;
    Ok(url)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Capability {
    aud: String,
    workspace_id: Uuid,
    round_id: Uuid,
    editor_key: Uuid,
    version_id: Uuid,
    actor: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ticket {
    token: String,
}

fn read_capability(
    config: &Config,
    token: &str,
    purpose: &str,
    workspace_id: Uuid,
    key: Uuid,
) -> Result<Capability, ApiErr> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_audience(&[purpose]);
    // This is a signed scope binding, not a bearer authorization. Both routes
    // also require a fresh Document Server request JWT. Lifetime/revocation is
    // the existing active editor key (or an exact completed callback receipt).
    validation.required_spec_claims.remove("exp");
    validation.validate_exp = false;
    let claims = decode::<Capability>(
        token,
        &DecodingKey::from_secret(config.capability_secret().as_bytes()),
        &validation,
    )
    .map_err(|_| {
        fail(
            StatusCode::UNAUTHORIZED,
            "ONLYOFFICE_CAPABILITY_INVALID",
            "document scope binding is invalid",
        )
    })?
    .claims;
    if claims.workspace_id != workspace_id || claims.editor_key != key {
        return Err(fail(
            StatusCode::FORBIDDEN,
            "ONLYOFFICE_CAPABILITY_SCOPE",
            "document capability scope mismatch",
        ));
    }
    Ok(claims)
}

fn identity(value: &Value, field: &str) -> Result<Uuid, ApiErr> {
    value[field]
        .as_str()
        .and_then(|v| Uuid::parse_str(v).ok())
        .ok_or_else(|| validation("DOCX stored identity missing"))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpenEditor {
    version_id: Uuid,
    docx_sha256: String,
    language: Option<String>,
}

fn validate_language(language: &str) -> Result<(), ApiErr> {
    // Language/subtag syntax only. Available translations belong to the
    // deployed Document Server; do not maintain a second language catalog.
    let mut parts = language.split('-');
    let primary = parts.next().unwrap_or_default();
    if !(2..=8).contains(&primary.len())
        || !primary.bytes().all(|byte| byte.is_ascii_alphabetic())
        || !parts.all(|part| {
            (1..=8).contains(&part.len()) && part.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
    {
        return Err(validation("invalid editor language tag"));
    }
    Ok(())
}

async fn open(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workspace): Path<Uuid>,
    BidJson(expected): BidJson<OpenEditor>,
) -> Result<Json<Value>, ApiErr> {
    let (user_id, actor) = human_actor(&headers, &state).await?;
    let request_key = required_idempotency_key(&headers)?;
    if let Some(language) = &expected.language {
        validate_language(language)?;
    }
    let config = Config::load().map_err(map_transport)?;
    let pool = require_bid_pool().await?;
    let version =
        bidding::docx_round::get_docx_version(&pool, workspace, expected.version_id, &actor)
            .await
            .map_err(map_docx_sql)?;
    read_verified_docx(version).await?;
    // Resolve the authenticated account server-side. A browser cannot select
    // another collaborator identity or supply an unsigned display name.
    let (_, email) = knowledge::find_user_by_id(&pool, user_id)
        .await
        .map_err(map_docx_sql)?
        .ok_or_else(crate::err::unauthorized)?;
    let opened = bidding::docx_round::editor_command(&pool, workspace, "open",
        &json!({"expected_version_id":expected.version_id,"expected_docx_sha256":expected.docx_sha256}), &actor, &request_key).await.map_err(map_docx_sql)?;
    let key = identity(&opened, "editor_key")?;
    let session = bidding::docx_round::get_editor(&pool, workspace, key, &actor)
        .await
        .map_err(map_docx_sql)?;
    read_verified_docx(session["base"].clone()).await?;
    let mut capability = Capability {
        aud: "docx-source".into(),
        workspace_id: workspace,
        round_id: identity(&session, "round_id")?,
        editor_key: key,
        version_id: identity(&session["base"], "version_id")?,
        actor: actor.clone(),
    };
    let prefix = format!("api/v2/submission-workspaces/{workspace}/docx/editor/{key}");
    let mut source_url = config
        .api
        .join(&format!("{prefix}/source"))
        .map_err(|_| validation("invalid API origin"))?;
    source_url.query_pairs_mut().append_pair(
        "token",
        &config.sign(&capability, true).map_err(map_transport)?,
    );
    capability.aud = "docx-callback".into();
    let mut callback_url = config
        .api
        .join(&format!("{prefix}/callback"))
        .map_err(|_| validation("invalid API origin"))?;
    callback_url.query_pairs_mut().append_pair(
        "token",
        &config.sign(&capability, true).map_err(map_transport)?,
    );
    let mut editor = json!({"documentType":"word", "document": {"fileType":"docx", "key":key.to_string(),
        "title":format!("{workspace}.docx"),"url":source_url.as_str(),"permissions":{"edit":true}},
        "editorConfig":{"mode":"edit","callbackUrl":callback_url.as_str(),"user":{"id":actor,"name":email},
            "customization":{"forcesave":false}},"exp":config.expiration().map_err(map_transport)?});
    if let Some(language) = expected.language {
        editor["editorConfig"]["lang"] = json!(language);
    }
    editor["token"] = json!(config.sign(&editor, false).map_err(map_transport)?);
    let browser_server = browser_document_server(&config.server, &headers)?;
    Ok(Json(json!({"config":editor,"session":session,
        "api_script_url":browser_server.join("web-apps/apps/api/documents/api.js").map_err(|_| validation("invalid server origin"))?.as_str()})))
}

async fn source(
    Path((workspace, key)): Path<(Uuid, Uuid)>,
    Query(ticket): Query<Ticket>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
) -> Result<Response, ApiErr> {
    let config = Config::load().map_err(map_transport)?;
    let capability = read_capability(&config, &ticket.token, "docx-source", workspace, key)?;
    let payload = read_service_payload(&config, &headers)?;
    let requested = config
        .api
        .join(&uri.to_string())
        .map_err(|_| validation("invalid source request URL"))?;
    if payload != json!({"url":requested.as_str()}) {
        return Err(fail(
            StatusCode::FORBIDDEN,
            "ONLYOFFICE_SIGNATURE_SCOPE",
            "document service signature does not match the source URL",
        ));
    }
    let pool = require_bid_pool().await?;
    let session = bidding::docx_round::get_editor(&pool, workspace, key, &capability.actor)
        .await
        .map_err(map_docx_sql)?;
    if identity(&session, "round_id")? != capability.round_id
        || identity(&session["base"], "version_id")? != capability.version_id
    {
        return Err(fail(
            StatusCode::FORBIDDEN,
            "ONLYOFFICE_CAPABILITY_SCOPE",
            "document opening baseline changed",
        ));
    }
    let bytes = read_verified_docx(session["base"].clone()).await?;
    Response::builder()
        .header("content-type", bidding::tender_upload::DOCX_MEDIA_TYPE)
        .header("cache-control", "private, no-store")
        .header("x-content-type-options", "nosniff")
        .body(Body::from(bytes))
        .map_err(|_| validation("document response failed"))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveRequest {
    editor_key: Uuid,
    expected: DocxVersionIdentity,
}

async fn saved_receipt(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace, editor_key, save_id)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<Option<Value>>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    bidding::docx_round::get_saved_receipt(&pool, workspace, editor_key, save_id, &actor)
        .await
        .map(Json)
        .map_err(map_docx_sql)
}

async fn force_save(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workspace): Path<Uuid>,
    BidJson(input): BidJson<SaveRequest>,
) -> Result<Json<Value>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let request_key = required_idempotency_key(&headers)?;
    let config = Config::load().map_err(map_transport)?;
    let pool = require_bid_pool().await?;
    let pending=bidding::docx_round::editor_command(&pool,workspace,"request_save",&json!({"editor_key":input.editor_key,
        "expected_version_id":input.expected.version_id,"expected_docx_sha256":input.expected.docx_sha256}),&actor,&request_key).await.map_err(map_docx_sql)?;
    if pending["dispatch"] != true {
        return Ok(Json(pending));
    }
    let save_id = identity(&pending, "save_id")?;
    let mut command =
        json!({"c":"forcesave","key":input.editor_key.to_string(),"userdata":save_id.to_string()});
    command["token"] = json!(config.sign(&command, false).map_err(map_transport)?);
    let response = config
        .client()
        .map_err(map_transport)?
        .post(
            config
                .command
                .join("coauthoring/CommandService.ashx")
                .map_err(|_| validation("invalid server origin"))?,
        )
        .json(&command)
        .send()
        .await
        .map_err(|_| {
            transport_error(
                "ONLYOFFICE_COMMAND_UNCERTAIN",
                "save command outcome is unknown; pending correlation retained, do not redispatch",
            )
        })?;
    if !response.status().is_success() {
        return Err(transport_error(
            "ONLYOFFICE_COMMAND_UNCERTAIN",
            "save command outcome is unknown; pending correlation retained",
        ));
    }
    let bytes = bounded_body(response, platform::max_file_bytes())
        .await
        .map_err(map_transport)?;
    let result: Value = serde_json::from_slice(&bytes).map_err(|_| {
        transport_error(
            "ONLYOFFICE_COMMAND_UNCERTAIN",
            "invalid save command response; pending correlation retained",
        )
    })?;
    let code = result["error"].as_i64().ok_or_else(|| {
        transport_error(
            "ONLYOFFICE_COMMAND_UNCERTAIN",
            "save command result missing; pending correlation retained",
        )
    })?;
    if code != 0 {
        bidding::docx_round::editor_command(
            &pool,
            workspace,
            "status",
            &json!({"editor_key":input.editor_key,"save_id":save_id,"kind":"command","code":code}),
            &actor,
            &format!("{save_id}:command"),
        )
        .await
        .map_err(map_docx_sql)?;
        return Err(transport_error(
            "ONLYOFFICE_COMMAND_REJECTED",
            "document service rejected the save command; no file version was published",
        ));
    }
    // Command acceptance is never a saved-version receipt.
    Ok(Json(pending))
}

fn read_service_payload(config: &Config, headers: &HeaderMap) -> Result<Value, ApiErr> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| {
            fail(
                StatusCode::UNAUTHORIZED,
                "ONLYOFFICE_SIGNATURE_INVALID",
                "document service signature required",
            )
        })?;
    let mut validation = Validation::new(Algorithm::HS256);
    validation.leeway = 0;
    let signed = decode::<Value>(
        token,
        &DecodingKey::from_secret(config.service_secret().as_bytes()),
        &validation,
    )
    .map_err(|_| {
        fail(
            StatusCode::UNAUTHORIZED,
            "ONLYOFFICE_SIGNATURE_INVALID",
            "document service signature invalid or expired",
        )
    })?
    .claims;
    signed
        .get("payload")
        .filter(|payload| payload.is_object())
        .cloned()
        .ok_or_else(|| {
            fail(
                StatusCode::FORBIDDEN,
                "ONLYOFFICE_SIGNATURE_SCOPE",
                "document service signed payload missing",
            )
        })
}

async fn callback(
    path: Path<(Uuid, Uuid)>,
    ticket: Query<Ticket>,
    headers: HeaderMap,
    body: BidJson<Value>,
) -> Response {
    match callback_inner(path, ticket, headers, body).await {
        Ok(value) => value.into_response(),
        Err((status, Json(body))) => {
            (status, Json(json!({"error": 1, "code": body.error.code}))).into_response()
        }
    }
}

async fn callback_inner(
    Path((workspace, key)): Path<(Uuid, Uuid)>,
    Query(ticket): Query<Ticket>,
    headers: HeaderMap,
    BidJson(body): BidJson<Value>,
) -> Result<Json<Value>, ApiErr> {
    let config = Config::load().map_err(map_transport)?;
    let capability = read_capability(&config, &ticket.token, "docx-callback", workspace, key)?;
    let signed = read_service_payload(&config, &headers)?;
    let mut content = body.clone();
    if let Some(object) = content.as_object_mut() {
        object.remove("token");
    }
    if signed != content || identity(&content, "key")? != key {
        return Err(fail(
            StatusCode::FORBIDDEN,
            "ONLYOFFICE_SIGNATURE_SCOPE",
            "callback content differs from the signed payload",
        ));
    }
    let status = content["status"]
        .as_u64()
        .ok_or_else(|| validation_error("callback status missing"))?;
    let pool = require_bid_pool().await?;
    let actor = &capability.actor;
    let save_id = if matches!(status, 6 | 7) {
        if content["forcesavetype"] != 0 {
            return Err(validation_error(
                "uncorrelated forcesave source is not supported",
            ));
        }
        Some(identity(&content, "userdata")?)
    } else {
        None
    };
    if matches!(status, 1 | 3 | 4 | 7) {
        bidding::docx_round::editor_command(
            &pool,
            workspace,
            "status",
            &json!({"editor_key":key,"save_id":save_id,"kind":"callback","code":status}),
            actor,
            &format!(
                "{key}:{status}:{}",
                save_id.map(|v| v.to_string()).unwrap_or_default()
            ),
        )
        .await
        .map_err(map_docx_sql)?;
        return Ok(Json(json!({"error":0})));
    }
    if !matches!(status, 2 | 6) {
        return Err(validation_error("unsupported callback status"));
    }
    let url = config
        .download_url(
            content["url"]
                .as_str()
                .ok_or_else(|| validation_error("save callback URL missing"))?,
        )
        .map_err(map_transport)?;
    // Hash only the verified semantic payload and capability scope. JWT/token
    // expiry/signature can change on redelivery without changing the save.
    // JCS also makes JSON property order irrelevant; no cache URL is persisted.
    let callback_bytes = platform::jcs_canonical_bytes(&json!({"callback":content,
        "round_id":capability.round_id,"base_version_id":capability.version_id}))
    .map_err(|_| validation_error("callback cannot be canonicalized"))?;
    let replay_input = json!({"editor_key":key,"save_id":save_id,"final":status==2,
        "callback_sha256":platform::sha256_hex(&callback_bytes)});
    let receipt_key = format!(
        "{key}:{}",
        save_id
            .map(|v| v.to_string())
            .unwrap_or_else(|| "final".into())
    );
    if replay_saved_callback(&pool, workspace, &replay_input, actor, &receipt_key).await? {
        return Ok(Json(json!({"error":0})));
    }
    // Refuse stale sessions and uncorrelated saves before downloading bytes. The
    // command repeats these checks under the Workspace lock before publication.
    let session = bidding::docx_round::get_editor(&pool, workspace, key, actor)
        .await
        .map_err(map_docx_sql)?;
    if identity(&session, "round_id")? != capability.round_id
        || identity(&session["base"], "version_id")? != capability.version_id
    {
        return Err(fail(
            StatusCode::FORBIDDEN,
            "ONLYOFFICE_CAPABILITY_SCOPE",
            "callback baseline mismatch",
        ));
    }
    if let Some(save_id) = save_id
        && session["pending_save_id"] != save_id.to_string()
    {
        return Err(fail(
            StatusCode::CONFLICT,
            "DOCX_SAVE_CORRELATION_MISMATCH",
            "save request is no longer pending",
        ));
    }
    let response = config
        .client()
        .map_err(map_transport)?
        .get(url)
        .send()
        .await
        .map_err(|_| {
            transport_error("ONLYOFFICE_READ_FAILED", "document service download failed")
        })?;
    if !response.status().is_success() {
        return Err(transport_error(
            "ONLYOFFICE_READ_FAILED",
            "document service download did not succeed",
        ));
    }
    let bytes = bounded_body(response, platform::max_file_bytes())
        .await
        .map_err(map_transport)?;
    let document = tokio::task::spawn_blocking(move || InitialDocx::new(bytes))
        .await
        .map_err(|_| transport_error("DOCX_VALIDATION_FAILED", "DOCX validation failed"))?
        .map_err(map_upload_validation)?;
    let staging = Uuid::new_v4();
    let mut input = replay_input.clone();
    input["docx_sha256"] = json!(document.sha256());
    input["byte_length"] = json!(document.bytes().len());
    input["staging_id"] = json!(staging);
    // Another callback may have published while this request downloaded.
    if replay_saved_callback(&pool, workspace, &replay_input, actor, &receipt_key).await? {
        return Ok(Json(json!({"error":0})));
    }
    stage_upload(
        &pool,
        staging,
        &document.object_ref(),
        document.sha256(),
        bidding::tender_upload::DOCX_MEDIA_TYPE,
        document.bytes(),
        actor,
    )
    .await?;
    if let Err(error)=read_verified_docx(json!({"object_ref":document.object_ref(),"docx_sha256":document.sha256(),"byte_length":document.bytes().len()})).await {
        schedule_staging_cleanup_required(staging).await?; return Err(error);
    }
    if let Err(error) =
        bidding::docx_round::editor_command(&pool, workspace, "save", &input, actor, &receipt_key)
            .await
    {
        schedule_staging_cleanup_required(staging).await?;
        return Err(map_docx_sql(error));
    }
    Ok(Json(json!({"error":0})))
}

async fn replay_saved_callback(
    pool: &sqlx::PgPool,
    workspace: Uuid,
    input: &Value,
    actor: &str,
    key: &str,
) -> Result<bool, ApiErr> {
    let Some(receipt) = bidding::docx_round::replay_editor_save(pool, workspace, input, actor, key)
        .await
        .map_err(map_docx_sql)?
    else {
        return Ok(false);
    };
    let version = bidding::docx_round::get_docx_version(
        pool,
        workspace,
        identity(&receipt, "version_id")?,
        actor,
    )
    .await
    .map_err(map_docx_sql)?;
    read_verified_docx(version).await?;
    Ok(true)
}

// Keep JWT Validation distinct from API validation errors in the callback path.
fn validation_error(message: &str) -> ApiErr {
    super::validation(message)
}

#[cfg(test)]
mod host_rewrite_tests {
    use super::hostname_from_host;

    #[test]
    fn strips_ipv4_port() {
        assert_eq!(hostname_from_host("192.168.1.10:28080"), "192.168.1.10");
        assert_eq!(hostname_from_host("127.0.0.1"), "127.0.0.1");
        assert_eq!(hostname_from_host("localhost:28080"), "localhost");
    }

    #[test]
    fn strips_ipv6_brackets() {
        assert_eq!(hostname_from_host("[::1]:28080"), "::1");
        assert_eq!(hostname_from_host("[::1]"), "::1");
    }
}
