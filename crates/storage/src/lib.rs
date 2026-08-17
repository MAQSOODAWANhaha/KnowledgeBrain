//! Object bytes keyed as `objects/{sha256}`. Postgres migrate + workspace seed.

mod persist;
mod s3;

pub use persist::*;
pub use s3::{configured as s3_configured, delete_object, get_object, put_object};

use domain::{Store, sha256_hex};
use std::path::{Path, PathBuf};

pub fn object_dir() -> PathBuf {
    std::env::var("OBJECT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("var/objects"))
}

pub fn blob_path(hash: &str) -> PathBuf {
    object_dir().join(hash)
}

pub fn write_blob(hash: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
    let dir = object_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(hash);
    std::fs::write(&path, bytes)?;
    let _ = s3::put_object(&object_key(hash), bytes);
    Ok(path)
}

pub fn read_blob(hash: &str) -> std::io::Result<Vec<u8>> {
    match std::fs::read(blob_path(hash)) {
        Ok(b) => Ok(b),
        Err(e) => {
            if let Ok(remote) = s3::get_object(&object_key(hash)) {
                let _ = std::fs::create_dir_all(object_dir());
                let _ = std::fs::write(blob_path(hash), &remote);
                return Ok(remote);
            }
            Err(e)
        }
    }
}

pub fn blob_exists(hash: &str) -> bool {
    Path::new(&blob_path(hash)).exists()
}

pub fn object_key(hash: &str) -> String {
    format!("objects/{hash}")
}

pub fn put(store: &mut Store, bytes: &[u8]) -> (String, String) {
    let hash = sha256_hex(bytes);
    let key = object_key(&hash);
    *store.object_refs.entry(hash.clone()).or_insert(0) += 1;
    store.objects.insert(key.clone(), bytes.to_vec());
    let _ = write_blob(&hash, bytes);
    (hash, key)
}

pub fn add_ref(store: &mut Store, hash: &str) {
    *store.object_refs.entry(hash.to_string()).or_insert(0) += 1;
}

pub fn drop_blob(hash: &str) {
    let _ = std::fs::remove_file(blob_path(hash));
    let _ = s3::delete_object(&object_key(hash));
}

pub fn drop_ref(store: &mut Store, hash: &str) {
    if let Some(rc) = store.object_refs.get_mut(hash) {
        *rc -= 1;
        if *rc <= 0 {
            store.object_refs.remove(hash);
            store.objects.remove(&object_key(hash));
            drop_blob(hash);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refcount_drops_blob() {
        let mut s = Store::default();
        let (hash, key) = put(&mut s, b"abc");
        assert_eq!(s.objects[&key], b"abc");
        add_ref(&mut s, &hash);
        drop_ref(&mut s, &hash);
        assert!(s.objects.contains_key(&key));
        drop_ref(&mut s, &hash);
        assert!(!s.objects.contains_key(&key));
    }
}
