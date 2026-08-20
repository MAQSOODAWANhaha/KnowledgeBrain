use api::{bind_addr, router};
use axum::Router;
use std::io::Write;
use tokio::net::TcpListener;
use tokio::signal::unix::{SignalKind, signal};

fn log_ready(addr: &str) {
    let line = format!("api ready on {addr}\n");
    let _ = std::io::stdout().write_all(line.as_bytes());
    let _ = std::io::stdout().flush();
}

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    storage::connect()
        .await
        .unwrap_or_else(|e| panic!("postgres initialization failed: {e}"));
    let addr = bind_addr();
    let listener = TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("bind {addr}: {e}"));
    log_ready(&addr);
    run(listener, router()).await;
    let _ = std::io::stdout().write_all(b"api exiting\n");
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
