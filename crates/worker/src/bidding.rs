//! Oxana adapters for the bidding v2 publish chain.
//!
//! Business logic lives in `bidding::worker` (and `bidding::journal_db`);
//! these adapters only translate between Oxana (`FromContext`, `Worker`) and
//! the bidding crate.

use crate::runtime::{AppCtx, JobErr};
use async_trait::async_trait;

pub struct TenderDocumentProcessV2Adapter {
    inner: bidding::worker::TenderDocumentProcessV2Worker,
}

impl oxana::FromContext<AppCtx> for TenderDocumentProcessV2Adapter {
    fn from_context(ctx: &AppCtx) -> Self {
        Self {
            inner: bidding::worker::TenderDocumentProcessV2Worker::new(
                ctx.pool.clone(),
                ctx.shutdown.clone(),
            ),
        }
    }
}

#[async_trait]
impl oxana::Worker<platform::TenderDocumentProcessJobV2> for TenderDocumentProcessV2Adapter {
    type Error = JobErr;

    fn max_retries(&self, _job: &platform::TenderDocumentProcessJobV2) -> u32 {
        platform::BID_AUTHORING_V2_MAX_RETRIES
    }

    async fn process(
        &self,
        job: platform::TenderDocumentProcessJobV2,
        _ctx: &oxana::JobContext,
    ) -> Result<(), Self::Error> {
        self.inner.run(&job).await.map_err(JobErr)
    }
}

pub struct ContentGenerateV2Adapter {
    inner: bidding::worker::ContentGenerateV2Worker,
}

impl oxana::FromContext<AppCtx> for ContentGenerateV2Adapter {
    fn from_context(ctx: &AppCtx) -> Self {
        Self {
            inner: bidding::worker::ContentGenerateV2Worker::new(ctx.pool.clone()),
        }
    }
}

#[async_trait]
impl oxana::Worker<platform::ContentGenerateJobV2> for ContentGenerateV2Adapter {
    type Error = JobErr;

    fn max_retries(&self, _job: &platform::ContentGenerateJobV2) -> u32 {
        platform::BID_AUTHORING_V2_MAX_RETRIES
    }

    async fn process(
        &self,
        job: platform::ContentGenerateJobV2,
        _ctx: &oxana::JobContext,
    ) -> Result<(), Self::Error> {
        self.inner.run(&job).await.map_err(JobErr)
    }
}
