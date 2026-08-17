//! Brain `sanitizeOCRText`.

use regex::Regex;
use std::sync::LazyLock;

static HTML_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").expect("tag"));
static CODE_FENCE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)^\s*```[a-zA-Z]*\s*\n(.*?)\n\s*```\s*$").expect("fence"));
static HTML_DOC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\s*(<!DOCTYPE|<html|<body|<div|<p[\s>]|<table|<h[1-6][\s>])").expect("html")
});
static MULTI_NL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n{3,}").expect("nl"));

const EMPTY_REPLIES: &[&str] = &[
    "无文字内容",
    "无法识别",
    "no text",
    "no text content",
    "no content",
    "empty",
    "图片中没有文字",
    "图片中没有可识别的文字",
];

pub fn sanitize_ocr_text(raw: &str) -> String {
    let mut text = raw.trim().to_string();
    if text.is_empty() {
        return String::new();
    }
    if let Some(caps) = CODE_FENCE.captures(&text) {
        text = caps[1].trim().to_string();
    }
    let plain = HTML_TAG.replace_all(&text, "").trim().to_string();
    if plain.chars().count() < 10 && HTML_TAG.is_match(&text) {
        return String::new();
    }
    if looks_like_html(&text) {
        text = plain;
        if text.is_empty() {
            return String::new();
        }
    }
    if is_known_empty(&text) {
        return String::new();
    }
    MULTI_NL.replace_all(text.trim(), "\n\n").into_owned()
}

fn looks_like_html(text: &str) -> bool {
    if HTML_DOC.is_match(text) {
        return true;
    }
    let tags: Vec<_> = HTML_TAG.find_iter(text).collect();
    if tags.is_empty() {
        return false;
    }
    let tag_chars: usize = tags.iter().map(|m| m.as_str().len()).sum();
    tag_chars as f64 / text.len() as f64 > 0.3
}

fn is_known_empty(text: &str) -> bool {
    let mut lower = text.trim().to_ascii_lowercase();
    while lower.ends_with(['.', '!', '?', '。', '！', '？']) {
        lower.pop();
    }
    EMPTY_REPLIES
        .iter()
        .any(|p| lower == p.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_known_empty() {
        assert!(sanitize_ocr_text("No text content.").is_empty());
        assert!(sanitize_ocr_text("无文字内容").is_empty());
    }

    #[test]
    fn unwraps_code_fence() {
        let got = sanitize_ocr_text("```markdown\nHello table\n```");
        assert_eq!(got, "Hello table");
    }

    #[test]
    fn strips_html_wrapper() {
        assert!(
            sanitize_ocr_text("<html><body><div class=\"x\"><img/></div></body></html>").is_empty()
        );
    }
}
