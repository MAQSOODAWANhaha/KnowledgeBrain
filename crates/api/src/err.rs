use axum::Json;
use axum::http::StatusCode;
use serde::Serialize;
use serde_json::Value;

#[derive(Serialize)]
pub struct ErrorBody {
    pub error: ErrorInner,
}

#[derive(Serialize)]
pub struct ErrorInner {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Box<Value>>,
}

pub fn fail(
    status: StatusCode,
    code: &str,
    message: impl Into<String>,
) -> (StatusCode, Json<ErrorBody>) {
    (
        status,
        Json(ErrorBody {
            error: ErrorInner {
                code: code.into(),
                message: message.into(),
                details: None,
            },
        }),
    )
}

pub fn fail_with_details(
    status: StatusCode,
    code: &str,
    message: impl Into<String>,
    details: Value,
) -> (StatusCode, Json<ErrorBody>) {
    (
        status,
        Json(ErrorBody {
            error: ErrorInner {
                code: code.into(),
                message: message.into(),
                details: Some(Box::new(details)),
            },
        }),
    )
}

pub fn unauthorized() -> (StatusCode, Json<ErrorBody>) {
    fail(
        StatusCode::UNAUTHORIZED,
        "UNAUTHORIZED",
        "missing or invalid token",
    )
}

pub fn forbidden() -> (StatusCode, Json<ErrorBody>) {
    fail(StatusCode::FORBIDDEN, "FORBIDDEN", "insufficient role")
}

pub fn not_found(msg: &str) -> (StatusCode, Json<ErrorBody>) {
    fail(StatusCode::NOT_FOUND, "NOT_FOUND", msg)
}

pub fn validation(msg: &str) -> (StatusCode, Json<ErrorBody>) {
    fail(StatusCode::BAD_REQUEST, "VALIDATION", msg)
}
