//! Submission HTTP contract: server-derived dependencies, project-scoped IDs, ShotSet CAS.

use api::{AppState, router_with};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::PgPool;
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

fn attachment_upload_request(token: &str, project_id: &str, bytes: &[u8]) -> Request<Body> {
    attachment_upload_request_with_kind(token, project_id, "bid_bond", bytes)
}

fn attachment_upload_request_with_kind(
    token: &str,
    project_id: &str,
    kind: &str,
    bytes: &[u8],
) -> Request<Body> {
    let boundary = format!("kb-attachment-{}", Uuid::new_v4().simple());
    let mut body = Vec::new();
    body.extend_from_slice(
        format!("--{boundary}\r\nContent-Disposition: form-data; name=\"kind\"\r\n\r\n{kind}\r\n")
            .as_bytes(),
    );
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"bond.pdf\"\r\nContent-Type: application/pdf\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    Request::builder()
        .method("POST")
        .uri(format!("/api/v1/bids/{project_id}/attachments"))
        .header("authorization", format!("Bearer {token}"))
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .header("idempotency-key", Uuid::new_v4().to_string())
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

fn stored_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut output = Vec::new();
    let mut central_entries = Vec::new();
    for &(name, data) in files {
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

fn minimal_docx() -> Vec<u8> {
    stored_zip(&[
        (
            "[Content_Types].xml",
            br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Tender</w:t></w:r></w:p><w:sectPr/></w:body></w:document>"#,
        ),
        (
            "word/_rels/document.xml.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>"#,
        ),
    ])
}

fn xlsx_with_sheet(sheet_xml: &[u8]) -> Vec<u8> {
    stored_zip(&[
        (
            "[Content_Types].xml",
            br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
        ),
        (
            "xl/workbook.xml",
            br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
        ),
        (
            "xl/_rels/workbook.xml.rels",
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
        ),
        ("xl/worksheets/sheet1.xml", sheet_xml),
    ])
}

fn minimal_xlsx() -> Vec<u8> {
    xlsx_with_sheet(
        br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><dimension ref="A1:A1"/><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Tender</t></is></c></row></sheetData></worksheet>"#,
    )
}

fn sparse_xlsx() -> Vec<u8> {
    xlsx_with_sheet(
        br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><dimension ref="A1:XFD1048576"/><sheetData><row r="1048576"><c r="XFD1048576" t="inlineStr"><is><t>bomb</t></is></c></row></sheetData></worksheet>"#,
    )
}

fn minimal_pdf() -> Vec<u8> {
    let objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>".as_slice(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".as_slice(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << >> >>".as_slice(),
    ];
    let mut result = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (index, value) in objects.iter().enumerate() {
        offsets.push(result.len());
        result.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        result.extend_from_slice(value);
        result.extend_from_slice(b"\nendobj\n");
    }
    let xref = result.len();
    result.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        result.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    result.extend_from_slice(
        format!(
            "trailer << /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    result
}

async fn live_submission_pool() -> Option<PgPool> {
    let pool = match platform::connect().await {
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
    let gate_generation: Option<i64> = sqlx::query_scalar(
        "WITH changed AS (
           UPDATE application_maintenance_gate
              SET mode='open',generation=generation+1,
                  updated_by='system:first-launch',updated_at=clock_timestamp()
            WHERE singleton_key AND mode='maintenance'
            RETURNING generation
         ), audited AS (
           INSERT INTO maintenance_gate_audit(
             id,from_mode,to_mode,generation,actor_identity,reason)
           SELECT $1,'maintenance','open',generation,'system:first-launch',
                  'open isolated Submission HTTP contract fixture'
             FROM changed
         )
         SELECT generation FROM changed",
    )
    .bind(Uuid::new_v4())
    .fetch_optional(&pool)
    .await
    .expect("open Submission HTTP contract maintenance gate");
    if gate_generation.is_none() {
        let mode: String =
            sqlx::query_scalar("SELECT mode FROM application_maintenance_gate WHERE singleton_key")
                .fetch_one(&pool)
                .await
                .expect("read Submission HTTP contract maintenance gate");
        assert_eq!(mode, "open");
    }
    Some(pool)
}

async fn live_actor(pool: &PgPool) -> (axum::Router, String) {
    let user_id = Uuid::new_v4();
    knowledge::insert_user(
        pool,
        user_id,
        &format!("submission-contract-{user_id}@invalid.test"),
        None,
    )
    .await
    .unwrap();
    let token = platform::issue_jwt(user_id, "submission-contract-secret").unwrap();
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

async fn procedural_listing_includes_frozen_segment_text() {
    let Some(pool) = live_submission_pool().await else {
        return;
    };
    let (app, token) = live_actor(&pool).await;
    let project = create_project(&app, &token, "Procedural segment context").await;
    let project_id = Uuid::parse_str(&project).unwrap();
    let owner_id: Uuid = sqlx::query_scalar("SELECT owner_user_id FROM bid_projects WHERE id=$1")
        .bind(project_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let actor = format!("user:{owner_id}");
    let clause_id = Uuid::new_v4();
    let segment_id = Uuid::new_v4();
    let classification_id = Uuid::new_v4();
    let segment_text = "提交授权委托书原件";
    let segment_sha256 = platform::sha256_hex(segment_text.as_bytes());
    let stable_key = platform::sha256_hex(format!("{clause_id}:{segment_sha256}").as_bytes());

    sqlx::query(
        "INSERT INTO bid_clauses(
           id,project_id,provenance,status,kind,text,must,revision,created_by)
         VALUES($1,$2,'manual','confirmed','procedural',$3,true,2,$4)",
    )
    .bind(clause_id)
    .bind(project_id)
    .bind(segment_text)
    .bind(&actor)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO bid_procedural_segment_artifacts(
           id,project_id,clause_id,stable_key,segmentation_version,start_offset,end_offset,
           segment_utf8,segment_sha256,provenance)
         VALUES($1,$2,$3,$4,'procedural-segment-v1',0,$5,$6,$7,'manual')",
    )
    .bind(segment_id)
    .bind(project_id)
    .bind(clause_id)
    .bind(stable_key)
    .bind(segment_text.len() as i64)
    .bind(segment_text.as_bytes())
    .bind(segment_sha256)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO bid_procedural_classification_artifacts(
           id,project_id,segment_id,revision,router_contract_version,router_promotion_generation,
           router_result_status,router_requirement_kind,effective_requirement_kind,lifecycle_status)
         VALUES($1,$2,$3,1,'procedural-router-v1',0,'classified',
                'authorization_support','authorization_support','current')",
    )
    .bind(classification_id)
    .bind(project_id)
    .bind(segment_id)
    .execute(&pool)
    .await
    .unwrap();

    let mut runtime_role = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE kb_runtime_api")
        .execute(&mut *runtime_role)
        .await
        .unwrap();
    let runtime_visible_text: String = sqlx::query_scalar(
        "SELECT segment_text
           FROM bidding_current_procedural_classifications
          WHERE id=$1",
    )
    .bind(classification_id)
    .fetch_one(&mut *runtime_role)
    .await
    .unwrap();
    assert_eq!(runtime_visible_text, segment_text);
    runtime_role.rollback().await.unwrap();

    let (status, body) = call(
        &app,
        json_request(
            &token,
            "GET",
            &format!("/api/v1/bids/{project}/procedural-requirements"),
            Value::Null,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["classifications"][0]["segment_text"], segment_text,
        "operators need the frozen segment body to resolve the requirement"
    );

    let resolve_uri =
        format!("/api/v1/bids/{project}/procedural-requirements/{classification_id}/resolve");
    let (missing_attachment_status, missing_attachment_body) = call(
        &app,
        json_request(
            &token,
            "POST",
            &resolve_uri,
            json!({"resolution":"satisfied_by_attachment"}),
        ),
    )
    .await;
    assert_eq!(
        missing_attachment_status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{missing_attachment_body}"
    );
    assert_eq!(
        missing_attachment_body["error"]["code"],
        "PROCEDURAL_RESOLUTION_INVALID"
    );

    let (unknown_attachment_status, unknown_attachment_body) = call(
        &app,
        json_request(
            &token,
            "POST",
            &resolve_uri,
            json!({
                "resolution":"satisfied_by_attachment",
                "attachment_id":Uuid::new_v4()
            }),
        ),
    )
    .await;
    assert_eq!(
        unknown_attachment_status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{unknown_attachment_body}"
    );
    assert_eq!(
        unknown_attachment_body["error"]["code"],
        "ATTACHMENT_NOT_VALID"
    );

    let (invalid_resolution_status, invalid_resolution_body) = call(
        &app,
        json_request(
            &token,
            "POST",
            &resolve_uri,
            json!({"resolution":"confirmed_by_user"}),
        ),
    )
    .await;
    assert_eq!(
        invalid_resolution_status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{invalid_resolution_body}"
    );
    assert_eq!(
        invalid_resolution_body["error"]["code"],
        "PROCEDURAL_RESOLUTION_INVALID"
    );

    let override_uri =
        format!("/api/v1/bids/{project}/procedural-classifications/{classification_id}/override");
    let (invalid_override_status, invalid_override_body) = call(
        &app,
        json_request(
            &token,
            "POST",
            &override_uri,
            json!({"effective_kind":"invalid","reason":"invalid contract probe"}),
        ),
    )
    .await;
    assert_eq!(
        invalid_override_status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{invalid_override_body}"
    );
    assert_eq!(
        invalid_override_body["error"]["code"],
        "PROCEDURAL_OVERRIDE_INVALID"
    );

    let (override_status, override_body) = call(
        &app,
        json_request(
            &token,
            "POST",
            &override_uri,
            json!({"effective_kind":"bid_bond","reason":"exercise stale classification"}),
        ),
    )
    .await;
    assert_eq!(override_status, StatusCode::OK, "{override_body}");
    let (stale_status, stale_body) = call(
        &app,
        json_request(
            &token,
            "POST",
            &override_uri,
            json!({"effective_kind":"seal_sample","reason":"stale classification probe"}),
        ),
    )
    .await;
    assert_eq!(stale_status, StatusCode::CONFLICT, "{stale_body}");
    assert_eq!(
        stale_body["error"]["code"],
        "PROCEDURAL_CLASSIFICATION_NOT_CURRENT"
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
    knowledge::insert_user(
        &pool,
        owner_b,
        &format!("submission-owner-{owner_b}@invalid.test"),
        None,
    )
    .await
    .unwrap();
    let owner_b_token = platform::issue_jwt(owner_b, "submission-contract-secret").unwrap();
    let project_a = create_project(&app, &owner_a_token, "Owner A project").await;
    let project_b = create_project(&app, &owner_b_token, "Owner B project").await;

    let (anonymous_status, anonymous_body) = call(
        &app,
        Request::builder()
            .method("GET")
            .uri(format!("/api/v1/bids/{project_a}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(
        anonymous_status,
        StatusCode::UNAUTHORIZED,
        "{anonymous_body}"
    );
    assert_eq!(anonymous_body["error"]["code"], "UNAUTHORIZED");

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
    assert_eq!(cross_owner_body["error"]["code"], "NOT_FOUND");

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

    let (family_status, family_body) = call(
        &app,
        json_request(
            &owner_a_token,
            "POST",
            &format!("/api/v1/bids/{project_a}/clauses"),
            json!({"text":"x","kind":"technical","must":true,"family":"technical"}),
        ),
    )
    .await;
    assert_ne!(family_status, StatusCode::OK, "{family_body}");
    assert_ne!(family_status, StatusCode::CREATED, "{family_body}");
    assert_ne!(family_body["error"]["code"], "NOT_FOUND", "{family_body}");

    for uri in [
        format!("/api/v1/bids/{project_a}/matching"),
        format!("/api/v1/bids/{project_a}/quote"),
        format!("/api/v1/bids/{project_a}/gate-issues"),
        format!("/api/v1/bids/{project_a}/parts"),
        format!("/api/v1/bids/{project_a}/company-profile"),
        format!("/api/v1/bids/{project_a}/submission-profile"),
    ] {
        let (status, body) = call(&app, json_request(&owner_a_token, "GET", &uri, json!({}))).await;
        assert_ne!(status, StatusCode::NOT_FOUND, "missing V1 GET {uri}");
        assert_ne!(body["error"]["code"], "NOT_FOUND", "missing V1 GET {uri}");
    }
}

async fn bid_sql_error_contracts_map_to_stable_http_statuses() {
    let Some(pool) = live_submission_pool().await else {
        return;
    };
    let (app, owner_token) = live_actor(&pool).await;

    let (past_end_status, past_end_body) = call(
        &app,
        json_request(
            &owner_token,
            "POST",
            "/api/v1/bids",
            json!({"title":"Past end","ends_at":"2000-01-01T00:00:00Z"}),
        ),
    )
    .await;
    assert_eq!(past_end_status, StatusCode::BAD_REQUEST, "{past_end_body}");
    assert_eq!(
        past_end_body["error"]["code"], "PROJECT_END_MUST_BE_FUTURE",
        "{past_end_body}"
    );

    let project = create_project(&app, &owner_token, "HTTP error mapping").await;
    let (clause_status, clause) = call(
        &app,
        json_request(
            &owner_token,
            "POST",
            &format!("/api/v1/bids/{project}/clauses"),
            json!({"text":"提供资质证明","kind":"qualification","must":true}),
        ),
    )
    .await;
    assert_eq!(clause_status, StatusCode::CREATED, "{clause}");
    let clause_id = clause["id"].as_str().unwrap();
    let clause_uri = format!("/api/v1/bids/{project}/clauses/{clause_id}");
    let (stale_status, stale_body) = call(
        &app,
        json_request(
            &owner_token,
            "PATCH",
            &clause_uri,
            json!({"action":"patch","expected_revision":0,"patch":{"text":"更新资质证明"}}),
        ),
    )
    .await;
    assert_eq!(stale_status, StatusCode::CONFLICT, "{stale_body}");
    assert_eq!(
        stale_body["error"]["code"], "CLAUSE_REVISION_CAS_MISMATCH",
        "{stale_body}"
    );

    let other_owner = Uuid::new_v4();
    knowledge::insert_user(
        &pool,
        other_owner,
        &format!("submission-error-owner-{other_owner}@invalid.test"),
        None,
    )
    .await
    .unwrap();
    let other_token = platform::issue_jwt(other_owner, "submission-contract-secret").unwrap();
    let (hidden_status, hidden_body) = call(
        &app,
        json_request(
            &other_token,
            "PATCH",
            &clause_uri,
            json!({"action":"patch","expected_revision":0,"patch":{"text":"越权更新"}}),
        ),
    )
    .await;
    assert_eq!(hidden_status, StatusCode::NOT_FOUND, "{hidden_body}");
    assert_eq!(hidden_body["error"]["code"], "NOT_FOUND", "{hidden_body}");

    let (upload_status, uploaded) = call(
        &app,
        attachment_upload_request(
            &owner_token,
            &project,
            b"%PDF-1.7\n% pending preparation\n%%EOF\n",
        ),
    )
    .await;
    assert_eq!(upload_status, StatusCode::CREATED, "{uploaded}");
    assert_eq!(uploaded["preparation_status"], "pending", "{uploaded}");
    let attachment_id = uploaded["id"].as_str().unwrap();
    let (preparation_status, preparation_body) = call(
        &app,
        json_request(
            &owner_token,
            "POST",
            &format!("/api/v1/bids/{project}/attachments/{attachment_id}/validate"),
            json!({"expected_revision":1}),
        ),
    )
    .await;
    assert_eq!(
        preparation_status,
        StatusCode::CONFLICT,
        "{preparation_body}"
    );
    assert_eq!(
        preparation_body["error"]["code"], "ATTACHMENT_PREPARATION_INCOMPLETE",
        "{preparation_body}"
    );

    let (invalid_kind_status, invalid_kind_body) = call(
        &app,
        attachment_upload_request_with_kind(
            &owner_token,
            &project,
            "unknown_material",
            ONE_PIXEL_PNG,
        ),
    )
    .await;
    assert_eq!(
        invalid_kind_status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{invalid_kind_body}"
    );
    assert_eq!(
        invalid_kind_body["error"]["code"],
        "ATTACHMENT_KIND_INVALID"
    );

    let quote_project = create_project(&app, &owner_token, "Quote review error mapping").await;
    let quote_project_id = Uuid::parse_str(&quote_project).unwrap();
    let (draft_status, draft) = call(
        &app,
        json_request(
            &owner_token,
            "POST",
            &format!("/api/v1/bids/{quote_project}/quote/draft"),
            json!({"tax_mode":"tax_inclusive","title":"Error contract quote","notes":null}),
        ),
    )
    .await;
    assert_eq!(draft_status, StatusCode::CREATED, "{draft}");
    let line_id = Uuid::new_v4();
    let (line_status, line) = call(
        &app,
        json_request(
            &owner_token,
            "PUT",
            &format!("/api/v1/bids/{quote_project}/quote/lines/{line_id}"),
            json!({
                "expected_edit_version":0,
                "ordinal":0,
                "description":"Complete line",
                "pricing_mode":"lump_sum",
                "quantity":null,
                "unit":null,
                "unit_price":null,
                "entered_amount":"100.00",
                "tax_rate":"0.130000",
                "user_confirmed":true
            }),
        ),
    )
    .await;
    assert_eq!(line_status, StatusCode::OK, "{line}");
    assert_eq!(line["complete"], true, "{line}");

    let (fact_revision, ceiling_revision, ceiling_identity_sha256): (i64, i64, String) =
        sqlx::query_as(
            "SELECT fact_revision,ceiling_revision,ceiling_identity_sha256
               FROM bid_projects WHERE id=$1",
        )
        .bind(quote_project_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let (pricing_revision, pricing_set_sha256): (i64, String) = sqlx::query_as(
        "SELECT revision,content_sha256 FROM bid_clause_set_identities
          WHERE project_id=$1 AND set_kind='pricing'",
    )
    .bind(quote_project_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let finalize_uri = format!("/api/v1/bids/{quote_project}/quote/finalize");
    let (review_required_status, review_required_body) = call(
        &app,
        json_request(
            &owner_token,
            "POST",
            &finalize_uri,
            json!({
                "expected_edit_version":1,
                "expected_fact_revision":fact_revision,
                "expected_ceiling_revision":ceiling_revision,
                "expected_ceiling_identity_sha256":ceiling_identity_sha256,
                "expected_pricing_revision":pricing_revision,
                "expected_pricing_set_sha256":pricing_set_sha256,
                "no_ceiling_reviewed":false,
                "no_ceiling_reason":null
            }),
        ),
    )
    .await;
    assert_eq!(
        review_required_status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{review_required_body}"
    );
    assert_eq!(
        review_required_body["error"]["code"],
        "NO_CEILING_REVIEW_REQUIRED"
    );

    let (fact_status, fact) = call(
        &app,
        json_request(
            &owner_token,
            "POST",
            &format!("/api/v1/bids/{quote_project}/facts"),
            json!({
                "action":"set",
                "expected_fact_revision":fact_revision,
                "candidate_id":null,
                "field":"ceiling_price",
                "typed_value":{
                    "amount":"1000.00",
                    "currency_code":"CNY",
                    "basis":"tax_inclusive"
                },
                "reason":null,
                "override_reason":null
            }),
        ),
    )
    .await;
    assert_eq!(fact_status, StatusCode::OK, "{fact}");
    let (ceiling_conflict_status, ceiling_conflict_body) = call(
        &app,
        json_request(
            &owner_token,
            "POST",
            &finalize_uri,
            json!({
                "expected_edit_version":1,
                "expected_fact_revision":fact["fact_revision"],
                "expected_ceiling_revision":fact["ceiling_revision"],
                "expected_ceiling_identity_sha256":fact["ceiling_identity_sha256"],
                "expected_pricing_revision":pricing_revision,
                "expected_pricing_set_sha256":pricing_set_sha256,
                "no_ceiling_reviewed":true,
                "no_ceiling_reason":"previous no-ceiling review"
            }),
        ),
    )
    .await;
    assert_eq!(
        ceiling_conflict_status,
        StatusCode::CONFLICT,
        "{ceiling_conflict_body}"
    );
    assert_eq!(
        ceiling_conflict_body["error"]["code"],
        "QUOTE_CEILING_REVIEW_CONFLICT"
    );
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
            "application/pdf",
            b"%PDF-1.7\n%%EOF\n",
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

    let (wrong_mime_status, wrong_mime_body) = call(
        &app,
        document_upload_request_with_key(
            &token,
            &project,
            &Uuid::new_v4().to_string(),
            "tender.pdf",
            "application/octet-stream",
            &minimal_pdf(),
        ),
    )
    .await;
    assert_eq!(
        wrong_mime_status,
        StatusCode::BAD_REQUEST,
        "{wrong_mime_body}"
    );

    let (pdf_status, pdf_body) = call(
        &app,
        document_upload_request_with_key(
            &token,
            &project,
            &pdf_key,
            "tender.pdf",
            "application/pdf",
            &minimal_pdf(),
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
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
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
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            &minimal_docx(),
        ),
    )
    .await;
    assert_eq!(docx_status, StatusCode::CREATED, "{docx_body}");

    let (sparse_status, sparse_body) = call(
        &app,
        document_upload_request_with_key(
            &token,
            &project,
            &Uuid::new_v4().to_string(),
            "sparse.xlsx",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            &sparse_xlsx(),
        ),
    )
    .await;
    assert_eq!(sparse_status, StatusCode::BAD_REQUEST, "{sparse_body}");
    assert_eq!(sparse_body["error"]["code"], "VALIDATION", "{sparse_body}");

    let xlsx_key = Uuid::new_v4().to_string();
    let (xlsx_status, xlsx_body) = call(
        &app,
        document_upload_request_with_key(
            &token,
            &project,
            &xlsx_key,
            "tender.xlsx",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            &minimal_xlsx(),
        ),
    )
    .await;
    assert_eq!(xlsx_status, StatusCode::CREATED, "{xlsx_body}");

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
    let xlsx = documents
        .iter()
        .find(|document| document["id"] == xlsx_body["id"])
        .unwrap();
    assert_eq!(pdf["media_type"], "application/pdf");
    assert_eq!(
        docx["media_type"],
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
    );
    assert_eq!(
        xlsx["media_type"],
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
    );
}

#[tokio::test]
async fn submission_http_contracts() {
    bid_sql_error_contracts_map_to_stable_http_statuses().await;
    bid_routes_and_list_are_isolated_by_project_owner().await;
    tender_upload_validates_bytes_before_staging_and_persists_media_type().await;
    procedural_listing_includes_frozen_segment_text().await;
    regenerate_rejects_caller_defined_identities_at_json_boundary().await;
    regenerate_rejects_existing_dependency_cas_mismatches().await;
    render_hides_cross_project_manifest_ids().await;
    shot_upload_replays_the_first_receipt_for_the_same_key().await;
    shot_set_rejects_cross_project_duplicates_and_stale_revision().await;
    render_rejects_frozen_renderer_contract_mismatch().await;
    render_job_status_is_durable_idempotent_and_project_scoped().await;
}
