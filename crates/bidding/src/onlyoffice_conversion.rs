//! Shared ONLYOFFICE transport and conversion of one frozen DOCX identity.
//! The source route must compare the capability to the registered export request
//! and serve that exact object's verified bytes, never the live current draft.
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Invalid,
    Unauthorized,
    Forbidden,
    Unavailable,
}

/// Deliberately contains no raw URL, credentials, token or provider response.
#[derive(Debug, thiserror::Error)]
#[error("{code}: {message}")]
pub struct Error {
    pub kind: ErrorKind,
    pub code: &'static str,
    pub message: &'static str,
}

impl Error {
    fn invalid(message: &'static str) -> Self {
        Self {
            kind: ErrorKind::Invalid,
            code: "VALIDATION",
            message,
        }
    }

    fn unavailable(code: &'static str, message: &'static str) -> Self {
        Self {
            kind: ErrorKind::Unavailable,
            code,
            message,
        }
    }
}

// No Debug implementation: the existing service and capability secrets remain
// backend-only even when callers log configuration/transport errors.
pub struct Config {
    pub server: Url,
    pub command: Url,
    pub api: Url,
    server_secret: String,
    capability_secret: String,
    ttl: u64,
    timeout: Duration,
}

impl Config {
    pub fn load() -> Result<Self, Error> {
        Self::from_values(|key| std::env::var(key).ok())
    }

    fn from_values(read: impl Fn(&str) -> Option<String>) -> Result<Self, Error> {
        let required = |name: &str| {
            read(name).filter(|v| !v.trim().is_empty()).ok_or_else(|| {
                Error::unavailable(
                    "ONLYOFFICE_NOT_CONFIGURED",
                    "ONLYOFFICE configuration is incomplete",
                )
            })
        };
        let origin = |value: String| -> Result<Url, Error> {
            let url =
                Url::parse(&value).map_err(|_| Error::invalid("invalid ONLYOFFICE origin"))?;
            if !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
                || url.path() != "/"
            {
                return Err(Error::invalid(
                    "ONLYOFFICE origins must be absolute HTTP(S) origins",
                ));
            }
            Ok(url)
        };
        let positive =
            |value: String| {
                value.parse::<u64>().ok().filter(|v| *v > 0).ok_or_else(|| {
                    Error::invalid("ONLYOFFICE time budgets must be positive seconds")
                })
            };
        let server_secret = required("KB_ONLYOFFICE_JWT_SECRET")?;
        let capability_secret = required("KB_ONLYOFFICE_CAPABILITY_SECRET")?;
        if server_secret == capability_secret {
            return Err(Error::invalid(
                "ONLYOFFICE service and capability secrets must be independent",
            ));
        }
        let server = origin(required("KB_ONLYOFFICE_SERVER_ORIGIN")?)?;
        let command = match read("KB_ONLYOFFICE_COMMAND_ORIGIN") {
            Some(value) if !value.trim().is_empty() => origin(value)?,
            _ => server.clone(),
        };
        Ok(Self {
            server,
            command,
            api: origin(required("KB_ONLYOFFICE_API_ORIGIN")?)?,
            server_secret,
            capability_secret,
            ttl: positive(required("KB_ONLYOFFICE_TOKEN_TTL_SECONDS")?)?,
            timeout: Duration::from_secs(positive(required(
                "KB_ONLYOFFICE_HTTP_TIMEOUT_SECONDS",
            )?)?),
        })
    }

    pub fn service_secret(&self) -> &str {
        &self.server_secret
    }
    pub fn capability_secret(&self) -> &str {
        &self.capability_secret
    }

    pub fn client(&self) -> Result<Client, Error> {
        Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .timeout(self.timeout)
            .build()
            .map_err(|_| {
                Error::unavailable(
                    "ONLYOFFICE_TRANSPORT_FAILED",
                    "document service client unavailable",
                )
            })
    }

    pub fn sign<T: Serialize>(&self, value: &T, capability: bool) -> Result<String, Error> {
        let secret = if capability {
            &self.capability_secret
        } else {
            &self.server_secret
        };
        encode(
            &Header::new(Algorithm::HS256),
            value,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .map_err(|_| {
            Error::unavailable("ONLYOFFICE_SIGN_FAILED", "document service signing failed")
        })
    }

    pub fn expiration(&self) -> Result<u64, Error> {
        let now = u64::try_from(chrono::Utc::now().timestamp())
            .map_err(|_| Error::invalid("invalid server clock"))?;
        now.checked_add(self.ttl)
            .filter(|v| *v <= i64::MAX as u64)
            .ok_or_else(|| Error::invalid("ONLYOFFICE token lifetime overflow"))
    }

    pub fn download_url(&self, value: &str) -> Result<Url, Error> {
        let mut url = Url::parse(value)
            .map_err(|_| Error::invalid("invalid document service download URL"))?;
        let origin = url.origin();
        if (origin != self.server.origin() && origin != self.command.origin())
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || !url.path().starts_with("/cache/files/")
        {
            return Err(Error {
                kind: ErrorKind::Forbidden,
                code: "ONLYOFFICE_DOWNLOAD_SCOPE",
                message: "document download is outside the configured service cache",
            });
        }
        if origin == self.server.origin() && self.command.origin() != self.server.origin() {
            let _ = url.set_scheme(self.command.scheme());
            url.set_host(self.command.host_str())
                .map_err(|_| Error::invalid("invalid document service download URL"))?;
            url.set_port(self.command.port())
                .map_err(|_| Error::invalid("invalid document service download URL"))?;
        }
        Ok(url)
    }

    pub fn source_url(&self, source: &FrozenDocxSource) -> Result<Url, Error> {
        source.validate()?;
        let capability = SourceCapability {
            aud: SOURCE_AUDIENCE.into(),
            source: source.clone(),
            exp: self.expiration()?,
        };
        let mut url = self
            .api
            .join(&format!(
                "api/v2/submission-workspaces/{}/exports/requests/{}/source",
                source.workspace_id, source.request_id,
            ))
            .map_err(|_| Error::invalid("invalid API origin"))?;
        url.query_pairs_mut()
            .append_pair("token", &self.sign(&capability, true)?);
        Ok(url)
    }

    /// Verifies signature, purpose and expiry. The route must additionally match
    /// every source field against the stored request, including the object length.
    pub fn verify_source_capability(&self, token: &str) -> Result<SourceCapability, Error> {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.leeway = 0;
        validation.set_audience(&[SOURCE_AUDIENCE]);
        let capability = decode::<SourceCapability>(
            token,
            &DecodingKey::from_secret(self.capability_secret.as_bytes()),
            &validation,
        )
        .map_err(|_| Error {
            kind: ErrorKind::Unauthorized,
            code: "ONLYOFFICE_CAPABILITY_INVALID",
            message: "export source capability invalid or expired",
        })?
        .claims;
        // jsonwebtoken validates a present aud; this field is required by serde.
        capability.source.validate()?;
        Ok(capability)
    }
}

const SOURCE_AUDIENCE: &str = "docx-export-source";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenDocxSource {
    pub workspace_id: Uuid,
    /// The frozen export request artifact, not the as-yet-unpublished output.
    pub request_id: Uuid,
    pub version_id: Uuid,
    pub docx_sha256: String,
    pub byte_length: u64,
}

impl FrozenDocxSource {
    fn validate(&self) -> Result<(), Error> {
        if self.byte_length == 0
            || self.docx_sha256.len() != 64
            || !self
                .docx_sha256
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err(Error::invalid("invalid frozen DOCX identity"));
        }
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceCapability {
    pub aud: String,
    pub source: FrozenDocxSource,
    pub exp: u64,
}

pub struct ConvertedPdf {
    pub source: FrozenDocxSource,
    pub pdf_sha256: String,
    pub bytes: Vec<u8>,
}

/// Enforces the caller's existing object/download limit, including chunked bodies.
pub async fn bounded_body(mut response: reqwest::Response, limit: usize) -> Result<Vec<u8>, Error> {
    if response
        .content_length()
        .is_some_and(|size| size > limit as u64)
    {
        return Err(Error::invalid("document service response too large"));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| {
        Error::unavailable("ONLYOFFICE_READ_FAILED", "document service response failed")
    })? {
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            return Err(Error::invalid("document service response too large"));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn conversion_body(config: &Config, source: &FrozenDocxSource) -> Result<Value, Error> {
    let url = config.source_url(source)?;
    // Stable for retries of this immutable request; distinct exports cannot share
    // an editor cache key, and edits require a new frozen export request.
    let key = format!(
        "kb-pdf-{}-{}",
        source.request_id.simple(),
        source.docx_sha256
    );
    let mut body =
        json!({"async":false,"filetype":"docx","outputtype":"pdf","key":key,"url":url.as_str()});
    body["token"] = json!(config.sign(&body, false)?);
    Ok(body)
}

#[derive(Deserialize)]
struct ConversionResponse {
    error: Option<i64>,
    #[serde(default, rename = "endConvert")]
    end_convert: bool,
    #[serde(rename = "fileUrl")]
    file_url: Option<String>,
}

/// No hidden retries and no independent PDF renderer. Only a completed, valid
/// PDF is returned. The caller owns object registration and atomic publication.
pub async fn convert_pdf(
    config: &Config,
    source: &FrozenDocxSource,
    max_pdf_bytes: usize,
    cancel: &CancellationToken,
) -> Result<ConvertedPdf, Error> {
    if max_pdf_bytes == 0 {
        return Err(Error::invalid("PDF output byte limit must be positive"));
    }
    let work = async {
        let body = conversion_body(config, source)?;
        let client = config.client()?;
        let endpoint = config
            .command
            .join("converter")
            .map_err(|_| Error::invalid("invalid server origin"))?;
        let response = client
            .post(endpoint)
            .json(&body)
            .send()
            .await
            .map_err(|_| {
                Error::unavailable(
                    "ONLYOFFICE_CONVERSION_FAILED",
                    "document conversion request failed",
                )
            })?;
        if !response.status().is_success() {
            return Err(Error::unavailable(
                "ONLYOFFICE_CONVERSION_FAILED",
                "document conversion did not succeed",
            ));
        }
        let response: ConversionResponse =
            serde_json::from_slice(&bounded_body(response, platform::max_file_bytes()).await?)
                .map_err(|_| {
                    Error::unavailable(
                        "ONLYOFFICE_CONVERSION_FAILED",
                        "invalid document conversion response",
                    )
                })?;
        if response.error.is_some_and(|code| code != 0) {
            return Err(Error::unavailable(
                "ONLYOFFICE_CONVERSION_REJECTED",
                "document service rejected conversion",
            ));
        }
        if !response.end_convert {
            return Err(Error::unavailable(
                "ONLYOFFICE_CONVERSION_INCOMPLETE",
                "document conversion did not complete",
            ));
        }
        let url = config.download_url(response.file_url.as_deref().ok_or_else(|| {
            Error::unavailable(
                "ONLYOFFICE_CONVERSION_FAILED",
                "converted document URL missing",
            )
        })?)?;
        let response = client.get(url).send().await.map_err(|_| {
            Error::unavailable("ONLYOFFICE_READ_FAILED", "document service download failed")
        })?;
        if !response.status().is_success() {
            return Err(Error::unavailable(
                "ONLYOFFICE_READ_FAILED",
                "document service download did not succeed",
            ));
        }
        let bytes = bounded_body(response, max_pdf_bytes).await?;
        let bytes = tokio::task::spawn_blocking(move || {
            crate::tender_upload::validate_pdf(&bytes)
                .map_err(|_| Error::invalid("document conversion did not produce a valid PDF"))?;
            Ok::<_, Error>(bytes)
        })
        .await
        .map_err(|_| {
            Error::unavailable("ONLYOFFICE_CONVERSION_FAILED", "PDF validation failed")
        })??;
        Ok(ConvertedPdf {
            source: source.clone(),
            pdf_sha256: platform::sha256_hex(&bytes),
            bytes,
        })
    };
    tokio::select! {
        biased;
        () = cancel.cancelled() => Err(Error::unavailable("ONLYOFFICE_CANCELLED", "document conversion cancelled")),
        result = work => result,
    }
}

#[cfg(test)]
mod tests;
