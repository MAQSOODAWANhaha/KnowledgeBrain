//! SQL persistence split from persist.rs (behavior unchanged).
use crate::{append_document_chunks, vector_literal};
use sqlx::{PgPool, Row};
use uuid::Uuid;

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

#[allow(clippy::too_many_arguments)]
pub async fn persist_wiki_changes_atomic(
    pool: &PgPool,
    version_id: Uuid,
    pages: &[crate::WikiPage],
    removed_page_ids: &[Uuid],
    folders: &[crate::WikiFolder],
    removed_folder_ids: &[Uuid],
    changed_slugs: &[String],
    chunks: &[crate::Chunk],
    embeddings: &[crate::ChunkEmbedding],
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    if !removed_page_ids.is_empty() {
        sqlx::query("DELETE FROM wiki_pages WHERE product_version_id=$1 AND id=ANY($2::uuid[])")
            .bind(version_id)
            .bind(removed_page_ids)
            .execute(&mut *tx)
            .await?;
    }
    for folder in folders {
        sqlx::query(
            "INSERT INTO wiki_folders
                (id,product_version_id,parent_id,name,path,depth,sort_order)
             VALUES($1,$2,$3,$4,$5,$6,$7)
             ON CONFLICT(id) DO UPDATE SET parent_id=EXCLUDED.parent_id,name=EXCLUDED.name,
               path=EXCLUDED.path,depth=EXCLUDED.depth,sort_order=EXCLUDED.sort_order,
               updated_at=clock_timestamp(),deleted_at=NULL",
        )
        .bind(folder.id)
        .bind(folder.product_version_id)
        .bind(folder.parent_id)
        .bind(&folder.name)
        .bind(&folder.path)
        .bind(folder.depth)
        .bind(folder.sort_order)
        .execute(&mut *tx)
        .await?;
    }
    for page in pages {
        let source_refs = page
            .source_refs
            .iter()
            .map(Uuid::to_string)
            .collect::<Vec<_>>();
        sqlx::query(
            "INSERT INTO wiki_pages
              (id,product_version_id,slug,title,content,page_type,status,summary,aliases,
               source_refs,chunk_refs,category_path,folder_id)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)
             ON CONFLICT(product_version_id,slug) DO UPDATE SET title=EXCLUDED.title,
               content=EXCLUDED.content,page_type=EXCLUDED.page_type,status=EXCLUDED.status,
               summary=EXCLUDED.summary,aliases=EXCLUDED.aliases,source_refs=EXCLUDED.source_refs,
               chunk_refs=EXCLUDED.chunk_refs,category_path=EXCLUDED.category_path,
               folder_id=EXCLUDED.folder_id,updated_at=clock_timestamp(),deleted_at=NULL",
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
        .bind(serde_json::json!(source_refs))
        .bind(serde_json::json!(
            page.chunk_refs
                .iter()
                .map(Uuid::to_string)
                .collect::<Vec<_>>()
        ))
        .bind(serde_json::json!(page.category_path))
        .bind(page.folder_id)
        .execute(&mut *tx)
        .await?;
    }
    if !removed_folder_ids.is_empty() {
        sqlx::query("DELETE FROM wiki_folders WHERE product_version_id=$1 AND id=ANY($2::uuid[])")
            .bind(version_id)
            .bind(removed_folder_ids)
            .execute(&mut *tx)
            .await?;
    }
    if !changed_slugs.is_empty() {
        sqlx::query(
            "DELETE FROM chunks WHERE product_version_id=$1 AND chunk_type='wiki_page'
               AND context_header=ANY($2::text[])",
        )
        .bind(version_id)
        .bind(changed_slugs)
        .execute(&mut *tx)
        .await?;
    }
    for chunk in chunks {
        sqlx::query(
            "INSERT INTO chunks(id,product_version_id,document_id,chunk_type,content,
               context_header,start_at,end_at,parent_chunk_id,generated_questions)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
        )
        .bind(chunk.id)
        .bind(chunk.product_version_id)
        .bind(chunk.document_id)
        .bind(&chunk.chunk_type)
        .bind(&chunk.content)
        .bind(&chunk.context_header)
        .bind(chunk.start_at)
        .bind(chunk.end_at)
        .bind(chunk.parent_chunk_id)
        .bind(serde_json::json!(chunk.generated_questions))
        .execute(&mut *tx)
        .await?;
    }
    for embedding in embeddings {
        let vector = vector_literal(&embedding.vector);
        sqlx::query(
            "INSERT INTO chunk_embeddings(chunk_id,product_version_id,document_id,embedding,tsv,content)
             VALUES($1,$2,$3,CAST($4 AS vector),to_tsvector('simple',$5),$5)",
        )
        .bind(embedding.chunk_id)
        .bind(embedding.product_version_id)
        .bind(embedding.document_id)
        .bind(vector)
        .bind(&embedding.content)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await
}

pub async fn upsert_wiki_page(
    pool: &PgPool,
    page: &crate::WikiPage,
    _source_document_id: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    let refs: Vec<String> = page.source_refs.iter().map(|id| id.to_string()).collect();
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
