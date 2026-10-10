//! Local, reproducible token accounting. The pinned library embeds its BPE
//! vocabularies; neither configuration nor counting downloads a tokenizer.
//!
//! Text token counts are exact for the selected encoding, not provider billing
//! counts: server-side chat/tool framing and vision preprocessing are not public
//! wire JSON. We count the entire canonical request (without image URLs) and add
//! explicit framing, image and safety reserves. Unsupported models require a
//! named, model-bound calibration rather than a silently guessed tokenizer.
use crate::agent_error::AgentError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tiktoken_rs::CoreBPE;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenEncoding {
    O200kBase,
    Cl100kBase,
}

impl TokenEncoding {
    fn bpe(self) -> &'static CoreBPE {
        match self {
            Self::O200kBase => tiktoken_rs::o200k_base_singleton(),
            Self::Cl100kBase => tiktoken_rs::cl100k_base_singleton(),
        }
    }
}

thread_local! {
    // One immutable candidate per worker thread, not a document-size limit.
    static LAST_TEXT_COUNT: std::cell::RefCell<Option<(TokenEncoding,String,usize)>> = const { std::cell::RefCell::new(None) };
}
thread_local! {
    static CACHE_COUNTS: std::cell::Cell<(u64,u64)> = const { std::cell::Cell::new((0,0)) };
}
pub(crate) fn cache_counts() -> (u64, u64) {
    CACHE_COUNTS.with(std::cell::Cell::get)
}
fn count_canonical_text(encoding: TokenEncoding, serialized: &str) -> usize {
    if let Some(count) = LAST_TEXT_COUNT.with(|cache| {
        cache.borrow().as_ref().and_then(|(prior, text, count)| {
            (*prior == encoding && text == serialized).then_some(*count)
        })
    }) {
        CACHE_COUNTS.with(|counts| {
            let (hit, miss) = counts.get();
            counts.set((hit + 1, miss));
        });
        return count;
    }
    CACHE_COUNTS.with(|counts| {
        let (hit, miss) = counts.get();
        counts.set((hit, miss + 1));
    });
    let count = encoding.bpe().count_ordinary(serialized);
    LAST_TEXT_COUNT.with(|cache| *cache.borrow_mut() = Some((encoding, serialized.into(), count)));
    count
}

/// A deployment-specific conservative calibration against measured provider
/// input usage. Record the provider/model, sample set/date and observed ratio in
/// provenance. 10_000 basis points means 1x; a calibration may never discount
/// tokens. Synthetic model doubles must identify themselves in provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenizerCalibration {
    pub multiplier_bps: u32,
    pub provenance: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenizerProfile {
    pub model_id: String,
    pub encoding: TokenEncoding,
    /// None is valid only for the explicitly supported model/encoding pairs.
    /// There is deliberately no Default or automatic unknown-model fallback.
    pub calibration: Option<TokenizerCalibration>,
}

fn invalid(message: impl Into<String>) -> AgentError {
    AgentError::new("AGENT_OUTPUT_INVALID", message)
}

fn overflow() -> AgentError {
    invalid("context token estimate overflow")
}

/// Explicit allowlist, not the library's permissive prefix heuristic (which
/// would also accept invented names such as gpt-5-unknown-vendor). Encoding
/// mappings follow OpenAI tiktoken/model.py and tiktoken-rs 0.12.1. New IDs must
/// be verified here or configured with explicit calibration.
fn supported_encoding(model_id: &str) -> Option<TokenEncoding> {
    match model_id {
        "gpt-5"
        | "gpt-5-mini"
        | "gpt-5-nano"
        | "gpt-5-2025-08-07"
        | "gpt-5-mini-2025-08-07"
        | "gpt-5-nano-2025-08-07"
        | "gpt-4.1"
        | "gpt-4.1-mini"
        | "gpt-4.1-nano"
        | "gpt-4.1-2025-04-14"
        | "gpt-4.1-mini-2025-04-14"
        | "gpt-4.1-nano-2025-04-14"
        | "gpt-4o"
        | "gpt-4o-2024-05-13"
        | "gpt-4o-2024-08-06"
        | "gpt-4o-2024-11-20"
        | "gpt-4o-mini"
        | "gpt-4o-mini-2024-07-18"
        | "o1"
        | "o1-2024-12-17"
        | "o3"
        | "o3-2025-04-16"
        | "o3-mini"
        | "o3-mini-2025-01-31"
        | "o4-mini"
        | "o4-mini-2025-04-16" => Some(TokenEncoding::O200kBase),
        "gpt-4"
        | "gpt-4-0314"
        | "gpt-4-0613"
        | "gpt-4-32k"
        | "gpt-4-32k-0314"
        | "gpt-4-32k-0613"
        | "gpt-4-turbo"
        | "gpt-4-turbo-2024-04-09"
        | "gpt-4-0125-preview"
        | "gpt-4-1106-preview"
        | "gpt-3.5-turbo"
        | "gpt-3.5-turbo-0301"
        | "gpt-3.5-turbo-0613"
        | "gpt-3.5-turbo-0125"
        | "gpt-3.5-turbo-1106" => Some(TokenEncoding::Cl100kBase),
        _ => None,
    }
}

impl TokenizerProfile {
    pub fn published_for_model(model_id: &str) -> Result<Self, AgentError> {
        let encoding = supported_encoding(model_id).ok_or_else(|| invalid(
            "custom model requires a measured KB_AUTHORING_TOKENIZER_PROFILE; no tokenizer is inferred"))?;
        Ok(Self {
            model_id: model_id.into(),
            encoding,
            calibration: None,
        })
    }

    pub fn validate_for_model(&self, model_id: &str) -> Result<(), AgentError> {
        if self.model_id.is_empty() || self.model_id.trim() != self.model_id {
            return Err(invalid("tokenizer model_id must be nonempty and trimmed"));
        }
        if self.model_id != model_id {
            return Err(invalid(
                "tokenizer profile does not match configured request model",
            ));
        }
        let supported = supported_encoding(model_id);
        if supported.is_some_and(|encoding| encoding != self.encoding) {
            return Err(invalid("tokenizer encoding does not match supported model"));
        }
        match &self.calibration {
            Some(calibration)
                if calibration.multiplier_bps < 10_000
                    || calibration.provenance.trim().is_empty() =>
            {
                Err(invalid(
                    "tokenizer calibration requires multiplier_bps >= 10000 and provenance",
                ))
            }
            None if supported.is_none() => Err(invalid(
                "unsupported model requires an explicit tokenizer encoding and calibration provenance",
            )),
            _ => Ok(()),
        }
    }

    /// Count ordinary text: strings resembling special tokens remain user text.
    /// With calibration configured this returns a rounded-up budget estimate.
    pub fn count_text_tokens(&self, text: &str) -> Result<usize, AgentError> {
        self.validate_for_model(&self.model_id)?;
        self.calibrate(self.encoding.bpe().count_ordinary(text))
    }

    fn calibrate(&self, tokens: usize) -> Result<usize, AgentError> {
        let multiplier = self
            .calibration
            .as_ref()
            .map_or(10_000, |c| c.multiplier_bps);
        let calibrated = (tokens as u128 * u128::from(multiplier)).div_ceil(10_000);
        usize::try_from(calibrated).map_err(|_| overflow())
    }

    /// A transport-allocation bound, never a second segmentation/admission
    /// policy. Both pinned vocabularies have a maximum ordinary piece of 128
    /// bytes, checked exhaustively against the embedded library data in tests.
    pub fn max_piece_bytes(&self) -> usize {
        128
    }
}

/// Counts are an application admission estimate, not exact provider usage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct RequestTokenEstimate {
    pub encoding: TokenEncoding,
    pub calibrated: bool,
    pub provenance: String,
    pub text_tokens: usize,
    pub calibrated_text_tokens: usize,
    pub framing_tokens: usize,
    pub image_tokens: usize,
    pub safety_tokens: usize,
    pub total_input_tokens: usize,
    pub output_reserved_tokens: usize,
    pub total_context_tokens: usize,
}

// Additional allowance on top of the tokenized canonical envelope. These are
// conservative framing policy constants, not claims about a provider template.
const REQUEST_FRAMING_TOKENS: usize = 16;
const MESSAGE_FRAMING_TOKENS: usize = 8;
const TOOL_FRAMING_TOKENS: usize = 16;

pub(crate) fn estimate_request_tokens(
    body: &Value,
    profile: &TokenizerProfile,
    image_token_reserve: usize,
    token_safety_margin: usize,
) -> Result<RequestTokenEstimate, AgentError> {
    estimate_request_tokens_with_reserve(body, profile, image_token_reserve, token_safety_margin, 0)
}

pub(crate) fn estimate_request_tokens_with_reserve(
    body: &Value,
    profile: &TokenizerProfile,
    image_token_reserve: usize,
    token_safety_margin: usize,
    internal_output_reserve: usize,
) -> Result<RequestTokenEstimate, AgentError> {
    let model = body["model"]
        .as_str()
        .ok_or_else(|| invalid("request model missing for tokenizer profile"))?;
    profile.validate_for_model(model)?;
    let mut text = body.clone();
    let mut image_tokens = 0usize;
    let messages = text["messages"]
        .as_array_mut()
        .ok_or_else(|| invalid("request messages missing for token accounting"))?;
    let message_count = messages.len();
    for message in messages {
        if let Some(parts) = message["content"].as_array_mut() {
            for part in parts {
                if part["type"] == "image_url" {
                    if image_token_reserve == 0 {
                        return Err(invalid(
                            "image token reserve must be positive for image requests",
                        ));
                    }
                    if !part["image_url"]["url"].is_string() {
                        return Err(invalid("image URL missing for token accounting"));
                    }
                    part["image_url"]["url"] = Value::String(String::new());
                    image_tokens = image_tokens
                        .checked_add(image_token_reserve)
                        .ok_or_else(overflow)?;
                }
            }
        }
    }
    let serialized =
        serde_json_canonicalizer::to_string(&text).map_err(|e| invalid(e.to_string()))?;
    let text_tokens = count_canonical_text(profile.encoding, &serialized);
    let calibrated_text_tokens = profile.calibrate(text_tokens)?;
    let tool_count = body["tools"].as_array().map_or(0, Vec::len);
    let framing_tokens = message_count
        .checked_mul(MESSAGE_FRAMING_TOKENS)
        .and_then(|messages| {
            tool_count
                .checked_mul(TOOL_FRAMING_TOKENS)
                .and_then(|tools| messages.checked_add(tools))
        })
        .and_then(|framing| framing.checked_add(REQUEST_FRAMING_TOKENS))
        .ok_or_else(overflow)?;
    let total_input_tokens = calibrated_text_tokens
        .checked_add(framing_tokens)
        .and_then(|tokens| tokens.checked_add(image_tokens))
        .and_then(|tokens| tokens.checked_add(token_safety_margin))
        .ok_or_else(overflow)?;
    let output_reserved_tokens = ["max_tokens", "max_completion_tokens"]
        .into_iter()
        .filter_map(|key| body.get(key))
        .map(|value| {
            value
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(|| invalid("invalid output token reserve"))
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .max()
        .unwrap_or(0)
        .max(internal_output_reserve);
    let total_context_tokens = total_input_tokens
        .checked_add(output_reserved_tokens)
        .ok_or_else(overflow)?;
    Ok(RequestTokenEstimate {
        encoding: profile.encoding,
        calibrated: profile.calibration.is_some(),
        provenance: profile.calibration.as_ref().map(|c| c.provenance.clone()).unwrap_or_else(|| "tiktoken-rs 0.12.1 embedded encoding; verified OpenAI model mapping; canonical JSON plus reserved framing, not provider usage".into()),
        text_tokens,
        calibrated_text_tokens,
        framing_tokens,
        image_tokens,
        safety_tokens: token_safety_margin,
        total_input_tokens,
        output_reserved_tokens,
        total_context_tokens,
    })
}

/// Input-only allowance; callers reserving provider output separately must use
/// this field rather than adding output again to total_context_tokens.
pub(crate) fn estimate_input_tokens(
    body: &Value,
    profile: &TokenizerProfile,
    image_token_reserve: usize,
    token_safety_margin: usize,
) -> Result<usize, AgentError> {
    Ok(
        estimate_request_tokens(body, profile, image_token_reserve, token_safety_margin)?
            .total_input_tokens,
    )
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[test]
fn exact_candidate_cache_never_reuses_changed_text_or_encoding() {
    for encoding in [TokenEncoding::O200kBase, TokenEncoding::Cl100kBase] {
        for text in [
            "{\"text\":\"Original 条件\"}",
            "{\"text\":\"Changed 条件😀\"}",
        ] {
            let expected = encoding.bpe().count_ordinary(text);
            assert_eq!(count_canonical_text(encoding, text), expected);
            assert_eq!(count_canonical_text(encoding, text), expected);
        }
    }
}
