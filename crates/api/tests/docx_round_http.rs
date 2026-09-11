#![cfg(feature = "docx-http-contract-tests")]

//! Explicit opt-in HTTP contract. Requires a dedicated prepared database,
//! runtime API DSN, owned object directory, Redis and two DOCX sample paths.
//! Missing prerequisites fail; this target never silently skips a test.
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::path::PathBuf;
use tower::ServiceExt;
use uuid::Uuid;

fn required(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is required for the DOCX HTTP contract"))
}

fn upload(
    workspace: Uuid,
    token: &str,
    key: &str,
    metadata: &Value,
    bytes: &[u8],
) -> Request<Body> {
    let boundary = Uuid::new_v4().simple().to_string();
    let mut body = format!("--{boundary}\r\nContent-Disposition: form-data; name=\"metadata\"\r\n\r\n{metadata}\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"file\"\r\nContent-Type: application/octet-stream\r\n\r\n").into_bytes();
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    Request::builder()
        .method("POST")
        .uri(format!(
            "/api/v2/submission-workspaces/{workspace}/docx-rounds"
        ))
        .header("authorization", format!("Bearer {token}"))
        .header("idempotency-key", key)
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap()
}

async fn call(app: &axum::Router, request: Request<Body>) -> (StatusCode, Vec<u8>) {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let body = to_bytes(
        response.into_body(),
        platform::max_file_bytes() + 1024 * 1024,
    )
    .await
    .unwrap();
    (status, body.to_vec())
}

fn get(uri: String, token: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

fn expect_json(result: (StatusCode, Vec<u8>), status: StatusCode) -> Value {
    assert_eq!(result.0, status, "{}", String::from_utf8_lossy(&result.1));
    serde_json::from_slice(&result.1).unwrap()
}

async fn version_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM bid_docx_version_artifacts")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn routed_docx_rounds_preserve_real_bytes_authorization_replay_and_history() {
    let admin_url = required("KNOWLEDGEBRAIN_TEST_DATABASE_URL");
    for url in [&admin_url, &required("DATABASE_URL")] {
        let options: sqlx::postgres::PgConnectOptions = url.parse().unwrap();
        assert!(matches!(
            options.get_host(),
            "127.0.0.1" | "localhost" | "::1"
        ));
        assert!(
            options
                .get_database()
                .unwrap()
                .starts_with("knowledgebrain_test_")
        );
    }
    required("REDIS_URL");
    let object_dir = PathBuf::from(required("OBJECT_DIR"))
        .canonicalize()
        .unwrap();
    assert!(object_dir.starts_with(std::env::temp_dir()) && object_dir != std::env::temp_dir());
    assert!(
        !platform::s3_configured(),
        "this fixture requires local-only owned storage"
    );
    let first_bytes = std::fs::read(required("KNOWLEDGEBRAIN_TEST_DOCX_PATH")).unwrap();
    let second_bytes = std::fs::read(required("KNOWLEDGEBRAIN_TEST_SECOND_DOCX_PATH")).unwrap();
    assert_ne!(first_bytes, second_bytes);
    let admin = PgPool::connect(&admin_url).await.unwrap();
    let runtime = platform::connect().await.unwrap();
    let role: String = sqlx::query_scalar("SELECT current_user")
        .fetch_one(&runtime)
        .await
        .unwrap();
    assert_eq!(role, "kb_runtime_api");
    let fixture: Value = sqlx::query_scalar("SELECT jsonb_build_object('workspace',w.id,'owner',p.owner_user_id) FROM bid_submission_workspaces w JOIN bid_projects p ON p.id=w.project_id")
        .fetch_one(&admin).await.unwrap();
    let workspace = Uuid::parse_str(fixture["workspace"].as_str().unwrap()).unwrap();
    let owner = Uuid::parse_str(fixture["owner"].as_str().unwrap()).unwrap();
    let secret = Uuid::new_v4().to_string();
    let token = platform::issue_jwt(owner, &secret).unwrap();
    let wrong_token = platform::issue_jwt(Uuid::new_v4(), &secret).unwrap();
    let app = api::router_with(api::AppState {
        jwt_secret: secret,
        bootstrap_key: String::new(),
    });
    let base_uri = format!("/api/v2/submission-workspaces/{workspace}/docx");
    let old = expect_json(
        call(&app, get(format!("{base_uri}/current"), &token)).await,
        StatusCode::OK,
    );
    let metadata = json!({"basis": {
        "document_set_id": old["round_basis"]["document_set_id"],
        "document_set_sha256": old["round_basis"]["document_set_sha256"],
        "requirement_set_id": old["round_basis"]["requirement_set_id"],
        "requirement_set_sha256": old["round_basis"]["requirement_set_sha256"]
    }, "expected": {"version_id":old["version_id"],"docx_sha256":old["docx_sha256"]}});
    assert_eq!(
        expect_json(
            call(&app, get(format!("{base_uri}-rounds/basis"), &token)).await,
            StatusCode::OK
        ),
        metadata["basis"]
    );
    expect_json(
        call(&app, get(format!("{base_uri}-rounds/basis"), &wrong_token)).await,
        StatusCode::FORBIDDEN,
    );
    let mut anonymous_basis = get(format!("{base_uri}-rounds/basis"), &token);
    anonymous_basis.headers_mut().remove("authorization");
    expect_json(call(&app, anonymous_basis).await, StatusCode::UNAUTHORIZED);
    // A fresh project has no DOCX. Its empty, initialized requirement set is
    // still an identity and must not be inferred from the first requirement row.
    let project = expect_json(
        call(
            &app,
            post_json(
                "/api/v2/bid-projects",
                &token,
                &Uuid::new_v4().to_string(),
                &json!({"title":format!("DOCX first publication {}", Uuid::new_v4())}),
            ),
        )
        .await,
        StatusCode::CREATED,
    );
    let fresh_workspace = Uuid::parse_str(project["workspace_id"].as_str().unwrap()).unwrap();
    let fresh_uri = format!("/api/v2/submission-workspaces/{fresh_workspace}/docx");
    assert!(
        expect_json(
            call(&app, get(format!("{fresh_uri}/current"), &token)).await,
            StatusCode::OK
        )
        .is_null()
    );
    let fresh_basis = expect_json(
        call(&app, get(format!("{fresh_uri}-rounds/basis"), &token)).await,
        StatusCode::OK,
    );
    assert!(fresh_basis["requirement_set_id"].is_string());
    let fresh_metadata = json!({"basis":fresh_basis,"expected":null});
    let fresh_key = Uuid::new_v4().to_string();
    let fresh = expect_json(
        call(
            &app,
            upload(
                fresh_workspace,
                &token,
                &fresh_key,
                &fresh_metadata,
                &first_bytes,
            ),
        )
        .await,
        StatusCode::CREATED,
    );
    assert_eq!(fresh["round_revision"], 1);
    assert_eq!(
        expect_json(
            call(
                &app,
                upload(
                    fresh_workspace,
                    &token,
                    &fresh_key,
                    &fresh_metadata,
                    &first_bytes
                )
            )
            .await,
            StatusCode::CREATED
        ),
        fresh
    );
    expect_json(
        call(
            &app,
            upload(
                fresh_workspace,
                &token,
                &Uuid::new_v4().to_string(),
                &fresh_metadata,
                &first_bytes,
            ),
        )
        .await,
        StatusCode::CONFLICT,
    );
    // Advance the real disposition head while requirements still reference its
    // predecessor; the read must return no usable basis, without deleting heads.
    let project_id = Uuid::parse_str(project["id"].as_str().unwrap()).unwrap();
    let disposition = Uuid::new_v4();
    sqlx::query("INSERT INTO bid_source_unit_disposition_set_artifacts(id,project_id,document_set_id,document_set_sequence,revision,canonical_payload,content_sha256,actor) SELECT $2,a.project_id,a.document_set_id,a.document_set_sequence,a.revision+1,a.canonical_payload,a.content_sha256,a.actor FROM bid_source_unit_disposition_set_current h JOIN bid_source_unit_disposition_set_artifacts a ON a.id=h.artifact_id WHERE h.scope_id=$1")
        .bind(project_id).bind(disposition).execute(&admin).await.unwrap();
    let advanced: bool = sqlx::query_scalar("SELECT kb_bid_v2_advance_disposition_set(scope_id,artifact_id,artifact_sha256,$2,artifact_sha256) FROM bid_source_unit_disposition_set_current WHERE scope_id=$1")
        .bind(project_id).bind(disposition).fetch_one(&admin).await.unwrap();
    assert!(advanced);
    assert!(
        expect_json(
            call(&app, get(format!("{fresh_uri}-rounds/basis"), &token)).await,
            StatusCode::OK
        )
        .is_null()
    );
    let stale_basis_metadata = json!({"basis":fresh_basis,"expected":{"version_id":fresh["version_id"],"docx_sha256":fresh["docx_sha256"]}});
    let stale_basis = expect_json(
        call(
            &app,
            upload(
                fresh_workspace,
                &token,
                &Uuid::new_v4().to_string(),
                &stale_basis_metadata,
                &first_bytes,
            ),
        )
        .await,
        StatusCode::CONFLICT,
    );
    assert_eq!(
        stale_basis["error"]["code"],
        "DOCX_ROUND_REQUIREMENTS_NOT_CURRENT"
    );
    let initial_count = version_count(&admin).await;
    let key = Uuid::new_v4().to_string();
    let mut anonymous = upload(workspace, &token, &key, &metadata, &first_bytes);
    anonymous.headers_mut().remove("authorization");
    expect_json(call(&app, anonymous).await, StatusCode::UNAUTHORIZED);
    expect_json(
        call(
            &app,
            upload(workspace, &wrong_token, &key, &metadata, &first_bytes),
        )
        .await,
        StatusCode::FORBIDDEN,
    );
    expect_json(
        call(
            &app,
            upload(workspace, &token, &key, &metadata, b"broken DOCX"),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );
    let mut extra = metadata.clone();
    extra["old_workspace"] = json!({"responses":["old"]});
    expect_json(
        call(&app, upload(workspace, &token, &key, &extra, &first_bytes)).await,
        StatusCode::BAD_REQUEST,
    );
    assert_eq!(version_count(&admin).await, initial_count);
    let first = expect_json(
        call(
            &app,
            upload(workspace, &token, &key, &metadata, &first_bytes),
        )
        .await,
        StatusCode::CREATED,
    );
    let first_hash = platform::sha256_hex(&first_bytes);
    assert_eq!(first["docx_sha256"], first_hash);
    let first_path = platform::blob_path(&first_hash).unwrap();
    assert!(first_path.starts_with(&object_dir));
    assert_eq!(std::fs::read(&first_path).unwrap(), first_bytes);

    let modified = std::fs::metadata(&first_path).unwrap().modified().unwrap();
    let replay = expect_json(
        call(
            &app,
            upload(workspace, &token, &key, &metadata, &first_bytes),
        )
        .await,
        StatusCode::CREATED,
    );
    assert_eq!(first, replay);
    assert_eq!(
        std::fs::metadata(&first_path).unwrap().modified().unwrap(),
        modified,
        "replay rewrote immutable bytes"
    );
    let mismatch = expect_json(
        call(
            &app,
            upload(workspace, &token, &key, &metadata, &second_bytes),
        )
        .await,
        StatusCode::CONFLICT,
    );
    assert_eq!(mismatch["error"]["code"], "IDEMPOTENCY_PAYLOAD_MISMATCH");
    let next_metadata = json!({"basis": metadata["basis"], "expected": {"version_id": first["version_id"], "docx_sha256": first["docx_sha256"]}});
    let second = expect_json(
        call(
            &app,
            upload(
                workspace,
                &token,
                &Uuid::new_v4().to_string(),
                &next_metadata,
                &second_bytes,
            ),
        )
        .await,
        StatusCode::CREATED,
    );
    assert_ne!(first["round_id"], second["round_id"]);
    assert_eq!(version_count(&admin).await, initial_count + 2);
    // Force a real write failure in this fixture's owned storage, then restore
    // the directory before asserting so a failed assertion cannot hide it.
    let backup = object_dir.with_extension(Uuid::new_v4().simple().to_string());
    std::fs::rename(&object_dir, &backup).unwrap();
    std::fs::write(&object_dir, b"not a directory").unwrap();
    let write_failure = call(&app, upload(workspace, &token, &Uuid::new_v4().to_string(),
        &json!({"basis":metadata["basis"],"expected":{"version_id":second["version_id"],"docx_sha256":second["docx_sha256"]}}), &first_bytes)).await;
    std::fs::remove_file(&object_dir).unwrap();
    std::fs::rename(&backup, &object_dir).unwrap();
    let write_error = expect_json(write_failure, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(write_error["error"]["code"], "OBJECT_WRITE_FAILED");
    assert_eq!(version_count(&admin).await, initial_count + 2);
    let download_uri = format!(
        "{base_uri}/versions/{}/download",
        first["version_id"].as_str().unwrap()
    );
    let response = app
        .clone()
        .oneshot(get(download_uri.clone(), &token))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert_eq!(
        response.headers()["content-type"],
        bidding::tender_upload::DOCX_MEDIA_TYPE
    );
    assert_eq!(
        to_bytes(response.into_body(), platform::max_file_bytes())
            .await
            .unwrap()
            .as_ref(),
        first_bytes
    );
    expect_json(
        call(&app, get(download_uri.clone(), &wrong_token)).await,
        StatusCode::FORBIDDEN,
    );
    let history_replay = expect_json(
        call(
            &app,
            upload(workspace, &token, &key, &metadata, &first_bytes),
        )
        .await,
        StatusCode::CREATED,
    );
    assert_eq!(history_replay, first);
    let current = expect_json(
        call(&app, get(format!("{base_uri}/current"), &token)).await,
        StatusCode::OK,
    );
    assert_eq!(current["version_id"], second["version_id"]);
    let stale = expect_json(
        call(
            &app,
            upload(
                workspace,
                &token,
                &Uuid::new_v4().to_string(),
                &metadata,
                &second_bytes,
            ),
        )
        .await,
        StatusCode::CONFLICT,
    );
    assert_eq!(stale["error"]["code"], "DOCX_VERSION_CAS_MISMATCH");
    assert_eq!(version_count(&admin).await, initial_count + 2);
    assert!(
        platform::oxana_connect()
            .unwrap()
            .enqueued_count(platform::RetentionQueue)
            .await
            .unwrap()
            >= 2,
        "write failure and failed publication must enqueue existing staging cleanup"
    );
    let mut corrupt = first_bytes.clone();
    corrupt[0] ^= 1;
    std::fs::write(&first_path, corrupt).unwrap();
    let invalid = expect_json(
        call(&app, get(download_uri.clone(), &token)).await,
        StatusCode::UNPROCESSABLE_ENTITY,
    );
    assert_eq!(invalid["error"]["code"], "DOCX_OBJECT_INTEGRITY_FAILED");
    std::fs::remove_file(&first_path).unwrap();
    let missing = expect_json(
        call(&app, get(download_uri, &token)).await,
        StatusCode::SERVICE_UNAVAILABLE,
    );
    assert_eq!(missing["error"]["code"], "DOCX_OBJECT_UNAVAILABLE");
    std::fs::write(&first_path, &first_bytes).unwrap();
    let current = expect_json(
        call(&app, get(format!("{base_uri}/current"), &token)).await,
        StatusCode::OK,
    );
    assert_eq!(current["version_id"], second["version_id"]);
    // The following requests exercise the product's editor routes. The configured
    // Document Server transport is an owned mock; PostgreSQL and blobs are real.
    let editor_uri = format!("{base_uri}/editor");
    let open_key = Uuid::new_v4().to_string();
    let expected = json!({"version_id":second["version_id"],"docx_sha256":second["docx_sha256"]});
    for language in [
        "",
        "zh_CN",
        "en&customer=other",
        "en\n",
        "zh-",
        "en-123456789",
    ] {
        let mut invalid = expected.clone();
        invalid["language"] = json!(language);
        expect_json(
            call(
                &app,
                post_json(&editor_uri, &token, &Uuid::new_v4().to_string(), &invalid),
            )
            .await,
            StatusCode::BAD_REQUEST,
        );
    }
    let mut spoofed = expected.clone();
    spoofed["user"] = json!({"id":Uuid::new_v4(),"name":"other account"});
    expect_json(
        call(
            &app,
            post_json(&editor_uri, &token, &Uuid::new_v4().to_string(), &spoofed),
        )
        .await,
        StatusCode::BAD_REQUEST,
    );
    assert_eq!(
        expect_json(
            call(&app, get(format!("{base_uri}/current"), &token)).await,
            StatusCode::OK
        ),
        current,
        "invalid presentation input changed the document session"
    );
    let mut localized = expected.clone();
    localized["language"] = json!("fr-CA");
    let (a, b) = tokio::join!(
        call(&app, post_json(&editor_uri, &token, &open_key, &localized)),
        call(
            &app,
            post_json(&editor_uri, &token, &Uuid::new_v4().to_string(), &expected)
        )
    );
    let opened = expect_json(a, StatusCode::OK);
    let peer = expect_json(b, StatusCode::OK);
    let email: String = sqlx::query_scalar("SELECT email FROM users WHERE id=$1")
        .bind(owner)
        .fetch_one(&admin)
        .await
        .unwrap();
    assert_eq!(
        opened["config"]["editorConfig"]["user"],
        json!({"id":format!("user:{owner}"),"name":email})
    );
    assert_eq!(opened["config"]["editorConfig"]["lang"], "fr-CA");
    assert!(
        peer["config"]["editorConfig"]["lang"].is_null(),
        "no locale supplied must retain the service default"
    );
    let signed = jsonwebtoken::decode::<Value>(
        opened["config"]["token"].as_str().unwrap(),
        &jsonwebtoken::DecodingKey::from_secret(required("KB_ONLYOFFICE_JWT_SECRET").as_bytes()),
        &jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256),
    )
    .unwrap()
    .claims;
    assert_eq!(signed["editorConfig"], opened["config"]["editorConfig"]);
    assert_eq!(
        opened["session"]["editor_key"],
        peer["session"]["editor_key"]
    );
    let editor_key = opened["session"]["editor_key"].as_str().unwrap();
    let source_uri = local_uri(opened["config"]["document"]["url"].as_str().unwrap());
    let callback_uri = local_uri(
        opened["config"]["editorConfig"]["callbackUrl"]
            .as_str()
            .unwrap(),
    );
    let anonymous_source = Request::builder()
        .uri(&source_uri)
        .body(Body::empty())
        .unwrap();
    expect_json(call(&app, anonymous_source).await, StatusCode::UNAUTHORIZED);
    let downloaded = call(&app, service_source(&source_uri)).await;
    assert_eq!(downloaded.0, StatusCode::OK);
    assert_eq!(downloaded.1, second_bytes);
    let service_origin = required("KB_ONLYOFFICE_SERVER_ORIGIN");
    let current_before_editor = version_count(&admin).await;
    let notify = json!({"key":editor_key,"status":4});
    expect_json(
        call(&app, service_callback(&callback_uri, &notify, None)).await,
        StatusCode::OK,
    );
    let peer = expect_json(
        call(
            &app,
            post_json(&editor_uri, &token, &Uuid::new_v4().to_string(), &expected),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(peer["session"]["editor_key"], editor_key);
    assert_eq!(version_count(&admin).await, current_before_editor);
    let bad_token = service_callback(&callback_uri, &notify, Some("wrong service secret"));
    expect_json(call(&app, bad_token).await, StatusCode::UNAUTHORIZED);
    let mut tampered = service_callback(&callback_uri, &notify, None);
    *tampered.body_mut() = Body::from(json!({"key":editor_key,"status":3}).to_string());
    expect_json(call(&app, tampered).await, StatusCode::FORBIDDEN);
    // Source scope URLs are not bearer credentials. A fresh service JWT must
    // bind the full URL, including its signed scope; absent expiry is rejected.
    for exp in [Some(chrono::Utc::now().timestamp() - 1), None] {
        let mut expired = service_source(&source_uri);
        let payload = json!({"url":opened["config"]["document"]["url"]});
        expired.headers_mut().insert(
            "authorization",
            service_authorization(&payload, exp).parse().unwrap(),
        );
        expect_json(call(&app, expired).await, StatusCode::UNAUTHORIZED);
        let mut expired = service_callback(&callback_uri, &notify, None);
        expired.headers_mut().insert(
            "authorization",
            service_authorization(&notify, exp).parse().unwrap(),
        );
        expect_json(call(&app, expired).await, StatusCode::UNAUTHORIZED);
    }
    let mut wrong_url = service_source(&source_uri);
    wrong_url.headers_mut().insert(
        "authorization",
        service_authorization(
            &json!({"url":opened["config"]["editorConfig"]["callbackUrl"]}),
            Some(chrono::Utc::now().timestamp() + 60),
        )
        .parse()
        .unwrap(),
    );
    expect_json(call(&app, wrong_url).await, StatusCode::FORBIDDEN);
    let mut invalid_source_signature = service_source(&source_uri);
    invalid_source_signature
        .headers_mut()
        .insert("authorization", "Bearer invalid".parse().unwrap());
    expect_json(
        call(&app, invalid_source_signature).await,
        StatusCode::UNAUTHORIZED,
    );
    let source_ticket =
        reqwest::Url::parse(opened["config"]["document"]["url"].as_str().unwrap()).unwrap();
    let wrong_purpose =
        callback_uri.split('?').next().unwrap().to_owned() + "?" + source_ticket.query().unwrap();
    expect_json(
        call(&app, service_callback(&wrong_purpose, &notify, None)).await,
        StatusCode::UNAUTHORIZED,
    );
    let wrong_scope = callback_uri.replace(&workspace.to_string(), &Uuid::new_v4().to_string());
    expect_json(
        call(&app, service_callback(&wrong_scope, &notify, None)).await,
        StatusCode::FORBIDDEN,
    );
    let mut request = json!({"editor_key":editor_key,"expected":expected});
    let force_uri = format!("{editor_uri}/save");
    let final_failure = json!({"key":editor_key,"status":3});
    expect_json(
        call(&app, service_callback(&callback_uri, &final_failure, None)).await,
        StatusCode::OK,
    );
    expect_json(
        call(&app, service_callback(&callback_uri, &notify, None)).await,
        StatusCode::OK,
    );
    let error_current = expect_json(
        call(&app, get(format!("{base_uri}/current"), &token)).await,
        StatusCode::OK,
    );
    assert_eq!(error_current["editor"]["save_error"]["code"], 3);
    assert_eq!(version_count(&admin).await, current_before_editor);
    let force_key = Uuid::new_v4().to_string();
    let pending = expect_json(
        call(&app, post_json(&force_uri, &token, &force_key, &request)).await,
        StatusCode::OK,
    );
    assert_eq!(pending["dispatch"], true);
    let saved_uri = format!(
        "{editor_uri}/{editor_key}/saves/{}",
        pending["save_id"].as_str().unwrap()
    );
    assert!(
        expect_json(
            call(&app, get(saved_uri.clone(), &token)).await,
            StatusCode::OK
        )
        .is_null()
    );
    expect_json(
        call(&app, get(saved_uri.clone(), &wrong_token)).await,
        StatusCode::FORBIDDEN,
    );
    let worker_can_read: bool = sqlx::query_scalar("SELECT has_function_privilege('kb_runtime_worker','kb_bid_v2_get_docx_saved_receipt(uuid,uuid,uuid,kb_actor_identity)','EXECUTE')")
        .fetch_one(&admin).await.unwrap();
    assert!(
        !worker_can_read,
        "save receipt lookup must remain owner/API-only"
    );

    let replay = expect_json(
        call(&app, post_json(&force_uri, &token, &force_key, &request)).await,
        StatusCode::OK,
    );
    assert_eq!(replay["dispatch"], false);
    assert_eq!(mock_commands().await.len(), 1);
    let busy = expect_json(
        call(
            &app,
            post_json(&force_uri, &token, &Uuid::new_v4().to_string(), &request),
        )
        .await,
        StatusCode::CONFLICT,
    );
    assert_eq!(busy["error"]["code"], "DOCX_SAVE_PENDING");
    let force_body = json!({"key":editor_key,"status":6,"forcesavetype":0,"userdata":pending["save_id"],"url":format!("{service_origin}/cache/files/first.docx")});
    let mut outside = force_body.clone();
    outside["url"] = json!(format!("{service_origin}/test/commands"));
    expect_json(
        call(&app, service_callback(&callback_uri, &outside, None)).await,
        StatusCode::FORBIDDEN,
    );
    outside["url"] = json!("https://example.invalid/cache/files/other.docx");
    expect_json(
        call(&app, service_callback(&callback_uri, &outside, None)).await,
        StatusCode::FORBIDDEN,
    );
    let mut uncorrelated = force_body.clone();
    uncorrelated["forcesavetype"] = json!(1);
    expect_json(
        call(&app, service_callback(&callback_uri, &uncorrelated, None)).await,
        StatusCode::BAD_REQUEST,
    );
    let mut corrupt_body = force_body.clone();
    corrupt_body["url"] = json!(format!("{service_origin}/cache/files/corrupt.docx"));
    expect_json(
        call(&app, service_callback(&callback_uri, &corrupt_body, None)).await,
        StatusCode::BAD_REQUEST,
    );
    corrupt_body["url"] = json!(format!("{service_origin}/cache/files/redirect"));
    expect_json(
        call(&app, service_callback(&callback_uri, &corrupt_body, None)).await,
        StatusCode::SERVICE_UNAVAILABLE,
    );
    assert_eq!(version_count(&admin).await, current_before_editor);
    let backup = object_dir.with_extension(Uuid::new_v4().simple().to_string());
    std::fs::rename(&object_dir, &backup).unwrap();
    std::fs::write(&object_dir, b"not a directory").unwrap();
    let failed_write = call(&app, service_callback(&callback_uri, &force_body, None)).await;
    std::fs::remove_file(&object_dir).unwrap();
    std::fs::rename(&backup, &object_dir).unwrap();
    let failed_write = expect_json(failed_write, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(failed_write["error"], 1);
    assert_eq!(failed_write["code"], "OBJECT_WRITE_FAILED");
    assert!(
        expect_json(
            call(&app, get(saved_uri.clone(), &token)).await,
            StatusCode::OK
        )
        .is_null()
    );

    assert_eq!(version_count(&admin).await, current_before_editor);
    expect_json(
        call(&app, service_callback(&callback_uri, &force_body, None)).await,
        StatusCode::OK,
    );
    assert_eq!(version_count(&admin).await, current_before_editor + 1);
    let saved = expect_json(
        call(&app, get(format!("{base_uri}/current"), &token)).await,
        StatusCode::OK,
    );
    assert_eq!(saved["docx_sha256"], platform::sha256_hex(&first_bytes));
    // Completed callbacks must survive Document Server cache expiry. Count
    // actual mock GETs as well as versions: success alone could hide a download.
    mock_cache_available(false).await;
    let downloads_before_replay = mock_downloads().await;
    let receipts_before_replay: i64 =
        sqlx::query_scalar("SELECT count(*) FROM idempotency_requests")
            .fetch_one(&admin)
            .await
            .unwrap();
    let exact_receipt = expect_json(
        call(&app, get(saved_uri.clone(), &token)).await,
        StatusCode::OK,
    );
    assert_eq!(
        exact_receipt,
        json!({
            "save_id":pending["save_id"], "editor_key":editor_key,
            "round_id":saved["round_id"], "version_id":saved["version_id"],
            "docx_sha256":platform::sha256_hex(&first_bytes),
            "parent_version_id":expected["version_id"], "revision":saved["revision"]
        })
    );
    for uri in [
        format!("{editor_uri}/{editor_key}/saves/{}", Uuid::new_v4()),
        format!(
            "{editor_uri}/{}/saves/{}",
            Uuid::new_v4(),
            pending["save_id"].as_str().unwrap()
        ),
    ] {
        assert!(expect_json(call(&app, get(uri, &token)).await, StatusCode::OK).is_null());
    }
    // The same owner has a second project. Knowing an exact receipt identity
    // must not let that workspace expose the first project's publication.
    assert!(
        expect_json(
            call(
                &app,
                get(
                    format!(
                        "{fresh_uri}/editor/{editor_key}/saves/{}",
                        pending["save_id"].as_str().unwrap()
                    ),
                    &token
                )
            )
            .await,
            StatusCode::OK
        )
        .is_null()
    );
    expect_json(
        call(&app, get(saved_uri.clone(), &wrong_token)).await,
        StatusCode::FORBIDDEN,
    );
    let modified = std::fs::metadata(&first_path).unwrap().modified().unwrap();
    expect_json(
        call(&app, service_callback(&callback_uri, &force_body, None)).await,
        StatusCode::OK,
    );
    assert_eq!(
        std::fs::metadata(&first_path).unwrap().modified().unwrap(),
        modified
    );
    assert_eq!(version_count(&admin).await, current_before_editor + 1);
    expect_json(
        call(
            &app,
            service_callback(&callback_uri, &force_body, Some("invalid signature")),
        )
        .await,
        StatusCode::UNAUTHORIZED,
    );
    // A refreshed service JWT/body token is transport metadata. The signed
    // semantic payload stays identical and still acknowledges the old bytes.
    let mut retokened = service_callback(&callback_uri, &force_body, None);
    let mut retokened_body = force_body.clone();
    retokened_body["token"] = json!("replacement transport token");
    *retokened.body_mut() = Body::from(retokened_body.to_string());
    expect_json(call(&app, retokened).await, StatusCode::OK);
    let mut expired_callback = service_callback(&callback_uri, &force_body, None);
    expired_callback.headers_mut().insert(
        "authorization",
        service_authorization(&force_body, Some(chrono::Utc::now().timestamp() - 1))
            .parse()
            .unwrap(),
    );
    expect_json(call(&app, expired_callback).await, StatusCode::UNAUTHORIZED);
    let mut unsigned_callback = service_callback(&callback_uri, &force_body, None);
    unsigned_callback.headers_mut().remove("authorization");
    expect_json(
        call(&app, unsigned_callback).await,
        StatusCode::UNAUTHORIZED,
    );
    // Exercise the commit fence for two deliveries that downloaded before the
    // winner committed. Matching notification identity cannot bind new bytes or
    // a different length to the completed version, even through the SQL API.
    let save_receipt_key = format!("{editor_key}:{}", pending["save_id"].as_str().unwrap());
    let recorded_input: Value = sqlx::query_scalar("SELECT convert_from(request_bytes,'UTF8')::jsonb->'input' FROM idempotency_requests WHERE operation='bid.v2.docx_editor.save' AND idempotency_key=$1")
        .bind(&save_receipt_key).fetch_one(&admin).await.unwrap();
    for (digest, length) in [
        (platform::sha256_hex(&second_bytes), second_bytes.len()),
        (platform::sha256_hex(&first_bytes), first_bytes.len() + 1),
    ] {
        let mut late_commit = recorded_input.clone();
        late_commit["docx_sha256"] = json!(digest);
        late_commit["byte_length"] = json!(length);
        late_commit["staging_id"] = json!(Uuid::new_v4());
        let error = bidding::docx_round::editor_command(
            &runtime,
            workspace,
            "save",
            &late_commit,
            &format!("user:{owner}"),
            &save_receipt_key,
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().message(),
            "IDEMPOTENCY_PAYLOAD_MISMATCH"
        );
    }
    // The local immutable object remains authoritative. A receipt alone cannot
    // mask missing/corrupt storage, even when the remote cache is unavailable.
    let stored_bytes = std::fs::read(&first_path).unwrap();
    std::fs::write(&first_path, b"corrupt stored DOCX").unwrap();
    let corrupt_replay = call(&app, service_callback(&callback_uri, &force_body, None)).await;
    std::fs::write(&first_path, &stored_bytes).unwrap();
    expect_json(corrupt_replay, StatusCode::UNPROCESSABLE_ENTITY);
    let missing_path = first_path.with_extension(Uuid::new_v4().simple().to_string());
    std::fs::rename(&first_path, &missing_path).unwrap();
    let missing_replay = call(&app, service_callback(&callback_uri, &force_body, None)).await;
    std::fs::rename(&missing_path, &first_path).unwrap();
    expect_json(missing_replay, StatusCode::SERVICE_UNAVAILABLE);
    let mut changed_notification = force_body.clone();
    changed_notification["users"] = json!([Uuid::new_v4().to_string()]);
    expect_json(
        call(
            &app,
            service_callback(&callback_uri, &changed_notification, None),
        )
        .await,
        StatusCode::CONFLICT,
    );
    let mut changed_receipt = force_body.clone();
    changed_receipt["url"] = json!(format!("{service_origin}/cache/files/second.docx"));
    expect_json(
        call(
            &app,
            service_callback(&callback_uri, &changed_receipt, None),
        )
        .await,
        StatusCode::CONFLICT,
    );
    assert_eq!(version_count(&admin).await, current_before_editor + 1);
    assert_eq!(mock_downloads().await, downloads_before_replay);
    let receipt_count: i64 = sqlx::query_scalar("SELECT count(*) FROM idempotency_requests")
        .fetch_one(&admin)
        .await
        .unwrap();
    assert_eq!(
        receipt_count, receipts_before_replay,
        "replay created another receipt"
    );
    mock_cache_available(true).await;
    let stable = expect_json(
        call(
            &app,
            post_json(
                &editor_uri,
                &token,
                &Uuid::new_v4().to_string(),
                &json!({"version_id":saved["version_id"],"docx_sha256":saved["docx_sha256"]}),
            ),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(stable["session"]["editor_key"], editor_key);
    assert_eq!(
        stable["session"]["base"]["version_id"],
        second["version_id"]
    );
    let downloaded = call(&app, service_source(&source_uri)).await;
    assert_eq!(
        downloaded.1, second_bytes,
        "active source baseline changed after forcesave"
    );
    request["expected"] =
        json!({"version_id":saved["version_id"],"docx_sha256":saved["docx_sha256"]});
    let pending2 = expect_json(
        call(
            &app,
            post_json(&force_uri, &token, &Uuid::new_v4().to_string(), &request),
        )
        .await,
        StatusCode::OK,
    );
    let failed =
        json!({"key":editor_key,"status":7,"forcesavetype":0,"userdata":pending2["save_id"]});
    expect_json(
        call(&app, service_callback(&callback_uri, &failed, None)).await,
        StatusCode::OK,
    );
    let failure_state = expect_json(
        call(
            &app,
            post_json(
                &editor_uri,
                &token,
                &Uuid::new_v4().to_string(),
                &request["expected"],
            ),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(failure_state["session"]["save_error"]["code"], 7);
    assert!(failure_state["session"]["pending_save_id"].is_null());
    assert_eq!(version_count(&admin).await, current_before_editor + 1);
    // Transport ambiguity must not redispatch a command with the same correlation.
    reqwest::Client::new()
        .post(format!("{service_origin}/test/mode"))
        .json(&json!({"mode":"invalid-response"}))
        .send()
        .await
        .unwrap();
    let uncertain_key = Uuid::new_v4().to_string();
    expect_json(
        call(
            &app,
            post_json(&force_uri, &token, &uncertain_key, &request),
        )
        .await,
        StatusCode::SERVICE_UNAVAILABLE,
    );
    let commands_before = mock_commands().await.len();
    let uncertain = expect_json(
        call(
            &app,
            post_json(&force_uri, &token, &uncertain_key, &request),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(uncertain["dispatch"], false);
    assert_eq!(uncertain["pending"], true);
    assert_eq!(mock_commands().await.len(), commands_before);
    expect_json(
        call(&app, service_callback(&callback_uri, &failed, None)).await,
        StatusCode::OK,
    );
    let after_old_error = expect_json(
        call(
            &app,
            post_json(
                &editor_uri,
                &token,
                &Uuid::new_v4().to_string(),
                &request["expected"],
            ),
        )
        .await,
        StatusCode::OK,
    );
    assert_eq!(
        after_old_error["session"]["pending_save_id"], uncertain["save_id"],
        "old error cleared a newer pending save"
    );
    let final_body = json!({"key":editor_key,"status":2,"url":format!("{service_origin}/cache/files/second.docx")});
    expect_json(
        call(&app, service_callback(&callback_uri, &final_body, None)).await,
        StatusCode::OK,
    );
    let final_version = expect_json(
        call(&app, get(format!("{base_uri}/current"), &token)).await,
        StatusCode::OK,
    );
    assert_eq!(
        final_version["docx_sha256"],
        platform::sha256_hex(&second_bytes)
    );
    mock_cache_available(false).await;
    let downloads_before_final_replay = mock_downloads().await;
    expect_json(
        call(&app, service_callback(&callback_uri, &final_body, None)).await,
        StatusCode::OK,
    );
    expect_json(
        call(&app, service_callback(&callback_uri, &force_body, None)).await,
        StatusCode::OK,
    );
    assert_eq!(version_count(&admin).await, current_before_editor + 2);
    let mut late = force_body.clone();
    late["userdata"] = uncertain["save_id"].clone();
    expect_json(
        call(&app, service_callback(&callback_uri, &late, None)).await,
        StatusCode::CONFLICT,
    );
    assert_eq!(
        mock_downloads().await,
        downloads_before_final_replay,
        "final replay or stale uncommitted callback touched the expired cache"
    );
    mock_cache_available(true).await;
    expect_json(
        call(&app, post_json(&editor_uri, &token, &open_key, &expected)).await,
        StatusCode::CONFLICT,
    );
    expect_json(
        call(&app, service_source(&source_uri)).await,
        StatusCode::CONFLICT,
    );
    let new_open = expect_json(call(&app,post_json(&editor_uri,&token,&Uuid::new_v4().to_string(),
        &json!({"version_id":final_version["version_id"],"docx_sha256":final_version["docx_sha256"]}))).await,StatusCode::OK);
    assert_ne!(new_open["session"]["editor_key"], editor_key);
    assert!(final_version["editor"]["key"].is_null());
    assert!(final_version["editor"]["pending_save_id"].is_null());
    assert!(final_version["editor"]["save_error"].is_null());
    let denied: bool=sqlx::query_scalar("SELECT NOT has_function_privilege('kb_runtime_worker','kb_bid_v2_docx_editor_command(uuid,text,jsonb,kb_actor_identity,text)','EXECUTE') AND NOT has_function_privilege('kb_runtime_worker','kb_bid_v2_get_docx_editor(uuid,uuid,kb_actor_identity)','EXECUTE')").fetch_one(&admin).await.unwrap();
    assert!(denied, "worker inherited editor capabilities");

    // Two genuine HTTP callbacks race through separate pool connections. Final
    // publication must win regardless of which snapshot reaches the lock first.
    reqwest::Client::new()
        .post(format!("{service_origin}/test/mode"))
        .json(&json!({"mode":"ok"}))
        .send()
        .await
        .unwrap();
    let fresh_key = new_open["session"]["editor_key"].as_str().unwrap();
    let fresh_callback = local_uri(
        new_open["config"]["editorConfig"]["callbackUrl"]
            .as_str()
            .unwrap(),
    );
    let race_pending=expect_json(call(&app,post_json(&force_uri,&token,&Uuid::new_v4().to_string(),
        &json!({"editor_key":fresh_key,"expected":{"version_id":final_version["version_id"],"docx_sha256":final_version["docx_sha256"]}}))).await,StatusCode::OK);
    let race_force = json!({"key":fresh_key,"status":6,"forcesavetype":0,"userdata":race_pending["save_id"],"url":format!("{service_origin}/cache/files/first.docx")});
    let race_final = json!({"key":fresh_key,"status":2,"url":format!("{service_origin}/cache/files/second.docx")});
    let (force_result, final_result) = tokio::join!(
        call(&app, service_callback(&fresh_callback, &race_force, None)),
        call(&app, service_callback(&fresh_callback, &race_final, None))
    );
    assert!(matches!(
        force_result.0,
        StatusCode::OK | StatusCode::CONFLICT
    ));
    expect_json(final_result, StatusCode::OK);
    let final_version = expect_json(
        call(&app, get(format!("{base_uri}/current"), &token)).await,
        StatusCode::OK,
    );
    assert_eq!(
        final_version["docx_sha256"],
        platform::sha256_hex(&second_bytes)
    );
    assert!(final_version["editor"]["key"].is_null());
    let new_open=expect_json(call(&app,post_json(&editor_uri,&token,&Uuid::new_v4().to_string(),
        &json!({"version_id":final_version["version_id"],"docx_sha256":final_version["docx_sha256"]}))).await,StatusCode::OK);
    let new_round = expect_json(call(&app,upload(workspace,&token,&Uuid::new_v4().to_string(),
        &json!({"basis":metadata["basis"],"expected":{"version_id":final_version["version_id"],"docx_sha256":final_version["docx_sha256"]}}),&first_bytes)).await,StatusCode::CREATED);
    let new_source = local_uri(new_open["config"]["document"]["url"].as_str().unwrap());
    expect_json(
        call(&app, service_source(&new_source)).await,
        StatusCode::CONFLICT,
    );
    mock_cache_available(false).await;
    let downloads_before_new_round_replay = mock_downloads().await;
    expect_json(
        call(&app, service_callback(&callback_uri, &final_body, None)).await,
        StatusCode::OK,
    );
    let final_current = expect_json(
        call(&app, get(format!("{base_uri}/current"), &token)).await,
        StatusCode::OK,
    );
    assert_eq!(final_current["version_id"], new_round["version_id"]);
    assert_ne!(final_current["version_id"], exact_receipt["version_id"]);
    let receipt_count_before: i64 = sqlx::query_scalar("SELECT count(*) FROM idempotency_requests")
        .fetch_one(&admin)
        .await
        .unwrap();
    assert_eq!(
        expect_json(call(&app, get(saved_uri, &token)).await, StatusCode::OK),
        exact_receipt,
        "historical receipt must not be substituted with a later save or round"
    );
    let receipt_count_after: i64 = sqlx::query_scalar("SELECT count(*) FROM idempotency_requests")
        .fetch_one(&admin)
        .await
        .unwrap();
    assert_eq!(receipt_count_after, receipt_count_before);

    assert_eq!(mock_downloads().await, downloads_before_new_round_replay);
}

fn post_json(uri: &str, token: &str, key: &str, body: &Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header("idempotency-key", key)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn local_uri(value: &str) -> String {
    let url = reqwest::Url::parse(value).unwrap();
    format!(
        "{}{}",
        url.path(),
        url.query().map(|q| format!("?{q}")).unwrap_or_default()
    )
}

fn service_callback(uri: &str, body: &Value, secret: Option<&str>) -> Request<Body> {
    let token = jsonwebtoken::encode(
        &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256),
        &json!({"payload":body,"exp":chrono::Utc::now().timestamp()+60}),
        &jsonwebtoken::EncodingKey::from_secret(
            secret
                .map(str::to_owned)
                .unwrap_or_else(|| required("KB_ONLYOFFICE_JWT_SECRET"))
                .as_bytes(),
        ),
    )
    .unwrap();
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

async fn mock_commands() -> Vec<Value> {
    reqwest::get(format!(
        "{}/test/commands",
        required("KB_ONLYOFFICE_SERVER_ORIGIN")
    ))
    .await
    .unwrap()
    .json()
    .await
    .unwrap()
}

async fn mock_cache_available(available: bool) {
    reqwest::Client::new()
        .post(format!(
            "{}/test/mode",
            required("KB_ONLYOFFICE_SERVER_ORIGIN")
        ))
        .json(&json!({"cache_available":available}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
}

async fn mock_downloads() -> Vec<Value> {
    reqwest::get(format!(
        "{}/test/downloads",
        required("KB_ONLYOFFICE_SERVER_ORIGIN")
    ))
    .await
    .unwrap()
    .error_for_status()
    .unwrap()
    .json()
    .await
    .unwrap()
}

fn service_authorization(payload: &Value, exp: Option<i64>) -> String {
    let mut claims = json!({"payload":payload});
    if let Some(exp) = exp {
        claims["exp"] = json!(exp);
    }
    let token = jsonwebtoken::encode(
        &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(required("KB_ONLYOFFICE_JWT_SECRET").as_bytes()),
    )
    .unwrap();
    format!("Bearer {token}")
}

fn service_source(uri: &str) -> Request<Body> {
    let origin = reqwest::Url::parse(&required("KB_ONLYOFFICE_API_ORIGIN")).unwrap();
    Request::builder()
        .uri(uri)
        .header(
            "authorization",
            service_authorization(
                &json!({"url":origin.join(uri).unwrap().as_str()}),
                Some(chrono::Utc::now().timestamp() + 60),
            ),
        )
        .body(Body::empty())
        .unwrap()
}
