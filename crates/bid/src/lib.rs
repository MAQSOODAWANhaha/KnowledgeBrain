//! Bid project extract, match jobs, coverage, and preview.

mod booklet;
mod export;
pub mod extraction;

pub use booklet::{BookletPartView, ensure_all_parts, ensure_part, save_part};
pub use export::{
    ExportDoc, ExportKind, build_export_docx, build_export_pdf, export_project, export_project_opts,
};

use chrono::{DateTime, Utc};
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

pub fn should_skip_match(prev_key: &str, prev_status: &str, key: &str) -> bool {
    prev_key == key && matches!(prev_status, "pending" | "running" | "done")
}

pub fn match_job_overall_status(tech_status: &str, comm_status: &str) -> &'static str {
    if tech_status == "failed" || comm_status == "failed" {
        "failed"
    } else {
        "done"
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
    });
    Ok(out)
}

pub async fn decorate_clauses(
    pool: &sqlx::PgPool,
    project_id: Uuid,
    clauses: &mut [ClauseView],
) -> Result<(), String> {
    let picks: Vec<serde_json::Value> = storage::bid::list_picks(pool, project_id)
        .await
        .map_err(|e| e.to_string())?
        .iter()
        .map(|r| {
            json!({
                "product_id": r.get::<Uuid, _>("product_id").to_string(),
                "unit_id": r.try_get::<Uuid, _>("unit_id").unwrap_or(Uuid::nil()).to_string(),
                "clauses": r.get::<serde_json::Value, _>("clauses"),
            })
        })
        .collect();
    let cov = coverage_for(clauses, &picks);
    let cmap: std::collections::HashMap<Uuid, String> =
        cov.into_iter().map(|r| (r.clause_id, r.status)).collect();
    let hits = storage::bid::list_commercial_hits(pool, project_id)
        .await
        .map_err(|e| e.to_string())?;
    let mut hmap = std::collections::HashMap::new();
    for h in hits {
        let cid: Uuid = h.get("clause_id");
        let outcome: String = h.get("outcome");
        let file = h
            .try_get::<Option<String>, _>("file_name")
            .ok()
            .flatten()
            .unwrap_or_default();
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

pub fn debounce_key(parts: &[(Uuid, &str, bool, &str)]) -> String {
    let mut acc = String::new();
    let mut rows = parts.to_vec();
    rows.sort_by_key(|a| a.0);
    for (id, text, must, family) in rows {
        acc.push_str(&id.to_string());
        acc.push('\t');
        acc.push_str(text);
        acc.push('\t');
        acc.push_str(if must { "1" } else { "0" });
        acc.push('\t');
        acc.push_str(family);
        acc.push('\n');
    }
    domain::sha256_hex(acc.as_bytes())
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

fn preview_clause_id(h: &serde_json::Value) -> Option<Uuid> {
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
        } else if outcome == "miss"
            && clauses.iter().any(|c| {
                c.id == cid && c.must && c.family == "commercial" && c.status == "confirmed"
            })
        {
            s5.push(json!({"clause_id": cid.to_string(), "status": "缺件"}));
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

pub async fn convert_document(pool: &sqlx::PgPool, document_id: Uuid) -> Result<(), String> {
    let Some((claim_token, name, key, _generation)) =
        storage::bid::claim_document_conversion(pool, document_id)
            .await
            .map_err(|e| e.to_string())?
    else {
        return Ok(());
    };
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
        Ok(()) => Ok(()),
        Err(error) => {
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
    for img in &result.images {
        if img.data.is_empty() {
            continue;
        }
        let (ihash, ikey) = {
            let h = domain::sha256_hex(&img.data);
            let k = storage::object_key(&h);
            storage::write_blob_off_runtime(&h, &img.data).map_err(|e| e.to_string())?;
            (h, k)
        };
        let _ = ihash;
        if multimodal_enabled {
            let (ocr, cap) = match enrichment::describe_image(&ikey, &image_source_type) {
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
    let mkey = storage::object_key(&mhash);
    storage::write_blob_off_runtime(&mhash, md.as_bytes()).map_err(|e| e.to_string())?;
    let quality_note = conversion_quality_note(&md, name);
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
            eprintln!("bid_extract run={run_id} fatal={}", bounded_error(&error));
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
            eprintln!(
                "bid_extract run={run_id} configuration_error={}",
                bounded_error(&error)
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
                eprintln!(
                    "bid_extract run={run_id} document={document_id} mode={} model={} policy={} prompt={} rounds={} retries={} tools={} candidate_spans={} covered_spans={} conflicts={} fallbacks={}",
                    report.diagnostics.mode,
                    report.diagnostics.model_id,
                    report.diagnostics.policy_version,
                    report.diagnostics.prompt_version,
                    report.diagnostics.agent_rounds,
                    report.diagnostics.retries,
                    report.diagnostics.tool_calls,
                    report.diagnostics.coverage.candidate_spans,
                    report.diagnostics.coverage.covered_spans,
                    report.diagnostics.family_conflicts,
                    report.diagnostics.fallback_reasons.len()
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
                eprintln!("bid_extract run={run_id} document={document_id} failed={category}");
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
    let markdown = String::from_utf8(bytes).map_err(|_| ("markdown_invalid_utf8".into(), None))?;
    let outline = extraction::sections_for_document(&markdown).map_err(|e| (e, None))?;
    if outline.is_empty() {
        return Err(("document contains no extractable text".into(), None));
    }
    storage::bid::persist_extraction_report(
        pool,
        storage::bid::PersistExtractionReport {
            run_id,
            claim_token,
            project_id,
            document_id,
            sections: &[],
            clauses: &[],
            replace_document: true,
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
    storage::bid::prune_unconfirmed_sections(pool, document_id, &keep)
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
    storage::bid::persist_extraction_report(
        pool,
        storage::bid::PersistExtractionReport {
            run_id,
            claim_token,
            project_id,
            document_id,
            sections: &section_rows,
            clauses: &clause_rows,
            replace_document,
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

pub async fn run_match_job(
    pool: &sqlx::PgPool,
    job_id: Uuid,
    project_id: Uuid,
) -> Result<(), String> {
    let Some(claim_token) = storage::bid::claim_match_job(pool, job_id, project_id)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(());
    };
    let (heartbeat_stop, mut heartbeat_stop_rx) = tokio::sync::oneshot::channel();
    let heartbeat_pool = pool.clone();
    let heartbeat_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(15));
        loop {
            tokio::select! {
                _ = &mut heartbeat_stop_rx => break,
                _ = interval.tick() => {
                    match storage::bid::heartbeat_match_job(&heartbeat_pool, job_id, claim_token).await {
                        Ok(true) => {}
                        _ => break,
                    }
                }
            }
        }
    });
    let result = run_claimed_match_job(pool, job_id, project_id, claim_token).await;
    let _ = heartbeat_stop.send(());
    let _ = heartbeat_task.await;
    result
}

async fn run_claimed_match_job(
    pool: &sqlx::PgPool,
    job_id: Uuid,
    project_id: Uuid,
    claim_token: Uuid,
) -> Result<(), String> {
    let unit = storage::bid::match_job_unit(pool, job_id)
        .await
        .map_err(|e| e.to_string())?;
    let merge = section_merge_map(pool, project_id).await?;
    let all_tech = storage::bid::confirmed_clauses(pool, project_id, "technical")
        .await
        .map_err(|e| e.to_string())?;
    let tech_rows: Vec<_> = if let Some(uid) = unit {
        all_tech
            .into_iter()
            .filter(|r| {
                let sid = r.try_get::<Option<Uuid>, _>("section_id").ok().flatten();
                resolve_unit(sid, &merge) == uid
            })
            .collect()
    } else {
        Vec::new()
    };
    let comm_rows = if unit.is_none() {
        storage::bid::confirmed_clauses(pool, project_id, "commercial")
            .await
            .map_err(|e| e.to_string())?
    } else {
        Vec::new()
    };
    let mut tech_status = "skipped";
    let mut comm_status = "skipped";
    let mut candidates = json!([]);
    let mut err = String::new();
    if !tech_rows.is_empty() {
        tech_status = "running";
        let mut reqs = Vec::new();
        for r in &tech_rows {
            reqs.push(search::Requirement {
                id: r.get::<Uuid, _>("id").to_string(),
                text: r.get("text"),
                weight: 1.0,
                must: r.get("must"),
                tag_ids: vec![],
                use_library: false,
            });
        }
        let mut all = Vec::new();
        for chunk in reqs.chunks(30) {
            let req = search::SearchRequest {
                mode: "matching".into(),
                query: None,
                product_id: None,
                version_id: None,
                include_library: false,
                tag_ids: vec![],
                match_count: 10,
                expand_wiki: false,
                expand_graph: false,
                requirements: chunk.to_vec(),
                version_scope: "current".into(),
                product_ids: vec![],
                workspace_id: None,
                scope: Some("product_lines".into()),
                group_by: "none".into(),
                tender_text: None,
            };
            match search::matching_pg(pool, &req).await {
                Ok(mut resp) => all.append(&mut resp.candidates),
                Err(e) => {
                    tech_status = "failed";
                    err = e.message;
                    break;
                }
            }
        }
        if tech_status != "failed" {
            candidates = merge_candidates(all, &reqs);
            tech_status = "done";
        }
    }
    if unit.is_some() {
        comm_status = "skipped";
    } else if comm_rows.is_empty() {
        if storage::bid::replace_commercial_hits_for_job(pool, job_id, project_id, claim_token, &[])
            .await
            .is_err()
        {
            comm_status = "failed";
        }
    } else {
        comm_status = "running";
        let mut hits_acc = Vec::new();
        let mut reqs = Vec::new();
        for r in &comm_rows {
            reqs.push(search::Requirement {
                id: r.get::<Uuid, _>("id").to_string(),
                text: r.get("text"),
                weight: 1.0,
                must: r.get("must"),
                tag_ids: vec![],
                use_library: false,
            });
        }
        let mut ok = true;
        for chunk in reqs.chunks(30) {
            let req = search::SearchRequest {
                mode: "matching".into(),
                query: None,
                product_id: None,
                version_id: None,
                include_library: false,
                tag_ids: vec![],
                match_count: 10,
                expand_wiki: false,
                expand_graph: false,
                requirements: chunk.to_vec(),
                version_scope: "current".into(),
                product_ids: vec![],
                workspace_id: None,
                scope: Some("company".into()),
                group_by: "none".into(),
                tender_text: None,
            };
            match search::matching_pg(pool, &req).await {
                Ok(resp) => hits_acc.extend(resp.clauses),
                Err(e) => {
                    comm_status = "failed";
                    if !err.is_empty() {
                        err.push_str("; ");
                    }
                    err.push_str(&e.message);
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            let rows: Vec<storage::bid::CommercialHitRow<'_>> = hits_acc
                .iter()
                .filter_map(|c| {
                    Some(storage::bid::CommercialHitRow {
                        clause_id: Uuid::parse_str(&c.id).ok()?,
                        outcome: c.outcome.as_str(),
                        document_id: c.document_id,
                        version_id: c.version_id,
                        file_name: c.file_name.clone(),
                        score: c.score,
                        product_id: c.product_id,
                    })
                })
                .collect();
            if storage::bid::replace_commercial_hits_for_job(
                pool,
                job_id,
                project_id,
                claim_token,
                &rows,
            )
            .await
            .is_err()
            {
                comm_status = "failed";
            } else {
                comm_status = "done";
            }
        }
    }
    let status = match_job_overall_status(tech_status, comm_status);
    storage::bid::set_match_job(
        pool,
        storage::bid::MatchJobFinish {
            id: job_id,
            project_id,
            claim_token,
            status,
            tech_status,
            commercial_status: comm_status,
            candidates: &candidates,
            error: &err,
        },
    )
    .await
    .map_err(|e| e.to_string())?;
    if comm_status == "done" {
        let _ = storage::bid::mark_booklet_stale(pool, project_id, &["4", "5"]).await;
    }
    Ok(())
}

fn merge_candidates(
    parts: Vec<search::Candidate>,
    all_reqs: &[search::Requirement],
) -> serde_json::Value {
    use std::collections::HashMap;
    let mut by: HashMap<Uuid, search::Candidate> = HashMap::new();
    for c in parts {
        by.entry(c.product_id)
            .and_modify(|e| {
                e.requirements.extend(c.requirements.clone());
            })
            .or_insert(c);
    }
    let mut out = Vec::new();
    for (_, mut c) in by {
        let mut wsum = 0.0;
        let mut weighted = 0.0;
        let mut hit_w = 0.0;
        let mut unmet = Vec::new();
        for r in all_reqs {
            if let Some(rr) = c.requirements.iter().find(|x| x.id == r.id) {
                weighted += r.weight * rr.score;
                wsum += r.weight;
                if rr.hit {
                    hit_w += r.weight;
                }
                if r.must && !rr.hit {
                    unmet.push(r.id.clone());
                }
            }
        }
        c.score = if wsum == 0.0 { 0.0 } else { weighted / wsum };
        c.coverage = if wsum == 0.0 { 0.0 } else { hit_w / wsum };
        c.unmet_must = unmet;
        out.push(c);
    }
    out.sort_by(|a, b| {
        b.unmet_must.is_empty().cmp(&a.unmet_must.is_empty()).then(
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });
    serde_json::to_value(out).unwrap_or(json!([]))
}

async fn enqueue_one_match(
    pool: &sqlx::PgPool,
    project_id: Uuid,
    generation: i64,
    job_kind: &str,
    unit_id: Option<Uuid>,
    parts: &[(Uuid, String, bool, String)],
) -> Result<Option<Uuid>, String> {
    let refs: Vec<(Uuid, &str, bool, &str)> = parts
        .iter()
        .map(|(id, t, m, f)| (*id, t.as_str(), *m, f.as_str()))
        .collect();
    let key = debounce_key(&refs);
    let requested_id = Uuid::new_v4();
    let job_id = match storage::bid::insert_match_job(
        pool,
        requested_id,
        project_id,
        generation,
        &key,
        job_kind,
        unit_id,
    )
    .await
    {
        Ok(job_id) => job_id,
        Err(sqlx::Error::RowNotFound) => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    // The pending match row is durable; housekeeping recovers a transient enqueue failure.
    let _ = runtime::enqueue_bid_match(job_id, project_id, key).await;
    Ok(Some(job_id))
}

pub async fn schedule_match(pool: &sqlx::PgPool, project_id: Uuid) -> Result<Option<Uuid>, String> {
    let generation = storage::bid::current_match_generation(pool, project_id)
        .await
        .map_err(|e| e.to_string())?;
    let tech = storage::bid::confirmed_clauses(pool, project_id, "technical")
        .await
        .map_err(|e| e.to_string())?;
    let comm = storage::bid::confirmed_clauses(pool, project_id, "commercial")
        .await
        .map_err(|e| e.to_string())?;
    if tech.is_empty() && comm.is_empty() {
        storage::bid::replace_commercial_hits(pool, project_id, &[])
            .await
            .map_err(|e| e.to_string())?;
        storage::bid::clear_match_dirty(pool, project_id, generation)
            .await
            .map_err(|e| e.to_string())?;
        return Ok(None);
    }
    let merge = section_merge_map(pool, project_id).await?;
    let mut by_unit: std::collections::HashMap<Uuid, Vec<(Uuid, String, bool, String)>> =
        std::collections::HashMap::new();
    for r in &tech {
        let sid = r.try_get::<Option<Uuid>, _>("section_id").ok().flatten();
        let unit = resolve_unit(sid, &merge);
        by_unit.entry(unit).or_default().push((
            r.get("id"),
            r.get("text"),
            r.get("must"),
            r.get("family"),
        ));
    }
    let mut last = None;
    for (unit, parts) in by_unit {
        last = enqueue_one_match(
            pool,
            project_id,
            generation,
            "technical",
            Some(unit),
            &parts,
        )
        .await?;
    }
    if !comm.is_empty() {
        let parts: Vec<_> = comm
            .iter()
            .map(|r| {
                (
                    r.get::<Uuid, _>("id"),
                    r.get::<String, _>("text"),
                    r.get::<bool, _>("must"),
                    r.get::<String, _>("family"),
                )
            })
            .collect();
        last = enqueue_one_match(pool, project_id, generation, "commercial", None, &parts).await?;
    }
    storage::bid::clear_match_dirty(pool, project_id, generation)
        .await
        .map_err(|e| e.to_string())?;
    Ok(last)
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
        let _ = schedule_match(pool, pid).await;
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
            match storage::bid::persist_section_retry(
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
                    eprintln!(
                        "bid section retry persist error: {}",
                        bounded_error(&error.to_string())
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
    fn debounce_key_stable() {
        let a = Uuid::nil();
        let k1 = debounce_key(&[(a, "iso", true, "commercial")]);
        let k2 = debounce_key(&[(a, "iso", true, "commercial")]);
        let k3 = debounce_key(&[(a, "iso", false, "commercial")]);
        assert_eq!(k1, k2);
        assert_ne!(k1, k3);
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
    fn skip_match_when_same_key_already_done() {
        assert!(should_skip_match("abc", "done", "abc"));
        assert!(should_skip_match("abc", "pending", "abc"));
        assert!(!should_skip_match("abc", "done", "xyz"));
        assert!(!should_skip_match("abc", "failed", "abc"));
    }

    #[test]
    fn match_job_fails_if_either_side_failed() {
        assert_eq!(match_job_overall_status("failed", "skipped"), "failed");
        assert_eq!(match_job_overall_status("skipped", "failed"), "failed");
        assert_eq!(match_job_overall_status("done", "skipped"), "done");
        assert_eq!(match_job_overall_status("failed", "failed"), "failed");
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
}
