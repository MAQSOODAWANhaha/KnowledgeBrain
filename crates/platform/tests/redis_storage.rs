//! Queue transport against an explicitly selected disposable Redis database.
use platform::{DefaultQueue, DocumentProcessJob};
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires owned Redis and KNOWLEDGEBRAIN_REQUIRE_REDIS_TESTS=1"]
async fn configured_redis_shares_jobs_across_producer_and_consumer() {
    assert_eq!(
        std::env::var("KNOWLEDGEBRAIN_REQUIRE_REDIS_TESTS").as_deref(),
        Ok("1")
    );
    let url = std::env::var("REDIS_URL").expect("owned Redis URL");
    let producer = platform::oxana_storage(&url).unwrap();
    let consumer = platform::oxana_storage(&url).unwrap();
    let job = DocumentProcessJob {
        document_id: Uuid::new_v4(),
        product_version_id: Uuid::new_v4(),
        attempt: 1,
        task_type: platform::TYPE_DOCUMENT_PROCESS.into(),
        passages: Vec::new(),
    };
    let id = producer.enqueue(DefaultQueue, job.clone()).await.unwrap();
    assert!(consumer.get_job(&id).await.unwrap().is_some());
    assert_eq!(producer.enqueue(DefaultQueue, job).await.unwrap(), id);
    consumer.delete_job(&id).await.unwrap();
    assert!(producer.get_job(&id).await.unwrap().is_none());
}
