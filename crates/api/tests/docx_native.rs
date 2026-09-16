#![cfg(feature = "docx-native-tests")]

//! Opt-in real Document Server test. The runner owns all infrastructure; this
//! target serves the product router without adding any test HTTP endpoints.
use serde_json::{Value, json};
use sqlx::PgPool;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use uuid::Uuid;

async fn observe(
    axum::extract::State(path): axum::extract::State<PathBuf>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    // Public evidence contains paths only. Successful save notifications are
    // retained in a separate private, temporary file for authentic redelivery.
    let method = request.method().to_string();
    let uri = request.uri().path().to_string();
    let source_capture = if method == "GET" && uri.ends_with("/source") {
        request.headers().get("authorization").and_then(|v| v.to_str().ok())
            .map(|authorization| json!({"uri":request.uri().to_string(),"authorization":authorization}))
    } else {
        None
    };
    let (request, capture) = if method == "POST" && uri.ends_with("/callback") {
        let (parts, body) = request.into_parts();
        let bytes = axum::body::to_bytes(body, platform::max_file_bytes())
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        let capture = if matches!(body["status"].as_u64(), Some(2 | 6)) {
            Some(
                json!({"uri":parts.uri.to_string(),"authorization":parts.headers["authorization"].to_str().unwrap(),"body":body}),
            )
        } else {
            None
        };
        (
            axum::extract::Request::from_parts(parts, axum::body::Body::from(bytes)),
            capture,
        )
    } else {
        (request, None)
    };
    let response = next.run(request).await;
    if response.status().is_success()
        && let Some(capture) = source_capture
    {
        let mut private = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(path.with_file_name("source-tickets.jsonl"))
            .unwrap();
        writeln!(private, "{capture}").unwrap();
    }
    if response.status().is_success()
        && let Some(capture) = capture
    {
        let mut private = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(path.with_file_name("callback-tickets.jsonl"))
            .unwrap();
        writeln!(private, "{capture}").unwrap();
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    writeln!(
        file,
        "{}",
        json!({"method":method,"path":uri,"status":response.status().as_u16()})
    )
    .unwrap();
    response
}

fn required(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is required for native DOCX testing"))
}

#[tokio::test]
async fn real_document_server_publishes_and_reopens_product_versions() {
    let root = PathBuf::from(required("KB_DOCX_NATIVE_RUN_DIR"))
        .canonicalize()
        .unwrap();
    assert!(root.starts_with(std::env::temp_dir()) && root != std::env::temp_dir());
    let objects = PathBuf::from(required("OBJECT_DIR"))
        .canonicalize()
        .unwrap();
    assert!(objects.starts_with(&root) && objects != root);
    assert!(!platform::s3_configured());
    let admin_url = required("KNOWLEDGEBRAIN_TEST_DATABASE_URL");
    for url in [&admin_url, &required("DATABASE_URL")] {
        let options: sqlx::postgres::PgConnectOptions = url.parse().unwrap();
        assert_eq!(options.get_host(), "127.0.0.1");
        assert!(
            options
                .get_database()
                .unwrap()
                .starts_with("knowledgebrain_test_")
        );
    }
    let admin = PgPool::connect(&admin_url).await.unwrap();
    // The real worker requires the actual release/catalog receipt. Never seed a
    // fake readiness row or let the Office probe bypass runtime admission.
    let descriptor: platform::ReleaseDescriptorV1 = serde_json::from_str(include_str!(
        "../../../deploy/release-descriptor-v1.development.json"
    ))
    .unwrap();
    let descriptor_path = root.join("release-descriptor.json");
    std::fs::write(&descriptor_path, serde_json::to_vec(&descriptor).unwrap()).unwrap();
    let descriptor_sha = descriptor.sha256().unwrap();
    let admission = platform::SchemaRuntimeIdentity::from_descriptor(
        descriptor_path.clone(),
        descriptor.clone(),
        &descriptor_sha,
        "migrator",
        descriptor
            .component_digest(platform::SchemaComponentKind::Migrator)
            .unwrap(),
        &required("KB_DEPLOYMENT_NAMESPACE_ID"),
    )
    .unwrap();
    platform::apply_fresh_baseline_with_identity(&admin, &admission)
        .await
        .unwrap();
    let fixture = std::fs::read_to_string(root.join("document_collection_acceptance.sql")).unwrap();
    let fixture = fixture
        .lines()
        .filter(|line| !line.starts_with("\\set "))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(fixture.matches("\nROLLBACK;").count(), 1);
    sqlx::raw_sql(sqlx::AssertSqlSafe(
        fixture.replace("\nROLLBACK;", "\nCOMMIT;"),
    ))
    .execute(&admin)
    .await
    .unwrap();
    let runtime = platform::connect().await.unwrap();
    let role: String = sqlx::query_scalar("SELECT current_user")
        .fetch_one(&runtime)
        .await
        .unwrap();
    assert_eq!(role, "kb_runtime_api");
    let fixtures: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('workspace',w.id,'owner',p.owner_user_id) FROM bid_submission_workspaces w JOIN bid_projects p ON p.id=w.project_id")
        .fetch_all(&admin).await.unwrap();
    assert_eq!(fixtures.len(), 1, "dedicated collection fixture required");
    let owner = Uuid::parse_str(fixtures[0]["owner"].as_str().unwrap()).unwrap();
    let secret = Uuid::new_v4().to_string();
    let token = platform::issue_jwt(owner, &secret).unwrap();
    let listener = tokio::net::TcpListener::bind(required("KB_DOCX_NATIVE_BIND"))
        .await
        .unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    assert_eq!(origin, required("KB_ONLYOFFICE_API_ORIGIN"));
    let app = api::router_with(api::AppState {
        jwt_secret: secret,
        bootstrap_key: String::new(),
    })
    .layer(axum::middleware::from_fn_with_state(
        root.join("http-events.jsonl"),
        observe,
    ));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let ticket = root.join("browser-ticket.json");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&ticket)
        .unwrap();
    write!(
        file,
        "{}",
        json!({"origin": origin, "workspace": fixtures[0]["workspace"], "token": token,
            "object_directory": platform::blob_path(&platform::sha256_hex(b"native-test-locator")).unwrap().parent().unwrap()})
    )
    .unwrap();
    drop(file);
    let mut worker = if let Ok(binary) = std::env::var("KB_DOCX_NATIVE_WORKER_BIN") {
        let options: sqlx::postgres::PgConnectOptions =
            required("KB_DOCX_NATIVE_WORKER_URL").parse().unwrap();
        assert_eq!(options.get_host(), "127.0.0.1");
        assert_eq!(options.get_username(), "kb_runtime_worker");
        assert_eq!(
            options.get_database(),
            admin_url
                .parse::<sqlx::postgres::PgConnectOptions>()
                .unwrap()
                .get_database()
        );
        let log = std::fs::File::create(root.join("worker.log")).unwrap();
        let environment = [
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
        .into_iter()
        .filter_map(|key| std::env::var(key).ok().map(|value| (key, value)));
        Some(
            tokio::process::Command::new(binary)
                .env_clear()
                .envs(environment)
                .current_dir(&root)
                .kill_on_drop(true)
                .env("DATABASE_URL", required("KB_DOCX_NATIVE_WORKER_URL"))
                .env("OBJECT_DIR", &objects)
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
                // This isolated Office probe never uses a model. A mistaken model
                // operation must fail locally, never inherit provider credentials.
                .env("KNOWLEDGEBRAIN_CHAT_BASE_URL", "http://127.0.0.1:1/v1")
                .env("KNOWLEDGEBRAIN_CHAT_API_KEY", "isolated-office-no-model")
                .env("KNOWLEDGEBRAIN_CHAT_MODEL", "isolated-office-no-model")
                .stdout(log.try_clone().unwrap())
                .stderr(log)
                .spawn()
                .unwrap(),
        )
    } else {
        None
    };
    let status = tokio::process::Command::new(required("KB_DOCX_NATIVE_PYTHON"))
        .arg(required("KB_DOCX_NATIVE_DRIVER"))
        .arg("drive")
        .arg(&root)
        .status()
        .await;
    if let Some(child) = &mut worker {
        child.start_kill().unwrap();
        child.wait().await.unwrap();
    }
    server.abort();
    let _ = server.await;
    std::fs::remove_file(ticket).unwrap();
    let captures = root.join("callback-tickets.jsonl");
    if captures.exists() {
        std::fs::remove_file(captures).unwrap();
    }
    let captures = root.join("source-tickets.jsonl");
    if captures.exists() {
        std::fs::remove_file(captures).unwrap();
    }
    assert!(status.unwrap().success(), "native browser driver failed");
    let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM object_upload_staging")
        .fetch_one(&admin)
        .await
        .unwrap();
    assert_eq!(pending, 0, "successful native saves left staging rows");
}
