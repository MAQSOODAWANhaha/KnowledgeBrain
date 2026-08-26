//! First-launch evaluation artifact and runtime-completion promotion gate.
//!
//! The checked-in evaluation is inconclusive: a real provider evaluation was
//! not run, and inconclusive status cannot promote runtime completion.

use crate::image_lock::ImageLock;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use thiserror::Error;

const EVAL_RELATIVE: &str = "deploy/eval/first-launch-evaluation.toml";
const COMPLETION_RELATIVE: &str = "deploy/first-launch/runtime-completion.toml";
const BIND_REGISTRY_RELATIVE: &str = "deploy/queue-registry.toml";
const BIND_READINESS_RELATIVE: &str = "deploy/health/mode-aware-probe.sh";
const BIND_IMAGES_RELATIVE: &str = "deploy/images.lock.json";
const BIND_TOPOLOGY_RELATIVE: &str = "deploy/first-launch/topology-inputs.toml";
const ALL_ZERO_SHA256: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Debug, Error)]
pub enum FirstLaunchEvaluationError {
    #[error("first-launch evaluation not found at {EVAL_RELATIVE}")]
    NotFound,
    #[error("first-launch evaluation unreadable: {0}")]
    Io(String),
    #[error("first-launch evaluation parse failed: {0}")]
    Parse(String),
    #[error("first-launch evaluation invalid: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationStatus {
    Inconclusive,
    Passed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FirstLaunchEvaluation {
    pub format_version: u32,
    pub contract: String,
    pub status: EvaluationStatus,
    pub records: EvaluationRecords,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluationRecords {
    /// Placeholder is allowed; this field is not a signed git identity.
    pub git_head: String,
    pub dirty: bool,
    pub commands: Vec<String>,
    pub provider: String,
    pub model: String,
    pub fixture_hash: String,
    #[serde(default)]
    pub exit_code: Option<i32>,
    pub notes: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeCompletion {
    pub format_version: u32,
    pub schema_manifest_sha256: String,
    pub contract: String,
    pub phase_1d_runtime_complete: bool,
    pub registry_sha256: String,
    pub evaluation_sha256: String,
    pub readiness_sha256: String,
    pub images_sha256: String,
    pub topology_sha256: String,
}

impl FirstLaunchEvaluation {
    pub fn parse(source: &str) -> Result<Self, FirstLaunchEvaluationError> {
        let evaluation: Self = toml::from_str(source)
            .map_err(|error| FirstLaunchEvaluationError::Parse(error.to_string()))?;
        evaluation.validate()?;
        Ok(evaluation)
    }

    pub fn load() -> Result<Self, FirstLaunchEvaluationError> {
        Self::load_from_path(locate_path(
            EVAL_RELATIVE,
            FirstLaunchEvaluationError::NotFound,
        )?)
    }

    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self, FirstLaunchEvaluationError> {
        let path = path.as_ref();
        let source = std::fs::read_to_string(path).map_err(|error| {
            FirstLaunchEvaluationError::Io(format!("{}: {error}", path.display()))
        })?;
        Self::parse(&source)
    }

    pub fn can_promote(&self) -> bool {
        self.status == EvaluationStatus::Passed
    }

    fn validate(&self) -> Result<(), FirstLaunchEvaluationError> {
        if self.format_version != 1 {
            return Err(invalid("format_version must be 1"));
        }
        if self.contract != "bidding-v1" {
            return Err(invalid("contract must be bidding-v1"));
        }
        if self.records.notes.trim().is_empty() {
            return Err(invalid("records.notes must explain evaluation evidence"));
        }
        Ok(())
    }
}

impl RuntimeCompletion {
    pub fn parse(source: &str) -> Result<Self, FirstLaunchEvaluationError> {
        toml::from_str(source).map_err(|error| FirstLaunchEvaluationError::Parse(error.to_string()))
    }

    pub fn load() -> Result<Self, FirstLaunchEvaluationError> {
        let path = locate_path(COMPLETION_RELATIVE, FirstLaunchEvaluationError::NotFound)?;
        let source = std::fs::read_to_string(&path).map_err(|error| {
            FirstLaunchEvaluationError::Io(format!("{}: {error}", path.display()))
        })?;
        Self::parse(&source)
    }

    pub fn sha256_fields(&self) -> [&str; 5] {
        [
            self.registry_sha256.as_str(),
            self.evaluation_sha256.as_str(),
            self.readiness_sha256.as_str(),
            self.images_sha256.as_str(),
            self.topology_sha256.as_str(),
        ]
    }

    pub fn hashes_are_empty(&self) -> bool {
        self.sha256_fields().iter().all(|value| value.is_empty())
    }
}

/// SHA-256 of the five first-launch bind inputs. Never written back to
/// `runtime-completion.toml` by this helper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindCompletionHashes {
    pub registry_sha256: String,
    pub evaluation_sha256: String,
    pub readiness_sha256: String,
    pub images_sha256: String,
    pub topology_sha256: String,
}

impl BindCompletionHashes {
    pub fn sha256_fields(&self) -> [&str; 5] {
        [
            self.registry_sha256.as_str(),
            self.evaluation_sha256.as_str(),
            self.readiness_sha256.as_str(),
            self.images_sha256.as_str(),
            self.topology_sha256.as_str(),
        ]
    }

    pub fn has_all_zero(&self) -> bool {
        self.sha256_fields().contains(&ALL_ZERO_SHA256)
    }
}

/// Hash the five bind artifacts without mutating runtime completion.
pub fn compute_bind_completion_hashes() -> Result<BindCompletionHashes, FirstLaunchEvaluationError>
{
    Ok(BindCompletionHashes {
        registry_sha256: hash_bind_file(BIND_REGISTRY_RELATIVE)?,
        evaluation_sha256: hash_bind_file(EVAL_RELATIVE)?,
        readiness_sha256: hash_bind_file(BIND_READINESS_RELATIVE)?,
        images_sha256: hash_bind_file(BIND_IMAGES_RELATIVE)?,
        topology_sha256: hash_bind_file(BIND_TOPOLOGY_RELATIVE)?,
    })
}

/// Fail-closed bind assessment. Computes hashes and refuses unless evaluation
/// is `passed`, the image lock is production-complete, and no digest is all-zero.
/// This never writes `phase_1d_runtime_complete=true` and never edits
/// `runtime-completion.toml`.
pub fn bind_completion_hashes() -> Result<BindCompletionHashes, FirstLaunchEvaluationError> {
    let evaluation = FirstLaunchEvaluation::load()?;
    let lock = ImageLock::load().map_err(|error| invalid(&error.to_string()))?;
    let hashes = compute_bind_completion_hashes()?;
    allow_bind_completion(&evaluation, &lock, &hashes)?;
    Ok(hashes)
}

/// Bind refuses unless evaluation passed, the lock is production-complete, and
/// no computed digest is all-zero. It does not consult or rewrite completion.
pub fn allow_bind_completion(
    evaluation: &FirstLaunchEvaluation,
    lock: &ImageLock,
    hashes: &BindCompletionHashes,
) -> Result<(), FirstLaunchEvaluationError> {
    if !evaluation.can_promote() {
        return Err(invalid("bind refuses: evaluation status is not passed"));
    }
    if !lock.is_production_complete() {
        return Err(invalid(
            "bind refuses: image lock is not production complete",
        ));
    }
    if hashes.has_all_zero() {
        return Err(invalid("bind refuses: hash would be all-zero"));
    }
    Ok(())
}

fn hash_bind_file(relative: &str) -> Result<String, FirstLaunchEvaluationError> {
    let path = locate_path(relative, FirstLaunchEvaluationError::NotFound)?;
    let bytes = std::fs::read(&path)
        .map_err(|error| FirstLaunchEvaluationError::Io(format!("{}: {error}", path.display())))?;
    Ok(crate::sha256_hex(&bytes))
}

/// Inconclusive evaluation or an empty/unsigned lock cannot promote 1D completion.
pub fn allow_phase_1d_runtime_complete(
    evaluation: &FirstLaunchEvaluation,
    lock: &ImageLock,
    completion: &RuntimeCompletion,
) -> Result<(), FirstLaunchEvaluationError> {
    if !completion.phase_1d_runtime_complete {
        return Ok(());
    }
    if !evaluation.can_promote() {
        return Err(invalid(
            "inconclusive evaluation cannot promote phase_1d_runtime_complete",
        ));
    }
    if !lock.is_production_complete() {
        return Err(invalid(
            "empty or unsigned image lock cannot promote phase_1d_runtime_complete",
        ));
    }
    if completion.hashes_are_empty() {
        return Err(invalid(
            "phase_1d_runtime_complete cannot be true while runtime-completion hashes are empty",
        ));
    }
    Ok(())
}

fn invalid(message: &str) -> FirstLaunchEvaluationError {
    FirstLaunchEvaluationError::Invalid(message.to_string())
}

fn locate_path(
    relative: &str,
    not_found: FirstLaunchEvaluationError,
) -> Result<PathBuf, FirstLaunchEvaluationError> {
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
        .ok_or(not_found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checked_in_eval() -> FirstLaunchEvaluation {
        FirstLaunchEvaluation::load().expect("repo-relative first-launch evaluation")
    }

    fn checked_in_completion() -> RuntimeCompletion {
        RuntimeCompletion::load().expect("repo-relative runtime completion")
    }

    #[test]
    fn checked_in_evaluation_is_inconclusive_and_cannot_promote() {
        let evaluation = checked_in_eval();
        assert_eq!(evaluation.format_version, 1);
        assert_eq!(evaluation.contract, "bidding-v1");
        assert_eq!(evaluation.status, EvaluationStatus::Inconclusive);
        assert!(!evaluation.records.git_head.trim().is_empty());
        assert!(evaluation.records.dirty);
        assert_eq!(evaluation.records.provider, "none");
        assert_eq!(evaluation.records.model, "");
        assert_eq!(evaluation.records.fixture_hash, "");
        assert_eq!(evaluation.records.exit_code, None);
        assert!(
            evaluation
                .records
                .notes
                .to_ascii_lowercase()
                .contains("not run"),
            "notes must say a real provider eval was NOT run"
        );
        assert!(!evaluation.can_promote());
    }

    #[test]
    fn deny_unknown_fields() {
        let source = r#"
format_version = 1
contract = "bidding-v1"
status = "inconclusive"
unexpected = true
[records]
git_head = "placeholder"
dirty = true
commands = []
provider = "none"
model = ""
fixture_hash = ""
notes = "Real provider evaluation was NOT run."
"#;
        assert!(FirstLaunchEvaluation::parse(source).is_err());
    }

    #[test]
    fn runtime_completion_stays_false_when_evaluation_is_not_passed() {
        let evaluation = checked_in_eval();
        let lock = ImageLock::load().expect("image lock");
        let completion = checked_in_completion();
        assert_eq!(evaluation.status, EvaluationStatus::Inconclusive);
        assert!(!completion.phase_1d_runtime_complete);
        assert!(completion.hashes_are_empty());
        assert_eq!(completion.contract, "bidding-v1");
        assert!(allow_phase_1d_runtime_complete(&evaluation, &lock, &completion).is_ok());

        let mut flipped = completion.clone();
        flipped.phase_1d_runtime_complete = true;
        let error = allow_phase_1d_runtime_complete(&evaluation, &lock, &flipped)
            .expect_err("inconclusive cannot promote");
        assert!(error.to_string().contains("cannot promote"));
    }

    #[test]
    fn bind_refuses_on_current_inconclusive_and_empty_lock() {
        let evaluation = checked_in_eval();
        let lock = ImageLock::load().expect("image lock");
        let completion_before = std::fs::read_to_string(
            locate_path(COMPLETION_RELATIVE, FirstLaunchEvaluationError::NotFound)
                .expect("runtime-completion.toml"),
        )
        .expect("read runtime-completion.toml");
        assert_eq!(evaluation.status, EvaluationStatus::Inconclusive);
        assert!(!evaluation.can_promote());
        assert!(
            lock.is_production_complete(),
            "signed api/worker/retention pins may make the lock production-complete; eval still cannot promote"
        );

        let hashes = compute_bind_completion_hashes().expect("can hash current bind inputs");
        assert!(
            hashes.sha256_fields().iter().all(|value| value.len() == 64
                && value.chars().all(|ch| ch.is_ascii_hexdigit())
                && *value != ALL_ZERO_SHA256),
            "current artifacts must hash to non-zero hex digests: {hashes:?}"
        );
        let error = allow_bind_completion(&evaluation, &lock, &hashes)
            .expect_err("bind must refuse current inconclusive eval");
        assert!(error.to_string().contains("bind refuses"), "{error}");
        let bind_error =
            bind_completion_hashes().expect_err("bind_completion_hashes must refuse current tree");
        assert!(
            bind_error.to_string().contains("bind refuses"),
            "{bind_error}"
        );

        let completion = checked_in_completion();
        assert!(!completion.phase_1d_runtime_complete);
        assert!(completion.hashes_are_empty());
        let completion_after = std::fs::read_to_string(
            locate_path(COMPLETION_RELATIVE, FirstLaunchEvaluationError::NotFound)
                .expect("runtime-completion.toml"),
        )
        .expect("reread runtime-completion.toml");
        assert_eq!(
            completion_before, completion_after,
            "bind must not edit runtime-completion.toml"
        );
    }

    #[test]
    fn bind_script_refuses_on_current_inconclusive_and_empty_lock() {
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../deploy/first-launch/bind-completion-hashes.sh");
        let completion_path =
            locate_path(COMPLETION_RELATIVE, FirstLaunchEvaluationError::NotFound)
                .expect("runtime-completion.toml");
        let before = std::fs::read_to_string(&completion_path).expect("read completion");
        let output = std::process::Command::new("bash")
            .arg(&script)
            .output()
            .expect("run bind-completion-hashes.sh");
        assert!(
            !output.status.success(),
            "bind script must refuse current tree; stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        for field in [
            "registry_sha256=",
            "evaluation_sha256=",
            "readiness_sha256=",
            "images_sha256=",
            "topology_sha256=",
        ] {
            assert!(stdout.contains(field), "missing {field} in {stdout}");
            assert!(
                !stdout.contains(&format!("{field}{ALL_ZERO_SHA256}")),
                "printed {field} must not be all-zero"
            );
        }
        let after = std::fs::read_to_string(&completion_path).expect("reread completion");
        assert_eq!(
            before, after,
            "bind script must not edit runtime-completion.toml"
        );
        assert!(before.contains("phase_1d_runtime_complete = false"));
        assert!(before.contains("registry_sha256 = \"\""));
    }

    #[test]
    fn passed_status_still_cannot_promote_empty_lock_or_empty_hashes() {
        let mut evaluation = checked_in_eval();
        evaluation.status = EvaluationStatus::Passed;
        let lock = ImageLock::load().expect("image lock");
        let mut completion = checked_in_completion();
        completion.phase_1d_runtime_complete = true;
        assert!(
            allow_phase_1d_runtime_complete(&evaluation, &lock, &completion).is_err(),
            "empty runtime-completion hashes must block promotion even if evaluation is marked passed"
        );
    }
}
