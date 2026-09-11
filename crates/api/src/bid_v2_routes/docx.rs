use super::*;
use bidding::docx_round::{CreateDocxRound, DocxRoundBasis, DocxVersionIdentity, InitialDocx};

mod composition;
mod editor;

pub(super) fn router() -> Router<AppState> {
    Router::new()
        .merge(editor::router())
        .merge(composition::router())
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/docx-rounds/basis",
            get(round_basis),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/docx-rounds",
            post(create_round),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/docx/current",
            get(current),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/docx/versions/{version_id}",
            get(version),
        )
        .route(
            "/api/v2/submission-workspaces/{workspace_id}/docx/versions/{version_id}/download",
            get(download),
        )
}

async fn round_basis(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<Option<DocxRoundBasis>>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    bidding::docx_round::get_docx_round_basis(&pool, workspace_id, &actor)
        .await
        .map(Json)
        .map_err(map_docx_sql)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RoundMetadata {
    basis: DocxRoundBasis,
    expected: Option<DocxVersionIdentity>,
}

fn validate_metadata(metadata: &RoundMetadata) -> Result<(), ApiErr> {
    let valid_sha = |value: &str| {
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    };
    if !valid_sha(&metadata.basis.document_set_sha256)
        || !valid_sha(&metadata.basis.requirement_set_sha256)
        || metadata
            .expected
            .as_ref()
            .is_some_and(|value| !valid_sha(&value.docx_sha256))
    {
        return Err(validation("invalid DOCX round SHA-256 identity"));
    }
    Ok(())
}

async fn read_upload(mut multipart: Multipart) -> Result<(RoundMetadata, Vec<u8>), ApiErr> {
    let mut metadata = None;
    let mut file = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| validation(&error.to_string()))?
    {
        match field.name() {
            Some("metadata") if metadata.is_none() => {
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|error| validation(&error.to_string()))?;
                let value: RoundMetadata = serde_json::from_slice(&bytes)
                    .map_err(|error| validation(&error.to_string()))?;
                validate_metadata(&value)?;
                metadata = Some(value);
            }
            Some("file") if file.is_none() => {
                file = Some(
                    field
                        .bytes()
                        .await
                        .map_err(|error| validation(&error.to_string()))?
                        .to_vec(),
                );
            }
            _ => {
                return Err(validation(
                    "exactly one metadata field and one file field are required",
                ));
            }
        }
    }
    Ok((
        metadata.ok_or_else(|| validation("metadata required"))?,
        file.ok_or_else(|| validation("file required"))?,
    ))
}

fn map_docx_sql(error: sqlx::Error) -> ApiErr {
    if let Some(database) = error.as_database_error()
        && database.code().as_deref() == Some("40001")
    {
        for code in [
            "DOCX_VERSION_CAS_MISMATCH",
            "DOCX_EDITOR_STALE",
            "DOCX_SAVE_PENDING",
            "DOCX_SAVE_CORRELATION_MISMATCH",
            "DOCX_ROUND_BASIS_CHANGED",
            "DOCX_ROUND_REQUIREMENTS_NOT_CURRENT",
        ] {
            if database.message() == code {
                let message = match code {
                    "DOCX_SAVE_PENDING" => {
                        "A save request is pending; wait for its callback or final save"
                    }
                    "DOCX_SAVE_CORRELATION_MISMATCH" => {
                        "The save request is no longer current; this callback cannot publish"
                    }
                    "DOCX_EDITOR_STALE" => {
                        "The editing session has ended or been replaced; reopen the current document"
                    }
                    _ => "DOCX round basis or version changed; reload before creating a new round",
                };
                return fail(StatusCode::CONFLICT, code, message);
            }
        }
    }
    map_sql(error)
}

async fn create_round(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<Uuid>,
    multipart: Multipart,
) -> Result<(StatusCode, Json<Value>), ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let key = required_idempotency_key(&headers)?;
    let pool = require_bid_pool().await?;
    // Owner verification precedes both body processing and object registration.
    bidding::docx_round::get_current_docx(&pool, workspace_id, &actor)
        .await
        .map_err(map_docx_sql)?;
    let (metadata, bytes) = read_upload(multipart).await?;
    let initial_docx = tokio::task::spawn_blocking(move || InitialDocx::new(bytes))
        .await
        .map_err(|_| {
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DOCX_VALIDATION_FAILED",
                "DOCX validation task failed",
            )
        })?
        .map_err(map_upload_validation)?;
    let staging_id = Uuid::new_v4();
    let input = CreateDocxRound {
        workspace_id,
        staging_id,
        basis: &metadata.basis,
        expected: metadata.expected.as_ref(),
        initial_docx: &initial_docx,
        actor: &actor,
        idempotency_key: &key,
    };
    if let Some(replay) = bidding::docx_round::replay_docx_round(&pool, &input)
        .await
        .map_err(map_docx_sql)?
    {
        let version_id = replay["version_id"]
            .as_str()
            .and_then(|value| Uuid::parse_str(value).ok())
            .ok_or_else(|| validation("DOCX receipt identity missing"))?;
        let persisted =
            bidding::docx_round::get_docx_version(&pool, workspace_id, version_id, &actor)
                .await
                .map_err(map_docx_sql)?;
        read_verified_docx(persisted).await?;
        return Ok((StatusCode::CREATED, Json(replay)));
    }
    stage_upload(
        &pool,
        staging_id,
        &initial_docx.object_ref(),
        initial_docx.sha256(),
        bidding::tender_upload::DOCX_MEDIA_TYPE,
        initial_docx.bytes(),
        &actor,
    )
    .await?;
    if let Err(error) = read_verified_docx(json!({
        "object_ref": initial_docx.object_ref(), "docx_sha256": initial_docx.sha256(),
        "byte_length": initial_docx.bytes().len()
    }))
    .await
    {
        schedule_staging_cleanup_required(staging_id).await?;
        return Err(error);
    }
    match bidding::docx_round::create_docx_round(&pool, input).await {
        Ok(receipt) => Ok((StatusCode::CREATED, Json(receipt))),
        Err(error) => {
            schedule_staging_cleanup_required(staging_id).await?;
            Err(map_docx_sql(error))
        }
    }
}

async fn current(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<Option<Value>>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    bidding::docx_round::get_current_docx(&pool, workspace_id, &actor)
        .await
        .map(Json)
        .map_err(map_docx_sql)
}

async fn version(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, version_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    bidding::docx_round::get_docx_version(&pool, workspace_id, version_id, &actor)
        .await
        .map(Json)
        .map_err(map_docx_sql)
}

async fn read_verified_docx(metadata: Value) -> Result<Vec<u8>, ApiErr> {
    let digest = metadata["docx_sha256"]
        .as_str()
        .ok_or_else(|| validation("DOCX digest missing"))?
        .to_owned();
    if metadata["object_ref"].as_str() != Some(platform::object_ref(&digest).as_str()) {
        return Err(validation("DOCX object identity mismatch"));
    }
    let length = metadata["byte_length"]
        .as_u64()
        .ok_or_else(|| validation("DOCX byte length missing"))?;
    tokio::task::spawn_blocking(move || {
        let bytes = platform::read_blob(&digest).map_err(|error| {
            tracing::error!(%error, "DOCX object read failed");
            fail(
                StatusCode::SERVICE_UNAVAILABLE,
                "DOCX_OBJECT_UNAVAILABLE",
                "DOCX file is unavailable",
            )
        })?;
        if bytes.len() as u64 != length || platform::sha256_hex(&bytes) != digest {
            return Err(fail(
                StatusCode::UNPROCESSABLE_ENTITY,
                "DOCX_OBJECT_INTEGRITY_FAILED",
                "DOCX bytes do not match the stored version",
            ));
        }
        bidding::tender_upload::validate_docx_document(&bytes).map_err(map_upload_validation)?;
        Ok(bytes)
    })
    .await
    .map_err(|_| {
        fail(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DOCX_READ_FAILED",
            "DOCX read task failed",
        )
    })?
}

async fn download(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, version_id)): Path<(Uuid, Uuid)>,
) -> Result<Response, ApiErr> {
    let (_, actor) = human_actor(&headers, &state).await?;
    let pool = require_bid_pool().await?;
    let metadata = bidding::docx_round::get_docx_version(&pool, workspace_id, version_id, &actor)
        .await
        .map_err(map_docx_sql)?;
    let digest = metadata["docx_sha256"]
        .as_str()
        .ok_or_else(|| validation("DOCX digest missing"))?
        .to_owned();
    let bytes = read_verified_docx(metadata).await?;
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", bidding::tender_upload::DOCX_MEDIA_TYPE)
        .header(
            "content-disposition",
            format!("attachment; filename=\"{version_id}.docx\""),
        )
        .header("cache-control", "private, no-store")
        .header("x-content-type-options", "nosniff")
        .header("etag", format!("\"{digest}\""))
        .body(Body::from(bytes))
        .map_err(|_| {
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "RESPONSE_BUILD_FAILED",
                "DOCX download response failed",
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata() -> String {
        json!({"basis": {
            "document_set_id": Uuid::new_v4(),
            "document_set_sha256": platform::sha256_hex(b"collection"),
            "requirement_set_id": Uuid::new_v4(),
            "requirement_set_sha256": platform::sha256_hex(b"requirements")
        }, "expected": null})
        .to_string()
    }

    async fn multipart(fields: &[(&str, &str)]) -> Multipart {
        let boundary = Uuid::new_v4().simple().to_string();
        let mut body = String::new();
        for (name, value) in fields {
            body.push_str(&format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            ));
        }
        body.push_str(&format!("--{boundary}--\r\n"));
        let request = Request::builder()
            .header(
                "content-type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from(body))
            .unwrap();
        Multipart::from_request(request, &()).await.unwrap()
    }

    #[tokio::test]
    async fn multipart_is_unambiguous_and_requires_both_fields() {
        let metadata = metadata();
        let parsed =
            read_upload(multipart(&[("file", "payload"), ("metadata", &metadata)]).await).await;
        assert!(matches!(parsed, Ok((_, bytes)) if bytes == b"payload"));
        for fields in [
            vec![("metadata", metadata.as_str())],
            vec![("file", "payload")],
            vec![
                ("metadata", &metadata),
                ("metadata", &metadata),
                ("file", "payload"),
            ],
            vec![
                ("metadata", &metadata),
                ("file", "payload"),
                ("file", "other"),
            ],
            vec![
                ("metadata", &metadata),
                ("file", "payload"),
                ("old_workspace", "old"),
            ],
            vec![("metadata", "{broken"), ("file", "payload")],
        ] {
            let error = read_upload(multipart(&fields).await).await.unwrap_err();
            assert_eq!(error.0, StatusCode::BAD_REQUEST);
        }
    }
}
