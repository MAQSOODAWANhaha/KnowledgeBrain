//! processChunks: write chunk rows, then vector/tsv for text (not parent_text).

use chrono::Utc;
use domain::{Chunk, ChunkEmbedding, ParseStatus, Store, SummaryStatus};
use models::EMBEDDING_DIM;
use uuid::Uuid;

pub fn embedding_http_configured() -> bool {
    !std::env::var("KNOWLEDGEBRAIN_EMBEDDING_BASE_URL")
        .unwrap_or_default()
        .trim()
        .is_empty()
}

/// Search / taxonomy: HTTP when configured, else hashed stub. HTTP errors fall back.
pub fn embed(text: &str) -> Vec<f32> {
    if let Ok(v) = embed_http(text, "")
        && v.len() == EMBEDDING_DIM
    {
        return v;
    }
    stub_embed(text)
}

/// processChunks: configured HTTP must succeed; missing URL stays stub.
pub fn embed_index(text: &str, model_id: &str) -> Result<Vec<f32>, String> {
    if embedding_http_configured() {
        embed_http(text, model_id)
    } else {
        Ok(stub_embed(text))
    }
}

pub fn keep_nonempty_chunks(chunks: Vec<Chunk>) -> Vec<Chunk> {
    chunks
        .into_iter()
        .filter(|c| !c.content.trim().is_empty())
        .collect()
}

/// Deterministic bag-of-tokens vector so the same text scores high against itself.
pub fn stub_embed(text: &str) -> Vec<f32> {
    let mut v = vec![0.0f32; EMBEDDING_DIM];
    for token in tokenize(text) {
        let mut h = 0u64;
        for b in token.bytes() {
            h = h.wrapping_mul(16777619).wrapping_add(b as u64);
        }
        let i = (h as usize) % EMBEDDING_DIM;
        v[i] += 1.0;
    }
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        for x in &mut v {
            *x /= n;
        }
    }
    v
}

pub fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

pub fn cosine(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f64;
    let mut na = 0.0f64;
    let mut nb = 0.0f64;
    for i in 0..a.len() {
        dot += a[i] as f64 * b[i] as f64;
        na += a[i] as f64 * a[i] as f64;
        nb += b[i] as f64 * b[i] as f64;
    }
    let d = na.sqrt() * nb.sqrt();
    if d == 0.0 { 0.0 } else { dot / d }
}

pub fn keyword_score(query: &str, content: &str) -> f64 {
    let q: Vec<_> = tokenize(query);
    if q.is_empty() {
        return 0.0;
    }
    let doc = tokenize(content);
    if doc.is_empty() {
        return 0.0;
    }
    let hits = q.iter().filter(|t| doc.contains(t)).count();
    hits as f64 / q.len() as f64
}

/// Brain processChunks + finalizeIndexedKnowledgeState.
pub fn process_chunks(
    store: &mut Store,
    document_id: Uuid,
    chunks: Vec<Chunk>,
    has_multimodal: bool,
) -> Result<(), String> {
    let Some(doc) = store.documents.get(&document_id).cloned() else {
        return Err("document missing".into());
    };
    if doc.parse_status.is_aborted() {
        return Ok(());
    }
    let Some(version) = store.versions.get(&doc.product_version_id).cloned() else {
        return Err("version missing".into());
    };
    store.clear_document_index(document_id);
    if store
        .documents
        .get(&document_id)
        .is_some_and(|d| d.parse_status.is_aborted())
    {
        return Ok(());
    }
    let title = doc.title.clone();
    let model_id = version.embedding_model_id.clone();
    let needs = version.needs_embedding();
    let chunks = keep_nonempty_chunks(chunks);
    let mut pending = Vec::new();
    let mut text_count = 0usize;
    for ch in &chunks {
        if ch.chunk_type == "text" {
            text_count += 1;
        }
        if ch.chunk_type == "parent_text" || !needs {
            continue;
        }
        if !matches!(
            ch.chunk_type.as_str(),
            "text" | "image_ocr" | "image_caption" | "summary" | "wiki_page" | "question"
        ) {
            continue;
        }
        let content = ch.index_content(&title);
        let vector = if version.vector_enabled {
            embed_index(&content, &model_id)?
        } else {
            Vec::new()
        };
        let tsv = if version.keyword_enabled {
            tokenize(&content).join(" ")
        } else {
            String::new()
        };
        pending.push(ChunkEmbedding {
            chunk_id: ch.id,
            product_version_id: doc.product_version_id,
            document_id,
            content,
            vector,
            tsv,
        });
    }
    if store
        .documents
        .get(&document_id)
        .is_some_and(|d| d.parse_status.is_aborted())
    {
        return Ok(());
    }
    for ch in chunks {
        store.chunks.insert(ch.id, ch);
    }
    for emb in pending {
        store.embeddings.insert(emb.chunk_id, emb);
    }
    if let Some(d) = store.documents.get_mut(&document_id) {
        d.enable_status = "enabled".into();
        d.processed_at = Some(Utc::now());
        d.summary_status = SummaryStatus::None;
        if text_count == 0 && !has_multimodal {
            d.parse_status = ParseStatus::Completed;
        }
    }
    Ok(())
}

pub fn index_chunks(
    chunks: &[Chunk],
    title: &str,
    vector_on: bool,
    keyword_on: bool,
    model_id: &str,
) -> Result<Vec<ChunkEmbedding>, String> {
    if !vector_on && !keyword_on {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for ch in chunks {
        if ch.chunk_type == "parent_text" {
            continue;
        }
        if ch.content.trim().is_empty() {
            continue;
        }
        if !matches!(
            ch.chunk_type.as_str(),
            "text" | "image_ocr" | "image_caption" | "summary" | "wiki_page" | "question"
        ) {
            continue;
        }
        let content = ch.index_content(title);
        out.push(ChunkEmbedding {
            chunk_id: ch.id,
            product_version_id: ch.product_version_id,
            document_id: ch.document_id,
            content: content.clone(),
            vector: if vector_on {
                embed_index(&content, model_id)?
            } else {
                Vec::new()
            },
            tsv: if keyword_on {
                tokenize(&content).join(" ")
            } else {
                String::new()
            },
        });
    }
    Ok(out)
}

pub fn index_one(
    store: &mut Store,
    chunk: &Chunk,
    title: &str,
    vector_on: bool,
    keyword_on: bool,
) -> Result<(), String> {
    if chunk.chunk_type == "parent_text" {
        return Ok(());
    }
    let content = chunk.index_content(title);
    let model = store
        .versions
        .get(&chunk.product_version_id)
        .map(|v| v.embedding_model_id.clone())
        .unwrap_or_default();
    let vector = if vector_on {
        embed_index(&content, &model)?
    } else {
        Vec::new()
    };
    store.embeddings.insert(
        chunk.id,
        ChunkEmbedding {
            chunk_id: chunk.id,
            product_version_id: chunk.product_version_id,
            document_id: chunk.document_id,
            content: content.clone(),
            vector,
            tsv: if keyword_on {
                tokenize(&content).join(" ")
            } else {
                String::new()
            },
        },
    );
    Ok(())
}

fn embed_http(text: &str, model_id: &str) -> Result<Vec<f32>, String> {
    let base = std::env::var("KNOWLEDGEBRAIN_EMBEDDING_BASE_URL").unwrap_or_default();
    if base.is_empty() {
        return Err("embedding not configured".into());
    }
    let key = std::env::var("KNOWLEDGEBRAIN_EMBEDDING_API_KEY").unwrap_or_default();
    let model = if !model_id.trim().is_empty() {
        model_id.trim().to_string()
    } else {
        std::env::var("KNOWLEDGEBRAIN_EMBEDDING_MODEL").unwrap_or_else(|_| "stub-emb".into())
    };
    let url = embeddings_url(&base);
    let body = serde_json::json!({
        "model": model,
        "input": text,
        "dimensions": EMBEDDING_DIM,
    });
    let mut req = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?
        .post(url)
        .json(&body);
    if !key.is_empty() {
        req = req.bearer_auth(key);
    }
    let resp = req.send().map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("embed failed: {}", resp.status()));
    }
    let v: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
    let arr = v["data"][0]["embedding"]
        .as_array()
        .ok_or_else(|| "embed missing vector".to_string())?;
    let out: Vec<f32> = arr
        .iter()
        .filter_map(|x| x.as_f64().map(|f| f as f32))
        .collect();
    if out.len() != EMBEDDING_DIM {
        return Err(format!("embed dim {} != {EMBEDDING_DIM}", out.len()));
    }
    Ok(out)
}

fn embeddings_url(base: &str) -> String {
    let b = base.trim_end_matches('/');
    if b.ends_with("/v1") {
        format!("{b}/embeddings")
    } else if b.ends_with("/embeddings") {
        b.to_string()
    } else {
        format!("{b}/v1/embeddings")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{Document, ProductVersion};

    #[test]
    fn parent_text_not_vectorized() {
        let mut s = Store::default();
        let v = ProductVersion::new(Uuid::new_v4(), "v1".into());
        let vid = v.id;
        s.versions.insert(vid, v);
        let mut doc = Document::new(
            vid,
            "Title".into(),
            "a.txt".into(),
            1,
            "h".into(),
            "h".into(),
        );
        doc.parse_status = ParseStatus::Processing;
        let did = doc.id;
        s.documents.insert(did, doc);
        let parent = Chunk {
            id: Uuid::new_v4(),
            document_id: did,
            product_version_id: vid,
            chunk_type: "parent_text".into(),
            content: "parent body".into(),
            context_header: String::new(),
            start_at: 0,
            end_at: 11,
            parent_chunk_id: None,
            generated_questions: Vec::new(),
        };
        let child = Chunk {
            id: Uuid::new_v4(),
            document_id: did,
            product_version_id: vid,
            chunk_type: "text".into(),
            content: "child body".into(),
            context_header: "H".into(),
            start_at: 0,
            end_at: 10,
            parent_chunk_id: Some(parent.id),
            generated_questions: Vec::new(),
        };
        process_chunks(&mut s, did, vec![parent.clone(), child.clone()], false).unwrap();
        assert!(!s.embeddings.contains_key(&parent.id));
        let emb = &s.embeddings[&child.id];
        assert!(emb.content.starts_with("Title\n"));
        assert!(emb.content.contains("H\n\nchild body"));
        assert_eq!(s.documents[&did].enable_status, "enabled");
        assert_eq!(s.documents[&did].summary_status, SummaryStatus::None);
    }

    #[test]
    fn skips_blank_chunks_and_completes_without_text() {
        let mut s = Store::default();
        let v = ProductVersion::new(Uuid::new_v4(), "v1".into());
        let vid = v.id;
        s.versions.insert(vid, v);
        let mut doc = Document::new(vid, "T".into(), "a.txt".into(), 1, "h".into(), "h".into());
        doc.parse_status = ParseStatus::Processing;
        doc.summary_status = SummaryStatus::Pending;
        let did = doc.id;
        s.documents.insert(did, doc);
        let blank = Chunk {
            id: Uuid::new_v4(),
            document_id: did,
            product_version_id: vid,
            chunk_type: "text".into(),
            content: "   \n".into(),
            context_header: String::new(),
            start_at: 0,
            end_at: 0,
            parent_chunk_id: None,
            generated_questions: Vec::new(),
        };
        process_chunks(&mut s, did, vec![blank], false).unwrap();
        assert!(s.chunks.is_empty());
        assert!(s.embeddings.is_empty());
        assert_eq!(s.documents[&did].parse_status, ParseStatus::Completed);
        assert_eq!(s.documents[&did].summary_status, SummaryStatus::None);
    }

    #[test]
    fn embed_index_without_url_is_stub() {
        let v = embed_index("throughput 40gbps", "stub-emb").unwrap();
        assert_eq!(v, stub_embed("throughput 40gbps"));
    }

    #[test]
    fn index_one_writes_vector_from_embed_index() {
        let mut s = Store::default();
        let v = ProductVersion::new(Uuid::new_v4(), "v1".into());
        let vid = v.id;
        s.versions.insert(vid, v);
        let did = Uuid::new_v4();
        let ch = Chunk {
            id: Uuid::new_v4(),
            document_id: did,
            product_version_id: vid,
            chunk_type: "text".into(),
            content: "throughput 40gbps".into(),
            context_header: String::new(),
            start_at: 0,
            end_at: 17,
            parent_chunk_id: None,
            generated_questions: Vec::new(),
        };
        index_one(&mut s, &ch, "T", true, true).unwrap();
        let emb = &s.embeddings[&ch.id];
        assert_eq!(emb.vector, stub_embed(&ch.index_content("T")));
        assert!(!emb.tsv.is_empty());
    }

    #[test]
    fn self_cosine_is_one() {
        let v = stub_embed("throughput 40gbps");
        assert_eq!(v.len(), EMBEDDING_DIM);
        assert!((cosine(&v, &v) - 1.0).abs() < 1e-5);
        let via = embed("throughput 40gbps");
        assert_eq!(via, v);
    }
}
