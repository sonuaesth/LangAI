use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::Row;
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

use crate::{
    error::{AppError, Result},
    secrets, AppState,
};
use langai_contracts::{SentenceResponse, SettingsResponse};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    connected: bool,
    server_url: Option<String>,
    user_id: Option<String>,
    device_id: Option<String>,
    status: String,
    pending_operations: i64,
    last_error: Option<String>,
    last_sync_at: Option<String>,
    initial_upload_completed: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeviceLoginRequest<'a> {
    email: &'a str,
    password: &'a str,
    device_name: &'a str,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceLoginResponse {
    device_id: Uuid,
    token: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionResponse {
    authenticated: bool,
    user: Option<SessionUser>,
}

#[derive(Deserialize)]
struct SessionUser {
    id: Uuid,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PushRequest {
    operations: Vec<PushOperation>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PushOperation {
    operation_id: Uuid,
    kind: String,
    entity_id: Uuid,
    base_revision: Option<i64>,
    payload: Option<serde_json::Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PushResponse {
    results: Vec<PushResult>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PushResult {
    operation_id: Uuid,
    entity_id: Uuid,
    revision: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PullResponse {
    cursor: i64,
    has_more: bool,
    changes: Vec<PullChange>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PullChange {
    entity_type: String,
    entity_id: Uuid,
    operation: String,
    revision: i64,
    payload: Option<serde_json::Value>,
}

fn normalize_server_url(value: &str) -> Result<String> {
    let value = value.trim().trim_end_matches('/');
    let local = value.starts_with("http://127.0.0.1")
        || value.starts_with("http://localhost")
        || value.starts_with("http://[::1]");
    if (!value.starts_with("https://") && !local) || value.len() > 2048 {
        return Err(AppError::Input(
            "Server URL must use HTTPS (HTTP is allowed only for localhost)".into(),
        ));
    }
    Ok(value.to_owned())
}

async fn api_error(response: reqwest::Response) -> AppError {
    let status = response.status();
    let message = response
        .text()
        .await
        .ok()
        .and_then(|body| serde_json::from_str::<serde_json::Value>(&body).ok())
        .and_then(|body| body["error"]["message"].as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("Server returned {status}"));
    AppError::Sync(message)
}

async fn upload_provider_key(
    client: &reqwest::Client,
    base_url: &str,
    token: &str,
    provider: &str,
    key: Option<String>,
) -> Result<()> {
    let request = match key {
        Some(key) => client
            .put(format!("{base_url}/api/v1/provider-keys/{provider}"))
            .bearer_auth(token)
            .json(&json!({ "apiKey": key })),
        None => client
            .delete(format!("{base_url}/api/v1/provider-keys/{provider}"))
            .bearer_auth(token),
    };
    let response = request.send().await?;
    if !response.status().is_success() {
        return Err(api_error(response).await);
    }
    Ok(())
}

async fn remote_id(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    entity_type: &str,
    local_key: &str,
) -> Result<Uuid> {
    if let Some(value) = sqlx::query_scalar::<_, String>(
        "SELECT remote_id FROM sync_entity_ids WHERE entity_type=? AND local_key=?",
    )
    .bind(entity_type)
    .bind(local_key)
    .fetch_optional(&mut **tx)
    .await?
    {
        return Uuid::parse_str(&value)
            .map_err(|_| AppError::Sync("Local sync mapping is corrupted".into()));
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO sync_entity_ids(entity_type,local_key,remote_id) VALUES(?,?,?)")
        .bind(entity_type)
        .bind(local_key)
        .bind(id.to_string())
        .execute(&mut **tx)
        .await?;
    Ok(id)
}

async fn enqueue_initial_snapshot(state: &AppState) -> Result<()> {
    let mut tx = state.db.begin().await?;
    let already_enqueued: i64 = sqlx::query_scalar("SELECT count(*) FROM sync_outbox")
        .fetch_one(&mut *tx)
        .await?;
    if already_enqueued > 0 {
        tx.commit().await?;
        return Ok(());
    }
    let settings = sqlx::query(
        "SELECT model,target_language,elevenlabs_voice_id,elevenlabs_voice_name
         FROM settings WHERE id=1",
    )
    .fetch_one(&mut *tx)
    .await?;
    let settings_operation = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO sync_outbox(operation_id,kind,entity_id,payload)
         VALUES(?,'settings.upsert',?,?)",
    )
    .bind(settings_operation.to_string())
    .bind(Uuid::nil().to_string())
    .bind(
        json!({
            "model": settings.get::<String, _>(0),
            "targetLanguage": settings.get::<String, _>(1),
            "elevenlabsVoiceId": settings.get::<Option<String>, _>(2),
            "elevenlabsVoiceName": settings.get::<Option<String>, _>(3)
        })
        .to_string(),
    )
    .execute(&mut *tx)
    .await?;

    let sentences = sqlx::query("SELECT id,source_text FROM sentences ORDER BY id")
        .fetch_all(&mut *tx)
        .await?;
    for sentence in sentences {
        let local_id = sentence.get::<i64, _>(0);
        let entity_id = remote_id(&mut tx, "sentence", &local_id.to_string()).await?;
        let languages = sqlx::query_scalar::<_, String>(
            "SELECT target_language FROM sentence_languages
             WHERE sentence_id=? ORDER BY target_language",
        )
        .bind(local_id)
        .fetch_all(&mut *tx)
        .await?;
        let topics = sqlx::query_scalar::<_, String>(
            "SELECT t.name FROM topics t JOIN sentence_topics st ON st.topic_id=t.id
             WHERE st.sentence_id=? ORDER BY t.name",
        )
        .bind(local_id)
        .fetch_all(&mut *tx)
        .await?;
        let preparation_rows = sqlx::query(
            "SELECT p.id,p.version,p.target_language,p.model,p.translation,
             EXISTS(
               SELECT 1 FROM sentence_languages sl
               WHERE sl.sentence_id=p.sentence_id
               AND sl.target_language=p.target_language
               AND sl.active_preparation_id=p.id
             )
             FROM preparations p WHERE p.sentence_id=? ORDER BY p.version",
        )
        .bind(local_id)
        .fetch_all(&mut *tx)
        .await?;
        let mut preparations = Vec::with_capacity(preparation_rows.len());
        for preparation in preparation_rows {
            let preparation_id = preparation.get::<i64, _>(0);
            let block_rows = sqlx::query(
                "SELECT id,position,correct,hint FROM blocks
                 WHERE preparation_id=? ORDER BY position",
            )
            .bind(preparation_id)
            .fetch_all(&mut *tx)
            .await?;
            let mut blocks = Vec::with_capacity(block_rows.len());
            for block in block_rows {
                let block_id = block.get::<i64, _>(0);
                let distractors = sqlx::query_scalar::<_, String>(
                    "SELECT text FROM options WHERE block_id=? AND is_correct=0 ORDER BY id",
                )
                .bind(block_id)
                .fetch_all(&mut *tx)
                .await?;
                blocks.push(json!({
                    "position": block.get::<i64, _>(1),
                    "correct": block.get::<String, _>(2),
                    "hint": block.get::<Option<String>, _>(3),
                    "distractors": distractors
                }));
            }
            preparations.push(json!({
                "version": preparation.get::<i64, _>(1),
                "targetLanguage": preparation.get::<String, _>(2),
                "model": preparation.get::<String, _>(3),
                "translation": preparation.get::<String, _>(4),
                "active": preparation.get::<i64, _>(5) == 1,
                "blocks": blocks
            }));
        }
        sqlx::query(
            "INSERT INTO sync_outbox(operation_id,kind,entity_id,payload)
             VALUES(?,'sentence.upsert',?,?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(entity_id.to_string())
        .bind(
            json!({
                "sourceText": sentence.get::<String, _>(1),
                "targetLanguages": languages,
                "topics": topics,
                "preparations": preparations
            })
            .to_string(),
        )
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query("UPDATE sync_state SET status='pending',last_error=NULL WHERE id=1")
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

async fn sync_connected(db: &sqlx::SqlitePool) -> Result<bool> {
    Ok(
        sqlx::query_scalar::<_, Option<String>>("SELECT server_url FROM sync_state WHERE id=1")
            .fetch_one(db)
            .await?
            .is_some(),
    )
}

async fn sentence_payload(db: &sqlx::SqlitePool, sentence_id: i64) -> Result<serde_json::Value> {
    let sentence = sqlx::query("SELECT source_text FROM sentences WHERE id=?")
        .bind(sentence_id)
        .fetch_optional(db)
        .await?
        .ok_or_else(|| AppError::Sync("Sentence was removed before it could be queued".into()))?;
    let languages = sqlx::query_scalar::<_, String>(
        "SELECT target_language FROM sentence_languages
         WHERE sentence_id=? ORDER BY target_language",
    )
    .bind(sentence_id)
    .fetch_all(db)
    .await?;
    let topics = sqlx::query_scalar::<_, String>(
        "SELECT t.name FROM topics t JOIN sentence_topics st ON st.topic_id=t.id
         WHERE st.sentence_id=? ORDER BY t.name",
    )
    .bind(sentence_id)
    .fetch_all(db)
    .await?;
    let preparation_rows = sqlx::query(
        "SELECT p.id,p.version,p.target_language,p.model,p.translation,
         EXISTS(
           SELECT 1 FROM sentence_languages sl
           WHERE sl.sentence_id=p.sentence_id AND sl.target_language=p.target_language
           AND sl.active_preparation_id=p.id
         )
         FROM preparations p WHERE p.sentence_id=? ORDER BY p.version",
    )
    .bind(sentence_id)
    .fetch_all(db)
    .await?;
    let mut preparations = Vec::with_capacity(preparation_rows.len());
    for preparation in preparation_rows {
        let preparation_id = preparation.get::<i64, _>(0);
        let block_rows = sqlx::query(
            "SELECT id,position,correct,hint FROM blocks
             WHERE preparation_id=? ORDER BY position",
        )
        .bind(preparation_id)
        .fetch_all(db)
        .await?;
        let mut blocks = Vec::with_capacity(block_rows.len());
        for block in block_rows {
            let distractors = sqlx::query_scalar::<_, String>(
                "SELECT text FROM options WHERE block_id=? AND is_correct=0 ORDER BY id",
            )
            .bind(block.get::<i64, _>(0))
            .fetch_all(db)
            .await?;
            blocks.push(json!({
                "position": block.get::<i64, _>(1),
                "correct": block.get::<String, _>(2),
                "hint": block.get::<Option<String>, _>(3),
                "distractors": distractors
            }));
        }
        preparations.push(json!({
            "version": preparation.get::<i64, _>(1),
            "targetLanguage": preparation.get::<String, _>(2),
            "model": preparation.get::<String, _>(3),
            "translation": preparation.get::<String, _>(4),
            "active": preparation.get::<i64, _>(5) == 1,
            "blocks": blocks
        }));
    }
    Ok(json!({
        "sourceText": sentence.get::<String, _>(0),
        "targetLanguages": languages,
        "topics": topics,
        "preparations": preparations
    }))
}

pub async fn enqueue_sentence_upsert(db: &sqlx::SqlitePool, sentence_id: i64) -> Result<()> {
    if !sync_connected(db).await? {
        return Ok(());
    }
    let mut tx = db.begin().await?;
    let entity_id = remote_id(&mut tx, "sentence", &sentence_id.to_string()).await?;
    let base_revision = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT server_revision FROM sync_entity_ids
         WHERE entity_type='sentence' AND local_key=?",
    )
    .bind(sentence_id.to_string())
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    let payload = sentence_payload(db, sentence_id).await?;
    sqlx::query(
        "INSERT INTO sync_outbox(
           operation_id,kind,entity_id,base_revision,payload
         ) VALUES(?,'sentence.upsert',?,?,?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(entity_id.to_string())
    .bind(base_revision)
    .bind(payload.to_string())
    .execute(db)
    .await?;
    sqlx::query("UPDATE sync_state SET status='pending' WHERE id=1")
        .execute(db)
        .await?;
    Ok(())
}

pub async fn enqueue_sentence_delete(db: &sqlx::SqlitePool, sentence_id: i64) -> Result<()> {
    if !sync_connected(db).await? {
        return Ok(());
    }
    let mut tx = db.begin().await?;
    let entity_id = remote_id(&mut tx, "sentence", &sentence_id.to_string()).await?;
    let base_revision = sqlx::query_scalar::<_, Option<i64>>(
        "SELECT server_revision FROM sync_entity_ids
         WHERE entity_type='sentence' AND local_key=?",
    )
    .bind(sentence_id.to_string())
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO sync_outbox(operation_id,kind,entity_id,base_revision)
         VALUES(?,'sentence.delete',?,?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(entity_id.to_string())
    .bind(base_revision)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE sync_state SET status='pending' WHERE id=1")
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn enqueue_settings(db: &sqlx::SqlitePool) -> Result<()> {
    if !sync_connected(db).await? {
        return Ok(());
    }
    let row = sqlx::query(
        "SELECT model,target_language,elevenlabs_voice_id,elevenlabs_voice_name
         FROM settings WHERE id=1",
    )
    .fetch_one(db)
    .await?;
    let payload = json!({
        "model": row.get::<String, _>(0),
        "targetLanguage": row.get::<String, _>(1),
        "elevenlabsVoiceId": row.get::<Option<String>, _>(2),
        "elevenlabsVoiceName": row.get::<Option<String>, _>(3)
    });
    sqlx::query(
        "INSERT INTO sync_outbox(operation_id,kind,entity_id,payload)
         VALUES(?,'settings.upsert',?,?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(Uuid::nil().to_string())
    .bind(payload.to_string())
    .execute(db)
    .await?;
    sqlx::query("UPDATE sync_state SET status='pending' WHERE id=1")
        .execute(db)
        .await?;
    Ok(())
}

#[tauri::command]
pub async fn connect_sync_account(
    server_url: String,
    email: String,
    password: String,
    device_name: String,
    state: State<'_, AppState>,
) -> Result<SyncStatus> {
    let base_url = normalize_server_url(&server_url)?;
    if email.trim().is_empty() || password.is_empty() || device_name.trim().is_empty() {
        return Err(AppError::Input(
            "Email, password and device name are required".into(),
        ));
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let response = client
        .post(format!("{base_url}/api/v1/auth/device-token"))
        .json(&DeviceLoginRequest {
            email: email.trim(),
            password: &password,
            device_name: device_name.trim(),
        })
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(api_error(response).await);
    }
    let login: DeviceLoginResponse = response.json().await?;
    let response = client
        .get(format!("{base_url}/api/v1/auth/device-session"))
        .bearer_auth(&login.token)
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(api_error(response).await);
    }
    let session: SessionResponse = response.json().await?;
    let user_id = session
        .authenticated
        .then_some(session.user)
        .flatten()
        .ok_or_else(|| AppError::Sync("Server did not create a session".into()))?
        .id;

    upload_provider_key(&client, &base_url, &login.token, "openai", secrets::get()?).await?;
    upload_provider_key(
        &client,
        &base_url,
        &login.token,
        "elevenlabs",
        secrets::get_elevenlabs()?,
    )
    .await?;
    secrets::set_sync_token(&login.token)?;

    let previous_user =
        sqlx::query_scalar::<_, Option<String>>("SELECT user_id FROM sync_state WHERE id=1")
            .fetch_one(&state.db)
            .await?;
    let mut tx = state.db.begin().await?;
    if previous_user
        .as_deref()
        .is_some_and(|value| value != user_id.to_string())
    {
        sqlx::query("DELETE FROM sync_outbox")
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM sync_entity_ids")
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query(
        "UPDATE sync_state SET server_url=?,user_id=?,device_id=?,pull_cursor=0,
         status='pending',last_error=NULL,initial_upload_completed=0 WHERE id=1",
    )
    .bind(&base_url)
    .bind(user_id.to_string())
    .bind(login.device_id.to_string())
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    enqueue_initial_snapshot(&state).await?;
    sync_status(state).await
}

#[tauri::command]
pub async fn sync_status(state: State<'_, AppState>) -> Result<SyncStatus> {
    let row = sqlx::query(
        "SELECT server_url,user_id,device_id,status,last_error,last_sync_at,
         initial_upload_completed FROM sync_state WHERE id=1",
    )
    .fetch_one(&state.db)
    .await?;
    let pending_operations: i64 = sqlx::query_scalar("SELECT count(*) FROM sync_outbox")
        .fetch_one(&state.db)
        .await?;
    let server_url = row.get::<Option<String>, _>(0);
    Ok(SyncStatus {
        connected: server_url.is_some() && secrets::get_sync_token()?.is_some(),
        server_url,
        user_id: row.get(1),
        device_id: row.get(2),
        status: row.get(3),
        pending_operations,
        last_error: row.get(4),
        last_sync_at: row.get(5),
        initial_upload_completed: row.get::<i64, _>(6) == 1,
    })
}

#[tauri::command]
pub async fn disconnect_sync_account(state: State<'_, AppState>) -> Result<SyncStatus> {
    secrets::delete_sync_token()?;
    sqlx::query("UPDATE sync_state SET status='disconnected',last_error=NULL WHERE id=1")
        .execute(&state.db)
        .await?;
    sync_status(state).await
}

async fn mark_sync_failure(state: &AppState, status: &str, error: &str) {
    let _ = sqlx::query("UPDATE sync_state SET status=?,last_error=? WHERE id=1")
        .bind(status)
        .bind(error)
        .execute(&state.db)
        .await;
}

async fn apply_settings(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    payload: serde_json::Value,
) -> Result<()> {
    let settings: SettingsResponse = serde_json::from_value(payload)
        .map_err(|_| AppError::Sync("Invalid settings received from server".into()))?;
    sqlx::query(
        "UPDATE settings SET model=?,target_language=?,elevenlabs_voice_id=?,
         elevenlabs_voice_name=? WHERE id=1",
    )
    .bind(settings.model)
    .bind(settings.target_language)
    .bind(settings.elevenlabs_voice_id)
    .bind(settings.elevenlabs_voice_name)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn sentence_local_id(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    remote_id: Uuid,
    sentence: &SentenceResponse,
) -> Result<i64> {
    if let Some(local_key) = sqlx::query_scalar::<_, String>(
        "SELECT local_key FROM sync_entity_ids WHERE entity_type='sentence' AND remote_id=?",
    )
    .bind(remote_id.to_string())
    .fetch_optional(&mut **tx)
    .await?
    {
        return local_key
            .parse::<i64>()
            .map_err(|_| AppError::Sync("Sentence sync mapping is corrupted".into()));
    }
    let default_language = sentence
        .languages
        .first()
        .map(|language| language.target_language.as_str())
        .unwrap_or("English");
    let local_id = sqlx::query("INSERT INTO sentences(source_text,target_language) VALUES(?,?)")
        .bind(&sentence.source_text)
        .bind(default_language)
        .execute(&mut **tx)
        .await?
        .last_insert_rowid();
    sqlx::query(
        "INSERT INTO sync_entity_ids(entity_type,local_key,remote_id,server_revision)
         VALUES('sentence',?,?,?)",
    )
    .bind(local_id.to_string())
    .bind(remote_id.to_string())
    .bind(sentence.revision)
    .execute(&mut **tx)
    .await?;
    Ok(local_id)
}

async fn apply_sentence(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    remote_id: Uuid,
    revision: i64,
    payload: serde_json::Value,
) -> Result<()> {
    let sentence: SentenceResponse = serde_json::from_value(payload)
        .map_err(|_| AppError::Sync("Invalid sentence received from server".into()))?;
    let local_id = sentence_local_id(tx, remote_id, &sentence).await?;
    sqlx::query("UPDATE sentences SET source_text=? WHERE id=?")
        .bind(&sentence.source_text)
        .bind(local_id)
        .execute(&mut **tx)
        .await?;

    sqlx::query("DELETE FROM sentence_topics WHERE sentence_id=?")
        .bind(local_id)
        .execute(&mut **tx)
        .await?;
    for topic in &sentence.topics {
        sqlx::query("INSERT INTO topics(name) VALUES(?) ON CONFLICT(name) DO NOTHING")
            .bind(topic)
            .execute(&mut **tx)
            .await?;
        sqlx::query(
            "INSERT OR IGNORE INTO sentence_topics(sentence_id,topic_id)
             SELECT ?,id FROM topics WHERE name=? COLLATE NOCASE",
        )
        .bind(local_id)
        .bind(topic)
        .execute(&mut **tx)
        .await?;
    }

    let received_languages = sentence
        .languages
        .iter()
        .map(|language| language.target_language.to_lowercase())
        .collect::<std::collections::HashSet<_>>();
    let existing_languages = sqlx::query_scalar::<_, String>(
        "SELECT target_language FROM sentence_languages WHERE sentence_id=?",
    )
    .bind(local_id)
    .fetch_all(&mut **tx)
    .await?;
    for existing in existing_languages {
        if !received_languages.contains(&existing.to_lowercase()) {
            sqlx::query("DELETE FROM sentence_languages WHERE sentence_id=? AND target_language=?")
                .bind(local_id)
                .bind(existing)
                .execute(&mut **tx)
                .await?;
        }
    }

    for language in sentence.languages {
        sqlx::query(
            "INSERT INTO sentence_languages(
               sentence_id,target_language,status,error,translation_comment
             ) VALUES(?,?,?,?,?)
             ON CONFLICT(sentence_id,target_language) DO UPDATE SET
               status=excluded.status,error=excluded.error,
               translation_comment=excluded.translation_comment",
        )
        .bind(local_id)
        .bind(&language.target_language)
        .bind(&language.status)
        .bind(&language.error)
        .bind(&language.translation_comment)
        .execute(&mut **tx)
        .await?;
        if let Some(audio) = &language.audio {
            sqlx::query(
                "INSERT INTO sync_audio_state(
                   sentence_id,target_language,remote_sha256,remote_mime,remote_name
                 ) VALUES(?,?,?,?,?)
                 ON CONFLICT(sentence_id,target_language) DO UPDATE SET
                   remote_sha256=excluded.remote_sha256,remote_mime=excluded.remote_mime,
                   remote_name=excluded.remote_name,updated_at=CURRENT_TIMESTAMP",
            )
            .bind(local_id)
            .bind(&language.target_language)
            .bind(&audio.sha256)
            .bind(&audio.mime)
            .bind(&audio.name)
            .execute(&mut **tx)
            .await?;
        } else if let Some((local_hash, remote_hash)) =
            sqlx::query_as::<_, (Option<String>, Option<String>)>(
                "SELECT local_sha256,remote_sha256 FROM sync_audio_state
                 WHERE sentence_id=? AND target_language=?",
            )
            .bind(local_id)
            .bind(&language.target_language)
            .fetch_optional(&mut **tx)
            .await?
        {
            if local_hash == remote_hash {
                sqlx::query(
                    "UPDATE sentence_languages SET audio_file=NULL,audio_name=NULL,audio_mime=NULL
                     WHERE sentence_id=? AND target_language=?",
                )
                .bind(local_id)
                .bind(&language.target_language)
                .execute(&mut **tx)
                .await?;
                sqlx::query(
                    "UPDATE sync_audio_state SET local_sha256=NULL,remote_sha256=NULL,
                     remote_mime=NULL,remote_name=NULL,updated_at=CURRENT_TIMESTAMP
                     WHERE sentence_id=? AND target_language=?",
                )
                .bind(local_id)
                .bind(&language.target_language)
                .execute(&mut **tx)
                .await?;
            } else {
                sqlx::query(
                    "UPDATE sync_audio_state SET remote_sha256=NULL,remote_mime=NULL,
                     remote_name=NULL,updated_at=CURRENT_TIMESTAMP
                     WHERE sentence_id=? AND target_language=?",
                )
                .bind(local_id)
                .bind(&language.target_language)
                .execute(&mut **tx)
                .await?;
            }
        }
        let Some(preparation) = language.active_preparation else {
            continue;
        };
        let mapped_local = sqlx::query_scalar::<_, String>(
            "SELECT local_key FROM sync_entity_ids
             WHERE entity_type='preparation' AND remote_id=?",
        )
        .bind(preparation.id.to_string())
        .fetch_optional(&mut **tx)
        .await?;
        let existing_local = if let Some(local_key) = mapped_local {
            Some(
                local_key
                    .parse::<i64>()
                    .map_err(|_| AppError::Sync("Preparation sync mapping is corrupted".into()))?,
            )
        } else {
            sqlx::query_scalar::<_, i64>(
                "SELECT id FROM preparations WHERE sentence_id=? AND version=?
                 AND target_language=?",
            )
            .bind(local_id)
            .bind(preparation.version)
            .bind(&language.target_language)
            .fetch_optional(&mut **tx)
            .await?
        };
        let preparation_local_id = if let Some(existing) = existing_local {
            sqlx::query(
                "UPDATE preparations SET model=?,translation=?,target_language=? WHERE id=?",
            )
            .bind(&preparation.model)
            .bind(&preparation.translation)
            .bind(&language.target_language)
            .bind(existing)
            .execute(&mut **tx)
            .await?;
            existing
        } else {
            let version = if sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM preparations WHERE sentence_id=? AND version=?)",
            )
            .bind(local_id)
            .bind(preparation.version)
            .fetch_one(&mut **tx)
            .await?
            {
                sqlx::query_scalar::<_, i64>(
                    "SELECT COALESCE(MAX(version),0)+1 FROM preparations WHERE sentence_id=?",
                )
                .bind(local_id)
                .fetch_one(&mut **tx)
                .await? as i32
            } else {
                preparation.version
            };
            let inserted = sqlx::query(
                "INSERT INTO preparations(
                   sentence_id,version,target_language,model,translation
                 ) VALUES(?,?,?,?,?)",
            )
            .bind(local_id)
            .bind(version)
            .bind(&language.target_language)
            .bind(&preparation.model)
            .bind(&preparation.translation)
            .execute(&mut **tx)
            .await?
            .last_insert_rowid();
            for block in &preparation.blocks {
                let block_id = sqlx::query(
                    "INSERT INTO blocks(preparation_id,position,correct,hint)
                     VALUES(?,?,?,?)",
                )
                .bind(inserted)
                .bind(block.position)
                .bind(&block.correct)
                .bind(&block.hint)
                .execute(&mut **tx)
                .await?
                .last_insert_rowid();
                for option in &block.options {
                    sqlx::query("INSERT INTO options(block_id,text,is_correct) VALUES(?,?,?)")
                        .bind(block_id)
                        .bind(&option.text)
                        .bind(option.is_correct)
                        .execute(&mut **tx)
                        .await?;
                }
            }
            inserted
        };
        sqlx::query(
            "INSERT INTO sync_entity_ids(entity_type,local_key,remote_id,server_revision)
             VALUES('preparation',?,?,?)
             ON CONFLICT(entity_type,local_key) DO UPDATE SET
               remote_id=excluded.remote_id,server_revision=excluded.server_revision,
               updated_at=CURRENT_TIMESTAMP",
        )
        .bind(preparation_local_id.to_string())
        .bind(preparation.id.to_string())
        .bind(language.revision)
        .execute(&mut **tx)
        .await?;
        sqlx::query(
            "UPDATE sentence_languages SET active_preparation_id=?
             WHERE sentence_id=? AND target_language=?",
        )
        .bind(preparation_local_id)
        .bind(local_id)
        .bind(&language.target_language)
        .execute(&mut **tx)
        .await?;
    }
    sqlx::query(
        "UPDATE sync_entity_ids SET server_revision=?,updated_at=CURRENT_TIMESTAMP
         WHERE entity_type='sentence' AND remote_id=?",
    )
    .bind(revision)
    .bind(remote_id.to_string())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn apply_delete(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    entity_type: &str,
    remote_id: Uuid,
    revision: i64,
) -> Result<()> {
    if entity_type == "sentence" {
        if let Some(local_key) = sqlx::query_scalar::<_, String>(
            "SELECT local_key FROM sync_entity_ids
             WHERE entity_type='sentence' AND remote_id=?",
        )
        .bind(remote_id.to_string())
        .fetch_optional(&mut **tx)
        .await?
        {
            let local_id = local_key
                .parse::<i64>()
                .map_err(|_| AppError::Sync("Sentence sync mapping is corrupted".into()))?;
            sqlx::query("DELETE FROM sentences WHERE id=?")
                .bind(local_id)
                .execute(&mut **tx)
                .await?;
            sqlx::query(
                "UPDATE sync_entity_ids SET server_revision=?,updated_at=CURRENT_TIMESTAMP
                 WHERE entity_type='sentence' AND remote_id=?",
            )
            .bind(revision)
            .bind(remote_id.to_string())
            .execute(&mut **tx)
            .await?;
        }
    }
    Ok(())
}

async fn pull_changes(
    state: &AppState,
    client: &reqwest::Client,
    base_url: &str,
    token: &str,
) -> Result<()> {
    loop {
        let cursor: i64 = sqlx::query_scalar("SELECT pull_cursor FROM sync_state WHERE id=1")
            .fetch_one(&state.db)
            .await?;
        let response = client
            .get(format!(
                "{base_url}/api/v1/sync/pull?cursor={cursor}&limit=200"
            ))
            .bearer_auth(token)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(api_error(response).await);
        }
        let page: PullResponse = response.json().await?;
        let mut tx = state.db.begin().await?;
        for change in page.changes {
            if change.operation == "delete" || change.payload.is_none() {
                apply_delete(
                    &mut tx,
                    &change.entity_type,
                    change.entity_id,
                    change.revision,
                )
                .await?;
            } else if change.entity_type == "settings" {
                apply_settings(&mut tx, change.payload.unwrap()).await?;
            } else if change.entity_type == "sentence" {
                apply_sentence(
                    &mut tx,
                    change.entity_id,
                    change.revision,
                    change.payload.unwrap(),
                )
                .await?;
            }
        }
        sqlx::query("UPDATE sync_state SET pull_cursor=? WHERE id=1")
            .bind(page.cursor)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        if !page.has_more {
            return Ok(());
        }
    }
}

fn audio_directory(app: &AppHandle) -> Result<std::path::PathBuf> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| AppError::Sync(error.to_string()))?
        .join("audio");
    std::fs::create_dir_all(&directory)?;
    Ok(directory)
}

fn file_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

async fn sync_audio_uploads(
    state: &AppState,
    app: &AppHandle,
    client: &reqwest::Client,
    base_url: &str,
    token: &str,
) -> Result<()> {
    let directory = audio_directory(app)?;
    let rows = sqlx::query(
        "SELECT sl.sentence_id,sl.target_language,sl.audio_file,sl.audio_name,sl.audio_mime,
         ids.remote_id,a.remote_sha256
         FROM sentence_languages sl
         JOIN sync_entity_ids ids
           ON ids.entity_type='sentence' AND ids.local_key=CAST(sl.sentence_id AS TEXT)
         LEFT JOIN sync_audio_state a
           ON a.sentence_id=sl.sentence_id AND a.target_language=sl.target_language
         WHERE sl.audio_file IS NOT NULL",
    )
    .fetch_all(&state.db)
    .await?;
    for row in rows {
        let sentence_id = row.get::<i64, _>(0);
        let target_language = row.get::<String, _>(1);
        let stored_name = row.get::<String, _>(2);
        if std::path::Path::new(&stored_name)
            .file_name()
            .and_then(|value| value.to_str())
            != Some(stored_name.as_str())
        {
            return Err(AppError::Sync("Invalid local audio path".into()));
        }
        let bytes = std::fs::read(directory.join(&stored_name))?;
        let hash = file_sha256(&bytes);
        if row.get::<Option<String>, _>(6).as_deref() == Some(hash.as_str()) {
            continue;
        }
        let remote_id = Uuid::parse_str(&row.get::<String, _>(5))
            .map_err(|_| AppError::Sync("Sentence sync mapping is corrupted".into()))?;
        let mime = row
            .get::<Option<String>, _>(4)
            .unwrap_or_else(|| "audio/mpeg".into());
        let mut request = client
            .put(format!("{base_url}/api/v1/sentences/{remote_id}/audio"))
            .query(&[("targetLanguage", target_language.as_str())])
            .bearer_auth(token)
            .header(reqwest::header::CONTENT_TYPE, &mime)
            .header("x-content-sha256", &hash)
            .body(bytes);
        if let Some(name) = row.get::<Option<String>, _>(3) {
            if name.is_ascii() {
                request = request.header("x-file-name", name);
            }
        }
        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(api_error(response).await);
        }
        let metadata: langai_contracts::AudioMetadataResponse = response.json().await?;
        if metadata.sha256 != hash {
            return Err(AppError::Sync(
                "Server confirmed a different audio checksum".into(),
            ));
        }
        sqlx::query(
            "INSERT INTO sync_audio_state(
               sentence_id,target_language,local_sha256,remote_sha256
             ) VALUES(?,?,?,?)
             ON CONFLICT(sentence_id,target_language) DO UPDATE SET
               local_sha256=excluded.local_sha256,remote_sha256=excluded.remote_sha256,
               updated_at=CURRENT_TIMESTAMP",
        )
        .bind(sentence_id)
        .bind(target_language)
        .bind(&hash)
        .bind(&hash)
        .execute(&state.db)
        .await?;
    }

    let deleted = sqlx::query(
        "SELECT a.sentence_id,a.target_language,ids.remote_id
         FROM sync_audio_state a
         JOIN sync_entity_ids ids
           ON ids.entity_type='sentence' AND ids.local_key=CAST(a.sentence_id AS TEXT)
         LEFT JOIN sentence_languages sl
           ON sl.sentence_id=a.sentence_id AND sl.target_language=a.target_language
         WHERE a.remote_sha256 IS NOT NULL
         AND (sl.sentence_id IS NULL OR sl.audio_file IS NULL)",
    )
    .fetch_all(&state.db)
    .await?;
    for row in deleted {
        let sentence_id = row.get::<i64, _>(0);
        let target_language = row.get::<String, _>(1);
        let remote_id = Uuid::parse_str(&row.get::<String, _>(2))
            .map_err(|_| AppError::Sync("Sentence sync mapping is corrupted".into()))?;
        let response = client
            .delete(format!("{base_url}/api/v1/sentences/{remote_id}/audio"))
            .query(&[("targetLanguage", target_language.as_str())])
            .bearer_auth(token)
            .send()
            .await?;
        if !response.status().is_success() && response.status() != reqwest::StatusCode::NOT_FOUND {
            return Err(api_error(response).await);
        }
        sqlx::query(
            "DELETE FROM sync_audio_state WHERE sentence_id=? AND target_language=?",
        )
        .bind(sentence_id)
        .bind(target_language)
        .execute(&state.db)
        .await?;
    }
    Ok(())
}

fn extension_for_mime(mime: &str) -> Result<&'static str> {
    match mime {
        "audio/mpeg" | "audio/mp3" => Ok("mp3"),
        "audio/wav" | "audio/x-wav" => Ok("wav"),
        "audio/ogg" => Ok("ogg"),
        "audio/mp4" | "audio/x-m4a" => Ok("m4a"),
        "audio/webm" => Ok("webm"),
        _ => Err(AppError::Sync("Server returned an unsupported audio type".into())),
    }
}

async fn sync_audio_downloads(
    state: &AppState,
    app: &AppHandle,
    client: &reqwest::Client,
    base_url: &str,
    token: &str,
) -> Result<()> {
    let directory = audio_directory(app)?;
    let rows = sqlx::query(
        "SELECT a.sentence_id,a.target_language,a.remote_sha256,a.remote_mime,a.remote_name,
         ids.remote_id,sl.audio_file
         FROM sync_audio_state a
         JOIN sync_entity_ids ids
           ON ids.entity_type='sentence' AND ids.local_key=CAST(a.sentence_id AS TEXT)
         JOIN sentence_languages sl
           ON sl.sentence_id=a.sentence_id AND sl.target_language=a.target_language
         WHERE a.remote_sha256 IS NOT NULL
         AND (a.local_sha256 IS NULL OR a.local_sha256<>a.remote_sha256)",
    )
    .fetch_all(&state.db)
    .await?;
    for row in rows {
        let sentence_id = row.get::<i64, _>(0);
        let target_language = row.get::<String, _>(1);
        let expected_hash = row.get::<String, _>(2);
        let expected_mime = row.get::<String, _>(3);
        let remote_id = Uuid::parse_str(&row.get::<String, _>(5))
            .map_err(|_| AppError::Sync("Sentence sync mapping is corrupted".into()))?;
        let response = client
            .get(format!("{base_url}/api/v1/sentences/{remote_id}/audio"))
            .query(&[("targetLanguage", target_language.as_str())])
            .bearer_auth(token)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(api_error(response).await);
        }
        let mime = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .unwrap_or(&expected_mime)
            .to_owned();
        let bytes = response.bytes().await?;
        if bytes.is_empty() || bytes.len() > 25 * 1024 * 1024 {
            return Err(AppError::Sync("Downloaded audio has an invalid size".into()));
        }
        let actual_hash = file_sha256(&bytes);
        if actual_hash != expected_hash {
            return Err(AppError::Sync(
                "Downloaded audio checksum does not match".into(),
            ));
        }
        let stored_name = format!("{sentence_id}-{expected_hash}.{}", extension_for_mime(&mime)?);
        let temporary = directory.join(format!(".{stored_name}.tmp"));
        std::fs::write(&temporary, &bytes)?;
        let destination = directory.join(&stored_name);
        if destination.exists() {
            std::fs::remove_file(&temporary)?;
        } else {
            std::fs::rename(&temporary, &destination)?;
        }
        let previous = row.get::<Option<String>, _>(6);
        let name = row
            .get::<Option<String>, _>(4)
            .unwrap_or_else(|| stored_name.clone());
        let mut tx = state.db.begin().await?;
        sqlx::query(
            "UPDATE sentence_languages SET audio_file=?,audio_name=?,audio_mime=?
             WHERE sentence_id=? AND target_language=?",
        )
        .bind(&stored_name)
        .bind(name)
        .bind(&mime)
        .bind(sentence_id)
        .bind(&target_language)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE sync_audio_state SET local_sha256=?,updated_at=CURRENT_TIMESTAMP
             WHERE sentence_id=? AND target_language=?",
        )
        .bind(&actual_hash)
        .bind(sentence_id)
        .bind(&target_language)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        if let Some(previous) = previous {
            if previous != stored_name
                && std::path::Path::new(&previous)
                    .file_name()
                    .and_then(|value| value.to_str())
                    == Some(previous.as_str())
            {
                let _ = std::fs::remove_file(directory.join(previous));
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn sync_now(app: AppHandle, state: State<'_, AppState>) -> Result<SyncStatus> {
    let token = secrets::get_sync_token()?
        .ok_or_else(|| AppError::Sync("Connect an account first".into()))?;
    let base_url =
        sqlx::query_scalar::<_, Option<String>>("SELECT server_url FROM sync_state WHERE id=1")
            .fetch_one(&state.db)
            .await?
            .ok_or_else(|| AppError::Sync("Connect an account first".into()))?;
    sqlx::query("UPDATE sync_state SET status='syncing',last_error=NULL WHERE id=1")
        .execute(&state.db)
        .await?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;
    if let Err(error) =
        upload_provider_key(&client, &base_url, &token, "openai", secrets::get()?).await
    {
        mark_sync_failure(&state, "error", &error.to_string()).await;
        return Err(error);
    }
    if let Err(error) = upload_provider_key(
        &client,
        &base_url,
        &token,
        "elevenlabs",
        secrets::get_elevenlabs()?,
    )
    .await
    {
        mark_sync_failure(&state, "error", &error.to_string()).await;
        return Err(error);
    }

    loop {
        let rows = sqlx::query(
            "SELECT operation_id,kind,entity_id,base_revision,payload
             FROM sync_outbox
             WHERE state IN ('pending','failed')
             AND (next_attempt_at IS NULL OR next_attempt_at<=CURRENT_TIMESTAMP)
             ORDER BY created_at LIMIT 50",
        )
        .fetch_all(&state.db)
        .await?;
        if rows.is_empty() {
            break;
        }
        let mut operations = Vec::with_capacity(rows.len());
        for row in rows {
            let operation_id = Uuid::parse_str(&row.get::<String, _>(0))
                .map_err(|_| AppError::Sync("Outbox operation ID is corrupted".into()))?;
            let entity_id = Uuid::parse_str(&row.get::<String, _>(2))
                .map_err(|_| AppError::Sync("Outbox entity ID is corrupted".into()))?;
            let payload = row
                .get::<Option<String>, _>(4)
                .map(|value| serde_json::from_str(&value))
                .transpose()
                .map_err(|_| AppError::Sync("Outbox payload is corrupted".into()))?;
            operations.push(PushOperation {
                operation_id,
                kind: row.get(1),
                entity_id,
                base_revision: row.get(3),
                payload,
            });
        }
        let expected_operation_ids = operations
            .iter()
            .map(|operation| operation.operation_id)
            .collect::<std::collections::HashSet<_>>();
        let response = match client
            .post(format!("{base_url}/api/v1/sync/push"))
            .bearer_auth(&token)
            .json(&PushRequest { operations })
            .send()
            .await
        {
            Ok(response) => response,
            Err(error) => {
                mark_sync_failure(&state, "offline", &error.to_string()).await;
                return Err(AppError::Http(error));
            }
        };
        if !response.status().is_success() {
            let error = api_error(response).await;
            mark_sync_failure(&state, "error", &error.to_string()).await;
            return Err(error);
        }
        let response: PushResponse = response.json().await?;
        let confirmed_operation_ids = response
            .results
            .iter()
            .map(|result| result.operation_id)
            .collect::<std::collections::HashSet<_>>();
        if confirmed_operation_ids != expected_operation_ids {
            let error =
                AppError::Sync("Server returned an incomplete synchronization confirmation".into());
            mark_sync_failure(&state, "error", &error.to_string()).await;
            return Err(error);
        }
        let mut tx = state.db.begin().await?;
        for result in response.results {
            sqlx::query("DELETE FROM sync_outbox WHERE operation_id=?")
                .bind(result.operation_id.to_string())
                .execute(&mut *tx)
                .await?;
            sqlx::query(
                "UPDATE sync_entity_ids SET server_revision=?,updated_at=CURRENT_TIMESTAMP
                 WHERE remote_id=?",
            )
            .bind(result.revision)
            .bind(result.entity_id.to_string())
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
    }
    if let Err(error) = sync_audio_uploads(&state, &app, &client, &base_url, &token).await {
        let status = if matches!(error, AppError::Http(_) | AppError::Io(_)) {
            "offline"
        } else {
            "error"
        };
        mark_sync_failure(&state, status, &error.to_string()).await;
        return Err(error);
    }
    if let Err(error) = pull_changes(&state, &client, &base_url, &token).await {
        let status = if matches!(error, AppError::Http(_)) {
            "offline"
        } else {
            "error"
        };
        mark_sync_failure(&state, status, &error.to_string()).await;
        return Err(error);
    }
    if let Err(error) = sync_audio_downloads(&state, &app, &client, &base_url, &token).await {
        let status = if matches!(error, AppError::Http(_)) {
            "offline"
        } else {
            "error"
        };
        mark_sync_failure(&state, status, &error.to_string()).await;
        return Err(error);
    }
    sqlx::query(
        "UPDATE sync_state SET status='synced',last_error=NULL,last_sync_at=CURRENT_TIMESTAMP,
         initial_upload_completed=1 WHERE id=1",
    )
    .execute(&state.db)
    .await?;
    sync_status(state).await
}
