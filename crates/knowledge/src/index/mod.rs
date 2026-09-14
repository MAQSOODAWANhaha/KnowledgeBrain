//! processChunks: write chunk rows, then vector/tsv for text (not parent_text).

use crate::models::EMBEDDING_DIM;
use crate::{Chunk, ChunkEmbedding};

pub fn embedding_http_configured() -> bool {
    !platform::embedding_base_url().trim().is_empty()
}

/// Empty or leftover `stub-emb` rows are not a model identity.
pub fn unbound_embedding_model(id: &str) -> bool {
    let t = id.trim();
    t.is_empty() || t == "stub-emb"
}

/// The single configured embedding model. Missing config is not a model name.
pub fn live_embedding_model_id() -> Result<String, String> {
    let model = platform::embedding_model();
    if model.trim().is_empty() {
        return Err("embedding model identity is empty".into());
    }
    Ok(model.trim().to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbeddingIdentity {
    Use(String),
    Bind(String),
}

/// Decide how to treat a version's stored embedding model id.
pub fn embedding_identity_plan(
    stored: &str,
    has_embeddings: bool,
    v3_model: Option<&str>,
) -> Result<EmbeddingIdentity, String> {
    let live = live_embedding_model_id()?;
    if let Some(v3) = v3_model.map(str::trim).filter(|s| !s.is_empty())
        && v3 != live
    {
        return Err(format!(
            "embedding model conflict: v3 binding is {v3}, environment is {live}"
        ));
    }
    let stored = stored.trim();
    if unbound_embedding_model(stored) {
        if has_embeddings {
            return Err(format!(
                "version already has embeddings under unknown identity; refusing to mix with {live}"
            ));
        }
        return Ok(EmbeddingIdentity::Bind(live));
    }
    if stored != live {
        return Err(format!(
            "embedding model conflict: version frozen as {stored}, environment is {live}"
        ));
    }
    Ok(EmbeddingIdentity::Use(stored.to_string()))
}

/// Query path: never substitute live for an unbound version.
pub fn query_embedding_model_id(stored: &str) -> Result<String, String> {
    if !embedding_http_configured() {
        return Ok(stored.trim().to_string());
    }
    if unbound_embedding_model(stored) {
        return Err(
            "version embedding identity is unbound; freeze it before querying HTTP embeddings"
                .into(),
        );
    }
    match embedding_identity_plan(stored, false, None)? {
        EmbeddingIdentity::Use(id) | EmbeddingIdentity::Bind(id) => Ok(id),
    }
}

/// Search / taxonomy: HTTP when configured, else hashed stub. HTTP errors fall back.
pub fn embed(text: &str) -> Vec<f32> {
    if embedding_http_configured()
        && let Ok(v) = embed_http(text)
        && v.len() == EMBEDDING_DIM
    {
        return v;
    }
    stub_embed(text)
}

/// processChunks: configured HTTP must succeed with the live model; missing URL stays stub.
pub fn embed_index(text: &str) -> Result<Vec<f32>, String> {
    if embedding_http_configured() {
        embed_http(text)
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

pub const KEYWORD_TOKENIZER_V2: &str = crate::knowledge_retrieval::RETRIEVAL_KEYWORD_TOKENIZER_V2;
pub const KEYWORD_TOKENIZER_VERSION_V2: &str =
    crate::knowledge_retrieval::RETRIEVAL_KEYWORD_TOKENIZER_VERSION_V2;

fn is_cjk_ideograph_v2(character: char) -> bool {
    matches!(
        character as u32,
        0x3400..=0x4dbf
            | 0x4e00..=0x9fff
            | 0xf900..=0xfaff
            | 0x20000..=0x2ebef
            | 0x2ebf0..=0x2ee5f
            | 0x2f800..=0x2fa1f
            | 0x30000..=0x323af
    )
}

/// Tokenizes the fixed V2 keyword policy without changing the legacy tokenizer.
pub fn keyword_tokens_v2(text: &str) -> Vec<String> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum RunKind {
        Ascii,
        Cjk,
    }

    fn flush(tokens: &mut Vec<String>, kind: &mut Option<RunKind>, run: &mut String) {
        match *kind {
            Some(RunKind::Ascii) => tokens.push(std::mem::take(run)),
            Some(RunKind::Cjk) => {
                let characters: Vec<char> = run.chars().collect();
                if characters.len() == 1 {
                    tokens.push(std::mem::take(run));
                } else {
                    tokens.extend(
                        characters
                            .windows(2)
                            .map(|pair| pair.iter().collect::<String>()),
                    );
                    run.clear();
                }
            }
            None => {}
        }
        *kind = None;
    }

    let mut tokens = Vec::new();
    let mut kind = None;
    let mut run = String::new();
    for character in text.chars() {
        let next_kind = if character.is_ascii_alphanumeric() {
            Some(RunKind::Ascii)
        } else if is_cjk_ideograph_v2(character) {
            Some(RunKind::Cjk)
        } else {
            None
        };
        if next_kind != kind {
            flush(&mut tokens, &mut kind, &mut run);
        }
        match next_kind {
            Some(RunKind::Ascii) => {
                kind = next_kind;
                run.push(character.to_ascii_lowercase());
            }
            Some(RunKind::Cjk) => {
                kind = next_kind;
                run.push(character);
            }
            None => {}
        }
    }
    flush(&mut tokens, &mut kind, &mut run);
    tokens
}

pub fn keyword_token_stream_v2(text: &str) -> String {
    keyword_tokens_v2(text).join(" ")
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
pub fn index_chunks(
    chunks: &[Chunk],
    title: &str,
    vector_on: bool,
    keyword_on: bool,
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
                embed_index(&content)?
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

pub fn index_one_in(
    embeddings: &mut std::collections::HashMap<uuid::Uuid, crate::ChunkEmbedding>,
    chunk: &Chunk,
    title: &str,
    vector_on: bool,
    keyword_on: bool,
) -> Result<(), String> {
    if chunk.chunk_type == "parent_text" {
        return Ok(());
    }
    let content = chunk.index_content(title);
    let vector = if vector_on {
        embed_index(&content)?
    } else {
        Vec::new()
    };
    embeddings.insert(
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

fn embed_http(text: &str) -> Result<Vec<f32>, String> {
    if tokio::runtime::Handle::try_current().is_ok() {
        tokio::task::block_in_place(|| embed_http_inner(text))
    } else {
        embed_http_inner(text)
    }
}

fn embed_http_inner(text: &str) -> Result<Vec<f32>, String> {
    let base = platform::embedding_base_url();
    if base.is_empty() {
        return Err("embedding not configured".into());
    }
    let key = platform::embedding_api_key();
    let model = live_embedding_model_id()?;
    let url = embeddings_url(&base);
    let body = serde_json::json!({
        "model": model,
        "input": text,
        "dimensions": EMBEDDING_DIM,
    });
    let v = crate::models::json_sse(&url, &key, body, false)?;
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
    use crate::{Chunk, TEST_ENV_LOCK};
    use uuid::Uuid;

    #[test]
    fn keyword_tokenizer_v2_goldens() {
        assert_eq!(KEYWORD_TOKENIZER_V2, "latin-numeric-cjk-bigram");
        assert_eq!(KEYWORD_TOKENIZER_VERSION_V2, "v1");
        for (input, expected) in [
            ("Router42 ABC123", "router42 abc123"),
            ("alpha,beta...gamma", "alpha beta gamma"),
            ("知识大脑", "知识 识大 大脑"),
            ("中", "中"),
            ("A\t B\n\rC", "a b c"),
            ("abc中国XYZ", "abc 中国 xyz"),
            ("café🙂naïve", "caf na ve"),
            ("𠀀𠀁", "𠀀𠀁"),
            ("\u{2ebef}\u{2ebf0}", "\u{2ebef}\u{2ebf0}"),
            ("\u{2ee5f}\u{2ee60}", "\u{2ee5f}"),
            ("A中B", "a 中 b"),
            ("", ""),
        ] {
            assert_eq!(keyword_token_stream_v2(input), expected, "input={input:?}");
        }
        assert_eq!(
            keyword_tokens_v2("repeat repeat 中国中国"),
            ["repeat", "repeat", "中国", "国中", "中国"]
        );
    }

    #[test]
    fn embed_index_without_url_is_stub() {
        let _guard = TEST_ENV_LOCK.lock().unwrap();
        let v = embed_index("throughput 40gbps").unwrap();
        assert_eq!(v, stub_embed("throughput 40gbps"));
    }

    #[test]
    fn index_one_writes_vector_from_embed_index() {
        let _guard = TEST_ENV_LOCK.lock().unwrap();
        let ch = Chunk {
            id: Uuid::new_v4(),
            document_id: Uuid::new_v4(),
            product_version_id: Uuid::new_v4(),
            chunk_type: "text".into(),
            content: "throughput 40gbps".into(),
            context_header: String::new(),
            start_at: 0,
            end_at: 17,
            parent_chunk_id: None,
            generated_questions: Vec::new(),
        };
        let mut embeddings = std::collections::HashMap::new();
        index_one_in(&mut embeddings, &ch, "T", true, true).unwrap();
        let emb = &embeddings[&ch.id];
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

    fn with_live_embedding_env<T>(url: &str, model: &str, f: impl FnOnce() -> T) -> T {
        let _guard = TEST_ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("KNOWLEDGEBRAIN_EMBEDDING_BASE_URL", url);
            std::env::set_var("KNOWLEDGEBRAIN_EMBEDDING_MODEL", model);
            std::env::remove_var("EMBEDDING_BASE_URL");
            std::env::remove_var("EMBEDDING_MODEL");
        }
        let out = f();
        unsafe {
            std::env::remove_var("KNOWLEDGEBRAIN_EMBEDDING_BASE_URL");
            std::env::remove_var("KNOWLEDGEBRAIN_EMBEDDING_MODEL");
        }
        out
    }

    #[test]
    fn identity_plan_binds_unbound_without_vectors() {
        with_live_embedding_env("http://127.0.0.1:9", "live-emb", || {
            assert_eq!(
                embedding_identity_plan("", false, None).unwrap(),
                EmbeddingIdentity::Bind("live-emb".into())
            );
            assert_eq!(
                embedding_identity_plan("stub-emb", false, None).unwrap(),
                EmbeddingIdentity::Bind("live-emb".into())
            );
        });
    }

    #[test]
    fn identity_plan_rejects_unbound_with_vectors() {
        with_live_embedding_env("http://127.0.0.1:9", "live-emb", || {
            let err = embedding_identity_plan("stub-emb", true, None).unwrap_err();
            assert!(err.contains("unknown identity"), "{err}");
        });
    }

    #[test]
    fn identity_plan_rejects_frozen_other_model() {
        with_live_embedding_env("http://127.0.0.1:9", "live-emb", || {
            let err = embedding_identity_plan("other-emb", false, None).unwrap_err();
            assert!(err.contains("frozen as other-emb"), "{err}");
            let err = embedding_identity_plan("live-emb", false, Some("v3-other")).unwrap_err();
            assert!(err.contains("v3 binding"), "{err}");
        });
    }

    #[test]
    fn query_rejects_unbound_when_http_configured() {
        with_live_embedding_env("http://127.0.0.1:9", "live-emb", || {
            assert!(query_embedding_model_id("stub-emb").is_err());
            assert_eq!(query_embedding_model_id("live-emb").unwrap(), "live-emb");
        });
    }
}
