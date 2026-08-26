use domain::knowledge_retrieval::{
    KNOWLEDGE_EVIDENCE_CONTRACT_V2, KNOWLEDGE_EVIDENCE_SCHEMA_V1, KnowledgeEvidenceScopeV2,
    KnowledgeRetrievalError, KnowledgeRetrievalPortV2, ProductEvidenceRequestV1,
    RETRIEVAL_A_PRIMARY_COMPARATOR_V2, RETRIEVAL_A_VERSION_COMPARATOR_V2,
    RETRIEVAL_B_EXACT_COMPARATOR_V2, RETRIEVAL_C_SEMANTIC_COMPARATOR_V2,
    RETRIEVAL_EMBEDDING_POLICY_V2, RETRIEVAL_EMBEDDING_POLICY_VERSION_V2,
    RETRIEVAL_KEYWORD_TOKENIZER_V2, RETRIEVAL_KEYWORD_TOKENIZER_VERSION_V2,
    RETRIEVAL_NORMALIZATION_VERSION_V2, RETRIEVAL_POLICY_SCHEMA_V2,
    RETRIEVAL_QUOTA_SEMANTICS_VERSION_V2, RETRIEVAL_RERANK_PROTOCOL_VERSION_V2,
    RETRIEVAL_RERANK_SCORE_NORMALIZATION_VERSION_V2, RETRIEVAL_TRUSTED_SOURCE_TYPES_V2,
    RetrievalEmbeddingPolicyV2, RetrievalKeywordPolicyV2, RetrievalPolicyIdentityV1,
    RetrievalPolicyV2, RetrievalRankingPolicyV2, RetrievalRequestQuotasV2, RetrievalRerankPolicyV2,
    RetrievalRrfPolicyV2,
};
use sqlx::PgPool;
use storage::knowledge_retrieval::PostgresKnowledgeRetrievalAdapter;
use uuid::Uuid;

mod support;

struct VersionFixture {
    product_id: Uuid,
    version_id: Uuid,
    document_id: Uuid,
    object_ref: String,
}

struct Fixture {
    workspace_id: Uuid,
    versions: Vec<VersionFixture>,
}

fn policy_artifact(max_hits: u32, max_chunk_bytes: u32, max_total_bytes: u64) -> RetrievalPolicyV2 {
    RetrievalPolicyV2 {
        schema_version: RETRIEVAL_POLICY_SCHEMA_V2,
        contract_version: KNOWLEDGE_EVIDENCE_CONTRACT_V2.into(),
        normalization_version: RETRIEVAL_NORMALIZATION_VERSION_V2.into(),
        trusted_source_types: RETRIEVAL_TRUSTED_SOURCE_TYPES_V2
            .iter()
            .map(|value| (*value).into())
            .collect(),
        ranking: RetrievalRankingPolicyV2 {
            a_primary_comparator: RETRIEVAL_A_PRIMARY_COMPARATOR_V2
                .iter()
                .map(|value| (*value).into())
                .collect(),
            a_version_comparator: RETRIEVAL_A_VERSION_COMPARATOR_V2
                .iter()
                .map(|value| (*value).into())
                .collect(),
            b_exact_comparator: RETRIEVAL_B_EXACT_COMPARATOR_V2
                .iter()
                .map(|value| (*value).into())
                .collect(),
            c_semantic_comparator: RETRIEVAL_C_SEMANTIC_COMPARATOR_V2
                .iter()
                .map(|value| (*value).into())
                .collect(),
            quota_semantics_version: RETRIEVAL_QUOTA_SEMANTICS_VERSION_V2.into(),
        },
        keyword: RetrievalKeywordPolicyV2 {
            tokenizer: RETRIEVAL_KEYWORD_TOKENIZER_V2.into(),
            tokenizer_version: RETRIEVAL_KEYWORD_TOKENIZER_VERSION_V2.into(),
            top_k: 128,
            threshold_millionths: 50_000,
        },
        embedding: RetrievalEmbeddingPolicyV2 {
            policy: RETRIEVAL_EMBEDDING_POLICY_V2.into(),
            policy_version: RETRIEVAL_EMBEDDING_POLICY_VERSION_V2.into(),
            model_revision_sha256: domain::sha256_hex(Uuid::new_v4().as_bytes()),
            top_k: 128,
            threshold_millionths: 100_000,
        },
        rrf: RetrievalRrfPolicyV2 {
            k: 60,
            keyword_weight_millionths: 1_000_000,
            vector_weight_millionths: 1_000_000,
        },
        rerank: RetrievalRerankPolicyV2 {
            provider_protocol_version: RETRIEVAL_RERANK_PROTOCOL_VERSION_V2.into(),
            model_revision_sha256: domain::sha256_hex(Uuid::new_v4().as_bytes()),
            config_revision_sha256: domain::sha256_hex(Uuid::new_v4().as_bytes()),
            top_k: 64,
            timeout_ms: 5_000,
            score_normalization_version: RETRIEVAL_RERANK_SCORE_NORMALIZATION_VERSION_V2.into(),
        },
        request_quotas: RetrievalRequestQuotasV2 {
            max_hits,
            max_chunk_bytes,
            max_total_bytes,
        },
    }
}

async fn register_policy(
    pool: &PgPool,
    max_hits: u32,
    max_chunk_bytes: u32,
    max_total_bytes: u64,
) -> RetrievalPolicyIdentityV1 {
    let artifact = policy_artifact(max_hits, max_chunk_bytes, max_total_bytes);
    artifact.validate().unwrap();
    let identity = artifact.request_identity().unwrap();
    sqlx::query(
        "INSERT INTO knowledge_retrieval_policies_v2(
             policy_sha256,canonical_policy_payload,contract_version,
             max_hits,max_chunk_bytes,max_total_bytes)
         VALUES($1,$2,$3,$4,$5,$6)",
    )
    .bind(&identity.policy_sha256)
    .bind(artifact.canonical_bytes().unwrap())
    .bind(&identity.contract_version)
    .bind(i64::from(identity.max_hits))
    .bind(i64::from(identity.max_chunk_bytes))
    .bind(i64::try_from(identity.max_total_bytes).unwrap())
    .execute(pool)
    .await
    .unwrap();
    identity
}

async fn final_schema(pool: &PgPool) -> bool {
    let ready =
        sqlx::query_scalar("SELECT to_regclass('knowledge_retrieval_policies_v2') IS NOT NULL")
            .fetch_one(pool)
            .await
            .unwrap_or(false);
    support::require_final_schema("KnowledgeRetrievalExactV2", ready)
}

async fn new_fixture(pool: &PgPool) -> Fixture {
    let workspace_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO workspaces(id,name,slug,kind) VALUES($1,'exact v2',$2,'product_line')",
    )
    .bind(workspace_id)
    .bind(format!("exact-v2-{workspace_id}"))
    .execute(pool)
    .await
    .unwrap();
    Fixture {
        workspace_id,
        versions: Vec::new(),
    }
}

async fn add_version(pool: &PgPool, fixture: &mut Fixture, chunks: &[(&str, &str)]) -> Uuid {
    let product_id = Uuid::new_v4();
    let version_id = Uuid::new_v4();
    let document_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO products(id,workspace_id,kind,name,slug)
         VALUES($1,$2,'product','exact v2 product',$3)",
    )
    .bind(product_id)
    .bind(fixture.workspace_id)
    .bind(format!("exact-v2-{product_id}"))
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO product_versions(id,product_id,label,status) VALUES($1,$2,'v1','active')",
    )
    .bind(version_id)
    .bind(product_id)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("UPDATE products SET current_version_id=$2 WHERE id=$1")
        .bind(product_id)
        .bind(version_id)
        .execute(pool)
        .await
        .unwrap();

    let file_hash = domain::sha256_hex(document_id.as_bytes());
    let object_ref = format!("objects/{file_hash}");
    sqlx::query(
        "INSERT INTO object_registry(object_ref,digest,media_type,byte_length,state)
         VALUES($1,$2,'text/plain',1,'available')",
    )
    .bind(&object_ref)
    .bind(&file_hash)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO object_owner_references(
             object_ref,owner_kind,owner_id,occurrence,created_by)
         VALUES($1,'knowledge_document',$2,'original','system:knowledge-document-ingest')",
    )
    .bind(&object_ref)
    .bind(document_id)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO documents(
             id,product_version_id,title,parse_status,enable_status,index_ready,
             file_name,file_size,file_hash,object_ref)
         VALUES($1,$2,'exact v2','completed','enabled',true,$3,1,$4,$5)",
    )
    .bind(document_id)
    .bind(version_id)
    .bind(format!("{version_id}.txt"))
    .bind(&file_hash)
    .bind(&object_ref)
    .execute(pool)
    .await
    .unwrap();
    for (chunk_type, content) in chunks {
        sqlx::query(
            "INSERT INTO chunks(id,product_version_id,document_id,chunk_type,content)
             VALUES($1,$2,$3,$4,$5)",
        )
        .bind(Uuid::new_v4())
        .bind(version_id)
        .bind(document_id)
        .bind(chunk_type)
        .bind(content)
        .execute(pool)
        .await
        .unwrap();
    }
    fixture.versions.push(VersionFixture {
        product_id,
        version_id,
        document_id,
        object_ref,
    });
    version_id
}

async fn remove_fixture(pool: &PgPool, fixture: &Fixture) {
    let documents = fixture
        .versions
        .iter()
        .map(|version| version.document_id)
        .collect::<Vec<_>>();
    let versions = fixture
        .versions
        .iter()
        .map(|version| version.version_id)
        .collect::<Vec<_>>();
    let products = fixture
        .versions
        .iter()
        .map(|version| version.product_id)
        .collect::<Vec<_>>();
    let object_refs = fixture
        .versions
        .iter()
        .map(|version| version.object_ref.clone())
        .collect::<Vec<_>>();
    sqlx::query("DELETE FROM chunks WHERE document_id=ANY($1)")
        .bind(&documents)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM object_owner_references WHERE object_ref=ANY($1)")
        .bind(&object_refs)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM documents WHERE id=ANY($1)")
        .bind(&documents)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM object_registry WHERE object_ref=ANY($1)")
        .bind(&object_refs)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("UPDATE products SET current_version_id=NULL WHERE id=ANY($1)")
        .bind(&products)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM product_versions WHERE id=ANY($1)")
        .bind(&versions)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM products WHERE id=ANY($1)")
        .bind(&products)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM workspaces WHERE id=$1")
        .bind(fixture.workspace_id)
        .execute(pool)
        .await
        .unwrap();
}

fn scope(
    requirement: &str,
    versions: Vec<Uuid>,
    policy: RetrievalPolicyIdentityV1,
) -> KnowledgeEvidenceScopeV2 {
    KnowledgeEvidenceScopeV2::ProductLine(ProductEvidenceRequestV1 {
        schema_version: KNOWLEDGE_EVIDENCE_SCHEMA_V1,
        requirement_identity_sha256: domain::sha256_hex(requirement.as_bytes()),
        requirement_text: requirement.into(),
        product_version_ids: versions,
        retrieval_policy: policy,
    })
}

#[tokio::test]
async fn exact_v2_returns_only_complete_trusted_snapshots_deterministically() {
    let Some(pool) = support::connect_postgres_contract("KnowledgeRetrievalExactV2").await else {
        return;
    };
    if !final_schema(&pool).await {
        return;
    }
    let mut fixture = new_fixture(&pool).await;
    let exact = add_version(
        &pool,
        &mut fixture,
        &[
            ("text", "设备 支持 Alpha 条款"),
            ("parent_text", "PARENT alpha条款"),
            ("image_ocr", "图片 ALPHA\n条款"),
            ("image_caption", "caption alpha条款"),
            ("question", "question alpha条款"),
        ],
    )
    .await;
    let no_hit = add_version(&pool, &mut fixture, &[("text", "unrelated")]).await;
    let policy = register_policy(&pool, 8, 1024, 4096).await;
    let adapter = PostgresKnowledgeRetrievalAdapter::new(pool.clone());
    let request = scope("Al PhA 条款", vec![exact, no_hit], policy);
    let first = adapter.retrieve_evidence_v2(request.clone()).await.unwrap();
    let second = adapter.retrieve_evidence_v2(request).await.unwrap();
    remove_fixture(&pool, &fixture).await;

    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&second).unwrap()
    );
    assert_eq!(first.eligible_versions.len(), 2);
    assert_eq!(first.hits.len(), 3);
    assert_eq!(first.exact_versions_truncated, 0);
    assert_eq!(first.exact_hits_truncated, 0);
    assert_eq!(first.semantic_hits_truncated, 0);
    assert_eq!(
        first
            .hits
            .iter()
            .map(|hit| serde_json::to_value(hit.source_type).unwrap())
            .collect::<std::collections::HashSet<_>>(),
        ["text", "parent_text", "image_ocr"]
            .into_iter()
            .map(serde_json::Value::from)
            .collect()
    );
    for (index, hit) in first.hits.iter().enumerate() {
        assert_eq!(hit.retrieval_rank, index as u32 + 1);
        assert_eq!(hit.retrieval_raw_score, "1.000000");
        assert_eq!(hit.quote_start_offset, 0);
        assert_eq!(hit.quote_end_offset, hit.chunk_byte_length);
    }
}

#[tokio::test]
async fn exact_v2_fairness_and_quota_fail_closed_are_explicit() {
    let Some(pool) = support::connect_postgres_contract("KnowledgeRetrievalExactV2Quota").await
    else {
        return;
    };
    if !final_schema(&pool).await {
        return;
    }
    let mut fixture = new_fixture(&pool).await;
    let first = add_version(&pool, &mut fixture, &[("text", "needle")]).await;
    let second = add_version(&pool, &mut fixture, &[("text", "needle")]).await;
    let third = add_version(&pool, &mut fixture, &[("text", "needle")]).await;
    let oversized = add_version(&pool, &mut fixture, &[("text", "needle-too-large")]).await;
    let adapter = PostgresKnowledgeRetrievalAdapter::new(pool.clone());

    let fairness_policy = register_policy(&pool, 2, 1024, 4096).await;
    let fairness = adapter
        .retrieve_evidence_v2(scope("needle", vec![first, second, third], fairness_policy))
        .await
        .unwrap();
    assert_eq!(fairness.eligible_versions.len(), 3);
    assert_eq!(fairness.hits.len(), 2);
    assert_eq!(fairness.exact_versions_truncated, 1);
    assert_eq!(fairness.exact_hits_truncated, 1);

    let chunk_policy = register_policy(&pool, 1, 6, 100).await;
    let chunk_error = adapter
        .retrieve_evidence_v2(scope("needle", vec![oversized], chunk_policy))
        .await;
    assert!(matches!(
        chunk_error,
        Err(KnowledgeRetrievalError::QuotaExceeded(_))
    ));

    let total_policy = register_policy(&pool, 2, 100, 11).await;
    let total_error = adapter
        .retrieve_evidence_v2(scope("needle", vec![first, second], total_policy))
        .await;
    assert!(matches!(
        total_error,
        Err(KnowledgeRetrievalError::QuotaExceeded(_))
    ));
    remove_fixture(&pool, &fixture).await;
}

#[tokio::test]
async fn exact_v2_rejects_selected_unknown_mismatched_and_revoked_policy() {
    let Some(pool) = support::connect_postgres_contract("KnowledgeRetrievalExactV2Policy").await
    else {
        return;
    };
    if !final_schema(&pool).await {
        return;
    }
    let mut fixture = new_fixture(&pool).await;
    let version = add_version(&pool, &mut fixture, &[("text", "needle")]).await;
    let adapter = PostgresKnowledgeRetrievalAdapter::new(pool.clone());
    let policy = register_policy(&pool, 2, 100, 100).await;

    let missing = adapter
        .retrieve_evidence_v2(scope("needle", vec![Uuid::new_v4()], policy.clone()))
        .await;
    assert!(matches!(
        missing,
        Err(KnowledgeRetrievalError::InvalidRequest(_))
    ));

    let mut unknown = policy.clone();
    unknown.policy_sha256 = domain::sha256_hex(Uuid::new_v4().as_bytes());
    let unknown = adapter
        .retrieve_evidence_v2(scope("needle", vec![version], unknown))
        .await;
    assert!(matches!(
        unknown,
        Err(KnowledgeRetrievalError::InvalidRequest(_))
    ));

    let mut mismatch = policy.clone();
    mismatch.max_hits += 1;
    let mismatch = adapter
        .retrieve_evidence_v2(scope("needle", vec![version], mismatch))
        .await;
    assert!(matches!(
        mismatch,
        Err(KnowledgeRetrievalError::InvalidRequest(_))
    ));

    sqlx::query(
        "UPDATE knowledge_retrieval_policies_v2 SET support_state='revoked' WHERE policy_sha256=$1",
    )
    .bind(&policy.policy_sha256)
    .execute(&pool)
    .await
    .unwrap();
    let revoked = adapter
        .retrieve_evidence_v2(scope("needle", vec![version], policy))
        .await;
    assert!(matches!(
        revoked,
        Err(KnowledgeRetrievalError::InvalidRequest(_))
    ));
    remove_fixture(&pool, &fixture).await;
}
