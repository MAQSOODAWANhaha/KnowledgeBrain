//! Project-owned HTTP adapter for the independent Python tender workflow.
use crate::AppState;
use crate::err::{ErrorBody, fail, not_found, validation};
use crate::routes::actor_from;
use crate::tender_state::ProjectStatus;
use axum::extract::{Multipart, Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use uuid::Uuid;

pub(crate) type ApiErr = (StatusCode, Json<ErrorBody>);

#[cfg(test)]
#[path = "tender_replay_tests.rs"]
mod replay_tests;

#[cfg(test)]
#[path = "tender_recovery_tests.rs"]
mod recovery_tests;

// The production pool is process-wide. Database tests must keep the runtime
// that drives those connections alive across individual test functions too.
#[cfg(test)]
pub(crate) fn test_runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap()
    })
}

pub(crate) fn pg_error(error: sqlx::Error) -> ApiErr {
    tracing::error!(kind = %error.as_database_error().map(|e| e.code().unwrap_or_default()).unwrap_or_default(), "Tender database operation failed");
    fail(
        StatusCode::SERVICE_UNAVAILABLE,
        "TENDER_STORAGE_UNAVAILABLE",
        "招标数据暂不可用，请重试；部署时须完成对应迁移",
    )
}

async fn user(headers: &HeaderMap, state: &AppState) -> Result<Uuid, ApiErr> {
    Ok(actor_from(headers, state).await?.user_id())
}

pub(crate) async fn require_project(
    headers: &HeaderMap,
    state: &AppState,
    project: Uuid,
) -> Result<(PgPool, Uuid), ApiErr> {
    let owner = user(headers, state).await?;
    let pool = platform::connect().await.map_err(pg_error)?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM tender_projects WHERE id=$1 AND owner_user_id=$2)",
    )
    .bind(project)
    .bind(owner)
    .fetch_one(&pool)
    .await
    .map_err(pg_error)?;
    if !exists {
        return Err(not_found("tender project"));
    }
    Ok((pool, owner))
}

pub(crate) async fn lock_open_project(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project: Uuid,
    owner: Uuid,
) -> Result<(), ApiErr> {
    let status: Option<ProjectStatus> = sqlx::query_scalar(
        "SELECT status FROM tender_projects WHERE id=$1 AND owner_user_id=$2 FOR UPDATE",
    )
    .bind(project)
    .bind(owner)
    .fetch_optional(&mut **tx)
    .await
    .map_err(pg_error)?;
    match status {
        Some(ProjectStatus::Open) => Ok(()),
        Some(ProjectStatus::Ended) => Err(fail(
            StatusCode::CONFLICT,
            "PROJECT_ENDED",
            "已结束的项目不可修改",
        )),
        None => Err(not_found("tender project")),
    }
}

pub(crate) async fn bridge(
    project: Uuid,
    owner: Uuid,
    action: &str,
    extra: Value,
    blob: Vec<u8>,
) -> Result<(Value, Vec<u8>), ApiErr> {
    let mut payload = extra
        .as_object()
        .cloned()
        .ok_or_else(|| validation("command body must be an object"))?;
    payload.insert("action".into(), json!(action));
    payload.insert("project_id".into(), json!(project));
    payload.insert("owner_user_id".into(), json!(owner));
    let encoded = serde_json::to_vec(&payload).map_err(|_| validation("invalid command"))?;
    let (data, bytes) = docparser::tender_command(encoded, blob)
        .await
        .map_err(|_| {
            fail(
                StatusCode::SERVICE_UNAVAILABLE,
                "TENDER_BRIDGE_UNAVAILABLE",
                "招标服务暂不可用；请检查DocReader桥接、认证及工作进程配置",
            )
        })?;
    let value: Value = serde_json::from_slice(&data).map_err(|_| {
        fail(
            StatusCode::BAD_GATEWAY,
            "TENDER_BRIDGE_PROTOCOL",
            "招标服务返回了无效结果",
        )
    })?;
    if let Some(error) = value.get("error") {
        let status = error
            .get("status")
            .and_then(Value::as_u64)
            .and_then(|n| u16::try_from(n).ok())
            .and_then(|n| StatusCode::from_u16(n).ok())
            .filter(StatusCode::is_client_error)
            .unwrap_or(StatusCode::SERVICE_UNAVAILABLE);
        return Err(fail(
            status,
            error
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or("TENDER_OPERATION_FAILED"),
            error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("招标操作失败，请使用相同请求标识重试"),
        ));
    }
    Ok((value, bytes))
}

fn request_key(headers: &HeaderMap) -> Result<String, ApiErr> {
    let key = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if key.is_empty() || key.len() > 128 || !key.bytes().all(|b| (33..=126).contains(&b)) {
        return Err(validation(
            "Idempotency-Key must contain 1-128 printable non-space characters",
        ));
    }
    Ok(key.to_owned())
}

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v3/tender-projects",
            get(list_projects).post(create_project),
        )
        .route(
            "/api/v3/tender-projects/{project}",
            get(project).patch(patch_project),
        )
        .route(
            "/api/v3/tender-projects/{project}/workbench",
            get(workbench),
        )
        .route(
            "/api/v3/tender-projects/{project}/documents",
            get(documents).post(upload),
        )
        .route(
            "/api/v3/tender-projects/{project}/documents/{document}/download",
            get(download_document),
        )
        .route(
            "/api/v3/tender-projects/{project}/analysis",
            get(analysis_status).post(analysis_start),
        )
        .route(
            "/api/v3/tender-projects/{project}/analysis/review",
            get(analysis_review),
        )
        .route(
            "/api/v3/tender-projects/{project}/analysis/confirm",
            post(analysis_confirm),
        )
        .route(
            "/api/v3/tender-projects/{project}/analysis/correct",
            post(analysis_correct),
        )
        .route(
            "/api/v3/tender-projects/{project}/analysis/recovery",
            get(analysis_recovery),
        )
        .route(
            "/api/v3/tender-projects/{project}/analysis/retry",
            post(analysis_retry),
        )
        .route(
            "/api/v3/tender-projects/{project}/authoring",
            get(authoring_status).post(authoring_start),
        )
        .route(
            "/api/v3/tender-projects/{project}/outline",
            get(outline_status).put(outline_save),
        )
        .route(
            "/api/v3/tender-projects/{project}/outline/generate",
            post(outline_start),
        )
        .route(
            "/api/v3/tender-projects/{project}/outline/revisions",
            get(outline_history),
        )
        .route(
            "/api/v3/tender-projects/{project}/outline/revisions/{revision}",
            get(outline_revision),
        )
        .route(
            "/api/v3/tender-projects/{project}/executions/{execution}/resume",
            post(resume),
        )
        .route(
            "/api/v3/tender-projects/{project}/executions/{execution}/cancel",
            post(cancel),
        )
        .route(
            "/api/v3/tender-projects/{project}/executions/{execution}/package",
            get(package),
        )
        .route(
            "/api/v3/tender-projects/{project}/executions/{execution}/materials/{material}/original",
            get(material_original),
        )
}

fn encode_segment(value: &str) -> String {
    use std::fmt::Write;
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            write!(&mut encoded, "%{byte:02X}").expect("writing to String");
        }
    }
    encoded
}

fn material_original_url(project: Uuid, execution: &str, material: &str) -> String {
    format!(
        "/api/v3/tender-projects/{project}/executions/{}/materials/{}/original",
        encode_segment(execution),
        encode_segment(material),
    )
}

async fn project_value(pool: &PgPool, id: Uuid) -> Result<Value, ApiErr> {
    let (mut value, status): (Value, ProjectStatus) = sqlx::query_as("SELECT jsonb_build_object('id',id,'title',title,'created_at',created_at,'ended_at',ended_at),status FROM tender_projects WHERE id=$1")
        .bind(id).fetch_optional(pool).await.map_err(pg_error)?.ok_or_else(|| not_found("tender project"))?;
    value["status"] = json!(status);
    Ok(value)
}

async fn list_projects(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiErr> {
    let owner = user(&headers, &state).await?;
    let pool = platform::connect().await.map_err(pg_error)?;
    let rows: Vec<(Value, ProjectStatus)> = sqlx::query_as("SELECT jsonb_build_object('id',id,'title',title,'created_at',created_at,'ended_at',ended_at),status FROM tender_projects WHERE owner_user_id=$1 ORDER BY created_at DESC,id")
        .bind(owner).fetch_all(&pool).await.map_err(pg_error)?;
    let rows: Vec<Value> = rows
        .into_iter()
        .map(|(mut value, status)| {
            value["status"] = json!(status);
            value
        })
        .collect();
    Ok(Json(json!(rows)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateProject {
    title: String,
}

async fn create_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateProject>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    let owner = user(&headers, &state).await?;
    let key = request_key(&headers)?;
    let title = body.title.trim();
    if title.is_empty() || title.len() > 1024 {
        return Err(validation("title must contain 1-1024 UTF-8 bytes"));
    }
    let id = Uuid::new_v5(&owner, format!("tender-project:{key}").as_bytes());
    let pool = platform::connect().await.map_err(pg_error)?;
    sqlx::query("INSERT INTO tender_projects(id,owner_user_id,title,status) VALUES($1,$2,$3,$4) ON CONFLICT(id) DO NOTHING")
        .bind(id).bind(owner).bind(title).bind(ProjectStatus::Open).execute(&pool).await.map_err(pg_error)?;
    let value = project_value(&pool, id).await?;
    if value["title"] != title {
        return Err(fail(
            StatusCode::CONFLICT,
            "IDEMPOTENCY_CONFLICT",
            "同一请求标识已用于另一项目名称",
        ));
    }
    Ok((StatusCode::CREATED, Json(value)))
}

async fn project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    let (pool, _) = require_project(&headers, &state, id).await?;
    Ok(Json(project_value(&pool, id).await?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PatchProject {
    title: Option<String>,
    status: Option<ProjectStatus>,
}

async fn patch_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<PatchProject>,
) -> Result<Json<Value>, ApiErr> {
    let (pool, owner) = require_project(&headers, &state, id).await?;
    if body
        .title
        .as_ref()
        .is_some_and(|s| s.trim().is_empty() || s.len() > 1024)
    {
        return Err(validation("invalid project title or status"));
    }
    let mut tx = pool.begin().await.map_err(pg_error)?;
    if matches!(body.status, Some(ProjectStatus::Ended)) {
        let current: ProjectStatus = sqlx::query_scalar(
            "SELECT status FROM tender_projects WHERE id=$1 AND owner_user_id=$2 FOR UPDATE",
        )
        .bind(id)
        .bind(owner)
        .fetch_optional(&mut *tx)
        .await
        .map_err(pg_error)?
        .ok_or_else(|| not_found("tender project"))?;
        if current == ProjectStatus::Open {
            let active: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tender_draft_state WHERE project_id=$1 AND (session_id IS NOT NULL OR pending_save_id IS NOT NULL))")
                .bind(id).fetch_one(&mut *tx).await.map_err(pg_error)?;
            if active {
                return Err(fail(
                    StatusCode::CONFLICT,
                    "PROJECT_HAS_ACTIVE_EDITOR",
                    "请先保存并关闭Word编辑会话，再结束项目。",
                ));
            }
        }
    }
    sqlx::query("UPDATE tender_projects SET title=COALESCE($2,title),status=COALESCE($3,status),ended_at=CASE WHEN $3='ended' THEN COALESCE(ended_at,now()) WHEN $3='open' THEN NULL ELSE ended_at END WHERE id=$1")
        .bind(id).bind(body.title.map(|s|s.trim().to_owned())).bind(body.status).execute(&mut *tx).await.map_err(pg_error)?;
    tx.commit().await.map_err(pg_error)?;
    Ok(Json(project_value(&pool, id).await?))
}

async fn document_values(pool: &PgPool, id: Uuid) -> Result<Value, ApiErr> {
    let values: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',id,'document_id',id,'file_name',file_name,'media_type',media_type,'byte_length',byte_length,'sha256',content_sha256,'created_at',created_at,'download_url','/api/v3/tender-projects/'||project_id::text||'/documents/'||id::text||'/download') FROM tender_documents WHERE project_id=$1 ORDER BY created_at,id")
        .bind(id).fetch_all(pool).await.map_err(pg_error)?;
    Ok(json!(values))
}

async fn documents(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    let (pool, _) = require_project(&headers, &state, id).await?;
    Ok(Json(document_values(&pool, id).await?))
}

fn file_media_type(name: &str) -> Option<&'static str> {
    Some(
        match name.rsplit('.').next()?.to_ascii_lowercase().as_str() {
            "pdf" => "application/pdf",
            "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "doc" => "application/msword",
            "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            "xls" => "application/vnd.ms-excel",
            "txt" => "text/plain",
            "md" => "text/markdown",
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            _ => return None,
        },
    )
}

async fn upload(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    let (pool, owner) = require_project(&headers, &state, id).await?;
    let key = request_key(&headers)?;
    let mut file = None;
    let mut role = None;
    let mut kind = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| validation("invalid multipart upload"))?
    {
        match field.name().unwrap_or("") {
            "file" if file.is_none() => {
                let name = field.file_name().unwrap_or("").to_owned();
                if name.is_empty() || name.len() > 1024 || name.contains(['/', '\\', '\r', '\n']) {
                    return Err(validation("invalid file name"));
                }
                let media = file_media_type(&name)
                    .ok_or_else(|| validation("unsupported tender file type"))?;
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|_| validation("could not read upload"))?
                    .to_vec();
                if bytes.is_empty()
                    || bytes.len() > platform::max_file_bytes().saturating_sub(65536)
                {
                    return Err(validation(
                        "file is empty or exceeds the transport size limit",
                    ));
                }
                file = Some((name, media, bytes));
            }
            "role" if role.is_none() => {
                role = Some(
                    field
                        .text()
                        .await
                        .map_err(|_| validation("invalid document role"))?,
                )
            }
            "kind" if kind.is_none() => {
                kind = Some(
                    field
                        .text()
                        .await
                        .map_err(|_| validation("invalid document kind"))?,
                )
            }
            _ => return Err(validation("unknown or duplicate upload field")),
        }
    }
    let (name, media, bytes) = file.ok_or_else(|| validation("one file is required"))?;
    let role = role.unwrap_or_else(|| "main".into());
    let kind = kind.unwrap_or_else(|| "tender".into());
    if !["main", "attachment", "clarification", "amendment"].contains(&role.as_str())
        || !["tender", "enterprise"].contains(&kind.as_str())
    {
        return Err(validation("invalid document role or kind"));
    }
    let mut tx = pool.begin().await.map_err(pg_error)?;
    let project_status: ProjectStatus = sqlx::query_scalar(
        "SELECT status FROM tender_projects WHERE id=$1 AND owner_user_id=$2 FOR UPDATE",
    )
    .bind(id)
    .bind(owner)
    .fetch_optional(&mut *tx)
    .await
    .map_err(pg_error)?
    .ok_or_else(|| not_found("tender project"))?;
    let document = Uuid::new_v5(&id, format!("document:{key}").as_bytes());
    let digest = platform::sha256_hex(&bytes);
    let object = platform::object_ref(&digest);
    let previous: Option<(String, String)> = sqlx::query_as(
        "SELECT content_sha256::text,file_name FROM tender_documents WHERE id=$1 AND project_id=$2",
    )
    .bind(document)
    .bind(id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(pg_error)?;
    if let Some((sha, filename)) = previous {
        if sha != digest || filename != name {
            return Err(fail(
                StatusCode::CONFLICT,
                "IDEMPOTENCY_CONFLICT",
                "同一请求标识已用于其他文件",
            ));
        }
    } else {
        if project_status != ProjectStatus::Open {
            return Err(fail(
                StatusCode::CONFLICT,
                "PROJECT_ENDED",
                "已结束的项目不可修改",
            ));
        }
        let staging = Uuid::new_v4();
        platform::stage_object_upload(
            &pool,
            staging,
            &object,
            &digest,
            media,
            bytes.len() as i64,
            Some(owner),
        )
        .await
        .map_err(pg_error)?;
        platform::write_blob_async(&digest, &bytes)
            .await
            .map_err(|_| {
                fail(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "OBJECT_WRITE_FAILED",
                    "原件保存失败；请使用相同请求标识重试",
                )
            })?;
        platform::publish_object_reference(
            &mut tx,
            Some(staging),
            &object,
            &digest,
            media,
            bytes.len() as i64,
            "bid_document",
            document,
            "source",
            Some(owner),
        )
        .await
        .map_err(pg_error)?;
        sqlx::query("INSERT INTO tender_documents(id,project_id,file_name,media_type,object_ref,content_sha256,byte_length) VALUES($1,$2,$3,$4,$5,$6,$7)")
            .bind(document).bind(id).bind(&name).bind(media).bind(&object).bind(&digest).bind(bytes.len() as i64)
            .execute(&mut *tx).await.map_err(pg_error)?;
    }
    tx.commit().await.map_err(pg_error)?;
    bridge(id,owner,"document.put",json!({"document_id":document,"file_name":name,"role":role,"kind":kind,"sha256":digest,"request_key":key}),bytes).await?;
    let mut value = document_values(&pool, id)
        .await?
        .as_array()
        .unwrap()
        .iter()
        .find(|doc| doc["id"] == json!(document))
        .cloned()
        .ok_or_else(|| not_found("document"))?;
    value["role"] = json!(role);
    value["kind"] = json!(kind);
    Ok((StatusCode::CREATED, Json(value)))
}

async fn download_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, document)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiErr> {
    let (pool, _) = require_project(&headers, &state, id).await?;
    let row = sqlx::query(
        "SELECT content_sha256::text,media_type FROM tender_documents WHERE id=$1 AND project_id=$2",
    )
    .bind(document)
    .bind(id)
    .fetch_optional(&pool)
    .await
    .map_err(pg_error)?
    .ok_or_else(|| not_found("document"))?;
    let digest: String = row.get(0);
    let media: String = row.get(1);
    let bytes = tokio::task::spawn_blocking(move || platform::read_blob(&digest))
        .await
        .map_err(|_| not_found("document bytes"))?
        .map_err(|_| not_found("document bytes"))?;
    Ok((
        [
            (header::CONTENT_TYPE, media),
            (header::CONTENT_DISPOSITION, "attachment".into()),
        ],
        bytes,
    )
        .into_response())
}

async fn workbench(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    let (pool, owner) = require_project(&headers, &state, id).await?;
    let before = crate::tender_editor::workbench_versions(&pool, id).await?;
    let context = before
        .pointer("/current_docx/execution_id")
        .and_then(Value::as_str);
    let extra = context
        .map(|execution| json!({"execution_id":execution}))
        .unwrap_or_else(|| json!({}));
    let (mut value, _) = bridge(id, owner, "workbench", extra, vec![]).await?;
    if value
        .pointer("/artifacts/docx_available")
        .and_then(Value::as_bool)
        == Some(true)
        && let Some(execution) = value
            .pointer("/artifacts/execution_id")
            .and_then(Value::as_str)
    {
        let exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tender_draft_versions WHERE project_id=$1 AND execution_id=$2 AND source='generated')").bind(id).bind(execution).fetch_one(&pool).await.map_err(pg_error)?;
        if !exists {
            let (_, bytes) = bridge(
                id,
                owner,
                "artifact.get",
                json!({"execution_id":execution,"kind":"docx"}),
                vec![],
            )
            .await?;
            crate::tender_editor::import_generated(&pool, id, execution, &bytes, owner).await?;
        }
    }
    let versions = crate::tender_editor::workbench_versions(&pool, id).await?;
    let object = value.as_object_mut().ok_or_else(|| {
        fail(
            StatusCode::BAD_GATEWAY,
            "TENDER_BRIDGE_PROTOCOL",
            "无效工作台结果",
        )
    })?;
    object.insert("project".into(), project_value(&pool, id).await?);
    // Python contributes kind/role, while the shared object catalog owns originals.
    let catalog = document_values(&pool, id).await?;
    if let Some(existing) = object.get("documents").and_then(Value::as_array) {
        let documents: Vec<Value> = catalog
            .as_array()
            .unwrap()
            .iter()
            .map(|doc| {
                let mut row = doc.clone();
                if let Some(meta) = existing
                    .iter()
                    .find(|meta| meta["document_id"] == doc["id"] || meta["id"] == doc["id"])
                {
                    for field in ["kind", "role", "status", "issues"] {
                        if let Some(v) = meta.get(field) {
                            row[field] = v.clone();
                        }
                    }
                }
                row
            })
            .collect();
        object.insert("documents".into(), json!(documents));
    } else {
        object.insert("documents".into(), catalog);
    }
    for field in ["current_docx", "versions"] {
        object.insert(
            field.into(),
            versions.get(field).cloned().unwrap_or(Value::Null),
        );
    }
    if let Some(current) = object
        .get_mut("current_docx")
        .and_then(Value::as_object_mut)
        && let Some(version) = current
            .get("id")
            .or_else(|| current.get("version_id"))
            .and_then(Value::as_str)
            .map(str::to_owned)
    {
        current.insert(
            "download_url".into(),
            json!(format!(
                "/api/v3/tender-projects/{id}/docx/versions/{version}/download"
            )),
        );
    }
    if let Some(versions) = object.get_mut("versions").and_then(Value::as_array_mut) {
        for version in versions {
            if let Some(version_id) = version
                .get("id")
                .or_else(|| version.get("version_id"))
                .and_then(Value::as_str)
                .map(str::to_owned)
            {
                version["download_url"] = json!(format!(
                    "/api/v3/tender-projects/{id}/docx/versions/{version_id}/download"
                ));
            }
        }
    }
    if let Some(execution) = object
        .get("context_execution_id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        && let Some(materials) = object.get_mut("materials").and_then(Value::as_array_mut)
    {
        for material in materials {
            if material.get("original_ref").is_some_and(Value::is_object)
                && let Some(material_id) = material.get("material_id").and_then(Value::as_str)
            {
                material["original_url"] =
                    json!(material_original_url(id, &execution, material_id));
            }
        }
    }
    if let Some(execution) = object
        .get("artifacts")
        .and_then(|a| a.get("execution_id"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        && object
            .get("artifacts")
            .and_then(|a| a.get("package_available"))
            .and_then(Value::as_bool)
            == Some(true)
    {
        object["artifacts"]["package_url"] = json!(format!(
            "/api/v3/tender-projects/{id}/executions/{execution}/package"
        ));
    }
    Ok(Json(value))
}

async fn status(
    state: AppState,
    headers: HeaderMap,
    id: Uuid,
    action: &str,
) -> Result<Json<Value>, ApiErr> {
    let (_, owner) = require_project(&headers, &state, id).await?;
    Ok(Json(bridge(id, owner, action, json!({}), vec![]).await?.0))
}
async fn start(
    state: AppState,
    headers: HeaderMap,
    id: Uuid,
    action: &str,
    mut body: Value,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    let (_, owner) = require_project(&headers, &state, id).await?;
    let object = body
        .as_object_mut()
        .ok_or_else(|| validation("body must be an object"))?;
    object.insert("request_key".into(), json!(request_key(&headers)?));
    Ok((
        StatusCode::ACCEPTED,
        Json(bridge(id, owner, action, body, vec![]).await?.0),
    ))
}
async fn analysis_status(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    status(s, h, id, "analysis.status").await
}
async fn analysis_review(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    status(s, h, id, "analysis.review").await
}
async fn analysis_confirm(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Json(b): Json<Value>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    start(s, h, id, "analysis.confirm", b).await
}
async fn analysis_correct(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Json(b): Json<Value>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    start(s, h, id, "analysis.correct", b).await
}
async fn analysis_recovery(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    status(s, h, id, "analysis.recovery").await
}
async fn analysis_retry(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Json(b): Json<Value>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    start(s, h, id, "analysis.retry", b).await
}
async fn outline_history(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    status(s, h, id, "outline.history").await
}
async fn outline_revision(
    State(s): State<AppState>,
    h: HeaderMap,
    Path((id, revision)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiErr> {
    let (_, owner) = require_project(&h, &s, id).await?;
    Ok(Json(
        bridge(
            id,
            owner,
            "outline.detail",
            json!({"revision_id": revision}),
            vec![],
        )
        .await?
        .0,
    ))
}
async fn authoring_status(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    status(s, h, id, "authoring.status").await
}
async fn outline_status(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiErr> {
    status(s, h, id, "outline.status").await
}
async fn outline_start(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Json(b): Json<Value>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    start(s, h, id, "outline.start", b).await
}
async fn outline_save(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Json(b): Json<Value>,
) -> Result<Json<Value>, ApiErr> {
    Ok(start(s, h, id, "outline.save", b).await?.1)
}
async fn analysis_start(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Json(b): Json<Value>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    start(s, h, id, "analysis.start", b).await
}
async fn authoring_start(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Json(b): Json<Value>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    let (pool, owner) = require_project(&h, &s, id).await?;
    admit_authoring_start(&pool, owner, id, &request_key(&h)?, &b).await?;
    start(s, h, id, "authoring.start", b).await
}

fn document_execution_id(project: Uuid, key: &str) -> String {
    // Same canonical UTF-8 JSON as Python web._request_key, then its job prefix.
    let request = json!({"key": key, "project_id": project});
    let bytes = serde_json::to_vec(&request).expect("string-only request identity is JSON");
    format!("web-document-{}", platform::sha256_hex(&bytes))
}

async fn admit_authoring_start(
    pool: &PgPool,
    owner: Uuid,
    project: Uuid,
    key: &str,
    body: &Value,
) -> Result<(), ApiErr> {
    let existing: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM tender_authoring_jobs WHERE project_id=$1 AND execution_id=$2)",
    )
    .bind(project)
    .bind(document_execution_id(project, key))
    .fetch_one(pool)
    .await
    .map_err(pg_error)?;
    if !existing {
        crate::tender_knowledge::validate_request(pool, owner, body).await?;
    }
    // Existing jobs still pass through Python's immutable request comparison.
    // The worker independently checks current knowledge access when it reads.
    Ok(())
}
async fn resume(
    State(s): State<AppState>,
    h: HeaderMap,
    Path((id, execution)): Path<(Uuid, String)>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    start(
        s,
        h,
        id,
        "authoring.resume",
        json!({"execution_id":execution}),
    )
    .await
}
async fn cancel(
    State(s): State<AppState>,
    h: HeaderMap,
    Path((id, execution)): Path<(Uuid, String)>,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    start(
        s,
        h,
        id,
        "authoring.cancel",
        json!({"execution_id":execution}),
    )
    .await
}
async fn package(
    State(s): State<AppState>,
    h: HeaderMap,
    Path((id, execution)): Path<(Uuid, String)>,
) -> Result<Response, ApiErr> {
    let (_, owner) = require_project(&h, &s, id).await?;
    let (_, bytes) = bridge(
        id,
        owner,
        "artifact.get",
        json!({"execution_id":execution,"kind":"package"}),
        vec![],
    )
    .await?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/zip"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"tender-package.zip\"",
            ),
        ],
        bytes,
    )
        .into_response())
}

async fn material_original(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project, execution, material)): Path<(Uuid, String, String)>,
) -> Result<Response, ApiErr> {
    let (_, owner) = require_project(&headers, &state, project).await?;
    let (metadata, bytes) = bridge(
        project,
        owner,
        "material.original",
        json!({"execution_id":execution,"material_id":material}),
        vec![],
    )
    .await?;
    let media = metadata["media_type"]
        .as_str()
        .unwrap_or("application/octet-stream");
    let name = metadata["file_name"]
        .as_str()
        .unwrap_or("material-original");
    Ok((
        [
            (header::CONTENT_TYPE, media.to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename*=UTF-8''{}", encode_segment(name)),
            ),
        ],
        bytes,
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_patch_rejects_unknown_status() {
        assert!(serde_json::from_value::<PatchProject>(json!({"status":"pending"})).is_err());
        assert_eq!(
            serde_json::from_value::<PatchProject>(json!({"status":"ended"}))
                .unwrap()
                .status,
            Some(ProjectStatus::Ended)
        );
    }
    use axum::body::Body;
    use tower::ServiceExt;

    #[tokio::test]
    async fn tender_routes_authenticate_before_database_or_bridge_access() {
        let state = AppState {
            jwt_secret: "tender-test-secret".into(),
            bootstrap_key: String::new(),
        };
        for path in [
            "/api/v3/tender-projects".to_owned(),
            format!("/api/v3/tender-projects/{}/workbench", Uuid::new_v4()),
            format!("/api/v3/tender-projects/{}/outline", Uuid::new_v4()),
            format!("/api/v3/tender-projects/{}/documents", Uuid::new_v4()),
            material_original_url(Uuid::new_v4(), "execution", "material"),
        ] {
            let response = router()
                .with_state(state.clone())
                .oneshot(
                    axum::http::Request::builder()
                        .uri(path)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }
    }

    #[tokio::test]
    async fn outline_mutations_authenticate_before_bridge_access() {
        let state = AppState {
            jwt_secret: "tender-test-secret".into(),
            bootstrap_key: String::new(),
        };
        for (method, suffix) in [
            ("POST", "outline/generate"),
            ("PUT", "outline"),
            ("GET", "outline/revisions"),
            ("GET", "analysis/review"),
            ("POST", "analysis/confirm"),
            ("POST", "analysis/correct"),
        ] {
            let response = router()
                .with_state(state.clone())
                .oneshot(
                    axum::http::Request::builder()
                        .method(method)
                        .uri(format!(
                            "/api/v3/tender-projects/{}/{suffix}",
                            Uuid::new_v4()
                        ))
                        .header("content-type", "application/json")
                        .body(Body::from("{}"))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }
    }

    #[test]
    fn idempotency_keys_and_upload_names_are_explicit() {
        let mut headers = HeaderMap::new();
        assert!(request_key(&headers).is_err());
        headers.insert("idempotency-key", "repeat-1".parse().unwrap());
        assert_eq!(request_key(&headers).unwrap(), "repeat-1");
        assert_eq!(file_media_type("招标.PDF"), Some("application/pdf"));
        assert!(file_media_type("run.sh").is_none());
    }

    #[test]
    fn material_identifiers_remain_single_url_segments() {
        let project = Uuid::new_v4();
        assert_eq!(
            material_original_url(project, "execution/one", "材料:#?%/1"),
            format!(
                "/api/v3/tender-projects/{project}/executions/execution%2Fone/materials/%E6%9D%90%E6%96%99%3A%23%3F%25%2F1/original"
            )
        );
    }

    #[test]
    #[ignore = "requires the explicitly isolated KB_TENDER_WEB_TEST_DATABASE_URL database"]
    fn project_http_permissions_and_retries_use_the_real_catalog() {
        test_runtime().block_on(async {
            let isolated = std::env::var("KB_TENDER_WEB_TEST_DATABASE_URL")
                .expect("dedicated test database required");
            assert_eq!(std::env::var("DATABASE_URL").unwrap(), isolated);
            assert!(isolated.contains("/knowledgebrain_test_tender_web"));
            let pool = platform::connect().await.unwrap();
            let owner = Uuid::new_v4();
            let outsider = Uuid::new_v4();
            for id in [owner, outsider] {
                sqlx::query("INSERT INTO platform_users(id,email) VALUES($1,$2)")
                    .bind(id)
                    .bind(format!("web-test-{id}@example.invalid"))
                    .execute(&pool)
                    .await
                    .unwrap();
            }
            let state = AppState {
                jwt_secret: "isolated-http-test-secret".into(),
                bootstrap_key: String::new(),
            };
            let owner_token = platform::issue_jwt(owner, &state.jwt_secret).unwrap();
            let outsider_token = platform::issue_jwt(outsider, &state.jwt_secret).unwrap();
            let app = crate::router_with(state);
            let key = Uuid::new_v4().to_string();
            async fn send(
                app: Router,
                method: &str,
                path: &str,
                token: &str,
                key: &str,
                body: Value,
            ) -> (StatusCode, Value) {
                let response = app
                    .oneshot(
                        axum::http::Request::builder()
                            .method(method)
                            .uri(path)
                            .header("authorization", format!("Bearer {token}"))
                            .header("idempotency-key", key)
                            .header("content-type", "application/json")
                            .body(Body::from(serde_json::to_vec(&body).unwrap()))
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                let status = response.status();
                let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
                    .await
                    .unwrap();
                (status, serde_json::from_slice(&bytes).unwrap())
            }
            let (code, created) = send(
                app.clone(),
                "POST",
                "/api/v3/tender-projects",
                &owner_token,
                &key,
                json!({"title":"HTTP真实项目"}),
            )
            .await;
            assert_eq!(code, StatusCode::CREATED, "{created}");
            let (_, replayed) = send(
                app.clone(),
                "POST",
                "/api/v3/tender-projects",
                &owner_token,
                &key,
                json!({"title":"HTTP真实项目"}),
            )
            .await;
            assert_eq!(created["id"], replayed["id"]);
            let (code, _) = send(
                app.clone(),
                "POST",
                "/api/v3/tender-projects",
                &owner_token,
                &key,
                json!({"title":"其他名称"}),
            )
            .await;
            assert_eq!(code, StatusCode::CONFLICT);
            let path = format!(
                "/api/v3/tender-projects/{}",
                created["id"].as_str().unwrap()
            );
            let (code, _) = send(app.clone(), "GET", &path, &outsider_token, &key, json!({})).await;
            assert_eq!(code, StatusCode::NOT_FOUND);
            let (code, list) = send(
                app.clone(),
                "GET",
                "/api/v3/tender-projects",
                &outsider_token,
                &key,
                json!({}),
            )
            .await;
            assert_eq!(code, StatusCode::OK);
            assert!(
                !list
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|p| p["id"] == created["id"])
            );
            let (code, ended) = send(
                app.clone(),
                "PATCH",
                &path,
                &owner_token,
                &key,
                json!({"status":"ended"}),
            )
            .await;
            assert_eq!(code, StatusCode::OK);
            assert_eq!(ended["status"], "ended");
            assert!(!ended["ended_at"].is_null());
            let before: i64 = sqlx::query_scalar("SELECT count(*) FROM platform_object_uploads")
                .fetch_one(&pool).await.unwrap();
            let body = "--fixture\r\nContent-Disposition: form-data; name=\"file\"; filename=\"source.txt\"\r\nContent-Type: text/plain\r\n\r\nsource text\r\n--fixture--\r\n";
            let rejected = app.clone().oneshot(axum::http::Request::builder()
                .method("POST").uri(format!("{path}/documents"))
                .header("authorization", format!("Bearer {owner_token}"))
                .header("idempotency-key", "ended-upload")
                .header("content-type", "multipart/form-data; boundary=fixture")
                .body(Body::from(body)).unwrap()).await.unwrap();
            assert_eq!(rejected.status(), StatusCode::CONFLICT);
            let after: i64 = sqlx::query_scalar("SELECT count(*) FROM platform_object_uploads")
                .fetch_one(&pool).await.unwrap();
            assert_eq!(before, after, "ended projects must reject before object staging");
            let documents: i64 = sqlx::query_scalar("SELECT count(*) FROM tender_documents WHERE project_id=$1")
                .bind(Uuid::parse_str(created["id"].as_str().unwrap()).unwrap())
                .fetch_one(&pool).await.unwrap();
            assert_eq!(documents, 0);
            let (code, opened) = send(
                app,
                "PATCH",
                &path,
                &owner_token,
                &key,
                json!({"status":"open"}),
            )
            .await;
            assert_eq!(code, StatusCode::OK);
            assert!(opened["ended_at"].is_null());
            let count: i64 =
                sqlx::query_scalar("SELECT count(*) FROM tender_projects WHERE owner_user_id=$1")
                    .bind(owner)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(count, 1);
        });
    }

    #[test]
    #[ignore = "requires isolated PostgreSQL, authenticated bridge fixture and task-owned OBJECT_DIR"]
    fn upload_and_workbench_cross_the_authenticated_python_bridge() {
        test_runtime().block_on(async {
        let isolated = std::env::var("KB_TENDER_WEB_TEST_DATABASE_URL").unwrap();
        assert_eq!(std::env::var("DATABASE_URL").unwrap(), isolated);
        assert!(isolated.contains("/knowledgebrain_test_tender_web"));
        assert!(
            std::env::var("DOCREADER_ADDR")
                .unwrap()
                .starts_with("127.0.0.1:")
        );
        let object_root = std::env::var("OBJECT_DIR").unwrap();
        assert!(object_root.contains("kb-tender-evidence/2026-10-27-web-integration")
            || object_root.contains("kb-tender-evidence/2026-10-28-outline-workbench/backend-bridge")
            || object_root.contains("kb-tender-evidence/2026-10-28-remaining-fixes/backend-bridge")
            || object_root.contains("kb-tender-evidence/2026-10-28-ui-edge-fixes/rust-local-objects"));
        assert!(
            !platform::s3_configured(),
            "fixture uses a task-owned local object directory"
        );
        let pool = platform::connect().await.unwrap();
        let owner = Uuid::new_v4();
        let project = Uuid::new_v4();
        sqlx::query("INSERT INTO platform_users(id,email) VALUES($1,$2)")
            .bind(owner)
            .bind(format!("bridge-test-{owner}@example.invalid"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO tender_projects(id,owner_user_id,title,status) VALUES($1,$2,'桥接验证','open')").bind(project).bind(owner).execute(&pool).await.unwrap();
        let state = AppState {
            jwt_secret: "isolated-bridge-http-test".into(),
            bootstrap_key: String::new(),
        };
        let token = platform::issue_jwt(owner, &state.jwt_secret).unwrap();
        let app = crate::router_with(state);
        let key = Uuid::new_v4().to_string();
        let content = "企业资料仅为来源，不代表满足招标要求。";
        let path = format!("/api/v3/tender-projects/{project}");
        async fn upload_request(
            app: Router,
            path: &str,
            token: &str,
            key: &str,
            content: &str,
        ) -> (StatusCode, Value) {
            let body = format!(
                "--tender-fixture\r\nContent-Disposition: form-data; name=\"kind\"\r\n\r\nenterprise\r\n--tender-fixture\r\nContent-Disposition: form-data; name=\"file\"; filename=\"source.txt\"\r\nContent-Type: text/plain\r\n\r\n{content}\r\n--tender-fixture--\r\n"
            );
            let response = app
                .oneshot(
                    axum::http::Request::builder()
                        .method("POST")
                        .uri(format!("{path}/documents"))
                        .header("authorization", format!("Bearer {token}"))
                        .header("idempotency-key", key)
                        .header(
                            "content-type",
                            "multipart/form-data; boundary=tender-fixture",
                        )
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = response.status();
            let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
                .await
                .unwrap();
            (status, serde_json::from_slice(&bytes).unwrap())
        }
        let (code, doc) = upload_request(app.clone(), &path, &token, &key, content).await;
        assert_eq!(code, StatusCode::CREATED, "{doc}");
        assert_eq!(doc["kind"], "enterprise");
        let (code, replay) = upload_request(app.clone(), &path, &token, &key, content).await;
        assert_eq!(code, StatusCode::CREATED, "{replay}");
        assert_eq!(replay["id"], doc["id"]);
        assert_eq!(
            upload_request(app.clone(), &path, &token, &key, "different source")
                .await
                .0,
            StatusCode::CONFLICT
        );
        sqlx::query("UPDATE tender_projects SET status='ended',ended_at=now() WHERE id=$1")
            .bind(project).execute(&pool).await.unwrap();
        let (code, ended_replay) = upload_request(app.clone(), &path, &token, &key, content).await;
        assert_eq!(code, StatusCode::CREATED, "{ended_replay}");
        assert_eq!(ended_replay["id"], doc["id"]);
        assert_eq!(upload_request(app.clone(), &path, &token, &key, "changed after ending").await.0,
                   StatusCode::CONFLICT);
        let (code, denied) = upload_request(app.clone(), &path, &token, "new-after-ending", content).await;
        assert_eq!(code, StatusCode::CONFLICT, "{denied}");
        assert_eq!(denied["error"]["code"], "PROJECT_ENDED");
        sqlx::query("UPDATE tender_projects SET status='open',ended_at=NULL WHERE id=$1")
            .bind(project).execute(&pool).await.unwrap();
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri(format!("{path}/workbench"))
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let value: Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 1024 * 1024)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(status, StatusCode::OK, "{value}");
        assert_eq!(value["project"]["id"], json!(project));
        assert_eq!(value["documents"][0]["kind"], "enterprise");
        assert!(value["analysis"].is_null());
        assert!(value["current_docx"].is_null());
        assert!(value["versions"].as_array().unwrap().is_empty());
        for (method, suffix, body, expected, expected_code) in [
            ("GET", "outline", None, StatusCode::OK, None),
            ("GET", "outline/revisions", None, StatusCode::OK, None),
            ("GET", "outline/revisions/00000000-0000-4000-8000-000000000000", None, StatusCode::NOT_FOUND, Some("OUTLINE_REVISION_NOT_FOUND")),
            ("GET", "analysis/review", None, StatusCode::OK, None),
            ("POST", "analysis/confirm", Some(json!({"run_id":"missing-analysis"})), StatusCode::NOT_FOUND, Some("ANALYSIS_NOT_FOUND")),
            ("POST", "analysis/correct", Some(json!({"run_id":"missing-analysis"})), StatusCode::NOT_FOUND, Some("ANALYSIS_NOT_FOUND")),
            ("POST", "outline/generate", Some(json!({})), StatusCode::CONFLICT, Some("TENDER_DOCUMENT_REQUIRED")),
            ("PUT", "outline", Some(json!({"expected_revision_id":null,"source_execution_id":"missing-outline","chapters":[]})), StatusCode::NOT_FOUND, Some("EXECUTION_NOT_FOUND")),
        ] {
            let response = app.clone().oneshot(axum::http::Request::builder()
                .method(method).uri(format!("{path}/{suffix}"))
                .header("authorization",format!("Bearer {token}"))
                .header("idempotency-key",format!("outline-http-{method}"))
                .header("content-type","application/json")
                .body(Body::from(body.map(|value|value.to_string()).unwrap_or_default())).unwrap()).await.unwrap();
            let status = response.status();
            let value: Value = serde_json::from_slice(&axum::body::to_bytes(response.into_body(),1024*1024).await.unwrap()).unwrap();
            assert_eq!(status,expected,"{value}");
            if let Some(code) = expected_code {
                assert_eq!(value["error"]["code"],code,"{value}");
            } else {
                let expected = match suffix {
                    "outline/revisions" => json!({"revisions":[]}),
                    "analysis/review" => json!({"review":null}),
                    _ => json!({"draft":null,"candidate":null,"generation":null}),
                };
                assert_eq!(value, expected);
            }
        }
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri(doc["download_url"].as_str().unwrap())
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            &axum::body::to_bytes(response.into_body(), 1024 * 1024)
                .await
                .unwrap()[..],
            content.as_bytes()
        );
        let documents: i64 =
            sqlx::query_scalar("SELECT count(*) FROM tender_documents WHERE project_id=$1")
                .bind(project)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(documents, 1);
        let models:i64=sqlx::query_scalar("SELECT count(*) FROM tender_analysis_model_requests WHERE run_id IN (SELECT run_id FROM tender_project_analyses WHERE project_id=$1)").bind(project).fetch_one(&pool).await.unwrap();
        assert_eq!(models, 0);

        // Seed only immutable synthetic receipts; this exercises real material
        // authorization/bytes across HTTP and gRPC without invoking a model.
        let root = std::env::var("KB_TENDER_WEB_BRIDGE_ARTIFACT_ROOT").unwrap();
        assert!(root.contains("kb-tender-evidence/2026-10-27-web-integration/")
            || root.contains("kb-tender-evidence/2026-10-28-outline-workbench/backend-bridge/")
            || root.contains("kb-tender-evidence/2026-10-28-remaining-fixes/backend-bridge/")
            || root.contains("kb-tender-evidence/2026-10-28-ui-edge-fixes/bridge-http/"));
        let original: Value = sqlx::query_scalar("SELECT original_ref FROM tender_document_sources WHERE document_id=$1")
            .bind(Uuid::parse_str(doc["id"].as_str().unwrap()).unwrap()).fetch_one(&pool).await.unwrap();
        let material_id = "资料/材料:#?%";
        let put_json = |value: Value| {
            let bytes = serde_json::to_vec(&value).unwrap();
            let digest = platform::sha256_hex(&bytes);
            let relative = format!("{}/{digest}", &digest[..2]);
            let file = std::path::Path::new(&root).join(&relative);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            if !file.exists() { std::fs::write(&file, &bytes).unwrap(); }
            assert_eq!(std::fs::read(file).unwrap(), bytes);
            json!({"sha256":digest,"relative_path":relative,"size":bytes.len(),"media_type":"application/json"})
        };
        let basis = put_json(json!({"materials":[
            {"material_id":material_id,"original_ref":original,"locator":{"file_name":"企业资料.txt"}},
            {"material_id":"text-only","text":"只有文字"}
        ]}));
        let result = put_json(json!({"basis_ref":basis}));
        let run = format!("http-material-{}", Uuid::new_v4());
        let execution = format!("material-execution-{}", Uuid::new_v4());
        sqlx::query("INSERT INTO tender_analysis_runs(run_id,request_key) VALUES($1,$1)").bind(&run).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO tender_project_analyses(run_id,project_id,request_key) VALUES($1,$2,$1)").bind(&run).bind(project).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO tender_authoring_executions(execution_id,authoring_input,execution_spec,status,result_ref) VALUES($1,'{}','{}','succeeded',$2)")
            .bind(&execution).bind(result).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO tender_authoring_jobs(execution_id,project_id,run_id,payload,status) VALUES($1,$2,$3,'{}','succeeded')")
            .bind(&execution).bind(project).bind(&run).execute(&pool).await.unwrap();
        for (material, status) in [(material_id,StatusCode::OK),("unknown",StatusCode::NOT_FOUND),("text-only",StatusCode::CONFLICT)] {
            let response=app.clone().oneshot(axum::http::Request::builder()
                .uri(material_original_url(project,&execution,material))
                .header("authorization",format!("Bearer {token}"))
                .body(Body::empty()).unwrap()).await.unwrap();
            assert_eq!(response.status(),status);
            if status==StatusCode::OK {
                assert_eq!(response.headers()[header::CONTENT_TYPE],"text/plain");
                assert!(response.headers()[header::CONTENT_DISPOSITION].to_str().unwrap().contains("filename*=UTF-8''"));
                assert_eq!(&axum::body::to_bytes(response.into_body(),1024*1024).await.unwrap()[..],content.as_bytes());
            }
        }
        let outline_execution = format!("outline-execution-{}",Uuid::new_v4());
        let outline_input = json!({"schema_version":2,"run_id":run,"source_version":1,"domain_revision":1});
        let outline_ref = put_json(json!({"schema_version":2,"authoring_input":outline_input,"issues":[],
            "chapters":[{"chapter_id":"chapter-1","title":"原始方案","targets":[],"children":[]}]}));
        sqlx::query("INSERT INTO tender_authoring_executions(execution_id,authoring_input,execution_spec,status,result_ref) VALUES($1,$2,'{}','succeeded',$3)")
            .bind(&outline_execution).bind(&outline_input).bind(outline_ref).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO tender_authoring_jobs(execution_id,project_id,run_id,payload,status) VALUES($1,$2,$3,$4,'succeeded')")
            .bind(&outline_execution).bind(project).bind(&run).bind(json!({"operation":"outline"})).execute(&pool).await.unwrap();
        let edit = json!({"expected_revision_id":null,"source_execution_id":outline_execution,
            "chapters":[{"chapter_id":"chapter-1","title":"人工编排方案","children":[]}]});
        let mut different_edit=edit.clone();
        different_edit["chapters"]=json!([]);
        let mut revision = None;
        for (key,body,expected,expected_code) in [
            ("save-outline",edit.clone(),StatusCode::OK,None),
            ("save-outline",edit.clone(),StatusCode::OK,None),
            ("save-outline",different_edit,StatusCode::CONFLICT,Some("REQUEST_CONFLICT")),
            ("save-stale-outline",edit,StatusCode::CONFLICT,Some("OUTLINE_REVISION_CONFLICT")),
        ] {
            let response=app.clone().oneshot(axum::http::Request::builder().method("PUT")
                .uri(format!("{path}/outline")).header("authorization",format!("Bearer {token}"))
                .header("idempotency-key",key).header("content-type","application/json")
                .body(Body::from(body.to_string())).unwrap()).await.unwrap();
            let status=response.status();
            let value:Value=serde_json::from_slice(&axum::body::to_bytes(response.into_body(),1024*1024).await.unwrap()).unwrap();
            assert_eq!(status,expected,"{value}");
            if let Some(code)=expected_code {
                assert_eq!(value["error"]["code"],code,"{value}");
            } else {
                assert_eq!(value["draft"]["outline"]["chapters"][0]["title"],"人工编排方案");
                if let Some(previous)=&revision {assert_eq!(&value["draft"]["revision_id"],previous);}
                revision=Some(value["draft"]["revision_id"].clone());
            }
        }
        });
    }
}
