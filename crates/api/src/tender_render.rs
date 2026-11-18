//! Temporary layout conversion; candidates never become saved Word versions here.
use crate::AppState;
use crate::err::{fail, forbidden, not_found, unauthorized, validation};
use crate::tender_editor::Office;
use crate::tender_routes::{ApiErr, pg_error};
use axum::Router;
use axum::body::{Body, Bytes, to_bytes};
use axum::extract::{Path, Query, Request};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
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

#[derive(Clone)]
struct Candidate {
    claims: SourceClaims,
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
    let nonce = candidate.claims.nonce;
    candidates()
        .lock()
        .map_err(|_| unavailable())?
        .insert(nonce, candidate);
    Ok(CandidateGuard(nonce))
}

fn candidate_bytes(claims: &SourceClaims) -> Result<Bytes, ApiErr> {
    if claims.exp <= now() {
        return Err(unauthorized());
    }
    candidates()
        .lock()
        .map_err(|_| unavailable())?
        .get(&claims.nonce)
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
    candidate_bytes(&claims)?;
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
    Ok((
        [
            ("content-type", DOCX),
            ("cache-control", "private, no-store"),
            ("x-content-type-options", "nosniff"),
        ],
        timed_body(candidate_bytes(&claims)?, deadline),
    )
        .into_response())
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

async fn convert(office: &Office, candidate: &SourceClaims) -> Result<Vec<u8>, ApiErr> {
    let mut url = office
        .api
        .join("api/internal/tender-render/")
        .map_err(|_| unavailable())?;
    url.path_segments_mut()
        .map_err(|_| unavailable())?
        .pop_if_empty()
        .push(&candidate.execution_id)
        .push(&candidate.nonce.to_string())
        .push("source");
    url.query_pairs_mut()
        .append_pair("token", &office.sign(candidate, true)?);
    let mut command = json!({"async":false,"filetype":"docx","outputtype":"pdf",
        "key":candidate.nonce.simple().to_string(),"title":"candidate.docx","url":url.as_str()});
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
    let source = SourceClaims {
        aud: SOURCE_PURPOSE.into(),
        execution_id: execution.clone(),
        docx_sha256: claims.docx_sha256.clone(),
        epoch: claims.epoch,
        project,
        owner,
        nonce: Uuid::new_v4(),
        exp: now() + seconds as usize + 15,
    };
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

#[cfg(test)]
#[path = "tender_render_tests.rs"]
mod tests;
