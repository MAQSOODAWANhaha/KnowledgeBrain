use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::str::FromStr;
use thiserror::Error;

pub const RELEASE_DESCRIPTOR_SCHEMA: &str =
    include_str!("../../../deploy/release-descriptor-v1.schema.json");
pub const RELEASE_DESCRIPTOR_HASH_DOMAIN: &[u8] = b"KB:ReleaseDescriptor:v1\0";
/// RFC 8785 canonical bytes shared by release and content hashing.
pub fn jcs_canonical_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, serde_json::Error> {
    serde_json_canonicalizer::to_vec(value)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseDescriptorV1 {
    pub schema_version: u32,
    pub release_revision: String,
    pub git_sha: String,
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
pub enum ReleaseComponentKind {
    Migrator,
    Api,
    Worker,
    Retention,
}

impl ReleaseComponentKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Migrator => "migrator",
            Self::Api => "api",
            Self::Worker => "worker",
            Self::Retention => "retention",
        }
    }
}

impl FromStr for ReleaseComponentKind {
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

impl ReleaseDescriptorV1 {
    pub fn validate(&self) -> Result<(), ReleaseIdentityError> {
        if self.schema_version != 1 {
            return Err(ReleaseIdentityError::DescriptorVersionMismatch);
        }
        require_revision(&self.release_revision)?;
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

    pub fn component_image(&self, component: ReleaseComponentKind) -> &str {
        match component {
            ReleaseComponentKind::Migrator => &self.images.migrator,
            ReleaseComponentKind::Api => &self.images.api,
            ReleaseComponentKind::Worker => &self.images.worker,
            ReleaseComponentKind::Retention => &self.images.retention,
        }
    }

    pub fn component_digest(
        &self,
        component: ReleaseComponentKind,
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
    #[error("RELEASE_IDENTITY_INVALID: component kind is invalid")]
    InvalidComponentKind,
    #[error("RELEASE_IDENTITY_INVALID: component digest is invalid")]
    ComponentDigestInvalid,
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
        bad_revision["release_revision"] = json!("bad revision");
        let mut bad_git = value.clone();
        bad_git["git_sha"] = json!("A".repeat(40));
        for invalid in [unknown_root, unknown_image, tagged, bad_revision, bad_git] {
            assert!(!validator.is_valid(&invalid), "schema accepted {invalid}");
        }
        assert!(
            serde_json::from_value::<ReleaseDescriptorV1>(json!({
                "schema_version":1,"release_revision":"x","git_sha":"0".repeat(40),
                "images":value["images"],"extra":true
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
        assert_eq!(descriptor.sha256().unwrap(), hex::encode(replay.finalize()));
        let mut changed = descriptor.clone();
        changed.release_revision.push('x');
        assert_ne!(changed.sha256().unwrap(), descriptor.sha256().unwrap());
        assert_eq!(
            descriptor
                .component_digest(ReleaseComponentKind::Api)
                .unwrap(),
            format!("sha256:{}", "2".repeat(64))
        );
    }
    #[test]
    fn utf16_key_order_is_rfc8785_order_not_unicode_scalar_order() {
        let value = json!({"\u{e000}": 1, "\u{10000}": 2, "a": 3});
        assert_eq!(
            String::from_utf8(jcs_canonical_bytes(&value).unwrap()).unwrap(),
            "{\"a\":3,\"𐀀\":2,\"\":1}"
        );
        assert_eq!(
            String::from_utf8(
                jcs_canonical_bytes(&json!({"fraction": 1.5, "large": 1e30, "small": 1e-7}))
                    .unwrap()
            )
            .unwrap(),
            "{\"fraction\":1.5,\"large\":1e+30,\"small\":1e-7}"
        );
    }
}
