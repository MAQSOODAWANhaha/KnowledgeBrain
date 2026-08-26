use oxana::{Job, JobConflictStrategy};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    error::Error,
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Instant,
};
use uuid::Uuid;

use crate::{BidConvertV1Queue, BidExtractV1Queue, BidMatchingV1Queue, BidRenderV1Queue};

pub const BID_DELIVERY_V1_TASK_TYPE: &str = "bid:delivery:v1";
pub const BID_DELIVERY_V1_PAYLOAD_VERSION: u16 = 1;

const BID_DELIVERY_V1_SCHEMA_VERSION: u16 = 1;
const BID_DELIVERY_V1_MAGIC: &[u8; 4] = b"KBDL";

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
    Returned { job_id: String },
    Indeterminate { error_class: TransportErrorClass },
    ReturnedJobIdMismatch { actual_job_id: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkTransportReadiness {
    Ready,
    FailedClosed,
}

pub type WorkTransportFuture<'a> = Pin<Box<dyn Future<Output = TransportOutcome> + Send + 'a>>;

pub trait WorkTransport: Send + Sync {
    fn offer<'a>(
        &'a self,
        prepared: &'a PreparedDelivery,
        deadline: Instant,
    ) -> WorkTransportFuture<'a>;

    fn readiness(&self) -> WorkTransportReadiness;
}

#[derive(Clone)]
pub struct OxanaStableAdapter {
    storage: oxana::Storage,
    ready: Arc<AtomicBool>,
}

impl OxanaStableAdapter {
    pub fn new(storage: oxana::Storage) -> Self {
        Self {
            storage,
            ready: Arc::new(AtomicBool::new(true)),
        }
    }
}

impl WorkTransport for OxanaStableAdapter {
    fn offer<'a>(
        &'a self,
        prepared: &'a PreparedDelivery,
        deadline: Instant,
    ) -> WorkTransportFuture<'a> {
        Box::pin(offer_once(
            prepared,
            deadline,
            &self.ready,
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
        readiness(&self.ready)
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
    response: RecordingResponse,
    enqueue_count: AtomicUsize,
    ready: AtomicBool,
}

impl RecordingTransport {
    pub fn new(response: RecordingResponse) -> Self {
        Self {
            response,
            enqueue_count: AtomicUsize::new(0),
            ready: AtomicBool::new(true),
        }
    }

    pub fn enqueue_count(&self) -> usize {
        self.enqueue_count.load(Ordering::SeqCst)
    }
}

impl WorkTransport for RecordingTransport {
    fn offer<'a>(
        &'a self,
        prepared: &'a PreparedDelivery,
        deadline: Instant,
    ) -> WorkTransportFuture<'a> {
        Box::pin(offer_once(
            prepared,
            deadline,
            &self.ready,
            move || async move {
                self.enqueue_count.fetch_add(1, Ordering::SeqCst);
                match &self.response {
                    RecordingResponse::ReturnExpected => Ok(prepared.expected_job_id.clone()),
                    RecordingResponse::ReturnJobId(job_id) => Ok(job_id.clone()),
                    RecordingResponse::Error(error_class) => Err(*error_class),
                    RecordingResponse::Pending => std::future::pending().await,
                }
            },
        ))
    }

    fn readiness(&self) -> WorkTransportReadiness {
        readiness(&self.ready)
    }
}

async fn offer_once<Enqueue, EnqueueFuture>(
    prepared: &PreparedDelivery,
    deadline: Instant,
    ready: &AtomicBool,
    enqueue_once: Enqueue,
) -> TransportOutcome
where
    Enqueue: FnOnce() -> EnqueueFuture,
    EnqueueFuture: Future<Output = Result<String, TransportErrorClass>>,
{
    if !ready.load(Ordering::SeqCst) {
        return TransportOutcome::Indeterminate {
            error_class: TransportErrorClass::AdapterMismatch,
        };
    }
    if Instant::now() >= deadline {
        return TransportOutcome::Indeterminate {
            error_class: TransportErrorClass::DeadlineExceeded,
        };
    }

    match tokio::time::timeout_at(deadline.into(), enqueue_once()).await {
        Err(_) => TransportOutcome::Indeterminate {
            error_class: TransportErrorClass::DeadlineExceeded,
        },
        Ok(Err(error_class)) => TransportOutcome::Indeterminate { error_class },
        Ok(Ok(job_id)) if job_id == prepared.expected_job_id => {
            TransportOutcome::Returned { job_id }
        }
        Ok(Ok(actual_job_id)) => {
            ready.store(false, Ordering::SeqCst);
            TransportOutcome::ReturnedJobIdMismatch { actual_job_id }
        }
    }
}

fn readiness(ready: &AtomicBool) -> WorkTransportReadiness {
    if ready.load(Ordering::SeqCst) {
        WorkTransportReadiness::Ready
    } else {
        WorkTransportReadiness::FailedClosed
    }
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

pub fn prepare_bid_delivery_v1(
    spec: DeliverySpec,
) -> Result<PreparedDelivery, PrepareDeliveryError> {
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
