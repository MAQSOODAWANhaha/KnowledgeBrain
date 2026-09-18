//! Explicit, fail-closed final-file profile over the shared DocReader transport.
use crate::{
    ConvertError, DocReaderReadError, ReadResult, StructuredSourceLocator, StructuredSourceUnitKind,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use tokio_util::sync::CancellationToken;

pub const OUTPUT_INVENTORY_PROFILE: &str = "output_inventory_v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OutputTableLayout {
    pub widths_twips: Vec<u32>,
    pub header_rows: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OutputInventoryEntry {
    pub unit_key: String,
    pub part: String,
    pub ordinal: usize,
    pub kind: String,
    pub bookmarks: Vec<String>,
    pub fields: Vec<String>,
    /// Outline level of a heading paragraph, read from the style `w:name` only:
    /// editors renumber `w:styleId` after a round trip.
    #[serde(default)]
    pub heading_level: Option<u32>,
    /// Paragraph style `w:name` when present. Fill only accepts compiler-safe
    /// styles; custom names become not_checked.
    #[serde(default)]
    pub style_name: Option<String>,
    /// Set when the carrier sits inside a field region. Table-of-contents
    /// entries repeat chapter titles verbatim, so a reader that cannot see the
    /// region counts every chapter twice.
    #[serde(default)]
    pub field_region: Option<String>,
    pub table_layout: Option<OutputTableLayout>,
    pub status: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OutputInventoryManifest {
    pub schema_version: u32,
    pub profile: String,
    pub file_sha256: String,
    pub parser: String,
    pub config: Value,
    pub page_count: Option<u32>,
    pub image_sha256: BTreeMap<String, String>,
    pub units: Vec<OutputInventoryEntry>,
}

pub struct OutputInventoryRead {
    pub parsed: ReadResult,
    pub manifest: OutputInventoryManifest,
}

pub async fn read_output_inventory(
    file_name: &str,
    bytes: Vec<u8>,
    cancel: &CancellationToken,
) -> Result<OutputInventoryRead, DocReaderReadError> {
    let file_type = file_name.rsplit('.').next().unwrap_or("");
    if !matches!(file_type, "docx" | "pdf") {
        return Err(DocReaderReadError::Configuration(
            "output inventory requires DOCX or PDF".into(),
        ));
    }
    let expected = hex::encode(Sha256::digest(&bytes));
    let overrides = HashMap::from([("output_inventory".into(), "v1".into())]);
    let parsed = crate::grpc::read_classified(
        crate::grpc::ConvertRequest {
            file_content: bytes,
            file_name: file_name.into(),
            file_type: file_type.into(),
            url: String::new(),
            title: file_name.into(),
            parser_engine: "builtin".into(),
            parser_engine_overrides: overrides,
        },
        cancel,
    )
    .await?;
    let manifest = validate_output_inventory(&parsed, &expected, file_type)
        .map_err(|error| DocReaderReadError::InvalidResponse(error.0))?;
    Ok(OutputInventoryRead { parsed, manifest })
}

/// Also usable with a recorded service response; no local parsing fallback.
pub fn validate_output_inventory(
    parsed: &ReadResult,
    expected_sha256: &str,
    file_type: &str,
) -> Result<OutputInventoryManifest, ConvertError> {
    let invalid = |message: &str| ConvertError(format!("DocReader output inventory: {message}"));
    if !parsed.error.is_empty() {
        return Err(invalid(&parsed.error));
    }
    let manifest: OutputInventoryManifest = serde_json::from_str(
        parsed
            .metadata
            .get("output_inventory_manifest")
            .ok_or_else(|| invalid("profile receipt missing; service upgrade required"))?,
    )
    .map_err(|e| invalid(&e.to_string()))?;
    if manifest.schema_version != 1
        || manifest.profile != OUTPUT_INVENTORY_PROFILE
        || manifest.config["profile"] != OUTPUT_INVENTORY_PROFILE
        || manifest.file_sha256 != expected_sha256
        || manifest.parser.trim().is_empty()
    {
        return Err(invalid("profile or immutable file identity mismatch"));
    }
    let expected_config = match file_type {
        "docx" => {
            serde_json::json!({"profile":OUTPUT_INVENTORY_PROFILE,"preserve_story_occurrences":true})
        }
        "pdf" => {
            serde_json::json!({"profile":OUTPUT_INVENTORY_PROFILE,"preserve_all_pages":true,"preserve_page_images":true,"text_cleaning":false})
        }
        _ => return Err(invalid("unsupported file type")),
    };
    let mut checked_config = manifest.config.clone();
    let settings = checked_config
        .as_object_mut()
        .and_then(|config| config.remove("settings"));
    if !settings.is_some_and(|settings| settings.is_object()) {
        return Err(invalid("parser settings receipt missing"));
    }
    if checked_config != expected_config
        || manifest.units.is_empty()
        || manifest.units.len() != parsed.structured_source_units.len()
    {
        return Err(invalid("configuration or unit count mismatch"));
    }
    let images: BTreeMap<_, _> = parsed
        .images
        .iter()
        .map(|image| {
            (
                image.original_ref.clone(),
                hex::encode(Sha256::digest(&image.data)),
            )
        })
        .collect();
    if images.len() != parsed.images.len() || images != manifest.image_sha256 {
        return Err(invalid("image bytes differ from inventory receipt"));
    }
    let mut keys = BTreeSet::new();
    let mut pages = BTreeSet::new();
    let mut page_images = BTreeSet::new();
    for (ordinal, (entry, unit)) in manifest
        .units
        .iter()
        .zip(&parsed.structured_source_units)
        .enumerate()
    {
        let compatible_kind = match unit.kind {
            StructuredSourceUnitKind::Section => {
                matches!(entry.kind.as_str(), "paragraphs" | "section" | "pdf_page")
            }
            StructuredSourceUnitKind::TableRegion => entry.kind == "table",
            StructuredSourceUnitKind::ImageRegion => {
                entry.kind == "image" && entry.status == "not_checked"
            }
            StructuredSourceUnitKind::AttachmentRegion => {
                entry.kind == "not_checked" && entry.status == "not_checked"
            }
            _ => false,
        };
        if !compatible_kind || unit.ordinal as usize != ordinal {
            return Err(invalid("unit carrier kind or occurrence order mismatch"));
        }
        let outline = entry
            .heading_level
            .is_none_or(|level| entry.kind == "paragraphs" && (1..=9).contains(&level));
        let region = entry
            .field_region
            .as_deref()
            .is_none_or(|region| matches!(region, "toc" | "field"));
        if entry.unit_key != unit.key
            || !keys.insert(&entry.unit_key)
            || entry.part.is_empty()
            || entry.kind.is_empty()
            || !outline
            || !region
            || !matches!(entry.status.as_str(), "extracted" | "not_checked")
            || (entry.status == "not_checked")
                != entry
                    .reason
                    .as_ref()
                    .is_some_and(|reason| !reason.is_empty())
        {
            return Err(invalid("unit mapping or explicit coverage status invalid"));
        }
        if file_type == "pdf" {
            let page = match unit.locator {
                StructuredSourceLocator::Page { page_ordinal, .. }
                | StructuredSourceLocator::PageTable { page_ordinal, .. } => page_ordinal,
                StructuredSourceLocator::Image {
                    page_ordinal: Some(page),
                    ..
                } => page,
                _ => return Err(invalid("PDF unit has no physical page")),
            };
            if entry.part != format!("pdf:page:{}", page + 1) {
                return Err(invalid("PDF sidecar page mismatch"));
            }
            if matches!(unit.locator, StructuredSourceLocator::Page { .. }) {
                pages.insert(page);
            }
            if let StructuredSourceLocator::Image {
                ref original_ref,
                left: None,
                top: None,
                right: None,
                bottom: None,
                ..
            } = unit.locator
            {
                if !manifest.image_sha256.contains_key(original_ref) {
                    return Err(invalid("page image has no byte identity"));
                }
                page_images.insert(page);
            }
        }
    }
    if file_type == "pdf"
        && !manifest.page_count.is_some_and(|count| {
            count > 0
                && pages.len() == count as usize
                && pages.iter().copied().eq(0..count)
                && page_images == pages
        })
    {
        return Err(invalid("PDF physical page inventory incomplete"));
    }
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ImageRef, StructuredSourceUnit};

    fn blank_pdf() -> ReadResult {
        let mut parsed = ReadResult {
            structured_source_units: vec![StructuredSourceUnit {
                key: "page:0:section:0".into(), ordinal: 0,
                kind: StructuredSourceUnitKind::Section, text: String::new(),
                locator: StructuredSourceLocator::Page { page_ordinal: 0, left: None, top: None, right: None, bottom: None }, grid: None,
            }],
            metadata: HashMap::from([("output_inventory_manifest".into(), serde_json::json!({
                "schema_version":1,"profile":OUTPUT_INVENTORY_PROFILE,"file_sha256":"immutable",
                "parser":"profile/implementation/sha/pdfium/version","page_count":1,"image_sha256":{},
                "config":{"profile":OUTPUT_INVENTORY_PROFILE,"preserve_all_pages":true,"preserve_page_images":true,"text_cleaning":false,"settings":{}},
                "units":[{"unit_key":"page:0:section:0","part":"pdf:page:1","ordinal":0,
                    "kind":"pdf_page","bookmarks":[],"fields":[],"status":"extracted","reason":null}]
            }).to_string())]), ..Default::default()
        };
        parsed.images.push(ImageRef {
            original_ref: "images/page.png".into(),
            data: vec![1, 2, 3],
            ..Default::default()
        });
        parsed.structured_source_units.push(StructuredSourceUnit {
            key: "page:0:image:0".into(),
            ordinal: 1,
            kind: StructuredSourceUnitKind::ImageRegion,
            text: String::new(),
            grid: None,
            locator: StructuredSourceLocator::Image {
                original_ref: "images/page.png".into(),
                width: 10,
                height: 10,
                media_type: "image/png".into(),
                page_ordinal: Some(0),
                compound_parent: None,
                left: None,
                top: None,
                right: None,
                bottom: None,
            },
        });
        let mut receipt: Value =
            serde_json::from_str(&parsed.metadata["output_inventory_manifest"]).unwrap();
        receipt["image_sha256"] =
            serde_json::json!({"images/page.png":hex::encode(Sha256::digest([1,2,3]))});
        receipt["units"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "unit_key":"page:0:image:0","part":"pdf:page:1","ordinal":1,"kind":"image",
                "bookmarks":[],"fields":[],"status":"not_checked","reason":"visual review required"
            }));
        parsed
            .metadata
            .insert("output_inventory_manifest".into(), receipt.to_string());
        parsed
    }

    #[test]
    fn explicit_profile_accepts_blank_page_without_fake_text() {
        let parsed = blank_pdf();
        assert!(parsed.markdown.is_empty());
        assert_eq!(
            validate_output_inventory(&parsed, "immutable", "pdf")
                .unwrap()
                .page_count,
            Some(1)
        );
    }

    #[test]
    fn old_service_wrong_source_and_incomplete_pages_fail_closed() {
        assert!(validate_output_inventory(&ReadResult::default(), "immutable", "pdf").is_err());
        let mut parsed = blank_pdf();
        assert!(validate_output_inventory(&parsed, "different", "pdf").is_err());
        parsed.structured_source_units[0].key = "orphan".into();
        assert!(validate_output_inventory(&parsed, "immutable", "pdf").is_err());
        let mut parsed = blank_pdf();
        parsed.structured_source_units[0].locator = StructuredSourceLocator::Page {
            page_ordinal: 1,
            left: None,
            top: None,
            right: None,
            bottom: None,
        };
        assert!(validate_output_inventory(&parsed, "immutable", "pdf").is_err());
    }

    #[test]
    fn unreceipted_or_changed_image_payload_cannot_enter_frozen_inventory() {
        let mut parsed = blank_pdf();
        validate_output_inventory(&parsed, "immutable", "pdf").unwrap();
        parsed.images[0].data[0] = 9;
        assert!(
            validate_output_inventory(&parsed, "immutable", "pdf")
                .unwrap_err()
                .0
                .contains("image bytes differ")
        );
        let mut parsed = blank_pdf();
        let mut receipt: Value =
            serde_json::from_str(&parsed.metadata["output_inventory_manifest"]).unwrap();
        receipt["units"].as_array_mut().unwrap().pop();
        parsed.structured_source_units.pop();
        parsed
            .metadata
            .insert("output_inventory_manifest".into(), receipt.to_string());
        assert!(validate_output_inventory(&parsed, "immutable", "pdf").is_err());
    }
}
