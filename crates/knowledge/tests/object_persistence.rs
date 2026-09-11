//! Run each storage configuration in its own process; do not mutate the
//! environment underneath parallel Rust tests or consult a developer's .env.
use std::{fs, process::Command};

#[test]
fn object_identity_requires_successful_persistence() {
    const CASE: &str = "KB_OBJECT_PERSISTENCE_TEST_CASE";
    if let Ok(case) = std::env::var(CASE) {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let bytes = b"object identity must refer to persisted bytes";
        let result = runtime.block_on(knowledge::put_bytes(bytes));
        if case == "success" {
            let (hash, reference) = result.unwrap();
            assert_eq!(reference, platform::object_ref(&hash));
            assert_eq!(platform::read_blob(&hash).unwrap(), bytes);
        } else {
            assert!(result.is_err(), "{case} must not return a stored identity");
        }
        return;
    }
    let root = std::env::temp_dir().join(format!("kb-persist-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    fs::write(root.join("file"), b"not a directory").unwrap();
    for case in [
        "success",
        "missing_root",
        "missing_namespace",
        "invalid_root",
    ] {
        let mut child = Command::new(std::env::current_exe().unwrap());
        child
            .args([
                "--exact",
                "object_identity_requires_successful_persistence",
                "--nocapture",
            ])
            .env(CASE, case)
            .env("OBJECT_DIR", root.join("objects"))
            .env(
                "KB_DEPLOYMENT_NAMESPACE_ID",
                uuid::Uuid::new_v4().to_string(),
            )
            .env("KNOWLEDGEBRAIN_S3_BUCKET", "")
            .env("KNOWLEDGEBRAIN_S3_ENDPOINT", "");
        match case {
            "missing_root" => {
                child.env_remove("OBJECT_DIR");
            }
            "missing_namespace" => {
                child.env_remove("KB_DEPLOYMENT_NAMESPACE_ID");
            }
            "invalid_root" => {
                child.env("OBJECT_DIR", root.join("file"));
            }
            _ => {}
        }
        let result = child.output().unwrap();
        assert!(result.status.success(), "{case}: {result:?}");
    }
    fs::remove_dir_all(root).unwrap();
}
