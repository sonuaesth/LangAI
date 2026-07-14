use crate::error::Result;
use sqlx::{sqlite::SqliteConnectOptions, SqlitePool};
use std::{path::Path, str::FromStr};
pub async fn connect(path: &Path) -> Result<SqlitePool> {
    let url = format!("sqlite:{}", path.display());
    let options = SqliteConnectOptions::from_str(&url)?
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = SqlitePool::connect_with(options).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
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
    }
}
