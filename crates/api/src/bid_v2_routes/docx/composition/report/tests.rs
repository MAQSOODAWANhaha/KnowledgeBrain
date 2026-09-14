use super::*;

fn fixture() -> (Uuid, Value, Vec<u8>) {
    let version = Uuid::new_v4();
    let docx = platform::sha256_hex(b"reviewed document version");
    let bytes = serde_json::to_vec_pretty(&json!({
        "schema_version":1,"analysis_sha256":platform::sha256_hex(b"analysis"),
        "draft_sha256":platform::sha256_hex(b"draft"),"docx_sha256":docx,
        "sections":[],"placements":[],"omissions":[],"relation_omissions":[],
        "source_quality":"verified","source_open_items":[],"status":"reviewed_template"
    }))
    .unwrap();
    let sha = platform::sha256_hex(&bytes);
    let metadata = json!({"version_id":version,"docx_sha256":docx,
        "object_ref":platform::object_ref(&sha),"sha256":sha,"byte_length":bytes.len()});
    (version, metadata, bytes)
}

#[tokio::test]
async fn download_preserves_persisted_bytes_and_exact_version_identity() {
    let (version, metadata, bytes) = fixture();
    let response = response(
        identity(version, metadata.clone())
            .unwrap_or_else(|error| panic!("{}", error.1.0.error.message)),
        bytes.clone(),
    )
    .unwrap_or_else(|error| panic!("{}", error.1.0.error.message));
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "application/json");
    assert_eq!(
        response.headers()["content-disposition"],
        format!("attachment; filename=\"composition-report-{version}.json\"")
    );
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    assert_eq!(
        response.headers()["etag"],
        format!("\"{}\"", metadata["sha256"].as_str().unwrap())
    );
    assert_eq!(
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap(),
        bytes
    );
}

#[test]
fn report_cannot_be_substituted_by_another_version_or_object() {
    let (version, metadata, bytes) = fixture();
    for (requested, mut value) in [
        (Uuid::new_v4(), metadata.clone()),
        (version, metadata.clone()),
    ] {
        if requested == version {
            value["object_ref"] = json!("objects/another-report");
        }
        let error = identity(requested, value).err().unwrap();
        assert_eq!(error.0, StatusCode::UNPROCESSABLE_ENTITY);
    }
    let mut wrong_document = metadata.clone();
    wrong_document["docx_sha256"] = json!(platform::sha256_hex(b"another document"));
    let error = response(
        identity(version, wrong_document)
            .unwrap_or_else(|error| panic!("{}", error.1.0.error.message)),
        bytes.clone(),
    )
    .unwrap_err();
    assert_eq!(
        error.1.0.error.code,
        "DOCX_COMPOSITION_REPORT_INTEGRITY_FAILED"
    );

    for mutation in ["length", "digest", "invalid_json"] {
        let mut value = metadata.clone();
        let mut content = bytes.clone();
        match mutation {
            "length" => value["byte_length"] = json!(content.len() + 1),
            "digest" => content[0] = b'[',
            "invalid_json" => {
                content = b"not a persisted JSON manifest".to_vec();
                let sha = platform::sha256_hex(&content);
                value["sha256"] = json!(sha);
                value["object_ref"] = json!(platform::object_ref(&sha));
                value["byte_length"] = json!(content.len());
            }
            _ => unreachable!(),
        }
        let error = response(
            identity(version, value).unwrap_or_else(|error| panic!("{}", error.1.0.error.message)),
            content,
        )
        .unwrap_err();
        assert_eq!(error.0, StatusCode::UNPROCESSABLE_ENTITY, "{mutation}");
        assert_eq!(
            error.1.0.error.code,
            "DOCX_COMPOSITION_REPORT_INTEGRITY_FAILED"
        );
    }
}
