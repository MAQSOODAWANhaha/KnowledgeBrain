//! Phase-2 agent. It may only turn knowledge hits into response data.

pub const RESPONSIBILITY: &str =
    "只按阶段一的响应槽位匹配知识库并生成响应；不重新解析招标文件，不新增章节，不改模板";

const CLOSED: &[&str] = &[
    "submit_outline_scan",
    "put_outline_items",
    "put_outline_item",
    "finish_outline",
    "put_chapter_template",
    "skip_chapter_content",
    "read_source",
    "read_form",
    "search_sources",
];

pub fn deny(tool: &str) -> Option<&'static str> {
    if CLOSED.contains(&tool) {
        Some("phase 2 cannot parse the tender, edit chapters, or rewrite template content")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_duty_rejects_tender_tools() {
        assert!(deny("read_source").is_some());
        assert!(deny("put_outline_items").is_some());
        assert!(deny("put_chapter_template").is_some());
        assert!(deny("search_knowledge").is_none());
    }
}
