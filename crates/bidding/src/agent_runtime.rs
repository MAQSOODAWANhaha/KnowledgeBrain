//! Shared durable turn state for extraction and composition. Business state
//! remains in their existing checkpoints; this tracks only the I/O boundaries.
use crate::agent_error::AgentError;
use knowledge::models::ChatTurn;
use serde::{Deserialize, Serialize};

pub(crate) mod chat;
mod driver;
pub mod progress;
mod session;
pub(crate) use driver::{Driver, Status, check_cancel, drive};

/// Versioned persistence contract, independent of provider and business rules.
pub const CHECKPOINT_CONTRACT_VERSION: u32 = 8;
/// Freeze the SDK/adapter separately from the Journal's persistence format.
pub const RUNTIME_ADAPTER_VERSION: &str = "rig-chat-0.42.0/4";

/// Host messages before the SDK evidence transcript: system instruction plus
/// per-request metadata. Extraction and composition share this layout.
pub(crate) const SESSION_PREFIX: usize = 2;
/// Extraction/review append a dynamic progress packet after the transcript.
pub(crate) const ANALYSIS_SESSION_SUFFIX: usize = 1;
/// Composition folds progress into the metadata message; no trailing packet.
pub(crate) const COMPOSITION_SESSION_SUFFIX: usize = 0;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnJournal {
    pub sequence: usize,
    pub pending: Option<PendingTurn>,
    pub session: Option<session::Session>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingTurn {
    pub turn: usize,
    pub role: String,
    /// Exact JCS request bytes represented as UTF-8, not a second serialization.
    pub body: String,
    pub response: Option<ChatTurn>,
}

fn invalid(message: &str) -> AgentError {
    AgentError::new("FROZEN_INPUT_DIGEST_MISMATCH", message)
}

impl TurnJournal {
    pub(crate) fn prepare_session(
        &mut self,
        body: &[u8],
        prefix: usize,
        suffix: usize,
        remaining_turns: usize,
        max_bytes: usize,
    ) -> Result<(), AgentError> {
        if self.pending.is_some() {
            return Err(invalid("cannot replace a pending SDK session"));
        }
        session::Session::prepare(
            &mut self.session,
            body,
            prefix,
            suffix,
            remaining_turns,
            max_bytes,
        )
    }
    fn advance(&mut self) -> Result<(), AgentError> {
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| invalid("checkpoint sequence exhausted"))?;
        Ok(())
    }

    pub fn prepare(&mut self, turn: usize, role: &str, body: &[u8]) -> Result<(), AgentError> {
        if self.pending.is_some() || !matches!(role, "main" | "reviewer") {
            return Err(invalid("finish the pending turn before preparing another"));
        }
        let body = String::from_utf8(body.to_vec()).map_err(|_| invalid("request is not UTF-8"))?;
        self.advance()?;
        self.pending = Some(PendingTurn {
            turn,
            role: role.into(),
            body,
            response: None,
        });
        Ok(())
    }

    /// Persist a host-only terminal result after the last committed model turn.
    pub(crate) fn finish(&mut self) -> Result<(), AgentError> {
        if self.pending.is_some() || self.sequence == 0 {
            return Err(invalid("finalization requires a committed checkpoint"));
        }
        self.advance()
    }

    pub fn validate(&self, turn: usize, role: &str) -> Result<(), AgentError> {
        if (self.sequence == 0 && (turn != 0 || self.pending.is_some()))
            || self.sequence < turn
            || !matches!(role, "main" | "reviewer")
            || self.response().is_some_and(|r| !valid_tool_turn(r))
            || self
                .pending
                .as_ref()
                .is_some_and(|p| p.turn != turn || p.role != role)
        {
            return Err(invalid("pending turn identity changed"));
        }
        if let Some(pending) = &self.pending {
            let body: serde_json::Value = serde_json::from_str(&pending.body)
                .map_err(|_| invalid("saved request is not JSON"))?;
            if !body.is_object()
                || serde_json_canonicalizer::to_vec(&body)
                    .map_err(|_| invalid("saved request is not canonical"))?
                    != pending.body.as_bytes()
            {
                return Err(invalid("saved request is not canonical"));
            }
            self.session
                .as_ref()
                .ok_or_else(|| invalid("pending SDK session missing"))?
                .validate(&body)?;
        }
        Ok(())
    }

    pub fn body(&self) -> Result<&[u8], AgentError> {
        self.pending
            .as_ref()
            .map(|p| p.body.as_bytes())
            .ok_or_else(|| invalid("prepared request missing"))
    }

    pub fn response(&self) -> Option<&ChatTurn> {
        self.pending.as_ref().and_then(|p| p.response.as_ref())
    }

    pub fn responded(&mut self, response: ChatTurn) -> Result<(), AgentError> {
        if !valid_tool_turn(&response) || self.pending.as_ref().is_none_or(|p| p.response.is_some())
        {
            return Err(invalid("response must follow a prepared request"));
        }
        self.advance()?;
        self.pending
            .as_mut()
            .expect("checked pending turn")
            .response = Some(response);
        Ok(())
    }

    pub fn committed(&mut self) -> Result<(), AgentError> {
        if self.response().is_none() {
            return Err(invalid("tool commit needs a saved response"));
        }
        self.advance()?;
        self.pending = None;
        Ok(())
    }
}

/// A malformed argument is returned as tool feedback; a partial response or
/// duplicate call ID cannot establish delivery or be durably replayed.
pub(crate) fn valid_tool_turn(turn: &ChatTurn) -> bool {
    let mut ids = std::collections::BTreeSet::new();
    turn.finish_reason == "tool_calls"
        && !turn.tool_calls.is_empty()
        && turn.tool_calls.iter().all(|call| {
            !call.id.trim().is_empty() && !call.name.trim().is_empty() && ids.insert(&call.id)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use knowledge::models::ChatToolCall;

    fn prepared(journal: &mut TurnJournal) {
        let body = br#"{"messages":[{"content":"test","role":"system"}]}"#;
        journal.prepare_session(body, 1, 0, 4, 4096).unwrap();
        journal.prepare(0, "main", body).unwrap();
    }

    #[test]
    fn invalid_response_and_out_of_order_transitions_leave_the_boundary_intact() {
        let mut journal = TurnJournal::default();
        assert!(journal.committed().is_err());
        assert!(journal.responded(ChatTurn::default()).is_err());
        assert_eq!(journal.sequence, 0);
        prepared(&mut journal);
        assert!(journal.prepare(0, "reviewer", b"{}").is_err());
        assert!(journal.validate(1, "main").is_err());
        assert!(journal.validate(0, "reviewer").is_err());
        let call = ChatToolCall {
            id: "one".into(),
            name: "read_source".into(),
            arguments: "invalid JSON becomes tool feedback".into(),
        };
        let mut response = ChatTurn {
            content: String::new(),
            tool_calls: vec![call.clone(), call],
            finish_reason: "tool_calls".into(),
            usage: None,
        };
        assert!(journal.responded(response.clone()).is_err());
        assert_eq!(journal.sequence, 1);
        response.tool_calls.pop();
        journal.responded(response).unwrap();
        assert!(journal.responded(ChatTurn::default()).is_err());
        journal.validate(0, "main").unwrap();
        journal.committed().unwrap();
        journal.validate(1, "reviewer").unwrap();
        assert!(journal.pending.is_none());
    }

    #[test]
    fn recovery_rejects_corrupt_saved_request_and_incomplete_response() {
        let mut journal = TurnJournal::default();
        journal.prepare(0, "main", br#"{ "messages":[] }"#).unwrap();
        assert!(
            journal.validate(0, "main").is_err(),
            "saved exact bytes must be JCS"
        );
        journal.pending.as_mut().unwrap().body = "{}".into();
        journal.pending.as_mut().unwrap().response = Some(ChatTurn::default());
        assert!(journal.validate(0, "main").is_err());
    }
}
