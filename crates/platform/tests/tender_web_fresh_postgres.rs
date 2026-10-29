//! Actual versioned migrator acceptance against one explicit disposable database.
use sqlx::PgPool;
use std::process::Command;
use std::str::FromStr;

#[tokio::test]
#[ignore = "requires an empty loopback test database"]
async fn migrator_tracks_python_and_word_tables_and_preserves_data() {
    let url = std::env::var("KB_TENDER_WEB_FRESH_DATABASE_URL")
        .expect("explicit isolated database URL is required");
    let options = sqlx::postgres::PgConnectOptions::from_str(&url).unwrap();
    assert_eq!(options.get_host(), "127.0.0.1");
    assert!(
        options
            .get_database()
            .is_some_and(|name| name.starts_with("knowledgebrain_test_"))
    );
    let pool = PgPool::connect_with(options).await.unwrap();
    let empty: bool = sqlx::query_scalar("SELECT to_regclass('public.platform_users') IS NULL")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(empty, "fixture requires an empty application database");
    let owner = uuid::Uuid::new_v4();
    for attempt in 0..2 {
        let mut command = Command::new(env!("CARGO_BIN_EXE_migrator"));
        // Avoid reading a developer .env or conflicting inherited DB parts.
        command
            .current_dir(std::env::temp_dir())
            .env("DATABASE_URL", &url);
        for key in [
            "POSTGRES_HOST",
            "POSTGRES_PORT",
            "POSTGRES_DB",
            "POSTGRES_USER",
            "POSTGRES_PASSWORD",
        ] {
            command.env_remove(key);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "migrator failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let versions: Vec<i64> = sqlx::query_scalar(
            "SELECT version FROM _sqlx_migrations WHERE success ORDER BY version",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(versions, [20261028000001, 20261028000002, 20261028000003]);
        let tables: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_class WHERE relnamespace='public'::regnamespace AND relkind='r'
             AND relname=ANY(ARRAY['tender_projects','tender_documents','tender_analysis_runs','tender_analysis_source_versions',
              'tender_analysis_revisions','tender_analysis_commands','tender_analysis_artifacts','tender_analysis_confirmations','tender_analysis_model_requests',
              'tender_authoring_executions','tender_authoring_model_requests','tender_document_sources',
              'tender_project_analyses','tender_authoring_jobs','tender_outline_revisions','tender_draft_versions','tender_editor_sessions',
              'tender_draft_state','tender_draft_saves','tender_draft_recoveries'])",
        ).fetch_one(&pool).await.unwrap();
        assert_eq!(
            tables, 20,
            "all Python and Word business tables must come from the migrator"
        );
        if attempt == 0 {
            sqlx::query("INSERT INTO platform_users(id,email) VALUES($1,$2)")
                .bind(owner)
                .bind(format!("fresh-fixture-{owner}@example.invalid"))
                .execute(&pool)
                .await
                .unwrap();
        }
        let retained: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM platform_users WHERE id=$1)")
                .bind(owner)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(
            retained,
            "repeated initialization must preserve existing data"
        );
    }
    pool.close().await;
}
