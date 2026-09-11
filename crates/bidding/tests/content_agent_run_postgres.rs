#[allow(dead_code)]
mod support;

use bidding::bid_authoring_v2::{self, ContentRunClaim, CreateContentRequestV2};
use bidding::content_runtime::ContentAgentRuntimeContractV1;
use serde_json::json;
use sqlx::{Executor, PgPool};
use uuid::Uuid;

const ACTOR: &str = "user:10000000-0000-4000-8000-000000000001";

async fn owner(pool: &PgPool) -> sqlx::pool::PoolConnection<sqlx::Postgres> {
    let mut connection = pool.acquire().await.unwrap();
    connection.execute("SET ROLE kb_app_owner").await.unwrap();
    connection
}

async fn ensure_phase1(pool: &PgPool) {
    if sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM bid_projects WHERE id='10000000-0000-4000-8000-000000000010')",
    )
    .fetch_one(pool)
    .await
    .unwrap()
    {
        return;
    }
    let sql = include_str!("sql/phase1_acceptance.sql")
        .lines()
        .filter(|line| !line.trim_start().starts_with('\\'))
        .collect::<Vec<_>>()
        .join("\n");
    let mut connection = owner(pool).await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql.as_str()))
        .execute(&mut *connection)
        .await
        .unwrap();
}

async fn create_request(pool: &PgPool, key: &str, operation: &str) -> serde_json::Value {
    ensure_phase1(pool).await;
    let mut connection = owner(pool).await;
    let workspace: (Uuid, Uuid, String) = sqlx::query_as(
        "SELECT workspace.id,head.artifact_id,head.artifact_sha256::text
         FROM bid_submission_workspaces workspace JOIN bid_workspace_heads head ON head.scope_id=workspace.id
         WHERE workspace.project_id='10000000-0000-4000-8000-000000000010'")
        .fetch_one(&mut *connection).await.unwrap();
    let checkpoint_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM bid_outline_checkpoint_artifacts WHERE workspace_id=$1 AND workspace_revision_id=$2)")
        .bind(workspace.0).bind(workspace.1).fetch_one(&mut *connection).await.unwrap();
    if !checkpoint_exists {
        let bytes = br#"{"checkpoint":"content-owner"}"#;
        sqlx::query("SELECT kb_bid_v2_create_outline_checkpoint($1,$2,$3::kb_sha256,$4,$5::kb_actor_identity,$6,$7,$8::kb_sha256)")
            .bind(workspace.0).bind(workspace.1).bind(&workspace.2).bind(Uuid::new_v4())
            .bind(ACTOR).bind(format!("checkpoint-{key}")).bind(bytes.as_slice())
            .bind(platform::sha256_hex(bytes)).execute(&mut *connection).await.unwrap();
    }
    drop(connection);
    let retrieval = knowledge::knowledge_retrieval_pg::freeze_retrieval_policy_identity_v1(pool)
        .await
        .unwrap()
        .unwrap_or_else(|| {
            let sha = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned();
            knowledge::knowledge_retrieval::FrozenRetrievalPolicyIdentityV1 {
                schema_version: 1,
                policy_sha256: sha.clone(),
                canonical_policy_utf8: "{}".into(),
                contract_version: "knowledge-evidence-v2".into(),
                mode: "exact".into(),
                max_hits: 1,
                max_chunk_bytes: 1,
                max_total_bytes: 1,
                embedding_revision_sha256: sha.clone(),
                canonical_embedding_revision_utf8: "{}".into(),
                embedding_credential_ref: "env:EMBED".into(),
                rerank_revision_sha256: sha,
                canonical_rerank_revision_utf8: "{}".into(),
                rerank_credential_ref: "env:RERANK".into(),
                product_version_ids: vec![],
                library_version_ids: vec![],
                eligible_scope_sha256:
                    "715d78b3301b4e5901d8dc93c9d33776a0cef3378d65de32353ee8e998541901".into(),
            }
        });
    let runtime = ContentAgentRuntimeContractV1 {
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
    let request_body = json!({"operation":operation,"workspace_id":workspace.0,"key":key});
    let context = bidding::MutationContext::new(ACTOR, key, &request_body).unwrap();
    bid_authoring_v2::create_content_request_v2(
        pool,
        CreateContentRequestV2 {
            workspace_id: workspace.0,
            expected_revision_id: workspace.1,
            expected_sha256: &workspace.2,
            operation,
            target_kind: "workspace",
            target_node_lineage_id: None,
            fill_policy: "append_candidate",
            insertion_anchor: None,
            evidence_selection_mode: "system_proposed",
            pick_set_artifact_id: None,
            retrieval_identity: Some(&retrieval),
            runtime_contract: (operation == "generate").then_some(&runtime),
        },
        &context,
    )
    .await
    .unwrap()
}

fn identity(value: &serde_json::Value) -> platform::BidAuthoringRequestIdentityV2 {
    serde_json::from_value(json!({
        "request_artifact_id":value["request_artifact_id"],
        "request_revision":value["request_revision"],
        "frozen_input_sha256":value["frozen_input_sha256"]
    }))
    .unwrap()
}

#[tokio::test]
async fn content_owner_stage_and_global_three_call_budget_are_live() {
    let Some(pool) = support::connect_postgres_contract("Content AgentRun").await else {
        return;
    };
    let request = identity(
        &create_request(
            &pool,
            &format!("content-owner-{}", Uuid::new_v4()),
            "generate",
        )
        .await,
    );
    let (left, right) = tokio::join!(
        bid_authoring_v2::claim_content_agent_run_v1(&pool, &request),
        bid_authoring_v2::claim_content_agent_run_v1(&pool, &request)
    );
    let owner = [left.unwrap(), right.unwrap()]
        .into_iter()
        .find_map(|claim| match claim {
            ContentRunClaim::Claimed(owner) => Some(owner),
            _ => None,
        })
        .unwrap();
    let stage = json!({"matches":[],"pending_scope":{},"existing_attestation":null,
        "agent_input":{"frozen":true}});
    let stored = bid_authoring_v2::store_content_agent_input_v1(&pool, &request, &owner, &stage)
        .await
        .unwrap();
    assert_eq!(stored["replayed"], false);
    assert_eq!(
        bid_authoring_v2::load_content_agent_input_v1(&pool, &request)
            .await
            .unwrap()
            .unwrap()["payload"],
        stage
    );
    let input_sha = stored["input_sha256"].as_str().unwrap();
    let typed: (Uuid, String, String, Uuid, String, Uuid, String, String) = sqlx::query_as(
        "SELECT prompt_contract_id,prompt_contract_sha256::text,prompt_sha256::text,
                agent_contract_id,agent_contract_sha256::text,model_contract_id,
                model_contract_sha256::text,runtime_contract_sha256::text
         FROM bid_content_generation_request_identities WHERE request_artifact_id=$1",
    )
    .bind(request.request_artifact_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let claim_call = || {
        bid_authoring_v2::claim_content_boundary_attempt_v1(
            &pool,
            &request,
            &owner,
            input_sha,
            typed.0,
            &typed.1,
            &typed.2,
            "urn:knowledgebrain:bid:content-generation-output:v1",
            "14187ee75ad1c275e45f830a106273fdad92b9423f3062912ba45c7f94b0fccd",
            typed.3,
            &typed.4,
            typed.5,
            &typed.6,
            &typed.7,
        )
    };
    let (a, b, c, d) = tokio::join!(claim_call(), claim_call(), claim_call(), claim_call());
    let results = [a, b, c, d];
    assert_eq!(results.iter().filter(|value| value.is_ok()).count(), 3);
    assert_eq!(results.iter().filter(|value| value.is_err()).count(), 1);
    bid_authoring_v2::yield_content_agent_run_v1(
        &pool,
        &request,
        &owner,
        bid_authoring_v2::ContentRetryYieldCode::Internal,
        "injected",
    )
    .await
    .unwrap();
    let replacement = match bid_authoring_v2::claim_content_agent_run_v1(&pool, &request)
        .await
        .unwrap()
    {
        ContentRunClaim::Claimed(owner) => owner,
        _ => panic!("replacement owner"),
    };
    assert_eq!(
        bid_authoring_v2::load_content_agent_input_v1(&pool, &request)
            .await
            .unwrap()
            .unwrap()["payload"],
        stage
    );
    let divergent = json!({"agent_input":{"frozen":false},"matches":[],"pending_scope":{},"existing_attestation":null});
    assert!(
        bid_authoring_v2::store_content_agent_input_v1(&pool, &request, &replacement, &divergent)
            .await
            .is_err()
    );
    assert!(
        bid_authoring_v2::heartbeat_content_agent_run_v1(&pool, &request, &owner)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn content_owner_expiry_heartbeat_attempt_cap_and_atomic_failure_are_live() {
    let Some(pool) = support::connect_postgres_contract("Content owner lifecycle").await else {
        return;
    };
    let request = identity(
        &create_request(
            &pool,
            &format!("content-life-{}", Uuid::new_v4()),
            "generate",
        )
        .await,
    );
    let first = match bid_authoring_v2::claim_content_agent_run_v1(&pool, &request)
        .await
        .unwrap()
    {
        ContentRunClaim::Claimed(owner) => owner,
        _ => panic!("first owner"),
    };
    assert!(matches!(
        bid_authoring_v2::claim_content_agent_run_v1(&pool, &request)
            .await
            .unwrap(),
        ContentRunClaim::LiveOwner { attempt: 1 }
    ));
    bid_authoring_v2::heartbeat_content_agent_run_v1(&pool, &request, &first)
        .await
        .unwrap();
    let mut connection = owner(&pool).await;
    let invalid=sqlx::query("SELECT kb_bid_v2_content_run_yield_for_retry($1,1,$2::kb_sha256,$3,$4,'AGENT_PROVIDER_UNAVAILABLE','invalid')")
        .bind(request.request_artifact_id).bind(&request.frozen_input_sha256)
        .bind(first.attempt).bind(first.execution_owner_token).execute(&mut *connection).await;
    assert!(invalid.is_err());
    drop(connection);
    let mut current = first;
    for expected in 2..=4 {
        let mut connection = owner(&pool).await;
        sqlx::query("UPDATE bid_content_agent_run_artifacts SET lease_expires_at=clock_timestamp()-interval '1 second' WHERE request_artifact_id=$1 AND attempt=$2")
            .bind(request.request_artifact_id).bind(current.attempt).execute(&mut *connection).await.unwrap();
        drop(connection);
        assert!(
            bid_authoring_v2::heartbeat_content_agent_run_v1(&pool, &request, &current)
                .await
                .is_err()
        );
        current = match bid_authoring_v2::claim_content_agent_run_v1(&pool, &request)
            .await
            .unwrap()
        {
            ContentRunClaim::Claimed(owner) => owner,
            _ => panic!("replacement {expected}"),
        };
        assert_eq!(current.attempt, expected);
    }
    bid_authoring_v2::mark_content_generation_failed_v2(
        &pool,
        &request,
        Some(&current),
        "AGENT_DEADLINE_EXCEEDED",
        "injected terminal",
    )
    .await
    .unwrap();
    let states:(String,String)=sqlx::query_as(
        "SELECT request.status,run.status FROM bid_async_request_snapshot_artifacts request
         JOIN bid_content_agent_run_artifacts run ON run.request_artifact_id=request.id AND run.attempt=request.current_attempt
         WHERE request.id=$1")
        .bind(request.request_artifact_id).fetch_one(&pool).await.unwrap();
    assert_eq!(states, ("failed".into(), "failed".into()));
    assert!(
        bid_authoring_v2::heartbeat_content_agent_run_v1(&pool, &request, &current)
            .await
            .is_err()
    );

    let capped = identity(
        &create_request(
            &pool,
            &format!("content-cap-{}", Uuid::new_v4()),
            "generate",
        )
        .await,
    );
    let mut capped_owner = match bid_authoring_v2::claim_content_agent_run_v1(&pool, &capped)
        .await
        .unwrap()
    {
        ContentRunClaim::Claimed(owner) => owner,
        _ => panic!("cap attempt one"),
    };
    for expected in 2..=4 {
        let mut connection = owner(&pool).await;
        sqlx::query("UPDATE bid_content_agent_run_artifacts SET lease_expires_at=clock_timestamp()-interval '1 second' WHERE request_artifact_id=$1 AND attempt=$2")
            .bind(capped.request_artifact_id).bind(capped_owner.attempt)
            .execute(&mut *connection).await.unwrap();
        drop(connection);
        capped_owner = match bid_authoring_v2::claim_content_agent_run_v1(&pool, &capped)
            .await
            .unwrap()
        {
            ContentRunClaim::Claimed(owner) => owner,
            _ => panic!("cap replacement {expected}"),
        };
        assert_eq!(capped_owner.attempt, expected);
    }
    let mut connection = owner(&pool).await;
    sqlx::query("UPDATE bid_content_agent_run_artifacts SET lease_expires_at=clock_timestamp()-interval '1 second' WHERE request_artifact_id=$1 AND attempt=4")
        .bind(capped.request_artifact_id).execute(&mut *connection).await.unwrap();
    drop(connection);
    assert!(matches!(
        bid_authoring_v2::claim_content_agent_run_v1(&pool, &capped)
            .await
            .unwrap(),
        ContentRunClaim::Exhausted
    ));
    let capped_state: (String, Option<String>) = sqlx::query_as(
        "SELECT status,error_code FROM bid_async_request_snapshot_artifacts WHERE id=$1",
    )
    .bind(capped.request_artifact_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        capped_state,
        (
            "failed".into(),
            Some("REQUEST_ATTEMPT_BUDGET_EXCEEDED".into())
        )
    );
}

#[tokio::test]
async fn generate_publication_requires_exact_stage_and_reserved_call() {
    let Some(pool) = support::connect_postgres_contract("Content publication prerequisites").await
    else {
        return;
    };
    let request = identity(
        &create_request(
            &pool,
            &format!("content-prereq-{}", Uuid::new_v4()),
            "generate",
        )
        .await,
    );
    let owner = match bid_authoring_v2::claim_content_agent_run_v1(&pool, &request)
        .await
        .unwrap()
    {
        ContentRunClaim::Claimed(owner) => owner,
        _ => panic!("publication owner"),
    };
    let missing_attestation = (Uuid::new_v4(), "a".repeat(64));
    let mut tx = pool.begin().await.unwrap();
    let no_stage = bid_authoring_v2::publish_content_generation_v2_in_transaction(
        &mut tx,
        &request,
        Some(&owner),
        (missing_attestation.0, &missing_attestation.1),
        &json!([]),
        None,
        &json!([]),
    )
    .await;
    assert!(
        no_stage
            .unwrap_err()
            .to_string()
            .contains("CONTENT_DIVERGENT_AGENT_INPUT_REPLAY")
    );
    tx.rollback().await.unwrap();

    let stage = json!({"matches":[],"pending_scope":{},"existing_attestation":null,"agent_input":{"frozen":true}});
    let stored = bid_authoring_v2::store_content_agent_input_v1(&pool, &request, &owner, &stage)
        .await
        .unwrap();
    let input_sha = stored["input_sha256"].as_str().unwrap();
    let mut tx = pool.begin().await.unwrap();
    let no_call = bid_authoring_v2::publish_content_generation_v2_in_transaction(
        &mut tx,
        &request,
        Some(&owner),
        (missing_attestation.0, &missing_attestation.1),
        &json!([]),
        None,
        &json!([]),
    )
    .await;
    assert!(
        no_call
            .unwrap_err()
            .to_string()
            .contains("CONTENT_DIVERGENT_AGENT_INPUT_REPLAY")
    );
    tx.rollback().await.unwrap();

    let typed:(Uuid,String,String,Uuid,String,Uuid,String,String)=sqlx::query_as(
        "SELECT prompt_contract_id,prompt_contract_sha256::text,prompt_sha256::text,
         agent_contract_id,agent_contract_sha256::text,model_contract_id,model_contract_sha256::text,
         runtime_contract_sha256::text FROM bid_content_generation_request_identities WHERE request_artifact_id=$1")
        .bind(request.request_artifact_id).fetch_one(&pool).await.unwrap();
    bid_authoring_v2::claim_content_boundary_attempt_v1(
        &pool,
        &request,
        &owner,
        input_sha,
        typed.0,
        &typed.1,
        &typed.2,
        "urn:knowledgebrain:bid:content-generation-output:v1",
        "14187ee75ad1c275e45f830a106273fdad92b9423f3062912ba45c7f94b0fccd",
        typed.3,
        &typed.4,
        typed.5,
        &typed.6,
        &typed.7,
    )
    .await
    .unwrap();
    let mut tx = pool.begin().await.unwrap();
    let after_call = bid_authoring_v2::publish_content_generation_v2_in_transaction(
        &mut tx,
        &request,
        Some(&owner),
        (missing_attestation.0, &missing_attestation.1),
        &json!([]),
        None,
        &json!([]),
    )
    .await;
    assert!(
        after_call
            .unwrap_err()
            .to_string()
            .contains("KNOWLEDGE_MATCHING_ATTESTATION_V2_MISMATCH")
    );
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn owner_cannot_erase_content_run_stage_or_call_history() {
    let Some(pool) = support::connect_postgres_contract("Content immutable history").await else {
        return;
    };
    let request = identity(
        &create_request(
            &pool,
            &format!("content-history-{}", Uuid::new_v4()),
            "generate",
        )
        .await,
    );
    let owner_lease = match bid_authoring_v2::claim_content_agent_run_v1(&pool, &request)
        .await
        .unwrap()
    {
        ContentRunClaim::Claimed(owner) => owner,
        _ => panic!("history owner"),
    };
    let stage = json!({"matches":[],"pending_scope":{},"existing_attestation":null,"agent_input":{"frozen":true}});
    let stored =
        bid_authoring_v2::store_content_agent_input_v1(&pool, &request, &owner_lease, &stage)
            .await
            .unwrap();
    let input_sha = stored["input_sha256"].as_str().unwrap();
    let typed:(Uuid,String,String,Uuid,String,Uuid,String,String)=sqlx::query_as(
        "SELECT prompt_contract_id,prompt_contract_sha256::text,prompt_sha256::text,agent_contract_id,
         agent_contract_sha256::text,model_contract_id,model_contract_sha256::text,runtime_contract_sha256::text
         FROM bid_content_generation_request_identities WHERE request_artifact_id=$1")
        .bind(request.request_artifact_id).fetch_one(&pool).await.unwrap();
    bid_authoring_v2::claim_content_boundary_attempt_v1(
        &pool,
        &request,
        &owner_lease,
        input_sha,
        typed.0,
        &typed.1,
        &typed.2,
        "urn:knowledgebrain:bid:content-generation-output:v1",
        "14187ee75ad1c275e45f830a106273fdad92b9423f3062912ba45c7f94b0fccd",
        typed.3,
        &typed.4,
        typed.5,
        &typed.6,
        &typed.7,
    )
    .await
    .unwrap();
    let mut connection = owner(&pool).await;
    for statement in [
        "DELETE FROM bid_content_agent_run_artifacts",
        "DELETE FROM bid_content_agent_input_artifacts",
        "DELETE FROM bid_content_agent_boundary_attempts",
        "TRUNCATE bid_content_agent_run_artifacts CASCADE",
        "TRUNCATE bid_content_agent_input_artifacts CASCADE",
        "TRUNCATE bid_content_agent_boundary_attempts CASCADE",
    ] {
        assert!(
            sqlx::query(sqlx::AssertSqlSafe(statement))
                .execute(&mut *connection)
                .await
                .is_err(),
            "{statement}"
        );
    }
    for statement in [
        "UPDATE bid_content_agent_run_artifacts SET request_artifact_id=gen_random_uuid()",
        "UPDATE bid_content_agent_input_artifacts SET input_sha256=repeat('b',64)",
        "UPDATE bid_content_agent_boundary_attempts SET call_ordinal=2",
    ] {
        assert!(
            sqlx::query(sqlx::AssertSqlSafe(statement))
                .execute(&mut *connection)
                .await
                .is_err(),
            "{statement}"
        );
    }
    let counts: (i64, i64, i64) = sqlx::query_as(
        "SELECT
      (SELECT count(*) FROM bid_content_agent_run_artifacts WHERE request_artifact_id=$1),
      (SELECT count(*) FROM bid_content_agent_input_artifacts WHERE request_artifact_id=$1),
      (SELECT count(*) FROM bid_content_agent_boundary_attempts WHERE request_artifact_id=$1)",
    )
    .bind(request.request_artifact_id)
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    assert_eq!(counts, (1, 1, 1));
}

#[tokio::test]
async fn knowledge_attestation_and_bidding_publication_roll_back_together() {
    let Some(pool) = support::connect_postgres_contract("Content atomic publication").await else {
        return;
    };
    let request = identity(
        &create_request(
            &pool,
            &format!("content-rollback-{}", Uuid::new_v4()),
            "generate",
        )
        .await,
    );
    let owner = match bid_authoring_v2::claim_content_agent_run_v1(&pool, &request)
        .await
        .unwrap()
    {
        ContentRunClaim::Claimed(owner) => owner,
        _ => panic!("publication owner"),
    };
    let attestation_id = Uuid::new_v4();
    let payload = br#"{}"#;
    let digest = platform::sha256_hex(payload);
    let mut tx = pool.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO knowledge_matching_scope_attestations_v2(id,schema_version,canonical_payload,content_sha256)
         VALUES($1,2,$2,$3)",
    )
    .bind(attestation_id)
    .bind(payload.as_slice())
    .bind(&digest)
    .execute(&mut *tx)
    .await
    .unwrap();
    let failure = bid_authoring_v2::publish_content_generation_v2_in_transaction(
        &mut tx,
        &request,
        Some(&owner),
        (attestation_id, &digest),
        &json!([]),
        None,
        &json!([]),
    )
    .await;
    assert!(failure.is_err());
    tx.rollback().await.unwrap();
    let attestation_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM knowledge_matching_scope_attestations_v2 WHERE id=$1)",
    )
    .bind(attestation_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let states: (String, String, i64) = sqlx::query_as(
        "SELECT request.status,run.status,
           (SELECT count(*) FROM bid_candidate_artifacts candidate WHERE candidate.request_artifact_id=request.id)
         FROM bid_async_request_snapshot_artifacts request
         JOIN bid_content_agent_run_artifacts run ON run.request_artifact_id=request.id AND run.attempt=request.current_attempt
         WHERE request.id=$1",
    )
    .bind(request.request_artifact_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!attestation_exists);
    assert_eq!(states, ("pending".into(), "running".into(), 0));
}

#[tokio::test]
async fn match_only_has_no_agent_owner_or_ledger() {
    let Some(pool) = support::connect_postgres_contract("Content match-only").await else {
        return;
    };
    let request = identity(
        &create_request(
            &pool,
            &format!("content-match-{}", Uuid::new_v4()),
            "match_only",
        )
        .await,
    );
    assert!(matches!(
        bid_authoring_v2::claim_content_agent_run_v1(&pool, &request)
            .await
            .unwrap(),
        ContentRunClaim::Obsolete
    ));
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM bid_content_agent_run_artifacts WHERE request_artifact_id=$1",
    )
    .bind(request.request_artifact_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 0);
}

#[path = "support/diagnostic_messages.rs"]
mod diagnostic_messages;

async fn content_diagnostic_snapshot(pool: &PgPool, id: Uuid) -> serde_json::Value {
    sqlx::query_scalar("SELECT jsonb_build_object('request',to_jsonb(request),'run',to_jsonb(run))
        FROM bid_async_request_snapshot_artifacts request JOIN bid_content_agent_run_artifacts run
        ON run.request_artifact_id=request.id AND run.attempt=request.current_attempt WHERE request.id=$1")
        .bind(id).fetch_one(pool).await.unwrap()
}

#[tokio::test]
async fn content_diagnostics_are_utf8_byte_bounded_and_owner_fenced() {
    let Some(pool) = support::connect_postgres_contract("Content diagnostics").await else {
        return;
    };
    for path in ["terminal", "retry"] {
        for (label, message) in diagnostic_messages::cases() {
            let request = identity(
                &create_request(
                    &pool,
                    &format!("diagnostic-{path}-{label}-{}", Uuid::new_v4()),
                    "generate",
                )
                .await,
            );
            let current = match bid_authoring_v2::claim_content_agent_run_v1(&pool, &request)
                .await
                .unwrap()
            {
                ContentRunClaim::Claimed(owner) => owner,
                _ => panic!("diagnostic owner"),
            };
            let sql = if path == "terminal" {
                "SELECT kb_bid_v2_mark_content_generation_failed($1,1,$2::kb_sha256,$5,$6,$3,$4)"
            } else {
                "SELECT kb_bid_v2_content_run_yield_for_retry($1,1,$2::kb_sha256,$3,$4,$5,$6)"
            };
            let code = if path == "terminal" {
                "AGENT_OUTPUT_INVALID"
            } else {
                "INTERNAL"
            };
            if label == "chinese-regression" {
                for invalid in ["token", "attempt", "lease", "code"] {
                    let mut connection = owner(&pool).await;
                    if invalid == "lease" {
                        sqlx::query("UPDATE bid_content_agent_run_artifacts SET lease_expires_at=clock_timestamp()-interval '1 second' WHERE request_artifact_id=$1 AND attempt=$2")
                            .bind(request.request_artifact_id).bind(current.attempt).execute(&mut *connection).await.unwrap();
                    }
                    let before =
                        content_diagnostic_snapshot(&pool, request.request_artifact_id).await;
                    let error = sqlx::query(sql)
                        .bind(request.request_artifact_id)
                        .bind(&request.frozen_input_sha256)
                        .bind(if invalid == "attempt" {
                            current.attempt + 1
                        } else {
                            current.attempt
                        })
                        .bind(if invalid == "token" {
                            Uuid::new_v4()
                        } else {
                            current.execution_owner_token
                        })
                        .bind(if invalid == "code" {
                            "UNKNOWN_DIAGNOSTIC_CODE"
                        } else {
                            code
                        })
                        .bind(message.as_deref())
                        .execute(&mut *connection)
                        .await
                        .unwrap_err();
                    assert_eq!(
                        error.as_database_error().unwrap().code().as_deref(),
                        Some(if invalid == "code" { "22023" } else { "40001" }),
                        "{path}/{invalid}: {error}"
                    );
                    assert_eq!(
                        before,
                        content_diagnostic_snapshot(&pool, request.request_artifact_id).await
                    );
                    if invalid == "lease" {
                        sqlx::query("UPDATE bid_content_agent_run_artifacts SET lease_expires_at=least(clock_timestamp()+interval '30 seconds',hard_deadline_at) WHERE request_artifact_id=$1 AND attempt=$2")
                            .bind(request.request_artifact_id).bind(current.attempt).execute(&mut *connection).await.unwrap();
                    }
                }
            }
            let before = content_diagnostic_snapshot(&pool, request.request_artifact_id).await;
            let mut connection = owner(&pool).await;
            sqlx::query(sql)
                .bind(request.request_artifact_id)
                .bind(&request.frozen_input_sha256)
                .bind(current.attempt)
                .bind(current.execution_owner_token)
                .bind(code)
                .bind(message.as_deref())
                .execute(&mut *connection)
                .await
                .unwrap_or_else(|error| panic!("{path}/{label}: {error}"));
            drop(connection);
            let after = content_diagnostic_snapshot(&pool, request.request_artifact_id).await;
            let run = &after["run"];
            diagnostic_messages::assert_message(
                &run["last_error_message"],
                Some(message.as_deref().unwrap_or("")),
            );
            assert_eq!(run["last_error_code"], code);
            assert_eq!(
                run["progress_detail"],
                if path == "terminal" {
                    json!({"phase":"failed","error_code":code})
                } else {
                    json!({"phase":"retrying","last_error_code":code})
                }
            );
            assert_eq!(
                run["progress_sequence"].as_i64().unwrap(),
                before["run"]["progress_sequence"].as_i64().unwrap() + 1
            );
            for key in ["attempt", "execution_owner_token", "hard_deadline_at"] {
                assert_eq!(run[key], before["run"][key]);
            }
            assert_eq!(run["lease_expires_at"], run["last_error_at"]);
            assert_eq!(
                after["request"]["status"],
                if path == "terminal" {
                    "failed"
                } else {
                    "pending"
                }
            );
            assert_eq!(
                run["status"],
                if path == "terminal" {
                    "failed"
                } else {
                    "retry_yielded"
                }
            );
            if path == "terminal" {
                assert_eq!(after["request"]["error_code"], code);
                assert_eq!(after["request"]["finished_at"], run["last_error_at"]);
            } else {
                assert_eq!(after["request"], before["request"]);
            }
        }
    }
    pool.close().await;
}
