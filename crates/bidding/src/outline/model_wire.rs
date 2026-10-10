//! One explicit request scope and typed local handles; canonical identities stay internal.
use crate::analysis::agent::Checkpoint;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    kind: char,
    canonical: String,
    owner: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    nonce: String,
    active: String,
    scopes: BTreeMap<String, BTreeMap<String, Binding>>,
}
impl Default for Registry {
    fn default() -> Self {
        Self {
            nonce: uuid::Uuid::new_v4().to_string(),
            active: String::new(),
            scopes: BTreeMap::new(),
        }
    }
}
fn kind(field: &str) -> Option<char> {
    Some(match field {
        "atom_key" | "owner_atom_keys" => 'a',
        "evidence_key" => 'e',
        "source_key" => 's',
        "support_key" => 'd',
        "claim_token" => 'c',
        "submission_operation_id" | "call_id" => 'o',
        "pack_id" | "pack_ids" => 'p',
        "requirement_id" | "requirement_ids" | "source_requirement_id" => 'r',
        "section_id" | "source_section_id" => 'n',
        "form_id" | "table_id" => 't',
        "cursor" | "next_cursor" => 'u',
        "review_evidence_key" | "structural_receipt_id" => 'v',
        "quote_handle" | "evidence_handles" | "primary_handles" | "support_handles" => 'q',
        "version" | "review_version" | "template_review_version" | "source_review_version" => 'z',
        "source_id" | "image_id" | "source_unit_revision_id" => 'i',
        _ => return None,
    })
}
impl Registry {
    fn issue(
        &mut self,
        kind: char,
        canonical: &str,
        owner: Option<&str>,
    ) -> Result<String, String> {
        let owner = if matches!(kind, 'q' | 'k') {
            owner.map(str::to_owned)
        } else {
            None
        };
        let bindings = self
            .scopes
            .get_mut(&self.active)
            .ok_or("request scope missing")?;
        if let Some((key, _)) = bindings
            .iter()
            .find(|(_, v)| v.kind == kind && v.canonical == canonical && v.owner == owner)
        {
            return Ok(key.clone());
        }
        let key = format!("{kind}{}", bindings.len() + 1);
        if bindings.contains_key(&key) {
            return Err("wire handle collision".into());
        }
        bindings.insert(
            key.clone(),
            Binding {
                kind,
                canonical: canonical.into(),
                owner,
            },
        );
        Ok(key)
    }
    fn walk_encode(
        &mut self,
        field: &str,
        value: &mut Value,
        owner: Option<&str>,
    ) -> Result<(), String> {
        match value {
            Value::String(text) => {
                if let Some(kind) = kind(field)
                    && !text.is_empty()
                {
                    *text = self.issue(kind, text, owner)?;
                }
            }
            Value::Array(items) => {
                for item in items {
                    self.walk_encode(field, item, owner)?;
                }
            }
            Value::Object(fields) => {
                let local_owner = fields
                    .get("requirement_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                let owner = local_owner.as_deref().or(owner);
                let id_kind =
                    if fields.contains_key("atoms") && fields.contains_key("pack_revision") {
                        Some('p')
                    } else if fields.contains_key("obligation_strength") {
                        Some('r')
                    } else {
                        None
                    };
                for (key, item) in fields {
                    if key == "id"
                        && let (Some(prefix), Some(text)) = (id_kind, item.as_str())
                    {
                        *item = json!(self.issue(prefix, text, owner)?);
                        continue;
                    }
                    self.walk_encode(key, item, owner)?;
                }
            }
            _ => (),
        }
        Ok(())
    }
    fn walk_decode(
        &self,
        scope: &str,
        field: &str,
        value: &mut Value,
        path: &str,
        owner: Option<&str>,
    ) -> Result<(), String> {
        match value {
            Value::String(text) => {
                if let Some(kind) = kind(field)
                    && !text.is_empty()
                {
                    let binding = self
                        .scopes
                        .get(scope)
                        .and_then(|bindings| bindings.get(text))
                        .ok_or_else(|| format!("{path}: unknown current-scope {field}"))?;
                    if binding.kind != kind
                        || (matches!(kind, 'q' | 'k') && binding.owner.as_deref() != owner)
                    {
                        return Err(format!("{path}: wrong reference namespace"));
                    }
                    *text = binding.canonical.clone();
                }
            }
            Value::Array(items) => {
                for (index, item) in items.iter_mut().enumerate() {
                    self.walk_decode(scope, field, item, &format!("{path}/{index}"), owner)?;
                }
            }
            Value::Object(fields) => {
                for (key, item) in fields {
                    self.walk_decode(scope, key, item, &format!("{path}/{key}"), owner)?;
                }
            }
            _ => (),
        }
        Ok(())
    }
    pub fn decode(&self, mut args: Value, historical: bool) -> Result<Value, String> {
        let scope = args
            .get("wire_scope")
            .and_then(Value::as_str)
            .ok_or("wire_scope required")?
            .to_owned();
        if !historical && scope != self.active {
            return Err("stale request scope".into());
        }
        if !self.scopes.contains_key(&scope) {
            return Err("unissued request scope".into());
        }
        args.as_object_mut()
            .ok_or("arguments must be object")?
            .remove("wire_scope");
        let owner = args
            .get("requirement_id")
            .and_then(Value::as_str)
            .and_then(|id| self.scopes.get(&scope)?.get(id))
            .filter(|binding| binding.kind == 'r')
            .map(|binding| binding.canonical.clone());
        self.walk_decode(&scope, "", &mut args, "", owner.as_deref())?;
        Ok(args)
    }
    #[cfg(test)]
    pub fn fixture_encode(&self, mut args: Value) -> Result<Value, String> {
        let mut copy = self.clone();
        copy.walk_encode("", &mut args, None)?;
        if copy.scopes != self.scopes {
            return Err("fixture refers to an unissued wire identity".into());
        }
        args["wire_scope"] = json!(self.active);
        Ok(args)
    }
}
fn schema(value: &mut Value) {
    if let Some(properties) = value.get_mut("properties").and_then(Value::as_object_mut) {
        for (field, spec) in properties {
            if let Some(prefix) = kind(field) {
                let target = if spec["type"] == "array" {
                    &mut spec["items"]
                } else {
                    &mut *spec
                };
                if target["type"] == "string" {
                    target["pattern"] = json!(format!("^{prefix}[1-9][0-9]*$"));
                }
            }
        }
    }
    match value {
        Value::Object(fields) => {
            for item in fields.values_mut() {
                schema(item)
            }
        }
        Value::Array(items) => {
            for item in items {
                schema(item)
            }
        }
        _ => (),
    }
}
pub fn project(state: &Checkpoint, body: &mut Value) -> Result<Registry, String> {
    let identity = json!([
        &state.input_sha256,
        state.turn,
        state
            .outline_run
            .reading_packs
            .as_ref()
            .map(super::discover::DiscoverWork::wire_scope_identity),
        state.outline_run.tool_draft.read_epoch
    ]);
    project_scope(&state.outline_run.tool_draft.model_wire, &identity, body)
}
pub(crate) fn project_scope(
    prior: &Registry,
    identity: &Value,
    body: &mut Value,
) -> Result<Registry, String> {
    let mut registry = prior.clone();
    let scope = format!(
        "w{}",
        &super::canonical_sha256(&(&registry.nonce, identity))?[..16]
    );
    registry.active = scope.clone();
    registry.scopes.entry(scope.clone()).or_default();
    for message in body["messages"].as_array_mut().ok_or("messages missing")? {
        if let Some(text) = message["content"].as_str()
            && let Ok(mut payload) = serde_json::from_str::<Value>(text)
        {
            registry.walk_encode("", &mut payload, None)?;
            message["content"] = json!(payload.to_string());
        }
    }
    for tool in body["tools"].as_array_mut().ok_or("tools missing")? {
        let parameters = &mut tool["function"]["parameters"];
        schema(parameters);
        parameters["properties"]["wire_scope"] =
            json!({"type":"string","pattern":"^w[0-9a-f]{16}$"});
        if parameters.get("required").is_none() {
            parameters["required"] = json!([]);
        }
        parameters["required"]
            .as_array_mut()
            .ok_or("required missing")?
            .push(json!("wire_scope"));
    }
    let host = body["messages"]
        .as_array_mut()
        .and_then(|messages| {
            messages
                .iter_mut()
                .rev()
                .find(|message| message["content"].is_string())
        })
        .ok_or("host packet missing")?;
    let mut payload: Value =
        serde_json::from_str(host["content"].as_str().ok_or("host content missing")?)
            .map_err(|e| e.to_string())?;
    payload["wire_scope"] = json!(scope);
    payload["reference_rule"] = json!(
        "Copy typed handles exactly and include wire_scope in every tool call. Handles have no authority in another request scope."
    );
    host["content"] = json!(payload.to_string());
    Ok(registry)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn body() -> Value {
        json!({"messages":[{"role":"user","content":json!({"pack":{"id":"pack-0","pack_revision":1,"atoms":[{"atom_key":"atom-full-hash","evidence_key":"ev-full-hash","source_key":"src-full-hash","support_key":"cs-full-hash"}],"claim_token":"claim-full-hash"},"submission_operation_id":"operation-full-hash","review_evidence_key":"review-full-hash","quote_handle":"q-original","claim_handle":"c-original","table_id":"table-original","next_cursor":"cursor-original"}).to_string()}],"tools":super::super::agent::schemas_for(super::super::agent::Duty::Discover)})
    }
    #[test]
    fn production_projection_is_deterministic_and_roundtrips_typed_ids() {
        let input = super::super::tests::input();
        let state = super::super::acceptance::checkpoint(&input);
        let mut wire = body();
        let registry = project(&state, &mut wire).unwrap();
        let mut second = body();
        assert_eq!(registry, project(&state, &mut second).unwrap());
        assert_eq!(wire, second);
        assert!(!wire.to_string().contains("full-hash"));
        let original = json!({"pack_id":"pack-0","claim_token":"claim-full-hash","call_id":"operation-full-hash","source_section_id":{"atom_key":"atom-full-hash"},"evidence":[{"evidence_key":"ev-full-hash"}],"cursor":"cursor-original","review_evidence_key":"review-full-hash","table_id":"table-original"});
        let encoded = registry.fixture_encode(original.clone()).unwrap();
        assert_eq!(registry.decode(encoded.clone(), false).unwrap(), original);
        let saved = serde_json::to_vec(&registry).unwrap();
        let restored: Registry = serde_json::from_slice(&saved).unwrap();
        assert_eq!(restored.decode(encoded, false).unwrap(), original);
    }
    #[test]
    fn template_review_version_is_issued_and_roundtrips_inside_review_batch() {
        let input = super::super::tests::input();
        let state = super::super::acceptance::checkpoint(&input);
        let mut wire = body();
        wire["messages"][0]["content"] = json!(
            json!({
                "requirement_id":"req", "template_review_version":"template-version",
            "pack_id":"pack", "source_review_version":"source-version"
            })
            .to_string()
        );
        let registry = project(&state, &mut wire).unwrap();
        assert!(!wire.to_string().contains("template-version"));
        let args = json!({"template_reviews":[{"requirement_id":"req","version":"template-version","claims":[]}],
            "source_dispositions":[{"pack_id":"pack","version":"source-version"}]});
        let encoded = registry.fixture_encode(args.clone()).unwrap();
        assert_eq!(registry.decode(encoded, false).unwrap(), args);
    }
    #[test]
    fn stale_unknown_raw_and_wrong_namespace_never_authorize() {
        let input = super::super::tests::input();
        let mut state = super::super::acceptance::checkpoint(&input);
        let registry = project(&state, &mut body()).unwrap();
        let args = registry
            .fixture_encode(json!({"atom_key":"atom-full-hash"}))
            .unwrap();
        assert!(registry.decode(json!({"atom_key":"a1"}), false).is_err());
        assert!(
            registry
                .decode(
                    json!({"wire_scope":registry.active,"atom_key":"atom-full-hash"}),
                    false
                )
                .is_err()
        );
        let mut wrong = args.clone();
        wrong["atom_key"] = registry
            .fixture_encode(json!({"evidence_key":"ev-full-hash"}))
            .unwrap()["evidence_key"]
            .clone();
        assert!(registry.decode(wrong, false).is_err());
        state.outline_run.tool_draft.model_wire = registry.clone();
        state.turn += 1;
        let next = project(&state, &mut body()).unwrap();
        assert!(next.decode(args.clone(), false).is_err());
        assert!(next.decode(args, true).is_ok());
        let other = super::super::acceptance::checkpoint(&input);
        let foreign = project(&other, &mut body()).unwrap();
        assert_ne!(foreign.active, registry.active);
    }
    #[test]
    fn local_quote_names_cannot_cross_requirement_owners() {
        let input = super::super::tests::input();
        let state = super::super::acceptance::checkpoint(&input);
        let mut wire = body();
        wire["messages"][0]["content"]=json!(json!({"units":[{"requirement_id":"r-one","quote_handle":"q0"},{"requirement_id":"r-two","quote_handle":"q0"}]}).to_string());
        let registry = project(&state, &mut wire).unwrap();
        let first = registry
            .fixture_encode(json!({"requirement_id":"r-one","quote_handle":"q0"}))
            .unwrap();
        let mut second = registry
            .fixture_encode(json!({"requirement_id":"r-two","quote_handle":"q0"}))
            .unwrap();
        assert_ne!(first["quote_handle"], second["quote_handle"]);
        second["quote_handle"] = first["quote_handle"].clone();
        assert!(registry.decode(second, false).is_err());
    }
    #[test]
    #[ignore = "private offline captured-request benchmark; no network or semantic model result"]
    fn private_final_wire_and_cache_benchmark() {
        use std::time::Instant;
        let root = std::path::PathBuf::from(std::env::var("KB_PRIVATE_WIRE_BENCHMARK").unwrap());
        let original: Value = serde_json::from_slice(
            &std::fs::read(root.join("v75-authorized-full-run/turn-0-main.json")).unwrap(),
        )
        .unwrap();
        let runtime: Value = serde_json::from_slice(
            &std::fs::read(root.join("v75-authorized-full-run/runtime.json")).unwrap(),
        )
        .unwrap();
        let profile: crate::agent_runtime::chat::TokenizerProfile =
            serde_json::from_value(runtime["limits"]["tokenizer"].clone()).unwrap();
        let input = super::super::tests::input();
        let state = super::super::acceptance::checkpoint(&input);
        let mut final_body = original.clone();
        final_body.as_object_mut().unwrap().remove("max_tokens");
        final_body
            .as_object_mut()
            .unwrap()
            .remove("max_completion_tokens");
        let started = Instant::now();
        super::super::source_wire::request(&state, &mut final_body).unwrap();
        let source_ms = started.elapsed().as_secs_f64() * 1000.;
        let started = Instant::now();
        let registry = project(&state, &mut final_body).unwrap();
        let registry_ms = started.elapsed().as_secs_f64() * 1000.;
        fn source_text(value: &Value, out: &mut Vec<String>) {
            match value {
                Value::Object(fields) => {
                    for (key, item) in fields {
                        if matches!(key.as_str(), "quote" | "text" | "url")
                            && let Some(text) = item.as_str()
                        {
                            out.push(format!("{key}:{text}"));
                        }
                        source_text(item, out);
                    }
                }
                Value::Array(items) => {
                    for item in items {
                        source_text(item, out)
                    }
                }
                Value::String(text) => {
                    if let Ok(payload) = serde_json::from_str::<Value>(text)
                        && (payload.is_object() || payload.is_array())
                    {
                        source_text(&payload, out)
                    }
                }
                _ => (),
            }
        }
        let mut before_text = Vec::new();
        let mut after_text = Vec::new();
        source_text(&original, &mut before_text);
        source_text(&final_body, &mut after_text);
        before_text.sort();
        after_text.sort();
        assert_eq!(before_text, after_text);
        let count = |value: &Value| {
            crate::agent_runtime::chat::estimate_request_tokens_with_reserve(
                value, &profile, 4096, 1024, 16384,
            )
            .unwrap()
        };
        let before = count(&original);
        let counters = crate::agent_runtime::chat::cache_counts();
        let started = Instant::now();
        let after = count(&final_body);
        let cold_ms = started.elapsed().as_secs_f64() * 1000.;
        let started = Instant::now();
        let repeat = count(&final_body);
        let warm_ms = started.elapsed().as_secs_f64() * 1000.;
        let final_counters = crate::agent_runtime::chat::cache_counts();
        assert_eq!(after.total_context_tokens, repeat.total_context_tokens);
        assert_eq!(final_counters.0 - counters.0, 1);
        assert_eq!(final_counters.1 - counters.1, 1);
        assert!(
            final_body.get("max_tokens").is_none()
                && final_body.get("max_completion_tokens").is_none()
        );
        let report = json!({"provider_calls":0,"baseline_request_bytes":original.to_string().len(),"final_request_bytes":final_body.to_string().len(),"baseline_estimated_input_tokens":before.total_input_tokens,"final_estimated_input_tokens":after.total_input_tokens,"internal_output_reserve":16384,"final_estimated_context_tokens":after.total_context_tokens,"source_projection_ms":source_ms,"registry_projection_ms":registry_ms,"cold_count_ms":cold_ms,"warm_count_ms":warm_ms,"cache_hits":final_counters.0-counters.0,"cache_misses":final_counters.1-counters.1,"source_text_and_image_urls_exact":true,"issued_handles":registry.scopes[&registry.active].len(),"caveat":"Offline projection of captured first request; fixture scope identity; calibrated estimate, not actual provider usage or full planner end-to-end speedup."});
        std::fs::write(
            root.join("final-wire-offline-benchmark.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
    }
}
