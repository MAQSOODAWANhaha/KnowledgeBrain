//! Engine routing: simple / anydoc / DocReader gRPC / HTTP.

use crate::grpc::ConvertRequest;
use crate::simple::convert_simple;
use crate::{ConvertError, ConvertInput, ReadResult, anydoc, http_engine};
use platform::is_simple_format;
use tokio_util::sync::CancellationToken;

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

/// Tender freeze path: always DocReader builtin (needs structured_source_units).
pub async fn convert_tender_source(
    file_name: &str,
    bytes: Vec<u8>,
    cancel: &CancellationToken,
) -> Result<ReadResult, ConvertError> {
    let ext = file_name
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(
        ext.as_str(),
        "pdf" | "docx" | "doc" | "xlsx" | "xls" | "xlsm" | "png" | "jpg" | "jpeg" | "webp"
    ) {
        return Err(ConvertError(format!(
            "unsupported tender source extension .{ext}"
        )));
    }
    convert_with_cancel(
        ConvertInput {
            engine: "builtin",
            file_name,
            file_type: &ext,
            is_url: false,
            bytes,
            url: "",
            title: file_name,
            overrides: &std::collections::HashMap::new(),
        },
        cancel,
    )
    .await
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
    convert_with_cancel(input, &CancellationToken::new()).await
}

pub async fn convert_with_cancel(
    input: ConvertInput<'_>,
    cancel: &CancellationToken,
) -> Result<ReadResult, ConvertError> {
    let resolved = resolve_engine(input.engine, input.file_type, input.is_url);
    match resolved {
        "simple" => Ok(convert_simple(input.file_name, &input.bytes)),
        "anydoc" => convert_anydoc(input, cancel).await,
        "docreader" => {
            crate::grpc::read(
                ConvertRequest {
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
                },
                cancel,
            )
            .await
        }
        "http-engine" => {
            http_engine::convert_http(input.engine, input.file_name, input.bytes, cancel).await
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
async fn convert_anydoc(
    input: ConvertInput<'_>,
    cancel: &CancellationToken,
) -> Result<ReadResult, ConvertError> {
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
    if crate::grpc::reader_addr().is_none() {
        return Ok(result);
    }
    tracing::warn!(file = input.file_name, reason, "docparser anydoc fallback");
    fallback_builtin(input, reason, cancel).await
}

async fn fallback_builtin(
    input: ConvertInput<'_>,
    reason: &str,
    cancel: &CancellationToken,
) -> Result<ReadResult, ConvertError> {
    let mut fallback = crate::grpc::read(
        ConvertRequest {
            file_content: input.bytes,
            file_name: input.file_name.to_string(),
            file_type: input.file_type.to_string(),
            url: input.url.to_string(),
            title: input.title.to_string(),
            parser_engine: "builtin".into(),
            parser_engine_overrides: input.overrides.clone(),
        },
        cancel,
    )
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

    static ADDR_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[tokio::test]
    async fn missing_addr_is_not_configured() {
        let _g = ADDR_LOCK.lock().await;
        unsafe { std::env::remove_var("DOCREADER_ADDR") };
        let r = convert("builtin", "a.pdf", "pdf", false, b"%PDF".to_vec(), "", "")
            .await
            .unwrap();
        assert_eq!(r.error, crate::NOT_CONFIGURED);
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

        async fn source_view(
            &self,
            _: Request<crate::proto::SourceViewRequest>,
        ) -> Result<Response<crate::proto::SourceViewResponse>, Status> {
            Err(Status::unimplemented(
                "source views unavailable in this legacy test service",
            ))
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

        async fn source_view(
            &self,
            _: Request<crate::proto::SourceViewRequest>,
        ) -> Result<Response<crate::proto::SourceViewResponse>, Status> {
            Err(Status::unimplemented(
                "source views unavailable in this legacy test service",
            ))
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

        async fn source_view(
            &self,
            _: Request<crate::proto::SourceViewRequest>,
        ) -> Result<Response<crate::proto::SourceViewResponse>, Status> {
            Err(Status::unimplemented(
                "source views unavailable in this legacy test service",
            ))
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

        async fn source_view(
            &self,
            _: Request<crate::proto::SourceViewRequest>,
        ) -> Result<Response<crate::proto::SourceViewResponse>, Status> {
            Err(Status::unimplemented(
                "source views unavailable in this legacy test service",
            ))
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

        async fn source_view(
            &self,
            _: Request<crate::proto::SourceViewRequest>,
        ) -> Result<Response<crate::proto::SourceViewResponse>, Status> {
            Err(Status::unimplemented(
                "source views unavailable in this legacy test service",
            ))
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
            crate::anydoc::tests::sample_docx_with_table(),
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

    struct HangForever;

    #[tonic::async_trait]
    impl DocReader for HangForever {
        type ReadStreamStream = ReceiverStream<Result<ReadStreamResponse, Status>>;

        async fn read(&self, _req: Request<ReadRequest>) -> Result<Response<ReadResponse>, Status> {
            Err(Status::unimplemented("no unary"))
        }

        async fn read_stream(
            &self,
            _req: Request<ReadRequest>,
        ) -> Result<Response<Self::ReadStreamStream>, Status> {
            let (tx, rx) = tokio::sync::mpsc::channel(1);
            std::mem::forget(tx);
            Ok(Response::new(ReceiverStream::new(rx)))
        }

        async fn source_view(
            &self,
            _: Request<crate::proto::SourceViewRequest>,
        ) -> Result<Response<crate::proto::SourceViewResponse>, Status> {
            Err(Status::unimplemented("unused"))
        }

        async fn list_engines(
            &self,
            _req: Request<ListEnginesRequest>,
        ) -> Result<Response<ListEnginesResponse>, Status> {
            Ok(Response::new(ListEnginesResponse { engines: vec![] }))
        }
    }

    #[tokio::test]
    async fn anydoc_fallback_honors_cancel_before_frame_idle() {
        let addr = serve(HangForever).await;
        let _g = ADDR_LOCK.lock().await;
        unsafe { std::env::set_var("DOCREADER_ADDR", &addr) };
        let cancel = CancellationToken::new();
        cancel.cancel();
        let empty = std::collections::HashMap::new();
        let input = ConvertInput {
            engine: "anydoc",
            file_name: "a.docx",
            file_type: "docx",
            is_url: false,
            bytes: b"not-a-docx".to_vec(),
            url: "",
            title: "",
            overrides: &empty,
        };
        let timed = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            convert_with_cancel(input, &cancel),
        )
        .await;
        unsafe { std::env::remove_var("DOCREADER_ADDR") };
        let err = timed
            .expect("must return before FRAME_IDLE")
            .expect_err("cancelled convert");
        assert!(
            err.0.contains("cancelled"),
            "expected cancelled, got {}",
            err.0
        );
    }
}
