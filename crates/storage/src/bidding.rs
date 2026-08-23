//! Final V1 TenderPublication and ClauseLifecycle storage adapter.
//!
//! Runtime callers execute only checked `SECURITY DEFINER` functions and read
//! typed projections. No API in this module exposes direct bidding-table DML.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct RequestIdentity {
    pub bytes: Vec<u8>,
    pub sha256: String,
}

impl RequestIdentity {
    pub fn canonical<T: Serialize>(payload: &T) -> Result<Self, serde_json::Error> {
        let bytes = serde_json::to_vec(payload)?;
        let sha256 = hex::encode(Sha256::digest(&bytes));
        Ok(Self { bytes, sha256 })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MutationContext {
    pub actor: String,
    pub idempotency_key: String,
    #[serde(skip)]
    pub request: RequestIdentity,
}

impl MutationContext {
    pub fn new<T: Serialize>(
        actor: impl Into<String>,
        idempotency_key: impl Into<String>,
        payload: &T,
    ) -> Result<Self, serde_json::Error> {
        Ok(Self {
            actor: actor.into(),
            idempotency_key: idempotency_key.into(),
            request: RequestIdentity::canonical(payload)?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: Uuid,
    pub title: String,
    pub owner_user_id: Uuid,
    pub ends_at: DateTime<Utc>,
    pub status: String,
    pub ended_at: Option<DateTime<Utc>>,
    pub fact_revision: i64,
    pub fact_sha256: String,
    pub budget_amount: Option<String>,
    pub budget_currency: Option<String>,
    pub ceiling_price: Option<String>,
    pub ceiling_currency: Option<String>,
    pub ceiling_basis: String,
    pub ceiling_revision: i64,
    pub ceiling_identity_sha256: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub bid_open_at: Option<DateTime<Utc>>,
    pub bid_valid_until: Option<DateTime<Utc>>,
    pub bid_valid_days: Option<i32>,
    pub matching_mutation_watermark: i64,
}

fn project_from_row(row: &sqlx::postgres::PgRow) -> Project {
    Project {
        id: row.get("id"),
        title: row.get("title"),
        owner_user_id: row.get("owner_user_id"),
        ends_at: row.get("ends_at"),
        status: row.get("status"),
        ended_at: row.get("ended_at"),
        fact_revision: row.get("fact_revision"),
        fact_sha256: row.get("fact_sha256"),
        budget_amount: row
            .get::<Option<rust_decimal::Decimal>, _>("budget_amount")
            .map(|value| format!("{value:.2}")),
        budget_currency: row.get("budget_currency"),
        ceiling_price: row
            .get::<Option<rust_decimal::Decimal>, _>("ceiling_price")
            .map(|value| format!("{value:.2}")),
        ceiling_currency: row.get("ceiling_currency"),
        ceiling_basis: row.get("ceiling_basis"),
        ceiling_revision: row.get("ceiling_revision"),
        ceiling_identity_sha256: row.get("ceiling_identity_sha256"),
        expires_at: row.get("expires_at"),
        bid_open_at: row.get("bid_open_at"),
        bid_valid_until: row.get("bid_valid_until"),
        bid_valid_days: row.get("bid_valid_days"),
        matching_mutation_watermark: row.get("matching_mutation_watermark"),
    }
}

pub async fn create_project(
    pool: &PgPool,
    id: Uuid,
    title: &str,
    owner_user_id: Uuid,
    ends_at: DateTime<Utc>,
    expires_at: Option<DateTime<Utc>>,
    context: &MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_create_project($1,$2,$3,$4,$5,$6,$7,$8,$9)")
        .bind(id)
        .bind(title)
        .bind(owner_user_id)
        .bind(ends_at)
        .bind(expires_at)
        .bind(&context.actor)
        .bind(&context.idempotency_key)
        .bind(&context.request.bytes)
        .bind(&context.request.sha256)
        .fetch_one(pool)
        .await
}

pub async fn list_projects(pool: &PgPool) -> Result<Vec<Project>, sqlx::Error> {
    let rows = sqlx::query("SELECT * FROM bidding_projects ORDER BY created_at DESC,id")
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(project_from_row).collect())
}

pub async fn get_project(pool: &PgPool, id: Uuid) -> Result<Option<Project>, sqlx::Error> {
    let row = sqlx::query("SELECT * FROM bidding_projects WHERE id=$1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row.as_ref().map(project_from_row))
}

pub async fn end_project(
    pool: &PgPool,
    project_id: Uuid,
    expected_fact_revision: i64,
    context: &MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_end_project($1,$2,$3,$4,$5,$6)")
        .bind(project_id)
        .bind(expected_fact_revision)
        .bind(&context.actor)
        .bind(&context.idempotency_key)
        .bind(&context.request.bytes)
        .bind(&context.request.sha256)
        .fetch_one(pool)
        .await
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BidDocument {
    pub id: Uuid,
    pub project_id: Uuid,
    pub file_name: String,
    pub media_type: String,
    pub byte_length: i64,
    pub original_object_ref: String,
    pub original_sha256: String,
    pub conversion_generation: i32,
    pub parse_status: String,
    pub current_converted_source_artifact_id: Option<Uuid>,
    pub parsed_at: Option<DateTime<Utc>>,
    pub error_code: Option<String>,
}

fn document_from_row(row: &sqlx::postgres::PgRow) -> BidDocument {
    BidDocument {
        id: row.get("id"),
        project_id: row.get("project_id"),
        file_name: row.get("file_name"),
        media_type: row.get("media_type"),
        byte_length: row.get("byte_length"),
        original_object_ref: row.get("original_object_ref"),
        original_sha256: row.get("original_sha256"),
        conversion_generation: row.get("conversion_generation"),
        parse_status: row.get("parse_status"),
        current_converted_source_artifact_id: row.get("current_converted_source_artifact_id"),
        parsed_at: row.get("parsed_at"),
        error_code: row.get("error_code"),
    }
}

pub struct UploadDocument<'a> {
    pub id: Uuid,
    pub project_id: Uuid,
    pub file_name: &'a str,
    pub media_type: &'a str,
    pub byte_length: i64,
    pub object_ref: &'a str,
    pub original_sha256: &'a str,
}

pub async fn upload_document(
    pool: &PgPool,
    input: UploadDocument<'_>,
    context: &MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_upload_document($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)")
        .bind(input.id)
        .bind(input.project_id)
        .bind(input.file_name)
        .bind(input.media_type)
        .bind(input.byte_length)
        .bind(input.object_ref)
        .bind(input.original_sha256)
        .bind(&context.actor)
        .bind(&context.idempotency_key)
        .bind(&context.request.bytes)
        .bind(&context.request.sha256)
        .fetch_one(pool)
        .await
}

pub async fn list_documents(
    pool: &PgPool,
    project_id: Uuid,
) -> Result<Vec<BidDocument>, sqlx::Error> {
    let rows =
        sqlx::query("SELECT * FROM bidding_documents WHERE project_id=$1 ORDER BY created_at,id")
            .bind(project_id)
            .fetch_all(pool)
            .await?;
    Ok(rows.iter().map(document_from_row).collect())
}

pub async fn get_document(
    pool: &PgPool,
    document_id: Uuid,
) -> Result<Option<BidDocument>, sqlx::Error> {
    let row = sqlx::query("SELECT * FROM bidding_documents WHERE id=$1")
        .bind(document_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.as_ref().map(document_from_row))
}

pub async fn retry_document_conversion(
    pool: &PgPool,
    project_id: Uuid,
    document_id: Uuid,
    expected_generation: i32,
    context: &MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_retry_document_conversion($1,$2,$3,$4,$5,$6,$7)")
        .bind(project_id)
        .bind(document_id)
        .bind(expected_generation)
        .bind(&context.actor)
        .bind(&context.idempotency_key)
        .bind(&context.request.bytes)
        .bind(&context.request.sha256)
        .fetch_one(pool)
        .await
}

#[derive(Debug, Clone)]
pub struct ConversionClaim {
    pub project_id: Uuid,
    pub file_name: String,
    pub object_ref: String,
    pub conversion_generation: i32,
    pub claim_lease_ms: i32,
}

pub async fn claim_document_conversion(
    pool: &PgPool,
    document_id: Uuid,
    claim_token: Uuid,
    claimed_by: &str,
) -> Result<Option<ConversionClaim>, sqlx::Error> {
    let row = sqlx::query("SELECT * FROM kb_bid_claim_document_conversion($1,$2,$3)")
        .bind(document_id)
        .bind(claim_token)
        .bind(claimed_by)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|row| ConversionClaim {
        project_id: row.get("project_id"),
        file_name: row.get("file_name"),
        object_ref: row.get("object_ref"),
        conversion_generation: row.get("conversion_generation"),
        claim_lease_ms: row.get("claim_lease_ms"),
    }))
}

pub async fn heartbeat_document_conversion(
    pool: &PgPool,
    document_id: Uuid,
    claim_token: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_heartbeat_document_conversion($1,$2)")
        .bind(document_id)
        .bind(claim_token)
        .fetch_one(pool)
        .await
}

pub async fn complete_document_conversion(
    pool: &PgPool,
    document_id: Uuid,
    claim_token: Uuid,
    source_artifact_id: Uuid,
    markdown: &[u8],
    converter_contract_version: &str,
    image_asset_set_sha256: &str,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_complete_document_conversion($1,$2,$3,$4,$5,$6)")
        .bind(document_id)
        .bind(claim_token)
        .bind(source_artifact_id)
        .bind(markdown)
        .bind(converter_contract_version)
        .bind(image_asset_set_sha256)
        .fetch_one(pool)
        .await
}

pub async fn fail_document_conversion(
    pool: &PgPool,
    document_id: Uuid,
    claim_token: Uuid,
    error_code: &str,
    retry: bool,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_fail_document_conversion($1,$2,$3,$4)")
        .bind(document_id)
        .bind(claim_token)
        .bind(error_code)
        .bind(retry)
        .fetch_one(pool)
        .await
}

#[derive(Debug, Clone)]
pub struct ExtractionSource {
    pub document_id: Uuid,
    pub project_id: Uuid,
    pub file_name: String,
    pub conversion_generation: i32,
    pub source_artifact_id: Uuid,
    pub markdown: Vec<u8>,
    pub markdown_sha256: String,
}

pub async fn extraction_source(
    pool: &PgPool,
    document_id: Uuid,
) -> Result<Option<ExtractionSource>, sqlx::Error> {
    let row = sqlx::query("SELECT * FROM bidding_extraction_sources WHERE document_id=$1")
        .bind(document_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|row| ExtractionSource {
        document_id: row.get("document_id"),
        project_id: row.get("project_id"),
        file_name: row.get("file_name"),
        conversion_generation: row.get("conversion_generation"),
        source_artifact_id: row.get("source_artifact_id"),
        markdown: row.get("canonical_markdown_utf8"),
        markdown_sha256: row.get("markdown_sha256"),
    }))
}

pub async fn schedule_extraction(
    pool: &PgPool,
    target_id: Uuid,
    document_id: Uuid,
    expected_section_count: i32,
    policy_version: &str,
    prompt_version: &str,
    context: &MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_schedule_extraction($1,$2,$3,$4,$5,$6,$7,$8,$9)")
        .bind(target_id)
        .bind(document_id)
        .bind(expected_section_count)
        .bind(policy_version)
        .bind(prompt_version)
        .bind(&context.actor)
        .bind(&context.idempotency_key)
        .bind(&context.request.bytes)
        .bind(&context.request.sha256)
        .fetch_one(pool)
        .await
}

#[derive(Debug, Clone)]
pub struct ExtractionClaim {
    pub project_id: Uuid,
    pub document_id: Uuid,
    pub source_artifact_id: Uuid,
    pub conversion_generation: i32,
    pub extraction_generation: i32,
    pub attempt: i32,
    pub claim_lease_ms: i32,
}

pub async fn claim_extraction(
    pool: &PgPool,
    target_id: Uuid,
    claim_token: Uuid,
    claimed_by: &str,
) -> Result<Option<ExtractionClaim>, sqlx::Error> {
    let row = sqlx::query("SELECT * FROM kb_bid_claim_extraction($1,$2,$3)")
        .bind(target_id)
        .bind(claim_token)
        .bind(claimed_by)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|row| ExtractionClaim {
        project_id: row.get("project_id"),
        document_id: row.get("document_id"),
        source_artifact_id: row.get("source_artifact_id"),
        conversion_generation: row.get("conversion_generation"),
        extraction_generation: row.get("extraction_generation"),
        attempt: row.get("attempt"),
        claim_lease_ms: row.get("claim_lease_ms"),
    }))
}

pub async fn heartbeat_extraction(
    pool: &PgPool,
    target_id: Uuid,
    claim_token: Uuid,
    attempt: i32,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_heartbeat_extraction($1,$2,$3)")
        .bind(target_id)
        .bind(claim_token)
        .bind(attempt)
        .fetch_one(pool)
        .await
}

#[derive(Debug, Clone, Serialize)]
pub struct PublishSection<'a> {
    pub target_id: Uuid,
    pub attempt: i32,
    pub claim_token: Uuid,
    pub section_key: &'a str,
    pub heading_path: &'a Value,
    pub parent_start_offset: i64,
    pub parent_end_offset: i64,
    pub expected_current_publication_id: Option<Uuid>,
    pub candidate_graph: &'a Value,
}

pub async fn publish_extraction_section(
    pool: &PgPool,
    input: PublishSection<'_>,
    context: &MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_bid_publish_extraction_section($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)",
    )
    .bind(input.target_id)
    .bind(input.attempt)
    .bind(input.claim_token)
    .bind(input.section_key)
    .bind(input.heading_path)
    .bind(input.parent_start_offset)
    .bind(input.parent_end_offset)
    .bind(input.expected_current_publication_id)
    .bind(input.candidate_graph)
    .bind(&context.actor)
    .bind(&context.idempotency_key)
    .bind(&context.request.bytes)
    .bind(&context.request.sha256)
    .fetch_one(pool)
    .await
}

pub async fn fail_extraction(
    pool: &PgPool,
    target_id: Uuid,
    attempt: i32,
    claim_token: Uuid,
    error_code: &str,
    retry: bool,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_fail_extraction($1,$2,$3,$4,$5)")
        .bind(target_id)
        .bind(attempt)
        .bind(claim_token)
        .bind(error_code)
        .bind(retry)
        .fetch_one(pool)
        .await
}

pub async fn current_section_publication(
    pool: &PgPool,
    project_id: Uuid,
    document_id: Uuid,
    section_key: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT publication_id FROM bidding_current_section_publication_state WHERE project_id=$1 AND document_id=$2 AND section_key=$3",
    )
    .bind(project_id)
    .bind(document_id)
    .bind(section_key)
    .fetch_optional(pool)
    .await
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Clause {
    pub id: Uuid,
    pub project_id: Uuid,
    pub publication_id: Option<Uuid>,
    pub provenance: String,
    pub status: String,
    pub kind: String,
    pub family: Option<String>,
    pub text: String,
    pub must: bool,
    pub revision: i64,
    pub current_source_span_v2: Option<Value>,
    pub extracted_origin_source_span_v2: Option<Value>,
    pub confirmation_required_reason: Option<String>,
    pub confirmation_required_router_generation: Option<i64>,
}

fn clause_from_row(row: &sqlx::postgres::PgRow) -> Clause {
    Clause {
        id: row.get("id"),
        project_id: row.get("project_id"),
        publication_id: row.get("publication_id"),
        provenance: row.get("provenance"),
        status: row.get("status"),
        kind: row.get("kind"),
        family: row.get("family"),
        text: row.get("text"),
        must: row.get("must"),
        revision: row.get("revision"),
        current_source_span_v2: row.get("current_source_span_v2"),
        extracted_origin_source_span_v2: row.get("extracted_origin_source_span_v2"),
        confirmation_required_reason: row.get("confirmation_required_reason"),
        confirmation_required_router_generation: row.get("confirmation_required_router_generation"),
    }
}

pub async fn list_clauses(
    pool: &PgPool,
    project_id: Uuid,
    include_history: bool,
) -> Result<Vec<Clause>, sqlx::Error> {
    let rows = if include_history {
        sqlx::query(
            "SELECT * FROM bidding_clause_history WHERE project_id=$1 ORDER BY created_at,id",
        )
        .bind(project_id)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query(
            "SELECT * FROM bidding_current_clauses WHERE project_id=$1 ORDER BY created_at,id",
        )
        .bind(project_id)
        .fetch_all(pool)
        .await?
    };
    Ok(rows.iter().map(clause_from_row).collect())
}

pub async fn create_clause(
    pool: &PgPool,
    clause_id: Uuid,
    project_id: Uuid,
    text: &str,
    kind: &str,
    must: bool,
    context: &MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_create_clause($1,$2,$3,$4,$5,$6,$7,$8,$9)")
        .bind(clause_id)
        .bind(project_id)
        .bind(text)
        .bind(kind)
        .bind(must)
        .bind(&context.actor)
        .bind(&context.idempotency_key)
        .bind(&context.request.bytes)
        .bind(&context.request.sha256)
        .fetch_one(pool)
        .await
}

pub async fn mutate_clause(
    pool: &PgPool,
    project_id: Uuid,
    clause_id: Uuid,
    action: &str,
    patch: &Value,
    expected_revision: i64,
    context: &MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_mutate_clause($1,$2,$3,$4,$5,$6,$7,$8,$9)")
        .bind(project_id)
        .bind(clause_id)
        .bind(action)
        .bind(patch)
        .bind(expected_revision)
        .bind(&context.actor)
        .bind(&context.idempotency_key)
        .bind(&context.request.bytes)
        .bind(&context.request.sha256)
        .fetch_one(pool)
        .await
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactSuggestion {
    pub id: Uuid,
    pub project_id: Uuid,
    pub field: String,
    pub typed_value: Value,
    pub raw_quote: String,
    pub confidence: String,
    pub decision_revision: i32,
    pub source_span_v2: Value,
}

pub async fn current_fact_suggestions(
    pool: &PgPool,
    project_id: Uuid,
) -> Result<Vec<FactSuggestion>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT * FROM bidding_current_fact_suggestions WHERE project_id=$1 ORDER BY field,id",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|row| FactSuggestion {
            id: row.get("id"),
            project_id: row.get("project_id"),
            field: row.get("field"),
            typed_value: row.get("typed_value"),
            raw_quote: row.get("raw_quote"),
            confidence: format!("{:.4}", row.get::<rust_decimal::Decimal, _>("confidence")),
            decision_revision: row.get("decision_revision"),
            source_span_v2: row.get("source_span_v2"),
        })
        .collect())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactSuggestionDecision {
    pub id: Uuid,
    pub project_id: Uuid,
    pub candidate_id: Uuid,
    pub revision: i32,
    pub status: String,
    pub reason: Option<String>,
    pub decided_by: String,
    pub decided_at: DateTime<Utc>,
    pub previous_decision_id: Option<Uuid>,
    pub field: String,
    pub typed_value: Value,
    pub raw_quote: String,
    pub confidence: String,
    pub source_span_v2: Value,
}

pub async fn fact_suggestion_history(
    pool: &PgPool,
    project_id: Uuid,
) -> Result<Vec<FactSuggestionDecision>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT * FROM bidding_fact_suggestion_history
         WHERE project_id=$1 ORDER BY candidate_id,revision",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|row| FactSuggestionDecision {
            id: row.get("id"),
            project_id: row.get("project_id"),
            candidate_id: row.get("candidate_id"),
            revision: row.get("revision"),
            status: row.get("status"),
            reason: row.get("reason"),
            decided_by: row.get("decided_by"),
            decided_at: row.get("decided_at"),
            previous_decision_id: row.get("previous_decision_id"),
            field: row.get("field"),
            typed_value: row.get("typed_value"),
            raw_quote: row.get("raw_quote"),
            confidence: format!("{:.4}", row.get::<rust_decimal::Decimal, _>("confidence")),
            source_span_v2: row.get("source_span_v2"),
        })
        .collect())
}

pub struct FactMutation<'a> {
    pub project_id: Uuid,
    pub action: &'a str,
    pub candidate_id: Option<Uuid>,
    pub field: Option<&'a str>,
    pub typed_value: Option<&'a Value>,
    pub reason: Option<&'a str>,
    pub override_reason: Option<&'a str>,
    pub expected_fact_revision: i64,
}

pub async fn mutate_fact(
    pool: &PgPool,
    input: FactMutation<'_>,
    context: &MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_mutate_fact($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)")
        .bind(input.project_id)
        .bind(input.action)
        .bind(input.candidate_id)
        .bind(input.field)
        .bind(input.typed_value)
        .bind(input.reason)
        .bind(input.override_reason)
        .bind(input.expected_fact_revision)
        .bind(&context.actor)
        .bind(&context.idempotency_key)
        .bind(&context.request.bytes)
        .bind(&context.request.sha256)
        .fetch_one(pool)
        .await
}

pub async fn register_kind_router_contract(
    pool: &PgPool,
    version: &str,
    canonical_payload: &[u8],
    content_sha256: &str,
    context: &MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_register_kind_router_contract($1,$2,$3,$4,$5,$6,$7)")
        .bind(version)
        .bind(canonical_payload)
        .bind(content_sha256)
        .bind(&context.actor)
        .bind(&context.idempotency_key)
        .bind(&context.request.bytes)
        .bind(&context.request.sha256)
        .fetch_one(pool)
        .await
}

pub async fn promote_kind_router(
    pool: &PgPool,
    target_version: &str,
    expected_current_version: &str,
    expected_promotion_generation: i64,
    context: &MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_promote_kind_router($1,$2,$3,$4,$5,$6,$7)")
        .bind(target_version)
        .bind(expected_current_version)
        .bind(expected_promotion_generation)
        .bind(&context.actor)
        .bind(&context.idempotency_key)
        .bind(&context.request.bytes)
        .bind(&context.request.sha256)
        .fetch_one(pool)
        .await
}
