//! Output language for summary / questions / captions.
//! Never default to English or Chinese in call sites: env override, then
//! version `chunk_languages`, then detect from the document text.

use crate::Store;
use uuid::Uuid;

pub fn env_content_language() -> Option<String> {
    let raw = std::env::var("KNOWLEDGEBRAIN_CONTENT_LANGUAGE").ok()?;
    let t = raw.trim();
    if t.is_empty() {
        None
    } else {
        Some(normalize_language_tag(t))
    }
}

pub fn normalize_language_tag(tag: &str) -> String {
    match tag.trim().to_ascii_lowercase().as_str() {
        "zh" | "zh-cn" | "zh_cn" | "zh-hans" | "chs" | "chinese" | "中文" => "Chinese".into(),
        "en" | "en-us" | "en_us" | "eng" | "english" => "English".into(),
        "" => "the same language as the source content".into(),
        _ => tag.trim().to_string(),
    }
}

/// Han vs Latin letters. Empty sample → follow the source, not a baked-in locale.
pub fn infer_output_language(sample: &str) -> String {
    if let Some(forced) = env_content_language() {
        return forced;
    }
    let mut han = 0u32;
    let mut latin = 0u32;
    for c in sample.chars() {
        match c {
            '\u{4e00}'..='\u{9fff}' => han += 1,
            'A'..='Z' | 'a'..='z' => latin += 1,
            _ => {}
        }
    }
    if han == 0 && latin == 0 {
        return "the same language as the source content".into();
    }
    if han >= latin {
        "Chinese".into()
    } else {
        "English".into()
    }
}

pub fn language_for_document(store: &Store, document_id: Uuid) -> String {
    if let Some(forced) = env_content_language() {
        return forced;
    }
    if let Some(version) = store.effective_version(document_id)
        && let Some(tag) = version
            .chunk_languages
            .iter()
            .map(|s| s.trim())
            .find(|s| !s.is_empty())
    {
        return normalize_language_tag(tag);
    }
    let mut sample = String::new();
    if let Some(doc) = store.documents.get(&document_id) {
        sample.push_str(&doc.title);
        sample.push('\n');
        sample.push_str(&doc.file_name);
        sample.push('\n');
        sample.push_str(&doc.markdown);
    }
    if sample
        .chars()
        .filter(|c| ('\u{4e00}'..='\u{9fff}').contains(c))
        .count()
        < 12
    {
        for chunk in store.chunks.values().filter(|c| {
            c.document_id == document_id && matches!(c.chunk_type.as_str(), "text" | "image_ocr")
        }) {
            sample.push_str(&chunk.content);
            sample.push('\n');
            if sample.len() > 6000 {
                break;
            }
        }
    }
    infer_output_language(&sample)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Document, ProductVersion, Store};

    #[test]
    fn detects_chinese_from_han() {
        assert_eq!(
            infer_output_language("云安全管理平台支持租户订单审批与组件升级。"),
            "Chinese"
        );
    }

    #[test]
    fn detects_english_from_latin() {
        assert_eq!(
            infer_output_language("The cloud security platform supports tenant billing."),
            "English"
        );
    }

    #[test]
    fn empty_follows_source() {
        assert_eq!(
            infer_output_language(""),
            "the same language as the source content"
        );
    }

    #[test]
    fn filename_chinese_counts() {
        assert_eq!(
            infer_output_language("云安全管理平台V2.0白皮书.docx"),
            "Chinese"
        );
    }

    #[test]
    fn version_chunk_languages_win() {
        let mut store = Store::default();
        let mut version = ProductVersion::new(Uuid::new_v4(), "v1".into());
        version.chunk_languages = vec!["en".into()];
        let vid = version.id;
        let doc = Document::new(
            vid,
            "白皮书".into(),
            "白皮书.docx".into(),
            1,
            "h".into(),
            "k".into(),
        );
        let did = doc.id;
        store.versions.insert(vid, version);
        store.documents.insert(did, doc);
        assert_eq!(language_for_document(&store, did), "English");
    }

    #[test]
    fn normalize_tags() {
        assert_eq!(normalize_language_tag("zh-CN"), "Chinese");
        assert_eq!(normalize_language_tag("en"), "English");
        assert_eq!(normalize_language_tag("日本語"), "日本語");
    }
}
