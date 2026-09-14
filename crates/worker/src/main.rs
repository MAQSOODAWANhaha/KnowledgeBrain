//! Worker process supervisor.

use tokio_util::sync::CancellationToken;
use worker::helpers::{
    run_object_read_helper, run_object_write_helper, run_pdf_raster_helper,
    run_submission_render_helper,
};
use worker::runtime::{AppCtx, run_core, shutdown_signal};

#[tokio::main]
async fn main() {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if let Some(helper) = arguments.first().map(String::as_str) {
        let result = match helper {
            "--kb-submission-render-helper-v1" => run_submission_render_helper(&arguments),
            "--kb-object-write-helper-v1" => run_object_write_helper(&arguments),
            "--kb-object-read-helper-v1" => run_object_read_helper(&arguments),
            "--kb-pdf-raster-helper-v1" => run_pdf_raster_helper(&arguments),
            _ => Ok(()),
        };
        if matches!(
            helper,
            "--kb-submission-render-helper-v1"
                | "--kb-object-write-helper-v1"
                | "--kb-object-read-helper-v1"
                | "--kb-pdf-raster-helper-v1"
        ) {
            if let Err(error) = result {
                eprintln!("worker helper failed: {error}");
                std::process::exit(2);
            }
            return;
        }
    }

    let _ = dotenvy::dotenv();
    platform::init_tracing();
    platform::require_openai_chat();
    let external_shutdown = CancellationToken::new();
    let root_cancel = CancellationToken::new();
    let signal_shutdown = external_shutdown.clone();
    let signal_task = tokio::spawn(async move {
        shutdown_signal().await;
        signal_shutdown.cancel();
    });
    let pool = tokio::select! {
        biased;
        () = external_shutdown.cancelled() => {
            let _ = tokio::time::timeout(
                std::time::Duration::from_millis(100),
                signal_task,
            )
            .await;
            tracing::info!("worker shutdown requested during PostgreSQL startup");
            return;
        }
        result = platform::connect_runtime_verified(platform::SchemaComponentKind::Worker) => {
            result.unwrap_or_else(|error| panic!("postgres schema readiness failed: {error}"))
        }
    };

    let probe_addr = worker::probe::probe_addr();
    let probe_listener = worker::probe::bind()
        .await
        .unwrap_or_else(|error| panic!("worker probe bind {probe_addr}: {error}"));
    tracing::info!(addr = %probe_addr, "worker probe listening");
    let probe_cancel = CancellationToken::new();
    let mut probe_task = tokio::spawn(worker::probe::serve(probe_listener, probe_cancel.clone()));
    let mut core_task = tokio::spawn(run_core(AppCtx {
        pool: Some(pool),
        shutdown: external_shutdown.clone(),
        root_cancel: root_cancel.clone(),
    }));

    enum ProcessCompletion {
        External,
        Core(Result<Result<(), String>, tokio::task::JoinError>),
        Probe(Result<std::io::Result<()>, tokio::task::JoinError>),
    }
    let completion = tokio::select! {
        biased;
        () = external_shutdown.cancelled() => ProcessCompletion::External,
        result = &mut core_task => ProcessCompletion::Core(result),
        result = &mut probe_task => ProcessCompletion::Probe(result),
    };
    let core_selected = matches!(completion, ProcessCompletion::Core(_));
    let probe_selected = matches!(completion, ProcessCompletion::Probe(_));
    let external_selected = matches!(completion, ProcessCompletion::External);
    let mut result = match completion {
        ProcessCompletion::External => Ok(()),
        ProcessCompletion::Core(Ok(Err(error))) => Err(error),
        ProcessCompletion::Core(Ok(Ok(()))) => {
            Err("worker core exited before external shutdown".into())
        }
        ProcessCompletion::Core(Err(error)) => Err(format!("worker core join failed: {error}")),
        ProcessCompletion::Probe(Ok(Ok(()))) => {
            Err("worker probe exited before external shutdown".into())
        }
        ProcessCompletion::Probe(Ok(Err(error))) => Err(format!("worker probe failed: {error}")),
        ProcessCompletion::Probe(Err(error)) => Err(format!("worker probe join failed: {error}")),
    };

    // Local cancellation stops siblings after a fatal child. It is intentionally
    // distinct from external_shutdown so a core failure can never look graceful.
    root_cancel.cancel();
    probe_cancel.cancel();
    let cleanup_deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
    let abort_deadline = cleanup_deadline
        .checked_sub(std::time::Duration::from_millis(100))
        .unwrap_or(cleanup_deadline);
    if !core_selected {
        match tokio::time::timeout_at(abort_deadline, &mut core_task).await {
            Ok(Ok(Err(error))) if result.is_ok() => result = Err(error),
            Ok(Err(error)) if result.is_ok() => {
                result = Err(format!("worker core shutdown join failed: {error}"));
            }
            Err(_) => {
                core_task.abort();
                let _ = tokio::time::timeout_at(cleanup_deadline, &mut core_task).await;
                if result.is_ok() {
                    result = Err("worker core shutdown exceeded 30 seconds".into());
                }
            }
            _ => {}
        }
    }
    if !probe_selected {
        match tokio::time::timeout_at(abort_deadline, &mut probe_task).await {
            Ok(Ok(Err(error))) if result.is_ok() => {
                result = Err(format!("worker probe shutdown failed: {error}"));
            }
            Ok(Err(error)) if result.is_ok() => {
                result = Err(format!("worker probe shutdown join failed: {error}"));
            }
            Err(_) => {
                probe_task.abort();
                let _ = tokio::time::timeout_at(cleanup_deadline, &mut probe_task).await;
                if result.is_ok() {
                    result = Err("worker probe shutdown exceeded 30 seconds".into());
                }
            }
            _ => {}
        }
    }
    if !external_selected {
        signal_task.abort();
    }
    let _ = tokio::time::timeout_at(cleanup_deadline, signal_task).await;

    match result {
        Ok(()) => tracing::info!("worker exiting after external shutdown"),
        Err(error) => {
            tracing::error!(%error, "worker supervisor failed");
            std::process::exit(1);
        }
    }
}
