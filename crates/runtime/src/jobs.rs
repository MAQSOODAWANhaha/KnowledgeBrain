//! Oxana job/queue types. Queue key `default`; task identity `document:process`.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const DOCUMENT_PROCESS_MAX_RETRY: u32 = 3;
pub const DOCUMENT_PROCESS_TIMEOUT_SECS: u64 = 2 * 60 * 60;
pub const POST_PROCESS_TIMEOUT_SECS: u64 = 30 * 60;

#[derive(Debug, Clone, Serialize, Deserialize, oxana::Job)]
#[oxana(unique_id = "document:process:{document_id}:{attempt}", on_conflict = Skip)]
pub struct DocumentProcessJob {
    pub document_id: Uuid,
    pub product_version_id: Uuid,
    pub attempt: i32,
    pub task_type: String,
    #[serde(default)]
    pub passages: Vec<String>,
    #[serde(default)]
    pub manual: bool,
}

#[derive(oxana::Queue)]
#[oxana(key = "default", concurrency = Dynamic(8))]
pub struct DefaultQueue;

#[derive(Debug, Clone, Serialize, Deserialize, oxana::Job)]
#[oxana(unique_id = "version:clone:{target_version_id}", on_conflict = Skip)]
pub struct VersionCloneJob {
    pub source_version_id: Uuid,
    pub target_version_id: Uuid,
    pub diffs: serde_json::Value,
    pub make_current: bool,
    pub task_type: String,
}

#[derive(oxana::Queue)]
#[oxana(key = "low", concurrency = Dynamic(4))]
pub struct LowQueue;

#[derive(Debug, Clone, Serialize, Deserialize, oxana::Job)]
#[oxana(unique_id = "knowledge:post_process:{document_id}", on_conflict = Skip)]
pub struct PostProcessJob {
    pub document_id: Uuid,
    pub product_version_id: Uuid,
    pub clone_keep: bool,
    pub task_type: String,
}

#[derive(oxana::Queue)]
#[oxana(key = "postprocess", concurrency = Dynamic(2))]
pub struct PostprocessQueue;

#[derive(Debug, Clone, Serialize, Deserialize, oxana::Job)]
#[oxana(unique_id = "summary:generation:{document_id}:{attempt}", on_conflict = Skip)]
pub struct SummaryJob {
    pub document_id: Uuid,
    pub attempt: i32,
    pub task_type: String,
}

#[derive(oxana::Queue)]
#[oxana(key = "summary", concurrency = Dynamic(12))]
pub struct SummaryQueue;

#[derive(Debug, Clone, Serialize, Deserialize, oxana::Job)]
#[oxana(unique_id = "question:generation:{document_id}:{batch}", on_conflict = Skip)]
pub struct QuestionJob {
    pub document_id: Uuid,
    pub chunk_ids: Vec<Uuid>,
    #[serde(default)]
    pub prev_ids: Vec<Option<Uuid>>,
    #[serde(default)]
    pub next_ids: Vec<Option<Uuid>>,
    pub attempt: i32,
    pub batch: u32,
    pub task_type: String,
}

#[derive(oxana::Queue)]
#[oxana(key = "question", concurrency = Dynamic(12))]
pub struct QuestionQueue;

#[derive(Debug, Clone, Serialize, Deserialize, oxana::Job)]
#[oxana(unique_id = "chunk:extract:{chunk_id}", on_conflict = Skip)]
pub struct ExtractJob {
    pub chunk_id: Uuid,
    pub document_id: Uuid,
    pub attempt: i32,
    pub task_type: String,
}

#[derive(oxana::Queue)]
#[oxana(key = "graph", concurrency = Dynamic(12))]
pub struct GraphQueue;

#[derive(Debug, Clone, Serialize, Deserialize, oxana::Job)]
#[oxana(unique_id = "datatable:summary:{document_id}", on_conflict = Skip)]
pub struct DatatableJob {
    pub document_id: Uuid,
    pub task_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, oxana::Job)]
#[oxana(unique_id = "knowledge:list_delete:{document_id}", on_conflict = Skip)]
pub struct ListDeleteJob {
    pub document_id: Uuid,
    pub task_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, oxana::Job)]
#[oxana(unique_id = "kb:delete:{product_version_id}", on_conflict = Skip)]
pub struct KbDeleteJob {
    pub product_version_id: Uuid,
    pub task_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, oxana::Job)]
#[oxana(unique_id = "knowledge:list_reparse:{document_id}", on_conflict = Skip)]
pub struct ListReparseJob {
    pub document_id: Uuid,
    pub task_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, oxana::Job)]
#[oxana(unique_id = "index:delete:{document_id}", on_conflict = Skip)]
pub struct IndexDeleteJob {
    pub document_id: Uuid,
    pub task_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, oxana::Job)]
#[oxana(
    unique_id = "image:multimodal:{document_id}:{image_key}:{attempt}",
    on_conflict = Skip
)]
pub struct ImageMultimodalJob {
    pub document_id: Uuid,
    pub image_key: String,
    pub image_source_type: String,
    pub enable_ocr: bool,
    pub enable_caption: bool,
    pub attempt: i32,
    pub task_type: String,
}

#[derive(oxana::Queue)]
#[oxana(key = "multimodal", concurrency = Dynamic(12))]
pub struct MultimodalQueue;

#[derive(Debug, Clone, Serialize, Deserialize, oxana::Job)]
#[oxana(unique_id = "wiki-ingest:{product_version_id}", on_conflict = Skip)]
pub struct WikiIngestJob {
    pub product_version_id: Uuid,
    pub task_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, oxana::Job)]
#[oxana(unique_id = "wiki-finalize:{product_version_id}", on_conflict = Skip)]
pub struct WikiFinalizeJob {
    pub product_version_id: Uuid,
    pub task_type: String,
}

#[derive(oxana::Queue)]
#[oxana(key = "wiki", concurrency = Dynamic(8))]
pub struct WikiQueue;

#[derive(Debug, Clone, Serialize, Deserialize, oxana::Job)]
#[oxana(unique_id = "bid:convert:{document_id}", on_conflict = Skip)]
pub struct BidConvertJob {
    pub document_id: Uuid,
    pub task_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, oxana::Job)]
#[oxana(unique_id = "bid:extract:{run_id}", on_conflict = Skip)]
pub struct BidExtractJob {
    pub run_id: Uuid,
    pub project_id: Uuid,
    pub document_id: Option<Uuid>,
    pub task_type: String,
}

#[derive(oxana::Queue)]
#[oxana(key = "bid-convert-v1", concurrency = Dynamic(4))]
pub struct BidConvertV1Queue;

#[derive(oxana::Queue)]
#[oxana(key = "bid-extract-v1", concurrency = Dynamic(4))]
pub struct BidExtractV1Queue;

#[derive(oxana::Queue)]
#[oxana(key = "bid-matching-v1", concurrency = Dynamic(4))]
pub struct BidMatchingV1Queue;

#[derive(oxana::Queue)]
#[oxana(key = "bid-render-v1", concurrency = Dynamic(2))]
pub struct BidRenderV1Queue;

pub const BID_MATCH_ROUTE_V1_SCHEMA: &str = "bid-match-route/v1";
pub const BID_MATCH_ROUTE_V1_PAYLOAD_VERSION: u16 = 1;
pub const BID_MATCH_ROUTE_V1_TRACE_ID_MAX: usize = 128;

#[derive(Debug, Clone, Serialize, Deserialize, oxana::Job)]
#[oxana(unique_id = "bid:match-route:v1:{job_id}", on_conflict = Skip)]
pub struct BidMatchRouteV1Job {
    pub job_id: Uuid,
    pub config_snapshot_id: Uuid,
    pub feature_snapshot_id: Uuid,
    pub score_policy_snapshot_id: Uuid,
    pub verifier_policy_snapshot_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    pub payload_version: u16,
    pub task_type: String,
}

impl BidMatchRouteV1Job {
    pub fn new(
        job_id: Uuid,
        snapshots: BidMatchRouteV1Snapshots,
        trace_id: Option<String>,
    ) -> Result<Self, String> {
        let trace_id = match trace_id {
            Some(value) => Some(bounded_opaque_trace_id(value)?),
            None => None,
        };
        Ok(Self {
            job_id,
            config_snapshot_id: snapshots.config_snapshot_id,
            feature_snapshot_id: snapshots.feature_snapshot_id,
            score_policy_snapshot_id: snapshots.score_policy_snapshot_id,
            verifier_policy_snapshot_id: snapshots.verifier_policy_snapshot_id,
            trace_id,
            payload_version: BID_MATCH_ROUTE_V1_PAYLOAD_VERSION,
            task_type: domain::TYPE_BID_MATCH_ROUTE_V1.to_string(),
        })
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.payload_version != BID_MATCH_ROUTE_V1_PAYLOAD_VERSION {
            return Err("rejected bid-match-route payload_version".into());
        }
        if self.task_type != domain::TYPE_BID_MATCH_ROUTE_V1 {
            return Err("rejected bid-match-route task_type".into());
        }
        if let Some(trace_id) = &self.trace_id {
            bounded_opaque_trace_id(trace_id.clone())?;
        }
        Ok(())
    }
}

pub const BID_RENDER_SUBMISSION_V1_SCHEMA: &str = "bid-render-submission/v1";
pub const BID_RENDER_SUBMISSION_V1_PAYLOAD_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, oxana::Job)]
#[oxana(
    unique_id = "bid:render-submission:v1:{render_job_id}",
    on_conflict = Skip
)]
pub struct BidRenderSubmissionV1Job {
    pub render_job_id: Uuid,
    pub payload_version: u16,
    pub task_type: String,
}

impl BidRenderSubmissionV1Job {
    pub fn new(render_job_id: Uuid) -> Result<Self, String> {
        let job = Self {
            render_job_id,
            payload_version: BID_RENDER_SUBMISSION_V1_PAYLOAD_VERSION,
            task_type: domain::TYPE_BID_RENDER_SUBMISSION_V1.to_string(),
        };
        job.validate()?;
        Ok(job)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.payload_version != BID_RENDER_SUBMISSION_V1_PAYLOAD_VERSION
            || self.task_type != domain::TYPE_BID_RENDER_SUBMISSION_V1
        {
            return Err("rejected bid-render-submission payload contract".into());
        }
        if self.render_job_id.is_nil() {
            return Err("rejected bid-render-submission render_job_id".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BidMatchRouteV1Snapshots {
    pub config_snapshot_id: Uuid,
    pub feature_snapshot_id: Uuid,
    pub score_policy_snapshot_id: Uuid,
    pub verifier_policy_snapshot_id: Uuid,
}

fn bounded_opaque_trace_id(value: String) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.len() > BID_MATCH_ROUTE_V1_TRACE_ID_MAX
        || !trimmed
            .bytes()
            .all(|b| b.is_ascii_graphic() || b == b'-' || b == b'_' || b == b':')
    {
        return Err("rejected bid-match-route trace_id".into());
    }
    Ok(trimmed.to_string())
}

/// Periodic sweep; oxana cron on `low` every 5 minutes (`HOUSEKEEP_CRON`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, oxana::Job)]
#[oxana(resurrect = false)]
pub struct HousekeepJob {}

pub fn runtime_concurrency(runtime: &str, default: usize) -> usize {
    let key = format!("KNOWLEDGEBRAIN_{runtime}_CONCURRENCY");
    std::env::var(key)
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|n| *n > 0)
        .unwrap_or(default)
}

pub fn redis_url() -> String {
    std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:16379".into())
}

pub async fn queue_depths() -> std::collections::HashMap<String, i64> {
    let mut out = std::collections::HashMap::new();
    let Ok(storage) = connect() else {
        return out;
    };
    for (name, n) in [
        (
            "default",
            storage.enqueued_count(DefaultQueue).await.unwrap_or(0),
        ),
        (
            "postprocess",
            storage.enqueued_count(PostprocessQueue).await.unwrap_or(0),
        ),
        ("low", storage.enqueued_count(LowQueue).await.unwrap_or(0)),
        (
            "multimodal",
            storage.enqueued_count(MultimodalQueue).await.unwrap_or(0),
        ),
        ("wiki", storage.enqueued_count(WikiQueue).await.unwrap_or(0)),
        (
            "summary",
            storage.enqueued_count(SummaryQueue).await.unwrap_or(0),
        ),
        (
            "question",
            storage.enqueued_count(QuestionQueue).await.unwrap_or(0),
        ),
        (
            "graph",
            storage.enqueued_count(GraphQueue).await.unwrap_or(0),
        ),
        (
            "bid-convert-v1",
            storage.enqueued_count(BidConvertV1Queue).await.unwrap_or(0),
        ),
        (
            "bid-extract-v1",
            storage.enqueued_count(BidExtractV1Queue).await.unwrap_or(0),
        ),
        (
            "bid-matching-v1",
            storage
                .enqueued_count(BidMatchingV1Queue)
                .await
                .unwrap_or(0),
        ),
        (
            "bid-render-v1",
            storage.enqueued_count(BidRenderV1Queue).await.unwrap_or(0),
        ),
    ] {
        out.insert(name.into(), n as i64);
    }
    out
}

pub fn connect() -> Result<oxana::Storage, oxana::OxanaError> {
    oxana::Storage::from_url(redis_url())
}

pub async fn connect_verified() -> Result<oxana::Storage, String> {
    let storage = connect().map_err(|error| error.to_string())?;
    storage
        .enqueued_count(DefaultQueue)
        .await
        .map_err(|error| error.to_string())?;
    Ok(storage)
}

/// Docker restart reuses hostname+pid, so oxana will not treat this process as dead.
/// Move leftover jobs from our processing list back onto their queues before workers start.
pub async fn replay_orphaned_local_jobs() -> Result<usize, String> {
    let client = redis::Client::open(redis_url()).map_err(|e| e.to_string())?;
    let mut con = client
        .get_multiplexed_async_connection()
        .await
        .map_err(|e| e.to_string())?;
    let pid = std::process::id();
    let mut hosts = vec![gethostname::gethostname().to_string_lossy().into_owned()];
    if let Ok(file_host) = std::fs::read_to_string("/etc/hostname") {
        let file_host = file_host.trim().to_string();
        if !file_host.is_empty() && !hosts.iter().any(|h| h == &file_host) {
            hosts.push(file_host);
        }
    }
    let mut n = 0usize;
    for host in hosts {
        let key = format!("oxanus:processing:{host}-{pid}");
        n += replay_processing_list(&mut con, &key).await?;
    }
    Ok(n)
}

async fn replay_processing_list(
    con: &mut redis::aio::MultiplexedConnection,
    key: &str,
) -> Result<usize, String> {
    let mut n = 0usize;
    loop {
        let id: Option<String> = redis::cmd("LPOP")
            .arg(key)
            .query_async(&mut *con)
            .await
            .map_err(|e| e.to_string())?;
        let Some(id) = id else {
            break;
        };
        let raw: Option<String> = redis::cmd("HGET")
            .arg("oxanus:jobs")
            .arg(&id)
            .query_async(&mut *con)
            .await
            .map_err(|e| e.to_string())?;
        let Some(raw) = raw else {
            continue;
        };
        let env: serde_json::Value = serde_json::from_str(&raw).unwrap_or(serde_json::Value::Null);
        let resurrect = env
            .pointer("/meta/resurrect")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        if resurrect {
            let queue = env
                .get("queue")
                .and_then(|v| v.as_str())
                .unwrap_or("default");
            let _: () = redis::cmd("LPUSH")
                .arg(format!("oxanus:queue:{queue}"))
                .arg(&id)
                .query_async(&mut *con)
                .await
                .map_err(|e| e.to_string())?;
            n += 1;
        } else {
            let _: () = redis::cmd("HDEL")
                .arg("oxanus:jobs")
                .arg(&id)
                .query_async(&mut *con)
                .await
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(n)
}

/// Pool build is lazy: `from_url` succeeds even when Redis is down.
/// Connection refused / deadpool errors mean "no oxana", not a failed enqueue.
fn redis_unreachable(err: &str) -> bool {
    let e = err.to_ascii_lowercase();
    e.contains("connection refused")
        || e.contains("deadpool")
        || e.contains("pool error")
        || e.contains("creating a new object")
        || e.contains("os error 111")
        || e.contains("connection reset")
        || e.contains("broken pipe")
        || e.contains("timed out")
        || e.contains("timeout")
        || e.contains("network is unreachable")
        || e.contains("no route to host")
        || e.contains("name or service not known")
        || e.contains("nodename nor servname")
}

fn oxana_id(r: Result<impl ToString, impl ToString>) -> Result<Option<String>, String> {
    match r {
        Ok(id) => Ok(Some(id.to_string())),
        Err(e) => {
            let msg = e.to_string();
            if redis_unreachable(&msg) {
                Ok(None)
            } else {
                Err(msg)
            }
        }
    }
}

/// Queue catalog for oxana-web. Does not start workers.
pub fn dashboard_catalog() -> Option<(oxana::Storage, oxana::Catalog)> {
    let storage = connect().ok()?;
    let builder = storage
        .runtime(())
        .queue::<DefaultQueue>()
        .queue::<PostprocessQueue>()
        .queue::<LowQueue>()
        .queue::<SummaryQueue>()
        .queue::<QuestionQueue>()
        .queue::<GraphQueue>()
        .queue::<MultimodalQueue>()
        .queue::<WikiQueue>()
        .queue::<BidConvertV1Queue>()
        .queue::<BidExtractV1Queue>()
        .queue::<BidMatchingV1Queue>();
    let catalog = builder.catalog();
    Some((storage, catalog))
}

pub async fn queue_job_previews() -> std::collections::HashMap<String, Vec<String>> {
    let mut out = std::collections::HashMap::new();
    let Ok(storage) = connect() else {
        return out;
    };
    let opts = oxana::QueueListOpts {
        count: 20,
        offset: 0,
    };
    for (name, jobs) in [
        (
            "default",
            storage.list_queue_jobs(DefaultQueue, &opts).await.ok(),
        ),
        (
            "postprocess",
            storage.list_queue_jobs(PostprocessQueue, &opts).await.ok(),
        ),
        ("low", storage.list_queue_jobs(LowQueue, &opts).await.ok()),
        (
            "summary",
            storage.list_queue_jobs(SummaryQueue, &opts).await.ok(),
        ),
        (
            "question",
            storage.list_queue_jobs(QuestionQueue, &opts).await.ok(),
        ),
        (
            "graph",
            storage.list_queue_jobs(GraphQueue, &opts).await.ok(),
        ),
        (
            "multimodal",
            storage.list_queue_jobs(MultimodalQueue, &opts).await.ok(),
        ),
        ("wiki", storage.list_queue_jobs(WikiQueue, &opts).await.ok()),
        (
            "bid-convert-v1",
            storage.list_queue_jobs(BidConvertV1Queue, &opts).await.ok(),
        ),
        (
            "bid-extract-v1",
            storage.list_queue_jobs(BidExtractV1Queue, &opts).await.ok(),
        ),
        (
            "bid-matching-v1",
            storage
                .list_queue_jobs(BidMatchingV1Queue, &opts)
                .await
                .ok(),
        ),
        (
            "bid-render-v1",
            storage.list_queue_jobs(BidRenderV1Queue, &opts).await.ok(),
        ),
    ] {
        if let Some(jobs) = jobs {
            out.insert(name.into(), jobs.iter().map(|j| format!("{j:?}")).collect());
        }
    }
    out
}

/// `Ok(None)` = Redis unreachable (memory queue still used). `Err` = connected but enqueue failed.
pub async fn enqueue_document_process(
    document_id: Uuid,
    product_version_id: Uuid,
    attempt: i32,
) -> Result<Option<String>, String> {
    enqueue_document_process_with(document_id, product_version_id, attempt, Vec::new()).await
}

pub async fn enqueue_document_process_with(
    document_id: Uuid,
    product_version_id: Uuid,
    attempt: i32,
    passages: Vec<String>,
) -> Result<Option<String>, String> {
    let Ok(storage) = connect() else {
        return Ok(None);
    };
    tracing::debug!(document_id = %document_id, job = "document:process", "enqueue");
    oxana_id(
        storage
            .enqueue(
                DefaultQueue,
                DocumentProcessJob {
                    document_id,
                    product_version_id,
                    attempt,
                    task_type: domain::TYPE_DOCUMENT_PROCESS.to_string(),
                    passages,
                    manual: false,
                },
            )
            .await,
    )
}

pub async fn enqueue_manual_process(
    document_id: Uuid,
    product_version_id: Uuid,
    attempt: i32,
) -> Result<Option<String>, String> {
    let Ok(storage) = connect() else {
        return Ok(None);
    };
    oxana_id(
        storage
            .enqueue(
                DefaultQueue,
                DocumentProcessJob {
                    document_id,
                    product_version_id,
                    attempt,
                    task_type: domain::TYPE_MANUAL_PROCESS.to_string(),
                    passages: Vec::new(),
                    manual: true,
                },
            )
            .await,
    )
}

pub async fn enqueue_post_process(
    document_id: Uuid,
    product_version_id: Uuid,
    clone_keep: bool,
) -> Result<Option<String>, String> {
    let Ok(storage) = connect() else {
        return Ok(None);
    };
    oxana_id(
        storage
            .enqueue(
                PostprocessQueue,
                PostProcessJob {
                    document_id,
                    product_version_id,
                    clone_keep,
                    task_type: domain::TYPE_POST_PROCESS.to_string(),
                },
            )
            .await,
    )
}

pub async fn enqueue_version_clone(
    source_version_id: Uuid,
    target_version_id: Uuid,
    diffs: serde_json::Value,
    make_current: bool,
) -> Result<Option<String>, String> {
    let Ok(storage) = connect() else {
        return Ok(None);
    };
    oxana_id(
        storage
            .enqueue(
                LowQueue,
                VersionCloneJob {
                    source_version_id,
                    target_version_id,
                    diffs,
                    make_current,
                    task_type: domain::TYPE_VERSION_CLONE.to_string(),
                },
            )
            .await,
    )
}

/// Spec 5.11 / brain `wikiIngestDelay`.
pub const WIKI_INGEST_DEBOUNCE_SECS: u64 = 30;
/// Spec 5.11 / brain `wikiFinalizeDelay`.
pub const WIKI_FINALIZE_DEBOUNCE_SECS: u64 = 20;
/// Brain `wikiFollowUpDelay` after a batch leaves remaining ingest rows.
pub const WIKI_FOLLOW_UP_DEBOUNCE_SECS: u64 = 5;
/// Brain `wikiIngestRetryDelay` / spec lock-conflict retry.
pub const WIKI_LOCK_RETRY_SECS: u64 = 15;

pub async fn enqueue_wiki_ingest(product_version_id: Uuid) -> Result<Option<String>, String> {
    enqueue_wiki_ingest_in(product_version_id, WIKI_INGEST_DEBOUNCE_SECS).await
}

pub async fn enqueue_wiki_ingest_in(
    product_version_id: Uuid,
    delay_secs: u64,
) -> Result<Option<String>, String> {
    let Ok(storage) = connect() else {
        return Ok(None);
    };
    oxana_id(
        storage
            .enqueue_in(
                WikiQueue,
                WikiIngestJob {
                    product_version_id,
                    task_type: domain::TYPE_WIKI_INGEST.to_string(),
                },
                delay_secs,
            )
            .await,
    )
}

pub async fn enqueue_image_multimodal(
    document_id: Uuid,
    image_key: &str,
    image_source_type: &str,
    enable_ocr: bool,
    enable_caption: bool,
    attempt: i32,
) -> Result<Option<String>, String> {
    if crate::queue_for(domain::TYPE_IMAGE_MULTIMODAL).starts_with("rejected:") {
        return Ok(None);
    }
    let Ok(storage) = connect() else {
        return Ok(None);
    };
    oxana_id(
        storage
            .enqueue(
                MultimodalQueue,
                ImageMultimodalJob {
                    document_id,
                    image_key: image_key.into(),
                    image_source_type: image_source_type.into(),
                    enable_ocr,
                    enable_caption,
                    attempt,
                    task_type: domain::TYPE_IMAGE_MULTIMODAL.to_string(),
                },
            )
            .await,
    )
}

pub async fn enqueue_summary(document_id: Uuid, attempt: i32) -> Result<Option<String>, String> {
    let Ok(storage) = connect() else {
        return Ok(None);
    };
    oxana_id(
        storage
            .enqueue(
                SummaryQueue,
                SummaryJob {
                    document_id,
                    attempt,
                    task_type: domain::TYPE_SUMMARY.to_string(),
                },
            )
            .await,
    )
}

pub async fn enqueue_question(
    document_id: Uuid,
    chunk_ids: Vec<Uuid>,
    attempt: i32,
    batch: u32,
) -> Result<Option<String>, String> {
    enqueue_question_neighbors(
        document_id,
        chunk_ids,
        Vec::new(),
        Vec::new(),
        attempt,
        batch,
    )
    .await
}

pub async fn enqueue_question_neighbors(
    document_id: Uuid,
    chunk_ids: Vec<Uuid>,
    prev_ids: Vec<Option<Uuid>>,
    next_ids: Vec<Option<Uuid>>,
    attempt: i32,
    batch: u32,
) -> Result<Option<String>, String> {
    if crate::queue_for(domain::TYPE_QUESTION).starts_with("rejected:") {
        return Ok(None);
    }
    let Ok(storage) = connect() else {
        return Ok(None);
    };
    oxana_id(
        storage
            .enqueue(
                QuestionQueue,
                QuestionJob {
                    document_id,
                    chunk_ids,
                    prev_ids,
                    next_ids,
                    attempt,
                    batch,
                    task_type: domain::TYPE_QUESTION.to_string(),
                },
            )
            .await,
    )
}

pub async fn enqueue_extract(
    chunk_id: Uuid,
    document_id: Uuid,
    attempt: i32,
) -> Result<Option<String>, String> {
    if crate::queue_for(domain::TYPE_CHUNK_EXTRACT).starts_with("rejected:") {
        return Ok(None);
    }
    let Ok(storage) = connect() else {
        return Ok(None);
    };
    oxana_id(
        storage
            .enqueue(
                GraphQueue,
                ExtractJob {
                    chunk_id,
                    document_id,
                    attempt,
                    task_type: domain::TYPE_CHUNK_EXTRACT.to_string(),
                },
            )
            .await,
    )
}

pub async fn enqueue_datatable(document_id: Uuid) -> Result<Option<String>, String> {
    let Ok(storage) = connect() else {
        return Ok(None);
    };
    oxana_id(
        storage
            .enqueue(
                SummaryQueue,
                DatatableJob {
                    document_id,
                    task_type: domain::TYPE_DATATABLE.to_string(),
                },
            )
            .await,
    )
}

pub async fn enqueue_list_delete(document_id: Uuid) -> Result<Option<String>, String> {
    let Ok(storage) = connect() else {
        return Ok(None);
    };
    oxana_id(
        storage
            .enqueue(
                LowQueue,
                ListDeleteJob {
                    document_id,
                    task_type: domain::TYPE_LIST_DELETE.to_string(),
                },
            )
            .await,
    )
}

pub async fn enqueue_kb_delete(product_version_id: Uuid) -> Result<Option<String>, String> {
    let Ok(storage) = connect() else {
        return Ok(None);
    };
    oxana_id(
        storage
            .enqueue(
                LowQueue,
                KbDeleteJob {
                    product_version_id,
                    task_type: domain::TYPE_KB_DELETE.to_string(),
                },
            )
            .await,
    )
}

pub async fn enqueue_index_delete(document_id: Uuid) -> Result<Option<String>, String> {
    let Ok(storage) = connect() else {
        return Ok(None);
    };
    oxana_id(
        storage
            .enqueue(
                LowQueue,
                IndexDeleteJob {
                    document_id,
                    task_type: domain::TYPE_INDEX_DELETE.to_string(),
                },
            )
            .await,
    )
}

pub async fn enqueue_list_reparse(document_id: Uuid) -> Result<Option<String>, String> {
    let Ok(storage) = connect() else {
        return Ok(None);
    };
    oxana_id(
        storage
            .enqueue(
                LowQueue,
                ListReparseJob {
                    document_id,
                    task_type: domain::TYPE_LIST_REPARSE.to_string(),
                },
            )
            .await,
    )
}

pub async fn enqueue_wiki_finalize(product_version_id: Uuid) -> Result<Option<String>, String> {
    enqueue_wiki_finalize_in(product_version_id, WIKI_FINALIZE_DEBOUNCE_SECS).await
}

pub async fn enqueue_bid_convert(document_id: Uuid) -> Result<Option<String>, String> {
    let Ok(storage) = connect() else {
        return Ok(None);
    };
    tracing::debug!(document_id = %document_id, job = "bid:convert", "enqueue");
    oxana_id(
        storage
            .enqueue(
                BidConvertV1Queue,
                BidConvertJob {
                    document_id,
                    task_type: domain::TYPE_BID_CONVERT.to_string(),
                },
            )
            .await,
    )
}

pub async fn enqueue_bid_extract(
    run_id: Uuid,
    project_id: Uuid,
    document_id: Option<Uuid>,
) -> Result<Option<String>, String> {
    let Ok(storage) = connect() else {
        return Ok(None);
    };
    let job = BidExtractJob {
        run_id,
        project_id,
        document_id,
        task_type: domain::TYPE_BID_EXTRACT.to_string(),
    };
    tracing::debug!(
        run_id = %run_id,
        project_id = %project_id,
        document_id = ?document_id,
        job = "bid:extract",
        "enqueue"
    );
    let _ = storage.delete_unique_job(&job).await;
    oxana_id(storage.enqueue(BidExtractV1Queue, job).await)
}

pub async fn enqueue_bid_match_route_v1(
    job_id: Uuid,
    snapshots: BidMatchRouteV1Snapshots,
    trace_id: Option<String>,
) -> Result<Option<String>, String> {
    let job = BidMatchRouteV1Job::new(job_id, snapshots, trace_id)?;
    let Ok(storage) = connect() else {
        return Ok(None);
    };
    tracing::debug!(job_id = %job_id, job = "bid:match-route:v1", "enqueue");
    oxana_id(storage.enqueue(BidMatchingV1Queue, job).await)
}

pub async fn enqueue_bid_render_submission_v1(
    render_job_id: Uuid,
) -> Result<Option<String>, String> {
    let job = BidRenderSubmissionV1Job::new(render_job_id)?;
    let Ok(storage) = connect() else {
        return Ok(None);
    };
    tracing::debug!(%render_job_id, job = "bid:render-submission:v1", "enqueue");
    oxana_id(storage.enqueue(BidRenderV1Queue, job).await)
}

pub async fn enqueue_wiki_finalize_in(
    product_version_id: Uuid,
    delay_secs: u64,
) -> Result<Option<String>, String> {
    let Ok(storage) = connect() else {
        return Ok(None);
    };
    oxana_id(
        storage
            .enqueue_in(
                WikiQueue,
                WikiFinalizeJob {
                    product_version_id,
                    task_type: domain::TYPE_WIKI_FINALIZE.to_string(),
                },
                delay_secs,
            )
            .await,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn redis_tests_required() -> bool {
        std::env::var("KNOWLEDGEBRAIN_REQUIRE_REDIS_TESTS")
            .map(|value| {
                matches!(
                    value.to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false)
    }

    fn redis_test_storage(test_name: &str) -> Option<oxana::Storage> {
        if std::env::var_os("REDIS_URL").is_none() && !redis_tests_required() {
            eprintln!("skip runtime test {test_name}: REDIS_URL is not configured");
            return None;
        }
        Some(connect().unwrap_or_else(|error| {
            panic!("required runtime test {test_name} could not configure redis: {error}")
        }))
    }

    fn redis_default_endpoint_open() -> bool {
        std::net::TcpStream::connect_timeout(
            &std::net::SocketAddr::from(([127, 0, 0, 1], 16379)),
            std::time::Duration::from_millis(150),
        )
        .is_ok()
    }

    async fn redis_live_if_up(test_name: &str) -> Option<oxana::Storage> {
        let url_set = std::env::var_os("REDIS_URL").is_some();
        if !url_set && !redis_tests_required() && !redis_default_endpoint_open() {
            eprintln!(
                "skip runtime test {test_name}: REDIS_URL unset and redis://127.0.0.1:16379 is down"
            );
            return None;
        }
        let storage = match connect() {
            Ok(storage) => storage,
            Err(error) => {
                if redis_tests_required() {
                    panic!("required runtime test {test_name} could not configure redis: {error}");
                }
                eprintln!("skip runtime test {test_name}: redis not configured ({error})");
                return None;
            }
        };
        match storage.enqueued_count(DefaultQueue).await {
            Ok(_) => Some(storage),
            Err(error) => {
                if redis_tests_required() {
                    panic!("required runtime test {test_name} redis is down: {error}");
                }
                eprintln!("skip runtime test {test_name}: redis not up ({error})");
                None
            }
        }
    }

    async fn redis_test_lock() -> tokio::sync::MutexGuard<'static, ()> {
        static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        LOCK.lock().await
    }

    #[test]
    fn wiki_debounce_matches_spec() {
        assert_eq!(WIKI_INGEST_DEBOUNCE_SECS, 30);
        assert_eq!(WIKI_FINALIZE_DEBOUNCE_SECS, 20);
        assert_eq!(WIKI_FOLLOW_UP_DEBOUNCE_SECS, 5);
        assert_eq!(WIKI_LOCK_RETRY_SECS, 15);
    }

    #[test]
    fn default_queue_key_is_brain_default() {
        assert_eq!(domain::QUEUE_DEFAULT, "default");
        assert_eq!(domain::QUEUE_WIKI, "wiki");
        match <WikiQueue as oxana::Queue>::to_config().kind {
            oxana::QueueKind::Static { key } => assert_eq!(key, "wiki"),
            other => panic!("wiki queue must be static, got {other:?}"),
        }
    }

    #[test]
    fn bid_match_route_v1_identity_schema_and_queue_key() {
        let job_id = Uuid::from_u128(1);
        let job = BidMatchRouteV1Job::new(
            job_id,
            BidMatchRouteV1Snapshots {
                config_snapshot_id: Uuid::from_u128(2),
                feature_snapshot_id: Uuid::from_u128(3),
                score_policy_snapshot_id: Uuid::from_u128(4),
                verifier_policy_snapshot_id: Uuid::from_u128(5),
            },
            Some("trace:route-1".into()),
        )
        .unwrap();
        assert_eq!(BID_MATCH_ROUTE_V1_SCHEMA, "bid-match-route/v1");
        assert_eq!(job.payload_version, BID_MATCH_ROUTE_V1_PAYLOAD_VERSION);
        assert_eq!(job.task_type, domain::TYPE_BID_MATCH_ROUTE_V1);
        assert_eq!(
            <BidMatchRouteV1Job as oxana::Job>::unique_id(&job),
            Some(format!("bid:match-route:v1:{job_id}"))
        );
        match <BidMatchingV1Queue as oxana::Queue>::to_config().kind {
            oxana::QueueKind::Static { key } => assert_eq!(key, domain::QUEUE_BID_MATCHING_V1),
            other => panic!("bid-matching-v1 queue must be static, got {other:?}"),
        }
        assert_eq!(
            crate::queue_for(domain::TYPE_BID_MATCH_ROUTE_V1),
            domain::QUEUE_BID_MATCHING_V1
        );
    }

    #[test]
    fn bid_render_submission_v1_identity_schema_and_queue_key() {
        let render_job_id = Uuid::from_u128(4);
        let job = BidRenderSubmissionV1Job::new(render_job_id).unwrap();
        assert_eq!(BID_RENDER_SUBMISSION_V1_SCHEMA, "bid-render-submission/v1");
        assert_eq!(
            job.payload_version,
            BID_RENDER_SUBMISSION_V1_PAYLOAD_VERSION
        );
        assert_eq!(job.task_type, domain::TYPE_BID_RENDER_SUBMISSION_V1);
        assert_eq!(job.render_job_id, render_job_id);
        assert_eq!(
            <BidRenderSubmissionV1Job as oxana::Job>::unique_id(&job),
            Some(format!("bid:render-submission:v1:{render_job_id}"))
        );
        match <BidRenderV1Queue as oxana::Queue>::to_config().kind {
            oxana::QueueKind::Static { key } => assert_eq!(key, domain::QUEUE_BID_RENDER_V1),
            other => panic!("bid-render-v1 queue must be static, got {other:?}"),
        }
        assert_eq!(
            crate::queue_for(domain::TYPE_BID_RENDER_SUBMISSION_V1),
            domain::QUEUE_BID_RENDER_V1
        );
    }

    #[test]
    fn unknown_old_bid_match_payload_is_rejected_not_default() {
        assert!(crate::rejected_legacy_bid_match_task("bid:match"));
        assert!(crate::rejected_legacy_bid_match_task("bid:match-route"));
        assert_ne!(crate::queue_for("bid:match"), domain::QUEUE_DEFAULT);
        assert_ne!(crate::queue_for("bid:match-route"), domain::QUEUE_DEFAULT);
        let old = serde_json::json!({
            "job_id": Uuid::from_u128(1),
            "project_id": Uuid::from_u128(2),
            "debounce_key": "same-snapshot",
            "task_type": "bid:match"
        });
        assert!(serde_json::from_value::<BidMatchRouteV1Job>(old).is_err());
    }

    #[test]
    fn bid_extract_pipeline_queues_keep_durable_ids_and_leave_default() {
        let document_id = Uuid::from_u128(1);
        let run_id = Uuid::from_u128(2);
        let convert = BidConvertJob {
            document_id,
            task_type: domain::TYPE_BID_CONVERT.to_string(),
        };
        let extract = BidExtractJob {
            run_id,
            project_id: Uuid::from_u128(4),
            document_id: Some(document_id),
            task_type: domain::TYPE_BID_EXTRACT.to_string(),
        };
        assert_eq!(
            <BidConvertJob as oxana::Job>::unique_id(&convert),
            Some(format!("bid:convert:{document_id}"))
        );
        assert_eq!(
            <BidExtractJob as oxana::Job>::unique_id(&extract),
            Some(format!("bid:extract:{run_id}"))
        );
        match <BidConvertV1Queue as oxana::Queue>::to_config().kind {
            oxana::QueueKind::Static { key } => assert_eq!(key, domain::QUEUE_BID_CONVERT_V1),
            other => panic!("bid-convert-v1 queue must be static, got {other:?}"),
        }
        match <BidExtractV1Queue as oxana::Queue>::to_config().kind {
            oxana::QueueKind::Static { key } => assert_eq!(key, domain::QUEUE_BID_EXTRACT_V1),
            other => panic!("bid-extract-v1 queue must be static, got {other:?}"),
        }
        assert_eq!(
            crate::queue_for(domain::TYPE_BID_CONVERT),
            domain::QUEUE_BID_CONVERT_V1
        );
        assert_eq!(
            crate::queue_for(domain::TYPE_BID_EXTRACT),
            domain::QUEUE_BID_EXTRACT_V1
        );
        assert_ne!(
            crate::queue_for(domain::TYPE_BID_CONVERT),
            domain::QUEUE_DEFAULT
        );
        assert_ne!(crate::queue_for("not-a-real-task"), domain::QUEUE_DEFAULT);
        assert_eq!(crate::queue_for("not-a-real-task"), "rejected:unknown");
    }

    #[test]
    fn max_retry_is_three() {
        assert_eq!(DOCUMENT_PROCESS_MAX_RETRY, 3);
        assert_eq!(DOCUMENT_PROCESS_TIMEOUT_SECS, 2 * 60 * 60);
        assert_eq!(POST_PROCESS_TIMEOUT_SECS, 30 * 60);
    }

    #[test]
    fn redis_pool_refused_is_unreachable() {
        assert!(redis_unreachable(
            "Deadpool Redis pool error: Error occurred while creating a new object: Connection refused (os error 111)"
        ));
        assert!(!redis_unreachable("unique_id conflict"));
    }

    #[test]
    fn runtime_concurrency_reads_env() {
        unsafe { std::env::remove_var("KNOWLEDGEBRAIN_CORE_CONCURRENCY") };
        assert_eq!(runtime_concurrency("CORE", 8), 8);
        unsafe { std::env::set_var("KNOWLEDGEBRAIN_CORE_CONCURRENCY", "16") };
        assert_eq!(runtime_concurrency("CORE", 8), 16);
        unsafe { std::env::set_var("KNOWLEDGEBRAIN_CORE_CONCURRENCY", "0") };
        assert_eq!(runtime_concurrency("CORE", 8), 8);
        unsafe { std::env::remove_var("KNOWLEDGEBRAIN_CORE_CONCURRENCY") };
    }

    #[tokio::test]
    async fn version_clone_lands_on_low_queue() {
        let Some(storage) = redis_test_storage("version_clone_lands_on_low_queue") else {
            return;
        };
        let _guard = redis_test_lock().await;
        let mut n = 0;
        for _ in 0..5 {
            let src = Uuid::new_v4();
            let dst = Uuid::new_v4();
            let pushed = enqueue_version_clone(src, dst, serde_json::json!([]), false)
                .await
                .expect("enqueue");
            assert!(pushed.is_some());
            n = storage.enqueued_count(LowQueue).await.unwrap();
            if n >= 1 {
                break;
            }
        }
        assert!(n >= 1, "low queue empty");
        storage.wipe_queue(LowQueue).await.expect("wipe low queue");
    }

    #[tokio::test]
    async fn index_delete_lands_on_low_queue() {
        let Some(storage) = redis_test_storage("index_delete_lands_on_low_queue") else {
            return;
        };
        let _guard = redis_test_lock().await;
        let mut n = 0;
        for _ in 0..5 {
            let did = Uuid::new_v4();
            let pushed = enqueue_index_delete(did).await.expect("enqueue");
            assert!(pushed.is_some());
            n = storage.enqueued_count(LowQueue).await.unwrap();
            if n >= 1 {
                break;
            }
        }
        assert!(n >= 1, "low queue empty after index:delete");
        storage.wipe_queue(LowQueue).await.expect("wipe low queue");
    }

    #[tokio::test]
    async fn enqueue_lands_on_default_queue() {
        let Some(storage) = redis_test_storage("enqueue_lands_on_default_queue") else {
            return;
        };
        let _guard = redis_test_lock().await;
        storage
            .wipe_queue(DefaultQueue)
            .await
            .expect("wipe default queue");
        let before = storage.enqueued_count(DefaultQueue).await.unwrap();
        let doc = Uuid::new_v4();
        let pushed =
            enqueue_document_process_with(doc, Uuid::new_v4(), 1, vec!["p1".into(), "p2".into()])
                .await
                .expect("enqueue");
        assert!(pushed.is_some());
        let after = storage.enqueued_count(DefaultQueue).await.unwrap();
        assert!(after > before, "before={before} after={after}");
        let jobs = storage
            .list_queue_jobs(
                DefaultQueue,
                &oxana::QueueListOpts {
                    count: 50,
                    offset: 0,
                },
            )
            .await
            .unwrap();
        let blob = format!("{jobs:?}");
        assert!(
            blob.contains(&doc.to_string()) || blob.contains("document:process"),
            "job missing on default queue: {blob}"
        );
        assert!(blob.contains("p1") && blob.contains("p2"), "{blob}");
        storage
            .wipe_queue(DefaultQueue)
            .await
            .expect("wipe default queue");
    }

    #[tokio::test]
    async fn wiki_ingest_lands_on_wiki_queue() {
        let Some(storage) = redis_test_storage("wiki_ingest_lands_on_wiki_queue") else {
            return;
        };
        let _guard = redis_test_lock().await;
        storage
            .wipe_queue(WikiQueue)
            .await
            .expect("wipe wiki queue");
        let vid = Uuid::new_v4();
        let opts = oxana::QueueListOpts {
            count: 500,
            offset: 0,
        };
        let mut found = false;
        for _ in 0..5 {
            let pushed = enqueue_wiki_ingest(vid).await.expect("enqueue");
            assert!(pushed.is_some());
            let scheduled = storage.list_scheduled(&opts).await.unwrap();
            if scheduled
                .iter()
                .any(|j| format!("{j:?}").contains(&vid.to_string()))
            {
                found = true;
                break;
            }
        }
        assert!(found, "wiki ingest job missing from scheduled set");
        let _ = enqueue_wiki_ingest(vid).await.expect("coalesce");
        let scheduled2 = storage.list_scheduled(&opts).await.unwrap();
        let hits = scheduled2
            .iter()
            .filter(|j| format!("{j:?}").contains(&vid.to_string()))
            .count();
        assert_eq!(hits, 1, "unique_id Skip should coalesce ingest debounce");
        storage
            .wipe_queue(WikiQueue)
            .await
            .expect("wipe wiki queue");
    }

    #[tokio::test]
    async fn bid_convert_lands_on_bid_convert_v1_not_default() {
        let Some(storage) = redis_test_storage("bid_convert_lands_on_bid_convert_v1_not_default")
        else {
            return;
        };
        let _guard = redis_test_lock().await;
        storage
            .wipe_queue(DefaultQueue)
            .await
            .expect("wipe default queue");
        storage
            .wipe_queue(BidConvertV1Queue)
            .await
            .expect("wipe bid-convert-v1");
        storage
            .wipe_queue(BidExtractV1Queue)
            .await
            .expect("wipe bid-extract-v1");
        let before_default = storage.enqueued_count(DefaultQueue).await.unwrap();
        let convert_id = Uuid::new_v4();
        let extract_id = Uuid::new_v4();
        assert!(
            enqueue_bid_convert(convert_id)
                .await
                .expect("convert")
                .is_some()
        );
        assert!(
            enqueue_bid_extract(extract_id, Uuid::new_v4(), Some(convert_id))
                .await
                .expect("extract")
                .is_some()
        );
        assert_eq!(
            storage.enqueued_count(DefaultQueue).await.unwrap(),
            before_default,
            "bid extract-pipeline jobs must not land on default"
        );
        assert!(storage.enqueued_count(BidConvertV1Queue).await.unwrap() >= 1);
        assert!(storage.enqueued_count(BidExtractV1Queue).await.unwrap() >= 1);
        storage
            .wipe_queue(BidConvertV1Queue)
            .await
            .expect("wipe bid-convert-v1");
        storage
            .wipe_queue(BidExtractV1Queue)
            .await
            .expect("wipe bid-extract-v1");
    }

    #[tokio::test]
    async fn declared_disabled_enqueues_return_ok_none_without_increasing_queue_counts() {
        let Some(storage) = redis_live_if_up(
            "declared_disabled_enqueues_return_ok_none_without_increasing_queue_counts",
        )
        .await
        else {
            return;
        };
        let _guard = redis_test_lock().await;
        let before_multimodal = storage.enqueued_count(MultimodalQueue).await.unwrap();
        let before_graph = storage.enqueued_count(GraphQueue).await.unwrap();
        let before_question = storage.enqueued_count(QuestionQueue).await.unwrap();
        let before_default = storage.enqueued_count(DefaultQueue).await.unwrap();

        assert_eq!(
            enqueue_image_multimodal(Uuid::new_v4(), "k", "page_image", true, true, 1)
                .await
                .expect("multimodal enqueue result"),
            None
        );
        assert_eq!(
            enqueue_extract(Uuid::new_v4(), Uuid::new_v4(), 1)
                .await
                .expect("extract enqueue result"),
            None
        );
        assert_eq!(
            enqueue_question(Uuid::new_v4(), vec![Uuid::new_v4()], 1, 0)
                .await
                .expect("question enqueue result"),
            None
        );

        assert_eq!(
            storage.enqueued_count(MultimodalQueue).await.unwrap(),
            before_multimodal
        );
        assert_eq!(
            storage.enqueued_count(GraphQueue).await.unwrap(),
            before_graph
        );
        assert_eq!(
            storage.enqueued_count(QuestionQueue).await.unwrap(),
            before_question
        );
        assert_eq!(
            storage.enqueued_count(DefaultQueue).await.unwrap(),
            before_default,
            "declared_disabled enqueues must not spill onto default"
        );

        for task_type in [
            domain::TYPE_BID_CONVERT,
            domain::TYPE_BID_EXTRACT,
            domain::TYPE_BID_MATCH_ROUTE_V1,
            domain::TYPE_BID_RENDER_SUBMISSION_V1,
        ] {
            let mapped = crate::queue_for(task_type);
            assert_ne!(mapped, domain::QUEUE_DEFAULT);
            assert!(!mapped.starts_with("rejected:"));
        }
    }
}
