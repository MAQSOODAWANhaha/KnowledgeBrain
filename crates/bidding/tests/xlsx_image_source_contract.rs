//! Real synthetic XLSX parser output through the required-image producer.
//! OCR completion below is a recording test double, never a provider result.
use async_trait::async_trait;
use bidding::{
    analysis::DocumentRole,
    outline::{
        discover::{DiscoverWork, PackCarrier},
        frozen::{
            self, FrozenImageResult, PARSER_CONTRACT_VERSION, PrepareTenderInput,
            RawTenderDocument, TenderImageProcessor, TenderSourceParser,
        },
    },
};
use docparser::{ImageRef, ReadResult, SourceContract, StructuredSourceUnit};
use prost::Message;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::Mutex;
use tokio_util::sync::CancellationToken;

struct ActualParserFixture;
#[async_trait]
impl TenderSourceParser for ActualParserFixture {
    async fn parse(
        &self,
        _: &RawTenderDocument,
        _: &CancellationToken,
    ) -> Result<ReadResult, String> {
        ReadResult::try_from(
            docparser::proto::ReadResponse::decode(
                include_bytes!("fixtures/synthetic-xlsx-image-wire.pb").as_slice(),
            )
            .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
    }
}
struct RecordingImages {
    calls: Mutex<Vec<(String, String)>>,
    complete: bool,
}
#[async_trait]
impl TenderImageProcessor for RecordingImages {
    async fn process(
        &self,
        document_id: &str,
        contract: &SourceContract,
        unit: &StructuredSourceUnit,
        image: &ImageRef,
        _: &CancellationToken,
    ) -> Result<FrozenImageResult, String> {
        assert!(!image.data.is_empty());
        assert_eq!(image.mime_type, "image/png");
        self.calls
            .lock()
            .unwrap()
            .push((unit.key.clone(), hex::encode(Sha256::digest(&image.data))));
        Ok(FrozenImageResult {
            document_id: document_id.into(),
            document_revision: contract.document_revision.clone(),
            parser_version: contract.parser_version.clone(),
            unit_id: unit.key.clone(),
            literal_text: "SYNTHETIC IMAGE LABEL".into(),
            completeness: if self.complete { "complete" } else { "partial" }.into(),
            finish_reason: "stop".into(),
            done_seen: true,
            transport_complete: true,
            persisted_image_ref: format!("synthetic/{}", hex::encode(Sha256::digest(&image.data))),
            provenance: json!({"kind":"recording_test_double","original_ref":image.original_ref}),
        })
    }
}
fn request() -> PrepareTenderInput {
    PrepareTenderInput {
        project_id: "synthetic-xlsx-project".into(),
        document_set_id: "synthetic-xlsx-set".into(),
        documents: vec![RawTenderDocument {
            role: DocumentRole::Primary,
            document_id: "synthetic-xlsx".into(),
            file_name: "synthetic-xlsx-image.xlsx".into(),
            bytes: include_bytes!("fixtures/synthetic-xlsx-image.xlsx").to_vec(),
        }],
        document_relations: vec![],
        decisions: vec![],
        parser_contract_version: PARSER_CONTRACT_VERSION.into(),
    }
}
#[tokio::test]
async fn xlsx_embedded_image_enters_mandatory_ocr_and_vision_inventory() {
    let processor = RecordingImages {
        calls: Mutex::new(vec![]),
        complete: true,
    };
    let frozen = frozen::prepare_tender_input_with(
        request(),
        &ActualParserFixture,
        &processor,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    frozen::validate_frozen_input_contract(&frozen).unwrap();
    let calls = processor.calls.lock().unwrap();
    assert_eq!(
        calls.len(),
        1,
        "the embedded occurrence cannot disappear before OCR"
    );
    let images: Vec<_> = frozen
        .source_units
        .iter()
        .filter(|s| s.locator["kind"] == "image_region")
        .collect();
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].locator["vision_required"], true);
    assert_eq!(images[0].locator["image_available"], true);
    let mut work = DiscoverWork::plan_with_budget(&frozen, &|_| Ok(true));
    assert!(work.planning_error().is_none());
    assert_eq!(
        work.claim(256)
            .iter()
            .flat_map(|p| &p.atoms)
            .filter(|a| matches!(
                &a.carrier,
                PackCarrier::Image {
                    vision_required: true,
                    ..
                }
            ))
            .count(),
        1,
        "the original image remains mandatory in the Discover visual inventory"
    );
}
#[tokio::test]
async fn xlsx_incomplete_image_ocr_cannot_freeze_as_complete() {
    let processor = RecordingImages {
        calls: Mutex::new(vec![]),
        complete: false,
    };
    let error = frozen::prepare_tender_input_with(
        request(),
        &ActualParserFixture,
        &processor,
        &CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert!(error.contains("incomplete"), "{error}");
    assert_eq!(processor.calls.lock().unwrap().len(), 1);
}
