use std::collections::HashSet;

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use langai_contracts::{
    CreateSentenceRequest, ExerciseBlockResponse, ExerciseOptionResponse, PreparationResponse,
    SentenceLanguageResponse, SentenceResponse, SettingsResponse, UpdateSettingsRequest,
};
use serde::Deserialize;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    auth::{authenticate_request, require_mutation_auth},
    error::ApiError,
    state::AppState,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListSentencesQuery {
    target_language: Option<String>,
    topic: Option<String>,
}

fn required_text(value: &str, label: &str, max: usize) -> Result<String, ApiError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > max {
        return Err(ApiError::InvalidInput(format!(
            "{label} must contain 1 to {max} characters"
        )));
    }
    Ok(value.to_owned())
}

fn unique_texts(values: Vec<String>, label: &str, max: usize) -> Result<Vec<String>, ApiError> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for value in values {
        let value = required_text(&value, label, max)?;
        if seen.insert(value.to_lowercase()) {
            result.push(value);
        }
    }
    Ok(result)
}

async fn record_change(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    entity_type: &str,
    entity_id: Uuid,
    operation: &str,
    revision: i64,
) -> Result<(), ApiError> {
    sqlx::query(
        "INSERT INTO sync_changes(user_id,entity_type,entity_id,operation,revision)
         VALUES($1,$2,$3,$4,$5)",
    )
    .bind(user_id)
    .bind(entity_type)
    .bind(entity_id)
    .bind(operation)
    .bind(revision)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn get_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SettingsResponse>, ApiError> {
    let auth = authenticate_request(&state, &headers).await?;
    let row = sqlx::query_as::<_, (String, String, Option<String>, Option<String>, i64)>(
        "INSERT INTO user_settings(user_id) VALUES($1)
         ON CONFLICT(user_id) DO UPDATE SET user_id=excluded.user_id
         RETURNING model,target_language,elevenlabs_voice_id,elevenlabs_voice_name,revision",
    )
    .bind(auth.user_id)
    .fetch_one(&state.database)
    .await?;
    Ok(Json(SettingsResponse {
        model: row.0,
        target_language: row.1,
        elevenlabs_voice_id: row.2,
        elevenlabs_voice_name: row.3,
        revision: row.4,
    }))
}

pub async fn update_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<UpdateSettingsRequest>,
) -> Result<Json<SettingsResponse>, ApiError> {
    let auth = authenticate_request(&state, &headers).await?;
    require_mutation_auth(&state, &headers, &auth)?;
    let model = required_text(&input.model, "Model", 100)?;
    let target_language = required_text(&input.target_language, "Target language", 100)?;
    let voice_id = input
        .elevenlabs_voice_id
        .map(|value| required_text(&value, "Voice ID", 200))
        .transpose()?;
    let voice_name = input
        .elevenlabs_voice_name
        .map(|value| required_text(&value, "Voice name", 200))
        .transpose()?;
    let mut tx = state.database.begin().await?;
    let row = sqlx::query_as::<_, (String, String, Option<String>, Option<String>, i64)>(
        "INSERT INTO user_settings(
             user_id,model,target_language,elevenlabs_voice_id,elevenlabs_voice_name
         ) VALUES($1,$2,$3,$4,$5)
         ON CONFLICT(user_id) DO UPDATE SET
             model=excluded.model,target_language=excluded.target_language,
             elevenlabs_voice_id=excluded.elevenlabs_voice_id,
             elevenlabs_voice_name=excluded.elevenlabs_voice_name,
             revision=user_settings.revision+1,updated_at=now()
         RETURNING model,target_language,elevenlabs_voice_id,elevenlabs_voice_name,revision",
    )
    .bind(auth.user_id)
    .bind(model)
    .bind(target_language)
    .bind(voice_id)
    .bind(voice_name)
    .fetch_one(&mut *tx)
    .await?;
    record_change(
        &mut tx,
        auth.user_id,
        "settings",
        auth.user_id,
        "upsert",
        row.4,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(SettingsResponse {
        model: row.0,
        target_language: row.1,
        elevenlabs_voice_id: row.2,
        elevenlabs_voice_name: row.3,
        revision: row.4,
    }))
}

pub async fn create_sentence(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateSentenceRequest>,
) -> Result<(StatusCode, Json<SentenceResponse>), ApiError> {
    let auth = authenticate_request(&state, &headers).await?;
    require_mutation_auth(&state, &headers, &auth)?;
    let source_text = required_text(&input.source_text, "Source text", 10_000)?;
    let languages = unique_texts(input.target_languages, "Target language", 100)?;
    if languages.is_empty() {
        return Err(ApiError::InvalidInput(
            "At least one target language is required".into(),
        ));
    }
    let topics = unique_texts(input.topics, "Topic", 100)?;
    let sentence_id = Uuid::new_v4();
    let mut tx = state.database.begin().await?;
    sqlx::query("INSERT INTO sentences(id,user_id,source_text) VALUES($1,$2,$3)")
        .bind(sentence_id)
        .bind(auth.user_id)
        .bind(&source_text)
        .execute(&mut *tx)
        .await?;
    for language in &languages {
        sqlx::query(
            "INSERT INTO sentence_languages(user_id,sentence_id,target_language)
             VALUES($1,$2,$3)",
        )
        .bind(auth.user_id)
        .bind(sentence_id)
        .bind(language)
        .execute(&mut *tx)
        .await?;
    }
    for topic in &topics {
        let proposed_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO topics(id,user_id,name) VALUES($1,$2,$3)
             ON CONFLICT DO NOTHING",
        )
        .bind(proposed_id)
        .bind(auth.user_id)
        .bind(topic)
        .execute(&mut *tx)
        .await?;
        let topic_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM topics
             WHERE user_id=$1 AND lower(name)=lower($2) AND deleted_at IS NULL",
        )
        .bind(auth.user_id)
        .bind(topic)
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query("INSERT INTO sentence_topics(user_id,sentence_id,topic_id) VALUES($1,$2,$3)")
            .bind(auth.user_id)
            .bind(sentence_id)
            .bind(topic_id)
            .execute(&mut *tx)
            .await?;
    }
    record_change(&mut tx, auth.user_id, "sentence", sentence_id, "upsert", 1).await?;
    tx.commit().await?;
    let sentence = load_sentence(&state.database, auth.user_id, sentence_id).await?;
    Ok((StatusCode::CREATED, Json(sentence)))
}

pub async fn list_sentences(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(filter): Query<ListSentencesQuery>,
) -> Result<Json<Vec<SentenceResponse>>, ApiError> {
    let auth = authenticate_request(&state, &headers).await?;
    let language = filter
        .target_language
        .map(|value| required_text(&value, "Target language", 100))
        .transpose()?;
    let topic = filter
        .topic
        .map(|value| required_text(&value, "Topic", 100))
        .transpose()?;
    let ids = sqlx::query_scalar::<_, Uuid>(
        "SELECT s.id FROM sentences s
         WHERE s.user_id=$1 AND s.deleted_at IS NULL
         AND ($2::text IS NULL OR EXISTS (
             SELECT 1 FROM sentence_languages sl
             WHERE sl.user_id=s.user_id AND sl.sentence_id=s.id
             AND sl.target_language=$2 AND sl.deleted_at IS NULL
         ))
         AND ($3::text IS NULL OR EXISTS (
             SELECT 1 FROM sentence_topics st JOIN topics t
               ON t.user_id=st.user_id AND t.id=st.topic_id
             WHERE st.user_id=s.user_id AND st.sentence_id=s.id
             AND lower(t.name)=lower($3) AND st.deleted_at IS NULL AND t.deleted_at IS NULL
         ))
         ORDER BY s.created_at DESC",
    )
    .bind(auth.user_id)
    .bind(language)
    .bind(topic)
    .fetch_all(&state.database)
    .await?;
    let mut sentences = Vec::with_capacity(ids.len());
    for id in ids {
        sentences.push(load_sentence(&state.database, auth.user_id, id).await?);
    }
    Ok(Json(sentences))
}

pub async fn get_sentence(
    State(state): State<AppState>,
    Path(sentence_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<SentenceResponse>, ApiError> {
    let auth = authenticate_request(&state, &headers).await?;
    Ok(Json(
        load_sentence(&state.database, auth.user_id, sentence_id).await?,
    ))
}

pub async fn delete_sentence(
    State(state): State<AppState>,
    Path(sentence_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let auth = authenticate_request(&state, &headers).await?;
    require_mutation_auth(&state, &headers, &auth)?;
    let mut tx = state.database.begin().await?;
    let revision = sqlx::query_scalar::<_, i64>(
        "UPDATE sentences SET deleted_at=now(),updated_at=now(),revision=revision+1
         WHERE id=$1 AND user_id=$2 AND deleted_at IS NULL RETURNING revision",
    )
    .bind(sentence_id)
    .bind(auth.user_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(ApiError::NotFound)?;
    sqlx::query(
        "UPDATE sentence_languages SET deleted_at=now(),updated_at=now(),revision=revision+1
         WHERE sentence_id=$1 AND user_id=$2 AND deleted_at IS NULL",
    )
    .bind(sentence_id)
    .bind(auth.user_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE sentence_topics SET deleted_at=now(),revision=revision+1
         WHERE sentence_id=$1 AND user_id=$2 AND deleted_at IS NULL",
    )
    .bind(sentence_id)
    .bind(auth.user_id)
    .execute(&mut *tx)
    .await?;
    record_change(
        &mut tx,
        auth.user_id,
        "sentence",
        sentence_id,
        "delete",
        revision,
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn load_sentence(
    database: &PgPool,
    user_id: Uuid,
    sentence_id: Uuid,
) -> Result<SentenceResponse, ApiError> {
    let (source_text, created_at, revision) = sqlx::query_as::<_, (String, String, i64)>(
        "SELECT source_text,
         to_char(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'),revision
         FROM sentences WHERE id=$1 AND user_id=$2 AND deleted_at IS NULL",
    )
    .bind(sentence_id)
    .bind(user_id)
    .fetch_optional(database)
    .await?
    .ok_or(ApiError::NotFound)?;
    let language_rows = sqlx::query_as::<
        _,
        (
            String,
            String,
            Option<String>,
            Option<String>,
            bool,
            Option<String>,
            Option<i64>,
            Option<String>,
            Option<String>,
            i64,
            Option<Uuid>,
        ),
    >(
        "SELECT target_language,status,error,translation_comment,
             audio_object_key IS NOT NULL,audio_sha256,audio_size,audio_mime,audio_name,
             revision,active_preparation_id
             FROM sentence_languages
             WHERE user_id=$1 AND sentence_id=$2 AND deleted_at IS NULL
             ORDER BY target_language",
    )
    .bind(user_id)
    .bind(sentence_id)
    .fetch_all(database)
    .await?;
    let mut languages = Vec::with_capacity(language_rows.len());
    for (
        target_language,
        status,
        error,
        translation_comment,
        audio_available,
        audio_sha256,
        audio_size,
        audio_mime,
        audio_name,
        revision,
        preparation_id,
    ) in language_rows
    {
        let active_preparation = match preparation_id {
            Some(preparation_id) => {
                let (version, model, translation) = sqlx::query_as::<_, (i32, String, String)>(
                    "SELECT version,model,translation FROM preparations
                         WHERE id=$1 AND user_id=$2 AND deleted_at IS NULL",
                )
                .bind(preparation_id)
                .bind(user_id)
                .fetch_optional(database)
                .await?
                .ok_or(ApiError::Internal)?;
                let block_rows = sqlx::query_as::<_, (Uuid, i32, String, Option<String>)>(
                    "SELECT id,position,correct,hint FROM blocks
                     WHERE user_id=$1 AND preparation_id=$2 AND deleted_at IS NULL
                     ORDER BY position",
                )
                .bind(user_id)
                .bind(preparation_id)
                .fetch_all(database)
                .await?;
                let mut blocks = Vec::with_capacity(block_rows.len());
                for (id, position, correct, hint) in block_rows {
                    let options = sqlx::query_as::<_, (Uuid, String, bool)>(
                        "SELECT id,text,is_correct FROM options
                         WHERE user_id=$1 AND block_id=$2 AND deleted_at IS NULL
                         ORDER BY is_correct DESC,id",
                    )
                    .bind(user_id)
                    .bind(id)
                    .fetch_all(database)
                    .await?
                    .into_iter()
                    .map(|(id, text, is_correct)| ExerciseOptionResponse {
                        id,
                        text,
                        is_correct,
                    })
                    .collect();
                    blocks.push(ExerciseBlockResponse {
                        id,
                        position,
                        correct,
                        hint,
                        options,
                    });
                }
                Some(PreparationResponse {
                    id: preparation_id,
                    version,
                    model,
                    translation,
                    blocks,
                })
            }
            None => None,
        };
        languages.push(SentenceLanguageResponse {
            target_language,
            status,
            error,
            translation_comment,
            audio_available,
            audio: match (audio_sha256, audio_size, audio_mime) {
                (Some(sha256), Some(size), Some(mime)) => {
                    Some(langai_contracts::AudioMetadataResponse {
                        sha256,
                        size,
                        mime,
                        name: audio_name,
                    })
                }
                _ => None,
            },
            revision,
            active_preparation,
        });
    }
    let topics = sqlx::query_scalar::<_, String>(
        "SELECT t.name FROM topics t JOIN sentence_topics st
           ON st.user_id=t.user_id AND st.topic_id=t.id
         WHERE st.user_id=$1 AND st.sentence_id=$2
         AND st.deleted_at IS NULL AND t.deleted_at IS NULL ORDER BY t.name",
    )
    .bind(user_id)
    .bind(sentence_id)
    .fetch_all(database)
    .await?;
    Ok(SentenceResponse {
        id: sentence_id,
        source_text,
        created_at,
        revision,
        languages,
        topics,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_lists_are_trimmed_and_case_insensitively_deduplicated() {
        assert_eq!(
            unique_texts(
                vec![" English ".into(), "english".into(), "German".into()],
                "Language",
                100
            )
            .unwrap(),
            vec!["English", "German"]
        );
    }
}
