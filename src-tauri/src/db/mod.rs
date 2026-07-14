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
        assert_eq!(n.0, 1)
    }
}
