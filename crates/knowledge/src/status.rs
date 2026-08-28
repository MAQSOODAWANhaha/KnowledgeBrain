use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParseStatus {
    Pending,
    Processing,
    Finalizing,
    Completed,
    Failed,
    Cancelled,
    Deleting,
}

impl ParseStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Processing => "processing",
            Self::Finalizing => "finalizing",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Deleting => "deleting",
        }
    }

    pub fn is_aborted(self) -> bool {
        matches!(self, Self::Cancelled | Self::Deleting)
    }

    /// Worker entry: skip completed; abort cancelled/deleting; failed may retry.
    pub fn worker_should_exit(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled | Self::Deleting)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum SummaryStatus {
    #[default]
    None,
    Pending,
    Processing,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionStatus {
    Cloning,
    Active,
    Archived,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProductKind {
    #[default]
    Product,
    Library,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceKind {
    #[default]
    ProductLine,
    Company,
}

impl WorkspaceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProductLine => "product_line",
            Self::Company => "company",
        }
    }

    pub fn parse(s: &str) -> Self {
        if s == "company" {
            Self::Company
        } else {
            Self::ProductLine
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Owner,
    Admin,
    Contributor,
    Viewer,
}

impl Role {
    pub fn can_write(self) -> bool {
        !matches!(self, Self::Viewer)
    }

    pub fn can_admin(self) -> bool {
        matches!(self, Self::Owner | Self::Admin)
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_type_strings_match_brain() {
        assert_eq!(TYPE_DOCUMENT_PROCESS, "document:process");
        assert_eq!(TYPE_POST_PROCESS, "knowledge:post_process");
        assert_eq!(TYPE_SEMANTIC_INDEX_V2, "knowledge:semantic_index:v2");
        assert_eq!(TYPE_KB_DELETE, "kb:delete");
        assert_eq!(TYPE_LIST_DELETE, "knowledge:list_delete");
        assert_eq!(TYPE_INDEX_DELETE, "index:delete");
        assert_eq!(TYPE_BID_DELIVERY_V1, "bid:delivery:v1");
        assert_eq!(QUEUE_LOW, "low");
        assert_eq!(QUEUE_BID_DELIVERY_V1, "bid-delivery-v1");
    }
}
