//! Exercise a persisted saved version through the public route and an actual source fetch.
use super::*;
use axum::http::HeaderMap;
use jsonwebtoken::{EncodingKey, Header, encode};
use reqwest::{Client, Url};
use std::sync::atomic::{AtomicBool, Ordering};

const OFFICE_SECRET: &[u8] = b"saved-pdf-loopback-office-fixture";
const SOURCE_SECRET: &[u8] = b"saved-pdf-loopback-source-fixture";
const OLD: &[u8] = b"PK\x03\x04original saved Word";
const NEW: &[u8] = b"PK\x03\x04later manual Word";

#[derive(Clone)]
struct Converter {
    pool: PgPool,
    client: Client,
    api_origin: Url,
    office_origin: Url,
    project: Uuid,
    old: Uuid,
    new: Uuid,
    reject: Arc<AtomicBool>,
    sources: Arc<Mutex<Vec<(Uuid, Url)>>>,
}

fn decoded_token(token: &str, secret: &[u8]) -> Value {
    let mut rules = Validation::new(Algorithm::HS256);
    rules.required_spec_claims.remove("exp");
    rules.validate_exp = false;
    rules.validate_aud = false;
    decode::<Value>(token, &DecodingKey::from_secret(secret), &rules)
        .unwrap()
        .claims
}

fn expired_source(source: &Url) -> Url {
    let token = source
        .query_pairs()
        .find(|(key, _)| key == "token")
        .unwrap()
        .1;
    let mut claims = decoded_token(&token, SOURCE_SECRET);
    claims["exp"] = json!(now() - 1);
    let expired = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(SOURCE_SECRET),
    )
    .unwrap();
    let mut url = source.clone();
    url.set_query(None);
    url.query_pairs_mut().append_pair("token", &expired);
    url
}

async fn converter(State(f): State<Converter>, Json(mut command): Json<Value>) -> Json<Value> {
    let token = command.as_object_mut().unwrap().remove("token").unwrap();
    assert_eq!(
        decoded_token(token.as_str().unwrap(), OFFICE_SECRET),
        command
    );
    assert_eq!(command["filetype"], "docx");
    assert_eq!(command["outputtype"], "pdf");
    let source = Url::parse(command["url"].as_str().unwrap()).unwrap();
    assert_eq!(source.origin(), f.api_origin.origin());
    let path: Vec<_> = source.path_segments().unwrap().collect();
    assert_eq!(&path[..3], ["api", "internal", "tender-saved-pdf"]);
    assert_eq!(path[3], f.project.to_string());
    let version: Uuid = path[4].parse().unwrap();
    let expected = if version == f.old {
        OLD
    } else {
        assert_eq!(version, f.new);
        NEW
    };

    // Advance the persisted current pointer after the request entered the converter.
    // The fetch below must still use the version addressed by the request.
    sqlx::query("UPDATE tender_draft_state SET version_id=$2 WHERE project_id=$1")
        .bind(f.project)
        .bind(f.new)
        .execute(&f.pool)
        .await
        .unwrap();
    let expired = f.client.get(expired_source(&source)).send().await.unwrap();
    assert_eq!(expired.status(), StatusCode::UNAUTHORIZED);
    let fetched = f.client.get(source.clone()).send().await.unwrap();
    assert_eq!(fetched.status(), StatusCode::OK);
    assert_eq!(fetched.bytes().await.unwrap().as_ref(), expected);
    f.sources.lock().unwrap().push((version, source));

    if f.reject.load(Ordering::SeqCst) {
        return Json(json!({"error":-3}));
    }
    let mut download = f.office_origin.join("fixture.pdf").unwrap();
    download
        .query_pairs_mut()
        .append_pair("source", &platform::sha256_hex(expected));
    Json(json!({"endConvert":true,"fileType":"pdf","fileUrl":download}))
}

#[derive(Deserialize)]
struct PdfResult {
    source: String,
}

async fn converted(Query(result): Query<PdfResult>) -> Response {
    (
        [("content-type", "application/pdf")],
        pdf_bytes(&result.source),
    )
        .into_response()
}

fn pdf_bytes(source: &str) -> String {
    format!("%PDF-1.7\nfixture PDF for source {source}\n")
}

async fn insert_version(
    pool: &PgPool,
    project: Uuid,
    owner: Uuid,
    revision: i64,
    sha: &str,
    byte_length: i64,
) -> Uuid {
    let version = Uuid::new_v4();
    let object = platform::object_ref(sha);
    sqlx::query("INSERT INTO platform_objects(object_ref,content_sha256,media_type,byte_length,state) VALUES($1,$2,$3,$4,'available')")
        .bind(&object).bind(sha).bind(DOCX).bind(byte_length).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO tender_draft_versions(id,project_id,execution_id,round_id,revision,source,object_ref,content_sha256,byte_length,outline,created_by_user_id) VALUES($1,$2,'saved-pdf-fixture',$3,$4,'manual',$5,$6,$7,'{}',$8)")
        .bind(version).bind(project).bind(Uuid::new_v4()).bind(revision).bind(object)
        .bind(sha).bind(byte_length).bind(owner).execute(pool).await.unwrap();
    version
}

fn assert_pdf_headers(headers: &HeaderMap, version: Uuid, sha: &str, revision: i64, pdf: &str) {
    assert_eq!(headers["content-type"], "application/pdf");
    assert_eq!(headers["x-kb-render-source-version"], version.to_string());
    assert_eq!(headers["x-kb-render-source-sha256"], sha);
    assert_eq!(
        headers["x-kb-render-pdf-sha256"],
        platform::sha256_hex(pdf.as_bytes())
    );
    let disposition = headers["content-disposition"].to_str().unwrap();
    assert!(disposition.starts_with("attachment;"));
    assert!(disposition.contains(&format!("v{revision}.pdf")));
}

#[tokio::test]
#[ignore = "requires empty task-owned loopback PostgreSQL and OBJECT_DIR"]
async fn saved_pdf_http_pins_version_bytes_and_enforces_conversion_boundaries() {
    let dsn = std::env::var("KB_TENDER_SAVED_PDF_TEST_DATABASE_URL").unwrap();
    assert_eq!(std::env::var("DATABASE_URL").unwrap(), dsn);
    let dsn = Url::parse(&dsn).unwrap();
    assert_eq!(dsn.host_str(), Some("127.0.0.1"));
    assert!(dsn.path().starts_with("/knowledgebrain_test_saved_pdf_"));
    assert!(
        std::env::var("OBJECT_DIR")
            .unwrap()
            .starts_with("/opt/workspace/kb-tender-evidence/")
    );
    assert!(!platform::s3_configured());
    let pool = platform::connect().await.unwrap();
    let empty: bool = sqlx::query_scalar("SELECT to_regclass('public.tender_projects') IS NULL")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        empty,
        "the saved-PDF fixture must migrate an empty owned database"
    );
    platform::migrate_database(&pool).await.unwrap();

    let owner = Uuid::new_v4();
    let outsider = Uuid::new_v4();
    let project = Uuid::new_v4();
    let other_project = Uuid::new_v4();
    for user in [owner, outsider] {
        sqlx::query("INSERT INTO platform_users(id,email) VALUES($1,$2)")
            .bind(user)
            .bind(format!("saved-pdf-{user}@example.invalid"))
            .execute(&pool)
            .await
            .unwrap();
    }
    for id in [project, other_project] {
        sqlx::query("INSERT INTO tender_projects(id,owner_user_id,title,status) VALUES($1,$2,'saved PDF fixture','open')")
            .bind(id).bind(owner).execute(&pool).await.unwrap();
    }
    let old_sha = platform::sha256_hex(OLD);
    let new_sha = platform::sha256_hex(NEW);
    let old = insert_version(&pool, project, owner, 1, &old_sha, OLD.len() as i64).await;
    let new = insert_version(&pool, project, owner, 2, &new_sha, NEW.len() as i64).await;
    platform::write_blob_async(&old_sha, OLD).await.unwrap();
    platform::write_blob_async(&new_sha, NEW).await.unwrap();
    sqlx::query("INSERT INTO tender_draft_state(project_id,version_id) VALUES($1,$2)")
        .bind(project)
        .bind(old)
        .execute(&pool)
        .await
        .unwrap();

    let broken_bytes = b"PK\x03\x04mutated bytes";
    let corrupt_sha = platform::sha256_hex(b"PK\x03\x04correct bytes");
    let corrupt = insert_version(
        &pool,
        project,
        owner,
        3,
        &corrupt_sha,
        broken_bytes.len() as i64,
    )
    .await;
    platform::write_blob_async(&corrupt_sha, broken_bytes)
        .await
        .unwrap();
    let mut length_mismatches = Vec::new();
    for (revision, delta) in [(4, 1), (5, -1)] {
        let bytes = format!("PK\x03\x04length mismatch fixture {revision}").into_bytes();
        let sha = platform::sha256_hex(&bytes);
        let version = insert_version(
            &pool,
            project,
            owner,
            revision,
            &sha,
            bytes.len() as i64 + delta,
        )
        .await;
        platform::write_blob_async(&sha, &bytes).await.unwrap();
        length_mismatches.push((version, sha));
    }
    let oversized_sha = platform::sha256_hex(b"oversized missing fixture");
    let oversized = insert_version(
        &pool,
        project,
        owner,
        6,
        &oversized_sha,
        MAX_BYTES as i64 + 1,
    )
    .await;

    let api = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let office = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let api_origin = Url::parse(&format!("http://{}/", api.local_addr().unwrap())).unwrap();
    let office_origin = Url::parse(&format!("http://{}/", office.local_addr().unwrap())).unwrap();
    // This ignored fixture runs by itself in a dedicated process with --test-threads=1.
    unsafe {
        std::env::set_var("KB_ONLYOFFICE_API_ORIGIN", api_origin.as_str());
        std::env::set_var("KB_ONLYOFFICE_SERVER_ORIGIN", office_origin.as_str());
        std::env::set_var("KB_ONLYOFFICE_COMMAND_ORIGIN", office_origin.as_str());
        std::env::set_var(
            "KB_ONLYOFFICE_JWT_SECRET",
            std::str::from_utf8(OFFICE_SECRET).unwrap(),
        );
        std::env::set_var(
            "KB_ONLYOFFICE_CAPABILITY_SECRET",
            std::str::from_utf8(SOURCE_SECRET).unwrap(),
        );
    }
    let client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    let stub = Converter {
        pool: pool.clone(),
        client: client.clone(),
        api_origin: api_origin.clone(),
        office_origin,
        project,
        old,
        new,
        reject: Arc::new(AtomicBool::new(false)),
        sources: Arc::new(Mutex::new(Vec::new())),
    };
    let server = Router::new()
        .route("/converter", post(converter))
        .route("/fixture.pdf", get(converted))
        .with_state(stub.clone());
    let office_task = tokio::spawn(async move { axum::serve(office, server).await.unwrap() });
    let state = AppState {
        jwt_secret: "saved-pdf-loopback-user-fixture".into(),
        bootstrap_key: String::new(),
    };
    let owner_token = platform::issue_jwt(owner, &state.jwt_secret).unwrap();
    let other_token = platform::issue_jwt(outsider, &state.jwt_secret).unwrap();
    let app = router()
        .merge(crate::tender_editor::router())
        .with_state(state);
    let api_task = tokio::spawn(async move { axum::serve(api, app).await.unwrap() });

    let version_url = |id, version, operation| {
        api_origin
            .join(&format!(
                "api/v3/tender-projects/{id}/docx/versions/{version}/{operation}"
            ))
            .unwrap()
    };
    let pdf_request = |id, version, sha: &str, token: &str| {
        client
            .post(version_url(id, version, "pdf"))
            .bearer_auth(token)
            .json(&json!({"docx_sha256":sha}))
    };
    for (id, version, sha, token, status) in [
        (project, old, &new_sha, &owner_token, StatusCode::CONFLICT),
        (project, old, &old_sha, &other_token, StatusCode::NOT_FOUND),
        (
            other_project,
            old,
            &old_sha,
            &owner_token,
            StatusCode::NOT_FOUND,
        ),
        (
            project,
            oversized,
            &oversized_sha,
            &owner_token,
            StatusCode::PAYLOAD_TOO_LARGE,
        ),
        (
            project,
            corrupt,
            &corrupt_sha,
            &owner_token,
            StatusCode::CONFLICT,
        ),
    ] {
        let response = pdf_request(id, version, sha, token).send().await.unwrap();
        assert_eq!(
            response.status(),
            status,
            "{}",
            response.text().await.unwrap()
        );
    }
    for (version, sha) in length_mismatches {
        let response = pdf_request(project, version, &sha, &owner_token)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }
    assert!(stub.sources.lock().unwrap().is_empty());
    let current: Uuid =
        sqlx::query_scalar("SELECT version_id FROM tender_draft_state WHERE project_id=$1")
            .bind(project)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(current, old);

    {
        let _all_slots = SLOTS.try_acquire_many(4).unwrap();
        let response = pdf_request(project, old, &old_sha, &owner_token)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
    for (version, sha, revision, docx) in [(old, &old_sha, 1, OLD), (new, &new_sha, 2, NEW)] {
        let response = pdf_request(project, version, sha, &owner_token)
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "{}",
            response.text().await.unwrap()
        );
        let pdf = pdf_bytes(sha);
        assert_pdf_headers(response.headers(), version, sha, revision, &pdf);
        assert_eq!(response.text().await.unwrap(), pdf);
        let response = client
            .get(version_url(project, version, "download"))
            .bearer_auth(&owner_token)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.bytes().await.unwrap().as_ref(), docx);
    }
    stub.reject.store(true, Ordering::SeqCst);
    let failed = pdf_request(project, old, &old_sha, &owner_token)
        .send()
        .await
        .unwrap();
    assert_eq!(failed.status(), StatusCode::SERVICE_UNAVAILABLE);
    let sources = stub.sources.lock().unwrap().clone();
    assert_eq!(
        sources
            .iter()
            .map(|(version, _)| *version)
            .collect::<Vec<_>>(),
        [old, new, old]
    );
    for (_, source) in sources {
        let response = client.get(source).send().await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "conversion source must disappear on success and failure"
        );
    }
    api_task.abort();
    office_task.abort();
}
