//! Authenticated saved-review replay must reach Python CAS without rereading Word.
use super::*;
use tower::ServiceExt;

async fn send_review(
    app: &Router,
    project: Uuid,
    version: Uuid,
    token: &str,
    key: &str,
    body: &Value,
) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/v3/tender-projects/{project}/docx/versions/{version}/review"
                ))
                .header("authorization", format!("Bearer {token}"))
                .header("idempotency-key", key)
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

fn execution_id(project: Uuid, key: &str) -> String {
    let request = json!({"key":key,"project_id":project});
    format!(
        "web-saved-review-{}",
        platform::sha256_hex(&serde_json::to_vec(&request).unwrap())
    )
}

#[test]
#[ignore = "requires isolated initialized PostgreSQL, authenticated Python bridge fixture and task-owned OBJECT_DIR"]
fn saved_review_http_replay_uses_same_job_without_saved_blob() {
    crate::tender_routes::test_runtime().block_on(async {
        let selected = std::env::var("KB_TENDER_WEB_TEST_DATABASE_URL")
            .expect("explicit isolated test database required");
        assert_eq!(std::env::var("DATABASE_URL").unwrap(), selected);
        let url = Url::parse(&selected).unwrap();
        assert_eq!(url.host_str(), Some("127.0.0.1"));
        assert!(url.path().contains("test") || url.path() == "/kb_complete");
        assert!(std::env::var("DOCREADER_ADDR").unwrap().starts_with("127.0.0.1:"));
        assert!(std::env::var("OBJECT_DIR").unwrap().contains("/kb-tender-evidence/"));
        assert!(!platform::s3_configured(), "task-owned local object fixture only");
        let pool = platform::connect().await.unwrap();
        let owner = Uuid::new_v4();
        let outsider = Uuid::new_v4();
        let project = Uuid::new_v4();
        let other_project = Uuid::new_v4();
        let version = Uuid::new_v4();
        let other_version = Uuid::new_v4();
        let run = format!("saved-replay-run-{}", Uuid::new_v4());
        let key = "same-review/\"quoted\"\\request";
        let execution = execution_id(project, key);
        let foreign_execution = execution_id(other_project, key);
        let hash = platform::sha256_hex(Uuid::new_v4().as_bytes());
        let object = platform::object_ref(&hash);
        // Register immutable version metadata without creating its physical blob.
        // An initial/new request must still fail on that unavailable input.
        assert!(platform::read_blob(&hash).is_err());
        for user in [owner, outsider] {
            sqlx::query("INSERT INTO platform_users(id,email) VALUES($1,$2)")
                .bind(user).bind(format!("saved-review-{user}@example.invalid"))
                .execute(&pool).await.unwrap();
        }
        for id in [project, other_project] {
            sqlx::query("INSERT INTO tender_projects(id,owner_user_id,title,status) VALUES($1,$2,'saved review replay','open')")
                .bind(id).bind(owner).execute(&pool).await.unwrap();
        }
        sqlx::query("INSERT INTO tender_analysis_runs(run_id,request_key) VALUES($1,$1)")
            .bind(&run).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO tender_project_analyses(run_id,project_id,request_key) VALUES($1,$2,$1)")
            .bind(&run).bind(project).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO platform_objects(object_ref,content_sha256,media_type,byte_length,state) VALUES($1,$2,$3,1,'available')")
            .bind(&object).bind(&hash).bind(DOCX).execute(&pool).await.unwrap();
        for (id, parent) in [(version, project), (other_version, other_project)] {
            sqlx::query("INSERT INTO tender_draft_versions(id,project_id,execution_id,round_id,revision,source,object_ref,content_sha256,byte_length,outline,created_by_user_id) VALUES($1,$2,'synthetic-original-document',$3,1,'manual',$4,$5,1,'{}',$6)")
                .bind(id).bind(parent).bind(Uuid::new_v4()).bind(&object).bind(&hash).bind(owner)
                .execute(&pool).await.unwrap();
        }
        let request = json!({"version_id":version,"docx_sha256":hash,"reason":"核对人工修改的工期"});
        let payload = json!({"operation":"saved_review","owner_user_id":owner,"request":request,
            "identity":{"run_id":run,"source_version":1,"domain_revision":1},
            "snapshot":{"run_id":run,"source_version":1,"domain_revision":1,"frozen_ref":null}});
        for id in [&execution, &foreign_execution] {
            sqlx::query("INSERT INTO tender_authoring_jobs(execution_id,project_id,run_id,payload) VALUES($1,$2,$3,$4)")
                .bind(id).bind(project).bind(&run).bind(&payload).execute(&pool).await.unwrap();
        }
        let state = AppState { jwt_secret: "saved-review-isolated-http-secret".into(), bootstrap_key: String::new() };
        let token = platform::issue_jwt(owner, &state.jwt_secret).unwrap();
        let other_token = platform::issue_jwt(outsider, &state.jwt_secret).unwrap();
        let app = crate::router_with(state);
        let body = json!({"docx_sha256":hash,"reason":"核对人工修改的工期"});
        let replay = send_review(&app, project, version, &token, key, &body).await;
        let changed = send_review(&app, project, version, &token, key,
            &json!({"docx_sha256":hash,"reason":"更改了同键请求的复核范围"})).await;
        let new_request = send_review(&app, project, version, &token, "new-review", &body).await;
        let wrong_owner = send_review(&app, project, version, &other_token, key, &body).await;
        let foreign_mapping = send_review(&app, other_project, other_version, &token, key, &body).await;
        sqlx::query("UPDATE tender_projects SET status='ended',ended_at=now() WHERE id=$1")
            .bind(project).execute(&pool).await.unwrap();
        let ended_replay = send_review(&app, project, version, &token, key, &body).await;
        let ended_changed = send_review(&app, project, version, &token, key,
            &json!({"docx_sha256":hash,"reason":"更改已结束项目的原复核请求"})).await;
        let ended_new = send_review(&app, project, version, &token, "ended-new-review", &body).await;
        let ended_foreign = send_review(&app, project, version, &other_token, key, &body).await;
        let models: i64 = sqlx::query_scalar("SELECT count(*) FROM tender_authoring_model_requests WHERE execution_id IN ($1,$2)")
            .bind(&execution).bind(&foreign_execution).fetch_one(&pool).await.unwrap();
        // Remove only this test's synthetic identities before assertions.
        sqlx::query("DELETE FROM tender_draft_versions WHERE project_id IN ($1,$2)")
            .bind(project).bind(other_project).execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM tender_authoring_jobs WHERE project_id=$1")
            .bind(project).execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM tender_project_analyses WHERE run_id=$1")
            .bind(&run).execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM tender_analysis_runs WHERE run_id=$1")
            .bind(&run).execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM tender_projects WHERE id IN ($1,$2)")
            .bind(project).bind(other_project).execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM platform_users WHERE id IN ($1,$2)")
            .bind(owner).bind(outsider).execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM platform_objects WHERE object_ref=$1")
            .bind(&object).execute(&pool).await.unwrap();
        assert_eq!(replay.0, StatusCode::ACCEPTED, "{replay:?}");
        assert_eq!(replay.1["review"]["execution_id"], execution);
        assert_eq!(replay.1["review"]["version_id"], version.to_string());
        assert_eq!(replay.1["review"]["docx_sha256"], hash);
        assert_eq!(changed.0, StatusCode::CONFLICT, "{changed:?}");
        assert_eq!(new_request.0, StatusCode::SERVICE_UNAVAILABLE, "{new_request:?}");
        assert_eq!(new_request.1["error"]["code"], "ONLYOFFICE_UNAVAILABLE");
        assert_eq!(wrong_owner.0, StatusCode::NOT_FOUND, "{wrong_owner:?}");
        assert_eq!(foreign_mapping.0, StatusCode::SERVICE_UNAVAILABLE, "{foreign_mapping:?}");
        assert_eq!(foreign_mapping.1["error"]["code"], "ONLYOFFICE_UNAVAILABLE");
        assert_eq!(ended_replay.0, StatusCode::ACCEPTED, "{ended_replay:?}");
        assert_eq!(ended_replay.1["review"]["execution_id"], execution);
        assert_eq!(ended_replay.1["review"]["version_id"], version.to_string());
        assert_eq!(ended_changed.0, StatusCode::CONFLICT, "{ended_changed:?}");
        assert_eq!(ended_new.0, StatusCode::CONFLICT, "{ended_new:?}");
        assert_eq!(ended_new.1["error"]["code"], "PROJECT_ENDED");
        assert_eq!(ended_foreign.0, StatusCode::NOT_FOUND, "{ended_foreign:?}");
        assert_eq!(models, 0);
    });
}
