#![cfg(feature = "docx-http-contract-tests")]
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

fn request(
    method: &str,
    path: &str,
    token: &str,
    key: Option<&str>,
    body: &Value,
) -> Request<Body> {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {token}"));
    if let Some(key) = key {
        request = request.header("idempotency-key", key);
    }
    request
        .header("content-type", "application/json")
        .body(if method == "GET" {
            Body::empty()
        } else {
            Body::from(body.to_string())
        })
        .unwrap()
}
async fn call(app: &axum::Router, request: Request<Body>, status: StatusCode) -> Value {
    let response = app.clone().oneshot(request).await.unwrap();
    let actual = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(actual, status, "{}", String::from_utf8_lossy(&bytes));
    serde_json::from_slice(&bytes).unwrap()
}
#[tokio::test]
async fn composition_http_replays_original_intent_and_exposes_scoped_progress() {
    let path = std::path::PathBuf::from(
        std::env::var("KB_COMPOSITION_HTTP_FIXTURE").expect("explicit fixture required"),
    );
    assert!(path.is_absolute() && path.starts_with(std::env::temp_dir()));
    let fixture: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let admin_url = std::env::var("KB_TENDER_AGENT_TEST_DATABASE_URL").unwrap();
    let options: sqlx::postgres::PgConnectOptions = admin_url.parse().unwrap();
    assert_eq!(options.get_host(), "127.0.0.1");
    assert!(
        options
            .get_database()
            .unwrap()
            .starts_with("knowledgebrain_test_")
    );
    assert_eq!(
        std::env::var("DATABASE_URL")
            .unwrap()
            .parse::<sqlx::postgres::PgConnectOptions>()
            .unwrap()
            .get_username(),
        "kb_runtime_api"
    );
    assert!(!platform::s3_configured());
    let admin = PgPool::connect(&admin_url).await.unwrap();
    let workspace = fixture["workspace"].as_str().unwrap();
    let actor = fixture["actor"].as_str().unwrap();
    let owner = Uuid::parse_str(actor.strip_prefix("user:").unwrap()).unwrap();
    let secret = Uuid::new_v4().to_string();
    let token = platform::issue_jwt(owner, &secret).unwrap();
    let wrong = platform::issue_jwt(Uuid::new_v4(), &secret).unwrap();
    let app = api::router_with(api::AppState {
        jwt_secret: secret,
        bootstrap_key: String::new(),
    });
    let base = format!("/api/v2/submission-workspaces/{workspace}/docx-compositions");
    let body = json!({"basis":fixture["basis"],"expected":{"version_id":fixture["current"]["version_id"],"docx_sha256":fixture["current"]["docx_sha256"]}});
    assert_eq!(
        call(
            &app,
            request("GET", &format!("{base}/basis"), &token, None, &Value::Null),
            StatusCode::OK
        )
        .await,
        fixture["basis"]
    );
    let latest = call(
        &app,
        request("GET", &format!("{base}/latest"), &token, None, &Value::Null),
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        latest["request_artifact_id"],
        fixture["completed"]["request_artifact_id"]
    );
    assert_eq!(latest["status"], "succeeded");
    assert_eq!(latest["result_identity"], fixture["current"]);
    call(
        &app,
        request("GET", &format!("{base}/latest"), &wrong, None, &Value::Null),
        StatusCode::FORBIDDEN,
    )
    .await;
    call(
        &app,
        request("POST", &base, &wrong, Some("forbidden"), &body),
        StatusCode::FORBIDDEN,
    )
    .await;
    call(
        &app,
        request("POST", &base, &token, None, &body),
        StatusCode::BAD_REQUEST,
    )
    .await;
    let key = Uuid::new_v4().to_string();
    // No fallback budgets/model; configuration failure creates no request.
    let count_before: i64 =
        sqlx::query_scalar("SELECT count(*) FROM bid_docx_composition_request_identities")
            .fetch_one(&admin)
            .await
            .unwrap();
    unsafe {
        std::env::remove_var("KB_DOCX_COMPOSITION_LIMITS");
    }
    let unavailable = call(
        &app,
        request("POST", &base, &token, Some(&key), &body),
        StatusCode::SERVICE_UNAVAILABLE,
    )
    .await;
    assert_eq!(unavailable["error"]["code"], "AGENT_PROVIDER_UNAVAILABLE");
    let count_after: i64 =
        sqlx::query_scalar("SELECT count(*) FROM bid_docx_composition_request_identities")
            .fetch_one(&admin)
            .await
            .unwrap();
    assert_eq!(count_before, count_after);
    unsafe {
        std::env::set_var(
            "KB_DOCX_COMPOSITION_LIMITS",
            fixture["config"]["limits"].to_string(),
        );
    }
    let redis = std::env::var("REDIS_URL").unwrap();
    assert!(redis.starts_with("redis://127.0.0.1:"));
    unsafe {
        std::env::set_var("REDIS_URL", "redis://127.0.0.1:1");
    }
    let unavailable = call(
        &app,
        request("POST", &base, &token, Some(&key), &body),
        StatusCode::SERVICE_UNAVAILABLE,
    )
    .await;
    assert_eq!(unavailable["error"]["code"], "QUEUE_UNAVAILABLE");
    let id = unavailable["error"]["details"]["request_artifact_id"]
        .as_str()
        .unwrap();
    let uri = format!("{base}/{id}");
    let original:Value=sqlx::query_scalar("SELECT frozen_input FROM bid_docx_composition_request_identities WHERE request_artifact_id=$1").bind(Uuid::parse_str(id).unwrap()).fetch_one(&admin).await.unwrap();
    let model = std::env::var("KNOWLEDGEBRAIN_CHAT_MODEL").unwrap();
    unsafe {
        std::env::set_var("REDIS_URL", &redis);
        std::env::set_var("KNOWLEDGEBRAIN_CHAT_MODEL", "changed-test-configuration");
        std::env::set_var("KB_DOCX_COMPOSITION_LIMITS", "");
    }
    let accepted = call(
        &app,
        request("POST", &base, &token, Some(&key), &body),
        StatusCode::ACCEPTED,
    )
    .await;
    assert_eq!(accepted["request_artifact_id"], id);
    assert_eq!(
        call(
            &app,
            request("POST", &base, &token, Some(&key), &body),
            StatusCode::ACCEPTED
        )
        .await,
        accepted
    );
    let unchanged:Value=sqlx::query_scalar("SELECT frozen_input FROM bid_docx_composition_request_identities WHERE request_artifact_id=$1").bind(Uuid::parse_str(id).unwrap()).fetch_one(&admin).await.unwrap();
    assert_eq!(original, unchanged);
    assert_eq!(original["config"]["provider"]["model_id"], model);
    let storage = platform::oxana_connect().unwrap();
    assert_eq!(
        storage
            .enqueued_count(platform::BidAuthoringV2Queue)
            .await
            .unwrap(),
        1
    );
    let mut changed = body.clone();
    changed["basis"]["document_set_sha256"] = Value::String("0".repeat(64));
    let mismatch = call(
        &app,
        request("POST", &base, &token, Some(&key), &changed),
        StatusCode::CONFLICT,
    )
    .await;
    assert_eq!(mismatch["error"]["code"], "IDEMPOTENCY_PAYLOAD_MISMATCH");
    let pending = call(
        &app,
        request("GET", &uri, &token, None, &Value::Null),
        StatusCode::OK,
    )
    .await;
    assert_eq!(pending["status"], "pending");
    assert!(pending["progress"].is_null());
    call(
        &app,
        request("GET", &uri, &wrong, None, &Value::Null),
        StatusCode::FORBIDDEN,
    )
    .await;
    call(
        &app,
        request(
            "GET",
            &format!("{base}/{}", Uuid::new_v4()),
            &token,
            None,
            &Value::Null,
        ),
        StatusCode::NOT_FOUND,
    )
    .await;
    let worker = PgPool::connect_with(options.username("kb_runtime_worker"))
        .await
        .unwrap();
    let _: Value = sqlx::query_scalar("SELECT kb_bid_v2_tender_agent_claim($1,$2,$3::kb_sha256)")
        .bind(Uuid::parse_str(id).unwrap())
        .bind(accepted["request_revision"].as_i64().unwrap())
        .bind(accepted["frozen_input_sha256"].as_str().unwrap())
        .fetch_one(&worker)
        .await
        .unwrap();
    let running = call(
        &app,
        request("GET", &uri, &token, None, &Value::Null),
        StatusCode::OK,
    )
    .await;
    assert_eq!(running["progress"]["phase"], "drafting");
    assert_eq!(running["progress"]["attempt"], 1);
    assert!(running.get("config").is_none());
    assert!(running.get("frozen_input").is_none());
    let project = call(
        &app,
        request(
            "POST",
            "/api/v2/bid-projects",
            &token,
            Some(&Uuid::new_v4().to_string()),
            &json!({"title":"isolated composition ownership test"}),
        ),
        StatusCode::CREATED,
    )
    .await;
    let other = project["workspace_id"].as_str().unwrap();
    call(
        &app,
        request(
            "GET",
            &format!("/api/v2/submission-workspaces/{other}/docx-compositions/{id}"),
            &token,
            None,
            &Value::Null,
        ),
        StatusCode::NOT_FOUND,
    )
    .await;
    assert!(
        call(
            &app,
            request(
                "GET",
                &format!("/api/v2/submission-workspaces/{other}/docx-compositions/basis"),
                &token,
                None,
                &Value::Null
            ),
            StatusCode::OK
        )
        .await
        .is_null()
    );
    // Original intent replay is independent of new source heads as well.
    let docs: Vec<Uuid> = fixture["documents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| Uuid::parse_str(d["document_id"].as_str().unwrap()).unwrap())
        .collect();
    let project_id: Uuid =
        sqlx::query_scalar("SELECT project_id FROM bid_submission_workspaces WHERE id=$1")
            .bind(Uuid::parse_str(workspace).unwrap())
            .fetch_one(&admin)
            .await
            .unwrap();
    let bytes = b"HTTP composition source replacement";
    let sha = platform::sha256_hex(bytes);
    let _:Value=sqlx::query_scalar("SELECT kb_bid_v2_freeze_document_set($1,$2,$3,$4::kb_sha256,$5,$6::kb_actor_identity,$7,$8,$9::kb_sha256,$10)")
        .bind(project_id).bind(docs).bind(Uuid::parse_str(fixture["basis"]["document_set_id"].as_str().unwrap()).unwrap()).bind(fixture["basis"]["document_set_sha256"].as_str().unwrap())
        .bind(Uuid::new_v4()).bind(actor).bind(Uuid::new_v4().to_string()).bind(bytes.as_slice()).bind(sha).bind(&fixture["tender_runtime"]).fetch_one(&admin).await.unwrap();
    assert_eq!(
        call(
            &app,
            request("POST", &base, &token, Some(&key), &body),
            StatusCode::ACCEPTED
        )
        .await,
        accepted
    );
    unsafe {
        std::env::set_var("KNOWLEDGEBRAIN_CHAT_MODEL", &model);
        std::env::set_var(
            "KB_DOCX_COMPOSITION_LIMITS",
            fixture["config"]["limits"].to_string(),
        );
    }
    call(
        &app,
        request(
            "POST",
            &base,
            &token,
            Some(&Uuid::new_v4().to_string()),
            &body,
        ),
        StatusCode::CONFLICT,
    )
    .await;
    let current: Uuid =
        sqlx::query_scalar("SELECT version_id FROM bid_docx_current WHERE scope_id=$1")
            .bind(Uuid::parse_str(workspace).unwrap())
            .fetch_one(&admin)
            .await
            .unwrap();
    assert_eq!(current.to_string(), fixture["current"]["version_id"]);
    worker.close().await;
    admin.close().await;
}
