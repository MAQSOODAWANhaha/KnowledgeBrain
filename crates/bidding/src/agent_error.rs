//! Shared authoring error classification.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryDisposition {
    Obsolete,
    Deterministic,
    Transient,
}

#[derive(Debug, Clone)]
pub struct AgentError {
    pub code: String,
    pub message: String,
    pub disposition: RetryDisposition,
}

impl AgentError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        let code = code
            .split([':', ';'])
            .next()
            .filter(|value| !value.is_empty())
            .unwrap_or(code);
        let disposition = match code {
            "REQUEST_OBSOLETE" | "REQUEST_ATTEMPT_SUPERSEDED" => RetryDisposition::Obsolete,
            // A model boundary already exhausted its single three-call budget.
            // Only internal infrastructure failures may be retried by the job
            // runner; deterministic contract/provider exhaustion is terminal.
            "INTERNAL" => RetryDisposition::Transient,
            _ => RetryDisposition::Deterministic,
        };
        Self {
            code: code.to_owned(),
            message: message.into(),
            disposition,
        }
    }
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AgentError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persistence_code_is_stable_and_detail_is_retained() {
        let error = AgentError::new(
            "OUTLINE_OBSERVATION_PARTITION_INVALID: missing ref",
            "full detail",
        );
        assert_eq!(error.code, "OUTLINE_OBSERVATION_PARTITION_INVALID");
        assert_eq!(error.message, "full detail");
        assert_eq!(error.disposition, RetryDisposition::Deterministic);
    }

    #[test]
    fn only_internal_failures_reenter_the_job_queue() {
        assert_eq!(
            AgentError::new("AGENT_PROVIDER_UNAVAILABLE", "exhausted").disposition,
            RetryDisposition::Deterministic
        );
        assert_eq!(
            AgentError::new("INTERNAL", "database unavailable").disposition,
            RetryDisposition::Transient
        );
    }
}
