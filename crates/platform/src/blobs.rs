use crate::object_store::LocalObjectStore;
use std::{io, path::PathBuf};

/// Diagnostic/test locator. Actual I/O uses descriptor-relative operations.
pub fn blob_path(hash: &str) -> io::Result<PathBuf> {
    LocalObjectStore::from_environment()?.path(hash)
}

pub fn object_ref(hash: &str) -> String {
    format!("objects/{hash}")
}

pub fn write_blob(hash: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    let store = LocalObjectStore::from_environment()?;
    let path = store.write(hash, bytes)?;
    crate::s3::put_object_in_namespace(store.namespace, &object_ref(hash), bytes)
        .map_err(io::Error::other)?;
    Ok(path)
}

pub async fn write_blob_async(hash: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    let hash = hash.to_string();
    let bytes = bytes.to_vec();
    tokio::task::spawn_blocking(move || write_blob(&hash, &bytes))
        .await
        .unwrap_or_else(|e| Err(io::Error::other(e)))
}

pub fn write_blob_off_runtime(hash: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    match tokio::runtime::Handle::try_current() {
        Ok(h) if h.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(|| write_blob(hash, bytes))
        }
        _ => write_blob(hash, bytes),
    }
}

pub fn read_blob(hash: &str) -> io::Result<Vec<u8>> {
    let store = LocalObjectStore::from_environment()?;
    match store.read(hash) {
        Ok(bytes) => Ok(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let bytes = crate::s3::get_object_in_namespace(store.namespace, &object_ref(hash))
                .map_err(|_| error)?;
            store.write(hash, &bytes)?;
            Ok(bytes)
        }
        // A path or permission failure must not be bypassed through MinIO.
        Err(error) => Err(error),
    }
}

pub fn blob_exists(hash: &str) -> bool {
    LocalObjectStore::from_environment()
        .and_then(|store| store.exists(hash))
        .unwrap_or(false)
}

pub async fn delete_retained_blob(digest: &str) -> Result<(), String> {
    let digest = digest.to_owned();
    tokio::task::spawn_blocking(move || {
        let store = LocalObjectStore::from_environment().map_err(|e| e.to_string())?;
        match store.remove(&digest) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
        crate::s3::delete_object_in_namespace(store.namespace, &object_ref(&digest))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires owned MinIO, OBJECT_DIR and KNOWLEDGEBRAIN_REQUIRE_S3_TESTS=1"]
    fn live_cache_fallback_and_retention_stay_in_namespace() {
        assert_eq!(
            std::env::var("KNOWLEDGEBRAIN_REQUIRE_S3_TESTS").as_deref(),
            Ok("1")
        );
        assert!(crate::s3_configured());
        let store = LocalObjectStore::from_environment().unwrap();
        let name = uuid::Uuid::new_v4().simple().to_string();
        let bytes = b"namespace cache roundtrip";
        let path = write_blob(&name, bytes).unwrap();
        assert_eq!(crate::get_object(&object_ref(&name)).unwrap(), bytes);
        store.remove(&name).unwrap();
        assert_eq!(read_blob(&name).unwrap(), bytes);
        assert_eq!(store.read(&name).unwrap(), bytes);

        store.remove(&name).unwrap();
        // Even when MinIO has valid bytes, a suspicious local path must fail.
        let outside = path
            .parent()
            .unwrap()
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::write(&outside, b"protected").unwrap();
        std::os::unix::fs::symlink(&outside, &path).unwrap();
        assert!(read_blob(&name).is_err());
        let runtime = tokio::runtime::Runtime::new().unwrap();
        assert!(runtime.block_on(delete_retained_blob(&name)).is_err());
        assert_eq!(crate::get_object(&object_ref(&name)).unwrap(), bytes);
        assert_eq!(std::fs::read(&outside).unwrap(), b"protected");
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_file(&outside).unwrap();
        assert_eq!(read_blob(&name).unwrap(), bytes);
        runtime.block_on(delete_retained_blob(&name)).unwrap();
        assert!(!store.exists(&name).unwrap());
        assert!(crate::get_object(&object_ref(&name)).is_err());
    }
}
