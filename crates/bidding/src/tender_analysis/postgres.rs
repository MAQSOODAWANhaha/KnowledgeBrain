//! PostgreSQL-backed Agent journal. Model calls and checkpoint publication are
//! fenced by the same attempt-independent request identity as other authoring work.
use super::{
    agent::{self, Checkpoint, Config, ConfiguredModel, Journal, Role},
    *,
};
use crate::{
    agent_error::{AgentError, RequestQueueEffect},
    bid_authoring_v2::AgentRunLease,
};
use async_trait::async_trait;
use platform::BidAuthoringRequestIdentityV2;
use serde_json::json;
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub struct PgJournal<'a> {
    pub pool: &'a PgPool,
    pub request: &'a BidAuthoringRequestIdentityV2,
    pub owner: &'a AgentRunLease,
    pub source_reader: Option<&'a dyn crate::tender_process::TenderObjectReader>,
}

fn is_lock_timeout(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|database| database.code())
        .is_some_and(|code| code == "55P03")
}

async fn heartbeat_once(
    pool: &PgPool,
    request: &BidAuthoringRequestIdentityV2,
    owner: &AgentRunLease,
) -> Result<(), AgentError> {
    let mut conn = pool.acquire().await.map_err(db_error)?;
    sqlx::query("SET lock_timeout = '2s'")
        .execute(&mut *conn)
        .await
        .map_err(db_error)?;
    let beat = sqlx::query("SELECT kb_bid_v2_tender_agent_heartbeat($1,$2::kb_sha256,$3,$4)")
        .bind(request.request_artifact_id)
        .bind(&request.frozen_input_sha256)
        .bind(owner.attempt)
        .bind(owner.execution_owner_token)
        .execute(&mut *conn)
        .await;
    let _ = sqlx::query("SET lock_timeout = '0'")
        .execute(&mut *conn)
        .await;
    match beat {
        Ok(_) => Ok(()),
        Err(error) if is_lock_timeout(&error) => Ok(()),
        Err(error) => Err(db_error(error)),
    }
}

pub fn db_error(e: sqlx::Error) -> AgentError {
    if let Some(db) = e.as_database_error() {
        let code = db.message().split([':', ';']).next().unwrap_or("");
        if matches!(
            code,
            "REQUEST_OBSOLETE"
                | "REQUEST_ATTEMPT_SUPERSEDED"
                | "AGENT_TURN_BUDGET_EXCEEDED"
                | "AGENT_PROVIDER_UNAVAILABLE"
                | "FROZEN_INPUT_DIGEST_MISMATCH"
        ) {
            return AgentError::new(code, db.message());
        }
        if db.code().is_some_and(|c| {
            c.starts_with("08") || c.starts_with("40") || c.starts_with("53") || c.starts_with("57")
        }) {
            return AgentError::new("INTERNAL", e.to_string());
        }
        return AgentError::new("AGENT_OUTPUT_INVALID", e.to_string());
    }
    match e {
        sqlx::Error::Io(_)
        | sqlx::Error::PoolTimedOut
        | sqlx::Error::PoolClosed
        | sqlx::Error::Tls(_) => AgentError::new("INTERNAL", e.to_string()),
        _ => AgentError::new("AGENT_OUTPUT_INVALID", e.to_string()),
    }
}
fn invalid(e: impl std::fmt::Display) -> AgentError {
    AgentError::new("AGENT_OUTPUT_INVALID", e.to_string())
}

#[async_trait]
impl Journal for PgJournal<'_> {
    async fn source_view(
        &self,
        source_id: &str,
        limits: &agent::Limits,
        cancel: &CancellationToken,
    ) -> Result<views::SourceView, AgentError> {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        use sha2::{Digest, Sha256};
        let reader = self.source_reader.ok_or_else(|| {
            AgentError::new(
                "SOURCE_VIEW_UNAVAILABLE",
                "original object reader unavailable",
            )
        })?;
        let original: Value = sqlx::query_scalar(
            "SELECT kb_bid_v2_tender_source_view_input($1,$2::kb_sha256,$3,$4,$5)",
        )
        .bind(self.request.request_artifact_id)
        .bind(&self.request.frozen_input_sha256)
        .bind(self.owner.attempt)
        .bind(self.owner.execution_owner_token)
        .bind(Uuid::parse_str(source_id).map_err(invalid)?)
        .fetch_one(self.pool)
        .await
        .map_err(db_error)?;
        let sha = original["sha256"]
            .as_str()
            .ok_or_else(|| invalid("original digest missing"))?;
        let media = original["media_type"]
            .as_str()
            .ok_or_else(|| invalid("original media type missing"))?;
        let page = if media == "application/pdf" {
            original["locator"]["page_ordinal"]
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| {
                    AgentError::new(
                        "SOURCE_VIEW_UNAVAILABLE",
                        "PDF source has no physical page locator",
                    )
                })?
        } else if matches!(media, "image/png" | "image/jpeg" | "image/webp") {
            0
        } else {
            return Err(AgentError::new(
                "SOURCE_VIEW_UNAVAILABLE",
                "Office source has structural locations; original-page rendering is not available",
            ));
        };
        let bytes = reader
            .read(sha, cancel)
            .await
            .map_err(|e| AgentError::new("INTERNAL", e.to_string()))?;
        if Some(bytes.len() as u64) != original["byte_length"].as_u64()
            || hex::encode(Sha256::digest(&bytes)) != sha
        {
            return Err(AgentError::new(
                "FROZEN_INPUT_DIGEST_MISMATCH",
                "original object bytes changed",
            ));
        }
        let result = docparser::source_view(docparser::proto::SourceViewRequest {
            file_content: bytes,
            media_type: media.into(),
            source_sha256: sha.into(),
            page_ordinal: page,
            max_edge: limits.max_source_view_edge,
            max_image_bytes: limits.max_source_view_bytes as u64,
        })
        .await
        .map_err(|e| AgentError::new("SOURCE_VIEW_UNAVAILABLE", e.to_string()))?;
        if result.source_sha256 != sha
            || result.page_ordinal != page
            || result.media_type != "image/jpeg"
        {
            return Err(invalid("service returned a different original/page/format"));
        }
        Ok(views::SourceView {
            identity: views::ViewIdentity {
                source_id: source_id.into(),
                original_sha256: result.source_sha256,
                image_sha256: result.image_sha256,
                page_ordinal: result.page_ordinal,
                width: result.width,
                height: result.height,
                renderer: result.renderer,
            },
            jpeg_base64: STANDARD.encode(result.image_data),
        })
    }
    async fn load(&self) -> Result<Option<Checkpoint>, AgentError> {
        let v: Option<Value> =
            sqlx::query_scalar("SELECT kb_bid_v2_tender_agent_checkpoint_get($1,$2::kb_sha256)")
                .bind(self.request.request_artifact_id)
                .bind(&self.request.frozen_input_sha256)
                .fetch_one(self.pool)
                .await
                .map_err(db_error)?;
        v.map(serde_json::from_value).transpose().map_err(invalid)
    }
    async fn reserve(&self, state: &Checkpoint, body: &[u8]) -> Result<Option<usize>, AgentError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let value: Option<i32> = sqlx::query_scalar(
            "SELECT kb_bid_v2_tender_agent_reserve($1,$2::kb_sha256,$3,$4,$5,$6,$7)",
        )
        .bind(self.request.request_artifact_id)
        .bind(&self.request.frozen_input_sha256)
        .bind(self.owner.attempt)
        .bind(self.owner.execution_owner_token)
        .bind(i32::try_from(state.turn).map_err(invalid)?)
        .bind(if state.role == Role::Main {
            "main"
        } else {
            "reviewer"
        })
        .bind(body)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_error)?;
        if value.is_some() {
            sqlx::query(
                "SELECT kb_bid_v2_tender_agent_checkpoint_put($1,$2::kb_sha256,$3,$4,$5,$6)",
            )
            .bind(self.request.request_artifact_id)
            .bind(&self.request.frozen_input_sha256)
            .bind(self.owner.attempt)
            .bind(self.owner.execution_owner_token)
            .bind(serde_json::to_value(state).map_err(invalid)?)
            .bind(json!({
                "phase": if state.role == Role::Main { "main" } else { "reviewer" },
                "turn": state.turn,
                "tool_calls": state.tool_calls,
                "review_rounds": state.review_rounds,
                "records": state.analysis.records.len(),
                "checkpoint_sequence": state.journal.sequence,
                "boundary": "prepared",
            }))
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        }
        tx.commit().await.map_err(db_error)?;
        value
            .map(|v| usize::try_from(v).map_err(invalid))
            .transpose()
    }
    async fn save(&self, state: &Checkpoint, progress: &Value) -> Result<(), AgentError> {
        sqlx::query("SELECT kb_bid_v2_tender_agent_checkpoint_put($1,$2::kb_sha256,$3,$4,$5,$6)")
            .bind(self.request.request_artifact_id)
            .bind(&self.request.frozen_input_sha256)
            .bind(self.owner.attempt)
            .bind(self.owner.execution_owner_token)
            .bind(serde_json::to_value(state).map_err(invalid)?)
            .bind(progress)
            .execute(self.pool)
            .await
            .map_err(db_error)?;
        Ok(())
    }
}

pub fn publication(input: &FrozenInput, result: &AnalysisResult) -> Result<Value, AgentError> {
    let mut requirements = Vec::new();
    for record in result.analysis.records.values() {
        if let RecordData::Requirement {
            text,
            categories,
            strength,
            compliance,
            applicability,
            criteria,
            proofs,
            response,
            ..
        } = &record.data
        {
            let mut spans = record.sources.clone();
            for span in applicability
                .grounds
                .iter()
                .chain(compliance.iter().flat_map(|c| &c.grounds))
                .chain(criteria.iter().flat_map(|c| &c.grounds))
                .chain(proofs.iter().flat_map(|p| &p.grounds))
                .chain(response.iter().flat_map(|r| &r.grounds))
            {
                if !spans.contains(span) {
                    spans.push(span.clone());
                }
            }
            let sources: std::collections::BTreeSet<_> =
                spans.iter().map(|s| s.source_id.clone()).collect();
            let cited_forms: std::collections::BTreeSet<_> = spans
                .iter()
                .filter_map(|span| span.grid_cell.as_ref().map(|c| c.form_id.clone()))
                .collect();
            // Non-text citations remain in the complete analysis. The legacy
            // quote table must never receive fabricated zero-length text quotes.
            spans.retain(|span| span.view_id.is_none() && span.grid_cell.is_none());
            let mut form_ids: Vec<_> = result
                .analysis
                .relations
                .values()
                .filter(|r| r.from == record.id && r.kind == RelationKind::RequiresTemplate)
                .filter_map(|r| result.analysis.records.get(&r.to))
                .flat_map(|r| match &r.data {
                    RecordData::Template { regions, .. } => {
                        regions.iter().filter_map(|r| r.form_id.clone()).collect()
                    }
                    _ => Vec::<String>::new(),
                })
                // Legacy projection is source-local; the full many-to-many
                // relationship remains in analysis_result without this filter.
                .filter(|id| {
                    input.structured_forms.iter().any(|f| {
                        f["form_definition_revision_id"] == *id
                            && sources
                                .contains(f["source_unit_revision_id"].as_str().unwrap_or_default())
                    })
                })
                .collect();
            for id in cited_forms {
                if !form_ids.contains(&id) {
                    form_ids.push(id);
                }
            }
            let policy = compliance
                .first()
                .filter(|first| compliance.iter().all(|claim| claim.policy == first.policy))
                .map_or(&Compliance::Unknown, |claim| &claim.policy);
            requirements.push(json!({"requirement_ref":digest(record).map_err(invalid)?,
                "requirement_kind":categories.first().ok_or_else(||invalid("requirement category missing"))?,
                // Repeated policies retain their distinct conditions in analysis;
                // different policy types cannot fit the legacy scalar.
                "requiredness":strength,"compliance_policy":policy,"requirement_text":text,
                "response_needs":response,"source_unit_revision_ids":sources,"source_spans":spans,
                "structured_form_revision_ids":form_ids,"applicability":{"status":match applicability.state {
                    ApplicabilityState::Applicable=>"required",ApplicabilityState::NotApplicable=>"not_applicable",
                    ApplicabilityState::Conditional=>"conditional",ApplicabilityState::Unknown=>"unknown",
                },"reason":applicability.condition,"source_unit_revision_ids":sources}}));
        }
    }
    Ok(
        json!({"schema_version":4,"source_unit_revision_ids":input.source_units.iter().map(|s|&s.source_unit_revision_id).collect::<Vec<_>>(),
        "requirements":requirements,"notices":[],"analysis_result":result}),
    )
}

pub async fn execute(
    pool: &PgPool,
    request: &BidAuthoringRequestIdentityV2,
    cancel: &CancellationToken,
    reader: &dyn crate::tender_process::TenderObjectReader,
) -> Result<Value, AgentError> {
    execute_with_model_and_reader(pool, request, cancel, &ConfiguredModel, Some(reader)).await
}

pub async fn execute_with_model<M: agent::Model>(
    pool: &PgPool,
    request: &BidAuthoringRequestIdentityV2,
    cancel: &CancellationToken,
    model: &M,
) -> Result<Value, AgentError> {
    execute_with_model_and_reader(pool, request, cancel, model, None).await
}

pub async fn execute_with_model_and_reader<M: agent::Model>(
    pool: &PgPool,
    request: &BidAuthoringRequestIdentityV2,
    cancel: &CancellationToken,
    model: &M,
    reader: Option<&dyn crate::tender_process::TenderObjectReader>,
) -> Result<Value, AgentError> {
    let claim: Value =
        sqlx::query_scalar("SELECT kb_bid_v2_tender_agent_claim($1,$2,$3::kb_sha256)")
            .bind(request.request_artifact_id)
            .bind(request.request_revision)
            .bind(&request.frozen_input_sha256)
            .fetch_one(pool)
            .await
            .map_err(db_error)?;
    match claim["disposition"].as_str() {
        Some("obsolete" | "live_owner" | "exhausted") => return Ok(claim),
        Some("claimed") => {}
        _ => return Err(invalid("unknown Agent claim disposition")),
    }
    let owner = AgentRunLease {
        attempt: claim["attempt"]
            .as_i64()
            .and_then(|n| i32::try_from(n).ok())
            .ok_or_else(|| invalid("Agent attempt missing"))?,
        max_attempts: claim["max_attempts"]
            .as_i64()
            .and_then(|n| i32::try_from(n).ok())
            .ok_or_else(|| invalid("Agent budget missing"))?,
        execution_owner_token: Uuid::parse_str(
            claim["execution_owner_token"]
                .as_str()
                .ok_or_else(|| invalid("Agent token missing"))?,
        )
        .map_err(invalid)?,
    };
    let journal = PgJournal {
        pool,
        request,
        owner: &owner,
        source_reader: reader,
    };
    let local = cancel.child_token();
    let finished = CancellationToken::new();
    let work = async {
        let result = async {
            let bundle: Value = sqlx::query_scalar(
                "SELECT kb_bid_v2_load_tender_analysis_input($1,$2,$3::kb_sha256)",
            )
            .bind(request.request_artifact_id)
            .bind(request.request_revision)
            .bind(&request.frozen_input_sha256)
            .fetch_one(pool)
            .await
            .map_err(db_error)?;
            if bundle["runtime"].is_null() {
                return Err(AgentError::new(
                    "AGENT_PROVIDER_UNAVAILABLE",
                    "request has no frozen Agent runtime",
                ));
            }
            let input: FrozenInput =
                serde_json::from_value(bundle["input"].clone()).map_err(invalid)?;
            let config: Config =
                serde_json::from_value(bundle["runtime"].clone()).map_err(invalid)?;
            let result = agent::run(&input, &config, &journal, model, &local).await?;
            let compiled = publication(&input, &result)?;
            let receipt:Value=sqlx::query_scalar("SELECT kb_bid_v2_publish_requirement_set_v4($1,$2,$3::kb_sha256,$4,$5::kb_actor_identity,$6,$7)")
            .bind(request.request_artifact_id).bind(request.request_revision).bind(&request.frozen_input_sha256)
            .bind(compiled).bind("system:requirement-set-compile-v4").bind(owner.attempt).bind(owner.execution_owner_token)
            .fetch_one(pool).await.map_err(db_error)?;
            Ok(receipt)
        }
        .await;
        finished.cancel();
        result
    };
    let heartbeat = async {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        interval.tick().await;
        let deadline = tokio::time::sleep(std::time::Duration::from_secs(45 * 60));
        tokio::pin!(deadline);
        loop {
            let error = tokio::select! {
                biased;
                _ = finished.cancelled() => return None,
                _ = &mut deadline => Some(AgentError::new("AGENT_DEADLINE_EXCEEDED","analysis deadline reached")),
                _ = local.cancelled() => Some(AgentError::new("INTERNAL","analysis cancelled")),
                _ = interval.tick() => heartbeat_once(pool, request, &owner).await.err(),
            };
            if let Some(error) = error {
                local.cancel();
                return Some(error);
            }
        }
    };
    let (work_result, heartbeat_error) = tokio::join!(work, heartbeat);
    let result = match (work_result, heartbeat_error) {
        (Ok(value), _) => Ok(value),
        (Err(_), Some(error)) => Err(error),
        (Err(error), None) => Err(error),
    };
    persist_attempt_outcome(pool, request, &owner, result).await
}

async fn persist_attempt_outcome(
    pool: &PgPool,
    request: &BidAuthoringRequestIdentityV2,
    owner: &AgentRunLease,
    result: Result<Value, AgentError>,
) -> Result<Value, AgentError> {
    let error = match result {
        Ok(value) => return Ok(value),
        Err(error) => error,
    };
    match error.request_queue_effect() {
        RequestQueueEffect::AckObsolete => Ok(json!({"disposition":"obsolete"})),
        RequestQueueEffect::RetryUnchanged => Err(error),
        RequestQueueEffect::ReleaseThenRetry => {
            let _ = sqlx::query(
                "SELECT kb_bid_v2_tender_agent_yield_for_retry($1,$2::kb_sha256,$3,$4,$5,$6)",
            )
            .bind(request.request_artifact_id)
            .bind(&request.frozen_input_sha256)
            .bind(owner.attempt)
            .bind(owner.execution_owner_token)
            .bind("INTERNAL")
            .bind(&error.message)
            .execute(pool)
            .await;
            Err(error)
        }
        RequestQueueEffect::YieldThenRetry => {
            record_attempt_sql(pool, request, owner, true, &error).await?;
            Err(error)
        }
        RequestQueueEffect::FailRequest => {
            match record_attempt_sql(pool, request, owner, false, &error).await {
                Ok(()) => Ok(json!({"status":"failed","error_code":error.code})),
                Err(recorded)
                    if recorded.request_queue_effect() == RequestQueueEffect::AckObsolete =>
                {
                    Ok(json!({"disposition":"obsolete"}))
                }
                Err(recorded) => Err(recorded),
            }
        }
    }
}

async fn record_attempt_sql(
    pool: &PgPool,
    request: &BidAuthoringRequestIdentityV2,
    owner: &AgentRunLease,
    yield_for_retry: bool,
    error: &AgentError,
) -> Result<(), AgentError> {
    let sql = if yield_for_retry {
        "SELECT kb_bid_v2_tender_agent_yield_for_retry($1,$2::kb_sha256,$3,$4,$5,$6)"
    } else {
        "SELECT kb_bid_v2_tender_agent_fail($1,$2::kb_sha256,$3,$4,$5,$6)"
    };
    sqlx::query(sql)
        .bind(request.request_artifact_id)
        .bind(&request.frozen_input_sha256)
        .bind(owner.attempt)
        .bind(owner.execution_owner_token)
        .bind(&error.code)
        .bind(&error.message)
        .execute(pool)
        .await
        .map(|_| ())
        .map_err(db_error)
}
