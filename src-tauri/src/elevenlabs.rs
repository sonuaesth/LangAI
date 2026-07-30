use crate::error::{AppError, Result};
use serde::Serialize;
use serde_json::{json, Value};

const BASE: &str = "https://api.elevenlabs.io";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Voice {
    pub voice_id: String,
    pub name: String,
}

pub async fn voices(key: &str) -> Result<Vec<Voice>> {
    let response = reqwest::Client::new()
        .get(format!(
            "{BASE}/v2/voices?page_size=100&sort=name&sort_direction=asc"
        ))
        .header("xi-api-key", key)
        .send()
        .await?;
    let status = response.status();
    if !status.is_success() {
        return Err(AppError::ElevenLabs(format!(
            "HTTP {status}: {}",
            response.text().await.unwrap_or_default()
        )));
    }
    let value: Value = response.json().await?;
    Ok(value["voices"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|voice| {
            Some(Voice {
                voice_id: voice["voice_id"].as_str()?.to_owned(),
                name: voice["name"].as_str()?.to_owned(),
            })
        })
        .collect())
}

pub async fn synthesize(key: &str, voice_id: &str, text: &str) -> Result<Vec<u8>> {
    let response = reqwest::Client::new()
        .post(format!(
            "{BASE}/v1/text-to-speech/{voice_id}?output_format=mp3_44100_128"
        ))
        .header("xi-api-key", key)
        .json(&json!({"text": text, "model_id": "eleven_multilingual_v2"}))
        .send()
        .await?;
    let status = response.status();
    if !status.is_success() {
        return Err(AppError::ElevenLabs(format!(
            "HTTP {status}: {}",
            response.text().await.unwrap_or_default()
        )));
    }
    Ok(response.bytes().await?.to_vec())
}
