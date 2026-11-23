//! ONLYOFFICE conversion for temporary layout candidates and saved Word versions.
use crate::AppState;
use crate::err::{fail, forbidden, not_found, unauthorized, validation};
use crate::tender_editor::Office;
use crate::tender_routes::{ApiErr, pg_error, require_project};
use axum::body::{Body, Bytes, to_bytes};
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use std::collections::BTreeMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock};
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::sync::{Semaphore, SemaphorePermit};
use uuid::Uuid;

const DOCX: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
const WORKER_PURPOSE: &str = "tender_docx_render_v1";
const SOURCE_PURPOSE: &str = "tender-docx-render-source";
const SAVED_SOURCE_PURPOSE: &str = "tender-saved-pdf-source";
const MAX_BYTES: usize = 50 * 1024 * 1024;
const MAX_SECONDS: u64 = 180;
static SLOTS: Semaphore = Semaphore::const_new(4);

struct QuotaBytes {
    bytes: Bytes,
    _permit: Arc<SemaphorePermit<'static>>,
}

impl AsRef<[u8]> for QuotaBytes {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

fn retain_quota(bytes: Bytes, permit: Arc<SemaphorePermit<'static>>) -> Bytes {
    Bytes::from_owner(QuotaBytes {
        bytes,
        _permit: permit,
    })
}

struct TimedBody {
    bytes: Bytes,
    deadline: Pin<Box<tokio::time::Sleep>>,
}

impl http_body::Body for TimedBody {
    type Data = Bytes;
    type Error = std::io::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Bytes>, Self::Error>>> {
        let this = self.get_mut();
        if this.bytes.is_empty() {
            return Poll::Ready(None);
        }
        if tokio::time::Instant::now() >= this.deadline.deadline()
            || std::future::Future::poll(this.deadline.as_mut(), cx).is_ready()
        {
            this.bytes = Bytes::new();
            return Poll::Ready(Some(Err(std::io::ErrorKind::TimedOut.into())));
        }
        let data = this.bytes.split_to(this.bytes.len().min(64 * 1024));
        Poll::Ready(Some(Ok(http_body::Frame::data(data))))
    }

    fn is_end_stream(&self) -> bool {
        self.bytes.is_empty()
    }

    fn size_hint(&self) -> http_body::SizeHint {
        http_body::SizeHint::with_exact(self.bytes.len() as u64)
    }
}

fn timed_body(bytes: Bytes, deadline: tokio::time::Instant) -> Body {
    Body::new(TimedBody {
        bytes,
        deadline: Box::pin(tokio::time::sleep_until(deadline)),
    })
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WorkerClaims {
    purpose: String,
    execution_id: String,
    docx_sha256: String,
    epoch: i64,
    exp: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct SourceClaims {
    aud: String,
    execution_id: String,
    docx_sha256: String,
    epoch: i64,
    project: Uuid,
    owner: Uuid,
    nonce: Uuid,
    exp: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct SavedSourceClaims {
    aud: String,
    project: Uuid,
    owner: Uuid,
    version: Uuid,
    docx_sha256: String,
    nonce: Uuid,
    exp: usize,
}

#[derive(Clone, PartialEq, Eq)]
enum ConversionSource {
    Render(SourceClaims),
    Saved(SavedSourceClaims),
}

impl ConversionSource {
    fn lifetime(&self) -> (Uuid, usize) {
        match self {
            Self::Render(claims) => (claims.nonce, claims.exp),
            Self::Saved(claims) => (claims.nonce, claims.exp),
        }
    }

    fn url(&self, office: &Office) -> Result<reqwest::Url, ApiErr> {
        let (base, parts, token) = match self {
            Self::Render(claims) => (
                "api/internal/tender-render/",
                vec![claims.execution_id.clone(), claims.nonce.to_string()],
                office.sign(claims, true)?,
            ),
            Self::Saved(claims) => (
                "api/internal/tender-saved-pdf/",
                vec![
                    claims.project.to_string(),
                    claims.version.to_string(),
                    claims.nonce.to_string(),
                ],
                office.sign(claims, true)?,
            ),
        };
        let mut url = office.api.join(base).map_err(|_| unavailable())?;
        url.path_segments_mut()
            .map_err(|_| unavailable())?
            .pop_if_empty()
            .extend(parts)
            .push("source");
        url.query_pairs_mut().append_pair("token", &token);
        Ok(url)
    }
}

#[derive(Clone)]
struct Candidate {
    claims: ConversionSource,
    bytes: Bytes,
}

fn candidates() -> &'static Mutex<BTreeMap<Uuid, Candidate>> {
    static CANDIDATES: OnceLock<Mutex<BTreeMap<Uuid, Candidate>>> = OnceLock::new();
    CANDIDATES.get_or_init(|| Mutex::new(BTreeMap::new()))
}

struct CandidateGuard(Uuid);
impl Drop for CandidateGuard {
    fn drop(&mut self) {
        if let Ok(mut stored) = candidates().lock() {
            stored.remove(&self.0);
        }
    }
}

fn install(candidate: Candidate) -> Result<CandidateGuard, ApiErr> {
    let (nonce, _) = candidate.claims.lifetime();
    candidates()
        .lock()
        .map_err(|_| unavailable())?
        .insert(nonce, candidate);
    Ok(CandidateGuard(nonce))
}

fn candidate_bytes(claims: &ConversionSource) -> Result<Bytes, ApiErr> {
    let (nonce, exp) = claims.lifetime();
    if exp <= now() {
        return Err(unauthorized());
    }
    candidates()
        .lock()
        .map_err(|_| unavailable())?
        .get(&nonce)
        .filter(|c| c.claims == *claims)
        .map(|c| c.bytes.clone())
        .ok_or_else(|| not_found("temporary layout candidate"))
}

fn unavailable() -> ApiErr {
    fail(
        StatusCode::SERVICE_UNAVAILABLE,
        "TENDER_RENDER_UNAVAILABLE",
        "Word页码布局暂不可用；候选保留，可重试布局或在Office中更新域并确认保存。",
    )
}

fn now() -> usize {
    chrono::Utc::now().timestamp().max(0) as usize
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn worker_claims(token: &str, secret: &str, execution: &str) -> Result<WorkerClaims, ApiErr> {
    let mut rules = Validation::new(Algorithm::HS256);
    rules.leeway = 0;
    let claims =
        decode::<WorkerClaims>(token, &DecodingKey::from_secret(secret.as_bytes()), &rules)
            .map_err(|_| unauthorized())?
            .claims;
    if secret.trim().is_empty()
        || claims.purpose != WORKER_PURPOSE
        || claims.execution_id != execution
        || claims.epoch < 1
        || !valid_hash(&claims.docx_sha256)
        || claims.exp <= now()
        || claims.exp > now() + 90
    {
        return Err(unauthorized());
    }
    Ok(claims)
}

fn source_claims(
    token: &str,
    secret: &str,
    execution: &str,
    nonce: Uuid,
) -> Result<SourceClaims, ApiErr> {
    let mut rules = Validation::new(Algorithm::HS256);
    rules.leeway = 0;
    rules.set_audience(&[SOURCE_PURPOSE]);
    let claims =
        decode::<SourceClaims>(token, &DecodingKey::from_secret(secret.as_bytes()), &rules)
            .map_err(|_| unauthorized())?
            .claims;
    if secret.trim().is_empty()
        || claims.execution_id != execution
        || claims.nonce != nonce
        || claims.epoch < 1
        || !valid_hash(&claims.docx_sha256)
        || claims.exp <= now()
        || claims.exp > now() + MAX_SECONDS as usize + 15
    {
        return Err(unauthorized());
    }
    Ok(claims)
}

async fn active_scope(pool: &PgPool, execution: &str, epoch: i64) -> Result<(Uuid, Uuid), ApiErr> {
    let row = sqlx::query(
        "SELECT p.id,p.owner_user_id,j.payload FROM tender_authoring_jobs j
         JOIN tender_projects p ON p.id=j.project_id
         JOIN tender_authoring_executions e ON e.execution_id=j.execution_id
         WHERE j.execution_id=$1 AND j.status IN ('pending','published') AND p.status='open'
         AND COALESCE(j.payload->>'operation','document')='document'
         AND e.status='running' AND e.epoch=$2 AND NOT e.cancel_requested
         AND e.lease_until>clock_timestamp() AND e.authoring_input->>'run_id'=j.run_id
         AND j.payload#>>'{identity,run_id}'=j.run_id
         AND e.authoring_input->>'source_version'=j.payload#>>'{identity,source_version}'
         AND e.authoring_input->>'domain_revision'=j.payload#>>'{identity,domain_revision}'",
    )
    .bind(execution)
    .bind(epoch)
    .fetch_optional(pool)
    .await
    .map_err(pg_error)?
    .ok_or_else(|| not_found("active layout execution"))?;
    let project: Uuid = row.try_get("id").map_err(pg_error)?;
    let owner: Uuid = row.try_get("owner_user_id").map_err(pg_error)?;
    let payload: Value = row.try_get("payload").map_err(pg_error)?;
    if payload["owner_user_id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        != Some(owner)
    {
        return Err(forbidden());
    }
    Ok((project, owner))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ticket {
    token: String,
}

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v3/tender-projects/{project}/docx/versions/{version}/pdf",
            post(saved_pdf),
        )
        .route(
            "/api/internal/tender-saved-pdf/{project}/{version}/{nonce}/source",
            get(saved_pdf_source),
        )
        .route("/api/internal/tender-render/{execution}", post(render))
        .route(
            "/api/internal/tender-render/{execution}/{nonce}/source",
            get(source),
        )
}

async fn source(
    Path((execution, nonce)): Path<(String, Uuid)>,
    Query(ticket): Query<Ticket>,
) -> Result<Response, ApiErr> {
    let secret =
        platform::required_secret("KB_ONLYOFFICE_CAPABILITY_SECRET").map_err(|_| unauthorized())?;
    let claims = source_claims(&ticket.token, &secret, &execution, nonce)?;
    let source = ConversionSource::Render(claims.clone());
    candidate_bytes(&source)?;
    let deadline =
        tokio::time::Instant::now() + Duration::from_secs(claims.exp.saturating_sub(now()) as u64);
    let pool = tokio::time::timeout_at(deadline, platform::connect())
        .await
        .map_err(|_| unavailable())?
        .map_err(pg_error)?;
    if tokio::time::timeout_at(deadline, active_scope(&pool, &execution, claims.epoch))
        .await
        .map_err(|_| unavailable())??
        != (claims.project, claims.owner)
    {
        return Err(forbidden());
    }
    Ok(source_response(candidate_bytes(&source)?, deadline))
}

fn source_response(bytes: Bytes, deadline: tokio::time::Instant) -> Response {
    (
        [
            ("content-type", DOCX),
            ("cache-control", "private, no-store"),
            ("x-content-type-options", "nosniff"),
        ],
        timed_body(bytes, deadline),
    )
        .into_response()
}

async fn bounded(mut response: reqwest::Response, limit: usize) -> Result<Vec<u8>, ApiErr> {
    if !response.status().is_success() {
        return Err(unavailable());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| unavailable())? {
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err(unavailable());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

async fn convert(office: &Office, source: &ConversionSource) -> Result<Vec<u8>, ApiErr> {
    let (nonce, _) = source.lifetime();
    let url = source.url(office)?;
    let mut command = json!({"async":false,"filetype":"docx","outputtype":"pdf",
        "key":nonce.simple().to_string(),"title":"candidate.docx","url":url.as_str()});
    command["token"] = json!(office.sign(&command, false)?);
    let client = office.client()?;
    let response = client
        .post(
            office
                .command
                .join("converter")
                .map_err(|_| unavailable())?,
        )
        .header("accept", "application/json")
        .json(&command)
        .send()
        .await
        .map_err(|_| unavailable())?;
    let result: Value =
        serde_json::from_slice(&bounded(response, 64 * 1024).await?).map_err(|_| unavailable())?;
    if result["endConvert"] != true
        || result.get("error").is_some_and(|e| e != 0)
        || result.get("fileType").is_some_and(|t| t != "pdf")
    {
        return Err(unavailable());
    }
    let download = office.download_url(result["fileUrl"].as_str().ok_or_else(unavailable)?)?;
    let response = client
        .get(download)
        .send()
        .await
        .map_err(|_| unavailable())?;
    let bytes = bounded(response, platform::max_file_bytes().min(MAX_BYTES)).await?;
    if !bytes.starts_with(b"%PDF-") {
        return Err(unavailable());
    }
    Ok(bytes)
}

async fn render(Path(execution): Path<String>, request: Request) -> Result<Response, ApiErr> {
    let token = request
        .headers()
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .ok_or_else(unauthorized)?;
    let secret = platform::required_secret("GRPC_AUTH_TOKEN").map_err(|_| unauthorized())?;
    let claims = worker_claims(token, &secret, &execution)?;
    let office = Office::load()?;
    let seconds = office.timeout.min(MAX_SECONDS);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(seconds);
    let pool = tokio::time::timeout_at(deadline, platform::connect())
        .await
        .map_err(|_| unavailable())?
        .map_err(pg_error)?;
    let (project, owner) =
        tokio::time::timeout_at(deadline, active_scope(&pool, &execution, claims.epoch))
            .await
            .map_err(|_| unavailable())??;
    let slot = Arc::new(SLOTS.try_acquire().map_err(|_| unavailable())?);
    if request
        .headers()
        .get("content-type")
        .and_then(|h| h.to_str().ok())
        != Some(DOCX)
    {
        return Err(validation("layout conversion requires a DOCX body"));
    }
    let body = tokio::time::timeout_at(
        deadline,
        to_bytes(
            request.into_body(),
            platform::max_file_bytes().min(MAX_BYTES),
        ),
    )
    .await
    .map_err(|_| unavailable())?
    .map_err(|_| validation("layout candidate exceeds the file limit"))?;
    if !body.starts_with(b"PK\x03\x04") || platform::sha256_hex(&body) != claims.docx_sha256 {
        return Err(validation("layout candidate hash or format mismatch"));
    }
    let source = ConversionSource::Render(SourceClaims {
        aud: SOURCE_PURPOSE.into(),
        execution_id: execution.clone(),
        docx_sha256: claims.docx_sha256.clone(),
        epoch: claims.epoch,
        project,
        owner,
        nonce: Uuid::new_v4(),
        exp: now() + seconds as usize + 15,
    });
    let _candidate = install(Candidate {
        claims: source.clone(),
        bytes: retain_quota(body, slot.clone()),
    })?;
    let pdf = tokio::time::timeout_at(deadline, convert(&office, &source))
        .await
        .map_err(|_| unavailable())??;
    if tokio::time::timeout_at(deadline, active_scope(&pool, &execution, claims.epoch))
        .await
        .map_err(|_| unavailable())??
        != (project, owner)
    {
        return Err(forbidden());
    }
    let service = platform::sha256_hex(office.command.as_str().as_bytes());
    let hash = platform::sha256_hex(&pdf);
    let mut response = timed_body(retain_quota(Bytes::from(pdf), slot), deadline).into_response();
    let headers = response.headers_mut();
    headers.insert("content-type", "application/pdf".parse().unwrap());
    headers.insert("cache-control", "private, no-store".parse().unwrap());
    headers.insert("x-content-type-options", "nosniff".parse().unwrap());
    headers.insert(
        "x-kb-render-engine",
        "onlyoffice-converter-v1".parse().unwrap(),
    );
    for (key, value) in [
        ("x-kb-render-service-sha256", service),
        ("x-kb-render-source-sha256", claims.docx_sha256),
        ("x-kb-render-pdf-sha256", hash),
    ] {
        headers.insert(key, value.parse().map_err(|_| unavailable())?);
    }
    Ok(response)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedPdfRequest {
    docx_sha256: String,
}

#[derive(sqlx::FromRow)]
struct SavedVersion {
    docx_sha256: String,
    byte_length: i64,
    revision: i64,
}

async fn saved_version(
    pool: &PgPool,
    project: Uuid,
    owner: Uuid,
    version: Uuid,
) -> Result<SavedVersion, ApiErr> {
    sqlx::query_as(
        "SELECT v.content_sha256::text AS docx_sha256,v.byte_length,v.revision
         FROM tender_draft_versions v JOIN tender_projects p ON p.id=v.project_id
         WHERE v.project_id=$1 AND p.owner_user_id=$2 AND v.id=$3",
    )
    .bind(project)
    .bind(owner)
    .bind(version)
    .fetch_optional(pool)
    .await
    .map_err(pg_error)?
    .ok_or_else(|| not_found("Word version"))
}

fn saved_pdf_unavailable() -> ApiErr {
    fail(
        StatusCode::SERVICE_UNAVAILABLE,
        "SAVED_PDF_UNAVAILABLE",
        "此版本的 PDF 暂不可用，请从版本行重试。",
    )
}

fn saved_pdf_conflict() -> ApiErr {
    fail(
        StatusCode::CONFLICT,
        "DOCX_VERSION_CONFLICT",
        "此 Word 版本与请求或存储的文件不一致，请刷新后核对版本。",
    )
}

fn saved_pdf_too_large() -> ApiErr {
    fail(
        StatusCode::PAYLOAD_TOO_LARGE,
        "SAVED_PDF_FILE_TOO_LARGE",
        "此 Word 版本超过 PDF 转换文件限额，可下载 DOCX。",
    )
}

async fn saved_pdf(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project, version)): Path<(Uuid, Uuid)>,
    Json(input): Json<SavedPdfRequest>,
) -> Result<Response, ApiErr> {
    let (pool, owner) = require_project(&headers, &state, project).await?;
    let saved = saved_version(&pool, project, owner, version).await?;
    if !valid_hash(&input.docx_sha256) || saved.docx_sha256 != input.docx_sha256 {
        return Err(saved_pdf_conflict());
    }
    let limit = platform::max_file_bytes().min(MAX_BYTES);
    let length = usize::try_from(saved.byte_length)
        .ok()
        .filter(|len| *len > 0)
        .ok_or_else(saved_pdf_conflict)?;
    if length > limit {
        return Err(saved_pdf_too_large());
    }

    let office = Office::load().map_err(|_| saved_pdf_unavailable())?;
    let seconds = office.timeout.min(MAX_SECONDS);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(seconds);
    let slot = Arc::new(SLOTS.try_acquire().map_err(|_| saved_pdf_unavailable())?);
    let storage_slot = slot.clone();
    let hash = input.docx_sha256.clone();
    let read = tokio::task::spawn_blocking(move || {
        // Keep the quota until the storage read ends even if its caller times out.
        let _slot = storage_slot;
        platform::read_blob_bounded(&hash, length)
    });
    let bytes = tokio::time::timeout_at(deadline, read)
        .await
        .map_err(|_| saved_pdf_unavailable())?
        .map_err(|_| saved_pdf_unavailable())?
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::FileTooLarge {
                saved_pdf_conflict()
            } else {
                saved_pdf_unavailable()
            }
        })?;
    if bytes.len() != length
        || !bytes.starts_with(b"PK\x03\x04")
        || platform::sha256_hex(&bytes) != input.docx_sha256
    {
        return Err(saved_pdf_conflict());
    }

    let source = ConversionSource::Saved(SavedSourceClaims {
        aud: SAVED_SOURCE_PURPOSE.into(),
        project,
        owner,
        version,
        docx_sha256: input.docx_sha256.clone(),
        nonce: Uuid::new_v4(),
        exp: now()
            + deadline
                .saturating_duration_since(tokio::time::Instant::now())
                .as_secs() as usize
            + 1,
    });
    let candidate = install(Candidate {
        claims: source.clone(),
        bytes: retain_quota(Bytes::from(bytes), slot.clone()),
    })
    .map_err(|_| saved_pdf_unavailable())?;
    let pdf = tokio::time::timeout_at(deadline, convert(&office, &source))
        .await
        .map_err(|_| saved_pdf_unavailable())?
        .map_err(|_| saved_pdf_unavailable())?;
    drop(candidate);
    let version_after =
        tokio::time::timeout_at(deadline, saved_version(&pool, project, owner, version))
            .await
            .map_err(|_| saved_pdf_unavailable())??;
    if version_after.docx_sha256 != input.docx_sha256 {
        return Err(saved_pdf_conflict());
    }
    let pdf_hash = platform::sha256_hex(&pdf);
    Response::builder()
        .header("content-type", "application/pdf")
        .header("cache-control", "private, no-store")
        .header("x-content-type-options", "nosniff")
        .header(
            "content-disposition",
            format!("attachment; filename=\"tender-v{}.pdf\"", saved.revision),
        )
        .header("x-kb-render-source-version", version.to_string())
        .header("x-kb-render-source-sha256", input.docx_sha256)
        .header("x-kb-render-pdf-sha256", pdf_hash)
        .body(timed_body(retain_quota(Bytes::from(pdf), slot), deadline))
        .map_err(|_| saved_pdf_unavailable())
}

async fn saved_pdf_source(
    Path((project, version, nonce)): Path<(Uuid, Uuid, Uuid)>,
    Query(ticket): Query<Ticket>,
) -> Result<Response, ApiErr> {
    let secret =
        platform::required_secret("KB_ONLYOFFICE_CAPABILITY_SECRET").map_err(|_| unauthorized())?;
    let mut rules = Validation::new(Algorithm::HS256);
    rules.leeway = 0;
    rules.set_audience(&[SAVED_SOURCE_PURPOSE]);
    let claims = decode::<SavedSourceClaims>(
        &ticket.token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &rules,
    )
    .map_err(|_| unauthorized())?
    .claims;
    let current_time = now();
    if claims.project != project
        || claims.version != version
        || claims.nonce != nonce
        || claims.exp <= current_time
        || claims.exp > current_time + MAX_SECONDS as usize + 1
        || !valid_hash(&claims.docx_sha256)
    {
        return Err(unauthorized());
    }
    let deadline =
        tokio::time::Instant::now() + Duration::from_secs((claims.exp - current_time) as u64);
    let owner = claims.owner;
    let sha = claims.docx_sha256.clone();
    let source = ConversionSource::Saved(claims);
    candidate_bytes(&source)?;
    let pool = tokio::time::timeout_at(deadline, platform::connect())
        .await
        .map_err(|_| saved_pdf_unavailable())?
        .map_err(pg_error)?;
    let saved = tokio::time::timeout_at(deadline, saved_version(&pool, project, owner, version))
        .await
        .map_err(|_| saved_pdf_unavailable())??;
    if saved.docx_sha256 != sha {
        return Err(saved_pdf_conflict());
    }
    Ok(source_response(candidate_bytes(&source)?, deadline))
}

#[cfg(test)]
#[path = "tender_render_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tender_saved_pdf_tests.rs"]
mod saved_pdf_tests;
