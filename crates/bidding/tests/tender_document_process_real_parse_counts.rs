//! Real DocReader gRPC convert-count for F1 replay.
//!
//! Counts wrapper calls into `DocReaderGrpcTenderSourceConverter` (inner
//! `convert_tender_source`). Vision stays mocked; this file does not require a
//! real VLM. The explicit `docreader-contract-tests` target requires a running
//! service and two input paths. CI provisions these through
//! scripts/docreader_replay_acceptance.py; missing prerequisites still fail.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bidding::tender_process::*;
use bidding::tender_upload::{DOCX_MEDIA_TYPE, PDF_MEDIA_TYPE};
use docparser::ReadResult;
use platform::{BidAuthoringJobPayloadV2, BidAuthoringRequestIdentityV2};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const REQUIRE_ENV: &str = "KNOWLEDGEBRAIN_REQUIRE_DOCREADER_TESTS";

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
struct CountingDocReaderConverter {
    convert_count: Arc<AtomicUsize>,
}

impl CountingDocReaderConverter {
    fn new() -> Self {
        Self {
            convert_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn count(&self) -> usize {
        self.convert_count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl TenderSourceConverter for CountingDocReaderConverter {
    async fn convert(
        &self,
        file_name: &str,
        bytes: Vec<u8>,
        cancel: &CancellationToken,
    ) -> Result<ReadResult, TenderDocumentProcessError> {
        self.convert_count.fetch_add(1, Ordering::SeqCst);
        DocReaderGrpcTenderSourceConverter
            .convert(file_name, bytes, cancel)
            .await
    }
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
async fn unchanged_success_does_not_increment_real_grpc_convert_count() {
    require_docreader_tests();
    let bytes_a = testdata_bytes("KB_DOCREADER_TEST_DOCX");
    let bytes_b = testdata_bytes("KB_DOCREADER_TEST_PDF");
    assert_ne!(bytes_a, bytes_b, "A and B must be different input bytes");

    let converter = CountingDocReaderConverter::new();
    let (document_a, payload_a) = frozen_fixture("input-a.docx", DOCX_MEDIA_TYPE, bytes_a);
    let repository_a = MockRepository::new(document_a);
    let service_a = TenderDocumentProcessService::new(
        repository_a,
        converter.clone(),
        vision("verified OCR text"),
        MockTransport::default(),
    );

    let first = service_a
        .process(&payload_a, &CancellationToken::new())
        .await
        .expect("first real DocReader convert of unchanged A");
    assert!(!first.replayed, "first process of A must not be replayed");
    assert_eq!(converter.count(), 1, "first A process must convert once");

    let replay = service_a
        .process(&payload_a, &CancellationToken::new())
        .await
        .expect("replay of unchanged A");
    assert!(replay.replayed, "second process of unchanged A must replay");
    assert_eq!(
        first.converted_source_revision_id,
        replay.converted_source_revision_id
    );
    assert_eq!(
        converter.count(),
        1,
        "unchanged successful A must not convert again"
    );

    let (document_b, payload_b) = frozen_fixture("input-b.pdf", PDF_MEDIA_TYPE, bytes_b);
    let repository_b = MockRepository::new(document_b);
    let service_b = TenderDocumentProcessService::new(
        repository_b,
        converter.clone(),
        vision("verified OCR text"),
        MockTransport::default(),
    );
    let different = service_b
        .process(&payload_b, &CancellationToken::new())
        .await
        .expect("different bytes B must convert");
    assert!(
        !different.replayed,
        "different bytes B must not reuse A's receipt"
    );
    assert_eq!(
        converter.count(),
        2,
        "different bytes B must increment convert_count"
    );
}

fn require_docreader_tests() {
    if std::env::var(REQUIRE_ENV).as_deref() != Ok("1") {
        panic!("{REQUIRE_ENV}=1 is required; skip is not allowed");
    }
}

fn testdata_bytes(key: &str) -> Vec<u8> {
    let path = PathBuf::from(std::env::var(key).unwrap_or_else(|_| panic!("{key} is required")));
    std::fs::read(&path).unwrap_or_else(|error| panic!("read {key} at {path:?}: {error}"))
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

fn deterministic_uuid(material: &[u8]) -> Uuid {
    let digest = Sha256::digest(material);
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}
