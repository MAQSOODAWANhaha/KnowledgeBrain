//! Drive the real `worker` binary: start, SIGTERM, exit 0. Twice.

use std::process::{Child, Command, Stdio};
use std::time::Duration;

fn spawn_ready() -> Child {
    Command::new(env!("CARGO_BIN_EXE_worker"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn worker")
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
