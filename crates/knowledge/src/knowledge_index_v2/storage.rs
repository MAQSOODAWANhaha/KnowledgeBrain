//! Index publication and intent state transitions share the caller's transaction.
use super::{SemanticIndexIntentV2, SemanticIndexPreparationV2, VectorIndexErrorV2};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

pub async fn source_snapshot_v2(
    conn: &mut PgConnection,
    version: Uuid,
    revision: &str,
) -> Result<(String, i64), sqlx::Error> {
    sqlx::query_as(r#"SELECT encode(public.digest(convert_to(
           $2||E'\n'||COALESCE(string_agg(
             chunk.id::text||':'||encode(public.digest(convert_to(
               chunk.context_header||E'\n'||chunk.content,'UTF8'),'sha256'),'hex')||E'\n',
             '' ORDER BY chunk.id),''),'UTF8'),'sha256'),'hex'),
         count(*)
    FROM public.knowledge_chunks chunk
    JOIN public.knowledge_documents document ON document.id=chunk.document_id
     AND document.product_version_id=chunk.product_version_id
   WHERE chunk.product_version_id=$1
     AND document.deleted_at IS NULL
     AND document.parse_status NOT IN ('deleting','deleted','cancelled')
     AND chunk.generation=document.active_generation
     AND document.enable_status='enabled'
     AND document.index_ready
     AND chunk.chunk_type=ANY(ARRAY[
       'text','parent_text','image_ocr','question','summary','image_caption','graph_node','wiki_page'])
     AND (chunk.chunk_type<>'image_ocr' OR EXISTS (
       SELECT 1 FROM public.knowledge_image_chunk_links mapping
        WHERE mapping.chunk_id=chunk.id))"#)
        .bind(version).bind(revision).fetch_one(conn).await
}

pub(super) async fn pending_derived(
    conn: &mut PgConnection,
    version: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM knowledge_documents WHERE product_version_id=$1
        AND deleted_at IS NULL AND parse_status NOT IN ('deleting','deleted','cancelled')
        AND (parse_status IN ('pending','processing','finalizing') OR pending_subtasks_count<>0
             OR summary_status IN ('pending','processing')))",
    )
    .bind(version)
    .fetch_one(conn)
    .await
}

pub(super) async fn lock_version(
    conn: &mut PgConnection,
    version: Uuid,
) -> Result<bool, sqlx::Error> {
    Ok(sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM knowledge_product_versions WHERE id=$1 AND deleted_at IS NULL FOR UPDATE",
    )
    .bind(version)
    .fetch_optional(conn)
    .await?
    .is_some())
}

pub(super) async fn lock_sources(
    conn: &mut PgConnection,
    version: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "SELECT id FROM knowledge_documents WHERE product_version_id=$1 ORDER BY id FOR UPDATE",
    )
    .bind(version)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "SELECT id FROM knowledge_chunks WHERE product_version_id=$1 ORDER BY id FOR UPDATE",
    )
    .bind(version)
    .execute(conn)
    .await?;
    Ok(())
}

async fn binding(
    conn: &mut PgConnection,
    version: Uuid,
) -> Result<Option<(String, crate::SupportState)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT binding.embedding_revision_sha256,revision.support_state
        FROM knowledge_product_version_embedding_bindings binding JOIN knowledge_embedding_revisions revision
        ON revision.revision_sha256=binding.embedding_revision_sha256
        WHERE binding.product_version_id=$1 FOR SHARE OF binding,revision",
    )
    .bind(version)
    .fetch_optional(conn)
    .await
}

pub(super) async fn supported_binding(
    conn: &mut PgConnection,
    version: Uuid,
    revision: &str,
) -> Result<bool, sqlx::Error> {
    Ok(binding(conn, version)
        .await?
        .is_some_and(|(sha, state)| sha == revision && state == crate::SupportState::Supported))
}

fn intent(row: sqlx::postgres::PgRow) -> Result<SemanticIndexIntentV2, sqlx::Error> {
    Ok(SemanticIndexIntentV2 {
        id: row.get("id"),
        product_version_id: row.get("product_version_id"),
        embedding_revision_sha256: row.get("embedding_revision_sha256"),
        source_snapshot_sha256: row.get("source_snapshot_sha256"),
        target_revision: row.get("target_revision"),
        status: row.try_get("status")?,
        generation_marker_sha256: row.get("generation_marker_sha256"),
        last_error_code: row.get("last_error_code"),
    })
}

pub(crate) async fn prepare(
    conn: &mut PgConnection,
    version: Uuid,
) -> Result<SemanticIndexPreparationV2, sqlx::Error> {
    if !lock_version(conn, version).await? {
        return Err(sqlx::Error::RowNotFound);
    }
    let Some((revision, state)) = binding(conn, version).await? else {
        return Ok(SemanticIndexPreparationV2::Unbound);
    };
    lock_sources(conn, version).await?;
    if pending_derived(conn, version).await? {
        return Ok(SemanticIndexPreparationV2::PendingDerived);
    }
    let (snapshot, count) = source_snapshot_v2(conn, version, &revision).await?;
    sqlx::query(
        "UPDATE knowledge_semantic_index_intents
        SET status='superseded',last_error_code='SOURCE_GENERATION_SUPERSEDED',
            last_error_detail='a newer settled source or binding generation exists'
        WHERE product_version_id=$1 AND status='pending'
          AND (embedding_revision_sha256<>$2 OR source_snapshot_sha256<>$3 OR $4<>'supported')",
    )
    .bind(version)
    .bind(&revision)
    .bind(&snapshot)
    .bind(state)
    .execute(&mut *conn)
    .await?;
    let rows = sqlx::query(
        "SELECT * FROM knowledge_semantic_index_intents
        WHERE product_version_id=$1 AND embedding_revision_sha256=$2 AND source_snapshot_sha256=$3
        ORDER BY target_revision DESC",
    )
    .bind(version)
    .bind(&revision)
    .bind(&snapshot)
    .fetch_all(&mut *conn)
    .await?;
    let intents: Vec<_> = rows.into_iter().map(intent).collect::<Result<_, _>>()?;
    if let Some(value) = intents
        .iter()
        .find(|value| value.status == crate::SemanticIndexIntentStatus::Pending)
    {
        return Ok(SemanticIndexPreparationV2::Enqueue(value.clone()));
    }
    if state == crate::SupportState::Supported
        && generations_match(conn, version, &revision, &snapshot, count).await?
    {
        if let Some(value) = intents
            .iter()
            .find(|value| value.status == crate::SemanticIndexIntentStatus::Completed)
        {
            return Ok(SemanticIndexPreparationV2::Ready(value.clone()));
        }
    } else if state != crate::SupportState::Supported
        && let Some(value) = intents
            .iter()
            .find(|value| value.status == crate::SemanticIndexIntentStatus::Terminal)
    {
        return Ok(SemanticIndexPreparationV2::Terminal(value.clone()));
    }
    let supported = state == crate::SupportState::Supported;
    let row=sqlx::query("INSERT INTO knowledge_semantic_index_intents(
        product_version_id,embedding_revision_sha256,source_snapshot_sha256,target_revision,status,last_error_code,last_error_detail)
        SELECT $1,$2,$3,COALESCE(max(target_revision),0)+1,$4,$5,$6
        FROM knowledge_semantic_index_intents WHERE product_version_id=$1 RETURNING *")
        .bind(version).bind(revision).bind(snapshot).bind(if supported {crate::SemanticIndexIntentStatus::Pending}else{crate::SemanticIndexIntentStatus::Terminal})
        .bind((!supported).then_some("EMBEDDING_REVISION_REVOKED"))
        .bind((!supported).then_some("embedding revision is revoked"))
        .fetch_one(conn).await?;
    Ok(if supported {
        SemanticIndexPreparationV2::Enqueue(intent(row)?)
    } else {
        SemanticIndexPreparationV2::Terminal(intent(row)?)
    })
}

async fn set_status(
    conn: &mut PgConnection,
    id: Uuid,
    status: crate::SemanticIndexIntentStatus,
    code: &str,
    detail: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE knowledge_semantic_index_intents SET status=$2,last_error_code=$3,last_error_detail=$4 WHERE id=$1")
        .bind(id).bind(status).bind(code).bind(detail).execute(conn).await?;
    Ok(())
}

pub(crate) async fn preflight(
    conn: &mut PgConnection,
    id: Uuid,
    target: i64,
    complete: bool,
) -> Result<String, sqlx::Error> {
    // All intent writers lock the parent before its child, matching prepare().
    let version:Option<Uuid>=sqlx::query_scalar("SELECT product_version_id FROM knowledge_semantic_index_intents WHERE id=$1 AND target_revision=$2")
        .bind(id).bind(target).fetch_optional(&mut *conn).await?;
    let Some(version) = version else {
        return Ok("duplicate".into());
    };
    let live = lock_version(conn, version).await?;
    let row=sqlx::query("SELECT * FROM knowledge_semantic_index_intents WHERE id=$1 AND target_revision=$2 FOR UPDATE")
        .bind(id).bind(target).fetch_optional(&mut *conn).await?;
    let Some(row) = row else {
        return Ok("duplicate".into());
    };
    let value = intent(row)?;
    if complete && value.status == crate::SemanticIndexIntentStatus::Completed {
        return Ok("duplicate".into());
    }
    if value.status != crate::SemanticIndexIntentStatus::Pending {
        return Ok(value.status.as_str().into());
    }
    if !live {
        set_status(
            conn,
            id,
            crate::SemanticIndexIntentStatus::Superseded,
            "SOURCE_GENERATION_SUPERSEDED",
            "product version is no longer live",
        )
        .await?;
        return Ok("superseded".into());
    }
    if !supported_binding(conn, version, &value.embedding_revision_sha256).await? {
        set_status(
            conn,
            id,
            crate::SemanticIndexIntentStatus::Terminal,
            "EMBEDDING_REVISION_UNSUPPORTED",
            "embedding binding or revision is unsupported",
        )
        .await?;
        return Ok("terminal".into());
    }
    lock_sources(conn, version).await?;
    if pending_derived(conn, version).await? {
        return Ok("pending_derived".into());
    }
    let (snapshot, count) =
        source_snapshot_v2(conn, version, &value.embedding_revision_sha256).await?;
    if snapshot != value.source_snapshot_sha256 {
        set_status(
            conn,
            id,
            crate::SemanticIndexIntentStatus::Superseded,
            "SOURCE_GENERATION_SUPERSEDED",
            "source snapshot changed before index publication",
        )
        .await?;
        return Ok("superseded".into());
    }
    if !complete {
        return Ok("current".into());
    }
    if !generations_match(
        conn,
        version,
        &value.embedding_revision_sha256,
        &snapshot,
        count,
    )
    .await?
    {
        return Ok("not_ready".into());
    }
    sqlx::query("UPDATE knowledge_semantic_index_intents SET status='completed',generation_marker_sha256=source_snapshot_sha256,
        last_error_code=NULL,last_error_detail=NULL,completed_at=clock_timestamp() WHERE id=$1 AND status='pending'")
        .bind(id).execute(conn).await?;
    Ok("completed".into())
}

async fn generations_match(
    conn: &mut PgConnection,
    version: Uuid,
    revision: &str,
    snapshot: &str,
    count: i64,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM knowledge_product_version_keyword_generations k
        JOIN knowledge_product_version_vector_generations v ON v.product_version_id=k.product_version_id
        AND v.embedding_revision_sha256=k.embedding_revision_sha256 AND v.source_snapshot_sha256=k.source_snapshot_sha256
        AND v.chunk_count=k.chunk_count
        WHERE k.product_version_id=$1 AND k.embedding_revision_sha256=$2 AND k.source_snapshot_sha256=$3 AND k.chunk_count=$4)")
        .bind(version).bind(revision).bind(snapshot).bind(count).fetch_one(conn).await
}

pub(crate) async fn rebuild_keywords(
    conn: &mut PgConnection,
    version: Uuid,
) -> Result<i64, sqlx::Error> {
    if !lock_version(conn, version).await? {
        return Err(sqlx::Error::RowNotFound);
    }
    let rows: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT id,content FROM knowledge_chunks WHERE product_version_id=$1 ORDER BY id FOR UPDATE",
    )
    .bind(version)
    .fetch_all(&mut *conn)
    .await?;
    let ids: Vec<_> = rows.iter().map(|(id, _)| *id).collect();
    let content: Vec<_> = rows.iter().map(|(_, content)| content.as_str()).collect();
    let tokens: Vec<_> = content
        .iter()
        .map(|content| crate::index::keyword_token_stream_v2(content))
        .collect();
    sqlx::query("DELETE FROM knowledge_chunk_keyword_indexes k USING knowledge_chunks c WHERE k.chunk_id=c.id
        AND c.product_version_id=$1 AND k.tokenizer='latin-numeric-cjk-bigram' AND k.tokenizer_version='v1'")
        .bind(version).execute(&mut *conn).await?;
    sqlx::query("INSERT INTO knowledge_chunk_keyword_indexes(chunk_id,tokenizer,tokenizer_version,indexed_content,indexed_content_sha256,tsv)
        SELECT id,'latin-numeric-cjk-bigram','v1',content,encode(digest(convert_to(content,'UTF8'),'sha256'),'hex'),to_tsvector('simple',tokens)
        FROM unnest($1::uuid[],$2::text[],$3::text[]) AS source(id,content,tokens)")
        .bind(ids).bind(content).bind(tokens).execute(conn).await?;
    Ok(rows.len() as i64)
}

pub(crate) async fn rebuild_semantic_keywords(
    conn: &mut PgConnection,
    version: Uuid,
    revision: &str,
    snapshot: &str,
) -> Result<i64, VectorIndexErrorV2> {
    use VectorIndexErrorV2::Database;
    if !lock_version(conn, version).await.map_err(Database)?
        || !supported_binding(conn, version, revision)
            .await
            .map_err(Database)?
    {
        return Err(VectorIndexErrorV2::InvalidConfiguration(
            "semantic embedding revision is unsupported".into(),
        ));
    }
    lock_sources(conn, version).await.map_err(Database)?;
    if pending_derived(conn, version).await.map_err(Database)? {
        return Err(VectorIndexErrorV2::PendingDerived(
            "semantic source has pending work".into(),
        ));
    }
    let (actual, count) = source_snapshot_v2(conn, version, revision)
        .await
        .map_err(Database)?;
    if actual != snapshot {
        return Err(VectorIndexErrorV2::SnapshotChanged(
            "semantic source snapshot changed".into(),
        ));
    }
    rebuild_keywords(conn, version).await.map_err(Database)?;
    sqlx::query("INSERT INTO knowledge_product_version_keyword_generations(product_version_id,embedding_revision_sha256,source_snapshot_sha256,chunk_count,completed_at)
        VALUES($1,$2,$3,$4,clock_timestamp()) ON CONFLICT(product_version_id) DO UPDATE SET
        embedding_revision_sha256=EXCLUDED.embedding_revision_sha256,source_snapshot_sha256=EXCLUDED.source_snapshot_sha256,
        chunk_count=EXCLUDED.chunk_count,completed_at=EXCLUDED.completed_at")
        .bind(version).bind(revision).bind(snapshot).bind(count).execute(conn).await.map_err(Database)?;
    Ok(count)
}
