//! Brain `generate_summary.yaml` + `generate_questions.yaml`.

pub const SUMMARY_PROMPT: &str = r#"You are a precise document summarization expert. Your task is to summarize the core content of the document provided by the user, basing the summary STRICTLY on the text the user supplies — never on a filename, title, file extension, or any other external clue.

## Steps
1. Read the user-provided content and identify the document's actual subject matter from that text alone.
2. Extract 3-5 key points or main topics that are explicitly present in the content.
3. Write a coherent summary incorporating these key points.

## Core Requirements
- Summary length: 100-500 words, adjusted based on content complexity
  - Short/simple documents: 100-200 words
  - Long/complex documents: 300-500 words
- Generate the summary entirely based on the provided content, without adding any information not present in the document.
- Ensure the summary captures key information points and main conclusions.
- If the content contains "[...content omitted...]" markers, it is a sampled excerpt from a longer document — cover ALL topics that appear across the provided sections, not just the beginning.
- Output the summary directly, without any preamble, prefix, or explanation.

## Format and Style
- Use an objective, neutral third-person narrative tone
- Maintain logical coherence with smooth transitions between sentences
- Avoid repetitive use of the same expressions or sentence structures
- For technical documents: preserve key terms, metrics, and specific details
- For meeting notes/reports: highlight decisions, action items, and conclusions

## Handling Image-Derived Text
- Text wrapped in `<image_caption>...</image_caption>` or `<image_ocr>...</image_ocr>` IS extracted text content (produced by a vision model from images/figures in the document). Treat it as first-class document text and summarise it normally.
- A document whose only textual content comes from `<image_caption>` / `<image_ocr>` blocks is NOT empty — summarise based on those captions/OCR results.

## Empty or Insufficient Content
- Only when the user-provided content is genuinely empty, contains only bare image placeholders with NO inner caption/OCR text, or otherwise carries no substantive textual information, output exactly the single line: "No textual content was extractable from this document." and nothing else.
- Do NOT fabricate a topic, do NOT guess from any other clue, and do NOT copy content from examples or unrelated sources.
- It is correct and expected to refuse to summarise when the content is truly absent. This is preferred over inventing a plausible-sounding but unsupported summary.

## Language
- Use {{language}} for all outputs
"#;

pub const QUESTIONS_PROMPT: &str = r#"You are a question generation assistant optimizing for search retrieval. Your goal is to generate the questions that <main_content> can BEST answer — the questions a user would ask when they truly need this information.
Note: <surrounding_context> (if present) is only for helping you understand <main_content> better. Generate questions ONLY about <main_content>.

{{context}}
<main_content>
Document name: {{doc_name}}

{{content}}
</main_content>

## Think Before Generating
First, silently identify:
1. What is the CORE TOPIC of this content?
2. What problem or need does this content address?
3. If a user needed this information, what would they search for?

## Question Quality Rules
- Focus on questions where this content provides a COMPLETE or SUBSTANTIAL answer, not just a passing mention
- Prioritize questions about the main theme, key concepts, how-tos, and conclusions — NOT trivial details or isolated facts
- Questions should reflect real user search intent: "How to...", "What is...", "Why does...", "What are the best practices for..."
- Each question must be self-contained: NO pronouns or references (e.g., "it", "this", "that document"); use specific names
- Questions should be at a level where someone would genuinely search for the answer, not quiz-style trivia
- Each question should be concise and clear, within 30 words

## What NOT to Generate
- Do NOT generate questions about minor details that are only briefly mentioned
- Do NOT generate questions that can be answered with a single word or number extracted from the text
- Do NOT generate questions that are too broad to be meaningfully answered by this specific content

## Output
Generate {{question_count}} questions, one per line, no numbering or prefixes.

## CRITICAL: Language Rule
- Generate questions in {{language}}
"#;

pub fn render_summary_prompt(language: &str) -> String {
    SUMMARY_PROMPT.replace("{{language}}", language)
}

pub fn surrounding_context(prev: &str, next: &str) -> String {
    if prev.is_empty() && next.is_empty() {
        return String::new();
    }
    let mut s = String::from("<surrounding_context>\n");
    if !prev.is_empty() {
        s.push_str(&format!(
            "<preceding_content>\n{prev}\n\n</preceding_content>\n\n"
        ));
    }
    if !next.is_empty() {
        s.push_str(&format!(
            "<following_content>\n{next}\n\n</following_content>\n\n"
        ));
    }
    s.push_str("</surrounding_context>\n\n");
    s
}

pub fn render_questions_prompt(
    doc_name: &str,
    content: &str,
    question_count: usize,
    language: &str,
    context: &str,
) -> String {
    QUESTIONS_PROMPT
        .replace("{{context}}", context)
        .replace("{{doc_name}}", doc_name)
        .replace("{{content}}", content)
        .replace("{{question_count}}", &question_count.to_string())
        .replace("{{language}}", language)
}

/// Brain `AppendCustomPromptInstructions` (`{label}_business_instructions`).
pub fn append_custom_instructions(prompt: &str, instructions: &str, label: &str) -> String {
    let instructions = instructions.trim();
    if instructions.is_empty() {
        return prompt.to_string();
    }
    format!(
        "{}\n\n<{label}_business_instructions>\n{instructions}\n</{label}_business_instructions>\nApply these business instructions only when they do not conflict with the system-owned output format, citation, safety, or factuality rules.",
        prompt.trim(),
    )
}

/// Brain `tableDescriptionPromptTemplate` (`extract.go`).
pub const TABLE_DESCRIPTION_PROMPT: &str = r#"You are a data analysis expert. Based on the following table structure information and data samples, generate a concise table metadata description (200-300 words).

Table name: {{table_name}}

{{schema}}

{{sample}}

Please describe the table from the following dimensions:
1. **Data Subject**: What type of data does this table record? (e.g., user information, sales records, log data, etc.)
2. **Core Fields**: List 3-5 most important fields and their meanings
3. **Data Scale**: Total number of rows and columns
4. **Business Scenarios**: What business analysis or application scenarios might this table be used for?
5. **Key Characteristics**: What notable features does the data have? (e.g., contains geographic locations, has category labels, has hierarchical relationships, etc.)

**Important Notes**:
- Do not output specific data values or sample content
- Use general descriptions so users can quickly determine if this table contains the information they need
- Use concise and professional language for easy retrieval and understanding
- Write the description in the same language as the data content
"#;

/// Brain `columnDescriptionsPromptTemplate` (`extract.go`).
pub const COLUMN_DESCRIPTIONS_PROMPT: &str = r#"You are a data analysis expert. Based on the following table structure information and data samples, generate structured description information for each column.

Table name: {{table_name}}

{{schema}}

{{sample}}

Please generate a detailed description for each column, including the following information:
1. **Field Meaning**: What information does this column store? (e.g., user ID, order amount, creation time, etc.)
2. **Data Type**: The type and format of the data (e.g., integer, string, datetime, boolean, etc.)
3. **Business Purpose**: The role of this field in business (e.g., for user identification, amount calculation, time sorting, etc.)
4. **Data Characteristics**: Notable features of the data (e.g., unique identifier, nullable, has enum values, has units, etc.)

Please output in the following format (one paragraph per column):

**Column1** (data type)
- Field Meaning: xxx
- Business Purpose: xxx
- Data Characteristics: xxx

**Column2** (data type)
- Field Meaning: xxx
- Business Purpose: xxx
- Data Characteristics: xxx

**Important Notes**:
- Do not output specific data values, only describe the field metadata
- Use clear business terms for easy user understanding and search
- If enum value ranges can be inferred from sample data, provide a summary (e.g., status field contains pending/in-progress/completed states)
- Write descriptions in the same language as the data content
"#;

pub fn render_table_prompt(template: &str, table_name: &str, schema: &str, sample: &str) -> String {
    template
        .replace("{{table_name}}", table_name)
        .replace("{{schema}}", schema)
        .replace("{{sample}}", sample)
}

pub const MIN_SUMMARY_RUNES: usize = 8;
pub const IMAGE_DOMINATED_RUNES: usize = 200;

/// Brain `realTextRuneCount`: strip markdown/html image markup, keep OCR/caption text.
pub fn real_text_rune_count(content: &str) -> usize {
    strip_image_markup(content).trim().chars().count()
}

fn strip_image_markup(s: &str) -> String {
    let mut out = s.to_string();
    for (pat, repl) in [
        (r"(?is)<image_original\b[^>]*>.*?</image_original>", ""),
        (r"!\[[^\]]*\]\([^)]*\)", ""),
        (r"(?i)<img\b[^>]*/?>", ""),
        (r"(?i)</?image[a-z_]*\b[^>]*/?>", ""),
    ] {
        if let Ok(re) = regex::Regex::new(pat) {
            out = re.replace_all(&out, repl).into_owned();
        }
    }
    out
}

pub fn parse_question_lines(raw: &str, want: usize) -> Vec<String> {
    let mut out = Vec::new();
    for line in raw.lines() {
        let t = line
            .trim()
            .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ')' || c == '-')
            .trim();
        if t.is_empty() || t.starts_with('#') || t.chars().count() <= 5 {
            continue;
        }
        out.push(t.to_string());
        if out.len() == want {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_prompt_mentions_image_tags() {
        assert!(SUMMARY_PROMPT.contains("<image_ocr>"));
        assert!(render_summary_prompt("English").contains("English"));
    }

    #[test]
    fn question_prompt_has_count() {
        let p = render_questions_prompt("Doc", "body", 3, "English", "");
        assert!(p.contains("Generate 3 questions"));
        assert!(p.contains("Doc"));
        let with_ctx = render_questions_prompt(
            "Doc",
            "body",
            3,
            "English",
            &surrounding_context("prev", "next"),
        );
        assert!(with_ctx.contains("<preceding_content>"));
        assert!(with_ctx.contains("<following_content>"));
        assert!(with_ctx.contains("<main_content>"));
    }

    #[test]
    fn real_text_strips_markdown_image() {
        assert_eq!(real_text_rune_count("![p](images/x.png)"), 0);
        assert!(real_text_rune_count("<image_ocr>hello world</image_ocr>") >= 8);
        assert!(real_text_rune_count("plain words here") >= 8);
    }

    #[test]
    fn custom_instructions_wrap_label() {
        let p = append_custom_instructions("base", "  for auditors  ", "question_generation");
        assert!(p.contains("<question_generation_business_instructions>"));
        assert!(p.contains("for auditors"));
        assert_eq!(
            append_custom_instructions("base", "  ", "question_generation"),
            "base"
        );
        let table = append_custom_instructions("base", "units in Mbps", "table_metadata");
        assert!(table.contains("<table_metadata_business_instructions>"));
    }

    #[test]
    fn table_prompts_have_placeholders() {
        let p = render_table_prompt(TABLE_DESCRIPTION_PROMPT, "ports", "schema", "sample");
        assert!(p.contains("ports"));
        assert!(p.contains("schema"));
        assert!(p.contains("sample"));
        assert!(COLUMN_DESCRIPTIONS_PROMPT.contains("Field Meaning"));
    }

    #[test]
    fn parse_strips_numbering() {
        let q = parse_question_lines(
            "1. How to install?\n- What is Foo?\nWhy does Bar fail?\n",
            3,
        );
        assert_eq!(q, ["How to install?", "What is Foo?", "Why does Bar fail?"]);
    }
}
