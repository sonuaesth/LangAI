use crate::{
    error::{AppError, Result},
    openai, secrets, AppState,
};
use futures::{stream, StreamExt};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sentence {
    pub id: i64,
    pub source_text: String,
    pub status: String,
    pub error: Option<String>,
    pub created_at: String,
    pub languages: Vec<SentenceLanguage>,
    pub topics: Vec<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SentenceLanguage {
    pub target_language: String,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SentenceDetails {
    pub id: i64,
    pub source_text: String,
    pub topics: Vec<String>,
    pub translations: Vec<ManualTranslation>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualTranslation {
    pub target_language: String,
    pub translation: String,
    pub blocks: Vec<ManualBlock>,
    pub audio_name: Option<String>,
    pub audio_mime: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualBlock {
    pub correct: String,
    pub distractors: Vec<String>,
    pub hint: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioData {
    mime_type: String,
    bytes: Vec<u8>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub api_key_configured: bool,
    pub model: String,
    pub target_language: String,
}
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Progress {
    sentence_id: i64,
    status: String,
    completed: usize,
    total: usize,
    error: Option<String>,
}
#[tauri::command]
pub async fn list_sentences(
    filter_language: Option<String>,
    target_language: Option<String>,
    filter_topic: Option<String>,
    s: State<'_, AppState>,
) -> Result<Vec<Sentence>> {
    let rows = sqlx::query("SELECT id,source_text,created_at FROM sentences WHERE (? IS NULL OR EXISTS(SELECT 1 FROM sentence_languages sl WHERE sl.sentence_id=sentences.id AND sl.target_language=?)) AND (? IS NULL OR EXISTS(SELECT 1 FROM sentence_topics st JOIN topics t ON t.id=st.topic_id WHERE st.sentence_id=sentences.id AND t.name=? COLLATE NOCASE)) ORDER BY id DESC")
        .bind(&filter_language).bind(&filter_language).bind(&filter_topic).bind(&filter_topic).fetch_all(&s.db).await?;
    let mut result = Vec::with_capacity(rows.len());
    for row in rows {
        let id: i64 = row.get(0);
        let language_rows = sqlx::query("SELECT target_language,status,error FROM sentence_languages WHERE sentence_id=? ORDER BY target_language").bind(id).fetch_all(&s.db).await?;
        let languages: Vec<SentenceLanguage> = language_rows
            .into_iter()
            .map(|item| SentenceLanguage {
                target_language: item.get(0),
                status: item.get(1),
                error: item.get(2),
            })
            .collect();
        let topics = sqlx::query("SELECT t.name FROM topics t JOIN sentence_topics st ON st.topic_id=t.id WHERE st.sentence_id=? ORDER BY t.name")
            .bind(id).fetch_all(&s.db).await?.into_iter().map(|item| item.get(0)).collect();
        let selected = target_language.as_ref().and_then(|language| {
            languages
                .iter()
                .find(|item| &item.target_language == language)
        });
        result.push(Sentence {
            id,
            source_text: row.get(1),
            created_at: row.get(2),
            status: selected
                .map(|item| item.status.clone())
                .unwrap_or_else(|| "unprepared".into()),
            error: selected.and_then(|item| item.error.clone()),
            languages,
            topics,
        });
    }
    Ok(result)
}

#[tauri::command]
pub async fn sentence_details(id: i64, s: State<'_, AppState>) -> Result<SentenceDetails> {
    let sentence = sqlx::query("SELECT source_text FROM sentences WHERE id=?")
        .bind(id)
        .fetch_optional(&s.db)
        .await?
        .ok_or_else(|| AppError::Input("Sentence was not found".into()))?;
    let topics = sqlx::query("SELECT t.name FROM topics t JOIN sentence_topics st ON st.topic_id=t.id WHERE st.sentence_id=? ORDER BY t.name")
        .bind(id).fetch_all(&s.db).await?.into_iter().map(|row| row.get(0)).collect();
    let prepared = sqlx::query("SELECT sl.target_language,p.translation,p.id,sl.audio_name,sl.audio_mime FROM sentence_languages sl JOIN preparations p ON p.id=sl.active_preparation_id WHERE sl.sentence_id=? ORDER BY sl.target_language")
        .bind(id).fetch_all(&s.db).await?;
    let mut translations = Vec::with_capacity(prepared.len());
    for row in prepared {
        let preparation_id: i64 = row.get(2);
        let block_rows = sqlx::query(
            "SELECT id,correct,hint FROM blocks WHERE preparation_id=? ORDER BY position",
        )
        .bind(preparation_id)
        .fetch_all(&s.db)
        .await?;
        let mut blocks = Vec::with_capacity(block_rows.len());
        for block in block_rows {
            let block_id: i64 = block.get(0);
            let distractors = sqlx::query(
                "SELECT text FROM options WHERE block_id=? AND is_correct=0 ORDER BY id LIMIT 3",
            )
            .bind(block_id)
            .fetch_all(&s.db)
            .await?
            .into_iter()
            .map(|option| option.get(0))
            .collect();
            blocks.push(ManualBlock {
                correct: block.get(1),
                distractors,
                hint: block.get(2),
            });
        }
        translations.push(ManualTranslation {
            target_language: row.get(0),
            translation: row.get(1),
            blocks,
            audio_name: row.get(3),
            audio_mime: row.get(4),
        });
    }
    Ok(SentenceDetails {
        id,
        source_text: sentence.get(0),
        topics,
        translations,
    })
}

fn audio_error(error: std::io::Error) -> AppError {
    AppError::Input(format!("Audio storage: {error}"))
}

fn audio_directory(app: &AppHandle) -> Result<std::path::PathBuf> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| AppError::Input(error.to_string()))?
        .join("audio");
    std::fs::create_dir_all(&directory).map_err(audio_error)?;
    Ok(directory)
}

#[tauri::command]
pub async fn save_sentence_audio(
    sentence_id: i64,
    target_language: String,
    file_name: String,
    mime_type: String,
    bytes: Vec<u8>,
    app: AppHandle,
    s: State<'_, AppState>,
) -> Result<()> {
    if bytes.is_empty() || bytes.len() > 25 * 1024 * 1024 {
        return Err(AppError::Input(
            "Audio file must be between 1 byte and 25 MB".into(),
        ));
    }
    let mime = mime_type.to_ascii_lowercase();
    let extension = match mime.as_str() {
        "audio/mpeg" | "audio/mp3" => "mp3",
        "audio/wav" | "audio/x-wav" => "wav",
        "audio/ogg" => "ogg",
        "audio/mp4" | "audio/x-m4a" => "m4a",
        "audio/webm" => "webm",
        _ => {
            return Err(AppError::Input(
                "Supported audio formats: MP3, WAV, OGG, M4A and WebM".into(),
            ))
        }
    };
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| AppError::Input(error.to_string()))?
        .as_millis();
    let stored_name = format!("{sentence_id}-{stamp}.{extension}");
    let directory = audio_directory(&app)?;
    std::fs::write(directory.join(&stored_name), bytes).map_err(audio_error)?;
    let previous: Option<(Option<String>,)> = sqlx::query_as(
        "SELECT audio_file FROM sentence_languages WHERE sentence_id=? AND target_language=?",
    )
    .bind(sentence_id)
    .bind(&target_language)
    .fetch_optional(&s.db)
    .await?;
    sqlx::query("INSERT INTO sentence_languages(sentence_id,target_language,status,audio_file,audio_name,audio_mime) VALUES(?,?,'unprepared',?,?,?) ON CONFLICT(sentence_id,target_language) DO UPDATE SET audio_file=excluded.audio_file,audio_name=excluded.audio_name,audio_mime=excluded.audio_mime")
        .bind(sentence_id).bind(&target_language).bind(&stored_name).bind(file_name).bind(mime_type).execute(&s.db).await?;
    if let Some(Some(old_name)) = previous.map(|value| value.0) {
        if old_name != stored_name {
            let _ = std::fs::remove_file(directory.join(old_name));
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn delete_sentence_audio(
    sentence_id: i64,
    target_language: String,
    app: AppHandle,
    s: State<'_, AppState>,
) -> Result<()> {
    let previous: Option<(Option<String>,)> = sqlx::query_as(
        "SELECT audio_file FROM sentence_languages WHERE sentence_id=? AND target_language=?",
    )
    .bind(sentence_id)
    .bind(&target_language)
    .fetch_optional(&s.db)
    .await?;
    sqlx::query("UPDATE sentence_languages SET audio_file=NULL,audio_name=NULL,audio_mime=NULL WHERE sentence_id=? AND target_language=?")
        .bind(sentence_id).bind(target_language).execute(&s.db).await?;
    if let Some(Some(name)) = previous.map(|value| value.0) {
        let _ = std::fs::remove_file(audio_directory(&app)?.join(name));
    }
    Ok(())
}

#[tauri::command]
pub async fn sentence_audio(
    sentence_id: i64,
    target_language: String,
    app: AppHandle,
    s: State<'_, AppState>,
) -> Result<AudioData> {
    let row = sqlx::query("SELECT audio_file,audio_mime FROM sentence_languages WHERE sentence_id=? AND target_language=? AND audio_file IS NOT NULL")
        .bind(sentence_id).bind(target_language).fetch_optional(&s.db).await?
        .ok_or_else(|| AppError::Input("Audio is not attached".into()))?;
    let name: String = row.get(0);
    if std::path::Path::new(&name)
        .file_name()
        .and_then(|value| value.to_str())
        != Some(name.as_str())
    {
        return Err(AppError::Input("Invalid audio path".into()));
    }
    let bytes = std::fs::read(audio_directory(&app)?.join(name)).map_err(audio_error)?;
    Ok(AudioData {
        mime_type: row.get(1),
        bytes,
    })
}

#[tauri::command]
pub async fn save_manual_translation(
    sentence_id: i64,
    translation: ManualTranslation,
    s: State<'_, AppState>,
) -> Result<()> {
    let source: (String,) = sqlx::query_as("SELECT source_text FROM sentences WHERE id=?")
        .bind(sentence_id)
        .fetch_optional(&s.db)
        .await?
        .ok_or_else(|| AppError::Input("Sentence was not found".into()))?;
    if translation.target_language.trim().is_empty() {
        return Err(AppError::Input("Target language is required".into()));
    }
    let generated = openai::types::Generated {
        source_text: source.0,
        target_language: translation.target_language.clone(),
        translation: translation.translation.trim().to_owned(),
        blocks: translation
            .blocks
            .into_iter()
            .enumerate()
            .map(|(position, block)| openai::types::GeneratedBlock {
                position,
                correct: block.correct.trim().to_owned(),
                distractors: block
                    .distractors
                    .into_iter()
                    .map(|value| value.trim().to_owned())
                    .collect(),
                hint: block
                    .hint
                    .map(|value| value.trim().to_owned())
                    .filter(|value| !value.is_empty()),
            })
            .collect(),
    };
    openai::validate::validate(&generated)?;
    sqlx::query("INSERT INTO sentence_languages(sentence_id,target_language,status) VALUES(?,?,'unprepared') ON CONFLICT(sentence_id,target_language) DO NOTHING")
        .bind(sentence_id).bind(&generated.target_language).execute(&s.db).await?;
    persist(
        &s,
        sentence_id,
        &generated,
        "manual",
        &generated.target_language,
    )
    .await
}
#[tauri::command]
pub async fn add_sentences(
    texts: Vec<String>,
    target_language: String,
    translation_comment: Option<String>,
    topic: Option<String>,
    s: State<'_, AppState>,
) -> Result<Vec<Sentence>> {
    if target_language.trim().is_empty() {
        return Err(AppError::Input("Target language is required".into()));
    }
    if translation_comment
        .as_ref()
        .is_some_and(|value| value.chars().count() > 1000)
    {
        return Err(AppError::Input("Translation comment is too long".into()));
    }
    validate_topic(&topic)?;
    for text in texts
        .into_iter()
        .map(|x| x.trim().to_owned())
        .filter(|x| !x.is_empty())
    {
        if text.len() > 2000 {
            return Err(AppError::Input("Sentence is too long".into()));
        }
        let existing = sqlx::query("SELECT id FROM sentences WHERE source_text=? COLLATE NOCASE")
            .bind(&text)
            .fetch_optional(&s.db)
            .await?;
        let id: i64 = if let Some(row) = existing {
            row.get(0)
        } else {
            sqlx::query("INSERT INTO sentences(source_text,target_language)VALUES(?,?)")
                .bind(&text)
                .bind(&target_language)
                .execute(&s.db)
                .await?
                .last_insert_rowid()
        };
        sqlx::query("INSERT INTO sentence_languages(sentence_id,target_language,translation_comment)VALUES(?,?,?) ON CONFLICT(sentence_id,target_language) DO UPDATE SET translation_comment=COALESCE(excluded.translation_comment,sentence_languages.translation_comment)")
        .bind(id)
        .bind(&target_language)
        .bind(&translation_comment)
        .execute(&s.db)
        .await?;
        assign_topic(&s.db, id, topic.as_deref()).await?;
    }
    list_sentences(None, Some(target_language), None, s).await
}

fn validate_topic(topic: &Option<String>) -> Result<()> {
    if topic.as_ref().is_some_and(|value| {
        let length = value.trim().chars().count();
        length == 0 || length > 100
    }) {
        return Err(AppError::Input(
            "Topic must contain 1 to 100 characters".into(),
        ));
    }
    Ok(())
}

async fn assign_topic(db: &sqlx::SqlitePool, sentence_id: i64, topic: Option<&str>) -> Result<()> {
    let Some(topic) = topic.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    sqlx::query("INSERT INTO topics(name) VALUES(?) ON CONFLICT(name) DO NOTHING")
        .bind(topic)
        .execute(db)
        .await?;
    sqlx::query("INSERT OR IGNORE INTO sentence_topics(sentence_id,topic_id) SELECT ?,id FROM topics WHERE name=? COLLATE NOCASE").bind(sentence_id).bind(topic).execute(db).await?;
    Ok(())
}
#[tauri::command]
pub async fn delete_sentences(ids: Vec<i64>, s: State<'_, AppState>) -> Result<()> {
    for id in ids {
        sqlx::query("DELETE FROM sentences WHERE id=?")
            .bind(id)
            .execute(&s.db)
            .await?;
    }
    Ok(())
}
async fn settings_inner(s: &AppState) -> Result<Settings> {
    let r = sqlx::query("SELECT model,target_language FROM settings WHERE id=1")
        .fetch_one(&s.db)
        .await?;
    Ok(Settings {
        api_key_configured: secrets::get()?.is_some(),
        model: r.get(0),
        target_language: r.get(1),
    })
}
#[tauri::command]
pub async fn get_settings(s: State<'_, AppState>) -> Result<Settings> {
    settings_inner(&s).await
}
#[tauri::command]
pub async fn save_settings(model: String, s: State<'_, AppState>) -> Result<Settings> {
    if model.trim().is_empty() {
        return Err(AppError::Input("Model is required".into()));
    }
    sqlx::query("UPDATE settings SET model=? WHERE id=1")
        .bind(model)
        .execute(&s.db)
        .await?;
    settings_inner(&s).await
}
#[tauri::command]
pub async fn verify_api_key(api_key: String) -> Result<Vec<String>> {
    openai::models(&api_key).await
}

#[tauri::command]
pub async fn list_available_models() -> Result<Vec<String>> {
    let key =
        secrets::get()?.ok_or_else(|| AppError::Input("Configure an API key first".into()))?;
    openai::exercise_models(&key).await
}
#[tauri::command]
pub async fn save_api_key(api_key: String, s: State<'_, AppState>) -> Result<Settings> {
    secrets::set(&api_key)?;
    settings_inner(&s).await
}
#[tauri::command]
pub async fn delete_api_key(s: State<'_, AppState>) -> Result<Settings> {
    secrets::delete()?;
    settings_inner(&s).await
}
async fn persist(
    state: &AppState,
    id: i64,
    g: &openai::types::Generated,
    model: &str,
    lang: &str,
) -> Result<()> {
    let mut tx = state.db.begin().await?;
    let version: (i64,) =
        sqlx::query_as("SELECT COALESCE(MAX(version),0)+1 FROM preparations WHERE sentence_id=?")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
    let p=sqlx::query("INSERT INTO preparations(sentence_id,version,target_language,model,translation)VALUES(?,?,?,?,?)").bind(id).bind(version.0).bind(lang).bind(model).bind(&g.translation).execute(&mut *tx).await?.last_insert_rowid();
    for b in &g.blocks {
        let bid =
            sqlx::query("INSERT INTO blocks(preparation_id,position,correct,hint)VALUES(?,?,?,?)")
                .bind(p)
                .bind(b.position as i64)
                .bind(&b.correct)
                .bind(&b.hint)
                .execute(&mut *tx)
                .await?
                .last_insert_rowid();
        sqlx::query("INSERT INTO options(block_id,text,is_correct)VALUES(?,?,1)")
            .bind(bid)
            .bind(&b.correct)
            .execute(&mut *tx)
            .await?;
        for d in &b.distractors {
            sqlx::query("INSERT INTO options(block_id,text,is_correct)VALUES(?,?,0)")
                .bind(bid)
                .bind(d)
                .execute(&mut *tx)
                .await?;
        }
    }
    sqlx::query("UPDATE sentence_languages SET active_preparation_id=?,status='ready',error=NULL WHERE sentence_id=? AND target_language=?")
    .bind(p)
    .bind(id)
    .bind(lang)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}
#[tauri::command]
pub async fn prepare_sentences(
    ids: Option<Vec<i64>>,
    target_language: Option<String>,
    translation_comment: Option<String>,
    topic: Option<String>,
    app: AppHandle,
    s: State<'_, AppState>,
) -> Result<()> {
    let key =
        secrets::get()?.ok_or_else(|| AppError::Input("Configure an API key first".into()))?;
    let requested_language =
        target_language.ok_or_else(|| AppError::Input("Target language is required".into()))?;
    if translation_comment
        .as_ref()
        .is_some_and(|value| value.chars().count() > 1000)
    {
        return Err(AppError::Input("Translation comment is too long".into()));
    }
    validate_topic(&topic)?;
    let cfg = settings_inner(&s).await?;
    let has_selected_ids = ids.is_some();
    let rows: Vec<(i64, String, String, Option<String>)> = if let Some(ids) = ids {
        let mut out = vec![];
        for id in ids {
            if let Some(r) = sqlx::query("SELECT id,source_text FROM sentences WHERE id=?")
                .bind(id)
                .fetch_optional(&s.db)
                .await?
            {
                assign_topic(&s.db, id, topic.as_deref()).await?;
                sqlx::query("INSERT INTO sentence_languages(sentence_id,target_language,translation_comment)VALUES(?,?,?) ON CONFLICT(sentence_id,target_language) DO UPDATE SET translation_comment=COALESCE(excluded.translation_comment,sentence_languages.translation_comment)").bind(id).bind(&requested_language).bind(&translation_comment).execute(&s.db).await?;
                let stored_comment: (Option<String>,) = sqlx::query_as("SELECT translation_comment FROM sentence_languages WHERE sentence_id=? AND target_language=?").bind(id).bind(&requested_language).fetch_one(&s.db).await?;
                out.push((
                    r.get::<i64, _>(0),
                    r.get::<String, _>(1),
                    requested_language.clone(),
                    stored_comment.0,
                ))
            }
        }
        out
    } else {
        sqlx::query_as("SELECT s.id,s.source_text,sl.target_language,COALESCE(?,sl.translation_comment) FROM sentences s JOIN sentence_languages sl ON sl.sentence_id=s.id WHERE sl.status IN('unprepared','failed') AND sl.target_language=?")
        .bind(&translation_comment)
        .bind(&requested_language)
        .fetch_all(&s.db)
        .await?
    };
    if !has_selected_ids {
        for (id, _, _, _) in &rows {
            assign_topic(&s.db, *id, topic.as_deref()).await?;
        }
    }
    let total = rows.len();
    for (id, _, _, _) in &rows {
        sqlx::query("UPDATE sentence_languages SET status='queued',error=NULL,translation_comment=COALESCE(?,translation_comment) WHERE sentence_id=? AND target_language=?")
            .bind(&translation_comment)
            .bind(id)
            .bind(&requested_language)
            .execute(&s.db)
            .await?;
    }
    let state = Arc::new(s.inner().clone());
    stream::iter(rows.into_iter().enumerate())
        .for_each_concurrent(2, |(completed, (id, text, lang, comment))| {
            let state = state.clone();
            let app = app.clone();
            let key = key.clone();
            let model = cfg.model.clone();
            async move {
                sqlx::query("UPDATE sentence_languages SET status='generating' WHERE sentence_id=? AND target_language=?")
                    .bind(id)
                    .bind(&lang)
                    .execute(&state.db)
                    .await
                    .ok();
                app.emit(
                    "preparation-progress",
                    Progress {
                        sentence_id: id,
                        status: "generating".into(),
                        completed,
                        total,
                        error: None,
                    },
                )
                .ok();
                let result = match openai::generate(&key, &model, &lang, &text, comment.as_deref()).await {
                    Ok(g) => persist(&state, id, &g, &model, &lang).await,
                    Err(e) => Err(e),
                };
                match result {
                    Ok(_) => {
                        app.emit(
                            "preparation-progress",
                            Progress {
                                sentence_id: id,
                                status: "ready".into(),
                                completed: completed + 1,
                                total,
                                error: None,
                            },
                        )
                        .ok();
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        sqlx::query("UPDATE sentence_languages SET status='failed',error=? WHERE sentence_id=? AND target_language=?")
                            .bind(&msg)
                            .bind(id)
                            .bind(&lang)
                            .execute(&state.db)
                            .await
                            .ok();
                        app.emit(
                            "preparation-progress",
                            Progress {
                                sentence_id: id,
                                status: "failed".into(),
                                completed: completed + 1,
                                total,
                                error: Some(msg),
                            },
                        )
                        .ok();
                    }
                }
            }
        })
        .await;
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Exercise {
    sentence_id: i64,
    source_text: String,
    target_language: String,
    translation: String,
    audio_available: bool,
    blocks: Vec<Block>,
}
#[derive(Serialize)]
pub struct Block {
    id: i64,
    position: i64,
    correct: String,
    prefix: String,
    suffix: String,
    hint: Option<String>,
    options: Vec<OptionItem>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionItem {
    id: i64,
    text: String,
    is_correct: bool,
}
#[tauri::command]
pub async fn exercise_languages(s: State<'_, AppState>) -> Result<Vec<String>> {
    let rows = sqlx::query("SELECT DISTINCT target_language FROM sentence_languages WHERE status='ready' AND active_preparation_id IS NOT NULL ORDER BY target_language")
        .fetch_all(&s.db).await?;
    Ok(rows.into_iter().map(|row| row.get(0)).collect())
}

#[tauri::command]
pub async fn list_topics(s: State<'_, AppState>) -> Result<Vec<String>> {
    let rows = sqlx::query("SELECT name FROM topics ORDER BY name")
        .fetch_all(&s.db)
        .await?;
    Ok(rows.into_iter().map(|row| row.get(0)).collect())
}

#[tauri::command]
pub async fn exercise_topics(
    target_language: String,
    s: State<'_, AppState>,
) -> Result<Vec<String>> {
    let rows = sqlx::query("SELECT DISTINCT t.name FROM topics t JOIN sentence_topics st ON st.topic_id=t.id JOIN sentence_languages sl ON sl.sentence_id=st.sentence_id WHERE sl.target_language=? AND sl.status='ready' AND sl.active_preparation_id IS NOT NULL ORDER BY t.name")
        .bind(target_language).fetch_all(&s.db).await?;
    Ok(rows.into_iter().map(|row| row.get(0)).collect())
}
#[tauri::command]
pub async fn next_exercise(
    last_id: Option<i64>,
    target_language: Option<String>,
    topic: Option<String>,
    s: State<'_, AppState>,
) -> Result<Option<Exercise>> {
    let rows = sqlx::query(
        "SELECT sl.sentence_id FROM sentence_languages sl WHERE sl.status='ready' AND sl.active_preparation_id IS NOT NULL AND (? IS NULL OR sl.target_language=?) AND (? IS NULL OR EXISTS(SELECT 1 FROM sentence_topics st JOIN topics t ON t.id=st.topic_id WHERE st.sentence_id=sl.sentence_id AND t.name=? COLLATE NOCASE))",
    )
    .bind(&target_language)
    .bind(&target_language)
    .bind(&topic)
    .bind(&topic)
    .fetch_all(&s.db)
    .await?;
    let ids = crate::exercise::next_cycle(rows.iter().map(|r| r.get(0)).collect(), last_id);
    let Some(id) = ids.first() else {
        return Ok(None);
    };
    let r=sqlx::query("SELECT s.source_text,p.translation,p.id,sl.target_language,sl.audio_file IS NOT NULL FROM sentences s JOIN sentence_languages sl ON sl.sentence_id=s.id JOIN preparations p ON p.id=sl.active_preparation_id WHERE s.id=? AND (? IS NULL OR sl.target_language=?)").bind(id).bind(&target_language).bind(&target_language).fetch_one(&s.db).await?;
    let pid: i64 = r.get(2);
    let mut blocks = vec![];
    for b in sqlx::query(
        "SELECT id,position,correct,hint FROM blocks WHERE preparation_id=? ORDER BY position",
    )
    .bind(pid)
    .fetch_all(&s.db)
    .await?
    {
        let bid: i64 = b.get(0);
        let stored_correct: String = b.get(2);
        let (prefix, correct, suffix) =
            crate::openai::validate::split_edge_punctuation(&stored_correct);
        // Older preparations may contain four distractors. Limit the result so
        // every exercise consistently shows one correct answer and three alternatives.
        let options = sqlx::query(
            "SELECT id,text,is_correct FROM options WHERE block_id=? ORDER BY is_correct DESC,id LIMIT 4",
        )
            .bind(bid)
            .fetch_all(&s.db)
            .await?
            .into_iter()
            .map(|o| {
                let stored: String = o.get(1);
                let (_, text, _) = crate::openai::validate::split_edge_punctuation(&stored);
                OptionItem {
                    id: o.get(0),
                    text,
                    is_correct: o.get::<i64, _>(2) != 0,
                }
            })
            .collect();
        blocks.push(Block {
            id: bid,
            position: b.get(1),
            correct,
            prefix,
            suffix,
            hint: b.get(3),
            options,
        })
    }
    Ok(Some(Exercise {
        sentence_id: *id,
        source_text: r.get(0),
        target_language: r.get(3),
        translation: r.get(1),
        audio_available: r.get::<i64, _>(4) != 0,
        blocks,
    }))
}
