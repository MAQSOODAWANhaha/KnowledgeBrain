//! Frozen, carrier-specific evidence. Offsets are nonempty UTF-8 byte ranges.
use crate::analysis::FrozenInput;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EvidenceRef {
    Text {
        input_digest: String,
        unit_id: String,
        start_byte: usize,
        end_byte: usize,
    },
    GridCell {
        input_digest: String,
        table_id: String,
        anchor_row: usize,
        anchor_column: usize,
        start_byte: usize,
        end_byte: usize,
    },
    ImageRegion {
        input_digest: String,
        image_id: String,
        region: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceExcerpt {
    pub evidence: EvidenceRef,
    pub quote: String,
    pub quote_digest: String,
    pub locator: Value,
    pub completeness: String,
}

pub fn input_digest(input: &FrozenInput) -> Result<String, String> {
    #[cfg(test)]
    let _profile = crate::analysis::agent::profile::enter("frozen_input_digest");
    super::canonical_sha256(input)
}

impl EvidenceRef {
    pub fn digest(&self) -> &str {
        match self {
            Self::Text { input_digest, .. }
            | Self::GridCell { input_digest, .. }
            | Self::ImageRegion { input_digest, .. } => input_digest,
        }
    }
    pub fn range(&self) -> Option<(usize, usize)> {
        match self {
            Self::Text {
                start_byte,
                end_byte,
                ..
            }
            | Self::GridCell {
                start_byte,
                end_byte,
                ..
            } => Some((*start_byte, *end_byte)),
            Self::ImageRegion { .. } => None,
        }
    }
    pub fn same_carrier(&self, other: &Self) -> bool {
        if self.digest() != other.digest() {
            return false;
        }
        match (self, other) {
            (Self::Text { unit_id: a, .. }, Self::Text { unit_id: b, .. }) => a == b,
            (
                Self::GridCell {
                    table_id: a,
                    anchor_row: ar,
                    anchor_column: ac,
                    ..
                },
                Self::GridCell {
                    table_id: b,
                    anchor_row: br,
                    anchor_column: bc,
                    ..
                },
            ) => (a, ar, ac) == (b, br, bc),
            (
                Self::ImageRegion {
                    image_id: a,
                    region: ar,
                    ..
                },
                Self::ImageRegion {
                    image_id: b,
                    region: br,
                    ..
                },
            ) => (a, ar) == (b, br),
            _ => false,
        }
    }
}

/// Adjacent spans form one delivered interval. A gap is never bridged.
pub fn covered_by_union(candidate: &EvidenceRef, delivered: &[EvidenceRef]) -> bool {
    let Some((start, end)) = candidate.range() else {
        return delivered.iter().any(|part| candidate.same_carrier(part));
    };
    if start >= end {
        return false;
    }
    let mut ranges: Vec<_> = delivered
        .iter()
        .filter(|part| candidate.same_carrier(part))
        .filter_map(EvidenceRef::range)
        .collect();
    ranges.sort_unstable();
    let mut cursor = start;
    for (left, right) in ranges {
        if right <= cursor {
            continue;
        }
        if left > cursor {
            return false;
        }
        cursor = right;
        if cursor >= end {
            return true;
        }
    }
    false
}

pub fn validate_evidence(
    refs: &[EvidenceRef],
    input: &FrozenInput,
    delivered_scope: &[EvidenceRef],
) -> Result<Vec<SourceExcerpt>, String> {
    let excerpts = resolve_evidence(input, refs)?;
    // Scope itself must describe real source ranges, rather than fabricated bounds.
    resolve_evidence(input, delivered_scope)?;
    for reference in refs {
        if !covered_by_union(reference, delivered_scope) {
            return Err(
                "outside_scope: evidence is not wholly covered by delivered intervals".into(),
            );
        }
    }
    Ok(excerpts)
}

pub fn resolve_evidence(
    input: &FrozenInput,
    refs: &[EvidenceRef],
) -> Result<Vec<SourceExcerpt>, String> {
    EvidenceResolver::new(input)?.resolve(refs)
}

/// A digest belongs to this immutable borrow, never to a caller-supplied string
/// or a cross-run cache. All reference/range/anchor checks still run per read.
pub(super) struct EvidenceResolver<'a> {
    input: &'a FrozenInput,
    digest: String,
}

impl<'a> EvidenceResolver<'a> {
    pub(super) fn new(input: &'a FrozenInput) -> Result<Self, String> {
        Ok(Self {
            input,
            digest: input_digest(input)?,
        })
    }

    pub(super) fn input(&self) -> &'a FrozenInput {
        self.input
    }

    pub(super) fn digest(&self) -> &str {
        &self.digest
    }

    pub(super) fn resolve(&self, refs: &[EvidenceRef]) -> Result<Vec<SourceExcerpt>, String> {
        #[cfg(test)]
        let _profile = crate::analysis::agent::profile::enter("evidence_resolve_batch");
        refs.iter()
            .map(|reference| resolve_one(self.input, reference, &self.digest))
            .collect()
    }
}

fn slice(text: &str, start: usize, end: usize) -> Result<&str, String> {
    if start >= end {
        return Err("empty_range: evidence must have a nonempty byte range".into());
    }
    text.get(start..end).ok_or_else(|| {
        "invalid_range: evidence offsets must be in bounds and on UTF-8 boundaries".into()
    })
}

pub(crate) fn table_definition<'a>(input: &'a FrozenInput, id: &str) -> Result<&'a Value, String> {
    let mut forms = input
        .structured_forms
        .iter()
        .filter(|form| form["form_definition_revision_id"].as_str() == Some(id));
    let form = forms.next().ok_or_else(|| format!("unknown_table: {id}"))?;
    if forms.next().is_some() {
        return Err(format!("duplicate_table: {id}"));
    }
    Ok(&form["definition"])
}

pub(crate) fn grid_cell(definition: &Value, row: usize, column: usize) -> Result<&Value, String> {
    let rows = definition["row_count"]
        .as_u64()
        .ok_or("invalid_grid: row_count is required")? as usize;
    let columns = definition["column_count"]
        .as_u64()
        .ok_or("invalid_grid: column_count is required")? as usize;
    if row >= rows || column >= columns {
        return Err("invalid_anchor: cell is outside the grid".into());
    }
    let cells = definition["cells"]
        .as_array()
        .ok_or("invalid_grid: cells are required")?;
    let mut anchor = None;
    for cell in cells {
        let r = cell["row"]
            .as_u64()
            .ok_or("invalid_grid: cell row is required")? as usize;
        let c = cell["column"]
            .as_u64()
            .ok_or("invalid_grid: cell column is required")? as usize;
        let rs = cell["row_span"].as_u64().unwrap_or(1) as usize;
        let cs = cell
            .get("column_span")
            .or_else(|| cell.get("col_span"))
            .and_then(Value::as_u64)
            .unwrap_or(1) as usize;
        if rs == 0
            || cs == 0
            || r.checked_add(rs).is_none_or(|end| end > rows)
            || c.checked_add(cs).is_none_or(|end| end > columns)
        {
            return Err("invalid_grid: merged span is out of bounds".into());
        }
        if row >= r && row < r + rs && column >= c && column < c + cs {
            if row != r || column != c {
                return Err("covered_cell: cite the merged cell anchor".into());
            }
            if anchor.replace(cell).is_some() {
                return Err("invalid_grid: overlapping cell anchors".into());
            }
        }
    }
    anchor.ok_or_else(|| "invalid_anchor: grid cell does not exist".into())
}

fn resolve_one(
    input: &FrozenInput,
    reference: &EvidenceRef,
    digest: &str,
) -> Result<SourceExcerpt, String> {
    if reference.digest() != digest {
        return Err("digest_mismatch: evidence belongs to another frozen input".into());
    }
    let (quote, locator, completeness) = match reference {
        EvidenceRef::Text {
            unit_id,
            start_byte,
            end_byte,
            ..
        } => {
            let mut matches = input
                .source_units
                .iter()
                .filter(|source| &source.source_unit_revision_id == unit_id);
            let source = matches
                .next()
                .ok_or_else(|| format!("unknown_unit: {unit_id}"))?;
            if matches.next().is_some() {
                return Err(format!("duplicate_unit: {unit_id}"));
            }
            (
                slice(&source.text, *start_byte, *end_byte)?.to_string(),
                source.locator.clone(),
                source.locator["completeness"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string(),
            )
        }
        EvidenceRef::GridCell {
            table_id,
            anchor_row,
            anchor_column,
            start_byte,
            end_byte,
            ..
        } => {
            let definition = table_definition(input, table_id)?;
            let cell = grid_cell(definition, *anchor_row, *anchor_column)?;
            let quote = slice(
                cell["text"]
                    .as_str()
                    .ok_or("invalid_grid: cell text is required")?,
                *start_byte,
                *end_byte,
            )?
            .to_string();
            (
                quote,
                json!({"table_id":table_id,"anchor_row":anchor_row,"anchor_column":anchor_column,"row_span":cell.get("row_span").unwrap_or(&json!(1)),"column_span":cell.get("column_span").or_else(||cell.get("col_span")).unwrap_or(&json!(1)),"physical_locator":definition["physical_locator"]}),
                definition["completeness"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string(),
            )
        }
        EvidenceRef::ImageRegion {
            image_id, region, ..
        } => {
            let source = input
                .source_units
                .iter()
                .find(|source| &source.source_unit_revision_id == image_id)
                .ok_or_else(|| format!("unknown_image: {image_id}"))?;
            if source.locator["locator_kind"] != "image"
                || region != "original"
                || source.locator["image_available"] != true
            {
                return Err("invalid_image: original region must have a persisted image".into());
            }
            (
                String::new(),
                source.locator.clone(),
                source.locator["completeness"]
                    .as_str()
                    .unwrap_or("unknown")
                    .to_string(),
            )
        }
    };
    Ok(SourceExcerpt {
        evidence: reference.clone(),
        quote_digest: hex::encode(Sha256::digest(quote.as_bytes())),
        quote,
        locator,
        completeness,
    })
}

pub fn evidence_schema() -> Value {
    let range = json!({"type":"integer","minimum":0});
    let identity = json!({"type":"string","minLength":1});
    json!({"oneOf":[
        {"type":"object","additionalProperties":false,"required":["kind","input_digest","unit_id","start_byte","end_byte"],"properties":{"kind":{"const":"text"},"input_digest":identity,"unit_id":identity,"start_byte":range,"end_byte":range}},
        {"type":"object","additionalProperties":false,"required":["kind","input_digest","table_id","anchor_row","anchor_column","start_byte","end_byte"],"properties":{"kind":{"const":"grid_cell"},"input_digest":identity,"table_id":identity,"anchor_row":range,"anchor_column":range,"start_byte":range,"end_byte":range}},
        {"type":"object","additionalProperties":false,"required":["kind","input_digest","image_id","region"],"properties":{"kind":{"const":"image_region"},"input_digest":identity,"image_id":identity,"region":{"const":"original"}}}
    ]})
}

/// Remaining ordered ranges after a prefix page; a split UTF-8 carrier keeps
/// its exact end position rather than silently skipping the rest of the unit.
pub(crate) fn selection_tail(refs: &[EvidenceRef], selected: &[EvidenceRef]) -> Vec<EvidenceRef> {
    let mut tail = refs[selected.len()..].to_vec();
    let original = &refs[selected.len() - 1];
    let last = selected.last().expect("nonempty selected page");
    if let (Some((_, end)), Some((_, full_end))) = (last.range(), original.range())
        && end < full_end
    {
        let mut rest = original.clone();
        match &mut rest {
            EvidenceRef::Text { start_byte, .. } | EvidenceRef::GridCell { start_byte, .. } => {
                *start_byte = end
            }
            _ => {}
        }
        tail.insert(0, rest);
    }
    tail
}

/// Shared range reduction for ordinary evidence reads and worker dependencies.
/// The caller tests the complete serialized next request, never a byte quota.
pub(crate) fn shrink_selection(
    input: &FrozenInput,
    selected: &mut Vec<EvidenceRef>,
) -> Result<(), String> {
    if selected.len() > 1 {
        selected.truncate(selected.len().div_ceil(2));
        return Ok(());
    }
    let quote = resolve_evidence(input, selected)?.remove(0).quote;
    let boundaries = quote
        .char_indices()
        .map(|(i, _)| i)
        .filter(|i| *i > 0)
        .collect::<Vec<_>>();
    if boundaries.is_empty() {
        return Err("remaining request token budget cannot fit source metadata plus one UTF-8 character; finish the current comparison or release redundant history, then retry the same cursor".into());
    }
    let cut = boundaries[boundaries.len() / 2];
    match &mut selected[0] {
        EvidenceRef::Text {
            start_byte,
            end_byte,
            ..
        }
        | EvidenceRef::GridCell {
            start_byte,
            end_byte,
            ..
        } => *end_byte = *start_byte + cut,
        _ => return Err("image evidence requires original view delivery".into()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn borrowed_resolver_keeps_input_and_range_validation() {
        let input = crate::outline::frozen::tests::related_native_input();
        let source = input
            .source_units
            .iter()
            .find(|source| !source.text.is_empty())
            .unwrap();
        let resolver = EvidenceResolver::new(&input).unwrap();
        let reference = EvidenceRef::Text {
            input_digest: resolver.digest().to_owned(),
            unit_id: source.source_unit_revision_id.clone(),
            start_byte: 0,
            end_byte: source.text.len(),
        };
        let expected = resolve_evidence(&input, std::slice::from_ref(&reference)).unwrap();
        for _ in 0..2 {
            assert_eq!(
                resolver.resolve(std::slice::from_ref(&reference)).unwrap(),
                expected
            );
        }
        let mut invalid = reference.clone();
        if let EvidenceRef::Text { end_byte, .. } = &mut invalid {
            *end_byte = source.text.len() + 1;
        }
        assert!(
            resolver
                .resolve(&[invalid])
                .unwrap_err()
                .contains("invalid_range")
        );
        let mut changed = input.clone();
        changed.source_units[0].text.push_str("changed");
        assert!(
            EvidenceResolver::new(&changed)
                .unwrap()
                .resolve(&[reference])
                .unwrap_err()
                .contains("digest_mismatch")
        );
    }

    fn span(start: usize, end: usize) -> EvidenceRef {
        EvidenceRef::Text {
            input_digest: "d".into(),
            unit_id: "u".into(),
            start_byte: start,
            end_byte: end,
        }
    }
    #[test]
    fn scope_uses_all_intervals_without_bridging_gaps() {
        assert!(covered_by_union(
            &span(2, 10),
            &[span(7, 10), span(0, 4), span(4, 7)]
        ));
        assert!(!covered_by_union(&span(2, 10), &[span(0, 4), span(5, 10)]));
        assert!(!covered_by_union(&span(2, 2), &[span(0, 4)]));
    }
    #[test]
    fn offsets_reject_chinese_and_emoji_interior_bytes() {
        let text = "甲😀乙";
        assert_eq!(slice(text, 3, 7).unwrap(), "😀");
        assert!(slice(text, 1, 3).is_err());
        assert!(slice(text, 3, 6).is_err());
        assert!(slice(text, 0, 0).is_err());
    }
    #[test]
    fn covered_merged_cell_is_not_an_anchor() {
        let grid = json!({"row_count":1,"column_count":2,"cells":[{"row":0,"column":0,"row_span":1,"col_span":2,"text":"固定条件"}]});
        assert!(grid_cell(&grid, 0, 0).is_ok());
        assert!(grid_cell(&grid, 0, 1).unwrap_err().contains("covered_cell"));
    }
}
