//! gRPC DocReader client: ReadStream first, unary Read if Unimplemented.

use std::time::Duration;

use tokio::time::timeout;
use tonic::Code;
use tonic::metadata::MetadataValue;
use tonic::service::Interceptor;
use tonic::transport::{Channel, ClientTlsConfig};

use crate::engines::EngineInfo;
use crate::proto::doc_reader_client::DocReaderClient;
use crate::proto::{
    ImageRef as ProtoImage, ListEnginesRequest, ReadConfig, ReadRequest, ReadStreamResponse,
    StructuredSourceUnit as ProtoStructuredSourceUnit,
};
use crate::{
    CompoundImageParent, ConvertError, ImageRef, NOT_CONFIGURED, ReadResult, SpreadsheetCell,
    SpreadsheetRange, SpreadsheetTableIdentity, StructuredSourceLocator, StructuredSourceUnit,
    StructuredSourceUnitKind,
};

pub const DOCREADER_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// After the meta frame, give up waiting for the next image / EOS.
pub const FRAME_IDLE: Duration = Duration::from_secs(120);

pub fn reader_addr() -> Option<String> {
    let v = std::env::var("DOCREADER_ADDR").unwrap_or_default();
    let t = v.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

pub fn max_message_size() -> usize {
    std::env::var("MAX_FILE_SIZE_MB")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(50)
        * 1024
        * 1024
}

pub fn endpoint_url(addr: &str) -> String {
    if addr.starts_with("http://") || addr.starts_with("https://") {
        addr.to_string()
    } else if tls_enabled() {
        format!("https://{addr}")
    } else {
        format!("http://{addr}")
    }
}

fn tls_enabled() -> bool {
    let v = std::env::var("GRPC_TLS_ENABLED").unwrap_or_default();
    matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

fn auth_token() -> Option<MetadataValue<tonic::metadata::Ascii>> {
    let raw = std::env::var("GRPC_AUTH_TOKEN").unwrap_or_default();
    let t = raw.trim();
    if t.is_empty() {
        return None;
    }
    format!("Bearer {t}").parse().ok()
}

#[derive(Clone)]
struct AuthInterceptor {
    token: Option<MetadataValue<tonic::metadata::Ascii>>,
}

impl Interceptor for AuthInterceptor {
    fn call(&mut self, mut req: tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status> {
        if let Some(t) = &self.token {
            req.metadata_mut().insert("authorization", t.clone());
        }
        Ok(req)
    }
}

type Client =
    DocReaderClient<tonic::service::interceptor::InterceptedService<Channel, AuthInterceptor>>;

pub struct ConvertRequest {
    pub file_content: Vec<u8>,
    pub file_name: String,
    pub file_type: String,
    pub url: String,
    pub title: String,
    pub parser_engine: String,
    pub parser_engine_overrides: std::collections::HashMap<String, String>,
}

pub async fn list_engines(
    overrides: &std::collections::HashMap<String, String>,
) -> Result<Vec<EngineInfo>, ConvertError> {
    let Some(addr) = reader_addr() else {
        return Err(ConvertError(NOT_CONFIGURED.into()));
    };
    let fut = list_engines_inner(&addr, overrides);
    match timeout(Duration::from_secs(10), fut).await {
        Ok(r) => r,
        Err(_) => Err(ConvertError("docreader ListEngines timeout".into())),
    }
}

async fn list_engines_inner(
    addr: &str,
    overrides: &std::collections::HashMap<String, String>,
) -> Result<Vec<EngineInfo>, ConvertError> {
    let mut client = connect(addr).await?;
    let resp = client
        .list_engines(ListEnginesRequest {
            config_overrides: overrides.clone(),
        })
        .await
        .map_err(|e| ConvertError(format!("gRPC ListEngines failed: {e}")))?
        .into_inner();
    Ok(resp
        .engines
        .into_iter()
        .map(|e| EngineInfo {
            name: e.name,
            description: e.description,
            file_types: e.file_types,
            available: e.available,
            unavailable_reason: e.unavailable_reason,
        })
        .collect())
}

pub async fn read(req: ConvertRequest) -> Result<ReadResult, ConvertError> {
    let Some(addr) = reader_addr() else {
        return Ok(ReadResult {
            error: NOT_CONFIGURED.into(),
            ..ReadResult::default()
        });
    };
    let fut = read_inner(&addr, req);
    match timeout(DOCREADER_TIMEOUT, fut).await {
        Ok(r) => r,
        Err(_) => Err(ConvertError(format!(
            "docreader call timeout after {:?}",
            DOCREADER_TIMEOUT
        ))),
    }
}

async fn connect(addr: &str) -> Result<Client, ConvertError> {
    let url = endpoint_url(addr);
    let mut endpoint =
        Channel::from_shared(url.clone()).map_err(|e| ConvertError(e.to_string()))?;
    if url.starts_with("https://") || tls_enabled() {
        let tls = ClientTlsConfig::new().with_native_roots();
        endpoint = endpoint
            .tls_config(tls)
            .map_err(|e| ConvertError(e.to_string()))?;
    }
    let channel = endpoint
        .connect()
        .await
        .map_err(|e| ConvertError(format!("failed to connect to docreader: {e}")))?;
    let max = max_message_size();
    Ok(DocReaderClient::with_interceptor(
        channel,
        AuthInterceptor {
            token: auth_token(),
        },
    )
    .max_decoding_message_size(max)
    .max_encoding_message_size(max))
}

fn to_proto(req: ConvertRequest, request_id: String) -> ReadRequest {
    ReadRequest {
        file_content: req.file_content,
        file_name: req.file_name,
        file_type: req.file_type,
        url: req.url,
        title: req.title,
        request_id,
        config: Some(ReadConfig {
            parser_engine: req.parser_engine,
            parser_engine_overrides: req.parser_engine_overrides,
        }),
    }
}

async fn read_inner(addr: &str, req: ConvertRequest) -> Result<ReadResult, ConvertError> {
    let mut client = connect(addr).await?;
    let proto_req = to_proto(req, uuid::Uuid::new_v4().to_string());
    match read_stream(&mut client, proto_req.clone()).await {
        Ok(r) => Ok(r),
        Err(e) if e.msg.contains("unimplemented") || e.code == Some(Code::Unimplemented) => {
            read_unary(&mut client, proto_req).await
        }
        Err(e) => Err(ConvertError(e.msg)),
    }
}

struct StreamErr {
    code: Option<Code>,
    msg: String,
}

async fn read_stream(client: &mut Client, req: ReadRequest) -> Result<ReadResult, StreamErr> {
    let mut stream = client
        .read_stream(req)
        .await
        .map_err(map_status)?
        .into_inner();
    let mut result = ReadResult::default();
    let mut got_meta = false;
    let mut expected_images: Option<usize> = None;
    loop {
        if got_meta && expected_images.is_some_and(|n| result.images.len() >= n) {
            break;
        }
        let next = if got_meta {
            match timeout(FRAME_IDLE, stream.message()).await {
                Ok(r) => r,
                Err(_) => break,
            }
        } else {
            stream.message().await
        };
        let Some(frame) = next.map_err(map_status)? else {
            break;
        };
        apply_frame(&mut result, &mut got_meta, &mut expected_images, frame)?;
    }
    if !got_meta {
        return Err(StreamErr {
            code: None,
            msg: "gRPC ReadStream returned no metadata frame".into(),
        });
    }
    if let Some(n) = expected_images
        && result.images.len() < n
    {
        return Err(StreamErr {
            code: None,
            msg: format!(
                "gRPC ReadStream incomplete: got {} of {n} images",
                result.images.len()
            ),
        });
    }
    Ok(result)
}

async fn read_unary(client: &mut Client, req: ReadRequest) -> Result<ReadResult, ConvertError> {
    let resp = client
        .read(req)
        .await
        .map_err(|e| ConvertError(format!("gRPC Read failed: {e}")))?
        .into_inner();
    Ok(ReadResult {
        markdown: resp.markdown_content,
        error: resp.error,
        images: resp.image_refs.into_iter().map(from_proto_image).collect(),
        metadata: resp.metadata,
        structured_source_units: from_proto_units(resp.structured_source_units)
            .map_err(ConvertError)?,
        ..ReadResult::default()
    })
}

fn apply_frame(
    result: &mut ReadResult,
    got_meta: &mut bool,
    expected_images: &mut Option<usize>,
    frame: ReadStreamResponse,
) -> Result<(), StreamErr> {
    match frame.payload {
        Some(crate::proto::read_stream_response::Payload::Meta(meta)) => {
            *got_meta = true;
            *expected_images = (meta.image_count > 0).then_some(meta.image_count as usize);
            result.markdown = meta.markdown_content;
            result.error = meta.error;
            result.metadata.extend(meta.metadata);
            result.structured_source_units = from_proto_units(meta.structured_source_units)
                .map_err(|msg| StreamErr { code: None, msg })?;
            if meta.image_count > 0 {
                result.images.reserve(meta.image_count as usize);
            }
        }
        Some(crate::proto::read_stream_response::Payload::Image(img)) => {
            result.images.push(from_proto_image(img));
        }
        None => {}
    }
    Ok(())
}

fn from_proto_units(
    units: Vec<ProtoStructuredSourceUnit>,
) -> Result<Vec<StructuredSourceUnit>, String> {
    const MAX_STRUCTURED_UNITS: usize = 200_000;
    if units.len() > MAX_STRUCTURED_UNITS {
        return Err("DocReader returned too many structured source units".into());
    }
    let mut keys = std::collections::HashSet::with_capacity(units.len());
    let mut mapped = Vec::with_capacity(units.len());
    for (expected_ordinal, unit) in units.into_iter().enumerate() {
        if unit.ordinal != expected_ordinal as u32 {
            return Err("DocReader returned non-contiguous structured source unit ordinals".into());
        }
        if !keys.insert(unit.key.clone()) {
            return Err("DocReader returned duplicate structured source unit key".into());
        }
        mapped.push(from_proto_unit(unit)?);
    }
    validate_compound_image_parents(&mapped)?;
    Ok(mapped)
}

fn from_proto_unit(unit: ProtoStructuredSourceUnit) -> Result<StructuredSourceUnit, String> {
    use crate::proto::StructuredSourceUnitKind as ProtoKind;
    use crate::proto::structured_source_unit::Locator;

    let kind = match ProtoKind::try_from(unit.kind) {
        Ok(ProtoKind::Section) => StructuredSourceUnitKind::Section,
        Ok(ProtoKind::TableRow) => StructuredSourceUnitKind::TableRow,
        Ok(ProtoKind::TableRegion) => StructuredSourceUnitKind::TableRegion,
        Ok(ProtoKind::FormRegion) => StructuredSourceUnitKind::FormRegion,
        Ok(ProtoKind::AttachmentRegion) => StructuredSourceUnitKind::AttachmentRegion,
        Ok(ProtoKind::ImageRegion) => StructuredSourceUnitKind::ImageRegion,
        _ => return Err("DocReader returned unspecified structured source unit kind".into()),
    };
    if unit.key.is_empty() {
        return Err("DocReader returned structured source unit without key".into());
    }
    let locator = match unit.locator.ok_or_else(|| {
        "DocReader returned structured source unit without typed locator".to_string()
    })? {
        Locator::Document(value) => StructuredSourceLocator::Document {
            section_ordinal: value.section_ordinal,
            table_ordinal: value.table_ordinal,
            row_ordinal: value.row_ordinal,
            form_ordinal: value.form_ordinal,
            heading_path: value.heading_path,
        },
        Locator::Page(value) => {
            validate_bounds(value.left, value.top, value.right, value.bottom, "page")?;
            StructuredSourceLocator::Page {
                page_ordinal: value.page_ordinal,
                left: value.left,
                top: value.top,
                right: value.right,
                bottom: value.bottom,
            }
        }
        Locator::Spreadsheet(value) => {
            if value.sheet_name.is_empty() {
                return Err("DocReader returned spreadsheet locator without sheet name".into());
            }
            let region = from_proto_range(value.region.ok_or_else(|| {
                "DocReader returned spreadsheet locator without region".to_string()
            })?)?;
            let mut cell_addresses = std::collections::HashSet::new();
            let mut cells = Vec::with_capacity(value.cells.len());
            let mut previous_cell: Option<(u32, u32, String)> = None;
            for cell in value.cells {
                let parsed_address = parse_a1_cell(&cell.address);
                let order = (cell.row, cell.column, cell.address.clone());
                if cell.address.is_empty()
                    || cell.row == 0
                    || cell.column == 0
                    || parsed_address != Some((cell.row, cell.column))
                    || cell.row < region.start_row
                    || cell.row > region.end_row
                    || cell.column < region.start_column
                    || cell.column > region.end_column
                    || previous_cell
                        .as_ref()
                        .is_some_and(|previous| previous >= &order)
                    || !cell_addresses.insert(cell.address.clone())
                {
                    return Err("DocReader returned invalid spreadsheet cell".into());
                }
                previous_cell = Some(order);
                cells.push(SpreadsheetCell {
                    address: cell.address,
                    row: cell.row,
                    column: cell.column,
                    text: cell.text,
                });
            }
            let mut merged_ranges = Vec::with_capacity(value.merged_ranges.len());
            for range in value.merged_ranges {
                let range = from_proto_range(range)?;
                if range.start_row < region.start_row
                    || range.end_row > region.end_row
                    || range.start_column < region.start_column
                    || range.end_column > region.end_column
                {
                    return Err("DocReader returned merged range outside sheet region".into());
                }
                merged_ranges.push(range);
            }
            if !merged_ranges
                .windows(2)
                .all(|pair| range_order(&pair[0]) <= range_order(&pair[1]))
            {
                return Err("DocReader returned non-deterministic merged range order".into());
            }
            let mut defined_tables = Vec::with_capacity(value.defined_tables.len());
            let mut previous_table = None;
            for table in value.defined_tables {
                let table_range = parse_a1_range(&table.a1_range);
                if table.name.is_empty()
                    || table.display_name.is_empty()
                    || table.a1_range.is_empty()
                    || table_range.is_none()
                    || table_range.is_some_and(|table_range| !range_contains(&region, &table_range))
                {
                    return Err("DocReader returned incomplete spreadsheet table identity".into());
                }
                if previous_table
                    .as_deref()
                    .is_some_and(|name| name >= table.name.as_str())
                {
                    return Err(
                        "DocReader returned non-deterministic spreadsheet table order".into(),
                    );
                }
                previous_table = Some(table.name.clone());
                defined_tables.push(SpreadsheetTableIdentity {
                    name: table.name,
                    display_name: table.display_name,
                    a1_range: table.a1_range,
                });
            }
            StructuredSourceLocator::Spreadsheet {
                sheet_ordinal: value.sheet_ordinal,
                sheet_name: value.sheet_name,
                region,
                cells,
                merged_ranges,
                defined_tables,
            }
        }
        Locator::Image(value) => {
            use crate::proto::image_locator::CompoundParent;

            let missing_parent_field =
                || "DocReader returned compound image parent without required ordinals".to_string();
            let compound_parent = match value.compound_parent {
                Some(CompoundParent::ParagraphParent(parent)) => {
                    Some(CompoundImageParent::Paragraph {
                        section_ordinal: parent.section_ordinal.ok_or_else(missing_parent_field)?,
                        paragraph_ordinal: parent
                            .paragraph_ordinal
                            .ok_or_else(missing_parent_field)?,
                    })
                }
                Some(CompoundParent::TableCellParent(parent)) => {
                    Some(CompoundImageParent::TableCell {
                        section_ordinal: parent.section_ordinal.ok_or_else(missing_parent_field)?,
                        table_ordinal: parent.table_ordinal.ok_or_else(missing_parent_field)?,
                        row_ordinal: parent.row_ordinal.ok_or_else(missing_parent_field)?,
                        cell_ordinal: parent.cell_ordinal.ok_or_else(missing_parent_field)?,
                    })
                }
                Some(CompoundParent::FormParent(parent)) => Some(CompoundImageParent::Form {
                    section_ordinal: parent.section_ordinal.ok_or_else(missing_parent_field)?,
                    form_ordinal: parent.form_ordinal.ok_or_else(missing_parent_field)?,
                }),
                None => None,
            };
            if value.width == 0
                || value.height == 0
                || value.original_ref.is_empty()
                || value.media_type.is_empty()
                || (value.page_ordinal.is_some() && compound_parent.is_some())
            {
                return Err("DocReader returned incomplete image locator".into());
            }
            validate_bounds(value.left, value.top, value.right, value.bottom, "image")?;
            if value.left.is_some() && value.page_ordinal.is_none() {
                return Err("DocReader returned bounded image without page identity".into());
            }
            if compound_parent.is_some() && value.left.is_some() {
                return Err("DocReader returned compound image with page bounds".into());
            }
            StructuredSourceLocator::Image {
                original_ref: value.original_ref,
                width: value.width,
                height: value.height,
                media_type: value.media_type,
                page_ordinal: value.page_ordinal,
                compound_parent,
                left: value.left,
                top: value.top,
                right: value.right,
                bottom: value.bottom,
            }
        }
        Locator::Attachment(value) => {
            if value.part_name.is_empty() || value.relationship_type.is_empty() {
                return Err("DocReader returned incomplete attachment locator".into());
            }
            StructuredSourceLocator::Attachment {
                part_name: value.part_name,
                relationship_type: value.relationship_type,
            }
        }
    };
    let compatible = matches!(
        (&kind, &locator),
        (
            StructuredSourceUnitKind::Section,
            StructuredSourceLocator::Document {
                table_ordinal: None,
                row_ordinal: None,
                form_ordinal: None,
                ..
            }
        ) | (
            StructuredSourceUnitKind::Section,
            StructuredSourceLocator::Page { .. }
        ) | (
            StructuredSourceUnitKind::Section,
            StructuredSourceLocator::Spreadsheet { .. }
        ) | (
            StructuredSourceUnitKind::TableRegion,
            StructuredSourceLocator::Document {
                table_ordinal: Some(_),
                row_ordinal: None,
                form_ordinal: None,
                ..
            }
        ) | (
            StructuredSourceUnitKind::TableRegion,
            StructuredSourceLocator::Page { .. }
        ) | (
            StructuredSourceUnitKind::TableRegion,
            StructuredSourceLocator::Spreadsheet { .. }
        ) | (
            StructuredSourceUnitKind::TableRow,
            StructuredSourceLocator::Document {
                table_ordinal: Some(_),
                row_ordinal: Some(_),
                form_ordinal: None,
                ..
            }
        ) | (
            StructuredSourceUnitKind::TableRow,
            StructuredSourceLocator::Page { .. }
        ) | (
            StructuredSourceUnitKind::TableRow,
            StructuredSourceLocator::Spreadsheet { .. }
        ) | (
            StructuredSourceUnitKind::FormRegion,
            StructuredSourceLocator::Document {
                table_ordinal: None,
                row_ordinal: None,
                form_ordinal: Some(_),
                ..
            }
        ) | (
            StructuredSourceUnitKind::FormRegion,
            StructuredSourceLocator::Page { .. }
        ) | (
            StructuredSourceUnitKind::FormRegion,
            StructuredSourceLocator::Spreadsheet { .. }
        ) | (
            StructuredSourceUnitKind::AttachmentRegion,
            StructuredSourceLocator::Attachment { .. }
        ) | (
            StructuredSourceUnitKind::ImageRegion,
            StructuredSourceLocator::Image { .. }
        )
    );
    if !compatible {
        return Err("DocReader returned incompatible structured source kind and locator".into());
    }
    Ok(StructuredSourceUnit {
        key: unit.key,
        ordinal: unit.ordinal,
        kind,
        text: unit.text,
        locator,
    })
}

fn validate_compound_image_parents(units: &[StructuredSourceUnit]) -> Result<(), String> {
    let mut sections = std::collections::HashSet::new();
    let mut table_rows = std::collections::HashSet::new();
    let mut forms = std::collections::HashSet::new();
    for unit in units {
        match (&unit.kind, &unit.locator) {
            (
                StructuredSourceUnitKind::Section,
                StructuredSourceLocator::Document {
                    section_ordinal, ..
                },
            ) => {
                sections.insert(*section_ordinal);
            }
            (
                StructuredSourceUnitKind::TableRow,
                StructuredSourceLocator::Document {
                    section_ordinal,
                    table_ordinal: Some(table_ordinal),
                    row_ordinal: Some(row_ordinal),
                    ..
                },
            ) => {
                table_rows.insert((*section_ordinal, *table_ordinal, *row_ordinal));
            }
            (
                StructuredSourceUnitKind::FormRegion,
                StructuredSourceLocator::Document {
                    section_ordinal,
                    form_ordinal: Some(form_ordinal),
                    ..
                },
            ) => {
                forms.insert((*section_ordinal, *form_ordinal));
            }
            _ => {}
        }
    }
    for unit in units {
        let StructuredSourceLocator::Image {
            compound_parent: Some(parent),
            ..
        } = &unit.locator
        else {
            continue;
        };
        let valid = match parent {
            CompoundImageParent::Paragraph {
                section_ordinal, ..
            } => sections.contains(section_ordinal),
            CompoundImageParent::TableCell {
                section_ordinal,
                table_ordinal,
                row_ordinal,
                ..
            } => table_rows.contains(&(*section_ordinal, *table_ordinal, *row_ordinal)),
            CompoundImageParent::Form {
                section_ordinal,
                form_ordinal,
            } => forms.contains(&(*section_ordinal, *form_ordinal)),
        };
        if !valid {
            return Err("DocReader returned image with unknown compound parent".into());
        }
    }
    Ok(())
}

fn validate_bounds(
    left: Option<f64>,
    top: Option<f64>,
    right: Option<f64>,
    bottom: Option<f64>,
    name: &str,
) -> Result<(), String> {
    match (left, top, right, bottom) {
        (None, None, None, None) => Ok(()),
        (Some(left), Some(top), Some(right), Some(bottom))
            if left.is_finite()
                && top.is_finite()
                && right.is_finite()
                && bottom.is_finite()
                && left < right
                && top < bottom =>
        {
            Ok(())
        }
        _ => Err(format!("DocReader returned invalid {name} bounds")),
    }
}

fn range_order(range: &SpreadsheetRange) -> (u32, u32, u32, u32, &str) {
    (
        range.start_row,
        range.start_column,
        range.end_row,
        range.end_column,
        range.a1_range.as_str(),
    )
}

fn parse_a1_cell(value: &str) -> Option<(u32, u32)> {
    let value = value.trim().replace('$', "").to_ascii_uppercase();
    let split = value.find(|character: char| character.is_ascii_digit())?;
    if split == 0 || split == value.len() {
        return None;
    }
    let (letters, digits) = value.split_at(split);
    if !letters
        .chars()
        .all(|character| character.is_ascii_uppercase())
        || !digits.chars().all(|character| character.is_ascii_digit())
    {
        return None;
    }
    let mut column = 0_u32;
    for character in letters.bytes() {
        column = column
            .checked_mul(26)?
            .checked_add(u32::from(character - b'A' + 1))?;
    }
    let row = digits.parse::<u32>().ok()?;
    (row > 0 && column > 0).then_some((row, column))
}

fn parse_a1_range(value: &str) -> Option<SpreadsheetRange> {
    let mut parts = value.split(':');
    let start = parse_a1_cell(parts.next()?)?;
    let end = parse_a1_cell(parts.next().unwrap_or(value))?;
    if parts.next().is_some() || end.0 < start.0 || end.1 < start.1 {
        return None;
    }
    Some(SpreadsheetRange {
        a1_range: value.to_string(),
        start_row: start.0,
        start_column: start.1,
        end_row: end.0,
        end_column: end.1,
    })
}

fn range_contains(outer: &SpreadsheetRange, inner: &SpreadsheetRange) -> bool {
    outer.start_row <= inner.start_row
        && inner.end_row <= outer.end_row
        && outer.start_column <= inner.start_column
        && inner.end_column <= outer.end_column
}

fn from_proto_range(value: crate::proto::SpreadsheetRange) -> Result<SpreadsheetRange, String> {
    let parsed = parse_a1_range(&value.a1_range);
    if value.a1_range.is_empty()
        || value.start_row == 0
        || value.start_column == 0
        || value.end_row < value.start_row
        || value.end_column < value.start_column
        || parsed.as_ref().is_none_or(|parsed| {
            parsed.start_row != value.start_row
                || parsed.start_column != value.start_column
                || parsed.end_row != value.end_row
                || parsed.end_column != value.end_column
        })
    {
        return Err("DocReader returned invalid spreadsheet range".into());
    }
    Ok(SpreadsheetRange {
        a1_range: value.a1_range,
        start_row: value.start_row,
        start_column: value.start_column,
        end_row: value.end_row,
        end_column: value.end_column,
    })
}

fn from_proto_image(img: ProtoImage) -> ImageRef {
    ImageRef {
        filename: img.filename,
        original_ref: img.original_ref,
        mime_type: img.mime_type,
        storage_key: img.storage_key,
        data: img.image_data,
    }
}

fn map_status(s: tonic::Status) -> StreamErr {
    StreamErr {
        code: Some(s.code()),
        msg: format!("gRPC ReadStream failed: {s}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proto_forwards_parser_engine_overrides() {
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("mineru_token".into(), "t".into());
        let req = ConvertRequest {
            file_content: vec![],
            file_name: "a.pdf".into(),
            file_type: "pdf".into(),
            url: String::new(),
            title: String::new(),
            parser_engine: "builtin".into(),
            parser_engine_overrides: overrides,
        };
        let proto = to_proto(req, "rid".into());
        let cfg = proto.config.expect("config");
        assert_eq!(cfg.parser_engine, "builtin");
        assert_eq!(
            cfg.parser_engine_overrides.get("mineru_token").unwrap(),
            "t"
        );
    }

    #[test]
    fn client_maps_typed_structured_units() {
        let unit = crate::proto::StructuredSourceUnit {
            key: "image:0".into(),
            ordinal: 0,
            kind: crate::proto::StructuredSourceUnitKind::ImageRegion as i32,
            text: String::new(),
            locator: Some(crate::proto::structured_source_unit::Locator::Image(
                crate::proto::ImageLocator {
                    original_ref: "images/proof.png".into(),
                    width: 4,
                    height: 3,
                    media_type: "image/png".into(),
                    page_ordinal: None,
                    ..Default::default()
                },
            )),
        };
        let mapped = from_proto_unit(unit).unwrap();
        assert_eq!(mapped.key, "image:0");
        assert_eq!(
            mapped.locator,
            StructuredSourceLocator::Image {
                original_ref: "images/proof.png".into(),
                width: 4,
                height: 3,
                media_type: "image/png".into(),
                page_ordinal: None,
                compound_parent: None,
                left: None,
                top: None,
                right: None,
                bottom: None,
            }
        );
    }

    #[test]
    fn structured_contract_rejects_unspecified_kind_or_missing_locator() {
        let missing = ProtoStructuredSourceUnit {
            key: "bad".into(),
            ordinal: 0,
            kind: crate::proto::StructuredSourceUnitKind::Unspecified as i32,
            text: String::new(),
            locator: None,
        };
        assert!(from_proto_unit(missing).is_err());
        let missing_locator = ProtoStructuredSourceUnit {
            key: "bad".into(),
            ordinal: 0,
            kind: crate::proto::StructuredSourceUnitKind::Section as i32,
            text: String::new(),
            locator: None,
        };
        assert!(from_proto_unit(missing_locator).is_err());
    }

    #[test]
    fn structured_collection_rejects_duplicate_keys_and_non_contiguous_ordinals() {
        fn section(key: &str, ordinal: u32) -> ProtoStructuredSourceUnit {
            ProtoStructuredSourceUnit {
                key: key.into(),
                ordinal,
                kind: crate::proto::StructuredSourceUnitKind::Section as i32,
                text: String::new(),
                locator: Some(crate::proto::structured_source_unit::Locator::Document(
                    crate::proto::DocumentLocator {
                        section_ordinal: ordinal,
                        ..Default::default()
                    },
                )),
            }
        }
        assert!(from_proto_units(vec![section("same", 0), section("same", 1)]).is_err());
        assert!(from_proto_units(vec![section("zero", 0), section("two", 2)]).is_err());
    }

    #[test]
    fn structured_contract_rejects_incompatible_locator_and_invalid_ranges() {
        let incompatible = ProtoStructuredSourceUnit {
            key: "bad-kind".into(),
            ordinal: 0,
            kind: crate::proto::StructuredSourceUnitKind::Section as i32,
            text: String::new(),
            locator: Some(crate::proto::structured_source_unit::Locator::Image(
                crate::proto::ImageLocator {
                    original_ref: "images/a.png".into(),
                    width: 1,
                    height: 1,
                    media_type: "image/png".into(),
                    ..Default::default()
                },
            )),
        };
        assert!(from_proto_unit(incompatible).is_err());

        let invalid_range = ProtoStructuredSourceUnit {
            key: "bad-range".into(),
            ordinal: 0,
            kind: crate::proto::StructuredSourceUnitKind::Section as i32,
            text: String::new(),
            locator: Some(crate::proto::structured_source_unit::Locator::Spreadsheet(
                crate::proto::SpreadsheetLocator {
                    sheet_ordinal: 0,
                    sheet_name: "Sheet1".into(),
                    region: Some(crate::proto::SpreadsheetRange {
                        a1_range: "A1:A0".into(),
                        start_row: 1,
                        start_column: 1,
                        end_row: 0,
                        end_column: 1,
                    }),
                    ..Default::default()
                },
            )),
        };
        assert!(from_proto_unit(invalid_range).is_err());
    }

    #[test]
    fn compound_image_parent_is_closed_and_must_resolve() {
        fn section() -> ProtoStructuredSourceUnit {
            ProtoStructuredSourceUnit {
                key: "section:0".into(),
                ordinal: 0,
                kind: crate::proto::StructuredSourceUnitKind::Section as i32,
                text: "Owner".into(),
                locator: Some(crate::proto::structured_source_unit::Locator::Document(
                    crate::proto::DocumentLocator {
                        section_ordinal: 0,
                        ..Default::default()
                    },
                )),
            }
        }
        fn paragraph_image(section_ordinal: u32) -> ProtoStructuredSourceUnit {
            use prost::Message;

            let locator = crate::proto::ImageLocator {
                original_ref: "images/proof.png".into(),
                width: 4,
                height: 3,
                media_type: "image/png".into(),
                ..Default::default()
            };
            let parent = crate::proto::ParagraphImageParent {
                section_ordinal: Some(section_ordinal),
                paragraph_ordinal: Some(2),
            }
            .encode_to_vec();
            let mut encoded = locator.encode_to_vec();
            encoded.push(0x62); // field 12, length-delimited paragraph_parent
            encoded.push(u8::try_from(parent.len()).expect("small parent fixture"));
            encoded.extend(parent);
            let locator = crate::proto::ImageLocator::decode(encoded.as_slice())
                .expect("decode paragraph parent fixture");
            ProtoStructuredSourceUnit {
                key: "image:0".into(),
                ordinal: 1,
                kind: crate::proto::StructuredSourceUnitKind::ImageRegion as i32,
                text: String::new(),
                locator: Some(crate::proto::structured_source_unit::Locator::Image(
                    locator,
                )),
            }
        }
        let valid = from_proto_units(vec![section(), paragraph_image(0)]).unwrap();
        assert!(matches!(
            valid[1].locator,
            StructuredSourceLocator::Image {
                compound_parent: Some(CompoundImageParent::Paragraph { .. }),
                ..
            }
        ));
        assert!(from_proto_units(vec![section(), paragraph_image(9)]).is_err());

        let mut conflicting = paragraph_image(0);
        if let Some(crate::proto::structured_source_unit::Locator::Image(locator)) =
            conflicting.locator.as_mut()
        {
            locator.page_ordinal = Some(0);
        }
        assert!(from_proto_unit(conflicting).is_err());

        use prost::Message;
        let base = crate::proto::ImageLocator {
            original_ref: "images/missing.png".into(),
            width: 1,
            height: 1,
            media_type: "image/png".into(),
            ..Default::default()
        };
        let mut encoded = base.encode_to_vec();
        encoded.extend_from_slice(&[0x62, 0x00]);
        let missing_parent = ProtoStructuredSourceUnit {
            key: "image:missing-parent".into(),
            ordinal: 0,
            kind: crate::proto::StructuredSourceUnitKind::ImageRegion as i32,
            text: String::new(),
            locator: Some(crate::proto::structured_source_unit::Locator::Image(
                crate::proto::ImageLocator::decode(encoded.as_slice()).unwrap(),
            )),
        };
        assert!(from_proto_unit(missing_parent).is_err());
    }

    #[test]
    fn spreadsheet_decoder_accepts_only_contained_sorted_geometry() {
        fn range(a1: &str, sr: u32, sc: u32, er: u32, ec: u32) -> crate::proto::SpreadsheetRange {
            crate::proto::SpreadsheetRange {
                a1_range: a1.into(),
                start_row: sr,
                start_column: sc,
                end_row: er,
                end_column: ec,
            }
        }
        let sheet = ProtoStructuredSourceUnit {
            key: "sheet:0".into(),
            ordinal: 0,
            kind: crate::proto::StructuredSourceUnitKind::Section as i32,
            text: "Merges".into(),
            locator: Some(crate::proto::structured_source_unit::Locator::Spreadsheet(
                crate::proto::SpreadsheetLocator {
                    sheet_ordinal: 0,
                    sheet_name: "Merges".into(),
                    region: Some(range("A1:F4", 1, 1, 4, 6)),
                    merged_ranges: vec![range("A1:A2", 1, 1, 2, 1), range("D4:E4", 4, 4, 4, 5)],
                    ..Default::default()
                },
            )),
        };
        let row = ProtoStructuredSourceUnit {
            key: "sheet:0:row:4".into(),
            ordinal: 1,
            kind: crate::proto::StructuredSourceUnitKind::TableRow as i32,
            text: "row | tail".into(),
            locator: Some(crate::proto::structured_source_unit::Locator::Spreadsheet(
                crate::proto::SpreadsheetLocator {
                    sheet_ordinal: 0,
                    sheet_name: "Merges".into(),
                    region: Some(range("D4:F4", 4, 4, 4, 6)),
                    cells: vec![
                        crate::proto::SpreadsheetCell {
                            address: "D4".into(),
                            row: 4,
                            column: 4,
                            text: "row".into(),
                        },
                        crate::proto::SpreadsheetCell {
                            address: "F4".into(),
                            row: 4,
                            column: 6,
                            text: "tail".into(),
                        },
                    ],
                    merged_ranges: vec![range("D4:E4", 4, 4, 4, 5)],
                    ..Default::default()
                },
            )),
        };
        assert!(from_proto_units(vec![sheet.clone(), row.clone()]).is_ok());

        let mut outside = row;
        if let Some(crate::proto::structured_source_unit::Locator::Spreadsheet(locator)) =
            outside.locator.as_mut()
        {
            locator.merged_ranges = vec![range("A1:A2", 1, 1, 2, 1)];
        }
        assert!(from_proto_units(vec![sheet, outside]).is_err());
    }

    #[test]
    fn endpoint_adds_scheme_and_tls() {
        unsafe { std::env::remove_var("GRPC_TLS_ENABLED") };
        assert_eq!(endpoint_url("127.0.0.1:50051"), "http://127.0.0.1:50051");
        assert_eq!(
            endpoint_url("https://reader.example"),
            "https://reader.example"
        );
        unsafe { std::env::set_var("GRPC_TLS_ENABLED", "true") };
        assert_eq!(endpoint_url("reader:50051"), "https://reader:50051");
        unsafe { std::env::remove_var("GRPC_TLS_ENABLED") };
    }
}
