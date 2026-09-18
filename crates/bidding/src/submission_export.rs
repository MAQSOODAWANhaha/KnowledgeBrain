//! Publish a frozen saved DOCX, its ONLYOFFICE PDF and a version-bound report.
//! No workspace block renderer participates in this path.
use crate::agent_error::{AgentError, RequestQueueEffect};
use crate::bid_authoring_v2::AgentRunLease;
use crate::tender_analysis::postgres::db_error;
use async_trait::async_trait;
use platform::SubmissionExportJobV2;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Debug)]
pub struct ExportError(pub String);
impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for ExportError {}

impl From<ExportError> for AgentError {
    fn from(error: ExportError) -> Self {
        if let Some(message) = error.0.strip_prefix("TRANSIENT_HANDLER:") {
            Self::new("INTERNAL", message)
        } else {
            Self::new("RENDERER_FAILED", error.0)
        }
    }
}

pub struct ExportLimits {
    pub max_input_bytes: usize,
    pub max_render_output_bytes: usize,
}

pub use crate::export_review::FrozenContext;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenSource {
    pub round_id: Uuid,
    pub version_id: Uuid,
    pub docx_sha256: String,
    pub object_ref: String,
    pub byte_length: u64,
    pub document_set_id: Uuid,
    pub document_set_sha256: String,
    pub requirement_set_id: Uuid,
    pub requirement_set_sha256: String,
    pub round_sha256: String,
}
impl FrozenSource {
    pub fn validate(&self, max_bytes: usize) -> Result<(), ExportError> {
        for hash in [
            &self.docx_sha256,
            &self.document_set_sha256,
            &self.requirement_set_sha256,
            &self.round_sha256,
        ] {
            if hash.len() != 64
                || !hash
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            {
                return Err(ExportError("frozen DOCX source digest invalid".into()));
            }
        }
        if self.byte_length == 0
            || self.byte_length > max_bytes as u64
            || self.object_ref != format!("objects/{}", self.docx_sha256)
        {
            return Err(ExportError(
                "frozen DOCX source identity or byte budget invalid".into(),
            ));
        }
        Ok(())
    }
    fn verify_bytes(&self, bytes: &[u8]) -> Result<(), ExportError> {
        if bytes.len() as u64 != self.byte_length
            || hex::encode(Sha256::digest(bytes)) != self.docx_sha256
        {
            return Err(ExportError(
                "frozen DOCX bytes do not match the saved version".into(),
            ));
        }
        crate::tender_upload::validate_docx_document(bytes)
            .map_err(|e| ExportError(e.to_string()))?;
        Ok(())
    }
}

#[async_trait]
pub trait ExportIo: Send + Sync {
    async fn read_blob(
        &self,
        sha256: &str,
        max_bytes: usize,
        cancel: &CancellationToken,
    ) -> Result<Vec<u8>, ExportError>;
    async fn stage_object(
        &self,
        pool: &PgPool,
        staging_id: Uuid,
        digest: &str,
        media_type: &str,
        bytes: &[u8],
        actor: &str,
    ) -> Result<String, ExportError>;
}

fn conversion_error(error: crate::onlyoffice_conversion::Error) -> AgentError {
    let code = match error.kind {
        crate::onlyoffice_conversion::ErrorKind::Unavailable => "INTERNAL",
        _ => "RENDERER_FAILED",
    };
    AgentError::new(code, error.to_string())
}

/// Technical checks are recorded separately from semantic/layout review.
/// Export remains available without claiming that an edited bid is compliant.
fn frozen_context_from_input(input: &Value) -> Result<Option<FrozenContext>, AgentError> {
    input
        .pointer("/request/frozen_context")
        .filter(|v| !v.is_null())
        .map(|v| {
            serde_json::from_value(v.clone())
                .map_err(|e| AgentError::new("FROZEN_INPUT_DIGEST_MISMATCH", e.to_string()))
        })
        .transpose()
}

fn export_review_check(
    inventory: &crate::export_review::Inventory,
    inventory_sha256: &str,
    review: Option<&crate::export_review::agent::Report>,
) -> Value {
    let bound_review = review.filter(|review| {
        review.docx_sha256 == inventory.docx_sha256
            && review.pdf_sha256 == inventory.pdf_sha256
            && review.inventory_sha256 == inventory_sha256
    });
    let (status, detail) = match bound_review.map(|review| review.status.as_str()) {
        Some("reviewed") => (
            "pass",
            "Independent final-file review completed for this frozen inventory.",
        ),
        Some("reviewed_with_findings") => (
            "fail",
            "Independent final-file review recorded findings against this frozen inventory.",
        ),
        Some("reviewed_with_source_limitations") => (
            "not_checked",
            "Independent final-file review completed with documented frozen-source limitations; full acceptance remains unresolved.",
        ),
        Some("not_checked") => (
            "not_checked",
            "Independent final-file review completed with unchecked output items; full acceptance remains unresolved.",
        ),
        _ => (
            "not_checked",
            "No completed export-review checkpoint is bound to this frozen inventory; composition approval is not inherited.",
        ),
    };
    json!({"id":"export_review","status":status,"detail":detail})
}

fn report(
    source: &FrozenSource,
    docx_id: Uuid,
    pdf_id: Uuid,
    pdf_sha: &str,
    pdf_length: usize,
    inventory: &crate::export_review::Inventory,
    review: Option<&crate::export_review::agent::Report>,
) -> Result<Value, ExportError> {
    let inventory_sha256 = crate::tender_analysis::digest(inventory).map_err(ExportError)?;
    let not_checked = inventory
        .units
        .iter()
        .filter(|unit| unit.kind == "not_checked")
        .count();
    let inventory_ok = inventory.docx_sha256 == source.docx_sha256
        && inventory.pdf_sha256.as_deref() == Some(pdf_sha)
        && !inventory.units.is_empty();
    Ok(
        json!({"schema_version":2,"status":"needs_review","source":source,
        "outputs":{"docx":{"artifact_id":docx_id,"sha256":source.docx_sha256,"byte_length":source.byte_length},
                   "pdf":{"artifact_id":pdf_id,"sha256":pdf_sha,"byte_length":pdf_length}},
        "output_images":inventory.images,
        "output_inventory":{
            "docx_sha256":inventory.docx_sha256,
            "pdf_sha256":inventory.pdf_sha256,
            "inventory_sha256":inventory_sha256,
            "unit_count":inventory.units.len(),
            "not_checked_count":not_checked
        },
        "checks":[
            {"id":"saved_source_identity","status":"pass","detail":"DOCX bytes match the frozen saved version, object digest and length."},
            {"id":"docx_structure","status":"pass","detail":"The actual DOCX passed the existing OOXML document validator."},
            {"id":"same_version_pdf","status":"pass","detail":"ONLYOFFICE converted a capability restricted to this exact saved DOCX; the returned PDF passed document validation."},
            {"id":"output_inventory","status":if inventory_ok {"pass"} else {"fail"},
                "detail":"File-native DOCX inventory is bound to this frozen version; generation bookmarks are not the scan denominator."},
            export_review_check(inventory, &inventory_sha256, review),
            {"id":"tender_semantics","status":"not_checked","detail":"Export does not claim semantic approval from outline, fill, or composition. Only checks that actually ran are recorded above."},
            {"id":"page_layout","status":"not_checked","detail":"Pagination, tables, signatures and source-specific layout still require inspection of these actual files."}
        ]}),
    )
}

pub async fn execute(
    pool: &PgPool,
    job: &SubmissionExportJobV2,
    cancel: CancellationToken,
    cleanup_tracker: platform::StagedObjectCleanupTracker,
    objects: &dyn ExportIo,
    limits: ExportLimits,
) -> Result<(), AgentError> {
    let claim: Value =
        sqlx::query_scalar("SELECT kb_bid_v2_tender_agent_claim($1,$2,$3::kb_sha256)")
            .bind(job.request.request_artifact_id)
            .bind(job.request.request_revision)
            .bind(&job.request.frozen_input_sha256)
            .fetch_one(pool)
            .await
            .map_err(db_error)?;
    match claim["disposition"].as_str() {
        Some("claimed") => {}
        Some("obsolete" | "exhausted") => return Ok(()),
        Some("live_owner") => {
            return Err(AgentError::new(
                "REQUEST_ATTEMPT_SUPERSEDED",
                "another export worker owns this request",
            ));
        }
        _ => {
            return Err(AgentError::new(
                "INTERNAL",
                "unknown export claim disposition",
            ));
        }
    }
    let owner = AgentRunLease {
        attempt: claim["attempt"]
            .as_i64()
            .and_then(|v| i32::try_from(v).ok())
            .ok_or_else(|| AgentError::new("INTERNAL", "export attempt missing"))?,
        max_attempts: claim["max_attempts"]
            .as_i64()
            .and_then(|v| i32::try_from(v).ok())
            .ok_or_else(|| AgentError::new("INTERNAL", "export attempt budget missing"))?,
        execution_owner_token: claim["execution_owner_token"]
            .as_str()
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| AgentError::new("INTERNAL", "export owner token missing"))?,
    };
    let local = cancel.child_token();
    let finished = CancellationToken::new();
    let work = async {
        let result = execute_owned(
            pool,
            job,
            local.clone(),
            cleanup_tracker,
            objects,
            limits,
            &owner,
        )
        .await;
        finished.cancel();
        result
    };
    let heartbeat = async {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            let error = tokio::select! {
                biased;
                _ = finished.cancelled() => return None,
                _ = local.cancelled() => Some(AgentError::new("INTERNAL", "export cancelled")),
                _ = interval.tick() => {
                    let beat = sqlx::query("SELECT kb_bid_v2_tender_agent_heartbeat($1,$2::kb_sha256,$3,$4)")
                        .bind(job.request.request_artifact_id).bind(&job.request.frozen_input_sha256)
                        .bind(owner.attempt).bind(owner.execution_owner_token).execute(pool);
                    tokio::select! {
                        biased;
                        _ = finished.cancelled() => return None,
                        _ = local.cancelled() => Some(AgentError::new("INTERNAL", "export cancelled")),
                        result = beat => result.err().map(db_error),
                    }
                }
            };
            if let Some(error) = error {
                local.cancel();
                return Some(error);
            }
        }
    };
    let (result, lease_error) = tokio::join!(work, heartbeat);
    if result.is_ok() {
        return result;
    }
    // A lost commit ACK wins over an error racing the successful publication.
    let published = crate::bid_authoring_v2::load_submission_export_input_v2(
        pool,
        job.request.request_artifact_id,
        job.request.request_revision,
        &job.request.frozen_input_sha256,
    )
    .await;
    if published
        .as_ref()
        .is_ok_and(|value| !value["published"].is_null())
    {
        return Ok(());
    }
    let error = lease_error.unwrap_or_else(|| result.unwrap_err());
    match error.request_queue_effect() {
        RequestQueueEffect::AckObsolete => Ok(()),
        RequestQueueEffect::RetryUnchanged => Err(error),
        RequestQueueEffect::YieldThenRetry | RequestQueueEffect::ReleaseThenRetry => {
            sqlx::query("SELECT kb_bid_v2_tender_agent_yield_for_retry($1,$2::kb_sha256,$3,$4,'INTERNAL',$5)")
                .bind(job.request.request_artifact_id).bind(&job.request.frozen_input_sha256)
                .bind(owner.attempt).bind(owner.execution_owner_token).bind(&error.message)
                .execute(pool).await.map_err(db_error)?;
            Err(error)
        }
        RequestQueueEffect::FailRequest => {
            sqlx::query("SELECT kb_bid_v2_tender_agent_fail($1,$2::kb_sha256,$3,$4,$5,$6)")
                .bind(job.request.request_artifact_id)
                .bind(&job.request.frozen_input_sha256)
                .bind(owner.attempt)
                .bind(owner.execution_owner_token)
                .bind(&error.code)
                .bind(&error.message)
                .execute(pool)
                .await
                .map_err(db_error)?;
            Ok(())
        }
    }
}

async fn execute_owned(
    pool: &PgPool,
    job: &SubmissionExportJobV2,
    cancel: CancellationToken,
    cleanup_tracker: platform::StagedObjectCleanupTracker,
    objects: &dyn ExportIo,
    limits: ExportLimits,
    owner: &AgentRunLease,
) -> Result<(), AgentError> {
    const ACTOR: &str = "system:submission-export-v2";
    let input = crate::bid_authoring_v2::load_submission_export_input_v2(
        pool,
        job.request.request_artifact_id,
        job.request.request_revision,
        &job.request.frozen_input_sha256,
    )
    .await
    .map_err(db_error)?;
    if serde_json::to_vec(&input)
        .map_err(|e| ExportError(e.to_string()))?
        .len()
        > limits.max_input_bytes
    {
        return Err(ExportError("frozen export input exceeds byte budget".into()).into());
    }
    if input.get("published").is_some_and(|v| !v.is_null()) {
        return Ok(());
    }
    if input["project_id"] != json!(job.project_id)
        || input["workspace_id"] != json!(job.workspace_id)
    {
        return Err(ExportError("export job belongs to another frozen source".into()).into());
    }
    let source: FrozenSource =
        serde_json::from_value(input["source"].clone()).map_err(|e| ExportError(e.to_string()))?;
    source.validate(limits.max_render_output_bytes)?;
    let docx = objects
        .read_blob(&source.docx_sha256, limits.max_render_output_bytes, &cancel)
        .await?;
    let checked_source = source.clone();
    let docx = tokio::task::spawn_blocking(move || {
        checked_source.verify_bytes(&docx)?;
        Ok::<_, ExportError>(docx)
    })
    .await
    .map_err(|e| ExportError(e.to_string()))??;
    let mut cleanup = cleanup_tracker.guard();
    let render = if input["render"].is_object() {
        input["render"].clone()
    } else {
        let config = crate::onlyoffice_conversion::Config::load().map_err(conversion_error)?;
        let conversion_source = crate::onlyoffice_conversion::FrozenDocxSource {
            workspace_id: job.workspace_id,
            request_id: job.request.request_artifact_id,
            version_id: source.version_id,
            docx_sha256: source.docx_sha256.clone(),
            byte_length: source.byte_length,
        };
        let pdf = crate::onlyoffice_conversion::convert_pdf(
            &config,
            &conversion_source,
            limits.max_render_output_bytes,
            &cancel,
        )
        .await
        .map_err(conversion_error)?;
        let stage = Uuid::new_v4();
        cleanup.register(stage);
        let object_ref = objects
            .stage_object(
                pool,
                stage,
                &pdf.pdf_sha256,
                crate::tender_upload::PDF_MEDIA_TYPE,
                &pdf.bytes,
                ACTOR,
            )
            .await?;
        let identity = json!({"schema_version":1,"source":source,"pdf":{"object_ref":object_ref,"sha256":pdf.pdf_sha256,"media_type":crate::tender_upload::PDF_MEDIA_TYPE,"byte_length":pdf.bytes.len()}});
        let saved: Value = sqlx::query_scalar(
            "SELECT kb_bid_v2_submission_export_render_put($1,$2::kb_sha256,$3,$4,$5,$6)",
        )
        .bind(job.request.request_artifact_id)
        .bind(&job.request.frozen_input_sha256)
        .bind(owner.attempt)
        .bind(owner.execution_owner_token)
        .bind(stage)
        .bind(&identity)
        .fetch_one(pool)
        .await
        .map_err(db_error)?;
        cleanup.disarm(stage);
        saved
    };
    if render["source"] != json!(source) {
        return Err(AgentError::new(
            "FROZEN_INPUT_DIGEST_MISMATCH",
            "render receipt belongs to another DOCX source",
        ));
    }
    let pdf_sha = render["pdf"]["sha256"].as_str().ok_or_else(|| {
        AgentError::new("FROZEN_INPUT_DIGEST_MISMATCH", "saved PDF identity missing")
    })?;
    let pdf = objects
        .read_blob(pdf_sha, limits.max_render_output_bytes, &cancel)
        .await?;
    if hex::encode(Sha256::digest(&pdf)) != pdf_sha
        || render["pdf"]["byte_length"].as_u64() != Some(pdf.len() as u64)
    {
        return Err(AgentError::new(
            "ASSET_DIGEST_MISMATCH",
            "saved PDF bytes changed",
        ));
    }
    let inventory: crate::export_review::Inventory = if input["render_snapshot"].is_object() {
        let snapshot = &input["render_snapshot"];
        if snapshot["schema_version"] != 2 {
            return Err(AgentError::new(
                "FROZEN_INPUT_DIGEST_MISMATCH",
                "output image contract cannot resume an older inventory",
            ));
        }
        if snapshot["inventory_sha256"]
            != crate::tender_analysis::digest(&snapshot["inventory"]).map_err(ExportError)?
        {
            return Err(AgentError::new(
                "FROZEN_INPUT_DIGEST_MISMATCH",
                "output snapshot digest changed",
            ));
        }
        // Replaying the receipt also checks that every immutable image still
        // has this request's durable owner; it never reparses or replaces pixels.
        sqlx::query("SELECT kb_bid_v2_submission_export_snapshot_put($1,$2::kb_sha256,$3,$4,$5)")
            .bind(job.request.request_artifact_id)
            .bind(&job.request.frozen_input_sha256)
            .bind(owner.attempt)
            .bind(owner.execution_owner_token)
            .bind(snapshot)
            .execute(pool)
            .await
            .map_err(db_error)?;
        serde_json::from_value(snapshot["inventory"].clone())
            .map_err(|e| ExportError(e.to_string()))?
    } else {
        let parsed = crate::export_review::inventory_from_files(&docx, Some(&pdf), &cancel).await?;
        let inventory = parsed.inventory;
        if parsed.images.len() != inventory.images.len() {
            return Err(AgentError::new(
                "FROZEN_INPUT_DIGEST_MISMATCH",
                "inventory image bytes are incomplete",
            ));
        }
        let mut image_stages = serde_json::Map::new();
        for (sha, image) in &inventory.images {
            crate::agent_runtime::check_cancel(&cancel)?;
            let bytes = parsed.images.get(sha).ok_or_else(|| {
                AgentError::new(
                    "FROZEN_INPUT_DIGEST_MISMATCH",
                    "inventory image bytes missing",
                )
            })?;
            image
                .validate_bytes(bytes)
                .map_err(|error| AgentError::new("ASSET_DIGEST_MISMATCH", error))?;
            let stage = Uuid::new_v4();
            cleanup.register(stage);
            let object_ref = objects
                .stage_object(pool, stage, sha, &image.media_type, bytes, ACTOR)
                .await?;
            if object_ref != image.object_ref {
                return Err(AgentError::new(
                    "FROZEN_INPUT_DIGEST_MISMATCH",
                    "inventory image object reference changed",
                ));
            }
            image_stages.insert(sha.clone(), json!(stage));
        }
        let snapshot = json!({"schema_version":2,"inventory_sha256":crate::tender_analysis::digest(&inventory).map_err(ExportError)?,"inventory":inventory});
        let saved: Value = sqlx::query_scalar(
            "SELECT kb_bid_v2_submission_export_snapshot_put($1,$2::kb_sha256,$3,$4,$5,$6)",
        )
        .bind(job.request.request_artifact_id)
        .bind(&job.request.frozen_input_sha256)
        .bind(owner.attempt)
        .bind(owner.execution_owner_token)
        .bind(snapshot)
        .bind(json!(&image_stages))
        .fetch_one(pool)
        .await
        .map_err(db_error)?;
        for stage in image_stages.values() {
            cleanup.disarm(
                serde_json::from_value(stage.clone()).map_err(|e| ExportError(e.to_string()))?,
            );
        }
        serde_json::from_value(saved["inventory"].clone())
            .map_err(|e| ExportError(e.to_string()))?
    };
    if inventory.docx_sha256 != source.docx_sha256
        || inventory.pdf_sha256.as_deref() != Some(pdf_sha)
        || inventory.units.is_empty()
    {
        return Err(AgentError::new(
            "FROZEN_INPUT_DIGEST_MISMATCH",
            "output inventory does not match frozen files",
        ));
    }
    let mut parser_files = std::collections::BTreeSet::new();
    if inventory.parser_manifests.len() != 2
        || inventory.parser_manifests.iter().any(|manifest| {
            manifest.schema_version != 1
                || manifest.profile != "output_inventory_v1"
                || !parser_files.insert(manifest.file_sha256.as_str())
        })
        || parser_files != std::collections::BTreeSet::from([source.docx_sha256.as_str(), pdf_sha])
    {
        return Err(AgentError::new(
            "FROZEN_INPUT_DIGEST_MISMATCH",
            "output snapshot lacks the exact DOCX/PDF parser manifests",
        ));
    }
    let docx_id = Uuid::new_v4();
    let pdf_id = Uuid::new_v4();
    let frozen_context = frozen_context_from_input(&input)?;
    let image_reader = crate::export_review::runtime::OutputImages { objects };
    let review = crate::export_review::runtime::run_if_allowed(
        pool,
        &job.request,
        owner,
        frozen_context.as_ref(),
        crate::export_review::agent::FrozenFiles {
            docx: &docx,
            pdf: Some(&pdf),
            inventory: &inventory,
            images: &image_reader,
        },
        &cancel,
    )
    .await?;
    let mut report = report(
        &source,
        docx_id,
        pdf_id,
        pdf_sha,
        pdf.len(),
        &inventory,
        review.as_ref(),
    )?;
    report["export_review"] = json!(review);
    if review.is_some() {
        let checkpoint: Value =
            sqlx::query_scalar("SELECT kb_bid_v2_export_review_checkpoint_get($1,$2::kb_sha256)")
                .bind(job.request.request_artifact_id)
                .bind(&job.request.frozen_input_sha256)
                .fetch_one(pool)
                .await
                .map_err(db_error)?;
        report["execution_contract_sha256"] = checkpoint["contract_sha256"].clone();
        report["review_checkpoint_sha256"] =
            json!(crate::tender_analysis::digest(&checkpoint).map_err(ExportError)?);
    }
    let docx_stage = Uuid::new_v4();
    cleanup.register(docx_stage);
    let docx_ref = objects
        .stage_object(
            pool,
            docx_stage,
            &source.docx_sha256,
            crate::tender_upload::DOCX_MEDIA_TYPE,
            &docx,
            ACTOR,
        )
        .await?;
    if cancel.is_cancelled() {
        return Err(AgentError::new(
            "INTERNAL",
            "export cancelled before publication",
        ));
    }
    let published = crate::bid_authoring_v2::publish_submission_export_v2(
        pool,
        &job.request,
        Uuid::new_v4(),
        (
            crate::bid_authoring_v2::SubmissionExportOutputV2 {
                staging_id: Some(docx_stage),
                artifact_id: docx_id,
                object_ref: &docx_ref,
                sha256: &source.docx_sha256,
                media_type: crate::tender_upload::DOCX_MEDIA_TYPE,
                byte_length: docx.len() as i64,
            },
            crate::bid_authoring_v2::SubmissionExportOutputV2 {
                staging_id: None,
                artifact_id: pdf_id,
                object_ref: render["pdf"]["object_ref"].as_str().ok_or_else(|| {
                    AgentError::new(
                        "FROZEN_INPUT_DIGEST_MISMATCH",
                        "saved PDF object reference missing",
                    )
                })?,
                sha256: pdf_sha,
                media_type: crate::tender_upload::PDF_MEDIA_TYPE,
                byte_length: pdf.len() as i64,
            },
        ),
        &report,
        ACTOR,
        owner,
    )
    .await
    .map_err(db_error)?;
    // The database may replay an earlier atomic publication after ACK loss.
    // Only disarm the exact staged outputs actually adopted by that publication.
    if published["outputs"]["docx"]["artifact_id"] == json!(docx_id)
        || platform::schedule_object_upload_cleanup(docx_stage)
            .await
            .is_ok()
    {
        cleanup.disarm(docx_stage);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source() -> FrozenSource {
        FrozenSource {
            round_id: Uuid::new_v4(),
            version_id: Uuid::new_v4(),
            docx_sha256: "a".repeat(64),
            object_ref: format!("objects/{}", "a".repeat(64)),
            byte_length: 10,
            document_set_id: Uuid::new_v4(),
            document_set_sha256: "b".repeat(64),
            requirement_set_id: Uuid::new_v4(),
            requirement_set_sha256: "c".repeat(64),
            round_sha256: "d".repeat(64),
        }
    }
    #[test]
    fn frozen_source_rejects_wrong_objects_lengths_and_digests_before_conversion() {
        let mut s = source();
        assert!(s.validate(10).is_ok());
        assert!(s.validate(9).is_err());
        s.object_ref = "objects/other".into();
        assert!(s.validate(10).is_err());
        s = source();
        s.requirement_set_sha256 = "unknown".into();
        assert!(s.validate(10).is_err());
        assert!(source().verify_bytes(b"different document").is_err());
    }
    #[test]
    fn frozen_context_without_analysis_cannot_start_semantic_export_review() {
        let unavailable = FrozenContext {
            analysis_identity: None,
            execution_contract: None,
            layout_result: None,
        };
        assert!(!unavailable.allows_semantic_export_review());
        let v1 = FrozenContext {
            analysis_identity: Some(
                json!({"id":"analysis","sha256":"a".repeat(64),"schema_version":1}),
            ),
            execution_contract: None,
            layout_result: None,
        };
        assert!(!v1.allows_semantic_export_review());
        let ready = FrozenContext {
            analysis_identity: Some(
                json!({"id":"analysis","sha256":"a".repeat(64),"schema_version":2}),
            ),
            execution_contract: None,
            layout_result: None,
        };
        assert!(
            !ready.allows_semantic_export_review(),
            "a v2 basis does not replace a frozen execution contract"
        );
        let keys: Vec<_> = serde_json::to_value(&ready)
            .unwrap()
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert_eq!(
            keys,
            ["analysis_identity", "execution_contract", "layout_result"]
        );
    }
    #[test]
    fn conversion_unavailability_retries_but_invalid_output_is_terminal() {
        use crate::onlyoffice_conversion::{Error, ErrorKind};
        assert_eq!(
            conversion_error(Error {
                kind: ErrorKind::Unavailable,
                code: "ONLYOFFICE_CONVERSION_UNAVAILABLE",
                message: "conversion unavailable"
            })
            .request_queue_effect(),
            RequestQueueEffect::YieldThenRetry
        );
        assert_eq!(
            conversion_error(Error {
                kind: ErrorKind::Invalid,
                code: "VALIDATION",
                message: "invalid PDF"
            })
            .request_queue_effect(),
            RequestQueueEffect::FailRequest
        );
    }

    #[test]
    fn malformed_frozen_context_cannot_silently_downgrade_to_manual_export() {
        let input = json!({"request":{"frozen_context":{"execution_contract":"broken"}}});
        assert_eq!(
            frozen_context_from_input(&input).unwrap_err().code,
            "FROZEN_INPUT_DIGEST_MISMATCH"
        );
    }

    #[test]
    fn export_report_binds_both_files_without_claiming_semantic_or_layout_acceptance() {
        let s = source();
        let d = Uuid::new_v4();
        let p = Uuid::new_v4();
        let inventory = crate::export_review::Inventory {
            docx_sha256: s.docx_sha256.clone(),
            pdf_sha256: Some("e".repeat(64)),
            units: vec![crate::export_review::OutputUnit {
                id: "docx:unit:0".into(),
                file_sha256: s.docx_sha256.clone(),
                part: "word/document.xml".into(),
                ordinal: 0,
                kind: "paragraphs".into(),
                content_sha256: "f".repeat(64),
                text: "投标函".into(),
                bookmark: None,
                bookmarks: vec![],
                source_unit: None,
                not_checked_reason: None,
                image_sha256s: vec![],
            }],
            parser_manifests: vec![],
            images: Default::default(),
        };
        let r = report(&s, d, p, &"e".repeat(64), 42, &inventory, None).unwrap();
        assert_eq!(r["source"]["version_id"], json!(s.version_id));
        assert_eq!(r["outputs"]["docx"]["sha256"], s.docx_sha256);
        assert_eq!(r["outputs"]["pdf"]["artifact_id"], json!(p));
        assert_eq!(r["status"], "needs_review");
        assert_eq!(r["output_inventory"]["unit_count"], 1);
        assert!(
            r["checks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["id"] == "output_inventory" && c["status"] == "pass")
        );
        assert!(
            r["checks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["id"] == "export_review" && c["status"] == "not_checked")
        );
        assert!(
            r["checks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["id"] == "tender_semantics" && c["status"] == "not_checked")
        );
        let inventory_sha256 = crate::tender_analysis::digest(&inventory).unwrap();
        let reviewed = crate::export_review::agent::Report {
            docx_sha256: s.docx_sha256.clone(),
            pdf_sha256: Some("e".repeat(64)),
            inventory_sha256,
            status: "reviewed".into(),
            reviews: Default::default(),
            obligations: Default::default(),
        };
        for status in ["reviewed_with_source_limitations", "not_checked"] {
            let mut limited = reviewed.clone();
            limited.status = status.into();
            let check = export_review_check(&inventory, &reviewed.inventory_sha256, Some(&limited));
            assert_eq!(check["status"], "not_checked");
            assert!(check["detail"].as_str().unwrap().contains("completed with"));
        }
        let passed = report(&s, d, p, &"e".repeat(64), 42, &inventory, Some(&reviewed)).unwrap();
        assert!(
            passed["checks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["id"] == "export_review" && c["status"] == "pass")
        );
    }
}
