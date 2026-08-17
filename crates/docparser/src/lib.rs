//! Convert: simple formats in-process; others via DocReader gRPC ReadStream.

mod asr;
mod grpc;
mod http_engine;
mod images;

pub use asr::{ASR_NOT_CONFIGURED, AsrSettings, apply as apply_asr, apply_stub as apply_asr_stub};
pub use grpc::{ConvertRequest, DOCREADER_TIMEOUT, reader_addr};
pub use images::{rewrite_images, rewrite_inline};

use domain::{is_audio_type, is_image_type, is_simple_format};

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

#[derive(Debug, Clone, Default)]
pub struct ReadResult {
    pub markdown: String,
    pub error: String,
    pub images: Vec<ImageRef>,
    pub is_audio: bool,
    pub audio_data: Vec<u8>,
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
            is_audio: true,
            audio_data: bytes.to_vec(),
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
    let resolved = resolve_engine(engine, file_type, is_url);
    match resolved {
        "simple" => Ok(convert_simple(file_name, &bytes)),
        "docreader" => {
            grpc::read(ConvertRequest {
                file_content: if is_url { Vec::new() } else { bytes },
                file_name: file_name.to_string(),
                file_type: file_type.to_string(),
                url: url.to_string(),
                title: title.to_string(),
                parser_engine: if engine.is_empty() {
                    String::new()
                } else {
                    engine.to_string()
                },
            })
            .await
        }
        "http-engine" => http_engine::convert_http(engine, file_name, bytes).await,
        other => Ok(ReadResult {
            error: format!("unknown convert engine {other}"),
            ..ReadResult::default()
        }),
    }
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
    fn builtin_never_falls_back_to_simple() {
        assert_eq!(resolve_engine("builtin", "md", false), "docreader");
        assert_eq!(resolve_engine("", "md", false), "simple");
        assert_eq!(resolve_engine("", "pdf", false), "docreader");
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
    }
}
