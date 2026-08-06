use ecat_data_clickhouse::ClickhouseConfig;
use ecat_data_redis::RedisConfig;
use ecat_mq_kafka::KafkaConfig;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct K8sConfig {
    pub endpoint: String,
}

impl Default for K8sConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:9091".into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CollectorConfig {
    pub collect_interval_secs: u64,
    pub inspect_interval_secs: u64,
    pub max_not_ready: usize,
    pub alert_consecutive: usize,
}

impl Default for CollectorConfig {
    fn default() -> Self {
        Self {
            collect_interval_secs: 60,
            inspect_interval_secs: 600,
            max_not_ready: 1,
            alert_consecutive: 2,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConsulConfig {
    pub address: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub ch: ClickhouseConfig,
    pub mq: KafkaConfig,
    pub lock: RedisConfig,
    #[serde(default)]
    pub k8s: K8sConfig,
    #[serde(default)]
    pub collector: CollectorConfig,
    #[serde(default)]
    pub consul: Option<ConsulConfig>,
}

pub fn collector_config_from(path: &str) -> anyhow::Result<Config> {
    let raw = std::fs::read_to_string(path)?;
    let cfg: Config = serde_yaml::from_str(&raw)?;
    Ok(cfg)
}

pub fn collector_config() -> anyhow::Result<Config> {
    let path = std::env::var("COLLECTOR_CONFIG").unwrap_or_else(|_| "config/collector.yaml".into());
    collector_config_from(&path)
}
