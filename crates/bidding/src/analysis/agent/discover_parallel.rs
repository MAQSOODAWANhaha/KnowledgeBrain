//! Production adapter for the bounded Discover coordinator.
use super::discover_coordinator::{
    self as coordinator, Host, Pending, Provider, Request, Scope, Worker,
};
use super::*;
use crate::outline::{agent::Duty, discover::PackStatus};
use serde_json::json;
use std::collections::BTreeSet;

#[derive(Debug)]
enum ReadPageFailure {
    Business(String),
    Capacity(String),
    Host(AgentError),
}
impl From<String> for ReadPageFailure {
    fn from(message: String) -> Self {
        Self::Business(message)
    }
}
impl From<&str> for ReadPageFailure {
    fn from(message: &str) -> Self {
        Self::Business(message.into())
    }
}

struct PackProvider<'a, M> {
    config: &'a Config,
    model: &'a M,
}
#[async_trait]
impl<M: Model> Provider for PackProvider<'_, M> {
    async fn call(&self, request: &Request) -> Result<ChatTurn, AgentError> {
        self.model.turn(self.config, &request.body).await
    }
}
struct PackHost<'a, J> {
    input: &'a FrozenInput,
    config: &'a Config,
    state: &'a mut Checkpoint,
    journal: &'a J,
    cancel: &'a CancellationToken,
}
/// Validate immutable request ownership before confirming reads or constructing
/// any selector page. Selectors without source keys obey the same boundary.
fn validate_worker_scope(
    input: &FrozenInput,
    state: &Checkpoint,
    request: &Request,
) -> Result<Value, String> {
    let digest = crate::outline::evidence::input_digest(input)?;
    let identity = state
        .outline_run
        .reading_packs
        .as_ref()
        .ok_or("reading plan missing")?
        .pack_wire_identity(&request.scope.pack)?;
    let worker = state
        .outline_run
        .discover_workers
        .get(&request.scope.pack)
        .ok_or("worker missing")?;
    if state.input_sha256 != digest
        || identity["input"] != digest
        || identity["run"] != request.scope.run
        || identity["revision"] != request.scope.revision
        || worker.generation != request.scope.generation
        || crate::outline::canonical_sha256(&(&identity, request.scope.generation, &request.body))?
            != request.scope.request
    {
        return Err("stale or foreign worker input, claim, generation or request".into());
    }
    Ok(identity)
}

impl<J: Journal> PackHost<'_, J> {
    async fn save(&self) -> Result<(), AgentError> {
        self.journal
            .save(self.state, &self.state.progress(self.input))
            .await
    }
    fn worker(&self, id: &str) -> Result<&Worker, AgentError> {
        self.state
            .outline_run
            .discover_workers
            .get(id)
            .ok_or_else(|| invalid("worker missing"))
    }
    fn worker_mut(&mut self, id: &str) -> Result<&mut Worker, AgentError> {
        self.state
            .outline_run
            .discover_workers
            .get_mut(id)
            .ok_or_else(|| invalid("worker missing"))
    }
    async fn directory_page(
        &mut self,
        request: &Request,
        response: &ChatTurn,
        cursor: &str,
    ) -> Result<Value, ReadPageFailure> {
        validate_worker_scope(self.input, self.state, request)?;
        let id = &request.scope.pack;
        let work = self
            .state
            .outline_run
            .reading_packs
            .as_ref()
            .ok_or("reading plan missing")?;
        let mut page = work.dependency_directory_page(self.input, id, cursor)?;
        let page_id = crate::outline::canonical_sha256(&page)?;
        page["new_coverage"] = json!(
            !self
                .worker(id)
                .map_err(|e| e.message)?
                .delivered_dependency_pages
                .contains(&page_id)
                && !self
                    .worker(id)
                    .map_err(|e| e.message)?
                    .related_read_frames
                    .iter()
                    .any(|frame| frame.directory_page.as_ref() == Some(&page_id))
        );
        let result = json!({"status":"read_pending_delivery","read":page});
        let call = &response.tool_calls[0];
        let frame = coordinator::RelatedReadFrame {
            user_page: None,
            directory_page: Some(page_id),
            call_id: call.id.clone(),
            pack_revision: request.scope.revision,
            input_digest: crate::outline::evidence::input_digest(self.input)?,
            options: vec![],
            sealed_sha256: None,
        };
        let mut candidate = self.state.clone();
        let worker = candidate
            .outline_run
            .discover_workers
            .get_mut(id)
            .ok_or("worker missing")?;
        worker.related_read_frames.push(frame.clone());
        worker.history.push(json!({"role":"assistant","content":response.content,"tool_calls":response.tool_calls.iter().map(|call|json!({"id":call.id,"type":"function","function":{"name":call.name,"arguments":call.arguments}})).collect::<Vec<_>>()}));
        worker.history.push(json!({"role":"tool","tool_call_id":call.id,"content":super::tool_result_envelope(result.clone()).to_string()}));
        PackHost {
            input: self.input,
            config: self.config,
            state: &mut candidate,
            journal: self.journal,
            cancel: self.cancel,
        }
        .prepare_inner(id, false)
        .await
        .map_err(|error| {
            if matches!(error.code.as_str(), "AGENT_TURN_BUDGET_EXCEEDED" | "DISCOVER_READ_CAPACITY_EXCEEDED") {
                ReadPageFailure::Capacity("The indivisible dependency directory page cannot fit the complete worker request. This worker has stopped; its audit and confirmed reads are preserved.".into())
            } else {
                ReadPageFailure::Host(error)
            }
        })?;
        self.worker_mut(id)
            .map_err(|e| e.message)?
            .related_read_frames
            .push(frame);
        Ok(result)
    }

    #[cfg(test)]
    async fn related_page(
        &mut self,
        request: &Request,
        response: &ChatTurn,
        args: &Value,
    ) -> Result<Value, String> {
        self.related_page_typed(request, response, args)
            .await
            .map_err(|failure| match failure {
                ReadPageFailure::Business(message) | ReadPageFailure::Capacity(message) => message,
                ReadPageFailure::Host(error) => error.message,
            })
    }

    async fn related_page_typed(
        &mut self,
        request: &Request,
        response: &ChatTurn,
        args: &Value,
    ) -> Result<Value, ReadPageFailure> {
        use crate::outline::evidence::{self, EvidenceRef};
        validate_worker_scope(self.input, self.state, request)?;
        if let Some(cursor) = args["dependency_cursor"].as_str() {
            return self.directory_page(request, response, cursor).await;
        }
        let id = &request.scope.pack;
        let digest = evidence::input_digest(self.input)?;
        let cursor = args["cursor"].as_str();
        let refs: Vec<EvidenceRef> = if let Some(cursor) = cursor {
            let saved = self
                .worker(id)
                .map_err(|e| e.message)?
                .related_continuations
                .get(cursor)
                .ok_or("unknown worker dependency cursor")?;
            if saved.run != request.scope.run
                || saved.pack != *id
                || saved.pack_revision != request.scope.revision
                || saved.input_digest != digest
            {
                return Err("worker dependency cursor scope changed".into());
            }
            saved.refs.clone()
        } else if let Some(dependency_id) = args["dependency_id"].as_str() {
            self.state
                .outline_run
                .reading_packs
                .as_ref()
                .ok_or("reading plan missing")?
                .dependency_selection(self.input, id, dependency_id)?
        } else {
            serde_json::from_value(args["refs"].clone()).map_err(|e| e.to_string())?
        };
        if refs.is_empty() {
            return Err("dependency selection must not be empty".into());
        }
        // Start with one native range instead of materializing a whole attachment.
        let mut selected = vec![refs[0].clone()];
        loop {
            let tail = evidence::selection_tail(&refs, &selected);
            let continuation = coordinator::RelatedContinuation {
                run: request.scope.run.clone(),
                pack: id.clone(),
                pack_revision: request.scope.revision,
                input_digest: digest.clone(),
                refs: tail.clone(),
            };
            let next = if tail.is_empty() {
                None
            } else {
                Some(format!(
                    "page_{}",
                    crate::outline::canonical_sha256(&continuation)?
                ))
            };
            let work = self
                .state
                .outline_run
                .reading_packs
                .as_ref()
                .ok_or("reading plan missing")?;
            let (mut value, options) = work.related_read(self.input, id, &selected)?;
            let delivered = &self
                .worker(id)
                .map_err(|e| e.message)?
                .delivered_related_evidence;
            let covered_evidence = delivered
                .iter()
                .cloned()
                .chain(
                    self.state.outline_run.discover_workers[id]
                        .related_read_frames
                        .iter()
                        .flat_map(|frame| frame.options.iter())
                        .flat_map(|option| option.evidence.iter().cloned()),
                )
                .collect::<Vec<_>>();
            value["new_coverage"] = json!(
                selected
                    .iter()
                    .any(|reference| !evidence::covered_by_union(reference, &covered_evidence))
            );
            value["next_cursor"] = json!(next);
            value["selection_complete"] = json!(tail.is_empty());
            let result = json!({"status":"read_pending_delivery","read":value});
            let call = &response.tool_calls[0];
            let frame = coordinator::RelatedReadFrame {
                user_page: None,
                directory_page: None,
                call_id: call.id.clone(),
                pack_revision: request.scope.revision,
                input_digest: digest.clone(),
                options,
                sealed_sha256: None,
            };
            let mut candidate = self.state.clone();
            let worker = candidate
                .outline_run
                .discover_workers
                .get_mut(id)
                .ok_or("worker missing")?;
            worker.related_read_frames.push(frame.clone());
            // Probe the exact continuation state the successful commit will
            // expose in dependency_progress.pending_cursors on the next turn.
            if let Some(cursor) = cursor {
                worker.related_continuations.remove(cursor);
            }
            if let Some(next) = &next {
                worker
                    .related_continuations
                    .insert(next.clone(), continuation.clone());
            }

            worker.history.push(json!({"role":"assistant","content":response.content,"tool_calls":response.tool_calls.iter().map(|call|json!({"id":call.id,"type":"function","function":{"name":call.name,"arguments":call.arguments}})).collect::<Vec<_>>()}));
            worker.history.push(json!({"role":"tool","tool_call_id":call.id,"content":super::tool_result_envelope(result.clone()).to_string()}));
            let mut probe = PackHost {
                input: self.input,
                config: self.config,
                state: &mut candidate,
                journal: self.journal,
                cancel: self.cancel,
            };
            match probe.prepare_inner(id, false).await {
                Ok(_) => {
                    let worker = self.worker_mut(id).map_err(|e| e.message)?;
                    worker.related_read_frames.push(frame);
                    if let Some(cursor) = cursor {
                        worker.related_continuations.remove(cursor);
                    }
                    if let Some(next) = next {
                        worker.related_continuations.insert(next, continuation);
                    }
                    return Ok(result);
                }
                Err(error) if matches!(error.code.as_str(), "AGENT_TURN_BUDGET_EXCEEDED" | "DISCOVER_READ_CAPACITY_EXCEEDED") => {
                    evidence::shrink_selection(self.input, &mut selected)
                        .map_err(|_| ReadPageFailure::Capacity(
                            "The minimum UTF-8 source page cannot fit the complete worker request. This worker has stopped; its audit and confirmed reads are preserved.".into()))?
                }
                Err(error) => return Err(ReadPageFailure::Host(error)),
            }
        }
    }

    fn split_for_capacity(&mut self, id: &str, limit: usize) -> Result<(), AgentError> {
        // Work on a clone: an indivisible source or invalid budget leaves every
        // old claim, completed read, and reservation untouched.
        if limit
            <= self.config.provider.output_token_reserve as usize
                + self.config.limits.token_safety_margin
        {
            return Err(error(
                "DISCOVER_READ_CAPACITY_EXCEEDED",
                "Reduced context cannot contain request reserves; no retry.",
            ));
        }
        let mut candidate = self.state.clone();
        let children = candidate
            .outline_run
            .reading_packs
            .as_mut()
            .ok_or_else(|| invalid("reading plan missing"))?
            .split_rejected_pack(self.input, id)
            .map_err(|_| {
                error(
                    "DISCOVER_READ_CAPACITY_EXCEEDED",
                    "Rejected pack cannot be split without losing original evidence; no retry.",
                )
            })?;
        for child in children {
            candidate.outline_run.discover_workers.insert(
                child,
                Worker {
                    reduced_context_tokens: Some(limit),
                    ..Worker::default()
                },
            );
        }
        // Parent workers retain all delivered evidence, history and rejected
        // physical requests. Children claim no inherited credit and reread their
        // exact originals; successful unrelated packs are never replanned.
        *self.state = candidate;
        Ok(())
    }

    async fn prepare(&mut self, id: &str) -> Result<Request, AgentError> {
        self.prepare_inner(id, true).await
    }
    async fn prepare_inner(&mut self, id: &str, persist: bool) -> Result<Request, AgentError> {
        let base = self.state.clone();
        let work = base
            .outline_run
            .reading_packs
            .as_ref()
            .ok_or_else(|| invalid("reading plan missing"))?;
        let missing = work
            .missing_required_support(self.input, id)
            .map_err(invalid)?;
        let has_original_frame = self
            .worker(id)?
            .related_read_frames
            .iter()
            .any(|frame| !frame.options.is_empty());
        let mut selected = if has_original_frame {
            vec![]
        } else {
            missing.into_iter().take(1).collect::<Vec<_>>()
        };
        loop {
            let mut candidate = base.clone();
            if !selected.is_empty() {
                let (page, options) = crate::outline::discover::automatic_support_page(
                    self.input,
                    work.canonical_pack(id)
                        .ok_or_else(|| invalid("pack missing"))?,
                    &selected,
                )
                .map_err(invalid)?;
                let identity = work.pack_wire_identity(id).map_err(invalid)?;
                candidate
                    .outline_run
                    .discover_workers
                    .get_mut(id)
                    .ok_or_else(|| invalid("worker missing"))?
                    .related_read_frames
                    .push(coordinator::RelatedReadFrame {
                        user_page: Some(page),
                        directory_page: None,
                        call_id: String::new(),
                        pack_revision: identity["revision"]
                            .as_u64()
                            .ok_or_else(|| invalid("revision missing"))?,
                        input_digest: base.input_sha256.clone(),
                        options,
                        sealed_sha256: None,
                    });
            }
            let result = PackHost {
                input: self.input,
                config: self.config,
                state: &mut candidate,
                journal: self.journal,
                cancel: self.cancel,
            }
            .prepare_candidate(id, false)
            .await;
            match result {
                Ok(request) => {
                    *self.state = candidate;
                    if persist {
                        self.save().await?;
                    }
                    return Ok(request);
                }
                Err(error) if error.code == "AGENT_TURN_BUDGET_EXCEEDED" => {
                    if !selected.is_empty()
                        && crate::outline::evidence::shrink_selection(self.input, &mut selected)
                            .is_ok()
                    {
                        continue;
                    }
                    // Candidate contains the exact unsent oversized request, not
                    // a fabricated send. No reservation or provider call occurred.
                    if let Some(Pending::Failed { code, .. }) = &mut candidate
                        .outline_run
                        .discover_workers
                        .get_mut(id)
                        .unwrap()
                        .pending
                    {
                        *code = "DISCOVER_READ_CAPACITY_EXCEEDED".into();
                    }
                    *self.state = candidate;
                    if persist {
                        self.save().await?;
                    }
                    return Err(super::error(
                        "DISCOVER_READ_CAPACITY_EXCEEDED",
                        "Minimum worker request cannot fit; worker stopped before reservation or send.",
                    ));
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn prepare_candidate(&mut self, id: &str, persist: bool) -> Result<Request, AgentError> {
        let work = self
            .state
            .outline_run
            .reading_packs
            .as_ref()
            .ok_or_else(|| invalid("reading plan missing"))?;
        let identity = work.pack_wire_identity(id).map_err(invalid)?;
        let mut session = work.session(self.input, id).map_err(invalid)?;
        project_owned_pack_headers(&mut session);
        let owned_image_ids = session["pack"]["atoms"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|atom| atom["carrier"]["kind"] == "image")
            .filter_map(|atom| {
                atom["carrier"]["evidence"]["image_id"]
                    .as_str()
                    .map(str::to_owned)
            })
            .collect::<BTreeSet<_>>();
        let mut image_ids = owned_image_ids.clone();
        for frame in &self.worker(id)?.related_read_frames {
            for reference in frame.options.iter().flat_map(|option| &option.evidence) {
                if let crate::outline::evidence::EvidenceRef::ImageRegion { image_id, .. } =
                    reference
                {
                    image_ids.insert(image_id.clone());
                }
            }
        }
        if !image_ids.is_empty() && !self.config.limits.vision_enabled {
            return Err(error(
                "AGENT_VISION_NOT_CONFIGURED",
                "original image dependency requires the configured model's image capability",
            ));
        }
        let mut views = Vec::new();
        for image_id in image_ids {
            let view = if let Some(view) = self
                .state
                .source_views
                .values()
                .find(|view| view.identity.source_id == image_id)
            {
                view.clone()
            } else {
                self.journal
                    .source_view(&image_id, &self.config.limits, self.cancel)
                    .await?
            };
            if view.identity.source_id != image_id {
                return Err(invalid("worker image identity mismatch"));
            }
            super::validate_frozen_view(self.input, self.config, &view).map_err(invalid)?;
            self.state
                .source_views
                .insert(view.id().map_err(invalid)?, view.clone());
            views.push(view);
        }
        let worker = self.worker(id)?;
        let generation = worker
            .generation
            .checked_add(1)
            .ok_or_else(|| invalid("worker generation overflow"))?;
        let queued = worker
            .related_read_frames
            .iter()
            .filter(|frame| {
                frame.input_digest == self.state.input_sha256
                    && frame.pack_revision == identity["revision"]
            })
            .flat_map(|frame| frame.options.iter())
            .flat_map(|option| option.evidence.iter().cloned())
            .collect::<Vec<_>>();
        let next_context = work
            .next_required_context_read(self.input, id, &queued)
            .map_err(invalid)?;
        let next_required_read = worker
            .related_continuations
            .keys()
            .next()
            .map(|cursor| json!({"tool":"read_evidence","args":{"cursor":cursor}}))
            .unwrap_or(next_context);
        session["pack"]["next_required_read"] = next_required_read.clone();
        session["pack"]["next_required_read_basis"] =
            json!("after_consuming_current_request_original_pages; navigation_only");
        let mut messages = vec![
            json!({"role":"system","content":format!("{}\nThis worker owns exactly one pack. Submit only that pack using submit_pack; all required original images are attached. Do not select another pack.",Duty::Discover.instructions())}),
            json!({"role":"user","content":json!({"reading_packs":[session], "semantic_contract":crate::outline::agent::semantic_guidance(Duty::Discover), "repair_context":self.state.outline_run.repair_events.iter().rev().find(|event|event["pack_id"]==id && event["status"]=="reopened" && event["pack_revision"]==identity["revision"])}).to_string()}),
        ];
        messages.push(json!({"role":"user","content":json!({"dependency_progress":{"next_required_read":next_required_read,"pending_cursors":worker.related_continuations.keys().collect::<Vec<_>>(),"policy":"After consuming all original pages attached to this request, follow next_required_read before submitting. The action anticipates these attached pages only for navigation and grants no delivery credit. original_read_complete marks only confirmed current-claim coverage; do not repeatedly read that completed entry or its support alias while another required original remains. This is navigation, not semantic applicability approval. Only completed exact original reads become durable coverage. Delivered tool payloads may be retired; preserve task-relevant observations in assistant content. Granted support handles remain accessible through condition_support_options and next_support_cursor (read_evidence dependency_cursor). Directory pages grant no original reading credit. Follow pending body cursors until selection_complete; reread original source keys or a support dependency_id when needed. No dependency grants ownership or completion of another pack."}}).to_string()}));
        messages.extend(project_worker_request_history(worker));
        for frame in &worker.related_read_frames {
            if let Some(page) = &frame.user_page {
                messages.push(json!({"role":"user","content":page.to_string()}));
            }
        }
        for view in &views {
            messages.push(view.message());
        }
        messages.push(json!({"role":"user","content":json!({"worker_pack":id,"generation":generation}).to_string()}));
        let tools = crate::outline::agent::schemas_for(Duty::Discover)
            .into_iter()
            .filter(|tool| {
                matches!(
                    tool["function"]["name"].as_str(),
                    Some("submit_pack" | "read_evidence")
                )
            })
            .collect();
        let bytes =
            crate::agent_runtime::chat::prepare(&self.config.provider, messages, tools).await?;
        let mut body: Value = serde_json::from_slice(&bytes).map_err(invalid)?;
        crate::outline::discover::compact_request(&mut body).map_err(invalid)?;
        let source_keys = crate::outline::source_wire::request_in_scope(
            &json!({"input":self.state.input_sha256,"worker":identity}),
            &worker.source_keys,
            &mut body,
        )
        .map_err(invalid)?;
        let registry = crate::outline::model_wire::project_scope(
            &worker.registry,
            &json!([identity, generation]),
            &mut body,
        )
        .map_err(invalid)?;
        let count = crate::agent_runtime::chat::estimate_request_tokens_with_reserve(
            &body,
            &self.config.limits.tokenizer,
            self.config.limits.image_token_reserve,
            self.config.limits.token_safety_margin,
            self.config.provider.output_token_reserve as usize,
        )?;
        let context_limit = worker
            .reduced_context_tokens
            .unwrap_or(self.config.limits.max_context_tokens);
        let exceeds_budget = count.total_context_tokens > context_limit;
        let mut read_frames = worker.related_read_frames.clone();
        for frame in &mut read_frames {
            frame.sealed_sha256 =
                related_read_wire_hash(self.input, self.state, frame, &body).map_err(invalid)?;
        }
        let body = {
            #[cfg(test)]
            let _profile = super::profile::enter("worker_request_serialization");
            crate::agent_runtime::canonical_request_bytes(&body)?
        };
        let request = Request {
            scope: Scope {
                run: identity["run"]
                    .as_str()
                    .ok_or_else(|| invalid("run scope missing"))?
                    .into(),
                pack: id.into(),
                revision: identity["revision"].as_u64().unwrap(),
                generation,
                request: crate::outline::canonical_sha256(&(&identity, generation, &body))
                    .map_err(invalid)?,
            },
            body,
            input_reserve: count.total_input_tokens as u64,
            output_reserve: self.config.provider.output_token_reserve as u64,
        };
        let worker = self.worker_mut(id)?;
        worker.registry = registry;
        worker.source_keys = source_keys;
        worker.related_read_frames = read_frames;
        worker.generation = generation;
        worker.images = views
            .iter()
            .filter(|view| owned_image_ids.contains(&view.identity.source_id))
            .map(|view| {
                (
                    view.identity.source_id.clone(),
                    view.identity.image_sha256.clone(),
                )
            })
            .collect();
        worker.pending = Some(if exceeds_budget {
            Pending::Failed {
                request: request.clone(),
                code: "DISCOVER_READ_CAPACITY_EXCEEDED".into(),
            }
        } else {
            Pending::Prepared {
                request: request.clone(),
            }
        });
        if exceeds_budget {
            return Err(error(
                "AGENT_TURN_BUDGET_EXCEEDED",
                "worker final wire exceeds context; no send",
            ));
        }
        if persist {
            self.save().await?;
        }
        Ok(request)
    }
}
#[async_trait]
impl<J: Journal> Host for PackHost<'_, J> {
    async fn replay_received(&mut self) -> Result<(), AgentError> {
        let received = self
            .state
            .outline_run
            .discover_workers
            .values()
            .filter_map(|worker| match &worker.pending {
                Some(Pending::Received { request, response }) => {
                    Some((request.clone(), response.clone()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        for (request, response) in received {
            self.commit(&request, serde_json::from_value(response).map_err(invalid)?)
                .await?;
        }
        if self
            .state
            .outline_run
            .discover_workers
            .values()
            .any(|worker| matches!(worker.pending, Some(Pending::Sending { .. })))
        {
            return Err(error(
                "AGENT_SEND_OUTCOME_UNKNOWN",
                "interrupted worker send outcome unknown; reservation retained; explicit resolution required",
            ));
        }
        Ok(())
    }
    async fn next(&mut self, active: &BTreeSet<String>) -> Result<Option<Request>, AgentError> {
        loop {
            let mut local_capacity_failure = false;
            let work = self
                .state
                .outline_run
                .reading_packs
                .as_ref()
                .ok_or_else(|| invalid("reading plan missing"))?;
            let mut settled = BTreeSet::new();
            for id in work.pack_ids() {
                let Some(worker) = self.state.outline_run.discover_workers.get(&id) else {
                    continue;
                };
                let Some(Pending::Failed { request, code }) = &worker.pending else {
                    continue;
                };
                if request.scope.pack != id {
                    continue;
                }
                let identity = work.pack_wire_identity(&id).map_err(invalid)?;
                if code != "BUSINESS_REJECTED"
                    && request.scope.run == identity["run"]
                    && identity["revision"]
                        .as_u64()
                        .is_some_and(|revision| request.scope.revision <= revision)
                {
                    settled.insert(id);
                }
            }
            let work = self.state.outline_run.reading_packs.as_mut().unwrap();
            work.claim_excluding_settled(coordinator::WORKERS, &settled);
            let ids = work
                .pack_ids()
                .into_iter()
                .filter(|id| {
                    matches!(
                        work.status(id),
                        Some(PackStatus::Running | PackStatus::Failed)
                    )
                })
                .collect::<Vec<_>>();
            for id in ids {
                let identity = self
                    .state
                    .outline_run
                    .reading_packs
                    .as_ref()
                    .unwrap()
                    .pack_wire_identity(&id)
                    .map_err(invalid)?;
                let worker = self
                    .state
                    .outline_run
                    .discover_workers
                    .entry(id.clone())
                    .or_default();
                // A terminal host/stall outcome stays settled across same-run
                // claim repair. Never silently revive it after reload or rotation.
                if matches!(&worker.pending, Some(Pending::Failed { request, code })
                if code != "BUSINESS_REJECTED" && request.scope.run == identity["run"]
                    && identity["revision"].as_u64().is_some_and(|revision| request.scope.revision <= revision))
                {
                    continue;
                }
                if let Some(pending) = &worker.pending {
                    let request = match pending {
                        Pending::Prepared { request }
                        | Pending::Sending { request }
                        | Pending::Received { request, .. }
                        | Pending::Committed { request, .. }
                        | Pending::Failed { request, .. } => request,
                    };
                    if request.scope.run != identity["run"]
                        || request.scope.revision != identity["revision"]
                    {
                        if matches!(pending, Pending::Sending { .. } | Pending::Received { .. }) {
                            return Err(invalid(
                                "unresolved worker journal scope changed; settle or replay before resetting",
                            ));
                        }
                        let rejected_turns = worker.rejected_turns;
                        let failure_signatures = worker.failure_signatures.clone();
                        let progress_evidence = worker.progress_evidence.clone();
                        let history = worker.history.clone();
                        let history_revision_start = history.len();
                        let retired_read_calls = worker.retired_read_calls.clone();
                        // Repair/reopen rotates evidence claims, not the
                        // provider capacity already learned within this run.
                        let reduced_context_tokens = (request.scope.run == identity["run"])
                            .then_some(worker.reduced_context_tokens)
                            .flatten();
                        *worker = Worker {
                            rejected_turns,
                            failure_signatures,
                            progress_evidence,
                            history,
                            history_revision_start,
                            retired_read_calls,
                            reduced_context_tokens,
                            ..Default::default()
                        };
                    } else if matches!(pending, Pending::Committed { .. }) {
                        continue;
                    }
                }
                match &worker.pending {
                    Some(Pending::Prepared { request }) => return Ok(Some(request.clone())),
                    Some(Pending::Sending { request })
                        if active.contains(&request.scope.request) =>
                    {
                        continue;
                    }
                    Some(Pending::Sending { .. } | Pending::Received { .. }) => {
                        return Err(invalid("unresolved worker journal"));
                    }
                    Some(Pending::Failed { code, .. }) if code != "BUSINESS_REJECTED" => continue,
                    _ => (),
                }
                match self.prepare(&id).await {
                    Ok(request) => return Ok(Some(request)),
                    Err(error) if error.code == "DISCOVER_READ_CAPACITY_EXCEEDED" => {
                        if let Some(limit) = self.worker(&id)?.reduced_context_tokens
                            && self.split_for_capacity(&id, limit).is_ok()
                        {
                            self.save().await?;
                        }
                        local_capacity_failure = true;
                        continue;
                    }
                    Err(error) => return Err(error),
                }
            }
            if !local_capacity_failure {
                return Ok(None);
            }
            // Newly settled local failures release slots for untouched packs.
        }
    }
    async fn reserve(&mut self, request: &Request) -> Result<(), AgentError> {
        if self.cancel.is_cancelled() {
            return Err(error(
                "AGENT_CANCELLED",
                "cancelled before worker admission",
            ));
        }
        if !matches!(&self.worker(&request.scope.pack)?.pending,Some(Pending::Prepared{request:saved}) if saved==request)
        {
            return Err(invalid("worker request changed before reservation"));
        }
        let prior = self.state.journal.accounting.clone();
        self.state.journal.accounting.record(
            request.input_reserve,
            request.output_reserve,
            crate::agent_runtime::budget::unix_seconds(),
        )?;
        if let Err(error) = self.journal.admit(self.state, &request.body).await {
            self.state.journal.accounting = prior;
            self.save().await?;
            return Err(error);
        }
        self.worker_mut(&request.scope.pack)?.pending = Some(Pending::Sending {
            request: request.clone(),
        });
        self.journal
            .reserve_discover_worker(self.state, &request.scope.request, &request.body)
            .await
    }
    async fn received(&mut self, request: &Request, response: &ChatTurn) -> Result<(), AgentError> {
        if !matches!(&self.worker(&request.scope.pack)?.pending,Some(Pending::Sending{request:saved}) if saved==request)
        {
            return Err(invalid("response outside worker reservation"));
        }
        self.worker_mut(&request.scope.pack)?.pending = Some(Pending::Received {
            request: request.clone(),
            response: serde_json::to_value(response).map_err(invalid)?,
        });
        self.state.journal.note_usage(response);
        self.save().await
    }
    async fn commit(&mut self, request: &Request, response: ChatTurn) -> Result<(), AgentError> {
        let id = &request.scope.pack;
        if !matches!(&self.worker(id)?.pending,Some(Pending::Received{request:saved,..}) if saved==request)
        {
            return Err(invalid("worker commit without durable response"));
        }
        let action = canonical_worker_action(self.input, self.state, request, &response);
        let mut rejection_class = "invalid_tool_turn";
        let mut confirmation_failed = false;
        let outcome = (|| -> Result<Value, String> {
            let identity =
                validate_worker_scope(self.input, self.state, request).inspect_err(|_| {
                    confirmation_failed = true;
                })?;
            if !crate::agent_runtime::valid_tool_turn(&response) {
                return Err("worker response is not a complete tool turn".into());
            }
            confirm_worker_related_reads(self.input, self.state, request).inspect_err(|_| {
                confirmation_failed = true;
            })?;
            confirm_worker_images(self.input, self.config, self.state, request).inspect_err(
                |_| {
                    confirmation_failed = true;
                },
            )?;
            if response.tool_calls.len() != 1
                || !matches!(
                    response.tool_calls[0].name.as_str(),
                    "submit_pack" | "read_evidence"
                )
            {
                return Err(
                    "worker must request a dependency or submit exactly one owned pack".into(),
                );
            }
            let call = &response.tool_calls[0];
            rejection_class = "invalid_json_arguments";
            let args: Value = serde_json::from_str(&call.arguments).map_err(|e| e.to_string())?;
            rejection_class = "invalid_wire_identity";
            let args = self.state.outline_run.discover_workers[id]
                .registry
                .decode(args, false)?;
            rejection_class = "cross_pack_submission";
            if call.name == "submit_pack" && args["pack_id"] != *id {
                return Err("cross-pack worker submission".into());
            }
            if call.name == "read_evidence" {
                rejection_class = "invalid_source_read";
                let keys = &self.state.outline_run.discover_workers[id].source_keys;
                let args = crate::outline::source_wire::resolve_in_scope(
                    &json!({"input":self.state.input_sha256,"worker":identity}),
                    "read_evidence",
                    args,
                    keys,
                )?;
                return Ok(json!({"status":"read_request","args":args}));
            }
            rejection_class = "invalid_submission_schema";
            crate::outline::agent::validate_arguments("submit_pack", &args)?;
            if !self
                .state
                .outline_run
                .reading_packs
                .as_ref()
                .unwrap()
                .missing_required_support(self.input, id)?
                .is_empty()
            {
                rejection_class = "required_support_pending";
                return Err("Required structural support pages remain; the host will attach the next page automatically before accepting this submission.".into());
            }
            rejection_class = "submission_rejected";
            crate::outline::agent::apply_for_duty(
                self.input,
                self.state,
                "submit_pack",
                &args,
                Duty::Discover,
            )
        })();
        let mut host_failure_code = None;
        let outcome = match outcome {
            Ok(value) if value["status"] == "read_request" => self
                .related_page_typed(request, &response, &value["args"])
                .await
                .map_err(|failure| match failure {
                    ReadPageFailure::Business(message) => message,
                    ReadPageFailure::Capacity(message) => {
                        host_failure_code = Some("DISCOVER_READ_CAPACITY_EXCEEDED".to_owned());
                        message
                    }
                    ReadPageFailure::Host(error) => {
                        host_failure_code = Some(error.code);
                        error.message
                    }
                }),
            other => other,
        };
        let mut receipt = match outcome {
            Ok(value) => super::tool_result_envelope(value),
            Err(message) if host_failure_code.is_some() => json!({"ok":false,"error":message,
                "code":host_failure_code,"retryable":false}),
            Err(message) if confirmation_failed => json!({"ok":false,"error":message,
                "code":"DISCOVER_SOURCE_CONFIRMATION_FAILED",
                "retryable":false}),
            Err(message) => json!({"ok":false,"error":message}),
        };
        let committed = receipt["ok"] == true && receipt["result"]["status"] == "committed";
        let no_coverage = receipt["ok"] == true
            && receipt["result"]["status"] == "read_pending_delivery"
            && receipt["result"]["read"]["new_coverage"] == false;
        let business_failure = host_failure_code.is_none()
            && !confirmation_failed
            && rejection_class != "required_support_pending"
            && (receipt["ok"] != true || no_coverage);
        let classes = rejection_classes(
            &receipt,
            if no_coverage {
                "read_without_new_coverage"
            } else {
                rejection_class
            },
        );
        let signature = crate::outline::canonical_sha256(&(action, classes)).map_err(invalid)?;
        let worker = self.worker_mut(id)?;
        advance_worker_progress(worker);
        let repeated_failure = business_failure && !worker.failure_signatures.insert(signature);
        if repeated_failure {
            receipt["code"] = json!("DISCOVER_NO_STATE_PROGRESS");
            receipt["retryable"] = json!(false);
            receipt["stop_reason"] = json!(
                "This canonical failure already occurred without new source coverage or a valid product; worker stopped with audit preserved."
            );
        }
        worker.history.push(json!({"role":"assistant","content":response.content,"tool_calls":response.tool_calls.iter().map(|call|json!({"id":call.id,"type":"function","function":{"name":call.name,"arguments":call.arguments}})).collect::<Vec<_>>()}));
        for call in &response.tool_calls {
            worker
                .history
                .push(json!({"role":"tool","tool_call_id":call.id,"content":receipt.to_string()}));
        }
        if receipt["ok"] == true && receipt["result"]["status"] == "read_pending_delivery" {
            if receipt["result"]["read"]["new_coverage"] == false {
                worker.rejected_turns += 1;
                worker.pending = Some(Pending::Failed {
                    request: request.clone(),
                    code: if repeated_failure {
                        "DISCOVER_NO_STATE_PROGRESS"
                    } else {
                        "BUSINESS_REJECTED"
                    }
                    .into(),
                });
            } else {
                worker.pending = None;
            }
        } else if committed {
            // A canonical product starts a new business epoch. Historical
            // rejection counts remain audit facts, not a future stop budget.
            worker.failure_signatures.clear();
            worker.pending = Some(Pending::Committed {
                request: request.clone(),
                receipt,
            });
        } else {
            // A host delivery confirmation cannot be repaired by changing the
            // model's submission. Preserve the failed frame and settled audit,
            // but never purchase another response to the same host failure.
            if !confirmation_failed && host_failure_code.is_none() {
                worker.rejected_turns += 1;
            }
            worker.pending = Some(Pending::Failed {
                request: request.clone(),
                code: if let Some(code) = host_failure_code {
                    code
                } else if confirmation_failed {
                    "DISCOVER_SOURCE_CONFIRMATION_FAILED".into()
                } else if repeated_failure {
                    "DISCOVER_NO_STATE_PROGRESS".into()
                } else {
                    "BUSINESS_REJECTED".into()
                },
            });
        }
        self.state.turn = self
            .state
            .turn
            .checked_add(1)
            .ok_or_else(|| invalid("turn overflow"))?;
        self.state.journal.sequence = self
            .state
            .journal
            .sequence
            .checked_add(1)
            .ok_or_else(|| invalid("journal overflow"))?;
        self.state.tool_calls += response.tool_calls.len();
        self.save().await
    }
    async fn failed(&mut self, request: &Request, error: &AgentError) -> Result<(), AgentError> {
        if !matches!(&self.worker(&request.scope.pack)?.pending, Some(Pending::Sending { request: saved }) if saved == request)
        {
            return Err(invalid(
                "failure does not match the reserved physical request",
            ));
        }
        self.worker_mut(&request.scope.pack)?.pending = Some(Pending::Failed {
            request: request.clone(),
            code: error.code.clone(),
        });
        if error.code == "AGENT_PROVIDER_CONTEXT_EXCEEDED" {
            let replans = self
                .state
                .outline_run
                .reading_packs
                .as_ref()
                .ok_or_else(|| invalid("reading plan missing"))?
                .context_replans;
            if replans < 3 {
                let current = self
                    .worker(&request.scope.pack)?
                    .reduced_context_tokens
                    .unwrap_or(self.config.limits.max_context_tokens);
                // Measured rejected size can be lower than the configured
                // window. Always shrink below both, never resend the same wire.
                let measured = request
                    .input_reserve
                    .saturating_add(request.output_reserve)
                    .min(usize::MAX as u64) as usize;
                let limit = current.min(measured).saturating_mul(3) / 4;
                if self.split_for_capacity(&request.scope.pack, limit).is_ok() {
                    self.state
                        .outline_run
                        .reading_packs
                        .as_mut()
                        .unwrap()
                        .context_replans += 1;
                }
            }
        }
        self.save().await
    }
    async fn cancelled(&mut self, _: &BTreeSet<String>) -> Result<(), AgentError> {
        self.save().await
    }
    fn all_committed(&self) -> bool {
        self.state
            .outline_run
            .reading_packs
            .as_ref()
            .is_some_and(crate::outline::discover::DiscoverWork::complete)
    }
}
// Only complete single-submit failures appended by commit are replaceable.
// History remains the audit; this is a request projection, never read credit.
fn settled_submission_pair(history: &[Value], index: usize) -> Option<&Value> {
    let message = history.get(index)?;
    if message["role"] != "assistant" {
        return None;
    }
    let calls = message["tool_calls"].as_array()?;
    if calls.len() != 1 || calls[0]["function"]["name"] != "submit_pack" {
        return None;
    }
    let call_id = calls[0]["id"].as_str()?;
    let receipt = history.get(index + 1)?;
    if receipt["role"] != "tool" || receipt["tool_call_id"] != call_id {
        return None;
    }
    let value: Value = serde_json::from_str(receipt["content"].as_str()?).ok()?;
    (value["ok"] == false).then_some(receipt)
}
#[cfg(test)]
fn settled_submission_audit(history: &[Value]) -> Vec<Value> {
    let mut result = Vec::new();
    for index in 0..history.len() {
        if let Some(receipt) = settled_submission_pair(history, index) {
            result.push(history[index].clone());
            result.push(receipt.clone());
        }
    }
    result
}
pub(super) fn project_settled_submission_history(history: &[Value]) -> Vec<Value> {
    let mut result = Vec::new();
    let mut index = 0;
    let mut feedbacks = BTreeSet::new();
    let mut summaries = Vec::new();
    while index < history.len() {
        if let Some(receipt) = settled_submission_pair(history, index) {
            if let Some(content) = history[index]["content"].as_str().filter(|s| !s.is_empty()) {
                result.push(json!({"role":"assistant","content":content}));
            }
            if feedbacks.insert(receipt["content"].clone().to_string()) {
                summaries.push(json!({"role":"user","content":json!({"settled_submission_failure":serde_json::from_str::<Value>(receipt["content"].as_str().unwrap()).unwrap(),"instruction":"The prior submission failed. Correct it using the current pack claim and source keys; the omitted duplicate submission body remains in the host audit. This grants no completion or evidence credit."}).to_string()}));
            }
            index += 2;
        } else {
            result.push(history[index].clone());
            index += 1;
        }
    }
    result.extend(summaries);
    result
}

// Worker-only projection: preserve the audit and every unconsumed tool group.
// Normalize only host-issued identities; this never authorizes an operation.
fn canonical_worker_action(
    input: &FrozenInput,
    state: &Checkpoint,
    request: &Request,
    response: &ChatTurn,
) -> Value {
    let worker = &state.outline_run.discover_workers[&request.scope.pack];
    let work = state.outline_run.reading_packs.as_ref().unwrap();
    let calls = response
        .tool_calls
        .iter()
        .map(|call| {
            let raw = match serde_json::from_str::<Value>(&call.arguments) {
            Ok(raw) => raw,
            Err(_) => return json!({"tool":call.name,"invalid_json_sha256":crate::outline::canonical_sha256(&call.arguments).expect("string hash")}),
        };
            let mut args = worker.registry.decode(raw.clone(), true).unwrap_or(raw);
            if call.name == "submit_pack" {
                if let Some(pack) = work.canonical_pack(&request.scope.pack) {
                    // Partial normalization is still useful for malformed submissions;
                    // opaque leftovers below become one invalid-handle marker.
                    let _ = crate::outline::discover::resolve_submission_handles(pack, &mut args);
                }
            } else if call.name == "read_evidence" {
                let resolved = if let Some(key) = args["dependency_id"].as_str() {
                    work.dependency_selection(input, &request.scope.pack, key)
                        .ok()
                        .map(|refs| json!({"refs":refs}))
                } else if let Some(cursor) = args["cursor"].as_str() {
                    worker
                        .related_continuations
                        .get(cursor)
                        .map(|page| json!({"refs":page.refs}))
                } else if let Some(cursor) = args["dependency_cursor"].as_str() {
                    work.dependency_directory_page(input, &request.scope.pack, cursor)
                        .ok()
                } else {
                    work.pack_wire_identity(&request.scope.pack)
                        .ok()
                        .and_then(|identity| {
                            crate::outline::source_wire::resolve_in_scope(
                                &json!({"input":state.input_sha256,"worker":identity}),
                                "read_evidence",
                                args.clone(),
                                &worker.source_keys,
                            )
                            .ok()
                        })
                };
                if let Some(resolved) = resolved {
                    args = resolved;
                }
            }
            scrub_action_transport(&mut args);
            json!({"tool":call.name,"arguments":args})
        })
        .collect::<Vec<_>>();
    json!(calls)
}
fn scrub_action_transport(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            for key in [
                "wire_scope",
                "call_id",
                "claim_token",
                "pack_revision",
                "revision",
                "generation",
                "run",
                "repair",
                "review_version",
            ] {
                fields.remove(key);
            }
            for (key, child) in fields.iter_mut() {
                if matches!(
                    key.as_str(),
                    "atom_key"
                        | "evidence_key"
                        | "source_key"
                        | "support_key"
                        | "dependency_id"
                        | "dependency_cursor"
                        | "cursor"
                        | "next_cursor"
                        | "next_dependency_cursor"
                        | "next_support_cursor"
                ) {
                    *child = json!("unresolved_handle");
                } else {
                    scrub_action_transport(child);
                    if matches!(
                        key.as_str(),
                        "description" | "no_requirement_reason" | "requested_information"
                    ) && child.as_str().is_some_and(|text| text.trim().is_empty())
                    {
                        *child = json!("");
                    }
                    if matches!(
                        key.as_str(),
                        "evidence" | "inspected_atom_ids" | "condition_support"
                    ) && let Some(items) = child.as_array_mut()
                    {
                        items.sort_by_key(Value::to_string);
                    }
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                scrub_action_transport(item);
            }
        }
        _ => (),
    }
}
fn rejection_classes(receipt: &Value, fallback: &str) -> Value {
    fn collect(value: &Value, classes: &mut BTreeSet<(String, String)>) {
        match value {
            Value::Object(fields) => {
                if let Some(code) = fields.get("code").and_then(Value::as_str) {
                    classes.insert((
                        fields
                            .get("path")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .into(),
                        code.into(),
                    ));
                }
                for child in fields.values() {
                    collect(child, classes);
                }
            }
            Value::Array(items) => {
                for item in items {
                    collect(item, classes);
                }
            }
            _ => (),
        }
    }
    let mut classes = BTreeSet::new();
    collect(&receipt["result"], &mut classes);
    if classes.is_empty() {
        classes.insert((String::new(), fallback.into()));
    }
    json!(classes)
}
fn advance_worker_progress(worker: &mut Worker) {
    let mut advanced = false;
    for reference in &worker.delivered_related_evidence {
        if !crate::outline::evidence::covered_by_union(reference, &worker.progress_evidence) {
            worker.progress_evidence.push(reference.clone());
            advanced = true;
        }
    }
    if advanced {
        worker.failure_signatures.clear();
    }
}

// Structural support is delivered once in a sealed user page. Keep native
// layout and owned header fragments; retire only context-only source text.
use crate::outline::discover::project_owned_pack_headers;

fn project_worker_request_history(worker: &Worker) -> Vec<Value> {
    let split = worker.history_revision_start.min(worker.history.len());
    let mut history = worker.history[..split]
        .iter()
        .filter_map(|message| {
            (message["role"] == "assistant")
                .then(|| message["content"].as_str())
                .flatten()
                .filter(|text| !text.is_empty())
                .map(|text| json!({"role":"assistant","content":text}))
        })
        .collect::<Vec<_>>();
    history.extend(
        retire_delivered_related_history(&worker.history[split..], &worker.retired_read_calls)
            .into_iter()
            .filter(|message| {
                message["role"] != "user"
                    || !message["content"]
                        .as_str()
                        .and_then(|text| serde_json::from_str::<Value>(text).ok())
                        .is_some_and(|value| value.get("automatic_support_page").is_some())
            }),
    );
    project_worker_history(&history)
}

fn project_worker_history(history: &[Value]) -> Vec<Value> {
    let mut projected = project_settled_submission_history(history);
    projected.dedup_by(|later, earlier| {
        later == earlier
            && later["role"] == "assistant"
            && later["content"].is_string()
            && later.as_object().is_some_and(|fields| fields.len() == 2)
    });
    projected
}

fn retire_delivered_related_history(history: &[Value], delivered: &BTreeSet<String>) -> Vec<Value> {
    history
        .iter()
        .filter_map(|message| {
            if message["role"] == "tool"
                && message["tool_call_id"]
                    .as_str()
                    .is_some_and(|id| delivered.contains(id))
            {
                return None;
            }
            if message["role"] == "assistant"
                && message["tool_calls"].as_array().is_some_and(|calls| {
                    calls
                        .iter()
                        .any(|call| call["id"].as_str().is_some_and(|id| delivered.contains(id)))
                })
            {
                return message["content"]
                    .as_str()
                    .filter(|text| !text.is_empty())
                    .map(|text| json!({"role":"assistant","content":text}));
            }
            Some(message.clone())
        })
        .collect()
}

fn related_read_wire_hash(
    input: &FrozenInput,
    state: &Checkpoint,
    frame: &coordinator::RelatedReadFrame,
    body: &Value,
) -> Result<Option<String>, String> {
    let tool_hash = if let Some(page) = &frame.user_page {
        let marker = page["automatic_support_page"]["id"]
            .as_str()
            .ok_or("automatic support page identity missing")?;
        let matches = body["messages"]
            .as_array()
            .ok_or("messages missing")?
            .iter()
            .filter(|message| message["role"] == "user")
            .filter(|message| {
                let text = match &message["content"] {
                    Value::String(text) => text.clone(),
                    Value::Array(parts) => parts
                        .iter()
                        .filter_map(|part| part["text"].as_str())
                        .collect::<String>(),
                    _ => String::new(),
                };
                serde_json::from_str::<Value>(&text)
                    .is_ok_and(|value| value["automatic_support_page"]["id"] == marker)
            })
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return Err("automatic support page missing or duplicated in exact request".into());
        }
        Some(crate::outline::canonical_sha256(&(
            matches[0],
            &frame.options,
            body,
        ))?)
    } else {
        crate::outline::read_receipts::wire_hash(body, &frame.call_id)?
    };
    let mut originals = Vec::new();
    for reference in frame.options.iter().flat_map(|option| &option.evidence) {
        if let crate::outline::evidence::EvidenceRef::ImageRegion {
            image_id, region, ..
        } = reference
        {
            if region != "original" {
                return Err("dependency image region is not original".into());
            }
            let source = input
                .source_units
                .iter()
                .find(|source| source.source_unit_revision_id == *image_id)
                .ok_or("dependency image source missing")?;
            let view = state
                .source_views
                .values()
                .find(|view| view.identity.source_id == *image_id)
                .ok_or("dependency image view missing")?;
            view.validate(
                image_id,
                view.identity.width.max(view.identity.height),
                view.jpeg_base64.len(),
            )?;
            if source.locator["image_ref"]
                .as_str()
                .and_then(|r| r.strip_prefix("objects/"))
                != Some(view.identity.original_sha256.as_str())
            {
                return Err("dependency image original hash changed".into());
            }
            let message = view.message();
            if !body["messages"]
                .as_array()
                .is_some_and(|messages| messages.contains(&message))
            {
                return Err(
                    "dependency original image pixels and identity absent from exact request"
                        .into(),
                );
            }
            originals.push(message);
        }
    }
    if originals.is_empty() {
        return Ok(tool_hash);
    }
    tool_hash
        .map(|hash| crate::outline::canonical_sha256(&(hash, originals)))
        .transpose()
}

fn confirm_worker_related_reads(
    input: &FrozenInput,
    state: &mut Checkpoint,
    request: &Request,
) -> Result<(), String> {
    if crate::outline::evidence::input_digest(input)? != state.input_sha256 {
        return Err("related read checkpoint input changed".into());
    }
    let worker = state
        .outline_run
        .discover_workers
        .get(&request.scope.pack)
        .ok_or("worker missing")?;
    if !matches!(&worker.pending, Some(Pending::Received { request: saved, response })
        if saved == request && serde_json::from_value::<ChatTurn>(response.clone()).is_ok_and(|turn| crate::agent_runtime::valid_tool_turn(&turn)))
    {
        return Err("related read requires this worker's durable complete response".into());
    }
    let body: Value = serde_json::from_slice(&request.body).map_err(|e| e.to_string())?;
    let mut work = state
        .outline_run
        .reading_packs
        .clone()
        .ok_or("reading plan missing")?;
    let identity = work.pack_wire_identity(&request.scope.pack)?;
    if identity["run"] != request.scope.run || identity["revision"] != request.scope.revision {
        return Err("related read worker scope is stale".into());
    }
    let mut pending = Vec::new();
    let mut delivered_calls = BTreeSet::new();
    let mut directory_pages = BTreeSet::new();
    let mut delivered_evidence = worker.delivered_related_evidence.clone();
    let mut delivered_user_pages = Vec::new();
    for frame in &worker.related_read_frames {
        if frame.pack_revision != request.scope.revision || frame.input_digest != state.input_sha256
        {
            return Err("related read frame belongs to another input or pack revision".into());
        }
        if frame.sealed_sha256.is_some()
            && related_read_wire_hash(input, state, frame, &body)? == frame.sealed_sha256
        {
            if let Some(page) = &frame.directory_page {
                if !frame.options.is_empty() {
                    return Err("directory receipt cannot grant source credit".into());
                }
                directory_pages.insert(page.clone());
            } else {
                work.confirm_related_read(input, &request.scope.pack, &frame.options)?;
                for reference in frame.options.iter().flat_map(|option| &option.evidence) {
                    if !crate::outline::evidence::covered_by_union(reference, &delivered_evidence) {
                        delivered_evidence.push(reference.clone());
                    }
                }
            }
            if let Some(page) = &frame.user_page {
                delivered_user_pages.push(json!({"role":"user","content":page.to_string()}));
            } else {
                delivered_calls.insert(frame.call_id.clone());
            }
        } else {
            pending.push(frame.clone());
        }
    }
    state.outline_run.reading_packs = Some(work);
    let worker = state
        .outline_run
        .discover_workers
        .get_mut(&request.scope.pack)
        .unwrap();
    worker.related_read_frames = pending;
    worker.delivered_dependency_pages.extend(directory_pages);
    worker.delivered_related_evidence = delivered_evidence;
    worker.retired_read_calls.extend(delivered_calls);
    worker.history.extend(delivered_user_pages);
    Ok(())
}

fn confirm_worker_images(
    input: &FrozenInput,
    config: &Config,
    state: &mut Checkpoint,
    request: &Request,
) -> Result<(), String> {
    let body: Value = serde_json::from_slice(&request.body).map_err(|e| e.to_string())?;
    let urls = body["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["content"].as_array())
        .flatten()
        .filter(|p| p["type"] == "image_url")
        .filter_map(|p| p["image_url"]["url"].as_str())
        .collect::<BTreeSet<_>>();
    let worker = state
        .outline_run
        .discover_workers
        .get(&request.scope.pack)
        .ok_or("worker missing")?;
    if !matches!(&worker.pending,Some(Pending::Received{request:saved,response}) if saved==request && serde_json::from_value::<ChatTurn>(response.clone()).is_ok_and(|r|crate::agent_runtime::valid_tool_turn(&r)))
    {
        return Err("image credit requires a complete durably received worker response".into());
    }
    let mut candidate = state
        .outline_run
        .reading_packs
        .clone()
        .ok_or("reading plan missing")?;
    for (image, hash) in &worker.images {
        let view = state
            .source_views
            .values()
            .find(|v| v.identity.source_id == *image && v.identity.image_sha256 == *hash)
            .ok_or("worker image cache mismatch")?;
        super::validate_frozen_view(input, config, view)?;
        if !urls.contains(format!("data:image/jpeg;base64,{}", view.jpeg_base64).as_str())
            || !body["messages"]
                .as_array()
                .is_some_and(|messages| messages.contains(&view.message()))
        {
            return Err("worker original image absent from exact reserved request".into());
        }
        candidate.confirm_pack_image(input, &request.scope.pack, image, hash)?;
    }
    state.outline_run.reading_packs = Some(candidate);
    Ok(())
}

pub(super) async fn run<J: Journal, M: Model>(
    input: &FrozenInput,
    config: &Config,
    state: &mut Checkpoint,
    journal: &J,
    model: &M,
    cancel: &CancellationToken,
) -> Result<(), AgentError> {
    if state.outline_run.reading_packs.is_none() {
        super::request(input, config, state).await?;
    }
    let provider = PackProvider { config, model };
    let mut host = PackHost {
        input,
        config,
        state,
        journal,
        cancel,
    };
    if coordinator::drive(&mut host, &provider, cancel).await? {
        Ok(())
    } else if crate::outline::partial_source::eligible(input, host.state).is_some() {
        host.save().await?;
        Ok(())
    } else {
        Err(error(
            "AGENT_DISCOVER_BLOCKED",
            "Discover workers stopped without all-pack commit; checkpoint retained",
        ))
    }
}

#[cfg(test)]
#[path = "discover_parallel_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "discover_s1_tests.rs"]
mod s1_tests;

// Append inside analysis/agent/discover_parallel.rs; test-only, no transport.
#[cfg(test)]
pub(crate) async fn profile_all_pack_requests<J: Journal>(
    input: &FrozenInput,
    config: &Config,
    planned: &Checkpoint,
    journal: &J,
    output_directory: &std::path::Path,
) -> Result<Vec<Value>, AgentError> {
    use sha2::{Digest, Sha256};
    let ids = planned
        .outline_run
        .reading_packs
        .as_ref()
        .ok_or_else(|| invalid("offline profile requires an existing plan"))?
        .pack_ids();
    if !output_directory.is_dir() {
        return Err(invalid("private output directory must already exist"));
    }
    let mut rows = Vec::new();
    for id in &ids {
        let mut state = planned.clone();
        let work = state.outline_run.reading_packs.as_mut().unwrap();
        if ids.iter().any(|id| {
            !matches!(
                work.status(id),
                Some(PackStatus::Pending | PackStatus::Running)
            )
        }) {
            return Err(invalid(
                "offline first-request profile requires an unsubmitted plan",
            ));
        }
        // Running claims are preserved; only Pending packs receive first claims.
        // This disposable projection snapshot is never saved or dispatched.
        work.claim(ids.len());
        state
            .outline_run
            .discover_workers
            .insert(id.clone(), Worker::default());
        let cancel = CancellationToken::new();
        let mut host = PackHost {
            input,
            config,
            state: &mut state,
            journal,
            cancel: &cancel,
        };
        let prepare_profile = super::profile::enter("offline_pack_prepare");
        let request = host.prepare_inner(id, false).await?;
        drop(prepare_profile);
        let body: Value = serde_json::from_slice(&request.body).map_err(invalid)?;
        let pack = body["messages"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|message| message["content"].as_str())
            .filter_map(|content| serde_json::from_str::<Value>(content).ok())
            .find_map(|value| {
                value
                    .get("reading_packs")?
                    .as_array()?
                    .first()?
                    .get("pack")
                    .cloned()
            })
            .ok_or_else(|| invalid("offline request pack payload missing"))?;
        let wire_path = output_directory.join(format!(
            "profile-pack-{}-request.json",
            pack["order"]
                .as_u64()
                .ok_or_else(|| invalid("pack order missing"))?
        ));
        std::fs::write(&wire_path, &request.body).map_err(invalid)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&wire_path, std::fs::Permissions::from_mode(0o600))
                .map_err(invalid)?;
        }
        rows.push(json!({
            "pack_id":id,"order":pack["order"],
            "atoms":pack["atoms"].as_array().map(Vec::len),
            "input_estimate":request.input_reserve,"output_reserve":request.output_reserve,
            "request_bytes":request.body.len(),
            "private_wire_file":wire_path.file_name().and_then(|name|name.to_str()),
            "request_sha256":hex::encode(Sha256::digest(&request.body)),
            "initial_full_request":true,"provider_called":false
        }));
    }
    rows.sort_by_key(|row| row["order"].as_u64().unwrap_or(u64::MAX));
    Ok(rows)
}
