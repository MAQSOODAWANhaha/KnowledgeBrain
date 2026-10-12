//! Model-wire source keys. Canonical evidence remains an internal type.
use super::evidence::EvidenceRef;
use crate::analysis::agent::Checkpoint;
use serde_json::{Value, json};
use std::collections::BTreeMap;
#[path = "discover_wire.rs"]
mod discover_wire;
pub type Keys = BTreeMap<String, EvidenceRef>;
fn scope(state: &Checkpoint) -> Value {
    json!({"input":state.input_sha256,"revision":state.outline_run.reading_packs.as_ref().map(|w|w.revision),"epoch":state.outline_run.tool_draft.read_epoch})
}
#[cfg(test)]
fn key(state: &Checkpoint, reference: &EvidenceRef) -> Result<String, String> {
    scoped_key(&scope(state), reference)
}
fn scoped_key(scope: &Value, reference: &EvidenceRef) -> Result<String, String> {
    Ok(format!(
        "src_{}",
        super::canonical_sha256(&(scope, reference))?
    ))
}
fn project(scope: &Value, value: &mut Value, keys: &mut Keys) -> Result<(), String> {
    if let Ok(reference) = serde_json::from_value::<EvidenceRef>(value.clone()) {
        let id = scoped_key(scope, &reference)?;
        if keys.get(&id).is_some_and(|prior| prior != &reference) {
            return Err("source key collision".into());
        }
        keys.insert(id.clone(), reference);
        *value = json!({"source_key":id});
        return Ok(());
    }
    match value {
        Value::Object(map) => {
            map.remove("atom_ref");
            map.remove("evidence_index");
            for v in map.values_mut() {
                project(scope, v, keys)?;
            }
        }
        Value::Array(items) => {
            for v in items {
                project(scope, v, keys)?;
            }
        }
        _ => {}
    }
    Ok(())
}
/// Same projection for planner probes and the final admitted transport body.
pub fn request(state: &Checkpoint, body: &mut Value) -> Result<Keys, String> {
    request_in_scope(
        &scope(state),
        &state.outline_run.tool_draft.source_keys,
        body,
    )
}
pub(crate) fn request_in_scope(
    scope: &Value,
    issued: &Keys,
    body: &mut Value,
) -> Result<Keys, String> {
    let mut keys = issued.clone();
    for message in body["messages"]
        .as_array_mut()
        .ok_or("request messages missing")?
    {
        if let Some(mut payload) = message["content"]
            .as_str()
            .and_then(|s| serde_json::from_str::<Value>(s).ok())
        {
            project(scope, &mut payload, &mut keys)?;
            discover_wire::compact(&mut payload)?;
            message["content"] = json!(payload.to_string());
        }
    }
    Ok(keys)
}
pub fn resolve(state: &Checkpoint, name: &str, args: Value) -> Result<Value, String> {
    resolve_in_scope(
        &scope(state),
        name,
        args,
        &state.outline_run.tool_draft.source_keys,
    )
}
pub(crate) fn resolve_in_scope(
    scope: &Value,
    name: &str,
    mut args: Value,
    keys: &Keys,
) -> Result<Value, String> {
    super::agent::validate_arguments(name, &args)?;
    fn walk(scope: &Value, value: &mut Value, keys: &Keys) -> Result<(), String> {
        if let Some(id) = value.get("source_key").and_then(Value::as_str) {
            if value.as_object().is_none_or(|m| m.len() != 1) {
                return Err("source_key must be the only field".into());
            }
            let reference = keys.get(id).ok_or("source key was not issued")?;
            if scoped_key(scope, reference)? != id {
                return Err("source key scope or epoch is stale".into());
            }
            *value = json!(reference);
            return Ok(());
        }
        match value {
            Value::Object(map) => {
                for v in map.values_mut() {
                    walk(scope, v, keys)?
                }
            }
            Value::Array(items) => {
                for v in items {
                    walk(scope, v, keys)?
                }
            }
            _ => {}
        }
        Ok(())
    }
    walk(scope, &mut args, keys)?;

    Ok(args)
}
#[cfg(test)]
pub(crate) fn fixture_arguments(
    input: &crate::analysis::FrozenInput,
    state: &Checkpoint,
    name: &str,
    mut args: Value,
) -> Result<Value, String> {
    if name == "submit_pack" {
        return super::discover::fixture_key_submission(
            input,
            state
                .outline_run
                .reading_packs
                .as_ref()
                .ok_or("pack missing")?,
            args,
        );
    }
    fn encode(state: &Checkpoint, value: &mut Value) -> Result<(), String> {
        if let Ok(reference) = serde_json::from_value::<EvidenceRef>(value.clone()) {
            let id = key(state, &reference)?;
            if state.outline_run.tool_draft.source_keys.get(&id) != Some(&reference) {
                return Err("runtime fixture must first obtain current source keys from an admitted request".into());
            }
            *value = json!({"source_key":id});
            return Ok(());
        }
        match value {
            Value::Object(map) => {
                for v in map.values_mut() {
                    encode(state, v)?
                }
            }
            Value::Array(items) => {
                for v in items {
                    encode(state, v)?
                }
            }
            _ => {}
        }
        Ok(())
    }
    encode(state, &mut args)?;
    Ok(args)
}

#[cfg(test)]
mod tests {
    #[test]
    fn model_key_boundary_rejects_unissued_raw_stale_and_wrong_purpose() {
        use super::*;
        let input = super::super::tests::input();
        let mut state = super::super::acceptance::checkpoint(&input);
        let reference = EvidenceRef::Text {
            input_digest: super::super::evidence::input_digest(&input).unwrap(),
            unit_id: input.source_units[0].source_unit_revision_id.clone(),
            start_byte: 0,
            end_byte: 3,
        };
        assert!(resolve(&state, "read_evidence", json!({"refs":[reference]})).is_err());
        let mut body = json!({"messages":[{"role":"tool","content":json!({"evidence":reference}).to_string()}]});
        let issued = request(&state, &mut body).unwrap();
        let projected: Value =
            serde_json::from_str(body["messages"][0]["content"].as_str().unwrap()).unwrap();
        let keyed = projected["evidence"].clone();
        assert!(
            resolve(&state, "read_evidence", json!({"refs":[keyed]})).is_err(),
            "projecting a candidate does not issue keys"
        );
        state.outline_run.tool_draft.source_keys = issued;
        assert_eq!(
            resolve(&state, "read_evidence", json!({"refs":[keyed]})).unwrap()["refs"][0],
            json!(reference)
        );
        assert!(
            resolve(
                &state,
                "read_evidence",
                json!({"refs":[keyed],"max_bytes":16384})
            )
            .is_err()
        );
        assert!(
            resolve(
                &state,
                "submit_review",
                json!({"requirement_ids":[],"pack_ids":[],"inspected_evidence":[keyed],"issues":[]})
            )
            .is_err(),
            "source keys cannot grant Check review credit"
        );
        let mut stale = state.clone();
        stale.outline_run.tool_draft.read_epoch += 1;
        assert!(resolve(&stale, "read_evidence", json!({"refs":[keyed]})).is_err());
        state.input_sha256.push('x');
        assert!(resolve(&state, "read_evidence", json!({"refs":[keyed]})).is_err());
    }
    #[test]
    fn advertised_outline_protocol_has_no_legacy_source_inputs() {
        for duty in [
            super::super::agent::Duty::Discover,
            super::super::agent::Duty::Organize,
            super::super::agent::Duty::Check,
        ] {
            for tool in super::super::agent::schemas_for(duty) {
                let name = tool["function"]["name"].as_str().unwrap();
                let parameters = &tool["function"]["parameters"];
                let text = parameters.to_string();
                for legacy in ["\"input_digest\"", "\"atom_ref\"", "\"evidence_index\""] {
                    assert!(!text.contains(legacy), "{name} exposes {legacy}");
                }
                if matches!(
                    name,
                    "read_requirements" | "read_outline" | "read_evidence" | "read_claim_evidence"
                ) {
                    assert_eq!(parameters["properties"]["cursor"]["type"], "string");
                    assert!(parameters["properties"].get("max_bytes").is_none());
                    assert!(parameters["properties"].get("version").is_none());
                }
            }
        }
    }
}
