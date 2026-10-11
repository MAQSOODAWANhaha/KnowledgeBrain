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
            let owned = fields.get("context_only") == Some(&json!(false));
            if owned && let Some(atom_key) = fields.get("atom_key").cloned() {
                // A directly copyable field shared by this atom's excerpts and
                // cells. Reuse the existing handle, not another numbering scheme.
                fields.insert("source_section_id".into(), json!({"atom_key":atom_key}));
            }
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
                        if owned
                            && excerpt
                                .get("evidence")
                                .and_then(Value::as_object)
                                .is_some_and(|reference| {
                                    reference.len() == 1
                                        && reference.get("source_key").is_some_and(Value::is_string)
                                })
                        {
                            // Keep the submit evidence_key beside its original
                            // quote. The s-handle is explicitly a reading target.
                            let reference = excerpt.remove("evidence").unwrap();
                            if excerpt
                                .get("read_reference")
                                .is_some_and(|prior| prior != &reference)
                            {
                                return Err("conflicting excerpt read reference".into());
                            }
                            excerpt.insert("read_reference".into(), reference);
                        }
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
    fn canonical_session_pipeline_emits_distinct_read_and_submit_references() {
        use crate::outline::{discover, evidence, model_wire, source_wire};
        let input = crate::outline::tests::input();
        let before = json!(input);
        let mut work = discover::DiscoverWork::plan(&input, 32768);
        let pack = work.claim(1).remove(0);
        let identity = work.pack_wire_identity(&pack.id).unwrap();
        let mut session = work.session(&input, &pack.id).unwrap();
        discover::project_owned_pack_headers(&mut session);
        let original = session["pack"]["atoms"][0]["excerpts"][0].clone();
        let canonical_reference: evidence::EvidenceRef =
            serde_json::from_value(original["evidence"].clone()).unwrap();
        let mut body = json!({"messages":[{"role":"system","content":"Synthetic Discover pipeline"},
            {"role":"user","content":json!({"reading_packs":[session]}).to_string()}],"tools":[]});
        // Match PackHost::prepare_candidate order, starting with canonical
        // EvidenceRef rather than hand-built source_key aliases.
        discover::compact_request(&mut body).unwrap();
        let after_compact: Value =
            serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
        assert_eq!(
            after_compact["reading_packs"][0]["pack"]["atoms"][0]["excerpts"][0]["evidence"],
            json!(canonical_reference)
        );
        let keys = source_wire::request_in_scope(
            &json!({"input":evidence::input_digest(&input).unwrap(),"worker":identity}),
            &BTreeMap::new(),
            &mut body,
        )
        .unwrap();
        let registry = model_wire::project_claim_scope(
            &model_wire::Registry::default(),
            &identity,
            &json!([identity, 1]),
            &mut body,
        )
        .unwrap();
        let wire: Value =
            serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
        let atom = &wire["reading_packs"][0]["pack"]["atoms"][0];
        let excerpt = &atom["excerpts"][0];
        assert!(excerpt.get("evidence").is_none());
        assert_eq!(excerpt["quote"], original["quote"]);
        assert!(
            excerpt["read_reference"]["source_key"]
                .as_str()
                .unwrap()
                .starts_with('s')
        );
        assert!(excerpt["evidence_key"].as_str().unwrap().starts_with('e'));
        assert_eq!(atom["source_section_id"]["atom_key"], atom["atom_key"]);
        let read = registry
            .decode(
                json!({"wire_scope":wire["wire_scope"],
            "refs":[excerpt["read_reference"]]}),
                false,
            )
            .unwrap();
        assert_eq!(
            keys.get(read["refs"][0]["source_key"].as_str().unwrap()),
            Some(&canonical_reference)
        );
        let submit = registry
            .decode(
                json!({"wire_scope":wire["wire_scope"],
            "source_section_id":atom["source_section_id"],
            "evidence":[{"evidence_key":excerpt["evidence_key"]}]}),
                false,
            )
            .unwrap();
        assert_eq!(
            submit["evidence"][0]["evidence_key"],
            original["evidence_key"]
        );
        assert_eq!(json!(input), before);
    }

    #[test]
    fn owned_submission_references_share_existing_handles_without_navigation_aliases() {
        let quote = "Synthetic original 😀 wording\r\n";
        let text = json!({"atom_key":"owned-atom","context_only":false,
            "carrier":{"kind":"text"},"excerpts":[{"evidence_key":"owned-evidence",
                "evidence":{"source_key":"owned-source"},"quote":quote,
                "locator":{"page_ordinal":3}}]});
        let grid = json!({"atom_key":"grid-atom","context_only":false,"carrier":{"kind":"grid"},
            "cells":[{"anchor_row":0,"anchor_column":0,"text":"Owned header","evidence_key":"header-evidence"},
                {"anchor_row":1,"anchor_column":0,"text":"Owned body","evidence_key":"cell-evidence"}]});
        let context = json!({"atom_key":"context-atom","context_only":true,
            "carrier":{"kind":"text"},"excerpts":[{"evidence_key":"context-evidence",
                "evidence":{"source_key":"context-source"},"quote":"Supporting context"}]});
        let original = json!({"reading_packs":[{"pack":{"atoms":[text,grid.clone(),context]}}]});
        let mut projected = original.clone();
        compact(&mut projected).unwrap();
        let atoms = &projected["reading_packs"][0]["pack"]["atoms"];
        assert_eq!(
            atoms[0]["source_section_id"],
            json!({"atom_key":"owned-atom"})
        );
        assert_eq!(atoms[0]["excerpts"][0]["quote"], quote);
        assert_eq!(
            atoms[0]["excerpts"][0]["locator"],
            json!({"page_ordinal":3})
        );
        assert_eq!(atoms[0]["excerpts"][0]["evidence_key"], "owned-evidence");
        assert_eq!(
            atoms[0]["excerpts"][0]["read_reference"],
            json!({"source_key":"owned-source"})
        );
        assert!(atoms[0]["excerpts"][0].get("evidence").is_none());
        assert_eq!(atoms[1]["cells"], grid["cells"]);
        assert_eq!(
            atoms[1]["source_section_id"],
            json!({"atom_key":"grid-atom"})
        );
        assert!(atoms[2].get("source_section_id").is_none());
        assert_eq!(
            atoms[2]["excerpts"][0]["evidence"],
            json!({"source_key":"context-source"})
        );
        let once = projected.clone();
        compact(&mut projected).unwrap();
        assert_eq!(projected, once);

        // The added relationship reuses existing typed identities: one source,
        // evidence and atom handle, never a guessed prefix conversion.
        let make_body = |payload: &Value| json!({"messages":[{"role":"user","content":payload.to_string()}],"tools":[]});
        let mut wire = make_body(&projected);
        let registry = crate::outline::model_wire::project_scope(
            &crate::outline::model_wire::Registry::default(),
            &json!(1),
            &mut wire,
        )
        .unwrap();
        let packet: Value =
            serde_json::from_str(wire["messages"][0]["content"].as_str().unwrap()).unwrap();
        let atom = &packet["reading_packs"][0]["pack"]["atoms"][0];
        assert_eq!(atom["source_section_id"]["atom_key"], atom["atom_key"]);
        let evidence = &atom["excerpts"][0];
        let args = json!({"wire_scope":packet["wire_scope"],"source_section_id":atom["source_section_id"],
            "evidence":[{"evidence_key":evidence["evidence_key"]}]});
        let decoded = registry.decode(args.clone(), false).unwrap();
        assert_eq!(
            decoded["source_section_id"],
            json!({"atom_key":"owned-atom"})
        );
        assert_eq!(
            decoded["evidence"],
            json!([{"evidence_key":"owned-evidence"}])
        );
        let source = evidence["read_reference"]["source_key"].as_str().unwrap();
        let mut confused = args.clone();
        confused["evidence"][0]["evidence_key"] = json!(format!("e{}", &source[1..]));
        assert!(registry.decode(confused.clone(), false).is_err());
        confused["evidence"][0]["evidence_key"] = json!(source);
        assert!(registry.decode(confused.clone(), false).is_err());
        confused["evidence"][0]["evidence_key"] = json!("owned-evidence");
        assert!(registry.decode(confused, false).is_err());
        let next = crate::outline::model_wire::project_scope(
            &registry,
            &json!(2),
            &mut make_body(&projected),
        )
        .unwrap();
        assert!(next.decode(args, false).is_err());
        assert_eq!(
            original["reading_packs"][0]["pack"]["atoms"][0]["excerpts"][0]["evidence"],
            json!({"source_key":"owned-source"})
        );
    }

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
