//! Opt-in disposable-database acceptance. Never connects without an explicit,
//! loopback-only test URL and never drops application objects.
use sqlx::migrate::{MigrateError, Migration, Migrator};
use sqlx::{AssertSqlSafe, PgPool, SqlSafeStr};
use std::str::FromStr;
use std::time::Duration;

#[tokio::test]
async fn fresh_database_initializes_current_business_without_database_authorization() {
    let required =
        std::env::var("KNOWLEDGEBRAIN_REQUIRE_FRESH_DATABASE_TEST").is_ok_and(|value| value == "1");
    let url = match std::env::var("KNOWLEDGEBRAIN_FRESH_TEST_DATABASE_URL") {
        Ok(value) if !value.trim().is_empty() => value,
        _ if required => panic!("explicit fresh test database URL is required"),
        _ => return,
    };
    let options = sqlx::postgres::PgConnectOptions::from_str(&url).unwrap();
    assert_eq!(options.get_host(), "127.0.0.1");
    assert!(
        options
            .get_database()
            .is_some_and(|name| name.starts_with("knowledgebrain_test_"))
    );
    let pool = PgPool::connect_with(options).await.unwrap();
    let existing: bool =
        sqlx::query_scalar("SELECT to_regclass('public.platform_users') IS NOT NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        !existing,
        "fresh fixture requires an empty application database"
    );
    // Ordinary PostgreSQL name conflicts must leave existing rows intact and
    // roll back the first migration's preceding DDL and success receipt.
    sqlx::query("CREATE TABLE platform_users(id uuid)")
        .execute(&pool)
        .await
        .unwrap();
    let fixture_id = uuid::Uuid::new_v4();
    sqlx::query("INSERT INTO platform_users(id) VALUES($1)")
        .bind(fixture_id)
        .execute(&pool)
        .await
        .unwrap();
    let conflict = platform::migrate_database(&pool).await;
    assert!(
        matches!(
            &conflict,
            Err(MigrateError::ExecuteMigration(sqlx::Error::Database(error), 20261028000001))
                if error.code().as_deref() == Some("42P07")
        ),
        "same-name table must fail naturally: {conflict:?}"
    );
    let original_rows: Vec<uuid::Uuid> = sqlx::query_scalar("SELECT id FROM platform_users")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(original_rows, [fixture_id]);
    let successes: i64 = sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations WHERE success")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(successes, 0);
    let preceding_ddl_rolled_back: bool = sqlx::query_scalar(
        "SELECT to_regtype('public.kb_sha256') IS NULL
         AND to_regtype('public.kb_object_ref') IS NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        preceding_ddl_rolled_back,
        "name conflict must roll back preceding domains"
    );
    // Keep this unrelated fixture and its row throughout every subsequent
    // migration scenario; standard SQLx does not reject nonconflicting objects.
    sqlx::query("ALTER TABLE platform_users RENAME TO migration_fixture_unrelated")
        .execute(&pool)
        .await
        .unwrap();
    let migrations: Vec<_> = platform::MIGRATOR.iter().cloned().collect();
    assert_eq!(
        migrations
            .iter()
            .map(|migration| migration.version)
            .collect::<Vec<_>>(),
        [20261028000001, 20261028000002, 20261028000003]
    );
    assert!(migrations.iter().all(|migration| !migration.no_tx));
    platform::MIGRATOR
        .run_to(migrations[0].version, &pool)
        .await
        .unwrap();
    let applied: Vec<i64> =
        sqlx::query_scalar("SELECT version FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(applied, [migrations[0].version]);
    let owner_id = uuid::Uuid::new_v4();
    sqlx::query("INSERT INTO platform_users(id,email) VALUES($1,$2)")
        .bind(owner_id)
        .bind(format!("migration-fixture-{owner_id}@example.invalid"))
        .execute(&pool)
        .await
        .unwrap();

    // A failed second file must roll back its own DDL and receipt while leaving
    // the first committed file and existing business data intact.
    let mut failing_migrations = migrations.clone();
    let second = &migrations[1];
    failing_migrations[1] = Migration::new(
        second.version,
        second.description.clone(),
        second.migration_type,
        AssertSqlSafe(format!("{}\nSELECT 1 / 0;", second.sql.as_str())).into_sql_str(),
        false,
    );
    let mut connection = pool.acquire().await.unwrap();
    connection.close_on_drop();
    let failed = Migrator::with_migrations(failing_migrations)
        .run(&mut *connection)
        .await;
    connection.close().await.unwrap();
    assert!(matches!(
        failed,
        Err(MigrateError::ExecuteMigration(sqlx::Error::Database(error), 20261028000002))
            if error.code().as_deref() == Some("22012")
    ));
    let rolled_back: bool = sqlx::query_scalar(
        "SELECT to_regclass('public.knowledge_workspaces') IS NULL
         AND (SELECT count(*) FROM _sqlx_migrations) = 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        rolled_back,
        "failed migration must not leave its objects or receipt"
    );

    // Concurrent startup resumes only unapplied files under SQLx's lock.
    let (first, second) = tokio::time::timeout(Duration::from_secs(30), async {
        tokio::join!(
            platform::migrate_database(&pool),
            platform::migrate_database(&pool)
        )
    })
    .await
    .expect("concurrent migration must release its advisory lock");
    first.unwrap();
    second.unwrap();
    let ledger: Vec<(i64, bool, Vec<u8>)> =
        sqlx::query_as("SELECT version,success,checksum FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        ledger,
        migrations
            .iter()
            .map(|migration| { (migration.version, true, migration.checksum.to_vec()) })
            .collect::<Vec<_>>()
    );
    let role: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&pool)
        .await
        .unwrap();
    let owner: String = sqlx::query_scalar(
        "SELECT pg_get_userbyid(relowner)::text FROM pg_class WHERE oid='public.platform_users'::regclass",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(owner, role);
    let functions: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
         WHERE n.nspname='public' AND NOT EXISTS(
           SELECT 1 FROM pg_depend d WHERE d.classid='pg_proc'::regclass AND d.objid=p.oid AND d.deptype='e'
         )",
    ).fetch_one(&pool).await.unwrap();
    assert_eq!(
        functions, 0,
        "business logic belongs in service transactions"
    );
    let triggers: i64 =
        sqlx::query_scalar("SELECT count(*) FROM pg_trigger WHERE NOT tgisinternal")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(triggers, 0);
    let actor_domain: bool =
        sqlx::query_scalar("SELECT to_regtype('public.kb_actor_identity') IS NOT NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        !actor_domain,
        "actor identity domain must not be initialized"
    );
    let actor_columns: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM information_schema.columns
         WHERE table_schema='public' AND column_name LIKE '%actor_identity'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(actor_columns, 0);
    let upload_owner: (String, String) = sqlx::query_as(
        "SELECT udt_name,is_nullable FROM information_schema.columns
         WHERE table_schema='public' AND table_name='platform_object_uploads' AND column_name='owner_user_id'",
    ).fetch_one(&pool).await.unwrap();
    assert_eq!(upload_owner, ("uuid".into(), "YES".into()));
    let upload_owner_fk: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM pg_constraint c
         JOIN pg_attribute source ON source.attrelid=c.conrelid AND source.attname='owner_user_id'
         JOIN pg_attribute target ON target.attrelid=c.confrelid AND target.attname='id'
         WHERE c.contype='f' AND c.conrelid='public.platform_object_uploads'::regclass
           AND c.confrelid='public.platform_users'::regclass
           AND c.conkey=ARRAY[source.attnum] AND c.confkey=ARRAY[target.attnum])",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        upload_owner_fk,
        "upload ownership must reference a real user"
    );
    let duplicated_reference_owner: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM information_schema.columns
         WHERE table_schema='public' AND table_name='platform_object_references' AND column_name='owner_user_id')",
    ).fetch_one(&pool).await.unwrap();
    assert!(
        !duplicated_reference_owner,
        "resource references use their business entity owner"
    );
    let domains: Vec<(String, i64)> = sqlx::query_as(
        "SELECT split_part(tablename,'_',1),count(*) FROM pg_tables
         WHERE schemaname='public' AND tablename NOT IN ('_sqlx_migrations','migration_fixture_unrelated')
         GROUP BY 1 ORDER BY 1",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        domains,
        vec![
            ("knowledge".into(), 26),
            ("platform".into(), 10),
            ("tender".into(), 20)
        ]
    );
    for (table, column) in [
        ("platform_objects", "content_sha256"),
        ("platform_object_uploads", "owner_user_id"),
        ("knowledge_documents", "content_sha256"),
        ("knowledge_documents", "byte_length"),
        ("knowledge_documents", "document_kind"),
        ("knowledge_chunks", "start_offset"),
        ("knowledge_chunks", "end_offset"),
        ("tender_draft_versions", "content_sha256"),
        ("tender_draft_versions", "created_by_user_id"),
    ] {
        let present: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM information_schema.columns WHERE table_schema='public' AND table_name=$1 AND column_name=$2)")
            .bind(table).bind(column).fetch_one(&pool).await.unwrap();
        assert!(present, "missing normalized column {table}.{column}");
    }
    for table in [
        "tender_analysis_runs",
        "tender_analysis_commands",
        "tender_authoring_executions",
        "tender_draft_versions",
        "tender_editor_sessions",
        "tender_draft_state",
        "tender_draft_saves",
        "tender_draft_recoveries",
    ] {
        let present: bool = sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
            .bind(format!("public.{table}"))
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(present, "missing current business table {table}");
    }
    for retired in [
        "workspace_members",
        "bid_outline_runs",
        "bid_response_sets",
        "idempotency_requests",
    ] {
        let absent: bool = sqlx::query_scalar("SELECT to_regclass($1) IS NULL")
            .bind(format!("public.{retired}"))
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(absent, "retired table {retired}");
    }
    let before: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_class WHERE relnamespace='public'::regnamespace",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    platform::migrate_database(&pool).await.unwrap();
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

    // Editing an already applied migration is rejected; the real definitions
    // remain usable afterwards without altering the database ledger.
    let mut changed_migrations = migrations.clone();
    let original = &migrations[0];
    changed_migrations[0] = Migration::new(
        original.version,
        original.description.clone(),
        original.migration_type,
        AssertSqlSafe(format!(
            "{}\n-- changed applied migration",
            original.sql.as_str()
        ))
        .into_sql_str(),
        false,
    );
    let mut connection = pool.acquire().await.unwrap();
    connection.close_on_drop();
    let changed = Migrator::with_migrations(changed_migrations)
        .run(&mut *connection)
        .await;
    connection.close().await.unwrap();
    assert!(matches!(
        changed,
        Err(MigrateError::VersionMismatch(20261028000001))
    ));
    tokio::time::timeout(Duration::from_secs(30), platform::migrate_database(&pool))
        .await
        .expect("checksum rejection must not retain an advisory lock")
        .unwrap();
    let retained: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM platform_users WHERE id=$1)")
            .bind(owner_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        retained,
        "incremental, concurrent and repeated migration must preserve business data"
    );
    let unrelated_rows: Vec<uuid::Uuid> =
        sqlx::query_scalar("SELECT id FROM migration_fixture_unrelated")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        unrelated_rows,
        [fixture_id],
        "unrelated table data must be preserved"
    );
    pool.close().await;
}
