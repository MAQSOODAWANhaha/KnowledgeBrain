use std::collections::{BTreeSet, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

const SEMANTIC_ROLES: &[&str] = &[
    "cover",
    "toc",
    "qualification",
    "technical",
    "commercial",
    "quotation",
    "deviation",
    "implementation",
    "evidence_index",
    "attachment",
    "other",
];
const RENDER_ROLES: &[&str] = &["section", "front_matter", "toc", "appendix", "hidden"];
const CHANNELS: &[&str] = &[
    "narrative_content",
    "response_table",
    "deviation_statement",
    "structured_form",
    "evidence_attachment",
    "quotation",
];
const NOTICE_CODES: &[&str] = &[
    "UNMAPPED_REQUIREMENT",
    "CONFLICTING_STRUCTURE",
    "LOW_CONFIDENCE",
    "UNRESOLVED_SOURCE",
    "FORM_STRUCTURE_DEVIATION",
    "EXCLUDED_NOT_APPLICABLE",
];
const SEVERITIES: &[&str] = &["info", "warning", "high"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineValidationError {
    pub code: &'static str,
    pub message: String,
}

impl OutlineValidationError {
    fn invalid(message: impl Into<String>) -> Self {
        Self {
            code: "AGENT_OUTPUT_INVALID",
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutlineGenerationOutputV2 {
    pub schema_version: u32,
    pub nodes: Vec<OutlineNodeV1>,
    pub bindings: Vec<OutlineBindingV1>,
    pub section_obligation_bindings: Vec<SectionObligationBindingV1>,
    pub notices: Vec<OutlineNoticeV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutlineNodeV1 {
    pub client_node_ref: String,
    pub parent_client_node_ref: Option<String>,
    pub ordinal: i64,
    pub title: String,
    pub semantic_role: String,
    pub render_role: String,
    pub origin_source_unit_revision_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutlineBindingV1 {
    pub need_occurrence_id: Uuid,
    pub channel: String,
    pub target_client_node_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SectionObligationBindingV1 {
    pub obligation_id: String,
    pub target_client_node_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutlineNoticeV1 {
    pub code: String,
    pub severity: String,
    pub message: String,
    pub source_identity: String,
}

#[derive(Debug, Clone)]
struct ExpectedNeed {
    channel: String,
    mandatory: bool,
}

fn valid_client_ref(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn default_channel(kind: &str) -> &'static str {
    match kind {
        "pricing" => "quotation",
        "deviation" => "deviation_statement",
        "form" => "structured_form",
        _ => "narrative_content",
    }
}

fn expected_needs(input: &Value) -> Result<HashMap<Uuid, ExpectedNeed>, OutlineValidationError> {
    let mut out = HashMap::new();
    for requirement in input
        .get("requirements")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let mandatory =
            requirement.get("requiredness").and_then(Value::as_str) == Some("mandatory");
        let fallback_channel = default_channel(
            requirement
                .get("requirement_kind")
                .and_then(Value::as_str)
                .unwrap_or(""),
        );
        if let Some(needs) = requirement
            .get("need_occurrences")
            .and_then(Value::as_array)
        {
            for need in needs {
                let id = need
                    .get("need_occurrence_id")
                    .and_then(Value::as_str)
                    .and_then(|raw| Uuid::parse_str(raw).ok())
                    .ok_or_else(|| {
                        OutlineValidationError::invalid("frozen need occurrence id invalid")
                    })?;
                let channel = need
                    .get("channel")
                    .and_then(Value::as_str)
                    .unwrap_or(fallback_channel);
                if !CHANNELS.contains(&channel) {
                    return Err(OutlineValidationError::invalid(
                        "frozen need channel invalid",
                    ));
                }
                if out
                    .insert(
                        id,
                        ExpectedNeed {
                            channel: channel.to_owned(),
                            mandatory,
                        },
                    )
                    .is_some()
                {
                    return Err(OutlineValidationError::invalid(
                        "duplicate frozen need occurrence id",
                    ));
                }
            }
            continue;
        }
        let Some(id) = requirement
            .get("need_occurrence_id")
            .and_then(Value::as_str)
            .and_then(|raw| Uuid::parse_str(raw).ok())
        else {
            return Err(OutlineValidationError::invalid(
                "frozen requirement need occurrence missing",
            ));
        };
        if out
            .insert(
                id,
                ExpectedNeed {
                    channel: fallback_channel.to_owned(),
                    mandatory,
                },
            )
            .is_some()
        {
            return Err(OutlineValidationError::invalid(
                "duplicate frozen need occurrence id",
            ));
        }
    }
    Ok(out)
}

fn validate_tree(
    nodes: &[OutlineNodeV1],
    allowed_sources: &HashSet<Uuid>,
    structural_sources: &HashSet<Uuid>,
) -> Result<HashSet<String>, OutlineValidationError> {
    if nodes.is_empty() || nodes.len() > 1000 {
        return Err(OutlineValidationError::invalid(
            "outline nodes must contain 1..1000 items",
        ));
    }
    let mut by_ref = HashMap::new();
    for (index, node) in nodes.iter().enumerate() {
        if !valid_client_ref(&node.client_node_ref) {
            return Err(OutlineValidationError::invalid("invalid client_node_ref"));
        }
        if by_ref.insert(node.client_node_ref.clone(), index).is_some() {
            return Err(OutlineValidationError::invalid("duplicate client_node_ref"));
        }
        if node.title.is_empty() || node.title.chars().count() > 1024 {
            return Err(OutlineValidationError::invalid(
                "outline node title length invalid",
            ));
        }
        if !SEMANTIC_ROLES.contains(&node.semantic_role.as_str())
            || !RENDER_ROLES.contains(&node.render_role.as_str())
        {
            return Err(OutlineValidationError::invalid("outline node role invalid"));
        }
        if node.ordinal < 0 {
            return Err(OutlineValidationError::invalid(
                "outline node ordinal invalid",
            ));
        }
        if node.origin_source_unit_revision_ids.is_empty() {
            return Err(OutlineValidationError::invalid(
                "outline node must cite frozen evidence",
            ));
        }
        let origins: HashSet<_> = node
            .origin_source_unit_revision_ids
            .iter()
            .copied()
            .collect();
        if origins.len() != node.origin_source_unit_revision_ids.len()
            || origins.iter().any(|id| !allowed_sources.contains(id))
        {
            return Err(OutlineValidationError::invalid(
                "outline node cites duplicate or out-of-scope source",
            ));
        }
    }
    let roots: Vec<_> = nodes
        .iter()
        .filter(|node| node.parent_client_node_ref.is_none())
        .collect();
    if roots.len() != 1 {
        return Err(OutlineValidationError::invalid(
            "outline tree must have exactly one root",
        ));
    }
    let root_ref = roots[0].client_node_ref.clone();
    let mut children: HashMap<Option<String>, Vec<&OutlineNodeV1>> = HashMap::new();
    for node in nodes {
        if let Some(parent) = &node.parent_client_node_ref
            && (parent == &node.client_node_ref || !by_ref.contains_key(parent))
        {
            return Err(OutlineValidationError::invalid(
                "outline parent is missing or self-referential",
            ));
        }
        children
            .entry(node.parent_client_node_ref.clone())
            .or_default()
            .push(node);
    }
    for siblings in children.values_mut() {
        siblings.sort_by_key(|node| node.ordinal);
        if siblings
            .iter()
            .enumerate()
            .any(|(expected, node)| node.ordinal != expected as i64)
        {
            return Err(OutlineValidationError::invalid(
                "outline sibling ordinals must be contiguous from zero",
            ));
        }
    }
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    fn walk(
        node_ref: &str,
        children: &HashMap<Option<String>, Vec<&OutlineNodeV1>>,
        visiting: &mut HashSet<String>,
        visited: &mut HashSet<String>,
    ) -> Result<(), OutlineValidationError> {
        if visited.contains(node_ref) {
            return Ok(());
        }
        if !visiting.insert(node_ref.to_owned()) {
            return Err(OutlineValidationError::invalid(
                "outline tree contains a cycle",
            ));
        }
        if let Some(items) = children.get(&Some(node_ref.to_owned())) {
            for child in items {
                walk(&child.client_node_ref, children, visiting, visited)?;
            }
        }
        visiting.remove(node_ref);
        visited.insert(node_ref.to_owned());
        Ok(())
    }
    walk(&root_ref, &children, &mut visiting, &mut visited)?;
    if visited.len() != nodes.len() {
        return Err(OutlineValidationError::invalid(
            "outline tree contains unreachable nodes or a disconnected cycle",
        ));
    }
    for node in nodes
        .iter()
        .filter(|node| node.parent_client_node_ref.as_deref() == Some(root_ref.as_str()))
    {
        if !node
            .origin_source_unit_revision_ids
            .iter()
            .any(|id| structural_sources.contains(id))
        {
            return Err(OutlineValidationError {
                code: "STRUCTURE_EVIDENCE_INSUFFICIENT",
                message: format!(
                    "top-level node {} lacks structure evidence",
                    node.client_node_ref
                ),
            });
        }
    }
    Ok(by_ref.into_keys().collect())
}

fn validate_requirement_closure(
    bindings: &[OutlineBindingV1],
    notices: &[OutlineNoticeV1],
    node_refs: &HashSet<String>,
    expected: &HashMap<Uuid, ExpectedNeed>,
) -> Result<(), OutlineValidationError> {
    if bindings.len() > 100_000 || notices.len() > 10_000 {
        return Err(OutlineValidationError::invalid(
            "outline bindings or notices exceed contract limits",
        ));
    }
    let mut bound = HashMap::new();
    for binding in bindings {
        let Some(need) = expected.get(&binding.need_occurrence_id) else {
            return Err(OutlineValidationError::invalid(
                "binding references unknown frozen need",
            ));
        };
        if binding.channel != need.channel || !CHANNELS.contains(&binding.channel.as_str()) {
            return Err(OutlineValidationError::invalid(
                "binding channel does not match frozen need",
            ));
        }
        if !node_refs.contains(&binding.target_client_node_ref) {
            return Err(OutlineValidationError::invalid(
                "binding target node missing",
            ));
        }
        if bound
            .insert(binding.need_occurrence_id, &binding.target_client_node_ref)
            .is_some()
        {
            return Err(OutlineValidationError::invalid(
                "duplicate or conflicting need binding",
            ));
        }
    }
    let mut unmapped = HashSet::new();
    for notice in notices {
        if !NOTICE_CODES.contains(&notice.code.as_str())
            || !SEVERITIES.contains(&notice.severity.as_str())
            || notice.message.is_empty()
            || notice.message.chars().count() > 4096
            || notice.source_identity.is_empty()
            || notice.source_identity.len() > 256
        {
            return Err(OutlineValidationError::invalid("outline notice invalid"));
        }
        if notice.code == "UNMAPPED_REQUIREMENT" {
            let id = Uuid::parse_str(&notice.source_identity).map_err(|_| {
                OutlineValidationError::invalid("unmapped notice must identify a frozen need UUID")
            })?;
            let Some(need) = expected.get(&id) else {
                return Err(OutlineValidationError::invalid(
                    "unmapped notice references unknown frozen need",
                ));
            };
            if need.mandatory {
                return Err(OutlineValidationError {
                    code: "AGENT_REQUIREMENT_CLOSURE_FAILED",
                    message: "mandatory frozen need cannot use UNMAPPED_REQUIREMENT".to_owned(),
                });
            }
            if !unmapped.insert(id) {
                return Err(OutlineValidationError::invalid(
                    "duplicate unmapped requirement notice",
                ));
            }
        }
    }
    if bound.keys().any(|id| unmapped.contains(id)) {
        return Err(OutlineValidationError::invalid(
            "need cannot be both bound and unmapped",
        ));
    }
    let disposed: HashSet<_> = bound
        .keys()
        .copied()
        .chain(unmapped.iter().copied())
        .collect();
    let missing: BTreeSet<_> = expected
        .keys()
        .filter(|id| !disposed.contains(id))
        .copied()
        .collect();
    if !missing.is_empty() {
        return Err(OutlineValidationError::invalid(format!(
            "{} frozen needs lack binding or unmapped notice",
            missing.len()
        )));
    }
    Ok(())
}

fn sha256_text(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn uuid_set(value: Option<&Value>) -> HashSet<Uuid> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(|raw| Uuid::parse_str(raw).ok())
        .collect()
}

fn reduce_structural_sources(reduce: &Value) -> HashSet<Uuid> {
    reduce
        .get("structure_fragments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|fragment| uuid_set(fragment.get("source_unit_revision_ids")))
        .collect()
}

fn semantic_failure(message: impl Into<String>) -> OutlineValidationError {
    OutlineValidationError {
        code: "AGENT_SEMANTIC_VALIDATION_FAILED",
        message: message.into(),
    }
}

fn spine_node_ref(section_ref: &str) -> String {
    format!("spine_{}", &section_ref[..section_ref.len().min(24)])
}

fn validate_semantic_spine_and_obligations(
    output: &OutlineGenerationOutputV2,
    reduce: &Value,
    expected: &HashMap<Uuid, ExpectedNeed>,
) -> Result<(), OutlineValidationError> {
    let spine = reduce
        .get("composition_spine")
        .and_then(Value::as_object)
        .ok_or_else(|| semantic_failure("Reduce V3 composition spine is missing"))?;
    let sections = spine
        .get("sections")
        .and_then(Value::as_array)
        .filter(|values| values.len() >= 2)
        .ok_or_else(|| semantic_failure("composition spine sections are missing"))?;
    let by_ref = output
        .nodes
        .iter()
        .map(|node| (node.client_node_ref.clone(), node))
        .collect::<HashMap<_, _>>();
    let parent_by_ref = output
        .nodes
        .iter()
        .map(|node| {
            (
                node.client_node_ref.clone(),
                node.parent_client_node_ref.clone(),
            )
        })
        .collect::<HashMap<_, _>>();
    let root = by_ref
        .get("root")
        .filter(|node| {
            node.parent_client_node_ref.is_none()
                && node.semantic_role == "cover"
                && node.render_role == "front_matter"
                && node.title
                    == spine
                        .get("root_title")
                        .and_then(Value::as_str)
                        .unwrap_or("")
        })
        .ok_or_else(|| semantic_failure("deterministic cover root does not match spine"))?;
    let mut top = output
        .nodes
        .iter()
        .filter(|node| node.parent_client_node_ref.as_deref() == Some(&root.client_node_ref))
        .collect::<Vec<_>>();
    top.sort_by_key(|node| node.ordinal);
    if top.len() != sections.len() + 1
        || top.first().is_none_or(|node| {
            node.client_node_ref != "toc"
                || node.semantic_role != "toc"
                || node.render_role != "toc"
        })
    {
        return Err(semantic_failure(
            "top level must contain only TOC followed by composition spine sections",
        ));
    }
    for (ordinal, section) in sections.iter().enumerate() {
        let section_ref = section
            .get("section_ref")
            .and_then(Value::as_str)
            .ok_or_else(|| semantic_failure("composition section ref invalid"))?;
        let node = top[ordinal + 1];
        if node.client_node_ref != spine_node_ref(section_ref)
            || node.title != section.get("title").and_then(Value::as_str).unwrap_or("")
            || node.semantic_role
                != section
                    .get("semantic_role")
                    .and_then(Value::as_str)
                    .unwrap_or("other")
            || node.render_role != "section"
            || uuid_set(section.get("source_unit_revision_ids")).is_disjoint(
                &node
                    .origin_source_unit_revision_ids
                    .iter()
                    .copied()
                    .collect(),
            )
        {
            return Err(semantic_failure(format!(
                "top-level section {} does not match CompositionSpineV1",
                ordinal
            )));
        }
        let has_descendant = output.nodes.iter().any(|candidate| {
            if candidate.client_node_ref == node.client_node_ref {
                return false;
            }
            let mut current = Some(candidate.client_node_ref.as_str());
            for _ in 0..=output.nodes.len() {
                match current {
                    Some(value) if value == node.client_node_ref => return true,
                    Some(value) => {
                        current = parent_by_ref
                            .get(value)
                            .and_then(|parent| parent.as_deref())
                    }
                    None => return false,
                }
            }
            false
        });
        if !has_descendant {
            return Err(semantic_failure(format!(
                "top-level section {} has no evidence-backed child",
                node.client_node_ref
            )));
        }
    }

    let source_numberings = reduce
        .get("structure_fragments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|fragment| fragment.get("source_numbering").and_then(Value::as_str))
        .filter(|numbering| !numbering.trim().is_empty())
        .collect::<HashSet<_>>();
    for node in &output.nodes {
        let title = node.title.trim_start();
        for numbering in &source_numberings {
            if let Some(rest) = title.strip_prefix(numbering)
                && rest.chars().next().is_some_and(|character| {
                    character.is_whitespace() || matches!(character, '、' | '.' | '．' | ':' | '：')
                })
            {
                return Err(semantic_failure(format!(
                    "node {} leaked frozen source numbering into its semantic title",
                    node.client_node_ref
                )));
            }
        }
    }
    for fragment in reduce
        .get("structure_fragments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|fragment| {
            matches!(
                fragment.get("outline_usage").and_then(Value::as_str),
                Some("requirement_context" | "reference_only")
            )
        })
    {
        let Some(title) = fragment.get("title").and_then(Value::as_str) else {
            continue;
        };
        let sources = uuid_set(fragment.get("source_unit_revision_ids"));
        if output.nodes.iter().any(|node| {
            node.title == title
                && !sources.is_disjoint(
                    &node
                        .origin_source_unit_revision_ids
                        .iter()
                        .copied()
                        .collect(),
                )
        }) {
            return Err(semantic_failure(
                "requirement context/reference fragment was promoted into the output outline",
            ));
        }
    }

    let groups = reduce
        .get("fulfillment_groups")
        .and_then(Value::as_array)
        .ok_or_else(|| semantic_failure("Reduce V3 fulfillment groups are missing"))?
        .iter()
        .map(|group| {
            let group_ref = group
                .get("group_ref")
                .and_then(Value::as_str)
                .filter(|value| sha256_text(value))
                .ok_or_else(|| semantic_failure("fulfillment group ref invalid"))?;
            Ok((group_ref.to_owned(), group))
        })
        .collect::<Result<HashMap<_, _>, OutlineValidationError>>()?;
    let mut obligations = HashMap::<String, (String, &Value, bool, bool)>::new();
    for section in reduce
        .pointer("/section_obligation_matrix/sections")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let section_ref = section
            .get("section_ref")
            .and_then(Value::as_str)
            .ok_or_else(|| semantic_failure("obligation section ref invalid"))?
            .to_owned();
        for (key, required, excluded) in [
            ("required_group_refs", true, false),
            ("conditional_group_refs", false, false),
            ("excluded_group_refs", false, true),
        ] {
            for group_ref in section
                .get(key)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                if !sha256_text(group_ref) {
                    return Err(semantic_failure("matrix fulfillment group ref invalid"));
                }
                let group = groups.get(group_ref).ok_or_else(|| {
                    semantic_failure("matrix references unknown fulfillment group")
                })?;
                if group.get("section_ref").and_then(Value::as_str) != Some(&section_ref)
                    || excluded
                        != (group.get("applicability").and_then(Value::as_str)
                            == Some("not_applicable"))
                    || (required
                        && group.get("requiredness").and_then(Value::as_str) != Some("mandatory"))
                {
                    return Err(semantic_failure(
                        "matrix fulfillment group classification conflicts with frozen group",
                    ));
                }
                if obligations
                    .insert(
                        group_ref.to_owned(),
                        (section_ref.clone(), *group, required, excluded),
                    )
                    .is_some()
                {
                    return Err(semantic_failure("duplicate matrix fulfillment group ref"));
                }
            }
        }
    }
    if obligations.len() != groups.len() {
        return Err(semantic_failure(
            "every fulfillment group must appear exactly once in Matrix V2",
        ));
    }
    if output.section_obligation_bindings.len() > 100_000 {
        return Err(semantic_failure("too many section obligation bindings"));
    }
    let mut bound = HashMap::<String, &SectionObligationBindingV1>::new();
    for binding in &output.section_obligation_bindings {
        if !sha256_text(&binding.obligation_id)
            || !valid_client_ref(&binding.target_client_node_ref)
        {
            return Err(semantic_failure("section obligation binding invalid"));
        }
        let (section_ref, group, _, excluded) = obligations
            .get(&binding.obligation_id)
            .ok_or_else(|| semantic_failure("binding references unknown fulfillment group"))?;
        if *excluded || group.get("materialization").and_then(Value::as_str) == Some("audit_only") {
            return Err(semantic_failure(
                "excluded fulfillment group cannot be bound",
            ));
        }
        let target = by_ref
            .get(&binding.target_client_node_ref)
            .ok_or_else(|| semantic_failure("fulfillment group target node missing"))?;
        let section_node_ref = spine_node_ref(section_ref);
        if target.client_node_ref == section_node_ref {
            return Err(semantic_failure(
                "fulfillment group cannot bind directly to its top-level section",
            ));
        }
        let mut current = Some(target.client_node_ref.as_str());
        let mut descendant = false;
        for _ in 0..=output.nodes.len() {
            match current {
                Some(value) if value == section_node_ref => {
                    descendant = true;
                    break;
                }
                Some(value) => {
                    current = parent_by_ref
                        .get(value)
                        .and_then(|parent| parent.as_deref())
                }
                None => break,
            }
        }
        if !descendant
            || uuid_set(group.get("source_unit_revision_ids")).is_disjoint(
                &target
                    .origin_source_unit_revision_ids
                    .iter()
                    .copied()
                    .collect(),
            )
        {
            return Err(semantic_failure(
                "fulfillment group target lacks section ancestry or shared frozen evidence",
            ));
        }
        if bound
            .insert(binding.obligation_id.clone(), binding)
            .is_some()
        {
            return Err(semantic_failure(
                "fulfillment group was bound more than once",
            ));
        }
    }
    if obligations
        .iter()
        .any(|(id, (_, _, required, _))| *required && !bound.contains_key(id))
    {
        return Err(OutlineValidationError {
            code: "AGENT_OBLIGATION_COVERAGE_FAILED",
            message: "required SectionObligationMatrixV2 group is not bound".to_owned(),
        });
    }
    let routes = output
        .bindings
        .iter()
        .map(|binding| (binding.need_occurrence_id, binding))
        .collect::<HashMap<_, _>>();
    let mut grouped_needs = HashMap::<Uuid, (String, String)>::new();
    for (group_ref, (_, group, required, excluded)) in &obligations {
        for occurrence in group
            .get("need_occurrences")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let need = occurrence
                .get("need_occurrence_id")
                .and_then(Value::as_str)
                .and_then(|raw| Uuid::parse_str(raw).ok())
                .ok_or_else(|| semantic_failure("grouped need identity invalid"))?;
            let channel = occurrence
                .get("channel")
                .and_then(Value::as_str)
                .ok_or_else(|| semantic_failure("grouped need channel invalid"))?;
            if grouped_needs
                .insert(need, (group_ref.clone(), channel.to_owned()))
                .is_some()
            {
                return Err(semantic_failure(
                    "need occurrence belongs to more than one fulfillment group",
                ));
            }
            if *required && !*excluded {
                let frozen = expected
                    .get(&need)
                    .ok_or_else(|| semantic_failure("group contains non-frozen need occurrence"))?;
                if frozen.channel != channel {
                    return Err(semantic_failure(
                        "grouped need channel conflicts with frozen requirement",
                    ));
                }
            }
        }
    }
    for (need, requirement) in expected.iter().filter(|(_, need)| need.mandatory) {
        let route = routes.get(need).ok_or_else(|| OutlineValidationError {
            code: "AGENT_REQUIREMENT_CLOSURE_FAILED",
            message: format!("mandatory need {need} is not routed"),
        })?;
        let (group_ref, channel) =
            grouped_needs
                .get(need)
                .ok_or_else(|| OutlineValidationError {
                    code: "AGENT_REQUIREMENT_CLOSURE_FAILED",
                    message: format!("mandatory need {need} is not in a fulfillment group"),
                })?;
        if channel != &requirement.channel
            || bound.get(group_ref).is_none_or(|binding| {
                binding.target_client_node_ref != route.target_client_node_ref
            })
        {
            return Err(OutlineValidationError {
                code: "AGENT_REQUIREMENT_CLOSURE_FAILED",
                message: format!(
                    "mandatory need {need} is not closed by its matching fulfillment group"
                ),
            });
        }
    }
    Ok(())
}

pub fn validate_outline_output(
    input: &Value,
    reduce: &Value,
    payload: Value,
) -> Result<Value, OutlineValidationError> {
    let output: OutlineGenerationOutputV2 = serde_json::from_value(payload).map_err(|error| {
        OutlineValidationError::invalid(format!("outline output schema invalid: {error}"))
    })?;
    if output.schema_version != 2 {
        return Err(OutlineValidationError::invalid(
            "outline schema_version must be 2",
        ));
    }
    let allowed_sources: HashSet<Uuid> = input
        .get("source_units")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|unit| unit.get("source_unit_revision_id").and_then(Value::as_str))
        .filter_map(|raw| Uuid::parse_str(raw).ok())
        .collect();
    let node_refs = validate_tree(
        &output.nodes,
        &allowed_sources,
        &reduce_structural_sources(reduce),
    )?;
    let expected = expected_needs(input)?;
    validate_requirement_closure(&output.bindings, &output.notices, &node_refs, &expected)?;
    validate_semantic_spine_and_obligations(&output, reduce, &expected)?;
    serde_json::to_value(output).map_err(|error| OutlineValidationError::invalid(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const COMMERCIAL_REF: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const TECHNICAL_REF: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const COMMERCIAL_OBLIGATION: &str =
        "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    const TECHNICAL_OBLIGATION: &str =
        "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn input() -> Value {
        json!({
            "source_units": [
                {"source_unit_revision_id": id(1).to_string()},
                {"source_unit_revision_id": id(2).to_string()}
            ],
            "requirements": [{
                "requirement_revision_id":id(3),
                "requirement_text":"必须逐条响应技术要求",
                "requirement_kind":"technical",
                "requiredness":"mandatory",
                "source_unit_revision_ids":[id(2)],
                "need_occurrences":[{
                    "need_occurrence_id":id(3),"channel":"narrative_content"
                }]
            }]
        })
    }

    fn reduce() -> Value {
        json!({
            "schema_version":3,
            "composition_spine":{
                "schema_version":1,"root_title":"投标文件",
                "root_source_unit_revision_ids":[id(1),id(2)],
                "sections":[
                    {"section_ref":COMMERCIAL_REF,"title":"商务文件","semantic_role":"commercial","ordinal":0,"source_numbering":"3.1.1","applicability":"required","evidence_kind":"explicit_composition_clause","confidence":"high","source_unit_revision_ids":[id(1)]},
                    {"section_ref":TECHNICAL_REF,"title":"技术文件","semantic_role":"technical","ordinal":1,"source_numbering":"3.1.2","applicability":"required","evidence_kind":"explicit_composition_clause","confidence":"high","source_unit_revision_ids":[id(2)]}
                ]
            },
            "fulfillment_groups":[
                {"group_ref":COMMERCIAL_OBLIGATION,"group_key":"bid-letter","title":"投标函","section_ref":COMMERCIAL_REF,"semantic_role":"commercial","materialization":"explicit_child","requiredness":"mandatory","applicability":"required","need_occurrences":[],"source_unit_revision_ids":[id(1)],"structured_form_revision_ids":[],"fragment_refs":[]},
                {"group_ref":TECHNICAL_OBLIGATION,"group_key":"technical-response","title":"技术要求响应","section_ref":TECHNICAL_REF,"semantic_role":"technical","materialization":"explicit_child","requiredness":"mandatory","applicability":"required","need_occurrences":[{"need_occurrence_id":id(3),"channel":"narrative_content"}],"source_unit_revision_ids":[id(2)],"structured_form_revision_ids":[],"fragment_refs":[]}
            ],
            "section_obligation_matrix":{
                "schema_version":2,"sections":[
                    {"section_ref":COMMERCIAL_REF,"required_group_refs":[COMMERCIAL_OBLIGATION],"conditional_group_refs":[],"excluded_group_refs":[]},
                    {"section_ref":TECHNICAL_REF,"required_group_refs":[TECHNICAL_OBLIGATION],"conditional_group_refs":[],"excluded_group_refs":[]}
                ]
            },
            "structure_fragments":[
                {"title":"商务文件","outline_usage":"composition_spine","source_numbering":"3.1.1","source_unit_revision_ids":[id(1)]},
                {"title":"技术文件","outline_usage":"composition_spine","source_numbering":"3.1.2","source_unit_revision_ids":[id(2)]},
                {"title":"编制说明","outline_usage":"requirement_context","source_numbering":"第六章","source_unit_revision_ids":[id(1)]}
            ]
        })
    }

    fn valid() -> Value {
        json!({
            "schema_version":2,
            "nodes":[
                {"client_node_ref":"root","parent_client_node_ref":null,"ordinal":0,"title":"投标文件","semantic_role":"cover","render_role":"front_matter","origin_source_unit_revision_ids":[id(1),id(2)]},
                {"client_node_ref":"toc","parent_client_node_ref":"root","ordinal":0,"title":"目录","semantic_role":"toc","render_role":"toc","origin_source_unit_revision_ids":[id(1),id(2)]},
                {"client_node_ref":"spine_aaaaaaaaaaaaaaaaaaaaaaaa","parent_client_node_ref":"root","ordinal":1,"title":"商务文件","semantic_role":"commercial","render_role":"section","origin_source_unit_revision_ids":[id(1)]},
                {"client_node_ref":"spine_bbbbbbbbbbbbbbbbbbbbbbbb","parent_client_node_ref":"root","ordinal":2,"title":"技术文件","semantic_role":"technical","render_role":"section","origin_source_unit_revision_ids":[id(2)]},
                {"client_node_ref":"commercial_letter","parent_client_node_ref":"spine_aaaaaaaaaaaaaaaaaaaaaaaa","ordinal":0,"title":"投标函","semantic_role":"commercial","render_role":"section","origin_source_unit_revision_ids":[id(1)]},
                {"client_node_ref":"technical_response","parent_client_node_ref":"spine_bbbbbbbbbbbbbbbbbbbbbbbb","ordinal":0,"title":"技术要求响应","semantic_role":"technical","render_role":"section","origin_source_unit_revision_ids":[id(2)]}
            ],
            "bindings":[{"need_occurrence_id":id(3),"channel":"narrative_content","target_client_node_ref":"technical_response"}],
            "section_obligation_bindings":[
                {"obligation_id":COMMERCIAL_OBLIGATION,"target_client_node_ref":"commercial_letter"},
                {"obligation_id":TECHNICAL_OBLIGATION,"target_client_node_ref":"technical_response"}
            ],
            "notices":[]
        })
    }

    #[test]
    fn valid_semantic_outline_passes() {
        assert!(validate_outline_output(&input(), &reduce(), valid()).is_ok());
    }

    #[test]
    fn disconnected_cycle_and_bad_ordinal_fail() {
        let mut cycle = valid();
        cycle["nodes"].as_array_mut().unwrap().extend([
            json!({"client_node_ref":"a","parent_client_node_ref":"b","ordinal":0,"title":"A","semantic_role":"other","render_role":"section","origin_source_unit_revision_ids":[id(1)]}),
            json!({"client_node_ref":"b","parent_client_node_ref":"a","ordinal":0,"title":"B","semantic_role":"other","render_role":"section","origin_source_unit_revision_ids":[id(1)]})
        ]);
        assert!(validate_outline_output(&input(), &reduce(), cycle).is_err());
        let mut ordinal = valid();
        ordinal["nodes"][2]["ordinal"] = json!(8);
        assert!(validate_outline_output(&input(), &reduce(), ordinal).is_err());
    }

    #[test]
    fn mandatory_need_cannot_be_unmapped_even_at_high_severity() {
        let mut value = valid();
        value["bindings"] = json!([]);
        value["notices"] = json!([{"code":"UNMAPPED_REQUIREMENT","severity":"high","message":"未映射","source_identity":id(3)}]);
        assert!(validate_outline_output(&input(), &reduce(), value).is_err());
    }

    #[test]
    fn missing_obligation_and_wrong_top_level_order_fail() {
        let mut missing = valid();
        missing["section_obligation_bindings"]
            .as_array_mut()
            .unwrap()
            .pop();
        assert!(validate_outline_output(&input(), &reduce(), missing).is_err());
        let mut order = valid();
        order["nodes"][2]["title"] = json!("技术文件");
        assert!(validate_outline_output(&input(), &reduce(), order).is_err());
    }

    #[test]
    fn source_numbering_and_context_promotion_fail() {
        let mut numbered = valid();
        numbered["nodes"][2]["title"] = json!("3.1.1 商务文件");
        assert!(validate_outline_output(&input(), &reduce(), numbered).is_err());
        let mut context = valid();
        context["nodes"][4]["title"] = json!("编制说明");
        assert!(validate_outline_output(&input(), &reduce(), context).is_err());
    }

    #[test]
    fn out_of_scope_and_missing_structure_source_fail() {
        let mut source = valid();
        source["nodes"][4]["origin_source_unit_revision_ids"] = json!([id(99)]);
        assert!(validate_outline_output(&input(), &reduce(), source).is_err());
        let mut no_structure = reduce();
        no_structure["structure_fragments"] = json!([]);
        assert!(validate_outline_output(&input(), &no_structure, valid()).is_err());
    }
}
