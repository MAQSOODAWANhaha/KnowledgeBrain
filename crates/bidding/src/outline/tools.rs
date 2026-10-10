//! The outline model writes chapters, attachment bindings, and template slots.
//!
//! Eight tools, and a turn is shown only the ones its duty allows.
//!
//! 禁止硬编码: chapter shape uses attachment chains and the documented depth
//! constants. It does not match chapter titles or a sample keyword list.

use super::{
    ChapterOutline, ChapterPurpose, Fulfillment, ReviewIssue, SlotKind, TargetRef, TemplateBody,
    TemplateContent,
    chapters::{
        AttachmentBinding, DEPTH_CHAIN_MIN, RESPONSE_LEAF_DEPTH, attachment_chains,
        attachment_form_ids, form_cards,
    },
};
use crate::analysis::FrozenInput;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashSet};

/// Independent Check reads never inherit the Organize transcript's receipts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckReads {
    pub pending_evidence: Vec<(usize, super::evidence::EvidenceRef)>,
    pub evidence: Vec<super::evidence::EvidenceRef>,
    pub pending_empty_pack_ids: Vec<(usize, String)>,
    pub empty_pack_ids: BTreeSet<String>,
    pub pending_slot_ranges: Vec<(usize, String, usize, usize)>,
    pub pending_structure_keys: Vec<(usize, String)>,
    pub structure_keys: BTreeSet<String>,
    pub slot_ranges: std::collections::BTreeMap<String, Vec<(usize, usize)>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    pub model_wire: super::model_wire::Registry,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub source_keys: super::source_wire::Keys,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub metadata_records: std::collections::BTreeMap<String, super::metadata_fragments::Record>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_metadata_parts: Vec<(usize, String, Vec<super::metadata_fragments::Field>)>,
    #[serde(default)]
    pub read_epoch: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read_frames: Vec<super::read_receipts::Frame>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub evidence_continuations: std::collections::BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub claim_comparisons: std::collections::BTreeMap<String, super::claim_review::Comparison>,
    #[serde(default)]
    pub check_reads: CheckReads,
    #[serde(default)]
    pub discovery_revision: Option<String>,
    /// Reads are credited only after a subsequent complete model turn, never to
    /// another tool call emitted alongside the read in the same response.
    #[serde(default)]
    pub pending_evidence: Vec<(usize, super::evidence::EvidenceRef)>,
    #[serde(default)]
    pub pending_empty_pack_ids: Vec<(usize, String)>,
    #[serde(default)]
    pub delivered_empty_pack_ids: BTreeSet<String>,
    #[serde(default)]
    pub delivered_evidence: Vec<super::evidence::EvidenceRef>,
    #[serde(default)]
    pub required_requirement_ids: BTreeSet<String>,
    #[serde(default)]
    pub requirements: std::collections::BTreeMap<String, super::discover::RequirementRecord>,
    #[serde(default)]
    pub required_pack_ids: BTreeSet<String>,
    #[serde(default)]
    pub fulfillments: Vec<Fulfillment>,
    #[serde(default)]
    pub reviewed_requirement_ids: BTreeSet<String>,
    #[serde(default)]
    pub reviewed_pack_ids: BTreeSet<String>,
    #[serde(default)]
    pub reviewed_pack_evidence:
        std::collections::BTreeMap<String, Vec<super::evidence::EvidenceRef>>,
    #[serde(default)]
    pub review_issues: Vec<ReviewIssue>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chapters: Vec<ChapterOutline>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<AttachmentBinding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slots: Vec<TemplateContent>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub slots_submitted: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub finished: bool,
}

impl Draft {
    pub fn is_empty(&self) -> bool {
        self.chapters.is_empty()
            && self.bindings.is_empty()
            && self.slots.is_empty()
            && !self.slots_submitted
            && !self.finished
    }
}

pub fn unmapped_forms(input: &FrozenInput, draft: &Draft) -> Vec<String> {
    let mapped: BTreeSet<_> = draft
        .bindings
        .iter()
        .map(|binding| binding.form_id.as_str())
        .collect();
    attachment_form_ids(input)
        .into_iter()
        .filter(|id| !mapped.contains(id.as_str()))
        .collect()
}

/// Apply already-resolved host provenance. Model wire validation belongs to source_wire.
pub fn apply_canonical(
    input: &FrozenInput,
    draft: &mut Draft,
    requirement_ids: &BTreeSet<String>,
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    let mut candidate = draft.clone();
    let value = apply_inner(input, &mut candidate, requirement_ids, name, args)?;
    *draft = candidate;
    Ok(value)
}

fn apply_inner(
    input: &FrozenInput,
    draft: &mut Draft,
    requirement_ids: &BTreeSet<String>,
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    let upsert = if matches!(name, "put_chapters" | "bind_forms" | "put_slots") {
        match args["mode"].as_str() {
            Some("replace") => false,
            Some("upsert") => true,
            _ => return Err("mode must be explicitly replace or upsert".into()),
        }
    } else {
        false
    };
    match name {
        "put_chapters" if !upsert => put_chapters(input, draft, requirement_ids, args),
        "put_chapters" => {
            let incoming: Vec<ChapterOutline> =
                serde_json::from_value(args["chapters"].clone()).map_err(|e| e.to_string())?;
            let mut rows = draft.chapters.clone();
            let mut seen = BTreeSet::new();
            for chapter in incoming {
                if !seen.insert(chapter.id.clone()) {
                    return Err("duplicate chapter in incremental batch".into());
                }
                match rows.iter_mut().find(|row| row.id == chapter.id) {
                    Some(existing) => *existing = chapter,
                    None => rows.push(chapter),
                }
            }
            put_chapters(input, draft, requirement_ids, &json!({"chapters":rows}))
        }
        "bind_forms" if upsert => upsert_forms(input, draft, args),
        "bind_forms" => bind_forms(input, draft, args),
        "put_slots" if upsert => upsert_slots(input, draft, args),
        "put_slots" => put_slots(input, draft, args),
        "put_fulfillments" => put_fulfillments(input, draft, requirement_ids, args),
        "read_outline" => read_outline(input, draft, args),
        "finish_outline" => finish(input, draft, requirement_ids),
        _ => Err("unknown outline tool".into()),
    }
}

fn put_chapters(
    input: &FrozenInput,
    draft: &mut Draft,
    known_requirements: &BTreeSet<String>,
    args: &Value,
) -> Result<Value, String> {
    let rows = args["chapters"]
        .as_array()
        .ok_or("chapters must be an array")?;
    if rows.is_empty() {
        return Err("chapters must name at least one chapter".into());
    }
    let mut chapters = Vec::with_capacity(rows.len());
    let mut seen = HashSet::new();
    let mut seen_requirements = HashSet::new();
    let mut errors = Vec::new();
    for chapter in rows {
        match parse_chapter(
            chapter,
            known_requirements,
            &mut seen,
            &mut seen_requirements,
        ) {
            Ok(chapter) => chapters.push(chapter),
            Err(found) => errors.extend(found),
        }
    }
    if errors.is_empty() {
        errors.extend(validate_tree(&chapters));
    }
    if errors.is_empty() {
        errors.extend(depth_gaps(input, &chapters));
    }
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    // Chapter edits are transactional. Existing targets must remain valid;
    // deleting or demoting their leaf requires an explicit target migration first.
    let mut probe = draft.clone();
    probe.chapters = chapters;
    validate_targets(input, &probe, known_requirements)?;
    probe.required_requirement_ids = known_requirements.clone();
    invalidate_review(&mut probe);
    *draft = probe;
    Ok(view(input, draft))
}

fn binding_sets<'a>(
    input: &FrozenInput,
    draft: &'a Draft,
) -> (HashSet<String>, HashSet<&'a str>, HashSet<&'a str>) {
    let attachments: HashSet<String> = attachment_form_ids(input).into_iter().collect();
    let chapters: HashSet<&str> = draft
        .chapters
        .iter()
        .map(|chapter| chapter.id.as_str())
        .collect();
    let groups: HashSet<&str> = draft
        .chapters
        .iter()
        .filter(|chapter| chapter.purpose == ChapterPurpose::Group)
        .map(|chapter| chapter.id.as_str())
        .collect();
    (attachments, chapters, groups)
}

fn parse_binding(
    binding: &Value,
    attachments: &HashSet<String>,
    chapters: &HashSet<&str>,
    groups: &HashSet<&str>,
    seen: &mut HashSet<String>,
) -> Result<AttachmentBinding, String> {
    let form_id = required(binding, "form_id")?;
    let chapter_id = required(binding, "chapter_id")?;
    if !attachments.contains(&form_id) {
        return Err(format!("form {form_id} is not an attachment table"));
    }
    if !chapters.contains(chapter_id.as_str()) {
        return Err(format!("chapter {chapter_id} is not in the outline"));
    }
    if groups.contains(chapter_id.as_str()) {
        return Err(format!(
            "chapter {chapter_id} is a group chapter and cannot take an attachment table"
        ));
    }
    if !seen.insert(form_id.clone()) {
        return Err(format!(
            "attachment table {form_id} is bound more than once"
        ));
    }
    Ok(AttachmentBinding {
        form_id,
        chapter_id,
    })
}

fn bind_forms(input: &FrozenInput, draft: &mut Draft, args: &Value) -> Result<Value, String> {
    let rows = args["bindings"]
        .as_array()
        .ok_or("bindings must be an array")?;
    let (attachments, chapters, groups) = binding_sets(input, draft);
    let mut bindings = Vec::with_capacity(rows.len());
    let mut seen = HashSet::new();
    let mut errors = Vec::new();
    for binding in rows {
        match parse_binding(binding, &attachments, &chapters, &groups, &mut seen) {
            Ok(parsed) => bindings.push(parsed),
            Err(error) => errors.push(error),
        }
    }
    if errors.is_empty() {
        let mut probe = draft.clone();
        probe.bindings = bindings.clone();
        errors.extend(render_chain_gaps(&chain_gaps(input, &probe)));
    }
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    let mut probe = draft.clone();
    probe.bindings = bindings.clone();
    validate_targets(input, &probe, &draft.required_requirement_ids)?;
    draft.bindings = bindings;
    invalidate_review(draft);
    Ok(view(input, draft))
}

/// Incrementally bind attachment forms: submitted rows are upserted by
/// `form_id`, untouched bindings are kept. Only the chains touched by this
/// call are probe-checked, so one bad chain no longer rejects the whole table.
fn upsert_forms(input: &FrozenInput, draft: &mut Draft, args: &Value) -> Result<Value, String> {
    let rows = args["bindings"]
        .as_array()
        .ok_or("bindings must be an array")?;
    if rows.is_empty() {
        return Err("bindings must name at least one binding".into());
    }
    let (attachments, chapters, groups) = binding_sets(input, draft);
    let mut upserts = Vec::with_capacity(rows.len());
    let mut seen = HashSet::new();
    let mut errors = Vec::new();
    for binding in rows {
        match parse_binding(binding, &attachments, &chapters, &groups, &mut seen) {
            Ok(parsed) => upserts.push(parsed),
            Err(error) => errors.push(error),
        }
    }
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    let mut probe = draft.clone();
    for upsert in &upserts {
        match probe
            .bindings
            .iter_mut()
            .find(|binding| binding.form_id == upsert.form_id)
        {
            Some(existing) => existing.chapter_id = upsert.chapter_id.clone(),
            None => probe.bindings.push(upsert.clone()),
        }
    }
    let chains = attachment_chains(input);
    let form_chain: std::collections::HashMap<&str, usize> = chains
        .iter()
        .enumerate()
        .flat_map(|(index, chain)| chain.iter().map(move |id| (id.as_str(), index)))
        .collect();
    let affected: HashSet<usize> = upserts
        .iter()
        .filter_map(|binding| form_chain.get(binding.form_id.as_str()).copied())
        .collect();
    let gaps: Vec<ChainGap> = chain_gaps(input, &probe)
        .into_iter()
        .filter(|gap| affected.contains(&gap.chain_index))
        .collect();
    if !gaps.is_empty() {
        return Err(render_chain_gaps(&gaps).join("\n"));
    }
    validate_targets(input, &probe, &draft.required_requirement_ids)?;
    draft.bindings = probe.bindings;
    invalidate_review(draft);
    Ok(view(input, draft))
}

fn parse_slot(
    input: &FrozenInput,
    slot: &Value,
    chapters: &HashSet<&str>,
    groups: &HashSet<&str>,
    delivered: &[super::evidence::EvidenceRef],
    seen: &mut HashSet<String>,
) -> Result<TemplateContent, String> {
    let slot_id = required(slot, "slot_id")?;
    let chapter_id = required(slot, "chapter_id")?;
    if !seen.insert(slot_id.clone()) {
        return Err(format!("duplicate slot {slot_id}"));
    }
    if !chapters.contains(chapter_id.as_str()) {
        return Err(format!("slot {slot_id} is not on a chapter"));
    }
    if groups.contains(chapter_id.as_str()) {
        return Err(format!(
            "group chapter {chapter_id} cannot carry template slots"
        ));
    }
    let content: TemplateBody = serde_json::from_value(slot["content"].clone())
        .map_err(|error| format!("slot {slot_id} content: {error}"))?;
    let (kind, text, match_query) = match &content {
        TemplateBody::SourceCopy { refs } => {
            if slot.get("text").is_some()
                || slot.get("match_query").is_some()
                || slot.get("blank_kind").is_some()
            {
                return Err(
                    "source_copy accepts exactly one reference; omit text, match_query and blank_kind; text is resolved by the host".into(),
                );
            }
            super::evidence::validate_evidence(refs, input, delivered)?;
            (
                SlotKind::FixedText,
                source_copy(input, refs)?,
                String::new(),
            )
        }
        TemplateBody::EditableBlank => {
            if slot.get("text").is_some() {
                return Err("editable blank cannot contain supplied text".into());
            }
            let kind = match slot["blank_kind"].as_str() {
                Some("signature") => SlotKind::Signature,
                Some("bidder_blank") => SlotKind::BidderBlank,
                _ => return Err("editable blank needs bidder_blank or signature blank_kind".into()),
            };
            (kind, String::new(), required(slot, "match_query")?)
        }
        TemplateBody::GeneratedExplanation { supporting_refs } => {
            if slot.get("match_query").is_some() || slot.get("blank_kind").is_some() {
                return Err("generated_explanation requires text and supporting_refs; omit match_query and blank_kind".into());
            }
            super::evidence::validate_evidence(supporting_refs, input, delivered)?;
            if supporting_refs.is_empty() {
                return Err("generated explanation needs supporting evidence".into());
            }
            (
                SlotKind::Instruction,
                required(slot, "text")?,
                String::new(),
            )
        }
    };
    let parsed = TemplateContent {
        slot_id,
        chapter_id,
        kind,
        content,
        text,
        response_required: kind.accepts_knowledge_response(),
        match_query,
    };
    Ok(parsed)
}

/// A copy slot is one source span. Separate slots express explicit paragraph or
/// cell boundaries, avoiding invented joining punctuation or ambiguous order.
pub fn source_copy(
    input: &FrozenInput,
    refs: &[super::evidence::EvidenceRef],
) -> Result<String, String> {
    if refs.len() != 1 {
        return Err("source_copy requires exactly one source span; use separate ordered slots for separate paragraphs or cells".into());
    }
    if refs
        .iter()
        .any(|reference| matches!(reference, super::evidence::EvidenceRef::ImageRegion { .. }))
    {
        return Err(
            "source_copy requires exact text or grid evidence, not an undelivered image".into(),
        );
    }
    let excerpts = super::evidence::resolve_evidence(input, refs)?;
    let text = excerpts
        .into_iter()
        .next()
        .ok_or("source_copy has no evidence")?
        .quote;
    if text.is_empty() {
        return Err("source_copy must contain source text".into());
    }
    Ok(text)
}

fn slot_sets(draft: &Draft) -> (HashSet<&str>, HashSet<&str>) {
    let chapters: HashSet<&str> = draft
        .chapters
        .iter()
        .map(|chapter| chapter.id.as_str())
        .collect();
    let groups: HashSet<&str> = draft
        .chapters
        .iter()
        .filter(|chapter| chapter.purpose == ChapterPurpose::Group)
        .map(|chapter| chapter.id.as_str())
        .collect();
    (chapters, groups)
}

fn put_slots(input: &FrozenInput, draft: &mut Draft, args: &Value) -> Result<Value, String> {
    let rows = args["slots"].as_array().ok_or("slots must be an array")?;
    let (chapters, groups) = slot_sets(draft);
    let mut slots = Vec::with_capacity(rows.len());
    let mut seen = HashSet::new();
    for slot in rows {
        slots.push(parse_slot(
            input,
            slot,
            &chapters,
            &groups,
            &draft.delivered_evidence,
            &mut seen,
        )?);
    }
    let mut probe = draft.clone();
    probe.slots = slots.clone();
    validate_targets(input, &probe, &draft.required_requirement_ids)?;
    draft.slots = slots;
    draft.slots_submitted = true;
    invalidate_review(draft);
    Ok(view(input, draft))
}

/// Incrementally write template slots: submitted rows are upserted by
/// `slot_id`, untouched slots are kept. Only the submitted rows are
/// validated; the full-readiness gate stays in `finish` and
/// `missing_response_slot`, so Organize can fill leaves round by round.
fn upsert_slots(input: &FrozenInput, draft: &mut Draft, args: &Value) -> Result<Value, String> {
    let rows = args["slots"].as_array().ok_or("slots must be an array")?;
    if rows.is_empty() {
        return Err("slots must name at least one slot".into());
    }
    let (chapters, groups) = slot_sets(draft);
    let mut parsed = Vec::with_capacity(rows.len());
    let mut seen = HashSet::new();
    for slot in rows {
        parsed.push(parse_slot(
            input,
            slot,
            &chapters,
            &groups,
            &draft.delivered_evidence,
            &mut seen,
        )?);
    }
    let mut probe = draft.clone();
    for slot in parsed {
        match probe
            .slots
            .iter_mut()
            .find(|existing| existing.slot_id == slot.slot_id)
        {
            Some(existing) => *existing = slot,
            None => probe.slots.push(slot),
        }
    }
    validate_targets(input, &probe, &draft.required_requirement_ids)?;
    draft.slots = probe.slots;
    draft.slots_submitted = true;
    invalidate_review(draft);
    Ok(view(input, draft))
}

fn finish(
    input: &FrozenInput,
    draft: &mut Draft,
    requirements: &BTreeSet<String>,
) -> Result<Value, String> {
    validate_final_outline(input, requirements, draft)?;
    if draft.reviewed_requirement_ids != draft.required_requirement_ids
        || draft.reviewed_pack_ids != draft.required_pack_ids
    {
        return Err(
            "semantic review must cover every requirement and reading pack before finishing".into(),
        );
    }
    for slot in &draft.slots {
        if matches!(slot.content, TemplateBody::GeneratedExplanation { .. })
            && !check_read_slot_complete(draft, slot)
        {
            return Err(format!(
                "Check must read the full generated slot body {} before finishing",
                slot.slot_id
            ));
        }
    }
    if draft
        .required_requirement_ids
        .iter()
        .any(|id| !draft.claim_comparisons.contains_key(id))
    {
        return Err(
            "semantic claim comparisons must cover every requirement before finishing".into(),
        );
    }
    draft.finished = true;
    Ok(view(input, draft))
}

fn invalidate_review(draft: &mut Draft) {
    draft.read_epoch = draft.read_epoch.saturating_add(1);
    draft.read_frames.retain(|frame| !frame.check);
    draft.evidence_continuations.clear();
    draft.metadata_records.clear();
    draft.pending_metadata_parts.clear();
    draft.finished = false;
    draft.check_reads = CheckReads::default();
    draft.claim_comparisons.clear();
    draft.reviewed_requirement_ids.clear();
    draft.reviewed_pack_ids.clear();
    draft.reviewed_pack_evidence.clear();
    draft.review_issues.clear();
}

fn put_fulfillments(
    input: &FrozenInput,
    draft: &mut Draft,
    requirements: &BTreeSet<String>,
    args: &Value,
) -> Result<Value, String> {
    let rows: Vec<Fulfillment> = serde_json::from_value(args["fulfillments"].clone())
        .map_err(|error| format!("fulfillments: {error}"))?;
    let mut seen = BTreeSet::new();
    let mut probe = draft.clone();
    for row in rows {
        if !seen.insert(row.requirement_id.clone()) {
            return Err("duplicate fulfillment in submission".into());
        }
        probe
            .fulfillments
            .retain(|existing| existing.requirement_id != row.requirement_id);
        probe.fulfillments.push(row);
    }
    probe.required_requirement_ids = requirements.clone();
    validate_targets(input, &probe, requirements)?;
    invalidate_review(&mut probe);
    *draft = probe;
    Ok(view(input, draft))
}

/// Revalidate persisted state independently of the sequence of accepted tools.
/// Used at finish, projection and publication. A previous successful mutation is
/// never proof that a target is legal in the final tree.
pub fn validate_final_outline(
    input: &FrozenInput,
    requirements: &BTreeSet<String>,
    draft: &Draft,
) -> Result<(), String> {
    if input.schema_version != 2 {
        return Err(
            "FrozenInput schema is unsupported; reparse instead of resuming an old snapshot".into(),
        );
    }
    if draft.chapters.is_empty() {
        return Err("put_chapters before finishing the outline".into());
    }
    if &draft.required_requirement_ids != requirements {
        return Err("outline requirement snapshot is stale".into());
    }
    if draft.requirements.keys().cloned().collect::<BTreeSet<_>>() != *requirements {
        return Err(
            "outline requirement records do not match the frozen discovery snapshot".into(),
        );
    }
    for (id, record) in &draft.requirements {
        if record.description.trim().is_empty()
            || record.evidence.is_empty()
            || record.source_section_id.is_empty()
        {
            return Err(format!(
                "requirement {id} has incomplete evidence provenance"
            ));
        }
        super::evidence::resolve_evidence(input, &record.evidence)?;
    }
    validate_targets(input, draft, requirements)?;
    let mut assigned = BTreeSet::new();
    for chapter in &draft.chapters {
        for id in &chapter.requirement_ids {
            if !requirements.contains(id) || !assigned.insert(id.clone()) {
                return Err(format!("unknown or duplicate requirement {id}"));
            }
        }
    }
    if &assigned != requirements {
        return Err("some requirements have no responsible response leaf".into());
    }
    let fulfilled: BTreeSet<_> = draft
        .fulfillments
        .iter()
        .map(|f| f.requirement_id.clone())
        .collect();
    if &fulfilled != requirements {
        return Err("some requirements have no actual fulfillment target".into());
    }
    if let Some(id) = unmapped_forms(input, draft).into_iter().next() {
        return Err(format!("attachment table {id} is not mapped to a chapter"));
    }
    if !draft.slots_submitted {
        return Err("put_slots before finishing the outline".into());
    }
    if let Some(id) = missing_response_slot(draft) {
        return Err(format!(
            "response chapter {id} has no template slot or form binding"
        ));
    }
    let gaps = granularity_gaps(input, draft);
    if !gaps.is_empty() {
        return Err(gaps.join("\n"));
    }
    Ok(())
}

fn response_leaf<'a>(draft: &'a Draft, id: &str) -> Result<&'a ChapterOutline, String> {
    draft.chapters.iter().find(|chapter| chapter.id == id && chapter.purpose == ChapterPurpose::Response
        && !draft.chapters.iter().any(|child| child.parent_id.as_deref() == Some(id)))
        .ok_or_else(|| format!("chapter {id} is not a Response leaf; dependent slots, forms and fulfillments must be migrated first"))
}

fn validate_targets(
    input: &FrozenInput,
    draft: &Draft,
    requirements: &BTreeSet<String>,
) -> Result<(), String> {
    let tree = validate_tree(&draft.chapters);
    if !tree.is_empty() {
        return Err(tree.join("\n"));
    }
    let mut chapter_ids = BTreeSet::new();
    for chapter in &draft.chapters {
        if !chapter_ids.insert(&chapter.id) {
            return Err("duplicate chapter identity".into());
        }
        if chapter.purpose == ChapterPurpose::Group && !chapter.requirement_ids.is_empty() {
            return Err(format!(
                "group chapter {} cannot own requirements",
                chapter.id
            ));
        }
        if !chapter.requirement_ids.is_empty() {
            response_leaf(draft, &chapter.id)?;
        }
        for id in &chapter.requirement_ids {
            if !requirements.contains(id) {
                return Err(format!("unknown requirement {id}"));
            }
        }
    }
    let attachments: BTreeSet<_> = attachment_form_ids(input).into_iter().collect();
    let mut forms = BTreeSet::new();
    for binding in &draft.bindings {
        response_leaf(draft, &binding.chapter_id)?;
        if !attachments.contains(&binding.form_id) || !forms.insert(&binding.form_id) {
            return Err(format!(
                "unknown or duplicate form binding {}",
                binding.form_id
            ));
        }
    }
    for (index, chain) in attachment_chains(input).iter().enumerate() {
        let positions: Vec<_> = chain
            .iter()
            .filter_map(|id| {
                draft
                    .bindings
                    .iter()
                    .position(|binding| binding.form_id == *id)
            })
            .collect();
        if positions.windows(2).any(|pair| pair[0] >= pair[1]) {
            let current = draft
                .bindings
                .iter()
                .filter(|binding| chain.contains(&binding.form_id))
                .map(|binding| binding.form_id.as_str())
                .collect::<Vec<_>>();
            return Err(format!(
                "attachment chain {index} bindings are not in source order: current={current:?}, expected={chain:?}; upsert keeps existing positions and appends new ids, so it cannot reorder; read all current bindings, then replace the complete valid binding list in source order"
            ));
        }
    }
    let mut slots = BTreeSet::new();
    for slot in &draft.slots {
        response_leaf(draft, &slot.chapter_id)?;
        if slot.slot_id.is_empty() || !slots.insert(&slot.slot_id) {
            return Err("duplicate or empty slot identity".into());
        }
        match &slot.content {
            TemplateBody::SourceCopy { refs } => {
                if slot.text != source_copy(input, refs)?
                    || slot.kind != SlotKind::FixedText
                    || slot.response_required
                    || !slot.match_query.is_empty()
                {
                    return Err(format!(
                        "source_copy {} differs from frozen source",
                        slot.slot_id
                    ));
                }
            }
            TemplateBody::EditableBlank => {
                if !slot.kind.accepts_knowledge_response()
                    || !slot.response_required
                    || !slot.text.is_empty()
                    || slot.match_query.trim().is_empty()
                {
                    return Err(format!(
                        "editable blank {} has invalid content",
                        slot.slot_id
                    ));
                }
            }
            TemplateBody::GeneratedExplanation { supporting_refs } => {
                if slot.kind != SlotKind::Instruction
                    || supporting_refs.is_empty()
                    || slot.text.trim().is_empty()
                    || slot.response_required
                    || !slot.match_query.is_empty()
                {
                    return Err(format!(
                        "generated explanation {} has invalid role",
                        slot.slot_id
                    ));
                }
                super::evidence::resolve_evidence(input, supporting_refs)?;
            }
        }
    }
    let mut fulfilled = BTreeSet::new();
    for fulfillment in &draft.fulfillments {
        if !requirements.contains(&fulfillment.requirement_id)
            || !fulfilled.insert(&fulfillment.requirement_id)
        {
            return Err("unknown or duplicate fulfillment requirement".into());
        }
        let chapter = response_leaf(draft, &fulfillment.primary_response_chapter_id)?;
        if !chapter
            .requirement_ids
            .contains(&fulfillment.requirement_id)
            || fulfillment.target_refs.is_empty()
        {
            return Err(format!(
                "fulfillment {} lacks its responsible leaf or targets",
                fulfillment.requirement_id
            ));
        }
        let mut target_ids = BTreeSet::new();
        let mut actual_response = false;
        for target in &fulfillment.target_refs {
            let identity = serde_json::to_string(target).map_err(|e| e.to_string())?;
            if !target_ids.insert(identity) {
                return Err("duplicate fulfillment target".into());
            }
            match target {
                TargetRef::TextSlot { slot_id } => {
                    let slot = draft
                        .slots
                        .iter()
                        .find(|slot| slot.slot_id == *slot_id && slot.chapter_id == chapter.id)
                        .ok_or_else(|| {
                            format!(
                                "fulfillment target slot {slot_id} is missing or on another chapter"
                            )
                        })?;
                    actual_response |= matches!(
                        slot.content,
                        TemplateBody::EditableBlank | TemplateBody::SourceCopy { .. }
                    );
                }
                TargetRef::FormBinding { form_id } => {
                    actual_response = true;
                    if !draft.bindings.iter().any(|binding| {
                        binding.form_id == *form_id && binding.chapter_id == chapter.id
                    }) {
                        return Err(format!(
                            "fulfillment target form {form_id} is missing or on another chapter"
                        ));
                    }
                }
                TargetRef::ManualTask {
                    task_id,
                    description,
                } => {
                    actual_response = true;
                    if task_id.trim().is_empty() || description.trim().is_empty() {
                        return Err(
                            "manual task must be visible with an identity and description".into(),
                        );
                    }
                }
            }
        }
        if !actual_response {
            return Err(format!(
                "fulfillment {} has only generated instructions; an actual response slot, source declaration, form or visible manual task is required",
                fulfillment.requirement_id
            ));
        }
    }
    Ok(())
}

/// First response chapter, in chapter order, that has no template slot.
pub fn missing_response_slot(draft: &Draft) -> Option<&str> {
    draft.chapters.iter().find_map(|chapter| {
        (chapter.purpose == ChapterPurpose::Response
            && !draft.slots.iter().any(|slot| slot.chapter_id == chapter.id)
            && !draft
                .bindings
                .iter()
                .any(|binding| binding.chapter_id == chapter.id))
        .then_some(chapter.id.as_str())
    })
}

pub const MAX_PAGE_BYTES: usize = 16_384;

/// The envelope, not only its rows, must fit. Cursor and version prevent silent
/// omission or replaying pages against a changed draft.
pub fn bounded_page(
    rows: &[Value],
    cursor: usize,
    max_bytes: usize,
    version: &str,
) -> Result<Value, String> {
    if cursor > rows.len() {
        return Err("page cursor is out of range".into());
    }
    if max_bytes == 0 {
        return Err("max_bytes exceeds the outline page budget".into());
    }
    let make = |items: &[Value], next: usize| json!({"version":version,"items":items,"next_cursor": if next < rows.len() { Some(next) } else { None },"remaining":rows.len()-next,"total":rows.len()});
    let mut end = cursor;
    if serde_json::to_vec(&make(&[], end))
        .map_err(|e| e.to_string())?
        .len()
        > max_bytes
    {
        return Err("page budget cannot fit the envelope".into());
    }
    // Serialize each row once. Serializing every growing prefix makes the
    // unbounded projection used by token fitting quadratic in source size.
    let mut item_bytes = 0usize;
    while end < rows.len() {
        let row_bytes = serde_json::to_vec(&rows[end])
            .map_err(|e| e.to_string())?
            .len();
        let next_bytes = item_bytes
            .checked_add(row_bytes)
            .and_then(|n| n.checked_add(usize::from(end > cursor)))
            .ok_or("page size overflow")?;
        let envelope_bytes = serde_json::to_vec(&make(&[], end + 1))
            .map_err(|e| e.to_string())?
            .len();
        if next_bytes
            .checked_add(envelope_bytes)
            .ok_or("page size overflow")?
            > max_bytes
        {
            break;
        }
        item_bytes = next_bytes;
        end += 1;
    }
    if end == cursor && end < rows.len() {
        return Err("one item exceeds page budget; read its bounded evidence separately".into());
    }
    Ok(make(&rows[cursor..end], end))
}

/// Independent of structural readiness: unresolved source judgments and
/// manual targets can never be presented as semantically ready.
pub(crate) fn needs_semantic_review(draft: &Draft) -> bool {
    !draft.review_issues.is_empty()
        || draft.requirements.values().any(|r| {
            r.extraction_quality != "explicit"
                || r.kind == "unknown"
                || r.obligation_strength == "unknown"
        })
        || draft.fulfillments.iter().any(|f| {
            f.target_refs
                .iter()
                .any(|t| matches!(t, TargetRef::ManualTask { .. }))
        })
}

fn view(input: &FrozenInput, draft: &Draft) -> Value {
    json!({
        "version":super::canonical_sha256(draft).unwrap_or_default(),
        "chapter_count":draft.chapters.len(), "binding_count":draft.bindings.len(),
        "slot_count":draft.slots.len(), "requirement_count":draft.required_requirement_ids.len(),
        "unassigned_count":draft.required_requirement_ids.iter().filter(|id| !draft.chapters.iter().any(|chapter|chapter.requirement_ids.contains(id))).count(),
        "unfulfilled_count":draft.required_requirement_ids.iter().filter(|id| !draft.fulfillments.iter().any(|f|f.requirement_id == **id)).count(),
        "unreviewed_requirement_count":draft.required_requirement_ids.difference(&draft.reviewed_requirement_ids).count(),
        "unreviewed_pack_count":draft.required_pack_ids.difference(&draft.reviewed_pack_ids).count(),
        "needs_review":needs_semantic_review(draft),
        "semantic_ready":draft.finished && draft.reviewed_requirement_ids==draft.required_requirement_ids
            && draft.reviewed_pack_ids==draft.required_pack_ids && !needs_semantic_review(draft),
        "slots_submitted":draft.slots_submitted,"finished":draft.finished,
        "readiness":readiness(input,draft)
    })
}

pub(crate) fn read_outline(
    input: &FrozenInput,
    draft: &Draft,
    args: &Value,
) -> Result<Value, String> {
    let max = args["max_bytes"].as_u64().unwrap_or(8192) as usize;
    if max == 0 {
        return Err("max_bytes exceeds outline budget".into());
    }
    let mode = args["mode"].as_str().unwrap_or("summary");
    if mode == "summary" {
        let value = view(input, draft);
        if serde_json::to_vec(&value).map_err(|e| e.to_string())?.len() > max {
            return Err("summary exceeds requested budget".into());
        }
        return Ok(value);
    }
    if mode == "slot_body" {
        return slot_body_page(draft, args, max);
    }
    let chapter_id = args["chapter_id"].as_str();
    let rows: Vec<Value> = match mode {
        "chapters" => draft.chapters.iter().filter(|c|chapter_id.is_none_or(|id|c.id==id || c.parent_id.as_deref()==Some(id))).map(|c|json!({"id":c.id,"parent_id":c.parent_id,"order":c.order,"title":c.title,"purpose":c.purpose,"requirement_count":c.requirement_ids.len()})).collect(),
        "slots" => draft.slots.iter().filter(|s|chapter_id.is_none_or(|id|s.chapter_id==id)).map(|s|json!({"slot_id":s.slot_id,"chapter_id":s.chapter_id,"kind":s.kind,"content":s.content,"text_bytes":s.text.len(),"body_mode":"slot_body","match_query":s.match_query})).collect(),
        "bindings" => draft.bindings.iter().filter(|s|chapter_id.is_none_or(|id|s.chapter_id==id)).map(|s|json!(s)).collect(),
        "fulfillments" => draft.fulfillments.iter().filter(|s|chapter_id.is_none_or(|id|s.primary_response_chapter_id==id)).map(|s|json!(s)).collect(),
        "forms" => form_cards(input,&unmapped_forms(input,draft)),
        "issues" => draft.review_issues.iter().map(|s|json!(s)).collect(),
        _ => return Err("read_outline mode must be summary, chapters, slots, bindings, fulfillments, forms or issues".into()),
    };
    let version = super::canonical_sha256(draft)?;
    let cursor = args["cursor"].as_u64().unwrap_or(0) as usize;
    if cursor > 0 && args["version"].as_str() != Some(&version) {
        return Err("outline page version changed; restart at cursor 0".into());
    }
    bounded_page(&rows, cursor, max, &version)
}

fn slot_body_page(draft: &Draft, args: &Value, max: usize) -> Result<Value, String> {
    let id = required(args, "slot_id")?;
    let slot = draft
        .slots
        .iter()
        .find(|slot| slot.slot_id == id)
        .ok_or("unknown template slot")?;
    let cursor = args["cursor"].as_u64().unwrap_or(0) as usize;
    if cursor > slot.text.len() || !slot.text.is_char_boundary(cursor) {
        return Err("slot body cursor must be an in-range UTF-8 boundary".into());
    }
    let version = super::canonical_sha256(slot)?;
    if cursor > 0 && args["version"].as_str() != Some(&version) {
        return Err("slot body version changed; restart at cursor 0".into());
    }
    let page = |end: usize| json!({"slot_id":slot.slot_id,"chapter_id":slot.chapter_id,"kind":slot.kind,"match_query":slot.match_query,"version":version,"start_byte":cursor,"end_byte":end,"text":&slot.text[cursor..end],"next_cursor":if end<slot.text.len(){Some(end)}else{None},"remaining":slot.text.len()-end,"total_bytes":slot.text.len()});
    if serde_json::to_vec(&page(cursor))
        .map_err(|e| e.to_string())?
        .len()
        > max
    {
        return Err("slot body envelope exceeds requested budget".into());
    }
    if serde_json::to_vec(&page(slot.text.len()))
        .map_err(|e| e.to_string())?
        .len()
        <= max
    {
        return Ok(page(slot.text.len()));
    }
    let mut end = cursor;
    for (offset, ch) in slot.text[cursor..].char_indices() {
        let next = cursor + offset + ch.len_utf8();
        if serde_json::to_vec(&page(next))
            .map_err(|e| e.to_string())?
            .len()
            > max
        {
            break;
        }
        end = next;
    }
    if end == cursor && cursor < slot.text.len() {
        return Err("slot body budget cannot fit one UTF-8 character".into());
    }
    Ok(page(end))
}

pub(crate) fn check_read_slot_complete(draft: &Draft, slot: &TemplateContent) -> bool {
    let Some(ranges) = draft.check_reads.slot_ranges.get(&slot.slot_id) else {
        return false;
    };
    if slot.text.is_empty() {
        return ranges.contains(&(0, 0));
    }
    let mut ranges = ranges.clone();
    ranges.sort_unstable();
    let mut covered = 0;
    for (start, end) in ranges {
        if start > covered {
            return false;
        }
        covered = covered.max(end);
        if covered >= slot.text.len() {
            return true;
        }
    }
    false
}

/// What is still missing, and the one next tool once the draft can be finished.
pub fn readiness(input: &FrozenInput, draft: &Draft) -> Value {
    let mut missing = Vec::new();
    if draft.chapters.is_empty() {
        missing.push("chapters are missing".to_string());
    }
    let unmapped = unmapped_forms(input, draft);
    if !unmapped.is_empty() {
        missing.push(format!(
            "{} attachment tables are not bound",
            unmapped.len()
        ));
    }
    if !draft.slots_submitted {
        missing.push("slots have not been submitted".to_string());
    }
    if let Some(id) = missing_response_slot(draft) {
        missing.push(format!("response chapter {id} has no template slot"));
    }
    missing.extend(granularity_gaps(input, draft));
    if draft
        .required_requirement_ids
        .iter()
        .any(|id| !draft.fulfillments.iter().any(|f| &f.requirement_id == id))
    {
        missing.push("requirements need actual fulfillment targets".into());
    }
    if let Err(error) = validate_targets(input, draft, &draft.required_requirement_ids) {
        missing.push(error);
    }
    let total_missing = missing.len();
    missing.truncate(8);
    for diagnostic in &mut missing {
        if diagnostic.chars().count() > 256 {
            *diagnostic = diagnostic.chars().take(253).collect::<String>() + "...";
        }
    }
    let ready = missing.is_empty();
    let mut value = json!({
        "ready": ready,
        "missing": missing,
        "missing_total":total_missing,
    });
    if ready {
        value["next"] = json!("finish_outline");
    }
    value
}

/// Flat merges are rejected once several attachment chains exist.
///
/// A chain is a continuation run from [`attachment_chains`]. With at least
/// [`DEPTH_CHAIN_MIN`] chains, every response chapter sits at
/// [`RESPONSE_LEAF_DEPTH`] under a group. One leaf may bind several chains.
/// One chain may not be split across leaves.
fn granularity_gaps(input: &FrozenInput, draft: &Draft) -> Vec<String> {
    let mut gaps = depth_gaps(input, &draft.chapters);
    gaps.extend(render_chain_gaps(&chain_gaps(input, draft)));
    gaps
}

fn depth_gaps(input: &FrozenInput, chapters: &[ChapterOutline]) -> Vec<String> {
    if attachment_chains(input).len() < DEPTH_CHAIN_MIN {
        return Vec::new();
    }
    let mut gaps = Vec::new();
    for chapter in chapters {
        if chapter.purpose != ChapterPurpose::Response {
            continue;
        }
        let parent_is_group = chapter
            .parent_id
            .as_deref()
            .and_then(|id| chapters.iter().find(|candidate| candidate.id == id))
            .is_some_and(|parent| parent.purpose == ChapterPurpose::Group);
        if response_depth(chapters, chapter) < RESPONSE_LEAF_DEPTH || !parent_is_group {
            gaps.push(format!(
                "response chapter {} is not under a mid-level group",
                chapter.id
            ));
        }
    }
    gaps
}

/// One attachment chain bound across more than one response leaf.
///
/// Returned by [`chain_gaps`] so callers can render an actionable error or
/// filter to the chains a single incremental call touched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainGap {
    pub chain_index: usize,
    pub form_ids: Vec<String>,
    pub chapter_ids: Vec<String>,
    pub hint: String,
}

/// Attachment chains split across response leaves, as structured gaps.
/// Empty when fewer than [`DEPTH_CHAIN_MIN`] chains exist.
fn chain_gaps(input: &FrozenInput, draft: &Draft) -> Vec<ChainGap> {
    let chains = attachment_chains(input);
    let form_chain: std::collections::HashMap<&str, usize> = chains
        .iter()
        .enumerate()
        .flat_map(|(index, chain)| chain.iter().map(move |id| (id.as_str(), index)))
        .collect();
    let mut chain_chapters: std::collections::HashMap<usize, BTreeSet<&str>> =
        std::collections::HashMap::new();
    for binding in &draft.bindings {
        let Some(&chain) = form_chain.get(binding.form_id.as_str()) else {
            continue;
        };
        chain_chapters
            .entry(chain)
            .or_default()
            .insert(binding.chapter_id.as_str());
    }
    let mut split: Vec<_> = chain_chapters
        .iter()
        .filter(|(_, chapters)| chapters.len() > 1)
        .map(|(chain, _)| *chain)
        .collect();
    split.sort_unstable();
    let mut gaps = Vec::new();
    for chain in split {
        let form_ids: Vec<String> = chains.get(chain).cloned().unwrap_or_default();
        let chapter_ids: Vec<String> = chain_chapters
            .get(&chain)
            .map(|set| set.iter().map(|id| id.to_string()).collect())
            .unwrap_or_default();
        // Suggest the leaf that already holds most of the chain; ties fall
        // back to the last id in sort order, which is deterministic.
        let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        for binding in &draft.bindings {
            if form_chain.get(binding.form_id.as_str()) == Some(&chain) {
                *counts.entry(binding.chapter_id.as_str()).or_default() += 1;
            }
        }
        let suggested = chapter_ids
            .iter()
            .max_by_key(|id| counts.get(id.as_str()).copied().unwrap_or(0))
            .cloned()
            .unwrap_or_default();
        gaps.push(ChainGap {
            chain_index: chain,
            chapter_ids,
            hint: format!(
                "bind all {} forms to the same leaf (suggested: {suggested})",
                form_ids.len(),
            ),
            form_ids,
        });
    }
    gaps
}

/// Actionable rendering of [`chain_gaps`]: names the chain, its forms, the
/// chapters they are split across, and a suggested fix.
pub fn render_chain_gaps(gaps: &[ChainGap]) -> Vec<String> {
    gaps.iter()
        .map(|gap| {
            format!(
                "attachment chain {} ({}) is split across response chapters [{}]; fix: {}",
                gap.chain_index,
                gap.form_ids.join(", "),
                gap.chapter_ids.join(", "),
                gap.hint
            )
        })
        .collect()
}

fn response_depth(chapters: &[ChapterOutline], chapter: &ChapterOutline) -> usize {
    let mut depth = 1;
    let mut parent = chapter.parent_id.as_deref();
    let mut seen = HashSet::from([chapter.id.as_str()]);
    while let Some(id) = parent {
        if !seen.insert(id) {
            break;
        }
        depth += 1;
        parent = chapters
            .iter()
            .find(|candidate| candidate.id == id)
            .and_then(|candidate| candidate.parent_id.as_deref());
    }
    depth
}

fn validate_tree(chapters: &[ChapterOutline]) -> Vec<String> {
    let ids: HashSet<_> = chapters.iter().map(|chapter| chapter.id.as_str()).collect();
    let mut orders = std::collections::HashMap::new();
    let mut errors = Vec::new();
    for chapter in chapters {
        if chapter.id.is_empty() || chapter.title.trim().is_empty() {
            errors.push("chapter id and title are required".to_string());
        }
        if chapter.purpose == ChapterPurpose::Response
            && chapters
                .iter()
                .any(|child| child.parent_id.as_deref() == Some(chapter.id.as_str()))
        {
            errors.push(format!("response chapter {} must be a leaf", chapter.id));
        }
        if let Some(parent) = chapter.parent_id.as_deref()
            && !ids.contains(parent)
        {
            errors.push(format!("chapter {} parent is missing", chapter.id));
        }
        if let Some(existing) = orders.insert(
            (chapter.parent_id.as_deref(), chapter.order),
            chapter.id.as_str(),
        ) {
            errors.push(format!("chapter {} repeats a sibling order: parent={}, order={}, existing_chapter={existing}; upsert preserves omitted chapters; update the existing id or choose an unused sibling order; use replace only for an intentional complete tree", chapter.id, chapter.parent_id.as_deref().unwrap_or("<root>"), chapter.order));
        }
    }
    for chapter in chapters {
        let mut seen = HashSet::from([chapter.id.as_str()]);
        let mut parent = chapter.parent_id.as_deref();
        while let Some(id) = parent {
            if !seen.insert(id) {
                errors.push("chapter parent cycle".to_string());
                break;
            }
            parent = chapters
                .iter()
                .find(|candidate| candidate.id == id)
                .and_then(|candidate| candidate.parent_id.as_deref());
        }
    }
    errors
}

fn parse_chapter(
    chapter: &Value,
    known: &BTreeSet<String>,
    seen: &mut HashSet<String>,
    seen_requirements: &mut HashSet<String>,
) -> Result<ChapterOutline, Vec<String>> {
    let mut errors = Vec::new();
    let id = match required(chapter, "id") {
        Ok(id) => id,
        Err(error) => return Err(vec![error]),
    };
    if !seen.insert(id.clone()) {
        errors.push(format!("duplicate chapter {id}"));
    }
    let parent_id = match &chapter["parent_id"] {
        Value::Null => Some(None),
        Value::String(parent) if !parent.is_empty() => Some(Some(parent.clone())),
        _ => {
            errors.push(format!("chapter {id} parent_id must be an id or null"));
            None
        }
    };
    let purpose = match chapter["purpose"].as_str() {
        Some("group") => Some(ChapterPurpose::Group),
        Some("response") => Some(ChapterPurpose::Response),
        _ => {
            errors.push(format!("chapter {id} purpose must be group or response"));
            None
        }
    };
    let order = match chapter["order"].as_u64() {
        Some(order) => Some(order as usize),
        None => {
            errors.push(format!("chapter {id} order must be a non-negative integer"));
            None
        }
    };
    let title = match required(chapter, "title") {
        Ok(title) => Some(title),
        Err(error) => {
            errors.push(format!("chapter {id} {error}"));
            None
        }
    };
    let (requirement_ids, requirement_errors) =
        take_requirement_ids(chapter, &id, known, seen_requirements);
    errors.extend(requirement_errors);
    if purpose == Some(ChapterPurpose::Group) && !requirement_ids.is_empty() {
        errors.push(format!("group chapter {id} cannot own requirements"));
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(ChapterOutline {
        id,
        parent_id: parent_id.expect("parent checked"),
        order: order.expect("order checked"),
        title: title.expect("title checked"),
        purpose: purpose.expect("purpose checked"),
        requirement_ids,
    })
}

fn take_requirement_ids(
    chapter: &Value,
    id: &str,
    known: &BTreeSet<String>,
    seen: &mut HashSet<String>,
) -> (Vec<String>, Vec<String>) {
    let Some(rows) = chapter.get("requirement_ids").and_then(Value::as_array) else {
        return (
            Vec::new(),
            vec![format!("chapter {id} requirement_ids must be an array")],
        );
    };
    let mut ids = Vec::with_capacity(rows.len());
    let mut errors = Vec::new();
    for requirement in rows {
        let Some(requirement) = requirement
            .as_str()
            .filter(|requirement| !requirement.is_empty())
        else {
            errors.push(format!("chapter {id} requirement_ids must be strings"));
            continue;
        };
        if !known.contains(requirement) {
            errors.push(format!("unknown requirement {requirement}"));
            continue;
        }
        if !seen.insert(requirement.to_string()) {
            errors.push(format!(
                "requirement {requirement} is assigned more than once"
            ));
            continue;
        }
        ids.push(requirement.to_string());
    }
    (ids, errors)
}

fn required(value: &Value, field: &str) -> Result<String, String> {
    value[field]
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("{field} is required"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::Source;
    fn input() -> FrozenInput {
        FrozenInput {
            schema_version: 2,
            project_id: "project".into(),
            document_set_id: "set".into(),
            documents: vec![],
            document_relations: vec![],
            source_units: vec![Source {
                source_unit_revision_id: "source".into(),
                document_id: "doc".into(),
                text: "附件".into(),
                locator: json!({"heading_path": "附件"}),
                ordinal: 0,
            }],
            structured_forms: vec![json!({
                "form_definition_revision_id": "form-1",
                "source_unit_revision_id": "source",
                "definition": {
                    "title": "source_unit:form-1",
                    "row_count": 2,
                    "column_count": 2,
                    "cells": [
                        {"row": 0, "column": 0, "text": "A"},
                        {"row": 0, "column": 1, "text": ""},
                        {"row": 1, "column": 0, "text": ""},
                        {"row": 1, "column": 1, "text": ""}
                    ]
                }
            })],
            decisions: vec![],
        }
    }
    fn fill_in(id: &str, source_id: &str, header: &str) -> Value {
        json!({
            "form_definition_revision_id": id,
            "source_unit_revision_id": source_id,
            "definition": {
                "title": "source_unit:form",
                "row_count": 2,
                "column_count": 2,
                "cells": [
                    {"row": 0, "column": 0, "text": header},
                    {"row": 0, "column": 1, "text": ""},
                    {"row": 1, "column": 0, "text": ""},
                    {"row": 1, "column": 1, "text": ""}
                ]
            }
        })
    }
    fn two_chains() -> FrozenInput {
        FrozenInput {
            schema_version: 2,
            project_id: "project".into(),
            document_set_id: "set".into(),
            documents: vec![],
            document_relations: vec![],
            source_units: vec![
                Source {
                    source_unit_revision_id: "left".into(),
                    document_id: "doc".into(),
                    text: String::new(),
                    locator: json!({"heading_path": "卷一 > 甲"}),
                    ordinal: 0,
                },
                Source {
                    source_unit_revision_id: "right".into(),
                    document_id: "doc".into(),
                    text: String::new(),
                    locator: json!({"heading_path": "卷一 > 乙"}),
                    ordinal: 1,
                },
            ],
            structured_forms: vec![
                fill_in("form-a", "left", "甲"),
                fill_in("form-b", "right", "乙"),
            ],
            decisions: vec![],
        }
    }
    fn continued_chain() -> FrozenInput {
        FrozenInput {
            schema_version: 2,
            project_id: "project".into(),
            document_set_id: "set".into(),
            documents: vec![],
            document_relations: vec![],
            source_units: vec![
                Source {
                    source_unit_revision_id: "left".into(),
                    document_id: "doc".into(),
                    text: String::new(),
                    locator: json!({"heading_path": "卷一"}),
                    ordinal: 0,
                },
                Source {
                    source_unit_revision_id: "right".into(),
                    document_id: "doc".into(),
                    text: String::new(),
                    locator: json!({"heading_path": ""}),
                    ordinal: 1,
                },
                Source {
                    source_unit_revision_id: "other".into(),
                    document_id: "doc".into(),
                    text: String::new(),
                    locator: json!({"heading_path": "卷二"}),
                    ordinal: 2,
                },
            ],
            structured_forms: vec![
                fill_in("form-a", "left", "甲"),
                fill_in("form-b", "right", "甲"),
                fill_in("form-c", "other", "乙"),
            ],
            decisions: vec![],
        }
    }
    fn three_level_chapters() -> Value {
        json!({"mode":"replace","chapters":[
            {"id":"root","parent_id":null,"order":0,"title":"根","purpose":"group","requirement_ids":[]},
            {"id":"mid","parent_id":"root","order":0,"title":"中","purpose":"group","requirement_ids":[]},
            {"id":"leaf-a","parent_id":"mid","order":0,"title":"甲","purpose":"response","requirement_ids":[]},
            {"id":"leaf-b","parent_id":"mid","order":1,"title":"乙","purpose":"response","requirement_ids":[]}
        ]})
    }

    #[test]
    fn collection_modes_reject_missing_and_unknown_without_mutation() {
        let input = input();
        let mut draft = Draft::default();
        let saved = draft.clone();
        for name in ["put_chapters", "bind_forms", "put_slots"] {
            for args in [json!({}), json!({"mode":"append"})] {
                assert!(
                    apply_canonical(&input, &mut draft, &BTreeSet::new(), name, &args)
                        .unwrap_err()
                        .contains("mode")
                );
                assert_eq!(draft, saved);
            }
        }
    }

    #[test]
    fn groups_cannot_own_requirements_or_targets() {
        let input = input();
        let mut draft = Draft::default();
        let reqs = BTreeSet::from(["r".to_string()]);
        assert!(apply_canonical(&input,&mut draft,&reqs,"put_chapters",&json!({"mode":"replace","chapters":[{"id":"g","parent_id":null,"order":0,"title":"G","purpose":"group","requirement_ids":["r"]}]})).unwrap_err().contains("group"));
        assert!(draft.is_empty());
    }

    #[test]
    fn response_demotion_or_deletion_preserves_state_and_dependencies() {
        let input = input();
        let mut draft = Draft::default();
        let empty = BTreeSet::new();
        let rows = json!({"mode":"replace","chapters":[{"id":"leaf","parent_id":null,"order":0,"title":"L","purpose":"response","requirement_ids":[]}]});
        apply_canonical(&input, &mut draft, &empty, "put_chapters", &rows).unwrap();
        apply_canonical(
            &input,
            &mut draft,
            &empty,
            "bind_forms",
            &json!({"mode":"replace","bindings":[{"form_id":"form-1","chapter_id":"leaf"}]}),
        )
        .unwrap();
        let saved = draft.clone();
        let mut demote = rows.clone();
        demote["chapters"][0]["purpose"] = json!("group");
        assert!(
            apply_canonical(&input, &mut draft, &empty, "put_chapters", &demote)
                .unwrap_err()
                .contains("migrated")
        );
        assert_eq!(draft, saved);
        let mut remove = rows;
        remove["chapters"][0]["id"] = json!("replacement");
        assert!(apply_canonical(&input, &mut draft, &empty, "put_chapters", &remove).is_err());
        assert_eq!(draft, saved);
    }

    #[test]
    fn one_continuation_chain_cannot_split_and_must_be_complete() {
        let mut input = continued_chain();
        input
            .structured_forms
            .retain(|f| f["form_definition_revision_id"] != "form-c");
        input
            .source_units
            .retain(|s| s.source_unit_revision_id != "third");
        assert_eq!(attachment_chains(&input).len(), 1);
        let mut draft = Draft::default();
        let empty = BTreeSet::new();
        apply_canonical(
            &input,
            &mut draft,
            &empty,
            "put_chapters",
            &three_level_chapters(),
        )
        .unwrap();
        let split = json!({"mode":"replace","bindings":[{"form_id":"form-a","chapter_id":"leaf-a"},{"form_id":"form-b","chapter_id":"leaf-b"}]});
        assert!(
            apply_canonical(&input, &mut draft, &empty, "bind_forms", &split)
                .unwrap_err()
                .contains("split")
        );
        assert!(draft.bindings.is_empty());
        apply_canonical(
            &input,
            &mut draft,
            &empty,
            "bind_forms",
            &json!({"mode":"upsert","bindings":[{"form_id":"form-a","chapter_id":"leaf-a"}]}),
        )
        .unwrap();
        assert!(
            validate_final_outline(&input, &empty, &draft)
                .unwrap_err()
                .contains("not mapped")
        );
        apply_canonical(
            &input,
            &mut draft,
            &empty,
            "bind_forms",
            &json!({"mode":"upsert","bindings":[{"form_id":"form-b","chapter_id":"leaf-a"}]}),
        )
        .unwrap();
        let mut duplicate = draft.clone();
        duplicate.bindings.push(duplicate.bindings[0].clone());
        assert!(
            validate_final_outline(&input, &empty, &duplicate)
                .unwrap_err()
                .contains("duplicate")
        );
    }

    #[test]
    fn multiple_independent_chains_preserve_depth_and_incremental_mapping() {
        let input = two_chains();
        let mut draft = Draft::default();
        let empty = BTreeSet::new();
        assert_eq!(attachment_chains(&input).len(), 2);
        assert!(apply_canonical(&input,&mut draft,&empty,"put_chapters",&json!({"mode":"replace","chapters":[{"id":"leaf","parent_id":null,"order":0,"title":"L","purpose":"response","requirement_ids":[]}]})).unwrap_err().contains("mid-level"));
        apply_canonical(
            &input,
            &mut draft,
            &empty,
            "put_chapters",
            &three_level_chapters(),
        )
        .unwrap();
        apply_canonical(
            &input,
            &mut draft,
            &empty,
            "bind_forms",
            &json!({"mode":"upsert","bindings":[{"form_id":"form-a","chapter_id":"leaf-a"}]}),
        )
        .unwrap();
        apply_canonical(
            &input,
            &mut draft,
            &empty,
            "bind_forms",
            &json!({"mode":"upsert","bindings":[{"form_id":"form-b","chapter_id":"leaf-b"}]}),
        )
        .unwrap();
        apply_canonical(
            &input,
            &mut draft,
            &empty,
            "put_slots",
            &json!({"mode":"replace","slots":[]}),
        )
        .unwrap();
        assert!(finish(&input, &mut draft, &empty).is_ok());
    }

    #[test]
    fn organize_contract_matrix_covers_all_writes_and_repair_modes() {
        // Synthetic equivalent of the observed multi-leaf Organize failures;
        // no private document content or model acceptance is represented here.
        let mut input = continued_chain();
        input.source_units[0].text = "合成声明：填写并签署。".into();
        let reference = super::super::evidence::EvidenceRef::Text {
            input_digest: super::super::evidence::input_digest(&input).unwrap(),
            unit_id: "left".into(),
            start_byte: 0,
            end_byte: input.source_units[0].text.len(),
        };
        let mut work = super::super::discover::DiscoverWork::plan(&input, 131072);
        let packs = work.claim(2);
        assert_eq!(packs.len(), 1);
        let pack = &packs[0];
        let records = (0..2)
            .map(|i| super::super::discover::PackRequirement {
                condition_support: vec![],
                obligation_strength: "mandatory".into(),
                extraction_quality: "explicit".into(),
                description: format!("Synthetic response {i}"),
                evidence: vec![reference.clone()],
                source_section_id: pack.atoms[0].section_id.clone(),
                kind: "format".into(),
            })
            .collect();
        work.submit(
            &input,
            &pack.id,
            super::super::discover::PackSubmit {
                call_id: "synthetic-organize".into(),
                claim_token: pack.claim_token.clone(),
                pack_revision: pack.pack_revision,
                requirements: records,
                no_requirement_reason: None,
                inspected_atom_ids: pack
                    .atoms
                    .iter()
                    .filter(|a| !a.context_only)
                    .map(|a| a.id.clone())
                    .collect(),
            },
        )
        .unwrap();
        let ids = work.requirement_ids();
        let ordered = ids.iter().cloned().collect::<Vec<_>>();
        let req_a = &ordered[0];
        let req_b = &ordered[1];
        let mut draft = Draft {
            requirements: work.requirement_records().clone(),
            ..Default::default()
        };
        let mut chapters = three_level_chapters();
        chapters["chapters"][2]["requirement_ids"] = json!([req_a]);
        chapters["chapters"][3]["requirement_ids"] = json!([req_b]);
        apply_canonical(&input, &mut draft, &ids, "put_chapters", &chapters).unwrap();
        let bindings = json!([{"form_id":"form-a","chapter_id":"leaf-a"},{"form_id":"form-b","chapter_id":"leaf-a"},{"form_id":"form-c","chapter_id":"leaf-b"}]);
        let before = draft.clone();
        let mut wrong = bindings.clone();
        wrong[0]["chapter_id"] = json!("mid");
        assert!(
            apply_canonical(
                &input,
                &mut draft,
                &ids,
                "bind_forms",
                &json!({"mode":"replace","bindings":wrong})
            )
            .is_err()
        );
        assert_eq!(draft, before);
        apply_canonical(
            &input,
            &mut draft,
            &ids,
            "bind_forms",
            &json!({"mode":"replace","bindings":bindings}),
        )
        .unwrap();
        // A preexisting partial list cannot be reordered by resubmitting A,B.
        let mut partial = draft.clone();
        partial.bindings.remove(0);
        let saved = partial.clone();
        let error = apply_canonical(
            &input,
            &mut partial,
            &ids,
            "bind_forms",
            &json!({"mode":"upsert","bindings":bindings}),
        )
        .unwrap_err();
        for part in ["current=", "expected=", "cannot reorder", "replace"] {
            assert!(error.contains(part), "{error}");
        }
        assert_eq!(partial, saved);
        apply_canonical(
            &input,
            &mut partial,
            &ids,
            "bind_forms",
            &json!({"mode":"replace","bindings":bindings}),
        )
        .unwrap();
        assert_eq!(partial.bindings, draft.bindings);
        let slots = json!([
            {"slot_id":"copy","chapter_id":"leaf-a","content":{"type":"source_copy","refs":[reference]}},
            {"slot_id":"blank-a","chapter_id":"leaf-a","content":{"type":"editable_blank"},"blank_kind":"bidder_blank","match_query":"材料A"},
            {"slot_id":"sign","chapter_id":"leaf-a","content":{"type":"editable_blank"},"blank_kind":"signature","match_query":"签署人"},
            {"slot_id":"explain","chapter_id":"leaf-a","content":{"type":"generated_explanation","supporting_refs":[reference]},"text":"填写说明"},
            {"slot_id":"blank-b","chapter_id":"leaf-b","content":{"type":"editable_blank"},"blank_kind":"bidder_blank","match_query":"材料B"}]);
        let saved = draft.clone();
        assert!(
            apply_canonical(
                &input,
                &mut draft,
                &ids,
                "put_slots",
                &json!({"mode":"replace","slots":slots})
            )
            .unwrap_err()
            .contains("outside_scope")
        );
        assert_eq!(draft, saved);
        assert_eq!(
            super::super::evidence::resolve_evidence(&input, std::slice::from_ref(&reference))
                .unwrap()[0]
                .quote,
            input.source_units[0].text
        );
        draft.delivered_evidence.push(reference.clone()); // explicit synthetic delivery seam
        apply_canonical(
            &input,
            &mut draft,
            &ids,
            "put_slots",
            &json!({"mode":"replace","slots":slots}),
        )
        .unwrap();
        let mut reversed = slots.as_array().unwrap().clone();
        reversed.reverse();
        apply_canonical(
            &input,
            &mut draft,
            &ids,
            "put_slots",
            &json!({"mode":"upsert","slots":reversed}),
        )
        .unwrap();
        assert_eq!(draft.slots[0].slot_id, "copy");
        apply_canonical(
            &input,
            &mut draft,
            &ids,
            "put_slots",
            &json!({"mode":"replace","slots":reversed}),
        )
        .unwrap();
        assert_eq!(draft.slots[0].slot_id, "blank-b");
        let fulfill = |targets: Value| json!({"fulfillments":[{"requirement_id":req_a,"primary_response_chapter_id":"leaf-a","target_refs":targets}]});
        for targets in [
            json!([]),
            json!([{"type":"text_slot","slot_id":"blank-b"}]),
            json!([{"type":"form_binding","form_id":"form-c"}]),
            json!([{"type":"text_slot","slot_id":"explain"}]),
        ] {
            let saved = draft.clone();
            assert!(
                apply_canonical(
                    &input,
                    &mut draft,
                    &ids,
                    "put_fulfillments",
                    &fulfill(targets)
                )
                .is_err()
            );
            assert_eq!(draft, saved);
        }
        apply_canonical(&input,&mut draft,&ids,"put_fulfillments",&fulfill(json!([{"type":"text_slot","slot_id":"copy"},{"type":"text_slot","slot_id":"blank-a"},{"type":"form_binding","form_id":"form-a"}]))).unwrap();
        apply_canonical(&input,&mut draft,&ids,"put_fulfillments",&json!({"fulfillments":[{"requirement_id":req_b,"primary_response_chapter_id":"leaf-b","target_refs":[{"type":"form_binding","form_id":"form-c"}]}]})).unwrap();
        validate_final_outline(&input, &ids, &draft).unwrap();
        assert_eq!(
            (
                draft.chapters.len(),
                draft.bindings.len(),
                draft.slots.len(),
                draft.fulfillments.len()
            ),
            (4, 3, 5, 2)
        );
        assert!(!draft.finished);
        assert!(draft.reviewed_pack_ids.is_empty());
        let mut checkpoint = super::super::acceptance::checkpoint(&input);
        checkpoint.outline_run.reading_packs = Some(work);
        checkpoint.outline_run.tool_draft = draft;
        assert_eq!(
            super::super::agent::current(&input, &checkpoint),
            super::super::agent::Duty::Check
        );
        assert!(
            checkpoint
                .outline_run
                .tool_draft
                .check_reads
                .evidence
                .is_empty()
        );
        assert!(!checkpoint.done); // Check-ready is not semantic acceptance or completion.
        let revision = checkpoint
            .outline_run
            .reading_packs
            .as_ref()
            .unwrap()
            .revision;
        let identity = json!({"tool":"read_requirements","duty":"Check","revision":revision,"input":checkpoint.input_sha256,"epoch":checkpoint.outline_run.tool_draft.read_epoch});
        checkpoint
            .outline_run
            .tool_draft
            .evidence_continuations
            .insert(
                "saved-cursor".into(),
                json!({"identity":identity,"page":{"items":[]}}),
            );
        checkpoint.transcript.clear();
        let host =
            super::super::agent::host_packet(&input, &checkpoint, 0, json!({}), json!({}), None);
        assert_eq!(host["check_work"]["remaining_comparisons"], 2);
        assert!(ids.contains(host["check_work"]["requirement_id"].as_str().unwrap()));
        assert_eq!(
            host["available_read_continuations"]["items"][0]["cursor"],
            "saved-cursor"
        );
        assert_eq!(
            checkpoint.outline_run.tool_draft.check_reads.evidence.len(),
            0
        );
        checkpoint.outline_run.tool_draft.read_epoch += 1;
        let stale =
            super::super::agent::host_packet(&input, &checkpoint, 0, json!({}), json!({}), None);
        assert_eq!(stale["available_read_continuations"]["items"], json!([]));
    }

    #[test]
    fn slot_variant_schema_and_host_contracts_agree() {
        let input = input();
        let reference = super::super::evidence::EvidenceRef::Text {
            input_digest: super::super::evidence::input_digest(&input).unwrap(),
            unit_id: "source".into(),
            start_byte: 0,
            end_byte: input.source_units[0].text.len(),
        };
        let wire_ref = json!({"source_key":format!("src_{}","a".repeat(64))});
        let variants = [
            json!({"slot_id":"copy","chapter_id":"leaf","content":{"type":"source_copy","refs":[wire_ref.clone()]}}),
            json!({"slot_id":"blank","chapter_id":"leaf","content":{"type":"editable_blank"},"blank_kind":"bidder_blank","match_query":"资格材料"}),
            json!({"slot_id":"signature","chapter_id":"leaf","content":{"type":"editable_blank"},"blank_kind":"signature","match_query":"签署人"}),
            json!({"slot_id":"explanation","chapter_id":"leaf","content":{"type":"generated_explanation","supporting_refs":[wire_ref]},"text":"说明"}),
        ];
        for row in &variants {
            super::super::agent::validate_arguments(
                "put_slots",
                &json!({"mode":"upsert","slots":[row]}),
            )
            .unwrap();
        }
        for count in [0, 2] {
            let mut bad = variants[0].clone();
            bad["content"]["refs"] = json!(vec![bad["content"]["refs"][0].clone(); count]);
            assert!(
                super::super::agent::validate_arguments(
                    "put_slots",
                    &json!({"mode":"upsert","slots":[bad]})
                )
                .is_err()
            );
        }
        for (index, field) in [(1, "match_query"), (1, "blank_kind"), (3, "text")] {
            let mut bad = variants[index].clone();
            bad.as_object_mut().unwrap().remove(field);
            assert!(
                super::super::agent::validate_arguments(
                    "put_slots",
                    &json!({"mode":"upsert","slots":[bad]})
                )
                .is_err()
            );
        }
        let mut unsupported = variants[3].clone();
        unsupported["content"]["supporting_refs"] = json!([]);
        assert!(
            super::super::agent::validate_arguments(
                "put_slots",
                &json!({"mode":"upsert","slots":[unsupported]})
            )
            .is_err()
        );

        for (index, field, value) in [
            (0, "match_query", json!("")),
            (0, "match_query", json!("repair")),
            (0, "text", json!("invented")),
            (0, "blank_kind", json!("signature")),
            (3, "match_query", json!("repair")),
            (3, "blank_kind", json!("signature")),
            (1, "text", json!("invented")),
        ] {
            let mut row = variants[index].clone();
            row[field] = value;
            assert!(
                super::super::agent::validate_arguments(
                    "put_slots",
                    &json!({"mode":"upsert","slots":[row]})
                )
                .is_err()
            );
        }
        let mut draft = Draft::default();
        let reqs = BTreeSet::new();
        apply_canonical(&input,&mut draft,&reqs,"put_chapters",&json!({"mode":"replace","chapters":[{"id":"leaf","parent_id":null,"order":0,"title":"Response","purpose":"response","requirement_ids":[]}]})).unwrap();
        draft.delivered_evidence.push(reference.clone());
        let mut slots = variants.to_vec();
        slots[0]["content"]["refs"] = json!([reference]);
        slots[3]["content"]["supporting_refs"] = slots[0]["content"]["refs"].clone();
        apply_canonical(
            &input,
            &mut draft,
            &reqs,
            "put_slots",
            &json!({"mode":"replace","slots":slots}),
        )
        .unwrap();
        assert_eq!(draft.slots.len(), 4);
        assert_eq!(draft.slots[0].text, input.source_units[0].text);
        assert!(draft.slots[1].text.is_empty());
        for index in [0, 3] {
            let before = serde_json::to_value(&draft).unwrap();
            let mut bad = slots[index].clone();
            bad["match_query"] = json!("repair");
            assert!(
                apply_canonical(
                    &input,
                    &mut draft,
                    &reqs,
                    "put_slots",
                    &json!({"mode":"upsert","slots":[bad]})
                )
                .is_err()
            );
            assert_eq!(serde_json::to_value(&draft).unwrap(), before);
        }
    }

    #[test]
    fn chapter_collision_names_both_roots_and_preserves_state() {
        let input = input();
        let mut draft = Draft::default();
        let ids = BTreeSet::new();
        let root = |id: &str, order: u64| json!({"id":id,"parent_id":null,"order":order,"title":"Root","purpose":"response","requirement_ids":[]});
        apply_canonical(
            &input,
            &mut draft,
            &ids,
            "put_chapters",
            &json!({"mode":"replace","chapters":[root("existing",0)]}),
        )
        .unwrap();
        let before = serde_json::to_value(&draft).unwrap();
        let error = apply_canonical(
            &input,
            &mut draft,
            &ids,
            "put_chapters",
            &json!({"mode":"upsert","chapters":[root("new",0)]}),
        )
        .unwrap_err();
        for part in [
            "chapter new",
            "parent=<root>",
            "order=0",
            "existing_chapter=existing",
            "upsert preserves",
        ] {
            assert!(error.contains(part), "{error}");
        }
        assert_eq!(serde_json::to_value(&draft).unwrap(), before);
        apply_canonical(
            &input,
            &mut draft,
            &ids,
            "put_chapters",
            &json!({"mode":"upsert","chapters":[root("existing",0)]}),
        )
        .unwrap();
        assert_eq!(draft.chapters.len(), 1);
        apply_canonical(
            &input,
            &mut draft,
            &ids,
            "put_chapters",
            &json!({"mode":"replace","chapters":[root("new",0)]}),
        )
        .unwrap();
        assert_eq!(draft.chapters.len(), 1);
        assert_eq!(draft.chapters[0].id, "new");
    }

    #[test]
    fn whole_serialized_pages_fit_and_require_stable_versions() {
        let rows = (0..1000)
            .map(|i| json!({"id":i,"description":"资格义务".repeat(8)}))
            .collect::<Vec<_>>();
        let mut cursor = 0;
        let mut count = 0;
        loop {
            let page = bounded_page(&rows, cursor, 1024, "v2").unwrap();
            assert!(serde_json::to_vec(&page).unwrap().len() <= 1024);
            count += page["items"].as_array().unwrap().len();
            match page["next_cursor"].as_u64() {
                Some(next) => cursor = next as usize,
                None => break,
            }
        }
        assert_eq!(count, 1000);
        assert!(bounded_page(&[json!({"text":"x".repeat(2048)})], 0, 1024, "v").is_err());
        let input = input();
        let draft = Draft::default();
        assert!(
            read_outline(
                &input,
                &draft,
                &json!({"mode":"slots","cursor":1,"version":"stale"})
            )
            .unwrap_err()
            .contains("version")
        );
    }

    #[test]
    fn incremental_page_sizes_preserve_exact_prefix_boundaries() {
        let rows = (0..24)
            .map(|i| json!({"id":i,"text":"条件\n\"\\".repeat(i)}))
            .collect::<Vec<_>>();
        for cursor in [0, 9, 23, 24] {
            for budget in (1..5000).step_by(17).chain([usize::MAX]) {
                let make = |end: usize| json!({"version":"v","items":&rows[cursor..end],"next_cursor":if end<rows.len(){Some(end)}else{None},"remaining":rows.len()-end,"total":rows.len()});
                let expected = if serde_json::to_vec(&make(cursor)).unwrap().len() > budget {
                    None
                } else {
                    let mut end = cursor;
                    while end < rows.len()
                        && serde_json::to_vec(&make(end + 1)).unwrap().len() <= budget
                    {
                        end += 1;
                    }
                    if end == cursor && end < rows.len() {
                        None
                    } else {
                        Some(make(end))
                    }
                };
                assert_eq!(
                    bounded_page(&rows, cursor, budget, "v").ok(),
                    expected,
                    "cursor={cursor} budget={budget}"
                );
            }
        }
    }
}
