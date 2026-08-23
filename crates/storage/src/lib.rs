//! Object bytes keyed as `objects/{sha256}`. Postgres migrate + workspace seed.

pub mod bid;
pub mod bid_extract_publication;
pub mod bid_matching;
pub mod bidding;
mod first_launch;
pub mod knowledge_retrieval;
mod object_registry;
mod persist;
mod probe;
mod s3;

pub use first_launch::{
    FirstLaunchVerificationError, FreshPretrafficOwnerBindings, VerifiedCatalogRows,
    require_production_first_launch_verified, verify_fresh_pretraffic_catalog_rows,
};
pub use object_registry::{
    RetentionClaim, process_one_retention_item, register_knowledge_document_object,
    release_knowledge_document_object,
};
pub use persist::*;
pub use probe::*;
pub use s3::{configured as s3_configured, get_object, put_object};

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

/// Disk write plus a blocking S3 PUT. Must not run on a tokio worker thread
/// (`reqwest::blocking` deadlocks there). Use [`write_blob_async`] from async code.
pub fn write_blob(hash: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
    let dir = object_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(hash);
    std::fs::write(&path, bytes)?;
    s3::put_object(&object_ref(hash), bytes).map_err(std::io::Error::other)?;
    Ok(path)
}

pub async fn write_blob_async(hash: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
    let hash = hash.to_string();
    let bytes = bytes.to_vec();
    tokio::task::spawn_blocking(move || write_blob(&hash, &bytes))
        .await
        .unwrap_or_else(|e| Err(std::io::Error::other(e)))
}

/// Sync write that parks the tokio worker instead of deadlocking `reqwest::blocking`.
pub fn write_blob_off_runtime(hash: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
    match tokio::runtime::Handle::try_current() {
        Ok(h) if h.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(|| write_blob(hash, bytes))
        }
        _ => write_blob(hash, bytes),
    }
}

pub fn read_blob(hash: &str) -> std::io::Result<Vec<u8>> {
    match std::fs::read(blob_path(hash)) {
        Ok(b) => Ok(b),
        Err(e) => {
            if let Ok(remote) = s3::get_object(&object_ref(hash)) {
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

pub fn object_ref(hash: &str) -> String {
    format!("objects/{hash}")
}

pub fn put(store: &mut Store, bytes: &[u8]) -> (String, String) {
    let hash = sha256_hex(bytes);
    let reference = object_ref(&hash);
    store.objects.insert(reference.clone(), bytes.to_vec());
    let _ = write_blob_off_runtime(&hash, bytes);
    (hash, reference)
}

/// Remove an upload from the in-memory request model after its database
/// transaction failed. Durable object deletion is retention-only.
pub fn discard_unpersisted_object(store: &mut Store, hash: &str) {
    store.objects.remove(&object_ref(hash));
}

async fn delete_claimed_blob(digest: &str) -> Result<(), String> {
    let digest = digest.to_owned();
    tokio::task::spawn_blocking(move || {
        match std::fs::remove_file(blob_path(&digest)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
        s3::delete_object(&object_ref(&digest))
    })
    .await
    .map_err(|error| error.to_string())?
}
