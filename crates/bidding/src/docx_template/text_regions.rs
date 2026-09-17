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

/// Initial text for the existing quote/blank and contiguous-region primitives.
/// Removed ranges address the original UTF-8 bytes, before newline normalization.
pub(crate) fn initial_text_fragment(
    raw: &str,
    blank: bool,
    inline: bool,
    prior_cr: &mut bool,
) -> (String, Vec<std::ops::Range<usize>>) {
    if blank && !inline {
        return (
            String::new(),
            (!raw.is_empty())
                .then_some(0..raw.len())
                .into_iter()
                .collect(),
        );
    }
    let mut fragment = String::new();
    for ch in raw.chars() {
        if ch != '\n' || !*prior_cr {
            fragment.push(if ch == '\r' { '\n' } else { ch });
        }
        *prior_cr = ch == '\r';
    }
    let mut removed = Vec::new();
    if blank {
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
        let mut start = 0;
        for line in raw.split_inclusive(['\r', '\n']) {
            let text = line.trim_end_matches(['\r', '\n']);
            if !text.chars().all(char::is_whitespace) {
                removed.push(start..start + text.len());
            }
            start += line.len();
        }
    }
    (fragment, removed)
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
        let (fragment, _) = initial_text_fragment(raw, region.blank, true, &mut prior_cr);
        out.push(fragment);
    }
    Ok(out)
}
