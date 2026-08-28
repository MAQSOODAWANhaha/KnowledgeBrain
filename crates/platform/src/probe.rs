//! Process liveness and readiness JSON shared by API, worker, and retention.

use serde::Serialize;
use sqlx::PgPool;

pub const REASON_POSTGRES_UNAVAILABLE: &str = "postgres_unavailable";
pub const REASON_MAINTENANCE_GATE_UNREADABLE: &str = "maintenance_gate_unreadable";
pub const REASON_MAINTENANCE: &str = "maintenance";
pub const REASON_DRAINING: &str = "draining";
pub const REASON_ROLLBACK: &str = "rollback";
pub const REASON_GATE_MODE_NOT_LIVE_READY: &str = "gate_mode_not_live_ready";
pub const REASON_QUEUE_REGISTRY_UNREADABLE: &str = "queue_registry_unreadable";

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
    pub gate_mode: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadyCheck {
    Ready {
        gate_mode: String,
    },
    NotReady {
        reason: &'static str,
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
        ReadyCheck::Ready { gate_mode } => ReadyBody {
            status: "ok",
            probe: "ready",
            service,
            reason: None,
            gate_mode: Some(gate_mode.clone()),
        },
        ReadyCheck::NotReady { reason, gate_mode } => ReadyBody {
            status: "not_ready",
            probe: "ready",
            service,
            reason: Some(*reason),
            gate_mode: gate_mode.clone(),
        },
    }
}

pub async fn check_readiness() -> ReadyCheck {
    match crate::connect().await {
        Ok(pool) => inspect_readiness(&pool).await,
        Err(_) => ReadyCheck::NotReady {
            reason: REASON_POSTGRES_UNAVAILABLE,
            gate_mode: None,
        },
    }
}

/// Readiness checks live dependencies and the ordinary maintenance gate only.
/// It has no deployment-manifest, launch-marker, catalog-verifier, or desired-state input.
pub async fn inspect_readiness(pool: &PgPool) -> ReadyCheck {
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
    let mode = match sqlx::query_scalar::<_, String>(
        "SELECT mode FROM public.application_maintenance_gate WHERE singleton_key",
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
            gate_mode: Some(LIVE_READY_GATE_MODE.to_string()),
        };
    }
    check
}

pub fn inspect_gate_mode(mode: impl Into<String>) -> ReadyCheck {
    let mode = mode.into();
    match gate_mode_not_ready_reason(&mode) {
        None => ReadyCheck::Ready { gate_mode: mode },
        Some(reason) => ReadyCheck::NotReady {
            reason,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maintenance_modes_fail_closed() {
        assert!(inspect_gate_mode("open").is_ready());
        assert_eq!(
            gate_mode_not_ready_reason("maintenance"),
            Some(REASON_MAINTENANCE)
        );
        assert_eq!(
            gate_mode_not_ready_reason("unknown"),
            Some(REASON_GATE_MODE_NOT_LIVE_READY)
        );
    }

    #[test]
    fn readiness_contract_has_no_launch_manifest_identity() {
        let body = ready_body("api", &inspect_gate_mode("open"));
        let json = serde_json::to_value(body).unwrap();
        assert!(json.get("schema_manifest_sha256").is_none());
    }
}
