use std::io::{Cursor, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bidding::tender_process::*;
use bidding::tender_upload::{
    DOC_MEDIA_TYPE, DOCX_MEDIA_TYPE, JPEG_MEDIA_TYPE, PDF_MEDIA_TYPE, PNG_MEDIA_TYPE,
    WEBP_MEDIA_TYPE, XLS_MEDIA_TYPE, XLSM_MEDIA_TYPE, XLSX_MEDIA_TYPE,
};
use docparser::{
    CompoundImageParent, ImageRef, PdfTableCell, ReadResult, SpreadsheetCell, SpreadsheetRange,
    SpreadsheetTableIdentity, StructuredSourceLocator, StructuredSourceUnit,
    StructuredSourceUnitKind, TableGrid,
};
use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba};
use platform::{BidAuthoringJobPayloadV2, BidAuthoringRequestIdentityV2};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use zip::write::SimpleFileOptions;

struct StoredSuccessfulReceipt {
    request: BidAuthoringRequestIdentityV2,
    project_id: Uuid,
    document_id: Uuid,
    document_sha256: String,
    converter_contract_id: Uuid,
    converter_contract_sha256: String,
    source_sha: String,
    unit_shas: Vec<String>,
    receipt: TenderDocumentProcessReceipt,
}

#[derive(Clone)]
struct MockRepository {
    document: Arc<Mutex<FrozenTenderDocument>>,
    objects: Arc<Mutex<Vec<FrozenObjectIdentity>>>,
    first: Arc<Mutex<Option<StoredSuccessfulReceipt>>>,
    publications: Arc<Mutex<Vec<TenderDocumentPublication>>>,
    abandoned: Arc<AtomicUsize>,
}

impl MockRepository {
    fn new(document: FrozenTenderDocument) -> Self {
        Self {
            document: Arc::new(Mutex::new(document)),
            objects: Arc::new(Mutex::new(Vec::new())),
            first: Arc::new(Mutex::new(None)),
            publications: Arc::new(Mutex::new(Vec::new())),
            abandoned: Arc::new(AtomicUsize::new(0)),
        }
    }
}

#[async_trait]
impl TenderDocumentProcessRepository for MockRepository {
    async fn load_frozen_document(
        &self,
        _payload: &BidAuthoringJobPayloadV2,
        _cancel: &CancellationToken,
    ) -> Result<Option<FrozenTenderDocument>, TenderDocumentProcessError> {
        Ok(Some(self.document.lock().unwrap().clone()))
    }

    async fn load_successful_receipt(
        &self,
        document: &FrozenTenderDocument,
    ) -> Result<Option<TenderDocumentProcessReceipt>, TenderDocumentProcessError> {
        let first = self.first.lock().unwrap();
        let Some(stored) = first.as_ref() else {
            return Ok(None);
        };
        if stored.request != document.request
            || stored.project_id != document.project_id
            || stored.document_id != document.document_id
            || stored.document_sha256 != document.document_sha256
            || stored.converter_contract_id != document.converter_contract_id
            || stored.converter_contract_sha256 != document.converter_contract_sha256
        {
            return Ok(None);
        }
        Ok(Some(stored.receipt.clone()))
    }

    async fn stage_object(
        &self,
        _owner_id: Uuid,
        _occurrence: &str,
        media_type: &str,
        bytes: &[u8],
    ) -> Result<FrozenObjectIdentity, TenderDocumentProcessError> {
        if bytes.is_empty() {
            return Err(TenderDocumentProcessError::ObjectFreeze("empty".into()));
        }
        let sha256 = hex::encode(Sha256::digest(bytes));
        let object = FrozenObjectIdentity {
            // Production staging attempts are intentionally ephemeral. Frozen
            // artifact hashes must remain stable across a fresh staging UUID.
            staging_id: Uuid::new_v4(),
            object_ref: format!("objects/{sha256}"),
            sha256,
            media_type: media_type.into(),
            byte_length: bytes.len() as i64,
        };
        self.objects.lock().unwrap().push(object.clone());
        Ok(object)
    }

    async fn abandon_staged_object(
        &self,
        _object: &FrozenObjectIdentity,
    ) -> Result<(), TenderDocumentProcessError> {
        self.abandoned.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn publish(
        &self,
        publication: TenderDocumentPublication,
    ) -> Result<TenderDocumentProcessReceipt, TenderDocumentProcessError> {
        self.publications.lock().unwrap().push(publication.clone());
        let source_sha = publication.converted_source.source_object.sha256.clone();
        let unit_shas = publication
            .source_units
            .iter()
            .map(|unit| unit.content_sha256.clone())
            .collect::<Vec<_>>();
        let mut first = self.first.lock().unwrap();
        if let Some(stored) = first.as_ref() {
            if stored.source_sha != source_sha || stored.unit_shas != unit_shas {
                return Err(TenderDocumentProcessError::Publication(
                    "same request has conflicting source unit SHA".into(),
                ));
            }
            let mut replay = stored.receipt.clone();
            replay.replayed = true;
            return Ok(replay);
        }
        let receipt = TenderDocumentProcessReceipt {
            request_artifact_id: publication.request.request_artifact_id,
            converted_source_revision_id: publication.converted_source.id,
            converted_source_sha256: source_sha.clone(),
            image_asset_set_sha256: publication.converted_source.image_asset_set_sha256.clone(),
            source_unit_set_sha256: publication.converted_source.source_unit_set_sha256.clone(),
            source_unit_count: publication.source_units.len() as u32,
            image_ocr_region_count: publication.image_artifacts.len() as u32,
            replayed: false,
        };
        *first = Some(StoredSuccessfulReceipt {
            request: publication.request.clone(),
            project_id: publication.project_id,
            document_id: publication.document_id,
            document_sha256: publication.document_sha256.clone(),
            converter_contract_id: publication.converted_source.converter_contract_id,
            converter_contract_sha256: publication
                .converted_source
                .converter_contract_sha256
                .clone(),
            source_sha,
            unit_shas,
            receipt: receipt.clone(),
        });
        Ok(receipt)
    }
}

#[derive(Clone)]
struct FixtureConverter {
    image_bytes: Vec<u8>,
    seen: Arc<Mutex<Vec<String>>>,
    omit_image_bytes: bool,
    empty_units: bool,
}

#[async_trait]
impl TenderSourceConverter for FixtureConverter {
    async fn convert(
        &self,
        file_name: &str,
        bytes: Vec<u8>,
        _cancel: &CancellationToken,
    ) -> Result<ReadResult, TenderDocumentProcessError> {
        self.seen.lock().unwrap().push(file_name.to_string());
        let ext = file_name.rsplit('.').next().unwrap();
        let mut result = ReadResult {
            markdown: format!("# {file_name}"),
            ..ReadResult::default()
        };
        if self.empty_units {
            return Ok(result);
        }
        match ext {
            "pdf" => {
                result.structured_source_units.push(unit(
                    0,
                    "pdf-page",
                    StructuredSourceUnitKind::Section,
                    "PDF requirement",
                    StructuredSourceLocator::Page {
                        page_ordinal: 0,
                        left: Some(0.0),
                        top: Some(0.0),
                        right: Some(1.0),
                        bottom: Some(1.0),
                    },
                ));
                let mut table = unit(
                    1,
                    "pdf-table",
                    StructuredSourceUnitKind::TableRegion,
                    "",
                    StructuredSourceLocator::PageTable {
                        page_ordinal: 0,
                        table_ordinal: 0,
                        left: 40.0,
                        top: 700.0,
                        right: 520.0,
                        bottom: 620.0,
                    },
                );
                table.grid = Some(TableGrid {
                    row_count: 1,
                    column_count: 3,
                    cells: vec![
                        PdfTableCell {
                            row: 0,
                            column: 0,
                            row_span: 1,
                            col_span: 1,
                            text: "列甲".into(),
                        },
                        PdfTableCell {
                            row: 0,
                            column: 1,
                            row_span: 1,
                            col_span: 1,
                            text: "列乙".into(),
                        },
                        PdfTableCell {
                            row: 0,
                            column: 2,
                            row_span: 1,
                            col_span: 1,
                            text: "列丙".into(),
                        },
                    ],
                    widths_mm: Some(vec![45.0, 67.5, 67.5]),
                });
                result.structured_source_units.push(table);
                add_image(&mut result, 2, "pdf-image", self, Some(0), None);
            }
            "docx" | "doc" => {
                result.structured_source_units.push(unit(
                    0,
                    "doc-section",
                    StructuredSourceUnitKind::Section,
                    "Section requirement",
                    document_locator(0, None, None, None),
                ));
                result.structured_source_units.push(unit(
                    1,
                    "doc-row",
                    StructuredSourceUnitKind::TableRow,
                    "Table requirement",
                    document_locator(0, Some(0), Some(0), None),
                ));
                result.structured_source_units.push(unit(
                    2,
                    "doc-form",
                    StructuredSourceUnitKind::FormRegion,
                    "Form requirement",
                    document_locator(0, None, None, Some(0)),
                ));
                result.structured_source_units.push(unit(
                    3,
                    "doc-attachment",
                    StructuredSourceUnitKind::AttachmentRegion,
                    "Attachment requirement",
                    StructuredSourceLocator::Attachment {
                        part_name: "word/embeddings/item1.pdf".into(),
                        relationship_type: "package".into(),
                    },
                ));
                add_image(
                    &mut result,
                    4,
                    "doc-image",
                    self,
                    None,
                    Some(CompoundImageParent::TableCell {
                        section_ordinal: 0,
                        table_ordinal: 0,
                        row_ordinal: 0,
                        cell_ordinal: 0,
                    }),
                );
            }
            "xlsx" | "xlsm" | "xls" => result.structured_source_units.push(unit(
                0,
                "sheet-row",
                StructuredSourceUnitKind::TableRow,
                "A1 requirement",
                StructuredSourceLocator::Spreadsheet {
                    sheet_ordinal: 0,
                    sheet_name: "Requirements".into(),
                    region: SpreadsheetRange {
                        a1_range: "A1:B1".into(),
                        start_row: 1,
                        start_column: 1,
                        end_row: 1,
                        end_column: 2,
                    },
                    cells: vec![SpreadsheetCell {
                        address: "A1".into(),
                        row: 1,
                        column: 1,
                        text: "requirement".into(),
                    }],
                    merged_ranges: vec![SpreadsheetRange {
                        a1_range: "A1:B1".into(),
                        start_row: 1,
                        start_column: 1,
                        end_row: 1,
                        end_column: 2,
                    }],
                    defined_tables: vec![SpreadsheetTableIdentity {
                        name: "RequirementsTable".into(),
                        display_name: "RequirementsTable".into(),
                        a1_range: "A1:B2".into(),
                    }],
                },
            )),
            "png" | "jpg" | "webp" => {
                add_image_bytes(&mut result, 0, "standalone-image", bytes, ext, None, None);
                if self.omit_image_bytes {
                    result.images.clear();
                }
            }
            _ => unreachable!(),
        }
        Ok(result)
    }
}

fn add_image(
    result: &mut ReadResult,
    ordinal: u32,
    key: &str,
    converter: &FixtureConverter,
    page_ordinal: Option<u32>,
    compound_parent: Option<CompoundImageParent>,
) {
    if converter.omit_image_bytes {
        add_image_bytes(
            result,
            ordinal,
            key,
            Vec::new(),
            "png",
            page_ordinal,
            compound_parent,
        );
        result.images.clear();
    } else {
        add_image_bytes(
            result,
            ordinal,
            key,
            converter.image_bytes.clone(),
            "png",
            page_ordinal,
            compound_parent,
        );
    }
}

fn add_image_bytes(
    result: &mut ReadResult,
    ordinal: u32,
    key: &str,
    bytes: Vec<u8>,
    extension: &str,
    page_ordinal: Option<u32>,
    compound_parent: Option<CompoundImageParent>,
) {
    let media_type = match extension {
        "png" => PNG_MEDIA_TYPE,
        "jpg" => JPEG_MEDIA_TYPE,
        "webp" => WEBP_MEDIA_TYPE,
        _ => unreachable!(),
    };
    let original_ref = format!("images/{key}.{extension}");
    result.structured_source_units.push(unit(
        ordinal,
        key,
        StructuredSourceUnitKind::ImageRegion,
        "",
        StructuredSourceLocator::Image {
            original_ref: original_ref.clone(),
            width: 2,
            height: 3,
            media_type: media_type.into(),
            page_ordinal,
            compound_parent,
            left: Some(0.0),
            top: Some(0.0),
            right: Some(1.0),
            bottom: Some(1.0),
        },
    ));
    result.images.push(ImageRef {
        filename: format!("{key}.{extension}"),
        original_ref,
        mime_type: media_type.into(),
        storage_key: String::new(),
        data: bytes,
    });
}

#[derive(Clone)]
struct MockVision {
    text: Arc<Mutex<Result<String, String>>>,
}

#[async_trait]
impl TenderVisionEnricher for MockVision {
    async fn enrich(
        &self,
        _image_bytes: &[u8],
        _image_media_type: &str,
        _image_source_type: &str,
        _output_language: &str,
        _cancel: &CancellationToken,
    ) -> Result<VisionEnrichment, TenderDocumentProcessError> {
        let text = self
            .text
            .lock()
            .unwrap()
            .clone()
            .map_err(TenderDocumentProcessError::Vision)?;
        let model_payload = b"test-vision-model-v1".to_vec();
        let operation_payload = b"tender-image-ocr-v1".to_vec();
        Ok(VisionEnrichment {
            ocr_text: text,
            caption: "caption not used as OCR".into(),
            model_contract: FrozenContractIdentity {
                id: deterministic_uuid(&model_payload),
                sha256: hex::encode(Sha256::digest(&model_payload)),
                canonical_payload: model_payload,
            },
            operation_contract: FrozenContractIdentity {
                id: deterministic_uuid(&operation_payload),
                sha256: hex::encode(Sha256::digest(&operation_payload)),
                canonical_payload: operation_payload,
            },
        })
    }
}

#[derive(Clone, Default)]
struct MockTransport(Arc<AtomicUsize>);

#[async_trait]
impl TenderProcessTransport for MockTransport {
    async fn enqueue_requirement_set_compile(
        &self,
        _project_id: Uuid,
    ) -> Result<(), TenderDocumentProcessError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn accepted_formats_use_one_document_path_and_freeze_typed_provenance() {
    let png = image_bytes(ImageFormat::Png);
    let fixtures = vec![
        ("tender.pdf", PDF_MEDIA_TYPE, minimal_pdf()),
        ("tender.docx", DOCX_MEDIA_TYPE, office(true)),
        ("tender.xlsx", XLSX_MEDIA_TYPE, office(false)),
        ("tender.xlsm", XLSM_MEDIA_TYPE, office_xlsm()),
        ("tender.doc", DOC_MEDIA_TYPE, ole_container("WordDocument")),
        ("tender.xls", XLS_MEDIA_TYPE, ole_container("Workbook")),
        ("tender.png", PNG_MEDIA_TYPE, png.clone()),
        (
            "tender.jpg",
            JPEG_MEDIA_TYPE,
            image_bytes(ImageFormat::Jpeg),
        ),
        (
            "tender.webp",
            WEBP_MEDIA_TYPE,
            image_bytes(ImageFormat::WebP),
        ),
    ];
    for (name, media_type, bytes) in fixtures {
        let (document, payload) = frozen_fixture(name, media_type, bytes);
        let repository = MockRepository::new(document);
        let converter = FixtureConverter {
            image_bytes: png.clone(),
            seen: Arc::new(Mutex::new(Vec::new())),
            omit_image_bytes: false,
            empty_units: false,
        };
        let seen = converter.seen.clone();
        let transport = MockTransport::default();
        let transport_count = transport.0.clone();
        let service = TenderDocumentProcessService::new(
            repository.clone(),
            converter,
            vision("verified OCR text"),
            transport,
        );
        let receipt = service
            .process(&payload, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(seen.lock().unwrap().as_slice(), &[name.to_string()]);
        assert!(!receipt.replayed);
        assert_eq!(transport_count.load(Ordering::SeqCst), 0);
        let objects = repository.objects.lock().unwrap();
        assert!(
            objects
                .iter()
                .any(|value| value.media_type == "application/json")
        );
        let publications = repository.publications.lock().unwrap();
        let publication = publications.first().unwrap();
        assert_eq!(
            publication.converted_source.converter_contract_id,
            Uuid::from_u128(20)
        );
        assert_eq!(
            publication.converted_source.converter_contract_sha256,
            tender_converter_contract_sha256()
        );
        assert_eq!(
            publication.converted_source.source_unit_set_sha256,
            receipt.source_unit_set_sha256
        );
        assert!(publication.source_units.iter().all(|unit| {
            unit.source_span_v2.source_purpose == TENDER_SOURCE_PURPOSE
                && unit.source_span_v2.project_id == publication.project_id
                && unit.source_span_v2.document_id == publication.document_id
                && unit.source_span_v2.converted_source_revision_id
                    == publication.converted_source.id
                && {
                    let persisted = serde_json::to_value(&unit.source_span_v2).unwrap();
                    persisted["schema_version"] == 2
                        && persisted["locator"].is_object()
                        && persisted["locator"]["locator_kind"].is_string()
                        && persisted.get("locator_kind").is_none()
                }
        }));
        if name.ends_with("xlsx") || name.ends_with("xlsm") || name.ends_with("xls") {
            assert_eq!(receipt.image_ocr_region_count, 0);
        } else {
            assert_eq!(receipt.image_ocr_region_count, 1);
            let expected_image_media = if name.ends_with(".jpg") {
                JPEG_MEDIA_TYPE
            } else if name.ends_with(".webp") {
                WEBP_MEDIA_TYPE
            } else {
                PNG_MEDIA_TYPE
            };
            assert!(
                objects
                    .iter()
                    .any(|value| value.media_type == expected_image_media)
            );
            assert!(objects.iter().any(|value| value.media_type == "text/plain"));
            let image = publication.image_artifacts.first().unwrap();
            let canonical: serde_json::Value =
                serde_json::from_slice(&image.canonical_payload).unwrap();
            assert!(canonical["original_object"].get("staging_id").is_none());
            assert!(canonical["ocr_text_object"].get("staging_id").is_none());
            assert_eq!(
                canonical["original_object"]["media_type"],
                expected_image_media
            );
        }
    }
}

#[tokio::test]
async fn replay_is_deterministic_conflicting_ocr_is_rejected_and_staging_is_abandoned() {
    let bytes = image_bytes(ImageFormat::Png);
    let (document, payload) = frozen_fixture("scan.png", PNG_MEDIA_TYPE, bytes);
    let repository = MockRepository::new(document);
    let converter = FixtureConverter {
        image_bytes: image_bytes(ImageFormat::Png),
        seen: Arc::new(Mutex::new(Vec::new())),
        omit_image_bytes: false,
        empty_units: false,
    };
    let seen = converter.seen.clone();
    let mutable_text = Arc::new(Mutex::new(Ok("first OCR".into())));
    let service = TenderDocumentProcessService::new(
        repository.clone(),
        converter,
        MockVision {
            text: mutable_text.clone(),
        },
        MockTransport::default(),
    );
    let first = service
        .process(&payload, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(seen.lock().unwrap().as_slice(), &["scan.png".to_string()]);
    assert!(!first.replayed);
    let objects_after_first = repository.objects.lock().unwrap().len();
    let abandoned_after_first = repository.abandoned.load(Ordering::SeqCst);
    let replay = service
        .process(&payload, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(seen.lock().unwrap().as_slice(), &["scan.png".to_string()]);
    assert_eq!(
        first.converted_source_revision_id,
        replay.converted_source_revision_id
    );
    assert!(replay.replayed);
    assert_eq!(
        repository.objects.lock().unwrap().len(),
        objects_after_first
    );
    assert_eq!(
        repository.abandoned.load(Ordering::SeqCst),
        abandoned_after_first
    );
    *mutable_text.lock().unwrap() = Ok("different OCR".into());
    let skipped = service
        .process(&payload, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert!(skipped.replayed);
    assert_eq!(
        skipped.converted_source_sha256,
        first.converted_source_sha256
    );
}

#[tokio::test]
async fn first_failure_or_changed_identity_allows_convert_again() {
    let bytes = image_bytes(ImageFormat::Png);
    let (document, payload) = frozen_fixture("scan.png", PNG_MEDIA_TYPE, bytes.clone());
    let repository = MockRepository::new(document);
    let converter = FixtureConverter {
        image_bytes: image_bytes(ImageFormat::Png),
        seen: Arc::new(Mutex::new(Vec::new())),
        omit_image_bytes: false,
        empty_units: false,
    };
    let seen = converter.seen.clone();
    let mutable_text = Arc::new(Mutex::new(Err("model unavailable".into())));
    let service = TenderDocumentProcessService::new(
        repository.clone(),
        converter,
        MockVision {
            text: mutable_text.clone(),
        },
        MockTransport::default(),
    );
    assert!(matches!(
        service.process(&payload, &CancellationToken::new()).await,
        Err(TenderDocumentProcessError::Vision(_))
    ));
    assert_eq!(seen.lock().unwrap().len(), 1);

    *mutable_text.lock().unwrap() = Ok("first OCR".into());
    let recovered = service
        .process(&payload, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(seen.lock().unwrap().len(), 2);
    assert!(!recovered.replayed);

    {
        let mut frozen = repository.document.lock().unwrap();
        frozen.converter_contract_sha256 = "c".repeat(64);
    }
    assert!(matches!(
        service.process(&payload, &CancellationToken::new()).await,
        Err(TenderDocumentProcessError::FrozenInputMismatch(message))
            if message.contains(TENDER_CONVERTER_OPERATION)
    ));
    assert_eq!(seen.lock().unwrap().len(), 2);

    let jpeg = image_bytes(ImageFormat::Jpeg);
    {
        let mut frozen = repository.document.lock().unwrap();
        frozen.converter_contract_sha256 = tender_converter_contract_sha256();
        frozen.bytes = jpeg.clone();
        frozen.document_sha256 = hex::encode(Sha256::digest(&jpeg));
        frozen.file_name = "scan.jpg".into();
        frozen.media_type = JPEG_MEDIA_TYPE.into();
    }
    assert!(matches!(
        service.process(&payload, &CancellationToken::new()).await,
        Err(TenderDocumentProcessError::Publication(message)) if message.contains("conflicting")
    ));
    assert_eq!(seen.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn digest_missing_image_model_and_empty_ocr_fail_closed() {
    let bytes = image_bytes(ImageFormat::Png);
    let (mut document, payload) = frozen_fixture("scan.png", PNG_MEDIA_TYPE, bytes.clone());
    document.document_sha256 = "f".repeat(64);
    let bad_digest = service_for(document, false, vision("OCR"));
    assert!(matches!(
        bad_digest
            .process(&payload, &CancellationToken::new())
            .await,
        Err(TenderDocumentProcessError::FrozenInputMismatch(_))
    ));

    let (document, payload) = frozen_fixture("scan.png", PNG_MEDIA_TYPE, bytes.clone());
    let missing = service_for(document, true, vision("OCR"));
    assert!(matches!(
        missing.process(&payload, &CancellationToken::new()).await,
        Err(TenderDocumentProcessError::MissingImage(_))
    ));

    let (document, payload) = frozen_fixture("scan.png", PNG_MEDIA_TYPE, bytes.clone());
    let failed = service_for(
        document,
        false,
        MockVision {
            text: Arc::new(Mutex::new(Err("model unavailable".into()))),
        },
    );
    assert!(matches!(
        failed.process(&payload, &CancellationToken::new()).await,
        Err(TenderDocumentProcessError::Vision(_))
    ));

    let (document, payload) = frozen_fixture("scan.png", PNG_MEDIA_TYPE, bytes);
    let repository = MockRepository::new(document);
    let service = TenderDocumentProcessService::new(
        repository.clone(),
        FixtureConverter {
            image_bytes: image_bytes(ImageFormat::Png),
            seen: Arc::new(Mutex::new(Vec::new())),
            omit_image_bytes: false,
            empty_units: false,
        },
        vision("  "),
        MockTransport::default(),
    );
    let receipt = service
        .process(&payload, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(receipt.image_ocr_region_count, 1);
    let publication = repository.publications.lock().unwrap();
    let unit = publication[0]
        .source_units
        .iter()
        .find(|unit| unit.unit_kind == PublishedSourceUnitKind::ImageOcrRegion)
        .unwrap();
    assert!(unit.text_utf8.is_empty());
    assert!(
        publication[0]
            .image_artifacts
            .iter()
            .any(|image| image.ocr_text.byte_length == UNRESOLVED_EMPTY_OCR.len() as i64)
    );
}

#[tokio::test]
async fn empty_structured_units_fail_closed_without_markdown_body() {
    let bytes = image_bytes(ImageFormat::Png);
    let (document, payload) = frozen_fixture("scan.png", PNG_MEDIA_TYPE, bytes);
    let service = TenderDocumentProcessService::new(
        MockRepository::new(document),
        FixtureConverter {
            image_bytes: image_bytes(ImageFormat::Png),
            seen: Arc::new(Mutex::new(Vec::new())),
            omit_image_bytes: false,
            empty_units: true,
        },
        vision("OCR"),
        MockTransport::default(),
    );
    assert!(matches!(
        service.process(&payload, &CancellationToken::new()).await,
        Err(TenderDocumentProcessError::StructuredSource(message))
            if message.contains("no structured units")
    ));
}

#[tokio::test]
async fn wrong_job_kind_and_tender_evidence_promotion_are_explicitly_rejected() {
    let bytes = image_bytes(ImageFormat::Png);
    let (document, payload) = frozen_fixture("scan.png", PNG_MEDIA_TYPE, bytes);
    let service = service_for(document, false, vision("OCR"));
    let request = payload.request().clone();
    let wrong = BidAuthoringJobPayloadV2::RequirementSetCompile {
        request,
        project_id: Uuid::from_u128(1),
        document_set_revision_id: Uuid::from_u128(2),
        disposition_set_revision_id: Uuid::from_u128(3),
    };
    assert_eq!(
        service.process(&wrong, &CancellationToken::new()).await,
        Err(TenderDocumentProcessError::WrongJobKind)
    );

    let span = SourceUnitSpanV2 {
        schema_version: 2,
        project_id: Uuid::from_u128(1),
        document_id: Uuid::from_u128(2),
        converted_source_revision_id: Uuid::from_u128(3),
        parser_unit_key: "unit".into(),
        parser_ordinal: 0,
        source_purpose: TENDER_SOURCE_PURPOSE.into(),
        locator: document_locator(0, None, None, None),
        tender_image_artifact_revision_id: None,
    };
    let unit = SourceUnitRevision {
        id: Uuid::from_u128(4),
        lineage_id: Uuid::from_u128(5),
        revision: 1,
        ordinal: 0,
        unit_kind: PublishedSourceUnitKind::Section,
        source_span_v2: span,
        source_span_sha256: "a".repeat(64),
        text_utf8: "tender requirement".into(),
        text_sha256: "b".repeat(64),
        canonical_payload: vec![1],
        content_sha256: "c".repeat(64),
        grid: None,
    };
    assert!(!tender_source_unit_can_be_bidder_evidence(&unit));
}

fn service_for(
    document: FrozenTenderDocument,
    omit_image_bytes: bool,
    vision: MockVision,
) -> TenderDocumentProcessService<MockRepository, FixtureConverter, MockVision, MockTransport> {
    TenderDocumentProcessService::new(
        MockRepository::new(document),
        FixtureConverter {
            image_bytes: image_bytes(ImageFormat::Png),
            seen: Arc::new(Mutex::new(Vec::new())),
            omit_image_bytes,
            empty_units: false,
        },
        vision,
        MockTransport::default(),
    )
}

fn vision(text: &str) -> MockVision {
    MockVision {
        text: Arc::new(Mutex::new(Ok(text.into()))),
    }
}

fn frozen_fixture(
    file_name: &str,
    media_type: &str,
    bytes: Vec<u8>,
) -> (FrozenTenderDocument, BidAuthoringJobPayloadV2) {
    let request = BidAuthoringRequestIdentityV2 {
        request_artifact_id: deterministic_uuid(file_name.as_bytes()),
        request_revision: 1,
        frozen_input_sha256: hex::encode(Sha256::digest(format!("input:{file_name}"))),
    };
    let project_id = Uuid::from_u128(10);
    let document_id = deterministic_uuid(format!("document:{file_name}").as_bytes());
    let document = FrozenTenderDocument {
        request: request.clone(),
        project_id,
        document_id,
        document_sha256: hex::encode(Sha256::digest(&bytes)),
        role_revision_id: deterministic_uuid(format!("role:{file_name}").as_bytes()),
        role_revision_sha256: "a".repeat(64),
        converter_contract_id: Uuid::from_u128(20),
        converter_contract_sha256: tender_converter_contract_sha256(),
        file_name: file_name.into(),
        media_type: media_type.into(),
        bytes,
    };
    let payload = BidAuthoringJobPayloadV2::TenderDocumentProcess {
        request,
        project_id,
        document_revision_id: document_id,
    };
    (document, payload)
}

fn unit(
    ordinal: u32,
    key: &str,
    kind: StructuredSourceUnitKind,
    text: &str,
    locator: StructuredSourceLocator,
) -> StructuredSourceUnit {
    StructuredSourceUnit {
        key: key.into(),
        ordinal,
        kind,
        text: text.into(),
        locator,
        grid: None,
    }
}

fn document_locator(
    section_ordinal: u32,
    table_ordinal: Option<u32>,
    row_ordinal: Option<u32>,
    form_ordinal: Option<u32>,
) -> StructuredSourceLocator {
    StructuredSourceLocator::Document {
        section_ordinal,
        table_ordinal,
        row_ordinal,
        form_ordinal,
        heading_path: "Requirements".into(),
    }
}

fn image_bytes(format: ImageFormat) -> Vec<u8> {
    let image = DynamicImage::ImageRgba8(ImageBuffer::from_pixel(2, 3, Rgba([1, 2, 3, 255])));
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, format).unwrap();
    bytes.into_inner()
}

fn office_xlsm() -> Vec<u8> {
    spreadsheet(true)
}

fn ole_container(stream: &str) -> Vec<u8> {
    const MAGIC: &[u8] = b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1";
    let mut bytes = vec![0_u8; 512 * 3];
    bytes[..MAGIC.len()].copy_from_slice(MAGIC);
    bytes[28..30].copy_from_slice(&0xFFFE_u16.to_le_bytes());
    bytes[30..32].copy_from_slice(&9_u16.to_le_bytes());
    bytes[32..34].copy_from_slice(&6_u16.to_le_bytes());
    bytes[44..48].copy_from_slice(&1_u32.to_le_bytes());
    bytes[48..52].copy_from_slice(&1_u32.to_le_bytes());
    bytes[60..64].copy_from_slice(&0xFFFF_FFFE_u32.to_le_bytes());
    bytes[68..72].copy_from_slice(&0xFFFF_FFFE_u32.to_le_bytes());
    bytes[76..80].copy_from_slice(&0_u32.to_le_bytes());
    for index in 1..109 {
        let offset = 76 + index * 4;
        bytes[offset..offset + 4].copy_from_slice(&0xFFFF_FFFF_u32.to_le_bytes());
    }
    let fat = 512;
    bytes[fat..fat + 4].copy_from_slice(&0xFFFF_FFFD_u32.to_le_bytes());
    bytes[fat + 4..fat + 8].copy_from_slice(&0xFFFF_FFFE_u32.to_le_bytes());
    fn entry(bytes: &mut [u8], offset: usize, name: &str, kind: u8, child: u32) {
        let encoded: Vec<u8> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
        bytes[offset..offset + encoded.len()].copy_from_slice(&encoded);
        let name_len = u16::try_from(encoded.len() + 2).unwrap();
        bytes[offset + 64..offset + 66].copy_from_slice(&name_len.to_le_bytes());
        bytes[offset + 66] = kind;
        bytes[offset + 76..offset + 80].copy_from_slice(&child.to_le_bytes());
        bytes[offset + 116..offset + 120].copy_from_slice(&0xFFFF_FFFE_u32.to_le_bytes());
    }
    entry(&mut bytes, 1024, "Root Entry", 5, 1);
    entry(&mut bytes, 1152, stream, 2, u32::MAX);
    bytes
}

fn spreadsheet(macro_enabled: bool) -> Vec<u8> {
    const RELS_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
    const OFFICE_REL: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
    let main_type = if macro_enabled {
        "application/vnd.ms-excel.sheet.macroEnabled.main+xml"
    } else {
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"
    };
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let entries: Vec<(&str, String)> = vec![
        ("[Content_Types].xml", format!("<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/xl/workbook.xml\" ContentType=\"{main_type}\"/><Override PartName=\"/xl/worksheets/sheet1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/></Types>")),
        ("_rels/.rels", format!("<Relationships xmlns=\"{RELS_NS}\"><Relationship Id=\"rId1\" Type=\"{OFFICE_REL}\" Target=\"xl/workbook.xml\"/></Relationships>")),
        ("xl/workbook.xml", "<workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><sheets><sheet name=\"Sheet1\" sheetId=\"1\" r:id=\"rId1\"/></sheets></workbook>".into()),
        ("xl/_rels/workbook.xml.rels", format!("<Relationships xmlns=\"{RELS_NS}\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet1.xml\"/></Relationships>")),
        ("xl/worksheets/sheet1.xml", "<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData><row r=\"1\"><c r=\"A1\" t=\"inlineStr\"><is><t>Requirement</t></is></c></row></sheetData></worksheet>".into()),
    ];
    for (name, content) in entries {
        writer.start_file(name, options).unwrap();
        writer.write_all(content.as_bytes()).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn office(docx: bool) -> Vec<u8> {
    const RELS_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
    const OFFICE_REL: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let entries: Vec<(&str, String)> = if docx {
        vec![
            ("[Content_Types].xml", "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>".into()),
            ("_rels/.rels", format!("<Relationships xmlns=\"{RELS_NS}\"><Relationship Id=\"rId1\" Type=\"{OFFICE_REL}\" Target=\"word/document.xml\"/></Relationships>")),
            ("word/document.xml", "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p><w:r><w:t>Tender</w:t></w:r></w:p><w:sectPr/></w:body></w:document>".into()),
            ("word/_rels/document.xml.rels", format!("<Relationships xmlns=\"{RELS_NS}\"/>")),
        ]
    } else {
        vec![
            ("[Content_Types].xml", "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/><Override PartName=\"/xl/worksheets/sheet1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/></Types>".into()),
            ("_rels/.rels", format!("<Relationships xmlns=\"{RELS_NS}\"><Relationship Id=\"rId1\" Type=\"{OFFICE_REL}\" Target=\"xl/workbook.xml\"/></Relationships>")),
            ("xl/workbook.xml", "<workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><sheets><sheet name=\"Sheet1\" sheetId=\"1\" r:id=\"rId1\"/></sheets></workbook>".into()),
            ("xl/_rels/workbook.xml.rels", format!("<Relationships xmlns=\"{RELS_NS}\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet1.xml\"/></Relationships>")),
            ("xl/worksheets/sheet1.xml", "<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData><row r=\"1\"><c r=\"A1\" t=\"inlineStr\"><is><t>Requirement</t></is></c></row></sheetData></worksheet>".into()),
        ]
    };
    for (name, content) in entries {
        writer.start_file(name, options).unwrap();
        writer.write_all(content.as_bytes()).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn minimal_pdf() -> Vec<u8> {
    let objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>".as_slice(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".as_slice(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << >> >>".as_slice(),
    ];
    let mut result = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0];
    for (index, value) in objects.iter().enumerate() {
        offsets.push(result.len());
        result.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        result.extend_from_slice(value);
        result.extend_from_slice(b"\nendobj\n");
    }
    let xref = result.len();
    result.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets.into_iter().skip(1) {
        result.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    result.extend_from_slice(
        format!(
            "trailer << /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    result
}

fn deterministic_uuid(material: &[u8]) -> Uuid {
    let digest = Sha256::digest(material);
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}
