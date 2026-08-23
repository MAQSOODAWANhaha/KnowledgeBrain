//! POST /answer — thin RAG over assembly. Chat model is always current.summary_model_id.

use crate::{AssemblyResponse, Hit, SearchError, SearchRequest, assembly};
use domain::Store;
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

pub fn current_summary_model(store: &Store, product_id: Uuid) -> Result<String, SearchError> {
    let product = store.products.get(&product_id).ok_or(SearchError {
        code: "NOT_FOUND",
        message: "product not found".into(),
    })?;
    let vid = product.current_version_id.ok_or(SearchError {
        code: "VALIDATION",
        message: "product has no current version".into(),
    })?;
    let version = store.versions.get(&vid).ok_or(SearchError {
        code: "NOT_FOUND",
        message: "current version not found".into(),
    })?;
    Ok(version.summary_model_id.clone())
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

pub fn answer(store: &Store, req: &AnswerRequest) -> Result<AnswerResponse, SearchError> {
    if req.query.trim().is_empty() {
        return Err(SearchError {
            code: "VALIDATION",
            message: "query required".into(),
        });
    }
    let model_id = current_summary_model(store, req.product_id)?;
    let product = store.products.get(&req.product_id).ok_or(SearchError {
        code: "NOT_FOUND",
        message: "product not found".into(),
    })?;
    let search_req = SearchRequest {
        mode: "assembly".into(),
        query: Some(req.query.clone()),
        product_id: Some(req.product_id),
        version_id: req.version_id.clone(),
        include_library: req.include_library,
        tag_ids: req.tag_ids.clone(),
        match_count: 8,
        expand_wiki: true,
        expand_graph: true,
        requirements: vec![],
        version_scope: "current".into(),
        product_ids: vec![],
        workspace_id: Some(product.workspace_id),
        scope: None,
        group_by: "none".into(),
        tender_text: None,
    };
    let AssemblyResponse { hits, warnings: _ } = assembly(store, &search_req)?;
    Ok(answer_from_hits(&req.query, &req.context, hits, &model_id))
}

pub fn answer_from_hits(
    query: &str,
    context: &[String],
    hits: Vec<crate::Hit>,
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
    let answer = enrichment::chat_complete(&system, &user, model_id).unwrap_or_default();
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
    use crate::Hit;
    use domain::{Document, Product, ProductKind, ProductVersion, Store, Workspace};
    use index::index_one;
    use uuid::Uuid;

    fn seed_product(s: &mut Store) -> (Uuid, Uuid, Uuid) {
        let ws = Workspace {
            id: Uuid::new_v4(),
            name: "ws".into(),
            slug: "ws".into(),
            kind: Default::default(),
            retrieval: domain::RetrievalConfig::default(),
        };
        let wid = ws.id;
        s.workspaces.insert(wid, ws);
        let p = Product {
            id: Uuid::new_v4(),
            workspace_id: wid,
            kind: ProductKind::Product,
            name: "sw".into(),
            slug: "sw".into(),
            current_version_id: None,
            embedding_model_id: "stub-emb".into(),
        };
        let pid = p.id;
        s.products.insert(pid, p);
        let mut v = ProductVersion::new(pid, "v1".into());
        v.summary_model_id = "stub-chat".into();
        let vid = v.id;
        s.versions.insert(vid, v);
        s.products.get_mut(&pid).unwrap().current_version_id = Some(vid);
        (wid, pid, vid)
    }

    fn add_hit_doc(s: &mut Store, vid: Uuid, title: &str, body: &str) {
        let doc = Document::new(vid, title.into(), "a.txt".into(), 1, "h".into(), "h".into());
        let did = doc.id;
        let mut d = doc;
        d.enable_status = "enabled".into();
        s.documents.insert(did, d);
        let ch = domain::Chunk {
            id: Uuid::new_v4(),
            document_id: did,
            product_version_id: vid,
            chunk_type: "text".into(),
            content: body.into(),
            context_header: String::new(),
            start_at: 0,
            end_at: body.chars().count() as i32,
            parent_chunk_id: None,
            generated_questions: Vec::new(),
        };
        let _ = index_one(s, &ch, title, true, true);
        s.chunks.insert(ch.id, ch);
    }

    #[test]
    fn model_always_from_current_even_if_other_version_requested() {
        let mut s = Store::default();
        let (_, pid, vid) = seed_product(&mut s);
        s.versions.get_mut(&vid).unwrap().summary_model_id = "current-chat".into();
        let mut other = ProductVersion::new(pid, "v2".into());
        other.summary_model_id = "other-chat".into();
        s.versions.insert(other.id, other);
        assert_eq!(current_summary_model(&s, pid).unwrap(), "current-chat");
    }

    #[test]
    fn no_current_is_validation() {
        let mut s = Store::default();
        let (_, pid, _) = seed_product(&mut s);
        s.products.get_mut(&pid).unwrap().current_version_id = None;
        let err = current_summary_model(&s, pid).unwrap_err();
        assert_eq!(err.code, "VALIDATION");
    }

    #[test]
    fn empty_hits_do_not_fabricate() {
        let mut s = Store::default();
        let (_, pid, _) = seed_product(&mut s);
        let out = answer(
            &s,
            &AnswerRequest {
                query: "anything".into(),
                product_id: pid,
                version_id: Some("current".into()),
                include_library: false,
                tag_ids: vec![],
                context: vec![],
            },
        )
        .unwrap();
        assert!(out.answer.is_empty());
        assert!(out.hits.is_empty());
        assert!(out.citations.is_empty());
    }

    #[test]
    fn answers_from_hits_via_current_model() {
        let mut s = Store::default();
        let (_, pid, vid) = seed_product(&mut s);
        add_hit_doc(&mut s, vid, "spec", "device throughput is 40Gbps");
        let out = answer(
            &s,
            &AnswerRequest {
                query: "What throughput?".into(),
                product_id: pid,
                version_id: Some("current".into()),
                include_library: false,
                tag_ids: vec![],
                context: vec![],
            },
        )
        .unwrap();
        assert!(!out.answer.is_empty());
        assert!(
            out.answer.contains("40Gbps") || out.answer.contains("throughput"),
            "{}",
            out.answer
        );
        assert!(!out.citations.is_empty());
        assert_eq!(out.citations[0].version_id, vid);
        assert!(ANSWER_SYSTEM_PROMPT.contains("retrieved information"));
        let sys = render_answer_system("English", &out.hits);
        assert!(sys.contains("40Gbps"));
        assert!(sys.contains("English"));
    }

    #[test]
    fn omitted_version_id_searches_all_active() {
        let mut s = Store::default();
        let (_, pid, vid) = seed_product(&mut s);
        add_hit_doc(&mut s, vid, "spec", "device throughput is 40Gbps");
        let mut v2 = ProductVersion::new(pid, "v2".into());
        v2.summary_model_id = "stub-chat".into();
        let vid2 = v2.id;
        s.versions.insert(vid2, v2);
        add_hit_doc(&mut s, vid2, "iso", "factory holds ISO9001 certificate");
        let all = answer(
            &s,
            &AnswerRequest {
                query: "ISO9001".into(),
                product_id: pid,
                version_id: None,
                include_library: false,
                tag_ids: vec![],
                context: vec![],
            },
        )
        .unwrap();
        assert!(
            all.hits.iter().any(|h| h.version_id == vid2),
            "all-active should reach v2: {:?}",
            all.hits.iter().map(|h| h.version_id).collect::<Vec<_>>()
        );
        let current = answer(
            &s,
            &AnswerRequest {
                query: "ISO9001".into(),
                product_id: pid,
                version_id: Some("current".into()),
                include_library: false,
                tag_ids: vec![],
                context: vec![],
            },
        )
        .unwrap();
        assert!(current.hits.iter().all(|h| h.version_id == vid));
    }

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
