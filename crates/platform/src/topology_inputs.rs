//! Signed Compose INPUT CLOSURE declarations.
//!
//! This reader validates the checked-in file list and workspace shape only.
//! It does not compute or fill runtime-completion `topology_sha256`.

use serde::Deserialize;
use std::path::{Path, PathBuf};
use thiserror::Error;

const INPUTS_RELATIVE: &str = "deploy/first-launch/topology-inputs.toml";
const ROOT_COMPOSE: &str = "docker-compose.yml";
const DEPLOY_COMPOSE: &str = "deploy/docker-compose.yml";
const IMPLICIT_OVERRIDE: &str = "docker-compose.override.yml";

/// Exact ordered Compose inputs recorded by the signed closure.
pub const REQUIRED_TOPOLOGY_FILES: [&str; 2] = [ROOT_COMPOSE, DEPLOY_COMPOSE];

#[derive(Debug, Error)]
pub enum TopologyInputsError {
    #[error("topology inputs not found at {INPUTS_RELATIVE}")]
    NotFound,
    #[error("topology inputs unreadable: {0}")]
    Io(String),
    #[error("topology inputs parse failed: {0}")]
    Parse(String),
    #[error("topology inputs invalid: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TopologyInputs {
    pub format_version: u32,
    pub contract: String,
    pub files: Vec<String>,
    pub reject_implicit_override: bool,
    pub notes: String,
}

impl TopologyInputs {
    pub fn parse(source: &str) -> Result<Self, TopologyInputsError> {
        let inputs: Self = toml::from_str(source)
            .map_err(|error| TopologyInputsError::Parse(error.to_string()))?;
        inputs.validate()?;
        Ok(inputs)
    }

    pub fn load() -> Result<Self, TopologyInputsError> {
        Self::load_from_path(locate_path(INPUTS_RELATIVE)?)
    }

    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self, TopologyInputsError> {
        let path = path.as_ref();
        let source = std::fs::read_to_string(path)
            .map_err(|error| TopologyInputsError::Io(format!("{}: {error}", path.display())))?;
        let inputs = Self::parse(&source)?;
        inputs.validate_workspace(&repo_root_from_inputs_path(path)?)?;
        Ok(inputs)
    }

    pub fn files(&self) -> &[String] {
        &self.files
    }

    fn validate(&self) -> Result<(), TopologyInputsError> {
        if self.format_version != 1 {
            return Err(invalid("format_version must be 1"));
        }
        if self.contract != "bid-matching-v1" {
            return Err(invalid("contract must be bid-matching-v1"));
        }
        if self.files.as_slice() != REQUIRED_TOPOLOGY_FILES {
            return Err(invalid(
                "files must be exactly [\"docker-compose.yml\", \"deploy/docker-compose.yml\"] in that order",
            ));
        }
        if !self.reject_implicit_override {
            return Err(invalid("reject_implicit_override must be true"));
        }
        if self.notes.trim().is_empty() {
            return Err(invalid("notes must explain refuse rules"));
        }
        let notes = self.notes.to_ascii_lowercase();
        if !notes.contains("docker-compose.override.yml")
            || !notes.contains("-f")
            || !notes.contains("compose_file")
            || !notes.contains("refuse")
        {
            return Err(invalid(
                "notes must refuse docker-compose.override.yml, extra -f, and COMPOSE_FILE not in this list",
            ));
        }
        Ok(())
    }

    fn validate_workspace(&self, repo_root: &Path) -> Result<(), TopologyInputsError> {
        refuse_implicit_override(repo_root)?;
        let root_compose = repo_root.join(ROOT_COMPOSE);
        let deploy_compose = repo_root.join(DEPLOY_COMPOSE);
        let root_source = std::fs::read_to_string(&root_compose).map_err(|error| {
            TopologyInputsError::Io(format!("{}: {error}", root_compose.display()))
        })?;
        validate_root_include_only(&root_source)?;
        if !deploy_compose.is_file() {
            return Err(invalid(
                "signed closure is missing deploy/docker-compose.yml final service definition",
            ));
        }
        Ok(())
    }
}

fn validate_root_include_only(source: &str) -> Result<(), TopologyInputsError> {
    let mut significant = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        significant.push(trimmed.to_string());
    }
    let expected = ["include:", "- path: deploy/docker-compose.yml"];
    let expected_quoted = ["include:", "- path: \"deploy/docker-compose.yml\""];
    if significant != expected && significant != expected_quoted {
        return Err(invalid(
            "root docker-compose.yml must contain only an include of deploy/docker-compose.yml",
        ));
    }
    Ok(())
}

fn refuse_implicit_override(repo_root: &Path) -> Result<(), TopologyInputsError> {
    let override_path = repo_root.join(IMPLICIT_OVERRIDE);
    if override_path.exists() {
        return Err(invalid(
            "docker-compose.override.yml is refuse; implicit Compose override is not in the signed input closure",
        ));
    }
    Ok(())
}

fn invalid(message: &str) -> TopologyInputsError {
    TopologyInputsError::Invalid(message.to_string())
}

fn locate_path(relative: &str) -> Result<PathBuf, TopologyInputsError> {
    let mut candidates = vec![
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../")
            .join(relative),
    ];
    if let Ok(cwd) = std::env::current_dir() {
        let mut dir = cwd;
        loop {
            candidates.push(dir.join(relative));
            if !dir.pop() {
                break;
            }
        }
    }
    candidates
        .into_iter()
        .find(|path| path.is_file())
        .ok_or(TopologyInputsError::NotFound)
}

fn repo_root_from_inputs_path(path: &Path) -> Result<PathBuf, TopologyInputsError> {
    path.parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| invalid("topology-inputs.toml must live at deploy/first-launch/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RuntimeCompletion;

    const VALID: &str = r#"
format_version = 1
contract = "bid-matching-v1"
files = [
  "docker-compose.yml",
  "deploy/docker-compose.yml",
]
reject_implicit_override = true
notes = "Any docker-compose.override.yml, extra -f, or COMPOSE_FILE not in this list is refuse."
"#;

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    #[test]
    fn checked_in_inputs_are_exact_two_files_in_order() {
        let inputs = TopologyInputs::load().expect("repo-relative topology inputs");
        assert_eq!(inputs.format_version, 1);
        assert_eq!(inputs.contract, "bid-matching-v1");
        assert_eq!(
            inputs.files(),
            &["docker-compose.yml", "deploy/docker-compose.yml"]
        );
        assert!(inputs.reject_implicit_override);
        assert!(inputs.notes.to_ascii_lowercase().contains("refuse"));
    }

    #[test]
    fn deny_unknown_fields() {
        let source = r#"
format_version = 1
contract = "bid-matching-v1"
files = [
  "docker-compose.yml",
  "deploy/docker-compose.yml",
]
reject_implicit_override = true
unexpected = true
notes = "Any docker-compose.override.yml, extra -f, or COMPOSE_FILE not in this list is refuse."
"#;
        assert!(TopologyInputs::parse(source).is_err());
    }

    #[test]
    fn files_must_be_exact_ordered_pair() {
        let swapped = r#"
format_version = 1
contract = "bid-matching-v1"
files = [
  "deploy/docker-compose.yml",
  "docker-compose.yml",
]
reject_implicit_override = true
notes = "Any docker-compose.override.yml, extra -f, or COMPOSE_FILE not in this list is refuse."
"#;
        let extra = r#"
format_version = 1
contract = "bid-matching-v1"
files = [
  "docker-compose.yml",
  "deploy/docker-compose.yml",
  "docker-compose.override.yml",
]
reject_implicit_override = true
notes = "Any docker-compose.override.yml, extra -f, or COMPOSE_FILE not in this list is refuse."
"#;
        for source in [swapped, extra] {
            let error = TopologyInputs::parse(source).expect_err(source);
            assert!(error.to_string().contains("exactly"), "{error}");
        }
        assert!(TopologyInputs::parse(VALID).is_ok());
    }

    #[test]
    fn root_compose_is_include_only_delegator() {
        let source = std::fs::read_to_string(repo_root().join(ROOT_COMPOSE))
            .expect("read real root docker-compose.yml");
        validate_root_include_only(&source)
            .expect("root docker-compose.yml must include only deploy/docker-compose.yml");
        TopologyInputs::load().expect("checked-in closure must accept the real root file");
    }

    #[test]
    fn implicit_override_in_repo_root_is_refuse() {
        let repo = repo_root();
        assert!(
            !repo.join(IMPLICIT_OVERRIDE).exists(),
            "repo root has no docker-compose.override.yml; implicit override is refuse and must stay absent"
        );
        refuse_implicit_override(&repo).expect("absent override is accepted");

        let tmp = std::env::temp_dir().join(format!("kb-topology-override-{}", crate::new_id()));
        std::fs::create_dir_all(tmp.join("deploy")).expect("temp deploy dir");
        std::fs::write(
            tmp.join(ROOT_COMPOSE),
            "include:\n  - path: deploy/docker-compose.yml\n",
        )
        .expect("temp root compose");
        std::fs::write(tmp.join(DEPLOY_COMPOSE), "services: {}\n").expect("temp deploy compose");
        std::fs::write(tmp.join(IMPLICIT_OVERRIDE), "services: {}\n").expect("temp override");
        let inputs = TopologyInputs::parse(VALID).expect("valid declaration");
        let error = inputs
            .validate_workspace(&tmp)
            .expect_err("docker-compose.override.yml must be refuse");
        assert!(
            error.to_string().contains("override") && error.to_string().contains("refuse"),
            "{error}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn runtime_completion_topology_sha256_stays_empty() {
        let completion = RuntimeCompletion::load().expect("runtime completion");
        assert!(
            !completion.phase_1d_runtime_complete,
            "topology input closure must not flip phase_1d_runtime_complete"
        );
        assert!(
            completion.topology_sha256.is_empty(),
            "do not invent topology_sha256; production hash fill-in is out of scope"
        );
        assert!(completion.hashes_are_empty());
    }
}
