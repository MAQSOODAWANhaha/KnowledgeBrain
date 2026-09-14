//! Deployment-scoped Redis multimodal counters.
//! Redis SET/DECR failure is an error; callers treat it as transient. There is
//! no process-local HashMap fallback on the public count API.

use platform::DeploymentNamespaceV1;
use std::collections::HashMap;
use uuid::Uuid;

pub fn pending_key(namespace: DeploymentNamespaceV1, document_id: Uuid) -> String {
    format!(
        "{}multimodal:pending:{document_id}",
        namespace.redis_prefix()
    )
}

fn redis_conn() -> Option<(redis::Connection, DeploymentNamespaceV1)> {
    let namespace = DeploymentNamespaceV1::from_environment().ok()?;
    let url = std::env::var("REDIS_URL").ok()?;
    let connection = redis::Client::open(url).ok()?.get_connection().ok()?;
    Some((connection, namespace))
}

fn redis_unavailable() -> String {
    "multimodal pending redis unavailable".into()
}

/// SET pending=N (TTL 24h) on the deployment Redis key.
pub fn set_pending_count(document_id: Uuid, n: i32) -> Result<(), String> {
    let (mut c, namespace) = redis_conn().ok_or_else(redis_unavailable)?;
    let key = pending_key(namespace, document_id);
    redis::cmd("SET")
        .arg(&key)
        .arg(n)
        .arg("EX")
        .arg(24 * 3600)
        .query::<()>(&mut c)
        .map_err(|error| error.to_string())
}

/// DECR. `Ok(true)` when the counter reached zero. Redis errors do not mean done.
pub fn decr_pending_count(document_id: Uuid) -> Result<bool, String> {
    let (mut c, namespace) = redis_conn().ok_or_else(redis_unavailable)?;
    let key = pending_key(namespace, document_id);
    match redis::cmd("DECR").arg(&key).query::<i64>(&mut c) {
        Ok(v) if v <= 0 => {
            let _: Result<(), _> = redis::cmd("DEL").arg(&key).query(&mut c);
            Ok(true)
        }
        Ok(_) => Ok(false),
        Err(error) => Err(error.to_string()),
    }
}

/// SET pending=N into a caller-owned map; Redis when reachable. Tests only.
pub fn set_pending(pending: &mut HashMap<Uuid, i32>, document_id: Uuid, n: i32) {
    pending.insert(document_id, n);
}

/// In-memory DECR for tests that do not touch Redis.
pub fn decr_pending(pending: &mut HashMap<Uuid, i32>, document_id: Uuid) -> bool {
    let n = {
        let e = pending.entry(document_id).or_insert(0);
        *e -= 1;
        *e
    };
    if n <= 0 {
        pending.remove(&document_id);
        true
    } else {
        false
    }
}

/// Redis GET of the pending counter (None if Redis down or key missing).
pub fn pending_count(document_id: Uuid) -> Option<i32> {
    let (mut c, namespace) = redis_conn()?;
    let n: i32 = redis::cmd("GET")
        .arg(pending_key(namespace, document_id))
        .query(&mut c)
        .ok()?;
    Some(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_decr_signals_postprocess() {
        let mut pending = HashMap::new();
        let id = Uuid::new_v4();
        set_pending(&mut pending, id, 1);
        assert!(decr_pending(&mut pending, id));
    }

    #[test]
    fn public_count_api_errors_without_redis() {
        let id = Uuid::new_v4();
        assert!(set_pending_count(id, 1).is_err());
        assert!(decr_pending_count(id).is_err());
    }
}
