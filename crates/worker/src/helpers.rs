//! Unix helper subprocesses for immutable object I/O.

use crate::runtime::{JobErr, TASK_ABORT_DRAIN_RESERVE, non_agent_sql_error};
use async_trait::async_trait;
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub(crate) const MAX_EXPORT_INPUT_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const MAX_TENDER_DOCUMENT_BYTES: usize = 50 * 1024 * 1024;
pub(crate) const MAX_RENDER_OUTPUT_BYTES: usize = 256 * 1024 * 1024;

pub(crate) async fn stage_export_object(
    pool: &PgPool,
    staging_id: Uuid,
    digest: &str,
    media_type: &str,
    bytes: &[u8],
    actor: &str,
) -> Result<String, JobErr> {
    if bytes.is_empty() || bytes.len() > MAX_RENDER_OUTPUT_BYTES {
        return Err(JobErr(
            "staged export object exceeds the byte budget".into(),
        ));
    }
    let object_ref = platform::object_ref(digest);
    let byte_length =
        i64::try_from(bytes.len()).map_err(|_| JobErr("rendered object too large".into()))?;
    platform::stage_object_upload(
        pool,
        staging_id,
        &object_ref,
        digest,
        media_type,
        byte_length,
        actor,
    )
    .await
    .map_err(non_agent_sql_error)?;
    if let Err(error) = platform::write_blob_off_runtime(digest, bytes) {
        let _ = platform::schedule_object_upload_cleanup(staging_id).await;
        return Err(JobErr(format!(
            "TRANSIENT_HANDLER:write rendered object: {error}"
        )));
    }
    Ok(object_ref)
}

pub(crate) async fn abort_task_until<T>(
    task: &mut tokio::task::JoinHandle<T>,
    deadline: tokio::time::Instant,
) {
    task.abort();
    let _ = tokio::time::timeout_at(deadline, &mut *task).await;
}

pub(crate) async fn kill_and_reap_child(
    child: &mut tokio::process::Child,
    label: &str,
    deadline: tokio::time::Instant,
) -> Result<(), JobErr> {
    let kill_error = child.start_kill().err();
    tokio::time::timeout_at(deadline, child.wait())
        .await
        .map_err(|_| JobErr(format!("reap {label} exceeded cleanup deadline")))?
        .map_err(|error| JobErr(format!("reap {label}: {error}")))?;
    if let Some(error) = kill_error
        && error.kind() != std::io::ErrorKind::InvalidInput
    {
        return Err(JobErr(format!("kill {label}: {error}")));
    }
    Ok(())
}

pub(crate) async fn kill_helper_group_and_reap_child(
    child: &mut tokio::process::Child,
    label: &str,
    deadline: tokio::time::Instant,
) -> Result<(), JobErr> {
    let Some(pid) = child.id() else {
        return kill_and_reap_child(child, label, deadline).await;
    };
    let group_result = i32::try_from(pid)
        .map_err(|_| JobErr(format!("{label} process id exceeds i32")))
        .and_then(|pid| {
            // SAFETY: `killpg` is called with a valid signal constant and the
            // checked process-group leader PID assigned at helper spawn.
            let result = unsafe { libc::killpg(pid, libc::SIGKILL) };
            if result == 0 {
                Ok(())
            } else {
                Err(JobErr(format!(
                    "kill process group for {label}: {}",
                    std::io::Error::last_os_error()
                )))
            }
        });
    if group_result.is_err() {
        child
            .start_kill()
            .map_err(|error| JobErr(format!("kill direct child for {label}: {error}")))?;
    }
    tokio::time::timeout_at(deadline, child.wait())
        .await
        .map_err(|_| JobErr(format!("reap {label} exceeded cleanup deadline")))?
        .map_err(|error| JobErr(format!("reap {label}: {error}")))?;
    Ok(())
}

pub(crate) const OBJECT_READ_HELPER_ARG: &str = "--kb-object-read-helper-v1";

pub fn run_object_read_helper(arguments: &[String]) -> Result<(), String> {
    use std::io::Write;
    if arguments.len() != 3 || arguments[0] != OBJECT_READ_HELPER_ARG {
        return Err("invalid object read helper arguments".into());
    }
    let digest = &arguments[1];
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err("invalid object read digest".into());
    }
    let max_bytes = arguments[2]
        .parse::<usize>()
        .map_err(|_| "invalid object read byte budget".to_string())?;
    let bytes = platform::read_blob(digest).map_err(|error| error.to_string())?;
    if bytes.is_empty() || bytes.len() > max_bytes {
        return Err("object read helper output exceeds budget".into());
    }
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(&bytes)
        .map_err(|error| error.to_string())?;
    stdout.flush().map_err(|error| error.to_string())
}

pub(crate) const OBJECT_WRITE_HELPER_ARG: &str = "--kb-object-write-helper-v1";

pub fn run_object_write_helper(arguments: &[String]) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    use std::io::{Read, Write};
    if arguments.len() != 3 || arguments[0] != OBJECT_WRITE_HELPER_ARG {
        return Err("invalid object write arguments".into());
    }
    let digest = &arguments[1];
    let length = arguments[2]
        .parse::<usize>()
        .map_err(|_| "invalid object write length")?;
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        || length == 0
        || length == usize::MAX
    {
        return Err("invalid object write identity".into());
    }
    let mut bytes = Vec::new();
    std::io::stdin()
        .lock()
        .take(length as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() != length || hex::encode(Sha256::digest(&bytes)) != *digest {
        return Err("object write digest or length mismatch".into());
    }
    platform::write_blob_off_runtime(digest, &bytes).map_err(|e| e.to_string())?;
    std::io::stdout()
        .lock()
        .write_all(b"ok")
        .map_err(|e| e.to_string())
}

pub(crate) struct HelperCompositionObjects;
#[async_trait]
impl bidding::docx_composition::runtime::ObjectIo for HelperCompositionObjects {
    async fn read(
        &self,
        sha: &str,
        max_bytes: usize,
        cancel: &CancellationToken,
    ) -> Result<Vec<u8>, bidding::agent_error::AgentError> {
        read_blob_in_helper(sha, max_bytes, cancel)
            .await
            .map_err(|e| bidding::agent_error::AgentError::new("INTERNAL", e.0))
    }
    async fn write(
        &self,
        sha: &str,
        bytes: &[u8],
        cancel: &CancellationToken,
    ) -> Result<(), bidding::agent_error::AgentError> {
        run_helper_capture(
            &[
                OBJECT_WRITE_HELPER_ARG.into(),
                sha.into(),
                bytes.len().to_string(),
            ],
            bytes.to_vec(),
            2,
            std::time::Duration::from_secs(5 * 60),
            cancel,
            "object write helper",
        )
        .await
        .map(|_| ())
        .map_err(|e| bidding::agent_error::AgentError::new("INTERNAL", e.0))
    }
}

pub(crate) struct HelperExportIo;

#[async_trait]
impl bidding::submission_export::ExportIo for HelperExportIo {
    async fn read_blob(
        &self,
        sha256: &str,
        max_bytes: usize,
        cancel: &CancellationToken,
    ) -> Result<Vec<u8>, bidding::submission_export::ExportError> {
        read_blob_in_helper(sha256, max_bytes, cancel)
            .await
            .map_err(|error| bidding::submission_export::ExportError(error.0))
    }

    async fn stage_object(
        &self,
        pool: &PgPool,
        staging_id: Uuid,
        digest: &str,
        media_type: &str,
        bytes: &[u8],
        actor: &str,
    ) -> Result<String, bidding::submission_export::ExportError> {
        stage_export_object(pool, staging_id, digest, media_type, bytes, actor)
            .await
            .map_err(|error| bidding::submission_export::ExportError(error.0))
    }
}

pub(crate) async fn run_helper_capture(
    arguments: &[String],
    input: Vec<u8>,
    max_output_bytes: usize,
    timeout: std::time::Duration,
    cancel: &CancellationToken,
    label: &str,
) -> Result<Vec<u8>, JobErr> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let executable = std::env::current_exe()
        .map_err(|error| JobErr(format!("resolve {label} executable: {error}")))?;
    let mut command = tokio::process::Command::new(executable);
    command
        .args(arguments)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    command.process_group(0);
    let mut child = command
        .spawn()
        .map_err(|error| JobErr(format!("start {label}: {error}")))?;
    let deadline = tokio::time::Instant::now() + timeout;
    let work_deadline = deadline
        .checked_sub(TASK_ABORT_DRAIN_RESERVE)
        .unwrap_or(deadline);
    let mut stdin = match child.stdin.take() {
        Some(stdin) => stdin,
        None => {
            kill_helper_group_and_reap_child(&mut child, &format!("invalid {label}"), deadline)
                .await?;
            return Err(JobErr(format!("{label} stdin unavailable")));
        }
    };
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            kill_helper_group_and_reap_child(&mut child, &format!("invalid {label}"), deadline)
                .await?;
            return Err(JobErr(format!("{label} stdout unavailable")));
        }
    };
    let mut writer = tokio::spawn(async move {
        stdin.write_all(&input).await?;
        stdin.shutdown().await
    });
    let mut reader = tokio::spawn(async move {
        let mut output = Vec::new();
        stdout
            .take((max_output_bytes + 1) as u64)
            .read_to_end(&mut output)
            .await?;
        Ok::<_, std::io::Error>(output)
    });
    let output = tokio::select! {
        biased;
        () = cancel.cancelled() => {
            kill_helper_group_and_reap_child(&mut child, &format!("cancelled {label}"), deadline).await?;
            abort_task_until(&mut writer, deadline).await;
            abort_task_until(&mut reader, deadline).await;
            return Err(JobErr("WORKER_SHUTDOWN".into()));
        }
        () = tokio::time::sleep_until(work_deadline) => {
            kill_helper_group_and_reap_child(&mut child, &format!("timed out {label}"), deadline).await?;
            abort_task_until(&mut writer, deadline).await;
            abort_task_until(&mut reader, deadline).await;
            return Err(JobErr(format!("{label} timed out")));
        }
        joined = &mut reader => match joined {
            Ok(Ok(output)) => Ok(output),
            Ok(Err(error)) => Err(JobErr(format!("read {label} output: {error}"))),
            Err(error) => Err(JobErr(format!("join {label} output: {error}"))),
        },
    };
    let output = match output {
        Ok(output) => output,
        Err(error) => {
            let _ =
                kill_helper_group_and_reap_child(&mut child, &format!("failed {label}"), deadline)
                    .await;
            abort_task_until(&mut writer, deadline).await;
            return Err(error);
        }
    };
    if output.len() > max_output_bytes {
        kill_helper_group_and_reap_child(&mut child, &format!("oversized {label}"), deadline)
            .await?;
        abort_task_until(&mut writer, deadline).await;
        return Err(JobErr(format!("{label} output exceeds budget")));
    }
    let writer_result = tokio::select! {
        biased;
        () = cancel.cancelled() => {
            kill_helper_group_and_reap_child(&mut child, &format!("cancelled {label}"), deadline).await?;
            abort_task_until(&mut writer, deadline).await;
            return Err(JobErr("WORKER_SHUTDOWN".into()));
        }
        () = tokio::time::sleep_until(work_deadline) => {
            kill_helper_group_and_reap_child(&mut child, &format!("timed out {label}"), deadline).await?;
            abort_task_until(&mut writer, deadline).await;
            return Err(JobErr(format!("{label} timed out")));
        }
        joined = &mut writer => match joined {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(JobErr(format!("write {label} input: {error}"))),
            Err(error) => Err(JobErr(format!("join {label} input: {error}"))),
        },
    };
    if let Err(error) = writer_result {
        let _ = kill_helper_group_and_reap_child(&mut child, &format!("failed {label}"), deadline)
            .await;
        return Err(error);
    }
    let status = tokio::select! {
        biased;
        () = cancel.cancelled() => {
            kill_helper_group_and_reap_child(&mut child, &format!("cancelled {label}"), deadline).await?;
            return Err(JobErr("WORKER_SHUTDOWN".into()));
        }
        () = tokio::time::sleep_until(work_deadline) => {
            kill_helper_group_and_reap_child(&mut child, &format!("timed out {label}"), deadline).await?;
            return Err(JobErr(format!("{label} timed out")));
        }
        status = child.wait() => status.map_err(|error| JobErr(format!("wait {label}: {error}"))),
    };
    let status = match status {
        Ok(status) => status,
        Err(error) => {
            let _ =
                kill_helper_group_and_reap_child(&mut child, &format!("failed {label}"), deadline)
                    .await;
            return Err(error);
        }
    };
    if !status.success() {
        return Err(JobErr(format!("{label} failed")));
    }
    if output.is_empty() {
        return Err(JobErr(format!("{label} returned no bytes")));
    }
    Ok(output)
}

pub(crate) async fn read_blob_in_helper(
    digest: &str,
    max_bytes: usize,
    cancel: &CancellationToken,
) -> Result<Vec<u8>, JobErr> {
    let result = run_helper_capture(
        &[
            OBJECT_READ_HELPER_ARG.into(),
            digest.into(),
            max_bytes.to_string(),
        ],
        Vec::new(),
        max_bytes,
        std::time::Duration::from_secs(5 * 60),
        cancel,
        "object read helper",
    )
    .await;
    result.map_err(|error| {
        if error.0.contains("exceeds budget") {
            error
        } else {
            JobErr(format!("TRANSIENT_HANDLER:{}", error.0))
        }
    })
}
