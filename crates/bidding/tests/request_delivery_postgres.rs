#[allow(dead_code)]
mod support;

use bidding::bid_authoring_v2::{self, CreateContentRequestV2};
use bidding::content_runtime::ContentAgentRuntimeContractV1;
use platform::{BidAuthoringJobPayloadV2, ContentGenerateOperationV2};
use retention::{ObjectRetentionWorker, ObjectUploadExpireWorker, RetentionCtx};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Executor, PgPool, Postgres};
use std::time::Duration;
use uuid::Uuid;

const ACTOR: &str = "user:10000000-0000-4000-8000-000000000001";
const PROJECT_ID: Uuid = Uuid::from_u128(0x10000000000040008000000000000010);
const DELIVERY_TEST_ADVISORY_LOCK: i64 = 0x4b425f44454c4956;

async fn acquire_delivery_test_lock(pool: &PgPool) -> sqlx::pool::PoolConnection<Postgres> {
    let mut connection = pool.acquire().await.expect("delivery test lock connection");
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(DELIVERY_TEST_ADVISORY_LOCK)
        .execute(&mut *connection)
        .await
        .expect("acquire delivery test advisory lock");
    connection
}

async fn release_delivery_test_lock(connection: &mut sqlx::pool::PoolConnection<Postgres>) {
    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(DELIVERY_TEST_ADVISORY_LOCK)
        .execute(&mut **connection)
        .await
        .expect("release delivery test advisory lock");
}

#[derive(Clone, Debug)]
struct CreatedRequest {
    response: Value,
    expected: ExpectedPayload,
}

#[derive(Clone, Debug)]
enum ExpectedPayload {
    Tender {
        document_revision_id: Uuid,
    },
    Requirement {
        document_set_revision_id: Uuid,
        disposition_set_revision_id: Uuid,
    },
    Content {
        workspace_id: Uuid,
        base_workspace_revision_id: Uuid,
        operation: ContentGenerateOperationV2,
    },
    Export {
        workspace_id: Uuid,
    },
}

impl CreatedRequest {
    fn id(&self) -> Uuid {
        Uuid::parse_str(
            self.response["request_artifact_id"]
                .as_str()
                .expect("request_artifact_id"),
        )
        .expect("request UUID")
    }

    fn revision(&self) -> i64 {
        self.response["request_revision"]
            .as_i64()
            .expect("request_revision")
    }

    fn frozen_sha(&self) -> &str {
        self.response["frozen_input_sha256"]
            .as_str()
            .expect("frozen_input_sha256")
    }
}

async fn owner_connection(pool: &PgPool) -> sqlx::pool::PoolConnection<Postgres> {
    let mut connection = pool.acquire().await.expect("acquire PostgreSQL");
    connection
        .execute("SET ROLE kb_app_owner")
        .await
        .expect("set application owner role");
    connection
}

async fn ensure_phase1_fixture(pool: &PgPool) {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM bid_projects WHERE id=$1)")
        .bind(PROJECT_ID)
        .fetch_one(pool)
        .await
        .expect("inspect phase-one fixture");
    if exists {
        return;
    }
    let phase1 = include_str!("sql/phase1_acceptance.sql")
        .lines()
        .skip(1)
        .collect::<Vec<_>>()
        .join("\n");
    let mut connection = owner_connection(pool).await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(phase1))
        .execute(&mut *connection)
        .await
        .expect("install minimum real creator fixture");
}

async fn workspace_head(pool: &PgPool) -> (Uuid, Uuid, String) {
    sqlx::query_as(
        "SELECT workspace.id,head.artifact_id,head.artifact_sha256
           FROM bid_submission_workspaces workspace
           JOIN bid_workspace_heads head ON head.scope_id=workspace.id
          WHERE workspace.project_id=$1",
    )
    .bind(PROJECT_ID)
    .fetch_one(pool)
    .await
    .expect("workspace head")
}

async fn ensure_checkpoint(pool: &PgPool) {
    let (workspace_id, revision_id, revision_sha) = workspace_head(pool).await;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(
           SELECT 1 FROM bid_outline_checkpoint_artifacts checkpoint
           JOIN bid_workspace_revision_artifacts revision
             ON revision.id=checkpoint.workspace_revision_id
          WHERE checkpoint.workspace_id=$1 AND checkpoint.workspace_revision_id=$2
            AND checkpoint.requirement_projection_id=revision.requirement_projection_id
            AND checkpoint.requirement_projection_sha256=revision.requirement_projection_sha256)",
    )
    .bind(workspace_id)
    .bind(revision_id)
    .fetch_one(pool)
    .await
    .expect("inspect checkpoint");
    if exists {
        return;
    }
    let bytes = br#"{"request_delivery_checkpoint":1}"#;
    let mut connection = owner_connection(pool).await;
    sqlx::query_scalar::<_, Value>(
        "SELECT kb_bid_v2_create_outline_checkpoint(
           $1,$2,$3::kb_sha256,$4,$5::kb_actor_identity,$6,$7,
           kb_bid_v2_sha256_bytes($7))",
    )
    .bind(workspace_id)
    .bind(revision_id)
    .bind(revision_sha)
    .bind(Uuid::new_v4())
    .bind(ACTOR)
    .bind(format!("delivery-checkpoint-{}", Uuid::new_v4()))
    .bind(bytes.as_slice())
    .fetch_one(&mut *connection)
    .await
    .expect("create checkpoint through real mutation");
}

async fn create_tender_request(pool: &PgPool) -> CreatedRequest {
    let staging_id = Uuid::new_v4();
    let document_id = Uuid::new_v4();
    let request_id = Uuid::new_v4();
    let object_sha = hex::encode(Sha256::digest(document_id.as_bytes()));
    let object_ref = format!("objects/{object_sha}");
    let request_bytes = br#"{"delivery_matrix":"tender"}"#;
    let mut connection = owner_connection(pool).await;
    sqlx::query(
        "SELECT kb_object_upload_stage($1,$2::kb_object_ref,$3::kb_sha256,
           'application/pdf',1,$4::kb_actor_identity)",
    )
    .bind(staging_id)
    .bind(&object_ref)
    .bind(&object_sha)
    .bind(ACTOR)
    .execute(&mut *connection)
    .await
    .expect("stage tender object");
    let response: Value = sqlx::query_scalar(
        "SELECT kb_bid_v2_upload_tender_document(
           $1,$2,$3,$4,'delivery-matrix.pdf','application/pdf',1,$5::kb_object_ref,
           $6::kb_sha256,$7::kb_actor_identity,$8,$9,kb_bid_v2_sha256_bytes($9))",
    )
    .bind(staging_id)
    .bind(document_id)
    .bind(request_id)
    .bind(PROJECT_ID)
    .bind(object_ref)
    .bind(object_sha)
    .bind(ACTOR)
    .bind(format!("delivery-tender-{}", Uuid::new_v4()))
    .bind(request_bytes.as_slice())
    .fetch_one(&mut *connection)
    .await
    .expect("create TenderDocumentProcess Request");
    CreatedRequest {
        response,
        expected: ExpectedPayload::Tender {
            document_revision_id: document_id,
        },
    }
}

async fn create_requirement_request(pool: &PgPool) -> CreatedRequest {
    let (document_set_id, _document_set_sha, disposition_id, disposition_sha): (
        Uuid,
        String,
        Uuid,
        String,
    ) = sqlx::query_as(
        "SELECT document_head.artifact_id,document_head.artifact_sha256,
                disposition_head.artifact_id,disposition_head.artifact_sha256
           FROM bid_document_set_current document_head
           JOIN bid_source_unit_disposition_set_current disposition_head
             ON disposition_head.scope_id=document_head.scope_id
          WHERE document_head.scope_id=$1",
    )
    .bind(PROJECT_ID)
    .fetch_one(pool)
    .await
    .expect("current document/disposition sets");
    let items: Value = sqlx::query_scalar(
        "SELECT jsonb_agg(jsonb_build_object(
                  'source_unit_revision_id',source.id,'disposition','requirement',
                  'reason','request delivery matrix') ORDER BY source.id)
           FROM bid_document_set_items set_item
           JOIN bid_source_unit_revision_artifacts source
             ON source.project_id=set_item.project_id
            AND source.source_revision_id=set_item.source_revision_id
          WHERE set_item.document_set_id=$1",
    )
    .bind(document_set_id)
    .fetch_one(pool)
    .await
    .expect("disposition coverage");
    let request_id = Uuid::new_v4();
    let request_bytes = br#"{"delivery_matrix":"requirement"}"#;
    let mut connection = owner_connection(pool).await;
    let response: Value = sqlx::query_scalar(
        "SELECT kb_bid_v2_publish_disposition_set(
           $1,$2,$3,$4,$5::kb_sha256,$6,$7::kb_actor_identity,$8,$9,
           kb_bid_v2_sha256_bytes($9))",
    )
    .bind(PROJECT_ID)
    .bind(document_set_id)
    .bind(items)
    .bind(disposition_id)
    .bind(disposition_sha)
    .bind(request_id)
    .bind(ACTOR)
    .bind(format!("delivery-requirement-{}", Uuid::new_v4()))
    .bind(request_bytes.as_slice())
    .fetch_one(&mut *connection)
    .await
    .expect("create RequirementSetCompile Request");
    let new_disposition_id = Uuid::parse_str(
        response["artifact_id"]
            .as_str()
            .expect("disposition artifact id"),
    )
    .expect("disposition UUID");
    CreatedRequest {
        response,
        expected: ExpectedPayload::Requirement {
            document_set_revision_id: document_set_id,
            disposition_set_revision_id: new_disposition_id,
        },
    }
}

async fn create_content_request(
    pool: &PgPool,
    operation: ContentGenerateOperationV2,
) -> CreatedRequest {
    ensure_checkpoint(pool).await;
    let (workspace_id, revision_id, revision_sha) = workspace_head(pool).await;
    let operation_text = match operation {
        ContentGenerateOperationV2::MatchOnly => "match_only",
        ContentGenerateOperationV2::Generate => "generate",
    };
    let idempotency_key = format!("delivery-content-{operation_text}-{}", Uuid::new_v4());
    let request_body = json!({"delivery_matrix":"content","operation":operation_text});
    let context = bidding::MutationContext::new(ACTOR, &idempotency_key, &request_body).unwrap();
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
    let response = bid_authoring_v2::create_content_request_v2(
        pool,
        CreateContentRequestV2 {
            workspace_id,
            expected_revision_id: revision_id,
            expected_sha256: &revision_sha,
            operation: operation_text,
            target_kind: "workspace",
            target_node_lineage_id: None,
            fill_policy: "append_candidate",
            insertion_anchor: None,
            evidence_selection_mode: "system_proposed",
            pick_set_artifact_id: None,
            retrieval_identity: Some(&retrieval),
            runtime_contract: (operation_text == "generate").then_some(&runtime),
        },
        &context,
    )
    .await
    .expect("create ContentGenerate Request");
    CreatedRequest {
        response,
        expected: ExpectedPayload::Content {
            workspace_id,
            base_workspace_revision_id: revision_id,
            operation,
        },
    }
}

async fn create_export_request(pool: &PgPool) -> CreatedRequest {
    let (workspace_id, _, _) = workspace_head(pool).await;
    let mut connection = owner_connection(pool).await;
    // This phase-one fixture has newer sources than its published requirements.
    // Seed an already-saved DOCX on that historical basis; export creation must
    // preserve it without pretending the older requirements are current. Actual
    // new-round creation and its basis CAS are exercised in phase-six SQL.
    let current: Option<(Uuid, String)> =
        sqlx::query_as("SELECT version_id,docx_sha256 FROM bid_docx_current WHERE scope_id=$1")
            .bind(workspace_id)
            .fetch_optional(&mut *connection)
            .await
            .unwrap();
    let (version_id, docx_sha) = match current {
        Some(current) => current,
        None => {
            let bytes = b"request-delivery saved DOCX identity";
            let digest = platform::sha256_hex(bytes);
            let staging = Uuid::new_v4();
            sqlx::query("SELECT kb_object_upload_stage($1,('objects/'||$2)::kb_object_ref,$2::kb_sha256,'application/vnd.openxmlformats-officedocument.wordprocessingml.document',$3,$4::kb_actor_identity)")
                .bind(staging).bind(&digest).bind(bytes.len() as i64).bind(ACTOR)
                .execute(&mut *connection).await.unwrap();
            let round_id = Uuid::new_v4();
            let version_id = Uuid::new_v4();
            connection.execute("BEGIN").await.unwrap();
            sqlx::query("SELECT kb_object_upload_commit($1,('objects/'||$2)::kb_object_ref,$2::kb_sha256,'application/vnd.openxmlformats-officedocument.wordprocessingml.document',$3,'bid_docx_version',$4,'document',$5::kb_actor_identity)")
                .bind(staging).bind(&digest).bind(bytes.len() as i64).bind(version_id).bind(ACTOR)
                .execute(&mut *connection).await.unwrap();
            sqlx::query("INSERT INTO bid_docx_round_artifacts(id,project_id,workspace_id,revision,document_set_id,requirement_set_id,canonical_payload,content_sha256,actor)
                SELECT $1,$2,$3,1,r.document_set_id,r.id,kb_bid_v2_json_payload(jsonb_build_object('saved_before_source_update',true)),
                  kb_bid_v2_sha256_bytes(kb_bid_v2_json_payload(jsonb_build_object('saved_before_source_update',true))),$4::kb_actor_identity
                FROM bid_requirement_set_current c JOIN bid_requirement_set_artifacts r ON r.id=c.artifact_id WHERE c.scope_id=$2")
                .bind(round_id).bind(PROJECT_ID).bind(workspace_id).bind(ACTOR)
                .execute(&mut *connection).await.unwrap();
            sqlx::query("INSERT INTO bid_docx_version_artifacts(id,project_id,workspace_id,round_id,revision,object_ref,docx_sha256,byte_length,actor)
                VALUES($1,$2,$3,$4,1,('objects/'||$5)::kb_object_ref,$5::kb_sha256,$6,$7::kb_actor_identity)")
                .bind(version_id).bind(PROJECT_ID).bind(workspace_id).bind(round_id).bind(&digest)
                .bind(bytes.len() as i64).bind(ACTOR).execute(&mut *connection).await.unwrap();
            sqlx::query("INSERT INTO bid_docx_current(scope_id,project_id,round_id,version_id,docx_sha256) VALUES($1,$2,$3,$4,$5::kb_sha256)")
                .bind(workspace_id).bind(PROJECT_ID).bind(round_id).bind(version_id).bind(&digest)
                .execute(&mut *connection).await.unwrap();
            connection.execute("COMMIT").await.unwrap();
            (version_id, digest)
        }
    };
    let request_bytes = br#"{"delivery_matrix":"export"}"#;
    let response: Value = sqlx::query_scalar(
        "SELECT kb_bid_v2_create_submission_export_request(
          $1,$2,$3::kb_sha256,$4::kb_actor_identity,$5,$6,kb_bid_v2_sha256_bytes($6))",
    )
    .bind(workspace_id)
    .bind(version_id)
    .bind(docx_sha)
    .bind(ACTOR)
    .bind(format!("delivery-export-{}", Uuid::new_v4()))
    .bind(request_bytes.as_slice())
    .fetch_one(&mut *connection)
    .await
    .expect("create SubmissionExport Request");
    CreatedRequest {
        response,
        expected: ExpectedPayload::Export { workspace_id },
    }
}

async fn create_real_request_matrix(pool: &PgPool) -> Vec<CreatedRequest> {
    vec![
        create_tender_request(pool).await,
        create_requirement_request(pool).await,
        create_content_request(pool, ContentGenerateOperationV2::MatchOnly).await,
        create_content_request(pool, ContentGenerateOperationV2::Generate).await,
        create_export_request(pool).await,
    ]
}

fn assert_expected_payload(payload: &BidAuthoringJobPayloadV2, expected: &ExpectedPayload) {
    match (payload, expected) {
        (
            BidAuthoringJobPayloadV2::TenderDocumentProcess {
                project_id,
                document_revision_id,
                ..
            },
            ExpectedPayload::Tender {
                document_revision_id: expected_document,
            },
        ) => {
            assert_eq!(*project_id, PROJECT_ID);
            assert_eq!(document_revision_id, expected_document);
        }
        (
            BidAuthoringJobPayloadV2::RequirementSetCompile {
                project_id,
                document_set_revision_id,
                disposition_set_revision_id,
                ..
            },
            ExpectedPayload::Requirement {
                document_set_revision_id: expected_document_set,
                disposition_set_revision_id: expected_disposition_set,
            },
        ) => {
            assert_eq!(*project_id, PROJECT_ID);
            assert_eq!(document_set_revision_id, expected_document_set);
            assert_eq!(disposition_set_revision_id, expected_disposition_set);
        }
        (
            BidAuthoringJobPayloadV2::ContentGenerate {
                project_id,
                workspace_id,
                base_workspace_revision_id,
                operation,
                ..
            },
            ExpectedPayload::Content {
                workspace_id: expected_workspace,
                base_workspace_revision_id: expected_revision,
                operation: expected_operation,
            },
        ) => {
            assert_eq!(*project_id, PROJECT_ID);
            assert_eq!(workspace_id, expected_workspace);
            assert_eq!(base_workspace_revision_id, expected_revision);
            assert_eq!(operation, expected_operation);
        }
        (
            BidAuthoringJobPayloadV2::SubmissionExport {
                project_id,
                workspace_id,
                ..
            },
            ExpectedPayload::Export {
                workspace_id: expected_workspace,
            },
        ) => {
            assert_eq!(*project_id, PROJECT_ID);
            assert_eq!(workspace_id, expected_workspace);
        }
        other => panic!("wrong closed payload variant: {other:?}"),
    }
}

async fn assert_frozen_payload_exact(pool: &PgPool, created: &CreatedRequest) {
    let mut connection = owner_connection(pool).await;
    let reservation: Value =
        sqlx::query_scalar("SELECT kb_bid_v2_load_authoring_job_payload($1,1,$2::kb_sha256)")
            .bind(created.id())
            .bind(created.frozen_sha())
            .fetch_one(&mut *connection)
            .await
            .expect("load real Request payload");
    assert_eq!(reservation["status"], "pending");
    assert_eq!(reservation["request_artifact_id"], created.id().to_string());
    assert_eq!(reservation["request_revision"], 1);
    assert_eq!(
        reservation["frozen_input_sha256"],
        created.response["frozen_input_sha256"]
    );
    assert_eq!(
        reservation["request_sha256"],
        created.response["request_sha256"]
    );

    let payload: BidAuthoringJobPayloadV2 = serde_json::from_value(
        reservation
            .get("job_payload")
            .cloned()
            .expect("reservation job_payload"),
    )
    .expect("deserialize closed BidAuthoringJobPayloadV2");
    payload.validate().expect("valid queue payload identity");
    assert_eq!(payload.request().request_artifact_id, created.id());
    assert_eq!(payload.request().request_revision, 1);
    assert_eq!(payload.request().frozen_input_sha256, created.frozen_sha());
    assert_eq!(
        reservation["request_kind"],
        payload.kind().as_str(),
        "frozen request kind must identify the deserialized variant"
    );
    assert_expected_payload(&payload, &created.expected);

    let (stored_bytes, stored_sha, frozen_sha): (Vec<u8>, String, String) = sqlx::query_as(
        "SELECT request_payload,request_sha256,frozen_input_sha256
           FROM bid_async_request_snapshot_artifacts WHERE id=$1",
    )
    .bind(created.id())
    .fetch_one(&mut *connection)
    .await
    .expect("stored Request envelope");
    let canonical: Vec<u8> = sqlx::query_scalar("SELECT kb_bid_v2_json_payload($1)")
        .bind(&reservation["job_payload"])
        .fetch_one(&mut *connection)
        .await
        .expect("serialize loaded payload through the frozen SQL byte seam");
    assert_eq!(
        stored_bytes, canonical,
        "stored bytes must equal the exact frozen SQL queue envelope bytes"
    );
    assert_eq!(stored_sha, hex::encode(Sha256::digest(&stored_bytes)));
    assert_eq!(stored_sha, reservation["request_sha256"]);
    assert_eq!(frozen_sha, reservation["frozen_input_sha256"]);
}

async fn assert_transaction_local_tamper_fails_closed(pool: &PgPool, request_id: Uuid) {
    let mut connection = pool.acquire().await.expect("tamper connection");
    connection
        .execute("RESET ROLE")
        .await
        .expect("reset pooled role");
    connection.execute("BEGIN").await.expect("begin tamper");
    connection
        .execute("ALTER TABLE bid_async_request_snapshot_artifacts DISABLE TRIGGER ALL")
        .await
        .expect("disable Request guards transaction-locally");
    sqlx::query(
        "UPDATE bid_async_request_snapshot_artifacts
            SET request_payload=convert_to('{}','UTF8'),
                request_sha256=kb_bid_v2_sha256_bytes(convert_to('{}','UTF8'))
          WHERE id=$1",
    )
    .bind(request_id)
    .execute(&mut *connection)
    .await
    .expect("tamper Request payload inside rollback-only transaction");
    connection
        .execute("ALTER TABLE bid_async_request_snapshot_artifacts ENABLE TRIGGER ALL")
        .await
        .expect("restore Request guards");
    connection
        .execute("SET LOCAL ROLE kb_app_owner")
        .await
        .expect("set owner for reconstruction");
    let error = sqlx::query_scalar::<_, Value>("SELECT kb_bid_v2_authoring_job_payload($1)")
        .bind(request_id)
        .fetch_one(&mut *connection)
        .await
        .expect_err("tampered persisted envelope must fail closed");
    assert!(
        error.to_string().contains("query returned no rows")
            || error.to_string().contains("REQUEST_JOB_PAYLOAD_MISMATCH"),
        "unexpected tamper error: {error}"
    );
    connection
        .execute("ROLLBACK")
        .await
        .expect("rollback tamper");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn all_real_request_creators_persist_exact_replayable_queue_payloads() {
    let Some(pool) = support::connect_postgres_contract("Request delivery real creators").await
    else {
        return;
    };
    let mut test_lock = acquire_delivery_test_lock(&pool).await;
    ensure_phase1_fixture(&pool).await;
    let created = create_real_request_matrix(&pool).await;
    assert_eq!(created.len(), 5);
    for request in &created {
        assert_frozen_payload_exact(&pool, request).await;
    }
    assert_transaction_local_tamper_fails_closed(&pool, created[2].id()).await;
    release_delivery_test_lock(&mut test_lock).await;
}

#[tokio::test]
async fn non_agent_handler_timeout_codes_terminalize_requests_atomically() {
    let Some(pool) =
        support::connect_postgres_contract("Request handler timeout terminality").await
    else {
        return;
    };
    let mut test_lock = acquire_delivery_test_lock(&pool).await;
    ensure_phase1_fixture(&pool).await;
    let requests = create_real_request_matrix(&pool).await;
    let tender = &requests[0];
    let requirement = &requests[1];
    let export = requests
        .iter()
        .find(|request| matches!(request.expected, ExpectedPayload::Export { .. }))
        .expect("export request");

    for (request_id, revision, digest) in [
        (tender.id(), tender.revision() + 1, tender.frozen_sha()),
        (tender.id(), tender.revision(), &"0".repeat(64)),
        (
            requirement.id(),
            requirement.revision(),
            requirement.frozen_sha(),
        ),
    ] {
        let error =
            sqlx::query("SELECT kb_bid_v2_mark_tender_document_failed($1,$2,$3::kb_sha256,$4)")
                .bind(request_id)
                .bind(revision)
                .bind(digest)
                .bind("TENDER_DOCUMENT_PROCESS_TIMEOUT")
                .execute(&pool)
                .await
                .expect_err("stale or cross-kind Tender terminal fence was accepted");
        assert!(
            error.to_string().contains("REQUEST_OBSOLETE"),
            "unexpected Tender fence error: {error}"
        );
    }
    let tender_status: String =
        sqlx::query_scalar("SELECT status FROM bid_async_request_snapshot_artifacts WHERE id=$1")
            .bind(tender.id())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(tender_status, "pending");

    bidding::bid_authoring_v2::mark_tender_document_failed_v2(
        &pool,
        tender.id(),
        tender.revision(),
        tender.frozen_sha(),
        "TENDER_DOCUMENT_PROCESS_TIMEOUT",
    )
    .await
    .unwrap();
    bidding::bid_authoring_v2::mark_requirement_set_compile_failed_v2(
        &pool,
        requirement.id(),
        1,
        requirement.frozen_sha(),
        "REQUIREMENT_COMPILE_TIMEOUT",
    )
    .await
    .unwrap();
    bidding::bid_authoring_v2::mark_submission_export_failed_v2(
        &pool,
        export.id(),
        1,
        export.frozen_sha(),
        "SUBMISSION_EXPORT_TIMEOUT",
    )
    .await
    .unwrap();

    let terminal: Vec<(Uuid, String, Option<String>, Option<Value>)> = sqlx::query_as(
        "SELECT id,status,error_code,result_identity
           FROM bid_async_request_snapshot_artifacts
          WHERE id=ANY($1) ORDER BY id",
    )
    .bind([tender.id(), requirement.id(), export.id()])
    .fetch_all(&pool)
    .await
    .unwrap();
    let expected = std::collections::HashMap::from([
        (tender.id(), "TENDER_DOCUMENT_PROCESS_TIMEOUT"),
        (requirement.id(), "REQUIREMENT_COMPILE_TIMEOUT"),
        (export.id(), "SUBMISSION_EXPORT_TIMEOUT"),
    ]);
    assert_eq!(terminal.len(), 3);
    for (id, status, code, result) in terminal {
        assert_eq!(status, "failed");
        assert_eq!(code.as_deref(), expected.get(&id).copied());
        assert!(result.is_none());
    }
    let document_id = match tender.expected {
        ExpectedPayload::Tender {
            document_revision_id,
        } => document_revision_id,
        _ => unreachable!(),
    };
    let parse_status: String =
        sqlx::query_scalar("SELECT parse_status FROM bid_documents WHERE id=$1")
            .bind(document_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(parse_status, "failed");

    let staging_id = Uuid::new_v4();
    let bytes = format!("timeout cleanup {staging_id}").into_bytes();
    require_local_objects();
    let digest = hex::encode(Sha256::digest(&bytes));
    let object_ref = platform::object_ref(&digest);
    let cleanup = platform::StagedObjectCleanupTracker::new(&pool, "system:submission-export-v2");
    cleanup.register(staging_id);
    platform::stage_object_upload(
        &pool,
        staging_id,
        &object_ref,
        &digest,
        "application/octet-stream",
        i64::try_from(bytes.len()).unwrap(),
        "system:submission-export-v2",
    )
    .await
    .unwrap();
    let blob = platform::write_blob_async(&digest, &bytes).await.unwrap();
    assert_eq!(std::fs::read(&blob).unwrap(), bytes);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    tokio::time::timeout_at(deadline, cleanup.cleanup_pending())
        .await
        .expect("handoff deadline")
        .unwrap();
    assert!(
        !cleanup.has_pending(),
        "confirmed handoff disarms the tracker"
    );
    let staging_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM object_upload_staging WHERE id=$1)")
            .bind(staging_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        staging_exists,
        "handoff cannot release staging without a consumer"
    );
    assert!(blob.is_file(), "handoff cannot reclaim the blob");
    let consumer = start_retention_consumer().await;
    let completed = wait_for_deleted(&pool, staging_id, &digest, bytes.len() as i64).await;
    consumer.finish().await;
    completed.expect("real typed consumer must release staging and reclaim the object");
    assert!(!blob.exists());
    release_delivery_test_lock(&mut test_lock).await;
}

fn require_local_objects() {
    let dir =
        std::env::var_os("OBJECT_DIR").expect("cleanup contract requires explicit OBJECT_DIR");
    assert!(
        std::path::Path::new(&dir).is_dir(),
        "OBJECT_DIR must already exist"
    );
    assert!(
        std::env::var("KNOWLEDGEBRAIN_S3_ENDPOINT")
            .unwrap_or_default()
            .is_empty(),
        "cleanup contract uses local objects only"
    );
}

struct RetentionConsumer {
    stop: tokio::sync::watch::Sender<bool>,
    task: tokio::task::JoinHandle<Result<(), oxana::OxanaError>>,
}

impl RetentionConsumer {
    async fn finish(mut self) {
        self.stop
            .send(true)
            .expect("retention runtime is still running");
        tokio::time::timeout(Duration::from_secs(10), &mut self.task)
            .await
            .expect("retention shutdown deadline")
            .expect("retention task join")
            .expect("retention runtime result");
    }
}

impl Drop for RetentionConsumer {
    fn drop(&mut self) {
        // Also stop the owned runtime when a test assertion panics.
        let _ = self.stop.send(true);
        self.task.abort();
    }
}

async fn start_retention_consumer() -> RetentionConsumer {
    platform::init_tracing();
    let url = std::env::var("KNOWLEDGEBRAIN_TEST_DATABASE_URL").unwrap();
    let options = url
        .parse::<sqlx::postgres::PgConnectOptions>()
        .unwrap()
        .username("kb_runtime_retention")
        .password(
            &std::env::var("KNOWLEDGEBRAIN_RETENTION_DB_PASSWORD")
                .expect("cleanup contract requires retention role password"),
        );
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect_with(options)
        .await
        .expect("real retention role login");
    let storage = platform::oxana_connect().expect("explicit Oxana configuration");
    let (stop, mut stopped) = tokio::sync::watch::channel(false);
    let runtime = storage
        .runtime(RetentionCtx::new(pool))
        .queue_with_concurrency::<platform::RetentionQueue>(1)
        .worker::<ObjectRetentionWorker, platform::ObjectRetentionJob>()
        .worker::<ObjectUploadExpireWorker, platform::ObjectUploadExpireJob>()
        .shutdown_on(async move {
            while !*stopped.borrow() {
                if stopped.changed().await.is_err() {
                    break;
                }
            }
            Ok(())
        })
        .shutdown_timeout(Duration::from_secs(5))
        .run();
    RetentionConsumer {
        stop,
        task: tokio::spawn(async move { runtime.await.map(|_| ()) }),
    }
}

async fn wait_for_deleted(
    pool: &PgPool,
    staging_id: Uuid,
    digest: &str,
    length: i64,
) -> Result<(), tokio::time::error::Elapsed> {
    tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            let completed: bool = sqlx::query_scalar(
                "SELECT NOT EXISTS(SELECT 1 FROM object_upload_staging WHERE id=$1)
                  AND EXISTS(SELECT 1 FROM object_registry r
                    JOIN object_retention_tombstones t ON t.object_ref=r.object_ref
                    JOIN object_deletion_artifacts d ON d.id=t.deletion_id
                    WHERE t.deletion_id=$1 AND r.digest=$2 AND t.digest=$2
                      AND d.digest=$2 AND d.byte_length=$3 AND t.byte_length=$3
                      AND r.byte_length=$3 AND r.state='deleted'
                      AND r.deleted_at IS NOT NULL AND t.deleted_at IS NOT NULL
                      AND t.deleted_by='system:retention-consumer')",
            )
            .bind(staging_id)
            .bind(digest)
            .bind(length)
            .fetch_one(pool)
            .await
            .unwrap();
            if completed
                && !platform::blob_path(digest)
                    .expect("valid configured test object path")
                    .exists()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
}

#[tokio::test]
async fn cleanup_native_duplicates_protect_references_and_retry_failed_blob_deletion() {
    let Some(pool) = support::connect_postgres_contract("typed cleanup recovery").await else {
        return;
    };
    let mut test_lock = acquire_delivery_test_lock(&pool).await;
    require_local_objects();
    let first = Uuid::new_v4();
    let remaining_owner = Uuid::new_v4();
    let bytes = format!("cleanup reference and retry {first}").into_bytes();
    let digest = platform::sha256_hex(&bytes);
    let object_ref = platform::object_ref(&digest);
    for id in [first, remaining_owner] {
        platform::stage_object_upload(
            &pool,
            id,
            &object_ref,
            &digest,
            "application/octet-stream",
            bytes.len() as i64,
            "system:submission-export-v2",
        )
        .await
        .unwrap();
    }
    let blob = platform::write_blob_async(&digest, &bytes).await.unwrap();
    let storage = platform::oxana_connect().unwrap();
    let first_job = storage
        .enqueue(
            platform::RetentionQueue,
            platform::ObjectUploadExpireJob { staging_id: first },
        )
        .await
        .unwrap();
    let duplicate_job = storage
        .enqueue(
            platform::RetentionQueue,
            platform::ObjectUploadExpireJob { staging_id: first },
        )
        .await
        .unwrap();
    assert_ne!(
        first_job, duplicate_job,
        "each handoff must acknowledge a new native envelope, not Skip"
    );
    for id in [&first_job, &duplicate_job] {
        let envelope = storage.get_job(id).await.unwrap().unwrap();
        assert_eq!(envelope.job.args["staging_id"], first.to_string());
        assert!(!envelope.meta.unique);
    }
    let consumer = start_retention_consumer().await;
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let released: bool = sqlx::query_scalar(
                "SELECT NOT EXISTS(SELECT 1 FROM object_upload_staging WHERE id=$1)",
            )
            .bind(first)
            .fetch_one(&pool)
            .await
            .unwrap();
            if released
                && storage.get_job(&first_job).await.unwrap().is_none()
                && storage.get_job(&duplicate_job).await.unwrap().is_none()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("both real duplicate upload consumers finish");
    let protected: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM object_upload_staging WHERE id=$1)
          AND EXISTS(SELECT 1 FROM object_registry WHERE object_ref=$2 AND state='available')
          AND NOT EXISTS(SELECT 1 FROM object_deletion_artifacts WHERE object_ref=$2)
          AND NOT EXISTS(SELECT 1 FROM object_retention_tombstones WHERE object_ref=$2)",
    )
    .bind(remaining_owner)
    .bind(&object_ref)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        protected,
        "the remaining reference must refuse physical deletion"
    );
    assert_eq!(std::fs::read(&blob).unwrap(), bytes);

    // A directory at the exact blob path makes the real filesystem deletion fail.
    // Preserve the actual bytes, then repair only this owned fixture after observing native retry.
    let saved = blob.with_extension("retry-bytes");
    std::fs::rename(&blob, &saved).unwrap();
    std::fs::create_dir(&blob).unwrap();
    let retry_job = storage
        .enqueue(
            platform::RetentionQueue,
            platform::ObjectUploadExpireJob {
                staging_id: remaining_owner,
            },
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let envelope = storage
                .get_job(&retry_job)
                .await
                .unwrap()
                .expect("failed job must survive");
            if envelope.meta.retries > 0 {
                assert!(
                    envelope
                        .meta
                        .error
                        .as_deref()
                        .is_some_and(|error| error.contains("Is a directory")),
                    "retry must be caused by the injected filesystem failure: {:?}",
                    envelope.meta.error
                );
                eprintln!(
                    "native retry observed job={} staging={} retries={}",
                    retry_job, remaining_owner, envelope.meta.retries
                );
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("real deletion error must enter Oxana native retry");
    let recoverable: bool = sqlx::query_scalar(
        "SELECT NOT EXISTS(SELECT 1 FROM object_upload_staging WHERE id=$1)
          AND EXISTS(SELECT 1 FROM object_deletion_artifacts WHERE id=$1 AND object_ref=$2 AND digest=$3)
          AND EXISTS(SELECT 1 FROM object_registry WHERE object_ref=$2 AND state='deleting')
          AND NOT EXISTS(SELECT 1 FROM object_retention_tombstones WHERE deletion_id=$1)")
        .bind(remaining_owner).bind(&object_ref).bind(&digest).fetch_one(&pool).await.unwrap();
    assert!(
        recoverable,
        "failed physical deletion preserves the immutable recovery identity, not a receipt"
    );
    assert_eq!(std::fs::read(&saved).unwrap(), bytes);
    std::fs::remove_dir(&blob).unwrap();
    std::fs::rename(&saved, &blob).unwrap();
    // No re-enqueue, SQL expiry/deletion call, or custom retry budget: same native job resumes.
    wait_for_deleted(&pool, remaining_owner, &digest, bytes.len() as i64)
        .await
        .expect("native retry must finish deletion with the original identity");

    let deletion = platform::ObjectRetentionJob {
        deletion_id: remaining_owner,
        object_ref,
        digest: digest.clone(),
        byte_length: bytes.len() as i64,
    };
    let one = storage
        .enqueue(platform::RetentionQueue, deletion.clone())
        .await
        .unwrap();
    let two = storage
        .enqueue(platform::RetentionQueue, deletion)
        .await
        .unwrap();
    assert_ne!(one, two);
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if storage.get_job(&one).await.unwrap().is_none()
                && storage.get_job(&two).await.unwrap().is_none()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("both real retention duplicates finish");

    let live_staging = Uuid::new_v4();
    let owner = Uuid::new_v4();
    let live_bytes = format!("persistent owner {owner}").into_bytes();
    let live_digest = platform::sha256_hex(&live_bytes);
    let live_ref = platform::object_ref(&live_digest);
    platform::stage_object_upload(
        &pool,
        live_staging,
        &live_ref,
        &live_digest,
        "application/octet-stream",
        live_bytes.len() as i64,
        "system:submission-export-v2",
    )
    .await
    .unwrap();
    let live_blob = platform::write_blob_async(&live_digest, &live_bytes)
        .await
        .unwrap();
    sqlx::query("SELECT kb_object_reference_add($1::kb_object_ref,$2::kb_sha256,'application/octet-stream',$3,
        'cleanup_test_owner',$4,'payload','system:submission-export-v2')")
        .bind(&live_ref).bind(&live_digest).bind(live_bytes.len() as i64).bind(owner)
        .execute(&pool).await.unwrap();
    let live_job = storage
        .enqueue(
            platform::RetentionQueue,
            platform::ObjectUploadExpireJob {
                staging_id: live_staging,
            },
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let released: bool = sqlx::query_scalar(
                "SELECT NOT EXISTS(SELECT 1 FROM object_upload_staging WHERE id=$1)",
            )
            .bind(live_staging)
            .fetch_one(&pool)
            .await
            .unwrap();
            if released && storage.get_job(&live_job).await.unwrap().is_none() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("real consumer releases staging with a live business owner");
    let live_protected: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM object_owner_references WHERE object_ref=$1 AND owner_id=$2 AND owner_kind='cleanup_test_owner')
          AND EXISTS(SELECT 1 FROM object_registry WHERE object_ref=$1 AND state='available')
          AND NOT EXISTS(SELECT 1 FROM object_deletion_artifacts WHERE object_ref=$1)
          AND NOT EXISTS(SELECT 1 FROM object_retention_tombstones WHERE object_ref=$1)")
        .bind(&live_ref).bind(owner).fetch_one(&pool).await.unwrap();
    assert!(live_protected);
    assert_eq!(std::fs::read(&live_blob).unwrap(), live_bytes);
    // Release the fixture's business ownership, never emulate the consumer's physical deletion.
    let released: Value = sqlx::query_scalar(
        "SELECT kb_object_reference_remove($1::kb_object_ref,'cleanup_test_owner',$2,'payload',$3)",
    )
    .bind(&live_ref)
    .bind(owner)
    .bind(live_staging)
    .fetch_one(&pool)
    .await
    .unwrap();
    let deletion: platform::ObjectDeletionIdentity = serde_json::from_value(released).unwrap();
    platform::dispatch_object_deletion(&pool, deletion)
        .await
        .unwrap();
    wait_for_deleted(&pool, live_staging, &live_digest, live_bytes.len() as i64)
        .await
        .expect("released live owner must reach the real retention consumer and receipt");
    consumer.finish().await;
    let receipts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM object_retention_tombstones WHERE deletion_id=$1")
            .bind(remaining_owner)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(receipts, 1);
    assert!(!blob.exists());
    release_delivery_test_lock(&mut test_lock).await;
}

// Own the fault thread before any child spawn/assertion can unwind. Stop is observed
// even before accept/read; Drop must never panic over the original test failure.
struct HandoffFaultServer {
    address: std::net::SocketAddr,
    request: std::sync::mpsc::Receiver<()>,
    stop: std::sync::mpsc::Sender<()>,
    task: Option<std::thread::JoinHandle<std::io::Result<()>>>,
}

impl HandoffFaultServer {
    fn start() -> std::io::Result<Self> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        listener.set_nonblocking(true)?;
        let (request_tx, request) = std::sync::mpsc::channel();
        let (stop, stop_rx) = std::sync::mpsc::channel();
        let task = std::thread::Builder::new()
            .name("handoff-fault-server".into())
            .spawn(move || {
                use std::io::Read;
                let deadline = std::time::Instant::now() + Duration::from_secs(10);
                let mut stream = None;
                let mut received = false;
                loop {
                    match stop_rx.recv_timeout(Duration::from_millis(10)) {
                        Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                            return Ok(());
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    }
                    if std::time::Instant::now() >= deadline {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "fault fixture stop deadline",
                        ));
                    }
                    if stream.is_none() {
                        match listener.accept() {
                            Ok((connection, _)) => {
                                connection.set_nonblocking(true)?;
                                stream = Some(connection);
                            }
                            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                            Err(error) => return Err(error),
                        }
                    }
                    if let Some(connection) = stream.as_mut().filter(|_| !received) {
                        match connection.read(&mut [0]) {
                            Ok(0) => {
                                return Err(std::io::Error::new(
                                    std::io::ErrorKind::UnexpectedEof,
                                    "fault client closed before request",
                                ));
                            }
                            Ok(_) => {
                                received = true;
                                request_tx.send(()).map_err(std::io::Error::other)?;
                            }
                            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                            Err(error) => return Err(error),
                        }
                    }
                    // Hold the real connection without replying until stop, not child EOF.
                }
            })?;
        Ok(Self {
            address,
            request,
            stop,
            task: Some(task),
        })
    }

    fn stop_and_join(&mut self) -> std::io::Result<()> {
        let Some(task) = self.task.take() else {
            return Ok(());
        };
        let _ = self.stop.send(());
        let result = task
            .join()
            .map_err(|_| std::io::Error::other("fault server panicked"))
            .and_then(|result| result);
        use std::io::Write;
        let _ = writeln!(
            std::io::stderr(),
            "fault server stopped/joined address={} result={result:?}",
            self.address
        );
        result
    }

    fn finish(mut self) {
        self.stop_and_join()
            .expect("fault server stopped and joined");
    }
}

impl Drop for HandoffFaultServer {
    fn drop(&mut self) {
        if let Err(error) = self.stop_and_join() {
            use std::io::Write;
            let _ = writeln!(std::io::stderr(), "fault server cleanup error: {error}");
        }
    }
}

#[test]
fn handoff_fault_server_reclaims_socket_after_child_failure() {
    const CHILD: &str = "KB_HANDOFF_FAULT_FAILURE_CHILD";
    if let Ok(address) = std::env::var(CHILD) {
        if address != "before-connect" {
            use std::io::Write;
            let mut stream = std::net::TcpStream::connect(address).unwrap();
            stream.write_all(b"*").unwrap();
        }
        panic!("intentional handoff child failure");
    }
    for mode in ["spawn-error", "before-connect", "after-connect"] {
        let server = HandoffFaultServer::start().unwrap();
        let address = server.address;
        let observed_failure = std::cell::Cell::new(false);
        let observed = &observed_failure;
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            // Move ownership into the unwinding scope, as in the async parent test.
            let server = server;
            let executable = if mode == "spawn-error" {
                std::env::current_exe().unwrap().join("nonexistent-child")
            } else {
                std::env::current_exe().unwrap()
            };
            let output = std::process::Command::new(executable)
                .env_clear()
                .env(
                    CHILD,
                    if mode == "after-connect" {
                        address.to_string()
                    } else {
                        "before-connect".into()
                    },
                )
                .args([
                    "--exact",
                    "handoff_fault_server_reclaims_socket_after_child_failure",
                    "--nocapture",
                ])
                .output();
            eprintln!(
                "failure probe mode={mode} spawn={:?}",
                output.as_ref().map(|out| out.status)
            );
            if mode == "spawn-error" {
                assert!(
                    output
                        .as_ref()
                        .is_err_and(|error| error.kind() == std::io::ErrorKind::NotADirectory)
                );
                observed.set(true);
            }
            let output = output.expect("intentional child startup failure");
            eprintln!("{}", String::from_utf8_lossy(&output.stdout));
            eprintln!("{}", String::from_utf8_lossy(&output.stderr));
            if mode == "after-connect" {
                server.request.recv_timeout(Duration::from_secs(1)).unwrap();
            }
            assert_eq!(output.status.code(), Some(101));
            assert!(
                String::from_utf8_lossy(&output.stderr)
                    .contains("intentional handoff child failure")
            );
            observed.set(true);
            assert!(
                output.status.success(),
                "intentional parent assertion on child failure"
            );
        }));
        assert!(failure.is_err(), "probe must really unwind");
        assert!(
            observed_failure.get(),
            "only the intended failure may satisfy the probe"
        );
        let rebound = std::net::TcpListener::bind(address).expect("Drop released fault socket");
        eprintln!(
            "failure probe mode={mode} unwind observed; socket rebound={}",
            rebound.local_addr().unwrap()
        );
    }
}

#[tokio::test]
async fn cleanup_unconfirmed_handoff_keeps_staging_blob_and_tracker() {
    let Some(pool) = support::connect_postgres_contract("unconfirmed cleanup handoff").await else {
        return;
    };
    require_local_objects();
    if let Ok(mode) = std::env::var("KB_CLEANUP_HANDOFF_CHILD") {
        let ids: Vec<Uuid> = std::env::var("KB_CLEANUP_STAGING_IDS")
            .unwrap()
            .split(',')
            .map(|id| Uuid::parse_str(id).unwrap())
            .collect();
        assert_eq!(ids.len(), 2);
        let tracker =
            platform::StagedObjectCleanupTracker::new(&pool, "system:submission-export-v2");
        for id in &ids {
            let bytes = format!("unconfirmed cleanup {id}").into_bytes();
            let digest = platform::sha256_hex(&bytes);
            platform::stage_object_upload(
                &pool,
                *id,
                &platform::object_ref(&digest),
                &digest,
                "application/octet-stream",
                bytes.len() as i64,
                "system:submission-export-v2",
            )
            .await
            .unwrap();
            platform::write_blob_async(&digest, &bytes).await.unwrap();
            tracker.register(*id);
        }
        let deadline = tokio::time::Instant::now() + Duration::from_millis(200);
        let result = tokio::time::timeout_at(deadline, tracker.cleanup_pending()).await;
        match mode.as_str() {
            "configuration-error" => {
                let error = result.unwrap().unwrap_err();
                assert!(
                    error.contains(&ids[0].to_string()),
                    "first identity must fail first: {error}"
                );
            }
            "unresponsive-redis" => assert!(
                result.is_err(),
                "unconfirmed handoff must exhaust only its absolute deadline"
            ),
            _ => panic!("unknown handoff fixture mode"),
        }
        assert_eq!(
            tracker.pending_staging_ids(),
            ids,
            "failure/cancellation must retain both identities in order"
        );
        for id in &ids {
            let retained: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM object_upload_staging WHERE id=$1)",
            )
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert!(retained);
            let bytes = format!("unconfirmed cleanup {id}").into_bytes();
            assert_eq!(
                std::fs::read(
                    platform::blob_path(&platform::sha256_hex(&bytes))
                        .expect("valid configured test object path")
                )
                .unwrap(),
                bytes
            );
        }
        eprintln!("unconfirmed mode={mode} retained staging={ids:?} tracker=2 blobs=true");
        return;
    }
    let mut test_lock = acquire_delivery_test_lock(&pool).await;
    // Each subprocess owns its environment, so the normal suite's real Redis is never changed.
    let server = HandoffFaultServer::start().unwrap();
    let unresponsive = format!("redis://{}/", server.address);
    let mut staged = Vec::new();
    for (mode, redis) in [
        ("configuration-error", "not-a-redis-url"),
        ("unresponsive-redis", unresponsive.as_str()),
    ] {
        let ids = [Uuid::new_v4(), Uuid::new_v4()];
        staged.extend(ids);
        let mut child = std::process::Command::new(std::env::current_exe().unwrap());
        child
            .env_clear()
            .args([
                "--exact",
                "cleanup_unconfirmed_handoff_keeps_staging_blob_and_tracker",
                "--nocapture",
            ])
            .env("KB_CLEANUP_STAGING_IDS", format!("{},{}", ids[0], ids[1]))
            .env("KB_CLEANUP_HANDOFF_CHILD", mode)
            .env("REDIS_URL", redis);
        for key in [
            "KNOWLEDGEBRAIN_TEST_DATABASE_URL",
            "KNOWLEDGEBRAIN_REQUIRE_POSTGRES_TESTS",
            "OBJECT_DIR",
            "KB_DEPLOYMENT_NAMESPACE_ID",
        ] {
            child.env(
                key,
                std::env::var_os(key).expect("explicit isolated child configuration"),
            );
        }
        let output = tokio::task::spawn_blocking(move || child.output())
            .await
            .unwrap()
            .unwrap();
        eprintln!("{}", String::from_utf8_lossy(&output.stdout));
        eprintln!("{}", String::from_utf8_lossy(&output.stderr));
        assert!(
            output.status.success(),
            "unconfirmed handoff child {mode} failed: {}",
            output.status
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed; 0 ignored;"));
    }
    server
        .request
        .recv_timeout(Duration::from_secs(1))
        .expect("Redis request actually arrived before cancellation");
    server.finish();
    let tracker = platform::StagedObjectCleanupTracker::new(&pool, "system:submission-export-v2");
    for id in &staged {
        tracker.register(*id);
    }
    tokio::time::timeout(Duration::from_secs(5), tracker.cleanup_pending())
        .await
        .unwrap()
        .unwrap();
    assert!(!tracker.has_pending());
    let consumer = start_retention_consumer().await;
    for id in staged {
        let bytes = format!("unconfirmed cleanup {id}").into_bytes();
        wait_for_deleted(&pool, id, &platform::sha256_hex(&bytes), bytes.len() as i64)
            .await
            .expect("explicit replay must recover the retained staging identity");
    }
    consumer.finish().await;
    release_delivery_test_lock(&mut test_lock).await;
}
