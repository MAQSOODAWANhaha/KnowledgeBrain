#![cfg(feature = "docx-http-contract-tests")]

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use bidding::docx_round::{self, CreateDocxRound, DocxVersionIdentity, InitialDocx};
use serde_json::Value;
use std::path::PathBuf;
use tower::ServiceExt;
use uuid::Uuid;

fn request(path: &str, token: Option<&str>) -> Request<Body> {
    let mut request = Request::builder().uri(path);
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    request.body(Body::empty()).unwrap()
}

async fn error(app: &axum::Router, request: Request<Body>, expected: StatusCode) -> String {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(status, expected, "{}", String::from_utf8_lossy(&bytes));
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    body["error"]["code"].as_str().unwrap().into()
}

struct RestoreObject {
    path: PathBuf,
    bytes: Vec<u8>,
}

impl Drop for RestoreObject {
    fn drop(&mut self) {
        std::fs::write(&self.path, &self.bytes).expect("restore owned report fixture");
    }
}

#[tokio::test]
async fn composition_report_is_authorized_verified_and_bound_to_its_published_version() {
    let fixture_path = PathBuf::from(std::env::var("KB_COMPOSITION_HTTP_FIXTURE").unwrap());
    assert!(fixture_path.is_absolute() && fixture_path.starts_with(std::env::temp_dir()));
    let fixture: Value = serde_json::from_slice(&std::fs::read(fixture_path).unwrap()).unwrap();
    let admin_url = std::env::var("KB_TENDER_AGENT_TEST_DATABASE_URL").unwrap();
    let options: sqlx::postgres::PgConnectOptions = admin_url.parse().unwrap();
    assert_eq!(options.get_host(), "127.0.0.1");
    assert!(
        options
            .get_database()
            .unwrap()
            .starts_with("knowledgebrain_test_")
    );
    let runtime_options: sqlx::postgres::PgConnectOptions =
        std::env::var("DATABASE_URL").unwrap().parse().unwrap();
    assert_eq!(runtime_options.get_username(), "kb_runtime_api");
    assert_eq!(runtime_options.get_host(), options.get_host());
    assert_eq!(runtime_options.get_database(), options.get_database());
    let object_dir = PathBuf::from(std::env::var("OBJECT_DIR").unwrap())
        .canonicalize()
        .unwrap();
    assert!(object_dir.starts_with(std::env::temp_dir()) && object_dir != std::env::temp_dir());
    assert!(!platform::s3_configured());

    let runtime = platform::connect().await.unwrap();
    let workspace = Uuid::parse_str(fixture["workspace"].as_str().unwrap()).unwrap();
    let version = Uuid::parse_str(fixture["current"]["version_id"].as_str().unwrap()).unwrap();
    let actor = fixture["actor"].as_str().unwrap();
    let owner = Uuid::parse_str(actor.strip_prefix("user:").unwrap()).unwrap();
    let secret = Uuid::new_v4().to_string();
    let token = platform::issue_jwt(owner, &secret).unwrap();
    let wrong_token = platform::issue_jwt(Uuid::new_v4(), &secret).unwrap();
    let app = api::router_with(api::AppState {
        jwt_secret: secret,
        bootstrap_key: String::new(),
    });
    let base = format!("/api/v2/submission-workspaces/{workspace}/docx/versions");
    let path = format!("{base}/{version}/composition-report");
    let metadata = bidding::docx_composition::postgres::get_manifest_identity(
        &runtime, workspace, version, actor,
    )
    .await
    .unwrap()
    .unwrap();
    let sha = metadata["sha256"].as_str().unwrap();
    let object_path = platform::blob_path(sha).unwrap();
    assert!(object_path.starts_with(&object_dir));
    let original = std::fs::read(&object_path).unwrap();
    assert_eq!(platform::sha256_hex(&original), sha);
    let response = app
        .clone()
        .oneshot(request(&path, Some(&token)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "application/json");
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    assert_eq!(response.headers()["etag"], format!("\"{sha}\""));
    assert_eq!(
        response.headers()["content-disposition"],
        format!("attachment; filename=\"composition-report-{version}.json\"")
    );
    assert_eq!(
        to_bytes(response.into_body(), usize::MAX).await.unwrap(),
        original
    );

    // Corrupt only this dedicated local fixture. Authorization and exact-version
    // checks must still fail before the unreadable report object is inspected.
    let restore = RestoreObject {
        path: object_path.clone(),
        bytes: original.clone(),
    };
    let mut corrupt = original.clone();
    corrupt[0] ^= 1;
    std::fs::write(&object_path, &corrupt).unwrap();
    assert_eq!(
        error(&app, request(&path, None), StatusCode::UNAUTHORIZED).await,
        "UNAUTHORIZED"
    );
    assert_eq!(
        error(
            &app,
            request(&path, Some(&wrong_token)),
            StatusCode::FORBIDDEN
        )
        .await,
        "FORBIDDEN"
    );
    error(
        &app,
        request(
            &format!("{base}/{}/composition-report", Uuid::new_v4()),
            Some(&token),
        ),
        StatusCode::NOT_FOUND,
    )
    .await;
    error(
        &app,
        request(
            &format!(
                "/api/v2/submission-workspaces/{}/docx/versions/{version}/composition-report",
                Uuid::new_v4()
            ),
            Some(&token),
        ),
        StatusCode::NOT_FOUND,
    )
    .await;
    assert_eq!(
        error(
            &app,
            request(&path, Some(&token)),
            StatusCode::UNPROCESSABLE_ENTITY
        )
        .await,
        "DOCX_COMPOSITION_REPORT_INTEGRITY_FAILED"
    );
    std::fs::remove_file(&object_path).unwrap();
    assert_eq!(
        error(
            &app,
            request(&path, Some(&token)),
            StatusCode::SERVICE_UNAVAILABLE
        )
        .await,
        "DOCX_COMPOSITION_REPORT_UNAVAILABLE"
    );
    drop(restore);

    // A real upload round has its own version but no composition manifest.
    // The historical generated version must keep its own report afterwards.
    let current = docx_round::get_current_docx(&runtime, workspace, actor)
        .await
        .unwrap()
        .unwrap();
    let basis = docx_round::get_docx_round_basis(&runtime, workspace, actor)
        .await
        .unwrap()
        .unwrap();
    let expected = DocxVersionIdentity {
        version_id: Uuid::parse_str(current["version_id"].as_str().unwrap()).unwrap(),
        docx_sha256: current["docx_sha256"].as_str().unwrap().into(),
    };
    let document = InitialDocx::new(platform::read_blob(&expected.docx_sha256).unwrap()).unwrap();
    let staging = Uuid::new_v4();
    platform::stage_object_upload(
        &runtime,
        staging,
        &document.object_ref(),
        document.sha256(),
        bidding::tender_upload::DOCX_MEDIA_TYPE,
        document.bytes().len() as i64,
        actor,
    )
    .await
    .unwrap();
    let uploaded = docx_round::create_docx_round(
        &runtime,
        CreateDocxRound {
            workspace_id: workspace,
            staging_id: staging,
            basis: &basis,
            expected: Some(&expected),
            initial_docx: &document,
            actor,
            idempotency_key: &Uuid::new_v4().to_string(),
        },
    )
    .await
    .unwrap();
    let uploaded_version = uploaded["version_id"].as_str().unwrap();
    assert_ne!(uploaded_version, version.to_string());
    assert_eq!(
        error(
            &app,
            request(
                &format!("{base}/{uploaded_version}/composition-report"),
                Some(&token)
            ),
            StatusCode::NOT_FOUND
        )
        .await,
        "DOCX_COMPOSITION_REPORT_NOT_FOUND"
    );
    let historical = app.oneshot(request(&path, Some(&token))).await.unwrap();
    assert_eq!(historical.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(historical.into_body(), usize::MAX).await.unwrap(),
        original
    );
    runtime.close().await;
}
