use oxana::{Job, JobConflictStrategy};
use serde::{Deserialize, Serialize};
use std::error::Error;
use uuid::Uuid;

pub const BID_DELIVERY_V1_QUEUE: &str = "bid-delivery-v1";
pub const BID_DELIVERY_V1_TASK_TYPE: &str = "bid:delivery:v1";

#[derive(oxana::Queue)]
#[oxana(key = "bid-delivery-v1", concurrency = Dynamic(4))]
pub struct BidDeliveryV1Queue;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BidDeliveryTargetKind {
    DocumentConversion,
    ExtractionTarget,
    MatchingSchedule,
    MatchingJob,
    AttachmentPreparation,
    SubmissionRender,
}

impl BidDeliveryTargetKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DocumentConversion => "document_conversion",
            Self::ExtractionTarget => "extraction_target",
            Self::MatchingSchedule => "matching_schedule",
            Self::MatchingJob => "matching_job",
            Self::AttachmentPreparation => "attachment_preparation",
            Self::SubmissionRender => "submission_render",
        }
    }
}

impl std::str::FromStr for BidDeliveryTargetKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "document_conversion" => Ok(Self::DocumentConversion),
            "extraction_target" => Ok(Self::ExtractionTarget),
            "matching_schedule" => Ok(Self::MatchingSchedule),
            "matching_job" => Ok(Self::MatchingJob),
            "attachment_preparation" => Ok(Self::AttachmentPreparation),
            "submission_render" => Ok(Self::SubmissionRender),
            other => Err(format!("unknown bid delivery target kind {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BidDeliveryV1Job {
    pub target_kind: BidDeliveryTargetKind,
    pub target_id: Uuid,
    pub target_revision: i64,
}

impl BidDeliveryV1Job {
    pub fn new(target_kind: BidDeliveryTargetKind, target_id: Uuid, target_revision: i64) -> Self {
        Self {
            target_kind,
            target_id,
            target_revision,
        }
    }
}

impl Job for BidDeliveryV1Job {
    fn name() -> &'static str {
        BID_DELIVERY_V1_TASK_TYPE
    }

    fn unique_id(&self) -> Option<String> {
        Some(format!(
            "{}:{}:{}",
            self.target_kind.as_str(),
            self.target_id.hyphenated(),
            self.target_revision
        ))
    }

    fn on_conflict(&self) -> JobConflictStrategy {
        JobConflictStrategy::Skip
    }

    fn should_resurrect() -> bool {
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BidDeliveryEnqueueOutcome {
    Accepted { job_id: String },
    Indeterminate { error: String },
}

#[derive(Clone)]
pub struct BidDeliveryEnqueuer {
    storage: oxana::Storage,
}

impl BidDeliveryEnqueuer {
    pub fn new(storage: oxana::Storage) -> Self {
        Self { storage }
    }

    pub async fn enqueue(
        &self,
        target_kind: BidDeliveryTargetKind,
        target_id: Uuid,
        target_revision: i64,
    ) -> BidDeliveryEnqueueOutcome {
        match self
            .storage
            .enqueue(
                BidDeliveryV1Queue,
                BidDeliveryV1Job::new(target_kind, target_id, target_revision),
            )
            .await
        {
            Ok(job_id) => BidDeliveryEnqueueOutcome::Accepted { job_id },
            Err(error) => BidDeliveryEnqueueOutcome::Indeterminate {
                error: error.to_string(),
            },
        }
    }
}

#[async_trait::async_trait]
pub trait BidDeliveryV1Handler: Send + Sync {
    type Error: Error + Send + Sync + 'static;

    async fn handle(&self, delivery: BidDeliveryV1Job) -> Result<(), Self::Error>;
}

pub struct BidDeliveryV1Worker<Handler> {
    handler: Handler,
}

impl<Handler> BidDeliveryV1Worker<Handler> {
    pub fn new(handler: Handler) -> Self {
        Self { handler }
    }
}

#[async_trait::async_trait]
impl<Handler> oxana::Worker<BidDeliveryV1Job> for BidDeliveryV1Worker<Handler>
where
    Handler: BidDeliveryV1Handler,
{
    type Error = Handler::Error;

    async fn process(
        &self,
        job: BidDeliveryV1Job,
        _context: &oxana::JobContext,
    ) -> Result<(), Self::Error> {
        self.handler.handle(job).await
    }

    fn max_retries(&self, _job: &BidDeliveryV1Job) -> u32 {
        3
    }

    fn retry_delay(&self, _job: &BidDeliveryV1Job, _retries: u32) -> u64 {
        10
    }
}

impl<Context, Handler> oxana::FromContext<Context> for BidDeliveryV1Worker<Handler>
where
    Handler: oxana::FromContext<Context>,
{
    fn from_context(context: &Context) -> Self {
        Self::new(Handler::from_context(context))
    }
}
