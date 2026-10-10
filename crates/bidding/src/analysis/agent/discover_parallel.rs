//! Production adapter for the bounded Discover coordinator.
use super::discover_coordinator::{
    self as coordinator, Host, Pending, Provider, Request, Scope, Worker,
};
use super::*;
use crate::outline::{agent::Duty, discover::PackStatus};
use serde_json::json;
use std::collections::BTreeSet;

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
    async fn related_page(
        &mut self,
        request: &Request,
        response: &ChatTurn,
        args: &Value,
    ) -> Result<Value, String> {
        use crate::outline::evidence::{self, EvidenceRef};
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
            value["new_coverage"] = json!(!work.related_read_already_delivered(id, &options));
            value["next_cursor"] = json!(next);
            value["selection_complete"] = json!(tail.is_empty());
            let result = json!({"status":"read_pending_delivery","read":value});
            let call = &response.tool_calls[0];
            let frame = coordinator::RelatedReadFrame {
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
                Err(error) if error.code == "AGENT_TURN_BUDGET_EXCEEDED" => {
                    evidence::shrink_selection(self.input, &mut selected)?
                }
                Err(error) => return Err(error.message),
            }
        }
    }

    async fn prepare(&mut self, id: &str) -> Result<Request, AgentError> {
        self.prepare_inner(id, true).await
    }
    async fn prepare_inner(&mut self, id: &str, persist: bool) -> Result<Request, AgentError> {
        let work = self
            .state
            .outline_run
            .reading_packs
            .as_ref()
            .ok_or_else(|| invalid("reading plan missing"))?;
        let identity = work.pack_wire_identity(id).map_err(invalid)?;
        let session = work.session(self.input, id).map_err(invalid)?;
        let image_ids = session["pack"]["atoms"]
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
                .insert(view.identity.image_sha256.clone(), view.clone());
            views.push(view);
        }
        let worker = self.worker(id)?;
        let generation = worker
            .generation
            .checked_add(1)
            .ok_or_else(|| invalid("worker generation overflow"))?;
        let mut messages = vec![
            json!({"role":"system","content":format!("{}\nThis worker owns exactly one pack. Submit only that pack using submit_pack; all required original images are attached. Do not select another pack.",Duty::Discover.instructions())}),
            json!({"role":"user","content":json!({"reading_packs":[session], "semantic_contract":crate::outline::agent::semantic_guidance(Duty::Discover), "repair_context":self.state.outline_run.repair_events.iter().rev().find(|event|event["pack_id"]==id && event["status"]=="reopened" && event["pack_revision"]==identity["revision"])}).to_string()}),
        ];
        messages.push(json!({"role":"user","content":json!({"dependency_progress":{"pending_cursors":worker.related_continuations.keys().collect::<Vec<_>>(),"policy":"Only completed exact original reads become durable coverage. Delivered tool payloads may be retired; preserve task-relevant observations in assistant content. Granted support handles remain in the owned session. Follow pending cursors until selection_complete; reread original source keys when needed. No dependency grants ownership or completion of another pack."}}).to_string()}));
        messages.extend(worker.history.iter().cloned());
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
        let source_keys =
            crate::outline::source_wire::request(self.state, &mut body).map_err(invalid)?;
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
        if count.total_context_tokens > self.config.limits.max_context_tokens {
            return Err(error(
                "AGENT_TURN_BUDGET_EXCEEDED",
                "worker final wire exceeds context; no send",
            ));
        }
        let mut read_frames = worker.related_read_frames.clone();
        for frame in &mut read_frames {
            frame.sealed_sha256 =
                crate::outline::read_receipts::wire_hash(&body, &frame.call_id).map_err(invalid)?;
        }
        let body = serde_json_canonicalizer::to_vec(&body).map_err(invalid)?;
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
            .map(|view| {
                (
                    view.identity.source_id.clone(),
                    view.identity.image_sha256.clone(),
                )
            })
            .collect();
        worker.pending = Some(Pending::Prepared {
            request: request.clone(),
        });
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
        let work = self
            .state
            .outline_run
            .reading_packs
            .as_mut()
            .ok_or_else(|| invalid("reading plan missing"))?;
        work.claim(coordinator::WORKERS);
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
            if let Some(Pending::Committed { request, .. }) = &worker.pending {
                if request.scope.revision != identity["revision"].as_u64().unwrap() {
                    *worker = Worker::default();
                } else {
                    continue;
                }
            }
            match &worker.pending {
                Some(Pending::Prepared { request }) => return Ok(Some(request.clone())),
                Some(Pending::Sending { request }) if active.contains(&request.scope.request) => {
                    continue;
                }
                Some(Pending::Sending { .. } | Pending::Received { .. }) => {
                    return Err(invalid("unresolved worker journal"));
                }
                Some(Pending::Failed { code, .. }) if code != "BUSINESS_REJECTED" => continue,
                _ => (),
            }
            if worker.rejected_turns >= self.config.limits.max_no_progress_turns {
                continue;
            }
            return self.prepare(&id).await.map(Some);
        }
        Ok(None)
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
        let outcome = (|| -> Result<Value, String> {
            if !crate::agent_runtime::valid_tool_turn(&response)
                || response.tool_calls.len() != 1
                || !matches!(
                    response.tool_calls[0].name.as_str(),
                    "submit_pack" | "read_evidence"
                )
            {
                return Err(
                    "worker must read a confirmed dependency or submit exactly one owned pack"
                        .into(),
                );
            }
            let call = &response.tool_calls[0];
            let args: Value = serde_json::from_str(&call.arguments).map_err(|e| e.to_string())?;
            let args = self.state.outline_run.discover_workers[id]
                .registry
                .decode(args, false)?;
            if call.name == "submit_pack" && args["pack_id"] != *id {
                return Err("cross-pack worker submission".into());
            }
            let identity = self
                .state
                .outline_run
                .reading_packs
                .as_ref()
                .unwrap()
                .pack_wire_identity(id)?;
            if identity["run"] != request.scope.run
                || identity["revision"] != request.scope.revision
            {
                return Err("stale worker revision".into());
            }
            confirm_worker_related_reads(self.input, self.state, request)?;
            if call.name == "read_evidence" {
                let keys = &self.state.outline_run.discover_workers[id].source_keys;
                let args = crate::outline::source_wire::resolve_with_keys(
                    self.state,
                    "read_evidence",
                    args,
                    keys,
                )?;
                return Ok(json!({"status":"read_request","args":args}));
            }
            crate::outline::agent::validate_arguments("submit_pack", &args)?;
            confirm_worker_images(self.input, self.config, self.state, request)?;
            crate::outline::agent::apply_for_duty(
                self.input,
                self.state,
                "submit_pack",
                &args,
                Duty::Discover,
            )
        })();
        let outcome = match outcome {
            Ok(value) if value["status"] == "read_request" => {
                self.related_page(request, &response, &value["args"]).await
            }
            other => other,
        };
        let receipt = match outcome {
            Ok(value) => super::tool_result_envelope(value),
            Err(message) => json!({"ok":false,"error":message}),
        };
        let committed = receipt["ok"] == true && receipt["result"]["status"] == "committed";
        let worker = self.worker_mut(id)?;
        worker.history.push(json!({"role":"assistant","content":response.content,"tool_calls":response.tool_calls.iter().map(|call|json!({"id":call.id,"type":"function","function":{"name":call.name,"arguments":call.arguments}})).collect::<Vec<_>>()}));
        for call in &response.tool_calls {
            worker
                .history
                .push(json!({"role":"tool","tool_call_id":call.id,"content":receipt.to_string()}));
        }
        if receipt["ok"] == true && receipt["result"]["status"] == "read_pending_delivery" {
            if receipt["result"]["read"]["new_coverage"] == false {
                worker.rejected_turns += 1;
            } else {
                worker.rejected_turns = 0;
            }
            worker.pending = None;
        } else if committed {
            worker.pending = Some(Pending::Committed {
                request: request.clone(),
                receipt,
            });
        } else {
            worker.rejected_turns += 1;
            worker.pending = Some(Pending::Failed {
                request: request.clone(),
                code: "BUSINESS_REJECTED".into(),
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
        self.worker_mut(&request.scope.pack)?.pending = Some(Pending::Failed {
            request: request.clone(),
            code: error.code.clone(),
        });
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

fn confirm_worker_related_reads(
    input: &FrozenInput,
    state: &mut Checkpoint,
    request: &Request,
) -> Result<(), String> {
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
    for frame in &worker.related_read_frames {
        if frame.pack_revision != request.scope.revision || frame.input_digest != state.input_sha256
        {
            return Err("related read frame belongs to another input or pack revision".into());
        }
        if frame.sealed_sha256.is_some()
            && crate::outline::read_receipts::wire_hash(&body, &frame.call_id)?
                == frame.sealed_sha256
        {
            work.confirm_related_read(input, &request.scope.pack, &frame.options)?;
            delivered_calls.insert(frame.call_id.clone());
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
    worker.history = retire_delivered_related_history(&worker.history, &delivered_calls);
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
        if !urls.contains(format!("data:image/jpeg;base64,{}", view.jpeg_base64).as_str()) {
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
