use crate::bidding::*;
use crate::helpers::*;
use crate::knowledge::*;
use crate::runtime::*;
use async_trait::async_trait;
use knowledge::{create_workspace_with_library, insert_document, insert_user};
use platform::{KnowledgeSemanticIndexV2Job, VersionCloneJob, WikiIngestJob};
use sqlx::{PgPool, Row};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[tokio::test]
async fn process_group_signal_failure_falls_back_to_direct_child_kill() {
    let mut child = tokio::process::Command::new("sleep")
        .arg("60")
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    kill_helper_group_and_reap_child(
        &mut child,
        "fallback test child",
        tokio::time::Instant::now() + std::time::Duration::from_secs(2),
    )
    .await
    .unwrap();
    assert!(child.try_wait().unwrap().is_some());
}

#[tokio::test]
async fn real_postgres_non_agent_and_content_timeout_cancel_late_writes() {
    let _guard = db_lock().await;
    let Ok(pool) = connect().await else {
        eprintln!("skip: isolated PostgreSQL test database is down");
        return;
    };
    sqlx::raw_sql(
        "DROP TABLE IF EXISTS kb_worker_no_late_write_probe;
             CREATE TABLE kb_worker_no_late_write_probe (
                singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
                non_agent_written boolean NOT NULL DEFAULT false,
                content_written boolean NOT NULL DEFAULT false
             );
             INSERT INTO kb_worker_no_late_write_probe DEFAULT VALUES",
    )
    .execute(&pool)
    .await
    .unwrap();

    let mut lock_connection = pool.acquire().await.unwrap();
    let advisory_key = 7_401_002_i64;
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(advisory_key)
        .execute(&mut *lock_connection)
        .await
        .unwrap();

    let generic_pool = pool.clone();
    let generic_cancel = CancellationToken::new();
    let now = tokio::time::Instant::now();
    let generic_run = run_owned_handler(
        async move {
            sqlx::query("SELECT pg_advisory_xact_lock($1)")
                .bind(advisory_key)
                .execute(&generic_pool)
                .await
                .map_err(|error| JobErr(error.to_string()))?;
            sqlx::query("UPDATE kb_worker_no_late_write_probe SET non_agent_written=true")
                .execute(&generic_pool)
                .await
                .map_err(|error| JobErr(error.to_string()))?;
            Ok(())
        },
        HandlerDeadline {
            hard: now + std::time::Duration::from_millis(50),
            cleanup: now + std::time::Duration::from_secs(1),
        },
        CancellationToken::new(),
        generic_cancel,
        None,
        |_| false,
    )
    .await;
    assert!(matches!(
        generic_run.completion,
        OwnedHandlerCompletion::TimedOut
    ));

    let content_pool = pool.clone();
    let pipeline_cancel = CancellationToken::new();
    let pipeline_stop = pipeline_cancel.clone();
    let mut pipeline = tokio::spawn(async move {
        tokio::select! {
            biased;
            () = pipeline_stop.cancelled() => Err(
                bidding::agent_error::AgentError::new("INTERNAL", "cancelled")),
            result = async {
                sqlx::query("SELECT pg_advisory_xact_lock($1)")
                    .bind(advisory_key)
                    .execute(&content_pool)
                    .await
                    .map_err(|error| bidding::agent_error::AgentError::new(
                        "INTERNAL", error.to_string()))?;
                sqlx::query(
                    "UPDATE kb_worker_no_late_write_probe SET content_written=true",
                )
                .execute(&content_pool)
                .await
                .map_err(|error| bidding::agent_error::AgentError::new(
                    "INTERNAL", error.to_string()))?;
                Ok(())
            } => result,
        }
    });
    let heartbeat_cancel = CancellationToken::new();
    let heartbeat_stop = heartbeat_cancel.clone();
    let mut heartbeat = tokio::spawn(async move {
        heartbeat_stop.cancelled().await;
    });
    let (_lease_sender, mut lease_receiver) = tokio::sync::oneshot::channel();
    let now = tokio::time::Instant::now();
    let content_run = await_content_owned_completion(
        &mut pipeline,
        &mut heartbeat,
        &mut lease_receiver,
        &pipeline_cancel,
        &heartbeat_cancel,
        &CancellationToken::new(),
        HandlerDeadline {
            hard: now + std::time::Duration::from_millis(50),
            cleanup: now + std::time::Duration::from_secs(1),
        },
    )
    .await;
    assert!(matches!(
        content_run.completion,
        ContentOwnedCompletion::TimedOut
    ));

    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(advisory_key)
        .execute(&mut *lock_connection)
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let flags: (bool, bool) = sqlx::query_as(
        "SELECT non_agent_written, content_written
             FROM kb_worker_no_late_write_probe",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(flags, (false, false));
    sqlx::query("DROP TABLE kb_worker_no_late_write_probe")
        .execute(&pool)
        .await
        .unwrap();
}

async fn single_connection_pool(
    database_url: &str,
) -> (PgPool, sqlx::pool::PoolConnection<sqlx::Postgres>) {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(std::time::Duration::from_secs(1))
        .connect(database_url)
        .await
        .unwrap();
    let connection = pool.acquire().await.unwrap();
    (pool, connection)
}

async fn assert_pending_request(pool: &PgPool, request_id: Uuid) {
    let state: (String, Option<String>) = sqlx::query_as(
        "SELECT status,error_code FROM bid_async_request_snapshot_artifacts WHERE id=$1",
    )
    .bind(request_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(state, ("pending".into(), None));
}

#[tokio::test]
async fn blocked_export_tender_and_content_effects_are_bounded_without_late_writes() {
    let _guard = db_lock().await;
    let Some(database_url) = std::env::var("KNOWLEDGEBRAIN_TEST_DATABASE_URL").ok() else {
        eprintln!("skip: isolated PostgreSQL test database is down");
        return;
    };
    if !database_url.contains("127.0.0.1:25433/knowledgebrain_test_") {
        panic!("blocked handler tests require 127.0.0.1:25433/knowledgebrain_test_*");
    }
    let Ok(pool) = connect().await else {
        eprintln!("skip: isolated PostgreSQL test database is down");
        return;
    };
    reset_test_schema(&pool).await;
    install_phase_fixture(&pool).await;
    let export = create_export_terminal_test_request(&pool).await;
    let tender = create_tender_terminal_test_request(&pool).await;
    let content = create_content_terminal_test_request(&pool).await;
    let content_owner = match bidding::bid_authoring_v2::claim_content_agent_run_v1(&pool, &content)
        .await
        .unwrap()
    {
        bidding::bid_authoring_v2::ContentRunClaim::Claimed(owner) => owner,
        other => panic!("expected claimed Content AgentRun, got {other:?}"),
    };

    let (blocked, held) = single_connection_pool(&database_url).await;
    let started = tokio::time::Instant::now();
    let export_result = terminalize_non_agent_failure_until(
        &blocked,
        &export,
        NonAgentTerminalFailure::SubmissionExport("RENDERER_FAILED"),
        started + std::time::Duration::from_millis(100),
        "blocked export failure",
    )
    .await;
    assert!(export_result.unwrap_err().0.contains("persistence reserve"));
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
    drop(held);
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_pending_request(&pool, export.request_artifact_id).await;

    let (blocked, held) = single_connection_pool(&database_url).await;
    let started = tokio::time::Instant::now();
    let tender_result = terminalize_non_agent_failure_until(
        &blocked,
        &tender,
        NonAgentTerminalFailure::TenderDocument("AGENT_OUTPUT_INVALID"),
        started + std::time::Duration::from_millis(100),
        "blocked tender failure",
    )
    .await;
    assert!(tender_result.unwrap_err().0.contains("persistence reserve"));
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
    drop(held);
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_pending_request(&pool, tender.request_artifact_id).await;

    let (blocked, held) = single_connection_pool(&database_url).await;
    let started = tokio::time::Instant::now();
    let content_result = yield_content_retry_until(
        &blocked,
        &content,
        &content_owner,
        "blocked transient yield",
        started + std::time::Duration::from_millis(100),
    )
    .await;
    assert!(
        content_result
            .unwrap_err()
            .0
            .contains("persistence reserve")
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
    drop(held);
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let content_state: (String, Option<String>, String, Option<String>) = sqlx::query_as(
        "SELECT request_value.status,request_value.error_code,run.status,run.last_error_code
             FROM bid_async_request_snapshot_artifacts request_value
             JOIN bid_content_agent_run_artifacts run
               ON run.request_artifact_id=request_value.id AND run.attempt=$2
             WHERE request_value.id=$1",
    )
    .bind(content.request_artifact_id)
    .bind(content_owner.attempt)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        content_state,
        ("pending".into(), None, "running".into(), None)
    );
}

#[tokio::test]
async fn process_group_kill_reaps_a_helper_with_hanging_grandchild() {
    let mut command = tokio::process::Command::new("sh");
    command.arg("-c").arg("sleep 60 & wait").kill_on_drop(true);
    command.process_group(0);
    let mut child = command.spawn().unwrap();
    let pid = i32::try_from(child.id().unwrap()).unwrap();
    kill_helper_group_and_reap_child(
        &mut child,
        "hanging grandchild test",
        tokio::time::Instant::now() + std::time::Duration::from_secs(2),
    )
    .await
    .unwrap();
    assert!(child.try_wait().unwrap().is_some());
    // SAFETY: signal 0 performs only an existence check for this test-owned group.
    assert_eq!(unsafe { libc::killpg(pid, 0) }, -1);
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ESRCH)
    );
}
use platform::{apply_fresh_baseline, write_blob};

async fn install_phase_fixture(pool: &PgPool) {
    let phase = include_str!("../../bidding/tests/sql/phase1_acceptance.sql")
        .lines()
        .skip(1)
        .collect::<Vec<_>>()
        .join("\n");
    let mut connection = pool.acquire().await.unwrap();
    sqlx::Executor::execute(&mut *connection, "SET ROLE kb_app_owner")
        .await
        .unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(phase))
        .execute(&mut *connection)
        .await
        .unwrap();
}

async fn owner_connection(pool: &PgPool) -> sqlx::pool::PoolConnection<sqlx::Postgres> {
    let mut connection = pool.acquire().await.unwrap();
    sqlx::Executor::execute(&mut *connection, "SET ROLE kb_app_owner")
        .await
        .unwrap();
    connection
}

fn request_identity(value: &serde_json::Value) -> platform::BidAuthoringRequestIdentityV2 {
    platform::BidAuthoringRequestIdentityV2 {
        request_artifact_id: Uuid::parse_str(value["request_artifact_id"].as_str().unwrap())
            .unwrap(),
        request_revision: value["request_revision"].as_i64().unwrap(),
        frozen_input_sha256: value["frozen_input_sha256"].as_str().unwrap().into(),
    }
}

const TEST_ACTOR: &str = "user:10000000-0000-4000-8000-000000000001";
const TEST_PROJECT_ID: Uuid = Uuid::from_u128(0x10000000000040008000000000000010);

async fn test_workspace_head(pool: &PgPool) -> (Uuid, Uuid, String) {
    sqlx::query_as(
        "SELECT workspace.id,head.artifact_id,head.artifact_sha256
             FROM bid_submission_workspaces workspace
             JOIN bid_workspace_heads head ON head.scope_id=workspace.id
             WHERE workspace.project_id=$1",
    )
    .bind(TEST_PROJECT_ID)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn create_tender_terminal_test_request(
    pool: &PgPool,
) -> platform::BidAuthoringRequestIdentityV2 {
    let document_id = Uuid::new_v4();
    let staging_id = Uuid::new_v4();
    let request_id = Uuid::new_v4();
    let object_sha = platform::sha256_hex(document_id.as_bytes());
    let object_ref = format!("objects/{object_sha}");
    let request_bytes = br#"{"reserve_test":"tender"}"#;
    let mut connection = owner_connection(pool).await;
    sqlx::query(
        "SELECT kb_object_upload_stage($1,$2::kb_object_ref,$3::kb_sha256,
             'application/pdf',1,$4::kb_actor_identity)",
    )
    .bind(staging_id)
    .bind(&object_ref)
    .bind(&object_sha)
    .bind(TEST_ACTOR)
    .execute(&mut *connection)
    .await
    .unwrap();
    let value: serde_json::Value = sqlx::query_scalar(
        "SELECT kb_bid_v2_upload_tender_document(
             $1,$2,$3,$4,'reserve.pdf','application/pdf',1,$5::kb_object_ref,
             $6::kb_sha256,$7::kb_actor_identity,$8,$9,kb_bid_v2_sha256_bytes($9))",
    )
    .bind(staging_id)
    .bind(document_id)
    .bind(request_id)
    .bind(TEST_PROJECT_ID)
    .bind(object_ref)
    .bind(object_sha)
    .bind(TEST_ACTOR)
    .bind(format!("reserve-tender-{}", Uuid::new_v4()))
    .bind(request_bytes.as_slice())
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    request_identity(&value)
}

async fn create_export_terminal_test_request(
    pool: &PgPool,
) -> platform::BidAuthoringRequestIdentityV2 {
    let (workspace_id, revision_id, revision_sha) = test_workspace_head(pool).await;
    let request_bytes = r#"{"reserve_test":"export"}"#;
    let mut connection = owner_connection(pool).await;
    let value: serde_json::Value = sqlx::query_scalar(
        "SELECT kb_bid_v2_create_submission_export_request(
             $1,$2,$3::kb_sha256,'review_draft','pdf',
             jsonb_build_object('watermark','reserve test'),$4::kb_actor_identity,$5,
             convert_to($6,'UTF8'),kb_bid_v2_sha256_bytes(convert_to($6,'UTF8')))",
    )
    .bind(workspace_id)
    .bind(revision_id)
    .bind(revision_sha)
    .bind(TEST_ACTOR)
    .bind(format!("reserve-export-{}", Uuid::new_v4()))
    .bind(request_bytes)
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    request_identity(&value)
}

async fn ensure_content_test_checkpoint(pool: &PgPool) {
    let (workspace_id, revision_id, revision_sha) = test_workspace_head(pool).await;
    let bytes = br#"{"reserve_test":"checkpoint"}"#;
    let mut connection = owner_connection(pool).await;
    sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT kb_bid_v2_create_outline_checkpoint(
             $1,$2,$3::kb_sha256,$4,$5::kb_actor_identity,$6,$7,
             kb_bid_v2_sha256_bytes($7))",
    )
    .bind(workspace_id)
    .bind(revision_id)
    .bind(revision_sha)
    .bind(Uuid::new_v4())
    .bind(TEST_ACTOR)
    .bind(format!("reserve-checkpoint-{}", Uuid::new_v4()))
    .bind(bytes.as_slice())
    .fetch_one(&mut *connection)
    .await
    .unwrap();
}

async fn create_content_terminal_test_request(
    pool: &PgPool,
) -> platform::BidAuthoringRequestIdentityV2 {
    ensure_content_test_checkpoint(pool).await;
    let (workspace_id, revision_id, revision_sha) = test_workspace_head(pool).await;
    let request_body = serde_json::json!({"reserve_test":"content"});
    let context = bidding::MutationContext::new(
        TEST_ACTOR,
        format!("reserve-content-{}", Uuid::new_v4()),
        &request_body,
    )
    .unwrap();
    let sha = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let retrieval = knowledge::knowledge_retrieval::FrozenRetrievalPolicyIdentityV1 {
        schema_version: 1,
        policy_sha256: sha.into(),
        canonical_policy_utf8: "{}".into(),
        contract_version: "knowledge-evidence-v2".into(),
        mode: "exact".into(),
        max_hits: 1,
        max_chunk_bytes: 1,
        max_total_bytes: 1,
        embedding_revision_sha256: sha.into(),
        canonical_embedding_revision_utf8: "{}".into(),
        embedding_credential_ref: "env:EMBED".into(),
        rerank_revision_sha256: sha.into(),
        canonical_rerank_revision_utf8: "{}".into(),
        rerank_credential_ref: "env:RERANK".into(),
        product_version_ids: vec![],
        library_version_ids: vec![],
        eligible_scope_sha256: "715d78b3301b4e5901d8dc93c9d33776a0cef3378d65de32353ee8e998541901"
            .into(),
    };
    let runtime = bidding::content_runtime::ContentAgentRuntimeContractV1 {
        schema_version: 1,
        base_url: "http://127.0.0.1:18080".into(),
        endpoint: "http://127.0.0.1:18080/v1/chat/completions".into(),
        protocol: "openai_chat_completions_sse".into(),
        model_id: "scripted-content".into(),
        credential_ref: "env:KNOWLEDGEBRAIN_CHAT_API_KEY".into(),
        stream: true,
        max_tokens: 8192,
        timeout_ms: 180000,
        response_mode: "strict_json_schema".into(),
        transport_retries: 0,
        temperature: None,
        reasoning_effort: None,
    };
    let value = bidding::bid_authoring_v2::create_content_request_v2(
        pool,
        bidding::bid_authoring_v2::CreateContentRequestV2 {
            workspace_id,
            expected_revision_id: revision_id,
            expected_sha256: &revision_sha,
            operation: "generate",
            target_kind: "workspace",
            target_node_lineage_id: None,
            fill_policy: "append_candidate",
            insertion_anchor: None,
            evidence_selection_mode: "system_proposed",
            pick_set_artifact_id: None,
            retrieval_identity: Some(&retrieval),
            runtime_contract: Some(&runtime),
        },
        &context,
    )
    .await
    .unwrap();
    request_identity(&value)
}

use std::{
    collections::VecDeque,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

#[tokio::test(start_paused = true)]
async fn early_completed_error_reserves_terminal_persistence_time() {
    let started = tokio::time::Instant::now();
    let run = run_owned_handler(
        async { Err(JobErr("deterministic".into())) },
        HandlerDeadline {
            hard: started + std::time::Duration::from_secs(60),
            cleanup: started + HANDLER_CLEANUP_MARGIN,
        },
        CancellationToken::new(),
        CancellationToken::new(),
        None,
        |_| true,
    )
    .await;
    assert!(matches!(
        run.completion,
        OwnedHandlerCompletion::Completed(Err(_))
    ));
    assert_eq!(
        run.cleanup_deadline.duration_since(run.teardown_deadline),
        TERMINAL_PERSISTENCE_RESERVE
    );

    let success = run_owned_handler(
        async { Ok(()) },
        HandlerDeadline {
            hard: started + std::time::Duration::from_secs(60),
            cleanup: started + HANDLER_CLEANUP_MARGIN,
        },
        CancellationToken::new(),
        CancellationToken::new(),
        None,
        |_| true,
    )
    .await;
    assert_eq!(success.cleanup_deadline, success.teardown_deadline);
}

#[tokio::test(start_paused = true)]
async fn content_heartbeat_failure_cannot_starve_the_effect_reserve() {
    let pipeline_cancel = CancellationToken::new();
    let heartbeat_cancel = CancellationToken::new();
    let mut pipeline = tokio::spawn(std::future::pending::<
        Result<(), bidding::agent_error::AgentError>,
    >());
    let mut heartbeat = tokio::spawn(std::future::pending::<()>());
    let (lease_tx, mut lease_rx) = tokio::sync::oneshot::channel();
    lease_tx
        .send(bidding::agent_error::AgentError::new(
            "INTERNAL",
            "heartbeat database unavailable",
        ))
        .unwrap();
    let started = tokio::time::Instant::now();
    let run = tokio::spawn(async move {
        await_content_owned_completion(
            &mut pipeline,
            &mut heartbeat,
            &mut lease_rx,
            &pipeline_cancel,
            &heartbeat_cancel,
            &CancellationToken::new(),
            HandlerDeadline {
                hard: started + std::time::Duration::from_secs(60),
                cleanup: started + HANDLER_CLEANUP_MARGIN,
            },
        )
        .await
    });
    tokio::task::yield_now().await;
    tokio::time::advance(HANDLER_CLEANUP_MARGIN - TERMINAL_PERSISTENCE_RESERVE).await;
    tokio::task::yield_now().await;
    let owned = run.await.unwrap();
    assert!(matches!(
        owned.completion,
        ContentOwnedCompletion::LeaseLost(_)
    ));
    assert!(owned.cleanup_deadline > tokio::time::Instant::now());
    assert_eq!(
        owned
            .cleanup_deadline
            .duration_since(tokio::time::Instant::now()),
        TERMINAL_PERSISTENCE_RESERVE
    );
}

#[tokio::test(start_paused = true)]
async fn all_active_v2_handler_deadlines_fire_at_the_exact_boundary() {
    let deadlines = [
        ("TenderDocumentProcess", TENDER_HANDLER_HARD_TIMEOUT),
        ("RequirementSetCompile", REQUIREMENT_HANDLER_HARD_TIMEOUT),
        ("DocxCompose", DOCX_COMPOSE_HANDLER_HARD_TIMEOUT),
        (
            "ContentGenerate(generate)",
            CONTENT_GENERATE_HANDLER_HARD_TIMEOUT,
        ),
        (
            "ContentGenerate(match_only)",
            CONTENT_MATCH_HANDLER_HARD_TIMEOUT,
        ),
        ("SubmissionExport", SUBMISSION_EXPORT_HANDLER_HARD_TIMEOUT),
    ];
    for (kind, deadline) in deadlines {
        let shutdown = CancellationToken::new();
        let local = CancellationToken::new();
        let pipeline_cancel = local.clone();
        let task = tokio::spawn(run_owned_handler(
            async move {
                pipeline_cancel.cancelled().await;
                Ok(())
            },
            HandlerDeadline::from_now(deadline),
            shutdown,
            local,
            None,
            |_| false,
        ));
        tokio::task::yield_now().await;
        tokio::time::advance(deadline - std::time::Duration::from_millis(1)).await;
        assert!(
            !task.is_finished(),
            "{kind} stopped before its exact deadline"
        );
        tokio::time::advance(std::time::Duration::from_millis(1)).await;
        tokio::task::yield_now().await;
        assert!(matches!(
            task.await.unwrap().completion,
            OwnedHandlerCompletion::TimedOut
        ));
    }
}

#[tokio::test(start_paused = true)]
async fn global_cancel_joins_handler_without_turning_it_into_a_timeout() {
    let shutdown = CancellationToken::new();
    let local = CancellationToken::new();
    let pipeline_cancel = local.clone();
    let stopped = Arc::new(AtomicBool::new(false));
    let pipeline_stopped = stopped.clone();
    let task = tokio::spawn(run_owned_handler(
        async move {
            pipeline_cancel.cancelled().await;
            pipeline_stopped.store(true, Ordering::SeqCst);
            Ok(())
        },
        HandlerDeadline::from_now(TENDER_HANDLER_HARD_TIMEOUT),
        shutdown.clone(),
        local,
        None,
        |_| false,
    ));
    tokio::task::yield_now().await;
    shutdown.cancel();
    tokio::task::yield_now().await;
    assert!(matches!(
        task.await.unwrap().completion,
        OwnedHandlerCompletion::ShuttingDown
    ));
    assert!(stopped.load(Ordering::SeqCst));
    tokio::time::advance(std::time::Duration::from_secs(60 * 60)).await;
    assert!(
        stopped.load(Ordering::SeqCst),
        "joined pipeline changed after return"
    );
}

#[tokio::test(start_paused = true)]
async fn content_generate_timeout_lease_loss_and_global_cancel_join_children() {
    async fn pending_pipeline(
        cancel: CancellationToken,
        stopped: Arc<AtomicBool>,
    ) -> Result<(), bidding::agent_error::AgentError> {
        struct StopProof(Arc<AtomicBool>);
        impl Drop for StopProof {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let _proof = StopProof(stopped);
        cancel.cancelled().await;
        Err(bidding::agent_error::AgentError::new(
            "INTERNAL",
            "cancelled",
        ))
    }

    let shutdown = CancellationToken::new();
    let pipeline_cancel = CancellationToken::new();
    let heartbeat_cancel = CancellationToken::new();
    let stopped = Arc::new(AtomicBool::new(false));
    let mut pipeline = tokio::spawn(pending_pipeline(pipeline_cancel.clone(), stopped.clone()));
    let heartbeat_stop = heartbeat_cancel.clone();
    let mut heartbeat = tokio::spawn(async move { heartbeat_stop.cancelled().await });
    let (_lease_tx, mut lease_rx) = tokio::sync::oneshot::channel();
    shutdown.cancel();
    assert!(matches!(
        await_content_owned_completion(
            &mut pipeline,
            &mut heartbeat,
            &mut lease_rx,
            &pipeline_cancel,
            &heartbeat_cancel,
            &shutdown,
            HandlerDeadline::from_now(CONTENT_GENERATE_HANDLER_HARD_TIMEOUT),
        )
        .await
        .completion,
        ContentOwnedCompletion::ShuttingDown
    ));
    assert!(pipeline.is_finished() && heartbeat.is_finished());
    assert!(stopped.load(Ordering::SeqCst));

    let shutdown = CancellationToken::new();
    let pipeline_cancel = CancellationToken::new();
    let heartbeat_cancel = CancellationToken::new();
    let mut pipeline = tokio::spawn(pending_pipeline(
        pipeline_cancel.clone(),
        Arc::new(AtomicBool::new(false)),
    ));
    let heartbeat_stop = heartbeat_cancel.clone();
    let mut heartbeat = tokio::spawn(async move { heartbeat_stop.cancelled().await });
    let (lease_tx, mut lease_rx) = tokio::sync::oneshot::channel();
    lease_tx
        .send(bidding::agent_error::AgentError::new(
            "REQUEST_ATTEMPT_SUPERSEDED",
            "expired owner",
        ))
        .unwrap();
    match await_content_owned_completion(
        &mut pipeline,
        &mut heartbeat,
        &mut lease_rx,
        &pipeline_cancel,
        &heartbeat_cancel,
        &shutdown,
        HandlerDeadline::from_now(CONTENT_GENERATE_HANDLER_HARD_TIMEOUT),
    )
    .await
    .completion
    {
        ContentOwnedCompletion::LeaseLost(error) => assert_eq!(
            error.disposition,
            bidding::agent_error::RetryDisposition::Obsolete
        ),
        _ => panic!("expected lease loss"),
    }
    assert!(pipeline.is_finished() && heartbeat.is_finished());

    let shutdown = CancellationToken::new();
    let pipeline_cancel = CancellationToken::new();
    let heartbeat_cancel = CancellationToken::new();
    let mut pipeline = tokio::spawn(pending_pipeline(
        pipeline_cancel.clone(),
        Arc::new(AtomicBool::new(false)),
    ));
    let heartbeat_stop = heartbeat_cancel.clone();
    let mut heartbeat = tokio::spawn(async move { heartbeat_stop.cancelled().await });
    let (_lease_tx, mut lease_rx) = tokio::sync::oneshot::channel();
    let timeout_task = tokio::spawn(async move {
        await_content_owned_completion(
            &mut pipeline,
            &mut heartbeat,
            &mut lease_rx,
            &pipeline_cancel,
            &heartbeat_cancel,
            &shutdown,
            HandlerDeadline::from_now(CONTENT_GENERATE_HANDLER_HARD_TIMEOUT),
        )
        .await
    });
    tokio::task::yield_now().await;
    tokio::time::advance(
        CONTENT_GENERATE_HANDLER_HARD_TIMEOUT - std::time::Duration::from_millis(1),
    )
    .await;
    assert!(!timeout_task.is_finished());
    tokio::time::advance(std::time::Duration::from_millis(1)).await;
    tokio::task::yield_now().await;
    assert!(matches!(
        timeout_task.await.unwrap().completion,
        ContentOwnedCompletion::TimedOut
    ));
}

#[tokio::test(start_paused = true)]
async fn transport_group_supervisor_handles_cancel_fatal_and_forced_abort() {
    assert_eq!(TRANSPORT_RUNTIME_COUNT, 6);
    let graceful_cancel = CancellationToken::new();
    let (graceful_tx, mut graceful_rx) = tokio::sync::watch::channel(false);
    let mut graceful_tasks = tokio::task::JoinSet::new();
    graceful_tasks.spawn(async move {
        while !*graceful_rx.borrow() {
            graceful_rx
                .changed()
                .await
                .map_err(|error| oxana::OxanaError::GenericError(error.to_string()))?;
        }
        Ok(())
    });
    graceful_cancel.cancel();
    join_transport_group(graceful_tasks, &graceful_cancel, graceful_tx)
        .await
        .unwrap();

    let fatal_cancel = CancellationToken::new();
    let (fatal_tx, mut fatal_rx) = tokio::sync::watch::channel(false);
    let sibling_stopped = Arc::new(AtomicBool::new(false));
    let sibling_flag = sibling_stopped.clone();
    let mut fatal_tasks = tokio::task::JoinSet::new();
    fatal_tasks.spawn(async { Err(oxana::OxanaError::GenericError("fatal child".into())) });
    fatal_tasks.spawn(async move {
        while !*fatal_rx.borrow() {
            fatal_rx
                .changed()
                .await
                .map_err(|error| oxana::OxanaError::GenericError(error.to_string()))?;
        }
        sibling_flag.store(true, Ordering::SeqCst);
        Ok(())
    });
    assert!(
        join_transport_group(fatal_tasks, &fatal_cancel, fatal_tx)
            .await
            .unwrap_err()
            .to_string()
            .contains("fatal child")
    );
    assert!(fatal_cancel.is_cancelled());
    assert!(sibling_stopped.load(Ordering::SeqCst));

    struct DropProof(Arc<AtomicBool>);
    impl Drop for DropProof {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let forced_cancel = CancellationToken::new();
    let (forced_tx, _forced_rx) = tokio::sync::watch::channel(false);
    let dropped = Arc::new(AtomicBool::new(false));
    let proof = DropProof(dropped.clone());
    let mut forced_tasks = tokio::task::JoinSet::new();
    forced_tasks.spawn(async move {
        let _proof = proof;
        std::future::pending::<()>().await;
        Ok(())
    });
    forced_cancel.cancel();
    let supervisor =
        tokio::spawn(
            async move { join_transport_group(forced_tasks, &forced_cancel, forced_tx).await },
        );
    tokio::task::yield_now().await;
    tokio::time::advance(HANDLER_CLEANUP_MARGIN).await;
    assert!(
        supervisor
            .await
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("cleanup exceeded")
    );
    assert!(dropped.load(Ordering::SeqCst));
}

#[test]
fn bidding_transport_retry_count_never_classifies_business_outcomes() {
    assert_eq!(platform::BID_AUTHORING_V2_MAX_RETRIES, 3);
    assert!(!include_str!("runtime.rs").contains(concat!("bid_failure_", "is_final")));
}

#[test]
fn frozen_asset_metadata_budget_rejects_before_reads_or_allocations() {
    let oversized = vec![serde_json::json!({
        "asset_revision_id":Uuid::new_v4(),
        "media_type":"image/png",
        "byte_length":bidding::submission_export::MAX_FROZEN_ASSET_TOTAL_BYTES + 1,
        "width_px":1,
        "height_px":1
    })];
    assert!(validate_frozen_asset_metadata(&oversized).is_err());
    let pixel_overflow = vec![serde_json::json!({
        "asset_revision_id":Uuid::new_v4(),
        "media_type":"image/png",
        "byte_length":1,
        "width_px":bidding::submission_export::MAX_FROZEN_ASSET_TOTAL_PIXELS,
        "height_px":2
    })];
    assert!(validate_frozen_asset_metadata(&pixel_overflow).is_err());
    let missing_length = vec![serde_json::json!({
        "asset_revision_id":Uuid::new_v4(),
        "media_type":"application/pdf",
        "page_count":1
    })];
    assert!(validate_frozen_asset_metadata(&missing_length).is_err());
}

#[test]
fn export_metadata_preflight_rejects_aggregate_table_work() {
    let input = serde_json::json!({
        "assets":[],
        "workspace":{"blocks":[{"content":{"type":"table","row_count":1_000_001u64,
            "column_count":1,"cells":[]}}]},
        "form_definitions":[],
        "attachment_preparations":[]
    });
    assert!(validate_submission_export_metadata(&input).is_err());
}

#[tokio::test]
async fn content_pipeline_abort_is_authoritatively_joined() {
    struct Dropped(Arc<AtomicBool>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let dropped = Arc::new(AtomicBool::new(false));
    let observed = dropped.clone();
    let cancel = CancellationToken::new();
    let pipeline_cancel = cancel.clone();
    let mut pipeline = tokio::spawn(async move {
        let _guard = Dropped(observed);
        pipeline_cancel.cancelled().await;
    });
    tokio::task::yield_now().await;
    cancel_and_join_task_until(
        &mut pipeline,
        &cancel,
        tokio::time::Instant::now() + HANDLER_CLEANUP_MARGIN,
    )
    .await;
    assert!(dropped.load(Ordering::SeqCst));
    assert!(pipeline.is_finished());
}

#[test]
fn content_ordinal_three_returns_the_exact_reserved_call_failure() {
    for code in [
        "AGENT_TURN_TIMEOUT",
        "AGENT_PROVIDER_UNAVAILABLE",
        "AGENT_OUTPUT_INVALID",
    ] {
        let failure = bidding::agent_error::AgentError::new(code, "ordinal-three");
        let returned =
            bidding::content_generate::retain_content_attempt_failure(3, failure).unwrap_err();
        assert_eq!(returned.code, code);
        assert_eq!(returned.message, "ordinal-three");
    }
    assert!(
        bidding::content_generate::retain_content_attempt_failure(
            2,
            bidding::agent_error::AgentError::new("AGENT_PROVIDER_UNAVAILABLE", "retryable",),
        )
        .unwrap()
        .contains("AGENT_PROVIDER_UNAVAILABLE")
    );
}

#[test]
fn content_candidate_verifier_rejects_unfrozen_targets_and_unknown_fields() {
    use bidding::content_block::{
        BlockContent, BlockKind, BlockOrigin, ContentBlockV1, Inline, RichNode,
    };
    let lineage = Uuid::new_v4();
    let content = BlockContent::RichText {
        nodes: vec![RichNode::Paragraph {
            content: vec![Inline::Text {
                text: "【待人工补充】候选响应".into(),
                marks: vec![],
            }],
        }],
    };
    let block = ContentBlockV1 {
        schema_version: 1,
        block_revision_id: Uuid::new_v4(),
        lineage_id: Uuid::new_v4(),
        revision: 1,
        kind: BlockKind::RichText,
        content_sha256: content.sha256().unwrap(),
        content,
        origin: BlockOrigin::AgentCandidate,
    };
    let requirement = Uuid::new_v4();
    let input = serde_json::json!({"target_nodes":[{"node_lineage_id":lineage,
            "node_revision_id":Uuid::new_v4(),"block_count":0,"blocks":[]}],
            "requirements":[{"requirement_revision_id":requirement}],
            "fill_policy":"append_candidate","generation_dependency_sha256":"a".repeat(64)});
    let output = serde_json::json!({"schema_version":1,"operations":[{
            "kind":"insert_block","client_operation_ref":"op-0","target_node_lineage_id":lineage,
            "ordinal":0,"block":block}],"factual_claims":[],"notices":[]});
    assert!(
        bidding::content_generate::content_candidate_output(
            &serde_json::to_string(&output).unwrap(),
            &input
        )
        .is_ok()
    );
    let mut invalid_notice = output.clone();
    invalid_notice["notices"] = serde_json::json!([{
        "code":"NO_EVIDENCE","severity":"warning","message":"需要证据",
        "requirement_revision_id":Uuid::new_v4()
    }]);
    assert!(
        bidding::content_generate::content_candidate_output(
            &serde_json::to_string(&invalid_notice).unwrap(),
            &input
        )
        .is_err()
    );
    let mut invalid = output;
    invalid["operations"][0]["target_node_lineage_id"] = serde_json::json!(Uuid::new_v4());
    assert!(
        bidding::content_generate::content_candidate_output(
            &serde_json::to_string(&invalid).unwrap(),
            &input
        )
        .is_err()
    );
}

#[test]
fn content_candidate_verifier_accepts_only_frozen_image_evidence_assets() {
    use bidding::content_block::{
        BlockContent, BlockKind, BlockOrigin, ContentBlockV1, Crop, ImageAlignment,
    };
    let node = Uuid::new_v4();
    let evidence_item = Uuid::new_v4();
    let content = BlockContent::Image {
        asset_revision_id: evidence_item,
        width_mm: 120.0,
        alignment: ImageAlignment::Center,
        crop: Crop {
            left: 0.0,
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
        },
        caption: Some("产品实拍图".into()),
        alt: "产品实拍图".into(),
    };
    let block = ContentBlockV1 {
        schema_version: 1,
        block_revision_id: Uuid::new_v4(),
        lineage_id: Uuid::new_v4(),
        revision: 1,
        kind: BlockKind::Image,
        content_sha256: content.sha256().unwrap(),
        content,
        origin: BlockOrigin::AgentCandidate,
    };
    let input = serde_json::json!({
        "target_nodes":[{"node_lineage_id":node,"node_revision_id":Uuid::new_v4(),
            "block_count":0,"blocks":[]}],
        "fill_policy":"append_candidate","generation_dependency_sha256":"a".repeat(64),
        "evidence_matches":[{"items":[{"kind":"image","evidence_item_id":evidence_item}]}]
    });
    let output = serde_json::json!({"schema_version":1,"operations":[{
            "kind":"insert_block","client_operation_ref":"image-0","target_node_lineage_id":node,
            "ordinal":0,"block":block}],"factual_claims":[],"notices":[]});
    assert!(
        bidding::content_generate::content_candidate_output(
            &serde_json::to_string(&output).unwrap(),
            &input
        )
        .is_ok()
    );
    let mut invalid = output;
    invalid["operations"][0]["block"]["content"]["asset_revision_id"] =
        serde_json::json!(Uuid::new_v4());
    assert!(
        bidding::content_generate::content_candidate_output(
            &serde_json::to_string(&invalid).unwrap(),
            &input
        )
        .is_err()
    );
}

#[test]
fn content_candidate_verifier_rejects_forbidden_content_code_links_and_offset_drift() {
    let node = Uuid::new_v4();
    let bundle = Uuid::new_v4();
    let item = Uuid::new_v4();
    let input = serde_json::json!({
        "target_nodes":[{"node_lineage_id":node,"node_revision_id":Uuid::new_v4(),
            "block_count":0,"blocks":[]}],
        "requirements":[],"fill_policy":"append_candidate",
        "generation_dependency_sha256":"a".repeat(64),
        "evidence_matches":[{"evidence_bundle_id":bundle,"items":[{
            "kind":"text_quote","evidence_item_id":item,"quote_utf8":"中文事实",
            "quote_start_offset":0,"quote_end_offset":12}]}]
    });
    let block = |content: serde_json::Value, kind: &str| {
        serde_json::json!({
            "schema_version":1,"block_revision_id":Uuid::new_v4(),"lineage_id":Uuid::new_v4(),
            "revision":1,"kind":kind,"content_sha256":"a".repeat(64),
            "content":content,"origin":"agent_candidate"
        })
    };
    let output = |block: serde_json::Value| {
        serde_json::json!({
            "schema_version":1,"operations":[{"kind":"insert_block","client_operation_ref":"op",
                "target_node_lineage_id":node,"ordinal":0,"block":block}],
            "factual_claims":[],"notices":[]
        })
    };
    let forbidden = [
        (
            serde_json::json!({"type":"attachment_ref","asset_revision_id":Uuid::new_v4(),
                "preparation_revision_id":null,"render_mode":"file_reference","start_new_page":false}),
            "attachment_ref",
        ),
        (
            serde_json::json!({"type":"structured_form","form_definition_revision_id":Uuid::new_v4(),"field_values":[]}),
            "structured_form",
        ),
        (serde_json::json!({"type":"page_break"}), "page_break"),
        (
            serde_json::json!({"type":"signature_placeholder","signature_kind":"signature",
                "width_mm":30.0,"height_mm":20.0,"label":"签字"}),
            "signature_placeholder",
        ),
    ];
    for (content, kind) in forbidden {
        let value = output(block(content, kind));
        assert!(
            bidding::content_generate::content_candidate_output(&value.to_string(), &input)
                .is_err(),
            "{kind}"
        );
    }
    for node_value in [
        serde_json::json!({"kind":"code_block","language":"sql","text":"SELECT 1"}),
        serde_json::json!({"kind":"paragraph","content":[{"kind":"text","text":"x",
                "marks":[{"kind":"code"}]}]}),
        serde_json::json!({"kind":"paragraph","content":[{"kind":"text","text":"x",
                "marks":[{"kind":"link","href":"https://example.invalid"}]}]}),
    ] {
        let value = output(block(
            serde_json::json!({"type":"rich_text","nodes":[node_value]}),
            "rich_text",
        ));
        assert!(
            bidding::content_generate::content_candidate_output(&value.to_string(), &input)
                .is_err()
        );
    }
    let mut evidenced = output(block(
        serde_json::json!({"type":"rich_text","nodes":[{
            "kind":"paragraph","content":[{"kind":"text","text":"中文事实","marks":[{
                "kind":"evidence_ref","evidence_bundle_id":bundle,"evidence_item_id":item,
                "quote_start_offset":0,"quote_end_offset":12}]}]}]}),
        "rich_text",
    ));
    evidenced["factual_claims"] = serde_json::json!([{"client_operation_ref":"op",
            "utf8_start":0,"utf8_end":12,"evidence_bundle_id":bundle,"evidence_item_id":item}]);
    assert!(
        bidding::content_generate::content_candidate_output(&evidenced.to_string(), &input).is_ok()
    );
    evidenced["operations"][0]["block"]["content"]["nodes"][0]["content"][0]["marks"][0]["quote_end_offset"] =
        serde_json::json!(11);
    assert!(
        bidding::content_generate::content_candidate_output(&evidenced.to_string(), &input)
            .is_err()
    );
}

#[derive(Clone, Copy)]
enum LifecycleProviderResult {
    Success,
    Unavailable,
}

struct LifecycleProvider {
    calls: AtomicUsize,
    results: std::sync::Mutex<VecDeque<LifecycleProviderResult>>,
}

#[async_trait]
impl knowledge::knowledge_index_v2::VectorEmbeddingProviderV2 for LifecycleProvider {
    async fn embed_batch(
        &self,
        _revision: &knowledge::knowledge_retrieval::EmbeddingRevisionV2,
        _credential_ref: &str,
        inputs: &[knowledge::knowledge_index_v2::VectorEmbeddingInputV2],
    ) -> Result<Vec<Vec<f32>>, knowledge::knowledge_index_v2::VectorIndexErrorV2> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match self
            .results
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(LifecycleProviderResult::Success)
        {
            LifecycleProviderResult::Success => Ok(inputs
                .iter()
                .enumerate()
                .map(|(index, _)| {
                    let mut vector = vec![0.0; 1024];
                    vector[index % 1024] = 1.0;
                    vector
                })
                .collect()),
            LifecycleProviderResult::Unavailable => Err(
                knowledge::knowledge_index_v2::VectorIndexErrorV2::Unavailable(
                    "injected provider timeout".into(),
                ),
            ),
        }
    }
}

struct PendingAfterLifecycleProvider {
    pool: PgPool,
    document_id: Uuid,
    calls: AtomicUsize,
}

#[async_trait]
impl knowledge::knowledge_index_v2::VectorEmbeddingProviderV2 for PendingAfterLifecycleProvider {
    async fn embed_batch(
        &self,
        _revision: &knowledge::knowledge_retrieval::EmbeddingRevisionV2,
        _credential_ref: &str,
        inputs: &[knowledge::knowledge_index_v2::VectorEmbeddingInputV2],
    ) -> Result<Vec<Vec<f32>>, knowledge::knowledge_index_v2::VectorIndexErrorV2> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        sqlx::query("UPDATE documents SET pending_subtasks_count=1 WHERE id=$1")
            .bind(self.document_id)
            .execute(&self.pool)
            .await
            .map_err(knowledge::knowledge_index_v2::VectorIndexErrorV2::Database)?;
        Ok(inputs
            .iter()
            .enumerate()
            .map(|(index, _)| {
                let mut vector = vec![0.0; 1024];
                vector[index % 1024] = 1.0;
                vector
            })
            .collect())
    }
}

struct MissingLifecycleCredential {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl knowledge::knowledge_index_v2::EmbeddingCredentialResolverV2 for MissingLifecycleCredential {
    async fn resolve(
        &self,
        _credential_ref: &str,
    ) -> Result<String, knowledge::knowledge_index_v2::VectorIndexErrorV2> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(
            knowledge::knowledge_index_v2::VectorIndexErrorV2::InvalidConfiguration(
                "injected missing credential reference".into(),
            ),
        )
    }
}

#[test]
fn semantic_index_v2_uses_the_native_three_by_ten_oxana_policy() {
    let job = KnowledgeSemanticIndexV2Job {
        target_id: Uuid::parse_str("018f3000-7d47-7a1b-9bb8-b3880f15478a").unwrap(),
        target_revision: 7,
    };
    let worker = KnowledgeSemanticIndexV2Worker {
        pool: None,
        shutdown: CancellationToken::new(),
        provider: None,
        provider_configuration_error: None,
    };
    assert_eq!(
        <KnowledgeSemanticIndexV2Worker as oxana::Worker<KnowledgeSemanticIndexV2Job>>::max_retries(
            &worker, &job
        ),
        3
    );
    for retries in 0..=3 {
        assert_eq!(
                <KnowledgeSemanticIndexV2Worker as oxana::Worker<
                    KnowledgeSemanticIndexV2Job,
                >>::retry_delay(&worker, &job, retries),
                10
            );
    }
}

#[test]
fn reuse_reads_scanned_pdf_from_docreader_span() {
    let tagged = serde_json::json!({"image_source_type": "scanned_pdf"});
    assert_eq!(
        knowledge::ingest::image_source_from_docreader_output(Some(&tagged)),
        "scanned_pdf"
    );
    let fallback = serde_json::json!({"anydoc_fallback": "scanned_pdf"});
    assert_eq!(
        knowledge::ingest::image_source_from_docreader_output(Some(&fallback)),
        "scanned_pdf"
    );
    assert_eq!(
        knowledge::ingest::image_source_from_docreader_output(None),
        ""
    );
}
use tokio::sync::Mutex;

async fn db_lock() -> tokio::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::const_new(());
    LOCK.lock().await
}

// Destructive schema tests must opt into a dedicated isolated database. Never
// inherit DATABASE_URL: production Compose is intentionally exposed on :15432.
fn destructive_test_database_url() -> Result<String, sqlx::Error> {
    let database_url = std::env::var("KNOWLEDGEBRAIN_TEST_DATABASE_URL").map_err(|_| {
        sqlx::Error::Configuration(
            "KNOWLEDGEBRAIN_TEST_DATABASE_URL is required for destructive PostgreSQL tests".into(),
        )
    })?;
    if database_url.contains(":15432/") {
        return Err(sqlx::Error::Configuration(
            "destructive PostgreSQL tests refuse the live :15432 database".into(),
        ));
    }
    Ok(database_url)
}

// Tokio creates a separate runtime for each async unit test. A process-global
// PgPool can retain runtime-bound connections between tests and time out.
async fn connect() -> Result<sqlx::PgPool, sqlx::Error> {
    let database_url = destructive_test_database_url()?;
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(16)
        .connect(&database_url)
        .await
}

async fn reset_test_schema(pool: &sqlx::PgPool) {
    sqlx::raw_sql(
        "DROP SCHEMA public CASCADE;
             CREATE SCHEMA public;
             GRANT ALL ON SCHEMA public TO CURRENT_USER;
             CREATE EXTENSION IF NOT EXISTS pgcrypto;
             CREATE EXTENSION IF NOT EXISTS vector;
             ALTER SCHEMA public OWNER TO kb_app_owner;",
    )
    .execute(pool)
    .await
    .unwrap();
    apply_fresh_baseline(pool).await.unwrap();
}

#[test]
fn wiki_ingest_retry_delay_is_lock_retry() {
    let w = WikiIngestWorker {
        pool: None,
        shutdown: CancellationToken::new(),
    };
    let job = WikiIngestJob {
        product_version_id: Uuid::new_v4(),
        document_id: Uuid::new_v4(),
        operation: knowledge::wiki::OP_INGEST.into(),
        task_type: platform::TYPE_WIKI_INGEST.to_string(),
    };
    assert_eq!(
        oxana::Worker::<WikiIngestJob>::retry_delay(&w, &job, 0),
        platform::WIKI_LOCK_RETRY_SECS
    );
    assert_eq!(
        oxana::Worker::<WikiIngestJob>::retry_delay(&w, &job, 4),
        platform::WIKI_LOCK_RETRY_SECS
    );
    assert_eq!(
        knowledge::wiki::INGEST_DEBOUNCE_SECS,
        platform::WIKI_INGEST_DEBOUNCE_SECS
    );
    assert_eq!(
        knowledge::wiki::FINALIZE_DEBOUNCE_SECS,
        platform::WIKI_FINALIZE_DEBOUNCE_SECS
    );
    assert_eq!(
        knowledge::wiki::FOLLOW_UP_DEBOUNCE_SECS,
        platform::WIKI_FOLLOW_UP_DEBOUNCE_SECS
    );
    assert_eq!(
        knowledge::wiki::LOCK_RETRY_SECS,
        platform::WIKI_LOCK_RETRY_SECS
    );
}

#[tokio::test]
async fn list_delete_skips_non_deleting_rows() {
    let _g = db_lock().await;
    let Ok(pool) = connect().await else {
        eprintln!("skip: postgres down");
        return;
    };
    reset_test_schema(&pool).await;
    let owner = Uuid::new_v4();
    insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
        .await
        .unwrap();
    let seeded = create_workspace_with_library(&pool, owner, "Ld", "ld")
        .await
        .unwrap();
    let did = Uuid::new_v4();
    let hash = platform::sha256_hex(b"keep");
    write_blob(&hash, b"keep").unwrap();
    insert_document(
        &pool,
        knowledge::NewDocument {
            id: did,
            product_version_id: seeded.library_version_id,
            title: "k",
            file_name: "k.txt",
            file_size: 4,
            file_hash: &hash,
            object_ref: &format!("objects/{hash}"),
        },
    )
    .await
    .unwrap();
    process_list_delete_pg(&pool, did).await.unwrap();
    let gone: bool =
        sqlx::query_scalar("SELECT deleted_at IS NOT NULL FROM documents WHERE id = $1")
            .bind(did)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(!gone, "pending row must survive list_delete");
    sqlx::query("UPDATE documents SET parse_status = 'deleting' WHERE id = $1")
        .bind(did)
        .execute(&pool)
        .await
        .unwrap();
    process_list_delete_pg(&pool, did).await.unwrap();
    let gone: bool =
        sqlx::query_scalar("SELECT deleted_at IS NOT NULL FROM documents WHERE id = $1")
            .bind(did)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(gone);
}

#[tokio::test]
async fn persist_blank_chunks_completes_without_postprocess() {
    let _g = db_lock().await;
    let Ok(pool) = connect().await else {
        eprintln!("skip: postgres down");
        return;
    };
    reset_test_schema(&pool).await;
    let owner = Uuid::new_v4();
    insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
        .await
        .unwrap();
    let seeded = create_workspace_with_library(&pool, owner, "Blank", "blank")
        .await
        .unwrap();
    let did = Uuid::new_v4();
    insert_document(
        &pool,
        knowledge::NewDocument {
            id: did,
            product_version_id: seeded.library_version_id,
            title: "empty",
            file_name: "e.txt",
            file_size: 1,
            file_hash: "ff71cf74abb3ccb005b8b64371725db15edc42c1ad33413bbe561b2da3c85ef9",
            object_ref: "objects/ff71cf74abb3ccb005b8b64371725db15edc42c1ad33413bbe561b2da3c85ef9",
        },
    )
    .await
    .unwrap();
    sqlx::query("UPDATE documents SET parse_status = 'processing' WHERE id = $1")
        .bind(did)
        .execute(&pool)
        .await
        .unwrap();
    let blank = knowledge::Chunk {
        id: Uuid::new_v4(),
        document_id: did,
        product_version_id: seeded.library_version_id,
        chunk_type: "text".into(),
        content: "  \n".into(),
        context_header: String::new(),
        start_at: 0,
        end_at: 0,
        parent_chunk_id: None,
        generated_questions: Vec::new(),
    };
    let out = knowledge::ingest::persist_indexed_chunks(
        &pool,
        did,
        seeded.library_version_id,
        &[blank],
        true,
        true,
    )
    .await
    .unwrap();
    let knowledge::ingest::PersistIndexResult::Written { text_count } = out else {
        panic!("expected written");
    };
    assert_eq!(text_count, 0);
    knowledge::ingest::after_index_fanout(
        &pool,
        did,
        seeded.library_version_id,
        1,
        text_count,
        &[],
        "e.txt",
    )
    .await
    .unwrap();
    let (parse, enable, summary): (String, String, String) = sqlx::query_as(
        "SELECT parse_status, enable_status, summary_status FROM documents WHERE id = $1",
    )
    .bind(did)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(parse, "completed");
    assert_eq!(enable, "enabled");
    assert_eq!(summary, "none");
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM chunks WHERE document_id = $1")
        .bind(did)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn persist_indexed_chunks_keeps_rows_when_embed_fails() {
    let _g = db_lock().await;
    let Ok(pool) = connect().await else {
        eprintln!("skip: postgres down");
        return;
    };
    reset_test_schema(&pool).await;
    let owner = Uuid::new_v4();
    insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
        .await
        .unwrap();
    let seeded = create_workspace_with_library(&pool, owner, "Ef", "ef")
        .await
        .unwrap();
    let did = Uuid::new_v4();
    let hash = platform::sha256_hex(b"body");
    write_blob(&hash, b"body").unwrap();
    insert_document(
        &pool,
        knowledge::NewDocument {
            id: did,
            product_version_id: seeded.library_version_id,
            title: "t",
            file_name: "t.txt",
            file_size: 4,
            file_hash: &hash,
            object_ref: &format!("objects/{hash}"),
        },
    )
    .await
    .unwrap();
    sqlx::query("UPDATE documents SET parse_status = 'processing' WHERE id = $1")
        .bind(did)
        .execute(&pool)
        .await
        .unwrap();
    let ch = knowledge::Chunk {
        id: Uuid::new_v4(),
        document_id: did,
        product_version_id: seeded.library_version_id,
        chunk_type: "text".into(),
        content: "keep this chunk".into(),
        context_header: String::new(),
        start_at: 0,
        end_at: 15,
        parent_chunk_id: None,
        generated_questions: Vec::new(),
    };
    let prev_base = std::env::var("KNOWLEDGEBRAIN_EMBEDDING_BASE_URL").ok();
    let prev_alias = std::env::var("EMBEDDING_BASE_URL").ok();
    unsafe {
        std::env::set_var("KNOWLEDGEBRAIN_EMBEDDING_BASE_URL", "http://127.0.0.1:1");
        std::env::set_var("EMBEDDING_BASE_URL", "http://127.0.0.1:1");
    }
    let err = match knowledge::ingest::persist_indexed_chunks(
        &pool,
        did,
        seeded.library_version_id,
        &[ch],
        true,
        true,
    )
    .await
    {
        Ok(_) => panic!("embed must fail"),
        Err(e) => e,
    };
    unsafe {
        match prev_base {
            Some(v) => std::env::set_var("KNOWLEDGEBRAIN_EMBEDDING_BASE_URL", v),
            None => std::env::remove_var("KNOWLEDGEBRAIN_EMBEDDING_BASE_URL"),
        }
        match prev_alias {
            Some(v) => std::env::set_var("EMBEDDING_BASE_URL", v),
            None => std::env::remove_var("EMBEDDING_BASE_URL"),
        }
    }
    assert!(
        err.contains("embed") || err.contains("error") || err.contains("connect"),
        "{err}"
    );
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM chunks WHERE document_id = $1")
        .bind(did)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1, "chunk row must survive embed failure");
    let e: i64 = sqlx::query_scalar("SELECT count(*) FROM chunk_embeddings WHERE document_id = $1")
        .bind(did)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(e, 0);
}

#[tokio::test]
async fn convert_reuses_markdown_and_chunks_after_embed_fail() {
    let _g = db_lock().await;
    let Ok(pool) = connect().await else {
        eprintln!("skip: postgres down");
        return;
    };
    reset_test_schema(&pool).await;
    let owner = Uuid::new_v4();
    insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
        .await
        .unwrap();
    let seeded = create_workspace_with_library(&pool, owner, "Ru", "ru")
        .await
        .unwrap();
    let did = Uuid::new_v4();
    let hash = platform::sha256_hex(b"hello reuse");
    write_blob(&hash, b"hello reuse").unwrap();
    insert_document(
        &pool,
        knowledge::NewDocument {
            id: did,
            product_version_id: seeded.library_version_id,
            title: "r",
            file_name: "r.txt",
            file_size: 11,
            file_hash: &hash,
            object_ref: &format!("objects/{hash}"),
        },
    )
    .await
    .unwrap();
    convert_document(&pool, did, 1, &[], false).await.unwrap();
    let started: String = sqlx::query_scalar(
        "SELECT started_at::text FROM document_processing_spans
             WHERE document_id = $1 AND name = 'docreader'",
    )
    .bind(did)
    .fetch_one(&pool)
    .await
    .unwrap();
    let chunk_ids: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM chunks WHERE document_id = $1 ORDER BY id")
            .bind(did)
            .fetch_all(&pool)
            .await
            .unwrap();
    assert!(!chunk_ids.is_empty());
    sqlx::query("DELETE FROM chunk_embeddings WHERE document_id = $1")
        .bind(did)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE document_processing_spans SET status = 'failed', finished_at = now()
             WHERE document_id = $1 AND name = 'embedding'",
    )
    .bind(did)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("UPDATE documents SET parse_status = 'processing' WHERE id = $1")
        .bind(did)
        .execute(&pool)
        .await
        .unwrap();
    convert_document(&pool, did, 1, &[], false).await.unwrap();
    let started2: String = sqlx::query_scalar(
        "SELECT started_at::text FROM document_processing_spans
             WHERE document_id = $1 AND name = 'docreader'",
    )
    .bind(did)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(started, started2, "docreader must not rerun");
    let chunk_ids2: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM chunks WHERE document_id = $1 ORDER BY id")
            .bind(did)
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(chunk_ids, chunk_ids2);
    let emb: i64 =
        sqlx::query_scalar("SELECT count(*) FROM chunk_embeddings WHERE document_id = $1")
            .bind(did)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(emb, chunk_ids.len() as i64);
}

#[tokio::test]
async fn convert_simple_txt_sets_processing_and_span() {
    let _g = db_lock().await;
    let Ok(pool) = connect().await else {
        eprintln!("skip: postgres down");
        return;
    };
    reset_test_schema(&pool).await;
    let owner = Uuid::new_v4();
    insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
        .await
        .unwrap();
    let seeded = create_workspace_with_library(&pool, owner, "W", "w")
        .await
        .unwrap();
    let did = Uuid::new_v4();
    let hash = platform::sha256_hex(b"hello worker");
    write_blob(&hash, b"hello worker").unwrap();
    insert_document(
        &pool,
        knowledge::NewDocument {
            id: did,
            product_version_id: seeded.library_version_id,
            title: "a",
            file_name: "a.txt",
            file_size: 12,
            file_hash: &hash,
            object_ref: &format!("objects/{hash}"),
        },
    )
    .await
    .unwrap();
    convert_document(&pool, did, 1, &[], false).await.unwrap();
    let status: String = sqlx::query_scalar("SELECT parse_status FROM documents WHERE id = $1")
        .bind(did)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "processing");
    let span: String = sqlx::query_scalar(
        "SELECT status FROM document_processing_spans
             WHERE document_id = $1 AND name = 'docreader'",
    )
    .bind(did)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(span, "done");
    let stages: Vec<(String, String)> = sqlx::query_as(
        "SELECT name, status FROM document_processing_spans
             WHERE document_id = $1 AND kind = 'stage' ORDER BY name",
    )
    .bind(did)
    .fetch_all(&pool)
    .await
    .unwrap();
    let map: std::collections::HashMap<_, _> = stages.into_iter().collect();
    assert_eq!(map.get("docreader").map(String::as_str), Some("done"));
    assert_eq!(map.get("chunking").map(String::as_str), Some("done"));
    assert_eq!(map.get("embedding").map(String::as_str), Some("done"));
    assert_eq!(map.get("multimodal").map(String::as_str), Some("skipped"));
    assert_eq!(map.get("postprocess").map(String::as_str), Some("running"));
    assert!(platform::blob_exists(&format!("{hash}.md")));
    let enabled: String = sqlx::query_scalar("SELECT enable_status FROM documents WHERE id = $1")
        .bind(did)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(enabled, "enabled");
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM chunks WHERE document_id = $1")
        .bind(did)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(n >= 1, "convert must persist chunks");
    let en: i64 =
        sqlx::query_scalar("SELECT count(*) FROM chunk_embeddings WHERE document_id = $1")
            .bind(did)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(en, n);
}

#[tokio::test]
async fn convert_pdf_without_reader_fails_immediately() {
    let _g = db_lock().await;
    unsafe { std::env::remove_var("DOCREADER_ADDR") };
    let Ok(pool) = connect().await else {
        eprintln!("skip: postgres down");
        return;
    };
    reset_test_schema(&pool).await;
    let owner = Uuid::new_v4();
    insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
        .await
        .unwrap();
    let seeded = create_workspace_with_library(&pool, owner, "W2", "w2")
        .await
        .unwrap();
    let did = Uuid::new_v4();
    let hash = platform::sha256_hex(b"%PDF-1.4");
    write_blob(&hash, b"%PDF-1.4").unwrap();
    insert_document(
        &pool,
        knowledge::NewDocument {
            id: did,
            product_version_id: seeded.library_version_id,
            title: "p",
            file_name: "p.pdf",
            file_size: 8,
            file_hash: &hash,
            object_ref: &format!("objects/{hash}"),
        },
    )
    .await
    .unwrap();
    convert_document(&pool, did, 1, &[], false).await.unwrap();
    let (status, err): (String, String) = sqlx::query_as(
        "SELECT parse_status, COALESCE(error_message,'') FROM documents WHERE id = $1",
    )
    .bind(did)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(status, "failed");
    assert!(err.contains("DOCREADER_ADDR"), "{err}");
    let cancelled: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM document_processing_spans
             WHERE document_id = $1 AND status = 'cancelled'",
    )
    .bind(did)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        cancelled >= 1,
        "failed docreader must cancel dependent stages, got {cancelled}"
    );
    let post: Option<String> = sqlx::query_scalar(
        "SELECT status FROM document_processing_spans
             WHERE document_id = $1 AND name = 'postprocess'",
    )
    .bind(did)
    .fetch_optional(&pool)
    .await
    .unwrap();
    assert_ne!(post.as_deref(), Some("running"));
}

#[tokio::test]
async fn convert_markdown_with_image_enqueues_multimodal() {
    let _g = db_lock().await;
    let Ok(pool) = connect().await else {
        eprintln!("skip: postgres down");
        return;
    };
    reset_test_schema(&pool).await;
    let owner = Uuid::new_v4();
    insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
        .await
        .unwrap();
    let seeded = create_workspace_with_library(&pool, owner, "Mm", "mm")
        .await
        .unwrap();
    sqlx::query(
            "UPDATE product_versions SET image_processing_config = '{\"enable_multimodel\":true}'::jsonb
             WHERE id = $1",
        )
        .bind(seeded.library_version_id)
        .execute(&pool)
        .await
        .unwrap();
    let body = b"See ![p](images/p1.jpg) in the guide.";
    let did = Uuid::new_v4();
    let hash = platform::sha256_hex(body);
    write_blob(&hash, body).unwrap();
    insert_document(
        &pool,
        knowledge::NewDocument {
            id: did,
            product_version_id: seeded.library_version_id,
            title: "g",
            file_name: "g.md",
            file_size: body.len() as i64,
            file_hash: &hash,
            object_ref: &format!("objects/{hash}"),
        },
    )
    .await
    .unwrap();
    convert_document(&pool, did, 1, &[], false).await.unwrap();
    if platform::vlm_configured() {
        let Ok(storage) = platform::oxana_connect() else {
            eprintln!("skip: redis down");
            return;
        };
        let n = storage
            .enqueued_count(platform::MultimodalQueue)
            .await
            .unwrap();
        assert!(n >= 1, "image:multimodal must be enqueued, got {n}");
        assert_eq!(knowledge::enrichment::pending_count(did), Some(1));
        let _ = storage.wipe_queue(platform::MultimodalQueue).await;
    } else {
        let (status, err): (String, String) = sqlx::query_as(
            "SELECT parse_status, COALESCE(error_message,'') FROM documents WHERE id = $1",
        )
        .bind(did)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(status, "finalizing");
        assert!(err.contains("ocr_error"), "{err}");
        let ready: bool = sqlx::query_scalar("SELECT index_ready FROM documents WHERE id = $1")
            .bind(did)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(!ready, "images without VLM must not be searchable");
    }
}

#[tokio::test]
async fn convert_audio_without_asr_fails_immediately() {
    let _g = db_lock().await;
    let Ok(pool) = connect().await else {
        eprintln!("skip: postgres down");
        return;
    };
    reset_test_schema(&pool).await;
    let owner = Uuid::new_v4();
    insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
        .await
        .unwrap();
    let seeded = create_workspace_with_library(&pool, owner, "W3", "w3")
        .await
        .unwrap();
    let did = Uuid::new_v4();
    let hash = platform::sha256_hex(b"RIFF");
    write_blob(&hash, b"RIFF").unwrap();
    insert_document(
        &pool,
        knowledge::NewDocument {
            id: did,
            product_version_id: seeded.library_version_id,
            title: "a",
            file_name: "a.wav",
            file_size: 4,
            file_hash: &hash,
            object_ref: &format!("objects/{hash}"),
        },
    )
    .await
    .unwrap();
    convert_document(&pool, did, 1, &[], false).await.unwrap();
    let (status, err): (String, String) = sqlx::query_as(
        "SELECT parse_status, COALESCE(error_message,'') FROM documents WHERE id = $1",
    )
    .bind(did)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(status, "failed");
    assert!(err.contains("ASR"), "{err}");
}

#[tokio::test]
async fn convert_audio_stub_writes_markdown() {
    let _g = db_lock().await;
    let Ok(pool) = connect().await else {
        eprintln!("skip: postgres down");
        return;
    };
    reset_test_schema(&pool).await;
    let owner = Uuid::new_v4();
    insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
        .await
        .unwrap();
    let seeded = create_workspace_with_library(&pool, owner, "W4", "w4")
        .await
        .unwrap();
    sqlx::query(
        "UPDATE product_versions SET asr_model_id = 'stub-asr',
                asr_config = '{\"enabled\":true}'::jsonb WHERE id = $1",
    )
    .bind(seeded.library_version_id)
    .execute(&pool)
    .await
    .unwrap();
    let did = Uuid::new_v4();
    let bytes = b"RIFFWAVE";
    let hash = platform::sha256_hex(bytes);
    write_blob(&hash, bytes).unwrap();
    insert_document(
        &pool,
        knowledge::NewDocument {
            id: did,
            product_version_id: seeded.library_version_id,
            title: "a",
            file_name: "talk.wav",
            file_size: bytes.len() as i64,
            file_hash: &hash,
            object_ref: &format!("objects/{hash}"),
        },
    )
    .await
    .unwrap();
    convert_document(&pool, did, 1, &[], false).await.unwrap();
    let status: String = sqlx::query_scalar("SELECT parse_status FROM documents WHERE id = $1")
        .bind(did)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "processing");
    let md = String::from_utf8(platform::read_blob(&format!("{hash}.md")).unwrap()).unwrap();
    assert_eq!(md, "[stub-asr:talk.wav:8]");
}

#[test]
fn stored_url_blob_is_detected() {
    let (ok, url) = knowledge::ingest::parse_stored_url(b"url:https://docs.example/a.md");
    assert!(ok);
    assert_eq!(url, "https://docs.example/a.md");
    assert!(!knowledge::ingest::parse_stored_url(b"hello").0);
    assert!(!knowledge::ingest::parse_stored_url(b"url:ftp://x").0);
}

#[tokio::test]
async fn convert_passages_skips_reader_and_indexes_each() {
    let _g = db_lock().await;
    let Ok(pool) = connect().await else {
        eprintln!("skip: postgres down");
        return;
    };
    reset_test_schema(&pool).await;
    let owner = Uuid::new_v4();
    insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
        .await
        .unwrap();
    let seeded = create_workspace_with_library(&pool, owner, "Wp", "wp")
        .await
        .unwrap();
    let did = Uuid::new_v4();
    let hash = platform::sha256_hex(b"unused");
    insert_document(
        &pool,
        knowledge::NewDocument {
            id: did,
            product_version_id: seeded.library_version_id,
            title: "p",
            file_name: "p.txt",
            file_size: 6,
            file_hash: &hash,
            object_ref: &format!("objects/{hash}"),
        },
    )
    .await
    .unwrap();
    convert_document(
        &pool,
        did,
        1,
        &["first passage".into(), "second passage".into()],
        false,
    )
    .await
    .unwrap();
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM chunks WHERE document_id = $1")
        .bind(did)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 2);
    let texts: Vec<String> =
        sqlx::query_scalar("SELECT content FROM chunks WHERE document_id = $1 ORDER BY content")
            .bind(did)
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(texts, vec!["first passage", "second passage"]);
    knowledge::set_document_source(
        &pool,
        did,
        "passage",
        &["first passage".into(), "second passage".into()],
    )
    .await
    .unwrap();
    let attempt = knowledge::mark_reparse_queued(&pool, did).await.unwrap();
    process_reparse_pg(&pool, did, attempt).await.unwrap();
    process_reparse_pg(&pool, did, attempt).await.unwrap();
    let replay_attempt = knowledge::mark_reparse_queued(&pool, did).await.unwrap();
    assert_eq!(replay_attempt, attempt);
    let kind: String = sqlx::query_scalar("SELECT type FROM documents WHERE id = $1")
        .bind(did)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(kind, "passage");
    let attempt: i32 = sqlx::query_scalar("SELECT attempt FROM documents WHERE id = $1")
        .bind(did)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(attempt, replay_attempt);
}

#[tokio::test]
async fn convert_url_without_reader_fails_immediately() {
    let _g = db_lock().await;
    unsafe { std::env::remove_var("DOCREADER_ADDR") };
    let Ok(pool) = connect().await else {
        eprintln!("skip: postgres down");
        return;
    };
    reset_test_schema(&pool).await;
    let owner = Uuid::new_v4();
    insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
        .await
        .unwrap();
    let seeded = create_workspace_with_library(&pool, owner, "Wu", "wu")
        .await
        .unwrap();
    let body = b"url:https://example.com/doc";
    let did = Uuid::new_v4();
    let hash = platform::sha256_hex(body);
    write_blob(&hash, body).unwrap();
    insert_document(
        &pool,
        knowledge::NewDocument {
            id: did,
            product_version_id: seeded.library_version_id,
            title: "u",
            file_name: "remote.md",
            file_size: body.len() as i64,
            file_hash: &hash,
            object_ref: &format!("objects/{hash}"),
        },
    )
    .await
    .unwrap();
    convert_document(&pool, did, 1, &[], false).await.unwrap();
    let (status, err): (String, String) = sqlx::query_as(
        "SELECT parse_status, COALESCE(error_message,'') FROM documents WHERE id = $1",
    )
    .bind(did)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(status, "failed");
    assert!(err.contains("DOCREADER_ADDR"), "{err}");
}

#[tokio::test]
async fn wiki_ingest_job_is_direct_idempotent_and_finalizes() {
    let _g = db_lock().await;
    let Ok(pool) = connect().await else {
        eprintln!("skip: postgres down");
        return;
    };
    reset_test_schema(&pool).await;
    let owner = Uuid::new_v4();
    insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
        .await
        .unwrap();
    let seeded = create_workspace_with_library(&pool, owner, "Ww", "ww")
        .await
        .unwrap();
    let vid = seeded.library_version_id;
    let did = Uuid::new_v4();
    let hash = platform::sha256_hex(b"wiki body");
    write_blob(&hash, b"wiki body").unwrap();
    insert_document(
        &pool,
        knowledge::NewDocument {
            id: did,
            product_version_id: vid,
            title: "w",
            file_name: "w.txt",
            file_size: 9,
            file_hash: &hash,
            object_ref: &format!("objects/{hash}"),
        },
    )
    .await
    .unwrap();
    sqlx::query(
        "UPDATE documents SET parse_status = 'finalizing', pending_subtasks_count = 1
             WHERE id = $1",
    )
    .bind(did)
    .execute(&pool)
    .await
    .unwrap();
    let cid = Uuid::new_v4();
    knowledge::replace_document_chunks(
        &pool,
        did,
        &[knowledge::Chunk {
            id: cid,
            document_id: did,
            product_version_id: vid,
            chunk_type: "text".into(),
            content: "wiki body about the product".into(),
            context_header: String::new(),
            start_at: 0,
            end_at: 27,
            parent_chunk_id: None,
            generated_questions: vec![],
        }],
        &[],
    )
    .await
    .unwrap();
    if let Err(error) = process_wiki_ingest(&pool, vid, did, knowledge::wiki::OP_INGEST).await {
        assert!(error.contains("Oxana Redis is not configured"), "{error}");
    }
    process_wiki_finalize(&pool, vid, did).await.unwrap();
    let postgres_queue_tables: bool = sqlx::query_scalar(
        "SELECT to_regclass('public.task_pending_ops') IS NOT NULL
                 OR to_regclass('public.task_dead_letters') IS NOT NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!postgres_queue_tables);
    let (status, pending): (String, i32) =
        sqlx::query_as("SELECT parse_status, pending_subtasks_count FROM documents WHERE id = $1")
            .bind(did)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(pending, 0);
    assert_eq!(status, "completed");
    let span: String = sqlx::query_scalar(
        "SELECT status FROM document_processing_spans
             WHERE document_id = $1 AND name = 'wiki.ingest'",
    )
    .bind(did)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(span, "done");
    let pages: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM wiki_pages WHERE product_version_id = $1 AND status = 'published'",
    )
    .bind(vid)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(pages >= 1, "wiki page persisted");
    let wiki_chunks: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM chunks WHERE document_id = $1 AND chunk_type = 'wiki_page'",
    )
    .bind(did)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(wiki_chunks >= 1, "wiki_page chunk persisted");
    let original_page: (Uuid, String, serde_json::Value) = sqlx::query_as(
        "SELECT id,content,source_refs FROM wiki_pages WHERE product_version_id=$1
               AND source_refs @> jsonb_build_array($2::text) ORDER BY slug LIMIT 1",
    )
    .bind(vid)
    .bind(did.to_string())
    .fetch_one(&pool)
    .await
    .unwrap();

    let mut concurrent_ids = Vec::new();
    for suffix in ["two", "three"] {
        let document_id = Uuid::new_v4();
        concurrent_ids.push(document_id);
        insert_document(
            &pool,
            knowledge::NewDocument {
                id: document_id,
                product_version_id: vid,
                title: "w",
                file_name: &format!("w-{suffix}.txt"),
                file_size: 9,
                file_hash: &hash,
                object_ref: &format!("objects/{hash}"),
            },
        )
        .await
        .unwrap();
        sqlx::query(
            "UPDATE documents SET parse_status='finalizing', pending_subtasks_count=1 WHERE id=$1",
        )
        .bind(document_id)
        .execute(&pool)
        .await
        .unwrap();
        let chunk_id = Uuid::new_v4();
        knowledge::replace_document_chunks(
            &pool,
            document_id,
            &[knowledge::Chunk {
                id: chunk_id,
                document_id,
                product_version_id: vid,
                chunk_type: "text".into(),
                content: "wiki body about the product".into(),
                context_header: String::new(),
                start_at: 0,
                end_at: 27,
                parent_chunk_id: None,
                generated_questions: vec![],
            }],
            &[],
        )
        .await
        .unwrap();
    }
    let left_pool = pool.clone();
    let right_pool = pool.clone();
    let left = concurrent_ids[0];
    let right = concurrent_ids[1];
    let (left_result, right_result) = tokio::join!(
        process_wiki_ingest(&left_pool, vid, left, knowledge::wiki::OP_INGEST),
        process_wiki_ingest(&right_pool, vid, right, knowledge::wiki::OP_INGEST),
    );
    for result in [left_result, right_result] {
        if let Err(error) = result {
            assert!(error.contains("Oxana Redis is not configured"), "{error}");
        }
    }
    for document_id in [did, left, right] {
        let owned_page: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM wiki_pages WHERE product_version_id=$1
                   AND source_refs @> jsonb_build_array($2::text))",
        )
        .bind(vid)
        .bind(document_id.to_string())
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(
            owned_page,
            "each concurrent document must retain source ownership"
        );
    }
    let original_page_after: (Uuid, String, serde_json::Value) =
        sqlx::query_as("SELECT id,content,source_refs FROM wiki_pages WHERE id=$1")
            .bind(original_page.0)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        original_page, original_page_after,
        "a second document job must not rewrite an unrelated page"
    );

    process_wiki_ingest(&pool, vid, left, knowledge::wiki::OP_RETRACT)
        .await
        .unwrap_or_else(|error| {
            assert!(error.contains("Oxana Redis is not configured"), "{error}");
        });
    let retracted_source_remains: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM wiki_pages WHERE product_version_id=$1
               AND source_refs @> jsonb_build_array($2::text))",
    )
    .bind(vid)
    .bind(left.to_string())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!retracted_source_remains);
    let survivor_is_searchable: bool = sqlx::query_scalar(
        "SELECT EXISTS(
                SELECT 1 FROM wiki_pages page
                JOIN chunks chunk ON chunk.product_version_id=page.product_version_id
                    AND chunk.chunk_type='wiki_page' AND chunk.context_header=page.slug
                WHERE page.product_version_id=$1
                  AND page.source_refs @> jsonb_build_array($2::text)
                  AND page.content<>'' AND chunk.content<>''
             )",
    )
    .bind(vid)
    .bind(right.to_string())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(survivor_is_searchable);

    let before_failure: (i64, i64, i64) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM wiki_pages WHERE product_version_id=$1),
                    (SELECT count(*) FROM wiki_folders WHERE product_version_id=$1),
                    (SELECT count(*) FROM chunks WHERE product_version_id=$1 AND chunk_type='wiki_page')",
        )
        .bind(vid)
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("CREATE FUNCTION kb_test_reject_wiki_write() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'forced wiki write failure'; END $$")
            .execute(&pool).await.unwrap();
    sqlx::query("CREATE TRIGGER kb_test_reject_wiki_write BEFORE INSERT OR UPDATE OR DELETE ON wiki_pages FOR EACH ROW EXECUTE FUNCTION kb_test_reject_wiki_write()")
            .execute(&pool).await.unwrap();
    let pages = knowledge::list_wiki_pages(&pool, vid).await.unwrap();
    let mut changed_page = pages
        .into_iter()
        .find(|page| page.product_version_id == vid && page.source_refs.contains(&right))
        .unwrap();
    changed_page.content.push_str(" forced change");
    let folder_rows = sqlx::query(
        "SELECT id, product_version_id, parent_id, name, path, depth, sort_order
             FROM wiki_folders
             WHERE product_version_id = $1 AND deleted_at IS NULL",
    )
    .bind(vid)
    .fetch_all(&pool)
    .await
    .unwrap();
    let changed_folders = folder_rows
        .into_iter()
        .map(|row| knowledge::WikiFolder {
            id: row.get("id"),
            product_version_id: row.get("product_version_id"),
            parent_id: row.get("parent_id"),
            name: row.get("name"),
            path: row.get("path"),
            depth: row.get("depth"),
            sort_order: row.get("sort_order"),
        })
        .collect::<Vec<_>>();
    assert!(
        knowledge::persist_wiki_changes_atomic(
            &pool,
            vid,
            &[changed_page],
            &[],
            &changed_folders,
            &[],
            &[],
            &[],
            &[],
        )
        .await
        .is_err()
    );
    sqlx::query("DROP TRIGGER kb_test_reject_wiki_write ON wiki_pages")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DROP FUNCTION kb_test_reject_wiki_write()")
        .execute(&pool)
        .await
        .unwrap();
    let after_failure: (i64, i64, i64) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM wiki_pages WHERE product_version_id=$1),
                    (SELECT count(*) FROM wiki_folders WHERE product_version_id=$1),
                    (SELECT count(*) FROM chunks WHERE product_version_id=$1 AND chunk_type='wiki_page')",
        )
        .bind(vid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        before_failure, after_failure,
        "Wiki publication must roll back atomically"
    );
}

#[tokio::test]
async fn wiki_disabled_skips_without_error() {
    let _g = db_lock().await;
    let Ok(pool) = connect().await else {
        eprintln!("skip: postgres down");
        return;
    };
    reset_test_schema(&pool).await;
    let owner = Uuid::new_v4();
    insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
        .await
        .unwrap();
    let seeded = create_workspace_with_library(&pool, owner, "Wn", "wn")
        .await
        .unwrap();
    sqlx::query(
        "UPDATE product_versions SET indexing_strategy = '{\"wiki\":false}'::jsonb WHERE id = $1",
    )
    .bind(seeded.library_version_id)
    .execute(&pool)
    .await
    .unwrap();
    process_wiki_ingest(
        &pool,
        seeded.library_version_id,
        Uuid::new_v4(),
        knowledge::wiki::OP_INGEST,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn version_clone_worker_copies_doc_and_sets_active() {
    let _g = db_lock().await;
    let Ok(pool) = connect().await else {
        eprintln!("skip: postgres down");
        return;
    };
    reset_test_schema(&pool).await;
    let owner = Uuid::new_v4();
    insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
        .await
        .unwrap();
    let seeded = create_workspace_with_library(&pool, owner, "Wc", "wc")
        .await
        .unwrap();
    let src = seeded.library_version_id;
    let src_doc = Uuid::new_v4();
    insert_document(
        &pool,
        knowledge::NewDocument {
            id: src_doc,
            product_version_id: src,
            title: "iso",
            file_name: "iso.txt",
            file_size: 3,
            file_hash: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            object_ref: "objects/ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        },
    )
    .await
    .unwrap();
    let dst = Uuid::new_v4();
    knowledge::insert_version_cloning(&pool, dst, seeded.library_id, "2026", src)
        .await
        .unwrap();
    process_version_clone(
        &pool,
        &VersionCloneJob {
            source_version_id: src,
            target_version_id: dst,
            diffs: serde_json::json!([]),
            make_current: false,
            task_type: platform::TYPE_VERSION_CLONE.into(),
        },
    )
    .await
    .unwrap();
    let src_n: i64 =
        sqlx::query_scalar("SELECT count(*) FROM documents WHERE product_version_id = $1")
            .bind(src)
            .fetch_one(&pool)
            .await
            .unwrap();
    let dst_n: i64 =
        sqlx::query_scalar("SELECT count(*) FROM documents WHERE product_version_id = $1")
            .bind(dst)
            .fetch_one(&pool)
            .await
            .unwrap();
    let status: String = sqlx::query_scalar("SELECT status FROM product_versions WHERE id = $1")
        .bind(dst)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(src_n, 1);
    assert_eq!(dst_n, 1);
    assert_eq!(status, "active");
    let dst_id: Uuid = sqlx::query_scalar("SELECT id FROM documents WHERE product_version_id = $1")
        .bind(dst)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_ne!(dst_id, src_doc);
}

#[tokio::test]
async fn worker_shutdown_state_is_persistent() {
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    stop_tx.send(true).unwrap();

    tokio::time::timeout(
        std::time::Duration::from_millis(100),
        wait_for_worker_shutdown(stop_rx),
    )
    .await
    .expect("a receiver created before shutdown must observe the persisted stop state");
}

#[tokio::test]
async fn process_post_process_clone_keep_requires_typed_wiki_delivery() {
    let _g = db_lock().await;
    let Ok(pool) = connect().await else {
        eprintln!("skip: postgres down");
        return;
    };
    reset_test_schema(&pool).await;
    let owner = Uuid::new_v4();
    insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
        .await
        .unwrap();
    let seeded = create_workspace_with_library(&pool, owner, "Pp", "pp")
        .await
        .unwrap();
    let did = Uuid::new_v4();
    insert_document(
        &pool,
        knowledge::NewDocument {
            id: did,
            product_version_id: seeded.library_version_id,
            title: "keep",
            file_name: "keep.txt",
            file_size: 8,
            file_hash: "930a443e0bc8b34f4fdba1201cf2e2a4d551d226d65270c47ef56e3256e8b3e9",
            object_ref: "objects/930a443e0bc8b34f4fdba1201cf2e2a4d551d226d65270c47ef56e3256e8b3e9",
        },
    )
    .await
    .unwrap();
    sqlx::query(
        "UPDATE documents SET parse_status = 'processing', enable_status = 'enabled'
             WHERE id = $1",
    )
    .bind(did)
    .execute(&pool)
    .await
    .unwrap();
    let cid = Uuid::new_v4();
    knowledge::replace_document_chunks(
        &pool,
        did,
        &[knowledge::Chunk {
            id: cid,
            document_id: did,
            product_version_id: seeded.library_version_id,
            chunk_type: "text".into(),
            content: "throughput keep".into(),
            context_header: String::new(),
            start_at: 0,
            end_at: 15,
            parent_chunk_id: None,
            generated_questions: vec![],
        }],
        &[knowledge::ChunkEmbedding {
            chunk_id: cid,
            product_version_id: seeded.library_version_id,
            document_id: did,
            content: "throughput keep".into(),
            vector: vec![0.1; knowledge::models::EMBEDDING_DIM],
            tsv: String::new(),
        }],
    )
    .await
    .unwrap();
    if let Err(error) = process_post_process(&pool, did, seeded.library_version_id, true).await {
        assert!(error.contains("Oxana Redis is not configured"), "{error}");
    }
    let status: String = sqlx::query_scalar("SELECT parse_status FROM documents WHERE id = $1")
        .bind(did)
        .fetch_one(&pool)
        .await
        .unwrap();
    let pending: i32 =
        sqlx::query_scalar("SELECT pending_subtasks_count FROM documents WHERE id = $1")
            .bind(did)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "finalizing");
    assert!(
        pending >= 1,
        "typed Wiki work must remain counted until its Oxana job settles"
    );
}

#[tokio::test]
async fn process_post_process_writes_summary_and_keeps_question_payload_closed() {
    let _g = db_lock().await;
    let Ok(pool) = connect().await else {
        eprintln!("skip: postgres down");
        return;
    };
    reset_test_schema(&pool).await;
    let owner = Uuid::new_v4();
    insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
        .await
        .unwrap();
    let seeded = create_workspace_with_library(&pool, owner, "Sm", "sm")
        .await
        .unwrap();
    sqlx::query("UPDATE product_versions SET summary_model_id = 'stub-chat' WHERE id = $1")
        .bind(seeded.library_version_id)
        .execute(&pool)
        .await
        .unwrap();
    let did = Uuid::new_v4();
    insert_document(
        &pool,
        knowledge::NewDocument {
            id: did,
            product_version_id: seeded.library_version_id,
            title: "spec",
            file_name: "spec.txt",
            file_size: 80,
            file_hash: "bb558b4638d76b2461f5cdeca98bc8b4ba29b652cfa1ca7662c82d15fd171063",
            object_ref: "objects/bb558b4638d76b2461f5cdeca98bc8b4ba29b652cfa1ca7662c82d15fd171063",
        },
    )
    .await
    .unwrap();
    sqlx::query(
        "UPDATE documents SET parse_status = 'processing', enable_status = 'enabled'
             WHERE id = $1",
    )
    .bind(did)
    .execute(&pool)
    .await
    .unwrap();
    let body = "The product delivers forty gigabit throughput on the line card. \
                    Operators use this guide to install the switch in a rack and verify ISO9001.";
    let cid = Uuid::new_v4();
    knowledge::replace_document_chunks(
        &pool,
        did,
        &[knowledge::Chunk {
            id: cid,
            document_id: did,
            product_version_id: seeded.library_version_id,
            chunk_type: "text".into(),
            content: body.into(),
            context_header: String::new(),
            start_at: 0,
            end_at: body.len() as i32,
            parent_chunk_id: None,
            generated_questions: vec![],
        }],
        &[knowledge::ChunkEmbedding {
            chunk_id: cid,
            product_version_id: seeded.library_version_id,
            document_id: did,
            content: body.into(),
            vector: vec![0.1; knowledge::models::EMBEDDING_DIM],
            tsv: String::new(),
        }],
    )
    .await
    .unwrap();
    let image = process_image_pg(&pool, did, "images/p1.jpg", "scanned_pdf", true, true, 1).await;
    if platform::vlm_configured() {
        image.unwrap();
        let text_n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM chunks WHERE document_id = $1 AND chunk_type = 'text'",
        )
        .bind(did)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(text_n, 1, "multimodal append must keep text chunks");
        let ocr_n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM chunks WHERE document_id = $1 AND chunk_type = 'image_ocr'",
        )
        .bind(did)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(ocr_n >= 1, "multimodal OCR chunk persisted");
    } else {
        assert!(image.is_err(), "no VLM must not stub OCR chunks");
    }

    process_post_process(&pool, did, seeded.library_version_id, false)
        .await
        .unwrap();
    let _ = process_summary_pg(&pool, did, 1, false).await;
    let _ = knowledge::pipeline::run_questions(&pool, did, &[cid], &[], &[], 1).await;
    let summaries: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM chunks WHERE document_id = $1 AND chunk_type = 'summary'",
    )
    .bind(did)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(summaries >= 1, "summary chunk persisted");
    let qs: serde_json::Value =
        sqlx::query_scalar("SELECT generated_questions FROM chunks WHERE id = $1")
            .bind(cid)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        qs.as_array().is_some(),
        "generated_questions must remain a closed array: {qs}"
    );
}

#[tokio::test]
async fn semantic_index_v2_business_lifecycle_is_fenced() {
    use knowledge::knowledge_index_v2::SemanticIndexPreparationV2;
    use knowledge::knowledge_retrieval::{
        EMBEDDING_DIMENSION_V2, EMBEDDING_OUTPUT_NORMALIZATION_VERSION_V2,
        EMBEDDING_PROVIDER_PROTOCOL_VERSION_V2, EMBEDDING_REVISION_SCHEMA_V2, EmbeddingRevisionV2,
    };

    let _g = db_lock().await;
    let pool = match connect().await {
        Ok(pool) => pool,
        Err(error)
            if std::env::var("KNOWLEDGEBRAIN_REQUIRE_POSTGRES_TESTS").as_deref() == Ok("1") =>
        {
            panic!("required semantic-index V2 PostgreSQL test unavailable: {error}")
        }
        Err(error) => {
            eprintln!("skip: postgres down: {error}");
            return;
        }
    };
    reset_test_schema(&pool).await;
    let schema_ready: bool = sqlx::query_scalar(
        "SELECT to_regprocedure('kb_knowledge_prepare_semantic_index_intent_v2(uuid)') IS NOT NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap_or(false);
    if !schema_ready {
        if std::env::var("KNOWLEDGEBRAIN_REQUIRE_POSTGRES_TESTS").as_deref() == Ok("1") {
            panic!("required semantic-index V2 schema is unavailable");
        }
        eprintln!("skip: semantic-index V2 schema unavailable");
        return;
    }

    let workspace_id = Uuid::new_v4();
    let product_id = Uuid::new_v4();
    let version_id = Uuid::new_v4();
    let document_id = Uuid::new_v4();
    let chunk_id = Uuid::new_v4();
    let file_hash = platform::sha256_hex(document_id.as_bytes());
    let object_ref = format!("objects/{file_hash}");
    let revision = EmbeddingRevisionV2 {
        schema_version: EMBEDDING_REVISION_SCHEMA_V2,
        provider_protocol_version: EMBEDDING_PROVIDER_PROTOCOL_VERSION_V2.into(),
        provider_model_identifier: format!("lifecycle-v2-{version_id}@2025-01-15"),
        provider_model_revision_sha256: platform::sha256_hex(b"lifecycle-v2-model"),
        endpoint_config_sha256: platform::sha256_hex(b"lifecycle-v2-endpoint"),
        endpoint_identity: "https://embeddings.example.test/v1/embeddings".into(),
        dimension: EMBEDDING_DIMENSION_V2,
        request_config_sha256: EmbeddingRevisionV2::canonical_request_config_sha256(),
        output_normalization_version: EMBEDDING_OUTPUT_NORMALIZATION_VERSION_V2.into(),
    };
    let revision_sha256 = revision.sha256().unwrap();

    sqlx::query("INSERT INTO workspaces(id,name,slug,kind) VALUES($1,'semantic lifecycle',$2,'product_line')")
            .bind(workspace_id)
            .bind(format!("semantic-lifecycle-{workspace_id}"))
            .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO products(id,workspace_id,kind,name,slug) VALUES($1,$2,'product','semantic lifecycle',$3)")
            .bind(product_id).bind(workspace_id)
            .bind(format!("semantic-lifecycle-{product_id}"))
            .execute(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO product_versions(id,product_id,label,status) VALUES($1,$2,'v2','active')",
    )
    .bind(version_id)
    .bind(product_id)
    .execute(&pool)
    .await
    .unwrap();

    let unbound =
        knowledge::knowledge_index_v2::prepare_semantic_index_intent_v2(&pool, version_id)
            .await
            .unwrap();
    assert_eq!(unbound, SemanticIndexPreparationV2::Unbound);

    sqlx::query("INSERT INTO embedding_revisions_v2(revision_sha256,canonical_revision_payload,schema_version,provider_protocol_version,provider_model_identifier,provider_model_revision_sha256,endpoint_config_sha256,endpoint_identity,dimension,request_config_sha256,output_normalization_version,credential_ref) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,'env:KNOWLEDGEBRAIN_TEST_MISSING_SEMANTIC_V2')")
            .bind(&revision_sha256)
            .bind(revision.canonical_bytes().unwrap())
            .bind(i16::try_from(revision.schema_version).unwrap())
            .bind(&revision.provider_protocol_version)
            .bind(&revision.provider_model_identifier)
            .bind(&revision.provider_model_revision_sha256)
            .bind(&revision.endpoint_config_sha256)
            .bind(&revision.endpoint_identity)
            .bind(i32::try_from(revision.dimension).unwrap())
            .bind(&revision.request_config_sha256)
            .bind(&revision.output_normalization_version)
            .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO product_version_embedding_bindings_v2(product_version_id,embedding_revision_sha256) VALUES($1,$2)")
            .bind(version_id).bind(&revision_sha256).execute(&pool).await.unwrap();

    let empty_intent =
        match knowledge::knowledge_index_v2::prepare_semantic_index_intent_v2(&pool, version_id)
            .await
            .unwrap()
        {
            SemanticIndexPreparationV2::Enqueue(intent) => intent,
            other => panic!("expected empty bound intent, got {other:?}"),
        };
    let empty_provider = LifecycleProvider {
        calls: AtomicUsize::new(0),
        results: std::sync::Mutex::new(VecDeque::new()),
    };
    process_semantic_index_intent_v2(
        &pool,
        empty_intent.id,
        empty_intent.target_revision,
        Some(&empty_provider),
        None,
    )
    .await
    .unwrap();
    assert_eq!(empty_provider.calls.load(Ordering::SeqCst), 0);
    let empty_completed = knowledge::knowledge_index_v2::semantic_index_intent_v2(
        &pool,
        empty_intent.id,
        empty_intent.target_revision,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(empty_completed.status, "completed");
    for statement in [
        "UPDATE knowledge_semantic_index_intents_v2 SET source_snapshot_sha256=repeat('0',64) WHERE id=$1",
        "DELETE FROM knowledge_semantic_index_intents_v2 WHERE id=$1",
    ] {
        let immutable_error = sqlx::query(statement)
            .bind(empty_intent.id)
            .execute(&pool)
            .await
            .unwrap_err();
        assert!(
            immutable_error
                .as_database_error()
                .is_some_and(|error| error
                    .message()
                    .contains("KNOWLEDGE_SEMANTIC_INDEX_INTENT_V2_IMMUTABLE")),
            "intent identity/history must be immutable: {immutable_error}"
        );
    }

    sqlx::query("INSERT INTO object_registry(object_ref,digest,media_type,byte_length,state) VALUES($1,$2,'text/plain',0,'available')")
            .bind(&object_ref).bind(&file_hash).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO object_owner_references(object_ref,owner_kind,owner_id,occurrence,created_by) VALUES($1,'knowledge_document',$2,'original','system:knowledge-document-ingest')")
            .bind(&object_ref).bind(document_id).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO documents(id,product_version_id,title,parse_status,pending_subtasks_count,summary_status,enable_status,index_ready,file_name,file_size,file_hash,object_ref) VALUES($1,$2,'lifecycle','finalizing',1,'pending','enabled',true,$3,0,$4,$5)")
            .bind(document_id).bind(version_id).bind(format!("{document_id}.txt"))
            .bind(&file_hash).bind(&object_ref).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO chunks(id,product_version_id,document_id,chunk_type,content,context_header) VALUES($1,$2,$3,'text','settled source','# lifecycle')")
            .bind(chunk_id).bind(version_id).bind(document_id).execute(&pool).await.unwrap();

    assert_eq!(
        knowledge::knowledge_index_v2::prepare_semantic_index_intent_v2(&pool, version_id)
            .await
            .unwrap(),
        SemanticIndexPreparationV2::PendingDerived
    );
    sqlx::query("UPDATE documents SET parse_status='completed',pending_subtasks_count=0,summary_status='completed' WHERE id=$1")
            .bind(document_id).execute(&pool).await.unwrap();
    let source_intent =
        match knowledge::knowledge_index_v2::prepare_semantic_index_intent_v2(&pool, version_id)
            .await
            .unwrap()
        {
            SemanticIndexPreparationV2::Enqueue(intent) => intent,
            other => panic!("expected settled source intent, got {other:?}"),
        };
    let scheduled_targets = Arc::new(std::sync::Mutex::new(Vec::new()));
    let first_calls = scheduled_targets.clone();
    assert!(
        knowledge::pipeline::schedule_semantic_index_v2_if_ready_with(
            &pool,
            version_id,
            move |id, revision| {
                let first_calls = first_calls.clone();
                async move {
                    first_calls.lock().unwrap().push((id, revision));
                    Ok(None)
                }
            }
        )
        .await
        .is_err(),
        "queue unavailability must remain an Oxana-retryable parent error"
    );
    let second_calls = scheduled_targets.clone();
    knowledge::pipeline::schedule_semantic_index_v2_if_ready_with(
        &pool,
        version_id,
        move |id, revision| {
            let second_calls = second_calls.clone();
            async move {
                second_calls.lock().unwrap().push((id, revision));
                Ok(Some("accepted".into()))
            }
        },
    )
    .await
    .unwrap();
    assert_eq!(
        scheduled_targets.lock().unwrap().as_slice(),
        &[
            (source_intent.id, source_intent.target_revision),
            (source_intent.id, source_intent.target_revision),
        ],
        "parent retry must replay the same unique business target"
    );
    assert_eq!(
        knowledge::document_parse_status(&pool, document_id)
            .await
            .unwrap()
            .as_deref(),
        Some("completed"),
        "V2 enqueue failure must not rewrite completed V1 status"
    );

    let unavailable_provider = LifecycleProvider {
        calls: AtomicUsize::new(0),
        results: std::sync::Mutex::new(VecDeque::from([
            LifecycleProviderResult::Unavailable,
            LifecycleProviderResult::Unavailable,
            LifecycleProviderResult::Unavailable,
            LifecycleProviderResult::Unavailable,
        ])),
    };
    for _ in 0..=platform::SEMANTIC_INDEX_V2_MAX_RETRY {
        assert!(
            process_semantic_index_intent_v2(
                &pool,
                source_intent.id,
                source_intent.target_revision,
                Some(&unavailable_provider),
                None,
            )
            .await
            .is_err()
        );
    }
    let retryable = knowledge::knowledge_index_v2::semantic_index_intent_v2(
        &pool,
        source_intent.id,
        source_intent.target_revision,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(retryable.status, "pending");
    assert_eq!(
        retryable.last_error_code.as_deref(),
        Some("PROVIDER_UNAVAILABLE")
    );

    assert_eq!(unavailable_provider.calls.load(Ordering::SeqCst), 4);
    // A fresh provider instance models native dead-job revival: the
    // business target remained pending and the same envelope can run again.
    let restarted_provider = LifecycleProvider {
        calls: AtomicUsize::new(0),
        results: std::sync::Mutex::new(VecDeque::from([LifecycleProviderResult::Success])),
    };
    process_semantic_index_intent_v2(
        &pool,
        source_intent.id,
        source_intent.target_revision,
        Some(&restarted_provider),
        None,
    )
    .await
    .unwrap();
    assert_eq!(restarted_provider.calls.load(Ordering::SeqCst), 1);
    let ready = knowledge::knowledge_index_v2::semantic_index_intent_v2(
        &pool,
        source_intent.id,
        source_intent.target_revision,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(ready.status, "completed");
    assert_eq!(
        ready.generation_marker_sha256.as_deref(),
        Some(source_intent.source_snapshot_sha256.as_str())
    );
    let complete_generation: bool = sqlx::query_scalar(
            "SELECT EXISTS(
               SELECT 1
                 FROM product_version_keyword_index_generations_v2 keyword_generation
                 JOIN product_version_vector_index_generations_v2 vector_generation
                   ON vector_generation.product_version_id=keyword_generation.product_version_id
                  AND vector_generation.embedding_revision_sha256=keyword_generation.embedding_revision_sha256
                  AND vector_generation.source_snapshot_sha256=keyword_generation.source_snapshot_sha256
                 JOIN knowledge_semantic_index_intents_v2 intent
                   ON intent.product_version_id=keyword_generation.product_version_id
                  AND intent.embedding_revision_sha256=keyword_generation.embedding_revision_sha256
                  AND intent.source_snapshot_sha256=keyword_generation.source_snapshot_sha256
                  AND intent.status='completed'
                  AND intent.generation_marker_sha256=intent.source_snapshot_sha256
                WHERE keyword_generation.product_version_id=$1
                  AND keyword_generation.source_snapshot_sha256=$2)",
        )
        .bind(version_id)
        .bind(&source_intent.source_snapshot_sha256)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(complete_generation);
    process_semantic_index_intent_v2(
        &pool,
        source_intent.id,
        source_intent.target_revision,
        Some(&restarted_provider),
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        restarted_provider.calls.load(Ordering::SeqCst),
        1,
        "duplicate delivery must noop"
    );
    let v1_ready: bool = sqlx::query_scalar("SELECT index_ready FROM documents WHERE id=$1")
        .bind(document_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(v1_ready, "V1 document readiness must remain unchanged");

    sqlx::query("UPDATE chunks SET content='aba generation b' WHERE id=$1")
        .bind(chunk_id)
        .execute(&pool)
        .await
        .unwrap();
    let aba_b =
        match knowledge::knowledge_index_v2::prepare_semantic_index_intent_v2(&pool, version_id)
            .await
            .unwrap()
        {
            SemanticIndexPreparationV2::Enqueue(intent) => intent,
            other => panic!("expected ABA generation B target, got {other:?}"),
        };
    let aba_b_provider = LifecycleProvider {
        calls: AtomicUsize::new(0),
        results: std::sync::Mutex::new(VecDeque::new()),
    };
    process_semantic_index_intent_v2(
        &pool,
        aba_b.id,
        aba_b.target_revision,
        Some(&aba_b_provider),
        None,
    )
    .await
    .unwrap();
    assert_eq!(aba_b_provider.calls.load(Ordering::SeqCst), 1);

    sqlx::query("UPDATE chunks SET content='settled source' WHERE id=$1")
        .bind(chunk_id)
        .execute(&pool)
        .await
        .unwrap();
    let aba_a =
        match knowledge::knowledge_index_v2::prepare_semantic_index_intent_v2(&pool, version_id)
            .await
            .unwrap()
        {
            SemanticIndexPreparationV2::Enqueue(intent) => intent,
            other => panic!("expected a new ABA generation A target, got {other:?}"),
        };
    assert_eq!(
        aba_a.source_snapshot_sha256,
        source_intent.source_snapshot_sha256
    );
    assert!(aba_a.target_revision > aba_b.target_revision);
    assert_ne!(aba_a.id, source_intent.id);

    let pending_after_provider = PendingAfterLifecycleProvider {
        pool: pool.clone(),
        document_id,
        calls: AtomicUsize::new(0),
    };
    assert!(
        process_semantic_index_intent_v2(
            &pool,
            aba_a.id,
            aba_a.target_revision,
            Some(&pending_after_provider),
            None,
        )
        .await
        .is_err(),
        "pending derived work introduced after provider I/O must fence publication"
    );
    assert_eq!(pending_after_provider.calls.load(Ordering::SeqCst), 1);
    let aba_pending = knowledge::knowledge_index_v2::semantic_index_intent_v2(
        &pool,
        aba_a.id,
        aba_a.target_revision,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(aba_pending.status, "pending");
    sqlx::query("UPDATE documents SET pending_subtasks_count=0 WHERE id=$1")
        .bind(document_id)
        .execute(&pool)
        .await
        .unwrap();
    let aba_retry_provider = LifecycleProvider {
        calls: AtomicUsize::new(0),
        results: std::sync::Mutex::new(VecDeque::new()),
    };
    process_semantic_index_intent_v2(
        &pool,
        aba_a.id,
        aba_a.target_revision,
        Some(&aba_retry_provider),
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        aba_retry_provider.calls.load(Ordering::SeqCst),
        0,
        "the already fenced vector generation is reused without duplicate provider I/O"
    );

    let prior_marker: String = sqlx::query_scalar("SELECT source_snapshot_sha256 FROM product_version_vector_index_generations_v2 WHERE product_version_id=$1")
            .bind(version_id).fetch_one(&pool).await.unwrap();
    sqlx::query("UPDATE chunks SET content='stale source generation' WHERE id=$1")
        .bind(chunk_id)
        .execute(&pool)
        .await
        .unwrap();
    let stale_intent =
        match knowledge::knowledge_index_v2::prepare_semantic_index_intent_v2(&pool, version_id)
            .await
            .unwrap()
        {
            SemanticIndexPreparationV2::Enqueue(intent) => intent,
            other => panic!("expected stale source intent, got {other:?}"),
        };
    sqlx::query("UPDATE chunks SET content='new immutable source generation' WHERE id=$1")
        .bind(chunk_id)
        .execute(&pool)
        .await
        .unwrap();
    let terminal_intent =
        match knowledge::knowledge_index_v2::prepare_semantic_index_intent_v2(&pool, version_id)
            .await
            .unwrap()
        {
            SemanticIndexPreparationV2::Enqueue(intent) => intent,
            other => panic!("expected newer source intent, got {other:?}"),
        };
    let stale_provider = LifecycleProvider {
        calls: AtomicUsize::new(0),
        results: std::sync::Mutex::new(VecDeque::new()),
    };
    let stale_successor = process_semantic_index_intent_v2(
        &pool,
        stale_intent.id,
        stale_intent.target_revision,
        Some(&stale_provider),
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        stale_provider.calls.load(Ordering::SeqCst),
        0,
        "superseded delivery must not reach the provider"
    );
    let stale = knowledge::knowledge_index_v2::semantic_index_intent_v2(
        &pool,
        stale_intent.id,
        stale_intent.target_revision,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(stale.status, "superseded");
    assert_eq!(
        stale_successor.as_ref().map(|successor| successor.id),
        Some(terminal_intent.id),
        "stale delivery must replay exactly the current successor target"
    );
    let credential_calls = Arc::new(AtomicUsize::new(0));
    let strict = knowledge::knowledge_index_v2::StrictVectorEmbeddingClientV2::new(Arc::new(
        MissingLifecycleCredential {
            calls: credential_calls.clone(),
        },
    ))
    .unwrap();
    process_semantic_index_intent_v2(
        &pool,
        terminal_intent.id,
        terminal_intent.target_revision,
        Some(&strict),
        None,
    )
    .await
    .unwrap();
    process_semantic_index_intent_v2(
        &pool,
        terminal_intent.id,
        terminal_intent.target_revision,
        Some(&strict),
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        credential_calls.load(Ordering::SeqCst),
        1,
        "terminal intent must never be reserved twice"
    );
    let terminal = knowledge::knowledge_index_v2::semantic_index_intent_v2(
        &pool,
        terminal_intent.id,
        terminal_intent.target_revision,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(terminal.status, "terminal");
    assert_eq!(
        terminal.last_error_code.as_deref(),
        Some("INVALID_IMMUTABLE_CONFIGURATION")
    );
    let retained_marker: String = sqlx::query_scalar("SELECT source_snapshot_sha256 FROM product_version_vector_index_generations_v2 WHERE product_version_id=$1")
            .bind(version_id).fetch_one(&pool).await.unwrap();
    assert_eq!(
        retained_marker, prior_marker,
        "failed publication must preserve the prior complete generation"
    );
    let current_snapshot: String = sqlx::query_scalar(
        "SELECT source_snapshot_sha256 FROM kb_knowledge_source_snapshot_v2($1,$2)",
    )
    .bind(version_id)
    .bind(&revision_sha256)
    .fetch_one(&pool)
    .await
    .unwrap();
    let stale_ready: bool = sqlx::query_scalar(
        "SELECT EXISTS(
               SELECT 1 FROM knowledge_semantic_index_intents_v2
                WHERE product_version_id=$1 AND embedding_revision_sha256=$2
                  AND source_snapshot_sha256=$3 AND status='completed'
                  AND generation_marker_sha256=$3)",
    )
    .bind(version_id)
    .bind(&revision_sha256)
    .bind(&current_snapshot)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        !stale_ready,
        "a failed new source generation must never be retrieval-ready"
    );

    sqlx::query("UPDATE chunks SET content='revision fence generation' WHERE id=$1")
        .bind(chunk_id)
        .execute(&pool)
        .await
        .unwrap();
    let revoked_intent =
        match knowledge::knowledge_index_v2::prepare_semantic_index_intent_v2(&pool, version_id)
            .await
            .unwrap()
        {
            SemanticIndexPreparationV2::Enqueue(intent) => intent,
            other => panic!("expected pre-revocation intent, got {other:?}"),
        };
    sqlx::query("UPDATE embedding_revisions_v2 SET support_state='revoked',updated_at=clock_timestamp() WHERE revision_sha256=$1")
            .bind(&revision_sha256).execute(&pool).await.unwrap();
    let revoked_provider = LifecycleProvider {
        calls: AtomicUsize::new(0),
        results: std::sync::Mutex::new(VecDeque::new()),
    };
    process_semantic_index_intent_v2(
        &pool,
        revoked_intent.id,
        revoked_intent.target_revision,
        Some(&revoked_provider),
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        revoked_provider.calls.load(Ordering::SeqCst),
        0,
        "revoked revision must fence before provider I/O"
    );
    let revoked = knowledge::knowledge_index_v2::semantic_index_intent_v2(
        &pool,
        revoked_intent.id,
        revoked_intent.target_revision,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(revoked.status, "terminal");
    assert_eq!(
        knowledge::knowledge_index_v2::prepare_semantic_index_intent_v2(&pool, version_id)
            .await
            .unwrap(),
        SemanticIndexPreparationV2::Terminal(revoked.clone()),
        "terminal immutable generation must not be re-enqueued"
    );

    let mut tx = pool.begin().await.unwrap();
    sqlx::query("ALTER TABLE public.embedding_revisions_v2 DISABLE TRIGGER USER")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM chunks WHERE product_version_id=$1")
        .bind(version_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM object_owner_references WHERE object_ref=$1")
        .bind(&object_ref)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM documents WHERE id=$1")
        .bind(document_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM object_registry WHERE object_ref=$1")
        .bind(&object_ref)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM product_versions WHERE id=$1")
        .bind(version_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM embedding_revisions_v2 WHERE revision_sha256=$1")
        .bind(&revision_sha256)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("ALTER TABLE public.embedding_revisions_v2 ENABLE TRIGGER USER")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM products WHERE id=$1")
        .bind(product_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM workspaces WHERE id=$1")
        .bind(workspace_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}
