//! Actual fresh baseline and read-only reset PostgreSQL preflight, on an owned
//! disposable cluster. The test never drops a database or runs namespace reset.
use platform::{
    DeploymentNamespaceV1, NamespaceResetError, NamespaceResetRequest, ReleaseDescriptorV1,
    SchemaRuntimeIdentity, namespace_reset_confirmation, verify_namespace_reset_postgres,
};
use serde_json::Value;
use sqlx::{
    Connection, PgConnection,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::{path::PathBuf, str::FromStr};
use uuid::Uuid;

fn required(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is required"))
}

fn identity(namespace: DeploymentNamespaceV1) -> SchemaRuntimeIdentity {
    let descriptor: ReleaseDescriptorV1 = serde_json::from_str(include_str!(
        "../../../deploy/release-descriptor-v1.development.json"
    ))
    .unwrap();
    let hash = descriptor.sha256().unwrap();
    let digest = descriptor
        .component_digest(platform::SchemaComponentKind::Migrator)
        .unwrap()
        .to_owned();
    SchemaRuntimeIdentity::from_descriptor(
        PathBuf::from("/owned-fixture/descriptor.json"),
        descriptor,
        &hash,
        "migrator",
        &digest,
        &namespace.to_string(),
    )
    .unwrap()
}

fn request(namespace: DeploymentNamespaceV1, revision: &str) -> NamespaceResetRequest {
    NamespaceResetRequest::new(
        &namespace.storage_label(),
        revision,
        &namespace_reset_confirmation(namespace, revision).unwrap(),
    )
    .unwrap()
}

async fn snapshot(connection: &mut PgConnection) -> Value {
    sqlx::query_scalar("SELECT to_jsonb(snapshot) FROM public.platform_schema_snapshot snapshot")
        .fetch_one(connection)
        .await
        .unwrap()
}

async fn expect_schema_rejection(
    connection: &mut PgConnection,
    request: &NamespaceResetRequest,
    identity: &SchemaRuntimeIdentity,
) {
    let error = verify_namespace_reset_postgres(connection, request, identity)
        .await
        .unwrap_err();
    assert!(
        matches!(error, NamespaceResetError::Schema(ref error)
        if error.code() == platform::SCHEMA_REVISION_MISMATCH),
        "{error}"
    );
}

#[tokio::test]
#[ignore = "requires owned fresh PostgreSQL and KNOWLEDGEBRAIN_REQUIRE_RESET_POSTGRES_TEST=1"]
async fn reset_postgres_preflight_requires_actual_mapping_receipt_and_no_other_sessions() {
    assert_eq!(required("KNOWLEDGEBRAIN_REQUIRE_RESET_POSTGRES_TEST"), "1");
    let namespace: DeploymentNamespaceV1 = required("KB_DEPLOYMENT_NAMESPACE_ID").parse().unwrap();
    let admin =
        PgConnectOptions::from_str(&required("KNOWLEDGEBRAIN_RESET_TEST_ADMIN_URL")).unwrap();
    let migrator =
        PgConnectOptions::from_str(&required("KNOWLEDGEBRAIN_RESET_TEST_MIGRATOR_URL")).unwrap();
    for options in [&admin, &migrator] {
        assert_eq!(options.get_host(), "127.0.0.1");
        assert_eq!(options.get_port(), admin.get_port());
        assert_eq!(
            options.get_database(),
            Some(namespace.postgres_database().as_str())
        );
    }
    let mut connection = PgConnection::connect_with(&admin).await.unwrap();
    let existing: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace
         WHERE n.nspname='public' AND c.relkind IN ('r','p','v','m','S','f')",
    ).fetch_one(&mut connection).await.unwrap();
    assert_eq!(
        existing, 0,
        "fixture must start with an empty owned database"
    );
    let identity = identity(namespace);
    let request = request(namespace, identity.schema_revision());
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(migrator)
        .await
        .unwrap();
    platform::apply_fresh_baseline_with_identity(&pool, &identity)
        .await
        .unwrap();
    pool.close().await;

    let before = snapshot(&mut connection).await;
    let expected_identity: (String, i64, String) = sqlx::query_as(
        "SELECT current_database(), oid::bigint, (SELECT system_identifier::text FROM pg_catalog.pg_control_system())
         FROM pg_catalog.pg_database WHERE datname=current_database()",
    ).fetch_one(&mut connection).await.unwrap();
    // Let PostgreSQL enforce read-only execution, in addition to checking data.
    sqlx::query("SET default_transaction_read_only=on")
        .execute(&mut connection)
        .await
        .unwrap();
    let first = verify_namespace_reset_postgres(&mut connection, &request, &identity)
        .await
        .unwrap();
    let replay = verify_namespace_reset_postgres(&mut connection, &request, &identity)
        .await
        .unwrap();
    assert_eq!(first, replay);
    assert_eq!(
        (
            first.database.clone(),
            first.database_oid,
            first.cluster_system_identifier.clone()
        ),
        expected_identity
    );
    assert_eq!(first.receipt.deployment_namespace_id, namespace.id());
    assert_eq!(snapshot(&mut connection).await, before);
    sqlx::query("SET default_transaction_read_only=off")
        .execute(&mut connection)
        .await
        .unwrap();

    let mut wrong_database = PgConnection::connect_with(&admin.clone().database("postgres"))
        .await
        .unwrap();
    assert!(matches!(
        verify_namespace_reset_postgres(&mut wrong_database, &request, &identity).await,
        Err(NamespaceResetError::DatabaseMapping)
    ));
    wrong_database.close().await.unwrap();
    let foreign = Uuid::new_v4()
        .to_string()
        .parse::<DeploymentNamespaceV1>()
        .unwrap();
    let foreign_request = self::request(foreign, identity.schema_revision());
    assert!(matches!(
        verify_namespace_reset_postgres(&mut connection, &foreign_request, &identity).await,
        Err(NamespaceResetError::Identity)
    ));
    let foreign_identity = self::identity(foreign);
    assert!(matches!(
        verify_namespace_reset_postgres(&mut connection, &foreign_request, &foreign_identity).await,
        Err(NamespaceResetError::DatabaseMapping)
    ));
    let changed_revision = self::request(namespace, "different-revision");
    assert!(matches!(
        verify_namespace_reset_postgres(&mut connection, &changed_revision, &identity).await,
        Err(NamespaceResetError::Identity)
    ));

    let mut other_session = PgConnection::connect_with(&admin).await.unwrap();
    let other_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut other_session)
        .await
        .unwrap();
    assert!(matches!(
        verify_namespace_reset_postgres(&mut connection, &request, &identity).await,
        Err(NamespaceResetError::DatabaseSessions)
    ));
    other_session.close().await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let present: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE pid=$1)")
                    .bind(other_pid)
                    .fetch_one(&mut connection)
                    .await
                    .unwrap();
            if !present {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();

    sqlx::query("UPDATE public.platform_schema_snapshot SET deployment_namespace_id=$1")
        .bind(foreign.id())
        .execute(&mut connection)
        .await
        .unwrap();
    expect_schema_rejection(&mut connection, &request, &identity).await;
    sqlx::query("UPDATE public.platform_schema_snapshot SET deployment_namespace_id=$1")
        .bind(namespace.id())
        .execute(&mut connection)
        .await
        .unwrap();
    sqlx::query("UPDATE public.platform_schema_snapshot SET schema_revision='different-revision'")
        .execute(&mut connection)
        .await
        .unwrap();
    expect_schema_rejection(&mut connection, &request, &identity).await;
    sqlx::query("UPDATE public.platform_schema_snapshot SET schema_revision=$1")
        .bind(identity.schema_revision())
        .execute(&mut connection)
        .await
        .unwrap();
    sqlx::query("DELETE FROM public.platform_schema_snapshot")
        .execute(&mut connection)
        .await
        .unwrap();
    expect_schema_rejection(&mut connection, &request, &identity).await;
    sqlx::query("INSERT INTO public.platform_schema_snapshot SELECT * FROM jsonb_populate_record(NULL::public.platform_schema_snapshot,$1)")
        .bind(&before).execute(&mut connection).await.unwrap();
    let index = format!("kb_reset_fixture_{}", Uuid::new_v4().simple());
    // Identifier consists solely of this literal prefix and generated hex;
    // no caller input is interpolated into either DDL statement.
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "CREATE INDEX {index} ON public.platform_schema_snapshot(schema_revision)"
    )))
    .execute(&mut connection)
    .await
    .unwrap();
    expect_schema_rejection(&mut connection, &request, &identity).await;
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP INDEX public.{index}")))
        .execute(&mut connection)
        .await
        .unwrap();
    assert_eq!(
        verify_namespace_reset_postgres(&mut connection, &request, &identity)
            .await
            .unwrap(),
        first
    );
    assert_eq!(snapshot(&mut connection).await, before);
    connection.close().await.unwrap();
}
