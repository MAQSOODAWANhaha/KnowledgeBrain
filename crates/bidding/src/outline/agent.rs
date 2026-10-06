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
        Duty::Discover | Duty::Organize => DISCOVER,
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
        let template = duty(DraftStage::Fill, Phase::Complete, false);
        assert_eq!(template, Duty::Template);
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
        assert!(deny(mapping, "finish_outline", true).is_some());
        assert!(deny(mapping, "read_form", true).is_none());
        assert!(deny(mapping, "put_outline_items", true).is_none());
    }
}
