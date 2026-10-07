//! Stable chapter identity and attachment-table mapping.
//!
//! A chapter id is the identity. Titles may be renamed without moving an
//! attachment table. Each attachment form binds to exactly one chapter through
//! a template `form_id` or a format-reference grid cell.

use crate::analysis::draft::{DraftPlanItem, DraftStatus};
use crate::analysis::{FrozenInput, Record, RecordData};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttachmentBinding {
    pub form_id: String,
    pub chapter_id: String,
}

/// Attachment tables in the frozen tender, in form id order.
pub fn attachment_form_ids(input: &FrozenInput) -> Vec<String> {
    let mut ids: Vec<_> = input
        .structured_forms
        .iter()
        .filter(|form| is_attachment_form(input, form))
        .filter_map(|form| form["form_definition_revision_id"].as_str())
        .map(str::to_string)
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

/// Bind every attachment table to the chapter that carries its form id.
pub fn map_attachment_tables(
    input: &FrozenInput,
    plan: &[DraftPlanItem],
    records: &BTreeMap<String, Record>,
) -> Result<Vec<AttachmentBinding>, String> {
    let mut bindings: BTreeMap<String, String> = BTreeMap::new();
    for item in plan
        .iter()
        .filter(|item| item.status != DraftStatus::Omitted)
    {
        for form_id in chapter_form_ids(item, records) {
            if !attachment_form_ids(input).contains(&form_id) {
                continue;
            }
            if let Some(existing) = bindings.insert(form_id.clone(), item.id.clone())
                && existing != item.id
            {
                return Err(format!(
                    "attachment table {form_id} is mapped to both {existing} and {}",
                    item.id
                ));
            }
        }
    }
    Ok(bindings
        .into_iter()
        .map(|(form_id, chapter_id)| AttachmentBinding {
            form_id,
            chapter_id,
        })
        .collect())
}

pub fn unmapped_attachment_forms(
    input: &FrozenInput,
    plan: &[DraftPlanItem],
    records: &BTreeMap<String, Record>,
) -> Vec<String> {
    let Ok(bindings) = map_attachment_tables(input, plan, records) else {
        return attachment_form_ids(input);
    };
    let mapped: BTreeSet<_> = bindings
        .iter()
        .map(|binding| binding.form_id.as_str())
        .collect();
    attachment_form_ids(input)
        .into_iter()
        .filter(|id| !mapped.contains(id.as_str()))
        .collect()
}

fn chapter_form_ids(item: &DraftPlanItem, records: &BTreeMap<String, Record>) -> Vec<String> {
    let mut ids: Vec<_> = item
        .format_refs
        .iter()
        .filter_map(|span| span.grid_cell.as_ref().map(|cell| cell.form_id.clone()))
        .collect();
    if let Some(template_id) = item.template_id.as_deref()
        && let Some(record) = records.get(template_id)
        && let RecordData::Template { regions, .. } = &record.data
    {
        ids.extend(regions.iter().filter_map(|region| region.form_id.clone()));
    }
    ids.sort();
    ids.dedup();
    ids
}

fn is_attachment_form(input: &FrozenInput, form: &Value) -> bool {
    let title = form["definition"]["title"].as_str().unwrap_or("");
    if title.contains('附') && title.contains("件") {
        return true;
    }
    let source_id = form["source_unit_revision_id"].as_str().unwrap_or("");
    input.source_units.iter().any(|source| {
        source.source_unit_revision_id == source_id
            && source.locator["heading_path"]
                .as_str()
                .unwrap_or("")
                .contains("附件")
    })
}
