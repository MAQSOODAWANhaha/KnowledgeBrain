use sqlx::{PgPool, Row};
use uuid::Uuid;

pub async fn workspace_slug_taken(pool: &PgPool, slug: &str) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM workspaces WHERE slug = $1)")
        .bind(slug)
        .fetch_one(pool)
        .await
}

pub async fn list_tags_for_workspace(
    pool: &PgPool,
    workspace_id: Uuid,
) -> Result<Vec<crate::Tag>, sqlx::Error> {
    let rows = sqlx::query("SELECT id, name, slug FROM tags WHERE workspace_id = $1 ORDER BY name")
        .bind(workspace_id)
        .fetch_all(pool)
        .await?;
    let mut out = Vec::new();
    for t in rows {
        let id: Uuid = t.try_get("id")?;
        out.push(crate::Tag {
            id,
            workspace_id,
            name: t.try_get("name")?,
            slug: t.try_get("slug")?,
        });
    }
    Ok(out)
}

pub async fn load_tag(pool: &PgPool, tag_id: Uuid) -> Result<Option<crate::Tag>, sqlx::Error> {
    let t = sqlx::query("SELECT id, workspace_id, name, slug FROM tags WHERE id = $1")
        .bind(tag_id)
        .fetch_optional(pool)
        .await?;
    let Some(t) = t else {
        return Ok(None);
    };
    Ok(Some(crate::Tag {
        id: t.try_get("id")?,
        workspace_id: t.try_get("workspace_id")?,
        name: t.try_get("name")?,
        slug: t.try_get("slug")?,
    }))
}

pub async fn tags_in_workspace(
    pool: &PgPool,
    workspace_id: Uuid,
    tag_ids: &[Uuid],
) -> Result<Vec<Uuid>, sqlx::Error> {
    if tag_ids.is_empty() {
        return Ok(Vec::new());
    }
    sqlx::query_scalar("SELECT id FROM tags WHERE workspace_id = $1 AND id = ANY($2)")
        .bind(workspace_id)
        .bind(tag_ids)
        .fetch_all(pool)
        .await
}

pub async fn tags_belong_to_workspace(
    pool: &PgPool,
    workspace_id: Uuid,
    tag_ids: &[Uuid],
) -> Result<bool, sqlx::Error> {
    if tag_ids.is_empty() {
        return Ok(true);
    }
    let mut unique = tag_ids.to_vec();
    unique.sort_unstable();
    unique.dedup();
    let kept = tags_in_workspace(pool, workspace_id, tag_ids).await?;
    Ok(kept.len() == unique.len())
}

fn wiki_page_from_row(p: &sqlx::postgres::PgRow) -> Result<crate::WikiPage, sqlx::Error> {
    let id: Uuid = p.try_get("id")?;
    let vid: Uuid = p.try_get("product_version_id")?;
    let slug: String = p.try_get("slug")?;
    let aliases: Vec<String> = p
        .try_get::<serde_json::Value, _>("aliases")
        .ok()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    let refs: Vec<String> = p
        .try_get::<serde_json::Value, _>("source_refs")
        .ok()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    let chunk_refs: Vec<String> = p
        .try_get::<serde_json::Value, _>("chunk_refs")
        .ok()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    let path: Vec<String> = p
        .try_get::<serde_json::Value, _>("category_path")
        .ok()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    Ok(crate::WikiPage {
        id,
        product_version_id: vid,
        slug,
        title: p.try_get("title")?,
        content: p.try_get("content").unwrap_or_default(),
        page_type: p.try_get("page_type").unwrap_or_else(|_| "summary".into()),
        status: p.try_get("status").unwrap_or_else(|_| "draft".into()),
        summary: p.try_get("summary").unwrap_or_default(),
        aliases,
        source_refs: refs
            .into_iter()
            .filter_map(|s| Uuid::parse_str(&s).ok())
            .collect(),
        chunk_refs: chunk_refs
            .into_iter()
            .filter_map(|s| Uuid::parse_str(&s).ok())
            .collect(),
        category_path: path,
        folder_id: p.try_get::<Option<Uuid>, _>("folder_id").ok().flatten(),
    })
}

pub async fn list_wiki_pages(
    pool: &PgPool,
    version_id: Uuid,
) -> Result<Vec<crate::WikiPage>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, product_version_id, slug, title, content, page_type, status,
                COALESCE(summary, '') AS summary, aliases, source_refs,
                COALESCE(chunk_refs, '[]'::jsonb) AS chunk_refs, category_path, folder_id
         FROM wiki_pages WHERE product_version_id = $1 AND deleted_at IS NULL
         ORDER BY slug",
    )
    .bind(version_id)
    .fetch_all(pool)
    .await?;
    rows.iter().map(wiki_page_from_row).collect()
}

pub async fn load_wiki_page(
    pool: &PgPool,
    version_id: Uuid,
    slug: &str,
) -> Result<Option<crate::WikiPage>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, product_version_id, slug, title, content, page_type, status,
                COALESCE(summary, '') AS summary, aliases, source_refs,
                COALESCE(chunk_refs, '[]'::jsonb) AS chunk_refs, category_path, folder_id
         FROM wiki_pages WHERE product_version_id = $1 AND slug = $2 AND deleted_at IS NULL",
    )
    .bind(version_id)
    .bind(slug)
    .fetch_optional(pool)
    .await?;
    row.as_ref().map(wiki_page_from_row).transpose()
}

fn api_key_from_row(r: sqlx::postgres::PgRow) -> Result<crate::ApiKey, sqlx::Error> {
    Ok(crate::ApiKey {
        id: r.try_get("id")?,
        name: r.try_get("name")?,
        key_hash: r.try_get("key_hash")?,
        prefix: r.try_get("prefix")?,
        scope_type: r.try_get("scope_type")?,
        scope_id: r.try_get("scope_id")?,
        scopes: r.try_get("scopes")?,
    })
}

pub async fn load_api_key(pool: &PgPool, id: Uuid) -> Result<Option<crate::ApiKey>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, name, key_hash, prefix, scope_type, scope_id, scopes FROM api_keys WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    row.map(api_key_from_row).transpose()
}

pub async fn list_api_keys_for_workspace(
    pool: &PgPool,
    workspace_id: Uuid,
) -> Result<Vec<crate::ApiKey>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, name, key_hash, prefix, scope_type, scope_id, scopes
         FROM api_keys
         WHERE (scope_type = 'workspace' AND scope_id = $1)
            OR (scope_type = 'product' AND scope_id IN (
                    SELECT id FROM products WHERE workspace_id = $1
                ))
         ORDER BY prefix",
    )
    .bind(workspace_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(api_key_from_row).collect()
}
