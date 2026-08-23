use bid::tender::outline_and_route;
use serde_json::{Value, json};
use sqlx::PgPool;
use storage::bidding::{FactMutation, MutationContext, PublishSection};
use uuid::Uuid;

struct PublicationSeed {
    project_id: Uuid,
    document_id: Uuid,
    source_artifact_id: Uuid,
    actor: String,
}

async fn final_tender_schema_is_ready(pool: &PgPool) -> bool {
    sqlx::query_scalar(
        "SELECT to_regprocedure(
          'kb_bid_publish_extraction_section(uuid,integer,uuid,text,jsonb,bigint,bigint,uuid,jsonb,kb_actor_identity,text,bytea,kb_sha256)'
         ) IS NOT NULL",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(false)
}

async fn seed_publication(pool: &PgPool, markdown: &str) -> PublicationSeed {
    let user_id = Uuid::new_v4();
    let project_id = Uuid::new_v4();
    let document_id = Uuid::new_v4();
    let source_artifact_id = Uuid::new_v4();
    let actor = format!("user:{user_id}");
    let original_sha256 = "1".repeat(64);
    let markdown_sha256 = domain::sha256_hex(markdown.as_bytes());

    let mut tx = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO users(id,email) VALUES($1,$2)")
        .bind(user_id)
        .bind(format!("{user_id}@tender-publication.invalid"))
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO bid_projects
         (id,title,owner_user_id,ends_at,fact_sha256,ceiling_identity_sha256,created_by)
         VALUES($1,'Tender publication contract',$2,clock_timestamp()+interval '30 days',
           repeat('0',64),repeat('0',64),$3)",
    )
    .bind(project_id)
    .bind(user_id)
    .bind(&actor)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO bid_clause_set_identities(project_id,set_kind,revision,content_sha256,updated_at)
         SELECT $1,set_kind,0,
           encode(digest(convert_to('ClauseSetV1:'||set_kind||':','UTF8'),'sha256'),'hex'),
           clock_timestamp()
         FROM unnest(ARRAY['service','pricing','schedule_payment','schedule_delivery','evaluation','procedural']) set_kind",
    )
    .bind(project_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO bid_documents
         (id,project_id,file_name,media_type,byte_length,original_object_ref,original_sha256,
          conversion_generation,parse_status)
         VALUES($1,$2,'tender.md','text/markdown',$3,$4,$5,1,'completed')",
    )
    .bind(document_id)
    .bind(project_id)
    .bind(markdown.len() as i64)
    .bind(format!("objects/{original_sha256}"))
    .bind(&original_sha256)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO bid_converted_source_artifacts
         (id,project_id,document_id,conversion_generation,original_object_ref,original_sha256,
          canonical_markdown_utf8,markdown_sha256,byte_length,converter_contract_version,
          image_asset_set_sha256)
         VALUES($1,$2,$3,1,$4,$5,$6,$7,$8,'test-converter-v1',repeat('2',64))",
    )
    .bind(source_artifact_id)
    .bind(project_id)
    .bind(document_id)
    .bind(format!("objects/{original_sha256}"))
    .bind(&original_sha256)
    .bind(markdown.as_bytes())
    .bind(markdown_sha256)
    .bind(markdown.len() as i64)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE bid_documents SET current_converted_source_artifact_id=$2
         WHERE id=$1",
    )
    .bind(document_id)
    .bind(source_artifact_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();

    PublicationSeed {
        project_id,
        document_id,
        source_artifact_id,
        actor,
    }
}

async fn seed_running_target(
    pool: &PgPool,
    seed: &PublicationSeed,
    target_id: Uuid,
    extraction_generation: i32,
    claim_token: Uuid,
) {
    let mut tx = pool.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO bid_extraction_targets
         (id,project_id,document_id,source_artifact_id,conversion_generation,
          extraction_generation,router_contract_version,policy_version,prompt_version,
          output_schema_version,expected_section_count,state)
         VALUES($1,$2,$3,$4,1,$5,'kind-router-v1','requirement-span-v1',
          'bounded-tender-publication-v1',1,1,'running')",
    )
    .bind(target_id)
    .bind(seed.project_id)
    .bind(seed.document_id)
    .bind(seed.source_artifact_id)
    .bind(extraction_generation)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO bid_extraction_attempts
         (target_id,attempt,claim_token,claimed_by,claim_lease_ms,claimed_at,heartbeat_at,status)
         VALUES($1,1,$2,'tender-publication-test',300000,clock_timestamp(),clock_timestamp(),'running')",
    )
    .bind(target_id)
    .bind(claim_token)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

fn publication_context(
    seed: &PublicationSeed,
    target_id: Uuid,
    suffix: &str,
    graph: &Value,
) -> MutationContext {
    MutationContext::new(
        "system:bid-extraction-worker",
        format!("{target_id}:{suffix}"),
        &json!({
            "project_id": seed.project_id,
            "document_id": seed.document_id,
            "target_id": target_id,
            "candidate_graph": graph,
        }),
    )
    .unwrap()
}

#[tokio::test]
async fn publication_is_atomic_and_fact_accept_race_is_linearizable() {
    let Ok(pool) = storage::connect().await else {
        eprintln!("skipped live TenderPublication contract: database unavailable");
        return;
    };
    if !final_tender_schema_is_ready(&pool).await {
        eprintln!("skipped live TenderPublication contract: final V1 schema unavailable");
        return;
    }

    let markdown = "# 技术要求\n系统必须支持国密协议。\n最高限价1000.00元，报价不得超过限价。";
    let section = outline_and_route(markdown).unwrap().remove(0);
    let graph = section.candidate_graph();
    let seed = seed_publication(&pool, markdown).await;

    let first_target_id = Uuid::new_v4();
    let first_claim_token = Uuid::new_v4();
    seed_running_target(&pool, &seed, first_target_id, 1, first_claim_token).await;
    let first_context = publication_context(&seed, first_target_id, "first", &graph);
    let first = storage::bidding::publish_extraction_section(
        &pool,
        PublishSection {
            target_id: first_target_id,
            attempt: 1,
            claim_token: first_claim_token,
            section_key: &section.key,
            heading_path: &json!(section.heading_path),
            parent_start_offset: section.parent_start_offset as i64,
            parent_end_offset: section.parent_end_offset as i64,
            expected_current_publication_id: None,
            candidate_graph: &graph,
        },
        &first_context,
    )
    .await
    .unwrap();
    let first_publication_id = first["publication_id"].as_str().unwrap().parse().unwrap();
    let old_fact_id = first["facts"][0]["id"].as_str().unwrap().parse().unwrap();
    let old_clause_id: Uuid = first["clauses"][0]["id"].as_str().unwrap().parse().unwrap();

    let second_target_id = Uuid::new_v4();
    let second_claim_token = Uuid::new_v4();
    seed_running_target(&pool, &seed, second_target_id, 2, second_claim_token).await;
    let mut invalid_graph = graph.clone();
    let invalid_segment = invalid_graph
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|segment| !segment["facts"].as_array().unwrap().is_empty())
        .unwrap();
    invalid_segment["quote"] = json!("这不是被冻结原文");
    let invalid_context = publication_context(&seed, second_target_id, "invalid", &invalid_graph);
    let invalid = storage::bidding::publish_extraction_section(
        &pool,
        PublishSection {
            target_id: second_target_id,
            attempt: 1,
            claim_token: second_claim_token,
            section_key: &section.key,
            heading_path: &json!(section.heading_path),
            parent_start_offset: section.parent_start_offset as i64,
            parent_end_offset: section.parent_end_offset as i64,
            expected_current_publication_id: Some(first_publication_id),
            candidate_graph: &invalid_graph,
        },
        &invalid_context,
    )
    .await;
    assert!(invalid.is_err());

    let zero_writes: (i64, i64, i64) = sqlx::query_as(
        "SELECT
          (SELECT count(*) FROM bid_section_publications WHERE target_id=$1),
          (SELECT count(*) FROM bid_extract_segment_candidates WHERE target_id=$1),
          (SELECT count(*) FROM bid_extract_fact_candidates WHERE target_id=$1)",
    )
    .bind(second_target_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(zero_writes, (0, 0, 0));
    let current_after_failure: Uuid = sqlx::query_scalar(
        "SELECT publication_id FROM bid_current_section_publications
         WHERE project_id=$1 AND document_id=$2 AND section_key=$3",
    )
    .bind(seed.project_id)
    .bind(seed.document_id)
    .bind(&section.key)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(current_after_failure, first_publication_id);
    let old_clause_status: String =
        sqlx::query_scalar("SELECT status FROM bid_clauses WHERE id=$1")
            .bind(old_clause_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(old_clause_status, "draft");

    let accept_body = json!({
        "project_id": seed.project_id,
        "candidate_id": old_fact_id,
        "expected_fact_revision": 0,
    });
    let accept_context = MutationContext::new(
        seed.actor.clone(),
        format!("accept-{old_fact_id}"),
        &accept_body,
    )
    .unwrap();
    let valid_context = publication_context(&seed, second_target_id, "valid", &graph);
    let heading_path = json!(section.heading_path);
    let accept = storage::bidding::mutate_fact(
        &pool,
        FactMutation {
            project_id: seed.project_id,
            action: "accept",
            candidate_id: Some(old_fact_id),
            field: None,
            typed_value: None,
            reason: None,
            override_reason: None,
            expected_fact_revision: 0,
        },
        &accept_context,
    );
    let replace = storage::bidding::publish_extraction_section(
        &pool,
        PublishSection {
            target_id: second_target_id,
            attempt: 1,
            claim_token: second_claim_token,
            section_key: &section.key,
            heading_path: &heading_path,
            parent_start_offset: section.parent_start_offset as i64,
            parent_end_offset: section.parent_end_offset as i64,
            expected_current_publication_id: Some(first_publication_id),
            candidate_graph: &graph,
        },
        &valid_context,
    );
    let (accept_result, replace_result) = tokio::join!(accept, replace);
    replace_result.unwrap();

    let project_fact_revision: i64 =
        sqlx::query_scalar("SELECT fact_revision FROM bid_projects WHERE id=$1")
            .bind(seed.project_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let history = storage::bidding::fact_suggestion_history(&pool, seed.project_id)
        .await
        .unwrap();
    let old_latest = history
        .iter()
        .filter(|decision| decision.candidate_id == old_fact_id)
        .max_by_key(|decision| decision.revision)
        .unwrap();
    if accept_result.is_ok() {
        assert_eq!(project_fact_revision, 1);
        assert_eq!(old_latest.status, "accepted");
    } else {
        assert_eq!(project_fact_revision, 0);
        assert_eq!(old_latest.status, "superseded");
    }
    let current = storage::bidding::current_fact_suggestions(&pool, seed.project_id)
        .await
        .unwrap();
    assert_eq!(current.len(), 1);
    assert_ne!(current[0].id, old_fact_id);
}

#[tokio::test]
async fn confirmed_clause_cannot_patch_kind_before_leaving_old_membership() {
    let Ok(pool) = storage::connect().await else {
        eprintln!("skipped live ClauseLifecycle contract: database unavailable");
        return;
    };
    if !final_tender_schema_is_ready(&pool).await {
        eprintln!("skipped live ClauseLifecycle contract: final V1 schema unavailable");
        return;
    }

    let seed = seed_publication(&pool, "# 招标要求\n系统必须支持国密协议。").await;
    let clause_id = Uuid::new_v4();
    let create_body = json!({
        "project_id": seed.project_id,
        "clause_id": clause_id,
        "text": "系统必须支持国密协议。",
        "kind": "technical",
        "must": true,
    });
    let create_context = MutationContext::new(
        seed.actor.clone(),
        format!("create-{clause_id}"),
        &create_body,
    )
    .unwrap();
    storage::bidding::create_clause(
        &pool,
        clause_id,
        seed.project_id,
        "系统必须支持国密协议。",
        "technical",
        true,
        &create_context,
    )
    .await
    .unwrap();

    let confirm_context = MutationContext::new(
        seed.actor.clone(),
        format!("confirm-{clause_id}"),
        &json!({"project_id":seed.project_id,"clause_id":clause_id,"expected_revision":1}),
    )
    .unwrap();
    storage::bidding::mutate_clause(
        &pool,
        seed.project_id,
        clause_id,
        "confirm",
        &json!({}),
        1,
        &confirm_context,
    )
    .await
    .unwrap();

    let forbidden_context = MutationContext::new(
        seed.actor.clone(),
        format!("forbidden-cross-kind-{clause_id}"),
        &json!({
            "project_id":seed.project_id,
            "clause_id":clause_id,
            "expected_revision":2,
            "kind":"service",
        }),
    )
    .unwrap();
    let forbidden = storage::bidding::mutate_clause(
        &pool,
        seed.project_id,
        clause_id,
        "patch",
        &json!({"kind":"service"}),
        2,
        &forbidden_context,
    )
    .await;
    assert!(
        forbidden.is_err(),
        "confirmed cross-kind PATCH must require explicit unconfirm"
    );

    let unchanged = storage::bidding::list_clauses(&pool, seed.project_id, false)
        .await
        .unwrap()
        .into_iter()
        .find(|clause| clause.id == clause_id)
        .unwrap();
    assert_eq!(unchanged.status, "confirmed");
    assert_eq!(unchanged.kind, "technical");
    assert_eq!(unchanged.revision, 2);
    let watermark_after_rejection: i64 =
        sqlx::query_scalar("SELECT matching_mutation_watermark FROM bid_projects WHERE id=$1")
            .bind(seed.project_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(watermark_after_rejection, 1);

    let unconfirm_context = MutationContext::new(
        seed.actor.clone(),
        format!("unconfirm-{clause_id}"),
        &json!({"project_id":seed.project_id,"clause_id":clause_id,"expected_revision":2}),
    )
    .unwrap();
    storage::bidding::mutate_clause(
        &pool,
        seed.project_id,
        clause_id,
        "unconfirm",
        &json!({}),
        2,
        &unconfirm_context,
    )
    .await
    .unwrap();
    let patch_context = MutationContext::new(
        seed.actor.clone(),
        format!("draft-cross-kind-{clause_id}"),
        &json!({
            "project_id":seed.project_id,
            "clause_id":clause_id,
            "expected_revision":3,
            "kind":"service",
        }),
    )
    .unwrap();
    storage::bidding::mutate_clause(
        &pool,
        seed.project_id,
        clause_id,
        "patch",
        &json!({"kind":"service"}),
        3,
        &patch_context,
    )
    .await
    .unwrap();
    let reconfirm_context = MutationContext::new(
        seed.actor,
        format!("reconfirm-{clause_id}"),
        &json!({"project_id":seed.project_id,"clause_id":clause_id,"expected_revision":4}),
    )
    .unwrap();
    let reconfirmed = storage::bidding::mutate_clause(
        &pool,
        seed.project_id,
        clause_id,
        "confirm",
        &json!({}),
        4,
        &reconfirm_context,
    )
    .await
    .unwrap();
    assert_eq!(reconfirmed["status"], "confirmed");
    assert_eq!(reconfirmed["kind"], "service");
    assert_eq!(reconfirmed["family"], "commercial");
    assert_eq!(reconfirmed["revision"], 5);

    let final_watermark: i64 =
        sqlx::query_scalar("SELECT matching_mutation_watermark FROM bid_projects WHERE id=$1")
            .bind(seed.project_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let service_revision: i64 = sqlx::query_scalar(
        "SELECT revision FROM bid_clause_set_identities
         WHERE project_id=$1 AND set_kind='service'",
    )
    .bind(seed.project_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(final_watermark, 3);
    assert_eq!(service_revision, 1);
}
