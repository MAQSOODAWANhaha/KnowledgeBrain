//! Language-aware token approximation (brain `tokens.go`).

use std::collections::HashMap;
use std::sync::LazyLock;

pub const LANG_ENGLISH: &str = "en";
pub const LANG_GERMAN: &str = "de";
pub const LANG_CHINESE: &str = "zh";
pub const LANG_MIXED: &str = "mixed";

static CHARS_PER_TOKEN: LazyLock<HashMap<&'static str, f64>> = LazyLock::new(|| {
    HashMap::from([
        (LANG_ENGLISH, 4.0),
        (LANG_GERMAN, 4.5),
        (LANG_CHINESE, 1.7),
        (LANG_MIXED, 3.0),
    ])
});

pub fn chars_for_token_limit(tokens: usize, lang: &str) -> usize {
    if tokens == 0 {
        return 0;
    }
    let ratio = CHARS_PER_TOKEN
        .get(lang)
        .copied()
        .unwrap_or(CHARS_PER_TOKEN[LANG_MIXED]);
    (tokens as f64 * ratio * 0.9) as usize
}

pub fn detect_language(s: &str) -> &'static str {
    if s.is_empty() {
        return LANG_MIXED;
    }
    let mut cjk = 0usize;
    let mut latin = 0usize;
    let mut umlaut = 0usize;
    for r in s.chars() {
        if is_cjk(r) {
            cjk += 1;
        } else if is_german_umlaut(r) {
            umlaut += 1;
            latin += 1;
        } else if r.is_ascii_alphabetic() {
            latin += 1;
        }
    }
    let total = cjk + latin;
    if total == 0 {
        return LANG_MIXED;
    }
    let cjk_ratio = cjk as f64 / total as f64;
    let latin_ratio = latin as f64 / total as f64;
    if cjk_ratio >= 0.15 && latin_ratio >= 0.15 {
        return LANG_MIXED;
    }
    if cjk_ratio > 0.3 {
        return LANG_CHINESE;
    }
    if umlaut > 0 || has_german_words(s) {
        return LANG_GERMAN;
    }
    LANG_ENGLISH
}

fn is_cjk(r: char) -> bool {
    ('\u{4E00}'..='\u{9FFF}').contains(&r)
        || ('\u{3400}'..='\u{4DBF}').contains(&r)
        || ('\u{AC00}'..='\u{D7AF}').contains(&r)
        || ('\u{3040}'..='\u{309F}').contains(&r)
        || ('\u{30A0}'..='\u{30FF}').contains(&r)
}

fn is_german_umlaut(r: char) -> bool {
    matches!(r, 'ä' | 'ö' | 'ü' | 'Ä' | 'Ö' | 'Ü' | 'ß')
}

fn has_german_words(s: &str) -> bool {
    let sample = byte_prefix(s, 512);
    for w in [
        " der ", " die ", " das ", " und ", " ist ", " nicht ", " mit ", " auf ",
    ] {
        if contains_lower(sample, w) {
            return true;
        }
    }
    false
}

pub(crate) fn byte_prefix(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn contains_lower(haystack: &str, needle: &str) -> bool {
    if haystack.len() < needle.len() {
        return false;
    }
    let hbytes = haystack.as_bytes();
    let nbytes = needle.as_bytes();
    for i in 0..=hbytes.len().saturating_sub(nbytes.len()) {
        let mut ok = true;
        for j in 0..nbytes.len() {
            let mut h = hbytes[i + j];
            if h.is_ascii_uppercase() {
                h += b'a' - b'A';
            }
            if h != nbytes[j] {
                ok = false;
                break;
            }
        }
        if ok {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_scripts() {
        assert_eq!(detect_language("Hello world this is English"), LANG_ENGLISH);
        assert_eq!(detect_language("这是一段中文内容用于检测"), LANG_CHINESE);
        assert_eq!(
            detect_language("Das ist nicht mit auf der die"),
            LANG_GERMAN
        );
    }

    #[test]
    fn token_budget_is_conservative() {
        assert_eq!(chars_for_token_limit(100, LANG_ENGLISH), 360);
        assert_eq!(chars_for_token_limit(0, LANG_ENGLISH), 0);
    }
}
