//! Durable usage accounting; no cumulative operation quotas.
use crate::agent_error::AgentError;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelAccounting {
    pub physical_calls: u64,
    pub reserved_input_tokens: u64,
    pub reserved_output_tokens: u64,
    pub started_unix_seconds: Option<u64>,
}
impl ModelAccounting {
    pub fn record(&mut self, input: u64, output: u64, now: u64) -> Result<(), AgentError> {
        let failure = || AgentError::new("AGENT_OUTPUT_INVALID", "model accounting overflow");
        let physical = self.physical_calls.checked_add(1).ok_or_else(failure)?;
        let input = self
            .reserved_input_tokens
            .checked_add(input)
            .ok_or_else(failure)?;
        let output = self
            .reserved_output_tokens
            .checked_add(output)
            .ok_or_else(failure)?;
        self.started_unix_seconds.get_or_insert(now);
        self.physical_calls = physical;
        self.reserved_input_tokens = input;
        self.reserved_output_tokens = output;
        Ok(())
    }
}
pub fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn usage_counts_have_no_operation_quota_and_overflow_is_atomic() {
        let mut accounting = ModelAccounting::default();
        accounting.record(120_000, 8192, 1).unwrap();
        accounting.record(120_000, 8192, 1_000_000).unwrap();
        assert_eq!(accounting.physical_calls, 2);
        accounting.reserved_input_tokens = u64::MAX;
        assert!(accounting.record(1, 0, 1_000_001).is_err());
        assert_eq!(accounting.physical_calls, 2);
    }
}
