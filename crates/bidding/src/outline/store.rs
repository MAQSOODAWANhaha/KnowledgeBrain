//! Publish a frozen outline into the bidding baseline.

use super::tools::Draft;
use super::{OutlineArtifact, canonical_sha256, project_draft};
use crate::analysis::FrozenInput;
use crate::outline::chapters::AttachmentBinding;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

/// Project a finished tool draft and write that artifact. Unfinished drafts are not written.
pub async fn publish_finished(
    pool: &PgPool,
    project_id: Uuid,
    run_id: Uuid,
    input: &FrozenInput,
    frozen_input_sha256: &str,
    draft: &Draft,
) -> Result<Value, String> {
    let projected = project_draft(input, frozen_input_sha256, draft)?;
    publish(
        pool,
        project_id,
        run_id,
        &projected.artifact,
        &projected.bindings,
    )
    .await
    .map_err(|error| error.to_string())
}

pub async fn publish(
    pool: &PgPool,
    project_id: Uuid,
    run_id: Uuid,
    artifact: &OutlineArtifact,
    bindings: &[AttachmentBinding],
) -> Result<Value, sqlx::Error> {
    let bytes = canonical_sha256_bytes(artifact)?;
    let binding_json = json!(
        bindings
            .iter()
            .map(|binding| json!({
                "form_id": binding.form_id,
                "chapter_id": binding.chapter_id,
            }))
            .collect::<Vec<_>>()
    );
    sqlx::query_scalar(
        "SELECT kb_bid_v2_publish_outline($1,$2,$3::kb_sha256,$4,$5,NULL::kb_actor_identity)",
    )
    .bind(run_id)
    .bind(project_id)
    .bind(&artifact.frozen_input_sha256)
    .bind(bytes)
    .bind(binding_json)
    .fetch_one(pool)
    .await
}

fn canonical_sha256_bytes(artifact: &OutlineArtifact) -> Result<Vec<u8>, sqlx::Error> {
    serde_json_canonicalizer::to_vec(artifact)
        .map_err(|error| sqlx::Error::Protocol(format!("outline canonical JSON: {error}")))
}

pub fn outline_sha256(artifact: &OutlineArtifact) -> Result<String, String> {
    canonical_sha256(artifact)
}

/// B5: register a frozen input so `kb_bid_v2_publish_outline` accepts its SHA.
/// Workers call this when they freeze the tender input, before publishing.
/// Idempotent: re-registering the same SHA is a no-op.
pub async fn register_frozen_input(
    pool: &PgPool,
    project_id: Uuid,
    input_sha256: &str,
    document_set_id: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO bid_frozen_inputs (input_sha256, project_id, document_set_id)
         VALUES ($1::kb_sha256, $2, $3)
         ON CONFLICT (input_sha256) DO NOTHING",
    )
    .bind(input_sha256)
    .bind(project_id)
    .bind(document_set_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Load the most recently published outline artifact for a project.
///
/// Used by `bid:content_generate:v2`: response generation always builds on the
/// latest published outline. Returns the full [`OutlineArtifact`] (deserialized
/// from the stored canonical JSON), not just the digest.
pub async fn load_published(pool: &PgPool, project_id: Uuid) -> Result<OutlineArtifact, String> {
    let artifact: Option<Value> = sqlx::query_scalar(
        "SELECT artifact FROM bid_outline_artifacts WHERE project_id = $1 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| format!("load published outline: {error}"))?;
    let value = artifact.ok_or_else(|| "no published outline for project".to_string())?;
    serde_json::from_value::<OutlineArtifact>(value)
        .map_err(|error| format!("published outline decode: {error}"))
}
