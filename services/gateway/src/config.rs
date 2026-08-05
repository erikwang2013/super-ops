use anyhow::Result;
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub auth: AuthConfig,
    pub database: DatabaseConfig,
    pub redis: RedisConfig,
    pub services: ServicesConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig { pub http_port: u16, pub grpc_port: u16 }

#[derive(Debug, Deserialize, Clone)]
pub struct AuthConfig {
    pub jwt_secret: String,
    pub access_token_ttl: u64,
    pub refresh_token_ttl: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseConfig { pub url: String }

#[derive(Debug, Deserialize, Clone)]
pub struct RedisConfig { pub url: String }

#[derive(Debug, Deserialize, Clone)]
pub struct ServicesConfig { pub k8s: K8sServiceConfig }

#[derive(Debug, Deserialize, Clone)]
pub struct K8sServiceConfig { pub endpoint: String }

impl Config {
    pub fn load() -> Result<Self> {
        let path = std::env::var("GATEWAY_CONFIG")
            .unwrap_or_else(|_| "config/gateway.yaml".into());
        let content = std::fs::read_to_string(path)?;
        Ok(serde_yaml::from_str(&content)?)
    }
}
