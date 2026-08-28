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

    pub fn parse(s: &str) -> Self {
        match s {
            "owner" => Self::Owner,
            "admin" => Self::Admin,
            "contributor" => Self::Contributor,
            _ => Self::Viewer,
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn task_type_strings_match_brain() {
        assert_eq!(crate::TYPE_DOCUMENT_PROCESS, "document:process");
        assert_eq!(crate::TYPE_POST_PROCESS, "knowledge:post_process");
        assert_eq!(crate::TYPE_SEMANTIC_INDEX_V2, "knowledge:semantic_index:v2");
        assert_eq!(crate::TYPE_KB_DELETE, "kb:delete");
        assert_eq!(crate::TYPE_LIST_DELETE, "knowledge:list_delete");
        assert_eq!(crate::TYPE_INDEX_DELETE, "index:delete");
        assert_eq!(crate::QUEUE_LOW, "low");
    }
}
