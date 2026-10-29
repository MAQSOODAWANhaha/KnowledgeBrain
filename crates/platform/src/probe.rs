//! Process liveness and readiness JSON shared by API, worker, and retention.

use serde::{Deserialize, Serialize};
use sqlx::PgPool;

pub const REASON_POSTGRES_UNAVAILABLE: &str = "postgres_unavailable";
pub const REASON_MAINTENANCE_GATE_UNREADABLE: &str = "maintenance_gate_unreadable";
pub const REASON_MAINTENANCE: &str = "maintenance";
pub const REASON_DRAINING: &str = "draining";
pub const REASON_ROLLBACK: &str = "rollback";
pub const REASON_QUEUE_REGISTRY_UNREADABLE: &str = "queue_registry_unreadable";
pub const REASON_EMBEDDING_UNCONFIGURED: &str = "embedding_unconfigured";

/// Tender deployments do not require embedding; knowledge deployments do.
pub fn embedding_config_not_ready_reason() -> Option<&'static str> {
    match crate::knowledge_enabled() {
        Ok(false) => None,
        Ok(true) => embedding_readiness_for_capability(
            true,
            &crate::embedding_base_url(),
            &crate::embedding_model(),
        ),
        Err(_) => Some("invalid_capability_configuration"),
    }
}

pub fn embedding_readiness_for_capability(
    enabled: bool,
    url: &str,
    model: &str,
) -> Option<&'static str> {
    if enabled {
        embedding_config_not_ready_reason_for(url, model)
    } else {
        None
    }
}

pub fn embedding_config_not_ready_reason_for(url: &str, model: &str) -> Option<&'static str> {
    if !crate::vlm_endpoint_ready(url, model) || model == "stub-emb" {
        Some(REASON_EMBEDDING_UNCONFIGURED)
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "text", rename_all = "snake_case")]
pub enum MaintenanceMode {
    Maintenance,
    Open,
    Draining,
    Rollback,
}

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
    pub gate_mode: Option<MaintenanceMode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadyCheck {
    Ready {
        gate_mode: MaintenanceMode,
    },
    NotReady {
        reason: &'static str,
        gate_mode: Option<MaintenanceMode>,
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
        ReadyCheck::Ready { gate_mode } => ReadyBody {
            status: "ok",
            probe: "ready",
            service,
            reason: None,
            gate_mode: Some(*gate_mode),
        },
        ReadyCheck::NotReady { reason, gate_mode } => ReadyBody {
            status: "not_ready",
            probe: "ready",
            service,
            reason: Some(*reason),
            gate_mode: *gate_mode,
        },
    }
}

pub async fn check_readiness(expected_component: crate::ReleaseComponentKind) -> ReadyCheck {
    match crate::connect().await {
        Ok(pool) => inspect_readiness(&pool, expected_component).await,
        Err(_) => ReadyCheck::NotReady {
            reason: REASON_POSTGRES_UNAVAILABLE,
            gate_mode: None,
        },
    }
}

/// Readiness checks database connectivity and the maintenance gate.
pub async fn inspect_readiness(
    pool: &PgPool,
    expected_component: crate::ReleaseComponentKind,
) -> ReadyCheck {
    if sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(pool)
        .await
        .is_err()
    {
        return ReadyCheck::NotReady {
            reason: REASON_POSTGRES_UNAVAILABLE,
            gate_mode: None,
        };
    }
    let mode = match sqlx::query_scalar::<_, MaintenanceMode>(
        "SELECT mode FROM public.platform_maintenance_state WHERE singleton_key",
    )
    .fetch_optional(pool)
    .await
    {
        Ok(Some(mode)) => mode,
        Ok(None) | Err(_) => {
            return ReadyCheck::NotReady {
                reason: REASON_MAINTENANCE_GATE_UNREADABLE,
                gate_mode: None,
            };
        }
    };
    let check = inspect_gate_mode(mode);
    if !check.is_ready() {
        return check;
    }
    if crate::QueueRegistry::load().is_err() {
        return ReadyCheck::NotReady {
            reason: REASON_QUEUE_REGISTRY_UNREADABLE,
            gate_mode: Some(MaintenanceMode::Open),
        };
    }
    if matches!(
        expected_component,
        crate::ReleaseComponentKind::Api | crate::ReleaseComponentKind::Worker
    ) && let Some(reason) = embedding_config_not_ready_reason()
    {
        return ReadyCheck::NotReady {
            reason,
            gate_mode: Some(MaintenanceMode::Open),
        };
    }
    check
}

pub fn inspect_gate_mode(mode: MaintenanceMode) -> ReadyCheck {
    match gate_mode_not_ready_reason(mode) {
        None => ReadyCheck::Ready { gate_mode: mode },
        Some(reason) => ReadyCheck::NotReady {
            reason,
            gate_mode: Some(mode),
        },
    }
}

pub fn gate_mode_not_ready_reason(mode: MaintenanceMode) -> Option<&'static str> {
    match mode {
        MaintenanceMode::Open => None,
        MaintenanceMode::Maintenance => Some(REASON_MAINTENANCE),
        MaintenanceMode::Draining => Some(REASON_DRAINING),
        MaintenanceMode::Rollback => Some(REASON_ROLLBACK),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tender_readiness_does_not_require_disabled_embedding() {
        assert_eq!(embedding_readiness_for_capability(false, "", ""), None);
        assert_eq!(
            embedding_readiness_for_capability(true, "", ""),
            Some(REASON_EMBEDDING_UNCONFIGURED)
        );
        assert_eq!(
            embedding_readiness_for_capability(true, "https://embedding.invalid", "embedding"),
            None
        );
        assert_eq!(
            embedding_readiness_for_capability(true, "not-a-url", "embedding"),
            Some(REASON_EMBEDDING_UNCONFIGURED)
        );
    }

    #[test]
    fn embedding_config_requires_url_and_model() {
        assert_eq!(
            embedding_config_not_ready_reason_for("", "live-emb"),
            Some(REASON_EMBEDDING_UNCONFIGURED)
        );
        assert_eq!(
            embedding_config_not_ready_reason_for("http://127.0.0.1:9", ""),
            Some(REASON_EMBEDDING_UNCONFIGURED)
        );
        assert_eq!(
            embedding_config_not_ready_reason_for("http://127.0.0.1:9", "live-emb"),
            None
        );
    }

    #[test]
    fn maintenance_modes_fail_closed() {
        assert!(inspect_gate_mode(MaintenanceMode::Open).is_ready());
        assert_eq!(
            gate_mode_not_ready_reason(MaintenanceMode::Maintenance),
            Some(REASON_MAINTENANCE)
        );
        assert!(serde_json::from_str::<MaintenanceMode>("\"unknown\"").is_err());
        for (mode, wire) in [
            (MaintenanceMode::Open, "open"),
            (MaintenanceMode::Maintenance, "maintenance"),
            (MaintenanceMode::Draining, "draining"),
            (MaintenanceMode::Rollback, "rollback"),
        ] {
            assert_eq!(serde_json::to_value(mode).unwrap(), wire);
            assert_eq!(
                serde_json::from_value::<MaintenanceMode>(serde_json::json!(wire)).unwrap(),
                mode
            );
            assert_eq!(
                inspect_gate_mode(mode).is_ready(),
                mode == MaintenanceMode::Open
            );
        }
    }

    #[test]
    fn readiness_failure_exposes_only_stable_reason() {
        let body = ready_body(
            "api",
            &ReadyCheck::NotReady {
                reason: REASON_MAINTENANCE_GATE_UNREADABLE,
                gate_mode: None,
            },
        );
        let json = serde_json::to_value(body).unwrap();
        assert_eq!(json["reason"], REASON_MAINTENANCE_GATE_UNREADABLE);
    }
}
