use crate::{
    error::{AppError, Result},
    openai, secrets, AppState,
};
use futures::{stream, StreamExt};
use serde::Serialize;
use sqlx::Row;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sentence {
    pub id: i64,
    pub source_text: String,
    pub status: String,
    pub error: Option<String>,
    pub created_at: String,
    pub languages: Vec<SentenceLanguage>,
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
    s: State<'_, AppState>,
) -> Result<Vec<Sentence>> {
    let rows = sqlx::query("SELECT id,source_text,created_at FROM sentences WHERE (? IS NULL OR EXISTS(SELECT 1 FROM sentence_languages sl WHERE sl.sentence_id=sentences.id AND sl.target_language=?)) ORDER BY id DESC")
        .bind(&filter_language).bind(&filter_language).fetch_all(&s.db).await?;
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
        });
    }
    Ok(result)
}
#[tauri::command]
pub async fn add_sentences(
    texts: Vec<String>,
    target_language: String,
    translation_comment: Option<String>,
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
    }
    list_sentences(None, Some(target_language), s).await
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
    let cfg = settings_inner(&s).await?;
    let rows: Vec<(i64, String, String, Option<String>)> = if let Some(ids) = ids {
        let mut out = vec![];
        for id in ids {
            if let Some(r) = sqlx::query("SELECT id,source_text FROM sentences WHERE id=?")
                .bind(id)
                .fetch_optional(&s.db)
                .await?
            {
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
    translation: String,
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
pub async fn next_exercise(
    last_id: Option<i64>,
    target_language: Option<String>,
    s: State<'_, AppState>,
) -> Result<Option<Exercise>> {
    let rows = sqlx::query(
        "SELECT sentence_id FROM sentence_languages WHERE status='ready' AND active_preparation_id IS NOT NULL AND (? IS NULL OR target_language=?)",
    )
    .bind(&target_language)
    .bind(&target_language)
    .fetch_all(&s.db)
    .await?;
    let ids = crate::exercise::next_cycle(rows.iter().map(|r| r.get(0)).collect(), last_id);
    let Some(id) = ids.first() else {
        return Ok(None);
    };
    let r=sqlx::query("SELECT s.source_text,p.translation,p.id FROM sentences s JOIN sentence_languages sl ON sl.sentence_id=s.id JOIN preparations p ON p.id=sl.active_preparation_id WHERE s.id=? AND (? IS NULL OR sl.target_language=?)").bind(id).bind(&target_language).bind(&target_language).fetch_one(&s.db).await?;
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
        translation: r.get(1),
        blocks,
    }))
}
