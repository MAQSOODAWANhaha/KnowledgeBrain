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
