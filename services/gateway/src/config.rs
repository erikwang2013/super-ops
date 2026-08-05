use anyhow::Result;
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub auth: AuthConfig,
    pub database: DatabaseConfig,
    // Phase 2 plumbing (sessions, gRPC proxy) — deserialized from YAML, not yet read
    #[allow(dead_code)]
    pub redis: RedisConfig,
    #[allow(dead_code)]
    pub services: ServicesConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub http_port: u16,
    #[allow(dead_code)]
    pub grpc_port: u16,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AuthConfig {
    pub jwt_secret: String,
    pub access_token_ttl: u64,
    pub refresh_token_ttl: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseConfig {
    pub url: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RedisConfig {
    #[allow(dead_code)]
    pub url: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServicesConfig {
    #[allow(dead_code)]
    pub k8s: K8sServiceConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct K8sServiceConfig {
    #[allow(dead_code)]
    pub endpoint: String,
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = std::env::var("GATEWAY_CONFIG").unwrap_or_else(|_| "config/gateway.yaml".into());
        let content = std::fs::read_to_string(path)?;
        let mut config: Config = serde_yaml::from_str(&content)?;
        if let Ok(secret) = std::env::var("SUPEROPS_JWT_SECRET") {
            if !secret.is_empty() {
                config.auth.jwt_secret = secret;
            }
        }
        if config.auth.jwt_secret == "change-me-in-production" {
            tracing::warn!(
                "using default JWT secret — set SUPEROPS_JWT_SECRET (>=32 chars) in production"
            );
        }
        Ok(config)
    }
}
