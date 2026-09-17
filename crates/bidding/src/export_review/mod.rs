//! Independent final-file evidence for export review (§13.2).
//! Scan the actual OOXML parts. Generation bookmarks are matching aids only.
pub mod agent;
#[cfg(test)]
mod frozen_contract_tests;
pub mod postgres;
pub mod runtime;
use crate::tender_analysis::digest;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FrozenContext {
    pub analysis_identity: Option<Value>,
    pub execution_contract: Option<FrozenExecution>,
    pub layout_result: Option<Value>,
}

impl FrozenContext {
    pub fn allows_semantic_export_review(&self) -> bool {
        self.execution_contract.is_some()
            && self
                .analysis_identity
                .as_ref()
                .is_some_and(|identity| identity.get("schema_version") == Some(&json!(2)))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct FrozenExecution {
    pub schema_version: u32,
    pub definition: Value,
    pub contract_sha256: String,
}

impl FrozenExecution {
    pub fn freeze(config: &agent::Config) -> Result<Self, crate::agent_error::AgentError> {
        Ok(Self {
            schema_version: 1,
            definition: config.contract_definition()?,
            contract_sha256: config.contract_sha256()?,
        })
    }

    pub fn config(&self) -> Result<agent::Config, crate::agent_error::AgentError> {
        let invalid = |message: String| {
            crate::agent_error::AgentError::new("FROZEN_INPUT_DIGEST_MISMATCH", message)
        };
        let config: agent::Config = serde_json::from_value(self.definition["config"].clone())
            .map_err(|e| invalid(e.to_string()))?;
        if self.schema_version != 1
            || digest(&self.definition).map_err(invalid)? != self.contract_sha256
            || config.contract_definition()? != self.definition
        {
            return Err(invalid("export-review execution contract changed".into()));
        }
        Ok(config)
    }
}
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputCoverage {
    pub units: BTreeMap<String, String>,
    #[serde(default)]
    pub views: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OutputUnit {
    pub id: String,
    pub file_sha256: String,
    pub part: String,
    pub ordinal: usize,
    pub kind: String,
    pub content_sha256: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bookmark: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bookmarks: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_unit: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_checked_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub image_sha256s: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Inventory {
    pub docx_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdf_sha256: Option<String>,
    pub units: Vec<OutputUnit>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parser_manifests: Vec<docparser::OutputInventoryManifest>,
    #[serde(default)]
    pub images: BTreeMap<String, OutputImage>,
}

/// Pixel bytes only live across parsing and staging, never in a checkpoint.
#[derive(Debug)]
pub struct ParsedInventory {
    pub inventory: Inventory,
    pub images: BTreeMap<String, Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OutputImage {
    pub sha256: String,
    pub object_ref: String,
    pub media_type: String,
    pub byte_length: u64,
    pub width: u32,
    pub height: u32,
}

impl OutputImage {
    fn from_bytes(bytes: &[u8], declared_type: &str) -> Result<Self, String> {
        if bytes.is_empty() {
            return Err("DocReader returned empty output image bytes".into());
        }
        // Inspect only format and dimensions. No rendering, pixel decoding or
        // resampling: the immutable original is the shared service's output.
        let (media_type, width, height) = match image::guess_format(bytes) {
            Ok(format) => {
                let actual = format.to_mime_type();
                if !declared_type.is_empty()
                    && declared_type != "application/octet-stream"
                    && declared_type != actual
                {
                    return Err("output image media type disagrees with bytes".into());
                }
                let (width, height) =
                    match image::ImageReader::with_format(std::io::Cursor::new(bytes), format)
                        .into_dimensions()
                    {
                        Ok(dimensions) => dimensions,
                        Err(image::ImageError::Unsupported(_))
                            if !matches!(actual, "image/png" | "image/jpeg" | "image/webp") =>
                        {
                            (0, 0)
                        }
                        Err(error) => return Err(format!("output image metadata: {error}")),
                    };
                (actual.to_owned(), width, height)
            }
            Err(_) if matches!(declared_type, "image/png" | "image/jpeg" | "image/webp") => {
                return Err("supported output image has invalid format bytes".into());
            }
            // Unsupported carriers remain retained, with explicitly unknown
            // dimensions. These objects cannot be used as model image views.
            Err(_) => (
                if declared_type.is_empty() {
                    "application/octet-stream"
                } else {
                    declared_type
                }
                .to_owned(),
                0,
                0,
            ),
        };
        let sha256 = hex::encode(Sha256::digest(bytes));
        Ok(Self {
            object_ref: format!("objects/{sha256}"),
            sha256,
            media_type,
            byte_length: bytes.len() as u64,
            width,
            height,
        })
    }

    pub fn supports_model_view(&self) -> bool {
        self.width > 0
            && self.height > 0
            && matches!(
                self.media_type.as_str(),
                "image/png" | "image/jpeg" | "image/webp"
            )
    }

    /// Revalidate immutable stored bytes before delivery, without transforming them.
    pub fn validate_bytes(&self, bytes: &[u8]) -> Result<(), String> {
        if Self::from_bytes(bytes, &self.media_type)? != *self {
            return Err("output image bytes or metadata differ from frozen inventory".into());
        }
        Ok(())
    }
}

pub fn schemas() -> Vec<Value> {
    serde_json::from_str(include_str!(
        "../../schemas/export-review-tools-v1.schema.json"
    ))
    .expect("checked export-review tool schemas")
}

/// Parse immutable output bytes once through the existing Python service.
pub async fn inventory_from_files(
    docx: &[u8],
    pdf: Option<&[u8]>,
    cancel: &CancellationToken,
) -> Result<ParsedInventory, crate::agent_error::AgentError> {
    let docx_sha256 = hex::encode(Sha256::digest(docx));
    let pdf_sha256 = pdf.map(|bytes| hex::encode(Sha256::digest(bytes)));
    let mut inventory = Inventory {
        docx_sha256,
        pdf_sha256,
        ..Inventory::default()
    };
    let parsed = docparser::read_output_inventory("output.docx", docx.to_vec(), cancel)
        .await
        .map_err(inventory_read_error)?;
    let mut images = append_service_inventory(&mut inventory, parsed, "docx")
        .map_err(inventory_contract_error)?;
    if let Some(pdf) = pdf {
        let parsed = docparser::read_output_inventory("output.pdf", pdf.to_vec(), cancel)
            .await
            .map_err(inventory_read_error)?;
        images.extend(
            append_service_inventory(&mut inventory, parsed, "pdf")
                .map_err(inventory_contract_error)?,
        );
    }
    if !inventory.images.keys().eq(images.keys()) {
        return Err(inventory_contract_error(
            "parsed inventory image bytes and object metadata differ".into(),
        ));
    }
    Ok(ParsedInventory { inventory, images })
}

fn inventory_contract_error(message: String) -> crate::agent_error::AgentError {
    crate::agent_error::AgentError::new("FROZEN_INPUT_DIGEST_MISMATCH", message)
}

fn inventory_read_error(error: docparser::DocReaderReadError) -> crate::agent_error::AgentError {
    use docparser::DocReaderReadError;
    let code = match &error {
        DocReaderReadError::Cancelled => "INTERNAL",
        DocReaderReadError::Transient(_) => "INTERNAL",
        DocReaderReadError::Configuration(_) => "AGENT_CONFIG_INVALID",
        DocReaderReadError::InvalidResponse(_) => "FROZEN_INPUT_DIGEST_MISMATCH",
    };
    crate::agent_error::AgentError::new(code, error.to_string())
}

fn append_service_inventory(
    inventory: &mut Inventory,
    read: docparser::OutputInventoryRead,
    file_type: &str,
) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let expected = match file_type {
        "docx" => &inventory.docx_sha256,
        "pdf" => inventory
            .pdf_sha256
            .as_ref()
            .ok_or("PDF identity missing")?,
        _ => return Err("unsupported output format".into()),
    };
    let manifest = docparser::validate_output_inventory(&read.parsed, expected, file_type)
        .map_err(|error| error.to_string())?;
    if manifest != read.manifest {
        return Err("output inventory manifest differs from service response".into());
    }
    let mut images = BTreeMap::new();
    let mut image_refs = BTreeMap::new();
    for image in read.parsed.images {
        let metadata = OutputImage::from_bytes(&image.data, &image.mime_type)?;
        if manifest.image_sha256.get(&image.original_ref) != Some(&metadata.sha256) {
            return Err("output image has no matching parser byte receipt".into());
        }
        if let Some(previous) = inventory.images.get(&metadata.sha256)
            && previous != &metadata
        {
            return Err("same output image bytes have conflicting metadata".into());
        }
        image_refs.insert(image.original_ref, metadata.sha256.clone());
        images.insert(metadata.sha256.clone(), image.data);
        inventory.images.insert(metadata.sha256.clone(), metadata);
    }
    for (entry, source) in manifest
        .units
        .iter()
        .zip(&read.parsed.structured_source_units)
    {
        let mut image_sha256s = Vec::new();
        let mut not_checked_reason = entry.reason.clone();
        if let docparser::StructuredSourceLocator::Image {
            original_ref,
            width,
            height,
            media_type,
            ..
        } = &source.locator
        {
            let sha = image_refs
                .get(original_ref)
                .ok_or("image locator has no delivered bytes")?;
            let image = inventory.images.get(sha).ok_or("image metadata missing")?;
            if image.width > 0
                && (image.width != *width
                    || image.height != *height
                    || image.media_type != *media_type)
            {
                return Err(
                    "image locator dimensions or media type disagree with original bytes".into(),
                );
            }
            image_sha256s.push(sha.clone());
            if !image.supports_model_view() {
                not_checked_reason = Some(format!(
                    "{}; unsupported output image format ({})",
                    entry
                        .reason
                        .as_deref()
                        .unwrap_or("image requires visual review"),
                    image.media_type
                ));
            }
        }
        let source_unit = serde_json::to_value(source).map_err(|error| error.to_string())?;
        let kind = if entry.status == "not_checked" {
            "not_checked"
        } else {
            &entry.kind
        };
        let content_sha256 = digest(&json!({
            "entry":entry,"source_unit":source_unit,"parser":manifest.parser,"config":manifest.config,"images":manifest.image_sha256,"image_sha256s":image_sha256s,"not_checked_reason":not_checked_reason
        }))?;
        inventory.units.push(OutputUnit {
            id: format!("{file_type}:{expected}:{}", source.key),
            file_sha256: expected.clone(),
            part: entry.part.clone(),
            ordinal: entry.ordinal,
            kind: kind.into(),
            content_sha256,
            text: source.text.clone(),
            bookmark: entry
                .bookmarks
                .iter()
                .find(|name| !name.starts_with('_'))
                .cloned(),
            bookmarks: entry.bookmarks.clone(),
            source_unit: Some(source_unit),
            not_checked_reason,
            image_sha256s,
        });
    }
    inventory.parser_manifests.push(manifest);
    Ok(images)
}

pub fn extra_units_not_covered_by_bookmarks<'a>(
    inventory: &'a Inventory,
    bookmarks: &[String],
) -> Vec<&'a OutputUnit> {
    inventory
        .units
        .iter()
        .filter(|unit| {
            matches!(unit.kind.as_str(), "paragraphs" | "table")
                && !unit
                    .bookmarks
                    .iter()
                    .chain(unit.bookmark.iter())
                    .any(|bookmark| bookmarks.iter().any(|known| known == bookmark))
        })
        .collect()
}

pub fn read_output_evidence(
    inventory: &Inventory,
    coverage: &mut OutputCoverage,
    args: &Value,
    max_bytes: usize,
) -> Result<Value, String> {
    let offset = args["offset"].as_u64().ok_or("offset required")? as usize;
    let limit = args["limit"].as_u64().ok_or("limit required")? as usize;
    if limit == 0 {
        return Err("limit must be positive".into());
    }
    let id = args["id"].as_str();
    let items: Vec<&OutputUnit> = if let Some(id) = id {
        inventory
            .units
            .iter()
            .filter(|unit| unit.id == id)
            .collect()
    } else {
        inventory.units.iter().skip(offset).take(limit).collect()
    };
    if id.is_some() && items.is_empty() {
        return Err("unknown output evidence id".into());
    }
    let page = json!({
        "docx_sha256":inventory.docx_sha256,
        "pdf_sha256":inventory.pdf_sha256,
        "total":inventory.units.len(),
        "next":offset.saturating_add(items.len()),
        "items":items,
    });
    if serde_json::to_vec(&page).map_err(|e| e.to_string())?.len() > max_bytes {
        return Err("output evidence page exceeds budget".into());
    }
    for unit in items {
        coverage
            .units
            .insert(unit.id.clone(), unit.content_sha256.clone());
    }
    Ok(page)
}

#[cfg(test)]
#[path = "inventory_tests.rs"]
mod tests;
