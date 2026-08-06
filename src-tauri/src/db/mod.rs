use crate::error::Result;
use sqlx::{sqlite::SqliteConnectOptions, SqlitePool};
use std::{path::Path, str::FromStr};

fn backup_before_cloud_migration(path: &Path) -> std::io::Result<Option<std::path::PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }
    let backup = path.with_file_name("langai-before-cloud-sync.sqlite3");
    if backup.exists() {
        return Ok(Some(backup));
    }
    std::fs::copy(path, &backup)?;
    for suffix in ["-wal", "-shm"] {
        let source = std::path::PathBuf::from(format!("{}{suffix}", path.display()));
        if source.exists() {
            std::fs::copy(
                &source,
                std::path::PathBuf::from(format!("{}{suffix}", backup.display())),
            )?;
        }
    }
    Ok(Some(backup))
}

pub async fn connect(path: &Path) -> Result<SqlitePool> {
    let backup = backup_before_cloud_migration(path)?;
    let url = format!("sqlite:{}", path.display());
    let options = SqliteConnectOptions::from_str(&url)?
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = SqlitePool::connect_with(options).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    if let Some(backup) = backup {
        sqlx::query("UPDATE sync_import_state SET backup_path=COALESCE(backup_path,?) WHERE id=1")
            .bind(backup.to_string_lossy().as_ref())
            .execute(&pool)
            .await?;
    }
    Ok(pool)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn migrations_apply() {
        let p = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&p).await.unwrap();
        let n: (i64,) = sqlx::query_as("SELECT count(*) FROM settings")
            .fetch_one(&p)
            .await
            .unwrap();
        assert_eq!(n.0, 1);
        sqlx::query("INSERT INTO sentences(source_text,target_language) VALUES('Hello','German')")
            .execute(&p)
            .await
            .unwrap();
        let language: (String,) = sqlx::query_as("SELECT target_language FROM sentences")
            .fetch_one(&p)
            .await
            .unwrap();
        assert_eq!(language.0, "German");
        let sentence_id: (i64,) = sqlx::query_as("SELECT id FROM sentences")
            .fetch_one(&p)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO sentence_languages(sentence_id,target_language) VALUES(?, 'German')",
        )
        .bind(sentence_id.0)
        .execute(&p)
        .await
        .unwrap();
        sqlx::query("INSERT OR IGNORE INTO sentence_languages(sentence_id,target_language) VALUES(?, 'English')")
            .bind(sentence_id.0).execute(&p).await.unwrap();
        let language_count: (i64,) =
            sqlx::query_as("SELECT count(*) FROM sentence_languages WHERE sentence_id=?")
                .bind(sentence_id.0)
                .fetch_one(&p)
                .await
                .unwrap();
        assert_eq!(language_count.0, 2);
        let sync_state: (String, i64) =
            sqlx::query_as("SELECT status,pull_cursor FROM sync_state WHERE id=1")
                .fetch_one(&p)
                .await
                .unwrap();
        assert_eq!(sync_state, ("disconnected".into(), 0));
    }
}
