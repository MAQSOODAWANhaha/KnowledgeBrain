//! Document-scoped working set for enrichment/graph jobs. Not the process catalog.

use crate::{
    Chunk, ChunkEmbedding, DeadLetter, Document, GraphNode, GraphRelation, Job, ParseStatus,
    ProductVersion, Store, WikiFolder, WikiPage, WikiPendingOp,
};
use chrono::{DateTime, Utc};
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
    pub fn from_store(store: &Store, document_id: Uuid) -> Option<Self> {
        let document = store.documents.get(&document_id)?.clone();
        let mut version = store.versions.get(&document.product_version_id)?.clone();
        crate::resolve_process_config(&version, document.process_overrides.as_ref())
            .apply_to(&mut version);
        let chunks = store
            .chunks
            .iter()
            .filter(|(_, chunk)| chunk.document_id == document_id)
            .map(|(id, chunk)| (*id, chunk.clone()))
            .collect();
        let embeddings = store
            .embeddings
            .iter()
            .filter(|(_, embedding)| embedding.document_id == document_id)
            .map(|(id, embedding)| (*id, embedding.clone()))
            .collect();
        let graph = store
            .graph
            .iter()
            .filter(|(_, node)| node.document_id == document_id)
            .map(|(key, node)| (key.clone(), node.clone()))
            .collect();
        let relations = store
            .relations
            .iter()
            .filter(|(_, rel)| rel.document_id == document_id)
            .map(|(key, rel)| (key.clone(), rel.clone()))
            .collect();
        Some(Self {
            document,
            version,
            chunks,
            embeddings,
            graph,
            relations,
        })
    }

    pub fn write_back(self, store: &mut Store) {
        let document_id = self.document.id;
        store.versions.insert(self.version.id, self.version);
        store.documents.insert(document_id, self.document);
        store
            .chunks
            .retain(|_, chunk| chunk.document_id != document_id);
        store.chunks.extend(self.chunks);
        store
            .embeddings
            .retain(|_, embedding| embedding.document_id != document_id);
        store.embeddings.extend(self.embeddings);
        store
            .graph
            .retain(|_, node| node.document_id != document_id);
        store.graph.extend(self.graph);
        store
            .relations
            .retain(|_, rel| rel.document_id != document_id);
        store.relations.extend(self.relations);
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
    pub fn from_store(store: &Store, version_id: Uuid) -> Self {
        let documents: HashMap<_, _> = store
            .documents
            .iter()
            .filter(|(_, d)| d.product_version_id == version_id)
            .map(|(k, v)| (*k, v.clone()))
            .collect();
        let doc_ids: std::collections::HashSet<_> = documents.keys().copied().collect();
        let chunks = store
            .chunks
            .iter()
            .filter(|(_, c)| c.product_version_id == version_id || doc_ids.contains(&c.document_id))
            .map(|(k, v)| (*k, v.clone()))
            .collect();
        let embeddings = store
            .embeddings
            .iter()
            .filter(|(_, e)| e.product_version_id == version_id || doc_ids.contains(&e.document_id))
            .map(|(k, v)| (*k, v.clone()))
            .collect();
        Self {
            version_id,
            versions: store
                .versions
                .iter()
                .filter(|(id, _)| **id == version_id)
                .map(|(k, v)| (*k, v.clone()))
                .collect(),
            documents,
            chunks,
            embeddings,
            graph: store
                .graph
                .iter()
                .filter(|(_, n)| n.version_id == version_id)
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            wiki: store
                .wiki
                .iter()
                .filter(|((vid, _), _)| *vid == version_id)
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            wiki_folders: store
                .wiki_folders
                .iter()
                .filter(|(_, f)| f.product_version_id == version_id)
                .map(|(k, v)| (*k, v.clone()))
                .collect(),
            wiki_ops: store
                .wiki_ops
                .iter()
                .filter(|o| o.version_id == version_id)
                .cloned()
                .collect(),
            wiki_tombstones: store
                .wiki_tombstones
                .iter()
                .filter(|((vid, _), _)| *vid == version_id)
                .map(|(k, v)| (*k, *v))
                .collect(),
            wiki_slug_locks: store.wiki_slug_locks.clone(),
            wiki_inflight: store
                .wiki_inflight
                .iter()
                .filter(|(id, _)| **id == version_id)
                .map(|(k, v)| (*k, *v))
                .collect(),
            wiki_op_seq: store.wiki_op_seq,
            queue: VecDeque::new(),
            dead_letters: Vec::new(),
        }
    }

    pub fn write_back(self, store: &mut Store) {
        let version_id = self.version_id;
        store.versions.extend(self.versions);
        store
            .documents
            .retain(|_, d| d.product_version_id != version_id);
        store.documents.extend(self.documents);
        store
            .chunks
            .retain(|_, c| c.product_version_id != version_id);
        store.chunks.extend(self.chunks);
        store
            .embeddings
            .retain(|_, e| e.product_version_id != version_id);
        store.embeddings.extend(self.embeddings);
        store.graph.retain(|_, n| n.version_id != version_id);
        store.graph.extend(self.graph);
        store.wiki.retain(|(vid, _), _| *vid != version_id);
        store.wiki.extend(self.wiki);
        store
            .wiki_folders
            .retain(|_, f| f.product_version_id != version_id);
        store.wiki_folders.extend(self.wiki_folders);
        store.wiki_ops.retain(|o| o.version_id != version_id);
        store.wiki_ops.extend(self.wiki_ops);
        store
            .wiki_tombstones
            .retain(|(vid, _), _| *vid != version_id);
        store.wiki_tombstones.extend(self.wiki_tombstones);
        store.wiki_slug_locks = self.wiki_slug_locks;
        store.wiki_inflight.retain(|id, _| *id != version_id);
        store.wiki_inflight.extend(self.wiki_inflight);
        store.wiki_op_seq = self.wiki_op_seq.max(store.wiki_op_seq);
        store.queue.extend(self.queue);
        store.dead_letters.extend(self.dead_letters);
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
