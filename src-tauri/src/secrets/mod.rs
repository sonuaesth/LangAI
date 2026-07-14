use crate::error::{AppError, Result};
const SERVICE: &str = "LangAI";
const USER: &str = "openai-api-key";
fn entry() -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, USER).map_err(|e| AppError::Secret(e.to_string()))
}
pub fn set(value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(AppError::Input("API key is empty".into()));
    }
    entry()?
        .set_password(value)
        .map_err(|e| AppError::Secret(e.to_string()))
}
pub fn get() -> Result<Option<String>> {
    match entry()?.get_password() {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::Secret(e.to_string())),
    }
}
pub fn delete() -> Result<()> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(AppError::Secret(e.to_string())),
    }
}
