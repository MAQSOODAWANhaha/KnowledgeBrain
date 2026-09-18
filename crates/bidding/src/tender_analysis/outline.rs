//! Bid-submission outline preview. Not a composition basis; draft_plan is.
use super::*;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OutlineNode {
    pub title: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<OutlineNode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OutlineDocument {
    pub id: String,
    pub file_name: String,
    pub parse_status: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source: Vec<OutlineNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TenderOutline {
    pub quality: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compile_status: Option<String>,
    pub extracted_from: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extracted: Vec<OutlineNode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub documents: Vec<OutlineDocument>,
}

#[derive(Debug, Deserialize)]
struct Bundle {
    compile_status: Option<String>,
    extracted_from: String,
    #[serde(default)]
    documents: Vec<BundleDocument>,
    #[serde(default)]
    records: BTreeMap<String, Value>,
    #[serde(default)]
    draft_plan: Vec<crate::tender_analysis::draft::DraftPlanItem>,
}

#[derive(Debug, Deserialize)]
struct BundleDocument {
    id: String,
    file_name: String,
    parse_status: String,
    #[serde(default)]
    headings: Vec<Heading>,
}

#[derive(Debug, Deserialize)]
struct Heading {
    heading_path: String,
}

pub fn from_bundle(value: Value) -> Result<TenderOutline, String> {
    let bundle: Bundle = serde_json::from_value(value).map_err(|e| e.to_string())?;
    let records = parse_records(&bundle.records);
    Ok(TenderOutline {
        quality: "draft".into(),
        compile_status: bundle.compile_status.filter(|s| !s.is_empty()),
        extracted_from: bundle.extracted_from,
        extracted: {
            let plan = plan_tree(&bundle.draft_plan);
            if plan.is_empty() {
                extracted(&records)
            } else {
                plan
            }
        },
        documents: bundle
            .documents
            .into_iter()
            .map(|document| OutlineDocument {
                id: document.id,
                file_name: document.file_name,
                parse_status: document.parse_status,
                source: source_tree(
                    document
                        .headings
                        .iter()
                        .map(|heading| heading.heading_path.as_str()),
                ),
            })
            .collect(),
    })
}

fn parse_records(raw: &BTreeMap<String, Value>) -> BTreeMap<String, Record> {
    let mut records = BTreeMap::new();
    for (id, value) in raw {
        if let Ok(record) = serde_json::from_value::<Record>(value.clone()) {
            records.insert(id.clone(), record);
        }
    }
    records
}

pub fn source_tree<'a>(paths: impl IntoIterator<Item = &'a str>) -> Vec<OutlineNode> {
    let mut unique = Vec::new();
    for path in paths {
        let path = path.trim();
        if path.is_empty() {
            continue;
        }
        if unique.last().is_some_and(|last: &String| last == path) {
            continue;
        }
        unique.push(path.to_string());
    }
    let mut roots = Vec::new();
    for path in unique {
        let parts: Vec<String> = path
            .split(" > ")
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(str::to_string)
            .collect();
        insert_path(&mut roots, &parts);
    }
    roots
}

fn insert_path(nodes: &mut Vec<OutlineNode>, parts: &[String]) {
    let Some(title) = parts.first() else {
        return;
    };
    if let Some(existing) = nodes.iter_mut().find(|node| node.title == *title) {
        insert_path(&mut existing.children, &parts[1..]);
        return;
    }
    let mut node = OutlineNode {
        title: title.clone(),
        kind: "heading".into(),
        children: Vec::new(),
        record_id: None,
    };
    insert_path(&mut node.children, &parts[1..]);
    nodes.push(node);
}

pub fn plan_tree(plan: &[crate::tender_analysis::draft::DraftPlanItem]) -> Vec<OutlineNode> {
    let live: Vec<_> = plan
        .iter()
        .filter(|item| item.status != crate::tender_analysis::draft::DraftStatus::Omitted)
        .collect();
    if live.is_empty() {
        return Vec::new();
    }
    let ids: BTreeSet<_> = live.iter().map(|item| item.id.clone()).collect();
    let mut children: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut roots = Vec::new();
    let mut ordered = live;
    ordered.sort_by(|a, b| a.order.cmp(&b.order).then(a.id.cmp(&b.id)));
    for item in &ordered {
        match &item.parent {
            Some(parent) if ids.contains(parent) && parent != &item.id => {
                children
                    .entry(parent.clone())
                    .or_default()
                    .push(item.id.clone());
            }
            _ => roots.push(item.id.clone()),
        }
    }
    let titles: BTreeMap<_, _> = ordered
        .iter()
        .map(|item| (item.id.clone(), item.title.clone()))
        .collect();
    roots
        .into_iter()
        .filter_map(|id| plan_node(&id, &titles, &children, &mut BTreeSet::new()))
        .collect()
}

fn plan_node(
    id: &str,
    titles: &BTreeMap<String, String>,
    children: &BTreeMap<String, Vec<String>>,
    stack: &mut BTreeSet<String>,
) -> Option<OutlineNode> {
    if !stack.insert(id.to_string()) {
        return None;
    }
    let title = titles.get(id)?.clone();
    let node = OutlineNode {
        title,
        kind: "chapter".into(),
        children: children
            .get(id)
            .into_iter()
            .flatten()
            .filter_map(|child| plan_node(child, titles, children, stack))
            .collect(),
        record_id: Some(id.to_string()),
    };
    stack.remove(id);
    Some(node)
}

pub fn extracted(records: &BTreeMap<String, Record>) -> Vec<OutlineNode> {
    let templates = template_tree(records);
    if !templates.is_empty() {
        return templates;
    }
    composition_items(records)
}

fn template_tree(records: &BTreeMap<String, Record>) -> Vec<OutlineNode> {
    let mut items = Vec::new();
    for record in records.values() {
        let RecordData::Template {
            label,
            title,
            parent,
            order,
            ..
        } = &record.data
        else {
            continue;
        };
        let title = if title.trim().is_empty() {
            label.clone()
        } else {
            title.clone()
        };
        if title.trim().is_empty() {
            continue;
        }
        items.push((
            record.id.clone(),
            title,
            parent.clone(),
            order.unwrap_or(usize::MAX),
        ));
    }
    items.sort_by(|a, b| a.3.cmp(&b.3).then(a.1.cmp(&b.1)).then(a.0.cmp(&b.0)));
    let ids: BTreeSet<_> = items.iter().map(|item| item.0.clone()).collect();
    let mut children: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut roots = Vec::new();
    for (id, _, parent, _) in &items {
        match parent {
            Some(parent) if ids.contains(parent) && parent != id => {
                children.entry(parent.clone()).or_default().push(id.clone());
            }
            _ => roots.push(id.clone()),
        }
    }
    let reachable = reachable(&roots, &children);
    for (id, _, _, _) in &items {
        if !reachable.contains(id) {
            roots.push(id.clone());
            children.remove(id);
        }
    }
    let titles: BTreeMap<_, _> = items
        .iter()
        .map(|(id, title, _, _)| (id.clone(), title.clone()))
        .collect();
    roots
        .into_iter()
        .filter_map(|id| template_node(&id, &titles, &children, &mut BTreeSet::new()))
        .collect()
}

fn reachable(roots: &[String], children: &BTreeMap<String, Vec<String>>) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut stack = roots.to_vec();
    while let Some(id) = stack.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        if let Some(next) = children.get(&id) {
            stack.extend(next.iter().cloned());
        }
    }
    seen
}

fn template_node(
    id: &str,
    titles: &BTreeMap<String, String>,
    children: &BTreeMap<String, Vec<String>>,
    stack: &mut BTreeSet<String>,
) -> Option<OutlineNode> {
    if !stack.insert(id.to_string()) {
        return None;
    }
    let title = titles.get(id)?.clone();
    let node = OutlineNode {
        title,
        kind: "template".into(),
        children: children
            .get(id)
            .into_iter()
            .flatten()
            .filter_map(|child| template_node(child, titles, children, stack))
            .collect(),
        record_id: Some(id.to_string()),
    };
    stack.remove(id);
    Some(node)
}

fn composition_items(records: &BTreeMap<String, Record>) -> Vec<OutlineNode> {
    let mut nodes = Vec::new();
    for record in records.values() {
        let RecordData::Rule { items, .. } = &record.data else {
            continue;
        };
        for item in items {
            if item.kind != RuleItemKind::Composition || item.text.trim().is_empty() {
                continue;
            }
            nodes.push(OutlineNode {
                title: item.text.clone(),
                kind: "composition".into(),
                children: Vec::new(),
                record_id: Some(format!("{}:{}", record.id, item.id)),
            });
        }
    }
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn applicable() -> Applicability {
        Applicability {
            state: ApplicabilityState::Applicable,
            condition: String::new(),
            scope: "项目".into(),
            grounds: vec![],
        }
    }

    fn template(id: &str, title: &str, parent: Option<&str>, order: Option<usize>) -> Record {
        Record {
            id: id.into(),
            sources: vec![],
            data: RecordData::Template {
                label: id.into(),
                title: title.into(),
                parent: parent.map(str::to_owned),
                order,
                purpose: "提交".into(),
                applicability: applicable(),
                regions: vec![],
            },
        }
    }

    #[test]
    fn nested_heading_paths_collapse_and_keep_order() {
        let tree = source_tree([
            "第一章 须知",
            "第一章 须知",
            "第一章 须知 > 1.1 总则",
            "第二章 格式",
        ]);
        assert_eq!(
            tree,
            vec![
                OutlineNode {
                    title: "第一章 须知".into(),
                    kind: "heading".into(),
                    children: vec![OutlineNode {
                        title: "1.1 总则".into(),
                        kind: "heading".into(),
                        children: vec![],
                        record_id: None,
                    }],
                    record_id: None,
                },
                OutlineNode {
                    title: "第二章 格式".into(),
                    kind: "heading".into(),
                    children: vec![],
                    record_id: None,
                },
            ]
        );
    }

    #[test]
    fn empty_heading_paths_are_not_an_outline() {
        assert!(source_tree(["", "   "]).is_empty());
    }

    #[test]
    fn templates_nest_by_parent_and_order() {
        let mut records = BTreeMap::new();
        records.insert(
            "child".into(),
            template("child", "授权书", Some("root"), Some(2)),
        );
        records.insert("root".into(), template("root", "投标文件", None, Some(1)));
        records.insert(
            "other".into(),
            template("other", "报价表", Some("root"), Some(1)),
        );
        assert_eq!(
            extracted(&records),
            vec![OutlineNode {
                title: "投标文件".into(),
                kind: "template".into(),
                record_id: Some("root".into()),
                children: vec![
                    OutlineNode {
                        title: "报价表".into(),
                        kind: "template".into(),
                        children: vec![],
                        record_id: Some("other".into()),
                    },
                    OutlineNode {
                        title: "授权书".into(),
                        kind: "template".into(),
                        children: vec![],
                        record_id: Some("child".into()),
                    },
                ],
            }]
        );
    }

    #[test]
    fn composition_items_are_used_when_templates_are_absent() {
        let mut records = BTreeMap::new();
        records.insert(
            "rule".into(),
            Record {
                id: "rule".into(),
                sources: vec![],
                data: RecordData::Rule {
                    text: "组成".into(),
                    scope: "项目".into(),
                    applicability: applicable(),
                    items: vec![RuleItem {
                        id: "i1".into(),
                        kind: RuleItemKind::Composition,
                        text: "投标函".into(),
                        grounds: vec![],
                        condition: String::new(),
                        targets: vec![],
                        sequence: vec![],
                        format_key: None,
                        format_value: None,
                    }],
                },
            },
        );
        assert_eq!(extracted(&records)[0].title, "投标函");
        records.insert("t".into(), template("t", "投标函格式", None, Some(1)));
        assert_eq!(extracted(&records)[0].kind, "template");
    }

    #[test]
    fn cyclic_template_parents_still_emit_nodes() {
        let mut records = BTreeMap::new();
        records.insert("a".into(), template("a", "甲", Some("b"), Some(1)));
        records.insert("b".into(), template("b", "乙", Some("a"), Some(2)));
        let tree = extracted(&records);
        assert_eq!(tree.len(), 2);
        assert!(tree.iter().all(|node| node.children.is_empty()));
    }

    #[test]
    fn bundle_preview_is_always_draft() {
        let outline = from_bundle(json!({
            "compile_status": "pending",
            "extracted_from": "checkpoint",
            "documents": [{"id":"d","file_name":"招标文件.docx","parse_status":"ready",
                "headings":[{"ordinal":0,"kind":"section","heading_path":"须知","page_ordinal":null}]}],
            "records": {"t": template("t", "投标函", None, Some(1))}
        }))
        .unwrap();
        assert_eq!(outline.quality, "draft");
        assert_eq!(outline.extracted_from, "checkpoint");
        assert_eq!(outline.extracted[0].title, "投标函");
        assert_eq!(outline.documents[0].source[0].title, "须知");
    }

    #[test]
    fn draft_plan_tree_nests_by_parent_not_heading_path() {
        let outline = from_bundle(json!({
            "compile_status": "succeeded",
            "extracted_from": "checkpoint",
            "documents": [{"id":"d","file_name":"招标文件.pdf","parse_status":"ready",
                "headings":[{"ordinal":0,"kind":"section","heading_path":"须知 > 8. 包装","page_ordinal":null}]}],
            "records": {},
            "draft_plan": [
                {"id":"vol","parent":null,"order":0,"title":"技术投标文件","prescribed":true,"source_ids":[],"windows":[],"window_index":0,"status":"pending"},
                {"id":"ch8","parent":"vol","order":8,"title":"8. 包装及运输","prescribed":true,"source_ids":[],"windows":[],"window_index":0,"status":"pending"},
                {"id":"ch81","parent":"ch8","order":1,"title":"8.1 大件运输","prescribed":true,"source_ids":[],"windows":[],"window_index":0,"status":"pending"}
            ]
        }))
        .unwrap();
        assert_eq!(outline.extracted[0].title, "技术投标文件");
        assert_eq!(outline.extracted[0].kind, "chapter");
        assert_eq!(outline.extracted[0].children[0].title, "8. 包装及运输");
        assert_eq!(
            outline.extracted[0].children[0].children[0].title,
            "8.1 大件运输"
        );
        assert_eq!(outline.documents[0].source[0].title, "须知");
    }
}
