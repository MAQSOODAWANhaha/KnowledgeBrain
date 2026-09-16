//! Publish a frozen saved DOCX, its ONLYOFFICE PDF and a version-bound report.
//! No workspace block renderer participates in this path.
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

fn sql_error(error: sqlx::Error) -> ExportError {
    let deterministic = match &error {
        sqlx::Error::RowNotFound
        | sqlx::Error::ColumnIndexOutOfBounds { .. }
        | sqlx::Error::ColumnNotFound(_)
        | sqlx::Error::ColumnDecode { .. }
        | sqlx::Error::Decode(_)
        | sqlx::Error::TypeNotFound { .. } => true,
        sqlx::Error::Database(database) => database.code().is_some_and(|code| {
            code.starts_with("22")
                || code.starts_with("23")
                || matches!(code.as_ref(), "P0001" | "P0002")
        }),
        _ => false,
    };
    if deterministic {
        ExportError(error.to_string())
    } else {
        ExportError(format!("TRANSIENT_HANDLER:{error}"))
    }
}

/// Technical checks are recorded separately from semantic/layout review.
/// Export remains available without claiming that an edited bid is compliant.
fn frozen_context_from_input(input: &Value) -> Option<FrozenContext> {
    serde_json::from_value(input.pointer("/request/frozen_context").cloned()?).ok()
}

async fn load_completed_export_review(
    pool: &PgPool,
    job: &SubmissionExportJobV2,
    inventory: &crate::export_review::Inventory,
    input: &Value,
) -> Result<Option<crate::export_review::agent::Report>, ExportError> {
    if !frozen_context_from_input(input)
        .is_some_and(|context| context.allows_semantic_export_review())
    {
        return Ok(None);
    }
    let value: Option<sqlx::types::Json<crate::export_review::agent::Checkpoint>> = sqlx::query_scalar(
        "SELECT kb_bid_v2_export_review_checkpoint_get($1,$2::kb_sha256)",
    )
    .bind(job.request.request_artifact_id)
    .bind(&job.request.frozen_input_sha256)
    .fetch_one(pool)
    .await
    .map_err(sql_error)?;
    Ok(value.and_then(|v| v.0.completed_report()).filter(|review| {
        review.docx_sha256 == inventory.docx_sha256
    }))
}

fn export_review_check(
    inventory: &crate::export_review::Inventory,
    inventory_sha256: &str,
    review: Option<&crate::export_review::agent::Report>,
) -> Value {
    match review {
        Some(review)
            if review.docx_sha256 == inventory.docx_sha256
                && review.inventory_sha256 == inventory_sha256
                && review.status == "reviewed" =>
        {
            json!({"id":"export_review","status":"pass",
                "detail":"Independent final-file review completed for this frozen inventory."})
        }
        Some(review)
            if review.docx_sha256 == inventory.docx_sha256
                && review.inventory_sha256 == inventory_sha256
                && review.status == "reviewed_with_findings" =>
        {
            json!({"id":"export_review","status":"fail",
                "detail":"Independent final-file review recorded findings against this frozen inventory."})
        }
        _ => json!({"id":"export_review","status":"not_checked",
            "detail":"No completed export-review checkpoint is bound to this frozen inventory; composition approval is not inherited."}),
    }
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
    let inventory_sha256 =
        crate::tender_analysis::digest(inventory).map_err(ExportError)?;
    let not_checked = inventory
        .units
        .iter()
        .filter(|unit| unit.kind == "not_checked")
        .count();
    let inventory_ok = inventory.docx_sha256 == source.docx_sha256
        && inventory.pdf_sha256.as_deref() == Some(pdf_sha)
        && !inventory.units.is_empty();
    Ok(json!({"schema_version":2,"status":"needs_review","source":source,
    "outputs":{"docx":{"artifact_id":docx_id,"sha256":source.docx_sha256,"byte_length":source.byte_length},
               "pdf":{"artifact_id":pdf_id,"sha256":pdf_sha,"byte_length":pdf_length}},
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
        {"id":"tender_semantics","status":"not_checked","detail":"Export does not inherit semantic approval from an earlier composition manifest. Complete final-document review against the frozen tender requirements remains necessary."},
        {"id":"page_layout","status":"not_checked","detail":"Pagination, tables, signatures and source-specific layout still require inspection of these actual final files."}
    ]}))
}

pub async fn execute(
    pool: &PgPool,
    job: &SubmissionExportJobV2,
    cancel: CancellationToken,
    cleanup_tracker: platform::StagedObjectCleanupTracker,
    objects: &dyn ExportIo,
    limits: ExportLimits,
) -> Result<(), ExportError> {
    const ACTOR: &str = "system:submission-export-v2";
    let input = crate::bid_authoring_v2::load_submission_export_input_v2(
        pool,
        job.request.request_artifact_id,
        job.request.request_revision,
        &job.request.frozen_input_sha256,
    )
    .await
    .map_err(sql_error)?;
    if serde_json::to_vec(&input)
        .map_err(|e| ExportError(e.to_string()))?
        .len()
        > limits.max_input_bytes
    {
        return Err(ExportError(
            "frozen export input exceeds byte budget".into(),
        ));
    }
    if input.get("published").is_some_and(|v| !v.is_null()) {
        return Ok(());
    }
    if input["project_id"] != json!(job.project_id)
        || input["workspace_id"] != json!(job.workspace_id)
    {
        return Err(ExportError(
            "export job belongs to another frozen source".into(),
        ));
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
    let config =
        crate::onlyoffice_conversion::Config::load().map_err(|e| ExportError(e.to_string()))?;
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
    .map_err(|e| ExportError(e.to_string()))?;
    if cancel.is_cancelled() {
        return Err(ExportError("export cancelled".into()));
    }
    let docx_id = Uuid::new_v4();
    let pdf_id = Uuid::new_v4();
    let inventory = crate::export_review::inventory_from_files(&docx, Some(&pdf.bytes))
        .map_err(ExportError)?;
    if inventory.docx_sha256 != source.docx_sha256 || inventory.units.is_empty() {
        return Err(ExportError(
            "output inventory does not match the frozen DOCX".into(),
        ));
    }
    let review = match crate::export_review::runtime::run_if_allowed(
        pool,
        &job.request,
        frozen_context_from_input(&input).as_ref(),
        &docx,
        &pdf.bytes,
        &cancel,
    )
    .await
    {
        Ok(Some(report)) => Some(report),
        Ok(None) => load_completed_export_review(pool, job, &inventory, &input).await?,
        Err(error) if error.code == "AGENT_PROVIDER_UNAVAILABLE" => {
            load_completed_export_review(pool, job, &inventory, &input).await?
        }
        Err(error) => return Err(ExportError(error.message)),
    };
    let report = report(
        &source,
        docx_id,
        pdf_id,
        &pdf.pdf_sha256,
        pdf.bytes.len(),
        &inventory,
        review.as_ref(),
    )?;
    let docx_stage = Uuid::new_v4();
    let pdf_stage = Uuid::new_v4();
    let mut cleanup = cleanup_tracker.guard();
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
    cleanup.register(pdf_stage);
    let pdf_ref = objects
        .stage_object(
            pool,
            pdf_stage,
            &pdf.pdf_sha256,
            crate::tender_upload::PDF_MEDIA_TYPE,
            &pdf.bytes,
            ACTOR,
        )
        .await?;
    if cancel.is_cancelled() {
        return Err(ExportError("export cancelled before publication".into()));
    }
    let published = crate::bid_authoring_v2::publish_submission_export_v2(
        pool,
        &job.request,
        Uuid::new_v4(),
        crate::bid_authoring_v2::SubmissionExportOutputV2 {
            staging_id: docx_stage,
            artifact_id: docx_id,
            object_ref: &docx_ref,
            sha256: &source.docx_sha256,
            media_type: crate::tender_upload::DOCX_MEDIA_TYPE,
            byte_length: docx.len() as i64,
        },
        crate::bid_authoring_v2::SubmissionExportOutputV2 {
            staging_id: pdf_stage,
            artifact_id: pdf_id,
            object_ref: &pdf_ref,
            sha256: &pdf.pdf_sha256,
            media_type: crate::tender_upload::PDF_MEDIA_TYPE,
            byte_length: pdf.bytes.len() as i64,
        },
        &report,
        ACTOR,
    )
    .await
    .map_err(sql_error)?;
    // The database may replay an earlier atomic publication after ACK loss.
    // Only disarm the exact staged outputs actually adopted by that publication.
    for (stage, id, format) in [(docx_stage, docx_id, "docx"), (pdf_stage, pdf_id, "pdf")] {
        if published["outputs"][format]["artifact_id"] == json!(id)
            || platform::schedule_object_upload_cleanup(stage)
                .await
                .is_ok()
        {
            cleanup.disarm(stage);
        }
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
            analysis_identity: Some(json!({"id":"analysis","sha256":"a".repeat(64),"schema_version":1})),
            execution_contract: None,
            layout_result: None,
        };
        assert!(!v1.allows_semantic_export_review());
        let ready = FrozenContext {
            analysis_identity: Some(json!({"id":"analysis","sha256":"a".repeat(64),"schema_version":2})),
            execution_contract: None,
            layout_result: None,
        };
        assert!(ready.allows_semantic_export_review());
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
            }],
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
        };
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
