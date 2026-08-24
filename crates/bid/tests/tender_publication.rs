use bid::tender::outline_and_route;
use serde_json::{Value, json};
use sqlx::PgPool;
use storage::bidding::{
    CompleteDocumentConversion, ConvertedSourceImageUpload, FactMutation, MutationContext,
    PublishSection, UploadDocument,
};
use uuid::Uuid;

mod support;

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
    let Some(pool) = support::connect_postgres_contract("TenderPublication").await else {
        return;
    };
    if !support::require_final_schema(
        "TenderPublication",
        final_tender_schema_is_ready(&pool).await,
    ) {
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
async fn kind_router_promotions_refresh_generation_two_and_three_markers() {
    let Some(pool) = support::connect_postgres_contract("TenderPublication").await else {
        return;
    };
    if !support::require_final_schema(
        "TenderPublication",
        final_tender_schema_is_ready(&pool).await,
    ) {
        return;
    }

    let markdown = "# 技术要求\n系统必须支持国密协议。";
    let section = outline_and_route(markdown).unwrap().remove(0);
    let graph = section.candidate_graph();
    let seed = seed_publication(&pool, markdown).await;
    let target_id = Uuid::new_v4();
    let claim_token = Uuid::new_v4();
    seed_running_target(&pool, &seed, target_id, 1, claim_token).await;
    let publication = storage::bidding::publish_extraction_section(
        &pool,
        PublishSection {
            target_id,
            attempt: 1,
            claim_token,
            section_key: &section.key,
            heading_path: &json!(section.heading_path),
            parent_start_offset: section.parent_start_offset as i64,
            parent_end_offset: section.parent_end_offset as i64,
            expected_current_publication_id: None,
            candidate_graph: &graph,
        },
        &publication_context(&seed, target_id, "kind-promotion", &graph),
    )
    .await
    .expect("publish extracted clause for promotion");
    let extracted_clause_id: Uuid = publication["clauses"][0]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let clause_text: String = sqlx::query_scalar("SELECT text FROM bid_clauses WHERE id=$1")
        .bind(extracted_clause_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let confirm_extracted = MutationContext::new(
        seed.actor.clone(),
        format!("confirm-extracted-{extracted_clause_id}"),
        &json!({"action":"confirm","expected_revision":1}),
    )
    .unwrap();
    storage::bidding::mutate_clause(
        &pool,
        seed.project_id,
        extracted_clause_id,
        "confirm",
        &json!({}),
        1,
        &confirm_extracted,
    )
    .await
    .expect("confirm extracted clause before promotion");

    let source_span_id: Uuid =
        sqlx::query_scalar("SELECT current_source_span_artifact_id FROM bid_clauses WHERE id=$1")
            .bind(extracted_clause_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let manualized_extracted_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO bid_clauses
         (id,project_id,provenance,status,kind,text,must,current_source_span_artifact_id,
          extracted_origin_source_span_artifact_id,revision,created_by)
         VALUES($1,$2,'extracted','draft','technical',$3,true,$4,$4,1,$5)",
    )
    .bind(manualized_extracted_id)
    .bind(seed.project_id)
    .bind(&clause_text)
    .bind(source_span_id)
    .bind(&seed.actor)
    .execute(&pool)
    .await
    .unwrap();
    let manualize_context = MutationContext::new(
        seed.actor.clone(),
        format!("manualize-extracted-{manualized_extracted_id}"),
        &json!({"action":"patch","expected_revision":1,"kind":"service"}),
    )
    .unwrap();
    storage::bidding::mutate_clause(
        &pool,
        seed.project_id,
        manualized_extracted_id,
        "patch",
        &json!({"kind":"service"}),
        1,
        &manualize_context,
    )
    .await
    .expect("editing extracted kind must make the clause manual-after-edit");
    let manualized: (String, Option<Uuid>) = sqlx::query_as(
        "SELECT provenance,current_source_span_artifact_id FROM bid_clauses WHERE id=$1",
    )
    .bind(manualized_extracted_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(manualized, ("manual_after_edit".into(), None));

    let manual_clause_id = Uuid::new_v4();
    let create_manual = MutationContext::new(
        seed.actor.clone(),
        format!("create-manual-{manual_clause_id}"),
        &json!({"text":clause_text,"kind":"technical"}),
    )
    .unwrap();
    storage::bidding::create_clause(
        &pool,
        manual_clause_id,
        seed.project_id,
        &clause_text,
        "technical",
        true,
        &create_manual,
    )
    .await
    .expect("create manual control clause");
    let confirm_manual = MutationContext::new(
        seed.actor.clone(),
        format!("confirm-manual-{manual_clause_id}"),
        &json!({"action":"confirm","expected_revision":1}),
    )
    .unwrap();
    storage::bidding::mutate_clause(
        &pool,
        seed.project_id,
        manual_clause_id,
        "confirm",
        &json!({}),
        1,
        &confirm_manual,
    )
    .await
    .expect("confirm manual control clause");

    let initial_router: (String, i64) = sqlx::query_as(
        "SELECT version,promotion_generation FROM kind_router_current WHERE singleton_key",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(initial_router, ("kind-router-v1".into(), 0));

    let mut transaction = pool.begin().await.unwrap();
    sqlx::query(
        "UPDATE application_maintenance_gate
            SET mode='maintenance',generation=generation+1,updated_by=$1,updated_at=clock_timestamp()
          WHERE singleton_key",
    )
    .bind(&seed.actor)
    .execute(&mut *transaction)
    .await
    .unwrap();
    let text_sha256 = domain::sha256_hex(clause_text.as_bytes());
    let suffix = Uuid::new_v4().simple().to_string();
    let versions = [
        (format!("kind-router-{suffix}-v2"), "service"),
        (format!("kind-router-{suffix}-v3"), "pricing"),
        (format!("kind-router-{suffix}-v4"), "service"),
    ];
    for (version, kind) in &versions {
        let mut overrides = serde_json::Map::new();
        overrides.insert(text_sha256.clone(), json!(kind));
        let contract = json!({
            "schema_version": 1,
            "version": version,
            "family": {
                "technical": "technical",
                "qualification": "commercial",
                "service": "commercial",
                "pricing": null,
                "schedule_delivery": null,
                "schedule_payment": null,
                "evaluation": null,
                "procedural": null,
            },
            "overrides": overrides,
        });
        let canonical_payload = serde_json::to_vec(&contract).unwrap();
        let content_sha256 = domain::sha256_hex(&canonical_payload);
        let request = json!({"version":version,"content_sha256":content_sha256});
        let request_bytes = serde_json::to_vec(&request).unwrap();
        let request_sha256 = domain::sha256_hex(&request_bytes);
        let registered: Value =
            sqlx::query_scalar("SELECT kb_bid_register_kind_router_contract($1,$2,$3,$4,$5,$6,$7)")
                .bind(version)
                .bind(canonical_payload)
                .bind(content_sha256)
                .bind(&seed.actor)
                .bind(format!("register-{version}"))
                .bind(request_bytes)
                .bind(request_sha256)
                .fetch_one(&mut *transaction)
                .await
                .unwrap_or_else(|error| panic!("register {version}: {error}"));
        assert_eq!(registered["version"], version.as_str());
    }

    let mut expected_version = "kind-router-v1".to_string();
    for (index, (target_version, expected_kind)) in versions.iter().enumerate() {
        let expected_generation = i64::try_from(index).unwrap();
        let target_generation = expected_generation + 1;
        let request = json!({
            "target_version": target_version,
            "expected_current_version": expected_version,
            "expected_promotion_generation": expected_generation,
        });
        let request_bytes = serde_json::to_vec(&request).unwrap();
        let request_sha256 = domain::sha256_hex(&request_bytes);
        let promoted: Value =
            sqlx::query_scalar("SELECT kb_bid_promote_kind_router($1,$2,$3,$4,$5,$6,$7)")
                .bind(target_version)
                .bind(&expected_version)
                .bind(expected_generation)
                .bind(&seed.actor)
                .bind(format!("promote-{target_version}"))
                .bind(request_bytes)
                .bind(request_sha256)
                .fetch_one(&mut *transaction)
                .await
                .unwrap_or_else(|error| panic!("promote {target_version}: {error}"));
        assert_eq!(promoted["promotion_generation"], target_generation);

        let extracted: (String, String, i64, Option<i64>) = sqlx::query_as(
            "SELECT status,kind,revision,confirmation_required_router_generation
               FROM bid_clauses WHERE id=$1",
        )
        .bind(extracted_clause_id)
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
        assert_eq!(
            extracted,
            (
                "draft".into(),
                (*expected_kind).into(),
                target_generation + 2,
                Some(target_generation),
            )
        );
        let manual: (String, String, i64, Option<i64>) = sqlx::query_as(
            "SELECT status,kind,revision,confirmation_required_router_generation
               FROM bid_clauses WHERE id=$1",
        )
        .bind(manual_clause_id)
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
        assert_eq!(manual, ("confirmed".into(), "technical".into(), 2, None));
        let manualized: (String, String, String, i64, Option<i64>) = sqlx::query_as(
            "SELECT provenance,status,kind,revision,confirmation_required_router_generation
               FROM bid_clauses WHERE id=$1",
        )
        .bind(manualized_extracted_id)
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
        assert_eq!(
            manualized,
            (
                "manual_after_edit".into(),
                "draft".into(),
                "service".into(),
                2,
                None,
            )
        );
        expected_version = target_version.clone();
    }

    let clause_audits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events
          WHERE operation='bid.kind_router.promotion_clause'
            AND entity_locator->>'clause_id'=$1",
    )
    .bind(extracted_clause_id.to_string())
    .fetch_one(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(clause_audits, 3);
    sqlx::query(
        "UPDATE application_maintenance_gate
            SET mode='open',generation=generation+1,updated_by=$1,updated_at=clock_timestamp()
          WHERE singleton_key",
    )
    .bind(&seed.actor)
    .execute(&mut *transaction)
    .await
    .unwrap();
    let reconfirm = json!({"action":"confirm","expected_revision":5});
    let reconfirm_bytes = serde_json::to_vec(&reconfirm).unwrap();
    let reconfirm_sha256 = domain::sha256_hex(&reconfirm_bytes);
    let reconfirmed: Value = sqlx::query_scalar(
        "SELECT kb_bid_mutate_clause($1,$2,'confirm','{}'::jsonb,5,$3,$4,$5,$6)",
    )
    .bind(seed.project_id)
    .bind(extracted_clause_id)
    .bind(&seed.actor)
    .bind(format!("reconfirm-{extracted_clause_id}"))
    .bind(reconfirm_bytes)
    .bind(reconfirm_sha256)
    .fetch_one(&mut *transaction)
    .await
    .expect("reconfirm against the current generation-three marker");
    assert_eq!(reconfirmed["status"], "confirmed");
    assert_eq!(reconfirmed["kind"], "service");
    assert_eq!(
        reconfirmed["confirmation_required_router_generation"],
        Value::Null
    );
    let service_revision: i64 = sqlx::query_scalar(
        "SELECT revision FROM bid_clause_set_identities
          WHERE project_id=$1 AND set_kind='service'",
    )
    .bind(seed.project_id)
    .fetch_one(&mut *transaction)
    .await
    .unwrap();
    assert_eq!(
        service_revision, 1,
        "NEW membership is entered exactly once"
    );

    sqlx::query(
        "UPDATE application_maintenance_gate
            SET mode='maintenance',generation=generation+1,updated_by=$1,updated_at=clock_timestamp()
          WHERE singleton_key",
    )
    .bind(&seed.actor)
    .execute(&mut *transaction)
    .await
    .unwrap();

    let cas_request = json!({"target_version":versions[2].0,"expected_generation":2});
    let cas_bytes = serde_json::to_vec(&cas_request).unwrap();
    let cas_sha256 = domain::sha256_hex(&cas_bytes);
    let cas: Result<Value, sqlx::Error> =
        sqlx::query_scalar("SELECT kb_bid_promote_kind_router($1,$2,2,$3,$4,$5,$6)")
            .bind(&versions[2].0)
            .bind(&versions[1].0)
            .bind(&seed.actor)
            .bind(format!("stale-cas-{suffix}"))
            .bind(cas_bytes)
            .bind(cas_sha256)
            .fetch_one(&mut *transaction)
            .await;
    let message = cas
        .expect_err("a stale generation-two CAS must not overwrite generation three")
        .as_database_error()
        .map(|error| error.message().to_string())
        .unwrap_or_default();
    assert!(message.contains("KIND_ROUTER_PROMOTION_CAS_MISMATCH"));
    transaction.rollback().await.unwrap();
    let rolled_back_router: (String, i64) = sqlx::query_as(
        "SELECT version,promotion_generation FROM kind_router_current WHERE singleton_key",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(rolled_back_router, initial_router);
}

#[tokio::test]
async fn extraction_reaper_terminates_only_the_expired_attempt() {
    let Some(pool) = support::connect_postgres_contract("TenderPublication").await else {
        return;
    };
    if !support::require_final_schema(
        "TenderPublication",
        final_tender_schema_is_ready(&pool).await,
    ) {
        return;
    }
    let seed = seed_publication(&pool, "# 技术要求\n系统必须支持国密协议。").await;
    let target_id = Uuid::new_v4();
    let old_claim_token = Uuid::new_v4();
    seed_running_target(&pool, &seed, target_id, 1, old_claim_token).await;
    sqlx::query(
        "UPDATE bid_extraction_attempts
            SET heartbeat_at=clock_timestamp()-interval '10 minutes'
          WHERE target_id=$1 AND attempt=1 AND claim_token=$2",
    )
    .bind(target_id)
    .bind(old_claim_token)
    .execute(&pool)
    .await
    .unwrap();

    let reclaimed = storage::bid_submission::reclaim_stale_extractions(&pool)
        .await
        .unwrap();
    assert!(reclaimed.iter().any(|row| row.0 == target_id));
    let old_status: String = sqlx::query_scalar(
        "SELECT status FROM bid_extraction_attempts WHERE target_id=$1 AND attempt=1",
    )
    .bind(target_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(old_status, "reaped");

    let new_claim_token = Uuid::new_v4();
    let new_claim = storage::bidding::claim_extraction(
        &pool,
        target_id,
        new_claim_token,
        "reaper-regression-test",
    )
    .await
    .unwrap()
    .expect("reaped target must be claimable");
    assert_eq!(new_claim.attempt, 2);
    assert!(
        storage::bid_submission::reclaim_stale_extractions(&pool)
            .await
            .unwrap()
            .iter()
            .all(|row| row.0 != target_id),
        "the old expired attempt must not reap its healthy successor"
    );
    let state: String = sqlx::query_scalar("SELECT state FROM bid_extraction_targets WHERE id=$1")
        .bind(target_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "running");
    assert!(
        storage::bidding::fail_extraction(
            &pool,
            target_id,
            new_claim.attempt,
            new_claim_token,
            "TEST_COMPLETE",
            false,
        )
        .await
        .unwrap()
    );
}

#[tokio::test]
async fn conversion_reaper_rejects_renewal_after_logical_expiry() {
    let Some(pool) = support::connect_postgres_contract("TenderPublication").await else {
        return;
    };
    if !support::require_final_schema(
        "TenderPublication",
        final_tender_schema_is_ready(&pool).await,
    ) {
        return;
    }
    let mut reaper_guard = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(2026082401)")
        .execute(&mut *reaper_guard)
        .await
        .unwrap();
    let seed = seed_publication(&pool, "# 技术要求\n系统必须支持国密协议。").await;
    let document_id = Uuid::new_v4();
    let claim_token = Uuid::new_v4();
    let digest = "3".repeat(64);
    sqlx::query(
        "INSERT INTO bid_documents
         (id,project_id,file_name,media_type,byte_length,original_object_ref,original_sha256,
          conversion_generation,parse_status)
         VALUES($1,$2,'lease-race.docx','application/vnd.openxmlformats-officedocument.wordprocessingml.document',
           1,$3,$4,1,'processing')",
    )
    .bind(document_id)
    .bind(seed.project_id)
    .bind(format!("objects/{digest}"))
    .bind(&digest)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO bid_document_conversion_attempts
         (document_id,conversion_generation,attempt,claim_token,claimed_by,claim_lease_ms,
          claimed_at,heartbeat_at,status)
         VALUES($1,1,1,$2,'conversion-reaper-race',300000,
           clock_timestamp()-interval '10 minutes',clock_timestamp()-interval '10 minutes','running')",
    )
    .bind(document_id)
    .bind(claim_token)
    .execute(&pool)
    .await
    .unwrap();

    let mut blocker = pool.begin().await.unwrap();
    sqlx::query("SELECT 1 FROM bid_documents WHERE id=$1 FOR UPDATE")
        .bind(document_id)
        .execute(&mut *blocker)
        .await
        .unwrap();

    let application_name = format!("conversion-reaper-{}", Uuid::new_v4());
    let mut reaper_connection = pool.acquire().await.unwrap();
    sqlx::query("SELECT set_config('application_name',$1,false)")
        .bind(&application_name)
        .execute(&mut *reaper_connection)
        .await
        .unwrap();
    let reaper = tokio::spawn(async move {
        sqlx::query_scalar::<_, Vec<Uuid>>("SELECT kb_bid_reclaim_stale_conversions()")
            .fetch_one(&mut *reaper_connection)
            .await
    });

    let mut waiting = false;
    for _ in 0..100 {
        waiting = sqlx::query_scalar(
            "SELECT EXISTS(
               SELECT 1 FROM pg_stat_activity
                WHERE application_name=$1 AND wait_event_type='Lock')",
        )
        .bind(&application_name)
        .fetch_one(&pool)
        .await
        .unwrap();
        if waiting {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(
        waiting,
        "reaper must reach the document lock before renewal"
    );
    assert!(
        !storage::bidding::heartbeat_document_conversion(&pool, document_id, claim_token)
            .await
            .unwrap(),
        "an already expired owner must not renew while reaper is waiting"
    );
    blocker.commit().await.unwrap();

    let reaped = reaper.await.unwrap().unwrap();
    assert_eq!(reaped, vec![document_id]);
    let state: (String, String) = sqlx::query_as(
        "SELECT document.parse_status,attempt.status
           FROM bid_documents document
           JOIN bid_document_conversion_attempts attempt
             ON attempt.document_id=document.id AND attempt.conversion_generation=document.conversion_generation
          WHERE document.id=$1 AND attempt.claim_token=$2",
    )
    .bind(document_id)
    .bind(claim_token)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(state, ("pending".into(), "reaped".into()));
    reaper_guard.rollback().await.unwrap();
}

#[tokio::test]
async fn conversion_reaper_rechecks_the_exact_attempt_after_waiting_for_a_lock() {
    let Some(pool) = support::connect_postgres_contract("TenderPublication").await else {
        return;
    };
    if !support::require_final_schema(
        "TenderPublication",
        final_tender_schema_is_ready(&pool).await,
    ) {
        return;
    }
    let mut reaper_guard = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(2026082401)")
        .execute(&mut *reaper_guard)
        .await
        .unwrap();
    let seed = seed_publication(&pool, "# 技术要求\n系统必须支持国密协议。").await;
    let document_id = Uuid::new_v4();
    let claim_token = Uuid::new_v4();
    let digest = "4".repeat(64);
    sqlx::query(
        "INSERT INTO bid_documents
         (id,project_id,file_name,media_type,byte_length,original_object_ref,original_sha256,
          conversion_generation,parse_status)
         VALUES($1,$2,'lease-recheck.docx','application/vnd.openxmlformats-officedocument.wordprocessingml.document',
           1,$3,$4,1,'processing')",
    )
    .bind(document_id)
    .bind(seed.project_id)
    .bind(format!("objects/{digest}"))
    .bind(&digest)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO bid_document_conversion_attempts
         (document_id,conversion_generation,attempt,claim_token,claimed_by,claim_lease_ms,
          claimed_at,heartbeat_at,status)
         VALUES($1,1,1,$2,'conversion-reaper-recheck',300000,
           clock_timestamp()-interval '10 minutes',clock_timestamp()-interval '10 minutes','running')",
    )
    .bind(document_id)
    .bind(claim_token)
    .execute(&pool)
    .await
    .unwrap();

    let mut blocker = pool.begin().await.unwrap();
    sqlx::query("SELECT 1 FROM bid_documents WHERE id=$1 FOR UPDATE")
        .bind(document_id)
        .execute(&mut *blocker)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE bid_document_conversion_attempts
            SET heartbeat_at=clock_timestamp()
          WHERE document_id=$1 AND conversion_generation=1 AND attempt=1",
    )
    .bind(document_id)
    .execute(&mut *blocker)
    .await
    .unwrap();

    let application_name = format!("conversion-recheck-{}", Uuid::new_v4());
    let mut reaper_connection = pool.acquire().await.unwrap();
    sqlx::query("SELECT set_config('application_name',$1,false)")
        .bind(&application_name)
        .execute(&mut *reaper_connection)
        .await
        .unwrap();
    let reaper = tokio::spawn(async move {
        sqlx::query_scalar::<_, Vec<Uuid>>("SELECT kb_bid_reclaim_stale_conversions()")
            .fetch_one(&mut *reaper_connection)
            .await
    });

    let mut waiting = false;
    for _ in 0..100 {
        waiting = sqlx::query_scalar(
            "SELECT EXISTS(
               SELECT 1 FROM pg_stat_activity
                WHERE application_name=$1 AND wait_event_type='Lock')",
        )
        .bind(&application_name)
        .fetch_one(&pool)
        .await
        .unwrap();
        if waiting {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(waiting, "reaper must wait on the exact conversion rows");
    blocker.commit().await.unwrap();

    assert!(
        reaper.await.unwrap().unwrap().is_empty(),
        "a reaper that waited for a lock must recheck the current attempt lease"
    );
    let state: (String, String) = sqlx::query_as(
        "SELECT document.parse_status,attempt.status
           FROM bid_documents document
           JOIN bid_document_conversion_attempts attempt
             ON attempt.document_id=document.id AND attempt.conversion_generation=document.conversion_generation
          WHERE document.id=$1 AND attempt.claim_token=$2",
    )
    .bind(document_id)
    .bind(claim_token)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(state, ("processing".into(), "running".into()));
    reaper_guard.rollback().await.unwrap();
}

#[tokio::test]
async fn confirmed_clause_cannot_patch_kind_before_leaving_old_membership() {
    let Some(pool) = support::connect_postgres_contract("ClauseLifecycle").await else {
        return;
    };
    if !support::require_final_schema("ClauseLifecycle", final_tender_schema_is_ready(&pool).await)
    {
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

#[tokio::test]
async fn converted_images_transfer_staging_to_source_artifact_owners() {
    let Some(pool) = support::connect_postgres_contract("conversion object").await else {
        return;
    };
    if !support::require_final_schema(
        "conversion object",
        final_tender_schema_is_ready(&pool).await,
    ) {
        return;
    }

    let user_id = Uuid::new_v4();
    let project_id = Uuid::new_v4();
    let document_id = Uuid::new_v4();
    let actor = format!("user:{user_id}");
    sqlx::query("INSERT INTO users(id,email) VALUES($1,$2)")
        .bind(user_id)
        .bind(format!("{user_id}@converted-image.invalid"))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO bid_projects
         (id,title,owner_user_id,ends_at,fact_sha256,ceiling_identity_sha256,created_by)
         VALUES($1,'Converted image registry contract',$2,clock_timestamp()+interval '30 days',
           repeat('0',64),repeat('0',64),$3)",
    )
    .bind(project_id)
    .bind(user_id)
    .bind(&actor)
    .execute(&pool)
    .await
    .unwrap();

    let original = b"original tender bytes";
    let original_digest = domain::sha256_hex(original);
    let original_ref = storage::object_ref(&original_digest);
    let original_staging_id = Uuid::new_v4();
    storage::stage_object_upload(
        &pool,
        original_staging_id,
        &original_ref,
        &original_digest,
        "application/pdf",
        original.len() as i64,
        &actor,
    )
    .await
    .unwrap();
    let upload_body = json!({
        "project_id": project_id,
        "document_id": document_id,
        "object_ref": original_ref,
        "sha256": original_digest,
    });
    let upload_context =
        MutationContext::new(actor.clone(), format!("upload-{document_id}"), &upload_body).unwrap();
    storage::bidding::upload_document(
        &pool,
        original_staging_id,
        UploadDocument {
            id: document_id,
            project_id,
            file_name: "converted-image.pdf",
            media_type: "application/pdf",
            byte_length: original.len() as i64,
            object_ref: &original_ref,
            original_sha256: &original_digest,
        },
        &upload_context,
    )
    .await
    .unwrap();

    let claim_token = Uuid::new_v4();
    storage::bidding::claim_document_conversion(
        &pool,
        document_id,
        claim_token,
        "converted-image-contract-test",
    )
    .await
    .unwrap()
    .expect("pending document must be claimable");

    let image = b"\x89PNG\r\n\x1a\nconverted-image";
    let image_digest = domain::sha256_hex(image);
    let image_ref = storage::object_ref(&image_digest);
    let image_staging_id = Uuid::new_v4();
    let conversion_actor = "system:bid-convert-worker";
    storage::stage_object_upload(
        &pool,
        image_staging_id,
        &image_ref,
        &image_digest,
        "image/png",
        image.len() as i64,
        conversion_actor,
    )
    .await
    .unwrap();
    let image_set_sha256 = domain::sha256_hex(
        format!("ConvertedSourceArtifactV1:image-set:{image_digest}").as_bytes(),
    );
    let source_artifact_id = Uuid::new_v4();
    let extraction_target_id = Uuid::new_v4();
    let completed = storage::bidding::complete_document_conversion(
        &pool,
        CompleteDocumentConversion {
            document_id,
            claim_token,
            source_artifact_id,
            markdown: format!("![topology]({image_ref})").as_bytes(),
            converter_contract_version: "converted-image-contract-v1",
            image_asset_set_sha256: &image_set_sha256,
            image_assets: &[ConvertedSourceImageUpload {
                staging_id: image_staging_id,
                object_ref: image_ref.clone(),
                digest: image_digest.clone(),
                media_type: "image/png".into(),
                byte_length: image.len() as i64,
                occurrence: "image:0".into(),
            }],
            extraction_target_id,
            expected_section_count: 1,
            policy_version: "requirement-span-v1+fact-suggestion-v1",
            prompt_version: "bounded-tender-publication-v1",
            actor: conversion_actor,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        completed["source_artifact_id"],
        source_artifact_id.to_string()
    );
    assert_eq!(
        completed["extraction_target_id"],
        extraction_target_id.to_string()
    );
    let target: (Uuid, i32, String) = sqlx::query_as(
        "SELECT source_artifact_id,conversion_generation,state
           FROM bid_extraction_targets WHERE id=$1",
    )
    .bind(extraction_target_id)
    .fetch_one(&pool)
    .await
    .expect("conversion completion must durably create its extraction target");
    assert_eq!(target, (source_artifact_id, 1, "pending".into()));

    let owner_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM object_owner_references
         WHERE object_ref=$1 AND owner_kind='bid_converted_source_image'
           AND owner_id=$2 AND occurrence='image:0'",
    )
    .bind(&image_ref)
    .bind(source_artifact_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(owner_count, 1);
    let staging_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM object_upload_staging WHERE id=$1")
            .bind(image_staging_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        staging_count, 0,
        "completion must consume the staging owner"
    );
    let state: String = sqlx::query_scalar("SELECT state FROM object_registry WHERE object_ref=$1")
        .bind(&image_ref)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "available");
}
