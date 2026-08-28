use reqwest::{Response, StatusCode};

pub(crate) const STRICT_V2_MAX_RESPONSE_BYTES: usize = 64 * 1024 * 1024;

pub(crate) struct BoundedResponseBodyV2 {
    pub(crate) status: StatusCode,
    pub(crate) content_length: Option<u64>,
    pub(crate) bytes: Vec<u8>,
}

#[derive(Debug)]
pub(crate) enum BoundedBodyErrorV2 {
    TooLarge,
    Transport(reqwest::Error),
}

/// Reads a provider body without ever extending the owned buffer beyond `limit`.
/// `Content-Length` is rejected up front when available; chunked responses are
/// counted before each append and the connection is dropped on the first
/// over-limit chunk.
pub(crate) async fn read_bounded_response_body_v2(
    mut response: Response,
    limit: usize,
) -> Result<BoundedResponseBodyV2, BoundedBodyErrorV2> {
    let status = response.status();
    let content_length = response.content_length();
    if content_length.is_some_and(|length| length > limit as u64) {
        return Err(BoundedBodyErrorV2::TooLarge);
    }
    let initial_capacity = content_length
        .and_then(|length| usize::try_from(length).ok())
        .unwrap_or(0)
        .min(limit);
    let mut bytes = Vec::with_capacity(initial_capacity);
    loop {
        let Some(chunk) = response
            .chunk()
            .await
            .map_err(BoundedBodyErrorV2::Transport)?
        else {
            break;
        };
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            return Err(BoundedBodyErrorV2::TooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(BoundedResponseBodyV2 {
        status,
        content_length,
        bytes,
    })
}
