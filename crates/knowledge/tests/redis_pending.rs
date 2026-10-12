//! Exercise document counters against an explicitly selected disposable Redis.
use knowledge::enrichment::{decr_pending_count, pending_count, pending_key, set_pending_count};
use redis::Commands;
use uuid::Uuid;

#[test]
#[ignore = "requires owned Redis and KNOWLEDGEBRAIN_REQUIRE_REDIS_TESTS=1"]
fn counters_are_shared_across_calls_and_isolated_by_document() {
    assert_eq!(
        std::env::var("KNOWLEDGEBRAIN_REQUIRE_REDIS_TESTS").as_deref(),
        Ok("1")
    );
    let url = std::env::var("REDIS_URL").expect("owned Redis URL");
    let mut connection = redis::Client::open(url).unwrap().get_connection().unwrap();
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    set_pending_count(first, 2).unwrap();
    set_pending_count(second, 7).unwrap();
    assert!(!decr_pending_count(first).unwrap());
    assert_eq!(pending_count(first), Some(1));
    assert!(decr_pending_count(first).unwrap());
    assert_eq!(pending_count(first), None);
    assert_eq!(pending_count(second), Some(7));
    let removed: usize = connection.del(pending_key(second)).unwrap();
    assert_eq!(removed, 1);
}
