//! MinerU / PaddleOCR-VL HTTP convert (spec §5.2).

use crate::engines::EffectiveEngineConfig;
use crate::{ConvertError, ReadResult};
use tokio_util::sync::CancellationToken;

async fn wait_cancel<T>(
    cancel: &CancellationToken,
    fut: impl std::future::Future<Output = Result<T, ConvertError>>,
) -> Result<T, ConvertError> {
    tokio::select! {
        biased;
        () = cancel.cancelled() => Err(ConvertError("cancelled".into())),
        result = fut => result,
    }
}

pub async fn convert_http(
    config: &EffectiveEngineConfig,
    file_name: &str,
    bytes: Vec<u8>,
    cancel: &CancellationToken,
) -> Result<ReadResult, ConvertError> {
    if cancel.is_cancelled() {
        return Err(ConvertError("cancelled".into()));
    }
    match config.engine.as_str() {
        "mineru" => mineru_parse(config, file_name, &bytes, cancel).await,
        "paddleocr_vl" => paddle_parse(config, file_name, &bytes, cancel).await,
        _ => Err(ConvertError("unsupported HTTP parser engine".into())),
    }
}

async fn mineru_parse(
    config: &EffectiveEngineConfig,
    file_name: &str,
    bytes: &[u8],
    cancel: &CancellationToken,
) -> Result<ReadResult, ConvertError> {
    let base = &config.endpoint;
    let url = format!("{}/file_parse", base.trim_end_matches('/'));
    let part = reqwest::multipart::Part::bytes(bytes.to_vec())
        .file_name(file_name.to_string())
        .mime_str("application/octet-stream")
        .map_err(|e| ConvertError(e.to_string()))?;
    let form = reqwest::multipart::Form::new().part("files", part);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30 * 60))
        .build()
        .map_err(|e| ConvertError(e.to_string()))?;
    let resp = wait_cancel(cancel, async {
        authenticate(client.post(url), config)
            .multipart(form)
            .send()
            .await
            .map_err(|_| ConvertError("parser HTTP request failed".into()))
    })
    .await?;
    if !resp.status().is_success() {
        return Err(ConvertError(format!("mineru {}", resp.status())));
    }
    let v: serde_json::Value = wait_cancel(cancel, async {
        resp.json()
            .await
            .map_err(|_| ConvertError("parser HTTP request failed".into()))
    })
    .await?;
    Ok(ReadResult {
        markdown: extract_markdown(&v),
        ..ReadResult::default()
    })
}

async fn paddle_parse(
    config: &EffectiveEngineConfig,
    file_name: &str,
    bytes: &[u8],
    cancel: &CancellationToken,
) -> Result<ReadResult, ConvertError> {
    let base = &config.endpoint;
    let url = format!("{}/layout-parsing", base.trim_end_matches('/'));
    let body = serde_json::json!({
        "file": data_encoding_base64(bytes),
        "fileName": file_name,
    });
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30 * 60))
        .build()
        .map_err(|e| ConvertError(e.to_string()))?;
    let resp = wait_cancel(cancel, async {
        authenticate(client.post(url), config)
            .json(&body)
            .send()
            .await
            .map_err(|_| ConvertError("parser HTTP request failed".into()))
    })
    .await?;
    if !resp.status().is_success() {
        return Err(ConvertError(format!("paddle {}", resp.status())));
    }
    let v: serde_json::Value = wait_cancel(cancel, async {
        resp.json()
            .await
            .map_err(|_| ConvertError("parser HTTP request failed".into()))
    })
    .await?;
    Ok(ReadResult {
        markdown: extract_markdown(&v),
        ..ReadResult::default()
    })
}

fn authenticate(
    request: reqwest::RequestBuilder,
    config: &EffectiveEngineConfig,
) -> reqwest::RequestBuilder {
    match &config.bearer_token {
        Some(token) => request.bearer_auth(token),
        None => request,
    }
}

fn extract_markdown(v: &serde_json::Value) -> String {
    for key in ["md_content", "markdown", "markdown_content"] {
        if let Some(s) = v.get(key).and_then(|x| x.as_str()) {
            return s.to_string();
        }
    }
    if let Some(s) = v.pointer("/result/md_content").and_then(|x| x.as_str()) {
        return s.to_string();
    }
    if let Some(map) = v.get("results").and_then(|x| x.as_object()) {
        for item in map.values() {
            if let Some(s) = item.get("md_content").and_then(|x| x.as_str()) {
                return s.to_string();
            }
        }
    }
    String::new()
}

fn data_encoding_base64(bytes: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let b0 = bytes[i];
        let b1 = if i + 1 < bytes.len() { bytes[i + 1] } else { 0 };
        let b2 = if i + 2 < bytes.len() { bytes[i + 2] } else { 0 };
        out.push(T[(b0 >> 2) as usize] as char);
        out.push(T[(((b0 & 3) << 4) | (b1 >> 4)) as usize] as char);
        if i + 1 < bytes.len() {
            out.push(T[(((b1 & 15) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if i + 2 < bytes.len() {
            out.push(T[(b2 & 63) as usize] as char);
        } else {
            out.push('=');
        }
        i += 3;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_picks_md_content() {
        let v = serde_json::json!({"md_content": "# Hi"});
        assert_eq!(extract_markdown(&v), "# Hi");
        let nested = serde_json::json!({"results": {"a.pdf": {"md_content": "x"}}});
        assert_eq!(extract_markdown(&nested), "x");
    }

    #[tokio::test]
    async fn conversion_sends_override_endpoint_and_auth_for_both_engines() {
        use std::collections::HashMap;
        use std::io::{Read, Write};
        for (engine, endpoint_key, token_key, path) in [
            (
                "mineru",
                "mineru_endpoint",
                "mineru_api_key",
                "/override/file_parse",
            ),
            (
                "paddleocr_vl",
                "paddleocr_vl_endpoint",
                "paddleocr_vl_token",
                "/override/layout-parsing",
            ),
        ] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 4096];
                loop {
                    let count = stream.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    request.extend_from_slice(&buffer[..count]);
                    if let Some(end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
                        let size = headers
                            .lines()
                            .find_map(|line| {
                                line.strip_prefix("content-length:")
                                    .and_then(|value| value.trim().parse::<usize>().ok())
                            })
                            .unwrap();
                        if request.len() >= end + 4 + size {
                            break;
                        }
                    }
                }
                let body = r##"{"markdown":"# received"}"##;
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
                String::from_utf8_lossy(&request).to_ascii_lowercase()
            });
            let overrides = HashMap::from([
                (endpoint_key.into(), format!("http://{addr}/override")),
                (token_key.into(), "test-bearer".into()),
            ]);
            let parsed = crate::convert_with(crate::ConvertInput {
                engine,
                file_name: "fixture.pdf",
                file_type: "pdf",
                is_url: false,
                bytes: b"%PDF fixture".to_vec(),
                url: "",
                title: "",
                overrides: &overrides,
            })
            .await
            .unwrap();
            assert_eq!(parsed.markdown, "# received");
            let request = server.join().unwrap();
            assert!(request.starts_with(&format!("post {path} http/1.1")));
            assert!(request.contains("authorization: bearer test-bearer"));
        }
    }

    #[tokio::test]
    async fn cloud_only_configuration_is_explicitly_unsupported() {
        let overrides =
            std::collections::HashMap::from([("mineru_api_key".into(), "do-not-log".into())]);
        let error = crate::convert_with(crate::ConvertInput {
            engine: "mineru_cloud",
            file_name: "a.pdf",
            file_type: "pdf",
            is_url: false,
            bytes: vec![],
            url: "",
            title: "",
            overrides: &overrides,
        })
        .await
        .unwrap_err();
        assert!(error.0.contains("unsupported"));
        assert!(!error.0.contains("do-not-log"));
    }
}
