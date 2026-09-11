use serde_json::Value;

pub fn cases() -> Vec<(&'static str, Option<String>)> {
    vec![
        ("chinese-regression", Some("中".repeat(3000))),
        ("ascii-before", Some("a".repeat(8191))),
        ("ascii-at", Some("a".repeat(8192))),
        ("ascii-after", Some("a".repeat(8193))),
        ("chinese-before", Some("中".repeat(2730))),
        ("chinese-at", Some(format!("{}ab", "中".repeat(2730)))),
        ("chinese-after", Some("中".repeat(2731))),
        ("emoji-before", Some("😀".repeat(2047))),
        ("emoji-at", Some("😀".repeat(2048))),
        ("emoji-after", Some("😀".repeat(2049))),
        ("mixed-before", Some(format!("{}中😀", "a".repeat(8184)))),
        ("mixed-at", Some(format!("{}中😀", "a".repeat(8185)))),
        ("mixed-after", Some(format!("{}中😀", "a".repeat(8186)))),
        ("short", Some("诊断 😀 e\u{301}".into())),
        ("empty", Some(String::new())),
        ("null", None),
    ]
}

// Independent scalar-boundary oracle; PostgreSQL must enforce the byte limit itself.
pub fn prefix(message: &str) -> &str {
    let mut end = message.len().min(8192);
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    &message[..end]
}

pub fn assert_message(actual: &Value, message: Option<&str>) {
    match message {
        Some(message) => {
            let actual = actual.as_str().expect("text diagnostic");
            assert_eq!(actual, prefix(message));
            assert!(actual.len() <= 8192);
            assert!(message.starts_with(actual));
        }
        None => assert!(actual.is_null()),
    }
}
