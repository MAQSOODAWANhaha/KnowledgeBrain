//! Drive the real `api` binary: bind, GET /health, SIGTERM, exit 0. Twice.

use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("ephemeral bind")
        .local_addr()
        .expect("local_addr")
        .port()
}

fn wait_port(port: u16) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        if Instant::now() > deadline {
            panic!("api did not listen on {port}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn spawn_ready(port: u16) -> Child {
    let child = Command::new(env!("CARGO_BIN_EXE_api"))
        .env("API_PORT", port.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn api");
    wait_port(port);
    child
}

fn get_health(port: u16) -> (u16, String) {
    let url = format!("http://127.0.0.1:{port}/health");
    let resp = Command::new("curl")
        .args(["-sS", "-w", "\n%{http_code}", url.as_str()])
        .output()
        .expect("curl");
    assert!(resp.status.success(), "curl failed: {resp:?}");
    let text = String::from_utf8_lossy(&resp.stdout);
    let mut parts = text.rsplitn(2, '\n');
    let code: u16 = parts.next().unwrap_or("0").parse().unwrap_or(0);
    let body = parts.next().unwrap_or("").to_string();
    (code, body)
}

fn stop(mut child: Child) {
    let ok = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .expect("kill -TERM")
        .success();
    assert!(ok, "kill -TERM api");
    let status = child.wait().expect("wait api");
    assert!(status.success(), "api exit status {status}");
}

fn one_launch() {
    let port = free_port();
    let child = spawn_ready(port);
    let (code, body) = get_health(port);
    assert_eq!(code, 200, "health status, body={body}");
    assert!(body.contains("\"status\":\"ok\""), "health body={body}");
    assert!(body.contains("\"service\":\"api\""), "health body={body}");
    stop(child);
}

#[test]
fn api_starts_serves_health_and_exits_twice() {
    one_launch();
    one_launch();
}
