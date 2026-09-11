//! Optional S3/MinIO projection. Local disk remains the write-through cache.

use crate::DeploymentNamespaceV1;
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

fn access_key() -> Result<String, String> {
    std::env::var("KNOWLEDGEBRAIN_S3_ACCESS_KEY")
        .or_else(|_| std::env::var("MINIO_ROOT_USER"))
        .ok()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "S3 access key is required".into())
}

fn secret_key() -> Result<String, String> {
    std::env::var("KNOWLEDGEBRAIN_S3_SECRET_KEY")
        .or_else(|_| std::env::var("MINIO_ROOT_PASSWORD"))
        .ok()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "S3 secret key is required".into())
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

pub(crate) fn reset_preflight(
    namespace: DeploymentNamespaceV1,
    timeout: Duration,
) -> Result<Option<crate::NamespaceResetMinioPreflight>, String> {
    let endpoint = endpoint();
    let bucket = bucket();
    if endpoint.is_empty() && bucket.is_empty() {
        return Ok(None);
    }
    reset_preflight_with_config(
        ConnectionConfig::from_environment_at(endpoint, bucket)?,
        namespace,
        timeout,
    )
    .map(Some)
}

fn reset_preflight_with_config(
    mut config: ConnectionConfig,
    namespace: DeploymentNamespaceV1,
    timeout: Duration,
) -> Result<crate::NamespaceResetMinioPreflight, String> {
    validate_bucket(&config.bucket)?;
    let parsed =
        reqwest::Url::parse(&config.endpoint).map_err(|_| "invalid MinIO reset endpoint")?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.path() != "/"
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || config.region.trim().is_empty()
        || timeout.is_zero()
    {
        return Err("invalid MinIO reset connection mapping or timeout".into());
    }
    config.endpoint = parsed.as_str().trim_end_matches('/').to_owned();
    let observation = crate::NamespaceResetMinioPreflight {
        namespace,
        endpoint: config.endpoint.clone(),
        bucket: config.bucket.clone(),
        signing_region: config.region.clone(),
        prefix: format!("{}/", namespace.storage_label()),
    };
    // HeadBucket authenticates only this bucket. Missing/forbidden is a failure;
    // preflight never lists or creates buckets and never mutates objects.
    with_client(config, timeout, |client, bucket| async move {
        client
            .head_bucket()
            .bucket(bucket)
            .send()
            .await
            .map_err(|error| sdk_error("HEAD bucket", error))?;
        Ok(())
    })?;
    Ok(observation)
}

pub fn put_object(key: &str, bytes: &[u8]) -> Result<(), String> {
    if !configured() {
        return Ok(());
    }
    put_object_in_namespace(
        DeploymentNamespaceV1::from_environment().map_err(|e| e.to_string())?,
        key,
        bytes,
    )
}

pub(crate) fn put_object_in_namespace(
    namespace: DeploymentNamespaceV1,
    key: &str,
    bytes: &[u8],
) -> Result<(), String> {
    if !configured() {
        return Ok(());
    }
    let key = object_key(namespace, key)?;
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
    get_object_in_namespace(
        DeploymentNamespaceV1::from_environment().map_err(|e| e.to_string())?,
        key,
    )
}

pub(crate) fn get_object_in_namespace(
    namespace: DeploymentNamespaceV1,
    key: &str,
) -> Result<Vec<u8>, String> {
    if !configured() {
        return Err("s3 not configured".into());
    }
    let key = object_key(namespace, key)?;
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

pub(crate) fn delete_object_in_namespace(
    namespace: DeploymentNamespaceV1,
    key: &str,
) -> Result<(), String> {
    if !configured() {
        return Ok(());
    }
    let key = object_key(namespace, key)?;
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

fn object_key(namespace: DeploymentNamespaceV1, key: &str) -> Result<String, String> {
    let name = key
        .strip_prefix("objects/")
        .ok_or("logical object reference must start with objects/")?;
    crate::object_store::object_name(name).map_err(|e| e.to_string())?;
    Ok(format!("{}/objects/{}", namespace.storage_label(), name))
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

    // Only fixtures bypass the namespace prefix, to prove legacy isolation.
    fn legacy_object(method: &str, key: &str, bytes: &[u8]) -> Result<Vec<u8>, String> {
        with_client(
            ConnectionConfig::from_environment()?,
            Duration::from_secs(5),
            |client, bucket| async move {
                match method {
                    "PUT" => {
                        client
                            .put_object()
                            .bucket(bucket)
                            .key(key)
                            .body(ByteStream::from(bytes.to_vec()))
                            .send()
                            .await
                            .map_err(|e| sdk_error("fixture PUT", e))?;
                        Ok(Vec::new())
                    }
                    "GET" => {
                        let output = client
                            .get_object()
                            .bucket(bucket)
                            .key(key)
                            .send()
                            .await
                            .map_err(|e| sdk_error("fixture GET", e))?;
                        Ok(output
                            .body
                            .collect()
                            .await
                            .map_err(|e| e.to_string())?
                            .into_bytes()
                            .to_vec())
                    }
                    "DELETE" => {
                        client
                            .delete_object()
                            .bucket(bucket)
                            .key(key)
                            .send()
                            .await
                            .map_err(|e| sdk_error("fixture DELETE", e))?;
                        Ok(Vec::new())
                    }
                    _ => unreachable!("invalid fixture operation"),
                }
            },
        )
    }

    #[test]
    fn reset_mapping_rejects_unsafe_locations_before_connecting() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let config = ConnectionConfig {
            endpoint: endpoint.clone(),
            bucket: "preflight-test".into(),
            access_key: "test-access".into(),
            secret_key: "test-secret".into(),
            region: "test-region".into(),
        };
        let namespace = uuid::Uuid::new_v4().to_string().parse().unwrap();
        let mut cases = Vec::new();
        for location in [
            String::new(),
            format!("{endpoint}/prefix"),
            format!("{endpoint}?parameter=value"),
            format!("{endpoint}#fragment"),
            endpoint.replace("http://", "http://user:password@"),
        ] {
            let mut case = config.clone();
            case.endpoint = location;
            cases.push(case);
        }
        for bucket in ["", ".", "..", "../other", "bucket/other", "bucket?query"] {
            let mut case = config.clone();
            case.bucket = bucket.into();
            cases.push(case);
        }
        let mut blank_region = config.clone();
        blank_region.region = " ".into();
        cases.push(blank_region);
        for case in cases {
            assert!(reset_preflight_with_config(case, namespace, Duration::from_secs(1)).is_err());
        }
        assert!(reset_preflight_with_config(config, namespace, Duration::ZERO).is_err());
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    #[test]
    #[ignore = "requires owned MinIO service and KNOWLEDGEBRAIN_REQUIRE_S3_TESTS=1"]
    fn live_reset_preflight_reads_bucket_without_creating_or_deleting() {
        assert_eq!(
            std::env::var("KNOWLEDGEBRAIN_REQUIRE_S3_TESTS").as_deref(),
            Ok("1")
        );
        let config = ConnectionConfig::from_environment().unwrap();
        assert_eq!(
            reqwest::Url::parse(&config.endpoint).unwrap().host_str(),
            Some("127.0.0.1")
        );
        let target: DeploymentNamespaceV1 = uuid::Uuid::new_v4().to_string().parse().unwrap();
        let foreign = uuid::Uuid::new_v4().to_string().parse().unwrap();
        let key = format!("objects/{}", uuid::Uuid::new_v4());
        put_object_in_namespace(target, &key, b"target-sentinel").unwrap();
        put_object_in_namespace(foreign, &key, b"foreign-sentinel").unwrap();
        let legacy = key.clone();
        legacy_object("PUT", &legacy, b"legacy-sentinel").unwrap();
        let revision = "minio-preflight-test";
        let request = crate::NamespaceResetRequest::new(
            &target.storage_label(),
            revision,
            &crate::namespace_reset_confirmation(target, revision).unwrap(),
        )
        .unwrap();
        let observed = crate::verify_namespace_reset_minio(&request, Duration::from_secs(5))
            .unwrap()
            .unwrap();
        assert_eq!(observed.namespace, target);
        assert_eq!(observed.endpoint, config.endpoint);
        assert_eq!(observed.bucket, config.bucket);
        assert_eq!(observed.signing_region, config.region);
        assert_eq!(observed.prefix, format!("{}/", target.storage_label()));
        assert_eq!(
            reset_preflight_with_config(config.clone(), target, Duration::from_secs(5)).unwrap(),
            observed
        );
        let other =
            reset_preflight_with_config(config.clone(), foreign, Duration::from_secs(5)).unwrap();
        assert_eq!(observed.bucket, other.bucket);
        assert_ne!(observed.prefix, other.prefix);
        let serialized = serde_json::to_string(&observed).unwrap();
        assert!(
            !serialized.contains(&config.access_key) && !serialized.contains(&config.secret_key)
        );

        let mut denied = config.clone();
        denied.secret_key = uuid::Uuid::new_v4().to_string();
        assert!(reset_preflight_with_config(denied, target, Duration::from_secs(5)).is_err());
        let mut missing = config.clone();
        missing.bucket = format!("missing-{}", uuid::Uuid::new_v4().simple());
        assert!(
            reset_preflight_with_config(missing.clone(), target, Duration::from_secs(5)).is_err()
        );
        assert!(
            with_client(
                missing,
                Duration::from_secs(5),
                |client, bucket| async move {
                    client
                        .head_bucket()
                        .bucket(bucket)
                        .send()
                        .await
                        .map_err(|error| sdk_error("HEAD bucket", error))?;
                    Ok(())
                }
            )
            .is_err()
        );

        assert_eq!(
            get_object_in_namespace(target, &key).unwrap(),
            b"target-sentinel"
        );
        assert_eq!(
            get_object_in_namespace(foreign, &key).unwrap(),
            b"foreign-sentinel"
        );
        assert_eq!(
            legacy_object("GET", &legacy, &[]).unwrap(),
            b"legacy-sentinel"
        );
        delete_object_in_namespace(target, &key).unwrap();
        delete_object_in_namespace(foreign, &key).unwrap();
        legacy_object("DELETE", &legacy, &[]).unwrap();
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
        delete_object_in_namespace(DeploymentNamespaceV1::from_environment().unwrap(), &key)
            .expect("delete test object");
    }

    #[test]
    #[ignore = "requires owned MinIO service and KNOWLEDGEBRAIN_REQUIRE_S3_TESTS=1"]
    fn live_namespaces_keep_same_key_isolated_and_never_adopt_legacy_objects() {
        assert_eq!(
            std::env::var("KNOWLEDGEBRAIN_REQUIRE_S3_TESTS").as_deref(),
            Ok("1")
        );
        assert!(
            configured(),
            "required namespace test needs configured owned MinIO"
        );
        let first = uuid::Uuid::new_v4().to_string().parse().unwrap();
        let second = uuid::Uuid::new_v4().to_string().parse().unwrap();
        let key = format!("objects/{}", uuid::Uuid::new_v4());
        put_object_in_namespace(first, &key, b"first").unwrap();
        assert!(get_object_in_namespace(second, &key).is_err());
        put_object_in_namespace(second, &key, b"second").unwrap();
        assert_eq!(get_object_in_namespace(first, &key).unwrap(), b"first");
        assert_eq!(get_object_in_namespace(second, &key).unwrap(), b"second");
        // Deliberately create a legacy object only inside this owned fixture.
        let legacy = key.clone();
        legacy_object("PUT", &legacy, b"legacy").unwrap();
        for invalid in [
            "../escape",
            "/objects/escape",
            "objects/../escape",
            "objects/%2e%2e",
            "objects/a?x",
            "objects/a#x",
            "foreign/object",
        ] {
            assert!(put_object_in_namespace(first, invalid, b"invalid").is_err());
            assert!(get_object_in_namespace(first, invalid).is_err());
            assert!(delete_object_in_namespace(first, invalid).is_err());
        }
        delete_object_in_namespace(first, &key).unwrap();
        assert!(get_object_in_namespace(first, &key).is_err());
        assert_eq!(get_object_in_namespace(second, &key).unwrap(), b"second");
        assert_eq!(legacy_object("GET", &legacy, &[]).unwrap(), b"legacy");
        delete_object_in_namespace(second, &key).unwrap();
        legacy_object("DELETE", &legacy, &[]).unwrap();
    }
}
