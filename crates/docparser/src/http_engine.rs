//! MinerU / PaddleOCR-VL HTTP convert (spec §5.2).

use crate::{ConvertError, NOT_CONFIGURED, ReadResult};

pub fn mineru_endpoint() -> String {
    std::env::var("KNOWLEDGEBRAIN_MINERU_ENDPOINT")
        .or_else(|_| std::env::var("MINERU_ENDPOINT"))
        .unwrap_or_default()
}

pub fn paddle_endpoint() -> String {
    std::env::var("KNOWLEDGEBRAIN_PADDLE_ENDPOINT")
        .or_else(|_| std::env::var("PADDLEOCR_VL_ENDPOINT"))
        .unwrap_or_default()
}

pub async fn convert_http(
    engine: &str,
    file_name: &str,
    bytes: Vec<u8>,
) -> Result<ReadResult, ConvertError> {
    match engine {
        "mineru" | "mineru_cloud" => mineru_parse(file_name, &bytes).await,
        "paddleocr_vl" | "paddleocr_vl_cloud" => paddle_parse(file_name, &bytes).await,
        _ => Ok(ReadResult {
            error: NOT_CONFIGURED.into(),
            ..ReadResult::default()
        }),
    }
}

async fn mineru_parse(file_name: &str, bytes: &[u8]) -> Result<ReadResult, ConvertError> {
    let base = mineru_endpoint();
    if base.is_empty() {
        return Ok(ReadResult {
            error: NOT_CONFIGURED.into(),
            ..ReadResult::default()
        });
    }
    let url = format!("{}/file_parse", base.trim_end_matches('/'));
    let part = reqwest::multipart::Part::bytes(bytes.to_vec())
        .file_name(file_name.to_string())
        .mime_str("application/octet-stream")
        .map_err(|e| ConvertError(e.to_string()))?;
    let form = reqwest::multipart::Form::new().part("files", part);
    let resp = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30 * 60))
        .build()
        .map_err(|e| ConvertError(e.to_string()))?
        .post(url)
        .multipart(form)
        .send()
        .await
        .map_err(|e| ConvertError(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(ConvertError(format!("mineru {}", resp.status())));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| ConvertError(e.to_string()))?;
    Ok(ReadResult {
        markdown: extract_markdown(&v),
        ..ReadResult::default()
    })
}

async fn paddle_parse(file_name: &str, bytes: &[u8]) -> Result<ReadResult, ConvertError> {
    let base = paddle_endpoint();
    if base.is_empty() {
        return Ok(ReadResult {
            error: NOT_CONFIGURED.into(),
            ..ReadResult::default()
        });
    }
    let url = format!("{}/layout-parsing", base.trim_end_matches('/'));
    let body = serde_json::json!({
        "file": data_encoding_base64(bytes),
        "fileName": file_name,
    });
    let resp = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30 * 60))
        .build()
        .map_err(|e| ConvertError(e.to_string()))?
        .post(url)
        .json(&body)
        .send()
        .await
        .map_err(|e| ConvertError(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(ConvertError(format!("paddle {}", resp.status())));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| ConvertError(e.to_string()))?;
    Ok(ReadResult {
        markdown: extract_markdown(&v),
        ..ReadResult::default()
    })
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
    async fn missing_endpoints_are_not_configured() {
        unsafe {
            std::env::remove_var("KNOWLEDGEBRAIN_MINERU_ENDPOINT");
            std::env::remove_var("MINERU_ENDPOINT");
            std::env::remove_var("KNOWLEDGEBRAIN_PADDLE_ENDPOINT");
            std::env::remove_var("PADDLEOCR_VL_ENDPOINT");
        }
        let r = convert_http("mineru", "a.pdf", b"%PDF".to_vec())
            .await
            .unwrap();
        assert_eq!(r.error, NOT_CONFIGURED);
        let r = convert_http("paddleocr_vl", "a.pdf", b"%PDF".to_vec())
            .await
            .unwrap();
        assert_eq!(r.error, NOT_CONFIGURED);
    }
}
