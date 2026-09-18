//! Published v2 analysis must remain exact when reused for final-file review.
use super::*;

#[tokio::test]
#[ignore = "requires KB_TENDER_AGENT_TEST_DATABASE_URL pointing to a fresh owned test database"]
async fn analysis_v2_publication_and_export_basis_preserve_exact_frozen_input() {
    let url =
        std::env::var("KB_TENDER_AGENT_TEST_DATABASE_URL").expect("dedicated test URL required");
    let pool = PgPool::connect(&url).await.unwrap();
    let database: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(database.starts_with("knowledgebrain_test_"));
    let (request, input) = seed(&pool).await;
    let owner = claim_agent_run(&pool, &request).await;
    let journal = postgres::PgJournal {
        pool: &pool,
        request: &request,
        owner: &owner,
        source_reader: None,
    };
    let analysis = bidding::tender_analysis::agent::run(
        &input,
        &config(),
        &journal,
        &script(&input),
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    let compiled = postgres::publication(&input, &analysis).unwrap();
    for case in [
        "downgrade",
        "wrong_contract",
        "missing_checks",
        "unread_ground",
    ] {
        let mut changed = compiled.clone();
        match case {
            "downgrade" => changed["analysis_result"]["schema_version"] = json!(1),
            "wrong_contract" => {
                changed["analysis_result"]["review"]["contract_sha256"] = json!("a".repeat(64))
            }
            "missing_checks" => changed["analysis_result"]["review"]["global_checks"] = json!([]),
            "unread_ground" => changed["analysis_result"]["review"]["coverage"]["text"] = json!({}),
            _ => unreachable!(),
        }
        let error = sqlx::query_scalar::<_,Value>("SELECT kb_bid_v2_publish_requirement_set_v4($1,$2,$3::kb_sha256,$4,'system:requirement-set-compile-v4'::kb_actor_identity,$5,$6,$7)")
            .bind(request.request_artifact_id).bind(request.request_revision).bind(&request.frozen_input_sha256)
            .bind(changed).bind(owner.attempt).bind(owner.execution_owner_token).bind(None::<Uuid>).fetch_one(&pool).await.unwrap_err();
        assert!(error.to_string().contains("global"), "{case}: {error}");
    }
    let output: Value = sqlx::query_scalar("SELECT kb_bid_v2_publish_requirement_set_v4($1,$2,$3::kb_sha256,$4,'system:requirement-set-compile-v4'::kb_actor_identity,$5,$6,$7)")
        .bind(request.request_artifact_id).bind(request.request_revision).bind(&request.frozen_input_sha256)
        .bind(compiled).bind(owner.attempt).bind(owner.execution_owner_token).bind(None::<Uuid>).fetch_one(&pool).await.unwrap();
    assert_eq!(output["analysis_quality"], "verified", "{output}");
    let (workspace, actor, payload): (Uuid, String, Value) = sqlx::query_as(
        "SELECT w.id,'user:'||p.owner_user_id,convert_from(s.canonical_payload,'UTF8')::jsonb
         FROM bid_projects p JOIN bid_submission_workspaces w ON w.project_id=p.id
         JOIN bid_requirement_set_current c ON c.scope_id=p.id
         JOIN bid_requirement_set_artifacts s ON s.id=c.artifact_id WHERE p.id=$1",
    )
    .bind(Uuid::parse_str(&input.project_id).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    let result: bidding::tender_analysis::AnalysisResult =
        serde_json::from_value(payload["analysis_result"].clone()).unwrap();
    assert_eq!(result.schema_version, 2);
    bidding::tender_analysis::rule_contract::validate_review(
        &input,
        &result.analysis,
        &result.review,
    )
    .unwrap();

    // SQL identity fixture only; this test does not claim real DOCX conversion.
    let bytes = b"global-review saved DOCX identity";
    let sha = platform::sha256_hex(bytes);
    let staging = Uuid::new_v4();
    sqlx::query("SELECT kb_object_upload_stage($1,('objects/'||$2)::kb_object_ref,$2::kb_sha256,'application/vnd.openxmlformats-officedocument.wordprocessingml.document',$3,$4::kb_actor_identity)")
        .bind(staging).bind(&sha).bind(bytes.len() as i64).bind(&actor).execute(&pool).await.unwrap();
    let basis: Value = sqlx::query_scalar(
        "SELECT jsonb_build_object('document_set_id',d.artifact_id,'document_set_sha256',d.artifact_sha256,
         'requirement_set_id',r.artifact_id,'requirement_set_sha256',r.artifact_sha256,
         'expected_version_id',NULL,'expected_docx_sha256',NULL,'docx_sha256',$2::text,'byte_length',$3::bigint)
         FROM bid_document_set_current d JOIN bid_requirement_set_current r ON r.scope_id=d.scope_id WHERE d.scope_id=$1")
        .bind(Uuid::parse_str(&input.project_id).unwrap()).bind(&sha).bind(bytes.len() as i64).fetch_one(&pool).await.unwrap();
    let saved: Value =
        sqlx::query_scalar("SELECT kb_bid_v2_create_docx_round($1,$2,$3,$4::kb_actor_identity,$5)")
            .bind(workspace)
            .bind(staging)
            .bind(basis)
            .bind(&actor)
            .bind(Uuid::new_v4().to_string())
            .fetch_one(&pool)
            .await
            .unwrap();
    let version = Uuid::parse_str(saved["version_id"].as_str().unwrap()).unwrap();
    let request_bytes = b"global-review export identity";
    let export: Value = sqlx::query_scalar("SELECT kb_bid_v2_create_submission_export_request($1,$2,$3::kb_sha256,$4::kb_actor_identity,$5,$6,kb_bid_v2_sha256_bytes($6))")
        .bind(workspace).bind(version).bind(&sha).bind(&actor).bind(Uuid::new_v4().to_string())
        .bind(request_bytes.as_slice()).fetch_one(&pool).await.unwrap();
    let export_id = Uuid::parse_str(export["request_artifact_id"].as_str().unwrap()).unwrap();
    let export_sha = export["frozen_input_sha256"].as_str().unwrap();
    let loaded: Value =
        sqlx::query_scalar("SELECT kb_bid_v2_load_export_review_basis($1,$2::kb_sha256)")
            .bind(export_id)
            .bind(export_sha)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(loaded["allowed"], true, "{loaded}");
    assert_eq!(
        loaded["input"],
        json!(input),
        "export must load the exact analyzed input, not a reconstructed newer disposition"
    );
    assert_eq!(loaded["analysis_result"], json!(result));
    let loaded_input: FrozenInput = serde_json::from_value(loaded["input"].clone()).unwrap();
    assert_eq!(
        bidding::tender_analysis::digest(&loaded_input).unwrap(),
        result.frozen_input_sha256
    );
    bidding::tender_analysis::rule_contract::validate_review(
        &loaded_input,
        &result.analysis,
        &result.review,
    )
    .unwrap();

    let wrong = sqlx::query_scalar::<_, Value>(
        "SELECT kb_bid_v2_load_export_review_basis($1,$2::kb_sha256)",
    )
    .bind(export_id)
    .bind("a".repeat(64))
    .fetch_one(&pool)
    .await
    .unwrap_err();
    assert_eq!(
        wrong.as_database_error().unwrap().code().as_deref(),
        Some("P0002")
    );

    // A caller cannot downgrade the analysis identity frozen from this DOCX.
    let mut legacy_context = export["frozen_context"].clone();
    legacy_context["analysis_identity"]["schema_version"] = json!(1);
    let rejected = sqlx::query_scalar::<_,Value>("SELECT kb_bid_v2_create_submission_export_request($1,$2,$3::kb_sha256,$4::kb_actor_identity,$5,$6,kb_bid_v2_sha256_bytes($6),$7)")
        .bind(workspace).bind(version).bind(&sha).bind(&actor).bind(Uuid::new_v4().to_string())
        .bind(request_bytes.as_slice()).bind(legacy_context).fetch_one(&pool).await.unwrap_err();
    assert!(
        rejected
            .to_string()
            .contains("SUBMISSION_EXPORT_CONTEXT_INVALID: source analysis changed")
    );
    pool.close().await;
}

#[test]
fn legacy_v1_publication_projection_retains_its_own_contract() {
    use bidding::tender_analysis::{Analysis, AnalysisResult, Review};
    let input = FrozenInput {
        schema_version: 1,
        project_id: Uuid::new_v4().to_string(),
        document_set_id: Uuid::new_v4().to_string(),
        documents: vec![],
        document_relations: vec![],
        decisions: vec![],
        source_units: vec![],
        structured_forms: vec![],
    };
    let analysis = Analysis::default();
    let result = AnalysisResult {
        schema_version: 1,
        frozen_input_sha256: bidding::tender_analysis::digest(&input).unwrap(),
        review: Review {
            analysis_sha256: bidding::tender_analysis::digest(&analysis).unwrap(),
            ..Default::default()
        },
        analysis,
        quality: "verified".into(),
        source_views: Default::default(),
    };
    let projected = postgres::publication(&input, &result).unwrap();
    assert_eq!(projected["analysis_result"]["schema_version"], 1);
    assert!(
        projected["analysis_result"]["review"]
            .get("global_checks")
            .is_none()
    );
    assert!(
        bidding::tender_analysis::rule_contract::validate_review(
            &input,
            &result.analysis,
            &result.review
        )
        .is_err()
    );
}
