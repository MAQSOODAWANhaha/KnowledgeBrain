//! Drive the real `worker` binary: start, SIGTERM, exit 0. Twice.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn infrastructure_tests_required() -> bool {
    [
        "KNOWLEDGEBRAIN_REQUIRE_POSTGRES_TESTS",
        "KNOWLEDGEBRAIN_REQUIRE_REDIS_TESTS",
    ]
    .iter()
    .any(|name| {
        std::env::var(name)
            .map(|value| {
                matches!(
                    value.to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false)
    })
}

fn launch_test_may_skip() -> bool {
    !infrastructure_tests_required()
        && std::env::var_os("DATABASE_URL").is_none()
        && std::env::var_os("REDIS_URL").is_none()
}

fn worker_command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_worker"));
    command
        .env("KNOWLEDGEBRAIN_CHAT_BASE_URL", "http://127.0.0.1:9/v1")
        .env("KNOWLEDGEBRAIN_CHAT_API_KEY", "worker-launch-test-key")
        .env("KNOWLEDGEBRAIN_CHAT_MODEL", "worker-launch-test-model");
    command
}

fn spawn_ready() -> Child {
    let mut child = worker_command()
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn worker");
    let stdout = child.stdout.take().expect("worker stdout");
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if send.send(line).is_err() {
                break;
            }
        }
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut output = Vec::new();
    loop {
        if let Ok(line) = receive.recv_timeout(Duration::from_millis(100)) {
            let line = line.expect("read worker output");
            let ready = line.contains("worker ready");
            output.push(line);
            if ready {
                return child;
            }
        }
        if let Some(status) = child.try_wait().expect("poll worker startup") {
            panic!(
                "worker exited before readiness ({status}): {}",
                output.join("\n")
            );
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("worker did not become ready: {}", output.join("\n"));
        }
    }
}

fn stop(mut child: Child) {
    // Give the process a moment to install the SIGTERM handler.
    std::thread::sleep(Duration::from_millis(200));
    let ok = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .expect("kill -TERM")
        .success();
    assert!(ok, "kill -TERM worker");
    let status = child.wait().expect("wait worker");
    assert!(status.success(), "worker exit status {status}");
}

#[test]
fn worker_starts_and_exits_twice() {
    if launch_test_may_skip() {
        eprintln!("skip launch test: DATABASE_URL and REDIS_URL are not configured");
        return;
    }
    let a = spawn_ready();
    stop(a);
    let b = spawn_ready();
    stop(b);
}

#[test]
fn worker_fails_before_consuming_when_postgres_initialization_fails() {
    let mut child = worker_command()
        .env("DATABASE_URL", "invalid-database-url")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn worker");
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll worker") {
            break status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("worker did not fail startup");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(!status.success());
}
