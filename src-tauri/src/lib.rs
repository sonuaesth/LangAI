mod commands;
mod db;
mod error;
mod exercise;
mod openai;
mod secrets;
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
            commands::add_sentences,
            commands::delete_sentences,
            commands::get_settings,
            commands::save_settings,
            commands::verify_api_key,
            commands::save_api_key,
            commands::delete_api_key,
            commands::prepare_sentences,
            commands::next_exercise
        ])
        .run(tauri::generate_context!())
        .expect("error while running LangAI")
}
