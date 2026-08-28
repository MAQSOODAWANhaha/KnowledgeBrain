//! Shared request identity and mutation context for V2 bidding commands.

use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
pub struct RequestIdentity {
    pub bytes: Vec<u8>,
    pub sha256: String,
}

impl RequestIdentity {
    pub fn canonical<T: Serialize>(payload: &T) -> Result<Self, serde_json::Error> {
        let bytes = serde_json::to_vec(payload)?;
        let sha256 = hex::encode(Sha256::digest(&bytes));
        Ok(Self { bytes, sha256 })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MutationContext {
    pub actor: String,
    pub idempotency_key: String,
    #[serde(skip)]
    pub request: RequestIdentity,
}

impl MutationContext {
    pub fn new<T: Serialize>(
        actor: impl Into<String>,
        idempotency_key: impl Into<String>,
        payload: &T,
    ) -> Result<Self, serde_json::Error> {
        Ok(Self {
            actor: actor.into(),
            idempotency_key: idempotency_key.into(),
            request: RequestIdentity::canonical(payload)?,
        })
    }
}
