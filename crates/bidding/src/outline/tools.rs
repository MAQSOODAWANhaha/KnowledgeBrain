//! The outline model writes chapters, attachment bindings, and template slots.
//!
//! Six tools, and a turn is shown only the ones its duty allows.

use super::{
    ChapterOutline, ChapterPurpose, SlotKind, TemplateContent,
    chapters::{AttachmentBinding, attachment_form_ids},
};
use crate::analysis::FrozenInput;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashSet};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chapters: Vec<ChapterOutline>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<AttachmentBinding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slots: Vec<TemplateContent>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub slots_submitted: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub finished: bool,
}

impl Draft {
    pub fn is_empty(&self) -> bool {
        self.chapters.is_empty()
            && self.bindings.is_empty()
            && self.slots.is_empty()
            && !self.slots_submitted
            && !self.finished
    }
}

/// State the outline model reads. It names no retired tools.
pub fn model_state(input: &FrozenInput, draft: &Draft) -> Value {
    let mut state = view(draft);
    state["unmapped_forms"] = json!(unmapped_forms(input, draft));
    state
}

pub fn unmapped_forms(input: &FrozenInput, draft: &Draft) -> Vec<String> {
    let mapped: BTreeSet<_> = draft
        .bindings
        .iter()
        .map(|binding| binding.form_id.as_str())
        .collect();
    attachment_form_ids(input)
        .into_iter()
        .filter(|id| !mapped.contains(id.as_str()))
        .collect()
}

pub fn apply(
    input: &FrozenInput,
    draft: &mut Draft,
    requirement_ids: &BTreeSet<String>,
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    match name {
        "put_chapters" => put_chapters(draft, requirement_ids, args),
        "bind_forms" => bind_forms(input, draft, args),
        "put_slots" => put_slots(draft, args),
        "read_outline" => Ok(view(draft)),
        "finish_outline" => finish(input, draft),
        _ => Err("unknown outline tool".into()),
    }
}

fn put_chapters(
    draft: &mut Draft,
    known_requirements: &BTreeSet<String>,
    args: &Value,
) -> Result<Value, String> {
    let rows = args["chapters"]
        .as_array()
        .ok_or("chapters must be an array")?;
    if rows.is_empty() {
        return Err("chapters must name at least one chapter".into());
    }
    let mut chapters = Vec::with_capacity(rows.len());
    let mut seen = HashSet::new();
    let mut seen_requirements = HashSet::new();
    for chapter in rows {
        let id = required(chapter, "id")?;
        if !seen.insert(id.clone()) {
            return Err(format!("duplicate chapter {id}"));
        }
        let parent_id = match &chapter["parent_id"] {
            Value::Null => None,
            Value::String(parent) if !parent.is_empty() => Some(parent.clone()),
            _ => return Err(format!("chapter {id} parent_id must be an id or null")),
        };
        let purpose = match chapter["purpose"].as_str() {
            Some("group") => ChapterPurpose::Group,
            Some("response") => ChapterPurpose::Response,
            _ => return Err(format!("chapter {id} purpose must be group or response")),
        };
        let requirement_ids =
            requirement_ids(chapter, &id, known_requirements, &mut seen_requirements)?;
        chapters.push(ChapterOutline {
            id,
            parent_id,
            order: chapter["order"]
                .as_u64()
                .ok_or("chapter order must be a non-negative integer")? as usize,
            title: required(chapter, "title")?,
            purpose,
            requirement_ids,
        });
    }
    if let Some(id) = known_requirements
        .iter()
        .find(|id| !seen_requirements.contains(id.as_str()))
    {
        return Err(format!("requirement {id} is not on a chapter"));
    }
    validate_tree(&chapters)?;
    let ids: HashSet<_> = chapters.iter().map(|chapter| chapter.id.as_str()).collect();
    let slots_before = draft.slots.len();
    draft
        .bindings
        .retain(|binding| ids.contains(binding.chapter_id.as_str()));
    draft
        .slots
        .retain(|slot| ids.contains(slot.chapter_id.as_str()));
    if draft.slots.len() != slots_before {
        draft.slots_submitted = false;
    }
    draft.chapters = chapters;
    draft.finished = false;
    Ok(view(draft))
}

fn bind_forms(input: &FrozenInput, draft: &mut Draft, args: &Value) -> Result<Value, String> {
    let rows = args["bindings"]
        .as_array()
        .ok_or("bindings must be an array")?;
    let attachments: HashSet<_> = attachment_form_ids(input).into_iter().collect();
    let chapters: HashSet<_> = draft
        .chapters
        .iter()
        .map(|chapter| chapter.id.as_str())
        .collect();
    let mut bindings = Vec::with_capacity(rows.len());
    let mut seen = HashSet::new();
    for binding in rows {
        let form_id = required(binding, "form_id")?;
        let chapter_id = required(binding, "chapter_id")?;
        if !attachments.contains(&form_id) {
            return Err(format!("form {form_id} is not an attachment table"));
        }
        if !chapters.contains(chapter_id.as_str()) {
            return Err(format!("chapter {chapter_id} is not in the outline"));
        }
        if !seen.insert(form_id.clone()) {
            return Err(format!(
                "attachment table {form_id} is bound more than once"
            ));
        }
        bindings.push(AttachmentBinding {
            form_id,
            chapter_id,
        });
    }
    draft.bindings = bindings;
    draft.finished = false;
    Ok(view(draft))
}

fn put_slots(draft: &mut Draft, args: &Value) -> Result<Value, String> {
    let rows = args["slots"].as_array().ok_or("slots must be an array")?;
    let chapters: HashSet<_> = draft
        .chapters
        .iter()
        .map(|chapter| chapter.id.as_str())
        .collect();
    let groups: HashSet<_> = draft
        .chapters
        .iter()
        .filter(|chapter| chapter.purpose == ChapterPurpose::Group)
        .map(|chapter| chapter.id.as_str())
        .collect();
    let mut slots = Vec::with_capacity(rows.len());
    let mut seen = HashSet::new();
    for slot in rows {
        let slot_id = required(slot, "slot_id")?;
        let chapter_id = required(slot, "chapter_id")?;
        if !seen.insert(slot_id.clone()) {
            return Err(format!("duplicate slot {slot_id}"));
        }
        if !chapters.contains(chapter_id.as_str()) {
            return Err(format!("slot {slot_id} is not on a chapter"));
        }
        let kind = match slot["kind"].as_str() {
            Some("fixed_text") => SlotKind::FixedText,
            Some("tender_value") => SlotKind::TenderValue,
            Some("instruction") => SlotKind::Instruction,
            Some("bidder_blank") => SlotKind::BidderBlank,
            Some("signature") => SlotKind::Signature,
            Some("preserved") => SlotKind::Preserved,
            _ => return Err(format!("slot {slot_id} kind is not recognized")),
        };
        let text = slot["text"].as_str().unwrap_or("");
        let match_query = slot["match_query"].as_str().unwrap_or("");
        if kind.accepts_knowledge_response() {
            if groups.contains(chapter_id.as_str()) {
                return Err(format!(
                    "group chapter {chapter_id} cannot carry a knowledge response"
                ));
            }
            if !text.is_empty() || match_query.trim().is_empty() {
                return Err(format!(
                    "slot {slot_id} stays empty and needs a match query"
                ));
            }
        } else if !match_query.is_empty() {
            return Err(format!("slot {slot_id} cannot carry a knowledge query"));
        }
        slots.push(TemplateContent {
            slot_id,
            chapter_id,
            kind,
            text: text.to_string(),
            response_required: kind.accepts_knowledge_response(),
            match_query: match_query.to_string(),
        });
    }
    draft.slots = slots;
    draft.slots_submitted = true;
    draft.finished = false;
    Ok(view(draft))
}

fn finish(input: &FrozenInput, draft: &mut Draft) -> Result<Value, String> {
    if draft.chapters.is_empty() {
        return Err("put_chapters before finishing the outline".into());
    }
    validate_tree(&draft.chapters)?;
    if let Some(form_id) = unmapped_forms(input, draft).into_iter().next() {
        return Err(format!(
            "attachment table {form_id} is not mapped to a chapter"
        ));
    }
    if !draft.slots_submitted {
        return Err("put_slots before finishing the outline".into());
    }
    if let Some(id) = missing_response_slot(draft) {
        return Err(format!("response chapter {id} has no template slot"));
    }
    draft.finished = true;
    Ok(view(draft))
}

/// First response chapter, in chapter order, that has no template slot.
pub fn missing_response_slot(draft: &Draft) -> Option<&str> {
    draft.chapters.iter().find_map(|chapter| {
        (chapter.purpose == ChapterPurpose::Response
            && !draft.slots.iter().any(|slot| slot.chapter_id == chapter.id))
        .then_some(chapter.id.as_str())
    })
}

fn view(draft: &Draft) -> Value {
    json!({
        "chapters": draft.chapters.iter().map(|chapter| json!({
            "id": chapter.id,
            "parent_id": chapter.parent_id,
            "order": chapter.order,
            "title": chapter.title,
            "purpose": chapter.purpose,
            "requirement_ids": chapter.requirement_ids,
        })).collect::<Vec<_>>(),
        "bindings": draft.bindings,
        "slots": draft.slots.iter().map(|slot| json!({
            "slot_id": slot.slot_id,
            "chapter_id": slot.chapter_id,
            "kind": slot.kind,
            "text": slot.text,
            "match_query": slot.match_query,
        })).collect::<Vec<_>>(),
        "slots_submitted": draft.slots_submitted,
        "finished": draft.finished,
    })
}

fn validate_tree(chapters: &[ChapterOutline]) -> Result<(), String> {
    let ids: HashSet<_> = chapters.iter().map(|chapter| chapter.id.as_str()).collect();
    let mut orders = HashSet::new();
    for chapter in chapters {
        if chapter.id.is_empty() || chapter.title.trim().is_empty() {
            return Err("chapter id and title are required".into());
        }
        if let Some(parent) = chapter.parent_id.as_deref()
            && !ids.contains(parent)
        {
            return Err(format!("chapter {} parent is missing", chapter.id));
        }
        if !orders.insert((chapter.parent_id.as_deref(), chapter.order)) {
            return Err(format!("chapter {} repeats a sibling order", chapter.id));
        }
    }
    for chapter in chapters {
        let mut seen = HashSet::from([chapter.id.as_str()]);
        let mut parent = chapter.parent_id.as_deref();
        while let Some(id) = parent {
            if !seen.insert(id) {
                return Err("chapter parent cycle".into());
            }
            parent = chapters
                .iter()
                .find(|candidate| candidate.id == id)
                .and_then(|candidate| candidate.parent_id.as_deref());
        }
    }
    Ok(())
}

fn requirement_ids(
    chapter: &Value,
    id: &str,
    known: &BTreeSet<String>,
    seen: &mut HashSet<String>,
) -> Result<Vec<String>, String> {
    let Some(rows) = chapter.get("requirement_ids").and_then(Value::as_array) else {
        return Err(format!("chapter {id} requirement_ids must be an array"));
    };
    let mut ids = Vec::with_capacity(rows.len());
    for requirement in rows {
        let requirement = requirement
            .as_str()
            .filter(|requirement| !requirement.is_empty())
            .ok_or_else(|| format!("chapter {id} requirement_ids must be strings"))?;
        if !known.contains(requirement) {
            return Err(format!("unknown requirement {requirement}"));
        }
        if !seen.insert(requirement.to_string()) {
            return Err(format!(
                "requirement {requirement} is assigned more than once"
            ));
        }
        ids.push(requirement.to_string());
    }
    Ok(ids)
}

fn required(value: &Value, field: &str) -> Result<String, String> {
    value[field]
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("{field} is required"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::Source;

    fn input() -> FrozenInput {
        FrozenInput {
            schema_version: 1,
            project_id: "project".into(),
            document_set_id: "set".into(),
            documents: vec![],
            document_relations: vec![],
            source_units: vec![Source {
                source_unit_revision_id: "source".into(),
                document_id: "doc".into(),
                text: "附件".into(),
                locator: json!({"heading_path": "附件"}),
                ordinal: 0,
            }],
            structured_forms: vec![json!({
                "form_definition_revision_id": "form-1",
                "source_unit_revision_id": "source",
                "definition": {
                    "title": "source_unit:form-1",
                    "row_count": 2,
                    "column_count": 2,
                    "cells": [
                        {"row": 0, "column": 0, "text": "A"},
                        {"row": 0, "column": 1, "text": ""},
                        {"row": 1, "column": 0, "text": ""},
                        {"row": 1, "column": 1, "text": ""}
                    ]
                }
            })],
            decisions: vec![],
        }
    }

    #[test]
    fn chapters_bindings_and_slots_persist_until_finish() {
        let input = input();
        let mut draft = Draft::default();
        apply(
            &input,
            &mut draft,
            &BTreeSet::new(),
            "put_chapters",
            &json!({"chapters":[
                {"id":"group","parent_id":null,"order":0,"title":"投标文件","purpose":"group","requirement_ids":[]},
                {"id":"letter","parent_id":"group","order":0,"title":"投标函","purpose":"response","requirement_ids":[]}
            ]}),
        )
        .unwrap();
        assert_eq!(draft.chapters.len(), 2);
        apply(
            &input,
            &mut draft,
            &BTreeSet::new(),
            "bind_forms",
            &json!({"bindings":[{"form_id":"form-1","chapter_id":"letter"}]}),
        )
        .unwrap();
        assert!(
            finish(&input, &mut draft)
                .unwrap_err()
                .contains("put_slots")
        );
        apply(
            &input,
            &mut draft,
            &BTreeSet::new(),
            "put_slots",
            &json!({"slots":[
                {"slot_id":"letter:fixed","chapter_id":"letter","kind":"fixed_text","text":"投标函","match_query":""},
                {"slot_id":"letter:bidder","chapter_id":"letter","kind":"bidder_blank","text":"","match_query":"投标人名称"}
            ]}),
        )
        .unwrap();
        let err = apply(
            &input,
            &mut draft,
            &BTreeSet::new(),
            "put_slots",
            &json!({"slots":[{"slot_id":"group:bidder","chapter_id":"group","kind":"bidder_blank","text":"","match_query":"名称"}]}),
        )
        .unwrap_err();
        assert!(err.contains("group chapter"));
        let finished = finish(&input, &mut draft).unwrap();
        assert_eq!(finished["finished"], json!(true));
        assert_eq!(finished["bindings"][0]["form_id"], json!("form-1"));
        assert_eq!(draft.slots.len(), 2);
        let again = apply(
            &input,
            &mut draft,
            &BTreeSet::new(),
            "read_outline",
            &json!({}),
        )
        .unwrap();
        assert_eq!(again["slots"][1]["match_query"], json!("投标人名称"));
    }
}
