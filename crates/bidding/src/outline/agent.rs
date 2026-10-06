//! Phase-1 agent duties. A turn has one duty and only that duty's tools.
//!
//! Discover reads the tender and records requirements. Organize builds the
//! chapter tree. MapAttachments binds attachment tables before the outline can
//! finish. Check only reviews the current packet. Template writes prescribed
//! content and cannot change chapters.

use crate::tender_analysis::draft::DraftStage;
use crate::tender_analysis::outline_flow::Phase;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Duty {
    Discover,
    Organize,
    MapAttachments,
    Check,
    Template,
}

impl Duty {
    pub fn responsibility(self) -> &'static str {
        match self {
            Self::Discover => "读取招标文件并提交已检查范围和要求，不写模板正文",
            Self::Organize => "把要求组织成稳定章节，不写模板正文，不匹配知识库",
            Self::MapAttachments => "把附件表映射到唯一章节后再结束大纲",
            Self::Check => "只核对接对包，不重新扫描，不改章节",
            Self::Template => "只写规定模板内容，不改章节，不填写我方事实",
        }
    }

    /// Turn instruction placed in front of the shared outline contract.
    pub fn instructions(self) -> &'static str {
        match self {
            Self::Discover => {
                "本轮只做发现。只阅读已领取的 reading pack，用 submit_pack_scan 提交该包范围内的要求。校验失败时用 repair_pack_scan 按反馈改正后重交。不要写章节，不要写模板，不要匹配知识库。"
            }
            Self::Organize => {
                "本轮只做组章。用已保存的要求整理章节树。不要重新扫描招标文件，不要写模板正文，不要匹配知识库。"
            }
            Self::MapAttachments => {
                "本轮只做附件表映射。每个附件表必须落到唯一章节后才能结束大纲。不要写模板正文，不要匹配知识库。"
            }
            Self::Check => {
                "本轮只做核对。只阅读当前核对包并提交核对结论。不要重新扫描，不要改章节，不要写模板。"
            }
            Self::Template => {
                "本轮只写规定模板。只填写招标文件已经给出的文字，投标人事实留空。不要改章节，不要重新扫描。"
            }
        }
    }
}

pub fn duty(stage: DraftStage, phase: Phase, unmapped_attachments: bool) -> Duty {
    match stage {
        DraftStage::Fill | DraftStage::Published => Duty::Template,
        DraftStage::None | DraftStage::Outline => match phase {
            Phase::Check | Phase::Complete => Duty::Check,
            Phase::Outline if unmapped_attachments => Duty::MapAttachments,
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
        Duty::MapAttachments => MAP_ATTACHMENTS,
        Duty::Check => CHECK,
        Duty::Template => TEMPLATE,
    };
    if DRAFT_TOOLS.contains(&tool) && !allowed.contains(&tool) {
        Some(duty_denial(duty))
    } else {
        None
    }
}

fn duty_denial(duty: Duty) -> &'static str {
    match duty {
        Duty::Discover | Duty::Organize => {
            "discover and organize cannot write template content or knowledge responses"
        }
        Duty::MapAttachments => "map attachment tables before finishing the outline",
        Duty::Check => "check duty cannot rescan, reorganize, or write template content",
        Duty::Template => "template duty cannot change chapters or rescan the tender",
    }
}

const DRAFT_TOOLS: &[&str] = &[
    "collection_index",
    "source_index",
    "search_sources",
    "read_source",
    "read_form",
    "read_form_cell",
    "read_source_view",
    "submit_outline_scan",
    "read_outline",
    "read_outline_fragment",
    "assign_outline_fragments",
    "put_outline_item",
    "put_outline_items",
    "finish_outline",
    "submit_outline_check",
    "omit_outline_item",
    "put_chapter_template",
    "skip_chapter_content",
    "submit_pack_scan",
    "repair_pack_scan",
];

const DISCOVER: &[&str] = &[
    "collection_index",
    "source_index",
    "search_sources",
    "read_source",
    "read_form",
    "read_form_cell",
    "read_source_view",
    "submit_outline_scan",
    "read_outline",
    "read_outline_fragment",
    "assign_outline_fragments",
    "put_outline_item",
    "put_outline_items",
    "finish_outline",
    "omit_outline_item",
    "submit_pack_scan",
    "repair_pack_scan",
];

const ORGANIZE: &[&str] = &[
    "collection_index",
    "source_index",
    "search_sources",
    "read_source",
    "read_form",
    "read_form_cell",
    "read_source_view",
    "submit_outline_scan",
    "read_outline",
    "read_outline_fragment",
    "assign_outline_fragments",
    "put_outline_item",
    "put_outline_items",
    "finish_outline",
    "omit_outline_item",
];

const MAP_ATTACHMENTS: &[&str] = &[
    "collection_index",
    "source_index",
    "search_sources",
    "read_source",
    "read_form",
    "read_form_cell",
    "read_source_view",
    "submit_outline_scan",
    "read_outline",
    "read_outline_fragment",
    "assign_outline_fragments",
    "put_outline_item",
    "put_outline_items",
    "omit_outline_item",
];

const CHECK: &[&str] = &[
    "read_outline",
    "read_outline_fragment",
    "submit_outline_check",
];

const TEMPLATE: &[&str] = &[
    "collection_index",
    "source_index",
    "search_sources",
    "read_source",
    "read_form",
    "read_form_cell",
    "read_source_view",
    "read_outline",
    "read_outline_fragment",
    "put_chapter_template",
    "skip_chapter_content",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duties_keep_template_writing_away_from_discovery() {
        let discover = duty(DraftStage::Outline, Phase::Discover, false);
        assert!(deny(discover, "put_chapter_template", false).is_some());
        assert!(deny(discover, "put_outline_items", false).is_none());
        assert!(deny(discover, "submit_pack_scan", false).is_none());
        let organize = duty(DraftStage::Outline, Phase::Outline, false);
        assert!(deny(organize, "submit_pack_scan", false).is_some());
        let template = duty(DraftStage::Fill, Phase::Complete, false);
        assert_eq!(template, Duty::Template);
        assert!(deny(template, "submit_pack_scan", false).is_some());
        assert!(deny(template, "put_outline_items", false).is_some());
        assert!(deny(template, "read_source", false).is_none());
        assert!(deny(template, "put_chapter_template", false).is_none());
    }

    #[test]
    fn unmapped_attachment_blocks_finish_and_selects_mapping_duty() {
        let mapping = duty(DraftStage::Outline, Phase::Outline, true);
        assert_eq!(mapping, Duty::MapAttachments);
        assert_eq!(
            mapping.responsibility(),
            "把附件表映射到唯一章节后再结束大纲"
        );
        assert!(mapping.instructions().contains("附件表"));
        assert!(Duty::Discover.instructions().contains("submit_pack_scan"));
        assert!(Duty::Discover.instructions().contains("repair_pack_scan"));
        assert!(!Duty::Organize.instructions().contains("submit_pack_scan"));
        assert!(Duty::Template.instructions().contains("留空"));
        assert!(deny(mapping, "finish_outline", true).is_some());
        assert!(deny(mapping, "read_form", true).is_none());
        assert!(deny(mapping, "put_outline_items", true).is_none());
    }
}
