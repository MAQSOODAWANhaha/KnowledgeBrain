use super::{
    PostgresKnowledgeRetrievalAdapter, RetrievalPolicyIdentityV1, RetrievalPolicyV2,
    ValidatedSemanticPolicyV2, normalize_exact_v2,
};
use crate::knowledge_retrieval::{
    EmbeddingRevisionV2, KnowledgeRetrievalError, KnowledgeSourceTypeV2,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Row, Transaction};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    sync::Arc,
    time::Duration,
};
use uuid::Uuid;

const EMBEDDING_DIMENSION_V2: usize = 1024;
const MILLION: u128 = 1_000_000;
const SIGNAL_TYPES: [&str; 8] = [
    "text",
    "parent_text",
    "image_ocr",
    "question",
    "summary",
    "image_caption",
    "graph_node",
    "wiki_page",
];

#[derive(Clone, Debug)]
pub(crate) struct QueryEmbeddingV2 {
    revision_sha256: String,
    values: Vec<f32>,
}

#[async_trait]
pub(crate) trait EmbeddingCredentialResolverV2: Send + Sync {
    async fn resolve(&self, credential_ref: &str) -> Result<String, KnowledgeRetrievalError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct EnvironmentEmbeddingCredentialResolverV2;

#[async_trait]
impl EmbeddingCredentialResolverV2 for EnvironmentEmbeddingCredentialResolverV2 {
    async fn resolve(&self, credential_ref: &str) -> Result<String, KnowledgeRetrievalError> {
        let variable = credential_ref.strip_prefix("env:").ok_or_else(|| {
            invalid("embedding/rerank credential reference must use env:<VARIABLE_NAME>")
        })?;
        if variable.is_empty()
            || variable.len() > 128
            || !variable
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        {
            return Err(invalid(
                "embedding/rerank credential environment name is invalid",
            ));
        }
        std::env::var(variable).map_err(|_| integrity("embedding/rerank credential is unavailable"))
    }
}

/// Strict semantic-v2 embedding transport. Endpoint, model, dimensions, and
/// request shape always come from the validated immutable registry revision.
#[derive(Clone)]
pub(crate) struct StrictEmbeddingClientV2 {
    http: reqwest::Client,
    credentials: Arc<dyn EmbeddingCredentialResolverV2>,
}

#[allow(dead_code)]
impl StrictEmbeddingClientV2 {
    pub(crate) fn new(
        credentials: Arc<dyn EmbeddingCredentialResolverV2>,
    ) -> Result<Self, KnowledgeRetrievalError> {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|error| integrity(format!("failed to configure embedding client: {error}")))?;
        Ok(Self { http, credentials })
    }

    pub(crate) async fn embed(
        &self,
        requirement_text: &str,
        validated: &ValidatedSemanticPolicyV2,
    ) -> Result<QueryEmbeddingV2, KnowledgeRetrievalError> {
        if !validated.revision.endpoint_identity.starts_with("https://") {
            return Err(invalid("embedding endpoint must use https"));
        }
        let credential = self
            .credentials
            .resolve(&validated.credential_ref)
            .await
            .map_err(|error| match error {
                KnowledgeRetrievalError::InvalidRequest(_)
                | KnowledgeRetrievalError::Unavailable(_) => error,
                KnowledgeRetrievalError::QuotaExceeded(_)
                | KnowledgeRetrievalError::InvalidHit(_) => {
                    invalid("embedding credential resolver returned an invalid error variant")
                }
            })?;
        if credential.is_empty() {
            return Err(invalid(
                "embedding credential resolution returned an empty credential",
            ));
        }
        let response = self
            .http
            .post(&validated.revision.endpoint_identity)
            .bearer_auth(credential)
            .json(&serde_json::json!({
                "model": validated.revision.provider_model_identifier,
                "input": [requirement_text],
                "dimensions": EMBEDDING_DIMENSION_V2,
                "request_config_sha256": validated.revision.request_config_sha256,
            }))
            .send()
            .await
            .map_err(|error| integrity(format!("embedding provider request failed: {error}")))?;
        if !response.status().is_success() {
            return Err(embedding_status_error(response.status()));
        }
        let response = super::http_v2::read_bounded_response_body_v2(
            response,
            super::http_v2::STRICT_V2_MAX_RESPONSE_BYTES,
        )
        .await
        .map_err(|error| match error {
            super::http_v2::BoundedBodyErrorV2::TooLarge => {
                integrity("embedding provider response exceeds byte limit")
            }
            super::http_v2::BoundedBodyErrorV2::Transport(error) => {
                integrity(format!("embedding provider response failed: {error}"))
            }
        })?;
        parse_embedding_response(&response.bytes, &validated.revision)
    }
}

#[derive(Deserialize)]
struct EmbeddingResponseV2 {
    data: Vec<EmbeddingDataV2>,
    model: String,
    model_revision_sha256: String,
    request_config_sha256: String,
}

#[derive(Deserialize)]
struct EmbeddingDataV2 {
    index: u32,
    embedding: Vec<f32>,
}

fn embedding_status_error(status: reqwest::StatusCode) -> KnowledgeRetrievalError {
    let message = format!("embedding provider returned status {status}");
    if status == reqwest::StatusCode::REQUEST_TIMEOUT
        || status == reqwest::StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
    {
        integrity(&message)
    } else {
        invalid(&message)
    }
}

fn parse_embedding_response(
    bytes: &[u8],
    revision: &EmbeddingRevisionV2,
) -> Result<QueryEmbeddingV2, KnowledgeRetrievalError> {
    let response: EmbeddingResponseV2 = serde_json::from_slice(bytes)
        .map_err(|error| integrity(format!("invalid embedding provider JSON: {error}")))?;
    if response.model != revision.provider_model_identifier
        || response.model_revision_sha256 != revision.provider_model_revision_sha256
        || response.request_config_sha256 != revision.request_config_sha256
    {
        return Err(invalid("embedding provider response identity mismatch"));
    }
    if response.data.len() != 1 || response.data[0].index != 0 {
        return Err(integrity(
            "embedding provider response must contain exactly data index 0",
        ));
    }
    let Some(data) = response.data.into_iter().next() else {
        return Err(integrity("embedding provider response data disappeared"));
    };
    let embedding = QueryEmbeddingV2 {
        revision_sha256: revision
            .sha256()
            .map_err(|error| invalid(&format!("invalid embedding revision: {error}")))?,
        values: data.embedding,
    };
    validate_query_embedding_values(&embedding)
        .map_err(|_| integrity("embedding provider returned an invalid vector"))?;
    Ok(embedding)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ExactRationalV2 {
    pub(crate) numerator: u128,
    pub(crate) denominator: u128,
}

impl ExactRationalV2 {
    fn new(numerator: u128, denominator: u128) -> Result<Self, KnowledgeRetrievalError> {
        if denominator == 0 {
            return Err(integrity("zero RRF denominator"));
        }
        let divisor = gcd(numerator, denominator);
        Ok(Self {
            numerator: numerator / divisor,
            denominator: denominator / divisor,
        })
    }

    fn channel(weight_millionths: u32, k: u32, rank: u32) -> Result<Self, KnowledgeRetrievalError> {
        let rank_denominator = u128::from(k)
            .checked_add(u128::from(rank))
            .ok_or_else(|| integrity("RRF rank denominator overflow"))?;
        let denominator = MILLION
            .checked_mul(rank_denominator)
            .ok_or_else(|| integrity("RRF weight denominator overflow"))?;
        Self::new(u128::from(weight_millionths), denominator)
    }

    fn add(self, other: Self) -> Result<Self, KnowledgeRetrievalError> {
        let left = self
            .numerator
            .checked_mul(other.denominator)
            .ok_or_else(|| integrity("RRF numerator overflow"))?;
        let right = other
            .numerator
            .checked_mul(self.denominator)
            .ok_or_else(|| integrity("RRF numerator overflow"))?;
        let numerator = left
            .checked_add(right)
            .ok_or_else(|| integrity("RRF numerator overflow"))?;
        let denominator = self
            .denominator
            .checked_mul(other.denominator)
            .ok_or_else(|| integrity("RRF denominator overflow"))?;
        Self::new(numerator, denominator)
    }

    fn cmp_exact(&self, other: &Self) -> Ordering {
        compare_fractions_without_products(
            self.numerator,
            self.denominator,
            other.numerator,
            other.denominator,
        )
    }
}

fn compare_fractions_without_products(
    mut left_numerator: u128,
    mut left_denominator: u128,
    mut right_numerator: u128,
    mut right_denominator: u128,
) -> Ordering {
    let mut inverted = false;
    loop {
        let left_integer = left_numerator / left_denominator;
        let right_integer = right_numerator / right_denominator;
        if left_integer != right_integer {
            let ordering = left_integer.cmp(&right_integer);
            return if inverted {
                ordering.reverse()
            } else {
                ordering
            };
        }
        let left_remainder = left_numerator % left_denominator;
        let right_remainder = right_numerator % right_denominator;
        if left_remainder == 0 || right_remainder == 0 {
            let ordering = left_remainder.cmp(&right_remainder);
            return if inverted {
                ordering.reverse()
            } else {
                ordering
            };
        }
        left_numerator = left_denominator;
        left_denominator = left_remainder;
        right_numerator = right_denominator;
        right_denominator = right_remainder;
        inverted = !inverted;
    }
}

fn gcd(mut left: u128, mut right: u128) -> u128 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left.max(1)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SemanticCandidateV2 {
    pub(crate) document_id: Uuid,
    pub(crate) source_chunk_id: Uuid,
    pub(crate) product_id: Uuid,
    pub(crate) product_version_id: Uuid,
    pub(crate) frozen_document_display_name: String,
    pub(crate) chunk_utf8: String,
    pub(crate) chunk_sha256: String,
    pub(crate) chunk_byte_length: u64,
    pub(crate) source_type: KnowledgeSourceTypeV2,
    pub(crate) vector_rank: Option<u32>,
    pub(crate) keyword_rank: Option<u32>,
    pub(crate) exact_rrf_score: ExactRationalV2,
    pub(crate) pre_rerank_rrf_rank: u32,
}

#[derive(Clone, Debug)]
struct Signal {
    id: Uuid,
    product_id: Uuid,
    version_id: Uuid,
    document_id: Uuid,
    chunk_type: String,
    context_header: String,
    parent_chunk_id: Option<Uuid>,
}

#[derive(Clone, Debug)]
struct Source {
    product_id: Uuid,
    version_id: Uuid,
    document_id: Uuid,
    id: Uuid,
    chunk_type: String,
    content: String,
    file_name: String,
    context_header: String,
    parent_chunk_id: Option<Uuid>,
}

#[derive(Default)]
struct FoldedRanks {
    vector_rank: Option<u32>,
    keyword_rank: Option<u32>,
}

impl PostgresKnowledgeRetrievalAdapter {
    #[allow(dead_code)]
    pub(crate) async fn retrieve_semantic_candidates_v2(
        &self,
        workspace_kind: &'static str,
        selected_versions: &[Uuid],
        requirement_text: &str,
        policy_identity: &RetrievalPolicyIdentityV1,
        embedding_client: &StrictEmbeddingClientV2,
    ) -> Result<Vec<SemanticCandidateV2>, KnowledgeRetrievalError> {
        if !matches!(workspace_kind, "product_line" | "company") {
            return Err(invalid("unsupported semantic workspace kind"));
        }
        if normalize_exact_v2(requirement_text).is_empty() {
            return Err(invalid(
                "semantic requirement must not be empty or whitespace",
            ));
        }
        let selected = selected_versions.iter().copied().collect::<HashSet<_>>();
        if selected.len() != selected_versions.len() || selected.contains(&Uuid::nil()) {
            return Err(invalid("selected versions must be unique, non-nil UUIDs"));
        }

        // Validate the immutable registry artifacts before the network request,
        // but do not retain a database lock while waiting on the provider.
        let validated = self.validate_supported_policy_v2(policy_identity).await?;
        let query_embedding = embedding_client.embed(requirement_text, &validated).await?;
        validate_query_embedding(&query_embedding, &validated.policy)?;

        let mut tx = self.pool.begin().await.map_err(db)?;
        // PostgreSQL forbids FOR SHARE in a transaction declared READ ONLY. This
        // transaction performs reads only, but remains formally read-write so the
        // policy/revision row locks can serialize irreversible revocation.
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        lock_supported_policy_in_snapshot(&mut tx, policy_identity, &validated).await?;
        let result = recall_in_snapshot(
            &mut tx,
            workspace_kind,
            selected_versions,
            requirement_text,
            &validated.policy,
            &query_embedding,
        )
        .await;
        match result {
            Ok(candidates) => {
                tx.commit().await.map_err(db)?;
                Ok(candidates)
            }
            Err(error) => {
                let _ = tx.rollback().await;
                Err(error)
            }
        }
    }
}

pub(crate) fn validate_query_embedding(
    embedding: &QueryEmbeddingV2,
    policy: &RetrievalPolicyV2,
) -> Result<(), KnowledgeRetrievalError> {
    if embedding.revision_sha256 != policy.embedding.model_revision_sha256 {
        return Err(invalid("query embedding revision does not match policy"));
    }
    validate_query_embedding_values(embedding)
}

fn validate_query_embedding_values(
    embedding: &QueryEmbeddingV2,
) -> Result<(), KnowledgeRetrievalError> {
    if embedding.values.len() != EMBEDDING_DIMENSION_V2 {
        return Err(invalid("query embedding dimension must be exactly 1024"));
    }
    if embedding.values.iter().any(|value| !value.is_finite()) {
        return Err(invalid("query embedding contains a non-finite value"));
    }
    let norm = embedding
        .values
        .iter()
        .map(|value| f64::from(*value) * f64::from(*value))
        .sum::<f64>();
    if !norm.is_finite() || norm == 0.0 {
        return Err(invalid("query embedding must have a finite non-zero norm"));
    }
    Ok(())
}

pub(crate) async fn lock_supported_policy_in_snapshot(
    tx: &mut Transaction<'_, Postgres>,
    identity: &RetrievalPolicyIdentityV1,
    validated: &ValidatedSemanticPolicyV2,
) -> Result<(), KnowledgeRetrievalError> {
    let row = sqlx::query(
        "SELECT canonical_policy_payload,canonical_revision_payload,embedding_revision_sha256,credential_ref
           FROM public.kb_knowledge_lock_semantic_policy_v2($1)",
    )
    .bind(&identity.policy_sha256)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db)?;
    let Some(row) = row else {
        return Err(invalid("unknown or revoked knowledge-evidence-v2 policy"));
    };
    let canonical_policy = validated
        .policy
        .canonical_bytes()
        .map_err(|error| invalid(&format!("invalid policy artifact: {error}")))?;
    let registered_policy = row.get::<Vec<u8>, _>("canonical_policy_payload");
    let revision_payload = row.get::<Vec<u8>, _>("canonical_revision_payload");
    let revision_sha = row.get::<String, _>("embedding_revision_sha256");
    let credential_ref = row.get::<String, _>("credential_ref");
    let canonical_revision = validated
        .revision
        .canonical_bytes()
        .map_err(|error| invalid(&format!("invalid embedding revision: {error}")))?;
    if registered_policy != canonical_policy
        || revision_payload != canonical_revision
        || revision_sha != validated.policy.embedding.model_revision_sha256
        || hex::encode(Sha256::digest(&revision_payload)) != revision_sha
        || credential_ref != validated.credential_ref
    {
        return Err(invalid(
            "policy or embedding revision changed during semantic recall",
        ));
    }
    Ok(())
}

pub(crate) async fn recall_in_snapshot(
    tx: &mut Transaction<'_, Postgres>,
    workspace_kind: &str,
    selected_versions: &[Uuid],
    requirement_text: &str,
    policy: &RetrievalPolicyV2,
    query_embedding: &QueryEmbeddingV2,
) -> Result<Vec<SemanticCandidateV2>, KnowledgeRetrievalError> {
    let normalized_requirement = normalize_exact_v2(requirement_text);
    if normalized_requirement.is_empty() {
        return Err(invalid(
            "semantic requirement must not be empty or whitespace",
        ));
    }
    let eligible_rows = sqlx::query(
        "SELECT p.id AS product_id,pv.id AS version_id
           FROM workspaces w JOIN products p ON p.workspace_id=w.id
           JOIN product_versions pv ON pv.product_id=p.id AND p.current_version_id=pv.id
          WHERE w.kind=$1 AND pv.status='active' AND pv.deleted_at IS NULL
            AND (($1='product_line' AND p.kind='product') OR ($1='company' AND p.kind='library'))
            AND (cardinality($2::uuid[])=0 OR pv.id=ANY($2::uuid[]))
          ORDER BY p.id,pv.id",
    )
    .bind(workspace_kind)
    .bind(selected_versions)
    .fetch_all(&mut **tx)
    .await
    .map_err(db)?;
    let eligible = eligible_rows
        .iter()
        .map(|row| row.get::<Uuid, _>("version_id"))
        .collect::<BTreeSet<_>>();
    let selected = selected_versions.iter().copied().collect::<BTreeSet<_>>();
    if !selected.is_empty() && selected != eligible {
        return Err(invalid(
            "selected versions are not the exact current eligible workspace scope",
        ));
    }
    let eligible_vec = eligible.iter().copied().collect::<Vec<_>>();

    let bindings = sqlx::query(
        "SELECT product_version_id,embedding_revision_sha256
           FROM product_version_embedding_bindings_v2 WHERE product_version_id=ANY($1::uuid[])
          ORDER BY product_version_id",
    )
    .bind(&eligible_vec)
    .fetch_all(&mut **tx)
    .await
    .map_err(db)?;
    if bindings.len() != eligible.len()
        || bindings.iter().any(|row| {
            row.get::<String, _>("embedding_revision_sha256")
                != policy.embedding.model_revision_sha256
        })
    {
        return Err(invalid(
            "eligible version embedding binding is missing or mismatched",
        ));
    }

    let marker_rows = sqlx::query(
        "SELECT vector_generation.product_version_id,
                vector_generation.embedding_revision_sha256,
                vector_generation.source_snapshot_sha256,
                vector_generation.chunk_count
           FROM product_version_vector_index_generations_v2 vector_generation
           JOIN product_version_keyword_index_generations_v2 keyword_generation
             ON keyword_generation.product_version_id=vector_generation.product_version_id
            AND keyword_generation.embedding_revision_sha256=vector_generation.embedding_revision_sha256
            AND keyword_generation.source_snapshot_sha256=vector_generation.source_snapshot_sha256
            AND keyword_generation.chunk_count=vector_generation.chunk_count
           JOIN knowledge_semantic_index_intents_v2 intent
             ON intent.product_version_id=vector_generation.product_version_id
            AND intent.embedding_revision_sha256=vector_generation.embedding_revision_sha256
            AND intent.source_snapshot_sha256=vector_generation.source_snapshot_sha256
            AND intent.generation_marker_sha256=vector_generation.source_snapshot_sha256
            AND intent.status='completed'
          WHERE vector_generation.product_version_id=ANY($1::uuid[])
            AND NOT EXISTS(
              SELECT 1 FROM documents pending_document
               WHERE pending_document.product_version_id=vector_generation.product_version_id
                 AND pending_document.deleted_at IS NULL
                 AND (pending_document.parse_status IN ('pending','processing','finalizing')
                      OR pending_document.pending_subtasks_count<>0
                      OR pending_document.summary_status IN ('pending','processing')))
            AND NOT EXISTS(
              SELECT 1 FROM task_pending_ops pending
               WHERE pending.scope='product_version'
                 AND pending.scope_id=vector_generation.product_version_id)
          ORDER BY vector_generation.product_version_id",
    )
    .bind(&eligible_vec)
    .fetch_all(&mut **tx)
    .await
    .map_err(db)?;
    if marker_rows.len() != eligible.len() {
        return Err(integrity("missing complete V2 vector generation"));
    }
    let markers = marker_rows
        .into_iter()
        .map(|row| {
            (
                row.get::<Uuid, _>("product_version_id"),
                (
                    row.get::<String, _>("embedding_revision_sha256"),
                    row.get::<String, _>("source_snapshot_sha256"),
                    row.get::<i64, _>("chunk_count"),
                ),
            )
        })
        .collect::<HashMap<_, _>>();

    let sidecars = sqlx::query(
        "SELECT c.id,c.product_version_id,c.content,c.context_header,
                k.chunk_id AS keyword_chunk_id,k.tokenizer,k.tokenizer_version,
                k.indexed_content,k.indexed_content_sha256 AS keyword_sha,
                v.chunk_id AS vector_chunk_id,v.indexed_content_sha256 AS vector_sha,
                v.source_snapshot_sha256 AS vector_snapshot_sha
           FROM chunks c JOIN documents d ON d.id=c.document_id
           LEFT JOIN chunk_keyword_indexes_v2 k ON k.chunk_id=c.id
             AND k.tokenizer=$2 AND k.tokenizer_version=$3
           LEFT JOIN chunk_vector_indexes_v2 v ON v.chunk_id=c.id
             AND v.product_version_id=c.product_version_id
             AND v.embedding_revision_sha256=$4
          WHERE c.product_version_id=ANY($1::uuid[]) AND d.product_version_id=c.product_version_id
AND d.deleted_at IS NULL AND d.enable_status='enabled' AND d.index_ready
AND c.chunk_type=ANY($5::text[])
AND (c.chunk_type<>'image_ocr' OR EXISTS (SELECT 1 FROM knowledge_image_ocr_chunk_artifact_mappings mapping WHERE mapping.chunk_id=c.id))
ORDER BY c.id",
    )
    .bind(&eligible_vec)
    .bind(&policy.keyword.tokenizer)
    .bind(&policy.keyword.tokenizer_version)
    .bind(&policy.embedding.model_revision_sha256)
    .bind(SIGNAL_TYPES.as_slice())
    .fetch_all(&mut **tx)
    .await
    .map_err(db)?;
    let mut snapshot_inputs =
        BTreeMap::<Uuid, Vec<crate::knowledge_index_v2::VectorEmbeddingInputV2>>::new();
    for row in &sidecars {
        let version_id = row.get::<Uuid, _>("product_version_id");
        let content = row.get::<String, _>("content");
        let context_header = row.get::<String, _>("context_header");
        let keyword_digest = hex::encode(Sha256::digest(content.as_bytes()));
        let canonical_input =
            crate::knowledge_index_v2::canonical_embedding_input_v2(&context_header, &content);
        let vector_digest = hex::encode(Sha256::digest(canonical_input.as_bytes()));
        let marker = markers
            .get(&version_id)
            .ok_or_else(|| integrity("missing complete V2 vector generation"))?;
        let keyword_ok = row.get::<Option<Uuid>, _>("keyword_chunk_id").is_some()
            && row.get::<Option<String>, _>("indexed_content").as_deref() == Some(content.as_str())
            && row.get::<Option<String>, _>("keyword_sha").as_deref()
                == Some(keyword_digest.as_str());
        let vector_ok = row.get::<Option<Uuid>, _>("vector_chunk_id").is_some()
            && row.get::<Option<String>, _>("vector_sha").as_deref()
                == Some(vector_digest.as_str())
            && row
                .get::<Option<String>, _>("vector_snapshot_sha")
                .as_deref()
                == Some(marker.1.as_str());
        if !keyword_ok || !vector_ok {
            return Err(integrity("missing or stale V2 semantic sidecar"));
        }
        snapshot_inputs.entry(version_id).or_default().push(
            crate::knowledge_index_v2::VectorEmbeddingInputV2 {
                chunk_id: row.get("id"),
                canonical_input,
                indexed_content_sha256: vector_digest,
            },
        );
    }
    for version_id in &eligible_vec {
        let marker = markers
            .get(version_id)
            .ok_or_else(|| integrity("missing complete V2 vector generation"))?;
        let inputs = snapshot_inputs
            .get(version_id)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let input_count = i64::try_from(inputs.len())
            .map_err(|_| integrity("vector generation count overflow"))?;
        let computed_snapshot =
            crate::knowledge_index_v2::source_snapshot_sha256_v2(&marker.0, inputs);
        if marker.0 != policy.embedding.model_revision_sha256
            || marker.2 != input_count
            || marker.1 != computed_snapshot
        {
            return Err(integrity(format!(
                "missing or stale V2 vector generation (marker_count={}, input_count={}, marker_snapshot={}, computed_snapshot={})",
                marker.2, input_count, marker.1, computed_snapshot
            )));
        }
    }

    let signals = load_signals(tx, workspace_kind, &eligible_vec).await?;
    let query_vector = vector_literal(&query_embedding.values);
    let keyword_ranks =
        keyword_ranks(tx, workspace_kind, &eligible_vec, requirement_text, policy).await?;
    let vector_ranks =
        vector_ranks(tx, workspace_kind, &eligible_vec, &query_vector, policy).await?;
    let active_signal_ids = keyword_ranks
        .keys()
        .chain(vector_ranks.keys())
        .copied()
        .collect::<HashSet<_>>();
    let graph_refs = load_graph_refs(tx, &signals, &active_signal_ids).await?;
    let wiki_refs = load_wiki_refs(tx, &signals, &active_signal_ids).await?;
    let sources = load_sources(tx, workspace_kind, &eligible_vec).await?;
    let source_by_id = sources
        .into_iter()
        .map(|source| (source.id, source))
        .collect::<HashMap<_, _>>();
    // This set is deliberately derived from every live trusted source in the
    // recall snapshot, not from quota-truncated exact A/B results or a caller.
    let exact_source_chunk_ids = source_by_id
        .values()
        .filter(|source| normalize_exact_v2(&source.content).contains(&normalized_requirement))
        .map(|source| source.id)
        .collect::<HashSet<_>>();

    let mut folded = HashMap::<Uuid, FoldedRanks>::new();
    for signal_id in active_signal_ids {
        let Some(signal) = signals.get(&signal_id) else {
            return Err(integrity("ranked signal left snapshot scope"));
        };
        let Some(source_id) =
            fold_signal(signal, &signals, &source_by_id, &graph_refs, &wiki_refs)?
        else {
            continue;
        };
        if exact_source_chunk_ids.contains(&source_id) {
            continue;
        }
        let ranks = folded.entry(source_id).or_default();
        if let Some(rank) = vector_ranks.get(&signal_id) {
            ranks.vector_rank = Some(ranks.vector_rank.map_or(*rank, |old| old.min(*rank)));
        }
        if let Some(rank) = keyword_ranks.get(&signal_id) {
            ranks.keyword_rank = Some(ranks.keyword_rank.map_or(*rank, |old| old.min(*rank)));
        }
    }

    let mut candidates = Vec::with_capacity(folded.len());
    for (source_id, ranks) in folded {
        let source = source_by_id
            .get(&source_id)
            .ok_or_else(|| integrity("folded source left snapshot scope"))?;
        let score = rrf_score(policy, ranks.vector_rank, ranks.keyword_rank)?;
        let source_type = match source.chunk_type.as_str() {
            "text" => KnowledgeSourceTypeV2::Text,
            "parent_text" => KnowledgeSourceTypeV2::ParentText,
            "image_ocr" => KnowledgeSourceTypeV2::ImageOcr,
            _ => return Err(integrity("folded source is not trusted")),
        };
        candidates.push(SemanticCandidateV2 {
            document_id: source.document_id,
            source_chunk_id: source.id,
            product_id: source.product_id,
            product_version_id: source.version_id,
            frozen_document_display_name: source.file_name.clone(),
            chunk_utf8: source.content.clone(),
            chunk_sha256: hex::encode(Sha256::digest(source.content.as_bytes())),
            chunk_byte_length: source.content.len() as u64,
            source_type,
            vector_rank: ranks.vector_rank,
            keyword_rank: ranks.keyword_rank,
            exact_rrf_score: score,
            pre_rerank_rrf_rank: 0,
        });
    }
    candidates.sort_by(compare_candidates);
    candidates.truncate(policy.rerank.top_k as usize);
    for (index, candidate) in candidates.iter_mut().enumerate() {
        candidate.pre_rerank_rrf_rank =
            u32::try_from(index + 1).map_err(|_| integrity("pre-rerank rank overflow"))?;
    }
    Ok(candidates)
}

async fn load_signals(
    tx: &mut Transaction<'_, Postgres>,
    workspace_kind: &str,
    versions: &[Uuid],
) -> Result<HashMap<Uuid, Signal>, KnowledgeRetrievalError> {
    let rows = sqlx::query(
        "SELECT c.id,c.product_version_id,c.document_id,c.chunk_type,c.content,c.context_header,c.parent_chunk_id,
                p.id AS product_id,d.file_name
           FROM chunks c JOIN documents d ON d.id=c.document_id AND d.product_version_id=c.product_version_id
           JOIN product_versions pv ON pv.id=c.product_version_id JOIN products p ON p.id=pv.product_id
           JOIN workspaces w ON w.id=p.workspace_id
          WHERE w.kind=$1 AND c.product_version_id=ANY($2::uuid[]) AND d.deleted_at IS NULL
            AND d.enable_status='enabled' AND d.index_ready AND c.chunk_type=ANY($3::text[])
            AND (c.chunk_type<>'image_ocr' OR EXISTS (SELECT 1 FROM knowledge_image_ocr_chunk_artifact_mappings mapping WHERE mapping.chunk_id=c.id))"
    ).bind(workspace_kind).bind(versions).bind(SIGNAL_TYPES.as_slice()).fetch_all(&mut **tx).await.map_err(db)?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let id = row.get("id");
            (
                id,
                Signal {
                    id,
                    product_id: row.get("product_id"),
                    version_id: row.get("product_version_id"),
                    document_id: row.get("document_id"),
                    chunk_type: row.get("chunk_type"),
                    context_header: row.get("context_header"),
                    parent_chunk_id: row.get("parent_chunk_id"),
                },
            )
        })
        .collect())
}

async fn load_sources(
    tx: &mut Transaction<'_, Postgres>,
    workspace_kind: &str,
    versions: &[Uuid],
) -> Result<Vec<Source>, KnowledgeRetrievalError> {
    let rows=sqlx::query(
        "SELECT c.id,c.product_version_id,c.document_id,c.chunk_type,c.content,c.context_header,c.parent_chunk_id,p.id AS product_id,d.file_name
           FROM chunks c JOIN documents d ON d.id=c.document_id AND d.product_version_id=c.product_version_id
           JOIN product_versions pv ON pv.id=c.product_version_id JOIN products p ON p.id=pv.product_id JOIN workspaces w ON w.id=p.workspace_id
          WHERE w.kind=$1 AND c.product_version_id=ANY($2::uuid[]) AND d.deleted_at IS NULL AND d.enable_status='enabled' AND d.index_ready
            AND c.chunk_type=ANY($3::text[])
            AND (c.chunk_type<>'image_ocr' OR EXISTS (SELECT 1 FROM knowledge_image_ocr_chunk_artifact_mappings mapping WHERE mapping.chunk_id=c.id))"
    ).bind(workspace_kind).bind(versions).bind(["text","parent_text","image_ocr"].as_slice()).fetch_all(&mut **tx).await.map_err(db)?;
    Ok(rows
        .into_iter()
        .map(|row| Source {
            product_id: row.get("product_id"),
            version_id: row.get("product_version_id"),
            document_id: row.get("document_id"),
            id: row.get("id"),
            chunk_type: row.get("chunk_type"),
            content: row.get("content"),
            file_name: row.get("file_name"),
            context_header: row.get("context_header"),
            parent_chunk_id: row.get("parent_chunk_id"),
        })
        .collect())
}

async fn keyword_ranks(
    tx: &mut Transaction<'_, Postgres>,
    workspace_kind: &str,
    versions: &[Uuid],
    query: &str,
    policy: &RetrievalPolicyV2,
) -> Result<HashMap<Uuid, u32>, KnowledgeRetrievalError> {
    let rows=sqlx::query(
        "WITH prepared AS (SELECT public.kb_knowledge_keyword_token_stream_v2($3) AS stream),
         query_value AS (SELECT CASE WHEN stream='' THEN NULL::tsquery ELSE to_tsquery('simple',replace(stream,' ',' | ')) END AS value FROM prepared),
         scored AS (SELECT c.id,p.id AS product_id,c.product_version_id,c.document_id,
                    floor(least(1.0::real,greatest(0.0::real,ts_rank_cd(k.tsv,q.value,32)))*1000000)::bigint AS score
           FROM query_value q JOIN chunks c ON true JOIN documents d ON d.id=c.document_id AND d.product_version_id=c.product_version_id
           JOIN product_versions pv ON pv.id=c.product_version_id JOIN products p ON p.id=pv.product_id JOIN workspaces w ON w.id=p.workspace_id
           JOIN chunk_keyword_indexes_v2 k ON k.chunk_id=c.id AND k.tokenizer=$4 AND k.tokenizer_version=$5
          WHERE q.value IS NOT NULL AND w.kind=$1 AND c.product_version_id=ANY($2::uuid[]) AND d.deleted_at IS NULL
            AND d.enable_status='enabled' AND d.index_ready AND c.chunk_type=ANY($8::text[])
AND (c.chunk_type<>'image_ocr' OR EXISTS (SELECT 1 FROM knowledge_image_ocr_chunk_artifact_mappings mapping WHERE mapping.chunk_id=c.id))
AND k.tsv @@ q.value),
         chosen AS (SELECT *,row_number() OVER(ORDER BY score DESC,product_id,product_version_id,document_id,id) AS rank FROM scored WHERE score >= $6)
         SELECT id,rank FROM chosen WHERE rank <= $7 ORDER BY rank"
    ).bind(workspace_kind).bind(versions).bind(query).bind(&policy.keyword.tokenizer).bind(&policy.keyword.tokenizer_version)
      .bind(i64::from(policy.keyword.threshold_millionths)).bind(i64::from(policy.keyword.top_k)).bind(SIGNAL_TYPES.as_slice())
      .fetch_all(&mut **tx).await.map_err(db)?;
    rows.into_iter()
        .map(|row| {
            Ok((
                row.get("id"),
                u32::try_from(row.get::<i64, _>("rank"))
                    .map_err(|_| integrity("keyword rank overflow"))?,
            ))
        })
        .collect()
}

async fn vector_ranks(
    tx: &mut Transaction<'_, Postgres>,
    workspace_kind: &str,
    versions: &[Uuid],
    query_vector: &str,
    policy: &RetrievalPolicyV2,
) -> Result<HashMap<Uuid, u32>, KnowledgeRetrievalError> {
    let invalid_distance_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM chunks c
           JOIN documents d ON d.id=c.document_id AND d.product_version_id=c.product_version_id
           JOIN product_versions pv ON pv.id=c.product_version_id
           JOIN products p ON p.id=pv.product_id JOIN workspaces w ON w.id=p.workspace_id
           JOIN chunk_vector_indexes_v2 v ON v.chunk_id=c.id
            AND v.product_version_id=c.product_version_id
            AND v.embedding_revision_sha256=$4
          WHERE w.kind=$1 AND c.product_version_id=ANY($2::uuid[]) AND d.deleted_at IS NULL
            AND d.enable_status='enabled' AND d.index_ready AND c.chunk_type=ANY($5::text[])
            AND (c.chunk_type<>'image_ocr' OR EXISTS (SELECT 1 FROM knowledge_image_ocr_chunk_artifact_mappings mapping WHERE mapping.chunk_id=c.id))
            AND ((v.embedding <=> CAST($3 AS vector))::float8)::text IN ('NaN','Infinity','-Infinity')",
    )
    .bind(workspace_kind)
    .bind(versions)
    .bind(query_vector)
    .bind(&policy.embedding.model_revision_sha256)
    .bind(SIGNAL_TYPES.as_slice())
    .fetch_one(&mut **tx)
    .await
    .map_err(db)?;
    if invalid_distance_count != 0 {
        return Err(integrity("non-finite V2 cosine distance"));
    }
    let rows=sqlx::query(
        "WITH distances AS (SELECT c.id,p.id AS product_id,c.product_version_id,c.document_id,(v.embedding <=> CAST($3 AS vector))::float8 AS distance
           FROM chunks c JOIN documents d ON d.id=c.document_id AND d.product_version_id=c.product_version_id
           JOIN product_versions pv ON pv.id=c.product_version_id JOIN products p ON p.id=pv.product_id JOIN workspaces w ON w.id=p.workspace_id
           JOIN chunk_vector_indexes_v2 v ON v.chunk_id=c.id
            AND v.product_version_id=c.product_version_id
            AND v.embedding_revision_sha256=$4
          WHERE w.kind=$1 AND c.product_version_id=ANY($2::uuid[]) AND d.deleted_at IS NULL AND d.enable_status='enabled' AND d.index_ready AND c.chunk_type=ANY($7::text[])
AND (c.chunk_type<>'image_ocr' OR EXISTS (SELECT 1 FROM knowledge_image_ocr_chunk_artifact_mappings mapping WHERE mapping.chunk_id=c.id))),
         scored AS (SELECT *,floor(least(1.0::float8,greatest(0.0::float8,1.0-distance))*1000000)::bigint AS score FROM distances
                    WHERE distance::text NOT IN ('NaN','Infinity','-Infinity')),
         chosen AS (SELECT *,row_number() OVER(ORDER BY score DESC,product_id,product_version_id,document_id,id) AS rank FROM scored WHERE score >= $5)
         SELECT id,rank FROM chosen WHERE rank <= $6 ORDER BY rank"
    ).bind(workspace_kind).bind(versions).bind(query_vector).bind(&policy.embedding.model_revision_sha256)
      .bind(i64::from(policy.embedding.threshold_millionths)).bind(i64::from(policy.embedding.top_k)).bind(SIGNAL_TYPES.as_slice())
      .fetch_all(&mut **tx).await.map_err(db)?;
    rows.into_iter()
        .map(|row| {
            Ok((
                row.get("id"),
                u32::try_from(row.get::<i64, _>("rank"))
                    .map_err(|_| integrity("vector rank overflow"))?,
            ))
        })
        .collect()
}

async fn load_graph_refs(
    tx: &mut Transaction<'_, Postgres>,
    signals: &HashMap<Uuid, Signal>,
    active: &HashSet<Uuid>,
) -> Result<HashMap<Uuid, Vec<Uuid>>, KnowledgeRetrievalError> {
    let ids = active
        .iter()
        .filter(|id| {
            signals
                .get(id)
                .is_some_and(|s| s.chunk_type == "graph_node")
        })
        .copied()
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows=sqlx::query("SELECT s.id,n.chunk_ids FROM chunks s JOIN graph_nodes n ON n.product_version_id=s.product_version_id AND n.document_id=s.document_id AND n.name=s.context_header WHERE s.id=ANY($1::uuid[]) AND s.content=n.name ORDER BY s.id")
        .bind(&ids).fetch_all(&mut **tx).await.map_err(db)?;
    let mut refs = HashMap::new();
    for row in rows {
        let id = row
            .try_get::<Uuid, _>("id")
            .map_err(|error| integrity(format!("invalid graph signal id: {error}")))?;
        let values = row
            .try_get::<Vec<Option<Uuid>>, _>("chunk_ids")
            .map_err(|error| integrity(format!("invalid graph chunk_ids: {error}")))?;
        let mut parsed = Vec::with_capacity(values.len());
        for value in values {
            parsed.push(value.ok_or_else(|| integrity("graph chunk_ids contains NULL"))?);
        }
        refs.insert(id, parsed);
    }
    Ok(refs)
}

async fn load_wiki_refs(
    tx: &mut Transaction<'_, Postgres>,
    signals: &HashMap<Uuid, Signal>,
    active: &HashSet<Uuid>,
) -> Result<HashMap<Uuid, Vec<Uuid>>, KnowledgeRetrievalError> {
    let ids = active
        .iter()
        .filter(|id| signals.get(id).is_some_and(|s| s.chunk_type == "wiki_page"))
        .copied()
        .collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows=sqlx::query("SELECT s.id,w.chunk_refs FROM chunks s JOIN wiki_pages w ON w.product_version_id=s.product_version_id AND w.slug=s.context_header WHERE s.id=ANY($1::uuid[]) AND s.content=w.content AND w.status='published' AND w.deleted_at IS NULL ORDER BY s.id")
        .bind(&ids).fetch_all(&mut **tx).await.map_err(db)?;
    let mut refs = HashMap::new();
    for row in rows {
        let value = row.get::<Value, _>("chunk_refs");
        let array = value
            .as_array()
            .ok_or_else(|| integrity("wiki chunk_refs is not an array"))?;
        let mut parsed = Vec::with_capacity(array.len());
        for item in array {
            let text = item
                .as_str()
                .ok_or_else(|| integrity("wiki chunk_refs contains a non-string"))?;
            parsed.push(
                Uuid::parse_str(text)
                    .map_err(|_| integrity("wiki chunk_refs contains an invalid UUID"))?,
            );
        }
        refs.insert(row.get("id"), parsed);
    }
    Ok(refs)
}

fn fold_signal(
    signal: &Signal,
    signals: &HashMap<Uuid, Signal>,
    sources: &HashMap<Uuid, Source>,
    graph_refs: &HashMap<Uuid, Vec<Uuid>>,
    wiki_refs: &HashMap<Uuid, Vec<Uuid>>,
) -> Result<Option<Uuid>, KnowledgeRetrievalError> {
    let candidates = match signal.chunk_type.as_str() {
        "text" | "parent_text" | "image_ocr" => vec![signal.id],
        "question" | "summary" => signal.parent_chunk_id.into_iter().collect(),
        "image_caption" => {
            if signal.context_header.is_empty() {
                Vec::new()
            } else {
                sources
                    .values()
                    .filter(|source| {
                        source.chunk_type == "image_ocr"
                            && source.version_id == signal.version_id
                            && source.document_id == signal.document_id
                            && source.context_header == signal.context_header
                            && source.parent_chunk_id == signal.parent_chunk_id
                    })
                    .map(|source| source.id)
                    .collect()
            }
        }
        "graph_node" => graph_refs.get(&signal.id).cloned().unwrap_or_default(),
        "wiki_page" => wiki_refs.get(&signal.id).cloned().unwrap_or_default(),
        _ => Vec::new(),
    };
    if candidates.len() != 1 {
        return Ok(None);
    }
    let id = candidates[0];
    let Some(source) = sources.get(&id) else {
        return Ok(None);
    };
    if source.version_id != signal.version_id
        || source.document_id != signal.document_id
        || source.product_id != signal.product_id
    {
        return Ok(None);
    }
    if matches!(signal.chunk_type.as_str(), "question" | "summary")
        && !signals.contains_key(&signal.id)
    {
        return Err(integrity("derived signal missing"));
    }
    Ok(Some(id))
}

fn rrf_score(
    policy: &RetrievalPolicyV2,
    vector_rank: Option<u32>,
    keyword_rank: Option<u32>,
) -> Result<ExactRationalV2, KnowledgeRetrievalError> {
    let mut score = None;
    if let Some(rank) = vector_rank {
        score = Some(ExactRationalV2::channel(
            policy.rrf.vector_weight_millionths,
            policy.rrf.k,
            rank,
        )?)
    }
    if let Some(rank) = keyword_rank {
        let part =
            ExactRationalV2::channel(policy.rrf.keyword_weight_millionths, policy.rrf.k, rank)?;
        score = Some(match score {
            Some(old) => old.add(part)?,
            None => part,
        });
    }
    score.ok_or_else(|| integrity("folded source has no channel rank"))
}

fn compare_candidates(left: &SemanticCandidateV2, right: &SemanticCandidateV2) -> Ordering {
    right
        .exact_rrf_score
        .cmp_exact(&left.exact_rrf_score)
        .then_with(|| option_rank(left.vector_rank, right.vector_rank))
        .then_with(|| option_rank(left.keyword_rank, right.keyword_rank))
        .then(left.product_id.cmp(&right.product_id))
        .then(left.product_version_id.cmp(&right.product_version_id))
        .then(left.document_id.cmp(&right.document_id))
        .then(left.source_chunk_id.cmp(&right.source_chunk_id))
}
fn option_rank(left: Option<u32>, right: Option<u32>) -> Ordering {
    match (left, right) {
        (Some(l), Some(r)) => l.cmp(&r),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}
fn vector_literal(values: &[f32]) -> String {
    let mut out = String::with_capacity(values.len() * 4);
    out.push('[');
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            out.push(',')
        }
        out.push_str(&value.to_string());
    }
    out.push(']');
    out
}
fn invalid(message: &str) -> KnowledgeRetrievalError {
    KnowledgeRetrievalError::InvalidRequest(message.into())
}
fn integrity(message: impl Into<String>) -> KnowledgeRetrievalError {
    KnowledgeRetrievalError::Unavailable(message.into())
}
fn db(error: sqlx::Error) -> KnowledgeRetrievalError {
    KnowledgeRetrievalError::Unavailable(error.to_string())
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::knowledge_retrieval::{
        EMBEDDING_DIMENSION_V2, EMBEDDING_OUTPUT_NORMALIZATION_VERSION_V2,
        EMBEDDING_PROVIDER_PROTOCOL_VERSION_V2, EMBEDDING_REVISION_SCHEMA_V2, EmbeddingRevisionV2,
        RERANK_REVISION_SCHEMA_V2, RETRIEVAL_A_PRIMARY_COMPARATOR_V2,
        RETRIEVAL_A_VERSION_COMPARATOR_V2, RETRIEVAL_B_EXACT_COMPARATOR_V2,
        RETRIEVAL_C_SEMANTIC_COMPARATOR_V2, RETRIEVAL_CHANNEL_RANK_COMPARATOR_V2,
        RETRIEVAL_PRE_RERANK_RRF_COMPARATOR_V2, RerankRevisionV2, RetrievalEmbeddingPolicyV2,
        RetrievalKeywordPolicyV2, RetrievalRankingPolicyV2, RetrievalRequestQuotasV2,
        RetrievalRerankPolicyV2, RetrievalRrfPolicyV2,
    };
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    struct SemanticFixtureVectorProvider {
        other_vector_ids: HashSet<Uuid>,
        calls: AtomicUsize,
    }

    #[async_trait]
    impl crate::knowledge_index_v2::VectorEmbeddingProviderV2 for SemanticFixtureVectorProvider {
        async fn embed_batch(
            &self,
            _revision: &EmbeddingRevisionV2,
            _credential_ref: &str,
            inputs: &[crate::knowledge_index_v2::VectorEmbeddingInputV2],
        ) -> Result<Vec<Vec<f32>>, crate::knowledge_index_v2::VectorIndexErrorV2> {
            self.calls.fetch_add(1, AtomicOrdering::SeqCst);
            Ok(inputs
                .iter()
                .map(|input| {
                    let mut values = vec![0.0; EMBEDDING_DIMENSION_V2 as usize];
                    values[usize::from(self.other_vector_ids.contains(&input.chunk_id))] = 1.0;
                    values
                })
                .collect())
        }
    }

    pub(crate) fn policy() -> RetrievalPolicyV2 {
        RetrievalPolicyV2 {
            schema_version: 2,
            contract_version: "knowledge-evidence-v2".into(),
            normalization_version: "unicode-whitespace-lowercase-v1".into(),
            trusted_source_types: vec!["text".into(), "parent_text".into(), "image_ocr".into()],
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
                source_folding_version: "unique-live-trusted-source-v1".into(),
                channel_score_quantization_version: "floor-unit-interval-millionths-v1".into(),
                channel_rank_comparator: RETRIEVAL_CHANNEL_RANK_COMPARATOR_V2
                    .iter()
                    .map(|value| (*value).into())
                    .collect(),
                pre_rerank_rrf_comparator: RETRIEVAL_PRE_RERANK_RRF_COMPARATOR_V2
                    .iter()
                    .map(|value| (*value).into())
                    .collect(),
                quota_semantics_version: "fair-exact-prefix-fail-closed-v1".into(),
            },
            keyword: RetrievalKeywordPolicyV2 {
                tokenizer: "latin-numeric-cjk-bigram".into(),
                tokenizer_version: "v1".into(),
                score_version: "postgres-ts-rank-cd-normalization32-millionths-v1".into(),
                top_k: 128,
                threshold_millionths: 50000,
            },
            embedding: RetrievalEmbeddingPolicyV2 {
                policy: "declared-version-model".into(),
                policy_version: "v1".into(),
                similarity_version: "pgvector-cosine-clamp-zero-one-millionths-v1".into(),
                model_revision_sha256: "a".repeat(64),
                top_k: 128,
                threshold_millionths: 100000,
            },
            rrf: RetrievalRrfPolicyV2 {
                k: 60,
                keyword_weight_millionths: 1_000_000,
                vector_weight_millionths: 1_000_000,
                score_representation_version: "reduced-u128-rational-v1".into(),
            },
            rerank: RetrievalRerankPolicyV2 {
                provider_protocol_version: "indexed-json-v1".into(),
                revision_sha256: "d".repeat(64),
                model_revision_sha256: "b".repeat(64),
                config_revision_sha256: "c".repeat(64),
                top_k: 64,
                timeout_ms: 5000,
                score_normalization_version: "unit-interval-millionths-v1".into(),
            },
            request_quotas: RetrievalRequestQuotasV2 {
                max_hits: 64,
                max_chunk_bytes: 262144,
                max_total_bytes: 8388608,
            },
        }
    }
    #[test]
    fn unit_weight_rrf_is_exact() {
        let score = rrf_score(&policy(), Some(1), Some(2)).unwrap();
        assert_eq!(
            score,
            ExactRationalV2 {
                numerator: 123,
                denominator: 3782
            }
        );
    }
    struct CountingCredentialResolver(AtomicUsize);

    #[async_trait]
    impl EmbeddingCredentialResolverV2 for CountingCredentialResolver {
        async fn resolve(&self, _credential_ref: &str) -> Result<String, KnowledgeRetrievalError> {
            self.0.fetch_add(1, AtomicOrdering::SeqCst);
            Ok("credential".into())
        }
    }

    struct ErrorCredentialResolver(u8);

    #[async_trait]
    impl EmbeddingCredentialResolverV2 for ErrorCredentialResolver {
        async fn resolve(&self, _credential_ref: &str) -> Result<String, KnowledgeRetrievalError> {
            match self.0 {
                0 => Err(invalid("credential configuration is invalid")),
                1 => Err(integrity("credential service is unavailable")),
                2 => Err(KnowledgeRetrievalError::QuotaExceeded("unexpected".into())),
                3 => Err(KnowledgeRetrievalError::InvalidHit("unexpected".into())),
                _ => Ok(String::new()),
            }
        }
    }

    fn validated_policy(revision: EmbeddingRevisionV2) -> ValidatedSemanticPolicyV2 {
        ValidatedSemanticPolicyV2 {
            policy: policy(),
            revision,
            credential_ref: "test:semantic-v2".into(),
        }
    }

    #[tokio::test]
    async fn embedding_transport_rejects_non_https_before_credential_resolution() {
        let resolver = Arc::new(CountingCredentialResolver(AtomicUsize::new(0)));
        let client = StrictEmbeddingClientV2::new(resolver.clone()).unwrap();
        let mut revision = test_revision();
        revision.endpoint_identity = "http://embeddings.example.test/v1/embeddings".into();

        assert!(matches!(
            client.embed("alpha", &validated_policy(revision)).await,
            Err(KnowledgeRetrievalError::InvalidRequest(_))
        ));
        assert_eq!(resolver.0.load(AtomicOrdering::SeqCst), 0);
    }

    #[tokio::test]
    async fn embedding_credential_errors_have_strict_taxonomy() {
        for (resolver_result, transient) in
            [(0, false), (1, true), (2, false), (3, false), (4, false)]
        {
            let client =
                StrictEmbeddingClientV2::new(Arc::new(ErrorCredentialResolver(resolver_result)))
                    .unwrap();
            let result = client
                .embed("alpha", &validated_policy(test_revision()))
                .await;
            if transient {
                assert!(matches!(
                    result,
                    Err(KnowledgeRetrievalError::Unavailable(_))
                ));
            } else {
                assert!(matches!(
                    result,
                    Err(KnowledgeRetrievalError::InvalidRequest(_))
                ));
            }
        }
    }

    #[test]
    fn embedding_http_status_taxonomy_is_strict() {
        for status in [408, 429, 500, 503, 599] {
            assert!(matches!(
                embedding_status_error(reqwest::StatusCode::from_u16(status).unwrap()),
                KnowledgeRetrievalError::Unavailable(_)
            ));
        }
        for status in [300, 307, 400, 401, 404, 422] {
            assert!(matches!(
                embedding_status_error(reqwest::StatusCode::from_u16(status).unwrap()),
                KnowledgeRetrievalError::InvalidRequest(_)
            ));
        }
    }

    #[test]
    fn embedding_response_rejects_model_data_index_dimension_and_zero_vector() {
        let revision = test_revision();
        let valid = serde_json::json!({
            "data": [{"index": 0, "embedding": vec![1.0; 1024]}],
            "model": revision.provider_model_identifier,
            "model_revision_sha256": revision.provider_model_revision_sha256,
            "request_config_sha256": revision.request_config_sha256,
        });
        assert!(parse_embedding_response(&serde_json::to_vec(&valid).unwrap(), &revision).is_ok());

        let wrong_model = serde_json::json!({
            "data": [{"index": 0, "embedding": vec![1.0; 1024]}],
            "model": "wrong-model",
            "model_revision_sha256": revision.provider_model_revision_sha256,
            "request_config_sha256": revision.request_config_sha256,
        });
        assert!(matches!(
            parse_embedding_response(&serde_json::to_vec(&wrong_model).unwrap(), &revision),
            Err(KnowledgeRetrievalError::InvalidRequest(_))
        ));

        let malformed_data_or_vectors = [
            serde_json::json!({"data": [], "model": revision.provider_model_identifier}),
            serde_json::json!({"data": [{"index": 1, "embedding": vec![1.0; 1024]}], "model": revision.provider_model_identifier}),
            serde_json::json!({"data": [
                {"index": 0, "embedding": vec![1.0; 1024]},
                {"index": 0, "embedding": vec![1.0; 1024]}
            ], "model": revision.provider_model_identifier}),
            serde_json::json!({"data": [{"index": 0, "embedding": vec![1.0; 1023]}], "model": revision.provider_model_identifier}),
            serde_json::json!({"data": [{"index": 0, "embedding": vec![0.0; 1024]}], "model": revision.provider_model_identifier}),
        ];
        for mutation in malformed_data_or_vectors {
            assert!(matches!(
                parse_embedding_response(&serde_json::to_vec(&mutation).unwrap(), &revision),
                Err(KnowledgeRetrievalError::Unavailable(_))
            ));
        }
        assert!(matches!(
            parse_embedding_response(br#"{"data":not-json}"#, &revision),
            Err(KnowledgeRetrievalError::Unavailable(_))
        ));
    }

    #[test]
    fn query_embedding_rejects_revision_dimension_nonfinite_and_zero() {
        let p = policy();
        let mut embedding = QueryEmbeddingV2 {
            revision_sha256: p.embedding.model_revision_sha256.clone(),
            values: vec![0.0; 1024],
        };
        assert!(validate_query_embedding(&embedding, &p).is_err());
        embedding.values[0] = 1.0;
        assert!(validate_query_embedding(&embedding, &p).is_ok());
        embedding.values[1] = f32::NAN;
        assert!(validate_query_embedding(&embedding, &p).is_err());
        embedding.values = vec![1.0; 1023];
        assert!(validate_query_embedding(&embedding, &p).is_err());
        embedding.values = vec![1.0; 1024];
        embedding.revision_sha256 = "d".repeat(64);
        assert!(validate_query_embedding(&embedding, &p).is_err());
    }
    #[test]
    fn fraction_comparator_matches_small_cross_products_exhaustively() {
        for left_numerator in 0..=8 {
            for left_denominator in 1..=8 {
                for right_numerator in 0..=8 {
                    for right_denominator in 1..=8 {
                        assert_eq!(
                            compare_fractions_without_products(
                                left_numerator,
                                left_denominator,
                                right_numerator,
                                right_denominator,
                            ),
                            (left_numerator * right_denominator)
                                .cmp(&(right_numerator * left_denominator))
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn fold_signal_maps_each_supported_signal_independently() {
        let product_id = Uuid::from_u128(1);
        let version_id = Uuid::from_u128(2);
        let document_id = Uuid::from_u128(3);
        let source_id = Uuid::from_u128(4);
        let source = Source {
            product_id,
            version_id,
            document_id,
            id: source_id,
            chunk_type: "image_ocr".into(),
            content: "trusted".into(),
            file_name: "d".into(),
            context_header: "image-1".into(),
            parent_chunk_id: None,
        };
        let sources = HashMap::from([(source_id, source)]);
        let make_signal = |id, chunk_type: &str, header: &str, parent_chunk_id| Signal {
            id,
            product_id,
            version_id,
            document_id,
            chunk_type: chunk_type.into(),
            context_header: header.into(),
            parent_chunk_id,
        };
        let cases = [
            make_signal(source_id, "image_ocr", "image-1", None),
            make_signal(Uuid::from_u128(5), "question", "", Some(source_id)),
            make_signal(Uuid::from_u128(6), "summary", "", Some(source_id)),
            make_signal(Uuid::from_u128(7), "image_caption", "image-1", None),
            make_signal(Uuid::from_u128(8), "graph_node", "graph", None),
            make_signal(Uuid::from_u128(9), "wiki_page", "wiki", None),
        ];
        let signals = cases
            .iter()
            .cloned()
            .map(|signal| (signal.id, signal))
            .collect::<HashMap<_, _>>();
        let graph_refs = HashMap::from([(Uuid::from_u128(8), vec![source_id])]);
        let wiki_refs = HashMap::from([(Uuid::from_u128(9), vec![source_id])]);
        for signal in cases {
            assert_eq!(
                fold_signal(&signal, &signals, &sources, &graph_refs, &wiki_refs).unwrap(),
                Some(source_id),
                "{} must fold independently",
                signal.chunk_type
            );
        }
    }

    #[test]
    fn comparator_uses_exact_score_then_nullable_channel_ranks_and_identity() {
        let base = SemanticCandidateV2 {
            document_id: Uuid::from_u128(3),
            source_chunk_id: Uuid::from_u128(4),
            product_id: Uuid::from_u128(1),
            product_version_id: Uuid::from_u128(2),
            frozen_document_display_name: "d".into(),
            chunk_utf8: "s".into(),
            chunk_sha256: "x".into(),
            chunk_byte_length: 1,
            source_type: KnowledgeSourceTypeV2::Text,
            vector_rank: Some(1),
            keyword_rank: None,
            exact_rrf_score: ExactRationalV2::new(1, 61).unwrap(),
            pre_rerank_rrf_rank: 0,
        };
        let mut later = base.clone();
        later.vector_rank = None;
        later.keyword_rank = Some(1);
        assert_eq!(compare_candidates(&base, &later), Ordering::Less);
        later = base.clone();
        later.source_chunk_id = Uuid::from_u128(5);
        assert_eq!(compare_candidates(&base, &later), Ordering::Less);
    }

    fn test_revision() -> EmbeddingRevisionV2 {
        EmbeddingRevisionV2 {
            schema_version: EMBEDDING_REVISION_SCHEMA_V2,
            provider_protocol_version: EMBEDDING_PROVIDER_PROTOCOL_VERSION_V2.into(),
            provider_model_identifier: "semantic-v2-test@2025-01-15".into(),
            provider_model_revision_sha256: crate::sha256_hex(Uuid::new_v4().as_bytes()),
            endpoint_config_sha256: crate::sha256_hex(Uuid::new_v4().as_bytes()),
            endpoint_identity: "https://embeddings.example.test/v1/embeddings".into(),
            dimension: EMBEDDING_DIMENSION_V2,
            request_config_sha256: EmbeddingRevisionV2::canonical_request_config_sha256(),
            output_normalization_version: EMBEDDING_OUTPUT_NORMALIZATION_VERSION_V2.into(),
        }
    }

    async fn postgres_pool() -> Option<sqlx::PgPool> {
        let database_url = match std::env::var("KNOWLEDGEBRAIN_TEST_DATABASE_URL") {
            Ok(value) => value,
            Err(_) if std::env::var_os("KNOWLEDGEBRAIN_REQUIRE_POSTGRES_TESTS").is_some() => {
                panic!("KNOWLEDGEBRAIN_TEST_DATABASE_URL is required for semantic V2 PostgreSQL tests")
            }
            Err(_) => return None,
        };
        assert!(
            !database_url.contains(":15432/"),
            "semantic V2 PostgreSQL tests refuse the live :15432 database"
        );
        Some(
            sqlx::postgres::PgPoolOptions::new()
                .max_connections(5)
                .connect(&database_url)
                .await
                .unwrap(),
        )
    }

    async fn wait_for_blocker(pool: &sqlx::PgPool, blocked_pid: i32, blocker_pid: i32) -> bool {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        loop {
            let blockers: Vec<i32> = sqlx::query_scalar("SELECT pg_catalog.pg_blocking_pids($1)")
                .bind(blocked_pid)
                .fetch_one(pool)
                .await
                .unwrap();
            if blockers.contains(&blocker_pid) {
                return true;
            }
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    async fn register_lock_policy(
        pool: &sqlx::PgPool,
    ) -> (RetrievalPolicyIdentityV1, String, String) {
        let revision = EmbeddingRevisionV2 {
            schema_version: EMBEDDING_REVISION_SCHEMA_V2,
            provider_protocol_version: EMBEDDING_PROVIDER_PROTOCOL_VERSION_V2.into(),
            provider_model_identifier: "semantic-v2-lock-test@2025-01-15".into(),
            provider_model_revision_sha256: crate::sha256_hex(b"semantic-v2-lock-model"),
            endpoint_config_sha256: crate::sha256_hex(b"semantic-v2-lock-endpoint"),
            endpoint_identity: "https://embeddings.example.test/v1/embeddings".into(),
            dimension: EMBEDDING_DIMENSION_V2,
            request_config_sha256: EmbeddingRevisionV2::canonical_request_config_sha256(),
            output_normalization_version: EMBEDDING_OUTPUT_NORMALIZATION_VERSION_V2.into(),
        };
        let revision_bytes = revision.canonical_bytes().unwrap();
        let revision_sha = revision.sha256().unwrap();
        sqlx::query("INSERT INTO embedding_revisions_v2(revision_sha256,canonical_revision_payload,schema_version,provider_protocol_version,provider_model_identifier,provider_model_revision_sha256,endpoint_config_sha256,endpoint_identity,dimension,request_config_sha256,output_normalization_version,credential_ref) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,'test:semantic-lock-v2') ON CONFLICT (revision_sha256) DO NOTHING")
            .bind(&revision_sha).bind(&revision_bytes).bind(i16::try_from(revision.schema_version).unwrap()).bind(&revision.provider_protocol_version).bind(&revision.provider_model_identifier).bind(&revision.provider_model_revision_sha256).bind(&revision.endpoint_config_sha256).bind(&revision.endpoint_identity).bind(i32::try_from(revision.dimension).unwrap()).bind(&revision.request_config_sha256).bind(&revision.output_normalization_version).execute(pool).await.unwrap();
        let mut artifact = policy();
        artifact.embedding.model_revision_sha256 = revision_sha.clone();
        artifact.rerank.model_revision_sha256 = crate::sha256_hex(b"semantic-v2-lock-reranker");
        let reranker = RerankRevisionV2 {
            schema_version: RERANK_REVISION_SCHEMA_V2,
            provider_protocol_version: artifact.rerank.provider_protocol_version.clone(),
            provider_model_identifier: "semantic-v2-lock-reranker@2025-01-15".into(),
            provider_model_revision_sha256: artifact.rerank.model_revision_sha256.clone(),
            config_revision_sha256: artifact.rerank.config_revision_sha256.clone(),
            endpoint_identity: "https://rerank.example.test/v1/rerank".into(),
            request_config_sha256: RerankRevisionV2::canonical_request_config_sha256(),
            score_normalization_version: artifact.rerank.score_normalization_version.clone(),
        };
        let reranker_sha = reranker.sha256().unwrap();
        artifact.rerank.revision_sha256 = reranker_sha.clone();
        sqlx::query("INSERT INTO rerank_revisions_v2(revision_sha256,canonical_revision_payload,schema_version,provider_protocol_version,provider_model_identifier,provider_model_revision_sha256,config_revision_sha256,endpoint_identity,request_config_sha256,score_normalization_version,credential_ref) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,'test:semantic-lock-rerank-v2') ON CONFLICT (revision_sha256) DO NOTHING")
            .bind(&reranker_sha).bind(reranker.canonical_bytes().unwrap()).bind(i16::try_from(reranker.schema_version).unwrap()).bind(&reranker.provider_protocol_version).bind(&reranker.provider_model_identifier).bind(&reranker.provider_model_revision_sha256).bind(&reranker.config_revision_sha256).bind(&reranker.endpoint_identity).bind(&reranker.request_config_sha256).bind(&reranker.score_normalization_version).execute(pool).await.unwrap();
        artifact.validate().unwrap();
        let identity = artifact.request_identity().unwrap();
        sqlx::query("INSERT INTO knowledge_retrieval_policies_v2(policy_sha256,canonical_policy_payload,embedding_revision_sha256,contract_version,max_hits,max_chunk_bytes,max_total_bytes) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT (policy_sha256) DO NOTHING")
            .bind(&identity.policy_sha256).bind(artifact.canonical_bytes().unwrap()).bind(&revision_sha).bind(&identity.contract_version).bind(i64::from(identity.max_hits)).bind(i64::from(identity.max_chunk_bytes)).bind(i64::try_from(identity.max_total_bytes).unwrap()).execute(pool).await.unwrap();
        (identity, revision_sha, reranker_sha)
    }

    #[tokio::test]
    async fn runtime_lock_is_shadow_safe_acl_narrow_and_serializes_revocations() {
        let _permit = crate::TEST_PG_SERIAL
            .acquire()
            .await
            .expect("test semaphore closed");
        let Some(pool) = postgres_pool().await else {
            return;
        };
        let ready: bool = sqlx::query_scalar(
            "SELECT to_regprocedure('kb_knowledge_lock_semantic_policy_v2(text)') IS NOT NULL",
        )
        .fetch_one(&pool)
        .await
        .unwrap_or(false);
        if !ready {
            if std::env::var_os("KNOWLEDGEBRAIN_REQUIRE_POSTGRES_TESTS").is_some() {
                panic!("semantic policy lock function is required")
            }
            return;
        }
        let public_execute: bool = sqlx::query_scalar(
            "SELECT EXISTS (
                 SELECT 1 FROM pg_catalog.pg_proc procedure
                 JOIN pg_catalog.pg_namespace namespace ON namespace.oid=procedure.pronamespace
                 CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(
                     procedure.proacl,pg_catalog.acldefault('f',procedure.proowner)
                 )) privilege
                WHERE namespace.nspname='public'
                  AND procedure.proname='kb_knowledge_lock_semantic_policy_v2'
                  AND privilege.grantee=0 AND privilege.privilege_type='EXECUTE')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let runtime_execute: bool = sqlx::query_scalar(
            "SELECT has_function_privilege('kb_runtime_api', 'public.kb_knowledge_lock_semantic_policy_v2(text)', 'EXECUTE')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let runtime_tokenizer_execute: bool = sqlx::query_scalar(
            "SELECT has_function_privilege('kb_runtime_api', 'public.kb_knowledge_keyword_token_stream_v2(text)', 'EXECUTE')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let runtime_registry_update: bool = sqlx::query_scalar(
            "SELECT has_table_privilege('kb_runtime_api', 'public.knowledge_retrieval_policies_v2', 'UPDATE')
                 OR has_table_privilege('kb_runtime_api', 'public.embedding_revisions_v2', 'UPDATE')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(!public_execute);
        assert!(runtime_execute);
        assert!(runtime_tokenizer_execute);
        assert!(!runtime_registry_update);

        let (identity, revision_sha, reranker_sha) = register_lock_policy(&pool).await;
        struct FailingCredentialResolver;
        #[async_trait]
        impl EmbeddingCredentialResolverV2 for FailingCredentialResolver {
            async fn resolve(
                &self,
                _credential_ref: &str,
            ) -> Result<String, KnowledgeRetrievalError> {
                Err(integrity("test embedding provider unavailable"))
            }
        }
        let client = StrictEmbeddingClientV2::new(Arc::new(FailingCredentialResolver)).unwrap();
        let adapter = PostgresKnowledgeRetrievalAdapter::new(pool.clone());
        let provider_failure = adapter
            .retrieve_semantic_candidates_v2("product_line", &[], "alpha", &identity, &client)
            .await;
        assert!(matches!(
            provider_failure,
            Err(KnowledgeRetrievalError::Unavailable(message))
                if message == "test embedding provider unavailable"
        ));

        let mut locked = pool.begin().await.unwrap();
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .execute(&mut *locked)
            .await
            .unwrap();
        sqlx::query("CREATE TEMP TABLE knowledge_retrieval_policies_v2 (LIKE public.knowledge_retrieval_policies_v2 INCLUDING ALL) ON COMMIT DROP")
            .execute(&mut *locked).await.unwrap();
        sqlx::query("CREATE TEMP TABLE embedding_revisions_v2 (LIKE public.embedding_revisions_v2 INCLUDING ALL) ON COMMIT DROP")
            .execute(&mut *locked).await.unwrap();
        sqlx::query("SET LOCAL search_path = pg_temp, public, pg_catalog")
            .execute(&mut *locked)
            .await
            .unwrap();
        sqlx::query("SET LOCAL ROLE kb_runtime_api")
            .execute(&mut *locked)
            .await
            .unwrap();
        let token_stream: String =
            sqlx::query_scalar("SELECT public.kb_knowledge_keyword_token_stream_v2('Alpha知识')")
                .fetch_one(&mut *locked)
                .await
                .unwrap();
        assert_eq!(token_stream, "alpha 知识");
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM public.kb_knowledge_lock_semantic_policy_v2($1)",
        )
        .bind(&identity.policy_sha256)
        .fetch_one(&mut *locked)
        .await
        .unwrap();
        assert_eq!(count, 1, "temporary registry shadows must be ignored");
        let locked_pid: i32 = sqlx::query_scalar("SELECT pg_catalog.pg_backend_pid()")
            .fetch_one(&mut *locked)
            .await
            .unwrap();

        let policy_pool = pool.clone();
        let policy_sha = identity.policy_sha256.clone();
        let (policy_pid_sender, policy_pid_receiver) = tokio::sync::oneshot::channel();
        let mut policy_revoke = tokio::spawn(async move {
            let mut tx = policy_pool.begin().await.unwrap();
            let pid: i32 = sqlx::query_scalar("SELECT pg_catalog.pg_backend_pid()")
                .fetch_one(&mut *tx)
                .await
                .unwrap();
            let _ = policy_pid_sender.send(pid);
            sqlx::query("UPDATE public.knowledge_retrieval_policies_v2 SET support_state='revoked' WHERE policy_sha256=$1")
                .bind(policy_sha).execute(&mut *tx).await.unwrap();
            tx.rollback().await.unwrap();
        });
        let revision_pool = pool.clone();
        let (revision_pid_sender, revision_pid_receiver) = tokio::sync::oneshot::channel();
        let mut revision_revoke = tokio::spawn(async move {
            let mut tx = revision_pool.begin().await.unwrap();
            let pid: i32 = sqlx::query_scalar("SELECT pg_catalog.pg_backend_pid()")
                .fetch_one(&mut *tx)
                .await
                .unwrap();
            let _ = revision_pid_sender.send(pid);
            sqlx::query("UPDATE public.embedding_revisions_v2 SET support_state='revoked' WHERE revision_sha256=$1")
                .bind(revision_sha).execute(&mut *tx).await.unwrap();
            tx.rollback().await.unwrap();
        });
        let reranker_pool = pool.clone();
        let (reranker_pid_sender, reranker_pid_receiver) = tokio::sync::oneshot::channel();
        let mut reranker_revoke = tokio::spawn(async move {
            let mut tx = reranker_pool.begin().await.unwrap();
            let pid: i32 = sqlx::query_scalar("SELECT pg_catalog.pg_backend_pid()")
                .fetch_one(&mut *tx)
                .await
                .unwrap();
            let _ = reranker_pid_sender.send(pid);
            sqlx::query("UPDATE public.rerank_revisions_v2 SET support_state='revoked' WHERE revision_sha256=$1")
                .bind(reranker_sha).execute(&mut *tx).await.unwrap();
            tx.rollback().await.unwrap();
        });
        let policy_pid = tokio::time::timeout(Duration::from_secs(3), policy_pid_receiver)
            .await
            .unwrap()
            .unwrap();
        let revision_pid = tokio::time::timeout(Duration::from_secs(3), revision_pid_receiver)
            .await
            .unwrap()
            .unwrap();
        let reranker_pid = tokio::time::timeout(Duration::from_secs(3), reranker_pid_receiver)
            .await
            .unwrap()
            .unwrap();
        let policy_blocked = wait_for_blocker(&pool, policy_pid, locked_pid).await;
        let revision_blocked = wait_for_blocker(&pool, revision_pid, locked_pid).await;
        let reranker_blocked = wait_for_blocker(&pool, reranker_pid, locked_pid).await;

        locked.rollback().await.unwrap();
        let policy_join = tokio::time::timeout(Duration::from_secs(3), &mut policy_revoke).await;
        if policy_join.is_err() {
            policy_revoke.abort();
            let _ = policy_revoke.await;
        }
        let revision_join =
            tokio::time::timeout(Duration::from_secs(3), &mut revision_revoke).await;
        if revision_join.is_err() {
            revision_revoke.abort();
            let _ = revision_revoke.await;
        }
        let reranker_join =
            tokio::time::timeout(Duration::from_secs(3), &mut reranker_revoke).await;
        if reranker_join.is_err() {
            reranker_revoke.abort();
            let _ = reranker_revoke.await;
        }
        assert!(
            policy_blocked,
            "policy revocation must wait for the recall lock"
        );
        assert!(
            revision_blocked,
            "embedding revision revocation must wait for the recall lock"
        );
        assert!(
            reranker_blocked,
            "rerank revision revocation must wait for the recall lock"
        );
        policy_join.unwrap().unwrap();
        revision_join.unwrap().unwrap();
        reranker_join.unwrap().unwrap();

        let states: (String, String, String) = sqlx::query_as(
            "SELECT policy.support_state,revision.support_state,reranker.support_state
               FROM public.knowledge_retrieval_policies_v2 policy
               JOIN public.embedding_revisions_v2 revision
                 ON revision.revision_sha256=policy.embedding_revision_sha256
               JOIN public.rerank_revisions_v2 reranker
                 ON reranker.revision_sha256=policy.rerank_revision_sha256
              WHERE policy.policy_sha256=$1",
        )
        .bind(&identity.policy_sha256)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            states,
            ("supported".into(), "supported".into(), "supported".into())
        );
    }

    #[tokio::test]
    async fn postgres_folding_recall_repeatability_exclusion_and_integrity_are_fail_closed() {
        let _permit = crate::TEST_PG_SERIAL
            .acquire()
            .await
            .expect("test semaphore closed");
        let Some(pool) = postgres_pool().await else {
            return;
        };
        let ready: bool = sqlx::query_scalar(
            "SELECT to_regclass('chunk_vector_indexes_v2') IS NOT NULL
                 AND to_regclass('graph_nodes') IS NOT NULL
                 AND to_regclass('wiki_pages') IS NOT NULL",
        )
        .fetch_one(&pool)
        .await
        .unwrap_or(false);
        if !ready {
            if std::env::var_os("KNOWLEDGEBRAIN_REQUIRE_POSTGRES_TESTS").is_some() {
                panic!("final semantic V2 schema is required")
            }
            return;
        }

        let mut tx = pool.begin().await.unwrap();
        let workspace_id = Uuid::new_v4();
        let product_id = Uuid::new_v4();
        let version_id = Uuid::new_v4();
        let document_id = Uuid::new_v4();
        let other_document_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO workspaces(id,name,slug,kind) VALUES($1,'semantic v2',$2,'product_line')",
        )
        .bind(workspace_id)
        .bind(format!("semantic-v2-{workspace_id}"))
        .execute(&mut *tx)
        .await
        .unwrap();
        sqlx::query("INSERT INTO products(id,workspace_id,kind,name,slug) VALUES($1,$2,'product','semantic v2',$3)")
            .bind(product_id).bind(workspace_id).bind(format!("semantic-v2-{product_id}")).execute(&mut *tx).await.unwrap();
        sqlx::query(
            "INSERT INTO product_versions(id,product_id,label,status) VALUES($1,$2,'v1','active')",
        )
        .bind(version_id)
        .bind(product_id)
        .execute(&mut *tx)
        .await
        .unwrap();
        sqlx::query("UPDATE products SET current_version_id=$2 WHERE id=$1")
            .bind(product_id)
            .bind(version_id)
            .execute(&mut *tx)
            .await
            .unwrap();
        for id in [document_id, other_document_id] {
            let digest = crate::sha256_hex(id.as_bytes());
            let object_ref = format!("objects/{digest}");
            sqlx::query("INSERT INTO object_registry(object_ref,digest,media_type,byte_length,state) VALUES($1,$2,'text/plain',1,'available')")
                .bind(&object_ref).bind(&digest).execute(&mut *tx).await.unwrap();
            sqlx::query("INSERT INTO object_owner_references(object_ref,owner_kind,owner_id,occurrence,created_by) VALUES($1,'knowledge_document',$2,'original','system:knowledge-document-ingest')")
                .bind(&object_ref).bind(id).execute(&mut *tx).await.unwrap();
            sqlx::query("INSERT INTO documents(id,product_version_id,title,parse_status,enable_status,index_ready,file_name,file_size,file_hash,object_ref) VALUES($1,$2,'semantic','completed','enabled',true,$3,1,$4,$5)")
                .bind(id).bind(version_id).bind(format!("{id}.txt")).bind(&digest).bind(&object_ref).execute(&mut *tx).await.unwrap();
        }

        let source_one = Uuid::new_v4();
        let source_two = Uuid::new_v4();
        let source_ambiguous = Uuid::new_v4();
        let cross_source = Uuid::new_v4();
        let direct = Uuid::new_v4();
        let exact_source = Uuid::new_v4();
        let ocr = Uuid::new_v4();
        let ocr_ambiguous_one = Uuid::new_v4();
        let ocr_ambiguous_two = Uuid::new_v4();
        let question = Uuid::new_v4();
        let summary = Uuid::new_v4();
        let caption = Uuid::new_v4();
        let ambiguous_caption = Uuid::new_v4();
        let graph = Uuid::new_v4();
        let ambiguous_graph = Uuid::new_v4();
        let wiki = Uuid::new_v4();
        let cross_question = Uuid::new_v4();
        let derived_parent = Uuid::new_v4();
        let derived_child = Uuid::new_v4();
        let chunks = vec![
            (
                source_one,
                document_id,
                "text",
                "trusted source one",
                "",
                None,
            ),
            (
                source_two,
                document_id,
                "parent_text",
                "trusted source two",
                "",
                None,
            ),
            (
                source_ambiguous,
                document_id,
                "text",
                "ambiguous only",
                "",
                None,
            ),
            (
                cross_source,
                other_document_id,
                "text",
                "cross source",
                "",
                None,
            ),
            (
                direct,
                document_id,
                "text",
                "vector-only direct trusted",
                "manual context",
                None,
            ),
            (
                exact_source,
                document_id,
                "text",
                "trusted A L P H A exact source",
                "",
                None,
            ),
            (
                ocr,
                document_id,
                "image_ocr",
                "trusted image ocr",
                "img-1",
                None,
            ),
            (
                ocr_ambiguous_one,
                document_id,
                "image_ocr",
                "ambiguous ocr one",
                "img-2",
                None,
            ),
            (
                ocr_ambiguous_two,
                document_id,
                "image_ocr",
                "ambiguous ocr two",
                "img-2",
                None,
            ),
            (
                question,
                document_id,
                "question",
                "alpha parent question",
                "",
                Some(source_one),
            ),
            (
                summary,
                document_id,
                "summary",
                "alpha parent summary",
                "",
                Some(source_one),
            ),
            (
                caption,
                document_id,
                "image_caption",
                "alpha image caption",
                "img-1",
                None,
            ),
            (
                ambiguous_caption,
                document_id,
                "image_caption",
                "alpha ambiguous caption",
                "img-2",
                None,
            ),
            (
                graph,
                document_id,
                "graph_node",
                "alpha graph",
                "alpha graph",
                None,
            ),
            (
                ambiguous_graph,
                document_id,
                "graph_node",
                "alpha ambiguous graph",
                "alpha ambiguous graph",
                None,
            ),
            (
                wiki,
                document_id,
                "wiki_page",
                "alpha wiki",
                "semantic-page",
                None,
            ),
            (
                cross_question,
                document_id,
                "question",
                "alpha cross question",
                "",
                Some(cross_source),
            ),
            (
                derived_parent,
                document_id,
                "question",
                "derived parent",
                "",
                Some(source_one),
            ),
            (
                derived_child,
                document_id,
                "question",
                "alpha derived child",
                "",
                Some(derived_parent),
            ),
        ];
        for (id, doc, kind, content, header, parent) in &chunks {
            sqlx::query("INSERT INTO chunks(id,product_version_id,document_id,chunk_type,content,context_header,parent_chunk_id) VALUES($1,$2,$3,$4,$5,$6,$7)")
                .bind(id).bind(version_id).bind(doc).bind(kind).bind(content).bind(header).bind(parent).execute(&mut *tx).await.unwrap();
        }
        let image_artifact_id = Uuid::new_v4();
        let image_content_sha = "1111111111111111111111111111111111111111111111111111111111111111";
        let image_object_ref = format!("objects/{image_content_sha}");
        let canonical_media = br#"{"schema_version":3,"fixture":"semantic-v2"}"#;
        sqlx::query(
            "INSERT INTO object_registry(object_ref,digest,media_type,byte_length,state)
             VALUES($1,$2,'image/png',1,'available')",
        )
        .bind(&image_object_ref)
        .bind(image_content_sha)
        .execute(&mut *tx)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO knowledge_image_artifact_revisions(
               id,product_version_id,document_id,revision,object_ref,content_sha256,
               media_type,width,height,source_image_key,canonical_payload,artifact_sha256)
             VALUES($1,$2,$3,1,$4,$5,'image/png',1,1,'semantic-v2-fixture',$6,
                    encode(public.digest($6,'sha256'),'hex'))",
        )
        .bind(image_artifact_id)
        .bind(version_id)
        .bind(document_id)
        .bind(&image_object_ref)
        .bind(image_content_sha)
        .bind(canonical_media.as_slice())
        .execute(&mut *tx)
        .await
        .unwrap();
        for image_chunk_id in [ocr, ocr_ambiguous_one, ocr_ambiguous_two] {
            sqlx::query(
                "INSERT INTO knowledge_image_ocr_chunk_artifact_mappings(
                   chunk_id,product_version_id,document_id,image_artifact_revision_id,
                   object_ref,content_sha256,media_type)
                 VALUES($1,$2,$3,$4,$5,$6,'image/png')",
            )
            .bind(image_chunk_id)
            .bind(version_id)
            .bind(document_id)
            .bind(image_artifact_id)
            .bind(&image_object_ref)
            .bind(image_content_sha)
            .execute(&mut *tx)
            .await
            .unwrap();
        }
        sqlx::query("INSERT INTO graph_nodes(product_version_id,document_id,name,chunk_ids) VALUES($1,$2,'alpha graph',$3),($1,$2,'alpha ambiguous graph',$4)")
            .bind(version_id).bind(document_id).bind(vec![source_two]).bind(vec![source_ambiguous, source_one]).execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO wiki_pages(id,product_version_id,slug,title,status,content,chunk_refs) VALUES($1,$2,'semantic-page','semantic','published','alpha wiki',$3)")
            .bind(Uuid::new_v4()).bind(version_id).bind(serde_json::json!([source_two.to_string()])).execute(&mut *tx).await.unwrap();

        let revision = test_revision();
        let revision_bytes = revision.canonical_bytes().unwrap();
        let revision_sha = revision.sha256().unwrap();
        sqlx::query("INSERT INTO embedding_revisions_v2(revision_sha256,canonical_revision_payload,schema_version,provider_protocol_version,provider_model_identifier,provider_model_revision_sha256,endpoint_config_sha256,endpoint_identity,dimension,request_config_sha256,output_normalization_version,credential_ref) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,'test:semantic-v2')")
            .bind(&revision_sha).bind(&revision_bytes).bind(i16::try_from(revision.schema_version).unwrap()).bind(&revision.provider_protocol_version).bind(&revision.provider_model_identifier).bind(&revision.provider_model_revision_sha256).bind(&revision.endpoint_config_sha256).bind(&revision.endpoint_identity).bind(i32::try_from(revision.dimension).unwrap()).bind(&revision.request_config_sha256).bind(&revision.output_normalization_version).execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO product_version_embedding_bindings_v2(product_version_id,embedding_revision_sha256) VALUES($1,$2)")
            .bind(version_id).bind(&revision_sha).execute(&mut *tx).await.unwrap();
        let lifecycle: (String, Option<Uuid>, Option<i64>) = sqlx::query_as(
            "SELECT state,intent_id,target_revision FROM kb_knowledge_prepare_semantic_index_intent_v2($1)",
        )
        .bind(version_id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        assert_eq!(lifecycle.0, "enqueue");
        let lifecycle_intent_id = lifecycle.1.unwrap();
        let lifecycle_target_revision = lifecycle.2.unwrap();
        let lifecycle_source_snapshot: String = sqlx::query_scalar(
            "SELECT source_snapshot_sha256 FROM knowledge_semantic_index_intents_v2 WHERE id=$1",
        )
        .bind(lifecycle_intent_id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        sqlx::query("SELECT kb_knowledge_rebuild_semantic_keyword_indexes_v2($1,$2,$3)")
            .bind(version_id)
            .bind(&revision_sha)
            .bind(&lifecycle_source_snapshot)
            .execute(&mut *tx)
            .await
            .unwrap();
        let query_values = {
            let mut values = vec![0.0f32; 1024];
            values[0] = 1.0;
            values
        };
        let provider = SemanticFixtureVectorProvider {
            other_vector_ids: chunks
                .iter()
                .filter_map(|(id, _, kind, _, _, _)| {
                    (matches!(*kind, "text" | "parent_text" | "image_ocr") && *id != direct)
                        .then_some(*id)
                })
                .collect(),
            calls: AtomicUsize::new(0),
        };
        let generation = crate::knowledge_index_v2::rebuild_vector_indexes_v2_in_transaction(
            &mut tx, version_id, &provider,
        )
        .await
        .unwrap();
        assert_eq!(generation.chunk_count, u32::try_from(chunks.len()).unwrap());
        assert_eq!(provider.calls.load(AtomicOrdering::SeqCst), 1);
        let lifecycle_completed: String =
            sqlx::query_scalar("SELECT kb_knowledge_complete_semantic_index_intent_v2($1,$2)")
                .bind(lifecycle_intent_id)
                .bind(lifecycle_target_revision)
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        assert_eq!(lifecycle_completed, "completed");
        let direct_digest: String = sqlx::query_scalar(
            "SELECT indexed_content_sha256 FROM chunk_vector_indexes_v2
              WHERE product_version_id=$1 AND chunk_id=$2",
        )
        .bind(version_id)
        .bind(direct)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        assert_eq!(
            direct_digest,
            crate::knowledge_index_v2::canonical_embedding_input_sha256_v2(
                "manual context",
                "vector-only direct trusted"
            )
        );
        let mut p = policy();
        p.embedding.model_revision_sha256 = revision_sha.clone();
        let reranker = RerankRevisionV2 {
            schema_version: RERANK_REVISION_SCHEMA_V2,
            provider_protocol_version: p.rerank.provider_protocol_version.clone(),
            provider_model_identifier: "semantic-v2-reranker@2025-01-15".into(),
            provider_model_revision_sha256: p.rerank.model_revision_sha256.clone(),
            config_revision_sha256: p.rerank.config_revision_sha256.clone(),
            endpoint_identity: "https://rerank.example.test/v1/rerank".into(),
            request_config_sha256: RerankRevisionV2::canonical_request_config_sha256(),
            score_normalization_version: p.rerank.score_normalization_version.clone(),
        };
        p.rerank.revision_sha256 = reranker.sha256().unwrap();
        sqlx::query("INSERT INTO rerank_revisions_v2(revision_sha256,canonical_revision_payload,schema_version,provider_protocol_version,provider_model_identifier,provider_model_revision_sha256,config_revision_sha256,endpoint_identity,request_config_sha256,score_normalization_version,credential_ref) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,'test:semantic-rerank-v2')")
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
            .execute(&mut *tx).await.unwrap();
        p.validate().unwrap();
        let policy_identity = p.request_identity().unwrap();
        sqlx::query("INSERT INTO knowledge_retrieval_policies_v2(policy_sha256,canonical_policy_payload,embedding_revision_sha256,contract_version,max_hits,max_chunk_bytes,max_total_bytes) VALUES($1,$2,$3,$4,$5,$6,$7)")
            .bind(&policy_identity.policy_sha256)
            .bind(p.canonical_bytes().unwrap())
            .bind(&revision_sha)
            .bind(&policy_identity.contract_version)
            .bind(i64::from(policy_identity.max_hits))
            .bind(i64::from(policy_identity.max_chunk_bytes))
            .bind(i64::try_from(policy_identity.max_total_bytes).unwrap())
            .execute(&mut *tx).await.unwrap();
        sqlx::query("SET LOCAL ROLE kb_runtime_api")
            .execute(&mut *tx)
            .await
            .unwrap();
        let locked: i64 =
            sqlx::query_scalar("SELECT count(*) FROM kb_knowledge_lock_semantic_policy_v2($1)")
                .bind(&policy_identity.policy_sha256)
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        assert_eq!(
            locked, 1,
            "runtime SELECT-only role can acquire semantic registry locks"
        );
        sqlx::query("RESET ROLE").execute(&mut *tx).await.unwrap();
        let first = recall_in_snapshot(
            &mut tx,
            "product_line",
            &[version_id],
            "alpha",
            &p,
            &QueryEmbeddingV2 {
                revision_sha256: revision_sha.clone(),
                values: query_values.clone(),
            },
        )
        .await
        .unwrap();
        let second = recall_in_snapshot(
            &mut tx,
            "product_line",
            &[version_id],
            "alpha",
            &p,
            &QueryEmbeddingV2 {
                revision_sha256: revision_sha.clone(),
                values: query_values.clone(),
            },
        )
        .await
        .unwrap();
        assert_eq!(first, second);
        let ids = first
            .iter()
            .map(|candidate| candidate.source_chunk_id)
            .collect::<HashSet<_>>();
        assert_eq!(ids, HashSet::from([source_one, source_two, direct, ocr]));
        assert!(
            !ids.contains(&exact_source),
            "every exact trusted source is excluded internally from raw C"
        );
        assert!(!ids.contains(&source_ambiguous));
        assert!(!ids.contains(&cross_source));
        assert!(
            first
                .iter()
                .all(|candidate| candidate.pre_rerank_rrf_rank > 0)
        );

        sqlx::query("UPDATE graph_nodes SET chunk_ids=ARRAY[NULL]::uuid[] WHERE product_version_id=$1 AND document_id=$2 AND name='alpha graph'")
            .bind(version_id).bind(document_id).execute(&mut *tx).await.unwrap();
        let null_graph_ref = recall_in_snapshot(
            &mut tx,
            "product_line",
            &[version_id],
            "alpha",
            &p,
            &QueryEmbeddingV2 {
                revision_sha256: revision_sha.clone(),
                values: query_values.clone(),
            },
        )
        .await;
        assert!(matches!(
            null_graph_ref,
            Err(KnowledgeRetrievalError::Unavailable(_))
        ));
        sqlx::query("UPDATE graph_nodes SET chunk_ids=$3 WHERE product_version_id=$1 AND document_id=$2 AND name='alpha graph'")
            .bind(version_id).bind(document_id).bind(vec![source_two]).execute(&mut *tx).await.unwrap();

        let empty_requirement = recall_in_snapshot(
            &mut tx,
            "product_line",
            &[version_id],
            " \t\n\u{2003}",
            &p,
            &QueryEmbeddingV2 {
                revision_sha256: revision_sha.clone(),
                values: query_values.clone(),
            },
        )
        .await;
        assert!(matches!(
            empty_requirement,
            Err(KnowledgeRetrievalError::InvalidRequest(_))
        ));

        let mut inclusive = p.clone();
        inclusive.keyword.threshold_millionths = 1_000_000;
        inclusive.embedding.threshold_millionths = 1_000_000;
        let inclusive_hits = recall_in_snapshot(
            &mut tx,
            "product_line",
            &[version_id],
            "alpha",
            &inclusive,
            &QueryEmbeddingV2 {
                revision_sha256: revision_sha.clone(),
                values: query_values.clone(),
            },
        )
        .await
        .unwrap();
        assert!(
            !inclusive_hits.is_empty(),
            "millionth threshold is inclusive"
        );
        let mut top_one = p.clone();
        top_one.keyword.top_k = 1;
        top_one.embedding.top_k = 1;
        let capped_channels = recall_in_snapshot(
            &mut tx,
            "product_line",
            &[version_id],
            "alpha",
            &top_one,
            &QueryEmbeddingV2 {
                revision_sha256: revision_sha.clone(),
                values: query_values.clone(),
            },
        )
        .await
        .unwrap();
        assert!(capped_channels.len() <= 2);

        let mut mismatched = p.clone();
        mismatched.embedding.model_revision_sha256 = "f".repeat(64);
        let binding_error = recall_in_snapshot(
            &mut tx,
            "product_line",
            &[version_id],
            "alpha",
            &mismatched,
            &QueryEmbeddingV2 {
                revision_sha256: "f".repeat(64),
                values: query_values.clone(),
            },
        )
        .await;
        assert!(matches!(
            binding_error,
            Err(KnowledgeRetrievalError::InvalidRequest(_))
        ));

        sqlx::query(
            "UPDATE chunk_vector_indexes_v2 SET indexed_content_sha256=$2 WHERE chunk_id=$1",
        )
        .bind(question)
        .bind("f".repeat(64))
        .execute(&mut *tx)
        .await
        .unwrap();
        let stale_vector = recall_in_snapshot(
            &mut tx,
            "product_line",
            &[version_id],
            "alpha",
            &p,
            &QueryEmbeddingV2 {
                revision_sha256: revision_sha.clone(),
                values: query_values.clone(),
            },
        )
        .await;
        assert!(matches!(
            stale_vector,
            Err(KnowledgeRetrievalError::Unavailable(_))
        ));
        sqlx::query(
            "UPDATE chunk_vector_indexes_v2 SET indexed_content_sha256=$2 WHERE chunk_id=$1",
        )
        .bind(question)
        .bind(
            crate::knowledge_index_v2::canonical_embedding_input_sha256_v2(
                "",
                "alpha parent question",
            ),
        )
        .execute(&mut *tx)
        .await
        .unwrap();

        sqlx::query("DELETE FROM chunk_keyword_indexes_v2 WHERE chunk_id=$1")
            .bind(question)
            .execute(&mut *tx)
            .await
            .unwrap();
        let stale = recall_in_snapshot(
            &mut tx,
            "product_line",
            &[version_id],
            "alpha",
            &p,
            &QueryEmbeddingV2 {
                revision_sha256: revision_sha.clone(),
                values: query_values,
            },
        )
        .await;
        assert!(matches!(
            stale,
            Err(KnowledgeRetrievalError::Unavailable(_))
        ));
        tx.rollback().await.unwrap();
    }
}
