//! Worker: consume oxana `default` / `document:process` when Redis is up.

use worker::consume::{AppCtx, run_core};

fn log_line(msg: &str) {
    let _ = std::io::Write::write_all(&mut std::io::stdout(), format!("{msg}\n").as_bytes());
    let _ = std::io::Write::flush(&mut std::io::stdout());
}

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();
    let pool = storage::connect()
        .await
        .unwrap_or_else(|e| panic!("postgres initialization failed: {e}"));
    let extractor = bid::extraction::TenderExtractionEngine::from_env()
        .unwrap_or_else(|e| panic!("bid extraction configuration failed: {e}"));
    eprintln!(
        "bid extraction configured mode={} model={} policy={} prompt={}",
        extractor.mode().as_str(),
        extractor.model_id(),
        extractor.policy_version(),
        extractor.prompt_version()
    );
    consume_loop(pool).await;
    log_line("worker exiting");
}

async fn consume_loop(pool: sqlx::PgPool) {
    let mut backoff = std::time::Duration::from_secs(1);
    loop {
        match runtime::connect_verified().await {
            Ok(_) => {
                backoff = std::time::Duration::from_secs(1);
                log_line("worker ready");
                match runtime::replay_orphaned_local_jobs().await {
                    Ok(n) if n > 0 => eprintln!("replayed {n} orphaned oxana jobs"),
                    Err(error) => eprintln!("orphan job replay skipped: {error}"),
                    _ => {}
                }
                match run_core(AppCtx {
                    pool: Some(pool.clone()),
                })
                .await
                {
                    Ok(()) => return,
                    Err(error) => eprintln!("worker consume ended; reconnecting: {error}"),
                }
            }
            Err(error) => eprintln!("redis unavailable; reconnecting: {error}"),
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(std::time::Duration::from_secs(30));
    }
}
