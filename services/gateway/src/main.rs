mod alert_rules_api;
mod alerts_api;
mod approval_api;
mod audit_api;
mod auth;
mod backup_api;
mod breaker;
mod capacity_api;
mod cmdb_api;
mod config;
mod config_remote;
mod files_api;
mod health;
mod logs_api;
mod metrics;
mod metrics_api;
mod model;
mod oncall_api;
mod openapi;
mod proxy;
mod recorder;
#[cfg(test)]
mod recorder_test;
mod recordings_api;
mod registry;
mod release_api;
mod routes;
mod runbook_api;
mod scripts_api;
mod secrets_api;
mod ticket_api;
mod vault;
#[cfg(test)]
mod vault_test;

use crate::auth::middleware::AuthState;
use crate::config::Config;
use crate::model::user::UserStore;
use ecat::App;
use ecat_config_remote::ConsulConfigSource;
use ecat_data_clickhouse::ClickhouseClient;
use ecat_middleware::{MemoryStore, RateLimitStore, RedisRateLimitStore};
use ecat_mq_kafka::KafkaMq;
use ecat_registry::{Registration, Registry, ServiceInfo};
use ecat_registry_consul::ConsulRegistry;
use ecat_transport_http::HttpServer;
use sqlx::mysql::MySqlPool;
use std::sync::{Arc, Mutex, RwLock};

#[derive(Clone)]
pub struct AppState {
    pub user_store: UserStore,
    pub auth: Arc<AuthState>,
    pub ch: Arc<ClickhouseClient>,
    pub alert_acks: Arc<crate::alerts_api::AlertAckStore>,
    pub mq: Option<Arc<KafkaMq>>,
    pub k8s_endpoint: Arc<RwLock<String>>,
    pub api_keys: Arc<crate::model::api_key::ApiKeyStore>,
    pub pool: MySqlPool,
    pub approval_enabled: bool,
    pub master_key: Option<Vec<u8>>,
    pub recording_enabled: bool,
    pub terminal: crate::config::TerminalConfig,
    pub consul: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::load()?;
    let _otlp = match &config.otlp {
        Some(endpoint) => Some(
            ecat_tracing_otlp::init("superops-gateway", endpoint)
                .map_err(|e| anyhow::anyhow!("otlp init: {e}"))?,
        ),
        None => None,
    };
    let dynamic_cfg = Arc::new(crate::config_remote::DynamicConfig::default());
    let inner: Arc<dyn RateLimitStore> = match RedisRateLimitStore::connect(&config.redis.url).await
    {
        Ok(store) => Arc::new(store),
        Err(e) => {
            tracing::warn!(error = %e, "redis rate-limit store unavailable; falling back to in-memory");
            Arc::new(MemoryStore::new())
        }
    };
    let rate_store = Arc::new(crate::config_remote::DynamicRateLimitStore::new(
        inner,
        Arc::clone(&dynamic_cfg),
    ));
    let alert_acks = Arc::new(crate::alerts_api::AlertAckStore::connect(&config.redis.url).await);
    let pool = MySqlPool::connect(&config.database.url).await?;
    let ch = Arc::new(
        ClickhouseClient::from_config(config.ch.clone())
            .map_err(|e| anyhow::anyhow!("clickhouse config: {e}"))?,
    );
    let mq = match &config.mq {
        Some(cfg) => Some(Arc::new(
            KafkaMq::from_config(cfg.clone())
                .await
                .map_err(|e| anyhow::anyhow!("kafka config: {e}"))?,
        )),
        None => None,
    };
    let master_key = std::env::var("SUPEROPS_MASTER_KEY")
        .ok()
        .and_then(|k| (k.as_bytes().len() == 32).then(|| k.into_bytes()));
    if master_key.is_none() {
        tracing::warn!("SUPEROPS_MASTER_KEY 未设置或非 32 字节，/api/secrets 将返回 503");
    }
    let state = AppState {
        user_store: UserStore::new(pool.clone()),
        auth: Arc::new(AuthState::new(config.auth.clone())),
        ch,
        alert_acks,
        mq,
        k8s_endpoint: Arc::new(RwLock::new(config.services.k8s.endpoint.clone())),
        api_keys: Arc::new(crate::model::api_key::ApiKeyStore::new(pool.clone())),
        pool: pool.clone(),
        approval_enabled: config.approval.enabled,
        master_key,
        recording_enabled: config.recording.enabled,
        terminal: config.terminal.clone(),
        consul: config.consul.as_ref().map(|c| c.address.clone()),
    };
    let api_keys = Arc::clone(&state.api_keys);
    tokio::spawn(async move {
        if let Err(e) = api_keys.load().await {
            tracing::warn!(error = %e, "api_keys load failed; run deploy/init.sql");
        }
    });
    // 上传目录不可创建时仅告警：文件上传不可用但服务照常启动
    if let Err(e) = tokio::fs::create_dir_all("data/uploads").await {
        tracing::warn!(error = %e, "cannot create data/uploads; file upload disabled");
    }

    let state_start = state.clone();
    let app = crate::routes::app(state.clone(), &config, rate_store, pool.clone()).await?;

    let http = HttpServer::new(format!("0.0.0.0:{}", config.server.http_port)).router(app);

    let reg_holder = Arc::new(Mutex::new(None::<Registration>));
    let reg_start = Arc::clone(&reg_holder);
    let cfg_start = config.clone();
    let dynamic_start = Arc::clone(&dynamic_cfg);

    let mut app = App::builder()
        .name("superops-gateway")
        .version(env!("CARGO_PKG_VERSION"))
        .server(http)
        .on_start(move || {
            let reg = Arc::clone(&reg_start);
            let state = state_start.clone();
            let cfg = cfg_start.clone();
            let dynamic = Arc::clone(&dynamic_start);
            async move {
                if let Some(consul) = &cfg.consul {
                    let registry = ConsulRegistry::new(&consul.address);
                    let info = ServiceInfo::new("superops-gateway", env!("CARGO_PKG_VERSION"))
                        .with_endpoint(format!("http://localhost:{}", cfg.server.http_port));
                    let registration = registry.register(info).await?;
                    tracing::info!(service = "superops-gateway", "registered in consul");
                    *reg.lock().unwrap() = Some(registration);

                    match registry.discover("superops-k8s").await {
                        Ok(discovered) => {
                            let endpoint = crate::registry::resolve_k8s_endpoint(
                                &discovered,
                                &cfg.services.k8s.endpoint,
                            );
                            tracing::info!(
                                endpoint = %endpoint,
                                source = if discovered.is_empty() { "static" } else { "consul" },
                                "resolved k8s backend"
                            );
                            *state.k8s_endpoint.write().unwrap() = endpoint;
                        }
                        Err(e) => tracing::warn!(
                            error = %e,
                            "k8s discovery failed; keeping static endpoint"
                        ),
                    }

                    let source =
                        ConsulConfigSource::new(&consul.address, "config/superops/gateway");
                    tokio::spawn(crate::config_remote::run_config_watcher(
                        source.watch(),
                        dynamic,
                    ));
                }
                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
            }
        })
        .build()
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    app.run().await.map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(())
}
