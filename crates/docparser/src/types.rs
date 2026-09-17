//! Convert DTOs shared by simple/anydoc/gRPC/HTTP engines.

#[derive(Debug, Clone, Default)]
pub struct ImageRef {
    pub filename: String,
    pub original_ref: String,
    pub mime_type: String,
    pub storage_key: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StructuredSourceUnitKind {
    Section,
    TableRow,
    TableRegion,
    FormRegion,
    AttachmentRegion,
    ImageRegion,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PdfTableCell {
    pub row: u32,
    pub column: u32,
    pub row_span: u32,
    pub col_span: u32,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PdfTableMergedRange {
    pub start_row: u32,
    pub start_column: u32,
    pub end_row: u32,
    pub end_column: u32,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SpreadsheetCell {
    pub address: String,
    pub row: u32,
    pub column: u32,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SpreadsheetRange {
    pub a1_range: String,
    pub start_row: u32,
    pub start_column: u32,
    pub end_row: u32,
    pub end_column: u32,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SpreadsheetTableIdentity {
    pub name: String,
    pub display_name: String,
    pub a1_range: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "parent_kind", rename_all = "snake_case")]
pub enum CompoundImageParent {
    Paragraph {
        section_ordinal: u32,
        paragraph_ordinal: u32,
    },
    TableCell {
        section_ordinal: u32,
        table_ordinal: u32,
        row_ordinal: u32,
        cell_ordinal: u32,
    },
    Form {
        section_ordinal: u32,
        form_ordinal: u32,
    },
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "locator_kind", rename_all = "snake_case")]
pub enum StructuredSourceLocator {
    Document {
        section_ordinal: u32,
        table_ordinal: Option<u32>,
        row_ordinal: Option<u32>,
        form_ordinal: Option<u32>,
        heading_path: String,
    },
    Page {
        page_ordinal: u32,
        left: Option<f64>,
        top: Option<f64>,
        right: Option<f64>,
        bottom: Option<f64>,
    },
    PageTable {
        page_ordinal: u32,
        table_ordinal: u32,
        left: f64,
        top: f64,
        right: f64,
        bottom: f64,
    },
    Spreadsheet {
        sheet_ordinal: u32,
        sheet_name: String,
        region: SpreadsheetRange,
        cells: Vec<SpreadsheetCell>,
        merged_ranges: Vec<SpreadsheetRange>,
        defined_tables: Vec<SpreadsheetTableIdentity>,
    },
    Image {
        original_ref: String,
        width: u32,
        height: u32,
        media_type: String,
        page_ordinal: Option<u32>,
        compound_parent: Option<CompoundImageParent>,
        left: Option<f64>,
        top: Option<f64>,
        right: Option<f64>,
        bottom: Option<f64>,
    },
    Attachment {
        part_name: String,
        relationship_type: String,
    },
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TableGrid {
    pub row_count: u32,
    pub column_count: u32,
    pub cells: Vec<PdfTableCell>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub widths_mm: Option<Vec<f64>>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StructuredSourceUnit {
    pub key: String,
    pub ordinal: u32,
    pub kind: StructuredSourceUnitKind,
    pub text: String,
    pub locator: StructuredSourceLocator,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid: Option<TableGrid>,
}

#[derive(Debug, Clone, Default)]
pub struct ReadResult {
    pub markdown: String,
    pub error: String,
    pub images: Vec<ImageRef>,
    pub structured_source_units: Vec<StructuredSourceUnit>,
    pub is_audio: bool,
    pub audio_data: Vec<u8>,
    pub metadata: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct ConvertError(pub String);

impl std::fmt::Display for ConvertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ConvertError {}

/// Typed read boundary for callers that must distinguish retries from bad input.
/// The existing ConvertError API remains available to general ingest callers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocReaderReadError {
    Cancelled,
    Transient(String),
    Configuration(String),
    InvalidResponse(String),
}

impl std::fmt::Display for DocReaderReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => f.write_str("cancelled"),
            Self::Transient(message)
            | Self::Configuration(message)
            | Self::InvalidResponse(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for DocReaderReadError {}

pub const NOT_CONFIGURED: &str = "Document parsing service is not configured. Please use text/paragraph import or set DOCREADER_ADDR.";

pub struct ConvertInput<'a> {
    pub engine: &'a str,
    pub file_name: &'a str,
    pub file_type: &'a str,
    pub is_url: bool,
    pub bytes: Vec<u8>,
    pub url: &'a str,
    pub title: &'a str,
    pub overrides: &'a std::collections::HashMap<String, String>,
}
