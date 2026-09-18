#![cfg(feature = "docx-http-contract-tests")]

//! Opt-in actual HTTP -> isolated Redis -> actual worker -> mock converter ->
//! actual capability HTTP -> immutable publication/download contract.
//! The converter returns an explicitly synthetic PDF. This does not test real
//! ONLYOFFICE rendering, tender extraction, semantic correctness or page layout.
use axum::{
    Json, Router,
    extract::State,
    routing::{get, post},
};
use bidding::onlyoffice_conversion::{Config, FrozenDocxSource};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{
    collections::BTreeMap,
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use uuid::Uuid;

/// Preparation only. The separate owner starts the existing API/worker binaries;
/// this creates no project, document, analysis request or model call.
#[tokio::test]
#[ignore = "requires an owned empty database and exact deploy/.env service configuration"]
async fn prepare_isolated_full_flow_startup() {
    let root = PathBuf::from(required("KB_EXPORT_HTTP_RUN_DIR"))
        .canonicalize()
        .unwrap();
    assert!(root.starts_with(std::env::temp_dir()) && root != std::env::temp_dir());
    let env_file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../deploy/.env");
    let declared: BTreeMap<String, String> = dotenvy::from_path_iter(&env_file)
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    for name in [
        "KNOWLEDGEBRAIN_CHAT_BASE_URL",
        "KNOWLEDGEBRAIN_CHAT_API_KEY",
        "KNOWLEDGEBRAIN_CHAT_MODEL",
        "LLM_BASE_URL",
        "LLM_API_KEY",
        "LLM_MODEL",
        "KNOWLEDGEBRAIN_CHAT_REASONING_EFFORT",
        "KB_AUTHORING_MAX_OUTPUT_TOKENS",
        "KB_AUTHORING_TIMEOUT_MS",
        "KB_TENDER_AGENT_LIMITS",
    ] {
        let configured = declared.get(name).filter(|value| !value.is_empty());
        let actual = std::env::var(name).ok().filter(|value| !value.is_empty());
        assert!(
            actual.as_ref() == configured,
            "{name} must come exclusively from deploy/.env"
        );
    }
    let analysis = bidding::tender_analysis::agent::Config::from_environment().unwrap();
    let runtime_url = local_database("DATABASE_URL", "kb_runtime_api");
    let admin_url = local_database("KB_EXPORT_HTTP_ADMIN_URL", "postgres");
    assert_eq!(
        runtime_url
            .parse::<sqlx::postgres::PgConnectOptions>()
            .unwrap()
            .get_database(),
        admin_url
            .parse::<sqlx::postgres::PgConnectOptions>()
            .unwrap()
            .get_database()
    );
    let runtime = platform::connect_runtime_verified(platform::SchemaComponentKind::Api)
        .await
        .unwrap();
    let admin = PgPool::connect(&admin_url).await.unwrap();
    let projects: i64 = sqlx::query_scalar("SELECT count(*) FROM bid_projects")
        .fetch_one(&admin)
        .await
        .unwrap();
    assert_eq!(
        projects, 0,
        "the driver must create all projects and source data through product APIs"
    );
    let owner = Uuid::new_v4();
    sqlx::query("INSERT INTO users(id,email) VALUES($1,$2)")
        .bind(owner)
        .bind(format!("{owner}@example.invalid"))
        .execute(&admin)
        .await
        .unwrap();
    let token = platform::issue_jwt(owner, &required("JWT_SECRET")).unwrap();
    let origin = required("KB_REAL_FLOW_API_ORIGIN");
    let endpoint = reqwest::Url::parse(&origin).unwrap();
    assert_eq!(endpoint.host_str(), Some("127.0.0.1"));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(root.join("connection.json"))
        .unwrap();
    write!(
        file,
        "{}",
        json!({"origin":origin,"token":token,"owner_user_id":owner,
        "startup":{"env_file_sha256":platform::sha256_hex(&std::fs::read(env_file).unwrap()),
        "runtime":{"analysis":analysis}}})
    )
    .unwrap();
    file.sync_all().unwrap();
    runtime.close().await;
    admin.close().await;
}

fn required(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} required for isolated export HTTP test"))
}
fn local_database(name: &str, role: &str) -> String {
    let url = required(name);
    let options: sqlx::postgres::PgConnectOptions = url.parse().unwrap();
    assert_eq!(options.get_host(), "127.0.0.1");
    assert_eq!(options.get_username(), role);
    assert!(
        options
            .get_database()
            .unwrap()
            .starts_with("knowledgebrain_test_export_http")
    );
    url
}
async fn json_response(response: reqwest::Response, expected: StatusCode) -> Value {
    let status = response.status();
    let bytes = response.bytes().await.unwrap();
    assert_eq!(status, expected, "{}", String::from_utf8_lossy(&bytes));
    serde_json::from_slice(&bytes).unwrap()
}
async fn upload(client: &Client, url: &str, token: &str, metadata: &Value, bytes: &[u8]) -> Value {
    let boundary = Uuid::new_v4().simple().to_string();
    let mut body = format!("--{boundary}\r\nContent-Disposition: form-data; name=\"metadata\"\r\n\r\n{metadata}\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"file\"\r\nContent-Type: application/octet-stream\r\n\r\n").into_bytes();
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    json_response(
        client
            .post(url)
            .bearer_auth(token)
            .header("Idempotency-Key", Uuid::new_v4().to_string())
            .header(
                "Content-Type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(body)
            .send()
            .await
            .unwrap(),
        StatusCode::CREATED,
    )
    .await
}
#[derive(Clone)]
struct MockOffice {
    origin: String,
    api_origin: String,
    pdf: Vec<u8>,
    received: Arc<Mutex<Vec<Vec<u8>>>>,
    unexpected: Arc<AtomicUsize>,
}
async fn convert(State(state): State<MockOffice>, Json(body): Json<Value>) -> Json<Value> {
    let token = body["token"].as_str().unwrap();
    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256);
    validation.required_spec_claims.clear();
    validation.validate_exp = false;
    validation.validate_aud = false;
    let claims = jsonwebtoken::decode::<Value>(
        token,
        &jsonwebtoken::DecodingKey::from_secret(required("KB_ONLYOFFICE_JWT_SECRET").as_bytes()),
        &validation,
    )
    .unwrap()
    .claims;
    let mut unsigned = body.clone();
    unsigned.as_object_mut().unwrap().remove("token");
    assert_eq!(claims, unsigned);
    assert_eq!(body["outputtype"], "pdf");
    let source = reqwest::Url::parse(body["url"].as_str().unwrap()).unwrap();
    assert_eq!(source.origin().ascii_serialization(), state.api_origin);
    assert!(source.path().contains("/exports/requests/") && source.path().ends_with("/source"));
    let response = Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(source)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.bytes().await.unwrap().to_vec();
    state.received.lock().unwrap().push(bytes);
    Json(
        json!({"endConvert":true,"fileUrl":format!("{}/cache/files/export-http/output.pdf",state.origin)}),
    )
}
async fn unexpected_request(State(state): State<MockOffice>) -> StatusCode {
    state.unexpected.fetch_add(1, Ordering::SeqCst);
    StatusCode::NOT_FOUND
}
async fn pdf(State(state): State<MockOffice>) -> ([(String, String); 1], Vec<u8>) {
    (
        [("content-type".into(), "application/pdf".into())],
        state.pdf,
    )
}

#[tokio::test]
async fn routed_export_runs_real_worker_and_keeps_saved_source_after_later_edit() {
    let root = PathBuf::from(required("KB_EXPORT_HTTP_RUN_DIR"))
        .canonicalize()
        .unwrap();
    assert!(root.starts_with(std::env::temp_dir()) && root != std::env::temp_dir());
    let objects = PathBuf::from(required("OBJECT_DIR"))
        .canonicalize()
        .unwrap();
    assert!(objects.starts_with(&root) && objects != root);
    assert!(!platform::s3_configured());
    let admin_url = local_database("KB_EXPORT_HTTP_ADMIN_URL", "postgres");
    let runtime_url = local_database("DATABASE_URL", "kb_runtime_api");
    let worker_url = local_database("KB_EXPORT_HTTP_WORKER_URL", "kb_runtime_worker");
    let db_name = |url: &str| {
        url.parse::<sqlx::postgres::PgConnectOptions>()
            .unwrap()
            .get_database()
            .unwrap()
            .to_owned()
    };
    assert_eq!(db_name(&admin_url), db_name(&runtime_url));
    assert_eq!(db_name(&admin_url), db_name(&worker_url));
    assert!(required("REDIS_URL").starts_with("redis://127.0.0.1:"));
    let worker_bin = PathBuf::from(required("KB_EXPORT_HTTP_WORKER_BIN"))
        .canonicalize()
        .unwrap();
    let first = std::fs::read(root.join("first.docx")).unwrap();
    let second = std::fs::read(root.join("second.docx")).unwrap();
    assert_ne!(first, second);
    let pdf_bytes = std::fs::read(root.join("mock.pdf")).unwrap();
    let descriptor: platform::ReleaseDescriptorV1 = serde_json::from_str(include_str!(
        "../../../deploy/release-descriptor-v1.development.json"
    ))
    .unwrap();
    let descriptor_path = root.join("release-descriptor.json");
    std::fs::write(&descriptor_path, serde_json::to_vec(&descriptor).unwrap()).unwrap();
    let namespace = Uuid::new_v4().to_string();
    let descriptor_sha = descriptor.sha256().unwrap();
    let identity = platform::SchemaRuntimeIdentity::from_descriptor(
        descriptor_path.clone(),
        descriptor.clone(),
        &descriptor_sha,
        "migrator",
        descriptor
            .component_digest(platform::SchemaComponentKind::Migrator)
            .unwrap(),
        &namespace,
    )
    .unwrap();
    let admin = PgPool::connect(&admin_url).await.unwrap();
    platform::apply_fresh_baseline_with_identity(&admin, &identity)
        .await
        .unwrap();
    let owner = Uuid::new_v4();
    sqlx::query("INSERT INTO users(id,email) VALUES($1,$2)")
        .bind(owner)
        .bind(format!("{owner}@example.invalid"))
        .execute(&admin)
        .await
        .unwrap();
    let secret = Uuid::new_v4().to_string();
    let token = platform::issue_jwt(owner, &secret).unwrap();
    let wrong = platform::issue_jwt(Uuid::new_v4(), &secret).unwrap();
    let api_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let api_origin = format!("http://{}", api_listener.local_addr().unwrap());
    let office_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let office_origin = format!("http://{}", office_listener.local_addr().unwrap());
    // Test process only: no production configuration or .env is changed.
    unsafe {
        std::env::set_var("KB_DEPLOYMENT_NAMESPACE_ID", &namespace);
        std::env::set_var("KB_ONLYOFFICE_SERVER_ORIGIN", &office_origin);
        std::env::set_var("KB_ONLYOFFICE_COMMAND_ORIGIN", &office_origin);
        std::env::set_var("KB_ONLYOFFICE_API_ORIGIN", &api_origin);
        std::env::set_var("KB_ONLYOFFICE_JWT_SECRET", Uuid::new_v4().to_string());
        std::env::set_var(
            "KB_ONLYOFFICE_CAPABILITY_SECRET",
            Uuid::new_v4().to_string(),
        );
        std::env::set_var("KB_ONLYOFFICE_TOKEN_TTL_SECONDS", "60");
        std::env::set_var("KB_ONLYOFFICE_HTTP_TIMEOUT_SECONDS", "10");
    }
    let app = api::router_with(api::AppState {
        jwt_secret: secret,
        bootstrap_key: String::new(),
    });
    let api_task = tokio::spawn(async move {
        axum::serve(api_listener, app).await.unwrap();
    });
    let received = Arc::new(Mutex::new(Vec::new()));
    let unexpected = Arc::new(AtomicUsize::new(0));
    let mock = MockOffice {
        origin: office_origin.clone(),
        api_origin: api_origin.clone(),
        pdf: pdf_bytes.clone(),
        received: received.clone(),
        unexpected: unexpected.clone(),
    };
    let office = Router::new()
        .route("/converter", post(convert))
        .route("/cache/files/export-http/output.pdf", get(pdf))
        .fallback(unexpected_request)
        .with_state(mock);
    let office_task = tokio::spawn(async move {
        axum::serve(office_listener, office).await.unwrap();
    });
    let client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(20))
        .build()
        .unwrap();
    let project = json_response(
        client
            .post(format!("{api_origin}/api/v2/bid-projects"))
            .bearer_auth(&token)
            .header("Idempotency-Key", Uuid::new_v4().to_string())
            .json(&json!({"title":"isolated export HTTP worker contract"}))
            .send()
            .await
            .unwrap(),
        StatusCode::CREATED,
    )
    .await;
    let workspace = project["workspace_id"].as_str().unwrap();
    let base = format!("{api_origin}/api/v2/submission-workspaces/{workspace}");
    let basis = json_response(
        client
            .get(format!("{base}/docx-rounds/basis"))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap(),
        StatusCode::OK,
    )
    .await;
    let initial = upload(
        &client,
        &format!("{base}/docx-rounds"),
        &token,
        &json!({"basis":basis,"expected":null}),
        &first,
    )
    .await;
    let body = json!({"version_id":initial["version_id"]});
    let digest = initial["docx_sha256"].as_str().unwrap();
    let export = |auth: &str, key: &str, sha: &str| {
        client
            .post(format!("{base}/exports"))
            .bearer_auth(auth)
            .header("Idempotency-Key", key)
            .header("If-Match", sha)
            .json(&body)
    };
    json_response(
        client
            .post(format!("{base}/exports"))
            .json(&body)
            .send()
            .await
            .unwrap(),
        StatusCode::UNAUTHORIZED,
    )
    .await;
    json_response(
        export(&wrong, "wrong-owner", digest).send().await.unwrap(),
        StatusCode::FORBIDDEN,
    )
    .await;
    json_response(
        client
            .post(format!("{base}/exports"))
            .bearer_auth(&token)
            .header("Idempotency-Key", "missing-sha")
            .json(&body)
            .send()
            .await
            .unwrap(),
        StatusCode::PRECONDITION_REQUIRED,
    )
    .await;
    json_response(
        export(&token, "stale-sha", &"0".repeat(64))
            .send()
            .await
            .unwrap(),
        StatusCode::CONFLICT,
    )
    .await;
    let key = Uuid::new_v4().to_string();
    let accepted = json_response(
        export(&token, &key, digest).send().await.unwrap(),
        StatusCode::ACCEPTED,
    )
    .await;
    assert_eq!(accepted["source"]["version_id"], initial["version_id"]);
    assert_eq!(accepted["source"]["docx_sha256"], digest);
    let request_id = accepted["request_artifact_id"].as_str().unwrap();
    let request_url = format!("{base}/requests/{request_id}");
    assert_eq!(
        json_response(
            export(&token, &key, digest).send().await.unwrap(),
            StatusCode::ACCEPTED
        )
        .await,
        accepted
    );
    let source = FrozenDocxSource {
        workspace_id: workspace.parse().unwrap(),
        request_id: request_id.parse().unwrap(),
        version_id: initial["version_id"].as_str().unwrap().parse().unwrap(),
        docx_sha256: digest.into(),
        byte_length: first.len() as u64,
    };
    let config = Config::load().unwrap();
    let source_url = config.source_url(&source).unwrap();
    assert_eq!(
        client
            .get(source_url.clone())
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap(),
        first
    );
    let later = upload(&client, &format!("{base}/docx-rounds"), &token,
        &json!({"basis":basis,"expected":{"version_id":initial["version_id"],"docx_sha256":digest}}), &second).await;
    assert_ne!(later["docx_sha256"], digest);
    assert_eq!(
        client
            .get(source_url.clone())
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap(),
        first
    );
    json_response(
        export(&token, "stale-version", digest)
            .send()
            .await
            .unwrap(),
        StatusCode::CONFLICT,
    )
    .await;
    assert_eq!(
        json_response(
            export(&token, &key, digest).send().await.unwrap(),
            StatusCode::ACCEPTED
        )
        .await,
        accepted
    );
    for tampered in [
        source_url
            .as_str()
            .replace(workspace, &Uuid::new_v4().to_string()),
        source_url
            .as_str()
            .replace(request_id, &Uuid::new_v4().to_string()),
    ] {
        assert_eq!(
            client.get(tampered).send().await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
    }
    let mut changed = source.clone();
    changed.version_id = later["version_id"].as_str().unwrap().parse().unwrap();
    assert_eq!(
        client
            .get(config.source_url(&changed).unwrap())
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    changed = source.clone();
    changed.docx_sha256 = later["docx_sha256"].as_str().unwrap().into();
    assert_eq!(
        client
            .get(config.source_url(&changed).unwrap())
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    changed = source.clone();
    changed.byte_length += 1;
    assert_eq!(
        client
            .get(config.source_url(&changed).unwrap())
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let mut no_token = source_url.clone();
    no_token.set_query(None);
    assert_eq!(
        client.get(no_token).send().await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    let mut bad_token = source_url.clone();
    bad_token
        .query_pairs_mut()
        .clear()
        .append_pair("token", "invalid-test-capability");
    assert_eq!(
        client.get(bad_token).send().await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let worker_log = std::fs::File::create(root.join("worker.log")).unwrap();
    let child_environment = [
        "REDIS_URL",
        "KB_DEPLOYMENT_NAMESPACE_ID",
        "KB_ONLYOFFICE_SERVER_ORIGIN",
        "KB_ONLYOFFICE_COMMAND_ORIGIN",
        "KB_ONLYOFFICE_API_ORIGIN",
        "KB_ONLYOFFICE_JWT_SECRET",
        "KB_ONLYOFFICE_CAPABILITY_SECRET",
        "KB_ONLYOFFICE_TOKEN_TTL_SECONDS",
        "KB_ONLYOFFICE_HTTP_TIMEOUT_SECONDS",
    ]
    .map(|name| (name, required(name)));
    let mut child = tokio::process::Command::new(worker_bin)
        .env_clear()
        .envs(child_environment)
        .env("OBJECT_DIR", &objects)
        .current_dir(&root)
        .kill_on_drop(true)
        .env("DATABASE_URL", worker_url)
        .env("KB_RELEASE_DESCRIPTOR_PATH", &descriptor_path)
        .env("KB_RELEASE_DESCRIPTOR_SHA256", &descriptor_sha)
        .env("KB_COMPONENT_KIND", "worker")
        .env(
            "KB_COMPONENT_IMAGE_DIGEST",
            descriptor
                .component_digest(platform::SchemaComponentKind::Worker)
                .unwrap(),
        )
        .env("KNOWLEDGEBRAIN_WORKER_PROBE_ADDR", "127.0.0.1:0")
        .env("KNOWLEDGEBRAIN_CHAT_BASE_URL", &office_origin)
        .env("KNOWLEDGEBRAIN_CHAT_API_KEY", "isolated-no-model-calls")
        .env(
            "KNOWLEDGEBRAIN_CHAT_MODEL",
            "isolated-export-no-model-calls",
        )
        .stdout(worker_log.try_clone().unwrap())
        .stderr(worker_log)
        .spawn()
        .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(90), async {
        loop {
            assert!(
                child.try_wait().unwrap().is_none(),
                "worker exited; inspect owned worker.log"
            );
            let status = json_response(
                client
                    .get(&request_url)
                    .bearer_auth(&token)
                    .send()
                    .await
                    .unwrap(),
                StatusCode::OK,
            )
            .await;
            if status["status"] != "pending" {
                break status;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("worker did not publish export within 90 seconds");
    assert_eq!(result["status"], "succeeded", "{result}");
    assert_eq!(*received.lock().unwrap(), vec![first.clone()]);
    assert_eq!(
        unexpected.load(Ordering::SeqCst),
        0,
        "export must not call a model or another endpoint"
    );
    let package = &result["result_identity"];
    assert_eq!(package["source"], accepted["source"]);
    for (format, expected) in [("docx", &first), ("pdf", &pdf_bytes)] {
        let output = &package["outputs"][format];
        let output_id = output["artifact_id"].as_str().unwrap();
        let url = format!("{base}/exports/{output_id}/download");
        assert_eq!(
            client.get(&url).send().await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            client
                .get(&url)
                .bearer_auth(&wrong)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        let response = client.get(&url).bearer_auth(&token).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.bytes().await.unwrap();
        assert_eq!(bytes, *expected);
        assert_eq!(output["sha256"], platform::sha256_hex(&bytes));
        std::fs::write(root.join(format!("output.{format}")), bytes).unwrap();
    }
    let manifest = package["manifest_id"].as_str().unwrap();
    let report_url = format!("{base}/exports/{manifest}/assessment-report");
    assert_eq!(
        client.get(&report_url).send().await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        client
            .get(&report_url)
            .bearer_auth(&wrong)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let report = json_response(
        client
            .get(&report_url)
            .bearer_auth(&token)
            .send()
            .await
            .unwrap(),
        StatusCode::OK,
    )
    .await;
    assert_eq!(report["source"], accepted["source"]);
    assert_eq!(report["outputs"], package["outputs"]);
    assert_eq!(report["status"], "needs_review");
    assert_eq!(
        report["content_sha256"],
        package["assessment_report_sha256"]
    );
    // The existing report API adds the stored digest to its JSON envelope;
    // the stored payload uses the baseline's PostgreSQL JSON serialization.
    let canonical: Vec<u8> = sqlx::query_scalar(
        "SELECT canonical_payload FROM bid_submission_assessment_report_artifacts WHERE id=$1",
    )
    .bind(Uuid::parse_str(package["assessment_report_id"].as_str().unwrap()).unwrap())
    .fetch_one(&admin)
    .await
    .unwrap();
    assert_eq!(
        platform::sha256_hex(&canonical),
        package["assessment_report_sha256"]
    );
    let mut report_content = report.clone();
    report_content
        .as_object_mut()
        .unwrap()
        .remove("content_sha256");
    assert_eq!(
        report_content,
        serde_json::from_slice::<Value>(&canonical).unwrap()
    );
    for format in ["docx", "pdf"] {
        let output = package["outputs"][format]["artifact_id"].as_str().unwrap();
        assert_eq!(
            json_response(
                client
                    .get(format!("{base}/exports/{output}/assessment-report"))
                    .bearer_auth(&token)
                    .send()
                    .await
                    .unwrap(),
                StatusCode::OK,
            )
            .await,
            report,
        );
    }
    let list = json_response(
        client
            .get(format!("{base}/exports"))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap(),
        StatusCode::OK,
    )
    .await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["manifest_id"], manifest);
    assert_eq!(
        json_response(
            client
                .get(format!("{base}/docx/current"))
                .bearer_auth(&token)
                .send()
                .await
                .unwrap(),
            StatusCode::OK
        )
        .await["version_id"],
        later["version_id"]
    );
    std::fs::write(
        root.join("assessment-report.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    std::fs::write(root.join("evidence.json"), serde_json::to_vec_pretty(&json!({"status":"passed","actual_worker":true,
        "actual_onlyoffice":false,"converter":"mock; actual HTTP source retrieval","model_calls":0,
        "frozen_version":initial,"later_version":later,"request":result,"source_fetches":received.lock().unwrap().len()})).unwrap()).unwrap();
    child.kill().await.unwrap();
    child.wait().await.unwrap();
    api_task.abort();
    office_task.abort();
    admin.close().await;
}
