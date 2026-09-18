use super::agent::{Checkpoint, Config, Journal, Limits};
use super::*;
use crate::{agent_error::AgentError, authoring_runtime::AuthoringRuntimeContractV1};
use async_trait::async_trait;
use serde_json::json;
use std::{collections::BTreeMap, sync::Mutex};
use tokio_util::sync::CancellationToken;

mod draft;
mod input;

fn input() -> FrozenInput {
    FrozenInput {
        schema_version: 1,
        project_id: "project".into(),
        document_set_id: "set".into(),
        documents: vec![],
        document_relations: vec![],
        decisions: vec![],
        structured_forms: vec![],
        source_units: vec![Source {
            source_unit_revision_id: "source".into(),
            document_id: "document".into(),
            text: "提交格式见附表甲。".into(),
            locator: json!({"heading_path":"须知"}),
            ordinal: 0,
        }],
    }
}

fn grid_citation_input() -> FrozenInput {
    let mut input = input();
    input.source_units[0].text.clear();
    input.structured_forms.push(
        json!({"form_definition_revision_id":"grid","source_unit_revision_id":"source",
        "definition":{"kind":"grid","row_count":2,"column_count":2,"widths_mm":[40,40],
            "cells":[{"row":0,"column":0,"row_span":1,"col_span":2,"text":"产品条件"},
                {"row":1,"column":0,"row_span":1,"col_span":1,"text":"投标时逐项响应"},
                {"row":1,"column":1,"row_span":1,"col_span":1,"text":"供货时提供证书"}]}}),
    );
    input
}

#[derive(Default)]
struct MemoryJournal {
    sdk_turns: Mutex<Vec<usize>>,
    strict_checkpoint_identity: bool,
    reject_compilation_checkpoint: bool,
    fail_boundary_ack: Mutex<Option<usize>>,
    cancel_boundary: Mutex<Option<(usize, CancellationToken)>>,
    reject_reservation: Mutex<bool>,
    state: Mutex<Option<Checkpoint>>,
    reservations: Mutex<BTreeMap<usize, (Vec<u8>, usize)>>,
    interrupt_after: Mutex<Option<usize>>,
    view: Mutex<Option<views::SourceView>>,
    view_calls: Mutex<usize>,
}
#[async_trait]
impl Journal for MemoryJournal {
    async fn source_view(
        &self,
        _: &str,
        _: &Limits,
        _: &CancellationToken,
    ) -> Result<views::SourceView, AgentError> {
        *self.view_calls.lock().unwrap() += 1;
        self.view
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| AgentError::new("SOURCE_VIEW_UNAVAILABLE", "injected unavailable view"))
    }
    async fn load(&self) -> Result<Option<Checkpoint>, AgentError> {
        Ok(self.state.lock().unwrap().clone())
    }
    async fn reserve(&self, state: &Checkpoint, body: &[u8]) -> Result<Option<usize>, AgentError> {
        if *self.reject_reservation.lock().unwrap() {
            return Err(crate::agent_error::AgentError::new(
                "INTERNAL",
                "reservation transaction rejected",
            ));
        }
        self.sdk_turns
            .lock()
            .unwrap()
            .push(state.journal.session.as_ref().unwrap().turn());
        let mut rows = self.reservations.lock().unwrap();
        let row = rows.entry(state.turn).or_insert_with(|| (body.to_vec(), 0));
        assert_eq!(
            row.0, body,
            "same boundary must preserve exact provider body"
        );
        if row.1 == 3 {
            return Ok(None);
        }
        row.1 += 1;
        *self.state.lock().unwrap() = Some(state.clone());
        if let Some((sequence, token)) = &*self.cancel_boundary.lock().unwrap()
            && *sequence == state.journal.sequence
        {
            token.cancel();
        }
        let mut boundary = self.fail_boundary_ack.lock().unwrap();
        if *boundary == Some(state.journal.sequence) {
            *boundary = None;
            return Err(crate::agent_error::AgentError::new(
                "INTERNAL",
                "lost boundary acknowledgement",
            ));
        }
        Ok(Some(row.1))
    }
    async fn save(&self, state: &Checkpoint, _: &serde_json::Value) -> Result<(), AgentError> {
        if self.reject_compilation_checkpoint && state.draft_docx_base64.is_some() {
            return Err(AgentError::new(
                "FROZEN_INPUT_DIGEST_MISMATCH",
                "injected compilation checkpoint rejection",
            ));
        }
        if self.strict_checkpoint_identity {
            let saved = self.state.lock().unwrap();
            if let Some(prior) = saved.as_ref() {
                if prior.journal.sequence == state.journal.sequence && json!(prior) != json!(state)
                {
                    return Err(AgentError::new(
                        "FROZEN_INPUT_DIGEST_MISMATCH",
                        "divergent checkpoint",
                    ));
                }
                if state.draft_docx_base64.is_some() && prior.draft_docx_base64.is_none() {
                    assert_eq!(state.journal.sequence, prior.journal.sequence + 1);
                    assert_eq!(state.turn, prior.turn);
                    assert!(state.journal.pending.is_none());
                    assert_eq!(json!(state.analysis), json!(prior.analysis));
                }
            }
        }
        *self.state.lock().unwrap() = Some(state.clone());
        if let Some((sequence, token)) = &*self.cancel_boundary.lock().unwrap()
            && *sequence == state.journal.sequence
        {
            token.cancel();
        }
        let mut boundary = self.fail_boundary_ack.lock().unwrap();
        if *boundary == Some(state.journal.sequence) {
            *boundary = None;
            return Err(crate::agent_error::AgentError::new(
                "INTERNAL",
                "lost boundary acknowledgement",
            ));
        }
        let mut interrupt = self.interrupt_after.lock().unwrap();
        if *interrupt == Some(state.turn) && state.journal.pending.is_none() {
            *interrupt = None;
            return Err(AgentError::new(
                "INTERNAL",
                "simulated lost checkpoint acknowledgement",
            ));
        }
        Ok(())
    }
}

pub(super) fn config() -> Config {
    // Test provider never makes network calls; production resolves this identity
    // from configuration and freezes it before execution.
    let provider: AuthoringRuntimeContractV1 = serde_json::from_value(json!({
        "schema_version":1,"base_url":"https://llm.example/v1",
        "endpoint":"https://llm.example/v1/chat/completions","protocol":"openai_chat_completions_sse","model_id":"test-frozen-model",
        "credential_ref":"env:LLM_API_KEY","stream":true,"max_tokens":8192,"timeout_ms":180000,"response_mode":"tool_calls",
        "transport_retries":0,"temperature":null,"reasoning_effort":null
    }))
    .unwrap();
    Config::with_provider(
        provider,
        Limits {
            max_no_progress_turns: 6,
            max_focus_turns: 24,
            max_focus_replans: 2,
            max_turns: 40,
            max_tool_calls: 80,
            max_read_bytes: 1000000,
            max_context_bytes: 100000,
            max_history_bytes: 32000,
            max_context_tokens: 1_000_000,
            image_token_reserve: 16000,
            token_safety_margin: 2048,
            max_tool_result_bytes: 16000,
            max_review_rounds: 3,
            max_source_view_edge: 1600,
            max_source_view_bytes: 16000,
            reviewer_reserve: 0,
            pack_max_units: 1,
            pack_max_chars: 0,
            pack_max_turns: 0,
            draft_path: false,
            draft_bind_terms: vec![],
            max_draft_docx_bytes: crate::tender_analysis::draft::DRAFT_MAX_DOCX_BYTES,
        },
    )
    .unwrap()
}
