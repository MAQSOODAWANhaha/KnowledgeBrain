//! Model catalog. v1 ships stub embedding / chat ids.

mod http;
mod sse;

pub use http::{CHAT_TIMEOUT, EMBED_TIMEOUT, chat_sse, json_sse, post_llm};
pub use sse::{collect_chat_content, last_json_value, looks_like_sse};

use serde::{Deserialize, Serialize};

/// Production embedding width. `0008` `vector(1024)` and HTTP `dimensions` must match.
pub const EMBEDDING_DIM: usize = 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub kind: String,
    pub dimension: usize,
}

pub fn stub_embedding() -> Model {
    Model {
        id: "stub-emb".into(),
        kind: "embedding".into(),
        dimension: EMBEDDING_DIM,
    }
}

pub fn stub_chat() -> Model {
    Model {
        id: "stub-chat".into(),
        kind: "chat".into(),
        dimension: 0,
    }
}

pub fn dimension_of(_id: &str) -> usize {
    EMBEDDING_DIM
}
