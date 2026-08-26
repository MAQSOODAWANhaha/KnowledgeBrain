//! PostgreSQL adapter for the knowledge-base-owned `KnowledgeRetrievalPort`.
//!
//! This is the only matching-facing implementation allowed to inspect live
//! knowledge rows.  A single query returns complete document/chunk snapshots;
//! callers receive owned DTOs and must freeze them immediately.

use async_trait::async_trait;
use domain::knowledge_retrieval::{
    CompanyEvidenceRequestV1, EligibleEvidenceVersionV1, KNOWLEDGE_EVIDENCE_CONTRACT_V1,
    KNOWLEDGE_EVIDENCE_CONTRACT_V2, KNOWLEDGE_EVIDENCE_SCHEMA_V1, KNOWLEDGE_EVIDENCE_SCHEMA_V2,
    KnowledgeEvidenceBatchV1, KnowledgeEvidenceBatchV2, KnowledgeEvidenceHitV1,
    KnowledgeEvidenceHitV2, KnowledgeEvidenceScopeV2, KnowledgeRetrievalError,
    KnowledgeRetrievalPort, KnowledgeRetrievalPortV2, KnowledgeSourceTypeV2,
    ProductEvidenceRequestV1, RetrievalPolicyIdentityV1, UTF8_BYTE_OFFSET_UNIT,
    validate_evidence_batch, validate_evidence_batch_v2,
};
use rust_decimal::{Decimal, RoundingStrategy};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use std::{collections::BTreeMap, collections::HashSet};
use uuid::Uuid;

const ABSOLUTE_MAX_HITS: u32 = 256;
const ABSOLUTE_MAX_CHUNK_BYTES: u32 = 1_048_576;
const ABSOLUTE_MAX_TOTAL_BYTES: u64 = 16 * 1_048_576;
const V2_MAX_HITS: u32 = 1_000_000;
const V2_MAX_CHUNK_BYTES: u32 = 1_073_741_824;
const V2_MAX_TOTAL_BYTES: u64 = 1_099_511_627_776;

#[derive(Clone)]
pub struct PostgresKnowledgeRetrievalAdapter {
    pool: PgPool,
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
        Self { pool }
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
    ) -> Result<KnowledgeEvidenceBatchV2, KnowledgeRetrievalError> {
        validate_request_v2(
            request_schema_version,
            requirement_identity_sha256,
            requirement_text,
            selected_versions,
            policy,
        )?;
        self.validate_supported_policy_v2(policy).await?;

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
              WHERE w.kind=$1 AND pv.status='active' AND pv.deleted_at IS NULL
                AND (($1='product_line' AND p.kind='product') OR ($1='company' AND p.kind='library'))
                AND (cardinality($2::uuid[])=0 OR pv.id=ANY($2::uuid[]))
              ORDER BY p.id,pv.id,d.id,c.id",
        )
        .bind(workspace_kind)
        .bind(selected_versions)
        .bind(trusted_types)
        .fetch_all(&self.pool)
        .await
        .map_err(|error| KnowledgeRetrievalError::Unavailable(error.to_string()))?;

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
        let hits = selection
            .hits
            .into_iter()
            .enumerate()
            .map(|(index, candidate)| {
                let chunk_byte_length = candidate.byte_length();
                KnowledgeEvidenceHitV2 {
                    schema_version: KNOWLEDGE_EVIDENCE_SCHEMA_V2,
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
                    retrieval_contract_version: KNOWLEDGE_EVIDENCE_CONTRACT_V2.into(),
                    source_type: candidate.source_type,
                }
            })
            .collect();
        let batch = KnowledgeEvidenceBatchV2 {
            schema_version: KNOWLEDGE_EVIDENCE_SCHEMA_V2,
            eligible_versions: eligible_versions.into_values().collect(),
            hits,
            exact_versions_truncated: selection.exact_versions_truncated,
            exact_hits_truncated: selection.exact_hits_truncated,
            semantic_hits_truncated: 0,
        };
        validate_evidence_batch_v2(workspace_kind, &batch, policy)?;
        Ok(batch)
    }

    async fn validate_supported_policy_v2(
        &self,
        policy: &RetrievalPolicyIdentityV1,
    ) -> Result<(), KnowledgeRetrievalError> {
        let row = sqlx::query(
            "SELECT contract_version,max_hits,max_chunk_bytes,max_total_bytes,support_state
               FROM knowledge_retrieval_policies_v2
              WHERE policy_sha256=$1",
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
        Ok(())
    }
}

#[async_trait]
impl KnowledgeRetrievalPortV2 for PostgresKnowledgeRetrievalAdapter {
    async fn retrieve_evidence_v2(
        &self,
        scope: KnowledgeEvidenceScopeV2,
    ) -> Result<KnowledgeEvidenceBatchV2, KnowledgeRetrievalError> {
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
