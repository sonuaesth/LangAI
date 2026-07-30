use axum::{extract::State, routing::get, Json, Router};
use langai_contracts::HealthResponse;

use crate::{auth, error::ApiError, state::AppState};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/v1/health", get(health))
        .route("/api/v1/auth/register", axum::routing::post(auth::register))
        .route("/api/v1/auth/login", axum::routing::post(auth::login))
        .route(
            "/api/v1/auth/device-token",
            axum::routing::post(auth::create_device_token),
        )
        .route(
            "/api/v1/auth/device-session",
            get(auth::current_device_session),
        )
        .route(
            "/api/v1/auth/session",
            get(auth::current_session).delete(auth::logout),
        )
        .route("/api/v1/devices", get(auth::list_devices))
        .route(
            "/api/v1/devices/{device_id}",
            axum::routing::delete(auth::revoke_device),
        )
        .with_state(state)
}

async fn health(State(state): State<AppState>) -> Result<Json<HealthResponse>, ApiError> {
    sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.database)
        .await?;
    Ok(Json(HealthResponse {
        status: "ok".into(),
        database: "ok".into(),
    }))
}
