//! Inactive V2 `TenderDocumentProcess` application service.
//!
//! This module is deliberately not registered with the active V1 API or worker.
//! It processes one immutable tender document, freezes parser/OCR identities and
//! publishes SourceUnit revisions. Project-wide DocumentSet, disposition and
//! requirement work belongs to the later `RequirementSetCompile` boundary.

use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
use docparser::{
    ReadResult, StructuredSourceLocator, StructuredSourceUnit, StructuredSourceUnitKind,
};
use runtime::{BidAuthoringJobPayloadV2, BidAuthoringRequestIdentityV2};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::tender_upload::validate_tender_upload;

pub const TENDER_SOURCE_PURPOSE: &str = "tender_requirements_and_structure_only";
pub const TENDER_CONVERTER_OPERATION: &str = "docparser-structured-source-v2";
pub const TENDER_VISION_OPERATION: &str = "tender-image-ocr-v1";
pub const TENDER_PROCESS_ACTOR: &str = "system:tender-document-process-v2";
pub const MAX_TENDER_IMAGE_BYTES: usize = 20 * 1024 * 1024;
pub const MAX_TENDER_SOURCE_UNITS: usize = 100_000;
pub const MAX_TENDER_SOURCE_TEXT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum TenderDocumentProcessError {
    #[error("payload is not a TenderDocumentProcess request")]
    WrongJobKind,
    #[error("frozen request identity is invalid: {0}")]
    InvalidRequest(String),
    #[error("frozen tender document is missing")]
    MissingDocument,
    #[error("frozen tender document identity mismatch: {0}")]
    FrozenInputMismatch(String),
    #[error("tender source conversion failed: {0}")]
    Conversion(String),
    #[error("tender structured source is invalid: {0}")]
    StructuredSource(String),
    #[error("tender image is missing: {0}")]
    MissingImage(String),
    #[error("tender image identity mismatch: {0}")]
    ImageIdentity(String),
    #[error("tender OCR/VLM failed: {0}")]
    Vision(String),
    #[error("tender OCR produced no text: {0}")]
    EmptyOcr(String),
    #[error("object freeze failed: {0}")]
    ObjectFreeze(String),
    #[error("source publication failed: {0}")]
    Publication(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrozenTenderDocument {
    pub request: BidAuthoringRequestIdentityV2,
    pub project_id: Uuid,
    /// Immutable `bid_documents` identity. The inactive transport retains the
    /// historical `document_revision_id` field name until the Phase 7 cutover.
    pub document_id: Uuid,
    pub document_sha256: String,
    pub role_revision_id: Uuid,
    pub role_revision_sha256: String,
    pub converter_contract_id: Uuid,
    pub converter_contract_sha256: String,
    pub file_name: String,
    pub media_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrozenObjectIdentity {
    pub staging_id: Uuid,
    pub object_ref: String,
    pub sha256: String,
    pub media_type: String,
    pub byte_length: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrozenContractIdentity {
    pub id: Uuid,
    pub sha256: String,
    pub canonical_payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceUnitSpanV2 {
    pub schema_version: u8,
    pub project_id: Uuid,
    pub document_id: Uuid,
    pub converted_source_revision_id: Uuid,
    pub parser_unit_key: String,
    pub parser_ordinal: u32,
    pub source_purpose: String,
    pub locator: StructuredSourceLocator,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tender_image_artifact_revision_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublishedSourceUnitKind {
    Section,
    TableRow,
    FormRegion,
    AttachmentRegion,
    ImageOcrRegion,
}

impl PublishedSourceUnitKind {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Section => "section",
            Self::TableRow => "table_row",
            Self::FormRegion => "form_region",
            Self::AttachmentRegion => "attachment_region",
            Self::ImageOcrRegion => "image_ocr_region",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TenderImageArtifactRevision {
    pub id: Uuid,
    pub ordinal: u32,
    pub original_ref: String,
    pub original: FrozenObjectIdentity,
    pub ocr_text: FrozenObjectIdentity,
    pub model_contract: FrozenContractIdentity,
    pub operation_contract: FrozenContractIdentity,
    pub source_locator: StructuredSourceLocator,
    pub canonical_payload: Vec<u8>,
    pub content_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceUnitRevision {
    pub id: Uuid,
    pub lineage_id: Uuid,
    pub revision: i64,
    pub ordinal: u32,
    pub unit_kind: PublishedSourceUnitKind,
    pub source_span_v2: SourceUnitSpanV2,
    pub source_span_sha256: String,
    pub text_utf8: String,
    pub text_sha256: String,
    pub canonical_payload: Vec<u8>,
    pub content_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConvertedTenderSourceRevision {
    pub id: Uuid,
    pub revision: i64,
    pub source_object: FrozenObjectIdentity,
    pub canonical_payload: Vec<u8>,
    pub converter_contract_sha256: String,
    pub image_asset_set_sha256: String,
}

#[derive(Debug, Clone)]
pub struct TenderDocumentPublication {
    pub request: BidAuthoringRequestIdentityV2,
    pub project_id: Uuid,
    pub document_id: Uuid,
    pub document_sha256: String,
    pub converted_source: ConvertedTenderSourceRevision,
    pub image_artifacts: Vec<TenderImageArtifactRevision>,
    pub source_units: Vec<SourceUnitRevision>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TenderDocumentProcessReceipt {
    pub request_artifact_id: Uuid,
    pub converted_source_revision_id: Uuid,
    pub converted_source_sha256: String,
    pub source_unit_count: u32,
    pub image_ocr_region_count: u32,
    pub replayed: bool,
}

#[async_trait]
pub trait TenderDocumentProcessRepository: Send + Sync {
    async fn load_frozen_document(
        &self,
        payload: &BidAuthoringJobPayloadV2,
    ) -> Result<Option<FrozenTenderDocument>, TenderDocumentProcessError>;

    async fn stage_object(
        &self,
        owner_id: Uuid,
        occurrence: &str,
        media_type: &str,
        bytes: &[u8],
    ) -> Result<FrozenObjectIdentity, TenderDocumentProcessError>;

    async fn abandon_staged_object(&self, object: &FrozenObjectIdentity);

    async fn publish(
        &self,
        publication: TenderDocumentPublication,
    ) -> Result<TenderDocumentProcessReceipt, TenderDocumentProcessError>;
}

#[async_trait]
pub trait TenderSourceConverter: Send + Sync {
    async fn convert(
        &self,
        file_name: &str,
        bytes: Vec<u8>,
    ) -> Result<ReadResult, TenderDocumentProcessError>;
}

pub struct DocParserTenderSourceConverter;

#[async_trait]
impl TenderSourceConverter for DocParserTenderSourceConverter {
    async fn convert(
        &self,
        file_name: &str,
        bytes: Vec<u8>,
    ) -> Result<ReadResult, TenderDocumentProcessError> {
        // Intentionally never routes through `convert_simple`: standalone
        // images need the validated DocReader image-region response.
        docparser::convert_tender_source(file_name, bytes)
            .await
            .map_err(|error| TenderDocumentProcessError::Conversion(error.0))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisionEnrichment {
    pub ocr_text: String,
    pub caption: String,
    pub model_contract: FrozenContractIdentity,
    pub operation_contract: FrozenContractIdentity,
}

#[async_trait]
pub trait TenderVisionEnricher: Send + Sync {
    async fn enrich(
        &self,
        image_object_ref: &str,
        image_source_type: &str,
        output_language: &str,
    ) -> Result<VisionEnrichment, TenderDocumentProcessError>;
}

pub struct ExistingTenderVisionEnricher;

#[async_trait]
impl TenderVisionEnricher for ExistingTenderVisionEnricher {
    async fn enrich(
        &self,
        image_object_ref: &str,
        image_source_type: &str,
        output_language: &str,
    ) -> Result<VisionEnrichment, TenderDocumentProcessError> {
        let image_object_ref = image_object_ref.to_string();
        let image_source_type = image_source_type.to_string();
        let output_language = output_language.to_string();
        let model = domain::vlm_model();
        if model.trim().is_empty() || !domain::vlm_configured() {
            return Err(TenderDocumentProcessError::Vision(
                "configured vision model identity is missing".into(),
            ));
        }
        let result = tokio::task::spawn_blocking(move || {
            enrichment::describe_image(
                &image_object_ref,
                &image_source_type,
                &output_language,
            )
        })
        .await
        .map_err(|_| TenderDocumentProcessError::Vision("vision task join failed".into()))?
        .map_err(TenderDocumentProcessError::Vision)?;
        let model_payload = format!("vision-model:{model}").into_bytes();
        let operation_payload = TENDER_VISION_OPERATION.as_bytes().to_vec();
        Ok(VisionEnrichment {
            ocr_text: result.0,
            caption: result.1,
            model_contract: FrozenContractIdentity {
                id: stable_uuid(b"tender-vision-model", model.as_bytes()),
                sha256: sha256_hex(&model_payload),
                canonical_payload: model_payload,
            },
            operation_contract: FrozenContractIdentity {
                id: stable_uuid(b"tender-vision-operation", TENDER_VISION_OPERATION.as_bytes()),
                sha256: sha256_hex(&operation_payload),
                canonical_payload: operation_payload,
            },
        })
    }
}

/// Present in the constructor so tests can prove this coarse job never emits a
/// project-wide compile job. Task 1C will own the only non-noop implementation.
#[async_trait]
pub trait TenderProcessTransport: Send + Sync {
    async fn enqueue_requirement_set_compile(
        &self,
        _project_id: Uuid,
    ) -> Result<(), TenderDocumentProcessError>;
}

pub struct InactiveTenderProcessTransport;

#[async_trait]
impl TenderProcessTransport for InactiveTenderProcessTransport {
    async fn enqueue_requirement_set_compile(
        &self,
        _project_id: Uuid,
    ) -> Result<(), TenderDocumentProcessError> {
        Err(TenderDocumentProcessError::Publication(
            "TenderDocumentProcess cannot enqueue RequirementSetCompile".into(),
        ))
    }
}

pub struct TenderDocumentProcessService<R, C, V, T> {
    repository: R,
    converter: C,
    vision: V,
    #[allow(dead_code)]
    transport: T,
}

impl<R, C, V, T> TenderDocumentProcessService<R, C, V, T>
where
    R: TenderDocumentProcessRepository,
    C: TenderSourceConverter,
    V: TenderVisionEnricher,
    T: TenderProcessTransport,
{
    pub fn new(repository: R, converter: C, vision: V, transport: T) -> Self {
        Self {
            repository,
            converter,
            vision,
            transport,
        }
    }

    pub async fn process(
        &self,
        payload: &BidAuthoringJobPayloadV2,
    ) -> Result<TenderDocumentProcessReceipt, TenderDocumentProcessError> {
        let (payload_request, project_id, document_id) = match payload {
            BidAuthoringJobPayloadV2::TenderDocumentProcess {
                request,
                project_id,
                document_revision_id,
            } => (request, *project_id, *document_revision_id),
            _ => return Err(TenderDocumentProcessError::WrongJobKind),
        };
        payload_request
            .validate()
            .map_err(|error| TenderDocumentProcessError::InvalidRequest(error.into()))?;
        let document = self
            .repository
            .load_frozen_document(payload)
            .await?
            .ok_or(TenderDocumentProcessError::MissingDocument)?;
        verify_frozen_document(&document, payload_request, project_id, document_id)?;
        validate_tender_upload(
            &document.file_name,
            Some(&document.media_type),
            &document.bytes,
        )
        .map_err(|error| TenderDocumentProcessError::FrozenInputMismatch(error.to_string()))?;

        let converted = self
            .converter
            .convert(&document.file_name, document.bytes.clone())
            .await?;
        if !converted.error.is_empty() {
            return Err(TenderDocumentProcessError::Conversion(converted.error));
        }
        validate_structured_units(&converted.structured_source_units)?;

        let converted_source_id = stable_uuid(
            b"tender-converted-source-v2",
            format!(
                "{}:{}:{}:{}",
                document.project_id,
                document.document_id,
                document.document_sha256,
                document.converter_contract_sha256
            )
            .as_bytes(),
        );
        let source_bytes = canonical_json(&json!({
            "schema_version": 2,
            "source_purpose": TENDER_SOURCE_PURPOSE,
            "project_id": document.project_id,
            "document_id": document.document_id,
            "document_sha256": document.document_sha256,
            "converter_contract_id": document.converter_contract_id,
            "converter_contract_sha256": document.converter_contract_sha256,
            "markdown": converted.markdown,
            "structured_source_units": converted.structured_source_units,
        }))?;
        let mut staged = Vec::new();
        let source_object = match self
            .repository
            .stage_object(
                converted_source_id,
                "structured-source",
                "application/json",
                &source_bytes,
            )
            .await
        {
            Ok(object) => {
                staged.push(object.clone());
                object
            }
            Err(error) => return Err(error),
        };

        let image_by_ref = converted
            .images
            .iter()
            .map(|image| (image.original_ref.as_str(), image))
            .collect::<HashMap<_, _>>();
        let image_source_type = converted
            .metadata
            .get("image_source_type")
            .cloned()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| {
                enrichment::image_source_type(&document.file_name, &converted.markdown).to_string()
            });
        let language = enrichment::infer_output_language(&format!(
            "{}\n{}",
            document.file_name, converted.markdown
        ));

        let build_result = self
            .build_publication(
                &document,
                payload_request,
                converted_source_id,
                source_object,
                source_bytes,
                &converted.structured_source_units,
                &image_by_ref,
                &image_source_type,
                &language,
                &mut staged,
            )
            .await;
        let publication = match build_result {
            Ok(publication) => publication,
            Err(error) => {
                self.abandon_all(&staged).await;
                return Err(error);
            }
        };
        match self.repository.publish(publication).await {
            Ok(receipt) => {
                if receipt.replayed {
                    self.abandon_all(&staged).await;
                }
                Ok(receipt)
            }
            Err(error) => {
                self.abandon_all(&staged).await;
                Err(error)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn build_publication(
        &self,
        document: &FrozenTenderDocument,
        request: &BidAuthoringRequestIdentityV2,
        converted_source_id: Uuid,
        source_object: FrozenObjectIdentity,
        source_payload: Vec<u8>,
        parser_units: &[StructuredSourceUnit],
        image_by_ref: &HashMap<&str, &docparser::ImageRef>,
        image_source_type: &str,
        language: &str,
        staged: &mut Vec<FrozenObjectIdentity>,
    ) -> Result<TenderDocumentPublication, TenderDocumentProcessError> {
        let mut image_artifacts = Vec::new();
        let mut source_units = Vec::with_capacity(parser_units.len());
        let mut image_set_digests = Vec::new();

        for parser_unit in parser_units {
            let (unit_kind, text, image_artifact_id) = if parser_unit.kind
                == StructuredSourceUnitKind::ImageRegion
            {
                let StructuredSourceLocator::Image {
                    original_ref,
                    media_type,
                    ..
                } = &parser_unit.locator
                else {
                    return Err(TenderDocumentProcessError::StructuredSource(format!(
                        "image unit {} lacks image locator",
                        parser_unit.key
                    )));
                };
                let image = image_by_ref.get(original_ref.as_str()).ok_or_else(|| {
                    TenderDocumentProcessError::MissingImage(original_ref.clone())
                })?;
                if image.data.is_empty() || image.data.len() > MAX_TENDER_IMAGE_BYTES {
                    return Err(TenderDocumentProcessError::ImageIdentity(format!(
                        "{} has invalid byte length",
                        original_ref
                    )));
                }
                if image.mime_type != *media_type {
                    return Err(TenderDocumentProcessError::ImageIdentity(format!(
                        "{} media type differs between unit and image bytes",
                        original_ref
                    )));
                }
                let image_id = stable_uuid(
                    b"tender-image-artifact-v2",
                    format!("{}:{}", converted_source_id, parser_unit.key).as_bytes(),
                );
                let original = self
                    .repository
                    .stage_object(image_id, "original", media_type, &image.data)
                    .await?;
                staged.push(original.clone());
                let enrichment = self
                    .vision
                    .enrich(&original.object_ref, image_source_type, language)
                    .await?;
                let ocr_text = enrichment.ocr_text.trim().to_string();
                if ocr_text.is_empty() {
                    return Err(TenderDocumentProcessError::EmptyOcr(
                        parser_unit.key.clone(),
                    ));
                }
                let ocr_object = self
                    .repository
                    .stage_object(
                        image_id,
                        "ocr-text",
                        "text/plain;charset=utf-8",
                        ocr_text.as_bytes(),
                    )
                    .await?;
                staged.push(ocr_object.clone());
                let image_payload = canonical_json(&json!({
                    "schema_version": 1,
                    "tender_image_artifact_revision_id": image_id,
                    "project_id": document.project_id,
                    "document_id": document.document_id,
                    "converted_source_revision_id": converted_source_id,
                    "ordinal": parser_unit.ordinal,
                    "source_purpose": TENDER_SOURCE_PURPOSE,
                    "original_ref": original_ref,
                    "source_locator": parser_unit.locator,
                    "original_object": original,
                    "ocr_text_object": ocr_object,
                    "model_contract": enrichment.model_contract,
                    "operation_contract": enrichment.operation_contract,
                }))?;
                let image_payload_sha = sha256_hex(&image_payload);
                image_set_digests.push(image_payload_sha.clone());
                image_artifacts.push(TenderImageArtifactRevision {
                    id: image_id,
                    ordinal: parser_unit.ordinal,
                    original_ref: original_ref.clone(),
                    original,
                    ocr_text: ocr_object,
                    model_contract: enrichment.model_contract,
                    operation_contract: enrichment.operation_contract,
                    source_locator: parser_unit.locator.clone(),
                    canonical_payload: image_payload,
                    content_sha256: image_payload_sha,
                });
                (
                    PublishedSourceUnitKind::ImageOcrRegion,
                    ocr_text,
                    Some(image_id),
                )
            } else {
                (
                    map_non_image_kind(&parser_unit.kind)?,
                    parser_unit.text.clone(),
                    None,
                )
            };
            if text.is_empty() {
                return Err(TenderDocumentProcessError::StructuredSource(format!(
                    "unit {} has empty text",
                    parser_unit.key
                )));
            }
            let span = SourceUnitSpanV2 {
                schema_version: 2,
                project_id: document.project_id,
                document_id: document.document_id,
                converted_source_revision_id: converted_source_id,
                parser_unit_key: parser_unit.key.clone(),
                parser_ordinal: parser_unit.ordinal,
                source_purpose: TENDER_SOURCE_PURPOSE.into(),
                locator: parser_unit.locator.clone(),
                tender_image_artifact_revision_id: image_artifact_id,
            };
            let span_bytes = canonical_json(&span)?;
            let span_sha = sha256_hex(&span_bytes);
            let lineage_id = stable_uuid(
                b"tender-source-unit-lineage-v2",
                format!("{}:{}", document.document_id, parser_unit.key).as_bytes(),
            );
            let revision_id = stable_uuid(
                b"tender-source-unit-revision-v2",
                format!(
                    "{}:{}:{}:{}",
                    lineage_id, request.request_revision, converted_source_id, span_sha
                )
                .as_bytes(),
            );
            let text_sha = sha256_hex(text.as_bytes());
            let canonical_payload = canonical_json(&json!({
                "schema_version": 1,
                "source_unit_revision_id": revision_id,
                "source_unit_lineage_id": lineage_id,
                "revision": request.request_revision,
                "project_id": document.project_id,
                "document_id": document.document_id,
                "converted_source_revision_id": converted_source_id,
                "unit_kind": unit_kind,
                "ordinal": parser_unit.ordinal,
                "source_purpose": TENDER_SOURCE_PURPOSE,
                "source_span_v2": span,
                "source_span_sha256": span_sha,
                "text_sha256": text_sha,
            }))?;
            let content_sha = sha256_hex(&canonical_payload);
            source_units.push(SourceUnitRevision {
                id: revision_id,
                lineage_id,
                revision: request.request_revision,
                ordinal: parser_unit.ordinal,
                unit_kind,
                source_span_v2: span,
                source_span_sha256: span_sha,
                text_utf8: text,
                text_sha256: text_sha,
                canonical_payload,
                content_sha256: content_sha,
            });
        }

        image_set_digests.sort();
        let image_asset_set_sha256 = sha256_hex(image_set_digests.join("").as_bytes());
        Ok(TenderDocumentPublication {
            request: request.clone(),
            project_id: document.project_id,
            document_id: document.document_id,
            document_sha256: document.document_sha256.clone(),
            converted_source: ConvertedTenderSourceRevision {
                id: converted_source_id,
                revision: request.request_revision,
                source_object,
                canonical_payload: source_payload,
                converter_contract_sha256: document.converter_contract_sha256.clone(),
                image_asset_set_sha256,
            },
            image_artifacts,
            source_units,
        })
    }

    async fn abandon_all(&self, objects: &[FrozenObjectIdentity]) {
        for object in objects {
            self.repository.abandon_staged_object(object).await;
        }
    }
}

#[derive(Clone)]
pub struct PgTenderDocumentProcessRepository {
    pool: sqlx::PgPool,
}

impl PgTenderDocumentProcessRepository {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TenderDocumentProcessRepository for PgTenderDocumentProcessRepository {
    async fn load_frozen_document(
        &self,
        payload: &BidAuthoringJobPayloadV2,
    ) -> Result<Option<FrozenTenderDocument>, TenderDocumentProcessError> {
        let (request, project_id, document_id) = match payload {
            BidAuthoringJobPayloadV2::TenderDocumentProcess {
                request,
                project_id,
                document_revision_id,
            } => (request, *project_id, *document_revision_id),
            _ => return Err(TenderDocumentProcessError::WrongJobKind),
        };
        let row = storage::bid_authoring_v2::load_tender_document_process_input_v2(
            &self.pool,
            request.request_artifact_id,
            request.request_revision,
            &request.frozen_input_sha256,
        )
        .await
        .map_err(|error| TenderDocumentProcessError::Publication(error.to_string()))?;
        let Some(row) = row else {
            return Ok(None);
        };
        if row.project_id != project_id || row.document_id != document_id {
            return Err(TenderDocumentProcessError::FrozenInputMismatch(
                "typed request scope differs from transport".into(),
            ));
        }
        if row.original_object_ref != format!("objects/{}", row.document_sha256) {
            return Err(TenderDocumentProcessError::FrozenInputMismatch(
                "original ObjectRegistry reference differs".into(),
            ));
        }
        let bytes = storage::read_blob(&row.document_sha256)
            .map_err(|_| TenderDocumentProcessError::MissingDocument)?;
        if i64::try_from(bytes.len()).ok() != Some(row.byte_length) {
            return Err(TenderDocumentProcessError::FrozenInputMismatch(
                "original byte length differs from ObjectRegistry".into(),
            ));
        }
        Ok(Some(FrozenTenderDocument {
            request: BidAuthoringRequestIdentityV2 {
                request_artifact_id: row.request_artifact_id,
                request_revision: row.request_revision,
                frozen_input_sha256: row.frozen_input_sha256,
            },
            project_id: row.project_id,
            document_id: row.document_id,
            document_sha256: row.document_sha256,
            role_revision_id: row.role_revision_id,
            role_revision_sha256: row.role_revision_sha256,
            converter_contract_id: row.converter_contract_id,
            converter_contract_sha256: row.converter_contract_sha256,
            file_name: row.file_name,
            media_type: row.media_type,
            bytes,
        }))
    }

    async fn stage_object(
        &self,
        _owner_id: Uuid,
        _occurrence: &str,
        media_type: &str,
        bytes: &[u8],
    ) -> Result<FrozenObjectIdentity, TenderDocumentProcessError> {
        if bytes.is_empty() || bytes.len() > MAX_TENDER_IMAGE_BYTES.max(MAX_TENDER_SOURCE_TEXT_BYTES)
        {
            return Err(TenderDocumentProcessError::ObjectFreeze(
                "object byte length is outside the bounded contract".into(),
            ));
        }
        let sha256 = sha256_hex(bytes);
        let object_ref = storage::object_ref(&sha256);
        let staging_id = Uuid::new_v4();
        storage::stage_object_upload(
            &self.pool,
            staging_id,
            &object_ref,
            &sha256,
            media_type,
            bytes.len() as i64,
            TENDER_PROCESS_ACTOR,
        )
        .await
        .map_err(|error| TenderDocumentProcessError::ObjectFreeze(error.to_string()))?;
        if let Err(error) = storage::write_blob_off_runtime(&sha256, bytes) {
            let _ = storage::abandon_object_upload(
                &self.pool,
                staging_id,
                TENDER_PROCESS_ACTOR,
            )
            .await;
            return Err(TenderDocumentProcessError::ObjectFreeze(error.to_string()));
        }
        Ok(FrozenObjectIdentity {
            staging_id,
            object_ref,
            sha256,
            media_type: media_type.to_string(),
            byte_length: bytes.len() as i64,
        })
    }

    async fn abandon_staged_object(&self, object: &FrozenObjectIdentity) {
        let _ = storage::abandon_object_upload(
            &self.pool,
            object.staging_id,
            TENDER_PROCESS_ACTOR,
        )
        .await;
    }

    async fn publish(
        &self,
        publication: TenderDocumentPublication,
    ) -> Result<TenderDocumentProcessReceipt, TenderDocumentProcessError> {
        let source = json!({
            "id": publication.converted_source.id,
            "revision": publication.converted_source.revision,
            "staging_id": publication.converted_source.source_object.staging_id,
            "object_ref": publication.converted_source.source_object.object_ref,
            "sha256": publication.converted_source.source_object.sha256,
            "media_type": publication.converted_source.source_object.media_type,
            "byte_length": publication.converted_source.source_object.byte_length,
            "canonical_payload_hex": hex::encode(&publication.converted_source.canonical_payload),
            "converter_contract_sha256": publication.converted_source.converter_contract_sha256,
            "image_asset_set_sha256": publication.converted_source.image_asset_set_sha256,
        });
        let images = Value::Array(
            publication
                .image_artifacts
                .iter()
                .map(|image| {
                    json!({
                        "id": image.id,
                        "ordinal": image.ordinal,
                        "source_purpose": TENDER_SOURCE_PURPOSE,
                        "source_locator": image.source_locator,
                        "original_ref": image.original_ref,
                        "original_staging_id": image.original.staging_id,
                        "original_object_ref": image.original.object_ref,
                        "original_sha256": image.original.sha256,
                        "original_media_type": image.original.media_type,
                        "original_byte_length": image.original.byte_length,
                        "ocr_staging_id": image.ocr_text.staging_id,
                        "ocr_object_ref": image.ocr_text.object_ref,
                        "ocr_sha256": image.ocr_text.sha256,
                        "ocr_media_type": image.ocr_text.media_type,
                        "ocr_byte_length": image.ocr_text.byte_length,
                        "model_contract_id": image.model_contract.id,
                        "model_contract_sha256": image.model_contract.sha256,
                        "model_contract_payload_hex": hex::encode(&image.model_contract.canonical_payload),
                        "operation_contract_id": image.operation_contract.id,
                        "operation_contract_sha256": image.operation_contract.sha256,
                        "operation_contract_payload_hex": hex::encode(&image.operation_contract.canonical_payload),
                        "canonical_payload_hex": hex::encode(&image.canonical_payload),
                        "content_sha256": image.content_sha256,
                    })
                })
                .collect(),
        );
        let units = Value::Array(
            publication
                .source_units
                .iter()
                .map(|unit| {
                    let span_payload = serde_json::to_vec(&unit.source_span_v2)
                        .expect("validated SourceUnitSpanV2 must serialize");
                    json!({
                        "id": unit.id,
                        "lineage_id": unit.lineage_id,
                        "revision": unit.revision,
                        "ordinal": unit.ordinal,
                        "unit_kind": unit.unit_kind.as_str(),
                        "source_purpose": TENDER_SOURCE_PURPOSE,
                        "source_span_v2": unit.source_span_v2,
                        "source_span_payload_hex": hex::encode(span_payload),
                        "source_span_sha256": unit.source_span_sha256,
                        "image_artifact_id": unit.source_span_v2.tender_image_artifact_revision_id,
                        "text_utf8_hex": hex::encode(unit.text_utf8.as_bytes()),
                        "text_sha256": unit.text_sha256,
                        "canonical_payload_hex": hex::encode(&unit.canonical_payload),
                        "content_sha256": unit.content_sha256,
                    })
                })
                .collect(),
        );
        let result = storage::bid_authoring_v2::publish_tender_document_process_v2(
            &self.pool,
            storage::bid_authoring_v2::PublishTenderDocumentProcessV2 {
                request_artifact_id: publication.request.request_artifact_id,
                request_revision: publication.request.request_revision,
                frozen_input_sha256: &publication.request.frozen_input_sha256,
                project_id: publication.project_id,
                document_id: publication.document_id,
                document_sha256: &publication.document_sha256,
                source: &source,
                images: &images,
                units: &units,
                actor: TENDER_PROCESS_ACTOR,
            },
        )
        .await
        .map_err(|error| TenderDocumentProcessError::Publication(error.to_string()))?;
        serde_json::from_value(result)
            .map_err(|error| TenderDocumentProcessError::Publication(error.to_string()))
    }
}

fn verify_frozen_document(
    document: &FrozenTenderDocument,
    request: &BidAuthoringRequestIdentityV2,
    project_id: Uuid,
    document_id: Uuid,
) -> Result<(), TenderDocumentProcessError> {
    if document.request != *request
        || document.project_id != project_id
        || document.document_id != document_id
    {
        return Err(TenderDocumentProcessError::FrozenInputMismatch(
            "request/project/document tuple differs".into(),
        ));
    }
    if sha256_hex(&document.bytes) != document.document_sha256 {
        return Err(TenderDocumentProcessError::FrozenInputMismatch(
            "document bytes digest differs".into(),
        ));
    }
    if document.role_revision_sha256.len() != 64
        || document.converter_contract_sha256.len() != 64
    {
        return Err(TenderDocumentProcessError::FrozenInputMismatch(
            "role or converter identity digest is malformed".into(),
        ));
    }
    Ok(())
}

fn validate_structured_units(
    units: &[StructuredSourceUnit],
) -> Result<(), TenderDocumentProcessError> {
    if units.is_empty() || units.len() > MAX_TENDER_SOURCE_UNITS {
        return Err(TenderDocumentProcessError::StructuredSource(
            "unit count is outside the bounded contract".into(),
        ));
    }
    let mut keys = HashSet::with_capacity(units.len());
    let mut total_text = 0usize;
    for (expected, unit) in units.iter().enumerate() {
        if unit.ordinal as usize != expected || !keys.insert(unit.key.as_str()) {
            return Err(TenderDocumentProcessError::StructuredSource(
                "unit ordering or key uniqueness is invalid".into(),
            ));
        }
        total_text = total_text.saturating_add(unit.text.len());
        if total_text > MAX_TENDER_SOURCE_TEXT_BYTES {
            return Err(TenderDocumentProcessError::StructuredSource(
                "unit text payload exceeds the bounded contract".into(),
            ));
        }
    }
    Ok(())
}

fn map_non_image_kind(
    kind: &StructuredSourceUnitKind,
) -> Result<PublishedSourceUnitKind, TenderDocumentProcessError> {
    match kind {
        StructuredSourceUnitKind::Section => Ok(PublishedSourceUnitKind::Section),
        StructuredSourceUnitKind::TableRow => Ok(PublishedSourceUnitKind::TableRow),
        // Parser table-region carries a bounded form/table definition. Rows are
        // independent table_row units; the region freezes the form structure.
        StructuredSourceUnitKind::TableRegion | StructuredSourceUnitKind::FormRegion => {
            Ok(PublishedSourceUnitKind::FormRegion)
        }
        StructuredSourceUnitKind::AttachmentRegion => {
            Ok(PublishedSourceUnitKind::AttachmentRegion)
        }
        StructuredSourceUnitKind::ImageRegion => Err(
            TenderDocumentProcessError::StructuredSource("unprocessed image unit".into()),
        ),
    }
}

pub fn tender_source_unit_can_be_bidder_evidence(_unit: &SourceUnitRevision) -> bool {
    false
}

fn canonical_json<T: Serialize>(value: &T) -> Result<Vec<u8>, TenderDocumentProcessError> {
    serde_json::to_vec(value)
        .map_err(|error| TenderDocumentProcessError::StructuredSource(error.to_string()))
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn stable_uuid(namespace: &[u8], material: &[u8]) -> Uuid {
    let mut hasher = Sha256::new();
    hasher.update(namespace);
    hasher.update([0]);
    hasher.update(material);
    let digest = hasher.finalize();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}
