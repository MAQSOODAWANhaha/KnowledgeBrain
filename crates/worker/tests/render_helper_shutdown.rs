use std::os::unix::fs::OpenOptionsExt;
use std::time::Duration;
use tokio::process::Command;
use uuid::Uuid;

#[tokio::test]
async fn hung_render_helper_is_killed_reaped_and_cannot_publish_output() {
    let directory = std::env::temp_dir().join(format!("kb-render-helper-test-{}", Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let input = directory.join("input.json");
    let output = directory.join("output.bin");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true).mode(0o600);
    std::io::Write::write_all(&mut options.open(&input).unwrap(), b"{}").unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_worker"))
        .arg("--kb-submission-render-helper-v1")
        .arg(&input)
        .arg(&output)
        .arg("docx")
        .env("KNOWLEDGEBRAIN_TEST_RENDER_HELPER_DELAY_MS", "60000")
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        child.try_wait().unwrap().is_none(),
        "helper did not enter delay seam"
    );
    child.kill().await.unwrap();
    let status = child.wait().await.unwrap();
    assert!(!status.success());
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        !output.exists(),
        "killed helper wrote output after being reaped"
    );
    std::fs::remove_dir_all(directory).unwrap();
}
