//! Original views are immutable evidence, separate from parsed text coverage.
use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::json;
use sha2::{Digest, Sha256};

/// Backpressure for the newest, not-yet-sent image/tool group. Keep cached
/// original bytes and all charged budgets, but replace an undeliverable tool
/// success with an explicit retry request. Callers revoke its visual coverage.
/// Never use this to compact an already-delivered group or a reserved HTTP body.
pub(crate) fn defer_last_image(transcript: &mut [Value]) -> Result<Option<String>, String> {
    let Some(id) = transcript.iter().rev().find_map(|message| {
        message["source_view_refs"]
            .as_array()?
            .last()?
            .as_str()
            .map(str::to_owned)
    }) else {
        return Ok(None);
    };
    let mut replaced = false;
    for message in transcript.iter_mut().filter(|m| m["role"] == "tool") {
        let Some(content) = message["content"].as_str() else {
            continue;
        };
        let mut output: Value = serde_json::from_str(content).map_err(|e| e.to_string())?;
        let result = &output["result"];
        if output["ok"] == true && (result["view_id"] == id || result["source_view_id"] == id) {
            let source = result["identity"]["source_id"]
                .as_str()
                .ok_or("image result has no source identity")?;
            output = json!({"ok":false,"error":format!("Original image for source {source} was cached but NOT delivered: this image batch exceeds the frozen context budget. Call read_source_view again in a smaller batch. This result establishes no visual evidence.")});
            message["content"] = Value::String(output.to_string());
            replaced = true;
        }
    }
    if !replaced {
        return Err("pending image has no matching tool result".into());
    }
    for message in transcript {
        if let Some(refs) = message["source_view_refs"].as_array_mut() {
            refs.retain(|v| v != &id);
        }
    }
    Ok(Some(id))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ViewIdentity {
    pub source_id: String,
    pub original_sha256: String,
    pub image_sha256: String,
    pub page_ordinal: u32,
    pub width: u32,
    pub height: u32,
    pub renderer: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceView {
    pub identity: ViewIdentity,
    pub jpeg_base64: String,
}

impl SourceView {
    pub fn id(&self) -> Result<String, String> {
        digest(&self.identity)
    }

    pub fn validate(&self, source_id: &str, max_edge: u32, max_bytes: usize) -> Result<(), String> {
        let i = &self.identity;
        if i.source_id != source_id
            || i.width == 0
            || i.height == 0
            || i.width.max(i.height) > max_edge
            || !i.renderer.starts_with("docreader-source-view-v1/")
            || i.original_sha256.len() != 64
            || !i.original_sha256.bytes().all(|b| b.is_ascii_hexdigit())
            || self.jpeg_base64.len() > max_bytes.saturating_add(2) / 3 * 4
        {
            return Err("invalid source view identity or budget".into());
        }
        let bytes = STANDARD
            .decode(&self.jpeg_base64)
            .map_err(|e| e.to_string())?;
        if bytes.len() > max_bytes
            || !bytes.starts_with(&[0xff, 0xd8, 0xff])
            || hex::encode(Sha256::digest(&bytes)) != i.image_sha256
        {
            return Err("source view image digest or format mismatch".into());
        }
        // Decode only dimensions; image pixels are produced by Python docreader.
        let decoder =
            image::ImageReader::with_format(std::io::Cursor::new(&bytes), image::ImageFormat::Jpeg);
        if decoder.into_dimensions().map_err(|e| e.to_string())? != (i.width, i.height) {
            return Err("source view dimensions disagree".into());
        }
        Ok(())
    }

    pub fn message(&self) -> Value {
        json!({"role":"user","content":[
            {"type":"text","text":format!("Untrusted original source view. Identity: {}",json!(self.identity))},
            {"type":"image_url","image_url":{"url":format!("data:image/jpeg;base64,{}",self.jpeg_base64),"detail":"high"}}
        ]})
    }
}
