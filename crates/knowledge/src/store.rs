use crate::process::{ParserEngineRule, ProcessOverrides, default_parser_engine_rules};
use crate::status::*;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub password_hash: String,
    #[serde(default)]
    pub ldap_dn: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    #[serde(default)]
    pub kind: WorkspaceKind,
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
            enable_multimodel: true,
            asr_enabled: false,
            asr_model_id: String::new(),
            embedding_model_id: String::new(),
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
            parser_engine_rules: default_parser_engine_rules(),
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
    pub object_ref: String,
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
    #[serde(default)]
    pub index_ready: bool,
}

impl Document {
    pub fn new(
        product_version_id: Uuid,
        title: String,
        file_name: String,
        file_size: i64,
        file_hash: String,
        object_ref: String,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            product_version_id,
            title,
            file_name,
            file_size,
            file_hash,
            object_ref,
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
            index_ready: false,
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
    pub chunk_refs: Vec<Uuid>,
    #[serde(default)]
    pub category_path: Vec<String>,
    #[serde(default)]
    pub folder_id: Option<Uuid>,
}

impl WikiPage {
    pub fn published(
        id: Uuid,
        product_version_id: Uuid,
        slug: String,
        title: String,
        content: String,
        page_type: String,
        summary: String,
    ) -> Self {
        Self {
            id,
            product_version_id,
            slug,
            title,
            content,
            page_type,
            status: "published".into(),
            summary,
            aliases: Vec::new(),
            source_refs: Vec::new(),
            chunk_refs: Vec::new(),
            category_path: Vec::new(),
            folder_id: None,
        }
    }
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

/// Ephemeral Wiki operation used only inside one typed Oxana job execution.
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
