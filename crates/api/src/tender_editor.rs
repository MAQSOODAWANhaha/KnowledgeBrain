//! Word editing over immutable Python tender drafts. A command acknowledgement
//! is never a saved-version receipt; only a correlated, signed callback publishes.
use crate::AppState;
use crate::err::{fail, forbidden, not_found, unauthorized, validation};
use crate::tender_routes::{ApiErr, bridge, lock_open_project, pg_error, require_project};
use crate::tender_state::{EditorMode, SaveState};
use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Transaction};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const DOCX: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

#[cfg(test)]
#[path = "tender_saved_review_tests.rs"]
mod saved_review_tests;

#[cfg(test)]
#[path = "tender_editor_lifecycle_tests.rs"]
mod lifecycle_tests;

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v3/tender-projects/{project}/docx/current",
            get(current).post(select),
        )
        .route(
            "/api/v3/tender-projects/{project}/docx/versions/{version}",
            get(version),
        )
        .route(
            "/api/v3/tender-projects/{project}/docx/versions/{version}/download",
            get(download),
        )
        .route(
            "/api/v3/tender-projects/{project}/docx/versions/{version}/review",
            get(review_status).post(review_start),
        )
        .route("/api/v3/tender-projects/{project}/docx/editor", post(open))
        .route(
            "/api/v3/tender-projects/{project}/docx/editor/save",
            post(save),
        )
        .route(
            "/api/v3/tender-projects/{project}/docx/editor/{key}/close",
            post(close),
        )
        .route(
            "/api/v3/tender-projects/{project}/docx/editor/{key}/saves/{save}",
            get(saved),
        )
        .route(
            "/api/v3/tender-projects/{project}/docx/editor/{key}/source",
            get(source),
        )
        .route(
            "/api/v3/tender-projects/{project}/docx/editor/{key}/callback",
            post(callback),
        )
}

fn conflict(message: &str) -> ApiErr {
    fail(StatusCode::CONFLICT, "DOCX_VERSION_CONFLICT", message)
}
fn unavailable(message: &str) -> ApiErr {
    fail(
        StatusCode::SERVICE_UNAVAILABLE,
        "ONLYOFFICE_UNAVAILABLE",
        message,
    )
}
fn id(value: &Value, field: &str) -> Result<Uuid, ApiErr> {
    value[field]
        .as_str()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| validation("stored Word identity is invalid"))
}
fn stored_save_state(value: &Value) -> Result<SaveState, ApiErr> {
    serde_json::from_value(value["state"].clone())
        .map_err(|error| pg_error(sqlx::Error::Decode(Box::new(error))))
}
fn request_key(headers: &HeaderMap) -> Result<String, ApiErr> {
    headers
        .get("idempotency-key")
        .and_then(|s| s.to_str().ok())
        .filter(|s| !s.is_empty() && s.len() <= 200)
        .map(str::to_owned)
        .ok_or_else(|| validation("Idempotency-Key is required"))
}
fn digest(value: &Value) -> Result<String, ApiErr> {
    platform::jcs_canonical_bytes(value)
        .map(|bytes| platform::sha256_hex(&bytes))
        .map_err(|_| validation("invalid request"))
}

macro_rules! version_query { ($suffix:literal) => { concat!("SELECT jsonb_build_object(
 'id',v.id,'version_id',v.id,'sha256',v.content_sha256,'docx_sha256',v.content_sha256,
 'project_id',v.project_id,'workspace_id',v.project_id,'round_id',v.round_id,
 'execution_id',v.execution_id,'revision',v.revision,'source',v.source,'created_at',v.created_at,
 'outline',v.outline,'byte_length',v.byte_length,'object_ref',v.object_ref,
 'editor',jsonb_build_object('key',CASE WHEN c.version_id=v.id THEN c.session_id END,
 'base_version_id',s.base_version_id,'pending_save_id',CASE WHEN c.version_id=v.id THEN c.pending_save_id END,
 'save_error',CASE WHEN c.version_id=v.id THEN c.editor_error END))
 FROM tender_draft_versions v LEFT JOIN tender_draft_state c ON c.project_id=v.project_id
 LEFT JOIN tender_editor_sessions s ON s.id=c.session_id", $suffix) }; }

async fn read_version(pool: &PgPool, project: Uuid, version: Uuid) -> Result<Value, ApiErr> {
    sqlx::query_scalar::<_, Value>(version_query!(" WHERE v.project_id=$1 AND v.id=$2"))
        .bind(project)
        .bind(version)
        .fetch_optional(pool)
        .await
        .map_err(pg_error)?
        .ok_or_else(|| not_found("Word version"))
}
async fn read_current(pool: &PgPool, project: Uuid) -> Result<Option<Value>, ApiErr> {
    sqlx::query_scalar::<_, Value>(version_query!(
        " WHERE v.project_id=$1 AND c.version_id=v.id"
    ))
    .bind(project)
    .fetch_optional(pool)
    .await
    .map_err(pg_error)
}
pub(crate) async fn workbench_versions(pool: &PgPool, project: Uuid) -> Result<Value, ApiErr> {
    let versions: Vec<Value> = sqlx::query_scalar(version_query!(
        " WHERE v.project_id=$1 ORDER BY v.revision DESC"
    ))
    .bind(project)
    .fetch_all(pool)
    .await
    .map_err(pg_error)?;
    Ok(json!({"current_docx":read_current(pool,project).await?,"versions":versions}))
}

async fn project_lock(
    tx: &mut Transaction<'_, Postgres>,
    project: Uuid,
    owner: Uuid,
) -> Result<(), ApiErr> {
    let found: Option<Uuid> =
        sqlx::query_scalar("SELECT owner_user_id FROM tender_projects WHERE id=$1 FOR UPDATE")
            .bind(project)
            .fetch_optional(&mut **tx)
            .await
            .map_err(pg_error)?;
    if found != Some(owner) {
        return Err(forbidden());
    }
    Ok(())
}

struct Blob {
    sha: String,
    len: i64,
    staging: Uuid,
}
async fn stage(pool: &PgPool, bytes: &[u8], owner: Uuid) -> Result<Blob, ApiErr> {
    let blob = Blob {
        sha: platform::sha256_hex(bytes),
        len: bytes.len() as i64,
        staging: Uuid::new_v4(),
    };
    platform::stage_object_upload(
        pool,
        blob.staging,
        &platform::object_ref(&blob.sha),
        &blob.sha,
        DOCX,
        blob.len,
        Some(owner),
    )
    .await
    .map_err(pg_error)?;
    if platform::write_blob_async(&blob.sha, bytes).await.is_err() {
        expire(pool, &blob, owner).await;
        return Err(unavailable("Word object storage failed"));
    }
    Ok(blob)
}
async fn expire(pool: &PgPool, blob: &Blob, owner: Uuid) {
    let _ = platform::request_object_upload_expiry(pool, blob.staging, Some(owner)).await;
    let _ = platform::schedule_object_upload_cleanup(blob.staging).await;
}
async fn publish_blob(
    tx: &mut Transaction<'_, Postgres>,
    blob: &Blob,
    version: Uuid,
    owner: Uuid,
) -> Result<(), ApiErr> {
    platform::publish_object_reference(
        tx,
        Some(blob.staging),
        &platform::object_ref(&blob.sha),
        &blob.sha,
        DOCX,
        blob.len,
        "tender_web_docx_version",
        version,
        "payload",
        Some(owner),
    )
    .await
    .map_err(pg_error)?;
    Ok(())
}

pub(crate) async fn import_generated(
    pool: &PgPool,
    project: Uuid,
    execution: &str,
    bytes: &[u8],
    owner: Uuid,
) -> Result<Value, ApiErr> {
    let existing: Option<Uuid> = sqlx::query_scalar("SELECT id FROM tender_draft_versions WHERE project_id=$1 AND execution_id=$2 AND source='generated'")
        .bind(project).bind(execution).fetch_optional(pool).await.map_err(pg_error)?;
    if let Some(existing) = existing {
        let previous = read_version(pool, project, existing).await?;
        if previous["docx_sha256"] != platform::sha256_hex(bytes) {
            return Err(conflict(
                "the generated execution already has a different Word artifact",
            ));
        }
        return Ok(previous);
    }
    let outline = bridge(
        project,
        owner,
        "docx.outline",
        json!({"execution_id":execution}),
        bytes.to_vec(),
    )
    .await?
    .0;
    let blob = stage(pool, bytes, owner).await?;
    let result = async {
        let mut tx = pool.begin().await.map_err(pg_error)?;
        project_lock(&mut tx, project, owner).await?;
        let duplicate: Option<Uuid> = sqlx::query_scalar("SELECT id FROM tender_draft_versions WHERE project_id=$1 AND execution_id=$2 AND source='generated'")
            .bind(project).bind(execution).fetch_optional(&mut *tx).await.map_err(pg_error)?;
        if let Some(duplicate) = duplicate { tx.rollback().await.map_err(pg_error)?; return Ok((duplicate,false)); }
        let revision: i64 = sqlx::query_scalar("SELECT COALESCE(max(revision),0)+1 FROM tender_draft_versions WHERE project_id=$1")
            .bind(project).fetch_one(&mut *tx).await.map_err(pg_error)?;
        let version = Uuid::new_v4();
        publish_blob(&mut tx, &blob, version, owner).await?;
        sqlx::query("INSERT INTO tender_draft_versions(id,project_id,execution_id,round_id,revision,source,object_ref,content_sha256,byte_length,outline,created_by_user_id) VALUES($1,$2,$3,$4,$5,'generated',$6,$7::kb_sha256,$8,$9,$10)")
            .bind(version).bind(project).bind(execution).bind(Uuid::new_v4()).bind(revision).bind(platform::object_ref(&blob.sha))
            .bind(&blob.sha).bind(blob.len).bind(&outline).bind(owner).execute(&mut *tx).await.map_err(pg_error)?;
        sqlx::query("INSERT INTO tender_draft_state(project_id,version_id) VALUES($1,$2) ON CONFLICT(project_id) DO NOTHING")
            .bind(project).bind(version).execute(&mut *tx).await.map_err(pg_error)?;
        tx.commit().await.map_err(pg_error)?;
        Ok((version,true))
    }.await;
    match result {
        Ok((version, published)) => {
            if !published {
                expire(pool, &blob, owner).await;
            }
            read_version(pool, project, version).await
        }
        Err(error) => {
            expire(pool, &blob, owner).await;
            Err(error)
        }
    }
}

async fn current(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project): Path<Uuid>,
) -> Result<Json<Option<Value>>, ApiErr> {
    let (pool, _) = require_project(&headers, &state, project).await?;
    read_current(&pool, project).await.map(Json)
}
async fn version(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project, version)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiErr> {
    let (pool, _) = require_project(&headers, &state, project).await?;
    read_version(&pool, project, version).await.map(Json)
}
async fn bytes_for(value: &Value) -> Result<Vec<u8>, ApiErr> {
    let sha = value["docx_sha256"]
        .as_str()
        .ok_or_else(|| validation("Word hash missing"))?
        .to_owned();
    let bytes = tokio::task::spawn_blocking(move || platform::read_blob(&sha))
        .await
        .map_err(|_| unavailable("Word read failed"))?
        .map_err(|_| unavailable("Word object is unavailable"))?;
    if platform::sha256_hex(&bytes) != value["docx_sha256"]
        || bytes.len() as i64 != value["byte_length"].as_i64().unwrap_or(-1)
    {
        return Err(unavailable("Word object integrity mismatch"));
    }
    Ok(bytes)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedReviewRequest {
    docx_sha256: String,
    reason: String,
}

async fn review_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project, version)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiErr> {
    let (pool, owner) = require_project(&headers, &state, project).await?;
    read_version(&pool, project, version).await?;
    Ok(Json(
        bridge(
            project,
            owner,
            "docx.review.status",
            json!({"version_id":version}),
            vec![],
        )
        .await?
        .0,
    ))
}

async fn review_start(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project, version)): Path<(Uuid, Uuid)>,
    Json(body): Json<SavedReviewRequest>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    let (pool, owner) = require_project(&headers, &state, project).await?;
    let key = request_key(&headers)?;
    let saved = read_version(&pool, project, version).await?;
    if saved["docx_sha256"].as_str() != Some(body.docx_sha256.as_str()) {
        return Err(conflict("请重新读取明确的已保存 Word 版本后复核。"));
    }
    if body.reason.trim().is_empty() || body.reason.chars().count() > 4000 {
        return Err(validation("请填写不超过 4000 字的保存稿复核说明。"));
    }
    let request = json!({"key":key,"project_id":project});
    let execution = format!(
        "web-saved-review-{}",
        platform::sha256_hex(
            &serde_json::to_vec(&request).map_err(|_| validation("invalid review request"))?
        )
    );
    let existing: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tender_authoring_jobs WHERE project_id=$1 AND execution_id=$2)")
        .bind(project).bind(execution).fetch_one(&pool).await.map_err(pg_error)?;
    let bytes = if existing {
        vec![]
    } else {
        let mut tx = pool.begin().await.map_err(pg_error)?;
        lock_open_project(&mut tx, project, owner).await?;
        tx.commit().await.map_err(pg_error)?;
        bytes_for(&saved).await?
    };
    let result = bridge(
        project,
        owner,
        "docx.review.start",
        json!({"version_id":version,
        "docx_sha256":body.docx_sha256,"reason":body.reason,"request_key":key}),
        bytes,
    )
    .await?
    .0;
    Ok((StatusCode::ACCEPTED, Json(result)))
}

async fn download(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project, version)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiErr> {
    let (pool, _) = require_project(&headers, &state, project).await?;
    word_response(bytes_for(&read_version(&pool, project, version).await?).await?)
}
fn word_response(bytes: Vec<u8>) -> Result<Response, ApiErr> {
    Response::builder()
        .header("content-type", DOCX)
        .header("cache-control", "private, no-store")
        .header("x-content-type-options", "nosniff")
        .body(Body::from(bytes))
        .map_err(|_| unavailable("Word response failed"))
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
struct Identity {
    version_id: Uuid,
    docx_sha256: String,
}
async fn select(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project): Path<Uuid>,
    Json(input): Json<Identity>,
) -> Result<Json<Value>, ApiErr> {
    let (pool, owner) = require_project(&headers, &state, project).await?;
    let selected = read_version(&pool, project, input.version_id).await?;
    if selected["docx_sha256"] != input.docx_sha256 {
        return Err(conflict("selected Word identity changed"));
    }
    let mut tx = pool.begin().await.map_err(pg_error)?;
    lock_open_project(&mut tx, project, owner).await?;
    let active: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tender_draft_state WHERE project_id=$1 AND (session_id IS NOT NULL OR pending_save_id IS NOT NULL))")
        .bind(project).fetch_one(&mut *tx).await.map_err(pg_error)?;
    if active {
        return Err(conflict(
            "save and close the current editor before selecting another draft",
        ));
    }
    sqlx::query(
        "UPDATE tender_draft_state SET version_id=$2,editor_error=NULL WHERE project_id=$1",
    )
    .bind(project)
    .bind(input.version_id)
    .execute(&mut *tx)
    .await
    .map_err(pg_error)?;
    tx.commit().await.map_err(pg_error)?;
    read_version(&pool, project, input.version_id)
        .await
        .map(Json)
}

struct Office {
    server: Url,
    command: Url,
    api: Url,
    secret: String,
    capability: String,
    ttl: u64,
    timeout: u64,
}
impl Office {
    fn load() -> Result<Self, ApiErr> {
        let origin = |name| -> Result<Url, ApiErr> {
            let url = Url::parse(
                &std::env::var(name)
                    .map_err(|_| unavailable("ONLYOFFICE origins must be configured"))?,
            )
            .map_err(|_| unavailable("ONLYOFFICE origin is invalid"))?;
            if !matches!(url.scheme(), "http" | "https")
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err(unavailable("ONLYOFFICE origin is invalid"));
            }
            Ok(url)
        };
        let number = |name: &str, default: u64| -> Result<u64, ApiErr> {
            std::env::var(name)
                .unwrap_or_else(|_| default.to_string())
                .parse::<u64>()
                .ok()
                .filter(|v| *v > 0)
                .ok_or_else(|| unavailable("ONLYOFFICE timeout is invalid"))
        };
        Ok(Self {
            server: origin("KB_ONLYOFFICE_SERVER_ORIGIN")?,
            command: origin("KB_ONLYOFFICE_COMMAND_ORIGIN")?,
            api: origin("KB_ONLYOFFICE_API_ORIGIN")?,
            secret: platform::required_secret("KB_ONLYOFFICE_JWT_SECRET")
                .map_err(|_| unavailable("ONLYOFFICE signing secret is required"))?,
            capability: platform::required_secret("KB_ONLYOFFICE_CAPABILITY_SECRET")
                .map_err(|_| unavailable("ONLYOFFICE capability secret is required"))?,
            ttl: number("KB_ONLYOFFICE_TOKEN_TTL_SECONDS", 1200u64)?,
            timeout: number("KB_ONLYOFFICE_HTTP_TIMEOUT_SECONDS", 120u64)?,
        })
    }
    fn sign(&self, value: &impl Serialize, capability: bool) -> Result<String, ApiErr> {
        encode(
            &Header::new(Algorithm::HS256),
            value,
            &EncodingKey::from_secret(if capability {
                self.capability.as_bytes()
            } else {
                self.secret.as_bytes()
            }),
        )
        .map_err(|_| unavailable("Word configuration signing failed"))
    }
    fn client(&self) -> Result<reqwest::Client, ApiErr> {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(self.timeout))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| unavailable("Word transport unavailable"))
    }
    fn download_url(&self, raw: &str) -> Result<Url, ApiErr> {
        let mut url =
            Url::parse(raw).map_err(|_| validation("callback download URL is invalid"))?;
        if !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || !matches!(url.origin(),o if o==self.command.origin() || o==self.server.origin())
        {
            return Err(forbidden());
        }
        // Server cache links can use its browser origin. Fetch that same path
        // through the configured service origin, without changing network policy.
        url.set_scheme(self.command.scheme())
            .map_err(|_| forbidden())?;
        url.set_host(self.command.host_str())
            .map_err(|_| forbidden())?;
        url.set_port(self.command.port()).map_err(|_| forbidden())?;
        Ok(url)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Capability {
    aud: String,
    project: Uuid,
    key: Uuid,
    version: Uuid,
    owner: Uuid,
    mode: EditorMode,
}
#[derive(Deserialize)]
struct Ticket {
    token: String,
}
fn capability(
    office: &Office,
    token: &str,
    purpose: &str,
    project: Uuid,
    key: Uuid,
) -> Result<Capability, ApiErr> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_audience(&[purpose]);
    validation.required_spec_claims.remove("exp");
    // Office can retain a document for a long edit. These session capabilities
    // are revoked by verify_session's closed-at and current project-owner checks.
    validation.validate_exp = false;
    let claims = decode::<Capability>(
        token,
        &DecodingKey::from_secret(office.capability.as_bytes()),
        &validation,
    )
    .map_err(|_| unauthorized())?
    .claims;
    if claims.project != project || claims.key != key {
        return Err(forbidden());
    }
    Ok(claims)
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Open {
    version_id: Uuid,
    docx_sha256: String,
    mode: EditorMode,
    language: Option<String>,
}
fn editor_config(
    office: &Office,
    cap: &Capability,
    source: &str,
    callback: Option<&str>,
    language: Option<&str>,
    navigation_origin: Option<&str>,
) -> Result<Value, ApiErr> {
    let (project, key, mode, owner) = (cap.project, cap.key, cap.mode, cap.owner);
    let edit = mode == EditorMode::Edit;
    let mut config = json!({"documentType":"word","document":{"fileType":"docx","key":key.to_string(),"title":format!("投标文件-{project}.docx"),"url":source,
        "permissions":{"edit":edit,"review":edit,"fillForms":edit,"comment":edit,"chat":false,"download":true}},
        "editorConfig":{"mode":mode,"user":{"id":owner.to_string(),"name":"项目编制"},"customization":{"forcesave":false}},
        "exp":SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_|unavailable("clock unavailable"))?.as_secs()+office.ttl});
    if let Some(callback) = callback {
        config["editorConfig"]["callbackUrl"] = json!(callback);
    }
    if let Some(language) = language {
        if language.len() > 40
            || !language
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(validation("invalid editor language"));
        }
        config["editorConfig"]["lang"] = json!(language);
    }
    if let Some(origin) = navigation_origin {
        config["editorConfig"]["plugins"] = json!({"autostart":["asc.{0A2B83C0-6CD7-4D77-A5DE-17372E05C1B8}"],
            "pluginsData":[format!("{origin}/onlyoffice-navigation/config.json")]});
    }
    config["token"] = json!(office.sign(&config, false)?);
    Ok(config)
}

fn navigation_origin(headers: &HeaderMap) -> Option<String> {
    let origin = headers.get("origin").and_then(|value| value.to_str().ok());
    let raw = origin.or_else(|| headers.get("referer").and_then(|value| value.to_str().ok()))?;
    let url = Url::parse(raw).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.host_str().is_none()
    {
        return None;
    }
    if origin.is_some() && (url.path() != "/" || url.query().is_some() || url.fragment().is_some())
    {
        return None;
    }
    Some(url.origin().ascii_serialization())
}

async fn open(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project): Path<Uuid>,
    Json(input): Json<Open>,
) -> Result<Json<Value>, ApiErr> {
    let (pool, owner) = require_project(&headers, &state, project).await?;
    let request = request_key(&headers)?;
    let request_sha = digest(&json!(input))?;
    if input.mode == EditorMode::Edit {
        let admitted: bool = sqlx::query_scalar("SELECT p.status='open' OR EXISTS(SELECT 1 FROM tender_editor_sessions s WHERE s.project_id=p.id AND s.owner_user_id=$2 AND s.request_key=$3 AND s.request_sha256=$4::kb_sha256 AND s.closed_at IS NULL) FROM tender_projects p WHERE p.id=$1 AND p.owner_user_id=$2")
            .bind(project).bind(owner).bind(&request).bind(&request_sha).fetch_one(&pool).await.map_err(pg_error)?;
        if !admitted {
            return Err(fail(
                StatusCode::CONFLICT,
                "PROJECT_ENDED",
                "已结束的项目不可新建Word编辑会话",
            ));
        }
    }
    let office = Office::load()?;
    let version = read_version(&pool, project, input.version_id).await?;
    if version["docx_sha256"] != input.docx_sha256 {
        return Err(conflict("Word version hash mismatch"));
    }
    bytes_for(&version).await?;
    let mut tx = pool.begin().await.map_err(pg_error)?;
    project_lock(&mut tx, project, owner).await?;
    let previous:Option<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'request_sha',request_sha256,'closed',closed_at IS NOT NULL) FROM tender_editor_sessions WHERE project_id=$1 AND request_key=$2")
        .bind(project).bind(&request).fetch_optional(&mut *tx).await.map_err(pg_error)?;
    let key = if let Some(previous) = previous {
        if previous["request_sha"] != request_sha || previous["closed"] == true {
            return Err(conflict("editor opening request is no longer applicable"));
        }
        id(&previous, "id")?
    } else {
        if input.mode == EditorMode::Edit {
            lock_open_project(&mut tx, project, owner).await?;
            let head:Value=sqlx::query_scalar("SELECT jsonb_build_object('version_id',version_id,'editor_key',session_id,'pending_save_id',pending_save_id) FROM tender_draft_state WHERE project_id=$1 FOR UPDATE")
                .bind(project).fetch_one(&mut *tx).await.map_err(pg_error)?;
            if head["version_id"] != input.version_id.to_string()
                || !head["pending_save_id"].is_null()
            {
                return Err(conflict(
                    "select this draft and finish any pending save before editing",
                ));
            }
            if !head["editor_key"].is_null() {
                return Err(conflict(
                    "another editing session is active; close it before opening a new editor",
                ));
            }
        }
        let key = Uuid::new_v4();
        sqlx::query("INSERT INTO tender_editor_sessions(id,project_id,base_version_id,owner_user_id,mode,request_key,request_sha256) VALUES($1,$2,$3,$4,$5,$6,$7::kb_sha256)")
            .bind(key).bind(project).bind(input.version_id).bind(owner).bind(input.mode).bind(request).bind(request_sha).execute(&mut *tx).await.map_err(pg_error)?;
        if input.mode == EditorMode::Edit {
            sqlx::query(
                "UPDATE tender_draft_state SET session_id=$2,editor_error=NULL WHERE project_id=$1",
            )
            .bind(project)
            .bind(key)
            .execute(&mut *tx)
            .await
            .map_err(pg_error)?;
        }
        key
    };
    tx.commit().await.map_err(pg_error)?;
    let mut cap = Capability {
        aud: "tender-word-source".into(),
        project,
        key,
        version: input.version_id,
        owner,
        mode: input.mode,
    };
    let prefix = format!("api/v3/tender-projects/{project}/docx/editor/{key}");
    let mut source = office
        .api
        .join(&format!("{prefix}/source"))
        .map_err(|_| unavailable("invalid API origin"))?;
    source
        .query_pairs_mut()
        .append_pair("token", &office.sign(&cap, true)?);
    cap.aud = "tender-word-callback".into();
    let mut callback = office
        .api
        .join(&format!("{prefix}/callback"))
        .map_err(|_| unavailable("invalid API origin"))?;
    callback
        .query_pairs_mut()
        .append_pair("token", &office.sign(&cap, true)?);
    let config = editor_config(
        &office,
        &cap,
        source.as_str(),
        if input.mode == EditorMode::Edit {
            Some(callback.as_str())
        } else {
            None
        },
        input.language.as_deref(),
        navigation_origin(&headers).as_deref(),
    )?;
    Ok(Json(
        json!({"config":config,"api_script_url":office.server.join("web-apps/apps/api/documents/api.js").map_err(|_|unavailable("invalid server origin"))?.as_str(),
        "session":{"editor_key":key,"round_id":version["round_id"],"version_id":input.version_id}}),
    ))
}

async fn verify_session(pool: &PgPool, cap: &Capability) -> Result<(), ApiErr> {
    let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tender_editor_sessions s JOIN tender_projects p ON p.id=s.project_id WHERE s.id=$1 AND s.project_id=$2 AND s.base_version_id=$3 AND s.owner_user_id=$4 AND p.owner_user_id=$4 AND s.mode=$5 AND s.closed_at IS NULL)")
        .bind(cap.key).bind(cap.project).bind(cap.version).bind(cap.owner).bind(cap.mode).fetch_one(pool).await.map_err(pg_error)?;
    if !valid {
        return Err(forbidden());
    }
    Ok(())
}
async fn source(
    Path((project, key)): Path<(Uuid, Uuid)>,
    Query(ticket): Query<Ticket>,
) -> Result<Response, ApiErr> {
    let office = Office::load()?;
    let cap = capability(&office, &ticket.token, "tender-word-source", project, key)?;
    let pool = platform::connect().await.map_err(pg_error)?;
    verify_session(&pool, &cap).await?;
    word_response(bytes_for(&read_version(&pool, project, cap.version).await?).await?)
}
async fn close(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project, key)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiErr> {
    let (pool, owner) = require_project(&headers, &state, project).await?;
    let mut tx = pool.begin().await.map_err(pg_error)?;
    project_lock(&mut tx, project, owner).await?;
    let pending:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tender_draft_state WHERE project_id=$1 AND session_id=$2 AND pending_save_id IS NOT NULL)")
        .bind(project).bind(key).fetch_one(&mut *tx).await.map_err(pg_error)?;
    if pending {
        return Err(conflict("the save result is still pending"));
    }
    sqlx::query("UPDATE tender_draft_state SET session_id=NULL,editor_error=NULL WHERE project_id=$1 AND session_id=$2")
        .bind(project).bind(key).execute(&mut *tx).await.map_err(pg_error)?;
    sqlx::query("UPDATE tender_editor_sessions SET closed_at=COALESCE(closed_at,now()) WHERE project_id=$1 AND id=$2 AND owner_user_id=$3")
        .bind(project).bind(key).bind(owner).execute(&mut *tx).await.map_err(pg_error)?;
    tx.commit().await.map_err(pg_error)?;
    Ok(Json(json!({"closed":true})))
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Save {
    editor_key: Uuid,
    expected: Identity,
}
async fn save(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project): Path<Uuid>,
    Json(input): Json<Save>,
) -> Result<Json<Value>, ApiErr> {
    let (pool, owner) = require_project(&headers, &state, project).await?;
    let request = request_key(&headers)?;
    let office = Office::load()?;
    let plan = prepare_save(&pool, project, owner, &request, &input).await?;
    let save = plan.id;
    if !plan.dispatch {
        return Ok(Json(
            json!({"save_id":save,"pending":false,"dispatch":false}),
        ));
    }
    let mut command =
        json!({"c":"forcesave","key":input.editor_key.to_string(),"userdata":save.to_string()});
    command["token"] = json!(office.sign(&command, false)?);
    let response = office
        .client()?
        .post(
            office
                .command
                .join("coauthoring/CommandService.ashx")
                .map_err(|_| unavailable("invalid Word service origin"))?,
        )
        .json(&command)
        .send()
        .await
        .map_err(|_| {
            fail(
                StatusCode::SERVICE_UNAVAILABLE,
                "ONLYOFFICE_COMMAND_UNCERTAIN",
                "save outcome unknown; the same request must be confirmed",
            )
        })?;
    if !response.status().is_success() {
        return Err(unavailable(
            "save command outcome is unknown; pending request retained",
        ));
    }
    let body: Value = serde_json::from_slice(&bounded(response).await?)
        .map_err(|_| unavailable("save command result is unknown"))?;
    let error = body["error"]
        .as_i64()
        .ok_or_else(|| unavailable("save command result is unknown"))?;
    if error != 0 {
        if !plan.replay {
            record_failure(&pool, project, input.editor_key, save, "command", error).await?;
        }
        let published: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tender_draft_saves WHERE id=$1 AND project_id=$2 AND session_id=$3 AND state='saved')")
            .bind(save).bind(project).bind(input.editor_key).fetch_one(&pool).await.map_err(pg_error)?;
        if published {
            return Ok(Json(
                json!({"save_id":save,"pending":false,"dispatch":true}),
            ));
        }
        if plan.replay {
            // The first command may still be completing. A retry's "no changes"
            // response cannot invalidate its original, correlated callback.
            return Err(fail(
                StatusCode::SERVICE_UNAVAILABLE,
                "ONLYOFFICE_COMMAND_UNCERTAIN",
                "the original save is still awaiting its signed callback; confirm the same request",
            ));
        }
        return Err(fail(
            StatusCode::SERVICE_UNAVAILABLE,
            "ONLYOFFICE_COMMAND_REJECTED",
            "the Word service rejected this save",
        ));
    }
    Ok(Json(json!({"save_id":save,"pending":true,"dispatch":true})))
}
struct SavePlan {
    id: Uuid,
    dispatch: bool,
    replay: bool,
}
async fn prepare_save(
    pool: &PgPool,
    project: Uuid,
    owner: Uuid,
    request: &str,
    input: &Save,
) -> Result<SavePlan, ApiErr> {
    let request_sha = digest(&json!(input))?;
    let mut tx = pool.begin().await.map_err(pg_error)?;
    project_lock(&mut tx, project, owner).await?;
    let previous:Option<Value>=sqlx::query_scalar("SELECT jsonb_build_object('save_id',id,'request_sha',request_sha256,'state',state) FROM tender_draft_saves WHERE project_id=$1 AND request_key=$2")
        .bind(project).bind(request).fetch_optional(&mut *tx).await.map_err(pg_error)?;
    let repeated = if let Some(previous) = previous {
        if previous["request_sha"] != request_sha {
            return Err(conflict("save request identity changed"));
        }
        let id = id(&previous, "save_id")?;
        let previous_state = stored_save_state(&previous)?;
        if previous_state == SaveState::Saved {
            return Ok(SavePlan {
                id,
                dispatch: false,
                replay: true,
            });
        }
        if previous_state != SaveState::Pending {
            return Err(conflict(
                "this save failed; submit a new explicit save request",
            ));
        }
        Some(id)
    } else {
        lock_open_project(&mut tx, project, owner).await?;
        None
    };
    let head:Value=sqlx::query_scalar("SELECT jsonb_build_object('version_id',c.version_id,'sha',v.content_sha256,'editor_key',c.session_id,'pending_save_id',c.pending_save_id) FROM tender_draft_state c JOIN tender_draft_versions v ON v.id=c.version_id WHERE c.project_id=$1 FOR UPDATE OF c")
        .bind(project).fetch_one(&mut *tx).await.map_err(pg_error)?;
    if head["version_id"] != input.expected.version_id.to_string()
        || head["sha"] != input.expected.docx_sha256
        || head["editor_key"] != input.editor_key.to_string()
        || repeated.map_or_else(
            || !head["pending_save_id"].is_null(),
            |save| head["pending_save_id"] != save.to_string(),
        )
    {
        return Err(conflict(
            "Word save baseline changed or another save is pending",
        ));
    }
    if let Some(id) = repeated {
        tx.commit().await.map_err(pg_error)?;
        return Ok(SavePlan {
            id,
            dispatch: true,
            replay: true,
        });
    }
    let save = Uuid::new_v4();
    sqlx::query("INSERT INTO tender_draft_saves(id,project_id,session_id,parent_version_id,parent_sha256,request_key,request_sha256,state) VALUES($1,$2,$3,$4,$5::kb_sha256,$6,$7::kb_sha256,$8)")
        .bind(save).bind(project).bind(input.editor_key).bind(input.expected.version_id).bind(&input.expected.docx_sha256).bind(request).bind(request_sha).bind(SaveState::Pending).execute(&mut *tx).await.map_err(pg_error)?;
    sqlx::query(
        "UPDATE tender_draft_state SET pending_save_id=$2,editor_error=NULL WHERE project_id=$1",
    )
    .bind(project)
    .bind(save)
    .execute(&mut *tx)
    .await
    .map_err(pg_error)?;
    tx.commit().await.map_err(pg_error)?;
    Ok(SavePlan {
        id: save,
        dispatch: true,
        replay: false,
    })
}
async fn saved(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project, key, save)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<Option<Value>>, ApiErr> {
    let (pool, _) = require_project(&headers, &state, project).await?;
    let receipt:Option<Value>=sqlx::query_scalar("SELECT jsonb_build_object('save_id',s.id,'editor_key',s.session_id,'parent_version_id',s.parent_version_id,'round_id',v.round_id,'version_id',v.id,'docx_sha256',v.content_sha256,'revision',v.revision) FROM tender_draft_saves s JOIN tender_draft_versions v ON v.id=s.result_version_id WHERE s.project_id=$1 AND s.session_id=$2 AND s.id=$3 AND s.state='saved'")
        .bind(project).bind(key).bind(save).fetch_optional(&pool).await.map_err(pg_error)?;
    Ok(Json(receipt))
}
async fn record_failure(
    pool: &PgPool,
    project: Uuid,
    key: Uuid,
    save: Uuid,
    kind: &str,
    code: i64,
) -> Result<(), ApiErr> {
    let mut tx = pool.begin().await.map_err(pg_error)?;
    sqlx::query("UPDATE tender_draft_saves SET state=$5,error_code=$4 WHERE project_id=$1 AND session_id=$2 AND id=$3 AND state='pending'")
        .bind(project).bind(key).bind(save).bind(code).bind(SaveState::Failed).execute(&mut *tx).await.map_err(pg_error)?;
    sqlx::query("UPDATE tender_draft_state SET pending_save_id=NULL,editor_error=$4 WHERE project_id=$1 AND session_id=$2 AND pending_save_id=$3")
        .bind(project).bind(key).bind(save).bind(json!({"kind":kind,"code":code})).execute(&mut *tx).await.map_err(pg_error)?;
    tx.commit().await.map_err(pg_error)?;
    Ok(())
}

async fn bounded(mut response: reqwest::Response) -> Result<Vec<u8>, ApiErr> {
    let limit = platform::max_file_bytes();
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| unavailable("Word service response failed"))?
    {
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err(validation("Word service response exceeds file limit"));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn service_payload(office: &Office, headers: &HeaderMap, body: &Value) -> Result<Value, ApiErr> {
    let token = body["token"]
        .as_str()
        .or_else(|| {
            headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
        })
        .ok_or_else(unauthorized)?;
    let mut validation = Validation::new(Algorithm::HS256);
    validation.required_spec_claims.remove("exp");
    validation.validate_aud = false;
    let mut signed = decode::<Value>(
        token,
        &DecodingKey::from_secret(office.secret.as_bytes()),
        &validation,
    )
    .map_err(|_| unauthorized())?
    .claims;
    if let Some(payload) = signed.get("payload") {
        return Ok(payload.clone());
    }
    // DocumentServer 8.3 signs direct callback fields together with JWT timing
    // claims, while omitting those claims from the callback's business body.
    if let Some(claims) = signed.as_object_mut() {
        claims.remove("iat");
        claims.remove("exp");
    }
    Ok(signed)
}
async fn callback(
    Path((project, key)): Path<(Uuid, Uuid)>,
    Query(ticket): Query<Ticket>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    match callback_inner(project, key, &ticket.token, &headers, body).await {
        Ok(()) => Json(json!({"error":0})).into_response(),
        Err((status, Json(error))) => {
            (status, Json(json!({"error":1,"code":error.error.code}))).into_response()
        }
    }
}
async fn callback_inner(
    project: Uuid,
    key: Uuid,
    token: &str,
    headers: &HeaderMap,
    mut body: Value,
) -> Result<(), ApiErr> {
    let office = Office::load()?;
    let cap = capability(&office, token, "tender-word-callback", project, key)?;
    if cap.mode != EditorMode::Edit {
        return Err(forbidden());
    }
    let signed = service_payload(&office, headers, &body)?;
    if let Some(object) = body.as_object_mut() {
        object.remove("token");
    }
    if signed != body || id(&body, "key")? != key {
        return Err(forbidden());
    }
    let status = body["status"]
        .as_i64()
        .ok_or_else(|| validation("Word callback status missing"))?;
    let pool = platform::connect().await.map_err(pg_error)?;
    if status == 1 {
        verify_session(&pool, &cap).await?;
        return Ok(());
    }
    if status == 4 {
        verify_session(&pool, &cap).await?;
        let mut tx = pool.begin().await.map_err(pg_error)?;
        project_lock(&mut tx, project, cap.owner).await?;
        sqlx::query("UPDATE tender_draft_state SET session_id=NULL,editor_error=NULL WHERE project_id=$1 AND session_id=$2 AND pending_save_id IS NULL")
            .bind(project).bind(key).execute(&mut *tx).await.map_err(pg_error)?;
        sqlx::query("UPDATE tender_editor_sessions SET closed_at=now() WHERE id=$1 AND NOT EXISTS(SELECT 1 FROM tender_draft_state WHERE session_id=$1)")
            .bind(key).execute(&mut *tx).await.map_err(pg_error)?;
        tx.commit().await.map_err(pg_error)?;
        return Ok(());
    }
    if status == 2 {
        return recover_final(&pool, &office, &cap, &body).await;
    }
    // Only an explicit forcesave can acknowledge this application's Save action.
    if !matches!(status, 6 | 7) || body["forcesavetype"] != 0 {
        return Err(validation("an explicit correlated Word save is required"));
    }
    let save = id(&body, "userdata")?;
    let callback_sha = digest(&json!({"body":body,"version":cap.version,"key":key}))?;
    let previous:Option<Value>=sqlx::query_scalar("SELECT jsonb_build_object('state',state,'callback_sha',callback_sha256) FROM tender_draft_saves WHERE id=$1 AND project_id=$2 AND session_id=$3")
        .bind(save).bind(project).bind(key).fetch_optional(&pool).await.map_err(pg_error)?;
    let previous_state = previous.as_ref().map(stored_save_state).transpose()?;
    if let Some(previous) = &previous
        && previous_state == Some(SaveState::Saved)
    {
        return if previous["callback_sha"] == callback_sha {
            Ok(())
        } else {
            Err(conflict("callback replay content changed"))
        };
    }
    verify_session(&pool, &cap).await?;
    if previous_state != Some(SaveState::Pending) {
        return Err(conflict("Word save is not pending"));
    }
    if status == 7 {
        return record_failure(&pool, project, key, save, "callback", 7).await;
    }
    let url = office.download_url(
        body["url"]
            .as_str()
            .ok_or_else(|| validation("Word callback URL missing"))?,
    )?;
    let response = office
        .client()?
        .get(url)
        .send()
        .await
        .map_err(|_| unavailable("Word saved file download failed"))?;
    if !response.status().is_success() {
        return Err(unavailable("Word saved file download failed"));
    }
    let bytes = bounded(response).await?;
    let origin = read_version(&pool, project, cap.version).await?;
    let outline = bridge(
        project,
        cap.owner,
        "docx.outline",
        json!({"execution_id":origin["execution_id"]}),
        bytes.clone(),
    )
    .await?
    .0;
    let blob = stage(&pool, &bytes, cap.owner).await?;
    let result = publish_save(&pool, save, &cap, &callback_sha, &outline, &blob).await;
    if result.is_err() {
        expire(&pool, &blob, cap.owner).await;
    }
    result
}

async fn recover_final(
    pool: &PgPool,
    office: &Office,
    cap: &Capability,
    body: &Value,
) -> Result<(), ApiErr> {
    let callback_sha = digest(&json!({"body":body,"version":cap.version,"key":cap.key}))?;
    let previous: Option<String> = sqlx::query_scalar(
        "SELECT callback_sha256::text FROM tender_draft_recoveries WHERE session_id=$1",
    )
    .bind(cap.key)
    .fetch_optional(pool)
    .await
    .map_err(pg_error)?;
    if let Some(previous) = previous {
        return if previous == callback_sha {
            Ok(())
        } else {
            Err(conflict("final callback replay content changed"))
        };
    }
    verify_session(pool, cap).await?;
    let url = office.download_url(
        body["url"]
            .as_str()
            .ok_or_else(|| validation("final Word callback URL missing"))?,
    )?;
    let response = office
        .client()?
        .get(url)
        .send()
        .await
        .map_err(|_| unavailable("final Word download failed"))?;
    if !response.status().is_success() {
        return Err(unavailable("final Word download failed"));
    }
    let bytes = bounded(response).await?;
    let origin = read_version(pool, cap.project, cap.version).await?;
    let outline = bridge(
        cap.project,
        cap.owner,
        "docx.outline",
        json!({"execution_id":origin["execution_id"]}),
        bytes.clone(),
    )
    .await?
    .0;
    let blob = stage(pool, &bytes, cap.owner).await?;
    let result=async{
        let mut tx=pool.begin().await.map_err(pg_error)?;project_lock(&mut tx,cap.project,cap.owner).await?;
        let previous:Option<String>=sqlx::query_scalar("SELECT callback_sha256::text FROM tender_draft_recoveries WHERE session_id=$1")
            .bind(cap.key).fetch_optional(&mut *tx).await.map_err(pg_error)?;
        if let Some(previous)=previous{return if previous==callback_sha{Ok(false)}else{Err(conflict("final callback replay content changed"))};}
        let head:Value=sqlx::query_scalar("SELECT jsonb_build_object('version',v.id,'key',c.session_id,'round_id',v.round_id,'execution_id',v.execution_id) FROM tender_draft_state c JOIN tender_draft_versions v ON v.id=c.version_id WHERE c.project_id=$1 FOR UPDATE OF c")
            .bind(cap.project).fetch_one(&mut *tx).await.map_err(pg_error)?;
        if head["key"]!=cap.key.to_string(){return Err(conflict("the final callback belongs to an inactive editor"));}
        let version=Uuid::new_v4();let revision:i64=sqlx::query_scalar("SELECT COALESCE(max(revision),0)+1 FROM tender_draft_versions WHERE project_id=$1")
            .bind(cap.project).fetch_one(&mut *tx).await.map_err(pg_error)?;
        publish_blob(&mut tx,&blob,version,cap.owner).await?;
        sqlx::query("INSERT INTO tender_draft_versions(id,project_id,execution_id,round_id,revision,parent_version_id,source,object_ref,content_sha256,byte_length,outline,created_by_user_id) VALUES($1,$2,$3,$4,$5,$6,'recovered',$7,$8::kb_sha256,$9,$10,$11)")
            .bind(version).bind(cap.project).bind(head["execution_id"].as_str().unwrap_or_default()).bind(id(&head,"round_id")?).bind(revision).bind(id(&head,"version")?)
            .bind(platform::object_ref(&blob.sha)).bind(&blob.sha).bind(blob.len).bind(&outline).bind(cap.owner).execute(&mut *tx).await.map_err(pg_error)?;
        sqlx::query("INSERT INTO tender_draft_recoveries(session_id,callback_sha256,version_id) VALUES($1,$2::kb_sha256,$3)")
            .bind(cap.key).bind(&callback_sha).bind(version).execute(&mut *tx).await.map_err(pg_error)?;
        // The final file is preserved, but does not certify any uncorrelated Save
        // action and does not replace the user's selected draft.
        sqlx::query("UPDATE tender_draft_saves SET state=$3,error_code=2 WHERE project_id=$1 AND session_id=$2 AND state='pending'")
            .bind(cap.project).bind(cap.key).bind(SaveState::Failed).execute(&mut *tx).await.map_err(pg_error)?;
        sqlx::query("UPDATE tender_draft_state SET session_id=NULL,pending_save_id=NULL,editor_error=NULL WHERE project_id=$1 AND session_id=$2")
            .bind(cap.project).bind(cap.key).execute(&mut *tx).await.map_err(pg_error)?;
        sqlx::query("UPDATE tender_editor_sessions SET closed_at=now() WHERE id=$1")
            .bind(cap.key).execute(&mut *tx).await.map_err(pg_error)?;
        tx.commit().await.map_err(pg_error)?;Ok(true)
    }.await;
    if !matches!(result, Ok(true)) {
        expire(pool, &blob, cap.owner).await;
    }
    result.map(|_| ())
}
async fn publish_save(
    pool: &PgPool,
    save: Uuid,
    cap: &Capability,
    callback_sha: &str,
    outline: &Value,
    blob: &Blob,
) -> Result<(), ApiErr> {
    let (project, key) = (cap.project, cap.key);
    let mut tx = pool.begin().await.map_err(pg_error)?;
    project_lock(&mut tx, project, cap.owner).await?;
    let previous:Value=sqlx::query_scalar("SELECT jsonb_build_object('state',state,'parent',parent_version_id,'sha',parent_sha256,'callback_sha',callback_sha256) FROM tender_draft_saves WHERE id=$1 AND project_id=$2 AND session_id=$3 FOR UPDATE")
        .bind(save).bind(project).bind(key).fetch_one(&mut *tx).await.map_err(pg_error)?;
    let previous_state = stored_save_state(&previous)?;
    if previous_state == SaveState::Saved {
        return if previous["callback_sha"] == callback_sha {
            Ok(())
        } else {
            Err(conflict("callback replay content changed"))
        };
    }
    let head:Value=sqlx::query_scalar("SELECT jsonb_build_object('version',c.version_id,'key',c.session_id,'pending',c.pending_save_id,'sha',v.content_sha256,'round_id',v.round_id,'execution_id',v.execution_id) FROM tender_draft_state c JOIN tender_draft_versions v ON v.id=c.version_id WHERE c.project_id=$1 FOR UPDATE OF c")
        .bind(project).fetch_one(&mut *tx).await.map_err(pg_error)?;
    if previous_state != SaveState::Pending
        || head["version"] != previous["parent"]
        || head["sha"] != previous["sha"]
        || head["key"] != key.to_string()
        || head["pending"] != save.to_string()
    {
        return Err(conflict("saved Word parent or session changed"));
    }
    let version = Uuid::new_v4();
    let revision: i64 = sqlx::query_scalar(
        "SELECT COALESCE(max(revision),0)+1 FROM tender_draft_versions WHERE project_id=$1",
    )
    .bind(project)
    .fetch_one(&mut *tx)
    .await
    .map_err(pg_error)?;
    publish_blob(&mut tx, blob, version, cap.owner).await?;
    sqlx::query("INSERT INTO tender_draft_versions(id,project_id,execution_id,round_id,revision,parent_version_id,source,object_ref,content_sha256,byte_length,outline,created_by_user_id) VALUES($1,$2,$3,$4,$5,$6,'manual',$7,$8::kb_sha256,$9,$10,$11)")
        .bind(version).bind(project).bind(head["execution_id"].as_str().unwrap_or_default()).bind(id(&head,"round_id")?).bind(revision).bind(id(&previous,"parent")?)
        .bind(platform::object_ref(&blob.sha)).bind(&blob.sha).bind(blob.len).bind(outline).bind(cap.owner).execute(&mut *tx).await.map_err(pg_error)?;
    sqlx::query("UPDATE tender_draft_state SET version_id=$2,pending_save_id=NULL,editor_error=NULL WHERE project_id=$1")
        .bind(project).bind(version).execute(&mut *tx).await.map_err(pg_error)?;
    sqlx::query("UPDATE tender_draft_saves SET state=$4,result_version_id=$2,callback_sha256=$3::kb_sha256 WHERE id=$1")
        .bind(save).bind(version).bind(callback_sha).bind(SaveState::Saved).execute(&mut *tx).await.map_err(pg_error)?;
    tx.commit().await.map_err(pg_error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_stored_save_state_is_a_storage_error() {
        for value in [json!({"state":"completed"}), json!({})] {
            let (status, Json(error)) = stored_save_state(&value).unwrap_err();
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(error.error.code, "TENDER_STORAGE_UNAVAILABLE");
        }
        assert_eq!(
            stored_save_state(&json!({"state":"saved"})).unwrap(),
            SaveState::Saved
        );
        let request =
            json!({"version_id":Uuid::new_v4(),"docx_sha256":"a".repeat(64),"mode":"admin"});
        assert!(serde_json::from_value::<Open>(request).is_err());
    }

    fn office() -> Office {
        Office {
            server: Url::parse("https://office.example.test/").unwrap(),
            command: Url::parse("http://office:80/").unwrap(),
            api: Url::parse("http://api:8080/").unwrap(),
            secret: "office-test-secret-with-more-than-32-characters".into(),
            capability: "scope-test-secret-with-more-than-32-characters".into(),
            ttl: 1200,
            timeout: 2,
        }
    }
    fn scope(mode: &str) -> Capability {
        Capability {
            aud: "tender-word-source".into(),
            project: Uuid::new_v4(),
            key: Uuid::new_v4(),
            version: Uuid::new_v4(),
            owner: Uuid::new_v4(),
            mode: serde_json::from_value(json!(mode)).unwrap(),
        }
    }

    #[test]
    fn readonly_permissions_are_inside_the_signed_configuration() {
        let office = office();
        for mode in ["view", "edit"] {
            let cap = scope(mode);
            let mut config = editor_config(
                &office,
                &cap,
                "http://api/source",
                if mode == "edit" {
                    Some("http://api/callback")
                } else {
                    None
                },
                Some("zh-CN"),
                Some("https://app.example.test"),
            )
            .unwrap();
            let token = config.as_object_mut().unwrap().remove("token").unwrap();
            let signed = decode::<Value>(
                token.as_str().unwrap(),
                &DecodingKey::from_secret(office.secret.as_bytes()),
                &Validation::new(Algorithm::HS256),
            )
            .unwrap()
            .claims;
            assert_eq!(signed, config);
            assert_eq!(signed["editorConfig"]["mode"], mode);
            assert_eq!(signed["document"]["permissions"]["download"], true);
            for permission in ["edit", "review", "fillForms", "comment"] {
                assert_eq!(
                    signed["document"]["permissions"][permission],
                    mode == "edit"
                );
            }
            assert_eq!(
                signed["editorConfig"]["callbackUrl"].is_string(),
                mode == "edit"
            );
            assert_eq!(
                signed["editorConfig"]["plugins"]["pluginsData"][0],
                "https://app.example.test/onlyoffice-navigation/config.json"
            );
        }
    }
    #[test]
    fn navigation_plugin_uses_the_browser_origin_or_referer() {
        let mut headers = HeaderMap::new();
        assert!(navigation_origin(&headers).is_none());
        headers.insert(
            "referer",
            "https://app.example.test/workbench?project=sample"
                .parse()
                .unwrap(),
        );
        assert_eq!(
            navigation_origin(&headers).as_deref(),
            Some("https://app.example.test")
        );
        headers.insert("origin", "http://127.0.0.1:5174".parse().unwrap());
        assert_eq!(
            navigation_origin(&headers).as_deref(),
            Some("http://127.0.0.1:5174")
        );
        for raw in [
            "null",
            "file:///tmp/site",
            "https://user@app.example.test",
            "https://app.example.test/path",
            "https://app.example.test/?token=sample",
        ] {
            headers.insert("origin", raw.parse().unwrap());
            assert!(navigation_origin(&headers).is_none(), "{raw}");
        }
    }
    #[test]
    fn source_capabilities_cannot_cross_project_session_or_purpose() {
        let office = office();
        let cap = scope("view");
        let token = office.sign(&cap, true).unwrap();
        assert!(capability(&office, &token, "tender-word-source", cap.project, cap.key).is_ok());
        assert!(
            capability(
                &office,
                &token,
                "tender-word-callback",
                cap.project,
                cap.key
            )
            .is_err()
        );
        assert!(
            capability(
                &office,
                &token,
                "tender-word-source",
                Uuid::new_v4(),
                cap.key
            )
            .is_err()
        );
        assert!(
            capability(
                &office,
                &token,
                "tender-word-source",
                cap.project,
                Uuid::new_v4()
            )
            .is_err()
        );
    }
    #[test]
    fn callback_download_is_restricted_to_the_configured_document_service() {
        let office = office();
        assert_eq!(
            office
                .download_url("https://office.example.test/cache/file.docx?token=sample")
                .unwrap()
                .as_str(),
            "http://office/cache/file.docx?token=sample"
        );
        for url in [
            "http://127.0.0.1:15432/",
            "https://attacker.test/file",
            "https://office.example.test.attacker.test/file",
            "https://user@office.example.test/file",
            "file:///etc/passwd",
        ] {
            assert!(office.download_url(url).is_err(), "{url}");
        }
    }
    #[test]
    fn callback_authentication_requires_document_service_signature() {
        let office = office();
        let content = json!({"key":Uuid::new_v4(),"status":6,"forcesavetype":0,"userdata":Uuid::new_v4(),"url":"http://office/cache.docx"});
        assert!(service_payload(&office, &HeaderMap::new(), &content).is_err());
        let signed = json!({"payload":content});
        let mut body = content.clone();
        body["token"] = json!(office.sign(&signed, false).unwrap());
        assert_eq!(
            service_payload(&office, &HeaderMap::new(), &body).unwrap(),
            content
        );
        body["url"] = json!("http://office/other.docx");
        body.as_object_mut().unwrap().remove("token");
        assert_ne!(signed["payload"], body);
    }

    #[test]
    fn office_direct_callback_time_claims_are_not_business_fields() {
        let office = office();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let content = json!({"key":Uuid::new_v4(),"status":1,"users":["owner"],"actions":[{"type":1,"userid":"owner"}]});
        let mut signed = content.clone();
        signed["iat"] = json!(now);
        signed["exp"] = json!(now + 60);
        let mut body = content.clone();
        body["token"] = json!(office.sign(&signed, false).unwrap());
        assert_eq!(
            service_payload(&office, &HeaderMap::new(), &body).unwrap(),
            content
        );
        for (field, value) in [
            ("users", json!(["other"])),
            ("url", json!("http://office/changed.docx")),
            ("status", json!(6)),
        ] {
            let mut changed = body.clone();
            changed[field] = value;
            changed.as_object_mut().unwrap().remove("token");
            assert_ne!(
                service_payload(&office, &HeaderMap::new(), &body).unwrap(),
                changed
            );
        }
        // JWT expiration is still enforced before normalizing its direct claims.
        signed["exp"] = json!(now - 3600);
        body["token"] = json!(office.sign(&signed, false).unwrap());
        assert!(service_payload(&office, &HeaderMap::new(), &body).is_err());
        // A wrapped payload may itself contain business fields named iat/exp.
        let wrapped_content = json!({"key":Uuid::new_v4(),"status":6,"iat":12,"exp":34});
        let wrapped = json!({"payload":wrapped_content,"iat":now,"exp":now + 60});
        let mut wrapped_body = wrapped_content.clone();
        wrapped_body["token"] = json!(office.sign(&wrapped, false).unwrap());
        assert_eq!(
            service_payload(&office, &HeaderMap::new(), &wrapped_body).unwrap(),
            wrapped_content
        );
    }

    #[tokio::test]
    async fn word_routes_require_user_authentication_before_reading_storage() {
        use tower::ServiceExt;
        let app = router().with_state(AppState {
            jwt_secret: "editor-test-jwt-secret".into(),
            bootstrap_key: String::new(),
        });
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri(format!(
                        "/api/v3/tender-projects/{}/docx/current",
                        Uuid::new_v4()
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    async fn fixture_blob(pool: &PgPool, owner: Uuid) -> Blob {
        let bytes = Uuid::new_v4().as_bytes().to_vec();
        let sha = platform::sha256_hex(&bytes);
        let staging = Uuid::new_v4();
        platform::stage_object_upload(
            pool,
            staging,
            &platform::object_ref(&sha),
            &sha,
            DOCX,
            bytes.len() as i64,
            Some(owner),
        )
        .await
        .unwrap();
        Blob {
            sha,
            len: bytes.len() as i64,
            staging,
        }
    }

    #[test]
    #[ignore = "requires the explicitly provisioned isolated tender web database"]
    fn postgres_save_receipt_is_correlated_atomic_and_replay_safe() {
        crate::tender_routes::test_runtime().block_on(async {
        let url = std::env::var("KB_TENDER_WEB_TEST_DATABASE_URL")
            .expect("isolated tender web database required");
        assert!(
            url.contains("knowledgebrain_test_tender_web"),
            "refusing a non-test database"
        );
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(3)
            .connect(&url)
            .await
            .unwrap();
        let cap = scope("edit");
        let save = Uuid::new_v4();
        let round = Uuid::new_v4();
        sqlx::query("INSERT INTO platform_users(id,email) VALUES($1,$2)")
            .bind(cap.owner)
            .bind(format!("editor-{}@example.test", cap.owner))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO tender_projects(id,owner_user_id,title,status) VALUES($1,$2,'editor storage test','open')").bind(cap.project).bind(cap.owner).execute(&pool).await.unwrap();
        let original = fixture_blob(&pool, cap.owner).await;
        let mut tx = pool.begin().await.unwrap();
        publish_blob(&mut tx, &original, cap.version, cap.owner)
            .await
            .unwrap();
        sqlx::query("INSERT INTO tender_draft_versions(id,project_id,execution_id,round_id,revision,source,object_ref,content_sha256,byte_length,outline,created_by_user_id) VALUES($1,$2,'test-execution',$3,1,'generated',$4,$5::kb_sha256,$6,'{}',$7)")
            .bind(cap.version).bind(cap.project).bind(round).bind(platform::object_ref(&original.sha)).bind(&original.sha).bind(original.len).bind(cap.owner).execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO tender_editor_sessions(id,project_id,base_version_id,owner_user_id,mode,request_key,request_sha256) VALUES($1,$2,$3,$4,'edit','open-test',$5::kb_sha256)")
            .bind(cap.key).bind(cap.project).bind(cap.version).bind(cap.owner).bind(&original.sha).execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO tender_draft_state(project_id,version_id,session_id,pending_save_id) VALUES($1,$2,$3,$4)")
            .bind(cap.project).bind(cap.version).bind(cap.key).bind(save).execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO tender_draft_saves(id,project_id,session_id,parent_version_id,parent_sha256,request_key,request_sha256,state) VALUES($1,$2,$3,$4,$5::kb_sha256,'save-test',$5::kb_sha256,'pending')")
            .bind(save).bind(cap.project).bind(cap.key).bind(cap.version).bind(&original.sha).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        let other_project = Uuid::new_v4();
        sqlx::query("INSERT INTO tender_projects(id,owner_user_id,title,status) VALUES($1,$2,'other project','open')")
            .bind(other_project).bind(cap.owner).execute(&pool).await.unwrap();
        let cross_project = sqlx::query("INSERT INTO tender_draft_state(project_id,version_id) VALUES($1,$2)")
            .bind(other_project).bind(cap.version).execute(&pool).await.unwrap_err();
        assert_eq!(cross_project.as_database_error().and_then(|e| e.code()).as_deref(), Some("23503"));
        let cross_session = sqlx::query("INSERT INTO tender_editor_sessions(id,project_id,base_version_id,owner_user_id,mode,request_key,request_sha256) VALUES($1,$2,$3,$4,'edit','invalid-project',$5)")
            .bind(Uuid::new_v4()).bind(other_project).bind(cap.version).bind(cap.owner).bind(&original.sha)
            .execute(&pool).await.unwrap_err();
        assert_eq!(cross_session.as_database_error().and_then(|e| e.code()).as_deref(), Some("23503"));
        let input = Save { editor_key: cap.key, expected: Identity { version_id: cap.version, docx_sha256: original.sha.clone() } };
        sqlx::query("UPDATE tender_draft_saves SET request_sha256=$2::kb_sha256 WHERE id=$1")
            .bind(save).bind(digest(&json!(input)).unwrap()).execute(&pool).await.unwrap();
        // The process may have committed pending before the first command reached
        // Office. Only an explicit replay may resend that same correlation ID.
        let retried = prepare_save(&pool, cap.project, cap.owner, "save-test", &input).await.unwrap();
        assert_eq!(retried.id, save);
        assert!(retried.dispatch, "an explicit pending replay must be able to resend a lost command");
        assert!(retried.replay);
        assert_eq!(
            read_current(&pool, cap.project).await.unwrap().unwrap()["version_id"],
            cap.version.to_string()
        );
        let next = fixture_blob(&pool, cap.owner).await;
        let callback_sha = platform::sha256_hex(b"correlated callback");
        // A callback with a foreign session cannot publish even valid object bytes.
        let foreign = Capability {
            key: Uuid::new_v4(),
            ..scope("edit")
        };
        assert!(
            publish_save(
                &pool,
                save,
                &foreign,
                &callback_sha,
                &json!({"chapters":[]}),
                &next
            )
            .await
            .is_err()
        );
        publish_save(
            &pool,
            save,
            &cap,
            &callback_sha,
            &json!({"chapters":[]}),
            &next,
        )
        .await
        .unwrap();
        let current = read_current(&pool, cap.project).await.unwrap().unwrap();
        let retried = prepare_save(&pool, cap.project, cap.owner, "save-test", &input).await.unwrap();
        assert_eq!(retried.id, save);
        assert!(!retried.dispatch);
        assert_ne!(current["version_id"], cap.version.to_string());
        assert_eq!(current["docx_sha256"], next.sha);
        assert!(current["editor"]["pending_save_id"].is_null());
        publish_save(
            &pool,
            save,
            &cap,
            &callback_sha,
            &json!({"chapters":[]}),
            &next,
        )
        .await
        .unwrap();
        assert!(
            publish_save(
                &pool,
                save,
                &cap,
                &platform::sha256_hex(b"changed callback"),
                &json!({}),
                &next
            )
            .await
            .is_err()
        );
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM tender_draft_versions WHERE project_id=$1")
                .bind(cap.project)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 2);
        // A late command error must not corrupt an already published callback.
        record_failure(&pool, cap.project, cap.key, save, "command", 4)
            .await
            .unwrap();
        assert_eq!(
            read_current(&pool, cap.project).await.unwrap().unwrap()["version_id"],
            current["version_id"]
        );
        assert_eq!(
            read_version(&pool, cap.project, cap.version).await.unwrap()["docx_sha256"],
            original.sha
        );
        });
    }
}
