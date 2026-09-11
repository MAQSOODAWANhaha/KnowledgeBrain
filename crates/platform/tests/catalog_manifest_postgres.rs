use platform::{
    CATALOG_MANIFEST_SCHEMA, CatalogDefinition, CatalogKind, ReleaseDescriptorV1, ReleaseImagesV1,
    SchemaComponentKind, SchemaRuntimeIdentity, apply_fresh_baseline_with_identity,
    build_catalog_manifest, catalog_manifest_sha256, jcs_canonical_bytes,
    validate_catalog_manifest, verify_runtime_schema, verify_runtime_schema_on_connection,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Acquire, PgPool};
use std::{path::PathBuf, str::FromStr};

const URL_ENV: &str = "KNOWLEDGEBRAIN_CATALOG_TEST_DATABASE_URL";
const REQUIRE_ENV: &str = "KNOWLEDGEBRAIN_REQUIRE_CATALOG_POSTGRES_TEST";

fn required() -> bool {
    std::env::var(REQUIRE_ENV).is_ok_and(|value| value == "1")
}

fn descriptor() -> ReleaseDescriptorV1 {
    ReleaseDescriptorV1 {
        schema_version: 1,
        release_revision: "catalog-integration".into(),
        git_sha: "0123456789abcdef0123456789abcdef01234567".into(),
        platform_schema_revision: "catalog-integration-v1".into(),
        images: ReleaseImagesV1 {
            migrator: format!("test.local/kb/migrator@sha256:{}", "1".repeat(64)),
            api: format!("test.local/kb/api@sha256:{}", "2".repeat(64)),
            worker: format!("test.local/kb/worker@sha256:{}", "3".repeat(64)),
            retention: format!("test.local/kb/retention@sha256:{}", "4".repeat(64)),
            docreader: format!("test.local/kb/docreader@sha256:{}", "5".repeat(64)),
        },
    }
}

fn identity(
    descriptor: ReleaseDescriptorV1,
    kind: SchemaComponentKind,
    namespace: &str,
) -> SchemaRuntimeIdentity {
    let hash = descriptor.sha256().unwrap();
    let digest = descriptor.component_digest(kind).unwrap().to_owned();
    SchemaRuntimeIdentity::from_descriptor(
        PathBuf::from("/isolated-test/release-descriptor.json"),
        descriptor,
        &hash,
        kind.as_str(),
        &digest,
        namespace,
    )
    .unwrap()
}

fn isolated_url() -> Option<String> {
    let url = match std::env::var(URL_ENV) {
        Ok(value) if !value.trim().is_empty() => value,
        _ if required() => panic!("{REQUIRE_ENV}=1 requires an explicit {URL_ENV}"),
        _ => {
            eprintln!("skipped catalog manifest PostgreSQL fixture: {URL_ENV} is unset");
            return None;
        }
    };
    let options = sqlx::postgres::PgConnectOptions::from_str(&url)
        .unwrap_or_else(|error| panic!("invalid {URL_ENV}: {error}"));
    assert_eq!(
        options.get_host(),
        "127.0.0.1",
        "refusing catalog test outside 127.0.0.1"
    );
    assert_eq!(
        options.get_port(),
        25433,
        "refusing catalog test outside port 25433"
    );
    assert!(
        options
            .get_database()
            .is_some_and(|database| database.starts_with("knowledgebrain_test_")),
        "refusing catalog test outside knowledgebrain_test_*"
    );
    Some(url)
}

#[tokio::test]
async fn catalog_manifest_postgres_16_fixture() {
    let Some(url) = isolated_url() else { return };
    let pool = match PgPool::connect(&url).await {
        Ok(pool) => pool,
        Err(error) if required() => {
            panic!("required catalog PostgreSQL fixture unavailable: {error}")
        }
        Err(error) => {
            eprintln!("skipped catalog manifest PostgreSQL fixture: {error}");
            return;
        }
    };
    let server_version: i32 =
        sqlx::query_scalar("SELECT current_setting('server_version_num')::integer")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        (160000..170000).contains(&server_version),
        "fixture requires PostgreSQL 16"
    );
    let existing: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_catalog.pg_class relation
         JOIN pg_catalog.pg_namespace namespace_value ON namespace_value.oid=relation.relnamespace
         WHERE namespace_value.nspname='public' AND relation.relkind IN ('r','p','v','m','S','f')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        existing, 0,
        "catalog fixture requires an explicitly fresh isolated database"
    );

    let namespace = "123e4567-e89b-12d3-a456-426614174000";
    let descriptor = descriptor();
    let migrator_identity = identity(descriptor.clone(), SchemaComponentKind::Migrator, namespace);
    let runtime_identity = identity(descriptor.clone(), SchemaComponentKind::Api, namespace);
    apply_fresh_baseline_with_identity(&pool, &migrator_identity)
        .await
        .unwrap();
    let receipt_before: (chrono::DateTime<chrono::Utc>, String, Value, i32) = sqlx::query_as(
        "SELECT created_at,catalog_manifest_sha256::text,extensions,postgres_server_version_num
         FROM platform_schema_snapshot WHERE singleton",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(receipt_before.3, server_version);
    assert!(
        receipt_before
            .2
            .as_array()
            .is_some_and(|values| !values.is_empty())
    );
    apply_fresh_baseline_with_identity(&pool, &migrator_identity)
        .await
        .unwrap();
    let receipt_after: (chrono::DateTime<chrono::Utc>, String, Value, i32) = sqlx::query_as(
        "SELECT created_at,catalog_manifest_sha256::text,extensions,postgres_server_version_num
         FROM platform_schema_snapshot WHERE singleton",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        receipt_before, receipt_after,
        "matching replay must write no receipt or DDL"
    );
    verify_runtime_schema(&pool, &runtime_identity)
        .await
        .unwrap();

    let bad_component = SchemaRuntimeIdentity::from_descriptor(
        PathBuf::from("/isolated-test/release-descriptor.json"),
        descriptor.clone(),
        &descriptor.sha256().unwrap(),
        "api",
        &format!("sha256:{}", "9".repeat(64)),
        namespace,
    );
    assert!(bad_component.is_err());
    let mut changed_descriptor = descriptor.clone();
    changed_descriptor.release_revision = "catalog-integration-2".into();
    let changed_identity = identity(changed_descriptor, SchemaComponentKind::Api, namespace);
    assert!(
        verify_runtime_schema(&pool, &changed_identity)
            .await
            .is_err()
    );
    let wrong_namespace = identity(
        descriptor.clone(),
        SchemaComponentKind::Api,
        "123e4567-e89b-12d3-a456-426614174001",
    );
    assert!(
        verify_runtime_schema(&pool, &wrong_namespace)
            .await
            .is_err()
    );

    let mut connection = pool.acquire().await.unwrap();
    sqlx::query("SET ROLE kb_app_owner")
        .execute(&mut *connection)
        .await
        .unwrap();
    sqlx::raw_sql(
        r#"
        CREATE TABLE catalog_dependency_source(id integer PRIMARY KEY,payload text NOT NULL);
        CREATE VIEW catalog_dependency_view AS
          SELECT source.id,source.payload FROM catalog_dependency_source source;
        ALTER TABLE application_maintenance_gate
          ADD COLUMN catalog_test_numeric numeric(50,20),
          ADD COLUMN catalog_test_json jsonb,
          ADD COLUMN catalog_test_uuid uuid,
          ADD COLUMN catalog_test_timestamp timestamp,
          ADD COLUMN catalog_test_timestamptz timestamptz,
          ADD COLUMN catalog_test_bytes bytea,
          ADD COLUMN catalog_test_null text,
          ADD COLUMN catalog_test_boolean boolean,
          ADD COLUMN catalog_test_matrix numeric[][];
        UPDATE application_maintenance_gate SET
          catalog_test_numeric=12345678901234567890.12345678901234567890,
          catalog_test_json='{"fraction":1.5,"large":1e30,"small":1e-7}'::jsonb,
          catalog_test_uuid='A0B1C2D3-E4F5-4678-9ABC-DEF012345678',
          catalog_test_timestamp='2026-01-02 03:04:05.123456',
          catalog_test_timestamptz='2026-01-02 11:04:05.654321+08',
          catalog_test_bytes=decode('A0ff01','hex'),
          catalog_test_null=NULL,
          catalog_test_boolean=true,
          catalog_test_matrix=ARRAY[[1.2300,NULL],[-0.00,2.5000]]::numeric[][];
        "#,
    )
    .execute(&mut *connection)
    .await
    .unwrap();

    let records = build_catalog_manifest(&mut connection).await.unwrap();
    validate_catalog_manifest(&records).unwrap();
    let schema: Value = serde_json::from_str(CATALOG_MANIFEST_SCHEMA).unwrap();
    let validator = jsonschema::JSONSchema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .compile(&schema)
        .unwrap();
    let manifest_value = serde_json::to_value(&records).unwrap();
    assert!(
        validator.is_valid(&manifest_value),
        "live manifest must satisfy CatalogManifestV1"
    );

    let bytes = jcs_canonical_bytes(&records).unwrap();
    let replay_records = build_catalog_manifest(&mut connection).await.unwrap();
    assert_eq!(bytes, jcs_canonical_bytes(&replay_records).unwrap());
    let mut replay = Sha256::new();
    replay.update(b"KB:PlatformCatalogManifest:v1\0");
    replay.update(&bytes);
    assert_eq!(
        catalog_manifest_sha256(&records).unwrap(),
        hex::encode(replay.finalize())
    );

    let view = records
        .iter()
        .find(|record| {
            record.kind == CatalogKind::View && record.identity == "public.catalog_dependency_view"
        })
        .unwrap();
    for expected in [
        (CatalogKind::Relation, "public.catalog_dependency_source"),
        (CatalogKind::Column, "public.catalog_dependency_source.id"),
        (
            CatalogKind::Column,
            "public.catalog_dependency_source.payload",
        ),
    ] {
        assert!(
            view.dependencies.iter().any(
                |dependency| dependency.kind == expected.0 && dependency.identity == expected.1
            ),
            "missing view dependency {:?} {} in {:?}",
            expected.0,
            expected.1,
            view.dependencies
        );
    }

    let seed = records
        .iter()
        .find(|record| {
            record.kind == CatalogKind::SeedRow && record.name == "application_maintenance_gate"
        })
        .unwrap();
    let CatalogDefinition::SeedRow(seed) = &seed.definition else {
        panic!("seed definition")
    };
    assert_eq!(
        seed.values["catalog_test_numeric"],
        json!("12345678901234567890.1234567890123456789")
    );
    assert_eq!(
        seed.values["catalog_test_uuid"],
        json!("a0b1c2d3-e4f5-4678-9abc-def012345678")
    );
    assert_eq!(
        seed.values["catalog_test_timestamp"],
        json!("2026-01-02T03:04:05.123456Z")
    );
    assert_eq!(
        seed.values["catalog_test_timestamptz"],
        json!("2026-01-02T03:04:05.654321Z")
    );
    assert_eq!(seed.values["catalog_test_bytes"], json!("a0ff01"));
    assert_eq!(seed.values["catalog_test_null"], Value::Null);
    assert_eq!(seed.values["catalog_test_boolean"], json!(true));
    assert_eq!(
        seed.values["catalog_test_matrix"],
        json!([["1.23", null], ["0", "2.5"]])
    );
    assert_eq!(
        String::from_utf8(jcs_canonical_bytes(&seed.values["catalog_test_json"]).unwrap()).unwrap(),
        "{\"fraction\":1.5,\"large\":1e+30,\"small\":1e-7}"
    );

    assert!(
        records
            .iter()
            .all(|record| record.owner.as_deref() != Some("wrong_owner"))
    );
    assert!(
        records
            .iter()
            .all(|record| !(record.kind == CatalogKind::Type && record.name == "vector"))
    );
    let snapshot_rows: i64 = sqlx::query_scalar("SELECT count(*) FROM platform_schema_snapshot")
        .fetch_one(&mut *connection)
        .await
        .unwrap();
    assert_eq!(
        snapshot_rows, 1,
        "fresh bootstrap writes exactly one receipt"
    );

    sqlx::raw_sql(
        r#"
        DROP VIEW catalog_dependency_view;
        DROP TABLE catalog_dependency_source;
        ALTER TABLE application_maintenance_gate
          DROP COLUMN catalog_test_numeric,
          DROP COLUMN catalog_test_json,
          DROP COLUMN catalog_test_uuid,
          DROP COLUMN catalog_test_timestamp,
          DROP COLUMN catalog_test_timestamptz,
          DROP COLUMN catalog_test_bytes,
          DROP COLUMN catalog_test_null,
          DROP COLUMN catalog_test_boolean,
          DROP COLUMN catalog_test_matrix;
        "#,
    )
    .execute(&mut *connection)
    .await
    .unwrap();

    let fixture_objects: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_catalog.pg_attribute
         WHERE attrelid='application_maintenance_gate'::regclass AND attname LIKE 'catalog_test_%'",
    )
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    assert_eq!(fixture_objects, 0, "fixture columns must be cleaned");
    verify_runtime_schema_on_connection(&mut connection, &runtime_identity)
        .await
        .unwrap();

    let mut drift = connection.begin().await.unwrap();
    sqlx::query("UPDATE platform_schema_snapshot SET shared_baseline_sha256=$1 WHERE singleton")
        .bind("0".repeat(64))
        .execute(&mut *drift)
        .await
        .unwrap();
    assert!(
        verify_runtime_schema_on_connection(&mut drift, &runtime_identity)
            .await
            .is_err()
    );
    drift.rollback().await.unwrap();

    let mut drift = connection.begin().await.unwrap();
    sqlx::query(
        "UPDATE platform_schema_snapshot SET postgres_server_version_num=postgres_server_version_num+1 WHERE singleton",
    )
    .execute(&mut *drift)
    .await
    .unwrap();
    assert!(
        verify_runtime_schema_on_connection(&mut drift, &runtime_identity)
            .await
            .is_err()
    );
    drift.rollback().await.unwrap();

    let mut drift = connection.begin().await.unwrap();
    sqlx::query("UPDATE platform_schema_snapshot SET extensions='[]'::jsonb WHERE singleton")
        .execute(&mut *drift)
        .await
        .unwrap();
    assert!(
        verify_runtime_schema_on_connection(&mut drift, &runtime_identity)
            .await
            .is_err()
    );
    drift.rollback().await.unwrap();

    let mut drift = connection.begin().await.unwrap();
    sqlx::query("UPDATE platform_schema_snapshot SET created_at=created_at+interval '1 day' WHERE singleton")
        .execute(&mut *drift).await.unwrap();
    verify_runtime_schema_on_connection(&mut drift, &runtime_identity)
        .await
        .unwrap();
    drift.rollback().await.unwrap();

    let mut drift = connection.begin().await.unwrap();
    sqlx::query(
        "UPDATE application_maintenance_gate SET generation=generation+1 WHERE singleton_key",
    )
    .execute(&mut *drift)
    .await
    .unwrap();
    assert!(
        verify_runtime_schema_on_connection(&mut drift, &runtime_identity)
            .await
            .is_err()
    );
    drift.rollback().await.unwrap();

    let mut drift = connection.begin().await.unwrap();
    sqlx::query("ALTER FUNCTION kb_actor_identity_valid(text) VOLATILE")
        .execute(&mut *drift)
        .await
        .unwrap();
    assert!(
        verify_runtime_schema_on_connection(&mut drift, &runtime_identity)
            .await
            .is_err()
    );
    drift.rollback().await.unwrap();

    let mut drift = connection.begin().await.unwrap();
    sqlx::query("GRANT SELECT ON object_registry TO kb_runtime_api")
        .execute(&mut *drift)
        .await
        .unwrap();
    assert!(
        verify_runtime_schema_on_connection(&mut drift, &runtime_identity)
            .await
            .is_err()
    );
    drift.rollback().await.unwrap();

    let admin_password = std::env::var("KNOWLEDGEBRAIN_CATALOG_TEST_ADMIN_PASSWORD")
        .unwrap_or_else(|_| {
            if required() {
                panic!("required catalog admin password is missing")
            } else {
                String::new()
            }
        });
    if !admin_password.is_empty() {
        let admin_options = sqlx::postgres::PgConnectOptions::from_str(&url)
            .unwrap()
            .username("knowledgebrain")
            .password(&admin_password);
        let admin_pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect_with(admin_options)
            .await
            .unwrap();
        let mut admin_connection = admin_pool.acquire().await.unwrap();
        let mut drift = admin_connection.begin().await.unwrap();
        sqlx::query("ALTER TABLE object_registry OWNER TO kb_migrator")
            .execute(&mut *drift)
            .await
            .unwrap();
        assert!(
            verify_runtime_schema_on_connection(&mut drift, &runtime_identity)
                .await
                .is_err()
        );
        drift.rollback().await.unwrap();

        let baseline_manifest = build_catalog_manifest(&mut admin_connection).await.unwrap();
        let baseline_sha = catalog_manifest_sha256(&baseline_manifest).unwrap();
        let suffix = uuid::Uuid::new_v4().simple().to_string();
        let external_member = format!("external_member_{suffix}");
        let mut membership_drift = admin_connection.begin().await.unwrap();
        // UUID-simple suffixes are restricted to lowercase hex; identifiers are injection-safe.
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "CREATE ROLE {external_member} NOLOGIN"
        )))
        .execute(&mut *membership_drift)
        .await
        .unwrap();
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "GRANT kb_app_owner TO {external_member}"
        )))
        .execute(&mut *membership_drift)
        .await
        .unwrap();
        let member_manifest = build_catalog_manifest(&mut membership_drift).await.unwrap();
        assert_ne!(
            catalog_manifest_sha256(&member_manifest).unwrap(),
            baseline_sha
        );
        let member_edge = member_manifest.iter().find(|record| {
            matches!(&record.definition, CatalogDefinition::Membership(edge)
                if edge.role == "kb_app_owner"
                    && edge.member == external_member
                    && edge.grantor == "knowledgebrain"
                    && !edge.admin_option)
        });
        assert!(
            member_edge.is_some(),
            "allowlisted role to external member edge missing"
        );
        assert!(
            verify_runtime_schema_on_connection(&mut membership_drift, &runtime_identity)
                .await
                .is_err()
        );
        membership_drift.rollback().await.unwrap();

        let external_role = format!("external_role_{suffix}");
        let mut membership_drift = admin_connection.begin().await.unwrap();
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "CREATE ROLE {external_role} NOLOGIN"
        )))
        .execute(&mut *membership_drift)
        .await
        .unwrap();
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "GRANT {external_role} TO kb_runtime_api"
        )))
        .execute(&mut *membership_drift)
        .await
        .unwrap();
        let role_manifest = build_catalog_manifest(&mut membership_drift).await.unwrap();
        assert_ne!(
            catalog_manifest_sha256(&role_manifest).unwrap(),
            baseline_sha
        );
        let role_edge = role_manifest.iter().find(|record| {
            matches!(&record.definition, CatalogDefinition::Membership(edge)
                if edge.role == external_role
                    && edge.member == "kb_runtime_api"
                    && edge.grantor == "knowledgebrain"
                    && !edge.admin_option)
        });
        assert!(
            role_edge.is_some(),
            "external role to allowlisted member edge missing"
        );
        let membership_identities: Vec<_> = role_manifest
            .iter()
            .filter(|record| record.kind == CatalogKind::Membership)
            .map(|record| record.identity.as_str())
            .collect();
        assert!(
            membership_identities
                .windows(2)
                .all(|pair| pair[0].as_bytes() <= pair[1].as_bytes())
        );
        assert!(
            verify_runtime_schema_on_connection(&mut membership_drift, &runtime_identity)
                .await
                .is_err()
        );
        membership_drift.rollback().await.unwrap();

        let external_role_count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_catalog.pg_roles WHERE rolname=$1 OR rolname=$2",
        )
        .bind(&external_member)
        .bind(&external_role)
        .fetch_one(&mut *admin_connection)
        .await
        .unwrap();
        assert_eq!(
            external_role_count, 0,
            "external fixture roles must rollback"
        );
        drop(admin_connection);
        admin_pool.close().await;
    }

    let mut drift = connection.begin().await.unwrap();
    sqlx::query("DELETE FROM platform_schema_snapshot WHERE singleton")
        .execute(&mut *drift)
        .await
        .unwrap();
    assert!(
        verify_runtime_schema_on_connection(&mut drift, &runtime_identity)
            .await
            .is_err()
    );
    drift.rollback().await.unwrap();

    let api_password = std::env::var("KNOWLEDGEBRAIN_API_DB_PASSWORD").unwrap_or_else(|_| {
        if required() {
            panic!("required runtime role password is missing")
        } else {
            String::new()
        }
    });
    if !api_password.is_empty() {
        let options = sqlx::postgres::PgConnectOptions::from_str(&url)
            .unwrap()
            .username("kb_runtime_api")
            .password(&api_password);
        let runtime_pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        verify_runtime_schema(&runtime_pool, &runtime_identity)
            .await
            .unwrap();
        sqlx::query("REVOKE SELECT ON platform_schema_snapshot FROM kb_runtime_api")
            .execute(&mut *connection)
            .await
            .unwrap();
        let permission_error = verify_runtime_schema(&runtime_pool, &runtime_identity)
            .await
            .unwrap_err();
        assert!(matches!(
            permission_error,
            platform::SchemaError::RevisionMismatch { .. }
        ));
        sqlx::query("GRANT SELECT ON platform_schema_snapshot TO kb_runtime_api")
            .execute(&mut *connection)
            .await
            .unwrap();
        verify_runtime_schema(&runtime_pool, &runtime_identity)
            .await
            .unwrap();
        assert!(
            sqlx::query("CREATE TABLE runtime_must_not_create(id integer)")
                .execute(&runtime_pool)
                .await
                .is_err()
        );
        let runtime_snapshot_rows: i64 =
            sqlx::query_scalar("SELECT count(*) FROM platform_schema_snapshot WHERE singleton")
                .fetch_one(&runtime_pool)
                .await
                .unwrap();
        assert_eq!(runtime_snapshot_rows, 1);
        runtime_pool.close().await;
        let closed_error = verify_runtime_schema(&runtime_pool, &runtime_identity)
            .await
            .unwrap_err();
        assert!(matches!(
            closed_error,
            platform::SchemaError::Sql(sqlx::Error::PoolClosed)
        ));
    }

    sqlx::query("DELETE FROM platform_schema_snapshot WHERE singleton")
        .execute(&mut *connection)
        .await
        .unwrap();
    drop(connection);
    let partial = apply_fresh_baseline_with_identity(&pool, &migrator_identity)
        .await
        .unwrap_err();
    assert_eq!(partial.code(), "SCHEMA_REVISION_MISMATCH");
}
