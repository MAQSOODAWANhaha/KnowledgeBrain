//! Bid project extract, match jobs, coverage, and preview.

mod booklet;
mod export;
pub mod extraction;
pub mod matching;
pub mod tender;

pub use booklet::{BookletPartView, ensure_all_parts, ensure_part, save_part};
pub use export::{
    ExportDoc, ExportKind, build_export_docx, build_export_pdf, export_project, export_project_opts,
};

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectView {
    pub id: Uuid,
    pub title: String,
    pub owner_name: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub status: String,
    pub ended_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClauseView {
    pub id: Uuid,
    pub text: String,
    pub raw_text: String,
    pub family: String,
    pub must: bool,
    pub status: String,
    #[serde(default)]
    pub family_conflict: bool,
    pub deviate: bool,
    pub deviate_note: String,
    #[serde(default)]
    pub section_id: Option<Uuid>,
    #[serde(default)]
    pub assessment: String,
    #[serde(default)]
    pub unit_id: Uuid,
    #[serde(default)]
    pub suggestion: String,
    #[serde(default)]
    pub hit_outcome: String,
    #[serde(default)]
    pub hit_file: String,
}

pub fn unsectioned_unit() -> Uuid {
    Uuid::nil()
}

pub fn resolve_unit(
    section_id: Option<Uuid>,
    merge: &std::collections::HashMap<Uuid, Option<Uuid>>,
) -> Uuid {
    let Some(mut cur) = section_id else {
        return unsectioned_unit();
    };
    let mut seen = std::collections::HashSet::new();
    loop {
        if !seen.insert(cur) {
            return cur;
        }
        match merge.get(&cur).copied().flatten() {
            Some(next) => cur = next,
            None => return cur,
        }
    }
}

pub fn clause_from_row(
    r: &sqlx::postgres::PgRow,
    merge: &std::collections::HashMap<Uuid, Option<Uuid>>,
) -> ClauseView {
    let section_id = r.try_get::<Option<Uuid>, _>("section_id").ok().flatten();
    let assessment = r
        .try_get::<String, _>("assessment")
        .unwrap_or_else(|_| "unset".into());
    let deviate = r.get::<bool, _>("deviate") || assessment == "deviate";
    ClauseView {
        id: r.get("id"),
        text: r.get("text"),
        raw_text: r.try_get("raw_text").unwrap_or_default(),
        family: r.get("family"),
        must: r.get("must"),
        status: r.get("status"),
        family_conflict: r.try_get("family_conflict").unwrap_or(false),
        deviate,
        deviate_note: r.try_get("deviate_note").unwrap_or_default(),
        section_id,
        assessment,
        unit_id: resolve_unit(section_id, merge),
        suggestion: String::new(),
        hit_outcome: String::new(),
        hit_file: String::new(),
    }
}

pub fn booklet_key_for_unit(unit: Uuid) -> String {
    if unit == unsectioned_unit() {
        "2:unsectioned".into()
    } else {
        format!("2:{unit}")
    }
}

pub fn meet_blocked_by_suggestion(suggestion: &str) -> bool {
    suggestion == "unmet"
}

pub fn stale_keys_after_pick(unit: Uuid) -> Vec<String> {
    vec!["1".into(), booklet_key_for_unit(unit), "3".into()]
}

#[derive(Debug, Clone, Serialize)]
pub struct MatchUnitView {
    pub kind: String,
    pub id: Option<Uuid>,
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
    let mut keys = vec!["1".into()];
    for u in units {
        if u.kind != "technical" && u.kind != "unsectioned" {
            continue;
        }
        let id = u.id.unwrap_or_else(unsectioned_unit);
        if confirmed_units.contains(&id) {
            keys.push(booklet_key_for_unit(id));
        }
    }
    keys.extend(["3".into(), "4".into(), "5".into()]);
    keys
}

pub async fn list_match_units(
    pool: &sqlx::PgPool,
    project_id: Uuid,
) -> Result<Vec<MatchUnitView>, String> {
    let merge = section_merge_map(pool, project_id).await?;
    let sections = storage::bid::list_sections(pool, project_id)
        .await
        .map_err(|e| e.to_string())?;
    let clauses: Vec<ClauseView> = storage::bid::list_clauses(pool, project_id, false)
        .await
        .map_err(|e| e.to_string())?
        .iter()
        .map(|r| clause_from_row(r, &merge))
        .collect();
    let mut out = vec![MatchUnitView {
        kind: "commercial".into(),
        id: None,
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
    }];
    let mut prev: Option<Uuid> = None;
    for s in sections {
        let sid: Uuid = s.get("id");
        if merge.get(&sid).copied().flatten().is_some() {
            continue;
        }
        let n = clauses
            .iter()
            .filter(|c| c.family == "technical" && c.unit_id == sid)
            .count();
        if n == 0 {
            continue;
        }
        out.push(MatchUnitView {
            kind: "technical".into(),
            id: Some(sid),
            heading_path: s.get("heading_path"),
            technical_count: Some(n),
            prev_id: prev,
            extract_status: Some(s.get("extract_status")),
            error_message: Some(s.get("error_message")),
            retry_status: Some(s.get("retry_status")),
            publication_generation: s.try_get("published_extraction_generation").ok().flatten(),
            stale: s.try_get("publication_stale").ok().flatten(),
            removed: s.try_get("publication_removed").ok().flatten(),
            quality: s.try_get("publication_quality_status").ok().flatten(),
            degraded: s.try_get("publication_degraded").ok().flatten(),
            reason: s.try_get("publication_reason_codes").ok().flatten(),
        });
        prev = Some(sid);
    }
    out.push(MatchUnitView {
        kind: "unsectioned".into(),
        id: Some(unsectioned_unit()),
        heading_path: "未归段".into(),
        technical_count: Some(
            clauses
                .iter()
                .filter(|c| c.family == "technical" && c.unit_id == unsectioned_unit())
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
    });
    Ok(out)
}

pub async fn decorate_clauses(
    pool: &sqlx::PgPool,
    project_id: Uuid,
    clauses: &mut [ClauseView],
) -> Result<(), String> {
    let picks = visible_pick_json(pool, project_id, None).await?;
    let cov = coverage_for(clauses, &picks);
    let cmap: std::collections::HashMap<Uuid, String> =
        cov.into_iter().map(|r| (r.clause_id, r.status)).collect();
    let hits = visible_commercial_json(pool, project_id).await?;
    let commercial_cov = coverage_for_commercial(clauses, &hits);
    let mut hmap = std::collections::HashMap::new();
    for hit in &hits {
        let Some(cid) = preview_clause_id(hit) else {
            continue;
        };
        let outcome = commercial_cov
            .iter()
            .find(|row| row.clause_id == cid)
            .map(|row| row.status.clone())
            .unwrap_or_else(|| {
                hit.get("outcome")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .to_string()
            });
        let file = hit
            .get("file_name")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();
        hmap.insert(cid, (outcome, file));
    }
    for c in clauses.iter_mut() {
        if c.family == "technical" && c.status == "confirmed" {
            c.suggestion = cmap.get(&c.id).cloned().unwrap_or_default();
        }
        if c.family == "commercial"
            && let Some((outcome, file)) = hmap.get(&c.id)
        {
            c.hit_outcome = outcome.clone();
            c.hit_file = file.clone();
        }
    }
    Ok(())
}

pub async fn visible_pick_json(
    pool: &sqlx::PgPool,
    project_id: Uuid,
    unit_id: Option<Uuid>,
) -> Result<Vec<serde_json::Value>, String> {
    let rows = storage::bid_matching::visible_picks(pool, project_id, unit_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(rows
        .iter()
        .map(|row| {
            json!({
                "product_id": row.try_get::<Uuid, _>("product_id").ok(),
                "unit_id": row.try_get::<Uuid, _>("unit_id").ok().unwrap_or_else(Uuid::nil),
                "version_id": row.try_get::<Uuid, _>("product_version_id").ok()
                    .or_else(|| row.try_get::<Uuid, _>("version_id").ok()),
                "score": row.try_get::<Decimal, _>("score_value").ok()
                    .map(|value| value.to_string()),
                "coverage": row.try_get::<i32, _>("coverage_supported").ok(),
                "clauses": row.try_get::<serde_json::Value, _>("clauses").ok()
                    .unwrap_or_else(|| json!([])),
            })
        })
        .collect())
}

pub async fn visible_commercial_json(
    pool: &sqlx::PgPool,
    project_id: Uuid,
) -> Result<Vec<serde_json::Value>, String> {
    let rows = storage::bid_matching::current_commercial_decisions(pool, project_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(rows
        .iter()
        .filter_map(|row| {
            let decision = row.try_get::<String, _>("system_decision").ok()?;
            let outcome = match decision.as_str() {
                "select" => "hit",
                "reject" => "miss",
                "review" => "review",
                _ => return None,
            };
            Some(json!({
                "clause_id": row.try_get::<Uuid, _>("source_clause_id").ok(),
                "outcome": outcome,
                "file_name": row.try_get::<Option<String>, _>("file_name").ok().flatten(),
            }))
        })
        .collect())
}

pub async fn visible_technical_candidates_json(
    pool: &sqlx::PgPool,
    project_id: Uuid,
    unit_id: Uuid,
) -> Result<Vec<serde_json::Value>, String> {
    let rows = storage::bid_matching::current_technical_candidates(pool, project_id, unit_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(rows
        .iter()
        .map(|row| {
            json!({
                "product_id": row.try_get::<Uuid, _>("product_id").ok(),
                "product_version_id": row.try_get::<Uuid, _>("product_version_id").ok(),
                "system_decision": row.try_get::<String, _>("system_decision").ok(),
                "quality_status": row.try_get::<String, _>("quality_status").ok(),
            })
        })
        .collect())
}

pub async fn section_merge_map(
    pool: &sqlx::PgPool,
    project_id: Uuid,
) -> Result<std::collections::HashMap<Uuid, Option<Uuid>>, String> {
    let rows = storage::bid::list_sections(pool, project_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(rows
        .iter()
        .map(|r| {
            (
                r.get::<Uuid, _>("id"),
                r.try_get::<Option<Uuid>, _>("merge_into").ok().flatten(),
            )
        })
        .collect())
}

#[derive(Debug, Clone, Serialize)]
pub struct CoverageRow {
    pub clause_id: Uuid,
    pub status: String,
}

pub fn coverage_for(clauses: &[ClauseView], picks: &[serde_json::Value]) -> Vec<CoverageRow> {
    let tech: Vec<_> = clauses
        .iter()
        .filter(|c| c.family == "technical" && c.status == "confirmed")
        .collect();
    tech.iter()
        .map(|c| {
            let unit_picks: Vec<_> = picks
                .iter()
                .filter(|p| {
                    p.get("unit_id")
                        .and_then(|x| x.as_str())
                        .and_then(|s| Uuid::parse_str(s).ok())
                        == Some(c.unit_id)
                })
                .collect();
            if unit_picks.is_empty() {
                return CoverageRow {
                    clause_id: c.id,
                    status: "pending".into(),
                };
            }
            let snaps: Vec<_> = unit_picks
                .iter()
                .filter_map(|p| p.get("clauses").and_then(|x| x.as_array()))
                .flatten()
                .filter(|row| {
                    row.get("clause_id")
                        .and_then(|x| x.as_str())
                        .and_then(|s| Uuid::parse_str(s).ok())
                        == Some(c.id)
                })
                .collect();
            if snaps.is_empty() {
                return CoverageRow {
                    clause_id: c.id,
                    status: "need_rematch".into(),
                };
            }
            let stale = snaps.iter().any(|row| {
                let t = row.get("text").and_then(|x| x.as_str()).unwrap_or("");
                let m = row.get("must").and_then(|x| x.as_bool()).unwrap_or(false);
                t != c.text || m != c.must
            });
            if stale {
                return CoverageRow {
                    clause_id: c.id,
                    status: "need_rematch".into(),
                };
            }
            let any_hit = snaps
                .iter()
                .any(|row| row.get("hit").and_then(|x| x.as_bool()) == Some(true));
            let status = if any_hit {
                "cover"
            } else if c.must {
                "unmet"
            } else {
                "uncovered"
            };
            CoverageRow {
                clause_id: c.id,
                status: status.into(),
            }
        })
        .collect()
}

/// Map a current commercial v1 decision read onto confirmed clauses.
/// Missing current projection yields no rows (hidden/empty), never a fabricated miss.
pub fn coverage_for_commercial(
    clauses: &[ClauseView],
    decisions: &[serde_json::Value],
) -> Vec<CoverageRow> {
    let by_clause: std::collections::HashMap<Uuid, String> = decisions
        .iter()
        .filter_map(|decision| {
            let clause_id = preview_clause_id(decision)?;
            let outcome = decision.get("outcome").and_then(|value| value.as_str())?;
            let status = match outcome {
                "hit" | "select" => "hit",
                "miss" | "reject" => "miss",
                "review" => "review",
                _ => return None,
            };
            Some((clause_id, status.to_string()))
        })
        .collect();
    clauses
        .iter()
        .filter(|clause| clause.family == "commercial" && clause.status == "confirmed")
        .filter_map(|clause| {
            by_clause.get(&clause.id).map(|status| CoverageRow {
                clause_id: clause.id,
                status: status.clone(),
            })
        })
        .collect()
}

pub(crate) fn preview_clause_id(h: &serde_json::Value) -> Option<Uuid> {
    let v = h.get("clause_id")?;
    if let Some(s) = v.as_str() {
        return Uuid::parse_str(s).ok();
    }
    serde_json::from_value(v.clone()).ok()
}

pub fn preview_json(
    project: &ProjectView,
    clauses: &[ClauseView],
    picks: &[serde_json::Value],
    commercial: &[serde_json::Value],
    coverage: &[CoverageRow],
) -> serde_json::Value {
    let cov: std::collections::HashMap<_, _> = coverage
        .iter()
        .map(|c| (c.clause_id, c.status.as_str()))
        .collect();
    let mut s2 = Vec::new();
    for c in clauses
        .iter()
        .filter(|c| c.family == "technical" && c.status == "confirmed")
    {
        let st = *cov.get(&c.id).unwrap_or(&"pending");
        if c.deviate {
            s2.push(json!({
                "clause_id": c.id,
                "text": c.text,
                "response": c.deviate_note,
                "status": "deviate"
            }));
            continue;
        }
        if st == "cover" {
            for p in picks {
                if let Some(rows) = p.get("clauses").and_then(|x| x.as_array()) {
                    for row in rows {
                        if row.get("clause_id").and_then(|x| x.as_str()) == Some(&c.id.to_string())
                            && row.get("hit").and_then(|x| x.as_bool()) == Some(true)
                        {
                            s2.push(json!({
                                "clause_id": c.id,
                                "text": c.text,
                                "product_id": p.get("product_id"),
                                "hits": row.get("hits"),
                                "status": "cover"
                            }));
                        }
                    }
                }
            }
        } else {
            let label = match st {
                "pending" => "待勾选",
                "need_rematch" => "需重新匹配",
                _ => "未覆盖",
            };
            s2.push(json!({
                "clause_id": c.id,
                "text": c.text,
                "response": label,
                "status": st
            }));
        }
    }
    let s3: Vec<_> = coverage
        .iter()
        .filter(|c| c.status == "deviate" || c.status == "unmet")
        .map(|c| json!({"clause_id": c.clause_id, "status": c.status}))
        .collect();
    let confirmed_comm: std::collections::HashSet<Uuid> = clauses
        .iter()
        .filter(|c| c.family == "commercial" && c.status == "confirmed")
        .map(|c| c.id)
        .collect();
    let mut s4 = Vec::new();
    let mut s5 = Vec::new();
    for h in commercial {
        let Some(cid) = preview_clause_id(h) else {
            continue;
        };
        if !confirmed_comm.contains(&cid) {
            continue;
        }
        let outcome = h.get("outcome").and_then(|x| x.as_str()).unwrap_or("");
        if outcome == "hit" {
            s4.push(json!({
                "clause_id": cid.to_string(),
                "file_name": h.get("file_name")
            }));
        } else if (outcome == "miss" || outcome == "review")
            && clauses.iter().any(|c| {
                c.id == cid && c.must && c.family == "commercial" && c.status == "confirmed"
            })
        {
            let status = if outcome == "review" {
                "待复核"
            } else {
                "缺件"
            };
            s5.push(json!({"clause_id": cid.to_string(), "status": status}));
        }
    }
    json!({
        "project_id": project.id,
        "generated_at": Utc::now(),
        "sections": {
            "1": {
                "title": project.title,
                "owner_name": project.owner_name,
                "expires_at": project.expires_at,
                "products": picks.iter().map(|p| p.get("product_id")).collect::<Vec<_>>()
            },
            "2": s2,
            "3": s3,
            "4": s4,
            "5": s5
        }
    })
}

#[tracing::instrument(name = "bid.convert", skip_all, fields(document_id = %document_id))]
pub async fn convert_document(pool: &sqlx::PgPool, document_id: Uuid) -> Result<(), String> {
    let Some((claim_token, name, key, _generation)) =
        storage::bid::claim_document_conversion(pool, document_id)
            .await
            .map_err(|e| e.to_string())?
    else {
        return Ok(());
    };
    tracing::info!(document_id = %document_id, file = %name, "bid_convert start");
    let hash = key.trim_start_matches("objects/");
    let (heartbeat_stop, mut heartbeat_stop_rx) = tokio::sync::oneshot::channel();
    let heartbeat_pool = pool.clone();
    let heartbeat_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(15));
        loop {
            tokio::select! {
                _ = &mut heartbeat_stop_rx => break,
                _ = interval.tick() => {
                    match storage::bid::heartbeat_document_conversion(
                        &heartbeat_pool,
                        document_id,
                        claim_token,
                    ).await {
                        Ok(true) => {}
                        _ => break,
                    }
                }
            }
        }
    });
    let conversion = convert_document_inner(pool, document_id, claim_token, &name, hash).await;
    let _ = heartbeat_stop.send(());
    let _ = heartbeat_task.await;
    match conversion {
        Ok(()) => {
            tracing::info!(document_id = %document_id, file = %name, "bid_convert done");
            Ok(())
        }
        Err(error) => {
            tracing::error!(
                document_id = %document_id,
                file = %name,
                error = %error,
                retryable = conversion_error_is_retryable(&error),
                "bid_convert fail"
            );
            let status = if conversion_error_is_retryable(&error) {
                "pending"
            } else {
                "failed"
            };
            let _ = storage::bid::finish_document_conversion(
                pool,
                document_id,
                claim_token,
                status,
                None,
                &error,
            )
            .await;
            Err(error)
        }
    }
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

async fn convert_document_inner(
    pool: &sqlx::PgPool,
    document_id: Uuid,
    claim_token: Uuid,
    name: &str,
    hash: &str,
) -> Result<(), String> {
    let bytes = storage::read_blob(hash).map_err(|e| e.to_string())?;
    let result = docparser::convert_to_markdown(name, bytes)
        .await
        .map_err(|e| e.0)?;
    if !result.error.is_empty() {
        return Err(result.error);
    }
    tracing::info!(
        document_id = %document_id,
        file = name,
        parser = result.metadata.get("parser").map(String::as_str).unwrap_or("-"),
        images = result.images.len(),
        anydoc_fallback = result.metadata.get("anydoc_fallback").map(String::as_str).unwrap_or("-"),
        "bid_convert parsed"
    );
    if !storage::bid::heartbeat_document_conversion(pool, document_id, claim_token)
        .await
        .map_err(|e| e.to_string())?
    {
        return Err("document conversion lease lost".into());
    }
    let has_images = result.images.iter().any(|img| !img.data.is_empty());
    if has_images && !domain::vlm_configured() {
        return Err("vlm not configured: images require a vision model".into());
    }
    let image_source_type = result
        .metadata
        .get("image_source_type")
        .cloned()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| enrichment::image_source_type(name, &result.markdown).to_string());
    let multimodal_enabled = has_images;
    let multimodal_status = if multimodal_enabled {
        "running"
    } else {
        "skipped"
    };
    if !storage::bid::set_document_multimodal_status(
        pool,
        document_id,
        claim_token,
        multimodal_status,
        "",
    )
    .await
    .map_err(|e| e.to_string())?
    {
        return Err("document conversion lease lost".into());
    }
    let mut md = result.markdown;
    let language = enrichment::infer_output_language(&format!("{name}\n{md}"));
    for img in &result.images {
        if img.data.is_empty() {
            continue;
        }
        let (ihash, ikey) = {
            let h = domain::sha256_hex(&img.data);
            let k = storage::object_ref(&h);
            storage::write_blob_off_runtime(&h, &img.data).map_err(|e| e.to_string())?;
            (h, k)
        };
        let _ = ihash;
        if multimodal_enabled {
            let (ocr, cap) = match enrichment::describe_image(&ikey, &image_source_type, &language)
            {
                Ok(description) => description,
                Err(error) => {
                    let message = format!("tender multimodal stage failed: {error}");
                    let _ = storage::bid::set_document_multimodal_status(
                        pool,
                        document_id,
                        claim_token,
                        "failed",
                        &message,
                    )
                    .await;
                    return Err(message);
                }
            };
            md = md.replacen(
                &format!("]({})", img.original_ref),
                &format!("]({ikey})\n\n{ocr}\n\n{cap}\n"),
                1,
            );
        } else if md.contains(&img.original_ref) {
            md = md.replace(&img.original_ref, &ikey);
        }
        if !storage::bid::heartbeat_document_conversion(pool, document_id, claim_token)
            .await
            .map_err(|e| e.to_string())?
        {
            return Err("document conversion lease lost".into());
        }
    }
    if multimodal_enabled
        && !storage::bid::set_document_multimodal_status(pool, document_id, claim_token, "done", "")
            .await
            .map_err(|e| e.to_string())?
    {
        return Err("document conversion lease lost".into());
    }
    let mhash = domain::sha256_hex(md.as_bytes());
    let mkey = storage::object_ref(&mhash);
    storage::write_blob_off_runtime(&mhash, md.as_bytes()).map_err(|e| e.to_string())?;
    let quality_note = conversion_quality_note(&md, name);
    if !quality_note.is_empty() {
        tracing::warn!(
            document_id = %document_id,
            file = name,
            note = quality_note,
            "bid_convert quality"
        );
    }
    let finished = storage::bid::finish_document_conversion(
        pool,
        document_id,
        claim_token,
        "completed",
        Some(&mkey),
        quality_note,
    )
    .await
    .map_err(|e| e.to_string())?;
    if !finished {
        return Err("document conversion lease lost".into());
    }
    Ok(())
}

pub async fn extract_document(
    pool: &sqlx::PgPool,
    run_id: Uuid,
    project_id: Uuid,
    document_id: Uuid,
) -> Result<(), String> {
    extract_run(pool, run_id, project_id, Some(document_id)).await
}

struct ExtractLease {
    pool: sqlx::PgPool,
    run_id: Uuid,
    claim_token: Uuid,
    armed: bool,
}

impl ExtractLease {
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ExtractLease {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let pool = self.pool.clone();
        let run_id = self.run_id;
        let claim_token = self.claim_token;
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let _ = tokio::task::block_in_place(|| {
                handle.block_on(storage::bid::release_extract_run_to_pending(
                    &pool,
                    run_id,
                    claim_token,
                ))
            });
        }
    }
}

#[tracing::instrument(name = "bid.extract", skip_all, fields(run_id = %run_id, project_id = %project_id))]
pub async fn extract_run(
    pool: &sqlx::PgPool,
    run_id: Uuid,
    project_id: Uuid,
    document_id: Option<Uuid>,
) -> Result<(), String> {
    let claim_token =
        match storage::bid::claim_extract_run(pool, run_id, project_id, document_id).await {
            Ok(Some(token)) => token,
            Ok(None) => return Ok(()),
            Err(e) => return Err(e.to_string()),
        };
    let mut lease = ExtractLease {
        pool: pool.clone(),
        run_id,
        claim_token,
        armed: true,
    };
    let (heartbeat_stop, heartbeat_task) =
        spawn_full_extract_heartbeat(pool.clone(), run_id, project_id, claim_token);
    let run_result = extract_run_body(pool, run_id, claim_token, project_id, document_id).await;
    let _ = heartbeat_stop.send(());
    let _ = heartbeat_task.await;
    match run_result {
        Ok(()) => {
            lease.disarm();
            Ok(())
        }
        Err(error) => {
            let mode = extraction::configured_mode()
                .map(|mode| mode.as_str())
                .unwrap_or("hybrid");
            let model_id = extraction::configured_model_id();
            let (policy_version, prompt_version) =
                extraction::embedded_policy_versions().unwrap_or(("", ""));
            tracing::error!(
                run_id = %run_id,
                error = %bounded_error(&error),
                "bid_extract run fail"
            );
            let diagnostics = json!({"fatal_error": "extract_run_internal_error"});
            let _ = storage::bid::finish_extract_run(
                pool,
                storage::bid::FinishExtractRun {
                    id: run_id,
                    claim_token,
                    status: "failed",
                    section_total: 0,
                    section_done: 0,
                    error_message: diagnostics["fatal_error"].as_str().unwrap_or(""),
                    extractor_mode: mode,
                    model_id: &model_id,
                    policy_version,
                    prompt_version,
                    diagnostics: &diagnostics,
                },
            )
            .await;
            lease.disarm();
            Err(diagnostics["fatal_error"]
                .as_str()
                .unwrap_or("extract run failed")
                .to_string())
        }
    }
}

fn bounded_error(error: &str) -> String {
    error
        .replace(['\n', '\r', '\t'], " ")
        .chars()
        .take(160)
        .collect()
}

fn spawn_full_extract_heartbeat(
    pool: sqlx::PgPool,
    run_id: Uuid,
    project_id: Uuid,
    claim_token: Uuid,
) -> (
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
) {
    let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = &mut stop_rx => break,
                _ = interval.tick() => {
                    match storage::bid::heartbeat_extract_run(
                        &pool,
                        run_id,
                        project_id,
                        claim_token,
                    ).await {
                        Ok(true) => {}
                        Ok(false) => break,
                        Err(_) => continue,
                    }
                }
            }
        }
    });
    (stop_tx, task)
}

fn spawn_section_retry_heartbeat(
    pool: sqlx::PgPool,
    project_id: Uuid,
    section_id: Uuid,
    retry_token: Uuid,
) -> (
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
) {
    let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = &mut stop_rx => break,
                _ = interval.tick() => {
                    match storage::bid::heartbeat_section_retry(
                        &pool,
                        project_id,
                        section_id,
                        retry_token,
                    ).await {
                        Ok(true) => {}
                        Ok(false) => break,
                        Err(_) => continue,
                    }
                }
            }
        }
    });
    (stop_tx, task)
}

async fn extract_run_body(
    pool: &sqlx::PgPool,
    run_id: Uuid,
    claim_token: Uuid,
    project_id: Uuid,
    document_id: Option<Uuid>,
) -> Result<(), String> {
    let engine = match extraction::TenderExtractionEngine::from_env() {
        Ok(engine) => engine,
        Err(error) => {
            let mode = extraction::configured_mode()
                .map(|mode| mode.as_str())
                .unwrap_or("hybrid");
            let model_id = extraction::configured_model_id();
            let (policy_version, prompt_version) =
                extraction::embedded_policy_versions().unwrap_or(("", ""));
            tracing::error!(
                run_id = %run_id,
                error = %bounded_error(&error),
                "bid_extract run fail"
            );
            let diagnostics = json!({"configuration_error": "invalid_extraction_configuration"});
            storage::bid::finish_extract_run(
                pool,
                storage::bid::FinishExtractRun {
                    id: run_id,
                    claim_token,
                    status: "failed",
                    section_total: 0,
                    section_done: 0,
                    error_message: diagnostics["configuration_error"].as_str().unwrap_or(""),
                    extractor_mode: mode,
                    model_id: &model_id,
                    policy_version,
                    prompt_version,
                    diagnostics: &diagnostics,
                },
            )
            .await
            .map_err(|e| e.to_string())?;
            return Ok(());
        }
    };
    let doc_ids: Vec<Uuid> = if let Some(did) = document_id {
        vec![did]
    } else {
        storage::bid::list_documents(pool, project_id)
            .await
            .map_err(|e| e.to_string())?
            .iter()
            .filter(|row| {
                row.try_get::<String, _>("parse_status").unwrap_or_default() == "completed"
            })
            .filter_map(|row| row.try_get("id").ok())
            .collect()
    };
    let mut total = 0i32;
    let mut done = 0i32;
    let mut successful_documents = 0usize;
    let mut failed_documents = 0usize;
    let mut errors = Vec::new();
    let mut document_diagnostics = Vec::new();

    if doc_ids.is_empty() {
        failed_documents = 1;
        errors.push("no_completed_documents".into());
        document_diagnostics.push(json!({
            "status": "failed",
            "error": "no_completed_documents"
        }));
    }

    for document_id in doc_ids {
        match extract_one_document(pool, &engine, run_id, claim_token, project_id, document_id)
            .await
        {
            Ok(report) => {
                total += report.sections.len() as i32;
                done += report.sections.len() as i32;
                tracing::info!(
                    run_id = %run_id,
                    document_id = %document_id,
                    rounds = report.diagnostics.agent_rounds,
                    candidate_spans = report.diagnostics.coverage.candidate_spans,
                    covered_spans = report.diagnostics.coverage.covered_spans,
                    fallbacks = report.diagnostics.fallback_reasons.len(),
                    partial_failure = report.diagnostics.partial_failure,
                    "bid_extract document done"
                );
                document_diagnostics.push(json!({
                    "document_id": document_id,
                    "status": if report.diagnostics.partial_failure { "partial" } else { "done" },
                    "diagnostics": report.diagnostics
                }));
                successful_documents += 1;
            }
            Err((category, diagnostics)) => {
                if category == "extract_run_lease_lost" {
                    return Err(category);
                }
                tracing::error!(
                    run_id = %run_id,
                    document_id = %document_id,
                    category = %category,
                    "bid_extract run fail"
                );
                failed_documents += 1;
                errors.push(format!("{document_id}:{category}"));
                document_diagnostics.push(json!({
                    "document_id": document_id,
                    "status": "failed",
                    "error": category,
                    "diagnostics": diagnostics
                }));
            }
        }
    }
    let partial = document_diagnostics
        .iter()
        .any(|d| d.get("status").and_then(|v| v.as_str()) == Some("partial"));
    let status = if failed_documents > 0 && successful_documents == 0 {
        "failed"
    } else {
        "done"
    };
    let diagnostics = json!({
        "documents": document_diagnostics,
        "successful_documents": successful_documents,
        "failed_documents": failed_documents,
        "partial_failure": partial || failed_documents > 0
    });
    let error_message = if errors.is_empty() && (partial || failed_documents > 0) {
        "partial_failure".to_string()
    } else {
        errors.join("; ")
    };
    storage::bid::finish_extract_run(
        pool,
        storage::bid::FinishExtractRun {
            id: run_id,
            claim_token,
            status,
            section_total: total,
            section_done: done,
            error_message: &error_message,
            extractor_mode: engine.mode().as_str(),
            model_id: engine.model_id(),
            policy_version: engine.policy_version(),
            prompt_version: engine.prompt_version(),
            diagnostics: &diagnostics,
        },
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tracing::instrument(
    name = "bid.extract.document",
    skip_all,
    fields(run_id = %run_id, document_id = %document_id)
)]
async fn extract_one_document(
    pool: &sqlx::PgPool,
    engine: &extraction::TenderExtractionEngine,
    run_id: Uuid,
    claim_token: Uuid,
    project_id: Uuid,
    document_id: Uuid,
) -> Result<extraction::ExtractionReport, (String, Option<extraction::ExtractionDiagnostics>)> {
    let row = storage::bid::document_row(pool, document_id)
        .await
        .map_err(|_| ("document_lookup_failed".into(), None))?
        .ok_or_else(|| ("document_missing".into(), None))?;
    if row.try_get::<Uuid, _>("project_id").ok() != Some(project_id) {
        return Err(("document_project_mismatch".into(), None));
    }
    let parse_status: String = row.try_get("parse_status").unwrap_or_default();
    if parse_status != "completed" {
        return Err(("document_not_completed".into(), None));
    }
    let md_ref: String = row.try_get("markdown_ref").unwrap_or_default();
    let hash = md_ref.trim_start_matches("objects/");
    if hash.is_empty() {
        return Err(("markdown_reference_missing".into(), None));
    }
    let bytes = storage::read_blob(hash).map_err(|_| ("markdown_blob_unavailable".into(), None))?;
    let file_name: String = row.try_get("file_name").unwrap_or_default();
    let markdown = String::from_utf8(bytes).map_err(|_| ("markdown_invalid_utf8".into(), None))?;
    let outline = extraction::sections_for_document(&markdown).map_err(|e| (e, None))?;
    tracing::info!(
        run_id = %run_id,
        document_id = %document_id,
        file = %file_name,
        sections = outline.len(),
        mode = engine.mode().as_str(),
        model = engine.model_id(),
        "bid_extract start"
    );
    if outline.is_empty() {
        return Err(("document contains no extractable text".into(), None));
    }
    let pending_sections: Vec<_> = outline
        .iter()
        .map(|section| storage::bid::ExtractionSectionRow {
            id: Uuid::new_v4(),
            section_key: &section.key,
            heading_path: &section.heading_path,
            hint_family: &section.hint_family,
            body: &section.body,
            extract_status: "pending",
            error_message: "",
        })
        .collect();
    storage::bid_extract_publication::ExtractionPublicationStore::publish_document(
        pool,
        storage::bid::PersistExtractionReport {
            run_id,
            claim_token,
            project_id,
            document_id,
            sections: &pending_sections,
            clauses: &[],
            replace_document: true,
            scoped_section_count: Some(outline.len() as i32),
        },
    )
    .await
    .map_err(|error| {
        let category = if error.to_string().contains("lease lost") {
            "extract_run_lease_lost"
        } else {
            "document_persist_failed"
        };
        (category.into(), None)
    })?;
    let mut combined = extraction::ExtractionReport {
        sections: Vec::new(),
        clauses: Vec::new(),
        diagnostics: extraction::ExtractionDiagnostics {
            mode: engine.mode().as_str().into(),
            model_id: engine.model_id().to_string(),
            policy_version: engine.policy_version().to_string(),
            prompt_version: engine.prompt_version().to_string(),
            ..Default::default()
        },
    };
    let mut any_ok = false;
    for section in outline.iter() {
        let input = extraction::ExtractionInput::section(document_id, section);
        match engine.extract(input).await {
            Ok(report) => {
                tracing::info!(
                    run_id = %run_id,
                    document_id = %document_id,
                    section_key = %section.key,
                    clauses = report.clauses.len(),
                    rounds = report.diagnostics.agent_rounds,
                    fallbacks = report.diagnostics.fallback_reasons.len(),
                    "bid_extract section done"
                );
                persist_report(
                    pool,
                    run_id,
                    claim_token,
                    project_id,
                    document_id,
                    &report,
                    false,
                )
                .await?;
                merge_extract_diagnostics(&mut combined.diagnostics, &report.diagnostics);
                combined.sections.extend(report.sections);
                combined.clauses.extend(report.clauses);
                any_ok = true;
            }
            Err(failure) => {
                tracing::warn!(
                    run_id = %run_id,
                    document_id = %document_id,
                    section_key = %section.key,
                    error = %bounded_error(&failure.message),
                    "bid_extract section fail"
                );
                if failure.message.contains("lease") {
                    return Err(("extract_run_lease_lost".into(), Some(failure.diagnostics)));
                }
                let mut failed = section.clone();
                failed.extract_status = "failed".into();
                failed.error_message = failure.message.clone();
                let failed_report = extraction::ExtractionReport {
                    sections: vec![failed],
                    clauses: vec![],
                    diagnostics: failure.diagnostics.clone(),
                };
                persist_report(
                    pool,
                    run_id,
                    claim_token,
                    project_id,
                    document_id,
                    &failed_report,
                    false,
                )
                .await?;
                merge_extract_diagnostics(&mut combined.diagnostics, &failure.diagnostics);
                combined.diagnostics.partial_failure = true;
                combined.diagnostics.fallback_reasons.push(format!(
                    "section_failed:{}:{}",
                    section.key, failure.message
                ));
                combined.sections.push(section.clone());
            }
        }
    }
    if !any_ok && combined.clauses.is_empty() {
        return Err(("document_extract_failed".into(), Some(combined.diagnostics)));
    }
    let keep: Vec<String> = outline.iter().map(|section| section.key.clone()).collect();
    storage::bid_extract_publication::ExtractionPublicationStore::prune_unconfirmed_sections(
        pool,
        document_id,
        &keep,
    )
    .await
    .map_err(|_| {
        (
            "document_persist_failed".into(),
            Some(combined.diagnostics.clone()),
        )
    })?;
    Ok(combined)
}

fn merge_extract_diagnostics(
    into: &mut extraction::ExtractionDiagnostics,
    from: &extraction::ExtractionDiagnostics,
) {
    into.agent_rounds += from.agent_rounds;
    into.retries += from.retries;
    into.tool_calls += from.tool_calls;
    into.rejected_invalid_quotes += from.rejected_invalid_quotes;
    into.family_conflicts += from.family_conflicts;
    into.agent_terminations
        .extend(from.agent_terminations.iter().cloned());
    into.fallback_reasons
        .extend(from.fallback_reasons.iter().cloned());
    into.failed_spans.extend(from.failed_spans.iter().cloned());
    into.coverage.candidate_spans += from.coverage.candidate_spans;
    into.coverage.covered_spans += from.coverage.covered_spans;
    into.coverage
        .uncovered_spans
        .extend(from.coverage.uncovered_spans.iter().cloned());
    into.coverage.ambiguous_clauses += from.coverage.ambiguous_clauses;
    if from.partial_failure {
        into.partial_failure = true;
    }
}

async fn persist_report(
    pool: &sqlx::PgPool,
    run_id: Uuid,
    claim_token: Uuid,
    project_id: Uuid,
    document_id: Uuid,
    report: &extraction::ExtractionReport,
    replace_document: bool,
) -> Result<(), (String, Option<extraction::ExtractionDiagnostics>)> {
    let section_rows: Vec<_> = report
        .sections
        .iter()
        .map(|section| storage::bid::ExtractionSectionRow {
            id: Uuid::new_v4(),
            section_key: &section.key,
            heading_path: &section.heading_path,
            hint_family: &section.hint_family,
            body: &section.body,
            extract_status: &section.extract_status,
            error_message: &section.error_message,
        })
        .collect();
    let source_spans: Vec<_> = report
        .clauses
        .iter()
        .map(|clause| {
            json!({
                "span_id": clause.span_id,
                "heading_path": clause.heading_path,
                "quote": clause.quote
            })
        })
        .collect();
    let clause_rows: Vec<_> = report
        .clauses
        .iter()
        .zip(source_spans.iter())
        .map(|(clause, source_span)| storage::bid::ExtractionClauseRow {
            id: Uuid::new_v4(),
            section_key: &clause.section_key,
            source_span,
            family_conflict: clause.family_conflict,
            extraction_meta: &clause.extraction_meta,
            raw_text: &clause.quote,
            text: &clause.text,
            family: &clause.family,
            must: clause.must,
        })
        .collect();
    storage::bid_extract_publication::ExtractionPublicationStore::publish_document(
        pool,
        storage::bid::PersistExtractionReport {
            run_id,
            claim_token,
            project_id,
            document_id,
            sections: &section_rows,
            clauses: &clause_rows,
            replace_document,
            scoped_section_count: None,
        },
    )
    .await
    .map_err(|error| {
        let category = if error.to_string().contains("lease lost") {
            "extract_run_lease_lost"
        } else {
            "document_persist_failed"
        };
        (category.into(), Some(report.diagnostics.clone()))
    })
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
) -> Result<Option<Uuid>, String> {
    let receipt = storage::bid_matching::schedule_dirty_project(
        pool,
        project_id,
        matching_schedule_environment(),
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

pub async fn maybe_rematch_company_doc(
    pool: &sqlx::PgPool,
    document_id: Uuid,
) -> Result<(), String> {
    if !storage::bid::document_is_company(pool, document_id)
        .await
        .unwrap_or(false)
    {
        return Ok(());
    }
    let ready: bool =
        sqlx::query_scalar("SELECT COALESCE(index_ready, false) FROM documents WHERE id = $1")
            .bind(document_id)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten()
            .unwrap_or(false);
    if !ready {
        return Ok(());
    }
    let projects = storage::bid::open_projects_with_commercial(pool)
        .await
        .map_err(|e| e.to_string())?;
    for pid in projects {
        let _ = schedule_dirty_and_enqueue(pool, pid).await;
    }
    Ok(())
}

pub async fn retry_section_claimed(
    pool: &sqlx::PgPool,
    expected_project_id: Uuid,
    section_id: Uuid,
    retry_token: Uuid,
) -> Result<(), String> {
    let engine = extraction::TenderExtractionEngine::from_env()?;
    let row = match storage::bid::section_row(pool, section_id).await {
        Ok(Some(row)) if row.get::<Uuid, _>("project_id") == expected_project_id => row,
        Ok(Some(_)) => return Err("section_project_mismatch".into()),
        Ok(None) => return Err("section missing".into()),
        Err(error) => return Err(error.to_string()),
    };
    let project_id = expected_project_id;
    let document_id: Uuid = row.get("document_id");
    let section_key: String = row.get("section_key");
    let heading_path: String = row.get("heading_path");
    let hint_family: String = row.get("hint_family");
    let body: String = row.get("body");
    if let Err(error) = storage::bid::set_section_retry_status(
        pool,
        project_id,
        section_id,
        retry_token,
        "running",
        "",
    )
    .await
    {
        return Err(error.to_string());
    }
    let input = extraction::ExtractionInput {
        document_id,
        markdown: body.clone(),
        scope: extraction::ExtractionScope::Section {
            section_key,
            heading_path,
            hint_family,
            body,
        },
    };
    let (heartbeat_stop, heartbeat_task) =
        spawn_section_retry_heartbeat(pool.clone(), project_id, section_id, retry_token);
    let extraction = engine.extract(input).await;
    let _ = heartbeat_stop.send(());
    let _ = heartbeat_task.await;
    match extraction {
        Ok(report) => {
            let Some(section) = report.sections.first() else {
                let error = "section_extraction_empty";
                let _ = storage::bid::set_section_retry_status(
                    pool,
                    project_id,
                    section_id,
                    retry_token,
                    "failed",
                    error,
                )
                .await;
                return Err(error.into());
            };
            let section_row = storage::bid::ExtractionSectionRow {
                id: section_id,
                section_key: &section.key,
                heading_path: &section.heading_path,
                hint_family: &section.hint_family,
                body: &section.body,
                extract_status: &section.extract_status,
                error_message: &section.error_message,
            };
            let source_spans: Vec<_> = report
                .clauses
                .iter()
                .map(|clause| {
                    json!({
                        "span_id": clause.span_id,
                        "heading_path": clause.heading_path,
                        "quote": clause.quote
                    })
                })
                .collect();
            let clauses: Vec<_> = report
                .clauses
                .iter()
                .zip(source_spans.iter())
                .map(|(clause, source_span)| storage::bid::ExtractionClauseRow {
                    id: Uuid::new_v4(),
                    section_key: &clause.section_key,
                    source_span,
                    family_conflict: clause.family_conflict,
                    extraction_meta: &clause.extraction_meta,
                    raw_text: &clause.quote,
                    text: &clause.text,
                    family: &clause.family,
                    must: clause.must,
                })
                .collect();
            match storage::bid_extract_publication::ExtractionPublicationStore::publish_section(
                pool,
                retry_token,
                project_id,
                document_id,
                &section_row,
                &clauses,
            )
            .await
            {
                Ok(()) => Ok(()),
                Err(error) => {
                    let category = "section_retry_persist_failed";
                    let _ = storage::bid::set_section_retry_status(
                        pool,
                        project_id,
                        section_id,
                        retry_token,
                        "failed",
                        category,
                    )
                    .await;
                    tracing::error!(
                        error = %bounded_error(&error.to_string()),
                        "bid section retry persist failed"
                    );
                    Err(category.into())
                }
            }
        }
        Err(failure) => {
            let _ = storage::bid::set_section_retry_status(
                pool,
                project_id,
                section_id,
                retry_token,
                "failed",
                &failure.message,
            )
            .await;
            Err(failure.message)
        }
    }
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
    fn resolve_unit_follows_merge() {
        let a = Uuid::from_u128(1);
        let b = Uuid::from_u128(2);
        let mut m = std::collections::HashMap::new();
        m.insert(a, Some(b));
        m.insert(b, None);
        assert_eq!(resolve_unit(Some(a), &m), b);
        assert_eq!(resolve_unit(None, &m), Uuid::nil());
    }

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
    fn coverage_only_uses_same_unit_picks() {
        let switch = Uuid::from_u128(1);
        let store = Uuid::from_u128(2);
        let c1 = ClauseView {
            id: Uuid::from_u128(10),
            text: "40G".into(),
            raw_text: String::new(),
            family: "technical".into(),
            must: true,
            status: "confirmed".into(),
            family_conflict: false,
            deviate: false,
            deviate_note: String::new(),
            section_id: Some(switch),
            assessment: "unset".into(),
            unit_id: switch,
            suggestion: String::new(),
            hit_outcome: String::new(),
            hit_file: String::new(),
        };
        let c2 = ClauseView {
            id: Uuid::from_u128(11),
            text: "RAID".into(),
            raw_text: String::new(),
            family: "technical".into(),
            must: true,
            status: "confirmed".into(),
            family_conflict: false,
            deviate: false,
            deviate_note: String::new(),
            section_id: Some(store),
            assessment: "unset".into(),
            unit_id: store,
            suggestion: String::new(),
            hit_outcome: String::new(),
            hit_file: String::new(),
        };
        let picks = vec![json!({
            "unit_id": switch.to_string(),
            "product_id": Uuid::from_u128(99).to_string(),
            "clauses": [{
                "clause_id": c1.id.to_string(),
                "text": "40G",
                "must": true,
                "hit": true
            }]
        })];
        let cov = coverage_for(&[c1, c2], &picks);
        assert_eq!(
            cov.iter()
                .find(|r| r.clause_id == Uuid::from_u128(10))
                .unwrap()
                .status,
            "cover"
        );
        assert_eq!(
            cov.iter()
                .find(|r| r.clause_id == Uuid::from_u128(11))
                .unwrap()
                .status,
            "pending"
        );
    }

    #[test]
    fn commercial_coverage_maps_decisions_and_skips_hidden() {
        let hit = Uuid::from_u128(1);
        let miss = Uuid::from_u128(2);
        let review = Uuid::from_u128(3);
        let clauses = vec![
            ClauseView {
                id: hit,
                text: "iso".into(),
                raw_text: String::new(),
                family: "commercial".into(),
                must: true,
                status: "confirmed".into(),
                family_conflict: false,
                deviate: false,
                deviate_note: String::new(),
                section_id: None,
                assessment: "unset".into(),
                unit_id: Uuid::nil(),
                suggestion: String::new(),
                hit_outcome: String::new(),
                hit_file: String::new(),
            },
            ClauseView {
                id: miss,
                text: "miss".into(),
                raw_text: String::new(),
                family: "commercial".into(),
                must: true,
                status: "confirmed".into(),
                family_conflict: false,
                deviate: false,
                deviate_note: String::new(),
                section_id: None,
                assessment: "unset".into(),
                unit_id: Uuid::nil(),
                suggestion: String::new(),
                hit_outcome: String::new(),
                hit_file: String::new(),
            },
            ClauseView {
                id: review,
                text: "review".into(),
                raw_text: String::new(),
                family: "commercial".into(),
                must: true,
                status: "confirmed".into(),
                family_conflict: false,
                deviate: false,
                deviate_note: String::new(),
                section_id: None,
                assessment: "unset".into(),
                unit_id: Uuid::nil(),
                suggestion: String::new(),
                hit_outcome: String::new(),
                hit_file: String::new(),
            },
        ];
        let decisions = vec![
            json!({"clause_id": hit.to_string(), "outcome": "hit"}),
            json!({"clause_id": miss.to_string(), "outcome": "miss"}),
            json!({"clause_id": review.to_string(), "outcome": "review"}),
        ];
        let cov = coverage_for_commercial(&clauses, &decisions);
        assert_eq!(
            cov.iter().find(|row| row.clause_id == hit).unwrap().status,
            "hit"
        );
        assert_eq!(
            cov.iter().find(|row| row.clause_id == miss).unwrap().status,
            "miss"
        );
        assert_eq!(
            cov.iter()
                .find(|row| row.clause_id == review)
                .unwrap()
                .status,
            "review"
        );
        assert!(coverage_for_commercial(&clauses, &[]).is_empty());
    }

    #[test]
    fn preview_drops_unconfirmed_commercial_hits() {
        let project = ProjectView {
            id: Uuid::nil(),
            title: "p".into(),
            owner_name: "o".into(),
            expires_at: None,
            status: "open".into(),
            ended_at: None,
        };
        let live = Uuid::from_u128(1);
        let gone = Uuid::from_u128(2);
        let miss = Uuid::from_u128(3);
        let clauses = vec![
            ClauseView {
                id: live,
                text: "iso".into(),
                raw_text: "iso".into(),
                family: "commercial".into(),
                must: true,
                status: "confirmed".into(),
                family_conflict: false,
                deviate: false,
                deviate_note: String::new(),
                section_id: None,
                assessment: "unset".into(),
                unit_id: Uuid::nil(),
                suggestion: String::new(),
                hit_outcome: String::new(),
                hit_file: String::new(),
            },
            ClauseView {
                id: gone,
                text: "old".into(),
                raw_text: "old".into(),
                family: "commercial".into(),
                must: true,
                status: "rejected".into(),
                family_conflict: false,
                deviate: false,
                deviate_note: String::new(),
                section_id: None,
                assessment: "unset".into(),
                unit_id: Uuid::nil(),
                suggestion: String::new(),
                hit_outcome: String::new(),
                hit_file: String::new(),
            },
            ClauseView {
                id: miss,
                text: "cert".into(),
                raw_text: "cert".into(),
                family: "commercial".into(),
                must: true,
                status: "confirmed".into(),
                family_conflict: false,
                deviate: false,
                deviate_note: String::new(),
                section_id: None,
                assessment: "unset".into(),
                unit_id: Uuid::nil(),
                suggestion: String::new(),
                hit_outcome: String::new(),
                hit_file: String::new(),
            },
        ];
        let commercial = vec![
            json!({"clause_id": live.to_string(), "outcome": "hit", "file_name": "a.pdf"}),
            json!({"clause_id": gone.to_string(), "outcome": "hit", "file_name": "b.pdf"}),
            json!({"clause_id": miss.to_string(), "outcome": "miss", "file_name": null}),
        ];
        let out = preview_json(&project, &clauses, &[], &commercial, &[]);
        let s4 = out["sections"]["4"].as_array().unwrap();
        let s5 = out["sections"]["5"].as_array().unwrap();
        assert_eq!(s4.len(), 1, "{s4:?}");
        assert_eq!(s4[0]["clause_id"], live.to_string());
        assert_eq!(s5.len(), 1, "{s5:?}");
        assert_eq!(s5[0]["clause_id"], miss.to_string());
    }

    #[test]
    fn coverage_ignores_human_assessment() {
        let unit = Uuid::from_u128(1);
        let cid = Uuid::from_u128(10);
        let clause = ClauseView {
            id: cid,
            text: "40G".into(),
            raw_text: String::new(),
            family: "technical".into(),
            must: true,
            status: "confirmed".into(),
            family_conflict: false,
            deviate: true,
            deviate_note: "偏".into(),
            section_id: Some(unit),
            assessment: "deviate".into(),
            unit_id: unit,
            suggestion: String::new(),
            hit_outcome: String::new(),
            hit_file: String::new(),
        };
        let picks = vec![json!({
            "unit_id": unit.to_string(),
            "product_id": Uuid::from_u128(99).to_string(),
            "clauses": [{
                "clause_id": cid.to_string(),
                "text": "40G",
                "must": true,
                "hit": false
            }]
        })];
        let cov = coverage_for(&[clause], &picks);
        assert_eq!(cov[0].status, "unmet");
        assert_ne!(cov[0].status, "deviate");
    }

    #[test]
    fn meet_blocked_only_on_unmet() {
        assert!(meet_blocked_by_suggestion("unmet"));
        assert!(!meet_blocked_by_suggestion("cover"));
        assert!(!meet_blocked_by_suggestion("pending"));
    }

    #[test]
    fn pick_marks_cover_and_deviate_parts() {
        let unit = Uuid::from_u128(9);
        let keys = stale_keys_after_pick(unit);
        assert!(keys.contains(&"1".into()));
        assert!(keys.contains(&"3".into()));
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
                format!("2:{late}"),
                format!("2:{early}"),
                "3".into(),
                "4".into(),
                "5".into(),
            ]
        );
        let none = expected_part_keys_from(&units, &[]);
        assert_eq!(none, vec!["1", "3", "4", "5"]);
    }

    #[test]
    fn match_unit_publication_metadata_serializes_only_when_present() {
        let mut unit = MatchUnitView {
            kind: "commercial".into(),
            id: None,
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
