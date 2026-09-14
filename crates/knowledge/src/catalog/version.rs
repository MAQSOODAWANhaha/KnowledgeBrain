//! SQL persistence split from persist.rs (behavior unchanged).
use crate::load_product;
use sqlx::{PgPool, Row};
use uuid::Uuid;

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

pub async fn insert_version(
    pool: &PgPool,
    id: Uuid,
    product_id: Uuid,
    label: &str,
    status: &str,
    cloned_from: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    let embedding_model_id = platform::embedding_model();
    sqlx::query(
        "INSERT INTO product_versions (
            id, product_id, label, status, cloned_from_version_id,
            embedding_model_id, summary_model_id
         ) VALUES ($1, $2, $3, $4, $5, $6, 'stub-chat')
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(id)
    .bind(product_id)
    .bind(label)
    .bind(status)
    .bind(cloned_from)
    .bind(embedding_model_id.trim())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn version_has_chunk_embeddings(
    pool: &PgPool,
    version_id: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM chunk_embeddings
            WHERE product_version_id = $1 AND embedding IS NOT NULL
         )",
    )
    .bind(version_id)
    .fetch_one(pool)
    .await
}

/// Stamp `embedding_model_id` to the live env model before writing vectors.
pub async fn freeze_version_embedding_model(
    pool: &PgPool,
    version_id: Uuid,
) -> Result<String, String> {
    if !crate::index::embedding_http_configured() {
        let stored: String = sqlx::query_scalar(
            "SELECT COALESCE(embedding_model_id, '') FROM product_versions WHERE id = $1",
        )
        .bind(version_id)
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;
        return Ok(stored);
    }
    let stored: String = sqlx::query_scalar(
        "SELECT COALESCE(embedding_model_id, '') FROM product_versions WHERE id = $1",
    )
    .bind(version_id)
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;
    let has_embeddings = version_has_chunk_embeddings(pool, version_id)
        .await
        .map_err(|e| e.to_string())?;
    let v3: Option<String> = sqlx::query_scalar(
        "SELECT revision.provider_model_identifier
         FROM product_version_embedding_bindings_v2 binding
         JOIN embedding_revisions_v2 revision
           ON revision.revision_sha256 = binding.embedding_revision_sha256
         WHERE binding.product_version_id = $1",
    )
    .bind(version_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;
    match crate::index::embedding_identity_plan(&stored, has_embeddings, v3.as_deref())? {
        crate::index::EmbeddingIdentity::Use(id) => Ok(id),
        crate::index::EmbeddingIdentity::Bind(id) => {
            let n = sqlx::query(
                "UPDATE product_versions
                 SET embedding_model_id = $2, updated_at = now()
                 WHERE id = $1
                   AND (COALESCE(embedding_model_id, '') = ''
                        OR embedding_model_id = 'stub-emb')",
            )
            .bind(version_id)
            .bind(&id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?
            .rows_affected();
            if n == 0 {
                let again: String = sqlx::query_scalar(
                    "SELECT COALESCE(embedding_model_id, '') FROM product_versions WHERE id = $1",
                )
                .bind(version_id)
                .fetch_one(pool)
                .await
                .map_err(|e| e.to_string())?;
                if again == id {
                    Ok(id)
                } else {
                    Err(format!(
                        "embedding model conflict: version frozen as {again}, environment is {id}"
                    ))
                }
            } else {
                Ok(id)
            }
        }
    }
}

pub(crate) fn parse_kind(s: &str) -> crate::ProductKind {
    if s == "library" {
        crate::ProductKind::Library
    } else {
        crate::ProductKind::Product
    }
}

pub(crate) fn parse_version_status(s: &str) -> crate::VersionStatus {
    match s {
        "cloning" => crate::VersionStatus::Cloning,
        "archived" => crate::VersionStatus::Archived,
        "failed" => crate::VersionStatus::Failed,
        _ => crate::VersionStatus::Active,
    }
}

pub(crate) fn product_version_from_row(
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

pub async fn version_exists(pool: &PgPool, version_id: Uuid) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM product_versions WHERE id = $1 AND deleted_at IS NULL)",
    )
    .bind(version_id)
    .fetch_one(pool)
    .await
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
