//! ContentGenerateV2 job body: evidence, Agent turn, candidate verify, publish.
//! Worker adapters call [`execute`].

use platform::{ContentGenerateJobV2, ContentGenerateOperationV2};
use sqlx::PgPool;
use uuid::Uuid;

pub fn stable_candidate_uuid(parts: &[&str]) -> Uuid {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    for part in parts {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part.as_bytes());
    }
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest.finalize()[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct GeneratedEvidenceRange {
    start: usize,
    end: usize,
    bundle: String,
    item: String,
}

fn collect_generated_evidence_ranges(
    nodes: &[crate::content_block::RichNode],
    offset: &mut usize,
    ranges: &mut Vec<GeneratedEvidenceRange>,
) -> Result<(), String> {
    use crate::content_block::{Inline, ListItem, Paragraph, RichNode, TextMark};
    fn inlines(
        values: &[Inline],
        offset: &mut usize,
        ranges: &mut Vec<GeneratedEvidenceRange>,
    ) -> Result<(), String> {
        for value in values {
            if let Inline::Text { text, marks } = value {
                let start = *offset;
                *offset += text.len();
                let end = *offset;
                if marks
                    .iter()
                    .any(|mark| matches!(mark, TextMark::Code | TextMark::Link { .. }))
                {
                    return Err("generated Content blocks forbid code and link marks".into());
                }
                let refs = marks
                    .iter()
                    .filter_map(|mark| {
                        if let TextMark::EvidenceRef {
                            evidence_bundle_id,
                            evidence_item_id,
                            ..
                        } = mark
                        {
                            Some(GeneratedEvidenceRange {
                                start,
                                end,
                                bundle: evidence_bundle_id.to_string(),
                                item: evidence_item_id.to_string(),
                            })
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();
                if !text.trim().is_empty()
                    && refs.is_empty()
                    && !text.trim_start().starts_with("【待人工补充】")
                    && !text.trim_start().starts_with("[NO_EVIDENCE]")
                {
                    return Err("generated text without evidence_ref must be an explicit no-evidence placeholder".into());
                }
                ranges.extend(refs);
            }
        }
        Ok(())
    }
    for node in nodes {
        match node {
            RichNode::Paragraph { content } => inlines(content, offset, ranges)?,
            RichNode::HorizontalRule => {}
            RichNode::CodeBlock { .. } => {
                return Err("generated Content blocks forbid code blocks".into());
            }
            RichNode::Blockquote { content } => {
                for paragraph in content {
                    let Paragraph::Paragraph { content } = paragraph;
                    inlines(content, offset, ranges)?;
                }
            }
            RichNode::BulletList { content } | RichNode::OrderedList { content } => {
                for item in content {
                    let ListItem::ListItem { content } = item;
                    for paragraph in content {
                        let Paragraph::Paragraph { content } = paragraph;
                        inlines(content, offset, ranges)?;
                    }
                }
            }
        }
    }
    Ok(())
}

fn block_generated_evidence_ranges(
    block: &crate::content_block::BlockContent,
) -> Result<(usize, Vec<GeneratedEvidenceRange>), String> {
    let mut offset = 0;
    let mut ranges = Vec::new();
    match block {
        crate::content_block::BlockContent::RichText { nodes } => {
            collect_generated_evidence_ranges(nodes, &mut offset, &mut ranges)?
        }
        crate::content_block::BlockContent::Table { cells, .. } => {
            for cell in cells {
                collect_generated_evidence_ranges(&cell.content, &mut offset, &mut ranges)?;
            }
        }
        _ => {}
    }
    Ok((offset, ranges))
}

pub fn content_candidate_output(
    raw: &str,
    input: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let mut output: serde_json::Value = serde_json::from_str(raw)
        .map_err(|error| format!("candidate is not closed JSON: {error}"))?;
    crate::content_runtime::validate_output_schema(&output)?;
    let root = output
        .as_object()
        .ok_or_else(|| "candidate root must be an object".to_string())?;
    let mut keys = root.keys().map(String::as_str).collect::<Vec<_>>();
    keys.sort_unstable();
    if keys != ["factual_claims", "notices", "operations", "schema_version"]
        || output
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            != Some(1)
    {
        return Err("candidate root contract is not ContentGenerationOutputV1".into());
    }
    let allowed_nodes = input
        .get("target_nodes")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "frozen target nodes missing".to_string())?;
    let mut node_limits = std::collections::HashMap::new();
    let mut node_revisions = std::collections::HashMap::new();
    for node in allowed_nodes {
        let lineage = node
            .get("node_lineage_id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "frozen node lineage missing".to_string())?;
        let block_count = node
            .get("block_count")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| "frozen node block count missing".to_string())?;
        node_limits.insert(lineage, block_count);
        let revision = node
            .get("node_revision_id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "frozen node revision missing".to_string())?;
        node_revisions.insert(revision, (lineage, node));
    }
    let anchor = input
        .get("insertion_anchor")
        .filter(|value| !value.is_null())
        .map(|anchor| {
            let node_revision = anchor
                .get("node_revision_id")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| "insertion anchor node missing".to_string())?;
            let (lineage, node) = node_revisions
                .get(node_revision)
                .ok_or_else(|| "insertion anchor node is outside target".to_string())?;
            let ordinal = if let Some(block_revision) = anchor
                .get("block_revision_id")
                .and_then(serde_json::Value::as_str)
            {
                node.get("blocks")
                    .and_then(serde_json::Value::as_array)
                    .into_iter()
                    .flatten()
                    .find(|block| {
                        block
                            .get("block_revision_id")
                            .and_then(serde_json::Value::as_str)
                            == Some(block_revision)
                    })
                    .and_then(|block| block.get("ordinal"))
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| "insertion anchor block is outside target node".to_string())?
                    + 1
            } else {
                0
            };
            Ok::<_, String>((*lineage, ordinal))
        })
        .transpose()?;
    let fill_policy = input
        .get("fill_policy")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "frozen fill policy missing".to_string())?;
    let dependency = input
        .get("generation_dependency_sha256")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "frozen generation dependency missing".to_string())?
        .to_owned();
    let allowed_image_assets = input
        .get("evidence_matches")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|entry| {
            entry
                .get("items")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter(|item| item.get("kind").and_then(serde_json::Value::as_str) == Some("image"))
        .filter_map(|item| {
            item.get("evidence_item_id")
                .and_then(serde_json::Value::as_str)
        })
        .collect::<std::collections::HashSet<_>>();
    let operations = output
        .get_mut("operations")
        .and_then(serde_json::Value::as_array_mut)
        .ok_or_else(|| "candidate operations missing".to_string())?;
    if operations.len() > 10_000 {
        return Err("candidate operation bound exceeded".into());
    }
    if fill_policy == "missing_requirements_only"
        && input
            .get("requirements")
            .and_then(serde_json::Value::as_array)
            .is_some_and(Vec::is_empty)
        && !operations.is_empty()
    {
        return Err("missing_requirements_only has no uncovered Need to generate".into());
    }
    let mut refs = std::collections::HashSet::new();
    let mut operation_text_lengths = std::collections::HashMap::new();
    let mut operation_marked_ranges = std::collections::HashMap::new();
    for operation in operations {
        let object = operation
            .as_object_mut()
            .ok_or_else(|| "candidate operation must be an object".to_string())?;
        let mut operation_keys = object.keys().map(String::as_str).collect::<Vec<_>>();
        operation_keys.sort_unstable();
        if operation_keys
            != [
                "block",
                "client_operation_ref",
                "kind",
                "ordinal",
                "target_node_lineage_id",
            ]
            || object.get("kind").and_then(serde_json::Value::as_str) != Some("insert_block")
        {
            return Err("only closed insert_block candidate operations are accepted".into());
        }
        let client_ref = object
            .get("client_operation_ref")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "client_operation_ref missing".to_string())?
            .to_owned();
        if client_ref.is_empty()
            || client_ref.len() > 128
            || !client_ref
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
            || !refs.insert(client_ref.clone())
        {
            return Err("client_operation_ref is invalid or duplicated".into());
        }
        let lineage = object
            .get("target_node_lineage_id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "candidate target lineage missing".to_string())?;
        let limit = node_limits
            .get(lineage)
            .ok_or_else(|| "candidate targets a node outside the frozen input".to_string())?;
        let ordinal = object
            .get("ordinal")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| "candidate block ordinal missing".to_string())?;
        if ordinal > *limit {
            return Err("candidate block ordinal exceeds the frozen node".into());
        }
        if fill_policy == "empty_only" && *limit != 0 {
            return Err("empty_only candidate targets a non-empty node".into());
        }
        if let Some((anchor_lineage, anchor_ordinal)) = anchor
            && (lineage != anchor_lineage || ordinal != anchor_ordinal)
        {
            return Err("candidate does not honor the frozen insertion anchor".into());
        }
        let mut block: crate::content_block::ContentBlockV1 = serde_json::from_value(
            object
                .get("block")
                .ok_or_else(|| "candidate block missing".to_string())?
                .clone(),
        )
        .map_err(|error| format!("candidate block schema invalid: {error}"))?;
        block.block_revision_id = stable_candidate_uuid(&[&dependency, &client_ref, "revision"]);
        block.lineage_id = stable_candidate_uuid(&[&dependency, &client_ref, "lineage"]);
        block.revision = 1;
        block.origin = crate::content_block::BlockOrigin::AgentCandidate;
        block.content_sha256 = block.content.sha256().map_err(|error| error.to_string())?;
        block.validate().map_err(str::to_owned)?;
        if !matches!(
            &block.content,
            crate::content_block::BlockContent::RichText { .. }
                | crate::content_block::BlockContent::Table { .. }
                | crate::content_block::BlockContent::Image { .. }
        ) {
            return Err("generated Content accepts only rich_text, table, or image blocks".into());
        }
        if let crate::content_block::BlockContent::Image {
            asset_revision_id, ..
        } = &block.content
            && !allowed_image_assets.contains(asset_revision_id.to_string().as_str())
        {
            return Err("candidate image asset is outside frozen image evidence".into());
        }
        let (visible_length, marked_ranges) = block_generated_evidence_ranges(&block.content)?;
        let block_value = serde_json::to_value(&block).map_err(|error| error.to_string())?;
        operation_text_lengths.insert(client_ref.clone(), visible_length);
        operation_marked_ranges.insert(client_ref, marked_ranges);
        object.insert("block".into(), block_value);
    }
    let allowed_evidence = input
        .get("evidence_matches")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|entry| {
            let bundle = entry
                .get("evidence_bundle_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned();
            entry
                .get("items")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(move |item| {
                    let id = item.get("evidence_item_id")?.as_str()?.to_owned();
                    let start = item.get("quote_start_offset")?.as_u64()?;
                    let end = item.get("quote_end_offset")?.as_u64()?;
                    let quote = item.get("quote_utf8")?.as_str()?;
                    let start_usize = usize::try_from(start).ok()?;
                    let end_usize = usize::try_from(end).ok()?;
                    if start >= end
                        || end_usize > quote.len()
                        || !quote.is_char_boundary(start_usize)
                        || !quote.is_char_boundary(end_usize)
                    {
                        return None;
                    }
                    Some(((bundle.clone(), id), (start, end)))
                })
        })
        .collect::<std::collections::HashMap<_, _>>();
    let claims = output
        .get("factual_claims")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "factual_claims must be an array".to_string())?;
    if claims.len() > 100_000 {
        return Err("factual claim bound exceeded".into());
    }
    let mut declared_ranges = std::collections::HashSet::new();
    for claim in claims {
        let claim = claim
            .as_object()
            .ok_or_else(|| "factual claim must be an object".to_string())?;
        let mut keys = claim.keys().map(String::as_str).collect::<Vec<_>>();
        keys.sort_unstable();
        if keys
            != [
                "client_operation_ref",
                "evidence_bundle_id",
                "evidence_item_id",
                "utf8_end",
                "utf8_start",
            ]
        {
            return Err("factual claim contract is not closed".into());
        }
        let client_ref = claim
            .get("client_operation_ref")
            .and_then(serde_json::Value::as_str)
            .ok_or("claim operation ref missing")?;
        let start = claim
            .get("utf8_start")
            .and_then(serde_json::Value::as_u64)
            .ok_or("claim start missing")? as usize;
        let end = claim
            .get("utf8_end")
            .and_then(serde_json::Value::as_u64)
            .ok_or("claim end missing")? as usize;
        if start >= end
            || end
                > *operation_text_lengths
                    .get(client_ref)
                    .ok_or("claim operation ref is not generated")?
        {
            return Err("factual claim range is outside generated text".into());
        }
        let bundle = claim
            .get("evidence_bundle_id")
            .and_then(serde_json::Value::as_str)
            .ok_or("claim bundle missing")?;
        let item = claim
            .get("evidence_item_id")
            .and_then(serde_json::Value::as_str)
            .ok_or("claim item missing")?;
        if !allowed_evidence.contains_key(&(bundle.to_owned(), item.to_owned())) {
            return Err("factual claim evidence is outside frozen selection".into());
        }
        if !declared_ranges.insert((
            client_ref.to_owned(),
            start,
            end,
            bundle.to_owned(),
            item.to_owned(),
        )) {
            return Err("factual claim is duplicated".into());
        }
    }
    let marked_ranges = operation_marked_ranges
        .into_iter()
        .flat_map(|(client_ref, ranges)| {
            ranges.into_iter().map(move |range| {
                (
                    client_ref.clone(),
                    range.start,
                    range.end,
                    range.bundle,
                    range.item,
                )
            })
        })
        .collect::<std::collections::HashSet<_>>();
    if declared_ranges != marked_ranges {
        return Err("factual claims and evidence_ref spans must correspond exactly".into());
    }
    fn validate_evidence_refs(
        value: &serde_json::Value,
        allowed: &std::collections::HashMap<(String, String), (u64, u64)>,
    ) -> Result<(), String> {
        match value {
            serde_json::Value::Object(map) => {
                if map.get("kind").and_then(serde_json::Value::as_str) == Some("evidence_ref") {
                    let bundle = map
                        .get("evidence_bundle_id")
                        .and_then(serde_json::Value::as_str)
                        .ok_or("evidence_ref bundle missing")?;
                    let item = map
                        .get("evidence_item_id")
                        .and_then(serde_json::Value::as_str)
                        .ok_or("evidence_ref item missing")?;
                    let start = map
                        .get("quote_start_offset")
                        .and_then(serde_json::Value::as_u64)
                        .ok_or("evidence_ref quote start missing")?;
                    let end = map
                        .get("quote_end_offset")
                        .and_then(serde_json::Value::as_u64)
                        .ok_or("evidence_ref quote end missing")?;
                    if allowed.get(&(bundle.to_owned(), item.to_owned())) != Some(&(start, end)) {
                        return Err("content EvidenceRef identity or quote offsets differ from frozen selection".into());
                    }
                }
                for nested in map.values() {
                    validate_evidence_refs(nested, allowed)?;
                }
            }
            serde_json::Value::Array(values) => {
                for nested in values {
                    validate_evidence_refs(nested, allowed)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    validate_evidence_refs(output.get("operations").unwrap(), &allowed_evidence)?;
    let notices = output
        .get("notices")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "notices must be an array".to_string())?;
    if notices.len() > 10_000 {
        return Err("candidate notice bound exceeded".into());
    }
    let allowed_requirements = input
        .get("requirements")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|requirement| {
            requirement
                .get("requirement_revision_id")
                .and_then(serde_json::Value::as_str)
        })
        .collect::<std::collections::HashSet<_>>();
    let notice_codes = [
        "NO_EVIDENCE",
        "WEAK_EVIDENCE",
        "UNSUPPORTED_FACT",
        "FORM_CONSTRAINT",
        "TARGET_ALREADY_HAS_CONTENT",
    ];
    for notice in notices {
        let Some(object) = notice.as_object() else {
            return Err("candidate notice must be an object".into());
        };
        let mut keys = object.keys().map(String::as_str).collect::<Vec<_>>();
        keys.sort_unstable();
        let requirement = notice
            .get("requirement_revision_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let message = notice
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if keys != ["code", "message", "requirement_revision_id", "severity"]
            || !notice_codes.contains(
                &notice
                    .get("code")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default(),
            )
            || !["info", "warning"].contains(
                &notice
                    .get("severity")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default(),
            )
            || message.is_empty()
            || message.len() > 4_096
            || Uuid::parse_str(requirement).is_err()
            || !allowed_requirements.contains(requirement)
        {
            return Err("candidate notice is outside the frozen closed contract".into());
        }
    }
    Ok(output)
}

fn content_retrieval_error(
    error: knowledge::KnowledgeRetrievalError,
) -> crate::agent_error::AgentError {
    let (code, message) = match error {
        knowledge::KnowledgeRetrievalError::InvalidRequest(message) => {
            ("CONTENT_RETRIEVAL_INVALID_REQUEST", message)
        }
        knowledge::KnowledgeRetrievalError::Unavailable(message) => {
            ("CONTENT_RETRIEVAL_UNAVAILABLE", message)
        }
        knowledge::KnowledgeRetrievalError::PolicyRevoked(message) => {
            ("CONTENT_RETRIEVAL_POLICY_REVOKED", message)
        }
        knowledge::KnowledgeRetrievalError::DigestMismatch(message) => {
            ("CONTENT_RETRIEVAL_DIGEST_MISMATCH", message)
        }
        knowledge::KnowledgeRetrievalError::QuotaExceeded(message) => {
            ("CONTENT_RETRIEVAL_QUOTA_EXCEEDED", message)
        }
        knowledge::KnowledgeRetrievalError::InvalidHit(message) => {
            ("CONTENT_RETRIEVAL_INVALID_HIT", message)
        }
    };
    crate::agent_error::AgentError::new(code, message)
}

async fn retrieve_content_scope_with_retry_v2(
    adapter: &knowledge::PostgresKnowledgeRetrievalAdapter,
    frozen: &knowledge::knowledge_retrieval::FrozenRetrievalPolicyIdentityV1,
    scope: knowledge::KnowledgeEvidenceScopeV2,
) -> Result<knowledge::KnowledgeEvidenceBatchV3, crate::agent_error::AgentError> {
    let mut unavailable = None;
    for _ in 0..3 {
        match adapter
            .retrieve_frozen_evidence_v3(frozen, scope.clone())
            .await
        {
            Ok(batch) => return Ok(batch),
            Err(knowledge::KnowledgeRetrievalError::Unavailable(message)) => {
                unavailable = Some(message)
            }
            Err(error) => return Err(content_retrieval_error(error)),
        }
    }
    Err(crate::agent_error::AgentError::new(
        "CONTENT_RETRIEVAL_UNAVAILABLE",
        unavailable.unwrap_or_else(|| "retrieval unavailable".into()),
    ))
}

async fn prepare_content_evidence_v2(
    pool: &PgPool,
    input: &serde_json::Value,
) -> Result<
    (
        knowledge::knowledge_retrieval::RetrievalPolicyIdentityV1,
        serde_json::Value,
        serde_json::Value,
    ),
    crate::agent_error::AgentError,
> {
    let frozen_value = input.get("retrieval_identity").cloned().ok_or_else(|| {
        crate::agent_error::AgentError::new(
            "CONTENT_RETRIEVAL_INVALID_REQUEST",
            "frozen retrieval identity missing",
        )
    })?;
    let frozen_utf8 = input
        .get("retrieval_identity_utf8")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            crate::agent_error::AgentError::new(
                "CONTENT_RETRIEVAL_INVALID_REQUEST",
                "frozen retrieval identity bytes missing",
            )
        })?;
    let frozen: knowledge::knowledge_retrieval::FrozenRetrievalPolicyIdentityV1 =
        serde_json::from_str(frozen_utf8).map_err(|error| {
            crate::agent_error::AgentError::new(
                "CONTENT_RETRIEVAL_INVALID_REQUEST",
                error.to_string(),
            )
        })?;
    if serde_json::to_value(&frozen).ok().as_ref() != Some(&frozen_value) {
        return Err(crate::agent_error::AgentError::new(
            "CONTENT_RETRIEVAL_DIGEST_MISMATCH",
            "retrieval identity value differs from frozen bytes",
        ));
    }
    let (frozen_bytes, canonical_sha) = frozen.canonical_bytes_and_sha256().map_err(|error| {
        crate::agent_error::AgentError::new("CONTENT_RETRIEVAL_INVALID_REQUEST", error)
    })?;
    if frozen_bytes.as_slice() != frozen_utf8.as_bytes() {
        return Err(crate::agent_error::AgentError::new(
            "CONTENT_RETRIEVAL_DIGEST_MISMATCH",
            "retrieval identity bytes are not canonical",
        ));
    }
    let expected_sha = input
        .get("retrieval_identity_sha256")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            crate::agent_error::AgentError::new(
                "CONTENT_RETRIEVAL_INVALID_REQUEST",
                "retrieval identity digest missing",
            )
        })?;
    if canonical_sha != expected_sha {
        return Err(crate::agent_error::AgentError::new(
            "CONTENT_RETRIEVAL_DIGEST_MISMATCH",
            "retrieval identity bytes changed",
        ));
    }
    let policy = frozen.validate().map_err(|error| {
        let code = if error.starts_with("CONTENT_RETRIEVAL_DIGEST_MISMATCH:") {
            "CONTENT_RETRIEVAL_DIGEST_MISMATCH"
        } else {
            "CONTENT_RETRIEVAL_INVALID_REQUEST"
        };
        crate::agent_error::AgentError::new(code, error)
    })?;
    let adapter = knowledge::PostgresKnowledgeRetrievalAdapter::new_complete_v2_from_environment(
        pool.clone(),
    )
    .map_err(|error| {
        crate::agent_error::AgentError::new("CONTENT_RETRIEVAL_UNAVAILABLE", error.to_string())
    })?;
    let requirements = input
        .get("requirements")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            crate::agent_error::AgentError::new(
                "CONTENT_RETRIEVAL_INVALID_REQUEST",
                "frozen generation requirements missing",
            )
        })?;
    let request_id = input
        .get("request_artifact_id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            crate::agent_error::AgentError::new(
                "CONTENT_RETRIEVAL_INVALID_REQUEST",
                "frozen request identity missing",
            )
        })?
        .to_owned();
    let mut batches = Vec::with_capacity(requirements.len());
    for requirement in requirements {
        let requirement_id = requirement
            .get("requirement_revision_id")
            .and_then(serde_json::Value::as_str)
            .and_then(|value| Uuid::parse_str(value).ok())
            .ok_or_else(|| {
                crate::agent_error::AgentError::new(
                    "CONTENT_RETRIEVAL_INVALID_REQUEST",
                    "frozen requirement identity missing",
                )
            })?;
        let requirement_text = requirement
            .get("requirement_text")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                crate::agent_error::AgentError::new(
                    "CONTENT_RETRIEVAL_INVALID_REQUEST",
                    "frozen requirement text missing",
                )
            })?
            .to_owned();
        let requirement_identity_sha256 = requirement
            .get("requirement_identity_sha256")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                crate::agent_error::AgentError::new(
                    "CONTENT_RETRIEVAL_INVALID_REQUEST",
                    "frozen requirement digest missing",
                )
            })?
            .to_owned();
        let product_request = knowledge::ProductEvidenceRequestV1 {
            schema_version: 1,
            requirement_identity_sha256: requirement_identity_sha256.clone(),
            requirement_text: requirement_text.clone(),
            product_version_ids: frozen.product_version_ids.clone(),
            retrieval_policy: policy.clone(),
        };
        let company_request = knowledge::CompanyEvidenceRequestV1 {
            schema_version: 1,
            requirement_identity_sha256: requirement_identity_sha256.clone(),
            requirement_text: requirement_text.clone(),
            library_version_ids: frozen.library_version_ids.clone(),
            retrieval_policy: policy.clone(),
        };
        let product_line = retrieve_content_scope_with_retry_v2(
            &adapter,
            &frozen,
            knowledge::KnowledgeEvidenceScopeV2::ProductLine(product_request),
        )
        .await?;
        let company = retrieve_content_scope_with_retry_v2(
            &adapter,
            &frozen,
            knowledge::KnowledgeEvidenceScopeV2::Company(company_request),
        )
        .await?;
        batches.push(
            knowledge::knowledge_retrieval_pg::RequirementEvidenceBatchesV2 {
                route_id: requirement_id,
                requirement_artifact_id: requirement_id,
                requirement_identity_sha256,
                requirement_text,
                product_line,
                company,
            },
        );
    }
    let canonical_scope =
        knowledge::knowledge_retrieval_pg::compile_requirement_evidence_scope_v2(&policy, &batches)
            .map_err(content_retrieval_error)?;
    let products = canonical_scope
        .get("products")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            crate::agent_error::AgentError::new(
                "CONTENT_RETRIEVAL_INVALID_HIT",
                "attested evidence products missing",
            )
        })?;
    let hits = canonical_scope
        .get("frozen_hits")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            crate::agent_error::AgentError::new(
                "CONTENT_RETRIEVAL_INVALID_HIT",
                "attested evidence hits missing",
            )
        })?;
    let matches=serde_json::Value::Array(batches.iter().map(|batch| {
        let requirement_id=batch.requirement_artifact_id.to_string();
        let bundle_id=stable_candidate_uuid(&[&request_id,&requirement_id,"bundle"]);
        let items=hits.iter().filter(|hit|hit.get("requirement_artifact_id").and_then(serde_json::Value::as_str)
            ==Some(requirement_id.as_str())).filter_map(|hit|{
            let product_id=hit.get("product_version_artifact_id")?.as_str()?;
            let product=products.iter().find(|product|product.get("id").and_then(serde_json::Value::as_str)==Some(product_id))?;
            let hit_id=hit.get("id")?.as_str()?;
            let evidence_item_id=stable_candidate_uuid(&[&request_id,&requirement_id,hit_id,"item"]);
            if hit.get("source_type").and_then(serde_json::Value::as_str)==Some("image_ocr") {
                let media=hit.get("media")?;
                Some(serde_json::json!({"kind":"image","evidence_item_id":evidence_item_id,
                    "document_id":hit.get("document_id")?,"source_chunk_id":hit.get("source_chunk_id")?,
                    "product_version_id":product.get("product_version_id")?,"workspace_kind":product.get("workspace_kind")?,
                    "quote_utf8":hit.get("chunk_utf8")?,"quote_sha256":hit.get("chunk_sha256")?,
                    "quote_start_offset":hit.get("quote_start_offset")?,"quote_end_offset":hit.get("quote_end_offset")?,
                    "retrieval_rank":hit.get("retrieval_rank")?,"retrieval_contract_version":hit.get("retrieval_contract_version")?,
                    "image_artifact_revision_id":media.get("image_artifact_revision_id")?,
                    "object_ref":media.get("object_ref")?,"sha256":media.get("sha256")?,
                    "media_type":media.get("media_type")?,"width":media.get("width")?,"height":media.get("height")?,
                    "frozen_document_display_name":media.get("frozen_document_display_name")?,
                    "page_ordinal":media.get("page_ordinal").cloned().unwrap_or(serde_json::Value::Null),
                    "bounding_region":media.get("bounding_region").cloned().unwrap_or(serde_json::Value::Null)}))
            }else{
                Some(serde_json::json!({"kind":"text_quote","evidence_item_id":evidence_item_id,
                    "document_id":hit.get("document_id")?,"source_chunk_id":hit.get("source_chunk_id")?,
                    "product_version_id":product.get("product_version_id")?,"workspace_kind":product.get("workspace_kind")?,
                    "frozen_document_display_name":hit.get("frozen_document_display_name")?,
                    "quote_utf8":hit.get("chunk_utf8")?,"quote_sha256":hit.get("chunk_sha256")?,
                    "quote_start_offset":hit.get("quote_start_offset")?,"quote_end_offset":hit.get("quote_end_offset")?,
                    "retrieval_rank":hit.get("retrieval_rank")?,"retrieval_contract_version":hit.get("retrieval_contract_version")?}))
            }
        }).collect::<Vec<_>>();
        serde_json::json!({"requirement_revision_id":batch.requirement_artifact_id,
            "evidence_bundle_id":bundle_id,"items":items})
    }).collect());
    Ok((policy, canonical_scope, matches))
}

struct PickErr(String);

async fn load_user_pick_evidence_v2(
    pool: &PgPool,
    input: &serde_json::Value,
) -> Result<
    (
        knowledge::knowledge_retrieval_pg::AttestedEvidenceScopeV2,
        serde_json::Value,
    ),
    PickErr,
> {
    let request_id = input
        .get("request_artifact_id")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| PickErr("frozen request identity missing".into()))?;
    let frozen_sha = input
        .get("generation_dependency_sha256")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| PickErr("frozen input digest missing".into()))?;
    let frozen: serde_json::Value =
        sqlx::query_scalar("SELECT kb_bid_v2_load_user_pick_evidence($1,1,$2::kb_sha256)")
            .bind(request_id)
            .bind(frozen_sha)
            .fetch_one(pool)
            .await
            .map_err(|error| PickErr(format!("EVIDENCE_UNAVAILABLE: {error}")))?;
    let attestation = knowledge::knowledge_retrieval_pg::AttestedEvidenceScopeV2 {
        attestation_id: frozen
            .get("attestation_id")
            .and_then(serde_json::Value::as_str)
            .and_then(|value| Uuid::parse_str(value).ok())
            .ok_or_else(|| PickErr("PickSet attestation identity missing".into()))?,
        attestation_sha256: frozen
            .get("attestation_sha256")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| PickErr("PickSet attestation digest missing".into()))?
            .to_owned(),
        canonical_scope: frozen
            .get("canonical_scope")
            .cloned()
            .ok_or_else(|| PickErr("PickSet attestation snapshot missing".into()))?,
    };
    let original_bundle = frozen
        .get("evidence_bundle_id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| PickErr("PickSet evidence bundle identity missing".into()))?;
    let request = request_id.to_string();
    let copied_bundle = stable_candidate_uuid(&[&request, original_bundle, "user-pick-bundle"]);
    let mut copied_items = frozen
        .get("items")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .ok_or_else(|| PickErr("PickSet selected evidence items missing".into()))?;
    for item in &mut copied_items {
        let old = item
            .get("evidence_item_id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| PickErr("PickSet evidence item identity missing".into()))?;
        let copied = stable_candidate_uuid(&[&request, original_bundle, old, "user-pick-item"]);
        item.as_object_mut()
            .ok_or_else(|| PickErr("PickSet evidence item invalid".into()))?
            .insert("evidence_item_id".into(), serde_json::json!(copied));
    }
    let matches = serde_json::json!([{"requirement_revision_id":frozen.get("requirement_revision_id"),
        "evidence_bundle_id":copied_bundle,"items":copied_items}]);
    Ok((attestation, matches))
}

fn content_runtime_error(
    error: crate::content_runtime::ContentTurnError,
) -> crate::agent_error::AgentError {
    crate::agent_error::AgentError::new(error.code(), error.message())
}

pub fn retain_content_attempt_failure(
    call_ordinal: i32,
    failure: crate::agent_error::AgentError,
) -> Result<String, crate::agent_error::AgentError> {
    if call_ordinal == 3 {
        Err(failure)
    } else {
        Ok(failure.to_string())
    }
}

fn content_contract_error(message: String) -> crate::agent_error::AgentError {
    let code = message.split(':').next().unwrap_or("INPUT_SCHEMA_INVALID");
    crate::agent_error::AgentError::new(code, message.clone())
}

pub fn content_database_error(error: sqlx::Error) -> crate::agent_error::AgentError {
    const CLOSED: &[&str] = &[
        "REQUEST_OBSOLETE",
        "REQUEST_ATTEMPT_SUPERSEDED",
        "FROZEN_INPUT_MISSING",
        "FROZEN_INPUT_DIGEST_MISMATCH",
        "INPUT_SCHEMA_INVALID",
        "WORKSPACE_CAS_CONFLICT",
        "AGENT_OUTPUT_INVALID",
        "AGENT_TURN_BUDGET_EXCEEDED",
        "CONTENT_RETRIEVAL_INVALID_REQUEST",
        "CONTENT_RETRIEVAL_UNAVAILABLE",
        "CONTENT_RETRIEVAL_QUOTA_EXCEEDED",
        "CONTENT_RETRIEVAL_INVALID_HIT",
        "CONTENT_RETRIEVAL_POLICY_REVOKED",
        "CONTENT_RETRIEVAL_DIGEST_MISMATCH",
        "CONTENT_MATCH_TIMEOUT",
    ];
    if let Some(database) = error.as_database_error() {
        let message = database.message();
        if let Some(code) = CLOSED.iter().find(|code| {
            message == **code
                || message
                    .strip_prefix(**code)
                    .is_some_and(|suffix| suffix.starts_with(':') || suffix.starts_with(' '))
        }) {
            return crate::agent_error::AgentError::new(code, message);
        }
    }
    crate::agent_error::AgentError::new("INTERNAL", error.to_string())
}

async fn run_content_agent_v1(
    pool: &PgPool,
    request: &platform::BidAuthoringRequestIdentityV2,
    owner: &crate::bid_authoring_v2::ContentRunLease,
    input: &serde_json::Value,
    staged_input_sha256: &str,
) -> Result<serde_json::Value, crate::agent_error::AgentError> {
    let contract = input.get("agent_contract").ok_or_else(|| {
        crate::agent_error::AgentError::new(
            "INPUT_SCHEMA_INVALID",
            "generate Agent contract missing",
        )
    })?;
    let runtime: crate::content_runtime::ContentAgentRuntimeContractV1 =
        serde_json::from_value(contract.get("runtime_contract").cloned().ok_or_else(|| {
            crate::agent_error::AgentError::new(
                "INPUT_SCHEMA_INVALID",
                "Content runtime contract missing",
            )
        })?)
        .map_err(|error| {
            crate::agent_error::AgentError::new("INPUT_SCHEMA_INVALID", error.to_string())
        })?;
    runtime.validate().map_err(content_contract_error)?;
    let prompt = contract
        .get("prompt_utf8")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            crate::agent_error::AgentError::new(
                "INPUT_SCHEMA_INVALID",
                "Content prompt bytes missing",
            )
        })?;
    let schema = contract
        .get("output_schema_utf8")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            crate::agent_error::AgentError::new(
                "INPUT_SCHEMA_INVALID",
                "Content output schema bytes missing",
            )
        })?;
    if prompt.as_bytes() != crate::content_runtime::CONTENT_AGENT_SYSTEM_PROMPT.as_bytes()
        || schema.as_bytes() != crate::content_runtime::CONTENT_OUTPUT_SCHEMA_UTF8.as_bytes()
        || platform::sha256_hex(prompt.as_bytes())
            != contract
                .get("prompt_sha256")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
        || platform::sha256_hex(schema.as_bytes())
            != contract
                .get("output_schema_sha256")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
    {
        return Err(crate::agent_error::AgentError::new(
            "FROZEN_INPUT_DIGEST_MISMATCH",
            "Content prompt/schema bytes changed",
        ));
    }
    let runtime_value = serde_json::to_value(&runtime).map_err(|error| {
        crate::agent_error::AgentError::new("INPUT_SCHEMA_INVALID", error.to_string())
    })?;
    let runtime_bytes = crate::content_runtime::canonical_json_bytes(&runtime_value)
        .map_err(|error| crate::agent_error::AgentError::new("INPUT_SCHEMA_INVALID", error))?;
    let runtime_sha = platform::sha256_hex(&runtime_bytes);
    if Some(runtime_sha.as_str())
        != contract
            .get("runtime_contract_sha256")
            .and_then(serde_json::Value::as_str)
    {
        return Err(crate::agent_error::AgentError::new(
            "FROZEN_INPUT_DIGEST_MISMATCH",
            "Content runtime bytes changed",
        ));
    }
    if staged_input_sha256.len() != 64
        || !staged_input_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(crate::agent_error::AgentError::new(
            "CONTENT_DIVERGENT_AGENT_INPUT_REPLAY",
            "staged Content Agent input digest invalid",
        ));
    }
    let prompt_contract_id = contract
        .get("prompt_contract_id")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| {
            crate::agent_error::AgentError::new(
                "INPUT_SCHEMA_INVALID",
                "prompt contract id missing",
            )
        })?;
    let agent_contract_id = contract
        .get("agent_contract_id")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| {
            crate::agent_error::AgentError::new("INPUT_SCHEMA_INVALID", "agent contract id missing")
        })?;
    let model_contract_id = contract
        .get("model_contract_id")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| {
            crate::agent_error::AgentError::new("INPUT_SCHEMA_INVALID", "model contract id missing")
        })?;
    let required = |name: &str| {
        contract
            .get(name)
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                crate::agent_error::AgentError::new(
                    "INPUT_SCHEMA_INVALID",
                    format!("{name} missing"),
                )
            })
    };
    let transport = crate::content_runtime::ReqwestContentHttpTransport;
    let mut prior_validation_error: Option<String> = None;
    for _ in 0..3 {
        let call_ordinal = crate::bid_authoring_v2::claim_content_boundary_attempt_v1(
            pool,
            request,
            owner,
            staged_input_sha256,
            prompt_contract_id,
            required("prompt_contract_sha256")?,
            required("prompt_sha256")?,
            required("output_schema_id")?,
            required("output_schema_sha256")?,
            agent_contract_id,
            required("agent_contract_sha256")?,
            model_contract_id,
            required("model_contract_sha256")?,
            &runtime_sha,
        )
        .await
        .map_err(content_database_error)?;
        let user_payload = serde_json::json!({
            "frozen_input": input,
            "prior_validation_error": prior_validation_error,
        });
        let failure =
            match crate::content_runtime::turn_once_with(&transport, &runtime, &user_payload).await
            {
                Ok(value) => match content_candidate_output(&value.to_string(), input) {
                    Ok(output) => return Ok(output),
                    Err(message) => {
                        crate::agent_error::AgentError::new("AGENT_OUTPUT_INVALID", message)
                    }
                },
                Err(error) => content_runtime_error(error),
            };
        prior_validation_error = Some(retain_content_attempt_failure(call_ordinal, failure)?);
    }
    let message = prior_validation_error.unwrap_or_else(|| "Content Agent output invalid".into());
    if message.starts_with("AGENT_TURN_TIMEOUT:") {
        Err(crate::agent_error::AgentError::new(
            "AGENT_TURN_TIMEOUT",
            message,
        ))
    } else if message.starts_with("AGENT_PROVIDER_UNAVAILABLE:") {
        Err(crate::agent_error::AgentError::new(
            "AGENT_PROVIDER_UNAVAILABLE",
            message,
        ))
    } else {
        Err(crate::agent_error::AgentError::new(
            "AGENT_OUTPUT_INVALID",
            message,
        ))
    }
}

pub async fn execute(
    pool: &PgPool,
    job: &ContentGenerateJobV2,
    owner: Option<&crate::bid_authoring_v2::ContentRunLease>,
) -> Result<(), crate::agent_error::AgentError> {
    let input = crate::bid_authoring_v2::load_content_generation_input_v2(
        pool,
        job.request.request_artifact_id,
        job.request.request_revision,
        &job.request.frozen_input_sha256,
    )
    .await
    .map_err(content_database_error)?;
    let input_operation = input
        .get("operation")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            crate::agent_error::AgentError::new("INPUT_SCHEMA_INVALID", "Content operation missing")
        })?;
    let queued_operation = match job.operation {
        ContentGenerateOperationV2::Generate => "generate",
        ContentGenerateOperationV2::MatchOnly => "match_only",
    };
    if input_operation != queued_operation {
        return Err(crate::agent_error::AgentError::new(
            "INPUT_SCHEMA_INVALID",
            "queued Content operation differs from frozen operation",
        ));
    }
    let staged = if let Some(owner) = owner {
        let staged = crate::bid_authoring_v2::load_content_agent_input_v1(pool, &job.request)
            .await
            .map_err(content_database_error)?;
        if staged.is_none() {
            crate::bid_authoring_v2::progress_content_agent_run_v1(
                pool,
                &job.request,
                owner,
                "retrieving",
                serde_json::json!({"phase":"retrieving"}),
            )
            .await
            .map_err(content_database_error)?;
        }
        staged
    } else {
        None
    };
    let (existing_attestation, pending_scope, matches, agent_input, staged_input_sha256) =
        if let Some(staged) = staged {
            let payload = staged.get("payload").ok_or_else(|| {
                crate::agent_error::AgentError::new(
                    "CONTENT_DIVERGENT_AGENT_INPUT_REPLAY",
                    "stored Content Agent input payload missing",
                )
            })?;
            let matches = payload.get("matches").cloned().ok_or_else(|| {
                crate::agent_error::AgentError::new(
                    "CONTENT_DIVERGENT_AGENT_INPUT_REPLAY",
                    "stored matches missing",
                )
            })?;
            let agent_input = payload.get("agent_input").cloned().ok_or_else(|| {
                crate::agent_error::AgentError::new(
                    "CONTENT_DIVERGENT_AGENT_INPUT_REPLAY",
                    "stored Agent input missing",
                )
            })?;
            let pending_scope = payload
                .get("pending_scope")
                .filter(|value| !value.is_null())
                .cloned();
            let existing_attestation = payload
                .get("existing_attestation")
                .filter(|value| !value.is_null())
                .map(|value| {
                    Ok::<_, crate::agent_error::AgentError>(
                        knowledge::knowledge_retrieval_pg::AttestedEvidenceScopeV2 {
                            attestation_id: value
                                .get("attestation_id")
                                .and_then(serde_json::Value::as_str)
                                .and_then(|value| Uuid::parse_str(value).ok())
                                .ok_or_else(|| {
                                    crate::agent_error::AgentError::new(
                                        "CONTENT_DIVERGENT_AGENT_INPUT_REPLAY",
                                        "stored attestation id invalid",
                                    )
                                })?,
                            attestation_sha256: value
                                .get("attestation_sha256")
                                .and_then(serde_json::Value::as_str)
                                .ok_or_else(|| {
                                    crate::agent_error::AgentError::new(
                                        "CONTENT_DIVERGENT_AGENT_INPUT_REPLAY",
                                        "stored attestation digest invalid",
                                    )
                                })?
                                .to_owned(),
                            canonical_scope: value.get("canonical_scope").cloned().ok_or_else(
                                || {
                                    crate::agent_error::AgentError::new(
                                        "CONTENT_DIVERGENT_AGENT_INPUT_REPLAY",
                                        "stored attestation scope missing",
                                    )
                                },
                            )?,
                        },
                    )
                })
                .transpose()?;
            let staged_input_sha256 = staged
                .get("input_sha256")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    crate::agent_error::AgentError::new(
                        "CONTENT_DIVERGENT_AGENT_INPUT_REPLAY",
                        "stored Agent input digest missing",
                    )
                })?
                .to_owned();
            (
                existing_attestation,
                pending_scope,
                matches,
                agent_input,
                staged_input_sha256,
            )
        } else {
            let user_pick = input
                .get("evidence_selection_mode")
                .and_then(serde_json::Value::as_str)
                == Some("user_pick_set");
            let (existing_attestation, pending_scope, matches) = if user_pick {
                let (attestation, matches) = load_user_pick_evidence_v2(pool, &input)
                    .await
                    .map_err(|error| {
                        crate::agent_error::AgentError::new(
                            "CONTENT_RETRIEVAL_INVALID_REQUEST",
                            error.0,
                        )
                    })?;
                (Some(attestation), None, matches)
            } else {
                let (_, scope, matches) = prepare_content_evidence_v2(pool, &input).await?;
                (None, Some(scope), matches)
            };
            let mut agent_input = input.clone();
            agent_input["evidence_matches"] = matches.clone();
            if let Some(owner) = owner {
                let stage_payload = serde_json::json!({
                    "matches":matches,
                    "pending_scope":pending_scope,
                    "existing_attestation":existing_attestation.as_ref().map(|value| serde_json::json!({
                        "attestation_id":value.attestation_id,
                        "attestation_sha256":value.attestation_sha256,
                        "canonical_scope":value.canonical_scope,
                    })),
                    "agent_input":agent_input,
                });
                let stored = crate::bid_authoring_v2::store_content_agent_input_v1(
                    pool,
                    &job.request,
                    owner,
                    &stage_payload,
                )
                .await
                .map_err(content_database_error)?;
                let staged_input_sha256 = stored
                    .get("input_sha256")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        crate::agent_error::AgentError::new(
                            "CONTENT_DIVERGENT_AGENT_INPUT_REPLAY",
                            "stored Agent input digest missing",
                        )
                    })?
                    .to_owned();
                (
                    existing_attestation,
                    pending_scope,
                    matches,
                    agent_input,
                    staged_input_sha256,
                )
            } else {
                (
                    existing_attestation,
                    pending_scope,
                    matches,
                    agent_input,
                    String::new(),
                )
            }
        };
    let (candidate_id, payload, digest, operations) = match job.operation {
        ContentGenerateOperationV2::MatchOnly => (None, None, None, serde_json::json!([])),
        ContentGenerateOperationV2::Generate => {
            let owner = owner.ok_or_else(|| {
                crate::agent_error::AgentError::new(
                    "REQUEST_ATTEMPT_SUPERSEDED",
                    "generate owner missing",
                )
            })?;
            crate::bid_authoring_v2::progress_content_agent_run_v1(
                pool,
                &job.request,
                owner,
                "generating",
                serde_json::json!({"phase":"generating"}),
            )
            .await
            .map_err(content_database_error)?;
            let output = run_content_agent_v1(
                pool,
                &job.request,
                owner,
                &agent_input,
                &staged_input_sha256,
            )
            .await?;
            let operations = output.get("operations").cloned().ok_or_else(|| {
                crate::agent_error::AgentError::new(
                    "AGENT_OUTPUT_INVALID",
                    "verified operations missing",
                )
            })?;
            let bytes = crate::content_runtime::canonical_json_bytes(&output).map_err(|error| {
                crate::agent_error::AgentError::new("AGENT_OUTPUT_INVALID", error)
            })?;
            let digest = platform::sha256_hex(&bytes);
            let request_id = job.request.request_artifact_id.to_string();
            let candidate_id =
                stable_candidate_uuid(&[&request_id, &digest, "content-candidate-v1"]);
            (Some(candidate_id), Some(bytes), Some(digest), operations)
        }
    };
    let candidate = match (candidate_id, payload.as_deref(), digest.as_deref()) {
        (Some(id), Some(bytes), Some(sha256)) => Some((id, bytes, sha256)),
        (None, None, None) => None,
        _ => {
            return Err(crate::agent_error::AgentError::new(
                "AGENT_OUTPUT_INVALID",
                "candidate publication identity incomplete",
            ));
        }
    };
    if let Some(owner) = owner {
        crate::bid_authoring_v2::progress_content_agent_run_v1(
            pool,
            &job.request,
            owner,
            "publishing",
            serde_json::json!({"phase":"publishing"}),
        )
        .await
        .map_err(content_database_error)?;
    }
    let mut tx = pool.begin().await.map_err(content_database_error)?;
    crate::bid_authoring_v2::assert_content_owner_in_transaction(&mut tx, &job.request, owner)
        .await
        .map_err(content_database_error)?;
    let attestation = match (existing_attestation, pending_scope) {
        (Some(attestation), None) => attestation,
        (None, Some(scope)) => {
            knowledge::knowledge_retrieval_pg::attest_compiled_requirement_evidence_v2(
                &mut tx, &scope,
            )
            .await
            .map_err(content_retrieval_error)?
        }
        _ => {
            return Err(crate::agent_error::AgentError::new(
                "CONTENT_RETRIEVAL_INVALID_REQUEST",
                "attestation state is not closed",
            ));
        }
    };
    crate::bid_authoring_v2::publish_content_generation_v2_in_transaction(
        &mut tx,
        &job.request,
        owner,
        (attestation.attestation_id, &attestation.attestation_sha256),
        &matches,
        candidate,
        &operations,
    )
    .await
    .map_err(content_database_error)?;
    tx.commit().await.map_err(content_database_error)?;
    Ok(())
}
