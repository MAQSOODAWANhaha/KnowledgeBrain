use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionMode {
    Agent,
    Hybrid,
    Heuristic,
}

impl ExtractionMode {
    pub fn from_env() -> Result<Self, String> {
        match std::env::var("BID_EXTRACT_MODE")
            .unwrap_or_else(|_| "hybrid".into())
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "agent" => Ok(Self::Agent),
            "hybrid" | "" => Ok(Self::Hybrid),
            "heuristic" => Ok(Self::Heuristic),
            other => Err(format!("invalid BID_EXTRACT_MODE: {other}")),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Hybrid => "hybrid",
            Self::Heuristic => "heuristic",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClauseFamily {
    Technical,
    Commercial,
}

impl ClauseFamily {
    pub const ALL: [Self; 2] = [Self::Technical, Self::Commercial];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Technical => "technical",
            Self::Commercial => "commercial",
        }
    }

    pub fn other(self) -> Self {
        match self {
            Self::Technical => Self::Commercial,
            Self::Commercial => Self::Technical,
        }
    }
}

#[derive(Debug, Clone)]
pub enum ExtractionScope {
    Document,
    Section {
        section_key: String,
        heading_path: String,
        hint_family: String,
        body: String,
    },
}

#[derive(Debug, Clone)]
pub struct ExtractionInput {
    pub document_id: Uuid,
    pub markdown: String,
    pub scope: ExtractionScope,
}

impl ExtractionInput {
    pub fn document(document_id: Uuid, markdown: String) -> Self {
        Self {
            document_id,
            markdown,
            scope: ExtractionScope::Document,
        }
    }

    pub fn section(document_id: Uuid, section: &ExtractedSection) -> Self {
        Self {
            document_id,
            markdown: section.body.clone(),
            scope: ExtractionScope::Section {
                section_key: section.key.clone(),
                heading_path: section.heading_path.clone(),
                hint_family: section.hint_family.clone(),
                body: section.body.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedSpan {
    pub id: String,
    pub section_key: String,
    pub heading_path: String,
    pub ordinal: usize,
    #[serde(default)]
    pub context: String,
    pub body: String,
    pub candidate: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedSection {
    pub key: String,
    pub heading_path: String,
    pub hint_family: String,
    pub body: String,
    pub spans: Vec<ExtractedSpan>,
    pub extract_status: String,
    pub error_message: String,
}

#[derive(Debug, Clone)]
pub struct CandidateClause {
    pub span_id: String,
    pub quote: String,
    pub text: String,
    pub must: bool,
    pub proposed_family: ClauseFamily,
    pub extractor: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedClause {
    pub section_key: String,
    pub span_id: String,
    pub heading_path: String,
    pub quote: String,
    pub text: String,
    pub family: String,
    pub must: bool,
    pub family_conflict: bool,
    pub extraction_meta: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CoverageReport {
    pub candidate_spans: usize,
    pub covered_spans: usize,
    pub uncovered_spans: Vec<String>,
    pub ambiguous_clauses: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtractionDiagnostics {
    pub mode: String,
    pub model_id: String,
    pub policy_version: String,
    pub prompt_version: String,
    pub agent_rounds: usize,
    pub retries: usize,
    pub tool_calls: usize,
    pub agent_terminations: Vec<String>,
    pub rejected_invalid_quotes: usize,
    pub family_conflicts: usize,
    pub fallback_reasons: Vec<String>,
    pub failed_spans: Vec<String>,
    pub coverage: CoverageReport,
    #[serde(default)]
    pub partial_failure: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionReport {
    pub sections: Vec<ExtractedSection>,
    pub clauses: Vec<ExtractedClause>,
    pub diagnostics: ExtractionDiagnostics,
}

#[derive(Debug, Clone)]
pub struct ExtractionFailure {
    pub message: String,
    pub diagnostics: ExtractionDiagnostics,
}

impl std::fmt::Display for ExtractionFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ExtractionFailure {}
