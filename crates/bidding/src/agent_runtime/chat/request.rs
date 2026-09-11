//! Request preparation uses Rig's public compatible-provider builder and HTTP
//! seam. This client has no network backend; only the reserved bytes are sent.
use super::*;
use crate::authoring_runtime::AuthoringRuntimeContractV1;
use rig::{
    client::{Client, ClientBuilder, DebugExt, Nothing, Provider, ProviderBuilder},
    completion::{CompletionModel, CompletionRequest, ToolDefinition},
    message::{Message, ToolChoice},
    providers::openai::completion::{
        self as wire, GenericCompletionModel, OpenAICompatibleProvider,
    },
};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tracing::instrument::WithSubscriber;

pub(crate) fn default_context_tokens() -> usize {
    131072
}
pub(crate) fn default_image_token_reserve() -> usize {
    16384
}
pub(crate) fn default_token_safety_margin() -> usize {
    4096
}

/// Conservative application estimate, not a provider tokenizer. Base64 is
/// transport encoding; each image instead consumes its configured allowance.
pub(crate) fn estimate_input_tokens(
    body: &Value,
    image_token_reserve: usize,
    token_safety_margin: usize,
) -> Result<usize, AgentError> {
    let overflow = || AgentError::new("AGENT_OUTPUT_INVALID", "context token estimate overflow");
    let mut text = body.clone();
    let mut reserve = token_safety_margin;
    if let Some(messages) = text["messages"].as_array_mut() {
        for message in messages {
            if let Some(parts) = message["content"].as_array_mut() {
                for part in parts {
                    if part["type"] == "image_url" {
                        part["image_url"]["url"] = json!("");
                        reserve = reserve
                            .checked_add(image_token_reserve)
                            .ok_or_else(overflow)?;
                    }
                }
            }
        }
    }
    serde_json_canonicalizer::to_vec(&text)
        .map_err(|e| AgentError::new("AGENT_OUTPUT_INVALID", e.to_string()))?
        .len()
        .checked_add(reserve)
        .ok_or_else(overflow)
}

/// The frozen compatible Chat contract uses max_tokens for every configured
/// model. Use the SDK's compatible-provider default, without name heuristics.
#[derive(Debug, Default, Clone)]
struct FrozenChat;
impl Provider for FrozenChat {
    type Builder = Self;
    const VERIFY_PATH: &'static str = "/models";
}
impl ProviderBuilder for FrozenChat {
    type Extension<H>
        = Self
    where
        H: HttpClientExt;
    type ApiKey = Nothing;
    const BASE_URL: &'static str = "";
    fn build<H: HttpClientExt>(_: &ClientBuilder<Self, Nothing, H>) -> http_client::Result<Self> {
        Ok(Self)
    }
}
impl DebugExt for FrozenChat {}
impl OpenAICompatibleProvider for FrozenChat {
    const PROVIDER_NAME: &'static str = "openai-chat-compatible";
    type StreamingUsage = wire::Usage;
    type Response = wire::CompletionResponse;
}

#[derive(Clone, Default, Debug)]
struct Capture(Option<mpsc::Sender<Result<Vec<u8>, AgentError>>>);
impl HttpClientExt for Capture {
    fn send<T, U>(
        &self,
        _: Request<T>,
    ) -> impl Future<Output = http_client::Result<Response<LazyBody<U>>>> + Send + 'static
    where
        T: Into<Bytes> + Send,
        U: From<Bytes> + Send + 'static,
    {
        std::future::ready(Err(http_unavailable()))
    }
    fn send_multipart<U>(
        &self,
        _: Request<MultipartForm>,
    ) -> impl Future<Output = http_client::Result<Response<LazyBody<U>>>> + Send + 'static
    where
        U: From<Bytes> + Send + 'static,
    {
        std::future::ready(Err(http_unavailable()))
    }
    async fn send_streaming<T>(&self, request: Request<T>) -> http_client::Result<StreamingResponse>
    where
        T: Into<Bytes> + Send,
    {
        let tx = self.0.as_ref().ok_or_else(http_unavailable)?;
        let body: Bytes = request.into_body().into();
        let canonical = serde_json::from_slice::<Value>(&body)
            .and_then(|body| serde_json_canonicalizer::to_vec(&body))
            .map_err(|_| invalid());
        tx.send(canonical).await.map_err(|_| http_unavailable())?;
        // The caller drops this suspended stream once it has the body. There
        // is no task, connection, retry, credential or reservation to leak.
        std::future::pending().await
    }
}

pub(crate) fn system_content(prompt: &str) -> Value {
    serde_json::to_value(wire::Message::system(prompt)).expect("SDK system message is serializable")
        ["content"]
        .clone()
}

pub(crate) async fn prepare(
    runtime: &AuthoringRuntimeContractV1,
    messages: Vec<Value>,
    tools: Vec<Value>,
) -> Result<Vec<u8>, AgentError> {
    let history = messages
        .into_iter()
        .map(|value| {
            if value["role"] == "system" {
                return value["content"]
                    .as_str()
                    .map(Message::system)
                    .ok_or_else(invalid);
            }
            let wire: wire::Message = serde_json::from_value(value).map_err(|_| invalid())?;
            Message::try_from(wire).map_err(|_| invalid())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let tools = tools
        .into_iter()
        .map(|tool| {
            serde_json::from_value::<ToolDefinition>(tool["function"].clone())
                .map_err(|_| invalid())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let request = CompletionRequest {
        model: None,
        preamble: None,
        chat_history: history,
        documents: vec![],
        tools,
        temperature: None,
        max_tokens: Some(runtime.max_tokens.into()),
        tool_choice: Some(ToolChoice::Required),
        additional_params: runtime
            .reasoning_effort
            .as_ref()
            .map(|value| json!({"reasoning_effort":value})),
        output_schema: None,
        record_telemetry_content: false,
    };
    let (tx, mut rx) = mpsc::channel(1);
    let capture = Capture(Some(tx));
    let base = runtime
        .endpoint
        .strip_suffix("/chat/completions")
        .ok_or_else(unavailable)?;
    let client = Client::<FrozenChat>::builder()
        .api_key(Nothing)
        .base_url(base)
        .http_client(capture)
        .build()
        .map_err(|_| unavailable())?;
    let model = GenericCompletionModel::new(client, &runtime.model_id);
    async {
        let mut stream = model.stream(request).await.map_err(response_error)?;
        tokio::select! {
            body = rx.recv() => body.ok_or_else(invalid)?,
            _ = stream.next() => Err(invalid()),
        }
    }
    // Rig's trace-level request logger includes full content. Preparation is
    // silent; the host emits only its existing size/timing metadata afterward.
    .with_subscriber(tracing::subscriber::NoSubscriber::default())
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sdk_preparation_preserves_frozen_fields_tools_images_and_arbitrary_model_names() {
        let tools = vec![
            json!({"type":"function","function":{"name":"inspect","description":"Exact source lookup","parameters":{"type":"object","properties":{"id":{"type":"string"}},"required":["id"],"additionalProperties":false}}}),
        ];
        let messages = vec![
            json!({"role":"system","content":"按来源提取，不编造"}),
            json!({"role":"user","content":"项目身份"}),
            json!({"role":"assistant","content":null,"tool_calls":[{"id":"saved-call","type":"function","function":{"name":"inspect","arguments":"{\"id\":\"来源甲\"}"}}]}),
            json!({"role":"tool","tool_call_id":"saved-call","content":"完整原文结果"}),
            json!({"role":"user","content":[{"type":"text","text":"原页"},{"type":"image_url","image_url":{"url":"data:image/jpeg;base64,AAAA","detail":"high"}}]}),
        ];
        for model in ["configured-model", "gpt-5.6-sol", "arbitrary/vendor/model"] {
            let runtime: AuthoringRuntimeContractV1 = serde_json::from_value(json!({
                "schema_version":1,"base_url":"https://model.example.invalid/custom/v1",
                "endpoint":"https://model.example.invalid/custom/v1/chat/completions",
                "protocol":"openai_chat_completions_sse","model_id":model,
                "credential_ref":"env:LLM_API_KEY","stream":true,"max_tokens":8192,"timeout_ms":90000,
                "response_mode":"tool_calls","transport_retries":0,"temperature":null,"reasoning_effort":"medium"
            })).unwrap();
            let bytes = prepare(&runtime, messages.clone(), tools.clone())
                .await
                .unwrap();
            let body: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(body["model"], model);
            assert_eq!(body["max_tokens"], runtime.max_tokens);
            assert!(body.get("max_completion_tokens").is_none());
            assert_eq!(body["reasoning_effort"], "medium");
            assert_eq!(body["tool_choice"], "required");
            assert_eq!(body["stream_options"], json!({"include_usage":true}));
            assert_eq!(body["tools"], json!(tools));
            assert_eq!(body["messages"][0]["role"], "system");
            assert_eq!(
                body["messages"][0]["content"],
                system_content("按来源提取，不编造")
            );
            assert_eq!(body["messages"][2]["tool_calls"][0]["id"], "saved-call");
            assert_eq!(body["messages"][3], messages[3]);
            assert_eq!(body["messages"][4], messages[4]);
            assert_eq!(bytes, serde_json_canonicalizer::to_vec(&body).unwrap());
            assert_eq!(
                bytes,
                prepare(&runtime, messages.clone(), tools.clone())
                    .await
                    .unwrap()
            );
        }
    }
}
