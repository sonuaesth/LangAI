use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::Row;
use tauri::State;
use uuid::Uuid;

use crate::{
    error::{AppError, Result},
    secrets, AppState,
};

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
    let Some(key) = key else {
        return Ok(());
    };
    let response = client
        .put(format!("{base_url}/api/v1/provider-keys/{provider}"))
        .bearer_auth(token)
        .json(&json!({ "apiKey": key }))
        .send()
        .await?;
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
    let _ = sqlx::query(
        "UPDATE sync_state SET status=?,last_error=? WHERE id=1",
    )
    .bind(status)
    .bind(error)
    .execute(&state.db)
    .await;
}

#[tauri::command]
pub async fn sync_now(state: State<'_, AppState>) -> Result<SyncStatus> {
    let token = secrets::get_sync_token()?
        .ok_or_else(|| AppError::Sync("Connect an account first".into()))?;
    let base_url = sqlx::query_scalar::<_, Option<String>>(
        "SELECT server_url FROM sync_state WHERE id=1",
    )
    .fetch_one(&state.db)
    .await?
    .ok_or_else(|| AppError::Sync("Connect an account first".into()))?;
    sqlx::query("UPDATE sync_state SET status='syncing',last_error=NULL WHERE id=1")
        .execute(&state.db)
        .await?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;

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
    sqlx::query(
        "UPDATE sync_state SET status='synced',last_error=NULL,last_sync_at=CURRENT_TIMESTAMP,
         initial_upload_completed=1 WHERE id=1",
    )
    .execute(&state.db)
    .await?;
    sync_status(state).await
}
