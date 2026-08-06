use std::collections::{HashMap, HashSet};

use axum::{
    extract::{Query, State},
    http::HeaderMap,
    Json,
};
use langai_contracts::{
    SyncChangeResponse, SyncOperationResult, SyncPullResponse, SyncPushOperation, SyncPushRequest,
    SyncPushResponse, UpdateSettingsRequest,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{
    auth::{authenticate_request, require_mutation_auth, AuthContext},
    domain::load_sentence,
    error::ApiError,
    state::AppState,
};

const MAX_PUSH_OPERATIONS: usize = 100;
const MAX_PULL_CHANGES: i64 = 500;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullQuery {
    #[serde(default)]
    cursor: i64,
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SentencePayload {
    source_text: String,
    created_at: Option<String>,
    created_order: Option<i64>,
    target_languages: Vec<String>,
    #[serde(default)]
    topics: Vec<String>,
    #[serde(default)]
    preparations: Vec<ImportedPreparation>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportedPreparation {
    target_language: String,
    version: i32,
    model: String,
    translation: String,
    active: bool,
    blocks: Vec<ImportedBlock>,
}

#[derive(Debug, Deserialize)]
struct ImportedBlock {
    position: i32,
    correct: String,
    hint: Option<String>,
    distractors: Vec<String>,
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
    let mut output = Vec::new();
    for value in values {
        let value = required_text(&value, label, max)?;
        if seen.insert(value.to_lowercase()) {
            output.push(value);
        }
    }
    Ok(output)
}

async fn record_sentence_change(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    sentence_id: Uuid,
    operation: &str,
    revision: i64,
) -> Result<(), ApiError> {
    sqlx::query(
        "INSERT INTO sync_changes(user_id,entity_type,entity_id,operation,revision)
         VALUES($1,'sentence',$2,$3,$4)",
    )
    .bind(user_id)
    .bind(sentence_id)
    .bind(operation)
    .bind(revision)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn upsert_sentence(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    operation: &SyncPushOperation,
) -> Result<i64, ApiError> {
    let payload: SentencePayload = serde_json::from_value(
        operation
            .payload
            .clone()
            .ok_or_else(|| ApiError::InvalidInput("Sentence payload is required".into()))?,
    )
    .map_err(|_| ApiError::InvalidInput("Invalid sentence payload".into()))?;
    let source = required_text(&payload.source_text, "Source text", 10_000)?;
    let languages = unique_texts(payload.target_languages, "Target language", 100)?;
    if languages.is_empty() {
        return Err(ApiError::InvalidInput(
            "At least one target language is required".into(),
        ));
    }
    let topics = unique_texts(payload.topics, "Topic", 100)?;
    let revision = sqlx::query_scalar::<_, i64>(
        "INSERT INTO sentences(id,user_id,source_text,created_at,sync_order)
         VALUES($1,$2,$3,COALESCE($4::timestamptz,now()),COALESCE($5,0))
         ON CONFLICT(id) DO UPDATE SET source_text=excluded.source_text,deleted_at=NULL,
           created_at=excluded.created_at,sync_order=excluded.sync_order,
           updated_at=now(),revision=sentences.revision+1
         WHERE sentences.user_id=excluded.user_id
         RETURNING revision",
    )
    .bind(operation.entity_id)
    .bind(user_id)
    .bind(source)
    .bind(payload.created_at)
    .bind(payload.created_order)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(ApiError::Forbidden)?;

    for language in &languages {
        sqlx::query(
            "INSERT INTO sentence_languages(user_id,sentence_id,target_language)
             VALUES($1,$2,$3)
             ON CONFLICT(user_id,sentence_id,target_language) DO UPDATE SET
               deleted_at=NULL,updated_at=now(),revision=sentence_languages.revision+1",
        )
        .bind(user_id)
        .bind(operation.entity_id)
        .bind(language)
        .execute(&mut **tx)
        .await?;
    }
    sqlx::query(
        "UPDATE sentence_languages SET deleted_at=now(),updated_at=now(),revision=revision+1
         WHERE user_id=$1 AND sentence_id=$2 AND deleted_at IS NULL
         AND NOT(target_language=ANY($3))",
    )
    .bind(user_id)
    .bind(operation.entity_id)
    .bind(&languages)
    .execute(&mut **tx)
    .await?;

    sqlx::query(
        "UPDATE sentence_topics SET deleted_at=now(),revision=revision+1
         WHERE user_id=$1 AND sentence_id=$2 AND deleted_at IS NULL",
    )
    .bind(user_id)
    .bind(operation.entity_id)
    .execute(&mut **tx)
    .await?;
    for topic in topics {
        let proposed_id = Uuid::new_v4();
        sqlx::query("INSERT INTO topics(id,user_id,name) VALUES($1,$2,$3) ON CONFLICT DO NOTHING")
            .bind(proposed_id)
            .bind(user_id)
            .bind(&topic)
            .execute(&mut **tx)
            .await?;
        let topic_id = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM topics WHERE user_id=$1 AND lower(name)=lower($2)
             AND deleted_at IS NULL",
        )
        .bind(user_id)
        .bind(topic)
        .fetch_one(&mut **tx)
        .await?;
        sqlx::query(
            "INSERT INTO sentence_topics(user_id,sentence_id,topic_id)
             VALUES($1,$2,$3)
             ON CONFLICT(user_id,sentence_id,topic_id) DO UPDATE SET
               deleted_at=NULL,revision=sentence_topics.revision+1",
        )
        .bind(user_id)
        .bind(operation.entity_id)
        .bind(topic_id)
        .execute(&mut **tx)
        .await?;
    }
    for preparation in payload.preparations {
        let target_language = required_text(&preparation.target_language, "Target language", 100)?;
        let model = required_text(&preparation.model, "Model", 100)?;
        let translation = required_text(&preparation.translation, "Translation", 1000)?;
        if preparation.version < 1 || preparation.blocks.is_empty() || preparation.blocks.len() > 50
        {
            return Err(ApiError::InvalidInput(
                "Invalid imported preparation".into(),
            ));
        }
        let proposed_id = Uuid::new_v4();
        let inserted = sqlx::query(
            "INSERT INTO preparations(
               id,user_id,sentence_id,version,target_language,model,translation
             ) VALUES($1,$2,$3,$4,$5,$6,$7)
             ON CONFLICT(user_id,sentence_id,target_language,version) DO NOTHING",
        )
        .bind(proposed_id)
        .bind(user_id)
        .bind(operation.entity_id)
        .bind(preparation.version)
        .bind(&target_language)
        .bind(model)
        .bind(translation)
        .execute(&mut **tx)
        .await?;
        let preparation_id = if inserted.rows_affected() == 1 {
            for block in preparation.blocks {
                if block.position < 0
                    || block.correct.trim().is_empty()
                    || block.correct.chars().count() > 200
                    // Older desktop releases generated four distractors. Accept
                    // both persisted formats during the initial cloud import;
                    // newly generated preparations still use exactly three.
                    || !(3..=4).contains(&block.distractors.len())
                {
                    return Err(ApiError::InvalidInput("Invalid imported block".into()));
                }
                let block_id = Uuid::new_v4();
                sqlx::query(
                    "INSERT INTO blocks(id,user_id,preparation_id,position,correct,hint)
                     VALUES($1,$2,$3,$4,$5,$6)",
                )
                .bind(block_id)
                .bind(user_id)
                .bind(proposed_id)
                .bind(block.position)
                .bind(&block.correct)
                .bind(&block.hint)
                .execute(&mut **tx)
                .await?;
                for (text, is_correct) in std::iter::once((block.correct, true))
                    .chain(block.distractors.into_iter().map(|text| (text, false)))
                {
                    let text = required_text(&text, "Option", 200)?;
                    sqlx::query(
                        "INSERT INTO options(id,user_id,block_id,text,is_correct)
                         VALUES($1,$2,$3,$4,$5)",
                    )
                    .bind(Uuid::new_v4())
                    .bind(user_id)
                    .bind(block_id)
                    .bind(text)
                    .bind(is_correct)
                    .execute(&mut **tx)
                    .await?;
                }
            }
            proposed_id
        } else {
            sqlx::query_scalar::<_, Uuid>(
                "SELECT id FROM preparations WHERE user_id=$1 AND sentence_id=$2
                 AND target_language=$3 AND version=$4",
            )
            .bind(user_id)
            .bind(operation.entity_id)
            .bind(&target_language)
            .bind(preparation.version)
            .fetch_one(&mut **tx)
            .await?
        };
        if preparation.active {
            sqlx::query(
                "UPDATE sentence_languages SET active_preparation_id=$1,status='ready',
                 error=NULL,updated_at=now(),revision=revision+1
                 WHERE user_id=$2 AND sentence_id=$3 AND target_language=$4",
            )
            .bind(preparation_id)
            .bind(user_id)
            .bind(operation.entity_id)
            .bind(target_language)
            .execute(&mut **tx)
            .await?;
        }
    }
    record_sentence_change(tx, user_id, operation.entity_id, "upsert", revision).await?;
    Ok(revision)
}

async fn delete_sentence(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    sentence_id: Uuid,
) -> Result<i64, ApiError> {
    let revision = sqlx::query_scalar::<_, i64>(
        "UPDATE sentences SET deleted_at=COALESCE(deleted_at,now()),updated_at=now(),
           revision=revision+1 WHERE id=$1 AND user_id=$2 RETURNING revision",
    )
    .bind(sentence_id)
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await?
    .unwrap_or(1);
    if revision == 1 {
        sqlx::query(
            "INSERT INTO sentences(id,user_id,source_text,deleted_at)
             VALUES($1,$2,'Deleted offline sentence',now()) ON CONFLICT DO NOTHING",
        )
        .bind(sentence_id)
        .bind(user_id)
        .execute(&mut **tx)
        .await?;
    }
    record_sentence_change(tx, user_id, sentence_id, "delete", revision).await?;
    Ok(revision)
}

async fn upsert_settings(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    operation: &SyncPushOperation,
) -> Result<i64, ApiError> {
    let payload: UpdateSettingsRequest = serde_json::from_value(
        operation
            .payload
            .clone()
            .ok_or_else(|| ApiError::InvalidInput("Settings payload is required".into()))?,
    )
    .map_err(|_| ApiError::InvalidInput("Invalid settings payload".into()))?;
    let model = required_text(&payload.model, "Model", 100)?;
    let language = required_text(&payload.target_language, "Target language", 100)?;
    let revision = sqlx::query_scalar::<_, i64>(
        "INSERT INTO user_settings(
           user_id,model,target_language,elevenlabs_voice_id,elevenlabs_voice_name
         ) VALUES($1,$2,$3,$4,$5)
         ON CONFLICT(user_id) DO UPDATE SET model=excluded.model,
           target_language=excluded.target_language,
           elevenlabs_voice_id=excluded.elevenlabs_voice_id,
           elevenlabs_voice_name=excluded.elevenlabs_voice_name,
           updated_at=now(),revision=user_settings.revision+1 RETURNING revision",
    )
    .bind(user_id)
    .bind(model)
    .bind(language)
    .bind(payload.elevenlabs_voice_id)
    .bind(payload.elevenlabs_voice_name)
    .fetch_one(&mut **tx)
    .await?;
    sqlx::query(
        "INSERT INTO sync_changes(user_id,entity_type,entity_id,operation,revision)
         VALUES($1,'settings',$1,'upsert',$2)",
    )
    .bind(user_id)
    .bind(revision)
    .execute(&mut **tx)
    .await?;
    Ok(revision)
}

async fn apply_operation(
    tx: &mut Transaction<'_, Postgres>,
    auth: &AuthContext,
    operation: &SyncPushOperation,
) -> Result<SyncOperationResult, ApiError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("{}:{}", auth.user_id, operation.operation_id))
        .execute(&mut **tx)
        .await?;
    if let Some(stored) = sqlx::query_scalar::<_, Value>(
        "SELECT response FROM sync_operations WHERE user_id=$1 AND operation_id=$2",
    )
    .bind(auth.user_id)
    .bind(operation.operation_id)
    .fetch_optional(&mut **tx)
    .await?
    {
        let mut result: SyncOperationResult =
            serde_json::from_value(stored).map_err(|_| ApiError::Internal)?;
        result.replayed = true;
        return Ok(result);
    }
    if operation.base_revision.is_some_and(|revision| revision < 0) {
        return Err(ApiError::InvalidInput(
            "baseRevision cannot be negative".into(),
        ));
    }
    let revision = match operation.kind.as_str() {
        "sentence.upsert" => upsert_sentence(tx, auth.user_id, operation).await?,
        "sentence.delete" => delete_sentence(tx, auth.user_id, operation.entity_id).await?,
        "settings.upsert" => upsert_settings(tx, auth.user_id, operation).await?,
        _ => return Err(ApiError::InvalidInput("Unknown sync operation kind".into())),
    };
    let result = SyncOperationResult {
        operation_id: operation.operation_id,
        entity_id: operation.entity_id,
        revision,
        replayed: false,
    };
    sqlx::query(
        "INSERT INTO sync_operations(user_id,operation_id,device_id,response)
         VALUES($1,$2,$3,$4)",
    )
    .bind(auth.user_id)
    .bind(operation.operation_id)
    .bind(auth.device_id)
    .bind(serde_json::to_value(&result).map_err(|_| ApiError::Internal)?)
    .execute(&mut **tx)
    .await?;
    Ok(result)
}

pub async fn push(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<SyncPushRequest>,
) -> Result<Json<SyncPushResponse>, ApiError> {
    let auth = authenticate_request(&state, &headers).await?;
    require_mutation_auth(&state, &headers, &auth)?;
    if input.operations.is_empty() || input.operations.len() > MAX_PUSH_OPERATIONS {
        return Err(ApiError::InvalidInput(format!(
            "A sync batch must contain 1 to {MAX_PUSH_OPERATIONS} operations"
        )));
    }
    let unique = input
        .operations
        .iter()
        .map(|operation| operation.operation_id)
        .collect::<HashSet<_>>();
    if unique.len() != input.operations.len() {
        return Err(ApiError::InvalidInput(
            "A sync batch contains duplicate operation IDs".into(),
        ));
    }
    let mut tx = state.database.begin().await?;
    let mut results = Vec::with_capacity(input.operations.len());
    for operation in &input.operations {
        results.push(apply_operation(&mut tx, &auth, operation).await?);
    }
    tx.commit().await?;
    Ok(Json(SyncPushResponse { results }))
}

async fn settings_payload(state: &AppState, user_id: Uuid) -> Result<Option<Value>, ApiError> {
    let row = sqlx::query_as::<_, (String, String, Option<String>, Option<String>, i64)>(
        "SELECT model,target_language,elevenlabs_voice_id,elevenlabs_voice_name,revision
         FROM user_settings WHERE user_id=$1",
    )
    .bind(user_id)
    .fetch_optional(&state.database)
    .await?;
    Ok(row.map(|row| {
        json!({
            "model": row.0,
            "targetLanguage": row.1,
            "elevenlabsVoiceId": row.2,
            "elevenlabsVoiceName": row.3,
            "revision": row.4
        })
    }))
}

pub async fn pull(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<PullQuery>,
) -> Result<Json<SyncPullResponse>, ApiError> {
    let auth = authenticate_request(&state, &headers).await?;
    if query.cursor < 0 {
        return Err(ApiError::InvalidInput("Cursor cannot be negative".into()));
    }
    let limit = query.limit.unwrap_or(MAX_PULL_CHANGES);
    if !(1..=MAX_PULL_CHANGES).contains(&limit) {
        return Err(ApiError::InvalidInput(format!(
            "Pull limit must be between 1 and {MAX_PULL_CHANGES}"
        )));
    }
    let rows = sqlx::query_as::<_, (i64, String, Uuid, String, i64)>(
        "SELECT sequence,entity_type,entity_id,operation,revision FROM sync_changes
         WHERE user_id=$1 AND sequence>$2 ORDER BY sequence LIMIT $3",
    )
    .bind(auth.user_id)
    .bind(query.cursor)
    .bind(limit + 1)
    .fetch_all(&state.database)
    .await?;
    let has_more = rows.len() as i64 > limit;
    let visible = rows.into_iter().take(limit as usize).collect::<Vec<_>>();
    let cursor = visible.last().map(|row| row.0).unwrap_or(query.cursor);

    // Several child changes can point to one sentence. Return only its latest state in this page.
    let mut latest: HashMap<(String, Uuid), (i64, String, i64)> = HashMap::new();
    for (sequence, entity_type, entity_id, operation, revision) in visible {
        let normalized =
            if entity_type == "sentence" || entity_type.starts_with("sentence_language") {
                "sentence"
            } else {
                entity_type.as_str()
            };
        latest.insert(
            (normalized.to_owned(), entity_id),
            (sequence, operation, revision),
        );
    }
    let mut changes = Vec::with_capacity(latest.len());
    for ((entity_type, entity_id), (sequence, operation, revision)) in latest {
        let payload = match entity_type.as_str() {
            "sentence" => match load_sentence(&state.database, auth.user_id, entity_id).await {
                Ok(sentence) => {
                    Some(serde_json::to_value(sentence).map_err(|_| ApiError::Internal)?)
                }
                Err(ApiError::NotFound) => None,
                Err(error) => return Err(error),
            },
            "settings" => settings_payload(&state, auth.user_id).await?,
            _ => continue,
        };
        changes.push(SyncChangeResponse {
            sequence,
            entity_type,
            entity_id,
            operation: if payload.is_none() {
                "delete".into()
            } else {
                operation
            },
            revision,
            payload,
        });
    }
    changes.sort_by_key(|change| change.sequence);
    Ok(Json(SyncPullResponse {
        cursor,
        has_more,
        changes,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_payload_texts_are_normalized_and_deduplicated() {
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
