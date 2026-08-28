use std::path::{Path, PathBuf};

pub fn object_dir() -> PathBuf {
    std::env::var("OBJECT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("var/objects"))
}

pub fn blob_path(hash: &str) -> PathBuf {
    object_dir().join(hash)
}

pub fn object_ref(hash: &str) -> String {
    format!("objects/{hash}")
}

pub fn write_blob(hash: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
    let dir = object_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(hash);
    std::fs::write(&path, bytes)?;
    crate::s3::put_object(&object_ref(hash), bytes).map_err(std::io::Error::other)?;
    Ok(path)
}

pub async fn write_blob_async(hash: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
    let hash = hash.to_string();
    let bytes = bytes.to_vec();
    tokio::task::spawn_blocking(move || write_blob(&hash, &bytes))
        .await
        .unwrap_or_else(|e| Err(std::io::Error::other(e)))
}

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
            if let Ok(remote) = crate::s3::get_object(&object_ref(hash)) {
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

pub async fn delete_claimed_blob(digest: &str) -> Result<(), String> {
    let digest = digest.to_owned();
    tokio::task::spawn_blocking(move || {
        match std::fs::remove_file(blob_path(&digest)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
        crate::s3::delete_object(&object_ref(&digest))
    })
    .await
    .map_err(|error| error.to_string())?
}
