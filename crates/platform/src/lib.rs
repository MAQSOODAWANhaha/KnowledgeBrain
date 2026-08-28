//! Shared platform: auth, queue transport, Postgres pool, objects, and observability.

mod auth;
mod bid_authoring_contract;
mod blobs;
mod db;
mod env;
mod image_lock;
mod jobs;
mod object_registry;
mod probe;
mod queue_registry;
mod s3;
mod topology_inputs;
mod work_transport;

pub use auth::*;
pub use bid_authoring_contract::*;
pub use blobs::*;
pub use db::*;
pub use env::*;
pub use image_lock::*;
pub use jobs::*;
pub use object_registry::*;
pub use probe::*;
pub use queue_registry::*;
pub use s3::{configured as s3_configured, get_object, put_object};
pub use topology_inputs::*;
pub use work_transport::*;

pub const TYPE_DOCUMENT_PROCESS: &str = "document:process";
pub const TYPE_MANUAL_PROCESS: &str = "manual:process";
pub const TYPE_POST_PROCESS: &str = "knowledge:post_process";
pub const TYPE_SEMANTIC_INDEX_V2: &str = "knowledge:semantic_index:v2";
pub const TYPE_SUMMARY: &str = "summary:generation";
pub const TYPE_QUESTION: &str = "question:generation";
pub const TYPE_IMAGE_MULTIMODAL: &str = "image:multimodal";
pub const TYPE_CHUNK_EXTRACT: &str = "chunk:extract";
pub const TYPE_WIKI_INGEST: &str = "wiki:ingest";
pub const TYPE_WIKI_FINALIZE: &str = "wiki:finalize";
pub const TYPE_VERSION_CLONE: &str = "version:clone";
pub const TYPE_KB_DELETE: &str = "kb:delete";
pub const TYPE_LIST_DELETE: &str = "knowledge:list_delete";
pub const TYPE_LIST_REPARSE: &str = "knowledge:list_reparse";
pub const TYPE_INDEX_DELETE: &str = "index:delete";
pub const TYPE_DATATABLE: &str = "datatable:summary";
pub const TYPE_BID_DELIVERY_V1: &str = "bid:delivery:v1";

pub const QUEUE_DEFAULT: &str = "default";
pub const QUEUE_POSTPROCESS: &str = "postprocess";
pub const QUEUE_SUMMARY: &str = "summary";
pub const QUEUE_MULTIMODAL: &str = "multimodal";
pub const QUEUE_GRAPH: &str = "graph";
pub const QUEUE_QUESTION: &str = "question";
pub const QUEUE_WIKI: &str = "wiki";
pub const QUEUE_LOW: &str = "low";
pub const QUEUE_BID_DELIVERY_V1: &str = "bid-delivery-v1";

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

fn declared_disabled_task(task_type: &str) -> bool {
    match launch_mode(task_type) {
        Ok(Some(LaunchMode::DeclaredDisabled)) => true,
        Ok(Some(_)) => false,
        Ok(None) | Err(_) => matches!(
            task_type,
            TYPE_IMAGE_MULTIMODAL | TYPE_CHUNK_EXTRACT | TYPE_QUESTION
        ),
    }
}

pub fn queue_for(task_type: &str) -> &'static str {
    if declared_disabled_task(task_type) {
        return "rejected:declared-disabled";
    }
    match task_type {
        TYPE_DOCUMENT_PROCESS | TYPE_MANUAL_PROCESS => QUEUE_DEFAULT,
        TYPE_BID_DELIVERY_V1 => QUEUE_BID_DELIVERY_V1,
        TYPE_POST_PROCESS | TYPE_SEMANTIC_INDEX_V2 => QUEUE_POSTPROCESS,
        TYPE_SUMMARY | TYPE_DATATABLE => QUEUE_SUMMARY,
        TYPE_IMAGE_MULTIMODAL => QUEUE_MULTIMODAL,
        TYPE_CHUNK_EXTRACT => QUEUE_GRAPH,
        TYPE_QUESTION => QUEUE_QUESTION,
        TYPE_WIKI_INGEST | TYPE_WIKI_FINALIZE => QUEUE_WIKI,
        TYPE_VERSION_CLONE | TYPE_KB_DELETE | TYPE_LIST_DELETE | TYPE_LIST_REPARSE
        | TYPE_INDEX_DELETE => QUEUE_LOW,
        _ => "rejected:unknown",
    }
}

pub const HOUSEKEEP_STALE_SECS: i64 = crate::jobs::DOCUMENT_PROCESS_TIMEOUT_SECS as i64 + 10 * 60;
pub const HOUSEKEEP_EXTRACT_STALE_SECS: i64 = 90;
pub const HOUSEKEEP_CRON: &str = "0 */5 * * * *";

pub fn housekeep_enabled() -> bool {
    match std::env::var("KNOWLEDGEBRAIN_HOUSEKEEPING_ENABLED") {
        Ok(v) => !matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "off" | "no"
        ),
        Err(_) => false,
    }
}

#[cfg(test)]
mod workspace_layout {
    #[test]
    fn workspace_members_are_the_seven_crates() {
        let manifest = include_str!("../../../Cargo.toml");
        for member in [
            "crates/platform",
            "crates/knowledge",
            "crates/docparser",
            "crates/bidding",
            "crates/api",
            "crates/worker",
            "crates/retention",
        ] {
            assert!(
                manifest.contains(&format!("\"{member}\"")),
                "missing workspace member {member}"
            );
        }
        for retired in [
            "crates/domain",
            "crates/storage",
            "crates/runtime",
            "crates/auth",
            "crates/bid",
        ] {
            assert!(
                !manifest.contains(&format!("\"{retired}\"")),
                "retired crate still a workspace member: {retired}"
            );
        }
    }
}
