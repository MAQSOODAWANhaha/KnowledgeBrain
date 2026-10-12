// Included only inside test modules. Never use the application DATABASE_URL here.
async fn reset_isolated_schema(app_pool: &sqlx::PgPool) -> Result<(), sqlx::Error> {
    fn require_isolated(options: &sqlx::postgres::PgConnectOptions) -> Result<(), sqlx::Error> {
        if options.get_host() != "127.0.0.1"
            || options.get_port() != 25433
            || !options
                .get_database()
                .is_some_and(|name| name.starts_with("knowledgebrain_test_"))
        {
            return Err(sqlx::Error::Configuration(
                "schema reset requires 127.0.0.1:25433/knowledgebrain_test_*".into(),
            ));
        }
        Ok(())
    }
    let app_options = app_pool.connect_options();
    require_isolated(&app_options)?;
    let admin_url = std::env::var("KNOWLEDGEBRAIN_TEST_ADMIN_DATABASE_URL").map_err(|_| {
        sqlx::Error::Configuration(
            "explicit isolated KNOWLEDGEBRAIN_TEST_ADMIN_DATABASE_URL required".into(),
        )
    })?;
    let admin_options: sqlx::postgres::PgConnectOptions = admin_url.parse()?;
    require_isolated(&admin_options)?;
    if app_options.get_database() != admin_options.get_database() {
        return Err(sqlx::Error::Configuration(
            "test administrator and application databases differ".into(),
        ));
    }
    let (app_user, superuser): (String, bool) = sqlx::query_as(
        "SELECT current_user::text,rolsuper FROM pg_roles WHERE rolname=current_user",
    )
    .fetch_one(app_pool)
    .await?;
    if superuser || app_user == admin_options.get_username() {
        return Err(sqlx::Error::Configuration(
            "business tests require a separate non-superuser application identity".into(),
        ));
    }
    let quoted_user = format!("\"{}\"", app_user.replace('"', "\"\""));
    let admin = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect_with(admin_options)
        .await?;
    let reset_sql = format!(
        "DROP SCHEMA public CASCADE;
         CREATE SCHEMA public AUTHORIZATION {quoted_user};
         REVOKE ALL ON SCHEMA public FROM PUBLIC;
         CREATE EXTENSION IF NOT EXISTS pgcrypto WITH SCHEMA public;
         CREATE EXTENSION IF NOT EXISTS vector WITH SCHEMA public;"
    );
    // Identifiers are escaped above; both DSNs are restricted to the same disposable test DB.
    sqlx::raw_sql(sqlx::AssertSqlSafe(reset_sql.as_str()))
        .execute(&admin)
        .await?;
    admin.close().await;
    Ok(())
}
