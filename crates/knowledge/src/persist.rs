use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct SeededWorkspace {
    pub workspace_id: Uuid,
    pub library_id: Uuid,
    pub library_version_id: Uuid,
}

pub async fn create_workspace_with_library(
    pool: &PgPool,
    owner_id: Uuid,
    name: &str,
    slug: &str,
) -> Result<SeededWorkspace, sqlx::Error> {
    let workspace_id = Uuid::new_v4();
    let library_id = Uuid::new_v4();
    let library_version_id = Uuid::new_v4();
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO workspaces (id, name, slug, kind) VALUES ($1, $2, $3, 'product_line')",
    )
    .bind(workspace_id)
    .bind(name)
    .bind(slug)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, 'owner')",
    )
    .bind(workspace_id)
    .bind(owner_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO products (id, workspace_id, kind, name, slug, current_version_id)
         VALUES ($1, $2, 'library', '公司资料', 'library', $3)",
    )
    .bind(library_id)
    .bind(workspace_id)
    .bind(library_version_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO product_versions (id, product_id, label, status)
         VALUES ($1, $2, 'current', 'active')",
    )
    .bind(library_version_id)
    .bind(library_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(SeededWorkspace {
        workspace_id,
        library_id,
        library_version_id,
    })
}

pub async fn insert_workspace(
    pool: &PgPool,
    id: Uuid,
    name: &str,
    slug: &str,
) -> Result<(), sqlx::Error> {
    insert_workspace_kind(pool, id, name, slug, "product_line").await
}

pub async fn insert_workspace_kind(
    pool: &PgPool,
    id: Uuid,
    name: &str,
    slug: &str,
    kind: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO workspaces (id, name, slug, kind) VALUES ($1, $2, $3, $4)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(id)
    .bind(name)
    .bind(slug)
    .bind(kind)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn ensure_company_workspace(pool: &PgPool) -> Result<Uuid, sqlx::Error> {
    if let Some(id) =
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM workspaces WHERE kind = 'company' LIMIT 1")
            .fetch_optional(pool)
            .await?
    {
        return Ok(id);
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO workspaces (id, name, slug, kind) VALUES ($1, $2, 'company', 'company')
         ON CONFLICT (slug) DO UPDATE SET kind = 'company'
         RETURNING id",
    )
    .bind(id)
    .bind("公司资料")
    .fetch_optional(pool)
    .await?;
    let id = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM workspaces WHERE slug = 'company' OR kind = 'company' LIMIT 1",
    )
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn list_workspace_ids(pool: &PgPool) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar("SELECT id FROM workspaces ORDER BY created_at")
        .fetch_all(pool)
        .await
}

fn workspace_from_row(r: sqlx::postgres::PgRow) -> Result<crate::Workspace, sqlx::Error> {
    let id: Uuid = r.try_get("id")?;
    let retrieval: crate::RetrievalConfig = r
        .try_get::<serde_json::Value, _>("retrieval_config")
        .ok()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    let kind_s: String = r.try_get("kind").unwrap_or_else(|_| "product_line".into());
    Ok(crate::Workspace {
        id,
        name: r.try_get("name")?,
        slug: r.try_get("slug")?,
        kind: crate::WorkspaceKind::parse(&kind_s),
        retrieval,
    })
}

pub async fn list_workspaces(pool: &PgPool) -> Result<Vec<crate::Workspace>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, name, slug, retrieval_config,
                COALESCE(kind, 'product_line') AS kind
         FROM workspaces ORDER BY created_at",
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(workspace_from_row).collect()
}

pub async fn load_workspace(
    pool: &PgPool,
    workspace_id: Uuid,
) -> Result<Option<crate::Workspace>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, name, slug, retrieval_config,
                COALESCE(kind, 'product_line') AS kind
         FROM workspaces WHERE id = $1",
    )
    .bind(workspace_id)
    .fetch_optional(pool)
    .await?;
    row.map(workspace_from_row).transpose()
}

pub async fn list_products_in_workspace(
    pool: &PgPool,
    workspace_id: Uuid,
) -> Result<Vec<crate::Product>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, kind, name, slug, current_version_id FROM products WHERE workspace_id = $1",
    )
    .bind(workspace_id)
    .fetch_all(pool)
    .await?;
    let mut out = Vec::new();
    for p in rows {
        let id: Uuid = p.try_get("id")?;
        let kind: String = p.try_get("kind")?;
        out.push(crate::Product {
            id,
            workspace_id,
            kind: parse_kind(&kind),
            name: p.try_get("name")?,
            slug: p.try_get("slug")?,
            current_version_id: p.try_get("current_version_id")?,
            embedding_model_id: String::new(),
        });
    }
    Ok(out)
}

pub async fn load_product(
    pool: &PgPool,
    product_id: Uuid,
) -> Result<Option<crate::Product>, sqlx::Error> {
    let p = sqlx::query(
        "SELECT id, workspace_id, kind, name, slug, current_version_id FROM products WHERE id = $1",
    )
    .bind(product_id)
    .fetch_optional(pool)
    .await?;
    let Some(p) = p else {
        return Ok(None);
    };
    let kind: String = p.try_get("kind")?;
    Ok(Some(crate::Product {
        id: p.try_get("id")?,
        workspace_id: p.try_get("workspace_id")?,
        kind: parse_kind(&kind),
        name: p.try_get("name")?,
        slug: p.try_get("slug")?,
        current_version_id: p.try_get("current_version_id")?,
        embedding_model_id: String::new(),
    }))
}

pub async fn product_slug_taken(
    pool: &PgPool,
    workspace_id: Uuid,
    slug: &str,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM products WHERE workspace_id = $1 AND slug = $2)",
    )
    .bind(workspace_id)
    .bind(slug)
    .fetch_one(pool)
    .await
}

pub async fn list_members_for_workspace(
    pool: &PgPool,
    workspace_id: Uuid,
) -> Result<Vec<(Uuid, String)>, sqlx::Error> {
    sqlx::query_as("SELECT user_id, role FROM workspace_members WHERE workspace_id = $1")
        .bind(workspace_id)
        .fetch_all(pool)
        .await
}

pub async fn list_versions_for_product(
    pool: &PgPool,
    product_id: Uuid,
) -> Result<Vec<crate::ProductVersion>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, label, status, cloned_from_version_id, indexing_strategy,
                image_processing_config, chunking_config,
                embedding_model_id, summary_model_id, asr_model_id, asr_config,
                extract_config, wiki_config, question_generation_config
         FROM product_versions WHERE product_id = $1 AND deleted_at IS NULL",
    )
    .bind(product_id)
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|row| product_version_from_row(product_id, row))
        .collect()
}

pub async fn load_version(
    pool: &PgPool,
    version_id: Uuid,
) -> Result<Option<crate::ProductVersion>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, product_id, label, status, cloned_from_version_id, indexing_strategy,
                image_processing_config, chunking_config,
                embedding_model_id, summary_model_id, asr_model_id, asr_config,
                extract_config, wiki_config, question_generation_config
         FROM product_versions WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(version_id)
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let product_id: Uuid = row.try_get("product_id")?;
    Ok(Some(product_version_from_row(product_id, &row)?))
}

pub async fn resolve_product_version_id(
    pool: &PgPool,
    product_id: Uuid,
    version_id: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    if version_id == "current" {
        let product = load_product(pool, product_id).await?;
        return Ok(product.and_then(|p| p.current_version_id));
    }
    let Ok(vid) = Uuid::parse_str(version_id) else {
        return Ok(None);
    };
    let version = load_version(pool, vid).await?;
    Ok(version.filter(|v| v.product_id == product_id).map(|v| v.id))
}

pub async fn clear_product_current_if(
    pool: &PgPool,
    product_id: Uuid,
    version_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE products SET current_version_id = NULL WHERE id = $1 AND current_version_id = $2",
    )
    .bind(product_id)
    .bind(version_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn version_label_taken(
    pool: &PgPool,
    product_id: Uuid,
    label: &str,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM product_versions
            WHERE product_id = $1 AND label = $2 AND status <> 'archived' AND deleted_at IS NULL
         )",
    )
    .bind(product_id)
    .bind(label)
    .fetch_one(pool)
    .await
}

pub async fn workspace_embedding_conflict(
    pool: &PgPool,
    workspace_id: Uuid,
    incoming: &str,
) -> Result<Option<String>, sqlx::Error> {
    if incoming.is_empty() {
        return Ok(None);
    }
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT pv.embedding_model_id
         FROM products p
         JOIN product_versions pv ON pv.id = p.current_version_id
         WHERE p.workspace_id = $1 AND p.kind = 'product'
           AND COALESCE(pv.embedding_model_id, '') <> ''
         LIMIT 1",
    )
    .bind(workspace_id)
    .fetch_optional(pool)
    .await?;
    Ok(existing.filter(|have| have != incoming).map(|have| {
        format!("workspace products must share embedding_model_id (have {have}, got {incoming})")
    }))
}

fn document_from_row(d: sqlx::postgres::PgRow) -> Result<crate::Document, sqlx::Error> {
    let did: Uuid = d.try_get("id")?;
    let st: String = d.try_get("parse_status")?;
    let title: String = d.try_get("title")?;
    let file_name: String = d.try_get("file_name")?;
    let file_size: i64 = d.try_get("file_size")?;
    let file_hash: String = d.try_get("file_hash")?;
    let object_ref: String = d.try_get("object_ref")?;
    let vid: Uuid = d.try_get("product_version_id")?;
    let mut doc = crate::Document::new(vid, title, file_name, file_size, file_hash, object_ref);
    doc.id = did;
    doc.parse_status = parse_parse_status(&st);
    doc.enable_status = d.try_get("enable_status")?;
    doc.pending_subtasks_count = d.try_get("pending_subtasks_count")?;
    doc.error_message = d.try_get("error_message")?;
    doc.attempt = d.try_get("attempt").unwrap_or(1);
    doc.description = d.try_get("description").unwrap_or_default();
    if let Ok(sum) = d.try_get::<String, _>("summary_status") {
        doc.summary_status = match sum.as_str() {
            "pending" => crate::SummaryStatus::Pending,
            "processing" => crate::SummaryStatus::Processing,
            "completed" => crate::SummaryStatus::Completed,
            "failed" => crate::SummaryStatus::Failed,
            _ => crate::SummaryStatus::None,
        };
    }
    doc.index_ready = d.try_get("index_ready").unwrap_or(false);
    doc.doc_type = d.try_get("doc_type").unwrap_or_else(|_| "file".into());
    if let Ok(Some(raw)) = d.try_get::<Option<serde_json::Value>, _>("source_passages") {
        doc.source_passages = serde_json::from_value(raw).unwrap_or_default();
    }
    if let Ok(Some(raw)) = d.try_get::<Option<serde_json::Value>, _>("process_overrides") {
        doc.process_overrides = serde_json::from_value(raw).ok();
    }
    Ok(doc)
}

pub async fn list_documents_in_version(
    pool: &PgPool,
    version_id: Uuid,
    parse_status: Option<&str>,
    keyword: Option<&str>,
    tag_id: Option<Uuid>,
) -> Result<Vec<crate::Document>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, title, file_name, file_size, file_hash, object_ref,
                parse_status, enable_status, pending_subtasks_count,
                COALESCE(error_message, '') AS error_message,
                process_overrides,
                COALESCE(type, 'file') AS doc_type,
                COALESCE(attempt, 1) AS attempt,
                COALESCE(description, '') AS description,
                COALESCE(summary_status, 'none') AS summary_status,
                source_passages, index_ready, product_version_id
         FROM documents
         WHERE product_version_id = $1 AND deleted_at IS NULL
           AND ($2::text IS NULL OR parse_status = $2)
           AND (
             $3::text IS NULL
             OR title ILIKE '%' || $3 || '%'
             OR file_name ILIKE '%' || $3 || '%'
             OR COALESCE(description, '') ILIKE '%' || $3 || '%'
           )
           AND (
             $4::uuid IS NULL
             OR EXISTS (
                SELECT 1 FROM document_tags dt
                WHERE dt.document_id = documents.id AND dt.tag_id = $4
             )
           )
         ORDER BY created_at DESC",
    )
    .bind(version_id)
    .bind(parse_status)
    .bind(keyword)
    .bind(tag_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(document_from_row).collect()
}

pub async fn load_document(
    pool: &PgPool,
    document_id: Uuid,
) -> Result<Option<crate::Document>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, title, file_name, file_size, file_hash, object_ref,
                parse_status, enable_status, pending_subtasks_count,
                COALESCE(error_message, '') AS error_message,
                process_overrides,
                COALESCE(type, 'file') AS doc_type,
                COALESCE(attempt, 1) AS attempt,
                COALESCE(description, '') AS description,
                COALESCE(summary_status, 'none') AS summary_status,
                source_passages, index_ready, product_version_id
         FROM documents WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(document_id)
    .fetch_optional(pool)
    .await?;
    row.map(document_from_row).transpose()
}

pub async fn company_workspace_id(pool: &PgPool) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar("SELECT id FROM workspaces WHERE kind = 'company' LIMIT 1")
        .fetch_optional(pool)
        .await
}

pub async fn is_frozen_default_library(
    pool: &PgPool,
    product_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let row = sqlx::query(
        "SELECT p.kind, p.slug, w.kind AS ws_kind
         FROM products p JOIN workspaces w ON w.id = p.workspace_id
         WHERE p.id = $1",
    )
    .bind(product_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.is_some_and(|r| {
        r.get::<String, _>("kind") == "library"
            && r.get::<String, _>("slug") == "library"
            && r.get::<String, _>("ws_kind") == "product_line"
    }))
}

pub async fn insert_member(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    role: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, $3)
         ON CONFLICT DO NOTHING",
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(role)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn insert_user(
    pool: &PgPool,
    id: Uuid,
    email: &str,
    password_hash: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO users (id, email, password_hash) VALUES ($1, $2, $3)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(id)
    .bind(email)
    .bind(password_hash)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn insert_product(
    pool: &PgPool,
    id: Uuid,
    workspace_id: Uuid,
    kind: &str,
    name: &str,
    slug: &str,
    current_version_id: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO products (id, workspace_id, kind, name, slug, current_version_id)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(id)
    .bind(workspace_id)
    .bind(kind)
    .bind(name)
    .bind(slug)
    .bind(current_version_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_models(pool: &PgPool) -> Result<Vec<(String, String, i32)>, sqlx::Error> {
    let rows = sqlx::query("SELECT id, kind, COALESCE(dimension, 0) FROM models ORDER BY id")
        .fetch_all(pool)
        .await?;
    let mut out = Vec::new();
    for r in rows {
        out.push((r.try_get(0)?, r.try_get(1)?, r.try_get(2)?));
    }
    Ok(out)
}

pub async fn upsert_model(
    pool: &PgPool,
    id: &str,
    kind: &str,
    dimension: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO models (id, kind, dimension) VALUES ($1,$2,$3)
         ON CONFLICT (id) DO UPDATE SET kind = EXCLUDED.kind, dimension = EXCLUDED.dimension",
    )
    .bind(id)
    .bind(kind)
    .bind(dimension)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_product_current(
    pool: &PgPool,
    product_id: Uuid,
    version_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE products SET current_version_id = $2 WHERE id = $1")
        .bind(product_id)
        .bind(version_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn insert_version(
    pool: &PgPool,
    id: Uuid,
    product_id: Uuid,
    label: &str,
    status: &str,
    cloned_from: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO product_versions (
            id, product_id, label, status, cloned_from_version_id,
            embedding_model_id, summary_model_id
         ) VALUES ($1, $2, $3, $4, $5, 'stub-emb', 'stub-chat')
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(id)
    .bind(product_id)
    .bind(label)
    .bind(status)
    .bind(cloned_from)
    .execute(pool)
    .await?;
    Ok(())
}

/// INSERT target version `cloning` with all config columns copied from source.
fn parse_role(s: &str) -> crate::Role {
    match s {
        "owner" => crate::Role::Owner,
        "admin" => crate::Role::Admin,
        "contributor" => crate::Role::Contributor,
        _ => crate::Role::Viewer,
    }
}

fn parse_kind(s: &str) -> crate::ProductKind {
    if s == "library" {
        crate::ProductKind::Library
    } else {
        crate::ProductKind::Product
    }
}

fn parse_version_status(s: &str) -> crate::VersionStatus {
    match s {
        "cloning" => crate::VersionStatus::Cloning,
        "archived" => crate::VersionStatus::Archived,
        "failed" => crate::VersionStatus::Failed,
        _ => crate::VersionStatus::Active,
    }
}

fn parse_parse_status(s: &str) -> crate::ParseStatus {
    match s {
        "processing" => crate::ParseStatus::Processing,
        "finalizing" => crate::ParseStatus::Finalizing,
        "completed" => crate::ParseStatus::Completed,
        "failed" => crate::ParseStatus::Failed,
        "cancelled" => crate::ParseStatus::Cancelled,
        "deleting" => crate::ParseStatus::Deleting,
        _ => crate::ParseStatus::Pending,
    }
}

/// Load one workspace (members, products, versions, documents, tags,
/// graph, wiki, chunks) into `store`.
pub async fn hydrate_workspace(
    pool: &PgPool,
    store: &mut crate::Store,
    workspace_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, name, slug, retrieval_config,
                COALESCE(kind, 'product_line') AS kind
         FROM workspaces WHERE id = $1",
    )
    .bind(workspace_id)
    .fetch_optional(pool)
    .await?;
    let Some(r) = row else {
        return Ok(false);
    };
    let retrieval: crate::RetrievalConfig = r
        .try_get::<serde_json::Value, _>("retrieval_config")
        .ok()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    let kind_s: String = r.try_get("kind").unwrap_or_else(|_| "product_line".into());
    store.workspaces.insert(
        workspace_id,
        crate::Workspace {
            id: workspace_id,
            name: r.try_get("name")?,
            slug: r.try_get("slug")?,
            kind: crate::WorkspaceKind::parse(&kind_s),
            retrieval,
        },
    );
    let members =
        sqlx::query("SELECT user_id, role FROM workspace_members WHERE workspace_id = $1")
            .bind(workspace_id)
            .fetch_all(pool)
            .await?;
    for m in members {
        let uid: Uuid = m.try_get("user_id")?;
        let role: String = m.try_get("role")?;
        store.members.insert((workspace_id, uid), parse_role(&role));
    }
    let products = sqlx::query(
        "SELECT id, kind, name, slug, current_version_id FROM products WHERE workspace_id = $1",
    )
    .bind(workspace_id)
    .fetch_all(pool)
    .await?;
    let mut product_ids = Vec::new();
    let mut version_ids = Vec::new();
    let mut document_ids = Vec::new();
    for p in products {
        let pid: Uuid = p.try_get("id")?;
        product_ids.push(pid);
        let kind: String = p.try_get("kind")?;
        store.products.insert(
            pid,
            crate::Product {
                id: pid,
                workspace_id,
                kind: parse_kind(&kind),
                name: p.try_get("name")?,
                slug: p.try_get("slug")?,
                current_version_id: p.try_get("current_version_id")?,
                embedding_model_id: String::new(),
            },
        );
    }
    for pid in product_ids {
        let versions = sqlx::query(
            "SELECT id, label, status, cloned_from_version_id, indexing_strategy,
                    image_processing_config, chunking_config,
                    embedding_model_id, summary_model_id, asr_model_id, asr_config,
                    extract_config, wiki_config, question_generation_config
             FROM product_versions WHERE product_id = $1 AND deleted_at IS NULL",
        )
        .bind(pid)
        .fetch_all(pool)
        .await?;
        for v in versions {
            let vid: Uuid = v.try_get("id")?;
            let status: String = v.try_get("status")?;
            let idx: serde_json::Value = v
                .try_get("indexing_strategy")
                .unwrap_or_else(|_| serde_json::json!({}));
            let asr_cfg: serde_json::Value = v
                .try_get("asr_config")
                .unwrap_or_else(|_| serde_json::json!({}));
            let ext_cfg: serde_json::Value = v
                .try_get("extract_config")
                .unwrap_or_else(|_| serde_json::json!({}));
            let mut pv = crate::ProductVersion::new(pid, v.try_get("label")?);
            pv.id = vid;
            pv.status = parse_version_status(&status);
            pv.cloned_from = v.try_get("cloned_from_version_id")?;
            pv.vector_enabled = idx.get("vector").and_then(|x| x.as_bool()).unwrap_or(true);
            pv.keyword_enabled = idx.get("keyword").and_then(|x| x.as_bool()).unwrap_or(true);
            pv.wiki_enabled = idx.get("wiki").and_then(|x| x.as_bool()).unwrap_or(true);
            pv.graph_enabled = idx.get("graph").and_then(|x| x.as_bool()).unwrap_or(true);
            let img_cfg: serde_json::Value = v
                .try_get("image_processing_config")
                .unwrap_or_else(|_| serde_json::json!({}));
            pv.enable_multimodel = img_cfg
                .get("enable_multimodel")
                .and_then(|x| x.as_bool())
                .or_else(|| idx.get("multimodal").and_then(|x| x.as_bool()))
                .unwrap_or(true);
            pv.extract_enabled = ext_cfg
                .get("enabled")
                .and_then(|x| x.as_bool())
                .unwrap_or(true);
            pv.asr_enabled = asr_cfg
                .get("enabled")
                .and_then(|x| x.as_bool())
                .unwrap_or(false);
            if let Ok(Some(m)) = v.try_get::<Option<String>, _>("embedding_model_id") {
                pv.embedding_model_id = m;
            }
            if let Ok(Some(m)) = v.try_get::<Option<String>, _>("summary_model_id") {
                pv.summary_model_id = m;
            }
            if let Ok(Some(m)) = v.try_get::<Option<String>, _>("asr_model_id") {
                pv.asr_model_id = m;
            }
            let wiki_cfg: serde_json::Value = v
                .try_get("wiki_config")
                .unwrap_or_else(|_| serde_json::json!({}));
            if let Some(m) = wiki_cfg
                .get("synthesis_model_id")
                .and_then(|x| x.as_str())
                .filter(|s| !s.is_empty())
            {
                pv.wiki_synthesis_model_id = m.to_string();
            }
            let q_cfg: serde_json::Value = v
                .try_get("question_generation_config")
                .unwrap_or_else(|_| serde_json::json!({}));
            if let Some(b) = q_cfg.get("enabled").and_then(|x| x.as_bool()) {
                pv.question_enabled = b;
            }
            if let Some(n) = q_cfg.get("question_count").and_then(|x| x.as_u64()) {
                pv.question_count = n as usize;
            }
            if let Some(s) = q_cfg.get("custom_instructions").and_then(|x| x.as_str()) {
                pv.question_custom_instructions = s.to_string();
            }
            let chunk_cfg: serde_json::Value = v
                .try_get("chunking_config")
                .unwrap_or_else(|_| serde_json::json!({}));
            if let Some(n) = chunk_cfg.get("chunk_size").and_then(|x| x.as_u64()) {
                pv.chunk_size = n as usize;
            }
            if let Some(n) = chunk_cfg.get("chunk_overlap").and_then(|x| x.as_u64()) {
                pv.chunk_overlap = n as usize;
            }
            if let Some(s) = chunk_cfg.get("strategy").and_then(|x| x.as_str()) {
                pv.chunk_strategy = s.to_string();
            }
            pv.enable_parent_child = chunk_cfg
                .get("enable_parent_child")
                .and_then(|x| x.as_bool())
                .unwrap_or(false);
            if let Some(n) = chunk_cfg.get("parent_chunk_size").and_then(|x| x.as_u64()) {
                pv.parent_chunk_size = n as usize;
            }
            if let Some(n) = chunk_cfg.get("child_chunk_size").and_then(|x| x.as_u64()) {
                pv.child_chunk_size = n as usize;
            }
            if let Some(seps) = chunk_cfg.get("separators").and_then(|x| x.as_array()) {
                pv.chunk_separators = seps
                    .iter()
                    .filter_map(|s| s.as_str().map(str::to_string))
                    .collect();
            }
            if let Some(n) = chunk_cfg.get("token_limit").and_then(|x| x.as_u64()) {
                pv.chunk_token_limit = n as usize;
            }
            if let Some(langs) = chunk_cfg.get("languages").and_then(|x| x.as_array()) {
                pv.chunk_languages = langs
                    .iter()
                    .filter_map(|s| s.as_str().map(str::to_string))
                    .collect();
            }
            if let Some(rules) = chunk_cfg
                .get("parser_engine_rules")
                .and_then(|x| x.as_array())
            {
                pv.parser_engine_rules = rules
                    .iter()
                    .filter_map(|r| serde_json::from_value(r.clone()).ok())
                    .collect();
            }
            if let Some(s) = chunk_cfg
                .get("table_metadata_instructions")
                .and_then(|x| x.as_str())
            {
                pv.table_metadata_instructions = s.to_string();
            }
            store.versions.insert(vid, pv);
            version_ids.push(vid);
            let docs = sqlx::query(
                "SELECT id, title, file_name, file_size, file_hash, object_ref,
                        parse_status, enable_status, pending_subtasks_count,
                        COALESCE(error_message, '') AS error_message,
                        process_overrides,
                        COALESCE(type, 'file') AS doc_type,
                        COALESCE(attempt, 1) AS attempt,
                        COALESCE(description, '') AS description,
                        COALESCE(summary_status, 'none') AS summary_status,
                        source_passages,
                        COALESCE(index_ready, false) AS index_ready
                 FROM documents WHERE product_version_id = $1 AND deleted_at IS NULL",
            )
            .bind(vid)
            .fetch_all(pool)
            .await?;
            for d in docs {
                let did: Uuid = d.try_get("id")?;
                let st: String = d.try_get("parse_status")?;
                let title: String = d.try_get("title")?;
                let file_name: String = d.try_get("file_name")?;
                let file_size: i64 = d.try_get("file_size")?;
                let file_hash: String = d.try_get("file_hash")?;
                let object_ref: String = d.try_get("object_ref")?;
                let mut doc =
                    crate::Document::new(vid, title, file_name, file_size, file_hash, object_ref);
                doc.id = did;
                doc.parse_status = parse_parse_status(&st);
                doc.enable_status = d.try_get("enable_status")?;
                doc.pending_subtasks_count = d.try_get("pending_subtasks_count")?;
                doc.error_message = d.try_get("error_message")?;
                doc.attempt = d.try_get("attempt").unwrap_or(1);
                doc.description = d.try_get("description").unwrap_or_default();
                if let Ok(st) = d.try_get::<String, _>("summary_status") {
                    doc.summary_status = match st.as_str() {
                        "pending" => crate::SummaryStatus::Pending,
                        "processing" => crate::SummaryStatus::Processing,
                        "completed" => crate::SummaryStatus::Completed,
                        "failed" => crate::SummaryStatus::Failed,
                        _ => crate::SummaryStatus::None,
                    };
                }
                doc.index_ready = d.try_get("index_ready").unwrap_or(false);
                doc.doc_type = d.try_get("doc_type").unwrap_or_else(|_| "file".into());
                if let Ok(Some(raw)) = d.try_get::<Option<serde_json::Value>, _>("source_passages")
                {
                    doc.source_passages = serde_json::from_value(raw).unwrap_or_default();
                }
                if let Ok(Some(raw)) =
                    d.try_get::<Option<serde_json::Value>, _>("process_overrides")
                {
                    doc.process_overrides = serde_json::from_value(raw).ok();
                }
                store.documents.insert(did, doc);
                document_ids.push(did);
            }
        }
    }
    hydrate_workspace_index(pool, store, workspace_id, &version_ids, &document_ids).await?;
    Ok(true)
}

/// Load one document plus its version/product/workspace and that document's chunks.
pub async fn hydrate_document(
    pool: &PgPool,
    store: &mut crate::Store,
    document_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let meta = sqlx::query(
        "SELECT d.product_version_id, pv.product_id, p.workspace_id
         FROM documents d
         JOIN product_versions pv ON pv.id = d.product_version_id
         JOIN products p ON p.id = pv.product_id
         WHERE d.id = $1 AND d.deleted_at IS NULL",
    )
    .bind(document_id)
    .fetch_optional(pool)
    .await?;
    let Some(meta) = meta else {
        return Ok(false);
    };
    let version_id: Uuid = meta.try_get("product_version_id")?;
    let product_id: Uuid = meta.try_get("product_id")?;
    let workspace_id: Uuid = meta.try_get("workspace_id")?;
    hydrate_scope(
        pool,
        store,
        workspace_id,
        product_id,
        version_id,
        &[document_id],
    )
    .await?;
    Ok(true)
}

/// Load one product version, its documents, and wiki/chunk index for those docs.
pub async fn hydrate_version(
    pool: &PgPool,
    store: &mut crate::Store,
    version_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let meta = sqlx::query(
        "SELECT pv.product_id, p.workspace_id
         FROM product_versions pv
         JOIN products p ON p.id = pv.product_id
         WHERE pv.id = $1 AND pv.deleted_at IS NULL",
    )
    .bind(version_id)
    .fetch_optional(pool)
    .await?;
    let Some(meta) = meta else {
        return Ok(false);
    };
    let product_id: Uuid = meta.try_get("product_id")?;
    let workspace_id: Uuid = meta.try_get("workspace_id")?;
    let document_ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM documents WHERE product_version_id = $1 AND deleted_at IS NULL",
    )
    .bind(version_id)
    .fetch_all(pool)
    .await?;
    hydrate_scope(
        pool,
        store,
        workspace_id,
        product_id,
        version_id,
        &document_ids,
    )
    .await?;
    Ok(true)
}

fn product_version_from_row(
    product_id: Uuid,
    v: &sqlx::postgres::PgRow,
) -> Result<crate::ProductVersion, sqlx::Error> {
    let vid: Uuid = v.try_get("id")?;
    let status: String = v.try_get("status")?;
    let idx: serde_json::Value = v
        .try_get("indexing_strategy")
        .unwrap_or_else(|_| serde_json::json!({}));
    let asr_cfg: serde_json::Value = v
        .try_get("asr_config")
        .unwrap_or_else(|_| serde_json::json!({}));
    let ext_cfg: serde_json::Value = v
        .try_get("extract_config")
        .unwrap_or_else(|_| serde_json::json!({}));
    let mut pv = crate::ProductVersion::new(product_id, v.try_get("label")?);
    pv.id = vid;
    pv.status = parse_version_status(&status);
    pv.cloned_from = v.try_get("cloned_from_version_id")?;
    pv.vector_enabled = idx.get("vector").and_then(|x| x.as_bool()).unwrap_or(true);
    pv.keyword_enabled = idx.get("keyword").and_then(|x| x.as_bool()).unwrap_or(true);
    pv.wiki_enabled = idx.get("wiki").and_then(|x| x.as_bool()).unwrap_or(true);
    pv.graph_enabled = idx.get("graph").and_then(|x| x.as_bool()).unwrap_or(true);
    let img_cfg: serde_json::Value = v
        .try_get("image_processing_config")
        .unwrap_or_else(|_| serde_json::json!({}));
    pv.enable_multimodel = img_cfg
        .get("enable_multimodel")
        .and_then(|x| x.as_bool())
        .or_else(|| idx.get("multimodal").and_then(|x| x.as_bool()))
        .unwrap_or(true);
    pv.extract_enabled = ext_cfg
        .get("enabled")
        .and_then(|x| x.as_bool())
        .unwrap_or(true);
    pv.asr_enabled = asr_cfg
        .get("enabled")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    if let Ok(Some(m)) = v.try_get::<Option<String>, _>("embedding_model_id") {
        pv.embedding_model_id = m;
    }
    if let Ok(Some(m)) = v.try_get::<Option<String>, _>("summary_model_id") {
        pv.summary_model_id = m;
    }
    if let Ok(Some(m)) = v.try_get::<Option<String>, _>("asr_model_id") {
        pv.asr_model_id = m;
    }
    let wiki_cfg: serde_json::Value = v
        .try_get("wiki_config")
        .unwrap_or_else(|_| serde_json::json!({}));
    if let Some(m) = wiki_cfg
        .get("synthesis_model_id")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
    {
        pv.wiki_synthesis_model_id = m.to_string();
    }
    let q_cfg: serde_json::Value = v
        .try_get("question_generation_config")
        .unwrap_or_else(|_| serde_json::json!({}));
    if let Some(b) = q_cfg.get("enabled").and_then(|x| x.as_bool()) {
        pv.question_enabled = b;
    }
    if let Some(n) = q_cfg.get("question_count").and_then(|x| x.as_u64()) {
        pv.question_count = n as usize;
    }
    if let Some(s) = q_cfg.get("custom_instructions").and_then(|x| x.as_str()) {
        pv.question_custom_instructions = s.to_string();
    }
    let chunk_cfg: serde_json::Value = v
        .try_get("chunking_config")
        .unwrap_or_else(|_| serde_json::json!({}));
    if let Some(n) = chunk_cfg.get("chunk_size").and_then(|x| x.as_u64()) {
        pv.chunk_size = n as usize;
    }
    if let Some(n) = chunk_cfg.get("chunk_overlap").and_then(|x| x.as_u64()) {
        pv.chunk_overlap = n as usize;
    }
    if let Some(s) = chunk_cfg.get("strategy").and_then(|x| x.as_str()) {
        pv.chunk_strategy = s.to_string();
    }
    pv.enable_parent_child = chunk_cfg
        .get("enable_parent_child")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    if let Some(n) = chunk_cfg.get("parent_chunk_size").and_then(|x| x.as_u64()) {
        pv.parent_chunk_size = n as usize;
    }
    if let Some(n) = chunk_cfg.get("child_chunk_size").and_then(|x| x.as_u64()) {
        pv.child_chunk_size = n as usize;
    }
    if let Some(seps) = chunk_cfg.get("separators").and_then(|x| x.as_array()) {
        pv.chunk_separators = seps
            .iter()
            .filter_map(|s| s.as_str().map(str::to_string))
            .collect();
    }
    if let Some(n) = chunk_cfg.get("token_limit").and_then(|x| x.as_u64()) {
        pv.chunk_token_limit = n as usize;
    }
    if let Some(langs) = chunk_cfg.get("languages").and_then(|x| x.as_array()) {
        pv.chunk_languages = langs
            .iter()
            .filter_map(|s| s.as_str().map(str::to_string))
            .collect();
    }
    if let Some(rules) = chunk_cfg
        .get("parser_engine_rules")
        .and_then(|x| x.as_array())
    {
        pv.parser_engine_rules = rules
            .iter()
            .filter_map(|r| serde_json::from_value(r.clone()).ok())
            .collect();
    }
    if let Some(s) = chunk_cfg
        .get("table_metadata_instructions")
        .and_then(|x| x.as_str())
    {
        pv.table_metadata_instructions = s.to_string();
    }
    Ok(pv)
}

async fn hydrate_scope(
    pool: &PgPool,
    store: &mut crate::Store,
    workspace_id: Uuid,
    product_id: Uuid,
    version_id: Uuid,
    document_ids: &[Uuid],
) -> Result<(), sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, name, slug, retrieval_config,
                COALESCE(kind, 'product_line') AS kind
         FROM workspaces WHERE id = $1",
    )
    .bind(workspace_id)
    .fetch_optional(pool)
    .await?;
    if let Some(r) = row {
        let retrieval: crate::RetrievalConfig = r
            .try_get::<serde_json::Value, _>("retrieval_config")
            .ok()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        let kind_s: String = r.try_get("kind").unwrap_or_else(|_| "product_line".into());
        store.workspaces.insert(
            workspace_id,
            crate::Workspace {
                id: workspace_id,
                name: r.try_get("name")?,
                slug: r.try_get("slug")?,
                kind: crate::WorkspaceKind::parse(&kind_s),
                retrieval,
            },
        );
    }
    if let Some(p) =
        sqlx::query("SELECT id, kind, name, slug, current_version_id FROM products WHERE id = $1")
            .bind(product_id)
            .fetch_optional(pool)
            .await?
    {
        let kind: String = p.try_get("kind")?;
        store.products.insert(
            product_id,
            crate::Product {
                id: product_id,
                workspace_id,
                kind: parse_kind(&kind),
                name: p.try_get("name")?,
                slug: p.try_get("slug")?,
                current_version_id: p.try_get("current_version_id")?,
                embedding_model_id: String::new(),
            },
        );
    }
    if let Some(v) = sqlx::query(
        "SELECT id, label, status, cloned_from_version_id, indexing_strategy,
                image_processing_config, chunking_config,
                embedding_model_id, summary_model_id, asr_model_id, asr_config,
                extract_config, wiki_config, question_generation_config
         FROM product_versions WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(version_id)
    .fetch_optional(pool)
    .await?
    {
        store
            .versions
            .insert(version_id, product_version_from_row(product_id, &v)?);
    }
    if !document_ids.is_empty() {
        let docs = sqlx::query(
            "SELECT id, title, file_name, file_size, file_hash, object_ref,
                    parse_status, enable_status, pending_subtasks_count,
                    COALESCE(error_message, '') AS error_message,
                    process_overrides,
                    COALESCE(type, 'file') AS doc_type,
                    COALESCE(attempt, 1) AS attempt,
                    COALESCE(description, '') AS description,
                    COALESCE(summary_status, 'none') AS summary_status,
                    source_passages, index_ready, product_version_id
             FROM documents WHERE id = ANY($1) AND deleted_at IS NULL",
        )
        .bind(document_ids)
        .fetch_all(pool)
        .await?;
        for d in docs {
            let did: Uuid = d.try_get("id")?;
            let st: String = d.try_get("parse_status")?;
            let title: String = d.try_get("title")?;
            let file_name: String = d.try_get("file_name")?;
            let file_size: i64 = d.try_get("file_size")?;
            let file_hash: String = d.try_get("file_hash")?;
            let object_ref: String = d.try_get("object_ref")?;
            let vid: Uuid = d.try_get("product_version_id")?;
            let mut doc =
                crate::Document::new(vid, title, file_name, file_size, file_hash, object_ref);
            doc.id = did;
            doc.parse_status = parse_parse_status(&st);
            doc.enable_status = d.try_get("enable_status")?;
            doc.pending_subtasks_count = d.try_get("pending_subtasks_count")?;
            doc.error_message = d.try_get("error_message")?;
            doc.attempt = d.try_get("attempt").unwrap_or(1);
            doc.description = d.try_get("description").unwrap_or_default();
            if let Ok(sum) = d.try_get::<String, _>("summary_status") {
                doc.summary_status = match sum.as_str() {
                    "pending" => crate::SummaryStatus::Pending,
                    "processing" => crate::SummaryStatus::Processing,
                    "completed" => crate::SummaryStatus::Completed,
                    "failed" => crate::SummaryStatus::Failed,
                    _ => crate::SummaryStatus::None,
                };
            }
            doc.index_ready = d.try_get("index_ready").unwrap_or(false);
            doc.doc_type = d.try_get("doc_type").unwrap_or_else(|_| "file".into());
            if let Ok(Some(raw)) = d.try_get::<Option<serde_json::Value>, _>("source_passages") {
                doc.source_passages = serde_json::from_value(raw).unwrap_or_default();
            }
            if let Ok(Some(raw)) = d.try_get::<Option<serde_json::Value>, _>("process_overrides") {
                doc.process_overrides = serde_json::from_value(raw).ok();
            }
            store.documents.insert(did, doc);
        }
    }
    hydrate_workspace_index(pool, store, workspace_id, &[version_id], document_ids).await?;
    if let Some(p) = store.products.get_mut(&product_id)
        && let Some(v) = store.versions.get(&version_id)
    {
        p.embedding_model_id = v.embedding_model_id.clone();
    }
    Ok(())
}

async fn hydrate_workspace_index(
    pool: &PgPool,
    store: &mut crate::Store,
    workspace_id: Uuid,
    version_ids: &[Uuid],
    document_ids: &[Uuid],
) -> Result<(), sqlx::Error> {
    let tags = sqlx::query("SELECT id, name, slug FROM tags WHERE workspace_id = $1")
        .bind(workspace_id)
        .fetch_all(pool)
        .await?;
    for t in tags {
        let id: Uuid = t.try_get("id")?;
        store.tags.insert(
            id,
            crate::Tag {
                id,
                workspace_id,
                name: t.try_get("name")?,
                slug: t.try_get("slug")?,
            },
        );
    }
    if !document_ids.is_empty() {
        let links = sqlx::query(
            "SELECT document_id, tag_id FROM document_tags WHERE document_id = ANY($1)",
        )
        .bind(document_ids)
        .fetch_all(pool)
        .await?;
        for l in links {
            let did: Uuid = l.try_get("document_id")?;
            let tid: Uuid = l.try_get("tag_id")?;
            store.document_tags.insert((did, tid));
        }
        let chunks = sqlx::query(
            "SELECT id, document_id, product_version_id, chunk_type, content,
                    context_header, start_at, end_at, parent_chunk_id, generated_questions
             FROM chunks WHERE document_id = ANY($1)",
        )
        .bind(document_ids)
        .fetch_all(pool)
        .await?;
        for c in chunks {
            let id: Uuid = c.try_get("id")?;
            let qs: serde_json::Value = c
                .try_get("generated_questions")
                .unwrap_or_else(|_| serde_json::json!([]));
            let generated: Vec<String> = serde_json::from_value(qs).unwrap_or_default();
            store.chunks.insert(
                id,
                crate::Chunk {
                    id,
                    document_id: c.try_get("document_id")?,
                    product_version_id: c.try_get("product_version_id")?,
                    chunk_type: c.try_get("chunk_type")?,
                    content: c.try_get("content")?,
                    context_header: c.try_get("context_header").unwrap_or_default(),
                    start_at: c.try_get("start_at")?,
                    end_at: c.try_get("end_at")?,
                    parent_chunk_id: c.try_get("parent_chunk_id")?,
                    generated_questions: generated,
                },
            );
        }
        let embs = sqlx::query(
            "SELECT chunk_id, product_version_id, document_id, content, embedding::text AS emb
             FROM chunk_embeddings WHERE document_id = ANY($1)",
        )
        .bind(document_ids)
        .fetch_all(pool)
        .await?;
        for e in embs {
            let cid: Uuid = e.try_get("chunk_id")?;
            let lit: String = e.try_get("emb").unwrap_or_default();
            store.embeddings.insert(
                cid,
                crate::ChunkEmbedding {
                    chunk_id: cid,
                    product_version_id: e.try_get("product_version_id")?,
                    document_id: e.try_get("document_id")?,
                    content: e.try_get("content")?,
                    vector: parse_vector_literal(&lit),
                    tsv: String::new(),
                },
            );
        }
    }
    if !version_ids.is_empty() {
        let nodes = sqlx::query(
            "SELECT product_version_id, document_id, name, chunk_ids
             FROM graph_nodes WHERE product_version_id = ANY($1)",
        )
        .bind(version_ids)
        .fetch_all(pool)
        .await?;
        for n in nodes {
            let vid: Uuid = n.try_get("product_version_id")?;
            let did: Uuid = n.try_get("document_id")?;
            let name: String = n.try_get("name")?;
            let chunk_ids: Vec<Uuid> = n.try_get("chunk_ids").unwrap_or_default();
            store.graph.insert(
                (vid, did, name.clone()),
                crate::GraphNode {
                    version_id: vid,
                    document_id: did,
                    name,
                    chunk_ids,
                },
            );
        }
        let rels = sqlx::query(
            "SELECT product_version_id, document_id, node1, node2, rel_type
             FROM graph_relations WHERE product_version_id = ANY($1)",
        )
        .bind(version_ids)
        .fetch_all(pool)
        .await?;
        for r in rels {
            let vid: Uuid = r.try_get("product_version_id")?;
            let did: Uuid = r.try_get("document_id")?;
            let n1: String = r.try_get("node1")?;
            let n2: String = r.try_get("node2")?;
            let rt: String = r.try_get("rel_type")?;
            store.relations.insert(
                (vid, did, n1.clone(), n2.clone(), rt.clone()),
                crate::GraphRelation {
                    version_id: vid,
                    document_id: did,
                    node1: n1,
                    node2: n2,
                    rel_type: rt,
                },
            );
        }
        let pages = sqlx::query(
            "SELECT id, product_version_id, slug, title, content, page_type, status,
                    COALESCE(summary, '') AS summary, aliases, source_refs,
                    COALESCE(chunk_refs, '[]'::jsonb) AS chunk_refs, category_path, folder_id
             FROM wiki_pages WHERE product_version_id = ANY($1) AND deleted_at IS NULL",
        )
        .bind(version_ids)
        .fetch_all(pool)
        .await?;
        for p in pages {
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
            store.wiki.insert(
                (vid, slug.clone()),
                crate::WikiPage {
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
                },
            );
        }
        let folders = sqlx::query(
            "SELECT id, product_version_id, parent_id, name, path, depth, sort_order
             FROM wiki_folders WHERE product_version_id = ANY($1) AND deleted_at IS NULL",
        )
        .bind(version_ids)
        .fetch_all(pool)
        .await?;
        for f in folders {
            let id: Uuid = f.try_get("id")?;
            store.wiki_folders.insert(
                id,
                crate::WikiFolder {
                    id,
                    product_version_id: f.try_get("product_version_id")?,
                    parent_id: f.try_get("parent_id")?,
                    name: f.try_get("name")?,
                    path: f.try_get("path")?,
                    depth: f.try_get("depth")?,
                    sort_order: f.try_get("sort_order").unwrap_or(0),
                },
            );
        }
    }
    for p in store.products.values_mut() {
        if let Some(vid) = p.current_version_id
            && let Some(v) = store.versions.get(&vid)
        {
            p.embedding_model_id = v.embedding_model_id.clone();
        }
    }
    Ok(())
}

fn parse_vector_literal(s: &str) -> Vec<f32> {
    s.trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .filter_map(|p| p.trim().parse().ok())
        .collect()
}

pub async fn workspaces_for_user(pool: &PgPool, user_id: Uuid) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar("SELECT workspace_id FROM workspace_members WHERE user_id = $1")
        .bind(user_id)
        .fetch_all(pool)
        .await
}

pub async fn document_workspace_id(
    pool: &PgPool,
    document_id: Uuid,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT p.workspace_id FROM documents d
         JOIN product_versions pv ON pv.id = d.product_version_id
         JOIN products p ON p.id = pv.product_id
         WHERE d.id = $1",
    )
    .bind(document_id)
    .fetch_optional(pool)
    .await
}

pub async fn version_workspace_id(
    pool: &PgPool,
    version_id: Uuid,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT p.workspace_id FROM product_versions pv
         JOIN products p ON p.id = pv.product_id
         WHERE pv.id = $1",
    )
    .bind(version_id)
    .fetch_optional(pool)
    .await
}

pub async fn product_workspace_id(
    pool: &PgPool,
    product_id: Uuid,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar("SELECT workspace_id FROM products WHERE id = $1")
        .bind(product_id)
        .fetch_optional(pool)
        .await
}

pub async fn insert_version_cloning(
    pool: &PgPool,
    target_id: Uuid,
    product_id: Uuid,
    label: &str,
    source_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO product_versions (
            id, product_id, label, status, cloned_from_version_id,
            chunking_config, indexing_strategy, image_processing_config,
            embedding_model_id, summary_model_id, vlm_model_id, asr_model_id,
            vlm_config, asr_config, extract_config, wiki_config, question_generation_config
         )
         SELECT $1, $2, $3, 'cloning', $4,
            chunking_config, indexing_strategy, image_processing_config,
            embedding_model_id, summary_model_id, vlm_model_id, asr_model_id,
            vlm_config, asr_config, extract_config, wiki_config, question_generation_config
         FROM product_versions WHERE id = $4",
    )
    .bind(target_id)
    .bind(product_id)
    .bind(label)
    .bind(source_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub struct NewDocument<'a> {
    pub id: Uuid,
    pub product_version_id: Uuid,
    pub title: &'a str,
    pub file_name: &'a str,
    pub file_size: i64,
    pub file_hash: &'a str,
    pub object_ref: &'a str,
}

pub async fn insert_tag(
    pool: &PgPool,
    id: Uuid,
    workspace_id: Uuid,
    name: &str,
    slug: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO tags (id, workspace_id, name, slug) VALUES ($1,$2,$3,$4)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(id)
    .bind(workspace_id)
    .bind(name)
    .bind(slug)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn insert_document_tags(
    pool: &PgPool,
    document_id: Uuid,
    tag_ids: &[Uuid],
) -> Result<(), sqlx::Error> {
    for tag_id in tag_ids {
        sqlx::query(
            "INSERT INTO document_tags (document_id, tag_id) VALUES ($1,$2)
             ON CONFLICT DO NOTHING",
        )
        .bind(document_id)
        .bind(tag_id)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub async fn replace_document_tags(
    pool: &PgPool,
    document_id: Uuid,
    tag_ids: &[Uuid],
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM document_tags WHERE document_id = $1")
        .bind(document_id)
        .execute(pool)
        .await?;
    insert_document_tags(pool, document_id, tag_ids).await
}

pub async fn insert_dead_letter(
    pool: &PgPool,
    task_type: &str,
    related_id: Uuid,
    last_error: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO task_dead_letters (id, task_type, scope, related_id, last_error)
         VALUES ($1,$2,'document',$3,$4)",
    )
    .bind(Uuid::new_v4())
    .bind(task_type)
    .bind(related_id)
    .bind(last_error)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_dead_letters(pool: &PgPool) -> Result<Vec<crate::DeadLetter>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT task_type, COALESCE(related_id, '00000000-0000-0000-0000-000000000000'::uuid), last_error
         FROM task_dead_letters ORDER BY failed_at DESC LIMIT 200",
    )
    .fetch_all(pool)
    .await?;
    let mut out = Vec::new();
    for r in rows {
        out.push(crate::DeadLetter {
            task_type: r.try_get(0)?,
            related_id: r.try_get(1)?,
            last_error: r.try_get(2).unwrap_or_default(),
        });
    }
    Ok(out)
}

pub async fn count_dead_letters(pool: &PgPool) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT COUNT(*) FROM task_dead_letters")
        .fetch_one(pool)
        .await
}

pub async fn version_exists(pool: &PgPool, version_id: Uuid) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM product_versions WHERE id = $1 AND deleted_at IS NULL)",
    )
    .bind(version_id)
    .fetch_one(pool)
    .await
}

pub async fn find_duplicate_document(
    pool: &PgPool,
    version_id: Uuid,
    file_name: &str,
    file_size: i64,
    file_hash: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT id FROM documents
         WHERE product_version_id = $1 AND file_name = $2 AND file_size = $3
           AND file_hash = $4 AND deleted_at IS NULL",
    )
    .bind(version_id)
    .bind(file_name)
    .bind(file_size)
    .bind(file_hash)
    .fetch_optional(pool)
    .await
}

pub fn is_unique_violation(err: &sqlx::Error) -> bool {
    err.as_database_error()
        .and_then(|e| e.code())
        .is_some_and(|c| c == "23505")
}

pub async fn delete_image_chunks(
    pool: &PgPool,
    document_id: Uuid,
    image_key: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "DELETE FROM chunks
         WHERE document_id = $1
           AND context_header = $2
           AND chunk_type IN ('image_ocr', 'image_caption')",
    )
    .bind(document_id)
    .bind(image_key)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn insert_document(pool: &PgPool, doc: NewDocument<'_>) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO documents (
            id, product_version_id, title, parse_status, enable_status,
            file_name, file_size, file_hash, object_ref, type
         ) VALUES ($1, $2, $3, 'pending', 'disabled', $4, $5, $6, $7, 'file')",
    )
    .bind(doc.id)
    .bind(doc.product_version_id)
    .bind(doc.title)
    .bind(doc.file_name)
    .bind(doc.file_size)
    .bind(doc.file_hash)
    .bind(doc.object_ref)
    .execute(&mut *tx)
    .await?;
    sqlx::query_scalar::<_, String>(
        "SELECT kb_register_knowledge_document_object(
            $1,'application/octet-stream',$2::kb_actor_identity,$3,$4)",
    )
    .bind(doc.id)
    .bind("system:knowledge-document-ingest")
    .bind(format!("knowledge-document:{}", doc.id))
    .bind(Uuid::new_v4())
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await
}

pub async fn set_index_ready(
    pool: &PgPool,
    document_id: Uuid,
    ready: bool,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE documents SET index_ready = $2, updated_at = now() WHERE id = $1")
        .bind(document_id)
        .bind(ready)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_document_source(
    pool: &PgPool,
    document_id: Uuid,
    doc_type: &str,
    passages: &[String],
) -> Result<(), sqlx::Error> {
    let raw = if passages.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::json!(passages)
    };
    sqlx::query(
        "UPDATE documents SET type = $2, source_passages = $3, updated_at = now() WHERE id = $1",
    )
    .bind(document_id)
    .bind(doc_type)
    .bind(raw)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn persist_summary_chunks(
    pool: &PgPool,
    store: &crate::Store,
    document_id: Uuid,
) -> Result<(), sqlx::Error> {
    delete_chunks_by_types(pool, document_id, &["summary"]).await?;
    let chunks: Vec<_> = store
        .chunks
        .values()
        .filter(|c| c.document_id == document_id && c.chunk_type == "summary")
        .cloned()
        .collect();
    let ids: std::collections::HashSet<_> = chunks.iter().map(|c| c.id).collect();
    let embeddings: Vec<_> = store
        .embeddings
        .values()
        .filter(|e| ids.contains(&e.chunk_id))
        .cloned()
        .collect();
    append_document_chunks(pool, &chunks, &embeddings).await
}

pub async fn persist_question_updates(
    pool: &PgPool,
    store: &crate::Store,
    document_id: Uuid,
    parent_ids: &[Uuid],
) -> Result<(), sqlx::Error> {
    if !parent_ids.is_empty() {
        sqlx::query(
            "DELETE FROM chunks
             WHERE document_id = $1
               AND (
                    chunk_type = 'question'
                    OR (chunk_type = 'text'
                        AND parent_chunk_id = ANY($2)
                        AND COALESCE(jsonb_array_length(generated_questions), 0) = 0)
               )
               AND parent_chunk_id = ANY($2)",
        )
        .bind(document_id)
        .bind(parent_ids)
        .execute(pool)
        .await?;
    }
    for ch in store.chunks.values().filter(|c| parent_ids.contains(&c.id)) {
        sqlx::query("UPDATE chunks SET generated_questions = $2 WHERE id = $1")
            .bind(ch.id)
            .bind(serde_json::json!(ch.generated_questions))
            .execute(pool)
            .await?;
    }
    let parents: std::collections::HashSet<_> = parent_ids.iter().copied().collect();
    let kids: Vec<_> = store
        .chunks
        .values()
        .filter(|c| {
            c.document_id == document_id
                && c.chunk_type == "question"
                && c.parent_chunk_id.is_some_and(|p| parents.contains(&p))
        })
        .cloned()
        .collect();
    let ids: std::collections::HashSet<_> = kids.iter().map(|c| c.id).collect();
    let embeddings: Vec<_> = store
        .embeddings
        .values()
        .filter(|e| ids.contains(&e.chunk_id))
        .cloned()
        .collect();
    append_document_chunks(pool, &kids, &embeddings).await
}

pub async fn set_process_overrides(
    pool: &PgPool,
    document_id: Uuid,
    overrides: &crate::ProcessOverrides,
) -> Result<(), sqlx::Error> {
    let raw = serde_json::to_value(overrides).unwrap_or(serde_json::Value::Null);
    sqlx::query("UPDATE documents SET process_overrides = $2, updated_at = now() WHERE id = $1")
        .bind(document_id)
        .bind(raw)
        .execute(pool)
        .await?;
    Ok(())
}

/// Brain `SetFinalizing`: only from `processing`. Returns whether the row flipped.
pub async fn set_finalizing(
    pool: &PgPool,
    document_id: Uuid,
    pending_subtasks_count: i32,
) -> Result<bool, sqlx::Error> {
    let n = sqlx::query(
        "UPDATE documents SET parse_status = 'finalizing', pending_subtasks_count = $2,
                updated_at = now()
         WHERE id = $1 AND parse_status = 'processing'",
    )
    .bind(document_id)
    .bind(pending_subtasks_count)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(n == 1)
}

pub async fn set_document_progress(
    pool: &PgPool,
    document_id: Uuid,
    status: &str,
    pending_subtasks_count: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE documents SET parse_status = $2, pending_subtasks_count = $3,
                updated_at = now() WHERE id = $1",
    )
    .bind(document_id)
    .bind(status)
    .bind(pending_subtasks_count)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_summary_status(
    pool: &PgPool,
    document_id: Uuid,
    status: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE documents SET summary_status = $2, updated_at = now() WHERE id = $1")
        .bind(document_id)
        .bind(status)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn persist_graph_for_document(
    pool: &PgPool,
    store: &crate::Store,
    document_id: Uuid,
) -> Result<(), sqlx::Error> {
    for n in store
        .graph
        .values()
        .filter(|n| n.document_id == document_id)
    {
        sqlx::query(
            "INSERT INTO graph_nodes (product_version_id, document_id, name, chunk_ids)
             VALUES ($1,$2,$3,$4)
             ON CONFLICT (product_version_id, document_id, name)
             DO UPDATE SET chunk_ids = (
                 SELECT ARRAY(SELECT DISTINCT x FROM unnest(graph_nodes.chunk_ids || EXCLUDED.chunk_ids) AS x)
             )",
        )
        .bind(n.version_id)
        .bind(n.document_id)
        .bind(&n.name)
        .bind(&n.chunk_ids)
        .execute(pool)
        .await?;
    }
    for r in store
        .relations
        .values()
        .filter(|r| r.document_id == document_id)
    {
        sqlx::query(
            "INSERT INTO graph_relations
                (product_version_id, document_id, node1, node2, rel_type)
             VALUES ($1,$2,$3,$4,$5)
             ON CONFLICT DO NOTHING",
        )
        .bind(r.version_id)
        .bind(r.document_id)
        .bind(&r.node1)
        .bind(&r.node2)
        .bind(&r.rel_type)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub async fn delete_graph_for_document(
    pool: &PgPool,
    document_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM graph_relations WHERE document_id = $1")
        .bind(document_id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM graph_nodes WHERE document_id = $1")
        .bind(document_id)
        .execute(pool)
        .await?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct PgGraphHit {
    pub name: String,
    pub document_id: Uuid,
    pub document_title: String,
    pub chunk_id: Uuid,
    pub content: String,
    pub start_at: i32,
    pub end_at: i32,
    pub product_id: Uuid,
    pub product_kind: String,
    pub version_id: Uuid,
    pub version_label: String,
    pub is_current: bool,
    pub tag_ids: Vec<Uuid>,
    pub tag_slugs: Vec<String>,
}

pub async fn graph_hits_pg(
    pool: &PgPool,
    version_id: Uuid,
    query: &str,
    limit: i64,
    tag_ids: &[Uuid],
) -> Result<Vec<PgGraphHit>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT n.name, n.document_id, d.title,
                COALESCE(c.id, '00000000-0000-0000-0000-000000000000'::uuid) AS chunk_id,
                COALESCE(c.content, '') AS content,
                COALESCE(c.start_at, 0) AS start_at,
                COALESCE(c.end_at, 0) AS end_at,
                p.id AS product_id, p.kind AS product_kind,
                pv.id AS version_id, pv.label AS version_label,
                (p.current_version_id = pv.id) AS is_current,
                COALESCE((
                    SELECT array_agg(dt.tag_id)
                    FROM document_tags dt WHERE dt.document_id = d.id
                ), ARRAY[]::uuid[]) AS tag_ids,
                COALESCE((
                    SELECT array_agg(t.slug)
                    FROM document_tags dt
                    JOIN tags t ON t.id = dt.tag_id
                    WHERE dt.document_id = d.id
                ), ARRAY[]::text[]) AS tag_slugs
         FROM graph_nodes n
         JOIN documents d ON d.id = n.document_id
         JOIN product_versions pv ON pv.id = n.product_version_id
         JOIN products p ON p.id = pv.product_id
         LEFT JOIN LATERAL (
            SELECT id, content, start_at, end_at
            FROM chunks
            WHERE id = ANY(n.chunk_ids)
            LIMIT 1
         ) c ON true
         WHERE n.product_version_id = $1
           AND d.deleted_at IS NULL
           AND (
                n.name ILIKE '%' || $2 || '%'
                OR $2 ILIKE '%' || n.name || '%'
           )
           AND (
                cardinality($4::uuid[]) = 0
                OR EXISTS (
                    SELECT 1 FROM document_tags dt
                    WHERE dt.document_id = d.id AND dt.tag_id = ANY($4)
                )
           )
         LIMIT $3",
    )
    .bind(version_id)
    .bind(query)
    .bind(limit)
    .bind(tag_ids)
    .fetch_all(pool)
    .await?;
    let mut out = Vec::new();
    for r in rows {
        out.push(PgGraphHit {
            name: r.try_get("name")?,
            document_id: r.try_get("document_id")?,
            document_title: r.try_get("title")?,
            chunk_id: r.try_get("chunk_id")?,
            content: r.try_get("content")?,
            start_at: r.try_get("start_at")?,
            end_at: r.try_get("end_at")?,
            product_id: r.try_get("product_id")?,
            product_kind: r.try_get("product_kind")?,
            version_id: r.try_get("version_id")?,
            version_label: r.try_get("version_label")?,
            is_current: r.try_get("is_current")?,
            tag_ids: r.try_get("tag_ids").unwrap_or_default(),
            tag_slugs: r.try_get("tag_slugs").unwrap_or_default(),
        });
    }
    Ok(out)
}

pub async fn set_parse_status(
    pool: &PgPool,
    document_id: Uuid,
    status: &str,
    error_message: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE documents SET parse_status = $2, error_message = $3, updated_at = now() WHERE id = $1",
    )
        .bind(document_id)
        .bind(status)
        .bind(error_message)
        .execute(pool)
        .await?;
    Ok(())
}

/// Spec 3.2: flip to `processing` only if the row is not already abort/completed.
pub async fn try_set_processing(pool: &PgPool, document_id: Uuid) -> Result<bool, sqlx::Error> {
    let n = sqlx::query(
        "UPDATE documents SET parse_status = 'processing', error_message = '', updated_at = now()
         WHERE id = $1
           AND parse_status NOT IN ('cancelled', 'deleting', 'completed')",
    )
    .bind(document_id)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(n > 0)
}

pub async fn document_parse_status(
    pool: &PgPool,
    document_id: Uuid,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT parse_status FROM documents WHERE id = $1")
        .bind(document_id)
        .fetch_optional(pool)
        .await
}

fn span_kind(name: &str) -> &'static str {
    match name {
        "document_processing" => "root",
        "docreader" | "chunking" | "embedding" | "multimodal" | "postprocess" => "stage",
        _ => "subspan",
    }
}

fn span_status(status: &str) -> &'static str {
    match status {
        "ok" | "done" => "done",
        "error" | "failed" => "failed",
        "running" => "running",
        "pending" => "pending",
        "skipped" => "skipped",
        "cancelled" => "cancelled",
        _ => "done",
    }
}

async fn lookup_span_id(
    pool: &PgPool,
    document_id: Uuid,
    attempt: i32,
    name: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT span_id FROM document_processing_spans
         WHERE document_id = $1 AND attempt = $2 AND name = $3",
    )
    .bind(document_id)
    .bind(attempt)
    .bind(name)
    .fetch_optional(pool)
    .await
    .map(|v| v.flatten())
}

pub async fn open_attempt(
    pool: &PgPool,
    document_id: Uuid,
    attempt: i32,
) -> Result<(), sqlx::Error> {
    start_span(
        pool,
        document_id,
        attempt,
        "document_processing",
        None,
        None,
    )
    .await?;
    let root = lookup_span_id(pool, document_id, attempt, "document_processing").await?;
    for name in [
        "docreader",
        "chunking",
        "embedding",
        "multimodal",
        "postprocess",
    ] {
        sqlx::query(
            "INSERT INTO document_processing_spans
                (document_id, attempt, name, span_id, parent_span_id, kind, status, started_at)
             VALUES ($1, $2, $3, $4, $5, 'stage', 'pending', NULL)
             ON CONFLICT (document_id, attempt, name) DO NOTHING",
        )
        .bind(document_id)
        .bind(attempt)
        .bind(name)
        .bind(Uuid::new_v4())
        .bind(root)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub async fn start_span(
    pool: &PgPool,
    document_id: Uuid,
    attempt: i32,
    name: &str,
    parent_name: Option<&str>,
    input: Option<serde_json::Value>,
) -> Result<(), sqlx::Error> {
    let parent = parent_name.or(if name == "document_processing" {
        None
    } else {
        Some("document_processing")
    });
    let parent_id = if let Some(p) = parent {
        lookup_span_id(pool, document_id, attempt, p).await?
    } else {
        None
    };
    let kind = span_kind(name);
    sqlx::query(
        "INSERT INTO document_processing_spans
            (document_id, attempt, name, span_id, parent_span_id, kind, status, input,
             started_at, finished_at, duration_ms)
         VALUES ($1, $2, $3, $4, $5, $6, 'running', $7, now(), NULL, NULL)
         ON CONFLICT (document_id, attempt, name) DO UPDATE SET
            status = 'running',
            input = COALESCE(EXCLUDED.input, document_processing_spans.input),
            parent_span_id = COALESCE(EXCLUDED.parent_span_id, document_processing_spans.parent_span_id),
            kind = EXCLUDED.kind,
            started_at = now(),
            finished_at = NULL,
            duration_ms = NULL,
            error_message = NULL",
    )
    .bind(document_id)
    .bind(attempt)
    .bind(name)
    .bind(Uuid::new_v4())
    .bind(parent_id)
    .bind(kind)
    .bind(input)
    .execute(pool)
    .await?;
    sqlx::query("UPDATE documents SET updated_at = now() WHERE id = $1")
        .bind(document_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn finish_span(
    pool: &PgPool,
    document_id: Uuid,
    attempt: i32,
    name: &str,
    status: &str,
    output: Option<serde_json::Value>,
) -> Result<(), sqlx::Error> {
    let status = span_status(status);
    let err = if status == "failed" {
        output
            .as_ref()
            .and_then(|v| v.get("error"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    } else {
        None
    };
    let n = sqlx::query(
        "UPDATE document_processing_spans SET
            status = $4,
            output = $5,
            error_message = $6,
            finished_at = now(),
            duration_ms = (EXTRACT(EPOCH FROM (now() - COALESCE(started_at, now()))) * 1000)::bigint
         WHERE document_id = $1 AND attempt = $2 AND name = $3",
    )
    .bind(document_id)
    .bind(attempt)
    .bind(name)
    .bind(status)
    .bind(&output)
    .bind(&err)
    .execute(pool)
    .await?
    .rows_affected();
    if n == 0 {
        upsert_span(pool, document_id, attempt, name, status, output).await?;
    }
    sqlx::query("UPDATE documents SET updated_at = now() WHERE id = $1")
        .bind(document_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn skip_span(
    pool: &PgPool,
    document_id: Uuid,
    attempt: i32,
    name: &str,
    reason: &str,
) -> Result<(), sqlx::Error> {
    start_span(
        pool,
        document_id,
        attempt,
        name,
        Some("document_processing"),
        None,
    )
    .await?;
    sqlx::query(
        "UPDATE document_processing_spans SET
            status = 'skipped',
            error_message = $4,
            finished_at = now(),
            duration_ms = 0
         WHERE document_id = $1 AND attempt = $2 AND name = $3",
    )
    .bind(document_id)
    .bind(attempt)
    .bind(name)
    .bind(reason)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn cancel_dependent_stages(
    pool: &PgPool,
    document_id: Uuid,
    attempt: i32,
    failed_stage: &str,
) -> Result<(), sqlx::Error> {
    let deps: &[&str] = match failed_stage {
        "docreader" => &["chunking", "embedding", "multimodal", "postprocess"],
        "chunking" => &["embedding", "multimodal", "postprocess"],
        "embedding" | "multimodal" => &["postprocess"],
        _ => &[],
    };
    if deps.is_empty() {
        return Ok(());
    }
    let names: Vec<String> = deps.iter().map(|s| (*s).to_string()).collect();
    sqlx::query(
        "UPDATE document_processing_spans SET
            status = 'cancelled',
            error_message = $4,
            finished_at = now()
         WHERE document_id = $1 AND attempt = $2
           AND name = ANY($3)
           AND status IN ('pending', 'running')",
    )
    .bind(document_id)
    .bind(attempt)
    .bind(&names)
    .bind(format!("upstream {failed_stage} failed"))
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn upsert_span(
    pool: &PgPool,
    document_id: Uuid,
    attempt: i32,
    name: &str,
    status: &str,
    output: Option<serde_json::Value>,
) -> Result<(), sqlx::Error> {
    let running = status == "running";
    sqlx::query(
        "INSERT INTO document_processing_spans
            (document_id, attempt, name, span_id, kind, status, output, started_at, finished_at)
         VALUES ($1, $2, $3, $4, $3, $5, $6, now(), CASE WHEN $7 THEN NULL ELSE now() END)
         ON CONFLICT (document_id, attempt, name) DO UPDATE SET
            status = EXCLUDED.status,
            output = EXCLUDED.output,
            finished_at = CASE WHEN $7 THEN NULL ELSE now() END,
            duration_ms = CASE WHEN $7 THEN NULL
                ELSE (EXTRACT(EPOCH FROM (now() - document_processing_spans.started_at)) * 1000)::bigint
            END",
    )
    .bind(document_id)
    .bind(attempt)
    .bind(name)
    .bind(Uuid::new_v4())
    .bind(status)
    .bind(output)
    .bind(running)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct SpanRow {
    pub document_id: Uuid,
    pub attempt: i32,
    pub name: String,
    pub span_id: Option<Uuid>,
    pub parent_span_id: Option<Uuid>,
    pub kind: Option<String>,
    pub status: Option<String>,
    pub output: Option<serde_json::Value>,
    pub error_message: Option<String>,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    pub duration_ms: Option<i64>,
}

impl SpanRow {
    pub fn into_span(self) -> crate::Span {
        crate::Span {
            span_id: self.span_id.unwrap_or_else(Uuid::new_v4),
            document_id: self.document_id,
            attempt: self.attempt,
            name: self.name,
            parent_span_id: self.parent_span_id,
            kind: self.kind.unwrap_or_else(|| "stage".into()),
            status: self.status.unwrap_or_else(|| "pending".into()),
            output: self.output,
            error_message: self.error_message.unwrap_or_default(),
            started_at: self.started_at.unwrap_or_else(chrono::Utc::now),
            finished_at: self.finished_at,
            duration_ms: self.duration_ms,
        }
    }
}

pub async fn latest_span_attempt(pool: &PgPool, document_id: Uuid) -> Result<i32, sqlx::Error> {
    let n: Option<i32> = sqlx::query_scalar(
        "SELECT COALESCE(MAX(attempt), 0) FROM document_processing_spans WHERE document_id = $1",
    )
    .bind(document_id)
    .fetch_one(pool)
    .await?;
    Ok(n.unwrap_or(0))
}

pub async fn list_spans(pool: &PgPool, document_id: Uuid) -> Result<Vec<SpanRow>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT document_id, attempt, name, span_id, parent_span_id, kind, status,
                output, error_message, started_at, finished_at, duration_ms
         FROM document_processing_spans
         WHERE document_id = $1
         ORDER BY started_at NULLS LAST, name",
    )
    .bind(document_id)
    .fetch_all(pool)
    .await?;
    let mut out = Vec::new();
    for r in rows {
        out.push(SpanRow {
            document_id: r.try_get("document_id")?,
            attempt: r.try_get("attempt")?,
            name: r.try_get("name")?,
            span_id: r.try_get("span_id")?,
            parent_span_id: r.try_get("parent_span_id")?,
            kind: r.try_get("kind")?,
            status: r.try_get("status")?,
            output: r.try_get("output")?,
            error_message: r.try_get("error_message")?,
            started_at: r.try_get("started_at")?,
            finished_at: r.try_get("finished_at")?,
            duration_ms: r.try_get("duration_ms")?,
        });
    }
    Ok(out)
}

pub async fn list_spans_attempt(
    pool: &PgPool,
    document_id: Uuid,
    attempt: i32,
) -> Result<Vec<SpanRow>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT document_id, attempt, name, span_id, parent_span_id, kind, status,
                output, error_message, started_at, finished_at, duration_ms
         FROM document_processing_spans
         WHERE document_id = $1 AND attempt = $2
         ORDER BY started_at NULLS LAST, name",
    )
    .bind(document_id)
    .bind(attempt)
    .fetch_all(pool)
    .await?;
    let mut out = Vec::new();
    for r in rows {
        out.push(SpanRow {
            document_id: r.try_get("document_id")?,
            attempt: r.try_get("attempt")?,
            name: r.try_get("name")?,
            span_id: r.try_get("span_id")?,
            parent_span_id: r.try_get("parent_span_id")?,
            kind: r.try_get("kind")?,
            status: r.try_get("status")?,
            output: r.try_get("output")?,
            error_message: r.try_get("error_message")?,
            started_at: r.try_get("started_at")?,
            finished_at: r.try_get("finished_at")?,
            duration_ms: r.try_get("duration_ms")?,
        });
    }
    Ok(out)
}

pub async fn mark_reparse_queued(pool: &PgPool, document_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE documents SET
            parse_status = 'pending',
            enable_status = 'disabled',
            pending_subtasks_count = 0,
            error_message = '',
            updated_at = now()
         WHERE id = $1",
    )
    .bind(document_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_product(pool: &PgPool, product_id: Uuid) -> Result<(), PersistError> {
    let row = sqlx::query("SELECT kind, slug FROM products WHERE id = $1")
        .bind(product_id)
        .fetch_optional(pool)
        .await?;
    let Some(row) = row else {
        return Err(PersistError::NotFound);
    };
    let kind: String = row.try_get("kind")?;
    let slug: String = row.try_get("slug")?;
    if kind == "library" && slug == "library" {
        return Err(PersistError::DefaultLibrary);
    }
    sqlx::query("DELETE FROM products WHERE id = $1")
        .bind(product_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_workspace_name(
    pool: &PgPool,
    workspace_id: Uuid,
    name: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE workspaces SET name = $2, updated_at = now() WHERE id = $1")
        .bind(workspace_id)
        .bind(name)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_product_name(
    pool: &PgPool,
    product_id: Uuid,
    name: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE products SET name = $2 WHERE id = $1")
        .bind(product_id)
        .bind(name)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn upsert_member(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
    role: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO workspace_members (workspace_id, user_id, role) VALUES ($1, $2, $3)
         ON CONFLICT (workspace_id, user_id) DO UPDATE SET role = EXCLUDED.role",
    )
    .bind(workspace_id)
    .bind(user_id)
    .bind(role)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_member(
    pool: &PgPool,
    workspace_id: Uuid,
    user_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM workspace_members WHERE workspace_id = $1 AND user_id = $2")
        .bind(workspace_id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_version_status(
    pool: &PgPool,
    version_id: Uuid,
    status: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE product_versions SET status = $2, updated_at = now() WHERE id = $1")
        .bind(version_id)
        .bind(status)
        .execute(pool)
        .await?;
    Ok(())
}

pub struct VersionConfig {
    pub status: Option<String>,
    pub chunking: Option<serde_json::Value>,
    pub indexing: Option<serde_json::Value>,
    pub image_processing: Option<serde_json::Value>,
    pub embedding_model_id: Option<String>,
    pub summary_model_id: Option<String>,
    pub asr_model_id: Option<String>,
    pub asr_config: Option<serde_json::Value>,
    pub extract_config: Option<serde_json::Value>,
    pub wiki_config: Option<serde_json::Value>,
    pub question_generation_config: Option<serde_json::Value>,
}

pub async fn update_version_config(
    pool: &PgPool,
    version_id: Uuid,
    cfg: VersionConfig,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE product_versions SET
            status = COALESCE($2, status),
            chunking_config = COALESCE($3, chunking_config),
            indexing_strategy = COALESCE($4, indexing_strategy),
            image_processing_config = COALESCE($5, image_processing_config),
            embedding_model_id = COALESCE($6, embedding_model_id),
            summary_model_id = COALESCE($7, summary_model_id),
            asr_model_id = COALESCE($8, asr_model_id),
            asr_config = COALESCE($9, asr_config),
            extract_config = COALESCE($10, extract_config),
            wiki_config = COALESCE($11, wiki_config),
            question_generation_config = COALESCE($12, question_generation_config),
            updated_at = now()
         WHERE id = $1",
    )
    .bind(version_id)
    .bind(cfg.status)
    .bind(cfg.chunking)
    .bind(cfg.indexing)
    .bind(cfg.image_processing)
    .bind(cfg.embedding_model_id)
    .bind(cfg.summary_model_id)
    .bind(cfg.asr_model_id)
    .bind(cfg.asr_config)
    .bind(cfg.extract_config)
    .bind(cfg.wiki_config)
    .bind(cfg.question_generation_config)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn update_user_email(
    pool: &PgPool,
    user_id: Uuid,
    email: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE users SET email = $2, updated_at = now() WHERE id = $1")
        .bind(user_id)
        .bind(email)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn retire_workspace(pool: &PgPool, workspace_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM workspace_members WHERE workspace_id = $1")
        .bind(workspace_id)
        .execute(pool)
        .await?;
    sqlx::query(
        "DELETE FROM api_keys
         WHERE (scope_type = 'workspace' AND scope_id = $1)
            OR (scope_type = 'product' AND scope_id IN (
                SELECT id FROM products WHERE workspace_id = $1
            ))",
    )
    .bind(workspace_id)
    .execute(pool)
    .await?;
    sqlx::query("UPDATE workspaces SET slug = $2, name = $3, updated_at = now() WHERE id = $1")
        .bind(workspace_id)
        .bind(format!("__deleted_{workspace_id}"))
        .bind(format!("deleted-{workspace_id}"))
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_empty_product(pool: &PgPool, product_id: Uuid) -> Result<bool, sqlx::Error> {
    let left: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM product_versions WHERE product_id = $1 AND deleted_at IS NULL",
    )
    .bind(product_id)
    .fetch_one(pool)
    .await?;
    if left > 0 {
        return Ok(false);
    }
    sqlx::query("UPDATE products SET current_version_id = NULL WHERE id = $1")
        .bind(product_id)
        .execute(pool)
        .await?;
    sqlx::query(
        "DELETE FROM documents WHERE product_version_id IN (
            SELECT id FROM product_versions WHERE product_id = $1 AND deleted_at IS NOT NULL
         )",
    )
    .bind(product_id)
    .execute(pool)
    .await?;
    sqlx::query("DELETE FROM product_versions WHERE product_id = $1 AND deleted_at IS NOT NULL")
        .bind(product_id)
        .execute(pool)
        .await?;
    let row = sqlx::query("SELECT kind, slug, workspace_id FROM products WHERE id = $1")
        .bind(product_id)
        .fetch_optional(pool)
        .await?;
    let Some(row) = row else {
        return Ok(false);
    };
    let kind: String = row.try_get("kind")?;
    let slug: String = row.try_get("slug")?;
    let ws: Uuid = row.try_get("workspace_id")?;
    if kind == "library" && slug == "library" {
        return Ok(false);
    }
    sqlx::query("DELETE FROM products WHERE id = $1")
        .bind(product_id)
        .execute(pool)
        .await?;
    let products: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM products WHERE workspace_id = $1")
        .bind(ws)
        .fetch_one(pool)
        .await?;
    if products == 0 {
        retire_workspace(pool, ws).await?;
    }
    Ok(true)
}

pub async fn version_ids_for_workspace(
    pool: &PgPool,
    workspace_id: Uuid,
) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT v.id FROM product_versions v
         JOIN products p ON p.id = v.product_id
         WHERE p.workspace_id = $1 AND v.deleted_at IS NULL",
    )
    .bind(workspace_id)
    .fetch_all(pool)
    .await
}

pub async fn version_ids_for_product(
    pool: &PgPool,
    product_id: Uuid,
) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT id FROM product_versions WHERE product_id = $1 AND deleted_at IS NULL",
    )
    .bind(product_id)
    .fetch_all(pool)
    .await
}

pub async fn cancel_active_docs_for_versions(
    pool: &PgPool,
    version_ids: &[Uuid],
) -> Result<u64, sqlx::Error> {
    if version_ids.is_empty() {
        return Ok(0);
    }
    let res = sqlx::query(
        "UPDATE documents SET parse_status = 'cancelled', updated_at = now()
         WHERE product_version_id = ANY($1)
           AND parse_status IN ('pending', 'processing', 'finalizing')
           AND deleted_at IS NULL",
    )
    .bind(version_ids)
    .execute(pool)
    .await?;
    Ok(res.rows_affected())
}

pub async fn delete_tag(pool: &PgPool, tag_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM document_tags WHERE tag_id = $1")
        .bind(tag_id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM tags WHERE id = $1")
        .bind(tag_id)
        .execute(pool)
        .await?;
    Ok(())
}

#[derive(Debug)]
pub enum PersistError {
    DefaultLibrary,
    NotFound,
    Sql(sqlx::Error),
}

impl From<sqlx::Error> for PersistError {
    fn from(e: sqlx::Error) -> Self {
        Self::Sql(e)
    }
}

/// Fail pending/processing/finalizing rows whose last heartbeat
/// (span finished/started, else `updated_at`) is older than `stale_secs`.
pub async fn housekeep_documents(pool: &PgPool, stale_secs: i64) -> Result<u64, sqlx::Error> {
    let msg = format!("task stuck in processing > {stale_secs}s, recovered by housekeeping");
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "UPDATE documents d
         SET parse_status = 'failed',
             error_message = $2,
             pending_subtasks_count = 0,
             updated_at = now()
         WHERE d.parse_status IN ('processing', 'finalizing')
           AND COALESCE(
                 (SELECT MAX(COALESCE(finished_at, started_at))
                    FROM document_processing_spans s
                   WHERE s.document_id = d.id),
                 d.updated_at
               ) < now() - make_interval(secs => $1::double precision)
         RETURNING d.id",
    )
    .bind(stale_secs)
    .bind(&msg)
    .fetch_all(pool)
    .await?;
    if !ids.is_empty() {
        let _ = sqlx::query(
            "UPDATE document_processing_spans
             SET status = 'failed', finished_at = COALESCE(finished_at, now()),
                 duration_ms = COALESCE(duration_ms, 0)
             WHERE document_id = ANY($1)
               AND status IN ('running', 'pending')",
        )
        .bind(&ids)
        .execute(pool)
        .await;
    }
    Ok(ids.len() as u64)
}

#[derive(Debug, Clone)]
pub struct PendingOp {
    pub id: Uuid,
    pub task_type: String,
    pub scope_id: Uuid,
    pub op: String,
    pub dedup_key: Option<String>,
    pub payload: serde_json::Value,
    pub fail_count: i32,
}

pub async fn enqueue_pending_op(
    pool: &PgPool,
    task_type: &str,
    scope_id: Uuid,
    op: &str,
    dedup_key: Option<&str>,
    payload: serde_json::Value,
) -> Result<Uuid, sqlx::Error> {
    if let Some(key) = dedup_key {
        sqlx::query(
            "DELETE FROM task_pending_ops
             WHERE task_type = $1 AND scope_id = $2 AND op = $3 AND dedup_key = $4
               AND claimed_at IS NULL",
        )
        .bind(task_type)
        .bind(scope_id)
        .bind(op)
        .bind(key)
        .execute(pool)
        .await?;
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO task_pending_ops
            (id, task_type, scope, scope_id, op, dedup_key, payload)
         VALUES ($1, $2, 'product_version', $3, $4, $5, $6)",
    )
    .bind(id)
    .bind(task_type)
    .bind(scope_id)
    .bind(op)
    .bind(dedup_key)
    .bind(payload)
    .execute(pool)
    .await?;
    Ok(id)
}

pub async fn claim_pending_batch(
    pool: &PgPool,
    task_type: &str,
    scope_id: Uuid,
    limit: i64,
    stale_minutes: i64,
) -> Result<Vec<PendingOp>, sqlx::Error> {
    let rows = sqlx::query(
        "UPDATE task_pending_ops SET claimed_at = now()
         WHERE id IN (
            SELECT id FROM task_pending_ops
            WHERE task_type = $1 AND scope_id = $2
              AND (claimed_at IS NULL
                   OR claimed_at < now() - make_interval(mins => $3::int))
            ORDER BY enqueued_at
            LIMIT $4
            FOR UPDATE SKIP LOCKED
         )
         RETURNING id, task_type, scope_id, op, dedup_key, COALESCE(payload, '{}'::jsonb) AS payload, fail_count",
    )
    .bind(task_type)
    .bind(scope_id)
    .bind(stale_minutes as i32)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    let mut out = Vec::new();
    for r in rows {
        out.push(PendingOp {
            id: r.try_get("id")?,
            task_type: r.try_get("task_type")?,
            scope_id: r.try_get("scope_id")?,
            op: r.try_get("op")?,
            dedup_key: r.try_get("dedup_key")?,
            payload: r.try_get("payload")?,
            fail_count: r.try_get("fail_count")?,
        });
    }
    Ok(out)
}

pub async fn drop_pending_ops(
    pool: &PgPool,
    task_type: &str,
    scope_id: Uuid,
) -> Result<u64, sqlx::Error> {
    let n = sqlx::query("DELETE FROM task_pending_ops WHERE task_type = $1 AND scope_id = $2")
        .bind(task_type)
        .bind(scope_id)
        .execute(pool)
        .await?
        .rows_affected();
    Ok(n)
}

pub async fn delete_pending_ids(pool: &PgPool, ids: &[Uuid]) -> Result<u64, sqlx::Error> {
    if ids.is_empty() {
        return Ok(0);
    }
    let n = sqlx::query("DELETE FROM task_pending_ops WHERE id = ANY($1)")
        .bind(ids)
        .execute(pool)
        .await?
        .rows_affected();
    Ok(n)
}

pub async fn unclaim_pending_ids(pool: &PgPool, ids: &[Uuid]) -> Result<u64, sqlx::Error> {
    if ids.is_empty() {
        return Ok(0);
    }
    let n = sqlx::query("UPDATE task_pending_ops SET claimed_at = NULL WHERE id = ANY($1)")
        .bind(ids)
        .execute(pool)
        .await?
        .rows_affected();
    Ok(n)
}

pub async fn retry_pending_op(pool: &PgPool, id: Uuid, fail_count: i32) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE task_pending_ops SET claimed_at = NULL, fail_count = $2 WHERE id = $1")
        .bind(id)
        .bind(fail_count)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn replace_wiki_page_chunks(
    pool: &PgPool,
    version_id: Uuid,
    slugs: &[String],
    chunks: &[crate::Chunk],
    embeddings: &[crate::ChunkEmbedding],
) -> Result<(), sqlx::Error> {
    if !slugs.is_empty() {
        sqlx::query(
            "DELETE FROM chunks
             WHERE product_version_id = $1
               AND chunk_type = 'wiki_page'
               AND context_header = ANY($2)",
        )
        .bind(version_id)
        .bind(slugs)
        .execute(pool)
        .await?;
    }
    append_document_chunks(pool, chunks, embeddings).await
}

pub async fn pending_op_counts(
    pool: &PgPool,
) -> Result<std::collections::HashMap<String, i64>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT task_type, COUNT(*)::bigint AS n FROM task_pending_ops GROUP BY task_type",
    )
    .fetch_all(pool)
    .await?;
    let mut out = std::collections::HashMap::new();
    for r in rows {
        let t: String = r.try_get("task_type")?;
        let n: i64 = r.try_get("n")?;
        out.insert(t, n);
    }
    Ok(out)
}

pub async fn count_pending(
    pool: &PgPool,
    task_type: &str,
    scope_id: Uuid,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM task_pending_ops WHERE task_type = $1 AND scope_id = $2",
    )
    .bind(task_type)
    .bind(scope_id)
    .fetch_one(pool)
    .await
}

pub async fn version_multimodal_enabled(
    pool: &PgPool,
    version_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let row: Option<bool> = sqlx::query_scalar(
        "SELECT COALESCE(
            (image_processing_config->>'enable_multimodel')::boolean,
            (indexing_strategy->>'multimodal')::boolean,
            false
         )
         FROM product_versions WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(version_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.unwrap_or(false))
}

pub async fn current_summary_model(
    pool: &PgPool,
    product_id: Uuid,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT COALESCE(pv.summary_model_id, 'stub-chat')
         FROM products p
         JOIN product_versions pv ON pv.id = p.current_version_id
         WHERE p.id = $1",
    )
    .bind(product_id)
    .fetch_optional(pool)
    .await
}

pub async fn set_document_description(
    pool: &PgPool,
    document_id: Uuid,
    description: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE documents SET description = $2, updated_at = now() WHERE id = $1")
        .bind(document_id)
        .bind(description)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn bump_document_attempt(pool: &PgPool, document_id: Uuid) -> Result<i32, sqlx::Error> {
    let n: i32 = sqlx::query_scalar(
        "UPDATE documents SET
            attempt = attempt + 1,
            parse_status = 'pending',
            enable_status = 'disabled',
            pending_subtasks_count = 0,
            error_message = '',
            updated_at = now()
         WHERE id = $1
         RETURNING attempt",
    )
    .bind(document_id)
    .fetch_one(pool)
    .await?;
    Ok(n)
}

pub async fn purge_document_index(pool: &PgPool, document_id: Uuid) -> Result<(), sqlx::Error> {
    delete_graph_for_document(pool, document_id).await?;
    sqlx::query("DELETE FROM chunks WHERE document_id = $1")
        .bind(document_id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM wiki_pages WHERE source_refs @> jsonb_build_array($1::text)")
        .bind(document_id.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn upsert_wiki_page(
    pool: &PgPool,
    page: &crate::WikiPage,
    source_document_id: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    let mut refs: Vec<String> = page.source_refs.iter().map(|id| id.to_string()).collect();
    if let Some(id) = source_document_id {
        let s = id.to_string();
        if !refs.contains(&s) {
            refs.push(s);
        }
    }
    sqlx::query(
        "INSERT INTO wiki_pages
            (id, product_version_id, slug, title, content, page_type, status,
             summary, aliases, source_refs, chunk_refs, category_path, folder_id)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)
         ON CONFLICT (product_version_id, slug) DO UPDATE SET
            title = EXCLUDED.title,
            content = EXCLUDED.content,
            page_type = EXCLUDED.page_type,
            status = EXCLUDED.status,
            summary = EXCLUDED.summary,
            aliases = EXCLUDED.aliases,
            source_refs = EXCLUDED.source_refs,
            chunk_refs = EXCLUDED.chunk_refs,
            category_path = EXCLUDED.category_path,
            folder_id = EXCLUDED.folder_id,
            updated_at = now(),
            deleted_at = NULL",
    )
    .bind(page.id)
    .bind(page.product_version_id)
    .bind(&page.slug)
    .bind(&page.title)
    .bind(&page.content)
    .bind(&page.page_type)
    .bind(&page.status)
    .bind(&page.summary)
    .bind(serde_json::json!(page.aliases))
    .bind(serde_json::json!(refs))
    .bind(serde_json::json!(
        page.chunk_refs
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
    ))
    .bind(serde_json::json!(page.category_path))
    .bind(page.folder_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn upsert_wiki_folder(
    pool: &PgPool,
    folder: &crate::WikiFolder,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO wiki_folders
            (id, product_version_id, parent_id, name, path, depth, sort_order)
         VALUES ($1,$2,$3,$4,$5,$6,$7)
         ON CONFLICT (id) DO UPDATE SET
            name = EXCLUDED.name,
            path = EXCLUDED.path,
            parent_id = EXCLUDED.parent_id,
            depth = EXCLUDED.depth,
            sort_order = EXCLUDED.sort_order,
            updated_at = now(),
            deleted_at = NULL",
    )
    .bind(folder.id)
    .bind(folder.product_version_id)
    .bind(folder.parent_id)
    .bind(&folder.name)
    .bind(&folder.path)
    .bind(folder.depth)
    .bind(folder.sort_order)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn append_document_chunks(
    pool: &PgPool,
    chunks: &[crate::Chunk],
    embeddings: &[crate::ChunkEmbedding],
) -> Result<(), sqlx::Error> {
    use sha2::{Digest, Sha256};
    struct PreparedImage {
        chunk_id: Uuid,
        product_version_id: Uuid,
        document_id: Uuid,
        id: Uuid,
        object_ref: String,
        sha256: String,
        media_type: String,
        byte_length: i64,
        width: i32,
        height: i32,
        source_key: String,
        payload: Vec<u8>,
    }
    let mut prepared = Vec::new();
    for chunk in chunks
        .iter()
        .filter(|chunk| chunk.chunk_type == "image_ocr")
    {
        if !chunk.context_header.starts_with("objects/") {
            return Err(sqlx::Error::Protocol(
                "image OCR chunk requires an objects/{sha256} source identity".into(),
            ));
        }
        let sha256 = chunk
            .context_header
            .strip_prefix("objects/")
            .unwrap_or_default()
            .to_owned();
        if sha256.len() != 64
            || !sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(sqlx::Error::Protocol(
                "image OCR source object identity is invalid".into(),
            ));
        }
        let digest_for_read = sha256.clone();
        let bytes = tokio::task::spawn_blocking(move || platform::read_blob(&digest_for_read))
            .await
            .map_err(|error| sqlx::Error::Protocol(format!("join image media read: {error}")))?
            .map_err(|error| sqlx::Error::Protocol(format!("read image media: {error}")))?;
        if hex::encode(Sha256::digest(&bytes)) != sha256 {
            return Err(sqlx::Error::Protocol(
                "image media object digest mismatch".into(),
            ));
        }
        let byte_length = i64::try_from(bytes.len())
            .map_err(|_| sqlx::Error::Protocol("image media byte length overflow".into()))?;
        let format = image::guess_format(&bytes)
            .map_err(|error| sqlx::Error::Protocol(format!("image media format: {error}")))?;
        let media_type = match format {
            image::ImageFormat::Png => "image/png",
            image::ImageFormat::Jpeg => "image/jpeg",
            image::ImageFormat::WebP => "image/webp",
            _ => {
                return Err(sqlx::Error::Protocol(
                    "image OCR media type is unsupported".into(),
                ));
            }
        }
        .to_owned();
        let image = image::load_from_memory_with_format(&bytes, format)
            .map_err(|error| sqlx::Error::Protocol(format!("decode image media: {error}")))?;
        let width = i32::try_from(image.width())
            .map_err(|_| sqlx::Error::Protocol("image width overflow".into()))?;
        let height = i32::try_from(image.height())
            .map_err(|_| sqlx::Error::Protocol("image height overflow".into()))?;
        let mut identity = Sha256::new();
        identity.update(chunk.id.as_bytes());
        identity.update(sha256.as_bytes());
        let mut id_bytes: [u8; 16] = identity.finalize()[..16].try_into().expect("sha256 prefix");
        id_bytes[6] = (id_bytes[6] & 0x0f) | 0x50;
        id_bytes[8] = (id_bytes[8] & 0x3f) | 0x80;
        let id = Uuid::from_bytes(id_bytes);
        let payload=serde_json::to_vec(&serde_json::json!({"schema_version":1,"image_artifact_revision_id":id,
            "product_version_id":chunk.product_version_id,"document_id":chunk.document_id,"revision":1,
            "object_ref":chunk.context_header,"sha256":sha256,"media_type":media_type,"width":width,"height":height,
            "page_ordinal":null,"bounding_region":null,"source_image_key":chunk.context_header}))
            .map_err(|error|sqlx::Error::Protocol(error.to_string()))?;
        prepared.push(PreparedImage {
            chunk_id: chunk.id,
            product_version_id: chunk.product_version_id,
            document_id: chunk.document_id,
            id,
            object_ref: chunk.context_header.clone(),
            sha256,
            media_type,
            byte_length,
            width,
            height,
            source_key: chunk.context_header.clone(),
            payload,
        });
    }
    let mut tx = pool.begin().await?;
    for ch in chunks {
        sqlx::query(
            "INSERT INTO chunks (
                id, product_version_id, document_id, chunk_type, content,
                context_header, start_at, end_at, parent_chunk_id, generated_questions
             ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
             ON CONFLICT (id) DO UPDATE SET
                content = EXCLUDED.content,
                generated_questions = EXCLUDED.generated_questions",
        )
        .bind(ch.id)
        .bind(ch.product_version_id)
        .bind(ch.document_id)
        .bind(&ch.chunk_type)
        .bind(&ch.content)
        .bind(&ch.context_header)
        .bind(ch.start_at)
        .bind(ch.end_at)
        .bind(ch.parent_chunk_id)
        .bind(serde_json::json!(ch.generated_questions))
        .execute(&mut *tx)
        .await?;
    }
    for media in prepared {
        sqlx::query("SELECT kb_register_knowledge_image_object($1,$2::kb_object_ref,$3::kb_sha256,$4,$5,$6::kb_actor_identity)")
            .bind(media.id).bind(&media.object_ref).bind(&media.sha256).bind(&media.media_type)
            .bind(media.byte_length).bind("system:knowledge-document-ingest").execute(&mut *tx).await?;
        let artifact_sha = hex::encode(Sha256::digest(&media.payload));
        sqlx::query("INSERT INTO knowledge_image_artifact_revisions(id,product_version_id,document_id,revision,
            object_ref,content_sha256,media_type,width,height,page_ordinal,bounding_region,source_image_key,canonical_payload,artifact_sha256)
          VALUES($1,$2,$3,1,$4,$5,$6,$7,$8,NULL,NULL,$9,$10,$11) ON CONFLICT(id) DO NOTHING")
            .bind(media.id).bind(media.product_version_id).bind(media.document_id).bind(&media.object_ref)
            .bind(&media.sha256).bind(&media.media_type).bind(media.width).bind(media.height).bind(&media.source_key)
            .bind(&media.payload).bind(&artifact_sha).execute(&mut *tx).await?;
        let artifact_matches:bool=sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM knowledge_image_artifact_revisions
            WHERE id=$1 AND product_version_id=$2 AND document_id=$3 AND revision=1 AND object_ref=$4
              AND content_sha256=$5 AND media_type=$6 AND width=$7 AND height=$8 AND source_image_key=$9
              AND canonical_payload=$10 AND artifact_sha256=$11)")
            .bind(media.id).bind(media.product_version_id).bind(media.document_id).bind(&media.object_ref)
            .bind(&media.sha256).bind(&media.media_type).bind(media.width).bind(media.height).bind(&media.source_key)
            .bind(&media.payload).bind(&artifact_sha).fetch_one(&mut *tx).await?;
        if !artifact_matches {
            return Err(sqlx::Error::Protocol(
                "image artifact idempotency conflict".into(),
            ));
        }
        sqlx::query("INSERT INTO knowledge_image_ocr_chunk_artifact_mappings(chunk_id,product_version_id,document_id,
            image_artifact_revision_id,object_ref,content_sha256,media_type)
          VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(chunk_id) DO NOTHING")
            .bind(media.chunk_id).bind(media.product_version_id).bind(media.document_id).bind(media.id)
            .bind(&media.object_ref).bind(&media.sha256).bind(&media.media_type).execute(&mut *tx).await?;
        let mapping_matches:bool=sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM knowledge_image_ocr_chunk_artifact_mappings
            WHERE chunk_id=$1 AND product_version_id=$2 AND document_id=$3 AND image_artifact_revision_id=$4
              AND object_ref=$5 AND content_sha256=$6 AND media_type=$7)")
            .bind(media.chunk_id).bind(media.product_version_id).bind(media.document_id).bind(media.id)
            .bind(&media.object_ref).bind(&media.sha256).bind(&media.media_type).fetch_one(&mut *tx).await?;
        if !mapping_matches {
            return Err(sqlx::Error::Protocol(
                "image OCR mapping idempotency conflict".into(),
            ));
        }
    }
    for e in embeddings {
        let lit = vector_literal(&e.vector);
        sqlx::query(
            "INSERT INTO chunk_embeddings
                (chunk_id, product_version_id, document_id, embedding, tsv, content)
             VALUES ($1,$2,$3, CAST($4 AS vector), to_tsvector('simple', $5), $5)
             ON CONFLICT (chunk_id) DO UPDATE SET
                embedding = EXCLUDED.embedding,
                tsv = EXCLUDED.tsv,
                content = EXCLUDED.content",
        )
        .bind(e.chunk_id)
        .bind(e.product_version_id)
        .bind(e.document_id)
        .bind(&lit)
        .bind(&e.content)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await
}

pub async fn delete_wiki_for_document(
    pool: &PgPool,
    version_id: Uuid,
    document_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM chunks WHERE document_id = $1 AND chunk_type = 'wiki_page'")
        .bind(document_id)
        .execute(pool)
        .await?;
    sqlx::query(
        "DELETE FROM wiki_pages WHERE product_version_id = $1 AND source_refs @> jsonb_build_array($2::text)",
    )
    .bind(version_id)
    .bind(document_id.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_wiki_folders(
    pool: &PgPool,
    version_id: Uuid,
) -> Result<Vec<(Uuid, String, String, i32)>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, name, path, depth FROM wiki_folders
         WHERE product_version_id = $1 AND deleted_at IS NULL
         ORDER BY sort_order, name",
    )
    .bind(version_id)
    .fetch_all(pool)
    .await?;
    let mut out = Vec::new();
    for r in rows {
        out.push((
            r.try_get("id")?,
            r.try_get("name")?,
            r.try_get("path")?,
            r.try_get("depth")?,
        ));
    }
    Ok(out)
}

pub async fn version_references_object(
    pool: &PgPool,
    version_id: Uuid,
    key: &str,
    hash: &str,
) -> Result<bool, sqlx::Error> {
    let object_ref = if key.starts_with("objects/") {
        key.to_string()
    } else {
        format!("objects/{hash}")
    };
    sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM documents
            WHERE product_version_id = $1
              AND deleted_at IS NULL
              AND (object_ref = $2 OR object_ref = $3 OR file_hash = $4)
         ) OR EXISTS (
            SELECT 1 FROM chunks
            WHERE product_version_id = $1
              AND (position($2 in content) > 0 OR position($3 in content) > 0)
         )",
    )
    .bind(version_id)
    .bind(&object_ref)
    .bind(key)
    .bind(hash)
    .fetch_one(pool)
    .await
}

pub async fn version_wiki_enabled(pool: &PgPool, version_id: Uuid) -> Result<bool, sqlx::Error> {
    let row: Option<bool> = sqlx::query_scalar(
        "SELECT COALESCE((indexing_strategy->>'wiki')::boolean, true)
         FROM product_versions WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(version_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.unwrap_or(false))
}

pub async fn finalize_subtask(pool: &PgPool, document_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE documents SET
            pending_subtasks_count = GREATEST(pending_subtasks_count - 1, 0),
            parse_status = CASE
                WHEN parse_status = 'finalizing'
                     AND pending_subtasks_count <= 1
                     AND COALESCE(error_message, '') NOT LIKE '%ocr_error%'
                     AND COALESCE(error_message, '') NOT LIKE '%caption_error%'
                THEN 'completed'
                ELSE parse_status
            END,
            updated_at = now()
         WHERE id = $1",
    )
    .bind(document_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub struct NewApiKey<'a> {
    pub id: Uuid,
    pub name: &'a str,
    pub key_hash: &'a str,
    pub prefix: &'a str,
    pub scope_type: &'a str,
    pub scope_id: Uuid,
    pub scopes: &'a [String],
}

pub async fn insert_api_key(pool: &PgPool, key: NewApiKey<'_>) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO api_keys (id, name, key_hash, prefix, scope_type, scope_id, scopes)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(key.id)
    .bind(key.name)
    .bind(key.key_hash)
    .bind(key.prefix)
    .bind(key.scope_type)
    .bind(key.scope_id)
    .bind(key.scopes)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_api_key(pool: &PgPool, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM api_keys WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn find_api_key_by_hash(
    pool: &PgPool,
    key_hash: &str,
) -> Result<Option<crate::ApiKey>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, name, key_hash, prefix, scope_type, scope_id, scopes FROM api_keys WHERE key_hash = $1",
    )
    .bind(key_hash)
    .fetch_optional(pool)
    .await?;
    let Some(r) = row else {
        return Ok(None);
    };
    Ok(Some(crate::ApiKey {
        id: r.try_get("id")?,
        name: r.try_get("name")?,
        key_hash: r.try_get("key_hash")?,
        prefix: r.try_get("prefix")?,
        scope_type: r.try_get("scope_type")?,
        scope_id: r.try_get("scope_id")?,
        scopes: r.try_get("scopes")?,
    }))
}

pub async fn find_user_by_id(
    pool: &PgPool,
    id: Uuid,
) -> Result<Option<(Uuid, String)>, sqlx::Error> {
    let row = sqlx::query("SELECT id, email FROM users WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    let Some(r) = row else {
        return Ok(None);
    };
    Ok(Some((r.try_get("id")?, r.try_get("email")?)))
}

pub async fn find_user_by_email(
    pool: &PgPool,
    email: &str,
) -> Result<Option<(Uuid, String, String)>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, email, COALESCE(password_hash, '') AS password_hash FROM users WHERE email = $1",
    )
    .bind(email)
    .fetch_optional(pool)
    .await?;
    let Some(r) = row else {
        return Ok(None);
    };
    Ok(Some((
        r.try_get("id")?,
        r.try_get("email")?,
        r.try_get("password_hash")?,
    )))
}

pub async fn embeddings_schema_ready(pool: &PgPool) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM information_schema.tables
            WHERE table_schema = 'public' AND table_name = 'chunk_embeddings'
        )",
    )
    .fetch_one(pool)
    .await
}

/// Copy chunks + embeddings (vector, tsv, questions) onto a new document id.
pub async fn copy_document_index(
    pool: &PgPool,
    source_document_id: Uuid,
    target_document_id: Uuid,
    target_version_id: Uuid,
) -> Result<usize, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, chunk_type, content, context_header, start_at, end_at,
                parent_chunk_id, generated_questions
         FROM chunks WHERE document_id = $1",
    )
    .bind(source_document_id)
    .fetch_all(pool)
    .await?;
    let mut id_map: std::collections::HashMap<Uuid, Uuid> = std::collections::HashMap::new();
    for r in &rows {
        let old: Uuid = r.try_get("id")?;
        id_map.insert(old, Uuid::new_v4());
    }
    for r in &rows {
        let old: Uuid = r.try_get("id")?;
        let new_id = id_map[&old];
        let parent: Option<Uuid> = r.try_get("parent_chunk_id")?;
        let parent = parent.and_then(|p| id_map.get(&p).copied());
        sqlx::query(
            "INSERT INTO chunks (
                id, product_version_id, document_id, chunk_type, content,
                context_header, start_at, end_at, parent_chunk_id, generated_questions
             ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
        )
        .bind(new_id)
        .bind(target_version_id)
        .bind(target_document_id)
        .bind(r.try_get::<String, _>("chunk_type")?)
        .bind(r.try_get::<String, _>("content")?)
        .bind(r.try_get::<String, _>("context_header")?)
        .bind(r.try_get::<i32, _>("start_at")?)
        .bind(r.try_get::<i32, _>("end_at")?)
        .bind(parent)
        .bind(r.try_get::<serde_json::Value, _>("generated_questions")?)
        .execute(pool)
        .await?;
    }
    for (old, new_id) in &id_map {
        sqlx::query(
            "INSERT INTO chunk_embeddings
                (chunk_id, product_version_id, document_id, embedding, tsv, content)
             SELECT $1, $2, $3, e.embedding, e.tsv, e.content
             FROM chunk_embeddings e WHERE e.chunk_id = $4",
        )
        .bind(new_id)
        .bind(target_version_id)
        .bind(target_document_id)
        .bind(old)
        .execute(pool)
        .await?;
    }
    Ok(rows.len())
}

pub async fn document_chunk_count(pool: &PgPool, document_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM chunks WHERE document_id = $1")
        .bind(document_id)
        .fetch_one(pool)
        .await
}

pub async fn delete_chunks_by_types(
    pool: &PgPool,
    document_id: Uuid,
    types: &[&str],
) -> Result<(), sqlx::Error> {
    let types: Vec<String> = types.iter().map(|s| (*s).to_string()).collect();
    sqlx::query("DELETE FROM chunks WHERE document_id = $1 AND chunk_type = ANY($2)")
        .bind(document_id)
        .bind(&types)
        .execute(pool)
        .await?;
    Ok(())
}

pub fn vector_literal(v: &[f32]) -> String {
    let padded = if v.is_empty() {
        vec![0.0; crate::models::EMBEDDING_DIM]
    } else {
        v.to_vec()
    };
    let body: Vec<String> = padded.iter().map(|x| format!("{x}")).collect();
    format!("[{}]", body.join(","))
}

pub async fn insert_document_chunks(
    pool: &PgPool,
    chunks: &[crate::Chunk],
    embeddings: &[crate::ChunkEmbedding],
) -> Result<(), sqlx::Error> {
    for ch in chunks {
        sqlx::query(
            "INSERT INTO chunks (
                id, product_version_id, document_id, chunk_type, content,
                context_header, start_at, end_at, parent_chunk_id, generated_questions
             ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
        )
        .bind(ch.id)
        .bind(ch.product_version_id)
        .bind(ch.document_id)
        .bind(&ch.chunk_type)
        .bind(&ch.content)
        .bind(&ch.context_header)
        .bind(ch.start_at)
        .bind(ch.end_at)
        .bind(ch.parent_chunk_id)
        .bind(serde_json::json!(ch.generated_questions))
        .execute(pool)
        .await?;
    }
    for e in embeddings {
        let lit = vector_literal(&e.vector);
        sqlx::query(
            "INSERT INTO chunk_embeddings
                (chunk_id, product_version_id, document_id, embedding, tsv, content)
             VALUES ($1,$2,$3, CAST($4 AS vector), to_tsvector('simple', $5), $5)",
        )
        .bind(e.chunk_id)
        .bind(e.product_version_id)
        .bind(e.document_id)
        .bind(&lit)
        .bind(&e.content)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub async fn replace_document_chunks(
    pool: &PgPool,
    document_id: Uuid,
    chunks: &[crate::Chunk],
    embeddings: &[crate::ChunkEmbedding],
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM chunks WHERE document_id = $1")
        .bind(document_id)
        .execute(pool)
        .await?;
    insert_document_chunks(pool, chunks, embeddings).await
}

pub async fn replace_document_embeddings(
    pool: &PgPool,
    document_id: Uuid,
    embeddings: &[crate::ChunkEmbedding],
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM chunk_embeddings WHERE document_id = $1")
        .bind(document_id)
        .execute(pool)
        .await?;
    insert_document_chunks(pool, &[], embeddings).await
}

pub async fn load_document_chunks(
    pool: &PgPool,
    document_id: Uuid,
) -> Result<Vec<crate::Chunk>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, document_id, product_version_id, chunk_type, content,
                context_header, start_at, end_at, parent_chunk_id, generated_questions
         FROM chunks WHERE document_id = $1
         ORDER BY start_at, id",
    )
    .bind(document_id)
    .fetch_all(pool)
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for c in rows {
        let qs: serde_json::Value = c
            .try_get("generated_questions")
            .unwrap_or_else(|_| serde_json::json!([]));
        out.push(crate::Chunk {
            id: c.try_get("id")?,
            document_id: c.try_get("document_id")?,
            product_version_id: c.try_get("product_version_id")?,
            chunk_type: c.try_get("chunk_type")?,
            content: c.try_get("content")?,
            context_header: c.try_get("context_header").unwrap_or_default(),
            start_at: c.try_get("start_at")?,
            end_at: c.try_get("end_at")?,
            parent_chunk_id: c.try_get("parent_chunk_id")?,
            generated_questions: serde_json::from_value(qs).unwrap_or_default(),
        });
    }
    Ok(out)
}

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

fn pg_hit_from_row(r: &sqlx::postgres::PgRow) -> Result<PgSearchHit, sqlx::Error> {
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

pub async fn embedding_models_for_versions(
    pool: &PgPool,
    version_ids: &[Uuid],
) -> Result<Vec<(Uuid, String)>, sqlx::Error> {
    if version_ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query(
        "SELECT id, COALESCE(embedding_model_id, '') AS emb
         FROM product_versions WHERE id = ANY($1)",
    )
    .bind(version_ids)
    .fetch_all(pool)
    .await?;
    let mut out = Vec::new();
    for r in rows {
        out.push((r.try_get("id")?, r.try_get("emb")?));
    }
    Ok(out)
}

pub async fn workspace_thresholds_for_product(
    pool: &PgPool,
    product_id: Uuid,
) -> Result<(f64, f64), sqlx::Error> {
    let cfg: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT w.retrieval_config FROM workspaces w
         JOIN products p ON p.workspace_id = w.id WHERE p.id = $1",
    )
    .bind(product_id)
    .fetch_optional(pool)
    .await?;
    Ok(thresholds_from_cfg(cfg.as_ref()))
}

pub async fn workspace_thresholds_for_version(
    pool: &PgPool,
    version_id: Uuid,
) -> Result<(f64, f64), sqlx::Error> {
    let cfg: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT w.retrieval_config FROM workspaces w
         JOIN products p ON p.workspace_id = w.id
         JOIN product_versions pv ON pv.product_id = p.id
         WHERE pv.id = $1",
    )
    .bind(version_id)
    .fetch_optional(pool)
    .await?;
    Ok(thresholds_from_cfg(cfg.as_ref()))
}

pub async fn set_retrieval_config(
    pool: &PgPool,
    workspace_id: Uuid,
    vector_threshold: f64,
    keyword_threshold: f64,
    embedding_top_k: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE workspaces SET retrieval_config = $2::jsonb WHERE id = $1")
        .bind(workspace_id)
        .bind(serde_json::json!({
            "vector_threshold": vector_threshold,
            "keyword_threshold": keyword_threshold,
            "embedding_top_k": embedding_top_k,
        }))
        .execute(pool)
        .await?;
    Ok(())
}

fn thresholds_from_cfg(cfg: Option<&serde_json::Value>) -> (f64, f64) {
    let vth = cfg
        .and_then(|c| c.get("vector_threshold"))
        .and_then(|x| x.as_f64())
        .unwrap_or(0.15);
    let kth = cfg
        .and_then(|c| c.get("keyword_threshold"))
        .and_then(|x| x.as_f64())
        .unwrap_or(0.3);
    (vth, kth)
}

pub async fn version_indexing_flags(
    pool: &PgPool,
    version_id: Uuid,
) -> Result<(bool, bool), sqlx::Error> {
    let idx: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT COALESCE(indexing_strategy, '{}'::jsonb) FROM product_versions WHERE id = $1",
    )
    .bind(version_id)
    .fetch_optional(pool)
    .await?;
    let idx = idx.unwrap_or_else(|| serde_json::json!({}));
    Ok((
        idx.get("vector").and_then(|v| v.as_bool()).unwrap_or(true),
        idx.get("keyword").and_then(|v| v.as_bool()).unwrap_or(true),
    ))
}

pub async fn workspace_top_k_for_version(
    pool: &PgPool,
    version_id: Uuid,
) -> Result<usize, sqlx::Error> {
    let cfg: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT w.retrieval_config FROM workspaces w
         JOIN products p ON p.workspace_id = w.id
         JOIN product_versions pv ON pv.product_id = p.id
         WHERE pv.id = $1",
    )
    .bind(version_id)
    .fetch_optional(pool)
    .await?;
    let n = cfg
        .as_ref()
        .and_then(|c| c.get("embedding_top_k"))
        .and_then(|x| x.as_u64())
        .unwrap_or(50) as usize;
    Ok(n.max(1))
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

pub async fn table_names(pool: &PgPool) -> Result<Vec<String>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT tablename FROM pg_tables WHERE schemaname = 'public' ORDER BY tablename",
    )
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|r| r.try_get::<String, _>("tablename"))
        .collect()
}

pub async fn column_names(pool: &PgPool, table: &str) -> Result<Vec<String>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT column_name FROM information_schema.columns
         WHERE table_schema = 'public' AND table_name = $1",
    )
    .bind(table)
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|r| r.try_get::<String, _>("column_name"))
        .collect()
}

pub async fn product_name(pool: &PgPool, product_id: Uuid) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT name FROM products WHERE id = $1")
        .bind(product_id)
        .fetch_optional(pool)
        .await
}

pub async fn document_file_meta(
    pool: &PgPool,
    document_id: Uuid,
) -> Result<Option<(String, String)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT COALESCE(file_name, ''), COALESCE(object_ref, '')
         FROM documents WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(document_id)
    .fetch_optional(pool)
    .await
}

pub async fn document_image_object_refs(
    pool: &PgPool,
    document_id: Uuid,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT DISTINCT context_header FROM chunks
         WHERE document_id = $1
           AND chunk_type IN ('image_ocr', 'image_caption')
           AND context_header IS NOT NULL AND context_header <> ''",
    )
    .bind(document_id)
    .fetch_all(pool)
    .await
}
