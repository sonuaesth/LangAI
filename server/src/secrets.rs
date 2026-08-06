use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use langai_contracts::{ProviderKeyStatus, SaveProviderKeyRequest, VoiceResponse};
use rand_core::{OsRng, RngCore};
use uuid::Uuid;

use crate::{
    auth::{authenticate_request, require_mutation_auth},
    error::ApiError,
    state::AppState,
};

fn provider(value: &str) -> Result<&str, ApiError> {
    match value {
        "openai" | "elevenlabs" => Ok(value),
        _ => Err(ApiError::InvalidInput("Unknown provider".into())),
    }
}

fn aad(user_id: Uuid, provider: &str) -> String {
    format!("langai:{user_id}:{provider}:v1")
}

fn encrypt(
    master_key: &[u8; 32],
    user_id: Uuid,
    provider: &str,
    plaintext: &[u8],
) -> Result<(Vec<u8>, Vec<u8>), ApiError> {
    let cipher = Aes256Gcm::new_from_slice(master_key).map_err(|_| ApiError::Internal)?;
    let mut nonce = [0_u8; 12];
    OsRng.fill_bytes(&mut nonce);
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad: aad(user_id, provider).as_bytes(),
            },
        )
        .map_err(|_| ApiError::Internal)?;
    Ok((ciphertext, nonce.to_vec()))
}

pub async fn decrypt_key(
    state: &AppState,
    user_id: Uuid,
    provider_name: &str,
) -> Result<String, ApiError> {
    let provider_name = provider(provider_name)?;
    let (ciphertext, nonce) = sqlx::query_as::<_, (Vec<u8>, Vec<u8>)>(
        "SELECT ciphertext,nonce FROM provider_credentials
         WHERE user_id=$1 AND provider=$2",
    )
    .bind(user_id)
    .bind(provider_name)
    .fetch_optional(&state.database)
    .await?
    .ok_or_else(|| ApiError::InvalidInput(format!("{provider_name} API key is not configured")))?;
    let cipher = Aes256Gcm::new_from_slice(&state.config.secrets_master_key)
        .map_err(|_| ApiError::Internal)?;
    if nonce.len() != 12 {
        return Err(ApiError::Internal);
    }
    let plaintext = cipher
        .decrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &ciphertext,
                aad: aad(user_id, provider_name).as_bytes(),
            },
        )
        .map_err(|_| ApiError::Internal)?;
    String::from_utf8(plaintext).map_err(|_| ApiError::Internal)
}

pub async fn key_statuses(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<ProviderKeyStatus>>, ApiError> {
    let auth = authenticate_request(&state, &headers).await?;
    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT provider,key_hint FROM provider_credentials WHERE user_id=$1",
    )
    .bind(auth.user_id)
    .fetch_all(&state.database)
    .await?;
    let statuses = ["openai", "elevenlabs"]
        .into_iter()
        .map(|provider| {
            let hint = rows
                .iter()
                .find(|(stored, _)| stored == provider)
                .map(|(_, hint)| hint.clone());
            ProviderKeyStatus {
                provider: provider.into(),
                configured: hint.is_some(),
                key_hint: hint,
            }
        })
        .collect();
    Ok(Json(statuses))
}

pub async fn save_key(
    State(state): State<AppState>,
    Path(provider_name): Path<String>,
    headers: HeaderMap,
    Json(input): Json<SaveProviderKeyRequest>,
) -> Result<StatusCode, ApiError> {
    let provider_name = provider(&provider_name)?;
    let auth = authenticate_request(&state, &headers).await?;
    require_mutation_auth(&state, &headers, &auth)?;
    let api_key = input.api_key.trim();
    if !(16..=500).contains(&api_key.len()) || api_key.chars().any(char::is_whitespace) {
        return Err(ApiError::InvalidInput("Invalid API key format".into()));
    }
    let visible = api_key.chars().rev().take(4).collect::<Vec<_>>();
    let hint = format!("••••{}", visible.into_iter().rev().collect::<String>());
    let (ciphertext, nonce) = encrypt(
        &state.config.secrets_master_key,
        auth.user_id,
        provider_name,
        api_key.as_bytes(),
    )?;
    sqlx::query(
        "INSERT INTO provider_credentials(user_id,provider,ciphertext,nonce,key_hint)
         VALUES($1,$2,$3,$4,$5)
         ON CONFLICT(user_id,provider) DO UPDATE SET
           ciphertext=excluded.ciphertext,nonce=excluded.nonce,key_hint=excluded.key_hint,
           updated_at=now()",
    )
    .bind(auth.user_id)
    .bind(provider_name)
    .bind(ciphertext)
    .bind(nonce)
    .bind(hint)
    .execute(&state.database)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_key(
    State(state): State<AppState>,
    Path(provider_name): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let provider_name = provider(&provider_name)?;
    let auth = authenticate_request(&state, &headers).await?;
    require_mutation_auth(&state, &headers, &auth)?;
    sqlx::query("DELETE FROM provider_credentials WHERE user_id=$1 AND provider=$2")
        .bind(auth.user_id)
        .bind(provider_name)
        .execute(&state.database)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

fn supported_openai_model(id: &str) -> bool {
    let id = id.to_ascii_lowercase();
    let supported = id.starts_with("gpt-5")
        || id.starts_with("gpt-4.1")
        || id.starts_with("gpt-4o")
        || id.starts_with("o3")
        || id.starts_with("o4");
    supported
        && ![
            "audio",
            "realtime",
            "transcribe",
            "tts",
            "image",
            "search",
            "embedding",
            "moderation",
            "computer-use",
            "codex",
        ]
        .iter()
        .any(|marker| id.contains(marker))
}

async fn models_for_key(key: &str) -> Result<Vec<String>, ApiError> {
    let response = reqwest::Client::new()
        .get("https://api.openai.com/v1/models")
        .bearer_auth(key)
        .send()
        .await
        .map_err(|_| ApiError::Provider("OpenAI request failed".into()))?;
    if !response.status().is_success() {
        return Err(ApiError::Provider(format!(
            "OpenAI key verification failed with status {}",
            response.status()
        )));
    }
    let value: serde_json::Value = response
        .json()
        .await
        .map_err(|_| ApiError::Provider("Invalid OpenAI response".into()))?;
    let mut models = value["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["id"].as_str())
        .filter(|id| supported_openai_model(id))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    models.sort();
    models.dedup();
    Ok(models)
}

async fn voices_for_key(key: &str) -> Result<Vec<VoiceResponse>, ApiError> {
    let response = reqwest::Client::new()
        .get("https://api.elevenlabs.io/v2/voices?page_size=100&sort=name&sort_direction=asc")
        .header("xi-api-key", key)
        .send()
        .await
        .map_err(|_| ApiError::Provider("ElevenLabs request failed".into()))?;
    if !response.status().is_success() {
        return Err(ApiError::Provider(format!(
            "ElevenLabs key verification failed with status {}",
            response.status()
        )));
    }
    let value: serde_json::Value = response
        .json()
        .await
        .map_err(|_| ApiError::Provider("Invalid ElevenLabs response".into()))?;
    Ok(value["voices"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|voice| {
            Some(VoiceResponse {
                voice_id: voice["voice_id"].as_str()?.to_owned(),
                name: voice["name"].as_str()?.to_owned(),
            })
        })
        .collect())
}

pub async fn openai_models(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<String>>, ApiError> {
    let auth = authenticate_request(&state, &headers).await?;
    let key = decrypt_key(&state, auth.user_id, "openai").await?;
    Ok(Json(models_for_key(&key).await?))
}

pub async fn verify_openai(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<SaveProviderKeyRequest>,
) -> Result<Json<Vec<String>>, ApiError> {
    let auth = authenticate_request(&state, &headers).await?;
    require_mutation_auth(&state, &headers, &auth)?;
    Ok(Json(models_for_key(input.api_key.trim()).await?))
}

pub async fn elevenlabs_voices(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<VoiceResponse>>, ApiError> {
    let auth = authenticate_request(&state, &headers).await?;
    let key = decrypt_key(&state, auth.user_id, "elevenlabs").await?;
    Ok(Json(voices_for_key(&key).await?))
}

pub async fn verify_elevenlabs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<SaveProviderKeyRequest>,
) -> Result<Json<Vec<VoiceResponse>>, ApiError> {
    let auth = authenticate_request(&state, &headers).await?;
    require_mutation_auth(&state, &headers, &auth)?;
    Ok(Json(voices_for_key(input.api_key.trim()).await?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encryption_uses_random_nonces() {
        let key = [7_u8; 32];
        let user_id = Uuid::new_v4();
        let first = encrypt(&key, user_id, "openai", b"secret").unwrap();
        let second = encrypt(&key, user_id, "openai", b"secret").unwrap();
        assert_ne!(first.0, second.0);
        assert_ne!(first.1, second.1);
    }

    #[test]
    fn filters_non_exercise_openai_models() {
        assert!(supported_openai_model("gpt-5-mini"));
        assert!(!supported_openai_model("gpt-4o-realtime-preview"));
        assert!(!supported_openai_model("text-embedding-3-small"));
    }
}
