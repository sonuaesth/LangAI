use std::path::PathBuf;

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use langai_contracts::{AudioMetadataResponse, GenerateAudioRequest};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    auth::{authenticate_request, require_mutation_auth},
    error::ApiError,
    secrets::decrypt_key,
    state::AppState,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioQuery {
    target_language: String,
}

fn language(value: &str) -> Result<String, ApiError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 100 {
        return Err(ApiError::InvalidInput("Invalid target language".into()));
    }
    Ok(value.to_owned())
}

fn safe_name(value: Option<&str>) -> Result<Option<String>, ApiError> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            if value.chars().count() > 255 || value.chars().any(char::is_control) {
                Err(ApiError::InvalidInput("Invalid audio file name".into()))
            } else {
                Ok(value.to_owned())
            }
        })
        .transpose()
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn valid_mp3(bytes: &[u8]) -> bool {
    bytes.len() >= 3 && (bytes.starts_with(b"ID3") || (bytes[0] == 0xff && bytes[1] & 0xe0 == 0xe0))
}

fn object_key(user_id: Uuid, hash: &str) -> String {
    format!("{user_id}/{}/{hash}.mp3", &hash[..2])
}

fn object_path(state: &AppState, user_id: Uuid, hash: &str) -> Result<PathBuf, ApiError> {
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ApiError::Internal);
    }
    Ok(state
        .config
        .audio_storage_path
        .join(user_id.to_string())
        .join(&hash[..2])
        .join(format!("{hash}.mp3")))
}

async fn store_bytes(
    state: &AppState,
    user_id: Uuid,
    bytes: &[u8],
) -> Result<(String, String), ApiError> {
    if bytes.is_empty() || bytes.len() > state.config.max_audio_bytes {
        return Err(ApiError::PayloadTooLarge);
    }
    if !valid_mp3(bytes) {
        return Err(ApiError::InvalidInput(
            "Only a valid MP3 audio file is supported".into(),
        ));
    }
    let hash = sha256(bytes);
    let path = object_path(state, user_id, &hash)?;
    let parent = path.parent().ok_or(ApiError::Internal)?;
    tokio::fs::create_dir_all(parent)
        .await
        .map_err(|_| ApiError::Internal)?;
    if tokio::fs::metadata(&path).await.is_err() {
        let temporary = parent.join(format!(".{}.{}.tmp", hash, Uuid::new_v4()));
        tokio::fs::write(&temporary, bytes)
            .await
            .map_err(|_| ApiError::Internal)?;
        match tokio::fs::rename(&temporary, &path).await {
            Ok(()) => {}
            Err(_) if tokio::fs::metadata(&path).await.is_ok() => {
                let _ = tokio::fs::remove_file(&temporary).await;
            }
            Err(_) => {
                let _ = tokio::fs::remove_file(&temporary).await;
                return Err(ApiError::Internal);
            }
        }
    }
    Ok((hash.clone(), object_key(user_id, &hash)))
}

async fn attach_audio(
    state: &AppState,
    user_id: Uuid,
    sentence_id: Uuid,
    target_language: &str,
    bytes: &[u8],
    name: Option<String>,
) -> Result<AudioMetadataResponse, ApiError> {
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(
           SELECT 1 FROM sentences WHERE id=$1 AND user_id=$2 AND deleted_at IS NULL
         )",
    )
    .bind(sentence_id)
    .bind(user_id)
    .fetch_one(&state.database)
    .await?;
    if !exists {
        return Err(ApiError::NotFound);
    }
    let (hash, key) = store_bytes(state, user_id, bytes).await?;
    let size = bytes.len() as i64;
    let mut tx = state.database.begin().await?;
    sqlx::query(
        "INSERT INTO sentence_languages(
           user_id,sentence_id,target_language,audio_object_key,audio_name,audio_mime,
           audio_sha256,audio_size
         ) VALUES($1,$2,$3,$4,$5,'audio/mpeg',$6,$7)
         ON CONFLICT(user_id,sentence_id,target_language) DO UPDATE SET
           audio_object_key=excluded.audio_object_key,audio_name=excluded.audio_name,
           audio_mime=excluded.audio_mime,audio_sha256=excluded.audio_sha256,
           audio_size=excluded.audio_size,deleted_at=NULL,
           revision=sentence_languages.revision+1,updated_at=now()",
    )
    .bind(user_id)
    .bind(sentence_id)
    .bind(target_language)
    .bind(key)
    .bind(&name)
    .bind(&hash)
    .bind(size)
    .execute(&mut *tx)
    .await?;
    let revision = sqlx::query_scalar::<_, i64>(
        "SELECT revision FROM sentence_languages
         WHERE user_id=$1 AND sentence_id=$2 AND target_language=$3",
    )
    .bind(user_id)
    .bind(sentence_id)
    .bind(target_language)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO sync_changes(user_id,entity_type,entity_id,operation,revision)
         VALUES($1,$2,$3,'upsert',$4)",
    )
    .bind(user_id)
    .bind(format!("sentence_language:{target_language}"))
    .bind(sentence_id)
    .bind(revision)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(AudioMetadataResponse {
        sha256: hash,
        size,
        mime: "audio/mpeg".into(),
        name,
    })
}

pub async fn upload_audio(
    State(state): State<AppState>,
    Path(sentence_id): Path<Uuid>,
    Query(query): Query<AudioQuery>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<AudioMetadataResponse>), ApiError> {
    let auth = authenticate_request(&state, &headers).await?;
    require_mutation_auth(&state, &headers, &auth)?;
    if headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        != Some("audio/mpeg")
    {
        return Err(ApiError::InvalidInput(
            "Content-Type must be audio/mpeg".into(),
        ));
    }
    if body.len() > state.config.max_audio_bytes {
        return Err(ApiError::PayloadTooLarge);
    }
    let expected_hash = headers
        .get("x-content-sha256")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::InvalidInput("X-Content-SHA256 is required".into()))?;
    let actual_hash = sha256(&body);
    if !actual_hash.eq_ignore_ascii_case(expected_hash) {
        return Err(ApiError::InvalidInput(
            "Audio checksum does not match".into(),
        ));
    }
    let name = safe_name(
        headers
            .get("x-file-name")
            .and_then(|value| value.to_str().ok()),
    )?;
    let metadata = attach_audio(
        &state,
        auth.user_id,
        sentence_id,
        &language(&query.target_language)?,
        &body,
        name,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(metadata)))
}

pub async fn download_audio(
    State(state): State<AppState>,
    Path(sentence_id): Path<Uuid>,
    Query(query): Query<AudioQuery>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let auth = authenticate_request(&state, &headers).await?;
    let language = language(&query.target_language)?;
    let (hash, name) = sqlx::query_as::<_, (String, Option<String>)>(
        "SELECT audio_sha256,audio_name FROM sentence_languages
         WHERE user_id=$1 AND sentence_id=$2 AND target_language=$3
         AND deleted_at IS NULL AND audio_sha256 IS NOT NULL",
    )
    .bind(auth.user_id)
    .bind(sentence_id)
    .bind(language)
    .fetch_optional(&state.database)
    .await?
    .ok_or(ApiError::NotFound)?;
    let bytes = tokio::fs::read(object_path(&state, auth.user_id, &hash)?)
        .await
        .map_err(|_| ApiError::NotFound)?;
    if sha256(&bytes) != hash {
        tracing::error!(user_id = %auth.user_id, sentence_id = %sentence_id, "audio checksum failed");
        return Err(ApiError::Internal);
    }
    let mut response_headers = HeaderMap::new();
    response_headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("audio/mpeg"));
    response_headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=3600"),
    );
    response_headers.insert(
        "x-content-sha256",
        HeaderValue::from_str(&hash).map_err(|_| ApiError::Internal)?,
    );
    if let Some(name) = safe_name(name.as_deref())? {
        let fallback = name.replace(['"', '\\'], "_");
        response_headers.insert(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_str(&format!("inline; filename=\"{fallback}\""))
                .map_err(|_| ApiError::Internal)?,
        );
    }
    Ok((response_headers, bytes).into_response())
}

pub async fn delete_audio(
    State(state): State<AppState>,
    Path(sentence_id): Path<Uuid>,
    Query(query): Query<AudioQuery>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let auth = authenticate_request(&state, &headers).await?;
    require_mutation_auth(&state, &headers, &auth)?;
    let target_language = language(&query.target_language)?;
    let mut tx = state.database.begin().await?;
    let revision = sqlx::query_scalar::<_, i64>(
        "UPDATE sentence_languages SET audio_object_key=NULL,audio_name=NULL,audio_mime=NULL,
         audio_sha256=NULL,audio_size=NULL,revision=revision+1,updated_at=now()
         WHERE user_id=$1 AND sentence_id=$2 AND target_language=$3
         AND deleted_at IS NULL AND audio_sha256 IS NOT NULL RETURNING revision",
    )
    .bind(auth.user_id)
    .bind(sentence_id)
    .bind(&target_language)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ApiError::NotFound)?;
    sqlx::query(
        "INSERT INTO sync_changes(user_id,entity_type,entity_id,operation,revision)
         VALUES($1,$2,$3,'upsert',$4)",
    )
    .bind(auth.user_id)
    .bind(format!("sentence_language:{target_language}"))
    .bind(sentence_id)
    .bind(revision)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn generate_audio(
    State(state): State<AppState>,
    Path(sentence_id): Path<Uuid>,
    headers: HeaderMap,
    Json(input): Json<GenerateAudioRequest>,
) -> Result<Json<AudioMetadataResponse>, ApiError> {
    let auth = authenticate_request(&state, &headers).await?;
    require_mutation_auth(&state, &headers, &auth)?;
    let target_language = language(&input.target_language)?;
    let (translation, voice_id) = sqlx::query_as::<_, (String, String)>(
        "SELECT p.translation,us.elevenlabs_voice_id
         FROM sentence_languages sl
         JOIN preparations p ON p.id=sl.active_preparation_id AND p.user_id=sl.user_id
         JOIN user_settings us ON us.user_id=sl.user_id
         WHERE sl.user_id=$1 AND sl.sentence_id=$2 AND sl.target_language=$3
         AND sl.deleted_at IS NULL AND sl.status='ready'
         AND us.elevenlabs_voice_id IS NOT NULL",
    )
    .bind(auth.user_id)
    .bind(sentence_id)
    .bind(&target_language)
    .fetch_optional(&state.database)
    .await?
    .ok_or_else(|| ApiError::InvalidInput("Select an ElevenLabs voice first".into()))?;
    let key = decrypt_key(&state, auth.user_id, "elevenlabs").await?;
    let response = reqwest::Client::new()
        .post(format!(
            "https://api.elevenlabs.io/v1/text-to-speech/{voice_id}?output_format=mp3_44100_128"
        ))
        .header("xi-api-key", key)
        .json(&serde_json::json!({
            "text": translation,
            "model_id": "eleven_multilingual_v2"
        }))
        .send()
        .await
        .map_err(|_| ApiError::Provider("ElevenLabs request failed".into()))?;
    if !response.status().is_success() {
        return Err(ApiError::Provider(format!(
            "ElevenLabs request failed with status {}",
            response.status()
        )));
    }
    if response
        .content_length()
        .is_some_and(|size| size > state.config.max_audio_bytes as u64)
    {
        return Err(ApiError::PayloadTooLarge);
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|_| ApiError::Provider("Invalid ElevenLabs response".into()))?;
    let metadata = attach_audio(
        &state,
        auth.user_id,
        sentence_id,
        &target_language,
        &bytes,
        Some(format!("{sentence_id}-{target_language}.mp3")),
    )
    .await?;
    Ok(Json(metadata))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_mp3_magic_and_names() {
        assert!(valid_mp3(b"ID3payload"));
        assert!(valid_mp3(&[0xff, 0xfb, 0x90]));
        assert!(!valid_mp3(b"not audio"));
        assert!(safe_name(Some("bad\nname.mp3")).is_err());
    }
}
