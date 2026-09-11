//! Required, explicitly selected test against an owned disposable Redis.
use platform::{DefaultQueue, DeploymentNamespaceV1, DocumentProcessJob};
use std::collections::BTreeSet;
use uuid::Uuid;

async fn key_names(connection: &mut redis::aio::MultiplexedConnection) -> BTreeSet<String> {
    let mut cursor = 0u64;
    let mut keys = BTreeSet::new();
    loop {
        let (next, batch): (u64, Vec<String>) = redis::cmd("SCAN")
            .arg(cursor)
            .query_async(connection)
            .await
            .unwrap();
        keys.extend(batch);
        if next == 0 {
            return keys;
        }
        cursor = next;
    }
}

#[tokio::test]
#[ignore = "requires owned Redis and KNOWLEDGEBRAIN_REQUIRE_REDIS_TESTS=1"]
async fn official_storage_isolates_jobs_and_uses_only_derived_namespace_keys() {
    assert_eq!(
        std::env::var("KNOWLEDGEBRAIN_REQUIRE_REDIS_TESTS").as_deref(),
        Ok("1")
    );
    let url = std::env::var("REDIS_URL").expect("owned Redis URL");
    let first: DeploymentNamespaceV1 = Uuid::new_v4().to_string().parse().unwrap();
    let second: DeploymentNamespaceV1 = Uuid::new_v4().to_string().parse().unwrap();
    let a = platform::oxana_storage_in_namespace(&url, first).unwrap();
    let b = platform::oxana_storage_in_namespace(&url, second).unwrap();
    // Only this fixture deliberately uses the old raw UUID namespace.
    let legacy = oxana::Storage::builder()
        .namespace(first.to_string())
        .build_from_redis_url(&url)
        .unwrap();
    let mut connection = redis::Client::open(url)
        .unwrap()
        .get_multiplexed_async_connection()
        .await
        .unwrap();
    let before = key_names(&mut connection).await;
    let job = DocumentProcessJob {
        document_id: Uuid::new_v4(),
        product_version_id: Uuid::new_v4(),
        attempt: 1,
        task_type: platform::TYPE_DOCUMENT_PROCESS.into(),
        passages: Vec::new(),
    };
    let old_id = legacy.enqueue(DefaultQueue, job.clone()).await.unwrap();
    assert!(a.get_job(&old_id).await.unwrap().is_none());
    let a_id = a.enqueue(DefaultQueue, job.clone()).await.unwrap();
    assert!(b.get_job(&a_id).await.unwrap().is_none());
    let b_id = b.enqueue(DefaultQueue, job.clone()).await.unwrap();
    assert_eq!(
        a_id, b_id,
        "identical business unique identity in each deployment"
    );
    assert_eq!(a.enqueue(DefaultQueue, job).await.unwrap(), a_id);
    assert_eq!(a.jobs_count().await.unwrap(), 1);
    assert_eq!(b.jobs_count().await.unwrap(), 1);
    let after = key_names(&mut connection).await;
    let added: Vec<_> = after.difference(&before).collect();
    let expected_a = format!("kb:{}:", first.id().simple());
    let expected_b = format!("kb:{}:", second.id().simple());
    let expected_legacy = format!("{first}:");
    assert!(added.iter().any(|key| key.starts_with(&expected_a)));
    assert!(added.iter().any(|key| key.starts_with(&expected_b)));
    // Check only physical namespace ownership; do not interpret private queue keys.
    assert!(added.iter().all(|key| key.starts_with(&expected_a)
        || key.starts_with(&expected_b)
        || key.starts_with(&expected_legacy)));
    a.delete_job(&a_id).await.unwrap();
    assert!(a.get_job(&a_id).await.unwrap().is_none());
    assert!(b.get_job(&b_id).await.unwrap().is_some());
    assert!(legacy.get_job(&old_id).await.unwrap().is_some());
    b.delete_job(&b_id).await.unwrap();
    legacy.delete_job(&old_id).await.unwrap();
}
