//! Response agent. It may only fill frozen outline slots from knowledge hits.

use super::{NO_EVIDENCE_TEXT, ResponseSet, ResponseStatus, SlotResponse};
use crate::outline::{OutlineArtifact, SCHEMA_VERSION, canonical_sha256, validate_artifact};
use serde_json::{Value, json};

pub const RESPONSIBILITY: &str =
    "只按冻结大纲的响应槽位写响应；不重新解析招标文件，不新增章节，不改模板";

const ALLOWED: &[&str] = &["read_outline", "put_responses"];

pub fn deny(tool: &str) -> Option<&'static str> {
    if ALLOWED.contains(&tool) {
        None
    } else {
        Some("response generation can only read the frozen outline and write slot responses")
    }
}

pub fn schemas() -> Vec<serde_json::Value> {
    serde_json::from_str(include_str!("../../schemas/response-tools-v1.schema.json"))
        .expect("response tools")
}

pub fn apply(artifact: &OutlineArtifact, name: &str, args: &Value) -> Result<Value, String> {
    if let Some(reason) = deny(name) {
        return Err(reason.into());
    }
    validate_artifact(artifact)?;
    match name {
        "read_outline" => Ok(json!({
            "chapters": artifact.chapters.iter().map(|chapter| json!({
                "id": chapter.id,
                "parent_id": chapter.parent_id,
                "order": chapter.order,
                "title": chapter.title,
                "purpose": chapter.purpose,
            })).collect::<Vec<_>>(),
            "slots": artifact.templates.iter().filter(|slot| slot.response_required).map(|slot| json!({
                "slot_id": slot.slot_id,
                "chapter_id": slot.chapter_id,
                "kind": slot.kind,
                "match_query": slot.match_query,
            })).collect::<Vec<_>>(),
        })),
        "put_responses" => {
            let set = put_responses(artifact, args)?;
            Ok(json!(set))
        }
        _ => Err("unknown response tool".into()),
    }
}

fn put_responses(artifact: &OutlineArtifact, args: &Value) -> Result<ResponseSet, String> {
    let rows = args["responses"]
        .as_array()
        .ok_or("responses must be an array")?;
    let expected: Vec<_> = artifact
        .templates
        .iter()
        .filter(|slot| slot.response_required)
        .collect();
    let mut written = std::collections::BTreeMap::new();
    for row in rows {
        let slot_id = row["slot_id"].as_str().unwrap_or("").trim();
        if slot_id.is_empty() {
            return Err("slot_id is required".into());
        }
        if written.contains_key(slot_id) {
            return Err(format!("slot {slot_id} is repeated"));
        }
        let slot = expected
            .iter()
            .find(|slot| slot.slot_id == slot_id)
            .ok_or_else(|| format!("slot {slot_id} is not a response slot"))?;
        let status = match row["status"].as_str() {
            Some("matched") => ResponseStatus::Matched,
            Some("no_evidence") => ResponseStatus::NoEvidence,
            _ => {
                return Err(format!(
                    "slot {slot_id} status must be matched or no_evidence"
                ));
            }
        };
        let text = row["text"].as_str().unwrap_or("");
        match status {
            ResponseStatus::NoEvidence if text == NO_EVIDENCE_TEXT => {}
            ResponseStatus::Matched if !text.is_empty() && text != NO_EVIDENCE_TEXT => {}
            ResponseStatus::NoEvidence => {
                return Err(format!("slot {slot_id} must use {NO_EVIDENCE_TEXT}"));
            }
            ResponseStatus::Matched => {
                return Err(format!("slot {slot_id} needs evidence text"));
            }
        }
        written.insert(
            slot_id.to_string(),
            SlotResponse {
                slot_id: slot.slot_id.clone(),
                chapter_id: slot.chapter_id.clone(),
                status,
                text: text.to_string(),
                evidence_ids: Vec::new(),
            },
        );
    }
    if written.len() != expected.len() {
        return Err("put_responses must cover every response slot".into());
    }
    let responses = expected
        .iter()
        .map(|slot| written.remove(slot.slot_id.as_str()).expect("slot checked"))
        .collect();
    Ok(ResponseSet {
        schema_version: SCHEMA_VERSION,
        outline_sha256: canonical_sha256(artifact)?,
        responses,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_duty_has_two_tools() {
        assert!(deny("read_outline").is_none());
        assert!(deny("put_responses").is_none());
        assert!(deny("read_source").is_some());
        assert!(deny("put_chapters").is_some());
        assert!(deny("put_slots").is_some());
        assert!(deny("submit_pack").is_some());
        let names: Vec<_> = schemas()
            .iter()
            .map(|tool| tool["function"]["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(names, ["read_outline", "put_responses"]);
    }

    #[test]
    fn put_responses_covers_every_slot_or_leaves_the_placeholder() {
        use crate::outline::{
            ChapterOutline, ChapterPurpose, OutlineArtifact, SlotKind, TemplateContent,
        };
        use serde_json::json;
        let artifact = OutlineArtifact {
            schema_version: crate::outline::SCHEMA_VERSION,
            project_id: "project".into(),
            frozen_input_sha256: "ab".repeat(32),
            chapters: vec![ChapterOutline {
                id: "letter".into(),
                parent_id: None,
                order: 0,
                title: "投标函".into(),
                purpose: ChapterPurpose::Response,
                requirement_ids: vec![],
            }],
            templates: vec![TemplateContent {
                slot_id: "bidder".into(),
                chapter_id: "letter".into(),
                kind: SlotKind::BidderBlank,
                content: crate::outline::TemplateBody::EditableBlank,
                text: String::new(),
                response_required: true,
                match_query: "投标人".into(),
            }],
            open_issues: vec![],
            required_requirement_ids: Default::default(),
            requirements: Default::default(),
            fulfillments: vec![],
            review_issues: vec![],
            needs_review: false,
            semantic_review_complete: true,
        };
        let read = apply(&artifact, "read_outline", &json!({})).unwrap();
        assert_eq!(read["slots"][0]["slot_id"], json!("bidder"));
        assert!(apply(&artifact, "put_chapters", &json!({"mode":"replace",})).is_err());
        let missing = apply(
            &artifact,
            "put_responses",
            &json!({"responses":[{"slot_id":"bidder","status":"matched","text":""}]}),
        )
        .unwrap_err();
        assert!(missing.contains("evidence text"));
        let set = apply(
            &artifact,
            "put_responses",
            &json!({"responses":[{"slot_id":"bidder","status":"no_evidence","text":"【待人工补充】"}]}),
        )
        .unwrap();
        assert_eq!(set["responses"][0]["status"], json!("no_evidence"));
        assert_eq!(set["responses"][0]["text"], json!("【待人工补充】"));
    }
}
