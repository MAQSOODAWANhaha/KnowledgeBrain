//! Convert: simple formats in-process; others via DocReader gRPC ReadStream.

mod anydoc;
mod asr;
mod engines;
mod grpc;
mod http_engine;
mod images;

pub use anydoc::{
    ENGINE as ANYDOC_ENGINE, supported_file_types as anydoc_file_types,
    supports as anydoc_supports, version as anydoc_version,
};
pub use asr::{ASR_NOT_CONFIGURED, AsrSettings, apply as apply_asr, apply_stub as apply_asr_stub};
pub use engines::{EngineCatalog, EngineInfo, list_all_engines, local_engines, merge_engines};
pub use grpc::{ConvertRequest, DOCREADER_TIMEOUT, reader_addr};
pub use images::{rewrite_images, rewrite_inline};

use platform::{is_audio_type, is_image_type, is_simple_format};

pub const NOT_CONFIGURED: &str = "Document parsing service is not configured. Please use text/paragraph import or set DOCREADER_ADDR.";

pub mod proto {
    tonic::include_proto!("docreader");
}

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
pub struct StructuredSourceUnit {
    pub key: String,
    pub ordinal: u32,
    pub kind: StructuredSourceUnitKind,
    pub text: String,
    pub locator: StructuredSourceLocator,
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

pub fn resolve_engine(engine: &str, file_type: &str, is_url: bool) -> &'static str {
    match engine {
        "simple" => "simple",
        "builtin" => "docreader",
        "anydoc" => "anydoc",
        "mineru" | "mineru_cloud" | "paddleocr_vl" | "paddleocr_vl_cloud" => "http-engine",
        "" => {
            if !is_url && is_simple_format(file_type) {
                "simple"
            } else {
                "docreader"
            }
        }
        _ => {
            if !is_url && is_simple_format(file_type) {
                "simple"
            } else {
                "docreader"
            }
        }
    }
}

pub fn convert_simple(file_name: &str, bytes: &[u8]) -> ReadResult {
    if is_audio_type(file_name) {
        let name = file_name.rsplit('/').next().unwrap_or(file_name);
        return ReadResult {
            markdown: format!("[Audio file: {name}]"),
            error: String::new(),
            images: Vec::new(),
            structured_source_units: Vec::new(),
            is_audio: true,
            audio_data: bytes.to_vec(),
            metadata: std::collections::HashMap::new(),
        };
    }
    if is_image_type(file_name) {
        let name = file_name.rsplit('/').next().unwrap_or(file_name);
        let original = format!("images/{name}");
        return ReadResult {
            markdown: format!("![{name}]({original})"),
            images: vec![ImageRef {
                filename: name.to_string(),
                original_ref: original,
                mime_type: String::new(),
                storage_key: String::new(),
                data: bytes.to_vec(),
            }],
            ..ReadResult::default()
        };
    }
    let ext = file_name
        .rsplit('.')
        .next()
        .unwrap_or("txt")
        .to_ascii_lowercase();
    if matches!(ext.as_str(), "md" | "markdown" | "txt" | "text") {
        match String::from_utf8(bytes.to_vec()) {
            Ok(s) => ReadResult {
                markdown: s,
                ..ReadResult::default()
            },
            Err(_) => ReadResult {
                error: "invalid utf-8".into(),
                ..ReadResult::default()
            },
        }
    } else if ext == "json" {
        ReadResult {
            markdown: json_to_md(bytes),
            ..ReadResult::default()
        }
    } else if ext == "csv" {
        ReadResult {
            markdown: csv_to_md(bytes),
            ..ReadResult::default()
        }
    } else {
        ReadResult {
            error: format!("simple reader cannot parse .{ext}"),
            ..ReadResult::default()
        }
    }
}

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

/// Convert bytes to markdown without a Document row. Shared by product ingest and bid:convert.
/// Engine comes from the product default table (office→anydoc, pdf→builtin).
pub async fn convert_tender_source(
    file_name: &str,
    bytes: Vec<u8>,
) -> Result<ReadResult, ConvertError> {
    let ext = file_name
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(
        ext.as_str(),
        "pdf" | "docx" | "xlsx" | "png" | "jpg" | "jpeg" | "webp"
    ) {
        return Err(ConvertError(format!(
            "unsupported tender source extension .{ext}"
        )));
    }
    convert("builtin", file_name, &ext, false, bytes, "", file_name).await
}

fn default_engine_for_ext(ext: &str) -> String {
    match ext {
        "docx" | "doc" | "docm" | "xlsx" | "xls" | "xlsm" | "pptx" | "ppt" | "pptm" => {
            "anydoc".into()
        }
        "pdf" => "builtin".into(),
        _ => String::new(),
    }
}

pub async fn convert_to_markdown(
    file_name: &str,
    bytes: Vec<u8>,
) -> Result<ReadResult, ConvertError> {
    let ext = file_name
        .rsplit('.')
        .next()
        .unwrap_or("txt")
        .to_ascii_lowercase();
    let engine = default_engine_for_ext(&ext);
    convert(&engine, file_name, &ext, false, bytes, "", file_name).await
}

/// Route like brain convert: simple in-process; DocReader via gRPC; no reader → error field.
pub async fn convert(
    engine: &str,
    file_name: &str,
    file_type: &str,
    is_url: bool,
    bytes: Vec<u8>,
    url: &str,
    title: &str,
) -> Result<ReadResult, ConvertError> {
    let empty = std::collections::HashMap::new();
    convert_with(ConvertInput {
        engine,
        file_name,
        file_type,
        is_url,
        bytes,
        url,
        title,
        overrides: &empty,
    })
    .await
}

pub async fn convert_with(input: ConvertInput<'_>) -> Result<ReadResult, ConvertError> {
    let resolved = resolve_engine(input.engine, input.file_type, input.is_url);
    match resolved {
        "simple" => Ok(convert_simple(input.file_name, &input.bytes)),
        "anydoc" => convert_anydoc(input).await,
        "docreader" => {
            grpc::read(ConvertRequest {
                file_content: if input.is_url {
                    Vec::new()
                } else {
                    input.bytes
                },
                file_name: input.file_name.to_string(),
                file_type: input.file_type.to_string(),
                url: input.url.to_string(),
                title: input.title.to_string(),
                parser_engine: if input.engine.is_empty() {
                    String::new()
                } else {
                    input.engine.to_string()
                },
                parser_engine_overrides: input.overrides.clone(),
            })
            .await
        }
        "http-engine" => {
            http_engine::convert_http(input.engine, input.file_name, input.bytes).await
        }
        other => Ok(ReadResult {
            error: format!("unknown convert engine {other}"),
            ..ReadResult::default()
        }),
    }
}

fn is_office_file_type(file_type: &str) -> bool {
    matches!(
        file_type
            .trim_start_matches('.')
            .to_ascii_lowercase()
            .as_str(),
        "doc"
            | "docx"
            | "docm"
            | "xls"
            | "xlsx"
            | "xlsm"
            | "ppt"
            | "pptx"
            | "pptm"
            | "odt"
            | "ods"
            | "odp"
            | "rtf"
    )
}

/// Brain AnydocReader: in-process convert. Keep a successful anydoc result.
/// Fallback builtin only on scanned PDF or office convert error/empty.
async fn convert_anydoc(input: ConvertInput<'_>) -> Result<ReadResult, ConvertError> {
    let extract_images = anydoc::extract_images_enabled(input.overrides);
    let result = anydoc::convert(
        input.file_name,
        input.file_type,
        &input.bytes,
        input.is_url,
        extract_images,
    );
    if result.error.is_empty() && !result.markdown.trim().is_empty() {
        return Ok(result);
    }
    let pdf = input.file_type.eq_ignore_ascii_case("pdf");
    let scanned = pdf
        && (result.error.contains("OCR is required")
            || result.error.contains("no extractable text")
            || result.markdown.trim().is_empty());
    let reason = if scanned {
        "scanned_pdf"
    } else if is_office_file_type(input.file_type) {
        "office_error"
    } else {
        return Ok(result);
    };
    if grpc::reader_addr().is_none() {
        return Ok(result);
    }
    tracing::warn!(file = input.file_name, reason, "docparser anydoc fallback");
    fallback_builtin(input, reason).await
}

async fn fallback_builtin(
    input: ConvertInput<'_>,
    reason: &str,
) -> Result<ReadResult, ConvertError> {
    let mut fallback = grpc::read(ConvertRequest {
        file_content: input.bytes,
        file_name: input.file_name.to_string(),
        file_type: input.file_type.to_string(),
        url: input.url.to_string(),
        title: input.title.to_string(),
        parser_engine: "builtin".into(),
        parser_engine_overrides: input.overrides.clone(),
    })
    .await?;
    if fallback.metadata.get("parser").map(String::as_str) != Some("builtin") {
        fallback.metadata.insert("parser".into(), "builtin".into());
    }
    fallback
        .metadata
        .insert("anydoc_fallback".into(), reason.into());
    if reason == "scanned_pdf"
        && fallback
            .metadata
            .get("image_source_type")
            .map(String::is_empty)
            .unwrap_or(true)
    {
        fallback
            .metadata
            .insert("image_source_type".into(), "scanned_pdf".into());
    }
    if fallback.error.is_empty() {
        return Ok(fallback);
    }
    fallback.error = format!(
        "anydoc {reason} fallback failed for {:?}: {}",
        input.file_name, fallback.error
    );
    Ok(fallback)
}

fn json_to_md(bytes: &[u8]) -> String {
    match serde_json::from_slice::<serde_json::Value>(bytes) {
        Ok(v) => json_value_to_md(&v, 0),
        Err(_) => String::from_utf8_lossy(bytes).into_owned(),
    }
}

fn json_value_to_md(v: &serde_json::Value, depth: usize) -> String {
    match v {
        serde_json::Value::Object(map) => {
            let heading = "#".repeat((depth + 1).min(6));
            let mut out = String::new();
            for (k, val) in map {
                match val {
                    serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
                        out.push_str(&format!("{heading} {k}\n\n"));
                        out.push_str(&json_value_to_md(val, depth + 1));
                    }
                    _ => out.push_str(&format!("- **{k}**: {}\n", json_scalar(val))),
                }
            }
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out
        }
        serde_json::Value::Array(arr) => {
            let mut out = String::new();
            for (i, item) in arr.iter().enumerate() {
                match item {
                    serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
                        out.push_str(&format!("{}.\n\n", i + 1));
                        out.push_str(&json_value_to_md(item, depth + 1));
                    }
                    _ => out.push_str(&format!("{}. {}\n", i + 1, json_scalar(item))),
                }
            }
            out
        }
        other => format!("{}\n", json_scalar(other)),
    }
}

fn json_scalar(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn csv_to_md(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut lines = text.lines();
    let Some(header) = lines.next() else {
        return String::new();
    };
    let cols = csv_split_line(header);
    let mut out = format!(
        "| {} |\n|{}|\n",
        cols.join(" | "),
        vec!["---"; cols.len()].join("|")
    );
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        out.push_str(&format!("| {} |\n", csv_split_line(line).join(" | ")));
    }
    out
}

fn csv_split_line(line: &str) -> Vec<String> {
    let mut cols = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '"' {
            if in_quotes && chars.peek() == Some(&'"') {
                chars.next();
                cur.push('"');
            } else {
                in_quotes = !in_quotes;
            }
        } else if c == ',' && !in_quotes {
            cols.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    cols.push(cur);
    cols
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::doc_reader_server::{DocReader, DocReaderServer};
    use crate::proto::{
        ImageRef as ProtoImage, ListEnginesRequest, ListEnginesResponse, ReadRequest, ReadResponse,
        ReadStreamMeta, ReadStreamResponse,
    };
    use tokio_stream::wrappers::ReceiverStream;
    use tonic::{Request, Response, Status};

    #[test]
    fn office_types_are_distinct_from_pdf() {
        assert!(is_office_file_type("docx"));
        assert!(is_office_file_type(".XLSX"));
        assert!(!is_office_file_type("pdf"));
        assert!(!is_office_file_type("md"));
    }

    #[test]
    fn builtin_never_falls_back_to_simple() {
        assert_eq!(resolve_engine("builtin", "md", false), "docreader");
        assert_eq!(resolve_engine("", "md", false), "simple");
        assert_eq!(resolve_engine("", "pdf", false), "docreader");
        assert_eq!(resolve_engine("anydoc", "docx", false), "anydoc");
        assert_eq!(resolve_engine("mineru", "pdf", false), "http-engine");
        assert_eq!(resolve_engine("paddleocr_vl", "pdf", false), "http-engine");
        assert_eq!(resolve_engine("", "md", true), "docreader");
    }

    #[test]
    fn json_expands_object() {
        let r = convert_simple("a.json", br#"{"name":"sw","ports":[1,2]}"#);
        assert!(r.error.is_empty());
        assert!(r.markdown.contains("**name**: sw"), "{}", r.markdown);
        assert!(r.markdown.contains("ports"), "{}", r.markdown);
    }

    #[test]
    fn csv_keeps_quoted_comma() {
        let r = convert_simple("a.csv", b"a,b\n\"x,y\",z\n");
        assert!(r.markdown.contains("| x,y | z |"), "{}", r.markdown);
    }

    #[test]
    fn txt_convert() {
        let r = convert_simple("a.txt", b"hello world");
        assert!(r.error.is_empty());
        assert_eq!(r.markdown, "hello world");
    }

    #[test]
    fn audio_simple_keeps_bytes() {
        let r = convert_simple("talk.wav", b"RIFF");
        assert!(r.is_audio);
        assert_eq!(r.audio_data, b"RIFF");
        assert!(r.markdown.contains("talk.wav"));
    }

    #[test]
    fn image_simple_keeps_bytes() {
        let r = convert_simple("pic.png", b"\x89PNG");
        assert!(r.markdown.contains("images/pic.png"));
        assert_eq!(r.images.len(), 1);
        assert_eq!(r.images[0].data, b"\x89PNG");
    }

    static ADDR_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[tokio::test]
    async fn missing_addr_is_not_configured() {
        let _g = ADDR_LOCK.lock().await;
        unsafe { std::env::remove_var("DOCREADER_ADDR") };
        let r = convert("builtin", "a.pdf", "pdf", false, b"%PDF".to_vec(), "", "")
            .await
            .unwrap();
        assert_eq!(r.error, NOT_CONFIGURED);
    }

    struct StreamSvc;

    #[tonic::async_trait]
    impl DocReader for StreamSvc {
        type ReadStreamStream = ReceiverStream<Result<ReadStreamResponse, Status>>;

        async fn read(&self, _req: Request<ReadRequest>) -> Result<Response<ReadResponse>, Status> {
            Err(Status::unimplemented("no unary"))
        }

        async fn read_stream(
            &self,
            _req: Request<ReadRequest>,
        ) -> Result<Response<Self::ReadStreamStream>, Status> {
            let (tx, rx) = tokio::sync::mpsc::channel(4);
            tx.send(Ok(ReadStreamResponse {
                payload: Some(crate::proto::read_stream_response::Payload::Meta(
                    ReadStreamMeta {
                        markdown_content: "# hi\n\nbody".into(),
                        error: String::new(),
                        image_count: 1,
                        metadata: std::collections::HashMap::from([(
                            "image_source_type".into(),
                            "scanned_pdf".into(),
                        )]),
                        ..Default::default()
                    },
                )),
            }))
            .await
            .unwrap();
            tx.send(Ok(ReadStreamResponse {
                payload: Some(crate::proto::read_stream_response::Payload::Image(
                    ProtoImage {
                        filename: "p.png".into(),
                        original_ref: "images/p.png".into(),
                        image_data: vec![1, 2, 3],
                        ..Default::default()
                    },
                )),
            }))
            .await
            .unwrap();
            Ok(Response::new(ReceiverStream::new(rx)))
        }

        async fn list_engines(
            &self,
            _req: Request<ListEnginesRequest>,
        ) -> Result<Response<ListEnginesResponse>, Status> {
            Ok(Response::new(ListEnginesResponse { engines: vec![] }))
        }
    }

    struct UnaryOnly;

    #[tonic::async_trait]
    impl DocReader for UnaryOnly {
        type ReadStreamStream = ReceiverStream<Result<ReadStreamResponse, Status>>;

        async fn read(&self, _req: Request<ReadRequest>) -> Result<Response<ReadResponse>, Status> {
            Ok(Response::new(ReadResponse {
                markdown_content: "unary md".into(),
                metadata: std::collections::HashMap::from([(
                    "image_source_type".into(),
                    "scanned_pdf".into(),
                )]),
                ..Default::default()
            }))
        }

        async fn read_stream(
            &self,
            _req: Request<ReadRequest>,
        ) -> Result<Response<Self::ReadStreamStream>, Status> {
            Err(Status::unimplemented("old server"))
        }

        async fn list_engines(
            &self,
            _req: Request<ListEnginesRequest>,
        ) -> Result<Response<ListEnginesResponse>, Status> {
            Ok(Response::new(ListEnginesResponse { engines: vec![] }))
        }
    }

    async fn serve(svc: impl DocReader) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(DocReaderServer::new(svc))
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
                .await
                .unwrap();
        });
        let dest = format!("127.0.0.1:{}", addr.port());
        for _ in 0..50 {
            if tokio::net::TcpStream::connect(&dest).await.is_ok() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        dest
    }

    struct HangAfterImages;

    #[tonic::async_trait]
    impl DocReader for HangAfterImages {
        type ReadStreamStream = ReceiverStream<Result<ReadStreamResponse, Status>>;

        async fn read(&self, _req: Request<ReadRequest>) -> Result<Response<ReadResponse>, Status> {
            Err(Status::unimplemented("no unary"))
        }

        async fn read_stream(
            &self,
            _req: Request<ReadRequest>,
        ) -> Result<Response<Self::ReadStreamStream>, Status> {
            let (tx, rx) = tokio::sync::mpsc::channel(4);
            tx.send(Ok(ReadStreamResponse {
                payload: Some(crate::proto::read_stream_response::Payload::Meta(
                    ReadStreamMeta {
                        markdown_content: "# hi\n\nbody".into(),
                        error: String::new(),
                        image_count: 1,
                        ..Default::default()
                    },
                )),
            }))
            .await
            .unwrap();
            tx.send(Ok(ReadStreamResponse {
                payload: Some(crate::proto::read_stream_response::Payload::Image(
                    ProtoImage {
                        filename: "p.png".into(),
                        original_ref: "images/p.png".into(),
                        image_data: vec![1, 2, 3],
                        ..Default::default()
                    },
                )),
            }))
            .await
            .unwrap();
            std::mem::forget(tx);
            Ok(Response::new(ReceiverStream::new(rx)))
        }

        async fn list_engines(
            &self,
            _req: Request<ListEnginesRequest>,
        ) -> Result<Response<ListEnginesResponse>, Status> {
            Ok(Response::new(ListEnginesResponse { engines: vec![] }))
        }
    }

    #[tokio::test]
    async fn readstream_returns_after_image_count_without_eos() {
        let addr = serve(HangAfterImages).await;
        let _g = ADDR_LOCK.lock().await;
        unsafe { std::env::set_var("DOCREADER_ADDR", &addr) };
        let r = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            convert("builtin", "a.pdf", "pdf", false, b"%PDF".to_vec(), "", ""),
        )
        .await
        .expect("must not wait for stream EOS after image_count")
        .unwrap();
        unsafe { std::env::remove_var("DOCREADER_ADDR") };
        assert_eq!(r.markdown, "# hi\n\nbody");
        assert_eq!(r.images.len(), 1);
        assert_eq!(r.images[0].data, vec![1, 2, 3]);
    }

    struct IncompleteImages;

    #[tonic::async_trait]
    impl DocReader for IncompleteImages {
        type ReadStreamStream = ReceiverStream<Result<ReadStreamResponse, Status>>;

        async fn read(&self, _req: Request<ReadRequest>) -> Result<Response<ReadResponse>, Status> {
            Err(Status::unimplemented("no unary"))
        }

        async fn read_stream(
            &self,
            _req: Request<ReadRequest>,
        ) -> Result<Response<Self::ReadStreamStream>, Status> {
            let (tx, rx) = tokio::sync::mpsc::channel(4);
            tx.send(Ok(ReadStreamResponse {
                payload: Some(crate::proto::read_stream_response::Payload::Meta(
                    ReadStreamMeta {
                        markdown_content: "# hi".into(),
                        error: String::new(),
                        image_count: 2,
                        ..Default::default()
                    },
                )),
            }))
            .await
            .unwrap();
            tx.send(Ok(ReadStreamResponse {
                payload: Some(crate::proto::read_stream_response::Payload::Image(
                    ProtoImage {
                        filename: "a.png".into(),
                        image_data: vec![1],
                        ..Default::default()
                    },
                )),
            }))
            .await
            .unwrap();
            drop(tx);
            Ok(Response::new(ReceiverStream::new(rx)))
        }

        async fn list_engines(
            &self,
            _req: Request<ListEnginesRequest>,
        ) -> Result<Response<ListEnginesResponse>, Status> {
            Ok(Response::new(ListEnginesResponse { engines: vec![] }))
        }
    }

    struct ZeroCountThenImage;

    #[tonic::async_trait]
    impl DocReader for ZeroCountThenImage {
        type ReadStreamStream = ReceiverStream<Result<ReadStreamResponse, Status>>;

        async fn read(&self, _req: Request<ReadRequest>) -> Result<Response<ReadResponse>, Status> {
            Err(Status::unimplemented("no unary"))
        }

        async fn read_stream(
            &self,
            _req: Request<ReadRequest>,
        ) -> Result<Response<Self::ReadStreamStream>, Status> {
            let (tx, rx) = tokio::sync::mpsc::channel(4);
            tx.send(Ok(ReadStreamResponse {
                payload: Some(crate::proto::read_stream_response::Payload::Meta(
                    ReadStreamMeta {
                        markdown_content: "# z".into(),
                        error: String::new(),
                        image_count: 0,
                        ..Default::default()
                    },
                )),
            }))
            .await
            .unwrap();
            tx.send(Ok(ReadStreamResponse {
                payload: Some(crate::proto::read_stream_response::Payload::Image(
                    ProtoImage {
                        filename: "z.png".into(),
                        image_data: vec![9],
                        ..Default::default()
                    },
                )),
            }))
            .await
            .unwrap();
            drop(tx);
            Ok(Response::new(ReceiverStream::new(rx)))
        }

        async fn list_engines(
            &self,
            _req: Request<ListEnginesRequest>,
        ) -> Result<Response<ListEnginesResponse>, Status> {
            Ok(Response::new(ListEnginesResponse { engines: vec![] }))
        }
    }

    #[tokio::test]
    async fn readstream_incomplete_image_count_is_error() {
        let addr = serve(IncompleteImages).await;
        let _g = ADDR_LOCK.lock().await;
        unsafe { std::env::set_var("DOCREADER_ADDR", &addr) };
        let err = convert("builtin", "a.pdf", "pdf", false, b"%PDF".to_vec(), "", "")
            .await
            .unwrap_err();
        unsafe { std::env::remove_var("DOCREADER_ADDR") };
        assert!(
            err.0.contains("incomplete") && err.0.contains("1 of 2"),
            "{}",
            err.0
        );
    }

    #[tokio::test]
    async fn readstream_zero_count_still_reads_images_until_eos() {
        let addr = serve(ZeroCountThenImage).await;
        let _g = ADDR_LOCK.lock().await;
        unsafe { std::env::set_var("DOCREADER_ADDR", &addr) };
        let r = convert("builtin", "a.pdf", "pdf", false, b"%PDF".to_vec(), "", "")
            .await
            .unwrap();
        unsafe { std::env::remove_var("DOCREADER_ADDR") };
        assert_eq!(r.markdown, "# z");
        assert_eq!(r.images.len(), 1);
        assert_eq!(r.images[0].data, vec![9]);
    }

    #[tokio::test]
    async fn readstream_meta_then_images() {
        let addr = serve(StreamSvc).await;
        let _g = ADDR_LOCK.lock().await;
        unsafe { std::env::set_var("DOCREADER_ADDR", &addr) };
        let r = convert("builtin", "a.pdf", "pdf", false, b"%PDF".to_vec(), "", "")
            .await
            .unwrap();
        unsafe { std::env::remove_var("DOCREADER_ADDR") };
        assert_eq!(r.markdown, "# hi\n\nbody");
        assert!(r.error.is_empty());
        assert_eq!(r.images.len(), 1);
        assert_eq!(r.images[0].data, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn unimplemented_stream_falls_back_to_unary() {
        let addr = serve(UnaryOnly).await;
        let _g = ADDR_LOCK.lock().await;
        unsafe { std::env::set_var("DOCREADER_ADDR", &addr) };
        let r = convert("builtin", "a.pdf", "pdf", false, b"%PDF".to_vec(), "", "")
            .await
            .unwrap();
        unsafe { std::env::remove_var("DOCREADER_ADDR") };
        assert_eq!(r.markdown, "unary md");
        assert_eq!(
            r.metadata.get("image_source_type").map(String::as_str),
            Some("scanned_pdf")
        );
    }

    #[tokio::test]
    async fn readstream_forwards_docreader_metadata() {
        let addr = serve(StreamSvc).await;
        let _g = ADDR_LOCK.lock().await;
        unsafe { std::env::set_var("DOCREADER_ADDR", &addr) };
        let r = convert("builtin", "a.pdf", "pdf", false, b"%PDF".to_vec(), "", "")
            .await
            .unwrap();
        unsafe { std::env::remove_var("DOCREADER_ADDR") };
        assert_eq!(
            r.metadata.get("image_source_type").map(String::as_str),
            Some("scanned_pdf")
        );
        assert_eq!(r.markdown, "# hi\n\nbody");
    }

    #[tokio::test]
    async fn anydoc_office_error_falls_back_to_builtin() {
        let addr = serve(UnaryOnly).await;
        let _g = ADDR_LOCK.lock().await;
        unsafe { std::env::set_var("DOCREADER_ADDR", &addr) };
        let r = convert(
            "anydoc",
            "a.docx",
            "docx",
            false,
            b"not-a-docx".to_vec(),
            "",
            "",
        )
        .await
        .unwrap();
        unsafe { std::env::remove_var("DOCREADER_ADDR") };
        assert_eq!(r.markdown, "unary md");
        assert_eq!(
            r.metadata.get("anydoc_fallback").map(String::as_str),
            Some("office_error")
        );
    }

    #[tokio::test]
    async fn anydoc_success_does_not_call_builtin() {
        let addr = serve(UnaryOnly).await;
        let _g = ADDR_LOCK.lock().await;
        unsafe { std::env::set_var("DOCREADER_ADDR", &addr) };
        let r = convert(
            "anydoc",
            "tender.docx",
            "docx",
            false,
            anydoc::tests::sample_docx_with_table(),
            "",
            "",
        )
        .await
        .unwrap();
        unsafe { std::env::remove_var("DOCREADER_ADDR") };
        assert!(!r.metadata.contains_key("anydoc_fallback"));
        assert_eq!(r.metadata.get("parser").map(String::as_str), Some("anydoc"));
        assert!(
            r.markdown.contains('|') && r.markdown.contains("hot-swap"),
            "{}",
            r.markdown
        );
    }
}
