//! Convert: simple formats in-process; others via DocReader gRPC ReadStream.

mod anydoc;
mod asr;
mod convert;
mod engines;
mod grpc;
mod http_engine;
mod images;
mod output_inventory;
mod simple;
mod table_grid;
mod types;

pub use anydoc::{
    ENGINE as ANYDOC_ENGINE, supported_file_types as anydoc_file_types,
    supports as anydoc_supports, version as anydoc_version,
};
pub use asr::{ASR_NOT_CONFIGURED, AsrSettings, apply as apply_asr, apply_stub as apply_asr_stub};
pub use convert::{
    convert, convert_tender_source, convert_to_markdown, convert_with, convert_with_cancel,
    resolve_engine,
};
pub use engines::{EngineCatalog, EngineInfo, list_all_engines, local_engines, merge_engines};
pub use grpc::{ConvertRequest, DOCREADER_TIMEOUT, reader_addr, source_view};
pub use images::{rewrite_images, rewrite_inline};
pub use output_inventory::{
    OUTPUT_INVENTORY_PROFILE, OutputInventoryEntry, OutputInventoryManifest, OutputInventoryRead,
    OutputTableLayout, read_output_inventory, validate_output_inventory,
};
pub use simple::convert_simple;
pub use table_grid::validate_table_grid;
pub use types::{
    CompoundImageParent, ConvertError, ConvertInput, DocReaderReadError, ImageRef, NOT_CONFIGURED,
    PdfTableCell, PdfTableMergedRange, ReadResult, SpreadsheetCell, SpreadsheetRange,
    SpreadsheetTableIdentity, StructuredSourceLocator, StructuredSourceUnit,
    StructuredSourceUnitKind, TableGrid,
};

pub mod proto {
    tonic::include_proto!("docreader");
}
