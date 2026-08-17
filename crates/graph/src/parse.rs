//! Brain `chat_pipeline` Formater: fenced JSON of entity / entity1 / entity2 / relation.

use crate::prompt::{FewShotNode, FewShotRel};
use serde_json::{Value, json};

pub struct ExtractedNode {
    pub name: String,
    pub attributes: Vec<String>,
}

pub struct ExtractedRel {
    pub node1: String,
    pub node2: String,
    pub rel_type: String,
}

#[derive(Default)]
pub struct GraphData {
    pub nodes: Vec<ExtractedNode>,
    pub relations: Vec<ExtractedRel>,
}

const NODE_PREFIX: &str = "entity";
const ATTR_SUFFIX: &str = "_attributes";
const REL_SOURCE: &str = "entity1";
const REL_TARGET: &str = "entity2";
const REL_PREFIX: &str = "relation";

pub fn format_extraction(nodes: &[FewShotNode], rels: &[FewShotRel]) -> String {
    let mut items = Vec::new();
    for n in nodes {
        let mut item = json!({ NODE_PREFIX: n.name });
        if !n.attributes.is_empty() {
            item[format!("{NODE_PREFIX}{ATTR_SUFFIX}")] = json!(n.attributes);
        }
        items.push(item);
    }
    for r in rels {
        items.push(json!({
            REL_SOURCE: r.node1,
            REL_TARGET: r.node2,
            REL_PREFIX: r.rel_type,
        }));
    }
    let body = serde_json::to_string_pretty(&items).unwrap_or_else(|_| "[]".into());
    format!("```json\n{body}\n```")
}

pub fn parse_graph(text: &str) -> Result<GraphData, String> {
    let content = extract_content(text);
    if content.trim().is_empty() {
        return Err("empty or invalid input string".into());
    }
    let parsed: Value =
        serde_json::from_str(&content).map_err(|e| format!("failed to parse JSON content: {e}"))?;
    let items: Vec<Value> = match parsed {
        Value::Array(a) => a,
        Value::Object(_) => vec![parsed],
        _ => return Err("expected list or dict".into()),
    };
    let mut data = GraphData::default();
    for group in items {
        let Some(obj) = group.as_object() else {
            return Err("each item in the sequence must be a mapping".into());
        };
        if let Some(name) = obj.get(NODE_PREFIX) {
            let attributes = obj
                .get(&format!("{NODE_PREFIX}{ATTR_SUFFIX}"))
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .map(|x| match x {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        })
                        .collect()
                })
                .unwrap_or_default();
            data.nodes.push(ExtractedNode {
                name: value_as_string(name),
                attributes,
            });
        } else if let (Some(src), Some(dst)) = (obj.get(REL_SOURCE), obj.get(REL_TARGET)) {
            data.relations.push(ExtractedRel {
                node1: value_as_string(src),
                node2: value_as_string(dst),
                rel_type: obj
                    .get(REL_PREFIX)
                    .map(value_as_string)
                    .unwrap_or_else(|| "RELATES_TO".into()),
            });
        }
    }
    rebuild_graph(&mut data);
    Ok(data)
}

fn rebuild_graph(graph: &mut GraphData) {
    let mut seen = std::collections::HashSet::new();
    let mut nodes = Vec::new();
    for n in graph.nodes.drain(..) {
        if seen.insert(n.name.clone()) {
            nodes.push(n);
        }
    }
    let mut relations = Vec::new();
    for r in graph.relations.drain(..) {
        if r.node1 == r.node2 {
            continue;
        }
        if seen.insert(r.node1.clone()) {
            nodes.push(ExtractedNode {
                name: r.node1.clone(),
                attributes: Vec::new(),
            });
        }
        if seen.insert(r.node2.clone()) {
            nodes.push(ExtractedNode {
                name: r.node2.clone(),
                attributes: Vec::new(),
            });
        }
        relations.push(r);
    }
    graph.nodes = nodes;
    graph.relations = relations;
}

fn extract_content(text: &str) -> String {
    let trimmed = text.trim();
    if let Some(start) = trimmed.find("```") {
        let after = &trimmed[start + 3..];
        let body = if let Some(nl) = after.find('\n') {
            &after[nl + 1..]
        } else {
            after
        };
        if let Some(end) = body.find("```") {
            return body[..end].trim().to_string();
        }
        return strip_trailing_fences(body);
    }
    if let Some(i) = trimmed.find('[') {
        return recover_json(&trimmed[i..]);
    }
    if let Some(i) = trimmed.find('{') {
        return recover_json(&trimmed[i..]);
    }
    trimmed.to_string()
}

fn strip_trailing_fences(body: &str) -> String {
    body.trim().trim_end_matches('`').trim().to_string()
}

fn recover_json(s: &str) -> String {
    let t = s.trim();
    if t.starts_with('[')
        && let Some(end) = t.rfind(']')
    {
        return t[..=end].to_string();
    }
    if t.starts_with('{')
        && let Some(end) = t.rfind('}')
    {
        return t[..=end].to_string();
    }
    t.to_string()
}

fn value_as_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

pub fn stub_extract_json(content: &str) -> String {
    let names = local_entities(content);
    let mut items = Vec::new();
    for n in &names {
        items.push(json!({ NODE_PREFIX: n }));
    }
    if names.len() >= 2 {
        items.push(json!({
            REL_SOURCE: names[0],
            REL_TARGET: names[1],
            REL_PREFIX: "RELATES_TO",
        }));
    }
    format!(
        "```json\n{}\n```",
        serde_json::to_string_pretty(&items).unwrap_or_else(|_| "[]".into())
    )
}

fn local_entities(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for tok in text.split(|c: char| !c.is_alphanumeric() && c != '_') {
        if tok.chars().count() >= 3 && !out.iter().any(|x: &String| x == tok) {
            out.push(tok.to_string());
        }
        if out.len() >= 8 {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fenced_entity_json() {
        let raw = r#"```json
[
  {"entity": "William Shakespeare", "entity_attributes": ["Playwright"]},
  {"entity": "Romeo and Juliet"},
  {"entity1": "William Shakespeare", "entity2": "Romeo and Juliet", "relation": "Author"}
]
```"#;
        let g = parse_graph(raw).expect("parse");
        assert!(g.nodes.iter().any(|n| n.name == "William Shakespeare"));
        assert_eq!(g.relations[0].rel_type, "Author");
    }

    #[test]
    fn recovers_unfenced_array() {
        let raw = r#"Here you go: [{"entity": "Alpha"}] thanks"#;
        let g = parse_graph(raw).expect("parse");
        assert_eq!(g.nodes[0].name, "Alpha");
    }
}
