//! Final V1 storage adapter for `MatchingPublication`.
//!
//! The application interface is intentionally small: schedule, claim/load,
//! heartbeat, publish, and replace a route pick set.  The large-artifact
//! Open/Stage/Commit protocol is private to this implementation.

use domain::knowledge_retrieval::{
    CompanyEvidenceRequestV1, KNOWLEDGE_EVIDENCE_SCHEMA_V1, KnowledgeRetrievalPort,
    ProductEvidenceRequestV1, RetrievalPolicyIdentityV1, validate_evidence_hit_batch,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Row, Transaction};
use std::collections::{BTreeSet, HashMap};
use uuid::Uuid;

const CLAIM_LEASE_MS: i32 = 300_000;
const LEASE_POLICY_GENERATION: i64 = 1;
const STAGING_TTL_MS: i64 = 900_000;
const MAX_BATCH_BYTES: usize = 1024 * 1024;
const MAX_BATCH_ITEMS: usize = 10_000;
const RETRIEVAL_MAX_HITS: u32 = 64;
const RETRIEVAL_MAX_CHUNK_BYTES: u32 = 256 * 1024;
const RETRIEVAL_MAX_TOTAL_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MatchRoute {
    Technical { unit_id: Uuid },
    Commercial,
}

impl MatchRoute {
    fn parts(&self) -> (&'static str, Option<Uuid>) {
        match self {
            Self::Technical { unit_id } => ("technical", Some(*unit_id)),
            Self::Commercial => ("commercial", None),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnvelopeSnapshotIdentity {
    pub config_snapshot_id: Uuid,
    pub feature_snapshot_id: Uuid,
    pub score_policy_snapshot_id: Uuid,
    pub verifier_policy_snapshot_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct ScheduledJob {
    pub id: Uuid,
    pub snapshots: EnvelopeSnapshotIdentity,
}

#[derive(Debug, Clone)]
pub struct ScheduleReceipt {
    pub manifest_id: Uuid,
    pub jobs: Vec<ScheduledJob>,
}

#[derive(Debug, Clone)]
pub struct ScheduleEnvironment {
    pub environment: String,
    pub max_attempts: i32,
}

#[derive(Debug, Clone)]
pub enum ScheduleMutationContext {
    Human {
        actor: String,
        idempotency_key: String,
    },
    System,
}

impl ScheduleMutationContext {
    pub fn human(actor: String, idempotency_key: String) -> Self {
        Self::Human {
            actor,
            idempotency_key,
        }
    }

    pub fn system() -> Self {
        Self::System
    }

    fn identity(&self, project_id: Uuid, watermark: i64) -> (String, String) {
        match self {
            Self::Human {
                actor,
                idempotency_key,
            } => (actor.clone(), idempotency_key.clone()),
            Self::System => (
                "system:matching-publication".into(),
                format!("matching-schedule:{project_id}:{watermark}"),
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchClaim {
    pub token: Uuid,
    pub attempt: i32,
    pub claim_lease_ms: i32,
    pub lease_policy_generation: i64,
}

#[derive(Debug, Clone)]
pub struct LoadedRequirement {
    pub id: Uuid,
    pub ordinal: u32,
    pub text: String,
    pub requirement_sha256: String,
}

#[derive(Debug, Clone)]
pub struct LoadedFrozenHit {
    pub requirement_artifact_id: Uuid,
    pub product_version_artifact_id: Uuid,
    pub route_product_ordinal: u32,
    pub document_id: Uuid,
    pub source_chunk_id: Uuid,
    pub frozen_document_display_name: String,
    pub chunk_utf8: String,
    pub chunk_sha256: String,
    pub chunk_byte_length: u64,
    pub retrieval_rank: u32,
    pub retrieval_raw_score: String,
    pub quote_start_offset: u64,
    pub quote_end_offset: u64,
    pub offset_unit: String,
    pub retrieval_contract_version: String,
}

#[derive(Debug, Clone)]
pub struct ClaimedMatchingRequest {
    pub job_id: Uuid,
    pub manifest_id: Uuid,
    pub project_id: Uuid,
    pub generation: i64,
    pub mutation_watermark: i64,
    pub route_id: Uuid,
    pub route: MatchRoute,
    pub empty_policy: String,
    pub claim: MatchClaim,
    pub requirements: Vec<LoadedRequirement>,
    pub frozen_hits: Vec<LoadedFrozenHit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StagedSourceArtifactV1 {
    pub id: Uuid,
    pub product_version_artifact_id: Uuid,
    pub document_id: Uuid,
    pub source_chunk_id: Uuid,
    pub frozen_document_display_name: String,
    pub chunk_utf8: String,
    pub chunk_sha256: String,
    pub chunk_byte_length: u64,
    pub retrieval_rank: u32,
    pub retrieval_raw_score: String,
    pub retrieval_contract_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StagedEvidenceV1 {
    pub id: Uuid,
    pub candidate_artifact_id: Uuid,
    pub source_chunk_artifact_id: Uuid,
    pub document_id: Uuid,
    pub document_display_name: String,
    pub source_chunk_id: Uuid,
    pub source_chunk_sha256: String,
    pub quote: String,
    pub start_offset: u64,
    pub end_offset: u64,
    pub offset_unit: String,
    pub ordinal: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StagedCandidateV1 {
    pub id: Uuid,
    pub requirement_artifact_id: Uuid,
    pub product_version_artifact_id: Uuid,
    pub route_product_ordinal: u32,
    pub retrieval_rank: u32,
    pub retrieval_raw_score: String,
    pub candidate_identity_sha256: String,
    pub evidence_v1_sha256: String,
    pub support: String,
    pub business_value_status: String,
    pub business_value: Option<String>,
    pub recommended: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StagedDecisionV1 {
    pub id: Uuid,
    pub requirement_artifact_id: Uuid,
    pub final_support: String,
    pub system_decision: String,
    pub quality_status: String,
    pub reason_code: String,
    pub selected_candidate_artifact_id: Option<Uuid>,
    pub ordinal: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StagedCandidateGroupV1 {
    pub id: Uuid,
    pub requirement_artifact_id: Uuid,
    pub support: String,
    pub ordinal: u32,
    pub canonical_payload: Vec<u8>,
    pub content_sha256: String,
}

#[derive(Debug, Clone)]
pub struct PublishRouteV2 {
    pub report_id: Uuid,
    pub report_nonce: Uuid,
    pub canonical_payload: Vec<u8>,
    pub sources: Vec<StagedSourceArtifactV1>,
    pub candidates: Vec<StagedCandidateV1>,
    pub evidences: Vec<StagedEvidenceV1>,
    pub decisions: Vec<StagedDecisionV1>,
    pub candidate_groups: Vec<StagedCandidateGroupV1>,
    pub reason_codes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishReceipt {
    Committed { report_id: Uuid },
    Replayed { report_id: Uuid },
    Stale,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PickSelectionV1 {
    pub requirement_artifact_id: Uuid,
    pub candidate_artifact_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct ReplaceRoutePickSetV1 {
    pub project_id: Uuid,
    pub route_id: Uuid,
    pub source_report_artifact_id: Uuid,
    pub report_sha256: String,
    pub expected_revision: i64,
    pub selections: Vec<PickSelectionV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PickSetReceiptV1 {
    pub route_pick_set_id: Uuid,
    pub route_revision: i64,
    pub route_sha256: String,
    pub project_pick_set_id: Uuid,
    pub project_revision: i64,
    pub project_sha256: String,
}

#[derive(Debug, Clone)]
struct FrozenRequirementInput {
    id: Uuid,
    route_id: Uuid,
    clause_id: Uuid,
    ordinal: u32,
    text: String,
    sha256: String,
}

#[derive(Debug, Clone)]
struct FrozenProductInput {
    id: Uuid,
    product_id: Uuid,
    product_version_id: Uuid,
    workspace_kind: String,
    frozen_display_name: String,
    identity_sha256: String,
}

#[derive(Debug, Clone)]
struct FrozenHitInput {
    id: Uuid,
    requirement_artifact_id: Uuid,
    product_version_artifact_id: Uuid,
    route_id: Uuid,
    hit: domain::knowledge_retrieval::KnowledgeEvidenceHitV1,
}

#[derive(Debug, Clone)]
struct FrozenRouteInput {
    id: Uuid,
    route: MatchRoute,
    ordinal: u32,
    empty_policy: String,
    scope_sha256: String,
}

/// Invalidate every matching consumer in the caller's domain transaction.
/// Clause/fact publishers use this same seam; no asynchronous repair scan is
/// required to hide stale reports or picks.
pub async fn mark_project_matching_mutation(
    tx: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
) -> Result<(), sqlx::Error> {
    let updated = sqlx::query(
        "UPDATE bid_projects SET matching_mutation_watermark=matching_mutation_watermark+1,
          updated_at=clock_timestamp() WHERE id=$1 AND status='open'",
    )
    .bind(project_id)
    .execute(&mut **tx)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(protocol("bid project is not open"));
    }
    sqlx::query("DELETE FROM bid_current_matching_reports WHERE project_id=$1")
        .bind(project_id)
        .execute(&mut **tx)
        .await?;
    sqlx::query("DELETE FROM bid_current_route_pick_sets WHERE project_id=$1")
        .bind(project_id)
        .execute(&mut **tx)
        .await?;
    rebuild_project_pick_set(tx, project_id, "system:matching-invalidation").await?;
    sqlx::query(
        "UPDATE bid_current_parts SET stale=true,
          stale_reason_codes=(SELECT ARRAY(SELECT DISTINCT code
            FROM unnest(stale_reason_codes||ARRAY['MATCHING_INPUT_CHANGED']) code ORDER BY code))
          WHERE project_id=$1 AND (part_key LIKE '2:%' OR part_key IN ('3','4','5','6:implementation_plan'))",
    )
    .bind(project_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn schedule_dirty_project(
    pool: &PgPool,
    project_id: Uuid,
    environment: ScheduleEnvironment,
    context: &ScheduleMutationContext,
) -> Result<Option<ScheduleReceipt>, sqlx::Error> {
    let port = crate::knowledge_retrieval::PostgresKnowledgeRetrievalAdapter::new(pool.clone());
    schedule_dirty_project_with_port(pool, project_id, environment, context, &port).await
}

async fn schedule_dirty_project_with_port<P: KnowledgeRetrievalPort>(
    pool: &PgPool,
    project_id: Uuid,
    environment: ScheduleEnvironment,
    context: &ScheduleMutationContext,
    port: &P,
) -> Result<Option<ScheduleReceipt>, sqlx::Error> {
    if !(1..=32).contains(&environment.max_attempts)
        || !matches!(
            environment.environment.as_str(),
            "development" | "test" | "production"
        )
    {
        return Err(protocol("invalid matching scheduling policy"));
    }
    let project =
        sqlx::query("SELECT status,matching_mutation_watermark FROM bidding_projects WHERE id=$1")
            .bind(project_id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| protocol("bid project does not exist"))?;
    if project.get::<String, _>("status") != "open" {
        return Err(protocol("bid project is not open"));
    }
    let watermark: i64 = project.get("matching_mutation_watermark");
    let (actor, idempotency_key) = context.identity(project_id, watermark);
    let request_bytes = serde_json::to_vec(&serde_json::json!({
        "schema_version": 1,
        "project_id": project_id,
        "expected_watermark": watermark,
        "environment": environment.environment,
        "max_attempts": environment.max_attempts
    }))
    .expect("matching schedule request serializes");
    let request_sha256 = sha256_hex(&request_bytes);
    let already_scheduled: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM bidding_current_routes WHERE project_id=$1 AND mutation_watermark=$2)",
    )
    .bind(project_id)
    .bind(watermark)
    .fetch_one(pool)
    .await?;
    if already_scheduled {
        return persist_schedule(
            pool,
            PersistScheduleInput {
                project_id,
                watermark,
                max_attempts: environment.max_attempts,
                payload: &serde_json::json!({}),
                actor: &actor,
                idempotency_key: &idempotency_key,
                request_bytes: &request_bytes,
                request_sha256: &request_sha256,
            },
        )
        .await;
    }

    let clause_rows = sqlx::query(
        "SELECT c.id,c.text,c.family,
                CASE WHEN c.family='technical' THEN COALESCE(
                    (c.current_source_span_v2->>'section_artifact_id')::uuid,
                    '00000000-0000-0000-0000-000000000000'::uuid) ELSE NULL END AS unit_id
           FROM bidding_current_clauses c
          WHERE c.project_id=$1 AND c.status='confirmed' AND c.family IS NOT NULL
          ORDER BY c.family,c.id",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;

    let manifest_id = Uuid::new_v4();
    let mut technical_units = BTreeSet::new();
    for row in &clause_rows {
        if row.get::<String, _>("family") == "technical" {
            technical_units.insert(row.get::<Uuid, _>("unit_id"));
        }
    }
    let mut routes = Vec::new();
    for unit_id in technical_units {
        routes.push(FrozenRouteInput {
            id: Uuid::new_v4(),
            route: MatchRoute::Technical { unit_id },
            ordinal: routes.len() as u32,
            empty_policy: "clear_route".into(),
            scope_sha256: sha256_hex(format!("technical:{unit_id}").as_bytes()),
        });
    }
    routes.push(FrozenRouteInput {
        id: Uuid::new_v4(),
        route: MatchRoute::Commercial,
        ordinal: routes.len() as u32,
        empty_policy: "clear_route".into(),
        scope_sha256: sha256_hex(b"commercial"),
    });
    let commercial_route = routes
        .iter()
        .find(|route| route.route == MatchRoute::Commercial)
        .expect("commercial route exists")
        .id;
    let route_by_unit: HashMap<Uuid, Uuid> = routes
        .iter()
        .filter_map(|route| match route.route {
            MatchRoute::Technical { unit_id } => Some((unit_id, route.id)),
            MatchRoute::Commercial => None,
        })
        .collect();
    let mut requirements = Vec::new();
    for row in clause_rows {
        let family: String = row.get("family");
        let route_id = if family == "technical" {
            route_by_unit[&row.get::<Uuid, _>("unit_id")]
        } else {
            commercial_route
        };
        let text: String = row.get("text");
        requirements.push(FrozenRequirementInput {
            id: Uuid::new_v4(),
            route_id,
            clause_id: row.get("id"),
            ordinal: 0,
            sha256: sha256_hex(text.as_bytes()),
            text,
        });
    }
    requirements.sort_by_key(|row| (row.route_id, row.clause_id));
    let mut route_ordinals: HashMap<Uuid, u32> = HashMap::new();
    for requirement in &mut requirements {
        let ordinal = route_ordinals.entry(requirement.route_id).or_default();
        requirement.ordinal = *ordinal;
        *ordinal += 1;
    }

    let policy = RetrievalPolicyIdentityV1 {
        contract_version: "knowledge-evidence-v1".into(),
        policy_sha256: sha256_hex(b"knowledge-evidence-v1:lexical-current-eligible"),
        max_hits: RETRIEVAL_MAX_HITS,
        max_chunk_bytes: RETRIEVAL_MAX_CHUNK_BYTES,
        max_total_bytes: RETRIEVAL_MAX_TOTAL_BYTES,
    };
    let route_lookup: HashMap<Uuid, &FrozenRouteInput> =
        routes.iter().map(|route| (route.id, route)).collect();
    let mut raw_hits = Vec::new();
    for requirement in &requirements {
        let (workspace_kind, hits) = match route_lookup[&requirement.route_id].route {
            MatchRoute::Technical { .. } => (
                "product_line",
                port.retrieve_product_evidence(ProductEvidenceRequestV1 {
                    schema_version: KNOWLEDGE_EVIDENCE_SCHEMA_V1,
                    requirement_identity_sha256: requirement.sha256.clone(),
                    requirement_text: requirement.text.clone(),
                    product_version_ids: Vec::new(),
                    retrieval_policy: policy.clone(),
                })
                .await
                .map_err(|error| protocol(error.to_string()))?
                .into_iter()
                .map(|value| value.0)
                .collect::<Vec<_>>(),
            ),
            MatchRoute::Commercial => (
                "company",
                port.retrieve_company_evidence(CompanyEvidenceRequestV1 {
                    schema_version: KNOWLEDGE_EVIDENCE_SCHEMA_V1,
                    requirement_identity_sha256: requirement.sha256.clone(),
                    requirement_text: requirement.text.clone(),
                    library_version_ids: Vec::new(),
                    retrieval_policy: policy.clone(),
                })
                .await
                .map_err(|error| protocol(error.to_string()))?
                .into_iter()
                .map(|value| value.0)
                .collect::<Vec<_>>(),
            ),
        };
        validate_evidence_hit_batch(workspace_kind, &hits, &policy)
            .map_err(|error| protocol(error.to_string()))?;
        for hit in hits {
            raw_hits.push((requirement, hit));
        }
    }

    let mut product_keys = BTreeSet::new();
    for (_, hit) in &raw_hits {
        product_keys.insert((
            hit.product_id,
            hit.product_version_id,
            hit.workspace_kind.clone(),
        ));
    }
    let mut products = Vec::new();
    for (product_id, product_version_id, workspace_kind) in product_keys {
        let identity = sha256_hex(
            format!("ProductVersionEvidenceV1:{product_id}:{product_version_id}:{workspace_kind}")
                .as_bytes(),
        );
        products.push(FrozenProductInput {
            id: deterministic_uuid(
                "ProductVersionArtifactV1",
                format!("{manifest_id}:{identity}").as_bytes(),
            ),
            product_id,
            product_version_id,
            workspace_kind,
            frozen_display_name: product_version_id.to_string(),
            identity_sha256: identity,
        });
    }
    let product_by_version: HashMap<Uuid, Uuid> = products
        .iter()
        .map(|row| (row.product_version_id, row.id))
        .collect();
    let mut frozen_hits = Vec::new();
    for (requirement, hit) in raw_hits {
        frozen_hits.push(FrozenHitInput {
            id: deterministic_uuid(
                "FrozenRetrievedHitV1",
                format!(
                    "{}:{}:{}:{}:{}:{}",
                    requirement.id,
                    hit.product_version_id,
                    hit.document_id,
                    hit.source_chunk_id,
                    hit.quote_start_offset,
                    hit.quote_end_offset
                )
                .as_bytes(),
            ),
            requirement_artifact_id: requirement.id,
            product_version_artifact_id: product_by_version[&hit.product_version_id],
            route_id: requirement.route_id,
            hit,
        });
    }
    frozen_hits.sort_by_key(|row| {
        (
            row.route_id,
            row.requirement_artifact_id,
            row.hit.retrieval_rank,
            row.id,
        )
    });

    let requirement_set_sha256 = sha256_hex(
        &serde_json::to_vec(
            &requirements
                .iter()
                .map(|row| (&row.route_id, &row.clause_id, &row.text, &row.sha256))
                .collect::<Vec<_>>(),
        )
        .expect("frozen requirements serialize"),
    );
    let eligible_scope_sha256 = sha256_hex(
        &serde_json::to_vec(
            &products
                .iter()
                .map(|row| {
                    (
                        &row.product_id,
                        &row.product_version_id,
                        &row.identity_sha256,
                    )
                })
                .collect::<Vec<_>>(),
        )
        .expect("frozen scope serializes"),
    );

    let mut memberships = Vec::new();
    for route in &routes {
        let mut members: BTreeSet<(Uuid, Uuid)> = BTreeSet::new();
        for hit in frozen_hits.iter().filter(|hit| hit.route_id == route.id) {
            members.insert((hit.hit.product_version_id, hit.product_version_artifact_id));
        }
        for (ordinal, (_, artifact_id)) in members.into_iter().enumerate() {
            memberships.push(serde_json::json!({
                "route_id": route.id,
                "product_version_artifact_id": artifact_id,
                "route_product_ordinal": ordinal
            }));
        }
    }
    let payload = serde_json::json!({
        "schema_version": 1,
        "manifest_id": manifest_id,
        "project_id": project_id,
        "mutation_watermark": watermark,
        "requirement_set_sha256": requirement_set_sha256,
        "eligible_scope_sha256": eligible_scope_sha256,
        "routes": routes.iter().map(|route| {
            let (kind, unit_id) = route.route.parts();
            serde_json::json!({
                "id": route.id,
                "route_kind": kind,
                "unit_id": unit_id,
                "ordinal": route.ordinal,
                "empty_policy": route.empty_policy,
                "route_scope_sha256": route.scope_sha256
            })
        }).collect::<Vec<_>>(),
        "requirements": requirements.iter().map(|row| serde_json::json!({
            "id": row.id,
            "route_id": row.route_id,
            "clause_id": row.clause_id,
            "ordinal": row.ordinal,
            "text": row.text,
            "sha256": row.sha256
        })).collect::<Vec<_>>(),
        "products": products.iter().map(|row| serde_json::json!({
            "id": row.id,
            "product_id": row.product_id,
            "product_version_id": row.product_version_id,
            "workspace_kind": row.workspace_kind,
            "frozen_display_name": row.frozen_display_name,
            "identity_sha256": row.identity_sha256
        })).collect::<Vec<_>>(),
        "memberships": memberships,
        "frozen_hits": frozen_hits.iter().map(|row| serde_json::json!({
            "id": row.id,
            "route_id": row.route_id,
            "requirement_artifact_id": row.requirement_artifact_id,
            "product_version_artifact_id": row.product_version_artifact_id,
            "document_id": row.hit.document_id,
            "source_chunk_id": row.hit.source_chunk_id,
            "frozen_document_display_name": row.hit.frozen_document_display_name,
            "chunk_utf8": row.hit.chunk_utf8,
            "chunk_sha256": row.hit.chunk_sha256,
            "chunk_byte_length": row.hit.chunk_byte_length,
            "retrieval_rank": row.hit.retrieval_rank,
            "retrieval_raw_score": row.hit.retrieval_raw_score,
            "quote_start_offset": row.hit.quote_start_offset,
            "quote_end_offset": row.hit.quote_end_offset,
            "offset_unit": row.hit.offset_unit,
            "retrieval_contract_version": row.hit.retrieval_contract_version
        })).collect::<Vec<_>>()
    });
    persist_schedule(
        pool,
        PersistScheduleInput {
            project_id,
            watermark,
            max_attempts: environment.max_attempts,
            payload: &payload,
            actor: &actor,
            idempotency_key: &idempotency_key,
            request_bytes: &request_bytes,
            request_sha256: &request_sha256,
        },
    )
    .await
}

struct PersistScheduleInput<'a> {
    project_id: Uuid,
    watermark: i64,
    max_attempts: i32,
    payload: &'a serde_json::Value,
    actor: &'a str,
    idempotency_key: &'a str,
    request_bytes: &'a [u8],
    request_sha256: &'a str,
}

async fn persist_schedule(
    pool: &PgPool,
    input: PersistScheduleInput<'_>,
) -> Result<Option<ScheduleReceipt>, sqlx::Error> {
    let scheduled: Option<serde_json::Value> =
        sqlx::query_scalar("SELECT kb_bid_matching_schedule($1,$2,$3,$4,$5,$6,$7,$8)")
            .bind(input.project_id)
            .bind(input.watermark)
            .bind(input.max_attempts)
            .bind(input.payload)
            .bind(input.actor)
            .bind(input.idempotency_key)
            .bind(input.request_bytes)
            .bind(input.request_sha256)
            .fetch_one(pool)
            .await?;
    let Some(scheduled) = scheduled else {
        return Ok(None);
    };
    let manifest_id = scheduled
        .get("manifest_id")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| protocol("matching schedule receipt missing manifest"))?;
    let snapshots = snapshot_identity(manifest_id);
    let jobs = scheduled
        .get("job_ids")
        .and_then(|value| value.as_array())
        .ok_or_else(|| protocol("matching schedule receipt missing jobs"))?
        .iter()
        .filter_map(|value| value.as_str().and_then(|value| Uuid::parse_str(value).ok()))
        .map(|id| ScheduledJob { id, snapshots })
        .collect();
    Ok(Some(ScheduleReceipt { manifest_id, jobs }))
}

pub async fn claim_and_load(
    pool: &PgPool,
    job_id: Uuid,
    snapshots: EnvelopeSnapshotIdentity,
) -> Result<Option<ClaimedMatchingRequest>, sqlx::Error> {
    let token = Uuid::new_v4();
    let claimed: Option<serde_json::Value> =
        sqlx::query_scalar("SELECT kb_bid_matching_claim($1,$2)")
            .bind(job_id)
            .bind(token)
            .fetch_one(pool)
            .await?;
    let Some(claimed) = claimed else {
        return Ok(None);
    };
    let manifest_id = claimed
        .get("manifest_id")
        .and_then(|value| value.as_str())
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| protocol("matching claim missing manifest"))?;
    if snapshots != snapshot_identity(manifest_id) {
        return Err(protocol("matching envelope snapshot mismatch"));
    }
    let mut tx = pool.begin().await?;
    let job = sqlx::query(
        "SELECT j.*,m.generation,m.mutation_watermark,r.route_kind,r.unit_id,r.empty_policy
           FROM bid_matching_jobs j
           JOIN bid_matching_manifests m ON m.id=j.manifest_id
           JOIN bid_matching_routes r ON r.id=j.route_id
          WHERE j.id=$1",
    )
    .bind(job_id)
    .fetch_one(&mut *tx)
    .await?;
    let attempt = claimed
        .get("attempt")
        .and_then(|value| value.as_i64())
        .ok_or_else(|| protocol("matching claim missing attempt"))? as i32;
    let lease_ms = claimed
        .get("claim_lease_ms")
        .and_then(|value| value.as_i64())
        .unwrap_or(CLAIM_LEASE_MS as i64) as i32;
    let lease_generation = claimed
        .get("lease_policy_generation")
        .and_then(|value| value.as_i64())
        .unwrap_or(LEASE_POLICY_GENERATION);
    let request = load_claimed(&mut tx, &job, token, attempt, lease_ms, lease_generation).await?;
    tx.commit().await?;
    Ok(Some(request))
}

async fn load_claimed(
    tx: &mut Transaction<'_, Postgres>,
    job: &sqlx::postgres::PgRow,
    token: Uuid,
    attempt: i32,
    lease_ms: i32,
    lease_generation: i64,
) -> Result<ClaimedMatchingRequest, sqlx::Error> {
    let route_id: Uuid = job.get("route_id");
    let requirements = sqlx::query(
        "SELECT id,ordinal,requirement_text,requirement_sha256
           FROM bid_matching_requirement_artifacts WHERE route_id=$1 ORDER BY ordinal,id",
    )
    .bind(route_id)
    .fetch_all(&mut **tx)
    .await?
    .into_iter()
    .map(|row| LoadedRequirement {
        id: row.get("id"),
        ordinal: row.get::<i32, _>("ordinal") as u32,
        text: row.get("requirement_text"),
        requirement_sha256: row.get("requirement_sha256"),
    })
    .collect();
    let frozen_hits = sqlx::query(
        "SELECT h.*,membership.route_product_ordinal
           FROM bid_matching_frozen_retrieved_hits h
           JOIN bid_matching_route_memberships membership
             ON membership.route_id=h.route_id
            AND membership.product_version_artifact_id=h.product_version_artifact_id
          WHERE h.route_id=$1
          ORDER BY h.requirement_artifact_id,membership.route_product_ordinal,h.retrieval_rank,h.id",
    )
    .bind(route_id)
    .fetch_all(&mut **tx)
    .await?
    .into_iter()
    .map(|row| LoadedFrozenHit {
        requirement_artifact_id: row.get("requirement_artifact_id"),
        product_version_artifact_id: row.get("product_version_artifact_id"),
        route_product_ordinal: row.get::<i32, _>("route_product_ordinal") as u32,
        document_id: row.get("document_id"),
        source_chunk_id: row.get("source_chunk_id"),
        frozen_document_display_name: row.get("frozen_document_display_name"),
        chunk_utf8: String::from_utf8(row.get::<Vec<u8>, _>("chunk_utf8")).unwrap_or_default(),
        chunk_sha256: row.get("chunk_sha256"),
        chunk_byte_length: row.get::<i64, _>("chunk_byte_length") as u64,
        retrieval_rank: row.get::<i32, _>("retrieval_rank") as u32,
        retrieval_raw_score: format!("{:.6}", row.get::<Decimal, _>("retrieval_raw_score")),
        quote_start_offset: row.get::<i64, _>("quote_start_offset") as u64,
        quote_end_offset: row.get::<i64, _>("quote_end_offset") as u64,
        offset_unit: row.get("offset_unit"),
        retrieval_contract_version: row.get("retrieval_contract_version"),
    })
    .collect();
    let route = if job.get::<String, _>("route_kind") == "technical" {
        MatchRoute::Technical {
            unit_id: job.get("unit_id"),
        }
    } else {
        MatchRoute::Commercial
    };
    Ok(ClaimedMatchingRequest {
        job_id: job.get("id"),
        manifest_id: job.get("manifest_id"),
        project_id: job.get("project_id"),
        generation: job.get("generation"),
        mutation_watermark: job.get("mutation_watermark"),
        route_id,
        route,
        empty_policy: job.get("empty_policy"),
        claim: MatchClaim {
            token,
            attempt,
            claim_lease_ms: lease_ms,
            lease_policy_generation: lease_generation,
        },
        requirements,
        frozen_hits,
    })
}

pub async fn heartbeat_claim(
    pool: &PgPool,
    request: &ClaimedMatchingRequest,
) -> Result<bool, sqlx::Error> {
    let ok: bool = sqlx::query_scalar("SELECT kb_bid_matching_heartbeat($1,$2,$3,$4,$5,$6)")
        .bind(request.job_id)
        .bind(request.claim.token)
        .bind(request.claim.attempt)
        .bind(request.claim.claim_lease_ms)
        .bind(request.claim.lease_policy_generation)
        .bind(STAGING_TTL_MS as i32)
        .fetch_one(pool)
        .await?;
    if ok { Ok(true) } else { Ok(false) }
}

pub async fn publish_route(
    pool: &PgPool,
    claimed: &ClaimedMatchingRequest,
    report: PublishRouteV2,
) -> Result<PublishReceipt, sqlx::Error> {
    // The adapter is the single authority that derives the persisted report
    // hash. Callers provide canonical bytes, never a second digest assertion.
    let report_sha256 = sha256_hex(&report.canonical_payload);
    if let Some((status, completed_report_id, completed_report_sha256)) =
        sqlx::query_as::<_, (String, Option<Uuid>, Option<String>)>(
            "SELECT job.status,job.completed_report_id,report.content_sha256
           FROM bid_matching_jobs job
           LEFT JOIN bidding_matching_report_history report ON report.id=job.completed_report_id
          WHERE job.id=$1",
        )
        .bind(claimed.job_id)
        .fetch_optional(pool)
        .await?
        && status == "completed"
    {
        return completed_publish_receipt(
            completed_report_id,
            completed_report_sha256.as_deref(),
            report.report_id,
            &report_sha256,
        );
    }
    let batches = stage_collections(&report)?;
    let staging_id = open_staging_set(pool, claimed, &report, &batches).await?;
    stage_report_payload_for_set(
        pool,
        claimed,
        staging_id,
        &report.canonical_payload,
        &report_sha256,
    )
    .await?;
    for batch in batches {
        stage_batch(pool, claimed, staging_id, batch).await?;
    }
    commit_staged_route(pool, claimed, staging_id, report.report_id, &report_sha256).await
}

fn completed_publish_receipt(
    completed_report_id: Option<Uuid>,
    completed_report_sha256: Option<&str>,
    requested_report_id: Uuid,
    requested_report_sha256: &str,
) -> Result<PublishReceipt, sqlx::Error> {
    if completed_report_id != Some(requested_report_id) {
        return Ok(PublishReceipt::Stale);
    }
    if completed_report_sha256 != Some(requested_report_sha256) {
        return Err(protocol("COMPLETED_REPORT_PAYLOAD_MISMATCH"));
    }
    Ok(PublishReceipt::Replayed {
        report_id: requested_report_id,
    })
}

#[derive(Debug)]
struct StageBatch {
    ordinal: i32,
    kind: &'static str,
    bytes: Vec<u8>,
    item_count: usize,
}

async fn open_staging_set(
    pool: &PgPool,
    claimed: &ClaimedMatchingRequest,
    report: &PublishRouteV2,
    batches: &[StageBatch],
) -> Result<Uuid, sqlx::Error> {
    let expected_item_count = batches
        .iter()
        .map(|batch| batch.item_count as i64)
        .sum::<i64>();
    let expected_byte_length = batches
        .iter()
        .map(|batch| batch.bytes.len() as i64)
        .sum::<i64>();
    let open_hash = sha256_hex(
        format!(
            "OpenStagingSetV1:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}",
            claimed.job_id,
            claimed.claim.attempt,
            claimed.route_id,
            claimed.manifest_id,
            claimed.project_id,
            claimed.generation,
            claimed.mutation_watermark,
            report.report_nonce,
            batches.len(),
            expected_item_count,
            expected_byte_length
        )
        .as_bytes(),
    );
    let payload = serde_json::json!({
        "job_id": claimed.job_id,
        "claim_token": claimed.claim.token,
        "attempt": claimed.claim.attempt,
        "route_id": claimed.route_id,
        "manifest_id": claimed.manifest_id,
        "project_id": claimed.project_id,
        "generation": claimed.generation,
        "mutation_watermark": claimed.mutation_watermark,
        "report_nonce": report.report_nonce,
        "open_payload_sha256": open_hash,
        "expected_batch_count": batches.len(),
        "expected_item_count": expected_item_count,
        "expected_byte_length": expected_byte_length,
        "ttl_ms": STAGING_TTL_MS
    });
    sqlx::query_scalar("SELECT kb_bid_matching_open_staging($1)")
        .bind(payload)
        .fetch_one(pool)
        .await
}

fn stage_collections(report: &PublishRouteV2) -> Result<Vec<StageBatch>, sqlx::Error> {
    let mut batches = Vec::new();
    split_collection("source_artifacts", &report.sources, &mut batches)?;
    split_collection("candidates", &report.candidates, &mut batches)?;
    split_collection("evidences", &report.evidences, &mut batches)?;
    split_collection("requirement_decisions", &report.decisions, &mut batches)?;
    split_collection("candidate_groups", &report.candidate_groups, &mut batches)?;
    split_collection("reason_codes", &report.reason_codes, &mut batches)?;
    Ok(batches)
}

fn split_collection<T: Serialize>(
    kind: &'static str,
    items: &[T],
    out: &mut Vec<StageBatch>,
) -> Result<(), sqlx::Error> {
    if items.is_empty() {
        out.push(StageBatch {
            ordinal: out.len() as i32,
            kind,
            bytes: b"[]".to_vec(),
            item_count: 0,
        });
        return Ok(());
    }
    let mut current = Vec::<serde_json::Value>::new();
    for item in items {
        let value = serde_json::to_value(item).map_err(|error| protocol(error.to_string()))?;
        let single_size = serde_json::to_vec(&[&value])
            .map_err(|error| protocol(error.to_string()))?
            .len();
        if single_size > MAX_BATCH_BYTES {
            return Err(protocol("STAGING_ITEM_QUOTA_EXCEEDED"));
        }
        current.push(value);
        let bytes = serde_json::to_vec(&current).map_err(|error| protocol(error.to_string()))?;
        if (bytes.len() > MAX_BATCH_BYTES || current.len() > MAX_BATCH_ITEMS) && current.len() > 1 {
            let last = current.pop().expect("batch contains the last item");
            let completed =
                serde_json::to_vec(&current).map_err(|error| protocol(error.to_string()))?;
            out.push(StageBatch {
                ordinal: out.len() as i32,
                kind,
                item_count: current.len(),
                bytes: completed,
            });
            current = vec![last];
        }
    }
    let bytes = serde_json::to_vec(&current).map_err(|error| protocol(error.to_string()))?;
    out.push(StageBatch {
        ordinal: out.len() as i32,
        kind,
        item_count: current.len(),
        bytes,
    });
    Ok(())
}

async fn stage_batch(
    pool: &PgPool,
    claimed: &ClaimedMatchingRequest,
    staging_id: Uuid,
    batch: StageBatch,
) -> Result<(), sqlx::Error> {
    let items = stage_batch_items(batch.kind, &batch.bytes)?;
    let payload = serde_json::json!({
        "staging_set_id": staging_id,
        "job_id": claimed.job_id,
        "claim_token": claimed.claim.token,
        "attempt": claimed.claim.attempt,
        "batch_ordinal": batch.ordinal,
        "collection_kind": batch.kind,
        "canonical_items_b64": base64_encode(&batch.bytes),
        "payload_sha256": sha256_hex(&batch.bytes),
        "item_count": batch.item_count,
        "byte_length": batch.bytes.len(),
        "items": items
    });
    sqlx::query("SELECT kb_bid_matching_stage_batch($1)")
        .bind(payload)
        .execute(pool)
        .await?;
    Ok(())
}

fn stage_batch_items(kind: &str, bytes: &[u8]) -> Result<serde_json::Value, sqlx::Error> {
    if kind != "candidate_groups" {
        return serde_json::from_slice(bytes).map_err(|error| protocol(error.to_string()));
    }
    let groups: Vec<StagedCandidateGroupV1> =
        serde_json::from_slice(bytes).map_err(|error| protocol(error.to_string()))?;
    Ok(serde_json::Value::Array(
        groups
            .into_iter()
            .map(|group| {
                serde_json::json!({
                    "id": group.id,
                    "requirement_artifact_id": group.requirement_artifact_id,
                    "support": group.support,
                    "ordinal": group.ordinal,
                    "canonical_payload_b64": base64_encode(&group.canonical_payload),
                    "content_sha256": group.content_sha256,
                })
            })
            .collect(),
    ))
}

async fn commit_staged_route(
    pool: &PgPool,
    claimed: &ClaimedMatchingRequest,
    staging_id: Uuid,
    report_id: Uuid,
    expected_report_sha256: &str,
) -> Result<PublishReceipt, sqlx::Error> {
    let receipt: serde_json::Value =
        sqlx::query_scalar("SELECT kb_bid_matching_commit($1,$2,$3,$4,$5,$6)")
            .bind(claimed.job_id)
            .bind(claimed.claim.token)
            .bind(claimed.claim.attempt)
            .bind(staging_id)
            .bind(report_id)
            .bind(expected_report_sha256)
            .fetch_one(pool)
            .await?;
    match receipt.get("status").and_then(|value| value.as_str()) {
        Some("replayed") => Ok(PublishReceipt::Replayed { report_id }),
        Some("committed") => Ok(PublishReceipt::Committed { report_id }),
        _ => Err(protocol("matching commit receipt invalid")),
    }
}

async fn stage_report_payload_for_set(
    pool: &PgPool,
    _claimed: &ClaimedMatchingRequest,
    staging_id: Uuid,
    canonical_payload: &[u8],
    content_sha256: &str,
) -> Result<(), sqlx::Error> {
    if sha256_hex(canonical_payload) != content_sha256 {
        return Err(protocol("REPORT_HASH_MISMATCH"));
    }
    sqlx::query("SELECT kb_bid_matching_stage_report_payload($1,$2,$3)")
        .bind(staging_id)
        .bind(canonical_payload)
        .bind(content_sha256)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn retry_claim(
    pool: &PgPool,
    claimed: &ClaimedMatchingRequest,
    code: &str,
    detail: &str,
) -> Result<bool, sqlx::Error> {
    sqlx::query("SELECT kb_bid_matching_retry_claim($1,$2,$3,$4,$5)")
        .bind(claimed.job_id)
        .bind(claimed.claim.token)
        .bind(claimed.claim.attempt)
        .bind(code)
        .bind(bound_detail(detail))
        .execute(pool)
        .await?;
    Ok(true)
}

pub async fn fail_claim(
    pool: &PgPool,
    claimed: &ClaimedMatchingRequest,
    code: &str,
    detail: &str,
) -> Result<bool, sqlx::Error> {
    sqlx::query("SELECT kb_bid_matching_fail_claim($1,$2,$3,$4,$5)")
        .bind(claimed.job_id)
        .bind(claimed.claim.token)
        .bind(claimed.claim.attempt)
        .bind(code)
        .bind(bound_detail(detail))
        .execute(pool)
        .await?;
    Ok(true)
}

pub async fn reap_expired_claims(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let n: i32 = sqlx::query_scalar("SELECT kb_bid_matching_reap()")
        .fetch_one(pool)
        .await?;
    Ok(n as u64)
}

pub async fn pending_route_envelopes(
    pool: &PgPool,
) -> Result<Vec<PendingRouteEnvelope>, sqlx::Error> {
    let rows=sqlx::query("SELECT id,manifest_id FROM bid_matching_jobs WHERE status='pending' ORDER BY created_at,id")
      .fetch_all(pool).await?;
    Ok(rows
        .into_iter()
        .map(|row| PendingRouteEnvelope {
            job_id: row.get("id"),
            snapshots: snapshot_identity(row.get("manifest_id")),
        })
        .collect())
}

#[derive(Debug, Clone)]
pub struct PendingRouteEnvelope {
    pub job_id: Uuid,
    pub snapshots: EnvelopeSnapshotIdentity,
}

pub async fn current_routes(
    pool: &PgPool,
    project_id: Uuid,
) -> Result<Vec<sqlx::postgres::PgRow>, sqlx::Error> {
    sqlx::query(
        "SELECT route.id AS route_id,route.route_kind,route.unit_id
           FROM bid_matching_manifests manifest
           JOIN bid_matching_routes route ON route.manifest_id=manifest.id
           JOIN bid_projects project ON project.id=manifest.project_id
          WHERE manifest.project_id=$1 AND project.status='open'
            AND manifest.mutation_watermark=project.matching_mutation_watermark
            AND manifest.generation=(SELECT max(generation) FROM bid_matching_manifests WHERE project_id=$1)
          ORDER BY route.ordinal",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await
}

pub async fn current_route_jobs(
    pool: &PgPool,
    project_id: Uuid,
) -> Result<Vec<sqlx::postgres::PgRow>, sqlx::Error> {
    sqlx::query("SELECT j.id,r.route_kind,r.unit_id,j.status,j.error_detail,j.error_code AS terminal_error_code
      FROM bid_matching_jobs j JOIN bid_matching_routes r ON r.id=j.route_id
      JOIN bid_matching_manifests m ON m.id=j.manifest_id
      WHERE j.project_id=$1 AND m.generation=(SELECT max(generation) FROM bid_matching_manifests WHERE project_id=$1)
      ORDER BY r.ordinal").bind(project_id).fetch_all(pool).await
}

pub async fn current_technical_candidates(
    pool: &PgPool,
    project_id: Uuid,
    unit_id: Uuid,
) -> Result<Vec<sqlx::postgres::PgRow>, sqlx::Error> {
    sqlx::query("SELECT candidate.*,product.product_id,product.product_version_id,decision.system_decision,decision.quality_status
      FROM bid_current_matching_reports current_value JOIN bid_matching_reports report ON report.id=current_value.report_id
      JOIN bid_matching_routes route ON route.id=report.route_id
      JOIN bid_matching_candidate_artifacts candidate ON candidate.report_id=report.id AND candidate.support='supported'
      JOIN bid_matching_product_version_artifacts product ON product.id=candidate.product_version_artifact_id
      JOIN bid_matching_requirement_decisions decision ON decision.report_id=report.id AND decision.requirement_artifact_id=candidate.requirement_artifact_id
      JOIN bid_projects project ON project.id=current_value.project_id
      WHERE current_value.project_id=$1 AND route.route_kind='technical' AND route.unit_id=$2
        AND project.status='open' AND report.mutation_watermark=project.matching_mutation_watermark
      ORDER BY candidate.requirement_artifact_id,candidate.recommended DESC,candidate.route_product_ordinal,
        candidate.retrieval_rank,candidate.candidate_identity_sha256,candidate.evidence_v1_sha256")
      .bind(project_id).bind(unit_id).fetch_all(pool).await
}

pub async fn current_commercial_decisions(
    pool: &PgPool,
    project_id: Uuid,
) -> Result<Vec<sqlx::postgres::PgRow>, sqlx::Error> {
    sqlx::query("SELECT decision.*,requirement.clause_id AS source_clause_id,source.frozen_document_display_name AS file_name
      FROM bid_current_matching_reports current_value JOIN bid_matching_reports report ON report.id=current_value.report_id
      JOIN bid_matching_routes route ON route.id=report.route_id
      JOIN bid_matching_requirement_decisions decision ON decision.report_id=report.id
      JOIN bid_matching_requirement_artifacts requirement ON requirement.id=decision.requirement_artifact_id
      LEFT JOIN bid_matching_candidate_artifacts candidate ON candidate.id=decision.selected_candidate_artifact_id
      LEFT JOIN bid_matching_evidence_artifacts evidence ON evidence.candidate_artifact_id=candidate.id AND evidence.ordinal=0
      LEFT JOIN bid_matching_source_artifacts source ON source.id=evidence.source_chunk_artifact_id
      JOIN bid_projects project ON project.id=current_value.project_id
      WHERE current_value.project_id=$1 AND route.route_kind='commercial' AND project.status='open'
       AND report.mutation_watermark=project.matching_mutation_watermark ORDER BY decision.ordinal")
      .bind(project_id).fetch_all(pool).await
}

pub async fn current_commercial_projection(
    pool: &PgPool,
    project_id: Uuid,
) -> Result<Option<sqlx::postgres::PgRow>, sqlx::Error> {
    sqlx::query("SELECT report.* FROM bid_current_matching_reports current_value
      JOIN bid_matching_reports report ON report.id=current_value.report_id JOIN bid_matching_routes route ON route.id=report.route_id
      JOIN bid_projects project ON project.id=current_value.project_id
      WHERE current_value.project_id=$1 AND route.route_kind='commercial' AND project.status='open'
       AND report.mutation_watermark=project.matching_mutation_watermark")
      .bind(project_id).fetch_optional(pool).await
}

pub async fn visible_picks(
    pool: &PgPool,
    project_id: Uuid,
    unit_id: Option<Uuid>,
) -> Result<Vec<sqlx::postgres::PgRow>, sqlx::Error> {
    sqlx::query("SELECT item.*,artifact.route_id,artifact.revision,artifact.content_sha256
      FROM bid_current_route_pick_sets current_value JOIN bid_route_pick_set_artifacts artifact ON artifact.id=current_value.pick_set_id
      JOIN bid_route_pick_set_items item ON item.pick_set_id=artifact.id
      WHERE current_value.project_id=$1 AND ($2::uuid IS NULL OR item.unit_id=$2)
      ORDER BY artifact.route_id,item.ordinal").bind(project_id).bind(unit_id).fetch_all(pool).await
}

pub async fn current_route_report(
    pool: &PgPool,
    project_id: Uuid,
    route_id: Uuid,
) -> Result<Option<sqlx::postgres::PgRow>, sqlx::Error> {
    sqlx::query(
        "SELECT report.id AS report_id,report.content_sha256,report.generation,
                report.mutation_watermark,route.route_kind,route.unit_id,
                COALESCE(pick.revision,0) AS pick_revision
           FROM bidding_current_matching_reports report
           JOIN bid_matching_routes route ON route.id=report.route_id
           LEFT JOIN bid_current_route_pick_sets pick ON pick.project_id=report.project_id
             AND pick.route_id=report.route_id
          WHERE report.project_id=$1 AND report.route_id=$2",
    )
    .bind(project_id)
    .bind(route_id)
    .fetch_optional(pool)
    .await
}

pub async fn current_route_pick_items(
    pool: &PgPool,
    project_id: Uuid,
    route_id: Uuid,
) -> Result<Vec<sqlx::postgres::PgRow>, sqlx::Error> {
    sqlx::query(
        "SELECT item.* FROM bid_current_route_pick_sets current_value
         JOIN bid_route_pick_set_items item ON item.pick_set_id=current_value.pick_set_id
         WHERE current_value.project_id=$1 AND current_value.route_id=$2 ORDER BY item.ordinal",
    )
    .bind(project_id)
    .bind(route_id)
    .fetch_all(pool)
    .await
}

pub async fn matching_overview(
    pool: &PgPool,
    project_id: Uuid,
) -> Result<serde_json::Value, sqlx::Error> {
    let routes: serde_json::Value = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(to_jsonb(r) ORDER BY r.ordinal), '[]'::jsonb)
           FROM bidding_current_routes r WHERE r.project_id=$1",
    )
    .bind(project_id)
    .fetch_one(pool)
    .await?;
    let reports: serde_json::Value = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(jsonb_build_object(
            'id', r.id,
            'route_id', r.route_id,
            'generation', r.generation,
            'mutation_watermark', r.mutation_watermark,
            'quality_status', r.quality_status,
            'degraded', r.degraded,
            'reason_codes', to_jsonb(r.reason_codes),
            'coverage_total', r.coverage_total,
            'coverage_supported', r.coverage_supported,
            'coverage_unresolved', r.coverage_unresolved,
            'coverage_insufficient', r.coverage_insufficient,
            'coverage_contradicted', r.coverage_contradicted,
            'content_sha256', r.content_sha256,
            'empty_disposition', r.empty_disposition
          ) ORDER BY r.route_id), '[]'::jsonb)
           FROM bidding_current_matching_reports r WHERE r.project_id=$1",
    )
    .bind(project_id)
    .fetch_one(pool)
    .await?;
    let technical_candidates: serde_json::Value = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(to_jsonb(c) ORDER BY c.route_id,c.requirement_artifact_id,c.recommended DESC,c.route_product_ordinal,c.retrieval_rank), '[]'::jsonb)
           FROM bidding_current_technical_candidates c WHERE c.project_id=$1",
    )
    .bind(project_id)
    .fetch_one(pool)
    .await?;
    let commercial_decisions: serde_json::Value = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(to_jsonb(c) ORDER BY c.ordinal), '[]'::jsonb)
           FROM bidding_current_commercial_decisions c WHERE c.project_id=$1",
    )
    .bind(project_id)
    .fetch_one(pool)
    .await?;
    let route_pick_sets: serde_json::Value = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(jsonb_build_object(
            'route_id', p.route_id,
            'revision', p.revision,
            'source_report_artifact_id', p.source_report_artifact_id,
            'report_sha256', p.report_sha256,
            'report_generation', p.report_generation,
            'route_unit_id', p.route_unit_id,
            'content_sha256', p.content_sha256,
            'payload', convert_from(p.canonical_payload,'UTF8')::jsonb
          ) ORDER BY p.route_id), '[]'::jsonb)
           FROM bidding_current_route_pick_sets p WHERE p.project_id=$1",
    )
    .bind(project_id)
    .fetch_one(pool)
    .await?;
    let project_pick_set: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object(
            'id', p.id,
            'revision', p.revision,
            'content_sha256', p.content_sha256,
            'payload', convert_from(p.canonical_payload,'UTF8')::jsonb
          )
           FROM bidding_current_project_pick_sets p WHERE p.project_id=$1",
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await?;
    Ok(serde_json::json!({
        "routes": routes,
        "reports": reports,
        "technical_candidates": technical_candidates,
        "commercial_decisions": commercial_decisions,
        "route_pick_sets": route_pick_sets,
        "project_pick_set": project_pick_set
    }))
}

pub async fn matching_report_artifact_json(
    pool: &PgPool,
    project_id: Uuid,
    report_id: Uuid,
) -> Result<Option<serde_json::Value>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT jsonb_build_object(
            'id', report.id,
            'project_id', report.project_id,
            'route_id', report.route_id,
            'generation', report.generation,
            'mutation_watermark', report.mutation_watermark,
            'content_sha256', report.content_sha256,
            'published_at', report.published_at,
            'canonical_payload', convert_from(report.canonical_payload,'UTF8'),
            'payload', convert_from(report.canonical_payload,'UTF8')::jsonb
          )
           FROM bidding_matching_report_history report
          WHERE report.project_id=$1 AND report.id=$2",
    )
    .bind(project_id)
    .bind(report_id)
    .fetch_optional(pool)
    .await
}

pub async fn route_pick_set_json(
    pool: &PgPool,
    project_id: Uuid,
    route_id: Uuid,
) -> Result<serde_json::Value, sqlx::Error> {
    let route: Option<(String, Option<Uuid>)> = sqlx::query_as(
        "SELECT route_kind, unit_id FROM bidding_current_routes WHERE project_id=$1 AND route_id=$2",
    )
    .bind(project_id)
    .bind(route_id)
    .fetch_optional(pool)
    .await?;
    let Some((route_kind, unit_id)) = route else {
        return Ok(serde_json::json!({
            "route_id": route_id,
            "exists": false,
            "items": [],
            "supported_candidates": []
        }));
    };
    let report: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object(
            'id', r.id,
            'generation', r.generation,
            'content_sha256', r.content_sha256,
            'mutation_watermark', r.mutation_watermark,
            'quality_status', r.quality_status,
            'degraded', r.degraded,
            'reason_codes', to_jsonb(r.reason_codes)
          )
           FROM bidding_current_matching_reports r
          WHERE r.project_id=$1 AND r.route_id=$2",
    )
    .bind(project_id)
    .bind(route_id)
    .fetch_optional(pool)
    .await?;
    let pick: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object(
            'revision', p.revision,
            'source_report_artifact_id', p.source_report_artifact_id,
            'report_sha256', p.report_sha256,
            'report_generation', p.report_generation,
            'route_unit_id', p.route_unit_id,
            'content_sha256', p.content_sha256,
            'payload', convert_from(p.canonical_payload,'UTF8')::jsonb
          )
           FROM bidding_current_route_pick_sets p
          WHERE p.project_id=$1 AND p.route_id=$2",
    )
    .bind(project_id)
    .bind(route_id)
    .fetch_optional(pool)
    .await?;
    let supported: serde_json::Value = sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(to_jsonb(c) ORDER BY c.requirement_artifact_id,c.recommended DESC,c.route_product_ordinal,c.retrieval_rank), '[]'::jsonb)
           FROM bidding_current_technical_candidates c
          WHERE c.project_id=$1 AND c.route_id=$2",
    )
    .bind(project_id)
    .bind(route_id)
    .fetch_one(pool)
    .await?;
    let items = pick
        .as_ref()
        .and_then(|value| value.get("payload"))
        .and_then(|value| value.get("items"))
        .cloned()
        .unwrap_or_else(|| serde_json::json!([]));
    let revision = pick
        .as_ref()
        .and_then(|value| value.get("revision"))
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0);
    Ok(serde_json::json!({
        "exists": true,
        "route_id": route_id,
        "route_kind": route_kind,
        "unit_id": unit_id,
        "source_report_artifact_id": report.as_ref().and_then(|value| value.get("id")).cloned(),
        "report_sha256": report.as_ref().and_then(|value| value.get("content_sha256")).cloned(),
        "report_generation": report.as_ref().and_then(|value| value.get("generation")).cloned(),
        "matching_mutation_watermark": report.as_ref().and_then(|value| value.get("mutation_watermark")).cloned(),
        "quality_status": report.as_ref().and_then(|value| value.get("quality_status")).cloned(),
        "degraded": report.as_ref().and_then(|value| value.get("degraded")).cloned(),
        "reason_codes": report.as_ref().and_then(|value| value.get("reason_codes")).cloned(),
        "revision": revision,
        "items": items,
        "supported_candidates": supported,
        "pick_set": pick
    }))
}

pub async fn current_route_supported_candidates(
    pool: &PgPool,
    project_id: Uuid,
    route_id: Uuid,
) -> Result<Vec<sqlx::postgres::PgRow>, sqlx::Error> {
    sqlx::query(
        "SELECT candidate.id AS candidate_artifact_id,candidate.requirement_artifact_id,
                product.product_id,product.product_version_id,candidate.recommended,
                candidate.route_product_ordinal,candidate.retrieval_rank,
                to_char(candidate.retrieval_raw_score,'FM99999999999999999990.000000') AS retrieval_raw_score,
                candidate.evidence_v1_sha256
           FROM bidding_current_matching_reports report
           JOIN bid_matching_candidate_artifacts candidate
             ON candidate.report_id=report.id AND candidate.support='supported'
           JOIN bid_matching_product_version_artifacts product
             ON product.id=candidate.product_version_artifact_id
          WHERE report.project_id=$1 AND report.route_id=$2
          ORDER BY candidate.requirement_artifact_id,candidate.recommended DESC,
            candidate.route_product_ordinal,candidate.retrieval_rank,
            candidate.candidate_identity_sha256,candidate.evidence_v1_sha256",
    )
    .bind(project_id)
    .bind(route_id)
    .fetch_all(pool)
    .await
}

pub async fn replace_route_pick_set(
    pool: &PgPool,
    input: ReplaceRoutePickSetV1,
    context: &crate::bidding::MutationContext,
) -> Result<PickSetReceiptV1, sqlx::Error> {
    let selections =
        serde_json::to_value(&input.selections).map_err(|error| protocol(error.to_string()))?;
    let value: serde_json::Value = sqlx::query_scalar(
        "SELECT kb_bid_matching_replace_route_picks($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
    )
    .bind(input.project_id)
    .bind(input.route_id)
    .bind(input.source_report_artifact_id)
    .bind(&input.report_sha256)
    .bind(input.expected_revision)
    .bind(selections)
    .bind(&context.actor)
    .bind(&context.idempotency_key)
    .bind(&context.request.bytes)
    .bind(&context.request.sha256)
    .fetch_one(pool)
    .await?;
    Ok(PickSetReceiptV1 {
        route_pick_set_id: parse_uuid(&value, "route_pick_set_id")?,
        route_revision: value
            .get("route_revision")
            .and_then(|v| v.as_i64())
            .unwrap_or(0),
        route_sha256: value
            .get("route_sha256")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        project_pick_set_id: parse_uuid(&value, "project_pick_set_id")?,
        project_revision: value
            .get("project_revision")
            .and_then(|v| v.as_i64())
            .unwrap_or(0),
        project_sha256: value
            .get("project_sha256")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
    })
}

fn parse_uuid(value: &serde_json::Value, key: &str) -> Result<Uuid, sqlx::Error> {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .and_then(|v| Uuid::parse_str(v).ok())
        .ok_or_else(|| protocol(format!("matching receipt missing {key}")))
}

async fn rebuild_project_pick_set(
    tx: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    actor: &str,
) -> Result<(Uuid, i64, String), sqlx::Error> {
    let revision: i64=sqlx::query_scalar("SELECT COALESCE(max(revision),0)+1 FROM bid_project_pick_set_artifacts WHERE project_id=$1")
      .bind(project_id).fetch_one(&mut **tx).await?;
    let rows=sqlx::query("SELECT current_value.pick_set_id AS route_pick_set_id,artifact.source_report_artifact_id,
      item.requirement_artifact_id,item.candidate_artifact_id,item.product_id,item.product_version_id,item.unit_id,
      artifact.route_id FROM bid_current_route_pick_sets current_value
      JOIN bid_route_pick_set_artifacts artifact ON artifact.id=current_value.pick_set_id
      JOIN bid_route_pick_set_items item ON item.pick_set_id=artifact.id WHERE current_value.project_id=$1
      ORDER BY artifact.route_id,item.requirement_artifact_id,item.candidate_artifact_id")
      .bind(project_id).fetch_all(&mut **tx).await?;
    verify_mixed_pick_rows(tx, project_id, &rows).await?;
    let id = Uuid::new_v4();
    let items:Vec<serde_json::Value>=rows.iter().map(|row|serde_json::json!({"route_pick_set_id":row.get::<Uuid,_>("route_pick_set_id"),
      "source_report_artifact_id":row.get::<Uuid,_>("source_report_artifact_id"),"requirement_artifact_id":row.get::<Uuid,_>("requirement_artifact_id"),
      "candidate_artifact_id":row.get::<Uuid,_>("candidate_artifact_id"),"product_id":row.get::<Option<Uuid>,_>("product_id"),
      "product_version_id":row.get::<Uuid,_>("product_version_id"),"unit_id":row.get::<Option<Uuid>,_>("unit_id")})).collect();
    let payload = serde_json::json!({"schema_version":1,"project_id":project_id,"revision":revision,"items":items});
    let bytes = serde_json::to_vec(&payload).unwrap();
    let digest = sha256_hex(&bytes);
    sqlx::query("INSERT INTO bid_project_pick_set_artifacts(id,project_id,revision,canonical_payload,content_sha256,created_by,created_at)
      VALUES($1,$2,$3,$4,$5,$6,clock_timestamp())")
      .bind(id).bind(project_id).bind(revision).bind(&bytes).bind(&digest).bind(actor).execute(&mut **tx).await?;
    for (ordinal, row) in rows.iter().enumerate() {
        sqlx::query("INSERT INTO bid_project_pick_set_items(project_pick_set_id,ordinal,route_pick_set_id,
      source_report_artifact_id,requirement_artifact_id,candidate_artifact_id,product_id,product_version_id,unit_id)
      VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)")
      .bind(id).bind(ordinal as i32).bind(row.get::<Uuid,_>("route_pick_set_id")).bind(row.get::<Uuid,_>("source_report_artifact_id"))
      .bind(row.get::<Uuid,_>("requirement_artifact_id")).bind(row.get::<Uuid,_>("candidate_artifact_id"))
      .bind(row.get::<Option<Uuid>,_>("product_id")).bind(row.get::<Uuid,_>("product_version_id"))
      .bind(row.get::<Option<Uuid>,_>("unit_id")).execute(&mut **tx).await?;
    }
    sqlx::query("INSERT INTO bid_current_project_pick_sets(project_id,pick_set_id,revision) VALUES($1,$2,$3)
      ON CONFLICT(project_id) DO UPDATE SET pick_set_id=EXCLUDED.pick_set_id,revision=EXCLUDED.revision")
      .bind(project_id).bind(id).bind(revision).execute(&mut **tx).await?;
    Ok((id, revision, digest))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct PickUnionIdentity {
    source_report_artifact_id: Uuid,
    requirement_artifact_id: Uuid,
    candidate_artifact_id: Uuid,
    unit_id: Uuid,
}

fn verify_project_pick_union(
    unsectioned_report_id: Option<Uuid>,
    project_items: &BTreeSet<PickUnionIdentity>,
    unsectioned_route_items: &BTreeSet<(Uuid, Uuid)>,
) -> Result<(), sqlx::Error> {
    let mut subset = BTreeSet::new();
    for item in project_items {
        if Some(item.source_report_artifact_id) == unsectioned_report_id {
            if !item.unit_id.is_nil() {
                return Err(protocol("UNSECTIONED_PICK_SUBSET_UNIT_MISMATCH"));
            }
            subset.insert((item.requirement_artifact_id, item.candidate_artifact_id));
        } else if item.unit_id.is_nil() {
            return Err(protocol("ORDINARY_PICK_HAS_NIL_UNIT"));
        }
    }
    if unsectioned_report_id.is_some() && subset != *unsectioned_route_items {
        return Err(protocol("PROJECT_PICK_UNSECTIONED_SUBSET_MISMATCH"));
    }
    Ok(())
}

async fn verify_mixed_pick_rows(
    tx: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    rows: &[sqlx::postgres::PgRow],
) -> Result<(), sqlx::Error> {
    let unsectioned:Option<Uuid>=sqlx::query_scalar("SELECT report.id FROM bid_current_matching_reports current_value
      JOIN bid_matching_reports report ON report.id=current_value.report_id JOIN bid_matching_routes route ON route.id=report.route_id
      WHERE current_value.project_id=$1 AND route.route_kind='technical' AND route.unit_id='00000000-0000-0000-0000-000000000000'::uuid")
      .bind(project_id).fetch_optional(&mut **tx).await?;
    let project_items = rows
        .iter()
        .map(|row| {
            Ok(PickUnionIdentity {
                source_report_artifact_id: row.get("source_report_artifact_id"),
                requirement_artifact_id: row.get("requirement_artifact_id"),
                candidate_artifact_id: row.get("candidate_artifact_id"),
                unit_id: row
                    .get::<Option<Uuid>, _>("unit_id")
                    .ok_or_else(|| protocol("TECHNICAL_PICK_UNIT_MISSING"))?,
            })
        })
        .collect::<Result<BTreeSet<_>, sqlx::Error>>()?;
    let route_items = if let Some(report_id) = unsectioned {
        sqlx::query(
            "SELECT item.requirement_artifact_id,item.candidate_artifact_id
          FROM bid_current_route_pick_sets current_value
          JOIN bid_route_pick_set_artifacts artifact ON artifact.id=current_value.pick_set_id
          JOIN bid_route_pick_set_items item ON item.pick_set_id=artifact.id
          WHERE current_value.project_id=$1 AND artifact.source_report_artifact_id=$2",
        )
        .bind(project_id)
        .bind(report_id)
        .fetch_all(&mut **tx)
        .await?
        .into_iter()
        .map(|row| {
            (
                row.get("requirement_artifact_id"),
                row.get("candidate_artifact_id"),
            )
        })
        .collect()
    } else {
        BTreeSet::new()
    };
    verify_project_pick_union(unsectioned, &project_items, &route_items)
}

fn snapshot_identity(manifest_id: Uuid) -> EnvelopeSnapshotIdentity {
    EnvelopeSnapshotIdentity {
        config_snapshot_id: deterministic_uuid("config", manifest_id.as_bytes()),
        feature_snapshot_id: deterministic_uuid("feature", manifest_id.as_bytes()),
        score_policy_snapshot_id: deterministic_uuid("score", manifest_id.as_bytes()),
        verifier_policy_snapshot_id: deterministic_uuid("verifier", manifest_id.as_bytes()),
    }
}
fn deterministic_uuid(tag: &str, identity: &[u8]) -> Uuid {
    let mut h = Sha256::new();
    h.update(tag);
    h.update([0]);
    h.update(identity);
    let mut b: [u8; 16] = h.finalize()[..16].try_into().unwrap();
    b[6] = (b[6] & 15) | 80;
    b[8] = (b[8] & 63) | 128;
    Uuid::from_bytes(b)
}
fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let b0 = bytes[i];
        let b1 = if i + 1 < bytes.len() { bytes[i + 1] } else { 0 };
        let b2 = if i + 2 < bytes.len() { bytes[i + 2] } else { 0 };
        let n = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        if i + 1 < bytes.len() {
            out.push(TABLE[((n >> 6) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        if i + 2 < bytes.len() {
            out.push(TABLE[(n & 63) as usize] as char);
        } else {
            out.push('=');
        }
        i += 3;
    }
    out
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
#[cfg(test)]
fn parse_collection<T: for<'de> Deserialize<'de>>(
    collections: &HashMap<String, Vec<Vec<u8>>>,
    kind: &str,
) -> Result<Vec<T>, sqlx::Error> {
    let mut values = Vec::new();
    for batch in collections
        .get(kind)
        .ok_or_else(|| protocol("staged collection missing"))?
    {
        let mut decoded: Vec<T> =
            serde_json::from_slice(batch).map_err(|error| protocol(error.to_string()))?;
        values.append(&mut decoded);
    }
    Ok(values)
}

fn bound_detail(value: &str) -> String {
    value.chars().take(1024).collect()
}
fn protocol(message: impl Into<String>) -> sqlx::Error {
    sqlx::Error::Protocol(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staging_splits_large_collections_with_contiguous_ordinals() {
        let payload = "证据".repeat(40_000);
        let items = (0..12)
            .map(|index| serde_json::json!({"ordinal":index,"payload":payload}))
            .collect::<Vec<_>>();
        let mut batches = Vec::new();
        split_collection("candidates", &items, &mut batches).unwrap();
        assert!(batches.len() > 1);
        for (ordinal, batch) in batches.iter().enumerate() {
            assert_eq!(batch.ordinal, ordinal as i32);
            assert_eq!(batch.kind, "candidates");
            assert!(batch.bytes.len() <= MAX_BATCH_BYTES);
            assert!(batch.item_count <= MAX_BATCH_ITEMS);
        }
        let mut collections = HashMap::new();
        collections.insert(
            "candidates".to_string(),
            batches.into_iter().map(|batch| batch.bytes).collect(),
        );
        let replayed: Vec<serde_json::Value> =
            parse_collection(&collections, "candidates").unwrap();
        assert_eq!(replayed, items);
    }

    #[test]
    fn candidate_group_stage_items_use_the_sql_base64_field() {
        let canonical_payload = br#"{"schema_version":1}"#.to_vec();
        let group = StagedCandidateGroupV1 {
            id: Uuid::from_u128(1),
            requirement_artifact_id: Uuid::from_u128(2),
            support: "supported".into(),
            ordinal: 0,
            content_sha256: sha256_hex(&canonical_payload),
            canonical_payload: canonical_payload.clone(),
        };
        let mut batches = Vec::new();

        split_collection("candidate_groups", &[group], &mut batches).unwrap();

        let items = stage_batch_items(batches[0].kind, &batches[0].bytes).unwrap();
        let expected = base64_encode(&canonical_payload);
        assert_eq!(
            items[0]
                .get("canonical_payload_b64")
                .and_then(serde_json::Value::as_str),
            Some(expected.as_str())
        );
        assert!(items[0].get("canonical_payload").is_none());
    }

    #[test]
    fn mixed_ordinary_and_unsectioned_project_pick_union_is_valid() {
        let unsectioned_report = Uuid::from_u128(1);
        let ordinary_report = Uuid::from_u128(2);
        let unsectioned_requirement = Uuid::from_u128(3);
        let unsectioned_candidate = Uuid::from_u128(4);
        let ordinary_requirement = Uuid::from_u128(5);
        let ordinary_candidate = Uuid::from_u128(6);
        let ordinary_unit = Uuid::from_u128(7);
        let items = BTreeSet::from([
            PickUnionIdentity {
                source_report_artifact_id: unsectioned_report,
                requirement_artifact_id: unsectioned_requirement,
                candidate_artifact_id: unsectioned_candidate,
                unit_id: Uuid::nil(),
            },
            PickUnionIdentity {
                source_report_artifact_id: ordinary_report,
                requirement_artifact_id: ordinary_requirement,
                candidate_artifact_id: ordinary_candidate,
                unit_id: ordinary_unit,
            },
        ]);
        let route_items = BTreeSet::from([(unsectioned_requirement, unsectioned_candidate)]);
        verify_project_pick_union(Some(unsectioned_report), &items, &route_items).unwrap();

        let wrong_subset = BTreeSet::new();
        assert!(
            verify_project_pick_union(Some(unsectioned_report), &items, &wrong_subset).is_err()
        );
        let mut wrong_nil = items;
        wrong_nil.replace(PickUnionIdentity {
            source_report_artifact_id: ordinary_report,
            requirement_artifact_id: ordinary_requirement,
            candidate_artifact_id: ordinary_candidate,
            unit_id: Uuid::nil(),
        });
        assert!(
            verify_project_pick_union(Some(unsectioned_report), &wrong_nil, &route_items).is_err()
        );
    }

    #[test]
    fn completed_publish_replay_requires_the_same_report_and_hash() {
        let report_id = Uuid::from_u128(1);
        let digest = "a".repeat(64);
        assert_eq!(
            completed_publish_receipt(Some(report_id), Some(&digest), report_id, &digest).unwrap(),
            PublishReceipt::Replayed { report_id }
        );
        assert!(
            completed_publish_receipt(Some(report_id), Some(&"b".repeat(64)), report_id, &digest)
                .is_err()
        );
        assert_eq!(
            completed_publish_receipt(Some(Uuid::from_u128(2)), Some(&digest), report_id, &digest)
                .unwrap(),
            PublishReceipt::Stale
        );
    }
}
