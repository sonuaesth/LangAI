pub mod types;
pub mod validate;
use crate::error::{AppError, Result};
use rand::Rng;
use reqwest::{header::RETRY_AFTER, Client, StatusCode};
use serde_json::{json, Value};
use std::time::Duration;
use types::Generated;
const URL: &str = "https://api.openai.com/v1/responses";
pub async fn models(key: &str) -> Result<Vec<String>> {
    let r = Client::new()
        .get("https://api.openai.com/v1/models")
        .bearer_auth(key)
        .send()
        .await?;
    if !r.status().is_success() {
        return Err(AppError::OpenAi(format!(
            "Key verification failed ({})",
            r.status()
        )));
    }
    let v: Value = r.json().await?;
    Ok(v["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|x| x["id"].as_str().map(str::to_owned))
        .collect())
}
fn schema() -> Value {
    json!({"type":"object","additionalProperties":false,"required":["source_text","target_language","translation","blocks"],"properties":{"source_text":{"type":"string"},"target_language":{"type":"string"},"translation":{"type":"string"},"blocks":{"type":"array","minItems":1,"maxItems":50,"items":{"type":"object","additionalProperties":false,"required":["position","correct","distractors","hint"],"properties":{"position":{"type":"integer","minimum":0},"correct":{"type":"string"},"distractors":{"type":"array","minItems":4,"maxItems":4,"items":{"type":"string"}},"hint":{"type":["string","null"]}}}}}})
}
fn retryable(s: StatusCode) -> bool {
    s == StatusCode::TOO_MANY_REQUESTS || s.is_server_error()
}
pub async fn generate(key: &str, model: &str, language: &str, source: &str) -> Result<Generated> {
    let client = Client::builder().timeout(Duration::from_secs(60)).build()?;
    for attempt in 0..3 {
        let body = json!({"model":model,"instructions":"Translate naturally. Split the exact translation into ordered semantic blocks (words, phrases, or attached punctuation). For every block provide exactly four plausible but unambiguously wrong alternatives. Hint must be short and must not reveal the answer. The source language may be any language.","input":format!("Target language: {language}\nSource: {source}"),"text":{"format":{"type":"json_schema","name":"translation_exercise","strict":true,"schema":schema()}}});
        let sent = client.post(URL).bearer_auth(key).json(&body).send().await;
        match sent {
            Ok(r) if r.status().is_success() => {
                let v: Value = r.json().await?;
                let text = v["output"]
                    .as_array()
                    .and_then(|a| {
                        a.iter()
                            .flat_map(|o| o["content"].as_array().into_iter().flatten())
                            .find_map(|c| c["text"].as_str())
                    })
                    .ok_or_else(|| {
                        AppError::OpenAi("Response did not contain structured output".into())
                    })?;
                let mut parsed: Generated =
                    serde_json::from_str(text).map_err(|e| AppError::Validation(e.to_string()))?;
                // Array order is authoritative; normalize the redundant field because
                // models sometimes return one-based positions despite the schema.
                for (position, block) in parsed.blocks.iter_mut().enumerate() {
                    block.position = position;
                }
                validate::validate(&parsed)?;
                return Ok(parsed);
            }
            Ok(r) => {
                let status = r.status();
                let wait = r
                    .headers()
                    .get(RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok());
                let detail = r.text().await.unwrap_or_default();
                if !retryable(status) || attempt == 2 {
                    return Err(AppError::OpenAi(format!("HTTP {status}: {detail}")));
                }
                let jitter = rand::thread_rng().gen_range(0..=1);
                let delay = wait.unwrap_or((1 << attempt) + jitter);
                tokio::time::sleep(Duration::from_secs(delay)).await
            }
            Err(e) => {
                if attempt == 2 || (!e.is_timeout() && !e.is_connect()) {
                    return Err(e.into());
                }
                let jitter = rand::thread_rng().gen_range(0..=1);
                let delay = (1 << attempt) + jitter;
                tokio::time::sleep(Duration::from_secs(delay)).await
            }
        }
    }
    unreachable!()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retry_policy() {
        assert!(retryable(StatusCode::TOO_MANY_REQUESTS));
        assert!(retryable(StatusCode::BAD_GATEWAY));
        assert!(!retryable(StatusCode::UNAUTHORIZED));
        assert!(!retryable(StatusCode::BAD_REQUEST))
    }
}
