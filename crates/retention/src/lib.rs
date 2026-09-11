use async_trait::async_trait;
use sqlx::PgPool;

#[derive(Clone)]
pub struct RetentionCtx {
    pool: PgPool,
}

impl RetentionCtx {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

pub struct ObjectRetentionWorker {
    pool: PgPool,
}

pub struct ObjectUploadExpireWorker {
    pool: PgPool,
}

impl oxana::FromContext<RetentionCtx> for ObjectRetentionWorker {
    fn from_context(ctx: &RetentionCtx) -> Self {
        Self {
            pool: ctx.pool.clone(),
        }
    }
}

impl oxana::FromContext<RetentionCtx> for ObjectUploadExpireWorker {
    fn from_context(ctx: &RetentionCtx) -> Self {
        Self {
            pool: ctx.pool.clone(),
        }
    }
}

#[derive(Debug)]
pub struct RetentionError(String);

impl std::fmt::Display for RetentionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for RetentionError {}

#[async_trait]
impl oxana::Worker<platform::ObjectRetentionJob> for ObjectRetentionWorker {
    type Error = RetentionError;

    fn max_retries(&self, _job: &platform::ObjectRetentionJob) -> u32 {
        10
    }

    async fn process(
        &self,
        job: platform::ObjectRetentionJob,
        _ctx: &oxana::JobContext,
    ) -> Result<(), Self::Error> {
        platform::process_object_deletion(
            &self.pool,
            &platform::ObjectDeletionIdentity {
                deletion_id: job.deletion_id,
                object_ref: job.object_ref,
                digest: job.digest,
                byte_length: job.byte_length,
            },
        )
        .await
        .map_err(RetentionError)
    }
}

#[async_trait]
impl oxana::Worker<platform::ObjectUploadExpireJob> for ObjectUploadExpireWorker {
    type Error = RetentionError;

    fn max_retries(&self, _job: &platform::ObjectUploadExpireJob) -> u32 {
        10
    }

    async fn process(
        &self,
        job: platform::ObjectUploadExpireJob,
        _ctx: &oxana::JobContext,
    ) -> Result<(), Self::Error> {
        if let Some(deletion) = platform::expire_one_object_upload(&self.pool, job.staging_id)
            .await
            .map_err(RetentionError)?
        {
            platform::process_object_deletion(&self.pool, &deletion)
                .await
                .map_err(RetentionError)?;
        }
        Ok(())
    }
}
