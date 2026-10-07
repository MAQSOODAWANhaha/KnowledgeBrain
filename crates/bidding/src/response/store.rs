//! Publish response data for one frozen outline.

use super::ResponseSet;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

pub async fn publish(
    pool: &PgPool,
    project_id: Uuid,
    response: &ResponseSet,
) -> Result<Value, sqlx::Error> {
    let bytes = serde_json_canonicalizer::to_vec(response)
        .map_err(|error| sqlx::Error::Protocol(format!("response canonical JSON: {error}")))?;
    sqlx::query_scalar(
        "SELECT kb_bid_v2_publish_response($1,$2::kb_sha256,$3,NULL::kb_actor_identity)",
    )
    .bind(project_id)
    .bind(&response.outline_sha256)
    .bind(bytes)
    .fetch_one(pool)
    .await
}
