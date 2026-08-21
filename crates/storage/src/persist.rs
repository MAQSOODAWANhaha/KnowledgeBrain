use sqlx::{PgPool, Row};
use uuid::Uuid;

pub const MIGRATION_0001: &str = include_str!("../../../migrations/0001_domain.sql");
pub const MIGRATION_0002: &str = include_str!("../../../migrations/0002_models.sql");
pub const MIGRATION_0003: &str = include_str!("../../../migrations/0003_api_keys.sql");
pub const MIGRATION_0004: &str = include_str!("../../../migrations/0004_embeddings.sql");
pub const MIGRATION_0005: &str = include_str!("../../../migrations/0005_graph.sql");
pub const MIGRATION_0006: &str = include_str!("../../../migrations/0006_wiki.sql");
pub const MIGRATION_0007: &str = include_str!("../../../migrations/0007_bid.sql");
pub const MIGRATION_0008: &str = include_str!("../../../migrations/0008_backfill.sql");
pub const MIGRATION_0009: &str = include_str!("../../../migrations/0009_bid_extract_running.sql");

pub fn database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgres://knowledgebrain:knowledgebrain@127.0.0.1:15432/knowledgebrain".into()
    })
}

pub async fn connect() -> Result<PgPool, sqlx::Error> {
    let pool = PgPool::connect(&database_url()).await?;
    apply_0001(&pool).await?;
    Ok(pool)
}

pub async fn apply_0001(pool: &PgPool) -> Result<(), sqlx::Error> {
    // API and worker can start together against an empty database. Serialize the
    // repository-local idempotent migration bundle across processes.
    const MIGRATION_LOCK_ID: i64 = 0x4b_42_4d_49_47_52_41_54;
    let mut lock_connection = pool.acquire().await?;
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(MIGRATION_LOCK_ID)
        .execute(&mut *lock_connection)
        .await?;
    let result = apply_migrations(pool).await;
    let unlock_result = sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(MIGRATION_LOCK_ID)
        .execute(&mut *lock_connection)
        .await;
    result?;
    unlock_result?;
    Ok(())
}

async fn apply_migrations(pool: &PgPool) -> Result<(), sqlx::Error> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM information_schema.tables
            WHERE table_schema = 'public' AND table_name = 'workspaces'
        )",
    )
    .fetch_one(pool)
    .await?;
    if !exists {
        sqlx::raw_sql(MIGRATION_0001).execute(pool).await?;
    }
    sqlx::raw_sql(MIGRATION_0002).execute(pool).await?;
    sqlx::raw_sql(MIGRATION_0003).execute(pool).await?;
    sqlx::raw_sql(MIGRATION_0004).execute(pool).await?;
    sqlx::raw_sql(MIGRATION_0005).execute(pool).await?;
    sqlx::raw_sql(MIGRATION_0006).execute(pool).await?;
    sqlx::raw_sql(MIGRATION_0007).execute(pool).await?;
    ensure_company_workspace(pool).await?;
    sqlx::raw_sql(MIGRATION_0008).execute(pool).await?;
    sqlx::raw_sql(MIGRATION_0009).execute(pool).await?;
    Ok(())
}

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
fn parse_role(s: &str) -> domain::Role {
    match s {
        "owner" => domain::Role::Owner,
        "admin" => domain::Role::Admin,
        "contributor" => domain::Role::Contributor,
        _ => domain::Role::Viewer,
    }
}

fn parse_kind(s: &str) -> domain::ProductKind {
    if s == "library" {
        domain::ProductKind::Library
    } else {
        domain::ProductKind::Product
    }
}

fn parse_version_status(s: &str) -> domain::VersionStatus {
    match s {
        "cloning" => domain::VersionStatus::Cloning,
        "archived" => domain::VersionStatus::Archived,
        "failed" => domain::VersionStatus::Failed,
        _ => domain::VersionStatus::Active,
    }
}

fn parse_parse_status(s: &str) -> domain::ParseStatus {
    match s {
        "processing" => domain::ParseStatus::Processing,
        "finalizing" => domain::ParseStatus::Finalizing,
        "completed" => domain::ParseStatus::Completed,
        "failed" => domain::ParseStatus::Failed,
        "cancelled" => domain::ParseStatus::Cancelled,
        "deleting" => domain::ParseStatus::Deleting,
        _ => domain::ParseStatus::Pending,
    }
}

/// Load one workspace (members, products, versions, documents, tags,
/// graph, wiki, chunks) into `store`.
pub async fn hydrate_workspace(
    pool: &PgPool,
    store: &mut domain::Store,
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
    let retrieval: domain::RetrievalConfig = r
        .try_get::<serde_json::Value, _>("retrieval_config")
        .ok()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    let kind_s: String = r.try_get("kind").unwrap_or_else(|_| "product_line".into());
    store.workspaces.insert(
        workspace_id,
        domain::Workspace {
            id: workspace_id,
            name: r.try_get("name")?,
            slug: r.try_get("slug")?,
            kind: domain::WorkspaceKind::parse(&kind_s),
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
            domain::Product {
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
            let mut pv = domain::ProductVersion::new(pid, v.try_get("label")?);
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
                "SELECT id, title, file_name, file_size, file_hash, object_key,
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
                let object_key: String = d.try_get("object_key")?;
                let mut doc =
                    domain::Document::new(vid, title, file_name, file_size, file_hash, object_key);
                doc.id = did;
                doc.parse_status = parse_parse_status(&st);
                doc.enable_status = d.try_get("enable_status")?;
                doc.pending_subtasks_count = d.try_get("pending_subtasks_count")?;
                doc.error_message = d.try_get("error_message")?;
                doc.attempt = d.try_get("attempt").unwrap_or(1);
                doc.description = d.try_get("description").unwrap_or_default();
                if let Ok(st) = d.try_get::<String, _>("summary_status") {
                    doc.summary_status = match st.as_str() {
                        "pending" => domain::SummaryStatus::Pending,
                        "processing" => domain::SummaryStatus::Processing,
                        "completed" => domain::SummaryStatus::Completed,
                        "failed" => domain::SummaryStatus::Failed,
                        _ => domain::SummaryStatus::None,
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

async fn hydrate_workspace_index(
    pool: &PgPool,
    store: &mut domain::Store,
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
            domain::Tag {
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
                domain::Chunk {
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
                domain::ChunkEmbedding {
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
                domain::GraphNode {
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
                domain::GraphRelation {
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
                domain::WikiPage {
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
                domain::WikiFolder {
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

pub async fn soft_delete_document(pool: &PgPool, document_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE documents SET deleted_at = now(), updated_at = now() WHERE id = $1")
        .bind(document_id)
        .execute(pool)
        .await?;
    delete_graph_for_document(pool, document_id).await?;
    Ok(())
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
    pub object_key: &'a str,
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

pub async fn list_dead_letters(pool: &PgPool) -> Result<Vec<domain::DeadLetter>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT task_type, COALESCE(related_id, '00000000-0000-0000-0000-000000000000'::uuid), last_error
         FROM task_dead_letters ORDER BY failed_at DESC LIMIT 200",
    )
    .fetch_all(pool)
    .await?;
    let mut out = Vec::new();
    for r in rows {
        out.push(domain::DeadLetter {
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
    sqlx::query(
        "INSERT INTO documents (
            id, product_version_id, title, parse_status, enable_status,
            file_name, file_size, file_hash, object_key, type
         ) VALUES ($1, $2, $3, 'pending', 'disabled', $4, $5, $6, $7, 'file')",
    )
    .bind(doc.id)
    .bind(doc.product_version_id)
    .bind(doc.title)
    .bind(doc.file_name)
    .bind(doc.file_size)
    .bind(doc.file_hash)
    .bind(doc.object_key)
    .execute(pool)
    .await?;
    Ok(())
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
    store: &domain::Store,
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
    store: &domain::Store,
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
    overrides: &domain::ProcessOverrides,
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
    store: &domain::Store,
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
    pub fn into_span(self) -> domain::Span {
        domain::Span {
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

pub async fn release_object_ref(pool: &PgPool, hash: &str) -> Result<i32, sqlx::Error> {
    let n: Option<i32> = sqlx::query_scalar(
        "UPDATE content_objects SET refcount = GREATEST(refcount - 1, 0)
         WHERE hash = $1
         RETURNING refcount",
    )
    .bind(hash)
    .fetch_optional(pool)
    .await?;
    let left = n.unwrap_or(-1);
    if left == 0 {
        sqlx::query("DELETE FROM content_objects WHERE hash = $1 AND refcount <= 0")
            .bind(hash)
            .execute(pool)
            .await?;
        crate::drop_blob(hash);
    }
    Ok(left)
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

pub async fn bump_object_ref(pool: &PgPool, hash: &str, size: i64) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO content_objects (hash, size, refcount) VALUES ($1, $2, 1)
         ON CONFLICT (hash) DO UPDATE SET refcount = content_objects.refcount + 1",
    )
    .bind(hash)
    .bind(size)
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
    chunks: &[domain::Chunk],
    embeddings: &[domain::ChunkEmbedding],
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
    page: &domain::WikiPage,
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
    folder: &domain::WikiFolder,
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
    chunks: &[domain::Chunk],
    embeddings: &[domain::ChunkEmbedding],
) -> Result<(), sqlx::Error> {
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
        .execute(pool)
        .await?;
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
        .execute(pool)
        .await?;
    }
    Ok(())
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
    let object_key = if key.starts_with("objects/") {
        key.to_string()
    } else {
        format!("objects/{hash}")
    };
    sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM documents
            WHERE product_version_id = $1
              AND deleted_at IS NULL
              AND (object_key = $2 OR object_key = $3 OR file_hash = $4)
         ) OR EXISTS (
            SELECT 1 FROM chunks
            WHERE product_version_id = $1
              AND (position($2 in content) > 0 OR position($3 in content) > 0)
         )",
    )
    .bind(version_id)
    .bind(&object_key)
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
                     AND error_message NOT LIKE '%ocr_error%'
                     AND error_message NOT LIKE '%caption_error%'
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
) -> Result<Option<domain::ApiKey>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, name, key_hash, prefix, scope_type, scope_id, scopes FROM api_keys WHERE key_hash = $1",
    )
    .bind(key_hash)
    .fetch_optional(pool)
    .await?;
    let Some(r) = row else {
        return Ok(None);
    };
    Ok(Some(domain::ApiKey {
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
        vec![0.0; models::EMBEDDING_DIM]
    } else {
        v.to_vec()
    };
    let body: Vec<String> = padded.iter().map(|x| format!("{x}")).collect();
    format!("[{}]", body.join(","))
}

pub async fn insert_document_chunks(
    pool: &PgPool,
    chunks: &[domain::Chunk],
    embeddings: &[domain::ChunkEmbedding],
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
    chunks: &[domain::Chunk],
    embeddings: &[domain::ChunkEmbedding],
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
    embeddings: &[domain::ChunkEmbedding],
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
) -> Result<Vec<domain::Chunk>, sqlx::Error> {
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
        out.push(domain::Chunk {
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
    pub document_object_key: String,
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
        document_object_key: r.try_get("document_object_key").unwrap_or_default(),
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
                COALESCE(d.object_key, '') AS document_object_key, d.title,
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
                COALESCE(d.object_key, '') AS document_object_key, d.title,
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
        "SELECT COALESCE(file_name, ''), COALESCE(object_key, '')
         FROM documents WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(document_id)
    .fetch_optional(pool)
    .await
}

pub async fn document_image_object_keys(
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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn summary_append_keeps_text_and_soft_delete_frees_unique() {
        let _g = db_lock().await;
        let Some(pool) = setup().await else {
            return;
        };
        let owner = Uuid::new_v4();
        insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
            .await
            .unwrap();
        let seeded = create_workspace_with_library(&pool, owner, "Unq", "unq")
            .await
            .unwrap();
        let did = Uuid::new_v4();
        insert_document(
            &pool,
            NewDocument {
                id: did,
                product_version_id: seeded.library_version_id,
                title: "t",
                file_name: "same.txt",
                file_size: 4,
                file_hash: "samehash",
                object_key: "objects/samehash",
            },
        )
        .await
        .unwrap();
        let cid = Uuid::new_v4();
        replace_document_chunks(
            &pool,
            did,
            &[domain::Chunk {
                id: cid,
                document_id: did,
                product_version_id: seeded.library_version_id,
                chunk_type: "text".into(),
                content: "body".into(),
                context_header: String::new(),
                start_at: 0,
                end_at: 4,
                parent_chunk_id: None,
                generated_questions: vec![],
            }],
            &[],
        )
        .await
        .unwrap();
        let mut store = domain::Store::default();
        let sid = Uuid::new_v4();
        store.chunks.insert(
            sid,
            domain::Chunk {
                id: sid,
                document_id: did,
                product_version_id: seeded.library_version_id,
                chunk_type: "summary".into(),
                content: "sum".into(),
                context_header: String::new(),
                start_at: 0,
                end_at: 3,
                parent_chunk_id: Some(cid),
                generated_questions: vec![],
            },
        );
        persist_summary_chunks(&pool, &store, did).await.unwrap();
        let text_n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM chunks WHERE document_id = $1 AND chunk_type = 'text'",
        )
        .bind(did)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(text_n, 1);
        let sum_n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM chunks WHERE document_id = $1 AND chunk_type = 'summary'",
        )
        .bind(did)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(sum_n, 1);

        soft_delete_document(&pool, did).await.unwrap();
        let did2 = Uuid::new_v4();
        insert_document(
            &pool,
            NewDocument {
                id: did2,
                product_version_id: seeded.library_version_id,
                title: "t",
                file_name: "same.txt",
                file_size: 4,
                file_hash: "samehash",
                object_key: "objects/samehash",
            },
        )
        .await
        .expect("partial unique must allow re-upload after soft-delete");

        sqlx::query("UPDATE documents SET attempt = 4, description = 'd' WHERE id = $1")
            .bind(did2)
            .execute(&pool)
            .await
            .unwrap();
        let mut loaded = domain::Store::default();
        hydrate_workspace(&pool, &mut loaded, seeded.workspace_id)
            .await
            .unwrap();
        let got = loaded.documents.get(&did2).expect("hydrated");
        assert_eq!(got.attempt, 4);
        assert_eq!(got.description, "d");
    }

    #[tokio::test]
    async fn soft_delete_version_frees_label() {
        let _g = db_lock().await;
        let Some(pool) = setup().await else {
            return;
        };
        let owner = Uuid::new_v4();
        insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
            .await
            .unwrap();
        let seeded = create_workspace_with_library(&pool, owner, "Lbl", "lbl")
            .await
            .unwrap();
        let pid = Uuid::new_v4();
        let vid1 = Uuid::new_v4();
        insert_product(
            &pool,
            pid,
            seeded.workspace_id,
            "product",
            "P",
            "p",
            Some(vid1),
        )
        .await
        .unwrap();
        insert_version(&pool, vid1, pid, "v1", "active", None)
            .await
            .unwrap();
        sqlx::query(
            "UPDATE product_versions SET status = 'archived', deleted_at = now() WHERE id = $1",
        )
        .bind(vid1)
        .execute(&pool)
        .await
        .unwrap();
        let vid2 = Uuid::new_v4();
        insert_version(&pool, vid2, pid, "v1", "active", None)
            .await
            .expect("partial unique must allow reused label after soft-delete");
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM product_versions WHERE product_id = $1 AND label = 'v1'",
        )
        .bind(pid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(n, 2);
        let live: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM product_versions
             WHERE product_id = $1 AND label = 'v1' AND deleted_at IS NULL",
        )
        .bind(pid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(live, 1);
    }

    #[test]
    fn empty_vector_pads_to_embedding_dim() {
        let lit = vector_literal(&[]);
        assert!(lit.starts_with('['));
        assert_eq!(lit.matches(',').count() + 1, models::EMBEDDING_DIM);
        assert!(!vector_literal(&[0.1; 8]).contains("0, 0, 0"));
    }

    async fn db_lock() -> tokio::sync::MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::const_new(());
        LOCK.lock().await
    }

    async fn setup() -> Option<PgPool> {
        let pool = match PgPool::connect(&database_url()).await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("skip persist test: {e}");
                return None;
            }
        };
        let _ = sqlx::query(
            "DROP TABLE IF EXISTS
                schema_flags,
                bid_booklet_parts, bid_shots, bid_commercial_hits, bid_picks, bid_match_jobs,
                bid_clauses, bid_extract_runs, bid_sections, bid_documents, bid_projects,
                wiki_log_entries, wiki_folders, wiki_pages,
                graph_relations, graph_nodes, chunk_embeddings, chunks,
                api_keys, models,
                task_dead_letters, task_pending_ops, document_processing_spans,
                document_tags, tags, documents, content_objects,
                product_versions, products, workspace_members, users, workspaces
             CASCADE",
        )
        .execute(&pool)
        .await;
        apply_0001(&pool).await.expect("migrate 0001");
        Some(pool)
    }

    #[tokio::test]
    async fn migration_has_spec_tables_and_no_quota() {
        let _g = db_lock().await;
        let Some(pool) = setup().await else {
            return;
        };
        let names = table_names(&pool).await.unwrap();
        for t in [
            "workspaces",
            "users",
            "workspace_members",
            "products",
            "product_versions",
            "documents",
            "tags",
            "document_tags",
            "content_objects",
            "document_processing_spans",
            "task_pending_ops",
            "task_dead_letters",
            "models",
            "api_keys",
            "chunks",
            "chunk_embeddings",
            "graph_nodes",
            "graph_relations",
            "wiki_pages",
            "bid_projects",
            "bid_documents",
            "bid_clauses",
        ] {
            assert!(names.iter().any(|n| n == t), "missing table {t}: {names:?}");
        }
        let cols = column_names(&pool, "workspaces").await.unwrap();
        for banned in ["quota", "token_limit", "tenant_id", "billing"] {
            assert!(
                !cols.iter().any(|c| c.contains(banned)),
                "banned col {banned}"
            );
        }
        let row = sqlx::query("SELECT retrieval_config FROM workspaces LIMIT 0")
            .fetch_optional(&pool)
            .await
            .unwrap();
        let _ = row;
        let project_cols = column_names(&pool, "bid_projects").await.unwrap();
        for required in [
            "extract_lock_token",
            "extract_lock_kind",
            "extract_lock_at",
            "extract_lock_section_id",
        ] {
            assert!(project_cols.iter().any(|column| column == required));
        }
        let run_cols = column_names(&pool, "bid_extract_runs").await.unwrap();
        for required in ["claim_token", "heartbeat_at"] {
            assert!(run_cols.iter().any(|column| column == required));
        }
        let invariant_project = Uuid::new_v4();
        sqlx::query("INSERT INTO bid_projects (id, title) VALUES ($1, 'invariant test')")
            .bind(invariant_project)
            .execute(&pool)
            .await
            .unwrap();
        let invalid_running = sqlx::query(
            "INSERT INTO bid_extract_runs (id, project_id, status) VALUES ($1, $2, 'running')",
        )
        .bind(Uuid::new_v4())
        .bind(invariant_project)
        .execute(&pool)
        .await;
        assert!(
            invalid_running.is_err(),
            "running run requires a live lease"
        );
    }

    #[tokio::test]
    async fn bid_extract_claim_is_serialized_and_stale_owner_is_fenced() {
        let _g = db_lock().await;
        let Some(pool) = setup().await else {
            return;
        };
        let project_id = Uuid::new_v4();
        let run_a = Uuid::new_v4();
        let run_b = Uuid::new_v4();
        sqlx::query("INSERT INTO bid_projects (id, title) VALUES ($1, 'claim test')")
            .bind(project_id)
            .execute(&pool)
            .await
            .unwrap();
        for run_id in [run_a, run_b] {
            crate::bid::insert_extract_run(&pool, run_id, project_id, None, "manual")
                .await
                .unwrap();
        }
        let mismatched =
            crate::bid::claim_extract_run(&pool, run_a, project_id, Some(Uuid::new_v4()))
                .await
                .unwrap();
        assert!(
            mismatched.is_none(),
            "payload identity must match stored run"
        );

        let (claim_a, claim_b) = tokio::join!(
            crate::bid::claim_extract_run(&pool, run_a, project_id, None),
            crate::bid::claim_extract_run(&pool, run_b, project_id, None)
        );
        let claim_a = claim_a.unwrap();
        let claim_b = claim_b.unwrap();
        assert_ne!(claim_a.is_some(), claim_b.is_some());
        let running: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM bid_extract_runs WHERE project_id = $1 AND status = 'running'",
        )
        .bind(project_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(running, 1);

        let (run_id, old_token) = if let Some(token) = claim_a {
            (run_a, token)
        } else {
            (run_b, claim_b.unwrap())
        };
        sqlx::query(
            "UPDATE bid_extract_runs SET heartbeat_at = now() - interval '2 hours' WHERE id = $1",
        )
        .bind(run_id)
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            crate::bid::heartbeat_extract_run(&pool, run_id, project_id, old_token)
                .await
                .unwrap()
        );
        assert!(
            crate::bid::reclaim_stale_extracts(&pool, 60)
                .await
                .unwrap()
                .is_empty(),
            "a live heartbeat must prevent reclaim"
        );
        sqlx::query(
            "UPDATE bid_extract_runs SET heartbeat_at = now() - interval '2 hours' WHERE id = $1",
        )
        .bind(run_id)
        .execute(&pool)
        .await
        .unwrap();
        crate::bid::reclaim_stale_extracts(&pool, 60).await.unwrap();
        let stale_persist = crate::bid::persist_extraction_report(
            &pool,
            crate::bid::PersistExtractionReport {
                run_id,
                claim_token: old_token,
                project_id,
                document_id: Uuid::new_v4(),
                sections: &[],
                clauses: &[],
                replace_document: true,
            },
        )
        .await;
        assert!(stale_persist.is_err(), "stale owner must not persist");

        let diagnostics = serde_json::json!({});
        let finish = crate::bid::finish_extract_run(
            &pool,
            crate::bid::FinishExtractRun {
                id: run_id,
                claim_token: old_token,
                status: "done",
                section_total: 0,
                section_done: 0,
                error_message: "",
                extractor_mode: "heuristic",
                model_id: "",
                policy_version: "cn-tender-v2",
                prompt_version: "clause-extractor-v2",
                diagnostics: &diagnostics,
            },
        )
        .await;
        assert!(finish.is_err(), "stale owner must not finish reclaimed run");

        let document_id = Uuid::new_v4();
        let section_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO bid_documents
                (id, project_id, file_name, file_hash, object_key, parse_status, multimodal_status)
             VALUES ($1, $2, 'retry.md', 'retry-hash', 'objects/retry-hash', 'completed', 'skipped')",
        )
        .bind(document_id)
        .bind(project_id)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO bid_sections
                (id, project_id, document_id, section_key, body, extract_status)
             VALUES ($1, $2, $3, 'retry-section', '必须支持接口', 'done')",
        )
        .bind(section_id)
        .bind(project_id)
        .bind(document_id)
        .execute(&pool)
        .await
        .unwrap();
        let retry_token = crate::bid::claim_section_retry(&pool, project_id, section_id)
            .await
            .unwrap()
            .expect("section retry should claim free project");
        crate::bid::set_section_retry_status(
            &pool,
            project_id,
            section_id,
            retry_token,
            "running",
            "",
        )
        .await
        .unwrap();
        assert!(
            crate::bid::heartbeat_section_retry(&pool, project_id, section_id, retry_token)
                .await
                .unwrap()
        );
        let full_during_retry = crate::bid::claim_extract_run(&pool, run_b, project_id, None)
            .await
            .unwrap();
        assert!(full_during_retry.is_none());
        sqlx::query(
            "UPDATE bid_projects SET extract_lock_at = now() - interval '2 hours' WHERE id = $1",
        )
        .bind(project_id)
        .execute(&pool)
        .await
        .unwrap();
        crate::bid::reclaim_stale_extracts(&pool, 60).await.unwrap();
        let replacement = crate::bid::claim_section_retry(&pool, project_id, section_id)
            .await
            .unwrap()
            .expect("replacement retry should claim reclaimed project");
        assert!(
            crate::bid::set_section_retry_status(
                &pool,
                project_id,
                section_id,
                retry_token,
                "failed",
                "late stale failure",
            )
            .await
            .is_err(),
            "stale retry must not mutate section status"
        );
        crate::bid::set_section_retry_status(
            &pool,
            project_id,
            section_id,
            replacement,
            "running",
            "",
        )
        .await
        .unwrap();
        assert!(
            crate::bid::finish_section_retry(&pool, project_id, section_id, retry_token)
                .await
                .is_err(),
            "stale retry must not release replacement lease"
        );
        crate::bid::finish_section_retry(&pool, project_id, section_id, replacement)
            .await
            .unwrap();
        assert!(crate::bid::end_project(&pool, project_id).await.unwrap());
        assert!(
            crate::bid::claim_extract_run(&pool, run_b, project_id, None)
                .await
                .unwrap()
                .is_none(),
            "ended projects must not start queued extraction"
        );
    }

    #[tokio::test]
    async fn document_mutations_require_project_ownership() {
        let _g = db_lock().await;
        let Some(pool) = setup().await else {
            return;
        };
        let owner = Uuid::new_v4();
        let other = Uuid::new_v4();
        let document_id = Uuid::new_v4();
        for (id, title) in [(owner, "owner"), (other, "other")] {
            sqlx::query("INSERT INTO bid_projects (id, title) VALUES ($1, $2)")
                .bind(id)
                .bind(title)
                .execute(&pool)
                .await
                .unwrap();
        }
        sqlx::query(
            "INSERT INTO bid_documents
                (id, project_id, file_name, file_hash, object_key, parse_status)
             VALUES ($1, $2, 'owned.md', 'owned-hash', 'objects/owned-hash', 'failed')",
        )
        .bind(document_id)
        .bind(owner)
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            !crate::bid::reset_document_for_retry(&pool, other, document_id)
                .await
                .unwrap()
        );
        assert!(
            !crate::bid::delete_document_for_project(&pool, other, document_id)
                .await
                .unwrap()
        );
        let status: String =
            sqlx::query_scalar("SELECT parse_status FROM bid_documents WHERE id = $1")
                .bind(document_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status, "failed");
        assert!(
            crate::bid::reset_document_for_retry(&pool, owner, document_id)
                .await
                .unwrap()
        );
        assert!(
            crate::bid::delete_document_for_project(&pool, owner, document_id)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn bid_section_retry_intent_is_idempotent_and_stale_owner_is_fenced() {
        let _g = db_lock().await;
        let Some(pool) = setup().await else {
            return;
        };
        let project_id = Uuid::new_v4();
        let document_id = Uuid::new_v4();
        let section_id = Uuid::new_v4();
        sqlx::query("INSERT INTO bid_projects (id, title) VALUES ($1, 'section retry')")
            .bind(project_id)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO bid_documents
                (id, project_id, file_name, file_hash, object_key, parse_status, multimodal_status)
             VALUES ($1, $2, 'retry.md', 'hash', 'objects/hash', 'completed', 'skipped')",
        )
        .bind(document_id)
        .bind(project_id)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO bid_sections
                (id, project_id, document_id, section_key, body, extract_status)
             VALUES ($1, $2, $3, 'retry', '必须支持接口', 'failed')",
        )
        .bind(section_id)
        .bind(project_id)
        .bind(document_id)
        .execute(&pool)
        .await
        .unwrap();
        let job_id = crate::bid::enqueue_section_retry(&pool, project_id, section_id)
            .await
            .unwrap();
        assert_eq!(
            job_id,
            crate::bid::enqueue_section_retry(&pool, project_id, section_id)
                .await
                .unwrap()
        );
        let token = crate::bid::claim_section_retry_job(&pool, job_id, project_id, section_id)
            .await
            .unwrap()
            .unwrap();
        let project_token: Option<Uuid> =
            sqlx::query_scalar("SELECT extract_lock_token FROM bid_projects WHERE id = $1")
                .bind(project_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(project_token, Some(token));
        assert!(
            crate::bid::claim_section_retry(&pool, project_id, section_id)
                .await
                .unwrap()
                .is_none(),
            "paired Section retry lease must fence another extraction owner"
        );
        sqlx::query(
            "UPDATE bid_section_retry_jobs
             SET heartbeat_at = now() - interval '2 hours' WHERE id = $1",
        )
        .bind(job_id)
        .execute(&pool)
        .await
        .unwrap();
        let reclaimed = crate::bid::reclaim_stale_section_retry_jobs(&pool, 60)
            .await
            .unwrap();
        assert_eq!(reclaimed, vec![(job_id, project_id, section_id)]);
        let project_token: Option<Uuid> =
            sqlx::query_scalar("SELECT extract_lock_token FROM bid_projects WHERE id = $1")
                .bind(project_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(project_token, None);
        assert!(
            !crate::bid::finish_section_retry_job(
                &pool, job_id, project_id, section_id, token, "done", ""
            )
            .await
            .unwrap(),
            "the reclaimed owner must not finish the durable retry"
        );
    }

    #[tokio::test]
    async fn bid_match_generation_fences_stale_job_and_clause_dirty_is_atomic() {
        let _g = db_lock().await;
        let Some(pool) = setup().await else {
            return;
        };
        let project_id = Uuid::new_v4();
        sqlx::query("INSERT INTO bid_projects (id, title) VALUES ($1, 'match fence')")
            .bind(project_id)
            .execute(&pool)
            .await
            .unwrap();
        let old_job = Uuid::new_v4();
        crate::bid::insert_match_job(&pool, old_job, project_id, 0, "old", "commercial", None)
            .await
            .unwrap();
        let old_token = crate::bid::claim_match_job(&pool, old_job, project_id)
            .await
            .unwrap()
            .unwrap();
        let clause_id = Uuid::new_v4();
        crate::bid::insert_clause(
            &pool,
            crate::bid::NewClause {
                id: clause_id,
                project_id,
                extract_run_id: None,
                section_id: None,
                source_document_id: None,
                source_span: None,
                family_conflict: false,
                extraction_meta: None,
                raw_text: "必须支持接口",
                text: "必须支持接口",
                family: "technical",
                must: true,
                status: "confirmed",
            },
        )
        .await
        .unwrap();
        let candidates = serde_json::json!([]);
        assert!(
            crate::bid::set_match_job(
                &pool,
                crate::bid::MatchJobFinish {
                    id: old_job,
                    project_id,
                    claim_token: old_token,
                    status: "done",
                    tech_status: "done",
                    commercial_status: "skipped",
                    candidates: &candidates,
                    error: "",
                },
            )
            .await
            .is_err(),
            "a prior generation must not publish results"
        );
        let (generation, dirty): (i64, bool) =
            sqlx::query_as("SELECT match_generation, match_dirty FROM bid_projects WHERE id = $1")
                .bind(project_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(generation, 1);
        assert!(dirty);
        assert!(
            crate::bid::update_clause(
                &pool,
                crate::bid::ClausePatch {
                    id: clause_id,
                    project_id,
                    expected_status: "confirmed",
                    text: Some("必须支持双接口"),
                    family: None,
                    must: None,
                    status: None,
                    deviate: None,
                    deviate_note: None,
                    assessment: None,
                },
            )
            .await
            .unwrap()
            .is_some()
        );
        let generation: i64 =
            sqlx::query_scalar("SELECT match_generation FROM bid_projects WHERE id = $1")
                .bind(project_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(generation, 2);
    }

    #[tokio::test]
    async fn bid_match_scheduler_rejects_stale_snapshot_generation() {
        let _g = db_lock().await;
        let Some(pool) = setup().await else {
            return;
        };
        let project_id = Uuid::new_v4();
        sqlx::query("INSERT INTO bid_projects (id, title) VALUES ($1, 'schedule race')")
            .bind(project_id)
            .execute(&pool)
            .await
            .unwrap();

        let mut scheduler = pool.acquire().await.unwrap();
        let stale_generation: i64 =
            sqlx::query_scalar("SELECT match_generation FROM bid_projects WHERE id = $1")
                .bind(project_id)
                .fetch_one(&mut *scheduler)
                .await
                .unwrap();
        let stale_clause_count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM bid_clauses WHERE project_id = $1 AND status = 'confirmed'",
        )
        .bind(project_id)
        .fetch_one(&mut *scheduler)
        .await
        .unwrap();
        assert_eq!(stale_clause_count, 0);

        crate::bid::insert_clause(
            &pool,
            crate::bid::NewClause {
                id: Uuid::new_v4(),
                project_id,
                extract_run_id: None,
                section_id: None,
                source_document_id: None,
                source_span: None,
                family_conflict: false,
                extraction_meta: None,
                raw_text: "投标人须提供营业执照",
                text: "投标人须提供营业执照",
                family: "commercial",
                must: true,
                status: "confirmed",
            },
        )
        .await
        .unwrap();

        assert!(
            crate::bid::insert_match_job(
                &pool,
                Uuid::new_v4(),
                project_id,
                stale_generation,
                "stale-snapshot",
                "commercial",
                None,
            )
            .await
            .is_err(),
            "a scheduler may not relabel its generation-0 snapshot as generation 1"
        );
        let current_generation: i64 =
            sqlx::query_scalar("SELECT match_generation FROM bid_projects WHERE id = $1")
                .bind(project_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        let authoritative = crate::bid::insert_match_job(
            &pool,
            Uuid::new_v4(),
            project_id,
            current_generation,
            "current-snapshot",
            "commercial",
            None,
        )
        .await
        .unwrap();
        let duplicate = crate::bid::insert_match_job(
            &pool,
            Uuid::new_v4(),
            project_id,
            current_generation,
            "duplicate-delivery",
            "commercial",
            None,
        )
        .await
        .unwrap();
        assert_eq!(authoritative, duplicate);
        let technical = crate::bid::insert_match_job(
            &pool,
            Uuid::new_v4(),
            project_id,
            current_generation,
            "unsectioned-technical",
            "technical",
            Some(Uuid::nil()),
        )
        .await
        .unwrap();
        assert_ne!(authoritative, technical);
        let kinds: Vec<String> = sqlx::query_scalar(
            "SELECT job_kind FROM bid_match_jobs
             WHERE project_id = $1 AND generation = $2 ORDER BY job_kind",
        )
        .bind(project_id)
        .bind(current_generation)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(kinds, vec!["commercial", "technical"]);
    }

    #[tokio::test]
    async fn partial_clause_patches_preserve_omitted_match_inputs() {
        let _g = db_lock().await;
        let Some(pool) = setup().await else { return };
        let project_id = Uuid::new_v4();
        let clause_id = Uuid::new_v4();
        sqlx::query("INSERT INTO bid_projects (id, title) VALUES ($1, 'patch race')")
            .bind(project_id)
            .execute(&pool)
            .await
            .unwrap();
        crate::bid::insert_clause(
            &pool,
            crate::bid::NewClause {
                id: clause_id,
                project_id,
                extract_run_id: None,
                section_id: None,
                source_document_id: None,
                source_span: None,
                family_conflict: false,
                extraction_meta: None,
                raw_text: "X",
                text: "X",
                family: "technical",
                must: false,
                status: "confirmed",
            },
        )
        .await
        .unwrap();
        let a = crate::bid::update_clause(
            &pool,
            crate::bid::ClausePatch {
                id: clause_id,
                project_id,
                expected_status: "confirmed",
                text: Some("Y"),
                family: None,
                must: Some(true),
                status: None,
                deviate: None,
                deviate_note: None,
                assessment: None,
            },
        );
        let b = crate::bid::update_clause(
            &pool,
            crate::bid::ClausePatch {
                id: clause_id,
                project_id,
                expected_status: "confirmed",
                text: None,
                family: None,
                must: None,
                status: None,
                deviate: None,
                deviate_note: None,
                assessment: Some("meet"),
            },
        );
        let (a, b) = tokio::join!(a, b);
        assert!(a.unwrap().is_some());
        assert!(b.unwrap().is_some());
        let row = sqlx::query("SELECT text, must, assessment FROM bid_clauses WHERE id = $1")
            .bind(clause_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(row.get::<String, _>("text"), "Y");
        assert!(row.get::<bool, _>("must"));
        assert_eq!(row.get::<String, _>("assessment"), "meet");
        let generation: i64 =
            sqlx::query_scalar("SELECT match_generation FROM bid_projects WHERE id = $1")
                .bind(project_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            generation, 2,
            "insert confirmation plus one match-input patch"
        );
    }

    #[tokio::test]
    async fn opposing_section_merges_cannot_create_a_cycle() {
        let _g = db_lock().await;
        let Some(pool) = setup().await else { return };
        let project_id = Uuid::new_v4();
        let document_id = Uuid::new_v4();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        sqlx::query("INSERT INTO bid_projects (id, title) VALUES ($1, 'merge race')")
            .bind(project_id)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO bid_documents (id, project_id, file_name, file_hash, object_key) VALUES ($1,$2,'x.md','h','objects/h')")
            .bind(document_id).bind(project_id).execute(&pool).await.unwrap();
        for id in [a, b] {
            sqlx::query("INSERT INTO bid_sections (id, project_id, document_id, section_key) VALUES ($1,$2,$3,$4)")
                .bind(id).bind(project_id).bind(document_id).bind(id.to_string())
                .execute(&pool).await.unwrap();
        }
        let (ab, ba) = tokio::join!(
            crate::bid::set_section_merge(&pool, project_id, a, Some(b)),
            crate::bid::set_section_merge(&pool, project_id, b, Some(a)),
        );
        assert_eq!(
            [ab.is_ok(), ba.is_ok()]
                .into_iter()
                .filter(|ok| *ok)
                .count(),
            1
        );
        let rows = sqlx::query("SELECT id, merge_into FROM bid_sections WHERE project_id = $1")
            .bind(project_id)
            .fetch_all(&pool)
            .await
            .unwrap();
        let graph: std::collections::HashMap<Uuid, Option<Uuid>> = rows
            .iter()
            .map(|row| (row.get("id"), row.get("merge_into")))
            .collect();
        assert!(!(graph.get(&a) == Some(&Some(b)) && graph.get(&b) == Some(&Some(a))));
    }

    #[tokio::test]
    async fn section_retry_terminal_finish_releases_both_leases_atomically() {
        let _g = db_lock().await;
        let Some(pool) = setup().await else { return };
        let project_id = Uuid::new_v4();
        let document_id = Uuid::new_v4();
        let section_id = Uuid::new_v4();
        sqlx::query("INSERT INTO bid_projects (id, title) VALUES ($1, 'retry finish')")
            .bind(project_id)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO bid_documents (id, project_id, file_name, file_hash, object_key) VALUES ($1,$2,'x.md','h','objects/h')")
            .bind(document_id).bind(project_id).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO bid_sections (id, project_id, document_id, section_key) VALUES ($1,$2,$3,'s')")
            .bind(section_id).bind(project_id).bind(document_id).execute(&pool).await.unwrap();
        let job_id = crate::bid::enqueue_section_retry(&pool, project_id, section_id)
            .await
            .unwrap();
        let token = crate::bid::claim_section_retry_job(&pool, job_id, project_id, section_id)
            .await
            .unwrap()
            .unwrap();
        assert!(
            crate::bid::finish_section_retry_job(
                &pool, job_id, project_id, section_id, token, "done", ""
            )
            .await
            .unwrap()
        );
        assert!(
            !crate::bid::finish_section_retry_job(
                &pool, job_id, project_id, section_id, token, "done", ""
            )
            .await
            .unwrap()
        );
        let state: (String, Option<Uuid>) = sqlx::query_as(
            "SELECT j.status, p.extract_lock_token FROM bid_section_retry_jobs j
             JOIN bid_projects p ON p.id = j.project_id WHERE j.id = $1",
        )
        .bind(job_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(state, ("done".into(), None));
    }

    #[tokio::test]
    async fn bid_conversion_retry_fences_stale_owner_and_auto_handoff_is_idempotent() {
        let _g = db_lock().await;
        let Some(pool) = setup().await else {
            return;
        };
        let project_id = Uuid::new_v4();
        let document_id = Uuid::new_v4();
        sqlx::query("INSERT INTO bid_projects (id, title) VALUES ($1, 'convert fence')")
            .bind(project_id)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO bid_documents
                (id, project_id, file_name, file_hash, object_key)
             VALUES ($1, $2, 'tender.md', 'hash', 'objects/hash')",
        )
        .bind(document_id)
        .bind(project_id)
        .execute(&pool)
        .await
        .unwrap();
        let (old_token, _, _, old_generation) =
            crate::bid::claim_document_conversion(&pool, document_id)
                .await
                .unwrap()
                .unwrap();
        assert_eq!(old_generation, 0);
        assert!(
            crate::bid::reset_document_for_retry(&pool, project_id, document_id)
                .await
                .unwrap()
        );
        assert!(
            !crate::bid::finish_document_conversion(
                &pool,
                document_id,
                old_token,
                "completed",
                Some("objects/stale"),
                "",
            )
            .await
            .unwrap(),
            "stale conversion must not publish"
        );
        let (token, _, _, generation) = crate::bid::claim_document_conversion(&pool, document_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(generation, 1);
        assert!(
            crate::bid::set_document_multimodal_status(&pool, document_id, token, "skipped", "",)
                .await
                .unwrap()
        );
        assert!(
            crate::bid::finish_document_conversion(
                &pool,
                document_id,
                token,
                "completed",
                Some("objects/current"),
                "",
            )
            .await
            .unwrap()
        );
        let first = crate::bid::ensure_auto_extract_run(&pool, document_id)
            .await
            .unwrap()
            .unwrap();
        let second = crate::bid::ensure_auto_extract_run(&pool, document_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first, second);
    }

    #[tokio::test]
    async fn tables_flat_conversion_skips_auto_extract() {
        let _g = db_lock().await;
        let Some(pool) = setup().await else {
            return;
        };
        let project_id = Uuid::new_v4();
        let document_id = Uuid::new_v4();
        sqlx::query("INSERT INTO bid_projects (id, title) VALUES ($1, 'tables flat')")
            .bind(project_id)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO bid_documents
                (id, project_id, file_name, file_hash, object_key)
             VALUES ($1, $2, 'spec.docx', 'hash', 'objects/hash')",
        )
        .bind(document_id)
        .bind(project_id)
        .execute(&pool)
        .await
        .unwrap();
        let (token, _, _, _) = crate::bid::claim_document_conversion(&pool, document_id)
            .await
            .unwrap()
            .unwrap();
        assert!(
            crate::bid::set_document_multimodal_status(&pool, document_id, token, "skipped", "",)
                .await
                .unwrap()
        );
        assert!(
            crate::bid::finish_document_conversion(
                &pool,
                document_id,
                token,
                "completed",
                Some("objects/md"),
                "conversion_quality=tables_flat",
            )
            .await
            .unwrap()
        );
        assert!(
            crate::bid::ensure_auto_extract_run(&pool, document_id)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn transient_conversion_failure_can_be_claimed_again_then_complete() {
        let _g = db_lock().await;
        let Some(pool) = setup().await else {
            return;
        };
        let project_id = Uuid::new_v4();
        let document_id = Uuid::new_v4();
        sqlx::query("INSERT INTO bid_projects (id, title) VALUES ($1, 'convert retry')")
            .bind(project_id)
            .execute(&pool)
            .await
            .unwrap();
        crate::bid::insert_document(
            &pool,
            document_id,
            project_id,
            "retry.md",
            "retry-hash",
            1,
            "objects/retry-hash",
        )
        .await
        .unwrap();
        let (first, _, _, generation) = crate::bid::claim_document_conversion(&pool, document_id)
            .await
            .unwrap()
            .unwrap();
        assert!(
            crate::bid::finish_document_conversion(
                &pool,
                document_id,
                first,
                "pending",
                None,
                "transient provider error",
            )
            .await
            .unwrap()
        );
        let (second, _, _, retry_generation) =
            crate::bid::claim_document_conversion(&pool, document_id)
                .await
                .unwrap()
                .unwrap();
        assert_ne!(first, second);
        assert_eq!(generation, retry_generation);
        assert!(
            crate::bid::set_document_multimodal_status(&pool, document_id, second, "skipped", "",)
                .await
                .unwrap()
        );
        assert!(
            crate::bid::finish_document_conversion(
                &pool,
                document_id,
                second,
                "completed",
                Some("objects/markdown"),
                "",
            )
            .await
            .unwrap()
        );
    }

    #[tokio::test]
    async fn ending_project_fences_active_conversion_and_match() {
        let _g = db_lock().await;
        let Some(pool) = setup().await else {
            return;
        };
        let project_id = Uuid::new_v4();
        let document_id = Uuid::new_v4();
        sqlx::query("INSERT INTO bid_projects (id, title) VALUES ($1, 'end fence')")
            .bind(project_id)
            .execute(&pool)
            .await
            .unwrap();
        crate::bid::insert_document(
            &pool,
            document_id,
            project_id,
            "active.md",
            "active-hash",
            1,
            "objects/active-hash",
        )
        .await
        .unwrap();
        let (conversion_token, _, _, _) = crate::bid::claim_document_conversion(&pool, document_id)
            .await
            .unwrap()
            .unwrap();
        let match_id = crate::bid::insert_match_job(
            &pool,
            Uuid::new_v4(),
            project_id,
            0,
            "active-match",
            "commercial",
            None,
        )
        .await
        .unwrap();
        let match_token = crate::bid::claim_match_job(&pool, match_id, project_id)
            .await
            .unwrap()
            .unwrap();

        assert!(crate::bid::end_project(&pool, project_id).await.unwrap());
        assert!(
            !crate::bid::heartbeat_document_conversion(&pool, document_id, conversion_token)
                .await
                .unwrap()
        );
        assert!(
            !crate::bid::finish_document_conversion(
                &pool,
                document_id,
                conversion_token,
                "completed",
                Some("objects/late"),
                "",
            )
            .await
            .unwrap()
        );
        assert!(
            !crate::bid::heartbeat_match_job(&pool, match_id, match_token)
                .await
                .unwrap()
        );
        let status: String = sqlx::query_scalar("SELECT status FROM bid_match_jobs WHERE id = $1")
            .bind(match_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(status, "failed");
        assert!(
            !crate::bid::any_match_running(&pool, project_id)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn document_delete_cascades_pending_run_and_rejects_active_lease() {
        let _g = db_lock().await;
        let Some(pool) = setup().await else {
            return;
        };
        let project_id = Uuid::new_v4();
        sqlx::query("INSERT INTO bid_projects (id, title) VALUES ($1, 'delete lease')")
            .bind(project_id)
            .execute(&pool)
            .await
            .unwrap();

        let pending_document = Uuid::new_v4();
        crate::bid::insert_document(
            &pool,
            pending_document,
            project_id,
            "pending.md",
            "pending-delete-hash",
            1,
            "objects/pending-delete-hash",
        )
        .await
        .unwrap();
        let pending_run = Uuid::new_v4();
        crate::bid::insert_extract_run(
            &pool,
            pending_run,
            project_id,
            Some(pending_document),
            "auto",
        )
        .await
        .unwrap();
        assert!(
            crate::bid::delete_document_for_project(&pool, project_id, pending_document)
                .await
                .unwrap()
        );
        let pending_exists: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM bid_extract_runs WHERE id = $1)")
                .bind(pending_run)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(!pending_exists, "document-scoped pending run must cascade");

        let active_document = Uuid::new_v4();
        crate::bid::insert_document(
            &pool,
            active_document,
            project_id,
            "active.md",
            "active-delete-hash",
            1,
            "objects/active-delete-hash",
        )
        .await
        .unwrap();
        let active_run = Uuid::new_v4();
        crate::bid::insert_extract_run(
            &pool,
            active_run,
            project_id,
            Some(active_document),
            "auto",
        )
        .await
        .unwrap();
        let token =
            crate::bid::claim_extract_run(&pool, active_run, project_id, Some(active_document))
                .await
                .unwrap()
                .unwrap();
        assert!(
            crate::bid::delete_document_for_project(&pool, project_id, active_document)
                .await
                .is_err(),
            "active extraction lease must fence document deletion"
        );
        let diagnostics = serde_json::json!({});
        crate::bid::finish_extract_run(
            &pool,
            crate::bid::FinishExtractRun {
                id: active_run,
                claim_token: token,
                status: "failed",
                section_total: 0,
                section_done: 0,
                error_message: "test cleanup",
                extractor_mode: "heuristic",
                model_id: "",
                policy_version: "cn-tender-v2",
                prompt_version: "clause-extractor-v2",
                diagnostics: &diagnostics,
            },
        )
        .await
        .unwrap();
        assert!(crate::bid::end_project(&pool, project_id).await.unwrap());
        assert!(
            crate::bid::insert_document(
                &pool,
                Uuid::new_v4(),
                project_id,
                "late.md",
                "late-hash",
                1,
                "objects/late-hash",
            )
            .await
            .is_err()
        );
        assert!(
            crate::bid::insert_extract_run(&pool, Uuid::new_v4(), project_id, None, "manual",)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn failed_document_persistence_rolls_back_draft_replacement() {
        let _g = db_lock().await;
        let Some(pool) = setup().await else {
            return;
        };
        let project_id = Uuid::new_v4();
        let document_id = Uuid::new_v4();
        let section_id = Uuid::new_v4();
        let old_clause_id = Uuid::new_v4();
        let run_id = Uuid::new_v4();
        sqlx::query("INSERT INTO bid_projects (id, title) VALUES ($1, 'atomic test')")
            .bind(project_id)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO bid_documents
                (id, project_id, file_name, file_hash, object_key, parse_status, multimodal_status)
             VALUES ($1, $2, 't.md', 'hash', 'objects/hash', 'completed', 'skipped')",
        )
        .bind(document_id)
        .bind(project_id)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO bid_sections
                (id, project_id, document_id, section_key, body, extract_status)
             VALUES ($1, $2, $3, 'stable-key', 'old body', 'done')",
        )
        .bind(section_id)
        .bind(project_id)
        .bind(document_id)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO bid_clauses
                (id, project_id, section_id, source_document_id, text, family, status)
             VALUES ($1, $2, $3, $4, 'old draft', 'technical', 'draft')",
        )
        .bind(old_clause_id)
        .bind(project_id)
        .bind(section_id)
        .bind(document_id)
        .execute(&pool)
        .await
        .unwrap();
        crate::bid::insert_extract_run(&pool, run_id, project_id, Some(document_id), "manual")
            .await
            .unwrap();
        let token = crate::bid::claim_extract_run(&pool, run_id, project_id, Some(document_id))
            .await
            .unwrap()
            .unwrap();
        let section = crate::bid::ExtractionSectionRow {
            id: Uuid::new_v4(),
            section_key: "stable-key",
            heading_path: "new heading",
            hint_family: "technical",
            body: "new body",
            extract_status: "done",
            error_message: "",
        };
        let source_span = serde_json::json!({"span_id": "span-1", "quote": "new"});
        let extraction_meta = serde_json::json!({});
        let clause = crate::bid::ExtractionClauseRow {
            id: Uuid::new_v4(),
            section_key: "stable-key",
            source_span: &source_span,
            family_conflict: false,
            extraction_meta: &extraction_meta,
            raw_text: "new",
            text: "new",
            family: "invalid-family",
            must: true,
        };
        let result = crate::bid::persist_extraction_report(
            &pool,
            crate::bid::PersistExtractionReport {
                run_id,
                claim_token: token,
                project_id,
                document_id,
                sections: &[section],
                clauses: &[clause],
                replace_document: true,
            },
        )
        .await;
        assert!(result.is_err());
        let status: String = sqlx::query_scalar("SELECT status FROM bid_clauses WHERE id = $1")
            .bind(old_clause_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        let body: String = sqlx::query_scalar("SELECT body FROM bid_sections WHERE id = $1")
            .bind(section_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(status, "draft");
        assert_eq!(body, "old body");
    }

    #[tokio::test]
    async fn workspace_seeds_default_library_and_blocks_delete() {
        let _g = db_lock().await;
        let Some(pool) = setup().await else {
            return;
        };
        let owner = Uuid::new_v4();
        sqlx::query("INSERT INTO users (id, email) VALUES ($1, $2)")
            .bind(owner)
            .bind(format!("{owner}@ex.com"))
            .execute(&pool)
            .await
            .unwrap();
        let seeded = create_workspace_with_library(&pool, owner, "Acme", "acme")
            .await
            .unwrap();
        let rec = sqlx::query("SELECT name, slug, kind FROM products WHERE id = $1")
            .bind(seeded.library_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(rec.get::<String, _>("slug"), "library");
        assert_eq!(rec.get::<String, _>("name"), "公司资料");
        assert_eq!(rec.get::<String, _>("kind"), "library");
        let cfg: serde_json::Value =
            sqlx::query_scalar("SELECT retrieval_config FROM workspaces WHERE id = $1")
                .bind(seeded.workspace_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(cfg["vector_threshold"], 0.15);
        assert_eq!(cfg["keyword_threshold"], 0.3);
        assert_eq!(cfg["embedding_top_k"], 50);
        match delete_product(&pool, seeded.library_id).await {
            Err(PersistError::DefaultLibrary) => {}
            other => panic!("expected DefaultLibrary, got {other:?}"),
        }
        let still = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM products WHERE id = $1")
            .bind(seeded.library_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(still, 1);
        let company = ensure_company_workspace(&pool).await.unwrap();
        let kind: String = sqlx::query_scalar("SELECT kind FROM workspaces WHERE id = $1")
            .bind(company)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(kind, "company");
        let again = ensure_company_workspace(&pool).await.unwrap();
        assert_eq!(company, again);
    }

    #[tokio::test]
    async fn housekeep_fails_stale_processing_keeps_fresh_span() {
        let _g = db_lock().await;
        let Some(pool) = setup().await else {
            return;
        };
        let owner = Uuid::new_v4();
        insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
            .await
            .unwrap();
        let seeded = create_workspace_with_library(&pool, owner, "Hk", "hk")
            .await
            .unwrap();
        let stale = Uuid::new_v4();
        let live = Uuid::new_v4();
        for id in [stale, live] {
            insert_document(
                &pool,
                NewDocument {
                    id,
                    product_version_id: seeded.library_version_id,
                    title: "t",
                    file_name: &format!("{id}.txt"),
                    file_size: 1,
                    file_hash: &id.to_string(),
                    object_key: "k",
                },
            )
            .await
            .unwrap();
            sqlx::query("UPDATE documents SET parse_status = 'processing', updated_at = now() - interval '3 hours' WHERE id = $1")
                .bind(id)
                .execute(&pool)
                .await
                .unwrap();
        }
        upsert_span(&pool, live, 1, "docreader", "ok", None)
            .await
            .unwrap();
        let n = housekeep_documents(&pool, 2 * 3600 + 10 * 60)
            .await
            .unwrap();
        assert!(n >= 1, "expected at least the stale row");
        let stale_st: String =
            sqlx::query_scalar("SELECT parse_status FROM documents WHERE id = $1")
                .bind(stale)
                .fetch_one(&pool)
                .await
                .unwrap();
        let live_st: String =
            sqlx::query_scalar("SELECT parse_status FROM documents WHERE id = $1")
                .bind(live)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(stale_st, "failed");
        assert_eq!(live_st, "processing");
    }

    #[tokio::test]
    async fn try_set_processing_skips_cancelled() {
        let _g = db_lock().await;
        let Some(pool) = setup().await else {
            return;
        };
        let owner = Uuid::new_v4();
        insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
            .await
            .unwrap();
        let seeded = create_workspace_with_library(&pool, owner, "Ab", "ab")
            .await
            .unwrap();
        let did = Uuid::new_v4();
        insert_document(
            &pool,
            NewDocument {
                id: did,
                product_version_id: seeded.library_version_id,
                title: "t",
                file_name: "t.txt",
                file_size: 1,
                file_hash: "h",
                object_key: "k",
            },
        )
        .await
        .unwrap();
        assert!(try_set_processing(&pool, did).await.unwrap());
        let st: String = sqlx::query_scalar("SELECT parse_status FROM documents WHERE id = $1")
            .bind(did)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(st, "processing");
        set_parse_status(&pool, did, "cancelled", "").await.unwrap();
        assert!(!try_set_processing(&pool, did).await.unwrap());
        let st: String = sqlx::query_scalar("SELECT parse_status FROM documents WHERE id = $1")
            .bind(did)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(st, "cancelled");
    }

    #[tokio::test]
    async fn release_object_ref_drops_at_zero() {
        let _g = db_lock().await;
        let Some(pool) = setup().await else {
            return;
        };
        bump_object_ref(&pool, "abc", 3).await.unwrap();
        bump_object_ref(&pool, "abc", 3).await.unwrap();
        assert_eq!(release_object_ref(&pool, "abc").await.unwrap(), 1);
        let n: i32 = sqlx::query_scalar("SELECT refcount FROM content_objects WHERE hash = 'abc'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 1);
        assert_eq!(release_object_ref(&pool, "abc").await.unwrap(), 0);
        let left: Option<i32> =
            sqlx::query_scalar("SELECT refcount FROM content_objects WHERE hash = 'abc'")
                .fetch_optional(&pool)
                .await
                .unwrap();
        assert!(left.is_none());
    }

    #[tokio::test]
    async fn api_key_roundtrip_and_chunk_embedding_persist() {
        let _g = db_lock().await;
        let Some(pool) = setup().await else {
            return;
        };
        let owner = Uuid::new_v4();
        insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
            .await
            .unwrap();
        let seeded = create_workspace_with_library(&pool, owner, "Ak", "ak")
            .await
            .unwrap();
        let kid = Uuid::new_v4();
        insert_api_key(
            &pool,
            NewApiKey {
                id: kid,
                name: "ci",
                key_hash: "hash-abc",
                prefix: "kb_abc",
                scope_type: "workspace",
                scope_id: seeded.workspace_id,
                scopes: &["search".into()],
            },
        )
        .await
        .unwrap();
        let found = find_api_key_by_hash(&pool, "hash-abc").await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().scope_id, seeded.workspace_id);

        let did = Uuid::new_v4();
        insert_document(
            &pool,
            NewDocument {
                id: did,
                product_version_id: seeded.library_version_id,
                title: "t",
                file_name: "t.txt",
                file_size: 4,
                file_hash: "hh",
                object_key: "objects/hh",
            },
        )
        .await
        .unwrap();
        let dup = find_duplicate_document(&pool, seeded.library_version_id, "t.txt", 4, "hh")
            .await
            .unwrap();
        assert_eq!(dup, Some(did));
        let cid = Uuid::new_v4();
        let ch = domain::Chunk {
            id: cid,
            document_id: did,
            product_version_id: seeded.library_version_id,
            chunk_type: "text".into(),
            content: "40Gbps throughput".into(),
            context_header: String::new(),
            start_at: 0,
            end_at: 17,
            parent_chunk_id: None,
            generated_questions: vec![],
        };
        let emb = domain::ChunkEmbedding {
            chunk_id: cid,
            product_version_id: seeded.library_version_id,
            document_id: did,
            content: "40Gbps throughput".into(),
            vector: vec![0.1; models::EMBEDDING_DIM],
            tsv: "40gbps throughput".into(),
        };
        replace_document_chunks(&pool, did, &[ch], &[emb])
            .await
            .unwrap();
        let img = domain::Chunk {
            id: Uuid::new_v4(),
            document_id: did,
            product_version_id: seeded.library_version_id,
            chunk_type: "image_ocr".into(),
            content: "ocr".into(),
            context_header: "images/p1.jpg".into(),
            start_at: 0,
            end_at: 3,
            parent_chunk_id: Some(cid),
            generated_questions: vec![],
        };
        insert_document_chunks(&pool, &[img], &[]).await.unwrap();
        delete_image_chunks(&pool, did, "images/p1.jpg")
            .await
            .unwrap();
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM chunks WHERE document_id = $1")
            .bind(did)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 1);
        let dim: i32 = sqlx::query_scalar(
            "SELECT vector_dims(embedding)::int FROM chunk_embeddings WHERE chunk_id = $1",
        )
        .bind(cid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(dim, models::EMBEDDING_DIM as i32);

        sqlx::query("UPDATE documents SET enable_status = 'enabled' WHERE id = $1")
            .bind(did)
            .execute(&pool)
            .await
            .unwrap();
        let qv = vector_literal(&vec![0.1; models::EMBEDDING_DIM]);
        let hits = hybrid_search_pg(
            &pool,
            seeded.library_version_id,
            "throughput",
            &qv,
            &[],
            true,
            8,
        )
        .await
        .unwrap();
        assert!(
            hits.iter().any(|h| h.chunk_id == cid),
            "pg hybrid search missed chunk"
        );
        assert!(hits[0].vec_score > 0.9, "cosine {}", hits[0].vec_score);
        sqlx::query(
            "UPDATE product_versions SET indexing_strategy = '{\"vector\":false,\"keyword\":true}'::jsonb
             WHERE id = $1",
        )
        .bind(seeded.library_version_id)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE chunk_embeddings SET tsv = to_tsvector('simple', content) WHERE chunk_id = $1",
        )
        .bind(cid)
        .execute(&pool)
        .await
        .unwrap();
        let kw_only = hybrid_search_pg(
            &pool,
            seeded.library_version_id,
            "throughput",
            &qv,
            &[],
            true,
            8,
        )
        .await
        .unwrap();
        assert!(
            kw_only.iter().any(|h| h.chunk_id == cid),
            "keyword-only channel must still recall the chunk"
        );
        assert!(
            kw_only.iter().all(|h| h.vec_score == 0.0),
            "vector channel must stay off"
        );
    }

    #[tokio::test]
    async fn hydrate_workspace_fills_empty_store() {
        let _g = db_lock().await;
        let Some(pool) = setup().await else {
            return;
        };
        let owner = Uuid::new_v4();
        insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
            .await
            .unwrap();
        let seeded = create_workspace_with_library(&pool, owner, "Hy", "hy")
            .await
            .unwrap();
        let mut store = domain::Store::default();
        assert!(
            hydrate_workspace(&pool, &mut store, seeded.workspace_id)
                .await
                .unwrap()
        );
        assert!(store.workspaces.contains_key(&seeded.workspace_id));
        assert_eq!(
            store.members.get(&(seeded.workspace_id, owner)).copied(),
            Some(domain::Role::Owner)
        );
        assert_eq!(store.products[&seeded.library_id].slug, "library");
        assert!(store.versions.contains_key(&seeded.library_version_id));
        let ids = workspaces_for_user(&pool, owner).await.unwrap();
        assert_eq!(ids, vec![seeded.workspace_id]);
    }

    #[tokio::test]
    async fn set_finalizing_only_from_processing() {
        let _g = db_lock().await;
        let Some(pool) = setup().await else {
            return;
        };
        let owner = Uuid::new_v4();
        insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
            .await
            .unwrap();
        let seeded = create_workspace_with_library(&pool, owner, "Fz", "fz")
            .await
            .unwrap();
        let did = Uuid::new_v4();
        insert_document(
            &pool,
            NewDocument {
                id: did,
                product_version_id: seeded.library_version_id,
                title: "d",
                file_name: "d.txt",
                file_size: 1,
                file_hash: "fz1",
                object_key: "objects/fz1",
            },
        )
        .await
        .unwrap();
        assert!(!set_finalizing(&pool, did, 3).await.unwrap());
        sqlx::query("UPDATE documents SET parse_status = 'processing' WHERE id = $1")
            .bind(did)
            .execute(&pool)
            .await
            .unwrap();
        assert!(set_finalizing(&pool, did, 3).await.unwrap());
        let (st, n): (String, i32) = sqlx::query_as(
            "SELECT parse_status, pending_subtasks_count FROM documents WHERE id = $1",
        )
        .bind(did)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(st, "finalizing");
        assert_eq!(n, 3);
        assert!(!set_finalizing(&pool, did, 9).await.unwrap());
        let n2: i32 =
            sqlx::query_scalar("SELECT pending_subtasks_count FROM documents WHERE id = $1")
                .bind(did)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(n2, 3);
    }

    #[tokio::test]
    async fn persist_graph_unions_chunk_ids() {
        let _g = db_lock().await;
        let Some(pool) = setup().await else {
            return;
        };
        let owner = Uuid::new_v4();
        insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
            .await
            .unwrap();
        let seeded = create_workspace_with_library(&pool, owner, "Fg", "fg")
            .await
            .unwrap();
        let did = Uuid::new_v4();
        insert_document(
            &pool,
            NewDocument {
                id: did,
                product_version_id: seeded.library_version_id,
                title: "d",
                file_name: "d.txt",
                file_size: 1,
                file_hash: "fg1",
                object_key: "objects/fg1",
            },
        )
        .await
        .unwrap();
        let c1 = Uuid::new_v4();
        let c2 = Uuid::new_v4();
        let mut a = domain::Store::default();
        a.upsert_node(seeded.library_version_id, did, "Alpha", c1);
        persist_graph_for_document(&pool, &a, did).await.unwrap();
        let mut b = domain::Store::default();
        b.upsert_node(seeded.library_version_id, did, "Alpha", c2);
        persist_graph_for_document(&pool, &b, did).await.unwrap();
        let ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT chunk_ids FROM graph_nodes
             WHERE document_id = $1 AND name = 'Alpha'",
        )
        .bind(did)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(ids.len(), 2, "{ids:?}");
        assert!(ids.contains(&c1));
        assert!(ids.contains(&c2));
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM graph_nodes WHERE document_id = $1")
            .bind(did)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 1);
    }

    #[tokio::test]
    async fn hydrate_reads_question_generation_config() {
        let _g = db_lock().await;
        let Some(pool) = setup().await else {
            return;
        };
        let owner = Uuid::new_v4();
        insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
            .await
            .unwrap();
        let seeded = create_workspace_with_library(&pool, owner, "Hq", "hq")
            .await
            .unwrap();
        update_version_config(
            &pool,
            seeded.library_version_id,
            VersionConfig {
                status: None,
                chunking: Some(serde_json::json!({
                    "chunk_size": 512,
                    "chunk_overlap": 80,
                    "strategy": "auto",
                    "enable_parent_child": true,
                    "parent_chunk_size": 2000,
                    "child_chunk_size": 200,
                    "separators": ["\n\n", "。"],
                    "token_limit": 256,
                    "languages": ["zh"],
                    "table_metadata_instructions": "units in Mbps"
                })),
                indexing: None,
                image_processing: None,
                embedding_model_id: None,
                summary_model_id: None,
                asr_model_id: None,
                asr_config: None,
                extract_config: None,
                wiki_config: None,
                question_generation_config: Some(serde_json::json!({
                    "enabled": true,
                    "question_count": 7,
                    "custom_instructions": "for auditors"
                })),
            },
        )
        .await
        .unwrap();
        let mut store = domain::Store::default();
        assert!(
            hydrate_workspace(&pool, &mut store, seeded.workspace_id)
                .await
                .unwrap()
        );
        let v = &store.versions[&seeded.library_version_id];
        assert_eq!(v.question_count(), 7);
        assert_eq!(v.question_custom_instructions, "for auditors");
        assert!(v.question_enabled);
        assert_eq!(v.parent_chunk_size(), 2000);
        assert_eq!(v.child_chunk_size(), 200);
        assert_eq!(v.chunk_separators, ["\n\n", "。"]);
        assert_eq!(v.chunk_token_limit, 256);
        assert_eq!(v.chunk_languages, ["zh"]);
        assert!(v.enable_parent_child);
        assert_eq!(v.table_metadata_instructions, "units in Mbps");
    }

    #[tokio::test]
    async fn hydrate_workspace_loads_tags_graph_wiki_chunks() {
        let _g = db_lock().await;
        let Some(pool) = setup().await else {
            return;
        };
        let owner = Uuid::new_v4();
        insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
            .await
            .unwrap();
        let seeded = create_workspace_with_library(&pool, owner, "Hx", "hx")
            .await
            .unwrap();
        let tag_id = Uuid::new_v4();
        sqlx::query("INSERT INTO tags (id, workspace_id, name, slug) VALUES ($1,$2,'iso','iso')")
            .bind(tag_id)
            .bind(seeded.workspace_id)
            .execute(&pool)
            .await
            .unwrap();
        let did = Uuid::new_v4();
        insert_document(
            &pool,
            NewDocument {
                id: did,
                product_version_id: seeded.library_version_id,
                title: "cert",
                file_name: "cert.txt",
                file_size: 4,
                file_hash: "hyd1",
                object_key: "objects/hyd1",
            },
        )
        .await
        .unwrap();
        sqlx::query("INSERT INTO document_tags (document_id, tag_id) VALUES ($1,$2)")
            .bind(did)
            .bind(tag_id)
            .execute(&pool)
            .await
            .unwrap();
        let cid = Uuid::new_v4();
        replace_document_chunks(
            &pool,
            did,
            &[domain::Chunk {
                id: cid,
                document_id: did,
                product_version_id: seeded.library_version_id,
                chunk_type: "text".into(),
                content: "iso certified".into(),
                context_header: String::new(),
                start_at: 0,
                end_at: 13,
                parent_chunk_id: None,
                generated_questions: vec![],
            }],
            &[domain::ChunkEmbedding {
                chunk_id: cid,
                product_version_id: seeded.library_version_id,
                document_id: did,
                content: "iso certified".into(),
                vector: vec![0.3; models::EMBEDDING_DIM],
                tsv: String::new(),
            }],
        )
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO graph_nodes (product_version_id, document_id, name, chunk_ids)
             VALUES ($1,$2,'Widget',$3)",
        )
        .bind(seeded.library_version_id)
        .bind(did)
        .bind(&[cid][..])
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO graph_relations
                (product_version_id, document_id, node1, node2, rel_type)
             VALUES ($1,$2,'Widget','Spec','mentions')",
        )
        .bind(seeded.library_version_id)
        .bind(did)
        .execute(&pool)
        .await
        .unwrap();
        let page_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO wiki_pages (id, product_version_id, slug, title, content, status)
             VALUES ($1,$2,'overview','Overview','wiki body','published')",
        )
        .bind(page_id)
        .bind(seeded.library_version_id)
        .execute(&pool)
        .await
        .unwrap();
        let mut store = domain::Store::default();
        assert!(
            hydrate_workspace(&pool, &mut store, seeded.workspace_id)
                .await
                .unwrap()
        );
        assert!(store.tags.contains_key(&tag_id));
        assert!(store.document_tags.contains(&(did, tag_id)));
        assert!(store.chunks.contains_key(&cid));
        assert!(store.embeddings.contains_key(&cid));
        assert_eq!(store.chunks[&cid].content, "iso certified");
        assert!(
            store
                .graph
                .contains_key(&(seeded.library_version_id, did, "Widget".into()))
        );
        assert!(store.relations.contains_key(&(
            seeded.library_version_id,
            did,
            "Widget".into(),
            "Spec".into(),
            "mentions".into()
        )));
        assert!(
            store
                .wiki
                .contains_key(&(seeded.library_version_id, "overview".into()))
        );
        assert_eq!(
            store.wiki[&(seeded.library_version_id, "overview".into())].content,
            "wiki body"
        );
        let gh = graph_hits_pg(&pool, seeded.library_version_id, "Widget", 10, &[])
            .await
            .unwrap();
        assert!(
            gh.iter()
                .any(|h| h.name == "Widget" && h.document_id == did),
            "{gh:?}"
        );
    }

    #[tokio::test]
    async fn persist_document_tags_and_dead_letters() {
        let _g = db_lock().await;
        let Some(pool) = setup().await else {
            return;
        };
        let owner = Uuid::new_v4();
        insert_user(&pool, owner, &format!("{owner}@ex.com"), None)
            .await
            .unwrap();
        let seeded = create_workspace_with_library(&pool, owner, "Tg", "tg")
            .await
            .unwrap();
        let tag_id = Uuid::new_v4();
        insert_tag(&pool, tag_id, seeded.workspace_id, "iso", "iso")
            .await
            .unwrap();
        let did = Uuid::new_v4();
        insert_document(
            &pool,
            NewDocument {
                id: did,
                product_version_id: seeded.library_version_id,
                title: "cert",
                file_name: "cert.txt",
                file_size: 4,
                file_hash: "tag1",
                object_key: "objects/tag1",
            },
        )
        .await
        .unwrap();
        insert_document_tags(&pool, did, &[tag_id]).await.unwrap();
        insert_dead_letter(&pool, domain::TYPE_DOCUMENT_PROCESS, did, "parse failed")
            .await
            .unwrap();
        let mut store = domain::Store::default();
        assert!(
            hydrate_workspace(&pool, &mut store, seeded.workspace_id)
                .await
                .unwrap()
        );
        assert!(store.tags.contains_key(&tag_id));
        assert!(store.document_tags.contains(&(did, tag_id)));
        replace_document_tags(&pool, did, &[]).await.unwrap();
        store = domain::Store::default();
        hydrate_workspace(&pool, &mut store, seeded.workspace_id)
            .await
            .unwrap();
        assert!(!store.document_tags.contains(&(did, tag_id)));
        let letters = list_dead_letters(&pool).await.unwrap();
        assert!(
            letters
                .iter()
                .any(|l| l.related_id == did && l.last_error.contains("parse failed")),
            "{letters:?}"
        );
        assert!(count_dead_letters(&pool).await.unwrap() >= 1);
    }
}
