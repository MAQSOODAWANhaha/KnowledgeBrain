//! Process liveness and readiness JSON shared by API and worker probes.

use serde::Serialize;
use sqlx::PgPool;
use std::collections::BTreeSet;

pub const REASON_POSTGRES_UNAVAILABLE: &str = "postgres_unavailable";
pub const REASON_SCHEMA_MANIFEST_IDENTITY_MISMATCH: &str = "schema_manifest_identity_mismatch";
pub const REASON_MAINTENANCE_GATE_UNREADABLE: &str = "maintenance_gate_unreadable";
pub const REASON_MAINTENANCE: &str = "maintenance";
pub const REASON_DRAINING: &str = "draining";
pub const REASON_ROLLBACK: &str = "rollback";
pub const REASON_GATE_MODE_NOT_LIVE_READY: &str = "gate_mode_not_live_ready";
pub const REASON_QUEUE_REGISTRY_UNREADABLE: &str = "queue_registry_unreadable";
pub const REASON_INTENDED_STATE_UNREADABLE: &str = "intended_state_unreadable";
pub const REASON_DISABLED_LANE_MISMATCH: &str = "disabled_lane_mismatch";
pub const REASON_CHAT_CAPABILITY_UNCONFIGURED: &str = "chat_capability_unconfigured";
pub const REASON_EMBEDDING_CAPABILITY_UNCONFIGURED: &str = "embedding_capability_unconfigured";
pub const REASON_REDIS_CAPABILITY_UNCONFIGURED: &str = "redis_capability_unconfigured";
pub const REASON_OBJECT_STORE_CAPABILITY_UNCONFIGURED: &str =
    "object_store_capability_unconfigured";
pub const REASON_DOCREADER_CAPABILITY_UNCONFIGURED: &str = "docreader_capability_unconfigured";

/// Schema allowlist for `application_maintenance_gate.mode`.
const LIVE_READY_GATE_MODE: &str = "open";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LiveBody {
    pub status: &'static str,
    pub probe: &'static str,
    pub service: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReadyBody {
    pub status: &'static str,
    pub probe: &'static str,
    pub service: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema_manifest_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gate_mode: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadyCheck {
    Ready {
        schema_manifest_sha256: String,
        gate_mode: String,
    },
    NotReady {
        reason: &'static str,
        schema_manifest_sha256: Option<String>,
        gate_mode: Option<String>,
    },
}

impl ReadyCheck {
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready { .. })
    }
}

pub fn live_body(service: &'static str) -> LiveBody {
    LiveBody {
        status: "ok",
        probe: "live",
        service,
    }
}

pub fn ready_body(service: &'static str, check: &ReadyCheck) -> ReadyBody {
    match check {
        ReadyCheck::Ready {
            schema_manifest_sha256,
            gate_mode,
        } => ReadyBody {
            status: "ok",
            probe: "ready",
            service,
            reason: None,
            schema_manifest_sha256: Some(schema_manifest_sha256.clone()),
            gate_mode: Some(gate_mode.clone()),
        },
        ReadyCheck::NotReady {
            reason,
            schema_manifest_sha256,
            gate_mode,
        } => ReadyBody {
            status: "not_ready",
            probe: "ready",
            service,
            reason: Some(*reason),
            schema_manifest_sha256: schema_manifest_sha256.clone(),
            gate_mode: gate_mode.clone(),
        },
    }
}

pub async fn check_readiness() -> ReadyCheck {
    match crate::connect().await {
        Ok(pool) => inspect_readiness(&pool).await,
        Err(_) => ReadyCheck::NotReady {
            reason: REASON_POSTGRES_UNAVAILABLE,
            schema_manifest_sha256: None,
            gate_mode: None,
        },
    }
}

pub async fn inspect_readiness(pool: &PgPool) -> ReadyCheck {
    if sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(pool)
        .await
        .is_err()
    {
        return ReadyCheck::NotReady {
            reason: REASON_POSTGRES_UNAVAILABLE,
            schema_manifest_sha256: None,
            gate_mode: None,
        };
    }
    if crate::verify_schema_identity(pool).await.is_err() {
        return ReadyCheck::NotReady {
            reason: REASON_SCHEMA_MANIFEST_IDENTITY_MISMATCH,
            schema_manifest_sha256: None,
            gate_mode: None,
        };
    }
    let schema_manifest_sha256 = crate::schema_manifest_sha256();

    match sqlx::query_scalar::<_, String>(
        "SELECT mode FROM public.application_maintenance_gate WHERE singleton_key",
    )
    .fetch_optional(pool)
    .await
    {
        Ok(Some(mode)) => inspect_ready_after_gate(inspect_gate_mode(schema_manifest_sha256, mode)),
        Ok(None) | Err(_) => ReadyCheck::NotReady {
            reason: REASON_MAINTENANCE_GATE_UNREADABLE,
            schema_manifest_sha256: Some(schema_manifest_sha256),
            gate_mode: None,
        },
    }
}

/// Ready only when `mode` is exactly the schema live-ready value `open`.
/// `maintenance`, `draining`, and `rollback` are distinct not-ready reasons.
/// Any other value, including unknown modes, is fail-closed not-ready.
pub fn inspect_gate_mode(
    schema_manifest_sha256: impl Into<String>,
    mode: impl Into<String>,
) -> ReadyCheck {
    let schema_manifest_sha256 = schema_manifest_sha256.into();
    let mode = mode.into();
    match gate_mode_not_ready_reason(&mode) {
        None => ReadyCheck::Ready {
            schema_manifest_sha256,
            gate_mode: mode,
        },
        Some(reason) => ReadyCheck::NotReady {
            reason,
            schema_manifest_sha256: Some(schema_manifest_sha256),
            gate_mode: Some(mode),
        },
    }
}

pub fn gate_mode_not_ready_reason(mode: &str) -> Option<&'static str> {
    match mode {
        LIVE_READY_GATE_MODE => None,
        "maintenance" => Some(REASON_MAINTENANCE),
        "draining" => Some(REASON_DRAINING),
        "rollback" => Some(REASON_ROLLBACK),
        _ => Some(REASON_GATE_MODE_NOT_LIVE_READY),
    }
}

fn signed_launch_artifact_reason(registry_ok: bool, intended_ok: bool) -> Option<&'static str> {
    if !registry_ok {
        Some(REASON_QUEUE_REGISTRY_UNREADABLE)
    } else if !intended_ok {
        Some(REASON_INTENDED_STATE_UNREADABLE)
    } else {
        None
    }
}

/// Load the signed intended-state snapshot and canonical queue registry.
/// Does not require image-lock production-completeness or runtime-completion.
pub fn inspect_signed_launch_artifacts() -> Result<(), &'static str> {
    let registry_ok = domain::QueueRegistry::load().is_ok();
    let intended_ok = registry_ok && domain::IntendedState::load().is_ok();
    match signed_launch_artifact_reason(registry_ok, intended_ok) {
        Some(reason) => Err(reason),
        None => Ok(()),
    }
}

fn chat_capability_ready() -> bool {
    let model = domain::chat_model();
    let model = model.trim();
    !domain::chat_base_url().is_empty() && !model.is_empty() && model != "stub-chat"
}

fn embedding_capability_ready() -> bool {
    let model = domain::embedding_model();
    let model = model.trim();
    !domain::embedding_base_url().is_empty() && !model.is_empty() && model != "stub-emb"
}

fn env_nonempty(key: &str) -> bool {
    std::env::var(key)
        .ok()
        .is_some_and(|value| !value.trim().is_empty())
}

fn redis_capability_ready() -> bool {
    env_nonempty("REDIS_URL")
}

fn object_store_capability_ready() -> bool {
    !crate::s3::bucket().trim().is_empty()
        && !crate::s3::endpoint().trim().is_empty()
        && (env_nonempty("KNOWLEDGEBRAIN_S3_ACCESS_KEY") || env_nonempty("MINIO_ROOT_USER"))
        && (env_nonempty("KNOWLEDGEBRAIN_S3_SECRET_KEY") || env_nonempty("MINIO_ROOT_PASSWORD"))
}

fn docreader_capability_ready() -> bool {
    env_nonempty("DOCREADER_ADDR")
}

fn disabled_lanes_ready(
    intended: &domain::IntendedState,
    registry: &domain::QueueRegistry,
) -> bool {
    let intended_disabled: BTreeSet<&str> = intended.disabled_tasks().into_iter().collect();
    let registry_disabled: BTreeSet<&str> =
        registry.declared_disabled_tasks().into_iter().collect();
    let expected_disabled = BTreeSet::from([
        domain::TYPE_IMAGE_MULTIMODAL,
        domain::TYPE_CHUNK_EXTRACT,
        domain::TYPE_QUESTION,
    ]);
    if intended_disabled != registry_disabled || intended_disabled != expected_disabled {
        return false;
    }
    [domain::TYPE_BID_DELIVERY_V1].into_iter().all(|task_type| {
        intended
            .lane_for_task(task_type)
            .is_some_and(|lane| lane.state == domain::FeatureState::Enabled)
    })
}

fn enabled_lane_lists_capability(
    intended: &domain::IntendedState,
    registry: &domain::QueueRegistry,
    capability: &str,
) -> Result<bool, &'static str> {
    for lane in intended.lanes() {
        if lane.state != domain::FeatureState::Enabled {
            continue;
        }
        let Some(entry) = registry.entry_for_task(&lane.task_type) else {
            return Err(REASON_DISABLED_LANE_MISMATCH);
        };
        if entry.capabilities.iter().any(|item| item == capability) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Enforce disabled-lane equality, production no-stub, and enabled
/// chat/embedding/redis/object_store/docreader capabilities.
/// Does not require VLM/OCR/graph, image lock, or runtime-completion.
pub fn inspect_launch_runtime_policy() -> Result<(), &'static str> {
    let registry = domain::QueueRegistry::load().map_err(|_| REASON_DISABLED_LANE_MISMATCH)?;
    let intended = domain::IntendedState::load().map_err(|_| REASON_DISABLED_LANE_MISMATCH)?;
    if !disabled_lanes_ready(&intended, &registry) {
        return Err(REASON_DISABLED_LANE_MISMATCH);
    }
    if enabled_lane_lists_capability(&intended, &registry, "chat")? && !chat_capability_ready() {
        return Err(REASON_CHAT_CAPABILITY_UNCONFIGURED);
    }
    if enabled_lane_lists_capability(&intended, &registry, "embedding")?
        && !embedding_capability_ready()
    {
        return Err(REASON_EMBEDDING_CAPABILITY_UNCONFIGURED);
    }
    if enabled_lane_lists_capability(&intended, &registry, "redis")? && !redis_capability_ready() {
        return Err(REASON_REDIS_CAPABILITY_UNCONFIGURED);
    }
    if enabled_lane_lists_capability(&intended, &registry, "object_store")?
        && !object_store_capability_ready()
    {
        return Err(REASON_OBJECT_STORE_CAPABILITY_UNCONFIGURED);
    }
    if enabled_lane_lists_capability(&intended, &registry, "docreader")?
        && !docreader_capability_ready()
    {
        return Err(REASON_DOCREADER_CAPABILITY_UNCONFIGURED);
    }
    Ok(())
}

/// After the gate is exactly `open`, require signed intended-state + registry,
/// then launch runtime policy. Non-open gates stay not-ready without consulting
/// those artifacts or policy.
fn inspect_ready_after_gate(check: ReadyCheck) -> ReadyCheck {
    match check {
        ReadyCheck::NotReady { .. } => check,
        ReadyCheck::Ready {
            schema_manifest_sha256,
            gate_mode,
        } => match inspect_signed_launch_artifacts() {
            Err(reason) => ReadyCheck::NotReady {
                reason,
                schema_manifest_sha256: Some(schema_manifest_sha256),
                gate_mode: Some(gate_mode),
            },
            Ok(()) => match inspect_launch_runtime_policy() {
                Ok(()) => ReadyCheck::Ready {
                    schema_manifest_sha256,
                    gate_mode,
                },
                Err(reason) => ReadyCheck::NotReady {
                    reason,
                    schema_manifest_sha256: Some(schema_manifest_sha256),
                    gate_mode: Some(gate_mode),
                },
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_reports_manifest_identity_not_numeric_head() {
        let digest = "a".repeat(64);
        let body = ready_body("api", &inspect_gate_mode(digest.clone(), "open"));
        assert_eq!(body.schema_manifest_sha256, Some(digest));
        assert!(
            serde_json::to_value(body)
                .unwrap()
                .get("migration_head")
                .is_none()
        );
    }

    #[test]
    fn unknown_gate_mode_fails_closed() {
        let check = inspect_gate_mode("a".repeat(64), "future");
        assert!(matches!(
            check,
            ReadyCheck::NotReady {
                reason: REASON_GATE_MODE_NOT_LIVE_READY,
                ..
            }
        ));
    }
}
