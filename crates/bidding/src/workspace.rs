use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceMutationRequestV1 {
    pub schema_version: u8,
    pub workspace_id: Uuid,
    pub expected_workspace_revision_id: Uuid,
    pub expected_workspace_sha256: String,
    pub operations: Vec<WorkspaceOperationV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentMargins {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentSettings {
    pub page_size: String,
    pub margins_mm: DocumentMargins,
    pub body_font_pt: f64,
    pub line_spacing: f64,
    pub heading_numbering: String,
    pub header: String,
    pub footer: String,
    pub page_number: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BindingTargetV1 {
    OutlineNode { node_lineage_id: Uuid },
    ResponseTable { block_lineage_id: Uuid },
    StructuredForm { form_definition_revision_id: Uuid },
    Quote { quote_snapshot_id: Uuid },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkspaceOperationV1 {
    InsertNode {
        client_node_ref: String,
        parent_lineage_id: Option<Uuid>,
        ordinal: u64,
        title: String,
        semantic_role: String,
        render_role: String,
    },
    RenameNode {
        node_lineage_id: Uuid,
        title: String,
    },
    MoveNode {
        node_lineage_id: Uuid,
        parent_lineage_id: Option<Uuid>,
        ordinal: u64,
    },
    SplitNode {
        node_lineage_id: Uuid,
        titles: Vec<String>,
    },
    MergeNodes {
        node_lineage_ids: Vec<Uuid>,
        title: String,
    },
    DeleteNode {
        node_lineage_id: Uuid,
    },
    InsertBlock {
        node_lineage_id: Uuid,
        ordinal: u64,
        block: Value,
    },
    UpdateBlock {
        block_lineage_id: Uuid,
        block: Value,
    },
    MoveBlock {
        block_lineage_id: Uuid,
        target_node_lineage_id: Uuid,
        ordinal: u64,
    },
    DeleteBlock {
        block_lineage_id: Uuid,
    },
    InsertAssetBlock {
        node_lineage_id: Uuid,
        asset_revision_id: Uuid,
        ordinal: u64,
    },
    UpdateDocumentSettings {
        settings: DocumentSettings,
    },
    BindFulfillment {
        need_occurrence_id: Uuid,
        channel: String,
        requirement_projection_revision_id: Uuid,
        requirement_projection_sha256: String,
        target: BindingTargetV1,
        reason: String,
    },
    RemapFulfillment {
        binding_lineage_id: Uuid,
        need_occurrence_id: Uuid,
        channel: String,
        requirement_projection_revision_id: Uuid,
        requirement_projection_sha256: String,
        target: BindingTargetV1,
        reason: String,
    },
    UnbindFulfillment {
        binding_lineage_id: Uuid,
    },
}

pub fn validate_document_settings(value: &Value) -> Result<()> {
    let settings: DocumentSettings = serde_json::from_value(value.clone())
        .map_err(|error| invalid(format!("document settings schema invalid: {error}")))?;
    let margins = [
        settings.margins_mm.top,
        settings.margins_mm.right,
        settings.margins_mm.bottom,
        settings.margins_mm.left,
    ];
    if settings.page_size != "A4"
        || margins
            .iter()
            .any(|margin| !margin.is_finite() || !(5.0..=80.0).contains(margin))
        || !settings.body_font_pt.is_finite()
        || !(6.0..=48.0).contains(&settings.body_font_pt)
        || !settings.line_spacing.is_finite()
        || !(0.8..=4.0).contains(&settings.line_spacing)
        || !matches!(
            settings.heading_numbering.as_str(),
            "decimal" | "chinese" | "none"
        )
        || settings.header.chars().count() > 2_048
        || settings.footer.chars().count() > 2_048
        || !matches!(
            settings.page_number.as_str(),
            "none" | "footer_center" | "footer_outside"
        )
    {
        return Err(invalid(
            "document settings are outside the closed V1 contract",
        ));
    }
    Ok(())
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum WorkspaceMutationError {
    #[error("workspace identity does not match the mutation request")]
    WorkspaceIdentityMismatch,
    #[error("workspace CAS identity does not match the mutation request")]
    WorkspaceCasMismatch,
    #[error("workspace operation is invalid: {0}")]
    InvalidOperation(String),
    #[error("workspace node is missing: {0}")]
    MissingNode(String),
    #[error("workspace block is missing: {0}")]
    MissingBlock(String),
    #[error("workspace tree would contain a cycle")]
    TreeCycle,
}

type Result<T> = std::result::Result<T, WorkspaceMutationError>;

pub fn apply_workspace_operations(
    current: &Value,
    request: &WorkspaceMutationRequestV1,
) -> Result<Value> {
    if request.operations.is_empty() || request.operations.len() > 1_000 {
        return Err(invalid(
            "operations count is outside the closed V1 contract",
        ));
    }
    let mut client_refs = HashSet::new();
    let operations = request
        .operations
        .iter()
        .map(serde_json::to_value)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|error| invalid(format!("workspace operation serialization failed: {error}")))?
        .into_iter()
        .map(|mut operation| {
            if operation.get("kind").and_then(Value::as_str) == Some("insert_node") {
                let client_ref = string(&operation, "client_node_ref")?;
                if client_ref != client_ref.trim() || client_ref.chars().count() > 128 {
                    return Err(invalid("client_node_ref invalid"));
                }
                if !client_refs.insert(client_ref.to_owned()) {
                    return Err(invalid(
                        "client_node_ref must be unique within one mutation",
                    ));
                }
                let lineage = deterministic_node_identity(
                    request.workspace_id,
                    request.expected_workspace_revision_id,
                    client_ref,
                    b"lineage",
                );
                let revision = deterministic_node_identity(
                    request.workspace_id,
                    request.expected_workspace_revision_id,
                    client_ref,
                    b"revision",
                );
                let object = operation
                    .as_object_mut()
                    .ok_or_else(|| invalid("operation must be an object"))?;
                object.remove("client_node_ref");
                object.insert("lineage_id".into(), Value::String(lineage.to_string()));
                object.insert("revision_id".into(), Value::String(revision.to_string()));
            }
            Ok(operation)
        })
        .collect::<Result<Vec<_>>>()?;
    apply_workspace_operation_values(
        current,
        request.schema_version,
        request.workspace_id,
        request.expected_workspace_revision_id,
        &request.expected_workspace_sha256,
        &operations,
        true,
    )
}

fn deterministic_node_identity(
    workspace_id: Uuid,
    expected_revision_id: Uuid,
    client_ref: &str,
    purpose: &[u8],
) -> Uuid {
    let mut hasher = Sha256::new();
    hasher.update(b"knowledgebrain.bid.workspace-node.v1\0");
    hasher.update(workspace_id.as_bytes());
    hasher.update(expected_revision_id.as_bytes());
    hasher.update(purpose);
    hasher.update([0]);
    hasher.update(client_ref.as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

/// Applies identity-bearing operations produced only by a validated immutable Candidate.
/// This is deliberately separate from the public mutation DTO, which never accepts IDs.
pub fn apply_trusted_candidate_operations(
    current: &Value,
    workspace_id: Uuid,
    expected_workspace_revision_id: Uuid,
    expected_workspace_sha256: &str,
    operations: &[Value],
) -> Result<Value> {
    apply_workspace_operation_values(
        current,
        1,
        workspace_id,
        expected_workspace_revision_id,
        expected_workspace_sha256,
        operations,
        true,
    )
}

fn apply_workspace_operation_values(
    current: &Value,
    schema_version: u8,
    workspace_id: Uuid,
    expected_workspace_revision_id: Uuid,
    expected_workspace_sha256: &str,
    operations: &[Value],
    trusted_candidate: bool,
) -> Result<Value> {
    if operations.is_empty() || operations.len() > 1_000 {
        return Err(invalid(
            "operations count is outside the closed V1 contract",
        ));
    }
    if schema_version != 1
        || current.get("workspace_id").and_then(Value::as_str)
            != Some(workspace_id.to_string().as_str())
    {
        return Err(WorkspaceMutationError::WorkspaceIdentityMismatch);
    }
    if current
        .get("revision_id")
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        != Some(expected_workspace_revision_id)
        || current.get("sha256").and_then(Value::as_str) != Some(expected_workspace_sha256)
    {
        return Err(WorkspaceMutationError::WorkspaceCasMismatch);
    }

    let mut nodes = array(current, "nodes")?;
    let mut blocks = array(current, "blocks")?;
    let mut bindings = array(current, "bindings")?;
    let mut settings = current
        .get("document_settings")
        .cloned()
        .ok_or_else(|| invalid("document_settings missing"))?;
    let mut lineage_edges = Vec::new();

    for operation in operations {
        let kind = string(operation, "kind")?;
        let allowed: &[&str] = match kind {
            "insert_node" if trusted_candidate => &[
                "kind",
                "lineage_id",
                "revision_id",
                "parent_lineage_id",
                "ordinal",
                "title",
                "semantic_role",
                "render_role",
            ],
            "insert_node" => &[
                "kind",
                "client_node_ref",
                "parent_lineage_id",
                "ordinal",
                "title",
                "semantic_role",
                "render_role",
            ],
            "rename_node" => &["kind", "node_lineage_id", "title"],
            "move_node" => &["kind", "node_lineage_id", "parent_lineage_id", "ordinal"],
            "split_node" => &["kind", "node_lineage_id", "titles"],
            "merge_nodes" => &["kind", "node_lineage_ids", "title"],
            "delete_node" => &["kind", "node_lineage_id"],
            "insert_block" => &["kind", "node_lineage_id", "ordinal", "block"],
            "update_block" => &["kind", "block_lineage_id", "block"],
            "move_block" => &[
                "kind",
                "block_lineage_id",
                "target_node_lineage_id",
                "ordinal",
            ],
            "delete_block" => &["kind", "block_lineage_id"],
            "insert_asset_block" => &["kind", "node_lineage_id", "asset_revision_id", "ordinal"],
            "update_document_settings" => &["kind", "settings"],
            "bind_fulfillment" => &[
                "kind",
                "need_occurrence_id",
                "channel",
                "requirement_projection_revision_id",
                "requirement_projection_sha256",
                "target",
                "reason",
            ],
            "remap_fulfillment" => &[
                "kind",
                "binding_lineage_id",
                "need_occurrence_id",
                "channel",
                "requirement_projection_revision_id",
                "requirement_projection_sha256",
                "target",
                "reason",
            ],
            "unbind_fulfillment" => &["kind", "binding_lineage_id"],
            other => return Err(invalid(format!("unknown operation kind {other}"))),
        };
        exact_keys(operation, allowed)?;
        match kind {
            "insert_node" => insert_node(&mut nodes, operation)?,
            "rename_node" => rename_node(&mut nodes, operation)?,
            "move_node" => move_node(&mut nodes, operation)?,
            "split_node" => split_node(&mut nodes, &mut lineage_edges, operation)?,
            "merge_nodes" => merge_nodes(&mut nodes, &mut lineage_edges, operation)?,
            "delete_node" => delete_node(&mut nodes, &mut blocks, operation)?,
            "insert_block" => insert_block(&mut nodes, &mut blocks, operation)?,
            "update_block" => update_block(&mut blocks, operation)?,
            "move_block" => move_block(&mut nodes, operation)?,
            "delete_block" => delete_block(&mut nodes, &mut blocks, operation)?,
            "insert_asset_block" => insert_asset_block(&mut nodes, &mut blocks, operation)?,
            "update_document_settings" => {
                settings = operation
                    .get("settings")
                    .cloned()
                    .ok_or_else(|| invalid("settings missing"))?;
                validate_document_settings(&settings)?;
            }
            "bind_fulfillment" => {
                validate_projection_identity(current, operation)?;
                bind_fulfillment(&mut bindings, operation)?;
            }
            "remap_fulfillment" => {
                validate_projection_identity(current, operation)?;
                remap_fulfillment(&mut bindings, operation)?;
            }
            "unbind_fulfillment" => unbind_fulfillment(&mut bindings, operation)?,
            other => return Err(invalid(format!("unknown operation kind {other}"))),
        }
        normalize_tree(&mut nodes)?;
    }

    Ok(json!({
        "schema_version": 1,
        "document_settings": settings,
        "nodes": nodes,
        "blocks": blocks,
        "bindings": bindings,
        "lineage_edges": lineage_edges,
    }))
}

fn exact_keys(value: &Value, allowed: &[&str]) -> Result<()> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid("operation must be an object"))?;
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(invalid("operation contains unknown fields"));
    }
    Ok(())
}

fn validate_projection_identity(current: &Value, operation: &Value) -> Result<()> {
    let projection_id = uuid(operation, "requirement_projection_revision_id")?;
    let projection_sha = string(operation, "requirement_projection_sha256")?;
    if projection_sha.len() != 64
        || !projection_sha
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        || current
            .get("requirement_projection_revision_id")
            .and_then(Value::as_str)
            .and_then(|value| Uuid::parse_str(value).ok())
            != Some(projection_id)
        || current
            .get("requirement_projection_sha256")
            .and_then(Value::as_str)
            != Some(projection_sha)
    {
        return Err(invalid("requirement projection identity is not current"));
    }
    Ok(())
}

fn validate_binding_target(value: &Value) -> Result<()> {
    let kind = string(value, "kind")?;
    let allowed: &[&str] = match kind {
        "outline_node" => &["kind", "node_lineage_id"],
        "response_table" => &["kind", "block_lineage_id"],
        "structured_form" => &["kind", "form_definition_revision_id"],
        "quote" => &["kind", "quote_snapshot_id"],
        _ => return Err(invalid("binding target kind invalid")),
    };
    exact_keys(value, allowed)?;
    uuid(value, allowed[1])?;
    Ok(())
}

fn invalid(message: impl Into<String>) -> WorkspaceMutationError {
    WorkspaceMutationError::InvalidOperation(message.into())
}

fn array(value: &Value, field: &str) -> Result<Vec<Value>> {
    value
        .get(field)
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| invalid(format!("{field} must be an array")))
}

fn string<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| invalid(format!("{field} missing")))
}

fn bounded_title<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    let title = string(value, field)?.trim();
    if title.is_empty() || title.chars().count() > 1_024 {
        return Err(invalid(format!("{field} is empty or too long")));
    }
    Ok(title)
}

fn uuid(value: &Value, field: &str) -> Result<Uuid> {
    Uuid::parse_str(string(value, field)?).map_err(|_| invalid(format!("{field} invalid")))
}

fn optional_uuid(value: &Value, field: &str) -> Result<Option<Uuid>> {
    match value.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(raw)) => Uuid::parse_str(raw)
            .map(Some)
            .map_err(|_| invalid(format!("{field} invalid"))),
        _ => Err(invalid(format!("{field} invalid"))),
    }
}

fn ordinal(value: &Value, field: &str) -> Result<u64> {
    value
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid(format!("{field} invalid")))
}

fn node_index(nodes: &[Value], lineage_id: Uuid) -> Result<usize> {
    nodes
        .iter()
        .position(|node| {
            node.get("lineage_id").and_then(Value::as_str) == Some(lineage_id.to_string().as_str())
        })
        .ok_or_else(|| WorkspaceMutationError::MissingNode(lineage_id.to_string()))
}

fn block_index(blocks: &[Value], lineage_id: Uuid) -> Result<usize> {
    blocks
        .iter()
        .position(|block| {
            block.get("lineage_id").and_then(Value::as_str) == Some(lineage_id.to_string().as_str())
        })
        .ok_or_else(|| WorkspaceMutationError::MissingBlock(lineage_id.to_string()))
}

fn node_blocks(node: &Value) -> Result<Vec<Value>> {
    node.get("block_lineage_ids")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| invalid("node block_lineage_ids missing"))
}

fn set_node_blocks(node: &mut Value, blocks: Vec<Value>) -> Result<()> {
    node.as_object_mut()
        .ok_or_else(|| invalid("node must be an object"))?
        .insert("block_lineage_ids".into(), Value::Array(blocks));
    Ok(())
}

fn insert_node(nodes: &mut Vec<Value>, operation: &Value) -> Result<()> {
    if let Some(client_ref) = operation.get("client_node_ref") {
        let client_ref = client_ref
            .as_str()
            .filter(|value| !value.trim().is_empty() && value.chars().count() <= 128)
            .ok_or_else(|| invalid("client_node_ref invalid"))?;
        if client_ref != client_ref.trim() {
            return Err(invalid("client_node_ref invalid"));
        }
    }
    let parent = optional_uuid(operation, "parent_lineage_id")?;
    if let Some(parent) = parent {
        node_index(nodes, parent)?;
    }
    let title = bounded_title(operation, "title")?;
    if !matches!(
        string(operation, "semantic_role")?,
        "cover"
            | "toc"
            | "qualification"
            | "technical"
            | "commercial"
            | "quotation"
            | "deviation"
            | "implementation"
            | "evidence_index"
            | "attachment"
            | "other"
    ) || !matches!(
        string(operation, "render_role")?,
        "section" | "front_matter" | "toc" | "appendix" | "hidden"
    ) {
        return Err(invalid("node role is outside the closed V1 contract"));
    }
    let lineage_id = optional_uuid(operation, "lineage_id")?.unwrap_or_else(Uuid::new_v4);
    let revision_id = optional_uuid(operation, "revision_id")?.unwrap_or_else(Uuid::new_v4);
    if nodes.iter().any(|node| {
        node.get("lineage_id").and_then(Value::as_str) == Some(lineage_id.to_string().as_str())
            || node.get("revision_id").and_then(Value::as_str)
                == Some(revision_id.to_string().as_str())
    }) {
        return Err(invalid("node identity already exists"));
    }
    nodes.push(json!({
        "lineage_id": lineage_id,
        "revision_id": revision_id,
        "parent_lineage_id": parent,
        "ordinal": ordinal(operation, "ordinal")?,
        "title": title,
        "semantic_role": string(operation, "semantic_role")?,
        "render_role": string(operation, "render_role")?,
        "stale": false,
        "block_lineage_ids": [],
    }));
    Ok(())
}

fn rename_node(nodes: &mut [Value], operation: &Value) -> Result<()> {
    let index = node_index(nodes, uuid(operation, "node_lineage_id")?)?;
    let title = bounded_title(operation, "title")?;
    let object = nodes[index]
        .as_object_mut()
        .ok_or_else(|| invalid("node must be an object"))?;
    object.insert("title".into(), Value::String(title.into()));
    object.insert(
        "revision_id".into(),
        Value::String(Uuid::new_v4().to_string()),
    );
    Ok(())
}

fn move_node(nodes: &mut [Value], operation: &Value) -> Result<()> {
    let lineage = uuid(operation, "node_lineage_id")?;
    let parent = optional_uuid(operation, "parent_lineage_id")?;
    if parent == Some(lineage) {
        return Err(WorkspaceMutationError::TreeCycle);
    }
    if let Some(parent) = parent {
        node_index(nodes, parent)?;
    }
    let index = node_index(nodes, lineage)?;
    let object = nodes[index]
        .as_object_mut()
        .ok_or_else(|| invalid("node must be an object"))?;
    object.insert(
        "parent_lineage_id".into(),
        parent.map_or(Value::Null, |value| Value::String(value.to_string())),
    );
    object.insert(
        "ordinal".into(),
        Value::from(ordinal(operation, "ordinal")?),
    );
    object.insert(
        "revision_id".into(),
        Value::String(Uuid::new_v4().to_string()),
    );
    Ok(())
}

fn split_node(nodes: &mut Vec<Value>, edges: &mut Vec<Value>, operation: &Value) -> Result<()> {
    let source = uuid(operation, "node_lineage_id")?;
    let index = node_index(nodes, source)?;
    let old = nodes.remove(index);
    let titles = operation
        .get("titles")
        .and_then(Value::as_array)
        .filter(|values| (2..=100).contains(&values.len()))
        .ok_or_else(|| invalid("split titles invalid"))?;
    let parent = old.get("parent_lineage_id").cloned().unwrap_or(Value::Null);
    let base_ordinal = old.get("ordinal").and_then(Value::as_u64).unwrap_or(0);
    let semantic = old
        .get("semantic_role")
        .cloned()
        .unwrap_or_else(|| json!("other"));
    let render = old
        .get("render_role")
        .cloned()
        .unwrap_or_else(|| json!("section"));
    let blocks = node_blocks(&old)?;
    let mut replacements = Vec::new();
    for (offset, title) in titles.iter().enumerate() {
        let title = title
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty() && value.chars().count() <= 1_024)
            .ok_or_else(|| invalid("split title invalid"))?;
        let lineage = Uuid::new_v4();
        nodes.push(json!({
            "lineage_id": lineage,
            "revision_id": Uuid::new_v4(),
            "parent_lineage_id": parent,
            "ordinal": base_ordinal + offset as u64,
            "title": title,
            "semantic_role": semantic,
            "render_role": render,
            "stale": false,
            "block_lineage_ids": if offset == 0 { blocks.clone() } else { Vec::<Value>::new() },
        }));
        replacements.push(lineage);
        edges.push(json!({"kind":"split_from","from_lineage_id":source,"to_lineage_id":lineage}));
    }
    let first = replacements[0];
    for node in nodes.iter_mut() {
        if node.get("parent_lineage_id").and_then(Value::as_str)
            == Some(source.to_string().as_str())
        {
            node.as_object_mut()
                .ok_or_else(|| invalid("node must be object"))?
                .insert("parent_lineage_id".into(), Value::String(first.to_string()));
        }
    }
    Ok(())
}

fn merge_nodes(nodes: &mut Vec<Value>, edges: &mut Vec<Value>, operation: &Value) -> Result<()> {
    let ids = operation
        .get("node_lineage_ids")
        .and_then(Value::as_array)
        .filter(|values| (2..=100).contains(&values.len()))
        .ok_or_else(|| invalid("merge node_lineage_ids invalid"))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .and_then(|raw| Uuid::parse_str(raw).ok())
                .ok_or_else(|| invalid("merge lineage invalid"))
        })
        .collect::<Result<Vec<_>>>()?;
    if ids.iter().copied().collect::<HashSet<_>>().len() != ids.len() {
        return Err(invalid("merge node_lineage_ids must be unique"));
    }
    let first = nodes[node_index(nodes, ids[0])?].clone();
    let parent = first
        .get("parent_lineage_id")
        .cloned()
        .unwrap_or(Value::Null);
    let mut block_ids = Vec::new();
    let mut min_ordinal = u64::MAX;
    for id in &ids {
        let node = &nodes[node_index(nodes, *id)?];
        if node.get("parent_lineage_id") != Some(&parent) {
            return Err(invalid("merged nodes must share a parent"));
        }
        min_ordinal = min_ordinal.min(node.get("ordinal").and_then(Value::as_u64).unwrap_or(0));
        block_ids.extend(node_blocks(node)?);
    }
    let merged = Uuid::new_v4();
    nodes.retain(|node| {
        !node
            .get("lineage_id")
            .and_then(Value::as_str)
            .and_then(|raw| Uuid::parse_str(raw).ok())
            .is_some_and(|value| ids.contains(&value))
    });
    for node in nodes.iter_mut() {
        if node
            .get("parent_lineage_id")
            .and_then(Value::as_str)
            .and_then(|raw| Uuid::parse_str(raw).ok())
            .is_some_and(|value| ids.contains(&value))
        {
            node.as_object_mut().expect("node object validated").insert(
                "parent_lineage_id".into(),
                Value::String(merged.to_string()),
            );
        }
    }
    nodes.push(json!({
        "lineage_id": merged,
        "revision_id": Uuid::new_v4(),
        "parent_lineage_id": parent,
        "ordinal": min_ordinal,
        "title": bounded_title(operation, "title")?,
        "semantic_role": first.get("semantic_role").cloned().unwrap_or_else(|| json!("other")),
        "render_role": first.get("render_role").cloned().unwrap_or_else(|| json!("section")),
        "stale": false,
        "block_lineage_ids": block_ids,
    }));
    for source in ids {
        edges.push(json!({"kind":"merged_into","from_lineage_id":source,"to_lineage_id":merged}));
    }
    Ok(())
}

fn delete_node(nodes: &mut Vec<Value>, blocks: &mut Vec<Value>, operation: &Value) -> Result<()> {
    let root = uuid(operation, "node_lineage_id")?;
    node_index(nodes, root)?;
    let mut removed = HashSet::from([root]);
    loop {
        let before = removed.len();
        for node in nodes.iter() {
            let Some(lineage) = node
                .get("lineage_id")
                .and_then(Value::as_str)
                .and_then(|raw| Uuid::parse_str(raw).ok())
            else {
                continue;
            };
            let parent = node
                .get("parent_lineage_id")
                .and_then(Value::as_str)
                .and_then(|raw| Uuid::parse_str(raw).ok());
            if parent.is_some_and(|value| removed.contains(&value)) {
                removed.insert(lineage);
            }
        }
        if before == removed.len() {
            break;
        }
    }
    let removed_blocks: HashSet<Uuid> = nodes
        .iter()
        .filter(|node| {
            node.get("lineage_id")
                .and_then(Value::as_str)
                .and_then(|raw| Uuid::parse_str(raw).ok())
                .is_some_and(|value| removed.contains(&value))
        })
        .flat_map(|node| node_blocks(node).unwrap_or_default())
        .filter_map(|value| value.as_str().and_then(|raw| Uuid::parse_str(raw).ok()))
        .collect();
    nodes.retain(|node| {
        !node
            .get("lineage_id")
            .and_then(Value::as_str)
            .and_then(|raw| Uuid::parse_str(raw).ok())
            .is_some_and(|value| removed.contains(&value))
    });
    blocks.retain(|block| {
        !block
            .get("lineage_id")
            .and_then(Value::as_str)
            .and_then(|raw| Uuid::parse_str(raw).ok())
            .is_some_and(|value| removed_blocks.contains(&value))
    });
    Ok(())
}

fn insert_block(nodes: &mut [Value], blocks: &mut Vec<Value>, operation: &Value) -> Result<()> {
    let node = uuid(operation, "node_lineage_id")?;
    let node_index = node_index(nodes, node)?;
    let mut block = operation
        .get("block")
        .cloned()
        .ok_or_else(|| invalid("block missing"))?;
    crate::content_block::validate_content_block(&block).map_err(invalid)?;
    let lineage = uuid(&block, "lineage_id")?;
    if blocks
        .iter()
        .any(|value| value.get("lineage_id") == block.get("lineage_id"))
    {
        return Err(invalid("block lineage already exists"));
    }
    block
        .as_object_mut()
        .ok_or_else(|| invalid("block must be an object"))?
        .insert(
            "block_revision_id".into(),
            Value::String(Uuid::new_v4().to_string()),
        );
    blocks.push(block);
    let mut ids = node_blocks(&nodes[node_index])?;
    let at = usize::try_from(ordinal(operation, "ordinal")?).map_err(|_| invalid("ordinal"))?;
    ids.insert(at.min(ids.len()), Value::String(lineage.to_string()));
    set_node_blocks(&mut nodes[node_index], ids)
}

fn update_block(blocks: &mut [Value], operation: &Value) -> Result<()> {
    let lineage = uuid(operation, "block_lineage_id")?;
    let index = block_index(blocks, lineage)?;
    let old_revision = blocks[index]
        .get("revision")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let mut next = operation
        .get("block")
        .cloned()
        .ok_or_else(|| invalid("block missing"))?;
    crate::content_block::validate_content_block(&next).map_err(invalid)?;
    if uuid(&next, "lineage_id")? != lineage {
        return Err(invalid("updated block lineage changed"));
    }
    let object = next
        .as_object_mut()
        .ok_or_else(|| invalid("block must be an object"))?;
    object.insert(
        "block_revision_id".into(),
        Value::String(Uuid::new_v4().to_string()),
    );
    object.insert("revision".into(), Value::from(old_revision + 1));
    blocks[index] = next;
    Ok(())
}

fn move_block(nodes: &mut [Value], operation: &Value) -> Result<()> {
    let block = uuid(operation, "block_lineage_id")?;
    let target = uuid(operation, "target_node_lineage_id")?;
    let target_index = node_index(nodes, target)?;
    let mut found = false;
    for node in nodes.iter_mut() {
        let mut ids = node_blocks(node)?;
        let before = ids.len();
        ids.retain(|value| value.as_str() != Some(block.to_string().as_str()));
        found |= before != ids.len();
        set_node_blocks(node, ids)?;
    }
    if !found {
        return Err(WorkspaceMutationError::MissingBlock(block.to_string()));
    }
    let mut ids = node_blocks(&nodes[target_index])?;
    let at = usize::try_from(ordinal(operation, "ordinal")?).map_err(|_| invalid("ordinal"))?;
    ids.insert(at.min(ids.len()), Value::String(block.to_string()));
    set_node_blocks(&mut nodes[target_index], ids)
}

fn delete_block(nodes: &mut [Value], blocks: &mut Vec<Value>, operation: &Value) -> Result<()> {
    let lineage = uuid(operation, "block_lineage_id")?;
    block_index(blocks, lineage)?;
    blocks.retain(|block| {
        block.get("lineage_id").and_then(Value::as_str) != Some(lineage.to_string().as_str())
    });
    for node in nodes {
        let mut ids = node_blocks(node)?;
        ids.retain(|value| value.as_str() != Some(lineage.to_string().as_str()));
        set_node_blocks(node, ids)?;
    }
    Ok(())
}

fn insert_asset_block(
    nodes: &mut [Value],
    blocks: &mut Vec<Value>,
    operation: &Value,
) -> Result<()> {
    let lineage = Uuid::new_v4();
    let asset = uuid(operation, "asset_revision_id")?;
    let content = crate::content_block::BlockContent::Image {
        asset_revision_id: asset,
        width_mm: 120.0,
        alignment: crate::content_block::ImageAlignment::Center,
        crop: crate::content_block::Crop {
            left: 0.0,
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
        },
        caption: None,
        alt: String::new(),
    };
    let content_sha256 = content
        .sha256()
        .map_err(|error| invalid(error.to_string()))?;
    let synthetic = json!({
        "kind": "insert_block",
        "node_lineage_id": uuid(operation, "node_lineage_id")?,
        "ordinal": ordinal(operation, "ordinal")?,
        "block": {
            "schema_version": 1,
            "block_revision_id": Uuid::new_v4(),
            "lineage_id": lineage,
            "revision": 1,
            "kind": "image",
            "origin": "human",
            "content_sha256": content_sha256,
            "content": content
        }
    });
    insert_block(nodes, blocks, &synthetic)
}

fn validate_binding_fields(operation: &Value) -> Result<()> {
    if !matches!(
        string(operation, "channel")?,
        "narrative_content"
            | "response_table"
            | "deviation_statement"
            | "structured_form"
            | "evidence_attachment"
            | "quotation"
    ) {
        return Err(invalid("fulfillment channel invalid"));
    }
    let reason = string(operation, "reason")?;
    if reason.chars().count() > 4_096 {
        return Err(invalid("fulfillment reason invalid"));
    }
    Ok(())
}

fn bind_fulfillment(bindings: &mut Vec<Value>, operation: &Value) -> Result<()> {
    uuid(operation, "need_occurrence_id")?;
    validate_binding_fields(operation)?;
    validate_binding_target(
        operation
            .get("target")
            .ok_or_else(|| invalid("binding target missing"))?,
    )?;
    let mut binding = operation.clone();
    let object = binding
        .as_object_mut()
        .ok_or_else(|| invalid("binding must be object"))?;
    object.remove("kind");
    object.insert(
        "binding_lineage_id".into(),
        Value::String(Uuid::new_v4().to_string()),
    );
    object.insert(
        "binding_revision_id".into(),
        Value::String(Uuid::new_v4().to_string()),
    );
    object.insert("revision".into(), Value::from(1));
    object.insert("state".into(), Value::String("bound".into()));
    object.insert("stale".into(), Value::Bool(false));
    bindings.push(binding);
    Ok(())
}

fn remap_fulfillment(bindings: &mut [Value], operation: &Value) -> Result<()> {
    let lineage = uuid(operation, "binding_lineage_id")?;
    uuid(operation, "need_occurrence_id")?;
    validate_binding_fields(operation)?;
    validate_binding_target(
        operation
            .get("target")
            .ok_or_else(|| invalid("binding target missing"))?,
    )?;
    let index = bindings
        .iter()
        .position(|value| {
            value.get("binding_lineage_id").and_then(Value::as_str)
                == Some(lineage.to_string().as_str())
        })
        .ok_or_else(|| invalid("binding missing"))?;
    let revision = bindings[index]
        .get("revision")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        + 1;
    let mut next = operation.clone();
    let object = next
        .as_object_mut()
        .ok_or_else(|| invalid("binding must be object"))?;
    object.remove("kind");
    object.insert(
        "binding_revision_id".into(),
        Value::String(Uuid::new_v4().to_string()),
    );
    object.insert("revision".into(), Value::from(revision));
    object.insert("state".into(), Value::String("bound".into()));
    object.insert("stale".into(), Value::Bool(false));
    bindings[index] = next;
    Ok(())
}

fn unbind_fulfillment(bindings: &mut [Value], operation: &Value) -> Result<()> {
    let lineage = uuid(operation, "binding_lineage_id")?;
    let index = bindings
        .iter()
        .position(|value| {
            value.get("binding_lineage_id").and_then(Value::as_str)
                == Some(lineage.to_string().as_str())
        })
        .ok_or_else(|| invalid("binding missing"))?;
    let revision = bindings[index]
        .get("revision")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        + 1;
    let object = bindings[index]
        .as_object_mut()
        .ok_or_else(|| invalid("binding must be object"))?;
    object.insert(
        "binding_revision_id".into(),
        Value::String(Uuid::new_v4().to_string()),
    );
    object.insert("revision".into(), Value::from(revision));
    object.insert("state".into(), Value::String("unbound".into()));
    object.insert("reason".into(), Value::String("user_unbound".into()));
    object.insert("stale".into(), Value::Bool(false));
    Ok(())
}

fn normalize_tree(nodes: &mut [Value]) -> Result<()> {
    let ids: HashSet<Uuid> = nodes
        .iter()
        .map(|node| uuid(node, "lineage_id"))
        .collect::<Result<_>>()?;
    let parents: HashMap<Uuid, Option<Uuid>> = nodes
        .iter()
        .map(|node| {
            Ok((
                uuid(node, "lineage_id")?,
                optional_uuid(node, "parent_lineage_id")?,
            ))
        })
        .collect::<Result<_>>()?;
    for parent in parents.values().flatten() {
        if !ids.contains(parent) {
            return Err(WorkspaceMutationError::MissingNode(parent.to_string()));
        }
    }
    for id in &ids {
        let mut seen = HashSet::new();
        let mut cursor = Some(*id);
        while let Some(value) = cursor {
            if !seen.insert(value) {
                return Err(WorkspaceMutationError::TreeCycle);
            }
            cursor = parents.get(&value).copied().flatten();
        }
    }
    let mut groups: HashMap<Option<Uuid>, Vec<(usize, u64, Uuid)>> = HashMap::new();
    for (index, node) in nodes.iter().enumerate() {
        groups
            .entry(optional_uuid(node, "parent_lineage_id")?)
            .or_default()
            .push((
                index,
                node.get("ordinal").and_then(Value::as_u64).unwrap_or(0),
                uuid(node, "lineage_id")?,
            ));
    }
    for values in groups.values_mut() {
        values.sort_by_key(|(_, ordinal, lineage)| (*ordinal, *lineage));
        for (ordinal, (index, _, _)) in values.iter().enumerate() {
            nodes[*index]
                .as_object_mut()
                .ok_or_else(|| invalid("node must be object"))?
                .insert("ordinal".into(), Value::from(ordinal));
        }
    }
    let parents: HashMap<Uuid, Option<Uuid>> = nodes
        .iter()
        .map(|node| {
            Ok((
                uuid(node, "lineage_id")?,
                optional_uuid(node, "parent_lineage_id")?,
            ))
        })
        .collect::<Result<_>>()?;
    for node in nodes {
        let lineage = uuid(node, "lineage_id")?;
        let mut depth = 0_u64;
        let mut cursor = parents.get(&lineage).copied().flatten();
        while let Some(parent) = cursor {
            depth += 1;
            cursor = parents.get(&parent).copied().flatten();
        }
        node.as_object_mut()
            .ok_or_else(|| invalid("node must be object"))?
            .insert("depth".into(), Value::from(depth));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> Value {
        json!({
            "workspace_id":"00000000-0000-4000-8000-000000000001",
            "revision_id":"00000000-0000-4000-8000-000000000002",
            "sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "document_settings":{"page_size":"A4"},
            "nodes":[],"blocks":[],"bindings":[]
        })
    }

    fn request(operations: Vec<Value>) -> WorkspaceMutationRequestV1 {
        WorkspaceMutationRequestV1 {
            schema_version: 1,
            workspace_id: Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap(),
            expected_workspace_revision_id: Uuid::parse_str("00000000-0000-4000-8000-000000000002")
                .unwrap(),
            expected_workspace_sha256: "a".repeat(64),
            operations: operations
                .into_iter()
                .map(|operation| serde_json::from_value(operation).unwrap())
                .collect(),
        }
    }

    fn apply_trusted(operations: Vec<Value>) -> Result<Value> {
        apply_trusted_candidate_operations(
            &workspace(),
            Uuid::parse_str("00000000-0000-4000-8000-000000000001").unwrap(),
            Uuid::parse_str("00000000-0000-4000-8000-000000000002").unwrap(),
            &"a".repeat(64),
            &operations,
        )
    }

    #[test]
    fn insert_rename_and_move_preserve_lineage_and_reject_cycles() {
        let first = apply_workspace_operations(
            &workspace(),
            &request(vec![json!({
                "kind":"insert_node","client_node_ref":"n1","parent_lineage_id":null,
                "ordinal":0,"title":"技术方案","semantic_role":"technical","render_role":"section"
            })]),
        )
        .unwrap();
        let node = first["nodes"][0].clone();
        let lineage = node["lineage_id"].as_str().unwrap();
        let revision = node["revision_id"].as_str().unwrap();
        let mut current = workspace();
        current["nodes"] = first["nodes"].clone();
        let renamed = apply_workspace_operations(
            &current,
            &request(vec![json!({
                "kind":"rename_node","node_lineage_id":lineage,"title":"技术响应"
            })]),
        )
        .unwrap();
        assert_eq!(renamed["nodes"][0]["lineage_id"], lineage);
        assert_ne!(renamed["nodes"][0]["revision_id"], revision);
        current["nodes"] = renamed["nodes"].clone();
        let error = apply_workspace_operations(
            &current,
            &request(vec![json!({
                "kind":"move_node","node_lineage_id":lineage,
                "parent_lineage_id":lineage,"ordinal":0
            })]),
        )
        .unwrap_err();
        assert_eq!(error, WorkspaceMutationError::TreeCycle);
    }

    #[test]
    fn manual_authoring_round_trip_covers_tree_table_text_image_and_deletion() {
        fn block(lineage: Uuid, kind: &str, content: Value) -> Value {
            let typed: crate::content_block::BlockContent =
                serde_json::from_value(content).unwrap();
            json!({
                "schema_version":1,"block_revision_id":Uuid::new_v4(),"lineage_id":lineage,
                "revision":1,"kind":kind,"content":typed,"origin":"human",
                "content_sha256":typed.sha256().unwrap()
            })
        }

        let root = Uuid::new_v4();
        let child = Uuid::new_v4();
        let rich_lineage = Uuid::new_v4();
        let table_lineage = Uuid::new_v4();
        let asset = Uuid::new_v4();
        let rich = block(
            rich_lineage,
            "rich_text",
            json!({
                "type":"rich_text","nodes":[{"kind":"paragraph","content":[{"kind":"text","text":"人工响应","marks":[]}]}]
            }),
        );
        let updated_rich = block(
            rich_lineage,
            "rich_text",
            json!({
                "type":"rich_text","nodes":[{"kind":"paragraph","content":[{"kind":"text","text":"保存后的人工响应","marks":[]}]}]
            }),
        );
        let table = block(
            table_lineage,
            "table",
            json!({
                "type":"table","row_count":1,"column_count":1,
                "cells":[{"row":0,"column":0,"rowspan":1,"colspan":1,"content":[]}],
                "widths_mm":[100.0],"repeat_header_rows":0
            }),
        );
        let result = apply_trusted(vec![
            json!({"kind":"insert_node","lineage_id":root,
                "revision_id":Uuid::new_v4(),"parent_lineage_id":null,"ordinal":0,
                "title":"技术方案","semantic_role":"technical","render_role":"section"}),
            json!({"kind":"insert_node","lineage_id":child,
                "revision_id":Uuid::new_v4(),"parent_lineage_id":root,"ordinal":0,
                "title":"实施细节","semantic_role":"technical","render_role":"section"}),
            json!({"kind":"insert_block","node_lineage_id":root,"ordinal":0,"block":rich}),
            json!({"kind":"update_block","block_lineage_id":rich_lineage,"block":updated_rich}),
            json!({"kind":"insert_block","node_lineage_id":root,"ordinal":1,"block":table}),
            json!({"kind":"move_block","block_lineage_id":rich_lineage,
                "target_node_lineage_id":child,"ordinal":0}),
            json!({"kind":"move_node","node_lineage_id":child,"parent_lineage_id":null,"ordinal":1}),
            json!({"kind":"delete_block","block_lineage_id":table_lineage}),
            json!({"kind":"delete_node","node_lineage_id":child}),
            json!({"kind":"insert_asset_block","node_lineage_id":root,
                "asset_revision_id":asset,"ordinal":0})
        ]).unwrap();

        assert_eq!(result["nodes"].as_array().unwrap().len(), 1);
        assert_eq!(result["nodes"][0]["lineage_id"], root.to_string());
        assert_eq!(result["blocks"].as_array().unwrap().len(), 1);
        assert_eq!(result["blocks"][0]["kind"], "image");
        assert_eq!(
            result["blocks"][0]["content"]["asset_revision_id"],
            asset.to_string()
        );
    }

    #[test]
    fn manual_update_creates_a_new_block_revision_without_stored_stale_state() {
        let make_block = |lineage: Uuid, text: &str| {
            let content: crate::content_block::BlockContent = serde_json::from_value(json!({
                "type":"rich_text","nodes":[{"kind":"paragraph","content":[{
                    "kind":"text","text":text,"marks":[]}]}]
            }))
            .unwrap();
            json!({"schema_version":1,"block_revision_id":Uuid::new_v4(),"lineage_id":lineage,
                "revision":1,"kind":"rich_text","content":content,"origin":"human",
                "content_sha256":content.sha256().unwrap()})
        };
        let node = Uuid::new_v4();
        let lineage = Uuid::new_v4();
        let original = make_block(lineage, "候选内容");
        let edited = make_block(lineage, "人工修改候选内容");
        let result = apply_trusted(vec![
            json!({"kind":"insert_node","lineage_id":node,
                    "revision_id":Uuid::new_v4(),"parent_lineage_id":null,"ordinal":0,
                    "title":"技术方案","semantic_role":"technical","render_role":"section"}),
            json!({"kind":"insert_block","node_lineage_id":node,"ordinal":0,"block":original}),
            json!({"kind":"update_block","block_lineage_id":lineage,"block":edited}),
        ])
        .unwrap();
        assert_eq!(result["blocks"][0]["revision"], 2);
        assert!(result["blocks"][0].get("dependency_sha256").is_none());
        assert!(result["blocks"][0].get("stale").is_none());
    }

    #[test]
    fn public_workspace_operation_contract_rejects_integrity_and_anchor_fields() {
        let base = json!({
            "kind":"insert_node","client_node_ref":"root","parent_lineage_id":null,
            "ordinal":0,"title":"技术方案","semantic_role":"technical","render_role":"section"
        });
        assert!(serde_json::from_value::<WorkspaceOperationV1>(base.clone()).is_ok());
        for invalid in [
            json!({"kind":"insert_node","parent_lineage_id":null,"ordinal":0,
                "title":"技术方案","semantic_role":"technical","render_role":"section"}),
            json!({"kind":"insert_node","client_node_ref":"root","lineage_id":Uuid::new_v4(),
                "parent_lineage_id":null,"ordinal":0,"title":"技术方案",
                "semantic_role":"technical","render_role":"section"}),
            json!({"kind":"insert_node","client_node_ref":"root","revision_id":Uuid::new_v4(),
                "parent_lineage_id":null,"ordinal":0,"title":"技术方案",
                "semantic_role":"technical","render_role":"section"}),
            json!({"kind":"insert_block","node_lineage_id":Uuid::new_v4(),"ordinal":0,
                "block":{},"insertion_anchor":{"node_revision_id":Uuid::new_v4()}}),
            json!({"kind":"bind_fulfillment","need_occurrence_id":Uuid::new_v4(),
                "channel":"narrative_content","requirement_projection_revision_id":Uuid::new_v4(),
                "requirement_projection_sha256":"a".repeat(64),
                "target":{"kind":"outline_node","node_lineage_id":Uuid::new_v4()},
                "reason":"manual","state":"bound"}),
        ] {
            assert!(
                serde_json::from_value::<WorkspaceOperationV1>(invalid.clone()).is_err(),
                "accepted forged public operation {invalid}"
            );
        }
    }

    #[test]
    fn public_insert_generates_server_identities_and_binding_requires_projection_digest() {
        let inserted = apply_workspace_operations(
            &workspace(),
            &request(vec![json!({
                "kind":"insert_node","client_node_ref":"root","parent_lineage_id":null,
                "ordinal":0,"title":"技术方案","semantic_role":"technical","render_role":"section"
            })]),
        )
        .unwrap();
        assert!(Uuid::parse_str(inserted["nodes"][0]["lineage_id"].as_str().unwrap()).is_ok());
        assert!(Uuid::parse_str(inserted["nodes"][0]["revision_id"].as_str().unwrap()).is_ok());
        let replayed = apply_workspace_operations(
            &workspace(),
            &request(vec![json!({
                "kind":"insert_node","client_node_ref":"root","parent_lineage_id":null,
                "ordinal":0,"title":"技术方案","semantic_role":"technical","render_role":"section"
            })]),
        )
        .unwrap();
        assert_eq!(
            inserted["nodes"][0]["lineage_id"],
            replayed["nodes"][0]["lineage_id"]
        );
        assert_eq!(
            inserted["nodes"][0]["revision_id"],
            replayed["nodes"][0]["revision_id"]
        );

        let mut current = workspace();
        current["requirement_projection_revision_id"] = json!(Uuid::new_v4());
        current["requirement_projection_sha256"] = json!("b".repeat(64));
        let operation = json!({
            "kind":"bind_fulfillment","need_occurrence_id":Uuid::new_v4(),
            "channel":"narrative_content",
            "requirement_projection_revision_id":current["requirement_projection_revision_id"],
            "requirement_projection_sha256":"a".repeat(64),
            "target":{"kind":"outline_node","node_lineage_id":Uuid::new_v4()},
            "reason":"manual"
        });
        let error = apply_workspace_operations(&current, &request(vec![operation])).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("projection identity is not current")
        );
    }

    #[test]
    fn stale_cas_is_rejected_before_any_operation() {
        let mut request = request(vec![json!({
            "kind":"insert_node","client_node_ref":"stale","parent_lineage_id":null,
            "ordinal":0,"title":"stale","semantic_role":"other","render_role":"section"
        })]);
        request.expected_workspace_sha256 = "b".repeat(64);
        assert_eq!(
            apply_workspace_operations(&workspace(), &request).unwrap_err(),
            WorkspaceMutationError::WorkspaceCasMismatch
        );
    }
}
