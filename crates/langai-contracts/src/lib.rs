use serde::{Deserialize, Serialize};

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
