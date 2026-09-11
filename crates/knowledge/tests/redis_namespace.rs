//! Exercise the public counter API in separate configured processes.
use knowledge::enrichment::{decr_pending_count, pending_count, pending_key, set_pending_count};
use platform::DeploymentNamespaceV1;
use redis::Commands;
use std::process::Command;
use uuid::Uuid;

#[test]
#[ignore = "requires owned Redis and KNOWLEDGEBRAIN_REQUIRE_REDIS_TESTS=1"]
fn multimodal_counters_isolate_the_same_document_and_preserve_legacy_keys() {
    assert_eq!(
        std::env::var("KNOWLEDGEBRAIN_REQUIRE_REDIS_TESTS").as_deref(),
        Ok("1")
    );
    const CASE: &str = "KB_PENDING_NAMESPACE_CASE";
    const DOCUMENT: &str = "KB_PENDING_NAMESPACE_DOCUMENT";
    if let Ok(case) = std::env::var(CASE) {
        let id: Uuid = std::env::var(DOCUMENT).unwrap().parse().unwrap();
        match case.as_str() {
            "first" => {
                assert_eq!(pending_count(id), None);
                set_pending_count(id, 2);
                assert_eq!(pending_count(id), Some(2));
            }
            "second" => {
                assert_eq!(pending_count(id), None);
                set_pending_count(id, 7);
                assert_eq!(pending_count(id), Some(7));
            }
            "finish_first" => {
                assert!(!decr_pending_count(id));
                assert_eq!(pending_count(id), Some(1));
                assert!(decr_pending_count(id));
                assert_eq!(pending_count(id), None);
            }
            "no_namespace" => {
                set_pending_count(id, 5);
                assert_eq!(pending_count(id), None);
            }
            _ => panic!("unknown fixture operation"),
        }
        return;
    }
    let url = std::env::var("REDIS_URL").expect("owned Redis URL");
    let mut connection = redis::Client::open(url).unwrap().get_connection().unwrap();
    let first: DeploymentNamespaceV1 = Uuid::new_v4().to_string().parse().unwrap();
    let second: DeploymentNamespaceV1 = Uuid::new_v4().to_string().parse().unwrap();
    let id = Uuid::new_v4();
    let legacy = format!("multimodal:pending:{id}");
    let _: () = connection.set(&legacy, 11).unwrap();
    for (case, namespace) in [
        ("first", Some(first)),
        ("second", Some(second)),
        ("finish_first", Some(first)),
        ("no_namespace", None),
    ] {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "multimodal_counters_isolate_the_same_document_and_preserve_legacy_keys",
                "--include-ignored",
                "--nocapture",
            ])
            .env(CASE, case)
            .env(DOCUMENT, id.to_string());
        if let Some(namespace) = namespace {
            command.env("KB_DEPLOYMENT_NAMESPACE_ID", namespace.to_string());
        } else {
            command.env_remove("KB_DEPLOYMENT_NAMESPACE_ID");
        }
        let output = command.output().unwrap();
        assert!(output.status.success(), "{case}: {output:?}");
    }
    let first_value: Option<i32> = connection.get(pending_key(first, id)).unwrap();
    let second_value: i32 = connection.get(pending_key(second, id)).unwrap();
    let legacy_value: i32 = connection.get(&legacy).unwrap();
    assert_eq!((first_value, second_value, legacy_value), (None, 7, 11));
    let removed: usize = connection.del(&[pending_key(second, id), legacy]).unwrap();
    assert_eq!(removed, 2);
}
