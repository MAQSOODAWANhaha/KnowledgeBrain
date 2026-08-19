//! Worker: consume oxana `default` / `document:process` when Redis is up.

use tokio::signal::unix::{SignalKind, signal};
use worker::consume::{AppCtx, run_core};

fn log_line(msg: &str) {
    let _ = std::io::Write::write_all(&mut std::io::stdout(), format!("{msg}\n").as_bytes());
    let _ = std::io::Write::flush(&mut std::io::stdout());
}

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    log_line("worker ready");
    tokio::select! {
        _ = shutdown_signal() => {}
        _ = consume_loop() => {}
    }
    log_line("worker exiting");
}

async fn consume_loop() {
    let pool = storage::connect().await.ok();
    if runtime::connect().is_err() {
        shutdown_signal().await;
        return;
    }
    if let Err(e) = run_core(AppCtx { pool }).await {
        eprintln!("worker consume ended: {e}");
    }
}

async fn shutdown_signal() {
    let mut sigterm = signal(SignalKind::terminate()).expect("sigterm");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = sigterm.recv() => {}
    }
}
