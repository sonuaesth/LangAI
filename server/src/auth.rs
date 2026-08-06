use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use langai_contracts::{
    AuthResponse, AuthUser, CreateDeviceTokenRequest, CreateDeviceTokenResponse, DeviceResponse,
    LoginRequest, RegisterRequest, SessionResponse,
};
use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Transaction};
use subtle::ConstantTimeEq;
use uuid::Uuid;

use crate::{error::ApiError, state::AppState};

const SESSION_COOKIE: &str = "langai_session";
const CSRF_COOKIE: &str = "langai_csrf";

struct SessionTokens {
    session: String,
    csrf: String,
}

pub struct AuthContext {
    pub user_id: Uuid,
    #[allow(dead_code)] // Used by the upcoming sync operation audit trail.
    pub device_id: Option<Uuid>,
    session_csrf: Option<Vec<u8>>,
}

fn normalize_email(value: &str) -> Result<String, ApiError> {
    let email = value.trim().to_lowercase();
    if email.len() > 254
        || email.len() < 3
        || email.chars().any(char::is_whitespace)
        || email.matches('@').count() != 1
        || email.starts_with('@')
        || email.ends_with('@')
    {
        return Err(ApiError::InvalidInput("Invalid email address".into()));
    }
    Ok(email)
}

fn validate_password(value: &str) -> Result<(), ApiError> {
    if !(12..=200).contains(&value.chars().count()) {
        return Err(ApiError::InvalidInput(
            "Password must contain 12 to 200 characters".into(),
        ));
    }
    Ok(())
}

fn normalize_device_name(value: &str) -> Result<String, ApiError> {
    let name = value.trim();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(ApiError::InvalidInput(
            "Device name must contain 1 to 100 characters".into(),
        ));
    }
    Ok(name.to_owned())
}

async fn hash_password(password: String) -> Result<String, ApiError> {
    tokio::task::spawn_blocking(move || {
        Argon2::default()
            .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
            .map(|value| value.to_string())
            .map_err(|_| ApiError::Internal)
    })
    .await
    .map_err(|_| ApiError::Internal)?
}

async fn verify_password(password: String, encoded: String) -> Result<bool, ApiError> {
    tokio::task::spawn_blocking(move || {
        let parsed = PasswordHash::new(&encoded).map_err(|_| ApiError::Internal)?;
        Ok(Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok())
    })
    .await
    .map_err(|_| ApiError::Internal)?
}

fn random_token() -> String {
    let mut bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn token_hash(pepper: &str, token: &str) -> Vec<u8> {
    Sha256::new()
        .chain_update(pepper.as_bytes())
        .chain_update([0])
        .chain_update(token.as_bytes())
        .finalize()
        .to_vec()
}

fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|part| part.trim().split_once('='))
        .find_map(|(key, value)| (key == name).then(|| value.to_owned()))
}

fn set_auth_cookies(headers: &mut HeaderMap, state: &AppState, tokens: &SessionTokens) {
    let secure = if state.config.secure_cookies {
        "; Secure"
    } else {
        ""
    };
    let session = format!(
        "{SESSION_COOKIE}={}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}{}",
        tokens.session, state.config.session_ttl_seconds, secure
    );
    let csrf = format!(
        "{CSRF_COOKIE}={}; Path=/; SameSite=Strict; Max-Age={}{}",
        tokens.csrf, state.config.session_ttl_seconds, secure
    );
    headers.append(header::SET_COOKIE, HeaderValue::from_str(&session).unwrap());
    headers.append(header::SET_COOKIE, HeaderValue::from_str(&csrf).unwrap());
}

fn clear_auth_cookies(headers: &mut HeaderMap, secure: bool) {
    let suffix = if secure { "; Secure" } else { "" };
    for name in [SESSION_COOKIE, CSRF_COOKIE] {
        headers.append(
            header::SET_COOKIE,
            HeaderValue::from_str(&format!(
                "{name}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0{suffix}"
            ))
            .unwrap(),
        );
    }
}

async fn create_session(
    tx: &mut Transaction<'_, Postgres>,
    state: &AppState,
    user_id: Uuid,
) -> Result<SessionTokens, ApiError> {
    let tokens = SessionTokens {
        session: random_token(),
        csrf: random_token(),
    };
    sqlx::query(
        "INSERT INTO sessions(id,user_id,token_hash,csrf_hash,expires_at)
         VALUES($1,$2,$3,$4,now()+($5*interval '1 second'))",
    )
    .bind(Uuid::new_v4())
    .bind(user_id)
    .bind(token_hash(&state.config.session_pepper, &tokens.session))
    .bind(token_hash(&state.config.session_pepper, &tokens.csrf))
    .bind(state.config.session_ttl_seconds)
    .execute(&mut **tx)
    .await?;
    Ok(tokens)
}

async fn consume_invite(
    tx: &mut Transaction<'_, Postgres>,
    state: &AppState,
    invite: Option<&str>,
    email: &str,
    user_id: Uuid,
) -> Result<(), ApiError> {
    if state.config.registration_open {
        return Ok(());
    }
    let invite =
        invite.ok_or_else(|| ApiError::InvalidInput("Registration requires an invite".into()))?;
    let result = sqlx::query(
        "UPDATE registration_invites SET used_by=$1,used_at=now()
         WHERE token_hash=$2 AND used_at IS NULL AND expires_at>now()
         AND (email IS NULL OR email=$3)",
    )
    .bind(user_id)
    .bind(token_hash(&state.config.session_pepper, invite))
    .bind(email)
    .execute(&mut **tx)
    .await?;
    if result.rows_affected() != 1 {
        return Err(ApiError::InvalidInput(
            "Invite is invalid or expired".into(),
        ));
    }
    Ok(())
}

pub async fn register(
    State(state): State<AppState>,
    Json(input): Json<RegisterRequest>,
) -> Result<Response, ApiError> {
    let email = normalize_email(&input.email)?;
    validate_password(&input.password)?;
    let password_hash = hash_password(input.password).await?;
    let user_id = Uuid::new_v4();
    let mut tx = state.database.begin().await?;
    let inserted = sqlx::query(
        "INSERT INTO users(id,email,password_hash) VALUES($1,$2,$3)
         ON CONFLICT(email) DO NOTHING",
    )
    .bind(user_id)
    .bind(&email)
    .bind(password_hash)
    .execute(&mut *tx)
    .await?;
    if inserted.rows_affected() != 1 {
        return Err(ApiError::Conflict(
            "An account with this email already exists".into(),
        ));
    }
    consume_invite(&mut tx, &state, input.invite.as_deref(), &email, user_id).await?;
    let tokens = create_session(&mut tx, &state, user_id).await?;
    tx.commit().await?;
    let mut headers = HeaderMap::new();
    set_auth_cookies(&mut headers, &state, &tokens);
    Ok((
        StatusCode::CREATED,
        headers,
        Json(AuthResponse {
            user: AuthUser { id: user_id, email },
        }),
    )
        .into_response())
}

pub async fn login(
    State(state): State<AppState>,
    Json(input): Json<LoginRequest>,
) -> Result<Response, ApiError> {
    let email = normalize_email(&input.email)?;
    let user = sqlx::query_as::<_, (Uuid, String, String)>(
        "SELECT id,email,password_hash FROM users WHERE email=$1 AND disabled_at IS NULL",
    )
    .bind(&email)
    .fetch_optional(&state.database)
    .await?;
    let Some((user_id, email, encoded)) = user else {
        let _ = hash_password(input.password).await?;
        return Err(ApiError::InvalidCredentials);
    };
    if !verify_password(input.password, encoded).await? {
        return Err(ApiError::InvalidCredentials);
    }
    let mut tx = state.database.begin().await?;
    let tokens = create_session(&mut tx, &state, user_id).await?;
    tx.commit().await?;
    let mut headers = HeaderMap::new();
    set_auth_cookies(&mut headers, &state, &tokens);
    Ok((
        headers,
        Json(AuthResponse {
            user: AuthUser { id: user_id, email },
        }),
    )
        .into_response())
}

async fn session_from_headers(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(Uuid, String, Uuid, Vec<u8>), ApiError> {
    let token = cookie(headers, SESSION_COOKIE).ok_or(ApiError::Unauthorized)?;
    sqlx::query_as::<_, (Uuid, String, Uuid, Vec<u8>)>(
        "SELECT u.id,u.email,s.id,s.csrf_hash FROM sessions s
         JOIN users u ON u.id=s.user_id
         WHERE s.token_hash=$1 AND s.revoked_at IS NULL AND s.expires_at>now()
         AND u.disabled_at IS NULL",
    )
    .bind(token_hash(&state.config.session_pepper, &token))
    .fetch_optional(&state.database)
    .await?
    .ok_or(ApiError::Unauthorized)
}

pub async fn device_user_from_headers(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(Uuid, String, Uuid), ApiError> {
    let authorization = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or(ApiError::Unauthorized)?;
    let token = authorization
        .strip_prefix("Bearer ")
        .filter(|value| !value.is_empty())
        .ok_or(ApiError::Unauthorized)?;
    let device = sqlx::query_as::<_, (Uuid, String, Uuid)>(
        "UPDATE devices d SET last_seen_at=now()
         FROM users u
         WHERE d.user_id=u.id AND d.token_hash=$1 AND d.revoked_at IS NULL
         AND u.disabled_at IS NULL
         RETURNING u.id,u.email,d.id",
    )
    .bind(token_hash(&state.config.session_pepper, token))
    .fetch_optional(&state.database)
    .await?;
    device.ok_or(ApiError::Unauthorized)
}

fn require_csrf(
    state: &AppState,
    headers: &HeaderMap,
    expected_csrf: &[u8],
) -> Result<(), ApiError> {
    let csrf_header = headers
        .get("x-csrf-token")
        .and_then(|value| value.to_str().ok())
        .ok_or(ApiError::Forbidden)?;
    let csrf_cookie = cookie(headers, CSRF_COOKIE).ok_or(ApiError::Forbidden)?;
    if csrf_header
        .as_bytes()
        .ct_eq(csrf_cookie.as_bytes())
        .unwrap_u8()
        != 1
        || token_hash(&state.config.session_pepper, csrf_header)
            .as_slice()
            .ct_eq(expected_csrf)
            .unwrap_u8()
            != 1
    {
        return Err(ApiError::Forbidden);
    }
    Ok(())
}

pub async fn authenticate_request(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<AuthContext, ApiError> {
    if headers.contains_key(header::AUTHORIZATION) {
        let (user_id, _, device_id) = device_user_from_headers(state, headers).await?;
        return Ok(AuthContext {
            user_id,
            device_id: Some(device_id),
            session_csrf: None,
        });
    }
    let (user_id, _, _, expected_csrf) = session_from_headers(state, headers).await?;
    Ok(AuthContext {
        user_id,
        device_id: None,
        session_csrf: Some(expected_csrf),
    })
}

pub fn require_mutation_auth(
    state: &AppState,
    headers: &HeaderMap,
    auth: &AuthContext,
) -> Result<(), ApiError> {
    match &auth.session_csrf {
        Some(expected) => require_csrf(state, headers, expected),
        None => Ok(()),
    }
}

pub async fn create_device_token(
    State(state): State<AppState>,
    Json(input): Json<CreateDeviceTokenRequest>,
) -> Result<(StatusCode, Json<CreateDeviceTokenResponse>), ApiError> {
    let email = normalize_email(&input.email)?;
    let device_name = normalize_device_name(&input.device_name)?;
    let user = sqlx::query_as::<_, (Uuid, String)>(
        "SELECT id,password_hash FROM users WHERE email=$1 AND disabled_at IS NULL",
    )
    .bind(email)
    .fetch_optional(&state.database)
    .await?;
    let Some((user_id, encoded)) = user else {
        let _ = hash_password(input.password).await?;
        return Err(ApiError::InvalidCredentials);
    };
    if !verify_password(input.password, encoded).await? {
        return Err(ApiError::InvalidCredentials);
    }
    let device_id = Uuid::new_v4();
    let token = random_token();
    sqlx::query("INSERT INTO devices(id,user_id,name,token_hash) VALUES($1,$2,$3,$4)")
        .bind(device_id)
        .bind(user_id)
        .bind(device_name)
        .bind(token_hash(&state.config.session_pepper, &token))
        .execute(&state.database)
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(CreateDeviceTokenResponse { device_id, token }),
    ))
}

pub async fn list_devices(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<DeviceResponse>>, ApiError> {
    let (user_id, _, _, _) = session_from_headers(&state, &headers).await?;
    let devices = sqlx::query_as::<_, (Uuid, String, Option<String>, String)>(
        "SELECT id,name,
         CASE WHEN last_seen_at IS NULL THEN NULL
              ELSE to_char(last_seen_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') END,
         to_char(created_at, 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')
         FROM devices
         WHERE user_id=$1 AND revoked_at IS NULL ORDER BY created_at DESC",
    )
    .bind(user_id)
    .fetch_all(&state.database)
    .await?
    .into_iter()
    .map(|(id, name, last_seen_at, created_at)| DeviceResponse {
        id,
        name,
        last_seen_at,
        created_at,
    })
    .collect();
    Ok(Json(devices))
}

pub async fn revoke_device(
    State(state): State<AppState>,
    Path(device_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let (user_id, _, _, expected_csrf) = session_from_headers(&state, &headers).await?;
    require_csrf(&state, &headers, &expected_csrf)?;
    let result = sqlx::query(
        "UPDATE devices SET revoked_at=now() WHERE id=$1 AND user_id=$2 AND revoked_at IS NULL",
    )
    .bind(device_id)
    .bind(user_id)
    .execute(&state.database)
    .await?;
    if result.rows_affected() != 1 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn current_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SessionResponse>, ApiError> {
    match session_from_headers(&state, &headers).await {
        Ok((id, email, _, _)) => Ok(Json(SessionResponse {
            authenticated: true,
            user: Some(AuthUser { id, email }),
        })),
        Err(ApiError::Unauthorized) => Ok(Json(SessionResponse {
            authenticated: false,
            user: None,
        })),
        Err(error) => Err(error),
    }
}

pub async fn current_device_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SessionResponse>, ApiError> {
    let (id, email, _) = device_user_from_headers(&state, &headers).await?;
    Ok(Json(SessionResponse {
        authenticated: true,
        user: Some(AuthUser { id, email }),
    }))
}

pub async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let (_, _, session_id, expected_csrf) = session_from_headers(&state, &headers).await?;
    require_csrf(&state, &headers, &expected_csrf)?;
    sqlx::query("UPDATE sessions SET revoked_at=now() WHERE id=$1")
        .bind(session_id)
        .execute(&state.database)
        .await?;
    let mut response_headers = HeaderMap::new();
    clear_auth_cookies(&mut response_headers, state.config.secure_cookies);
    Ok((StatusCode::NO_CONTENT, response_headers).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_is_normalized_and_validated() {
        assert_eq!(
            normalize_email(" User@Example.COM ").unwrap(),
            "user@example.com"
        );
        assert!(normalize_email("invalid").is_err());
        assert!(normalize_email("a @example.com").is_err());
    }

    #[tokio::test]
    async fn password_hash_is_salted_and_verifiable() {
        let first = hash_password("a sufficiently long password".into())
            .await
            .unwrap();
        let second = hash_password("a sufficiently long password".into())
            .await
            .unwrap();
        assert_ne!(first, second);
        assert!(
            verify_password("a sufficiently long password".into(), first)
                .await
                .unwrap()
        );
        assert!(!verify_password("wrong password".into(), second)
            .await
            .unwrap());
    }

    #[test]
    fn cookie_parser_does_not_match_name_prefixes() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            "xlangai_session=bad; langai_session=good".parse().unwrap(),
        );
        assert_eq!(cookie(&headers, SESSION_COOKIE).as_deref(), Some("good"));
    }
}
