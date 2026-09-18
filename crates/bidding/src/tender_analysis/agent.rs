use super::*;
use crate::agent_runtime::{Driver, Status, drive};
pub(super) mod context;
pub(super) mod evidence_delivery;
pub(super) mod main_dispatch;
mod main_handoff;
pub(super) mod main_work;
pub mod repair;
pub(super) mod repair_recovery;
pub(super) mod repair_task_host;
mod view_io;
use crate::agent_runtime::progress::{Progress, Recovery};
use crate::{agent_error::AgentError, authoring_runtime::AuthoringRuntimeContractV1};
use async_trait::async_trait;
pub use context::WorkState;
use context::WorkStatus;
use knowledge::models::ChatTurn;
use serde_json::{Value, json};
use std::time::Instant;
use tokio_util::sync::CancellationToken;
use view_io::read_source_view;

const MAIN: &str = include_str!("../../prompts/tender-analysis-main-v1.txt");

const REVIEWER: &str = include_str!("../../prompts/tender-analysis-reviewer-v1.txt");
const DRAFT_OUTLINE: &str = include_str!("../../prompts/tender-draft-outline-v1.txt");
const DRAFT_FILL: &str = include_str!("../../prompts/tender-draft-fill-v1.txt");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    #[serde(default = "crate::agent_runtime::progress::default_no_progress_turns")]
    pub max_no_progress_turns: usize,
    #[serde(default = "crate::agent_runtime::progress::default_focus_turns")]
    pub max_focus_turns: usize,
    #[serde(default = "crate::agent_runtime::progress::default_focus_replans")]
    pub max_focus_replans: usize,
    pub max_turns: usize,
    pub max_tool_calls: usize,
    pub max_read_bytes: usize,
    pub max_context_bytes: usize,
    /// Maximum retained delivered history, excluding the latest pending tool group.
    pub max_history_bytes: usize,
    /// Application token budget, including estimated input and reserved output.
    pub max_context_tokens: usize,
    pub image_token_reserve: usize,
    pub token_safety_margin: usize,
    pub max_tool_result_bytes: usize,
    pub max_review_rounds: usize,
    pub max_source_view_bytes: usize,
    pub max_source_view_edge: u32,
    /// Turns reserved for independent review. Main must EnterReview before spending these.
    #[serde(default)]
    pub reviewer_reserve: usize,
    /// 1 keeps tests on one source per dispatch root. Production must set ≥6.
    #[serde(default = "default_pack_max_units")]
    pub pack_max_units: usize,
    /// 0 means only the unit cap applies.
    #[serde(default)]
    pub pack_max_chars: usize,
    /// 0 keeps the existing per-root batch cap. Production packs use 3.
    #[serde(default)]
    pub pack_max_turns: usize,
    /// 分析入口固定大纲→填章。产品不提供开关；仅单测可构造 false 覆盖抽取合同。
    #[serde(default)]
    pub draft_path: bool,
    /// 可选组成绑定附加词；缺省只用 title 包含匹配，禁止代码内置行业词表。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub draft_bind_terms: Vec<String>,
    /// 草稿编译字节上限。骨架用小值，整份填充用大值。
    #[serde(default = "crate::tender_analysis::draft::default_draft_docx_bytes")]
    pub max_draft_docx_bytes: usize,
}

fn default_pack_max_units() -> usize {
    1
}

impl Limits {
    fn progress(&self) -> crate::agent_runtime::progress::ProgressLimits {
        crate::agent_runtime::progress::ProgressLimits {
            max_no_progress_turns: self.max_no_progress_turns,
            max_focus_turns: self.max_focus_turns,
            max_focus_replans: self.max_focus_replans,
        }
    }

    pub fn at_least_for(
        mut self,
        input: &FrozenInput,
    ) -> Result<Self, crate::tender_analysis::budget::BudgetRefused> {
        if self.draft_path {
            // 阶段一的绝对门。填章请求不走这里：它在冻结请求时按待填章数另记一套
            // 额度（`draft::fill_limits`），所以这个 20 只约束大纲与骨架。
            self.max_turns = self
                .max_turns
                .max(crate::tender_analysis::draft::outline_turn_cap(input));
            self.max_tool_calls = self.max_tool_calls.max(self.max_turns.saturating_mul(12));
            self.max_read_bytes = self.max_read_bytes.max(
                self.max_turns
                    .saturating_mul(self.max_tool_result_bytes)
                    .saturating_mul(4),
            );
            if self.max_turns == 0 {
                self.max_turns = crate::tender_analysis::draft::OUTLINE_MAX_TURNS;
            }
            self.reviewer_reserve = 0;
            return Ok(self);
        }
        let budget = crate::tender_analysis::budget::apply_with_pack(
            input,
            self.max_turns,
            self.max_turns,
            self.pack_max_units,
            self.pack_max_chars,
        )?;
        self.max_turns = budget.applied_turns;
        self.reviewer_reserve = self.reviewer_reserve.max(budget.reviewer_reserve);
        Ok(self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunBudget {
    pub chunk_count: usize,
    pub total_timeout_secs: u64,
    pub publish_reserve_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub checkpoint_contract_version: u32,
    pub runtime_adapter: String,
    pub repair_task_policy: String,
    pub main_dispatch_policy: String,
    pub provider: AuthoringRuntimeContractV1,
    pub limits: Limits,
    pub budget: RunBudget,
    pub tools_sha256: String,
    pub review_tools_sha256: String,
    pub main_prompt_sha256: String,
    pub review_prompt_sha256: String,
    pub fill_tools_sha256: String,
    pub fill_prompt_sha256: String,
}

impl Config {
    pub fn from_environment() -> Result<Self, AgentError> {
        Self::from_limits(Self::environment_limits()?)
    }

    pub fn from_environment_for(input: &FrozenInput) -> Result<Self, AgentError> {
        let limits = Self::environment_limits()?
            .at_least_for(input)
            .map_err(|refused| {
                error(
                    "AGENT_PROVIDER_UNAVAILABLE",
                    format!(
                        "extraction turn estimate {} exceeds ceiling {}",
                        refused.estimated_turns, refused.ceiling
                    ),
                )
            })?;
        Self::with_provider_for(
            AuthoringRuntimeContractV1::resolve_tools_from_environment()
                .map_err(|e| error("AGENT_PROVIDER_UNAVAILABLE", e))?,
            limits,
            Some(input),
        )
    }

    fn environment_limits() -> Result<Limits, AgentError> {
        let raw = std::env::var("KB_TENDER_AGENT_LIMITS").map_err(|_| {
            error(
                "AGENT_PROVIDER_UNAVAILABLE",
                "KB_TENDER_AGENT_LIMITS is required",
            )
        })?;
        let mut limits: Limits = serde_json::from_str(&raw).map_err(invalid)?;
        if limits.pack_max_units < 6 || limits.pack_max_chars < 1200 || limits.pack_max_turns < 3 {
            return Err(error(
                "AGENT_PROVIDER_UNAVAILABLE",
                "KB_TENDER_AGENT_LIMITS must set pack_max_units>=6, pack_max_chars>=1200, pack_max_turns>=3",
            ));
        }
        limits.draft_path = true;
        Ok(limits)
    }

    fn from_limits(limits: Limits) -> Result<Self, AgentError> {
        Self::with_provider(
            AuthoringRuntimeContractV1::resolve_tools_from_environment()
                .map_err(|e| error("AGENT_PROVIDER_UNAVAILABLE", e))?,
            limits,
        )
    }

    pub fn with_provider(
        provider: AuthoringRuntimeContractV1,
        limits: Limits,
    ) -> Result<Self, AgentError> {
        Self::with_provider_for(provider, limits, None)
    }

    pub fn with_provider_for(
        provider: AuthoringRuntimeContractV1,
        mut limits: Limits,
        input: Option<&FrozenInput>,
    ) -> Result<Self, AgentError> {
        limits.draft_path = true;
        if let Some(input) = input {
            limits = limits
                .at_least_for(input)
                .map_err(|e| invalid(format!("outline budget refused: {e:?}")))?;
        }
        let budget = RunBudget {
            chunk_count: input.map_or(0, |input| draft::outline_chunks(input).len()),
            total_timeout_secs: (limits.max_turns as u64)
                .saturating_mul(
                    provider
                        .timeout_ms
                        .div_ceil(1000)
                        .saturating_mul(3)
                        .saturating_add(30),
                )
                .saturating_add(draft::PUBLISH_RESERVE_SECS),
            publish_reserve_secs: draft::PUBLISH_RESERVE_SECS,
        };
        let fill_tools_sha256 =
            digest(&crate::tender_analysis::draft::fill_schemas()).map_err(invalid)?;
        let fill_prompt_sha256 =
            digest(&crate::agent_runtime::chat::system_content(DRAFT_FILL)).map_err(invalid)?;
        let tools_sha256 =
            digest(&crate::tender_analysis::draft::outline_schemas()).map_err(invalid)?;
        let review_tools_sha256 = digest(&tools::schemas_for(true, &limits)).map_err(invalid)?;
        let main_prompt_sha256 = digest(&crate::agent_runtime::chat::system_content(
            if limits.draft_path {
                DRAFT_OUTLINE
            } else {
                MAIN
            },
        ))
        .map_err(invalid)?;
        let review_prompt_sha256 =
            digest(&crate::agent_runtime::chat::system_content(REVIEWER)).map_err(invalid)?;
        let config = Self {
            checkpoint_contract_version: crate::agent_runtime::CHECKPOINT_CONTRACT_VERSION,
            runtime_adapter: crate::agent_runtime::RUNTIME_ADAPTER_VERSION.into(),
            repair_task_policy: repair_task_host::POLICY.into(),
            main_dispatch_policy: main_dispatch::POLICY.into(),
            provider,
            limits,
            budget,
            tools_sha256,
            review_tools_sha256,
            main_prompt_sha256,
            review_prompt_sha256,
            fill_tools_sha256,
            fill_prompt_sha256,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), AgentError> {
        self.provider.validate().map_err(invalid)?;
        let l = &self.limits;
        if self.budget.publish_reserve_secs == 0
            || self.budget.total_timeout_secs <= self.budget.publish_reserve_secs
            || !l.progress().validate()
            || self.checkpoint_contract_version != crate::agent_runtime::CHECKPOINT_CONTRACT_VERSION
            || self.repair_task_policy != repair_task_host::POLICY
            || self.main_dispatch_policy != main_dispatch::POLICY
            || repair::tasks::limit(l).is_err()
            || self.runtime_adapter != crate::agent_runtime::RUNTIME_ADAPTER_VERSION
            || self.provider.response_mode != "tool_calls"
            || l.max_turns == 0
            || l.reviewer_reserve >= l.max_turns
            || l.max_tool_calls == 0
            || l.max_read_bytes == 0
            || l.max_tool_result_bytes < 1024
            || l.max_context_bytes <= l.max_tool_result_bytes
            || l.max_history_bytes == 0
            || l.max_history_bytes >= l.max_context_bytes
            || l.image_token_reserve == 0
            || l.token_safety_margin == 0
            || l.token_safety_margin
                .checked_add(self.provider.max_tokens as usize)
                .is_none_or(|reserved| reserved >= l.max_context_tokens)
            || l.max_review_rounds == 0
            || l.max_source_view_bytes == 0
            || l.max_source_view_edge == 0
            || (l.draft_path && l.reviewer_reserve != 0)
            || (!l.draft_path
                && self.tools_sha256 != digest(&tools::schemas_for(false, l)).map_err(invalid)?)
            || (l.draft_path && {
                let outline =
                    digest(&crate::tender_analysis::draft::outline_schemas()).map_err(invalid)?;
                let fill =
                    digest(&crate::tender_analysis::draft::fill_schemas()).map_err(invalid)?;
                !(self.tools_sha256 == outline
                    && self.fill_tools_sha256 == fill
                    && self.main_prompt_sha256
                        == digest(&crate::agent_runtime::chat::system_content(DRAFT_OUTLINE))
                            .map_err(invalid)?
                    && self.fill_prompt_sha256
                        == digest(&crate::agent_runtime::chat::system_content(DRAFT_FILL))
                            .map_err(invalid)?)
            })
            || self.review_tools_sha256 != digest(&tools::schemas_for(true, l)).map_err(invalid)?
            || (!l.draft_path
                && self.main_prompt_sha256
                    != digest(&crate::agent_runtime::chat::system_content(MAIN))
                        .map_err(invalid)?)
            || self.review_prompt_sha256
                != digest(&crate::agent_runtime::chat::system_content(REVIEWER)).map_err(invalid)?
        {
            return Err(error(
                "FROZEN_INPUT_DIGEST_MISMATCH",
                "invalid or changed frozen Agent contract",
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
    Reviewer,
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
    pub review_draft: BTreeMap<String, Finding>,
    pub source_review: Option<source_review::State>,
    #[serde(default)]
    pub repair: repair::State,
    #[serde(default)]
    pub dispatch: main_dispatch::State,
    pub reviewer_coverage: Coverage,
    /// Coverage after pending read results, committed only after the next
    /// complete model response. Belongs to `role`, not to the other Agent.
    pub pending_coverage: Option<Coverage>,
    pub transcript: Vec<Value>,
    #[serde(default)]
    pub main_progress: Progress,
    #[serde(default)]
    pub reviewer_progress: Progress,
    pub main_work: Option<WorkState>,
    pub reviewer_work: Option<WorkState>,
    pub done: bool,
    pub source_views: BTreeMap<String, views::SourceView>,
    #[serde(default)]
    pub draft_stage: crate::tender_analysis::draft::DraftStage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft_active_id: Option<String>,
    /// 上一轮宿主完整性清单的缺口条数，用来判定修补轮是否还在减少缺口。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft_outline_gaps: Option<usize>,
    #[serde(default, skip_serializing_if = "no_stall")]
    pub draft_outline_stalls: usize,
    /// 大纲窗游标。宿主推进、随 committed 落盘，恢复后接着投下一窗。
    #[serde(default, skip_serializing_if = "no_stall")]
    pub draft_outline_window: usize,
    /// 为压进字节上限而被退回空标题的章，供 UI 告知用户。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub draft_degraded: Vec<String>,
    /// 这次填章是用户叫停的，不是填完了。稿子照出，剩下的章仍空着。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub draft_stopped: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft_compile_object_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft_docx_base64: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline_config_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_config_sha256: Option<String>,
    pub outline_run: crate::tender_analysis::outline_flow::OutlineRun,
}

impl Checkpoint {
    // A prior draft is repair feedback, never a completed review. In normal
    // repair cycles the finalized report remains the authoritative feedback.
    pub(super) fn findings_for_repair(&self) -> Vec<&Finding> {
        self.review.as_ref().map_or_else(
            || self.review_draft.values().collect(),
            |review| review.findings.iter().collect(),
        )
    }

    pub(super) fn work(&self) -> Option<&WorkState> {
        if self.role == Role::Main {
            self.main_work.as_ref()
        } else {
            self.reviewer_work.as_ref()
        }
    }

    fn execution(&self) -> &Progress {
        if self.role == Role::Main {
            &self.main_progress
        } else {
            &self.reviewer_progress
        }
    }

    pub(super) fn coverage(&self) -> &Coverage {
        if self.role == Role::Main {
            &self.analysis.coverage
        } else {
            &self.reviewer_coverage
        }
    }

    pub(super) fn replace_coverage(&mut self, coverage: Coverage) -> Coverage {
        if self.role == Role::Main {
            std::mem::replace(&mut self.analysis.coverage, coverage)
        } else {
            std::mem::replace(&mut self.reviewer_coverage, coverage)
        }
    }

    pub fn progress(&self, input: &FrozenInput) -> Value {
        let coverage = if self.role == Role::Main {
            &self.analysis.coverage
        } else {
            &self.reviewer_coverage
        };
        json!({"phase":self.role,"draft_stage":self.draft_stage,"outline_phase":self.analysis.outline.phase,
            "outline_chapters":self.analysis.draft_plan.len(),
            "outline_requirements":self.analysis.outline.requirements.len(),
            "outline_open_issues":self.analysis.outline.issues.values().filter(|issue| issue.status == crate::tender_analysis::outline_flow::IssueStatus::Open).count(),
            "turn":self.turn,"tool_calls":self.tool_calls,
            "read_bytes":self.read_bytes,"review_rounds":self.review_rounds,"records":self.analysis.records.len(),
            "relations":self.analysis.relations.len(),"unread_ranges":tools::reading_gaps(input,coverage).len(),
            "source_count":input.source_units.len(),"disposition_count":self.analysis.dispositions.len(),
            "source_views":coverage.views.len(),"source_view_failures":coverage.view_failures.len(),
            "review_findings":self.review.as_ref().map_or(0,|r|r.findings.len()),
            "draft_review_findings":self.review_draft.len(),
            "draft_active_id":self.draft_active_id,
            // 填章面板要显示「已填 N/M 章 + 当前章」，这三项是它唯一的数据来源。
            "draft_chapters":self.analysis.draft_plan.iter()
                .filter(|item| item.status != crate::tender_analysis::draft::DraftStatus::Omitted).count(),
            "draft_filled":self.analysis.draft_plan.iter()
                .filter(|item| item.status == crate::tender_analysis::draft::DraftStatus::Filled).count(),
            "draft_active_title":self.draft_active_id.as_ref().and_then(|id|
                self.analysis.draft_plan.iter().find(|item| &item.id == id).map(|item| item.title.clone())),
            "draft_stopped":self.draft_stopped,
            "execution_watch":self.execution().watch,"execution_blockers":self.main_progress.blockers.len()+self.reviewer_progress.blockers.len()})
    }
}

#[async_trait]
pub trait Journal: Send + Sync {
    async fn load(&self) -> Result<Option<Checkpoint>, AgentError>;
    /// Reserve before HTTP; None means the attempt-independent boundary budget
    /// is exhausted. Persist the exact request bytes, including tool contracts.
    async fn reserve(&self, state: &Checkpoint, body: &[u8]) -> Result<Option<usize>, AgentError>;
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
fn invalid(message: impl std::fmt::Display) -> AgentError {
    error("AGENT_OUTPUT_INVALID", message.to_string())
}

struct RunDriver<'a, J, M> {
    input: &'a FrozenInput,
    config: &'a Config,
    state: &'a mut Checkpoint,
    journal: &'a J,
    model: &'a M,
}

#[async_trait]
impl<J: Journal, M: Model> Driver for RunDriver<'_, J, M> {
    fn status(&self) -> Status<'_> {
        let limits = &self.config.limits;
        Status {
            journal: &self.state.journal,
            max_context_bytes: limits.max_context_bytes,
            turn: self.state.turn,
            role: if self.state.role == Role::Main {
                "main"
            } else {
                "reviewer"
            },
            done: self.state.done,
            execution_blocked: (self.state.role == Role::Reviewer
                && repair_task_host::exhausted(self.state, limits))
                || (limits.draft_path
                    && self.state.role == Role::Main
                    && self.state.draft_stage == draft::DraftStage::Outline
                    && self.state.analysis.outline.phase == super::outline_flow::Phase::Discover
                    && self.state.main_progress.watch.recovery == Recovery::Blocked),
            budget_exhausted: self.state.turn >= limits.max_turns
                || self.state.tool_calls >= limits.max_tool_calls
                || self.state.read_bytes >= limits.max_read_bytes,
        }
    }
    fn journal_mut(&mut self) -> &mut crate::agent_runtime::TurnJournal {
        &mut self.state.journal
    }
    async fn prepare_request(&mut self) -> Result<Vec<u8>, AgentError> {
        let started = Instant::now();
        let body = request(self.input, self.config, self.state).await?;
        let estimated_input_tokens = context::estimate_input_tokens(
            &serde_json::from_slice(&body).map_err(invalid)?,
            &self.config.limits,
        )?;
        tracing::info!(event="analysis_request_built",turn=self.state.turn,role=?self.state.role,
            request_bytes=body.len(),estimated_input_tokens,reserved_output_tokens=self.config.provider.max_tokens,
            elapsed_ms=started.elapsed().as_millis() as u64);
        self.state.journal.prepare_session(
            &body,
            crate::agent_runtime::SESSION_PREFIX,
            crate::agent_runtime::ANALYSIS_SESSION_SUFFIX,
            self.config.limits.max_turns - self.state.turn,
            self.config.limits.max_context_bytes,
        )?;
        Ok(body)
    }
    async fn reserve(&self, body: &[u8], _local_attempt: usize) -> Result<usize, AgentError> {
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
    run_seeded(input, config, journal, model, cancel, None, None).await
}

/// Stop model work at the reserve boundary; checked outlines may still compile.
pub(crate) async fn run_with_model_budget<J: Journal, M: Model>(
    input: &FrozenInput,
    config: &Config,
    journal: &J,
    model: &M,
    cancel: &CancellationToken,
    model_budget: std::time::Duration,
) -> Result<AnalysisResult, AgentError> {
    run_seeded(
        input,
        config,
        journal,
        model,
        cancel,
        None,
        Some(model_budget),
    )
    .await
}

/// 用户触发的填章 run：从**回读出来的章树**起跑，而不是从零开始拉大纲。
///
/// 种子是这次 Job 的输入（请求里冻着它的摘要），不是 agent 的产出：填章 run 的
/// 第一个检查点就带着整棵树，checkpoint 闸门按摘要核对。已带正文的章在种子里
/// 就是 `Filled`，填充回路只会派到还空着的章。
pub async fn run_fill<J: Journal, M: Model>(
    input: &FrozenInput,
    config: &Config,
    journal: &J,
    model: &M,
    cancel: &CancellationToken,
    seed: Vec<crate::tender_analysis::draft::DraftPlanItem>,
    outline: super::outline_flow::OutlineState,
) -> Result<AnalysisResult, AgentError> {
    if seed.is_empty() {
        return Err(invalid("fill run needs a read-back chapter tree"));
    }
    if !config.limits.draft_path {
        return Err(invalid("fill run requires the draft path"));
    }
    run_seeded(
        input,
        config,
        journal,
        model,
        cancel,
        Some((seed, outline)),
        None,
    )
    .await
}

async fn run_seeded<J: Journal, M: Model>(
    input: &FrozenInput,
    config: &Config,
    journal: &J,
    model: &M,
    cancel: &CancellationToken,
    seed: Option<(
        Vec<crate::tender_analysis::draft::DraftPlanItem>,
        super::outline_flow::OutlineState,
    )>,
    model_budget: Option<std::time::Duration>,
) -> Result<AnalysisResult, AgentError> {
    let model_deadline = model_budget.map(|budget| tokio::time::Instant::now() + budget);
    tools::validate_input(input).map_err(invalid)?;
    config.validate()?;
    let started = Instant::now();
    let seeded = seed.is_some();
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
        review_draft: BTreeMap::new(),
        source_review: None,
        repair: Default::default(),
        dispatch: Default::default(),
        reviewer_coverage: Coverage::default(),
        pending_coverage: None,
        transcript: vec![],
        main_progress: Progress::default(),
        reviewer_progress: Progress::default(),
        main_work: None,
        reviewer_work: None,
        done: false,
        source_views: BTreeMap::new(),
        draft_stage: Default::default(),
        draft_active_id: None,
        draft_outline_gaps: None,
        draft_outline_stalls: 0,
        draft_outline_window: 0,
        draft_degraded: Vec::new(),
        draft_stopped: false,
        draft_compile_object_id: None,
        draft_docx_base64: None,
        outline_config_sha256: None,
        fill_config_sha256: None,
        outline_run: Default::default(),
    });
    if state.input_sha256 != input_sha256 || state.config_sha256 != config_sha256 {
        return Err(error(
            "FROZEN_INPUT_DIGEST_MISMATCH",
            "checkpoint input or runtime changed",
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
    state.fill_config_sha256 = Some(config.fill_tools_sha256.clone());
    let is_outline_run = seed.is_none();
    match seed {
        Some((plan, outline)) => {
            let seed_identities = super::readback::seed_identities(&plan).map_err(invalid)?;
            if state.draft_stage != crate::tender_analysis::draft::DraftStage::None
                && state.analysis.fill_seed_chapters != seed_identities {
                return Err(invalid("fill checkpoint differs from the frozen saved-document seed"));
            }
            state.analysis.fill_seed_chapters = seed_identities;
            if state.draft_stage == crate::tender_analysis::draft::DraftStage::None {
                state.analysis.draft_plan = plan;
                state.analysis.outline = outline;
                state.draft_stage = crate::tender_analysis::draft::DraftStage::Fill;
                crate::tender_analysis::draft::after_batch(input, &mut state, false, false)
                    .map_err(invalid)?;
            } else if state.draft_stage == crate::tender_analysis::draft::DraftStage::Outline {
                return Err(error(
                    "FROZEN_INPUT_DIGEST_MISMATCH",
                    "fill run resumed an outline checkpoint",
                ));
            }
        }
        None if state.draft_stage == crate::tender_analysis::draft::DraftStage::None => {
            state.draft_stage = crate::tender_analysis::draft::DraftStage::Outline;
            crate::tender_analysis::draft::preload_outline_window(input, &mut state);
        }
        None => {}
    }
    let already_checked = is_outline_run && super::outline_flow::checked(input, &state);
    let driven = if already_checked {
        Ok(())
    } else {
        let mut driver = RunDriver {
            input,
            config,
            state: &mut state,
            journal,
            model,
        };
        if let Some(deadline) = model_deadline {
            if deadline <= tokio::time::Instant::now() {
                return Err(error(
                    "AGENT_DEADLINE_EXCEEDED",
                    "publish reserve reached before checking completed",
                ));
            }
            tokio::time::timeout_at(deadline, drive(&mut driver, cancel))
                .await
                .map_err(|_| {
                    error(
                        "AGENT_DEADLINE_EXCEEDED",
                        "model deadline reached; no partial outline published",
                    )
                })?
        } else {
            drive(&mut driver, cancel).await
        }
    };
    if let Err(error) = driven
        && (is_outline_run
            || !crate::tender_analysis::draft::draft_should_publish_partial(
                &error.code,
                &error.message,
            ))
    {
        return Err(error);
    }
    if is_outline_run && !super::outline_flow::checked(input, &state) {
        return Err(invalid(
            "outline completeness check has not passed; checkpoint retained",
        ));
    }
    if !is_outline_run
        && !state
            .analysis
            .draft_plan
            .iter()
            .any(|item| item.template_id.is_some())
    {
        return Err(invalid(
            "fill produced no new content; current document remains unchanged",
        ));
    }
    let stage = state.draft_stage;
    let turns = state.turn;
    let result = finish_draft_path(input, config, journal, &mut state, input_sha256).await;
    // 出稿快慢是要压下去的目标，不是闸门：这里只记账，供回归对比，不影响成败。
    if let Ok(published) = result.as_ref() {
        let elapsed_secs = started.elapsed().as_secs();
        let target = if seeded {
            crate::tender_analysis::draft::FILL_DEADLINE_SECS
        } else {
            crate::tender_analysis::draft::OUTLINE_DEADLINE_TARGET_SECS
        };
        tracing::info!(
            event = "draft_run_published",
            stage = ?stage,
            fill_run = seeded,
            turns,
            elapsed_secs,
            chapters = published.analysis.draft_plan.len(),
            filled = published
                .analysis
                .draft_plan
                .iter()
                .filter(|item| {
                    item.status == crate::tender_analysis::draft::DraftStatus::Filled
                })
                .count(),
            degraded = state.draft_degraded.len(),
            stopped = state.draft_stopped,
            turn_target = if seeded {
                crate::tender_analysis::draft::fill_turn_cap(published.analysis.draft_plan.len())
            } else {
                crate::tender_analysis::draft::OUTLINE_TURN_TARGET
            },
            seconds_target = target,
        );
    }
    result
}

async fn finish_draft_path<J: Journal>(
    input: &FrozenInput,
    config: &Config,
    journal: &J,
    state: &mut Checkpoint,
    input_sha256: String,
) -> Result<AnalysisResult, AgentError> {
    // Running out of turns never retires a chapter. Pending means "still waiting
    // for a body", and it compiles to an empty heading the user can write into;
    // Omitted drops the heading entirely, which would delete a chapter the user
    // has in front of them in Word.
    if !crate::tender_analysis::draft::plan_ready(&state.analysis.draft_plan) {
        // A zero-node outline cannot compile a chapter document without
        // inventing a chapter, so this stays an explicit failure rather than a
        // job that succeeds with no editable artifact.
        return Err(invalid(
            "draft outline has no chapter; tender parsing produced no bid composition clause",
        ));
    }
    state.draft_stage = crate::tender_analysis::draft::DraftStage::Published;
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
    };
    // Phase one must hand the user an editable Word file, so the outline
    // skeleton compiles too; a job that succeeds without a DOCX leaves the
    // workspace with nothing to edit.
    let outcome = crate::tender_analysis::draft::compile_draft(
        input,
        &result,
        config.limits.max_draft_docx_bytes,
    )
    .map_err(|error| invalid(format!("draft compile failed: {error}")))?;
    if !outcome.degraded.is_empty() {
        tracing::warn!(
            event = "draft_compile_degraded",
            chapters = outcome.degraded.len(),
            "chapter bodies dropped to fit the configured byte budget"
        );
        state.draft_degraded = outcome.degraded.clone();
    }
    let compiled = outcome.compiled;
    let sha = {
        use sha2::{Digest, Sha256};
        hex::encode(Sha256::digest(&compiled.docx))
    };
    state.draft_compile_object_id = Some(format!("objects/{sha}"));
    state.draft_docx_base64 = Some(base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        &compiled.docx,
    ));
    if let Err(error) = journal.save(state, &state.progress(input)).await {
        if error.code == "FROZEN_INPUT_DIGEST_MISMATCH" {
            tracing::warn!(
                event = "draft_finish_checkpoint_skipped",
                error = %error.message,
                "compile overlay kept in memory for publish"
            );
        } else {
            return Err(error);
        }
    }
    result.analysis = state.analysis.clone();
    Ok(result)
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
    let body = serde_json::from_slice(state.journal.body()?).map_err(invalid)?;
    let estimated_input_tokens = context::estimate_input_tokens(&body, &config.limits)?;
    let review_batch = source_review::BatchVersion::capture(state, &body).map_err(invalid)?;
    if response.tool_calls.len() > config.limits.max_tool_calls - state.tool_calls {
        return Err(error(
            "AGENT_TURN_BUDGET_EXCEEDED",
            "tool batch exceeds remaining budget; checkpoint retained",
        ));
    }
    let actual_input_tokens = response.usage.as_ref().and_then(|u| u.prompt_tokens);
    evidence_delivery::confirm(input, config, state, &body)?;
    tracing::info!(event="analysis_token_usage",turn=state.turn,role=?state.role,
        estimated_input_tokens, actual_input_tokens,
        actual_output_tokens=response.usage.as_ref().and_then(|u|u.completion_tokens),
        cached_tokens=response.usage.as_ref().and_then(|u|u.cached_tokens),
        reasoning_tokens=response.usage.as_ref().and_then(|u|u.reasoning_tokens),
        estimate_exceeded=actual_input_tokens.map(|actual| actual > estimated_input_tokens as u64));
    if state.role == Role::Main {
        let messages = body["messages"]
            .as_array()
            .ok_or_else(|| invalid("completed request messages missing"))?;
        let receipts = delivered_repair_feedback(state, messages).map_err(invalid)?;
        state.main_progress.seen.extend(receipts);
        let recovery = repair_recovery::delivered(state, messages).map_err(invalid)?;
        state.main_progress.seen.extend(recovery);
        repair::begin(state).map_err(invalid)?;
    }
    if let Some(delivered) = state.pending_coverage.take() {
        state.replace_coverage(delivered);
    }
    let delivered_discovery = (config.limits.draft_path
        && state.draft_stage == draft::DraftStage::Outline
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
    fit_batch(input, config, state, &response.tool_calls, &pending_views).await?;
    for (call_index, call) in response.tool_calls.iter().enumerate() {
        let tool_started = Instant::now();
        state.tool_calls += 1;
        if state.tool_calls > config.limits.max_tool_calls {
            return Err(error("AGENT_TURN_BUDGET_EXCEEDED", "tool budget exhausted"));
        }
        let readonly = matches!(
            call.name.as_str(),
            "collection_index"
                | "source_index"
                | "read_source"
                | "read_form"
                | "read_form_cell"
                | "read_outline"
                | "read_outline_fragment"
                | "read_review_task"
                | "read_source_view"
                | "search_sources"
                | "inspect_analysis"
                | "check_gaps"
                | "inspect_review"
        );
        let write_before = (!readonly).then(|| state.clone());
        let pending_before = state.pending_coverage.clone();
        let views_before = pending_views.len();
        let read_bytes_before = state.read_bytes;
        let args: Result<Value, _> = serde_json::from_str(&call.arguments);
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
        } else if matches!(call.name.as_str(), "inspect_analysis" | "inspect_review") {
            match args {
                Ok(args) => {
                    inspect_in_context(
                        input,
                        config,
                        state,
                        &args,
                        &response.tool_calls[call_index..],
                        &pending_views,
                        prior_coverage.as_ref().expect("inspection stages coverage"),
                    )
                    .await
                }
                Err(error) => Err(error.to_string()),
            }
        } else {
            args.map_err(|e| e.to_string()).and_then(|args| {
                apply_in_batch(
                    input,
                    config,
                    state,
                    &call.name,
                    &args,
                    review_batch.as_ref(),
                )
            })
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
                json!({"ok":true,"result":value})
            }
            Err(message) => json!({"ok":false,"error":message}),
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
        if state.read_bytes > config.limits.max_read_bytes {
            return Err(error(
                "AGENT_TURN_BUDGET_EXCEEDED",
                "tool output exceeds remaining read budget; checkpoint retained",
            ));
        }
        state
            .transcript
            .push(json!({"role":"tool","tool_call_id":call.id,"content":content}));
        if state.role == role && !state.done {
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
        batch_failed |= !succeeded;
        if succeeded
            && let Some(completion) =
                context::focused_completion(state, &call.name, &out["result"]).map_err(invalid)?
        {
            local_completion = Some(completion);
        }
        if succeeded && let Ok(args) = serde_json::from_str(&call.arguments) {
            source_review::record_query(input, state, &call.name, &args);
        }
        if succeeded
            && call.name == "put_source_review"
            && response
                .tool_calls
                .get(call_index + 1)
                .is_some_and(|next| next.name == "read_review_task")
        {
            source_review::select_next(input, config, state).map_err(invalid)?;
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
    if !pending_views.is_empty() {
        state
            .transcript
            .push(json!({"role":"user","source_view_refs":pending_views}));
    }
    context::observe_progress(state, &role, local_completion, &config.limits).map_err(invalid)?;
    if role == Role::Reviewer {
        source_review::charge_pack(input, config, state).map_err(invalid)?;
        finish_review_batch(input, config, state).map_err(invalid)?;
        source_review::select_next(input, config, state).map_err(invalid)?;
    } else if state.role == Role::Reviewer
        && state.review.is_some()
        && review_complete(input, config, state).map_err(invalid)?
    {
        // A successful handoff with all independent judgments still current
        // has no new review work. Main-role reading receipts can change the
        // full analysis digest without repairing any of its findings.
        record_completed_review(input, config, state, true).map_err(invalid)?;
    }
    // 停止只在填章回路里问一次，且只在章界生效：本章仍按预算写完，之后不再派新章。
    let stop = state.draft_stage == crate::tender_analysis::draft::DraftStage::Fill
        && journal.stop_requested().await?;
    crate::tender_analysis::draft::after_batch(input, state, batch_failed, stop)
        .map_err(invalid)?;
    state.turn += 1;
    if state.role != role
        || (outline_phase_before != state.analysis.outline.phase
            && state.analysis.outline.phase == super::outline_flow::Phase::Check)
    {
        state.transcript.clear();
    }
    Ok(tool_results)
}

const BATCH_OUTPUT_DEFERRED: &str = "Tool output does not fit the remaining batch context. Request a smaller range or fewer calls next turn. This result commits no business change or new reading/review coverage.";

/// Called once after every tool in the saved response has been applied. The
/// caller commits this result with the tools, never as a separate model turn.
pub(super) fn finish_review_batch(
    input: &FrozenInput,
    config: &Config,
    state: &mut Checkpoint,
) -> Result<(), String> {
    if state.role != Role::Reviewer || state.done || !review_complete(input, config, state)? {
        return Ok(());
    }
    record_completed_review(input, config, state, false)
}

fn record_completed_review(
    input: &FrozenInput,
    config: &Config,
    state: &mut Checkpoint,
    unchanged_handoff: bool,
) -> Result<(), String> {
    let sha = digest(&state.analysis)?;
    let findings: Vec<_> = state.review_draft.values().cloned().collect();
    let repeated = state
        .review
        .as_ref()
        .is_some_and(|r| r.analysis_sha256 == sha);
    state.review_rounds += 1;
    state.done = findings.is_empty()
        || repeated
        || unchanged_handoff
        || state.review_rounds >= config.limits.max_review_rounds;
    let source_review = state
        .source_review
        .as_mut()
        .ok_or("source review state missing")?;
    source_review.completed_analysis_sha256 = Some(sha.clone());
    source_review.active_task = None;
    let global_checks: Vec<_> = state
        .analysis
        .review_global_checks
        .values()
        .cloned()
        .collect();
    let contract_sha256 = crate::tender_analysis::rule_contract::contract_sha256()?;
    let omitted_sources = source_review::omitted_sources(input, config, state)?;
    state.review = Some(Review {
        analysis_sha256: sha,
        coverage: state.reviewer_coverage.clone(),
        findings,
        contract_sha256,
        global_checks,
        omitted_sources,
        draft: false,
    });
    // Retain findings and source receipts through repairs. The reviewer must
    // explicitly revise/withdraw them against the repaired candidate versions.
    if !state.done {
        state.role = Role::Main;
    }
    Ok(())
}

pub(super) fn review_complete(
    input: &FrozenInput,
    config: &Config,
    state: &Checkpoint,
) -> Result<bool, String> {
    if !state.main_progress.blockers.is_empty()
        || !state.reviewer_progress.blockers.is_empty()
        || !tools::gaps(input, &state.analysis).is_empty()
        || !tools::review_gaps(input, &state.analysis, &state.reviewer_coverage).is_empty()
        || !source_review::pending(input, config, state)?.is_empty()
    {
        return Ok(false);
    }
    let scope: Vec<_> = input
        .source_units
        .iter()
        .map(|s| s.source_unit_revision_id.clone())
        .collect();
    for key in context::scope_references(&state.analysis, &scope) {
        if !context::has_review_outcome(state, &key)? {
            return Ok(false);
        }
    }
    for finding in state.review_draft.values() {
        if validate_finding(input, state, finding).is_err() {
            return Ok(false);
        }
    }
    let checks: Vec<_> = state
        .analysis
        .review_global_checks
        .values()
        .cloned()
        .collect();
    let findings: Vec<_> = state.review_draft.values().cloned().collect();
    if rule_contract::validate_inventory(input, &state.analysis, &checks, &findings).is_err()
        || checks.iter().any(|check| {
            rule_contract::validate_evidence(
                input,
                &state.analysis,
                &state.reviewer_coverage,
                check,
            )
            .is_err()
        })
    {
        return Ok(false);
    }
    Ok(true)
}

/// Failed exact lookups remain failures; suggest only an existing navigation query.
fn inspection_error(error: tools::InspectionError, args: &Value, max_bytes: usize) -> String {
    let tools::InspectionError::CandidateIdentity(message) = error else {
        return error.into();
    };
    let Some(limit) = args["limit"].as_u64().filter(|limit| *limit > 0) else {
        return message;
    };
    let mut query =
        json!({"kind":"all","offset":0,"limit":limit,"view":"index","scope":"collection"});
    if let Some(source_id) = args.get("source_id") {
        query["source_id"] = source_id.clone();
    }
    let guided = format!(
        "{message}. Locate full candidate IDs in the collection index; do not guess or complete an ID. An explicit source_id still narrows this query. The returned query_scope and total describe only its filters, not semantic absence. Follow returned next until total as needed, then inspect exact IDs with view=detail. Index navigation grants no detail receipt or work handoff. Next inspect_analysis query: {query}"
    );
    if json!({"ok":false,"error":guided}).to_string().len() <= max_bytes {
        guided
    } else {
        message
    }
}

/// Candidate and finding pages must coexist with the source under comparison.
/// Shrink the page before falling back to eviction of that evidence.
/// Probe only bounded transcript/receipt data, not the graph or cached pixels.
pub(super) async fn inspect_in_context(
    input: &FrozenInput,
    config: &Config,
    state: &mut Checkpoint,
    args: &Value,
    remaining: &[knowledge::models::ChatToolCall],
    views: &[String],
    committed: &Coverage,
) -> Result<Value, String> {
    let scope = state
        .work()
        .filter(|work| work.status == WorkStatus::Active)
        .map(|work| work.source_scope.clone());
    let expected = context::focused_work_evidence(state, &state.transcript);
    let mut query = args.clone();
    loop {
        let mut coverage = state.coverage().clone();
        let page = if remaining[0].name == "inspect_review" {
            inspect_review(state, &query, config.limits.max_tool_result_bytes)?
        } else {
            tools::inspect_analysis(
                input,
                &state.analysis,
                &mut coverage,
                committed,
                &query,
                config.limits.max_tool_result_bytes,
                scope.as_deref(),
            )
            .map_err(|error| inspection_error(error, &query, config.limits.max_tool_result_bytes))?
        };
        let transcript = state.transcript.clone();
        let counters = (state.turn, state.tool_calls, state.read_bytes);
        let staged = state.replace_coverage(committed.clone());
        let pending = state.pending_coverage.replace(coverage.clone());
        state
            .transcript
            .push(json!({"role":"tool","tool_call_id":remaining[0].id,
            "content":json!({"ok":true,"result":page}).to_string()}));
        state
            .transcript
            .extend(remaining[1..].iter().map(|call| deferred_message(&call.id)));
        if !views.is_empty() {
            state
                .transcript
                .push(json!({"role":"user","source_view_refs":views}));
        }
        state.turn = state.turn.saturating_add(1);
        state.tool_calls = state.tool_calls.saturating_add(remaining.len() - 1);
        state.read_bytes = config.limits.max_read_bytes;
        let sized = prepare_request(input, config, state, false).await;
        let mut actual = context::focused_work_evidence(state, &state.transcript);
        state.transcript = transcript;
        (state.turn, state.tool_calls, state.read_bytes) = counters;
        state.replace_coverage(staged);
        state.pending_coverage = pending;
        // Active original views may have moved from history into working
        // messages. Verify their actual serialized identity and pixels; cache
        // presence alone is not evidence that the model will receive them.
        let body = sized
            .as_ref()
            .ok()
            .map(|bytes| serde_json::from_slice::<Value>(bytes))
            .transpose()
            .map_err(|error| error.to_string())?;
        if let Some(messages) = body.as_ref().and_then(|body| body["messages"].as_array()) {
            actual.extend(context::focused_work_evidence(state, messages));
        }
        let missing: Vec<_> = expected
            .iter()
            .filter(|(key, ranges)| {
                !ranges
                    .iter()
                    .all(|&(a, b)| tools::contains(actual.get(*key), a, b))
                    && !key.strip_prefix("view:").is_some_and(|id| {
                        state.source_views.get(id).is_some_and(|view| {
                            body.as_ref()
                                .and_then(|body| body["messages"].as_array())
                                .is_some_and(|messages| messages.contains(&view.message()))
                        })
                    })
            })
            .map(|(key, _)| key.clone())
            .collect();
        match sized {
            Ok(_) if missing.is_empty() => {
                state.replace_coverage(coverage);
                return Ok(page);
            }
            Err(error) if error.code != "AGENT_TURN_BUDGET_EXCEEDED" => return Err(error.message),
            _ => {}
        }
        let returned = page["items"]
            .as_array()
            .ok_or("inspection page missing")?
            .len();
        if returned <= 1 {
            let missing = tools::bounded_page(
                &missing,
                0,
                usize::MAX,
                config.limits.max_tool_result_bytes / 4,
            )?;
            return Err(format!(
                "inspection result and current source evidence cannot fit together even with one result; narrow the current comparison focus or finish its evidence comparison before fetching more details. An index is navigation only. Evidence omitted by the projected request: {missing}"
            ));
        }
        query["limit"] = json!(returned / 2);
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
    state
        .transcript
        .extend(remaining.iter().map(|c| deferred_message(&c.id)));
    if !views.is_empty() {
        state
            .transcript
            .push(json!({"role":"user","source_view_refs":views}));
    }
    // Size the next turn, including worst-case digits in the read counter.
    let (turn, tool_calls, read_bytes) = (state.turn, state.tool_calls, state.read_bytes);
    state.turn = state.turn.saturating_add(1);
    state.tool_calls = state.tool_calls.saturating_add(remaining.len());
    state.read_bytes = config.limits.max_read_bytes;
    let result = prepare_request(input, config, state, false)
        .await
        .map(|_| ());
    (state.turn, state.tool_calls, state.read_bytes) = (turn, tool_calls, read_bytes);
    let temporary = remaining.len() + usize::from(!views.is_empty());
    state
        .transcript
        .truncate(state.transcript.len() - temporary);
    result
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
    context::annotate_delivered_source_lines(state, config.limits.max_tool_result_bytes)
        .map_err(invalid)?;
    if state.execution().watch.needs_replan_context() {
        // Recovery must change the working context before another full-budget
        // inventory loop. Reuse the lossless history operations proactively;
        // the latest group, receipts and cumulative recovery state are intact.
        while context::compact_delivered_navigation(&mut state.transcript)
            || context::compact_recallable_candidate_details(
                state,
                config.limits.max_tool_result_bytes,
            )
            || context::evict_delivered_group(state, config.limits.max_history_bytes, false)
        {
        }
    }
    let mut excluded_recall = std::collections::BTreeSet::new();
    let mut omit_preloaded_evidence = false;
    let mut package_budget = None;
    let discovering = config.limits.draft_path
        && matches!(
            state.draft_stage,
            draft::DraftStage::None | draft::DraftStage::Outline
        )
        && state.analysis.outline.phase == super::outline_flow::Phase::Discover;
    loop {
        let reviewer = state.role == Role::Reviewer;
        let review_packet = if reviewer {
            source_review::packet(input, config, state).map_err(invalid)?
        } else {
            Value::Null
        };
        let draft_fill = config.limits.draft_path
            && matches!(
                state.draft_stage,
                crate::tender_analysis::draft::DraftStage::Fill
                    | crate::tender_analysis::draft::DraftStage::Published
            );
        let system = if config.limits.draft_path {
            if draft_fill {
                DRAFT_FILL
            } else {
                DRAFT_OUTLINE
            }
        } else if reviewer {
            REVIEWER
        } else {
            MAIN
        };
        let mut messages = vec![
            json!({"role":"system","content":system}),
            json!({"role":"user","content":json!({"project_id":input.project_id,"document_set_id":input.document_set_id,
                "source_count":input.source_units.len(),"form_count":input.structured_forms.len(),
                "document_count":input.documents.len(),"relation_count":input.document_relations.len(),"decision_count":input.decisions.len(),
            }).to_string()}),
        ];
        let latest = state
            .transcript
            .iter()
            .rposition(|m| m["role"] == "assistant")
            .map(|index| {
                if index > 0 && evidence_delivery::is_retained_message(&state.transcript[index - 1])
                {
                    index - 1
                } else {
                    index
                }
            })
            .unwrap_or(0);
        let mut history_end = messages.len();
        let mut visible_views = std::collections::BTreeSet::new();
        for (index, message) in state.transcript.iter().enumerate() {
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
            if index < latest {
                history_end = messages.len();
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
            let support_views = context::reviewer_support_view_ids(state);
            let assigned_pack = if reviewer {
                work.source_scope.clone()
            } else {
                Vec::new()
            };
            // One original page can cover several parsed text/grid sources.
            // Reuse one delivered image, preferring already visible pixels.
            let layout_view = state
                .coverage()
                .views
                .iter()
                .filter(|(_, identity)| {
                    review_packet["current"]["layout_view"].is_object()
                        && work.source_scope.contains(&identity.source_id)
                        && assigned_pack.iter().any(|source| {
                            source_review::same_page(input, source, &identity.source_id)
                        })
                })
                .min_by_key(|(id, _)| !visible_views.contains(id.as_str()))
                .map(|(id, _)| id);
            for (id, identity) in &state.coverage().views {
                if ((work.source_scope.contains(&identity.source_id)
                    && (assigned_pack.contains(&identity.source_id)
                        || layout_view == Some(id)
                        || focused_views.contains(id)))
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
            context::retained_candidate_message(state, config.limits.max_tool_result_bytes)
                .map_err(invalid)?;
        context::trim_optional_candidate_recall(state, &mut recalled, &mut excluded_recall, 0)
            .map_err(invalid)?;
        if !recalled.is_null() {
            messages.push(recalled.clone());
        }
        let preloaded_evidence = if omit_preloaded_evidence {
            None
        } else {
            evidence_delivery::select_with_budget(input, config, state, package_budget)?
                .map(|evidence| evidence.content)
        };
        if discovering && package_budget.is_some() && preloaded_evidence.is_none() {
            if context::evict_completed_discovery_history(state, config.limits.max_history_bytes) {
                package_budget = None;
                continue;
            }
            return Err(error(
                "AGENT_TURN_BUDGET_EXCEEDED",
                "context budget cannot hold a minimal discovery evidence package",
            ));
        }
        let has_preloaded_evidence = preloaded_evidence.is_some();
        let host = if config.limits.draft_path {
            let mut packet = json!({
                "progress":state.progress(input),
                "work":context::request_work(state).map_err(invalid)?,
            });
            if matches!(
                state.draft_stage,
                draft::DraftStage::None | draft::DraftStage::Outline
            ) {
                packet["source_index"] =
                    draft::outline_index(input, state, config.limits.max_tool_result_bytes);
                packet["outline_state"] =
                    super::outline_flow::packet(input, state, config.limits.max_tool_result_bytes)
                        .map_err(invalid)?;
            }
            if let Some(evidence) = &preloaded_evidence {
                packet["preloaded_evidence"] = evidence.clone();
            }
            packet
        } else {
            json!({
            "progress":state.progress(input),"work":context::request_work(state).map_err(invalid)?,
            "execution":context::execution_packet(state,config.limits.max_tool_result_bytes).map_err(invalid)?,
            "work_state":context::request_work_state(input,state,config.limits.max_tool_result_bytes).map_err(invalid)?,
            "source_review":review_packet,
            "main_dispatch":json!(main_dispatch::projection(input,config,state).map_err(invalid)?),
            "global_analysis_checks":global_check_packet(input,state).map_err(invalid)?,
            "preloaded_evidence":preloaded_evidence,
            "review_findings":if reviewer {Value::Null}else{repair_feedback_packet(state, &messages, &config.limits).map_err(invalid)?}
            })
        };
        messages.push(json!({"role":"user","content":host.to_string()}));
        let bytes = crate::agent_runtime::chat::prepare(
            &config.provider,
            messages,
            if config.limits.draft_path {
                if draft_fill {
                    crate::tender_analysis::draft::fill_schemas()
                } else {
                    crate::tender_analysis::draft::outline_schemas()
                }
            } else {
                tools::schemas_for(reviewer, &config.limits)
            },
        )
        .await?;
        let body: Value = serde_json::from_slice(&bytes).map_err(invalid)?;
        let history_bytes = if history_end > 2 {
            let messages = body["messages"]
                .as_array()
                .ok_or_else(|| invalid("SDK messages missing"))?;
            let history = messages
                .get(2..history_end)
                .ok_or_else(|| invalid("SDK message grouping changed"))?;
            serde_json::to_vec(history).map_err(invalid)?.len()
        } else {
            0
        };
        let context_excess = bytes
            .len()
            .saturating_sub(config.limits.max_context_bytes)
            .max(
                context::estimate_input_tokens(&body, &config.limits)?
                    .saturating_add(config.provider.max_tokens as usize)
                    .saturating_sub(config.limits.max_context_tokens),
            );
        let fits_total = context_excess == 0;
        if fits_total && history_bytes <= config.limits.max_history_bytes {
            return Ok(bytes);
        }
        // A mixed batch can bind a large old index to unique source evidence.
        // Compact only delivered navigation before evicting whole groups.
        if context::compact_delivered_navigation(&mut state.transcript)
            || context::compact_recallable_candidate_details(
                state,
                config.limits.max_tool_result_bytes,
            )
            || context::evict_delivered_group(state, config.limits.max_history_bytes, false)
        {
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
        } else if discovering && has_preloaded_evidence {
            if context::evict_completed_discovery_history(state, config.limits.max_history_bytes) {
                package_budget = None;
                continue;
            }
            // Retry the entire request with a smaller package. The exact cap
            // travels in the reserved payload so receipt replay is deterministic.
            let current =
                preloaded_evidence.as_ref().unwrap()["assigned_evidence"]["workload_bytes_limit"]
                    .as_u64()
                    .unwrap_or(0) as usize;
            if current <= 1 {
                return Err(error(
                    "AGENT_TURN_BUDGET_EXCEEDED",
                    "context budget cannot hold a minimal discovery evidence package",
                ));
            }
            package_budget = Some(current / 2);
        } else if has_preloaded_evidence {
            // Keep the current protocol group intact. An optional evidence
            // preload that cannot fit grants no receipt; explicit tools remain.
            omit_preloaded_evidence = true;
        } else if context::evict_delivered_group(state, config.limits.max_history_bytes, true) {
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
        } else {
            return Err(error(
                "AGENT_TURN_BUDGET_EXCEEDED",
                "context budget cannot hold current tool group",
            ));
        }
    }
}

pub fn validate_finding(
    input: &FrozenInput,
    state: &Checkpoint,
    finding: &Finding,
) -> Result<(), String> {
    if finding.code.trim().is_empty()
        || finding.message.trim().is_empty()
        || finding.correction.trim().is_empty()
    {
        return Err("actionable finding needs code, message and a concrete correction".into());
    }
    for source in &finding.sources {
        tools::validate_span(input, &state.reviewer_coverage, source)?;
    }
    let mut fields = std::collections::BTreeSet::new();
    for affected in &finding.affected {
        let id = &affected.id;
        if !fields.insert((id, &affected.path)) {
            return Err("duplicate affected field".into());
        }
        let kind = if state.analysis.records.contains_key(id) {
            "record"
        } else if state.analysis.relations.contains_key(id) {
            "relation"
        } else {
            return Err("foreign affected record".into());
        };
        let key = format!("{kind}:{id}");
        let object = context::reference(&state.analysis, &key)?;
        let current = digest(&object)?;
        if state.reviewer_coverage.candidate.get(&key) != Some(&current) {
            return Err(format!(
                "independently inspect current affected outcome: {key}"
            ));
        }
        if object.pointer(&affected.path).is_none() {
            return Err(format!(
                "unknown affected field: {key}{}; use a JSON Pointer to the inspected object",
                affected.path
            ));
        }
    }
    if finding.sources.is_empty() && finding.affected.is_empty() {
        return Err("finding needs source evidence or an affected record".into());
    }
    Ok(())
}

fn repair_finding_receipt(finding: &Finding) -> Result<String, String> {
    Ok(format!("repair_feedback:{}", digest(finding)?))
}

// Read receipts belong to the main role and exact finding contents. They use
// the existing durable seen set, but do not reset a progress watch or attest a
// repair. Only tool results actually present in a completed request count.
pub(super) fn delivered_repair_feedback(
    state: &Checkpoint,
    messages: &[Value],
) -> Result<std::collections::BTreeSet<String>, String> {
    let known = state
        .findings_for_repair()
        .into_iter()
        .map(repair_finding_receipt)
        .collect::<Result<std::collections::BTreeSet<_>, _>>()?;
    let queries: std::collections::BTreeSet<_> = messages
        .iter()
        .filter(|message| message["role"] == "assistant")
        .flat_map(|message| message["tool_calls"].as_array().into_iter().flatten())
        .filter(|call| call["function"]["name"] == "inspect_review")
        .filter_map(|call| call["id"].as_str())
        .collect();
    let mut received = std::collections::BTreeSet::new();
    for message in messages {
        if message["role"] != "tool"
            || !message["tool_call_id"]
                .as_str()
                .is_some_and(|id| queries.contains(id))
        {
            continue;
        }
        let Some(content) = message["content"]
            .as_str()
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
        else {
            continue;
        };
        if content["ok"] != true {
            continue;
        }
        for value in content["result"]["items"].as_array().into_iter().flatten() {
            // Main feedback pages contain whole Finding values, unlike the
            // reviewer's independently owned draft ID/value index.
            if let Ok(finding) = serde_json::from_value::<Finding>(value.clone()) {
                let receipt = repair_finding_receipt(&finding)?;
                if known.contains(&receipt) {
                    received.insert(receipt);
                }
            }
        }
    }
    Ok(received)
}

pub(super) fn repair_feedback_packet(
    state: &Checkpoint,
    messages: &[Value],
    limits: &Limits,
) -> Result<Value, String> {
    // Project only the evidence delivered in this request for navigation. The
    // same receipts become durable after its complete model response, before
    // applying that response's tools; queued or failed sends cannot grant them.
    let projected = delivered_repair_feedback(state, messages)?;
    let findings = state.findings_for_repair();
    let mut unread = Vec::new();
    for (index, finding) in findings.iter().enumerate() {
        let receipt = repair_finding_receipt(finding)?;
        if !state.main_progress.seen.contains(&receipt) && !projected.contains(&receipt) {
            unread.push(index);
        }
    }
    Ok(
        json!({"count":findings.len(),"received":findings.len()-unread.len(),
        "unread":unread.len(),
        "next_query":unread.first().map(|offset|json!({"offset":offset,"limit":findings.len()-offset})),
        "repair":repair::packet(state, limits)?,
        "instruction":"Read missing repair feedback with inspect_review using next_query and follow each returned next offset. Current-request receipts are provisional until this response completes. Compare every finding against original evidence and repair or source-back a disagreement before requesting independent review. Receiving the whole feedback is required for handoff, but is never proof of repair or approval."}),
    )
}

/// Retrieve complete findings within the existing byte budget. Querying a
/// finding neither changes it nor acknowledges any semantic comparison.
pub(super) fn inspect_review(
    state: &Checkpoint,
    args: &Value,
    max_bytes: usize,
) -> Result<Value, String> {
    let object = args.as_object().ok_or("review query must be an object")?;
    if object
        .keys()
        .any(|key| !matches!(key.as_str(), "offset" | "limit" | "ids"))
    {
        return Err("only offset, limit and optional draft finding ids are accepted".into());
    }
    let offset = args["offset"]
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .ok_or("offset required")?;
    let limit = args["limit"]
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .filter(|n| *n > 0)
        .ok_or("positive limit required")?;
    let mut selected = Vec::new();
    if let Some(value) = object.get("ids") {
        if !matches!(state.role, Role::Reviewer) {
            return Err("ids select reviewer draft findings; page prior repair feedback with offset and limit".into());
        }
        let ids: Vec<String> = serde_json::from_value(value.clone())
            .map_err(|_| tools::field_error("/ids", "use a nonempty array of draft finding IDs"))?;
        let mut seen = std::collections::BTreeSet::new();
        if ids.is_empty() {
            return Err(tools::field_error(
                "/ids",
                "use a nonempty array of draft finding IDs",
            ));
        }
        for id in &ids {
            if !seen.insert(id) {
                return Err(tools::field_error("/ids", "finding IDs must be distinct"));
            }
            let finding = state.review_draft.get(id).ok_or_else(|| {
                tools::field_error("/ids", format!("unknown draft finding ID: {id}"))
            })?;
            selected.push(repair::reviewer_item(state, id, finding)?);
        }
    } else if matches!(state.role, Role::Reviewer) {
        selected.extend(
            state
                .review_draft
                .iter()
                .map(|(id, finding)| repair::reviewer_item(state, id, finding))
                .collect::<Result<Vec<_>, String>>()?,
        );
    } else {
        selected.extend(
            state
                .findings_for_repair()
                .into_iter()
                .map(|finding| json!(finding)),
        );
    }
    let total = selected.len();
    if offset > total {
        return Err("offset outside review".into());
    }
    let mut out = json!({"total":total,"next":offset,"items":[]});
    if state.role == Role::Main {
        out["repair_history"] = json!({});
    }
    if serde_json::to_vec(&out).map_err(|e| e.to_string())?.len() > max_bytes {
        return Err("review pagination envelope exceeds budget".into());
    }
    for (index, item) in selected.into_iter().enumerate().skip(offset).take(limit) {
        let history_id = if state.role == Role::Main {
            let finding: Finding =
                serde_json::from_value(item.clone()).map_err(|e| e.to_string())?;
            let id = digest(&finding)?;
            let new_history = out["repair_history"].get(&id).is_none();
            out["repair_history"][&id] = repair::main_history(state, &finding)?;
            Some((id, new_history))
        } else {
            None
        };
        out["items"].as_array_mut().unwrap().push(item);
        out["next"] = json!(index + 1);
        if serde_json::to_vec(&out).map_err(|e| e.to_string())?.len() > max_bytes {
            out["items"].as_array_mut().unwrap().pop();
            if let Some((id, true)) = history_id {
                out["repair_history"].as_object_mut().unwrap().remove(&id);
            }
            out["next"] = json!(index);
            if index == offset {
                return Err("single complete review finding exceeds budget; finding text cannot be truncated".into());
            }
            break;
        }
    }
    Ok(out)
}

#[cfg(test)]
pub(super) fn apply(
    input: &FrozenInput,
    config: &Config,
    state: &mut Checkpoint,
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    apply_in_batch(input, config, state, name, args, None)
}

pub(super) fn apply_in_batch(
    input: &FrozenInput,
    config: &Config,
    state: &mut Checkpoint,
    name: &str,
    args: &Value,
    review_batch: Option<&source_review::BatchVersion>,
) -> Result<Value, String> {
    let args = evidence_refs::expand(input, args)?;
    let result = apply_inner(input, config, state, name, &args, review_batch)?;
    context::synchronize_outcomes(state);
    Ok(result)
}

fn put_analysis_check(
    input: &FrozenInput,
    state: &mut Checkpoint,
    args: &Value,
) -> Result<Value, String> {
    let key = args["key"]
        .as_str()
        .ok_or("global check key required")?
        .to_owned();
    if !ANALYSIS_GLOBAL_CHECK_KEYS.contains(&key.as_str()) {
        return Err(format!("unknown global check {key}"));
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Args {
        key: String,
        expected_scope_sha256: String,
        conclusion: GlobalCheckConclusion,
        grounds: Vec<Span>,
        #[serde(default)]
        record_ids: Vec<String>,
        #[serde(default)]
        finding_ids: Vec<String>,
    }
    let args: Args = serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
    let expected = rule_contract::scope_sha256(input, &state.analysis)?;
    if args.expected_scope_sha256 != expected {
        return Err(format!(
            "global check scope changed; compare the current graph at {expected}"
        ));
    }
    let mut finding_ids = Vec::new();
    for id in &args.finding_ids {
        let finding = state
            .review_draft
            .get(id)
            .or_else(|| {
                state
                    .review_draft
                    .values()
                    .find(|finding| digest(finding).ok().as_ref() == Some(id))
            })
            .ok_or_else(|| format!("unknown global check finding {id}"))?;
        // Freeze the finding's content identity, which survives the published
        // Review vector without trusting a free-form code or a discarded UUID.
        if state.role == Role::Reviewer {
            validate_finding(input, state, finding)?;
        }
        finding_ids.push(digest(finding)?);
    }
    let check = GlobalCheck {
        key: args.key,
        scope_sha256: expected,
        conclusion: args.conclusion,
        grounds: args.grounds,
        record_ids: args.record_ids,
        finding_ids,
    };
    rule_contract::validate_check(
        input,
        &state.analysis,
        &check,
        &state.review_draft.values().cloned().collect::<Vec<_>>(),
    )?;
    rule_contract::validate_evidence(input, &state.analysis, state.coverage(), &check)?;
    if state.role == Role::Reviewer {
        state
            .analysis
            .review_global_checks
            .insert(key.clone(), check);
    } else {
        state.analysis.main_global_checks.insert(key.clone(), check);
    }
    Ok(json!({"saved":true,"key":key}))
}

fn global_check_packet(input: &FrozenInput, state: &Checkpoint) -> Result<Value, String> {
    let checks = if state.role == Role::Reviewer {
        &state.analysis.review_global_checks
    } else {
        &state.analysis.main_global_checks
    };
    let scope = rule_contract::scope_sha256(input, &state.analysis)?;
    let findings: Vec<_> = state.review_draft.values().cloned().collect();
    let pending: Vec<_> = ANALYSIS_GLOBAL_CHECK_KEYS
        .iter()
        .filter(|key| {
            checks.get(**key).is_none_or(|check| {
                rule_contract::validate_check(input, &state.analysis, check, &findings).is_err()
                    || rule_contract::validate_evidence(
                        input,
                        &state.analysis,
                        state.coverage(),
                        check,
                    )
                    .is_err()
            })
        })
        .copied()
        .collect();
    Ok(json!({"expected_scope_sha256":scope,"pending_keys":pending,
        "instruction":"After source work and relevant candidate comparisons, independently judge these fixed global categories against the current graph and originals. Submit put_analysis_check with this expected scope. A write changing the graph invalidates the scope; prior receipts and empty findings are not semantic approval. Multiple checks may share a tool batch. Finding IDs must name saved findings; the host stores their content digests."}))
}

fn apply_inner(
    input: &FrozenInput,
    config: &Config,
    state: &mut Checkpoint,
    name: &str,
    args: &Value,
    review_batch: Option<&source_review::BatchVersion>,
) -> Result<Value, String> {
    let reviewer = state.role == Role::Reviewer;
    if state.execution().watch.recovery == Recovery::Blocked
        && !(config.limits.draft_path
            && matches!(
                name,
                "put_outline_item"
                    | "put_outline_items"
                    | "submit_outline_scan"
                    | "read_outline"
                    | "assign_outline_fragments"
                | "finish_outline"
                    | "submit_outline_check"
                    | "omit_outline_item"
                    | "put_chapter_template"
                    | "skip_chapter_content"
            ))
        && !matches!(
            name,
            "set_work_note" | "check_gaps" | "source_index" | "collection_index" | "inspect_review"
        )
    {
        return Err("local execution is blocked; select an independent source scope, or retry after its saved dependencies change".into());
    }
    if config.limits.draft_path {
        if state.analysis.outline.phase == crate::tender_analysis::outline_flow::Phase::Check
            && !matches!(
                name,
                "read_outline" | "read_outline_fragment" | "submit_outline_check"
            )
        {
            return Err(
                "check phase allows only read_outline and submit_outline_check; rescanning and free source reads are closed"
                    .into(),
            );
        }
        if matches!(
            name,
            "put_outline_item"
                | "put_outline_items"
                | "submit_outline_scan"
                | "read_outline"
                | "read_outline_fragment"
                | "assign_outline_fragments"
                | "finish_outline"
                | "submit_outline_check"
                | "omit_outline_item"
                | "put_chapter_template"
                | "skip_chapter_content"
        ) {
            return crate::tender_analysis::draft::apply(input, config, state, name, args);
        }
        if !matches!(
            name,
            "collection_index"
                | "source_index"
                | "search_sources"
                | "read_source"
                | "read_form"
                | "read_form_cell"
                | "read_source_view"
        ) {
            return Err("unknown or role-forbidden tool".into());
        }
    }
    main_dispatch::check_action(input, state, name, args)?;
    context::check_delete(state, name, args)?;
    match name {
        "read_review_task" if reviewer => {
            if args.as_object().is_none_or(|args| !args.is_empty()) {
                return Err("read_review_task takes no arguments; use the assigned task".into());
            }
            let evidence = source_review::evidence(input, config, state)?.ok_or(
                "assigned task has no readable text/grid packet; use explicit reading tools",
            )?;
            state.reviewer_coverage = evidence.coverage;
            Ok(evidence.content)
        }
        "put_source_review" if reviewer => {
            source_review::put_in_batch(input, config, state, args, review_batch)
        }
        "complete_review_check" if reviewer => {
            context::complete_review_check(input, state, args, config.limits.max_tool_result_bytes)
        }
        "set_work_note" => {
            if args.get("output_refs").is_some() || args.get("pending_refs").is_some() {
                return Err("output_refs and pending_refs are maintained by the host; omit them from tool input".into());
            }
            let mut next: WorkState =
                serde_json::from_value(args.clone()).map_err(|e| e.to_string())?;
            if next.status == WorkStatus::Blocked {
                return Err("execution blocking is maintained by the host".into());
            }
            if !reviewer && repair::tasks::active(state).is_some() {
                repair_task_host::check_scope(state, &next.source_scope)?;
            }
            context::check_blocked_scope(state, &next.source_scope)?;
            context::retain_outcomes(&state.analysis, &mut next, state.work());
            context::validate(input, state, &next, config.limits.max_tool_result_bytes)?;
            let resume = if !reviewer && repair::tasks::active(state).is_some() {
                false
            } else {
                repair_recovery::enter(state, &next.source_scope)?
            };
            let handoff = resume
                || next.status == WorkStatus::Complete
                || state.work().is_some_and(|w| {
                    w.status != WorkStatus::Complete
                        && w.source_scope
                            .iter()
                            .any(|id| !next.source_scope.contains(id))
                });
            if reviewer {
                state.reviewer_work = Some(next);
            } else {
                state.main_work = Some(next);
            }
            if handoff
                && let Some(start) = state
                    .transcript
                    .iter()
                    .rposition(|m| m["role"] == "assistant")
            {
                state.transcript.drain(..start);
            }
            Ok(json!({"saved":true,"handoff":handoff}))
        }
        "check_gaps" if args["scope"] == "execution" || args["scope"] == "pending" => {
            context::execution_gaps(state, args, config.limits.max_tool_result_bytes)
        }
        "check_gaps" if args["scope"] == "work" => {
            context::work_gaps(input, state, args, config.limits.max_tool_result_bytes)
        }
        "put_review_finding" if reviewer => {
            if args
                .as_object()
                .is_none_or(|o| o.len() != 2 || !o.contains_key("id") || !o.contains_key("finding"))
            {
                return Err("review finding needs exactly id and finding".into());
            }
            let id = if args["id"].is_null() {
                uuid::Uuid::new_v4().to_string()
            } else {
                args["id"]
                    .as_str()
                    .filter(|id| state.review_draft.contains_key(*id))
                    .ok_or("unknown review finding; use null to allocate an ID")?
                    .to_owned()
            };
            let finding: Finding =
                serde_json::from_value(args["finding"].clone()).map_err(|e| e.to_string())?;
            validate_finding(input, state, &finding)?;
            // A saved finding must remain retrievable as a whole page item,
            // including its ID and pagination envelope. Draft size cannot
            // exceed the request's existing tool-call budget.
            let page = json!({"total":config.limits.max_tool_calls,
                "next":config.limits.max_tool_calls,"items":[{"id":id,"finding":finding}]});
            if serde_json::to_vec(&page).map_err(|e| e.to_string())?.len()
                > config.limits.max_tool_result_bytes
            {
                return Err(
                    "finding exceeds budget; retain a concise field error and source references"
                        .into(),
                );
            }
            repair::check_main_budget(
                &finding,
                state.repair.results.get(&digest(&finding)?),
                config.limits.max_tool_calls,
                config.limits.max_tool_result_bytes,
            )?;
            let prior = state.review_draft.get(&id).cloned();
            source_review::finding_changed(state, prior.as_ref(), Some(&finding))?;
            state.review_draft.insert(id.clone(), finding);
            Ok(json!({"id":id,"saved":true}))
        }
        "delete_review_finding" if reviewer => {
            if args
                .as_object()
                .is_none_or(|o| o.len() != 1 || !o.contains_key("id"))
            {
                return Err("only a review finding ID is accepted".into());
            }
            let id = args["id"].as_str().ok_or("review finding ID required")?;
            let prior = state
                .review_draft
                .get(id)
                .cloned()
                .ok_or("unknown review finding")?;
            source_review::finding_changed(state, Some(&prior), None)?;
            state.review_draft.remove(id);
            Ok(json!({"deleted":id}))
        }
        "inspect_review" => inspect_review(state, args, config.limits.max_tool_result_bytes),
        "put_repair_result" => {
            repair::tasks::check_put(
                state,
                args["finding_sha256"]
                    .as_str()
                    .ok_or("finding SHA required")?,
                &config.limits,
            )?;
            repair::put(input, config, state, args)
        }
        "request_review" if !reviewer => {
            if args.as_object().is_none_or(|o| !o.is_empty()) {
                return Err("review request takes no arguments".into());
            }
            main_handoff::enter(input, config, state)
        }
        "put_analysis_check" => put_analysis_check(input, state, args),
        _ => {
            let mut coverage = if reviewer {
                state.reviewer_coverage.clone()
            } else {
                state.analysis.coverage.clone()
            };
            let result = tools::invoke(
                input,
                &mut state.analysis,
                &mut coverage,
                reviewer,
                name,
                args,
                config.limits.max_tool_result_bytes,
            )?;
            if reviewer {
                state.reviewer_coverage = coverage;
            }
            Ok(result)
        }
    }
}
