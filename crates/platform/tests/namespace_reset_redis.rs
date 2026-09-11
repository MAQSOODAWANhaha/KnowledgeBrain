//! Explicitly selected read-only reset preflight on a disposable Redis.
use platform::{
    DeploymentNamespaceV1, NamespaceResetError, NamespaceResetRequest,
    namespace_reset_confirmation, verify_namespace_reset_redis,
};
use std::time::Duration;
use uuid::Uuid;

fn request() -> NamespaceResetRequest {
    let namespace: DeploymentNamespaceV1 = Uuid::new_v4().to_string().parse().unwrap();
    NamespaceResetRequest::new(
        &namespace.storage_label(),
        "redis-preflight-test",
        &namespace_reset_confirmation(namespace, "redis-preflight-test").unwrap(),
    )
    .unwrap()
}

#[tokio::test]
#[ignore = "requires owned Redis and KNOWLEDGEBRAIN_REQUIRE_RESET_REDIS_TEST=1"]
async fn reset_redis_observes_actual_database_and_server_without_key_writes() {
    assert_eq!(
        std::env::var("KNOWLEDGEBRAIN_REQUIRE_RESET_REDIS_TEST").as_deref(),
        Ok("1")
    );
    let admin =
        redis::Client::open(std::env::var("KNOWLEDGEBRAIN_RESET_TEST_REDIS_URL").unwrap()).unwrap();
    assert!(matches!(admin.get_connection_info().addr(),
        redis::ConnectionAddr::Tcp(host, _) if host == "127.0.0.1"));
    let db = admin.get_connection_info().redis_settings().db();
    assert!(
        db > 0 && db < 15,
        "fixture must select an explicit non-default database"
    );
    let mut connection = admin.get_multiplexed_async_connection().await.unwrap();
    let target = request();
    let foreign = request();
    let target_key = format!("{}sentinel", target.namespace().redis_prefix());
    let foreign_key = format!("{}sentinel", foreign.namespace().redis_prefix());
    let legacy_key = format!("{}:sentinel", target.namespace());
    let keys = [&target_key, &foreign_key, &legacy_key];
    for key in keys {
        let _: () = redis::cmd("SET")
            .arg(key)
            .arg(key)
            .query_async(&mut connection)
            .await
            .unwrap();
    }
    let user = format!("reset_preflight_{}", Uuid::new_v4().simple());
    let password = Uuid::new_v4().to_string();
    let _: () = redis::cmd("ACL")
        .arg("SETUSER")
        .arg(&user)
        .arg("on")
        .arg(format!(">{password}"))
        .arg("+info")
        .arg("+client|info")
        .arg("+client|setinfo")
        .arg("+ping")
        .arg("+select")
        .query_async(&mut connection)
        .await
        .unwrap();
    let readonly = redis::Client::open(
        admin.get_connection_info().clone().set_redis_settings(
            admin
                .get_connection_info()
                .redis_settings()
                .clone()
                .set_username(&user)
                .set_password(&password),
        ),
    )
    .unwrap();
    let first = verify_namespace_reset_redis(&readonly, &target, Duration::from_secs(5))
        .await
        .unwrap();
    let replay = verify_namespace_reset_redis(&readonly, &target, Duration::from_secs(5))
        .await
        .unwrap();
    assert_eq!(first, replay);
    assert_eq!(first.database, db);
    assert_eq!(first.namespace, target.namespace());
    assert_eq!(first.prefix, target.namespace().redis_prefix());
    let info: redis::InfoDict = redis::cmd("INFO")
        .arg("server")
        .query_async(&mut connection)
        .await
        .unwrap();
    assert_eq!(
        Some(first.server_run_id.clone()),
        info.get::<String>("run_id")
    );
    let serialized = serde_json::to_string(&first).unwrap();
    assert!(!serialized.contains(&password) && !serialized.contains(&user));

    // A different database and namespace remain distinct even on this server.
    let other = redis::Client::open(
        readonly.get_connection_info().clone().set_redis_settings(
            readonly
                .get_connection_info()
                .redis_settings()
                .clone()
                .set_db(db + 1),
        ),
    )
    .unwrap();
    let other = verify_namespace_reset_redis(&other, &foreign, Duration::from_secs(5))
        .await
        .unwrap();
    assert_ne!(first.database, other.database);
    assert_ne!(first.prefix, other.prefix);
    assert_eq!(first.server_run_id, other.server_run_id);
    let mut read_connection = readonly.get_multiplexed_async_connection().await.unwrap();
    let denied_write: redis::RedisResult<()> = redis::cmd("SET")
        .arg(&target_key)
        .arg("not permitted")
        .query_async(&mut read_connection)
        .await;
    assert!(
        denied_write.is_err(),
        "preflight user must not be able to write keys"
    );

    // Missing introspection permissions must not fall back to configured values.
    let _: () = redis::cmd("ACL")
        .arg("SETUSER")
        .arg(&user)
        .arg("-client|info")
        .query_async(&mut connection)
        .await
        .unwrap();
    assert!(matches!(
        verify_namespace_reset_redis(&readonly, &target, Duration::from_secs(5)).await,
        Err(NamespaceResetError::Redis(_))
    ));
    for key in keys {
        let actual: String = redis::cmd("GET")
            .arg(key)
            .query_async(&mut connection)
            .await
            .unwrap();
        assert_eq!(&actual, key);
    }
    let _: i64 = redis::cmd("ACL")
        .arg("DELUSER")
        .arg(&user)
        .query_async(&mut connection)
        .await
        .unwrap();
    let _: i64 = redis::cmd("DEL")
        .arg(&keys)
        .query_async(&mut connection)
        .await
        .unwrap();
}
