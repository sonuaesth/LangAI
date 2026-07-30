use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiErrorBody {
    pub error: ApiErrorDetail,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiErrorDetail {
    pub code: &'static str,
    pub message: String,
    pub request_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    pub status: String,
    pub database: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    pub invite: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateDeviceTokenRequest {
    pub email: String,
    pub password: String,
    pub device_name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateDeviceTokenResponse {
    pub device_id: Uuid,
    pub token: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceResponse {
    pub id: Uuid,
    pub name: String,
    pub last_seen_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthUser {
    pub id: Uuid,
    pub email: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthResponse {
    pub user: AuthUser,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionResponse {
    pub authenticated: bool,
    pub user: Option<AuthUser>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsResponse {
    pub model: String,
    pub target_language: String,
    pub elevenlabs_voice_id: Option<String>,
    pub elevenlabs_voice_name: Option<String>,
    pub revision: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSettingsRequest {
    pub model: String,
    pub target_language: String,
    pub elevenlabs_voice_id: Option<String>,
    pub elevenlabs_voice_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSentenceRequest {
    pub source_text: String,
    pub target_languages: Vec<String>,
    #[serde(default)]
    pub topics: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SentenceLanguageResponse {
    pub target_language: String,
    pub status: String,
    pub error: Option<String>,
    pub translation_comment: Option<String>,
    pub audio_available: bool,
    pub revision: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SentenceResponse {
    pub id: Uuid,
    pub source_text: String,
    pub created_at: String,
    pub revision: i64,
    pub languages: Vec<SentenceLanguageResponse>,
    pub topics: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveProviderKeyRequest {
    pub api_key: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderKeyStatus {
    pub provider: String,
    pub configured: bool,
    pub key_hint: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareSentenceRequest {
    pub target_language: String,
    pub translation_comment: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_contract_uses_camel_case_and_never_leaks_extra_fields() {
        let body = ApiErrorBody {
            error: ApiErrorDetail {
                code: "invalid_input",
                message: "Invalid value".into(),
                request_id: Some("request-1".into()),
            },
        };
        assert_eq!(
            serde_json::to_value(body).unwrap(),
            serde_json::json!({
                "error": {
                    "code": "invalid_input",
                    "message": "Invalid value",
                    "requestId": "request-1"
                }
            })
        );
    }
}
