//! Localhost process probe: /live is process-only, /ready inspects Postgres.

use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use tokio::net::TcpListener;

pub const DEFAULT_PROBE_ADDR: &str = "127.0.0.1:8081";

pub fn probe_addr() -> String {
    std::env::var("KNOWLEDGEBRAIN_WORKER_PROBE_ADDR")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_PROBE_ADDR.into())
}

pub fn router() -> Router {
    Router::new()
        .route("/live", get(live))
        .route("/ready", get(ready))
}

async fn live() -> Json<platform::LiveBody> {
    Json(platform::live_body("worker"))
}

async fn ready() -> (StatusCode, Json<platform::ReadyBody>) {
    let check = platform::check_readiness(platform::SchemaComponentKind::Worker).await;
    let status = if check.is_ready() {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, Json(platform::ready_body("worker", &check)))
}

pub async fn bind() -> std::io::Result<TcpListener> {
    TcpListener::bind(probe_addr()).await
}

pub async fn serve(
    listener: TcpListener,
    shutdown: tokio_util::sync::CancellationToken,
) -> std::io::Result<()> {
    axum::serve(listener, router())
        .with_graceful_shutdown(shutdown.cancelled_owned())
        .await
}
