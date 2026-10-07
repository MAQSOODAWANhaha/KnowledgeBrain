//! Unix helper subprocesses for immutable object I/O.

pub(crate) const OBJECT_READ_HELPER_ARG: &str = "--kb-object-read-helper-v1";

pub fn run_object_read_helper(arguments: &[String]) -> Result<(), String> {
    use std::io::Write;
    if arguments.len() != 3 || arguments[0] != OBJECT_READ_HELPER_ARG {
        return Err("invalid object read helper arguments".into());
    }
    let digest = &arguments[1];
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err("invalid object read digest".into());
    }
    let max_bytes = arguments[2]
        .parse::<usize>()
        .map_err(|_| "invalid object read byte budget".to_string())?;
    let bytes = platform::read_blob(digest).map_err(|error| error.to_string())?;
    if bytes.is_empty() || bytes.len() > max_bytes {
        return Err("object read helper output exceeds budget".into());
    }
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(&bytes)
        .map_err(|error| error.to_string())?;
    stdout.flush().map_err(|error| error.to_string())
}

pub(crate) const OBJECT_WRITE_HELPER_ARG: &str = "--kb-object-write-helper-v1";

pub fn run_object_write_helper(arguments: &[String]) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    use std::io::{Read, Write};
    if arguments.len() != 3 || arguments[0] != OBJECT_WRITE_HELPER_ARG {
        return Err("invalid object write arguments".into());
    }
    let digest = &arguments[1];
    let length = arguments[2]
        .parse::<usize>()
        .map_err(|_| "invalid object write length")?;
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        || length == 0
        || length == usize::MAX
    {
        return Err("invalid object write identity".into());
    }
    let mut bytes = Vec::new();
    std::io::stdin()
        .lock()
        .take(length as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() != length || hex::encode(Sha256::digest(&bytes)) != *digest {
        return Err("object write digest or length mismatch".into());
    }
    platform::write_blob_off_runtime(digest, &bytes).map_err(|e| e.to_string())?;
    std::io::stdout()
        .lock()
        .write_all(b"ok")
        .map_err(|e| e.to_string())
}
