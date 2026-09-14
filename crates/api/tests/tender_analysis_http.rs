#![cfg(feature = "docx-http-contract-tests")]
//! Uses the existing isolated publication fixture; never a live analysis run.
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

async fn get(app: &axum::Router, path: &str, token: &str, expected: StatusCode) -> Value {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(status, expected, "{}", String::from_utf8_lossy(&bytes));
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn frozen_analysis_http_preserves_complete_fields_and_project_ownership() {
    let path = std::path::PathBuf::from(
        std::env::var("KB_COMPOSITION_HTTP_FIXTURE")
            .expect("isolated publication fixture required"),
    );
    assert!(path.is_absolute() && path.starts_with(std::env::temp_dir()));
    let fixture: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let admin_url = std::env::var("KB_TENDER_AGENT_TEST_DATABASE_URL").unwrap();
    for url in [&admin_url, &std::env::var("DATABASE_URL").unwrap()] {
        let options: sqlx::postgres::PgConnectOptions = url.parse().unwrap();
        assert_eq!(options.get_host(), "127.0.0.1");
        assert!(
            options
                .get_database()
                .unwrap()
                .starts_with("knowledgebrain_test_")
        );
    }
    assert_eq!(
        std::env::var("DATABASE_URL")
            .unwrap()
            .parse::<sqlx::postgres::PgConnectOptions>()
            .unwrap()
            .get_username(),
        "kb_runtime_api"
    );
    let admin = PgPool::connect(&admin_url).await.unwrap();
    let project = Uuid::parse_str(fixture["project"].as_str().unwrap()).unwrap();
    let set = Uuid::parse_str(fixture["basis"]["requirement_set_id"].as_str().unwrap()).unwrap();
    let owner = Uuid::parse_str(
        fixture["actor"]
            .as_str()
            .unwrap()
            .strip_prefix("user:")
            .unwrap(),
    )
    .unwrap();
    let secret = Uuid::new_v4().to_string();
    let token = platform::issue_jwt(owner, &secret).unwrap();
    let foreign = platform::issue_jwt(Uuid::new_v4(), &secret).unwrap();
    let app = api::router_with(api::AppState {
        jwt_secret: secret,
        bootstrap_key: String::new(),
    });
    let base = format!("/api/v2/bid-projects/{project}/requirement-sets/{set}/analysis");
    let original: Value = sqlx::query_scalar("SELECT convert_from(canonical_payload,'UTF8')::jsonb FROM bid_requirement_set_artifacts WHERE project_id=$1 AND id=$2")
        .bind(project).bind(set).fetch_one(&admin).await.unwrap();
    let analysis = &original["analysis_result"];
    for (kind, rows) in [
        ("all", &analysis["analysis"]["records"]),
        ("relation", &analysis["analysis"]["relations"]),
    ] {
        let page = get(
            &app,
            &format!("{base}?kind={kind}&offset=0&limit=100"),
            &token,
            StatusCode::OK,
        )
        .await;
        assert_eq!(page["requirement_set_id"], set.to_string());
        assert_eq!(page["quality"], analysis["quality"]);
        assert_eq!(page["input_sha256"], analysis["frozen_input_sha256"]);
        let expected: Vec<Value> = rows
            .as_object()
            .unwrap()
            .iter()
            .map(|(id, value)| json!({"id":id,"value":value}))
            .collect();
        assert_eq!(page["items"], json!(expected));
    }
    let requirement = analysis["analysis"]["records"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap();
    assert!(requirement["data"]["categories"].as_array().unwrap().len() > 1);
    assert!(requirement["data"]["compliance"].as_array().unwrap().len() > 1);
    get(
        &app,
        &format!("{base}?kind=all&offset=0&limit=10"),
        &foreign,
        StatusCode::FORBIDDEN,
    )
    .await;
    get(
        &app,
        &format!("{base}?kind=all&offset=0&limit=10"),
        "",
        StatusCode::UNAUTHORIZED,
    )
    .await;
    for query in [
        "kind=all&offset=-1&limit=10",
        "kind=all&offset=0&limit=101",
        "kind=all&offset=2147483647&limit=1",
        "kind=bad&offset=0&limit=10",
        "kind=all&limit=10",
    ] {
        get(
            &app,
            &format!("{base}?{query}"),
            &token,
            StatusCode::BAD_REQUEST,
        )
        .await;
    }
    let missing = format!(
        "/api/v2/bid-projects/{project}/requirement-sets/{}/analysis?kind=all&offset=0&limit=10",
        Uuid::new_v4()
    );
    get(&app, &missing, &token, StatusCode::NOT_FOUND).await;
    // A valid set owned by another project cannot be read through this project path.
    let other_project = Uuid::new_v4();
    bidding::bid_authoring_v2::create_project_v2(
        &admin,
        other_project,
        "analysis tenant check",
        owner,
        &bidding::MutationContext::new(
            format!("user:{owner}"),
            Uuid::new_v4().to_string(),
            &json!({"title":"analysis tenant check"}),
        )
        .unwrap(),
    )
    .await
    .unwrap();
    let other_path = format!(
        "/api/v2/bid-projects/{other_project}/requirement-sets/{set}/analysis?kind=all&offset=0&limit=10"
    );
    get(&app, &other_path, &token, StatusCode::NOT_FOUND).await;

    // An append-only transport fixture exercises grid/visual spans and retained
    // findings. It is not an accepted analysis or a composition basis, and never
    // advances the project's current requirement pointer.
    let mut archived = original.clone();
    let records = archived["analysis_result"]["analysis"]["records"]
        .as_object_mut()
        .unwrap();
    let mut record = records.values().next().unwrap().clone();
    let source = record["sources"][0]["source_id"].clone();
    let spans = json!([
        {"source_id":source,"start":0,"end":0,"view_id":null,"grid_cell":{"form_id":Uuid::new_v4(),"row":1,"column":2}},
        {"source_id":source,"start":0,"end":0,"view_id":"frozen-view","grid_cell":null}
    ]);
    let id = Uuid::new_v4().to_string();
    record["id"] = json!(id);
    record["sources"] = spans.clone();
    records.insert(id.clone(), record.clone());
    let finding = json!({"code":"SOURCE_UNCERTAIN","message":"Retained review issue","correction":"Check the frozen original","affected":[{"id":id,"path":"/sources"}],"sources":spans});
    archived["analysis_result"]["review"]["findings"] = json!([finding.clone()]);
    archived["analysis_result"]["quality"] = json!("needs_review");
    let archive_id = insert_archive(&admin, project, set, &archived).await;
    let archive_path =
        format!("/api/v2/bid-projects/{project}/requirement-sets/{archive_id}/analysis");
    let mut observed = vec![];
    for offset in 0..2 {
        let page = get(
            &app,
            &format!("{archive_path}?kind=all&offset={offset}&limit=1"),
            &token,
            StatusCode::OK,
        )
        .await;
        assert_eq!(page["total"], 2);
        assert_eq!(page["quality"], "needs_review");
        observed.extend(page["items"].as_array().unwrap().clone());
    }
    let expected: Vec<Value> = archived["analysis_result"]["analysis"]["records"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(id, value)| json!({"id":id,"value":value}))
        .collect();
    assert_eq!(observed, expected);
    assert_eq!(
        observed.iter().find(|entry| entry["id"] == id).unwrap()["value"],
        record
    );
    let page = get(
        &app,
        &format!("{archive_path}?kind=finding&offset=0&limit=1"),
        &token,
        StatusCode::OK,
    )
    .await;
    assert_eq!(page["items"][0]["value"], finding);
    let empty = get(
        &app,
        &format!("{archive_path}?kind=all&offset=2&limit=1"),
        &token,
        StatusCode::OK,
    )
    .await;
    assert_eq!(empty["total"], 2);
    assert_eq!(empty["items"], json!([]));
    archived.as_object_mut().unwrap().remove("analysis_result");
    let legacy = insert_archive(&admin, project, set, &archived).await;
    let unavailable = get(&app, &format!("/api/v2/bid-projects/{project}/requirement-sets/{legacy}/analysis?kind=all&offset=0&limit=1"), &token, StatusCode::OK).await;
    assert_eq!(
        unavailable,
        json!({"available":false,"requirement_set_id":legacy})
    );
    let current: Uuid =
        sqlx::query_scalar("SELECT artifact_id FROM bid_requirement_set_current WHERE scope_id=$1")
            .bind(project)
            .fetch_one(&admin)
            .await
            .unwrap();
    assert_eq!(
        current, set,
        "HTTP queries and archive fixtures do not change the active analysis"
    );
}

async fn insert_archive(admin: &PgPool, project: Uuid, original: Uuid, payload: &Value) -> Uuid {
    let id = Uuid::new_v4();
    let bytes = serde_json::to_vec(payload).unwrap();
    sqlx::query("INSERT INTO bid_requirement_set_artifacts(id,project_id,document_set_id,document_set_sequence,disposition_set_id,disposition_set_sequence,revision,canonical_payload,content_sha256) SELECT $1,project_id,document_set_id,document_set_sequence,disposition_set_id,disposition_set_sequence,(SELECT max(revision)+1 FROM bid_requirement_set_artifacts WHERE project_id=$2),$4,kb_bid_v2_sha256_bytes($4) FROM bid_requirement_set_artifacts WHERE project_id=$2 AND id=$3")
        .bind(id).bind(project).bind(original).bind(bytes).execute(admin).await.unwrap();
    id
}
