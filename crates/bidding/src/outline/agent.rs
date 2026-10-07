//! Outline duties. A turn has one duty and the model sees only that duty's tools.
//!
//! Discover submits reading packs. Organize writes the chapter tree and binds
//! every attachment table to one chapter. Template writes prescribed slots.
//! Check reads the draft and finishes it.

use crate::tender_analysis::draft::DraftStage;
use crate::tender_analysis::outline_flow::Phase;

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
                "把要求组织成稳定章节，并把每个附件表绑定到唯一章节。不写模板正文，不匹配知识库"
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
                "本轮只做组章。用已保存的要求整理章节树，并把每个附件表绑定到唯一章节。不要重新扫描招标文件，不要写模板正文，不要匹配知识库。"
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
/// Organize then writes chapters and binds every attachment table. Template
/// slots come after that, then finish.
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
    if !chapters_ready || unmapped_attachments {
        return Duty::Organize;
    }
    if !slots_ready {
        return Duty::Template;
    }
    Duty::Check
}

pub fn current(
    input: &crate::tender_analysis::FrozenInput,
    state: &crate::tender_analysis::agent::Checkpoint,
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
        state.outline_run.tool_draft.slots_submitted,
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
    crate::tender_analysis::draft::outline_schemas()
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
        Duty::Discover | Duty::Organize => {
            "discover and organize cannot write template content or knowledge responses"
        }
        Duty::Check => "check duty cannot rescan, reorganize, or write template content",
        Duty::Template => "template duty cannot change chapters or rescan the tender",
    }
}

const DISCOVER: &[&str] = &["submit_pack", "read_outline"];
const ORGANIZE: &[&str] = &["put_chapters", "bind_forms", "read_outline"];
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
        let template = duty(DraftStage::Fill, Phase::Complete, false);
        assert_eq!(template, Duty::Template);
        assert!(deny(template, "submit_pack", false).is_some());
        assert!(deny(template, "put_chapters", false).is_some());
        assert!(deny(template, "read_source", false).is_some());
        assert!(deny(template, "put_slots", false).is_none());
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
            Duty::Template
        );
        assert_eq!(
            select(DraftStage::Outline, false, true, false, true),
            Duty::Check
        );
        assert_eq!(
            select(DraftStage::Fill, true, false, true, false),
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
                "read_outline".to_string()
            ]
        );
        assert_eq!(names(Duty::Template).len(), 2);
        assert_eq!(crate::tender_analysis::draft::outline_schemas().len(), 6);
    }

    #[test]
    fn prompts_name_only_the_closed_tools() {
        let outline = include_str!("../../prompts/tender-draft-outline-v1.txt");
        let fill = include_str!("../../prompts/tender-draft-fill-v1.txt");
        assert!(outline.contains("put_chapters"));
        assert!(outline.contains("bind_forms"));
        assert!(outline.contains("submit_pack"));
        assert!(fill.contains("put_slots"));
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
