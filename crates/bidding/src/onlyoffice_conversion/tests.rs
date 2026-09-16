use super::*;
use std::collections::BTreeMap;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn config() -> Config {
    configured(&[]).unwrap()
}

fn configured(overrides: &[(&str, Option<&str>)]) -> Result<Config, Error> {
    let mut values = BTreeMap::from([
        ("KB_ONLYOFFICE_SERVER_ORIGIN", "http://office.example:8080"),
        ("KB_ONLYOFFICE_API_ORIGIN", "http://api.example:3000"),
        ("KB_ONLYOFFICE_JWT_SECRET", "test-service-secret"),
        ("KB_ONLYOFFICE_CAPABILITY_SECRET", "test-capability-secret"),
        ("KB_ONLYOFFICE_TOKEN_TTL_SECONDS", "600"),
        ("KB_ONLYOFFICE_HTTP_TIMEOUT_SECONDS", "5"),
    ]);
    for (name, value) in overrides {
        match value {
            Some(value) => {
                values.insert(name, value);
            }
            None => {
                values.remove(name);
            }
        }
    }
    Config::from_values(|name| values.get(name).map(|s| s.to_string()))
}

fn source() -> FrozenDocxSource {
    FrozenDocxSource {
        workspace_id: Uuid::new_v4(),
        request_id: Uuid::new_v4(),
        version_id: Uuid::new_v4(),
        docx_sha256: platform::sha256_hex(b"a frozen DOCX identity"),
        byte_length: 1234,
    }
}

#[test]
fn shared_configuration_preserves_editor_trust_boundaries() {
    assert_eq!(config().server, config().command);
    for bad in [
        "https://user:pass@office.example",
        "file:///tmp/office",
        "https://office.example/path",
        "https://office.example/?secret=bad",
        "https://office.example/#fragment",
    ] {
        assert!(configured(&[("KB_ONLYOFFICE_SERVER_ORIGIN", Some(bad))]).is_err());
    }
    assert!(configured(&[("KB_ONLYOFFICE_JWT_SECRET", None)]).is_err());
    assert!(
        configured(&[(
            "KB_ONLYOFFICE_CAPABILITY_SECRET",
            Some("test-service-secret")
        )])
        .is_err()
    );
    assert!(configured(&[("KB_ONLYOFFICE_HTTP_TIMEOUT_SECONDS", Some("0"))]).is_err());
    assert!(
        configured(&[(
            "KB_ONLYOFFICE_TOKEN_TTL_SECONDS",
            Some("18446744073709551615")
        )])
        .unwrap()
        .expiration()
        .is_err()
    );
}

#[test]
fn cache_download_is_scoped_and_rewrites_only_to_configured_command_origin() {
    let cfg = configured(&[(
        "KB_ONLYOFFICE_COMMAND_ORIGIN",
        Some("http://office-internal:8000"),
    )])
    .unwrap();
    assert_eq!(
        cfg.download_url("http://office.example:8080/cache/files/id/out.pdf?md5=signed")
            .unwrap()
            .as_str(),
        "http://office-internal:8000/cache/files/id/out.pdf?md5=signed"
    );
    for bad in [
        "http://untrusted.example/cache/files/x",
        "http://office.example:8080/private/x",
        "http://secret@office.example:8080/cache/files/x",
        "http://office.example:8080/cache/files/x#fragment",
    ] {
        let error = cfg.download_url(bad).unwrap_err();
        assert_eq!(error.code, "ONLYOFFICE_DOWNLOAD_SCOPE");
        assert!(!error.to_string().contains(bad));
    }
}

#[test]
fn conversion_capability_binds_frozen_request_and_has_independent_signature() {
    let cfg = config();
    let frozen = source();
    let url = cfg.source_url(&frozen).unwrap();
    assert_eq!(
        url.path(),
        format!(
            "/api/v2/submission-workspaces/{}/exports/requests/{}/source",
            frozen.workspace_id, frozen.request_id
        )
    );
    let token = url
        .query_pairs()
        .find(|(key, _)| key == "token")
        .unwrap()
        .1
        .into_owned();
    let mut claim = cfg.verify_source_capability(&token).unwrap();
    assert_eq!(claim.source, frozen);
    assert!(claim.exp > chrono::Utc::now().timestamp() as u64);
    assert_eq!(
        cfg.verify_source_capability(&cfg.sign(&claim, false).unwrap())
            .err()
            .unwrap()
            .kind,
        ErrorKind::Unauthorized
    );
    claim.aud = "docx-source".into();
    assert!(
        cfg.verify_source_capability(&cfg.sign(&claim, true).unwrap())
            .is_err()
    );
    claim.aud = SOURCE_AUDIENCE.into();
    claim.exp = 1;
    assert!(
        cfg.verify_source_capability(&cfg.sign(&claim, true).unwrap())
            .is_err()
    );
    let mut invalid = frozen;
    invalid.docx_sha256 = "not-a-sha".into();
    assert!(cfg.source_url(&invalid).is_err());
}

fn pdf() -> Vec<u8> {
    use lopdf::{Document, Object, dictionary};
    let mut doc = Document::with_version("1.5");
    let pages = doc.new_object_id();
    let page = doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages,
    "MediaBox" => vec![Object::from(0), Object::from(0), Object::from(100), Object::from(100)] });
    doc.objects.insert(
        pages,
        dictionary! { "Type" => "Pages", "Kids" => vec![Object::Reference(page)], "Count" => 1 }
            .into(),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    bytes
}

struct Reply {
    status: u16,
    headers: Vec<(&'static str, String)>,
    body: Vec<u8>,
}
impl Reply {
    fn body(body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 200,
            headers: vec![],
            body: body.into(),
        }
    }
}

async fn server(
    replies: impl FnOnce(&str) -> Vec<Reply>,
) -> (Config, tokio::task::JoinHandle<Vec<(String, Vec<u8>)>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let mut cfg = config();
    cfg.server = Url::parse(&origin).unwrap();
    cfg.command = cfg.server.clone();
    let replies = replies(&origin);
    let task = tokio::spawn(async move {
        let mut requests = vec![];
        for reply in replies {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut received = vec![];
            let head_end = loop {
                let mut buffer = [0; 4096];
                let count = stream.read(&mut buffer).await.unwrap();
                assert!(count > 0);
                received.extend_from_slice(&buffer[..count]);
                if let Some(at) = received.windows(4).position(|x| x == b"\r\n\r\n") {
                    break at + 4;
                }
            };
            let headers = String::from_utf8(received[..head_end].to_vec()).unwrap();
            let length = headers
                .lines()
                .find_map(|line| {
                    let (key, value) = line.split_once(':')?;
                    key.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            while received.len() < head_end + length {
                let mut buffer = [0; 4096];
                let count = stream.read(&mut buffer).await.unwrap();
                assert!(count > 0);
                received.extend_from_slice(&buffer[..count]);
            }
            requests.push((
                headers.lines().next().unwrap().to_owned(),
                received[head_end..].to_vec(),
            ));
            let extra = reply
                .headers
                .iter()
                .map(|(k, v)| format!("{k}: {v}\r\n"))
                .collect::<String>();
            let response = format!(
                "HTTP/1.1 {} Result\r\nContent-Length: {}\r\nConnection: close\r\n{}\r\n",
                reply.status,
                reply.body.len(),
                extra
            );
            stream.write_all(response.as_bytes()).await.unwrap();
            stream.write_all(&reply.body).await.unwrap();
        }
        requests
    });
    (cfg, task)
}

async fn requests(task: tokio::task::JoinHandle<Vec<(String, Vec<u8>)>>) -> Vec<(String, Vec<u8>)> {
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn completed_conversion_signs_exact_request_and_returns_original_binding() {
    let expected = pdf();
    let (cfg, task) = server(|origin| vec![
        Reply::body(json!({"endConvert":true,"fileUrl":format!("{origin}/cache/files/request/out.pdf")}).to_string()),
        Reply::body(expected.clone()),
    ]).await;
    let frozen = source();
    let converted = convert_pdf(&cfg, &frozen, expected.len(), &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(converted.source, frozen);
    assert_eq!(converted.bytes, expected);
    assert_eq!(converted.pdf_sha256, platform::sha256_hex(&expected));
    let sent = requests(task).await;
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0].0, "POST /converter HTTP/1.1");
    assert_eq!(sent[1].0, "GET /cache/files/request/out.pdf HTTP/1.1");
    let mut body: Value = serde_json::from_slice(&sent[0].1).unwrap();
    let token = body.as_object_mut().unwrap().remove("token").unwrap();
    let mut validation = Validation::new(Algorithm::HS256);
    validation.required_spec_claims.clear();
    validation.validate_exp = false;
    let signed = decode::<Value>(
        token.as_str().unwrap(),
        &DecodingKey::from_secret(cfg.service_secret().as_bytes()),
        &validation,
    )
    .unwrap()
    .claims;
    assert_eq!(signed, body);
    assert_eq!(body["async"], false);
    assert_eq!(body["filetype"], "docx");
    assert_eq!(body["outputtype"], "pdf");
    assert_eq!(body["key"], conversion_body(&cfg, &frozen).unwrap()["key"]);
    let mut other = frozen;
    other.request_id = Uuid::new_v4();
    assert_ne!(body["key"], conversion_body(&cfg, &other).unwrap()["key"]);
}

#[tokio::test]
async fn incomplete_error_and_foreign_downloads_do_not_fetch_or_retry() {
    for (response, expected) in [
        (
            json!({"endConvert":false,"percent":50}),
            "ONLYOFFICE_CONVERSION_INCOMPLETE",
        ),
        (json!({"error":-3}), "ONLYOFFICE_CONVERSION_REJECTED"),
        (
            json!({"endConvert":true,"fileUrl":"http://untrusted.example/cache/files/out.pdf"}),
            "ONLYOFFICE_DOWNLOAD_SCOPE",
        ),
    ] {
        let (cfg, task) = server(|_| vec![Reply::body(response.to_string())]).await;
        assert_eq!(
            convert_pdf(&cfg, &source(), 1024, &CancellationToken::new())
                .await
                .err()
                .unwrap()
                .code,
            expected
        );
        assert_eq!(requests(task).await.len(), 1);
    }
}

#[tokio::test]
async fn redirects_oversize_and_invalid_pdf_cannot_be_published() {
    let (cfg, task) = server(|origin| {
        vec![Reply {
            status: 302,
            headers: vec![("Location", format!("{origin}/private"))],
            body: vec![],
        }]
    })
    .await;
    assert_eq!(
        convert_pdf(&cfg, &source(), 1024, &CancellationToken::new())
            .await
            .err()
            .unwrap()
            .code,
        "ONLYOFFICE_CONVERSION_FAILED"
    );
    assert_eq!(requests(task).await.len(), 1);
    for (bytes, limit, expected) in [
        (pdf(), 16, "document service response too large"),
        (
            b"%PDF-1.5\nnot-a-document\n%%EOF".to_vec(),
            1024,
            "document conversion did not produce a valid PDF",
        ),
    ] {
        let (cfg, task) = server(|origin| {
            vec![
                Reply::body(
                    json!({"endConvert":true,"fileUrl":format!("{origin}/cache/files/out.pdf")})
                        .to_string(),
                ),
                Reply::body(bytes),
            ]
        })
        .await;
        assert_eq!(
            convert_pdf(&cfg, &source(), limit, &CancellationToken::new())
                .await
                .err()
                .unwrap()
                .message,
            expected
        );
        assert_eq!(requests(task).await.len(), 2);
    }
}

#[tokio::test]
async fn cancelled_conversion_does_not_send_a_request() {
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert_eq!(
        convert_pdf(&config(), &source(), 1024, &cancelled)
            .await
            .err()
            .unwrap()
            .code,
        "ONLYOFFICE_CANCELLED"
    );
}

#[tokio::test]
async fn shared_download_bound_also_applies_without_content_length() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let sent = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buffer = [0; 4096];
        assert!(stream.read(&mut buffer).await.unwrap() > 0);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n3\r\nabc\r\n3\r\ndef\r\n0\r\n\r\n")
            .await
            .unwrap();
    });
    let response = config().client().unwrap().get(origin).send().await.unwrap();
    assert_eq!(response.content_length(), None);
    assert_eq!(
        bounded_body(response, 5).await.unwrap_err().message,
        "document service response too large"
    );
    sent.await.unwrap();
}

#[tokio::test]
async fn cancelling_an_in_flight_conversion_returns_no_publishable_output() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let cfg = configured(&[("KB_ONLYOFFICE_SERVER_ORIGIN", Some(&origin))]).unwrap();
    let (arrived, request_arrived) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut byte = [0];
        stream.read_exact(&mut byte).await.unwrap();
        arrived.send(()).unwrap();
        // Hold the response open; cancellation must not wait for this server.
        std::future::pending::<()>().await;
        drop(stream);
    });
    let cancel = CancellationToken::new();
    let task_cancel = cancel.clone();
    let conversion =
        tokio::spawn(async move { convert_pdf(&cfg, &source(), 1024, &task_cancel).await });
    tokio::time::timeout(Duration::from_secs(2), request_arrived)
        .await
        .unwrap()
        .unwrap();
    cancel.cancel();
    let result = tokio::time::timeout(Duration::from_secs(1), conversion)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.err().unwrap().code, "ONLYOFFICE_CANCELLED");
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
}
