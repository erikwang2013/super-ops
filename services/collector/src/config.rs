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
pub struct NotifyConfig {
    #[serde(default = "default_silence")]
    pub silence_secs: u64,
    #[serde(default)]
    pub targets: Vec<crate::notify::NotifyTarget>,
}

impl Default for NotifyConfig {
    fn default() -> Self {
        Self {
            silence_secs: 300,
            targets: Vec::new(),
        }
    }
}

fn default_silence() -> u64 {
    300
}

// B4 邮件通知通道：notify targets kind=email 时经此 SMTP 发送
#[derive(Debug, Clone, Deserialize)]
pub struct SmtpConfig {
    pub host: String,
    #[serde(default = "default_smtp_port")]
    pub port: u16,
    pub username: String,
    pub password: String,
    pub from: String,
}

fn default_smtp_port() -> u16 {
    587
}

#[derive(Debug, Clone, Deserialize)]
pub struct LogtailConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub namespaces: Vec<String>,
    #[serde(default = "default_tail")]
    pub tail_lines: i32,
    #[serde(default = "default_max_line")]
    pub max_line_bytes: i32,
    #[serde(default = "default_logtail_interval")]
    pub interval_secs: u64,
}

impl Default for LogtailConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            namespaces: Vec::new(),
            tail_lines: 200,
            max_line_bytes: 1024,
            interval_secs: 30,
        }
    }
}

fn default_tail() -> i32 {
    200
}

fn default_max_line() -> i32 {
    1024
}

fn default_logtail_interval() -> u64 {
    30
}

#[derive(Debug, Clone, Deserialize)]
pub struct SelfhealConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_selfheal_cluster")]
    pub cluster_id: String,
    #[serde(default = "default_selfheal_max")]
    pub max_actions_per_cycle: usize,
}

impl Default for SelfhealConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            cluster_id: default_selfheal_cluster(),
            max_actions_per_cycle: default_selfheal_max(),
        }
    }
}

fn default_selfheal_cluster() -> String {
    "default".into()
}

fn default_selfheal_max() -> usize {
    5
}

#[derive(Debug, Clone, Deserialize)]
pub struct HousekeepingConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_housekeeping_interval")]
    pub interval_secs: u64,
}

// 配置漂移检测：CMDB deployment 资产 vs 集群实际 deployment（需要 mysql 配置）
#[derive(Debug, Clone, Deserialize)]
pub struct DriftConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_drift_interval")]
    pub interval_secs: u64,
}

impl Default for DriftConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_secs: default_drift_interval(),
        }
    }
}

fn default_drift_interval() -> u64 {
    3600
}

impl Default for HousekeepingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_secs: default_housekeeping_interval(),
        }
    }
}

// 发布自动回滚：status=ok 的发布在 delay 秒后进入观察窗口，deployment ready==0 或缺失时自动回滚
#[derive(Debug, Clone, Deserialize)]
pub struct RollbackConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_rollback_interval")]
    pub interval_secs: u64,
    #[serde(default = "default_rollback_delay")]
    pub delay_secs: u64,
    #[serde(default = "default_rollback_window")]
    pub window_secs: u64,
}

impl Default for RollbackConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_secs: default_rollback_interval(),
            delay_secs: default_rollback_delay(),
            window_secs: default_rollback_window(),
        }
    }
}

fn default_rollback_interval() -> u64 {
    120
}

fn default_rollback_delay() -> u64 {
    120
}

fn default_rollback_window() -> u64 {
    3600
}

fn default_housekeeping_interval() -> u64 {
    86_400
}

#[derive(Debug, Clone, Deserialize)]
pub struct MysqlConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub database: String,
    pub backup_dir: String,
}

impl Default for MysqlConfig {
    fn default() -> Self {
        Self {
            host: "localhost".into(),
            port: 3307,
            user: "root".into(),
            password: "root123".into(),
            database: "superops".into(),
            backup_dir: "data/backups".into(),
        }
    }
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
    #[serde(default)]
    pub otlp: Option<String>,
    #[serde(default)]
    pub notify: NotifyConfig,
    #[serde(default)]
    pub logtail: LogtailConfig,
    #[serde(default)]
    pub housekeeping: HousekeepingConfig,
    #[serde(default)]
    pub drift: DriftConfig,
    #[serde(default)]
    pub selfheal: SelfhealConfig,
    #[serde(default)]
    pub rollback: RollbackConfig,
    #[serde(default)]
    pub mysql: Option<MysqlConfig>,
    #[serde(default)]
    pub smtp: Option<SmtpConfig>,
}

pub fn collector_config_from(path: &str) -> anyhow::Result<Config> {
    let raw = std::fs::read_to_string(path)?;
    let mut cfg: Config = serde_yaml::from_str(&raw)?;
    if let Some(mysql) = &mut cfg.mysql {
        if let Ok(secret) = std::env::var("SUPEROPS_MYSQL_PASSWORD") {
            if !secret.is_empty() {
                mysql.password = secret;
            }
        }
    }
    if let Some(smtp) = &mut cfg.smtp {
        if let Ok(secret) = std::env::var("SUPEROPS_SMTP_PASSWORD") {
            if !secret.is_empty() {
                smtp.password = secret;
            }
        }
    }
    Ok(cfg)
}

pub fn collector_config() -> anyhow::Result<Config> {
    let path = std::env::var("COLLECTOR_CONFIG").unwrap_or_else(|_| "config/collector.yaml".into());
    collector_config_from(&path)
}
