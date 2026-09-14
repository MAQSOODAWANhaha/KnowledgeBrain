//! Document-scoped working set for enrichment/graph jobs. Not the process catalog.

use crate::{
    Chunk, ChunkEmbedding, DeadLetter, Document, GraphNode, GraphRelation, Job, ParseStatus,
    ProductVersion, WikiFolder, WikiPage, WikiPendingOp,
};
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use std::collections::{HashMap, VecDeque};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct DocJob {
    pub document: Document,
    pub version: ProductVersion,
    pub chunks: HashMap<Uuid, Chunk>,
    pub embeddings: HashMap<Uuid, ChunkEmbedding>,
    pub graph: HashMap<(Uuid, Uuid, String), GraphNode>,
    pub relations: HashMap<(Uuid, Uuid, String, String, String), GraphRelation>,
}

impl DocJob {
    pub async fn from_pool(pool: &PgPool, document_id: Uuid) -> Result<Option<Self>, sqlx::Error> {
        let Some(document) = crate::load_document(pool, document_id).await? else {
            return Ok(None);
        };
        let Some(mut version) = crate::load_version(pool, document.product_version_id).await?
        else {
            return Ok(None);
        };
        crate::resolve_process_config(&version, document.process_overrides.as_ref())
            .apply_to(&mut version);
        let chunk_list = crate::load_document_chunks(pool, document_id).await?;
        let chunks = chunk_list
            .into_iter()
            .map(|chunk| (chunk.id, chunk))
            .collect();
        let (graph, relations) =
            crate::graph::sql::load_graph_for_document(pool, document_id).await?;
        Ok(Some(Self {
            document,
            version,
            chunks,
            embeddings: HashMap::new(),
            graph,
            relations,
        }))
    }

    pub fn for_test(document: Document, mut version: ProductVersion) -> Self {
        crate::resolve_process_config(&version, document.process_overrides.as_ref())
            .apply_to(&mut version);
        Self {
            document,
            version,
            chunks: HashMap::new(),
            embeddings: HashMap::new(),
            graph: HashMap::new(),
            relations: HashMap::new(),
        }
    }

    pub fn upsert_node(&mut self, version_id: Uuid, document_id: Uuid, name: &str, chunk_id: Uuid) {
        let key = (version_id, document_id, name.to_string());
        let node = self.graph.entry(key).or_insert_with(|| GraphNode {
            version_id,
            document_id,
            name: name.to_string(),
            chunk_ids: Vec::new(),
        });
        if !node.chunk_ids.contains(&chunk_id) {
            node.chunk_ids.push(chunk_id);
        }
    }

    pub fn upsert_rel(
        &mut self,
        version_id: Uuid,
        document_id: Uuid,
        node1: &str,
        node2: &str,
        rel_type: &str,
    ) {
        let key = (
            version_id,
            document_id,
            node1.to_string(),
            node2.to_string(),
            rel_type.to_string(),
        );
        self.relations.entry(key).or_insert(GraphRelation {
            version_id,
            document_id,
            node1: node1.to_string(),
            node2: node2.to_string(),
            rel_type: rel_type.to_string(),
        });
    }

    pub fn finalize_subtask(&mut self) {
        if self.document.pending_subtasks_count > 0 {
            self.document.pending_subtasks_count -= 1;
        }
        if self.document.parse_status == ParseStatus::Finalizing
            && self.document.pending_subtasks_count == 0
        {
            self.document.parse_status = ParseStatus::Completed;
        }
    }
}

/// Version-scoped wiki working set.
#[derive(Debug, Clone, Default)]
pub struct WikiJob {
    pub version_id: Uuid,
    pub versions: HashMap<Uuid, ProductVersion>,
    pub documents: HashMap<Uuid, Document>,
    pub chunks: HashMap<Uuid, Chunk>,
    pub embeddings: HashMap<Uuid, ChunkEmbedding>,
    pub graph: HashMap<(Uuid, Uuid, String), GraphNode>,
    pub wiki: HashMap<(Uuid, String), WikiPage>,
    pub wiki_folders: HashMap<Uuid, WikiFolder>,
    pub wiki_ops: Vec<WikiPendingOp>,
    pub wiki_tombstones: HashMap<(Uuid, Uuid), DateTime<Utc>>,
    pub wiki_slug_locks: HashMap<String, DateTime<Utc>>,
    pub wiki_inflight: HashMap<Uuid, DateTime<Utc>>,
    pub wiki_op_seq: i64,
    pub queue: VecDeque<Job>,
    pub dead_letters: Vec<DeadLetter>,
}

impl WikiJob {
    pub async fn from_pool(pool: &PgPool, version_id: Uuid) -> Result<Self, sqlx::Error> {
        let mut versions = HashMap::new();
        if let Some(version) = crate::load_version(pool, version_id).await? {
            versions.insert(version_id, version);
        }
        let documents_list =
            crate::list_documents_in_version(pool, version_id, None, None, None).await?;
        let mut documents = HashMap::new();
        let mut chunks = HashMap::new();
        for document in documents_list {
            for chunk in crate::load_document_chunks(pool, document.id).await? {
                chunks.insert(chunk.id, chunk);
            }
            documents.insert(document.id, document);
        }
        let mut wiki = HashMap::new();
        for page in crate::list_wiki_pages(pool, version_id).await? {
            wiki.insert((page.product_version_id, page.slug.clone()), page);
        }
        let folder_rows = sqlx::query(
            "SELECT id, product_version_id, parent_id, name, path, depth, sort_order
             FROM wiki_folders
             WHERE product_version_id = $1 AND deleted_at IS NULL",
        )
        .bind(version_id)
        .fetch_all(pool)
        .await?;
        let mut wiki_folders = HashMap::new();
        for row in folder_rows {
            let id: Uuid = row.try_get("id")?;
            wiki_folders.insert(
                id,
                crate::WikiFolder {
                    id,
                    product_version_id: row.try_get("product_version_id")?,
                    parent_id: row.try_get("parent_id")?,
                    name: row.try_get("name")?,
                    path: row.try_get("path")?,
                    depth: row.try_get("depth")?,
                    sort_order: row.try_get("sort_order").unwrap_or(0),
                },
            );
        }
        Ok(Self {
            version_id,
            versions,
            documents,
            chunks,
            embeddings: HashMap::new(),
            graph: HashMap::new(),
            wiki,
            wiki_folders,
            wiki_ops: Vec::new(),
            wiki_tombstones: HashMap::new(),
            wiki_slug_locks: HashMap::new(),
            wiki_inflight: HashMap::new(),
            wiki_op_seq: 0,
            queue: VecDeque::new(),
            dead_letters: Vec::new(),
        })
    }

    pub fn finalize_subtask(&mut self, doc_id: Uuid) {
        let Some(d) = self.documents.get_mut(&doc_id) else {
            return;
        };
        if d.pending_subtasks_count > 0 {
            d.pending_subtasks_count -= 1;
        }
        if d.parse_status == ParseStatus::Finalizing && d.pending_subtasks_count == 0 {
            d.parse_status = ParseStatus::Completed;
        }
    }

    pub fn dead_letter(&mut self, task_type: &str, related_id: Uuid, last_error: &str) {
        let msg: String = last_error.chars().take(8 * 1024).collect();
        self.dead_letters.push(DeadLetter {
            task_type: task_type.to_string(),
            related_id,
            last_error: msg,
        });
    }

    pub fn enqueue(&mut self, task_type: &str, queue: &str, payload: serde_json::Value) -> Uuid {
        let id = Uuid::new_v4();
        self.queue.push_back(Job {
            id,
            task_type: task_type.to_string(),
            queue: queue.to_string(),
            payload,
            retries: 0,
            max_retry: 3,
        });
        id
    }
}
