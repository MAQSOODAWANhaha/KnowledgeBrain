//! Final V1 storage adapter for `MatchingPublication`.
//!
//! The application interface is intentionally small: schedule, claim/load,
//! heartbeat, publish, and replace a route pick set.  The large-artifact
//! Open/Stage/Commit protocol is private to this implementation.

use domain::knowledge_retrieval::{
    CompanyEvidenceRequestV1, KNOWLEDGE_EVIDENCE_SCHEMA_V1, KnowledgeRetrievalPort,
    ProductEvidenceRequestV1, RetrievalPolicyIdentityV1,
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
const MAX_ACTIVE_STAGING_PER_PROJECT: i64 = 8;
const MAX_STAGING_ROWS_PER_PROJECT: i64 = 100_000;
const MAX_STAGING_BYTES_PER_PROJECT: i64 = 64 * 1024 * 1024;
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
    pub content_sha256: String,
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
) -> Result<Option<ScheduleReceipt>, sqlx::Error> {
    let port = crate::knowledge_retrieval::PostgresKnowledgeRetrievalAdapter::new(pool.clone());
    schedule_dirty_project_with_port(pool, project_id, environment, &port).await
}

async fn schedule_dirty_project_with_port<P: KnowledgeRetrievalPort>(
    pool: &PgPool,
    project_id: Uuid,
    environment: ScheduleEnvironment,
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
        sqlx::query("SELECT status,matching_mutation_watermark FROM bid_projects WHERE id=$1")
            .bind(project_id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| protocol("bid project does not exist"))?;
    if project.get::<String, _>("status") != "open" {
        return Err(protocol("bid project is not open"));
    }
    let watermark: i64 = project.get("matching_mutation_watermark");
    let already_scheduled: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM bid_matching_manifests WHERE project_id=$1 AND mutation_watermark=$2)",
    )
    .bind(project_id)
    .bind(watermark)
    .fetch_one(pool)
    .await?;
    if already_scheduled {
        return Ok(None);
    }

    let clause_rows = sqlx::query(
        "SELECT c.id,c.text,c.family,
                CASE WHEN c.family='technical' THEN COALESCE(span.section_artifact_id,
                    '00000000-0000-0000-0000-000000000000'::uuid) ELSE NULL END AS unit_id
           FROM bid_clauses c
           LEFT JOIN bid_source_span_artifacts span ON span.id=c.current_source_span_artifact_id
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
        let hits = match route_lookup[&requirement.route_id].route {
            MatchRoute::Technical { .. } => port
                .retrieve_product_evidence(ProductEvidenceRequestV1 {
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
            MatchRoute::Commercial => port
                .retrieve_company_evidence(CompanyEvidenceRequestV1 {
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
        };
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

    let mut tx = pool.begin().await?;
    let current = sqlx::query(
        "SELECT status,matching_mutation_watermark FROM bid_projects WHERE id=$1 FOR UPDATE",
    )
    .bind(project_id)
    .fetch_one(&mut *tx)
    .await?;
    if current.get::<String, _>("status") != "open"
        || current.get::<i64, _>("matching_mutation_watermark") != watermark
    {
        tx.rollback().await?;
        return Err(protocol("matching schedule fence lost"));
    }
    let duplicate: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM bid_matching_manifests WHERE project_id=$1 AND mutation_watermark=$2)",
    )
    .bind(project_id)
    .bind(watermark)
    .fetch_one(&mut *tx)
    .await?;
    if duplicate {
        tx.rollback().await?;
        return Ok(None);
    }
    let generation: i64 = sqlx::query_scalar(
        "SELECT COALESCE(max(generation),0)+1 FROM bid_matching_manifests WHERE project_id=$1",
    )
    .bind(project_id)
    .fetch_one(&mut *tx)
    .await?;
    let manifest_payload = serde_json::json!({
        "schema_version": 1,
        "manifest_id": manifest_id,
        "project_id": project_id,
        "generation": generation,
        "mutation_watermark": watermark,
        "requirement_set_sha256": requirement_set_sha256,
        "eligible_scope_sha256": eligible_scope_sha256,
    });
    let manifest_bytes =
        serde_json::to_vec(&manifest_payload).expect("manifest payload serializes");
    sqlx::query(
        "INSERT INTO bid_matching_manifests
         (id,project_id,generation,mutation_watermark,requirement_set_sha256,eligible_scope_sha256,
          canonical_payload,content_sha256) VALUES($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(manifest_id)
    .bind(project_id)
    .bind(generation)
    .bind(watermark)
    .bind(&requirement_set_sha256)
    .bind(&eligible_scope_sha256)
    .bind(&manifest_bytes)
    .bind(sha256_hex(&manifest_bytes))
    .execute(&mut *tx)
    .await?;
    for route in &routes {
        let (kind, unit_id) = route.route.parts();
        sqlx::query(
            "INSERT INTO bid_matching_routes
             (id,manifest_id,project_id,route_kind,unit_id,ordinal,empty_policy,route_scope_sha256)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8)",
        )
        .bind(route.id)
        .bind(manifest_id)
        .bind(project_id)
        .bind(kind)
        .bind(unit_id)
        .bind(route.ordinal as i32)
        .bind(&route.empty_policy)
        .bind(&route.scope_sha256)
        .execute(&mut *tx)
        .await?;
    }
    for requirement in &requirements {
        sqlx::query(
            "INSERT INTO bid_matching_requirement_artifacts
             (id,manifest_id,route_id,clause_id,ordinal,requirement_text,requirement_sha256)
             VALUES($1,$2,$3,$4,$5,$6,$7)",
        )
        .bind(requirement.id)
        .bind(manifest_id)
        .bind(requirement.route_id)
        .bind(requirement.clause_id)
        .bind(requirement.ordinal as i32)
        .bind(&requirement.text)
        .bind(&requirement.sha256)
        .execute(&mut *tx)
        .await?;
    }
    for product in &products {
        sqlx::query(
            "INSERT INTO bid_matching_product_version_artifacts
             (id,manifest_id,product_id,product_version_id,workspace_kind,frozen_display_name,identity_sha256)
             VALUES($1,$2,$3,$4,$5,$6,$7)",
        )
        .bind(product.id)
        .bind(manifest_id)
        .bind(product.product_id)
        .bind(product.product_version_id)
        .bind(&product.workspace_kind)
        .bind(&product.frozen_display_name)
        .bind(&product.identity_sha256)
        .execute(&mut *tx)
        .await?;
    }
    for route in &routes {
        let mut members: BTreeSet<(Uuid, Uuid)> = BTreeSet::new();
        for hit in frozen_hits.iter().filter(|hit| hit.route_id == route.id) {
            members.insert((hit.hit.product_version_id, hit.product_version_artifact_id));
        }
        for (ordinal, (_, artifact_id)) in members.into_iter().enumerate() {
            sqlx::query(
                "INSERT INTO bid_matching_route_memberships
                 (route_id,product_version_artifact_id,route_product_ordinal) VALUES($1,$2,$3)",
            )
            .bind(route.id)
            .bind(artifact_id)
            .bind(ordinal as i32)
            .execute(&mut *tx)
            .await?;
        }
    }
    for frozen in &frozen_hits {
        sqlx::query(
            "INSERT INTO bid_matching_frozen_retrieved_hits
             (id,manifest_id,route_id,requirement_artifact_id,product_version_artifact_id,
              document_id,source_chunk_id,frozen_document_display_name,chunk_utf8,chunk_sha256,
              chunk_byte_length,retrieval_rank,retrieval_raw_score,quote_start_offset,quote_end_offset,
              offset_unit,retrieval_contract_version)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,convert_to($9,'UTF8'),$10,$11,$12,$13,$14,$15,$16,$17)",
        )
        .bind(frozen.id)
        .bind(manifest_id)
        .bind(frozen.route_id)
        .bind(frozen.requirement_artifact_id)
        .bind(frozen.product_version_artifact_id)
        .bind(frozen.hit.document_id)
        .bind(frozen.hit.source_chunk_id)
        .bind(&frozen.hit.frozen_document_display_name)
        .bind(&frozen.hit.chunk_utf8)
        .bind(&frozen.hit.chunk_sha256)
        .bind(frozen.hit.chunk_byte_length as i64)
        .bind(frozen.hit.retrieval_rank as i32)
        .bind(frozen.hit.retrieval_raw_score.parse::<Decimal>().map_err(|_| protocol("invalid retrieval score"))?)
        .bind(frozen.hit.quote_start_offset as i64)
        .bind(frozen.hit.quote_end_offset as i64)
        .bind(&frozen.hit.offset_unit)
        .bind(&frozen.hit.retrieval_contract_version)
        .execute(&mut *tx)
        .await?;
    }
    let snapshots = snapshot_identity(manifest_id);
    let mut jobs = Vec::new();
    for route in &routes {
        let job_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO bid_matching_jobs
             (id,project_id,manifest_id,route_id,status,max_attempts,claim_lease_ms,
              lease_policy_generation,created_at)
             VALUES($1,$2,$3,$4,'pending',$5,$6,$7,clock_timestamp())",
        )
        .bind(job_id)
        .bind(project_id)
        .bind(manifest_id)
        .bind(route.id)
        .bind(environment.max_attempts)
        .bind(CLAIM_LEASE_MS)
        .bind(LEASE_POLICY_GENERATION)
        .execute(&mut *tx)
        .await?;
        jobs.push(ScheduledJob {
            id: job_id,
            snapshots,
        });
    }
    tx.commit().await?;
    Ok(Some(ScheduleReceipt { manifest_id, jobs }))
}

pub async fn claim_and_load(
    pool: &PgPool,
    job_id: Uuid,
    snapshots: EnvelopeSnapshotIdentity,
) -> Result<Option<ClaimedMatchingRequest>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let Some(job) = sqlx::query(
        "SELECT j.*,m.generation,m.mutation_watermark,r.route_kind,r.unit_id,r.empty_policy
           FROM bid_matching_jobs j
           JOIN bid_matching_manifests m ON m.id=j.manifest_id
           JOIN bid_matching_routes r ON r.id=j.route_id
           JOIN bid_projects p ON p.id=j.project_id
          WHERE j.id=$1 AND j.status='pending' AND p.status='open'
            AND p.matching_mutation_watermark=m.mutation_watermark
            AND m.generation=(SELECT max(m2.generation) FROM bid_matching_manifests m2 WHERE m2.project_id=m.project_id)
          FOR UPDATE OF j",
    )
    .bind(job_id)
    .fetch_optional(&mut *tx)
    .await?
    else {
        tx.rollback().await?;
        return Ok(None);
    };
    if snapshots != snapshot_identity(job.get("manifest_id")) {
        tx.rollback().await?;
        return Err(protocol("matching envelope snapshot mismatch"));
    }
    let attempt: i32 = sqlx::query_scalar(
        "SELECT COALESCE(max(attempt),0)+1 FROM bid_matching_job_claims WHERE job_id=$1",
    )
    .bind(job_id)
    .fetch_one(&mut *tx)
    .await?;
    if attempt > job.get::<i32, _>("max_attempts") {
        sqlx::query("UPDATE bid_matching_jobs SET status='failed',error_code='ATTEMPTS_EXHAUSTED',finished_at=clock_timestamp() WHERE id=$1")
            .bind(job_id).execute(&mut *tx).await?;
        tx.commit().await?;
        return Ok(None);
    }
    let token = Uuid::new_v4();
    let lease_ms: i32 = job.get("claim_lease_ms");
    let lease_generation: i64 = job.get("lease_policy_generation");
    sqlx::query(
        "INSERT INTO bid_matching_job_claims
         (job_id,attempt,claim_token,claim_lease_ms,lease_policy_generation,claimed_at,heartbeat_at,status)
         VALUES($1,$2,$3,$4,$5,clock_timestamp(),clock_timestamp(),'running')",
    )
    .bind(job_id)
    .bind(attempt)
    .bind(token)
    .bind(lease_ms)
    .bind(lease_generation)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE bid_matching_jobs SET status='running',active_attempt=$2,started_at=COALESCE(started_at,clock_timestamp()) WHERE id=$1")
        .bind(job_id).bind(attempt).execute(&mut *tx).await?;
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
    let mut tx = pool.begin().await?;
    let result = sqlx::query(
        "UPDATE bid_matching_job_claims claim SET heartbeat_at=clock_timestamp()
          FROM bid_matching_jobs job
         WHERE claim.job_id=$1 AND claim.attempt=$2 AND claim.claim_token=$3
           AND claim.status='running' AND job.id=claim.job_id AND job.status='running'
           AND job.active_attempt=claim.attempt
           AND claim.heartbeat_at + make_interval(secs => claim.claim_lease_ms::double precision/1000.0) > clock_timestamp()",
    )
    .bind(request.job_id)
    .bind(request.claim.attempt)
    .bind(request.claim.token)
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() == 1 {
        sqlx::query(
            "UPDATE bid_matching_staging_sets SET expires_at=clock_timestamp()+make_interval(secs=>$4::double precision/1000.0)
             WHERE job_id=$1 AND attempt=$2 AND claim_token=$3 AND state='active'",
        )
        .bind(request.job_id)
        .bind(request.claim.attempt)
        .bind(request.claim.token)
        .bind(STAGING_TTL_MS)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(true)
    } else {
        tx.rollback().await?;
        Ok(false)
    }
}

pub async fn publish_route(
    pool: &PgPool,
    claimed: &ClaimedMatchingRequest,
    report: PublishRouteV2,
) -> Result<PublishReceipt, sqlx::Error> {
    if let Some((status, completed_report_id)) = sqlx::query_as::<_, (String, Option<Uuid>)>(
        "SELECT status,completed_report_id FROM bid_matching_jobs WHERE id=$1",
    )
    .bind(claimed.job_id)
    .fetch_optional(pool)
    .await?
        && status == "completed"
    {
        return Ok(if completed_report_id == Some(report.report_id) {
            PublishReceipt::Replayed {
                report_id: report.report_id,
            }
        } else {
            PublishReceipt::Stale
        });
    }
    let batches = stage_collections(&report)?;
    let staging_id = open_staging_set(pool, claimed, &report, &batches).await?;
    stage_report_payload_for_set(
        pool,
        claimed,
        staging_id,
        &report.canonical_payload,
        &report.content_sha256,
    )
    .await?;
    for batch in batches {
        stage_batch(pool, claimed, staging_id, batch).await?;
    }
    commit_staged_route(
        pool,
        claimed,
        staging_id,
        report.report_id,
        &report.content_sha256,
    )
    .await
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
    let mut tx = pool.begin().await?;
    lock_live_claim(&mut tx, claimed).await?;
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
            "OpenStagingSetV1:{}:{}:{}:{}:{}:{}:{}",
            claimed.job_id,
            claimed.claim.attempt,
            claimed.route_id,
            report.report_nonce,
            batches.len(),
            expected_item_count,
            expected_byte_length
        )
        .as_bytes(),
    );
    if let Some(row) = sqlx::query(
        "SELECT id,report_nonce,open_payload_sha256,state FROM bid_matching_staging_sets
         WHERE job_id=$1 AND claim_token=$2 AND attempt=$3 AND route_id=$4 FOR UPDATE",
    )
    .bind(claimed.job_id)
    .bind(claimed.claim.token)
    .bind(claimed.claim.attempt)
    .bind(claimed.route_id)
    .fetch_optional(&mut *tx)
    .await?
    {
        if row.get::<Uuid, _>("report_nonce") != report.report_nonce
            || row.get::<String, _>("open_payload_sha256") != open_hash
        {
            return Err(protocol("OPEN_STAGING_PAYLOAD_MISMATCH"));
        }
        let state: String = row.get("state");
        if state == "expired" || state == "failed" {
            return Err(protocol("staging set is terminal"));
        }
        let id = row.get("id");
        tx.commit().await?;
        return Ok(id);
    }
    let active: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM bid_matching_staging_sets WHERE project_id=$1 AND state='active'",
    )
    .bind(claimed.project_id)
    .fetch_one(&mut *tx)
    .await?;
    if active >= MAX_ACTIVE_STAGING_PER_PROJECT {
        return Err(protocol("STAGING_ACTIVE_SET_QUOTA_EXCEEDED"));
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO bid_matching_staging_sets
         (id,job_id,route_id,claim_token,attempt,manifest_id,project_id,generation,mutation_watermark,
          report_nonce,state,expires_at,open_payload_sha256,expected_batch_count,expected_item_count,
          expected_byte_length,staged_item_count,staged_byte_length)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,'active',
          clock_timestamp()+make_interval(secs=>$11::double precision/1000.0),$12,$13,$14,$15,0,0)",
    )
    .bind(id)
    .bind(claimed.job_id)
    .bind(claimed.route_id)
    .bind(claimed.claim.token)
    .bind(claimed.claim.attempt)
    .bind(claimed.manifest_id)
    .bind(claimed.project_id)
    .bind(claimed.generation)
    .bind(claimed.mutation_watermark)
    .bind(report.report_nonce)
    .bind(STAGING_TTL_MS)
    .bind(open_hash)
    .bind(batches.len() as i32)
    .bind(expected_item_count)
    .bind(expected_byte_length)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(id)
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
    let mut tx = pool.begin().await?;
    lock_live_claim(&mut tx, claimed).await?;
    let set = sqlx::query(
        "SELECT * FROM bid_matching_staging_sets WHERE id=$1 AND state='active' AND expires_at>clock_timestamp() FOR UPDATE",
    )
    .bind(staging_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| protocol("staging set is not active"))?;
    if set.get::<Uuid, _>("job_id") != claimed.job_id
        || set.get::<Uuid, _>("claim_token") != claimed.claim.token
        || set.get::<i32, _>("attempt") != claimed.claim.attempt
    {
        return Err(protocol("staging claim mismatch"));
    }
    let payload_sha256 = sha256_hex(&batch.bytes);
    if let Some(existing) = sqlx::query(
        "SELECT payload_sha256,canonical_items FROM bid_matching_staged_batches
         WHERE staging_set_id=$1 AND batch_ordinal=$2",
    )
    .bind(staging_id)
    .bind(batch.ordinal)
    .fetch_optional(&mut *tx)
    .await?
    {
        if existing.get::<String, _>("payload_sha256") != payload_sha256
            || existing.get::<Vec<u8>, _>("canonical_items") != batch.bytes
        {
            return Err(protocol("STAGING_BATCH_PAYLOAD_MISMATCH"));
        }
        tx.commit().await?;
        return Ok(());
    }
    let project_totals: (i64, i64) = sqlx::query_as(
        "SELECT COALESCE(sum(staged_item_count),0)::bigint,
                COALESCE(sum(staged_byte_length),0)::bigint
           FROM bid_matching_staging_sets WHERE project_id=$1 AND state='active'",
    )
    .bind(claimed.project_id)
    .fetch_one(&mut *tx)
    .await?;
    if project_totals.0 + batch.item_count as i64 > MAX_STAGING_ROWS_PER_PROJECT
        || project_totals.1 + batch.bytes.len() as i64 > MAX_STAGING_BYTES_PER_PROJECT
    {
        return Err(protocol("STAGING_PROJECT_QUOTA_EXCEEDED"));
    }
    sqlx::query(
        "INSERT INTO bid_matching_staged_batches
         (staging_set_id,batch_ordinal,collection_kind,canonical_items,payload_sha256,item_count,byte_length)
         VALUES($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(staging_id)
    .bind(batch.ordinal)
    .bind(batch.kind)
    .bind(&batch.bytes)
    .bind(payload_sha256)
    .bind(batch.item_count as i32)
    .bind(batch.bytes.len() as i64)
    .execute(&mut *tx)
    .await?;
    stage_typed_rows(&mut tx, staging_id, batch.ordinal, batch.kind, &batch.bytes).await?;
    sqlx::query(
        "UPDATE bid_matching_staging_sets SET staged_item_count=staged_item_count+$2,
          staged_byte_length=staged_byte_length+$3 WHERE id=$1",
    )
    .bind(staging_id)
    .bind(batch.item_count as i64)
    .bind(batch.bytes.len() as i64)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

async fn stage_typed_rows(
    tx: &mut Transaction<'_, Postgres>,
    staging_id: Uuid,
    batch_ordinal: i32,
    kind: &str,
    bytes: &[u8],
) -> Result<(), sqlx::Error> {
    match kind {
        "source_artifacts" => {
            for (item_ordinal, row) in serde_json::from_slice::<Vec<StagedSourceArtifactV1>>(bytes)
                .map_err(|error| protocol(error.to_string()))?
                .into_iter()
                .enumerate()
            {
                sqlx::query(
                    "INSERT INTO bid_matching_staged_source_artifacts
                  (staging_set_id,id,batch_ordinal,item_ordinal,product_version_artifact_id,
                   document_id,source_chunk_id,frozen_document_display_name,chunk_utf8,chunk_sha256,
                   chunk_byte_length,retrieval_rank,retrieval_raw_score,retrieval_contract_version)
                  VALUES($1,$2,$3,$4,$5,$6,$7,$8,convert_to($9,'UTF8'),$10,$11,$12,$13,$14)",
                )
                .bind(staging_id)
                .bind(row.id)
                .bind(batch_ordinal)
                .bind(item_ordinal as i32)
                .bind(row.product_version_artifact_id)
                .bind(row.document_id)
                .bind(row.source_chunk_id)
                .bind(row.frozen_document_display_name)
                .bind(row.chunk_utf8)
                .bind(row.chunk_sha256)
                .bind(row.chunk_byte_length as i64)
                .bind(row.retrieval_rank as i32)
                .bind(
                    row.retrieval_raw_score
                        .parse::<Decimal>()
                        .map_err(|_| protocol("invalid staged source score"))?,
                )
                .bind(row.retrieval_contract_version)
                .execute(&mut **tx)
                .await?;
            }
        }
        "candidates" => {
            for (item_ordinal, row) in serde_json::from_slice::<Vec<StagedCandidateV1>>(bytes)
                .map_err(|error| protocol(error.to_string()))?
                .into_iter()
                .enumerate()
            {
                sqlx::query("INSERT INTO bid_matching_staged_candidates
                  (staging_set_id,id,batch_ordinal,item_ordinal,requirement_artifact_id,
                   product_version_artifact_id,route_product_ordinal,retrieval_rank,retrieval_raw_score,
                   candidate_identity_sha256,evidence_v1_sha256,support,business_value_status,business_value,recommended)
                  VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)")
                    .bind(staging_id).bind(row.id).bind(batch_ordinal).bind(item_ordinal as i32)
                    .bind(row.requirement_artifact_id).bind(row.product_version_artifact_id)
                    .bind(row.route_product_ordinal as i32).bind(row.retrieval_rank as i32)
                    .bind(row.retrieval_raw_score.parse::<Decimal>().map_err(|_| protocol("invalid staged candidate score"))?)
                    .bind(row.candidate_identity_sha256).bind(row.evidence_v1_sha256).bind(row.support)
                    .bind(row.business_value_status)
                    .bind(row.business_value.as_deref().map(str::parse::<Decimal>).transpose().map_err(|_| protocol("invalid staged business value"))?)
                    .bind(row.recommended).execute(&mut **tx).await?;
            }
        }
        "evidences" => {
            for (item_ordinal, row) in serde_json::from_slice::<Vec<StagedEvidenceV1>>(bytes)
                .map_err(|error| protocol(error.to_string()))?
                .into_iter()
                .enumerate()
            {
                sqlx::query(
                    "INSERT INTO bid_matching_staged_evidences
                  (staging_set_id,id,batch_ordinal,item_ordinal,candidate_artifact_id,
                   source_chunk_artifact_id,document_id,document_display_name,source_chunk_id,
                   source_chunk_sha256,quote,start_offset,end_offset,offset_unit,ordinal)
                  VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)",
                )
                .bind(staging_id)
                .bind(row.id)
                .bind(batch_ordinal)
                .bind(item_ordinal as i32)
                .bind(row.candidate_artifact_id)
                .bind(row.source_chunk_artifact_id)
                .bind(row.document_id)
                .bind(row.document_display_name)
                .bind(row.source_chunk_id)
                .bind(row.source_chunk_sha256)
                .bind(row.quote)
                .bind(row.start_offset as i64)
                .bind(row.end_offset as i64)
                .bind(row.offset_unit)
                .bind(row.ordinal as i32)
                .execute(&mut **tx)
                .await?;
            }
        }
        "requirement_decisions" => {
            for (item_ordinal, row) in serde_json::from_slice::<Vec<StagedDecisionV1>>(bytes)
                .map_err(|error| protocol(error.to_string()))?
                .into_iter()
                .enumerate()
            {
                sqlx::query("INSERT INTO bid_matching_staged_requirement_decisions
                  (staging_set_id,id,batch_ordinal,item_ordinal,requirement_artifact_id,final_support,
                   system_decision,quality_status,reason_code,selected_candidate_artifact_id,ordinal)
                  VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)")
                    .bind(staging_id).bind(row.id).bind(batch_ordinal).bind(item_ordinal as i32)
                    .bind(row.requirement_artifact_id).bind(row.final_support).bind(row.system_decision)
                    .bind(row.quality_status).bind(row.reason_code).bind(row.selected_candidate_artifact_id)
                    .bind(row.ordinal as i32).execute(&mut **tx).await?;
            }
        }
        "candidate_groups" => {
            for (item_ordinal, row) in serde_json::from_slice::<Vec<StagedCandidateGroupV1>>(bytes)
                .map_err(|error| protocol(error.to_string()))?
                .into_iter()
                .enumerate()
            {
                sqlx::query(
                    "INSERT INTO bid_matching_staged_candidate_groups
                  (staging_set_id,id,batch_ordinal,item_ordinal,requirement_artifact_id,support,
                   ordinal,canonical_payload,content_sha256) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)",
                )
                .bind(staging_id)
                .bind(row.id)
                .bind(batch_ordinal)
                .bind(item_ordinal as i32)
                .bind(row.requirement_artifact_id)
                .bind(row.support)
                .bind(row.ordinal as i32)
                .bind(row.canonical_payload)
                .bind(row.content_sha256)
                .execute(&mut **tx)
                .await?;
            }
        }
        "reason_codes" => {
            for (item_ordinal, reason_code) in serde_json::from_slice::<Vec<String>>(bytes)
                .map_err(|error| protocol(error.to_string()))?
                .into_iter()
                .enumerate()
            {
                sqlx::query(
                    "INSERT INTO bid_matching_staged_reason_codes
                  (staging_set_id,batch_ordinal,item_ordinal,reason_code) VALUES($1,$2,$3,$4)",
                )
                .bind(staging_id)
                .bind(batch_ordinal)
                .bind(item_ordinal as i32)
                .bind(reason_code)
                .execute(&mut **tx)
                .await?;
            }
        }
        _ => return Err(protocol("unknown staged collection kind")),
    }
    Ok(())
}

struct OwnedStagedReportRelations {
    sources: Vec<StagedSourceArtifactV1>,
    candidates: Vec<StagedCandidateV1>,
    evidences: Vec<StagedEvidenceV1>,
    decisions: Vec<StagedDecisionV1>,
    groups: Vec<StagedCandidateGroupV1>,
    reason_codes: Vec<String>,
}

async fn load_typed_staged_rows(
    tx: &mut Transaction<'_, Postgres>,
    staging_id: Uuid,
) -> Result<OwnedStagedReportRelations, sqlx::Error> {
    let sources = sqlx::query(
        "SELECT source_value.*,convert_from(chunk_utf8,'UTF8') AS chunk_text
      FROM bid_matching_staged_source_artifacts source_value
      WHERE staging_set_id=$1 ORDER BY batch_ordinal,item_ordinal",
    )
    .bind(staging_id)
    .fetch_all(&mut **tx)
    .await?
    .into_iter()
    .map(|row| StagedSourceArtifactV1 {
        id: row.get("id"),
        product_version_artifact_id: row.get("product_version_artifact_id"),
        document_id: row.get("document_id"),
        source_chunk_id: row.get("source_chunk_id"),
        frozen_document_display_name: row.get("frozen_document_display_name"),
        chunk_utf8: row.get("chunk_text"),
        chunk_sha256: row.get("chunk_sha256"),
        chunk_byte_length: row.get::<i64, _>("chunk_byte_length") as u64,
        retrieval_rank: row.get::<i32, _>("retrieval_rank") as u32,
        retrieval_raw_score: format!("{:.6}", row.get::<Decimal, _>("retrieval_raw_score")),
        retrieval_contract_version: row.get("retrieval_contract_version"),
    })
    .collect();
    let candidates = sqlx::query(
        "SELECT * FROM bid_matching_staged_candidates
      WHERE staging_set_id=$1 ORDER BY batch_ordinal,item_ordinal",
    )
    .bind(staging_id)
    .fetch_all(&mut **tx)
    .await?
    .into_iter()
    .map(|row| StagedCandidateV1 {
        id: row.get("id"),
        requirement_artifact_id: row.get("requirement_artifact_id"),
        product_version_artifact_id: row.get("product_version_artifact_id"),
        route_product_ordinal: row.get::<i32, _>("route_product_ordinal") as u32,
        retrieval_rank: row.get::<i32, _>("retrieval_rank") as u32,
        retrieval_raw_score: format!("{:.6}", row.get::<Decimal, _>("retrieval_raw_score")),
        candidate_identity_sha256: row.get("candidate_identity_sha256"),
        evidence_v1_sha256: row.get("evidence_v1_sha256"),
        support: row.get("support"),
        business_value_status: row.get("business_value_status"),
        business_value: row
            .get::<Option<Decimal>, _>("business_value")
            .map(|value| format!("{value:.6}")),
        recommended: row.get("recommended"),
    })
    .collect();
    let evidences = sqlx::query(
        "SELECT * FROM bid_matching_staged_evidences
      WHERE staging_set_id=$1 ORDER BY batch_ordinal,item_ordinal",
    )
    .bind(staging_id)
    .fetch_all(&mut **tx)
    .await?
    .into_iter()
    .map(|row| StagedEvidenceV1 {
        id: row.get("id"),
        candidate_artifact_id: row.get("candidate_artifact_id"),
        source_chunk_artifact_id: row.get("source_chunk_artifact_id"),
        document_id: row.get("document_id"),
        document_display_name: row.get("document_display_name"),
        source_chunk_id: row.get("source_chunk_id"),
        source_chunk_sha256: row.get("source_chunk_sha256"),
        quote: row.get("quote"),
        start_offset: row.get::<i64, _>("start_offset") as u64,
        end_offset: row.get::<i64, _>("end_offset") as u64,
        offset_unit: row.get("offset_unit"),
        ordinal: row.get::<i32, _>("ordinal") as u32,
    })
    .collect();
    let decisions = sqlx::query(
        "SELECT * FROM bid_matching_staged_requirement_decisions
      WHERE staging_set_id=$1 ORDER BY batch_ordinal,item_ordinal",
    )
    .bind(staging_id)
    .fetch_all(&mut **tx)
    .await?
    .into_iter()
    .map(|row| StagedDecisionV1 {
        id: row.get("id"),
        requirement_artifact_id: row.get("requirement_artifact_id"),
        final_support: row.get("final_support"),
        system_decision: row.get("system_decision"),
        quality_status: row.get("quality_status"),
        reason_code: row.get("reason_code"),
        selected_candidate_artifact_id: row.get("selected_candidate_artifact_id"),
        ordinal: row.get::<i32, _>("ordinal") as u32,
    })
    .collect();
    let groups = sqlx::query(
        "SELECT * FROM bid_matching_staged_candidate_groups
      WHERE staging_set_id=$1 ORDER BY batch_ordinal,item_ordinal",
    )
    .bind(staging_id)
    .fetch_all(&mut **tx)
    .await?
    .into_iter()
    .map(|row| StagedCandidateGroupV1 {
        id: row.get("id"),
        requirement_artifact_id: row.get("requirement_artifact_id"),
        support: row.get("support"),
        ordinal: row.get::<i32, _>("ordinal") as u32,
        canonical_payload: row.get("canonical_payload"),
        content_sha256: row.get("content_sha256"),
    })
    .collect();
    let reason_codes = sqlx::query_scalar(
        "SELECT reason_code FROM bid_matching_staged_reason_codes
      WHERE staging_set_id=$1 ORDER BY batch_ordinal,item_ordinal",
    )
    .bind(staging_id)
    .fetch_all(&mut **tx)
    .await?;
    Ok(OwnedStagedReportRelations {
        sources,
        candidates,
        evidences,
        decisions,
        groups,
        reason_codes,
    })
}

async fn commit_staged_route(
    pool: &PgPool,
    claimed: &ClaimedMatchingRequest,
    staging_id: Uuid,
    report_id: Uuid,
    expected_report_sha256: &str,
) -> Result<PublishReceipt, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let job = lock_live_claim(&mut tx, claimed).await?;
    if job.get::<String, _>("status") == "completed" {
        let completed: Option<Uuid> = job.get("completed_report_id");
        tx.rollback().await?;
        return Ok(completed.map_or(PublishReceipt::Stale, |report_id| {
            PublishReceipt::Replayed { report_id }
        }));
    }
    let set = sqlx::query("SELECT * FROM bid_matching_staging_sets WHERE id=$1 FOR UPDATE")
        .bind(staging_id)
        .fetch_one(&mut *tx)
        .await?;
    if set.get::<String, _>("state") == "consumed" {
        let completed: Option<Uuid> = set.get("consumed_report_id");
        tx.rollback().await?;
        return Ok(completed.map_or(PublishReceipt::Stale, |report_id| {
            PublishReceipt::Replayed { report_id }
        }));
    }
    let ttl_live: bool = sqlx::query_scalar(
        "SELECT expires_at>clock_timestamp() FROM bid_matching_staging_sets WHERE id=$1",
    )
    .bind(staging_id)
    .fetch_one(&mut *tx)
    .await?;
    if set.get::<String, _>("state") != "active" || !ttl_live {
        return Err(protocol("staging set expired"));
    }
    let rows = sqlx::query(
        "SELECT batch_ordinal,collection_kind,canonical_items,item_count,byte_length
         FROM bid_matching_staged_batches WHERE staging_set_id=$1 ORDER BY batch_ordinal",
    )
    .bind(staging_id)
    .fetch_all(&mut *tx)
    .await?;
    if rows.len() < 6
        || rows
            .iter()
            .enumerate()
            .any(|(index, row)| row.get::<i32, _>("batch_ordinal") != index as i32)
    {
        return Err(protocol("STAGING_BATCH_ORDINAL_GAP"));
    }
    let mut collections: HashMap<String, Vec<Vec<u8>>> = HashMap::new();
    let mut expected_items = 0i64;
    let mut expected_bytes = 0i64;
    for row in rows {
        expected_items += i64::from(row.get::<i32, _>("item_count"));
        expected_bytes += row.get::<i64, _>("byte_length");
        collections
            .entry(row.get::<String, _>("collection_kind"))
            .or_default()
            .push(row.get::<Vec<u8>, _>("canonical_items"));
    }
    if expected_items != set.get::<i64, _>("staged_item_count")
        || expected_bytes != set.get::<i64, _>("staged_byte_length")
    {
        return Err(protocol("STAGING_COUNTER_MISMATCH"));
    }
    let required_kinds = [
        "source_artifacts",
        "candidates",
        "evidences",
        "requirement_decisions",
        "candidate_groups",
        "reason_codes",
    ];
    if collections.len() != required_kinds.len()
        || required_kinds
            .iter()
            .any(|kind| !collections.contains_key(*kind))
    {
        return Err(protocol("STAGING_COLLECTION_SET_MISMATCH"));
    }
    let loaded = load_typed_staged_rows(&mut tx, staging_id).await?;
    let sources = loaded.sources;
    let candidates = loaded.candidates;
    let evidences = loaded.evidences;
    let decisions = loaded.decisions;
    let groups = loaded.groups;
    let reason_codes = loaded.reason_codes;
    if (sources.len()
        + candidates.len()
        + evidences.len()
        + decisions.len()
        + groups.len()
        + reason_codes.len()) as i64
        != expected_items
    {
        return Err(protocol("STAGING_TYPED_ROW_COUNT_MISMATCH"));
    }

    let payload: Vec<u8> = sqlx::query_scalar(
        "SELECT canonical_payload FROM bid_matching_staging_report_payloads WHERE staging_set_id=$1",
    )
    .bind(staging_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| protocol("staged report payload missing"))?;
    if sha256_hex(&payload) != expected_report_sha256 {
        return Err(protocol("REPORT_HASH_MISMATCH"));
    }
    verify_staged_report(
        &mut tx,
        claimed,
        report_id,
        &payload,
        StagedReportRelations {
            sources: &sources,
            candidates: &candidates,
            evidences: &evidences,
            decisions: &decisions,
            groups: &groups,
            reason_codes: &reason_codes,
        },
    )
    .await?;
    let parsed: serde_json::Value =
        serde_json::from_slice(&payload).map_err(|_| protocol("invalid report JSON"))?;
    let coverage = parsed
        .get("coverage")
        .ok_or_else(|| protocol("coverage missing"))?;
    sqlx::query(
        "INSERT INTO bid_matching_reports
         (id,project_id,manifest_id,job_id,route_id,generation,mutation_watermark,empty_disposition,
          coverage_total,coverage_supported,coverage_contradicted,coverage_insufficient,coverage_unresolved,
          quality_status,degraded,reason_codes,canonical_payload,content_sha256,ai_run_id,ai_span_id,published_at)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,clock_timestamp())",
    )
    .bind(report_id)
    .bind(claimed.project_id)
    .bind(claimed.manifest_id)
    .bind(claimed.job_id)
    .bind(claimed.route_id)
    .bind(claimed.generation)
    .bind(claimed.mutation_watermark)
    .bind(parsed.get("empty_disposition").and_then(|value| value.as_str()))
    .bind(json_i32(coverage, "total")?)
    .bind(json_i32(coverage, "supported")?)
    .bind(json_i32(coverage, "contradicted")?)
    .bind(json_i32(coverage, "insufficient")?)
    .bind(json_i32(coverage, "unresolved")?)
    .bind(json_str(&parsed, "quality_status")?)
    .bind(parsed.get("degraded").and_then(|value| value.as_bool()).ok_or_else(|| protocol("degraded missing"))?)
    .bind(&reason_codes)
    .bind(&payload)
    .bind(expected_report_sha256)
    .bind(parsed.get("ai_run_id").and_then(|value| value.as_str()).and_then(|value| Uuid::parse_str(value).ok()))
    .bind(parsed.get("ai_span_id").and_then(|value| value.as_str()).and_then(|value| Uuid::parse_str(value).ok()))
    .execute(&mut *tx)
    .await?;
    for source in &sources {
        sqlx::query(
            "INSERT INTO bid_matching_source_artifacts
             (id,report_id,product_version_artifact_id,document_id,source_chunk_id,
              frozen_document_display_name,chunk_utf8,chunk_sha256,chunk_byte_length,
              retrieval_rank,retrieval_raw_score,retrieval_contract_version)
             VALUES($1,$2,$3,$4,$5,$6,convert_to($7,'UTF8'),$8,$9,$10,$11,$12)",
        )
        .bind(source.id)
        .bind(report_id)
        .bind(source.product_version_artifact_id)
        .bind(source.document_id)
        .bind(source.source_chunk_id)
        .bind(&source.frozen_document_display_name)
        .bind(&source.chunk_utf8)
        .bind(&source.chunk_sha256)
        .bind(source.chunk_byte_length as i64)
        .bind(source.retrieval_rank as i32)
        .bind(
            source
                .retrieval_raw_score
                .parse::<Decimal>()
                .map_err(|_| protocol("invalid source score"))?,
        )
        .bind(&source.retrieval_contract_version)
        .execute(&mut *tx)
        .await?;
    }
    for candidate in &candidates {
        sqlx::query(
            "INSERT INTO bid_matching_candidate_artifacts
             (id,report_id,requirement_artifact_id,product_version_artifact_id,support,
              candidate_identity_sha256,evidence_v1_sha256,business_value_status,business_value,
              route_product_ordinal,retrieval_rank,retrieval_raw_score,recommended)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)",
        )
        .bind(candidate.id)
        .bind(report_id)
        .bind(candidate.requirement_artifact_id)
        .bind(candidate.product_version_artifact_id)
        .bind(&candidate.support)
        .bind(&candidate.candidate_identity_sha256)
        .bind(&candidate.evidence_v1_sha256)
        .bind(&candidate.business_value_status)
        .bind(
            candidate
                .business_value
                .as_deref()
                .map(str::parse::<Decimal>)
                .transpose()
                .map_err(|_| protocol("invalid business value"))?,
        )
        .bind(candidate.route_product_ordinal as i32)
        .bind(candidate.retrieval_rank as i32)
        .bind(
            candidate
                .retrieval_raw_score
                .parse::<Decimal>()
                .map_err(|_| protocol("invalid candidate score"))?,
        )
        .bind(candidate.recommended)
        .execute(&mut *tx)
        .await?;
    }
    for evidence in &evidences {
        sqlx::query(
            "INSERT INTO bid_matching_evidence_artifacts
             (id,report_id,candidate_artifact_id,source_chunk_artifact_id,start_offset,end_offset,
              quote_utf8,quote_sha256,ordinal,offset_unit,document_id,document_display_name,
              source_chunk_id,source_chunk_sha256)
             VALUES($1,$2,$3,$4,$5,$6,convert_to($7,'UTF8'),$8,$9,$10,$11,$12,$13,$14)",
        )
        .bind(evidence.id)
        .bind(report_id)
        .bind(evidence.candidate_artifact_id)
        .bind(evidence.source_chunk_artifact_id)
        .bind(evidence.start_offset as i64)
        .bind(evidence.end_offset as i64)
        .bind(&evidence.quote)
        .bind(sha256_hex(evidence.quote.as_bytes()))
        .bind(evidence.ordinal as i32)
        .bind(&evidence.offset_unit)
        .bind(evidence.document_id)
        .bind(&evidence.document_display_name)
        .bind(evidence.source_chunk_id)
        .bind(&evidence.source_chunk_sha256)
        .execute(&mut *tx)
        .await?;
    }
    for decision in &decisions {
        sqlx::query(
            "INSERT INTO bid_matching_requirement_decisions
             (id,report_id,requirement_artifact_id,final_support,system_decision,quality_status,
              reason_code,selected_candidate_artifact_id,ordinal)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)",
        )
        .bind(decision.id)
        .bind(report_id)
        .bind(decision.requirement_artifact_id)
        .bind(&decision.final_support)
        .bind(&decision.system_decision)
        .bind(&decision.quality_status)
        .bind(&decision.reason_code)
        .bind(decision.selected_candidate_artifact_id)
        .bind(decision.ordinal as i32)
        .execute(&mut *tx)
        .await?;
    }
    for group in &groups {
        sqlx::query(
            "INSERT INTO bid_matching_candidate_groups
             (id,report_id,requirement_artifact_id,support,ordinal,canonical_payload,content_sha256)
             VALUES($1,$2,$3,$4,$5,$6,$7)",
        )
        .bind(group.id)
        .bind(report_id)
        .bind(group.requirement_artifact_id)
        .bind(&group.support)
        .bind(group.ordinal as i32)
        .bind(&group.canonical_payload)
        .bind(&group.content_sha256)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query(
        "INSERT INTO bid_current_matching_reports(project_id,route_id,report_id,generation,mutation_watermark)
         VALUES($1,$2,$3,$4,$5)
         ON CONFLICT(project_id,route_id) DO UPDATE SET report_id=EXCLUDED.report_id,
          generation=EXCLUDED.generation,mutation_watermark=EXCLUDED.mutation_watermark",
    )
    .bind(claimed.project_id).bind(claimed.route_id).bind(report_id)
    .bind(claimed.generation).bind(claimed.mutation_watermark).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM bid_current_route_pick_sets WHERE project_id=$1 AND route_id=$2")
        .bind(claimed.project_id)
        .bind(claimed.route_id)
        .execute(&mut *tx)
        .await?;
    rebuild_project_pick_set(&mut tx, claimed.project_id, "system:matching-publication").await?;
    sqlx::query("UPDATE bid_matching_job_claims SET status='completed' WHERE job_id=$1 AND attempt=$2 AND claim_token=$3")
        .bind(claimed.job_id).bind(claimed.claim.attempt).bind(claimed.claim.token).execute(&mut *tx).await?;
    sqlx::query("UPDATE bid_matching_jobs SET status='completed',active_attempt=NULL,completed_report_id=$2,finished_at=clock_timestamp() WHERE id=$1")
        .bind(claimed.job_id).bind(report_id).execute(&mut *tx).await?;
    sqlx::query(
        "UPDATE bid_matching_staging_sets SET state='consumed',consumed_report_id=$2 WHERE id=$1",
    )
    .bind(staging_id)
    .bind(report_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(PublishReceipt::Committed { report_id })
}

async fn lock_live_claim(
    tx: &mut Transaction<'_, Postgres>,
    claimed: &ClaimedMatchingRequest,
) -> Result<sqlx::postgres::PgRow, sqlx::Error> {
    let job = sqlx::query(
        "SELECT j.*,claim.heartbeat_at,claim.claim_lease_ms,claim.status AS claim_status,
                (claim.heartbeat_at+make_interval(secs=>claim.claim_lease_ms::double precision/1000.0)>clock_timestamp()) AS lease_live,
                m.generation,m.mutation_watermark,p.status AS project_status,p.matching_mutation_watermark
           FROM bid_matching_jobs j
           JOIN bid_matching_job_claims claim ON claim.job_id=j.id AND claim.attempt=j.active_attempt
           JOIN bid_matching_manifests m ON m.id=j.manifest_id
           JOIN bid_projects p ON p.id=j.project_id
          WHERE j.id=$1 FOR UPDATE OF p,j,claim",
    )
    .bind(claimed.job_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| protocol("matching claim missing"))?;
    if job.get::<Uuid, _>("manifest_id") != claimed.manifest_id
        || job.get::<Uuid, _>("route_id") != claimed.route_id
        || job.get::<i32, _>("active_attempt") != claimed.claim.attempt
        || job.get::<String, _>("claim_status") != "running"
        || job.get::<String, _>("project_status") != "open"
        || job.get::<i64, _>("matching_mutation_watermark") != claimed.mutation_watermark
        || job.get::<i64, _>("generation") != claimed.generation
        || job.get::<i64, _>("mutation_watermark") != claimed.mutation_watermark
        || !job.get::<bool, _>("lease_live")
    {
        return Err(protocol("matching claim fence lost"));
    }
    Ok(job)
}

struct StagedReportRelations<'a> {
    sources: &'a [StagedSourceArtifactV1],
    candidates: &'a [StagedCandidateV1],
    evidences: &'a [StagedEvidenceV1],
    decisions: &'a [StagedDecisionV1],
    groups: &'a [StagedCandidateGroupV1],
    reason_codes: &'a [String],
}

async fn verify_staged_report(
    tx: &mut Transaction<'_, Postgres>,
    claimed: &ClaimedMatchingRequest,
    report_id: Uuid,
    payload: &[u8],
    relations: StagedReportRelations<'_>,
) -> Result<(), sqlx::Error> {
    let StagedReportRelations {
        sources,
        candidates,
        evidences,
        decisions,
        groups,
        reason_codes,
    } = relations;
    let parsed: serde_json::Value =
        serde_json::from_slice(payload).map_err(|_| protocol("invalid MatchingReportV1 bytes"))?;
    let object = parsed
        .as_object()
        .ok_or_else(|| protocol("report must be an object"))?;
    let expected_keys = [
        "schema_version",
        "report_id",
        "manifest_id",
        "job_id",
        "route_id",
        "route",
        "generation",
        "mutation_watermark",
        "empty_disposition",
        "coverage",
        "quality_status",
        "degraded",
        "reason_codes",
        "score",
        "requirement_decisions",
        "candidates",
        "candidate_groups",
        "source_artifacts",
        "ai_run_id",
        "ai_span_id",
    ];
    let expected_route = match claimed.route {
        MatchRoute::Technical { unit_id } => {
            serde_json::json!({"kind":"technical","unit_id":unit_id})
        }
        MatchRoute::Commercial => serde_json::json!({"kind":"commercial"}),
    };
    if object.len() != expected_keys.len()
        || expected_keys.iter().any(|key| !object.contains_key(*key))
        || parsed
            .get("schema_version")
            .and_then(|value| value.as_u64())
            != Some(1)
        || parsed.get("report_id").and_then(|value| value.as_str())
            != Some(report_id.to_string().as_str())
        || parsed.get("manifest_id").and_then(|value| value.as_str())
            != Some(claimed.manifest_id.to_string().as_str())
        || parsed.get("job_id").and_then(|value| value.as_str())
            != Some(claimed.job_id.to_string().as_str())
        || parsed.get("route_id").and_then(|value| value.as_str())
            != Some(claimed.route_id.to_string().as_str())
        || parsed.get("generation").and_then(|value| value.as_i64()) != Some(claimed.generation)
        || parsed
            .get("mutation_watermark")
            .and_then(|value| value.as_i64())
            != Some(claimed.mutation_watermark)
        || parsed.get("route") != Some(&expected_route)
    {
        return Err(protocol("report fixed header mismatch"));
    }
    if parsed.get("reason_codes") != Some(&serde_json::to_value(reason_codes).unwrap())
        || !strict_sorted_unique(reason_codes)
    {
        return Err(protocol("report reason relation mismatch"));
    }
    let requirement_ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM bid_matching_requirement_artifacts WHERE route_id=$1 ORDER BY ordinal,id",
    )
    .bind(claimed.route_id)
    .fetch_all(&mut **tx)
    .await?;
    if decisions.len() != requirement_ids.len()
        || decisions
            .iter()
            .map(|row| row.requirement_artifact_id)
            .collect::<Vec<_>>()
            != requirement_ids
    {
        return Err(protocol("decision cardinality or order mismatch"));
    }
    let memberships: HashMap<Uuid, i32> = sqlx::query(
        "SELECT product_version_artifact_id,route_product_ordinal FROM bid_matching_route_memberships WHERE route_id=$1",
    )
    .bind(claimed.route_id).fetch_all(&mut **tx).await?.into_iter()
    .map(|row| (row.get("product_version_artifact_id"), row.get("route_product_ordinal"))).collect();
    let source_map: HashMap<Uuid, &StagedSourceArtifactV1> =
        sources.iter().map(|row| (row.id, row)).collect();
    if source_map.len() != sources.len() {
        return Err(protocol("duplicate source artifact"));
    }
    let mut evidence_by_candidate: HashMap<Uuid, Vec<&StagedEvidenceV1>> = HashMap::new();
    for evidence in evidences {
        let source = source_map
            .get(&evidence.source_chunk_artifact_id)
            .ok_or_else(|| protocol("evidence source missing"))?;
        let bytes = source.chunk_utf8.as_bytes();
        let start =
            usize::try_from(evidence.start_offset).map_err(|_| protocol("offset overflow"))?;
        let end = usize::try_from(evidence.end_offset).map_err(|_| protocol("offset overflow"))?;
        if evidence.offset_unit != "utf8_byte"
            || start >= end
            || end > bytes.len()
            || !source.chunk_utf8.is_char_boundary(start)
            || !source.chunk_utf8.is_char_boundary(end)
            || source.chunk_utf8.get(start..end) != Some(evidence.quote.as_str())
            || evidence.document_id != source.document_id
            || evidence.source_chunk_id != source.source_chunk_id
            || evidence.source_chunk_sha256 != source.chunk_sha256
            || evidence.document_display_name != source.frozen_document_display_name
        {
            return Err(protocol("EvidenceV1 byte-slice verifier failed"));
        }
        evidence_by_candidate
            .entry(evidence.candidate_artifact_id)
            .or_default()
            .push(evidence);
    }
    let mut relation_candidates = Vec::with_capacity(candidates.len());
    let candidates_by_requirement: HashMap<Uuid, Vec<&StagedCandidateV1>> = {
        let mut map: HashMap<Uuid, Vec<&StagedCandidateV1>> = HashMap::new();
        for candidate in candidates {
            let Some(ordinal) = memberships.get(&candidate.product_version_artifact_id) else {
                return Err(protocol("candidate outside route membership"));
            };
            if *ordinal != candidate.route_product_ordinal as i32
                || !requirement_ids.contains(&candidate.requirement_artifact_id)
            {
                return Err(protocol("candidate scope or route ordinal mismatch"));
            }
            let mut evidence_rows = evidence_by_candidate
                .remove(&candidate.id)
                .unwrap_or_default();
            evidence_rows.sort_by_key(|row| row.ordinal);
            let evidence_value = serde_json::json!({"schema_version":1,"items":evidence_rows.iter().map(|row| serde_json::json!({
                "source_chunk_artifact_id":row.source_chunk_artifact_id,"document_id":row.document_id,
                "document_display_name":row.document_display_name,"source_chunk_id":row.source_chunk_id,
                "source_chunk_sha256":row.source_chunk_sha256,"quote":row.quote,"start_offset":row.start_offset,
                "end_offset":row.end_offset,"offset_unit":row.offset_unit
            })).collect::<Vec<_>>()});
            if sha256_hex(&serde_json::to_vec(&evidence_value).unwrap())
                != candidate.evidence_v1_sha256
            {
                return Err(protocol("EvidenceV1 hash mismatch"));
            }
            let business_value = match (&candidate.business_value_status, &candidate.business_value)
            {
                (status, Some(value)) if status == "scored" => {
                    serde_json::json!({"status":"scored","value":value,"source":"verifier"})
                }
                (status, None) if status == "not_scored" => {
                    serde_json::json!({"status":"not_scored","reason":"NO_EVIDENCE"})
                }
                _ => return Err(protocol("candidate business value matrix mismatch")),
            };
            relation_candidates.push(serde_json::json!({
                "id":candidate.id,"requirement_artifact_id":candidate.requirement_artifact_id,
                "product_version_artifact_id":candidate.product_version_artifact_id,
                "route_product_ordinal":candidate.route_product_ordinal,"retrieval_rank":candidate.retrieval_rank,
                "retrieval_raw_score":candidate.retrieval_raw_score,"candidate_identity_sha256":candidate.candidate_identity_sha256,
                "evidence_v1_sha256":candidate.evidence_v1_sha256,"evidence":evidence_value,
                "support":candidate.support,"business_value":business_value,"recommended":candidate.recommended
            }));
            map.entry(candidate.requirement_artifact_id)
                .or_default()
                .push(candidate);
        }
        map
    };
    if !evidence_by_candidate.is_empty() {
        return Err(protocol("orphan staged evidence"));
    }
    let candidates_by_id: HashMap<Uuid, &StagedCandidateV1> =
        candidates.iter().map(|row| (row.id, row)).collect();
    let mut relation_decisions = Vec::with_capacity(decisions.len());
    for decision in decisions {
        let rows = candidates_by_requirement
            .get(&decision.requirement_artifact_id)
            .cloned()
            .unwrap_or_default();
        let expected = aggregate_decision(&rows);
        let recommended = rows
            .iter()
            .filter(|row| row.recommended)
            .map(|row| row.id)
            .collect::<Vec<_>>();
        if decision.final_support != expected.0
            || decision.system_decision != expected.1
            || decision.quality_status != expected.2
            || decision.reason_code != expected.3
            || decision.selected_candidate_artifact_id != expected.4
            || recommended != expected.4.into_iter().collect::<Vec<_>>()
        {
            return Err(protocol("RequirementDecisionV1 aggregation mismatch"));
        }
        let business_value = decision
            .selected_candidate_artifact_id
            .and_then(|id| candidates_by_id.get(&id).copied())
            .map_or_else(
                || serde_json::json!({"status":"not_scored","reason":"NO_EVIDENCE"}),
                |candidate| match &candidate.business_value {
                    Some(value) => {
                        serde_json::json!({"status":"scored","value":value,"source":"verifier"})
                    }
                    None => serde_json::json!({"status":"not_scored","reason":"NO_EVIDENCE"}),
                },
            );
        relation_decisions.push(serde_json::json!({"requirement_artifact_id":decision.requirement_artifact_id,
          "final_support":decision.final_support,"system_decision":decision.system_decision,"quality_status":decision.quality_status,
          "reason_code":decision.reason_code,"selected_candidate_artifact_id":decision.selected_candidate_artifact_id,
          "business_value":business_value}));
    }
    let quality = if decisions.iter().any(|row| row.quality_status == "block") {
        "block"
    } else if decisions.is_empty() || decisions.iter().any(|row| row.quality_status == "review") {
        "review"
    } else {
        "pass"
    };
    let mut expected_reasons = BTreeSet::from(["FROZEN_SCOPE".to_string()]);
    expected_reasons.extend(decisions.iter().map(|row| row.reason_code.clone()));
    if decisions.is_empty() {
        expected_reasons.insert("EMPTY_ROUTE".into());
        if parsed
            .get("empty_disposition")
            .and_then(|value| value.as_str())
            == Some("skip_unit")
        {
            expected_reasons.insert("SKIP_UNIT".into());
        }
    }
    if expected_reasons.into_iter().collect::<Vec<_>>() != reason_codes {
        return Err(protocol("MatchingReportV1 reason aggregation mismatch"));
    }
    let coverage = parsed
        .get("coverage")
        .ok_or_else(|| protocol("coverage missing"))?;
    let counts = (
        decisions
            .iter()
            .filter(|row| row.final_support == "supported")
            .count() as i32,
        decisions
            .iter()
            .filter(|row| row.final_support == "contradicted")
            .count() as i32,
        decisions
            .iter()
            .filter(|row| row.final_support == "insufficient")
            .count() as i32,
        decisions
            .iter()
            .filter(|row| row.final_support == "unresolved")
            .count() as i32,
    );
    if json_str(&parsed, "quality_status")? != quality
        || parsed.get("degraded").and_then(|v| v.as_bool()) != Some(quality != "pass")
        || json_i32(coverage, "total")? != decisions.len() as i32
        || json_i32(coverage, "eligible")? != decisions.len() as i32
        || (
            json_i32(coverage, "supported")?,
            json_i32(coverage, "contradicted")?,
            json_i32(coverage, "insufficient")?,
            json_i32(coverage, "unresolved")?,
        ) != counts
    {
        return Err(protocol("MatchingReportV1 aggregation mismatch"));
    }
    let relation_groups = groups
        .iter()
        .map(|row| serde_json::from_slice::<serde_json::Value>(&row.canonical_payload))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| protocol("invalid candidate group payload"))?;
    let relation_sources=sources.iter().map(|row|serde_json::json!({"id":row.id,
      "product_version_artifact_id":row.product_version_artifact_id,"document_id":row.document_id,
      "source_chunk_id":row.source_chunk_id,"frozen_document_display_name":row.frozen_document_display_name,
      "chunk_sha256":row.chunk_sha256,"chunk_byte_length":row.chunk_byte_length,"retrieval_rank":row.retrieval_rank,
      "retrieval_raw_score":row.retrieval_raw_score,"retrieval_contract_version":row.retrieval_contract_version})).collect::<Vec<_>>();
    if parsed.get("candidates") != Some(&serde_json::Value::Array(relation_candidates))
        || parsed.get("requirement_decisions")
            != Some(&serde_json::Value::Array(relation_decisions))
        || parsed.get("candidate_groups") != Some(&serde_json::Value::Array(relation_groups))
        || parsed.get("source_artifacts") != Some(&serde_json::Value::Array(relation_sources))
    {
        return Err(protocol("MatchingReportV1 payload/relation mismatch"));
    }
    Ok(())
}

fn aggregate_decision(
    rows: &[&StagedCandidateV1],
) -> (String, String, String, String, Option<Uuid>) {
    let has = |support: &str| rows.iter().any(|row| row.support == support);
    if has("supported") {
        let selected = rows
            .iter()
            .filter(|row| row.support == "supported")
            .min_by_key(|row| {
                (
                    row.route_product_ordinal,
                    row.retrieval_rank,
                    row.candidate_identity_sha256.clone(),
                    row.evidence_v1_sha256.clone(),
                )
            })
            .map(|row| row.id);
        (
            "supported".into(),
            "select".into(),
            "pass".into(),
            "SUPPORTED".into(),
            selected,
        )
    } else if has("unresolved") {
        (
            "unresolved".into(),
            "review".into(),
            "review".into(),
            "UNRESOLVED".into(),
            None,
        )
    } else if has("insufficient") {
        (
            "insufficient".into(),
            "review".into(),
            "review".into(),
            "INSUFFICIENT".into(),
            None,
        )
    } else if has("contradicted") {
        (
            "contradicted".into(),
            "reject".into(),
            "block".into(),
            "CONTRADICTED".into(),
            None,
        )
    } else {
        (
            "insufficient".into(),
            "review".into(),
            "review".into(),
            "NO_EVIDENCE".into(),
            None,
        )
    }
}

async fn stage_report_payload_for_set(
    pool: &PgPool,
    claimed: &ClaimedMatchingRequest,
    staging_id: Uuid,
    canonical_payload: &[u8],
    content_sha256: &str,
) -> Result<(), sqlx::Error> {
    if sha256_hex(canonical_payload) != content_sha256 {
        return Err(protocol("REPORT_HASH_MISMATCH"));
    }
    let mut tx = pool.begin().await?;
    lock_live_claim(&mut tx, claimed).await?;
    let active: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM bid_matching_staging_sets
          WHERE id=$1 AND state='active' AND expires_at>clock_timestamp())",
    )
    .bind(staging_id)
    .fetch_one(&mut *tx)
    .await?;
    if !active {
        return Err(protocol("active staging set missing"));
    }
    sqlx::query(
        "INSERT INTO bid_matching_staging_report_payloads(staging_set_id,canonical_payload,content_sha256)
         VALUES($1,$2,$3) ON CONFLICT(staging_set_id) DO NOTHING",
    )
    .bind(staging_id)
    .bind(canonical_payload)
    .bind(content_sha256)
    .execute(&mut *tx)
    .await?;
    let stored: (Vec<u8>, String) = sqlx::query_as(
        "SELECT canonical_payload,content_sha256 FROM bid_matching_staging_report_payloads WHERE staging_set_id=$1",
    )
    .bind(staging_id)
    .fetch_one(&mut *tx)
    .await?;
    if stored.0 != canonical_payload || stored.1 != content_sha256 {
        return Err(protocol("STAGED_REPORT_PAYLOAD_MISMATCH"));
    }
    tx.commit().await?;
    Ok(())
}

pub async fn retry_claim(
    pool: &PgPool,
    claimed: &ClaimedMatchingRequest,
    code: &str,
    detail: &str,
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    if lock_live_claim(&mut tx, claimed).await.is_err() {
        tx.rollback().await?;
        return Ok(false);
    }
    sqlx::query("UPDATE bid_matching_job_claims SET status='failed' WHERE job_id=$1 AND attempt=$2 AND claim_token=$3")
      .bind(claimed.job_id).bind(claimed.claim.attempt).bind(claimed.claim.token).execute(&mut *tx).await?;
    sqlx::query("UPDATE bid_matching_jobs SET status='pending',active_attempt=NULL,error_code=$2,error_detail=$3 WHERE id=$1")
      .bind(claimed.job_id).bind(code).bind(bound_detail(detail)).execute(&mut *tx).await?;
    sqlx::query("UPDATE bid_matching_staging_sets SET state='failed' WHERE job_id=$1 AND attempt=$2 AND claim_token=$3 AND state='active'")
      .bind(claimed.job_id).bind(claimed.claim.attempt).bind(claimed.claim.token).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(true)
}

pub async fn fail_claim(
    pool: &PgPool,
    claimed: &ClaimedMatchingRequest,
    code: &str,
    detail: &str,
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    if lock_live_claim(&mut tx, claimed).await.is_err() {
        tx.rollback().await?;
        return Ok(false);
    }
    sqlx::query("UPDATE bid_matching_job_claims SET status='failed' WHERE job_id=$1 AND attempt=$2 AND claim_token=$3")
      .bind(claimed.job_id).bind(claimed.claim.attempt).bind(claimed.claim.token).execute(&mut *tx).await?;
    sqlx::query("UPDATE bid_matching_jobs SET status='failed',active_attempt=NULL,error_code=$2,error_detail=$3,finished_at=clock_timestamp() WHERE id=$1")
      .bind(claimed.job_id).bind(code).bind(bound_detail(detail)).execute(&mut *tx).await?;
    sqlx::query("UPDATE bid_matching_staging_sets SET state='failed' WHERE job_id=$1 AND attempt=$2 AND claim_token=$3 AND state='active'")
      .bind(claimed.job_id).bind(claimed.claim.attempt).bind(claimed.claim.token).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(true)
}

pub async fn reap_expired_claims(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let rows=sqlx::query("SELECT claim.job_id,claim.attempt,claim.claim_token,job.max_attempts
      FROM bid_matching_job_claims claim JOIN bid_matching_jobs job ON job.id=claim.job_id AND job.active_attempt=claim.attempt
      WHERE claim.status='running' AND job.status='running'
        AND claim.heartbeat_at+make_interval(secs=>claim.claim_lease_ms::double precision/1000.0)<=clock_timestamp()
      ORDER BY claim.job_id FOR UPDATE OF job,claim SKIP LOCKED").fetch_all(&mut *tx).await?;
    for row in &rows {
        let job_id: Uuid = row.get("job_id");
        let attempt: i32 = row.get("attempt");
        let max: i32 = row.get("max_attempts");
        sqlx::query(
            "UPDATE bid_matching_job_claims SET status='reaped' WHERE job_id=$1 AND attempt=$2",
        )
        .bind(job_id)
        .bind(attempt)
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE bid_matching_jobs SET status=$2,active_attempt=NULL,error_code='CLAIM_LEASE_EXPIRED',
        finished_at=CASE WHEN $2='failed' THEN clock_timestamp() ELSE NULL END WHERE id=$1")
        .bind(job_id).bind(if attempt>=max{"failed"}else{"pending"}).execute(&mut *tx).await?;
        sqlx::query("UPDATE bid_matching_staging_sets SET state='expired' WHERE job_id=$1 AND attempt=$2 AND state='active'")
        .bind(job_id).bind(attempt).execute(&mut *tx).await?;
    }
    let expired=sqlx::query("UPDATE bid_matching_staging_sets SET state='expired' WHERE state='active' AND expires_at<=clock_timestamp()")
      .execute(&mut *tx).await?.rows_affected();
    sqlx::query("DELETE FROM bid_matching_staging_report_payloads payload USING bid_matching_staging_sets staging
      WHERE payload.staging_set_id=staging.id AND staging.state IN ('expired','failed')
        AND staging.created_at<clock_timestamp()-interval '24 hours'")
      .execute(&mut *tx).await?;
    sqlx::query(
        "DELETE FROM bid_matching_staged_batches batch USING bid_matching_staging_sets staging
      WHERE batch.staging_set_id=staging.id AND staging.state IN ('expired','failed')
        AND staging.created_at<clock_timestamp()-interval '24 hours'",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "DELETE FROM bid_matching_staging_sets WHERE state IN ('expired','failed')
      AND created_at<clock_timestamp()-interval '24 hours'",
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(rows.len() as u64 + expired)
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
    if context.actor.starts_with("system:") {
        return Err(protocol("HUMAN_ACTOR_REQUIRED"));
    }
    let mut tx = pool.begin().await?;
    let replay = idempotency_begin(&mut tx, context, "bid.matching.route_pick.replace").await?;
    if let Some(bytes) = replay {
        return serde_json::from_slice(&bytes).map_err(|_| protocol("invalid pick receipt"));
    }
    let project = sqlx::query("SELECT status FROM bid_projects WHERE id=$1 FOR UPDATE")
        .bind(input.project_id)
        .fetch_one(&mut *tx)
        .await?;
    if project.get::<String, _>("status") != "open" {
        return Err(protocol("bid project is not open"));
    }
    let report=sqlx::query("SELECT report.*,route.unit_id,route.route_kind FROM bid_current_matching_reports current_value
      JOIN bid_matching_reports report ON report.id=current_value.report_id JOIN bid_matching_routes route ON route.id=report.route_id
      JOIN bid_projects project ON project.id=current_value.project_id
      WHERE current_value.project_id=$1 AND current_value.route_id=$2 AND report.id=$3
        AND project.status='open' AND report.mutation_watermark=project.matching_mutation_watermark
        AND report.generation=(SELECT max(generation) FROM bid_matching_manifests WHERE project_id=$1)
      FOR UPDATE OF current_value")
      .bind(input.project_id).bind(input.route_id).bind(input.source_report_artifact_id).fetch_optional(&mut *tx).await?
      .ok_or_else(||protocol("current matching report mismatch"))?;
    if report.get::<String, _>("route_kind") != "technical" {
        return Err(protocol("ROUTE_PICK_REQUIRES_TECHNICAL_REPORT"));
    }
    if report.get::<String, _>("content_sha256") != input.report_sha256 {
        return Err(protocol("REPORT_SHA256_MISMATCH"));
    }
    let current_revision:Option<i64>=sqlx::query_scalar("SELECT revision FROM bid_current_route_pick_sets WHERE project_id=$1 AND route_id=$2 FOR UPDATE")
      .bind(input.project_id).bind(input.route_id).fetch_optional(&mut *tx).await?;
    if current_revision.unwrap_or(0) != input.expected_revision {
        return Err(protocol("ROUTE_PICK_REVISION_MISMATCH"));
    }
    let before_digest =
        current_route_pick_digest(&mut tx, input.project_id, input.route_id).await?;
    let mut selections = input.selections;
    selections.sort_by_key(|row| (row.requirement_artifact_id, row.candidate_artifact_id));
    selections.dedup_by_key(|row| (row.requirement_artifact_id, row.candidate_artifact_id));
    let ids: Vec<Uuid> = selections
        .iter()
        .map(|row| row.candidate_artifact_id)
        .collect();
    let candidates = if ids.is_empty() {
        Vec::new()
    } else {
        sqlx::query("SELECT candidate.id,candidate.requirement_artifact_id,product.product_id,product.product_version_id
      FROM bid_matching_candidate_artifacts candidate JOIN bid_matching_product_version_artifacts product ON product.id=candidate.product_version_artifact_id
      WHERE candidate.report_id=$1 AND candidate.support='supported' AND candidate.id=ANY($2::uuid[]) ORDER BY candidate.requirement_artifact_id,candidate.id")
      .bind(input.source_report_artifact_id).bind(&ids).fetch_all(&mut *tx).await?
    };
    if candidates.len() != selections.len()
        || candidates.iter().zip(&selections).any(|(row, selected)| {
            row.get::<Uuid, _>("id") != selected.candidate_artifact_id
                || row.get::<Uuid, _>("requirement_artifact_id") != selected.requirement_artifact_id
        })
    {
        return Err(protocol("pick item is not a visible supported candidate"));
    }
    let selected_at: chrono::DateTime<chrono::Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut *tx)
        .await?;
    let revision = input.expected_revision + 1;
    let pick_set_id = Uuid::new_v4();
    let unit_id: Option<Uuid> = report.get("unit_id");
    let items:Vec<serde_json::Value>=candidates.iter().zip(&selections).map(|(row,selection)|serde_json::json!({
      "requirement_artifact_id":selection.requirement_artifact_id,"candidate_artifact_id":selection.candidate_artifact_id,
      "product_id":row.get::<Option<Uuid>,_>("product_id"),"product_version_id":row.get::<Uuid,_>("product_version_id"),
      "source_report_artifact_id":input.source_report_artifact_id,"unit_id":unit_id,"selected_by":context.actor,
      "selected_at":selected_at.to_rfc3339_opts(chrono::SecondsFormat::Micros,true)
    })).collect();
    let payload = serde_json::json!({"schema_version":1,"project_id":input.project_id,"route_id":input.route_id,
      "source_report_artifact_id":input.source_report_artifact_id,"report_generation":report.get::<i64,_>("generation"),
      "report_sha256":input.report_sha256,"route_unit_id":unit_id,"revision":revision,"items":items});
    let bytes = serde_json::to_vec(&payload).unwrap();
    let digest = sha256_hex(&bytes);
    sqlx::query("INSERT INTO bid_route_pick_set_artifacts(id,project_id,route_id,source_report_artifact_id,report_generation,
      report_sha256,route_unit_id,revision,canonical_payload,content_sha256,selected_by,selected_at)
      VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)")
      .bind(pick_set_id).bind(input.project_id).bind(input.route_id).bind(input.source_report_artifact_id)
      .bind(report.get::<i64,_>("generation")).bind(&input.report_sha256).bind(unit_id).bind(revision).bind(&bytes).bind(&digest)
      .bind(&context.actor).bind(selected_at).execute(&mut *tx).await?;
    for (ordinal, (row, selection)) in candidates.iter().zip(&selections).enumerate() {
        sqlx::query("INSERT INTO bid_route_pick_set_items(pick_set_id,ordinal,requirement_artifact_id,candidate_artifact_id,
       product_id,product_version_id,source_report_artifact_id,unit_id,selected_by,selected_at)
       VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
       .bind(pick_set_id).bind(ordinal as i32).bind(selection.requirement_artifact_id).bind(selection.candidate_artifact_id)
       .bind(row.get::<Option<Uuid>,_>("product_id")).bind(row.get::<Uuid,_>("product_version_id"))
       .bind(input.source_report_artifact_id).bind(unit_id).bind(&context.actor).bind(selected_at).execute(&mut *tx).await?;
    }
    sqlx::query("INSERT INTO bid_current_route_pick_sets(project_id,route_id,pick_set_id,revision) VALUES($1,$2,$3,$4)
      ON CONFLICT(project_id,route_id) DO UPDATE SET pick_set_id=EXCLUDED.pick_set_id,revision=EXCLUDED.revision")
      .bind(input.project_id).bind(input.route_id).bind(pick_set_id).bind(revision).execute(&mut *tx).await?;
    let project_pick = rebuild_project_pick_set(&mut tx, input.project_id, &context.actor).await?;
    let part_key = match unit_id {
        Some(id) if id.is_nil() => "2:unsectioned".into(),
        Some(id) => format!("2:{id}"),
        None => "4".into(),
    };
    sqlx::query("UPDATE bid_current_parts SET stale=true,stale_reason_codes=(SELECT ARRAY(SELECT DISTINCT code FROM unnest(stale_reason_codes||ARRAY['MATCHING_PICK_CHANGED']) code ORDER BY code))
      WHERE project_id=$1 AND (part_key=$2 OR part_key IN ('3','5','6:implementation_plan'))")
      .bind(input.project_id).bind(part_key).execute(&mut *tx).await?;
    let receipt = PickSetReceiptV1 {
        route_pick_set_id: pick_set_id,
        route_revision: revision,
        route_sha256: digest,
        project_pick_set_id: project_pick.0,
        project_revision: project_pick.1,
        project_sha256: project_pick.2,
    };
    let response = serde_json::to_vec(&receipt).unwrap();
    sqlx::query("INSERT INTO audit_events(id,schema_version,operation,actor_identity,idempotency_key,request_sha256,response_sha256,
      entity_kind,entity_locator,before_revision,before_sha256,after_revision,after_sha256)
      VALUES($1,1,'bid.matching.route_pick.replace',$2,$3,$4,$5,'bid_route_pick_set',$6,$7,$8,$9,$10)")
      .bind(Uuid::new_v4()).bind(&context.actor).bind(&context.idempotency_key).bind(&context.request.sha256).bind(sha256_hex(&response))
      .bind(serde_json::json!({"project_id":input.project_id,"route_id":input.route_id}))
      .bind((input.expected_revision>0).then_some(input.expected_revision))
      .bind(before_digest).bind(revision).bind(&receipt.route_sha256).execute(&mut *tx).await?;
    idempotency_complete(
        &mut tx,
        context,
        "bid.matching.route_pick.replace",
        200,
        &response,
    )
    .await?;
    tx.commit().await?;
    Ok(receipt)
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

async fn current_route_pick_digest(
    tx: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    route_id: Uuid,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT artifact.content_sha256 FROM bid_current_route_pick_sets current_value JOIN bid_route_pick_set_artifacts artifact ON artifact.id=current_value.pick_set_id WHERE current_value.project_id=$1 AND current_value.route_id=$2")
 .bind(project_id).bind(route_id).fetch_optional(&mut **tx).await
}

async fn idempotency_begin(
    tx: &mut Transaction<'_, Postgres>,
    context: &crate::bidding::MutationContext,
    operation: &str,
) -> Result<Option<Vec<u8>>, sqlx::Error> {
    let replay: Option<Vec<u8>> =
        sqlx::query_scalar("SELECT kb_bid_idempotency_begin($1,$2,$3,$4,$5)")
            .bind(&context.actor)
            .bind(operation)
            .bind(&context.idempotency_key)
            .bind(&context.request.bytes)
            .bind(&context.request.sha256)
            .fetch_one(&mut **tx)
            .await?;
    Ok(replay)
}
async fn idempotency_complete(
    tx: &mut Transaction<'_, Postgres>,
    context: &crate::bidding::MutationContext,
    operation: &str,
    status: i32,
    response: &[u8],
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT kb_bid_idempotency_complete($1,$2,$3,$4,$5)")
        .bind(&context.actor)
        .bind(operation)
        .bind(&context.idempotency_key)
        .bind(status)
        .bind(response)
        .execute(&mut **tx)
        .await?;
    Ok(())
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
fn json_i32(value: &serde_json::Value, key: &str) -> Result<i32, sqlx::Error> {
    value
        .get(key)
        .and_then(|v| v.as_u64())
        .and_then(|v| i32::try_from(v).ok())
        .ok_or_else(|| protocol(format!("{key} missing")))
}
fn json_str<'a>(value: &'a serde_json::Value, key: &str) -> Result<&'a str, sqlx::Error> {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| protocol(format!("{key} missing")))
}
fn strict_sorted_unique(values: &[String]) -> bool {
    values.windows(2).all(|w| w[0] < w[1])
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
    fn decision_aggregation_uses_contract_priority_and_comparator() {
        let requirement = Uuid::new_v4();
        let later = StagedCandidateV1 {
            id: Uuid::from_u128(2),
            requirement_artifact_id: requirement,
            product_version_artifact_id: Uuid::new_v4(),
            route_product_ordinal: 2,
            retrieval_rank: 1,
            retrieval_raw_score: "1.000000".into(),
            candidate_identity_sha256: "b".repeat(64),
            evidence_v1_sha256: "b".repeat(64),
            support: "supported".into(),
            business_value_status: "not_scored".into(),
            business_value: None,
            recommended: false,
        };
        let mut earlier = later.clone();
        earlier.id = Uuid::from_u128(1);
        earlier.route_product_ordinal = 1;
        earlier.candidate_identity_sha256 = "a".repeat(64);
        earlier.evidence_v1_sha256 = "a".repeat(64);
        let mut unresolved = later.clone();
        unresolved.id = Uuid::from_u128(3);
        unresolved.route_product_ordinal = 0;
        unresolved.support = "unresolved".into();
        let decision = aggregate_decision(&[&later, &unresolved, &earlier]);
        assert_eq!(decision.0, "supported");
        assert_eq!(decision.4, Some(earlier.id));
    }
}
