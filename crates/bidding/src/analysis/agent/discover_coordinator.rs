//! Bounded Discover I/O. One coordinator owns durable and canonical mutations;
//! in-flight futures own only an immutable per-pack request, never a Checkpoint.
use super::{AgentError, ChatTurn};
use async_trait::async_trait;
use futures::{StreamExt, stream::FuturesUnordered};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use tokio_util::sync::CancellationToken;

pub(super) const WORKERS: usize = 2;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Scope {
    pub run: String,
    pub pack: String,
    pub revision: u64,
    pub generation: u64,
    pub request: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Request {
    pub scope: Scope,
    pub body: Vec<u8>,
    pub input_reserve: u64,
    pub output_reserve: u64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Pending {
    Prepared { request: Request },
    Sending { request: Request },
    Received { request: Request, response: Value },
    Committed { request: Request, receipt: Value },
    Failed { request: Request, code: String },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RelatedReadFrame {
    pub call_id: String,
    pub pack_revision: u64,
    pub input_digest: String,
    pub options: Vec<crate::outline::discover::ConditionSupport>,
    pub sealed_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RelatedContinuation {
    pub run: String,
    pub pack: String,
    pub pack_revision: u64,
    pub input_digest: String,
    pub refs: Vec<crate::outline::evidence::EvidenceRef>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Worker {
    pub history: Vec<Value>,
    pub pending: Option<Pending>,
    pub generation: u64,
    pub registry: crate::outline::model_wire::Registry,
    pub rejected_turns: usize,
    pub images: Vec<(String, String)>,
    pub source_keys: crate::outline::source_wire::Keys,
    pub related_read_frames: Vec<RelatedReadFrame>,
    pub related_continuations: std::collections::BTreeMap<String, RelatedContinuation>,
}
#[async_trait]
pub(super) trait Host: Send {
    /// Replay received results through the same typed commit path; never send.
    async fn replay_received(&mut self) -> Result<(), AgentError>;
    /// Return a durable prepared job excluding all requests currently in flight.
    async fn next(&mut self, active: &BTreeSet<String>) -> Result<Option<Request>, AgentError>;
    /// Atomic shared admission/reservation and Sending save, before dispatch.
    async fn reserve(&mut self, request: &Request) -> Result<(), AgentError>;
    /// Save the response before canonical validation and commit.
    async fn received(&mut self, request: &Request, response: &ChatTurn) -> Result<(), AgentError>;
    /// Serialized typed validation. Rejection is isolated to this worker.
    async fn commit(&mut self, request: &Request, response: ChatTurn) -> Result<(), AgentError>;
    async fn failed(&mut self, request: &Request, error: &AgentError) -> Result<(), AgentError>;
    /// Mark all dispatched but unresolved requests unknown; keep reservations.
    async fn cancelled(&mut self, active: &BTreeSet<String>) -> Result<(), AgentError>;
    fn all_committed(&self) -> bool;
}
#[async_trait]
pub(super) trait Provider: Sync {
    async fn call(&self, request: &Request) -> Result<ChatTurn, AgentError>;
}
pub(super) async fn drive<H: Host, P: Provider>(
    host: &mut H,
    provider: &P,
    cancel: &CancellationToken,
) -> Result<bool, AgentError> {
    host.replay_received().await?;
    let mut active = BTreeSet::new();
    let mut calls = FuturesUnordered::new();
    let mut admission_error = None;
    loop {
        if cancel.is_cancelled() {
            host.cancelled(&active).await?;
            return Err(AgentError::new(
                "AGENT_CANCELLED",
                "Discover cancelled; unresolved sends retain their reservations",
            ));
        }
        while active.len() < WORKERS && admission_error.is_none() {
            if cancel.is_cancelled() {
                break;
            }
            let request = match host.next(&active).await {
                Ok(Some(request)) => request,
                Ok(None) => break,
                Err(error) => {
                    admission_error = Some(error);
                    break;
                }
            };
            if active.contains(&request.scope.request) {
                return Err(AgentError::new(
                    "AGENT_OUTPUT_INVALID",
                    "duplicate active Discover request",
                ));
            }
            if let Err(error) = host.reserve(&request).await {
                admission_error = Some(error);
                break;
            }
            active.insert(request.scope.request.clone());
            calls.push(async move {
                let response = provider.call(&request).await;
                (request, response)
            });
        }
        if calls.is_empty() {
            if let Some(error) = admission_error {
                return Err(error);
            }
            return Ok(host.all_committed());
        }
        let result = tokio::select! {
            biased;
            _=cancel.cancelled()=>{host.cancelled(&active).await?;return Err(AgentError::new("AGENT_CANCELLED","Discover cancelled; unresolved sends retain reservations"));},
            result=calls.next()=>result.expect("inflight request"),
        };
        let (request, response) = result;
        active.remove(&request.scope.request);
        match response {
            Ok(response) => {
                host.received(&request, &response).await?;
                host.commit(&request, response).await?;
            }
            Err(error) => host.failed(&request, &error).await?,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::BTreeMap,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };
    struct FakeProvider {
        live: Arc<AtomicUsize>,
        peak: Arc<AtomicUsize>,
        calls: Arc<AtomicUsize>,
    }
    #[async_trait]
    impl Provider for FakeProvider {
        async fn call(&self, request: &Request) -> Result<ChatTurn, AgentError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let n = self.live.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(n, Ordering::SeqCst);
            struct DropLive(Arc<AtomicUsize>);
            impl Drop for DropLive {
                fn drop(&mut self) {
                    self.0.fetch_sub(1, Ordering::SeqCst);
                }
            }
            let _live = DropLive(self.live.clone());
            tokio::time::sleep(Duration::from_millis(if request.scope.pack == "a" {
                40
            } else {
                5
            }))
            .await;
            if request.scope.pack == "bad" {
                Err(AgentError::new("TEST_FAILURE", "isolated"))
            } else {
                Ok(ChatTurn::default())
            }
        }
    }
    #[derive(Default)]
    struct FakeHost {
        workers: BTreeMap<String, Worker>,
        reserves: usize,
        limit: usize,
        commits: Vec<String>,
        unknown: usize,
    }
    impl FakeHost {
        fn new(ids: &[&str]) -> Self {
            Self {
                limit: 99,
                workers: ids
                    .iter()
                    .map(|id| {
                        let scope = Scope {
                            run: "run".into(),
                            pack: (*id).into(),
                            revision: 1,
                            generation: 1,
                            request: format!("job-{id}"),
                        };
                        (
                            (*id).into(),
                            Worker {
                                pending: Some(Pending::Prepared {
                                    request: Request {
                                        scope,
                                        body: vec![],
                                        input_reserve: 10,
                                        output_reserve: 2,
                                    },
                                }),
                                ..Default::default()
                            },
                        )
                    })
                    .collect(),
                ..Default::default()
            }
        }
    }
    #[async_trait]
    impl Host for FakeHost {
        async fn replay_received(&mut self) -> Result<(), AgentError> {
            let items = self
                .workers
                .values()
                .filter_map(|w| match &w.pending {
                    Some(Pending::Received { request, response }) => Some((
                        request.clone(),
                        serde_json::from_value(response.clone()).unwrap(),
                    )),
                    _ => None,
                })
                .collect::<Vec<_>>();
            for (r, v) in items {
                self.commit(&r, v).await?;
            }
            Ok(())
        }
        async fn next(&mut self, _: &BTreeSet<String>) -> Result<Option<Request>, AgentError> {
            Ok(self.workers.values().find_map(|w| match &w.pending {
                Some(Pending::Prepared { request }) => Some(request.clone()),
                _ => None,
            }))
        }
        async fn reserve(&mut self, r: &Request) -> Result<(), AgentError> {
            if self.reserves >= self.limit {
                return Err(AgentError::new("BUDGET", "shared admission"));
            }
            self.reserves += 1;
            self.workers.get_mut(&r.scope.pack).unwrap().pending =
                Some(Pending::Sending { request: r.clone() });
            Ok(())
        }
        async fn received(&mut self, r: &Request, v: &ChatTurn) -> Result<(), AgentError> {
            self.workers.get_mut(&r.scope.pack).unwrap().pending = Some(Pending::Received {
                request: r.clone(),
                response: serde_json::to_value(v).unwrap(),
            });
            Ok(())
        }
        async fn commit(&mut self, r: &Request, _: ChatTurn) -> Result<(), AgentError> {
            self.commits.push(r.scope.pack.clone());
            self.workers.get_mut(&r.scope.pack).unwrap().pending = Some(Pending::Committed {
                request: r.clone(),
                receipt: Value::Null,
            });
            Ok(())
        }
        async fn failed(&mut self, r: &Request, e: &AgentError) -> Result<(), AgentError> {
            self.workers.get_mut(&r.scope.pack).unwrap().pending = Some(Pending::Failed {
                request: r.clone(),
                code: e.code.clone(),
            });
            Ok(())
        }
        async fn cancelled(&mut self, active: &BTreeSet<String>) -> Result<(), AgentError> {
            self.unknown += active.len();
            Ok(())
        }
        fn all_committed(&self) -> bool {
            self.workers
                .values()
                .all(|w| matches!(w.pending, Some(Pending::Committed { .. })))
        }
    }
    fn provider() -> FakeProvider {
        FakeProvider {
            live: Arc::default(),
            peak: Arc::default(),
            calls: Arc::default(),
        }
    }
    #[tokio::test]
    async fn overlap_out_of_order_and_failure_isolation() {
        let mut host = FakeHost::new(&["a", "b", "bad"]);
        let p = provider();
        assert!(
            !drive(&mut host, &p, &CancellationToken::new())
                .await
                .unwrap()
        );
        assert_eq!(p.peak.load(Ordering::SeqCst), 2);
        assert_eq!(host.commits, vec!["b", "a"]);
        assert_eq!(host.reserves, 3);
    }
    #[tokio::test]
    async fn replay_received_never_calls_provider() {
        let mut host = FakeHost::new(&["a"]);
        let Some(Pending::Prepared { request }) = host.workers["a"].pending.clone() else {
            panic!()
        };
        host.received(&request, &ChatTurn::default()).await.unwrap();
        let p = provider();
        assert!(
            drive(&mut host, &p, &CancellationToken::new())
                .await
                .unwrap()
        );
        assert_eq!(p.calls.load(Ordering::SeqCst), 0);
        assert_eq!(host.reserves, 0);
    }
    #[tokio::test]
    async fn shared_budget_drains_prior_send_and_never_sends_denied_job() {
        let mut host = FakeHost::new(&["a", "b"]);
        host.limit = 1;
        let p = provider();
        assert!(
            drive(&mut host, &p, &CancellationToken::new())
                .await
                .is_err()
        );
        assert_eq!(p.calls.load(Ordering::SeqCst), 1);
        assert_eq!(host.commits, vec!["a"]);
    }
    #[tokio::test(start_paused = true)]
    async fn cancellation_preserves_unknown_reservations() {
        let mut host = FakeHost::new(&["a", "b"]);
        let p = provider();
        let cancel = CancellationToken::new();
        let trigger = cancel.clone();
        let task = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(1)).await;
            trigger.cancel();
        });
        assert!(drive(&mut host, &p, &cancel).await.is_err());
        task.await.unwrap();
        assert_eq!(host.reserves, 2);
        assert_eq!(host.unknown, 2);
        assert_eq!(p.live.load(Ordering::SeqCst), 0);
    }
}
