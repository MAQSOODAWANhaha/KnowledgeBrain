//! Quote storage adapter. Runtime callers execute only checked functions.

use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::bidding::MutationContext;

pub async fn quote_state(pool: &PgPool, project_id: Uuid) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_quote_state_json($1)")
        .bind(project_id)
        .fetch_one(pool)
        .await
}

pub async fn create_quote_draft(
    pool: &PgPool,
    project_id: Uuid,
    tax_mode: &str,
    title: &str,
    notes: Option<&str>,
    context: &MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_create_quote_draft($1,$2,$3,$4,$5,$6,$7,$8)")
        .bind(project_id)
        .bind(tax_mode)
        .bind(title)
        .bind(notes)
        .bind(&context.actor)
        .bind(&context.idempotency_key)
        .bind(&context.request.bytes)
        .bind(&context.request.sha256)
        .fetch_one(pool)
        .await
}

pub async fn patch_quote_header(
    pool: &PgPool,
    project_id: Uuid,
    expected_edit_version: i64,
    tax_mode: &str,
    title: &str,
    notes: Option<&str>,
    context: &MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_patch_quote_header($1,$2,$3,$4,$5,$6,$7,$8,$9)")
        .bind(project_id)
        .bind(expected_edit_version)
        .bind(tax_mode)
        .bind(title)
        .bind(notes)
        .bind(&context.actor)
        .bind(&context.idempotency_key)
        .bind(&context.request.bytes)
        .bind(&context.request.sha256)
        .fetch_one(pool)
        .await
}

pub struct UpsertQuoteLine<'a> {
    pub project_id: Uuid,
    pub line_id: Uuid,
    pub expected_edit_version: i64,
    pub ordinal: i32,
    pub description: &'a str,
    pub pricing_mode: &'a str,
    pub quantity: Option<&'a str>,
    pub unit: Option<&'a str>,
    pub unit_price: Option<&'a str>,
    pub entered_amount: Option<&'a str>,
    pub tax_rate: &'a str,
    pub user_confirmed: bool,
}

pub async fn upsert_quote_line(
    pool: &PgPool,
    input: UpsertQuoteLine<'_>,
    context: &MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT kb_bid_upsert_quote_line($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)",
    )
    .bind(input.project_id)
    .bind(input.line_id)
    .bind(input.expected_edit_version)
    .bind(input.ordinal)
    .bind(input.description)
    .bind(input.pricing_mode)
    .bind(input.quantity)
    .bind(input.unit)
    .bind(input.unit_price)
    .bind(input.entered_amount)
    .bind(input.tax_rate)
    .bind(input.user_confirmed)
    .bind(&context.actor)
    .bind(&context.idempotency_key)
    .bind(&context.request.bytes)
    .bind(&context.request.sha256)
    .fetch_one(pool)
    .await
}

pub async fn delete_quote_line(
    pool: &PgPool,
    project_id: Uuid,
    line_id: Uuid,
    expected_edit_version: i64,
    context: &MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_delete_quote_line($1,$2,$3,$4,$5,$6,$7)")
        .bind(project_id)
        .bind(line_id)
        .bind(expected_edit_version)
        .bind(&context.actor)
        .bind(&context.idempotency_key)
        .bind(&context.request.bytes)
        .bind(&context.request.sha256)
        .fetch_one(pool)
        .await
}

pub async fn reorder_quote_lines(
    pool: &PgPool,
    project_id: Uuid,
    expected_edit_version: i64,
    line_ids: &[Uuid],
    context: &MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_reorder_quote_lines($1,$2,$3,$4,$5,$6,$7)")
        .bind(project_id)
        .bind(expected_edit_version)
        .bind(line_ids)
        .bind(&context.actor)
        .bind(&context.idempotency_key)
        .bind(&context.request.bytes)
        .bind(&context.request.sha256)
        .fetch_one(pool)
        .await
}

pub async fn preview_quote_totals(pool: &PgPool, project_id: Uuid) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_preview_quote_totals($1)")
        .bind(project_id)
        .fetch_one(pool)
        .await
}

pub struct FinalizeQuote<'a> {
    pub project_id: Uuid,
    pub expected_edit_version: i64,
    pub expected_fact_revision: i64,
    pub expected_ceiling_revision: i64,
    pub expected_ceiling_identity_sha256: &'a str,
    pub expected_pricing_revision: i64,
    pub expected_pricing_set_sha256: &'a str,
    pub no_ceiling_reviewed: bool,
    pub no_ceiling_reason: Option<&'a str>,
}

pub async fn finalize_quote(
    pool: &PgPool,
    input: FinalizeQuote<'_>,
    context: &MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_finalize_quote($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)")
        .bind(input.project_id)
        .bind(input.expected_edit_version)
        .bind(input.expected_fact_revision)
        .bind(input.expected_ceiling_revision)
        .bind(input.expected_ceiling_identity_sha256)
        .bind(input.expected_pricing_revision)
        .bind(input.expected_pricing_set_sha256)
        .bind(input.no_ceiling_reviewed)
        .bind(input.no_ceiling_reason)
        .bind(&context.actor)
        .bind(&context.idempotency_key)
        .bind(&context.request.bytes)
        .bind(&context.request.sha256)
        .fetch_one(pool)
        .await
}

pub async fn reopen_quote(
    pool: &PgPool,
    project_id: Uuid,
    expected_snapshot_id: Uuid,
    expected_fact_revision: i64,
    expected_pricing_revision: i64,
    context: &MutationContext,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_reopen_quote($1,$2,$3,$4,$5,$6,$7,$8)")
        .bind(project_id)
        .bind(expected_snapshot_id)
        .bind(expected_fact_revision)
        .bind(expected_pricing_revision)
        .bind(&context.actor)
        .bind(&context.idempotency_key)
        .bind(&context.request.bytes)
        .bind(&context.request.sha256)
        .fetch_one(pool)
        .await
}

pub async fn get_quote_snapshot(
    pool: &PgPool,
    project_id: Uuid,
    snapshot_id: Uuid,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query_scalar("SELECT kb_bid_get_quote_snapshot($1,$2)")
        .bind(project_id)
        .bind(snapshot_id)
        .fetch_one(pool)
        .await
}
