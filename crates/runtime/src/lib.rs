//! Queue names, pool labels, housekeeping, and oxana enqueue.

mod jobs;
pub use jobs::*;

use chrono::{Duration, Utc};
use domain::{
    ParseStatus, QUEUE_DEFAULT, QUEUE_GRAPH, QUEUE_LOW, QUEUE_MULTIMODAL, QUEUE_POSTPROCESS,
    QUEUE_QUESTION, QUEUE_SUMMARY, QUEUE_WIKI, Store,
};

pub const POOL_CORE: &str = "core";
pub const POOL_POSTPROCESS: &str = "postprocess";
pub const POOL_ENRICHMENT: &str = "enrichment";
pub const POOL_MAINTENANCE: &str = "maintenance";
pub const POOL_SHARED: &str = "shared";
pub const POOL_WIKI: &str = "wiki";

pub fn queue_for(task_type: &str) -> &'static str {
    match task_type {
        domain::TYPE_DOCUMENT_PROCESS | domain::TYPE_MANUAL_PROCESS => QUEUE_DEFAULT,
        domain::TYPE_POST_PROCESS => QUEUE_POSTPROCESS,
        domain::TYPE_SUMMARY | domain::TYPE_DATATABLE => QUEUE_SUMMARY,
        domain::TYPE_IMAGE_MULTIMODAL => QUEUE_MULTIMODAL,
        domain::TYPE_CHUNK_EXTRACT => QUEUE_GRAPH,
        domain::TYPE_QUESTION => QUEUE_QUESTION,
        domain::TYPE_WIKI_INGEST | domain::TYPE_WIKI_FINALIZE => QUEUE_WIKI,
        domain::TYPE_VERSION_CLONE
        | domain::TYPE_KB_DELETE
        | domain::TYPE_LIST_DELETE
        | domain::TYPE_LIST_REPARSE
        | domain::TYPE_INDEX_DELETE => QUEUE_LOW,
        _ => QUEUE_DEFAULT,
    }
}

/// DocumentProcessTimeout (2h) + 10m buffer. Brain `staleThreshold`.
pub const HOUSEKEEP_STALE_SECS: i64 = DOCUMENT_PROCESS_TIMEOUT_SECS as i64 + 10 * 60;
pub const HOUSEKEEP_CRON: &str = "0 */5 * * * *";

pub fn housekeep_stale() -> Duration {
    Duration::seconds(HOUSEKEEP_STALE_SECS)
}

pub fn housekeep_enabled() -> bool {
    match std::env::var("KNOWLEDGEBRAIN_HOUSEKEEPING_ENABLED") {
        Ok(v) => !matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "off" | "no"
        ),
        Err(_) => true,
    }
}

/// Fail pending/processing/finalizing rows older than `timeout` with no span heartbeat.
pub fn housekeep(store: &mut Store, timeout: Duration) {
    let cutoff = Utc::now() - timeout;
    let stale: Vec<_> = store
        .documents
        .values()
        .filter(|d| {
            matches!(
                d.parse_status,
                ParseStatus::Processing | ParseStatus::Finalizing
            ) && last_heartbeat(store, d) < cutoff
        })
        .map(|d| d.id)
        .collect();
    for id in stale {
        store.fail_document(
            id,
            &format!("task stuck in processing > {timeout}, recovered by housekeeping"),
        );
    }
}

fn last_heartbeat(store: &Store, d: &domain::Document) -> chrono::DateTime<Utc> {
    let mut beat = d.updated_at;
    for s in store.spans.iter().filter(|s| s.document_id == d.id) {
        if s.started_at > beat {
            beat = s.started_at;
        }
        if let Some(f) = s.finished_at
            && f > beat
        {
            beat = f;
        }
    }
    beat
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn brain_queue_keys() {
        assert_eq!(queue_for(domain::TYPE_DOCUMENT_PROCESS), "default");
        assert_eq!(queue_for(domain::TYPE_POST_PROCESS), "postprocess");
        assert_eq!(queue_for(domain::TYPE_KB_DELETE), "low");
        assert_eq!(queue_for(domain::TYPE_WIKI_INGEST), "wiki");
        assert_eq!(POOL_SHARED, "shared");
        assert_eq!(HOUSEKEEP_CRON, "0 */5 * * * *");
        assert_eq!(HOUSEKEEP_STALE_SECS, 2 * 3600 + 10 * 60);
    }

    #[test]
    fn housekeep_skips_fresh_span_fails_stale() {
        use domain::Document;
        let mut store = Store::default();
        let old = Document::new(
            Uuid::new_v4(),
            "t".into(),
            "a.txt".into(),
            1,
            "h".into(),
            "k".into(),
        );
        let mut stale = old.clone();
        stale.id = Uuid::new_v4();
        stale.parse_status = ParseStatus::Processing;
        stale.updated_at = Utc::now() - Duration::hours(3);
        let mut live = old;
        live.id = Uuid::new_v4();
        live.parse_status = ParseStatus::Processing;
        live.updated_at = Utc::now() - Duration::hours(3);
        store.spans.push(domain::Span {
            span_id: Uuid::new_v4(),
            document_id: live.id,
            attempt: 1,
            name: "docreader".into(),
            parent_span_id: None,
            kind: "docreader".into(),
            status: "ok".into(),
            output: None,
            error_message: String::new(),
            started_at: Utc::now(),
            finished_at: Some(Utc::now()),
            duration_ms: Some(1),
        });
        let sid = stale.id;
        let lid = live.id;
        store.documents.insert(sid, stale);
        store.documents.insert(lid, live);
        housekeep(&mut store, housekeep_stale());
        assert_eq!(store.documents[&sid].parse_status, ParseStatus::Failed);
        assert_eq!(store.documents[&lid].parse_status, ParseStatus::Processing);
    }

    #[test]
    fn housekeep_leaves_stale_pending() {
        use domain::Document;
        let mut store = Store::default();
        let mut pending = Document::new(
            Uuid::new_v4(),
            "t".into(),
            "a.txt".into(),
            1,
            "h".into(),
            "k".into(),
        );
        pending.parse_status = ParseStatus::Pending;
        pending.updated_at = Utc::now() - Duration::hours(3);
        let id = pending.id;
        store.documents.insert(id, pending);
        housekeep(&mut store, housekeep_stale());
        assert_eq!(store.documents[&id].parse_status, ParseStatus::Pending);
    }
}
