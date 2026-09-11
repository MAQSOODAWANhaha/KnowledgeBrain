//! Canonical deployment identity shared by runtime and backend isolation.
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DeploymentNamespaceV1(Uuid);

#[derive(Debug, thiserror::Error)]
pub enum DeploymentNamespaceError {
    #[error("KB_DEPLOYMENT_NAMESPACE_ID is required")]
    Missing,
    #[error("deployment namespace must be a canonical lowercase UUID")]
    Invalid,
}

impl DeploymentNamespaceV1 {
    pub fn from_environment() -> Result<Self, DeploymentNamespaceError> {
        std::env::var(crate::DEPLOYMENT_NAMESPACE_ID_ENV)
            .map_err(|_| DeploymentNamespaceError::Missing)?
            .parse()
    }

    pub fn id(self) -> Uuid {
        self.0
    }

    pub fn storage_label(self) -> String {
        format!("kb-{}", self.0.simple())
    }

    pub fn postgres_database(self) -> String {
        format!("kb_{}", self.0.simple())
    }

    /// Parse the exact deployment label accepted by the reset CLI.
    pub fn from_storage_label(value: &str) -> Result<Self, DeploymentNamespaceError> {
        let id = value
            .strip_prefix("kb-")
            .ok_or(DeploymentNamespaceError::Invalid)?;
        let namespace = Self(Uuid::parse_str(id).map_err(|_| DeploymentNamespaceError::Invalid)?);
        if namespace.storage_label() != value {
            return Err(DeploymentNamespaceError::Invalid);
        }
        Ok(namespace)
    }

    /// Oxana appends its own separator to this configured namespace.
    pub fn redis_namespace(self) -> String {
        format!("kb:{}", self.0.simple())
    }

    /// Prefix including the separator, for application-owned Redis keys.
    pub fn redis_prefix(self) -> String {
        format!("{}:", self.redis_namespace())
    }
}

impl FromStr for DeploymentNamespaceV1 {
    type Err = DeploymentNamespaceError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let id = Uuid::parse_str(value).map_err(|_| DeploymentNamespaceError::Invalid)?;
        if id.to_string() != value {
            return Err(DeploymentNamespaceError::Invalid);
        }
        Ok(Self(id))
    }
}

impl TryFrom<String> for DeploymentNamespaceV1 {
    type Error = DeploymentNamespaceError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl fmt::Display for DeploymentNamespaceV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl From<DeploymentNamespaceV1> for String {
    fn from(value: DeploymentNamespaceV1) -> Self {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespace_serialization_and_parsing_reject_aliases() {
        let raw = "a4598c31-6184-4f75-9ee2-4a0a2783d803";
        let namespace: DeploymentNamespaceV1 = raw.parse().unwrap();
        assert_eq!(namespace.id().to_string(), raw);
        assert_eq!(serde_json::to_value(namespace).unwrap(), raw);
        assert_eq!(
            serde_json::from_value::<DeploymentNamespaceV1>(serde_json::json!(raw)).unwrap(),
            namespace
        );
        for invalid in [
            String::new(),
            raw.to_uppercase(),
            raw.replace('-', ""),
            format!(" {raw}"),
            format!("{raw}\n"),
            format!("kb-{}", raw.replace('-', "")),
            format!("urn:uuid:{raw}"),
        ] {
            assert!(invalid.parse::<DeploymentNamespaceV1>().is_err());
            assert!(
                serde_json::from_value::<DeploymentNamespaceV1>(serde_json::json!(invalid))
                    .is_err()
            );
        }
    }
}
