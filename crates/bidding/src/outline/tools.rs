//! The outline model writes chapters, attachment bindings, and template slots.
//!
//! Eight tools, and a turn is shown only the ones its duty allows.
//!
//! 禁止硬编码: chapter shape uses attachment chains and the documented depth
//! constants. It does not match chapter titles or a sample keyword list.

use super::{
    ChapterOutline, ChapterPurpose, SlotKind, TemplateContent,
    chapters::{
        AttachmentBinding, DEPTH_CHAIN_MIN, RESPONSE_LEAF_DEPTH, attachment_chains,
        attachment_form_ids, form_cards,
    },
};
use crate::analysis::FrozenInput;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashSet};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Draft {
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

/// State the outline model reads. It names no retired tools.
///
/// `unmapped_forms` is a byte-bounded page of form cards, not bare ids.
/// `max_bytes` is the same per-request budget as the rest of the host packet.
pub fn model_state(input: &FrozenInput, draft: &Draft, max_bytes: usize) -> Value {
    let mut state = view(input, draft);
    state["unmapped_forms"] = page_form_cards(input, draft, max_bytes);
    state
}

fn page_form_cards(input: &FrozenInput, draft: &Draft, max_bytes: usize) -> Value {
    let cards = form_cards(input, &unmapped_forms(input, draft));
    if cards.is_empty() {
        return json!({"total": 0, "next": 0, "items": []});
    }
    crate::analysis::tools::bounded_page(&cards, 0, cards.len(), max_bytes.max(1)).unwrap_or_else(
        |error| json!({"total": cards.len(), "next": 0, "items": [], "error": error}),
    )
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

pub fn apply(
    input: &FrozenInput,
    draft: &mut Draft,
    requirement_ids: &BTreeSet<String>,
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    match name {
        "put_chapters" => put_chapters(input, draft, requirement_ids, args),
        "bind_forms" => bind_forms(input, draft, args),
        "bind_forms_append" => bind_forms_append(input, draft, args),
        "put_slots" => put_slots(input, draft, args),
        "put_slots_append" => put_slots_append(input, draft, args),
        "read_outline" => Ok(view(input, draft)),
        "finish_outline" => finish(input, draft),
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
    for id in known_requirements {
        if !seen_requirements.contains(id.as_str()) {
            errors.push(format!("requirement {id} is not on a chapter"));
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
    let ids: HashSet<_> = chapters.iter().map(|chapter| chapter.id.as_str()).collect();
    let slots_before = draft.slots.len();
    let mut dropped_bindings = Vec::new();
    draft.bindings.retain(|binding| {
        let keep = ids.contains(binding.chapter_id.as_str());
        if !keep {
            dropped_bindings.push(binding.form_id.clone());
        }
        keep
    });
    let mut dropped_slots = Vec::new();
    draft.slots.retain(|slot| {
        let keep = ids.contains(slot.chapter_id.as_str());
        if !keep {
            dropped_slots.push(slot.slot_id.clone());
        }
        keep
    });
    if draft.slots.len() != slots_before {
        draft.slots_submitted = false;
    }
    draft.chapters = chapters;
    draft.finished = false;
    let mut state = view(input, draft);
    state["dropped_bindings"] = json!(dropped_bindings);
    state["dropped_slots"] = json!(dropped_slots);
    Ok(state)
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
    draft.bindings = bindings;
    draft.finished = false;
    Ok(view(input, draft))
}

/// Incrementally bind attachment forms: submitted rows are upserted by
/// `form_id`, untouched bindings are kept. Only the chains touched by this
/// call are probe-checked, so one bad chain no longer rejects the whole table.
fn bind_forms_append(
    input: &FrozenInput,
    draft: &mut Draft,
    args: &Value,
) -> Result<Value, String> {
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
    draft.bindings = probe.bindings;
    draft.finished = false;
    Ok(view(input, draft))
}

fn parse_slot(
    slot: &Value,
    chapters: &HashSet<&str>,
    groups: &HashSet<&str>,
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
    let kind = match slot["kind"].as_str() {
        Some("fixed_text") => SlotKind::FixedText,
        Some("tender_value") => SlotKind::TenderValue,
        Some("instruction") => SlotKind::Instruction,
        Some("bidder_blank") => SlotKind::BidderBlank,
        Some("signature") => SlotKind::Signature,
        Some("preserved") => SlotKind::Preserved,
        _ => return Err(format!("slot {slot_id} kind is not recognized")),
    };
    let text = slot["text"].as_str().unwrap_or("");
    let match_query = slot["match_query"].as_str().unwrap_or("");
    if kind.accepts_knowledge_response() {
        if groups.contains(chapter_id.as_str()) {
            return Err(format!(
                "group chapter {chapter_id} cannot carry a knowledge response"
            ));
        }
        if !text.is_empty() || match_query.trim().is_empty() {
            return Err(format!(
                "slot {slot_id} stays empty and needs a match query"
            ));
        }
    } else if !match_query.is_empty() {
        return Err(format!("slot {slot_id} cannot carry a knowledge query"));
    }
    Ok(TemplateContent {
        slot_id,
        chapter_id,
        kind,
        text: text.to_string(),
        response_required: kind.accepts_knowledge_response(),
        match_query: match_query.to_string(),
    })
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
        slots.push(parse_slot(slot, &chapters, &groups, &mut seen)?);
    }
    draft.slots = slots;
    draft.slots_submitted = true;
    draft.finished = false;
    Ok(view(input, draft))
}

/// Incrementally write template slots: submitted rows are upserted by
/// `slot_id`, untouched slots are kept. Only the submitted rows are
/// validated; the full-readiness gate stays in `finish` and
/// `missing_response_slot`, so Organize can fill leaves round by round.
fn put_slots_append(input: &FrozenInput, draft: &mut Draft, args: &Value) -> Result<Value, String> {
    let rows = args["slots"].as_array().ok_or("slots must be an array")?;
    if rows.is_empty() {
        return Err("slots must name at least one slot".into());
    }
    let (chapters, groups) = slot_sets(draft);
    let mut parsed = Vec::with_capacity(rows.len());
    let mut seen = HashSet::new();
    for slot in rows {
        parsed.push(parse_slot(slot, &chapters, &groups, &mut seen)?);
    }
    for slot in parsed {
        match draft
            .slots
            .iter_mut()
            .find(|existing| existing.slot_id == slot.slot_id)
        {
            Some(existing) => *existing = slot,
            None => draft.slots.push(slot),
        }
    }
    draft.slots_submitted = true;
    draft.finished = false;
    Ok(view(input, draft))
}

fn finish(input: &FrozenInput, draft: &mut Draft) -> Result<Value, String> {
    if draft.chapters.is_empty() {
        return Err("put_chapters before finishing the outline".into());
    }
    let tree = validate_tree(&draft.chapters);
    if !tree.is_empty() {
        return Err(tree.join("\n"));
    }
    if let Some(form_id) = unmapped_forms(input, draft).into_iter().next() {
        return Err(format!(
            "attachment table {form_id} is not mapped to a chapter"
        ));
    }
    if !draft.slots_submitted {
        return Err("put_slots before finishing the outline".into());
    }
    if let Some(id) = missing_response_slot(draft) {
        return Err(format!("response chapter {id} has no template slot"));
    }
    let shape = granularity_gaps(input, draft);
    if !shape.is_empty() {
        return Err(shape.join("\n"));
    }
    draft.finished = true;
    Ok(view(input, draft))
}

/// First response chapter, in chapter order, that has no template slot.
pub fn missing_response_slot(draft: &Draft) -> Option<&str> {
    draft.chapters.iter().find_map(|chapter| {
        (chapter.purpose == ChapterPurpose::Response
            && !draft.slots.iter().any(|slot| slot.chapter_id == chapter.id))
        .then_some(chapter.id.as_str())
    })
}

fn view(input: &FrozenInput, draft: &Draft) -> Value {
    // B6: forest roots (chapters without a parent). A draft may legally hold
    // multiple roots (multi-package tender); validate_tree only checks
    // parent existence and acyclicity.
    let roots: Vec<&str> = draft
        .chapters
        .iter()
        .filter(|chapter| chapter.parent_id.is_none())
        .map(|chapter| chapter.id.as_str())
        .collect();
    let mut state = json!({
        "chapters": draft.chapters.iter().map(|chapter| json!({
            "id": chapter.id,
            "parent_id": chapter.parent_id,
            "order": chapter.order,
            "title": chapter.title,
            "purpose": chapter.purpose,
            "requirement_ids": chapter.requirement_ids,
        })).collect::<Vec<_>>(),
        "roots": roots,
        "bindings": draft.bindings,
        "slots": draft.slots.iter().map(|slot| json!({
            "slot_id": slot.slot_id,
            "chapter_id": slot.chapter_id,
            "kind": slot.kind,
            "text": slot.text,
            "match_query": slot.match_query,
        })).collect::<Vec<_>>(),
        "slots_submitted": draft.slots_submitted,
        "finished": draft.finished,
    });
    state["readiness"] = readiness(input, draft);
    state
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
    let ready = missing.is_empty();
    let mut value = json!({
        "ready": ready,
        "missing": missing,
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
    if chains.len() < DEPTH_CHAIN_MIN {
        return Vec::new();
    }
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
    let mut orders = HashSet::new();
    let mut errors = Vec::new();
    for chapter in chapters {
        if chapter.id.is_empty() || chapter.title.trim().is_empty() {
            errors.push("chapter id and title are required".to_string());
        }
        if let Some(parent) = chapter.parent_id.as_deref()
            && !ids.contains(parent)
        {
            errors.push(format!("chapter {} parent is missing", chapter.id));
        }
        if !orders.insert((chapter.parent_id.as_deref(), chapter.order)) {
            errors.push(format!("chapter {} repeats a sibling order", chapter.id));
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
            schema_version: 1,
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

    #[test]
    fn chapters_bindings_and_slots_persist_until_finish() {
        let input = input();
        let mut draft = Draft::default();
        apply(
            &input,
            &mut draft,
            &BTreeSet::new(),
            "put_chapters",
            &json!({"chapters":[
                {"id":"group","parent_id":null,"order":0,"title":"投标文件","purpose":"group","requirement_ids":[]},
                {"id":"letter","parent_id":"group","order":0,"title":"投标函","purpose":"response","requirement_ids":[]}
            ]}),
        )
        .unwrap();
        assert_eq!(draft.chapters.len(), 2);
        apply(
            &input,
            &mut draft,
            &BTreeSet::new(),
            "bind_forms",
            &json!({"bindings":[{"form_id":"form-1","chapter_id":"letter"}]}),
        )
        .unwrap();
        assert!(
            finish(&input, &mut draft)
                .unwrap_err()
                .contains("put_slots")
        );
        apply(
            &input,
            &mut draft,
            &BTreeSet::new(),
            "put_slots",
            &json!({"slots":[
                {"slot_id":"letter:fixed","chapter_id":"letter","kind":"fixed_text","text":"投标函","match_query":""},
                {"slot_id":"letter:bidder","chapter_id":"letter","kind":"bidder_blank","text":"","match_query":"投标人名称"}
            ]}),
        )
        .unwrap();
        let err = apply(
            &input,
            &mut draft,
            &BTreeSet::new(),
            "put_slots",
            &json!({"slots":[{"slot_id":"group:bidder","chapter_id":"group","kind":"bidder_blank","text":"","match_query":"名称"}]}),
        )
        .unwrap_err();
        assert!(err.contains("group chapter"));
        let finished = finish(&input, &mut draft).unwrap();
        assert_eq!(finished["finished"], json!(true));
        assert_eq!(finished["bindings"][0]["form_id"], json!("form-1"));
        assert_eq!(draft.slots.len(), 2);
        let again = apply(
            &input,
            &mut draft,
            &BTreeSet::new(),
            "read_outline",
            &json!({}),
        )
        .unwrap();
        assert_eq!(again["slots"][1]["match_query"], json!("投标人名称"));
        assert_eq!(again["readiness"]["ready"], json!(true));
        assert_eq!(again["readiness"]["next"], json!("finish_outline"));
    }

    #[test]
    fn bind_forms_rejects_a_group_chapter() {
        let input = input();
        let mut draft = Draft::default();
        apply(
            &input,
            &mut draft,
            &BTreeSet::new(),
            "put_chapters",
            &json!({"chapters":[
                {"id":"group","parent_id":null,"order":0,"title":"投标文件","purpose":"group","requirement_ids":[]},
                {"id":"letter","parent_id":"group","order":0,"title":"投标函","purpose":"response","requirement_ids":[]}
            ]}),
        )
        .unwrap();
        let err = apply(
            &input,
            &mut draft,
            &BTreeSet::new(),
            "bind_forms",
            &json!({"bindings":[{"form_id":"form-1","chapter_id":"group"}]}),
        )
        .unwrap_err();
        assert!(err.contains("group chapter"));
        assert!(draft.bindings.is_empty());
    }

    #[test]
    fn put_chapters_reports_every_requirement_violation_together() {
        let input = input();
        let mut draft = Draft::default();
        let known = BTreeSet::from(["pack-0:0".to_string(), "pack-1:0".to_string()]);
        let err = apply(
            &input,
            &mut draft,
            &known,
            "put_chapters",
            &json!({"chapters":[
                {"id":"letter","parent_id":null,"order":0,"title":"投标函","purpose":"response","requirement_ids":["missing","pack-0:0"]},
                {"id":"other","parent_id":"letter","order":0,"title":"其他","purpose":"response","requirement_ids":["pack-0:0"]}
            ]}),
        )
        .unwrap_err();
        assert!(err.contains("unknown requirement missing"), "{err}");
        assert!(
            err.contains("requirement pack-0:0 is assigned more than once"),
            "{err}"
        );
        assert!(
            err.contains("requirement pack-1:0 is not on a chapter"),
            "{err}"
        );
        assert!(draft.chapters.is_empty());
    }

    #[test]
    fn unmapped_form_cards_fit_the_request_budget() {
        let input = input();
        let state = model_state(&input, &Draft::default(), 4_000);
        let items = state["unmapped_forms"]["items"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["form_id"], "form-1");
        assert_eq!(items[0]["header"], json!(["A", ""]));
        assert_eq!(items[0]["ordinal"], 0);
        assert_eq!(items[0]["heading"], "附件");
        let tight = model_state(&input, &Draft::default(), 8);
        assert_eq!(
            tight["unmapped_forms"]["items"].as_array().unwrap().len(),
            0
        );
        assert!(tight["unmapped_forms"]["error"].is_string());
        assert_eq!(state["readiness"]["ready"], json!(false));
        assert!(state["readiness"].get("next").is_none());
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
            schema_version: 1,
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

    #[test]
    fn two_attachment_chains_need_a_three_level_tree() {
        let input = two_chains();
        assert_eq!(crate::outline::chapters::attachment_chains(&input).len(), 2);
        let mut draft = Draft::default();
        let flat = apply(
            &input,
            &mut draft,
            &BTreeSet::new(),
            "put_chapters",
            &json!({"chapters":[
                {"id":"root","parent_id":null,"order":0,"title":"根","purpose":"group","requirement_ids":[]},
                {"id":"letter","parent_id":"root","order":0,"title":"甲","purpose":"response","requirement_ids":[]},
                {"id":"other","parent_id":"root","order":1,"title":"乙","purpose":"response","requirement_ids":[]}
            ]}),
        )
        .unwrap_err();
        assert!(flat.contains("not under a mid-level group"), "{flat}");
        assert!(draft.chapters.is_empty());

        apply(
            &input,
            &mut draft,
            &BTreeSet::new(),
            "put_chapters",
            &json!({"chapters":[
                {"id":"root","parent_id":null,"order":0,"title":"根","purpose":"group","requirement_ids":[]},
                {"id":"mid","parent_id":"root","order":0,"title":"中","purpose":"group","requirement_ids":[]},
                {"id":"leaf","parent_id":"mid","order":0,"title":"叶","purpose":"response","requirement_ids":[]}
            ]}),
        )
        .unwrap();
        apply(
            &input,
            &mut draft,
            &BTreeSet::new(),
            "bind_forms",
            &json!({"bindings":[
                {"form_id":"form-a","chapter_id":"leaf"},
                {"form_id":"form-b","chapter_id":"leaf"}
            ]}),
        )
        .unwrap();
        assert_eq!(draft.bindings.len(), 2);

        apply(
            &input,
            &mut draft,
            &BTreeSet::new(),
            "put_chapters",
            &json!({"chapters":[
                {"id":"root","parent_id":null,"order":0,"title":"根","purpose":"group","requirement_ids":[]},
                {"id":"mid","parent_id":"root","order":0,"title":"中","purpose":"group","requirement_ids":[]},
                {"id":"leaf-a","parent_id":"mid","order":0,"title":"甲","purpose":"response","requirement_ids":[]},
                {"id":"leaf-b","parent_id":"mid","order":1,"title":"乙","purpose":"response","requirement_ids":[]}
            ]}),
        )
        .unwrap();
        apply(
            &input,
            &mut draft,
            &BTreeSet::new(),
            "bind_forms",
            &json!({"bindings":[
                {"form_id":"form-a","chapter_id":"leaf-a"},
                {"form_id":"form-b","chapter_id":"leaf-b"}
            ]}),
        )
        .unwrap();
        apply(
            &input,
            &mut draft,
            &BTreeSet::new(),
            "put_slots",
            &json!({"slots":[
                {"slot_id":"a:fixed","chapter_id":"leaf-a","kind":"fixed_text","text":"甲","match_query":""},
                {"slot_id":"b:fixed","chapter_id":"leaf-b","kind":"fixed_text","text":"乙","match_query":""}
            ]}),
        )
        .unwrap();
        let finished = finish(&input, &mut draft).unwrap();
        assert_eq!(finished["readiness"]["ready"], json!(true));
        assert_eq!(finished["chapters"].as_array().unwrap().len(), 4);
        assert_eq!(finished["roots"], json!(["root"]));

        let continued = continued_chain();
        assert_eq!(
            crate::outline::chapters::attachment_chains(&continued).len(),
            2
        );
        let mut draft = Draft::default();
        apply(
            &continued,
            &mut draft,
            &BTreeSet::new(),
            "put_chapters",
            &json!({"chapters":[
                {"id":"root","parent_id":null,"order":0,"title":"根","purpose":"group","requirement_ids":[]},
                {"id":"mid","parent_id":"root","order":0,"title":"中","purpose":"group","requirement_ids":[]},
                {"id":"leaf-a","parent_id":"mid","order":0,"title":"甲","purpose":"response","requirement_ids":[]},
                {"id":"leaf-b","parent_id":"mid","order":1,"title":"乙","purpose":"response","requirement_ids":[]}
            ]}),
        )
        .unwrap();
        let split = apply(
            &continued,
            &mut draft,
            &BTreeSet::new(),
            "bind_forms",
            &json!({"bindings":[
                {"form_id":"form-a","chapter_id":"leaf-a"},
                {"form_id":"form-b","chapter_id":"leaf-b"},
                {"form_id":"form-c","chapter_id":"leaf-b"}
            ]}),
        )
        .unwrap_err();
        assert!(
            split.contains("is split across response chapters"),
            "{split}"
        );
        assert!(draft.bindings.is_empty());
        apply(
            &continued,
            &mut draft,
            &BTreeSet::new(),
            "bind_forms",
            &json!({"bindings":[
                {"form_id":"form-a","chapter_id":"leaf-a"},
                {"form_id":"form-b","chapter_id":"leaf-a"},
                {"form_id":"form-c","chapter_id":"leaf-a"}
            ]}),
        )
        .unwrap();
        assert_eq!(draft.bindings.len(), 3);
    }

    fn three_level_chapters() -> Value {
        json!({"chapters":[
            {"id":"root","parent_id":null,"order":0,"title":"根","purpose":"group","requirement_ids":[]},
            {"id":"mid","parent_id":"root","order":0,"title":"中","purpose":"group","requirement_ids":[]},
            {"id":"leaf-a","parent_id":"mid","order":0,"title":"甲","purpose":"response","requirement_ids":[]},
            {"id":"leaf-b","parent_id":"mid","order":1,"title":"乙","purpose":"response","requirement_ids":[]}
        ]})
    }

    #[test]
    fn bind_forms_append_fixes_one_split_chain_incrementally() {
        // chain 0 = [form-a, form-b] (continuation), chain 1 = [form-c].
        let input = continued_chain();
        let mut draft = Draft::default();
        apply(
            &input,
            &mut draft,
            &BTreeSet::new(),
            "put_chapters",
            &three_level_chapters(),
        )
        .unwrap();

        // Full-table submit with one split chain is still rejected, now with
        // an actionable error naming the chain, forms, chapters, and fix.
        let err = apply(
            &input,
            &mut draft,
            &BTreeSet::new(),
            "bind_forms",
            &json!({"bindings":[
                {"form_id":"form-a","chapter_id":"leaf-a"},
                {"form_id":"form-b","chapter_id":"leaf-b"},
                {"form_id":"form-c","chapter_id":"leaf-b"}
            ]}),
        )
        .unwrap_err();
        assert!(
            err.contains(
                "attachment chain 0 (form-a, form-b) is split across response chapters [leaf-a, leaf-b]"
            ),
            "{err}"
        );
        assert!(err.contains("suggested:"), "{err}");
        assert!(draft.bindings.is_empty());

        // Incremental fix: only the split chain's rows; the rest is untouched.
        apply(
            &input,
            &mut draft,
            &BTreeSet::new(),
            "bind_forms_append",
            &json!({"bindings":[
                {"form_id":"form-a","chapter_id":"leaf-a"},
                {"form_id":"form-b","chapter_id":"leaf-a"}
            ]}),
        )
        .unwrap();
        assert_eq!(draft.bindings.len(), 2);
        apply(
            &input,
            &mut draft,
            &BTreeSet::new(),
            "bind_forms_append",
            &json!({"bindings":[{"form_id":"form-c","chapter_id":"leaf-b"}]}),
        )
        .unwrap();
        assert_eq!(draft.bindings.len(), 3);

        // An append that would split a chain is rejected without touching
        // the draft; chains not touched by the call are never reported.
        let err = apply(
            &input,
            &mut draft,
            &BTreeSet::new(),
            "bind_forms_append",
            &json!({"bindings":[{"form_id":"form-b","chapter_id":"leaf-b"}]}),
        )
        .unwrap_err();
        assert!(err.contains("attachment chain 0"), "{err}");
        assert!(!err.contains("attachment chain 1"), "{err}");
        assert_eq!(
            draft
                .bindings
                .iter()
                .find(|binding| binding.form_id == "form-b")
                .unwrap()
                .chapter_id,
            "leaf-a"
        );
    }

    #[test]
    fn put_slots_append_builds_slots_incrementally() {
        let input = two_chains();
        let mut draft = Draft::default();
        apply(
            &input,
            &mut draft,
            &BTreeSet::new(),
            "put_chapters",
            &three_level_chapters(),
        )
        .unwrap();

        // Round 1: one leaf only. No full-readiness probe on append.
        apply(
            &input,
            &mut draft,
            &BTreeSet::new(),
            "put_slots_append",
            &json!({"slots":[
                {"slot_id":"a:fixed","chapter_id":"leaf-a","kind":"fixed_text","text":"甲","match_query":""}
            ]}),
        )
        .unwrap();
        assert!(draft.slots_submitted);
        assert_eq!(missing_response_slot(&draft), Some("leaf-b"));

        // Round 2: a bad row is rejected; earlier rounds are kept.
        let err = apply(
            &input,
            &mut draft,
            &BTreeSet::new(),
            "put_slots_append",
            &json!({"slots":[
                {"slot_id":"b:fixed","chapter_id":"leaf-b","kind":"bogus","text":"乙","match_query":""}
            ]}),
        )
        .unwrap_err();
        assert!(err.contains("kind is not recognized"), "{err}");
        assert_eq!(draft.slots.len(), 1);

        // Round 3: upsert by slot_id, then the second leaf completes.
        apply(
            &input,
            &mut draft,
            &BTreeSet::new(),
            "put_slots_append",
            &json!({"slots":[
                {"slot_id":"a:fixed","chapter_id":"leaf-a","kind":"fixed_text","text":"甲改","match_query":""},
                {"slot_id":"b:fixed","chapter_id":"leaf-b","kind":"fixed_text","text":"乙","match_query":""}
            ]}),
        )
        .unwrap();
        assert_eq!(draft.slots.len(), 2);
        assert_eq!(
            draft
                .slots
                .iter()
                .find(|slot| slot.slot_id == "a:fixed")
                .unwrap()
                .text,
            "甲改"
        );
        assert!(missing_response_slot(&draft).is_none());
    }

    #[test]
    fn put_chapters_reports_dropped_bindings_and_slots() {
        let input = two_chains();
        let mut draft = Draft::default();
        apply(
            &input,
            &mut draft,
            &BTreeSet::new(),
            "put_chapters",
            &three_level_chapters(),
        )
        .unwrap();
        apply(
            &input,
            &mut draft,
            &BTreeSet::new(),
            "bind_forms",
            &json!({"bindings":[
                {"form_id":"form-a","chapter_id":"leaf-a"},
                {"form_id":"form-b","chapter_id":"leaf-b"}
            ]}),
        )
        .unwrap();
        apply(
            &input,
            &mut draft,
            &BTreeSet::new(),
            "put_slots",
            &json!({"slots":[
                {"slot_id":"a:fixed","chapter_id":"leaf-a","kind":"fixed_text","text":"甲","match_query":""},
                {"slot_id":"b:fixed","chapter_id":"leaf-b","kind":"fixed_text","text":"乙","match_query":""}
            ]}),
        )
        .unwrap();

        // Rename/drop leaf-b: the view must name what was dropped.
        let state = apply(
            &input,
            &mut draft,
            &BTreeSet::new(),
            "put_chapters",
            &json!({"chapters":[
                {"id":"root","parent_id":null,"order":0,"title":"根","purpose":"group","requirement_ids":[]},
                {"id":"mid","parent_id":"root","order":0,"title":"中","purpose":"group","requirement_ids":[]},
                {"id":"leaf-a","parent_id":"mid","order":0,"title":"甲","purpose":"response","requirement_ids":[]}
            ]}),
        )
        .unwrap();
        assert_eq!(state["dropped_bindings"], json!(["form-b"]));
        assert_eq!(state["dropped_slots"], json!(["b:fixed"]));
        assert_eq!(draft.bindings.len(), 1);
        assert_eq!(draft.slots.len(), 1);
    }

    fn continued_chain() -> FrozenInput {
        FrozenInput {
            schema_version: 1,
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
}
