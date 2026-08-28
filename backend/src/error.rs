//! Central application error type.
//! Every handler returns `AppResult<T>` so errors are converted to safe,
//! consistent JSON responses. Database errors never leak internal details.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    Unauthorized(String),
    #[error("{0}")]
    Forbidden(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    #[error("rate limit exceeded")]
    RateLimited(u64),
    #[error("payload too large")]
    PayloadTooLarge,
    #[error("{0}")]
    UnsupportedMedia(String),
    #[error("internal server error: {0}")]
    Internal(String),
}

pub type AppResult<T> = Result<T, AppError>;

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::BadRequest(m) => (StatusCode::BAD_REQUEST, m.clone()),
            AppError::Unauthorized(m) => (StatusCode::UNAUTHORIZED, m.clone()),
            AppError::Forbidden(m) => (StatusCode::FORBIDDEN, m.clone()),
            AppError::NotFound(m) => (StatusCode::NOT_FOUND, m.clone()),
            AppError::Conflict(m) => (StatusCode::CONFLICT, m.clone()),
            AppError::RateLimited(retry_after) => {
                let mut r = (
                    StatusCode::TOO_MANY_REQUESTS,
                    "تم تجاوز حد الطلبات المسموح. حاول مرة أخرى بعد قليل.".to_string(),
                )
                    .into_response();
                r.headers_mut().insert(
                    "retry-after",
                    retry_after.to_string().parse().unwrap_or_else(|_| "60".parse().unwrap()),
                );
                return r;
            }
            AppError::PayloadTooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "حجم الملف المرفوع أكبر من الحد المسموح.".to_string(),
            )
                .into_response(),
            AppError::UnsupportedMedia(m) => (StatusCode::UNSUPPORTED_MEDIA_TYPE, m.clone()),
            AppError::Internal(m) => {
                tracing::error!("internal error: {m}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "حدث خطأ غير متوقع في الخادم.".to_string(),
                )
                    .into_response()
            }
        };
        let body = Json(json!({ "error": message, "code": status.as_u16() }));
        let mut resp = (status, body).into_response();
        if let StatusCode::TOO_MANY_REQUESTS = status {
            resp.headers_mut().insert("retry-after", "60".parse().unwrap());
        }
        resp
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        match &e {
            sqlx::Error::Database(db) => {
                let code = db.code().map(|c| c.to_string()).unwrap_or_default();
                let msg = db.message();
                match code.as_str() {
                    "23505" => AppError::Conflict("هذا الاسم أو البريد الإلكتروني مستخدم بالفعل.".into()),
                    "23503" => AppError::BadRequest("مرجع غير صالح.".into()),
                    "23514" => AppError::BadRequest(format!("قيمة غير صالحة: {msg}")),
                    "22001" => AppError::PayloadTooLarge,
                    _ => {
                        tracing::error!("database error [{}]: {}", code, db.message());
                        AppError::Internal("database error".into())
                    }
                }
            }
            sqlx::Error::RowNotFound => AppError::NotFound("العنصر المطلوب غير موجود.".into()),
            other => {
                tracing::error!("sqlx error: {other}");
                AppError::Internal("database error".into())
            }
        }
    }
}
