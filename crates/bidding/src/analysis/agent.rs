pub(crate) mod discover_coordinator;
mod discover_parallel;
use super::*;
use crate::agent_runtime::{Driver, Status, drive};
pub(super) mod context;
#[cfg(test)]
mod retirement;
mod view_io;
use crate::agent_runtime::progress::{Progress, Recovery};
use crate::{agent_error::AgentError, authoring_runtime::AuthoringRuntimeContractV1};
use async_trait::async_trait;
pub use context::WorkState;
use context::WorkStatus;
use knowledge::models::ChatTurn;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::time::Instant;
use tokio_util::sync::CancellationToken;
use view_io::read_source_view;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    /// Explicit capability declaration for the frozen authoring provider/model.
    /// Images never switch to a different provider behind this contract.
    pub vision_enabled: bool,
    pub tokenizer: crate::agent_runtime::chat::TokenizerProfile,
    #[serde(default = "crate::agent_runtime::progress::default_no_progress_turns")]
    pub max_no_progress_turns: usize,
    #[serde(default = "crate::agent_runtime::progress::default_focus_turns")]
    pub max_focus_turns: usize,
    #[serde(default = "crate::agent_runtime::progress::default_focus_replans")]
    pub max_focus_replans: usize,
    /// Application token budget, including estimated input and reserved output.
    pub max_context_tokens: usize,
    pub image_token_reserve: usize,
    pub token_safety_margin: usize,
    pub max_source_view_bytes: usize,
    pub max_source_view_edge: u32,
    /// 可选组成绑定附加词；缺省只用 title 包含匹配，禁止代码内置行业词表。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub draft_bind_terms: Vec<String>,
}

impl Limits {
    /// Explicit ceiling for the currently admitted model profiles. Raising it
    /// requires a newly verified profile contract, never a Limits override.
    pub const PROFILE_CONTEXT_CEILING: usize = 131_072;
    /// Allocation ceiling derived from the sole context-token allowance, not
    /// an independent segmentation or admission policy. Actual requests are
    /// admitted exclusively by tokenizer accounting plus output reservation.
    pub fn context_wire_ceiling(&self) -> usize {
        let text = self
            .max_context_tokens
            .saturating_mul(self.tokenizer.max_piece_bytes());
        let images = self.max_context_tokens / self.image_token_reserve.max(1);
        let encoded_image = (self.max_source_view_bytes.saturating_add(2) / 3).saturating_mul(4);
        text.saturating_add(images.saturating_mul(encoded_image.saturating_add(32)))
    }

    fn progress(&self) -> crate::agent_runtime::progress::ProgressLimits {
        crate::agent_runtime::progress::ProgressLimits {
            max_no_progress_turns: self.max_no_progress_turns,
            max_focus_turns: self.max_focus_turns,
            max_focus_replans: self.max_focus_replans,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub checkpoint_contract_version: u32,
    pub runtime_adapter: String,
    pub provider: AuthoringRuntimeContractV1,
    pub limits: Limits,
    pub tools_sha256: String,
    pub main_prompt_sha256: String,
}

impl Config {
    pub fn from_environment() -> Result<Self, AgentError> {
        let provider = AuthoringRuntimeContractV1::resolve_tools_from_environment()
            .map_err(|e| error("AGENT_PROVIDER_UNAVAILABLE", e))?;
        let limits = Self::environment_limits(&provider)?;
        Self::with_provider(provider, limits)
    }

    pub fn from_environment_for(input: &FrozenInput) -> Result<Self, AgentError> {
        let provider = AuthoringRuntimeContractV1::resolve_tools_from_environment()
            .map_err(|e| error("AGENT_PROVIDER_UNAVAILABLE", e))?;
        let limits = Self::environment_limits(&provider)?;
        Self::with_provider_for(provider, limits, Some(input))
    }

    fn environment_limits(provider: &AuthoringRuntimeContractV1) -> Result<Limits, AgentError> {
        Self::configured_limits(
            provider,
            std::env::var("KB_AUTHORING_TOKENIZER_PROFILE")
                .ok()
                .as_deref(),
            std::env::var("KB_TENDER_AGENT_LIMITS").ok().as_deref(),
        )
    }

    fn configured_limits(
        provider: &AuthoringRuntimeContractV1,
        profile: Option<&str>,
        overrides: Option<&str>,
    ) -> Result<Limits, AgentError> {
        let tokenizer = match profile.filter(|v| !v.trim().is_empty()) {
            Some(raw) => {
                // Model identity comes from the selected provider, not a duplicate setting.
                let mut value: Value = serde_json::from_str(raw).map_err(invalid)?;
                let object = value
                    .as_object_mut()
                    .ok_or_else(|| invalid("tokenizer profile must be an object"))?;
                if object.contains_key("model_id") {
                    return Err(invalid(
                        "tokenizer profile model_id is derived from configured provider",
                    ));
                }
                object.insert("model_id".into(), json!(provider.model_id));
                serde_json::from_value(value).map_err(invalid)?
            }
            None => {
                crate::agent_runtime::TokenizerProfile::published_for_model(&provider.model_id)?
            }
        };
        let mut limits = Self::default_limits(provider, tokenizer)?;
        // Optional scalar tuning. The model-bound tokenizer and capabilities
        // are independent of this object; no complete Limits JSON is required.
        if let Some(raw) = overrides.filter(|v| !v.trim().is_empty()) {
            let overrides: Value = serde_json::from_str(raw).map_err(invalid)?;
            let object = overrides
                .as_object()
                .ok_or_else(|| invalid("optional limits must be an object"))?;
            if object.contains_key("tokenizer") {
                return Err(invalid(
                    "use model-bound KB_AUTHORING_TOKENIZER_PROFILE instead of limits.tokenizer",
                ));
            }
            let mut effective = serde_json::to_value(&limits).map_err(invalid)?;
            for (key, value) in object {
                effective[key] = value.clone();
            }
            limits = serde_json::from_value(effective).map_err(invalid)?;
        }
        Ok(limits)
    }

    fn default_limits(
        provider: &AuthoringRuntimeContractV1,
        tokenizer: crate::agent_runtime::TokenizerProfile,
    ) -> Result<Limits, AgentError> {
        tokenizer.validate_for_model(&provider.model_id)?;
        let context = Limits::PROFILE_CONTEXT_CEILING;
        Ok(Limits {
            vision_enabled: false,
            tokenizer,
            max_no_progress_turns: crate::agent_runtime::progress::default_no_progress_turns(),
            max_focus_turns: crate::agent_runtime::progress::default_focus_turns(),
            max_focus_replans: crate::agent_runtime::progress::default_focus_replans(),
            max_context_tokens: context,
            image_token_reserve: 16384,
            token_safety_margin: 4096,
            // Image decode/resize safeguards; never a cumulative reading allowance.
            max_source_view_bytes: 1_500_000,
            max_source_view_edge: 1800,
            draft_bind_terms: Vec::new(),
        })
    }

    pub fn with_provider(
        provider: AuthoringRuntimeContractV1,
        limits: Limits,
    ) -> Result<Self, AgentError> {
        Self::with_provider_for(provider, limits, None)
    }

    pub fn with_provider_for(
        provider: AuthoringRuntimeContractV1,
        limits: Limits,
        _input: Option<&FrozenInput>,
    ) -> Result<Self, AgentError> {
        let tools_sha256 = digest(&crate::outline::agent::schemas()).map_err(invalid)?;
        let main_prompt_sha256 = digest(&crate::agent_runtime::chat::system_content(
            crate::outline::agent::OUTLINE_PROMPT,
        ))
        .map_err(invalid)?;
        let config = Self {
            checkpoint_contract_version: crate::agent_runtime::CHECKPOINT_CONTRACT_VERSION,
            runtime_adapter: crate::agent_runtime::RUNTIME_ADAPTER_VERSION.into(),
            provider,
            limits,
            tools_sha256,
            main_prompt_sha256,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), AgentError> {
        self.provider.validate().map_err(invalid)?;
        let l = &self.limits;
        if !l.progress().validate() {
            return Err(invalid("progress limits must be positive"));
        }
        if self.checkpoint_contract_version != crate::agent_runtime::CHECKPOINT_CONTRACT_VERSION {
            return Err(error(
                "FROZEN_INPUT_DIGEST_MISMATCH",
                format!(
                    "checkpoint contract version {} is not {}",
                    self.checkpoint_contract_version,
                    crate::agent_runtime::CHECKPOINT_CONTRACT_VERSION
                ),
            ));
        }
        if self.runtime_adapter != crate::agent_runtime::RUNTIME_ADAPTER_VERSION {
            return Err(error(
                "FROZEN_INPUT_DIGEST_MISMATCH",
                format!(
                    "runtime adapter {} is not {}",
                    self.runtime_adapter,
                    crate::agent_runtime::RUNTIME_ADAPTER_VERSION
                ),
            ));
        }
        if self.provider.response_mode != "tool_calls" {
            return Err(invalid("response_mode must be tool_calls"));
        }
        if l.max_context_tokens > Limits::PROFILE_CONTEXT_CEILING {
            return Err(invalid(
                "max_context_tokens exceeds the current model profile ceiling of 131072; a larger window requires a newly verified profile",
            ));
        }
        l.tokenizer.validate_for_model(&self.provider.model_id)?;
        if l.image_token_reserve == 0 {
            return Err(invalid("image_token_reserve must be positive"));
        }
        if l.token_safety_margin == 0 {
            return Err(invalid("token_safety_margin must be positive"));
        }
        if l.token_safety_margin
            .checked_add(self.provider.output_token_reserve as usize)
            .is_none_or(|reserved| reserved >= l.max_context_tokens)
        {
            return Err(invalid(
                "token_safety_margin plus output_token_reserve must fit in max_context_tokens",
            ));
        }
        if l.max_source_view_bytes == 0 || l.max_source_view_edge == 0 {
            return Err(invalid("source view limits must be positive"));
        }
        let outline = digest(&crate::outline::agent::schemas()).map_err(invalid)?;
        if self.tools_sha256 != outline {
            return Err(error(
                "FROZEN_INPUT_DIGEST_MISMATCH",
                "outline tools digest changed",
            ));
        }
        if self.main_prompt_sha256
            != digest(&crate::agent_runtime::chat::system_content(
                crate::outline::agent::OUTLINE_PROMPT,
            ))
            .map_err(invalid)?
        {
            return Err(error(
                "FROZEN_INPUT_DIGEST_MISMATCH",
                "outline prompt digest changed",
            ));
        }
        Ok(())
    }

    /// 填章 Job 走 `docx_compose` 的请求轨道，那张 identity 表要求
    /// `contract_definition->'config'` 与 `frozen_input->'config'` 逐字节相同。
    /// 提示与工具本身由 `*_sha256` 钉住，这里不重复内联。
    pub fn contract_definition(&self) -> Value {
        json!({
            "checkpoint_contract_version": crate::agent_runtime::CHECKPOINT_CONTRACT_VERSION,
            "runtime_adapter": crate::agent_runtime::RUNTIME_ADAPTER_VERSION,
            "config": self,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Main,
}

fn no_stall(stalls: &usize) -> bool {
    *stalls == 0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    pub journal: crate::agent_runtime::TurnJournal,
    pub input_sha256: String,
    pub config_sha256: String,
    pub turn: usize,
    pub tool_calls: usize,
    pub read_bytes: usize,
    pub review_rounds: usize,
    pub role: Role,
    pub analysis: Analysis,
    pub review: Option<Review>,
    /// Coverage after pending read results, committed only after the next
    /// complete model response. Belongs to `role`, not to the other Agent.
    pub pending_coverage: Option<Coverage>,
    pub transcript: Vec<Value>,
    #[serde(default)]
    pub main_progress: Progress,
    pub main_work: Option<WorkState>,
    pub done: bool,
    pub source_views: BTreeMap<String, views::SourceView>,
    #[serde(default)]
    pub draft_stage: crate::analysis::draft::DraftStage,
    /// 上一轮宿主完整性清单的缺口条数，用来判定修补轮是否还在减少缺口。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft_outline_gaps: Option<usize>,
    #[serde(default, skip_serializing_if = "no_stall")]
    pub draft_outline_stalls: usize,
    #[serde(default, skip_serializing_if = "no_stall")]
    pub draft_outline_window: usize,
    /// 这次填章是用户叫停的，不是填完了。稿子照出，剩下的章仍空着。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline_config_sha256: Option<String>,
    pub outline_run: crate::analysis::outline_flow::OutlineRun,
}

impl Checkpoint {
    // A prior draft is repair feedback, never a completed review. In normal
    // repair cycles the finalized report remains the authoritative feedback.
    pub(super) fn work(&self) -> Option<&WorkState> {
        self.main_work.as_ref()
    }
    fn execution(&self) -> &Progress {
        &self.main_progress
    }
    pub(super) fn coverage(&self) -> &Coverage {
        &self.analysis.coverage
    }
    pub(super) fn replace_coverage(&mut self, coverage: Coverage) -> Coverage {
        std::mem::replace(&mut self.analysis.coverage, coverage)
    }

    pub fn progress(&self, input: &FrozenInput) -> Value {
        let coverage = &self.analysis.coverage;
        let product_outline =
            self.outline_run.reading_packs.is_some() || !self.outline_run.tool_draft.is_empty();
        let outline_chapters = if product_outline {
            self.outline_run.tool_draft.chapters.len()
        } else {
            self.analysis.draft_plan.len()
        };
        let outline_requirements = self
            .outline_run
            .reading_packs
            .as_ref()
            .map(crate::outline::discover::DiscoverWork::requirement_count)
            .unwrap_or(self.analysis.outline.requirements.len());
        let mut progress = json!({"phase":self.role,"draft_stage":self.draft_stage,"outline_phase":self.analysis.outline.phase,
            "outline_repairing":!self.analysis.outline.checks.is_empty() && matches!(self.analysis.outline.phase, super::outline_flow::Phase::Discover | super::outline_flow::Phase::Outline),
            "outline_chapters":outline_chapters,
            "outline_scan_repair":self.outline_run.reading_packs.as_ref().map(|work| work.pack_counts().failed > 0).unwrap_or_else(|| super::outline_flow::scan_repair_pending(self)),
            "outline_requirements":outline_requirements,
            "outline_unmapped_forms":crate::outline::tools::unmapped_forms(input, &self.outline_run.tool_draft).len(),
            "outline_slots_submitted":self.outline_run.tool_draft.slots_submitted,
            "outline_finished":self.outline_run.tool_draft.finished,
            "outline_open_issues":self.analysis.outline.issues.values().filter(|issue| issue.status == crate::analysis::outline_flow::IssueStatus::Open).count(),
            "turn":self.turn,"tool_calls":self.tool_calls,
            "read_bytes":self.read_bytes,"review_rounds":self.review_rounds,"records":self.analysis.records.len(),
            "relations":self.analysis.relations.len(),"unread_ranges":tools::reading_gaps(input,coverage).len(),
            "source_count":input.source_units.len(),"disposition_count":self.analysis.dispositions.len(),
            "source_views":coverage.views.len(),"source_view_failures":coverage.view_failures.len(),
            "review_findings":self.review.as_ref().map_or(0,|r|r.findings.len()),

            // 填章面板要显示「已填 N/M 章 + 当前章」，这三项是它唯一的数据来源。
            "draft_chapters":self.analysis.draft_plan.iter()
                .filter(|item| item.status != crate::analysis::draft::DraftStatus::Omitted).count(),
            "draft_filled":self.analysis.draft_plan.iter()
                .filter(|item| item.status == crate::analysis::draft::DraftStatus::Filled).count(),
            "execution_watch":self.execution().watch,"execution_blockers":self.main_progress.blockers.len()});
        if let Some(work) = &self.outline_run.reading_packs {
            let counts = work.pack_counts();
            progress["outline_pack_total"] = json!(counts.total);
            progress["outline_pack_pending"] = json!(counts.pending);
            progress["outline_pack_running"] = json!(counts.running);
            progress["outline_pack_failed"] = json!(counts.failed);
            progress["outline_pack_committed"] = json!(counts.committed);
        }
        progress
    }
}

fn tool_result_envelope(value: Value) -> Value {
    if value.get("ok") == Some(&Value::Bool(false)) {
        json!({"ok":false,"error":"BUSINESS_REJECTED","result":value})
    } else {
        json!({"ok":true,"result":value})
    }
}

#[cfg(test)]
#[test]
fn business_rejection_is_false_at_model_boundary() {
    let value = json!({"ok":false,"feedback":{"code":"stale_claim"},"state_changed":false});
    let output = tool_result_envelope(value.clone());
    assert_eq!(output["ok"], false);
    assert_eq!(output["result"], value);
    assert_eq!(tool_result_envelope(json!({"ok":true}))["ok"], true);
}

#[async_trait]
pub trait Journal: Send + Sync {
    async fn load(&self) -> Result<Option<Checkpoint>, AgentError>;
    /// Read-only admission of projected accounting, before any durable reservation.
    async fn admit(&self, _state: &Checkpoint, _body: &[u8]) -> Result<(), AgentError> {
        Ok(())
    }
    /// Reserve before HTTP; None means the attempt-independent boundary budget
    /// is exhausted. Persist the exact request bytes, including tool contracts.
    async fn reserve(&self, state: &Checkpoint, body: &[u8]) -> Result<Option<usize>, AgentError>;
    /// The single Discover coordinator atomically persists all worker journals
    /// and shared accounting under the same root lease, before dispatch.
    async fn reserve_discover_worker(
        &self,
        state: &Checkpoint,
        request_id: &str,
        body: &[u8],
    ) -> Result<(), AgentError> {
        let valid=state.outline_run.discover_workers.values().any(|worker| matches!(&worker.pending,
            Some(discover_coordinator::Pending::Sending{request}) if request.scope.request==request_id && request.body==body));
        if !valid {
            return Err(invalid("worker reservation identity mismatch"));
        }
        self.save(state, &serde_json::json!({"discover_request":request_id}))
            .await
    }
    async fn save(&self, state: &Checkpoint, progress: &Value) -> Result<(), AgentError>;
    async fn source_view(
        &self,
        _source_id: &str,
        _limits: &Limits,
        _cancel: &CancellationToken,
    ) -> Result<views::SourceView, AgentError> {
        Err(error(
            "SOURCE_VIEW_UNAVAILABLE",
            "source view service is not available",
        ))
    }
    /// 用户在填章过程中点了「停止填充」。停不是杀进程：本章写完就不再派新章，
    /// 由正常收尾出稿，已填的章一个不丢，没填的仍是空 heading。
    async fn stop_requested(&self) -> Result<bool, AgentError> {
        Ok(false)
    }
    /// Write the projected outline. The default keeps unit journals free of a database.
    async fn publish_outline(
        &self,
        _artifact: &crate::outline::OutlineArtifact,
        _bindings: &[crate::outline::chapters::AttachmentBinding],
    ) -> Result<(), AgentError> {
        Ok(())
    }
}

#[async_trait]
pub trait Model: Send + Sync {
    async fn turn(&self, config: &Config, body: &[u8]) -> Result<ChatTurn, AgentError>;
}

pub struct ConfiguredModel;
#[async_trait]
impl Model for ConfiguredModel {
    async fn turn(&self, config: &Config, body: &[u8]) -> Result<ChatTurn, AgentError> {
        crate::agent_runtime::chat::provider_turn(&config.provider, body).await
    }
}

fn error(code: &str, message: impl Into<String>) -> AgentError {
    AgentError::new(code, message)
}

/// One-shot outline stalls at every phase. The old scan path still blocks only
/// while discovery has no reading packs.
pub(in crate::analysis) fn outline_execution_blocked(state: &Checkpoint) -> bool {
    state.main_progress.watch.recovery == Recovery::Blocked
        && (state.outline_run.reading_packs.is_some()
            || state.analysis.outline.phase == super::outline_flow::Phase::Discover)
}

/// Stall stop for the one-shot outline. Names the checkpoint phase.
/// This is failed progress, not a turn budget.
pub(in crate::analysis) fn outline_stall_message(state: &Checkpoint) -> String {
    let phase = match state.analysis.outline.phase {
        super::outline_flow::Phase::Discover => "discover",
        super::outline_flow::Phase::Outline => "outline",
        super::outline_flow::Phase::Check => "check",
        super::outline_flow::Phase::Complete => "complete",
    };
    format!("outline stalled in phase {phase}: no progress; checkpoint retained")
}
fn invalid(message: impl std::fmt::Display) -> AgentError {
    error("AGENT_OUTPUT_INVALID", message.to_string())
}

struct RunDriver<'a, J, M> {
    input: &'a FrozenInput,
    config: &'a Config,
    state: &'a mut Checkpoint,
    journal: &'a J,
    model: &'a M,
    cancel: &'a CancellationToken,
}

#[async_trait]
impl<J: Journal, M: Model> Driver for RunDriver<'_, J, M> {
    fn status(&self) -> Status<'_> {
        let limits = &self.config.limits;
        Status {
            journal: &self.state.journal,
            max_context_bytes: limits.context_wire_ceiling(),
            turn: self.state.turn,
            role: if self.state.role == Role::Main {
                "main"
            } else {
                "reviewer"
            },
            done: self.state.done,
            execution_blocked: (self.state.role == Role::Main
                && self.state.draft_stage == draft::DraftStage::Outline
                && outline_execution_blocked(self.state)),
        }
    }
    fn journal_mut(&mut self) -> &mut crate::agent_runtime::TurnJournal {
        &mut self.state.journal
    }
    async fn prepare_request(&mut self) -> Result<Vec<u8>, AgentError> {
        if crate::outline::agent::current(self.input, self.state)
            == crate::outline::agent::Duty::Discover
        {
            discover_parallel::run(
                self.input,
                self.config,
                self.state,
                self.journal,
                self.model,
                self.cancel,
            )
            .await?;
        }
        let started = Instant::now();
        let body = request(self.input, self.config, self.state).await?;
        let accounting = crate::agent_runtime::chat::estimate_request_tokens_with_reserve(
            &serde_json::from_slice(&body).map_err(invalid)?,
            &self.config.limits.tokenizer,
            self.config.limits.image_token_reserve,
            self.config.limits.token_safety_margin,
            self.config.provider.output_token_reserve as usize,
        )?;
        tracing::info!(event="analysis_request_built",turn=self.state.turn,role=?self.state.role,
            request_bytes=body.len(),estimated_input_tokens=accounting.total_input_tokens,
            context_tokens=accounting.total_context_tokens,
            tokenizer=?accounting.encoding, calibrated=accounting.calibrated,
            tokenizer_provenance=%accounting.provenance,
            image_tokens=accounting.image_tokens, framing_tokens=accounting.framing_tokens,
            reserved_output_tokens=self.config.provider.output_token_reserve,
            elapsed_ms=started.elapsed().as_millis() as u64);
        self.state.journal.prepare_session(
            &body,
            crate::agent_runtime::SESSION_PREFIX,
            crate::agent_runtime::ANALYSIS_SESSION_SUFFIX,
            self.config.limits.context_wire_ceiling(),
        )?;
        Ok(body)
    }
    async fn admit(&mut self, body: &[u8]) -> Result<(), AgentError> {
        let estimated = context::estimate_input_tokens(
            &serde_json::from_slice(body).map_err(invalid)?,
            &self.config.limits,
        )?;
        let mut projected = self.state.clone();
        projected.journal.accounting.record(
            estimated as u64,
            self.config.provider.output_token_reserve as u64,
            crate::agent_runtime::budget::unix_seconds(),
        )?;
        if let Err(error) = self.journal.admit(&projected, body).await {
            self.journal
                .save(self.state, &self.state.progress(self.input))
                .await?;
            return Err(error);
        }
        Ok(())
    }
    async fn reserve(&mut self, body: &[u8], _local_attempt: usize) -> Result<usize, AgentError> {
        let estimated = context::estimate_input_tokens(
            &serde_json::from_slice(body).map_err(invalid)?,
            &self.config.limits,
        )?;
        if let Err(error) = self.state.journal.accounting.record(
            estimated as u64,
            self.config.provider.output_token_reserve as u64,
            crate::agent_runtime::budget::unix_seconds(),
        ) {
            if let Some(pending) = self.state.journal.pending.as_mut() {
                pending.physical_attempts = pending.physical_attempts.saturating_sub(1);
            }
            self.journal
                .save(self.state, &self.state.progress(self.input))
                .await?;
            return Err(error);
        }
        // A restored prepared request skips prepare_request. Received replay
        // skips reserve entirely and still commits its already saved response.
        self.journal
            .reserve(self.state, body)
            .await?
            .ok_or_else(|| {
                error(
                    "AGENT_TURN_BUDGET_EXCEEDED",
                    "physical boundary budget exhausted",
                )
            })
    }
    async fn call_model(&self, body: &[u8]) -> Result<ChatTurn, AgentError> {
        self.model.turn(self.config, body).await
    }
    async fn execute(
        &mut self,
        response: ChatTurn,
        suppressed: BTreeMap<String, String>,
        cancel: &CancellationToken,
    ) -> Result<Vec<Value>, AgentError> {
        execute_turn(
            self.input,
            self.config,
            self.state,
            self.journal,
            response,
            suppressed,
            cancel,
        )
        .await
    }
    fn block_message(&self) -> String {
        if self.state.role == Role::Main
            && self.state.draft_stage == draft::DraftStage::Outline
            && outline_execution_blocked(self.state)
        {
            outline_stall_message(self.state)
        } else {
            "local execution and independent-work handoff allowances exhausted; blockers and checkpoint retained"
                .into()
        }
    }
    async fn save(&self) -> Result<(), AgentError> {
        self.journal
            .save(self.state, &self.state.progress(self.input))
            .await
    }
}

pub async fn run<J: Journal, M: Model>(
    input: &FrozenInput,
    config: &Config,
    journal: &J,
    model: &M,
    cancel: &CancellationToken,
) -> Result<AnalysisResult, AgentError> {
    run_seeded(input, config, journal, model, cancel).await
}

async fn run_seeded<J: Journal, M: Model>(
    input: &FrozenInput,
    config: &Config,
    journal: &J,
    model: &M,
    cancel: &CancellationToken,
) -> Result<AnalysisResult, AgentError> {
    crate::outline::frozen::validate_frozen_input_contract(input).map_err(invalid)?;
    tools::validate_input(input).map_err(invalid)?;
    config.validate()?;
    if !config.limits.vision_enabled
        && input
            .source_units
            .iter()
            .any(|source| source.locator["vision_required"] == true)
    {
        return Err(error(
            "AGENT_VISION_NOT_CONFIGURED",
            "this frozen tender requires original-image evidence; configure the same authoring provider with an image-capable model and vision_enabled=true before starting the run",
        ));
    }
    let started = Instant::now();
    let input_sha256 = digest(input).map_err(invalid)?;
    let config_sha256 = digest(config).map_err(invalid)?;
    let mut state = journal.load().await?.unwrap_or(Checkpoint {
        journal: Default::default(),
        input_sha256: input_sha256.clone(),
        config_sha256: config_sha256.clone(),
        turn: 0,
        tool_calls: 0,
        read_bytes: 0,
        review_rounds: 0,
        role: Role::Main,
        analysis: Analysis::default(),
        review: None,
        pending_coverage: None,
        transcript: vec![],
        main_progress: Progress::default(),
        main_work: None,
        done: false,
        source_views: BTreeMap::new(),
        draft_stage: Default::default(),
        draft_outline_gaps: None,
        draft_outline_stalls: 0,
        draft_outline_window: 0,
        outline_config_sha256: None,
        outline_run: Default::default(),
    });
    if state.input_sha256 != input_sha256 || state.config_sha256 != config_sha256 {
        return Err(error(
            "FROZEN_INPUT_DIGEST_MISMATCH",
            "checkpoint input or runtime changed; old checkpoints cannot resume, reparse and create a new run",
        ));
    }
    if state.role != Role::Main {
        return Err(error(
            "FROZEN_INPUT_DIGEST_MISMATCH",
            "retired reviewer checkpoint; start the current Discover/Organize/Check runtime",
        ));
    }
    state.journal.validate(
        state.turn,
        if state.role == Role::Main {
            "main"
        } else {
            "reviewer"
        },
    )?;
    if state.done && state.journal.pending.is_some() {
        return Err(error(
            "FROZEN_INPUT_DIGEST_MISMATCH",
            "completed analysis has a pending turn",
        ));
    }
    state.outline_config_sha256 = Some(config.tools_sha256.clone());
    if state.draft_stage == crate::analysis::draft::DraftStage::None {
        state.draft_stage = crate::analysis::draft::DraftStage::Outline;
    }
    let publication_ready = outline_publication_ready(input, &state, &input_sha256);
    let driven = if publication_ready {
        Ok(())
    } else {
        let mut driver = RunDriver {
            input,
            config,
            state: &mut state,
            journal,
            model,
            cancel,
        };
        drive(&mut driver, cancel).await
    };
    driven?;
    if !outline_publication_ready(input, &state, &input_sha256) {
        return Err(invalid(
            "outline is not ready to publish; checkpoint retained",
        ));
    }
    let stage = state.draft_stage;
    let turns = state.turn;
    let result = finalize_run(input, config, journal, &mut state, input_sha256).await;
    if let Ok(published) = result.as_ref() {
        tracing::info!(
            event = "draft_compile_ready",
            stage = ?stage,
            turns,
            elapsed_secs = started.elapsed().as_secs(),
            chapters = published.analysis.draft_plan.len(),
            filled = published
                .analysis
                .draft_plan
                .iter()
                .filter(|item| {
                    item.status == crate::analysis::draft::DraftStatus::Filled
                })
                .count(),
        );
    }
    result
}

fn published_outline(state: &Checkpoint) -> Option<crate::outline::tools::Draft> {
    state
        .outline_run
        .tool_draft
        .finished
        .then(|| state.outline_run.tool_draft.clone())
}

fn outline_publication_ready(input: &FrozenInput, state: &Checkpoint, input_sha256: &str) -> bool {
    crate::outline::project_draft(input, input_sha256, &state.outline_run.tool_draft).is_ok()
}

async fn finalize_run<J: Journal>(
    input: &FrozenInput,
    _config: &Config,
    journal: &J,
    state: &mut Checkpoint,
    input_sha256: String,
) -> Result<AnalysisResult, AgentError> {
    // Running out of turns never retires a chapter. Pending means "still waiting
    // for a body", and it compiles to an empty heading the user can write into;
    // Omitted drops the heading entirely, which would delete a chapter the user
    // has in front of them in Word.
    if !state.outline_run.tool_draft.finished
        && !crate::analysis::draft::plan_ready(&state.analysis.draft_plan)
    {
        // A zero-node outline cannot compile a chapter document without
        // inventing a chapter, so this stays an explicit failure rather than a
        // job that succeeds with no editable artifact.
        return Err(invalid(
            "draft outline has no chapter; tender parsing produced no bid composition clause",
        ));
    }
    if state.journal.pending.is_some() {
        return Err(invalid("cannot finalize an uncommitted model turn"));
    }
    let publication = state
        .outline_run
        .tool_draft
        .finished
        .then(|| crate::outline::project_draft(input, &input_sha256, &state.outline_run.tool_draft))
        .transpose()
        .map_err(invalid)?;
    state.draft_stage = crate::analysis::draft::DraftStage::Published;
    state.done = true;
    let review = Review {
        analysis_sha256: digest(&state.analysis).map_err(invalid)?,
        coverage: state.analysis.coverage.clone(),
        findings: vec![],
        draft: true,
        ..Default::default()
    };
    state.review = Some(review.clone());
    let mut result = AnalysisResult {
        schema_version: 2,
        frozen_input_sha256: input_sha256,
        analysis: state.analysis.clone(),
        review,
        quality: "needs_review".into(),
        source_views: state.source_views.clone(),
        usage: state.journal.usage.clone(),
        outline: published_outline(state),
    };
    if let Some(projected) = &publication {
        state.journal.publication_receipt = Some(crate::agent_runtime::PublicationReceipt {
            artifact_sha256: crate::outline::canonical_sha256(&projected.artifact)
                .map_err(invalid)?,
            bindings: serde_json::to_value(&projected.bindings).map_err(invalid)?,
        });
    }
    state.journal.finish()?;
    journal.save(state, &state.progress(input)).await?;
    if let Some(projected) = &publication {
        journal
            .publish_outline(&projected.artifact, &projected.bindings)
            .await?;
    }
    result.analysis = state.analysis.clone();
    Ok(result)
}

/// Credit only pixels present in the exact saved request consumed by the
/// complete provider response. A cached image or successful tool result alone
/// never establishes visual evidence, and each Check phase has its own scope.
fn validate_frozen_view(
    input: &FrozenInput,
    config: &Config,
    view: &views::SourceView,
) -> Result<(), String> {
    view.validate(
        &view.identity.source_id,
        config.limits.max_source_view_edge,
        config.limits.max_source_view_bytes,
    )?;
    let source = input
        .source_units
        .iter()
        .find(|source| {
            source.source_unit_revision_id == view.identity.source_id
                && source.locator["image_available"] == true
        })
        .ok_or("view has no frozen original source")?;
    let original = source.locator["image_ref"]
        .as_str()
        .and_then(|value| value.strip_prefix("objects/"));
    if original != Some(view.identity.original_sha256.as_str()) {
        return Err("delivered image original hash differs from frozen source".into());
    }
    Ok(())
}

fn confirm_outline_visual_delivery(
    input: &FrozenInput,
    config: &Config,
    state: &mut Checkpoint,
    body: &Value,
) -> Result<(), String> {
    let urls = body["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|message| message["content"].as_array())
        .flatten()
        .filter(|part| part["type"] == "image_url")
        .filter_map(|part| part["image_url"]["url"].as_str())
        .collect::<BTreeSet<_>>();
    if urls.is_empty() {
        return Ok(());
    }
    if !config.limits.vision_enabled {
        return Err("image payload in a non-vision frozen runtime".into());
    }
    let duty = crate::outline::agent::current(input, state);
    let mut delivered = Vec::new();
    for view in state.source_views.values() {
        let url = format!("data:image/jpeg;base64,{}", view.jpeg_base64);
        if !urls.contains(url.as_str()) {
            continue;
        }
        view.validate(
            &view.identity.source_id,
            config.limits.max_source_view_edge,
            config.limits.max_source_view_bytes,
        )?;
        // Only frozen native-image carriers participate in the new outline
        // image-evidence protocol; legacy page views retain their own scope.
        if let Some(source) = input.source_units.iter().find(|source| {
            source.source_unit_revision_id == view.identity.source_id
                && source.locator["image_available"] == true
        }) {
            let original = source.locator["image_ref"]
                .as_str()
                .and_then(|value| value.strip_prefix("objects/"));
            if original != Some(view.identity.original_sha256.as_str()) {
                return Err(
                    "delivered image original hash differs from the frozen source object".into(),
                );
            }
            delivered.push((
                view.identity.source_id.clone(),
                view.identity.image_sha256.clone(),
            ));
        }
    }
    for (source_id, image_sha256) in delivered {
        if let Some(work) = state.outline_run.reading_packs.as_mut() {
            work.confirm_visual_delivery(input, &source_id, &image_sha256)?;
        }
        crate::outline::agent::note_visual_delivery(input, state, &source_id, duty)?;
    }
    Ok(())
}

pub(super) async fn execute_turn<J: Journal>(
    input: &FrozenInput,
    config: &Config,
    state: &mut Checkpoint,
    journal: &J,
    response: ChatTurn,
    suppressed: BTreeMap<String, String>,
    cancel: &CancellationToken,
) -> Result<Vec<Value>, AgentError> {
    let mut staged = state.clone();
    let result = execute_turn_staged(
        input,
        config,
        &mut staged,
        journal,
        response,
        suppressed,
        cancel,
    )
    .await;
    if result.is_ok() {
        *state = staged;
    }
    result
}

async fn execute_turn_staged<J: Journal>(
    input: &FrozenInput,
    config: &Config,
    state: &mut Checkpoint,
    journal: &J,
    response: ChatTurn,
    suppressed: BTreeMap<String, String>,
    cancel: &CancellationToken,
) -> Result<Vec<Value>, AgentError> {
    if state.journal.response() != Some(&response) {
        return Err(invalid(
            "execution requires the matching durably received response",
        ));
    }
    let body = serde_json::from_slice(state.journal.body()?).map_err(invalid)?;
    let estimated_input_tokens = context::estimate_input_tokens(&body, &config.limits)?;
    let actual_input_tokens = response.usage.as_ref().and_then(|u| u.prompt_tokens);
    crate::outline::read_receipts::confirm(state, &body).map_err(invalid)?;
    confirm_outline_visual_delivery(input, config, state, &body).map_err(invalid)?;
    tracing::info!(event="analysis_token_usage",turn=state.turn,role=?state.role,
        estimated_input_tokens, actual_input_tokens,
        actual_output_tokens=response.usage.as_ref().and_then(|u|u.completion_tokens),
        cached_tokens=response.usage.as_ref().and_then(|u|u.cached_tokens),
        reasoning_tokens=response.usage.as_ref().and_then(|u|u.reasoning_tokens),
        estimate_exceeded=actual_input_tokens.map(|actual| actual > estimated_input_tokens as u64));
    if let Some(delivered) = state.pending_coverage.take() {
        state.replace_coverage(delivered);
    }
    let delivered_discovery = (state.draft_stage == draft::DraftStage::Outline
        && state.analysis.outline.phase == super::outline_flow::Phase::Discover)
        .then(|| context::visible_work_evidence(state, body["messages"].as_array().unwrap()));
    let outline_phase_before = state.analysis.outline.phase;
    let role = state.role.clone();
    state.transcript.push(json!({"role":"assistant","content":if response.content.is_empty(){Value::Null}else{json!(response.content)},
        "tool_calls":response.tool_calls.iter().map(|c|json!({"id":c.id,"type":"function","function":{"name":c.name,"arguments":c.arguments}})).collect::<Vec<_>>()}));
    let transition_requested = response
        .tool_calls
        .iter()
        .any(|c| matches!(c.name.as_str(), "request_review"));
    let batch_len = response.tool_calls.len();
    let mut pending_views = Vec::new();
    let mut tool_results = Vec::new();
    let mut local_completion = None;
    let mut batch_failed = false;
    let turn_duty = crate::outline::agent::current(input, state);
    // A submit-only Discover batch retires the source packs occupying the
    // current window. Probe the next request after those in-memory writes, not
    // before them while both the old sources and their response coexist.
    // execute_turn owns a full staged checkpoint; a failed final probe commits
    // neither the writes nor SDK/domain progress. No external write is skipped.
    let pack_commit_batch = turn_duty == crate::outline::agent::Duty::Discover
        && !response.tool_calls.is_empty()
        && response
            .tool_calls
            .iter()
            .all(|call| call.name == "submit_pack")
        && suppressed.is_empty();
    if !pack_commit_batch {
        fit_batch(input, config, state, &response.tool_calls, &pending_views).await?;
    }
    for (call_index, call) in response.tool_calls.iter().enumerate() {
        let tool_started = Instant::now();
        state.tool_calls += 1;
        let readonly = crate::outline::agent::registry()
            .iter()
            .find(|spec| spec.name == call.name)
            .is_some_and(|spec| !spec.mutating);
        let write_before = (!readonly).then(|| state.clone());
        let draft_before_read = readonly.then(|| state.outline_run.tool_draft.clone());
        let pending_before = state.pending_coverage.clone();
        let views_before = pending_views.len();
        let read_bytes_before = state.read_bytes;
        let args: Result<Value, String> = serde_json::from_str::<Value>(&call.arguments)
            .map_err(|e| e.to_string())
            .and_then(|args| {
                if crate::outline::agent::handles(&call.name) {
                    state
                        .outline_run
                        .tool_draft
                        .model_wire
                        .decode(args, false)
                        .and_then(|args| {
                            crate::outline::source_wire::resolve(state, &call.name, args)
                        })
                } else {
                    Ok(args)
                }
            });
        // Read tools may accumulate their own receipts, but subsequent
        // writes in this batch see only the previously delivered evidence.
        let prior_coverage = matches!(
            call.name.as_str(),
            "collection_index"
                | "read_source"
                | "read_form"
                | "read_form_cell"
                | "read_outline"
                | "read_outline_fragment"
                | "read_review_task"
                | "read_source_view"
                | "inspect_analysis"
                | "inspect_review"
        )
        .then(|| {
            let pending = state
                .pending_coverage
                .take()
                .unwrap_or_else(|| state.coverage().clone());
            state.replace_coverage(pending)
        });
        let scope_check = args
            .as_ref()
            .map_err(|e| e.to_string())
            .and_then(|args| context::check_read_scope(input, state, &call.name, args));
        let result = if suppressed.contains_key(&call.id) {
            Err("tool unavailable in this role".into())
        } else if let Err(message) = scope_check {
            Err(message)
        } else if transition_requested && batch_len != 1 {
            Err("review transition must be the only tool call in its turn".into())
        } else if batch_failed
            && call.name == "set_work_note"
            && args.as_ref().is_ok_and(|args| args["status"] == "complete")
        {
            Err("an earlier tool failed in this batch; inspect its feedback and repair or account for the failed operation before completing the scope in a later turn".into())
        } else if call.name == "read_source_view"
            && crate::outline::agent::deny(turn_duty, &call.name, false).is_some()
        {
            Err("original-image read is unavailable in the advertised duty".into())
        } else if call.name == "read_source_view" {
            match args {
                Ok(args) => {
                    match read_source_view(input, config, state, journal, &args, cancel).await {
                        Ok(value) => {
                            pending_views
                                .push(value["view_id"].as_str().expect("view identity").to_owned());
                            Ok(value)
                        }
                        Err(e)
                            if e.disposition == crate::agent_error::RetryDisposition::Obsolete
                                || e.code == "INTERNAL" =>
                        {
                            return Err(e);
                        }
                        Err(e) => Err(e.message),
                    }
                }
                Err(e) => Err(e.to_string()),
            }
        } else if matches!(call.name.as_str(), "read_requirements" | "read_outline") {
            match args {
                Ok(args) => {
                    read_projection_in_context(
                        input,
                        config,
                        state,
                        &args,
                        &response.tool_calls[call_index..],
                        &pending_views,
                        turn_duty,
                    )
                    .await
                }
                Err(e) => Err(e.to_string()),
            }
        } else if matches!(call.name.as_str(), "read_evidence" | "read_claim_evidence") {
            match args {
                Ok(args) => {
                    read_evidence_in_context(
                        input,
                        config,
                        state,
                        &args,
                        &response.tool_calls[call_index..],
                        &pending_views,
                        turn_duty,
                    )
                    .await
                }
                Err(e) => Err(e.to_string()),
            }
        } else {
            args.map_err(|e| e.to_string())
                .and_then(|args| apply_in_batch(input, config, state, &call.name, &args, turn_duty))
        };
        if let Some(prior) = prior_coverage {
            state.pending_coverage = Some(state.replace_coverage(prior));
        }
        let out = match result {
            Ok(value) => {
                let value = if let Some(delivered) = &delivered_discovery {
                    context::visible_read_receipt(state, delivered, &call.name, value)
                } else {
                    value
                };
                tool_result_envelope(value)
            }
            Err(message) => {
                if matches!(
                    call.name.as_str(),
                    "submit_outline_scan" | "put_outline_items"
                ) {
                    match serde_json::from_str::<Value>(&message) {
                        Ok(mut details) if details["committed"] == false => {
                            details["call_id"] = json!(call.id);
                            json!({"ok":false,"error":if call.name == "submit_outline_scan" { "SCAN_BATCH_INVALID" } else { "CHAPTER_BATCH_INVALID" },"details":details})
                        }
                        _ => json!({"ok":false,"error":message}),
                    }
                } else {
                    json!({"ok":false,"error":message})
                }
            }
        };
        let mut succeeded = out["ok"] == true;
        let content = match suppressed.get(&call.id) {
            Some(content) => content.clone(),
            None => serde_json::to_string(&out).map_err(invalid)?,
        };
        state.read_bytes = state
            .read_bytes
            .checked_add(content.len())
            .ok_or_else(|| invalid("read budget overflow"))?;
        state
            .transcript
            .push(json!({"role":"tool","tool_call_id":call.id,"content":content}));
        if !pack_commit_batch && state.role == role && !state.done {
            let fits = fit_batch(
                input,
                config,
                state,
                &response.tool_calls[call_index + 1..],
                &pending_views,
            )
            .await;
            if let Err(error) = fits {
                if error.code != "AGENT_TURN_BUDGET_EXCEEDED" {
                    return Err(error);
                }
                if let Some(before) = write_before {
                    // Admission is atomic for each write. Earlier fitted tools
                    // remain in this batch; a rejected write earns no progress.
                    *state = before;
                    state.transcript.push(deferred_message(&call.id));
                } else {
                    // Keep immutable pixels cached, but not their new receipts.
                    state.pending_coverage = pending_before;
                    if let Some(before) = &draft_before_read {
                        state.outline_run.tool_draft = before.clone();
                    }
                }
                succeeded = false;
                pending_views.truncate(views_before);
                let replacement = deferred_message(&call.id);
                state.read_bytes = read_bytes_before
                    .checked_add(replacement["content"].as_str().expect("tool content").len())
                    .ok_or_else(|| invalid("read budget overflow"))?;
                *state.transcript.last_mut().expect("current tool result") = replacement;
                fit_batch(
                    input,
                    config,
                    state,
                    &response.tool_calls[call_index + 1..],
                    &pending_views,
                )
                .await?;
            }
        }
        if readonly
            && succeeded
            && let Some(before) = &draft_before_read
        {
            crate::outline::read_receipts::queue(
                state,
                before,
                &call.id,
                turn_duty == crate::outline::agent::Duty::Check,
            );
        }
        batch_failed |= !succeeded;
        if succeeded
            && let Some(completion) =
                context::focused_completion(state, &call.name, &out["result"]).map_err(invalid)?
        {
            local_completion = Some(completion);
        }
        tool_results.push(
            state
                .transcript
                .last()
                .expect("current tool result")
                .clone(),
        );
        tracing::info!(event="analysis_tool_completed",turn=state.turn,role=?state.role,
            tool=call.name, success=succeeded, elapsed_ms=tool_started.elapsed().as_millis() as u64);
    }
    if pack_commit_batch && state.role == role && !state.done {
        // Collect the real tool receipts before successful completed-history
        // compaction is allowed to remove their transcript group.
        fit_batch(input, config, state, &[], &pending_views).await?;
    }
    if !pending_views.is_empty() {
        state
            .transcript
            .push(json!({"role":"user","source_view_refs":pending_views}));
    }
    context::observe_progress(state, &role, local_completion, &config.limits).map_err(invalid)?;
    // 停止只在填章回路里问一次，且只在章界生效：本章仍按预算写完，之后不再派新章。
    let stop = false;
    crate::analysis::draft::after_batch(input, state, batch_failed, stop).map_err(invalid)?;
    if turn_duty == crate::outline::agent::Duty::Check {
        crate::outline::agent::finish_check_repair_batch(input, state).map_err(invalid)?;
    }
    state.turn += 1;
    if state.role != role {
        state.transcript.clear();
    } else if outline_phase_before != state.analysis.outline.phase
        && state.analysis.outline.phase == super::outline_flow::Phase::Check
    {
        crate::outline::read_receipts::handoff_history(state);
    }
    Ok(tool_results)
}

const BATCH_OUTPUT_DEFERRED: &str = "Tool output does not fit the remaining batch context. Request a smaller range or fewer calls next turn. This result commits no business change or new reading/review coverage.";

/// Directory and actual target-body pages share transport admission with source pages.
async fn read_projection_in_context(
    input: &FrozenInput,
    config: &Config,
    state: &mut Checkpoint,
    args: &Value,
    remaining: &[knowledge::models::ChatToolCall],
    views: &[String],
    duty: crate::outline::agent::Duty,
) -> Result<Value, String> {
    let tool = &remaining[0].name;
    let projection_started = std::time::Instant::now();
    if let Some(error) = crate::outline::agent::deny(duty, tool, false) {
        return Err(error.into());
    }
    let identity = json!({"tool":tool,"input":crate::outline::evidence::input_digest(input)?,"revision":state.outline_run.reading_packs.as_ref().map(|w|w.revision),"epoch":state.outline_run.tool_draft.read_epoch,"duty":format!("{duty:?}")});
    let mut page = if let Some(cursor) = args["cursor"].as_str() {
        if args.as_object().is_none_or(|v| v.len() != 1) {
            return Err("continuation accepts only the host cursor".into());
        }
        let saved = state
            .outline_run
            .tool_draft
            .evidence_continuations
            .get(cursor)
            .ok_or("unknown or expired projection cursor")?;
        if saved["identity"] != identity {
            return Err("projection scope or revision changed".into());
        }
        saved["page"].clone()
    } else {
        crate::outline::agent::full_read_projection(input, state, tool, args)?
    };
    let projection_ms = projection_started.elapsed().as_millis() as u64;
    let mut fit_probes = 0usize;
    let fit_started = std::time::Instant::now();
    if page.get("items").is_none() && page.get("slot_id").is_none() {
        page = json!({"items":[page],"total":1});
    }
    let mut records = state.outline_run.tool_draft.metadata_records.clone();
    let mut original = page.clone();
    let mut fragments = page["items"][0]
        .get("field_fragments")
        .map(|v| {
            serde_json::from_value::<Vec<crate::outline::metadata_fragments::Field>>(v.clone())
                .map_err(|e| e.to_string())
        })
        .transpose()?;
    let mut selected = page["items"].as_array().map(Vec::len);
    let mut text_end = page["text"].as_str().map(str::len);
    loop {
        let mut tail = Value::Null;
        if let Some(count) = selected {
            let rows = original["items"].as_array().unwrap();
            page["items"] = json!(&rows[..count]);
            page["remaining"] = json!(rows.len() - count);
            if count < rows.len() {
                tail = original.clone();
                tail["items"] = json!(&rows[count..]);
            }
        }
        if let Some(end) = text_end {
            let text = original["text"].as_str().unwrap();
            let start = original["start_byte"].as_u64().unwrap_or(0) as usize;
            page["text"] = json!(&text[..end]);
            page["end_byte"] = json!(start + end);
            page["remaining"] = json!(text.len() - end);
            if end < text.len() {
                tail = original.clone();
                tail["text"] = json!(&text[end..]);
                tail["start_byte"] = json!(start + end);
            }
        }
        if let Some(fields) = &fragments {
            let key = original["items"][0]["record_key"]
                .as_str()
                .ok_or("fragment record identity missing")?;
            let record = records.get(key).ok_or("unknown metadata record")?;
            let all: Vec<crate::outline::metadata_fragments::Field> =
                serde_json::from_value(original["items"][0]["field_fragments"].clone())
                    .map_err(|e| e.to_string())?;
            let mut rest = all[fields.len()..].to_vec();
            if let (Some(end), Some(full_end)) = (
                fields.last().and_then(|f| f.end_byte),
                all.get(fields.len() - 1).and_then(|f| f.end_byte),
            ) && end < full_end
            {
                let mut last = all[fields.len() - 1].clone();
                let start = last.start_byte.unwrap();
                last.value = json!(&last.value.as_str().unwrap()[end - start..]);
                last.start_byte = Some(end);
                rest.insert(0, last);
            }
            page["items"] = json!([record.page(key, fields)]);
            if rest.is_empty() {
                let rows = &original["items"].as_array().unwrap()[1..];
                tail = if rows.is_empty() {
                    Value::Null
                } else {
                    let mut p = original.clone();
                    p["items"] = json!(rows);
                    p
                };
            } else {
                tail = original.clone();
                tail["items"][0] = record.page(key, &rest);
            }
        }
        let saved = json!({"identity":identity,"page":tail});
        let next = if tail.is_null() {
            None
        } else {
            Some(format!(
                "page_{}",
                crate::outline::canonical_sha256(&saved)?
            ))
        };
        page["next_cursor"] = json!(next);
        page["selection_complete"] = json!(tail.is_null());
        let mut candidate = state.clone();
        candidate.outline_run.tool_draft.metadata_records = records.clone();
        crate::outline::agent::queue_read_projection(&mut candidate, tool, &page, duty);
        if let Some(key) = &next {
            candidate
                .outline_run
                .tool_draft
                .evidence_continuations
                .insert(key.clone(), saved);
        }
        candidate.transcript.push(json!({"role":"tool","tool_call_id":remaining[0].id,"content":json!({"ok":true,"result":page}).to_string()}));
        fit_probes += 1;
        match fit_batch(input, config, &mut candidate, &remaining[1..], views).await {
            Ok(()) => {
                tracing::info!(
                    event = "analysis_read_projection",
                    tool,
                    projection_ms,
                    fit_probes,
                    fit_ms = fit_started.elapsed().as_millis() as u64,
                    selected_rows = selected,
                    selection_complete = tail.is_null()
                );
                if let Some(cursor) = args["cursor"].as_str() {
                    candidate
                        .outline_run
                        .tool_draft
                        .evidence_continuations
                        .remove(cursor);
                }
                state.outline_run.tool_draft = candidate.outline_run.tool_draft;
                return Ok(page);
            }
            Err(e) if e.code != "AGENT_TURN_BUDGET_EXCEEDED" => return Err(e.message),
            _ => {}
        }
        if let Some(count) = selected
            && count > 1
        {
            selected = Some(count.div_ceil(2));
            continue;
        }
        if let Some(end) = text_end {
            let text = &original["text"].as_str().unwrap()[..end];
            let boundaries = text
                .char_indices()
                .map(|(i, _)| i)
                .filter(|i| *i > 0)
                .collect::<Vec<_>>();
            if !boundaries.is_empty() {
                text_end = Some(boundaries[boundaries.len() / 2]);
                continue;
            }
        }
        if let Some(fields) = &mut fragments {
            if fields.len() > 1 {
                fields.truncate(fields.len().div_ceil(2));
                continue;
            }
            let (left, _) = crate::outline::metadata_fragments::split(&fields[0])?;
            fields[0] = left;
            continue;
        }
        if let Some(row) = original["items"]
            .as_array()
            .and_then(|rows| rows.first())
            .cloned()
        {
            let key = format!(
                "record_{}",
                crate::outline::canonical_sha256(&(&identity, &row))?
            );
            let record = crate::outline::metadata_fragments::Record::new(row);
            fragments = Some(record.fields.clone());
            original["items"][0] = record.page(&key, &record.fields);
            records.insert(key, record);
            selected = Some(1);
            continue;
        }
        return Err("remaining request token budget cannot fit one source character and its immutable identity".into());
    }
}

/// Host-owned source continuation. Admission uses the same complete next
/// request token budget as transport, not a separate source byte threshold.
async fn read_evidence_in_context(
    input: &FrozenInput,
    config: &Config,
    state: &mut Checkpoint,
    args: &Value,
    remaining: &[knowledge::models::ChatToolCall],
    views: &[String],
    duty: crate::outline::agent::Duty,
) -> Result<Value, String> {
    use crate::outline::evidence::{self, EvidenceRef};
    if let Some(error) = crate::outline::agent::deny(duty, &remaining[0].name, false) {
        return Err(error.into());
    }
    let tool = &remaining[0].name;
    let saved = args["cursor"]
        .as_str()
        .map(|cursor| {
            state
                .outline_run
                .tool_draft
                .evidence_continuations
                .get(cursor)
                .cloned()
                .ok_or("unknown or expired evidence cursor")
        })
        .transpose()?;
    let claim = if tool == "read_claim_evidence" {
        let id = saved
            .as_ref()
            .and_then(|v| v["requirement_id"].as_str())
            .or_else(|| args["requirement_id"].as_str())
            .ok_or("requirement_id is required")?;
        let record = state
            .outline_run
            .reading_packs
            .as_ref()
            .and_then(|w| w.requirement_records().get(id))
            .ok_or("unknown requirement")?;
        Some(crate::outline::claim_review::build(input, id, record)?)
    } else {
        None
    };
    let identity = json!({"tool":tool,"claim_version":claim.as_ref().map(|c|&c.version),"read_epoch":state.outline_run.tool_draft.read_epoch,"input":evidence::input_digest(input)?,"revision":state.outline_run.reading_packs.as_ref().map(|w|w.revision),"duty":format!("{duty:?}")});
    let refs: Vec<EvidenceRef> = if let Some(cursor) = args["cursor"].as_str() {
        if args.get("refs").is_some() {
            return Err("cursor cannot be combined with refs".into());
        }
        let saved = state
            .outline_run
            .tool_draft
            .evidence_continuations
            .get(cursor)
            .ok_or("unknown or expired evidence cursor")?;
        if saved["identity"] != identity {
            return Err("evidence cursor input, revision or duty changed".into());
        }
        serde_json::from_value(saved["refs"].clone()).map_err(|e| e.to_string())?
    } else if let Some(unit) = &claim {
        unit.evidence_units
            .iter()
            .flat_map(|u| u.excerpts.iter().map(|e| e.evidence.clone()))
            .collect()
    } else {
        serde_json::from_value(args["refs"].clone()).map_err(|e| e.to_string())?
    };
    if refs.is_empty() {
        return Err("evidence selection must not be empty".into());
    }
    evidence::resolve_evidence(input, &refs)?;
    let mut selected = refs.clone();
    loop {
        let tail = evidence::selection_tail(&refs, &selected);
        let continuation = json!({"identity":identity,"refs":tail,"requirement_id":claim.as_ref().map(|c|&c.requirement_id)});
        let next = if tail.is_empty() {
            None
        } else {
            Some(format!(
                "page_{}",
                crate::outline::canonical_sha256(&continuation)?
            ))
        };
        let mut candidate = state.clone();
        let mut page = crate::outline::agent::apply_for_duty(
            input,
            &mut candidate,
            "read_evidence",
            &json!({"refs":selected}),
            duty,
        )?;
        if let Some(unit) = &claim {
            let excerpts = page["excerpts"]
                .as_array()
                .ok_or("source projection missing")?;
            let mut items = Vec::new();
            for source in &unit.evidence_units {
                let partial = excerpts
                    .iter()
                    .filter(|excerpt| {
                        serde_json::from_value::<EvidenceRef>(excerpt["evidence"].clone())
                            .ok()
                            .is_some_and(|reference| {
                                source.excerpts.iter().any(|original| {
                                    evidence::covered_by_union(
                                        &reference,
                                        std::slice::from_ref(&original.evidence),
                                    )
                                })
                            })
                    })
                    .cloned()
                    .collect::<Vec<_>>();
                if !partial.is_empty() {
                    items.push(json!({"quote_handle":source.quote_handle,"role":source.role,"excerpts":partial,"unit_complete_in_this_page":source.excerpts.iter().all(|e|evidence::covered_by_union(&e.evidence,&selected))}));
                }
            }
            page = json!({"requirement_id":unit.requirement_id,"version":unit.version,"original_requirement":unit.original_requirement.description,"claims":unit.claims,"items":items,"total_units":unit.evidence_units.len(),"instruction":"A quote unit can span multiple host pages. Follow next_cursor until selection_complete, then compare complete units; fragments grant only their exact delivered intervals."});
        }
        page["next_cursor"] = json!(next);
        page["selection_complete"] = json!(tail.is_empty());
        if let Some(key) = &next {
            candidate
                .outline_run
                .tool_draft
                .evidence_continuations
                .insert(key.clone(), continuation);
        }
        candidate.transcript.push(json!({"role":"tool","tool_call_id":remaining[0].id,"content":json!({"ok":true,"result":page}).to_string()}));
        match fit_batch(input, config, &mut candidate, &remaining[1..], views).await {
            Ok(()) => {
                if let Some(cursor) = args["cursor"].as_str() {
                    candidate
                        .outline_run
                        .tool_draft
                        .evidence_continuations
                        .remove(cursor);
                }
                state.outline_run.tool_draft = candidate.outline_run.tool_draft;
                return Ok(page);
            }
            Err(e) if e.code != "AGENT_TURN_BUDGET_EXCEEDED" => return Err(e.message),
            _ => {}
        }
        evidence::shrink_selection(input, &mut selected)?;
    }
}

fn deferred_message(id: &str) -> Value {
    json!({"role":"tool","tool_call_id":id,"content":json!({"ok":false,"error":BATCH_OUTPUT_DEFERRED}).to_string()})
}

/// Reserve serialized space for every outstanding tool response, not only for
/// the current tool. This never sends, reserves a call, or confirms delivery.
async fn fit_batch(
    input: &FrozenInput,
    config: &Config,
    state: &mut Checkpoint,
    remaining: &[knowledge::models::ChatToolCall],
    views: &[String],
) -> Result<(), AgentError> {
    // Request preparation may compact completed history. Keep the probe
    // isolated so temporary responses/views cannot be evicted from underneath
    // the caller, and so a rejected batch cannot change its source receipts.
    // The actual next request repeats admission and commits any compaction.
    let mut sizing = state.clone();
    sizing
        .transcript
        .extend(remaining.iter().map(|c| deferred_message(&c.id)));
    if !views.is_empty() {
        sizing
            .transcript
            .push(json!({"role":"user","source_view_refs":views}));
    }
    // Size the next turn, including worst-case digits in the read counter.
    sizing.turn = sizing.turn.saturating_add(1);
    sizing.tool_calls = sizing.tool_calls.saturating_add(remaining.len());
    prepare_request(input, config, &mut sizing, false)
        .await
        .map(|_| ())
}

pub(super) async fn request(
    input: &FrozenInput,
    config: &Config,
    state: &mut Checkpoint,
) -> Result<Vec<u8>, AgentError> {
    prepare_request(input, config, state, true).await
}

pub(super) async fn prepare_request(
    input: &FrozenInput,
    config: &Config,
    state: &mut Checkpoint,
    defer_images: bool,
) -> Result<Vec<u8>, AgentError> {
    // Sizing (`fit_batch`, inspection) calls this with `defer_images` false.
    // It may claim and release packs to see whether a smaller brief fits, then
    // the snapshot is restored so those passes do not stick.
    let saved_packs = (!defer_images).then(|| state.outline_run.reading_packs.clone());
    let result = prepare_fitted_request(input, config, state, defer_images).await;
    if let Some(saved) = saved_packs {
        state.outline_run.reading_packs = saved;
    }
    result
}

/// Planning-only placeholders reserve pending original-image context. They are
/// never sent or entered into visual delivery receipts. Cached actual views use
/// their real identity; unrendered views reserve the same bounded identity shape.
fn reserve_pack_image_messages(
    input: &FrozenInput,
    config: &Config,
    state: &Checkpoint,
    sessions: &[Value],
    body: &mut Value,
) -> Result<(), String> {
    let ids = sessions
        .iter()
        .flat_map(|session| session["pack"]["atoms"].as_array().into_iter().flatten())
        .filter(|atom| {
            atom["carrier"]["kind"] == "image" && atom["carrier"]["vision_required"] == true
        })
        .filter_map(|atom| atom["carrier"]["evidence"]["image_id"].as_str())
        .collect::<BTreeSet<_>>();
    let existing = body["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|message| message["content"].as_array())
        .flatten()
        .filter_map(|part| part["image_url"]["url"].as_str())
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    for id in ids {
        let cached = state
            .source_views
            .values()
            .find(|view| view.identity.source_id == id);
        if cached.is_some_and(|view| {
            existing.contains(&format!("data:image/jpeg;base64,{}", view.jpeg_base64))
        }) {
            continue;
        }
        let message = if let Some(view) = cached {
            view.message()
        } else {
            let source = input
                .source_units
                .iter()
                .find(|source| source.source_unit_revision_id == id)
                .ok_or("image source missing while planning")?;
            let view = crate::analysis::views::SourceView {
                identity: crate::analysis::views::ViewIdentity {
                    source_id: id.into(),
                    original_sha256: source.locator["image_ref"]
                        .as_str()
                        .and_then(|value| value.strip_prefix("objects/"))
                        .ok_or("image object missing while planning")?
                        .into(),
                    image_sha256: "a1".repeat(32),
                    page_ordinal: source.locator["page_ordinal"].as_u64().unwrap_or(0) as u32,
                    width: config.limits.max_source_view_edge,
                    height: config.limits.max_source_view_edge,
                    renderer: format!(
                        "frozen-source-view-v2/full-frame/jpeg-q85/edge{}",
                        config.limits.max_source_view_edge
                    ),
                },
                jpeg_base64: String::new(),
            };
            view.message()
        };
        body["messages"]
            .as_array_mut()
            .ok_or("request messages missing")?
            .push(message);
    }
    Ok(())
}

/// The plan is built before its own progress counters exist. Reserve their
/// complete wire shape, including numeric growth during later discovery turns,
/// inside the same token budget. These placeholders are never transported.
fn reserve_discovery_progress(progress: &mut Value) {
    for key in [
        "outline_pack_total",
        "outline_pack_pending",
        "outline_pack_running",
        "outline_pack_failed",
        "outline_pack_committed",
    ] {
        progress[key] = json!(usize::MAX);
    }
    fn reserve_counters(value: &mut Value) {
        match value {
            Value::Number(_) => *value = json!(usize::MAX),
            Value::Object(fields) => fields.values_mut().for_each(reserve_counters),
            Value::Array(values) => values.iter_mut().for_each(reserve_counters),
            _ => {}
        }
    }
    reserve_counters(progress);
}

/// Keep complete errors in the checkpoint; show a progressively smaller repair
/// page only when the actual request needs space. Sources and claims are intact.
fn shrink_discovery_feedback(sessions: &mut [Value]) -> bool {
    let Some(session) = sessions
        .iter_mut()
        .filter(|session| {
            session["feedback"]["errors"]
                .as_array()
                .is_some_and(|errors| errors.len() > 1)
        })
        .max_by_key(|session| session["feedback"]["errors"].as_array().unwrap().len())
    else {
        return false;
    };
    let feedback = &mut session["feedback"];
    let errors = feedback["errors"].as_array_mut().unwrap();
    errors.truncate(errors.len().div_ceil(2));
    let shown = errors.len();
    feedback["truncated"] = json!(true);
    feedback["omitted_errors"] = json!(
        feedback["total"]
            .as_u64()
            .unwrap_or(shown as u64)
            .saturating_sub(shown as u64)
    );
    true
}

// Bounded non-durable schema/handle feedback plus its serialized tool framing.
// This is repair headroom in context tokens, never a source byte/pack limit.
pub(super) const DISCOVERY_FEEDBACK_RESERVE_TOKENS: usize = 1024;

async fn prepare_fitted_request(
    input: &FrozenInput,
    config: &Config,
    state: &mut Checkpoint,
    defer_images: bool,
) -> Result<Vec<u8>, AgentError> {
    context::annotate_delivered_source_lines(state, config.limits.context_wire_ceiling())
        .map_err(invalid)?;
    if state.execution().watch.needs_replan_context() {
        // Recovery must change the working context before another full-budget
        // inventory loop. Reuse the lossless history operations proactively;
        // the latest group, receipts and cumulative recovery state are intact.
        while context::compact_delivered_navigation(&mut state.transcript)
            || context::compact_recallable_candidate_details(
                state,
                config.limits.context_wire_ceiling(),
            )
            || context::evict_delivered_group(state, config.limits.context_wire_ceiling(), false)
        {
        }
    }
    let mut excluded_recall = std::collections::BTreeSet::new();
    let needs_discovery = matches!(
        state.draft_stage,
        draft::DraftStage::None | draft::DraftStage::Outline
    ) && state
        .outline_run
        .reading_packs
        .as_ref()
        .is_none_or(|work| !work.complete());
    let mut reading_sessions = if needs_discovery {
        if let Some(work) = state.outline_run.reading_packs.as_mut() {
            work.claim(crate::outline::discover::DEFAULT_PACK_CONCURRENCY);
            work.inflight_sessions(input)
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };
    if let Some(message) = state
        .outline_run
        .reading_packs
        .as_ref()
        .and_then(|work| work.planning_error())
    {
        return Err(error(
            "AGENT_PACK_PLAN_INVALID",
            format!("{message}; adjust the pack budget or reparse; checkpoint retained"),
        ));
    }

    let turn_duty = crate::outline::agent::current(input, state);
    let mut host_items = state
        .outline_run
        .reading_packs
        .as_ref()
        .map(|work| work.requirement_records().len())
        .unwrap_or(0);
    loop {
        let reviewer = false;
        let system = crate::outline::agent::system_prompt(turn_duty);
        let mut brief = json!({"project_id":input.project_id,"document_set_id":input.document_set_id,
            "source_count":input.source_units.len(),"form_count":input.structured_forms.len(),
            "document_count":input.documents.len(),"relation_count":input.document_relations.len(),"decision_count":input.decisions.len(),
        });
        if !reading_sessions.is_empty() {
            brief["reading_packs"] = json!(reading_sessions);
        }
        let mut messages = vec![
            json!({"role":"system","content":system}),
            json!({"role":"user","content":brief.to_string()}),
        ];
        let mut visible_views = std::collections::BTreeSet::new();
        for message in &state.transcript {
            if let Some(refs) = message["source_view_refs"].as_array() {
                for id in refs {
                    visible_views.insert(
                        id.as_str()
                            .ok_or_else(|| invalid("view reference invalid"))?,
                    );
                    let view = state
                        .source_views
                        .get(
                            id.as_str()
                                .ok_or_else(|| invalid("view reference invalid"))?,
                        )
                        .ok_or_else(|| invalid("checkpoint view bytes missing"))?;
                    messages.push(view.message());
                }
            } else {
                messages.push(message.clone());
            }
        }
        // The assigned original and explicitly focused visual evidence survive
        // history eviction. Explicit reviewer support reads still in bounded
        // history can retain pixels outside the work scope; evicted reads do
        // not pin every visited page. Only the current role's committed view
        // receipts qualify; the shared pixel cache grants no reading receipt.
        // These messages still count against total byte and token ceilings.
        if let Some(work) = state
            .work()
            .filter(|work| work.status == WorkStatus::Active)
        {
            let focused_views = context::focused_view_ids(state);
            let support_views = std::collections::BTreeSet::<String>::new();
            let assigned_pack = if reviewer {
                work.source_scope.clone()
            } else {
                Vec::new()
            };
            // One original page can cover several parsed text/grid sources.
            // Reuse one delivered image, preferring already visible pixels.
            for (id, identity) in &state.coverage().views {
                if ((work.source_scope.contains(&identity.source_id)
                    && (assigned_pack.contains(&identity.source_id) || focused_views.contains(id)))
                    || support_views.contains(id))
                    && !visible_views.contains(id.as_str())
                {
                    let view = state
                        .source_views
                        .get(id)
                        .filter(|v| &v.identity == identity)
                        .ok_or_else(|| invalid("focused source pixels missing or changed"))?;
                    messages.push(view.message());
                }
            }
        }
        let mut recalled =
            context::retained_candidate_message(state, config.limits.context_wire_ceiling())
                .map_err(invalid)?;
        context::trim_optional_candidate_recall(state, &mut recalled, &mut excluded_recall, 0)
            .map_err(invalid)?;
        if !recalled.is_null() {
            messages.push(recalled.clone());
        }
        let host = crate::outline::agent::host_packet(
            input,
            state,
            host_items,
            state.progress(input),
            context::request_work(state).map_err(invalid)?,
            None,
        );
        messages.push(json!({"role":"user","content":host.to_string()}));
        let bytes = crate::agent_runtime::chat::prepare(
            &config.provider,
            messages,
            crate::outline::agent::schemas_for(turn_duty),
        )
        .await?;
        let mut body: Value = serde_json::from_slice(&bytes).map_err(invalid)?;
        crate::outline::discover::compact_request(&mut body).map_err(invalid)?;
        let issued_keys =
            crate::outline::source_wire::request(state, &mut body).map_err(invalid)?;
        let canonical_body = body.clone();
        let issued_registry =
            crate::outline::model_wire::project(state, &mut body).map_err(invalid)?;
        let bytes = serde_json::to_vec(&body).map_err(invalid)?;
        if needs_discovery && state.outline_run.reading_packs.is_none() {
            // The initial reading plan is fitted to this complete real request
            // envelope, not to a second byte/character/cell threshold.
            let mut planning_host = host.clone();
            reserve_discovery_progress(&mut planning_host["progress"]);
            let planning_started = std::time::Instant::now();
            let candidate_fits = std::cell::Cell::new(0usize);
            let cache_before = crate::agent_runtime::chat::cache_counts();
            let fits = |sessions: &[Value]| -> Result<bool, String> {
                candidate_fits.set(candidate_fits.get() + 1);
                let mut candidate = canonical_body.clone();
                let mut candidate_brief = brief.clone();
                candidate_brief["reading_packs"] = json!(sessions);
                candidate["messages"][1]["content"] = json!(candidate_brief.to_string());
                candidate["messages"]
                    .as_array_mut()
                    .ok_or("request messages missing")?
                    .last_mut()
                    .ok_or("request host packet missing")?["content"] =
                    json!(planning_host.to_string());
                reserve_pack_image_messages(input, config, state, sessions, &mut candidate)?;
                crate::outline::discover::compact_request(&mut candidate)?;
                crate::outline::source_wire::request(state, &mut candidate)?;
                crate::outline::model_wire::project(state, &mut candidate)?;
                let tokens = context::estimate_input_tokens(&candidate, &config.limits)
                    .map_err(|error| error.to_string())?;
                Ok(tokens
                    .saturating_add(config.provider.output_token_reserve as usize)
                    .saturating_add(config.provider.output_token_reserve as usize)
                    .saturating_add(DISCOVERY_FEEDBACK_RESERVE_TOKENS)
                    <= config.limits.max_context_tokens)
            };
            let mut work = crate::outline::discover::DiscoverWork::plan_with_budget(input, &fits);
            let cache_after = crate::agent_runtime::chat::cache_counts();
            tracing::info!(
                event = "analysis_pack_planned",
                candidate_fits = candidate_fits.get(),
                cache_hits = cache_after.0.saturating_sub(cache_before.0),
                cache_misses = cache_after.1.saturating_sub(cache_before.1),
                elapsed_ms = planning_started.elapsed().as_millis() as u64
            );
            if let Some(message) = work.planning_error() {
                return Err(error("AGENT_PACK_PLAN_INVALID", message));
            }
            work.claim(crate::outline::discover::DEFAULT_PACK_CONCURRENCY);
            reading_sessions = work.inflight_sessions(input);
            state.outline_run.reading_packs = Some(work);
            continue;
        }
        // A running pack has one prior-response allowance inside the same
        // context budget. An existing latest tool group already consumes that
        // allowance (including identity errors without a durable pack receipt).
        // Failed packs use it for their projected repair feedback instead.
        let input_tokens = context::estimate_input_tokens(&body, &config.limits)?;
        let discovery_lifecycle_tokens = if reading_sessions
            .iter()
            .any(|session| session["status"] == "running")
        {
            let mut priorless = body.clone();
            let messages = priorless["messages"]
                .as_array_mut()
                .ok_or_else(|| invalid("SDK messages missing"))?;
            let consumed = if let Some(start) = messages
                .iter()
                .rposition(|message| message["role"] == "assistant")
            {
                messages.drain(start..messages.len() - 1);
                input_tokens
                    .saturating_sub(context::estimate_input_tokens(&priorless, &config.limits)?)
            } else {
                0
            };
            (config.provider.output_token_reserve as usize)
                .saturating_add(DISCOVERY_FEEDBACK_RESERVE_TOKENS)
                .saturating_sub(consumed)
        } else {
            0
        };
        let context_excess = input_tokens
            .saturating_add(config.provider.output_token_reserve as usize)
            .saturating_add(discovery_lifecycle_tokens)
            .saturating_sub(config.limits.max_context_tokens);
        #[cfg(test)]
        if let Ok(root) = std::env::var("KB_PRIVATE_BUDGET_DIAGNOSTIC_DIR") {
            let mut parts = Vec::new();
            if let Some(messages) = body["messages"].as_array() {
                for (index, message) in messages.iter().enumerate() {
                    let mut without = body.clone();
                    without["messages"].as_array_mut().unwrap().remove(index);
                    parts.push(json!({"index":index,"role":message["role"],"bytes":serde_json::to_vec(message).unwrap().len(),"marginal_tokens":input_tokens.saturating_sub(context::estimate_input_tokens(&without,&config.limits)?)}));
                }
            }
            let mut without_tools = body.clone();
            without_tools.as_object_mut().unwrap().remove("tools");
            let entry = json!({"input_tokens":input_tokens,"serialized_bytes":bytes.len(),"output_reserve":config.provider.output_token_reserve,"lifecycle_reserve":discovery_lifecycle_tokens,"feedback_reserve":DISCOVERY_FEEDBACK_RESERVE_TOKENS,"context_limit":config.limits.max_context_tokens,"excess":context_excess,"tools_marginal_tokens":input_tokens.saturating_sub(context::estimate_input_tokens(&without_tools,&config.limits)?),"messages":parts});
            use std::io::Write;
            let path = std::path::Path::new(&root).join("budget-preparation-iterations.jsonl");
            writeln!(
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .unwrap(),
                "{}",
                entry
            )
            .unwrap();
        }
        let fits_total = context_excess == 0;
        if !fits_total && !defer_images && reading_sessions.is_empty() {
            // A paging probe must not repeatedly evict older history when the
            // latest, unconsumed read group alone already exceeds the window.
            // This is only an early rejection: every accepted page still goes
            // through full request preparation and final transport admission.
            let messages = body["messages"]
                .as_array()
                .ok_or_else(|| invalid("messages missing"))?;
            if let Some(start) = messages.iter().rposition(|m| m["role"] == "assistant") {
                let is_read = messages[start]["tool_calls"]
                    .as_array()
                    .is_some_and(|calls| {
                        calls.iter().any(|call| {
                            matches!(
                                call["function"]["name"].as_str(),
                                Some(
                                    "read_requirements"
                                        | "read_outline"
                                        | "read_evidence"
                                        | "read_claim_evidence"
                                )
                            )
                        })
                    });
                if is_read && start + 1 < messages.len() {
                    let mut required = body.clone();
                    let mut kept = messages[..start]
                        .iter()
                        .filter(|m| m["role"] == "system")
                        .cloned()
                        .collect::<Vec<_>>();
                    kept.extend_from_slice(&messages[start..messages.len() - 1]);
                    required["messages"] = json!(kept);
                    let tokens = context::estimate_input_tokens(&required, &config.limits)?;
                    if tokens.saturating_add(config.provider.output_token_reserve as usize)
                        > config.limits.max_context_tokens
                    {
                        return Err(error(
                            "AGENT_TURN_BUDGET_EXCEEDED",
                            "unconsumed read group exceeds context; continue with a smaller source page",
                        ));
                    }
                }
            }
        }
        if fits_total {
            state.outline_run.tool_draft.source_keys = issued_keys;
            state.outline_run.tool_draft.model_wire = issued_registry;
            crate::outline::read_receipts::seal(state, &body).map_err(invalid)?;
            return Ok(bytes);
        }
        // Host requirement previews are optional. Only the complete request's
        // measured token fit can shrink them; tools retain the full records and
        // fragment even a single oversized record through opaque continuations.
        let dropped_preview = host_items > 0;
        host_items = 0;
        // Completed pack acknowledgements are reconstructible from durable
        // receipts. Release them before shrinking any still-unread source.
        // These operations preserve the latest unconsumed tool group and its
        // evidence. Finish the same lossless cleanup before rebuilding the
        // full request, instead of reserializing it after every old group.
        let mut compacted = false;
        while context::evict_completed_discovery_history(
            state,
            config.limits.context_wire_ceiling(),
        ) || context::compact_delivered_navigation(&mut state.transcript)
            || context::compact_recallable_candidate_details(
                state,
                config.limits.context_wire_ceiling(),
            )
            || context::evict_delivered_group(state, config.limits.context_wire_ceiling(), false)
            || shrink_discovery_feedback(&mut reading_sessions)
        {
            compacted = true;
        }
        if dropped_preview || compacted {
            continue;
        } else if !fits_total
            && context::trim_optional_candidate_recall(
                state,
                &mut recalled,
                &mut excluded_recall,
                context_excess,
            )
            .map_err(invalid)?
        {
            // Optional recall uses remaining space. Preserve focused candidates
            // and fresh results; try the full cache again on the next request.
            continue;
        } else if context::evict_delivered_group(state, config.limits.context_wire_ceiling(), true)
        {
            continue;
        } else if defer_images
            && let Some(id) = views::defer_last_image(&mut state.transcript).map_err(invalid)?
        {
            // Deferring a new receipt cannot erase a view already delivered
            // in an earlier turn, and must never affect the other role.
            if !state.coverage().views.contains_key(&id)
                && let Some(pending) = &mut state.pending_coverage
            {
                pending.views.remove(&id);
            }
        } else if state
            .outline_run
            .reading_packs
            .as_mut()
            .is_some_and(|work| work.release_highest_inflight())
        {
            // Reading packs live in the brief, which the shrink steps above do
            // not touch. Drop the highest-order in-flight pack and rebuild.
            reading_sessions = state
                .outline_run
                .reading_packs
                .as_ref()
                .map(|work| work.inflight_sessions(input))
                .unwrap_or_default();
        } else {
            return Err(error(
                "AGENT_TURN_BUDGET_EXCEEDED",
                "context budget cannot hold current tool group",
            ));
        }
    }
}

// Read receipts belong to the main role and exact finding contents. They use
// the existing durable seen set, but do not reset a progress watch or attest a
// repair. Only tool results actually present in a completed request count.

#[cfg(test)]
pub(super) fn apply(
    input: &FrozenInput,
    config: &Config,
    state: &mut Checkpoint,
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    apply_in_batch(
        input,
        config,
        state,
        name,
        args,
        crate::outline::agent::current(input, state),
    )
}

pub(super) fn apply_in_batch(
    input: &FrozenInput,
    config: &Config,
    state: &mut Checkpoint,
    name: &str,
    args: &Value,
    turn_duty: crate::outline::agent::Duty,
) -> Result<Value, String> {
    let args = if name == "submit_outline_scan" {
        args.clone()
    } else {
        evidence_refs::expand(input, args)?
    };
    let result = apply_inner(input, config, state, name, &args, turn_duty)?;
    context::synchronize_outcomes(state);
    Ok(result)
}

fn apply_inner(
    input: &FrozenInput,
    _config: &Config,
    state: &mut Checkpoint,
    name: &str,
    args: &Value,
    turn_duty: crate::outline::agent::Duty,
) -> Result<Value, String> {
    if state.execution().watch.recovery == Recovery::Blocked
        && !crate::outline::agent::handles(name)
    {
        return Err("local execution is blocked; checkpoint retained".into());
    }
    let unmapped =
        !crate::outline::tools::unmapped_forms(input, &state.outline_run.tool_draft).is_empty();
    if let Some(reason) = crate::outline::agent::deny(turn_duty, name, unmapped) {
        return Err(reason.into());
    }
    if crate::outline::agent::handles(name) {
        return crate::outline::agent::apply_for_duty(input, state, name, args, turn_duty);
    }
    Err("unknown or role-forbidden tool".into())
}

#[cfg(test)]
mod visual_transport_tests;

/// Local-only admission probe. This prepares the genuine SDK request without a
/// network transport and labels its explicit reference tokenizer configuration.
#[cfg(test)]
pub(crate) async fn local_token_plan(
    input: &FrozenInput,
) -> Result<(crate::outline::discover::DiscoverWork, Value), AgentError> {
    let reference = super::tests::config();
    let mut provider = reference.provider;
    provider.model_id = "gpt-4.1".into();
    let mut limits = reference.limits;
    limits.max_context_tokens = 131_072;
    limits.token_safety_margin = 4096;
    limits.image_token_reserve = 16_384;
    limits.tokenizer = crate::agent_runtime::TokenizerProfile {
        model_id: provider.model_id.clone(),
        encoding: crate::agent_runtime::TokenEncoding::O200kBase,
        calibration: None,
    };
    let config = Config::with_provider(provider, limits)?;
    let mut state = retirement::checkpoint(input);
    state.turn = 0;
    state.config_sha256 = digest(&config).map_err(invalid)?;
    let body = request(input, &config, &mut state).await?;
    let value: Value = serde_json::from_slice(&body).map_err(invalid)?;
    let accounting = crate::agent_runtime::chat::estimate_request_tokens_with_reserve(
        &value,
        &config.limits.tokenizer,
        config.limits.image_token_reserve,
        config.limits.token_safety_margin,
        config.provider.output_token_reserve as usize,
    )?;
    let first_brief: Value = serde_json::from_str(
        value["messages"][1]["content"]
            .as_str()
            .ok_or_else(|| invalid("brief missing"))?,
    )
    .map_err(invalid)?;
    let summary = json!({
        "reference_model":"gpt-4.1", "tokenizer":"o200k_base", "provider_called":false,
        "is_user_deployed_model":false, "max_context_tokens":config.limits.max_context_tokens,
        "first_request_bytes":body.len(), "first_request_packs":first_brief["reading_packs"].as_array().map_or(0,Vec::len),
        "token_accounting":accounting,
    });
    Ok((
        state
            .outline_run
            .reading_packs
            .ok_or_else(|| invalid("reading plan missing"))?,
        summary,
    ))
}

#[cfg(test)]
mod token_transport_validation;

#[cfg(test)]
mod provider_defaults_tests {
    use super::*;
    #[test]
    #[ignore = "isolated config-only entry probe; explicit synthetic loopback provider; never sends a request"]
    fn minimal_environment_entry_freezes_selected_model_and_defaults() {
        if std::env::var_os("KB_TEST_EXPECT_UNKNOWN_PROFILE").is_some() {
            assert!(
                Config::from_environment()
                    .unwrap_err()
                    .message
                    .contains("requires a measured")
            );
            return;
        }
        let config = Config::from_environment().unwrap();
        assert_eq!(config.limits.max_context_tokens, 131072);
        assert_eq!(config.limits.tokenizer.model_id, config.provider.model_id);
        let frozen = serde_json::to_value(&config).unwrap();
        for field in [
            "max_tool_result_bytes",
            "run_budget",
            "max_turns",
            "max_tool_calls",
            "max_read_bytes",
        ] {
            assert!(frozen["limits"].get(field).is_none());
        }
    }
    #[test]
    fn context_override_may_lower_but_cannot_exceed_profile_ceiling() {
        let mut provider = super::super::tests::config().provider;
        provider.model_id = "gpt-4.1".into();
        for allowed in [32768, Limits::PROFILE_CONTEXT_CEILING] {
            let limits = Config::configured_limits(
                &provider,
                None,
                Some(&json!({"max_context_tokens":allowed}).to_string()),
            )
            .unwrap();
            assert_eq!(
                Config::with_provider(provider.clone(), limits)
                    .unwrap()
                    .limits
                    .max_context_tokens,
                allowed
            );
        }
        for denied in [Limits::PROFILE_CONTEXT_CEILING + 1, usize::MAX] {
            let limits = Config::configured_limits(
                &provider,
                None,
                Some(&json!({"max_context_tokens":denied}).to_string()),
            )
            .unwrap();
            assert!(
                Config::with_provider(provider.clone(), limits)
                    .unwrap_err()
                    .message
                    .contains("profile ceiling")
            );
        }
    }
    #[test]
    fn basic_known_provider_needs_no_limits_blob_and_custom_profile_is_model_bound() {
        let mut provider = super::super::tests::config().provider;
        provider.model_id = "gpt-4.1".into();
        let defaults = Config::configured_limits(&provider, None, None).unwrap();
        assert_eq!(defaults.max_context_tokens, 131072);
        assert_eq!(defaults.tokenizer.model_id, provider.model_id);
        assert!(!defaults.vision_enabled);
        Config::with_provider(provider.clone(), defaults).unwrap();
        provider.model_id = "custom-unverified-model".into();
        assert!(Config::configured_limits(&provider, None, None).is_err());
        let profile = r#"{"encoding":"o200k_base","calibration":{"multiplier_bps":11500,"provenance":"synthetic measurement fixture only"}}"#;
        let custom =
            Config::configured_limits(&provider, Some(profile), Some(r#"{"vision_enabled":true}"#))
                .unwrap();
        assert_eq!(custom.tokenizer.model_id, provider.model_id);
        assert_eq!(custom.max_context_tokens, 131072);
        assert!(custom.vision_enabled);
        for key in [
            "run_budget",
            "max_turns",
            "max_tool_calls",
            "max_read_bytes",
            "max_review_rounds",
            "reviewer_reserve",
            "pack_max_units",
            "pack_max_turns",
            "max_tool_result_bytes",
            "tokenizer",
        ] {
            assert!(
                Config::configured_limits(
                    &provider,
                    Some(profile),
                    Some(&json!({key:1}).to_string())
                )
                .is_err(),
                "{key}"
            );
        }
        assert!(
            Config::configured_limits(
                &provider,
                Some(r#"{"model_id":"someone-else","encoding":"o200k_base"}"#),
                None
            )
            .is_err()
        );
    }
}
