//! SQL persistence split from persist.rs (behavior unchanged).
use sqlx::{PgPool, Row};
use uuid::Uuid;

pub(crate) fn span_kind(name: &str) -> &'static str {
    match name {
        "document_processing" => "root",
        "docreader" | "chunking" | "embedding" | "multimodal" | "postprocess" => "stage",
        _ => "subspan",
    }
}

pub(crate) fn span_status(status: &str) -> &'static str {
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

pub(crate) async fn lookup_span_id(
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
