use anyhow::Result;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    // Phase 2 (persistence) — deserialized from YAML, not yet read
    #[allow(dead_code)]
    pub database: DatabaseConfig,
}

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    pub grpc_port: u16,
}

#[derive(Debug, Deserialize)]
pub struct DatabaseConfig {
    #[allow(dead_code)]
    pub url: String,
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = std::env::var("K8S_CONFIG").unwrap_or_else(|_| "config/k8s-service.yaml".into());
        let content = std::fs::read_to_string(path)?;
        Ok(serde_yaml::from_str(&content)?)
    }
}
