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
            // Only infrastructure failures automatically reenter the queue.
            // Tender stream interruption is handled explicitly by its host:
            // retain pending state and wait for the user's continue action.
            "INTERNAL" => RetryDisposition::Transient,
            _ => RetryDisposition::Deterministic,
        };
        Self {
            code: code.to_owned(),
            message: message.into(),
            disposition,
        }
    }

    /// What the Oxana job should do after this attempt. Decided by error code,
    /// not by collapsing different causes into one boolean.
    pub fn request_queue_effect(&self) -> RequestQueueEffect {
        match self.code.as_str() {
            "REQUEST_OBSOLETE" => RequestQueueEffect::AckObsolete,
            "REQUEST_ATTEMPT_SUPERSEDED" => RequestQueueEffect::RetryUnchanged,
            "AGENT_DEADLINE_EXCEEDED" => RequestQueueEffect::ReleaseThenRetry,
            _ if self.disposition == RetryDisposition::Transient => {
                RequestQueueEffect::YieldThenRetry
            }
            _ => RequestQueueEffect::FailRequest,
        }
    }
}

/// Queue outcome for one AgentRun attempt. Keep causes separate: superseded is
/// already unlocked; deadline still holds the lease and must yield first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestQueueEffect {
    AckObsolete,
    RetryUnchanged,
    ReleaseThenRetry,
    YieldThenRetry,
    FailRequest,
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
    fn only_infrastructure_failures_automatically_reenter_the_job_queue() {
        assert_eq!(
            AgentError::new("AGENT_PROVIDER_UNAVAILABLE", "exhausted").disposition,
            RetryDisposition::Deterministic
        );
        assert_eq!(
            AgentError::new("AGENT_TRANSPORT_INTERRUPTED", "disconnect").request_queue_effect(),
            RequestQueueEffect::FailRequest
        );
        assert_eq!(
            AgentError::new("INTERNAL", "database unavailable").disposition,
            RetryDisposition::Transient
        );
    }

    #[test]
    fn request_queue_effect_keeps_causes_separate() {
        use RequestQueueEffect::*;
        assert_eq!(
            AgentError::new("REQUEST_OBSOLETE", "done").request_queue_effect(),
            AckObsolete
        );
        assert_eq!(
            AgentError::new("REQUEST_ATTEMPT_SUPERSEDED", "lease").request_queue_effect(),
            RetryUnchanged
        );
        assert_eq!(
            AgentError::new("AGENT_DEADLINE_EXCEEDED", "wall").request_queue_effect(),
            ReleaseThenRetry
        );
        assert_eq!(
            AgentError::new("INTERNAL", "db").request_queue_effect(),
            YieldThenRetry
        );
        assert_eq!(
            AgentError::new("AGENT_OUTPUT_INVALID", "schema").request_queue_effect(),
            FailRequest
        );
    }
}
