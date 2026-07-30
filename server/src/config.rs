use std::{net::SocketAddr, str::FromStr};

#[derive(Clone)]
pub struct Config {
    pub bind: SocketAddr,
    pub database_url: String,
    pub environment: String,
    pub session_pepper: String,
    pub session_ttl_seconds: i64,
    pub registration_open: bool,
    pub secure_cookies: bool,
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
        Ok(Self {
            bind: SocketAddr::from_str(&bind)
                .map_err(|error| anyhow::anyhow!("invalid LANGAI_BIND: {error}"))?,
            database_url,
            environment,
            session_pepper,
            session_ttl_seconds,
            registration_open,
            secure_cookies,
        })
    }
}
