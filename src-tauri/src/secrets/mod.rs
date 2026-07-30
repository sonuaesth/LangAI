use crate::error::{AppError, Result};
const SERVICE: &str = "LangAI";
const USER: &str = "openai-api-key";
const ELEVENLABS_USER: &str = "elevenlabs-api-key";
fn entry(user: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, user).map_err(|e| AppError::Secret(e.to_string()))
}
fn set_for(user: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(AppError::Input("API key is empty".into()));
    }
    entry(user)?
        .set_password(value)
        .map_err(|e| AppError::Secret(e.to_string()))
}
fn get_for(user: &str) -> Result<Option<String>> {
    match entry(user)?.get_password() {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::Secret(e.to_string())),
    }
}
fn delete_for(user: &str) -> Result<()> {
    match entry(user)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(AppError::Secret(e.to_string())),
    }
}
pub fn set(value: &str) -> Result<()> {
    set_for(USER, value)
}
pub fn get() -> Result<Option<String>> {
    get_for(USER)
}
pub fn delete() -> Result<()> {
    delete_for(USER)
}
pub fn set_elevenlabs(value: &str) -> Result<()> {
    set_for(ELEVENLABS_USER, value)
}
pub fn get_elevenlabs() -> Result<Option<String>> {
    get_for(ELEVENLABS_USER)
}
pub fn delete_elevenlabs() -> Result<()> {
    delete_for(ELEVENLABS_USER)
}
