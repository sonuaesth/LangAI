use serde::Serialize;
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Database: {0}")]
    Db(#[from] sqlx::Error),
    #[error("Migration: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("Network: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Secure storage: {0}")]
    Secret(String),
    #[error("OpenAI: {0}")]
    OpenAi(String),
    #[error("Invalid response: {0}")]
    Validation(String),
    #[error("{0}")]
    Input(String),
}
impl Serialize for AppError {
    fn serialize<S>(&self, s: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        s.serialize_str(&self.to_string())
    }
}
pub type Result<T> = std::result::Result<T, AppError>;
