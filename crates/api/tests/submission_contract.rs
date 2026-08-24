//! Submission HTTP contract: server-derived dependencies, project-scoped IDs, ShotSet CAS.

use api::{AppState, router_with};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use domain::Store;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;
use uuid::Uuid;

const ONE_PIXEL_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x04, 0x00, 0x00, 0x00, 0xb5, 0x1c, 0x0c,
    0x02, 0x00, 0x00, 0x00, 0x0b, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x64, 0xf8, 0x0f, 0x00,
    0x01, 0x05, 0x01, 0x01, 0x27, 0x18, 0xe3, 0x66, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44,
    0xae, 0x42, 0x60, 0x82,
];

fn app() -> axum::Router {
    router_with(AppState {
        store: Arc::new(Mutex::new(Store::default())),
        jwt_secret: "submission-contract-secret".into(),
        bootstrap_key: String::new(),
    })
}

async fn call(app: &axum::Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into()))
    };
    (status, body)
}

fn json_request(token: &str, method: &str, uri: &str, body: Value) -> Request<Body> {
    json_request_with_key(token, method, uri, body, &Uuid::new_v4().to_string())
}

fn json_request_with_key(
    token: &str,
    method: &str,
    uri: &str,
    body: Value,
    idempotency_key: &str,
) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .header("idempotency-key", idempotency_key)
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn shot_upload_request(token: &str, project_id: &str) -> Request<Body> {
    shot_upload_request_with_key(token, project_id, &Uuid::new_v4().to_string())
}

fn shot_upload_request_with_key(
    token: &str,
    project_id: &str,
    idempotency_key: &str,
) -> Request<Body> {
    let boundary = format!("kb-shot-{}", Uuid::new_v4().simple());
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"shot.png\"\r\nContent-Type: image/png\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(ONE_PIXEL_PNG);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    Request::builder()
        .method("POST")
        .uri(format!("/api/v1/bids/{project_id}/shots/artifacts"))
        .header("authorization", format!("Bearer {token}"))
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .header("idempotency-key", idempotency_key)
        .body(Body::from(body))
        .unwrap()
}

fn document_upload_request_with_key(
    token: &str,
    project_id: &str,
    idempotency_key: &str,
    file_name: &str,
    media_type: &str,
    bytes: &[u8],
) -> Request<Body> {
    let boundary = format!("kb-document-{}", Uuid::new_v4().simple());
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\nContent-Type: {media_type}\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    Request::builder()
        .method("POST")
        .uri(format!("/api/v1/bids/{project_id}/documents"))
        .header("authorization", format!("Bearer {token}"))
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .header("idempotency-key", idempotency_key)
        .body(Body::from(body))
        .unwrap()
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}

fn minimal_docx() -> Vec<u8> {
    let files: [(&str, &[u8]); 2] = [
        (
            "[Content_Types].xml",
            br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#,
        ),
        (
            "word/document.xml",
            br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Tender</w:t></w:r></w:p></w:body></w:document>"#,
        ),
    ];
    let mut output = Vec::new();
    let mut central_entries = Vec::new();
    for (name, data) in files {
        let name = name.as_bytes();
        let checksum = crc32(data);
        let local_offset = output.len() as u32;
        output.extend_from_slice(b"PK\x03\x04");
        output.extend_from_slice(&20u16.to_le_bytes());
        output.extend_from_slice(&0u16.to_le_bytes());
        output.extend_from_slice(&0u16.to_le_bytes());
        output.extend_from_slice(&0u16.to_le_bytes());
        output.extend_from_slice(&0u16.to_le_bytes());
        output.extend_from_slice(&checksum.to_le_bytes());
        output.extend_from_slice(&(data.len() as u32).to_le_bytes());
        output.extend_from_slice(&(data.len() as u32).to_le_bytes());
        output.extend_from_slice(&(name.len() as u16).to_le_bytes());
        output.extend_from_slice(&0u16.to_le_bytes());
        output.extend_from_slice(name);
        output.extend_from_slice(data);

        let mut central = Vec::new();
        central.extend_from_slice(b"PK\x01\x02");
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&checksum.to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&local_offset.to_le_bytes());
        central.extend_from_slice(name);
        central_entries.push(central);
    }
    let central_offset = output.len() as u32;
    for entry in central_entries {
        output.extend_from_slice(&entry);
    }
    let central_size = output.len() as u32 - central_offset;
    output.extend_from_slice(b"PK\x05\x06");
    output.extend_from_slice(&0u16.to_le_bytes());
    output.extend_from_slice(&0u16.to_le_bytes());
    output.extend_from_slice(&(files.len() as u16).to_le_bytes());
    output.extend_from_slice(&(files.len() as u16).to_le_bytes());
    output.extend_from_slice(&central_size.to_le_bytes());
    output.extend_from_slice(&central_offset.to_le_bytes());
    output.extend_from_slice(&0u16.to_le_bytes());
    output
}

async fn live_submission_pool() -> Option<PgPool> {
    let pool = match storage::connect().await {
        Ok(pool) => pool,
        Err(error) if std::env::var_os("DATABASE_URL").is_some() => {
            panic!("connect live Submission HTTP contract database: {error}")
        }
        Err(error) => {
            eprintln!("skipped live Submission HTTP contract: database unavailable: {error}");
            return None;
        }
    };
    let ready = sqlx::query_scalar::<_, bool>(
        "SELECT to_regprocedure(
          'kb_bid_replace_shot_set(uuid,bigint,uuid[],kb_actor_identity,text,bytea,kb_sha256)'
         ) IS NOT NULL
         AND to_regprocedure(
          'kb_bid_regenerate_part(uuid,text,bigint,kb_sha256,kb_actor_identity,text,bytea,kb_sha256)'
         ) IS NOT NULL
         AND to_regprocedure(
          'kb_bid_manifest_render_input(uuid,uuid)'
         ) IS NOT NULL
         AND to_regprocedure(
          'kb_bid_schedule_submission_render(uuid,uuid,uuid,kb_sha256,kb_actor_identity,text,bytea,kb_sha256)'
         ) IS NOT NULL
         AND to_regprocedure(
          'kb_bid_get_submission_render_job(uuid,uuid)'
         ) IS NOT NULL",
    )
    .fetch_one(&pool)
    .await
    .expect("probe final Submission HTTP schema");
    if !ready {
        if std::env::var_os("DATABASE_URL").is_some() {
            panic!("final Submission HTTP schema unavailable");
        }
        eprintln!("skipped live Submission HTTP contract: final Submission schema unavailable");
        return None;
    }
    Some(pool)
}

async fn live_actor(pool: &PgPool) -> (axum::Router, String) {
    let user_id = Uuid::new_v4();
    storage::insert_user(
        pool,
        user_id,
        &format!("submission-contract-{user_id}@invalid.test"),
        None,
    )
    .await
    .unwrap();
    let token = auth::issue_jwt(user_id, "submission-contract-secret").unwrap();
    (app(), token)
}

async fn create_project(app: &axum::Router, token: &str, title: &str) -> String {
    let (status, body) = call(
        app,
        json_request(
            token,
            "POST",
            "/api/v1/bids",
            json!({"title":title,"ends_at":"2099-01-01T00:00:00Z"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["id"].as_str().unwrap().to_string()
}

async fn regenerate_rejects_caller_defined_identities_at_json_boundary() {
    let Some(pool) = live_submission_pool().await else {
        return;
    };
    let (app, token) = live_actor(&pool).await;
    let project = create_project(&app, &token, "Caller identity rejection").await;
    let (status, body) = call(
        &app,
        json_request(
            &token,
            "POST",
            &format!("/api/v1/bids/{project}/parts/1/regenerate"),
            json!({
                "expected_content_revision":0,
                "typed_input_identities":[]
            }),
        ),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "caller identities must be rejected before database access: {body}"
    );
}

async fn regenerate_rejects_existing_dependency_cas_mismatches() {
    let Some(pool) = live_submission_pool().await else {
        return;
    };
    let (app, token) = live_actor(&pool).await;
    let project = create_project(&app, &token, "Submission dependency CAS").await;
    let uri = format!("/api/v1/bids/{project}/parts/1/regenerate");

    let (status, generated) = call(
        &app,
        json_request(&token, "POST", &uri, json!({"expected_content_revision":0})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{generated}");
    assert!(generated["dependency_sha256"].as_str().is_some());

    let (missing_cas_status, missing_cas_body) = call(
        &app,
        json_request(&token, "POST", &uri, json!({"expected_content_revision":1})),
    )
    .await;
    let (wrong_cas_status, wrong_cas_body) = call(
        &app,
        json_request(
            &token,
            "POST",
            &uri,
            json!({
                "expected_content_revision":1,
                "expected_dependency_sha256":"0000000000000000000000000000000000000000000000000000000000000000"
            }),
        ),
    )
    .await;

    assert_eq!(
        missing_cas_status,
        StatusCode::CONFLICT,
        "{missing_cas_body}"
    );
    assert_eq!(
        missing_cas_body["error"]["code"],
        "PART_DEPENDENCY_CAS_MISMATCH"
    );
    assert_eq!(wrong_cas_status, StatusCode::CONFLICT, "{wrong_cas_body}");
    assert_eq!(
        wrong_cas_body["error"]["code"],
        "PART_DEPENDENCY_CAS_MISMATCH"
    );
}

async fn bid_routes_and_list_are_isolated_by_project_owner() {
    let Some(pool) = live_submission_pool().await else {
        return;
    };
    let (app, owner_a_token) = live_actor(&pool).await;
    let owner_b = Uuid::new_v4();
    storage::insert_user(
        &pool,
        owner_b,
        &format!("submission-owner-{owner_b}@invalid.test"),
        None,
    )
    .await
    .unwrap();
    let owner_b_token = auth::issue_jwt(owner_b, "submission-contract-secret").unwrap();
    let project_a = create_project(&app, &owner_a_token, "Owner A project").await;
    let project_b = create_project(&app, &owner_b_token, "Owner B project").await;

    let (cross_owner_status, cross_owner_body) = call(
        &app,
        json_request(
            &owner_b_token,
            "GET",
            &format!("/api/v1/bids/{project_a}"),
            json!({}),
        ),
    )
    .await;
    assert_eq!(
        cross_owner_status,
        StatusCode::NOT_FOUND,
        "{cross_owner_body}"
    );

    let project_a_text = project_a.to_string();
    let encoded_project_a = format!(
        "%{:02X}{}",
        project_a_text.as_bytes()[0],
        &project_a_text[1..]
    );
    let (encoded_cross_owner_status, encoded_cross_owner_body) = call(
        &app,
        json_request(
            &owner_b_token,
            "GET",
            &format!("/api/v1/bids/{encoded_project_a}"),
            json!({}),
        ),
    )
    .await;
    assert_eq!(
        encoded_cross_owner_status,
        StatusCode::NOT_FOUND,
        "encoded project path bypassed owner isolation: {encoded_cross_owner_body}"
    );

    let (list_status, list_body) = call(
        &app,
        json_request(&owner_b_token, "GET", "/api/v1/bids", json!({})),
    )
    .await;
    assert_eq!(list_status, StatusCode::OK, "{list_body}");
    let ids = list_body
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|project| project["id"].as_str())
        .collect::<Vec<_>>();
    assert!(ids.contains(&project_b.as_str()));
    assert!(!ids.contains(&project_a.as_str()));
}

async fn render_hides_cross_project_manifest_ids() {
    let Some(pool) = live_submission_pool().await else {
        return;
    };
    let (app, token) = live_actor(&pool).await;
    let project_a = create_project(&app, &token, "Submission project A").await;
    let project_b = create_project(&app, &token, "Submission project B").await;

    let (manifest_status, manifest) = call(
        &app,
        json_request(
            &token,
            "POST",
            &format!("/api/v1/bids/{project_b}/submission/manifests"),
            json!({"format":"docx"}),
        ),
    )
    .await;
    assert_eq!(manifest_status, StatusCode::CREATED, "{manifest}");
    let manifest_id = manifest["manifest_id"].as_str().unwrap();
    let manifest_sha256 = manifest["content_sha256"].as_str().unwrap();

    let (cross_render_status, cross_render_body) = call(
        &app,
        json_request(
            &token,
            "POST",
            &format!("/api/v1/bids/{project_a}/submission/manifests/{manifest_id}/render"),
            json!({"expected_manifest_sha256":manifest_sha256}),
        ),
    )
    .await;
    assert_eq!(
        cross_render_status,
        StatusCode::NOT_FOUND,
        "{cross_render_body}"
    );
}

async fn shot_upload_replays_the_first_receipt_for_the_same_key() {
    let Some(pool) = live_submission_pool().await else {
        return;
    };
    let (app, token) = live_actor(&pool).await;
    let project = create_project(&app, &token, "Shot upload replay").await;
    let key = Uuid::new_v4().to_string();
    let (first_status, first) =
        call(&app, shot_upload_request_with_key(&token, &project, &key)).await;
    let (replay_status, replay) =
        call(&app, shot_upload_request_with_key(&token, &project, &key)).await;

    assert_eq!(first_status, StatusCode::CREATED, "{first}");
    assert_eq!(replay_status, StatusCode::CREATED, "{replay}");
    assert_eq!(replay, first, "same key and payload must replay exactly");
}

async fn shot_set_rejects_cross_project_duplicates_and_stale_revision() {
    let Some(pool) = live_submission_pool().await else {
        return;
    };
    let (app, token) = live_actor(&pool).await;
    let project_a = create_project(&app, &token, "Shot project A").await;
    let project_b = create_project(&app, &token, "Shot project B").await;

    let (upload_status, uploaded) = call(&app, shot_upload_request(&token, &project_b)).await;
    assert_eq!(upload_status, StatusCode::CREATED, "{uploaded}");
    let shot_id = uploaded["shot_artifact_id"].as_str().unwrap();

    let (cross_project_status, cross_project_body) = call(
        &app,
        json_request(
            &token,
            "PUT",
            &format!("/api/v1/bids/{project_a}/shots"),
            json!({"expected_revision":0,"shot_artifact_ids":[shot_id]}),
        ),
    )
    .await;
    let (duplicate_status, duplicate_body) = call(
        &app,
        json_request(
            &token,
            "PUT",
            &format!("/api/v1/bids/{project_b}/shots"),
            json!({"expected_revision":0,"shot_artifact_ids":[shot_id,shot_id]}),
        ),
    )
    .await;
    let (valid_status, valid_body) = call(
        &app,
        json_request(
            &token,
            "PUT",
            &format!("/api/v1/bids/{project_b}/shots"),
            json!({"expected_revision":0,"shot_artifact_ids":[shot_id]}),
        ),
    )
    .await;
    assert_eq!(valid_status, StatusCode::OK, "{valid_body}");
    assert_eq!(valid_body["revision"], 1);

    let (stale_revision_status, stale_revision_body) = call(
        &app,
        json_request(
            &token,
            "PUT",
            &format!("/api/v1/bids/{project_b}/shots"),
            json!({"expected_revision":0,"shot_artifact_ids":[shot_id]}),
        ),
    )
    .await;

    assert_eq!(
        cross_project_status,
        StatusCode::BAD_REQUEST,
        "{cross_project_body}"
    );
    assert_eq!(
        cross_project_body["error"]["code"],
        "SHOT_SET_ARTIFACTS_INVALID"
    );
    assert_eq!(
        duplicate_status,
        StatusCode::BAD_REQUEST,
        "{duplicate_body}"
    );
    assert_eq!(
        duplicate_body["error"]["code"],
        "SHOT_SET_ARTIFACTS_INVALID"
    );
    assert_eq!(
        stale_revision_status,
        StatusCode::CONFLICT,
        "{stale_revision_body}"
    );
    assert_eq!(
        stale_revision_body["error"]["code"],
        "SHOT_SET_REVISION_CAS_MISMATCH"
    );
}

async fn render_rejects_frozen_renderer_contract_mismatch() {
    let Some(pool) = live_submission_pool().await else {
        return;
    };
    let (app, token) = live_actor(&pool).await;
    let project = create_project(&app, &token, "Renderer contract mismatch").await;
    let (manifest_status, manifest) = call(
        &app,
        json_request(
            &token,
            "POST",
            &format!("/api/v1/bids/{project}/submission/manifests"),
            json!({"format":"docx"}),
        ),
    )
    .await;
    assert_eq!(manifest_status, StatusCode::CREATED, "{manifest}");
    let manifest_id = manifest["manifest_id"].as_str().unwrap();
    let manifest_uuid = Uuid::parse_str(manifest_id).unwrap();
    let manifest_sha256 = manifest["content_sha256"].as_str().unwrap();

    let mut transaction = pool.begin().await.unwrap();
    sqlx::query(
        "ALTER TABLE bid_submission_manifests
         DISABLE TRIGGER bid_submission_manifests_immutable",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE bid_submission_manifests
            SET end_state_identity=jsonb_set(
              end_state_identity,'{renderer_contract}',
              '{\"version\":\"knowledgebrain.bid.docx.incompatible\"}'::jsonb,false)
          WHERE id=$1",
    )
    .bind(manifest_uuid)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "ALTER TABLE bid_submission_manifests
         ENABLE TRIGGER bid_submission_manifests_immutable",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();

    let (render_status, render_body) = call(
        &app,
        json_request(
            &token,
            "POST",
            &format!("/api/v1/bids/{project}/submission/manifests/{manifest_id}/render"),
            json!({"expected_manifest_sha256":manifest_sha256}),
        ),
    )
    .await;
    assert_eq!(render_status, StatusCode::CONFLICT, "{render_body}");
    assert_eq!(render_body["error"]["code"], "RENDERER_CONTRACT_MISMATCH");
}

async fn render_job_status_is_durable_idempotent_and_project_scoped() {
    let Some(pool) = live_submission_pool().await else {
        return;
    };
    let (app, token) = live_actor(&pool).await;
    let project = create_project(&app, &token, "Durable render job").await;
    let other_project = create_project(&app, &token, "Other render scope").await;
    let (manifest_status, manifest) = call(
        &app,
        json_request(
            &token,
            "POST",
            &format!("/api/v1/bids/{project}/submission/manifests"),
            json!({"format":"docx"}),
        ),
    )
    .await;
    assert_eq!(manifest_status, StatusCode::CREATED, "{manifest}");
    let manifest_id = manifest["manifest_id"].as_str().unwrap();
    let manifest_sha256 = manifest["content_sha256"].as_str().unwrap();
    let render_uri = format!("/api/v1/bids/{project}/submission/manifests/{manifest_id}/render");
    let render_key = Uuid::new_v4().to_string();
    let render_body = json!({"expected_manifest_sha256":manifest_sha256});
    let (first_status, first) = call(
        &app,
        json_request_with_key(
            &token,
            "POST",
            &render_uri,
            render_body.clone(),
            &render_key,
        ),
    )
    .await;
    let (replay_status, replay) = call(
        &app,
        json_request_with_key(&token, "POST", &render_uri, render_body, &render_key),
    )
    .await;
    assert_eq!(first_status, StatusCode::ACCEPTED, "{first}");
    assert_eq!(replay_status, StatusCode::ACCEPTED, "{replay}");
    assert_eq!(first["status"], "queued");
    assert_eq!(replay["render_job_id"], first["render_job_id"]);
    let render_job_id = first["render_job_id"].as_str().unwrap();

    let (job_status, job) = call(
        &app,
        json_request(
            &token,
            "GET",
            &format!("/api/v1/bids/{project}/submission/render-jobs/{render_job_id}"),
            json!({}),
        ),
    )
    .await;
    assert_eq!(job_status, StatusCode::OK, "{job}");
    assert_eq!(job["render_job_id"], render_job_id);
    assert_eq!(job["manifest_id"], manifest_id);
    assert!(matches!(
        job["status"].as_str(),
        Some("pending" | "running")
    ));

    let (cross_status, cross_body) = call(
        &app,
        json_request(
            &token,
            "GET",
            &format!("/api/v1/bids/{other_project}/submission/render-jobs/{render_job_id}"),
            json!({}),
        ),
    )
    .await;
    assert_eq!(cross_status, StatusCode::NOT_FOUND, "{cross_body}");
}

async fn tender_upload_validates_bytes_before_staging_and_persists_media_type() {
    let Some(pool) = live_submission_pool().await else {
        return;
    };
    let (app, token) = live_actor(&pool).await;
    let project = create_project(&app, &token, "Tender upload validation").await;
    let pdf_key = Uuid::new_v4().to_string();
    let (invalid_pdf_status, invalid_pdf_body) = call(
        &app,
        document_upload_request_with_key(
            &token,
            &project,
            &pdf_key,
            "tender.pdf",
            "application/octet-stream",
            b"not a PDF",
        ),
    )
    .await;
    assert_eq!(
        invalid_pdf_status,
        StatusCode::BAD_REQUEST,
        "{invalid_pdf_body}"
    );
    assert_eq!(
        invalid_pdf_body["error"]["code"], "VALIDATION",
        "{invalid_pdf_body}"
    );

    let (pdf_status, pdf_body) = call(
        &app,
        document_upload_request_with_key(
            &token,
            &project,
            &pdf_key,
            "tender.pdf",
            "application/octet-stream",
            b"%PDF-1.7\n%%EOF\n",
        ),
    )
    .await;
    assert_eq!(pdf_status, StatusCode::CREATED, "{pdf_body}");

    let docx_key = Uuid::new_v4().to_string();
    let (invalid_docx_status, invalid_docx_body) = call(
        &app,
        document_upload_request_with_key(
            &token,
            &project,
            &docx_key,
            "tender.docx",
            "application/octet-stream",
            b"PK\x03\x04not-a-docx-package",
        ),
    )
    .await;
    assert_eq!(
        invalid_docx_status,
        StatusCode::BAD_REQUEST,
        "{invalid_docx_body}"
    );
    assert_eq!(
        invalid_docx_body["error"]["code"], "VALIDATION",
        "{invalid_docx_body}"
    );

    let (docx_status, docx_body) = call(
        &app,
        document_upload_request_with_key(
            &token,
            &project,
            &docx_key,
            "tender.docx",
            "application/octet-stream",
            &minimal_docx(),
        ),
    )
    .await;
    assert_eq!(docx_status, StatusCode::CREATED, "{docx_body}");

    let (list_status, list_body) = call(
        &app,
        json_request(
            &token,
            "GET",
            &format!("/api/v1/bids/{project}/documents"),
            json!({}),
        ),
    )
    .await;
    assert_eq!(list_status, StatusCode::OK, "{list_body}");
    let documents = list_body["documents"].as_array().unwrap();
    let pdf = documents
        .iter()
        .find(|document| document["id"] == pdf_body["id"])
        .unwrap();
    let docx = documents
        .iter()
        .find(|document| document["id"] == docx_body["id"])
        .unwrap();
    assert_eq!(pdf["media_type"], "application/pdf");
    assert_eq!(
        docx["media_type"],
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
    );
}

#[tokio::test]
async fn submission_http_contracts() {
    tender_upload_validates_bytes_before_staging_and_persists_media_type().await;
    regenerate_rejects_caller_defined_identities_at_json_boundary().await;
    regenerate_rejects_existing_dependency_cas_mismatches().await;
    bid_routes_and_list_are_isolated_by_project_owner().await;
    render_hides_cross_project_manifest_ids().await;
    shot_upload_replays_the_first_receipt_for_the_same_key().await;
    shot_set_rejects_cross_project_duplicates_and_stale_revision().await;
    render_rejects_frozen_renderer_contract_mismatch().await;
    render_job_status_is_durable_idempotent_and_project_scoped().await;
}
