use crate::jcs_canonical_bytes;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    str::FromStr,
};
use thiserror::Error;
use uuid::Uuid;

pub const RELEASE_DESCRIPTOR_SCHEMA: &str =
    include_str!("../../../deploy/release-descriptor-v1.schema.json");
pub const RELEASE_DESCRIPTOR_HASH_DOMAIN: &[u8] = b"KB:ReleaseDescriptor:v1\0";
pub const RELEASE_DESCRIPTOR_PATH_ENV: &str = "KB_RELEASE_DESCRIPTOR_PATH";
pub const RELEASE_DESCRIPTOR_SHA256_ENV: &str = "KB_RELEASE_DESCRIPTOR_SHA256";
pub const COMPONENT_KIND_ENV: &str = "KB_COMPONENT_KIND";
pub const COMPONENT_IMAGE_DIGEST_ENV: &str = "KB_COMPONENT_IMAGE_DIGEST";
pub const DEPLOYMENT_NAMESPACE_ID_ENV: &str = "KB_DEPLOYMENT_NAMESPACE_ID";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseDescriptorV1 {
    pub schema_version: u32,
    pub release_revision: String,
    pub git_sha: String,
    pub platform_schema_revision: String,
    pub images: ReleaseImagesV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseImagesV1 {
    pub migrator: String,
    pub api: String,
    pub worker: String,
    pub retention: String,
    pub docreader: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SchemaComponentKind {
    Migrator,
    Api,
    Worker,
    Retention,
}

impl SchemaComponentKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Migrator => "migrator",
            Self::Api => "api",
            Self::Worker => "worker",
            Self::Retention => "retention",
        }
    }
}

impl FromStr for SchemaComponentKind {
    type Err = ReleaseIdentityError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "migrator" => Ok(Self::Migrator),
            "api" => Ok(Self::Api),
            "worker" => Ok(Self::Worker),
            "retention" => Ok(Self::Retention),
            _ => Err(ReleaseIdentityError::InvalidComponentKind),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaRuntimeIdentity {
    pub descriptor_path: PathBuf,
    pub descriptor: ReleaseDescriptorV1,
    pub release_descriptor_sha256: String,
    pub component_kind: SchemaComponentKind,
    pub component_image_digest: String,
    pub deployment_namespace_id: Uuid,
}

impl SchemaRuntimeIdentity {
    pub fn load_from_env() -> Result<Self, ReleaseIdentityError> {
        let descriptor_path = required_env(RELEASE_DESCRIPTOR_PATH_ENV)?;
        let descriptor_sha256 = required_env(RELEASE_DESCRIPTOR_SHA256_ENV)?;
        let component_kind = required_env(COMPONENT_KIND_ENV)?;
        let component_image_digest = required_env(COMPONENT_IMAGE_DIGEST_ENV)?;
        let deployment_namespace_id = required_env(DEPLOYMENT_NAMESPACE_ID_ENV)?;
        Self::load_verified(
            Path::new(&descriptor_path),
            &descriptor_sha256,
            &component_kind,
            &component_image_digest,
            &deployment_namespace_id,
        )
    }

    pub fn load_verified(
        descriptor_path: &Path,
        expected_descriptor_sha256: &str,
        component_kind: &str,
        expected_component_digest: &str,
        deployment_namespace_id: &str,
    ) -> Result<Self, ReleaseIdentityError> {
        let bytes = std::fs::read(descriptor_path)
            .map_err(|_| ReleaseIdentityError::DescriptorUnreadable)?;
        let descriptor: ReleaseDescriptorV1 =
            serde_json::from_slice(&bytes).map_err(|_| ReleaseIdentityError::DescriptorInvalid)?;
        Self::from_descriptor(
            descriptor_path.to_path_buf(),
            descriptor,
            expected_descriptor_sha256,
            component_kind,
            expected_component_digest,
            deployment_namespace_id,
        )
    }

    pub fn from_descriptor(
        descriptor_path: PathBuf,
        descriptor: ReleaseDescriptorV1,
        expected_descriptor_sha256: &str,
        component_kind: &str,
        expected_component_digest: &str,
        deployment_namespace_id: &str,
    ) -> Result<Self, ReleaseIdentityError> {
        descriptor.validate()?;
        require_lower_hex(expected_descriptor_sha256, 64)
            .map_err(|_| ReleaseIdentityError::DescriptorHashInvalid)?;
        let calculated = descriptor.sha256()?;
        if calculated != expected_descriptor_sha256 {
            return Err(ReleaseIdentityError::DescriptorHashMismatch);
        }
        let component_kind = SchemaComponentKind::from_str(component_kind)?;
        require_digest_suffix(expected_component_digest)?;
        let selected_digest = descriptor.component_digest(component_kind)?.to_owned();
        if selected_digest != expected_component_digest {
            return Err(ReleaseIdentityError::ComponentDigestMismatch);
        }
        let deployment_namespace_id = deployment_namespace_id
            .parse::<crate::DeploymentNamespaceV1>()
            .map_err(|_| ReleaseIdentityError::DeploymentNamespaceInvalid)?
            .id();
        Ok(Self {
            descriptor_path,
            descriptor,
            release_descriptor_sha256: calculated,
            component_kind,
            component_image_digest: selected_digest,
            deployment_namespace_id,
        })
    }

    pub fn schema_revision(&self) -> &str {
        &self.descriptor.platform_schema_revision
    }
}

impl ReleaseDescriptorV1 {
    pub fn validate(&self) -> Result<(), ReleaseIdentityError> {
        if self.schema_version != 1 {
            return Err(ReleaseIdentityError::DescriptorVersionMismatch);
        }
        require_revision(&self.release_revision)?;
        require_revision(&self.platform_schema_revision)?;
        require_lower_hex(&self.git_sha, 40).map_err(|_| ReleaseIdentityError::GitShaInvalid)?;
        for image in [
            &self.images.migrator,
            &self.images.api,
            &self.images.worker,
            &self.images.retention,
            &self.images.docreader,
        ] {
            validate_digest_only_image(image)?;
        }
        Ok(())
    }

    pub fn sha256(&self) -> Result<String, ReleaseIdentityError> {
        self.validate()?;
        let canonical =
            jcs_canonical_bytes(self).map_err(|_| ReleaseIdentityError::DescriptorInvalid)?;
        let mut digest = Sha256::new();
        digest.update(RELEASE_DESCRIPTOR_HASH_DOMAIN);
        digest.update(canonical);
        Ok(hex::encode(digest.finalize()))
    }

    pub fn component_image(&self, component: SchemaComponentKind) -> &str {
        match component {
            SchemaComponentKind::Migrator => &self.images.migrator,
            SchemaComponentKind::Api => &self.images.api,
            SchemaComponentKind::Worker => &self.images.worker,
            SchemaComponentKind::Retention => &self.images.retention,
        }
    }

    pub fn component_digest(
        &self,
        component: SchemaComponentKind,
    ) -> Result<&str, ReleaseIdentityError> {
        let image = self.component_image(component);
        validate_digest_only_image(image)?;
        image
            .split_once('@')
            .map(|(_, digest)| digest)
            .ok_or(ReleaseIdentityError::ImageReferenceInvalid)
    }
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum ReleaseIdentityError {
    #[error("RELEASE_IDENTITY_INVALID: required environment is missing")]
    MissingEnvironment,
    #[error("RELEASE_IDENTITY_INVALID: release descriptor is unreadable")]
    DescriptorUnreadable,
    #[error("RELEASE_IDENTITY_INVALID: release descriptor is invalid")]
    DescriptorInvalid,
    #[error("RELEASE_IDENTITY_INVALID: descriptor schema version mismatch")]
    DescriptorVersionMismatch,
    #[error("RELEASE_IDENTITY_INVALID: release revision is invalid")]
    RevisionInvalid,
    #[error("RELEASE_IDENTITY_INVALID: git SHA is invalid")]
    GitShaInvalid,
    #[error("RELEASE_IDENTITY_INVALID: image reference is not digest-only")]
    ImageReferenceInvalid,
    #[error("RELEASE_IDENTITY_INVALID: descriptor hash is invalid")]
    DescriptorHashInvalid,
    #[error("RELEASE_IDENTITY_INVALID: descriptor hash mismatch")]
    DescriptorHashMismatch,
    #[error("RELEASE_IDENTITY_INVALID: component kind is invalid")]
    InvalidComponentKind,
    #[error("RELEASE_IDENTITY_INVALID: component digest is invalid")]
    ComponentDigestInvalid,
    #[error("RELEASE_IDENTITY_INVALID: component digest mismatch")]
    ComponentDigestMismatch,
    #[error("RELEASE_IDENTITY_INVALID: deployment namespace is invalid")]
    DeploymentNamespaceInvalid,
}

fn required_env(name: &str) -> Result<String, ReleaseIdentityError> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or(ReleaseIdentityError::MissingEnvironment)
}

pub(crate) fn require_revision(value: &str) -> Result<(), ReleaseIdentityError> {
    if value.is_empty()
        || value.len() > 128
        || !value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphanumeric() || (index > 0 && matches!(byte, b'.' | b'_' | b'-'))
        })
    {
        return Err(ReleaseIdentityError::RevisionInvalid);
    }
    Ok(())
}

fn require_lower_hex(value: &str, length: usize) -> Result<(), ()> {
    if value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(())
    }
}

fn require_digest_suffix(value: &str) -> Result<(), ReleaseIdentityError> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(ReleaseIdentityError::ComponentDigestInvalid);
    };
    require_lower_hex(hex, 64).map_err(|_| ReleaseIdentityError::ComponentDigestInvalid)
}

fn validate_digest_only_image(value: &str) -> Result<(), ReleaseIdentityError> {
    if value.bytes().any(|byte| byte.is_ascii_whitespace()) || value.matches('@').count() != 1 {
        return Err(ReleaseIdentityError::ImageReferenceInvalid);
    }
    let (repository, digest) = value
        .split_once('@')
        .ok_or(ReleaseIdentityError::ImageReferenceInvalid)?;
    let mut segments = repository.split('/');
    let registry = segments.next().unwrap_or_default();
    let path: Vec<_> = segments.collect();
    if registry.is_empty()
        || path.is_empty()
        || path
            .iter()
            .any(|segment| segment.is_empty() || segment.contains(':'))
        || !repository.bytes().all(|byte| {
            byte.is_ascii_digit()
                || byte.is_ascii_lowercase()
                || matches!(byte, b'.' | b'_' | b'-' | b'/' | b':')
        })
    {
        return Err(ReleaseIdentityError::ImageReferenceInvalid);
    }
    if let Some((host, port)) = registry.rsplit_once(':')
        && (host.is_empty()
            || host.contains(':')
            || port.is_empty()
            || !port.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(ReleaseIdentityError::ImageReferenceInvalid);
    }
    require_digest_suffix(digest).map_err(|_| ReleaseIdentityError::ImageReferenceInvalid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn descriptor() -> ReleaseDescriptorV1 {
        ReleaseDescriptorV1 {
            schema_version: 1,
            release_revision: "development-1".into(),
            git_sha: "0123456789abcdef0123456789abcdef01234567".into(),
            platform_schema_revision: "platform-v1".into(),
            images: ReleaseImagesV1 {
                migrator: format!("registry.example/kb/migrator@sha256:{}", "1".repeat(64)),
                api: format!("registry.example/kb/api@sha256:{}", "2".repeat(64)),
                worker: format!("registry.example/kb/worker@sha256:{}", "3".repeat(64)),
                retention: format!("registry.example/kb/retention@sha256:{}", "4".repeat(64)),
                docreader: format!("registry.example/kb/docreader@sha256:{}", "5".repeat(64)),
            },
        }
    }

    #[test]
    fn descriptor_schema_semantics_and_unknown_fields_are_closed() {
        let descriptor = descriptor();
        descriptor.validate().unwrap();
        let schema: serde_json::Value = serde_json::from_str(RELEASE_DESCRIPTOR_SCHEMA).unwrap();
        let validator = jsonschema::JSONSchema::options()
            .with_draft(jsonschema::Draft::Draft202012)
            .compile(&schema)
            .unwrap();
        let value = serde_json::to_value(&descriptor).unwrap();
        assert!(validator.is_valid(&value));
        let mut unknown_root = value.clone();
        unknown_root["extra"] = json!(true);
        let mut unknown_image = value.clone();
        unknown_image["images"]["extra"] = json!("x");
        let mut tagged = value.clone();
        tagged["images"]["api"] = json!("registry.example/kb/api:latest");
        let mut bad_revision = value.clone();
        bad_revision["platform_schema_revision"] = json!("bad revision");
        let mut bad_git = value.clone();
        bad_git["git_sha"] = json!("A".repeat(40));
        for invalid in [unknown_root, unknown_image, tagged, bad_revision, bad_git] {
            assert!(!validator.is_valid(&invalid), "schema accepted {invalid}");
        }
        assert!(
            serde_json::from_value::<ReleaseDescriptorV1>(json!({
                "schema_version":1,"release_revision":"x","git_sha":"0".repeat(40),
                "platform_schema_revision":"x","images":value["images"],"extra":true
            }))
            .is_err()
        );
    }

    #[test]
    fn descriptor_rejects_tags_uppercase_multiple_at_and_bad_revisions() {
        for invalid in [
            "registry.example/kb/api:latest",
            "registry.example/kb/api:tag@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "registry.example/kb/api@sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "Registry.example/kb/api@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "registry.example/kb/api@@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ] {
            let mut value = descriptor();
            value.images.api = invalid.into();
            assert!(value.validate().is_err(), "accepted {invalid}");
        }
        for invalid in ["", "has space", "slash/no", ".leading", &"x".repeat(129)] {
            let mut value = descriptor();
            value.release_revision = invalid.into();
            assert!(value.validate().is_err(), "accepted revision {invalid}");
        }
    }

    #[test]
    fn descriptor_hash_and_component_suffix_replay_are_exact() {
        let descriptor = descriptor();
        let canonical = jcs_canonical_bytes(&descriptor).unwrap();
        let mut replay = Sha256::new();
        replay.update(b"KB:ReleaseDescriptor:v1\0");
        replay.update(canonical);
        assert_eq!(
            descriptor.sha256().unwrap(),
            "0dd4a7f43c41ea62ee0c20dc3afc9077d10d323b9d740717e0a5ec08e4505a6d"
        );
        assert_eq!(descriptor.sha256().unwrap(), hex::encode(replay.finalize()));
        let mut changed = descriptor.clone();
        changed.release_revision.push('x');
        assert_ne!(changed.sha256().unwrap(), descriptor.sha256().unwrap());
        assert_eq!(
            descriptor
                .component_digest(SchemaComponentKind::Api)
                .unwrap(),
            format!("sha256:{}", "2".repeat(64))
        );
    }

    #[test]
    fn checked_in_development_descriptor_hash_is_frozen_but_not_runtime_proof() {
        let descriptor: ReleaseDescriptorV1 = serde_json::from_str(include_str!(
            "../../../deploy/release-descriptor-v1.development.json"
        ))
        .unwrap();
        assert_eq!(
            descriptor.sha256().unwrap(),
            "770ce81ba32a38ff76cdee4e26abe276e3645a07dd6a6bf9d14ebf2bee76d84c"
        );
    }

    #[test]
    fn executables_and_compose_use_the_shared_identity_gate() {
        let api = include_str!("../../api/src/main.rs");
        let worker = include_str!("../../worker/src/main.rs");
        let retention = include_str!("../../retention/src/main.rs");
        let migrator = include_str!("bin/migrator.rs");
        assert!(api.contains("connect_runtime_verified(platform::SchemaComponentKind::Api)"));
        assert!(worker.contains("connect_runtime_verified(platform::SchemaComponentKind::Worker)"));
        assert!(
            retention
                .contains("connect_runtime_verified(platform::SchemaComponentKind::Retention)")
        );
        assert!(migrator.contains("connect_unverified()"));
        assert!(migrator.contains("apply_fresh_baseline(&pool)"));

        let compose = include_str!("../../../deploy/docker-compose.yml");
        assert!(
            compose
                .matches("/run/knowledgebrain/release-descriptor.json:ro")
                .count()
                >= 4
        );
        for kind in ["migrator", "api", "worker", "retention"] {
            assert!(compose.contains(&format!("KB_COMPONENT_KIND: {kind}")));
        }
        assert!(!compose.contains("KB_COMPONENT_IMAGE_DIGEST: latest"));
    }

    #[test]
    fn runtime_identity_binds_descriptor_kind_digest_and_namespace() {
        let descriptor = descriptor();
        let hash = descriptor.sha256().unwrap();
        let digest = format!("sha256:{}", "2".repeat(64));
        let identity = SchemaRuntimeIdentity::from_descriptor(
            "/mounted/descriptor.json".into(),
            descriptor.clone(),
            &hash,
            "api",
            &digest,
            "123e4567-e89b-12d3-a456-426614174000",
        )
        .unwrap();
        assert_eq!(identity.schema_revision(), "platform-v1");
        for (wrong_hash, kind, wrong_digest, namespace) in [
            (
                "0".repeat(64),
                "api",
                digest.clone(),
                "123e4567-e89b-12d3-a456-426614174000".to_string(),
            ),
            (
                hash.clone(),
                "docreader",
                digest.clone(),
                "123e4567-e89b-12d3-a456-426614174000".to_string(),
            ),
            (
                hash.clone(),
                "api",
                format!("sha256:{}", "3".repeat(64)),
                "123e4567-e89b-12d3-a456-426614174000".to_string(),
            ),
            (
                hash.clone(),
                "api",
                digest.clone(),
                "123E4567-E89B-12D3-A456-426614174000".to_string(),
            ),
        ] {
            assert!(
                SchemaRuntimeIdentity::from_descriptor(
                    "/mounted/descriptor.json".into(),
                    descriptor.clone(),
                    &wrong_hash,
                    kind,
                    &wrong_digest,
                    &namespace,
                )
                .is_err()
            );
        }
    }
}
