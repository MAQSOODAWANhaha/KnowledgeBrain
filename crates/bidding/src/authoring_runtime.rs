use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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
    pub max_tokens: u32,
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
        let runtime = Self {
            schema_version: 1,
            base_url,
            endpoint,
            protocol: "openai_chat_completions_sse".into(),
            model_id,
            credential_ref: credential_ref.into(),
            stream: true,
            max_tokens: positive_setting(&read, "KB_AUTHORING_MAX_OUTPUT_TOKENS")?,
            timeout_ms: positive_setting(&read, "KB_AUTHORING_TIMEOUT_MS")?,
            response_mode: "strict_json_schema".into(),
            transport_retries: 0,
            temperature: None,
            reasoning_effort: read("KNOWLEDGEBRAIN_CHAT_REASONING_EFFORT"),
        };
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
            || self.max_tokens == 0
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
    std::env::var(name).ok().filter(|value| !value.is_empty())
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

fn positive_setting<T>(read: &impl Fn(&str) -> Option<String>, name: &str) -> Result<T, String>
where
    T: std::str::FromStr + Default + PartialOrd,
{
    read(name)
        .and_then(|value| value.parse::<T>().ok())
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
            max_tokens: 8192,
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
        drift.max_tokens = 4096;
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
            ("LLM_API_KEY", "test-key"),
            ("KB_AUTHORING_MAX_OUTPUT_TOKENS", "4096"),
            ("KB_AUTHORING_TIMEOUT_MS", "90000"),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect()
    }

    #[test]
    fn environment_parameters_are_required_and_reasoning_is_optional() {
        let mut env = environment();
        let runtime = AuthoringRuntimeContractV1::resolve_with(|k| env.get(k).cloned()).unwrap();
        assert_eq!(runtime.model_id, "configured-model");
        assert_eq!(runtime.max_tokens, 4096);
        assert_eq!(runtime.timeout_ms, 90000);
        assert_eq!(runtime.reasoning_effort, None);
        env.insert("KNOWLEDGEBRAIN_CHAT_REASONING_EFFORT".into(), "high".into());
        let runtime = AuthoringRuntimeContractV1::resolve_with(|k| env.get(k).cloned()).unwrap();
        assert_eq!(runtime.reasoning_effort.as_deref(), Some("high"));
        for key in ["KB_AUTHORING_MAX_OUTPUT_TOKENS", "KB_AUTHORING_TIMEOUT_MS"] {
            for value in [
                None,
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
