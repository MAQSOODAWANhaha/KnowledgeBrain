//! T20/T21 publication acceptance against an explicitly isolated fresh baseline.
//! Run with `--ignored`; never fall back to application database credentials.
use bidding::analysis::FrozenInput;
#[path = "support/frozen_fixture.rs"]
mod frozen_fixture;
use bidding::outline::store::OutlineLease;
use serde_json::{Value, json};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{Executor, PgPool};
use std::str::FromStr;
use uuid::Uuid;

async fn pool() -> PgPool {
    let url =
        std::env::var("KB_TENDER_AGENT_TEST_DATABASE_URL").expect("dedicated test URL required");
    let options = PgConnectOptions::from_str(&url).expect("parse dedicated test URL");
    assert_eq!(options.get_host(), "127.0.0.1");
    assert_eq!(options.get_port(), 25433);
    assert!(
        options
            .get_database()
            .is_some_and(|name| name.starts_with("knowledgebrain_test_"))
    );
    PgPoolOptions::new()
        .max_connections(6)
        .after_connect(|connection, _| {
            Box::pin(async move {
                connection.execute("SET ROLE kb_app_owner").await?;
                Ok(())
            })
        })
        .connect_with(options)
        .await
        .unwrap()
}

async fn project(pool: &PgPool) -> Uuid {
    let user = Uuid::new_v4();
    let project = Uuid::new_v4();
    sqlx::query("INSERT INTO users(id,email) VALUES ($1,$2)")
        .bind(user)
        .bind(format!("outline-{user}@example.invalid"))
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO bid_projects(id,owner_user_id,title,status) VALUES ($1,$2,'publication test','open')")
        .bind(project).bind(user).execute(pool).await.unwrap();
    project
}

// Metadata-only registration is an owner-only test seam. Runtime producers
// must publish the complete durable source/image/JSON manifest.
async fn register_frozen_fixture(
    pool: &PgPool,
    project: Uuid,
    sha: &str,
    set: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT kb_bid_v2_register_frozen_input($1,$2::kb_sha256,$3)")
        .bind(project)
        .bind(sha)
        .bind(set)
        .execute(pool)
        .await?;
    Ok(())
}

fn input(project: Uuid) -> FrozenInput {
    frozen_fixture::valid_text_input(&project.to_string(), "Response")
}

async fn claim(pool: &PgPool, project: Uuid, run: Uuid, sha: &str) -> OutlineLease {
    let receipt: Value =
        sqlx::query_scalar("SELECT kb_bid_v2_outline_claim($1,$2,$3::kb_sha256,$4,120)")
            .bind(run)
            .bind(project)
            .bind(sha)
            .bind(Uuid::new_v4())
            .fetch_one(pool)
            .await
            .unwrap();
    serde_json::from_value(receipt).unwrap()
}

fn sql_artifact(project: Uuid, sha: &str) -> Value {
    json!({"project_id":project,"frozen_input_sha256":sha,
        "chapters":[{"id":"response","parent_id":null,"order":0,"title":"Response","purpose":"response"}],
        "templates":[{"slot_id":"body","chapter_id":"response","kind":"bidder_blank","text":"","response_required":true,"match_query":"Response"}]})
}

async fn prepare_sql_publication(pool: &PgPool, run: Uuid, lease: OutlineLease, artifact: &Value) {
    let sha = platform::sha256_hex(&serde_json_canonicalizer::to_vec(artifact).unwrap());
    let checkpoint = json!({"done":true,"outline_run":{"tool_draft":{"finished":true}},
        "journal":{"publication_receipt":{"artifact_sha256":sha,"bindings":[]}}});
    sqlx::query("SELECT kb_bid_v2_outline_checkpoint($1,$2,$3,$4)")
        .bind(run)
        .bind(lease.lease_token)
        .bind(lease.lease_epoch)
        .bind(checkpoint)
        .execute(pool)
        .await
        .unwrap();
}

async fn publish_sql(
    pool: &PgPool,
    project: Uuid,
    run: Uuid,
    sha: &str,
    lease: OutlineLease,
    artifact: &Value,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_v2_publish_outline($1,$2,$3::kb_sha256,$4,$5,$6,'[]',NULL)")
        .bind(run)
        .bind(project)
        .bind(sha)
        .bind(lease.lease_token)
        .bind(lease.lease_epoch)
        .bind(serde_json_canonicalizer::to_vec(artifact).unwrap())
        .fetch_one(pool)
        .await
}

#[tokio::test]
#[ignore = "requires isolated fresh KB_TENDER_AGENT_TEST_DATABASE_URL"]
async fn sql_publication_fencing_and_durable_ownership_acceptance() {
    let pool = pool().await;
    sqlx::raw_sql(include_str!("sql/outline_response_acceptance.sql"))
        .execute(&pool)
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "requires isolated fresh KB_TENDER_AGENT_TEST_DATABASE_URL"]
async fn lost_ack_replay_requires_same_operation_fence_artifact_and_bindings() {
    let pool = pool().await;
    let project = project(&pool).await;
    let run = Uuid::new_v4();
    let sha = "ab".repeat(32);
    register_frozen_fixture(&pool, project, &sha, None)
        .await
        .unwrap();
    let lease = claim(&pool, project, run, &sha).await;
    let artifact = sql_artifact(project, &sha);
    prepare_sql_publication(&pool, run, lease, &artifact).await;
    let first = publish_sql(&pool, project, run, &sha, lease, &artifact)
        .await
        .unwrap();
    let second = publish_sql(&pool, project, run, &sha, lease, &artifact)
        .await
        .unwrap();
    assert_eq!(first["replayed"], false);
    assert_eq!(second["replayed"], true);
    assert_eq!(first["outline_sha256"], second["outline_sha256"]);
    let wrong = OutlineLease {
        lease_token: Uuid::new_v4(),
        ..lease
    };
    assert!(
        publish_sql(&pool, project, run, &sha, wrong, &artifact)
            .await
            .unwrap_err()
            .to_string()
            .contains("LEASE_LOST")
    );
    let mut changed = artifact.clone();
    changed["chapters"][0]["title"] = json!("Changed");
    assert!(
        publish_sql(&pool, project, run, &sha, lease, &changed)
            .await
            .unwrap_err()
            .to_string()
            .contains("MISMATCH")
    );
}

#[tokio::test]
#[ignore = "requires isolated fresh KB_TENDER_AGENT_TEST_DATABASE_URL"]
async fn stale_worker_blocked_behind_takeover_cannot_publish_or_change_state() {
    let pool = pool().await;
    let project = project(&pool).await;
    let run = Uuid::new_v4();
    let sha = "cd".repeat(32);
    register_frozen_fixture(&pool, project, &sha, None)
        .await
        .unwrap();
    let lease_a = claim(&pool, project, run, &sha).await;
    prepare_sql_publication(&pool, run, lease_a, &sql_artifact(project, &sha)).await;
    sqlx::query(
        "UPDATE bid_outline_runs SET lease_until=clock_timestamp()-interval '1 second' WHERE id=$1",
    )
    .bind(run)
    .execute(&pool)
    .await
    .unwrap();
    let mut takeover = pool.begin().await.unwrap();
    let receipt: Value =
        sqlx::query_scalar("SELECT kb_bid_v2_outline_claim($1,$2,$3::kb_sha256,$4,120)")
            .bind(run)
            .bind(project)
            .bind(&sha)
            .bind(Uuid::new_v4())
            .fetch_one(&mut *takeover)
            .await
            .unwrap();
    let lease_b: OutlineLease = serde_json::from_value(receipt).unwrap();
    assert!(lease_b.lease_epoch > lease_a.lease_epoch);
    let artifact = sql_artifact(project, &sha);
    let bytes = serde_json_canonicalizer::to_vec(&artifact).unwrap();
    let mut stale_connection = pool.acquire().await.unwrap();
    let stale_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *stale_connection)
        .await
        .unwrap();
    let stale_sha = sha.clone();
    let stale_bytes = bytes.clone();
    let stale = tokio::spawn(async move {
        sqlx::query_scalar::<_, Value>(
            "SELECT kb_bid_v2_publish_outline($1,$2,$3::kb_sha256,$4,$5,$6,'[]',NULL)",
        )
        .bind(run)
        .bind(project)
        .bind(stale_sha)
        .bind(lease_a.lease_token)
        .bind(lease_a.lease_epoch)
        .bind(stale_bytes)
        .fetch_one(&mut *stale_connection)
        .await
    });
    // Observe the database lock, not an arbitrary sleep ordering assumption.
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let blocked: bool = sqlx::query_scalar("SELECT cardinality(pg_blocking_pids($1)) > 0")
                .bind(stale_pid)
                .fetch_one(&pool)
                .await
                .unwrap();
            if blocked {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("stale publisher must wait behind the takeover transaction");
    let published: Value = sqlx::query_scalar(
        "SELECT kb_bid_v2_publish_outline($1,$2,$3::kb_sha256,$4,$5,$6,'[]',NULL)",
    )
    .bind(run)
    .bind(project)
    .bind(&sha)
    .bind(lease_b.lease_token)
    .bind(lease_b.lease_epoch)
    .bind(bytes)
    .fetch_one(&mut *takeover)
    .await
    .unwrap();
    let before: Value =
        sqlx::query_scalar("SELECT to_jsonb(r) FROM bid_outline_runs r WHERE id=$1")
            .bind(run)
            .fetch_one(&mut *takeover)
            .await
            .unwrap();
    takeover.commit().await.unwrap();
    let error = stale.await.unwrap().unwrap_err();
    assert!(error.to_string().contains("LEASE_LOST"), "{error}");
    let after: Value = sqlx::query_scalar("SELECT to_jsonb(r) FROM bid_outline_runs r WHERE id=$1")
        .bind(run)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(before, after);
    let hashes: Vec<String> =
        sqlx::query_scalar("SELECT sha256 FROM bid_outline_artifacts WHERE run_id=$1")
            .bind(run)
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(hashes, vec![published["outline_sha256"].as_str().unwrap()]);
}

#[tokio::test]
#[ignore = "requires isolated fresh KB_TENDER_AGENT_TEST_DATABASE_URL"]
async fn publish_finished_revalidates_draft_before_any_database_write() {
    use bidding::outline::tools::Draft;
    use bidding::outline::{
        ChapterOutline, ChapterPurpose, SlotKind, TemplateBody, TemplateContent,
    };
    let pool = pool().await;
    let project = project(&pool).await;
    let run = Uuid::new_v4();
    let input = input(project);
    let sha = bidding::outline::evidence::input_digest(&input).unwrap();
    register_frozen_fixture(&pool, project, &sha, Some(&input.document_set_id))
        .await
        .unwrap();
    let lease = claim(&pool, project, run, &sha).await;
    let mut draft = Draft {
        chapters: vec![ChapterOutline {
            id: "response".into(),
            parent_id: None,
            order: 0,
            title: "Response".into(),
            purpose: ChapterPurpose::Response,
            requirement_ids: vec![],
        }],
        slots: vec![TemplateContent {
            slot_id: "body".into(),
            chapter_id: "response".into(),
            kind: SlotKind::BidderBlank,
            content: TemplateBody::EditableBlank,
            text: String::new(),
            response_required: true,
            match_query: "Response".into(),
        }],
        slots_submitted: true,
        ..Draft::default()
    };
    let error =
        bidding::outline::store::publish_finished(&pool, project, run, lease, &input, &sha, &draft)
            .await
            .unwrap_err();
    assert!(error.contains("not finished"));
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM bid_outline_artifacts WHERE run_id=$1")
            .bind(run)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
    draft.finished = true;
    let projected = bidding::outline::project_draft(&input, &sha, &draft).unwrap();
    prepare_sql_publication(
        &pool,
        run,
        lease,
        &serde_json::to_value(projected.artifact).unwrap(),
    )
    .await;
    let result =
        bidding::outline::store::publish_finished(&pool, project, run, lease, &input, &sha, &draft)
            .await
            .unwrap();
    let stored = bidding::outline::store::load_published(&pool, project)
        .await
        .unwrap();
    assert_eq!(stored.frozen_input_sha256, sha);
    assert_eq!(stored.templates, draft.slots);
    assert_eq!(result["replayed"], false);
}

#[tokio::test]
#[ignore = "requires isolated fresh KB_TENDER_AGENT_TEST_DATABASE_URL"]
async fn frozen_identity_is_project_scoped_and_mismatch_cannot_be_reregistered() {
    let pool = pool().await;
    let first = project(&pool).await;
    let second = project(&pool).await;
    let sha = "ef".repeat(32);
    register_frozen_fixture(&pool, first, &sha, Some("first"))
        .await
        .unwrap();
    let error = register_frozen_fixture(&pool, first, &sha, Some("changed"))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("IDENTITY_MISMATCH"));
    register_frozen_fixture(&pool, second, &sha, Some("second"))
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM bid_frozen_inputs WHERE input_sha256=$1::kb_sha256",
    )
    .bind(sha)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 2);
}

#[tokio::test]
#[ignore = "requires isolated fresh KB_TENDER_AGENT_TEST_DATABASE_URL"]
async fn unknown_frozen_input_does_not_create_run_or_artifact() {
    let pool = pool().await;
    let project = project(&pool).await;
    let run = Uuid::new_v4();
    let sha = "ee".repeat(32);
    let artifact = sql_artifact(project, &sha);
    let lease = OutlineLease {
        lease_token: Uuid::new_v4(),
        lease_epoch: 1,
    };
    let error = publish_sql(&pool, project, run, &sha, lease, &artifact)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("BID_FROZEN_INPUT_UNKNOWN"));
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM bid_outline_runs WHERE id=$1)")
            .bind(run)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(!exists);
}

#[tokio::test]
#[ignore = "requires isolated fresh KB_TENDER_AGENT_TEST_DATABASE_URL"]
async fn concurrent_first_docx_publication_has_one_head_and_one_owner() {
    let pool = pool().await;
    let project = project(&pool).await;
    let run = Uuid::new_v4();
    let frozen = "aa".repeat(32);
    register_frozen_fixture(&pool, project, &frozen, None)
        .await
        .unwrap();
    let lease = claim(&pool, project, run, &frozen).await;
    prepare_sql_publication(&pool, run, lease, &sql_artifact(project, &frozen)).await;
    let receipt = publish_sql(
        &pool,
        project,
        run,
        &frozen,
        lease,
        &sql_artifact(project, &frozen),
    )
    .await
    .unwrap();
    let outline = receipt["outline_sha256"].as_str().unwrap();
    let stages = [Uuid::new_v4(), Uuid::new_v4()];
    let hashes = [
        platform::sha256_hex(stages[0].as_bytes()),
        platform::sha256_hex(stages[1].as_bytes()),
    ];
    for (stage, hash) in stages.iter().zip(&hashes) {
        platform::stage_object_upload(
            &pool,
            *stage,
            &format!("objects/{hash}"),
            hash,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            20,
            None,
        )
        .await
        .unwrap();
    }
    let publish = |index: usize| {
        let pool = pool.clone();
        let outline = outline.to_string();
        let hash = hashes[index].clone();
        let stage = stages[index];
        async move {
            sqlx::query_scalar::<_,Value>("SELECT kb_bid_v2_put_docx_version($1,$2::kb_sha256,$3::kb_object_ref,$4::kb_sha256,20,0,$5,NULL)")
                .bind(project).bind(outline).bind(format!("objects/{hash}")).bind(hash).bind(stage).fetch_one(&pool).await
        }
    };
    let (a, b) = tokio::join!(publish(0), publish(1));
    assert_ne!(a.is_ok(), b.is_ok(), "exactly one first-version CAS wins");
    let loser = if a.is_ok() { 1 } else { 0 };
    let failure = if let Err(error) = a {
        error
    } else {
        b.unwrap_err()
    };
    assert!(failure.to_string().contains("DOCX_VERSION_CAS_MISMATCH"));
    let versions: i64 =
        sqlx::query_scalar("SELECT count(*) FROM bid_docx_versions WHERE project_id=$1")
            .bind(project)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(versions, 1);
    let loser_has_stage: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM object_upload_staging WHERE id=$1)")
            .bind(stages[loser])
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        loser_has_stage,
        "failed CAS keeps temporary owner for cleanup"
    );
    let loser_owners:i64=sqlx::query_scalar("SELECT count(*) FROM object_owner_references WHERE object_ref=$1 AND owner_kind='bid_docx_version'").bind(format!("objects/{}",hashes[loser])).fetch_one(&pool).await.unwrap();
    assert_eq!(
        loser_owners, 0,
        "failed CAS cannot leak a durable version owner"
    );
}

#[tokio::test]
#[ignore = "requires isolated fresh KB_TENDER_AGENT_TEST_DATABASE_URL"]
async fn worker_rejects_loose_frozen_digest_without_durable_snapshot_ownership() {
    let pool = pool().await;
    let project = project(&pool).await;
    let input = input(project);
    let sha = bidding::outline::evidence::input_digest(&input).unwrap();
    register_frozen_fixture(&pool, project, &sha, Some(&input.document_set_id))
        .await
        .unwrap();
    let error = bidding::worker::verify_established_frozen_input(&pool, project, &input, &sha)
        .await
        .unwrap_err();
    assert!(error.contains("FROZEN_INPUT_NOT_PUBLISHED"), "{error}");
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM bid_outline_runs WHERE project_id=$1")
            .bind(project)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
#[ignore = "requires isolated fresh KB_TENDER_AGENT_TEST_DATABASE_URL"]
async fn distinct_valid_runs_can_share_content_without_borrowing_operation_replay() {
    let pool = pool().await;
    let project = project(&pool).await;
    let sha = "be".repeat(32);
    register_frozen_fixture(&pool, project, &sha, None)
        .await
        .unwrap();
    let artifact = sql_artifact(project, &sha);
    let first_run = Uuid::new_v4();
    let first_lease = claim(&pool, project, first_run, &sha).await;
    prepare_sql_publication(&pool, first_run, first_lease, &artifact).await;
    let first = publish_sql(&pool, project, first_run, &sha, first_lease, &artifact)
        .await
        .unwrap();
    let second_run = Uuid::new_v4();
    let second_lease = claim(&pool, project, second_run, &sha).await;
    prepare_sql_publication(&pool, second_run, second_lease, &artifact).await;
    assert!(
        publish_sql(&pool, project, second_run, &sha, first_lease, &artifact)
            .await
            .unwrap_err()
            .to_string()
            .contains("LEASE_LOST")
    );
    let second = publish_sql(&pool, project, second_run, &sha, second_lease, &artifact)
        .await
        .unwrap();
    assert_eq!(first["outline_sha256"], second["outline_sha256"]);
    assert_eq!(
        second["replayed"], false,
        "new valid run must complete its own operation"
    );
    assert_eq!(
        publish_sql(&pool, project, second_run, &sha, second_lease, &artifact)
            .await
            .unwrap()["replayed"],
        true
    );
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM bid_outline_artifacts WHERE project_id=$1")
            .bind(project)
            .fetch_one(&pool)
            .await
            .unwrap();
    let completed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM bid_outline_runs WHERE project_id=$1 AND status='published'",
    )
    .bind(project)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
    assert_eq!(completed, 2);
}
