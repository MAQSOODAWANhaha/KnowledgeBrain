#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    platform::init_tracing();
    let pool = platform::connect()
        .await
        .unwrap_or_else(|error| panic!("postgres schema verification failed: {error}"));
    let app_owner = std::env::var("KNOWLEDGEBRAIN_APP_OWNER")
        .expect("KNOWLEDGEBRAIN_APP_OWNER is required for first-launch verification");
    let bootstrap_owner = std::env::var("KNOWLEDGEBRAIN_BOOTSTRAP_OWNER")
        .expect("KNOWLEDGEBRAIN_BOOTSTRAP_OWNER is required for first-launch verification");
    let evidence = platform::verify_fresh_pretraffic_catalog_rows(
        &pool,
        &platform::FreshPretrafficOwnerBindings {
            app_owner,
            bootstrap_owner,
        },
    )
    .await
    .unwrap_or_else(|error| panic!("first-launch catalog/row verification failed: {error}"));
    tracing::info!(
        allowlist_sha256 = %evidence.allowlist_sha256,
        catalog_sha256 = %evidence.catalog_sha256,
        rows_sha256 = %evidence.rows_sha256,
        "fresh pretraffic catalog and seed rows verified"
    );
}
