#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    platform::init_tracing();
    let pool = platform::connect()
        .await
        .unwrap_or_else(|error| panic!("postgres bootstrap connection failed: {error}"));
    platform::apply_fresh_baseline(&pool)
        .await
        .unwrap_or_else(|error| panic!("fresh schema bootstrap failed: {error}"));
    tracing::info!("fresh schema bootstrap complete");
}
