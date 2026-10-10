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
    async fn prepare(&mut self, id: &str) -> Result<Request, AgentError> {
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
        messages.extend(worker.history.iter().cloned());
        for view in &views {
            messages.push(view.message());
        }
        messages.push(json!({"role":"user","content":json!({"worker_pack":id,"generation":generation}).to_string()}));
        let tools = crate::outline::agent::schemas_for(Duty::Discover)
            .into_iter()
            .filter(|tool| tool["function"]["name"] == "submit_pack")
            .collect();
        let bytes =
            crate::agent_runtime::chat::prepare(&self.config.provider, messages, tools).await?;
        let mut body: Value = serde_json::from_slice(&bytes).map_err(invalid)?;
        crate::outline::discover::compact_request(&mut body).map_err(invalid)?;
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
            return Err(invalid("worker final wire exceeds context; no send"));
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
        self.save().await?;
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
                || response.tool_calls[0].name != "submit_pack"
            {
                return Err("worker must submit exactly one complete owned pack".into());
            }
            let call = &response.tool_calls[0];
            let args: Value = serde_json::from_str(&call.arguments).map_err(|e| e.to_string())?;
            let args = self.state.outline_run.discover_workers[id]
                .registry
                .decode(args, false)?;
            if args["pack_id"] != *id {
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
        if committed {
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
