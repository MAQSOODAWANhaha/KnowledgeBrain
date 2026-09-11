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
    let status = tokio::process::Command::new(required("KB_DOCX_NATIVE_PYTHON"))
        .arg(required("KB_DOCX_NATIVE_DRIVER"))
        .arg("drive")
        .arg(&root)
        .status()
        .await;
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
