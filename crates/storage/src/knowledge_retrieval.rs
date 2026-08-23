//! PostgreSQL adapter for the knowledge-base-owned `KnowledgeRetrievalPort`.
//!
//! This is the only matching-facing implementation allowed to inspect live
//! knowledge rows.  A single query returns complete document/chunk snapshots;
//! callers receive owned DTOs and must freeze them immediately.

use async_trait::async_trait;
use domain::knowledge_retrieval::{
    CompanyEvidenceHitV1, CompanyEvidenceRequestV1, KNOWLEDGE_EVIDENCE_SCHEMA_V1,
    KnowledgeEvidenceHitV1, KnowledgeRetrievalError, KnowledgeRetrievalPort, ProductEvidenceHitV1,
    ProductEvidenceRequestV1, RetrievalPolicyIdentityV1, UTF8_BYTE_OFFSET_UNIT,
};
use rust_decimal::{Decimal, RoundingStrategy};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use std::{collections::HashSet, str::FromStr};
use uuid::Uuid;

const ABSOLUTE_MAX_HITS: u32 = 256;
const ABSOLUTE_MAX_CHUNK_BYTES: u32 = 1_048_576;
const ABSOLUTE_MAX_TOTAL_BYTES: u64 = 16 * 1_048_576;

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

impl PostgresKnowledgeRetrievalAdapter {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    async fn retrieve(
        &self,
        query: RetrievalQuery<'_>,
    ) -> Result<Vec<KnowledgeEvidenceHitV1>, KnowledgeRetrievalError> {
        let RetrievalQuery {
            workspace_kind,
            requirement_identity_sha256,
            requirement_text,
            selected_versions,
            policy,
        } = query;
        validate_request(
            requirement_identity_sha256,
            requirement_text,
            &policy.contract_version,
            &policy.policy_sha256,
            policy.max_hits,
            policy.max_chunk_bytes,
            policy.max_total_bytes,
        )?;
        let rows = sqlx::query(
            "SELECT c.id AS source_chunk_id,c.content,d.id AS document_id,d.file_name,
                    p.id AS product_id,pv.id AS product_version_id,w.kind AS workspace_kind
               FROM workspaces w
               JOIN products p ON p.workspace_id=w.id
               JOIN product_versions pv ON pv.product_id=p.id AND p.current_version_id=pv.id
               JOIN documents d ON d.product_version_id=pv.id
               JOIN chunks c ON c.document_id=d.id AND c.product_version_id=pv.id
              WHERE w.kind=$1 AND pv.status='active' AND pv.deleted_at IS NULL
                AND d.deleted_at IS NULL AND d.enable_status='enabled' AND d.index_ready
                AND (($1='product_line' AND p.kind='product') OR ($1='company' AND p.kind='library'))
                AND (cardinality($2::uuid[])=0 OR pv.id=ANY($2::uuid[]))
                AND octet_length(convert_to(c.content,'UTF8')) <= $3
              ORDER BY p.id,pv.id,d.id,c.id",
        )
        .bind(workspace_kind)
        .bind(selected_versions)
        .bind(i64::from(policy.max_chunk_bytes))
        .fetch_all(&self.pool)
        .await
        .map_err(|error| KnowledgeRetrievalError::Unavailable(error.to_string()))?;

        let mut ranked = Vec::new();
        for row in rows {
            let chunk_utf8: String = row.get("content");
            let raw_score = lexical_score(requirement_text, &chunk_utf8);
            ranked.push((
                raw_score,
                row.get::<Uuid, _>("product_id"),
                row.get::<Uuid, _>("product_version_id"),
                row.get::<Uuid, _>("document_id"),
                row.get::<Uuid, _>("source_chunk_id"),
                row.get::<String, _>("file_name"),
                chunk_utf8,
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

        // Preserve complete eligible version membership, including a version
        // whose best lexical score is zero. Additional ranked chunks may fill
        // the remaining hit quota. A scope that cannot fit is rejected rather
        // than silently truncated.
        let version_count = ranked.iter().map(|row| row.2).collect::<HashSet<_>>().len();
        if version_count > policy.max_hits as usize {
            return Err(KnowledgeRetrievalError::InvalidRequest(
                "eligible version scope exceeds retrieval hit quota".into(),
            ));
        }
        let mut chosen = Vec::new();
        let mut represented = HashSet::new();
        for row in &ranked {
            if represented.insert(row.2) {
                chosen.push(row.clone());
            }
        }
        let mut chosen_chunks: HashSet<Uuid> = chosen.iter().map(|row| row.4).collect();
        for row in ranked {
            if chosen.len() >= policy.max_hits as usize {
                break;
            }
            if chosen_chunks.insert(row.4) {
                chosen.push(row);
            }
        }
        chosen.sort_by(|left, right| {
            right
                .0
                .cmp(&left.0)
                .then(left.1.cmp(&right.1))
                .then(left.2.cmp(&right.2))
                .then(left.3.cmp(&right.3))
                .then(left.4.cmp(&right.4))
        });

        let required_total = chosen.iter().map(|row| row.6.len() as u64).sum::<u64>();
        if required_total > policy.max_total_bytes {
            return Err(KnowledgeRetrievalError::InvalidRequest(
                "eligible version scope exceeds retrieval byte quota".into(),
            ));
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
        validate_hits(
            workspace_kind,
            &hits,
            policy.max_hits,
            policy.max_chunk_bytes,
            policy.max_total_bytes,
        )?;
        Ok(hits)
    }
}

#[async_trait]
impl KnowledgeRetrievalPort for PostgresKnowledgeRetrievalAdapter {
    async fn retrieve_product_evidence(
        &self,
        request: ProductEvidenceRequestV1,
    ) -> Result<Vec<ProductEvidenceHitV1>, KnowledgeRetrievalError> {
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
        .map(|hits| hits.into_iter().map(ProductEvidenceHitV1).collect())
    }

    async fn retrieve_company_evidence(
        &self,
        request: CompanyEvidenceRequestV1,
    ) -> Result<Vec<CompanyEvidenceHitV1>, KnowledgeRetrievalError> {
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
        .map(|hits| hits.into_iter().map(CompanyEvidenceHitV1).collect())
    }
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

fn validate_hits(
    workspace_kind: &str,
    hits: &[KnowledgeEvidenceHitV1],
    max_hits: u32,
    max_chunk_bytes: u32,
    max_total_bytes: u64,
) -> Result<(), KnowledgeRetrievalError> {
    let mut total = 0u64;
    let mut seen = HashSet::new();
    if hits.len() > max_hits as usize {
        return Err(KnowledgeRetrievalError::InvalidHit(
            "hit quota exceeded".into(),
        ));
    }
    for (index, hit) in hits.iter().enumerate() {
        let bytes = hit.chunk_utf8.as_bytes();
        total = total.saturating_add(bytes.len() as u64);
        if hit.schema_version != KNOWLEDGE_EVIDENCE_SCHEMA_V1
            || hit.workspace_kind != workspace_kind
            || hit.offset_unit != UTF8_BYTE_OFFSET_UNIT
            || hit.chunk_byte_length != bytes.len() as u64
            || hit.chunk_byte_length > u64::from(max_chunk_bytes)
            || hit.chunk_sha256 != hex::encode(Sha256::digest(bytes))
            || hit.quote_start_offset >= hit.quote_end_offset
            || hit.quote_end_offset > hit.chunk_byte_length
            || !hit
                .chunk_utf8
                .is_char_boundary(hit.quote_start_offset as usize)
            || !hit
                .chunk_utf8
                .is_char_boundary(hit.quote_end_offset as usize)
            || hit.retrieval_rank != index as u32 + 1
            || Decimal::from_str(&hit.retrieval_raw_score).is_err()
            || !seen.insert((
                hit.document_id,
                hit.source_chunk_id,
                hit.quote_start_offset,
                hit.quote_end_offset,
            ))
        {
            return Err(KnowledgeRetrievalError::InvalidHit(
                "evidence bytes, digest, offset, rank, or identity is invalid".into(),
            ));
        }
    }
    if total > max_total_bytes {
        return Err(KnowledgeRetrievalError::InvalidHit(
            "byte quota exceeded".into(),
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
    fn chinese_lexical_score_is_nonzero_without_whitespace() {
        assert!(lexical_score("支持国密算法", "设备完整支持国密算法套件") > Decimal::ZERO);
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
            retrieval_contract_version: "knowledge-evidence-v1".into(),
        };
        assert!(validate_hits("product_line", &[hit], 1, 1024, 1024).is_err());
    }
}
