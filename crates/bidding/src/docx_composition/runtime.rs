//! Production execution on the shared authoring lease, object registry and
//! terminal ledger. Object I/O is supplied by the worker's owned child helpers.
use super::postgres;
use crate::{
    agent_error::{AgentError, RequestQueueEffect},
    bid_authoring_v2::AgentRunLease,
};
use async_trait::async_trait;
use platform::{DocxComposeJobV2, StagedObjectCleanupTracker};
use postgres::db_error;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[async_trait]
pub trait ObjectIo: Send + Sync {
    /// Implementations must finish or stop owned physical work before returning.
    async fn read(
        &self,
        sha256: &str,
        max_bytes: usize,
        cancel: &CancellationToken,
    ) -> Result<Vec<u8>, AgentError>;
    async fn write(
        &self,
        sha256: &str,
        bytes: &[u8],
        cancel: &CancellationToken,
    ) -> Result<(), AgentError>;
}
fn invalid(e: impl std::fmt::Display) -> AgentError {
    AgentError::new("AGENT_OUTPUT_INVALID", e.to_string())
}
fn cancelled() -> AgentError {
    AgentError::new("INTERNAL", "composition cancelled")
}

async fn verify(
    io: &dyn ObjectIo,
    sha: &str,
    length: u64,
    cancel: &CancellationToken,
) -> Result<(), AgentError> {
    if sha.len() != 64
        || !sha
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err(invalid("invalid object digest"));
    }
    let bytes = io
        .read(sha, usize::try_from(length).map_err(invalid)?, cancel)
        .await?;
    if bytes.len() as u64 != length || hex::encode(Sha256::digest(&bytes)) != sha {
        return Err(invalid(
            "published object bytes do not match reviewed identity",
        ));
    }
    Ok(())
}
async fn replay(
    pool: &PgPool,
    job: &DocxComposeJobV2,
    io: &dyn ObjectIo,
    cancel: &CancellationToken,
) -> Result<Option<Value>, AgentError> {
    let receipt = super::fill::replay_publication(pool, &job.request).await?;
    if let Some(r) = &receipt {
        verify(
            io,
            r["docx_sha256"]
                .as_str()
                .ok_or_else(|| invalid("DOCX digest missing"))?,
            r["byte_length"]
                .as_u64()
                .ok_or_else(|| invalid("DOCX length missing"))?,
            cancel,
        )
        .await?;
    }
    Ok(receipt)
}

/// 填章执行体：回读当前 Word → 分析侧 Fill 合同 → 整篇重编译 → 发布新版本。
async fn fill_work<M: crate::tender_analysis::agent::Model>(
    pool: &PgPool,
    job: &DocxComposeJobV2,
    owner: &AgentRunLease,
    io: &dyn ObjectIo,
    cleanup: &StagedObjectCleanupTracker,
    model: &M,
    cancel: &CancellationToken,
) -> Result<Value, AgentError> {
    use base64::Engine as _;
    let request = super::fill::load_request(pool, &job.request).await?;
    let expected = request
        .expected
        .clone()
        .ok_or_else(|| invalid("fill request does not name the DOCX it fills"))?;
    let ceiling = request.config.limits.max_draft_docx_bytes;
    let current = io.read(&expected.docx_sha256, ceiling, cancel).await?;
    let prepared = super::fill::restore(
        pool,
        request,
        &job.request.frozen_input_sha256,
        current,
        cancel,
    )
    .await?;
    let journal = crate::tender_analysis::postgres::PgJournal {
        pool,
        request: &job.request,
        owner,
        source_reader: None,
    };
    // 写作窗比信封窗短：到点只停「写」，编译、登记与入稿仍在信封里完成，所以到期
    // 也有一份带已填章的 Word，而不是零产物。外层取消（worker 关停）照旧穿透。
    let remaining: i64 = sqlx::query_scalar(
        "SELECT greatest(0, floor(extract(epoch from (kb_bid_v2_tender_agent_frozen_deadline($1)-clock_timestamp()))))::bigint"
    ).bind(job.request.request_artifact_id).fetch_one(pool).await.map_err(db_error)?;
    let writing_secs = (remaining.max(0) as u64)
        .saturating_sub(prepared.request.config.budget.publish_reserve_secs)
        .min(crate::tender_analysis::draft::FILL_DEADLINE_SECS);
    let writing = cancel.child_token();
    if writing_secs == 0 {
        return Err(AgentError::new(
            "AGENT_DEADLINE_EXCEEDED",
            "no writing time remains; current document unchanged",
        ));
    }
    let stop_writing = writing.clone();
    let window = tokio::spawn(async move {
        tokio::select! {
            biased;
            _ = stop_writing.cancelled() => {}
            _ = tokio::time::sleep(std::time::Duration::from_secs(
                writing_secs)) => stop_writing.cancel(),
        }
    });
    let run = crate::tender_analysis::agent::run_fill(
        &prepared.input,
        &prepared.request.config,
        &journal,
        model,
        &writing,
        prepared.seed,
        prepared.analysis.analysis.outline,
    )
    .await;
    writing.cancel();
    window.abort();
    run?;
    // 出稿字节只认持久化的检查点：内存里的产物不能绕过 checkpoint 攻证。
    let checkpoint = crate::tender_analysis::agent::Journal::load(&journal)
        .await?
        .ok_or_else(|| invalid("finished fill run has no checkpoint"))?;
    let docx = base64::engine::general_purpose::STANDARD
        .decode(
            checkpoint
                .draft_docx_base64
                .as_deref()
                .ok_or_else(|| invalid("fill run compiled no DOCX"))?,
        )
        .map_err(invalid)?;
    let sha = hex::encode(Sha256::digest(&docx));
    let stage = Uuid::new_v4();
    if cancel.is_cancelled() {
        return Err(cancelled());
    }
    cleanup.register(stage);
    platform::stage_object_upload(
        pool,
        stage,
        &platform::object_ref(&sha),
        &sha,
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        i64::try_from(docx.len()).map_err(invalid)?,
        &prepared.request.actor,
    )
    .await
    .map_err(db_error)?;
    io.write(&sha, &docx, cancel).await?;
    verify(io, &sha, docx.len() as u64, cancel).await?;
    if cancel.is_cancelled() {
        return Err(cancelled());
    }
    let receipt = super::fill::publish_staged(
        pool,
        &job.request,
        owner,
        &crate::tender_analysis::digest(&checkpoint).map_err(invalid)?,
        stage,
    )
    .await?;
    cleanup.disarm(stage);
    Ok(receipt)
}

pub async fn execute(
    pool: &PgPool,
    job: &DocxComposeJobV2,
    cancel: &CancellationToken,
    io: &dyn ObjectIo,
    cleanup: &StagedObjectCleanupTracker,
) -> Result<Value, AgentError> {
    execute_fill_with_model(
        pool,
        job,
        cancel,
        io,
        cleanup,
        &crate::tender_analysis::agent::ConfiguredModel,
    )
    .await
}

/// 用户触发的填章：分析侧 Fill 合同，出稿只有整篇重编译的 DOCX。
pub async fn execute_fill_with_model<M: crate::tender_analysis::agent::Model>(
    pool: &PgPool,
    job: &DocxComposeJobV2,
    cancel: &CancellationToken,
    io: &dyn ObjectIo,
    cleanup: &StagedObjectCleanupTracker,
    model: &M,
) -> Result<Value, AgentError> {
    job.request.validate().map_err(invalid)?;
    // Attest every transported scope before claiming; a forged payload cannot
    // consume another request's attempt budget or terminally fail its work.
    let payload: Option<Value> =
        sqlx::query_scalar("SELECT kb_bid_v2_load_docx_composition_job($1,$2,$3::kb_sha256)")
            .bind(job.request.request_artifact_id)
            .bind(job.request.request_revision)
            .bind(&job.request.frozen_input_sha256)
            .fetch_one(pool)
            .await
            .map_err(db_error)?;
    let expected = json!({"job_kind":"docx_compose","request":job.request,"project_id":job.project_id,"workspace_id":job.workspace_id});
    if payload.as_ref() != Some(&expected) {
        return Err(AgentError::new(
            "FROZEN_INPUT_DIGEST_MISMATCH",
            "composition job scope changed",
        ));
    }
    if let Some(receipt) = replay(pool, job, io, cancel).await? {
        return Ok(receipt);
    }
    if cancel.is_cancelled() {
        return Err(cancelled());
    }
    let claim: Value =
        sqlx::query_scalar("SELECT kb_bid_v2_tender_agent_claim($1,$2,$3::kb_sha256)")
            .bind(job.request.request_artifact_id)
            .bind(job.request.request_revision)
            .bind(&job.request.frozen_input_sha256)
            .fetch_one(pool)
            .await
            .map_err(db_error)?;
    match claim["disposition"].as_str() {
        Some("obsolete" | "live_owner" | "exhausted") => {
            // Publication may have committed between the initial read and claim.
            return Ok(replay(pool, job, io, cancel).await?.unwrap_or(claim));
        }
        Some("claimed") => {}
        _ => return Err(invalid("unknown composition claim disposition")),
    }
    let owner = AgentRunLease {
        attempt: claim["attempt"]
            .as_i64()
            .and_then(|n| i32::try_from(n).ok())
            .ok_or_else(|| invalid("attempt missing"))?,
        max_attempts: claim["max_attempts"]
            .as_i64()
            .and_then(|n| i32::try_from(n).ok())
            .ok_or_else(|| invalid("attempt budget missing"))?,
        execution_owner_token: Uuid::parse_str(
            claim["execution_owner_token"]
                .as_str()
                .ok_or_else(|| invalid("owner missing"))?,
        )
        .map_err(invalid)?,
    };
    let local = cancel.child_token();
    let finished = CancellationToken::new();
    let work = async {
        let result = fill_work(pool, job, &owner, io, cleanup, model, &local).await;
        finished.cancel();
        result
    };
    // Heartbeat errors cancel cooperative work, then join it. In particular,
    // never drop a live object helper before its kill/reap and cleanup handoff.
    let remaining =
        crate::tender_analysis::postgres::claim_budget_secs(&claim["hard_deadline_at"])?;
    let heartbeat = async {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let deadline = tokio::time::sleep(std::time::Duration::from_secs(remaining));
        tokio::pin!(deadline);
        loop {
            let error = tokio::select! {
                biased;
                _=finished.cancelled()=>return None,
                _=&mut deadline=>Some(AgentError::new("AGENT_DEADLINE_EXCEEDED","composition deadline reached")),
                _=local.cancelled()=>Some(cancelled()),
                _=interval.tick()=>{
                    let beat=sqlx::query("SELECT kb_bid_v2_tender_agent_heartbeat($1,$2::kb_sha256,$3,$4)")
                        .bind(job.request.request_artifact_id).bind(&job.request.frozen_input_sha256).bind(owner.attempt).bind(owner.execution_owner_token).execute(pool);
                    tokio::select! {
                        biased;
                        _=finished.cancelled()=>return None,
                        _=local.cancelled()=>Some(cancelled()),
                        _=&mut deadline=>Some(AgentError::new("AGENT_DEADLINE_EXCEEDED","composition heartbeat deadline reached")),
                        r=beat=>r.err().map(db_error),
                    }
                }
            };
            if let Some(error) = error {
                local.cancel();
                return Some(error);
            }
        }
    };
    let (result, heartbeat_error) = tokio::join!(work, heartbeat);
    if result.is_ok() {
        return result;
    }
    // Lost publication ACK must resolve through the immutable receipt before any
    // failure effect. This also covers a heartbeat racing a successful commit.
    if let Some(receipt) = replay(pool, job, io, cancel).await? {
        return Ok(receipt);
    }
    let error = heartbeat_error.unwrap_or_else(|| result.unwrap_err());
    match error.request_queue_effect() {
        RequestQueueEffect::AckObsolete => Ok(json!({"disposition":"obsolete"})),
        RequestQueueEffect::RetryUnchanged => Err(error),
        RequestQueueEffect::ReleaseThenRetry => {
            let _ = sqlx::query(
                "SELECT kb_bid_v2_tender_agent_yield_for_retry($1,$2::kb_sha256,$3,$4,$5,$6)",
            )
            .bind(job.request.request_artifact_id)
            .bind(&job.request.frozen_input_sha256)
            .bind(owner.attempt)
            .bind(owner.execution_owner_token)
            .bind("INTERNAL")
            .bind(&error.message)
            .execute(pool)
            .await;
            Err(error)
        }
        RequestQueueEffect::YieldThenRetry => {
            sqlx::query(
                "SELECT kb_bid_v2_tender_agent_yield_for_retry($1,$2::kb_sha256,$3,$4,$5,$6)",
            )
            .bind(job.request.request_artifact_id)
            .bind(&job.request.frozen_input_sha256)
            .bind(owner.attempt)
            .bind(owner.execution_owner_token)
            .bind(&error.code)
            .bind(&error.message)
            .execute(pool)
            .await
            .map_err(db_error)?;
            Err(error)
        }
        RequestQueueEffect::FailRequest => {
            let recorded =
                sqlx::query("SELECT kb_bid_v2_tender_agent_fail($1,$2::kb_sha256,$3,$4,$5,$6)")
                    .bind(job.request.request_artifact_id)
                    .bind(&job.request.frozen_input_sha256)
                    .bind(owner.attempt)
                    .bind(owner.execution_owner_token)
                    .bind(&error.code)
                    .bind(&error.message)
                    .execute(pool)
                    .await
                    .map_err(db_error);
            match recorded {
                Ok(_) => Ok(json!({"status":"failed","error_code":error.code})),
                Err(e) if e.request_queue_effect() == RequestQueueEffect::AckObsolete => {
                    Ok(json!({"disposition":"obsolete"}))
                }
                Err(e) => Err(e),
            }
        }
    }
}
