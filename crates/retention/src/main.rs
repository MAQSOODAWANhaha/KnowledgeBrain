use axum::{Json, Router, http::StatusCode, routing::get};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    runtime::init_tracing();
    let pool = storage::connect()
        .await
        .unwrap_or_else(|error| panic!("retention schema verification failed: {error}"));
    storage::require_production_first_launch_verified(&pool)
        .await
        .unwrap_or_else(|error| panic!("retention first-launch gate failed: {error}"));

    let consumer_pool = pool.clone();
    tokio::spawn(async move {
        loop {
            match storage::process_one_retention_item(&consumer_pool, "retention-v1").await {
                Ok(true) => continue,
                Ok(false) => tokio::time::sleep(std::time::Duration::from_secs(2)).await,
                Err(error) => {
                    tracing::error!(%error, "retention iteration failed");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        }
    });

    let probe_pool = pool.clone();
    let app = Router::new()
        .route(
            "/live",
            get(|| async { Json(storage::live_body("retention")) }),
        )
        .route(
            "/ready",
            get(move || {
                let pool = probe_pool.clone();
                async move {
                    let check = storage::inspect_readiness(&pool).await;
                    let status = if check.is_ready() {
                        StatusCode::OK
                    } else {
                        StatusCode::SERVICE_UNAVAILABLE
                    };
                    (status, Json(storage::ready_body("retention", &check)))
                }
            }),
        );
    let address =
        std::env::var("RETENTION_PROBE_ADDR").unwrap_or_else(|_| "0.0.0.0:8082".to_owned());
    let listener = TcpListener::bind(&address)
        .await
        .unwrap_or_else(|error| panic!("retention probe bind {address}: {error}"));
    tracing::info!(%address, "retention consumer ready");
    axum::serve(listener, app)
        .await
        .unwrap_or_else(|error| panic!("retention probe failed: {error}"));
}
