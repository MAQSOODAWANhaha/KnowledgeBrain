//! Ended projects retain immutable reads and previously accepted save receipts.
use super::*;
use tower::ServiceExt;

struct Fixture {
    pool: PgPool,
    cap: Capability,
    sha: String,
    app: Router,
    token: String,
}

async fn fixture(editing: bool) -> Fixture {
    let url = std::env::var("KB_TENDER_WEB_TEST_DATABASE_URL").unwrap();
    assert!(url.contains("knowledgebrain_test_tender_web"));
    assert_eq!(std::env::var("DATABASE_URL").unwrap(), url);
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .unwrap();
    let cap = Capability {
        aud: "tender-word-callback".into(),
        project: Uuid::new_v4(),
        key: Uuid::new_v4(),
        version: Uuid::new_v4(),
        owner: Uuid::new_v4(),
        mode: EditorMode::Edit,
    };
    sqlx::query("INSERT INTO platform_users(id,email) VALUES($1,$2)")
        .bind(cap.owner)
        .bind(format!("word-lifecycle-{}@example.invalid", cap.owner))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO tender_projects(id,owner_user_id,title,status) VALUES($1,$2,'synthetic lifecycle','open')")
        .bind(cap.project).bind(cap.owner).execute(&pool).await.unwrap();
    let bytes = format!("synthetic-word-{}", cap.version).into_bytes();
    let sha = platform::sha256_hex(&bytes);
    let staging = Uuid::new_v4();
    platform::stage_object_upload(
        &pool,
        staging,
        &platform::object_ref(&sha),
        &sha,
        DOCX,
        bytes.len() as i64,
        Some(cap.owner),
    )
    .await
    .unwrap();
    let blob = Blob {
        sha: sha.clone(),
        len: bytes.len() as i64,
        staging,
    };
    let mut tx = pool.begin().await.unwrap();
    publish_blob(&mut tx, &blob, cap.version, cap.owner)
        .await
        .unwrap();
    sqlx::query("INSERT INTO tender_draft_versions(id,project_id,execution_id,round_id,revision,source,object_ref,content_sha256,byte_length,outline,created_by_user_id) VALUES($1,$2,'lifecycle-test',$3,1,'generated',$4,$5::kb_sha256,$6,'{}',$7)")
        .bind(cap.version).bind(cap.project).bind(Uuid::new_v4()).bind(platform::object_ref(&sha))
        .bind(&sha).bind(blob.len).bind(cap.owner).execute(&mut *tx).await.unwrap();
    if editing {
        sqlx::query("INSERT INTO tender_editor_sessions(id,project_id,base_version_id,owner_user_id,mode,request_key,request_sha256) VALUES($1,$2,$3,$4,'edit','lifecycle-open',$5::kb_sha256)")
            .bind(cap.key).bind(cap.project).bind(cap.version).bind(cap.owner).bind(&sha).execute(&mut *tx).await.unwrap();
    }
    sqlx::query(
        "INSERT INTO tender_draft_state(project_id,version_id,session_id) VALUES($1,$2,$3)",
    )
    .bind(cap.project)
    .bind(cap.version)
    .bind(editing.then_some(cap.key))
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let state = AppState {
        jwt_secret: "synthetic-lifecycle-only".into(),
        bootstrap_key: String::new(),
    };
    let token = platform::issue_jwt(cap.owner, &state.jwt_secret).unwrap();
    Fixture {
        pool,
        cap,
        sha,
        app: crate::router_with(state),
        token,
    }
}

async fn request(f: &Fixture, method: &str, suffix: &str, body: Value) -> (StatusCode, Value) {
    let response = f
        .app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method(method)
                .uri(format!("/api/v3/tender-projects/{}{suffix}", f.cap.project))
                .header("authorization", format!("Bearer {}", f.token))
                .header("idempotency-key", "lifecycle-http")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[test]
#[ignore = "requires the explicitly provisioned isolated tender web database"]
fn project_cannot_end_while_a_word_editor_or_save_is_active() {
    crate::tender_routes::test_runtime().block_on(async {
        let f = fixture(true).await;
        let (status, body) = request(&f, "PATCH", "", json!({"status":"ended"})).await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(body["error"]["code"], "PROJECT_HAS_ACTIVE_EDITOR");
        let (status, current) = request(&f, "GET", "", json!({})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(current["status"], "open");
        let (status, _) = request(
            &f,
            "POST",
            &format!("/docx/editor/{}/close", f.cap.key),
            json!({}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, body) = request(&f, "PATCH", "", json!({"status":"ended"})).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["status"], "ended");
        let (status, _) = request(&f, "GET", "/docx/current", json!({})).await;
        assert_eq!(status, StatusCode::OK);
    });
}

#[test]
#[ignore = "requires the explicitly provisioned isolated tender web database"]
fn ended_project_rejects_new_word_edits_and_current_selection_before_office_io() {
    crate::tender_routes::test_runtime().block_on(async {
        let f = fixture(false).await;
        sqlx::query("UPDATE tender_projects SET status='ended',ended_at=now() WHERE id=$1")
            .bind(f.cap.project)
            .execute(&f.pool)
            .await
            .unwrap();
        let identity = json!({"version_id":f.cap.version,"docx_sha256":f.sha});
        let (status, body) = request(&f, "POST", "/docx/current", identity.clone()).await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(body["error"]["code"], "PROJECT_ENDED");
        let mut edit = identity;
        edit["mode"] = json!("edit");
        let (status, body) = request(&f, "POST", "/docx/editor", edit).await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(body["error"]["code"], "PROJECT_ENDED");
        let sessions: i64 =
            sqlx::query_scalar("SELECT count(*) FROM tender_editor_sessions WHERE project_id=$1")
                .bind(f.cap.project)
                .fetch_one(&f.pool)
                .await
                .unwrap();
        assert_eq!(sessions, 0);
        let (status, original) = request(&f, "GET", "/docx/current", json!({})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(original["version_id"], f.cap.version.to_string());
    });
}

#[test]
#[ignore = "requires the explicitly provisioned isolated tender web database"]
fn ended_project_retains_exact_pending_and_saved_word_receipts_but_rejects_new_saves() {
    crate::tender_routes::test_runtime().block_on(async {
        let f = fixture(true).await;
        let input = Save {
            editor_key: f.cap.key,
            expected: Identity {
                version_id: f.cap.version,
                docx_sha256: f.sha.clone(),
            },
        };
        let first = prepare_save(&f.pool, f.cap.project, f.cap.owner, "accepted-save", &input)
            .await
            .unwrap();
        sqlx::query("UPDATE tender_projects SET status='ended',ended_at=now() WHERE id=$1")
            .bind(f.cap.project)
            .execute(&f.pool)
            .await
            .unwrap();
        let replay = prepare_save(&f.pool, f.cap.project, f.cap.owner, "accepted-save", &input)
            .await
            .unwrap();
        assert_eq!(replay.id, first.id);
        assert!(replay.replay);
        let error =
            match prepare_save(&f.pool, f.cap.project, f.cap.owner, "new-save", &input).await {
                Ok(_) => panic!("ended project accepted a new save"),
                Err(error) => error,
            };
        assert_eq!(error.1.0.error.code, "PROJECT_ENDED");
        sqlx::query("UPDATE tender_draft_state SET pending_save_id=NULL WHERE project_id=$1")
            .bind(f.cap.project)
            .execute(&f.pool)
            .await
            .unwrap();
        sqlx::query("UPDATE tender_draft_saves SET state='saved',result_version_id=$2 WHERE id=$1")
            .bind(first.id)
            .bind(f.cap.version)
            .execute(&f.pool)
            .await
            .unwrap();
        let saved = prepare_save(&f.pool, f.cap.project, f.cap.owner, "accepted-save", &input)
            .await
            .unwrap();
        assert_eq!(saved.id, first.id);
        assert!(!saved.dispatch);
        let changed = Save {
            editor_key: Uuid::new_v4(),
            expected: input.expected,
        };
        assert!(
            prepare_save(
                &f.pool,
                f.cap.project,
                f.cap.owner,
                "accepted-save",
                &changed
            )
            .await
            .is_err()
        );
    });
}
