//! 用户触发的填章：跑在 `docx_compose` 请求轨道上的**分析侧 Fill 合同**。
//!
//! 与终稿编制共用请求、worker 与出稿的对象登记，但执行体完全不同：正文的作者是
//! 分析 agent（`put_chapter_template`），依据是用户保存的那份 Word 回读出来的章
//! 树。回读结果是**输入**——请求里冻着它的摘要，执行时用同一份字节重算，SQL 用摘
//! 要核对第一个检查点，因此「种子」不可能被模型或宿主偷换。
use super::CompositionMode;
use crate::{
    agent_error::AgentError,
    docx_round::{DocxRoundBasis, DocxVersionIdentity},
    tender_analysis::{
        AnalysisResult, FrozenInput, agent::Config, digest, draft::DraftPlanItem, readback,
    },
};
use platform::BidAuthoringRequestIdentityV2;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenFillRequest {
    pub schema_version: u8,
    pub workspace_id: Uuid,
    pub actor: String,
    pub basis: DocxRoundBasis,
    /// 填章必须绑定一个具体的当前版本：填的是**这一份**用户编辑过的 Word。
    pub expected: Option<DocxVersionIdentity>,
    pub mode: CompositionMode,
    pub seed_plan_sha256: Option<String>,
    pub source_request: BidAuthoringRequestIdentityV2,
    pub source_input_sha256: String,
    pub analysis_sha256: String,
    pub config: Config,
    pub contract_sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    source_request: BidAuthoringRequestIdentityV2,
    input: FrozenInput,
    analysis: AnalysisResult,
}

/// 一次填充 run 的全部输入：冻结的请求、原始来源，以及回读重建出的章树。
pub struct PreparedFill {
    pub request: FrozenFillRequest,
    pub input: FrozenInput,
    pub analysis: AnalysisResult,
    pub seed: Vec<DraftPlanItem>,
}

fn invalid(e: impl std::fmt::Display) -> AgentError {
    AgentError::new("AGENT_OUTPUT_INVALID", e.to_string())
}

fn changed(message: &str) -> AgentError {
    AgentError::new("FROZEN_INPUT_DIGEST_MISMATCH", message)
}

fn db_error(error: sqlx::Error) -> AgentError {
    crate::tender_analysis::postgres::db_error(error)
}

/// 回读当前 Word 并重建章树。纯函数（除了 docreader 这一次解析）：同一份字节永远
/// 得到同一棵树，所以它的摘要可以当请求身份的一部分。
pub async fn read_seed(
    input: &FrozenInput,
    analysis: &AnalysisResult,
    docx: Vec<u8>,
    cancel: &CancellationToken,
) -> Result<Vec<DraftPlanItem>, AgentError> {
    let (sections, titles) = readback::claim_basis(input, analysis).map_err(invalid)?;
    let document = readback::read_saved_docx(docx, &sections, &titles, cancel)
        .await
        .map_err(invalid)?;
    if !document.not_checked.is_empty() {
        return Err(AgentError::new("DOCX_FILL_UNSUPPORTED_CONTENT", format!("Saved document cannot be preserved: {}", document.not_checked.join("; "))));
    }
    let seed = readback::seed_plan(&document, &analysis.analysis.draft_plan);
    if seed.is_empty() {
        return Err(invalid("read-back document has no chapter to fill"));
    }
    Ok(seed)
}

impl FrozenFillRequest {
    pub fn sha256(&self) -> Result<String, AgentError> {
        digest(self).map_err(invalid)
    }

    fn validate(&self, source: &Source, seed: &[DraftPlanItem]) -> Result<(), AgentError> {
        self.source_request.validate().map_err(invalid)?;
        if self.schema_version != 1
            || self.workspace_id.is_nil()
            || self.mode != CompositionMode::DraftFill
            || self.expected.is_none()
            || self.source_request != source.source_request
            || self.source_input_sha256 != digest(&source.input).map_err(invalid)?
            || self.analysis_sha256 != digest(&source.analysis).map_err(invalid)?
            || self.contract_sha256
                != digest(&self.config.contract_definition()).map_err(invalid)?
            || self.seed_plan_sha256.as_deref() != Some(digest(&seed).map_err(invalid)?.as_str())
            || source.input.document_set_id != self.basis.document_set_id.to_string()
        {
            return Err(changed("fill source, runtime or read-back seed changed"));
        }
        self.config.validate()?;
        if !self.config.limits.draft_path {
            return Err(invalid("fill request requires the draft path"));
        }
        // 官方编制拒绝 draft，填章反过来**必须**是 draft：填的是草稿。
        super::validate_draft_basis(&source.input, &source.analysis).map_err(invalid)
    }
}

async fn load_source(
    pool: &PgPool,
    workspace_id: Uuid,
    basis: &DocxRoundBasis,
    actor: &str,
) -> Result<Source, AgentError> {
    let sqlx::types::Json(source): sqlx::types::Json<Source> = sqlx::query_scalar(
        "SELECT kb_bid_v2_load_docx_composition_source($1,$2,$3::kb_actor_identity,$4)",
    )
    .bind(workspace_id)
    .bind(sqlx::types::Json(basis))
    .bind(actor)
    .bind("draft-fill")
    .fetch_one(pool)
    .await
    .map_err(db_error)?;
    Ok(source)
}

/// 这次填充自己的额度：回合按**待填章数**推导（与阶段一的 20 回合门无关），编译
/// 上限按整份正文放大。额度冻在请求里，SQL 的 reserve 闸门照它记账。
fn fill_config(config: Config, seed: &[DraftPlanItem]) -> Result<Config, AgentError> {
    use crate::tender_analysis::draft::{DraftStatus, fill_limits};
    let pending = seed
        .iter()
        .filter(|item| item.status == DraftStatus::Pending)
        .count();
    Config::with_provider(
        config.provider.clone(),
        fill_limits(config.limits.clone(), pending),
    )
}

/// 一次填充请求的用户侧输入：填哪个工作区、哪套依据、哪一版 Word，以及那一版的字节。
pub struct FillIntent {
    pub workspace_id: Uuid,
    pub basis: DocxRoundBasis,
    pub expected: Option<DocxVersionIdentity>,
    pub actor: String,
    pub docx: Vec<u8>,
}

/// 从用户当前选中的来源与 DOCX 版本冻结一次填充请求。回读在这里做：解析不通过
/// 就当场失败，用户马上知道「这份文档读不出章」，而不是排队之后再失败。
pub async fn prepare(
    pool: &PgPool,
    intent: FillIntent,
    config: Config,
    cancel: &CancellationToken,
) -> Result<PreparedFill, AgentError> {
    let FillIntent {
        workspace_id,
        basis,
        expected,
        actor,
        docx,
    } = intent;
    let actor = actor.as_str();
    let sqlx::types::Json(source): sqlx::types::Json<Source> = sqlx::query_scalar(
        "SELECT kb_bid_v2_prepare_docx_composition_source($1,$2,$3,$4::kb_actor_identity,$5)",
    )
    .bind(workspace_id)
    .bind(sqlx::types::Json(&basis))
    .bind(sqlx::types::Json(&expected))
    .bind(actor)
    .bind("draft-fill")
    .fetch_one(pool)
    .await
    .map_err(db_error)?;
    let seed = read_seed(&source.input, &source.analysis, docx, cancel).await?;
    let config = fill_config(config, &seed)?;
    let request = FrozenFillRequest {
        schema_version: 1,
        workspace_id,
        actor: actor.into(),
        basis,
        expected,
        mode: CompositionMode::DraftFill,
        seed_plan_sha256: Some(digest(&seed).map_err(invalid)?),
        source_request: source.source_request.clone(),
        source_input_sha256: digest(&source.input).map_err(invalid)?,
        analysis_sha256: digest(&source.analysis).map_err(invalid)?,
        contract_sha256: digest(&config.contract_definition()).map_err(invalid)?,
        config,
    };
    request.validate(&source, &seed)?;
    Ok(PreparedFill {
        request,
        input: source.input,
        analysis: source.analysis,
        seed,
    })
}

/// 按持久化的请求摘要与原始来源恢复。种子由**同一份冻结字节**重算：用户此后又
/// 改了 Word 也不影响这次 Job，它填的还是当初那一份。
pub async fn restore(
    pool: &PgPool,
    request: FrozenFillRequest,
    expected_request_sha256: &str,
    docx: Vec<u8>,
    cancel: &CancellationToken,
) -> Result<PreparedFill, AgentError> {
    if request.sha256()? != expected_request_sha256 {
        return Err(changed("fill request snapshot changed"));
    }
    let source = load_source(pool, request.workspace_id, &request.basis, &request.actor).await?;
    let seed = read_seed(&source.input, &source.analysis, docx, cancel).await?;
    request.validate(&source, &seed)?;
    Ok(PreparedFill {
        request,
        input: source.input,
        analysis: source.analysis,
        seed,
    })
}

pub async fn create_request(
    pool: &PgPool,
    request: &FrozenFillRequest,
    idempotency_key: &str,
) -> Result<serde_json::Value, AgentError> {
    sqlx::query_scalar(
        "SELECT kb_bid_v2_create_docx_composition_request($1,$2,$3,$4::kb_actor_identity,$5)",
    )
    .bind(Uuid::new_v4())
    .bind(sqlx::types::Json(request))
    .bind(sqlx::types::Json(request.config.contract_definition()))
    .bind(&request.actor)
    .bind(idempotency_key)
    .fetch_one(pool)
    .await
    .map_err(db_error)
}

pub async fn load_request(
    pool: &PgPool,
    identity: &BidAuthoringRequestIdentityV2,
) -> Result<FrozenFillRequest, AgentError> {
    identity.validate().map_err(invalid)?;
    let snapshot: Option<sqlx::types::Json<FrozenFillRequest>> =
        sqlx::query_scalar("SELECT kb_bid_v2_load_docx_composition_request($1,$2,$3::kb_sha256)")
            .bind(identity.request_artifact_id)
            .bind(identity.request_revision)
            .bind(&identity.frozen_input_sha256)
            .fetch_one(pool)
            .await
            .map_err(db_error)?;
    let request = snapshot
        .ok_or_else(|| AgentError::new("FROZEN_INPUT_MISSING", "fill request missing"))?
        .0;
    if request.sha256()? != identity.frozen_input_sha256 {
        return Err(changed("fill request snapshot changed"));
    }
    Ok(request)
}

/// 出稿：把 checkpoint 里那份整篇重编译的 DOCX 发布成新版本。没有 composition
/// manifest，也没有 32 项复核——它还是草稿；编辑器会话没关时 SQL 会拒绝。
pub async fn publish_staged(
    pool: &PgPool,
    identity: &BidAuthoringRequestIdentityV2,
    owner: &crate::bid_authoring_v2::AgentRunLease,
    checkpoint_sha256: &str,
    docx_staging: Uuid,
) -> Result<serde_json::Value, AgentError> {
    sqlx::query_scalar(
        "SELECT kb_bid_v2_publish_docx_fill($1,$2::kb_sha256,$3,$4,$5::kb_sha256,$6)",
    )
    .bind(identity.request_artifact_id)
    .bind(&identity.frozen_input_sha256)
    .bind(owner.attempt)
    .bind(owner.execution_owner_token)
    .bind(checkpoint_sha256)
    .bind(docx_staging)
    .fetch_one(pool)
    .await
    .map_err(db_error)
}

pub async fn replay_publication(
    pool: &PgPool,
    identity: &BidAuthoringRequestIdentityV2,
) -> Result<Option<serde_json::Value>, AgentError> {
    identity.validate().map_err(invalid)?;
    sqlx::query_scalar("SELECT kb_bid_v2_replay_docx_fill($1,$2::kb_sha256)")
        .bind(identity.request_artifact_id)
        .bind(&identity.frozen_input_sha256)
        .fetch_one(pool)
        .await
        .map_err(db_error)
}
