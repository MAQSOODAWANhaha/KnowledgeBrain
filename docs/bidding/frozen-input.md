# Preparing a complete frozen tender

The internal producer is `prepare-tender-input`. It reads original source files, invokes the configured DocReader tender parser, completes required literal OCR, restores parser order, validates the SourceContract V2, and publishes one owned frozen snapshot. Its output is the canonical input digest accepted by the existing tender-processing job. HTTP upload and frontend wiring are outside this change.

```sh
cargo run -p bidding --bin prepare-tender-input -- tender-manifest.json
```

Configure the existing database, blob storage, DocReader and VLM settings for this process. Do not put credentials in the manifest. The project must already exist. Source paths are relative to the manifest file.

```json
{
  "project_id": "00000000-0000-0000-0000-000000000001",
  "document_set_id": "tender-2026-01",
  "parser_contract_version": "source-v2",
  "documents": [
    {"document_id": "main-tender", "file_path": "sources/tender.pdf"}
  ],
  "document_relations": [],
  "decisions": []
}
```

The default combined raw-file limit is 256 MiB. `KB_TENDER_PREPARE_MAX_BYTES` changes that explicit preparation budget. The command stages source and image objects before publication; the final database transaction acquires durable ownership of the complete snapshot and registers its digest. Failure abandons temporary staging references and never registers a partial frozen input. Physical cleanup follows the existing retention rules.

`outline::frozen::prepare_tender_input` is the equivalent internal service. `prepare_tender_input_with` exposes typed parser and image-processing ports for deterministic integration tests. Neither treats a caption as OCR, guesses missing image order, nor accepts a missing parser contract. Token-limited OCR triggers bounded spatial subdivision with stable region coordinates and exact overlap merging. Incomplete termination or ambiguous regional overlap blocks freezing. Original region text and physical-call counts remain in provenance. Textless nonblank images retain `ocr_complete=false` and require original-image vision. Original pixels are read with `read_source_view`, transported as bounded full-frame image content to the same configured authoring model, and only credited after a completed response confirms that exact request. Discover and Check have separate image-reading receipts. Configure `limits.vision_enabled` only when that same authoring model supports image input. A text-only model produces an explicit capability error rather than silently treating the image as a negative scan.

The frozen schema is version 2 and the supported parser contract is exactly `source-v2`. Source text and grids are verified against the parser's digests; OCR has separate provenance and a literal-text digest. Every required unit, image and physical page must be accounted for. Known partial inventory carriers block freezing rather than becoming successful empty evidence. New source bytes, parser version or OCR results produce a different canonical input digest.

Discovery uses ordered atoms, carrier-specific evidence and claimed pack revisions. Empty-text grids remain visible. Oversized cells are split at UTF-8 boundaries while preserving anchor identity. Repeated table headers are context with their original evidence identity. Stored image locators alone do not grant visual-reading evidence; actual transported image pixels require a completed-response receipt before an `ImageRegion` citation or whole-pack negative scan is accepted.

Native DOCX/XLSX contract fixtures contain the original OOXML bytes and the real producer responses. Regenerate them with `python services/docreader/scripts/generate_native_office_fixtures.py crates/docparser/tests/fixtures/python-native-office-v2.json`. This isolated local harness loads the unchanged native parser modules without importing optional MarkItDown/ONNX engines. Archive clocks are fixed so consecutive runs produce identical fixtures. The existing mixed-PDF fixture separately covers text, scanned and blank pages.
