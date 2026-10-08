//! Outline duties. A turn has one duty and the model sees only that duty's tools.
//!
//! Discover submits reading packs. Organize then writes the chapter tree, binds
//! every attachment table, and writes prescribed slots. Fill and published stay
//! on Template, which only writes slots. Check reads the draft and finishes it.

use crate::analysis::FrozenInput;
use crate::analysis::agent::Checkpoint;
use crate::analysis::draft::DraftStage;
use crate::analysis::outline_flow::Phase;
use serde_json::{Value, json};

pub const OUTLINE_PROMPT: &str = include_str!("prompts/outline.txt");
pub const TEMPLATE_PROMPT: &str = include_str!("prompts/template.txt");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Duty {
    Discover,
    Organize,
    Check,
    Template,
}

impl Duty {
    pub fn responsibility(self) -> &'static str {
        match self {
            Self::Discover => "读取招标文件并提交已检查范围和要求，不写模板正文",
            Self::Organize => {
                "把要求组织成稳定章节，把每个附件表绑定到唯一章节，并用 put_slots 写入规定模板。投标人和签字槽留空。不匹配知识库"
            }
            Self::Check => "核对章节、附件绑定和模板槽后结束大纲，不改章节",
            Self::Template => {
                "只用 put_slots 写规定模板。投标人和签字槽留空。不改章节，不填写我方事实"
            }
        }
    }

    /// Turn instruction placed in front of the shared outline contract.
    pub fn instructions(self) -> &'static str {
        match self {
            Self::Discover => {
                "本轮只做发现。只阅读已领取的阅读包，用 submit_pack 提交该包范围内的要求。同一包失败后把 repair 设为 true 再交。不要写章节，不要写模板，不要匹配知识库。"
            }
            Self::Organize => {
                "本轮写章节、附件绑定和规定模板槽。用检查点里的要求整理章节树，每个章节带上 requirement_ids，并把每个附件表绑定到唯一章节。用 put_slots 写入招标文件已经给出的文字。每个应答章节至少有一个槽。投标人和签字槽留空并带上 match_query，其他槽的 match_query 为空，分组章节不能带这两种槽。不要重新扫描招标文件，不要匹配知识库，不要填写我方事实。"
            }
            Self::Check => {
                "本轮只做收尾。用 read_outline 核对章节、附件绑定和模板槽，然后 finish_outline。不要改章节，不要重新扫描，不要写模板。"
            }
            Self::Template => {
                "本轮只写规定模板。用 put_slots 写入招标文件已经给出的文字，投标人和签字槽留空并带上 match_query。不要改章节，不要重新扫描。"
            }
        }
    }
}

/// Live duty. Discovery stays open until every reading pack is committed.
/// Organize then writes chapters, bindings, and slots together. A response
/// chapter without a slot stays in Organize. Fill and published stay
/// slot-only. Finish is `read_outline` and `finish_outline`.
pub fn select(
    stage: DraftStage,
    discovery_open: bool,
    chapters_ready: bool,
    unmapped_attachments: bool,
    slots_ready: bool,
) -> Duty {
    if matches!(stage, DraftStage::Fill | DraftStage::Published) {
        return Duty::Template;
    }
    if discovery_open {
        return Duty::Discover;
    }
    if !chapters_ready || unmapped_attachments || !slots_ready {
        return Duty::Organize;
    }
    Duty::Check
}

pub fn current(
    input: &crate::analysis::FrozenInput,
    state: &crate::analysis::agent::Checkpoint,
) -> Duty {
    let discovery_open = matches!(state.draft_stage, DraftStage::None | DraftStage::Outline)
        && state
            .outline_run
            .reading_packs
            .as_ref()
            .is_none_or(|work| !work.complete());
    select(
        state.draft_stage,
        discovery_open,
        !state.outline_run.tool_draft.chapters.is_empty(),
        !super::tools::unmapped_forms(input, &state.outline_run.tool_draft).is_empty(),
        state.outline_run.tool_draft.slots_submitted
            && super::tools::missing_response_slot(&state.outline_run.tool_draft).is_none(),
    )
}

/// Tools this duty may show the model.
pub fn schemas_for(duty: Duty) -> Vec<serde_json::Value> {
    let allowed = match duty {
        Duty::Discover => DISCOVER,
        Duty::Organize => ORGANIZE,
        Duty::Check => CHECK,
        Duty::Template => TEMPLATE,
    };
    schemas()
        .into_iter()
        .filter(|tool| {
            tool["function"]["name"]
                .as_str()
                .is_some_and(|name| allowed.contains(&name))
        })
        .collect()
}

pub fn duty(stage: DraftStage, phase: Phase, _unmapped_attachments: bool) -> Duty {
    match stage {
        DraftStage::Fill | DraftStage::Published => Duty::Template,
        DraftStage::None | DraftStage::Outline => match phase {
            Phase::Check | Phase::Complete => Duty::Check,
            Phase::Outline => Duty::Organize,
            Phase::Discover => Duty::Discover,
        },
    }
}

/// `Some` when this duty must reject the tool. `None` leaves non-draft tools
/// to the existing host checks.
pub fn deny(duty: Duty, tool: &str, unmapped_attachments: bool) -> Option<&'static str> {
    if tool == "finish_outline" && unmapped_attachments {
        return Some("attachment tables must be mapped to a chapter before the outline can finish");
    }
    let allowed: &[&str] = match duty {
        Duty::Discover => DISCOVER,
        Duty::Organize => ORGANIZE,
        Duty::Check => CHECK,
        Duty::Template => TEMPLATE,
    };
    if allowed.contains(&tool) {
        None
    } else {
        Some(duty_denial(duty))
    }
}

fn duty_denial(duty: Duty) -> &'static str {
    match duty {
        Duty::Discover => "discover cannot write chapters, template slots, or knowledge responses",
        Duty::Organize => "organize cannot rescan the tender or write knowledge responses",
        Duty::Check => "check duty cannot rescan, reorganize, or write template content",
        Duty::Template => "template duty cannot change chapters or rescan the tender",
    }
}

/// Outline tool contract. Fill and published use two of these tools.
pub fn schemas() -> Vec<Value> {
    serde_json::from_str(include_str!("../../schemas/outline-tools-v1.schema.json"))
        .expect("outline tools")
}

pub fn template_schemas() -> Vec<Value> {
    schemas()
        .into_iter()
        .filter(|tool| {
            matches!(
                tool["function"]["name"].as_str(),
                Some("put_slots" | "read_outline")
            )
        })
        .collect()
}

/// Duty instruction in front of the outline or template prompt.
pub fn system_prompt(duty: Duty, fill_stage: bool) -> String {
    let base = if fill_stage {
        TEMPLATE_PROMPT
    } else {
        OUTLINE_PROMPT
    };
    format!("{}\n\n{base}", duty.instructions())
}

/// Frozen sources for the outline model. Discovery reads claimed section packs.
pub fn source_index(input: &FrozenInput, max_bytes: usize) -> Value {
    let rows: Vec<Value> = input
        .source_units
        .iter()
        .map(|source| {
            json!({
                "source_id":source.source_unit_revision_id,"document_id":source.document_id,
                "ordinal":source.ordinal,"bytes":source.text.len(),"locator":source.locator,
                "forms":input.structured_forms.iter().filter(|f| f["source_unit_revision_id"]==source.source_unit_revision_id)
                    .map(|f| &f["form_definition_revision_id"]).collect::<Vec<_>>()
            })
        })
        .collect();
    json!({"total_sources":rows.len(),
        "sources":crate::analysis::tools::bounded_page(&rows,0,rows.len().max(1),max_bytes/2).unwrap_or_else(|e| json!({"error":e})),
        "documents":crate::analysis::tools::bounded_page(&input.documents,0,input.documents.len().max(1),max_bytes/4).unwrap_or_else(|e|json!({"error":e})),
        "instruction":"这些是冻结来源。发现只处理已领取的阅读包。组织时写章节，把每个附件表绑到唯一章节，并写入规定模板槽。"})
}

/// Host packet for an outline turn. Names no retired tools.
pub fn host_packet(
    input: &FrozenInput,
    state: &Checkpoint,
    max_bytes: usize,
    progress: Value,
    work: Value,
    preloaded_evidence: Option<&Value>,
) -> Value {
    let mut packet = json!({
        "progress": progress,
        "work": work,
    });
    if matches!(state.draft_stage, DraftStage::None | DraftStage::Outline) {
        packet["sources"] = source_index(input, max_bytes);
        packet["outline"] = super::tools::model_state(input, &state.outline_run.tool_draft);
    }
    if current(input, state) == Duty::Organize {
        let requirements = state
            .outline_run
            .reading_packs
            .as_ref()
            .map(super::discover::DiscoverWork::requirement_packet)
            .unwrap_or_default();
        packet["requirements"] = json!(requirements);
    }
    if let Some(evidence) = preloaded_evidence {
        packet["preloaded_evidence"] = evidence.clone();
    }
    packet
}

/// Apply one of the six outline tools. Anything else is refused.
pub fn apply(
    input: &FrozenInput,
    pack_max_chars: usize,
    state: &mut Checkpoint,
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    if name == "submit_pack" {
        let repair = args["repair"].as_bool().ok_or("repair must be a boolean")?;
        let mut forwarded = args.clone();
        forwarded
            .as_object_mut()
            .ok_or("submit_pack arguments must be an object")?
            .remove("repair");
        let tool = if repair {
            "repair_pack_scan"
        } else {
            "submit_pack_scan"
        };
        return super::discover::apply_pack_tool(
            &mut state.outline_run.reading_packs,
            input,
            super::discover::reading_budget(pack_max_chars),
            tool,
            &forwarded,
        );
    }
    if matches!(
        name,
        "put_chapters" | "bind_forms" | "put_slots" | "read_outline" | "finish_outline"
    ) {
        let known = state
            .outline_run
            .reading_packs
            .as_ref()
            .map(super::discover::DiscoverWork::requirement_ids)
            .unwrap_or_default();
        if name == "put_slots"
            && !matches!(state.draft_stage, DraftStage::Fill | DraftStage::Published)
        {
            let mut probe = state.outline_run.tool_draft.clone();
            let value = super::tools::apply(input, &mut probe, &known, name, args)?;
            if let Some(id) = super::tools::missing_response_slot(&probe) {
                return Err(format!("response chapter {id} has no template slot"));
            }
            state.outline_run.tool_draft = probe;
            return Ok(value);
        }
        let value =
            super::tools::apply(input, &mut state.outline_run.tool_draft, &known, name, args)?;
        if name == "finish_outline" {
            state.analysis.outline.phase = Phase::Complete;
            state.outline_run.phase = Phase::Complete;
        }
        return Ok(value);
    }
    Err("unknown outline tool".into())
}

const DISCOVER: &[&str] = &["submit_pack", "read_outline"];
const ORGANIZE: &[&str] = &["put_chapters", "bind_forms", "put_slots", "read_outline"];
const CHECK: &[&str] = &["read_outline", "finish_outline"];
const TEMPLATE: &[&str] = &["put_slots", "read_outline"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duties_keep_template_writing_away_from_discovery() {
        let discover = duty(DraftStage::Outline, Phase::Discover, false);
        assert!(deny(discover, "put_slots", false).is_some());
        assert!(deny(discover, "put_chapters", false).is_some());
        assert!(deny(discover, "submit_pack", false).is_none());
        assert!(deny(discover, "read_source", false).is_some());
        let organize = duty(DraftStage::Outline, Phase::Outline, true);
        assert!(deny(organize, "submit_pack", false).is_some());
        assert!(deny(organize, "put_chapters", false).is_none());
        assert!(deny(organize, "bind_forms", false).is_none());
        assert!(deny(organize, "put_slots", false).is_none());
        let template = duty(DraftStage::Fill, Phase::Complete, false);
        assert_eq!(template, Duty::Template);
        assert!(deny(template, "submit_pack", false).is_some());
        assert!(deny(template, "put_chapters", false).is_some());
        assert!(deny(template, "bind_forms", false).is_some());
        assert!(deny(template, "read_source", false).is_some());
        assert!(deny(template, "put_slots", false).is_none());
        let published = duty(DraftStage::Published, Phase::Complete, false);
        assert_eq!(published, Duty::Template);
        assert!(deny(published, "put_chapters", false).is_some());
        assert!(deny(published, "bind_forms", false).is_some());
        assert!(deny(published, "put_slots", false).is_none());
    }

    #[test]
    fn organize_binds_attachments_before_the_outline_can_finish() {
        let organize = duty(DraftStage::Outline, Phase::Outline, true);
        assert_eq!(organize, Duty::Organize);
        assert!(organize.responsibility().contains("附件表"));
        assert!(organize.instructions().contains("附件表"));
        assert!(Duty::Discover.instructions().contains("submit_pack"));
        assert!(Duty::Discover.instructions().contains("repair"));
        assert!(!Duty::Organize.instructions().contains("submit_pack"));
        assert!(Duty::Organize.instructions().contains("put_slots"));
        assert!(Duty::Organize.instructions().contains("留空"));
        assert!(Duty::Organize.instructions().contains("分组"));
        assert!(Duty::Template.instructions().contains("留空"));
        assert!(deny(organize, "finish_outline", true).is_some());
        assert!(deny(organize, "read_form", true).is_some());
        assert!(deny(organize, "bind_forms", true).is_none());
        assert!(deny(organize, "put_chapters", true).is_none());
    }

    #[test]
    fn a_turn_sees_only_its_duty_tools() {
        assert_eq!(
            select(DraftStage::Outline, true, false, false, false),
            Duty::Discover
        );
        assert_eq!(
            select(DraftStage::Outline, false, false, true, false),
            Duty::Organize
        );
        assert_eq!(
            select(DraftStage::Outline, false, true, true, false),
            Duty::Organize
        );
        assert_eq!(
            select(DraftStage::Outline, false, true, false, false),
            Duty::Organize
        );
        assert_eq!(
            select(DraftStage::Outline, false, true, false, true),
            Duty::Check
        );
        assert_eq!(
            select(DraftStage::Fill, true, false, true, false),
            Duty::Template
        );
        assert_eq!(
            select(DraftStage::Published, false, true, false, true),
            Duty::Template
        );
        let names = |duty| -> Vec<_> {
            schemas_for(duty)
                .iter()
                .map(|tool| tool["function"]["name"].as_str().unwrap().to_string())
                .collect()
        };
        assert_eq!(
            names(Duty::Discover),
            ["submit_pack".to_string(), "read_outline".to_string()]
        );
        assert_eq!(
            names(Duty::Check),
            ["read_outline".to_string(), "finish_outline".to_string()]
        );
        assert_eq!(
            names(Duty::Organize),
            [
                "put_chapters".to_string(),
                "bind_forms".to_string(),
                "put_slots".to_string(),
                "read_outline".to_string()
            ]
        );
        assert_eq!(
            names(Duty::Template),
            ["put_slots".to_string(), "read_outline".to_string()]
        );
        assert!(deny(Duty::Organize, "put_slots", false).is_none());
        assert!(deny(Duty::Check, "put_slots", false).is_some());
        assert!(deny(Duty::Check, "put_chapters", false).is_some());
        assert!(deny(Duty::Check, "bind_forms", false).is_some());
        for stage in [DraftStage::Fill, DraftStage::Published] {
            let slot_only = select(stage, false, true, false, false);
            assert_eq!(slot_only, Duty::Template);
            assert!(deny(slot_only, "put_chapters", false).is_some());
            assert!(deny(slot_only, "bind_forms", false).is_some());
            assert!(deny(slot_only, "finish_outline", false).is_some());
            assert!(deny(slot_only, "put_slots", false).is_none());
        }
        assert_eq!(schemas().len(), 6);
        assert_eq!(template_schemas().len(), 2);
    }

    #[test]
    fn prompts_name_only_the_closed_tools() {
        let outline = OUTLINE_PROMPT;
        let fill = TEMPLATE_PROMPT;
        assert!(outline.contains("put_chapters"));
        assert!(outline.contains("bind_forms"));
        assert!(outline.contains("put_slots"));
        assert!(outline.contains("match_query"));
        assert!(outline.contains("留空"));
        assert!(outline.contains("分组章节"));
        assert!(outline.contains("submit_pack"));
        assert!(fill.contains("put_slots"));
        assert!(fill.contains("留空"));
        assert!(fill.contains("不要改章节"));
        for retired in [
            "put_outline_items",
            "submit_outline_scan",
            "submit_outline_check",
            "read_source",
            "put_chapter_template",
            "skip_chapter_content",
        ] {
            assert!(!outline.contains(retired), "{retired}");
            assert!(!fill.contains(retired), "{retired}");
        }
    }
}
