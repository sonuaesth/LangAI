use axum::{
    extract::{DefaultBodyLimit, State},
    routing::get,
    Json, Router,
};
use langai_contracts::HealthResponse;

use crate::{audio, auth, domain, error::ApiError, openai, secrets, state::AppState, sync};

pub fn router(state: AppState) -> Router {
    let max_audio_bytes = state.config.max_audio_bytes;
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
        .route(
            "/api/v1/settings",
            get(domain::get_settings).put(domain::update_settings),
        )
        .route(
            "/api/v1/sentences",
            get(domain::list_sentences).post(domain::create_sentence),
        )
        .route(
            "/api/v1/sentences/{sentence_id}",
            get(domain::get_sentence).delete(domain::delete_sentence),
        )
        .route(
            "/api/v1/sentences/{sentence_id}/prepare",
            axum::routing::post(openai::prepare_sentence),
        )
        .route(
            "/api/v1/sentences/{sentence_id}/audio",
            get(audio::download_audio)
                .put(audio::upload_audio)
                .delete(audio::delete_audio),
        )
        .route(
            "/api/v1/sentences/{sentence_id}/audio/generate",
            axum::routing::post(audio::generate_audio),
        )
        .route("/api/v1/provider-keys", get(secrets::key_statuses))
        .route(
            "/api/v1/provider-keys/{provider}",
            axum::routing::put(secrets::save_key).delete(secrets::delete_key),
        )
        .route("/api/v1/sync/push", axum::routing::post(sync::push))
        .route("/api/v1/sync/pull", get(sync::pull))
        .layer(DefaultBodyLimit::max(max_audio_bytes))
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
