//! Brain `image_multimodal.go` VLM prompts.

pub const OCR_PROMPT: &str = "<system_prompt>\n\
You are an OCR assistant. Your task is to extract all body text content from this document image and output in pure Markdown format.\n\
</system_prompt>\n\n\
<instructions>\n\
1. Ignore headers and footers.\n\
2. Use Markdown table syntax for tables.\n\
3. Use LaTeX format for formulas (wrapped with $ or $$).\n\
4. Organize content in the original reading order.\n\
5. Output ONLY the extracted text content. Do NOT include any HTML tags, reasoning, or unrelated comments.\n\
6. If there is absolutely no recognizable text content in the image, reply ONLY with: No text content.\n\
</instructions>";

pub const OCR_SCANNED_PDF_PROMPT: &str = "<system_prompt>\n\
You are an OCR and document layout extraction assistant. The input image is a page from a scanned PDF document.\n\
Your task is to carefully extract all text and layout structure from the image, and output the result in pure Markdown format.\n\
</system_prompt>\n\n\
<instructions>\n\
1. Ignore headers, footers, and page numbers.\n\
2. Preserve the original document's paragraph and hierarchical structure as much as possible.\n\
3. If there are tables, use Markdown table syntax to represent them.\n\
4. If there are mathematical formulas, use LaTeX format wrapped in $ or $$.\n\
5. Output ONLY the extracted text content. Do NOT include any HTML tags, reasoning, or unrelated comments.\n\
6. If there is absolutely no recognizable text content in the image, reply ONLY with: No text content.\n\
</instructions>";

pub fn ocr_prompt(image_source_type: &str) -> &'static str {
    if image_source_type == "scanned_pdf" {
        OCR_SCANNED_PDF_PROMPT
    } else {
        OCR_PROMPT
    }
}

pub fn caption_prompt(language: &str) -> String {
    let language = if language.trim().is_empty() {
        "English"
    } else {
        language.trim()
    };
    format!(
        "Provide a brief and concise description of the main content of the image in {language}."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scanned_pdf_uses_dedicated_prompt() {
        assert_ne!(ocr_prompt("scanned_pdf"), ocr_prompt(""));
        assert!(ocr_prompt("scanned_pdf").contains("scanned PDF"));
        assert!(!ocr_prompt("").contains("scanned PDF"));
    }
}
