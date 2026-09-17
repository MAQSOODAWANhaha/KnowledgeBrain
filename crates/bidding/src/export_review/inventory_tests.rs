use super::*;
use docparser::{
    ReadResult, StructuredSourceLocator, StructuredSourceUnit, StructuredSourceUnitKind,
};

fn image_bytes(format: image::ImageFormat) -> Vec<u8> {
    let mut output = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(10, 10)
        .write_to(&mut output, format)
        .unwrap();
    output.into_inner()
}

fn refresh_receipt(read: &mut docparser::OutputInventoryRead) {
    read.parsed.metadata.insert(
        "output_inventory_manifest".into(),
        serde_json::to_string(&read.manifest).unwrap(),
    );
}

fn service_read(
    file_type: &str,
    sha: &str,
    kind: &str,
    text: &str,
) -> docparser::OutputInventoryRead {
    let pdf = file_type == "pdf";
    let source = StructuredSourceUnit {
        key: "source:0".into(),
        ordinal: 0,
        kind: if kind == "not_checked" {
            StructuredSourceUnitKind::AttachmentRegion
        } else {
            StructuredSourceUnitKind::Section
        },
        text: text.into(),
        locator: if kind == "not_checked" {
            StructuredSourceLocator::Attachment {
                part_name: "/word/document.xml".into(),
                relationship_type: "output_carrier".into(),
            }
        } else if pdf {
            StructuredSourceLocator::Page {
                page_ordinal: 0,
                left: None,
                top: None,
                right: None,
                bottom: None,
            }
        } else {
            StructuredSourceLocator::Document {
                section_ordinal: 0,
                table_ordinal: None,
                row_ordinal: None,
                form_ordinal: None,
                heading_path: String::new(),
            }
        },
        grid: None,
    };
    let mut manifest: docparser::OutputInventoryManifest = serde_json::from_value(json!({
        "schema_version":1,"profile":"output_inventory_v1","file_sha256":sha,
        "parser":"test-service-profile-v1","image_sha256":{},"page_count":if pdf {json!(1)} else {Value::Null},
        "config":if pdf {json!({"profile":"output_inventory_v1","preserve_all_pages":true,"preserve_page_images":true,"text_cleaning":false,"settings":{}})}
            else {json!({"profile":"output_inventory_v1","preserve_story_occurrences":true,"settings":{}})},
        "units":[{"unit_key":"source:0","part":if pdf {"pdf:page:1"} else {"/word/header1.xml"},
            "ordinal":0,"kind":kind,"bookmarks":["first","second"],"fields":[],
            "status":if kind=="not_checked" {"not_checked"} else {"extracted"},
            "reason":if kind=="not_checked" {json!("image requires visual review")} else {Value::Null}}]
    })).unwrap();
    let mut sources = vec![source];
    let mut images = vec![];
    if pdf {
        let original_ref = "images/page.png";
        let pixels = image_bytes(image::ImageFormat::Png);
        manifest
            .image_sha256
            .insert(original_ref.into(), hex::encode(Sha256::digest(&pixels)));
        manifest.units.push(docparser::OutputInventoryEntry {
            unit_key: "source:1".into(),
            part: "pdf:page:1".into(),
            ordinal: 1,
            kind: "image".into(),
            bookmarks: vec![],
            fields: vec![],
            status: "not_checked".into(),
            reason: Some("visual review required".into()),
        });
        sources.push(StructuredSourceUnit {
            key: "source:1".into(),
            ordinal: 1,
            kind: StructuredSourceUnitKind::ImageRegion,
            text: String::new(),
            grid: None,
            locator: StructuredSourceLocator::Image {
                original_ref: original_ref.into(),
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
        images.push(docparser::ImageRef {
            original_ref: original_ref.into(),
            data: pixels,
            mime_type: "image/png".into(),
            ..Default::default()
        });
    }
    let parsed = ReadResult {
        structured_source_units: sources,
        images,
        metadata: std::collections::HashMap::from([(
            "output_inventory_manifest".into(),
            serde_json::to_string(&manifest).unwrap(),
        )]),
        ..Default::default()
    };
    docparser::OutputInventoryRead { parsed, manifest }
}

#[test]
fn mapping_preserves_story_and_all_bookmarks_and_binds_service_content() {
    let mut inventory = Inventory {
        docx_sha256: "a".repeat(64),
        ..Default::default()
    };
    let read = service_read(
        "docx",
        &inventory.docx_sha256,
        "paragraphs",
        "actual header",
    );
    append_service_inventory(&mut inventory, read, "docx").unwrap();
    assert_eq!(inventory.units[0].text, "actual header");
    assert_eq!(inventory.units[0].part, "/word/header1.xml");
    assert_eq!(inventory.units[0].bookmarks, ["first", "second"]);
    assert_eq!(
        inventory.units[0].source_unit.as_ref().unwrap()["text"],
        "actual header"
    );
    assert_eq!(inventory.parser_manifests.len(), 1);
    assert_eq!(
        serde_json::to_value(&inventory).unwrap()["images"],
        json!({})
    );
    assert!(!extra_units_not_covered_by_bookmarks(&inventory, &[]).is_empty());
}

#[test]
fn missing_profile_wrong_file_and_orphan_unit_cannot_enter_inventory() {
    let mut inventory = Inventory {
        docx_sha256: "a".repeat(64),
        ..Default::default()
    };
    let mut read = service_read("docx", &inventory.docx_sha256, "paragraphs", "text");
    read.parsed.metadata.clear();
    assert!(
        append_service_inventory(&mut inventory, read, "docx")
            .unwrap_err()
            .contains("profile receipt missing")
    );
    let read = service_read("docx", &"b".repeat(64), "paragraphs", "text");
    assert!(append_service_inventory(&mut inventory, read, "docx").is_err());
    let mut read = service_read("docx", &inventory.docx_sha256, "paragraphs", "text");
    read.parsed.structured_source_units[0].key = "orphan".into();
    assert!(append_service_inventory(&mut inventory, read, "docx").is_err());
    assert!(inventory.units.is_empty());
}

#[test]
fn blank_pdf_page_is_retained_and_missing_pages_are_rejected() {
    let mut inventory = Inventory {
        pdf_sha256: Some("b".repeat(64)),
        ..Default::default()
    };
    let read = service_read(
        "pdf",
        inventory.pdf_sha256.as_ref().unwrap(),
        "pdf_page",
        "",
    );
    append_service_inventory(&mut inventory, read, "pdf").unwrap();
    assert_eq!(inventory.units.len(), 2);
    assert!(inventory.units[0].text.is_empty());
    let mut read = service_read(
        "pdf",
        inventory.pdf_sha256.as_ref().unwrap(),
        "pdf_page",
        "",
    );
    read.manifest.page_count = Some(2);
    read.parsed.metadata.insert(
        "output_inventory_manifest".into(),
        serde_json::to_string(&read.manifest).unwrap(),
    );
    assert!(
        append_service_inventory(&mut inventory, read, "pdf")
            .unwrap_err()
            .contains("page inventory incomplete")
    );
}

#[test]
fn unsupported_carrier_keeps_reason_and_delivery_receipts_are_bounded() {
    let mut inventory = Inventory {
        docx_sha256: "a".repeat(64),
        ..Default::default()
    };
    let read = service_read("docx", &inventory.docx_sha256, "not_checked", "");
    append_service_inventory(&mut inventory, read, "docx").unwrap();
    assert_eq!(inventory.units[0].kind, "not_checked");
    assert!(
        inventory.units[0]
            .not_checked_reason
            .as_ref()
            .unwrap()
            .contains("visual")
    );
    let mut coverage = OutputCoverage::default();
    let args = json!({"offset":0,"limit":8});
    assert!(read_output_evidence(&inventory, &mut coverage, &args, 1).is_err());
    assert!(coverage.units.is_empty());
    read_output_evidence(&inventory, &mut coverage, &args, 16_000).unwrap();
    assert_eq!(coverage.units.len(), 1);
}

#[test]
fn parser_errors_preserve_queue_meaning_without_message_heuristics() {
    use crate::agent_error::RequestQueueEffect;
    use docparser::DocReaderReadError;
    let network = inventory_read_error(DocReaderReadError::Transient("invalid schema".into()));
    assert_eq!(network.code, "INTERNAL");
    assert_eq!(
        network.request_queue_effect(),
        RequestQueueEffect::YieldThenRetry
    );
    let response = inventory_read_error(DocReaderReadError::InvalidResponse(
        "connection refused".into(),
    ));
    assert_eq!(response.code, "FROZEN_INPUT_DIGEST_MISMATCH");
    assert_eq!(
        response.request_queue_effect(),
        RequestQueueEffect::FailRequest
    );
    let config = inventory_read_error(DocReaderReadError::Configuration("timeout".into()));
    assert_eq!(
        config.request_queue_effect(),
        RequestQueueEffect::FailRequest
    );
    let cancelled = inventory_read_error(DocReaderReadError::Cancelled);
    assert_eq!(cancelled.code, "INTERNAL");
    assert_eq!(
        cancelled.request_queue_effect(),
        RequestQueueEffect::YieldThenRetry
    );
    assert_eq!(
        inventory_contract_error("mapping mismatch".into()).request_queue_effect(),
        RequestQueueEffect::FailRequest
    );
}

#[tokio::test]
async fn cancelled_inventory_read_is_retryable_before_service_connection() {
    let cancel = CancellationToken::new();
    cancel.cancel();
    let error = inventory_from_files(b"unused bytes", None, &cancel)
        .await
        .unwrap_err();
    assert_eq!(error.code, "INTERNAL");
    assert_eq!(error.message, "cancelled");
}

#[test]
fn real_pixels_bind_only_image_occurrences_and_deduplicate_storage() {
    let mut inventory = Inventory {
        pdf_sha256: Some("b".repeat(64)),
        ..Default::default()
    };
    let mut read = service_read(
        "pdf",
        inventory.pdf_sha256.as_ref().unwrap(),
        "pdf_page",
        "adjacent text",
    );
    let mut repeated = read.parsed.structured_source_units[1].clone();
    repeated.key = "source:2".into();
    repeated.ordinal = 2;
    read.parsed.structured_source_units.push(repeated);
    let mut repeated = read.manifest.units[1].clone();
    repeated.unit_key = "source:2".into();
    repeated.ordinal = 2;
    read.manifest.units.push(repeated);
    refresh_receipt(&mut read);
    let original = read.parsed.images[0].data.clone();
    let pixels = append_service_inventory(&mut inventory, read, "pdf").unwrap();
    let sha = hex::encode(Sha256::digest(&original));
    assert_eq!(pixels.len(), 1);
    assert_eq!(inventory.images.len(), 1);
    assert_eq!(pixels[&sha], original);
    let image = &inventory.images[&sha];
    assert_eq!((image.width, image.height), (10, 10));
    assert_eq!(image.object_ref, format!("objects/{sha}"));
    assert!(image.supports_model_view());
    image.validate_bytes(&pixels[&sha]).unwrap();
    assert!(inventory.units[0].image_sha256s.is_empty());
    assert_eq!(inventory.units[1].image_sha256s, std::slice::from_ref(&sha));
    assert_eq!(inventory.units[2].image_sha256s, [sha]);
    assert_eq!(inventory.units[1].kind, "not_checked");
    let checkpoint_json = serde_json::to_value(&inventory).unwrap();
    assert!(
        checkpoint_json["images"]
            .as_object()
            .unwrap()
            .values()
            .all(|value| value.get("data").is_none())
    );
}

#[test]
fn corrupted_pixels_false_media_and_false_dimensions_are_rejected() {
    for mutation in ["bytes", "media", "dimensions", "reference"] {
        let mut inventory = Inventory {
            pdf_sha256: Some("b".repeat(64)),
            ..Default::default()
        };
        let mut read = service_read(
            "pdf",
            inventory.pdf_sha256.as_ref().unwrap(),
            "pdf_page",
            "",
        );
        match mutation {
            "bytes" => read.parsed.images[0].data[0] ^= 1,
            "media" => read.parsed.images[0].mime_type = "image/jpeg".into(),
            "dimensions" => {
                if let StructuredSourceLocator::Image { width, .. } =
                    &mut read.parsed.structured_source_units[1].locator
                {
                    *width = 11;
                }
            }
            "reference" => {
                if let StructuredSourceLocator::Image { original_ref, .. } =
                    &mut read.parsed.structured_source_units[1].locator
                {
                    *original_ref = "images/missing.png".into();
                }
            }
            _ => unreachable!(),
        }
        assert!(
            append_service_inventory(&mut inventory, read, "pdf").is_err(),
            "{mutation}"
        );
    }
    let bytes = image_bytes(image::ImageFormat::Png);
    let mut metadata = OutputImage::from_bytes(&bytes, "image/png").unwrap();
    metadata.width += 1;
    assert!(metadata.validate_bytes(&bytes).is_err());
}

#[test]
fn unsupported_real_image_is_retained_but_does_not_become_a_model_view() {
    let mut inventory = Inventory {
        pdf_sha256: Some("b".repeat(64)),
        ..Default::default()
    };
    let mut read = service_read(
        "pdf",
        inventory.pdf_sha256.as_ref().unwrap(),
        "pdf_page",
        "",
    );
    let bytes = image_bytes(image::ImageFormat::Gif);
    let sha = hex::encode(Sha256::digest(&bytes));
    read.parsed.images[0].data = bytes.clone();
    read.parsed.images[0].mime_type = "image/gif".into();
    if let StructuredSourceLocator::Image { media_type, .. } =
        &mut read.parsed.structured_source_units[1].locator
    {
        *media_type = "image/gif".into();
    }
    read.manifest
        .image_sha256
        .insert("images/page.png".into(), sha.clone());
    refresh_receipt(&mut read);
    let pixels = append_service_inventory(&mut inventory, read, "pdf").unwrap();
    assert_eq!(pixels[&sha], bytes);
    assert!(!inventory.images[&sha].supports_model_view());
    assert_eq!(inventory.images[&sha].media_type, "image/gif");
    assert_eq!(inventory.units[1].kind, "not_checked");
    assert!(
        inventory.units[1]
            .not_checked_reason
            .as_ref()
            .unwrap()
            .contains("unsupported output image format")
    );
    assert!(inventory.units[0].image_sha256s.is_empty());
}

#[test]
fn unknown_vector_carrier_is_retained_without_invented_pixel_dimensions() {
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"/>"#;
    let image = OutputImage::from_bytes(svg, "image/svg+xml").unwrap();
    assert_eq!((image.width, image.height), (0, 0));
    assert!(!image.supports_model_view());
    image.validate_bytes(svg).unwrap();
    assert!(OutputImage::from_bytes(svg, "image/png").is_err());
    let png = image_bytes(image::ImageFormat::Png);
    let image = OutputImage::from_bytes(&png, "application/octet-stream").unwrap();
    assert_eq!(image.media_type, "image/png");
    assert!(image.supports_model_view());
}
