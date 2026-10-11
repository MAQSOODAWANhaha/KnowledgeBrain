//! Optional S3/MinIO projection. Local disk remains the write-through cache.

use aws_sdk_s3::{
    Client,
    config::{BehaviorVersion, Credentials, Region, retry::RetryConfig, timeout::TimeoutConfig},
    error::{ProvideErrorMetadata, SdkError},
    primitives::ByteStream,
    types::{BucketLocationConstraint, CreateBucketConfiguration},
};
use std::{future::Future, time::Duration};

pub fn configured() -> bool {
    !bucket().is_empty() && !endpoint().is_empty()
}

pub fn bucket() -> String {
    std::env::var("KNOWLEDGEBRAIN_S3_BUCKET").unwrap_or_default()
}

pub fn endpoint() -> String {
    std::env::var("KNOWLEDGEBRAIN_S3_ENDPOINT").unwrap_or_default()
}

fn credentials(get: &impl Fn(&str) -> Option<String>) -> Result<(String, String), String> {
    let read = |name| get(name).filter(|value| !value.trim().is_empty());
    let key = read("KNOWLEDGEBRAIN_S3_ACCESS_KEY");
    let secret = read("KNOWLEDGEBRAIN_S3_SECRET_KEY");
    match (key, secret) {
        (Some(key), Some(secret)) => Ok((key, secret)),
        (None, None) if read("KNOWLEDGEBRAIN_S3_ENDPOINT").is_some_and(|endpoint|
            endpoint.trim().trim_end_matches('/') == "http://minio:9000") => {
            Ok((read("MINIO_ROOT_USER").ok_or("MINIO_ROOT_USER is required for built-in storage")?,
                read("MINIO_ROOT_PASSWORD").ok_or("MINIO_ROOT_PASSWORD is required for built-in storage")?))
        }
        _ => Err("KNOWLEDGEBRAIN_S3_ACCESS_KEY and KNOWLEDGEBRAIN_S3_SECRET_KEY must be provided together for external storage".into()),
    }
}
fn access_key() -> Result<String, String> {
    credentials(&|key| std::env::var(key).ok()).map(|pair| pair.0)
}
fn secret_key() -> Result<String, String> {
    credentials(&|key| std::env::var(key).ok()).map(|pair| pair.1)
}

fn region() -> String {
    std::env::var("KNOWLEDGEBRAIN_S3_REGION").unwrap_or_else(|_| "us-east-1".into())
}

// One request must use the same location/credentials/region throughout signing
// and sending. This private snapshot is never serialized or formatted for logs.
#[derive(Clone)]
struct ConnectionConfig {
    endpoint: String,
    bucket: String,
    access_key: String,
    secret_key: String,
    region: String,
}

impl ConnectionConfig {
    fn from_environment() -> Result<Self, String> {
        Self::from_environment_at(endpoint(), bucket())
    }

    fn from_environment_at(endpoint: String, bucket: String) -> Result<Self, String> {
        Ok(Self {
            endpoint,
            bucket,
            access_key: access_key()?,
            secret_key: secret_key()?,
            region: region(),
        })
    }
}

pub fn put_object(key: &str, bytes: &[u8]) -> Result<(), String> {
    if !configured() {
        return Ok(());
    }
    let key = object_key(key)?;
    let bytes = bytes.to_vec();
    with_client(
        ConnectionConfig::from_environment()?,
        Duration::from_secs(30),
        |client, bucket| async move {
            ensure_bucket(&client, &bucket).await?;
            client
                .put_object()
                .bucket(bucket)
                .key(key)
                .body(ByteStream::from(bytes))
                .send()
                .await
                .map_err(|error| sdk_error("PUT object", error))?;
            Ok(())
        },
    )
}

pub fn get_object(key: &str) -> Result<Vec<u8>, String> {
    if !configured() {
        return Err("s3 not configured".into());
    }
    let key = object_key(key)?;
    with_client(
        ConnectionConfig::from_environment()?,
        Duration::from_secs(30),
        |client, bucket| async move {
            let response = client
                .get_object()
                .bucket(bucket)
                .key(key)
                .send()
                .await
                .map_err(|error| sdk_error("GET object", error))?;
            response
                .body
                .collect()
                .await
                .map(|body| body.into_bytes().to_vec())
                .map_err(|error| format!("S3 object body: {error}"))
        },
    )
}

pub(crate) fn delete_object(key: &str) -> Result<(), String> {
    if !configured() {
        return Ok(());
    }
    let key = object_key(key)?;
    with_client(
        ConnectionConfig::from_environment()?,
        Duration::from_secs(30),
        |client, bucket| async move {
            client
                .delete_object()
                .bucket(bucket)
                .key(key)
                .send()
                .await
                .map_err(|error| sdk_error("DELETE object", error))?;
            Ok(())
        },
    )
}

fn object_key(key: &str) -> Result<String, String> {
    let name = key
        .strip_prefix("objects/")
        .ok_or("logical object reference must start with objects/")?;
    crate::object_store::object_name(name).map_err(|e| e.to_string())?;
    Ok(format!("objects/{name}"))
}

fn validate_bucket(bucket: &str) -> Result<(), String> {
    if bucket.is_empty()
        || !bucket
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'.'))
        || matches!(bucket, "." | "..")
    {
        return Err("invalid configured S3 bucket".into());
    }
    Ok(())
}

async fn ensure_bucket(client: &Client, bucket: &str) -> Result<(), String> {
    match client.head_bucket().bucket(bucket).send().await {
        Ok(_) => return Ok(()),
        // Authentication, transport and service failures must not trigger writes.
        Err(error)
            if error
                .raw_response()
                .is_some_and(|response| response.status().as_u16() == 404) => {}
        Err(error) => return Err(sdk_error("HEAD bucket", error)),
    }
    let mut create = client.create_bucket().bucket(bucket);
    // The S3 protocol omits LocationConstraint for its default region.
    if let Some(region) = client
        .config()
        .region()
        .filter(|region| region.as_ref() != "us-east-1")
    {
        create = create.create_bucket_configuration(
            CreateBucketConfiguration::builder()
                .location_constraint(BucketLocationConstraint::from(region.as_ref()))
                .build(),
        );
    }
    match create.send().await {
        Ok(_) => Ok(()),
        // Another writer may have created our bucket after the initial HEAD.
        Err(error)
            if error
                .as_service_error()
                .is_some_and(|error| error.is_bucket_already_owned_by_you()) =>
        {
            Ok(())
        }
        Err(error) => Err(sdk_error("CREATE bucket", error)),
    }
}

fn sdk_error<E: ProvideErrorMetadata>(operation: &str, error: SdkError<E>) -> String {
    // Service bodies can echo credentials or request details; expose only the
    // protocol status and service error code, never a raw SDK debug dump.
    let status = error
        .raw_response()
        .map(|response| response.status().as_u16());
    let code = error.as_service_error().and_then(|error| error.code());
    format!("S3 {operation} failed (status={status:?}, code={code:?})")
}

fn with_client<T: Send, F, Fut>(
    config: ConnectionConfig,
    timeout: Duration,
    operation: F,
) -> Result<T, String>
where
    F: FnOnce(Client, String) -> Fut + Send,
    Fut: Future<Output = Result<T, String>>,
{
    validate_bucket(&config.bucket)?;
    // Existing blob APIs are synchronous and may also be invoked from a Tokio
    // runtime. Keep the SDK runtime on a scoped thread to avoid nested block_on.
    std::thread::scope(|scope| {
        scope
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|error| format!("S3 runtime: {error}"))?;
                runtime.block_on(async move {
                    let sdk_config = aws_sdk_s3::Config::builder()
                        .behavior_version(BehaviorVersion::latest())
                        .endpoint_url(config.endpoint)
                        .region(Region::new(config.region))
                        .credentials_provider(Credentials::new(
                            config.access_key,
                            config.secret_key,
                            None,
                            None,
                            "KnowledgeBrain configuration",
                        ))
                        .force_path_style(true)
                        .retry_config(RetryConfig::standard().with_max_attempts(1))
                        .timeout_config(TimeoutConfig::builder().operation_timeout(timeout).build())
                        .build();
                    // Also bound body collection after GET response headers arrive.
                    tokio::time::timeout(
                        timeout,
                        operation(Client::from_conf(sdk_config), config.bucket),
                    )
                    .await
                    .map_err(|_| "S3 operation timed out".to_string())?
                })
            })
            .join()
            .map_err(|_| "S3 SDK thread panicked".to_string())?
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_credentials_never_cross_external_endpoints() {
        let mut values = std::collections::HashMap::from([
            ("KNOWLEDGEBRAIN_S3_ENDPOINT", "http://minio:9000"),
            ("MINIO_ROOT_USER", "internal-user"),
            ("MINIO_ROOT_PASSWORD", " exact-secret "),
        ]);
        assert_eq!(
            credentials(&|key| values.get(key).map(|v| v.to_string()))
                .unwrap()
                .1,
            " exact-secret "
        );
        values.insert("KNOWLEDGEBRAIN_S3_ENDPOINT", "https://external.invalid");
        assert!(credentials(&|key| values.get(key).map(|v| v.to_string())).is_err());
        values.insert("KNOWLEDGEBRAIN_S3_ACCESS_KEY", "external-user");
        assert!(credentials(&|key| values.get(key).map(|v| v.to_string())).is_err());
        values.insert("KNOWLEDGEBRAIN_S3_SECRET_KEY", "external-secret");
        assert_eq!(
            credentials(&|key| values.get(key).map(|v| v.to_string()))
                .unwrap()
                .1,
            "external-secret"
        );
    }

    #[test]
    fn failed_bucket_probe_does_not_create_or_follow_redirects() {
        use std::io::{Read, Write};
        for status in [403, 500, 307] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            let server_listener = listener.try_clone().unwrap();
            let response = format!(
                "HTTP/1.1 {status} Rejected\r\nContent-Length: 0\r\nConnection: close\r\nLocation: {endpoint}/redirected\r\n\r\n"
            );
            let server = std::thread::spawn(move || {
                let started = std::time::Instant::now();
                let mut stream = loop {
                    match server_listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(started.elapsed() < Duration::from_secs(5));
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        Err(error) => panic!("test listener: {error}"),
                    }
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut headers = Vec::new();
                while !headers.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    headers.push(byte[0]);
                }
                assert!(headers.starts_with(b"HEAD /probe-bucket"));
                stream.write_all(response.as_bytes()).unwrap();
            });
            let result = with_client(
                ConnectionConfig {
                    endpoint,
                    bucket: "probe-bucket".into(),
                    access_key: "test-access".into(),
                    secret_key: "test-secret".into(),
                    region: "us-east-1".into(),
                },
                Duration::from_secs(2),
                |client, bucket| async move { ensure_bucket(&client, &bucket).await },
            );
            server.join().unwrap();
            assert!(
                result
                    .unwrap_err()
                    .contains(&format!("status=Some({status})"))
            );
            assert_eq!(
                listener.accept().unwrap_err().kind(),
                std::io::ErrorKind::WouldBlock
            );
        }
    }

    #[test]
    fn object_keys_use_objects_prefix_and_reject_unsafe_names() {
        assert_eq!(
            object_key("objects/example.md").unwrap(),
            "objects/example.md"
        );
        for invalid in [
            "",
            "../escape",
            "/objects/escape",
            "objects/../escape",
            "objects/%2e%2e",
            "objects/a?x",
            "objects/a#x",
            "foreign/object",
        ] {
            assert!(object_key(invalid).is_err());
        }
    }

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
            if std::env::var("KNOWLEDGEBRAIN_REQUIRE_S3_TESTS").as_deref() == Ok("1") {
                panic!("KNOWLEDGEBRAIN_REQUIRE_S3_TESTS=1 requires S3 configuration");
            }
            eprintln!("skip: s3 not configured");
            return;
        }
        let key = format!("objects/kb-test-{}", uuid::Uuid::new_v4());
        put_object(&key, b"knowledgebrain-s3").expect("s3 put");
        let got = get_object(&key).expect("s3 get");
        assert_eq!(got, b"knowledgebrain-s3");
        delete_object(&key).expect("delete test object");
    }
}
