//! Queue names, pool labels, housekeeping, and oxana enqueue.

mod jobs;
pub use jobs::*;
mod lease;
pub use lease::*;

use chrono::{Duration, Utc};
use domain::{
    ParseStatus, QUEUE_BID_CONVERT_V1, QUEUE_BID_EXTRACT_V1, QUEUE_BID_MATCHING_V1,
    QUEUE_BID_RENDER_V1, QUEUE_DEFAULT, QUEUE_GRAPH, QUEUE_LOW, QUEUE_MULTIMODAL,
    QUEUE_POSTPROCESS, QUEUE_QUESTION, QUEUE_SUMMARY, QUEUE_WIKI, Store,
};

pub const POOL_CORE: &str = "core";
pub const POOL_POSTPROCESS: &str = "postprocess";
pub const POOL_ENRICHMENT: &str = "enrichment";
pub const POOL_MAINTENANCE: &str = "maintenance";
pub const POOL_SHARED: &str = "shared";
pub const POOL_WIKI: &str = "wiki";

/// stdout + `RUST_LOG` (default `info`). Safe to call once from api/worker.
pub fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let ansi = std::env::var_os("NO_COLOR").is_none()
        && std::env::var("TERM").is_ok_and(|term| term != "dumb");
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_ansi(ansi)
        .with_writer(std::io::stdout)
        .try_init();
}

pub fn rejected_legacy_bid_match_task(task_type: &str) -> bool {
    matches!(task_type, "bid:match" | "bid:match-route")
        || (task_type.starts_with("bid:match") && task_type != domain::TYPE_BID_MATCH_ROUTE_V1)
}

fn declared_disabled_task(task_type: &str) -> bool {
    match domain::launch_mode(task_type) {
        Ok(Some(domain::LaunchMode::DeclaredDisabled)) => true,
        Ok(Some(_)) => false,
        Ok(None) | Err(_) => matches!(
            task_type,
            domain::TYPE_IMAGE_MULTIMODAL | domain::TYPE_CHUNK_EXTRACT | domain::TYPE_QUESTION
        ),
    }
}

pub fn queue_for(task_type: &str) -> &'static str {
    if rejected_legacy_bid_match_task(task_type) {
        return "rejected:bid-match";
    }
    if declared_disabled_task(task_type) {
        return "rejected:declared-disabled";
    }
    match task_type {
        domain::TYPE_DOCUMENT_PROCESS | domain::TYPE_MANUAL_PROCESS => QUEUE_DEFAULT,
        domain::TYPE_BID_CONVERT | domain::TYPE_BID_PREPARE_ATTACHMENT_V1 => QUEUE_BID_CONVERT_V1,
        domain::TYPE_BID_EXTRACT => QUEUE_BID_EXTRACT_V1,
        domain::TYPE_BID_MATCH_ROUTE_V1 => QUEUE_BID_MATCHING_V1,
        domain::TYPE_BID_RENDER_SUBMISSION_V1 => QUEUE_BID_RENDER_V1,
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
        _ => "rejected:unknown",
    }
}

/// DocumentProcessTimeout (2h) + 10m buffer. Brain `staleThreshold`.
pub const HOUSEKEEP_STALE_SECS: i64 = DOCUMENT_PROCESS_TIMEOUT_SECS as i64 + 10 * 60;
/// Bid extract heartbeats every 30s; three missed beats ≈ 90s.
pub const HOUSEKEEP_EXTRACT_STALE_SECS: i64 = 90;
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
        Err(_) => false,
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
        assert_eq!(queue_for(domain::TYPE_MANUAL_PROCESS), "default");
        assert_eq!(
            queue_for(domain::TYPE_BID_CONVERT),
            domain::QUEUE_BID_CONVERT_V1
        );
        assert_eq!(
            queue_for(domain::TYPE_BID_PREPARE_ATTACHMENT_V1),
            domain::QUEUE_BID_CONVERT_V1
        );
        assert_eq!(
            queue_for(domain::TYPE_BID_EXTRACT),
            domain::QUEUE_BID_EXTRACT_V1
        );
        assert_eq!(
            queue_for(domain::TYPE_BID_MATCH_ROUTE_V1),
            domain::QUEUE_BID_MATCHING_V1
        );
        assert_eq!(
            queue_for(domain::TYPE_BID_RENDER_SUBMISSION_V1),
            domain::QUEUE_BID_RENDER_V1
        );
        assert_eq!(queue_for(domain::TYPE_POST_PROCESS), "postprocess");
        assert_eq!(queue_for(domain::TYPE_KB_DELETE), "low");
        assert_eq!(queue_for(domain::TYPE_WIKI_INGEST), "wiki");
        assert_ne!(queue_for(domain::TYPE_BID_CONVERT), domain::QUEUE_DEFAULT);
        assert_ne!(queue_for("not-a-real-task"), domain::QUEUE_DEFAULT);
        assert_eq!(queue_for("not-a-real-task"), "rejected:unknown");
        assert_eq!(queue_for("bid:match"), "rejected:bid-match");
        assert_eq!(
            queue_for(domain::TYPE_IMAGE_MULTIMODAL),
            "rejected:declared-disabled"
        );
        assert_eq!(
            queue_for(domain::TYPE_CHUNK_EXTRACT),
            "rejected:declared-disabled"
        );
        assert_eq!(
            queue_for(domain::TYPE_QUESTION),
            "rejected:declared-disabled"
        );
        assert_ne!(
            queue_for(domain::TYPE_IMAGE_MULTIMODAL),
            domain::QUEUE_DEFAULT
        );
        assert_ne!(queue_for(domain::TYPE_CHUNK_EXTRACT), domain::QUEUE_DEFAULT);
        assert_ne!(queue_for(domain::TYPE_QUESTION), domain::QUEUE_DEFAULT);
        assert_eq!(POOL_SHARED, "shared");
        assert_eq!(HOUSEKEEP_CRON, "0 */5 * * * *");
        assert_eq!(HOUSEKEEP_STALE_SECS, 2 * 3600 + 10 * 60);
        assert_eq!(HOUSEKEEP_EXTRACT_STALE_SECS, 90);
    }

    #[test]
    fn implemented_bid_queue_registry_static_equality() {
        let registry = domain::QueueRegistry::load().expect("queue registry");
        for entry in registry.entries() {
            if entry.launch_mode == domain::LaunchMode::RequiredEnabled
                && entry.task_type.starts_with("bid:")
            {
                assert_eq!(
                    queue_for(&entry.task_type),
                    entry.physical_queue.as_str(),
                    "required_enabled Bid task {} must map to registry physical_queue",
                    entry.task_type
                );
                assert_ne!(queue_for(&entry.task_type), domain::QUEUE_DEFAULT);
            }
        }

        for legacy in ["bid:match", "bid:match-route", "bid:match-old"] {
            let mapped = queue_for(legacy);
            assert!(
                mapped.starts_with("rejected:"),
                "legacy {legacy} must be rejected:*, got {mapped}"
            );
            assert_ne!(mapped, domain::QUEUE_DEFAULT);
        }

        let task_types: std::collections::BTreeSet<_> = registry
            .entries()
            .iter()
            .map(|entry| entry.task_type.as_str())
            .collect();
        for constant in [
            domain::TYPE_BID_CONVERT,
            domain::TYPE_BID_PREPARE_ATTACHMENT_V1,
            domain::TYPE_BID_EXTRACT,
            domain::TYPE_BID_MATCH_ROUTE_V1,
            domain::TYPE_BID_RENDER_SUBMISSION_V1,
        ] {
            assert!(
                task_types.contains(constant),
                "registry must include domain constant {constant}"
            );
        }
    }

    #[test]
    fn queue_for_declared_disabled_registry_tasks() {
        let registry = domain::QueueRegistry::load().expect("deploy/queue-registry.toml");
        let mut disabled_queues: Vec<&str> = Vec::new();
        for entry in registry.entries() {
            if entry.launch_mode != domain::LaunchMode::DeclaredDisabled {
                continue;
            }
            assert_eq!(
                queue_for(&entry.task_type),
                "rejected:declared-disabled",
                "declared_disabled task {} must not map to a physical queue",
                entry.task_type
            );
            disabled_queues.push(entry.physical_queue.as_str());
        }
        disabled_queues.sort_unstable();
        disabled_queues.dedup();
        assert_eq!(disabled_queues, ["graph", "multimodal", "question"]);
        for task_type in registry.declared_disabled_tasks() {
            assert_eq!(queue_for(task_type), "rejected:declared-disabled");
        }
    }

    #[test]
    fn housekeep_enabled_defaults_off_when_unset() {
        let previous = std::env::var("KNOWLEDGEBRAIN_HOUSEKEEPING_ENABLED").ok();
        unsafe {
            std::env::remove_var("KNOWLEDGEBRAIN_HOUSEKEEPING_ENABLED");
        }
        assert!(
            !housekeep_enabled(),
            "maintenance_only housekeep must stay off when the env is unset"
        );
        if let Some(value) = previous {
            unsafe {
                std::env::set_var("KNOWLEDGEBRAIN_HOUSEKEEPING_ENABLED", value);
            }
        }
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
