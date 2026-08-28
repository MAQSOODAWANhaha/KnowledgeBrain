use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read};
use std::path::Path;

use image::ImageFormat;
use thiserror::Error;
use zip::ZipArchive;

const MAX_UPLOAD_BYTES: usize = 50 * 1024 * 1024;
const MAX_ZIP_ENTRIES: usize = 4_096;
const MAX_ZIP_UNCOMPRESSED_BYTES: u64 = 100 * 1024 * 1024;
const MAX_ZIP_COMPRESSION_RATIO: u64 = 100;
const MAX_CONTENT_TYPES_BYTES: u64 = 1024 * 1024;
const MAX_OFFICE_XML_BYTES: u64 = 50 * 1024 * 1024;
const MAX_XLSX_LOGICAL_ROW: u32 = 100_000;
const MAX_XLSX_LOGICAL_COLUMN: u32 = 2_048;
const MAX_XLSX_LOGICAL_AREA: u64 = 10_000_000;
const MAX_XLSX_MATERIALIZED_CELLS: usize = 100_000;
const MAX_XLSX_CELLS_PER_ROW: usize = 2_048;
const MAX_XLSX_MERGES: usize = 10_000;
const MAX_XLSX_TABLES: usize = 1_000;
const MAX_XLSX_RANGE_CELLS: u64 = 1_000_000;

pub const PDF_MEDIA_TYPE: &str = "application/pdf";
pub const DOCX_MEDIA_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
pub const XLSX_MEDIA_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
pub const PNG_MEDIA_TYPE: &str = "image/png";
pub const JPEG_MEDIA_TYPE: &str = "image/jpeg";
pub const WEBP_MEDIA_TYPE: &str = "image/webp";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedTenderUpload {
    pub media_type: &'static str,
    pub extension: &'static str,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TenderUploadError {
    #[error("TENDER_UPLOAD_EMPTY")]
    Empty,
    #[error("TENDER_UPLOAD_TOO_LARGE")]
    TooLarge,
    #[error("TENDER_UPLOAD_FILENAME_TYPE_INVALID")]
    FilenameTypeInvalid,
    #[error("TENDER_UPLOAD_DECLARED_TYPE_INVALID")]
    DeclaredTypeInvalid,
    #[error("TENDER_UPLOAD_MAGIC_INVALID")]
    MagicInvalid,
    #[error("TENDER_PDF_STRUCTURE_INVALID")]
    PdfStructureInvalid,
    #[error("TENDER_OFFICE_CONTAINER_INVALID")]
    OfficeContainerInvalid,
    #[error("TENDER_OFFICE_CONTAINER_UNSAFE")]
    OfficeContainerUnsafe,
    #[error("TENDER_OFFICE_KIND_MISMATCH")]
    OfficeKindMismatch,
    #[error("TENDER_XLSX_STRUCTURE_UNSAFE")]
    SpreadsheetStructureUnsafe,
}

pub fn validate_tender_upload(
    file_name: &str,
    declared_media_type: Option<&str>,
    bytes: &[u8],
) -> Result<ValidatedTenderUpload, TenderUploadError> {
    if bytes.is_empty() {
        return Err(TenderUploadError::Empty);
    }
    if bytes.len() > MAX_UPLOAD_BYTES {
        return Err(TenderUploadError::TooLarge);
    }
    let extension = Path::new(file_name)
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or(TenderUploadError::FilenameTypeInvalid)?;
    let (media_type, canonical_extension) = match extension.as_str() {
        "pdf" => (validate_pdf(bytes)?, "pdf"),
        "docx" => (validate_office(bytes, OfficeKind::Docx)?, "docx"),
        "xlsx" => (validate_office(bytes, OfficeKind::Xlsx)?, "xlsx"),
        "png" => (
            validate_image(bytes, ImageFormat::Png, PNG_MEDIA_TYPE)?,
            "png",
        ),
        "jpg" | "jpeg" => (
            validate_image(bytes, ImageFormat::Jpeg, JPEG_MEDIA_TYPE)?,
            "jpg",
        ),
        "webp" => (
            validate_image(bytes, ImageFormat::WebP, WEBP_MEDIA_TYPE)?,
            "webp",
        ),
        _ => return Err(TenderUploadError::FilenameTypeInvalid),
    };
    let declared = declared_media_type
        .map(|value| {
            value
                .split(';')
                .next()
                .unwrap_or(value)
                .trim()
                .to_ascii_lowercase()
        })
        .filter(|value| !value.is_empty())
        .ok_or(TenderUploadError::DeclaredTypeInvalid)?;
    if declared != media_type {
        return Err(TenderUploadError::DeclaredTypeInvalid);
    }
    Ok(ValidatedTenderUpload {
        media_type,
        extension: canonical_extension,
    })
}

fn validate_pdf(bytes: &[u8]) -> Result<&'static str, TenderUploadError> {
    if !bytes.starts_with(b"%PDF-") {
        return Err(TenderUploadError::MagicInvalid);
    }
    let eof = bytes
        .windows(b"%%EOF".len())
        .rposition(|window| window == b"%%EOF")
        .ok_or(TenderUploadError::PdfStructureInvalid)?;
    if bytes[eof + b"%%EOF".len()..]
        .iter()
        .any(|byte| !byte.is_ascii_whitespace())
    {
        return Err(TenderUploadError::PdfStructureInvalid);
    }
    let document =
        lopdf::Document::load_mem(bytes).map_err(|_| TenderUploadError::PdfStructureInvalid)?;
    if document.is_encrypted()
        || document.get_pages().is_empty()
        || document.trailer.get(b"Root").is_err()
    {
        return Err(TenderUploadError::PdfStructureInvalid);
    }
    Ok(PDF_MEDIA_TYPE)
}

fn validate_image(
    bytes: &[u8],
    expected: ImageFormat,
    media_type: &'static str,
) -> Result<&'static str, TenderUploadError> {
    let guessed = image::guess_format(bytes).map_err(|_| TenderUploadError::MagicInvalid)?;
    if guessed != expected {
        return Err(TenderUploadError::MagicInvalid);
    }
    image::load_from_memory_with_format(bytes, expected)
        .map_err(|_| TenderUploadError::MagicInvalid)?;
    Ok(media_type)
}

#[derive(Clone, Copy)]
enum OfficeKind {
    Docx,
    Xlsx,
}

fn validate_office(bytes: &[u8], expected: OfficeKind) -> Result<&'static str, TenderUploadError> {
    if !bytes.starts_with(b"PK\x03\x04") {
        return Err(TenderUploadError::MagicInvalid);
    }
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| TenderUploadError::OfficeContainerInvalid)?;
    if archive.is_empty() || archive.len() > MAX_ZIP_ENTRIES {
        return Err(TenderUploadError::OfficeContainerUnsafe);
    }
    let mut names = HashSet::with_capacity(archive.len());
    let mut total_uncompressed = 0_u64;
    let mut xml_parts = HashMap::new();
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|_| TenderUploadError::OfficeContainerInvalid)?;
        let name = file.name().replace('\\', "/");
        if name.starts_with('/')
            || name.split('/').any(|part| part == "..")
            || file.enclosed_name().is_none()
            || !names.insert(name.clone())
            || file.encrypted()
        {
            return Err(TenderUploadError::OfficeContainerUnsafe);
        }
        total_uncompressed = total_uncompressed
            .checked_add(file.size())
            .ok_or(TenderUploadError::OfficeContainerUnsafe)?;
        if total_uncompressed > MAX_ZIP_UNCOMPRESSED_BYTES
            || (file.compressed_size() == 0 && file.size() > 0)
            || (file.compressed_size() > 0
                && file.size()
                    > file
                        .compressed_size()
                        .saturating_mul(MAX_ZIP_COMPRESSION_RATIO))
        {
            return Err(TenderUploadError::OfficeContainerUnsafe);
        }
        let capture = matches!(
            name.as_str(),
            "[Content_Types].xml"
                | "_rels/.rels"
                | "word/document.xml"
                | "word/_rels/document.xml.rels"
                | "xl/workbook.xml"
                | "xl/_rels/workbook.xml.rels"
        ) || (matches!(expected, OfficeKind::Xlsx)
            && ((name.starts_with("xl/worksheets/") && name.ends_with(".xml"))
                || (name.starts_with("xl/tables/") && name.ends_with(".xml"))));
        if capture {
            let limit = if name == "[Content_Types].xml" {
                MAX_CONTENT_TYPES_BYTES
            } else {
                MAX_OFFICE_XML_BYTES
            };
            if file.size() > limit {
                return Err(TenderUploadError::OfficeContainerUnsafe);
            }
            let mut value = String::new();
            file.read_to_string(&mut value)
                .map_err(|_| TenderUploadError::OfficeContainerInvalid)?;
            xml_parts.insert(name, value);
        }
    }
    let media_type = validate_ooxml_parts(expected, &xml_parts)?;
    if matches!(expected, OfficeKind::Xlsx) {
        validate_xlsx_structure(&xml_parts)?;
    }
    Ok(media_type)
}

fn validate_ooxml_parts(
    expected: OfficeKind,
    parts: &HashMap<String, String>,
) -> Result<&'static str, TenderUploadError> {
    const RELS_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
    const CONTENT_TYPES_NS: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
    const OFFICE_DOCUMENT_REL: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
    let (main_part, main_rels, main_type, root_name, root_ns, media_type) = match expected {
        OfficeKind::Docx => (
            "word/document.xml",
            "word/_rels/document.xml.rels",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
            "document",
            "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
            DOCX_MEDIA_TYPE,
        ),
        OfficeKind::Xlsx => (
            "xl/workbook.xml",
            "xl/_rels/workbook.xml.rels",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml",
            "workbook",
            "http://schemas.openxmlformats.org/spreadsheetml/2006/main",
            XLSX_MEDIA_TYPE,
        ),
    };
    let content_types = roxmltree::Document::parse(
        parts
            .get("[Content_Types].xml")
            .ok_or(TenderUploadError::OfficeContainerInvalid)?,
    )
    .map_err(|_| TenderUploadError::OfficeContainerInvalid)?;
    let content_root = content_types.root_element();
    if content_root.tag_name().name() != "Types"
        || content_root.tag_name().namespace() != Some(CONTENT_TYPES_NS)
    {
        return Err(TenderUploadError::OfficeContainerInvalid);
    }
    let declared_main = content_root.descendants().any(|node| {
        node.is_element()
            && node.tag_name().name() == "Override"
            && node
                .attribute("PartName")
                .map(|v| v.trim_start_matches('/'))
                == Some(main_part)
            && node.attribute("ContentType") == Some(main_type)
    });
    if !declared_main {
        return Err(TenderUploadError::OfficeKindMismatch);
    }

    let package_rels = parse_relationships(
        parts
            .get("_rels/.rels")
            .ok_or(TenderUploadError::OfficeContainerInvalid)?,
        RELS_NS,
    )?;
    let owns_main = package_rels.descendants().any(|node| {
        node.is_element()
            && node.tag_name().name() == "Relationship"
            && node.attribute("Type") == Some(OFFICE_DOCUMENT_REL)
            && node.attribute("Target").map(|v| v.trim_start_matches('/')) == Some(main_part)
    });
    if !owns_main {
        return Err(TenderUploadError::OfficeContainerInvalid);
    }

    let main = roxmltree::Document::parse(
        parts
            .get(main_part)
            .ok_or(TenderUploadError::OfficeContainerInvalid)?,
    )
    .map_err(|_| TenderUploadError::OfficeContainerInvalid)?;
    if main.root_element().tag_name().name() != root_name
        || main.root_element().tag_name().namespace() != Some(root_ns)
    {
        return Err(TenderUploadError::OfficeContainerInvalid);
    }
    if let Some(relationships) = parts.get(main_rels) {
        parse_relationships(relationships, RELS_NS)?;
    } else if matches!(expected, OfficeKind::Xlsx) {
        return Err(TenderUploadError::OfficeContainerInvalid);
    }
    Ok(media_type)
}

fn parse_xlsx_cell(value: &str) -> Option<(u32, u32)> {
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

fn parse_xlsx_range(value: &str) -> Option<(u32, u32, u32, u32)> {
    let mut values = value.split(':');
    let (start_row, start_column) = parse_xlsx_cell(values.next()?)?;
    let (end_row, end_column) = parse_xlsx_cell(values.next().unwrap_or(value))?;
    if values.next().is_some() || end_row < start_row || end_column < start_column {
        return None;
    }
    Some((start_row, start_column, end_row, end_column))
}

fn validate_xlsx_range(value: &str, max_area: u64) -> Result<(), TenderUploadError> {
    let (start_row, start_column, end_row, end_column) =
        parse_xlsx_range(value).ok_or(TenderUploadError::SpreadsheetStructureUnsafe)?;
    let rows = u64::from(end_row - start_row + 1);
    let columns = u64::from(end_column - start_column + 1);
    if end_row > MAX_XLSX_LOGICAL_ROW
        || end_column > MAX_XLSX_LOGICAL_COLUMN
        || rows.saturating_mul(columns) > max_area
    {
        return Err(TenderUploadError::SpreadsheetStructureUnsafe);
    }
    Ok(())
}

fn validate_xlsx_structure(parts: &HashMap<String, String>) -> Result<(), TenderUploadError> {
    const SPREADSHEET_NS: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
    let worksheets = parts
        .iter()
        .filter(|(name, _)| name.starts_with("xl/worksheets/") && name.ends_with(".xml"));
    let mut worksheet_count = 0_usize;
    let mut materialized_cells = 0_usize;
    let mut merge_count = 0_usize;
    for (_, xml) in worksheets {
        worksheet_count += 1;
        let document = roxmltree::Document::parse(xml)
            .map_err(|_| TenderUploadError::SpreadsheetStructureUnsafe)?;
        let root = document.root_element();
        if root.tag_name().name() != "worksheet"
            || root.tag_name().namespace() != Some(SPREADSHEET_NS)
        {
            return Err(TenderUploadError::SpreadsheetStructureUnsafe);
        }
        for dimension in root
            .descendants()
            .filter(|node| node.is_element() && node.tag_name().name() == "dimension")
        {
            validate_xlsx_range(
                dimension
                    .attribute("ref")
                    .ok_or(TenderUploadError::SpreadsheetStructureUnsafe)?,
                MAX_XLSX_LOGICAL_AREA,
            )?;
        }
        let mut cells_per_row: HashMap<u32, usize> = HashMap::new();
        for cell in root
            .descendants()
            .filter(|node| node.is_element() && node.tag_name().name() == "c")
        {
            materialized_cells = materialized_cells
                .checked_add(1)
                .ok_or(TenderUploadError::SpreadsheetStructureUnsafe)?;
            if materialized_cells > MAX_XLSX_MATERIALIZED_CELLS {
                return Err(TenderUploadError::SpreadsheetStructureUnsafe);
            }
            let (row, column) = parse_xlsx_cell(
                cell.attribute("r")
                    .ok_or(TenderUploadError::SpreadsheetStructureUnsafe)?,
            )
            .ok_or(TenderUploadError::SpreadsheetStructureUnsafe)?;
            if row > MAX_XLSX_LOGICAL_ROW || column > MAX_XLSX_LOGICAL_COLUMN {
                return Err(TenderUploadError::SpreadsheetStructureUnsafe);
            }
            let count = cells_per_row.entry(row).or_default();
            *count += 1;
            if *count > MAX_XLSX_CELLS_PER_ROW {
                return Err(TenderUploadError::SpreadsheetStructureUnsafe);
            }
        }
        for merged in root
            .descendants()
            .filter(|node| node.is_element() && node.tag_name().name() == "mergeCell")
        {
            merge_count += 1;
            if merge_count > MAX_XLSX_MERGES {
                return Err(TenderUploadError::SpreadsheetStructureUnsafe);
            }
            validate_xlsx_range(
                merged
                    .attribute("ref")
                    .ok_or(TenderUploadError::SpreadsheetStructureUnsafe)?,
                MAX_XLSX_RANGE_CELLS,
            )?;
        }
    }
    if worksheet_count == 0 {
        return Err(TenderUploadError::SpreadsheetStructureUnsafe);
    }

    let tables: Vec<_> = parts
        .iter()
        .filter(|(name, _)| name.starts_with("xl/tables/") && name.ends_with(".xml"))
        .collect();
    if tables.len() > MAX_XLSX_TABLES {
        return Err(TenderUploadError::SpreadsheetStructureUnsafe);
    }
    for (_, xml) in tables {
        let document = roxmltree::Document::parse(xml)
            .map_err(|_| TenderUploadError::SpreadsheetStructureUnsafe)?;
        let root = document.root_element();
        if root.tag_name().name() != "table" || root.tag_name().namespace() != Some(SPREADSHEET_NS)
        {
            return Err(TenderUploadError::SpreadsheetStructureUnsafe);
        }
        validate_xlsx_range(
            root.attribute("ref")
                .ok_or(TenderUploadError::SpreadsheetStructureUnsafe)?,
            MAX_XLSX_RANGE_CELLS,
        )?;
    }
    Ok(())
}

fn parse_relationships<'a>(
    xml: &'a str,
    namespace: &str,
) -> Result<roxmltree::Document<'a>, TenderUploadError> {
    let document =
        roxmltree::Document::parse(xml).map_err(|_| TenderUploadError::OfficeContainerInvalid)?;
    let root = document.root_element();
    if root.tag_name().name() != "Relationships" || root.tag_name().namespace() != Some(namespace) {
        return Err(TenderUploadError::OfficeContainerInvalid);
    }
    for relation in root.children().filter(|node| node.is_element()) {
        if relation.tag_name().name() != "Relationship"
            || relation.attribute("Id").is_none()
            || relation.attribute("Type").is_none()
            || relation.attribute("Target").is_none()
        {
            return Err(TenderUploadError::OfficeContainerInvalid);
        }
    }
    Ok(document)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba};
    use zip::write::SimpleFileOptions;

    use super::*;

    fn image_bytes(format: ImageFormat) -> Vec<u8> {
        let image = DynamicImage::ImageRgba8(ImageBuffer::from_pixel(2, 3, Rgba([1, 2, 3, 255])));
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, format).unwrap();
        bytes.into_inner()
    }

    fn office(kind: OfficeKind, extra: &[(&str, &[u8])]) -> Vec<u8> {
        const RELS_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
        const OFFICE_REL: &str =
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
        let cursor = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let entries: Vec<(&str, String)> = match kind {
            OfficeKind::Docx => vec![
                ("[Content_Types].xml", "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>".into()),
                ("_rels/.rels", format!("<Relationships xmlns=\"{RELS_NS}\"><Relationship Id=\"rId1\" Type=\"{OFFICE_REL}\" Target=\"word/document.xml\"/></Relationships>")),
                ("word/document.xml", "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p><w:r><w:t>Tender</w:t></w:r></w:p><w:sectPr/></w:body></w:document>".into()),
                ("word/_rels/document.xml.rels", format!("<Relationships xmlns=\"{RELS_NS}\"/>")),
            ],
            OfficeKind::Xlsx => vec![
                ("[Content_Types].xml", "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/><Override PartName=\"/xl/worksheets/sheet1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/></Types>".into()),
                ("_rels/.rels", format!("<Relationships xmlns=\"{RELS_NS}\"><Relationship Id=\"rId1\" Type=\"{OFFICE_REL}\" Target=\"xl/workbook.xml\"/></Relationships>")),
                ("xl/workbook.xml", "<workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><sheets><sheet name=\"Sheet1\" sheetId=\"1\" r:id=\"rId1\"/></sheets></workbook>".into()),
                ("xl/_rels/workbook.xml.rels", format!("<Relationships xmlns=\"{RELS_NS}\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet1.xml\"/></Relationships>")),
                ("xl/worksheets/sheet1.xml", "<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData/></worksheet>".into()),
            ],
        };
        for (name, value) in entries {
            if extra.iter().any(|(extra_name, _)| *extra_name == name) {
                continue;
            }
            writer.start_file(name, options).unwrap();
            writer.write_all(value.as_bytes()).unwrap();
        }
        for (name, value) in extra {
            writer.start_file(*name, options).unwrap();
            writer.write_all(value).unwrap();
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

    #[test]
    fn exactly_six_formats_are_accepted_with_matching_filename_and_declared_type() {
        let pdf = minimal_pdf();
        assert_eq!(
            validate_tender_upload("a.pdf", Some(PDF_MEDIA_TYPE), &pdf)
                .unwrap()
                .media_type,
            PDF_MEDIA_TYPE
        );
        assert_eq!(
            validate_tender_upload(
                "a.docx",
                Some(DOCX_MEDIA_TYPE),
                &office(OfficeKind::Docx, &[])
            )
            .unwrap()
            .media_type,
            DOCX_MEDIA_TYPE
        );
        assert_eq!(
            validate_tender_upload(
                "a.xlsx",
                Some(XLSX_MEDIA_TYPE),
                &office(OfficeKind::Xlsx, &[])
            )
            .unwrap()
            .media_type,
            XLSX_MEDIA_TYPE
        );
        for (name, mime, format) in [
            ("a.png", PNG_MEDIA_TYPE, ImageFormat::Png),
            ("a.jpg", JPEG_MEDIA_TYPE, ImageFormat::Jpeg),
            ("a.webp", WEBP_MEDIA_TYPE, ImageFormat::WebP),
        ] {
            assert_eq!(
                validate_tender_upload(name, Some(mime), &image_bytes(format))
                    .unwrap()
                    .media_type,
                mime
            );
        }
    }

    #[test]
    fn filename_declared_type_magic_and_office_kind_must_agree() {
        let docx = office(OfficeKind::Docx, &[]);
        assert_eq!(
            validate_tender_upload("a.xlsx", Some(XLSX_MEDIA_TYPE), &docx),
            Err(TenderUploadError::OfficeKindMismatch)
        );
        assert_eq!(
            validate_tender_upload("a.docx", Some(PDF_MEDIA_TYPE), &docx),
            Err(TenderUploadError::DeclaredTypeInvalid)
        );
        assert_eq!(
            validate_tender_upload("a.docx", None, &docx),
            Err(TenderUploadError::DeclaredTypeInvalid)
        );
        assert_eq!(
            validate_tender_upload("a.png", Some(PNG_MEDIA_TYPE), b"not-png"),
            Err(TenderUploadError::MagicInvalid)
        );
        assert_eq!(
            validate_tender_upload("a.txt", Some("text/plain"), b"x"),
            Err(TenderUploadError::FilenameTypeInvalid)
        );
    }

    fn mark_first_entry_encrypted(mut bytes: Vec<u8>) -> Vec<u8> {
        if bytes.starts_with(b"PK\x03\x04") {
            let flags = u16::from_le_bytes([bytes[6], bytes[7]]) | 1;
            bytes[6..8].copy_from_slice(&flags.to_le_bytes());
        }
        if let Some(offset) = bytes.windows(4).position(|value| value == b"PK\x01\x02") {
            let flags = u16::from_le_bytes([bytes[offset + 8], bytes[offset + 9]]) | 1;
            bytes[offset + 8..offset + 10].copy_from_slice(&flags.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn structurally_malformed_pdf_and_ooxml_are_rejected() {
        assert_eq!(
            validate_tender_upload(
                "a.pdf",
                Some(PDF_MEDIA_TYPE),
                b"%PDF-1.7\n1 0 obj << /Type /Catalog >> endobj\n%%EOF\n"
            ),
            Err(TenderUploadError::PdfStructureInvalid)
        );
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, value) in [
            (
                "[Content_Types].xml",
                "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>",
            ),
            (
                "_rels/.rels",
                "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>",
            ),
            ("word/document.xml", "<root/>"),
            (
                "word/_rels/document.xml.rels",
                "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"/>",
            ),
        ] {
            writer.start_file(name, options).unwrap();
            writer.write_all(value.as_bytes()).unwrap();
        }
        let marker_only_docx = writer.finish().unwrap().into_inner();
        assert_eq!(
            validate_tender_upload("a.docx", Some(DOCX_MEDIA_TYPE), &marker_only_docx),
            Err(TenderUploadError::OfficeContainerInvalid)
        );
        let mut wrong_relationship = office(OfficeKind::Docx, &[]);
        let needle = b"word/document.xml";
        let at = wrong_relationship
            .windows(needle.len())
            .position(|value| value == needle)
            .expect("root relationship target");
        wrong_relationship[at..at + needle.len()].copy_from_slice(b"word/other.xmlxxx");
        assert!(
            validate_tender_upload("a.docx", Some(DOCX_MEDIA_TYPE), &wrong_relationship).is_err()
        );
    }

    #[test]
    fn sparse_xlsx_logical_dimension_is_rejected_before_parser_expansion() {
        let sparse_sheet = br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><dimension ref="A1:XFD1048576"/><sheetData><row r="1048576"><c r="XFD1048576" t="inlineStr"><is><t>bomb</t></is></c></row></sheetData></worksheet>"#;
        assert_eq!(
            validate_tender_upload(
                "sparse.xlsx",
                Some(XLSX_MEDIA_TYPE),
                &office(
                    OfficeKind::Xlsx,
                    &[("xl/worksheets/sheet1.xml", sparse_sheet)]
                ),
            ),
            Err(TenderUploadError::SpreadsheetStructureUnsafe)
        );
    }

    #[test]
    fn malformed_traversal_duplicate_encrypted_and_ratio_bomb_archives_are_rejected() {
        assert_eq!(
            validate_tender_upload("a.docx", Some(DOCX_MEDIA_TYPE), b"PK\x03\x04broken"),
            Err(TenderUploadError::OfficeContainerInvalid)
        );
        for extra in [
            vec![("../evil", b"x".as_slice())],
            vec![("word\\document.xml", b"duplicate".as_slice())],
        ] {
            assert_eq!(
                validate_tender_upload(
                    "a.docx",
                    Some(DOCX_MEDIA_TYPE),
                    &office(OfficeKind::Docx, &extra)
                ),
                Err(TenderUploadError::OfficeContainerUnsafe)
            );
        }
        assert!(
            validate_tender_upload(
                "a.docx",
                Some(DOCX_MEDIA_TYPE),
                &mark_first_entry_encrypted(office(OfficeKind::Docx, &[])),
            )
            .is_err()
        );
        let cursor = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        let deflated =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        writer.start_file("[Content_Types].xml", deflated).unwrap();
        writer.write_all(&vec![b'a'; 2 * 1024 * 1024]).unwrap();
        writer.start_file("_rels/.rels", deflated).unwrap();
        writer.write_all(b"x").unwrap();
        writer.start_file("word/document.xml", deflated).unwrap();
        writer.write_all(&vec![0; 4 * 1024 * 1024]).unwrap();
        let oversize_metadata = writer.finish().unwrap().into_inner();
        assert_eq!(
            validate_tender_upload("a.docx", Some(DOCX_MEDIA_TYPE), &oversize_metadata),
            Err(TenderUploadError::OfficeContainerUnsafe)
        );

        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, value) in [
            (
                "[Content_Types].xml",
                b"<Types><Override ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>".as_slice(),
            ),
            ("_rels/.rels", b"<Relationships/>".as_slice()),
            ("word/document.xml", b"<root/>".as_slice()),
        ] {
            writer.start_file(name, deflated).unwrap();
            writer.write_all(value).unwrap();
        }
        writer.start_file("word/media/bomb.bin", deflated).unwrap();
        writer.write_all(&vec![0; 4 * 1024 * 1024]).unwrap();
        let ratio_bomb = writer.finish().unwrap().into_inner();
        assert_eq!(
            validate_tender_upload("a.docx", Some(DOCX_MEDIA_TYPE), &ratio_bomb),
            Err(TenderUploadError::OfficeContainerUnsafe)
        );
    }
}
