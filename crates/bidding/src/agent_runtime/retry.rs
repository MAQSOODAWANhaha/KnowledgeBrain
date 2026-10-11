//! Bounded retry bookkeeping for an exact prepared request. Reservations and
//! unknown charges survive reload; this is not a run-wide spending quota.
use crate::agent_error::AgentError;
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const MAX_ATTEMPTS: usize = 3;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicalAttempt {
    pub request: String,
    pub number: usize,
    pub outcome: String,
    pub usage_unknown: bool,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetryJournal {
    pub attempts: Vec<PhysicalAttempt>,
    pub not_before_ms: u64,
}
pub fn usage_known(usage: Option<&knowledge::models::ChatUsage>) -> bool {
    usage.is_some_and(|usage| usage.prompt_tokens.is_some() && usage.completion_tokens.is_some())
}
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
pub fn retryable(code: &str) -> bool {
    matches!(
        code,
        "AGENT_PROVIDER_TRANSIENT"
            | "AGENT_TRANSPORT_INTERRUPTED"
            | "AGENT_TURN_TIMEOUT"
            | "AGENT_SEND_OUTCOME_UNKNOWN"
            | "AGENT_RESPONSE_INCOMPLETE"
            | "not_sent"
    )
}
impl RetryJournal {
    pub fn count(&self, request: &str) -> usize {
        self.attempts
            .iter()
            .filter(|attempt| attempt.request == request && attempt.outcome != "not_sent")
            .count()
    }
    pub fn reserve(&mut self, request: &str) {
        self.attempts.push(PhysicalAttempt {
            request: request.into(),
            number: self.count(request) + 1,
            outcome: "AGENT_SEND_OUTCOME_UNKNOWN".into(),
            usage_unknown: true,
        });
        self.not_before_ms = 0;
    }
    pub fn complete(&mut self, request: &str, outcome: &str, usage_known: bool) {
        if let Some(attempt) = self
            .attempts
            .iter_mut()
            .rev()
            .find(|attempt| attempt.request == request)
        {
            attempt.outcome = outcome.into();
            attempt.usage_unknown = !usage_known;
        }
    }
    pub fn last_outcome(&self, request: &str) -> Option<&str> {
        self.attempts
            .iter()
            .rev()
            .find(|attempt| attempt.request == request)
            .map(|attempt| attempt.outcome.as_str())
    }
    /// Older saved Sending/physical_attempts boundaries were already reserved.
    pub fn recover(&mut self, request: &str, charged_attempts: usize) {
        while self.count(request) < charged_attempts.min(MAX_ATTEMPTS) {
            self.reserve(request);
        }
    }
    pub fn schedule(&mut self, request: &str, error: &AgentError) {
        let delay = error
            .retry_after_ms
            .unwrap_or(self.count(request).max(1) as u64 * 1000);
        // Round the persisted deadline upward so millisecond truncation never
        // shortens the provider's requested wait.
        self.not_before_ms = now_ms().saturating_add(delay).saturating_add(1);
    }
    pub fn remaining(&self) -> Duration {
        Duration::from_millis(self.not_before_ms.saturating_sub(now_ms()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use knowledge::models::ChatUsage;

    #[test]
    fn partial_usage_remains_unknown_even_with_reported_total() {
        assert!(!usage_known(None));
        assert!(!usage_known(Some(&ChatUsage::default())));
        let mut usage = ChatUsage {
            prompt_tokens: Some(10),
            total_tokens: Some(12),
            ..Default::default()
        };
        assert!(!usage_known(Some(&usage)));
        usage.completion_tokens = Some(2);
        assert!(usage_known(Some(&usage)));
    }
}
