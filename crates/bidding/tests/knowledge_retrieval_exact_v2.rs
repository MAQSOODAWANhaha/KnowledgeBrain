#![cfg(feature = "knowledge-v2-exact-contract-tests")]

use knowledge::knowledge_retrieval::{
    EMBEDDING_DIMENSION_V2, EMBEDDING_OUTPUT_NORMALIZATION_VERSION_V2,
    EMBEDDING_PROVIDER_PROTOCOL_VERSION_V2, EMBEDDING_REVISION_SCHEMA_V2, EmbeddingRevisionV2,
    KNOWLEDGE_EVIDENCE_CONTRACT_V2, KNOWLEDGE_EVIDENCE_SCHEMA_V1, KnowledgeEvidenceScopeV2,
    KnowledgeRetrievalError, KnowledgeRetrievalPortV2, ProductEvidenceRequestV1,
    RERANK_REQUEST_CONFIG_SHA256_V2, RERANK_REVISION_SCHEMA_V2, RETRIEVAL_A_PRIMARY_COMPARATOR_V2,
    RETRIEVAL_A_VERSION_COMPARATOR_V2, RETRIEVAL_B_EXACT_COMPARATOR_V2,
    RETRIEVAL_C_SEMANTIC_COMPARATOR_V2, RETRIEVAL_CHANNEL_RANK_COMPARATOR_V2,
    RETRIEVAL_CHANNEL_SCORE_QUANTIZATION_VERSION_V2, RETRIEVAL_EMBEDDING_POLICY_V2,
    RETRIEVAL_EMBEDDING_POLICY_VERSION_V2, RETRIEVAL_EMBEDDING_SIMILARITY_VERSION_V2,
    RETRIEVAL_KEYWORD_SCORE_VERSION_V2, RETRIEVAL_KEYWORD_TOKENIZER_V2,
    RETRIEVAL_KEYWORD_TOKENIZER_VERSION_V2, RETRIEVAL_NORMALIZATION_VERSION_V2,
    RETRIEVAL_POLICY_SCHEMA_V2, RETRIEVAL_PRE_RERANK_RRF_COMPARATOR_V2,
    RETRIEVAL_QUOTA_SEMANTICS_VERSION_V2, RETRIEVAL_RERANK_PROTOCOL_VERSION_V2,
    RETRIEVAL_RERANK_SCORE_NORMALIZATION_VERSION_V2, RETRIEVAL_RRF_SCORE_REPRESENTATION_VERSION_V2,
    RETRIEVAL_SOURCE_FOLDING_VERSION_V2, RETRIEVAL_TRUSTED_SOURCE_TYPES_V2, RerankRevisionV2,
    RetrievalEmbeddingPolicyV2, RetrievalKeywordPolicyV2, RetrievalPolicyIdentityV1,
    RetrievalPolicyV2, RetrievalRankingPolicyV2, RetrievalRequestQuotasV2, RetrievalRerankPolicyV2,
    RetrievalRrfPolicyV2,
};
use sqlx::PgPool;
use knowledge::PostgresKnowledgeRetrievalAdapter;
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

fn embedding_revision_for(provider_model_identifier: impl Into<String>) -> EmbeddingRevisionV2 {
    EmbeddingRevisionV2 {
        schema_version: EMBEDDING_REVISION_SCHEMA_V2,
        provider_protocol_version: EMBEDDING_PROVIDER_PROTOCOL_VERSION_V2.into(),
        provider_model_identifier: provider_model_identifier.into(),
        provider_model_revision_sha256: knowledge::sha256_hex(
            b"exact-v2 fixture immutable provider revision metadata",
        ),
        endpoint_config_sha256: knowledge::sha256_hex(
            b"exact-v2 fixture immutable endpoint preprocessing config",
        ),
        endpoint_identity: "https://embeddings.example.test/v1/embeddings".into(),
        dimension: EMBEDDING_DIMENSION_V2,
        request_config_sha256: EmbeddingRevisionV2::canonical_request_config_sha256(),
        output_normalization_version: EMBEDDING_OUTPUT_NORMALIZATION_VERSION_V2.into(),
    }
}

fn embedding_revision() -> EmbeddingRevisionV2 {
    embedding_revision_for("exact-v2-fixture@2025-01-15")
}

fn rerank_revision() -> RerankRevisionV2 {
    RerankRevisionV2 {
        schema_version: RERANK_REVISION_SCHEMA_V2,
        provider_protocol_version: RETRIEVAL_RERANK_PROTOCOL_VERSION_V2.into(),
        provider_model_identifier: "exact-v2-reranker@2025-01-15".into(),
        provider_model_revision_sha256: knowledge::sha256_hex(b"exact-v2 reranker model"),
        config_revision_sha256: knowledge::sha256_hex(b"exact-v2 reranker config"),
        endpoint_identity: "https://rerank.example.test/v1/rerank".into(),
        request_config_sha256: RERANK_REQUEST_CONFIG_SHA256_V2.into(),
        score_normalization_version: RETRIEVAL_RERANK_SCORE_NORMALIZATION_VERSION_V2.into(),
    }
}

async fn register_rerank_revision(pool: &PgPool) {
    let reranker = rerank_revision();
    sqlx::query(
        "INSERT INTO rerank_revisions_v2(
             revision_sha256,canonical_revision_payload,schema_version,
             provider_protocol_version,provider_model_identifier,
             provider_model_revision_sha256,config_revision_sha256,endpoint_identity,
             request_config_sha256,score_normalization_version,credential_ref)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,'test:rerank-exact-v2')
         ON CONFLICT (revision_sha256) DO NOTHING",
    )
    .bind(reranker.sha256().unwrap())
    .bind(reranker.canonical_bytes().unwrap())
    .bind(i16::try_from(reranker.schema_version).unwrap())
    .bind(&reranker.provider_protocol_version)
    .bind(&reranker.provider_model_identifier)
    .bind(&reranker.provider_model_revision_sha256)
    .bind(&reranker.config_revision_sha256)
    .bind(&reranker.endpoint_identity)
    .bind(&reranker.request_config_sha256)
    .bind(&reranker.score_normalization_version)
    .execute(pool)
    .await
    .unwrap();
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
            source_folding_version: RETRIEVAL_SOURCE_FOLDING_VERSION_V2.into(),
            channel_score_quantization_version: RETRIEVAL_CHANNEL_SCORE_QUANTIZATION_VERSION_V2
                .into(),
            channel_rank_comparator: RETRIEVAL_CHANNEL_RANK_COMPARATOR_V2
                .iter()
                .map(|value| (*value).into())
                .collect(),
            pre_rerank_rrf_comparator: RETRIEVAL_PRE_RERANK_RRF_COMPARATOR_V2
                .iter()
                .map(|value| (*value).into())
                .collect(),
            quota_semantics_version: RETRIEVAL_QUOTA_SEMANTICS_VERSION_V2.into(),
        },
        keyword: RetrievalKeywordPolicyV2 {
            tokenizer: RETRIEVAL_KEYWORD_TOKENIZER_V2.into(),
            tokenizer_version: RETRIEVAL_KEYWORD_TOKENIZER_VERSION_V2.into(),
            score_version: RETRIEVAL_KEYWORD_SCORE_VERSION_V2.into(),
            top_k: 128,
            threshold_millionths: 50_000,
        },
        embedding: RetrievalEmbeddingPolicyV2 {
            policy: RETRIEVAL_EMBEDDING_POLICY_V2.into(),
            policy_version: RETRIEVAL_EMBEDDING_POLICY_VERSION_V2.into(),
            similarity_version: RETRIEVAL_EMBEDDING_SIMILARITY_VERSION_V2.into(),
            model_revision_sha256: embedding_revision().sha256().unwrap(),
            top_k: 128,
            threshold_millionths: 100_000,
        },
        rrf: RetrievalRrfPolicyV2 {
            k: 60,
            keyword_weight_millionths: 1_000_000,
            vector_weight_millionths: 1_000_000,
            score_representation_version: RETRIEVAL_RRF_SCORE_REPRESENTATION_VERSION_V2.into(),
        },
        rerank: RetrievalRerankPolicyV2 {
            provider_protocol_version: RETRIEVAL_RERANK_PROTOCOL_VERSION_V2.into(),
            revision_sha256: rerank_revision().sha256().unwrap(),
            model_revision_sha256: rerank_revision().provider_model_revision_sha256,
            config_revision_sha256: rerank_revision().config_revision_sha256,
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
    let revision = embedding_revision();
    let revision_bytes = revision.canonical_bytes().unwrap();
    let revision_sha256 = revision.sha256().unwrap();
    sqlx::query(
        "INSERT INTO embedding_revisions_v2(
             revision_sha256,canonical_revision_payload,schema_version,
             provider_protocol_version,provider_model_identifier,
             provider_model_revision_sha256,endpoint_config_sha256,endpoint_identity,
             dimension,request_config_sha256,output_normalization_version,credential_ref)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,'test:embedding-exact-v2')
         ON CONFLICT (revision_sha256) DO NOTHING",
    )
    .bind(&revision_sha256)
    .bind(&revision_bytes)
    .bind(i16::try_from(revision.schema_version).unwrap())
    .bind(&revision.provider_protocol_version)
    .bind(&revision.provider_model_identifier)
    .bind(&revision.provider_model_revision_sha256)
    .bind(&revision.endpoint_config_sha256)
    .bind(&revision.endpoint_identity)
    .bind(i32::try_from(revision.dimension).unwrap())
    .bind(&revision.request_config_sha256)
    .bind(&revision.output_normalization_version)
    .execute(pool)
    .await
    .unwrap();
    let revision_matches: bool = sqlx::query_scalar(
        "SELECT canonical_revision_payload=$2
           AND schema_version=$3 AND provider_protocol_version=$4
           AND provider_model_identifier=$5 AND provider_model_revision_sha256=$6
           AND endpoint_config_sha256=$7 AND endpoint_identity=$8 AND dimension=$9
           AND request_config_sha256=$10 AND output_normalization_version=$11
           AND credential_ref='test:embedding-exact-v2'
           FROM embedding_revisions_v2 WHERE revision_sha256=$1",
    )
    .bind(&revision_sha256)
    .bind(&revision_bytes)
    .bind(i16::try_from(revision.schema_version).unwrap())
    .bind(&revision.provider_protocol_version)
    .bind(&revision.provider_model_identifier)
    .bind(&revision.provider_model_revision_sha256)
    .bind(&revision.endpoint_config_sha256)
    .bind(&revision.endpoint_identity)
    .bind(i32::try_from(revision.dimension).unwrap())
    .bind(&revision.request_config_sha256)
    .bind(&revision.output_normalization_version)
    .fetch_one(pool)
    .await
    .unwrap();
    assert!(revision_matches);

    register_rerank_revision(pool).await;

    let artifact = policy_artifact(max_hits, max_chunk_bytes, max_total_bytes);
    artifact.validate().unwrap();
    let identity = artifact.request_identity().unwrap();
    sqlx::query(
        "INSERT INTO knowledge_retrieval_policies_v2(
             policy_sha256,canonical_policy_payload,embedding_revision_sha256,contract_version,
             max_hits,max_chunk_bytes,max_total_bytes)
         VALUES($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(&identity.policy_sha256)
    .bind(artifact.canonical_bytes().unwrap())
    .bind(&revision_sha256)
    .bind(&identity.contract_version)
    .bind(i64::from(identity.max_hits))
    .bind(i64::from(identity.max_chunk_bytes))
    .bind(i64::try_from(identity.max_total_bytes).unwrap())
    .execute(pool)
    .await
    .unwrap();
    identity
}

async fn insert_policy_raw(
    pool: &PgPool,
    payload: Vec<u8>,
    max_hits: i64,
    max_chunk_bytes: i64,
    max_total_bytes: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO knowledge_retrieval_policies_v2(
             policy_sha256,canonical_policy_payload,embedding_revision_sha256,contract_version,
             max_hits,max_chunk_bytes,max_total_bytes)
         VALUES($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(knowledge::sha256_hex(&payload))
    .bind(payload)
    .bind(embedding_revision().sha256().unwrap())
    .bind(KNOWLEDGE_EVIDENCE_CONTRACT_V2)
    .bind(max_hits)
    .bind(max_chunk_bytes)
    .bind(max_total_bytes)
    .execute(pool)
    .await
    .map(|_| ())
}

fn assert_policy_payload_constraint(error: sqlx::Error) {
    assert_eq!(
        error
            .as_database_error()
            .and_then(|database_error| database_error.constraint()),
        Some("knowledge_retrieval_policies_v2_payload_matches_columns")
    );
}

async fn insert_embedding_revision_raw(
    pool: &PgPool,
    revision: &EmbeddingRevisionV2,
    canonical_revision_payload: Vec<u8>,
    credential_ref: &str,
) -> Result<(), sqlx::Error> {
    let revision_sha256 = knowledge::sha256_hex(&canonical_revision_payload);
    sqlx::query(
        "INSERT INTO embedding_revisions_v2(
             revision_sha256,canonical_revision_payload,schema_version,
             provider_protocol_version,provider_model_identifier,
             provider_model_revision_sha256,endpoint_config_sha256,endpoint_identity,
             dimension,request_config_sha256,output_normalization_version,credential_ref)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
    )
    .bind(revision_sha256)
    .bind(canonical_revision_payload)
    .bind(i16::try_from(revision.schema_version).unwrap())
    .bind(&revision.provider_protocol_version)
    .bind(&revision.provider_model_identifier)
    .bind(&revision.provider_model_revision_sha256)
    .bind(&revision.endpoint_config_sha256)
    .bind(&revision.endpoint_identity)
    .bind(i32::try_from(revision.dimension).unwrap())
    .bind(&revision.request_config_sha256)
    .bind(&revision.output_normalization_version)
    .bind(credential_ref)
    .execute(pool)
    .await
    .map(|_| ())
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

    let file_hash = knowledge::sha256_hex(document_id.as_bytes());
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
        requirement_identity_sha256: knowledge::sha256_hex(requirement.as_bytes()),
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
    let adapter = PostgresKnowledgeRetrievalAdapter::new_exact_only_v2_contract_tests(pool.clone());
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
    assert_eq!(first.exact_prefix_hit_count, 3);
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
    let adapter = PostgresKnowledgeRetrievalAdapter::new_exact_only_v2_contract_tests(pool.clone());

    let fairness_policy = register_policy(&pool, 2, 1024, 4096).await;
    let fairness = adapter
        .retrieve_evidence_v2(scope("needle", vec![first, second, third], fairness_policy))
        .await
        .unwrap();
    assert_eq!(fairness.eligible_versions.len(), 3);
    assert_eq!(fairness.hits.len(), 2);
    assert_eq!(fairness.exact_prefix_hit_count, 2);
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
async fn exact_v2_revalidates_the_canonical_policy_artifact() {
    let Some(pool) = support::connect_postgres_contract("KnowledgeRetrievalExactV2Artifact").await
    else {
        return;
    };
    if !final_schema(&pool).await {
        return;
    }
    let mut fixture = new_fixture(&pool).await;
    let version = add_version(&pool, &mut fixture, &[("text", "needle")]).await;
    let adapter = PostgresKnowledgeRetrievalAdapter::new_exact_only_v2_contract_tests(pool.clone());

    let valid_policy = register_policy(&pool, 2, 100, 100).await;
    let valid = adapter
        .retrieve_evidence_v2(scope("needle", vec![version], valid_policy))
        .await
        .unwrap();
    assert_eq!(valid.exact_prefix_hit_count, 1);

    let canonical_artifact = policy_artifact(2, 100, 100);
    let canonical_payload = canonical_artifact.canonical_bytes().unwrap();
    let helper_accepts_canonical: bool =
        sqlx::query_scalar("SELECT kb_knowledge_valid_retrieval_policy_v2($1,$2,$3,$4,$5,$6)")
            .bind(&canonical_payload)
            .bind(embedding_revision().sha256().unwrap())
            .bind(KNOWLEDGE_EVIDENCE_CONTRACT_V2)
            .bind(2_i64)
            .bind(100_i64)
            .bind(100_i64)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(helper_accepts_canonical);

    let mut unsupported_normalization = canonical_artifact.clone();
    unsupported_normalization.normalization_version = "unsupported-normalization-v999".into();
    let mut wrong_ranking = canonical_artifact.clone();
    wrong_ranking.ranking.a_primary_comparator[0] = "document_id ASC".into();
    let mut wrong_tokenizer = canonical_artifact.clone();
    wrong_tokenizer.keyword.tokenizer = "mutable-tokenizer".into();
    let mut wrong_rrf_representation = canonical_artifact.clone();
    wrong_rrf_representation.rrf.score_representation_version = "decimal-v999".into();
    let mut wrong_rerank_protocol = canonical_artifact.clone();
    wrong_rerank_protocol.rerank.provider_protocol_version = "latest".into();

    let canonical_text = String::from_utf8(canonical_payload.clone()).unwrap();
    let canonical_prefix = "{\"schema_version\":2,\"contract_version\":\"knowledge-evidence-v2\",";
    let reordered_payload = format!(
        "{{\"contract_version\":\"knowledge-evidence-v2\",\"schema_version\":2,{}",
        canonical_text.strip_prefix(canonical_prefix).unwrap()
    )
    .into_bytes();
    let duplicate_key_payload = canonical_text
        .replacen('{', "{\"schema_version\":2,", 1)
        .into_bytes();
    let missing_key_payload = canonical_text
        .replacen(
            "\"normalization_version\":\"unicode-whitespace-lowercase-v1\",",
            "",
            1,
        )
        .into_bytes();
    let extra_key_payload = canonical_text
        .replacen(
            "\"schema_version\":2,",
            "\"schema_version\":2,\"extra\":true,",
            1,
        )
        .into_bytes();
    let alternate_number_payload = canonical_text
        .replacen("\"top_k\":128", "\"top_k\":1.28e2", 1)
        .into_bytes();

    let malformed_payloads = [
        serde_json::to_vec(&unsupported_normalization).unwrap(),
        serde_json::to_vec_pretty(&canonical_artifact).unwrap(),
        reordered_payload,
        duplicate_key_payload,
        missing_key_payload,
        extra_key_payload,
        alternate_number_payload,
        serde_json::to_vec(&wrong_ranking).unwrap(),
        serde_json::to_vec(&wrong_tokenizer).unwrap(),
        serde_json::to_vec(&wrong_rrf_representation).unwrap(),
        serde_json::to_vec(&wrong_rerank_protocol).unwrap(),
    ];
    for malformed_payload in malformed_payloads {
        let helper_rejects_malformed: bool =
            sqlx::query_scalar("SELECT kb_knowledge_valid_retrieval_policy_v2($1,$2,$3,$4,$5,$6)")
                .bind(&malformed_payload)
                .bind(embedding_revision().sha256().unwrap())
                .bind(KNOWLEDGE_EVIDENCE_CONTRACT_V2)
                .bind(2_i64)
                .bind(100_i64)
                .bind(100_i64)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(!helper_rejects_malformed);
        assert_policy_payload_constraint(
            insert_policy_raw(&pool, malformed_payload, 2, 100, 100)
                .await
                .unwrap_err(),
        );
    }
    remove_fixture(&pool, &fixture).await;
}

#[tokio::test]
async fn embedding_revision_registry_binding_sidecars_and_revocation_are_enforced() {
    let Some(pool) = support::connect_postgres_contract("EmbeddingRevisionV2Registry").await else {
        return;
    };
    if !final_schema(&pool).await {
        return;
    }
    register_rerank_revision(&pool).await;

    for role in ["kb_runtime_api", "kb_runtime_worker"] {
        for table in [
            "embedding_revisions_v2",
            "knowledge_retrieval_policies_v2",
            "product_version_embedding_bindings_v2",
        ] {
            let privileges: (bool, bool, bool, bool) = sqlx::query_as(
                "SELECT has_table_privilege($1,$2,'SELECT'),
                        has_table_privilege($1,$2,'INSERT'),
                        has_table_privilege($1,$2,'UPDATE'),
                        has_table_privilege($1,$2,'DELETE')",
            )
            .bind(role)
            .bind(table)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(privileges, (true, false, false, false));
        }
    }
    for table in ["chunk_keyword_indexes_v2", "chunk_vector_indexes_v2"] {
        let api_privileges: (bool, bool, bool, bool) = sqlx::query_as(
            "SELECT has_table_privilege('kb_runtime_api',$1,'SELECT'),
                    has_table_privilege('kb_runtime_api',$1,'INSERT'),
                    has_table_privilege('kb_runtime_api',$1,'UPDATE'),
                    has_table_privilege('kb_runtime_api',$1,'DELETE')",
        )
        .bind(table)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(api_privileges, (true, false, false, false));
        let worker_privileges: (bool, bool, bool, bool) = sqlx::query_as(
            "SELECT has_table_privilege('kb_runtime_worker',$1,'SELECT'),
                    has_table_privilege('kb_runtime_worker',$1,'INSERT'),
                    has_table_privilege('kb_runtime_worker',$1,'UPDATE'),
                    has_table_privilege('kb_runtime_worker',$1,'DELETE')",
        )
        .bind(table)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(worker_privileges, (true, false, false, false));
    }
    let worker_vector_reconcile: bool = sqlx::query_scalar(
        "SELECT has_function_privilege(
            'kb_runtime_worker',
            'kb_knowledge_reconcile_vector_indexes_v2(uuid,text,text,jsonb)',
            'EXECUTE')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(worker_vector_reconcile);

    let sidecar_counts: Vec<i64> = sqlx::query_scalar(
        "SELECT count(*) FROM chunk_keyword_indexes_v2
         UNION ALL SELECT count(*) FROM chunk_vector_indexes_v2",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(sidecar_counts, vec![0, 0]);
    let legacy_columns: Vec<String> = sqlx::query_scalar(
        "SELECT column_name FROM information_schema.columns
          WHERE table_schema='public' AND table_name='chunk_embeddings'
          ORDER BY ordinal_position",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        legacy_columns,
        [
            "chunk_id",
            "product_version_id",
            "document_id",
            "embedding",
            "tsv",
            "content"
        ]
    );
    let legacy_rows_before: i64 = sqlx::query_scalar("SELECT count(*) FROM chunk_embeddings")
        .fetch_one(&pool)
        .await
        .unwrap();

    for invalid_identifier in [
        "prod",
        "bare",
        "unversioned",
        "model@latest",
        "model@stable",
        "model@2025-02-29",
    ] {
        let mut malformed = embedding_revision();
        malformed.provider_model_identifier = invalid_identifier.into();
        let payload = serde_json::to_vec(&malformed).unwrap();
        assert!(
            insert_embedding_revision_raw(&pool, &malformed, payload, "test:malformed-model")
                .await
                .is_err()
        );
    }
    for accepted_endpoint in [
        "https://localhost:8443",
        "https://embedding-api.example.test/v1/embeddings",
        "https://a-b.c9:8080/path_1/~model.v2",
    ] {
        let accepted: bool =
            sqlx::query_scalar("SELECT kb_knowledge_valid_endpoint_identity_v2($1)")
                .bind(accepted_endpoint)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(accepted, "accepted endpoint {accepted_endpoint}");
    }
    for invalid_endpoint in [
        "ftp://localhost",
        "http://localhost",
        "http://a-b.c9:8080/path_1/~model.v2",
        "HTTP://localhost",
        "https://[::1]",
        "https://[]",
        "https://[not-an-ip]",
        "https://-host.example",
        "https://host-.example",
        "https://host_name.example",
        "https://Host.example",
        "http://localhost:80",
        "https://localhost:443",
        "https://localhost:0",
        "https://localhost:01",
        "https://localhost:65536",
        "https://localhost/",
        "https://localhost/a//b",
        "https://localhost/.",
        "https://localhost/..",
        "https://localhost/a/b%20c",
        "https://localhost/a+b",
        "https://user@localhost/path",
        "https://localhost/path?query",
        "https://localhost/path#fragment",
        "https://localhost/white space",
    ] {
        let accepted: bool =
            sqlx::query_scalar("SELECT kb_knowledge_valid_endpoint_identity_v2($1)")
                .bind(invalid_endpoint)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(!accepted, "rejected endpoint {invalid_endpoint}");
        let mut malformed = embedding_revision();
        malformed.endpoint_identity = invalid_endpoint.into();
        let payload = serde_json::to_vec(&malformed).unwrap();
        assert!(
            insert_embedding_revision_raw(&pool, &malformed, payload, "test:malformed-endpoint")
                .await
                .is_err()
        );
    }
    let mut uppercase_digest = embedding_revision();
    uppercase_digest.provider_model_revision_sha256 = "A".repeat(64);
    let uppercase_payload = serde_json::to_vec(&uppercase_digest).unwrap();
    assert!(
        insert_embedding_revision_raw(
            &pool,
            &uppercase_digest,
            uppercase_payload,
            "test:uppercase-digest",
        )
        .await
        .is_err()
    );

    let canonical_revision = embedding_revision();
    let pretty_payload = serde_json::to_vec_pretty(&canonical_revision).unwrap();
    let pretty_error = insert_embedding_revision_raw(
        &pool,
        &canonical_revision,
        pretty_payload,
        "test:pretty-payload",
    )
    .await
    .unwrap_err();
    assert_eq!(
        pretty_error
            .as_database_error()
            .and_then(|error| error.constraint()),
        Some("embedding_revisions_v2_payload_matches_columns")
    );
    let value = serde_json::to_value(&canonical_revision).unwrap();
    let mut reordered = serde_json::Map::new();
    for key in [
        "endpoint_identity",
        "schema_version",
        "provider_protocol_version",
        "provider_model_identifier",
        "provider_model_revision_sha256",
        "endpoint_config_sha256",
        "dimension",
        "request_config_sha256",
        "output_normalization_version",
    ] {
        reordered.insert(key.into(), value[key].clone());
    }
    let reordered_payload = serde_json::to_vec(&serde_json::Value::Object(reordered)).unwrap();
    assert_ne!(
        reordered_payload,
        canonical_revision.canonical_bytes().unwrap()
    );
    let reordered_error = insert_embedding_revision_raw(
        &pool,
        &canonical_revision,
        reordered_payload,
        "test:reordered-payload",
    )
    .await
    .unwrap_err();
    assert_eq!(
        reordered_error
            .as_database_error()
            .and_then(|error| error.constraint()),
        Some("embedding_revisions_v2_payload_matches_columns")
    );

    let mismatch_revision =
        embedding_revision_for(format!("mismatch-{}@2025-01-15", Uuid::new_v4()));
    let mismatch_error = sqlx::query(
        "INSERT INTO embedding_revisions_v2(
             revision_sha256,canonical_revision_payload,schema_version,
             provider_protocol_version,provider_model_identifier,
             provider_model_revision_sha256,endpoint_config_sha256,endpoint_identity,
             dimension,request_config_sha256,output_normalization_version,credential_ref)
         VALUES($1,$2,2,$3,$4,$5,$6,'https://different.example.test/v1/embeddings',1024,$7,$8,'test:mismatch')",
    )
    .bind(mismatch_revision.sha256().unwrap())
    .bind(mismatch_revision.canonical_bytes().unwrap())
    .bind(&mismatch_revision.provider_protocol_version)
    .bind(&mismatch_revision.provider_model_identifier)
    .bind(&mismatch_revision.provider_model_revision_sha256)
    .bind(&mismatch_revision.endpoint_config_sha256)
    .bind(&mismatch_revision.request_config_sha256)
    .bind(&mismatch_revision.output_normalization_version)
    .execute(&pool)
    .await
    .unwrap_err();
    assert_eq!(
        mismatch_error
            .as_database_error()
            .and_then(|error| error.constraint()),
        Some("embedding_revisions_v2_payload_matches_columns")
    );

    let revision = embedding_revision_for(format!("registry-{}@2025-01-15", Uuid::new_v4()));
    let revision_sha256 = revision.sha256().unwrap();
    sqlx::query(
        "INSERT INTO embedding_revisions_v2(
             revision_sha256,canonical_revision_payload,schema_version,
             provider_protocol_version,provider_model_identifier,
             provider_model_revision_sha256,endpoint_config_sha256,endpoint_identity,
             dimension,request_config_sha256,output_normalization_version,credential_ref)
         VALUES($1,$2,2,$3,$4,$5,$6,$7,1024,$8,$9,'test:registry')",
    )
    .bind(&revision_sha256)
    .bind(revision.canonical_bytes().unwrap())
    .bind(&revision.provider_protocol_version)
    .bind(&revision.provider_model_identifier)
    .bind(&revision.provider_model_revision_sha256)
    .bind(&revision.endpoint_config_sha256)
    .bind(&revision.endpoint_identity)
    .bind(&revision.request_config_sha256)
    .bind(&revision.output_normalization_version)
    .execute(&pool)
    .await
    .unwrap();

    for statement in [
        "UPDATE embedding_revisions_v2 SET credential_ref='test:mutated' WHERE revision_sha256=$1",
        "UPDATE embedding_revisions_v2 SET updated_at=updated_at+interval '1 second' WHERE revision_sha256=$1",
        "DELETE FROM embedding_revisions_v2 WHERE revision_sha256=$1",
    ] {
        let error = sqlx::query(statement)
            .bind(&revision_sha256)
            .execute(&pool)
            .await
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("EMBEDDING_REVISION_V2_IMMUTABLE")
        );
    }
    assert!(
        sqlx::query("TRUNCATE embedding_revisions_v2")
            .execute(&pool)
            .await
            .is_err()
    );

    let mut unsupported_keyword_score = policy_artifact(2, 1024, 2048);
    unsupported_keyword_score.embedding.model_revision_sha256 = revision_sha256.clone();
    unsupported_keyword_score.keyword.score_version = "latest".into();
    let mut unsupported_similarity = policy_artifact(2, 1024, 2048);
    unsupported_similarity.embedding.model_revision_sha256 = revision_sha256.clone();
    unsupported_similarity.embedding.similarity_version = "latest".into();
    for unsupported_policy in [unsupported_keyword_score, unsupported_similarity] {
        let payload = serde_json::to_vec(&unsupported_policy).unwrap();
        let error = sqlx::query(
            "INSERT INTO knowledge_retrieval_policies_v2(
                 policy_sha256,canonical_policy_payload,embedding_revision_sha256,contract_version,
                 max_hits,max_chunk_bytes,max_total_bytes)
             VALUES($1,$2,$3,$4,2,1024,2048)",
        )
        .bind(knowledge::sha256_hex(&payload))
        .bind(payload)
        .bind(&revision_sha256)
        .bind(KNOWLEDGE_EVIDENCE_CONTRACT_V2)
        .execute(&pool)
        .await
        .unwrap_err();
        assert_eq!(
            error
                .as_database_error()
                .and_then(|database_error| database_error.constraint()),
            Some("knowledge_retrieval_policies_v2_payload_matches_columns")
        );
    }

    let mut artifact = policy_artifact(2, 1024, 2048);
    artifact.embedding.model_revision_sha256 = revision_sha256.clone();
    let identity = artifact.request_identity().unwrap();
    sqlx::query(
        "INSERT INTO knowledge_retrieval_policies_v2(
             policy_sha256,canonical_policy_payload,embedding_revision_sha256,contract_version,
             max_hits,max_chunk_bytes,max_total_bytes)
         VALUES($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(&identity.policy_sha256)
    .bind(artifact.canonical_bytes().unwrap())
    .bind(&revision_sha256)
    .bind(&identity.contract_version)
    .bind(i64::from(identity.max_hits))
    .bind(i64::from(identity.max_chunk_bytes))
    .bind(i64::try_from(identity.max_total_bytes).unwrap())
    .execute(&pool)
    .await
    .unwrap();

    let mut fixture = new_fixture(&pool).await;
    let version = add_version(&pool, &mut fixture, &[("text", "registry needle")]).await;
    let mut binding_transaction = pool.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO product_version_embedding_bindings_v2(
             product_version_id,embedding_revision_sha256) VALUES($1,$2)",
    )
    .bind(version)
    .bind(&revision_sha256)
    .execute(&mut *binding_transaction)
    .await
    .unwrap();
    assert!(
        sqlx::query(
            "UPDATE product_version_embedding_bindings_v2
                SET embedding_revision_sha256=$2 WHERE product_version_id=$1",
        )
        .bind(version)
        .bind(embedding_revision().sha256().unwrap())
        .execute(&mut *binding_transaction)
        .await
        .unwrap_err()
        .to_string()
        .contains("PRODUCT_VERSION_EMBEDDING_BINDING_V2_IMMUTABLE")
    );
    binding_transaction.rollback().await.unwrap();

    let adapter = PostgresKnowledgeRetrievalAdapter::new_exact_only_v2_contract_tests(pool.clone());
    assert!(
        adapter
            .retrieve_evidence_v2(scope("needle", vec![version], identity.clone()))
            .await
            .is_ok()
    );
    sqlx::query(
        "UPDATE embedding_revisions_v2 SET support_state='revoked' WHERE revision_sha256=$1",
    )
    .bind(&revision_sha256)
    .execute(&pool)
    .await
    .unwrap();
    assert!(
        sqlx::query(
            "UPDATE embedding_revisions_v2 SET support_state='supported' WHERE revision_sha256=$1",
        )
        .bind(&revision_sha256)
        .execute(&pool)
        .await
        .unwrap_err()
        .to_string()
        .contains("EMBEDDING_REVISION_V2_IMMUTABLE")
    );
    let revoked = adapter
        .retrieve_evidence_v2(scope("needle", vec![version], identity))
        .await;
    assert!(matches!(
        revoked,
        Err(KnowledgeRetrievalError::InvalidRequest(_))
    ));
    let mismatched_version =
        add_version(&pool, &mut fixture, &[("text", "binding mismatch")]).await;
    assert!(
        sqlx::query(
            "INSERT INTO product_version_embedding_bindings_v2(
                 product_version_id,embedding_revision_sha256) VALUES($1,$2)",
        )
        .bind(mismatched_version)
        .bind(&revision_sha256)
        .execute(&pool)
        .await
        .unwrap_err()
        .to_string()
        .contains("EMBEDDING_REVISION_V2_NOT_SUPPORTED")
    );

    let unknown_sha = knowledge::sha256_hex(Uuid::new_v4().as_bytes());
    let mut unknown_artifact = policy_artifact(1, 1024, 1024);
    unknown_artifact.embedding.model_revision_sha256 = unknown_sha.clone();
    let unknown_identity = unknown_artifact.request_identity().unwrap();
    assert!(
        sqlx::query(
            "INSERT INTO knowledge_retrieval_policies_v2(
                 policy_sha256,canonical_policy_payload,embedding_revision_sha256,contract_version,
                 max_hits,max_chunk_bytes,max_total_bytes)
             VALUES($1,$2,$3,$4,1,1024,1024)",
        )
        .bind(&unknown_identity.policy_sha256)
        .bind(unknown_artifact.canonical_bytes().unwrap())
        .bind(&unknown_sha)
        .bind(KNOWLEDGE_EVIDENCE_CONTRACT_V2)
        .execute(&pool)
        .await
        .unwrap_err()
        .to_string()
        .contains("EMBEDDING_REVISION_V2_NOT_SUPPORTED")
    );
    let mut revoked_artifact = policy_artifact(1, 1024, 1024);
    revoked_artifact.embedding.model_revision_sha256 = revision_sha256.clone();
    let revoked_identity = revoked_artifact.request_identity().unwrap();
    assert!(
        sqlx::query(
            "INSERT INTO knowledge_retrieval_policies_v2(
                 policy_sha256,canonical_policy_payload,embedding_revision_sha256,contract_version,
                 max_hits,max_chunk_bytes,max_total_bytes)
             VALUES($1,$2,$3,$4,1,1024,1024)",
        )
        .bind(&revoked_identity.policy_sha256)
        .bind(revoked_artifact.canonical_bytes().unwrap())
        .bind(&revision_sha256)
        .bind(KNOWLEDGE_EVIDENCE_CONTRACT_V2)
        .execute(&pool)
        .await
        .unwrap_err()
        .to_string()
        .contains("EMBEDDING_REVISION_V2_NOT_SUPPORTED")
    );

    let sidecar_counts_after: Vec<i64> = sqlx::query_scalar(
        "SELECT count(*) FROM chunk_keyword_indexes_v2
         UNION ALL SELECT count(*) FROM chunk_vector_indexes_v2",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(sidecar_counts_after, vec![0, 0]);
    let legacy_rows_after: i64 = sqlx::query_scalar("SELECT count(*) FROM chunk_embeddings")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(legacy_rows_after, legacy_rows_before);
    let binding_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM product_version_embedding_bindings_v2
          WHERE product_version_id=ANY($1)",
    )
    .bind(
        fixture
            .versions
            .iter()
            .map(|fixture_version| fixture_version.version_id)
            .collect::<Vec<_>>(),
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(binding_count, 0);
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
    let adapter = PostgresKnowledgeRetrievalAdapter::new_exact_only_v2_contract_tests(pool.clone());
    let policy = register_policy(&pool, 3, 100, 100).await;

    let missing = adapter
        .retrieve_evidence_v2(scope("needle", vec![Uuid::new_v4()], policy.clone()))
        .await;
    assert!(matches!(
        missing,
        Err(KnowledgeRetrievalError::InvalidRequest(_))
    ));

    let mut unknown = policy.clone();
    unknown.policy_sha256 = knowledge::sha256_hex(Uuid::new_v4().as_bytes());
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
