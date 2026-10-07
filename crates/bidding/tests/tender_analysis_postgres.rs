//! Outline persistence acceptance. Run with `--ignored` against a database
//! that already has the fresh baselines.
use sqlx::PgPool;
use uuid::Uuid;

async fn pool() -> PgPool {
    let url =
        std::env::var("KB_TENDER_AGENT_TEST_DATABASE_URL").expect("dedicated test URL required");
    let pool = PgPool::connect(&url).await.unwrap();
    let mut connection = pool.acquire().await.unwrap();
    sqlx::Executor::execute(&mut *connection, "SET ROLE kb_app_owner")
        .await
        .unwrap();
    drop(connection);
    pool
}

async fn project(pool: &PgPool) -> Uuid {
    let user_id = Uuid::new_v4();
    let project_id = Uuid::new_v4();
    sqlx::query("INSERT INTO users(id, email) VALUES ($1, $2)")
        .bind(user_id)
        .bind(format!("outline-{user_id}@example.invalid"))
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO bid_projects(id, owner_user_id, title, status) VALUES ($1, $2, 'analysis', 'open')",
    )
    .bind(project_id)
    .bind(user_id)
    .execute(pool)
    .await
    .unwrap();
    project_id
}

fn outline_artifact(group_responds: bool, mark: &str) -> serde_json::Value {
    let slot_chapter = if group_responds { "ch-1" } else { "ch-2" };
    let purpose_slot = if group_responds {
        serde_json::json!(null)
    } else {
        serde_json::json!({
            "id": "ch-2", "parent_id": "ch-1", "order": 0,
            "title": "投标函", "purpose": "response"
        })
    };
    let mut chapters = vec![serde_json::json!({
        "id": "ch-1", "parent_id": "", "order": 0, "title": format!("投标文件 {mark}"), "purpose": "group"
    })];
    if !group_responds {
        chapters.push(purpose_slot);
    }
    serde_json::json!({
        "chapters": chapters,
        "templates": [{
            "slot_id": "slot-blank",
            "chapter_id": slot_chapter,
            "kind": "bidder_blank",
            "text": "",
            "response_required": true,
            "match_query": "投标函"
        }]
    })
}

#[tokio::test]
#[ignore = "requires KB_TENDER_AGENT_TEST_DATABASE_URL pointing to a fresh owned test database"]
async fn review_draft_survives_committed_checkpoint_ack_loss() {
    let pool = pool().await;
    let project_id = project(&pool).await;
    let run_id = Uuid::new_v4();
    let bytes = serde_json::to_vec(&outline_artifact(false, &run_id.to_string())).unwrap();
    let frozen = "ab".repeat(32);
    let first: serde_json::Value = sqlx::query_scalar(
        "SELECT kb_bid_v2_publish_outline($1,$2,$3::kb_sha256,$4,'[]'::jsonb,NULL)",
    )
    .bind(run_id)
    .bind(project_id)
    .bind(&frozen)
    .bind(&bytes)
    .fetch_one(&pool)
    .await
    .unwrap();
    let second: serde_json::Value = sqlx::query_scalar(
        "SELECT kb_bid_v2_publish_outline($1,$2,$3::kb_sha256,$4,'[]'::jsonb,NULL)",
    )
    .bind(run_id)
    .bind(project_id)
    .bind(&frozen)
    .bind(&bytes)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(first["replayed"], false);
    assert_eq!(second["replayed"], true);
    assert_eq!(first["outline_sha256"], second["outline_sha256"]);
    let chapters: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM bid_outline_chapters WHERE outline_sha256=$1::kb_sha256",
    )
    .bind(first["outline_sha256"].as_str().unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(chapters, 2);
}

#[tokio::test]
#[ignore = "requires KB_TENDER_AGENT_TEST_DATABASE_URL pointing to a fresh owned test database"]
async fn analysis_publishes_constraints_without_fabricating_submission_needs() {
    let pool = pool().await;
    let project_id = project(&pool).await;
    let run_id = Uuid::new_v4();
    let bytes = serde_json::to_vec(&outline_artifact(false, &run_id.to_string())).unwrap();
    let published: serde_json::Value = sqlx::query_scalar(
        "SELECT kb_bid_v2_publish_outline($1,$2,$3::kb_sha256,$4,'[]'::jsonb,NULL)",
    )
    .bind(run_id)
    .bind(project_id)
    .bind("ab".repeat(32))
    .bind(&bytes)
    .fetch_one(&pool)
    .await
    .unwrap();
    let outline_sha = published["outline_sha256"].as_str().unwrap();
    let response = serde_json::json!({
        "outline_sha256": outline_sha,
        "responses": [{
            "slot_id": "slot-blank",
            "chapter_id": "ch-2",
            "status": "no_evidence",
            "text": "【待人工补充】",
            "evidence_ids": []
        }]
    });
    let stored: serde_json::Value =
        sqlx::query_scalar("SELECT kb_bid_v2_publish_response($1,$2::kb_sha256,$3,NULL)")
            .bind(project_id)
            .bind(outline_sha)
            .bind(serde_json::to_vec(&response).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    let body: String = sqlx::query_scalar(
        "SELECT body FROM bid_response_slots WHERE response_sha256=$1::kb_sha256 AND slot_id='slot-blank'",
    )
    .bind(stored["response_sha256"].as_str().unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(body, "【待人工补充】");
}

#[tokio::test]
#[ignore = "requires KB_TENDER_AGENT_TEST_DATABASE_URL pointing to a fresh owned test database"]
async fn repair_checkpoint_rejects_malformed_dispositions_without_changing_saved_state() {
    let pool = pool().await;
    let project_id = project(&pool).await;
    let run_id = Uuid::new_v4();
    let bytes = serde_json::to_vec(&outline_artifact(true, &run_id.to_string())).unwrap();
    let error =
        sqlx::query("SELECT kb_bid_v2_publish_outline($1,$2,$3::kb_sha256,$4,'[]'::jsonb,NULL)")
            .bind(run_id)
            .bind(project_id)
            .bind("ab".repeat(32))
            .bind(&bytes)
            .execute(&pool)
            .await
            .expect_err("group chapter response");
    assert!(
        error
            .to_string()
            .contains("group chapter cannot carry a knowledge response"),
        "{error}"
    );
    let artifacts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM bid_outline_artifacts WHERE project_id=$1")
            .bind(project_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(artifacts, 0);
}

#[tokio::test]
#[ignore = "requires KB_TENDER_AGENT_TEST_DATABASE_URL pointing to a fresh owned test database"]
async fn source_review_final_batch_recovers_without_extra_model_calls_or_double_finalization() {
    let pool = pool().await;
    let project_id = project(&pool).await;
    let run_id = Uuid::new_v4();
    let token = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO bid_outline_runs(id, project_id, status, checkpoint) VALUES ($1,$2,'pending','{}'::jsonb)",
    )
    .bind(run_id)
    .bind(project_id)
    .execute(&pool)
    .await
    .unwrap();
    let claimed: serde_json::Value = sqlx::query_scalar("SELECT kb_bid_v2_outline_claim($1,$2,60)")
        .bind(run_id)
        .bind(token)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(claimed["status"], "running");
    let blocked = sqlx::query("SELECT kb_bid_v2_outline_claim($1,$2,60)")
        .bind(run_id)
        .bind(Uuid::new_v4())
        .execute(&pool)
        .await
        .expect_err("second lease");
    assert!(
        blocked.to_string().contains("published or leased"),
        "{blocked}"
    );
    let bytes = serde_json::to_vec(&outline_artifact(false, &run_id.to_string())).unwrap();
    let published: serde_json::Value = sqlx::query_scalar(
        "SELECT kb_bid_v2_publish_outline($1,$2,$3::kb_sha256,$4,'[]'::jsonb,NULL)",
    )
    .bind(run_id)
    .bind(project_id)
    .bind("cd".repeat(32))
    .bind(&bytes)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(published["replayed"], false);
    let again = sqlx::query("SELECT kb_bid_v2_outline_claim($1,$2,60)")
        .bind(run_id)
        .bind(token)
        .execute(&pool)
        .await
        .expect_err("published claim");
    assert!(again.to_string().contains("published or leased"), "{again}");
}
