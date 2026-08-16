use anyhow::Result;
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub server: ServerConfig,
    // Phase 2 (persistence) — deserialized from YAML, not yet read
    #[allow(dead_code)]
    pub database: DatabaseConfig,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub consul: Option<ConsulConfig>,
    #[serde(default)]
    pub otlp: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub grpc_port: u16,
}

/// 服务间鉴权：`token` 非空时启用，所有 gRPC 请求须携带 `authorization: Bearer <token>`。
#[derive(Debug, Deserialize, Clone, Default)]
pub struct AuthConfig {
    #[serde(default)]
    pub token: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseConfig {
    #[allow(dead_code)]
    pub url: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ConsulConfig {
    pub address: String,
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = std::env::var("K8S_CONFIG").unwrap_or_else(|_| "config/k8s-service.yaml".into());
        let content = std::fs::read_to_string(path)?;
        Ok(serde_yaml::from_str(&content)?)
    }
}
