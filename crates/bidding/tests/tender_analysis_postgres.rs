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

async fn register_frozen(pool: &PgPool, project_id: Uuid, sha: &str) {
    sqlx::query(
        "INSERT INTO bid_frozen_inputs (input_sha256, project_id, document_set_id)
         VALUES ($1::kb_sha256, $2, NULL)
         ON CONFLICT (input_sha256) DO NOTHING",
    )
    .bind(sha)
    .bind(project_id)
    .execute(pool)
    .await
    .unwrap();
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
    register_frozen(&pool, project_id, &frozen).await;
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
    let frozen = "ab".repeat(32);
    register_frozen(&pool, project_id, &frozen).await;
    let published: serde_json::Value = sqlx::query_scalar(
        "SELECT kb_bid_v2_publish_outline($1,$2,$3::kb_sha256,$4,'[]'::jsonb,NULL)",
    )
    .bind(run_id)
    .bind(project_id)
    .bind(&frozen)
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
    let frozen = "ab".repeat(32);
    register_frozen(&pool, project_id, &frozen).await;
    let error =
        sqlx::query("SELECT kb_bid_v2_publish_outline($1,$2,$3::kb_sha256,$4,'[]'::jsonb,NULL)")
            .bind(run_id)
            .bind(project_id)
            .bind(&frozen)
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
    let frozen = "cd".repeat(32);
    register_frozen(&pool, project_id, &frozen).await;
    let published: serde_json::Value = sqlx::query_scalar(
        "SELECT kb_bid_v2_publish_outline($1,$2,$3::kb_sha256,$4,'[]'::jsonb,NULL)",
    )
    .bind(run_id)
    .bind(project_id)
    .bind(&frozen)
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

#[tokio::test]
#[ignore = "requires KB_TENDER_AGENT_TEST_DATABASE_URL pointing to a fresh owned test database"]
async fn analysis_tool_draft_outline_publish_matches_the_finished_draft() {
    use bidding::analysis::{FrozenInput, Source};
    use bidding::outline::chapters::AttachmentBinding;
    use bidding::outline::tools::Draft;
    use bidding::outline::{ChapterOutline, ChapterPurpose, SlotKind, TemplateContent};

    let pool = pool().await;
    let project_id = project(&pool).await;
    let run_id = Uuid::new_v4();
    let input = FrozenInput {
        schema_version: 1,
        project_id: project_id.to_string(),
        document_set_id: "set".into(),
        documents: vec![],
        document_relations: vec![],
        source_units: vec![Source {
            source_unit_revision_id: "source".into(),
            document_id: "doc".into(),
            text: "附件".into(),
            locator: serde_json::json!({"heading_path": "附件"}),
            ordinal: 0,
        }],
        structured_forms: vec![serde_json::json!({
            "form_definition_revision_id": "form-1",
            "source_unit_revision_id": "source",
            "definition": {"title": "附件一 报价表", "row_count": 1, "column_count": 1, "cells": []}
        })],
        decisions: vec![],
    };
    let mut draft = Draft {
        chapters: vec![
            ChapterOutline {
                id: "group".into(),
                parent_id: None,
                order: 0,
                title: "投标文件".into(),
                purpose: ChapterPurpose::Group,
                requirement_ids: vec![],
            },
            ChapterOutline {
                id: "letter".into(),
                parent_id: Some("group".into()),
                order: 0,
                title: "投标函".into(),
                purpose: ChapterPurpose::Response,
                requirement_ids: vec!["pack-0:0".into()],
            },
        ],
        bindings: vec![AttachmentBinding {
            form_id: "form-1".into(),
            chapter_id: "letter".into(),
        }],
        slots: vec![
            TemplateContent {
                slot_id: "letter:fixed".into(),
                chapter_id: "letter".into(),
                kind: SlotKind::FixedText,
                text: "投标函".into(),
                response_required: false,
                match_query: String::new(),
            },
            TemplateContent {
                slot_id: "letter:bidder".into(),
                chapter_id: "letter".into(),
                kind: SlotKind::BidderBlank,
                text: String::new(),
                response_required: true,
                match_query: "投标人名称".into(),
            },
        ],
        slots_submitted: true,
        finished: false,
    };
    let sha = "ab".repeat(32);
    register_frozen(&pool, project_id, &sha).await;
    let refused =
        bidding::outline::store::publish_finished(&pool, project_id, run_id, &input, &sha, &draft)
            .await
            .expect_err("unfinished draft");
    assert!(refused.contains("not finished"), "{refused}");
    let artifacts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM bid_outline_artifacts WHERE project_id=$1")
            .bind(project_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(artifacts, 0);

    draft.finished = true;
    let published =
        bidding::outline::store::publish_finished(&pool, project_id, run_id, &input, &sha, &draft)
            .await
            .unwrap();
    assert_eq!(published["replayed"], false);
    let outline_sha = published["outline_sha256"].as_str().unwrap();
    let chapters: Vec<(String, Option<String>, i32, String)> = sqlx::query_as(
        "SELECT chapter_id, parent_id, ordinal, purpose FROM bid_outline_chapters WHERE outline_sha256=$1::kb_sha256 ORDER BY ordinal, chapter_id",
    )
    .bind(outline_sha)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        chapters,
        vec![
            ("group".into(), None, 0, "group".into()),
            ("letter".into(), Some("group".into()), 0, "response".into()),
        ]
    );
    let slots: Vec<(String, String, String, bool)> = sqlx::query_as(
        "SELECT slot_id, kind, match_query, response_required FROM bid_outline_template_slots WHERE outline_sha256=$1::kb_sha256 ORDER BY slot_id",
    )
    .bind(outline_sha)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        slots,
        vec![
            (
                "letter:bidder".into(),
                "bidder_blank".into(),
                "投标人名称".into(),
                true
            ),
            (
                "letter:fixed".into(),
                "fixed_text".into(),
                String::new(),
                false
            ),
        ]
    );
    let bound: (String, String) = sqlx::query_as(
        "SELECT form_id, chapter_id FROM bid_outline_attachment_bindings WHERE outline_sha256=$1::kb_sha256",
    )
    .bind(outline_sha)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(bound, ("form-1".into(), "letter".into()));
}

#[tokio::test]
#[ignore = "requires KB_TENDER_AGENT_TEST_DATABASE_URL pointing to a fresh owned test database"]
async fn publish_rejects_an_unregistered_frozen_input() {
    let pool = pool().await;
    let project_id = project(&pool).await;
    let run_id = Uuid::new_v4();
    let bytes = serde_json::to_vec(&outline_artifact(false, &run_id.to_string())).unwrap();
    let error =
        sqlx::query("SELECT kb_bid_v2_publish_outline($1,$2,$3::kb_sha256,$4,'[]'::jsonb,NULL)")
            .bind(run_id)
            .bind(project_id)
            .bind("ee".repeat(32))
            .bind(&bytes)
            .execute(&pool)
            .await
            .expect_err("unknown frozen input");
    assert!(
        error.to_string().contains("BID_FROZEN_INPUT_UNKNOWN"),
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
