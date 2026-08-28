//! Optional S3/MinIO projection. Local disk remains the write-through cache.

use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

pub fn configured() -> bool {
    !bucket().is_empty() && !endpoint().is_empty()
}

pub fn bucket() -> String {
    std::env::var("KNOWLEDGEBRAIN_S3_BUCKET").unwrap_or_default()
}

pub fn endpoint() -> String {
    std::env::var("KNOWLEDGEBRAIN_S3_ENDPOINT").unwrap_or_default()
}

fn access_key() -> String {
    std::env::var("KNOWLEDGEBRAIN_S3_ACCESS_KEY")
        .or_else(|_| std::env::var("MINIO_ROOT_USER"))
        .unwrap_or_else(|_| "minioadmin".into())
}

fn secret_key() -> String {
    std::env::var("KNOWLEDGEBRAIN_S3_SECRET_KEY")
        .or_else(|_| std::env::var("MINIO_ROOT_PASSWORD"))
        .unwrap_or_else(|_| "minioadmin".into())
}

fn region() -> String {
    std::env::var("KNOWLEDGEBRAIN_S3_REGION").unwrap_or_else(|_| "us-east-1".into())
}

pub fn put_object(key: &str, bytes: &[u8]) -> Result<(), String> {
    if !configured() {
        return Ok(());
    }
    ensure_bucket()?;
    signed_send("PUT", &object_path(key), bytes, true)?;
    Ok(())
}

pub fn get_object(key: &str) -> Result<Vec<u8>, String> {
    if !configured() {
        return Err("s3 not configured".into());
    }
    signed_send("GET", &object_path(key), &[], false)
}

pub(crate) fn delete_object(key: &str) -> Result<(), String> {
    if !configured() {
        return Ok(());
    }
    signed_send("DELETE", &object_path(key), &[], false).map(|_| ())
}

fn object_path(key: &str) -> String {
    let k = key.trim_start_matches('/');
    format!("/{}/{}", bucket(), k)
}

fn ensure_bucket() -> Result<(), String> {
    let path = format!("/{}", bucket());
    let head = signed_send("HEAD", &path, &[], false);
    if head.is_ok() {
        return Ok(());
    }
    signed_send("PUT", &path, &[], true)?;
    Ok(())
}

fn signed_send(method: &str, path: &str, body: &[u8], send_body: bool) -> Result<Vec<u8>, String> {
    let method = method.to_string();
    let path = path.to_string();
    let body = body.to_vec();
    std::thread::spawn(move || signed_send_blocking(&method, &path, &body, send_body))
        .join()
        .map_err(|_| "s3 request thread panicked".to_string())?
}

fn signed_send_blocking(
    method: &str,
    path: &str,
    body: &[u8],
    send_body: bool,
) -> Result<Vec<u8>, String> {
    let host = host_header();
    let now = chrono::Utc::now();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let date_stamp = now.format("%Y%m%d").to_string();
    let payload_hash = hex::encode(Sha256::digest(body));
    let signed_headers = "host;x-amz-content-sha256;x-amz-date";
    let canonical = format!(
        "{method}\n{path}\n\nhost:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\n\n{signed_headers}\n{payload_hash}"
    );
    let scope = format!("{date_stamp}/{}/s3/aws4_request", region());
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        hex::encode(Sha256::digest(canonical.as_bytes()))
    );
    let signing_key = aws_sign_key(&date_stamp)?;
    let signature = hex::encode(hmac_sha256(&signing_key, string_to_sign.as_bytes()));
    let auth = format!(
        "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
        access_key()
    );
    let url = format!("{}{path}", endpoint().trim_end_matches('/'));
    // Dedicated blocking client: never call this on a tokio worker (deadlock).
    // A plaintext MinIO endpoint needs no trust roots. Avoid initializing the
    // platform certificate verifier in minimal runtime images that do not ship
    // a CA bundle; HTTPS endpoints continue to use the platform verifier.
    let mut client_builder =
        reqwest::blocking::Client::builder().timeout(std::time::Duration::from_secs(30));
    if endpoint().starts_with("http://") {
        client_builder = client_builder.tls_certs_only(std::iter::empty::<reqwest::Certificate>());
    }
    let mut req = client_builder
        .build()
        .map_err(|error| format!("{error:?}"))?
        .request(
            method.parse().map_err(|_| format!("bad method {method}"))?,
            url,
        )
        .header("host", host)
        .header("x-amz-date", amz_date)
        .header("x-amz-content-sha256", payload_hash)
        .header("authorization", auth);
    if send_body && method != "GET" && method != "HEAD" && method != "DELETE" {
        req = req.body(body.to_vec());
    }
    let resp = req.send().map_err(|error| format!("{error:?}"))?;
    let status = resp.status();
    let bytes = resp.bytes().map_err(|error| format!("{error:?}"))?.to_vec();
    if !status.is_success() {
        return Err(format!("s3 {method} {path} -> {status}"));
    }
    Ok(bytes)
}

fn host_header() -> String {
    let ep = endpoint();
    ep.trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_string()
}

fn aws_sign_key(date_stamp: &str) -> Result<Vec<u8>, String> {
    let k_date = hmac_sha256(
        format!("AWS4{}", secret_key()).as_bytes(),
        date_stamp.as_bytes(),
    );
    let k_region = hmac_sha256(&k_date, region().as_bytes());
    let k_service = hmac_sha256(&k_region, b"s3");
    Ok(hmac_sha256(&k_service, b"aws4_request"))
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("hmac key");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unconfigured_put_is_ok() {
        if configured() {
            return;
        }
        assert!(put_object("objects/x", b"hi").is_ok());
    }

    #[test]
    fn live_put_get_roundtrip() {
        if !configured() {
            eprintln!("skip: s3 not configured");
            return;
        }
        let key = format!("objects/kb-test-{}", uuid::Uuid::new_v4());
        put_object(&key, b"knowledgebrain-s3").expect("s3 put");
        let got = get_object(&key).expect("s3 get");
        assert_eq!(got, b"knowledgebrain-s3");
        let _ = delete_object(&key);
    }
}
