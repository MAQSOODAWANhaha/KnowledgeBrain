//! Bid project extract, match jobs, quote, and submission.

pub mod matching;
pub mod quote;
mod render;
pub mod submission;
pub mod tender;

pub use render::{
    ManifestRenderAsset, ManifestRenderAssetLocator, render_manifest_document,
    renderer_contract_identity, validate_manifest_render_assets,
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectView {
    pub id: Uuid,
    pub title: String,
    pub owner_user_id: Uuid,
    pub ends_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub status: String,
    pub ended_at: Option<DateTime<Utc>>,
    pub fact_revision: i64,
    pub fact_sha256: String,
    pub ceiling_basis: String,
    pub ceiling_revision: i64,
    pub ceiling_identity_sha256: String,
}

pub fn unsectioned_unit() -> Uuid {
    Uuid::nil()
}

pub fn technical_part_key(unit: Uuid) -> String {
    if unit == unsectioned_unit() {
        "2:unsectioned".into()
    } else {
        format!("2:{unit}")
    }
}

pub fn stale_keys_after_pick(unit: Uuid) -> Vec<String> {
    vec![
        technical_part_key(unit),
        "3".into(),
        "5".into(),
        "6:implementation_plan".into(),
    ]
}

#[derive(Debug, Clone, Serialize)]
pub struct MatchUnitView {
    pub kind: String,
    pub id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route_id: Option<Uuid>,
    pub heading_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub technical_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prev_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extract_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publication_generation: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<Vec<String>>,
}

pub fn expected_part_keys_from(units: &[MatchUnitView], confirmed_units: &[Uuid]) -> Vec<String> {
    let routes: Vec<submission::TechnicalRouteRef> = units
        .iter()
        .filter(|unit| unit.kind == "technical" || unit.kind == "unsectioned")
        .filter(|unit| {
            let id = unit.id.unwrap_or_else(unsectioned_unit);
            confirmed_units.contains(&id) || unit.kind == "unsectioned"
        })
        .map(|unit| submission::TechnicalRouteRef {
            unit_id: Some(unit.id.unwrap_or_else(unsectioned_unit)),
        })
        .collect();
    submission::required_part_keys(&routes)
}

fn clause_unit_id(clause: &storage::bidding::Clause) -> Uuid {
    clause
        .current_source_span_v2
        .as_ref()
        .and_then(|value| value.get("section_artifact_id"))
        .and_then(|value| value.as_str())
        .and_then(|value| Uuid::parse_str(value).ok())
        .unwrap_or_else(Uuid::nil)
}

pub async fn list_match_units(
    pool: &sqlx::PgPool,
    project_id: Uuid,
) -> Result<Vec<MatchUnitView>, String> {
    let routes = storage::bid_submission::current_routes(pool, project_id)
        .await
        .map_err(|e| e.to_string())?;
    let clauses = storage::bidding::list_clauses(pool, project_id, false)
        .await
        .map_err(|e| e.to_string())?;
    let commercial_route_id = routes.iter().find_map(|route| {
        (route.get("route_kind").and_then(serde_json::Value::as_str) == Some("commercial"))
            .then(|| {
                route
                    .get("route_id")
                    .and_then(|value| value.as_str())
                    .and_then(|value| Uuid::parse_str(value).ok())
            })
            .flatten()
    });
    let mut out = vec![MatchUnitView {
        kind: "commercial".into(),
        id: None,
        route_id: commercial_route_id,
        heading_path: "商务".into(),
        technical_count: Some(
            clauses
                .iter()
                .filter(|clause| {
                    clause.family.as_deref() == Some("commercial") && clause.status == "confirmed"
                })
                .count(),
        ),
        prev_id: None,
        extract_status: None,
        error_message: None,
        retry_status: None,
        publication_generation: None,
        stale: None,
        removed: None,
        quality: None,
        degraded: None,
        reason: None,
    }];
    let mut prev = None;
    for route in routes {
        if route.get("route_kind").and_then(serde_json::Value::as_str) != Some("technical") {
            continue;
        }
        let route_id = route
            .get("route_id")
            .and_then(|value| value.as_str())
            .and_then(|value| Uuid::parse_str(value).ok());
        let unit_id = route
            .get("unit_id")
            .and_then(|value| value.as_str())
            .and_then(|value| Uuid::parse_str(value).ok())
            .unwrap_or_else(unsectioned_unit);
        let kind = if unit_id.is_nil() {
            "unsectioned"
        } else {
            "technical"
        };
        out.push(MatchUnitView {
            kind: kind.into(),
            id: Some(unit_id),
            route_id,
            heading_path: if unit_id.is_nil() {
                "未归段".into()
            } else {
                unit_id.to_string()
            },
            technical_count: Some(
                clauses
                    .iter()
                    .filter(|clause| {
                        clause.kind == "technical"
                            && clause.status == "confirmed"
                            && clause_unit_id(clause) == unit_id
                    })
                    .count(),
            ),
            prev_id: prev,
            extract_status: None,
            error_message: None,
            retry_status: None,
            publication_generation: route.get("generation").and_then(serde_json::Value::as_i64),
            stale: None,
            removed: None,
            quality: None,
            degraded: None,
            reason: None,
        });
        prev = Some(unit_id);
    }
    Ok(out)
}

pub fn conversion_is_thin(markdown: &str) -> bool {
    let text = markdown.trim();
    let tables = has_gfm_table(markdown);
    if text.chars().count() < 200 && !tables {
        return true;
    }
    let headings = markdown
        .lines()
        .filter(|line| line.trim_start().starts_with('#'))
        .count();
    headings == 0 && !tables
}

fn has_gfm_table(markdown: &str) -> bool {
    markdown.lines().any(|line| {
        let t = line.trim();
        t.starts_with('|') && t.matches('|').count() >= 2
    })
}

/// Office source with long prose but no GFM tables — likely flattened.
pub fn conversion_tables_flat(markdown: &str, file_name: &str) -> bool {
    let ext = file_name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if !matches!(
        ext.as_str(),
        "docx" | "doc" | "docm" | "xlsx" | "xls" | "xlsm"
    ) {
        return false;
    }
    if conversion_is_thin(markdown) {
        return false;
    }
    !has_gfm_table(markdown)
}

pub fn conversion_quality_note(markdown: &str, file_name: &str) -> &'static str {
    if conversion_is_thin(markdown) {
        "conversion_quality=thin"
    } else if conversion_tables_flat(markdown, file_name) {
        "conversion_quality=tables_flat"
    } else {
        ""
    }
}

/// Transient transport/service failures may retry; parse/format errors must not
/// sit in `pending` or the files UI looks like the job is still queued.
pub fn conversion_error_is_retryable(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    if error.contains("failed to parse")
        || error.contains("cannot parse")
        || error.contains("invalid utf-8")
        || error.contains("simple reader cannot parse")
        || error.contains("vlm not configured")
    {
        return false;
    }
    error.contains("timeout")
        || error.contains("timed out")
        || error.contains("unavailable")
        || error.contains("connection")
        || error.contains("reset")
        || error.contains("temporarily")
}

pub fn matching_schedule_environment() -> storage::bid_matching::ScheduleEnvironment {
    let profile = std::env::var("KNOWLEDGEBRAIN_RUNTIME_PROFILE")
        .or_else(|_| std::env::var("KNOWLEDGEBRAIN_PROFILE"))
        .unwrap_or_else(|_| "development".into());
    let environment = match profile.to_ascii_lowercase().as_str() {
        "prod" | "production" => "production",
        "test" => "test",
        _ => "development",
    }
    .to_string();
    storage::bid_matching::ScheduleEnvironment {
        environment,
        max_attempts: 3,
    }
}

pub async fn schedule_dirty_and_enqueue(
    pool: &sqlx::PgPool,
    project_id: Uuid,
    context: storage::bid_matching::ScheduleMutationContext,
) -> Result<Option<Uuid>, String> {
    let receipt = storage::bid_matching::schedule_dirty_project(
        pool,
        project_id,
        matching_schedule_environment(),
        &context,
    )
    .await
    .map_err(|e| e.to_string())?;
    let Some(receipt) = receipt else {
        return Ok(None);
    };
    let mut last = None;
    for job in receipt.jobs {
        let _ = runtime::enqueue_bid_match_route_v1(
            job.id,
            runtime::BidMatchRouteV1Snapshots {
                config_snapshot_id: job.snapshots.config_snapshot_id,
                feature_snapshot_id: job.snapshots.feature_snapshot_id,
                score_policy_snapshot_id: job.snapshots.score_policy_snapshot_id,
                verifier_policy_snapshot_id: job.snapshots.verifier_policy_snapshot_id,
            },
            None,
        )
        .await;
        last = Some(job.id);
    }
    Ok(last)
}

pub async fn maybe_rematch_company_doc(
    pool: &sqlx::PgPool,
    _document_id: Uuid,
) -> Result<(), String> {
    let projects = storage::bid_submission::dirty_match_projects(pool)
        .await
        .map_err(|error| error.to_string())?;
    for project_id in projects {
        let _ = schedule_dirty_and_enqueue(
            pool,
            project_id,
            storage::bid_matching::ScheduleMutationContext::system(),
        )
        .await;
    }
    Ok(())
}

pub async fn enqueue_pending_route_jobs(pool: &sqlx::PgPool) -> Result<usize, String> {
    let jobs = storage::bid_matching::pending_route_envelopes(pool)
        .await
        .map_err(|e| e.to_string())?;
    for job in &jobs {
        let _ = runtime::enqueue_bid_match_route_v1(
            job.job_id,
            runtime::BidMatchRouteV1Snapshots {
                config_snapshot_id: job.snapshots.config_snapshot_id,
                feature_snapshot_id: job.snapshots.feature_snapshot_id,
                score_policy_snapshot_id: job.snapshots.score_policy_snapshot_id,
                verifier_policy_snapshot_id: job.snapshots.verifier_policy_snapshot_id,
            },
            None,
        )
        .await;
    }
    Ok(jobs.len())
}

pub fn derived_status(
    files: i64,
    ready: i64,
    drafts: i64,
    picks: i64,
    pending_files: i64,
    extract_running: bool,
    match_running: bool,
) -> serde_json::Value {
    json!({
        "has_files": files > 0,
        "files_ready": files > 0 && files == ready,
        "extract_running": extract_running,
        "unconfirmed_drafts": drafts,
        "match_running": match_running,
        "has_picks": picks > 0,
        "files_not_in_clauses": pending_files
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversion_quality_flags_short_markdown() {
        assert!(conversion_is_thin("hi"));
        let long_no_heading = "招标正文。".repeat(80);
        assert!(conversion_is_thin(&long_no_heading));
        let long = format!("# 标题\n{}", "招标正文。".repeat(80));
        assert!(!conversion_is_thin(&long));
        assert!(conversion_tables_flat(&long, "a.docx"));
        assert!(!conversion_tables_flat(&long, "a.pdf"));
        let table = "# 标题\n\n| 条款 | 内容 |\n| --- | --- |\n| 电源 | 双路 |\n";
        assert!(!conversion_is_thin(table));
        assert!(!conversion_tables_flat(table, "a.docx"));
        assert_eq!(conversion_quality_note(table, "a.docx"), "");
        assert_eq!(
            conversion_quality_note(&long, "spec.docx"),
            "conversion_quality=tables_flat"
        );
    }

    #[test]
    fn parse_failures_are_terminal_conversion_errors() {
        assert!(!conversion_error_is_retryable(
            "Failed to parse: BiddingFile.doc"
        ));
        assert!(!conversion_error_is_retryable(
            "simple reader cannot parse .doc"
        ));
        assert!(conversion_error_is_retryable(
            "docreader connection timed out"
        ));
        assert!(!conversion_error_is_retryable(
            "vlm not configured: images require a vision model"
        ));
    }

    #[test]
    fn pick_marks_cover_and_deviate_parts() {
        let unit = Uuid::from_u128(9);
        let keys = stale_keys_after_pick(unit);
        assert!(!keys.contains(&"1".into()));
        assert!(keys.contains(&"3".into()));
        assert!(keys.contains(&"5".into()));
        assert!(keys.contains(&"6:implementation_plan".into()));
        assert!(keys.contains(&format!("2:{unit}")));
    }

    #[test]
    fn part_keys_follow_sidebar_not_uuid() {
        let late = Uuid::from_u128(0xffff);
        let early = Uuid::from_u128(1);
        let units = vec![
            MatchUnitView {
                kind: "commercial".into(),
                id: None,
                route_id: None,
                heading_path: "商务".into(),
                technical_count: None,
                prev_id: None,
                extract_status: None,
                error_message: None,
                retry_status: None,
                publication_generation: None,
                stale: None,
                removed: None,
                quality: None,
                degraded: None,
                reason: None,
            },
            MatchUnitView {
                kind: "technical".into(),
                id: Some(late),
                route_id: None,
                heading_path: "交换机".into(),
                technical_count: Some(1),
                prev_id: None,
                extract_status: None,
                error_message: None,
                retry_status: None,
                publication_generation: None,
                stale: None,
                removed: None,
                quality: None,
                degraded: None,
                reason: None,
            },
            MatchUnitView {
                kind: "technical".into(),
                id: Some(early),
                route_id: None,
                heading_path: "存储".into(),
                technical_count: Some(1),
                prev_id: Some(late),
                extract_status: None,
                error_message: None,
                retry_status: None,
                publication_generation: None,
                stale: None,
                removed: None,
                quality: None,
                degraded: None,
                reason: None,
            },
            MatchUnitView {
                kind: "unsectioned".into(),
                id: Some(Uuid::nil()),
                route_id: None,
                heading_path: "未归段".into(),
                technical_count: Some(0),
                prev_id: None,
                extract_status: None,
                error_message: None,
                retry_status: None,
                publication_generation: None,
                stale: None,
                removed: None,
                quality: None,
                degraded: None,
                reason: None,
            },
        ];
        let keys = expected_part_keys_from(&units, &[late, early]);
        assert_eq!(
            keys,
            vec![
                "1".into(),
                format!("2:{early}"),
                format!("2:{late}"),
                "2:unsectioned".into(),
                "3".into(),
                "4".into(),
                "5".into(),
                "6:letter".into(),
                "6:authorization".into(),
                "6:quote".into(),
                "6:implementation_plan".into(),
                "6:procedural".into(),
            ]
        );
        let none = expected_part_keys_from(&units, &[]);
        assert!(none.contains(&"1".into()));
        assert!(none.contains(&"6:quote".into()));
        assert!(none.contains(&"2:unsectioned".into()));
    }

    #[test]
    fn match_unit_publication_metadata_serializes_only_when_present() {
        let mut unit = MatchUnitView {
            kind: "commercial".into(),
            id: None,
            route_id: None,
            heading_path: "商务".into(),
            technical_count: None,
            prev_id: None,
            extract_status: None,
            error_message: None,
            retry_status: None,
            publication_generation: None,
            stale: None,
            removed: None,
            quality: None,
            degraded: None,
            reason: None,
        };
        let legacy = serde_json::to_value(&unit).unwrap();
        for field in [
            "publication_generation",
            "stale",
            "removed",
            "quality",
            "degraded",
            "reason",
        ] {
            assert!(legacy.get(field).is_none(), "legacy unit exposed {field}");
        }
        unit.kind = "technical".into();
        unit.publication_generation = Some(7);
        unit.stale = Some(true);
        unit.removed = Some(false);
        unit.quality = Some("review".into());
        unit.degraded = Some(true);
        unit.reason = Some(vec!["QUALITY_REVIEW".into()]);
        let modern = serde_json::to_value(&unit).unwrap();
        assert_eq!(modern["publication_generation"], 7);
        assert_eq!(modern["stale"], true);
        assert_eq!(modern["removed"], false);
        assert_eq!(modern["quality"], "review");
        assert_eq!(modern["degraded"], true);
        assert_eq!(modern["reason"], serde_json::json!(["QUALITY_REVIEW"]));
    }
}
