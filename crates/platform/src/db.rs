use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use sqlx::{Connection, PgConnection};

pub const KNOWLEDGE_BASE_BASELINE: &str =
    include_str!("../../../migrations/knowledge_base_baseline.sql");
pub const SHARED_PLATFORM_BASELINE: &str =
    include_str!("../../../migrations/shared_platform_baseline.sql");
pub const BIDDING_V1_BASELINE: &str = include_str!("../../../migrations/bidding_v1_baseline.sql");
const MIGRATION_MANIFEST: &str =
    include_str!("../../../deploy/first-launch/migration-manifest.toml");
const BOOTSTRAP_AUTHORITY: &str =
    include_str!("../../../deploy/postgres-init/010-runtime-identities.sh");

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MigrationManifest {
    format_version: u32,
    bootstrap: ManifestBootstrap,
    migrations: Vec<ManifestMigration>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestBootstrap {
    filename: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestMigration {
    version: i32,
    name: String,
    filename: String,
    sha256: String,
}

#[derive(Debug)]
struct EmbeddedMigration {
    version: i32,
    name: &'static str,
    filename: &'static str,
    sql: &'static str,
}

const EMBEDDED_MIGRATIONS: &[EmbeddedMigration] = &[
    EmbeddedMigration {
        version: 1,
        name: "knowledge_base_baseline",
        filename: "knowledge_base_baseline.sql",
        sql: KNOWLEDGE_BASE_BASELINE,
    },
    EmbeddedMigration {
        version: 2,
        name: "shared_platform_baseline",
        filename: "shared_platform_baseline.sql",
        sql: SHARED_PLATFORM_BASELINE,
    },
    EmbeddedMigration {
        version: 3,
        name: "bidding_v1_baseline",
        filename: "bidding_v1_baseline.sql",
        sql: BIDDING_V1_BASELINE,
    },
];

pub const CURRENT_SCHEMA_VERSION: i32 = 3;

const DEFAULT_DATABASE_URL: &str =
    "postgres://knowledgebrain:knowledgebrain@127.0.0.1:15432/knowledgebrain";

fn database_url_from_env_value(
    value: Result<String, std::env::VarError>,
) -> Result<String, std::env::VarError> {
    match value {
        Ok(value) => Ok(value),
        Err(std::env::VarError::NotPresent) => Ok(DEFAULT_DATABASE_URL.into()),
        Err(error) => Err(error),
    }
}

pub fn database_url() -> Result<String, std::env::VarError> {
    database_url_from_env_value(std::env::var("DATABASE_URL"))
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

/// Process-wide runtime pool. Opening it is schema-verification-only: runtime
/// binaries have no configuration switch which can execute DDL.
pub async fn connect() -> Result<PgPool, sqlx::Error> {
    POOL.get_or_try_init(|| async {
        let pool = open_pool().await?;
        verify_schema_identity(&pool).await?;
        Ok(pool)
    })
    .await
    .cloned()
}

/// First-launch-only unverified connection. The API and worker startup paths do
/// not call this interface; it exists solely for the migrate-only process.
pub async fn connect_for_first_launch_migration() -> Result<PgPool, sqlx::Error> {
    open_pool().await
}

/// Commit phase 1 of the irreversible handoff from the migration login to the
/// verifier. This closes authentication and removes authority, but intentionally
/// leaves existing migrator backends for the separate bootstrap-admin phase.
pub async fn handoff_first_launch_to_verifier(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT pg_catalog.kb_handoff_first_launch_to_verifier()")
        .execute(pool)
        .await?;
    Ok(())
}

/// Terminate every backend authenticated as the now-NOLOGIN migrator. The URL
/// must identify the PostgreSQL bootstrap owner; the database helper rejects
/// every other session identity and succeeds only after an exact-zero rescan.
pub async fn terminate_residual_migrator_backends(
    bootstrap_admin_database_url: &str,
) -> Result<(), sqlx::Error> {
    let mut connection = PgConnection::connect(bootstrap_admin_database_url).await?;
    let result = sqlx::query("SELECT pg_catalog.kb_terminate_residual_migrator_backends()")
        .execute(&mut connection)
        .await;
    let close_result = connection.close().await;
    result?;
    close_result?;
    Ok(())
}

pub async fn apply_fresh_baseline(pool: &PgPool) -> Result<(), sqlx::Error> {
    // The explicit first-launch migrator serializes the fixed three-slice fresh
    // baseline. Normal API/worker connection setup never reaches this function.
    const MIGRATION_LOCK_ID: i64 = 0x4b_42_4d_49_47_52_41_54;
    let mut lock_connection = pool.acquire().await?;
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(MIGRATION_LOCK_ID)
        .execute(&mut *lock_connection)
        .await?;
    let result = apply_migrations(pool).await;
    let unlock_result = sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(MIGRATION_LOCK_ID)
        .execute(&mut *lock_connection)
        .await;
    result?;
    unlock_result?;
    Ok(())
}

fn migration_checksum(sql: &str) -> String {
    hex::encode(Sha256::digest(sql.as_bytes()))
}

fn validated_migration_manifest(raw_manifest: &str) -> Result<MigrationManifest, sqlx::Error> {
    let manifest: MigrationManifest = toml::from_str(raw_manifest)
        .map_err(|error| sqlx::Error::Protocol(format!("invalid migration manifest: {error}")))?;
    if manifest.format_version != 1
        || manifest.bootstrap.filename != "deploy/postgres-init/010-runtime-identities.sh"
        || manifest.bootstrap.sha256 != migration_checksum(BOOTSTRAP_AUTHORITY)
        || manifest.migrations.len() != EMBEDDED_MIGRATIONS.len()
    {
        return Err(sqlx::Error::Protocol(
            "baseline manifest bootstrap or slice identity mismatch".into(),
        ));
    }

    let mut versions = std::collections::HashSet::new();
    let mut names = std::collections::HashSet::new();
    let mut filenames = std::collections::HashSet::new();
    for (entry, embedded) in manifest.migrations.iter().zip(EMBEDDED_MIGRATIONS) {
        if !versions.insert(entry.version)
            || !names.insert(entry.name.as_str())
            || !filenames.insert(entry.filename.as_str())
        {
            return Err(sqlx::Error::Protocol(
                "migration manifest contains a duplicate version, name, or filename".into(),
            ));
        }
        if entry.version != embedded.version
            || entry.name != embedded.name
            || entry.filename != embedded.filename
        {
            return Err(sqlx::Error::Protocol(format!(
                "migration manifest catalog mismatch at {}",
                embedded.filename
            )));
        }
        let checksum = migration_checksum(embedded.sql);
        if entry.sha256 != checksum {
            return Err(sqlx::Error::Protocol(format!(
                "migration manifest checksum mismatch at version {}",
                entry.version
            )));
        }
    }
    Ok(manifest)
}

async fn table_exists(pool: &PgPool, table_name: &str) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS (
            SELECT 1 FROM pg_catalog.pg_class relation
            JOIN pg_catalog.pg_namespace namespace ON namespace.oid=relation.relnamespace
            WHERE namespace.nspname='public' AND relation.relname=$1 AND relation.relkind IN ('r','p')
        )",
    )
    .bind(table_name)
    .fetch_one(pool)
    .await
}

async fn verify_migration_ledger_contract(pool: &PgPool) -> Result<(), sqlx::Error> {
    let valid: bool = sqlx::query_scalar(
        r#"SELECT
          (SELECT count(*)=1 AND bool_and(relation.relkind='r' AND relation.relpersistence='p'
             AND relation.relreplident='d' AND relation.relnatts=4 AND relation.relchecks=1
             AND relation.relam=(SELECT oid FROM pg_catalog.pg_am WHERE amname='heap')
             AND relation.reltablespace=0 AND relation.reloptions IS NULL
             AND NOT relation.relrowsecurity AND NOT relation.relforcerowsecurity)
           FROM pg_catalog.pg_class relation
           JOIN pg_catalog.pg_namespace namespace ON namespace.oid=relation.relnamespace
           WHERE namespace.nspname='public' AND relation.relname='schema_migrations')
          AND
          (SELECT count(*)=4
             AND count(*) FILTER(WHERE attribute.attnum=1 AND attribute.attname='version'
               AND pg_catalog.format_type(attribute.atttypid,attribute.atttypmod)='integer'
               AND attribute.attnotnull AND default_value.oid IS NULL)=1
             AND count(*) FILTER(WHERE attribute.attnum=2 AND attribute.attname='name'
               AND pg_catalog.format_type(attribute.atttypid,attribute.atttypmod)='text'
               AND attribute.attnotnull AND default_value.oid IS NULL)=1
             AND count(*) FILTER(WHERE attribute.attnum=3 AND attribute.attname='checksum'
               AND pg_catalog.format_type(attribute.atttypid,attribute.atttypmod)='text'
               AND attribute.attnotnull AND default_value.oid IS NULL)=1
             AND count(*) FILTER(WHERE attribute.attnum=4 AND attribute.attname='applied_at'
               AND pg_catalog.format_type(attribute.atttypid,attribute.atttypmod)='timestamp with time zone'
               AND attribute.attnotnull
               AND pg_catalog.pg_get_expr(default_value.adbin,default_value.adrelid)='now()')=1
             AND bool_and(attribute.attidentity='' AND attribute.attgenerated=''
                          AND attribute.attislocal AND attribute.attinhcount=0
                          AND NOT attribute.atthasmissing AND attribute.attoptions IS NULL
                          AND attribute.attcompression='')
           FROM pg_catalog.pg_attribute attribute
           LEFT JOIN pg_catalog.pg_attrdef default_value
             ON default_value.adrelid=attribute.attrelid AND default_value.adnum=attribute.attnum
           WHERE attribute.attrelid='public.schema_migrations'::regclass
             AND attribute.attnum>0 AND NOT attribute.attisdropped)
          AND
          (SELECT count(*)=3
             AND count(*) FILTER(WHERE constraint_value.conname='schema_migrations_pkey'
               AND constraint_value.contype='p' AND constraint_value.conkey=ARRAY[1]::smallint[]
               AND pg_catalog.pg_get_constraintdef(constraint_value.oid,true)='PRIMARY KEY (version)')=1
             AND count(*) FILTER(WHERE constraint_value.conname='schema_migrations_name_key'
               AND constraint_value.contype='u' AND constraint_value.conkey=ARRAY[2]::smallint[]
               AND pg_catalog.pg_get_constraintdef(constraint_value.oid,true)='UNIQUE (name)')=1
             AND count(*) FILTER(WHERE constraint_value.conname='schema_migrations_checksum_check'
               AND constraint_value.contype='c' AND constraint_value.conkey=ARRAY[3]::smallint[]
               AND pg_catalog.pg_get_expr(constraint_value.conbin,constraint_value.conrelid)
                   = '(checksum ~ ''^[0-9a-f]{64}$''::text)')=1
             AND bool_and(NOT constraint_value.condeferrable AND NOT constraint_value.condeferred
                          AND constraint_value.convalidated)
           FROM pg_catalog.pg_constraint constraint_value
           WHERE constraint_value.conrelid='public.schema_migrations'::regclass)
          AND
          (SELECT count(*)=2
             AND count(*) FILTER(WHERE index_relation.relname='schema_migrations_pkey'
               AND index_value.indisprimary AND index_value.indisunique
               AND index_value.indkey::text='1')=1
             AND count(*) FILTER(WHERE index_relation.relname='schema_migrations_name_key'
               AND NOT index_value.indisprimary AND index_value.indisunique
               AND index_value.indkey::text='2')=1
             AND bool_and(index_value.indisvalid AND index_value.indisready
                          AND index_value.indislive AND index_value.indimmediate
                          AND NOT index_value.indisexclusion)
           FROM pg_catalog.pg_index index_value
           JOIN pg_catalog.pg_class index_relation ON index_relation.oid=index_value.indexrelid
           WHERE index_value.indrelid='public.schema_migrations'::regclass)
          AND
          (SELECT count(*)=1 AND bool_and(function_value.prokind='f'
             AND function_value.prorettype='pg_catalog.trigger'::regtype
             AND language.lanname='plpgsql' AND NOT function_value.prosecdef
             AND NOT function_value.proleakproof AND function_value.provolatile='v'
             AND function_value.proparallel='u' AND function_value.pronargs=0
             AND function_value.proconfig=ARRAY['search_path=pg_catalog, public']::text[]
             AND btrim(regexp_replace(function_value.prosrc,'[[:space:]]+',' ','g'))
                 = 'BEGIN RAISE EXCEPTION ''schema migration ledger is immutable'' USING ERRCODE=''42501''; END'
             AND NOT EXISTS(SELECT 1 FROM aclexplode(COALESCE(function_value.proacl,
                    acldefault('f',function_value.proowner))) acl
                    WHERE acl.grantee=0 AND acl.privilege_type='EXECUTE'))
           FROM pg_catalog.pg_proc function_value
           JOIN pg_catalog.pg_namespace namespace ON namespace.oid=function_value.pronamespace
           JOIN pg_catalog.pg_language language ON language.oid=function_value.prolang
           WHERE namespace.nspname='public'
             AND function_value.proname='kb_reject_schema_migration_mutation'
             AND function_value.proargtypes=''::oidvector)
          AND
          (SELECT count(*)=2
             AND count(*) FILTER(WHERE trigger_value.tgname='schema_migrations_immutable'
               AND trigger_value.tgtype=27 AND trigger_value.tgenabled='O')=1
             AND count(*) FILTER(WHERE trigger_value.tgname='schema_migrations_no_truncate'
               AND trigger_value.tgtype=34 AND trigger_value.tgenabled='O')=1
             AND bool_and(NOT trigger_value.tgisinternal AND trigger_value.tgqual IS NULL
               AND trigger_value.tgnargs=0 AND octet_length(trigger_value.tgargs)=0
               AND trigger_value.tgattr::text=''
               AND trigger_value.tgfoid='public.kb_reject_schema_migration_mutation()'::regprocedure)
           FROM pg_catalog.pg_trigger trigger_value
           WHERE trigger_value.tgrelid='public.schema_migrations'::regclass
             AND NOT trigger_value.tgisinternal)
          AND NOT EXISTS(
           SELECT 1 FROM aclexplode(COALESCE(relation.relacl,acldefault('r',relation.relowner))) acl
           WHERE relation.oid='public.schema_migrations'::regclass AND acl.grantee=0
             AND acl.privilege_type IN ('INSERT','UPDATE','DELETE','TRUNCATE')
          )
         FROM pg_catalog.pg_class relation
         WHERE relation.oid='public.schema_migrations'::regclass"#,
    )
    .fetch_one(pool)
    .await?;
    if !valid {
        return Err(sqlx::Error::Protocol(
            "schema migration ledger contract mismatch".into(),
        ));
    }
    Ok(())
}

async fn verify_migration_ledger(
    pool: &PgPool,
    manifest: &MigrationManifest,
) -> Result<usize, sqlx::Error> {
    verify_migration_ledger_contract(pool).await?;
    let rows: Vec<(i32, String, String)> = sqlx::query_as(
        "SELECT version, name, checksum FROM public.schema_migrations ORDER BY version",
    )
    .fetch_all(pool)
    .await?;
    if rows.len() > manifest.migrations.len() {
        return Err(sqlx::Error::Protocol(
            "migration ledger has unknown or extra versions".into(),
        ));
    }
    for (offset, (version, name, checksum)) in rows.iter().enumerate() {
        let expected = &manifest.migrations[offset];
        if *version != expected.version || name != &expected.name {
            return Err(sqlx::Error::Protocol(format!(
                "migration ledger gap, order, or name mismatch at manifest position {}",
                offset + 1
            )));
        }
        if checksum != &expected.sha256 {
            return Err(sqlx::Error::Protocol(format!(
                "migration checksum mismatch at version {}",
                expected.version
            )));
        }
    }
    Ok(rows.len())
}

async fn persistent_user_relation_exists(pool: &PgPool) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS(
           SELECT 1 FROM pg_catalog.pg_class relation
           JOIN pg_catalog.pg_namespace namespace ON namespace.oid=relation.relnamespace
           WHERE relation.relpersistence IN ('p','u')
             AND relation.relkind IN ('r','p','v','m','S','f','c')
             AND namespace.nspname NOT IN ('pg_catalog','information_schema')
             AND namespace.nspname NOT LIKE 'pg_toast%'
             AND namespace.nspname NOT LIKE 'pg_temp_%'
             AND NOT EXISTS(
               SELECT 1 FROM pg_catalog.pg_depend dependency
               JOIN pg_catalog.pg_extension extension_value
                 ON extension_value.oid=dependency.refobjid
                AND dependency.refclassid='pg_catalog.pg_extension'::regclass
               WHERE dependency.classid='pg_catalog.pg_class'::regclass
                 AND dependency.objid=relation.oid
                 AND dependency.objsubid=0
                 AND dependency.deptype='e'
             )
         )",
    )
    .fetch_one(pool)
    .await
}

const LEDGER_DDL: &str = "CREATE TABLE public.schema_migrations (
    version integer PRIMARY KEY,
    name text NOT NULL UNIQUE,
    checksum text NOT NULL CHECK (checksum ~ '^[0-9a-f]{64}$'),
    applied_at timestamptz NOT NULL DEFAULT now()
 );
 CREATE OR REPLACE FUNCTION public.kb_reject_schema_migration_mutation()
 RETURNS trigger LANGUAGE plpgsql SET search_path=pg_catalog,public AS $$
 BEGIN RAISE EXCEPTION 'schema migration ledger is immutable' USING ERRCODE='42501'; END $$;
 CREATE TRIGGER schema_migrations_immutable BEFORE UPDATE OR DELETE ON public.schema_migrations
 FOR EACH ROW EXECUTE FUNCTION public.kb_reject_schema_migration_mutation();
 CREATE TRIGGER schema_migrations_no_truncate BEFORE TRUNCATE ON public.schema_migrations
 FOR EACH STATEMENT EXECUTE FUNCTION public.kb_reject_schema_migration_mutation();
 REVOKE INSERT, UPDATE, DELETE, TRUNCATE ON public.schema_migrations FROM PUBLIC;
 REVOKE ALL ON FUNCTION public.kb_reject_schema_migration_mutation() FROM PUBLIC;";

async fn create_migration_ledger(pool: &PgPool) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::raw_sql(LEDGER_DDL).execute(&mut *tx).await?;
    tx.commit().await
}

async fn apply_migration_transaction(
    pool: &PgPool,
    version: i32,
    name: &str,
    checksum: &str,
    migration_sql: &'static str,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::raw_sql(migration_sql).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO public.schema_migrations(version,name,checksum) VALUES($1,$2,$3)")
        .bind(version)
        .bind(name)
        .bind(checksum)
        .execute(&mut *tx)
        .await?;
    tx.commit().await
}

async fn apply_migrations(pool: &PgPool) -> Result<(), sqlx::Error> {
    // Validate the checked-in authority against every embedded raw source before
    // inspecting or mutating database state.
    let manifest = validated_migration_manifest(MIGRATION_MANIFEST)?;

    if !table_exists(pool, "schema_migrations").await? {
        if persistent_user_relation_exists(pool).await? {
            return Err(sqlx::Error::Protocol(
                "unledgered database contains persistent user relations".into(),
            ));
        }
        create_migration_ledger(pool).await?;
    }

    let applied = verify_migration_ledger(pool, &manifest).await?;
    for (entry, embedded) in manifest
        .migrations
        .iter()
        .zip(EMBEDDED_MIGRATIONS)
        .skip(applied)
    {
        apply_migration_transaction(
            pool,
            entry.version,
            &entry.name,
            &entry.sha256,
            embedded.sql,
        )
        .await?;
    }
    verify_schema_identity(pool).await
}

/// Verify the exact fixed manifest identity without executing DDL. Every
/// runtime connection and readiness probe uses this closed tuple, not a numeric
/// head probe.
pub async fn verify_schema_identity(pool: &PgPool) -> Result<(), sqlx::Error> {
    let manifest = validated_migration_manifest(MIGRATION_MANIFEST)?;
    if !table_exists(pool, "schema_migrations").await? {
        return Err(sqlx::Error::Protocol(
            "schema migration ledger is absent".into(),
        ));
    }
    let applied = verify_migration_ledger(pool, &manifest).await?;
    if applied != manifest.migrations.len() {
        return Err(sqlx::Error::Protocol(format!(
            "schema manifest is incomplete: expected {} slices, found {applied}",
            manifest.migrations.len()
        )));
    }
    Ok(())
}

pub fn schema_manifest_sha256() -> String {
    migration_checksum(MIGRATION_MANIFEST)
}

pub async fn current_schema_version(pool: &PgPool) -> Result<i32, sqlx::Error> {
    sqlx::query_scalar("SELECT COALESCE(max(version),0) FROM schema_migrations")
        .fetch_one(pool)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_manifest_is_exact_and_checksummed() {
        let manifest = validated_migration_manifest(MIGRATION_MANIFEST).unwrap();
        assert_eq!(manifest.format_version, 1);
        assert_eq!(
            manifest
                .migrations
                .iter()
                .map(|entry| (entry.version, entry.name.as_str(), entry.filename.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (1, "knowledge_base_baseline", "knowledge_base_baseline.sql"),
                (
                    2,
                    "shared_platform_baseline",
                    "shared_platform_baseline.sql"
                ),
                (3, "bidding_v1_baseline", "bidding_v1_baseline.sql"),
            ]
        );
    }

    #[test]
    fn baseline_has_no_incremental_or_legacy_schema_seams() {
        let all = [
            KNOWLEDGE_BASE_BASELINE,
            SHARED_PLATFORM_BASELINE,
            BIDDING_V1_BASELINE,
        ]
        .join("\n")
        .to_ascii_lowercase();
        for forbidden in [
            "add column if not exists",
            "bid_booklet_parts",
            "bid_picks ",
            "bid_commercial_hits",
            "commitroutev1",
        ] {
            assert!(
                !all.contains(forbidden),
                "legacy baseline token remains: {forbidden}"
            );
        }
    }

    #[test]
    fn runtime_connection_source_has_no_migration_switch() {
        let source = include_str!("db.rs");
        let connect_body = source
            .split("pub async fn connect()")
            .nth(1)
            .unwrap()
            .split("pub async fn connect_for_first_launch_migration")
            .next()
            .unwrap();
        assert!(connect_body.contains("verify_schema_identity"));
        assert!(!connect_body.contains("apply_fresh_baseline"));
    }
}
