use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text", rename_all = "snake_case")]
pub enum ParseStatus {
    Pending,
    Processing,
    Finalizing,
    Completed,
    Failed,
    Cancelled,
    Deleting,
    Deleted,
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
            Self::Deleted => "deleted",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type, Default)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text", rename_all = "snake_case")]
pub enum SummaryStatus {
    #[default]
    None,
    Pending,
    Processing,
    Completed,
    Failed,
}

impl SummaryStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Pending => "pending",
            Self::Processing => "processing",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text", rename_all = "snake_case")]
pub enum VersionStatus {
    Cloning,
    Active,
    Archived,
    Failed,
}

impl VersionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cloning => "cloning",
            Self::Active => "active",
            Self::Archived => "archived",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text", rename_all = "snake_case")]
pub enum EnableStatus {
    Disabled,
    Enabled,
}

impl EnableStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Enabled => "enabled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text", rename_all = "snake_case")]
pub enum WikiStatus {
    Draft,
    Published,
    Archived,
}

impl WikiStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Published => "published",
            Self::Archived => "archived",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text", rename_all = "snake_case")]
pub enum OutboxState {
    Pending,
    Published,
    Complete,
    Failed,
}

impl OutboxState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Published => "published",
            Self::Complete => "complete",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text", rename_all = "snake_case")]
pub enum SemanticIndexIntentStatus {
    Pending,
    Completed,
    Terminal,
    Superseded,
}

impl SemanticIndexIntentStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Completed => "completed",
            Self::Terminal => "terminal",
            Self::Superseded => "superseded",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text", rename_all = "snake_case")]
pub enum SupportState {
    Supported,
    Revoked,
}

impl SupportState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::Revoked => "revoked",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text", rename_all = "snake_case")]
pub enum SpanStatus {
    Pending,
    Running,
    #[serde(alias = "ok")]
    Done,
    #[serde(alias = "error")]
    Failed,
    Skipped,
    Cancelled,
}

impl SpanStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::Cancelled => "cancelled",
        }
    }
}

impl ParseStatus {
    pub fn is_aborted(self) -> bool {
        matches!(self, Self::Cancelled | Self::Deleting | Self::Deleted)
    }

    /// Completed and removed documents cannot be entered by a delayed worker.
    pub fn worker_should_exit(self) -> bool {
        self == Self::Completed || self.is_aborted()
    }
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

#[cfg(test)]
mod tests {
    #[test]
    fn deleted_document_status_is_a_supported_terminal_value() {
        let parsed = serde_json::from_str::<super::ParseStatus>("\"deleted\"");
        assert!(
            parsed.is_ok(),
            "stored deleted documents must decode: {parsed:?}"
        );
        assert!(parsed.unwrap().worker_should_exit());
    }

    #[test]
    fn task_type_strings_match_brain() {
        assert_eq!(platform::TYPE_DOCUMENT_PROCESS, "document:process");
        assert_eq!(platform::TYPE_POST_PROCESS, "knowledge:post_process");
        assert_eq!(
            platform::TYPE_SEMANTIC_INDEX_V2,
            "knowledge:semantic_index:v2"
        );
        assert_eq!(platform::TYPE_KB_DELETE, "kb:delete");
        assert_eq!(platform::TYPE_LIST_DELETE, "knowledge:list_delete");
        assert_eq!(platform::TYPE_INDEX_DELETE, "index:delete");
        assert_eq!(platform::QUEUE_LOW, "low");
    }
}
