//! Drive the real `worker` binary: start, SIGTERM, exit 0. Twice.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn spawn_ready() -> Child {
    let mut child = Command::new(env!("CARGO_BIN_EXE_worker"))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn worker");
    let mut ready = String::new();
    BufReader::new(child.stdout.take().expect("worker stdout"))
        .read_line(&mut ready)
        .expect("read worker ready");
    assert!(ready.contains("worker ready"), "unexpected output: {ready}");
    child
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
    let a = spawn_ready();
    stop(a);
    let b = spawn_ready();
    stop(b);
}

#[test]
fn worker_fails_before_consuming_when_postgres_initialization_fails() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_worker"))
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
