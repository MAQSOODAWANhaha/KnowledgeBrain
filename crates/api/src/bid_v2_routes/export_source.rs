//! Capability-scoped access to a registered export's frozen DOCX object.
use super::*;
use axum::extract::Query;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Ticket {
    token: String,
}

pub(super) async fn source(
    Path((workspace_id, request_id)): Path<(Uuid, Uuid)>,
    Query(ticket): Query<Ticket>,
) -> Result<Response, ApiErr> {
    let config = bidding::onlyoffice_conversion::Config::load()
        .map_err(|e| fail(StatusCode::SERVICE_UNAVAILABLE, e.code, e.message))?;
    let capability = config
        .verify_source_capability(&ticket.token)
        .map_err(|e| fail(StatusCode::FORBIDDEN, e.code, e.message))?;
    let signed = capability.source;
    if signed.workspace_id != workspace_id || signed.request_id != request_id {
        return Err(fail(
            StatusCode::FORBIDDEN,
            "ONLYOFFICE_SOURCE_SCOPE",
            "export source capability belongs to another request",
        ));
    }
    let pool = require_bid_pool().await?;
    let frozen = bidding::bid_authoring_v2::load_submission_export_source_v2(
        &pool,
        workspace_id,
        request_id,
        signed.version_id,
        &signed.docx_sha256,
    )
    .await
    .map_err(map_sql)?;
    if frozen["version_id"] != json!(signed.version_id)
        || frozen["docx_sha256"] != signed.docx_sha256
        || frozen["byte_length"].as_u64() != Some(signed.byte_length)
        || frozen["object_ref"] != format!("objects/{}", signed.docx_sha256)
    {
        return Err(fail(
            StatusCode::FORBIDDEN,
            "ONLYOFFICE_SOURCE_SCOPE",
            "export source capability does not match the frozen object",
        ));
    }
    let digest = signed.docx_sha256.clone();
    let bytes = tokio::task::spawn_blocking(move || platform::read_blob(&digest))
        .await
        .map_err(|_| {
            fail(
                StatusCode::SERVICE_UNAVAILABLE,
                "EXPORT_OBJECT_UNAVAILABLE",
                "object read interrupted",
            )
        })?
        .map_err(|_| {
            fail(
                StatusCode::SERVICE_UNAVAILABLE,
                "EXPORT_OBJECT_UNAVAILABLE",
                "frozen DOCX unavailable",
            )
        })?;
    if bytes.len() as u64 != signed.byte_length
        || platform::sha256_hex(&bytes) != signed.docx_sha256
    {
        return Err(fail(
            StatusCode::UNPROCESSABLE_ENTITY,
            "ASSET_DIGEST_MISMATCH",
            "frozen DOCX object mismatch",
        ));
    }
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", bidding::tender_upload::DOCX_MEDIA_TYPE)
        .header("content-length", bytes.len())
        .header("cache-control", "private, no-store")
        .header("etag", format!("\"{}\"", signed.docx_sha256))
        .body(Body::from(bytes))
        .map_err(|_| {
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "RESPONSE_BUILD_FAILED",
                "source response unavailable",
            )
        })
}
