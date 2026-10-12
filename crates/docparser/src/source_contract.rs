//! Source V2 sidecar: identity, section ownership and physical coordinates are
//! independent. Rendered offsets are half-open UTF-8 byte ranges, never guesses.
use crate::{ConvertError, ReadResult, StructuredSourceLocator, StructuredSourceUnitKind};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderedSpan {
    pub start_byte: usize,
    pub end_byte: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceCompleteness {
    Complete,
    Partial,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TableHeaderCell {
    pub row: u32,
    pub column: u32,
    pub role: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceUnitIdentity {
    pub unit_id: String,
    pub ordinal: u32,
    pub kind: StructuredSourceUnitKind,
    pub text_sha256: String,
    pub grid_sha256: Option<String>,
    pub section_id: Option<String>,
    pub parent_section_id: Option<String>,
    pub heading_level: Option<u32>,
    pub heading_path: String,
    pub physical_locator: Option<StructuredSourceLocator>,
    pub physical_path: Option<String>,
    pub physical_locator_unavailable_reason: Option<String>,
    pub rendered_spans: Vec<RenderedSpan>,
    pub completeness: SourceCompleteness,
    pub reasons: Vec<String>,
    pub table_id: Option<String>,
    pub header_cells: Vec<TableHeaderCell>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcePage {
    pub page_ordinal: u32,
    pub classification: String,
    pub unit_ids: Vec<String>,
    pub image_unit_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlyphNormalization {
    pub page_ordinal: u32,
    pub char_index: u32,
    pub raw_symbol: String,
    pub normalized_symbol: String,
    pub font_name: String,
    pub font_sha256: String,
    pub glyph_name: String,
    pub left: f64,
    pub bottom: f64,
    pub right: f64,
    pub top: f64,
    pub page_width: f64,
    pub page_height: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceContract {
    pub schema_version: u32,
    pub document_revision: String,
    pub parser_version: String,
    pub markdown_sha256: String,
    pub units: Vec<SourceUnitIdentity>,
    pub page_manifest: Vec<SourcePage>,
    #[serde(default)]
    pub glyph_normalizations: Vec<GlyphNormalization>,
}

fn invalid(message: &str) -> ConvertError {
    ConvertError(format!("DocReader source contract: {message}"))
}

/// Versioned, cross-language framing independent of JSON field order/float
/// spelling. Grid anchor order is normalized; covered merge shells stay absent.
pub fn table_grid_digest(grid: &crate::TableGrid) -> String {
    let mut digest = Sha256::new();
    digest.update(b"docreader-grid-v2\0");
    digest.update(grid.row_count.to_be_bytes());
    digest.update(grid.column_count.to_be_bytes());
    digest.update((grid.cells.len() as u32).to_be_bytes());
    let mut cells: Vec<_> = grid.cells.iter().collect();
    cells.sort_by_key(|cell| (cell.row, cell.column));
    for cell in cells {
        for value in [cell.row, cell.column, cell.row_span, cell.col_span] {
            digest.update(value.to_be_bytes());
        }
        digest.update((cell.text.len() as u64).to_be_bytes());
        digest.update(cell.text.as_bytes());
    }
    let widths = grid.widths_mm.as_deref().unwrap_or_default();
    digest.update((widths.len() as u32).to_be_bytes());
    for width in widths {
        digest.update(width.to_be_bytes());
    }
    hex::encode(digest.finalize())
}

/// Validate the auditable normalization receipt at parse and frozen-load
/// boundaries. Exact source bytes stay immutable; this is a decoded view.
pub fn validate_glyph_normalizations(contract: &SourceContract) -> Result<(), ConvertError> {
    let mut seen = BTreeSet::new();
    for glyph in &contract.glyph_normalizations {
        let name =
            glyph
                .font_name
                .split_once('+')
                .map_or(glyph.font_name.as_str(), |(prefix, rest)| {
                    if prefix.len() == 6 && prefix.bytes().all(|ch| ch.is_ascii_uppercase()) {
                        rest
                    } else {
                        glyph.font_name.as_str()
                    }
                });
        let family = name
            .split(',')
            .next()
            .unwrap_or(name)
            .replace(' ', "")
            .to_ascii_lowercase();
        let supported = matches!(
            (
                glyph.raw_symbol.as_str(),
                glyph.normalized_symbol.as_str(),
                glyph.glyph_name.as_str()
            ),
            ("\u{f052}", "☑", "boxcheck") | ("\u{f0a3}", "□", "box1")
        );
        if family != "wingdings2"
            || !supported
            || glyph.font_sha256.len() != 64
            || !glyph.font_sha256.bytes().all(|ch| ch.is_ascii_hexdigit())
            || glyph.page_ordinal as usize >= contract.page_manifest.len()
            || !seen.insert((glyph.page_ordinal, glyph.char_index))
            || [
                glyph.left,
                glyph.bottom,
                glyph.right,
                glyph.top,
                glyph.page_width,
                glyph.page_height,
            ]
            .iter()
            .any(|value| !value.is_finite())
            || glyph.page_width <= 0.0
            || glyph.page_height <= 0.0
            || glyph.left < 0.0
            || glyph.bottom < 0.0
            || glyph.left > glyph.right
            || glyph.bottom > glyph.top
            || glyph.right > glyph.page_width
            || glyph.top > glyph.page_height
        {
            return Err(invalid(
                "unverified font symbol normalization or physical location",
            ));
        }
    }
    Ok(())
}

pub fn physical_page(locator: &StructuredSourceLocator) -> Option<u32> {
    match locator {
        StructuredSourceLocator::Page { page_ordinal, .. }
        | StructuredSourceLocator::PageTable { page_ordinal, .. } => Some(*page_ordinal),
        StructuredSourceLocator::Image { page_ordinal, .. } => *page_ordinal,
        _ => None,
    }
}

impl SourceContract {
    pub fn unit(&self, unit_id: &str) -> Option<&SourceUnitIdentity> {
        self.units.iter().find(|unit| unit.unit_id == unit_id)
    }

    pub fn validate(&self, parsed: &ReadResult) -> Result<(), ConvertError> {
        validate_glyph_normalizations(self)?;
        if self.schema_version != 2
            || self.document_revision.len() != 64
            || !self
                .document_revision
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || self.parser_version.trim().is_empty()
            || self.markdown_sha256 != hex::encode(Sha256::digest(parsed.markdown.as_bytes()))
            || self.units.len() != parsed.structured_source_units.len()
        {
            return Err(invalid(
                "version, source identity, rendering digest or unit count mismatch",
            ));
        }
        let mut identities = BTreeSet::new();
        for (ordinal, (identity, unit)) in self
            .units
            .iter()
            .zip(&parsed.structured_source_units)
            .enumerate()
        {
            if identity.unit_id != unit.key
                || unit.key.is_empty()
                || identity.ordinal as usize != ordinal
                || unit.ordinal as usize != ordinal
                || identity.kind != unit.kind
                || identity.text_sha256 != hex::encode(Sha256::digest(unit.text.as_bytes()))
                || identity.grid_sha256 != unit.grid.as_ref().map(table_grid_digest)
                || !identities.insert(unit.key.as_str())
                || (identity.completeness == SourceCompleteness::Complete)
                    != identity.reasons.is_empty()
                || identity
                    .reasons
                    .iter()
                    .any(|reason| reason.trim().is_empty())
                || identity.heading_level.is_some_and(|level| level == 0)
                || identity
                    .parent_section_id
                    .as_ref()
                    .is_some_and(|parent| Some(parent) == identity.section_id.as_ref())
            {
                return Err(invalid("invalid unit identity, ordinal or completeness"));
            }
            if identity.physical_locator.is_none()
                && !identity
                    .physical_locator_unavailable_reason
                    .as_ref()
                    .is_some_and(|reason| !reason.trim().is_empty())
            {
                return Err(invalid("missing physical location without explanation"));
            }
            if !matches!(unit.locator, StructuredSourceLocator::Document { .. })
                && identity.physical_locator.as_ref() != Some(&unit.locator)
            {
                return Err(invalid("physical locator differs from native carrier"));
            }
            let mut prior_end = 0;
            for span in &identity.rendered_spans {
                if span.start_byte >= span.end_byte
                    || span.start_byte < prior_end
                    || span.end_byte > parsed.markdown.len()
                    || !parsed.markdown.is_char_boundary(span.start_byte)
                    || !parsed.markdown.is_char_boundary(span.end_byte)
                {
                    return Err(invalid("invalid UTF-8 rendered span"));
                }
                prior_end = span.end_byte;
            }
            if unit.grid.is_some() != identity.table_id.is_some()
                || identity
                    .table_id
                    .as_ref()
                    .is_some_and(|table| table != &unit.key)
            {
                return Err(invalid("table identity differs from native grid"));
            }
            if let Some(grid) = &unit.grid {
                crate::validate_table_grid(grid).map_err(|error| invalid(&error))?;
            }
            for header in &identity.header_cells {
                if !matches!(header.role.as_str(), "column_header" | "row_header")
                    || !unit.grid.as_ref().is_some_and(|grid| {
                        grid.cells
                            .iter()
                            .any(|cell| cell.row == header.row && cell.column == header.column)
                    })
                {
                    return Err(invalid("header must reference a real grid anchor"));
                }
            }
        }
        let unit_index: BTreeMap<_, _> = self
            .units
            .iter()
            .map(|unit| (unit.unit_id.as_str(), unit))
            .collect();
        let mut covered = BTreeSet::new();
        for (ordinal, page) in self.page_manifest.iter().enumerate() {
            if page.page_ordinal as usize != ordinal
                || !matches!(page.classification.as_str(), "text" | "scanned" | "blank")
            {
                return Err(invalid("invalid physical page manifest order or class"));
            }
            let mut page_ids = BTreeSet::new();
            for key in &page.unit_ids {
                let unit = unit_index
                    .get(key.as_str())
                    .ok_or_else(|| invalid("unknown page unit"))?;
                if !page_ids.insert(key)
                    || !covered.insert(key)
                    || unit.physical_locator.as_ref().and_then(physical_page)
                        != Some(page.page_ordinal)
                {
                    return Err(invalid("page/unit physical location mismatch"));
                }
            }
            let expected_images: BTreeSet<_> = page
                .unit_ids
                .iter()
                .filter(|key| {
                    unit_index
                        .get(key.as_str())
                        .is_some_and(|unit| unit.kind == StructuredSourceUnitKind::ImageRegion)
                })
                .collect();
            let actual_images: BTreeSet<_> = page.image_unit_ids.iter().collect();
            if expected_images != actual_images || actual_images.len() != page.image_unit_ids.len()
            {
                return Err(invalid("page image ownership mismatch"));
            }
        }
        if !self.page_manifest.is_empty() && covered.len() != self.units.len() {
            return Err(invalid("page manifest omits source units"));
        }
        if let Some(count) = parsed.metadata.get("page_count") {
            let count = count
                .parse::<usize>()
                .map_err(|_| invalid("invalid page count"))?;
            if count != self.page_manifest.len() {
                return Err(invalid("physical page coverage incomplete"));
            }
        }
        Ok(())
    }

    /// Adjust exact mappings during a host-owned literal replacement. Reject
    /// edits cutting through a mapping boundary rather than claim precision.
    pub(crate) fn replace_rendered(
        &mut self,
        markdown: &mut String,
        from: &str,
        to: &str,
    ) -> Result<(), ConvertError> {
        if from.is_empty() || from == to {
            return Ok(());
        }
        let ranges: Vec<_> = markdown
            .match_indices(from)
            .map(|(start, _)| (start, start + from.len()))
            .collect();
        if ranges.is_empty() {
            return Ok(());
        }
        let shift = |offset: usize| -> Result<usize, ConvertError> {
            let mut adjustment: i128 = 0;
            for &(start, end) in &ranges {
                if start < offset && offset < end {
                    return Err(invalid("image replacement cuts a rendered span boundary"));
                }
                if end <= offset {
                    adjustment += to.len() as i128 - from.len() as i128;
                }
            }
            usize::try_from(offset as i128 + adjustment)
                .map_err(|_| invalid("rendered span overflow"))
        };
        let mut updated = self.units.clone();
        for unit in &mut updated {
            for span in &mut unit.rendered_spans {
                span.start_byte = shift(span.start_byte)?;
                span.end_byte = shift(span.end_byte)?;
            }
        }
        *markdown = markdown.replace(from, to);
        self.units = updated;
        self.markdown_sha256 = hex::encode(Sha256::digest(markdown.as_bytes()));
        Ok(())
    }
}

/// Missing metadata is explicitly unresolved; a present receipt is always
/// validated and cannot degrade to a guessed string match.
pub fn parse_source_contract(parsed: &ReadResult) -> Result<Option<SourceContract>, ConvertError> {
    let Some(raw) = parsed.metadata.get("source_contract") else {
        return Ok(None);
    };
    let contract: SourceContract =
        serde_json::from_str(raw).map_err(|_| invalid("malformed V2 sidecar"))?;
    contract.validate(parsed)?;
    Ok(Some(contract))
}

#[cfg(test)]
mod symbol_tests {
    use super::*;

    fn source() -> SourceContract {
        SourceContract {
            schema_version: 2,
            document_revision: "a".repeat(64),
            parser_version: "synthetic".into(),
            markdown_sha256: "b".repeat(64),
            units: vec![],
            page_manifest: vec![SourcePage {
                page_ordinal: 0,
                classification: "text".into(),
                unit_ids: vec![],
                image_unit_ids: vec![],
            }],
            glyph_normalizations: vec![GlyphNormalization {
                page_ordinal: 0,
                char_index: 7,
                raw_symbol: "\u{f052}".into(),
                normalized_symbol: "☑".into(),
                font_name: "ABCDEF+Wingdings2,Bold".into(),
                font_sha256: "c".repeat(64),
                glyph_name: "boxcheck".into(),
                left: 10.0,
                bottom: 10.0,
                right: 20.0,
                top: 20.0,
                page_width: 100.0,
                page_height: 100.0,
            }],
        }
    }

    #[test]
    fn glyph_receipt_rejects_unverified_mappings_fonts_and_physical_positions() {
        let valid = source();
        validate_glyph_normalizations(&valid).unwrap();
        for corrupt in [
            |glyph: &mut GlyphNormalization| glyph.font_name = "Wingdings".into(),
            |glyph: &mut GlyphNormalization| glyph.normalized_symbol = "□".into(),
            |glyph: &mut GlyphNormalization| glyph.glyph_name = "box1".into(),
            |glyph: &mut GlyphNormalization| glyph.left = f64::NAN,
            |glyph: &mut GlyphNormalization| glyph.right = 101.0,
            |glyph: &mut GlyphNormalization| glyph.page_ordinal = 1,
            |glyph: &mut GlyphNormalization| glyph.font_sha256 = "unverified".into(),
        ] {
            let mut changed = valid.clone();
            corrupt(&mut changed.glyph_normalizations[0]);
            assert!(validate_glyph_normalizations(&changed).is_err());
        }
        let mut duplicate = valid.clone();
        duplicate
            .glyph_normalizations
            .push(valid.glyph_normalizations[0].clone());
        assert!(validate_glyph_normalizations(&duplicate).is_err());
    }
}
