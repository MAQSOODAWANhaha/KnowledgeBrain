use crate::process::{ParserEngineRule, ProcessOverrides};
use crate::status::*;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub password_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub retrieval: RetrievalConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalConfig {
    pub vector_threshold: f64,
    pub keyword_threshold: f64,
    pub embedding_top_k: usize,
}

impl Default for RetrievalConfig {
    fn default() -> Self {
        Self {
            vector_threshold: 0.15,
            keyword_threshold: 0.3,
            embedding_top_k: 50,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Product {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub kind: ProductKind,
    pub name: String,
    pub slug: String,
    pub current_version_id: Option<Uuid>,
    pub embedding_model_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductVersion {
    pub id: Uuid,
    pub product_id: Uuid,
    pub label: String,
    pub status: VersionStatus,
    pub cloned_from: Option<Uuid>,
    pub vector_enabled: bool,
    pub keyword_enabled: bool,
    pub wiki_enabled: bool,
    pub graph_enabled: bool,
    pub extract_enabled: bool,
    pub extract_custom_instructions: String,
    pub question_enabled: bool,
    pub question_count: usize,
    pub question_custom_instructions: String,
    pub enable_multimodel: bool,
    pub asr_enabled: bool,
    pub asr_model_id: String,
    pub embedding_model_id: String,
    pub summary_model_id: String,
    pub wiki_synthesis_model_id: String,
    pub chunk_size: usize,
    pub chunk_overlap: usize,
    pub chunk_strategy: String,
    pub enable_parent_child: bool,
    pub parent_chunk_size: usize,
    pub child_chunk_size: usize,
    pub chunk_separators: Vec<String>,
    pub chunk_token_limit: usize,
    pub chunk_languages: Vec<String>,
    pub parser_engine_rules: Vec<ParserEngineRule>,
    pub table_metadata_instructions: String,
}

impl ProductVersion {
    pub fn new(product_id: Uuid, label: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            product_id,
            label,
            status: VersionStatus::Active,
            cloned_from: None,
            vector_enabled: true,
            keyword_enabled: true,
            wiki_enabled: true,
            graph_enabled: true,
            extract_enabled: true,
            extract_custom_instructions: String::new(),
            question_enabled: true,
            question_count: 3,
            question_custom_instructions: String::new(),
            enable_multimodel: false,
            asr_enabled: false,
            asr_model_id: String::new(),
            embedding_model_id: "stub-emb".into(),
            summary_model_id: "stub-chat".into(),
            wiki_synthesis_model_id: String::new(),
            chunk_size: 512,
            chunk_overlap: 80,
            chunk_strategy: "auto".into(),
            enable_parent_child: false,
            parent_chunk_size: 0,
            child_chunk_size: 0,
            chunk_separators: Vec::new(),
            chunk_token_limit: 0,
            chunk_languages: Vec::new(),
            parser_engine_rules: Vec::new(),
            table_metadata_instructions: String::new(),
        }
    }

    /// Brain `ParentChunkSize` default 4096.
    pub fn parent_chunk_size(&self) -> usize {
        if self.parent_chunk_size == 0 {
            4096
        } else {
            self.parent_chunk_size
        }
    }

    /// Brain `ChildChunkSize` default 384.
    pub fn child_chunk_size(&self) -> usize {
        if self.child_chunk_size == 0 {
            384
        } else {
            self.child_chunk_size
        }
    }

    pub fn needs_embedding(&self) -> bool {
        self.vector_enabled || self.keyword_enabled
    }

    /// Spec 5.9 / brain: default 3, max 10; 0 means default.
    pub fn question_count(&self) -> usize {
        if self.question_count == 0 {
            3
        } else {
            self.question_count.min(10)
        }
    }

    /// Spec 5.11: wiki synthesis model, else summary_model_id.
    pub fn wiki_chat_model(&self) -> &str {
        if self.wiki_synthesis_model_id.is_empty() {
            &self.summary_model_id
        } else {
            &self.wiki_synthesis_model_id
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: Uuid,
    pub product_version_id: Uuid,
    pub title: String,
    pub file_name: String,
    pub file_size: i64,
    pub file_hash: String,
    pub object_key: String,
    pub parse_status: ParseStatus,
    pub enable_status: String,
    pub summary_status: SummaryStatus,
    pub pending_subtasks_count: i32,
    pub error_message: String,
    pub description: String,
    pub markdown: String,
    pub attempt: i32,
    pub processed_at: Option<DateTime<Utc>>,
    pub started_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
    pub process_overrides: Option<ProcessOverrides>,
    #[serde(default)]
    pub doc_type: String,
    #[serde(default)]
    pub source_passages: Vec<String>,
}

impl Document {
    pub fn new(
        product_version_id: Uuid,
        title: String,
        file_name: String,
        file_size: i64,
        file_hash: String,
        object_key: String,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            product_version_id,
            title,
            file_name,
            file_size,
            file_hash,
            object_key,
            parse_status: ParseStatus::Pending,
            enable_status: "disabled".into(),
            summary_status: SummaryStatus::default(),
            pending_subtasks_count: 0,
            error_message: String::new(),
            description: String::new(),
            markdown: String::new(),
            attempt: 1,
            processed_at: None,
            started_at: None,
            updated_at: Utc::now(),
            process_overrides: None,
            doc_type: "file".into(),
            source_passages: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub id: Uuid,
    pub document_id: Uuid,
    pub product_version_id: Uuid,
    pub chunk_type: String,
    pub content: String,
    pub context_header: String,
    pub start_at: i32,
    pub end_at: i32,
    pub parent_chunk_id: Option<Uuid>,
    pub generated_questions: Vec<String>,
}

impl Chunk {
    pub fn embedding_content(&self) -> String {
        let body = self.content.trim();
        if self.context_header.is_empty() {
            body.to_string()
        } else {
            format!("{}\n\n{body}", self.context_header)
        }
    }

    pub fn index_content(&self, title: &str) -> String {
        let prefix = if title.trim().is_empty() {
            String::new()
        } else {
            format!("{}\n", title.trim())
        };
        format!("{}{}", prefix, self.embedding_content())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkEmbedding {
    pub chunk_id: Uuid,
    pub product_version_id: Uuid,
    pub document_id: Uuid,
    pub content: String,
    pub vector: Vec<f32>,
    pub tsv: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tag {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub name: String,
    pub slug: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: Uuid,
    pub task_type: String,
    pub queue: String,
    pub payload: serde_json::Value,
    pub retries: u32,
    pub max_retry: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadLetter {
    pub task_type: String,
    pub related_id: Uuid,
    pub last_error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub version_id: Uuid,
    pub document_id: Uuid,
    pub name: String,
    pub chunk_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiPage {
    pub id: Uuid,
    pub product_version_id: Uuid,
    pub slug: String,
    pub title: String,
    pub content: String,
    pub page_type: String,
    pub status: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub source_refs: Vec<Uuid>,
    #[serde(default)]
    pub category_path: Vec<String>,
    #[serde(default)]
    pub folder_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiFolder {
    pub id: Uuid,
    pub product_version_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub path: String,
    pub depth: i32,
    pub sort_order: i32,
}

/// One row in brain `task_pending_ops`. Lane is `wiki:ingest` or `wiki:finalize`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiPendingOp {
    pub id: i64,
    pub lane: String,
    pub version_id: Uuid,
    pub op: String,
    pub document_id: Option<Uuid>,
    pub slug: String,
    pub title: String,
    pub claimed_at: Option<DateTime<Utc>>,
    pub fail_count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphRelation {
    pub version_id: Uuid,
    pub document_id: Uuid,
    pub node1: String,
    pub node2: String,
    pub rel_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Span {
    pub span_id: Uuid,
    pub document_id: Uuid,
    pub attempt: i32,
    pub name: String,
    pub parent_span_id: Option<Uuid>,
    pub kind: String,
    pub status: String,
    pub output: Option<serde_json::Value>,
    pub error_message: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub duration_ms: Option<i64>,
}

#[derive(Debug, Default)]
pub struct Store {
    pub users: HashMap<Uuid, User>,
    pub users_by_email: HashMap<String, Uuid>,
    pub members: HashMap<(Uuid, Uuid), Role>,
    pub workspaces: HashMap<Uuid, Workspace>,
    pub products: HashMap<Uuid, Product>,
    pub versions: HashMap<Uuid, ProductVersion>,
    pub documents: HashMap<Uuid, Document>,
    pub chunks: HashMap<Uuid, Chunk>,
    pub embeddings: HashMap<Uuid, ChunkEmbedding>,
    pub tags: HashMap<Uuid, Tag>,
    pub document_tags: HashSet<(Uuid, Uuid)>,
    pub objects: HashMap<String, Vec<u8>>,
    pub object_refs: HashMap<String, i32>,
    pub queue: VecDeque<Job>,
    pub dead_letters: Vec<DeadLetter>,
    pub graph: HashMap<(Uuid, Uuid, String), GraphNode>,
    pub relations: HashMap<(Uuid, Uuid, String, String, String), GraphRelation>,
    pub wiki: HashMap<(Uuid, String), WikiPage>,
    pub wiki_folders: HashMap<Uuid, WikiFolder>,
    pub wiki_ops: Vec<WikiPendingOp>,
    pub wiki_tombstones: HashMap<(Uuid, Uuid), DateTime<Utc>>,
    pub wiki_slug_locks: HashMap<String, DateTime<Utc>>,
    pub wiki_inflight: HashMap<Uuid, DateTime<Utc>>,
    pub wiki_op_seq: i64,
    pub multimodal_pending: HashMap<Uuid, i32>,
    pub spans: Vec<Span>,
    /// Test/ops hook: HTTP ingest treats this as enqueue failure.
    pub enqueue_fail: bool,
    pub api_keys: HashMap<Uuid, ApiKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: Uuid,
    pub name: String,
    pub key_hash: String,
    pub prefix: String,
    pub scope_type: String,
    pub scope_id: Uuid,
    pub scopes: Vec<String>,
}

impl Store {
    pub fn try_enqueue(
        &mut self,
        task_type: &str,
        queue: &str,
        payload: serde_json::Value,
    ) -> Result<Uuid, String> {
        if self.enqueue_fail {
            return Err("enqueue failed".into());
        }
        Ok(self.enqueue(task_type, queue, payload))
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

    pub fn pop_queue(&mut self) -> Option<Job> {
        self.queue.pop_front()
    }

    pub fn effective_version(&self, document_id: Uuid) -> Option<ProductVersion> {
        let doc = self.documents.get(&document_id)?;
        let mut version = self.versions.get(&doc.product_version_id)?.clone();
        crate::resolve_process_config(&version, doc.process_overrides.as_ref())
            .apply_to(&mut version);
        Some(version)
    }

    pub fn find_duplicate(
        &self,
        version_id: Uuid,
        file_name: &str,
        file_size: i64,
        file_hash: &str,
    ) -> Option<Uuid> {
        self.documents
            .values()
            .find(|d| {
                d.product_version_id == version_id
                    && d.file_name == file_name
                    && d.file_size == file_size
                    && d.file_hash == file_hash
            })
            .map(|d| d.id)
    }

    pub fn set_finalizing(&mut self, doc_id: Uuid, n: i32) -> bool {
        let Some(d) = self.documents.get_mut(&doc_id) else {
            return false;
        };
        if d.parse_status != ParseStatus::Processing {
            return false;
        }
        d.parse_status = ParseStatus::Finalizing;
        d.pending_subtasks_count = n;
        d.updated_at = Utc::now();
        true
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

    pub fn mark_processing(&mut self, doc_id: Uuid) -> Result<(), ParseStatus> {
        let d = self.documents.get_mut(&doc_id).ok_or(ParseStatus::Failed)?;
        if d.parse_status.is_aborted() || d.parse_status == ParseStatus::Completed {
            return Err(d.parse_status);
        }
        d.parse_status = ParseStatus::Processing;
        d.started_at = Some(Utc::now());
        d.updated_at = Utc::now();
        Ok(())
    }

    pub fn role_of(&self, workspace_id: Uuid, user_id: Uuid) -> Option<Role> {
        self.members.get(&(workspace_id, user_id)).copied()
    }

    pub fn product_workspace(&self, product_id: Uuid) -> Option<Uuid> {
        self.products.get(&product_id).map(|p| p.workspace_id)
    }

    pub fn resolve_version(&self, product_id: Uuid, version_id: &str) -> Option<Uuid> {
        if version_id == "current" {
            self.products
                .get(&product_id)
                .and_then(|p| p.current_version_id)
        } else {
            Uuid::parse_str(version_id).ok().filter(|id| {
                self.versions
                    .get(id)
                    .is_some_and(|v| v.product_id == product_id)
            })
        }
    }

    pub fn del_graph(&mut self, version_id: Uuid, document_id: Uuid) {
        self.graph
            .retain(|(v, d, _), _| !(*v == version_id && *d == document_id));
        self.relations
            .retain(|(v, d, _, _, _), _| !(*v == version_id && *d == document_id));
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

    pub fn clear_document_index(&mut self, document_id: Uuid) {
        let version_id = self
            .documents
            .get(&document_id)
            .map(|d| d.product_version_id);
        self.chunks.retain(|_, c| c.document_id != document_id);
        self.embeddings.retain(|_, e| e.document_id != document_id);
        if let Some(vid) = version_id {
            self.del_graph(vid, document_id);
        }
    }

    pub fn document_tag_ids(&self, document_id: Uuid) -> Vec<Uuid> {
        self.document_tags
            .iter()
            .filter(|(d, _)| *d == document_id)
            .map(|(_, t)| *t)
            .collect()
    }

    pub fn fail_document(&mut self, document_id: Uuid, message: &str) {
        if let Some(d) = self.documents.get_mut(&document_id) {
            d.parse_status = ParseStatus::Failed;
            d.error_message = message.to_string();
            d.updated_at = Utc::now();
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ParseStatus;

    #[test]
    fn set_finalizing_only_from_processing() {
        let mut s = Store::default();
        let id = Uuid::new_v4();
        s.documents.insert(
            id,
            Document {
                id,
                product_version_id: Uuid::new_v4(),
                title: "t".into(),
                file_name: "a.txt".into(),
                file_size: 1,
                file_hash: "h".into(),
                object_key: "h".into(),
                parse_status: ParseStatus::Pending,
                enable_status: "disabled".into(),
                summary_status: Default::default(),
                pending_subtasks_count: 0,
                error_message: String::new(),
                description: String::new(),
                markdown: String::new(),
                attempt: 1,
                processed_at: None,
                started_at: None,
                updated_at: Utc::now(),
                process_overrides: None,
                doc_type: "file".into(),
                source_passages: Vec::new(),
            },
        );
        assert!(!s.set_finalizing(id, 2));
        s.documents.get_mut(&id).unwrap().parse_status = ParseStatus::Processing;
        assert!(s.set_finalizing(id, 2));
        assert_eq!(s.documents[&id].parse_status, ParseStatus::Finalizing);
        s.finalize_subtask(id);
        assert_eq!(s.documents[&id].pending_subtasks_count, 1);
        s.finalize_subtask(id);
        assert_eq!(s.documents[&id].parse_status, ParseStatus::Completed);
    }
}
