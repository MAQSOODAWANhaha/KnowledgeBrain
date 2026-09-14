//! Knowledge assets, ingest pipeline, and retrieval.

#[cfg(test)]
pub(crate) static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
#[cfg(test)]
pub(crate) static TEST_PG_SERIAL: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

pub mod catalog;
pub mod chunker;
pub mod clone;
pub mod enrichment;
pub mod graph;
pub mod identity;
pub mod index;
pub mod models;
pub mod search;
pub mod wiki;

mod formula;
pub mod job;
pub mod obs;
mod persistence_api;
mod process;
mod status;
mod store;

pub mod ingest;
pub mod knowledge_index_v2;
pub mod knowledge_retrieval;
pub mod knowledge_retrieval_pg;
pub mod pipeline;
pub use knowledge_retrieval_pg::PostgresKnowledgeRetrievalAdapter;

pub use catalog::{
    NewApiKey, NewDocument, NewIngestDocument, PersistError, PgGraphHit, PgSearchHit,
    SeededWorkspace, SpanRow, VersionConfig, append_document_chunks, bump_document_attempt,
    cancel_active_docs_for_versions, cancel_dependent_stages, clear_product_current_if,
    column_names, company_workspace_id, copy_document_index, create_workspace_with_library,
    current_summary_model, delete_api_key, delete_chunks_by_types, delete_empty_product,
    delete_graph_for_document, delete_image_chunks, delete_member, delete_product, delete_tag,
    document_chunk_count, document_file_meta, document_image_object_refs, document_parse_status,
    document_tag_ids, document_workspace_id, embedding_models_for_versions,
    embeddings_schema_ready, ensure_company_workspace, finalize_subtask, find_api_key_by_hash,
    find_duplicate_document, find_user_by_email, find_user_by_id, finish_span,
    freeze_version_embedding_model, graph_hits_pg, housekeep_documents, hybrid_search_pg,
    insert_api_key, insert_document, insert_document_chunks, insert_document_tags,
    insert_ingest_document, insert_member, insert_product, insert_tag, insert_user, insert_version,
    insert_version_cloning, insert_workspace, insert_workspace_kind, is_frozen_default_library,
    is_unique_violation, latest_span_attempt, list_documents_in_version,
    list_members_for_workspace, list_models, list_products_in_workspace, list_spans,
    list_spans_attempt, list_versions_for_product, list_wiki_folders, list_workspace_ids,
    list_workspaces, load_document, load_document_chunks, load_product, load_version,
    load_workspace, mark_reparse_queued, open_attempt, persist_graph_maps, persist_question_maps,
    persist_summary_maps, persist_wiki_changes_atomic, product_name, product_slug_taken,
    product_workspace_id, purge_document_index, replace_document_chunks,
    replace_document_embeddings, replace_document_tags, replace_wiki_page_chunks,
    resolve_pg_assembly_targets, resolve_product_version_id, retire_workspace,
    set_document_description, set_document_progress, set_document_source, set_finalizing,
    set_index_ready, set_parse_status, set_process_overrides, set_product_current,
    set_retrieval_config, set_summary_status, set_version_status, skip_span, start_span,
    table_names, try_set_processing, update_product_name, update_user_email, update_version_config,
    update_workspace_name, upsert_member, upsert_model, upsert_span, upsert_wiki_folder,
    upsert_wiki_page, vector_literal, version_exists, version_has_chunk_embeddings,
    version_ids_for_product, version_ids_for_workspace, version_indexing_flags,
    version_label_taken, version_multimodal_enabled, version_references_object,
    version_wiki_enabled, version_workspace_id, workspace_embedding_conflict,
    workspace_thresholds_for_product, workspace_thresholds_for_version,
    workspace_top_k_for_version, workspaces_for_user,
};
pub use formula::expected_subtasks;
pub use job::{DocJob, WikiJob};
pub use knowledge_retrieval::*;
pub use persistence_api::{
    list_api_keys_for_workspace, list_tags_for_workspace, list_wiki_pages, load_api_key, load_tag,
    load_wiki_page, tags_belong_to_workspace, tags_in_workspace, workspace_slug_taken,
};
pub use process::{
    AsrOverride, ChunkingOverride, EffectiveProcessConfig, ExtractOverride, ParserEngineRule,
    ProcessOverrides, QuestionOverride, default_parser_engine_rules, engine_for_file,
    parser_engine_for, resolve_parser_engine, resolve_process_config,
};
pub use status::{ParseStatus, ProductKind, Role, SummaryStatus, VersionStatus, WorkspaceKind};
pub use store::{
    ApiKey, Chunk, ChunkEmbedding, DeadLetter, Document, GraphNode, GraphRelation, Job, Product,
    ProductVersion, RetrievalConfig, Span, Tag, User, WikiFolder, WikiPage, WikiPendingOp,
    Workspace,
};

/// Return an object identity only after the configured stores accept its bytes.
pub async fn put_bytes(bytes: &[u8]) -> std::io::Result<(String, String)> {
    let hash = platform::sha256_hex(bytes);
    let reference = platform::object_ref(&hash);
    platform::write_blob_async(&hash, bytes).await?;
    Ok((hash, reference))
}
