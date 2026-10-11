use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Conservative request defaults, not claims about a provider's native maximum.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestDefaults {
    pub output_token_reserve: u32,
    pub timeout_ms: u64,
}

pub fn request_defaults() -> &'static RequestDefaults {
    static DEFAULTS: std::sync::OnceLock<RequestDefaults> = std::sync::OnceLock::new();
    DEFAULTS.get_or_init(|| {
        serde_json::from_str(include_str!("../config/request-defaults.json"))
            .expect("checked-in request defaults")
    })
}

/// Resource bound, not a provider model limit. Keeps wire allocations bounded.
pub const MAX_CONTEXT_TOKENS: usize = 4_194_304;

pub(crate) fn context_tokens_with(read: &impl Fn(&str) -> Option<String>) -> Result<usize, String> {
    const KEY: &str = "KNOWLEDGEBRAIN_CHAT_MAX_CONTEXT_TOKENS";
    read(KEY)
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|value| (1..=MAX_CONTEXT_TOKENS).contains(value))
        .ok_or_else(|| format!("{KEY} must be an integer between 1 and {MAX_CONTEXT_TOKENS}"))
}

pub(crate) fn context_tokens_from_environment() -> Result<usize, String> {
    context_tokens_with(&nonempty_env)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoringRuntimeContractV1 {
    pub schema_version: u32,
    pub base_url: String,
    pub endpoint: String,
    pub protocol: String,
    pub model_id: String,
    pub credential_ref: String,
    pub stream: bool,
    pub output_token_reserve: u32,
    pub timeout_ms: u64,
    pub response_mode: String,
    pub transport_retries: u32,
    pub temperature: Option<String>,
    pub reasoning_effort: Option<String>,
}

impl AuthoringRuntimeContractV1 {
    pub fn resolve_tools_from_environment() -> Result<Self, String> {
        let mut runtime = Self::resolve_from_environment()?;
        runtime.response_mode = "tool_calls".into();
        Ok(runtime)
    }
    pub fn resolve_from_environment() -> Result<Self, String> {
        Self::resolve_with(nonempty_env)
    }

    fn resolve_with(read: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let read = |key: &str| {
            read(key).and_then(|value| {
                if value.trim().is_empty() {
                    return None;
                }
                Some(if key.ends_with("API_KEY") {
                    value
                } else {
                    value.trim().to_owned()
                })
            })
        };
        let base =
            alias(&read, "KNOWLEDGEBRAIN_CHAT_BASE_URL", "LLM_BASE_URL")?.ok_or_else(|| {
                "AGENT_PROVIDER_UNAVAILABLE: chat base URL is not configured".to_string()
            })?;
        let model_id =
            alias(&read, "KNOWLEDGEBRAIN_CHAT_MODEL", "LLM_MODEL")?.ok_or_else(|| {
                "AGENT_PROVIDER_UNAVAILABLE: concrete chat model is not configured".to_string()
            })?;
        if model_id == "stub-chat" {
            return Err("AGENT_PROVIDER_UNAVAILABLE: stub-chat is not a runtime model".into());
        }
        alias(&read, "KNOWLEDGEBRAIN_CHAT_API_KEY", "LLM_API_KEY")?;
        let credential_ref = if read("KNOWLEDGEBRAIN_CHAT_API_KEY").is_some() {
            "env:KNOWLEDGEBRAIN_CHAT_API_KEY"
        } else if read("LLM_API_KEY").is_some() {
            "env:LLM_API_KEY"
        } else {
            return Err("AGENT_PROVIDER_UNAVAILABLE: chat credential is not configured".into());
        };
        let mut parsed = reqwest::Url::parse(&base).map_err(|error| {
            format!("AGENT_PROVIDER_UNAVAILABLE: invalid chat base URL: {error}")
        })?;
        if !matches!(parsed.scheme(), "http" | "https")
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(
                "AGENT_PROVIDER_UNAVAILABLE: chat base URL is not an absolute HTTP endpoint".into(),
            );
        }
        let normalized_path = parsed.path().trim_end_matches('/').to_string();
        parsed.set_path(if normalized_path.is_empty() {
            "/"
        } else {
            &normalized_path
        });
        let base_url = parsed.as_str().trim_end_matches('/').to_string();
        let endpoint = if base_url.ends_with("/v1") {
            format!("{base_url}/chat/completions")
        } else {
            format!("{base_url}/v1/chat/completions")
        };
        let context = context_tokens_with(&read)?;
        let reasoning_effort = match (
            model_id.as_str(),
            read("KNOWLEDGEBRAIN_CHAT_REASONING_EFFORT"),
        ) {
            ("gpt-6-luna", None) => Some("none".to_owned()),
            ("gpt-6-luna", Some(value)) if value != "none" => {
                return Err(
                    "gpt-6-luna Chat Completions tool calls require reasoning_effort=none".into(),
                );
            }
            (_, value) => value,
        };
        let runtime = Self {
            schema_version: 1,
            base_url,
            endpoint,
            protocol: "openai_chat_completions_sse".into(),
            model_id,
            credential_ref: credential_ref.into(),
            stream: true,
            output_token_reserve: positive_setting(
                &read,
                "KB_AUTHORING_OUTPUT_TOKEN_RESERVE",
                request_defaults()
                    .output_token_reserve
                    .min((context / 4).max(1) as u32),
            )?,
            timeout_ms: positive_setting(
                &read,
                "KB_AUTHORING_TIMEOUT_MS",
                request_defaults().timeout_ms,
            )?,
            response_mode: "strict_json_schema".into(),
            transport_retries: 0,
            temperature: None,
            reasoning_effort,
        };
        if (runtime.output_token_reserve as usize)
            .checked_add((context / 16).clamp(1, 4096))
            .is_none_or(|reserved| reserved >= context)
        {
            return Err("KB_AUTHORING_OUTPUT_TOKEN_RESERVE plus safety reserve must fit KNOWLEDGEBRAIN_CHAT_MAX_CONTEXT_TOKENS".into());
        }
        runtime.validate()?;
        Ok(runtime)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1
            || self.protocol != "openai_chat_completions_sse"
            || self.model_id.is_empty()
            || self.model_id == "stub-chat"
            || !matches!(
                self.credential_ref.as_str(),
                "env:KNOWLEDGEBRAIN_CHAT_API_KEY" | "env:LLM_API_KEY"
            )
            || !self.stream
            || self.output_token_reserve == 0
            || self.timeout_ms == 0
            || !matches!(
                self.response_mode.as_str(),
                "strict_json_schema" | "tool_calls"
            )
            || self.transport_retries != 0
            || self.temperature.is_some()
            || self
                .reasoning_effort
                .as_ref()
                .is_some_and(|v| v.trim().is_empty())
        {
            return Err("FROZEN_INPUT_DIGEST_MISMATCH: invalid AuthoringRuntimeContractV1".into());
        }
        let resolved = reqwest::Url::parse(&self.endpoint)
            .map_err(|_| "FROZEN_INPUT_DIGEST_MISMATCH: invalid frozen endpoint".to_string())?;
        if resolved.as_str() != self.endpoint
            || !self.endpoint.starts_with(&format!("{}/", self.base_url))
            || !self.endpoint.ends_with("/chat/completions")
        {
            return Err("FROZEN_INPUT_DIGEST_MISMATCH: endpoint/base mismatch".into());
        }
        Ok(())
    }

    pub fn canonical_bytes_and_sha256(&self) -> Result<(Vec<u8>, String), String> {
        self.validate()?;
        let bytes = serde_json_canonicalizer::to_vec(self)
            .map_err(|error| format!("runtime contract JCS: {error}"))?;
        let sha = hex::encode(Sha256::digest(&bytes));
        Ok((bytes, sha))
    }

    pub fn resolve_api_key(&self) -> Result<String, String> {
        let name = self
            .credential_ref
            .strip_prefix("env:")
            .ok_or_else(|| "AGENT_PROVIDER_UNAVAILABLE: invalid credential ref".to_string())?;
        nonempty_env(name).ok_or_else(|| {
            format!("AGENT_PROVIDER_UNAVAILABLE: frozen credential ref {name} is unavailable")
        })
    }
}

fn nonempty_env(name: &str) -> Option<String> {
    std::env::var(name).ok().and_then(|value| {
        if value.trim().is_empty() {
            return None;
        }
        Some(if name.ends_with("API_KEY") {
            value
        } else {
            value.trim().to_owned()
        })
    })
}

fn alias(
    read: &impl Fn(&str) -> Option<String>,
    primary: &str,
    secondary: &str,
) -> Result<Option<String>, String> {
    let (a, b) = (read(primary), read(secondary));
    if a.as_ref().zip(b.as_ref()).is_some_and(|(a, b)| a != b) {
        // Values may contain credentials; errors must only identify the keys.
        return Err(format!(
            "AGENT_PROVIDER_UNAVAILABLE: conflicting {primary} and {secondary}"
        ));
    }
    Ok(a.or(b))
}

fn positive_setting<T>(
    read: &impl Fn(&str) -> Option<String>,
    name: &str,
    fallback: T,
) -> Result<T, String>
where
    T: std::str::FromStr + Default + PartialOrd,
{
    let Some(raw) = read(name) else {
        return Ok(fallback);
    };
    if raw.trim().is_empty() {
        return Ok(fallback);
    }
    raw.trim()
        .parse::<T>()
        .ok()
        .filter(|value| *value > T::default())
        .ok_or_else(|| format!("AGENT_PROVIDER_UNAVAILABLE: {name} must be a positive integer"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> AuthoringRuntimeContractV1 {
        AuthoringRuntimeContractV1 {
            schema_version: 1,
            base_url: "https://llm.example/v1".into(),
            endpoint: "https://llm.example/v1/chat/completions".into(),
            protocol: "openai_chat_completions_sse".into(),
            model_id: "frozen-model".into(),
            credential_ref: "env:KNOWLEDGEBRAIN_CHAT_API_KEY".into(),
            stream: true,
            output_token_reserve: 8192,
            timeout_ms: 180_000,
            response_mode: "strict_json_schema".into(),
            transport_retries: 0,
            temperature: None,
            reasoning_effort: None,
        }
    }

    #[test]
    fn runtime_identity_freezes_configured_tuning_and_rejects_transport_retries() {
        let (bytes, sha) = fixture().canonical_bytes_and_sha256().unwrap();
        assert_eq!(sha, hex::encode(Sha256::digest(&bytes)));
        assert!(
            String::from_utf8(bytes)
                .unwrap()
                .contains("\"temperature\":null")
        );
        let mut drift = fixture();
        drift.reasoning_effort = Some("high".into());
        drift.output_token_reserve = 4096;
        drift.timeout_ms = 90000;
        assert!(drift.validate().is_ok());
        assert_ne!(sha, drift.canonical_bytes_and_sha256().unwrap().1);
        let mut retries = fixture();
        retries.transport_retries = 1;
        assert!(retries.validate().is_err());
    }

    fn environment() -> std::collections::BTreeMap<String, String> {
        [
            ("LLM_BASE_URL", "https://llm.example/v1"),
            ("LLM_MODEL", "configured-model"),
            ("KNOWLEDGEBRAIN_CHAT_MAX_CONTEXT_TOKENS", "131072"),
            ("LLM_API_KEY", "test-key"),
            ("KB_AUTHORING_OUTPUT_TOKEN_RESERVE", "4096"),
            ("KB_AUTHORING_TIMEOUT_MS", "90000"),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect()
    }

    #[test]
    fn luna_tool_calls_use_supported_reasoning_effort() {
        let mut env = environment();
        env.insert("LLM_MODEL".into(), "gpt-6-luna".into());
        let runtime =
            AuthoringRuntimeContractV1::resolve_with(|key| env.get(key).cloned()).unwrap();
        assert_eq!(runtime.reasoning_effort.as_deref(), Some("none"));
        env.insert("KNOWLEDGEBRAIN_CHAT_REASONING_EFFORT".into(), "high".into());
        assert!(AuthoringRuntimeContractV1::resolve_with(|key| env.get(key).cloned()).is_err());
    }

    #[test]
    fn optional_environment_parameters_override_safe_defaults() {
        let mut env = environment();
        let runtime = AuthoringRuntimeContractV1::resolve_with(|k| env.get(k).cloned()).unwrap();
        assert_eq!(runtime.model_id, "configured-model");
        assert_eq!(runtime.output_token_reserve, 4096);
        assert_eq!(runtime.timeout_ms, 90000);
        assert_eq!(runtime.reasoning_effort, None);
        env.insert("KNOWLEDGEBRAIN_CHAT_REASONING_EFFORT".into(), "high".into());
        let runtime = AuthoringRuntimeContractV1::resolve_with(|k| env.get(k).cloned()).unwrap();
        assert_eq!(runtime.reasoning_effort.as_deref(), Some("high"));
        for key in [
            "KB_AUTHORING_OUTPUT_TOKEN_RESERVE",
            "KB_AUTHORING_TIMEOUT_MS",
        ] {
            let mut defaults = env.clone();
            defaults.remove("KB_AUTHORING_OUTPUT_TOKEN_RESERVE");
            defaults.remove("KB_AUTHORING_TIMEOUT_MS");
            let runtime =
                AuthoringRuntimeContractV1::resolve_with(|k| defaults.get(k).cloned()).unwrap();
            assert_eq!(
                runtime.output_token_reserve,
                request_defaults().output_token_reserve
            );
            assert_eq!(runtime.timeout_ms, request_defaults().timeout_ms);
            for value in [
                Some("0"),
                Some("-1"),
                Some("1.5"),
                Some("18446744073709551616"),
            ] {
                let mut invalid = env.clone();
                invalid.remove(key);
                if let Some(value) = value {
                    invalid.insert(key.into(), value.into());
                }
                let err = AuthoringRuntimeContractV1::resolve_with(|k| invalid.get(k).cloned())
                    .unwrap_err();
                assert!(err.contains(key));
            }
        }
    }

    #[test]
    fn context_is_required_bounded_and_derives_reserves() {
        let mut env = environment();
        env.remove("KB_AUTHORING_OUTPUT_TOKEN_RESERVE");
        for context in [64, 8192, 131072, 262144, MAX_CONTEXT_TOKENS] {
            env.insert(
                "KNOWLEDGEBRAIN_CHAT_MAX_CONTEXT_TOKENS".into(),
                context.to_string(),
            );
            let runtime =
                AuthoringRuntimeContractV1::resolve_with(|k| env.get(k).cloned()).unwrap();
            assert_eq!(
                runtime.output_token_reserve as usize,
                (context / 4).clamp(1, 16384)
            );
        }
        for raw in [
            "",
            "  ",
            "0",
            "-1",
            "1.5",
            "18446744073709551616",
            "4194305",
            "1",
            "2",
        ] {
            env.insert("KNOWLEDGEBRAIN_CHAT_MAX_CONTEXT_TOKENS".into(), raw.into());
            assert!(AuthoringRuntimeContractV1::resolve_with(|k| env.get(k).cloned()).is_err());
        }
        env.remove("KNOWLEDGEBRAIN_CHAT_MAX_CONTEXT_TOKENS");
        assert!(
            AuthoringRuntimeContractV1::resolve_with(|k| env.get(k).cloned())
                .unwrap_err()
                .contains("KNOWLEDGEBRAIN_CHAT_MAX_CONTEXT_TOKENS")
        );
        env.insert(
            "KNOWLEDGEBRAIN_CHAT_MAX_CONTEXT_TOKENS".into(),
            "8192".into(),
        );
        for reserve in [7680, 8192, u32::MAX] {
            env.insert(
                "KB_AUTHORING_OUTPUT_TOKEN_RESERVE".into(),
                reserve.to_string(),
            );
            assert!(AuthoringRuntimeContractV1::resolve_with(|k| env.get(k).cloned()).is_err());
        }
    }

    #[test]
    fn strings_trim_but_secret_bytes_are_preserved() {
        let mut env = environment();
        env.insert("LLM_MODEL".into(), "  configured-model  ".into());
        env.insert("LLM_API_KEY".into(), "  secret-key  ".into());
        env.insert(
            "KNOWLEDGEBRAIN_CHAT_API_KEY".into(),
            "  secret-key  ".into(),
        );
        let runtime = AuthoringRuntimeContractV1::resolve_with(|k| env.get(k).cloned()).unwrap();
        assert_eq!(runtime.model_id, "configured-model");
        env.insert("KNOWLEDGEBRAIN_CHAT_API_KEY".into(), "secret-key".into());
        assert!(AuthoringRuntimeContractV1::resolve_with(|k| env.get(k).cloned()).is_err());
        env.insert("LLM_API_KEY".into(), "   ".into());
        env.remove("KNOWLEDGEBRAIN_CHAT_API_KEY");
        assert!(AuthoringRuntimeContractV1::resolve_with(|k| env.get(k).cloned()).is_err());
    }

    #[test]
    fn conflicting_aliases_fail_without_exposing_values() {
        for (primary, secondary) in [
            ("KNOWLEDGEBRAIN_CHAT_BASE_URL", "LLM_BASE_URL"),
            ("KNOWLEDGEBRAIN_CHAT_MODEL", "LLM_MODEL"),
            ("KNOWLEDGEBRAIN_CHAT_API_KEY", "LLM_API_KEY"),
        ] {
            let mut env = environment();
            env.insert(primary.into(), env[secondary].clone());
            assert!(AuthoringRuntimeContractV1::resolve_with(|k| env.get(k).cloned()).is_ok());
            env.insert(primary.into(), "conflicting-secret-value".into());
            let err =
                AuthoringRuntimeContractV1::resolve_with(|k| env.get(k).cloned()).unwrap_err();
            assert!(err.contains(primary) && err.contains(secondary));
            assert!(!err.contains("conflicting-secret-value") && !err.contains(&env[secondary]));
        }
    }
}
