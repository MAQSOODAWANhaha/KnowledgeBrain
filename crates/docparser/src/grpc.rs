//! gRPC DocReader client: ReadStream first, unary Read if Unimplemented.

use std::time::Duration;

use tokio::time::timeout;
use tonic::Code;
use tonic::metadata::MetadataValue;
use tonic::service::Interceptor;
use tonic::transport::{Channel, ClientTlsConfig};

use crate::proto::doc_reader_client::DocReaderClient;
use crate::proto::{ImageRef as ProtoImage, ReadConfig, ReadRequest, ReadStreamResponse};
use crate::{ConvertError, ImageRef, NOT_CONFIGURED, ReadResult};

pub const DOCREADER_TIMEOUT: Duration = Duration::from_secs(30 * 60);

pub fn reader_addr() -> Option<String> {
    let v = std::env::var("DOCREADER_ADDR").unwrap_or_default();
    let t = v.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

pub fn max_message_size() -> usize {
    std::env::var("MAX_FILE_SIZE_MB")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(50)
        * 1024
        * 1024
}

pub fn endpoint_url(addr: &str) -> String {
    if addr.starts_with("http://") || addr.starts_with("https://") {
        addr.to_string()
    } else if tls_enabled() {
        format!("https://{addr}")
    } else {
        format!("http://{addr}")
    }
}

fn tls_enabled() -> bool {
    let v = std::env::var("GRPC_TLS_ENABLED").unwrap_or_default();
    matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

fn auth_token() -> Option<MetadataValue<tonic::metadata::Ascii>> {
    let raw = std::env::var("GRPC_AUTH_TOKEN").unwrap_or_default();
    let t = raw.trim();
    if t.is_empty() {
        return None;
    }
    format!("Bearer {t}").parse().ok()
}

#[derive(Clone)]
struct AuthInterceptor {
    token: Option<MetadataValue<tonic::metadata::Ascii>>,
}

impl Interceptor for AuthInterceptor {
    fn call(&mut self, mut req: tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status> {
        if let Some(t) = &self.token {
            req.metadata_mut().insert("authorization", t.clone());
        }
        Ok(req)
    }
}

type Client =
    DocReaderClient<tonic::service::interceptor::InterceptedService<Channel, AuthInterceptor>>;

pub struct ConvertRequest {
    pub file_content: Vec<u8>,
    pub file_name: String,
    pub file_type: String,
    pub url: String,
    pub title: String,
    pub parser_engine: String,
}

pub async fn read(req: ConvertRequest) -> Result<ReadResult, ConvertError> {
    let Some(addr) = reader_addr() else {
        return Ok(ReadResult {
            error: NOT_CONFIGURED.into(),
            ..ReadResult::default()
        });
    };
    let fut = read_inner(&addr, req);
    match timeout(DOCREADER_TIMEOUT, fut).await {
        Ok(r) => r,
        Err(_) => Err(ConvertError(format!(
            "docreader call timeout after {:?}",
            DOCREADER_TIMEOUT
        ))),
    }
}

async fn connect(addr: &str) -> Result<Client, ConvertError> {
    let url = endpoint_url(addr);
    let mut endpoint =
        Channel::from_shared(url.clone()).map_err(|e| ConvertError(e.to_string()))?;
    if url.starts_with("https://") || tls_enabled() {
        let tls = ClientTlsConfig::new().with_native_roots();
        endpoint = endpoint
            .tls_config(tls)
            .map_err(|e| ConvertError(e.to_string()))?;
    }
    let channel = endpoint
        .connect()
        .await
        .map_err(|e| ConvertError(format!("failed to connect to docreader: {e}")))?;
    let max = max_message_size();
    Ok(DocReaderClient::with_interceptor(
        channel,
        AuthInterceptor {
            token: auth_token(),
        },
    )
    .max_decoding_message_size(max)
    .max_encoding_message_size(max))
}

async fn read_inner(addr: &str, req: ConvertRequest) -> Result<ReadResult, ConvertError> {
    let mut client = connect(addr).await?;
    let proto_req = ReadRequest {
        file_content: req.file_content,
        file_name: req.file_name,
        file_type: req.file_type,
        url: req.url,
        title: req.title,
        request_id: uuid::Uuid::new_v4().to_string(),
        config: Some(ReadConfig {
            parser_engine: req.parser_engine,
            parser_engine_overrides: Default::default(),
        }),
    };
    match read_stream(&mut client, proto_req.clone()).await {
        Ok(r) => Ok(r),
        Err(e) if e.msg.contains("unimplemented") || e.code == Some(Code::Unimplemented) => {
            read_unary(&mut client, proto_req).await
        }
        Err(e) => Err(ConvertError(e.msg)),
    }
}

struct StreamErr {
    code: Option<Code>,
    msg: String,
}

async fn read_stream(client: &mut Client, req: ReadRequest) -> Result<ReadResult, StreamErr> {
    let mut stream = client
        .read_stream(req)
        .await
        .map_err(map_status)?
        .into_inner();
    let mut result = ReadResult::default();
    let mut got_meta = false;
    while let Some(frame) = stream.message().await.map_err(map_status)? {
        apply_frame(&mut result, &mut got_meta, frame);
    }
    if !got_meta {
        return Err(StreamErr {
            code: None,
            msg: "gRPC ReadStream returned no metadata frame".into(),
        });
    }
    Ok(result)
}

async fn read_unary(client: &mut Client, req: ReadRequest) -> Result<ReadResult, ConvertError> {
    let resp = client
        .read(req)
        .await
        .map_err(|e| ConvertError(format!("gRPC Read failed: {e}")))?
        .into_inner();
    Ok(ReadResult {
        markdown: resp.markdown_content,
        error: resp.error,
        images: resp.image_refs.into_iter().map(from_proto_image).collect(),
        ..ReadResult::default()
    })
}

fn apply_frame(result: &mut ReadResult, got_meta: &mut bool, frame: ReadStreamResponse) {
    match frame.payload {
        Some(crate::proto::read_stream_response::Payload::Meta(meta)) => {
            *got_meta = true;
            result.markdown = meta.markdown_content;
            result.error = meta.error;
            if meta.image_count > 0 {
                result.images.reserve(meta.image_count as usize);
            }
        }
        Some(crate::proto::read_stream_response::Payload::Image(img)) => {
            result.images.push(from_proto_image(img));
        }
        None => {}
    }
}

fn from_proto_image(img: ProtoImage) -> ImageRef {
    ImageRef {
        filename: img.filename,
        original_ref: img.original_ref,
        mime_type: img.mime_type,
        storage_key: img.storage_key,
        data: img.image_data,
    }
}

fn map_status(s: tonic::Status) -> StreamErr {
    StreamErr {
        code: Some(s.code()),
        msg: format!("gRPC ReadStream failed: {s}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_adds_scheme_and_tls() {
        unsafe { std::env::remove_var("GRPC_TLS_ENABLED") };
        assert_eq!(endpoint_url("127.0.0.1:50051"), "http://127.0.0.1:50051");
        assert_eq!(
            endpoint_url("https://reader.example"),
            "https://reader.example"
        );
        unsafe { std::env::set_var("GRPC_TLS_ENABLED", "true") };
        assert_eq!(endpoint_url("reader:50051"), "https://reader:50051");
        unsafe { std::env::remove_var("GRPC_TLS_ENABLED") };
    }
}
