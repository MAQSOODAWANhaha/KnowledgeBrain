use crate::knowledge_retrieval::EmbeddingRevisionV2;
use async_trait::async_trait;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Row, Transaction};
use std::{collections::HashSet, sync::Arc, time::Duration};
use uuid::Uuid;

pub const VECTOR_INDEX_DIMENSION_V2: usize = 1024;
const VECTOR_EMBEDDING_BATCH_MAX_INPUTS_V2: usize = 64;
const VECTOR_EMBEDDING_MAX_VERSION_INPUTS_V2: usize = 16_384;
const VECTOR_EMBEDDING_MAX_INPUT_BYTES_V2: usize = 1024 * 1024;
const VECTOR_EMBEDDING_MAX_REQUEST_BYTES_V2: usize = 8 * 1024 * 1024;
const VECTOR_EMBEDDING_MAX_BATCH_RESPONSE_BYTES_V2: usize = 64 * 1024 * 1024;
const VECTOR_EMBEDDING_MAX_TOTAL_RESPONSE_BYTES_V2: usize = 256 * 1024 * 1024;
const SIGNAL_TYPES_V2: [&str; 8] = [
    "text",
    "parent_text",
    "image_ocr",
    "question",
    "summary",
    "image_caption",
    "graph_node",
    "wiki_page",
];

#[derive(Debug, thiserror::Error)]
pub enum VectorIndexErrorV2 {
    #[error("invalid vector-index configuration: {0}")]
    InvalidConfiguration(String),
    #[error("vector-index provider unavailable: {0}")]
    Unavailable(String),
    #[error("vector-index source has pending derived work: {0}")]
    PendingDerived(String),
    #[error("vector-index source snapshot changed: {0}")]
    SnapshotChanged(String),
    #[error("vector-index database error: {0}")]
    Database(#[source] sqlx::Error),
}

impl VectorIndexErrorV2 {
    pub fn retryable(&self) -> bool {
        matches!(
            self,
            Self::Unavailable(_) | Self::PendingDerived(_) | Self::SnapshotChanged(_)
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct VectorEmbeddingInputV2 {
    pub chunk_id: Uuid,
    pub canonical_input: String,
    pub indexed_content_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VectorIndexGenerationV2 {
    pub product_version_id: Uuid,
    pub embedding_revision_sha256: String,
    pub source_snapshot_sha256: String,
    pub chunk_count: u32,
}

#[async_trait]
pub trait EmbeddingCredentialResolverV2: Send + Sync {
    async fn resolve(&self, credential_ref: &str) -> Result<String, VectorIndexErrorV2>;
}

/// Resolves an immutable registry reference of the form `env:VARIABLE_NAME`.
/// Queue payloads, intent rows, traces, and errors retain only the reference;
/// the secret value is read at provider-call time and is never persisted.
#[derive(Clone, Copy, Debug, Default)]
pub struct EnvironmentEmbeddingCredentialResolverV2;

#[async_trait]
impl EmbeddingCredentialResolverV2 for EnvironmentEmbeddingCredentialResolverV2 {
    async fn resolve(&self, credential_ref: &str) -> Result<String, VectorIndexErrorV2> {
        let Some(variable) = credential_ref.strip_prefix("env:") else {
            return Err(VectorIndexErrorV2::InvalidConfiguration(
                "embedding credential reference must use env:<VARIABLE_NAME>".into(),
            ));
        };
        if variable.is_empty()
            || variable.len() > 128
            || !variable
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        {
            return Err(VectorIndexErrorV2::InvalidConfiguration(
                "embedding credential environment reference is invalid".into(),
            ));
        }
        std::env::var(variable)
            .ok()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                VectorIndexErrorV2::InvalidConfiguration(
                    "embedding credential reference is not configured".into(),
                )
            })
    }
}

#[async_trait]
pub trait VectorEmbeddingProviderV2: Send + Sync {
    async fn embed_batch(
        &self,
        revision: &EmbeddingRevisionV2,
        credential_ref: &str,
        inputs: &[VectorEmbeddingInputV2],
    ) -> Result<Vec<Vec<f32>>, VectorIndexErrorV2>;
}

struct VectorEmbeddingTransportRequestV2 {
    endpoint: String,
    credential: String,
    model: String,
    request_config_sha256: String,
    inputs: Vec<String>,
}

struct VectorEmbeddingTransportResponseV2 {
    status: reqwest::StatusCode,
    content_length: Option<u64>,
    bytes: Vec<u8>,
}

#[async_trait]
trait VectorEmbeddingTransportV2: Send + Sync {
    async fn send(
        &self,
        request: VectorEmbeddingTransportRequestV2,
    ) -> Result<VectorEmbeddingTransportResponseV2, VectorIndexErrorV2>;
}

struct ReqwestVectorEmbeddingTransportV2 {
    http: reqwest::Client,
    max_response_bytes: usize,
}

#[async_trait]
impl VectorEmbeddingTransportV2 for ReqwestVectorEmbeddingTransportV2 {
    async fn send(
        &self,
        request: VectorEmbeddingTransportRequestV2,
    ) -> Result<VectorEmbeddingTransportResponseV2, VectorIndexErrorV2> {
        let response = self
            .http
            .post(request.endpoint)
            .bearer_auth(request.credential)
            .json(&serde_json::json!({
                "model": request.model,
                "input": request.inputs,
                "dimensions": VECTOR_INDEX_DIMENSION_V2,
                "request_config_sha256": request.request_config_sha256,
            }))
            .send()
            .await
            .map_err(|error| {
                VectorIndexErrorV2::Unavailable(format!(
                    "embedding provider request failed: {error}"
                ))
            })?;
        let response = crate::knowledge_retrieval_pg::http_v2::read_bounded_response_body_v2(
            response,
            self.max_response_bytes,
        )
        .await
        .map_err(|error| match error {
            crate::knowledge_retrieval_pg::http_v2::BoundedBodyErrorV2::TooLarge => {
                VectorIndexErrorV2::Unavailable(
                    "embedding provider response exceeds byte limit".into(),
                )
            }
            crate::knowledge_retrieval_pg::http_v2::BoundedBodyErrorV2::Transport(error) => {
                VectorIndexErrorV2::Unavailable(format!(
                    "embedding provider response failed: {error}"
                ))
            }
        })?;
        Ok(VectorEmbeddingTransportResponseV2 {
            status: response.status,
            content_length: response.content_length,
            bytes: response.bytes,
        })
    }
}

/// Strict HTTPS OpenAI-compatible indexed-array transport for V2 document vectors.
/// The immutable revision owns endpoint, model, dimensions, and request identity.
#[derive(Clone)]
pub struct StrictVectorEmbeddingClientV2 {
    credentials: Arc<dyn EmbeddingCredentialResolverV2>,
    transport: Arc<dyn VectorEmbeddingTransportV2>,
}

impl StrictVectorEmbeddingClientV2 {
    pub fn new(
        credentials: Arc<dyn EmbeddingCredentialResolverV2>,
    ) -> Result<Self, VectorIndexErrorV2> {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|error| {
                VectorIndexErrorV2::InvalidConfiguration(format!(
                    "failed to configure embedding client: {error}"
                ))
            })?;
        Ok(Self {
            credentials,
            transport: Arc::new(ReqwestVectorEmbeddingTransportV2 {
                http,
                max_response_bytes: VECTOR_EMBEDDING_MAX_BATCH_RESPONSE_BYTES_V2,
            }),
        })
    }

    #[cfg(test)]
    fn with_transport(
        credentials: Arc<dyn EmbeddingCredentialResolverV2>,
        transport: Arc<dyn VectorEmbeddingTransportV2>,
    ) -> Self {
        Self {
            credentials,
            transport,
        }
    }
}

#[async_trait]
impl VectorEmbeddingProviderV2 for StrictVectorEmbeddingClientV2 {
    async fn embed_batch(
        &self,
        revision: &EmbeddingRevisionV2,
        credential_ref: &str,
        inputs: &[VectorEmbeddingInputV2],
    ) -> Result<Vec<Vec<f32>>, VectorIndexErrorV2> {
        revision.validate().map_err(|error| {
            VectorIndexErrorV2::InvalidConfiguration(format!("invalid embedding revision: {error}"))
        })?;
        if !revision.endpoint_identity.starts_with("https://") {
            return Err(VectorIndexErrorV2::InvalidConfiguration(
                "embedding endpoint must use https".into(),
            ));
        }
        if inputs.is_empty() {
            return Ok(Vec::new());
        }
        validate_input_bounds(inputs)?;
        let credential = self.credentials.resolve(credential_ref).await?;
        if credential.is_empty() {
            return Err(VectorIndexErrorV2::InvalidConfiguration(
                "embedding credential resolution returned an empty credential".into(),
            ));
        }

        let mut embeddings = Vec::with_capacity(inputs.len());
        let mut total_response_bytes = 0usize;
        for batch in inputs.chunks(VECTOR_EMBEDDING_BATCH_MAX_INPUTS_V2) {
            let response = self
                .transport
                .send(VectorEmbeddingTransportRequestV2 {
                    endpoint: revision.endpoint_identity.clone(),
                    credential: credential.clone(),
                    model: revision.provider_model_identifier.clone(),
                    request_config_sha256: revision.request_config_sha256.clone(),
                    inputs: batch
                        .iter()
                        .map(|value| value.canonical_input.clone())
                        .collect(),
                })
                .await?;
            if !response.status.is_success() {
                return Err(provider_status_error(response.status));
            }
            if response
                .content_length
                .is_some_and(|length| length > VECTOR_EMBEDDING_MAX_BATCH_RESPONSE_BYTES_V2 as u64)
                || response.bytes.len() > VECTOR_EMBEDDING_MAX_BATCH_RESPONSE_BYTES_V2
            {
                return Err(VectorIndexErrorV2::Unavailable(
                    "embedding provider response exceeds byte limit".into(),
                ));
            }
            total_response_bytes = total_response_bytes
                .checked_add(response.bytes.len())
                .ok_or_else(|| {
                    VectorIndexErrorV2::Unavailable(
                        "embedding provider response byte count overflow".into(),
                    )
                })?;
            if total_response_bytes > VECTOR_EMBEDDING_MAX_TOTAL_RESPONSE_BYTES_V2 {
                return Err(VectorIndexErrorV2::Unavailable(
                    "embedding provider version response exceeds byte limit".into(),
                ));
            }
            embeddings.extend(parse_batch_response(
                &response.bytes,
                revision,
                batch.len(),
            )?);
        }
        if embeddings.len() != inputs.len() {
            return Err(VectorIndexErrorV2::Unavailable(
                "embedding provider returned partial batch output".into(),
            ));
        }
        Ok(embeddings)
    }
}

#[derive(Deserialize)]
struct EmbeddingBatchResponseV2 {
    data: Vec<EmbeddingBatchDataV2>,
    model: String,
    model_revision_sha256: String,
    request_config_sha256: String,
}

#[derive(Deserialize)]
struct EmbeddingBatchDataV2 {
    index: usize,
    embedding: Vec<f32>,
}

fn parse_batch_response(
    bytes: &[u8],
    revision: &EmbeddingRevisionV2,
    expected_count: usize,
) -> Result<Vec<Vec<f32>>, VectorIndexErrorV2> {
    let response: EmbeddingBatchResponseV2 = serde_json::from_slice(bytes).map_err(|error| {
        VectorIndexErrorV2::Unavailable(format!("invalid embedding provider JSON: {error}"))
    })?;
    if response.model != revision.provider_model_identifier
        || response.model_revision_sha256 != revision.provider_model_revision_sha256
        || response.request_config_sha256 != revision.request_config_sha256
    {
        return Err(VectorIndexErrorV2::InvalidConfiguration(
            "embedding provider response identity mismatch".into(),
        ));
    }
    if response.data.len() != expected_count {
        return Err(VectorIndexErrorV2::Unavailable(
            "embedding provider response is partial or oversized".into(),
        ));
    }
    let mut ordered = vec![None; expected_count];
    for value in response.data {
        if value.index >= expected_count || ordered[value.index].is_some() {
            return Err(VectorIndexErrorV2::Unavailable(
                "embedding provider response index is duplicate or out of range".into(),
            ));
        }
        validate_vector(&value.embedding)?;
        ordered[value.index] = Some(value.embedding);
    }
    ordered
        .into_iter()
        .map(|value| {
            value.ok_or_else(|| {
                VectorIndexErrorV2::Unavailable(
                    "embedding provider response is missing an input index".into(),
                )
            })
        })
        .collect()
}

fn validate_input_bounds(inputs: &[VectorEmbeddingInputV2]) -> Result<(), VectorIndexErrorV2> {
    if inputs.len() > VECTOR_EMBEDDING_MAX_VERSION_INPUTS_V2 {
        return Err(VectorIndexErrorV2::InvalidConfiguration(
            "embedding input count exceeds absolute bound".into(),
        ));
    }
    let mut total = 0usize;
    let mut ids = HashSet::with_capacity(inputs.len());
    for input in inputs {
        let bytes = input.canonical_input.len();
        if bytes > VECTOR_EMBEDDING_MAX_INPUT_BYTES_V2 {
            return Err(VectorIndexErrorV2::InvalidConfiguration(
                "embedding input exceeds per-input byte bound".into(),
            ));
        }
        total = total.checked_add(bytes).ok_or_else(|| {
            VectorIndexErrorV2::InvalidConfiguration("embedding input byte count overflow".into())
        })?;
        if total > VECTOR_EMBEDDING_MAX_REQUEST_BYTES_V2 {
            return Err(VectorIndexErrorV2::InvalidConfiguration(
                "embedding inputs exceed request byte bound".into(),
            ));
        }
        if !ids.insert(input.chunk_id) {
            return Err(VectorIndexErrorV2::InvalidConfiguration(
                "embedding inputs contain duplicate chunk identity".into(),
            ));
        }
        if canonical_sha256(&input.canonical_input) != input.indexed_content_sha256 {
            return Err(VectorIndexErrorV2::InvalidConfiguration(
                "embedding input digest mismatch".into(),
            ));
        }
    }
    Ok(())
}

fn validate_vector(vector: &[f32]) -> Result<(), VectorIndexErrorV2> {
    if vector.len() != VECTOR_INDEX_DIMENSION_V2
        || vector.iter().any(|component| !component.is_finite())
        || vector.iter().all(|component| *component == 0.0)
    {
        return Err(VectorIndexErrorV2::Unavailable(
            "embedding provider returned an invalid vector".into(),
        ));
    }
    Ok(())
}

fn provider_status_error(status: reqwest::StatusCode) -> VectorIndexErrorV2 {
    let message = format!("embedding provider returned status {status}");
    if status == reqwest::StatusCode::REQUEST_TIMEOUT
        || status == reqwest::StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
    {
        VectorIndexErrorV2::Unavailable(message)
    } else {
        VectorIndexErrorV2::InvalidConfiguration(message)
    }
}

#[derive(Clone)]
struct FrozenVectorIndexSnapshotV2 {
    product_version_id: Uuid,
    revision: EmbeddingRevisionV2,
    revision_sha256: String,
    credential_ref: String,
    inputs: Vec<VectorEmbeddingInputV2>,
    source_snapshot_sha256: String,
}

/// Builds one complete vector-sidecar generation for a bound product version.
/// A transaction-scoped version advisory lock serializes snapshot, provider,
/// and publication work without leaking a session lock through the pool.
/// Post-network reconcile still revalidates every canonical source byte plus
/// binding/revision. Any failure leaves the previous generation unchanged.
pub async fn rebuild_vector_indexes_v2(
    pool: &PgPool,
    product_version_id: Uuid,
    provider: &dyn VectorEmbeddingProviderV2,
) -> Result<VectorIndexGenerationV2, VectorIndexErrorV2> {
    let mut tx = pool.begin().await.map_err(VectorIndexErrorV2::Database)?;
    let generation =
        rebuild_vector_indexes_v2_in_transaction(&mut tx, product_version_id, provider).await?;
    tx.commit().await.map_err(VectorIndexErrorV2::Database)?;
    Ok(generation)
}

pub(crate) async fn rebuild_vector_indexes_v2_in_transaction(
    tx: &mut Transaction<'_, Postgres>,
    product_version_id: Uuid,
    provider: &dyn VectorEmbeddingProviderV2,
) -> Result<VectorIndexGenerationV2, VectorIndexErrorV2> {
    rebuild_vector_indexes_v2_expected_in_transaction(tx, product_version_id, None, provider).await
}

async fn rebuild_vector_indexes_v2_expected_in_transaction(
    tx: &mut Transaction<'_, Postgres>,
    product_version_id: Uuid,
    expected: Option<(&str, &str)>,
    provider: &dyn VectorEmbeddingProviderV2,
) -> Result<VectorIndexGenerationV2, VectorIndexErrorV2> {
    sqlx::query("SELECT pg_catalog.pg_advisory_xact_lock(pg_catalog.hashtextextended($1,0))")
        .bind(format!("knowledge-vector-v2:{product_version_id}"))
        .execute(&mut **tx)
        .await
        .map_err(VectorIndexErrorV2::Database)?;
    let snapshot = freeze_vector_index_snapshot_v2(tx, product_version_id).await?;
    if let Some((expected_revision, expected_snapshot)) = expected
        && (snapshot.revision_sha256 != expected_revision
            || snapshot.source_snapshot_sha256 != expected_snapshot)
    {
        return Err(VectorIndexErrorV2::SnapshotChanged(
            "vector source no longer matches the immutable semantic target".into(),
        ));
    }
    if expected.is_some() {
        let pending: bool =
            sqlx::query_scalar("SELECT public.kb_knowledge_has_pending_derived_v2($1)")
                .bind(product_version_id)
                .fetch_one(&mut **tx)
                .await
                .map_err(VectorIndexErrorV2::Database)?;
        if pending {
            return Err(VectorIndexErrorV2::PendingDerived(
                "semantic source has pending derived work".into(),
            ));
        }
    }
    let embeddings = if snapshot.inputs.is_empty() {
        Vec::new()
    } else {
        provider
            .embed_batch(
                &snapshot.revision,
                &snapshot.credential_ref,
                &snapshot.inputs,
            )
            .await?
    };
    if embeddings.len() != snapshot.inputs.len() {
        return Err(VectorIndexErrorV2::Unavailable(
            "embedding provider returned partial version output".into(),
        ));
    }
    for embedding in &embeddings {
        validate_vector(embedding)?;
    }
    let payload = snapshot
        .inputs
        .iter()
        .zip(embeddings)
        .map(|(input, embedding)| {
            serde_json::json!({
                "chunk_id": input.chunk_id,
                "indexed_content_sha256": input.indexed_content_sha256,
                "embedding": embedding,
            })
        })
        .collect::<Vec<_>>();
    let count: i64 =
        sqlx::query_scalar("SELECT public.kb_knowledge_reconcile_vector_indexes_v2($1,$2,$3,$4)")
            .bind(snapshot.product_version_id)
            .bind(&snapshot.revision_sha256)
            .bind(&snapshot.source_snapshot_sha256)
            .bind(serde_json::Value::Array(payload))
            .fetch_one(&mut **tx)
            .await
            .map_err(map_reconcile_error)?;
    if usize::try_from(count).ok() != Some(snapshot.inputs.len()) {
        return Err(VectorIndexErrorV2::Database(sqlx::Error::Protocol(
            "vector reconcile returned an impossible count".into(),
        )));
    }
    Ok(VectorIndexGenerationV2 {
        product_version_id,
        embedding_revision_sha256: snapshot.revision_sha256,
        source_snapshot_sha256: snapshot.source_snapshot_sha256,
        chunk_count: u32::try_from(snapshot.inputs.len()).map_err(|_| {
            VectorIndexErrorV2::InvalidConfiguration(
                "vector generation chunk count overflow".into(),
            )
        })?,
    })
}

async fn freeze_vector_index_snapshot_v2(
    tx: &mut Transaction<'_, Postgres>,
    product_version_id: Uuid,
) -> Result<FrozenVectorIndexSnapshotV2, VectorIndexErrorV2> {
    let row = sqlx::query(
        "SELECT binding.embedding_revision_sha256,revision.canonical_revision_payload,revision.credential_ref
           FROM public.product_versions version
           JOIN public.product_version_embedding_bindings_v2 binding
             ON binding.product_version_id=version.id
           JOIN public.embedding_revisions_v2 revision
             ON revision.revision_sha256=binding.embedding_revision_sha256
            AND revision.support_state='supported'
          WHERE version.id=$1 AND version.deleted_at IS NULL
          FOR SHARE OF version,binding,revision",
    )
    .bind(product_version_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(VectorIndexErrorV2::Database)?
    .ok_or_else(|| {
        VectorIndexErrorV2::InvalidConfiguration(
            "product version has no supported immutable V2 embedding binding".into(),
        )
    })?;
    let revision_sha256 = row.get::<String, _>("embedding_revision_sha256");
    let revision_payload = row.get::<Vec<u8>, _>("canonical_revision_payload");
    let revision =
        serde_json::from_slice::<EmbeddingRevisionV2>(&revision_payload).map_err(|error| {
            VectorIndexErrorV2::InvalidConfiguration(format!(
                "invalid embedding revision artifact: {error}"
            ))
        })?;
    let canonical = revision.canonical_bytes().map_err(|error| {
        VectorIndexErrorV2::InvalidConfiguration(format!(
            "invalid embedding revision artifact: {error}"
        ))
    })?;
    let digest = hex::encode(Sha256::digest(&canonical));
    if canonical != revision_payload || digest != revision_sha256 {
        return Err(VectorIndexErrorV2::InvalidConfiguration(
            "non-canonical embedding revision artifact".into(),
        ));
    }
    let rows = sqlx::query(
        "SELECT chunk.id,chunk.context_header,chunk.content
           FROM public.chunks chunk
           JOIN public.documents document ON document.id=chunk.document_id
            AND document.product_version_id=chunk.product_version_id
          WHERE chunk.product_version_id=$1
            AND document.deleted_at IS NULL
            AND document.enable_status='enabled'
            AND document.index_ready
            AND chunk.chunk_type=ANY($2::text[])
            AND (chunk.chunk_type<>'image_ocr' OR EXISTS (
              SELECT 1 FROM public.knowledge_image_ocr_chunk_artifact_mappings mapping
               WHERE mapping.chunk_id=chunk.id))
          ORDER BY chunk.id",
    )
    .bind(product_version_id)
    .bind(SIGNAL_TYPES_V2.as_slice())
    .fetch_all(&mut **tx)
    .await
    .map_err(VectorIndexErrorV2::Database)?;
    let inputs = rows
        .into_iter()
        .map(|row| {
            let chunk_id = row.get("id");
            let canonical_input = canonical_embedding_input_v2(
                row.get::<String, _>("context_header").as_str(),
                row.get::<String, _>("content").as_str(),
            );
            VectorEmbeddingInputV2 {
                chunk_id,
                indexed_content_sha256: canonical_sha256(&canonical_input),
                canonical_input,
            }
        })
        .collect::<Vec<_>>();
    validate_input_bounds(&inputs)?;
    let source_snapshot_sha256 = source_snapshot_sha256_v2(&revision_sha256, &inputs);
    let credential_ref = row.get::<String, _>("credential_ref");
    Ok(FrozenVectorIndexSnapshotV2 {
        product_version_id,
        revision,
        revision_sha256,
        credential_ref,
        inputs,
        source_snapshot_sha256,
    })
}

pub fn canonical_embedding_input_v2(context_header: &str, content: &str) -> String {
    format!("{context_header}\n{content}")
}

pub fn canonical_embedding_input_sha256_v2(context_header: &str, content: &str) -> String {
    canonical_sha256(&canonical_embedding_input_v2(context_header, content))
}

fn canonical_sha256(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}

pub fn source_snapshot_sha256_v2(
    embedding_revision_sha256: &str,
    inputs: &[VectorEmbeddingInputV2],
) -> String {
    let mut digest = Sha256::new();
    digest.update(embedding_revision_sha256.as_bytes());
    digest.update(b"\n");
    for input in inputs {
        digest.update(input.chunk_id.to_string().as_bytes());
        digest.update(b":");
        digest.update(input.indexed_content_sha256.as_bytes());
        digest.update(b"\n");
    }
    hex::encode(digest.finalize())
}

fn map_reconcile_error(error: sqlx::Error) -> VectorIndexErrorV2 {
    let message = error
        .as_database_error()
        .map(|database| database.message().to_string())
        .unwrap_or_default();
    if message.contains("KNOWLEDGE_VECTOR_INDEX_V2_SNAPSHOT_CHANGED") {
        VectorIndexErrorV2::SnapshotChanged(message)
    } else if message.contains("KNOWLEDGE_VECTOR_INDEX_V2_NOT_SUPPORTED")
        || message.contains("KNOWLEDGE_VECTOR_INDEX_V2_INVALID")
    {
        VectorIndexErrorV2::InvalidConfiguration(message)
    } else {
        VectorIndexErrorV2::Database(error)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticIndexIntentV2 {
    pub id: Uuid,
    pub product_version_id: Uuid,
    pub embedding_revision_sha256: String,
    pub source_snapshot_sha256: String,
    pub target_revision: i64,
    pub status: String,
    pub generation_marker_sha256: Option<String>,
    pub last_error_code: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SemanticIndexPreparationV2 {
    Unbound,
    PendingDerived,
    Enqueue(SemanticIndexIntentV2),
    Ready(SemanticIndexIntentV2),
    Terminal(SemanticIndexIntentV2),
    Superseded(SemanticIndexIntentV2),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemanticIndexPreflightV2 {
    Current,
    PendingDerived,
    Completed,
    Terminal,
    Superseded,
    Duplicate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemanticIndexCompletionV2 {
    Completed,
    PendingDerived,
    NotReady,
    Terminal,
    Superseded,
    Duplicate,
}

/// Rebuilds vectors only if the frozen provider input still matches the
/// immutable business target. The comparison happens before credential lookup
/// or provider I/O; post-network SQL reconciliation repeats the source fence.
pub async fn rebuild_vector_indexes_for_intent_v2(
    pool: &PgPool,
    intent: &SemanticIndexIntentV2,
    provider: &dyn VectorEmbeddingProviderV2,
) -> Result<VectorIndexGenerationV2, VectorIndexErrorV2> {
    let mut tx = pool.begin().await.map_err(VectorIndexErrorV2::Database)?;
    let generation = rebuild_vector_indexes_v2_expected_in_transaction(
        &mut tx,
        intent.product_version_id,
        Some((
            &intent.embedding_revision_sha256,
            &intent.source_snapshot_sha256,
        )),
        provider,
    )
    .await?;
    tx.commit().await.map_err(VectorIndexErrorV2::Database)?;
    Ok(generation)
}

async fn load_semantic_index_intent_v2(
    pool: &PgPool,
    intent_id: Uuid,
) -> Result<Option<SemanticIndexIntentV2>, sqlx::Error> {
    sqlx::query(
        "SELECT id,product_version_id,embedding_revision_sha256,
                source_snapshot_sha256,target_revision,status,
                generation_marker_sha256,last_error_code
           FROM knowledge_semantic_index_intents_v2 WHERE id=$1",
    )
    .bind(intent_id)
    .fetch_optional(pool)
    .await
    .map(|row| {
        row.map(|row| SemanticIndexIntentV2 {
            id: row.get("id"),
            product_version_id: row.get("product_version_id"),
            embedding_revision_sha256: row.get("embedding_revision_sha256"),
            source_snapshot_sha256: row.get("source_snapshot_sha256"),
            target_revision: row.get("target_revision"),
            status: row.get("status"),
            generation_marker_sha256: row.get("generation_marker_sha256"),
            last_error_code: row.get("last_error_code"),
        })
    })
}

pub async fn semantic_index_intent_v2(
    pool: &PgPool,
    intent_id: Uuid,
    target_revision: i64,
) -> Result<Option<SemanticIndexIntentV2>, sqlx::Error> {
    let intent = load_semantic_index_intent_v2(pool, intent_id).await?;
    Ok(intent.filter(|intent| intent.target_revision == target_revision))
}

/// Atomically observes one settled product-version source generation and
/// creates or reuses its immutable business intent. Oxana owns delivery phase;
/// this row owns only source/binding identity and final publication status.
pub async fn prepare_semantic_index_intent_v2(
    pool: &PgPool,
    product_version_id: Uuid,
) -> Result<SemanticIndexPreparationV2, sqlx::Error> {
    let row = sqlx::query(
        "SELECT state,intent_id,target_revision
           FROM kb_knowledge_prepare_semantic_index_intent_v2($1)",
    )
    .bind(product_version_id)
    .fetch_one(pool)
    .await?;
    let state = row.get::<String, _>("state");
    let intent_id = row.get::<Option<Uuid>, _>("intent_id");
    if state == "unbound" {
        return Ok(SemanticIndexPreparationV2::Unbound);
    }
    if state == "pending_derived" {
        return Ok(SemanticIndexPreparationV2::PendingDerived);
    }
    let intent_id = intent_id.ok_or_else(|| {
        sqlx::Error::Protocol("semantic index preparation omitted intent identity".into())
    })?;
    let intent = load_semantic_index_intent_v2(pool, intent_id)
        .await?
        .ok_or_else(|| sqlx::Error::Protocol("semantic index intent disappeared".into()))?;
    match state.as_str() {
        "enqueue" => Ok(SemanticIndexPreparationV2::Enqueue(intent)),
        "ready" => Ok(SemanticIndexPreparationV2::Ready(intent)),
        "terminal" => Ok(SemanticIndexPreparationV2::Terminal(intent)),
        "superseded" => Ok(SemanticIndexPreparationV2::Superseded(intent)),
        _ => Err(sqlx::Error::Protocol(
            "semantic index preparation returned an unknown state".into(),
        )),
    }
}

pub async fn preflight_semantic_index_intent_v2(
    pool: &PgPool,
    intent_id: Uuid,
    target_revision: i64,
) -> Result<SemanticIndexPreflightV2, sqlx::Error> {
    let state: String =
        sqlx::query_scalar("SELECT kb_knowledge_preflight_semantic_index_intent_v2($1,$2)")
            .bind(intent_id)
            .bind(target_revision)
            .fetch_one(pool)
            .await?;
    match state.as_str() {
        "current" => Ok(SemanticIndexPreflightV2::Current),
        "pending_derived" => Ok(SemanticIndexPreflightV2::PendingDerived),
        "completed" => Ok(SemanticIndexPreflightV2::Completed),
        "terminal" => Ok(SemanticIndexPreflightV2::Terminal),
        "superseded" => Ok(SemanticIndexPreflightV2::Superseded),
        "duplicate" => Ok(SemanticIndexPreflightV2::Duplicate),
        _ => Err(sqlx::Error::Protocol(
            "semantic index preflight returned an unknown state".into(),
        )),
    }
}

pub async fn rebuild_semantic_keyword_indexes_v2(
    pool: &PgPool,
    intent: &SemanticIndexIntentV2,
) -> Result<i64, VectorIndexErrorV2> {
    sqlx::query_scalar("SELECT kb_knowledge_rebuild_semantic_keyword_indexes_v2($1,$2,$3)")
        .bind(intent.product_version_id)
        .bind(&intent.embedding_revision_sha256)
        .bind(&intent.source_snapshot_sha256)
        .fetch_one(pool)
        .await
        .map_err(map_semantic_index_sql_error_v2)
}

pub async fn semantic_vector_generation_matches_intent_v2(
    pool: &PgPool,
    intent: &SemanticIndexIntentV2,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS(
           SELECT 1 FROM product_version_vector_index_generations_v2 generation
            WHERE generation.product_version_id=$1
              AND generation.embedding_revision_sha256=$2
              AND generation.source_snapshot_sha256=$3)",
    )
    .bind(intent.product_version_id)
    .bind(&intent.embedding_revision_sha256)
    .bind(&intent.source_snapshot_sha256)
    .fetch_one(pool)
    .await
}

pub async fn complete_semantic_index_intent_v2(
    pool: &PgPool,
    intent: &SemanticIndexIntentV2,
) -> Result<SemanticIndexCompletionV2, sqlx::Error> {
    let state: String =
        sqlx::query_scalar("SELECT kb_knowledge_complete_semantic_index_intent_v2($1,$2)")
            .bind(intent.id)
            .bind(intent.target_revision)
            .fetch_one(pool)
            .await?;
    match state.as_str() {
        "completed" => Ok(SemanticIndexCompletionV2::Completed),
        "pending_derived" => Ok(SemanticIndexCompletionV2::PendingDerived),
        "not_ready" => Ok(SemanticIndexCompletionV2::NotReady),
        "terminal" => Ok(SemanticIndexCompletionV2::Terminal),
        "superseded" => Ok(SemanticIndexCompletionV2::Superseded),
        "duplicate" => Ok(SemanticIndexCompletionV2::Duplicate),
        _ => Err(sqlx::Error::Protocol(
            "semantic index completion returned an unknown state".into(),
        )),
    }
}

pub async fn record_semantic_index_intent_v2(
    pool: &PgPool,
    intent: &SemanticIndexIntentV2,
    disposition: &str,
    error_code: &str,
    error_detail: &str,
) -> Result<bool, sqlx::Error> {
    record_semantic_index_error_v2(
        pool,
        intent.id,
        intent.target_revision,
        disposition,
        error_code,
        error_detail,
    )
    .await
}

pub async fn record_semantic_index_error_v2(
    pool: &PgPool,
    intent_id: Uuid,
    target_revision: i64,
    disposition: &str,
    error_code: &str,
    error_detail: &str,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_knowledge_record_semantic_index_intent_v2($1,$2,$3,$4,$5)")
        .bind(intent_id)
        .bind(target_revision)
        .bind(disposition)
        .bind(error_code)
        .bind(error_detail)
        .fetch_one(pool)
        .await
}

pub fn map_semantic_index_sql_error_v2(error: sqlx::Error) -> VectorIndexErrorV2 {
    let message = error
        .as_database_error()
        .map(|database| database.message().to_string())
        .unwrap_or_default();
    if message.contains("KNOWLEDGE_SEMANTIC_INDEX_V2_PENDING_DERIVED") {
        VectorIndexErrorV2::PendingDerived(message)
    } else if message.contains("KNOWLEDGE_SEMANTIC_INDEX_V2_SNAPSHOT_CHANGED") {
        VectorIndexErrorV2::SnapshotChanged(message)
    } else if message.contains("KNOWLEDGE_SEMANTIC_INDEX_V2_NOT_SUPPORTED")
        || message.contains("KNOWLEDGE_SEMANTIC_INDEX_V2_KEYWORD_INCOMPLETE")
    {
        VectorIndexErrorV2::InvalidConfiguration(message)
    } else {
        VectorIndexErrorV2::Database(error)
    }
}

/// Rebuilds the fixed V2 keyword sidecar for one product version in PostgreSQL.
pub async fn rebuild_keyword_indexes_v2(
    pool: &PgPool,
    product_version_id: Uuid,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_knowledge_rebuild_keyword_indexes_v2($1)")
        .bind(product_version_id)
        .fetch_one(pool)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, atomic::AtomicUsize};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        sync::oneshot,
    };

    fn revision() -> EmbeddingRevisionV2 {
        EmbeddingRevisionV2 {
            schema_version: 2,
            provider_protocol_version: "openai-compatible-embeddings-json-v1".into(),
            provider_model_identifier: "embedding-model@2025-01-15".into(),
            provider_model_revision_sha256: "a".repeat(64),
            endpoint_config_sha256: "b".repeat(64),
            endpoint_identity: "https://embedding.example.test/v1/embeddings".into(),
            dimension: 1024,
            request_config_sha256: EmbeddingRevisionV2::canonical_request_config_sha256(),
            output_normalization_version: "finite-vector-no-client-normalization-v1".into(),
        }
    }

    fn response(data: serde_json::Value) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "model": "embedding-model@2025-01-15",
            "model_revision_sha256": "a".repeat(64),
            "request_config_sha256": EmbeddingRevisionV2::canonical_request_config_sha256(),
            "data": data,
        }))
        .unwrap()
    }

    fn vector(value: f32) -> Vec<f32> {
        vec![value; 1024]
    }

    #[tokio::test]
    async fn environment_credentials_reject_inline_invalid_and_missing_references() {
        let resolver = EnvironmentEmbeddingCredentialResolverV2;
        for credential_ref in ["inline-secret", "env:", "env:invalid-name"] {
            assert!(matches!(
                resolver.resolve(credential_ref).await,
                Err(VectorIndexErrorV2::InvalidConfiguration(_))
            ));
        }
        assert!(matches!(
            resolver
                .resolve("env:KNOWLEDGEBRAIN_TEST_CREDENTIAL_THAT_MUST_NOT_EXIST_V2")
                .await,
            Err(VectorIndexErrorV2::InvalidConfiguration(_))
        ));
    }

    struct TestCredentials {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl EmbeddingCredentialResolverV2 for TestCredentials {
        async fn resolve(&self, _credential_ref: &str) -> Result<String, VectorIndexErrorV2> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok("secret".into())
        }
    }

    #[derive(Clone, Copy)]
    enum TestTransportMode {
        Success,
        Timeout,
        Oversized,
        Status(reqwest::StatusCode),
    }

    struct TestTransport {
        batches: Arc<Mutex<Vec<usize>>>,
        mode: TestTransportMode,
    }

    #[async_trait]
    impl VectorEmbeddingTransportV2 for TestTransport {
        async fn send(
            &self,
            request: VectorEmbeddingTransportRequestV2,
        ) -> Result<VectorEmbeddingTransportResponseV2, VectorIndexErrorV2> {
            assert_eq!(request.endpoint, revision().endpoint_identity);
            assert_eq!(request.model, revision().provider_model_identifier);
            assert_eq!(
                request.request_config_sha256,
                revision().request_config_sha256
            );
            assert_eq!(request.credential, "secret");
            self.batches.lock().unwrap().push(request.inputs.len());
            match self.mode {
                TestTransportMode::Success => {
                    let data = request
                        .inputs
                        .iter()
                        .enumerate()
                        .rev()
                        .map(|(index, _)| {
                            serde_json::json!({
                                "index": index,
                                "embedding": vector((index + 1) as f32),
                            })
                        })
                        .collect::<Vec<_>>();
                    Ok(VectorEmbeddingTransportResponseV2 {
                        status: reqwest::StatusCode::OK,
                        content_length: None,
                        bytes: response(serde_json::Value::Array(data)),
                    })
                }
                TestTransportMode::Timeout => Err(VectorIndexErrorV2::Unavailable(
                    "deterministic transport timeout".into(),
                )),
                TestTransportMode::Oversized => Ok(VectorEmbeddingTransportResponseV2 {
                    status: reqwest::StatusCode::OK,
                    content_length: Some(
                        u64::try_from(VECTOR_EMBEDDING_MAX_BATCH_RESPONSE_BYTES_V2).unwrap() + 1,
                    ),
                    bytes: Vec::new(),
                }),
                TestTransportMode::Status(status) => Ok(VectorEmbeddingTransportResponseV2 {
                    status,
                    content_length: Some(0),
                    bytes: Vec::new(),
                }),
            }
        }
    }

    async fn local_http_response(response: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = vec![0u8; 8192];
            let _ = socket.read(&mut request).await.unwrap();
            socket.write_all(&response).await.unwrap();
            socket.shutdown().await.unwrap();
        });
        format!("http://{address}/v1/embeddings")
    }

    async fn stalled_chunked_oversize_response() -> (
        String,
        oneshot::Receiver<()>,
        oneshot::Sender<()>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (body_sent_tx, body_sent_rx) = oneshot::channel();
        let (finish_tx, finish_rx) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = vec![0u8; 8192];
            let _ = socket.read(&mut request).await.unwrap();
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: keep-alive\r\n\r\nc\r\n123456789012\r\nc\r\nabcdefghijkl\r\n",
                )
                .await
                .unwrap();
            socket.flush().await.unwrap();
            let _ = body_sent_tx.send(());
            let _ = finish_rx.await;
            let _ = socket.write_all(b"0\r\n\r\n").await;
            let _ = socket.shutdown().await;
        });
        (
            format!("http://{address}/v1/embeddings"),
            body_sent_rx,
            finish_tx,
            server,
        )
    }

    fn transport_request(endpoint: String) -> VectorEmbeddingTransportRequestV2 {
        VectorEmbeddingTransportRequestV2 {
            endpoint,
            credential: "secret".into(),
            model: revision().provider_model_identifier,
            request_config_sha256: revision().request_config_sha256,
            inputs: vec!["header\ncontent".into()],
        }
    }

    fn inputs(count: usize) -> Vec<VectorEmbeddingInputV2> {
        (0..count)
            .map(|index| {
                let canonical_input = format!("header {index}\\ncontent {index}");
                VectorEmbeddingInputV2 {
                    chunk_id: Uuid::from_u128(index as u128 + 1),
                    indexed_content_sha256: canonical_sha256(&canonical_input),
                    canonical_input,
                }
            })
            .collect()
    }

    #[tokio::test]
    async fn strict_transport_batches_once_resolves_once_and_fails_as_a_whole() {
        let credential_calls = Arc::new(AtomicUsize::new(0));
        let batches = Arc::new(Mutex::new(Vec::new()));
        let client = StrictVectorEmbeddingClientV2::with_transport(
            Arc::new(TestCredentials {
                calls: credential_calls.clone(),
            }),
            Arc::new(TestTransport {
                batches: batches.clone(),
                mode: TestTransportMode::Success,
            }),
        );
        let output = client
            .embed_batch(&revision(), "credential:v2", &inputs(65))
            .await
            .unwrap();
        assert_eq!(output.len(), 65);
        assert_eq!(*batches.lock().unwrap(), vec![64, 1]);
        assert_eq!(
            credential_calls.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        assert!(
            client
                .embed_batch(&revision(), "credential:v2", &[])
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            credential_calls.load(std::sync::atomic::Ordering::SeqCst),
            1
        );

        let empty_credentials = Arc::new(AtomicUsize::new(0));
        let empty_sends = Arc::new(Mutex::new(Vec::new()));
        let empty_client = StrictVectorEmbeddingClientV2::with_transport(
            Arc::new(TestCredentials {
                calls: empty_credentials.clone(),
            }),
            Arc::new(TestTransport {
                batches: empty_sends.clone(),
                mode: TestTransportMode::Timeout,
            }),
        );
        assert!(
            empty_client
                .embed_batch(&revision(), "credential:v2", &[])
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            empty_credentials.load(std::sync::atomic::Ordering::SeqCst),
            0
        );
        assert!(empty_sends.lock().unwrap().is_empty());

        for mode in [TestTransportMode::Timeout, TestTransportMode::Oversized] {
            let failing = StrictVectorEmbeddingClientV2::with_transport(
                Arc::new(TestCredentials {
                    calls: Arc::new(AtomicUsize::new(0)),
                }),
                Arc::new(TestTransport {
                    batches: Arc::new(Mutex::new(Vec::new())),
                    mode,
                }),
            );
            assert!(matches!(
                failing
                    .embed_batch(&revision(), "credential:v2", &inputs(1))
                    .await,
                Err(VectorIndexErrorV2::Unavailable(_))
            ));
        }

        for (status, retryable) in [
            (reqwest::StatusCode::TOO_MANY_REQUESTS, true),
            (reqwest::StatusCode::BAD_GATEWAY, true),
            (reqwest::StatusCode::UNAUTHORIZED, false),
            (reqwest::StatusCode::FOUND, false),
        ] {
            let failing = StrictVectorEmbeddingClientV2::with_transport(
                Arc::new(TestCredentials {
                    calls: Arc::new(AtomicUsize::new(0)),
                }),
                Arc::new(TestTransport {
                    batches: Arc::new(Mutex::new(Vec::new())),
                    mode: TestTransportMode::Status(status),
                }),
            );
            let error = failing
                .embed_batch(&revision(), "credential:v2", &inputs(1))
                .await
                .unwrap_err();
            assert_eq!(error.retryable(), retryable, "status {status}");
        }
    }

    #[tokio::test]
    async fn reqwest_transport_rejects_redirect_malformed_and_chunked_oversize() {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let transport = ReqwestVectorEmbeddingTransportV2 {
            http,
            max_response_bytes: 16,
        };

        let redirect = local_http_response(
            b"HTTP/1.1 302 Found\r\nLocation: /elsewhere\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
        )
        .await;
        let redirected = transport.send(transport_request(redirect)).await.unwrap();
        assert_eq!(redirected.status, reqwest::StatusCode::FOUND);

        let malformed = local_http_response(
            b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nConnection: close\r\n\r\n{".to_vec(),
        )
        .await;
        let malformed = transport.send(transport_request(malformed)).await.unwrap();
        assert!(parse_batch_response(&malformed.bytes, &revision(), 1).is_err());

        let (oversized, body_sent, finish, server) = stalled_chunked_oversize_response().await;
        let mut request =
            tokio::spawn(async move { transport.send(transport_request(oversized)).await });
        body_sent.await.unwrap();
        let early_result = tokio::time::timeout(Duration::from_secs(5), &mut request).await;
        let _ = finish.send(());
        server.await.unwrap();
        let result = match early_result {
            Ok(result) => result.unwrap(),
            Err(_) => {
                let _ = request.await;
                panic!("bounded reader waited for the withheld terminating chunk")
            }
        };
        assert!(matches!(
            result,
            Err(VectorIndexErrorV2::Unavailable(message))
                if message.contains("exceeds byte limit")
        ));
    }

    #[test]
    fn batch_response_is_indexed_and_fail_closed() {
        let parsed = parse_batch_response(
            &response(serde_json::json!([
                {"index":1,"embedding":vector(2.0)},
                {"index":0,"embedding":vector(1.0)}
            ])),
            &revision(),
            2,
        )
        .unwrap();
        assert_eq!(parsed[0][0], 1.0);
        assert_eq!(parsed[1][0], 2.0);

        for invalid in [
            serde_json::json!([{"index":0,"embedding":vector(1.0)}]),
            serde_json::json!([
                {"index":0,"embedding":vector(1.0)},
                {"index":0,"embedding":vector(2.0)}
            ]),
            serde_json::json!([
                {"index":0,"embedding":vector(1.0)},
                {"index":2,"embedding":vector(2.0)}
            ]),
        ] {
            assert!(parse_batch_response(&response(invalid), &revision(), 2).is_err());
        }
        assert!(parse_batch_response(b"{", &revision(), 1).is_err());
        let wrong_dimension = response(serde_json::json!([
            {"index":0,"embedding":vec![1.0; 1023]}
        ]));
        assert!(parse_batch_response(&wrong_dimension, &revision(), 1).is_err());
        let zero = response(serde_json::json!([{"index":0,"embedding":vector(0.0)}]));
        assert!(parse_batch_response(&zero, &revision(), 1).is_err());
        let wrong_identity = serde_json::to_vec(&serde_json::json!({
            "model": "wrong-model",
            "model_revision_sha256": "a".repeat(64),
            "request_config_sha256": EmbeddingRevisionV2::canonical_request_config_sha256(),
            "data": [{"index":0,"embedding":vector(1.0)}],
        }))
        .unwrap();
        assert!(matches!(
            parse_batch_response(&wrong_identity, &revision(), 1),
            Err(VectorIndexErrorV2::InvalidConfiguration(_))
        ));
        let mut nonfinite = vector(1.0);
        nonfinite[0] = f32::NAN;
        assert!(validate_vector(&nonfinite).is_err());
    }

    #[test]
    fn snapshot_digest_binds_revision_order_and_canonical_bytes() {
        let first = VectorEmbeddingInputV2 {
            chunk_id: Uuid::from_u128(1),
            canonical_input: "header\ncontent".into(),
            indexed_content_sha256: canonical_sha256("header\ncontent"),
        };
        let second = VectorEmbeddingInputV2 {
            chunk_id: Uuid::from_u128(2),
            canonical_input: "\ncontent".into(),
            indexed_content_sha256: canonical_sha256("\ncontent"),
        };
        assert_ne!(
            source_snapshot_sha256_v2(&"a".repeat(64), &[first.clone(), second.clone()]),
            source_snapshot_sha256_v2(&"a".repeat(64), &[second, first])
        );
        assert_ne!(
            canonical_embedding_input_sha256_v2("", "content"),
            canonical_embedding_input_sha256_v2("header", "content")
        );
    }
}
