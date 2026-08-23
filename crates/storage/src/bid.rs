//! Bid* persistence.

use sqlx::{PgPool, Row};
use uuid::Uuid;

pub struct NewSection<'a> {
    pub id: Uuid,
    pub project_id: Uuid,
    pub document_id: Uuid,
    pub section_key: &'a str,
    pub heading_path: &'a str,
    pub hint_family: &'a str,
    pub body: &'a str,
    pub extract_status: &'a str,
}

pub struct NewClause<'a> {
    pub id: Uuid,
    pub project_id: Uuid,
    pub extract_run_id: Option<Uuid>,
    pub section_id: Option<Uuid>,
    pub source_document_id: Option<Uuid>,
    pub source_span: Option<&'a serde_json::Value>,
    pub family_conflict: bool,
    pub extraction_meta: Option<&'a serde_json::Value>,
    pub raw_text: &'a str,
    pub text: &'a str,
    pub family: &'a str,
    pub must: bool,
    pub status: &'a str,
}

pub struct ClausePatch<'a> {
    pub id: Uuid,
    pub project_id: Uuid,
    pub expected_status: &'a str,
    pub text: Option<&'a str>,
    pub family: Option<&'a str>,
    pub must: Option<bool>,
    pub status: Option<&'a str>,
    pub deviate: Option<bool>,
    pub deviate_note: Option<&'a str>,
    pub assessment: Option<&'a str>,
}

pub struct ClauseUpdateResult {
    pub match_changed: bool,
}

pub struct ExtractionSectionRow<'a> {
    pub id: Uuid,
    pub section_key: &'a str,
    pub heading_path: &'a str,
    pub hint_family: &'a str,
    pub body: &'a str,
    pub extract_status: &'a str,
    pub error_message: &'a str,
}

pub struct PersistExtractionReport<'a> {
    pub run_id: Uuid,
    pub claim_token: Uuid,
    pub project_id: Uuid,
    pub document_id: Uuid,
    pub sections: &'a [ExtractionSectionRow<'a>],
    pub clauses: &'a [ExtractionClauseRow<'a>],
    pub replace_document: bool,
    pub scoped_section_count: Option<i32>,
}

pub struct ExtractionClauseRow<'a> {
    pub id: Uuid,
    pub section_key: &'a str,
    pub source_span: &'a serde_json::Value,
    pub family_conflict: bool,
    pub extraction_meta: &'a serde_json::Value,
    pub raw_text: &'a str,
    pub text: &'a str,
    pub family: &'a str,
    pub must: bool,
}

pub struct FinishExtractRun<'a> {
    pub id: Uuid,
    pub claim_token: Uuid,
    pub status: &'a str,
    pub section_total: i32,
    pub section_done: i32,
    pub error_message: &'a str,
    pub extractor_mode: &'a str,
    pub model_id: &'a str,
    pub policy_version: &'a str,
    pub prompt_version: &'a str,
    pub diagnostics: &'a serde_json::Value,
}

pub struct NewShot<'a> {
    pub id: Uuid,
    pub project_id: Uuid,
    pub clause_id: Uuid,
    pub product_id: Uuid,
    pub version_id: Uuid,
    pub source: &'a str,
    pub object_ref: &'a str,
    pub kb_document_id: Option<Uuid>,
    pub kb_image_ref: Option<&'a str>,
}

pub async fn insert_project(
    pool: &PgPool,
    id: Uuid,
    title: &str,
    owner_name: &str,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO bid_projects (id, title, owner_name, expires_at, status)
         VALUES ($1, $2, $3, $4, 'open')",
    )
    .bind(id)
    .bind(title)
    .bind(owner_name)
    .bind(expires_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_projects(pool: &PgPool) -> Result<Vec<sqlx::postgres::PgRow>, sqlx::Error> {
    sqlx::query(
        "SELECT id, title, owner_name, expires_at, status, ended_at, created_at
         FROM bid_projects ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await
}

pub async fn get_project(
    pool: &PgPool,
    id: Uuid,
) -> Result<Option<sqlx::postgres::PgRow>, sqlx::Error> {
    sqlx::query(
        "SELECT id, title, owner_name, expires_at, status, ended_at, created_at
         FROM bid_projects WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn end_project(pool: &PgPool, id: Uuid) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let ended: Option<Uuid> = sqlx::query_scalar(
        "UPDATE bid_projects SET status = 'ended', ended_at = now(), updated_at = now(),
                scheduled_watermark = mutation_watermark, match_dirty = false
         WHERE id = $1 AND status = 'open' AND extract_lock_token IS NULL
         RETURNING id",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;
    if ended.is_none() {
        tx.rollback().await?;
        return Ok(false);
    }
    cancel_project_work(&mut tx, &[id]).await?;
    tx.commit().await?;
    Ok(true)
}

async fn cancel_project_work(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project_ids: &[Uuid],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE bid_documents
         SET parse_status = 'failed', error_message = 'project_ended',
             conversion_claim_token = NULL, conversion_heartbeat_at = NULL,
             multimodal_status = CASE WHEN multimodal_status = 'running' THEN 'failed' ELSE multimodal_status END,
             multimodal_error = CASE WHEN multimodal_status = 'running' THEN 'project_ended' ELSE multimodal_error END
         WHERE project_id = ANY($1) AND parse_status IN ('pending', 'processing')",
    )
    .bind(project_ids)
    .execute(&mut **tx)
    .await?;
    let extract_run_ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM bid_extract_runs
         WHERE project_id = ANY($1) AND status IN ('pending', 'running')",
    )
    .bind(project_ids)
    .fetch_all(&mut **tx)
    .await?;
    crate::bid_extract_publication::ExtractionPublicationStore::terminalize_runs(
        tx,
        &extract_run_ids,
    )
    .await?;
    sqlx::query(
        "UPDATE bid_extract_runs
         SET status = 'failed', error_message = 'project_ended', claim_token = NULL,
             heartbeat_at = NULL, finished_at = now()
         WHERE project_id = ANY($1) AND status IN ('pending', 'running')",
    )
    .bind(project_ids)
    .execute(&mut **tx)
    .await?;
    for run_id in &extract_run_ids {
        crate::bid_extract_publication::ExtractionPublicationStore::refresh_run_aggregates(
            tx, *run_id,
        )
        .await?;
    }
    sqlx::query(
        "UPDATE bid_match_jobs
         SET status = 'failed', terminal_error_code = 'OTHER_BOUNDED',
             error_detail = 'project_ended', claim_token = NULL,
             runtime_principal = NULL, claimed_at = NULL, heartbeat_at = NULL,
             finished_at = now()
         WHERE project_id = ANY($1) AND status IN ('pending', 'running')",
    )
    .bind(project_ids)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "UPDATE bid_section_retry_jobs
         SET status = 'failed', error_message = 'project_ended', claim_token = NULL,
             heartbeat_at = NULL, finished_at = now()
         WHERE project_id = ANY($1) AND status IN ('pending', 'running')",
    )
    .bind(project_ids)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn lock_open_project(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let status: Option<String> =
        sqlx::query_scalar("SELECT status FROM bid_projects WHERE id = $1 FOR UPDATE")
            .bind(project_id)
            .fetch_optional(&mut **tx)
            .await?;
    Ok(status.as_deref() == Some("open"))
}

pub async fn insert_document(
    pool: &PgPool,
    id: Uuid,
    project_id: Uuid,
    file_name: &str,
    file_hash: &str,
    file_size: i64,
    object_ref: &str,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    if !lock_open_project(&mut tx, project_id).await? {
        return Err(sqlx::Error::Protocol("bid project is not open".into()));
    }
    sqlx::query(
        "INSERT INTO bid_documents
            (id, project_id, file_name, file_hash, file_size, object_ref, parse_status)
         VALUES ($1, $2, $3, $4, $5, $6, 'pending')",
    )
    .bind(id)
    .bind(project_id)
    .bind(file_name)
    .bind(file_hash)
    .bind(file_size)
    .bind(object_ref)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn list_documents(
    pool: &PgPool,
    project_id: Uuid,
) -> Result<Vec<sqlx::postgres::PgRow>, sqlx::Error> {
    sqlx::query(
        "SELECT d.id, d.project_id, d.file_name, d.file_hash, d.file_size, d.object_ref,
                d.parse_status, d.markdown_ref, d.parsed_at, d.error_message,
                d.multimodal_status, d.multimodal_error,
                r.status AS extract_status, r.error_message AS extract_error,
                (SELECT count(*) FROM bid_clauses c
                  WHERE c.source_document_id = d.id AND c.status <> 'superseded') AS clause_count
         FROM bid_documents d
         LEFT JOIN LATERAL (
             SELECT status, error_message FROM bid_extract_runs
             WHERE document_id = d.id
             ORDER BY started_at DESC NULLS LAST, id DESC
             LIMIT 1
         ) r ON true
         WHERE d.project_id = $1 ORDER BY d.created_at",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await
}

pub async fn claim_document_conversion(
    pool: &PgPool,
    id: Uuid,
) -> Result<Option<(Uuid, String, String, i64)>, sqlx::Error> {
    let token = Uuid::new_v4();
    let row = sqlx::query(
        "UPDATE bid_documents d
         SET parse_status = 'processing', conversion_claim_token = $2,
             conversion_heartbeat_at = now(), parsed_at = now(), error_message = '',
             multimodal_status = 'pending', multimodal_error = ''
         FROM bid_projects p
         WHERE d.id = $1 AND d.parse_status = 'pending' AND p.id = d.project_id
           AND p.status = 'open' AND p.extract_lock_token IS NULL
         RETURNING d.project_id, d.file_name, d.object_ref, d.conversion_generation",
    )
    .bind(id)
    .bind(token)
    .fetch_optional(pool)
    .await?;
    use sqlx::Row;
    Ok(row.map(|row| {
        (
            token,
            row.get("file_name"),
            row.get("object_ref"),
            row.get("conversion_generation"),
        )
    }))
}

pub async fn heartbeat_document_conversion(
    pool: &PgPool,
    id: Uuid,
    token: Uuid,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE bid_documents d SET conversion_heartbeat_at = now()
         FROM bid_projects p
         WHERE d.id = $1 AND d.parse_status = 'processing' AND d.conversion_claim_token = $2
           AND p.id = d.project_id AND p.status = 'open'",
    )
    .bind(id)
    .bind(token)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn set_document_multimodal_status(
    pool: &PgPool,
    id: Uuid,
    token: Uuid,
    status: &str,
    error: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE bid_documents d SET multimodal_status = $3, multimodal_error = $4
         FROM bid_projects p
         WHERE d.id = $1 AND d.parse_status = 'processing' AND d.conversion_claim_token = $2
           AND p.id = d.project_id AND p.status = 'open'",
    )
    .bind(id)
    .bind(token)
    .bind(status)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn finish_document_conversion(
    pool: &PgPool,
    id: Uuid,
    token: Uuid,
    status: &str,
    markdown_ref: Option<&str>,
    error: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE bid_documents
         SET parse_status = $3, markdown_ref = COALESCE($4, markdown_ref), error_message = $5,
             conversion_claim_token = NULL, conversion_heartbeat_at = NULL,
             multimodal_status = CASE
                 WHEN $3 = 'failed' AND multimodal_status = 'running' THEN 'failed'
                 ELSE multimodal_status
             END,
             multimodal_error = CASE
                 WHEN $3 = 'failed' AND multimodal_status = 'running' THEN $5
                 ELSE multimodal_error
             END,
             parsed_at = CASE WHEN $3 = 'completed' THEN now() ELSE parsed_at END
         FROM bid_projects p
         WHERE bid_documents.id = $1 AND bid_documents.parse_status = 'processing'
           AND bid_documents.conversion_claim_token = $2
           AND p.id = bid_documents.project_id AND p.status = 'open'
           AND ($3 <> 'completed' OR bid_documents.multimodal_status IN ('done', 'skipped'))",
    )
    .bind(id)
    .bind(token)
    .bind(status)
    .bind(markdown_ref)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn fail_pending_document_conversion(
    pool: &PgPool,
    id: Uuid,
    error: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE bid_documents d
         SET parse_status = 'failed', error_message = $2,
             multimodal_status = CASE WHEN multimodal_status IN ('done', 'skipped') THEN multimodal_status ELSE 'failed' END,
             multimodal_error = CASE WHEN multimodal_status IN ('done', 'skipped') THEN multimodal_error ELSE $2 END
         FROM bid_projects p
         WHERE d.id = $1 AND d.parse_status = 'pending'
           AND p.id = d.project_id AND p.status = 'open'",
    )
    .bind(id)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn reset_document_for_retry(
    pool: &PgPool,
    project_id: Uuid,
    document_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    if !lock_open_project(&mut tx, project_id).await? {
        return Err(sqlx::Error::Protocol("bid project is not open".into()));
    }
    let locked: Option<Uuid> =
        sqlx::query_scalar("SELECT extract_lock_token FROM bid_projects WHERE id = $1")
            .bind(project_id)
            .fetch_one(&mut *tx)
            .await?;
    if locked.is_some() {
        return Err(sqlx::Error::Protocol(
            "project extraction is running".into(),
        ));
    }
    let result = sqlx::query(
        "UPDATE bid_documents
         SET parse_status = 'pending', error_message = '', markdown_ref = NULL,
             conversion_generation = conversion_generation + 1,
             conversion_claim_token = NULL, conversion_heartbeat_at = NULL,
             multimodal_status = 'pending', multimodal_error = ''
         WHERE id = $2 AND project_id = $1",
    )
    .bind(project_id)
    .bind(document_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(result.rows_affected() == 1)
}

pub async fn delete_document_for_project(
    pool: &PgPool,
    project_id: Uuid,
    document_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    if !lock_open_project(&mut tx, project_id).await? {
        return Err(sqlx::Error::Protocol("bid project is not open".into()));
    }
    let locked: Option<Uuid> =
        sqlx::query_scalar("SELECT extract_lock_token FROM bid_projects WHERE id = $1")
            .bind(project_id)
            .fetch_one(&mut *tx)
            .await?;
    if locked.is_some() {
        return Err(sqlx::Error::Protocol(
            "project extraction is running".into(),
        ));
    }
    let extracting: bool = sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM bid_extract_runs
             WHERE document_id = $1 AND status = 'running'
        )",
    )
    .bind(document_id)
    .fetch_one(&mut *tx)
    .await?;
    if extracting {
        return Err(sqlx::Error::Protocol(
            "document extraction is running".into(),
        ));
    }
    let result = sqlx::query(
        "DELETE FROM bid_documents
         WHERE id = $2 AND project_id = $1 AND parse_status <> 'processing'",
    )
    .bind(project_id)
    .bind(document_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(result.rows_affected() == 1)
}

pub async fn document_row(
    pool: &PgPool,
    id: Uuid,
) -> Result<Option<sqlx::postgres::PgRow>, sqlx::Error> {
    sqlx::query(
        "SELECT id, project_id, file_name, file_hash, file_size, object_ref,
                parse_status, markdown_ref, parsed_at, error_message,
                multimodal_status, multimodal_error
         FROM bid_documents WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn extract_running(pool: &PgPool, project_id: Uuid) -> Result<bool, sqlx::Error> {
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM bid_extract_runs
         WHERE project_id = $1 AND status IN ('pending', 'running')",
    )
    .bind(project_id)
    .fetch_one(pool)
    .await?;
    Ok(n > 0)
}

pub async fn enqueue_section_retry(
    pool: &PgPool,
    project_id: Uuid,
    section_id: Uuid,
) -> Result<Uuid, sqlx::Error> {
    let mut tx = pool.begin().await?;
    if !lock_open_project(&mut tx, project_id).await? {
        return Err(sqlx::Error::Protocol("bid project is not open".into()));
    }
    let owned: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM bid_sections WHERE id = $1 AND project_id = $2)",
    )
    .bind(section_id)
    .bind(project_id)
    .fetch_one(&mut *tx)
    .await?;
    if !owned {
        return Err(sqlx::Error::RowNotFound);
    }
    if let Some(id) = sqlx::query_scalar(
        "SELECT id FROM bid_section_retry_jobs
         WHERE section_id = $1 AND status IN ('pending', 'running')",
    )
    .bind(section_id)
    .fetch_optional(&mut *tx)
    .await?
    {
        tx.commit().await?;
        return Ok(id);
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO bid_section_retry_jobs (id, project_id, section_id)
         VALUES ($1, $2, $3)",
    )
    .bind(id)
    .bind(project_id)
    .bind(section_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(id)
}

pub async fn claim_section_retry_job(
    pool: &PgPool,
    id: Uuid,
    project_id: Uuid,
    section_id: Uuid,
) -> Result<Option<Uuid>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let available: bool = sqlx::query_scalar(
        "SELECT status = 'open' AND extract_lock_token IS NULL
         FROM bid_projects WHERE id = $1 FOR UPDATE",
    )
    .bind(project_id)
    .fetch_optional(&mut *tx)
    .await?
    .unwrap_or(false);
    if !available {
        tx.rollback().await?;
        return Ok(None);
    }
    let extracting: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM bid_extract_runs WHERE project_id = $1 AND status = 'running')",
    )
    .bind(project_id)
    .fetch_one(&mut *tx)
    .await?;
    if extracting {
        tx.rollback().await?;
        return Ok(None);
    }
    let pending: bool = sqlx::query_scalar(
        "SELECT EXISTS(
             SELECT 1 FROM bid_section_retry_jobs j
             JOIN bid_sections s ON s.id = j.section_id
             WHERE j.id = $1 AND j.project_id = $2 AND j.section_id = $3
               AND j.status = 'pending' AND s.project_id = $2
         )",
    )
    .bind(id)
    .bind(project_id)
    .bind(section_id)
    .fetch_one(&mut *tx)
    .await?;
    if !pending {
        tx.rollback().await?;
        return Ok(None);
    }
    let token = Uuid::new_v4();
    sqlx::query(
        "UPDATE bid_projects
         SET extract_lock_token = $3, extract_lock_kind = 'section_retry',
             extract_lock_at = now(), extract_lock_section_id = $2
         WHERE id = $1",
    )
    .bind(project_id)
    .bind(section_id)
    .bind(token)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE bid_section_retry_jobs SET status = 'running', claim_token = $4,
             heartbeat_at = now(), error_message = ''
         WHERE id = $1 AND project_id = $2 AND section_id = $3 AND status = 'pending'",
    )
    .bind(id)
    .bind(project_id)
    .bind(section_id)
    .bind(token)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Some(token))
}

pub async fn heartbeat_section_retry_job(
    pool: &PgPool,
    id: Uuid,
    token: Uuid,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE bid_section_retry_jobs SET heartbeat_at = now()
         WHERE id = $1 AND status = 'running' AND claim_token = $2",
    )
    .bind(id)
    .bind(token)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn finish_section_retry_job(
    pool: &PgPool,
    id: Uuid,
    project_id: Uuid,
    section_id: Uuid,
    token: Uuid,
    status: &str,
    error: &str,
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let project = sqlx::query(
        "SELECT id FROM bid_projects
         WHERE id = $1 AND extract_lock_token = $3
           AND extract_lock_kind = 'section_retry' AND extract_lock_section_id = $2
         FOR UPDATE",
    )
    .bind(project_id)
    .bind(section_id)
    .bind(token)
    .fetch_optional(&mut *tx)
    .await?;
    if project.is_none() {
        tx.rollback().await?;
        return Ok(false);
    }
    let job = sqlx::query(
        "UPDATE bid_section_retry_jobs SET status = $5, claim_token = NULL,
             heartbeat_at = NULL, error_message = $6,
             finished_at = CASE WHEN $5 IN ('done', 'failed') THEN now() ELSE NULL END
         WHERE id = $1 AND project_id = $2 AND section_id = $3
           AND status = 'running' AND claim_token = $4",
    )
    .bind(id)
    .bind(project_id)
    .bind(section_id)
    .bind(token)
    .bind(status)
    .bind(error)
    .execute(&mut *tx)
    .await?;
    if job.rows_affected() != 1 {
        tx.rollback().await?;
        return Ok(false);
    }
    let project = sqlx::query(
        "UPDATE bid_projects
         SET extract_lock_token = NULL, extract_lock_kind = NULL,
             extract_lock_at = NULL, extract_lock_section_id = NULL
         WHERE id = $1 AND extract_lock_token = $3
           AND extract_lock_kind = 'section_retry' AND extract_lock_section_id = $2",
    )
    .bind(project_id)
    .bind(section_id)
    .bind(token)
    .execute(&mut *tx)
    .await?;
    if project.rows_affected() != 1 {
        tx.rollback().await?;
        return Ok(false);
    }
    tx.commit().await?;
    Ok(true)
}

pub async fn pending_section_retries(
    pool: &PgPool,
) -> Result<Vec<(Uuid, Uuid, Uuid)>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT j.id, j.project_id, j.section_id FROM bid_section_retry_jobs j
         JOIN bid_projects p ON p.id = j.project_id
         WHERE j.status = 'pending' AND p.status = 'open' ORDER BY j.created_at",
    )
    .fetch_all(pool)
    .await?;
    use sqlx::Row;
    Ok(rows
        .into_iter()
        .map(|row| (row.get("id"), row.get("project_id"), row.get("section_id")))
        .collect())
}

pub async fn reclaim_stale_section_retry_jobs(
    pool: &PgPool,
    stale_secs: i64,
) -> Result<Vec<(Uuid, Uuid, Uuid)>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let rows = sqlx::query(
        "SELECT id, project_id, section_id, claim_token
         FROM bid_section_retry_jobs
         WHERE status = 'running'
           AND heartbeat_at < now() - make_interval(secs => $1)
         FOR UPDATE",
    )
    .bind(stale_secs as f64)
    .fetch_all(&mut *tx)
    .await?;
    use sqlx::Row;
    let mut reclaimed = Vec::new();
    for row in rows {
        let id: Uuid = row.get("id");
        let project_id: Uuid = row.get("project_id");
        let section_id: Uuid = row.get("section_id");
        let token: Uuid = row.get("claim_token");
        sqlx::query(
            "UPDATE bid_projects
             SET extract_lock_token = NULL, extract_lock_kind = NULL,
                 extract_lock_at = NULL, extract_lock_section_id = NULL
             WHERE id = $1 AND extract_lock_token = $2
               AND extract_lock_kind = 'section_retry' AND extract_lock_section_id = $3",
        )
        .bind(project_id)
        .bind(token)
        .bind(section_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE bid_section_retry_jobs
             SET status = 'pending', claim_token = NULL, heartbeat_at = NULL,
                 error_message = 'stale_retry_reclaimed'
             WHERE id = $1 AND status = 'running' AND claim_token = $2",
        )
        .bind(id)
        .bind(token)
        .execute(&mut *tx)
        .await?;
        reclaimed.push((id, project_id, section_id));
    }
    tx.commit().await?;
    Ok(reclaimed)
}

pub async fn claim_section_retry(
    pool: &PgPool,
    project_id: Uuid,
    section_id: Uuid,
) -> Result<Option<Uuid>, sqlx::Error> {
    let token = Uuid::new_v4();
    sqlx::query_scalar(
        "UPDATE bid_projects p
         SET extract_lock_token = $3, extract_lock_kind = 'section_retry',
             extract_lock_at = now(), extract_lock_section_id = $2
         WHERE p.id = $1 AND p.status = 'open' AND p.extract_lock_token IS NULL
           AND EXISTS (
               SELECT 1 FROM bid_sections s
               WHERE s.id = $2 AND s.project_id = p.id
           )
           AND NOT EXISTS (
               SELECT 1 FROM bid_extract_runs r
               WHERE r.project_id = p.id AND r.status = 'running'
           )
         RETURNING extract_lock_token",
    )
    .bind(project_id)
    .bind(section_id)
    .bind(token)
    .fetch_optional(pool)
    .await
}

pub async fn finish_section_retry(
    pool: &PgPool,
    project_id: Uuid,
    section_id: Uuid,
    token: Uuid,
) -> Result<(), sqlx::Error> {
    let result = sqlx::query(
        "UPDATE bid_projects
         SET extract_lock_token = NULL, extract_lock_kind = NULL,
             extract_lock_at = NULL, extract_lock_section_id = NULL
         WHERE id = $1 AND extract_lock_token = $3
           AND extract_lock_kind = 'section_retry' AND extract_lock_section_id = $2",
    )
    .bind(project_id)
    .bind(section_id)
    .bind(token)
    .execute(pool)
    .await?;
    if result.rows_affected() != 1 {
        return Err(sqlx::Error::Protocol("section retry lease lost".into()));
    }
    Ok(())
}

/// Claim this pending run, or re-claim the same running run after a worker restart.
pub async fn claim_extract_run(
    pool: &PgPool,
    id: Uuid,
    project_id: Uuid,
    document_id: Option<Uuid>,
) -> Result<Option<Uuid>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let project: Option<(String, Option<String>)> = sqlx::query_as(
        "SELECT status, extract_lock_kind FROM bid_projects WHERE id = $1 FOR UPDATE",
    )
    .bind(project_id)
    .fetch_optional(&mut *tx)
    .await?;
    match project {
        Some((ref status, _)) if status == "open" => {}
        _ => {
            tx.rollback().await?;
            return Ok(None);
        }
    }
    if matches!(
        project.as_ref().and_then(|p| p.1.as_deref()),
        Some("section_retry")
    ) {
        tx.rollback().await?;
        return Ok(None);
    }
    let claim_token = Uuid::new_v4();
    let claimed = sqlx::query_scalar(
        "UPDATE bid_extract_runs
         SET status = 'running', started_at = now(), heartbeat_at = now(), finished_at = NULL,
             claim_token = $3, error_message = ''
         WHERE id = $1 AND project_id = $2
           AND document_id IS NOT DISTINCT FROM $4
           AND (
                status = 'pending'
                OR (status = 'running'
                    AND (heartbeat_at IS NULL
                         OR heartbeat_at < now() - make_interval(secs => 90)))
           )
           AND NOT EXISTS (
               SELECT 1 FROM bid_extract_runs r
               WHERE r.status = 'running' AND r.id <> $1
                 AND r.document_id IS NOT DISTINCT FROM $4
           )
         RETURNING claim_token",
    )
    .bind(id)
    .bind(project_id)
    .bind(claim_token)
    .bind(document_id)
    .fetch_optional(&mut *tx)
    .await;
    match claimed {
        Ok(token) => {
            tx.commit().await?;
            Ok(token)
        }
        Err(error) if is_unique_violation(&error) => {
            tx.rollback().await?;
            Ok(None)
        }
        Err(error) => {
            tx.rollback().await?;
            Err(error)
        }
    }
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|e| e.code())
        .is_some_and(|code| code == "23505")
}

/// Drop-path: put a crashed running run back to pending so it can be claimed again.
pub async fn release_extract_run_to_pending(
    pool: &PgPool,
    run_id: Uuid,
    claim_token: Uuid,
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let released = sqlx::query(
        "UPDATE bid_extract_runs
         SET status = 'pending', claim_token = NULL, heartbeat_at = NULL,
             error_message = 'extract_lease_released'
         WHERE id = $1 AND claim_token = $2 AND status = 'running'",
    )
    .bind(run_id)
    .bind(claim_token)
    .execute(&mut *tx)
    .await?;
    if released.rows_affected() != 1 {
        tx.rollback().await?;
        return Ok(false);
    }
    sqlx::query(
        "UPDATE bid_projects
         SET extract_lock_token = NULL, extract_lock_kind = NULL,
             extract_lock_at = NULL, extract_lock_section_id = NULL
         WHERE extract_lock_token = $1 AND extract_lock_kind = 'full'",
    )
    .bind(claim_token)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(true)
}

pub async fn heartbeat_extract_run(
    pool: &PgPool,
    run_id: Uuid,
    project_id: Uuid,
    claim_token: Uuid,
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let leased: Option<Uuid> = sqlx::query_scalar(
        "SELECT r.id FROM bid_extract_runs r
         WHERE r.id = $1 AND r.project_id = $2 AND r.status = 'running'
           AND r.claim_token = $3
         FOR UPDATE",
    )
    .bind(run_id)
    .bind(project_id)
    .bind(claim_token)
    .fetch_optional(&mut *tx)
    .await?;
    if leased.is_none() {
        tx.rollback().await?;
        return Ok(false);
    }
    sqlx::query("UPDATE bid_extract_runs SET heartbeat_at = now() WHERE id = $1")
        .bind(run_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(true)
}

pub async fn heartbeat_section_retry(
    pool: &PgPool,
    project_id: Uuid,
    section_id: Uuid,
    token: Uuid,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE bid_projects SET extract_lock_at = now()
         WHERE id = $1 AND extract_lock_token = $3
           AND extract_lock_kind = 'section_retry' AND extract_lock_section_id = $2",
    )
    .bind(project_id)
    .bind(section_id)
    .bind(token)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn reclaim_stale_extracts(
    pool: &PgPool,
    stale_secs: i64,
) -> Result<Vec<(Uuid, Uuid, Option<Uuid>)>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let stale = sqlx::query(
        "SELECT id, project_id, document_id, claim_token
         FROM bid_extract_runs
         WHERE status = 'running' AND heartbeat_at IS NOT NULL
           AND heartbeat_at < now() - make_interval(secs => $1)
         FOR UPDATE",
    )
    .bind(stale_secs as f64)
    .fetch_all(&mut *tx)
    .await?;
    for row in &stale {
        let id: Uuid = row.get("id");
        let project_id: Uuid = row.get("project_id");
        let claim_token: Option<Uuid> = row.get("claim_token");
        sqlx::query(
            "UPDATE bid_extract_runs
             SET status = 'pending', claim_token = NULL, heartbeat_at = NULL,
                 error_message = 'stale_extract_reclaimed'
             WHERE id = $1",
        )
        .bind(id)
        .execute(&mut *tx)
        .await?;
        if let Some(claim_token) = claim_token {
            sqlx::query(
                "UPDATE bid_projects
                 SET extract_lock_token = NULL, extract_lock_kind = NULL,
                     extract_lock_at = NULL, extract_lock_section_id = NULL
                 WHERE id = $1 AND extract_lock_token = $2",
            )
            .bind(project_id)
            .bind(claim_token)
            .execute(&mut *tx)
            .await?;
        }
    }
    // Legacy/orphan Section project leases have no durable running retry row.
    // Paired retry leases are reclaimed atomically by reclaim_stale_section_retry_jobs.
    let stale_retries = sqlx::query(
        "SELECT p.id, p.extract_lock_token, p.extract_lock_section_id
         FROM bid_projects p
         WHERE p.extract_lock_kind = 'section_retry'
           AND p.extract_lock_at < now() - make_interval(secs => $1)
           AND NOT EXISTS (
               SELECT 1 FROM bid_section_retry_jobs j
               WHERE j.project_id = p.id AND j.status = 'running'
                 AND j.claim_token = p.extract_lock_token
           )
         FOR UPDATE",
    )
    .bind(stale_secs as f64)
    .fetch_all(&mut *tx)
    .await?;
    for row in stale_retries {
        let project_id: Uuid = row.get("id");
        let token: Uuid = row.get("extract_lock_token");
        let section_id: Uuid = row.get("extract_lock_section_id");
        sqlx::query(
            "UPDATE bid_sections
             SET extract_status = 'failed', error_message = 'section_retry_stale_reclaimed'
             WHERE id = $1 AND project_id = $2 AND extract_status = 'running'",
        )
        .bind(section_id)
        .bind(project_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE bid_projects
             SET extract_lock_token = NULL, extract_lock_kind = NULL,
                 extract_lock_at = NULL, extract_lock_section_id = NULL
             WHERE id = $1 AND extract_lock_token = $2",
        )
        .bind(project_id)
        .bind(token)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(stale
        .into_iter()
        .map(|row| (row.get("id"), row.get("project_id"), row.get("document_id")))
        .collect())
}

pub async fn reclaim_stale_converts(
    pool: &PgPool,
    stale_secs: i64,
) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "UPDATE bid_documents d
         SET parse_status = 'pending', error_message = 'reclaimed stale processing',
             conversion_claim_token = NULL, conversion_heartbeat_at = NULL
         FROM bid_projects p
         WHERE d.parse_status = 'processing'
           AND d.conversion_heartbeat_at < now() - make_interval(secs => $1)
           AND p.id = d.project_id AND p.status = 'open'
         RETURNING d.id",
    )
    .bind(stale_secs as f64)
    .fetch_all(pool)
    .await
}

pub async fn insert_extract_run(
    pool: &PgPool,
    id: Uuid,
    project_id: Uuid,
    document_id: Option<Uuid>,
    triggered_by: &str,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    if !lock_open_project(&mut tx, project_id).await? {
        return Err(sqlx::Error::Protocol("bid project is not open".into()));
    }
    sqlx::query(
        "INSERT INTO bid_extract_runs
            (id, project_id, document_id, status, triggered_by, started_at)
         VALUES ($1, $2, $3, 'pending', $4, now())",
    )
    .bind(id)
    .bind(project_id)
    .bind(document_id)
    .bind(triggered_by)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn ensure_auto_extract_run(
    pool: &PgPool,
    document_id: Uuid,
) -> Result<Option<(Uuid, Uuid)>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let document: Option<(Uuid, i64)> = sqlx::query_as(
        "SELECT d.project_id, d.conversion_generation
         FROM bid_documents d JOIN bid_projects p ON p.id = d.project_id
         WHERE d.id = $1 AND d.parse_status = 'completed' AND p.status = 'open'
           AND d.error_message NOT LIKE 'conversion_quality=%'
         FOR UPDATE OF d, p",
    )
    .bind(document_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((project_id, generation)) = document else {
        tx.rollback().await?;
        return Ok(None);
    };
    let proposed = Uuid::new_v4();
    let inserted: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO bid_extract_runs
            (id, project_id, document_id, status, triggered_by, started_at, conversion_generation)
         VALUES ($1, $2, $3, 'pending', 'auto', now(), $4)
         ON CONFLICT (document_id, conversion_generation) WHERE triggered_by = 'auto'
         DO NOTHING RETURNING id",
    )
    .bind(proposed)
    .bind(project_id)
    .bind(document_id)
    .bind(generation)
    .fetch_optional(&mut *tx)
    .await?;
    let run_id = match inserted {
        Some(id) => id,
        None => {
            sqlx::query_scalar(
                "SELECT id FROM bid_extract_runs
             WHERE document_id = $1 AND conversion_generation = $2 AND triggered_by = 'auto'",
            )
            .bind(document_id)
            .bind(generation)
            .fetch_one(&mut *tx)
            .await?
        }
    };
    tx.commit().await?;
    Ok(Some((run_id, project_id)))
}

pub async fn finish_extract_run(
    pool: &PgPool,
    row: FinishExtractRun<'_>,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    let result = sqlx::query(
        "UPDATE bid_extract_runs SET
            status = $2,
            section_total = $3,
            section_done = $4,
            error_message = $5,
            extractor_mode = $6,
            model_id = $7,
            policy_version = $8,
            prompt_version = $9,
            diagnostics = $10,
            claim_token = NULL,
            heartbeat_at = NULL,
            finished_at = now()
         WHERE id = $1 AND claim_token = $11 AND status = 'running'",
    )
    .bind(row.id)
    .bind(row.status)
    .bind(row.section_total)
    .bind(row.section_done)
    .bind(row.error_message)
    .bind(row.extractor_mode)
    .bind(row.model_id)
    .bind(row.policy_version)
    .bind(row.prompt_version)
    .bind(row.diagnostics)
    .bind(row.claim_token)
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() != 1 {
        return Err(sqlx::Error::Protocol("extract run lease lost".into()));
    }
    crate::bid_extract_publication::ExtractionPublicationStore::sync_finished_extract_run(
        &mut tx, row.id,
    )
    .await?;
    sqlx::query(
        "UPDATE bid_projects
         SET extract_lock_token = NULL, extract_lock_kind = NULL,
             extract_lock_at = NULL, extract_lock_section_id = NULL
         WHERE extract_lock_token = $1 AND extract_lock_kind = 'full'",
    )
    .bind(row.claim_token)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn latest_extract(
    pool: &PgPool,
    project_id: Uuid,
) -> Result<Option<sqlx::postgres::PgRow>, sqlx::Error> {
    sqlx::query(
        "SELECT id, status, triggered_by,
                extractor_mode, model_id, policy_version, prompt_version,
                diagnostics, error_message, started_at, finished_at,
                target_count, published_target_count, scoped_section_count,
                published_section_count, partial_publication, partial_failure,
                worst_quality_status, degraded, reason_codes
         FROM bid_extract_runs
         WHERE project_id = $1
         ORDER BY started_at DESC NULLS LAST, id DESC
         LIMIT 1",
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await
}

pub async fn upsert_section(pool: &PgPool, row: NewSection<'_>) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO bid_sections
            (id, project_id, document_id, section_key, heading_path, hint_family, body, extract_status)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         ON CONFLICT (document_id, section_key) DO UPDATE SET
            heading_path = EXCLUDED.heading_path,
            hint_family = EXCLUDED.hint_family,
            body = EXCLUDED.body,
            extract_status = EXCLUDED.extract_status,
            error_message = ''
         RETURNING id",
    )
    .bind(row.id)
    .bind(row.project_id)
    .bind(row.document_id)
    .bind(row.section_key)
    .bind(row.heading_path)
    .bind(row.hint_family)
    .bind(row.body)
    .bind(row.extract_status)
    .fetch_one(pool)
    .await
}

pub async fn insert_clause(pool: &PgPool, row: NewClause<'_>) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    if !lock_open_project(&mut tx, row.project_id).await? {
        return Err(sqlx::Error::Protocol("bid project is not open".into()));
    }
    sqlx::query(
        "INSERT INTO bid_clauses
            (id, project_id, extract_run_id, section_id, source_document_id,
             source_span, family_conflict, extraction_meta,
             raw_text, text, family, must, status, confirmed_at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,COALESCE($8, '{}'::jsonb),$9,$10,$11,$12,$13,
                 CASE WHEN $13 = 'confirmed' THEN now() ELSE NULL END)",
    )
    .bind(row.id)
    .bind(row.project_id)
    .bind(row.extract_run_id)
    .bind(row.section_id)
    .bind(row.source_document_id)
    .bind(row.source_span)
    .bind(row.family_conflict)
    .bind(row.extraction_meta)
    .bind(row.raw_text)
    .bind(row.text)
    .bind(row.family)
    .bind(row.must)
    .bind(row.status)
    .execute(&mut *tx)
    .await?;
    if row.status == "confirmed" {
        crate::bid_matching::mark_project_matching_mutation(&mut tx, row.project_id).await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn prune_unconfirmed_sections(
    pool: &PgPool,
    document_id: Uuid,
    keep_keys: &[String],
) -> Result<(), sqlx::Error> {
    crate::bid_extract_publication::ExtractionPublicationStore::prune_unconfirmed_sections(
        pool,
        document_id,
        keep_keys,
    )
    .await
}

pub async fn list_clauses(
    pool: &PgPool,
    project_id: Uuid,
    include_superseded: bool,
) -> Result<Vec<sqlx::postgres::PgRow>, sqlx::Error> {
    sqlx::query(
        "SELECT id, project_id, extract_run_id, section_id, source_document_id,
                source_span, family_conflict, extraction_meta,
                raw_text, text, family, must, status, deviate, deviate_note,
                confirmed_at, superseded_by_run_id, assessment
         FROM bid_clauses WHERE project_id = $1
           AND ($2 OR status <> 'superseded')
         ORDER BY created_at",
    )
    .bind(project_id)
    .bind(include_superseded)
    .fetch_all(pool)
    .await
}

pub async fn update_clause(
    pool: &PgPool,
    row: ClausePatch<'_>,
) -> Result<Option<ClauseUpdateResult>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    if !lock_open_project(&mut tx, row.project_id).await? {
        return Err(sqlx::Error::Protocol("bid project is not open".into()));
    }
    let current = sqlx::query(
        "SELECT text, family, must, status, deviate, deviate_note, assessment
         FROM bid_clauses WHERE id = $1 AND project_id = $2 FOR UPDATE",
    )
    .bind(row.id)
    .bind(row.project_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(current) = current else {
        tx.rollback().await?;
        return Ok(None);
    };
    let old_status: String = current.get("status");
    if old_status != row.expected_status {
        tx.rollback().await?;
        return Ok(None);
    }
    let old_text: String = current.get("text");
    let old_family: String = current.get("family");
    let old_must: bool = current.get("must");
    let text = row.text.unwrap_or(&old_text);
    let family = row.family.unwrap_or(&old_family);
    let must = row.must.unwrap_or(old_must);
    let status = row.status.unwrap_or(&old_status);
    let old_match = old_status == "confirmed";
    let new_match = status == "confirmed";
    let match_changed = old_match != new_match
        || (new_match && (text != old_text || family != old_family || must != old_must));
    let deviate = row.deviate.unwrap_or_else(|| current.get("deviate"));
    let old_note: String = current.get("deviate_note");
    let deviate_note = row.deviate_note.unwrap_or(&old_note);
    let old_assessment: String = current.get("assessment");
    let assessment = row.assessment.unwrap_or(&old_assessment);
    sqlx::query(
        "UPDATE bid_clauses SET text = $2, family = $3, must = $4, status = $5,
                deviate = $6, deviate_note = $7, assessment = $8,
                family_conflict = CASE WHEN $5 = 'confirmed' THEN false ELSE family_conflict END,
                confirmed_at = CASE WHEN $5 = 'confirmed' THEN COALESCE(confirmed_at, now()) ELSE confirmed_at END
         WHERE id = $1 AND project_id = $9",
    )
    .bind(row.id)
    .bind(text)
    .bind(family)
    .bind(must)
    .bind(status)
    .bind(deviate)
    .bind(deviate_note)
    .bind(assessment)
    .bind(row.project_id)
    .execute(&mut *tx)
    .await?;
    if match_changed {
        crate::bid_matching::mark_project_matching_mutation(&mut tx, row.project_id).await?;
    }
    tx.commit().await?;
    Ok(Some(ClauseUpdateResult { match_changed }))
}

pub async fn confirmed_clauses(
    pool: &PgPool,
    project_id: Uuid,
    family: &str,
) -> Result<Vec<sqlx::postgres::PgRow>, sqlx::Error> {
    sqlx::query(
        "SELECT id, text, must, family, section_id FROM bid_clauses
         WHERE project_id = $1 AND status = 'confirmed' AND family = $2",
    )
    .bind(project_id)
    .bind(family)
    .fetch_all(pool)
    .await
}

pub async fn current_match_generation(pool: &PgPool, project_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT match_generation FROM bid_projects WHERE id = $1")
        .bind(project_id)
        .fetch_one(pool)
        .await
}

pub async fn dirty_match_projects(pool: &PgPool) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT id FROM bid_projects WHERE status = 'open' AND match_dirty ORDER BY updated_at",
    )
    .fetch_all(pool)
    .await
}

pub async fn any_match_running(pool: &PgPool, project_id: Uuid) -> Result<bool, sqlx::Error> {
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM bid_match_jobs j
         JOIN bid_projects p ON p.id = j.project_id
         WHERE j.project_id = $1 AND p.status = 'open'
           AND j.generation = p.match_generation AND j.status IN ('pending', 'running', 'committing')",
    )
    .bind(project_id)
    .fetch_one(pool)
    .await?;
    Ok(n > 0)
}

pub async fn insert_shot(pool: &PgPool, row: NewShot<'_>) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "INSERT INTO bid_shots
            (id, project_id, clause_id, product_id, version_id, source, object_ref, kb_document_id, kb_image_ref)
         SELECT $1,$2,$3,$4,$5,$6,$7,$8,$9
         WHERE EXISTS (
             SELECT 1 FROM bid_clauses c WHERE c.id = $3 AND c.project_id = $2
         ) AND EXISTS (
             SELECT 1 FROM product_versions v WHERE v.id = $5 AND v.product_id = $4
         )",
    )
    .bind(row.id)
    .bind(row.project_id)
    .bind(row.clause_id)
    .bind(row.product_id)
    .bind(row.version_id)
    .bind(row.source)
    .bind(row.object_ref)
    .bind(row.kb_document_id)
    .bind(row.kb_image_ref)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn list_shots(
    pool: &PgPool,
    project_id: Uuid,
) -> Result<Vec<sqlx::postgres::PgRow>, sqlx::Error> {
    sqlx::query(
        "SELECT id, project_id, clause_id, product_id, version_id, source, object_ref,
                kb_document_id, kb_image_ref
         FROM bid_shots WHERE project_id = $1",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await
}

pub async fn delete_shot(pool: &PgPool, project_id: Uuid, id: Uuid) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM bid_shots WHERE id = $2 AND project_id = $1")
        .bind(project_id)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() == 1)
}

pub fn row_uuid(row: &sqlx::postgres::PgRow, col: &str) -> Uuid {
    row.get(col)
}

pub async fn delete_document(pool: &PgPool, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM bid_documents WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn next_pending_extract(
    pool: &PgPool,
    project_id: Uuid,
) -> Result<Option<(Uuid, Option<Uuid>)>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT r.id, r.document_id FROM bid_extract_runs r
         JOIN bid_projects p ON p.id = r.project_id
         WHERE r.project_id = $1 AND r.status = 'pending' AND p.status = 'open'
         ORDER BY r.started_at NULLS FIRST, r.id LIMIT 1",
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| (r.get("id"), r.get("document_id"))))
}

pub async fn pending_converts(pool: &PgPool) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT d.id FROM bid_documents d
         JOIN bid_projects p ON p.id = d.project_id
         WHERE d.parse_status = 'pending' AND p.status = 'open' ORDER BY d.created_at",
    )
    .fetch_all(pool)
    .await
}

pub async fn end_expired_projects(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let ended: Vec<Uuid> = sqlx::query_scalar(
        "UPDATE bid_projects SET status = 'ended', ended_at = now(), updated_at = now(),
                scheduled_watermark = mutation_watermark, match_dirty = false
         WHERE status = 'open' AND expires_at IS NOT NULL AND expires_at <= now()
           AND extract_lock_token IS NULL
         RETURNING id",
    )
    .fetch_all(&mut *tx)
    .await?;
    if !ended.is_empty() {
        cancel_project_work(&mut tx, &ended).await?;
    }
    tx.commit().await?;
    Ok(ended.len() as u64)
}

pub async fn document_is_company(pool: &PgPool, document_id: Uuid) -> Result<bool, sqlx::Error> {
    let kind: Option<String> = sqlx::query_scalar(
        "SELECT w.kind FROM documents d
         JOIN product_versions pv ON pv.id = d.product_version_id
         JOIN products p ON p.id = pv.product_id
         JOIN workspaces w ON w.id = p.workspace_id
         WHERE d.id = $1",
    )
    .bind(document_id)
    .fetch_optional(pool)
    .await?;
    Ok(kind.as_deref() == Some("company"))
}

pub async fn open_projects_with_commercial(pool: &PgPool) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT DISTINCT p.id FROM bid_projects p
         JOIN bid_clauses c ON c.project_id = p.id
         WHERE p.status = 'open' AND c.status = 'confirmed' AND c.family = 'commercial'",
    )
    .fetch_all(pool)
    .await
}

pub async fn list_sections(
    pool: &PgPool,
    project_id: Uuid,
) -> Result<Vec<sqlx::postgres::PgRow>, sqlx::Error> {
    sqlx::query(
        "SELECT s.id, s.project_id, s.document_id, s.section_key, s.heading_path, s.hint_family,
                s.extract_status, s.error_message, s.merge_into,
                COALESCE((
                    SELECT j.status FROM bid_section_retry_jobs j
                    WHERE j.section_id = s.id
                    ORDER BY j.created_at DESC LIMIT 1
                ), '') AS retry_status,
                publication.published_extraction_generation,
                publication.stale AS publication_stale,
                publication.removed AS publication_removed,
                publication.quality_status AS publication_quality_status,
                publication.degraded AS publication_degraded,
                publication.reason_codes AS publication_reason_codes
         FROM bid_sections s
         LEFT JOIN bid_section_publication_state publication
           ON publication.document_id=s.document_id AND publication.section_key=s.section_key
         WHERE s.project_id = $1 ORDER BY s.heading_path",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await
}

pub async fn set_section_merge(
    pool: &PgPool,
    project_id: Uuid,
    section_id: Uuid,
    into: Option<Uuid>,
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    if !lock_open_project(&mut tx, project_id).await? {
        return Err(sqlx::Error::Protocol("bid project is not open".into()));
    }
    let graph_rows =
        sqlx::query("SELECT id, merge_into FROM bid_sections WHERE project_id = $1 FOR UPDATE")
            .bind(project_id)
            .fetch_all(&mut *tx)
            .await?;
    let graph: std::collections::HashMap<Uuid, Option<Uuid>> = graph_rows
        .iter()
        .map(|row| (row.get("id"), row.get("merge_into")))
        .collect();
    if !graph.contains_key(&section_id) || into.is_some_and(|target| !graph.contains_key(&target)) {
        tx.rollback().await?;
        return Ok(false);
    }
    if into == Some(section_id) {
        return Err(sqlx::Error::Protocol(
            "section cannot merge into itself".into(),
        ));
    }
    let mut cursor = into;
    let mut visited = std::collections::HashSet::new();
    while let Some(current) = cursor {
        if current == section_id || !visited.insert(current) {
            return Err(sqlx::Error::Protocol("section merge would cycle".into()));
        }
        cursor = graph.get(&current).copied().flatten();
    }
    let result =
        sqlx::query("UPDATE bid_sections SET merge_into = $3 WHERE id = $2 AND project_id = $1")
            .bind(project_id)
            .bind(section_id)
            .bind(into)
            .execute(&mut *tx)
            .await?;
    if result.rows_affected() != 1 {
        tx.rollback().await?;
        return Ok(false);
    }
    crate::bid_matching::mark_project_matching_mutation(&mut tx, project_id).await?;
    tx.commit().await?;
    Ok(true)
}

pub async fn section_row(
    pool: &PgPool,
    id: Uuid,
) -> Result<Option<sqlx::postgres::PgRow>, sqlx::Error> {
    sqlx::query(
        "SELECT id, project_id, document_id, section_key, heading_path, hint_family, body,
                extract_status
         FROM bid_sections WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn set_section_retry_status(
    pool: &PgPool,
    project_id: Uuid,
    section_id: Uuid,
    token: Uuid,
    status: &str,
    error: &str,
) -> Result<(), sqlx::Error> {
    let result = sqlx::query(
        "UPDATE bid_sections s
         SET extract_status = $4, error_message = $5
         FROM bid_projects p
         WHERE s.id = $2 AND s.project_id = $1 AND p.id = $1
           AND p.extract_lock_token = $3 AND p.extract_lock_kind = 'section_retry'
           AND p.extract_lock_section_id = s.id",
    )
    .bind(project_id)
    .bind(section_id)
    .bind(token)
    .bind(status)
    .bind(error)
    .execute(pool)
    .await?;
    if result.rows_affected() != 1 {
        return Err(sqlx::Error::Protocol("section retry lease lost".into()));
    }
    Ok(())
}

pub async fn project_file_stats(
    pool: &PgPool,
    project_id: Uuid,
) -> Result<(i64, i64, i64, i64, i64, bool), sqlx::Error> {
    let files: i64 = sqlx::query_scalar("SELECT count(*) FROM bid_documents WHERE project_id = $1")
        .bind(project_id)
        .fetch_one(pool)
        .await?;
    let ready: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM bid_documents WHERE project_id = $1 AND parse_status = 'completed'",
    )
    .bind(project_id)
    .fetch_one(pool)
    .await?;
    let drafts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM bid_clauses WHERE project_id = $1 AND status = 'draft'",
    )
    .bind(project_id)
    .fetch_one(pool)
    .await?;
    let picks: i64 =
        sqlx::query_scalar("SELECT count(*) FROM bid_pick_current_visible WHERE project_id = $1")
            .bind(project_id)
            .fetch_one(pool)
            .await?;
    let pending_files: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM bid_documents d
         WHERE d.project_id = $1 AND d.parse_status = 'completed'
           AND NOT EXISTS (
             SELECT 1 FROM bid_clauses c WHERE c.source_document_id = d.id
           )",
    )
    .bind(project_id)
    .fetch_one(pool)
    .await?;
    let extract_running = extract_running(pool, project_id).await?;
    Ok((files, ready, drafts, picks, pending_files, extract_running))
}

pub async fn list_booklet_parts(
    pool: &PgPool,
    project_id: Uuid,
) -> Result<Vec<sqlx::postgres::PgRow>, sqlx::Error> {
    sqlx::query(
        "SELECT project_id, part_key, markdown, generated_at, edited_at, stale
         FROM bid_booklet_parts WHERE project_id = $1 ORDER BY part_key",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await
}

pub async fn get_booklet_part(
    pool: &PgPool,
    project_id: Uuid,
    key: &str,
) -> Result<Option<sqlx::postgres::PgRow>, sqlx::Error> {
    sqlx::query(
        "SELECT project_id, part_key, markdown, generated_at, edited_at, stale
         FROM bid_booklet_parts WHERE project_id = $1 AND part_key = $2",
    )
    .bind(project_id)
    .bind(key)
    .fetch_optional(pool)
    .await
}

pub async fn upsert_booklet_generated(
    pool: &PgPool,
    project_id: Uuid,
    key: &str,
    markdown: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO bid_booklet_parts (project_id, part_key, markdown, generated_at, stale)
         VALUES ($1,$2,$3,now(), false)
         ON CONFLICT (project_id, part_key) DO UPDATE SET
            markdown = EXCLUDED.markdown,
            generated_at = now(),
            stale = false",
    )
    .bind(project_id)
    .bind(key)
    .bind(markdown)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn update_booklet_markdown(
    pool: &PgPool,
    project_id: Uuid,
    key: &str,
    markdown: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE bid_booklet_parts SET markdown = $3, edited_at = now() WHERE project_id = $1 AND part_key = $2",
    )
    .bind(project_id)
    .bind(key)
    .bind(markdown)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn mark_booklet_stale(
    pool: &PgPool,
    project_id: Uuid,
    keys: &[&str],
) -> Result<(), sqlx::Error> {
    if keys.is_empty() {
        return Ok(());
    }
    let owned: Vec<String> = keys.iter().map(|s| (*s).to_string()).collect();
    sqlx::query(
        "UPDATE bid_booklet_parts SET stale = true WHERE project_id = $1 AND part_key = ANY($2)",
    )
    .bind(project_id)
    .bind(owned)
    .execute(pool)
    .await?;
    Ok(())
}
