mod commands;
mod db;
mod elevenlabs;
mod error;
mod exercise;
mod openai;
mod secrets;
mod sync;
use sqlx::SqlitePool;
use tauri::Manager;
#[derive(Clone)]
pub struct AppState {
    db: SqlitePool,
}
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let pool = tauri::async_runtime::block_on(db::connect(&dir.join("langai.sqlite3")))?;
            app.manage(AppState { db: pool });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_sentences,
            commands::sentence_details,
            commands::save_manual_translation,
            commands::save_sentence_audio,
            commands::delete_sentence_audio,
            commands::sentence_audio,
            commands::generate_sentence_audio,
            commands::add_sentences,
            commands::delete_sentences,
            commands::get_settings,
            commands::save_settings,
            commands::verify_api_key,
            commands::list_available_models,
            commands::save_api_key,
            commands::delete_api_key,
            commands::verify_elevenlabs_key,
            commands::save_elevenlabs_key,
            commands::delete_elevenlabs_key,
            commands::list_elevenlabs_voices,
            commands::save_elevenlabs_voice,
            commands::prepare_sentences,
            commands::list_topics,
            commands::exercise_languages,
            commands::exercise_topics,
            commands::next_exercise,
            sync::connect_sync_account,
            sync::sync_status,
            sync::disconnect_sync_account,
            sync::sync_now
        ])
        .run(tauri::generate_context!())
        .expect("error while running LangAI")
}
