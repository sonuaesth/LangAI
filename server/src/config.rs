use std::{net::SocketAddr, path::PathBuf, str::FromStr};

use base64::{engine::general_purpose::STANDARD, Engine};

#[derive(Clone)]
pub struct Config {
    pub bind: SocketAddr,
    pub database_url: String,
    pub environment: String,
    pub session_pepper: String,
    pub session_ttl_seconds: i64,
    pub registration_open: bool,
    pub secure_cookies: bool,
    pub secrets_master_key: [u8; 32],
    pub audio_storage_path: PathBuf,
    pub max_audio_bytes: usize,
    pub web_root: PathBuf,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let bind = std::env::var("LANGAI_BIND").unwrap_or_else(|_| "0.0.0.0:8080".into());
        let database_url = std::env::var("DATABASE_URL")
            .map_err(|_| anyhow::anyhow!("DATABASE_URL is required"))?;
        let environment = std::env::var("LANGAI_ENV").unwrap_or_else(|_| "development".into());
        let session_pepper = std::env::var("SESSION_PEPPER")
            .map_err(|_| anyhow::anyhow!("SESSION_PEPPER is required"))?;
        if session_pepper.as_bytes().len() < 32 {
            anyhow::bail!("SESSION_PEPPER must contain at least 32 bytes");
        }
        let session_ttl_seconds = std::env::var("SESSION_TTL_SECONDS")
            .unwrap_or_else(|_| "2592000".into())
            .parse::<i64>()
            .map_err(|_| anyhow::anyhow!("SESSION_TTL_SECONDS must be an integer"))?;
        if !(300..=31_536_000).contains(&session_ttl_seconds) {
            anyhow::bail!("SESSION_TTL_SECONDS must be between 300 and 31536000");
        }
        let registration_open = match std::env::var("REGISTRATION_MODE")
            .unwrap_or_else(|_| "invite".into())
            .as_str()
        {
            "open" => true,
            "invite" => false,
            _ => anyhow::bail!("REGISTRATION_MODE must be open or invite"),
        };
        let secure_cookies = environment != "development";
        let encoded_master_key = std::env::var("SECRETS_MASTER_KEY")
            .map_err(|_| anyhow::anyhow!("SECRETS_MASTER_KEY is required"))?;
        let decoded_master_key = STANDARD
            .decode(encoded_master_key)
            .map_err(|_| anyhow::anyhow!("SECRETS_MASTER_KEY must be valid base64"))?;
        let secrets_master_key: [u8; 32] = decoded_master_key
            .try_into()
            .map_err(|_| anyhow::anyhow!("SECRETS_MASTER_KEY must decode to exactly 32 bytes"))?;
        let audio_storage_path = std::env::var("AUDIO_STORAGE_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("./data/audio"));
        let max_audio_bytes = std::env::var("MAX_AUDIO_BYTES")
            .unwrap_or_else(|_| "26214400".into())
            .parse::<usize>()
            .map_err(|_| anyhow::anyhow!("MAX_AUDIO_BYTES must be a positive integer"))?;
        if max_audio_bytes == 0 || max_audio_bytes > 100 * 1024 * 1024 {
            anyhow::bail!("MAX_AUDIO_BYTES must be between 1 and 104857600");
        }
        let web_root = std::env::var("WEB_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("./out"));
        Ok(Self {
            bind: SocketAddr::from_str(&bind)
                .map_err(|error| anyhow::anyhow!("invalid LANGAI_BIND: {error}"))?,
            database_url,
            environment,
            session_pepper,
            session_ttl_seconds,
            registration_open,
            secure_cookies,
            secrets_master_key,
            audio_storage_path,
            max_audio_bytes,
            web_root,
        })
    }
}
