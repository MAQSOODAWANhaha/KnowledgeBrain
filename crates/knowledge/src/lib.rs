//! Knowledge assets, ingest pipeline, and retrieval.

pub mod chunker;
pub mod clone;
pub mod enrichment;
pub mod graph;
pub mod index;
pub mod models;
pub mod search;
pub mod wiki;

mod formula;
pub mod job;
pub mod obs;
mod persist;
mod persist_api;
mod process;
mod status;
mod store;

pub mod knowledge_index_v2;
pub mod knowledge_retrieval;
pub mod knowledge_retrieval_pg;
pub mod pipeline;
pub use knowledge_retrieval_pg::PostgresKnowledgeRetrievalAdapter;

pub use formula::*;
pub use job::{DocJob, WikiJob};
pub use knowledge_retrieval::*;
pub use persist::*;
pub use persist_api::*;
pub use process::*;
pub use status::*;
pub use store::*;

pub use platform::{
    LaunchMode, LiveBody, QUEUE_DEFAULT, QUEUE_GRAPH, QUEUE_LOW, QUEUE_MULTIMODAL,
    QUEUE_POSTPROCESS, QUEUE_QUESTION, QUEUE_SUMMARY, QUEUE_WIKI, QueueRegistry, ReadyBody,
    TYPE_CHUNK_EXTRACT, TYPE_DATATABLE, TYPE_DOCUMENT_PROCESS, TYPE_IMAGE_MULTIMODAL,
    TYPE_INDEX_DELETE, TYPE_KB_DELETE, TYPE_LIST_DELETE, TYPE_LIST_REPARSE, TYPE_MANUAL_PROCESS,
    TYPE_POST_PROCESS, TYPE_QUESTION, TYPE_SEMANTIC_INDEX_V2, TYPE_SUMMARY, TYPE_VERSION_CLONE,
    TYPE_WIKI_FINALIZE, TYPE_WIKI_INGEST, chat_api_key, chat_base_url, chat_model, check_readiness,
    embedding_api_key, embedding_base_url, embedding_model, first_env, is_audio_type,
    is_image_type, is_simple_format, is_valid_file_type, is_video, launch_mode, live_body,
    max_file_bytes, new_id, ready_body, sha256_hex, vlm_api_key, vlm_base_url, vlm_configured,
    vlm_endpoint_ready, vlm_model,
};

/// Disk write plus in-memory object map. Production bytes still go through platform blobs.
pub fn put(store: &mut Store, bytes: &[u8]) -> (String, String) {
    let (hash, reference) = put_bytes(bytes);
    store.objects.insert(reference.clone(), bytes.to_vec());
    (hash, reference)
}

pub fn put_bytes(bytes: &[u8]) -> (String, String) {
    let hash = sha256_hex(bytes);
    let reference = platform::object_ref(&hash);
    let _ = platform::write_blob_off_runtime(&hash, bytes);
    (hash, reference)
}

pub fn discard_unpersisted_object(store: &mut Store, hash: &str) {
    store.objects.remove(&platform::object_ref(hash));
}
