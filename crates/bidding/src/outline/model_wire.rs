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
    naming_scope: String,
    names: BTreeMap<String, Binding>,
}
#[derive(Default)]
struct ReferenceReport {
    reproject: bool,
    errors: Vec<Value>,
    mappings: Vec<Value>,
}
impl Default for Registry {
    fn default() -> Self {
        Self {
            nonce: uuid::Uuid::new_v4().to_string(),
            active: String::new(),
            scopes: BTreeMap::new(),
            naming_scope: String::new(),
            names: BTreeMap::new(),
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
        "document_key" => 'b',
        _ => return None,
    })
}
/// Metadata paths retain their domain field name through array indices.
pub(super) fn reference_field(path: &[String]) -> Option<&str> {
    let field = path
        .iter()
        .rev()
        .find(|part| part.parse::<usize>().is_err())?;
    kind(field).map(|_| field.as_str())
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
        let binding = Binding {
            kind,
            canonical: canonical.into(),
            owner,
        };
        let key = if let Some((key, _)) = self.names.iter().find(|(_, value)| **value == binding) {
            key.clone()
        } else {
            let key = format!("{kind}{}", self.names.len() + 1);
            self.names.insert(key.clone(), binding.clone());
            key
        };
        // Naming history is not authority. Only values projected into this
        // request enter its active allowlist.
        self.scopes
            .get_mut(&self.active)
            .ok_or("request scope missing")?
            .insert(key.clone(), binding);
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
                // A metadata fragment's `value` keeps the type of its canonical
                // field. Never expose raw IDs or reinterpret them as free text.
                if fields.contains_key("ordinal")
                    && fields.contains_key("start_byte")
                    && fields.contains_key("end_byte")
                    && let Some(path) = fields
                        .get("path")
                        .and_then(|path| serde_json::from_value::<Vec<String>>(path.clone()).ok())
                    && let Some(reference) = reference_field(&path)
                    && let Some(value) = fields.get_mut("value")
                {
                    self.walk_encode(reference, value, owner)?;
                    return Ok(());
                }
                let local_owner = fields
                    .get("requirement_id")
                    .or_else(|| {
                        fields
                            .get("record_identity")
                            .and_then(|identity| identity.get("requirement_id"))
                    })
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
                    if key == "rejected_submission_audit" {
                        continue;
                    }
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
        field_path: (&str, &str),
        value: &mut Value,
        owner: Option<&str>,
        report: &mut ReferenceReport,
    ) {
        let (field, path) = field_path;
        match value {
            Value::String(text) => {
                if let Some(kind) = kind(field)
                    && !text.is_empty()
                {
                    let binding = self.scopes.get(scope).and_then(|items| items.get(text));
                    let resolved = match binding {
                        None => Err(("unknown_reference", "unknown current-scope reference")),
                        Some(binding) if binding.kind != kind => {
                            Err(("wrong_namespace", "wrong reference namespace"))
                        }
                        Some(binding)
                            if matches!(kind, 'q' | 'k') && binding.owner.as_deref() != owner =>
                        {
                            Err(("wrong_owner", "wrong reference owner"))
                        }
                        Some(binding) if report.reproject => self
                            .scopes
                            .get(&self.active)
                            .and_then(|items| {
                                items.iter().find(|(_, candidate)| *candidate == binding)
                            })
                            .map(|(key, _)| key.clone())
                            .ok_or((
                                "not_currently_issued",
                                "historical reference is not issued in the current request",
                            )),
                        Some(binding) => Ok(binding.canonical.clone()),
                    };
                    match resolved {
                        Ok(resolved) => {
                            if report.reproject {
                                report.mappings.push(json!({"path":path,"value":text,
                                    "current_value":resolved,"expected_type":field}));
                            }
                            *text = resolved;
                        }
                        Err((code, reason)) => report.errors.push(json!({"path":path,"value":text,
                            "expected_type":field,"expected_prefix":kind.to_string(),"code":code,"reason":reason})),
                    }
                }
            }
            Value::Array(items) => {
                for (index, item) in items.iter_mut().enumerate() {
                    self.walk_decode(
                        scope,
                        (field, &format!("{path}/{index}")),
                        item,
                        owner,
                        report,
                    );
                }
            }
            Value::Object(fields) => {
                // Resolve the owner from the original wire value before any
                // field is decoded, including nested review batches.
                let local_owner = fields
                    .get("requirement_id")
                    .or_else(|| {
                        fields
                            .get("record_identity")
                            .and_then(|id| id.get("requirement_id"))
                    })
                    .and_then(Value::as_str)
                    .and_then(|id| self.scopes.get(scope)?.get(id))
                    .filter(|binding| binding.kind == 'r')
                    .map(|binding| binding.canonical.clone());
                let owner = local_owner.as_deref().or(owner);
                for (key, item) in fields {
                    let pointer_key = key.replace('~', "~0").replace('/', "~1");
                    self.walk_decode(
                        scope,
                        (key, &format!("{path}/{pointer_key}")),
                        item,
                        owner,
                        report,
                    );
                }
            }
            _ => (),
        }
    }
    fn argument_scope(&self, args: &Value, historical: bool) -> Result<String, String> {
        let scope = args
            .get("wire_scope")
            .and_then(Value::as_str)
            .ok_or("wire_scope required")?;
        if !historical && scope != self.active {
            return Err("stale request scope".into());
        }
        if !self.scopes.contains_key(scope) {
            return Err("unissued request scope".into());
        }
        Ok(scope.to_owned())
    }
    pub fn decode(&self, mut args: Value, historical: bool) -> Result<Value, String> {
        let scope = self.argument_scope(&args, historical)?;
        args.as_object_mut()
            .ok_or("arguments must be object")?
            .remove("wire_scope");
        let mut report = ReferenceReport::default();
        self.walk_decode(&scope, ("", ""), &mut args, None, &mut report);
        if !report.errors.is_empty() {
            return Err(format!(
                "invalid typed references: {}",
                json!(report.errors)
            ));
        }
        Ok(args)
    }
    pub(crate) fn reference_diagnostics(
        &self,
        args: &Value,
        historical: bool,
    ) -> Result<Vec<Value>, String> {
        let scope = self.argument_scope(args, historical)?;
        let mut arguments = args.clone();
        let mut report = ReferenceReport::default();
        self.walk_decode(&scope, ("", ""), &mut arguments, None, &mut report);
        Ok(report.errors)
    }
    /// Failure-loop fingerprint only. Successful references retain their
    /// identity even when another field is invalid; invalid strings cannot
    /// change the fingerprint by changing their spelling. The caller resolves
    /// claim-scoped domain handles, removes transport fields and normalizes
    /// unordered collections. No partially decoded action is returned or
    /// authorized: execution must still pass `decode` in full.
    pub(crate) fn canonical_signature(
        &self,
        args: &Value,
        normalize: impl FnOnce(&mut Value),
    ) -> Result<String, String> {
        let scope = self.argument_scope(args, false)?;
        let mut arguments = args.clone();
        arguments
            .as_object_mut()
            .ok_or("arguments must be object")?
            .remove("wire_scope");
        let mut report = ReferenceReport::default();
        self.walk_decode(&scope, ("", ""), &mut arguments, None, &mut report);
        for error in report.errors {
            let path = error["path"]
                .as_str()
                .ok_or("reference diagnostic path missing")?;
            let value = arguments
                .pointer_mut(path)
                .ok_or("reference diagnostic path invalid")?;
            *value = json!({"invalid_typed_reference":true});
        }
        normalize(&mut arguments);
        super::canonical_sha256(&arguments)
    }
    /// Diagnostic projection only: historical bindings identify what the model
    /// meant; the current allowlist alone decides whether a mapping is offered.
    /// Unknown historical values are never interpreted in the current scope.
    /// This does not issue any handle or return canonical identities.
    pub(crate) fn reproject_rejected_arguments(&self, args: &Value) -> Result<Value, String> {
        let scope = self.argument_scope(args, true)?;
        let mut arguments = args.clone();
        let mut report = ReferenceReport {
            reproject: true,
            ..ReferenceReport::default()
        };
        self.walk_decode(&scope, ("", ""), &mut arguments, None, &mut report);
        Ok(json!({"current_reference_mappings":report.mappings,
            "invalid_reference_diagnostics":report.errors}))
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
    project_claim_scope(prior, identity, identity, body)
}

/// The immutable claim owns names; each concrete request independently owns
/// authority. Callers without a stable claim keep using `project_scope`.
pub(crate) fn project_claim_scope(
    prior: &Registry,
    claim_identity: &Value,
    request_identity: &Value,
    body: &mut Value,
) -> Result<Registry, String> {
    let mut registry = prior.clone();
    let naming_scope = super::canonical_sha256(&(&registry.nonce, claim_identity))?;
    if registry.naming_scope != naming_scope {
        registry.naming_scope = naming_scope;
        registry.names.clear();
    }
    let scope = format!(
        "w{}",
        &super::canonical_sha256(&(&registry.nonce, claim_identity, request_identity))?[..16]
    );
    registry.active = scope.clone();
    // Even rebuilding a request may not revive a value omitted from its body.
    registry.scopes.insert(scope.clone(), BTreeMap::new());
    for message in body["messages"].as_array_mut().ok_or("messages missing")? {
        if let Some(text) = message["content"].as_str()
            && let Ok(mut payload) = serde_json::from_str::<Value>(text)
        {
            registry.walk_encode("", &mut payload, None)?;
            message["content"] = json!(payload.to_string());
        }
    }
    // Failed candidates are opaque audit text, never another source of
    // authority. Build repair hints only after all current bindings exist.
    for message in body["messages"].as_array_mut().ok_or("messages missing")? {
        if message["role"] != "user" {
            continue;
        }
        let Some(text) = message["content"].as_str() else {
            continue;
        };
        let Ok(mut payload) = serde_json::from_str::<Value>(text) else {
            continue;
        };
        let Some(audit) = payload
            .get_mut("rejected_submission_audit")
            .and_then(Value::as_object_mut)
        else {
            continue;
        };
        let projection = audit
            .get("original_arguments_text")
            .and_then(Value::as_str)
            .ok_or_else(|| "original arguments text missing".to_owned())
            .and_then(|text| serde_json::from_str::<Value>(text).map_err(|error| error.to_string()))
            .and_then(|args| registry.reproject_rejected_arguments(&args));
        let projection = projection.unwrap_or_else(|reason| json!({
            "current_reference_mappings":[],
            "invalid_reference_diagnostics":[{"path":"/wire_scope","expected_type":"wire_scope","reason":reason}]
        }));
        audit.insert(
            "current_reference_mappings".into(),
            projection["current_reference_mappings"].clone(),
        );
        audit.insert(
            "invalid_reference_diagnostics".into(),
            projection["invalid_reference_diagnostics"].clone(),
        );
        message["content"] = json!(payload.to_string());
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
    fn simple_body(payload: Value) -> Value {
        json!({"messages":[{"role":"user","content":payload.to_string()}],"tools":[]})
    }

    #[test]
    fn failure_signature_preserves_valid_references_and_collapses_invalid_spelling() {
        let claim = json!("claim");
        let payload = json!({"items":[{"evidence_key":"first"},{"evidence_key":"second"},
            {"atom_key":"atom-one"},{"atom_key":"atom-two"},
            {"call_id":"call-one"},{"call_id":"call-two"},{"source_key":"source"}]});
        let first = project_claim_scope(
            &Registry::default(),
            &claim,
            &json!(1),
            &mut simple_body(payload.clone()),
        )
        .unwrap();
        let mut args = first
            .fixture_encode(json!({"evidence":[{"evidence_key":"first"}],
            "inspected_atom_ids":[{"atom_key":"atom-one"},{"atom_key":"atom-two"}],
            "call_id":"call-one"}))
            .unwrap();
        args["evidence"]
            .as_array_mut()
            .unwrap()
            .push(json!({"evidence_key":"e999999"}));
        let normalize = |args: &mut Value| {
            args.as_object_mut().unwrap().remove("call_id");
            for field in ["evidence", "inspected_atom_ids"] {
                args[field]
                    .as_array_mut()
                    .unwrap()
                    .sort_by_key(Value::to_string);
            }
        };
        let fingerprint = first.canonical_signature(&args, normalize).unwrap();
        let mut changed_valid = args.clone();
        changed_valid["evidence"][0]["evidence_key"] = first
            .fixture_encode(json!({"evidence_key":"second"}))
            .unwrap()["evidence_key"]
            .clone();
        assert_ne!(
            fingerprint,
            first
                .canonical_signature(&changed_valid, normalize)
                .unwrap()
        );
        // Changing garbage, supplying a raw canonical ID, or borrowing a
        // different namespace cannot masquerade as meaningful source repair.
        for invalid in [
            json!("e123456"),
            json!("second"),
            first
                .fixture_encode(json!({"source_key":"source"}))
                .unwrap()["source_key"]
                .clone(),
        ] {
            let mut changed_invalid = args.clone();
            changed_invalid["evidence"][1]["evidence_key"] = invalid;
            assert_eq!(
                fingerprint,
                first
                    .canonical_signature(&changed_invalid, normalize)
                    .unwrap()
            );
            assert!(first.decode(changed_invalid, false).is_err());
        }
        let mut reordered = args.clone();
        reordered["evidence"].as_array_mut().unwrap().reverse();
        reordered["inspected_atom_ids"]
            .as_array_mut()
            .unwrap()
            .reverse();
        reordered["call_id"] =
            first.fixture_encode(json!({"call_id":"call-two"})).unwrap()["call_id"].clone();
        assert_eq!(
            fingerprint,
            first.canonical_signature(&reordered, normalize).unwrap()
        );
        assert!(first.decode(args.clone(), false).is_err());
        assert!(first.decode(changed_valid, false).is_err());
        let second =
            project_claim_scope(&first, &claim, &json!(2), &mut simple_body(payload)).unwrap();
        assert!(second.canonical_signature(&args, normalize).is_err());
        args["wire_scope"] = json!(second.active);
        assert_eq!(
            fingerprint,
            second.canonical_signature(&args, normalize).unwrap()
        );
    }

    #[test]
    fn signature_diagnostic_paths_escape_keys_before_replacing_invalid_values() {
        let registry =
            project_scope(&Registry::default(), &json!(1), &mut simple_body(json!({}))).unwrap();
        let first = json!({"wire_scope":registry.active,"nested/~key":{"evidence_key":"e123"}});
        let second =
            json!({"wire_scope":registry.active,"nested/~key":{"evidence_key":"raw-canonical"}});
        assert_eq!(
            registry.reference_diagnostics(&first, false).unwrap()[0]["path"],
            "/nested~1~0key/evidence_key"
        );
        assert_eq!(
            registry.canonical_signature(&first, |_| {}).unwrap(),
            registry.canonical_signature(&second, |_| {}).unwrap()
        );
    }

    #[test]
    fn claim_names_stay_stable_but_only_current_projection_authorizes() {
        let claim = json!({"input":"input-one","pack":"pack-one","revision":1,"claim":"one"});
        let first = project_claim_scope(
            &Registry::default(),
            &claim,
            &json!(1),
            &mut simple_body(json!({"items":[{"evidence_key":"first"},{"evidence_key":"shared"}]})),
        )
        .unwrap();
        let first_only = first
            .fixture_encode(json!({"evidence_key":"first"}))
            .unwrap();
        let shared = first
            .fixture_encode(json!({"evidence_key":"shared"}))
            .unwrap();
        let restored: Registry = serde_json::from_value(json!(first)).unwrap();
        let second = project_claim_scope(
            &restored,
            &claim,
            &json!(2),
            &mut simple_body(json!({"items":[{"source_key":"new"},{"evidence_key":"shared"}]})),
        )
        .unwrap();
        let current = second
            .fixture_encode(json!({"evidence_key":"shared"}))
            .unwrap();
        assert_eq!(shared["evidence_key"], current["evidence_key"]);
        assert_ne!(shared["wire_scope"], current["wire_scope"]);
        assert!(second.decode(shared, false).is_err());
        assert!(second.decode(first_only.clone(), true).is_ok());
        let mut retired = first_only;
        retired["wire_scope"] = json!(second.active);
        assert!(second.decode(retired, false).is_err());
        assert!(
            second
                .fixture_encode(json!({"evidence_key":"first"}))
                .is_err()
        );
        assert!(
            second
                .decode(
                    json!({"wire_scope":second.active,"evidence_key":"shared"}),
                    false
                )
                .is_err()
        );
        assert_eq!(
            second.decode(current.clone(), false).unwrap()["evidence_key"],
            "shared"
        );

        // Rebuilding even the same request identity replaces its allowlist.
        let rebuilt = project_claim_scope(
            &second,
            &claim,
            &json!(2),
            &mut simple_body(json!({"source_key":"only-source"})),
        )
        .unwrap();
        assert!(rebuilt.decode(current.clone(), false).is_err());
        for field in ["input", "pack", "revision", "claim"] {
            let mut other_claim = claim.clone();
            other_claim[field] = json!("changed");
            let other = project_claim_scope(
                &second,
                &other_claim,
                &json!(2),
                &mut simple_body(json!({"source_key":"only-source"})),
            )
            .unwrap();
            assert_ne!(other.naming_scope, second.naming_scope);
            assert_ne!(other.active, second.active);
            assert_eq!(other.names.len(), 1);
            assert!(other.decode(current.clone(), false).is_err());
            assert!(
                other
                    .fixture_encode(json!({"evidence_key":"shared"}))
                    .is_err()
            );
        }
    }

    #[test]
    fn decode_reports_all_invalid_references_without_partial_authorization() {
        let registry = project_scope(
            &Registry::default(),
            &json!("one"),
            &mut simple_body(json!({"items":[
                {"requirement_id":"one","quote_handle":"quote"},
                {"requirement_id":"two","quote_handle":"quote"},
                {"evidence_key":"canonical-evidence"},{"source_key":"canonical-source"}]})),
        )
        .unwrap();
        let first = registry
            .fixture_encode(json!({"requirement_id":"one","quote_handle":"quote"}))
            .unwrap();
        let second = registry
            .fixture_encode(json!({"requirement_id":"two","quote_handle":"quote"}))
            .unwrap();
        let source = registry
            .fixture_encode(json!({"source_key":"canonical-source"}))
            .unwrap();
        let args = json!({"wire_scope":registry.active,"requirement_id":first["requirement_id"],
            "quote_handle":second["quote_handle"],"evidence":[
                {"evidence_key":"e999999"},{"evidence_key":source["source_key"]},
                {"evidence_key":"canonical-evidence"}]});
        let structured = registry.reference_diagnostics(&args, false).unwrap();
        let error = registry.decode(args, false).unwrap_err();
        let diagnostics: Value =
            serde_json::from_str(error.strip_prefix("invalid typed references: ").unwrap())
                .unwrap();
        let by_path = |items: &[Value]| {
            items
                .iter()
                .map(|item| (item["path"].as_str().unwrap().to_owned(), item.clone()))
                .collect::<BTreeMap<_, _>>()
        };
        assert_eq!(
            by_path(diagnostics.as_array().unwrap()),
            by_path(&structured)
        );
        assert_eq!(diagnostics.as_array().unwrap().len(), 4);
        for (path, reason) in [
            ("/quote_handle", "wrong reference owner"),
            (
                "/evidence/0/evidence_key",
                "unknown current-scope reference",
            ),
            ("/evidence/1/evidence_key", "wrong reference namespace"),
            (
                "/evidence/2/evidence_key",
                "unknown current-scope reference",
            ),
        ] {
            let item = diagnostics
                .as_array()
                .unwrap()
                .iter()
                .find(|item| item["path"] == path)
                .unwrap();
            assert_eq!(item["reason"], reason);
            assert!(item["value"].is_string());
            assert!(item["expected_type"].is_string());
            assert!(item["code"].is_string());
        }
        assert!(registry.decode(first, false).is_ok());
    }

    #[test]
    fn rejected_audit_maps_only_historical_and_current_authorized_references() {
        let claim = json!("claim-one");
        let first = project_claim_scope(
            &Registry::default(),
            &claim,
            &json!(1),
            &mut simple_body(json!({"items":[{"source_key":"source"},
                {"evidence_key":"kept"},{"evidence_key":"retired"}]})),
        )
        .unwrap();
        let kept = first
            .fixture_encode(json!({"evidence_key":"kept"}))
            .unwrap();
        let retired = first
            .fixture_encode(json!({"evidence_key":"retired"}))
            .unwrap();
        let never_issued = format!("e{}", first.names.len() + 1);
        let args = json!({"wire_scope":first.active,"requirements":[
            {"evidence":[{"evidence_key":kept["evidence_key"]}]},
            {"evidence":[{"evidence_key":retired["evidence_key"]}]},
            {"evidence":[{"evidence_key":never_issued}]},
            {"evidence":[{"evidence_key":"raw-secret-canonical"}]}]});
        let original_text = args.to_string();
        let mut body = simple_body(
            json!({"items":[{"evidence_key":"new"},{"evidence_key":"kept"}],
            "rejected_submission_audit":{"original_arguments_text":original_text,
                "evidence_key":"audit-must-not-issue"}}),
        );
        let second = project_claim_scope(&first, &claim, &json!(2), &mut body).unwrap();
        assert_eq!(
            second
                .fixture_encode(json!({"evidence_key":"new"}))
                .unwrap()["evidence_key"],
            never_issued
        );
        assert_eq!(second.names.len(), 4);
        assert_eq!(second.scopes[&second.active].len(), 2);
        let projected: Value =
            serde_json::from_str(body["messages"][0]["content"].as_str().unwrap()).unwrap();
        let audit = &projected["rejected_submission_audit"];
        assert_eq!(audit["original_arguments_text"], original_text);
        let mappings = audit["current_reference_mappings"].as_array().unwrap();
        assert_eq!(mappings.len(), 1);
        assert_eq!(
            mappings[0]["path"],
            "/requirements/0/evidence/0/evidence_key"
        );
        assert_eq!(mappings[0]["current_value"], kept["evidence_key"]);
        let errors = audit["invalid_reference_diagnostics"].as_array().unwrap();
        assert_eq!(errors.len(), 3);
        assert_eq!(
            errors[0]["reason"],
            "historical reference is not issued in the current request"
        );
        assert_eq!(errors[1]["value"], never_issued);
        assert_eq!(errors[1]["reason"], "unknown current-scope reference");
        let mut unknown_scope = args;
        unknown_scope["wire_scope"] = json!("w0000000000000000");
        assert!(second.reproject_rejected_arguments(&unknown_scope).is_err());
        assert!(
            second
                .fixture_encode(json!({"evidence_key":"retired"}))
                .is_err()
        );
    }

    #[test]
    fn typed_metadata_fragments_reissue_current_handles_and_reject_raw_or_stale_ids() {
        let input = super::super::tests::input();
        let mut state = super::super::acceptance::checkpoint(&input);
        let record = super::super::metadata_fragments::Record::new(json!({
            "requirement_id":"canonical-owner", "requirement_ids":["canonical-member"],
            "quote_handle":"canonical-quote", "description":"unchanged prose"
        }));
        let original = record.page("record-synthetic", &record.fields);
        let mut previous = None;
        for _ in 0..2 {
            let mut wire =
                json!({"messages":[{"role":"user","content":original.to_string()}],"tools":[]});
            let registry = project(&state, &mut wire).unwrap();
            let page: Value =
                serde_json::from_str(wire["messages"][0]["content"].as_str().unwrap()).unwrap();
            let field = |name: &str| {
                page["field_fragments"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|field| field["path"][0] == name)
                    .unwrap()["value"]
                    .clone()
            };
            assert!(!wire.to_string().contains("canonical-"));
            assert_eq!(field("description"), "unchanged prose");
            let args = json!({"wire_scope":page["wire_scope"],"requirement_id":field("requirement_id"),
                "requirement_ids":[field("requirement_ids")],"quote_handle":field("quote_handle")});
            let decoded = registry.decode(args.clone(), false).unwrap();
            assert_eq!(decoded["requirement_ids"], json!(["canonical-member"]));
            assert_eq!(decoded["quote_handle"], "canonical-quote");
            assert!(registry.decode(json!({"wire_scope":page["wire_scope"],"requirement_ids":["canonical-member"]}),false).is_err());
            if let Some(old) = previous {
                assert_eq!(
                    registry.decode(old, false).unwrap_err(),
                    "stale request scope"
                );
            }
            previous = Some(args);
            state.outline_run.tool_draft.model_wire = registry;
            state = serde_json::from_value(json!(state)).unwrap();
            state.turn += 1;
        }
    }
    #[test]
    fn shared_document_identity_aliases_are_scoped_and_cannot_authorize_evidence() {
        let input = super::super::tests::input();
        let mut state = super::super::acceptance::checkpoint(&input);
        let mut wire = body();
        let payload = json!({"source_documents":[
            {"document_key":"doc-one-revision-one","identity":{"document_id":"one"}},
            {"document_key":"doc-two-revision-one","identity":{"document_id":"two"}}],
            "atoms":[{"source_identity":{"document_key":"doc-one-revision-one"}},
                {"source_identity":{"document_key":"doc-two-revision-one"}}]});
        wire["messages"][0]["content"] = json!(payload.to_string());
        let original = wire.clone();
        let registry = project(&state, &mut wire).unwrap();
        let projected: Value =
            serde_json::from_str(wire["messages"][0]["content"].as_str().unwrap()).unwrap();
        let first = &projected["source_documents"][0]["document_key"];
        let second = &projected["source_documents"][1]["document_key"];
        assert_ne!(first, second);
        assert!(first.as_str().unwrap().starts_with('b'));
        assert_eq!(
            *first,
            projected["atoms"][0]["source_identity"]["document_key"]
        );
        assert_eq!(
            *second,
            projected["atoms"][1]["source_identity"]["document_key"]
        );
        assert!(!wire.to_string().contains("doc-one-revision-one"));
        assert!(
            registry
                .decode(
                    json!({"wire_scope":registry.active,"evidence_key":first}),
                    false
                )
                .is_err()
        );
        let encoded = registry
            .fixture_encode(json!({"document_key":"doc-one-revision-one"}))
            .unwrap();
        let restored: Registry = serde_json::from_value(json!(registry)).unwrap();
        assert_eq!(
            restored.decode(encoded.clone(), false).unwrap()["document_key"],
            "doc-one-revision-one"
        );
        state.outline_run.tool_draft.model_wire = registry;
        state.turn += 1;
        let mut next_wire = original;
        let next = project(&state, &mut next_wire).unwrap();
        assert!(next.decode(encoded, false).is_err());
    }
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

    #[test]
    #[ignore = "offline synthetic registry timing; requires private output path"]
    fn synthetic_registry_generations_profile() {
        use std::io::Write;
        use std::time::Instant;
        let output =
            std::path::PathBuf::from(std::env::var("KB_PRIVATE_REGISTRY_PROFILE").unwrap());
        let parent = output.parent().unwrap().canonicalize().unwrap();
        assert!(!parent.starts_with(std::env::current_dir().unwrap().canonicalize().unwrap()));
        let limits: Value = serde_json::from_slice(
            &std::fs::read(std::env::var("KB_PRIVATE_PROFILE_LIMITS").unwrap()).unwrap(),
        )
        .unwrap();
        let profile: crate::agent_runtime::chat::TokenizerProfile =
            serde_json::from_value(limits["extraction"]["tokenizer"].clone()).unwrap();
        let canonical = json!({"model":profile.model_id,"messages":[{"role":"user","content":json!({
        "sources":(0..256).map(|n|json!({"requirement_id":format!("synthetic-r-{n}"),
        "source_unit_revision_id":format!("synthetic-unit-{n}"),"quote_handle":format!("synthetic-q-{n}"),
        "text":"Synthetic immutable source paragraph."})).collect::<Vec<_>>()
    }).to_string()}],"tools":[]});
        let mut registry = Registry::default();
        let mut rows = Vec::new();
        for generation in 0..32 {
            let start = Instant::now();
            let copied = registry.clone();
            let clone_us = start.elapsed().as_micros();
            drop(copied);
            let start = Instant::now();
            let mut body = canonical.clone();
            let body_clone_us = start.elapsed().as_micros();
            let start = Instant::now();
            registry = project_scope(
                &registry,
                &json!(["synthetic-fixed-source", generation]),
                &mut body,
            )
            .unwrap();
            let project_us = start.elapsed().as_micros();
            let start = Instant::now();
            let bytes = serde_json::to_vec(&registry).unwrap();
            let serialize_us = start.elapsed().as_micros();
            let path = parent.join("registry-benchmark-synthetic.tmp");
            let start = Instant::now();
            let mut options = std::fs::OpenOptions::new();
            options.create(true).truncate(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&path).unwrap();
            file.write_all(&bytes).unwrap();
            file.sync_all().unwrap();
            let write_sync_us = start.elapsed().as_micros();
            let wire_tokens = crate::agent_runtime::chat::estimate_request_tokens_with_reserve(
                &body, &profile, 0, 0, 0,
            )
            .unwrap()
            .total_input_tokens;
            rows.push(json!({"generation":generation,"scopes":registry.scopes.len(),"wire_estimated_input_tokens":wire_tokens,
            "bindings":registry.scopes.values().map(|v|v.len()).sum::<usize>(),
            "registry_bytes":bytes.len(),"wire_bytes":serde_json::to_vec(&body).unwrap().len(),
            "registry_clone_us":clone_us,"body_clone_us":body_clone_us,"project_inclusive_us":project_us,
            "registry_serialize_us":serialize_us,"registry_write_sync_us":write_sync_us}));
        }
        assert_eq!(registry.scopes.len(), 32);
        let summary = json!({"model_calls":0,"synthetic_only":true,"tokenizer":profile,"rows":rows,
        "limitations":"project includes its internal prior clone; standalone clone is separate, not additive. Registry serialization/write+fsync is not full checkpoint Journal.save. Full production prepare is measured by separate private planner helper; Wire tokens use the supplied calibrated profile, not provider usage."});
        let mut options = std::fs::OpenOptions::new();
        options.create(true).truncate(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options
            .open(output)
            .unwrap()
            .write_all(&serde_json::to_vec_pretty(&summary).unwrap())
            .unwrap();
    }
}
