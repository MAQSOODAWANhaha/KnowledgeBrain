//! Persist inline / remote images as `objects/{sha256}` and rewrite Markdown.

use crate::{ImageRef, ReadResult};

pub fn rewrite_inline(result: &ReadResult) -> (String, Vec<(String, Vec<u8>)>) {
    let mut md = result.markdown.clone();
    let mut blobs = Vec::new();
    for img in &result.images {
        if img.data.is_empty() {
            continue;
        }
        let hash = domain::sha256_hex(&img.data);
        let key = format!("objects/{hash}");
        replace_ref(&mut md, img, &key);
        blobs.push((hash, img.data.clone()));
    }
    (md, blobs)
}

pub async fn rewrite_images(result: &ReadResult) -> (String, Vec<(String, Vec<u8>)>) {
    let (mut md, mut blobs) = rewrite_inline(result);
    let remotes = remote_urls(&md);
    for url in remotes {
        if domain::url_blocked(&url) {
            continue;
        }
        match fetch_image(&url).await {
            Ok(data) if !data.is_empty() => {
                let hash = domain::sha256_hex(&data);
                let key = format!("objects/{hash}");
                md = md.replace(&url, &key);
                blobs.push((hash, data));
            }
            _ => {}
        }
    }
    (md, blobs)
}

fn replace_ref(md: &mut String, img: &ImageRef, key: &str) {
    if !img.original_ref.is_empty() {
        *md = md.replace(&img.original_ref, key);
    }
    if !img.storage_key.is_empty() && img.storage_key != img.original_ref {
        *md = md.replace(&img.storage_key, key);
    }
    if !img.filename.is_empty() {
        let nested = format!("images/{}", img.filename);
        if md.contains(&nested) {
            *md = md.replace(&nested, key);
        }
    }
}

fn remote_urls(md: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = md;
    while let Some(i) = rest.find("](") {
        let after = &rest[i + 2..];
        if let Some(end) = after.find(')') {
            let url = &after[..end];
            if url.starts_with("http://") || url.starts_with("https://") {
                out.push(url.to_string());
            }
            rest = &after[end + 1..];
        } else {
            break;
        }
    }
    out
}

fn max_bytes() -> usize {
    std::env::var("MAX_FILE_SIZE_MB")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(50)
        * 1024
        * 1024
}

fn join_redirect(current: &str, location: &str) -> Result<String, String> {
    if location.starts_with("http://") || location.starts_with("https://") {
        return Ok(location.to_string());
    }
    let base = reqwest::Url::parse(current).map_err(|e| e.to_string())?;
    Ok(base.join(location).map_err(|e| e.to_string())?.to_string())
}

async fn fetch_image(url: &str) -> Result<Vec<u8>, String> {
    if domain::url_blocked(url) {
        return Err("url failed SSRF check".into());
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    let mut current = url.to_string();
    for _ in 0..=3 {
        if domain::url_blocked(&current) {
            return Err("url failed SSRF check".into());
        }
        let resp = client
            .get(&current)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if resp.status().is_redirection() {
            let loc = resp
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| "redirect without location".to_string())?;
            current = join_redirect(&current, loc)?;
            continue;
        }
        if !resp.status().is_success() {
            return Err(format!("image fetch {}", resp.status()));
        }
        if let Some(len) = resp.content_length()
            && len as usize > max_bytes()
        {
            return Err("image exceeds MAX_FILE_SIZE_MB".into());
        }
        let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
        if bytes.len() > max_bytes() {
            return Err("image exceeds MAX_FILE_SIZE_MB".into());
        }
        return Ok(bytes.to_vec());
    }
    Err("too many redirects".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ImageRef;

    #[test]
    fn inline_rewrites_to_objects_key() {
        let data = b"PNGDATA".to_vec();
        let r = ReadResult {
            markdown: "see ![p](images/p.png)".into(),
            images: vec![ImageRef {
                filename: "p.png".into(),
                original_ref: "images/p.png".into(),
                data,
                ..ImageRef::default()
            }],
            ..ReadResult::default()
        };
        let (md, blobs) = rewrite_inline(&r);
        assert_eq!(blobs.len(), 1);
        let key = format!("objects/{}", blobs[0].0);
        assert!(md.contains(&key), "{md}");
        assert!(!md.contains("images/p.png"));
        assert_eq!(blobs[0].0, domain::sha256_hex(b"PNGDATA"));
    }

    #[test]
    fn redirect_to_loopback_is_blocked() {
        let next =
            join_redirect("https://example.com/a.png", "http://127.0.0.1/secret.png").unwrap();
        assert!(domain::url_blocked(&next));
        let rel = join_redirect("https://example.com/dir/a.png", "../b.png").unwrap();
        assert_eq!(rel, "https://example.com/b.png");
    }

    #[tokio::test]
    async fn blocked_remote_image_stays() {
        let r = ReadResult {
            markdown: "![x](http://127.0.0.1/secret.png)".into(),
            ..ReadResult::default()
        };
        let (md, blobs) = rewrite_images(&r).await;
        assert!(md.contains("http://127.0.0.1/secret.png"));
        assert!(blobs.is_empty());
    }
}
