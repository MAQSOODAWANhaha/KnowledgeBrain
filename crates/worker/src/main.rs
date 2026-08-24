//! Worker: consume oxana `default` / `document:process` when Redis is up.

use worker::consume::{AppCtx, run_core};

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    runtime::init_tracing();
    let pool = storage::connect()
        .await
        .unwrap_or_else(|e| panic!("postgres initialization failed: {e}"));
    storage::require_production_first_launch_verified(&pool)
        .await
        .unwrap_or_else(|error| panic!("production first-launch gate failed: {error}"));
    let probe_addr = worker::probe::probe_addr();
    let probe_listener = worker::probe::bind()
        .await
        .unwrap_or_else(|error| panic!("worker probe bind {probe_addr}: {error}"));
    tracing::info!(addr = %probe_addr, "worker probe listening");
    tokio::spawn(worker::probe::serve(probe_listener));
    consume_loop(pool).await;
    tracing::info!("worker exiting");
}

async fn consume_loop(pool: sqlx::PgPool) {
    let mut backoff = std::time::Duration::from_secs(1);
    loop {
        match runtime::connect_verified().await {
            Ok(_) => {
                backoff = std::time::Duration::from_secs(1);
                tracing::info!("worker ready");
                match runtime::replay_orphaned_local_jobs().await {
                    Ok(n) if n > 0 => tracing::info!(replayed = n, "replayed orphaned oxana jobs"),
                    Err(error) => tracing::warn!(%error, "orphan job replay skipped"),
                    _ => {}
                }
                match run_core(AppCtx {
                    pool: Some(pool.clone()),
                })
                .await
                {
                    Ok(()) => return,
                    Err(error) => tracing::error!(%error, "worker consume ended; reconnecting"),
                }
            }
            Err(error) => tracing::warn!(%error, "redis unavailable; reconnecting"),
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(std::time::Duration::from_secs(30));
    }
}
