//! summary / question / multimodal. Never fail parent parse_status.

mod chat;
mod language;
mod ocr;
mod pending;
mod prompts;
mod summary;

pub use chat::{
    ChatMessage, WIKI_LLM_MAX_ATTEMPTS, WIKI_LLM_MAX_TOKENS, attempt_superseded, chat_complete,
    chat_complete_limited, chat_complete_wiki, chat_http_configured, chat_messages,
    chat_messages_limited, sample_long_content,
};
pub use language::{infer_output_language, language_for_document, normalize_language_tag};
pub use ocr::sanitize_ocr_text;
pub use pending::{
    decr_pending, decr_pending_count, pending_count, pending_key, set_pending, set_pending_count,
};
pub use prompts::{OCR_PROMPT, OCR_SCANNED_PDF_PROMPT, caption_prompt, ocr_prompt};
pub use summary::{
    COLUMN_DESCRIPTIONS_PROMPT, IMAGE_DOMINATED_RUNES, MIN_SUMMARY_RUNES, SUMMARY_PROMPT,
    TABLE_DESCRIPTION_PROMPT, append_custom_instructions, real_text_rune_count,
    render_questions_prompt, render_summary_prompt, render_table_prompt, surrounding_context,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuestionOutcome {
    Done,
    Superseded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummaryOutcome {
    Done,
    Superseded,
}

use crate::index::index_one;
use crate::{Chunk, Store, SummaryStatus};
use uuid::Uuid;

pub const SUMMARY_MAX_INPUT: usize = 24 * 1024;

pub fn generate_summary(store: &mut Store, document_id: Uuid) {
    let _ = generate_summary_with(store, document_id, 0, false);
}

pub fn generate_summary_for_attempt(
    store: &mut Store,
    document_id: Uuid,
    job_attempt: i32,
) -> Result<SummaryOutcome, String> {
    generate_summary_with(store, document_id, job_attempt, false)
}

pub fn generate_summary_with(
    store: &mut Store,
    document_id: Uuid,
    job_attempt: i32,
    fallback: bool,
) -> Result<SummaryOutcome, String> {
    let Some(doc) = store.documents.get(&document_id).cloned() else {
        return Ok(SummaryOutcome::Done);
    };
    if attempt_superseded(doc.attempt, job_attempt) {
        return Ok(SummaryOutcome::Superseded);
    }
    if doc.parse_status.is_aborted() {
        store.finalize_subtask(document_id);
        return Ok(SummaryOutcome::Done);
    }
    let Some(version) = store.versions.get(&doc.product_version_id).cloned() else {
        store.finalize_subtask(document_id);
        return Ok(SummaryOutcome::Done);
    };
    if let Some(d) = store.documents.get_mut(&document_id) {
        d.summary_status = SummaryStatus::Processing;
    }
    let mut texts: Vec<_> = store
        .chunks
        .values()
        .filter(|c| c.document_id == document_id && c.chunk_type == "text")
        .cloned()
        .collect();
    texts.sort_by_key(|c| c.start_at);
    let parent_id = texts.first().map(|c| c.id);
    let mut body = assemble_by_start_at(&texts);
    body = sample_long_content(&body, SUMMARY_MAX_INPUT);
    if real_text_rune_count(&body) < IMAGE_DOMINATED_RUNES {
        let extra: String = store
            .chunks
            .values()
            .filter(|c| {
                c.document_id == document_id
                    && matches!(c.chunk_type.as_str(), "image_ocr" | "image_caption")
            })
            .map(|c| {
                if c.chunk_type == "image_ocr" {
                    format!("<image_ocr>{}</image_ocr>", c.content)
                } else {
                    format!("<image_caption>{}</image_caption>", c.content)
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        body.push_str(&extra);
        body = sample_long_content(&body, SUMMARY_MAX_INPUT);
    }
    if real_text_rune_count(&body) < MIN_SUMMARY_RUNES {
        if let Some(d) = store.documents.get_mut(&document_id) {
            d.description.clear();
            d.summary_status = SummaryStatus::Failed;
        }
        store.finalize_subtask(document_id);
        return Ok(SummaryOutcome::Done);
    }
    let language = language_for_document(store, document_id);
    tracing::info!(%document_id, language = %language, "summary language");
    let system = render_summary_prompt(&language);
    let user = format!("Output language: {language}\n\n{body}");
    let summary = match chat::chat_complete(&system, &user, &version.summary_model_id) {
        Ok(s) if !s.trim().is_empty() => s,
        other => {
            if fallback {
                let stem: String = texts
                    .first()
                    .map(|c| c.content.chars().take(500).collect())
                    .unwrap_or_default();
                if real_text_rune_count(&stem) < MIN_SUMMARY_RUNES {
                    if let Some(d) = store.documents.get_mut(&document_id) {
                        d.description.clear();
                        d.summary_status = SummaryStatus::Failed;
                    }
                    store.finalize_subtask(document_id);
                    return Ok(SummaryOutcome::Done);
                }
                stem
            } else if chat_http_configured() && version.summary_model_id != "stub-chat" {
                return Err(other.err().unwrap_or_else(|| "chat empty".into()));
            } else {
                if let Some(d) = store.documents.get_mut(&document_id) {
                    d.summary_status = SummaryStatus::Failed;
                }
                store.finalize_subtask(document_id);
                return Ok(SummaryOutcome::Done);
            }
        }
    };
    drop_prior_summary_chunks(store, document_id);
    if let Some(d) = store.documents.get_mut(&document_id) {
        d.description = summary.clone();
        d.summary_status = SummaryStatus::Completed;
    }
    let chunk = Chunk {
        id: Uuid::new_v4(),
        document_id,
        product_version_id: doc.product_version_id,
        chunk_type: "summary".into(),
        content: summary.clone(),
        context_header: String::new(),
        start_at: 0,
        end_at: summary.chars().count() as i32,
        parent_chunk_id: parent_id,
        generated_questions: Vec::new(),
    };
    index_one(
        store,
        &chunk,
        &doc.title,
        version.vector_enabled,
        version.keyword_enabled,
    )?;
    store.chunks.insert(chunk.id, chunk);
    store.finalize_subtask(document_id);
    Ok(SummaryOutcome::Done)
}

fn assemble_by_start_at(chunks: &[Chunk]) -> String {
    let mut out = String::new();
    for c in chunks {
        let start = (c.start_at.max(0) as usize).min(out.chars().count());
        let prefix: String = out.chars().take(start).collect();
        out = format!("{}{}", prefix, c.content);
    }
    out
}

fn drop_prior_summary_chunks(store: &mut Store, document_id: Uuid) {
    let drop: Vec<Uuid> = store
        .chunks
        .values()
        .filter(|c| c.document_id == document_id && c.chunk_type == "summary")
        .map(|c| c.id)
        .collect();
    for id in drop {
        store.chunks.remove(&id);
        store.embeddings.remove(&id);
    }
}

pub fn generate_questions(store: &mut Store, chunk_ids: &[Uuid], document_id: Uuid) {
    let _ = generate_questions_with(store, chunk_ids, &[], &[], document_id, 0);
}

pub fn generate_questions_for_attempt(
    store: &mut Store,
    chunk_ids: &[Uuid],
    document_id: Uuid,
    job_attempt: i32,
) -> Result<QuestionOutcome, String> {
    generate_questions_with(store, chunk_ids, &[], &[], document_id, job_attempt)
}

pub fn generate_questions_with(
    store: &mut Store,
    chunk_ids: &[Uuid],
    prev_ids: &[Option<Uuid>],
    next_ids: &[Option<Uuid>],
    document_id: Uuid,
    job_attempt: i32,
) -> Result<QuestionOutcome, String> {
    let Some(doc) = store.documents.get(&document_id).cloned() else {
        return Ok(QuestionOutcome::Done);
    };
    if attempt_superseded(doc.attempt, job_attempt) {
        return Ok(QuestionOutcome::Superseded);
    }
    if doc.parse_status.is_aborted() {
        store.finalize_subtask(document_id);
        return Ok(QuestionOutcome::Done);
    }
    let Some(version) = store.effective_version(document_id) else {
        store.finalize_subtask(document_id);
        return Ok(QuestionOutcome::Done);
    };
    drop_prior_question_chunks(store, chunk_ids);
    let want = version.question_count();
    let language = language_for_document(store, document_id);
    tracing::info!(%document_id, language = %language, "question language");
    for (i, cid) in chunk_ids.iter().enumerate() {
        let Some(mut ch) = store.chunks.get(cid).cloned() else {
            continue;
        };
        if ch.chunk_type != "text" || ch.content.trim().is_empty() {
            continue;
        }
        let prev_content = neighbor_content(store, prev_ids.get(i).and_then(|x| *x), &ch, true);
        let next_content = neighbor_content(store, next_ids.get(i).and_then(|x| *x), &ch, false);
        let ctx = surrounding_context(&prev_content, &next_content);
        let prompt = append_custom_instructions(
            &render_questions_prompt(&doc.title, &ch.content, want, &language, &ctx),
            &version.question_custom_instructions,
            "question_generation",
        );
        let raw = match chat::chat_complete(
            &prompt,
            &format!("Output language: {language}\n\n{}", ch.content),
            &version.summary_model_id,
        ) {
            Ok(s) => s,
            Err(_) if chat_http_configured() && version.summary_model_id != "stub-chat" => {
                continue;
            }
            Err(_) => String::new(),
        };
        let mut qs = summary::parse_question_lines(&raw, want);
        if qs.is_empty() {
            if chat_http_configured() && version.summary_model_id != "stub-chat" {
                continue;
            }
            let stem: String = ch.content.chars().take(40).collect();
            qs = fallback_questions(&stem, want);
        }
        ch.generated_questions = qs.clone();
        for q in &qs {
            let qc = Chunk {
                id: Uuid::new_v4(),
                document_id,
                product_version_id: doc.product_version_id,
                chunk_type: "question".into(),
                content: q.clone(),
                context_header: String::new(),
                start_at: 0,
                end_at: q.chars().count() as i32,
                parent_chunk_id: Some(ch.id),
                generated_questions: Vec::new(),
            };
            index_one(
                store,
                &qc,
                &doc.title,
                version.vector_enabled,
                version.keyword_enabled,
            )?;
            store.chunks.insert(qc.id, qc);
        }
        store.chunks.insert(*cid, ch);
    }
    store.finalize_subtask(document_id);
    Ok(QuestionOutcome::Done)
}

fn neighbor_content(store: &Store, hinted: Option<Uuid>, ch: &Chunk, prev: bool) -> String {
    if let Some(id) = hinted
        && let Some(n) = store.chunks.get(&id)
    {
        return n.content.clone();
    }
    let cand = store.chunks.values().filter(|o| {
        o.document_id == ch.document_id
            && o.chunk_type == "text"
            && o.parent_chunk_id.is_none()
            && o.id != ch.id
    });
    if prev {
        cand.filter(|o| o.end_at <= ch.start_at)
            .max_by_key(|o| o.end_at)
            .map(|o| o.content.clone())
            .unwrap_or_default()
    } else {
        cand.filter(|o| o.start_at >= ch.end_at)
            .min_by_key(|o| o.start_at)
            .map(|o| o.content.clone())
            .unwrap_or_default()
    }
}

fn drop_prior_question_chunks(store: &mut Store, parent_ids: &[Uuid]) {
    let drop: Vec<Uuid> = store
        .chunks
        .values()
        .filter(|c| {
            (c.chunk_type == "question"
                || (c.chunk_type == "text" && c.generated_questions.is_empty()))
                && c.parent_chunk_id.is_some_and(|p| parent_ids.contains(&p))
        })
        .map(|c| c.id)
        .collect();
    for id in drop {
        store.chunks.remove(&id);
        store.embeddings.remove(&id);
    }
}

fn fallback_questions(stem: &str, want: usize) -> Vec<String> {
    const TEMPLATES: [&str; 3] = ["How to {s}?", "What is {s}?", "Why does {s}?"];
    (0..want)
        .map(|i| TEMPLATES[i % TEMPLATES.len()].replace("{s}", stem))
        .collect()
}

pub fn process_image(store: &mut Store, document_id: Uuid, image_key: &str) {
    let _ = process_image_with(store, document_id, image_key, "", true, true);
}

/// OCR/caption + index. DECR `multimodal:pending` after the caller persists.
pub fn process_image_without_decr(
    store: &mut Store,
    document_id: Uuid,
    image_key: &str,
    image_source_type: &str,
    enable_ocr: bool,
    enable_caption: bool,
) -> Result<(), String> {
    process_image_core(
        store,
        document_id,
        image_key,
        image_source_type,
        enable_ocr,
        enable_caption,
        false,
    )
}

pub fn process_image_with(
    store: &mut Store,
    document_id: Uuid,
    image_key: &str,
    image_source_type: &str,
    enable_ocr: bool,
    enable_caption: bool,
) -> Result<(), String> {
    process_image_core(
        store,
        document_id,
        image_key,
        image_source_type,
        enable_ocr,
        enable_caption,
        true,
    )
}

fn process_image_core(
    store: &mut Store,
    document_id: Uuid,
    image_key: &str,
    image_source_type: &str,
    enable_ocr: bool,
    enable_caption: bool,
    decr: bool,
) -> Result<(), String> {
    let Some(doc) = store.documents.get(&document_id).cloned() else {
        if decr && decr_pending(store, document_id) {
            enqueue_post_process(store, document_id);
        }
        return Ok(());
    };
    if doc.parse_status.is_aborted() {
        if decr && decr_pending(store, document_id) {
            enqueue_post_process(store, document_id);
        }
        return Ok(());
    }
    let Some(version) = store.versions.get(&doc.product_version_id).cloned() else {
        if decr && decr_pending(store, document_id) {
            enqueue_post_process(store, document_id);
        }
        return Ok(());
    };
    drop_prior_image_chunks(store, document_id, image_key);
    let parent = parent_text_chunk(store, document_id, image_key);
    let language = language_for_document(store, document_id);
    let (ocr, caption) = describe_image(image_key, image_source_type, &language)?;
    let mut parts = Vec::new();
    if enable_ocr {
        let ocr = sanitize_ocr_text(&ocr);
        if !ocr.is_empty() {
            parts.push(("image_ocr", ocr));
        }
    }
    if enable_caption && !caption.trim().is_empty() {
        parts.push(("image_caption", caption));
    }
    for (ctype, content) in parts {
        let mut ch = Chunk {
            id: Uuid::new_v4(),
            document_id,
            product_version_id: doc.product_version_id,
            chunk_type: ctype.into(),
            content: content.clone(),
            context_header: String::new(),
            start_at: 0,
            end_at: content.chars().count() as i32,
            parent_chunk_id: parent,
            generated_questions: Vec::new(),
        };
        index_one(
            store,
            &ch,
            &doc.title,
            version.vector_enabled,
            version.keyword_enabled,
        )?;
        ch.context_header = image_key.to_string();
        store.chunks.insert(ch.id, ch);
    }
    if decr && decr_pending(store, document_id) {
        enqueue_post_process(store, document_id);
    }
    Ok(())
}

/// `scanned_pdf` only when the file is a PDF whose real text is image-dominated.
pub fn image_source_type(file_name: &str, markdown: &str) -> &'static str {
    if file_name.to_ascii_lowercase().ends_with(".pdf")
        && real_text_rune_count(markdown) < IMAGE_DOMINATED_RUNES
    {
        "scanned_pdf"
    } else {
        ""
    }
}

fn parent_text_chunk(store: &Store, document_id: Uuid, image_key: &str) -> Option<uuid::Uuid> {
    let texts: Vec<_> = store
        .chunks
        .values()
        .filter(|c| c.document_id == document_id && c.chunk_type == "text")
        .collect();
    texts
        .iter()
        .find(|c| c.content.contains(image_key))
        .or(texts.first())
        .map(|c| c.id)
}

fn drop_prior_image_chunks(store: &mut Store, document_id: Uuid, image_key: &str) {
    let drop: Vec<uuid::Uuid> = store
        .chunks
        .values()
        .filter(|c| {
            c.document_id == document_id
                && matches!(c.chunk_type.as_str(), "image_ocr" | "image_caption")
                && c.context_header == image_key
        })
        .map(|c| c.id)
        .collect();
    for id in drop {
        store.chunks.remove(&id);
        store.embeddings.remove(&id);
    }
}

fn truncate_key(key: &str) -> &str {
    let t = key.trim_start_matches("objects/").trim_start_matches('/');
    match t.char_indices().nth(16) {
        Some((i, _)) => &t[..i],
        None => t,
    }
}

/// OCR + caption. Unconfigured or stub VLM is an error, never fake text.
pub fn describe_image(
    image_key: &str,
    image_source_type: &str,
    language: &str,
) -> Result<(String, String), String> {
    if !vlm_configured() {
        tracing::warn!(image_key = truncate_key(image_key), "vlm not configured");
        return Err("vlm not configured".into());
    }
    let ocr_p = ocr_prompt(image_source_type);
    let cap_p = caption_prompt(language);
    vlm_describe(image_key, ocr_p, &cap_p)
}

pub fn vlm_configured() -> bool {
    crate::vlm_configured()
}

fn vlm_base_url() -> String {
    crate::vlm_base_url()
}

fn vlm_describe(
    image_key: &str,
    ocr_prompt: &str,
    cap_prompt: &str,
) -> Result<(String, String), String> {
    let base = vlm_base_url();
    if base.is_empty() {
        return Err("vlm not configured".into());
    }
    let ocr = match vlm_complete(&base, ocr_prompt, image_key) {
        Ok(text) => text,
        Err(error) => {
            tracing::warn!(
                image_key = truncate_key(image_key),
                error = %error,
                "describe_image failed"
            );
            return Err(error);
        }
    };
    let cap = match vlm_complete(&base, cap_prompt, image_key) {
        Ok(text) => text,
        Err(error) => {
            tracing::warn!(
                image_key = truncate_key(image_key),
                error = %error,
                "describe_image failed"
            );
            return Err(error);
        }
    };
    if ocr.is_empty() && cap.is_empty() {
        tracing::warn!(image_key = truncate_key(image_key), "describe_image failed");
        return Err("vlm empty".into());
    }
    Ok((ocr, cap))
}

fn image_data_url(image_key: &str) -> Result<String, String> {
    let key = image_key.trim();
    if key.starts_with("http://") || key.starts_with("https://") || key.starts_with("data:") {
        return Ok(key.to_string());
    }
    let hash = key.trim_start_matches("objects/").trim_start_matches('/');
    let dir = std::env::var("OBJECT_DIR").unwrap_or_else(|_| "var/objects".into());
    let bytes = std::fs::read(std::path::Path::new(&dir).join(hash))
        .map_err(|e| format!("read image {hash}: {e}"))?;
    let mime = match bytes.first() {
        Some(0x89) => "image/png",
        Some(0x47) => "image/gif",
        Some(0x52) => "image/webp",
        _ => "image/jpeg",
    };
    let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes);
    Ok(format!("data:{mime};base64,{b64}"))
}

fn vlm_complete(base: &str, prompt: &str, image_key: &str) -> Result<String, String> {
    tokio::task::block_in_place(|| vlm_complete_inner(base, prompt, image_key))
}

fn vlm_complete_inner(base: &str, prompt: &str, image_key: &str) -> Result<String, String> {
    let key = crate::vlm_api_key();
    let model = {
        let m = crate::vlm_model();
        if m.is_empty() || m == "stub-vlm" {
            return Err("vlm model not configured".into());
        }
        m
    };
    let url = chat::completions_url_for_vlm(base);
    let image_url = image_data_url(image_key)?;
    let body = serde_json::json!({
        "model": model,
        "max_tokens": 1024,
        "messages": [{
            "role": "user",
            "content": [
                {"type": "text", "text": prompt},
                {"type": "image_url", "image_url": {"url": image_url}}
            ]
        }]
    });
    crate::models::chat_sse(&url, &key, body)
}

fn enqueue_post_process(store: &mut Store, document_id: Uuid) {
    store.enqueue(
        crate::TYPE_POST_PROCESS,
        crate::QUEUE_POSTPROCESS,
        serde_json::json!({ "document_id": document_id, "clone_keep": false }),
    );
}

pub fn markdown_image_keys(md: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut rest = md;
    while let Some(i) = rest.find("](") {
        let after = &rest[i + 2..];
        if let Some(end) = after.find(')') {
            let url = &after[..end];
            if url.contains("images/") || url.starts_with("http") || url.starts_with("objects/") {
                keys.push(url.to_string());
            }
            rest = &after[end + 1..];
        } else {
            break;
        }
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Chunk, Document, ProductVersion, Store, SummaryStatus};

    #[test]
    fn image_source_type_only_for_image_dominated_pdf() {
        assert_eq!(
            image_source_type("scan.pdf", "![p](images/p.jpg)"),
            "scanned_pdf"
        );
        let long: String = "word ".repeat(80);
        assert_eq!(image_source_type("scan.pdf", &long), "");
        assert_eq!(image_source_type("photo.png", "![p](images/p.jpg)"), "");
    }

    #[test]
    fn scanned_pdf_ocr_uses_dedicated_prompt() {
        assert!(ocr_prompt("scanned_pdf").contains("scanned PDF"));
        assert!(!ocr_prompt("").contains("scanned PDF"));
    }

    #[test]
    fn describe_image_without_vlm_is_error() {
        assert!(describe_image("images/x.png", "", "Chinese").is_err());
        let mut s = Store::default();
        let mut v = ProductVersion::new(Uuid::new_v4(), "v1".into());
        v.enable_multimodel = true;
        let vid = v.id;
        s.versions.insert(vid, v);
        let mut doc = Document::new(
            vid,
            "T".into(),
            "scan.pdf".into(),
            1,
            "h".into(),
            "k".into(),
        );
        doc.parse_status = crate::ParseStatus::Processing;
        let did = doc.id;
        s.documents.insert(did, doc);
        set_pending(&mut s, did, 1);
        assert!(
            process_image_with(&mut s, did, "images/p1.jpg", "scanned_pdf", true, true).is_err()
        );
        assert!(!s.chunks.values().any(|c| c.chunk_type == "image_ocr"));
    }

    #[test]
    fn image_parent_is_text_chunk_that_contains_the_key() {
        let mut s = Store::default();
        let v = ProductVersion::new(Uuid::new_v4(), "v1".into());
        let vid = v.id;
        s.versions.insert(vid, v);
        let mut doc = Document::new(vid, "T".into(), "g.md".into(), 1, "h".into(), "k".into());
        doc.parse_status = crate::ParseStatus::Processing;
        let did = doc.id;
        s.documents.insert(did, doc);
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        s.chunks.insert(
            first,
            Chunk {
                id: first,
                document_id: did,
                product_version_id: vid,
                chunk_type: "text".into(),
                content: "intro without image".into(),
                context_header: String::new(),
                start_at: 0,
                end_at: 19,
                parent_chunk_id: None,
                generated_questions: Vec::new(),
            },
        );
        s.chunks.insert(
            second,
            Chunk {
                id: second,
                document_id: did,
                product_version_id: vid,
                chunk_type: "text".into(),
                content: "See ![p](images/p1.jpg) here".into(),
                context_header: String::new(),
                start_at: 20,
                end_at: 48,
                parent_chunk_id: None,
                generated_questions: Vec::new(),
            },
        );
        set_pending(&mut s, did, 1);
        assert_eq!(parent_text_chunk(&s, did, "images/p1.jpg"), Some(second));
    }

    #[test]
    fn superseded_summary_does_not_finalize() {
        let mut s = Store::default();
        let v = ProductVersion::new(Uuid::new_v4(), "v1".into());
        let vid = v.id;
        s.versions.insert(vid, v);
        let mut doc = Document::new(vid, "T".into(), "a.txt".into(), 1, "h".into(), "k".into());
        doc.attempt = 2;
        doc.parse_status = crate::ParseStatus::Finalizing;
        doc.pending_subtasks_count = 1;
        let did = doc.id;
        s.documents.insert(did, doc);
        generate_summary_for_attempt(&mut s, did, 1).unwrap();
        assert_eq!(s.documents[&did].pending_subtasks_count, 1);
        assert!(s.documents[&did].description.is_empty());
    }

    #[test]
    fn insufficient_image_only_body_fails_without_llm() {
        let mut s = Store::default();
        let v = ProductVersion::new(Uuid::new_v4(), "v1".into());
        let vid = v.id;
        s.versions.insert(vid, v);
        let mut doc = Document::new(vid, "T".into(), "a.txt".into(), 1, "h".into(), "k".into());
        doc.parse_status = crate::ParseStatus::Finalizing;
        doc.pending_subtasks_count = 1;
        let did = doc.id;
        s.documents.insert(did, doc);
        s.chunks.insert(
            did,
            Chunk {
                id: did,
                document_id: did,
                product_version_id: vid,
                chunk_type: "text".into(),
                content: "![p](images/x.png)".into(),
                context_header: String::new(),
                start_at: 0,
                end_at: 18,
                parent_chunk_id: None,
                generated_questions: Vec::new(),
            },
        );
        generate_summary_for_attempt(&mut s, did, 0).unwrap();
        assert_eq!(s.documents[&did].summary_status, SummaryStatus::Failed);
        assert!(s.documents[&did].description.is_empty());
        assert_eq!(s.documents[&did].pending_subtasks_count, 0);
    }

    #[test]
    fn markdown_image_keys_include_objects() {
        let keys =
            markdown_image_keys("![a](images/a.png) ![b](objects/abc) ![c](https://x/c.png)");
        assert_eq!(keys, vec!["images/a.png", "objects/abc", "https://x/c.png"]);
    }

    #[test]
    fn generate_questions_uses_version_count() {
        let mut s = Store::default();
        let mut v = ProductVersion::new(Uuid::new_v4(), "v1".into());
        v.question_count = 5;
        let vid = v.id;
        s.versions.insert(vid, v);
        let mut doc = Document::new(vid, "T".into(), "a.txt".into(), 1, "h".into(), "k".into());
        doc.parse_status = crate::ParseStatus::Finalizing;
        doc.pending_subtasks_count = 1;
        let did = doc.id;
        s.documents.insert(did, doc);
        let cid = Uuid::new_v4();
        s.chunks.insert(
            cid,
            Chunk {
                id: cid,
                document_id: did,
                product_version_id: vid,
                chunk_type: "text".into(),
                content: "line one\nline two\nline three\nline four\nline five\nline six".into(),
                context_header: String::new(),
                start_at: 0,
                end_at: 20,
                parent_chunk_id: None,
                generated_questions: Vec::new(),
            },
        );
        generate_questions(&mut s, &[cid], did);
        let qs = &s.chunks[&cid].generated_questions;
        assert_eq!(qs.len(), 5, "{qs:?}");
        assert_eq!(s.documents[&did].pending_subtasks_count, 0);
    }

    #[test]
    fn superseded_question_does_not_finalize() {
        let mut s = Store::default();
        let v = ProductVersion::new(Uuid::new_v4(), "v1".into());
        let vid = v.id;
        s.versions.insert(vid, v);
        let mut doc = Document::new(vid, "T".into(), "a.txt".into(), 1, "h".into(), "k".into());
        doc.attempt = 2;
        doc.parse_status = crate::ParseStatus::Finalizing;
        doc.pending_subtasks_count = 1;
        let did = doc.id;
        s.documents.insert(did, doc);
        let cid = Uuid::new_v4();
        s.chunks.insert(
            cid,
            Chunk {
                id: cid,
                document_id: did,
                product_version_id: vid,
                chunk_type: "text".into(),
                content: "enough text for a question prompt".into(),
                context_header: String::new(),
                start_at: 0,
                end_at: 33,
                parent_chunk_id: None,
                generated_questions: Vec::new(),
            },
        );
        let out = generate_questions_for_attempt(&mut s, &[cid], did, 1).unwrap();
        assert_eq!(out, QuestionOutcome::Superseded);
        assert_eq!(s.documents[&did].pending_subtasks_count, 1);
        assert!(s.chunks[&cid].generated_questions.is_empty());
    }

    #[test]
    fn question_skips_empty_and_non_text() {
        let mut s = Store::default();
        let v = ProductVersion::new(Uuid::new_v4(), "v1".into());
        let vid = v.id;
        s.versions.insert(vid, v);
        let mut doc = Document::new(vid, "T".into(), "a.txt".into(), 1, "h".into(), "k".into());
        doc.parse_status = crate::ParseStatus::Finalizing;
        doc.pending_subtasks_count = 1;
        let did = doc.id;
        s.documents.insert(did, doc);
        let empty = Uuid::new_v4();
        let ocr = Uuid::new_v4();
        let text = Uuid::new_v4();
        s.chunks.insert(
            empty,
            Chunk {
                id: empty,
                document_id: did,
                product_version_id: vid,
                chunk_type: "text".into(),
                content: "   ".into(),
                context_header: String::new(),
                start_at: 0,
                end_at: 0,
                parent_chunk_id: None,
                generated_questions: Vec::new(),
            },
        );
        s.chunks.insert(
            ocr,
            Chunk {
                id: ocr,
                document_id: did,
                product_version_id: vid,
                chunk_type: "image_ocr".into(),
                content: "scanned words from a figure".into(),
                context_header: String::new(),
                start_at: 0,
                end_at: 26,
                parent_chunk_id: None,
                generated_questions: Vec::new(),
            },
        );
        s.chunks.insert(
            text,
            Chunk {
                id: text,
                document_id: did,
                product_version_id: vid,
                chunk_type: "text".into(),
                content: "install the switch in the rack before power on".into(),
                context_header: String::new(),
                start_at: 10,
                end_at: 56,
                parent_chunk_id: None,
                generated_questions: Vec::new(),
            },
        );
        generate_questions(&mut s, &[empty, ocr, text], did);
        assert!(s.chunks[&empty].generated_questions.is_empty());
        assert!(s.chunks[&ocr].generated_questions.is_empty());
        assert!(!s.chunks[&text].generated_questions.is_empty());
        assert_eq!(s.documents[&did].pending_subtasks_count, 0);
    }

    #[test]
    fn question_uses_payload_neighbors() {
        let mut s = Store::default();
        let v = ProductVersion::new(Uuid::new_v4(), "v1".into());
        let vid = v.id;
        s.versions.insert(vid, v);
        let mut doc = Document::new(vid, "T".into(), "a.txt".into(), 1, "h".into(), "k".into());
        doc.parse_status = crate::ParseStatus::Finalizing;
        doc.pending_subtasks_count = 1;
        let did = doc.id;
        s.documents.insert(did, doc);
        let prev = Uuid::new_v4();
        let mid = Uuid::new_v4();
        let next = Uuid::new_v4();
        for (id, body, start) in [
            (prev, "preceding chapter about fabric topology", 0),
            (mid, "main content about installing the line card", 40),
            (next, "following chapter about power verification", 90),
        ] {
            s.chunks.insert(
                id,
                Chunk {
                    id,
                    document_id: did,
                    product_version_id: vid,
                    chunk_type: "text".into(),
                    content: body.into(),
                    context_header: String::new(),
                    start_at: start,
                    end_at: start + body.chars().count() as i32,
                    parent_chunk_id: None,
                    generated_questions: Vec::new(),
                },
            );
        }
        let hinted_prev = neighbor_content(&s, Some(prev), &s.chunks[&mid], true);
        let hinted_next = neighbor_content(&s, Some(next), &s.chunks[&mid], false);
        assert!(hinted_prev.contains("fabric topology"));
        assert!(hinted_next.contains("power verification"));
        generate_questions_with(&mut s, &[mid], &[Some(prev)], &[Some(next)], did, 0).unwrap();
        assert!(!s.chunks[&mid].generated_questions.is_empty());
        let kids: Vec<_> = s
            .chunks
            .values()
            .filter(|c| c.parent_chunk_id == Some(mid))
            .collect();
        assert_eq!(kids.len(), s.chunks[&mid].generated_questions.len());
        assert!(kids.iter().all(|c| c.chunk_type == "question"));
        assert_eq!(s.documents[&did].pending_subtasks_count, 0);
    }
}
