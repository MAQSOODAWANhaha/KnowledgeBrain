use bid::matching::run_match_route_v1;
use runtime::{BidMatchRouteV1Job, BidMatchRouteV1Snapshots};
use serde_json::json;
use sqlx::{PgPool, Row};
use storage::bid_matching::{
    PickSelectionV1, PublishRouteV2, ReplaceRoutePickSetV1, ScheduleEnvironment,
    StagedSourceArtifactV1,
};
use uuid::Uuid;

mod support;

fn empty_publish_route() -> PublishRouteV2 {
    PublishRouteV2 {
        report_id: Uuid::new_v4(),
        report_nonce: Uuid::new_v4(),
        canonical_payload: b"{}".to_vec(),
        sources: Vec::new(),
        candidates: Vec::new(),
        evidences: Vec::new(),
        decisions: Vec::new(),
        candidate_groups: Vec::new(),
        reason_codes: Vec::new(),
    }
}

fn assert_database_error(error: sqlx::Error, expected: &str) {
    let message = error
        .as_database_error()
        .map(|error| error.message().to_string())
        .unwrap_or_else(|| error.to_string());
    assert!(
        message.contains(expected),
        "expected database error {expected}, got {message}"
    );
}

fn matching_schedule_payload_without_eligible_scope(
    project_id: Uuid,
    watermark: i64,
    clause_id: Uuid,
    requirement: &str,
) -> serde_json::Value {
    let manifest_id = Uuid::new_v4();
    let technical_route_id = Uuid::new_v4();
    let commercial_route_id = Uuid::new_v4();
    let requirement_sha256 = domain::sha256_hex(requirement.as_bytes());
    let requirement_set_sha256 = domain::sha256_hex(
        &serde_json::to_vec(&vec![(
            technical_route_id,
            clause_id,
            requirement,
            requirement_sha256.as_str(),
        )])
        .unwrap(),
    );
    json!({
        "schema_version": 1,
        "manifest_id": manifest_id,
        "project_id": project_id,
        "mutation_watermark": watermark,
        "requirement_set_sha256": requirement_set_sha256,
        "eligible_scope_sha256": domain::sha256_hex(b"[]"),
        "routes": [
            {
                "id": technical_route_id,
                "route_kind": "technical",
                "unit_id": Uuid::nil(),
                "ordinal": 0,
                "empty_policy": "clear_route",
                "route_scope_sha256": domain::sha256_hex(b"technical:00000000-0000-0000-0000-000000000000")
            },
            {
                "id": commercial_route_id,
                "route_kind": "commercial",
                "unit_id": null,
                "ordinal": 1,
                "empty_policy": "clear_route",
                "route_scope_sha256": domain::sha256_hex(b"commercial")
            }
        ],
        "requirements": [{
            "id": Uuid::new_v4(),
            "route_id": technical_route_id,
            "clause_id": clause_id,
            "ordinal": 0,
            "text": requirement,
            "sha256": requirement_sha256
        }],
        "products": [],
        "memberships": [],
        "frozen_hits": []
    })
}

async fn assert_matching_schedule_rejected(
    pool: &PgPool,
    project_id: Uuid,
    payload: &serde_json::Value,
    idempotency_key: &str,
    expected: &str,
) {
    let request_bytes = serde_json::to_vec(payload).unwrap();
    let request_sha256 = domain::sha256_hex(&request_bytes);
    let mut tx = pool.begin().await.unwrap();
    let result = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT kb_bid_matching_schedule($1,1,3,$2,'system:matching-publication',$3,$4,$5)",
    )
    .bind(project_id)
    .bind(payload)
    .bind(idempotency_key)
    .bind(&request_bytes)
    .bind(&request_sha256)
    .fetch_one(&mut *tx)
    .await;
    let error = match result {
        Err(error) => {
            tx.rollback().await.unwrap();
            error
        }
        Ok(_) => tx
            .commit()
            .await
            .expect_err("invalid matching schedule payload must not commit"),
    };
    assert_database_error(error, expected);
}

async fn wait_for_lock_wait(pool: &PgPool, application_name: &str) {
    for _ in 0..100 {
        let waiting: bool = sqlx::query_scalar(
            "SELECT EXISTS(
               SELECT 1 FROM pg_stat_activity
                WHERE application_name=$1 AND wait_event_type='Lock')",
        )
        .bind(application_name)
        .fetch_one(pool)
        .await
        .unwrap();
        if waiting {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("matching settlement must reach the job lock");
}

async fn expire_claim_after_function_entry(pool: &PgPool, job_id: Uuid, attempt: i32) {
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    sqlx::query(
        "UPDATE bid_matching_job_claims
            SET claim_lease_ms=1000,
                heartbeat_at=clock_timestamp()-interval '1001 milliseconds'
          WHERE job_id=$1 AND attempt=$2",
    )
    .bind(job_id)
    .bind(attempt)
    .execute(pool)
    .await
    .unwrap();
}

async fn restore_claim_lease(pool: &PgPool, job_id: Uuid, attempt: i32, claim_lease_ms: i32) {
    sqlx::query(
        "UPDATE bid_matching_job_claims
            SET claim_lease_ms=$3,heartbeat_at=clock_timestamp()
          WHERE job_id=$1 AND attempt=$2",
    )
    .bind(job_id)
    .bind(attempt)
    .bind(claim_lease_ms)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn matching_publication_freezes_evidence_and_builds_unsectioned_pick_sets() {
    let Some(pool) = support::connect_postgres_contract("MatchingPublication").await else {
        return;
    };
    let final_schema: bool = sqlx::query_scalar(
        "SELECT to_regclass('public.bid_matching_frozen_retrieved_hits') IS NOT NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap_or(false);
    if !support::require_final_schema("MatchingPublication", final_schema) {
        return;
    }

    let user_id = Uuid::new_v4();
    let workspace_id = Uuid::new_v4();
    let product_id = Uuid::new_v4();
    let version_id = Uuid::new_v4();
    let document_id = Uuid::new_v4();
    let chunk_id = Uuid::new_v4();
    let second_product_id = Uuid::new_v4();
    let second_version_id = Uuid::new_v4();
    let second_document_id = Uuid::new_v4();
    let second_chunk_id = Uuid::new_v4();
    let project_id = Uuid::new_v4();
    let scope_project_id = Uuid::new_v4();
    let clause_id = Uuid::new_v4();
    let scope_clause_id = Uuid::new_v4();
    let ordinary_clause_id = Uuid::new_v4();
    let tender_document_id = Uuid::new_v4();
    let source_artifact_id = Uuid::new_v4();
    let ordinary_unit_id = Uuid::new_v4();
    let source_span_id = Uuid::new_v4();
    let actor = format!("user:{user_id}");
    let file_bytes = b"manual";
    let digest = domain::sha256_hex(file_bytes);
    let object_ref = format!("objects/{digest}");
    let requirement = "支持国密算法";
    let ordinary_requirement = "设备应支持国密算法";
    let chunk = "产品说明：设备应支持国密算法，并提供配置说明。";

    let mut seed = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO users(id,email) VALUES($1,$2)")
        .bind(user_id)
        .bind(format!("{user_id}@matching.invalid"))
        .execute(&mut *seed)
        .await
        .unwrap();
    sqlx::query("INSERT INTO workspaces(id,name,slug,kind) VALUES($1,'产品线',$2,'product_line')")
        .bind(workspace_id)
        .bind(format!("matching-{workspace_id}"))
        .execute(&mut *seed)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO products(id,workspace_id,kind,name,slug) VALUES($1,$2,'product','防火墙',$3)",
    )
    .bind(product_id)
    .bind(workspace_id)
    .bind(format!("firewall-{product_id}"))
    .execute(&mut *seed)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO product_versions(id,product_id,label,status) VALUES($1,$2,'v1','active')",
    )
    .bind(version_id)
    .bind(product_id)
    .execute(&mut *seed)
    .await
    .unwrap();
    sqlx::query("UPDATE products SET current_version_id=$2 WHERE id=$1")
        .bind(product_id)
        .bind(version_id)
        .execute(&mut *seed)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO products(id,workspace_id,kind,name,slug) VALUES($1,$2,'product','密码机',$3)",
    )
    .bind(second_product_id)
    .bind(workspace_id)
    .bind(format!("crypto-appliance-{second_product_id}"))
    .execute(&mut *seed)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO product_versions(id,product_id,label,status) VALUES($1,$2,'v1','active')",
    )
    .bind(second_version_id)
    .bind(second_product_id)
    .execute(&mut *seed)
    .await
    .unwrap();
    sqlx::query("UPDATE products SET current_version_id=$2 WHERE id=$1")
        .bind(second_product_id)
        .bind(second_version_id)
        .execute(&mut *seed)
        .await
        .unwrap();
    sqlx::query("SELECT kb_object_reference_add($1,$2,'application/pdf',$3,'knowledge_document',$4,'original',$5)")
        .bind(&object_ref)
        .bind(&digest)
        .bind(file_bytes.len() as i64)
        .bind(document_id)
        .bind(&actor)
        .execute(&mut *seed)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO documents
         (id,product_version_id,title,parse_status,enable_status,index_ready,file_name,file_size,file_hash,object_ref)
         VALUES($1,$2,'产品手册','completed','enabled',true,'国密手册.pdf',$3,$4,$5)",
    )
    .bind(document_id)
    .bind(version_id)
    .bind(file_bytes.len() as i64)
    .bind(&digest)
    .bind(&object_ref)
    .execute(&mut *seed)
    .await
    .unwrap();
    sqlx::query("SELECT kb_object_reference_add($1,$2,'application/pdf',$3,'knowledge_document',$4,'original',$5)")
        .bind(&object_ref)
        .bind(&digest)
        .bind(file_bytes.len() as i64)
        .bind(second_document_id)
        .bind(&actor)
        .execute(&mut *seed)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO documents
         (id,product_version_id,title,parse_status,enable_status,index_ready,file_name,file_size,file_hash,object_ref)
         VALUES($1,$2,'密码机手册','completed','enabled',true,'密码机国密手册.pdf',$3,$4,$5)",
    )
    .bind(second_document_id)
    .bind(second_version_id)
    .bind(file_bytes.len() as i64)
    .bind(&digest)
    .bind(&object_ref)
    .execute(&mut *seed)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chunks(id,product_version_id,document_id,chunk_type,content,start_at,end_at)
         VALUES($1,$2,$3,'text',$4,0,$5)",
    )
    .bind(second_chunk_id)
    .bind(second_version_id)
    .bind(second_document_id)
    .bind(chunk)
    .bind(chunk.len() as i32)
    .execute(&mut *seed)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chunks(id,product_version_id,document_id,chunk_type,content,start_at,end_at)
         VALUES($1,$2,$3,'text',$4,0,$5)",
    )
    .bind(chunk_id)
    .bind(version_id)
    .bind(document_id)
    .bind(chunk)
    .bind(chunk.len() as i32)
    .execute(&mut *seed)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO bid_projects
         (id,title,owner_user_id,ends_at,fact_sha256,ceiling_identity_sha256,
          matching_mutation_watermark,created_by)
         VALUES($1,'国密投标',$2,clock_timestamp()+interval '30 days',repeat('0',64),repeat('1',64),1,$3)",
    )
    .bind(project_id)
    .bind(user_id)
    .bind(&actor)
    .execute(&mut *seed)
    .await
    .unwrap();
    let tender_digest = "3".repeat(64);
    let source_bytes = ordinary_requirement.as_bytes();
    let source_digest = domain::sha256_hex(source_bytes);
    let section_key = "section:ordinary";
    let heading_path = json!(["技术要求"]);
    sqlx::query(
        "INSERT INTO bid_documents
         (id,project_id,file_name,media_type,byte_length,original_object_ref,original_sha256,
          conversion_generation,parse_status)
         VALUES($1,$2,'ordinary.md','text/markdown',$3,$4,$5,1,'completed')",
    )
    .bind(tender_document_id)
    .bind(project_id)
    .bind(source_bytes.len() as i64)
    .bind(format!("objects/{tender_digest}"))
    .bind(&tender_digest)
    .execute(&mut *seed)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO bid_converted_source_artifacts
         (id,project_id,document_id,conversion_generation,original_object_ref,original_sha256,
          canonical_markdown_utf8,markdown_sha256,byte_length,converter_contract_version,
          image_asset_set_sha256)
         VALUES($1,$2,$3,1,$4,$5,$6,$7,$8,'matching-test-v1',repeat('4',64))",
    )
    .bind(source_artifact_id)
    .bind(project_id)
    .bind(tender_document_id)
    .bind(format!("objects/{tender_digest}"))
    .bind(&tender_digest)
    .bind(source_bytes)
    .bind(&source_digest)
    .bind(source_bytes.len() as i64)
    .execute(&mut *seed)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO bid_section_artifacts
         (id,project_id,document_id,source_artifact_id,conversion_generation,section_key,
          heading_path,parent_start_offset,parent_end_offset,section_sha256)
         VALUES($1,$2,$3,$4,1,$5,$6,0,$7,$8)",
    )
    .bind(ordinary_unit_id)
    .bind(project_id)
    .bind(tender_document_id)
    .bind(source_artifact_id)
    .bind(section_key)
    .bind(&heading_path)
    .bind(source_bytes.len() as i64)
    .bind(&source_digest)
    .execute(&mut *seed)
    .await
    .unwrap();
    let source_span = json!({
        "schema_version": 2,
        "source_artifact_id": source_artifact_id,
        "section_artifact_id": ordinary_unit_id,
        "project_id": project_id,
        "document_id": tender_document_id,
        "conversion_generation": 1,
        "section_key": section_key,
        "parent_start_offset": 0,
        "parent_end_offset": source_bytes.len(),
        "start_offset": 0,
        "end_offset": source_bytes.len(),
        "offset_unit": "utf8_byte",
        "quote": ordinary_requirement,
        "quote_sha256": source_digest,
        "heading_path": heading_path,
    });
    let source_span_bytes = serde_json::to_vec(&source_span).unwrap();
    let source_span_digest = domain::sha256_hex(&source_span_bytes);
    sqlx::query(
        "INSERT INTO bid_source_span_artifacts
         (id,schema_version,project_id,document_id,source_artifact_id,section_artifact_id,
          conversion_generation,section_key,parent_start_offset,parent_end_offset,start_offset,
          end_offset,offset_unit,quote,quote_sha256,heading_path,source_span_v2,canonical_payload,
          content_sha256)
         VALUES($1,2,$2,$3,$4,$5,1,$6,0,$7,0,$7,'utf8_byte',$8,$9,$10,$11,$12,$13)",
    )
    .bind(source_span_id)
    .bind(project_id)
    .bind(tender_document_id)
    .bind(source_artifact_id)
    .bind(ordinary_unit_id)
    .bind(section_key)
    .bind(source_bytes.len() as i64)
    .bind(ordinary_requirement)
    .bind(&source_digest)
    .bind(&heading_path)
    .bind(&source_span)
    .bind(&source_span_bytes)
    .bind(&source_span_digest)
    .execute(&mut *seed)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE bid_documents SET current_converted_source_artifact_id=$2,parsed_at=clock_timestamp()
         WHERE id=$1",
    )
    .bind(tender_document_id)
    .bind(source_artifact_id)
    .execute(&mut *seed)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO bid_clauses
         (id,project_id,provenance,status,kind,text,must,revision,created_by)
         VALUES($1,$2,'manual','confirmed','technical',$3,true,1,$4)",
    )
    .bind(clause_id)
    .bind(project_id)
    .bind(requirement)
    .bind(&actor)
    .execute(&mut *seed)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO bid_clauses
         (id,project_id,provenance,status,kind,text,must,current_source_span_artifact_id,
          extracted_origin_source_span_artifact_id,revision,created_by)
         VALUES($1,$2,'extracted','confirmed','technical',$3,true,$4,$4,1,$5)",
    )
    .bind(ordinary_clause_id)
    .bind(project_id)
    .bind(ordinary_requirement)
    .bind(source_span_id)
    .bind(&actor)
    .execute(&mut *seed)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO bid_projects
         (id,title,owner_user_id,ends_at,fact_sha256,ceiling_identity_sha256,
          matching_mutation_watermark,created_by)
         VALUES($1,'可信边界测试',$2,clock_timestamp()+interval '30 days',repeat('0',64),repeat('1',64),1,$3)",
    )
    .bind(scope_project_id)
    .bind(user_id)
    .bind(&actor)
    .execute(&mut *seed)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO bid_clauses
         (id,project_id,provenance,status,kind,text,must,revision,created_by)
         VALUES($1,$2,'manual','confirmed','technical',$3,true,1,$4)",
    )
    .bind(scope_clause_id)
    .bind(scope_project_id)
    .bind(requirement)
    .bind(&actor)
    .execute(&mut *seed)
    .await
    .unwrap();
    seed.commit().await.unwrap();

    let schedule_context = storage::bid_matching::ScheduleMutationContext::system();
    let scheduled = storage::bid_matching::schedule_dirty_project(
        &pool,
        project_id,
        ScheduleEnvironment {
            environment: "test".into(),
            max_attempts: 3,
        },
        &schedule_context,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        scheduled.jobs.len(),
        3,
        "ordinary technical + unsectioned technical + commercial routes"
    );
    let replay = storage::bid_matching::schedule_dirty_project(
        &pool,
        project_id,
        ScheduleEnvironment {
            environment: "test".into(),
            max_attempts: 3,
        },
        &schedule_context,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(replay.manifest_id, scheduled.manifest_id);
    assert_eq!(
        replay.jobs.iter().map(|job| job.id).collect::<Vec<_>>(),
        scheduled.jobs.iter().map(|job| job.id).collect::<Vec<_>>()
    );
    let schedule_audit_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE operation='bid.matching.schedule' AND entity_locator->>'project_id'=$1",
    )
    .bind(project_id.to_string())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(schedule_audit_count, 1, "replay must not append audit");

    let cross_project_payload = matching_schedule_payload_without_eligible_scope(
        scope_project_id,
        1,
        clause_id,
        requirement,
    );
    assert_matching_schedule_rejected(
        &pool,
        scope_project_id,
        &cross_project_payload,
        "matching-scope-cross-project-clause",
        "MATCHING_MANIFEST_V1_SCOPE_MISMATCH",
    )
    .await;

    let missing_eligible_payload = matching_schedule_payload_without_eligible_scope(
        scope_project_id,
        1,
        scope_clause_id,
        requirement,
    );
    assert_matching_schedule_rejected(
        &pool,
        scope_project_id,
        &missing_eligible_payload,
        "matching-scope-missing-eligible-products",
        "MATCHING_MANIFEST_V1_SCOPE_MISMATCH",
    )
    .await;
    let rejected_manifest_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM bid_matching_manifests WHERE project_id=$1")
            .bind(scope_project_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(rejected_manifest_count, 0);

    let scope_scheduled = storage::bid_matching::schedule_dirty_project(
        &pool,
        scope_project_id,
        ScheduleEnvironment {
            environment: "test".into(),
            max_attempts: 3,
        },
        &schedule_context,
    )
    .await
    .unwrap()
    .unwrap();
    let scope_route_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM bid_matching_routes
         WHERE manifest_id=$1 AND route_kind='technical' ORDER BY ordinal LIMIT 1",
    )
    .bind(scope_scheduled.manifest_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let next_membership_ordinal: i32 = sqlx::query_scalar(
        "SELECT count(*)::integer FROM bid_matching_route_memberships WHERE route_id=$1",
    )
    .bind(scope_route_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let foreign_product_artifact_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM bid_matching_product_version_artifacts
         WHERE manifest_id=$1 ORDER BY id LIMIT 1",
    )
    .bind(scheduled.manifest_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let mut cross_manifest_tx = pool.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO bid_matching_route_memberships
         (manifest_id,route_id,product_version_artifact_id,route_product_ordinal)
         VALUES($1,$2,$3,$4)",
    )
    .bind(scope_scheduled.manifest_id)
    .bind(scope_route_id)
    .bind(foreign_product_artifact_id)
    .bind(next_membership_ordinal)
    .execute(&mut *cross_manifest_tx)
    .await
    .unwrap();
    let error = cross_manifest_tx
        .commit()
        .await
        .expect_err("cross-manifest membership must not commit");
    assert_database_error(error, "bid_matching_memberships_manifest_product_fk");
    let cross_manifest_membership_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM bid_matching_route_memberships
         WHERE manifest_id=$1 AND route_id=$2 AND product_version_artifact_id=$3",
    )
    .bind(scope_scheduled.manifest_id)
    .bind(scope_route_id)
    .bind(foreign_product_artifact_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(cross_manifest_membership_count, 0);

    let job_routes = sqlx::query(
        "SELECT job.id,route.route_kind,route.unit_id
         FROM bid_matching_jobs job
         JOIN bid_matching_routes route ON route.id=job.route_id
         WHERE job.manifest_id=$1",
    )
    .bind(scheduled.manifest_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    let commercial_job_id: Uuid = job_routes
        .iter()
        .find(|row| row.get::<String, _>("route_kind") == "commercial")
        .unwrap()
        .get("id");
    let ordinary_job_id: Uuid = job_routes
        .iter()
        .find(|row| row.get::<Option<Uuid>, _>("unit_id") == Some(ordinary_unit_id))
        .unwrap()
        .get("id");
    let unsectioned_job_id: Uuid = job_routes
        .iter()
        .find(|row| row.get::<Option<Uuid>, _>("unit_id") == Some(Uuid::nil()))
        .unwrap()
        .get("id");

    let commercial_job = scheduled
        .jobs
        .iter()
        .find(|job| job.id == commercial_job_id)
        .unwrap();
    let lease_claim =
        storage::bid_matching::claim_and_load(&pool, commercial_job.id, commercial_job.snapshots)
            .await
            .unwrap()
            .unwrap();
    sqlx::query(
        "UPDATE bid_matching_job_claims
         SET heartbeat_at=clock_timestamp()-interval '10 minutes'
         WHERE job_id=$1 AND attempt=$2",
    )
    .bind(lease_claim.job_id)
    .bind(lease_claim.claim.attempt)
    .execute(&pool)
    .await
    .unwrap();
    assert!(
        !storage::bid_matching::heartbeat_claim(&pool, &lease_claim)
            .await
            .unwrap()
    );
    let error = storage::bid_matching::publish_route(&pool, &lease_claim, empty_publish_route())
        .await
        .unwrap_err();
    assert_database_error(error, "MATCHING_CLAIM_LOST");
    let error = storage::bid_matching::retry_claim(
        &pool,
        &lease_claim,
        "LEASE_TEST_RETRY",
        "expired lease must be fenced",
    )
    .await
    .unwrap_err();
    assert_database_error(error, "MATCHING_CLAIM_LOST");
    assert_eq!(
        storage::bid_matching::reap_expired_claims(&pool)
            .await
            .unwrap(),
        1
    );
    let replacement_claim =
        storage::bid_matching::claim_and_load(&pool, commercial_job.id, commercial_job.snapshots)
            .await
            .unwrap()
            .expect("reaped matching job must be claimable again");
    assert_eq!(replacement_claim.claim.attempt, 2);

    let error = storage::bid_matching::retry_claim(
        &pool,
        &lease_claim,
        "STALE_ATTEMPT_RETRY",
        "an old attempt must not reset its replacement",
    )
    .await
    .unwrap_err();
    assert_database_error(error, "MATCHING_CLAIM_LOST");
    let error = storage::bid_matching::fail_claim(
        &pool,
        &lease_claim,
        "STALE_ATTEMPT_FAIL",
        "an old attempt must not fail its replacement",
    )
    .await
    .unwrap_err();
    assert_database_error(error, "MATCHING_CLAIM_LOST");

    let lease_status: (String, Option<i32>, String, String) = sqlx::query_as(
        "SELECT job.status,job.active_attempt,old_claim.status,new_claim.status
         FROM bid_matching_jobs job
         JOIN bid_matching_job_claims old_claim
           ON old_claim.job_id=job.id AND old_claim.attempt=$2
         JOIN bid_matching_job_claims new_claim
           ON new_claim.job_id=job.id AND new_claim.attempt=$3
         WHERE job.id=$1",
    )
    .bind(lease_claim.job_id)
    .bind(lease_claim.claim.attempt)
    .bind(replacement_claim.claim.attempt)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        lease_status,
        (
            "running".into(),
            Some(replacement_claim.claim.attempt),
            "reaped".into(),
            "running".into()
        )
    );

    let mut retry_blocker = pool.begin().await.unwrap();
    sqlx::query("SELECT 1 FROM bid_matching_jobs WHERE id=$1 FOR UPDATE")
        .bind(replacement_claim.job_id)
        .execute(&mut *retry_blocker)
        .await
        .unwrap();
    let retry_application_name = format!("matching-retry-{}", Uuid::new_v4());
    let mut retry_connection = pool.acquire().await.unwrap();
    sqlx::query("SELECT set_config('application_name',$1,false)")
        .bind(&retry_application_name)
        .execute(&mut *retry_connection)
        .await
        .unwrap();
    let retry_job_id = replacement_claim.job_id;
    let retry_token = replacement_claim.claim.token;
    let retry_attempt = replacement_claim.claim.attempt;
    let blocked_retry = tokio::spawn(async move {
        sqlx::query("SELECT kb_bid_matching_retry_claim($1,$2,$3,$4,$5)")
            .bind(retry_job_id)
            .bind(retry_token)
            .bind(retry_attempt)
            .bind("LEASE_EXPIRED_WHILE_WAITING")
            .bind("retry must use DB time after acquiring the job lock")
            .execute(&mut *retry_connection)
            .await
    });
    wait_for_lock_wait(&pool, &retry_application_name).await;
    expire_claim_after_function_entry(
        &pool,
        replacement_claim.job_id,
        replacement_claim.claim.attempt,
    )
    .await;
    retry_blocker.commit().await.unwrap();
    assert_database_error(
        blocked_retry.await.unwrap().unwrap_err(),
        "MATCHING_CLAIM_LOST",
    );
    restore_claim_lease(
        &pool,
        replacement_claim.job_id,
        replacement_claim.claim.attempt,
        replacement_claim.claim.claim_lease_ms,
    )
    .await;

    storage::bid_matching::retry_claim(
        &pool,
        &replacement_claim,
        "LEASE_TEST_RETRY",
        "the current claim may retry",
    )
    .await
    .unwrap();

    let ordinary_job = scheduled
        .jobs
        .iter()
        .find(|job| job.id == ordinary_job_id)
        .unwrap();
    let staging_claim =
        storage::bid_matching::claim_and_load(&pool, ordinary_job.id, ordinary_job.snapshots)
            .await
            .unwrap()
            .unwrap();
    let staged_source_id = Uuid::new_v4();
    let staged_source = StagedSourceArtifactV1 {
        id: staged_source_id,
        product_version_artifact_id: Uuid::new_v4(),
        document_id: Uuid::new_v4(),
        source_chunk_id: Uuid::new_v4(),
        frozen_document_display_name: "duplicate staging source".into(),
        chunk_utf8: "x".into(),
        chunk_sha256: domain::sha256_hex(b"x"),
        chunk_byte_length: 1,
        retrieval_rank: 1,
        retrieval_raw_score: "1.000000".into(),
        retrieval_contract_version: "matching-staging-test-v1".into(),
    };
    let mut duplicate_source_report = empty_publish_route();
    duplicate_source_report.sources = vec![staged_source.clone(), staged_source];
    assert!(
        storage::bid_matching::publish_route(&pool, &staging_claim, duplicate_source_report,)
            .await
            .is_err(),
        "duplicate staged identities must reject the batch"
    );
    let staging_before_retry: (String, i64) = sqlx::query_as(
        "SELECT staging.state,
           (SELECT count(*) FROM bid_matching_staged_batches batch
             WHERE batch.staging_set_id=staging.id)
         FROM bid_matching_staging_sets staging
         WHERE staging.job_id=$1 AND staging.attempt=$2",
    )
    .bind(staging_claim.job_id)
    .bind(staging_claim.claim.attempt)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(staging_before_retry, ("active".into(), 0));

    let mut fail_blocker = pool.begin().await.unwrap();
    sqlx::query("SELECT 1 FROM bid_matching_jobs WHERE id=$1 FOR UPDATE")
        .bind(staging_claim.job_id)
        .execute(&mut *fail_blocker)
        .await
        .unwrap();
    let fail_application_name = format!("matching-fail-{}", Uuid::new_v4());
    let mut fail_connection = pool.acquire().await.unwrap();
    sqlx::query("SELECT set_config('application_name',$1,false)")
        .bind(&fail_application_name)
        .execute(&mut *fail_connection)
        .await
        .unwrap();
    let fail_job_id = staging_claim.job_id;
    let fail_token = staging_claim.claim.token;
    let fail_attempt = staging_claim.claim.attempt;
    let blocked_fail = tokio::spawn(async move {
        sqlx::query("SELECT kb_bid_matching_fail_claim($1,$2,$3,$4,$5)")
            .bind(fail_job_id)
            .bind(fail_token)
            .bind(fail_attempt)
            .bind("LEASE_EXPIRED_WHILE_WAITING")
            .bind("failure settlement must use DB time after acquiring the job lock")
            .execute(&mut *fail_connection)
            .await
    });
    wait_for_lock_wait(&pool, &fail_application_name).await;
    expire_claim_after_function_entry(&pool, staging_claim.job_id, staging_claim.claim.attempt)
        .await;
    fail_blocker.commit().await.unwrap();
    assert_database_error(
        blocked_fail.await.unwrap().unwrap_err(),
        "MATCHING_CLAIM_LOST",
    );
    restore_claim_lease(
        &pool,
        staging_claim.job_id,
        staging_claim.claim.attempt,
        staging_claim.claim.claim_lease_ms,
    )
    .await;

    storage::bid_matching::retry_claim(
        &pool,
        &staging_claim,
        "STAGING_TEST_RETRY",
        "partial staging is retryable",
    )
    .await
    .unwrap();
    let staging_after_retry: (String, String, String) = sqlx::query_as(
        "SELECT job.status,claim.status,staging.state
         FROM bid_matching_jobs job
         JOIN bid_matching_job_claims claim ON claim.job_id=job.id AND claim.attempt=$2
         JOIN bid_matching_staging_sets staging ON staging.job_id=job.id AND staging.attempt=claim.attempt
         WHERE job.id=$1",
    )
    .bind(staging_claim.job_id)
    .bind(staging_claim.claim.attempt)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        staging_after_retry,
        ("pending".into(), "failed".into(), "failed".into())
    );

    let unsectioned_job = scheduled
        .jobs
        .iter()
        .find(|job| job.id == unsectioned_job_id)
        .unwrap();
    let commit_claim =
        storage::bid_matching::claim_and_load(&pool, unsectioned_job.id, unsectioned_job.snapshots)
            .await
            .unwrap()
            .unwrap();
    let invalid_commit_report = empty_publish_route();
    let invalid_report_id = invalid_commit_report.report_id;
    let invalid_report_nonce = invalid_commit_report.report_nonce;
    let invalid_report_sha256 = domain::sha256_hex(&invalid_commit_report.canonical_payload);
    assert!(
        storage::bid_matching::publish_route(&pool, &commit_claim, invalid_commit_report)
            .await
            .is_err(),
        "invalid staged report payload must fail during commit"
    );
    let commit_before_retry: (String, i64, i64) = sqlx::query_as(
        "SELECT staging.state,
           (SELECT count(*) FROM bid_matching_staged_batches batch
             WHERE batch.staging_set_id=staging.id),
           (SELECT count(*) FROM bid_matching_reports report WHERE report.job_id=staging.job_id)
         FROM bid_matching_staging_sets staging
         WHERE staging.job_id=$1 AND staging.attempt=$2",
    )
    .bind(commit_claim.job_id)
    .bind(commit_claim.claim.attempt)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(commit_before_retry, ("active".into(), 6, 0));
    let staging_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM bid_matching_staging_sets WHERE job_id=$1 AND attempt=$2",
    )
    .bind(commit_claim.job_id)
    .bind(commit_claim.claim.attempt)
    .fetch_one(&pool)
    .await
    .unwrap();

    let mut commit_blocker = pool.begin().await.unwrap();
    sqlx::query("SELECT 1 FROM bid_matching_jobs WHERE id=$1 FOR UPDATE")
        .bind(commit_claim.job_id)
        .execute(&mut *commit_blocker)
        .await
        .unwrap();
    let commit_application_name = format!("matching-commit-{}", Uuid::new_v4());
    let mut commit_connection = pool.acquire().await.unwrap();
    sqlx::query("SELECT set_config('application_name',$1,false)")
        .bind(&commit_application_name)
        .execute(&mut *commit_connection)
        .await
        .unwrap();
    let commit_job_id = commit_claim.job_id;
    let commit_token = commit_claim.claim.token;
    let commit_attempt = commit_claim.claim.attempt;
    let blocked_report_sha256 = invalid_report_sha256.clone();
    let blocked_commit = tokio::spawn(async move {
        sqlx::query("SELECT kb_bid_matching_commit($1,$2,$3,$4,$5,$6,$7)")
            .bind(commit_job_id)
            .bind(commit_token)
            .bind(commit_attempt)
            .bind(staging_id)
            .bind(invalid_report_id)
            .bind(invalid_report_nonce)
            .bind(blocked_report_sha256)
            .execute(&mut *commit_connection)
            .await
    });
    wait_for_lock_wait(&pool, &commit_application_name).await;
    expire_claim_after_function_entry(&pool, commit_claim.job_id, commit_claim.claim.attempt).await;
    commit_blocker.commit().await.unwrap();
    assert_database_error(
        blocked_commit.await.unwrap().unwrap_err(),
        "MATCHING_CLAIM_LOST",
    );
    restore_claim_lease(
        &pool,
        commit_claim.job_id,
        commit_claim.claim.attempt,
        commit_claim.claim.claim_lease_ms,
    )
    .await;

    sqlx::query(
        "UPDATE bid_matching_job_claims
            SET heartbeat_at=clock_timestamp()-interval '10 minutes'
          WHERE job_id=$1 AND attempt=$2",
    )
    .bind(commit_claim.job_id)
    .bind(commit_claim.claim.attempt)
    .execute(&pool)
    .await
    .unwrap();
    let expired_commit: Result<serde_json::Value, sqlx::Error> =
        sqlx::query_scalar("SELECT kb_bid_matching_commit($1,$2,$3,$4,$5,$6,$7)")
            .bind(commit_claim.job_id)
            .bind(commit_claim.claim.token)
            .bind(commit_claim.claim.attempt)
            .bind(staging_id)
            .bind(invalid_report_id)
            .bind(invalid_report_nonce)
            .bind(&invalid_report_sha256)
            .fetch_one(&pool)
            .await;
    assert_database_error(expired_commit.unwrap_err(), "MATCHING_CLAIM_LOST");
    sqlx::query(
        "UPDATE bid_matching_job_claims SET heartbeat_at=clock_timestamp()
          WHERE job_id=$1 AND attempt=$2",
    )
    .bind(commit_claim.job_id)
    .bind(commit_claim.claim.attempt)
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query("UPDATE bid_matching_staging_sets SET expires_at=clock_timestamp()-interval '1 second' WHERE id=$1")
        .bind(staging_id)
        .execute(&pool)
        .await
        .unwrap();
    let expired_staging_commit: Result<serde_json::Value, sqlx::Error> =
        sqlx::query_scalar("SELECT kb_bid_matching_commit($1,$2,$3,$4,$5,$6,$7)")
            .bind(commit_claim.job_id)
            .bind(commit_claim.claim.token)
            .bind(commit_claim.claim.attempt)
            .bind(staging_id)
            .bind(invalid_report_id)
            .bind(invalid_report_nonce)
            .bind(&invalid_report_sha256)
            .fetch_one(&pool)
            .await;
    assert_database_error(expired_staging_commit.unwrap_err(), "STAGING_NOT_ACTIVE");
    sqlx::query("UPDATE bid_matching_staging_sets SET expires_at=clock_timestamp()+interval '30 minutes' WHERE id=$1")
        .bind(staging_id)
        .execute(&pool)
        .await
        .unwrap();

    sqlx::query(
        "UPDATE bid_projects SET matching_mutation_watermark=matching_mutation_watermark+1 WHERE id=$1",
    )
    .bind(project_id)
    .execute(&pool)
    .await
    .unwrap();
    let stale_inputs_commit: Result<serde_json::Value, sqlx::Error> =
        sqlx::query_scalar("SELECT kb_bid_matching_commit($1,$2,$3,$4,$5,$6,$7)")
            .bind(commit_claim.job_id)
            .bind(commit_claim.claim.token)
            .bind(commit_claim.claim.attempt)
            .bind(staging_id)
            .bind(invalid_report_id)
            .bind(invalid_report_nonce)
            .bind(&invalid_report_sha256)
            .fetch_one(&pool)
            .await;
    assert_database_error(stale_inputs_commit.unwrap_err(), "MATCHING_INPUTS_STALE");
    sqlx::query(
        "UPDATE bid_projects SET matching_mutation_watermark=matching_mutation_watermark-1 WHERE id=$1",
    )
    .bind(project_id)
    .execute(&pool)
    .await
    .unwrap();

    storage::bid_matching::retry_claim(
        &pool,
        &commit_claim,
        "COMMIT_TEST_RETRY",
        "commit failure is retryable",
    )
    .await
    .unwrap();
    let commit_after_retry: (String, String, String) = sqlx::query_as(
        "SELECT job.status,claim.status,staging.state
         FROM bid_matching_jobs job
         JOIN bid_matching_job_claims claim ON claim.job_id=job.id AND claim.attempt=$2
         JOIN bid_matching_staging_sets staging ON staging.job_id=job.id AND staging.attempt=claim.attempt
         WHERE job.id=$1",
    )
    .bind(commit_claim.job_id)
    .bind(commit_claim.claim.attempt)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        commit_after_retry,
        ("pending".into(), "failed".into(), "failed".into())
    );

    for scheduled_job in &scheduled.jobs {
        run_match_route_v1(
            &pool,
            BidMatchRouteV1Job::new(
                scheduled_job.id,
                BidMatchRouteV1Snapshots {
                    config_snapshot_id: scheduled_job.snapshots.config_snapshot_id,
                    feature_snapshot_id: scheduled_job.snapshots.feature_snapshot_id,
                    score_policy_snapshot_id: scheduled_job.snapshots.score_policy_snapshot_id,
                    verifier_policy_snapshot_id: scheduled_job
                        .snapshots
                        .verifier_policy_snapshot_id,
                },
                None,
            )
            .unwrap(),
        )
        .await
        .unwrap();
    }

    let routes = storage::bid_matching::current_routes(&pool, project_id)
        .await
        .unwrap();
    let technical_route = routes
        .iter()
        .find(|row| {
            row.get::<String, _>("route_kind") == "technical"
                && row.get::<Option<Uuid>, _>("unit_id") == Some(Uuid::nil())
        })
        .unwrap();
    assert_eq!(technical_route.get::<Uuid, _>("unit_id"), Uuid::nil());
    let route_id: Uuid = technical_route.get("route_id");
    let report = storage::bid_matching::current_route_report(&pool, project_id, route_id)
        .await
        .unwrap()
        .unwrap();
    let historical_report = storage::bid_matching::matching_report_artifact_json(
        &pool,
        project_id,
        report.get("report_id"),
    )
    .await
    .unwrap()
    .unwrap();
    let canonical_payload = historical_report["canonical_payload"]
        .as_str()
        .expect("historical report must expose the exact canonical UTF-8 bytes");
    assert_eq!(
        domain::sha256_hex(canonical_payload.as_bytes()),
        report.get::<String, _>("content_sha256")
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(canonical_payload).unwrap(),
        historical_report["payload"]
    );
    let candidates =
        storage::bid_matching::current_route_supported_candidates(&pool, project_id, route_id)
            .await
            .unwrap();
    assert_eq!(candidates.len(), 2);
    assert_eq!(
        candidates
            .iter()
            .filter(|candidate| candidate.get::<bool, _>("recommended"))
            .count(),
        1
    );

    let candidate_id: Uuid = candidates[0].get("candidate_artifact_id");
    let second_candidate_id: Uuid = candidates[1].get("candidate_artifact_id");
    let requirement_id: Uuid = candidates[0].get("requirement_artifact_id");
    assert_eq!(
        candidates[1].get::<Uuid, _>("requirement_artifact_id"),
        requirement_id
    );
    sqlx::query(
        "INSERT INTO bid_clause_set_identities(project_id,set_kind,revision,content_sha256,updated_at)
         VALUES($1,'pricing',0,$2,clock_timestamp())
         ON CONFLICT (project_id,set_kind) DO NOTHING",
    )
    .bind(project_id)
    .bind("9".repeat(64))
    .execute(&pool)
    .await
    .unwrap();
    let gate_before_pick: serde_json::Value =
        sqlx::query_scalar("SELECT kb_bid_list_gate_issues($1,'pdf')")
            .bind(project_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        gate_before_pick["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue["code"] == "MATCHING_PICK_MISSING"),
        "a supported technical decision must require an explicit human pick before PDF export"
    );

    let report_identity: (Uuid, Uuid, i64, i64) = sqlx::query_as(
        "SELECT manifest_id,job_id,generation,mutation_watermark
           FROM bid_matching_reports WHERE id=$1",
    )
    .bind(report.get::<Uuid, _>("report_id"))
    .fetch_one(&pool)
    .await
    .unwrap();
    let first_candidate_identity: (Uuid, i32) = sqlx::query_as(
        "SELECT product_version_artifact_id,route_product_ordinal
           FROM bid_matching_candidate_artifacts WHERE id=$1",
    )
    .bind(candidate_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let second_candidate_identity: (Uuid, i32) = sqlx::query_as(
        "SELECT product_version_artifact_id,route_product_ordinal
           FROM bid_matching_candidate_artifacts WHERE id=$1",
    )
    .bind(second_candidate_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let forged_report_id = Uuid::new_v4();
    let forged_candidate_id = Uuid::new_v4();
    let omitted_candidate_id = Uuid::new_v4();
    let forged_generation = report_identity.2 + 1_000;
    let forged_candidate = json!({
        "id": forged_candidate_id,
        "requirement_artifact_id": requirement_id,
        "product_version_artifact_id": first_candidate_identity.0,
        "route_product_ordinal": first_candidate_identity.1,
        "retrieval_rank": 1,
        "retrieval_raw_score": "1.000000",
        "candidate_identity_sha256": "a".repeat(64),
        "evidence_v1_sha256": "c".repeat(64),
        "evidence": {"schema_version": 1, "items": []},
        "support": "supported",
        "business_value": {"status": "not_scored", "reason": "verifier regression"},
        "recommended": true
    });
    let forged_payload = json!({
        "schema_version": 1,
        "report_id": forged_report_id,
        "manifest_id": report_identity.0,
        "job_id": report_identity.1,
        "route_id": route_id,
        "route": historical_report["payload"]["route"].clone(),
        "generation": forged_generation,
        "mutation_watermark": report_identity.3,
        "empty_disposition": null,
        "coverage": {
            "total": 1,
            "eligible": 1,
            "supported": 1,
            "contradicted": 0,
            "insufficient": 0,
            "unresolved": 0
        },
        "quality_status": "pass",
        "degraded": false,
        "reason_codes": ["FROZEN_SCOPE", "SUPPORTED"],
        "score": {"status": "not_scored", "reason": "verifier regression"},
        "requirement_decisions": [{
            "requirement_artifact_id": requirement_id,
            "final_support": "supported",
            "system_decision": "select",
            "quality_status": "pass",
            "reason_code": "SUPPORTED",
            "selected_candidate_artifact_id": forged_candidate_id,
            "business_value": {"status": "not_scored", "reason": "verifier regression"}
        }],
        "candidates": [forged_candidate.clone(), forged_candidate],
        "candidate_groups": [],
        "source_artifacts": [],
        "ai_run_id": null,
        "ai_span_id": null
    });
    let forged_bytes = serde_json::to_vec(&forged_payload).unwrap();
    let forged_sha256 = domain::sha256_hex(&forged_bytes);
    let mut forged = pool.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO bid_matching_reports
         (id,project_id,manifest_id,job_id,route_id,generation,mutation_watermark,empty_disposition,
          coverage_total,coverage_supported,coverage_contradicted,coverage_insufficient,coverage_unresolved,
          quality_status,degraded,reason_codes,canonical_payload,content_sha256,published_at)
         VALUES($1,$2,$3,$4,$5,$6,$7,NULL,1,1,0,0,0,'pass',false,
           ARRAY['FROZEN_SCOPE','SUPPORTED'],$8,$9,clock_timestamp())",
    )
    .bind(forged_report_id)
    .bind(project_id)
    .bind(report_identity.0)
    .bind(report_identity.1)
    .bind(route_id)
    .bind(forged_generation)
    .bind(report_identity.3)
    .bind(forged_bytes)
    .bind(forged_sha256)
    .execute(&mut *forged)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO bid_matching_candidate_artifacts
         (id,report_id,requirement_artifact_id,product_version_artifact_id,support,
          candidate_identity_sha256,evidence_v1_sha256,business_value_status,business_value,
          route_product_ordinal,retrieval_rank,retrieval_raw_score,recommended)
         VALUES($1,$2,$3,$4,'supported',$5,$6,'not_scored',NULL,$7,1,1.000000,$8)",
    )
    .bind(forged_candidate_id)
    .bind(forged_report_id)
    .bind(requirement_id)
    .bind(first_candidate_identity.0)
    .bind("a".repeat(64))
    .bind("c".repeat(64))
    .bind(first_candidate_identity.1)
    .bind(true)
    .execute(&mut *forged)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO bid_matching_candidate_artifacts
         (id,report_id,requirement_artifact_id,product_version_artifact_id,support,
          candidate_identity_sha256,evidence_v1_sha256,business_value_status,business_value,
          route_product_ordinal,retrieval_rank,retrieval_raw_score,recommended)
         VALUES($1,$2,$3,$4,'supported',$5,$6,'not_scored',NULL,$7,1,1.000000,false)",
    )
    .bind(omitted_candidate_id)
    .bind(forged_report_id)
    .bind(requirement_id)
    .bind(second_candidate_identity.0)
    .bind("b".repeat(64))
    .bind("d".repeat(64))
    .bind(second_candidate_identity.1)
    .execute(&mut *forged)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO bid_matching_requirement_decisions
         (id,report_id,requirement_artifact_id,final_support,system_decision,quality_status,
          reason_code,selected_candidate_artifact_id,ordinal)
         VALUES($1,$2,$3,'supported','select','pass','SUPPORTED',$4,0)",
    )
    .bind(Uuid::new_v4())
    .bind(forged_report_id)
    .bind(requirement_id)
    .bind(forged_candidate_id)
    .execute(&mut *forged)
    .await
    .unwrap();
    let forged_verification = sqlx::query("SET CONSTRAINTS bid_matching_reports_verify IMMEDIATE")
        .execute(&mut *forged)
        .await;
    assert_database_error(
        forged_verification.expect_err(
            "a canonical report that duplicates one candidate and omits another must be rejected",
        ),
        "MATCHING_REPORT_V1_PAYLOAD_RELATION_MISMATCH",
    );
    forged.rollback().await.unwrap();

    let noncanonical_route_id = Uuid::new_v4();
    let noncanonical_unit_id = Uuid::new_v4();
    let noncanonical_job_id = Uuid::new_v4();
    let noncanonical_requirement_id = Uuid::new_v4();
    let noncanonical_report_id = Uuid::new_v4();
    let noncanonical_payload = json!({
        "schema_version": 1,
        "report_id": noncanonical_report_id,
        "manifest_id": report_identity.0,
        "job_id": noncanonical_job_id,
        "route_id": noncanonical_route_id,
        "route": {"kind": "commercial"},
        "generation": report_identity.2,
        "mutation_watermark": report_identity.3,
        "empty_disposition": null,
        "coverage": {
            "total": 1,
            "eligible": 0,
            "supported": 0,
            "contradicted": 0,
            "insufficient": 1,
            "unresolved": 0
        },
        "quality_status": "review",
        "degraded": true,
        "reason_codes": ["FROZEN_SCOPE", "NO_EVIDENCE"],
        "score": {"status": "scored", "value": "1.000000"},
        "requirement_decisions": [{
            "requirement_artifact_id": noncanonical_requirement_id,
            "final_support": "insufficient",
            "system_decision": "review",
            "quality_status": "review",
            "reason_code": "NO_EVIDENCE",
            "selected_candidate_artifact_id": null,
            "business_value": {"status": "scored", "value": "1.000000", "source": "forged"}
        }],
        "candidates": [],
        "candidate_groups": [],
        "source_artifacts": [],
        "ai_run_id": null,
        "ai_span_id": null
    });
    let noncanonical_bytes = serde_json::to_vec_pretty(&noncanonical_payload).unwrap();
    let noncanonical_sha256 = domain::sha256_hex(&noncanonical_bytes);
    let mut noncanonical = pool.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO bid_matching_routes
         (id,manifest_id,project_id,route_kind,unit_id,ordinal,empty_policy,route_scope_sha256)
         VALUES($1,$2,$3,'technical',$4,10000,'clear_route',$5)",
    )
    .bind(noncanonical_route_id)
    .bind(report_identity.0)
    .bind(project_id)
    .bind(noncanonical_unit_id)
    .bind("e".repeat(64))
    .execute(&mut *noncanonical)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO bid_matching_jobs
         (id,project_id,manifest_id,route_id,status,max_attempts,claim_lease_ms,lease_policy_generation)
         VALUES($1,$2,$3,$4,'pending',1,300000,1)",
    )
    .bind(noncanonical_job_id)
    .bind(project_id)
    .bind(report_identity.0)
    .bind(noncanonical_route_id)
    .execute(&mut *noncanonical)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO bid_matching_requirement_artifacts
         (id,manifest_id,route_id,clause_id,ordinal,requirement_text,requirement_sha256)
         VALUES($1,$2,$3,$4,0,'noncanonical report regression',$5)",
    )
    .bind(noncanonical_requirement_id)
    .bind(report_identity.0)
    .bind(noncanonical_route_id)
    .bind(clause_id)
    .bind("f".repeat(64))
    .execute(&mut *noncanonical)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO bid_matching_reports
         (id,project_id,manifest_id,job_id,route_id,generation,mutation_watermark,empty_disposition,
          coverage_total,coverage_supported,coverage_contradicted,coverage_insufficient,coverage_unresolved,
          quality_status,degraded,reason_codes,canonical_payload,content_sha256,published_at)
         VALUES($1,$2,$3,$4,$5,$6,$7,NULL,1,0,0,1,0,'review',true,
           ARRAY['FROZEN_SCOPE','NO_EVIDENCE'],$8,$9,clock_timestamp())",
    )
    .bind(noncanonical_report_id)
    .bind(project_id)
    .bind(report_identity.0)
    .bind(noncanonical_job_id)
    .bind(noncanonical_route_id)
    .bind(report_identity.2)
    .bind(report_identity.3)
    .bind(noncanonical_bytes)
    .bind(noncanonical_sha256)
    .execute(&mut *noncanonical)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO bid_matching_requirement_decisions
         (id,report_id,requirement_artifact_id,final_support,system_decision,quality_status,
          reason_code,selected_candidate_artifact_id,ordinal)
         VALUES($1,$2,$3,'insufficient','review','review','NO_EVIDENCE',NULL,0)",
    )
    .bind(Uuid::new_v4())
    .bind(noncanonical_report_id)
    .bind(noncanonical_requirement_id)
    .execute(&mut *noncanonical)
    .await
    .unwrap();
    let noncanonical_verification =
        sqlx::query("SET CONSTRAINTS bid_matching_reports_verify IMMEDIATE")
            .execute(&mut *noncanonical)
            .await;
    assert_database_error(
        noncanonical_verification.expect_err(
            "route, eligible coverage, score, decision business value, key order, and whitespace must be canonical",
        ),
        "MATCHING_REPORT_V1_NON_CANONICAL",
    );
    noncanonical.rollback().await.unwrap();

    let body = json!({
        "source_report_artifact_id": report.get::<Uuid, _>("report_id"),
        "report_sha256": report.get::<String, _>("content_sha256"),
        "expected_revision": 0,
        "items": [
            {"requirement_artifact_id": requirement_id, "candidate_artifact_id": candidate_id},
            {"requirement_artifact_id": requirement_id, "candidate_artifact_id": candidate_id},
            {"requirement_artifact_id": requirement_id, "candidate_artifact_id": second_candidate_id}
        ],
    });
    let context =
        storage::bidding::MutationContext::new(actor.clone(), format!("pick-{project_id}"), &body)
            .unwrap();
    let receipt = storage::bid_matching::replace_route_pick_set(
        &pool,
        ReplaceRoutePickSetV1 {
            project_id,
            route_id,
            source_report_artifact_id: report.get("report_id"),
            report_sha256: report.get("content_sha256"),
            expected_revision: 0,
            selections: vec![
                PickSelectionV1 {
                    requirement_artifact_id: requirement_id,
                    candidate_artifact_id: candidate_id,
                },
                PickSelectionV1 {
                    requirement_artifact_id: requirement_id,
                    candidate_artifact_id: candidate_id,
                },
                PickSelectionV1 {
                    requirement_artifact_id: requirement_id,
                    candidate_artifact_id: second_candidate_id,
                },
            ],
        },
        &context,
    )
    .await
    .unwrap();
    assert_eq!(receipt.route_revision, 1);
    let replay = storage::bid_matching::replace_route_pick_set(
        &pool,
        ReplaceRoutePickSetV1 {
            project_id,
            route_id,
            source_report_artifact_id: report.get("report_id"),
            report_sha256: report.get("content_sha256"),
            expected_revision: 0,
            selections: vec![
                PickSelectionV1 {
                    requirement_artifact_id: requirement_id,
                    candidate_artifact_id: candidate_id,
                },
                PickSelectionV1 {
                    requirement_artifact_id: requirement_id,
                    candidate_artifact_id: candidate_id,
                },
                PickSelectionV1 {
                    requirement_artifact_id: requirement_id,
                    candidate_artifact_id: second_candidate_id,
                },
            ],
        },
        &context,
    )
    .await
    .unwrap();
    assert_eq!(replay.route_pick_set_id, receipt.route_pick_set_id);
    assert_eq!(replay.route_revision, receipt.route_revision);
    assert_eq!(replay.route_sha256, receipt.route_sha256);
    let route_artifact_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM bid_route_pick_set_artifacts WHERE project_id=$1 AND route_id=$2",
    )
    .bind(project_id)
    .bind(route_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        route_artifact_count, 1,
        "idempotent replay must not append an artifact"
    );
    let pick_audit_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE operation='bid.matching.route_pick.replace'
         AND entity_locator->>'project_id'=$1 AND entity_locator->>'route_id'=$2",
    )
    .bind(project_id.to_string())
    .bind(route_id.to_string())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        pick_audit_count, 1,
        "idempotent replay must not append audit"
    );
    let (route_payload, route_digest): (Vec<u8>, String) = sqlx::query_as(
        "SELECT canonical_payload,content_sha256 FROM bid_route_pick_set_artifacts WHERE id=$1",
    )
    .bind(receipt.route_pick_set_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(domain::sha256_hex(&route_payload), route_digest);
    assert_eq!(route_digest, receipt.route_sha256);

    let ordinary_route = routes
        .iter()
        .find(|row| row.get::<Option<Uuid>, _>("unit_id") == Some(ordinary_unit_id))
        .unwrap();
    let ordinary_route_id: Uuid = ordinary_route.get("route_id");
    let ordinary_report =
        storage::bid_matching::current_route_report(&pool, project_id, ordinary_route_id)
            .await
            .unwrap()
            .unwrap();
    let ordinary_candidates = storage::bid_matching::current_route_supported_candidates(
        &pool,
        project_id,
        ordinary_route_id,
    )
    .await
    .unwrap();
    assert_eq!(ordinary_candidates.len(), 2);
    let ordinary_candidate_id: Uuid = ordinary_candidates[0].get("candidate_artifact_id");
    let ordinary_requirement_id: Uuid = ordinary_candidates[0].get("requirement_artifact_id");
    let ordinary_body = json!({
        "source_report_artifact_id": ordinary_report.get::<Uuid, _>("report_id"),
        "report_sha256": ordinary_report.get::<String, _>("content_sha256"),
        "expected_revision": 0,
        "items": [{
            "requirement_artifact_id": ordinary_requirement_id,
            "candidate_artifact_id": ordinary_candidate_id
        }],
    });
    let ordinary_context = storage::bidding::MutationContext::new(
        actor.clone(),
        format!("pick-ordinary-{project_id}"),
        &ordinary_body,
    )
    .unwrap();
    let ordinary_receipt = storage::bid_matching::replace_route_pick_set(
        &pool,
        ReplaceRoutePickSetV1 {
            project_id,
            route_id: ordinary_route_id,
            source_report_artifact_id: ordinary_report.get("report_id"),
            report_sha256: ordinary_report.get("content_sha256"),
            expected_revision: 0,
            selections: vec![PickSelectionV1 {
                requirement_artifact_id: ordinary_requirement_id,
                candidate_artifact_id: ordinary_candidate_id,
            }],
        },
        &ordinary_context,
    )
    .await
    .unwrap();
    assert_eq!(ordinary_receipt.route_revision, 1);
    let gate_after_picks: serde_json::Value =
        sqlx::query_scalar("SELECT kb_bid_list_gate_issues($1,'pdf')")
            .bind(project_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        gate_after_picks["issues"]
            .as_array()
            .unwrap()
            .iter()
            .all(|issue| issue["code"] != "MATCHING_PICK_MISSING"),
        "every supported technical requirement has an explicit current pick"
    );

    let project_units: Vec<Option<Uuid>> = sqlx::query_scalar(
        "SELECT item.unit_id FROM bid_current_project_pick_sets current_value
         JOIN bid_project_pick_set_items item ON item.project_pick_set_id=current_value.pick_set_id
         WHERE current_value.project_id=$1
         ORDER BY item.unit_id",
    )
    .bind(project_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        project_units,
        vec![Some(Uuid::nil()), Some(Uuid::nil()), Some(ordinary_unit_id)]
    );
    let current_route_pick_sets: i64 =
        sqlx::query_scalar("SELECT count(*) FROM bid_current_route_pick_sets WHERE project_id=$1")
            .bind(project_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(current_route_pick_sets, 2);
    let route_pick_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM bid_current_route_pick_sets current_value
         JOIN bid_route_pick_set_items item ON item.pick_set_id=current_value.pick_set_id
         WHERE current_value.project_id=$1 AND current_value.route_id=$2",
    )
    .bind(project_id)
    .bind(route_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        route_pick_count, 2,
        "duplicate selections are canonicalized while two distinct supported picks persist"
    );

    sqlx::query("UPDATE documents SET file_name='已改名.pdf' WHERE id=$1")
        .bind(document_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("SELECT kb_release_knowledge_document_object($1,$2,$3,$4)")
        .bind(document_id)
        .bind(format!("user:{user_id}"))
        .bind(format!("release-{document_id}"))
        .bind(Uuid::new_v4())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM chunks WHERE id=$1")
        .bind(chunk_id)
        .execute(&pool)
        .await
        .unwrap();
    let frozen_name: String = sqlx::query_scalar(
        "SELECT source.frozen_document_display_name FROM bid_matching_source_artifacts source
         JOIN bid_matching_reports report ON report.id=source.report_id
         WHERE report.project_id=$1 AND source.document_id=$2",
    )
    .bind(project_id)
    .bind(document_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(frozen_name, "国密手册.pdf");

    let mutation_body = json!({
        "clause_id": clause_id,
        "action": "patch",
        "expected_revision": 1,
        "patch": {"text": "支持国密算法并提供检测报告"}
    });
    let mutation_context = storage::bidding::MutationContext::new(
        actor.clone(),
        format!("invalidate-matching-{project_id}"),
        &mutation_body,
    )
    .unwrap();
    storage::bidding::mutate_clause(
        &pool,
        project_id,
        clause_id,
        "patch",
        &mutation_body["patch"],
        1,
        &mutation_context,
    )
    .await
    .unwrap();
    let gate_without_current_reports: serde_json::Value =
        sqlx::query_scalar("SELECT kb_bid_list_gate_issues($1,'pdf')")
            .bind(project_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(gate_without_current_reports["status"], "reject");
    assert_eq!(
        gate_without_current_reports["issues"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|issue| issue["code"] == "MATCHING_REPORT_MISSING")
            .count(),
        2,
        "every confirmed technical requirement remains gated after current matching is invalidated"
    );
    let required_after_invalidation = gate_without_current_reports["required_part_keys"]
        .as_array()
        .unwrap();
    assert!(required_after_invalidation.contains(&json!("2:unsectioned")));
    assert!(required_after_invalidation.contains(&json!(format!("2:{ordinary_unit_id}"))));

    sqlx::query("UPDATE bid_projects SET status='ended',ended_at=clock_timestamp() WHERE id=$1")
        .bind(project_id)
        .execute(&pool)
        .await
        .unwrap();
    let historical = storage::bid_matching::matching_report_artifact_json(
        &pool,
        project_id,
        report.get("report_id"),
    )
    .await
    .unwrap()
    .expect("immutable report remains readable after project end");
    assert_eq!(historical["id"], json!(report.get::<Uuid, _>("report_id")));
    assert_eq!(
        historical["content_sha256"],
        json!(report.get::<String, _>("content_sha256"))
    );
    assert_eq!(historical["payload"]["report_id"], historical["id"]);
}
