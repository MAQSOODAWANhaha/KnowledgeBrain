//! Submission part-key and render-reference contracts.

use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateFormat {
    Docx,
    Pdf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TechnicalRouteRef {
    pub unit_id: Option<Uuid>,
}

pub fn required_part_keys(technical_routes: &[TechnicalRouteRef]) -> Vec<String> {
    let mut keys = vec!["1".to_string()];
    let mut has_unsectioned = false;
    let mut units: Vec<Uuid> = Vec::new();
    for route in technical_routes {
        match route.unit_id {
            Some(unit) if unit.is_nil() => has_unsectioned = true,
            Some(unit) => units.push(unit),
            None => {}
        }
    }
    units.sort_unstable();
    units.dedup();
    for unit in units {
        keys.push(format!("2:{unit}"));
    }
    if has_unsectioned {
        keys.push("2:unsectioned".into());
    }
    keys.extend([
        "3".into(),
        "4".into(),
        "5".into(),
        "6:letter".into(),
        "6:authorization".into(),
        "6:quote".into(),
        "6:implementation_plan".into(),
        "6:procedural".into(),
    ]);
    keys
}

pub fn template_slot_for_part_key(part_key: &str) -> Option<&'static str> {
    if part_key == "2:unsectioned" {
        return Some("2:unsectioned");
    }
    if let Some(rest) = part_key.strip_prefix("2:") {
        return Uuid::parse_str(rest)
            .ok()
            .filter(|id| !id.is_nil() && id.as_hyphenated().to_string() == rest)
            .map(|_| "2:unit");
    }
    match part_key {
        "1"
        | "3"
        | "4"
        | "5"
        | "6:letter"
        | "6:authorization"
        | "6:quote"
        | "6:implementation_plan"
        | "6:procedural" => Some(match part_key {
            "1" => "1",
            "3" => "3",
            "4" => "4",
            "5" => "5",
            "6:letter" => "6:letter",
            "6:authorization" => "6:authorization",
            "6:quote" => "6:quote",
            "6:implementation_plan" => "6:implementation_plan",
            "6:procedural" => "6:procedural",
            _ => unreachable!(),
        }),
        _ => None,
    }
}

pub fn parse_markdown_object_occurrences(markdown: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let bytes = markdown.as_bytes();
    let mut i = 0;
    while i + 8 + 64 <= bytes.len() {
        if bytes[i..].starts_with(b"objects/") {
            let hex = &bytes[i + 8..i + 8 + 64];
            if hex
                .iter()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            {
                out.push((
                    out.len(),
                    format!("objects/{}", std::str::from_utf8(hex).expect("hex")),
                ));
                i += 8 + 64;
                continue;
            }
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_part_set_includes_units_unsectioned_and_group_six() {
        let ordinary = Uuid::from_u128(0xaaaa_aaaa_aaaa_aaaa_aaaa_aaaa_aaaa_aaaa);
        let keys = required_part_keys(&[
            TechnicalRouteRef {
                unit_id: Some(ordinary),
            },
            TechnicalRouteRef {
                unit_id: Some(Uuid::nil()),
            },
        ]);
        assert_eq!(
            keys,
            vec![
                "1".to_string(),
                format!("2:{ordinary}"),
                "2:unsectioned".into(),
                "3".into(),
                "4".into(),
                "5".into(),
                "6:letter".into(),
                "6:authorization".into(),
                "6:quote".into(),
                "6:implementation_plan".into(),
                "6:procedural".into(),
            ]
        );
        assert_eq!(
            template_slot_for_part_key(&format!("2:{ordinary}")),
            Some("2:unit")
        );
        assert_eq!(
            template_slot_for_part_key("2:unsectioned"),
            Some("2:unsectioned")
        );
        assert!(template_slot_for_part_key("2:00000000-0000-0000-0000-000000000000").is_none());
        assert!(template_slot_for_part_key("2:AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA").is_none());
        assert!(template_slot_for_part_key("2:not-a-uuid").is_none());
    }

    #[test]
    fn markdown_object_parser_is_fixed_and_lowercase_only() {
        let md = "见图 ![](objects/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa) 以及 objects/BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB";
        let found = parse_markdown_object_occurrences(md);
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].1,
            "objects/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
    }
}
