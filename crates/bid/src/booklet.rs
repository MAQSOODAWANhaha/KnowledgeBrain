//! 过程成稿：按分册生成 / 保存 MD，导出前校验 must 锚。

use serde::Serialize;
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;

use crate::{
    ClauseView, coverage_for, expected_part_keys_from, list_match_units, section_merge_map,
    unsectioned_unit,
};

#[derive(Debug, Clone, Serialize)]
pub struct BookletPartView {
    pub key: String,
    pub markdown: String,
    pub stale: bool,
    pub generated_at: Option<chrono::DateTime<chrono::Utc>>,
    pub edited_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub fn clause_anchor(id: Uuid) -> String {
    format!("<!-- clause:{id} -->")
}

pub fn missing_must_anchors(must_ids: &[Uuid], markdown: &str) -> Vec<Uuid> {
    must_ids
        .iter()
        .copied()
        .filter(|id| !markdown.contains(&clause_anchor(*id)))
        .collect()
}

pub fn missing_must_in_parts(must_ids: &[Uuid], parts: &[BookletPartView]) -> Vec<Uuid> {
    let md = parts
        .iter()
        .filter(|p| p.key.starts_with("2:"))
        .map(|p| p.markdown.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    missing_must_anchors(must_ids, &md)
}

pub fn sanitize_booklet_markdown(md: &str) -> String {
    let mut body = md;
    if let Some(rest) = body.strip_prefix("---") {
        if let Some(end) = rest.find("\n---") {
            body = rest[end + 4..].trim_start();
        } else if let Some(end) = rest.find("\n...") {
            body = rest[end + 4..].trim_start();
        }
    }
    let re = regex::Regex::new(r"(?s)<!--.*?-->|<[^>]+>")
        .unwrap_or_else(|_| regex::Regex::new(r"<[^>]+>").expect("html tag regex"));
    re.replace_all(body, |caps: &regex::Captures| {
        let m = caps.get(0).map(|x| x.as_str()).unwrap_or("");
        if m.starts_with("<!--") {
            m.to_string()
        } else {
            String::new()
        }
    })
    .into_owned()
}

async fn project_ended(pool: &sqlx::PgPool, project_id: Uuid) -> Result<bool, String> {
    Ok(storage::bid::get_project(pool, project_id)
        .await
        .map_err(|e| e.to_string())?
        .is_some_and(|r| r.get::<String, _>("status") == "ended"))
}

fn response_for(c: &ClauseView, cov: &str) -> String {
    if c.assessment == "meet" {
        return "已覆盖".into();
    }
    if c.assessment == "partial" {
        return c.deviate_note.clone();
    }
    if c.assessment == "deviate" || c.deviate {
        return if c.deviate_note.is_empty() {
            "偏离".into()
        } else {
            c.deviate_note.clone()
        };
    }
    if c.assessment == "fail" {
        return if c.deviate_note.is_empty() {
            "不响应".into()
        } else {
            c.deviate_note.clone()
        };
    }
    match cov {
        "cover" => "已覆盖".into(),
        "pending" => "待勾选".into(),
        "need_rematch" => "需重新匹配".into(),
        "unmet" | "uncovered" => "未覆盖".into(),
        _ => cov.into(),
    }
}

async fn load_clauses(pool: &sqlx::PgPool, project_id: Uuid) -> Result<Vec<ClauseView>, String> {
    let merge = section_merge_map(pool, project_id).await?;
    Ok(storage::bid::list_clauses(pool, project_id, true)
        .await
        .map_err(|e| e.to_string())?
        .iter()
        .map(|r| crate::clause_from_row(r, &merge))
        .collect())
}

async fn load_picks_json(
    pool: &sqlx::PgPool,
    project_id: Uuid,
) -> Result<Vec<serde_json::Value>, String> {
    Ok(storage::bid::list_picks(pool, project_id)
        .await
        .map_err(|e| e.to_string())?
        .iter()
        .map(|r| {
            json!({
                "product_id": r.get::<Uuid, _>("product_id"),
                "unit_id": r.try_get::<Uuid, _>("unit_id").unwrap_or(Uuid::nil()),
                "clauses": r.get::<serde_json::Value, _>("clauses"),
            })
        })
        .collect())
}

pub async fn expected_part_keys(
    pool: &sqlx::PgPool,
    project_id: Uuid,
) -> Result<Vec<String>, String> {
    let units = list_match_units(pool, project_id).await?;
    let clauses = load_clauses(pool, project_id).await?;
    let confirmed: Vec<Uuid> = {
        let mut v: Vec<Uuid> = clauses
            .iter()
            .filter(|c| c.family == "technical" && c.status == "confirmed")
            .map(|c| c.unit_id)
            .collect();
        v.sort();
        v.dedup();
        v
    };
    Ok(expected_part_keys_from(&units, &confirmed))
}

async fn heading_for(pool: &sqlx::PgPool, unit: Uuid) -> String {
    if unit == unsectioned_unit() {
        return "未归段".into();
    }
    storage::bid::section_row(pool, unit)
        .await
        .ok()
        .flatten()
        .and_then(|r| r.try_get::<String, _>("heading_path").ok())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "技术段".into())
}

pub async fn generate_part_markdown(
    pool: &sqlx::PgPool,
    project_id: Uuid,
    key: &str,
) -> Result<String, String> {
    let row = storage::bid::get_project(pool, project_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "bid missing".to_string())?;
    let title: String = row.get("title");
    let owner: String = row.get("owner_name");
    let clauses = load_clauses(pool, project_id).await?;
    let picks = load_picks_json(pool, project_id).await?;
    let cov = coverage_for(&clauses, &picks);
    let cov_map: std::collections::HashMap<_, _> = cov
        .iter()
        .map(|c| (c.clause_id, c.status.as_str()))
        .collect();

    if key == "1" {
        let mut names = Vec::new();
        for p in &picks {
            if let Some(id) = p.get("product_id").and_then(|x| x.as_str())
                && let Ok(pid) = Uuid::parse_str(id)
            {
                let n = storage::product_name(pool, pid)
                    .await
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| id.to_string());
                if !names.contains(&n) {
                    names.push(n);
                }
            }
        }
        let expires = row
            .try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("expires_at")
            .ok()
            .flatten()
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "未填".into());
        return Ok(format!(
            "# ① 项目扉页\n\n- 项目：{title}\n- 负责人：{owner}\n- 截止日：{expires}\n- 已选产品：{}\n",
            if names.is_empty() {
                "无".into()
            } else {
                names.join("、")
            }
        ));
    }

    if let Some(rest) = key.strip_prefix("2:") {
        let unit = if rest == "unsectioned" {
            unsectioned_unit()
        } else {
            Uuid::parse_str(rest).map_err(|_| "bad unit".to_string())?
        };
        let heading = heading_for(pool, unit).await;
        let tech: Vec<_> = clauses
            .iter()
            .filter(|c| c.family == "technical" && c.status == "confirmed" && c.unit_id == unit)
            .collect();
        let mut prod = String::new();
        for p in &picks {
            let uid = p
                .get("unit_id")
                .and_then(|x| x.as_str())
                .and_then(|s| Uuid::parse_str(s).ok());
            if uid == Some(unit)
                && let Some(id) = p.get("product_id").and_then(|x| x.as_str())
            {
                if !prod.is_empty() {
                    prod.push('、');
                }
                let n = if let Ok(pid) = Uuid::parse_str(id) {
                    storage::product_name(pool, pid)
                        .await
                        .ok()
                        .flatten()
                        .unwrap_or_else(|| id.to_string())
                } else {
                    id.to_string()
                };
                prod.push_str(&n);
            }
        }
        let mut md = format!(
            "# ② {heading}\n\n本段产品：{}\n\n",
            if prod.is_empty() {
                "未勾选".into()
            } else {
                prod
            }
        );
        md.push_str("| 招标要求 | 应答 |\n| --- | --- |\n");
        for c in &tech {
            let st = *cov_map.get(&c.id).unwrap_or(&"pending");
            let resp = response_for(c, st);
            md.push_str(&format!(
                "| {} {} | {} |\n",
                c.text.replace('|', "\\|"),
                clause_anchor(c.id),
                resp.replace('|', "\\|")
            ));
        }
        if !tech.is_empty() {
            return Ok(md);
        }
        md.push_str("\n（本段尚无已确认技术参数。）\n");
        return Ok(md);
    }

    if key == "3" {
        let mut md = String::from("# ③ 技术偏离表\n\n");
        let mut any = false;
        for c in clauses
            .iter()
            .filter(|c| c.family == "technical" && c.status == "confirmed")
        {
            let st = *cov_map.get(&c.id).unwrap_or(&"pending");
            let kind = if c.assessment == "partial" {
                Some("部分偏离")
            } else if c.assessment == "deviate" || c.deviate {
                Some("偏离")
            } else if c.assessment == "fail" {
                Some("不响应")
            } else if st == "unmet" {
                Some("must 未覆盖")
            } else {
                None
            };
            if let Some(k) = kind {
                any = true;
                md.push_str(&format!("- {}（{k}）\n", c.text));
            }
        }
        if !any {
            md.push_str("无偏离 / 无 must 未覆盖。\n");
        }
        return Ok(md);
    }

    let hits = storage::bid::list_commercial_hits(pool, project_id)
        .await
        .map_err(|e| e.to_string())?;
    if key == "4" {
        let mut md = String::from("# ④ 资格 / 商务材料\n\n");
        let mut any = false;
        for h in &hits {
            let cid: Uuid = h.get("clause_id");
            let outcome: String = h.get("outcome");
            if outcome != "hit" {
                continue;
            }
            if !clauses
                .iter()
                .any(|c| c.id == cid && c.family == "commercial" && c.status == "confirmed")
            {
                continue;
            }
            any = true;
            let name: String = h
                .try_get::<Option<String>, _>("file_name")
                .ok()
                .flatten()
                .unwrap_or_default();
            md.push_str(&format!("- {name}\n"));
        }
        if !any {
            md.push_str("暂无已命中的公司资料。\n");
        }
        return Ok(md);
    }
    if key == "5" {
        let mut md = String::from("# ⑤ 商务缺件\n\n");
        let mut any = false;
        for h in &hits {
            let cid: Uuid = h.get("clause_id");
            let outcome: String = h.get("outcome");
            if outcome != "miss" {
                continue;
            }
            if let Some(c) = clauses.iter().find(|c| {
                c.id == cid && c.family == "commercial" && c.status == "confirmed" && c.must
            }) {
                any = true;
                md.push_str(&format!("- {}\n", c.text));
            }
        }
        if !any {
            md.push_str("无 must 缺件。\n");
        }
        return Ok(md);
    }
    Err("unknown booklet part".into())
}

pub async fn ensure_part(
    pool: &sqlx::PgPool,
    project_id: Uuid,
    key: &str,
    force: bool,
) -> Result<BookletPartView, String> {
    if force && project_ended(pool, project_id).await? {
        let exists = storage::bid::get_booklet_part(pool, project_id, key)
            .await
            .map_err(|e| e.to_string())?
            .is_some();
        if exists {
            return Err("project ended".into());
        }
    }
    if !force && let Ok(Some(row)) = storage::bid::get_booklet_part(pool, project_id, key).await {
        return Ok(part_from_row(&row));
    }
    let md = generate_part_markdown(pool, project_id, key).await?;
    storage::bid::upsert_booklet_generated(pool, project_id, key, &md)
        .await
        .map_err(|e| e.to_string())?;
    let row = storage::bid::get_booklet_part(pool, project_id, key)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "booklet write missing".to_string())?;
    Ok(part_from_row(&row))
}

pub async fn ensure_all_parts(
    pool: &sqlx::PgPool,
    project_id: Uuid,
    regenerate_stale: bool,
) -> Result<Vec<BookletPartView>, String> {
    let keys = expected_part_keys(pool, project_id).await?;
    let mut out = Vec::new();
    for key in keys {
        let existing = storage::bid::get_booklet_part(pool, project_id, &key)
            .await
            .map_err(|e| e.to_string())?;
        let force = regenerate_stale
            && !project_ended(pool, project_id).await?
            && existing
                .as_ref()
                .is_some_and(|r| r.try_get::<bool, _>("stale").unwrap_or(false));
        match existing {
            Some(row) if !force => out.push(part_from_row(&row)),
            _ => out.push(ensure_part(pool, project_id, &key, true).await?),
        }
    }
    Ok(out)
}

fn part_from_row(r: &sqlx::postgres::PgRow) -> BookletPartView {
    BookletPartView {
        key: r.get("part_key"),
        markdown: r.get("markdown"),
        stale: r.get("stale"),
        generated_at: r.try_get("generated_at").ok().flatten(),
        edited_at: r.try_get("edited_at").ok().flatten(),
    }
}

pub async fn save_part(
    pool: &sqlx::PgPool,
    project_id: Uuid,
    key: &str,
    markdown: &str,
) -> Result<(), String> {
    let status: String = storage::bid::get_project(pool, project_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "bid missing".to_string())?
        .get("status");
    if status == "ended" {
        return Err("project ended".into());
    }
    if storage::bid::get_booklet_part(pool, project_id, key)
        .await
        .map_err(|e| e.to_string())?
        .is_none()
    {
        ensure_part(pool, project_id, key, true).await?;
    }
    let cleaned = sanitize_booklet_markdown(markdown);
    storage::bid::update_booklet_markdown(pool, project_id, key, &cleaned)
        .await
        .map_err(|e| e.to_string())
}

pub async fn must_ids(pool: &sqlx::PgPool, project_id: Uuid) -> Result<Vec<Uuid>, String> {
    Ok(load_clauses(pool, project_id)
        .await?
        .into_iter()
        .filter(|c| c.family == "technical" && c.status == "confirmed" && c.must)
        .map(|c| c.id)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_anchor_detected() {
        let id = Uuid::from_u128(7);
        let md = "# ②\n| 吞吐 | 已覆盖 |\n";
        assert_eq!(missing_must_anchors(&[id], md), vec![id]);
        let ok = format!("# ②\n| 吞吐 {} | 已覆盖 |\n", clause_anchor(id));
        assert!(missing_must_anchors(&[id], &ok).is_empty());
    }

    #[test]
    fn must_anchors_only_count_part_two() {
        let id = Uuid::from_u128(7);
        let parts = vec![
            BookletPartView {
                key: "3".into(),
                markdown: format!("# ③\n- 吞吐 {}", clause_anchor(id)),
                stale: false,
                generated_at: None,
                edited_at: None,
            },
            BookletPartView {
                key: "2:u".into(),
                markdown: "# ②\n| 吞吐 | 已覆盖 |\n".into(),
                stale: false,
                generated_at: None,
                edited_at: None,
            },
        ];
        assert_eq!(missing_must_in_parts(&[id], &parts), vec![id]);
        let ok = vec![BookletPartView {
            key: "2:u".into(),
            markdown: format!("# ②\n| 吞吐 {} | 已覆盖 |\n", clause_anchor(id)),
            stale: false,
            generated_at: None,
            edited_at: None,
        }];
        assert!(missing_must_in_parts(&[id], &ok).is_empty());
    }

    #[test]
    fn part_three_has_no_clause_anchor() {
        let md = "# ③ 技术偏离表\n\n- 吞吐（偏离）\n";
        assert!(!md.contains("<!-- clause:"));
    }

    #[test]
    fn sanitize_strips_html_and_yaml_keeps_anchor() {
        let id = Uuid::from_u128(3);
        let raw = format!(
            "---\ntitle: x\n---\n# ②\n<script>x</script>| 吞吐 {} | 已覆盖 |\n",
            clause_anchor(id)
        );
        let out = sanitize_booklet_markdown(&raw);
        assert!(!out.contains("<script>"));
        assert!(!out.contains("title: x"));
        assert!(out.contains(&clause_anchor(id)));
    }
}
