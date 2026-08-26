use oxana::{Job, JobConflictStrategy, Queue as _, Worker as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    error::Error,
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};
use uuid::Uuid;

use crate::{BidConvertV1Queue, BidExtractV1Queue, BidMatchingV1Queue, BidRenderV1Queue};

pub const BID_DELIVERY_V1_TASK_TYPE: &str = "bid:delivery:v1";
pub const BID_DELIVERY_V1_PAYLOAD_VERSION: u16 = 1;
pub const BID_DELIVERY_V1_MAX_OBSERVED_JOB_ID_BYTES: usize = 256;

const BID_DELIVERY_V1_SCHEMA_VERSION: u16 = 1;
const BID_DELIVERY_V1_MAGIC: &[u8; 4] = b"KBDL";
const BID_DELIVERY_V1_PAYLOAD_LENGTH: usize = 24;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedJobIdObservation {
    value: String,
    original_byte_len: usize,
    sha256: String,
    truncated: bool,
}

impl BoundedJobIdObservation {
    pub fn new(actual_job_id: &str) -> Self {
        let original_byte_len = actual_job_id.len();
        let sha256 = lowercase_sha256(actual_job_id.as_bytes());
        if original_byte_len <= BID_DELIVERY_V1_MAX_OBSERVED_JOB_ID_BYTES {
            return Self {
                value: actual_job_id.to_string(),
                original_byte_len,
                sha256,
                truncated: false,
            };
        }

        let mut end = BID_DELIVERY_V1_MAX_OBSERVED_JOB_ID_BYTES;
        while !actual_job_id.is_char_boundary(end) {
            end -= 1;
        }
        Self {
            value: actual_job_id[..end].to_string(),
            original_byte_len,
            sha256,
            truncated: true,
        }
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn original_byte_len(&self) -> usize {
        self.original_byte_len
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    pub fn truncated(&self) -> bool {
        self.truncated
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BidDeliveryPayloadError {
    Length,
    Magic,
    SchemaVersion,
    DispatchId,
    PayloadVersion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BidDeliveryV1Job {
    pub dispatch_id: Uuid,
    pub payload_version: u16,
}

impl BidDeliveryV1Job {
    pub fn new(dispatch_id: Uuid) -> Self {
        Self {
            dispatch_id,
            payload_version: BID_DELIVERY_V1_PAYLOAD_VERSION,
        }
    }
}

impl Job for BidDeliveryV1Job {
    fn name() -> &'static str {
        BID_DELIVERY_V1_TASK_TYPE
    }

    fn unique_id(&self) -> Option<String> {
        Some(self.dispatch_id.hyphenated().to_string())
    }

    fn on_conflict(&self) -> JobConflictStrategy {
        JobConflictStrategy::Skip
    }

    fn should_resurrect() -> bool {
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliverySpec {
    pub physical_lane: String,
    pub task_type: String,
    pub dispatch_id: Uuid,
    pub payload_version: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PhysicalLane {
    Convert,
    Extract,
    Matching,
    Render,
}

impl PhysicalLane {
    fn parse(value: &str) -> Option<Self> {
        match value {
            domain::QUEUE_BID_CONVERT_V1 => Some(Self::Convert),
            domain::QUEUE_BID_EXTRACT_V1 => Some(Self::Extract),
            domain::QUEUE_BID_MATCHING_V1 => Some(Self::Matching),
            domain::QUEUE_BID_RENDER_V1 => Some(Self::Render),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrepareDeliveryError {
    PayloadRejected(&'static str),
    AdapterMismatch(&'static str),
}

#[derive(Debug, Clone)]
pub struct PreparedDelivery {
    lane: PhysicalLane,
    job: BidDeliveryV1Job,
    expected_job_id: String,
    canonical_payload_bytes: Vec<u8>,
    canonical_payload_sha256: String,
}

impl PreparedDelivery {
    pub fn expected_job_id(&self) -> &str {
        &self.expected_job_id
    }

    pub fn canonical_payload_bytes(&self) -> &[u8] {
        &self.canonical_payload_bytes
    }

    pub fn canonical_payload_sha256(&self) -> &str {
        &self.canonical_payload_sha256
    }

    pub fn resurrect(&self) -> bool {
        BidDeliveryV1Job::should_resurrect()
    }

    pub fn on_conflict(&self) -> JobConflictStrategy {
        self.job.on_conflict()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportErrorClass {
    DeadlineExceeded,
    RedisUnavailable,
    EnqueueFailed,
    AdapterMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportOutcome {
    Returned {
        job_id: String,
    },
    Indeterminate {
        error_class: TransportErrorClass,
    },
    ReturnedJobIdMismatch {
        actual_job_id: BoundedJobIdObservation,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WorkTransportReadiness {
    #[default]
    Ready,
    Degraded,
    FailedClosed,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TransportMetricsSnapshot {
    pub offers_created: u64,
    pub offers_started: u64,
    pub enqueue_attempts: u64,
    pub returned: u64,
    pub indeterminate: u64,
    pub deadline_before_enqueue: u64,
    pub timeout_after_start: u64,
    pub redis_unavailable: u64,
    pub enqueue_failed: u64,
    pub returned_job_id_mismatch: u64,
    pub registry_closure_mismatch: u64,
    pub total_latency_micros: u64,
    pub last_latency_micros: u64,
}

#[derive(Default)]
struct TransportState {
    readiness: WorkTransportReadiness,
    metrics: TransportMetricsSnapshot,
}

#[derive(Default)]
struct TransportHealth(Mutex<TransportState>);

impl TransportHealth {
    fn readiness(&self) -> WorkTransportReadiness {
        self.0
            .lock()
            .expect("transport health lock poisoned")
            .readiness
    }

    fn update(&self, update: impl FnOnce(&mut TransportState)) {
        update(&mut self.0.lock().expect("transport health lock poisoned"));
    }

    fn degrade(state: &mut TransportState) {
        if state.readiness == WorkTransportReadiness::Ready {
            state.readiness = WorkTransportReadiness::Degraded;
        }
    }

    fn recover(state: &mut TransportState) {
        if state.readiness == WorkTransportReadiness::Degraded {
            state.readiness = WorkTransportReadiness::Ready;
        }
    }

    fn snapshot(&self) -> TransportMetricsSnapshot {
        self.0
            .lock()
            .expect("transport health lock poisoned")
            .metrics
    }
}

pub type WorkTransportFuture<'a> = Pin<Box<dyn Future<Output = TransportOutcome> + Send + 'a>>;

pub trait WorkTransport: Send + Sync {
    fn offer<'a>(
        &'a self,
        prepared: &'a PreparedDelivery,
        deadline: Instant,
    ) -> WorkTransportFuture<'a>;

    fn readiness(&self) -> WorkTransportReadiness;

    fn metrics_snapshot(&self) -> TransportMetricsSnapshot;
}

#[derive(Clone)]
pub struct OxanaStableAdapter {
    storage: oxana::Storage,
    health: Arc<TransportHealth>,
}

impl OxanaStableAdapter {
    pub fn new(storage: oxana::Storage) -> Result<Self, RegistryClosureFailure> {
        Self::with_registry_result(storage, validate_bid_delivery_v1_registry())
    }

    fn with_registry_result(
        storage: oxana::Storage,
        registry_result: Result<(), RegistryClosureError>,
    ) -> Result<Self, RegistryClosureFailure> {
        registry_result.map_err(RegistryClosureFailure::new)?;
        Ok(Self {
            storage,
            health: Arc::new(TransportHealth::default()),
        })
    }

    pub async fn diagnostics_snapshot(
        &self,
    ) -> Result<TransportDiagnosticsSnapshot, TransportErrorClass> {
        let convert_depth = self
            .storage
            .enqueued_count(BidConvertV1Queue)
            .await
            .map_err(|error| classify_oxana_error(&error))?;
        let extract_depth = self
            .storage
            .enqueued_count(BidExtractV1Queue)
            .await
            .map_err(|error| classify_oxana_error(&error))?;
        let matching_depth = self
            .storage
            .enqueued_count(BidMatchingV1Queue)
            .await
            .map_err(|error| classify_oxana_error(&error))?;
        let render_depth = self
            .storage
            .enqueued_count(BidRenderV1Queue)
            .await
            .map_err(|error| classify_oxana_error(&error))?;
        let dead_count = self
            .storage
            .dead_count()
            .await
            .map_err(|error| classify_oxana_error(&error))?;
        Ok(TransportDiagnosticsSnapshot {
            convert_depth,
            extract_depth,
            matching_depth,
            render_depth,
            dead_count,
            resurrection_enabled: BidDeliveryV1Job::should_resurrect(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransportDiagnosticsSnapshot {
    pub convert_depth: usize,
    pub extract_depth: usize,
    pub matching_depth: usize,
    pub render_depth: usize,
    pub dead_count: usize,
    pub resurrection_enabled: bool,
}

impl WorkTransport for OxanaStableAdapter {
    fn offer<'a>(
        &'a self,
        prepared: &'a PreparedDelivery,
        deadline: Instant,
    ) -> WorkTransportFuture<'a> {
        self.health
            .update(|state| increment(&mut state.metrics.offers_created, 1));
        Box::pin(offer_once(
            prepared,
            deadline,
            &self.health,
            move || async move {
                let job = prepared.job.clone();
                let result = match prepared.lane {
                    PhysicalLane::Convert => self.storage.enqueue(BidConvertV1Queue, job).await,
                    PhysicalLane::Extract => self.storage.enqueue(BidExtractV1Queue, job).await,
                    PhysicalLane::Matching => self.storage.enqueue(BidMatchingV1Queue, job).await,
                    PhysicalLane::Render => self.storage.enqueue(BidRenderV1Queue, job).await,
                };
                result.map_err(|error| classify_oxana_error(&error))
            },
        ))
    }

    fn readiness(&self) -> WorkTransportReadiness {
        self.health.readiness()
    }

    fn metrics_snapshot(&self) -> TransportMetricsSnapshot {
        self.health.snapshot()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordingResponse {
    ReturnExpected,
    ReturnJobId(String),
    Error(TransportErrorClass),
    Pending,
}

pub struct RecordingTransport {
    response: Mutex<RecordingResponse>,
    enqueue_count: AtomicUsize,
    health: Arc<TransportHealth>,
}

impl RecordingTransport {
    pub fn new(response: RecordingResponse) -> Self {
        Self {
            response: Mutex::new(response),
            enqueue_count: AtomicUsize::new(0),
            health: Arc::new(TransportHealth::default()),
        }
    }

    pub fn enqueue_count(&self) -> usize {
        self.enqueue_count.load(Ordering::SeqCst)
    }

    pub fn set_response(&self, response: RecordingResponse) {
        *self
            .response
            .lock()
            .expect("recording response lock poisoned") = response;
    }
}

impl WorkTransport for RecordingTransport {
    fn offer<'a>(
        &'a self,
        prepared: &'a PreparedDelivery,
        deadline: Instant,
    ) -> WorkTransportFuture<'a> {
        self.health
            .update(|state| increment(&mut state.metrics.offers_created, 1));
        Box::pin(offer_once(
            prepared,
            deadline,
            &self.health,
            move || async move {
                self.enqueue_count.fetch_add(1, Ordering::SeqCst);
                let response = self
                    .response
                    .lock()
                    .expect("recording response lock poisoned")
                    .clone();
                match response {
                    RecordingResponse::ReturnExpected => Ok(prepared.expected_job_id.clone()),
                    RecordingResponse::ReturnJobId(job_id) => Ok(job_id),
                    RecordingResponse::Error(error_class) => Err(error_class),
                    RecordingResponse::Pending => std::future::pending().await,
                }
            },
        ))
    }

    fn readiness(&self) -> WorkTransportReadiness {
        self.health.readiness()
    }

    fn metrics_snapshot(&self) -> TransportMetricsSnapshot {
        self.health.snapshot()
    }
}

async fn offer_once<Enqueue, EnqueueFuture>(
    prepared: &PreparedDelivery,
    deadline: Instant,
    health: &TransportHealth,
    enqueue_once: Enqueue,
) -> TransportOutcome
where
    Enqueue: FnOnce() -> EnqueueFuture,
    EnqueueFuture: Future<Output = Result<String, TransportErrorClass>>,
{
    health.update(|state| increment(&mut state.metrics.offers_started, 1));
    let started_at = Instant::now();
    if health.readiness() == WorkTransportReadiness::FailedClosed {
        health.update(|state| increment(&mut state.metrics.indeterminate, 1));
        record_latency(health, started_at);
        return TransportOutcome::Indeterminate {
            error_class: TransportErrorClass::AdapterMismatch,
        };
    }
    if Instant::now() >= deadline {
        health.update(|state| {
            increment(&mut state.metrics.indeterminate, 1);
            increment(&mut state.metrics.deadline_before_enqueue, 1);
        });
        record_latency(health, started_at);
        return TransportOutcome::Indeterminate {
            error_class: TransportErrorClass::DeadlineExceeded,
        };
    }

    health.update(|state| increment(&mut state.metrics.enqueue_attempts, 1));
    match tokio::time::timeout_at(deadline.into(), enqueue_once()).await {
        Err(_) => {
            health.update(|state| {
                increment(&mut state.metrics.indeterminate, 1);
                increment(&mut state.metrics.timeout_after_start, 1);
                TransportHealth::degrade(state);
            });
            record_latency(health, started_at);
            TransportOutcome::Indeterminate {
                error_class: TransportErrorClass::DeadlineExceeded,
            }
        }
        Ok(Err(error_class)) => {
            health.update(|state| {
                increment(&mut state.metrics.indeterminate, 1);
                match error_class {
                    TransportErrorClass::RedisUnavailable => {
                        increment(&mut state.metrics.redis_unavailable, 1)
                    }
                    TransportErrorClass::EnqueueFailed => {
                        increment(&mut state.metrics.enqueue_failed, 1)
                    }
                    TransportErrorClass::DeadlineExceeded => {}
                    TransportErrorClass::AdapterMismatch => {
                        state.readiness = WorkTransportReadiness::FailedClosed
                    }
                }
                if error_class != TransportErrorClass::AdapterMismatch {
                    TransportHealth::degrade(state);
                }
            });
            record_latency(health, started_at);
            TransportOutcome::Indeterminate { error_class }
        }
        Ok(Ok(job_id)) if job_id == prepared.expected_job_id => {
            health.update(|state| {
                increment(&mut state.metrics.returned, 1);
                TransportHealth::recover(state);
            });
            record_latency(health, started_at);
            TransportOutcome::Returned { job_id }
        }
        Ok(Ok(actual_job_id)) => {
            health.update(|state| {
                increment(&mut state.metrics.returned_job_id_mismatch, 1);
                state.readiness = WorkTransportReadiness::FailedClosed;
            });
            record_latency(health, started_at);
            TransportOutcome::ReturnedJobIdMismatch {
                actual_job_id: BoundedJobIdObservation::new(&actual_job_id),
            }
        }
    }
}

fn increment(counter: &mut u64, value: u64) {
    *counter = counter.saturating_add(value);
}

fn record_latency(health: &TransportHealth, started_at: Instant) {
    let micros = u64::try_from(started_at.elapsed().as_micros()).unwrap_or(u64::MAX);
    health.update(|state| {
        state.metrics.last_latency_micros = micros;
        increment(&mut state.metrics.total_latency_micros, micros);
    });
}

fn classify_oxana_error(error: &oxana::OxanaError) -> TransportErrorClass {
    match error {
        oxana::OxanaError::DeadpoolRedisError(_)
        | oxana::OxanaError::DeadpoolRedisPoolError(_)
        | oxana::OxanaError::DeadpoolRedisCreatePoolError(_)
        | oxana::OxanaError::DeadpoolRedisConfigError(_)
        | oxana::OxanaError::DeadpoolRedisBuildError(_) => TransportErrorClass::RedisUnavailable,
        _ => TransportErrorClass::EnqueueFailed,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedBidDeliveryV1 {
    pub dispatch_id: Uuid,
    pub payload_version: u16,
    pub observed_job_id: String,
}

#[async_trait::async_trait]
pub trait BidDeliveryV1Handler: Send + Sync {
    type Error: Error + Send + Sync + 'static;

    async fn handle(&self, delivery: ObservedBidDeliveryV1) -> Result<(), Self::Error>;
}

pub struct BidDeliveryV1WorkerAdapter<Handler> {
    handler: Handler,
}

impl<Handler> BidDeliveryV1WorkerAdapter<Handler> {
    pub fn new(handler: Handler) -> Self {
        Self { handler }
    }
}

#[async_trait::async_trait]
impl<Handler> oxana::Worker<BidDeliveryV1Job> for BidDeliveryV1WorkerAdapter<Handler>
where
    Handler: BidDeliveryV1Handler,
{
    type Error = Handler::Error;

    async fn process(
        &self,
        job: BidDeliveryV1Job,
        context: &oxana::JobContext,
    ) -> Result<(), Self::Error> {
        self.handler
            .handle(ObservedBidDeliveryV1 {
                dispatch_id: job.dispatch_id,
                payload_version: job.payload_version,
                observed_job_id: context.meta.id.clone(),
            })
            .await
    }

    fn max_retries(&self, _job: &BidDeliveryV1Job) -> u32 {
        0
    }
}

impl<Context, Handler> oxana::FromContext<Context> for BidDeliveryV1WorkerAdapter<Handler>
where
    Handler: oxana::FromContext<Context>,
{
    fn from_context(context: &Context) -> Self {
        Self::new(Handler::from_context(context))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryClosureError {
    Queue,
    TaskType,
    PayloadVersion,
    PayloadShape,
    ConflictStrategy,
    Resurrection,
    WorkerRetry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegistryClosureFailure {
    pub error: RegistryClosureError,
    pub readiness: WorkTransportReadiness,
    pub metrics: TransportMetricsSnapshot,
}

impl RegistryClosureFailure {
    fn new(error: RegistryClosureError) -> Self {
        let metrics = TransportMetricsSnapshot {
            registry_closure_mismatch: 1,
            ..TransportMetricsSnapshot::default()
        };
        Self {
            error,
            readiness: WorkTransportReadiness::FailedClosed,
            metrics,
        }
    }
}

struct RegistryObservation {
    lanes: [String; 4],
    task_type: &'static str,
    payload_version: u16,
    payload_fields: Vec<String>,
    on_conflict: JobConflictStrategy,
    resurrect: bool,
    worker_max_retries: u32,
}

#[derive(Debug)]
struct RegistryHandlerError;

impl std::fmt::Display for RegistryHandlerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("registry handler error")
    }
}

impl Error for RegistryHandlerError {}

struct RegistryHandler;

#[async_trait::async_trait]
impl BidDeliveryV1Handler for RegistryHandler {
    type Error = RegistryHandlerError;

    async fn handle(&self, _delivery: ObservedBidDeliveryV1) -> Result<(), Self::Error> {
        Ok(())
    }
}

fn actual_registry_observation() -> Result<RegistryObservation, RegistryClosureError> {
    let job = BidDeliveryV1Job::new(Uuid::nil());
    let mut payload_fields = serde_json::to_value(&job)
        .map_err(|_| RegistryClosureError::PayloadShape)?
        .as_object()
        .map(|object| object.keys().cloned().collect::<Vec<_>>())
        .ok_or(RegistryClosureError::PayloadShape)?;
    payload_fields.sort();
    let worker = BidDeliveryV1WorkerAdapter::new(RegistryHandler);
    Ok(RegistryObservation {
        lanes: [
            BidConvertV1Queue.key(),
            BidExtractV1Queue.key(),
            BidMatchingV1Queue.key(),
            BidRenderV1Queue.key(),
        ],
        task_type: BidDeliveryV1Job::name(),
        payload_version: job.payload_version,
        payload_fields,
        on_conflict: job.on_conflict(),
        resurrect: BidDeliveryV1Job::should_resurrect(),
        worker_max_retries: worker.max_retries(&job),
    })
}

fn verify_registry_observation(
    observation: &RegistryObservation,
) -> Result<(), RegistryClosureError> {
    if observation.lanes
        != [
            domain::QUEUE_BID_CONVERT_V1.to_string(),
            domain::QUEUE_BID_EXTRACT_V1.to_string(),
            domain::QUEUE_BID_MATCHING_V1.to_string(),
            domain::QUEUE_BID_RENDER_V1.to_string(),
        ]
    {
        return Err(RegistryClosureError::Queue);
    }
    if observation.task_type != BID_DELIVERY_V1_TASK_TYPE {
        return Err(RegistryClosureError::TaskType);
    }
    if observation.payload_version != BID_DELIVERY_V1_PAYLOAD_VERSION {
        return Err(RegistryClosureError::PayloadVersion);
    }
    if observation.payload_fields != ["dispatch_id".to_string(), "payload_version".to_string()] {
        return Err(RegistryClosureError::PayloadShape);
    }
    if observation.on_conflict != JobConflictStrategy::Skip {
        return Err(RegistryClosureError::ConflictStrategy);
    }
    if !observation.resurrect {
        return Err(RegistryClosureError::Resurrection);
    }
    if observation.worker_max_retries != 0 {
        return Err(RegistryClosureError::WorkerRetry);
    }
    Ok(())
}

pub fn validate_bid_delivery_v1_registry() -> Result<(), RegistryClosureError> {
    verify_registry_observation(&actual_registry_observation()?)
}

pub fn verify_bid_delivery_v1_payload(
    bytes: &[u8],
    expected_dispatch_id: Uuid,
) -> Result<(), BidDeliveryPayloadError> {
    if bytes.len() != BID_DELIVERY_V1_PAYLOAD_LENGTH {
        return Err(BidDeliveryPayloadError::Length);
    }
    if &bytes[..4] != BID_DELIVERY_V1_MAGIC {
        return Err(BidDeliveryPayloadError::Magic);
    }
    if u16::from_be_bytes([bytes[4], bytes[5]]) != BID_DELIVERY_V1_SCHEMA_VERSION {
        return Err(BidDeliveryPayloadError::SchemaVersion);
    }
    if &bytes[6..22] != expected_dispatch_id.as_bytes() {
        return Err(BidDeliveryPayloadError::DispatchId);
    }
    if u16::from_be_bytes([bytes[22], bytes[23]]) != BID_DELIVERY_V1_PAYLOAD_VERSION {
        return Err(BidDeliveryPayloadError::PayloadVersion);
    }
    Ok(())
}

pub fn prepare_bid_delivery_v1(
    spec: DeliverySpec,
) -> Result<PreparedDelivery, PrepareDeliveryError> {
    validate_bid_delivery_v1_registry()
        .map_err(|_| PrepareDeliveryError::AdapterMismatch("registry"))?;
    let lane = PhysicalLane::parse(&spec.physical_lane)
        .ok_or(PrepareDeliveryError::AdapterMismatch("physical_lane"))?;
    if spec.task_type != BID_DELIVERY_V1_TASK_TYPE {
        return Err(PrepareDeliveryError::PayloadRejected("task_type"));
    }
    if spec.payload_version != BID_DELIVERY_V1_PAYLOAD_VERSION {
        return Err(PrepareDeliveryError::PayloadRejected("payload_version"));
    }

    let job = BidDeliveryV1Job::new(spec.dispatch_id);
    let expected_job_id = format!(
        "{}/{}",
        BID_DELIVERY_V1_TASK_TYPE,
        spec.dispatch_id.hyphenated()
    );
    let canonical_payload_bytes = canonical_payload_bytes(spec.dispatch_id);
    verify_bid_delivery_v1_payload(&canonical_payload_bytes, spec.dispatch_id)
        .map_err(|_| PrepareDeliveryError::AdapterMismatch("codec"))?;
    let canonical_payload_sha256 = lowercase_sha256(&canonical_payload_bytes);

    Ok(PreparedDelivery {
        lane,
        job,
        expected_job_id,
        canonical_payload_bytes,
        canonical_payload_sha256,
    })
}

fn canonical_payload_bytes(dispatch_id: Uuid) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(24);
    bytes.extend_from_slice(BID_DELIVERY_V1_MAGIC);
    bytes.extend_from_slice(&BID_DELIVERY_V1_SCHEMA_VERSION.to_be_bytes());
    bytes.extend_from_slice(dispatch_id.as_bytes());
    bytes.extend_from_slice(&BID_DELIVERY_V1_PAYLOAD_VERSION.to_be_bytes());
    bytes
}

fn lowercase_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    encoded
}

#[cfg(test)]
mod registry_tests {
    use super::*;

    #[test]
    fn tampered_registry_closure_fails_closed() {
        let mut observation = actual_registry_observation().expect("registry observation");
        observation.lanes[0] = "wrong-lane".to_string();
        assert_eq!(
            verify_registry_observation(&observation),
            Err(RegistryClosureError::Queue)
        );

        let mut observation = actual_registry_observation().expect("registry observation");
        observation.task_type = "bid:delivery:v2";
        assert_eq!(
            verify_registry_observation(&observation),
            Err(RegistryClosureError::TaskType)
        );

        let mut observation = actual_registry_observation().expect("registry observation");
        observation.payload_version = 2;
        assert_eq!(
            verify_registry_observation(&observation),
            Err(RegistryClosureError::PayloadVersion)
        );

        let mut observation = actual_registry_observation().expect("registry observation");
        observation.payload_fields.push("unexpected".to_string());
        assert_eq!(
            verify_registry_observation(&observation),
            Err(RegistryClosureError::PayloadShape)
        );

        let mut observation = actual_registry_observation().expect("registry observation");
        observation.on_conflict = JobConflictStrategy::Replace;
        assert_eq!(
            verify_registry_observation(&observation),
            Err(RegistryClosureError::ConflictStrategy)
        );

        let mut observation = actual_registry_observation().expect("registry observation");
        observation.resurrect = false;
        assert_eq!(
            verify_registry_observation(&observation),
            Err(RegistryClosureError::Resurrection)
        );

        let mut observation = actual_registry_observation().expect("registry observation");
        observation.worker_max_retries = 1;
        assert_eq!(
            verify_registry_observation(&observation),
            Err(RegistryClosureError::WorkerRetry)
        );

        let storage = oxana::Storage::builder()
            .namespace("registry-negative")
            .build_from_redis_url("redis://127.0.0.1:1/")
            .expect("configure non-I/O registry fixture");
        let failure = match OxanaStableAdapter::with_registry_result(
            storage,
            Err(RegistryClosureError::WorkerRetry),
        ) {
            Ok(_) => panic!("registry mismatch must not construct an adapter"),
            Err(failure) => failure,
        };
        assert_eq!(failure.error, RegistryClosureError::WorkerRetry);
        assert_eq!(failure.readiness, WorkTransportReadiness::FailedClosed);
        assert_eq!(failure.metrics.registry_closure_mismatch, 1);
        assert_eq!(failure.metrics.enqueue_attempts, 0);
    }
}
