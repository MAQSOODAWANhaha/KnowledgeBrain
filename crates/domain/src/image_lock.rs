//! Checked-in image lock reader.
//!
//! linux/amd64 may pin inspectable runtime digests. The lock is not production-
//! complete until every runtime and build-base entry is signed, including api,
//! worker, and retention. An incomplete lock cannot flip runtime completion.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

const LOCK_RELATIVE: &str = "deploy/images.lock.json";
const REQUIRED_PLATFORM: &str = "linux/amd64";

#[derive(Debug, Error)]
pub enum ImageLockError {
    #[error("image lock not found at {LOCK_RELATIVE}")]
    NotFound,
    #[error("image lock unreadable: {0}")]
    Io(String),
    #[error("image lock parse failed: {0}")]
    Parse(String),
    #[error("image lock invalid: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageLock {
    pub schema_version: u32,
    pub platforms: BTreeMap<String, ImageLockPlatform>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageLockPlatform {
    pub runtime_deployable: Vec<ImageLockEntry>,
    pub build_base: Vec<ImageLockEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageLockEntry {
    pub lock_id: String,
    pub compose_service: String,
    pub deploy_target: String,
    /// `repository@sha256:<64 hex>` or empty for an unsigned stub.
    pub image: String,
    #[serde(default)]
    pub unsigned: bool,
}

impl ImageLock {
    pub fn parse(source: &str) -> Result<Self, ImageLockError> {
        let lock: Self = serde_json::from_str(source)
            .map_err(|error| ImageLockError::Parse(error.to_string()))?;
        lock.validate()?;
        Ok(lock)
    }

    pub fn load() -> Result<Self, ImageLockError> {
        let path = locate_lock_path()?;
        Self::load_from_path(path)
    }

    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self, ImageLockError> {
        let path = path.as_ref();
        let source = std::fs::read_to_string(path)
            .map_err(|error| ImageLockError::Io(format!("{}: {error}", path.display())))?;
        Self::parse(&source)
    }

    pub fn is_production_complete(&self) -> bool {
        if self.platforms.is_empty() {
            return false;
        }
        self.platforms.values().all(|platform| {
            !platform.runtime_deployable.is_empty()
                && !platform.build_base.is_empty()
                && platform
                    .runtime_deployable
                    .iter()
                    .chain(platform.build_base.iter())
                    .all(ImageLockEntry::is_signed)
                && has_signed_lock_id(&platform.runtime_deployable, "api")
                && has_signed_lock_id(&platform.runtime_deployable, "worker")
                && has_signed_lock_id(&platform.runtime_deployable, "retention")
        })
    }

    /// Production completion cannot be flipped while the lock is empty or unsigned.
    pub fn allow_runtime_complete(
        &self,
        phase_1d_runtime_complete: bool,
    ) -> Result<(), ImageLockError> {
        if phase_1d_runtime_complete && !self.is_production_complete() {
            return Err(invalid(
                "phase_1d_runtime_complete cannot be true while the image lock is empty or unsigned",
            ));
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), ImageLockError> {
        if self.schema_version != 1 {
            return Err(invalid("schema_version must be 1"));
        }
        if !self.platforms.contains_key(REQUIRED_PLATFORM) {
            return Err(invalid("platforms must include linux/amd64"));
        }
        for (platform, sets) in &self.platforms {
            if platform.trim().is_empty() {
                return Err(invalid("platform keys must be non-empty"));
            }
            for entry in sets.runtime_deployable.iter().chain(sets.build_base.iter()) {
                entry.validate()?;
            }
        }
        Ok(())
    }
}

impl ImageLockEntry {
    pub fn is_signed(&self) -> bool {
        !self.unsigned && is_signed_digest(&self.image)
    }

    fn validate(&self) -> Result<(), ImageLockError> {
        if self.lock_id.trim().is_empty()
            || self.compose_service.trim().is_empty()
            || self.deploy_target.trim().is_empty()
        {
            return Err(invalid(
                "lock_id, compose_service, and deploy_target must be non-empty",
            ));
        }
        if self.image.is_empty() {
            return Ok(());
        }
        if self.unsigned {
            return Err(invalid(
                "unsigned entries must leave image empty; tag or digest pins are forbidden",
            ));
        }
        if is_tag_only(&self.image) {
            return Err(invalid(
                "tag-only image references are forbidden; use repository@sha256:<hex>",
            ));
        }
        if !is_signed_digest(&self.image) {
            return Err(invalid(
                "image must be repository@sha256:<64 hex> or empty for an unsigned stub",
            ));
        }
        Ok(())
    }
}

fn has_signed_lock_id(entries: &[ImageLockEntry], lock_id: &str) -> bool {
    entries
        .iter()
        .any(|entry| entry.lock_id == lock_id && entry.is_signed())
}

fn is_tag_only(image: &str) -> bool {
    image.contains(':') && !image.contains("@sha256:")
}

fn is_signed_digest(image: &str) -> bool {
    let Some((repository, digest)) = image.split_once("@sha256:") else {
        return false;
    };
    !repository.is_empty()
        && !repository.contains([':', '@'])
        && digest.len() == 64
        && digest.chars().all(|ch| ch.is_ascii_hexdigit())
}

fn invalid(message: &str) -> ImageLockError {
    ImageLockError::Invalid(message.to_string())
}

fn locate_lock_path() -> Result<PathBuf, ImageLockError> {
    let mut candidates = vec![
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../")
            .join(LOCK_RELATIVE),
    ];
    if let Ok(cwd) = std::env::current_dir() {
        let mut dir = cwd;
        loop {
            candidates.push(dir.join(LOCK_RELATIVE));
            if !dir.pop() {
                break;
            }
        }
    }
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or(ImageLockError::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checked_in() -> ImageLock {
        ImageLock::load().expect("repo-relative deploy/images.lock.json")
    }

    fn platform_json(runtime: &str, build: &str) -> String {
        format!(
            r#"{{
  "schema_version": 1,
  "platforms": {{
    "linux/amd64": {{
      "runtime_deployable": [{runtime}],
      "build_base": [{build}]
    }}
  }}
}}"#
        )
    }

    #[test]
    fn checked_in_lock_parses_and_is_production_complete() {
        let lock = checked_in();
        assert_eq!(lock.schema_version, 1);
        let amd64 = lock
            .platforms
            .get("linux/amd64")
            .expect("linux/amd64 platform");
        let postgres = amd64
            .runtime_deployable
            .iter()
            .find(|entry| entry.lock_id == "postgres")
            .expect("postgres runtime pin");
        assert_eq!(postgres.compose_service, "postgres");
        assert_eq!(postgres.deploy_target, "runtime");
        assert!(!postgres.unsigned);
        assert_eq!(
            postgres.image,
            "pgvector/pgvector@sha256:ccc6e83d6e35e931dc7c5def2022729d5a6c370318d099181995567ff1fb4d6b"
        );
        assert!(postgres.is_signed());
        let redis = amd64
            .runtime_deployable
            .iter()
            .find(|entry| entry.lock_id == "redis")
            .expect("redis runtime pin");
        assert_eq!(redis.compose_service, "redis");
        assert_eq!(redis.deploy_target, "runtime");
        assert!(!redis.unsigned);
        assert_eq!(
            redis.image,
            "redis@sha256:ff02b58f971e7d7d156a1267e283fcbbeee91773b6aa36c49dac28ecfe28eadf"
        );
        assert!(redis.is_signed());
        let neo4j = amd64
            .runtime_deployable
            .iter()
            .find(|entry| entry.lock_id == "neo4j")
            .expect("neo4j runtime pin");
        assert_eq!(neo4j.compose_service, "neo4j");
        assert_eq!(neo4j.deploy_target, "runtime");
        assert!(!neo4j.unsigned);
        assert_eq!(
            neo4j.image,
            "neo4j@sha256:89d577f2e49606de76441eca8cf7a0fe88e594cbaac4d2a3d86c6e59676e2b1e"
        );
        assert!(neo4j.is_signed());
        let minio = amd64
            .runtime_deployable
            .iter()
            .find(|entry| entry.lock_id == "minio")
            .expect("minio runtime pin");
        assert_eq!(minio.compose_service, "minio");
        assert_eq!(minio.deploy_target, "runtime");
        assert!(!minio.unsigned);
        assert_eq!(
            minio.image,
            "minio/minio@sha256:14cea493d9a34af32f524e538b8346cf79f3321eff8e708c1e2960462bd8936e"
        );
        assert!(minio.is_signed());
        assert_eq!(amd64.runtime_deployable.len(), 8);
        let api = amd64
            .runtime_deployable
            .iter()
            .find(|entry| entry.lock_id == "api")
            .expect("api runtime pin");
        assert_eq!(api.compose_service, "api");
        assert_eq!(api.deploy_target, "runtime");
        assert!(!api.unsigned);
        assert_eq!(
            api.image,
            "knowledgebrain-api@sha256:30a9535e1fa6a139ebe67d7612d144eff75eea3558d621ae003385c73879dd9b"
        );
        assert!(api.is_signed());
        let worker = amd64
            .runtime_deployable
            .iter()
            .find(|entry| entry.lock_id == "worker")
            .expect("worker runtime pin");
        assert_eq!(worker.compose_service, "worker");
        assert_eq!(worker.deploy_target, "runtime");
        assert!(!worker.unsigned);
        assert_eq!(
            worker.image,
            "knowledgebrain-worker@sha256:5d2715634560254f7c975b5c2a35bc73b72a09ec7926696d91d0dbaa9be6d256"
        );
        assert!(worker.is_signed());
        let retention = amd64
            .runtime_deployable
            .iter()
            .find(|entry| entry.lock_id == "retention")
            .expect("retention runtime pin");
        assert_eq!(retention.compose_service, "retention");
        assert_eq!(retention.deploy_target, "runtime");
        assert!(!retention.unsigned);
        assert_eq!(
            retention.image,
            "knowledgebrain-worker@sha256:5d2715634560254f7c975b5c2a35bc73b72a09ec7926696d91d0dbaa9be6d256"
        );
        assert!(retention.is_signed());
        let docreader = amd64
            .runtime_deployable
            .iter()
            .find(|entry| entry.lock_id == "docreader")
            .expect("docreader runtime pin");
        assert_eq!(docreader.compose_service, "docreader");
        assert_eq!(docreader.deploy_target, "runtime");
        assert!(!docreader.unsigned);
        assert_eq!(
            docreader.image,
            "knowledgebrain-docreader@sha256:72a50677badec74ed84c173c6098e139c0ab9b58281f20212e60610bbe0dbb97"
        );
        assert!(docreader.is_signed());
        assert!(has_signed_lock_id(&amd64.runtime_deployable, "api"));
        assert!(has_signed_lock_id(&amd64.runtime_deployable, "worker"));
        assert!(has_signed_lock_id(&amd64.runtime_deployable, "retention"));
        assert_eq!(amd64.build_base.len(), 4);
        for (lock_id, image) in [
            (
                "rust-bookworm",
                "rust@sha256:0e2bcaef56d041a486784e54104a81aebe0da44bd03019bd70bc0401e42e4a97",
            ),
            (
                "node-web",
                "node@sha256:d649c27dae7ba0137b3cef5dd75baa422c08dc3d9e3fc0c23dfb172dc3cc6436",
            ),
            (
                "debian-runtime",
                "debian@sha256:abd67ffcfa541b485a3dff59865ab629aa048a6c613e639d36e7456b0b229241",
            ),
            (
                "python-docreader",
                "python@sha256:c17b9ca6def1a936e55c77f086aaaf5f6a6c5f4d2fb4b7ac8c0113ba94e8b6ab",
            ),
        ] {
            let entry = amd64
                .build_base
                .iter()
                .find(|entry| entry.lock_id == lock_id)
                .unwrap_or_else(|| panic!("{lock_id} build pin"));
            assert_eq!(entry.compose_service, "build");
            assert_eq!(entry.deploy_target, "build");
            assert!(!entry.unsigned);
            assert_eq!(entry.image, image);
            assert!(entry.is_signed());
        }
        assert!(lock.is_production_complete());
        assert!(lock.allow_runtime_complete(false).is_ok());
        assert!(lock.allow_runtime_complete(true).is_ok());
    }

    #[test]
    fn empty_or_unsigned_lock_cannot_flip_runtime_complete() {
        let empty = ImageLock::parse(&platform_json("", "")).expect("empty lock parses");
        let error = empty
            .allow_runtime_complete(true)
            .expect_err("empty lock must not flip runtime completion");
        assert!(error.to_string().contains("empty or unsigned"));

        let unsigned = ImageLock::parse(&platform_json(
            r#"{
              "lock_id": "minio-runtime",
              "compose_service": "minio",
              "deploy_target": "runtime",
              "image": "",
              "unsigned": true
            }"#,
            "",
        ))
        .expect("unsigned stub parses");
        assert!(!unsigned.is_production_complete());
        assert!(unsigned.allow_runtime_complete(true).is_err());
    }

    #[test]
    fn deny_unknown_fields() {
        let source = r#"{
          "schema_version": 1,
          "unexpected": true,
          "platforms": {
            "linux/amd64": {
              "runtime_deployable": [],
              "build_base": []
            }
          }
        }"#;
        assert!(ImageLock::parse(source).is_err());

        let source = r#"{
          "schema_version": 1,
          "platforms": {
            "linux/amd64": {
              "runtime_deployable": [],
              "build_base": [],
              "extra": []
            }
          }
        }"#;
        assert!(ImageLock::parse(source).is_err());
    }

    #[test]
    fn tag_only_image_fails() {
        for image in [
            "minio/minio:latest",
            "redis:7-alpine",
            "knowledgebrain-api:local",
            "pgvector/pgvector:0.8.6-pg16",
        ] {
            let source = platform_json(
                &format!(
                    r#"{{
                      "lock_id": "tag-only",
                      "compose_service": "minio",
                      "deploy_target": "runtime",
                      "image": "{image}"
                    }}"#
                ),
                "",
            );
            let error = ImageLock::parse(&source).expect_err(image);
            assert!(
                error.to_string().contains("tag-only")
                    || error.to_string().contains("repository@sha256"),
                "{image}: {error}"
            );
        }
    }

    #[test]
    fn signed_digest_is_accepted_but_not_invented_in_tree() {
        let digest = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let source = platform_json(
            &format!(
                r#"{{
                  "lock_id": "api",
                  "compose_service": "api",
                  "deploy_target": "runtime",
                  "image": "example.com/knowledgebrain-api@sha256:{digest}"
                }}, {{
                  "lock_id": "worker",
                  "compose_service": "worker",
                  "deploy_target": "runtime",
                  "image": "example.com/knowledgebrain-worker@sha256:{digest}"
                }}, {{
                  "lock_id": "retention",
                  "compose_service": "retention",
                  "deploy_target": "runtime",
                  "image": "example.com/knowledgebrain-worker@sha256:{digest}"
                }}"#
            ),
            &format!(
                r#"{{
                  "lock_id": "rust-bookworm",
                  "compose_service": "build",
                  "deploy_target": "build",
                  "image": "example.com/rust-base@sha256:{digest}"
                }}"#
            ),
        );
        let lock = ImageLock::parse(&source).expect("synthetic signed lock");
        assert!(lock.is_production_complete());
        assert!(lock.allow_runtime_complete(true).is_ok());

        let missing_worker = ImageLock::parse(&platform_json(
            &format!(
                r#"{{
                  "lock_id": "api",
                  "compose_service": "api",
                  "deploy_target": "runtime",
                  "image": "example.com/knowledgebrain-api@sha256:{digest}"
                }}"#
            ),
            &format!(
                r#"{{
                  "lock_id": "rust-bookworm",
                  "compose_service": "build",
                  "deploy_target": "build",
                  "image": "example.com/rust-base@sha256:{digest}"
                }}"#
            ),
        ))
        .expect("signed lock without worker");
        assert!(!missing_worker.is_production_complete());

        let checked = checked_in();
        const KNOWN_PINS: &[&str] = &[
            "pgvector/pgvector@sha256:ccc6e83d6e35e931dc7c5def2022729d5a6c370318d099181995567ff1fb4d6b",
            "redis@sha256:ff02b58f971e7d7d156a1267e283fcbbeee91773b6aa36c49dac28ecfe28eadf",
            "neo4j@sha256:89d577f2e49606de76441eca8cf7a0fe88e594cbaac4d2a3d86c6e59676e2b1e",
            "minio/minio@sha256:14cea493d9a34af32f524e538b8346cf79f3321eff8e708c1e2960462bd8936e",
            "knowledgebrain-api@sha256:30a9535e1fa6a139ebe67d7612d144eff75eea3558d621ae003385c73879dd9b",
            "knowledgebrain-worker@sha256:5d2715634560254f7c975b5c2a35bc73b72a09ec7926696d91d0dbaa9be6d256",
            "knowledgebrain-docreader@sha256:72a50677badec74ed84c173c6098e139c0ab9b58281f20212e60610bbe0dbb97",
            "rust@sha256:0e2bcaef56d041a486784e54104a81aebe0da44bd03019bd70bc0401e42e4a97",
            "node@sha256:d649c27dae7ba0137b3cef5dd75baa422c08dc3d9e3fc0c23dfb172dc3cc6436",
            "debian@sha256:abd67ffcfa541b485a3dff59865ab629aa048a6c613e639d36e7456b0b229241",
            "python@sha256:c17b9ca6def1a936e55c77f086aaaf5f6a6c5f4d2fb4b7ac8c0113ba94e8b6ab",
        ];
        assert!(
            checked
                .platforms
                .values()
                .flat_map(|platform| platform
                    .runtime_deployable
                    .iter()
                    .chain(platform.build_base.iter()))
                .filter(|entry| entry.is_signed())
                .all(|entry| KNOWN_PINS.contains(&entry.image.as_str())),
            "checked-in lock must not invent production digests"
        );
        assert!(checked.is_production_complete());
    }

    #[test]
    fn production_complete_requires_signed_retention_runtime() {
        let digest = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let without_retention = ImageLock::parse(&platform_json(
            &format!(
                r#"{{
                  "lock_id": "api",
                  "compose_service": "api",
                  "deploy_target": "runtime",
                  "image": "example.com/knowledgebrain-api@sha256:{digest}"
                }}, {{
                  "lock_id": "worker",
                  "compose_service": "worker",
                  "deploy_target": "runtime",
                  "image": "example.com/knowledgebrain-worker@sha256:{digest}"
                }}"#
            ),
            &format!(
                r#"{{
                  "lock_id": "rust-bookworm",
                  "compose_service": "build",
                  "deploy_target": "build",
                  "image": "example.com/rust-base@sha256:{digest}"
                }}"#
            ),
        ))
        .expect("signed lock without retention");

        assert!(!without_retention.is_production_complete());
    }

    #[test]
    fn checked_in_compose_uses_the_locked_retention_image() {
        let lock = checked_in();
        let retention = lock.platforms[REQUIRED_PLATFORM]
            .runtime_deployable
            .iter()
            .find(|entry| entry.lock_id == "retention")
            .expect("retention runtime pin");
        let compose_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../deploy/docker-compose.yml");
        let compose = std::fs::read_to_string(compose_path).expect("checked-in Compose file");
        let retention_block = compose
            .split_once("\n  retention:\n")
            .map(|(_, tail)| tail)
            .expect("retention service")
            .split_once("\nvolumes:\n")
            .map(|(block, _)| block)
            .expect("retention service boundary");
        let compose_image = retention_block
            .lines()
            .find_map(|line| line.trim().strip_prefix("image: "))
            .expect("retention image");

        assert_eq!(
            compose_image.replace(":local@sha256:", "@sha256:"),
            retention.image
        );
        assert!(retention_block.contains("args:\n        BIN: worker"));
        assert!(retention_block.contains("\n      BIN: retention\n"));
    }
}
