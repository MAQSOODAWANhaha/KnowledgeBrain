use crate::{
    CATALOG_MANIFEST_CONTRACT_VERSION, CatalogError, ReleaseIdentityError, SchemaRuntimeIdentity,
    bidding_baseline_sha256, build_catalog_manifest, catalog_manifest_sha256,
    knowledge_baseline_sha256, shared_baseline_sha256,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Connection, PgConnection, PgPool, Row};
use thiserror::Error;
use uuid::Uuid;

pub const KNOWLEDGE_BASE_BASELINE: &str =
    include_str!("../../../migrations/knowledge_base_baseline.sql");
pub const SHARED_PLATFORM_BASELINE: &str =
    include_str!("../../../migrations/shared_platform_baseline.sql");
pub const BIDDING_BASELINE: &str = include_str!("../../../migrations/bidding_v2_baseline.sql");

const BOOTSTRAP_LOCK_ID: i64 = 0x4b_42_53_43_48_45_4d_41;
pub const SCHEMA_REVISION_MISMATCH: &str = "SCHEMA_REVISION_MISMATCH";

#[derive(Debug, Error)]
pub enum SchemaError {
    #[error("{0}")]
    ReleaseIdentity(#[from] ReleaseIdentityError),
    #[error("postgres schema verification failed")]
    Sql(#[from] sqlx::Error),
    #[error("SCHEMA_REVISION_MISMATCH: reset required ({reason})")]
    RevisionMismatch { reason: &'static str },
}

impl SchemaError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::RevisionMismatch { .. } => SCHEMA_REVISION_MISMATCH,
            Self::ReleaseIdentity(_) => "RELEASE_IDENTITY_INVALID",
            Self::Sql(_) => "POSTGRES_UNAVAILABLE",
        }
    }

    fn mismatch(reason: &'static str) -> Self {
        Self::RevisionMismatch { reason }
    }
}

fn sql_error_is_unavailable(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::Io(_)
        | sqlx::Error::Tls(_)
        | sqlx::Error::Protocol(_)
        | sqlx::Error::PoolTimedOut
        | sqlx::Error::PoolClosed
        | sqlx::Error::WorkerCrashed
        | sqlx::Error::BeginFailed => true,
        sqlx::Error::Database(database) => database.code().is_some_and(|code| {
            code.starts_with("08") || matches!(code.as_ref(), "57P01" | "57P02" | "57P03")
        }),
        _ => false,
    }
}

fn map_sql_verification_error(error: sqlx::Error, reason: &'static str) -> SchemaError {
    if sql_error_is_unavailable(&error) {
        SchemaError::Sql(error)
    } else {
        SchemaError::mismatch(reason)
    }
}

fn map_catalog_verification_error(error: CatalogError) -> SchemaError {
    match error {
        CatalogError::Sql(error) => {
            map_sql_verification_error(error, "catalog manifest is unavailable or unreadable")
        }
        CatalogError::Json(_)
        | CatalogError::UnsupportedValue(_)
        | CatalogError::InvalidSeedSpec(_)
        | CatalogError::InvalidCatalog(_) => {
            SchemaError::mismatch("catalog manifest extraction violated its contract")
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionIdentity {
    pub name: String,
    pub version: String,
    pub schema: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SchemaReceiptV1 {
    pub schema_revision: String,
    pub shared_baseline_sha256: String,
    pub knowledge_baseline_sha256: String,
    pub bidding_baseline_sha256: String,
    pub manifest_contract_version: i32,
    pub catalog_manifest_sha256: String,
    pub postgres_server_version_num: i32,
    pub extensions: Vec<ExtensionIdentity>,
    pub release_descriptor_sha256: String,
    pub deployment_namespace_id: Uuid,
    pub created_at: DateTime<Utc>,
}

pub fn database_url() -> Result<String, std::env::VarError> {
    let value = std::env::var("DATABASE_URL")?;
    if value.is_empty() {
        Err(std::env::VarError::NotPresent)
    } else {
        Ok(value)
    }
}

static POOL: tokio::sync::OnceCell<PgPool> = tokio::sync::OnceCell::const_new();

async fn open_pool() -> Result<PgPool, sqlx::Error> {
    let database_url =
        database_url().map_err(|error| sqlx::Error::Configuration(Box::new(error)))?;
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(16)
        .acquire_timeout(std::time::Duration::from_secs(10))
        .connect(&database_url)
        .await
}

/// Explicit unverified connection for the migrator before a receipt can exist.
pub async fn connect_unverified() -> Result<PgPool, sqlx::Error> {
    POOL.get_or_try_init(open_pool).await.cloned()
}

/// Existing request-path pool accessor. Startup verification is performed once by
/// `connect_runtime_verified`; request handlers never bootstrap or repair schema.
pub async fn connect() -> Result<PgPool, sqlx::Error> {
    connect_unverified().await
}

/// Runtime startup gate shared by API, Worker, and Retention.
pub async fn connect_runtime_verified(
    expected_component: crate::SchemaComponentKind,
) -> Result<PgPool, SchemaError> {
    let identity = SchemaRuntimeIdentity::load_from_env()?;
    if identity.component_kind != expected_component {
        return Err(SchemaError::mismatch(
            "component kind differs from executable",
        ));
    }
    let pool = connect_unverified().await?;
    verify_runtime_schema(&pool, &identity).await?;
    Ok(pool)
}

async fn application_object_count(connection: &mut PgConnection) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT count(*) FROM (
           SELECT class_value.oid FROM pg_catalog.pg_class class_value
           JOIN pg_catalog.pg_namespace namespace_value ON namespace_value.oid=class_value.relnamespace
           JOIN pg_catalog.pg_roles owner_role ON owner_role.oid=class_value.relowner
           WHERE owner_role.rolname='kb_app_owner'
             AND namespace_value.nspname NOT IN ('pg_catalog','information_schema')
             AND namespace_value.nspname !~ '^pg_(temp|toast)'
           UNION ALL
           SELECT procedure_value.oid FROM pg_catalog.pg_proc procedure_value
           JOIN pg_catalog.pg_namespace namespace_value ON namespace_value.oid=procedure_value.pronamespace
           JOIN pg_catalog.pg_roles owner_role ON owner_role.oid=procedure_value.proowner
           WHERE owner_role.rolname='kb_app_owner'
             AND namespace_value.nspname NOT IN ('pg_catalog','information_schema')
             AND namespace_value.nspname !~ '^pg_(temp|toast)'
           UNION ALL
           SELECT type_value.oid FROM pg_catalog.pg_type type_value
           JOIN pg_catalog.pg_namespace namespace_value ON namespace_value.oid=type_value.typnamespace
           JOIN pg_catalog.pg_roles owner_role ON owner_role.oid=type_value.typowner
           WHERE owner_role.rolname='kb_app_owner'
             AND namespace_value.nspname NOT IN ('pg_catalog','information_schema')
             AND namespace_value.nspname !~ '^pg_(temp|toast)'
           UNION ALL
           SELECT namespace_value.oid FROM pg_catalog.pg_namespace namespace_value
           JOIN pg_catalog.pg_roles owner_role ON owner_role.oid=namespace_value.nspowner
           WHERE owner_role.rolname='kb_app_owner' AND namespace_value.nspname<>'public'
             AND namespace_value.nspname NOT IN ('pg_catalog','information_schema')
             AND namespace_value.nspname !~ '^pg_(temp|toast)'
         ) application_objects",
    )
    .fetch_one(connection)
    .await
}

async fn installed_extensions(
    connection: &mut PgConnection,
) -> Result<Vec<ExtensionIdentity>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT extension_value.extname AS name,extension_value.extversion AS version,
                namespace_value.nspname AS schema
         FROM pg_catalog.pg_extension extension_value
         JOIN pg_catalog.pg_namespace namespace_value ON namespace_value.oid=extension_value.extnamespace",
    )
    .fetch_all(connection)
    .await?;
    let mut extensions = rows
        .into_iter()
        .map(|row| {
            Ok(ExtensionIdentity {
                name: row.try_get("name")?,
                version: row.try_get("version")?,
                schema: row.try_get("schema")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;
    extensions.sort_by(|left, right| {
        (&left.name, &left.version, &left.schema).cmp(&(&right.name, &right.version, &right.schema))
    });
    Ok(extensions)
}

async fn server_version(connection: &mut PgConnection) -> Result<i32, sqlx::Error> {
    sqlx::query_scalar("SELECT current_setting('server_version_num')::integer")
        .fetch_one(connection)
        .await
}

async fn read_receipt(connection: &mut PgConnection) -> Result<SchemaReceiptV1, SchemaError> {
    let rows = sqlx::query(
        "SELECT schema_revision,shared_baseline_sha256::text,knowledge_baseline_sha256::text,
                bidding_baseline_sha256::text,manifest_contract_version,catalog_manifest_sha256::text,
                postgres_server_version_num,extensions,release_descriptor_sha256::text,
                deployment_namespace_id,created_at
         FROM public.platform_schema_snapshot WHERE singleton",
    )
    .fetch_all(connection)
    .await
    .map_err(|error| {
        map_sql_verification_error(error, "schema receipt is absent or unreadable")
    })?;
    if rows.len() != 1 {
        return Err(SchemaError::mismatch(
            "schema receipt must contain exactly one singleton row",
        ));
    }
    let row = &rows[0];
    let invalid_column = |_| SchemaError::mismatch("schema receipt columns are invalid");
    let extensions: serde_json::Value = row.try_get("extensions").map_err(invalid_column)?;
    let extensions = serde_json::from_value(extensions)
        .map_err(|_| SchemaError::mismatch("schema receipt extensions are invalid"))?;
    Ok(SchemaReceiptV1 {
        schema_revision: row.try_get("schema_revision").map_err(invalid_column)?,
        shared_baseline_sha256: row
            .try_get("shared_baseline_sha256")
            .map_err(invalid_column)?,
        knowledge_baseline_sha256: row
            .try_get("knowledge_baseline_sha256")
            .map_err(invalid_column)?,
        bidding_baseline_sha256: row
            .try_get("bidding_baseline_sha256")
            .map_err(invalid_column)?,
        manifest_contract_version: row
            .try_get("manifest_contract_version")
            .map_err(invalid_column)?,
        catalog_manifest_sha256: row
            .try_get("catalog_manifest_sha256")
            .map_err(invalid_column)?,
        postgres_server_version_num: row
            .try_get("postgres_server_version_num")
            .map_err(invalid_column)?,
        extensions,
        release_descriptor_sha256: row
            .try_get("release_descriptor_sha256")
            .map_err(invalid_column)?,
        deployment_namespace_id: row
            .try_get("deployment_namespace_id")
            .map_err(invalid_column)?,
        created_at: row.try_get("created_at").map_err(invalid_column)?,
    })
}

async fn verify_receipt_on_connection(
    connection: &mut PgConnection,
    identity: &SchemaRuntimeIdentity,
) -> Result<SchemaReceiptV1, SchemaError> {
    let receipt = read_receipt(connection).await?;
    let manifest = build_catalog_manifest(connection)
        .await
        .map_err(map_catalog_verification_error)?;
    let manifest_sha = catalog_manifest_sha256(&manifest)
        .map_err(|_| SchemaError::mismatch("catalog manifest is invalid"))?;
    let extensions = installed_extensions(connection).await.map_err(|error| {
        map_sql_verification_error(error, "installed extension identity is unreadable")
    })?;
    let version = server_version(connection).await.map_err(|error| {
        map_sql_verification_error(error, "PostgreSQL server identity is unreadable")
    })?;
    let exact = receipt.schema_revision == identity.schema_revision()
        && receipt.shared_baseline_sha256 == shared_baseline_sha256()
        && receipt.knowledge_baseline_sha256 == knowledge_baseline_sha256()
        && receipt.bidding_baseline_sha256 == bidding_baseline_sha256()
        && receipt.manifest_contract_version == CATALOG_MANIFEST_CONTRACT_VERSION
        && receipt.catalog_manifest_sha256 == manifest_sha
        && receipt.postgres_server_version_num == version
        && receipt.extensions == extensions
        && receipt.release_descriptor_sha256 == identity.release_descriptor_sha256
        && receipt.deployment_namespace_id == identity.deployment_namespace_id;
    if !exact {
        return Err(SchemaError::mismatch("receipt or catalog identity differs"));
    }
    Ok(receipt)
}

#[doc(hidden)]
pub async fn verify_runtime_schema_on_connection(
    connection: &mut PgConnection,
    identity: &SchemaRuntimeIdentity,
) -> Result<SchemaReceiptV1, SchemaError> {
    verify_receipt_on_connection(connection, identity).await
}

pub async fn verify_runtime_schema(
    pool: &PgPool,
    identity: &SchemaRuntimeIdentity,
) -> Result<SchemaReceiptV1, SchemaError> {
    let mut transaction = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *transaction)
        .await?;
    let receipt = verify_receipt_on_connection(&mut transaction, identity).await?;
    transaction.commit().await?;
    Ok(receipt)
}

pub async fn apply_fresh_baseline(pool: &PgPool) -> Result<(), SchemaError> {
    let identity = SchemaRuntimeIdentity::load_from_env()?;
    apply_fresh_baseline_with_identity(pool, &identity).await
}

pub async fn apply_fresh_baseline_with_identity(
    pool: &PgPool,
    identity: &SchemaRuntimeIdentity,
) -> Result<(), SchemaError> {
    if identity.component_kind != crate::SchemaComponentKind::Migrator {
        return Err(SchemaError::mismatch(
            "fresh baseline requires migrator component identity",
        ));
    }
    let mut lock_connection = pool.acquire().await?;
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(BOOTSTRAP_LOCK_ID)
        .execute(&mut *lock_connection)
        .await?;

    let result = async {
        let snapshot_exists: bool =
            sqlx::query_scalar("SELECT to_regclass('public.platform_schema_snapshot') IS NOT NULL")
                .fetch_one(&mut *lock_connection)
                .await?;
        if snapshot_exists {
            let mut transaction = lock_connection.begin().await?;
            sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
                .execute(&mut *transaction)
                .await?;
            verify_receipt_on_connection(&mut transaction, identity).await?;
            transaction.commit().await?;
            return Ok(());
        }
        if application_object_count(&mut lock_connection).await? != 0 {
            return Err(SchemaError::mismatch(
                "snapshot missing from a partial application catalog",
            ));
        }

        let mut transaction = lock_connection.begin().await?;
        sqlx::query("SET LOCAL ROLE kb_app_owner")
            .execute(&mut *transaction)
            .await?;
        sqlx::raw_sql(SHARED_PLATFORM_BASELINE)
            .execute(&mut *transaction)
            .await?;
        sqlx::raw_sql(KNOWLEDGE_BASE_BASELINE)
            .execute(&mut *transaction)
            .await?;
        sqlx::raw_sql(BIDDING_BASELINE)
            .execute(&mut *transaction)
            .await?;
        crate::catalog::verify_fresh_seed_selection(&mut transaction)
            .await
            .map_err(map_catalog_verification_error)?;
        let manifest = build_catalog_manifest(&mut transaction)
            .await
            .map_err(map_catalog_verification_error)?;
        let manifest_sha = catalog_manifest_sha256(&manifest)
            .map_err(|_| SchemaError::mismatch("catalog manifest is invalid"))?;
        let extensions = installed_extensions(&mut transaction).await?;
        let version = server_version(&mut transaction).await?;
        sqlx::query(
            "INSERT INTO public.platform_schema_snapshot(
               singleton,schema_revision,shared_baseline_sha256,knowledge_baseline_sha256,
               bidding_baseline_sha256,manifest_contract_version,catalog_manifest_sha256,
               postgres_server_version_num,extensions,release_descriptor_sha256,
               deployment_namespace_id,created_at)
             VALUES(true,$1,$2,$3,$4,$5,$6,$7,$8,$9,$10,clock_timestamp())",
        )
        .bind(identity.schema_revision())
        .bind(shared_baseline_sha256())
        .bind(knowledge_baseline_sha256())
        .bind(bidding_baseline_sha256())
        .bind(CATALOG_MANIFEST_CONTRACT_VERSION)
        .bind(manifest_sha)
        .bind(version)
        .bind(serde_json::to_value(extensions).expect("extensions serialize"))
        .bind(&identity.release_descriptor_sha256)
        .bind(identity.deployment_namespace_id)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }
    .await;

    let unlock = sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(BOOTSTRAP_LOCK_ID)
        .execute(&mut *lock_connection)
        .await;
    result?;
    unlock?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_url_has_no_checked_in_fallback() {
        let source = include_str!("db.rs");
        assert!(!source.contains(&["postgres", "://"].concat()));
        assert!(!source.contains(&["154", "32"].concat()));
        assert!(source.contains("std::env::var(\"DATABASE_URL\")?"));
    }

    #[test]
    fn verification_error_mapping_preserves_transport_and_contract_classes() {
        let transport = map_sql_verification_error(
            sqlx::Error::Io(std::io::Error::new(
                std::io::ErrorKind::ConnectionReset,
                "closed",
            )),
            "receipt",
        );
        assert!(matches!(transport, SchemaError::Sql(_)));
        assert_eq!(transport.code(), "POSTGRES_UNAVAILABLE");

        let pool_timeout =
            map_catalog_verification_error(CatalogError::Sql(sqlx::Error::PoolTimedOut));
        assert!(matches!(pool_timeout, SchemaError::Sql(_)));
        let contract = map_sql_verification_error(sqlx::Error::RowNotFound, "receipt");
        assert!(matches!(contract, SchemaError::RevisionMismatch { .. }));
        assert_eq!(contract.code(), SCHEMA_REVISION_MISMATCH);
        let catalog_contract =
            map_catalog_verification_error(CatalogError::InvalidCatalog("fixture".into()));
        assert!(matches!(
            catalog_contract,
            SchemaError::RevisionMismatch { .. }
        ));
    }

    #[test]
    fn baseline_order_receipt_and_owner_boundary_are_explicit() {
        let source = include_str!("db.rs");
        let owner = source.find("SET LOCAL ROLE kb_app_owner").unwrap();
        let shared = source
            .find("sqlx::raw_sql(SHARED_PLATFORM_BASELINE)")
            .unwrap();
        let knowledge = source
            .find("sqlx::raw_sql(KNOWLEDGE_BASE_BASELINE)")
            .unwrap();
        let bidding = source.find("sqlx::raw_sql(BIDDING_BASELINE)").unwrap();
        let manifest = source
            .find("let manifest = build_catalog_manifest(&mut transaction)")
            .unwrap();
        let receipt = source
            .find("INSERT INTO public.platform_schema_snapshot")
            .unwrap();
        assert!(
            owner < shared
                && shared < knowledge
                && knowledge < bidding
                && bidding < manifest
                && manifest < receipt
        );
    }

    #[test]
    fn matching_replay_and_runtime_verification_are_read_only() {
        let source = include_str!("db.rs");
        let verifier = source
            .split_once("async fn verify_receipt_on_connection")
            .unwrap()
            .1
            .split_once("pub async fn verify_runtime_schema")
            .unwrap()
            .0;
        assert!(!verifier.contains("raw_sql"));
        assert!(!verifier.contains("INSERT"));
        assert!(!verifier.contains("UPDATE"));
        assert!(!verifier.contains("DELETE"));
        assert!(!verifier.contains("SET ROLE"));
        assert!(source.contains("REPEATABLE READ READ ONLY"));
        assert!(!source.contains(&["schema_slice", "state"].join("_")));
    }

    #[test]
    fn shared_slice_contains_exact_snapshot_structure_without_seed_row() {
        let snapshot = SHARED_PLATFORM_BASELINE
            .split_once("CREATE TABLE platform_schema_snapshot (")
            .unwrap()
            .1
            .split_once(");")
            .unwrap()
            .0;
        for column in [
            "singleton boolean PRIMARY KEY CHECK (singleton)",
            "schema_revision text NOT NULL",
            "shared_baseline_sha256 char(64) NOT NULL",
            "knowledge_baseline_sha256 char(64) NOT NULL",
            "bidding_baseline_sha256 char(64) NOT NULL",
            "manifest_contract_version integer NOT NULL",
            "catalog_manifest_sha256 char(64) NOT NULL",
            "postgres_server_version_num integer NOT NULL",
            "extensions jsonb NOT NULL",
            "release_descriptor_sha256 char(64) NOT NULL",
            "deployment_namespace_id uuid NOT NULL",
            "created_at timestamptz NOT NULL",
        ] {
            assert!(
                snapshot.contains(column),
                "missing snapshot column {column}"
            );
        }
        assert!(!SHARED_PLATFORM_BASELINE.contains("INSERT INTO platform_schema_snapshot"));
    }

    #[test]
    fn unverified_and_verified_connection_entrypoints_are_separate() {
        let source = include_str!("db.rs");
        assert!(source.contains("pub async fn connect_unverified()"));
        assert!(source.contains("pub async fn connect_runtime_verified("));
        assert!(source.contains("verify_runtime_schema(&pool, &identity).await?"));
    }
}
