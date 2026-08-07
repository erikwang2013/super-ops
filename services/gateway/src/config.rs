use anyhow::Result;
use ecat_data_clickhouse::ClickhouseConfig;
use ecat_data_s3::S3Config;
use ecat_mq_kafka::KafkaConfig;
use ecat_mq_mqtt::MqttConfig;
use ecat_mq_nats::NatsConfig;
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
    pub mqtt: Option<MqttConfig>,
    #[serde(default)]
    pub nats: Option<NatsConfig>,
    #[serde(default)]
    pub consul: Option<ConsulConfig>,
    #[serde(default)]
    pub etcd: Option<EtcdConfig>,
    #[serde(default)]
    pub otlp: Option<String>,
    #[serde(default)]
    pub oauth2: Option<OAuth2Config>,
    #[serde(default)]
    pub approval: ApprovalConfig,
    #[serde(default)]
    pub recording: RecordingConfig,
    #[serde(default)]
    pub terminal: TerminalConfig,
    #[serde(default)]
    pub graph: Option<GraphConfig>,
    #[serde(default)]
    pub search: Option<SearchConfig>,
    #[serde(default)]
    pub storage: Option<S3Config>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ConsulConfig {
    pub address: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EtcdConfig {
    pub endpoints: Vec<String>,
    #[serde(default = "default_etcd_prefix")]
    pub prefix: String,
}

fn default_etcd_prefix() -> String {
    "/superops/services".into()
}

#[derive(Debug, Deserialize, Clone)]
pub struct GraphConfig {
    #[serde(default = "default_graph_provider")]
    pub provider: String, // neo4j | nebulagraph | arangodb
    pub base_url: String,
    pub username: String,
    pub password: String,
    #[serde(default = "default_graph_space")]
    pub space: String, // nebulagraph space / arangodb db
}

fn default_graph_provider() -> String {
    "neo4j".into()
}

fn default_graph_space() -> String {
    "superops".into()
}

impl GraphConfig {
    pub fn is_neo4j(&self) -> bool {
        self.provider == "neo4j"
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct SearchConfig {
    #[serde(default = "default_search_provider")]
    pub provider: String, // elasticsearch | opensearch
    pub base_url: String,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default = "default_search_index")]
    pub index: String,
}

fn default_search_provider() -> String {
    "elasticsearch".into()
}

fn default_search_index() -> String {
    "superops-logs".into()
}

#[derive(Debug, Deserialize, Clone)]
pub struct TerminalConfig {
    /// 终端会话管控：require_confirm 时 ws 升级需带 confirm=1；max_session_secs 为会话时长上限
    pub require_confirm: bool,
    pub max_session_secs: u64,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            require_confirm: true,
            max_session_secs: 1800,
        }
    }
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct ApprovalConfig {
    /// 开启后删除 deployment 需先通过审批（approval 表有 approved 的 delete 单）。
    pub enabled: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RecordingConfig {
    /// 终端录制开关：false 时 exec ws 不落 ClickHouse。
    pub enabled: bool,
}

// 默认开启录制，保持旧行为；显式配置 recording.enabled: false 才关闭
impl Default for RecordingConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
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
    // MySQL 连接 TLS（可选）：ca_cert/client_cert/client_key 为 PEM 路径，skip_verify 跳过域名校验
    #[serde(default)]
    pub tls: Option<ecat_tls::TlsClientConfig>,
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
        if let Ok(secret) = std::env::var("SUPEROPS_JWT_SECRET")
            && !secret.is_empty()
        {
            config.auth.jwt_secret = secret;
        }
        if config.auth.jwt_secret == "change-me-in-production-0123456789abcdef" {
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

    #[test]
    fn recording_defaults_to_enabled() {
        let cfg: Config = serde_yaml::from_str(base_yaml()).unwrap();
        assert!(cfg.recording.enabled);
        let cfg: Config =
            serde_yaml::from_str(&format!("{}recording:\n  enabled: false\n", base_yaml()))
                .unwrap();
        assert!(!cfg.recording.enabled);
    }

    #[test]
    fn terminal_defaults_require_confirm_and_30min() {
        let cfg: Config = serde_yaml::from_str(base_yaml()).unwrap();
        assert!(cfg.terminal.require_confirm);
        assert_eq!(cfg.terminal.max_session_secs, 1800);
        let cfg: Config = serde_yaml::from_str(&format!(
            "{}terminal:\n  require_confirm: false\n  max_session_secs: 600\n",
            base_yaml()
        ))
        .unwrap();
        assert!(!cfg.terminal.require_confirm);
        assert_eq!(cfg.terminal.max_session_secs, 600);
    }
}
