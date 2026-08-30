use api::{bind_addr, router};
use axum::Router;
use tokio::net::TcpListener;
use tokio::signal::unix::{SignalKind, signal};

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    platform::init_tracing();
    platform::require_openai_chat();
    platform::connect()
        .await
        .unwrap_or_else(|e| panic!("postgres connection failed: {e}"));
    let addr = bind_addr();
    let listener = TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("bind {addr}: {e}"));
    tracing::info!(addr = %addr, "api ready");
    run(listener, router()).await;
    tracing::info!("api exiting");
}

async fn run(listener: TcpListener, app: Router) {
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await;
    if let Err(e) = result {
        panic!("serve: {e}");
    }
}

async fn shutdown_signal() {
    let mut sigterm = signal(SignalKind::terminate()).expect("sigterm");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = sigterm.recv() => {}
    }
}
