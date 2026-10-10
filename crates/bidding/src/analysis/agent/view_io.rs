use super::*;

pub(super) async fn read_source_view<J: Journal>(
    input: &FrozenInput,
    config: &Config,
    state: &mut Checkpoint,
    journal: &J,
    args: &Value,
    cancel: &CancellationToken,
) -> Result<Value, AgentError> {
    if !config.limits.vision_enabled {
        return Err(error(
            "AGENT_VISION_NOT_CONFIGURED",
            "original-image review requires vision_enabled=true and an image-capable configured authoring model; no alternative provider was selected",
        ));
    }
    let source_id = args["source_id"]
        .as_str()
        .ok_or_else(|| invalid("source_id required"))?;
    if args.as_object().is_none_or(|a| a.len() != 1)
        || !input
            .source_units
            .iter()
            .any(|s| s.source_unit_revision_id == source_id)
    {
        return Err(invalid(
            "source view must name a source in this frozen collection",
        ));
    }
    let coverage = &mut state.analysis.coverage;
    coverage
        .view_failures
        .insert(source_id.into(), "SOURCE_VIEW_NOT_DELIVERED".into());
    let cached = state
        .source_views
        .values()
        .find(|v| v.identity.source_id == source_id)
        .cloned();
    let result = match cached {
        Some(view) => Ok(view),
        None if input.source_units.iter().any(|source| {
            source.source_unit_revision_id == source_id && source.locator["image_available"] == true
        }) =>
        {
            crate::outline::visual::load_frozen_image_view(
                input,
                source_id,
                config.limits.max_source_view_edge,
                config.limits.max_source_view_bytes,
                cancel,
            )
            .await
            .map_err(|message| error("SOURCE_VIEW_UNAVAILABLE", message))
        }
        None => tokio::select! {
            biased;
            _=cancel.cancelled()=>return Err(error("INTERNAL","source view cancelled")),
            result=journal.source_view(source_id,&config.limits,cancel)=>result,
        },
    };
    let view = match result {
        Ok(view) => view,
        Err(e) => {
            let coverage = &mut state.analysis.coverage;
            coverage
                .view_failures
                .insert(source_id.into(), e.code.clone());
            return Err(e);
        }
    };
    view.validate(
        source_id,
        config.limits.max_source_view_edge,
        config.limits.max_source_view_bytes,
    )
    .map_err(invalid)?;
    let id = view.id().map_err(invalid)?;
    let out = json!({"view_id":id,"identity":view.identity,"citation":{"source_id":source_id,"start":0,"end":0,"view_id":id},
        "note":"The original image follows as a separate image message. It does not count as reading parsed text or grid cells."});
    state.read_bytes += view.jpeg_base64.len();
    let coverage = &mut state.analysis.coverage;
    coverage.view_failures.remove(source_id);
    coverage.views.insert(id.clone(), view.identity.clone());
    state.source_views.insert(id, view);
    Ok(out)
}
