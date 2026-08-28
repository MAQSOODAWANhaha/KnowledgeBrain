//! Multilingual regexes and heuristic boundary priorities (brain `patterns.go`).

use regex::Regex;
use std::sync::LazyLock;

use crate::chunker::tokens::{LANG_CHINESE, LANG_ENGLISH, LANG_GERMAN};

pub const PRIO_FORM_FEED: i32 = 100;
pub const PRIO_NUMBERED_HEAD: i32 = 90;
pub const PRIO_CHAPTER_MARKER: i32 = 85;
pub const PRIO_ALL_CAPS_HEADING: i32 = 70;
pub const PRIO_VISUAL_SEP: i32 = 60;
pub const PRIO_PAGE_FOOTER: i32 = 50;
pub const PRIO_BLANK_BLOCK: i32 = 40;

pub static MARKDOWN_HEADING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^(#{1,6})\s+(.+?)\s*#*\s*$").expect("heading"));

pub static NUMBERED_SECTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^[ \t]*(?:[0-9]+(?:\.[0-9]+){1,3}\.?|(?:[0-9]+|[IVX]{1,5})\.)[ \t]+\S.{0,200}$",
    )
    .expect("numbered")
});

pub static ALL_CAPS_HEADING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^[ \t]*([A-ZÄÖÜ][A-ZÄÖÜ \-]{3,80}):?\s*$").expect("caps"));

pub static VISUAL_SEPARATOR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^[ \t]*(?:-{3,}|={3,}|\*{3,}|_{3,})[ \t]*$").expect("sep"));

pub static EXCESSIVE_BLANKS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\n{3,}").expect("blanks"));

pub static PAGE_FOOTER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?mi)^[ \t]*(?:Seite|Page|页码?)\s+[0-9]+(?:\s*(?:von|of|/)\s*[0-9]+)?[ \t]*$")
        .expect("footer")
});

pub static GERMAN_CHAPTER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[ \t]*(?:Kapitel|Abschnitt|Teil)\s+(?:[0-9]+|[IVX]{1,5})[\.: ].{0,200}$")
        .expect("de-chapter")
});

pub static ENGLISH_CHAPTER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[ \t]*(?:Chapter|Section|Part)\s+(?:[0-9]+|[IVX]{1,5})[\.: ].{0,200}$")
        .expect("en-chapter")
});

pub static CHINESE_CHAPTER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[ \t]*第[ \t]*[一二三四五六七八九十百千零〇0-9]+[ \t]*(?:章|节|節|部分|篇)[ \t]?.{0,200}$")
        .expect("zh-chapter")
});

pub fn chapter_patterns_for_langs(langs: &[String]) -> Vec<&'static Regex> {
    if langs.is_empty() {
        return vec![&GERMAN_CHAPTER, &ENGLISH_CHAPTER, &CHINESE_CHAPTER];
    }
    let mut out = Vec::new();
    for l in langs {
        match l.as_str() {
            LANG_GERMAN => out.push(&*GERMAN_CHAPTER),
            LANG_ENGLISH => out.push(&*ENGLISH_CHAPTER),
            LANG_CHINESE => out.push(&*CHINESE_CHAPTER),
            _ => {}
        }
    }
    if out.is_empty() {
        out = vec![&GERMAN_CHAPTER, &ENGLISH_CHAPTER, &CHINESE_CHAPTER];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_heading_levels() {
        assert!(MARKDOWN_HEADING.is_match("# Title"));
        assert!(MARKDOWN_HEADING.is_match("## Sub"));
        assert!(MARKDOWN_HEADING.is_match("###### Deep"));
        assert!(!MARKDOWN_HEADING.is_match("####### Too deep"));
        assert!(!MARKDOWN_HEADING.is_match("#NoSpace"));
        let caps = MARKDOWN_HEADING.captures("### Hello world").unwrap();
        assert_eq!(&caps[1], "###");
        assert_eq!(&caps[2], "Hello world");
    }

    #[test]
    fn numbered_and_chapter_markers() {
        assert!(NUMBERED_SECTION.is_match("1. Intro"));
        assert!(NUMBERED_SECTION.is_match("2.3 Methods"));
        assert!(NUMBERED_SECTION.is_match("IV. Results"));
        assert!(NUMBERED_SECTION.is_match("2.2.1 用户与权限"));
        assert!(GERMAN_CHAPTER.is_match("Kapitel 1: Einleitung"));
        assert!(ENGLISH_CHAPTER.is_match("Chapter 2. Methods"));
        assert!(CHINESE_CHAPTER.is_match("第一章 引言"));
        assert!(CHINESE_CHAPTER.is_match("第 3 节 方法"));
        assert!(VISUAL_SEPARATOR.is_match("---"));
        assert!(VISUAL_SEPARATOR.is_match("***"));
        assert!(PAGE_FOOTER.is_match("Page 3 of 10"));
        assert!(PAGE_FOOTER.is_match("Seite 2 von 8"));
        assert!(ALL_CAPS_HEADING.is_match("INTRODUCTION"));
        assert!(!ALL_CAPS_HEADING.is_match("Not a heading"));
    }
}
