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
    pub active_preparation: Option<PreparationResponse>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparationResponse {
    pub id: Uuid,
    pub version: i32,
    pub model: String,
    pub translation: String,
    pub blocks: Vec<ExerciseBlockResponse>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExerciseBlockResponse {
    pub id: Uuid,
    pub position: i32,
    pub correct: String,
    pub hint: Option<String>,
    pub options: Vec<ExerciseOptionResponse>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExerciseOptionResponse {
    pub id: Uuid,
    pub text: String,
    pub is_correct: bool,
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateAudioRequest {
    pub target_language: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioMetadataResponse {
    pub sha256: String,
    pub size: i64,
    pub mime: String,
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncPushRequest {
    pub operations: Vec<SyncPushOperation>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncPushOperation {
    pub operation_id: Uuid,
    pub kind: String,
    pub entity_id: Uuid,
    pub base_revision: Option<i64>,
    pub payload: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SyncOperationResult {
    pub operation_id: Uuid,
    pub entity_id: Uuid,
    pub revision: i64,
    pub replayed: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncPushResponse {
    pub results: Vec<SyncOperationResult>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncChangeResponse {
    pub sequence: i64,
    pub entity_type: String,
    pub entity_id: Uuid,
    pub operation: String,
    pub revision: i64,
    pub payload: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncPullResponse {
    pub cursor: i64,
    pub has_more: bool,
    pub changes: Vec<SyncChangeResponse>,
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
