//! Reading packs follow the structured document.
//!
//! The smallest unit is one heading, or one page when the parser has no heading.
//! A pack contains whole units only. A byte budget decides whether the next
//! heading can join the current pack. It never cuts inside a heading, a page,
//! or a table.

use crate::tender_analysis::{FrozenInput, Source};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const DEFAULT_PACK_CONCURRENCY: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextSpan {
    pub source_id: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormSpan {
    pub form_id: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParsePack {
    pub id: String,
    pub document_id: String,
    pub order: usize,
    /// Parent heading. Empty at the top of a document or on a page pack.
    pub context_heading: String,
    pub heading: String,
    pub text: Vec<TextSpan>,
    pub forms: Vec<FormSpan>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackStatus {
    Pending,
    Running,
    Failed,
    Committed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldError {
    pub path: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackFeedback {
    pub pack_id: String,
    pub call_id: String,
    pub arguments_sha256: String,
    pub errors: Vec<FieldError>,
    pub total: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChapterBlock {
    document_id: String,
    heading: String,
    context_heading: String,
    text: Vec<TextSpan>,
    forms: Vec<FormSpan>,
    bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PackRecord {
    pack: ParsePack,
    status: PackStatus,
    attempt: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    feedback: Option<PackFeedback>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoverWork {
    pub plan_sha256: String,
    packs: BTreeMap<String, PackRecord>,
    requirements: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PackRequirement {
    pub description: String,
    pub source_id: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PackSubmit {
    pub call_id: String,
    pub requirements: Vec<PackRequirement>,
}

impl DiscoverWork {
    pub fn plan(input: &FrozenInput, soft_max_bytes: usize) -> Self {
        let packs = plan_packs(input, soft_max_bytes);
        let plan_sha256 = plan_sha(&packs);
        let packs = packs
            .into_iter()
            .map(|pack| {
                (
                    pack.id.clone(),
                    PackRecord {
                        pack,
                        status: PackStatus::Pending,
                        attempt: 0,
                        feedback: None,
                    },
                )
            })
            .collect();
        Self {
            plan_sha256,
            packs,
            requirements: BTreeMap::new(),
        }
    }

    /// Mark up to `limit` pending packs running. Already running packs stay put.
    pub fn claim(&mut self, limit: usize) -> Vec<ParsePack> {
        let mut claimed = Vec::new();
        for record in self.packs.values_mut() {
            if claimed.len() >= limit {
                break;
            }
            if record.status == PackStatus::Pending {
                record.status = PackStatus::Running;
                record.attempt = record.attempt.saturating_add(1);
                claimed.push(record.pack.clone());
            }
        }
        claimed
    }

    pub fn submit(&mut self, pack_id: &str, submit: PackSubmit) -> Result<(), PackFeedback> {
        let errors = self.validation_errors(pack_id, &submit);
        if !errors.is_empty() {
            let feedback = feedback(pack_id, &submit, errors);
            if let Some(record) = self.packs.get_mut(pack_id) {
                record.status = PackStatus::Failed;
                record.feedback = Some(feedback.clone());
            }
            return Err(feedback);
        }
        let record = self
            .packs
            .get_mut(pack_id)
            .expect("validation found the pack");
        record.status = PackStatus::Committed;
        record.feedback = None;
        for (index, requirement) in submit.requirements.iter().enumerate() {
            self.requirements.insert(
                format!("{pack_id}:{index}"),
                requirement.description.clone(),
            );
        }
        Ok(())
    }

    pub fn session(&self, pack_id: &str) -> Result<serde_json::Value, String> {
        let record = self
            .packs
            .get(pack_id)
            .ok_or_else(|| format!("unknown reading pack {pack_id}"))?;
        Ok(serde_json::json!({
            "duty": "discover",
            "pack": record.pack,
            "status": record.status,
            "feedback": record.feedback,
        }))
    }

    pub fn status(&self, pack_id: &str) -> Option<PackStatus> {
        self.packs.get(pack_id).map(|record| record.status)
    }

    pub fn requirement_count(&self) -> usize {
        self.requirements.len()
    }

    fn validation_errors(&self, pack_id: &str, submit: &PackSubmit) -> Vec<FieldError> {
        let Some(record) = self.packs.get(pack_id) else {
            return vec![field(
                "",
                "unknown_pack",
                "reading pack is not in the frozen plan",
            )];
        };
        if !matches!(record.status, PackStatus::Running | PackStatus::Failed) {
            return vec![field(
                "",
                "pack_not_running",
                "submit a pack that was claimed for this turn",
            )];
        }
        let mut errors = Vec::new();
        for (index, requirement) in submit.requirements.iter().enumerate() {
            let path = format!("/requirements/{index}");
            let Some(span) = record
                .pack
                .text
                .iter()
                .find(|span| span.source_id == requirement.source_id)
            else {
                errors.push(field(
                    &path,
                    "outside_pack",
                    "requirement cites a source outside this heading",
                ));
                continue;
            };
            if requirement.start != span.start || requirement.end != span.end {
                errors.push(field(
                    &path,
                    "partial_chapter",
                    "requirement must cite the whole heading or page, not a byte slice",
                ));
            }
        }
        errors
    }
}

pub fn plan_packs(input: &FrozenInput, soft_max_bytes: usize) -> Vec<ParsePack> {
    let blocks = chapter_blocks(input);
    let mut packs = Vec::new();
    let mut group: Vec<ChapterBlock> = Vec::new();
    let mut bytes = 0usize;
    for block in blocks {
        if !group.is_empty() && bytes.saturating_add(block.bytes) > soft_max_bytes.max(1) {
            packs.push(pack_from(packs.len(), &group));
            group.clear();
            bytes = 0;
        }
        bytes = bytes.saturating_add(block.bytes);
        group.push(block);
    }
    if !group.is_empty() {
        packs.push(pack_from(packs.len(), &group));
    }
    packs
}

fn chapter_blocks(input: &FrozenInput) -> Vec<ChapterBlock> {
    let mut sources = input.source_units.iter().collect::<Vec<_>>();
    sources.sort_by(|left, right| {
        (&left.document_id, left.ordinal).cmp(&(&right.document_id, right.ordinal))
    });
    let mut blocks: Vec<ChapterBlock> = Vec::new();
    for source in sources {
        let heading = structural_heading(source);
        let context_heading = parent_heading(&heading);
        let same = blocks.last().is_some_and(|block| {
            block.document_id == source.document_id && block.heading == heading
        });
        if !same {
            blocks.push(ChapterBlock {
                document_id: source.document_id.clone(),
                heading,
                context_heading,
                text: Vec::new(),
                forms: Vec::new(),
                bytes: 0,
            });
        }
        let block = blocks.last_mut().expect("block was just ensured");
        let end = source.text.len();
        block.bytes = block.bytes.saturating_add(end);
        block.text.push(TextSpan {
            source_id: source.source_unit_revision_id.clone(),
            start: 0,
            end,
        });
        for form in input
            .structured_forms
            .iter()
            .filter(|form| form["source_unit_revision_id"] == source.source_unit_revision_id)
        {
            let Some(form_id) = form["form_definition_revision_id"].as_str() else {
                continue;
            };
            let cells = form_cell_count(&form["definition"]);
            block.forms.push(FormSpan {
                form_id: form_id.to_string(),
                start: 0,
                end: cells,
            });
            block.bytes = block.bytes.saturating_add(cells);
        }
    }
    blocks
}

fn pack_from(order: usize, blocks: &[ChapterBlock]) -> ParsePack {
    let first = &blocks[0];
    ParsePack {
        id: format!("pack-{order}"),
        document_id: first.document_id.clone(),
        order,
        context_heading: first.context_heading.clone(),
        heading: blocks
            .iter()
            .map(|block| block.heading.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        text: blocks.iter().flat_map(|block| block.text.clone()).collect(),
        forms: blocks
            .iter()
            .flat_map(|block| block.forms.clone())
            .collect(),
    }
}

fn structural_heading(source: &Source) -> String {
    let heading = source.locator["heading_path"].as_str().unwrap_or("").trim();
    if !heading.is_empty() {
        return heading.to_string();
    }
    if let Some(page) = source.locator["page_ordinal"].as_u64() {
        return format!("page:{page}");
    }
    format!("unit:{}", source.source_unit_revision_id)
}

fn parent_heading(heading: &str) -> String {
    heading
        .rsplit_once(" > ")
        .map(|(parent, _)| parent.to_string())
        .unwrap_or_default()
}

fn form_cell_count(definition: &serde_json::Value) -> usize {
    let rows = definition["row_count"].as_u64().unwrap_or(0) as usize;
    let columns = definition["column_count"].as_u64().unwrap_or(0) as usize;
    rows.saturating_mul(columns)
}

fn plan_sha(packs: &[ParsePack]) -> String {
    let bytes = serde_json::to_vec(packs).unwrap_or_default();
    hex::encode(Sha256::digest(bytes))
}

fn field(path: &str, code: &str, message: &str) -> FieldError {
    FieldError {
        path: path.to_string(),
        code: code.to_string(),
        message: message.to_string(),
    }
}

fn feedback(pack_id: &str, submit: &PackSubmit, errors: Vec<FieldError>) -> PackFeedback {
    let total = errors.len();
    PackFeedback {
        pack_id: pack_id.to_string(),
        call_id: submit.call_id.clone(),
        arguments_sha256: hex::encode(Sha256::digest(
            serde_json::to_vec(submit).unwrap_or_default(),
        )),
        errors,
        total,
        truncated: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn source(id: &str, ordinal: usize, text: &str, heading: &str) -> Source {
        Source {
            source_unit_revision_id: id.into(),
            document_id: "doc".into(),
            text: text.into(),
            locator: json!({"heading_path": heading}),
            ordinal,
        }
    }

    fn input(sources: Vec<Source>, forms: Vec<serde_json::Value>) -> FrozenInput {
        FrozenInput {
            schema_version: 1,
            project_id: "project".into(),
            document_set_id: "set".into(),
            documents: vec![],
            document_relations: vec![],
            source_units: sources,
            structured_forms: forms,
            decisions: vec![],
        }
    }

    #[test]
    fn a_heading_stays_whole_when_the_byte_budget_is_smaller_than_the_text() {
        let text = "投标函".repeat(20);
        let frozen = input(vec![source("s1", 0, &text, "第一章 > 投标函")], vec![]);
        let packs = plan_packs(&frozen, 4);
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].text[0].start, 0);
        assert_eq!(packs[0].text[0].end, text.len());
        assert_eq!(packs[0].context_heading, "第一章");
    }

    #[test]
    fn adjacent_headings_merge_only_on_chapter_boundaries() {
        let frozen = input(
            vec![
                source("a", 0, "商务", "第一章 > 投标函"),
                source("b", 1, "技术", "第一章 > 技术方案"),
                source("c", 2, "报价表正文", "第二章 > 报价"),
            ],
            vec![json!({
                "form_definition_revision_id": "form-price",
                "source_unit_revision_id": "c",
                "definition": {"row_count": 2, "column_count": 2}
            })],
        );
        let separate = plan_packs(&frozen, 4);
        assert_eq!(separate.len(), 3);
        assert!(
            separate
                .iter()
                .all(|pack| pack.text.iter().all(|span| span.start == 0))
        );
        let price = separate
            .iter()
            .find(|pack| pack.heading.contains("报价"))
            .unwrap();
        assert_eq!(
            price.forms,
            vec![FormSpan {
                form_id: "form-price".into(),
                start: 0,
                end: 4
            }]
        );

        let merged = plan_packs(&frozen, 10_000);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].text.len(), 3);
        assert!(merged[0].text.iter().all(|span| span.start == 0));
    }

    #[test]
    fn a_failed_pack_does_not_commit_or_clear_another_pack() {
        let frozen = input(
            vec![
                source("a", 0, "商务", "第一章 > 投标函"),
                source("b", 1, "技术", "第二章 > 技术方案"),
            ],
            vec![],
        );
        let mut work = DiscoverWork::plan(&frozen, 1);
        let claimed = work.claim(DEFAULT_PACK_CONCURRENCY);
        assert_eq!(claimed.len(), 2);
        work.submit(
            "pack-0",
            PackSubmit {
                call_id: "call-1".into(),
                requirements: vec![PackRequirement {
                    description: "投标函".into(),
                    source_id: "a".into(),
                    start: 0,
                    end: "商务".len(),
                }],
            },
        )
        .unwrap();
        let err = work
            .submit(
                "pack-1",
                PackSubmit {
                    call_id: "call-2".into(),
                    requirements: vec![PackRequirement {
                        description: "错章".into(),
                        source_id: "a".into(),
                        start: 0,
                        end: 1,
                    }],
                },
            )
            .unwrap_err();
        assert_eq!(err.errors[0].code, "outside_pack");
        assert_eq!(err.total, 1);
        assert!(!err.truncated);
        assert_eq!(work.status("pack-0"), Some(PackStatus::Committed));
        assert_eq!(work.status("pack-1"), Some(PackStatus::Failed));
        assert_eq!(work.requirement_count(), 1);
        let session = work.session("pack-1").unwrap();
        assert!(session.get("feedback").is_some());
        assert!(!session.to_string().contains("商务"));
    }
}
