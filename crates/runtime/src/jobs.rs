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

#[derive(oxana::Queue)]
#[oxana(key = "sync", concurrency = Dynamic(4))]
pub struct SyncQueue;

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

#[derive(Debug, Clone, Serialize, Deserialize, oxana::Job)]
#[oxana(unique_id = "bid:section-retry:{job_id}", on_conflict = Skip)]
pub struct BidSectionRetryJob {
    pub job_id: Uuid,
    pub project_id: Uuid,
    pub section_id: Uuid,
    pub task_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, oxana::Job)]
#[oxana(unique_id = "bid:match:{project_id}:{debounce_key}", on_conflict = Skip)]
pub struct BidMatchOxanaJob {
    pub job_id: Uuid,
    pub project_id: Uuid,
    pub debounce_key: String,
    pub task_type: String,
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
        .queue::<SyncQueue>()
        .queue::<SummaryQueue>()
        .queue::<QuestionQueue>()
        .queue::<GraphQueue>()
        .queue::<MultimodalQueue>()
        .queue::<WikiQueue>();
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
    oxana_id(
        storage
            .enqueue(
                DefaultQueue,
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
    oxana_id(
        storage
            .enqueue(
                DefaultQueue,
                BidExtractJob {
                    run_id,
                    project_id,
                    document_id,
                    task_type: domain::TYPE_BID_EXTRACT.to_string(),
                },
            )
            .await,
    )
}

pub async fn enqueue_bid_section_retry(
    job_id: Uuid,
    project_id: Uuid,
    section_id: Uuid,
) -> Result<Option<String>, String> {
    let Ok(storage) = connect() else {
        return Ok(None);
    };
    oxana_id(
        storage
            .enqueue(
                DefaultQueue,
                BidSectionRetryJob {
                    job_id,
                    project_id,
                    section_id,
                    task_type: domain::TYPE_BID_SECTION_RETRY.to_string(),
                },
            )
            .await,
    )
}

pub async fn enqueue_bid_match(
    job_id: Uuid,
    project_id: Uuid,
    debounce_key: String,
) -> Result<Option<String>, String> {
    let Ok(storage) = connect() else {
        return Ok(None);
    };
    oxana_id(
        storage
            .enqueue(
                DefaultQueue,
                BidMatchOxanaJob {
                    job_id,
                    project_id,
                    debounce_key,
                    task_type: domain::TYPE_BID_MATCH.to_string(),
                },
            )
            .await,
    )
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
        let Ok(storage) = connect() else {
            eprintln!("skip: redis not reachable");
            return;
        };
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
        let _ = storage.wipe_queue(LowQueue).await;
    }

    #[tokio::test]
    async fn index_delete_lands_on_low_queue() {
        let Ok(storage) = connect() else {
            eprintln!("skip: redis not reachable");
            return;
        };
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
        let _ = storage.wipe_queue(LowQueue).await;
    }

    #[tokio::test]
    async fn enqueue_lands_on_default_queue() {
        let Ok(storage) = connect() else {
            eprintln!("skip: redis not reachable");
            return;
        };
        let _ = storage.wipe_queue(DefaultQueue).await;
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
        let _ = storage.wipe_queue(DefaultQueue).await;
    }

    #[tokio::test]
    async fn wiki_ingest_lands_on_wiki_queue() {
        let Ok(storage) = connect() else {
            eprintln!("skip: redis not reachable");
            return;
        };
        let _ = storage.wipe_queue(WikiQueue).await;
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
        let _ = storage.wipe_queue(WikiQueue).await;
    }
}
