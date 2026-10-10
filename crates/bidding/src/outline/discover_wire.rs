//! Presentation-only Discover projection. Canonical packs, keys and evidence
//! stay on the host; source text and table cell payloads are never shortened.
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(super) fn compact(payload: &mut Value) -> Result<(), String> {
    let Some(sessions) = payload
        .get_mut("reading_packs")
        .and_then(Value::as_array_mut)
    else {
        return Ok(());
    };
    let mut sections: BTreeMap<String, String> = BTreeMap::new();
    let mut dictionary = Vec::new();
    for session in sessions {
        let Some(atoms) = session["pack"]
            .get_mut("atoms")
            .and_then(Value::as_array_mut)
        else {
            continue;
        };
        for atom in atoms {
            // Original-image routing/delivery remains byte-for-byte unchanged.
            if atom["carrier"]["kind"] == "image" {
                continue;
            }
            let fields = atom
                .as_object_mut()
                .ok_or("reading atom must be an object")?;
            let atom_heading = fields.get("heading_path").cloned();
            if let Some(heading) = fields.remove("heading_path") {
                let signature = heading.to_string();
                let next = format!("s{}", sections.len());
                let id = sections
                    .entry(signature)
                    .or_insert_with(|| {
                        dictionary.push(json!({"section_key":next,"heading_path":heading}));
                        next
                    })
                    .clone();
                fields.insert("section_key".into(), json!(id));
            }
            // These are host identity fields, not the model's atom_key input.
            fields.remove("id");
            fields.remove("section_id");
            for key in ["previous_fragment_id", "next_fragment_id"] {
                if let Some(link) = fields.get_mut(key) {
                    // Keep exact fragment identity within a document, without
                    // repeating its document digest/long source prefix.
                    if let Some(text) = link.as_str()
                        && let Some((_, suffix)) = text.rsplit_once(":section:")
                    {
                        *link = json!(format!("section:{suffix}"));
                    }
                }
            }
            if let Some(excerpts) = fields.get_mut("excerpts").and_then(Value::as_array_mut) {
                for excerpt in excerpts {
                    if let Some(excerpt) = excerpt.as_object_mut() {
                        excerpt.remove("quote_digest");
                        if let Some(locator) =
                            excerpt.get_mut("locator").and_then(Value::as_object_mut)
                        {
                            for field in [
                                "document_revision",
                                "parser_version",
                                "unit_id",
                                "section_id",
                                "parent_section_id",
                                "rendered_spans",
                                "physical_path",
                            ] {
                                locator.remove(field);
                            }
                            if locator.get("heading_path") == atom_heading.as_ref() {
                                locator.remove("heading_path");
                            }
                            // The same physical coordinates remain at the top
                            // level; retain any differing nested location.
                            if let Some(physical) =
                                locator.get("physical_locator").and_then(Value::as_object)
                                && physical.iter().all(|(k, v)| locator.get(k) == Some(v))
                            {
                                locator.remove("physical_locator");
                            }
                        }
                    }
                }
                if fields.get("carrier").is_some_and(|v| v["kind"] == "text") {
                    // The exact source_key is still present beside its quote.
                    if let Some(carrier) = fields.get_mut("carrier").and_then(Value::as_object_mut)
                    {
                        carrier.remove("evidence");
                    }
                }
            }
        }
    }
    if !dictionary.is_empty() {
        payload["section_dictionary"] = json!(dictionary);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn projection_preserves_source_grid_condition_and_original_image_fields() {
        let text = json!({"id":"host-long-id","section_id":"host-section","heading_path":"Section > Detail","atom_key":"atom-issued","context_only":false,"carrier":{"kind":"text","evidence":{"source_key":"src-issued"}},"excerpts":[{"quote":"Original 😀 wording\r\n","quote_digest":"host-digest","evidence_key":"ev-issued","evidence":{"source_key":"src-issued"},"locator":{"page_ordinal":3,"document_revision":"host-revision","parser_version":"host-parser","heading_path":"Section > Detail","left":0.1}}]});
        let image = json!({"id":"image-route","carrier":{"kind":"image","evidence":{"source_key":"image-source"}},"visual_evidence_delivered":true});
        let grid = json!({"id":"grid-host","heading_path":"Section > Detail","atom_key":"grid-atom","carrier":{"kind":"grid","table_id":"table"},"cells":[{"text":"fixed cell","anchor_row":2,"anchor_column":1,"evidence_key":"grid-ev"}],"blank_range_refs":["3:4,0:1"]});
        let mut value = json!({"reading_packs":[{"pack":{"condition_support_options":[{"support_key":"cs-issued","excerpts":[{"quote":"conditional source"}]}],"atoms":[text,grid,image.clone()]}}],"table_structures":[{"table_id":"table","anchors":{"0,0":{"header_fragments":{"0,6":"Header"}}}}]});
        let before = value.clone();
        compact(&mut value).unwrap();
        assert_eq!(
            value["reading_packs"][0]["pack"]["atoms"][0]["excerpts"][0]["quote"],
            before["reading_packs"][0]["pack"]["atoms"][0]["excerpts"][0]["quote"]
        );
        assert_eq!(
            value["reading_packs"][0]["pack"]["atoms"][1]["cells"],
            before["reading_packs"][0]["pack"]["atoms"][1]["cells"]
        );
        assert_eq!(value["reading_packs"][0]["pack"]["atoms"][2], image);
        assert_eq!(value["table_structures"], before["table_structures"]);
        assert_eq!(
            value["reading_packs"][0]["pack"]["condition_support_options"],
            before["reading_packs"][0]["pack"]["condition_support_options"]
        );
        assert_eq!(value["section_dictionary"].as_array().unwrap().len(), 1);
        let once = value.clone();
        compact(&mut value).unwrap();
        assert_eq!(value, once);
        assert!(value.to_string().len() < before.to_string().len());
    }
}
