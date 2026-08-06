use anyhow::Result;
use ecat_data_clickhouse::ClickhouseConfig;
use ecat_mq_kafka::KafkaConfig;
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub auth: AuthConfig,
    pub database: DatabaseConfig,
    pub redis: RedisConfig,
    pub services: ServicesConfig,
    pub ch: ClickhouseConfig,
    #[serde(default)]
    pub mq: Option<KafkaConfig>,
    #[serde(default)]
    pub consul: Option<ConsulConfig>,
    #[serde(default)]
    pub otlp: Option<String>,
    #[serde(default)]
    pub oauth2: Option<OAuth2Config>,
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

#[derive(Debug, Deserialize, Clone)]
pub struct ConsulConfig {
    pub address: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct OAuth2Config {
    pub introspection_url: String,
    pub client_id: String,
    pub client_secret: String,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn base_yaml() -> &'static str {
        "server:\n  http_port: 8080\n  grpc_port: 9090\nauth:\n  jwt_secret: x\n  access_token_ttl: 1\n  refresh_token_ttl: 2\ndatabase:\n  url: mysql://u:p@h/d\nredis:\n  url: redis://h\nservices:\n  k8s:\n    endpoint: http://h\nch:\n  base_url: http://h\n  database: d\n"
    }

    #[test]
    fn oauth2_defaults_to_none() {
        let cfg: Config = serde_yaml::from_str(base_yaml()).unwrap();
        assert!(cfg.oauth2.is_none());
    }

    #[test]
    fn oauth2_parses_when_present() {
        let yaml = format!(
            "{}oauth2:\n  introspection_url: https://idp/oauth/introspect\n  client_id: cid\n  client_secret: secret\n",
            base_yaml()
        );
        let cfg: Config = serde_yaml::from_str(&yaml).unwrap();
        let o = cfg.oauth2.unwrap();
        assert_eq!(o.introspection_url, "https://idp/oauth/introspect");
        assert_eq!(o.client_id, "cid");
    }
}
