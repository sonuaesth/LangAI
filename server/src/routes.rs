use axum::{extract::State, routing::get, Json, Router};
use langai_contracts::HealthResponse;

use crate::{error::ApiError, state::AppState};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/v1/health", get(health))
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
