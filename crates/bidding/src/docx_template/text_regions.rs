use super::*;

/// No generated wording: all retained text is selected from one frozen source.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TextRegion {
    pub start: usize,
    pub end: usize,
    pub blank: bool,
}

pub(crate) fn region_bookmark_name(section: usize, block: usize, region: usize) -> String {
    format!("{}_r{region}", bookmark_name(section, Some(block)))
}

pub(crate) fn resolve_text_regions(
    input: &Value,
    block: &TemplateBlock,
) -> Result<Vec<String>, TemplateError> {
    let source = block
        .source_id
        .as_ref()
        .and_then(|id| {
            input["source_units"]
                .as_array()?
                .iter()
                .find(|s| s["source_unit_revision_id"] == *id)?["text"]
                .as_str()
        })
        .ok_or_else(|| invalid("text region source missing"))?;
    check(!block.text_regions.is_empty(), "text regions missing")?;
    let mut end = None;
    let mut prior_cr = false;
    let mut out = vec![];
    for region in &block.text_regions {
        check(
            end.is_none_or(|end| end == region.start),
            "text regions must be contiguous",
        )?;
        let raw = source
            .get(region.start..region.end)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| invalid("text region range invalid"))?;
        end = Some(region.end);
        // Normalize the shared stream, including a CRLF split across regions.
        let mut fragment = String::new();
        for ch in raw.chars() {
            if ch != '\n' || !prior_cr {
                fragment.push(if ch == '\r' { '\n' } else { ch });
            }
            prior_cr = ch == '\r';
        }
        if region.blank {
            fragment = fragment
                .split('\n')
                .map(|line| {
                    if line.chars().all(char::is_whitespace) {
                        line
                    } else {
                        " "
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
        }
        out.push(fragment);
    }
    Ok(out)
}
