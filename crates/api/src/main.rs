use anyhow::Context;
use api::{bind_addr, router};
use tokio::net::TcpListener;
use tokio::signal::unix::{SignalKind, signal};

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    platform::init_tracing();
    if let Err(error) = run().await {
        tracing::error!(error = format!("{error:#}"), "api failed");
        std::process::exit(1);
    }
}

async fn run() -> anyhow::Result<()> {
    platform::require_openai_chat();
    platform::connect_runtime_verified(platform::SchemaComponentKind::Api)
        .await
        .context("postgres schema readiness failed")?;
    let addr = bind_addr();
    let listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("bind {addr}"))?;
    tracing::info!(addr = %addr, "api ready");
    axum::serve(listener, router())
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("api serve failed")?;
    tracing::info!("api exiting");
    Ok(())
}

async fn shutdown_signal() {
    let mut sigterm = signal(SignalKind::terminate()).expect("sigterm");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = sigterm.recv() => {}
    }
}
