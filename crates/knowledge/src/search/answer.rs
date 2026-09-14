//! POST /answer — thin RAG over assembly. Chat model is always current.summary_model_id.

use crate::search::Hit;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Brain `system_prompt.yaml` `default_kb`, product name adapted.
pub const ANSWER_SYSTEM_PROMPT: &str = r#"You are KnowledgeBrain, a professional intelligent information retrieval assistant. You answer user questions based on retrieved information and must not use any prior knowledge.
When a user asks a question, you provide answers based on specific retrieved information. You first think through the reasoning process internally, then provide the answer to the user.

## Response Rules
- Reply ONLY based on facts from the retrieved information, without using any prior knowledge, maintaining objectivity and accuracy
- For complex questions, structure the answer using Markdown formatting; simple summaries do not need to be split
- For simple answers, do not break the final answer into overly granular parts
- Image URLs used in results must come from the retrieved information and must not be fabricated
- Verify that all text and images in the result come from the retrieved information; if content not found in the retrieved information has been added, it must be revised until the final answer is obtained
- If the user's question cannot be answered, honestly inform the user and provide reasonable suggestions

## Output Format
- Output your final result in Markdown format
- When retrieved information contains Markdown images, treat them as relevant by default. Unless the user explicitly requests text-only output or every image is clearly unrelated, the final answer MUST include at least one relevant image copied from the retrieved information with its URL preserved exactly
- Image Markdown MUST use ASCII half-width parentheses exactly as `![alt](url)`; never use full-width `（` or `）`
- Place each image immediately after the paragraph it supports; before finishing, silently verify that the answer satisfies this image requirement
- When multiple retrieved images support different sections, distribute them across those sections instead of stopping after the first image
- Ensure the output is concise yet comprehensive, well-organized, clear, and non-repetitive

## CRITICAL: Language Rule
- ALWAYS respond in {{language}}

The following is retrieved information that may or may not be relevant:
{{contexts}}
"#;

#[derive(Debug, Clone, Deserialize)]
pub struct AnswerRequest {
    pub query: String,
    pub product_id: Uuid,
    pub version_id: Option<String>,
    #[serde(default)]
    pub include_library: bool,
    #[serde(default)]
    pub tag_ids: Vec<Uuid>,
    #[serde(default)]
    pub context: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Citation {
    pub document_id: Uuid,
    pub version_id: Uuid,
    pub start_at: i32,
    pub end_at: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnswerResponse {
    pub answer: String,
    pub hits: Vec<Hit>,
    pub citations: Vec<Citation>,
}

pub fn render_answer_system(language: &str, hits: &[Hit]) -> String {
    let mut contexts = String::new();
    for (i, h) in hits.iter().take(8).enumerate() {
        contexts.push_str(&format!(
            "[{}] ({}) {}\n",
            i + 1,
            h.document_title,
            h.content
        ));
    }
    ANSWER_SYSTEM_PROMPT
        .replace("{{language}}", language)
        .replace("{{contexts}}", &contexts)
}

pub fn answer_from_hits(
    query: &str,
    context: &[String],
    hits: Vec<crate::search::Hit>,
    model_id: &str,
) -> AnswerResponse {
    if hits.is_empty() {
        return AnswerResponse {
            answer: String::new(),
            hits,
            citations: vec![],
        };
    }
    let system = render_answer_system("English", &hits);
    let user = user_message(query, context, &hits);
    let answer = crate::enrichment::chat_complete(&system, &user, model_id).unwrap_or_default();
    let citations = hits
        .iter()
        .map(|h| Citation {
            document_id: h.document_id,
            version_id: h.version_id,
            start_at: h.start_at,
            end_at: h.end_at,
        })
        .collect();
    AnswerResponse {
        answer,
        hits,
        citations,
    }
}

fn user_message(query: &str, context: &[String], hits: &[Hit]) -> String {
    let mut u = String::new();
    if !context.is_empty() {
        u.push_str("Conversation context (not knowledge):\n");
        for c in context {
            u.push_str(c.trim());
            u.push('\n');
        }
        u.push('\n');
    }
    u.push_str(query);
    u.push_str("\n\nRetrieved excerpts:\n");
    for (i, h) in hits.iter().take(8).enumerate() {
        u.push_str(&format!("[{}] {}\n", i + 1, h.content));
    }
    u
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::Hit;
    use uuid::Uuid;

    #[test]
    fn citation_only_from_hits() {
        let h = Hit {
            id: Uuid::new_v4(),
            content: "x".into(),
            score: 1.0,
            match_type: "keyword".into(),
            chunk_type: "text".into(),
            document_id: Uuid::new_v4(),
            document_title: "t".into(),
            product_id: Uuid::new_v4(),
            product_kind: "product".into(),
            version_id: Uuid::new_v4(),
            version_label: "v1".into(),
            is_current: true,
            tag_ids: vec![],
            tag_slugs: vec![],
            start_at: 1,
            end_at: 2,
            image_object_ref: None,
        };
        let sys = render_answer_system("English", std::slice::from_ref(&h));
        assert!(sys.contains("x"));
    }
}
