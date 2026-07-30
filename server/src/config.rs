use std::{net::SocketAddr, str::FromStr};

#[derive(Clone)]
pub struct Config {
    pub bind: SocketAddr,
    pub database_url: String,
    pub environment: String,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let bind = std::env::var("LANGAI_BIND").unwrap_or_else(|_| "0.0.0.0:8080".into());
        let database_url = std::env::var("DATABASE_URL")
            .map_err(|_| anyhow::anyhow!("DATABASE_URL is required"))?;
        let environment = std::env::var("LANGAI_ENV").unwrap_or_else(|_| "development".into());
        Ok(Self {
            bind: SocketAddr::from_str(&bind)
                .map_err(|error| anyhow::anyhow!("invalid LANGAI_BIND: {error}"))?,
            database_url,
            environment,
        })
    }
}
