//! In-process convert for txt/md/json/csv and audio/image passthrough.

use crate::{ImageRef, ReadResult};
use platform::{is_audio_type, is_image_type};

pub fn convert_simple(file_name: &str, bytes: &[u8]) -> ReadResult {
    if is_audio_type(file_name) {
        let name = file_name.rsplit('/').next().unwrap_or(file_name);
        return ReadResult {
            markdown: format!("[Audio file: {name}]"),
            error: String::new(),
            images: Vec::new(),
            structured_source_units: Vec::new(),
            is_audio: true,
            audio_data: bytes.to_vec(),
            metadata: std::collections::HashMap::new(),
        };
    }
    if is_image_type(file_name) {
        let name = file_name.rsplit('/').next().unwrap_or(file_name);
        let original = format!("images/{name}");
        return ReadResult {
            markdown: format!("![{name}]({original})"),
            images: vec![ImageRef {
                filename: name.to_string(),
                original_ref: original,
                mime_type: String::new(),
                storage_key: String::new(),
                data: bytes.to_vec(),
            }],
            ..ReadResult::default()
        };
    }
    let ext = file_name
        .rsplit('.')
        .next()
        .unwrap_or("txt")
        .to_ascii_lowercase();
    if matches!(ext.as_str(), "md" | "markdown" | "txt" | "text") {
        match String::from_utf8(bytes.to_vec()) {
            Ok(s) => ReadResult {
                markdown: s,
                ..ReadResult::default()
            },
            Err(_) => ReadResult {
                error: "invalid utf-8".into(),
                ..ReadResult::default()
            },
        }
    } else if ext == "json" {
        ReadResult {
            markdown: json_to_md(bytes),
            ..ReadResult::default()
        }
    } else if ext == "csv" {
        ReadResult {
            markdown: csv_to_md(bytes),
            ..ReadResult::default()
        }
    } else {
        ReadResult {
            error: format!("simple reader cannot parse .{ext}"),
            ..ReadResult::default()
        }
    }
}

fn json_to_md(bytes: &[u8]) -> String {
    match serde_json::from_slice::<serde_json::Value>(bytes) {
        Ok(v) => json_value_to_md(&v, 0),
        Err(_) => String::from_utf8_lossy(bytes).into_owned(),
    }
}

fn json_value_to_md(v: &serde_json::Value, depth: usize) -> String {
    match v {
        serde_json::Value::Object(map) => {
            let heading = "#".repeat((depth + 1).min(6));
            let mut out = String::new();
            for (k, val) in map {
                match val {
                    serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
                        out.push_str(&format!("{heading} {k}\n\n"));
                        out.push_str(&json_value_to_md(val, depth + 1));
                    }
                    _ => out.push_str(&format!("- **{k}**: {}\n", json_scalar(val))),
                }
            }
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out
        }
        serde_json::Value::Array(arr) => {
            let mut out = String::new();
            for (i, item) in arr.iter().enumerate() {
                match item {
                    serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
                        out.push_str(&format!("{}.\n\n", i + 1));
                        out.push_str(&json_value_to_md(item, depth + 1));
                    }
                    _ => out.push_str(&format!("{}. {}\n", i + 1, json_scalar(item))),
                }
            }
            out
        }
        other => format!("{}\n", json_scalar(other)),
    }
}

fn json_scalar(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn csv_to_md(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut lines = text.lines();
    let Some(header) = lines.next() else {
        return String::new();
    };
    let cols = csv_split_line(header);
    let mut out = format!(
        "| {} |\n|{}|\n",
        cols.join(" | "),
        vec!["---"; cols.len()].join("|")
    );
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        out.push_str(&format!("| {} |\n", csv_split_line(line).join(" | ")));
    }
    out
}

fn csv_split_line(line: &str) -> Vec<String> {
    let mut cols = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '"' {
            if in_quotes && chars.peek() == Some(&'"') {
                chars.next();
                cur.push('"');
            } else {
                in_quotes = !in_quotes;
            }
        } else if c == ',' && !in_quotes {
            cols.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    cols.push(cur);
    cols
}

#[cfg(test)]
mod tests {
    use super::convert_simple;

    #[test]
    fn json_expands_object() {
        let r = convert_simple("a.json", br#"{"name":"sw","ports":[1,2]}"#);
        assert!(r.error.is_empty());
        assert!(r.markdown.contains("**name**: sw"), "{}", r.markdown);
        assert!(r.markdown.contains("ports"), "{}", r.markdown);
    }

    #[test]
    fn csv_keeps_quoted_comma() {
        let r = convert_simple("a.csv", b"a,b\n\"x,y\",z\n");
        assert!(r.markdown.contains("| x,y | z |"), "{}", r.markdown);
    }

    #[test]
    fn txt_convert() {
        let r = convert_simple("a.txt", b"hello world");
        assert!(r.error.is_empty());
        assert_eq!(r.markdown, "hello world");
    }

    #[test]
    fn audio_simple_keeps_bytes() {
        let r = convert_simple("talk.wav", b"RIFF");
        assert!(r.is_audio);
        assert_eq!(r.audio_data, b"RIFF");
        assert!(r.markdown.contains("talk.wav"));
    }

    #[test]
    fn image_simple_keeps_bytes() {
        let r = convert_simple("pic.png", b"\x89PNG");
        assert!(r.markdown.contains("images/pic.png"));
        assert_eq!(r.images.len(), 1);
        assert_eq!(r.images[0].data, b"\x89PNG");
    }
}
