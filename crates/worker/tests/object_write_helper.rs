use sha2::{Digest, Sha256};
use std::{path::PathBuf, process::Stdio};
use tokio::{io::AsyncWriteExt, process::Command};
use uuid::Uuid;

struct Objects(PathBuf, platform::DeploymentNamespaceV1);
impl Objects {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("kb-composition-helper-test-{}", Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        Self(path, Uuid::new_v4().to_string().parse().unwrap())
    }
    fn command(&self, sha: &str, length: usize) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_worker"));
        command
            .args(["--kb-object-write-helper-v1", sha, &length.to_string()])
            .env("OBJECT_DIR", &self.0)
            .env("KB_DEPLOYMENT_NAMESPACE_ID", self.1.to_string())
            .env("KNOWLEDGEBRAIN_S3_BUCKET", "")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        command
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0
            .join(self.1.storage_label())
            .join("objects")
            .join(name)
    }
}
impl Drop for Objects {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[tokio::test]
async fn object_helper_writes_exact_bytes_and_rejects_wrong_identity() {
    let objects = Objects::new();
    let bytes = b"composition object helper exact bytes";
    let sha = hex::encode(Sha256::digest(bytes));
    let mut child = objects.command(&sha, bytes.len()).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(bytes).await.unwrap();
    stdin.shutdown().await.unwrap();
    drop(stdin);
    let output = child.wait_with_output().await.unwrap();
    assert!(output.status.success(), "{:?}", output);
    assert_eq!(output.stdout, b"ok");
    assert_eq!(std::fs::read(objects.path(&sha)).unwrap(), bytes);
    for (digest, length) in [
        ("0".repeat(64), bytes.len()),
        (sha.clone(), bytes.len() + 1),
        (sha.clone(), bytes.len() - 1),
    ] {
        let mut child = objects.command(&digest, length).spawn().unwrap();
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(bytes).await.unwrap();
        drop(stdin);
        assert!(!child.wait_with_output().await.unwrap().status.success());
    }
    assert_eq!(
        std::fs::read(objects.path(&sha)).unwrap(),
        bytes,
        "rejected bytes cannot overwrite existing content"
    );
    assert!(!objects.path(&"0".repeat(64)).exists());
}

#[tokio::test]
async fn stopped_object_helper_cannot_write_after_reaping() {
    let objects = Objects::new();
    let bytes = b"incomplete helper input";
    let sha = hex::encode(Sha256::digest(bytes));
    let mut child = objects.command(&sha, bytes.len()).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(&bytes[..3]).await.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(child.try_wait().unwrap().is_none());
    child.kill().await.unwrap();
    assert!(!child.wait().await.unwrap().success());
    drop(stdin);
    assert!(!objects.path(&sha).exists());
}
