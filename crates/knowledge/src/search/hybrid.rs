//! SQL persistence split from persist.rs (behavior unchanged).
use crate::{version_indexing_flags, workspace_top_k_for_version};
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct PgSearchHit {
    pub chunk_id: Uuid,
    pub content: String,
    pub chunk_type: String,
    pub document_id: Uuid,
    pub document_title: String,
    pub product_id: Uuid,
    pub product_kind: String,
    pub version_id: Uuid,
    pub version_label: String,
    pub is_current: bool,
    pub start_at: i32,
    pub end_at: i32,
    pub vec_score: f64,
    pub kw_score: f64,
    pub tag_ids: Vec<Uuid>,
    pub tag_slugs: Vec<String>,
    pub context_header: String,
    pub document_object_ref: String,
}

pub(crate) fn pg_hit_from_row(r: &sqlx::postgres::PgRow) -> Result<PgSearchHit, sqlx::Error> {
    Ok(PgSearchHit {
        chunk_id: r.try_get("chunk_id")?,
        content: r.try_get("content")?,
        chunk_type: r.try_get("chunk_type")?,
        document_id: r.try_get("document_id")?,
        document_title: r.try_get("title")?,
        product_id: r.try_get("product_id")?,
        product_kind: r.try_get("product_kind")?,
        version_id: r.try_get("version_id")?,
        version_label: r.try_get("version_label")?,
        is_current: r.try_get("is_current")?,
        start_at: r.try_get("start_at")?,
        end_at: r.try_get("end_at")?,
        vec_score: r.try_get::<f64, _>("vec_score").unwrap_or(0.0),
        kw_score: r.try_get::<f64, _>("kw_score").unwrap_or(0.0),
        tag_ids: r.try_get("tag_ids")?,
        tag_slugs: r.try_get("tag_slugs").unwrap_or_default(),
        context_header: r.try_get("context_header").unwrap_or_default(),
        document_object_ref: r.try_get("document_object_ref").unwrap_or_default(),
    })
}

pub async fn hybrid_search_pg(
    pool: &PgPool,
    version_id: Uuid,
    query: &str,
    query_vec: &str,
    tag_ids: &[Uuid],
    expand_wiki: bool,
    limit: i64,
) -> Result<Vec<PgSearchHit>, sqlx::Error> {
    let (vector_on, keyword_on) = version_indexing_flags(pool, version_id)
        .await
        .unwrap_or((true, true));
    if !vector_on && !keyword_on {
        return Ok(Vec::new());
    }
    let top_k = workspace_top_k_for_version(pool, version_id)
        .await
        .unwrap_or(50) as i64;
    const VEC_SQL: &str = "SELECT c.id AS chunk_id, c.content, c.chunk_type, c.document_id,
                c.start_at, c.end_at, COALESCE(c.context_header, '') AS context_header,
                COALESCE(d.object_ref, '') AS document_object_ref, d.title,
                p.id AS product_id, p.kind AS product_kind,
                pv.id AS version_id, pv.label AS version_label,
                (p.current_version_id = pv.id) AS is_current,
                COALESCE(1.0 - (e.embedding <=> CAST($2 AS vector)), 0.0) AS vec_score,
                COALESCE(ts_rank_cd(e.tsv, plainto_tsquery('simple', $3)), 0.0) AS kw_score,
                COALESCE(
                    (SELECT array_agg(dt.tag_id) FROM document_tags dt WHERE dt.document_id = d.id),
                    '{}'::uuid[]
                ) AS tag_ids,
                COALESCE(
                    (SELECT array_agg(t.slug) FROM document_tags dt
                     JOIN tags t ON t.id = dt.tag_id WHERE dt.document_id = d.id),
                    '{}'::text[]
                ) AS tag_slugs
         FROM chunk_embeddings e
         JOIN chunks c ON c.id = e.chunk_id
         JOIN documents d ON d.id = e.document_id
         JOIN product_versions pv ON pv.id = e.product_version_id
         JOIN products p ON p.id = pv.product_id
         WHERE e.product_version_id = $1
           AND d.enable_status = 'enabled'
           AND d.deleted_at IS NULL
           AND ($4 OR c.chunk_type <> 'wiki_page')
           AND (
                cardinality($5::uuid[]) = 0
                OR EXISTS (
                    SELECT 1 FROM document_tags dt
                    WHERE dt.document_id = d.id AND dt.tag_id = ANY($5)
                )
           )
         ORDER BY e.embedding <=> CAST($2 AS vector)
         LIMIT $6";
    const KW_SQL: &str = "SELECT c.id AS chunk_id, c.content, c.chunk_type, c.document_id,
                c.start_at, c.end_at, COALESCE(c.context_header, '') AS context_header,
                COALESCE(d.object_ref, '') AS document_object_ref, d.title,
                p.id AS product_id, p.kind AS product_kind,
                pv.id AS version_id, pv.label AS version_label,
                (p.current_version_id = pv.id) AS is_current,
                COALESCE(1.0 - (e.embedding <=> CAST($2 AS vector)), 0.0) AS vec_score,
                COALESCE(ts_rank_cd(e.tsv, plainto_tsquery('simple', $3)), 0.0) AS kw_score,
                COALESCE(
                    (SELECT array_agg(dt.tag_id) FROM document_tags dt WHERE dt.document_id = d.id),
                    '{}'::uuid[]
                ) AS tag_ids,
                COALESCE(
                    (SELECT array_agg(t.slug) FROM document_tags dt
                     JOIN tags t ON t.id = dt.tag_id WHERE dt.document_id = d.id),
                    '{}'::text[]
                ) AS tag_slugs
         FROM chunk_embeddings e
         JOIN chunks c ON c.id = e.chunk_id
         JOIN documents d ON d.id = e.document_id
         JOIN product_versions pv ON pv.id = e.product_version_id
         JOIN products p ON p.id = pv.product_id
         WHERE e.product_version_id = $1
           AND d.enable_status = 'enabled'
           AND d.deleted_at IS NULL
           AND ($4 OR c.chunk_type <> 'wiki_page')
           AND (
                cardinality($5::uuid[]) = 0
                OR EXISTS (
                    SELECT 1 FROM document_tags dt
                    WHERE dt.document_id = d.id AND dt.tag_id = ANY($5)
                )
           )
           AND e.tsv @@ plainto_tsquery('simple', $3)
         ORDER BY ts_rank_cd(e.tsv, plainto_tsquery('simple', $3)) DESC
         LIMIT $6";
    let vec_rows = if vector_on {
        sqlx::query(VEC_SQL)
            .bind(version_id)
            .bind(query_vec)
            .bind(query)
            .bind(expand_wiki)
            .bind(tag_ids)
            .bind(top_k.max(1))
            .fetch_all(pool)
            .await?
    } else {
        Vec::new()
    };
    let kw_rows = if keyword_on {
        sqlx::query(KW_SQL)
            .bind(version_id)
            .bind(query_vec)
            .bind(query)
            .bind(expand_wiki)
            .bind(tag_ids)
            .bind(limit.max(1))
            .fetch_all(pool)
            .await?
    } else {
        Vec::new()
    };
    let mut by_id: std::collections::HashMap<Uuid, PgSearchHit> = std::collections::HashMap::new();
    for r in vec_rows.iter().chain(kw_rows.iter()) {
        let mut hit = pg_hit_from_row(r)?;
        if !vector_on {
            hit.vec_score = 0.0;
        }
        if !keyword_on {
            hit.kw_score = 0.0;
        }
        match by_id.get_mut(&hit.chunk_id) {
            Some(old) => {
                if hit.vec_score > old.vec_score {
                    old.vec_score = hit.vec_score;
                }
                if hit.kw_score > old.kw_score {
                    old.kw_score = hit.kw_score;
                }
            }
            None => {
                by_id.insert(hit.chunk_id, hit);
            }
        }
    }
    let mut out: Vec<PgSearchHit> = by_id.into_values().collect();
    out.sort_by(|a, b| {
        a.vec_score
            .max(a.kw_score)
            .partial_cmp(&b.vec_score.max(b.kw_score))
            .unwrap_or(std::cmp::Ordering::Equal)
            .reverse()
    });
    out.truncate(limit.max(1) as usize);
    Ok(out)
}

pub async fn resolve_pg_assembly_targets(
    pool: &PgPool,
    product_id: Uuid,
    version_id: Option<&str>,
    include_library: bool,
) -> Result<Vec<Uuid>, sqlx::Error> {
    let mut targets = Vec::new();
    if let Some(vs) = version_id {
        if vs == "current" {
            let id = sqlx::query_scalar::<_, Option<Uuid>>(
                "SELECT current_version_id FROM products WHERE id = $1",
            )
            .bind(product_id)
            .fetch_optional(pool)
            .await?
            .flatten();
            let Some(id) = id else {
                return Ok(Vec::new());
            };
            targets.push(id);
        } else if let Ok(id) = Uuid::parse_str(vs) {
            let ok: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM product_versions WHERE id = $1 AND product_id = $2)",
            )
            .bind(id)
            .bind(product_id)
            .fetch_one(pool)
            .await?;
            if ok {
                targets.push(id);
            }
        }
    } else {
        let ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM product_versions
             WHERE product_id = $1 AND status = 'active' AND deleted_at IS NULL",
        )
        .bind(product_id)
        .fetch_all(pool)
        .await?;
        targets.extend(ids);
    }
    if include_library {
        let kind: Option<String> = sqlx::query_scalar("SELECT kind FROM products WHERE id = $1")
            .bind(product_id)
            .fetch_optional(pool)
            .await?;
        if kind.as_deref() != Some("library") {
            let ws: Option<Uuid> =
                sqlx::query_scalar("SELECT workspace_id FROM products WHERE id = $1")
                    .bind(product_id)
                    .fetch_optional(pool)
                    .await?;
            if let Some(ws) = ws {
                let libs: Vec<Uuid> = sqlx::query_scalar(
                    "SELECT p.current_version_id FROM products p
                     JOIN product_versions pv ON pv.id = p.current_version_id
                     WHERE p.workspace_id = $1 AND p.kind = 'library'
                       AND pv.status = 'active' AND pv.deleted_at IS NULL",
                )
                .bind(ws)
                .fetch_all(pool)
                .await?;
                targets.extend(libs);
            }
        }
    }
    targets.sort();
    targets.dedup();
    Ok(targets)
}
