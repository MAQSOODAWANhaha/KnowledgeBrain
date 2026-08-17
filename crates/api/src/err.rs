use axum::Json;
use axum::http::StatusCode;
use serde::Serialize;

#[derive(Serialize)]
pub struct ErrorBody {
    pub error: ErrorInner,
}

#[derive(Serialize)]
pub struct ErrorInner {
    pub code: String,
    pub message: String,
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
