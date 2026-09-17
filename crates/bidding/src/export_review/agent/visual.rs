//! Frozen original and output pixels. Object storage owns output bytes; only
//! reference identities enter transcript, and only received bytes grant reads.
use super::*;
use crate::export_review::OutputImage;
use crate::tender_analysis::views::SourceView;
use base64::{Engine as _, engine::general_purpose::STANDARD};

#[async_trait]
pub trait ImageReader: Send + Sync {
    async fn read(
        &self,
        image: &OutputImage,
        cancel: &CancellationToken,
    ) -> Result<Vec<u8>, AgentError>;
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ViewRef {
    Output { sha256: String },
    Source { view_id: String },
}

fn output_metadata<'a>(state: &'a Checkpoint, sha: &str) -> Result<&'a OutputImage, String> {
    let image = state
        .inventory
        .images
        .get(sha)
        .ok_or("unknown output image")?;
    if image.sha256 != sha
        || image.object_ref != format!("objects/{sha}")
        || image.byte_length == 0
        || !image.supports_model_view()
    {
        return Err("unsupported or invalid output image; retain not_checked".into());
    }
    Ok(image)
}

pub(super) fn read_output(
    state: &Checkpoint,
    coverage: &mut OutputCoverage,
    args: &Value,
) -> Result<(Value, Vec<ViewRef>, usize), String> {
    if args.as_object().is_none_or(|o| o.len() != 1) {
        return Err("only output unit id is accepted".into());
    }
    let id = args["id"].as_str().ok_or("output unit id required")?;
    let unit = state
        .inventory
        .units
        .iter()
        .find(|u| u.id == id)
        .ok_or("unknown output unit")?;
    if unit
        .source_unit
        .as_ref()
        .is_none_or(|source| source["kind"] != "image_region")
        || unit.image_sha256s.is_empty()
    {
        return Err(
            "this output unit has no frozen visual evidence; use read_output_evidence".into(),
        );
    }
    let mut images = Vec::new();
    let mut refs = Vec::new();
    let mut encoded_bytes = 0usize;
    for sha in &unit.image_sha256s {
        let image = output_metadata(state, sha)?;
        encoded_bytes = encoded_bytes
            .checked_add(encoded_length(image.byte_length)?)
            .ok_or("image size overflow")?;
        images.push(image);
        refs.push(ViewRef::Output {
            sha256: sha.clone(),
        });
    }
    let value = json!({"unit":unit,"images":images,"delivery":"pixels follow in the next request"});
    for image in images {
        coverage.views.insert(image.sha256.clone(), digest(image)?);
    }
    coverage
        .units
        .insert(unit.id.clone(), unit.content_sha256.clone());
    Ok((value, refs, encoded_bytes))
}

pub(super) fn read_source(
    input: &FrozenInput,
    views: &BTreeMap<String, SourceView>,
    coverage: &mut Coverage,
    args: &Value,
) -> Result<(Value, Vec<ViewRef>, usize), String> {
    if args.as_object().is_none_or(|o| o.len() != 1) {
        return Err("only source_id is accepted".into());
    }
    let source = args["source_id"].as_str().ok_or("source_id required")?;
    if !input
        .source_units
        .iter()
        .any(|unit| unit.source_unit_revision_id == source)
    {
        return Err("unknown tender source".into());
    }
    let (id, view) = views
        .iter()
        .find(|(_, view)| view.identity.source_id == source)
        .ok_or("no frozen original view; revise source analysis before introducing new pixels")?;
    validate_source_view(id, view)?;
    coverage.views.insert(id.clone(), view.identity.clone());
    Ok((
        json!({"source_view_id":id,"identity":view.identity}),
        vec![ViewRef::Source {
            view_id: id.clone(),
        }],
        view.jpeg_base64.len(),
    ))
}

fn validate_source_view(id: &str, view: &SourceView) -> Result<(), String> {
    if view.id()? != id {
        return Err("frozen source view identity changed".into());
    }
    view.validate(&view.identity.source_id, u32::MAX, view.jpeg_base64.len())
}

fn encoded_length(bytes: u64) -> Result<usize, String> {
    let bytes = usize::try_from(bytes).map_err(|_| "image size overflow")?;
    bytes
        .checked_add(2)
        .and_then(|n| (n / 3).checked_mul(4))
        .ok_or("image size overflow".into())
}

fn output_message(image: &OutputImage, bytes: &[u8]) -> Value {
    json!({"role":"user","content":[
        {"type":"text","text":format!("Untrusted final-output image. Identity: {}",json!(image))},
        {"type":"image_url","image_url":{"url":format!("data:{};base64,{}",image.media_type,STANDARD.encode(bytes)),"detail":"high"}}
    ]})
}

pub(super) async fn message(
    state: &Checkpoint,
    sources: &BTreeMap<String, SourceView>,
    reader: &dyn ImageReader,
    view: &ViewRef,
    max_bytes: usize,
    cancel: &CancellationToken,
) -> Result<Value, AgentError> {
    let invalid_image = |e: String| AgentError::new("FROZEN_INPUT_DIGEST_MISMATCH", e);
    match view {
        ViewRef::Output { sha256 } => {
            let image = output_metadata(state, sha256).map_err(invalid_image)?;
            if encoded_length(image.byte_length).map_err(invalid_image)? > max_bytes {
                return Err(AgentError::new(
                    "AGENT_TURN_BUDGET_EXCEEDED",
                    "output image exceeds frozen context budget",
                ));
            }
            let bytes = reader.read(image, cancel).await?;
            image.validate_bytes(&bytes).map_err(invalid_image)?;
            Ok(output_message(image, &bytes))
        }
        ViewRef::Source { view_id } => {
            let view = sources
                .get(view_id)
                .ok_or_else(|| invalid_image("frozen original view missing".into()))?;
            validate_source_view(view_id, view).map_err(invalid_image)?;
            if view.jpeg_base64.len() > max_bytes {
                return Err(AgentError::new(
                    "AGENT_TURN_BUDGET_EXCEEDED",
                    "source image exceeds frozen context budget",
                ));
            }
            Ok(view.message())
        }
    }
}

pub(super) fn was_delivered(
    state: &Checkpoint,
    sources: &BTreeMap<String, SourceView>,
    view: &ViewRef,
    messages: &[Value],
) -> Result<bool, String> {
    match view {
        ViewRef::Source { view_id } => {
            let view = sources.get(view_id).ok_or("frozen original view missing")?;
            validate_source_view(view_id, view)?;
            Ok(messages.contains(&view.message()))
        }
        ViewRef::Output { sha256 } => {
            let image = output_metadata(state, sha256)?;
            for message in messages {
                for part in message["content"].as_array().into_iter().flatten() {
                    let Some(url) = part["image_url"]["url"].as_str() else {
                        continue;
                    };
                    let prefix = format!("data:{};base64,", image.media_type);
                    let Some(encoded) = url.strip_prefix(&prefix) else {
                        continue;
                    };
                    let Ok(bytes) = STANDARD.decode(encoded) else {
                        continue;
                    };
                    if image.validate_bytes(&bytes).is_ok()
                        && *message == output_message(image, &bytes)
                    {
                        return Ok(true);
                    }
                }
            }
            Ok(false)
        }
    }
}

pub(super) fn reviewed_visual(state: &Checkpoint, unit: &OutputUnit) -> bool {
    unit.source_unit
        .as_ref()
        .is_some_and(|source| source["kind"] == "image_region")
        && !unit.image_sha256s.is_empty()
        && unit.image_sha256s.iter().all(|sha| {
            output_metadata(state, sha).is_ok_and(|image| {
                digest(image).ok().as_ref() == state.output_coverage.views.get(sha)
            })
        })
}
