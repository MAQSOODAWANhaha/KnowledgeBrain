//! Opt-in disposable-database acceptance. Never connects without an explicit,
//! loopback-only test URL and never drops application objects.
use sqlx::PgPool;
use std::str::FromStr;

#[tokio::test]
async fn fresh_database_uses_one_unprivileged_application_login() {
    let required =
        std::env::var("KNOWLEDGEBRAIN_REQUIRE_FRESH_DATABASE_TEST").is_ok_and(|value| value == "1");
    let url = match std::env::var("KNOWLEDGEBRAIN_FRESH_TEST_DATABASE_URL") {
        Ok(value) if !value.trim().is_empty() => value,
        _ if required => panic!("explicit fresh test database URL is required"),
        _ => return,
    };
    let options = sqlx::postgres::PgConnectOptions::from_str(&url).unwrap();
    assert_eq!(options.get_host(), "127.0.0.1");
    assert_eq!(options.get_port(), 25433);
    assert!(
        options
            .get_database()
            .is_some_and(|name| name.starts_with("knowledgebrain_test_"))
    );
    let pool = PgPool::connect_with(options).await.unwrap();
    let existing: bool = sqlx::query_scalar("SELECT to_regclass('public.users') IS NOT NULL")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        !existing,
        "fresh fixture requires an empty application database"
    );
    platform::verify_application_role(&pool).await.unwrap();
    platform::apply_fresh_baseline(&pool).await.unwrap();
    let role: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&pool)
        .await
        .unwrap();
    let owner: String = sqlx::query_scalar(
        "SELECT pg_get_userbyid(relowner)::text FROM pg_class WHERE oid='public.users'::regclass",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(owner, role);
    for denied in [
        "CREATE TEMP TABLE forbidden_temporary_shadow(id integer)",
        "CREATE ROLE forbidden_cluster_role NOLOGIN",
        "SET ROLE postgres",
    ] {
        let error = sqlx::query(denied).execute(&pool).await.unwrap_err();
        assert_eq!(
            error.as_database_error().and_then(|e| e.code()).as_deref(),
            Some("42501")
        );
    }
    let before: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_class WHERE relnamespace='public'::regnamespace",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    platform::apply_fresh_baseline(&pool).await.unwrap();
    let after: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_class WHERE relnamespace='public'::regnamespace",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        before, after,
        "repeated initialization must not recreate objects"
    );
    pool.close().await;
}
