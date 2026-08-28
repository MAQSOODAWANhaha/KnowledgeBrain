use knowledge::knowledge_retrieval::{
    EMBEDDING_DIMENSION_V2, EMBEDDING_OUTPUT_NORMALIZATION_VERSION_V2,
    EMBEDDING_PROVIDER_PROTOCOL_VERSION_V2, EMBEDDING_REVISION_SCHEMA_V2, EmbeddingRevisionV2,
    KNOWLEDGE_EVIDENCE_CONTRACT_V2, RERANK_REQUEST_CONFIG_SHA256_V2, RERANK_REVISION_SCHEMA_V2,
    RETRIEVAL_A_PRIMARY_COMPARATOR_V2, RETRIEVAL_A_VERSION_COMPARATOR_V2,
    RETRIEVAL_B_EXACT_COMPARATOR_V2, RETRIEVAL_C_SEMANTIC_COMPARATOR_V2,
    RETRIEVAL_CHANNEL_RANK_COMPARATOR_V2, RETRIEVAL_CHANNEL_SCORE_QUANTIZATION_VERSION_V2,
    RETRIEVAL_EMBEDDING_POLICY_V2, RETRIEVAL_EMBEDDING_POLICY_VERSION_V2,
    RETRIEVAL_EMBEDDING_SIMILARITY_VERSION_V2, RETRIEVAL_KEYWORD_SCORE_VERSION_V2,
    RETRIEVAL_KEYWORD_TOKENIZER_V2, RETRIEVAL_KEYWORD_TOKENIZER_VERSION_V2,
    RETRIEVAL_NORMALIZATION_VERSION_V2, RETRIEVAL_POLICY_SCHEMA_V2,
    RETRIEVAL_PRE_RERANK_RRF_COMPARATOR_V2, RETRIEVAL_QUOTA_SEMANTICS_VERSION_V2,
    RETRIEVAL_RERANK_PROTOCOL_VERSION_V2, RETRIEVAL_RERANK_SCORE_NORMALIZATION_VERSION_V2,
    RETRIEVAL_RRF_SCORE_REPRESENTATION_VERSION_V2, RETRIEVAL_SOURCE_FOLDING_VERSION_V2,
    RETRIEVAL_TRUSTED_SOURCE_TYPES_V2, RerankRevisionV2, RetrievalEmbeddingPolicyV2,
    RetrievalKeywordPolicyV2, RetrievalPolicyV2, RetrievalRankingPolicyV2,
    RetrievalRequestQuotasV2, RetrievalRerankPolicyV2, RetrievalRrfPolicyV2,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio::time::{Duration, sleep, timeout};
use uuid::Uuid;

mod support;

const CONTRACT: &str = KNOWLEDGE_EVIDENCE_CONTRACT_V2;
const DOCUMENT_NAME: &str = "v2-attestation.txt";

#[derive(Clone)]
struct ChunkFixture {
    id: Uuid,
    source_type: &'static str,
    content: &'static str,
}

struct Fixture {
    workspace_id: Uuid,
    product_id: Uuid,
    version_id: Uuid,
    product_artifact_id: Uuid,
    document_id: Uuid,
    file_hash: String,
    chunks: Vec<ChunkFixture>,
}

async fn final_schema(pool: &PgPool, label: &str) -> bool {
    let ready: bool = sqlx::query_scalar(
        "SELECT to_regprocedure('kb_knowledge_attest_matching_scope_v2(jsonb)') IS NOT NULL
             AND to_regprocedure('kb_knowledge_verify_matching_scope_v2(uuid,text,jsonb)') IS NOT NULL
             AND to_regclass('knowledge_matching_scope_attestations_v2') IS NOT NULL
             AND to_regclass('knowledge_retrieval_policies_v2') IS NOT NULL",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(false);
    support::require_final_schema(label, ready)
}

async fn seed_fixture(pool: &PgPool) -> Fixture {
    let workspace_id = Uuid::new_v4();
    let product_id = Uuid::new_v4();
    let version_id = Uuid::new_v4();
    let document_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO workspaces(id,name,slug,kind)
         VALUES($1,'v2 attestation fixture',$2,'product_line')",
    )
    .bind(workspace_id)
    .bind(format!("v2-attestation-{workspace_id}"))
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO products(id,workspace_id,kind,name,slug)
         VALUES($1,$2,'product','v2 attestation product',$3)",
    )
    .bind(product_id)
    .bind(workspace_id)
    .bind(format!("v2-attestation-{product_id}"))
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO product_versions(id,product_id,label,status)
         VALUES($1,$2,'v2-attestation','active')",
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
             id,product_version_id,type,title,parse_status,enable_status,index_ready,
             file_name,file_size,file_hash,object_ref)
         VALUES($1,$2,'file','v2 attestation','completed','enabled',true,$3,1,$4,$5)",
    )
    .bind(document_id)
    .bind(version_id)
    .bind(DOCUMENT_NAME)
    .bind(&file_hash)
    .bind(&object_ref)
    .execute(pool)
    .await
    .unwrap();

    let chunks = vec![
        ChunkFixture {
            id: Uuid::new_v4(),
            source_type: "text",
            content: "trusted alpha",
        },
        ChunkFixture {
            id: Uuid::new_v4(),
            source_type: "parent_text",
            content: "trusted beta",
        },
        ChunkFixture {
            id: Uuid::new_v4(),
            source_type: "text",
            content: "trusted gamma",
        },
        ChunkFixture {
            id: Uuid::new_v4(),
            source_type: "image_caption",
            content: "caption signal",
        },
    ];
    for chunk in &chunks {
        sqlx::query(
            "INSERT INTO chunks(id,product_version_id,document_id,chunk_type,content)
             VALUES($1,$2,$3,$4,$5)",
        )
        .bind(chunk.id)
        .bind(version_id)
        .bind(document_id)
        .bind(chunk.source_type)
        .bind(chunk.content)
        .execute(pool)
        .await
        .unwrap();
    }

    Fixture {
        workspace_id,
        product_id,
        version_id,
        product_artifact_id: Uuid::new_v4(),
        document_id,
        file_hash,
        chunks,
    }
}

async fn remove_fixture(pool: &PgPool, fixture: &Fixture) {
    sqlx::query(
        "DELETE FROM knowledge_matching_scope_attestations_v2
          WHERE convert_from(canonical_payload,'UTF8') LIKE $1",
    )
    .bind(format!("%{}%", fixture.version_id))
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM chunks WHERE document_id=$1")
        .bind(fixture.document_id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "DELETE FROM object_owner_references
          WHERE owner_kind='knowledge_document' AND owner_id=$1",
    )
    .bind(fixture.document_id)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("DELETE FROM documents WHERE id=$1")
        .bind(fixture.document_id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM object_registry WHERE digest=$1")
        .bind(&fixture.file_hash)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("UPDATE products SET current_version_id=NULL WHERE id=$1")
        .bind(fixture.product_id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM product_versions WHERE id=$1")
        .bind(fixture.version_id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM products WHERE id=$1")
        .bind(fixture.product_id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM workspaces WHERE id=$1")
        .bind(fixture.workspace_id)
        .execute(pool)
        .await
        .unwrap();
}

fn embedding_revision_for(provider_model_identifier: impl Into<String>) -> EmbeddingRevisionV2 {
    EmbeddingRevisionV2 {
        schema_version: EMBEDDING_REVISION_SCHEMA_V2,
        provider_protocol_version: EMBEDDING_PROVIDER_PROTOCOL_VERSION_V2.into(),
        provider_model_identifier: provider_model_identifier.into(),
        provider_model_revision_sha256: knowledge::sha256_hex(
            b"attestation-v2 fixture immutable provider revision metadata",
        ),
        endpoint_config_sha256: knowledge::sha256_hex(
            b"attestation-v2 fixture immutable endpoint preprocessing config",
        ),
        endpoint_identity: "https://embeddings.example.test/v1/embeddings".into(),
        dimension: EMBEDDING_DIMENSION_V2,
        request_config_sha256: EmbeddingRevisionV2::canonical_request_config_sha256(),
        output_normalization_version: EMBEDDING_OUTPUT_NORMALIZATION_VERSION_V2.into(),
    }
}

fn embedding_revision() -> EmbeddingRevisionV2 {
    embedding_revision_for("attestation-v2-fixture@2025-01-15")
}

fn rerank_revision() -> RerankRevisionV2 {
    RerankRevisionV2 {
        schema_version: RERANK_REVISION_SCHEMA_V2,
        provider_protocol_version: RETRIEVAL_RERANK_PROTOCOL_VERSION_V2.into(),
        provider_model_identifier: "attestation-v2-reranker@2025-01-15".into(),
        provider_model_revision_sha256: knowledge::sha256_hex(b"attestation-v2 reranker model"),
        config_revision_sha256: knowledge::sha256_hex(b"attestation-v2 reranker config"),
        endpoint_identity: "https://rerank.example.test/v1/rerank".into(),
        request_config_sha256: RERANK_REQUEST_CONFIG_SHA256_V2.into(),
        score_normalization_version: RETRIEVAL_RERANK_SCORE_NORMALIZATION_VERSION_V2.into(),
    }
}

async fn register_canonical_rerank_revision(pool: &PgPool) {
    let reranker = rerank_revision();
    sqlx::query(
        "INSERT INTO rerank_revisions_v2(
             revision_sha256,canonical_revision_payload,schema_version,
             provider_protocol_version,provider_model_identifier,
             provider_model_revision_sha256,config_revision_sha256,endpoint_identity,
             request_config_sha256,score_normalization_version,credential_ref)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,'test:rerank-attestation-v2-canonical')
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
        contract_version: CONTRACT.into(),
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
    max_hits: usize,
    max_chunk_bytes: usize,
    max_total_bytes: usize,
) -> Value {
    let mut artifact = policy_artifact(
        max_hits.try_into().unwrap(),
        max_chunk_bytes.try_into().unwrap(),
        max_total_bytes.try_into().unwrap(),
    );
    let revision = embedding_revision();
    let revision_bytes = revision.canonical_bytes().unwrap();
    let revision_sha256 = revision.sha256().unwrap();
    sqlx::query(
        "INSERT INTO embedding_revisions_v2(
             revision_sha256,canonical_revision_payload,schema_version,
             provider_protocol_version,provider_model_identifier,
             provider_model_revision_sha256,endpoint_config_sha256,endpoint_identity,
             dimension,request_config_sha256,output_normalization_version,credential_ref)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,'test:embedding-attestation-v2')
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
           AND credential_ref='test:embedding-attestation-v2'
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

    let mut reranker = rerank_revision();
    let unique = Uuid::new_v4();
    reranker.provider_model_identifier = format!(
        "attestation-v2-reranker@sha256:{}",
        knowledge::sha256_hex(unique.as_bytes())
    );
    reranker.provider_model_revision_sha256 =
        knowledge::sha256_hex(format!("model-{unique}").as_bytes());
    reranker.config_revision_sha256 = knowledge::sha256_hex(format!("config-{unique}").as_bytes());
    artifact.rerank.revision_sha256 = reranker.sha256().unwrap();
    artifact.rerank.model_revision_sha256 = reranker.provider_model_revision_sha256.clone();
    artifact.rerank.config_revision_sha256 = reranker.config_revision_sha256.clone();
    let reranker_bytes = reranker.canonical_bytes().unwrap();
    let reranker_sha256 = reranker.sha256().unwrap();
    sqlx::query(
        "INSERT INTO rerank_revisions_v2(
             revision_sha256,canonical_revision_payload,schema_version,
             provider_protocol_version,provider_model_identifier,
             provider_model_revision_sha256,config_revision_sha256,endpoint_identity,
             request_config_sha256,score_normalization_version,credential_ref)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,'test:rerank-attestation-v2')
         ON CONFLICT (revision_sha256) DO NOTHING",
    )
    .bind(&reranker_sha256)
    .bind(&reranker_bytes)
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

    artifact.validate().unwrap();
    let canonical_bytes = artifact.canonical_bytes().unwrap();
    let digest = artifact.sha256().unwrap();
    let identity = artifact.request_identity().unwrap();
    assert_eq!(identity.policy_sha256, digest);
    sqlx::query(
        "INSERT INTO knowledge_retrieval_policies_v2(
             policy_sha256,canonical_policy_payload,embedding_revision_sha256,contract_version,
             max_hits,max_chunk_bytes,max_total_bytes)
         VALUES($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(&identity.policy_sha256)
    .bind(canonical_bytes)
    .bind(&revision_sha256)
    .bind(&identity.contract_version)
    .bind(i64::from(identity.max_hits))
    .bind(i64::from(identity.max_chunk_bytes))
    .bind(i64::try_from(identity.max_total_bytes).unwrap())
    .execute(pool)
    .await
    .unwrap();
    serde_json::to_value(identity).unwrap()
}

fn product_artifact(fixture: &Fixture) -> Value {
    json!({
        "id": fixture.product_artifact_id,
        "product_id": fixture.product_id,
        "product_version_id": fixture.version_id,
        "workspace_kind": "product_line",
        "frozen_display_name": fixture.version_id.to_string(),
        "identity_sha256": knowledge::sha256_hex(
            format!(
                "ProductVersionEvidenceV1:{}:{}:product_line",
                fixture.product_id, fixture.version_id
            )
            .as_bytes()
        )
    })
}

fn frozen_hit(
    fixture: &Fixture,
    chunk: &ChunkFixture,
    requirement_artifact_id: Uuid,
    rank: usize,
) -> Value {
    let byte_length = chunk.content.len();
    json!({
        "id": Uuid::new_v4(),
        "route_id": requirement_artifact_id,
        "requirement_artifact_id": requirement_artifact_id,
        "product_version_artifact_id": fixture.product_artifact_id,
        "document_id": fixture.document_id,
        "source_chunk_id": chunk.id,
        "frozen_document_display_name": DOCUMENT_NAME,
        "chunk_utf8": chunk.content,
        "chunk_sha256": knowledge::sha256_hex(chunk.content.as_bytes()),
        "chunk_byte_length": byte_length,
        "source_type": chunk.source_type,
        "media": null,
        "retrieval_rank": rank,
        "retrieval_raw_score": "1.000000",
        "pre_rerank_rrf_rank": null,
        "quote_start_offset": 0,
        "quote_end_offset": byte_length,
        "offset_unit": "utf8_byte",
        "retrieval_contract_version": CONTRACT
    })
}

fn scope(fixture: &Fixture, retrieval_policy: Value, hits: Vec<Value>) -> Value {
    let mut requirements = std::collections::BTreeMap::new();
    for hit in &hits {
        let route_id = hit["route_id"].as_str().unwrap();
        let requirement_id = hit["requirement_artifact_id"].as_str().unwrap();
        requirements
            .entry((route_id.to_owned(), requirement_id.to_owned()))
            .and_modify(|current: &mut u64| *current += 1)
            .or_insert(1);
    }
    let retrieval_requirements = requirements
        .into_iter()
        .map(
            |((route_id, requirement_artifact_id), exact_prefix_hit_count)| {
                json!({
                    "route_id": route_id,
                    "requirement_artifact_id": requirement_artifact_id,
                    "requirement_identity_sha256": knowledge::sha256_hex(b"trusted"),
                    "requirement_text": "trusted",
                    "exact_prefix_hit_count": exact_prefix_hit_count
                })
            },
        )
        .collect::<Vec<_>>();
    json!({
        "schema_version": 2,
        "workspace_kinds": ["product_line"],
        "version_selections": {"product_line": [fixture.version_id], "company": []},
        "products": [product_artifact(fixture)],
        "retrieval_requirements": retrieval_requirements,
        "frozen_hits": hits,
        "retrieval_policy": retrieval_policy
    })
}

async fn attest(pool: &PgPool, value: &Value) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_knowledge_attest_matching_scope_v2($1)")
        .bind(value)
        .fetch_one(pool)
        .await
}

fn assert_contract_error(error: sqlx::Error, expected: &str) {
    let database_error = error
        .as_database_error()
        .unwrap_or_else(|| panic!("expected database error, got {error}"));
    assert_eq!(database_error.code().as_deref(), Some("23514"));
    assert!(
        database_error.message().contains(expected),
        "expected {expected}, got {}",
        database_error.message()
    );
}

fn assert_check_constraint(error: sqlx::Error, expected: &str) {
    let database_error = error
        .as_database_error()
        .unwrap_or_else(|| panic!("expected database error, got {error}"));
    assert_eq!(database_error.code().as_deref(), Some("23514"));
    assert_eq!(database_error.constraint(), Some(expected));
}

#[tokio::test]
async fn attest_verify_positive_trusted_types_and_cross_version_replay() {
    let Some(pool) =
        support::connect_postgres_contract("KnowledgeRetrievalAttestationV2Replay").await
    else {
        return;
    };
    if !final_schema(&pool, "KnowledgeRetrievalAttestationV2Replay").await {
        return;
    }
    let fixture = seed_fixture(&pool).await;
    let policy = register_policy(&pool, 3, 1024, 3072).await;
    let requirement_id = Uuid::new_v4();
    let value = scope(
        &fixture,
        policy,
        fixture.chunks[..3]
            .iter()
            .enumerate()
            .map(|(index, chunk)| frozen_hit(&fixture, chunk, requirement_id, index + 1))
            .collect(),
    );
    let attestation = attest(&pool, &value).await.unwrap();
    let id = attestation["id"].as_str().unwrap().parse::<Uuid>().unwrap();
    let digest = attestation["content_sha256"].as_str().unwrap();
    sqlx::query("SELECT kb_knowledge_verify_matching_scope_v2($1,$2,$3)")
        .bind(id)
        .bind(digest)
        .bind(&value)
        .execute(&pool)
        .await
        .unwrap();
    let stored_version: i16 = sqlx::query_scalar(
        "SELECT schema_version FROM knowledge_matching_scope_attestations_v2 WHERE id=$1",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stored_version, 2);

    assert_contract_error(
        sqlx::query("SELECT kb_knowledge_verify_matching_scope_v1($1,$2,$3)")
            .bind(id)
            .bind(digest)
            .bind(&value)
            .execute(&pool)
            .await
            .unwrap_err(),
        "KNOWLEDGE_MATCHING_ATTESTATION_V1_MISMATCH",
    );

    let v1_id = Uuid::new_v4();
    let v1_scope = json!({"fixture_version": fixture.version_id});
    let v1_digest: String = sqlx::query_scalar(
        "INSERT INTO knowledge_matching_scope_attestations(
             id,schema_version,canonical_payload,content_sha256)
         SELECT $1,1,convert_to($2::jsonb::text,'UTF8'),encode(digest(convert_to($2::jsonb::text,'UTF8'),'sha256'),'hex')
         RETURNING content_sha256",
    )
    .bind(v1_id)
    .bind(&v1_scope)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_contract_error(
        sqlx::query("SELECT kb_knowledge_verify_matching_scope_v2($1,$2,$3)")
            .bind(v1_id)
            .bind(&v1_digest)
            .bind(&v1_scope)
            .execute(&pool)
            .await
            .unwrap_err(),
        "KNOWLEDGE_MATCHING_ATTESTATION_V2_MISMATCH",
    );
    sqlx::query("DELETE FROM knowledge_matching_scope_attestations WHERE id=$1")
        .bind(v1_id)
        .execute(&pool)
        .await
        .unwrap();

    assert_contract_error(
        sqlx::query("SELECT kb_knowledge_verify_matching_scope_v2($1,$2,$3)")
            .bind(id)
            .bind("0".repeat(64))
            .bind(&value)
            .execute(&pool)
            .await
            .unwrap_err(),
        "KNOWLEDGE_MATCHING_ATTESTATION_V2_MISMATCH",
    );
    let mut payload_tamper = value.clone();
    payload_tamper["frozen_hits"][0]["route_id"] = json!(Uuid::new_v4());
    assert_contract_error(
        sqlx::query("SELECT kb_knowledge_verify_matching_scope_v2($1,$2,$3)")
            .bind(id)
            .bind(digest)
            .bind(payload_tamper)
            .execute(&pool)
            .await
            .unwrap_err(),
        "KNOWLEDGE_MATCHING_ATTESTATION_V2_MISMATCH",
    );
    remove_fixture(&pool, &fixture).await;
}

#[tokio::test]
async fn embedding_revision_revocation_invalidates_v2_attestation() {
    let Some(pool) =
        support::connect_postgres_contract("KnowledgeRetrievalAttestationV2EmbeddingRevocation")
            .await
    else {
        return;
    };
    if !final_schema(&pool, "KnowledgeRetrievalAttestationV2EmbeddingRevocation").await {
        return;
    }
    let fixture = seed_fixture(&pool).await;
    register_canonical_rerank_revision(&pool).await;
    let revision = embedding_revision_for(format!("attest-revoke-{}@2025-01-15", Uuid::new_v4()));
    let revision_sha256 = revision.sha256().unwrap();
    sqlx::query(
        "INSERT INTO embedding_revisions_v2(
             revision_sha256,canonical_revision_payload,schema_version,
             provider_protocol_version,provider_model_identifier,
             provider_model_revision_sha256,endpoint_config_sha256,endpoint_identity,
             dimension,request_config_sha256,output_normalization_version,credential_ref)
         VALUES($1,$2,2,$3,$4,$5,$6,$7,1024,$8,$9,'test:attestation-revocation')",
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
    let mut artifact = policy_artifact(1, 1024, 1024);
    artifact.embedding.model_revision_sha256 = revision_sha256.clone();
    let identity = artifact.request_identity().unwrap();
    sqlx::query(
        "INSERT INTO knowledge_retrieval_policies_v2(
             policy_sha256,canonical_policy_payload,embedding_revision_sha256,contract_version,
             max_hits,max_chunk_bytes,max_total_bytes)
         VALUES($1,$2,$3,$4,1,1024,1024)",
    )
    .bind(&identity.policy_sha256)
    .bind(artifact.canonical_bytes().unwrap())
    .bind(&revision_sha256)
    .bind(CONTRACT)
    .execute(&pool)
    .await
    .unwrap();
    let policy = serde_json::to_value(identity).unwrap();
    let valid = scope(&fixture, policy, vec![]);
    attest(&pool, &valid).await.unwrap();

    sqlx::query(
        "UPDATE embedding_revisions_v2 SET support_state='revoked' WHERE revision_sha256=$1",
    )
    .bind(&revision_sha256)
    .execute(&pool)
    .await
    .unwrap();
    assert_contract_error(
        attest(&pool, &valid).await.unwrap_err(),
        "KNOWLEDGE_MATCHING_RETRIEVAL_POLICY_V2_INVALID",
    );
    let policy_support_state: String = sqlx::query_scalar(
        "SELECT support_state FROM knowledge_retrieval_policies_v2 WHERE policy_sha256=$1",
    )
    .bind(valid["retrieval_policy"]["policy_sha256"].as_str().unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(policy_support_state, "supported");
    remove_fixture(&pool, &fixture).await;
}

#[tokio::test]
async fn embedding_revision_revoke_serializes_with_policy_insert() {
    let Some(pool) = support::connect_postgres_contract(
        "KnowledgeRetrievalAttestationV2EmbeddingInsertRevocationLock",
    )
    .await
    else {
        return;
    };
    if !final_schema(
        &pool,
        "KnowledgeRetrievalAttestationV2EmbeddingInsertRevocationLock",
    )
    .await
    {
        return;
    }
    register_canonical_rerank_revision(&pool).await;
    let revision = embedding_revision_for(format!("insert-lock-{}@2025-01-15", Uuid::new_v4()));
    let revision_sha256 = revision.sha256().unwrap();
    sqlx::query(
        "INSERT INTO embedding_revisions_v2(
             revision_sha256,canonical_revision_payload,schema_version,
             provider_protocol_version,provider_model_identifier,
             provider_model_revision_sha256,endpoint_config_sha256,endpoint_identity,
             dimension,request_config_sha256,output_normalization_version,credential_ref)
         VALUES($1,$2,2,$3,$4,$5,$6,$7,1024,$8,$9,'test:insert-lock')",
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
    let mut artifact = policy_artifact(1, 1024, 1024);
    artifact.embedding.model_revision_sha256 = revision_sha256.clone();
    let identity = artifact.request_identity().unwrap();

    let mut insert_transaction = pool.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO knowledge_retrieval_policies_v2(
             policy_sha256,canonical_policy_payload,embedding_revision_sha256,contract_version,
             max_hits,max_chunk_bytes,max_total_bytes)
         VALUES($1,$2,$3,$4,1,1024,1024)",
    )
    .bind(&identity.policy_sha256)
    .bind(artifact.canonical_bytes().unwrap())
    .bind(&revision_sha256)
    .bind(CONTRACT)
    .execute(&mut *insert_transaction)
    .await
    .unwrap();

    let mut revoke_connection = pool.acquire().await.unwrap();
    let revoke_backend_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *revoke_connection)
        .await
        .unwrap();
    let revoke_sha = revision_sha256.clone();
    let revoke_task = tokio::spawn(async move {
        sqlx::query(
            "UPDATE embedding_revisions_v2 SET support_state='revoked' WHERE revision_sha256=$1",
        )
        .bind(revoke_sha)
        .execute(&mut *revoke_connection)
        .await
    });
    let mut observed_waiting_lock = false;
    for _ in 0..250 {
        observed_waiting_lock = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM pg_locks WHERE pid=$1 AND NOT granted)",
        )
        .bind(revoke_backend_pid)
        .fetch_one(&mut *insert_transaction)
        .await
        .unwrap();
        if observed_waiting_lock || revoke_task.is_finished() {
            break;
        }
        sleep(Duration::from_millis(20)).await;
    }
    assert!(observed_waiting_lock);
    assert!(!revoke_task.is_finished());
    insert_transaction.commit().await.unwrap();
    assert_eq!(
        timeout(Duration::from_secs(10), revoke_task)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .rows_affected(),
        1
    );
}

#[tokio::test]
async fn policy_registry_rejects_unknown_revoked_quota_mismatch_and_mutation() {
    let Some(pool) =
        support::connect_postgres_contract("KnowledgeRetrievalAttestationV2Policy").await
    else {
        return;
    };
    if !final_schema(&pool, "KnowledgeRetrievalAttestationV2Policy").await {
        return;
    }
    let fixture = seed_fixture(&pool).await;
    register_canonical_rerank_revision(&pool).await;
    let _ = register_policy(&pool, 1, 1, 1).await;

    let mismatched_artifact = policy_artifact(4, 1024, 4096);
    mismatched_artifact.validate().unwrap();
    let mismatched_canonical_bytes = mismatched_artifact.canonical_bytes().unwrap();
    let mismatched_digest = mismatched_artifact.sha256().unwrap();
    assert_check_constraint(
        sqlx::query(
            "INSERT INTO knowledge_retrieval_policies_v2(
                 policy_sha256,canonical_policy_payload,embedding_revision_sha256,contract_version,
                 max_hits,max_chunk_bytes,max_total_bytes)
             VALUES($1,$2,$3,$4,$5,$6,$7)",
        )
        .bind(mismatched_digest)
        .bind(mismatched_canonical_bytes)
        .bind(embedding_revision().sha256().unwrap())
        .bind(CONTRACT)
        .bind(5_i64)
        .bind(1024_i64)
        .bind(4096_i64)
        .execute(&pool)
        .await
        .unwrap_err(),
        "knowledge_retrieval_policies_v2_payload_matches_columns",
    );

    let mut invalid_artifact = policy_artifact(2, 1024, 2048);
    invalid_artifact.normalization_version = "unsupported-normalization-v999".into();
    let invalid_payload = serde_json::to_vec(&invalid_artifact).unwrap();
    let invalid_digest = knowledge::sha256_hex(&invalid_payload);
    assert_check_constraint(
        sqlx::query(
            "INSERT INTO knowledge_retrieval_policies_v2(
                 policy_sha256,canonical_policy_payload,embedding_revision_sha256,contract_version,
                 max_hits,max_chunk_bytes,max_total_bytes)
             VALUES($1,$2,$3,$4,2,1024,2048)",
        )
        .bind(&invalid_digest)
        .bind(invalid_payload)
        .bind(embedding_revision().sha256().unwrap())
        .bind(CONTRACT)
        .execute(&pool)
        .await
        .unwrap_err(),
        "knowledge_retrieval_policies_v2_payload_matches_columns",
    );
    let invalid_policy = json!({
        "contract_version": CONTRACT,
        "policy_sha256": invalid_digest,
        "max_hits": 2,
        "max_chunk_bytes": 1024,
        "max_total_bytes": 2048
    });
    assert_contract_error(
        attest(&pool, &scope(&fixture, invalid_policy, vec![]))
            .await
            .unwrap_err(),
        "KNOWLEDGE_MATCHING_RETRIEVAL_POLICY_V2_INVALID",
    );

    let policy = register_policy(&pool, 2, 1024, 2048).await;
    let valid = scope(&fixture, policy.clone(), vec![]);
    attest(&pool, &valid).await.unwrap();

    let mut unknown = valid.clone();
    unknown["retrieval_policy"]["policy_sha256"] = json!("f".repeat(64));
    assert_contract_error(
        attest(&pool, &unknown).await.unwrap_err(),
        "KNOWLEDGE_MATCHING_RETRIEVAL_POLICY_V2_INVALID",
    );
    let mut quota_mismatch = valid.clone();
    quota_mismatch["retrieval_policy"]["max_hits"] = json!(3);
    assert_contract_error(
        attest(&pool, &quota_mismatch).await.unwrap_err(),
        "KNOWLEDGE_MATCHING_RETRIEVAL_POLICY_V2_INVALID",
    );

    let digest = policy["policy_sha256"].as_str().unwrap();
    assert_contract_error(
        sqlx::query(
            "UPDATE knowledge_retrieval_policies_v2
                SET canonical_policy_payload=canonical_policy_payload || decode('00','hex')
              WHERE policy_sha256=$1",
        )
        .bind(digest)
        .execute(&pool)
        .await
        .unwrap_err(),
        "KNOWLEDGE_RETRIEVAL_POLICY_V2_IMMUTABLE",
    );
    sqlx::query(
        "UPDATE knowledge_retrieval_policies_v2 SET support_state='revoked' WHERE policy_sha256=$1",
    )
    .bind(digest)
    .execute(&pool)
    .await
    .unwrap();
    assert_contract_error(
        sqlx::query("DELETE FROM knowledge_retrieval_policies_v2 WHERE policy_sha256=$1")
            .bind(digest)
            .execute(&pool)
            .await
            .unwrap_err(),
        "KNOWLEDGE_RETRIEVAL_POLICY_V2_IMMUTABLE",
    );
    assert_contract_error(
        sqlx::query("TRUNCATE knowledge_retrieval_policies_v2")
            .execute(&pool)
            .await
            .unwrap_err(),
        "KNOWLEDGE_RETRIEVAL_POLICY_V2_IMMUTABLE",
    );
    assert_contract_error(
        attest(&pool, &valid).await.unwrap_err(),
        "KNOWLEDGE_MATCHING_RETRIEVAL_POLICY_V2_INVALID",
    );
    assert_contract_error(
        sqlx::query(
            "UPDATE knowledge_retrieval_policies_v2 SET support_state='supported' WHERE policy_sha256=$1",
        )
        .bind(digest)
        .execute(&pool)
        .await
        .unwrap_err(),
        "KNOWLEDGE_RETRIEVAL_POLICY_V2_IMMUTABLE",
    );
    remove_fixture(&pool, &fixture).await;
}

#[tokio::test]
async fn policy_revocation_waits_for_open_attestation_transaction() {
    let Some(pool) =
        support::connect_postgres_contract("KnowledgeRetrievalAttestationV2RevocationLock").await
    else {
        return;
    };
    if !final_schema(&pool, "KnowledgeRetrievalAttestationV2RevocationLock").await {
        return;
    }
    let fixture = seed_fixture(&pool).await;
    let policy = register_policy(&pool, 1, 1024, 1024).await;
    let digest = policy["policy_sha256"].as_str().unwrap().to_owned();
    let valid = scope(&fixture, policy, vec![]);

    let mut attest_transaction = pool.begin().await.unwrap();
    let _: Value = sqlx::query_scalar("SELECT kb_knowledge_attest_matching_scope_v2($1)")
        .bind(&valid)
        .fetch_one(&mut *attest_transaction)
        .await
        .unwrap();

    let mut revoke_connection = pool.acquire().await.unwrap();
    let revoke_backend_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *revoke_connection)
        .await
        .unwrap();
    let revoke_digest = digest.clone();
    let revoke_task = tokio::spawn(async move {
        sqlx::query(
            "UPDATE knowledge_retrieval_policies_v2
                SET support_state='revoked'
              WHERE policy_sha256=$1",
        )
        .bind(revoke_digest)
        .execute(&mut *revoke_connection)
        .await
    });

    let mut observed_waiting_lock = false;
    for _ in 0..250 {
        observed_waiting_lock = sqlx::query_scalar(
            "SELECT EXISTS(
                 SELECT 1 FROM pg_locks WHERE pid=$1 AND NOT granted
             )",
        )
        .bind(revoke_backend_pid)
        .fetch_one(&mut *attest_transaction)
        .await
        .unwrap();
        if observed_waiting_lock || revoke_task.is_finished() {
            break;
        }
        sleep(Duration::from_millis(20)).await;
    }
    assert!(
        observed_waiting_lock,
        "concurrent revocation did not wait on the attestation policy lock"
    );
    assert!(!revoke_task.is_finished());

    attest_transaction.commit().await.unwrap();
    let revoke_result = timeout(Duration::from_secs(10), revoke_task)
        .await
        .expect("revocation remained blocked after attestation commit")
        .expect("revocation task panicked")
        .unwrap();
    assert_eq!(revoke_result.rows_affected(), 1);
    assert_contract_error(
        attest(&pool, &valid).await.unwrap_err(),
        "KNOWLEDGE_MATCHING_RETRIEVAL_POLICY_V2_INVALID",
    );
    remove_fixture(&pool, &fixture).await;
}

#[tokio::test]
async fn attestation_rejects_a_preestablished_repeatable_read_snapshot() {
    let Some(pool) =
        support::connect_postgres_contract("KnowledgeRetrievalAttestationV2Isolation").await
    else {
        return;
    };
    if !final_schema(&pool, "KnowledgeRetrievalAttestationV2Isolation").await {
        return;
    }
    let fixture = seed_fixture(&pool).await;
    let policy = register_policy(&pool, 1, 1024, 1024).await;
    let valid = scope(&fixture, policy, vec![]);

    let mut transaction = pool.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *transaction)
        .await
        .unwrap();
    let _: i64 = sqlx::query_scalar("SELECT count(*) FROM products")
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
    let error = sqlx::query_scalar::<_, Value>("SELECT kb_knowledge_attest_matching_scope_v2($1)")
        .bind(&valid)
        .fetch_one(&mut *transaction)
        .await
        .unwrap_err();
    assert_contract_error(error, "KNOWLEDGE_MATCHING_SCOPE_V2_INVALID");
    transaction.rollback().await.unwrap();
    remove_fixture(&pool, &fixture).await;
}

#[tokio::test]
async fn read_committed_attestation_waits_then_validates_the_committed_scope() {
    let Some(pool) =
        support::connect_postgres_contract("KnowledgeRetrievalAttestationV2PostLockState").await
    else {
        return;
    };
    if !final_schema(&pool, "KnowledgeRetrievalAttestationV2PostLockState").await {
        return;
    }
    let fixture = seed_fixture(&pool).await;
    let policy = register_policy(&pool, 1, 1024, 1024).await;
    let requirement_id = Uuid::new_v4();
    let valid = scope(
        &fixture,
        policy,
        vec![frozen_hit(&fixture, &fixture.chunks[0], requirement_id, 1)],
    );

    let mut mutation = pool.begin().await.unwrap();
    let mutation_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *mutation)
        .await
        .unwrap();
    sqlx::query("UPDATE products SET current_version_id=NULL WHERE id=$1")
        .bind(fixture.product_id)
        .execute(&mut *mutation)
        .await
        .unwrap();
    sqlx::query("UPDATE documents SET file_name='post-lock-mutated.txt' WHERE id=$1")
        .bind(fixture.document_id)
        .execute(&mut *mutation)
        .await
        .unwrap();
    sqlx::query("UPDATE chunks SET content='post-lock mutated bytes' WHERE id=$1")
        .bind(fixture.chunks[0].id)
        .execute(&mut *mutation)
        .await
        .unwrap();

    let mut attestation_connection = pool.acquire().await.unwrap();
    let attestation_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *attestation_connection)
        .await
        .unwrap();
    let attestation_scope = valid.clone();
    let attestation_task = tokio::spawn(async move {
        sqlx::query_scalar::<_, Value>("SELECT kb_knowledge_attest_matching_scope_v2($1)")
            .bind(attestation_scope)
            .fetch_one(&mut *attestation_connection)
            .await
    });

    let mut attestation_blocked = false;
    for _ in 0..250 {
        let blockers: Vec<i32> = sqlx::query_scalar("SELECT pg_blocking_pids($1)")
            .bind(attestation_pid)
            .fetch_one(&mut *mutation)
            .await
            .unwrap();
        attestation_blocked = blockers.contains(&mutation_pid);
        if attestation_blocked || attestation_task.is_finished() {
            break;
        }
        sleep(Duration::from_millis(20)).await;
    }
    assert!(
        attestation_blocked,
        "READ COMMITTED attestation did not wait for the in-flight scope mutation"
    );
    mutation.commit().await.unwrap();

    let error = timeout(Duration::from_secs(10), attestation_task)
        .await
        .expect("attestation remained blocked after mutation commit")
        .expect("attestation task panicked")
        .unwrap_err();
    assert_contract_error(error, "KNOWLEDGE_MATCHING_SCOPE_V2_MISMATCH");

    sqlx::query("UPDATE products SET current_version_id=$2 WHERE id=$1")
        .bind(fixture.product_id)
        .bind(fixture.version_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE documents SET file_name=$2 WHERE id=$1")
        .bind(fixture.document_id)
        .bind(DOCUMENT_NAME)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE chunks SET content=$2 WHERE id=$1")
        .bind(fixture.chunks[0].id)
        .bind(fixture.chunks[0].content)
        .execute(&pool)
        .await
        .unwrap();
    remove_fixture(&pool, &fixture).await;
}

#[tokio::test]
async fn attestation_locks_live_scope_and_rerank_revision_against_concurrent_mutation() {
    let Some(pool) =
        support::connect_postgres_contract("KnowledgeRetrievalAttestationV2AtomicScope").await
    else {
        return;
    };
    if !final_schema(&pool, "KnowledgeRetrievalAttestationV2AtomicScope").await {
        return;
    }
    let fixture = seed_fixture(&pool).await;
    let policy = register_policy(&pool, 1, 1024, 1024).await;
    let requirement_id = Uuid::new_v4();
    let valid = scope(
        &fixture,
        policy.clone(),
        vec![frozen_hit(&fixture, &fixture.chunks[0], requirement_id, 1)],
    );

    let mut attest_transaction = pool.begin().await.unwrap();
    let _: Value = sqlx::query_scalar("SELECT kb_knowledge_attest_matching_scope_v2($1)")
        .bind(&valid)
        .fetch_one(&mut *attest_transaction)
        .await
        .unwrap();

    let mut product_connection = pool.acquire().await.unwrap();
    let product_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *product_connection)
        .await
        .unwrap();
    let product_id = fixture.product_id;
    let product_task = tokio::spawn(async move {
        sqlx::query("UPDATE products SET current_version_id=NULL WHERE id=$1")
            .bind(product_id)
            .execute(&mut *product_connection)
            .await
    });

    let mut document_connection = pool.acquire().await.unwrap();
    let document_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *document_connection)
        .await
        .unwrap();
    let document_id = fixture.document_id;
    let document_task = tokio::spawn(async move {
        sqlx::query("UPDATE documents SET file_name='mutated.txt' WHERE id=$1")
            .bind(document_id)
            .execute(&mut *document_connection)
            .await
    });

    let mut chunk_connection = pool.acquire().await.unwrap();
    let chunk_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *chunk_connection)
        .await
        .unwrap();
    let chunk_id = fixture.chunks[0].id;
    let chunk_task = tokio::spawn(async move {
        sqlx::query("UPDATE chunks SET content='mutated bytes' WHERE id=$1")
            .bind(chunk_id)
            .execute(&mut *chunk_connection)
            .await
    });

    let mut all_blocked = false;
    for _ in 0..250 {
        all_blocked = sqlx::query_scalar(
            "SELECT bool_and(EXISTS(
                 SELECT 1 FROM pg_locks lock_value
                  WHERE lock_value.pid=pid_value AND NOT lock_value.granted))
               FROM unnest($1::int[]) pid_value",
        )
        .bind(vec![product_pid, document_pid, chunk_pid])
        .fetch_one(&mut *attest_transaction)
        .await
        .unwrap();
        if all_blocked
            || (product_task.is_finished()
                || document_task.is_finished()
                || chunk_task.is_finished())
        {
            break;
        }
        sleep(Duration::from_millis(20)).await;
    }
    assert!(
        all_blocked,
        "live-scope mutations did not wait on attestation locks"
    );
    attest_transaction.rollback().await.unwrap();
    for task in [product_task, document_task, chunk_task] {
        assert_eq!(
            timeout(Duration::from_secs(10), task)
                .await
                .unwrap()
                .unwrap()
                .unwrap()
                .rows_affected(),
            1
        );
    }
    sqlx::query("UPDATE products SET current_version_id=$2 WHERE id=$1")
        .bind(fixture.product_id)
        .bind(fixture.version_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE documents SET file_name=$2 WHERE id=$1")
        .bind(fixture.document_id)
        .bind(DOCUMENT_NAME)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE chunks SET content=$2 WHERE id=$1")
        .bind(fixture.chunks[0].id)
        .bind(fixture.chunks[0].content)
        .execute(&pool)
        .await
        .unwrap();

    let rerank_sha: String = sqlx::query_scalar(
        "SELECT rerank_revision_sha256 FROM knowledge_retrieval_policies_v2 WHERE policy_sha256=$1",
    )
    .bind(policy["policy_sha256"].as_str().unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    let mut rerank_attestation = pool.begin().await.unwrap();
    let _: Value = sqlx::query_scalar("SELECT kb_knowledge_attest_matching_scope_v2($1)")
        .bind(&valid)
        .fetch_one(&mut *rerank_attestation)
        .await
        .unwrap();
    let mut revoke_connection = pool.acquire().await.unwrap();
    let revoke_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *revoke_connection)
        .await
        .unwrap();
    let revoke_task = tokio::spawn(async move {
        sqlx::query(
            "UPDATE rerank_revisions_v2 SET support_state='revoked' WHERE revision_sha256=$1",
        )
        .bind(rerank_sha)
        .execute(&mut *revoke_connection)
        .await
    });
    let mut revoke_blocked = false;
    for _ in 0..250 {
        revoke_blocked = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM pg_locks WHERE pid=$1 AND NOT granted)",
        )
        .bind(revoke_pid)
        .fetch_one(&mut *rerank_attestation)
        .await
        .unwrap();
        if revoke_blocked || revoke_task.is_finished() {
            break;
        }
        sleep(Duration::from_millis(20)).await;
    }
    assert!(
        revoke_blocked,
        "rerank revocation did not wait on attestation lock"
    );
    rerank_attestation.rollback().await.unwrap();
    assert_eq!(
        timeout(Duration::from_secs(10), revoke_task)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .rows_affected(),
        1
    );
    assert_contract_error(
        attest(&pool, &valid).await.unwrap_err(),
        "KNOWLEDGE_MATCHING_RETRIEVAL_POLICY_V2_INVALID",
    );
    remove_fixture(&pool, &fixture).await;
}

#[tokio::test]
async fn independently_rejects_hit_identity_digest_length_and_offset_mutations() {
    let Some(pool) =
        support::connect_postgres_contract("KnowledgeRetrievalAttestationV2TamperMatrix").await
    else {
        return;
    };
    if !final_schema(&pool, "KnowledgeRetrievalAttestationV2TamperMatrix").await {
        return;
    }
    let fixture = seed_fixture(&pool).await;
    let policy = register_policy(&pool, 1, 1024, 1024).await;
    let valid = scope(
        &fixture,
        policy,
        vec![frozen_hit(&fixture, &fixture.chunks[0], Uuid::new_v4(), 1)],
    );
    attest(&pool, &valid).await.unwrap();

    let mut mutations = Vec::new();
    let mut value = valid.clone();
    value["products"][0]["identity_sha256"] = json!("0".repeat(64));
    mutations.push((
        "product/version identity digest",
        value,
        "KNOWLEDGE_MATCHING_SCOPE_V2_MISMATCH",
    ));
    let mut value = valid.clone();
    value["products"][0]["product_id"] = json!(Uuid::new_v4());
    mutations.push((
        "product identity",
        value,
        "KNOWLEDGE_MATCHING_SCOPE_V2_MISMATCH",
    ));
    let mut value = valid.clone();
    value["products"][0]["product_version_id"] = json!(Uuid::new_v4());
    mutations.push((
        "product version identity",
        value,
        "KNOWLEDGE_MATCHING_SCOPE_V2_MISMATCH",
    ));
    let mut value = valid.clone();
    value["frozen_hits"][0]["product_version_artifact_id"] = json!(Uuid::new_v4());
    mutations.push((
        "product/version artifact id",
        value,
        "KNOWLEDGE_MATCHING_HIT_V2_MISMATCH",
    ));
    let mut value = valid.clone();
    value["frozen_hits"][0]["document_id"] = json!(Uuid::new_v4());
    mutations.push(("document id", value, "KNOWLEDGE_MATCHING_HIT_V2_MISMATCH"));
    let mut value = valid.clone();
    value["frozen_hits"][0]["source_chunk_id"] = json!(Uuid::new_v4());
    mutations.push((
        "source chunk id",
        value,
        "KNOWLEDGE_MATCHING_HIT_V2_MISMATCH",
    ));
    let mut value = valid.clone();
    value["frozen_hits"][0]["chunk_sha256"] = json!("0".repeat(64));
    mutations.push((
        "chunk digest only",
        value,
        "KNOWLEDGE_MATCHING_HIT_V2_MISMATCH",
    ));
    let mut value = valid.clone();
    value["frozen_hits"][0]["chunk_byte_length"] = json!(fixture.chunks[0].content.len() + 1);
    mutations.push((
        "chunk length only",
        value,
        "KNOWLEDGE_MATCHING_HIT_V2_MISMATCH",
    ));
    let mut value = valid.clone();
    value["frozen_hits"][0]["quote_start_offset"] = json!(1);
    mutations.push(("start offset", value, "KNOWLEDGE_MATCHING_HIT_V2_INVALID"));
    let mut value = valid.clone();
    value["frozen_hits"][0]["quote_end_offset"] = json!(fixture.chunks[0].content.len() + 1);
    mutations.push(("end offset", value, "KNOWLEDGE_MATCHING_HIT_V2_MISMATCH"));
    let mut value = valid.clone();
    value["frozen_hits"][0]["retrieval_raw_score"] = json!("0.99999");
    mutations.push((
        "score precision",
        value,
        "KNOWLEDGE_MATCHING_HIT_V2_INVALID",
    ));

    for (_label, mutated, expected) in mutations {
        assert_contract_error(attest(&pool, &mutated).await.unwrap_err(), expected);
    }

    remove_fixture(&pool, &fixture).await;
}

#[tokio::test]
async fn rejects_untrusted_source_and_live_name_content_type_or_eligibility_mismatch() {
    let Some(pool) =
        support::connect_postgres_contract("KnowledgeRetrievalAttestationV2Sources").await
    else {
        return;
    };
    if !final_schema(&pool, "KnowledgeRetrievalAttestationV2Sources").await {
        return;
    }
    let fixture = seed_fixture(&pool).await;
    let policy = register_policy(&pool, 1, 1024, 1024).await;
    let requirement_id = Uuid::new_v4();

    let caption = scope(
        &fixture,
        policy.clone(),
        vec![frozen_hit(&fixture, &fixture.chunks[3], requirement_id, 1)],
    );
    assert_contract_error(
        attest(&pool, &caption).await.unwrap_err(),
        "KNOWLEDGE_MATCHING_HIT_V2_INVALID",
    );

    let valid = scope(
        &fixture,
        policy,
        vec![frozen_hit(&fixture, &fixture.chunks[0], requirement_id, 1)],
    );
    let mut wrong_name = valid.clone();
    wrong_name["frozen_hits"][0]["frozen_document_display_name"] = json!("renamed.txt");
    assert_contract_error(
        attest(&pool, &wrong_name).await.unwrap_err(),
        "KNOWLEDGE_MATCHING_HIT_V2_MISMATCH",
    );
    let mut wrong_content = valid.clone();
    wrong_content["frozen_hits"][0]["chunk_utf8"] = json!("forged");
    wrong_content["frozen_hits"][0]["chunk_sha256"] = json!(knowledge::sha256_hex(b"forged"));
    wrong_content["frozen_hits"][0]["chunk_byte_length"] = json!(6);
    wrong_content["frozen_hits"][0]["quote_end_offset"] = json!(6);
    assert_contract_error(
        attest(&pool, &wrong_content).await.unwrap_err(),
        "KNOWLEDGE_MATCHING_HIT_V2_MISMATCH",
    );

    sqlx::query("UPDATE chunks SET chunk_type='parent_text' WHERE id=$1")
        .bind(fixture.chunks[0].id)
        .execute(&pool)
        .await
        .unwrap();
    assert_contract_error(
        attest(&pool, &valid).await.unwrap_err(),
        "KNOWLEDGE_MATCHING_HIT_V2_MISMATCH",
    );
    sqlx::query("UPDATE chunks SET chunk_type='text' WHERE id=$1")
        .bind(fixture.chunks[0].id)
        .execute(&pool)
        .await
        .unwrap();

    sqlx::query("UPDATE documents SET enable_status='disabled' WHERE id=$1")
        .bind(fixture.document_id)
        .execute(&pool)
        .await
        .unwrap();
    assert_contract_error(
        attest(&pool, &valid).await.unwrap_err(),
        "KNOWLEDGE_MATCHING_HIT_V2_MISMATCH",
    );
    sqlx::query("UPDATE documents SET enable_status='enabled',index_ready=false WHERE id=$1")
        .bind(fixture.document_id)
        .execute(&pool)
        .await
        .unwrap();
    assert_contract_error(
        attest(&pool, &valid).await.unwrap_err(),
        "KNOWLEDGE_MATCHING_HIT_V2_MISMATCH",
    );
    sqlx::query("UPDATE documents SET index_ready=true WHERE id=$1")
        .bind(fixture.document_id)
        .execute(&pool)
        .await
        .unwrap();
    remove_fixture(&pool, &fixture).await;
}

#[tokio::test]
async fn explicit_exact_prefix_and_reranked_suffix_provenance_is_attested() {
    let Some(pool) =
        support::connect_postgres_contract("KnowledgeRetrievalAttestationV2Provenance").await
    else {
        return;
    };
    if !final_schema(&pool, "KnowledgeRetrievalAttestationV2Provenance").await {
        return;
    }
    let fixture = seed_fixture(&pool).await;
    let policy = register_policy(&pool, 3, 1024, 4096).await;
    let requirement_id = Uuid::new_v4();
    let mut valid = scope(
        &fixture,
        policy,
        vec![
            frozen_hit(&fixture, &fixture.chunks[0], requirement_id, 1),
            frozen_hit(&fixture, &fixture.chunks[1], requirement_id, 2),
        ],
    );
    valid["retrieval_requirements"][0]["requirement_text"] = json!("alpha");
    valid["retrieval_requirements"][0]["requirement_identity_sha256"] =
        json!(knowledge::sha256_hex(b"alpha"));
    valid["retrieval_requirements"][0]["exact_prefix_hit_count"] = json!(1);
    valid["frozen_hits"][1]["pre_rerank_rrf_rank"] = json!(2);
    // A C score of exactly one remains explicit C and must not bypass rerank provenance.
    valid["frozen_hits"][1]["retrieval_raw_score"] = json!("1.000000");
    attest(&pool, &valid).await.unwrap();

    let mut boundary_tamper = valid.clone();
    boundary_tamper["retrieval_requirements"][0]["exact_prefix_hit_count"] = json!(2);
    assert_contract_error(
        attest(&pool, &boundary_tamper).await.unwrap_err(),
        "KNOWLEDGE_MATCHING_HIT_V2_PROVENANCE_INVALID",
    );

    let mut exact_as_c = valid.clone();
    exact_as_c["retrieval_requirements"][0]["exact_prefix_hit_count"] = json!(0);
    exact_as_c["frozen_hits"][0]["pre_rerank_rrf_rank"] = json!(1);
    assert_contract_error(
        attest(&pool, &exact_as_c).await.unwrap_err(),
        "KNOWLEDGE_MATCHING_HIT_V2_PROVENANCE_INVALID",
    );

    let mut inversion = scope(
        &fixture,
        valid["retrieval_policy"].clone(),
        vec![
            frozen_hit(&fixture, &fixture.chunks[0], requirement_id, 1),
            frozen_hit(&fixture, &fixture.chunks[1], requirement_id, 2),
            frozen_hit(&fixture, &fixture.chunks[2], requirement_id, 3),
        ],
    );
    inversion["retrieval_requirements"][0]["requirement_text"] = json!("alpha");
    inversion["retrieval_requirements"][0]["requirement_identity_sha256"] =
        json!(knowledge::sha256_hex(b"alpha"));
    inversion["retrieval_requirements"][0]["exact_prefix_hit_count"] = json!(1);
    inversion["frozen_hits"][1]["retrieval_raw_score"] = json!("0.400000");
    inversion["frozen_hits"][1]["pre_rerank_rrf_rank"] = json!(1);
    inversion["frozen_hits"][2]["retrieval_raw_score"] = json!("0.500000");
    inversion["frozen_hits"][2]["pre_rerank_rrf_rank"] = json!(2);
    assert_contract_error(
        attest(&pool, &inversion).await.unwrap_err(),
        "KNOWLEDGE_MATCHING_HIT_V2_PROVENANCE_INVALID",
    );
    remove_fixture(&pool, &fixture).await;
}

#[tokio::test]
async fn rejects_missing_or_extra_product_scope() {
    let Some(pool) =
        support::connect_postgres_contract("KnowledgeRetrievalAttestationV2Scope").await
    else {
        return;
    };
    if !final_schema(&pool, "KnowledgeRetrievalAttestationV2Scope").await {
        return;
    }
    let fixture = seed_fixture(&pool).await;
    let policy = register_policy(&pool, 1, 1024, 1024).await;
    let mut missing = scope(&fixture, policy.clone(), vec![]);
    missing["products"] = json!([]);
    assert_contract_error(
        attest(&pool, &missing).await.unwrap_err(),
        "KNOWLEDGE_MATCHING_SCOPE_V2_MISMATCH",
    );

    let mut extra = scope(&fixture, policy, vec![]);
    let mut extra_product = product_artifact(&fixture);
    extra_product["id"] = json!(Uuid::new_v4());
    extra_product["product_id"] = json!(Uuid::new_v4());
    extra_product["product_version_id"] = json!(Uuid::new_v4());
    extra["products"]
        .as_array_mut()
        .unwrap()
        .push(extra_product);
    assert_contract_error(
        attest(&pool, &extra).await.unwrap_err(),
        "KNOWLEDGE_MATCHING_SCOPE_V2_MISMATCH",
    );
    remove_fixture(&pool, &fixture).await;
}

#[tokio::test]
async fn rejects_non_dense_ranks_and_duplicate_source_identity() {
    let Some(pool) =
        support::connect_postgres_contract("KnowledgeRetrievalAttestationV2Ranks").await
    else {
        return;
    };
    if !final_schema(&pool, "KnowledgeRetrievalAttestationV2Ranks").await {
        return;
    }
    let fixture = seed_fixture(&pool).await;
    let policy = register_policy(&pool, 2, 1024, 2048).await;
    let requirement_id = Uuid::new_v4();
    let non_dense = scope(
        &fixture,
        policy.clone(),
        vec![frozen_hit(&fixture, &fixture.chunks[0], requirement_id, 2)],
    );
    assert_contract_error(
        attest(&pool, &non_dense).await.unwrap_err(),
        "KNOWLEDGE_MATCHING_HIT_V2_PROVENANCE_INVALID",
    );
    let duplicate = scope(
        &fixture,
        policy,
        vec![
            frozen_hit(&fixture, &fixture.chunks[0], requirement_id, 1),
            frozen_hit(&fixture, &fixture.chunks[0], requirement_id, 2),
        ],
    );
    assert_contract_error(
        attest(&pool, &duplicate).await.unwrap_err(),
        "KNOWLEDGE_MATCHING_HIT_V2_INVALID",
    );
    remove_fixture(&pool, &fixture).await;
}

#[tokio::test]
async fn quota_boundaries_pass_and_each_excess_fails_closed() {
    let Some(pool) =
        support::connect_postgres_contract("KnowledgeRetrievalAttestationV2Quota").await
    else {
        return;
    };
    if !final_schema(&pool, "KnowledgeRetrievalAttestationV2Quota").await {
        return;
    }
    let fixture = seed_fixture(&pool).await;
    let requirement_id = Uuid::new_v4();
    let first = frozen_hit(&fixture, &fixture.chunks[0], requirement_id, 1);
    let second = frozen_hit(&fixture, &fixture.chunks[1], requirement_id, 2);
    let max_chunk = fixture.chunks[0]
        .content
        .len()
        .max(fixture.chunks[1].content.len());
    let total = fixture.chunks[0].content.len() + fixture.chunks[1].content.len();

    let exact_policy = register_policy(&pool, 2, max_chunk, total).await;
    attest(
        &pool,
        &scope(&fixture, exact_policy, vec![first.clone(), second.clone()]),
    )
    .await
    .unwrap();
    let max_policy = register_policy(&pool, 1_000_000, 1_073_741_824, 1_099_511_627_776).await;
    attest(&pool, &scope(&fixture, max_policy, vec![]))
        .await
        .unwrap();

    for mut invalid_policy in [
        json!({"max_hits": 1_000_001}),
        json!({"max_chunk_bytes": 1_073_741_825_u64}),
        json!({"max_total_bytes": 1_099_511_627_777_u64}),
    ] {
        let mut invalid_scope = scope(
            &fixture,
            json!({
                "contract_version": CONTRACT,
                "policy_sha256": "e".repeat(64),
                "max_hits": 1,
                "max_chunk_bytes": 1,
                "max_total_bytes": 1
            }),
            vec![],
        );
        for (key, value) in invalid_policy.as_object_mut().unwrap() {
            invalid_scope["retrieval_policy"][key.as_str()] = value.clone();
        }
        assert_contract_error(
            attest(&pool, &invalid_scope).await.unwrap_err(),
            "KNOWLEDGE_MATCHING_RETRIEVAL_POLICY_V2_INVALID",
        );
    }

    let hit_count_policy = register_policy(&pool, 1, max_chunk, total).await;
    let first_chunk_minus_one = fixture.chunks[0].content.len() - 1;
    let chunk_policy = register_policy(&pool, 2, first_chunk_minus_one, total).await;
    let total_policy = register_policy(&pool, 2, max_chunk, total - 1).await;
    for failed_scope in [
        scope(
            &fixture,
            hit_count_policy,
            vec![first.clone(), second.clone()],
        ),
        scope(&fixture, chunk_policy, vec![first.clone()]),
        scope(&fixture, total_policy, vec![first, second]),
    ] {
        assert_contract_error(
            attest(&pool, &failed_scope).await.unwrap_err(),
            "KNOWLEDGE_MATCHING_HIT_V2_QUOTA_EXCEEDED",
        );
    }
    remove_fixture(&pool, &fixture).await;
}
