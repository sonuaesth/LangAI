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
    #[error("{0}")]
    InvalidInput(String),
    #[error("resource not found")]
    NotFound,
    #[error("authentication required")]
    Unauthorized,
    #[error("access denied")]
    Forbidden,
}

impl ApiError {
    fn status_and_code(&self) -> (StatusCode, &'static str) {
        match self {
            Self::InvalidInput(_) => (StatusCode::BAD_REQUEST, "invalid_input"),
            Self::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            Self::Database(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = self.status_and_code();
        if matches!(self, Self::Database(_)) {
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
