#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    runtime::init_tracing();
    let pool = storage::connect_for_first_launch_migration()
        .await
        .unwrap_or_else(|error| panic!("postgres migration connection failed: {error}"));
    storage::apply_fresh_baseline(&pool)
        .await
        .unwrap_or_else(|error| panic!("fresh baseline application failed: {error}"));
    storage::handoff_first_launch_to_verifier(&pool)
        .await
        .unwrap_or_else(|error| panic!("first-launch handoff phase 1 failed: {error}"));

    pool.close().await;
    drop(pool);
    let bootstrap_admin_database_url = std::env::var("KNOWLEDGEBRAIN_BOOTSTRAP_ADMIN_DATABASE_URL")
        .expect("KNOWLEDGEBRAIN_BOOTSTRAP_ADMIN_DATABASE_URL is required for handoff phase 2");
    storage::terminate_residual_migrator_backends(&bootstrap_admin_database_url)
        .await
        .unwrap_or_else(|error| panic!("first-launch handoff phase 2 failed: {error}"));
    tracing::info!("fresh baseline and two-phase verifier handoff complete");
}
