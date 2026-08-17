//! Catalog, thin ingest, search, matching, answer, lifecycle.

use api::{AppState, router_with};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use domain::Store;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

fn app() -> (axum::Router, Arc<Mutex<Store>>) {
    let store = Arc::new(Mutex::new(Store::default()));
    let router = router_with(AppState {
        store: store.clone(),
        jwt_secret: "secret".into(),
        bootstrap_key: String::new(),
    });
    (router, store)
}

async fn call(app: &axum::Router, req: Request<Body>) -> (StatusCode, Value) {
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let v = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or(Value::String(String::from_utf8_lossy(&bytes).into()))
    };
    (status, v)
}

fn auth_json(token: &str, method: &str, uri: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

#[tokio::test]
async fn catalog_ingest_search_match_answer_lifecycle() {
    let (app, store) = app();

    let (st, v) = call(
        &app,
        Request::builder()
            .method("POST")
            .uri("/api/v1/auth/register")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"email":"a@b.c","password":"pw"}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let token = v["token"].as_str().unwrap().to_string();
    let owner = v["user_id"].as_str().unwrap().to_string();

    let (st, v) = call(
        &app,
        auth_json(
            &token,
            "POST",
            "/api/v1/workspaces",
            json!({"name":"Acme","slug":"acme"}),
        ),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    let ws = v["id"].as_str().unwrap().to_string();

    let (st, v) = call(
        &app,
        auth_json(
            &token,
            "GET",
            &format!("/api/v1/workspaces/{ws}/products?kind=library"),
            json!({}),
        ),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v[0]["slug"], "library");
    assert_eq!(v[0]["name"], "公司资料");
    let lib_id = v[0]["id"].as_str().unwrap().to_string();

    let (_reg_st, v) = call(
        &app,
        Request::builder()
            .method("POST")
            .uri("/api/v1/auth/register")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"email":"v@b.c","password":"pw"}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    let viewer_token = v["token"].as_str().unwrap().to_string();
    let viewer_id = v["user_id"].as_str().unwrap().to_string();
    let (st, _) = call(
        &app,
        auth_json(
            &token,
            "POST",
            &format!("/api/v1/workspaces/{ws}/members"),
            json!({"user_id": viewer_id, "role": "viewer"}),
        ),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED);
    let (st, v) = call(
        &app,
        auth_json(
            &viewer_token,
            "POST",
            &format!("/api/v1/workspaces/{ws}/products"),
            json!({"name":"nope","slug":"nope"}),
        ),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{v}");

    let (st, v) = call(
        &app,
        auth_json(
            &token,
            "POST",
            &format!("/api/v1/workspaces/{ws}/products"),
            json!({"name":"Alpha Switch","slug":"alpha"}),
        ),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    let pid = v["id"].as_str().unwrap().to_string();

    let (st, v) = call(
        &app,
        auth_json(
            &token,
            "POST",
            &format!("/api/v1/products/{pid}/versions"),
            json!({"label":"v1"}),
        ),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    let vid = v["id"].as_str().unwrap().to_string();

    let (st, v) = call(
        &app,
        auth_json(
            &token,
            "POST",
            &format!("/api/v1/workspaces/{ws}/tags"),
            json!({"name":"手册","slug":"manual"}),
        ),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED);
    let tag_id = v["id"].as_str().unwrap().to_string();

    let boundary = "----kb";
    let file_body = "The Alpha switch provides 40Gbps throughput for campus core.";
    let mp = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"spec.txt\"\r\nContent-Type: text/plain\r\n\r\n{file_body}\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"tag_ids\"\r\n\r\n[\"{tag_id}\"]\r\n--{boundary}--\r\n"
    );
    let (st, v) = call(
        &app,
        Request::builder()
            .method("POST")
            .uri(format!(
                "/api/v1/products/{pid}/versions/{vid}/documents/file"
            ))
            .header("authorization", format!("Bearer {token}"))
            .header(
                "content-type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from(mp))
            .unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    assert_eq!(v["parse_status"], "pending");
    assert_eq!(v["enable_status"], "disabled");
    let did = v["id"].as_str().unwrap().to_string();

    {
        let s = store.lock().unwrap();
        assert!(s.queue.iter().any(|j| j.task_type == "document:process"));
        assert_eq!(
            s.queue
                .iter()
                .find(|j| j.task_type == "document:process")
                .unwrap()
                .max_retry,
            3
        );
    }

    let (st, v) = call(
        &app,
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/products/{pid}/versions/{vid}/documents/file"))
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", format!("multipart/form-data; boundary={boundary}"))
            .body(Body::from(format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"spec.txt\"\r\nContent-Type: text/plain\r\n\r\n{file_body}\r\n--{boundary}--\r\n"
            )))
            .unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "{v}");
    assert!(v["error"]["message"].as_str().unwrap().contains(&did));

    worker::drain(&mut store.lock().unwrap());
    let (st, v) = call(
        &app,
        auth_json(
            &token,
            "GET",
            &format!("/api/v1/documents/{did}"),
            json!({}),
        ),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["parse_status"], "completed", "{v}");
    assert_eq!(v["enable_status"], "enabled");

    let (st, v) = call(
        &app,
        auth_json(
            &token,
            "POST",
            "/api/v1/search",
            json!({
                "mode": "assembly",
                "query": "40Gbps throughput",
                "product_id": pid,
                "version_id": vid
            }),
        ),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(!v["hits"].as_array().unwrap().is_empty(), "{v}");
    assert_eq!(v["hits"][0]["product_id"], pid);

    let lib_body = "Company holds ISO9001 quality certification.";
    let (st, v) = call(
        &app,
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/products/{lib_id}/versions/current/documents/file"))
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", format!("multipart/form-data; boundary={boundary}"))
            .body(Body::from(format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"iso.txt\"\r\nContent-Type: text/plain\r\n\r\n{lib_body}\r\n--{boundary}--\r\n"
            )))
            .unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    worker::drain(&mut store.lock().unwrap());

    let (st, v) = call(
        &app,
        auth_json(
            &token,
            "POST",
            "/api/v1/search",
            json!({
                "mode": "assembly",
                "query": "ISO9001",
                "product_id": pid,
                "include_library": true
            }),
        ),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(
        v["hits"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["product_kind"] == "library"),
        "{v}"
    );

    let (st, v) = call(
        &app,
        auth_json(
            &token,
            "POST",
            "/api/v1/match",
            json!({
                "requirements": [
                    {"id":"r1","text":"40Gbps throughput","weight":1.0,"must":true},
                    {"id":"r2","text":"ISO9001","weight":1.0,"must":false,"use_library":true}
                ],
                "version_scope": "current",
                "include_library": true,
                "workspace_id": ws
            }),
        ),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(v.get("best_product_id").is_none());
    assert_eq!(v["candidates"][0]["product_id"], pid);
    assert!(v["candidates"][0]["coverage"].as_f64().unwrap() > 0.0);

    let (st, v) = call(
        &app,
        auth_json(
            &token,
            "POST",
            "/api/v1/answer",
            json!({"query":"What throughput?","product_id": pid, "version_id": "current"}),
        ),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(!v["answer"].as_str().unwrap().is_empty());
    assert!(!v["citations"].as_array().unwrap().is_empty());

    let (st, v) = call(
        &app,
        auth_json(
            &token,
            "POST",
            &format!("/api/v1/documents/{did}/cancel"),
            json!({}),
        ),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["parse_status"], "cancelled");

    let (st, v) = call(
        &app,
        auth_json(
            &token,
            "POST",
            &format!("/api/v1/documents/{did}/reparse"),
            json!({}),
        ),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["parse_status"], "pending");
    assert_eq!(v["enable_status"], "disabled");
    worker::drain(&mut store.lock().unwrap());

    let (st, _) = call(
        &app,
        auth_json(
            &token,
            "DELETE",
            &format!("/api/v1/documents/{did}"),
            json!({}),
        ),
    )
    .await;
    assert_eq!(st, StatusCode::ACCEPTED);
    worker::drain(&mut store.lock().unwrap());
    let (st, v) = call(
        &app,
        auth_json(
            &token,
            "GET",
            &format!("/api/v1/documents/{did}"),
            json!({}),
        ),
    )
    .await;
    assert!(
        st == StatusCode::NOT_FOUND || (st == StatusCode::OK && v["parse_status"] == "deleting"),
        "{st} {v}"
    );

    let _ = owner;
}

#[tokio::test]
async fn ingest_enqueue_fail_returns_200_and_failed_row() {
    let store = Arc::new(Mutex::new(Store::default()));
    store.lock().unwrap().enqueue_fail = true;
    let app = router_with(AppState {
        store: store.clone(),
        jwt_secret: "secret".into(),
        bootstrap_key: String::new(),
    });
    let (_st, v) = call(
        &app,
        Request::builder()
            .method("POST")
            .uri("/api/v1/auth/register")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"email":"e@b.c","password":"pw"}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    let token = v["token"].as_str().unwrap().to_string();
    let (_st, v) = call(
        &app,
        auth_json(
            &token,
            "POST",
            "/api/v1/workspaces",
            json!({"name":"W","slug":"w2"}),
        ),
    )
    .await;
    let ws = v["id"].as_str().unwrap().to_string();
    let (_st, v) = call(
        &app,
        auth_json(
            &token,
            "POST",
            &format!("/api/v1/workspaces/{ws}/products"),
            json!({"name":"P","slug":"p"}),
        ),
    )
    .await;
    let pid = v["id"].as_str().unwrap().to_string();
    let (_st, v) = call(
        &app,
        auth_json(
            &token,
            "POST",
            &format!("/api/v1/products/{pid}/versions"),
            json!({"label":"v1"}),
        ),
    )
    .await;
    let vid = v["id"].as_str().unwrap().to_string();
    let boundary = "----kb";
    let (st, v) = call(
        &app,
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/products/{pid}/versions/{vid}/documents/file"))
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", format!("multipart/form-data; boundary={boundary}"))
            .body(Body::from(format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.txt\"\r\nContent-Type: text/plain\r\n\r\nhi\r\n--{boundary}--\r\n"
            )))
            .unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["parse_status"], "failed", "{v}");
    assert!(store.lock().unwrap().queue.is_empty());
}

#[tokio::test]
async fn health_still_works() {
    let (app, _) = app();
    let (st, v) = call(
        &app,
        Request::builder()
            .uri("/health")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["status"], "ok");
}

#[tokio::test]
async fn version_files_require_reference_and_passages_enqueue() {
    let (app, store) = app();
    let (st, v) = call(
        &app,
        Request::builder()
            .method("POST")
            .uri("/api/v1/auth/register")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"email":"f@b.c","password":"pw"}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    let token = v["token"].as_str().unwrap().to_string();
    let (_st, v) = call(
        &app,
        auth_json(
            &token,
            "POST",
            "/api/v1/workspaces",
            json!({"name":"Files","slug":"files"}),
        ),
    )
    .await;
    let ws = v["id"].as_str().unwrap().to_string();
    let (_st, v) = call(
        &app,
        auth_json(
            &token,
            "POST",
            &format!("/api/v1/workspaces/{ws}/products"),
            json!({"name":"P","slug":"pf"}),
        ),
    )
    .await;
    let pid = v["id"].as_str().unwrap().to_string();
    let (_st, v) = call(
        &app,
        auth_json(
            &token,
            "POST",
            &format!("/api/v1/products/{pid}/versions"),
            json!({"label":"v1"}),
        ),
    )
    .await;
    let vid = v["id"].as_str().unwrap().to_string();
    let boundary = "----kb";
    let body = "file for scope";
    let (st, v) = call(
        &app,
        Request::builder()
            .method("POST")
            .uri(format!(
                "/api/v1/products/{pid}/versions/{vid}/documents/file"
            ))
            .header("authorization", format!("Bearer {token}"))
            .header(
                "content-type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from(format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"s.txt\"\r\nContent-Type: text/plain\r\n\r\n{body}\r\n--{boundary}--\r\n"
            )))
            .unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    let hash = store
        .lock()
        .unwrap()
        .documents
        .values()
        .next()
        .unwrap()
        .file_hash
        .clone();
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/products/{pid}/versions/{vid}/files?key=objects/{hash}"
                ))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/products/{pid}/versions/{vid}/files?key=objects/deadbeef"
                ))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    let (st, v) = call(
        &app,
        auth_json(
            &token,
            "POST",
            &format!("/api/v1/products/{pid}/versions/{vid}/documents/passage"),
            json!({"title":"notes","passages":["alpha line","beta line"]}),
        ),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    {
        let queued = store.lock().unwrap();
        let job = queued
            .queue
            .iter()
            .rev()
            .find(|j| j.task_type == "document:process")
            .expect("passage job");
        assert_eq!(job.payload["passages"], json!(["alpha line", "beta line"]));
    }

    let (st, v) = call(
        &app,
        auth_json(
            &token,
            "POST",
            &format!("/api/v1/products/{pid}/versions/{vid}/documents/manual"),
            json!({"title":"guide","content":"# heading\n\nmanual body"}),
        ),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    {
        let queued = store.lock().unwrap();
        let job = queued
            .queue
            .iter()
            .rev()
            .find(|j| j.task_type == "manual:process")
            .expect("manual job");
        assert_eq!(job.payload["manual"], json!(true));
        assert!(job.payload.get("passages").is_none());
    }
}

#[tokio::test]
async fn current_version_empty_is_validation() {
    let (app, _) = app();
    let (_st, v) = call(
        &app,
        Request::builder()
            .method("POST")
            .uri("/api/v1/auth/register")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"email":"c@b.c","password":"pw"}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    let token = v["token"].as_str().unwrap().to_string();
    let (_st, v) = call(
        &app,
        auth_json(
            &token,
            "POST",
            "/api/v1/workspaces",
            json!({"name":"Empty","slug":"empty"}),
        ),
    )
    .await;
    let ws = v["id"].as_str().unwrap().to_string();
    let (_st, v) = call(
        &app,
        auth_json(
            &token,
            "POST",
            &format!("/api/v1/workspaces/{ws}/products"),
            json!({"name":"P","slug":"p"}),
        ),
    )
    .await;
    let pid = v["id"].as_str().unwrap().to_string();
    let (st, v) = call(
        &app,
        auth_json(
            &token,
            "POST",
            &format!("/api/v1/products/{pid}/versions/current/documents/url"),
            json!({"url":"https://example.com/a.md"}),
        ),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(v["error"]["code"], "VALIDATION");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("no current version"),
        "{v}"
    );
}

#[tokio::test]
async fn api_key_workspace_and_product_scope() {
    let (app, _) = app();
    let (_st, v) = call(
        &app,
        Request::builder()
            .method("POST")
            .uri("/api/v1/auth/register")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"email":"k@b.c","password":"pw"}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    let token = v["token"].as_str().unwrap().to_string();
    let (_st, v) = call(
        &app,
        auth_json(
            &token,
            "POST",
            "/api/v1/workspaces",
            json!({"name":"Keys","slug":"keys"}),
        ),
    )
    .await;
    let ws = v["id"].as_str().unwrap().to_string();
    let (_st, v) = call(
        &app,
        auth_json(
            &token,
            "POST",
            &format!("/api/v1/workspaces/{ws}/products"),
            json!({"name":"P","slug":"p"}),
        ),
    )
    .await;
    let pid = v["id"].as_str().unwrap().to_string();

    let (st, v) = call(
        &app,
        auth_json(
            &token,
            "POST",
            &format!("/api/v1/workspaces/{ws}/api-keys"),
            json!({"name":"ingest","scope_type":"workspace","scopes":["ingest","search"]}),
        ),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    let key = v["token"].as_str().unwrap().to_string();
    assert!(key.starts_with("kb_"));

    let (st, v) = call(
        &app,
        Request::builder()
            .method("GET")
            .uri(format!("/api/v1/workspaces/{ws}/products"))
            .header("x-api-key", &key)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert!(v.as_array().unwrap().iter().any(|p| p["id"] == pid));

    let (st, v) = call(
        &app,
        auth_json(
            &token,
            "POST",
            &format!("/api/v1/workspaces/{ws}/api-keys"),
            json!({"name":"prod","scope_type":"product","scope_id":pid,"scopes":["search"]}),
        ),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    let search_key = v["token"].as_str().unwrap().to_string();

    let (st, _) = call(
        &app,
        Request::builder()
            .method("POST")
            .uri(format!("/api/v1/workspaces/{ws}/products"))
            .header("x-api-key", &search_key)
            .header("content-type", "application/json")
            .body(Body::from(json!({"name":"X","slug":"x"}).to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    let (_st, v) = call(
        &app,
        auth_json(
            &token,
            "POST",
            "/api/v1/workspaces",
            json!({"name":"Other","slug":"other"}),
        ),
    )
    .await;
    let other = v["id"].as_str().unwrap().to_string();
    let (st, _) = call(
        &app,
        Request::builder()
            .method("GET")
            .uri(format!("/api/v1/workspaces/{other}"))
            .header("x-api-key", &key)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn document_filters_tag_delete_and_process_config() {
    let (app, store) = app();
    let (_st, v) = call(
        &app,
        Request::builder()
            .method("POST")
            .uri("/api/v1/auth/register")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"email":"g@b.c","password":"pw"}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    let token = v["token"].as_str().unwrap().to_string();
    let (_st, v) = call(
        &app,
        auth_json(
            &token,
            "POST",
            "/api/v1/workspaces",
            json!({"name":"Filt","slug":"filt"}),
        ),
    )
    .await;
    let ws = v["id"].as_str().unwrap().to_string();
    let (_st, v) = call(
        &app,
        auth_json(
            &token,
            "POST",
            &format!("/api/v1/workspaces/{ws}/products"),
            json!({"name":"Sw","slug":"sw"}),
        ),
    )
    .await;
    let pid = v["id"].as_str().unwrap().to_string();
    let (_st, v) = call(
        &app,
        auth_json(
            &token,
            "POST",
            &format!("/api/v1/products/{pid}/versions"),
            json!({"label":"v1"}),
        ),
    )
    .await;
    let vid = v["id"].as_str().unwrap().to_string();
    let (_st, v) = call(
        &app,
        auth_json(
            &token,
            "POST",
            &format!("/api/v1/workspaces/{ws}/tags"),
            json!({"name":"手册","slug":"manual"}),
        ),
    )
    .await;
    let tag_id = v["id"].as_str().unwrap().to_string();
    let boundary = "----kb";
    let cfg = r#"{"graph_enabled":false,"chunking_config":{"chunk_size":256,"enable_parent_child":false}}"#;
    let mp = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"alpha.txt\"\r\nContent-Type: text/plain\r\n\r\nAlpha 40Gbps\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"tag_ids\"\r\n\r\n[\"{tag_id}\"]\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"process_config\"\r\n\r\n{cfg}\r\n--{boundary}--\r\n"
    );
    let (st, v) = call(
        &app,
        Request::builder()
            .method("POST")
            .uri(format!(
                "/api/v1/products/{pid}/versions/{vid}/documents/file"
            ))
            .header("authorization", format!("Bearer {token}"))
            .header(
                "content-type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(Body::from(mp))
            .unwrap(),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{v}");
    let did = v["id"].as_str().unwrap().to_string();
    {
        let s = store.lock().unwrap();
        let doc = s
            .documents
            .values()
            .find(|d| d.id.to_string() == did)
            .unwrap();
        assert!(doc.process_overrides.is_some());
        assert_eq!(
            doc.process_overrides.as_ref().unwrap().graph_enabled,
            Some(false)
        );
    }
    let (st, v) = call(
        &app,
        auth_json(
            &token,
            "GET",
            &format!("/api/v1/products/{pid}/versions/{vid}/documents?parse_status=pending&tag={tag_id}&keyword=alpha"),
            json!({}),
        ),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v.as_array().unwrap().len(), 1);
    let (st, v) = call(
        &app,
        auth_json(
            &token,
            "GET",
            &format!("/api/v1/products/{pid}/versions/{vid}/documents?keyword=nope"),
            json!({}),
        ),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert!(v.as_array().unwrap().is_empty());
    let (st, _) = call(
        &app,
        auth_json(
            &token,
            "DELETE",
            &format!("/api/v1/workspaces/{ws}/tags/{tag_id}"),
            json!({}),
        ),
    )
    .await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    let (st, v) = call(
        &app,
        auth_json(
            &token,
            "GET",
            &format!("/api/v1/workspaces/{ws}/tags"),
            json!({}),
        ),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert!(v.as_array().unwrap().is_empty());
    {
        let s = store.lock().unwrap();
        assert!(
            s.documents
                .contains_key(&uuid::Uuid::parse_str(&did).unwrap())
        );
    }
    let (st, _) = call(
        &app,
        auth_json(
            &token,
            "DELETE",
            &format!("/api/v1/products/{pid}"),
            json!({}),
        ),
    )
    .await;
    assert_eq!(st, StatusCode::ACCEPTED);
    {
        let s = store.lock().unwrap();
        assert!(s.queue.iter().any(|j| j.task_type == "kb:delete"));
    }
}

#[tokio::test]
async fn patch_version_config_and_me_and_workspace_delete() {
    let (app, store) = app();
    let (_st, v) = call(
        &app,
        Request::builder()
            .method("POST")
            .uri("/api/v1/auth/register")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"email":"h@b.c","password":"pw"}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    let token = v["token"].as_str().unwrap().to_string();
    let (st, v) = call(
        &app,
        auth_json(
            &token,
            "PATCH",
            "/api/v1/me",
            json!({"email":"renamed@b.c"}),
        ),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["email"], "renamed@b.c");
    let (_st, v) = call(
        &app,
        auth_json(
            &token,
            "POST",
            "/api/v1/workspaces",
            json!({"name":"Cfg","slug":"cfg"}),
        ),
    )
    .await;
    let ws = v["id"].as_str().unwrap().to_string();
    let (_st, v) = call(
        &app,
        auth_json(
            &token,
            "POST",
            &format!("/api/v1/workspaces/{ws}/products"),
            json!({"name":"P","slug":"p"}),
        ),
    )
    .await;
    let pid = v["id"].as_str().unwrap().to_string();
    let (_st, v) = call(
        &app,
        auth_json(
            &token,
            "POST",
            &format!("/api/v1/products/{pid}/versions"),
            json!({"label":"v1"}),
        ),
    )
    .await;
    let vid = v["id"].as_str().unwrap().to_string();
    let (st, v) = call(
        &app,
        auth_json(
            &token,
            "PATCH",
            &format!("/api/v1/products/{pid}/versions/{vid}"),
            json!({
                "chunk_size": 256,
                "enable_parent_child": true,
                "parent_chunk_size": 2000,
                "child_chunk_size": 200,
                "graph_enabled": false,
                "wiki_enabled": false,
                "summary_model_id": "stub-chat",
                "question_count": 7,
                "question_custom_instructions": "for auditors",
                "table_metadata_instructions": "units in Mbps"
            }),
        ),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{v}");
    assert_eq!(v["chunk_size"], 256);
    assert_eq!(v["enable_parent_child"], true);
    assert_eq!(v["parent_chunk_size"], 2000);
    assert_eq!(v["child_chunk_size"], 200);
    assert_eq!(v["graph_enabled"], false);
    assert_eq!(v["wiki_enabled"], false);
    assert_eq!(v["question_count"], 7);
    assert_eq!(v["question_custom_instructions"], "for auditors");
    assert_eq!(v["table_metadata_instructions"], "units in Mbps");
    let (st, _) = call(
        &app,
        auth_json(
            &token,
            "DELETE",
            &format!("/api/v1/workspaces/{ws}"),
            json!({}),
        ),
    )
    .await;
    assert_eq!(st, StatusCode::ACCEPTED);
    {
        let s = store.lock().unwrap();
        assert!(
            !s.workspaces
                .contains_key(&uuid::Uuid::parse_str(&ws).unwrap())
        );
        assert!(s.queue.iter().any(|j| j.task_type == "kb:delete"));
    }
}
