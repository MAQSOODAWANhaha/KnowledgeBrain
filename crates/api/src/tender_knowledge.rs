//! Narrow worker knowledge reads bound to a persisted project job and fixed versions.

use crate::AppState;
use crate::err::{fail, not_found, unauthorized, validation};
use crate::tender_routes::{ApiErr, pg_error};
use axum::extract::Path;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use uuid::Uuid;

const PURPOSE: &str = "tender_knowledge_v1";
const MAX_ORIGINAL_BYTES: i64 = 50 * 1024 * 1024;

#[cfg(test)]
#[path = "tender_knowledge_tests.rs"]
mod postgres_tests;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
struct Selection {
    product_id: Uuid,
    version_id: Uuid,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerClaims {
    purpose: String,
    execution_id: String,
    exp: usize,
}

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    Search {
        product_id: Uuid,
        version_id: Uuid,
        query: String,
    },
    Document {
        product_id: Uuid,
        version_id: Uuid,
        document_id: Uuid,
    },
    File {
        product_id: Uuid,
        version_id: Uuid,
        key: String,
    },
}

impl Command {
    fn selection(&self) -> Selection {
        let (product_id, version_id) = match self {
            Self::Search {
                product_id,
                version_id,
                ..
            }
            | Self::Document {
                product_id,
                version_id,
                ..
            }
            | Self::File {
                product_id,
                version_id,
                ..
            } => (*product_id, *version_id),
        };
        Selection {
            product_id,
            version_id,
        }
    }
}

pub(crate) fn router() -> Router<AppState> {
    Router::new().route("/api/internal/tender-knowledge/{execution_id}", post(read))
}

fn verify_token(token: &str, secret: &str, execution_id: &str) -> Result<(), ApiErr> {
    if secret.trim().is_empty() {
        return Err(unauthorized());
    }
    let mut validation = Validation::new(Algorithm::HS256);
    validation.leeway = 0;
    let claims = decode::<WorkerClaims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map_err(|_| unauthorized())?
    .claims;
    let now = chrono::Utc::now().timestamp().max(0) as usize;
    if claims.purpose != PURPOSE
        || claims.execution_id != execution_id
        || claims.exp <= now
        || claims.exp > now + 90
    {
        return Err(unauthorized());
    }
    Ok(())
}

fn selections(value: &Value) -> Result<Vec<Selection>, ApiErr> {
    let Some(values) = value.get("knowledge_selections") else {
        return Ok(Vec::new());
    };
    let Some(values) = values.as_array().filter(|values| values.len() <= 20) else {
        return Err(validation("最多选择20个明确的知识资产版本。"));
    };
    let mut selected = values
        .iter()
        .cloned()
        .map(serde_json::from_value)
        .collect::<Result<Vec<Selection>, _>>()
        .map_err(|_| validation("知识资产仅接受产品与固定版本标识，不能使用current。"))?;
    selected.sort();
    selected.dedup();
    Ok(selected)
}

fn require_enabled() -> Result<(), ApiErr> {
    if !platform::knowledge_enabled().map_err(|_| {
        fail(
            StatusCode::SERVICE_UNAVAILABLE,
            "KNOWLEDGE_UNAVAILABLE",
            "知识资产配置暂不可用；可取消选择并使用上传资料继续编制。",
        )
    })? {
        return Err(fail(
            StatusCode::CONFLICT,
            "KNOWLEDGE_DISABLED",
            "知识资产尚未启用；可取消选择并使用上传资料继续编制。",
        ));
    }
    Ok(())
}

async fn require_selection(
    pool: &PgPool,
    owner: Uuid,
    selection: &Selection,
) -> Result<(), ApiErr> {
    let owned: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM knowledge_products p JOIN knowledge_workspaces w ON w.id=p.workspace_id
         JOIN knowledge_product_versions v ON v.product_id=p.id
         WHERE p.id=$1 AND v.id=$2 AND w.owner_user_id=$3 AND w.deleted_at IS NULL
           AND v.deleted_at IS NULL AND v.status='active')")
        .bind(selection.product_id).bind(selection.version_id).bind(owner)
        .fetch_one(pool).await.map_err(pg_error)?;
    if !owned {
        return Err(not_found("知识资产版本不存在、不可用或无权访问。"));
    }
    Ok(())
}

/// Admission and worker reads use the same ownership/version checks.
pub(crate) async fn validate_request(
    pool: &PgPool,
    owner: Uuid,
    request: &Value,
) -> Result<(), ApiErr> {
    let selected = selections(request)?;
    if selected.is_empty() {
        return Ok(());
    }
    require_enabled()?;
    for selection in selected {
        require_selection(pool, owner, &selection).await?;
    }
    Ok(())
}

async fn job_scope(pool: &PgPool, execution_id: &str) -> Result<(Uuid, Vec<Selection>), ApiErr> {
    let row = sqlx::query(
        "SELECT p.owner_user_id,j.payload FROM tender_authoring_jobs j
         JOIN tender_projects p ON p.id=j.project_id
         WHERE j.execution_id=$1 AND j.status<>'cancelled'",
    )
    .bind(execution_id)
    .fetch_optional(pool)
    .await
    .map_err(pg_error)?
    .ok_or_else(|| not_found("knowledge execution"))?;
    let owner: Uuid = row.try_get("owner_user_id").map_err(pg_error)?;
    let payload: Value = row.try_get("payload").map_err(pg_error)?;
    let recorded_owner = payload
        .get("owner_user_id")
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok());
    if recorded_owner != Some(owner) {
        return Err(not_found("knowledge execution"));
    }
    Ok((owner, selections(&payload)?))
}

async fn require_document(
    pool: &PgPool,
    selection: &Selection,
    id: Uuid,
) -> Result<knowledge::Document, ApiErr> {
    let available: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM knowledge_documents WHERE id=$1 AND product_version_id=$2
           AND deleted_at IS NULL AND enable_status='enabled' AND index_ready
           AND parse_status NOT IN ('deleting','deleted','cancelled'))",
    )
    .bind(id)
    .bind(selection.version_id)
    .fetch_one(pool)
    .await
    .map_err(pg_error)?;
    if !available {
        return Err(not_found("knowledge document"));
    }
    knowledge::load_document(pool, id)
        .await
        .map_err(pg_error)?
        .ok_or_else(|| not_found("knowledge document"))
}

async fn run_command(
    pool: &PgPool,
    execution_id: &str,
    command: Command,
) -> Result<Response, ApiErr> {
    let (owner, selected) = job_scope(pool, execution_id).await?;
    let selection = command.selection();
    if !selected.contains(&selection) {
        return Err(not_found("selected knowledge version"));
    }
    require_selection(pool, owner, &selection).await?;
    match command {
        Command::Search { query, .. } => {
            if query.trim().is_empty() || query.len() > 20000 {
                return Err(validation("知识检索内容须为非空且不超过20000字节。"));
            }
            let request: knowledge::search::SearchRequest = serde_json::from_value(json!({
                "mode":"assembly", "product_id":selection.product_id, "version_id":selection.version_id,
                "query":query, "match_count":8, "include_library":false, "expand_wiki":false, "expand_graph":false
            })).map_err(|_| validation("invalid knowledge search"))?;
            let result = knowledge::search::assembly_pg(pool, &request, owner)
                .await
                .map_err(|_| {
                    fail(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "KNOWLEDGE_UNAVAILABLE",
                        "知识检索暂不可用，相关资料保留待补。",
                    )
                })?;
            Ok(Json(result).into_response())
        }
        Command::Document { document_id, .. } => {
            let document = require_document(pool, &selection, document_id).await?;
            Ok(Json(
                json!({"id":document.id, "product_version_id":document.product_version_id,
                "file_name":document.file_name, "object_ref":document.object_ref}),
            )
            .into_response())
        }
        Command::File { key, .. } => {
            let Some(hash) = key.strip_prefix("objects/").filter(|hash| {
                hash.len() == 64
                    && hash
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            }) else {
                return Err(not_found("knowledge file"));
            };
            let document: Option<Uuid> = sqlx::query_scalar(
                "SELECT id FROM knowledge_documents WHERE product_version_id=$1 AND object_ref=$2
                 AND deleted_at IS NULL AND enable_status='enabled' AND index_ready
                 AND parse_status NOT IN ('deleting','deleted','cancelled') AND byte_length<=$3 LIMIT 1")
                .bind(selection.version_id).bind(&key).bind(MAX_ORIGINAL_BYTES)
                .fetch_optional(pool).await.map_err(pg_error)?;
            let document = document.ok_or_else(|| not_found("knowledge file"))?;
            require_document(pool, &selection, document).await?;
            let hash = hash.to_owned();
            let content = tokio::task::spawn_blocking(move || platform::read_blob(&hash))
                .await
                .map_err(|_| not_found("knowledge file"))?
                .map_err(|_| not_found("knowledge file"))?;
            if content.len() > MAX_ORIGINAL_BYTES as usize {
                return Err(not_found("knowledge file"));
            }
            Ok(content.into_response())
        }
    }
}

async fn read(
    Path(execution_id): Path<String>,
    headers: HeaderMap,
    Json(command): Json<Command>,
) -> Result<Response, ApiErr> {
    let token = headers
        .get("authorization")
        .and_then(|header| header.to_str().ok())
        .and_then(|header| header.strip_prefix("Bearer "))
        .ok_or_else(unauthorized)?;
    let secret = platform::required_secret("GRPC_AUTH_TOKEN").map_err(|_| unauthorized())?;
    verify_token(token, &secret, &execution_id)?;
    require_enabled()?;
    let pool = platform::connect().await.map_err(pg_error)?;
    run_command(&pool, &execution_id, command).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{EncodingKey, Header, encode};

    fn token(changes: Value, secret: &str) -> String {
        let mut claims = json!({"purpose":PURPOSE,"execution_id":"job",
            "exp":chrono::Utc::now().timestamp()+60});
        claims
            .as_object_mut()
            .unwrap()
            .extend(changes.as_object().unwrap().clone());
        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap()
    }

    #[test]
    fn worker_token_is_short_lived_signed_and_bound_to_exact_job_and_purpose() {
        let secret = "synthetic-worker-secret";
        assert!(verify_token(&token(json!({}), secret), secret, "job").is_ok());
        assert!(platform::parse_jwt(&token(json!({}), secret), secret).is_err());
        assert!(
            verify_token(
                &platform::issue_jwt(Uuid::new_v4(), secret).unwrap(),
                secret,
                "job"
            )
            .is_err()
        );
        for (value, key, job) in [
            (token(json!({}), secret), "wrong-secret", "job"),
            (token(json!({}), secret), secret, "other-job"),
            (token(json!({"purpose":"other"}), secret), secret, "job"),
            (
                token(json!({"exp":chrono::Utc::now().timestamp()-1}), secret),
                secret,
                "job",
            ),
            (
                token(json!({"exp":chrono::Utc::now().timestamp()+3600}), secret),
                secret,
                "job",
            ),
        ] {
            assert_eq!(
                verify_token(&value, key, job).unwrap_err().0,
                StatusCode::UNAUTHORIZED
            );
        }
    }

    #[test]
    fn only_explicit_bounded_selections_and_narrow_commands_are_accepted() {
        let product = Uuid::new_v4();
        let version = Uuid::new_v4();
        let selection = json!({"product_id":product,"version_id":version});
        assert_eq!(
            selections(&json!({"knowledge_selections":[selection,selection]}))
                .unwrap()
                .len(),
            1
        );
        assert!(
            selections(
                &json!({"knowledge_selections":[{"product_id":product,"version_id":"current"}]})
            )
            .is_err()
        );
        assert!(selections(&json!({"knowledge_selections":[{"product_id":product,"version_id":version,"bearer":"bad"}]})).is_err());
        assert!(
            serde_json::from_value::<Command>(json!({"operation":"search","product_id":product,
            "version_id":version,"query":"query","include_library":true}))
            .is_err()
        );
    }
}
