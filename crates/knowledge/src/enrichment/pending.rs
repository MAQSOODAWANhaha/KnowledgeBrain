//! Redis `multimodal:pending:{document_id}` with memory fallback.

use crate::Store;
use uuid::Uuid;

pub fn pending_key(document_id: Uuid) -> String {
    format!("multimodal:pending:{document_id}")
}

fn redis_conn() -> Option<redis::Connection> {
    let url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:16379".into());
    redis::Client::open(url).ok()?.get_connection().ok()
}

/// Redis pending counter without a catalog Store. Memory fallback is per-call only.
pub fn set_pending_count(document_id: Uuid, n: i32) {
    let mut store = Store::default();
    set_pending(&mut store, document_id, n);
}

pub fn decr_pending_count(document_id: Uuid) -> bool {
    let mut store = Store::default();
    decr_pending(&mut store, document_id)
}

/// SET pending=N (TTL 24h). Always writes the in-memory map; Redis when reachable.
pub fn set_pending(store: &mut Store, document_id: Uuid, n: i32) {
    store.multimodal_pending.insert(document_id, n);
    if let Some(mut c) = redis_conn() {
        let key = pending_key(document_id);
        let _: Result<(), _> = redis::cmd("SET")
            .arg(&key)
            .arg(n)
            .arg("EX")
            .arg(24 * 3600)
            .query(&mut c);
    }
}

/// DECR. Redis error → treat as done (fallback enqueue). Memory used if Redis down.
pub fn decr_pending(store: &mut Store, document_id: Uuid) -> bool {
    if let Some(mut c) = redis_conn() {
        let key = pending_key(document_id);
        let n: Result<i64, _> = redis::cmd("DECR").arg(&key).query(&mut c);
        match n {
            Ok(v) => {
                if v <= 0 {
                    let _: Result<(), _> = redis::cmd("DEL").arg(&key).query(&mut c);
                    store.multimodal_pending.remove(&document_id);
                    return true;
                }
                store.multimodal_pending.insert(document_id, v as i32);
                return false;
            }
            Err(_) => {
                store.multimodal_pending.remove(&document_id);
                return true;
            }
        }
    }
    let n = {
        let e = store.multimodal_pending.entry(document_id).or_insert(0);
        *e -= 1;
        *e
    };
    if n <= 0 {
        store.multimodal_pending.remove(&document_id);
        true
    } else {
        false
    }
}

/// Redis GET of the pending counter (None if Redis down or key missing).
pub fn pending_count(document_id: Uuid) -> Option<i32> {
    let mut c = redis_conn()?;
    let n: i32 = redis::cmd("GET")
        .arg(pending_key(document_id))
        .query(&mut c)
        .ok()?;
    Some(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_decr_signals_postprocess() {
        let mut s = Store::default();
        let id = Uuid::new_v4();
        set_pending(&mut s, id, 1);
        assert!(decr_pending(&mut s, id));
    }
}
