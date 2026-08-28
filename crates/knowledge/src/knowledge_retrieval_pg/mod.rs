//! PostgreSQL adapter for the knowledge-base-owned `KnowledgeRetrievalPort`.
//!
//! This is the only matching-facing implementation allowed to inspect live
//! knowledge rows.  A single query returns complete document/chunk snapshots;
//! callers receive owned DTOs and must freeze them immediately.

use crate::knowledge_retrieval::{
    CompanyEvidenceRequestV1, EligibleEvidenceVersionV1, EmbeddingRevisionV2,
    KNOWLEDGE_EVIDENCE_CONTRACT_V1, KNOWLEDGE_EVIDENCE_CONTRACT_V2, KNOWLEDGE_EVIDENCE_SCHEMA_V1,
    KNOWLEDGE_EVIDENCE_SCHEMA_V3, KnowledgeEvidenceBatchV1, KnowledgeEvidenceBatchV3,
    KnowledgeEvidenceHitV1, KnowledgeEvidenceHitV3, KnowledgeEvidenceMediaV1,
    KnowledgeEvidenceScopeV2, KnowledgeRetrievalError, KnowledgeRetrievalPort,
    KnowledgeRetrievalPortV3, KnowledgeSourceTypeV2, ProductEvidenceRequestV1, RerankRevisionV2,
    RetrievalPolicyIdentityV1, RetrievalPolicyV2, UTF8_BYTE_OFFSET_UNIT, validate_evidence_batch,
    validate_evidence_batch_v3,
};
use async_trait::async_trait;
use rust_decimal::{Decimal, RoundingStrategy};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Row, Transaction};
use std::{collections::BTreeMap, collections::HashMap, collections::HashSet, sync::Arc};
use uuid::Uuid;

pub(crate) mod http_v2;
pub(crate) mod rerank_v2;
mod semantic_v2;

const ABSOLUTE_MAX_HITS: u32 = 256;
const ABSOLUTE_MAX_CHUNK_BYTES: u32 = 1_048_576;
const ABSOLUTE_MAX_TOTAL_BYTES: u64 = 16 * 1_048_576;
const V2_MAX_HITS: u32 = 1_000_000;
const V2_MAX_CHUNK_BYTES: u32 = 1_073_741_824;
const V2_MAX_TOTAL_BYTES: u64 = 1_099_511_627_776;

#[derive(Clone)]
pub struct PostgresKnowledgeRetrievalAdapter {
    pool: PgPool,
    semantic_runtime_v2: Option<SemanticRuntimeV2>,
    allow_exact_only_v2_contract_tests: bool,
}

#[derive(Clone)]
struct SemanticRuntimeV2 {
    embedding: semantic_v2::StrictEmbeddingClientV2,
    rerank: rerank_v2::StrictRerankClientV2,
}

struct RetrievalQuery<'a> {
    workspace_kind: &'static str,
    requirement_identity_sha256: &'a str,
    requirement_text: &'a str,
    selected_versions: &'a [Uuid],
    policy: &'a RetrievalPolicyIdentityV1,
}

#[derive(Debug, PartialEq, Eq)]
enum RetrievalContract {
    LexicalV1,
}

fn dispatch_contract(contract_version: &str) -> Result<RetrievalContract, KnowledgeRetrievalError> {
    match contract_version {
        KNOWLEDGE_EVIDENCE_CONTRACT_V1 => Ok(RetrievalContract::LexicalV1),
        KNOWLEDGE_EVIDENCE_CONTRACT_V2 => Err(KnowledgeRetrievalError::InvalidRequest(
            "knowledge-evidence-v2 is not available until the v2 implementation".into(),
        )),
        _ => Err(KnowledgeRetrievalError::InvalidRequest(format!(
            "unsupported knowledge evidence contract: {contract_version}"
        ))),
    }
}

impl PostgresKnowledgeRetrievalAdapter {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            semantic_runtime_v2: None,
            allow_exact_only_v2_contract_tests: false,
        }
    }

    /// Exact-only seam for the exhaustive A/B PostgreSQL contract suite. It is
    /// absent from normal production builds so V2 can never silently emit an
    /// unrereanked semantic tail.
    #[cfg(feature = "knowledge-v2-exact-contract-tests")]
    #[doc(hidden)]
    pub fn new_exact_only_v2_contract_tests(pool: PgPool) -> Self {
        Self {
            pool,
            semantic_runtime_v2: None,
            allow_exact_only_v2_contract_tests: true,
        }
    }

    /// Constructs the fail-closed complete V2 adapter.
    pub(crate) fn new_complete_v2(
        pool: PgPool,
        credentials: Arc<dyn semantic_v2::EmbeddingCredentialResolverV2>,
    ) -> Result<Self, KnowledgeRetrievalError> {
        Ok(Self {
            pool,
            semantic_runtime_v2: Some(SemanticRuntimeV2 {
                embedding: semantic_v2::StrictEmbeddingClientV2::new(credentials.clone())?,
                rerank: rerank_v2::StrictRerankClientV2::new(credentials)?,
            }),
            allow_exact_only_v2_contract_tests: false,
        })
    }

    pub fn new_complete_v2_from_environment(pool: PgPool) -> Result<Self, KnowledgeRetrievalError> {
        Self::new_complete_v2(
            pool,
            Arc::new(semantic_v2::EnvironmentEmbeddingCredentialResolverV2),
        )
    }

    async fn retrieve(
        &self,
        query: RetrievalQuery<'_>,
    ) -> Result<KnowledgeEvidenceBatchV1, KnowledgeRetrievalError> {
        let RetrievalQuery {
            workspace_kind,
            requirement_identity_sha256,
            requirement_text,
            selected_versions,
            policy,
        } = query;
        let RetrievalContract::LexicalV1 = dispatch_contract(&policy.contract_version)?;
        validate_request(
            requirement_identity_sha256,
            requirement_text,
            &policy.contract_version,
            &policy.policy_sha256,
            policy.max_hits,
            policy.max_chunk_bytes,
            policy.max_total_bytes,
        )?;
        let selected_version_ids = selected_versions.iter().copied().collect::<HashSet<_>>();
        if selected_version_ids.len() != selected_versions.len()
            || selected_version_ids.contains(&Uuid::nil())
        {
            return Err(KnowledgeRetrievalError::InvalidRequest(
                "selected versions must be unique, non-nil UUIDs".into(),
            ));
        }
        let rows = sqlx::query(
            "SELECT c.id AS source_chunk_id,c.content,d.id AS document_id,d.file_name,
                    p.id AS product_id,pv.id AS product_version_id,w.kind AS workspace_kind
               FROM workspaces w
               JOIN products p ON p.workspace_id=w.id
               JOIN product_versions pv ON pv.product_id=p.id AND p.current_version_id=pv.id
               LEFT JOIN documents d ON d.product_version_id=pv.id
                AND d.deleted_at IS NULL AND d.enable_status='enabled' AND d.index_ready
               LEFT JOIN chunks c ON c.document_id=d.id AND c.product_version_id=pv.id
                AND octet_length(convert_to(c.content,'UTF8')) <= $3
              WHERE w.kind=$1 AND pv.status='active' AND pv.deleted_at IS NULL
                AND (($1='product_line' AND p.kind='product') OR ($1='company' AND p.kind='library'))
                AND (cardinality($2::uuid[])=0 OR pv.id=ANY($2::uuid[]))
              ORDER BY p.id,pv.id,d.id,c.id",
        )
        .bind(workspace_kind)
        .bind(selected_versions)
        .bind(i64::from(policy.max_chunk_bytes))
        .fetch_all(&self.pool)
        .await
        .map_err(|error| KnowledgeRetrievalError::Unavailable(error.to_string()))?;

        let mut ranked = Vec::new();
        let mut eligible_versions = BTreeMap::new();
        for row in rows {
            let product_id = row.get::<Uuid, _>("product_id");
            let product_version_id = row.get::<Uuid, _>("product_version_id");
            eligible_versions
                .entry((product_id, product_version_id))
                .or_insert_with(|| EligibleEvidenceVersionV1 {
                    product_id,
                    product_version_id,
                    workspace_kind: workspace_kind.to_string(),
                    frozen_display_name: product_version_id.to_string(),
                });
            let Some(chunk_utf8) = row.get::<Option<String>, _>("content") else {
                continue;
            };
            let (Some(document_id), Some(source_chunk_id), Some(file_name)) = (
                row.get::<Option<Uuid>, _>("document_id"),
                row.get::<Option<Uuid>, _>("source_chunk_id"),
                row.get::<Option<String>, _>("file_name"),
            ) else {
                return Err(KnowledgeRetrievalError::Unavailable(
                    "retrieval query returned a partial hit projection".into(),
                ));
            };
            let raw_score = lexical_score(requirement_text, &chunk_utf8);
            ranked.push((
                raw_score,
                product_id,
                product_version_id,
                document_id,
                source_chunk_id,
                file_name,
                chunk_utf8,
            ));
        }
        if !selected_version_ids.is_empty()
            && eligible_versions
                .values()
                .map(|version| version.product_version_id)
                .collect::<HashSet<_>>()
                != selected_version_ids
        {
            return Err(KnowledgeRetrievalError::InvalidRequest(
                "selected versions are not the exact current eligible workspace scope".into(),
            ));
        }
        ranked.sort_by(|left, right| {
            right
                .0
                .cmp(&left.0)
                .then(left.1.cmp(&right.1))
                .then(left.2.cmp(&right.2))
                .then(left.3.cmp(&right.3))
                .then(left.4.cmp(&right.4))
        });

        let mut chosen = Vec::new();
        let mut chosen_chunks = HashSet::new();
        let mut chosen_bytes = 0u64;
        for row in ranked {
            if chosen.len() >= policy.max_hits as usize {
                break;
            }
            if row.0 <= Decimal::ZERO || !chosen_chunks.insert(row.4) {
                continue;
            }
            let next_bytes = chosen_bytes + row.6.len() as u64;
            if next_bytes <= policy.max_total_bytes {
                chosen_bytes = next_bytes;
                chosen.push(row);
            }
        }
        let mut hits = Vec::new();
        for (score, product_id, product_version_id, document_id, source_chunk_id, name, chunk) in
            chosen
        {
            let chunk_byte_length = chunk.len() as u64;
            let chunk_sha256 = hex::encode(Sha256::digest(chunk.as_bytes()));
            hits.push(KnowledgeEvidenceHitV1 {
                schema_version: KNOWLEDGE_EVIDENCE_SCHEMA_V1,
                document_id,
                source_chunk_id,
                product_id,
                product_version_id,
                workspace_kind: workspace_kind.to_string(),
                frozen_document_display_name: name,
                chunk_utf8: chunk,
                chunk_sha256,
                chunk_byte_length,
                quote_start_offset: 0,
                quote_end_offset: chunk_byte_length,
                offset_unit: UTF8_BYTE_OFFSET_UNIT.to_string(),
                retrieval_rank: hits.len() as u32 + 1,
                retrieval_raw_score: fixed_six(score),
                retrieval_contract_version: policy.contract_version.clone(),
            });
        }
        let batch = KnowledgeEvidenceBatchV1 {
            schema_version: KNOWLEDGE_EVIDENCE_SCHEMA_V1,
            eligible_versions: eligible_versions.into_values().collect(),
            hits,
        };
        validate_evidence_batch(workspace_kind, &batch, policy)?;
        Ok(batch)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ExactCandidateV2 {
    document_id: Uuid,
    source_chunk_id: Uuid,
    product_id: Uuid,
    product_version_id: Uuid,
    frozen_document_display_name: String,
    chunk_utf8: String,
    source_type: KnowledgeSourceTypeV2,
}

impl ExactCandidateV2 {
    fn byte_length(&self) -> u64 {
        self.chunk_utf8.len() as u64
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ValidatedSemanticPolicyV2 {
    pub(crate) policy: RetrievalPolicyV2,
    pub(crate) revision: EmbeddingRevisionV2,
    pub(crate) credential_ref: String,
}

#[derive(Debug, PartialEq, Eq)]
struct ExactSelectionV2 {
    hits: Vec<ExactCandidateV2>,
    exact_versions_truncated: u64,
    exact_hits_truncated: u64,
}

fn normalize_exact_v2(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

fn compare_primary_v2(left: &ExactCandidateV2, right: &ExactCandidateV2) -> std::cmp::Ordering {
    left.byte_length()
        .cmp(&right.byte_length())
        .then(left.document_id.cmp(&right.document_id))
        .then(left.source_chunk_id.cmp(&right.source_chunk_id))
}

fn compare_b_v2(left: &ExactCandidateV2, right: &ExactCandidateV2) -> std::cmp::Ordering {
    left.product_id
        .cmp(&right.product_id)
        .then(left.product_version_id.cmp(&right.product_version_id))
        .then_with(|| compare_primary_v2(left, right))
}

fn select_exact_prefix_v2(
    candidates: Vec<ExactCandidateV2>,
    policy: &RetrievalPolicyIdentityV1,
) -> Result<ExactSelectionV2, KnowledgeRetrievalError> {
    let total_exact_hits = candidates.len() as u64;
    let mut by_version: BTreeMap<(Uuid, Uuid), Vec<ExactCandidateV2>> = BTreeMap::new();
    for candidate in candidates {
        by_version
            .entry((candidate.product_id, candidate.product_version_id))
            .or_default()
            .push(candidate);
    }
    for version_candidates in by_version.values_mut() {
        version_candidates.sort_by(compare_primary_v2);
    }

    let exact_versions_truncated = (by_version.len() as u64).saturating_sub(policy.max_hits as u64);
    let mut hits = Vec::new();
    let mut primary_ids = HashSet::new();
    let mut chosen_bytes = 0u64;
    for version_candidates in by_version.values().take(policy.max_hits as usize) {
        let primary = version_candidates
            .first()
            .expect("an exact-bearing version has a candidate");
        if primary.byte_length() > u64::from(policy.max_chunk_bytes) {
            return Err(KnowledgeRetrievalError::QuotaExceeded(format!(
                "exact primary chunk {} exceeds max_chunk_bytes",
                primary.source_chunk_id
            )));
        }
        chosen_bytes = chosen_bytes
            .checked_add(primary.byte_length())
            .ok_or_else(|| {
                KnowledgeRetrievalError::QuotaExceeded(
                    "exact primary prefix byte length overflow".into(),
                )
            })?;
        if chosen_bytes > policy.max_total_bytes {
            return Err(KnowledgeRetrievalError::QuotaExceeded(
                "exact primary prefix exceeds max_total_bytes".into(),
            ));
        }
        primary_ids.insert(primary.source_chunk_id);
        hits.push(primary.clone());
    }

    let mut remaining = by_version
        .into_values()
        .flatten()
        .filter(|candidate| !primary_ids.contains(&candidate.source_chunk_id))
        .collect::<Vec<_>>();
    remaining.sort_by(compare_b_v2);
    for candidate in remaining {
        if hits.len() >= policy.max_hits as usize
            || candidate.byte_length() > u64::from(policy.max_chunk_bytes)
        {
            continue;
        }
        let Some(next_bytes) = chosen_bytes.checked_add(candidate.byte_length()) else {
            continue;
        };
        if next_bytes > policy.max_total_bytes {
            continue;
        }
        chosen_bytes = next_bytes;
        hits.push(candidate);
    }

    Ok(ExactSelectionV2 {
        exact_hits_truncated: total_exact_hits.saturating_sub(hits.len() as u64),
        exact_versions_truncated,
        hits,
    })
}

#[async_trait]
impl KnowledgeRetrievalPort for PostgresKnowledgeRetrievalAdapter {
    async fn retrieve_product_evidence(
        &self,
        request: ProductEvidenceRequestV1,
    ) -> Result<KnowledgeEvidenceBatchV1, KnowledgeRetrievalError> {
        if request.schema_version != KNOWLEDGE_EVIDENCE_SCHEMA_V1 {
            return Err(KnowledgeRetrievalError::InvalidRequest(
                "unsupported ProductEvidenceRequestV1 schema_version".into(),
            ));
        }
        self.retrieve(RetrievalQuery {
            workspace_kind: "product_line",
            requirement_identity_sha256: &request.requirement_identity_sha256,
            requirement_text: &request.requirement_text,
            selected_versions: &request.product_version_ids,
            policy: &request.retrieval_policy,
        })
        .await
    }

    async fn retrieve_company_evidence(
        &self,
        request: CompanyEvidenceRequestV1,
    ) -> Result<KnowledgeEvidenceBatchV1, KnowledgeRetrievalError> {
        if request.schema_version != KNOWLEDGE_EVIDENCE_SCHEMA_V1 {
            return Err(KnowledgeRetrievalError::InvalidRequest(
                "unsupported CompanyEvidenceRequestV1 schema_version".into(),
            ));
        }
        self.retrieve(RetrievalQuery {
            workspace_kind: "company",
            requirement_identity_sha256: &request.requirement_identity_sha256,
            requirement_text: &request.requirement_text,
            selected_versions: &request.library_version_ids,
            policy: &request.retrieval_policy,
        })
        .await
    }
}

impl PostgresKnowledgeRetrievalAdapter {
    async fn retrieve_v2(
        &self,
        workspace_kind: &'static str,
        request_schema_version: u16,
        requirement_identity_sha256: &str,
        requirement_text: &str,
        selected_versions: &[Uuid],
        policy: &RetrievalPolicyIdentityV1,
    ) -> Result<KnowledgeEvidenceBatchV3, KnowledgeRetrievalError> {
        validate_request_v2(
            request_schema_version,
            requirement_identity_sha256,
            requirement_text,
            selected_versions,
            policy,
        )?;
        let validated_policy = self.validate_supported_policy_v2(policy).await?;
        let mut tx = self.pool.begin().await.map_err(database_unavailable)?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .execute(&mut *tx)
            .await
            .map_err(database_unavailable)?;
        semantic_v2::lock_supported_policy_in_snapshot(&mut tx, policy, &validated_policy).await?;
        // Lock and validate the canonical rerank identity even when C is empty.
        // Empty C still performs no credential lookup and no provider request.
        let (rerank_revision, rerank_credential_ref) =
            lock_rerank_revision_v2(&mut tx, &validated_policy.policy).await?;

        let selected_version_ids = selected_versions.iter().copied().collect::<HashSet<_>>();
        let trusted_types = ["text", "parent_text", "image_ocr"];
        let rows = sqlx::query(
            "SELECT c.id AS source_chunk_id,c.content,c.chunk_type,
                    d.id AS document_id,d.file_name,
                    p.id AS product_id,pv.id AS product_version_id,w.kind AS workspace_kind
               FROM workspaces w
               JOIN products p ON p.workspace_id=w.id
               JOIN product_versions pv ON pv.product_id=p.id AND p.current_version_id=pv.id
               LEFT JOIN documents d ON d.product_version_id=pv.id
                AND d.deleted_at IS NULL AND d.enable_status='enabled' AND d.index_ready
               LEFT JOIN chunks c ON c.document_id=d.id AND c.product_version_id=pv.id
                AND c.chunk_type=ANY($3::text[])
                AND (c.chunk_type<>'image_ocr' OR EXISTS (SELECT 1 FROM knowledge_image_ocr_chunk_artifact_mappings mapping WHERE mapping.chunk_id=c.id))
              WHERE w.kind=$1 AND pv.status='active' AND pv.deleted_at IS NULL
                AND (($1='product_line' AND p.kind='product') OR ($1='company' AND p.kind='library'))
                AND (cardinality($2::uuid[])=0 OR pv.id=ANY($2::uuid[]))
              ORDER BY p.id,pv.id,d.id,c.id",
        )
        .bind(workspace_kind)
        .bind(selected_versions)
        .bind(trusted_types)
        .fetch_all(&mut *tx)
        .await
        .map_err(database_unavailable)?;

        let normalized_requirement = normalize_exact_v2(requirement_text);
        let mut eligible_versions = BTreeMap::new();
        let mut candidates = Vec::new();
        for row in rows {
            let product_id = row.get::<Uuid, _>("product_id");
            let product_version_id = row.get::<Uuid, _>("product_version_id");
            eligible_versions
                .entry((product_id, product_version_id))
                .or_insert_with(|| EligibleEvidenceVersionV1 {
                    product_id,
                    product_version_id,
                    workspace_kind: workspace_kind.to_string(),
                    frozen_display_name: product_version_id.to_string(),
                });
            let Some(chunk_utf8) = row.get::<Option<String>, _>("content") else {
                continue;
            };
            let (Some(document_id), Some(source_chunk_id), Some(file_name), Some(chunk_type)) = (
                row.get::<Option<Uuid>, _>("document_id"),
                row.get::<Option<Uuid>, _>("source_chunk_id"),
                row.get::<Option<String>, _>("file_name"),
                row.get::<Option<String>, _>("chunk_type"),
            ) else {
                return Err(KnowledgeRetrievalError::Unavailable(
                    "v2 retrieval query returned a partial hit projection".into(),
                ));
            };
            if !normalize_exact_v2(&chunk_utf8).contains(&normalized_requirement) {
                continue;
            }
            let source_type = match chunk_type.as_str() {
                "text" => KnowledgeSourceTypeV2::Text,
                "parent_text" => KnowledgeSourceTypeV2::ParentText,
                "image_ocr" => KnowledgeSourceTypeV2::ImageOcr,
                _ => {
                    return Err(KnowledgeRetrievalError::Unavailable(
                        "v2 retrieval query returned an untrusted source type".into(),
                    ));
                }
            };
            candidates.push(ExactCandidateV2 {
                document_id,
                source_chunk_id,
                product_id,
                product_version_id,
                frozen_document_display_name: file_name,
                chunk_utf8,
                source_type,
            });
        }
        if !selected_version_ids.is_empty()
            && eligible_versions
                .values()
                .map(|version| version.product_version_id)
                .collect::<HashSet<_>>()
                != selected_version_ids
        {
            return Err(KnowledgeRetrievalError::InvalidRequest(
                "selected versions are not the exact current eligible workspace scope".into(),
            ));
        }

        let selection = select_exact_prefix_v2(candidates, policy)?;
        let exact_versions_truncated = selection.exact_versions_truncated;
        let exact_hits_truncated = selection.exact_hits_truncated;
        let mut hits: Vec<KnowledgeEvidenceHitV3> = selection
            .hits
            .into_iter()
            .enumerate()
            .map(|(index, candidate)| exact_candidate_hit_v2(candidate, workspace_kind, index))
            .collect();
        let exact_prefix_hit_count = u32::try_from(hits.len()).map_err(|_| {
            KnowledgeRetrievalError::InvalidHit(
                "v2 exact prefix length cannot be represented".into(),
            )
        })?;
        let mut semantic_hits_truncated = 0u64;

        if !eligible_versions.is_empty()
            && hits.len() < policy.max_hits as usize
            && self.semantic_runtime_v2.is_none()
            && !self.allow_exact_only_v2_contract_tests
        {
            return Err(KnowledgeRetrievalError::InvalidRequest(
                "complete knowledge-evidence-v2 semantic runtime is not configured".into(),
            ));
        }
        if !eligible_versions.is_empty()
            && hits.len() < policy.max_hits as usize
            && let Some(runtime) = &self.semantic_runtime_v2
        {
            let query_embedding = runtime
                .embedding
                .embed(requirement_text, &validated_policy)
                .await?;
            semantic_v2::validate_query_embedding(&query_embedding, &validated_policy.policy)?;
            let candidates = semantic_v2::recall_in_snapshot(
                &mut tx,
                workspace_kind,
                selected_versions,
                requirement_text,
                &validated_policy.policy,
                &query_embedding,
            )
            .await?;
            if !candidates.is_empty() {
                let reranked = runtime
                    .rerank
                    .rerank(
                        requirement_text,
                        candidates,
                        &validated_policy.policy,
                        &rerank_revision,
                        &rerank_credential_ref,
                    )
                    .await?;
                semantic_hits_truncated = semantic_hits_truncated.saturating_add(
                    append_semantic_tail_v2(&mut hits, reranked, workspace_kind, policy),
                );
            }
        }

        hydrate_evidence_media_v3(&mut tx, &mut hits).await?;
        let batch = KnowledgeEvidenceBatchV3 {
            schema_version: KNOWLEDGE_EVIDENCE_SCHEMA_V3,
            eligible_versions: eligible_versions.into_values().collect(),
            hits,
            exact_prefix_hit_count,
            exact_versions_truncated,
            exact_hits_truncated,
            semantic_hits_truncated,
        };
        validate_evidence_batch_v3(workspace_kind, &batch, policy)?;
        tx.commit().await.map_err(database_unavailable)?;
        Ok(batch)
    }

    async fn validate_supported_policy_v2(
        &self,
        policy: &RetrievalPolicyIdentityV1,
    ) -> Result<ValidatedSemanticPolicyV2, KnowledgeRetrievalError> {
        let row = sqlx::query(
            "SELECT policy.canonical_policy_payload,policy.embedding_revision_sha256,
                    policy.rerank_revision_sha256,
                    policy.contract_version,policy.max_hits,policy.max_chunk_bytes,
                    policy.max_total_bytes,policy.support_state,
                    revision.revision_sha256 AS registered_revision_sha256,
                    revision.canonical_revision_payload,revision.schema_version,
                    revision.provider_protocol_version,revision.provider_model_identifier,
                    revision.provider_model_revision_sha256,revision.endpoint_config_sha256,
                    revision.endpoint_identity,revision.dimension,revision.request_config_sha256,
                    revision.output_normalization_version,revision.credential_ref,
                    reranker.canonical_revision_payload AS rerank_payload,
                    reranker.credential_ref AS rerank_credential_ref
               FROM knowledge_retrieval_policies_v2 policy
               JOIN embedding_revisions_v2 revision
                 ON revision.revision_sha256=policy.embedding_revision_sha256
                AND revision.support_state='supported'
               JOIN rerank_revisions_v2 reranker
                 ON reranker.revision_sha256=policy.rerank_revision_sha256
                AND reranker.support_state='supported'
              WHERE policy.policy_sha256=$1",
        )
        .bind(&policy.policy_sha256)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| KnowledgeRetrievalError::Unavailable(error.to_string()))?;
        let Some(row) = row else {
            return Err(KnowledgeRetrievalError::InvalidRequest(
                "unknown knowledge-evidence-v2 policy".into(),
            ));
        };
        let canonical_policy_payload = row.get::<Vec<u8>, _>("canonical_policy_payload");
        let embedding_revision_sha256 = row.get::<String, _>("embedding_revision_sha256");
        let contract_version = row.get::<String, _>("contract_version");
        let max_hits = row.get::<i64, _>("max_hits");
        let max_chunk_bytes = row.get::<i64, _>("max_chunk_bytes");
        let max_total_bytes = row.get::<i64, _>("max_total_bytes");
        let support_state = row.get::<String, _>("support_state");
        if support_state != "supported"
            || contract_version != policy.contract_version
            || max_hits != i64::from(policy.max_hits)
            || max_chunk_bytes != i64::from(policy.max_chunk_bytes)
            || u64::try_from(max_total_bytes).ok() != Some(policy.max_total_bytes)
        {
            return Err(KnowledgeRetrievalError::InvalidRequest(
                "revoked or mismatched knowledge-evidence-v2 policy".into(),
            ));
        }

        let canonical_revision_payload = row.get::<Vec<u8>, _>("canonical_revision_payload");
        let registered_revision_sha256 = row.get::<String, _>("registered_revision_sha256");
        let revision = serde_json::from_slice::<EmbeddingRevisionV2>(&canonical_revision_payload)
            .map_err(|error| {
            KnowledgeRetrievalError::InvalidRequest(format!(
                "invalid embedding revision v2 artifact: {error}"
            ))
        })?;
        revision.validate().map_err(|error| {
            KnowledgeRetrievalError::InvalidRequest(format!(
                "invalid embedding revision v2 artifact: {error}"
            ))
        })?;
        let canonical_revision_bytes = revision.canonical_bytes().map_err(|error| {
            KnowledgeRetrievalError::InvalidRequest(format!(
                "invalid embedding revision v2 artifact: {error}"
            ))
        })?;
        let revision_digest = revision.sha256().map_err(|error| {
            KnowledgeRetrievalError::InvalidRequest(format!(
                "invalid embedding revision v2 identity: {error}"
            ))
        })?;
        if canonical_revision_bytes != canonical_revision_payload
            || revision_digest != registered_revision_sha256
            || revision_digest != embedding_revision_sha256
            || i16::try_from(revision.schema_version).ok()
                != Some(row.get::<i16, _>("schema_version"))
            || revision.provider_protocol_version
                != row.get::<String, _>("provider_protocol_version")
            || revision.provider_model_identifier
                != row.get::<String, _>("provider_model_identifier")
            || revision.provider_model_revision_sha256
                != row.get::<String, _>("provider_model_revision_sha256")
            || revision.endpoint_config_sha256 != row.get::<String, _>("endpoint_config_sha256")
            || revision.endpoint_identity != row.get::<String, _>("endpoint_identity")
            || i32::try_from(revision.dimension).ok() != Some(row.get::<i32, _>("dimension"))
            || revision.request_config_sha256 != row.get::<String, _>("request_config_sha256")
            || revision.output_normalization_version
                != row.get::<String, _>("output_normalization_version")
        {
            return Err(KnowledgeRetrievalError::InvalidRequest(
                "non-canonical or mismatched embedding revision v2 artifact".into(),
            ));
        }

        let artifact = serde_json::from_slice::<RetrievalPolicyV2>(&canonical_policy_payload)
            .map_err(|error| {
                KnowledgeRetrievalError::InvalidRequest(format!(
                    "invalid knowledge-evidence-v2 policy artifact: {error}"
                ))
            })?;
        artifact.validate().map_err(|error| {
            KnowledgeRetrievalError::InvalidRequest(format!(
                "invalid knowledge-evidence-v2 policy artifact: {error}"
            ))
        })?;
        let canonical_bytes = artifact.canonical_bytes().map_err(|error| {
            KnowledgeRetrievalError::InvalidRequest(format!(
                "invalid knowledge-evidence-v2 policy artifact: {error}"
            ))
        })?;
        let rerank_revision_sha256 = row.get::<String, _>("rerank_revision_sha256");
        let rerank_payload = row.get::<Vec<u8>, _>("rerank_payload");
        let rerank_revision =
            serde_json::from_slice::<RerankRevisionV2>(&rerank_payload).map_err(|error| {
                KnowledgeRetrievalError::InvalidRequest(format!(
                    "invalid rerank revision v2 artifact: {error}"
                ))
            })?;
        rerank_revision.validate().map_err(|error| {
            KnowledgeRetrievalError::InvalidRequest(format!(
                "invalid rerank revision v2 artifact: {error}"
            ))
        })?;
        if canonical_bytes != canonical_policy_payload
            || embedding_revision_sha256 != artifact.embedding.model_revision_sha256
            || rerank_revision_sha256 != artifact.rerank.revision_sha256
            || rerank_revision.sha256().ok().as_deref() != Some(rerank_revision_sha256.as_str())
            || rerank_revision.canonical_bytes().ok().as_deref() != Some(rerank_payload.as_slice())
            || rerank_revision.provider_model_revision_sha256
                != artifact.rerank.model_revision_sha256
            || rerank_revision.config_revision_sha256 != artifact.rerank.config_revision_sha256
        {
            return Err(KnowledgeRetrievalError::InvalidRequest(
                "non-canonical or mismatched knowledge-evidence-v2 policy artifact".into(),
            ));
        }
        let artifact_identity = artifact.request_identity().map_err(|error| {
            KnowledgeRetrievalError::InvalidRequest(format!(
                "invalid knowledge-evidence-v2 policy identity: {error}"
            ))
        })?;
        if artifact_identity != *policy {
            return Err(KnowledgeRetrievalError::InvalidRequest(
                "mismatched knowledge-evidence-v2 policy artifact identity".into(),
            ));
        }
        Ok(ValidatedSemanticPolicyV2 {
            policy: artifact,
            revision,
            credential_ref: row.get("credential_ref"),
        })
    }
}

#[async_trait]
impl KnowledgeRetrievalPortV3 for PostgresKnowledgeRetrievalAdapter {
    async fn retrieve_evidence_v3(
        &self,
        scope: KnowledgeEvidenceScopeV2,
    ) -> Result<KnowledgeEvidenceBatchV3, KnowledgeRetrievalError> {
        match scope {
            KnowledgeEvidenceScopeV2::ProductLine(request) => {
                self.retrieve_v2(
                    "product_line",
                    request.schema_version,
                    &request.requirement_identity_sha256,
                    &request.requirement_text,
                    &request.product_version_ids,
                    &request.retrieval_policy,
                )
                .await
            }
            KnowledgeEvidenceScopeV2::Company(request) => {
                self.retrieve_v2(
                    "company",
                    request.schema_version,
                    &request.requirement_identity_sha256,
                    &request.requirement_text,
                    &request.library_version_ids,
                    &request.retrieval_policy,
                )
                .await
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct RequirementEvidenceBatchesV2 {
    pub route_id: Uuid,
    pub requirement_artifact_id: Uuid,
    pub requirement_identity_sha256: String,
    pub requirement_text: String,
    pub product_line: KnowledgeEvidenceBatchV3,
    pub company: KnowledgeEvidenceBatchV3,
}

#[derive(Debug, Clone)]
pub struct AttestedEvidenceScopeV2 {
    pub attestation_id: Uuid,
    pub attestation_sha256: String,
    pub canonical_scope: serde_json::Value,
}

pub async fn latest_supported_retrieval_policy_v2(
    pool: &PgPool,
) -> Result<Option<RetrievalPolicyIdentityV1>, KnowledgeRetrievalError> {
    let row = sqlx::query(
        "SELECT policy_sha256,contract_version,max_hits,max_chunk_bytes,max_total_bytes
        FROM knowledge_retrieval_policies_v2 WHERE support_state='supported'
        ORDER BY created_at DESC,policy_sha256 LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .map_err(database_unavailable)?;
    let Some(row) = row else { return Ok(None) };
    Ok(Some(RetrievalPolicyIdentityV1 {
        contract_version: row.get("contract_version"),
        policy_sha256: row.get("policy_sha256"),
        max_hits: u32::try_from(row.get::<i64, _>("max_hits")).map_err(|_| {
            KnowledgeRetrievalError::InvalidRequest("policy max_hits out of range".into())
        })?,
        max_chunk_bytes: u32::try_from(row.get::<i64, _>("max_chunk_bytes")).map_err(|_| {
            KnowledgeRetrievalError::InvalidRequest("policy max_chunk_bytes out of range".into())
        })?,
        max_total_bytes: u64::try_from(row.get::<i64, _>("max_total_bytes")).map_err(|_| {
            KnowledgeRetrievalError::InvalidRequest("policy max_total_bytes out of range".into())
        })?,
    }))
}

fn deterministic_uuid_v2(parts: &[&str]) -> Uuid {
    let mut digest = Sha256::new();
    for part in parts {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part.as_bytes());
    }
    let mut bytes: [u8; 16] = digest.finalize()[..16].try_into().expect("sha256 prefix");
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

pub async fn attest_requirement_evidence_v2(
    pool: &PgPool,
    policy: &RetrievalPolicyIdentityV1,
    requirements: &[RequirementEvidenceBatchesV2],
) -> Result<AttestedEvidenceScopeV2, KnowledgeRetrievalError> {
    let mut products = BTreeMap::<Uuid, EligibleEvidenceVersionV1>::new();
    let mut requirement_values = Vec::with_capacity(requirements.len());
    let mut frozen_hits = Vec::new();
    for requirement in requirements {
        for version in requirement
            .product_line
            .eligible_versions
            .iter()
            .chain(requirement.company.eligible_versions.iter())
        {
            products
                .entry(version.product_version_id)
                .or_insert_with(|| version.clone());
        }
        let mut exact = Vec::new();
        let mut semantic = Vec::new();
        for batch in [&requirement.product_line, &requirement.company] {
            for hit in &batch.hits {
                if hit.retrieval_rank <= batch.exact_prefix_hit_count {
                    exact.push(hit.clone());
                } else {
                    semantic.push(hit.clone());
                }
            }
        }
        exact.sort_by_key(|hit| {
            (
                hit.product_id,
                hit.product_version_id,
                hit.document_id,
                hit.source_chunk_id,
            )
        });
        semantic.sort_by(|left, right| {
            right
                .retrieval_raw_score
                .cmp(&left.retrieval_raw_score)
                .then_with(|| left.pre_rerank_rrf_rank.cmp(&right.pre_rerank_rrf_rank))
                .then_with(|| {
                    (
                        left.product_id,
                        left.product_version_id,
                        left.document_id,
                        left.source_chunk_id,
                    )
                        .cmp(&(
                            right.product_id,
                            right.product_version_id,
                            right.document_id,
                            right.source_chunk_id,
                        ))
                })
        });
        let exact_count = exact.len().min(policy.max_hits as usize);
        exact.extend(semantic);
        exact.truncate(policy.max_hits as usize);
        requirement_values.push(serde_json::json!({
            "route_id":requirement.route_id,"requirement_artifact_id":requirement.requirement_artifact_id,
            "requirement_identity_sha256":requirement.requirement_identity_sha256,
            "requirement_text":requirement.requirement_text,"exact_prefix_hit_count":exact_count,
        }));
        for (index, hit) in exact.into_iter().enumerate() {
            let hit_id = deterministic_uuid_v2(&[
                &requirement.route_id.to_string(),
                &requirement.requirement_artifact_id.to_string(),
                &hit.product_version_id.to_string(),
                &hit.document_id.to_string(),
                &hit.source_chunk_id.to_string(),
            ]);
            frozen_hits.push(serde_json::json!({
                "id":hit_id,"route_id":requirement.route_id,"requirement_artifact_id":requirement.requirement_artifact_id,
                "product_version_artifact_id":hit.product_version_id,"document_id":hit.document_id,
                "source_chunk_id":hit.source_chunk_id,"frozen_document_display_name":hit.frozen_document_display_name,
                "chunk_utf8":hit.chunk_utf8,"chunk_sha256":hit.chunk_sha256,"chunk_byte_length":hit.chunk_byte_length,
                "source_type":hit.source_type,"retrieval_rank":index+1,"retrieval_raw_score":hit.retrieval_raw_score,
                "pre_rerank_rrf_rank":hit.pre_rerank_rrf_rank,"quote_start_offset":hit.quote_start_offset,
                "quote_end_offset":hit.quote_end_offset,"offset_unit":hit.offset_unit,
                "retrieval_contract_version":hit.retrieval_contract_version,"media":hit.media,
            }));
        }
    }
    let product_values=products.values().map(|version|serde_json::json!({
        "id":version.product_version_id,"product_id":version.product_id,"product_version_id":version.product_version_id,
        "workspace_kind":version.workspace_kind,"frozen_display_name":version.frozen_display_name,
        "identity_sha256":hex::encode(Sha256::digest(format!("ProductVersionEvidenceV1:{}:{}:{}",
            version.product_id,version.product_version_id,version.workspace_kind).as_bytes())),
    })).collect::<Vec<_>>();
    let mut workspace_kinds = products
        .values()
        .map(|version| version.workspace_kind.clone())
        .collect::<Vec<_>>();
    workspace_kinds.sort();
    workspace_kinds.dedup();
    let scope = serde_json::json!({
        "schema_version":2,"workspace_kinds":workspace_kinds,
        "version_selections":{"product_line":[],"company":[]},"products":product_values,
        "retrieval_requirements":requirement_values,"frozen_hits":frozen_hits,"retrieval_policy":policy,
    });
    let attested: serde_json::Value =
        sqlx::query_scalar("SELECT kb_knowledge_attest_matching_scope_v2($1)")
            .bind(&scope)
            .fetch_one(pool)
            .await
            .map_err(database_unavailable)?;
    let attestation_id = attested
        .get("id")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| {
            KnowledgeRetrievalError::InvalidHit("knowledge attestation id missing".into())
        })?;
    let attestation_sha256 = attested
        .get("content_sha256")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            KnowledgeRetrievalError::InvalidHit("knowledge attestation digest missing".into())
        })?
        .to_owned();
    Ok(AttestedEvidenceScopeV2 {
        attestation_id,
        attestation_sha256,
        canonical_scope: scope,
    })
}

fn exact_candidate_hit_v2(
    candidate: ExactCandidateV2,
    workspace_kind: &str,
    index: usize,
) -> KnowledgeEvidenceHitV3 {
    let chunk_byte_length = candidate.byte_length();
    KnowledgeEvidenceHitV3 {
        schema_version: KNOWLEDGE_EVIDENCE_SCHEMA_V3,
        document_id: candidate.document_id,
        source_chunk_id: candidate.source_chunk_id,
        product_id: candidate.product_id,
        product_version_id: candidate.product_version_id,
        workspace_kind: workspace_kind.to_string(),
        frozen_document_display_name: candidate.frozen_document_display_name,
        chunk_sha256: hex::encode(Sha256::digest(candidate.chunk_utf8.as_bytes())),
        chunk_utf8: candidate.chunk_utf8,
        chunk_byte_length,
        quote_start_offset: 0,
        quote_end_offset: chunk_byte_length,
        offset_unit: UTF8_BYTE_OFFSET_UNIT.to_string(),
        retrieval_rank: index as u32 + 1,
        retrieval_raw_score: "1.000000".into(),
        pre_rerank_rrf_rank: None,
        retrieval_contract_version: KNOWLEDGE_EVIDENCE_CONTRACT_V2.into(),
        source_type: candidate.source_type,
        media: None,
    }
}

fn append_semantic_tail_v2(
    hits: &mut Vec<KnowledgeEvidenceHitV3>,
    reranked: Vec<rerank_v2::RerankedSemanticCandidateV2>,
    workspace_kind: &str,
    policy: &RetrievalPolicyIdentityV1,
) -> u64 {
    let mut truncated = 0u64;
    let mut total_bytes = hits.iter().map(|hit| hit.chunk_byte_length).sum::<u64>();
    for reranked_candidate in reranked {
        let candidate = &reranked_candidate.candidate;
        let eligible = hits.len() < policy.max_hits as usize
            && candidate.chunk_byte_length <= u64::from(policy.max_chunk_bytes)
            && total_bytes
                .checked_add(candidate.chunk_byte_length)
                .is_some_and(|required| required <= policy.max_total_bytes);
        if !eligible {
            truncated = truncated.saturating_add(1);
            continue;
        }
        total_bytes += candidate.chunk_byte_length;
        hits.push(semantic_candidate_hit_v2(
            reranked_candidate,
            workspace_kind,
            hits.len(),
        ));
    }
    truncated
}

fn semantic_candidate_hit_v2(
    reranked: rerank_v2::RerankedSemanticCandidateV2,
    workspace_kind: &str,
    index: usize,
) -> KnowledgeEvidenceHitV3 {
    let score = reranked.fixed_score();
    let candidate = reranked.candidate;
    KnowledgeEvidenceHitV3 {
        schema_version: KNOWLEDGE_EVIDENCE_SCHEMA_V3,
        document_id: candidate.document_id,
        source_chunk_id: candidate.source_chunk_id,
        product_id: candidate.product_id,
        product_version_id: candidate.product_version_id,
        workspace_kind: workspace_kind.to_string(),
        frozen_document_display_name: candidate.frozen_document_display_name,
        chunk_sha256: candidate.chunk_sha256,
        chunk_utf8: candidate.chunk_utf8,
        chunk_byte_length: candidate.chunk_byte_length,
        quote_start_offset: 0,
        quote_end_offset: candidate.chunk_byte_length,
        offset_unit: UTF8_BYTE_OFFSET_UNIT.to_string(),
        retrieval_rank: index as u32 + 1,
        retrieval_raw_score: score,
        pre_rerank_rrf_rank: Some(candidate.pre_rerank_rrf_rank),
        retrieval_contract_version: KNOWLEDGE_EVIDENCE_CONTRACT_V2.into(),
        source_type: candidate.source_type,
        media: None,
    }
}

async fn hydrate_evidence_media_v3(
    tx: &mut Transaction<'_, Postgres>,
    hits: &mut [KnowledgeEvidenceHitV3],
) -> Result<(), KnowledgeRetrievalError> {
    let ids = hits
        .iter()
        .filter(|hit| hit.source_type == KnowledgeSourceTypeV2::ImageOcr)
        .map(|hit| hit.source_chunk_id)
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(());
    }
    let rows=sqlx::query("SELECT mapping.chunk_id,media.id image_artifact_revision_id,media.object_ref,
        media.content_sha256,media.media_type,media.width,media.height,media.page_ordinal,media.bounding_region
      FROM knowledge_image_ocr_chunk_artifact_mappings mapping
      JOIN knowledge_image_artifact_revisions media ON media.id=mapping.image_artifact_revision_id
        AND media.product_version_id=mapping.product_version_id AND media.document_id=mapping.document_id
        AND media.object_ref=mapping.object_ref AND media.content_sha256=mapping.content_sha256
        AND media.media_type=mapping.media_type AND media.object_state=mapping.object_state
      JOIN object_registry registry ON registry.object_ref=media.object_ref AND registry.digest=media.content_sha256
        AND registry.media_type=media.media_type AND registry.state=media.object_state
      WHERE mapping.chunk_id=ANY($1::uuid[])")
        .bind(&ids).fetch_all(&mut **tx).await.map_err(database_unavailable)?;
    let media = rows
        .into_iter()
        .map(|row| {
            let chunk_id: Uuid = row.get("chunk_id");
            let value = KnowledgeEvidenceMediaV1 {
                image_artifact_revision_id: row.get("image_artifact_revision_id"),
                object_ref: row.get("object_ref"),
                sha256: row.get("content_sha256"),
                media_type: row.get("media_type"),
                width: u32::try_from(row.get::<i32, _>("width")).unwrap_or_default(),
                height: u32::try_from(row.get::<i32, _>("height")).unwrap_or_default(),
                page_ordinal: row
                    .get::<Option<i32>, _>("page_ordinal")
                    .and_then(|value| u32::try_from(value).ok()),
                bounding_region: row.get("bounding_region"),
                frozen_document_display_name: String::new(),
            };
            (chunk_id, value)
        })
        .collect::<HashMap<_, _>>();
    for hit in hits {
        match hit.source_type {
            KnowledgeSourceTypeV2::ImageOcr => {
                let mut value = media.get(&hit.source_chunk_id).cloned().ok_or_else(|| {
                    KnowledgeRetrievalError::Unavailable(
                        "image OCR hit lacks immutable media mapping".into(),
                    )
                })?;
                value.frozen_document_display_name = hit.frozen_document_display_name.clone();
                hit.media = Some(value);
            }
            _ => hit.media = None,
        }
    }
    Ok(())
}

async fn lock_rerank_revision_v2(
    tx: &mut Transaction<'_, Postgres>,
    policy: &RetrievalPolicyV2,
) -> Result<(RerankRevisionV2, String), KnowledgeRetrievalError> {
    let row = sqlx::query(
        "SELECT canonical_revision_payload,credential_ref
           FROM public.kb_knowledge_lock_rerank_revision_v2($1)",
    )
    .bind(&policy.rerank.revision_sha256)
    .fetch_optional(&mut **tx)
    .await
    .map_err(database_unavailable)?
    .ok_or_else(|| {
        KnowledgeRetrievalError::InvalidRequest("unknown or revoked rerank revision v2".into())
    })?;
    let payload = row.get::<Vec<u8>, _>("canonical_revision_payload");
    let revision = serde_json::from_slice::<RerankRevisionV2>(&payload).map_err(|error| {
        KnowledgeRetrievalError::InvalidRequest(format!(
            "invalid rerank revision v2 artifact: {error}"
        ))
    })?;
    revision.validate().map_err(|error| {
        KnowledgeRetrievalError::InvalidRequest(format!(
            "invalid rerank revision v2 artifact: {error}"
        ))
    })?;
    if revision.canonical_bytes().map_err(|error| {
        KnowledgeRetrievalError::InvalidRequest(format!(
            "invalid rerank revision v2 artifact: {error}"
        ))
    })? != payload
        || revision.sha256().ok().as_deref() != Some(policy.rerank.revision_sha256.as_str())
        || revision.provider_model_revision_sha256 != policy.rerank.model_revision_sha256
        || revision.config_revision_sha256 != policy.rerank.config_revision_sha256
    {
        return Err(KnowledgeRetrievalError::InvalidRequest(
            "non-canonical or mismatched rerank revision v2 artifact".into(),
        ));
    }
    Ok((revision, row.get("credential_ref")))
}

fn database_unavailable(error: sqlx::Error) -> KnowledgeRetrievalError {
    KnowledgeRetrievalError::Unavailable(error.to_string())
}

fn validate_request_v2(
    request_schema_version: u16,
    requirement_sha256: &str,
    requirement_text: &str,
    selected_versions: &[Uuid],
    policy: &RetrievalPolicyIdentityV1,
) -> Result<(), KnowledgeRetrievalError> {
    let selected_version_ids = selected_versions.iter().copied().collect::<HashSet<_>>();
    if request_schema_version != KNOWLEDGE_EVIDENCE_SCHEMA_V1
        || policy.contract_version != KNOWLEDGE_EVIDENCE_CONTRACT_V2
        || !is_sha256(requirement_sha256)
        || !is_sha256(&policy.policy_sha256)
        || normalize_exact_v2(requirement_text).is_empty()
        || policy.max_hits == 0
        || policy.max_hits > V2_MAX_HITS
        || policy.max_chunk_bytes == 0
        || policy.max_chunk_bytes > V2_MAX_CHUNK_BYTES
        || policy.max_total_bytes == 0
        || policy.max_total_bytes > V2_MAX_TOTAL_BYTES
        || selected_version_ids.len() != selected_versions.len()
        || selected_version_ids.contains(&Uuid::nil())
    {
        return Err(KnowledgeRetrievalError::InvalidRequest(
            "invalid v2 evidence scope, policy, or quota".into(),
        ));
    }
    Ok(())
}

fn validate_request(
    requirement_sha256: &str,
    requirement_text: &str,
    contract_version: &str,
    policy_sha256: &str,
    max_hits: u32,
    max_chunk_bytes: u32,
    max_total_bytes: u64,
) -> Result<(), KnowledgeRetrievalError> {
    if !is_sha256(requirement_sha256)
        || !is_sha256(policy_sha256)
        || requirement_text.trim().is_empty()
        || contract_version.is_empty()
        || max_hits == 0
        || max_hits > ABSOLUTE_MAX_HITS
        || max_chunk_bytes == 0
        || max_chunk_bytes > ABSOLUTE_MAX_CHUNK_BYTES
        || max_total_bytes == 0
        || max_total_bytes > ABSOLUTE_MAX_TOTAL_BYTES
    {
        return Err(KnowledgeRetrievalError::InvalidRequest(
            "invalid evidence scope, policy, or quota".into(),
        ));
    }
    Ok(())
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn fixed_six(value: Decimal) -> String {
    let mut value = value.round_dp_with_strategy(6, RoundingStrategy::MidpointNearestEven);
    value.rescale(6);
    format!("{value:.6}")
}

fn lexical_score(requirement: &str, content: &str) -> Decimal {
    let requirement = requirement.trim().to_lowercase();
    let content = content.to_lowercase();
    if requirement.is_empty() || content.is_empty() {
        return Decimal::ZERO;
    }
    if content.contains(&requirement) {
        return Decimal::ONE;
    }
    let required = lexical_terms(&requirement);
    if required.is_empty() {
        return Decimal::ZERO;
    }
    let available = lexical_terms(&content);
    let matched = required
        .iter()
        .filter(|term| available.contains(*term))
        .count();
    Decimal::from(matched as u64) / Decimal::from(required.len() as u64)
}

fn lexical_terms(value: &str) -> HashSet<String> {
    let words: Vec<String> = value
        .split_whitespace()
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    if words.len() > 1 {
        return words.into_iter().collect();
    }
    let chars: Vec<char> = value.chars().filter(|ch| !ch.is_whitespace()).collect();
    chars
        .windows(2)
        .map(|window| window.iter().collect::<String>())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_dispatch_accepts_only_v1_lexical_retrieval() {
        assert_eq!(
            dispatch_contract(KNOWLEDGE_EVIDENCE_CONTRACT_V1).unwrap(),
            RetrievalContract::LexicalV1
        );

        let v2_error = dispatch_contract(KNOWLEDGE_EVIDENCE_CONTRACT_V2).unwrap_err();
        assert!(matches!(
            &v2_error,
            KnowledgeRetrievalError::InvalidRequest(_)
        ));
        assert!(
            v2_error
                .to_string()
                .contains("not available until the v2 implementation")
        );

        let unknown_error = dispatch_contract("knowledge-evidence-v99").unwrap_err();
        assert!(matches!(
            &unknown_error,
            KnowledgeRetrievalError::InvalidRequest(_)
        ));
        assert!(
            unknown_error
                .to_string()
                .contains("unsupported knowledge evidence contract: knowledge-evidence-v99")
        );
    }

    #[test]
    fn chinese_lexical_score_is_nonzero_without_whitespace() {
        assert!(lexical_score("支持国密算法", "设备完整支持国密算法套件") > Decimal::ZERO);
    }

    fn candidate(
        product: u128,
        version: u128,
        document: u128,
        chunk_id: u128,
        content: &str,
    ) -> ExactCandidateV2 {
        ExactCandidateV2 {
            document_id: Uuid::from_u128(document),
            source_chunk_id: Uuid::from_u128(chunk_id),
            product_id: Uuid::from_u128(product),
            product_version_id: Uuid::from_u128(version),
            frozen_document_display_name: format!("{document}.txt"),
            chunk_utf8: content.into(),
            source_type: KnowledgeSourceTypeV2::Text,
        }
    }

    fn exact_policy(
        max_hits: u32,
        max_chunk_bytes: u32,
        max_total_bytes: u64,
    ) -> RetrievalPolicyIdentityV1 {
        RetrievalPolicyIdentityV1 {
            contract_version: KNOWLEDGE_EVIDENCE_CONTRACT_V2.into(),
            policy_sha256: "a".repeat(64),
            max_hits,
            max_chunk_bytes,
            max_total_bytes,
        }
    }

    #[test]
    fn exact_normalization_matches_verifier_for_chinese_whitespace_and_case() {
        assert_eq!(normalize_exact_v2(" 支\t持\n国 密 "), "支持国密");
        assert_eq!(normalize_exact_v2(" A中\u{2003}Bc "), "a中bc");
        assert!(normalize_exact_v2("设备支持 国密算法").contains(&normalize_exact_v2("支 持国密")));
    }

    #[test]
    fn exact_selection_is_deterministic_for_sizes_ties_and_dense_final_sequence() {
        let input = vec![
            candidate(1, 10, 30, 300, "aa"),
            candidate(1, 10, 20, 201, "aa"),
            candidate(1, 10, 20, 200, "aa"),
            candidate(2, 20, 40, 400, "z"),
        ];
        let first = select_exact_prefix_v2(input.clone(), &exact_policy(4, 10, 100)).unwrap();
        let second =
            select_exact_prefix_v2(input.into_iter().rev().collect(), &exact_policy(4, 10, 100))
                .unwrap();
        assert_eq!(first, second);
        assert_eq!(
            first
                .hits
                .iter()
                .map(|hit| hit.source_chunk_id)
                .collect::<Vec<_>>(),
            vec![
                Uuid::from_u128(200),
                Uuid::from_u128(400),
                Uuid::from_u128(201),
                Uuid::from_u128(300)
            ]
        );
        assert_eq!(
            first
                .hits
                .iter()
                .enumerate()
                .map(|(index, _)| index + 1)
                .collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
    }

    #[test]
    fn exact_selection_v_greater_than_k_preserves_fair_prefix_and_metrics() {
        let selection = select_exact_prefix_v2(
            vec![
                candidate(3, 30, 30, 300, "x"),
                candidate(1, 10, 12, 102, "xx"),
                candidate(1, 10, 11, 101, "x"),
                candidate(2, 20, 20, 200, "x"),
            ],
            &exact_policy(2, 10, 100),
        )
        .unwrap();
        assert_eq!(selection.exact_versions_truncated, 1);
        assert_eq!(selection.exact_hits_truncated, 2);
        assert_eq!(
            selection
                .hits
                .iter()
                .map(|hit| hit.product_version_id)
                .collect::<Vec<_>>(),
            vec![Uuid::from_u128(10), Uuid::from_u128(20)]
        );
    }

    #[test]
    fn exact_selection_b_skips_count_byte_and_oversized_candidates() {
        let candidates = vec![
            candidate(1, 10, 10, 100, "x"),
            candidate(1, 10, 10, 101, "yy"),
            candidate(1, 10, 10, 102, "zzz"),
            candidate(1, 10, 10, 103, "oversized"),
        ];
        let byte_limited =
            select_exact_prefix_v2(candidates.clone(), &exact_policy(4, 3, 3)).unwrap();
        assert_eq!(byte_limited.hits.len(), 2);
        assert_eq!(byte_limited.exact_hits_truncated, 2);
        let count_limited = select_exact_prefix_v2(candidates, &exact_policy(2, 20, 100)).unwrap();
        assert_eq!(count_limited.hits.len(), 2);
        assert_eq!(count_limited.exact_hits_truncated, 2);
    }

    #[test]
    fn exact_selection_fails_closed_for_primary_chunk_or_total_bytes() {
        let oversized = select_exact_prefix_v2(
            vec![candidate(1, 10, 10, 100, "large")],
            &exact_policy(1, 4, 100),
        );
        assert!(matches!(
            oversized,
            Err(KnowledgeRetrievalError::QuotaExceeded(_))
        ));

        let total = select_exact_prefix_v2(
            vec![
                candidate(1, 10, 10, 100, "aaa"),
                candidate(2, 20, 20, 200, "bbb"),
            ],
            &exact_policy(2, 10, 5),
        );
        assert!(matches!(
            total,
            Err(KnowledgeRetrievalError::QuotaExceeded(_))
        ));

        let empty = select_exact_prefix_v2(Vec::new(), &exact_policy(2, 10, 10)).unwrap();
        assert!(empty.hits.is_empty());
        assert_eq!(empty.exact_versions_truncated, 0);
        assert_eq!(empty.exact_hits_truncated, 0);
    }

    fn reranked_candidate(
        id: u128,
        content: &str,
        rank: u32,
    ) -> rerank_v2::RerankedSemanticCandidateV2 {
        rerank_v2::RerankedSemanticCandidateV2 {
            candidate: semantic_v2::SemanticCandidateV2 {
                document_id: Uuid::from_u128(id + 100),
                source_chunk_id: Uuid::from_u128(id),
                product_id: Uuid::from_u128(id + 200),
                product_version_id: Uuid::from_u128(id + 300),
                frozen_document_display_name: "manual.pdf".into(),
                chunk_utf8: content.into(),
                chunk_sha256: hex::encode(Sha256::digest(content.as_bytes())),
                chunk_byte_length: content.len() as u64,
                source_type: KnowledgeSourceTypeV2::Text,
                vector_rank: Some(rank),
                keyword_rank: None,
                exact_rrf_score: semantic_v2::ExactRationalV2 {
                    numerator: 1,
                    denominator: u128::from(rank + 60),
                },
                pre_rerank_rrf_rank: rank,
            },
            score_millionths: 500_000,
        }
    }

    #[test]
    fn semantic_tail_skips_and_counts_count_chunk_and_total_quota_excess() {
        let mut hits = Vec::new();
        let chunk_policy = exact_policy(3, 3, 100);
        let truncated = append_semantic_tail_v2(
            &mut hits,
            vec![
                reranked_candidate(1, "wide", 1),
                reranked_candidate(2, "ok", 2),
            ],
            "product_line",
            &chunk_policy,
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(truncated, 1);

        let mut hits = Vec::new();
        let total_policy = exact_policy(3, 10, 3);
        let truncated = append_semantic_tail_v2(
            &mut hits,
            vec![
                reranked_candidate(3, "aa", 1),
                reranked_candidate(4, "bb", 2),
            ],
            "product_line",
            &total_policy,
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(truncated, 1);

        let mut hits = Vec::new();
        let count_policy = exact_policy(1, 10, 100);
        let truncated = append_semantic_tail_v2(
            &mut hits,
            vec![reranked_candidate(5, "a", 1), reranked_candidate(6, "b", 2)],
            "product_line",
            &count_policy,
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(truncated, 1);
    }

    #[test]
    fn utf8_snapshot_validator_rejects_non_boundary_offsets() {
        let chunk = "中A";
        let hit = KnowledgeEvidenceHitV1 {
            schema_version: 1,
            document_id: Uuid::new_v4(),
            source_chunk_id: Uuid::new_v4(),
            product_id: Uuid::new_v4(),
            product_version_id: Uuid::new_v4(),
            workspace_kind: "product_line".into(),
            frozen_document_display_name: "手册.pdf".into(),
            chunk_utf8: chunk.into(),
            chunk_sha256: hex::encode(Sha256::digest(chunk.as_bytes())),
            chunk_byte_length: chunk.len() as u64,
            quote_start_offset: 1,
            quote_end_offset: chunk.len() as u64,
            offset_unit: UTF8_BYTE_OFFSET_UNIT.into(),
            retrieval_rank: 1,
            retrieval_raw_score: "1.000000".into(),
            retrieval_contract_version: KNOWLEDGE_EVIDENCE_CONTRACT_V1.into(),
        };
        let policy = RetrievalPolicyIdentityV1 {
            contract_version: KNOWLEDGE_EVIDENCE_CONTRACT_V1.into(),
            policy_sha256: "0".repeat(64),
            max_hits: 1,
            max_chunk_bytes: 1024,
            max_total_bytes: 1024,
        };
        let batch = KnowledgeEvidenceBatchV1 {
            schema_version: KNOWLEDGE_EVIDENCE_SCHEMA_V1,
            eligible_versions: vec![EligibleEvidenceVersionV1 {
                product_id: hit.product_id,
                product_version_id: hit.product_version_id,
                workspace_kind: "product_line".into(),
                frozen_display_name: hit.product_version_id.to_string(),
            }],
            hits: vec![hit],
        };
        assert!(validate_evidence_batch("product_line", &batch, &policy).is_err());
    }
}
