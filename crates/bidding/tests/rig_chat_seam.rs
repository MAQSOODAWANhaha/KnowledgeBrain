//! SDK seam gate only: a private loopback server captures actual bytes. No
//! real documents, environment credentials, provider calls or business tools.
use bytes::Bytes;
use futures::StreamExt;
use rig::{
    completion::{CompletionModel, CompletionRequest, ToolDefinition},
    http_client::{
        self, HttpClientExt, LazyBody, MultipartForm, Request, Response, StreamingResponse,
    },
    message::{AssistantContent, ImageDetail, ImageMediaType, Message, ToolChoice, UserContent},
    providers::openai::CompletionsClient,
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

#[derive(Clone, Debug)]
struct Gate {
    client: reqwest::Client,
    reserved: Arc<Mutex<Vec<Vec<u8>>>>,
    entries: Arc<AtomicUsize>,
    reject: bool,
}
impl Default for Gate {
    fn default() -> Self {
        Self {
            client: reqwest::Client::new(),
            reserved: Arc::new(Mutex::new(vec![])),
            entries: Arc::new(AtomicUsize::new(0)),
            reject: true,
        }
    }
}
impl HttpClientExt for Gate {
    fn send<T, U>(
        &self,
        _: Request<T>,
    ) -> impl Future<Output = http_client::Result<Response<LazyBody<U>>>> + Send + 'static
    where
        T: Into<Bytes> + Send,
        U: From<Bytes> + Send + 'static,
    {
        std::future::ready(Err(http_client::Error::StreamEnded))
    }
    fn send_multipart<U>(
        &self,
        _: Request<MultipartForm>,
    ) -> impl Future<Output = http_client::Result<Response<LazyBody<U>>>> + Send + 'static
    where
        U: From<Bytes> + Send + 'static,
    {
        std::future::ready(Err(http_client::Error::StreamEnded))
    }
    async fn send_streaming<T>(&self, req: Request<T>) -> http_client::Result<StreamingResponse>
    where
        T: Into<Bytes> + Send,
    {
        self.entries.fetch_add(1, Ordering::SeqCst);
        let (mut parts, body) = req.into_parts();
        let bytes: Bytes = body.into();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        let canonical = serde_json_canonicalizer::to_vec(&value).unwrap();
        assert!(parts.uri.path().ends_with("/chat/completions"));
        assert_eq!(value["tool_choice"], "required");
        assert_eq!(value["stream_options"], json!({"include_usage":true}));
        assert_eq!(value["messages"][0]["role"], "system");
        assert_eq!(value["messages"][2]["tool_calls"][0]["id"], "prior-call");
        assert_eq!(value["messages"][3]["tool_call_id"], "prior-call");
        assert_eq!(
            value["messages"][4]["content"][1]["image_url"],
            json!({"url":"data:image/jpeg;base64,AAAA","detail":"high"})
        );
        if self.reject {
            return Err(http_client::Error::Instance(
                std::io::Error::other("reservation denied").into(),
            ));
        }
        self.reserved.lock().unwrap().push(canonical.clone());
        // SDK did the only provider serialization. Reservation and physical send
        // consume this one canonical body; no second SDK request construction.
        parts.headers.remove("content-length");
        self.client
            .send_streaming(Request::from_parts(parts, Bytes::from(canonical)))
            .await
    }
}
fn request() -> CompletionRequest {
    CompletionRequest {
        model: None,
        preamble: None,
        chat_history: vec![
            Message::system("Read source evidence; use the registered tool."),
            Message::user("中文证据"),
            Message::Assistant {
                id: None,
                content: vec![AssistantContent::tool_call(
                    "prior-call",
                    "inspect",
                    json!({"id":"a"}),
                )],
            },
            Message::tool_result("prior-call", "inspect", "{\"evidence\":\"中文\"}"),
            Message::User {
                content: vec![
                    UserContent::text("Continue."),
                    UserContent::image_base64(
                        "AAAA",
                        Some(ImageMediaType::JPEG),
                        Some(ImageDetail::High),
                    ),
                ],
            },
        ],
        documents: vec![],
        tools: vec![ToolDefinition {
            name: "inspect".into(),
            description: "Read one source ID".into(),
            parameters: json!({"type":"object","properties":{"id":{"type":"string"}},"required":["id"],"additionalProperties":false}),
        }],
        temperature: None,
        max_tokens: Some(8192),
        tool_choice: Some(ToolChoice::Required),
        additional_params: None,
        output_schema: None,
        record_telemetry_content: false,
    }
}
fn chunk(value: Value) -> String {
    format!("data: {value}\n\n")
}
async fn case(name: &str, reject: bool, status: u16, body: String, hold: bool) -> Value {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let received = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
    let captured = received.clone();
    let server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let captured = captured.clone();
            let body = body.clone();
            tokio::spawn(async move {
                let mut bytes = vec![];
                let mut block = [0u8; 4096];
                let end = loop {
                    let n = socket.read(&mut block).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&block[..n]);
                    if let Some(i) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        break i + 4;
                    }
                };
                let header = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
                let size: usize = header
                    .lines()
                    .find_map(|s| s.strip_prefix("content-length:"))
                    .unwrap()
                    .trim()
                    .parse()
                    .unwrap();
                while bytes.len() < end + size {
                    let n = socket.read(&mut block).await.unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&block[..n]);
                }
                captured
                    .lock()
                    .unwrap()
                    .push(bytes[end..end + size].to_vec());
                let length = if hold { body.len() + 10000 } else { body.len() };
                let header = format!(
                    "HTTP/1.1 {status} Test\r\nContent-Type: text/event-stream\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n"
                );
                socket.write_all(header.as_bytes()).await.unwrap();
                socket.write_all(body.as_bytes()).await.unwrap();
                if hold {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            });
        }
    });
    let reserved = Arc::new(Mutex::new(vec![]));
    let entries = Arc::new(AtomicUsize::new(0));
    let gate = Gate {
        client: reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .build()
            .unwrap(),
        reserved: reserved.clone(),
        entries: entries.clone(),
        reject,
    };
    let client = CompletionsClient::builder()
        .api_key("local-fixture")
        .base_url(format!("http://{addr}/v1"))
        .http_client(gate)
        .build()
        .unwrap();
    let model =
        rig::providers::openai::completion::CompletionModel::new(client, "fixture-chat-model");
    let mut stream = CompletionModel::stream(&model, request()).await.unwrap();
    let mut events = vec![];
    let mut errors = 0;
    let consume = async {
        while let Some(event) = stream.next().await {
            match event {
                Ok(e) => {
                    events.push(format!("{e:?}"));
                }
                Err(e) => {
                    errors += 1;
                    events.push(format!("ERROR {e}"));
                }
            }
        }
    };
    let timed_out = tokio::time::timeout(
        Duration::from_millis(if hold { 200 } else { 3000 }),
        consume,
    )
    .await
    .is_err();
    let finish=stream.response.as_ref().map(|r|json!({"finish":format!("{:?}",r.finish_reason),"raw":r.raw,"reported_usage":if r.usage.has_values(){Some(r.usage)}else{None}}));
    let calls = stream
        .choice
        .iter()
        .filter(|c| matches!(c, AssistantContent::ToolCall(_)))
        .count();
    let accepted = stream
        .response
        .as_ref()
        .is_some_and(|r| r.finish_reason == Some(rig::completion::FinishReason::ToolCalls))
        && calls > 0
        && errors == 0
        && !timed_out;
    if name == "valid" || name == "missing_usage" {
        assert!(accepted, "valid turn rejected: {name}");
        assert_eq!(calls, 1);
    } else {
        assert!(!accepted, "incomplete turn accepted: {name}");
    }
    drop(stream);
    tokio::time::sleep(Duration::from_millis(100)).await;
    server.abort();
    let sent = received.lock().unwrap().clone();
    let booked = reserved.lock().unwrap().clone();
    assert_eq!(
        sent, booked,
        "reserved bytes differ from received bytes: {name}"
    );
    assert_eq!(
        entries.load(Ordering::SeqCst),
        1,
        "SDK retries internally: {name}"
    );
    assert_eq!(
        sent.len(),
        usize::from(!reject),
        "unexpected physical send count: {name}"
    );
    if reject || status != 200 {
        assert!(errors > 0 && finish.is_none());
    }
    if hold {
        assert!(timed_out && finish.is_none());
    }
    json!({"case":name,"physical_sends":sent.len(),"seam_entries":entries.load(Ordering::SeqCst),"request_bytes":sent.first().map(Vec::len),"byte_identity":true,"errors":errors,"timed_out":timed_out,"accepted":accepted,"tool_calls":calls,"final":finish,"events":events})
}
#[tokio::test]
#[ignore = "requires an owned loopback HTTP listener; no external calls"]
async fn rig_chat_seam_preserves_bytes_and_rejects_incomplete_turns() {
    let tool = chunk(
        json!({"id":"r","object":"chat.completion.chunk","created":1,"model":"fixture-chat-model","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call-1","type":"function","function":{"name":"inspect","arguments":"{\"id\":\"a\"}"}}]},"finish_reason":null}]}),
    );
    let finish = chunk(
        json!({"id":"r","object":"chat.completion.chunk","created":1,"model":"fixture-chat-model","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
    );
    let usage = chunk(
        json!({"id":"r","choices":[],"usage":{"prompt_tokens":123,"completion_tokens":8,"total_tokens":131,"prompt_tokens_details":{"cached_tokens":20},"completion_tokens_details":{"reasoning_tokens":3}}}),
    );
    let valid = format!("{tool}{finish}{usage}data: [DONE]\n\n");
    let mut results = vec![];
    for (name, reject, status, body, hold) in [
        ("valid", false, 200, valid.clone(), false),
        ("reservation_denied", true, 200, valid, false),
        ("http_503", false, 503, "unavailable".into(), false),
        ("premature_eof", false, 200, tool.clone(), false),
        (
            "done_without_finish",
            false,
            200,
            format!("{tool}data: [DONE]\n\n"),
            false,
        ),
        (
            "missing_usage",
            false,
            200,
            format!("{tool}{finish}data: [DONE]\n\n"),
            false,
        ),
        ("cancel_midstream", false, 200, tool, true),
    ] {
        results.push(case(name, reject, status, body, hold).await);
    }
    println!("{}", serde_json::to_string_pretty(&results).unwrap());
}
