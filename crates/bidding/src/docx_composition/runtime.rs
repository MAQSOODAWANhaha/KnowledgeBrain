//! Production execution on the shared authoring lease, object registry and
//! terminal ledger. Object I/O is supplied by the worker's owned child helpers.
use super::{agent, postgres};
use crate::{
    agent_error::{AgentError, RetryDisposition},
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
    let receipt = postgres::replay_receipt(pool, &job.request).await?;
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
        verify(
            io,
            r["manifest"]["sha256"]
                .as_str()
                .ok_or_else(|| invalid("manifest digest missing"))?,
            r["manifest"]["byte_length"]
                .as_u64()
                .ok_or_else(|| invalid("manifest length missing"))?,
            cancel,
        )
        .await?;
    }
    Ok(receipt)
}

pub async fn execute(
    pool: &PgPool,
    job: &DocxComposeJobV2,
    cancel: &CancellationToken,
    io: &dyn ObjectIo,
    cleanup: &StagedObjectCleanupTracker,
) -> Result<Value, AgentError> {
    execute_with_model(pool, job, cancel, io, cleanup, &agent::ConfiguredModel).await
}

pub async fn execute_with_model<M: agent::Model>(
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
        let result = async {
            let prepared = postgres::load_request(pool, &job.request).await?;
            let journal = postgres::PgJournal {
                pool,
                request: &job.request,
                owner: &owner,
            };
            agent::run(
                &prepared.input,
                &prepared.analysis,
                &prepared.request.config,
                &journal,
                model,
                &local,
            )
            .await?;
            let publication = postgres::prepare_publication(pool, &job.request).await?;
            let docx_stage = Uuid::new_v4();
            let manifest_stage = Uuid::new_v4();
            for (stage, sha, media, bytes) in [
                (
                    docx_stage,
                    publication.docx.sha256(),
                    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                    publication.docx.bytes(),
                ),
                (
                    manifest_stage,
                    publication.manifest_sha256.as_str(),
                    "application/json",
                    publication.manifest.as_slice(),
                ),
            ] {
                if local.is_cancelled() {
                    return Err(cancelled());
                }
                // Register BEFORE SQL, so even a lost staging ACK is cleaned.
                cleanup.register(stage);
                platform::stage_object_upload(
                    pool,
                    stage,
                    &platform::object_ref(sha),
                    sha,
                    media,
                    i64::try_from(bytes.len()).map_err(invalid)?,
                    &publication.actor,
                )
                .await
                .map_err(db_error)?;
                io.write(sha, bytes, &local).await?;
                verify(io, sha, bytes.len() as u64, &local).await?;
            }
            if local.is_cancelled() {
                return Err(cancelled());
            }
            let receipt = postgres::commit_publication(
                pool,
                &job.request,
                &owner,
                &publication,
                docx_stage,
                manifest_stage,
            )
            .await?;
            cleanup.disarm(docx_stage);
            cleanup.disarm(manifest_stage);
            Ok(receipt)
        }
        .await;
        finished.cancel();
        result
    };
    // Heartbeat errors cancel cooperative work, then join it. In particular,
    // never drop a live object helper before its kill/reap and cleanup handoff.
    let heartbeat = async {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let deadline = tokio::time::sleep(std::time::Duration::from_secs(45 * 60));
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
    if error.disposition == RetryDisposition::Obsolete {
        return Ok(json!({"disposition":"obsolete"}));
    }
    let sql = if error.disposition == RetryDisposition::Transient {
        "SELECT kb_bid_v2_tender_agent_yield_for_retry($1,$2::kb_sha256,$3,$4,$5,$6)"
    } else {
        "SELECT kb_bid_v2_tender_agent_fail($1,$2::kb_sha256,$3,$4,$5,$6)"
    };
    let recorded = sqlx::query(sql)
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
        Err(e) if e.disposition == RetryDisposition::Obsolete => {
            Ok(json!({"disposition":"obsolete"}))
        }
        Err(e) => Err(e),
        Ok(_) if error.disposition == RetryDisposition::Transient => Err(error),
        Ok(_) => Ok(json!({"status":"failed","error_code":error.code})),
    }
}
