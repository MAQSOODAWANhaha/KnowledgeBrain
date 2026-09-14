//! SQL persistence split from persist.rs (behavior unchanged).
use sqlx::{PgPool, Row};
use uuid::Uuid;

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
