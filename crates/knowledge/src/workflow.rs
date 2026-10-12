//! Durable document-generation jobs. Domain writes and consumer receipts share
//! one transaction; Redis delivery is an at-least-once notification only.
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Work {
    Convert {
        passages: Vec<String>,
        manual: bool,
    },
    Postprocess {
        clone_keep: bool,
    },
    Summary,
    Datatable,
    Questions {
        chunk_ids: Vec<Uuid>,
        prev_ids: Vec<Option<Uuid>>,
        next_ids: Vec<Option<Uuid>>,
        batch: u32,
    },
    Extract {
        chunk_id: Uuid,
    },
    Image {
        image_key: String,
        image_source_type: String,
    },
    Wiki,
    Reconcile,
}
impl Work {
    pub fn key(&self) -> String {
        match self {
            Self::Convert { .. } => "convert".into(),
            Self::Postprocess { .. } => "postprocess".into(),
            Self::Summary => "summary".into(),
            Self::Datatable => "datatable".into(),
            Self::Questions { batch, .. } => format!("questions:{batch}"),
            Self::Extract { chunk_id } => format!("extract:{chunk_id}"),
            Self::Image { image_key, .. } => format!("image:{image_key}"),
            Self::Wiki => "wiki".into(),
            Self::Reconcile => "reconcile".into(),
        }
    }
    pub fn derived(&self) -> bool {
        matches!(
            self,
            Self::Summary
                | Self::Datatable
                | Self::Questions { .. }
                | Self::Extract { .. }
                | Self::Wiki
        )
    }
}

pub async fn current(pool: &PgPool, document_id: Uuid, attempt: i32) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM documents WHERE id=$1 AND attempt=$2 AND deleted_at IS NULL AND parse_status IN ('pending','processing','finalizing'))")
        .bind(document_id).bind(attempt).fetch_one(pool).await
}

pub(crate) async fn lock_current(
    tx: &mut Transaction<'_, Postgres>,
    document_id: Uuid,
    attempt: i32,
) -> Result<bool, sqlx::Error> {
    Ok(sqlx::query_scalar::<_, Uuid>("SELECT id FROM documents WHERE id=$1 AND attempt=$2 AND deleted_at IS NULL AND parse_status IN ('pending','processing','finalizing') FOR UPDATE")
        .bind(document_id).bind(attempt).fetch_optional(&mut **tx).await?.is_some())
}

pub(crate) async fn put_tx(
    tx: &mut Transaction<'_, Postgres>,
    document_id: Uuid,
    version_id: Uuid,
    attempt: i32,
    work: &Work,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO knowledge_job_outbox(document_id,product_version_id,attempt,job_key,payload,is_derived) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT(document_id,attempt,job_key) DO NOTHING")
        .bind(document_id).bind(version_id).bind(attempt).bind(work.key()).bind(serde_json::to_value(work).map_err(|e| sqlx::Error::Protocol(e.to_string()))?).bind(work.derived()).execute(&mut **tx).await?;
    Ok(())
}

pub async fn put(
    pool: &PgPool,
    document_id: Uuid,
    version_id: Uuid,
    attempt: i32,
    work: Work,
) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    if lock_current(&mut tx, document_id, attempt)
        .await
        .map_err(|e| e.to_string())?
    {
        put_tx(&mut tx, document_id, version_id, attempt, &work)
            .await
            .map_err(|e| e.to_string())?;
    }
    tx.commit().await.map_err(|e| e.to_string())
}

/// Return false for duplicate deliveries before any provider calls.
pub async fn pending(
    pool: &PgPool,
    document_id: Uuid,
    attempt: i32,
    key: &str,
) -> Result<bool, String> {
    if !current(pool, document_id, attempt)
        .await
        .map_err(|e| e.to_string())?
    {
        return Ok(false);
    }
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM knowledge_job_outbox WHERE document_id=$1 AND attempt=$2 AND job_key=$3 AND state IN ('pending','published')) AND NOT EXISTS(SELECT 1 FROM knowledge_job_receipts WHERE document_id=$1 AND attempt=$2 AND job_key=$3)")
        .bind(document_id).bind(attempt).bind(key).fetch_one(pool).await.map_err(|e|e.to_string())
}

pub(crate) async fn complete_tx(
    tx: &mut Transaction<'_, Postgres>,
    document_id: Uuid,
    attempt: i32,
    key: &str,
) -> Result<bool, sqlx::Error> {
    let fresh=sqlx::query("INSERT INTO knowledge_job_receipts(document_id,attempt,job_key) VALUES($1,$2,$3) ON CONFLICT DO NOTHING")
        .bind(document_id).bind(attempt).bind(key).execute(&mut **tx).await?.rows_affected()==1;
    if !fresh {
        return Ok(false);
    }
    sqlx::query("UPDATE knowledge_job_outbox SET state='complete',last_error=NULL WHERE document_id=$1 AND attempt=$2 AND job_key=$3")
        .bind(document_id).bind(attempt).bind(key).execute(&mut **tx).await?;
    if key.starts_with("image:") {
        sqlx::query("UPDATE document_processing_spans SET status='done',finished_at=now(),duration_ms=(EXTRACT(EPOCH FROM (now()-COALESCE(started_at,now())))*1000)::bigint WHERE document_id=$1 AND attempt=$2 AND name='multimodal' AND NOT EXISTS(SELECT 1 FROM knowledge_job_outbox WHERE document_id=$1 AND attempt=$2 AND payload->>'kind'='image' AND state<>'complete')")
            .bind(document_id).bind(attempt).execute(&mut **tx).await?;
        sqlx::query("INSERT INTO knowledge_job_outbox(document_id,product_version_id,attempt,job_key,payload) SELECT id,product_version_id,attempt,'postprocess','{\"kind\":\"postprocess\",\"clone_keep\":false}'::jsonb FROM documents WHERE id=$1 AND attempt=$2 AND NOT EXISTS(SELECT 1 FROM knowledge_job_outbox WHERE document_id=$1 AND attempt=$2 AND payload->>'kind'='image' AND state<>'complete') ON CONFLICT DO NOTHING")
            .bind(document_id).bind(attempt).execute(&mut **tx).await?;
    }
    // Derive the counter from durable obligations, never blind DECR on delivery.
    sqlx::query("UPDATE documents SET pending_subtasks_count=(SELECT count(*) FROM knowledge_job_outbox WHERE document_id=$1 AND attempt=$2 AND is_derived AND state<>'complete'), updated_at=now() WHERE id=$1 AND attempt=$2")
        .bind(document_id).bind(attempt).execute(&mut **tx).await?;
    sqlx::query("UPDATE documents SET parse_status='completed',active_generation=$2,index_ready=true,enable_status='enabled',updated_at=now() WHERE id=$1 AND attempt=$2 AND parse_status='finalizing' AND pending_subtasks_count=0 AND summary_status NOT IN ('pending','processing','failed') AND COALESCE(error_message,'')='' ")
        .bind(document_id).bind(attempt).execute(&mut **tx).await?;
    Ok(true)
}

pub async fn begin_postprocess(
    pool: &PgPool,
    document_id: Uuid,
    version_id: Uuid,
    attempt: i32,
    work: &[Work],
) -> Result<bool, String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    if !lock_current(&mut tx, document_id, attempt)
        .await
        .map_err(|e| e.to_string())?
    {
        return Ok(false);
    }
    let status: String = sqlx::query_scalar("SELECT parse_status FROM documents WHERE id=$1")
        .bind(document_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    if status != "processing" {
        return Ok(false);
    }
    for w in work {
        put_tx(&mut tx, document_id, version_id, attempt, w)
            .await
            .map_err(|e| e.to_string())?;
    }
    let summary = work.iter().any(|w| matches!(w, Work::Summary));
    sqlx::query("UPDATE documents SET parse_status='finalizing', pending_subtasks_count=$3, summary_status=CASE WHEN $4 THEN 'pending' ELSE 'none' END,updated_at=now() WHERE id=$1 AND attempt=$2")
        .bind(document_id).bind(attempt).bind(work.iter().filter(|w|w.derived()).count() as i32).bind(summary).execute(&mut *tx).await.map_err(|e|e.to_string())?;
    complete_tx(&mut tx, document_id, attempt, "postprocess")
        .await
        .map_err(|e| e.to_string())?;
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(true)
}

/// A single commit boundary for generated chunks, metadata and the receipt.
/// Work is computed in memory first; a stale generation performs zero writes.
#[allow(clippy::too_many_arguments)] // One atomic commit contract; all selectors are explicit.
pub async fn commit_chunks(
    pool: &PgPool,
    job: &crate::DocJob,
    attempt: i32,
    key: &str,
    types: &[&str],
    image_key: Option<&str>,
    parent_ids: &[Uuid],
    complete: bool,
) -> Result<bool, String> {
    let did = job.document.id;
    let chunks: Vec<_> = job
        .chunks
        .values()
        .filter(|c| {
            c.document_id == did
                && types.contains(&c.chunk_type.as_str())
                && image_key.is_none_or(|k| c.context_header == k)
                && (parent_ids.is_empty()
                    || c.parent_chunk_id.is_some_and(|p| parent_ids.contains(&p)))
        })
        .cloned()
        .collect();
    let ids: std::collections::HashSet<_> = chunks.iter().map(|c| c.id).collect();
    let embeddings: Vec<_> = job
        .embeddings
        .values()
        .filter(|e| ids.contains(&e.chunk_id))
        .cloned()
        .collect();
    let prepared = crate::catalog::chunks::prepare_image_artifacts(
        &chunks,
        crate::catalog::chunks::ArtifactMode::Strict,
    )
    .await
    .map_err(|e| e.to_string())?;
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    if !lock_current(&mut tx, did, attempt)
        .await
        .map_err(|e| e.to_string())?
    {
        return Ok(false);
    }
    if sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM knowledge_job_receipts WHERE document_id=$1 AND attempt=$2 AND job_key=$3)").bind(did).bind(attempt).bind(key).fetch_one(&mut *tx).await.map_err(|e|e.to_string())? { return Ok(false); }
    sqlx::query("DELETE FROM chunks WHERE document_id=$1 AND generation=$2 AND chunk_type=ANY($3) AND ($4::text IS NULL OR context_header=$4) AND (cardinality($5::uuid[])=0 OR parent_chunk_id=ANY($5))")
        .bind(did).bind(attempt).bind(types).bind(image_key).bind(parent_ids).execute(&mut *tx).await.map_err(|e|e.to_string())?;
    crate::catalog::chunks::append_document_chunks_tx(&mut tx, &chunks, &embeddings, &prepared)
        .await
        .map_err(|e| e.to_string())?;
    if key == "summary" {
        let status = match job.document.summary_status {
            crate::SummaryStatus::Completed => "completed",
            crate::SummaryStatus::Failed => "failed",
            _ => return Err("summary did not reach a terminal state".into()),
        };
        sqlx::query(
            "UPDATE documents SET description=$3,summary_status=$4 WHERE id=$1 AND attempt=$2",
        )
        .bind(did)
        .bind(attempt)
        .bind(&job.document.description)
        .bind(status)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    }
    for parent in parent_ids {
        if let Some(ch) = job.chunks.get(parent) {
            sqlx::query("UPDATE chunks SET generated_questions=$3 WHERE id=$1 AND generation=$2")
                .bind(parent)
                .bind(attempt)
                .bind(serde_json::json!(ch.generated_questions))
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
        }
    }
    if complete {
        complete_tx(&mut tx, did, attempt, key)
            .await
            .map_err(|e| e.to_string())?;
    }
    if key == "summary" && job.document.summary_status == crate::SummaryStatus::Failed {
        sqlx::query("UPDATE documents SET parse_status='failed',error_message='summary generation failed; retry required' WHERE id=$1 AND attempt=$2")
            .bind(did).bind(attempt).execute(&mut *tx).await.map_err(|e|e.to_string())?;
    }
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(true)
}

pub async fn complete(
    pool: &PgPool,
    document_id: Uuid,
    attempt: i32,
    key: &str,
) -> Result<bool, String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    if !lock_current(&mut tx, document_id, attempt)
        .await
        .map_err(|e| e.to_string())?
    {
        return Ok(false);
    }
    let changed = complete_tx(&mut tx, document_id, attempt, key)
        .await
        .map_err(|e| e.to_string())?;
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(changed)
}

/// At-least-once publication. Published messages are periodically offered again
/// until their consumer commits a receipt, including after Redis data loss.
pub async fn dispatch(pool: &PgPool) -> Result<(), String> {
    // Recovery is independent of optional housekeeping and creates a fresh
    // generation barrier. A timed-out worker cannot later publish into it.
    sqlx::query("UPDATE documents d SET attempt=attempt+1,parse_status='pending',enable_status='disabled',index_ready=false,pending_subtasks_count=0,summary_status='none',error_message='',updated_at=now() WHERE d.parse_status='processing' AND d.deleted_at IS NULL AND d.updated_at<now()-make_interval(secs=>$1::double precision) AND EXISTS(SELECT 1 FROM knowledge_job_outbox o WHERE o.document_id=d.id AND o.attempt=d.attempt AND o.job_key='convert' AND o.state IN ('pending','published'))")
        .bind(platform::HOUSEKEEP_STALE_SECS).execute(pool).await.map_err(|e|e.to_string())?;
    let max_attempts = std::env::var("KNOWLEDGEBRAIN_OUTBOX_MAX_DELIVERIES")
        .ok()
        .and_then(|v| v.parse::<i32>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(20);
    let rows:Vec<(Uuid,Uuid,i32,String,serde_json::Value,i32,i64)>=sqlx::query_as("SELECT document_id,product_version_id,attempt,job_key,payload,deliveries,revision FROM knowledge_job_outbox o WHERE state IN ('pending','published') AND next_delivery_at<=now() AND (job_key='reconcile' OR EXISTS(SELECT 1 FROM documents d WHERE d.id=o.document_id AND d.attempt=o.attempt AND d.deleted_at IS NULL AND (d.parse_status IN ('pending','processing','finalizing') OR (d.parse_status='failed' AND o.job_key='convert')))) ORDER BY next_delivery_at LIMIT 100")
        .fetch_all(pool).await.map_err(|e|e.to_string())?;
    for (did, vid, attempt, key, payload, deliveries, revision) in rows {
        let work: Work = serde_json::from_value(payload).map_err(|e| e.to_string())?;
        if !matches!(work, Work::Reconcile) && deliveries >= max_attempts {
            fail_job(
                pool,
                did,
                attempt,
                &key,
                "outbox delivery retries exhausted; retry the document generation",
            )
            .await?;
            continue;
        }
        let result = match work {
            Work::Convert { passages, manual } => {
                if manual {
                    platform::enqueue_manual_process(did, vid, attempt).await
                } else {
                    platform::enqueue_document_process_with(did, vid, attempt, passages).await
                }
            }
            Work::Postprocess { clone_keep } => {
                platform::enqueue_post_process(did, vid, clone_keep, attempt).await
            }
            Work::Summary => platform::enqueue_summary(did, attempt).await,
            Work::Datatable => platform::enqueue_datatable(did, attempt).await,
            Work::Questions {
                chunk_ids,
                prev_ids,
                next_ids,
                batch,
            } => {
                platform::enqueue_question_neighbors(
                    did, chunk_ids, prev_ids, next_ids, attempt, batch,
                )
                .await
            }
            Work::Extract { chunk_id } => platform::enqueue_extract(chunk_id, did, attempt).await,
            Work::Image {
                image_key,
                image_source_type,
            } => {
                platform::enqueue_image_multimodal(
                    did,
                    &image_key,
                    &image_source_type,
                    true,
                    true,
                    attempt,
                )
                .await
            }
            Work::Wiki => {
                platform::enqueue_wiki_ingest(vid, did, crate::wiki::OP_INGEST, attempt).await
            }
            Work::Reconcile => {
                let live:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM product_versions WHERE id=$1 AND deleted_at IS NULL AND status='active')").bind(vid).fetch_one(pool).await.map_err(|e|e.to_string())?;
                if !live {
                    sqlx::query("UPDATE knowledge_job_outbox SET state='complete' WHERE document_id=$1 AND attempt=$2 AND job_key=$3 AND revision=$4")
                        .bind(did).bind(attempt).bind(&key).bind(revision).execute(pool).await.map_err(|e|e.to_string())?;
                    continue;
                }
                use crate::knowledge_index_v2::SemanticIndexPreparationV2 as P;
                match crate::knowledge_index_v2::prepare_semantic_index_intent_v2(pool, vid)
                    .await
                    .map_err(|e| e.to_string())?
                {
                    P::Enqueue(intent) => {
                        if deliveries >= max_attempts {
                            sqlx::query("UPDATE knowledge_job_outbox SET state='failed',last_error='semantic delivery retries exhausted' WHERE document_id=$1 AND attempt=$2 AND job_key=$3 AND revision=$4")
                                .bind(did).bind(attempt).bind(&key).bind(revision).execute(pool).await.map_err(|e|e.to_string())?;
                            continue;
                        }
                        platform::enqueue_semantic_index_v2(intent.id, intent.target_revision).await
                    }
                    P::PendingDerived => {
                        sqlx::query("UPDATE knowledge_job_outbox SET last_error='waiting for derived work',next_delivery_at=now()+interval '1 minute' WHERE document_id=$1 AND attempt=$2 AND job_key=$3 AND revision=$4 AND state IN ('pending','published')")
                            .bind(did).bind(attempt).bind(&key).bind(revision).execute(pool).await.map_err(|e|e.to_string())?;
                        continue;
                    }
                    P::Ready(_) | P::Unbound | P::Superseded(_) => {
                        sqlx::query("UPDATE knowledge_job_outbox SET state='complete' WHERE document_id=$1 AND attempt=$2 AND job_key=$3 AND revision=$4").bind(did).bind(attempt).bind(&key).bind(revision).execute(pool).await.map_err(|e|e.to_string())?;
                        continue;
                    }
                    P::Terminal(_) => {
                        sqlx::query("UPDATE knowledge_job_outbox SET state='failed',last_error='semantic index needs attention' WHERE document_id=$1 AND attempt=$2 AND job_key=$3 AND revision=$4")
                            .bind(did).bind(attempt).bind(&key).bind(revision).execute(pool).await.map_err(|e|e.to_string())?;
                        continue;
                    }
                }
            }
        };
        let error = match result {
            Ok(Some(_)) => None,
            Ok(None) => Some("queue unavailable".to_owned()),
            Err(e) => Some(e),
        };
        sqlx::query("UPDATE knowledge_job_outbox SET state=CASE WHEN $4::text IS NULL THEN 'published' ELSE 'pending' END, deliveries=deliveries+1,last_error=$4,next_delivery_at=now()+CASE WHEN $4::text IS NULL THEN interval '15 minutes' ELSE interval '1 minute' END WHERE document_id=$1 AND attempt=$2 AND job_key=$3 AND revision=$5 AND state IN ('pending','published')")
            .bind(did).bind(attempt).bind(key).bind(error).bind(revision).execute(pool).await.map_err(|e|e.to_string())?;
    }
    Ok(())
}

pub async fn commit_graph(
    pool: &PgPool,
    job: &crate::DocJob,
    attempt: i32,
    key: &str,
) -> Result<bool, String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let did = job.document.id;
    if !lock_current(&mut tx, did, attempt)
        .await
        .map_err(|e| e.to_string())?
    {
        return Ok(false);
    }
    if !complete_tx(&mut tx, did, attempt, key)
        .await
        .map_err(|e| e.to_string())?
    {
        return Ok(false);
    }
    crate::graph::sql::persist_graph_maps_tx(&mut tx, &job.graph, &job.relations, did)
        .await
        .map_err(|e| e.to_string())?;
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(true)
}

pub async fn stage_index(
    pool: &PgPool,
    document_id: Uuid,
    attempt: i32,
    chunks: &[crate::Chunk],
    embeddings: &[crate::ChunkEmbedding],
    replace_chunks: bool,
) -> Result<bool, String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    if !lock_current(&mut tx, document_id, attempt)
        .await
        .map_err(|e| e.to_string())?
    {
        return Ok(false);
    }
    if replace_chunks {
        sqlx::query(
            "DELETE FROM chunks WHERE document_id=$1 AND generation=$2 AND chunk_type<>'wiki_page'",
        )
        .bind(document_id)
        .bind(attempt)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    } else {
        sqlx::query("DELETE FROM chunk_embeddings WHERE chunk_id IN (SELECT id FROM chunks WHERE document_id=$1 AND generation=$2)").bind(document_id).bind(attempt).execute(&mut *tx).await.map_err(|e|e.to_string())?;
    }
    crate::catalog::chunks::append_document_chunks_tx(
        &mut tx,
        if replace_chunks { chunks } else { &[] },
        embeddings,
        &[],
    )
    .await
    .map_err(|e| e.to_string())?;
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(true)
}

pub async fn update_status(
    pool: &PgPool,
    document_id: Uuid,
    attempt: i32,
    status: &str,
    error: &str,
) -> Result<(), String> {
    sqlx::query("UPDATE documents SET parse_status=$3,error_message=$4,updated_at=now() WHERE id=$1 AND attempt=$2 AND deleted_at IS NULL AND parse_status IN ('pending','processing','finalizing','failed')")
        .bind(document_id).bind(attempt).bind(status).bind(error).execute(pool).await.map_err(|e|e.to_string())?;
    Ok(())
}

pub async fn publish_empty(pool: &PgPool, document_id: Uuid, attempt: i32) -> Result<(), String> {
    sqlx::query("UPDATE documents SET parse_status='completed',active_generation=$2,index_ready=true,enable_status='enabled',updated_at=now() WHERE id=$1 AND attempt=$2 AND parse_status='processing' AND deleted_at IS NULL")
        .bind(document_id).bind(attempt).execute(pool).await.map_err(|e|e.to_string())?;
    Ok(())
}

/// Old document-wide delete envelopes can never purge a live generation.
pub async fn delete_generation(
    pool: &PgPool,
    document_id: Uuid,
    attempt: i32,
) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let row: Option<(i32, i32, String)> = sqlx::query_as(
        "SELECT attempt,active_generation,parse_status FROM documents WHERE id=$1 FOR UPDATE",
    )
    .bind(document_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;
    if let Some((current, active, status)) = row
        && (attempt < current && attempt != active
            || (attempt == current && matches!(status.as_str(), "deleting" | "deleted")))
    {
        sqlx::query(
            "DELETE FROM chunks WHERE document_id=$1 AND generation=$2 AND chunk_type<>'wiki_page'",
        )
        .bind(document_id)
        .bind(attempt)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    }
    tx.commit().await.map_err(|e| e.to_string())
}

pub async fn fail_job(
    pool: &PgPool,
    document_id: Uuid,
    attempt: i32,
    key: &str,
    error: &str,
) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let current:Option<Uuid>=sqlx::query_scalar("SELECT id FROM documents WHERE id=$1 AND attempt=$2 AND deleted_at IS NULL AND parse_status IN ('pending','processing','finalizing','failed') FOR UPDATE")
        .bind(document_id).bind(attempt).fetch_optional(&mut *tx).await.map_err(|e|e.to_string())?;
    if current.is_none() {
        return Ok(());
    }
    let changed=sqlx::query("UPDATE knowledge_job_outbox SET state='failed',last_error=$4 WHERE document_id=$1 AND attempt=$2 AND job_key=$3 AND state IN ('pending','published')")
        .bind(document_id).bind(attempt).bind(key).bind(error).execute(&mut *tx).await.map_err(|e|e.to_string())?.rows_affected();
    if changed > 0 {
        sqlx::query("UPDATE documents SET parse_status='failed',error_message=$3,summary_status=CASE WHEN summary_status IN ('pending','processing') THEN 'failed' ELSE summary_status END,index_ready=false,updated_at=now() WHERE id=$1 AND attempt=$2")
            .bind(document_id).bind(attempt).bind(error).execute(&mut *tx).await.map_err(|e|e.to_string())?;
    }
    tx.commit().await.map_err(|e| e.to_string())
}

/// Retry creates a new generation and fresh durable conversion obligation. Old
/// receipts, queue envelopes and staging chunks cannot mutate this generation.
pub async fn retry_document(pool: &PgPool, document_id: Uuid) -> Result<i32, String> {
    crate::bump_document_attempt(pool, document_id)
        .await
        .map_err(|e| e.to_string())
}
