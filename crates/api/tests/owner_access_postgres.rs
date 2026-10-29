//! Runs only against an explicitly selected disposable database; no model calls.
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

async fn request(
    app: &Router,
    method: &str,
    path: &str,
    credential: (&str, &str),
    body: Value,
) -> (StatusCode, Vec<u8>) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header(credential.0, credential.1)
                .header("idempotency-key", Uuid::new_v4().to_string())
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    (
        response.status(),
        to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
}

async fn json_request(
    app: &Router,
    method: &str,
    path: &str,
    credential: (&str, &str),
    body: Value,
) -> (StatusCode, Value) {
    let (status, bytes) = request(app, method, path, credential, body).await;
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
#[ignore = "requires an isolated initialized database and OBJECT_DIR"]
async fn users_and_their_keys_share_identity_without_cross_owner_access() {
    let url = std::env::var("DATABASE_URL").expect("explicit disposable database URL");
    let parsed = reqwest::Url::parse(&url).unwrap();
    assert_eq!(parsed.host_str(), Some("127.0.0.1"));
    assert!(parsed.path().starts_with("/knowledgebrain_test_"));
    assert_eq!(
        std::env::var("KNOWLEDGEBRAIN_KNOWLEDGE_ENABLED").as_deref(),
        Ok("true")
    );
    let pool = PgPool::connect(&url).await.unwrap();
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    knowledge::insert_user(&pool, first, &format!("{first}@isolation.test"), None)
        .await
        .unwrap();
    knowledge::insert_user(&pool, second, &format!("{second}@isolation.test"), None)
        .await
        .unwrap();
    let secret = "isolated-owner-access-test-secret";
    let first_token = format!("Bearer {}", platform::issue_jwt(first, secret).unwrap());
    let second_token = format!("Bearer {}", platform::issue_jwt(second, secret).unwrap());
    let a = ("authorization", first_token.as_str());
    let b = ("authorization", second_token.as_str());
    let app = api::router_with(api::AppState {
        jwt_secret: secret.into(),
        bootstrap_key: String::new(),
    });
    let login_before = knowledge::find_user_by_id(&pool, second)
        .await
        .unwrap()
        .unwrap();
    let (status, rejected) = json_request(
        &app,
        "PATCH",
        "/api/v1/me",
        b,
        json!({"email": "owner-review-admin"}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{rejected}");
    assert_eq!(rejected["error"]["code"], "LOGIN_IDENTITY_IMMUTABLE");
    assert_eq!(
        knowledge::find_user_by_id(&pool, second)
            .await
            .unwrap()
            .unwrap(),
        login_before
    );
    assert_eq!(
        json_request(&app, "GET", "/api/v1/models", b, Value::Null)
            .await
            .0,
        StatusCode::FORBIDDEN
    );

    let slug = "company";
    let (status, created) = json_request(
        &app,
        "POST",
        "/api/v1/workspaces",
        a,
        json!({"name":"Owner A", "slug":slug}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let workspace: Uuid = serde_json::from_value(created["id"].clone()).unwrap();
    let (status, other) = json_request(
        &app,
        "POST",
        "/api/v1/workspaces",
        b,
        json!({"name":"Owner B", "slug":slug}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "same slug is allowed for different owners: {other}"
    );
    let other_workspace = other["id"].as_str().unwrap();
    for credential in [a, b] {
        let (status, listed) =
            json_request(&app, "GET", "/api/v1/workspaces", credential, Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        let forbidden = if credential == a {
            other_workspace.to_owned()
        } else {
            workspace.to_string()
        };
        assert!(
            listed
                .as_array()
                .unwrap()
                .iter()
                .all(|w| w["id"] != forbidden)
        );
    }
    for (method, body) in [
        ("GET", Value::Null),
        ("PATCH", json!({"name":"stolen"})),
        ("DELETE", Value::Null),
    ] {
        assert_eq!(
            json_request(
                &app,
                method,
                &format!("/api/v1/workspaces/{workspace}"),
                b,
                body
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
    }
    let (status, key) = json_request(
        &app,
        "POST",
        "/api/v1/api-keys",
        a,
        json!({"name":"owner key"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{key}");
    assert_eq!(key["owner_user_id"], first.to_string());
    assert!(key.get("scopes").is_none());
    let credential = ("x-api-key", key["token"].as_str().unwrap());
    let (status, _) = json_request(
        &app,
        "PATCH",
        &format!("/api/v1/workspaces/{workspace}"),
        credential,
        json!({"name":"changed by key"}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "keys have the same read/write rights as their owner"
    );
    assert_eq!(
        json_request(
            &app,
            "GET",
            &format!("/api/v1/workspaces/{other_workspace}"),
            credential,
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let keys = json_request(&app, "GET", "/api/v1/api-keys", b, Value::Null)
        .await
        .1;
    assert!(keys.as_array().unwrap().is_empty());
    assert_eq!(
        json_request(
            &app,
            "DELETE",
            &format!("/api/v1/api-keys/{}", key["id"].as_str().unwrap()),
            b,
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );

    let product = Uuid::new_v4();
    let version = Uuid::new_v4();
    sqlx::query("INSERT INTO knowledge_products(id,workspace_id,kind,name,slug) VALUES($1,$2,'product','private','private')")
        .bind(product).bind(workspace).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO knowledge_product_versions(id,product_id,label,status,indexing_strategy) VALUES($1,$2,'current','active','{\"vector\":false,\"keyword\":true,\"wiki\":false,\"graph\":false}')")
        .bind(version).bind(product).execute(&pool).await.unwrap();
    sqlx::query("UPDATE knowledge_products SET current_version_id=$1 WHERE id=$2")
        .bind(version)
        .bind(product)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        json_request(
            &app,
            "GET",
            &format!("/api/v1/products/{product}"),
            b,
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        json_request(
            &app,
            "POST",
            "/api/v1/search",
            b,
            json!({"query":"private", "product_id":product})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        json_request(
            &app,
            "POST",
            "/api/v1/match",
            b,
            json!({"query":"private", "workspace_id":workspace})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (status, matched) = json_request(
        &app,
        "POST",
        "/api/v1/match",
        b,
        json!({"query":"private", "scope":"product_lines"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{matched}");
    let matched: knowledge::search::MatchingResponse = serde_json::from_value(matched).unwrap();
    assert!(matched.candidates.is_empty());

    let bytes = format!("private-source-{first}").into_bytes();
    let (digest, object) = knowledge::put_bytes(&bytes).await.unwrap();
    let document = Uuid::new_v4();
    knowledge::insert_document(
        &pool,
        knowledge::NewDocument {
            id: document,
            product_version_id: version,
            title: "Private",
            file_name: "private.txt",
            file_size: bytes.len() as i64,
            file_hash: &digest,
            object_ref: &object,
        },
    )
    .await
    .unwrap();
    let path = format!("/api/v1/files?key={object}");
    let (status, downloaded) = request(&app, "GET", &path, credential, Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(downloaded, bytes);
    assert_eq!(
        request(&app, "GET", &path, b, Value::Null).await.0,
        StatusCode::NOT_FOUND
    );
    let own_product = Uuid::new_v4();
    sqlx::query("INSERT INTO knowledge_products(id,workspace_id,kind,name,slug) VALUES($1,$2,'product','B product','b-product')")
        .bind(own_product).bind(Uuid::parse_str(other_workspace).unwrap()).execute(&pool).await.unwrap();
    let (list_status, _) = json_request(
        &app,
        "GET",
        &format!("/api/v1/products/{own_product}/versions/{version}/documents"),
        b,
        Value::Null,
    )
    .await;
    let (clone_status, _) = json_request(
        &app,
        "POST",
        &format!("/api/v1/products/{own_product}/versions"),
        b,
        json!({"label":"stolen-version", "clone_from":version}),
    )
    .await;
    assert_eq!(
        (list_status, clone_status),
        (StatusCode::NOT_FOUND, StatusCode::NOT_FOUND),
        "owning the path product does not authorize another product's version"
    );
    let copied: i64 =
        sqlx::query_scalar("SELECT count(*) FROM knowledge_product_versions WHERE product_id=$1")
            .bind(own_product)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(copied, 0, "a rejected clone must not persist source data");

    // A textual mention of another owner's CAS path must not grant download rights.
    let own_version = Uuid::new_v4();
    knowledge::insert_version(
        &pool,
        own_version,
        own_product,
        "current",
        knowledge::VersionStatus::Active,
        None,
    )
    .await
    .unwrap();
    let own_document = Uuid::new_v4();
    let mention = format!("See {object}");
    let (mention_digest, mention_object) = knowledge::put_bytes(mention.as_bytes()).await.unwrap();
    knowledge::insert_document(
        &pool,
        knowledge::NewDocument {
            id: own_document,
            product_version_id: own_version,
            title: "Object mention",
            file_name: "mention.txt",
            file_size: mention.len() as i64,
            file_hash: &mention_digest,
            object_ref: &mention_object,
        },
    )
    .await
    .unwrap();
    sqlx::query("INSERT INTO knowledge_chunks(id,product_version_id,document_id,chunk_type,content) VALUES($1,$2,$3,'text',$4)")
        .bind(Uuid::new_v4()).bind(own_version).bind(own_document).bind(&mention)
        .execute(&pool).await.unwrap();
    for key in [&object, &digest] {
        let path = format!("/api/v1/products/{own_product}/versions/{own_version}/files?key={key}");
        let (status, leaked) = request(&app, "GET", &path, b, Value::Null).await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "a text mention must not authorize {key}"
        );
        assert_ne!(leaked, bytes);
        let path = format!("/api/v1/products/{product}/versions/{version}/files?key={key}");
        let (status, downloaded) = request(&app, "GET", &path, credential, Value::Null).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "the source owner keeps download access"
        );
        assert_eq!(downloaded, bytes);
    }

    // An image artifact is a real version relationship even if no text mentions its path.
    let image_bytes = [
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f,
        0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8,
        0xcf, 0xc0, 0xf0, 0x1f, 0x00, 0x05, 0x00, 0x01, 0xff, 0x89, 0x99, 0x3d, 0x1d, 0x00, 0x00,
        0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    let (image_digest, image_object) = knowledge::put_bytes(&image_bytes).await.unwrap();
    knowledge::append_document_chunks(
        &pool,
        &[knowledge::Chunk {
            id: Uuid::new_v4(),
            document_id: document,
            product_version_id: version,
            chunk_type: "image_ocr".into(),
            content: "private image".into(),
            context_header: image_object.clone(),
            start_at: 0,
            end_at: 13,
            parent_chunk_id: None,
            generated_questions: Vec::new(),
            source_locator: None,
        }],
        &[],
    )
    .await
    .unwrap();
    for key in [&image_object, &image_digest] {
        let path = format!("/api/v1/products/{product}/versions/{version}/files?key={key}");
        let (status, downloaded) = request(&app, "GET", &path, credential, Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(downloaded, image_bytes);
        assert_eq!(
            request(&app, "GET", &path, b, Value::Null).await.0,
            StatusCode::NOT_FOUND
        );
        let path = format!("/api/v1/products/{own_product}/versions/{own_version}/files?key={key}");
        assert_eq!(
            request(&app, "GET", &path, b, Value::Null).await.0,
            StatusCode::NOT_FOUND
        );
    }
    sqlx::query("UPDATE knowledge_documents SET deleted_at=now() WHERE id=$1")
        .bind(document)
        .execute(&pool)
        .await
        .unwrap();
    for key in [&object, &image_object] {
        let path = format!("/api/v1/products/{product}/versions/{version}/files?key={key}");
        assert_eq!(
            request(&app, "GET", &path, credential, Value::Null).await.0,
            StatusCode::NOT_FOUND
        );
    }

    let (status, project) = json_request(
        &app,
        "POST",
        "/api/v3/tender-projects",
        credential,
        json!({"title":"key project"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{project}");
    assert_eq!(
        json_request(
            &app,
            "GET",
            &format!(
                "/api/v3/tender-projects/{}",
                project["id"].as_str().unwrap()
            ),
            b,
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        json_request(
            &app,
            "DELETE",
            &format!("/api/v1/api-keys/{}", key["id"].as_str().unwrap()),
            a,
            Value::Null
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        json_request(&app, "GET", "/api/v1/workspaces", credential, Value::Null)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
}
