//! Mandatory parse -> OCR -> ordered, complete, immutable input boundary.
use crate::analysis::{
    DocumentAvailability, DocumentRelation, DocumentRole, FrozenDocument, FrozenInput, Source,
};
use docparser::{ReadResult, SourceCompleteness, SourceContract, StructuredSourceUnitKind};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const FROZEN_SCHEMA_VERSION: u32 = 3;
pub const PARSER_CONTRACT_VERSION: &str = "source-v2";

#[derive(Debug, Clone)]
pub struct ParsedDocument {
    pub role: DocumentRole,
    pub document_id: String,
    pub parsed: ReadResult,
}
#[derive(Debug, Clone)]
pub struct FrozenBuildInput {
    pub project_id: String,
    pub document_set_id: String,
    pub documents: Vec<ParsedDocument>,
    pub document_relations: Vec<DocumentRelation>,
    pub decisions: Vec<Value>,
}
/// A successful caption is not an OCR receipt. The literal OCR termination and
/// persisted image reference must both be supplied, including for empty images.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenImageResult {
    pub document_id: String,
    pub document_revision: String,
    pub parser_version: String,
    pub unit_id: String,
    pub literal_text: String,
    pub completeness: String,
    pub finish_reason: String,
    pub done_seen: bool,
    pub transport_complete: bool,
    pub persisted_image_ref: String,
    pub provenance: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParseCoverage {
    pub expected_unit_keys: Vec<String>,
    pub published_unit_keys: Vec<String>,
    pub required_image_keys: Vec<String>,
    pub completed_image_keys: Vec<String>,
    pub complete: bool,
}

fn namespaced(document: &str, revision: &str, key: &str) -> String {
    format!("{document}:{revision}:{key}")
}

pub fn build_frozen_input(
    build: FrozenBuildInput,
    image_results: Vec<FrozenImageResult>,
    parser_contract_version: &str,
) -> Result<FrozenInput, String> {
    if build.project_id.trim().is_empty()
        || build.document_set_id.trim().is_empty()
        || parser_contract_version != PARSER_CONTRACT_VERSION
    {
        return Err(
            "frozen input requires project, document set and parser contract identity".into(),
        );
    }
    let mut images = BTreeMap::new();
    for image in image_results {
        let key = (image.document_id.clone(), image.unit_id.clone());
        if images.insert(key, image).is_some() {
            return Err("duplicate OCR result".into());
        }
    }
    let mut frozen = FrozenInput {
        schema_version: FROZEN_SCHEMA_VERSION,
        project_id: build.project_id,
        document_set_id: build.document_set_id,
        documents: vec![],
        document_relations: build.document_relations,
        source_units: vec![],
        structured_forms: vec![],
        decisions: build.decisions,
    };
    let mut document_ids = BTreeSet::new();
    for document in build.documents {
        if document.document_id.trim().is_empty()
            || !document_ids.insert(document.document_id.clone())
        {
            return Err("duplicate or missing document identity".into());
        }
        if !document.parsed.error.is_empty() {
            return Err(format!(
                "document {} parse failed: {}",
                document.document_id, document.parsed.error
            ));
        }
        let contract = docparser::parse_source_contract(&document.parsed)
            .map_err(|e| e.to_string())?
            .ok_or("source_contract V2 is required before freezing")?;
        let sections = super::parse::content_sections(&document.parsed.structured_source_units);
        let expected =
            super::parse::expected_unit_keys(&document.parsed.structured_source_units, &sections);
        let order = super::parse::publication_order(&document.parsed.structured_source_units);
        let mut finished = Vec::new();
        let mut required_images = Vec::new();
        let mut completed_images = Vec::new();
        for unit in &document.parsed.structured_source_units {
            let identity = contract
                .unit(&unit.key)
                .ok_or("source contract omitted a unit")?;
            if identity.completeness != SourceCompleteness::Complete {
                return Err(format!(
                    "unit {} is {:?}: {:?}",
                    unit.key, identity.completeness, identity.reasons
                ));
            }
            if !expected.contains(&unit.key) {
                continue;
            }
            let source_id = namespaced(
                &document.document_id,
                &contract.document_revision,
                &unit.key,
            );
            let mut locator =
                serde_json::to_value(identity.physical_locator.as_ref().unwrap_or(&unit.locator))
                    .map_err(|e| e.to_string())?;
            locator["unit_id"] = json!(unit.key);
            locator["document_revision"] = json!(contract.document_revision);
            locator["parser_version"] = json!(contract.parser_version);
            locator["section_id"] = json!(identity.section_id.as_deref().map(
                |section| namespaced(&document.document_id, &contract.document_revision, section)
            ));
            locator["parent_section_id"] = json!(identity.parent_section_id.as_deref().map(
                |section| namespaced(&document.document_id, &contract.document_revision, section)
            ));
            locator["heading_level"] = json!(identity.heading_level);
            locator["heading_path"] = json!(identity.heading_path);
            locator["physical_locator"] = json!(identity.physical_locator);
            locator["physical_path"] = json!(identity.physical_path);
            locator["physical_locator_unavailable_reason"] =
                json!(identity.physical_locator_unavailable_reason);
            locator["rendered_spans"] = json!(identity.rendered_spans);
            locator["completeness"] = json!("complete");
            locator["kind"] = json!(unit.kind);
            let text = if unit.kind == StructuredSourceUnitKind::ImageRegion {
                required_images.push(unit.key.clone());
                let result = images
                    .remove(&(document.document_id.clone(), unit.key.clone()))
                    .ok_or_else(|| format!("required OCR missing for {}", unit.key))?;
                validate_image_result(&result, &contract)?;
                let blank_page = docparser::physical_page(&unit.locator)
                    .and_then(|page| {
                        contract
                            .page_manifest
                            .iter()
                            .find(|entry| entry.page_ordinal == page)
                    })
                    .is_some_and(|page| page.classification == "blank");
                let blank_image = blank_page && result.literal_text.trim().is_empty();
                locator["blank_image"] = json!(blank_image);
                locator["vision_required"] = json!(!blank_image);
                locator["ocr_status"] = json!(if blank_image {
                    "blank"
                } else if result.literal_text.trim().is_empty() {
                    "empty_requires_vision"
                } else {
                    "complete"
                });
                locator["ocr_provenance"] = json!(result.provenance);
                locator["ocr_complete"] =
                    json!(blank_image || !result.literal_text.trim().is_empty());
                locator["image_available"] = json!(true);
                locator["image_ref"] = json!(result.persisted_image_ref);
                locator["ocr_receipt"] = json!({"finish_reason":result.finish_reason,"done_seen":result.done_seen,"transport_complete":result.transport_complete,"literal_text_sha256":hex::encode(Sha256::digest(result.literal_text.as_bytes()))});
                completed_images.push(unit.key.clone());
                result.literal_text
            } else {
                unit.text.clone()
            };
            if let Some(grid) = &unit.grid {
                docparser::validate_table_grid(grid).map_err(|e| e.to_string())?;
                let mut definition = serde_json::to_value(grid).map_err(|e| e.to_string())?;
                definition["schema_version"] = json!(3);
                definition["kind"] = json!("grid");
                definition["completeness"] = json!("complete");
                definition["physical_locator"] = json!(identity.physical_locator);
                for cell in definition["cells"]
                    .as_array_mut()
                    .ok_or("grid cells missing")?
                {
                    let role = identity
                        .header_cells
                        .iter()
                        .find(|header| cell["row"] == header.row && cell["column"] == header.column)
                        .map(|h| h.role.as_str())
                        .unwrap_or("none");
                    cell["header_role"] = json!(role);
                }
                let table_id = namespaced(
                    &document.document_id,
                    &contract.document_revision,
                    identity
                        .table_id
                        .as_deref()
                        .ok_or("grid table identity missing")?,
                );
                frozen.structured_forms.push(json!({"form_definition_revision_id":table_id,"source_unit_revision_id":source_id,"definition":definition}));
            }
            finished.push((
                unit.key.clone(),
                Source {
                    source_unit_revision_id: source_id,
                    document_id: document.document_id.clone(),
                    text,
                    locator,
                    ordinal: identity.ordinal as usize,
                },
            ));
        }
        let published = finished
            .iter()
            .map(|(key, _)| key.clone())
            .collect::<BTreeSet<_>>();
        super::parse::assert_complete(&expected, &published)?;
        let ordered = super::parse::place_in_publication_order(&order, finished)?;
        let published_order = ordered
            .iter()
            .map(|s| s.locator["unit_id"].as_str().unwrap_or("").to_string())
            .collect::<Vec<_>>();
        super::parse::assert_publication_order(&order, &published_order)?;
        let coverage = ParseCoverage {
            expected_unit_keys: order.clone(),
            published_unit_keys: published_order,
            required_image_keys: required_images,
            completed_image_keys: completed_images,
            complete: true,
        };
        frozen.documents.push(FrozenDocument {
            document_id: document.document_id,
            document_revision: contract.document_revision.clone(),
            parser_contract_version: parser_contract_version.into(),
            page_count: contract.page_manifest.len(),
            role: document.role,
            availability: DocumentAvailability::Available,
            source_contract: Some(contract),
            parse_coverage: Some(coverage),
        });
        frozen.source_units.extend(ordered);
    }
    if !images.is_empty() {
        return Err("OCR results contain an unknown document or non-required unit".into());
    }
    validate_frozen_input_contract(&frozen)?;
    Ok(frozen)
}
fn validate_image_result(
    result: &FrozenImageResult,
    contract: &SourceContract,
) -> Result<(), String> {
    if result.document_revision != contract.document_revision
        || result.parser_version != contract.parser_version
    {
        return Err("OCR revision does not match parsed source".into());
    }
    if result.completeness != "complete"
        || !result.done_seen
        || !result.transport_complete
        || !matches!(result.finish_reason.as_str(), "stop" | "end_turn")
    {
        return Err(format!("required OCR {} is incomplete", result.unit_id));
    }
    if result.persisted_image_ref.trim().is_empty() || result.provenance.is_null() {
        return Err("OCR requires persisted image and literal-text provenance".into());
    }
    Ok(())
}

/// Runtime entry gate. A caller cannot label an arbitrary payload V3 and skip
/// parse identity, completeness, physical coverage or mandatory OCR checks.
pub fn validate_frozen_input_contract(input: &FrozenInput) -> Result<(), String> {
    if input.schema_version != FROZEN_SCHEMA_VERSION
        || input.project_id.trim().is_empty()
        || input.document_set_id.trim().is_empty()
    {
        return Err("FrozenInput V3 with complete identity is required".into());
    }
    input.validate_document_relations()?;
    if input.documents.is_empty() {
        return Err("frozen input has no parsed documents".into());
    }
    let mut documents = BTreeSet::new();
    let mut all_units = BTreeSet::new();
    let mut table_ids = BTreeSet::new();
    for document in &input.documents {
        let id = document.document_id.as_str();
        if id.trim().is_empty() || document.availability != DocumentAvailability::Available {
            return Err("frozen document identity or availability invalid".into());
        }
        if !documents.insert(id) {
            return Err("duplicate frozen document".into());
        }
        if document.parser_contract_version != PARSER_CONTRACT_VERSION {
            return Err("unsupported parser contract version".into());
        }
        let contract = document
            .source_contract
            .as_ref()
            .ok_or("required source contract missing")?;
        docparser::validate_glyph_normalizations(contract).map_err(|error| error.to_string())?;
        if contract.schema_version != 2
            || contract.document_revision.len() != 64
            || !contract
                .document_revision
                .bytes()
                .all(|b| b.is_ascii_hexdigit())
            || contract.parser_version.trim().is_empty()
            || document.document_revision != contract.document_revision
        {
            return Err("frozen source revision is invalid".into());
        }
        let coverage = document
            .parse_coverage
            .as_ref()
            .ok_or("required parse coverage missing")?;
        let expected = coverage
            .expected_unit_keys
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        if !coverage.complete || expected.len() != coverage.expected_unit_keys.len() {
            return Err("frozen parse coverage is incomplete or duplicated".into());
        }
        super::parse::assert_complete(
            &expected,
            &coverage.published_unit_keys.iter().cloned().collect(),
        )?;
        super::parse::assert_publication_order(
            &coverage.expected_unit_keys,
            &coverage.published_unit_keys,
        )?;
        let published = input
            .source_units
            .iter()
            .filter(|s| s.document_id == id)
            .collect::<Vec<_>>();
        let actual = published
            .iter()
            .map(|s| s.locator["unit_id"].as_str().unwrap_or("").to_string())
            .collect::<Vec<_>>();
        super::parse::assert_publication_order(&coverage.published_unit_keys, &actual)?;
        let contract_ids = contract
            .units
            .iter()
            .map(|u| u.unit_id.as_str())
            .collect::<BTreeSet<_>>();
        if contract_ids.len() != contract.units.len()
            || contract_ids.len() != expected.len()
            || !expected
                .iter()
                .all(|key| contract_ids.contains(key.as_str()))
        {
            return Err("frozen unit index differs from source contract".into());
        }
        for (ordinal, identity) in contract.units.iter().enumerate() {
            if identity.ordinal as usize != ordinal
                || identity.completeness != SourceCompleteness::Complete
            {
                return Err("frozen contract contains unordered or incomplete units".into());
            }
        }
        let empty_text_digest = hex::encode(Sha256::digest(b""));
        let structural = |identity: &docparser::SourceUnitIdentity| {
            identity.kind == StructuredSourceUnitKind::Section
                && identity.text_sha256 == empty_text_digest
                && identity.table_id.is_none()
                && identity.section_id.is_some()
                && contract.units.iter().any(|other| {
                    other.section_id == identity.section_id
                        && matches!(
                            other.kind,
                            StructuredSourceUnitKind::TableRegion
                                | StructuredSourceUnitKind::TableRow
                                | StructuredSourceUnitKind::FormRegion
                                | StructuredSourceUnitKind::ImageRegion
                        )
                })
        };
        let independently_expected = contract
            .units
            .iter()
            .filter(|identity| !structural(identity))
            .map(|identity| identity.unit_id.clone())
            .collect::<Vec<_>>();
        super::parse::assert_publication_order(
            &independently_expected,
            &coverage.expected_unit_keys,
        )?;
        if document.page_count != contract.page_manifest.len() {
            return Err("frozen physical page count differs from parsed inventory".into());
        }
        let required = coverage
            .required_image_keys
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let completed = coverage
            .completed_image_keys
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        if required != completed
            || required.len() != coverage.required_image_keys.len()
            || completed.len() != coverage.completed_image_keys.len()
        {
            return Err("required OCR coverage is incomplete".into());
        }
        let mut actual_images = BTreeSet::new();
        for source in published {
            let key = source.locator["unit_id"]
                .as_str()
                .ok_or("unit key missing")?;
            let identity = contract
                .unit(key)
                .ok_or("source unit missing from contract")?;
            if source.source_unit_revision_id != namespaced(id, &contract.document_revision, key)
                || !all_units.insert(source.source_unit_revision_id.as_str())
                || source.ordinal != identity.ordinal as usize
                || source.locator["document_revision"] != contract.document_revision
                || source.locator["parser_version"] != contract.parser_version
                || source.locator["completeness"] != "complete"
            {
                return Err("frozen unit identity/order/completeness mismatch".into());
            }
            if let Some(physical) = &identity.physical_locator {
                let physical = serde_json::to_value(physical).map_err(|e| e.to_string())?;
                for (field, value) in physical
                    .as_object()
                    .ok_or("physical locator is not an object")?
                {
                    if source.locator.get(field) != Some(value) {
                        return Err(
                            "frozen physical coordinate projection differs from parser contract"
                                .into(),
                        );
                    }
                }
            }
            if source.locator["section_id"]
                != json!(identity.section_id.as_deref().map(|section| namespaced(
                    id,
                    &contract.document_revision,
                    section
                )))
                || source.locator["parent_section_id"]
                    != json!(
                        identity
                            .parent_section_id
                            .as_deref()
                            .map(|section| namespaced(id, &contract.document_revision, section))
                    )
                || source.locator["heading_path"] != identity.heading_path
                || source.locator["heading_level"] != json!(identity.heading_level)
                || source.locator["physical_locator"] != json!(identity.physical_locator)
                || source.locator["physical_path"] != json!(identity.physical_path)
                || source.locator["physical_locator_unavailable_reason"]
                    != json!(identity.physical_locator_unavailable_reason)
                || source.locator["rendered_spans"] != json!(identity.rendered_spans)
            {
                return Err(
                    "frozen source section or physical coordinates differ from parser contract"
                        .into(),
                );
            }
            if source.locator["kind"]
                != serde_json::to_value(&identity.kind).map_err(|e| e.to_string())?
            {
                return Err("frozen source carrier kind changed".into());
            }
            if identity.kind != StructuredSourceUnitKind::ImageRegion
                && identity.text_sha256 != hex::encode(Sha256::digest(source.text.as_bytes()))
            {
                return Err("frozen source text differs from parser contract".into());
            }
            if identity.kind == StructuredSourceUnitKind::ImageRegion {
                actual_images.insert(key.to_string());
                let blank_page = identity
                    .physical_locator
                    .as_ref()
                    .and_then(docparser::physical_page)
                    .and_then(|page| {
                        contract
                            .page_manifest
                            .iter()
                            .find(|entry| entry.page_ordinal == page)
                    })
                    .is_some_and(|page| page.classification == "blank");
                let blank_image = blank_page && source.text.trim().is_empty();
                let ocr_status = if blank_image {
                    "blank"
                } else if source.text.trim().is_empty() {
                    "empty_requires_vision"
                } else {
                    "complete"
                };
                if source.locator["blank_image"] != json!(blank_image)
                    || source.locator["vision_required"] != json!(!blank_image)
                    || source.locator["ocr_status"] != ocr_status
                {
                    return Err(
                        "frozen visual/OCR requirement flags differ from the original carrier"
                            .into(),
                    );
                }
                let receipt = &source.locator["ocr_receipt"];
                if receipt["literal_text_sha256"]
                    != hex::encode(Sha256::digest(source.text.as_bytes()))
                    || source.locator["ocr_complete"]
                        != json!(blank_image || !source.text.trim().is_empty())
                    || source.locator["image_available"] != true
                    || source.locator["image_ref"]
                        .as_str()
                        .is_none_or(|s| s.is_empty())
                    || source.locator["ocr_provenance"].is_null()
                    || receipt["done_seen"] != true
                    || receipt["transport_complete"] != true
                    || !matches!(receipt["finish_reason"].as_str(), Some("stop" | "end_turn"))
                {
                    return Err("frozen image is missing complete literal OCR".into());
                }
            }
            if let Some(table) = &identity.table_id {
                let table = namespaced(id, &contract.document_revision, table);
                let definition = super::evidence::table_definition(input, &table)?;
                let form = input
                    .structured_forms
                    .iter()
                    .find(|f| f["form_definition_revision_id"] == table)
                    .ok_or("required grid missing")?;
                if form["source_unit_revision_id"] != source.source_unit_revision_id
                    || definition["physical_locator"] != json!(identity.physical_locator)
                    || definition["completeness"] != "complete"
                {
                    return Err("frozen grid ownership/completeness mismatch".into());
                }
                let grid: docparser::TableGrid =
                    serde_json::from_value(definition.clone()).map_err(|e| e.to_string())?;
                docparser::validate_table_grid(&grid).map_err(|e| e.to_string())?;
                if identity.grid_sha256.as_deref()
                    != Some(docparser::table_grid_digest(&grid).as_str())
                {
                    return Err("frozen grid differs from parser contract".into());
                }
                for cell in definition["cells"].as_array().ok_or("grid cells missing")? {
                    let expected_role = identity
                        .header_cells
                        .iter()
                        .find(|header| cell["row"] == header.row && cell["column"] == header.column)
                        .map(|header| header.role.as_str())
                        .unwrap_or("none");
                    if cell["header_role"] != expected_role {
                        return Err("frozen grid header role differs from parser contract".into());
                    }
                }
                table_ids.insert(table);
            }
        }
        if actual_images != required {
            return Err("OCR manifest does not equal image units".into());
        }
        let mut physical = BTreeSet::new();
        for (ordinal, page) in contract.page_manifest.iter().enumerate() {
            if page.page_ordinal as usize != ordinal
                || !matches!(page.classification.as_str(), "text" | "scanned" | "blank")
            {
                return Err("invalid frozen physical page manifest".into());
            }
            for key in &page.unit_ids {
                let identity = contract.unit(key).ok_or("unknown physical page unit")?;
                if !physical.insert(key)
                    || identity
                        .physical_locator
                        .as_ref()
                        .and_then(docparser::physical_page)
                        != Some(page.page_ordinal)
                {
                    return Err("physical page identity mismatch".into());
                }
            }
            let expected_images = page
                .unit_ids
                .iter()
                .filter(|key| {
                    contract
                        .unit(key)
                        .is_some_and(|unit| unit.kind == StructuredSourceUnitKind::ImageRegion)
                })
                .collect::<BTreeSet<_>>();
            let actual_images = page.image_unit_ids.iter().collect::<BTreeSet<_>>();
            if expected_images != actual_images
                || actual_images.len() != page.image_unit_ids.len()
                || !page.image_unit_ids.iter().all(|key| required.contains(key))
            {
                return Err("page image inventory differs from mandatory OCR units".into());
            }
        }
        if !contract.page_manifest.is_empty() && physical.len() != contract.units.len() {
            return Err("physical page coverage is incomplete".into());
        }
    }
    if all_units.len() != input.source_units.len()
        || table_ids.len() != input.structured_forms.len()
    {
        return Err("unowned frozen source units or tables".into());
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct RawTenderDocument {
    pub role: DocumentRole,
    pub document_id: String,
    pub file_name: String,
    pub bytes: Vec<u8>,
}
#[derive(Debug, Clone)]
pub struct PrepareTenderInput {
    pub project_id: String,
    pub document_set_id: String,
    pub documents: Vec<RawTenderDocument>,
    pub document_relations: Vec<DocumentRelation>,
    pub decisions: Vec<Value>,
    pub parser_contract_version: String,
}

#[async_trait::async_trait]
pub trait TenderSourceParser: Send + Sync {
    async fn parse(
        &self,
        document: &RawTenderDocument,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<ReadResult, String>;
}
pub struct ConfiguredTenderSourceParser;
#[async_trait::async_trait]
impl TenderSourceParser for ConfiguredTenderSourceParser {
    async fn parse(
        &self,
        document: &RawTenderDocument,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<ReadResult, String> {
        docparser::convert_tender_source(&document.file_name, document.bytes.clone(), cancel)
            .await
            .map_err(|e| e.to_string())
    }
}

#[async_trait::async_trait]
pub trait TenderImageStore: Send + Sync {
    /// Persist under an object owner/staging lease; bare unmanaged writes are invalid.
    async fn persist_image(
        &self,
        document_id: &str,
        unit_id: &str,
        image: &docparser::ImageRef,
    ) -> Result<String, String>;
}
#[async_trait::async_trait]
pub trait TenderImageProcessor: Send + Sync {
    async fn process(
        &self,
        document_id: &str,
        contract: &SourceContract,
        unit: &docparser::StructuredSourceUnit,
        image: &docparser::ImageRef,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<FrozenImageResult, String>;
}
pub struct ConfiguredTenderImageProcessor<'a> {
    pub image_store: &'a dyn TenderImageStore,
}
#[async_trait::async_trait]
impl TenderImageProcessor for ConfiguredTenderImageProcessor<'_> {
    async fn process(
        &self,
        document_id: &str,
        contract: &SourceContract,
        unit: &docparser::StructuredSourceUnit,
        image: &docparser::ImageRef,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> Result<FrozenImageResult, String> {
        let source_type = if matches!(
            &unit.locator,
            docparser::StructuredSourceLocator::Image {
                page_ordinal: Some(_),
                ..
            }
        ) {
            "scanned_pdf"
        } else {
            ""
        };
        let ocr = tokio::select! {
            _=cancel.cancelled()=>return Err("tender OCR cancelled".into()),
            result=knowledge::enrichment::literal_ocr_regions_async(&image.data,&image.mime_type,source_type)=>result.map_err(|e|e.to_string())?,
        };
        if !ocr.complete {
            return Err(format!(
                "required OCR {} remains incomplete after {} bounded physical calls; regional overlap or termination needs review",
                unit.key, ocr.physical_calls
            ));
        }
        let persisted_image_ref = self
            .image_store
            .persist_image(document_id, &unit.key, image)
            .await?;
        Ok(FrozenImageResult {
            document_id: document_id.into(),
            document_revision: contract.document_revision.clone(),
            parser_version: contract.parser_version.clone(),
            unit_id: unit.key.clone(),
            literal_text: ocr.text,
            completeness: "complete".into(),
            finish_reason: "stop".into(),
            done_seen: true,
            transport_complete: true,
            persisted_image_ref,
            provenance: json!({"kind":"literal_ocr","unit_id":unit.key,"parser_version":contract.parser_version,"original_ref":image.original_ref,"physical_calls":ocr.physical_calls,"regions":ocr.fragments}),
        })
    }
}

/// Actual internal producer used by the preparation command. Parsing cannot be
/// replaced by LLM summaries, and image completion order cannot change identity.
pub async fn prepare_tender_input(
    request: PrepareTenderInput,
    images: &dyn TenderImageProcessor,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<FrozenInput, String> {
    prepare_tender_input_with(request, &ConfiguredTenderSourceParser, images, cancel).await
}
pub async fn prepare_tender_input_with(
    request: PrepareTenderInput,
    parser: &dyn TenderSourceParser,
    images: &dyn TenderImageProcessor,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<FrozenInput, String> {
    use sha2::{Digest, Sha256};
    if request.documents.is_empty() {
        return Err("tender preparation requires source documents".into());
    }
    let mut documents = Vec::new();
    let mut image_results = Vec::new();
    let mut ids = BTreeSet::new();
    for raw in request.documents {
        if raw.document_id.trim().is_empty()
            || !ids.insert(raw.document_id.clone())
            || raw.bytes.is_empty()
        {
            return Err("raw tender document identity/bytes are missing or duplicated".into());
        }
        if cancel.is_cancelled() {
            return Err("tender preparation cancelled".into());
        }
        let parsed = parser.parse(&raw, cancel).await?;
        let contract = docparser::parse_source_contract(&parsed)
            .map_err(|e| e.to_string())?
            .ok_or("DocReader did not return mandatory source contract V2")?;
        if contract.document_revision != hex::encode(Sha256::digest(&raw.bytes)) {
            return Err("DocReader source revision does not match submitted bytes".into());
        }
        let mut required = Vec::new();
        for unit in parsed
            .structured_source_units
            .iter()
            .filter(|u| u.kind == StructuredSourceUnitKind::ImageRegion)
        {
            let original = match &unit.locator {
                docparser::StructuredSourceLocator::Image { original_ref, .. } => original_ref,
                _ => return Err("image unit has no image locator".into()),
            };
            let mut matches = parsed
                .images
                .iter()
                .filter(|image| &image.original_ref == original);
            let image = matches
                .next()
                .ok_or_else(|| format!("required image bytes missing for {}", unit.key))?;
            if matches.next().is_some() || image.data.is_empty() {
                return Err("required image has duplicate or empty bytes".into());
            }
            required.push((unit, image));
        }
        let represented = required
            .iter()
            .map(|(_, image)| image.original_ref.as_str())
            .collect::<BTreeSet<_>>();
        if parsed
            .images
            .iter()
            .any(|image| !represented.contains(image.original_ref.as_str()))
        {
            return Err(
                "parsed image has no ordered source unit; cannot silently omit mandatory OCR"
                    .into(),
            );
        }
        let completed = super::parse::map_concurrent(
            required,
            super::parse::image_concurrency(),
            |(unit, image)| images.process(&raw.document_id, &contract, unit, image, cancel),
        )
        .await;
        for result in completed {
            image_results.push(result?);
        }
        documents.push(ParsedDocument {
            role: raw.role,
            document_id: raw.document_id,
            parsed,
        });
    }
    build_frozen_input(
        FrozenBuildInput {
            project_id: request.project_id,
            document_set_id: request.document_set_id,
            documents,
            document_relations: request.document_relations,
            decisions: request.decisions,
        },
        image_results,
        &request.parser_contract_version,
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    fn fixture(index: usize) -> ReadResult {
        let fixtures: Value = serde_json::from_str(include_str!(
            "../../../docparser/tests/fixtures/python-source-contract-v2.json"
        ))
        .unwrap();
        read_fixture_row(&fixtures[index])
    }
    fn read_fixture_row(row: &Value) -> ReadResult {
        ReadResult {
            markdown: row["markdown"].as_str().unwrap().into(),
            structured_source_units: serde_json::from_value(row["structured_source_units"].clone())
                .unwrap(),
            metadata: serde_json::from_value(row["metadata"].clone()).unwrap(),
            images: row["images"]
                .as_array()
                .unwrap()
                .iter()
                .map(|image| docparser::ImageRef {
                    original_ref: image["original_ref"].as_str().unwrap().into(),
                    filename: image["original_ref"].as_str().unwrap().into(),
                    mime_type: "image/jpeg".into(),
                    data: hex::decode(image["hex"].as_str().unwrap()).unwrap(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }
    fn results(parsed: &ReadResult) -> Vec<FrozenImageResult> {
        let contract = docparser::parse_source_contract(parsed).unwrap().unwrap();
        parsed
            .structured_source_units
            .iter()
            .filter(|u| u.kind == StructuredSourceUnitKind::ImageRegion)
            .rev()
            .map(|u| FrozenImageResult {
                document_id: "document".into(),
                document_revision: contract.document_revision.clone(),
                parser_version: contract.parser_version.clone(),
                unit_id: u.key.clone(),
                literal_text: format!("OCR {}", u.ordinal),
                completeness: "complete".into(),
                finish_reason: "stop".into(),
                done_seen: true,
                transport_complete: true,
                persisted_image_ref: format!(
                    "objects/{}",
                    hex::encode(Sha256::digest(&parsed.images.iter().find(|image|matches!(&u.locator,docparser::StructuredSourceLocator::Image{original_ref,..} if original_ref==&image.original_ref)).expect("fixture image bytes").data))
                ),
                provenance: json!({"unit_id":u.key,"literal":true}),
            })
            .collect()
    }
    fn build(parsed: ReadResult, images: Vec<FrozenImageResult>) -> Result<FrozenInput, String> {
        build_frozen_input(
            FrozenBuildInput {
                project_id: "project".into(),
                document_set_id: "set".into(),
                documents: vec![ParsedDocument {
                    role: crate::analysis::DocumentRole::Unspecified,
                    document_id: "document".into(),
                    parsed,
                }],
                document_relations: vec![],
                decisions: vec![],
            },
            images,
            "source-v2",
        )
    }
    #[test]
    fn python_excel_protobuf_preserves_native_values_through_freeze() {
        use prost::Message;
        let wire = include_bytes!("../../../docparser/tests/fixtures/python-excel-wire.pb");
        let response = docparser::proto::ReadResponse::decode(wire.as_slice()).unwrap();
        let parsed = ReadResult::try_from(response).unwrap();
        let contract = docparser::parse_source_contract(&parsed).unwrap().unwrap();
        // Check the protobuf fields themselves before Frozen prefers the physical
        // locator in source_contract; metadata JSON must not mask wire field loss.
        for unit in &parsed.structured_source_units {
            if let docparser::StructuredSourceLocator::Spreadsheet { cells, .. } = &unit.locator {
                let physical = serde_json::to_value(
                    contract
                        .unit(&unit.key)
                        .unwrap()
                        .physical_locator
                        .as_ref()
                        .unwrap(),
                )
                .unwrap();
                assert_eq!(serde_json::to_value(cells).unwrap(), physical["cells"]);
            }
        }
        let frozen = build(parsed, vec![]).unwrap();
        validate_frozen_input_contract(&frozen).unwrap();
        let native_grid = frozen
            .source_units
            .iter()
            .find(|source| source.locator["unit_id"] == "sheet:0:used")
            .unwrap();
        assert!(native_grid.text.is_empty());
        let cells = native_grid.locator["cells"].as_array().unwrap();
        assert_eq!(cells.len(), 8);
        assert_eq!(
            native_grid.locator["cells"],
            native_grid.locator["physical_locator"]["cells"]
        );
        let form = frozen
            .structured_forms
            .iter()
            .find(|form| form["source_unit_revision_id"] == native_grid.source_unit_revision_id)
            .unwrap();
        assert_eq!(
            form["definition"]["physical_locator"]["cells"],
            native_grid.locator["cells"]
        );
        let cell = |address: &str| {
            cells
                .iter()
                .find(|cell| cell["address"] == address)
                .unwrap()
        };
        assert_eq!(cell("A1")["display_text"], "15%");
        assert_eq!(cell("A1")["raw_value"], "0.15");
        assert_eq!(cell("B1")["display_text"], "2026-01-02");
        assert_eq!(cell("C1")["number_format"], "#,##0.00\"元\"");
        assert_eq!(cell("C1")["display_text"], "1,234.50元");
        assert_eq!(cell("D1")["formula"], "=C1*(1+A1)");
        assert_eq!(cell("D1")["formula_references"], json!(["C1", "A1"]));
        assert!(cell("D1")["cached_value"].is_null());
        assert_eq!(
            cell("D1")["display_incomplete_reason"],
            "formula_cached_value_missing"
        );
        assert_eq!(cell("E1")["cached_value"], "42");
        assert_eq!(cell("E1")["display_text"], "42.00元");
        assert_eq!(cell("E1")["formula_references"], json!(["'参数'!A1"]));
        assert_eq!(cell("F1")["display_complete"], false);
        assert_eq!(
            cell("F1")["display_incomplete_reason"],
            "unsupported_number_format"
        );
        assert_eq!(cell("G1")["cached_value_type"], "e");
        assert_eq!(
            cell("G1")["display_incomplete_reason"],
            "formula_cached_error"
        );
        assert_eq!(cell("H1")["display_text"], "普通文字");
    }

    pub(crate) fn related_native_input() -> FrozenInput {
        use crate::analysis::source_manifest::*;
        use prost::Message;
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../docparser/tests/fixtures/python-related-tender-wire.json"
        ))
        .unwrap();
        let mut documents = Vec::new();
        for (key, id, role) in [
            ("main_pdf", "main", DocumentRole::Primary),
            ("pricing_xlsx", "pricing", DocumentRole::Supplement),
        ] {
            let bytes = hex::decode(fixture[key].as_str().unwrap()).unwrap();
            let parsed = ReadResult::try_from(
                docparser::proto::ReadResponse::decode(bytes.as_slice()).unwrap(),
            )
            .unwrap();
            documents.push(ParsedDocument {
                document_id: id.into(),
                role,
                parsed,
            });
        }
        build_frozen_input(
            FrozenBuildInput {
                project_id: "project".into(),
                document_set_id: "related-set".into(),
                documents,
                document_relations: vec![DocumentRelation {
                    id: "pricing-reference".into(),
                    from: RelationEndpoint {
                        document_id: "main".into(),
                        unit_id: None,
                    },
                    to: Some(RelationEndpoint {
                        document_id: "pricing".into(),
                        unit_id: Some("sheet:0:used".into()),
                    }),
                    kind: DocumentRelationKind::ExplicitReference,
                    status: DocumentRelationStatus::Confirmed,
                    required: true,
                    basis: "pricing schedule is provided in synthetic.xlsx".into(),
                    locator: json!({"page_ordinal":0}),
                }],
                decisions: vec![],
            },
            vec![],
            PARSER_CONTRACT_VERSION,
        )
        .unwrap()
    }

    #[test]
    fn related_native_pdf_excel_selects_sources_and_keeps_split_read_dependencies() {
        let input = related_native_input();
        validate_frozen_input_contract(&input).unwrap();
        assert_eq!(input.documents[0].role, DocumentRole::Primary);
        assert!(
            input.source_units.iter().any(
                |source| source.document_id == "main" && source.text.contains("synthetic.xlsx")
            )
        );
        let combined =
            crate::outline::discover::plan_packs_with_budget(&input, &|_| Ok(true)).unwrap();
        assert_eq!(combined.len(), 1);
        assert_eq!(combined[0].document_ids, vec!["main", "pricing"]);
        let split = crate::outline::discover::plan_packs_with_budget(&input, &|sessions| {
            Ok(sessions
                .iter()
                .all(|session| session["pack"]["document_ids"].as_array().unwrap().len() == 1))
        })
        .unwrap();
        assert_eq!(split.len(), 2);
        let all_atoms = combined
            .iter()
            .flat_map(|pack| &pack.atoms)
            .map(|atom| &atom.id)
            .collect::<BTreeSet<_>>();
        assert_eq!(
            all_atoms,
            split
                .iter()
                .flat_map(|pack| &pack.atoms)
                .map(|atom| &atom.id)
                .collect()
        );
        assert!(split.iter().flat_map(|pack| &pack.atoms).all(|atom| {
            input.source_units.iter().any(|source| {
                source.source_unit_revision_id == atom.source_unit_revision_id
                    && source.document_id == atom.document_id
            })
        }));
        let mut work =
            crate::outline::discover::DiscoverWork::plan_with_budget(&input, &|sessions| {
                Ok(sessions
                    .iter()
                    .all(|session| session["pack"]["document_ids"].as_array().unwrap().len() == 1))
            });
        work.claim(work.pack_ids().len());
        let session = work
            .session(&input, work.pack_ids().first().unwrap())
            .unwrap();
        assert_eq!(
            session["pack"]["read_dependencies"][0]["relation"]["id"],
            "pricing-reference"
        );
        assert_eq!(
            session["pack"]["read_dependencies"][0]["target_in_pack"],
            false
        );
        let target = input
            .source_units
            .iter()
            .find(|source| {
                source.document_id == "pricing" && source.locator["unit_id"] == "sheet:0:used"
            })
            .unwrap();
        assert!(target.text.is_empty());
        let originals = session["pack"]["read_dependencies"][0]["available_originals"]
            .as_array()
            .unwrap();
        assert!(!originals.is_empty());
        assert!(originals.iter().all(|value| {
            matches!(
                serde_json::from_value::<crate::outline::evidence::EvidenceRef>(
                    value["evidence"].clone()
                )
                .unwrap(),
                crate::outline::evidence::EvidenceRef::GridCell { .. }
            )
        }));
    }

    #[test]
    fn related_manifest_rejects_implicit_links_missing_sources_and_stale_sets() {
        use crate::analysis::source_manifest::*;
        let input = related_native_input();
        let mut unconfirmed = input.clone();
        unconfirmed.document_relations[0].kind = DocumentRelationKind::InferredCandidate;
        unconfirmed.document_relations[0].status = DocumentRelationStatus::Unconfirmed;
        assert_eq!(
            crate::outline::discover::plan_packs_with_budget(&unconfirmed, &|_| Ok(true))
                .unwrap()
                .len(),
            2
        );
        assert!(!unconfirmed.required_relations_ready());
        unconfirmed.document_relations[0].status = DocumentRelationStatus::Confirmed;
        assert!(unconfirmed.validate_document_relations().is_err());
        let mut missing = input.clone();
        missing.document_relations[0].status = DocumentRelationStatus::Missing;
        missing.document_relations[0].to = None;
        assert!(!missing.required_relations_ready());
        assert!(crate::outline::tools::template_ready(&missing, &Default::default()).is_err());
        let mut invented = input.clone();
        invented.document_relations[0].basis = "invented reference not in the source".into();
        assert!(invented.validate_document_relations().is_err());
        invented = input.clone();
        invented.document_relations[0].locator = json!({"page_ordinal":999});
        assert!(invented.validate_document_relations().is_err());
        let mut duplicate = input.clone();
        duplicate.documents.push(duplicate.documents[0].clone());
        assert!(duplicate.validate_document_relations().is_err());
        let mut member = serde_json::to_value(&input.documents[0]).unwrap();
        member.as_object_mut().unwrap().remove("role");
        assert!(serde_json::from_value::<FrozenDocument>(member).is_err());
        let mut other_set = input.clone();
        other_set.document_set_id = "other-set".into();
        assert_ne!(
            crate::outline::evidence::input_digest(&input).unwrap(),
            crate::outline::evidence::input_digest(&other_set).unwrap()
        );
        assert_eq!(
            input.source_units[0].source_unit_revision_id,
            other_set.source_units[0].source_unit_revision_id
        );
        let work = crate::outline::discover::DiscoverWork::plan_with_budget(&input, &|_| Ok(true));
        assert!(
            work.session(&other_set, work.pack_ids().first().unwrap())
                .is_err()
        );
    }

    pub(crate) fn python_fixture_input(index: usize) -> FrozenInput {
        let parsed = fixture(index);
        let images = results(&parsed);
        build(parsed, images).unwrap()
    }
    pub(crate) fn native_office_fixture_input(index: usize) -> FrozenInput {
        let fixtures: Value = serde_json::from_str(include_str!(
            "../../../docparser/tests/fixtures/python-native-office-v2.json"
        ))
        .unwrap();
        let row = &fixtures[index];
        let source = hex::decode(row["source_hex"].as_str().unwrap()).unwrap();
        let parsed = read_fixture_row(row);
        let contract = docparser::parse_source_contract(&parsed).unwrap().unwrap();
        assert_eq!(
            contract.document_revision,
            hex::encode(Sha256::digest(source))
        );
        assert!(parsed.images.is_empty());
        build(parsed, vec![]).unwrap()
    }
    #[test]
    fn real_native_docx_and_xlsx_empty_text_grids_pass_strict_freeze_and_discover() {
        for index in 0..2 {
            let frozen = native_office_fixture_input(index);
            validate_frozen_input_contract(&frozen).unwrap();
            let table = frozen
                .structured_forms
                .iter()
                .find(|form| {
                    form["definition"]["cells"].as_array().is_some_and(|cells| {
                        cells.iter().any(|cell| {
                            cell["text"]
                                .as_str()
                                .is_some_and(|text| text.contains("必须提交营业执照"))
                        })
                    })
                })
                .expect("real native qualification grid");
            let table_source = frozen
                .source_units
                .iter()
                .find(|source| {
                    source.source_unit_revision_id
                        == table["source_unit_revision_id"].as_str().unwrap()
                })
                .unwrap();
            assert!(table_source.text.is_empty());
            let mut work = crate::outline::discover::DiscoverWork::plan(&frozen, 64_000);
            assert!(work.planning_error().is_none());
            let packs = work.claim(32);
            assert!(!packs.is_empty());
            assert!(packs.iter().flat_map(|pack| &pack.atoms).any(|atom| {
                matches!(
                    &atom.carrier,
                    crate::outline::discover::PackCarrier::Grid { .. }
                )
            }));
        }
    }
    /// Local-only contract validation. The environment paths and source contents
    /// never become test fixtures; this does not perform semantic extraction.
    #[tokio::test]
    #[ignore = "requires private local parser records and summary output paths"]
    async fn external_native_records_validate_freeze_pack_order_and_full_evidence_coverage() {
        use crate::outline::evidence::{EvidenceRef, covered_by_union};
        let path = std::env::var("KB_FROZEN_VALIDATION_RECORDS")
            .expect("set the private parser-record input path");
        let output = std::env::var("KB_FROZEN_VALIDATION_SUMMARY")
            .expect("set the private summary output path");
        let bytes = std::fs::read(path).unwrap_or_else(|_| panic!("private input read failed"));
        let rows: Vec<Value> = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| panic!("private parser records have invalid JSON"));
        let mut documents = Vec::new();
        let mut raw_bytes = 0;
        let mut glyph_receipts = Vec::new();
        for (index, row) in rows.iter().enumerate() {
            let source = hex::decode(row["source_hex"].as_str().expect("missing source bytes"))
                .expect("invalid source byte encoding");
            raw_bytes += source.len();
            let parsed = read_fixture_row(row);
            let contract = docparser::parse_source_contract(&parsed)
                .unwrap_or_else(|_| panic!("private source contract validation failed"))
                .expect("missing native source contract");
            assert!(contract.document_revision == hex::encode(Sha256::digest(source)));
            glyph_receipts.push(json!(contract.glyph_normalizations));
            assert!(
                parsed.images.is_empty(),
                "required image OCR has not been authorized or completed"
            );
            documents.push(ParsedDocument {
                role: crate::analysis::DocumentRole::Unspecified,
                document_id: format!("document-{index}"),
                parsed,
            });
        }
        let input = build_frozen_input(
            FrozenBuildInput {
                project_id: "local-validation".into(),
                document_set_id: "local-validation".into(),
                documents,
                document_relations: vec![],
                decisions: vec![],
            },
            vec![],
            PARSER_CONTRACT_VERSION,
        )
        .unwrap_or_else(|_| panic!("private input failed the mandatory freeze gate"));
        assert!(validate_frozen_input_contract(&input).is_ok());
        if let Ok(export) = std::env::var("KB_FROZEN_VALIDATION_EXPORT") {
            use std::io::Write;
            let path = std::path::PathBuf::from(export);
            let parent = path
                .parent()
                .expect("private export parent required")
                .canonicalize()
                .expect("private export parent must exist");
            let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(2)
                .unwrap()
                .canonicalize()
                .unwrap();
            assert!(
                !parent.starts_with(repository),
                "private input export cannot be inside the repository"
            );
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .expect("private export path must be a new file");
            file.write_all(&serde_json::to_vec(&input).unwrap())
                .expect("private input export failed");
        }
        for (document, receipt) in input.documents.iter().zip(&glyph_receipts) {
            assert!(
                serde_json::to_value(
                    &document
                        .source_contract
                        .as_ref()
                        .unwrap()
                        .glyph_normalizations
                )
                .unwrap()
                    == *receipt,
                "glyph provenance changed during freezing"
            );
        }
        let (mut work, request_summary) = crate::analysis::agent::local_token_plan(&input)
            .await
            .unwrap_or_else(|_| panic!("private input failed full-request context admission"));
        // Materialize each exact first claim for per-pack distribution only;
        // the actual first request retains the admission selected by runtime.
        work.claim(work.pack_ids().len());
        assert!(
            work.planning_error().is_none(),
            "private input could not be packed"
        );
        let mut sessions = work
            .pack_ids()
            .into_iter()
            .map(|id| {
                work.session(&input, &id)
                    .expect("frozen pack session invalid")
            })
            .collect::<Vec<_>>();
        sessions.sort_by_key(|session| session["pack"]["order"].as_u64().unwrap());
        let mut refs = Vec::new();
        let mut previous = None;
        let mut represented = BTreeSet::new();
        let mut empty_cells = BTreeSet::new();
        let mut atom_count = 0;
        let mut max_session_bytes = 0;
        let mut wire_sizes = Vec::new();
        let mut pack_tokens = Vec::new();
        let mut carrier_mix = BTreeMap::<String, usize>::new();
        let mut sections = BTreeSet::new();
        let mut repeated_header_cells = 0;
        for session in &sessions {
            let wire_size = serde_json::to_vec(session).unwrap().len();
            max_session_bytes = max_session_bytes.max(wire_size);
            wire_sizes.push(wire_size);
            pack_tokens.push(
                crate::agent_runtime::chat::TokenizerProfile {
                    model_id: "gpt-4o-2024-08-06".into(),
                    encoding: crate::agent_runtime::chat::TokenEncoding::O200kBase,
                    calibration: None,
                }
                .count_text_tokens(&serde_json::to_string(session).unwrap())
                .unwrap(),
            );
            let canonical = work
                .canonical_session(session["pack"]["id"].as_str().unwrap())
                .unwrap();
            let pack = &canonical["pack"];
            let kinds = pack["atoms"]
                .as_array()
                .unwrap()
                .iter()
                .map(|atom| atom["carrier"]["kind"].as_str().unwrap())
                .collect::<BTreeSet<_>>();
            let mix = if kinds.len() == 1 {
                format!("{}_only", kinds.iter().next().unwrap())
            } else {
                "mixed".into()
            };
            *carrier_mix.entry(mix).or_default() += 1;
            refs.extend(
                work.pack_evidence(&input, pack["id"].as_str().unwrap())
                    .unwrap(),
            );
            for atom in pack["atoms"].as_array().unwrap() {
                let document = atom["document_id"].as_str().unwrap();
                let document_order = input
                    .documents
                    .iter()
                    .position(|row| row.document_id == document)
                    .unwrap();
                if atom["context_only"] == true {
                    continue;
                }
                atom_count += 1;
                sections.insert((
                    document.to_string(),
                    atom["section_id"].as_str().unwrap().to_string(),
                ));
                repeated_header_cells += atom["carrier"]["header_context"]
                    .as_array()
                    .map_or(0, Vec::len);
                let ordinal = atom["unit_ordinal"].as_u64().unwrap() as usize;
                let key = (
                    document_order,
                    ordinal,
                    atom["fragment_ordinal"].as_u64().unwrap(),
                );
                assert!(
                    previous.is_none_or(|previous| previous <= key),
                    "source order changed in packs"
                );
                previous = Some(key);
                represented.insert((document.to_string(), ordinal));
                if let Some(cells) = atom["carrier"]["cells"].as_array() {
                    for cell in cells
                        .iter()
                        .filter(|cell| cell["start_byte"] == cell["end_byte"])
                    {
                        empty_cells.insert((
                            atom["carrier"]["table_id"].as_str().unwrap().to_string(),
                            cell["anchor_row"].as_u64().unwrap(),
                            cell["anchor_column"].as_u64().unwrap(),
                        ));
                    }
                }
            }
        }
        let digest = crate::outline::evidence::input_digest(&input).unwrap();
        let mut text_bytes = 0;
        for source in &input.source_units {
            assert!(
                represented.contains(&(source.document_id.clone(), source.ordinal)),
                "source unit omitted from packs"
            );
            text_bytes += source.text.len();
            if !source.text.is_empty() {
                assert!(
                    covered_by_union(
                        &EvidenceRef::Text {
                            input_digest: digest.clone(),
                            unit_id: source.source_unit_revision_id.clone(),
                            start_byte: 0,
                            end_byte: source.text.len()
                        },
                        &refs
                    ),
                    "source text is not fully covered"
                );
            }
        }
        let mut grid_cells = 0;
        let mut grid_text_bytes = 0;
        for form in &input.structured_forms {
            let table_id = form["form_definition_revision_id"].as_str().unwrap();
            for cell in form["definition"]["cells"].as_array().unwrap() {
                grid_cells += 1;
                let text = cell["text"].as_str().unwrap();
                grid_text_bytes += text.len();
                let row = cell["row"].as_u64().unwrap();
                let column = cell["column"].as_u64().unwrap();
                if text.is_empty() {
                    assert!(
                        empty_cells.contains(&(table_id.to_string(), row, column)),
                        "empty grid anchor omitted"
                    );
                } else {
                    assert!(
                        covered_by_union(
                            &EvidenceRef::GridCell {
                                input_digest: digest.clone(),
                                table_id: table_id.into(),
                                anchor_row: row as usize,
                                anchor_column: column as usize,
                                start_byte: 0,
                                end_byte: text.len()
                            },
                            &refs
                        ),
                        "grid cell text is not fully covered"
                    );
                }
            }
        }
        let distribution = |mut values: Vec<usize>| {
            values.sort_unstable();
            let percentile = |p: usize| {
                values[(values.len() * p)
                    .div_ceil(100)
                    .saturating_sub(1)
                    .min(values.len() - 1)]
            };
            json!({"min":values[0],"median":percentile(50),"p90":percentile(90),"max":values[values.len()-1],"sum":values.iter().sum::<usize>()})
        };
        let summary = json!({"documents":input.documents.len(),"source_bytes":raw_bytes,"source_units":input.source_units.len(),"tables":input.structured_forms.len(),"grid_cells":grid_cells,"source_text_bytes":text_bytes,"grid_text_bytes":grid_text_bytes,"packs":sessions.len(),"atoms":atom_count,"max_serialized_pack_bytes":max_session_bytes,"pack_wire_bytes":distribution(wire_sizes),"pack_only_estimated_tokens":distribution(pack_tokens),"pack_carrier_mix":carrier_mix,"distinct_sections":sections.len(),"repeated_header_cells":repeated_header_cells,"max_context_tokens":131072,"runtime_request":request_summary,"token_count_scope":"pack_payload_distribution_with_separate_full_runtime_admission","max_active_packs":crate::outline::discover::DEFAULT_PACK_CONCURRENCY,"frozen_json_bytes":serde_json::to_vec(&input).unwrap().len(),"glyph_normalizations":glyph_receipts.iter().map(|receipt|receipt.as_array().unwrap().len()).sum::<usize>(),"strict_freeze_valid":true,"source_order_preserved":true,"full_text_grid_coverage":true,"semantic_extraction_performed":false,"external_model_calls":0});
        std::fs::write(output, serde_json::to_vec_pretty(&summary).unwrap())
            .unwrap_or_else(|_| panic!("private summary write failed"));
        println!("{summary}");
    }
    #[test]
    fn real_python_pdf_passes_mandatory_freeze_and_discover() {
        for index in [0] {
            let frozen = python_fixture_input(index);
            validate_frozen_input_contract(&frozen).unwrap();
            let work = super::super::discover::DiscoverWork::plan(&frozen, 131_072);
            assert_eq!(work.planning_error(), None);
            assert!(!work.pack_ids().is_empty());
            for pack in work.pack_ids() {
                work.pack_evidence(&frozen, &pack).unwrap();
            }
        }
        let frozen = python_fixture_input(0);
        let ocr = frozen
            .source_units
            .iter()
            .filter(|s| s.locator["kind"] == "image_region")
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ocr, vec!["OCR 1", "OCR 2", "OCR 3"]);
        assert_eq!(frozen.documents[0].page_count, 3);
    }
    #[test]
    fn freeze_rejects_missing_partial_or_truncated_ocr() {
        let parsed = fixture(0);
        let images = results(&parsed);
        assert!(
            build(parsed.clone(), vec![])
                .unwrap_err()
                .contains("OCR missing")
        );
        for mode in 0..3 {
            let mut broken = images.clone();
            match mode {
                0 => broken[0].completeness = "partial".into(),
                1 => broken[0].finish_reason = "length".into(),
                _ => broken[0].done_seen = false,
            };
            assert!(
                build(parsed.clone(), broken)
                    .unwrap_err()
                    .contains("incomplete")
            );
        }
        let mut stale = images;
        stale[0].document_revision = "0".repeat(64);
        assert!(build(parsed, stale).unwrap_err().contains("revision"));
    }
    #[test]
    fn freeze_requires_contract_and_rejects_duplicate_completion() {
        let mut parsed = fixture(0);
        parsed.metadata.remove("source_contract");
        assert!(
            build(parsed, vec![])
                .unwrap_err()
                .contains("source_contract")
        );
        let parsed = fixture(0);
        let mut images = results(&parsed);
        images.push(images[0].clone());
        assert!(build(parsed, images).unwrap_err().contains("duplicate"));
    }
    #[test]
    fn frozen_validation_rejects_missing_units_reordered_ocr_and_text_drift() {
        let frozen = python_fixture_input(0);
        let mut missing = frozen.clone();
        missing.source_units.pop();
        assert!(validate_frozen_input_contract(&missing).is_err());
        let mut reordered = frozen.clone();
        reordered.source_units.swap(1, 2);
        assert!(validate_frozen_input_contract(&reordered).is_err());
        let mut changed = frozen.clone();
        changed.source_units[0].text.push('!');
        assert!(
            validate_frozen_input_contract(&changed)
                .unwrap_err()
                .contains("parser contract")
        );
        let mut changed = frozen;
        changed.source_units[1].text.push('!');
        assert!(validate_frozen_input_contract(&changed).is_err());
    }
    #[test]
    fn frozen_validation_rejects_coordinated_source_and_coverage_deletion() {
        let mut frozen = native_office_fixture_input(0);
        frozen.source_units.pop();
        let coverage = frozen.documents[0].parse_coverage.as_mut().unwrap();
        coverage.expected_unit_keys.pop();
        coverage.published_unit_keys.pop();
        assert!(
            validate_frozen_input_contract(&frozen)
                .unwrap_err()
                .contains("unit index differs")
        );
    }
    #[test]
    fn frozen_validation_rechecks_glyph_provenance_before_registration() {
        let mut frozen = python_fixture_input(0);
        frozen.documents[0]
            .source_contract
            .as_mut()
            .unwrap()
            .glyph_normalizations = serde_json::from_value(json!([{
            "page_ordinal":0,"char_index":0,"raw_symbol":"\u{f052}","normalized_symbol":"wrong",
            "font_name":"Wingdings 2","font_sha256":"a".repeat(64),"glyph_name":"boxcheck",
            "left":0.0,"bottom":0.0,"right":10.0,"top":10.0,"page_width":300.0,"page_height":300.0
        }]))
        .unwrap();
        assert!(
            validate_frozen_input_contract(&frozen)
                .unwrap_err()
                .contains("normalization")
        );
    }
    #[test]
    fn frozen_validation_rejects_grid_drift_and_deleted_blank_page() {
        let mut frozen = complete_grid_fixture_input();
        assert!(!frozen.structured_forms.is_empty());
        frozen.structured_forms[0]["definition"]["cells"][0]["text"] = json!("changed");
        assert!(
            validate_frozen_input_contract(&frozen)
                .unwrap_err()
                .contains("grid differs")
        );
        let mut pdf = python_fixture_input(0);
        pdf.documents[0]
            .source_contract
            .as_mut()
            .unwrap()
            .page_manifest
            .remove(1);
        assert!(validate_frozen_input_contract(&pdf).is_err());
    }
    pub(crate) fn complete_grid_fixture_input() -> FrozenInput {
        let grid = docparser::TableGrid {
            row_count: 1,
            column_count: 2,
            cells: vec![
                docparser::PdfTableCell {
                    row: 0,
                    column: 0,
                    row_span: 1,
                    col_span: 1,
                    text: "必须报价".into(),
                },
                docparser::PdfTableCell {
                    row: 0,
                    column: 1,
                    row_span: 1,
                    col_span: 1,
                    text: String::new(),
                },
            ],
            widths_mm: None,
        };
        let locator = docparser::StructuredSourceLocator::Document {
            section_ordinal: 0,
            table_ordinal: Some(0),
            row_ordinal: None,
            form_ordinal: None,
            heading_path: "报价要求".into(),
        };
        let unit = docparser::StructuredSourceUnit {
            key: "table".into(),
            ordinal: 0,
            kind: StructuredSourceUnitKind::TableRegion,
            text: String::new(),
            locator: locator.clone(),
            grid: Some(grid.clone()),
        };
        let contract = json!({"schema_version":2,"document_revision":hex::encode(Sha256::digest(b"grid fixture")),"parser_version":"test-source-v2","markdown_sha256":hex::encode(Sha256::digest(b"")),"page_manifest":[],"units":[{
            "unit_id":"table","ordinal":0,"kind":"table_region","text_sha256":hex::encode(Sha256::digest(b"")),"grid_sha256":docparser::table_grid_digest(&grid),"section_id":"section:0","parent_section_id":null,"heading_level":1,"heading_path":"报价要求","physical_locator":locator,"physical_path":"body/tbl:0","physical_locator_unavailable_reason":null,"rendered_spans":[],"completeness":"complete","reasons":[],"table_id":"table","header_cells":[]
        }]});
        build(
            ReadResult {
                structured_source_units: vec![unit],
                metadata: std::collections::HashMap::from([(
                    "source_contract".into(),
                    contract.to_string(),
                )]),
                ..Default::default()
            },
            vec![],
        )
        .unwrap()
    }
    #[test]
    fn real_python_incomplete_docx_inventory_is_not_frozen_as_complete() {
        let parsed = fixture(1);
        let images = results(&parsed);
        assert!(build(parsed, images).unwrap_err().contains("Partial"));
    }
    struct FixtureParser;
    #[async_trait::async_trait]
    impl TenderSourceParser for FixtureParser {
        async fn parse(
            &self,
            raw: &RawTenderDocument,
            _: &tokio_util::sync::CancellationToken,
        ) -> Result<ReadResult, String> {
            let mut parsed = fixture(0);
            let mut contract = docparser::parse_source_contract(&parsed).unwrap().unwrap();
            contract.document_revision = hex::encode(Sha256::digest(&raw.bytes));
            parsed.metadata.insert(
                "source_contract".into(),
                serde_json::to_string(&contract).unwrap(),
            );
            Ok(parsed)
        }
    }
    struct FixtureImages;
    #[async_trait::async_trait]
    impl TenderImageProcessor for FixtureImages {
        async fn process(
            &self,
            document: &str,
            contract: &SourceContract,
            unit: &docparser::StructuredSourceUnit,
            image: &docparser::ImageRef,
            _: &tokio_util::sync::CancellationToken,
        ) -> Result<FrozenImageResult, String> {
            tokio::time::sleep(std::time::Duration::from_millis(
                30 - unit.ordinal as u64 * 5,
            ))
            .await;
            Ok(FrozenImageResult {
                document_id: document.into(),
                document_revision: contract.document_revision.clone(),
                parser_version: contract.parser_version.clone(),
                unit_id: unit.key.clone(),
                literal_text: format!("OCR {}", unit.ordinal),
                completeness: "complete".into(),
                finish_reason: "stop".into(),
                done_seen: true,
                transport_complete: true,
                persisted_image_ref: format!(
                    "objects/{}",
                    hex::encode(Sha256::digest(&image.data))
                ),
                provenance: json!({"literal":true}),
            })
        }
    }
    #[tokio::test]
    async fn raw_producer_runs_parser_and_required_ocr_before_freezing() {
        let request = PrepareTenderInput {
            project_id: "project".into(),
            document_set_id: "set".into(),
            documents: vec![RawTenderDocument {
                role: crate::analysis::DocumentRole::Unspecified,
                document_id: "document".into(),
                file_name: "fixture.pdf".into(),
                bytes: b"raw parser input".to_vec(),
            }],
            document_relations: vec![],
            decisions: vec![],
            parser_contract_version: "source-v2".into(),
        };
        let frozen = prepare_tender_input_with(
            request,
            &FixtureParser,
            &FixtureImages,
            &tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();
        validate_frozen_input_contract(&frozen).unwrap();
        assert_eq!(
            frozen
                .source_units
                .iter()
                .filter(|s| s.locator["kind"] == "image_region")
                .map(|s| s.text.as_str())
                .collect::<Vec<_>>(),
            vec!["OCR 1", "OCR 2", "OCR 3"]
        );
        assert_eq!(
            frozen.documents[0].document_revision,
            hex::encode(Sha256::digest(b"raw parser input"))
        );
    }
    #[test]
    fn textless_nonblank_image_requires_real_vision_and_blank_page_is_accounted_for() {
        let parsed = fixture(0);
        let images = results(&parsed);
        let mut diagram = images.clone();
        diagram
            .iter_mut()
            .find(|image| image.unit_id == "page:2:image:0")
            .unwrap()
            .literal_text
            .clear();
        let visual = build(parsed.clone(), diagram).unwrap();
        assert_eq!(visual.source_units[3].locator["vision_required"], true);
        assert_eq!(visual.source_units[3].locator["ocr_complete"], false);
        assert_eq!(
            visual.source_units[3].locator["ocr_status"],
            "empty_requires_vision"
        );
        validate_frozen_input_contract(&visual).unwrap();
        let mut blank = images;
        blank
            .iter_mut()
            .find(|image| image.unit_id == "page:1:image:0")
            .unwrap()
            .literal_text
            .clear();
        let mut frozen = build(parsed, blank).unwrap();
        assert_eq!(frozen.source_units[2].locator["blank_image"], true);
        validate_frozen_input_contract(&frozen).unwrap();
        frozen.source_units[3].locator["blank_image"] = json!(true);
        assert!(
            validate_frozen_input_contract(&frozen)
                .unwrap_err()
                .contains("visual/OCR")
        );
    }
}
