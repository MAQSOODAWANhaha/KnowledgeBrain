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

fn unique_probe_addr() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("ephemeral probe port");
    let addr = listener.local_addr().expect("probe local addr");
    drop(listener);
    addr.to_string()
}

fn worker_command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_worker"));
    command
        .env("KNOWLEDGEBRAIN_CHAT_BASE_URL", "http://127.0.0.1:9/v1")
        .env("KNOWLEDGEBRAIN_CHAT_API_KEY", "worker-launch-test-key")
        .env("KNOWLEDGEBRAIN_CHAT_MODEL", "worker-launch-test-model")
        .env("KNOWLEDGEBRAIN_WORKER_PROBE_ADDR", unique_probe_addr());
    command
}

fn hanging_child_command() -> Command {
    let mut command = Command::new("/bin/sleep");
    command.arg("60");
    command
}

fn process_state(pid: u32) -> Option<char> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after_comm = stat.rsplit_once(')')?.1;
    after_comm.trim_start().chars().next()
}

fn spawn_ready() -> Child {
    spawn_ready_from(worker_command())
}

fn spawn_ready_from(mut command: Command) -> Child {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn worker");
    let pid = child.id();
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
            let ready = line.contains("worker probe listening");
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
            let reap_by = Instant::now() + Duration::from_secs(2);
            let mut reaped = false;
            while Instant::now() <= reap_by {
                if child.try_wait().expect("reap killed worker").is_some() {
                    reaped = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            let zombie = process_state(pid) == Some('Z');
            panic!(
                "worker did not become ready (waited/reaped={reaped} zombie={zombie}): {}",
                output.join("\n")
            );
        }
    }
}

fn stop(child: Child) {
    stop_with_signal(child, libc::SIGTERM);
}

fn stop_with_signal(mut child: Child, signal: i32) {
    // Give the process a moment to install the signal handler.
    std::thread::sleep(Duration::from_millis(200));
    let pid = i32::try_from(child.id()).expect("worker pid fits i32");
    // SAFETY: the PID belongs to this test-owned child and the signal is valid.
    assert_eq!(unsafe { libc::kill(pid, signal) }, 0);
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
fn worker_starts_and_exits_on_sigint() {
    if launch_test_may_skip() {
        eprintln!("skip launch test: DATABASE_URL and REDIS_URL are not configured");
        return;
    }
    let child = spawn_ready();
    stop_with_signal(child, libc::SIGINT);
}

#[test]
#[should_panic(expected = "waited/reaped=true zombie=false")]
fn spawn_ready_timeout_reaps_hanging_child() {
    let mut child = spawn_ready_from(hanging_child_command());
    // If readiness unexpectedly succeeds, still reap the owned process before
    // failing with a message that cannot satisfy the expected timeout panic.
    child.kill().expect("kill unexpectedly ready child");
    child.wait().expect("reap unexpectedly ready child");
    panic!("hanging child unexpectedly reported readiness");
}

#[test]
fn fatal_core_configuration_failure_exits_nonzero() {
    if std::env::var_os("DATABASE_URL").is_none() {
        eprintln!("skip fatal core test: DATABASE_URL is not configured");
        return;
    }
    let mut child = worker_command()
        .env("REDIS_URL", "://invalid-redis-url")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn worker");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().expect("poll worker") {
            assert!(!status.success(), "fatal core error exited successfully");
            break;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            child
                .wait()
                .expect("reap worker after fatal configuration timeout");
            panic!("fatal core configuration did not stop the worker");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
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
