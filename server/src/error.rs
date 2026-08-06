use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use langai_contracts::{ApiErrorBody, ApiErrorDetail};

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("database request failed")]
    Database(#[from] sqlx::Error),
    #[error("internal error")]
    Internal,
    #[error("{0}")]
    InvalidInput(String),
    #[error("resource not found")]
    NotFound,
    #[error("authentication required")]
    Unauthorized,
    #[error("access denied")]
    Forbidden,
    #[error("{0}")]
    Conflict(String),
    #[error("invalid email or password")]
    InvalidCredentials,
    #[error("{0}")]
    Provider(String),
    #[error("request body is too large")]
    PayloadTooLarge,
}

impl ApiError {
    fn status_and_code(&self) -> (StatusCode, &'static str) {
        match self {
            Self::InvalidInput(_) => (StatusCode::BAD_REQUEST, "invalid_input"),
            Self::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            Self::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            Self::InvalidCredentials => (StatusCode::UNAUTHORIZED, "invalid_credentials"),
            Self::Provider(_) => (StatusCode::BAD_GATEWAY, "provider_error"),
            Self::PayloadTooLarge => (StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large"),
            Self::Database(_) | Self::Internal => {
                (StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
            }
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = self.status_and_code();
        if matches!(self, Self::Database(_) | Self::Internal) {
            tracing::error!(error = ?self, "request failed");
        }
        let message = if status.is_server_error() {
            "Internal server error".into()
        } else {
            self.to_string()
        };
        (
            status,
            Json(ApiErrorBody {
                error: ApiErrorDetail {
                    code,
                    message,
                    request_id: None,
                },
            }),
        )
            .into_response()
    }
}
