mod agent;
mod coverage;
pub mod evaluation;
mod outline;
mod policy;
mod reconcile;
mod types;

use std::collections::HashSet;
use std::sync::Arc;

use agent::{AgentStats, OpenAiToolChat, ToolChat, extract_model_id};
use coverage::{candidate_spans, heuristic_candidates, uncovered_spans};
use outline::build_sections;
use policy::{ExtractionPolicy, default_policy};
use reconcile::reconcile_candidates;

pub use policy::{family_score, hint_family, resolve_must};
pub use reconcile::{normalize_quote, quote_in_body, quotes_overlap};
pub use types::{
    ClauseFamily, CoverageReport, ExtractedClause, ExtractedSection, ExtractedSpan,
    ExtractionDiagnostics, ExtractionFailure, ExtractionInput, ExtractionMode, ExtractionReport,
    ExtractionScope,
};

pub fn sections_for_document(markdown: &str) -> Result<Vec<ExtractedSection>, String> {
    let policy = default_policy()?;
    Ok(build_sections(markdown, &ExtractionScope::Document, policy))
}

pub fn configured_mode() -> Result<ExtractionMode, String> {
    ExtractionMode::from_env()
}

pub fn configured_model_id() -> String {
    extract_model_id()
}

pub fn embedded_policy_versions() -> Result<(&'static str, &'static str), String> {
    let policy = default_policy()?;
    Ok((&policy.version, &policy.prompt_version))
}

pub struct TenderExtractionEngine {
    policy: &'static ExtractionPolicy,
    mode: ExtractionMode,
    model_id: String,
    model_available: bool,
    chat: Arc<dyn ToolChat>,
}

impl TenderExtractionEngine {
    pub fn from_env() -> Result<Self, String> {
        let policy = default_policy()?;
        let mode = ExtractionMode::from_env()?;
        let model_id = extract_model_id();
        let model_available = OpenAiToolChat::configured(&model_id);
        validate_runtime_contract(mode, model_available)?;
        Ok(Self {
            policy,
            mode,
            model_id,
            model_available,
            chat: Arc::new(OpenAiToolChat),
        })
    }

    pub fn with_chat(
        policy: &'static ExtractionPolicy,
        mode: ExtractionMode,
        model_id: impl Into<String>,
        chat: Arc<dyn ToolChat>,
    ) -> Self {
        Self::with_chat_availability(policy, mode, model_id, true, chat)
    }

    fn with_chat_availability(
        policy: &'static ExtractionPolicy,
        mode: ExtractionMode,
        model_id: impl Into<String>,
        model_available: bool,
        chat: Arc<dyn ToolChat>,
    ) -> Self {
        Self {
            policy,
            mode,
            model_id: model_id.into(),
            model_available,
            chat,
        }
    }

    pub fn mode(&self) -> ExtractionMode {
        self.mode
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    pub fn policy_version(&self) -> &str {
        &self.policy.version
    }

    pub fn prompt_version(&self) -> &str {
        &self.policy.prompt_version
    }

    pub async fn extract(
        &self,
        input: ExtractionInput,
    ) -> Result<ExtractionReport, ExtractionFailure> {
        let mut sections = build_sections(&input.markdown, &input.scope, self.policy);
        let mut diagnostics = ExtractionDiagnostics {
            mode: self.mode.as_str().into(),
            model_id: self.model_id.clone(),
            policy_version: self.policy.version.clone(),
            prompt_version: self.policy.prompt_version.clone(),
            ..Default::default()
        };
        if sections.is_empty() {
            return Err(ExtractionFailure {
                message: "document contains no extractable text".into(),
                diagnostics,
            });
        }
        let model_configured = self.model_available;
        let mut candidates = Vec::new();

        if self.mode != ExtractionMode::Heuristic && model_configured {
            for family in ClauseFamily::ALL {
                match agent::run_family_agent(
                    self.policy,
                    family,
                    &sections,
                    &self.model_id,
                    self.chat.as_ref(),
                )
                .await
                {
                    Ok(outcome) => {
                        merge_stats(&mut diagnostics, &outcome.stats);
                        if outcome.stats.termination != "done" {
                            let reason = if outcome.stats.termination == "no_tool_call" {
                                format!("{}_agent_returned_no_tool_calls", family.as_str())
                            } else {
                                format!(
                                    "{}_agent_terminated:{}",
                                    family.as_str(),
                                    outcome.stats.termination
                                )
                            };
                            if self.mode == ExtractionMode::Agent {
                                return Err(ExtractionFailure {
                                    message: reason,
                                    diagnostics,
                                });
                            }
                            diagnostics.fallback_reasons.push(reason);
                        }
                        candidates.extend(outcome.candidates);
                    }
                    Err(error) => {
                        merge_stats(&mut diagnostics, &error.stats);
                        let reason = format!(
                            "{}_agent_failed:{}",
                            family.as_str(),
                            provider_error_category(&error.category)
                        );
                        if self.mode == ExtractionMode::Agent {
                            return Err(ExtractionFailure {
                                message: reason,
                                diagnostics,
                            });
                        }
                        diagnostics.fallback_reasons.push(reason);
                    }
                }
            }
        } else if self.mode == ExtractionMode::Hybrid {
            diagnostics
                .fallback_reasons
                .push("tool_model_not_configured".into());
        }

        let initial = reconcile_candidates(self.policy, &sections, candidates.clone());
        diagnostics.family_conflicts = initial.family_conflicts;

        if self.mode == ExtractionMode::Hybrid && model_configured {
            let uncovered = uncovered_spans(&sections, &initial.clauses);
            if uncovered.len() > self.policy.limits.max_sweep_spans {
                diagnostics.fallback_reasons.push(format!(
                    "span_sweep_cap:{}_of_{}",
                    self.policy.limits.max_sweep_spans,
                    uncovered.len()
                ));
            }
            for span in uncovered
                .into_iter()
                .take(self.policy.limits.max_sweep_spans)
            {
                if candidates.len() >= self.policy.limits.max_file_clauses {
                    break;
                }
                let mut span_succeeded = false;
                for family in ClauseFamily::ALL {
                    match agent::run_span_sweep(
                        self.policy,
                        family,
                        span,
                        &self.model_id,
                        self.chat.as_ref(),
                    )
                    .await
                    {
                        Ok(outcome) => {
                            merge_stats(&mut diagnostics, &outcome.stats);
                            span_succeeded |= !outcome.candidates.is_empty();
                            candidates.extend(outcome.candidates);
                        }
                        Err(error) => {
                            merge_stats(&mut diagnostics, &error.stats);
                            diagnostics.fallback_reasons.push(format!(
                                "span_sweep_failed:{}:{}:{}",
                                span.id,
                                family.as_str(),
                                provider_error_category(&error.category)
                            ));
                        }
                    }
                }
                if !span_succeeded {
                    diagnostics
                        .fallback_reasons
                        .push(format!("span_sweep_empty:{}", span.id));
                }
            }
        }

        let after_model = reconcile_candidates(self.policy, &sections, candidates.clone());
        if matches!(
            self.mode,
            ExtractionMode::Hybrid | ExtractionMode::Heuristic
        ) {
            let uncovered = uncovered_spans(&sections, &after_model.clauses);
            if !uncovered.is_empty() {
                diagnostics
                    .fallback_reasons
                    .push(format!("heuristic_spans:{}", uncovered.len()));
                candidates.extend(heuristic_candidates(self.policy, &uncovered));
            }
        }

        let reconciled = reconcile_candidates(self.policy, &sections, candidates);
        diagnostics.rejected_invalid_quotes += reconciled.rejected_invalid_quotes;
        diagnostics.family_conflicts = reconciled.family_conflicts;
        let all_candidates = candidate_spans(&sections);
        let candidate_ids: HashSet<_> =
            all_candidates.iter().map(|span| span.id.as_str()).collect();
        let covered: HashSet<_> = reconciled
            .clauses
            .iter()
            .map(|clause| clause.span_id.as_str())
            .filter(|span_id| candidate_ids.contains(span_id))
            .collect();
        let uncovered_ids: Vec<_> = all_candidates
            .iter()
            .filter(|span| !covered.contains(span.id.as_str()))
            .map(|span| span.id.clone())
            .collect();
        diagnostics.failed_spans = uncovered_ids.clone();
        diagnostics.coverage = CoverageReport {
            candidate_spans: all_candidates.len(),
            covered_spans: covered.len(),
            uncovered_spans: uncovered_ids.clone(),
            ambiguous_clauses: reconciled.family_conflicts,
        };

        for section in &mut sections {
            let candidate_ids: Vec<_> = section
                .spans
                .iter()
                .filter(|span| span.candidate)
                .map(|span| span.id.as_str())
                .collect();
            if candidate_ids.is_empty() {
                section.extract_status = "skipped".into();
            } else {
                let missing = candidate_ids
                    .iter()
                    .filter(|span_id| !covered.contains(**span_id))
                    .count();
                if missing == 0 {
                    section.extract_status = "done".into();
                } else {
                    section.extract_status = "failed".into();
                    section.error_message = format!("{missing} candidate spans uncovered");
                }
            }
        }

        if !diagnostics.coverage.uncovered_spans.is_empty() {
            if self.mode == ExtractionMode::Agent {
                return Err(ExtractionFailure {
                    message: "candidate_spans_uncovered".into(),
                    diagnostics,
                });
            }
            diagnostics.partial_failure = true;
        }
        if !diagnostics.fallback_reasons.is_empty() {
            diagnostics.partial_failure = true;
        }

        Ok(ExtractionReport {
            sections,
            clauses: reconciled.clauses,
            diagnostics,
        })
    }
}

fn provider_error_category(error: &str) -> &'static str {
    let error = error.to_ascii_lowercase();
    if error.contains("429") || error.contains("rate limit") {
        "rate_limited"
    } else if error.contains("timeout") || error.contains("timed out") {
        "timeout"
    } else if error.contains("401") || error.contains("403") || error.contains("auth") {
        "authentication"
    } else if error.contains("connect") || error.contains("unavailable") {
        "unavailable"
    } else {
        "provider_error"
    }
}

fn validate_runtime_contract(mode: ExtractionMode, model_available: bool) -> Result<(), String> {
    if mode == ExtractionMode::Agent && !model_available {
        Err("BID_EXTRACT_MODE=agent requires a configured tool-capable chat model".into())
    } else {
        Ok(())
    }
}

fn merge_stats(diagnostics: &mut ExtractionDiagnostics, stats: &AgentStats) {
    diagnostics.agent_rounds += stats.rounds;
    if !stats.termination.is_empty() {
        diagnostics
            .agent_terminations
            .push(stats.termination.clone());
    }
    diagnostics.retries += stats.retries;
    diagnostics.tool_calls += stats.tool_calls;
    diagnostics.rejected_invalid_quotes += stats.rejected_invalid_quotes;
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::extraction::agent::{
        ScriptedToolChat, ToolChat, ToolChatOptions, scripted_text_message, scripted_tool_message,
    };

    struct AlwaysFailChat;

    #[async_trait::async_trait]
    impl ToolChat for AlwaysFailChat {
        async fn complete(
            &self,
            _model: &str,
            _messages: &[async_openai::types::chat::ChatCompletionRequestMessage],
            _tools: &[async_openai::types::chat::ChatCompletionTools],
            _options: ToolChatOptions,
        ) -> Result<async_openai::types::chat::ChatCompletionResponseMessage, String> {
            Err("private provider message".into())
        }
    }

    #[tokio::test]
    async fn independent_agents_reconcile_conflict() {
        let policy = default_policy().unwrap();
        let preview = build_sections(
            "正文要求投标人提供说明。",
            &ExtractionScope::Document,
            policy,
        );
        let span = preview[0].spans[0].id.clone();
        let replies = vec![
            scripted_tool_message(
                "t1",
                "emit_clauses",
                json!({"clauses":[{"span_id":span,"quote":"投标人提供说明","text":"提供说明","must":false}]}),
            ),
            scripted_tool_message("t2", "done", json!({})),
            scripted_tool_message(
                "c1",
                "emit_clauses",
                json!({"clauses":[{"span_id":span,"quote":"投标人提供说明","text":"提供说明","must":false}]}),
            ),
            scripted_tool_message("c2", "done", json!({})),
        ];
        let engine = TenderExtractionEngine::with_chat(
            policy,
            ExtractionMode::Agent,
            "test-model",
            Arc::new(ScriptedToolChat::new(replies)),
        );
        let report = engine
            .extract(ExtractionInput::document(
                uuid::Uuid::new_v4(),
                "正文要求投标人提供说明。".into(),
            ))
            .await
            .unwrap();
        assert_eq!(report.clauses.len(), 1);
        assert!(report.clauses[0].family_conflict);
    }

    #[tokio::test]
    async fn terminal_provider_attempts_are_present_in_strict_and_hybrid_diagnostics() {
        let policy = default_policy().unwrap();
        let strict = TenderExtractionEngine::with_chat(
            policy,
            ExtractionMode::Agent,
            "test-model",
            Arc::new(AlwaysFailChat),
        );
        let failure = strict
            .extract(ExtractionInput::document(
                uuid::Uuid::new_v4(),
                "# 技术要求\n设备必须支持万兆接口。".into(),
            ))
            .await
            .unwrap_err();
        assert_eq!(failure.diagnostics.agent_rounds, 1);
        assert_eq!(failure.diagnostics.retries, policy.limits.max_retries - 1);
        assert!(!failure.message.contains("private provider message"));

        let hybrid = TenderExtractionEngine::with_chat(
            policy,
            ExtractionMode::Hybrid,
            "test-model",
            Arc::new(AlwaysFailChat),
        );
        let report = hybrid
            .extract(ExtractionInput::document(
                uuid::Uuid::new_v4(),
                "# 技术要求\n设备必须支持万兆接口。".into(),
            ))
            .await
            .unwrap();
        assert!(report.diagnostics.agent_rounds >= 2);
        assert!(report.diagnostics.retries >= 2 * (policy.limits.max_retries - 1));
        assert!(
            report
                .diagnostics
                .fallback_reasons
                .iter()
                .all(|reason| !reason.contains("private provider message"))
        );
    }

    #[tokio::test]
    async fn strict_agent_fails_when_candidate_spans_remain_uncovered() {
        let policy = default_policy().unwrap();
        let engine = TenderExtractionEngine::with_chat(
            policy,
            ExtractionMode::Agent,
            "test-model",
            Arc::new(ScriptedToolChat::new(vec![
                scripted_tool_message("t", "done", json!({})),
                scripted_tool_message("c", "done", json!({})),
            ])),
        );
        let failure = engine
            .extract(ExtractionInput::document(
                uuid::Uuid::new_v4(),
                "# 技术要求\n设备必须支持万兆接口。".into(),
            ))
            .await
            .unwrap_err();
        assert_eq!(failure.message, "candidate_spans_uncovered");
        assert_eq!(failure.diagnostics.coverage.uncovered_spans.len(), 1);
    }

    #[tokio::test]
    async fn one_emitted_clause_does_not_cover_coordinated_requirement() {
        let policy = default_policy().unwrap();
        let markdown = "# 技术要求\n设备必须支持万兆接口，并应提供双电源热插拔。";
        let preview = build_sections(markdown, &ExtractionScope::Document, policy);
        assert_eq!(preview[0].spans.len(), 2);
        let first = &preview[0].spans[0];
        let quote = first.body.trim_end_matches('，');
        let engine = TenderExtractionEngine::with_chat(
            policy,
            ExtractionMode::Agent,
            "test-model",
            Arc::new(ScriptedToolChat::new(vec![
                scripted_tool_message(
                    "emit",
                    "emit_clauses",
                    json!({"clauses":[{
                        "span_id": first.id,
                        "quote": quote,
                        "text": quote,
                        "must": true
                    }]}),
                ),
                scripted_tool_message("tech-done", "done", json!({})),
                scripted_tool_message("commercial-done", "done", json!({})),
            ])),
        );
        let failure = engine
            .extract(ExtractionInput::document(
                uuid::Uuid::new_v4(),
                markdown.into(),
            ))
            .await
            .unwrap_err();
        assert_eq!(failure.message, "candidate_spans_uncovered");
        assert_eq!(failure.diagnostics.coverage.uncovered_spans.len(), 1);
    }

    #[tokio::test]
    async fn strict_agent_rejects_plain_text_response() {
        let policy = default_policy().unwrap();
        let engine = TenderExtractionEngine::with_chat(
            policy,
            ExtractionMode::Agent,
            "test-model",
            Arc::new(ScriptedToolChat::new(vec![scripted_text_message("[]")])),
        );
        let failure = engine
            .extract(ExtractionInput::document(
                uuid::Uuid::new_v4(),
                "# 技术要求\n设备必须支持万兆接口。".into(),
            ))
            .await
            .unwrap_err();
        assert!(failure.message.contains("no_tool_calls"));
    }

    #[tokio::test]
    async fn hybrid_plain_text_response_is_visible_and_falls_back() {
        let policy = default_policy().unwrap();
        let engine = TenderExtractionEngine::with_chat(
            policy,
            ExtractionMode::Hybrid,
            "test-model",
            Arc::new(ScriptedToolChat::new(vec![
                scripted_text_message("[]"),
                scripted_text_message("[]"),
                scripted_text_message("[]"),
                scripted_text_message("[]"),
            ])),
        );
        let report = engine
            .extract(ExtractionInput::document(
                uuid::Uuid::new_v4(),
                "# 技术要求\n设备必须支持万兆接口。".into(),
            ))
            .await
            .unwrap();
        assert!(!report.clauses.is_empty());
        assert!(
            report
                .diagnostics
                .fallback_reasons
                .iter()
                .any(|reason| reason.contains("no_tool_calls"))
        );
        assert!(
            report
                .diagnostics
                .fallback_reasons
                .iter()
                .any(|reason| reason.starts_with("heuristic_spans:"))
        );
    }

    #[test]
    fn runtime_contract_is_strict_only_for_agent_mode() {
        assert!(validate_runtime_contract(ExtractionMode::Agent, false).is_err());
        assert!(validate_runtime_contract(ExtractionMode::Hybrid, false).is_ok());
        assert!(validate_runtime_contract(ExtractionMode::Heuristic, false).is_ok());
    }

    #[tokio::test]
    async fn golden_fixture_meets_offline_quality_gates() {
        assert_golden(
            include_str!("../../../../testdata/bid-extraction/cn-tender-golden-01.md"),
            include_str!("../../../../testdata/bid-extraction/cn-tender-golden-01.expected.json"),
        )
        .await;
        assert_golden(
            include_str!("../../../../testdata/bid-extraction/cn-tender-golden-02.md"),
            include_str!("../../../../testdata/bid-extraction/cn-tender-golden-02.expected.json"),
        )
        .await;
        assert_golden(
            include_str!("../../../../testdata/bid-extraction/cn-tender-golden-03.md"),
            include_str!("../../../../testdata/bid-extraction/cn-tender-golden-03.expected.json"),
        )
        .await;
    }

    async fn assert_golden(markdown: &str, expected: &str) {
        let policy = default_policy().unwrap();
        let engine = TenderExtractionEngine::with_chat(
            policy,
            ExtractionMode::Heuristic,
            "stub-chat",
            Arc::new(ScriptedToolChat::new(vec![])),
        );
        let expected: evaluation::GoldenExpected = serde_json::from_str(expected).unwrap();
        let report = engine
            .extract(ExtractionInput::document(
                uuid::Uuid::new_v4(),
                markdown.into(),
            ))
            .await
            .unwrap();
        let metrics = evaluation::evaluate(&report, &expected);
        assert!(metrics.passed, "{metrics:#?}");

        let mut with_duplicate = report.clone();
        with_duplicate.clauses.push(report.clauses[0].clone());
        let duplicate_metrics = evaluation::evaluate(&with_duplicate, &expected);
        assert!(!duplicate_metrics.passed);
        assert_eq!(duplicate_metrics.false_positives, 1);
        assert!(duplicate_metrics.precision < 1.0);

        let mut with_fragment = report.clone();
        with_fragment.clauses[0].quote = with_fragment.clauses[0].quote.chars().take(4).collect();
        with_fragment.clauses[0].text = with_fragment.clauses[0].quote.clone();
        let fragment_metrics = evaluation::evaluate(&with_fragment, &expected);
        assert!(!fragment_metrics.passed);
        assert_eq!(fragment_metrics.false_positives, 1);
    }

    #[tokio::test]
    async fn hybrid_without_model_records_visible_fallback() {
        let policy = default_policy().unwrap();
        let engine = TenderExtractionEngine::with_chat_availability(
            policy,
            ExtractionMode::Hybrid,
            "stub-chat",
            false,
            Arc::new(ScriptedToolChat::new(vec![])),
        );
        let report = engine
            .extract(ExtractionInput::document(
                uuid::Uuid::new_v4(),
                "# 技术要求\n设备必须支持万兆接口。".into(),
            ))
            .await
            .unwrap();
        assert!(!report.clauses.is_empty());
        assert!(
            report
                .diagnostics
                .fallback_reasons
                .contains(&"tool_model_not_configured".to_string())
        );
    }

    #[tokio::test]
    async fn heuristic_mode_never_needs_chat() {
        let policy = default_policy().unwrap();
        let engine = TenderExtractionEngine::with_chat(
            policy,
            ExtractionMode::Heuristic,
            "stub-chat",
            Arc::new(ScriptedToolChat::new(vec![])),
        );
        let report = engine
            .extract(ExtractionInput::document(
                uuid::Uuid::new_v4(),
                "# 技术要求\n设备必须支持万兆接口。".into(),
            ))
            .await
            .unwrap();
        assert!(
            report
                .clauses
                .iter()
                .any(|clause| clause.text.contains("万兆"))
        );
        assert!(
            report
                .diagnostics
                .fallback_reasons
                .iter()
                .any(|reason| reason.starts_with("heuristic_spans:"))
        );
    }
}
