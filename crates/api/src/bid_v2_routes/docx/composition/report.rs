use super::*;
use bidding::docx_composition::compiler::Manifest;
use bidding::docx_composition::postgres;

#[derive(Deserialize)]
struct Identity {
    version_id: Uuid,
    docx_sha256: String,
    object_ref: String,
    sha256: String,
    byte_length: u64,
}

fn integrity_failed() -> ApiErr {
    fail(
        StatusCode::UNPROCESSABLE_ENTITY,
        "DOCX_COMPOSITION_REPORT_INTEGRITY_FAILED",
        "Composition report does not match the stored version",
    )
}

fn identity(version: Uuid, metadata: Value) -> Result<Identity, ApiErr> {
    let identity: Identity = serde_json::from_value(metadata).map_err(|_| integrity_failed())?;
    if identity.version_id != version
        || identity.object_ref != platform::object_ref(&identity.sha256)
    {
        return Err(integrity_failed());
    }
    Ok(identity)
}

fn response(identity: Identity, bytes: Vec<u8>) -> Result<Response, ApiErr> {
    if bytes.len() as u64 != identity.byte_length || platform::sha256_hex(&bytes) != identity.sha256
    {
        return Err(integrity_failed());
    }
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|_| integrity_failed())?;
    if manifest.docx_sha256 != identity.docx_sha256 {
        return Err(integrity_failed());
    }
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .header(
            "content-disposition",
            format!(
                "attachment; filename=\"composition-report-{}.json\"",
                identity.version_id
            ),
        )
        .header("cache-control", "private, no-store")
        .header("x-content-type-options", "nosniff")
        .header("etag", format!("\"{}\"", identity.sha256))
        .body(Body::from(bytes))
        .map_err(|_| {
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "RESPONSE_BUILD_FAILED",
                "Composition report download response failed",
            )
        })
}

pub(super) async fn download(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace, version)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    // The owner-scoped SQL also checks this exact workspace/version pair.
    // Its original SQLSTATE must reach map_docx_sql before any object I/O.
    let metadata = postgres::get_manifest_identity(&pool, workspace, version, &actor)
        .await
        .map_err(map_docx_sql)?
        .ok_or_else(|| {
            fail(
                StatusCode::NOT_FOUND,
                "DOCX_COMPOSITION_REPORT_NOT_FOUND",
                "This document version has no composition report",
            )
        })?;
    let identity = identity(version, metadata)?;
    tokio::task::spawn_blocking(move || {
        let bytes = platform::read_blob(&identity.sha256).map_err(|error| {
            tracing::error!(%error, "Composition report object read failed");
            fail(
                StatusCode::SERVICE_UNAVAILABLE,
                "DOCX_COMPOSITION_REPORT_UNAVAILABLE",
                "Composition report file is unavailable",
            )
        })?;
        response(identity, bytes)
    })
    .await
    .map_err(|_| {
        fail(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DOCX_COMPOSITION_REPORT_READ_FAILED",
            "Composition report read task failed",
        )
    })?
}

#[cfg(test)]
mod tests;
