mod alerts_api;
mod approval_api;
mod audit_api;
mod auth;
mod breaker;
mod cmdb_api;
mod config;
mod config_remote;
mod files_api;
mod health;
mod logs_api;
mod metrics;
mod metrics_api;
mod model;
mod openapi;
mod proxy;
mod recorder;
#[cfg(test)]
mod recorder_test;
mod recordings_api;
mod registry;
mod scripts_api;
mod secrets_api;
mod vault;
#[cfg(test)]
mod vault_test;

use crate::auth::handler::{login, register};
use crate::auth::middleware::{AuthState, auth_middleware, require_role};
use crate::config::Config;
use crate::model::tenant::require_tenant;
use crate::model::user::UserStore;
use crate::proxy::k8s_proxy::{k8s_read_routes, k8s_write_routes};
use axum::{
    Router,
    http::{HeaderValue, Method},
    middleware,
    routing::{get, post},
};
use ecat::App;
use ecat_config_remote::ConsulConfigSource;
use ecat_data_clickhouse::ClickhouseClient;
use ecat_middleware::{MemoryStore, RateLimitLayer, RateLimitStore, RedisRateLimitStore};
use ecat_mq_kafka::KafkaMq;
use ecat_registry::{Registration, Registry, ServiceInfo};
use ecat_registry_consul::ConsulRegistry;
use ecat_transport_http::HttpServer;
use sqlx::mysql::MySqlPool;
use std::sync::{Arc, Mutex, RwLock};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

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
    };
    let api_keys = Arc::clone(&state.api_keys);
    tokio::spawn(async move {
        if let Err(e) = api_keys.load().await {
            tracing::warn!(error = %e, "api_keys load failed; run deploy/init.sql");
        }
    });
    tokio::fs::create_dir_all("data/uploads").await?;

    // 请求链路（axum 后注册的层先执行，故 .layer 自下而上为 breaker → require_role → [require_tenant] → auth_middleware）：
    // 实际执行：auth_middleware → require_tenant(仅 cmdb/scripts) → require_role → breaker → handler
    let k8s = k8s_read_routes()
        .layer(
            tower::ServiceBuilder::new()
                .layer(crate::breaker::ErrorToResponseLayer)
                .layer(crate::breaker::breaker_layer())
                .layer(crate::breaker::FiveXxToErrorLayer),
        )
        .layer(middleware::from_fn(move |req, next| {
            require_role("api:read", req, next)
        }))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));
    let k8s_write = k8s_write_routes()
        .layer(
            tower::ServiceBuilder::new()
                .layer(crate::breaker::ErrorToResponseLayer)
                .layer(crate::breaker::breaker_layer())
                .layer(crate::breaker::FiveXxToErrorLayer),
        )
        .layer(middleware::from_fn(move |req, next| {
            require_role("api:write", req, next)
        }))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));
    let k8s = k8s.merge(k8s_write);
    let k8s = match &config.oauth2 {
        Some(oauth2) => k8s.layer(
            tower::ServiceBuilder::new()
                .layer(crate::breaker::ErrorToResponseLayer)
                .layer(
                    ecat_auth::OAuth2Layer::new(
                        &oauth2.introspection_url,
                        &oauth2.client_id,
                        &oauth2.client_secret,
                    )
                    .map_err(|e| anyhow::anyhow!("oauth2 config: {e}"))?,
                ),
        ),
        None => k8s,
    };
    let keys = axum::Router::new()
        .route(
            "/api/keys",
            axum::routing::get(crate::auth::apikey::list_keys)
                .post(crate::auth::apikey::create_key),
        )
        .route(
            "/api/keys/{id}",
            axum::routing::delete(crate::auth::apikey::delete_key),
        )
        .layer(
            tower::ServiceBuilder::new()
                .layer(crate::breaker::ErrorToResponseLayer)
                .layer(crate::breaker::breaker_layer())
                .layer(crate::breaker::FiveXxToErrorLayer),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));
    let audit = axum::Router::new()
        .route(
            "/api/audit/events",
            axum::routing::get(crate::audit_api::audit_events),
        )
        .layer(
            tower::ServiceBuilder::new()
                .layer(crate::breaker::ErrorToResponseLayer)
                .layer(crate::breaker::breaker_layer())
                .layer(crate::breaker::FiveXxToErrorLayer),
        )
        .layer(middleware::from_fn(move |req, next| {
            require_role("ops:audit", req, next)
        }))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));
    let approvals = axum::Router::new()
        .route(
            "/api/approvals",
            axum::routing::get(crate::approval_api::list_approvals_handler)
                .post(crate::approval_api::create_approval_handler),
        )
        .route(
            "/api/approvals/{id}/decide",
            axum::routing::post(crate::approval_api::decide_approval_handler),
        )
        .layer(
            tower::ServiceBuilder::new()
                .layer(crate::breaker::ErrorToResponseLayer)
                .layer(crate::breaker::breaker_layer())
                .layer(crate::breaker::FiveXxToErrorLayer),
        )
        .layer(middleware::from_fn(move |req, next| {
            require_role("ops:audit", req, next)
        }))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));
    let users = axum::Router::new()
        .route(
            "/api/users",
            axum::routing::get(crate::auth::handler::list_users),
        )
        .route(
            "/api/users/{id}/status",
            axum::routing::patch(crate::auth::handler::set_user_status),
        )
        .layer(
            tower::ServiceBuilder::new()
                .layer(crate::breaker::ErrorToResponseLayer)
                .layer(crate::breaker::breaker_layer())
                .layer(crate::breaker::FiveXxToErrorLayer),
        )
        .layer(middleware::from_fn(move |req, next| {
            require_role("ops:users", req, next)
        }))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));
    let cmdb = axum::Router::new()
        .route(
            "/api/cmdb/assets",
            axum::routing::get(crate::cmdb_api::list_cmdb_assets)
                .post(crate::cmdb_api::create_cmdb_asset),
        )
        .route(
            "/api/cmdb/assets/{id}",
            axum::routing::delete(crate::cmdb_api::delete_cmdb_asset),
        )
        .route(
            "/api/cmdb/stats",
            axum::routing::get(crate::cmdb_api::cmdb_stats),
        )
        .layer(
            tower::ServiceBuilder::new()
                .layer(crate::breaker::ErrorToResponseLayer)
                .layer(crate::breaker::breaker_layer())
                .layer(crate::breaker::FiveXxToErrorLayer),
        )
        .layer(middleware::from_fn(move |req, next| {
            require_role("ops:cmdb", req, next)
        }))
        .layer(middleware::from_fn(require_tenant))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));
    let scripts = axum::Router::new()
        .route(
            "/api/scripts",
            axum::routing::get(crate::scripts_api::list_scripts_handler)
                .post(crate::scripts_api::create_script_handler),
        )
        .route(
            "/api/scripts/runs",
            axum::routing::get(crate::scripts_api::list_runs_handler),
        )
        .route(
            "/api/scripts/{id}",
            axum::routing::delete(crate::scripts_api::delete_script_handler),
        )
        .route(
            "/api/scripts/{id}/run",
            axum::routing::post(crate::scripts_api::run_script_handler),
        )
        .layer(
            tower::ServiceBuilder::new()
                .layer(crate::breaker::ErrorToResponseLayer)
                .layer(crate::breaker::breaker_layer())
                .layer(crate::breaker::FiveXxToErrorLayer),
        )
        .layer(middleware::from_fn(move |req, next| {
            require_role("ops:scripts", req, next)
        }))
        .layer(middleware::from_fn(require_tenant))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));
    let secrets = axum::Router::new()
        .route(
            "/api/secrets",
            axum::routing::get(crate::secrets_api::list_secrets)
                .post(crate::secrets_api::create_secret),
        )
        .route(
            "/api/secrets/{name}",
            axum::routing::get(crate::secrets_api::get_secret)
                .delete(crate::secrets_api::delete_secret),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));
    let recordings_read = axum::Router::new()
        .route(
            "/api/recordings",
            axum::routing::get(crate::recordings_api::list_recordings),
        )
        .route(
            "/api/recordings/{sid}/frames",
            axum::routing::get(crate::recordings_api::get_frames),
        )
        .layer(
            tower::ServiceBuilder::new()
                .layer(crate::breaker::ErrorToResponseLayer)
                .layer(crate::breaker::breaker_layer())
                .layer(crate::breaker::FiveXxToErrorLayer),
        )
        .layer(middleware::from_fn(move |req, next| {
            require_role("api:read", req, next)
        }))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));
    let recordings_write = axum::Router::new()
        .route(
            "/api/recordings/{sid}",
            axum::routing::delete(crate::recordings_api::delete_recording),
        )
        .layer(
            tower::ServiceBuilder::new()
                .layer(crate::breaker::ErrorToResponseLayer)
                .layer(crate::breaker::breaker_layer())
                .layer(crate::breaker::FiveXxToErrorLayer),
        )
        .layer(middleware::from_fn(move |req, next| {
            require_role("api:write", req, next)
        }))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));
    let files_read = axum::Router::new()
        .route(
            "/api/files/{name}",
            axum::routing::get(crate::files_api::get_file),
        )
        .layer(
            tower::ServiceBuilder::new()
                .layer(crate::breaker::ErrorToResponseLayer)
                .layer(crate::breaker::breaker_layer())
                .layer(crate::breaker::FiveXxToErrorLayer),
        )
        .layer(middleware::from_fn(move |req, next| {
            require_role("api:read", req, next)
        }))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));
    let files_write = axum::Router::new()
        .route(
            "/api/files",
            axum::routing::post(crate::files_api::upload_file),
        )
        .layer(axum::extract::DefaultBodyLimit::max(10 * 1024 * 1024))
        .layer(
            tower::ServiceBuilder::new()
                .layer(crate::breaker::ErrorToResponseLayer)
                .layer(crate::breaker::breaker_layer())
                .layer(crate::breaker::FiveXxToErrorLayer),
        )
        .layer(middleware::from_fn(move |req, next| {
            require_role("api:write", req, next)
        }))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));
    let logs = axum::Router::new()
        .route(
            "/api/logs/search",
            axum::routing::get(crate::logs_api::search_logs),
        )
        .layer(
            tower::ServiceBuilder::new()
                .layer(crate::breaker::ErrorToResponseLayer)
                .layer(crate::breaker::breaker_layer())
                .layer(crate::breaker::FiveXxToErrorLayer),
        )
        .layer(middleware::from_fn(move |req, next| {
            require_role("api:read", req, next)
        }))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));
    let alerts_read = axum::Router::new()
        .route(
            "/api/alerts",
            axum::routing::get(crate::alerts_api::list_alerts),
        )
        .route(
            "/api/alerts/acks",
            axum::routing::get(crate::alerts_api::list_acks),
        )
        .layer(
            tower::ServiceBuilder::new()
                .layer(crate::breaker::ErrorToResponseLayer)
                .layer(crate::breaker::breaker_layer())
                .layer(crate::breaker::FiveXxToErrorLayer),
        )
        .layer(middleware::from_fn(move |req, next| {
            require_role("ops:cmdb", req, next)
        }))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));
    let alerts_write = axum::Router::new()
        .route(
            "/api/alerts/{id}/ack",
            axum::routing::post(crate::alerts_api::ack_alert),
        )
        .layer(
            tower::ServiceBuilder::new()
                .layer(crate::breaker::ErrorToResponseLayer)
                .layer(crate::breaker::breaker_layer())
                .layer(crate::breaker::FiveXxToErrorLayer),
        )
        .layer(middleware::from_fn(move |req, next| {
            require_role("api:write", req, next)
        }))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));
    let login_limited = axum::Router::new()
        .route("/api/auth/login", post(login))
        .layer(
            tower::ServiceBuilder::new()
                .layer(crate::breaker::ErrorToResponseLayer)
                .layer(
                    RateLimitLayer::new(10, std::time::Duration::from_secs(60))
                        .with_store(rate_store)
                        .with_key_fn(|req: &axum::http::Request<axum::body::Body>| {
                            req.headers()
                                .get("x-forwarded-for")
                                .and_then(|v| v.to_str().ok())
                                .and_then(|v| v.split(',').next().map(str::trim))
                                .filter(|s| !s.is_empty())
                                .or_else(|| {
                                    req.headers().get("x-real-ip").and_then(|v| v.to_str().ok())
                                })
                                .unwrap_or("global")
                                .to_string()
                        }),
                ),
        );
    let state_start = state.clone();

    let app =
        Router::new()
            .route("/api/health", get(health))
            .route("/api/docs", get(docs))
            .route("/api/auth/register", post(register))
            .merge(login_limited)
            .merge(k8s)
            .merge(keys)
            .merge(audit)
            .merge(approvals)
            .merge(cmdb)
            .merge(scripts)
            .merge(logs)
            .merge(alerts_read)
            .merge(alerts_write)
            .merge(users)
            .merge(secrets)
            .merge(recordings_read)
            .merge(recordings_write)
            .merge(files_read)
            .merge(files_write)
            .layer(middleware::from_fn(metrics::count_requests))
            // INFO 级 span：确保经 EnvFilter 后仍进入 tracing→OTLP 导出链路（Jaeger 可见）
            .layer(TraceLayer::new_for_http().make_span_with(
                tower_http::trace::DefaultMakeSpan::new().level(tracing::Level::INFO),
            ))
            .layer(
                CorsLayer::new()
                    .allow_origin([
                        HeaderValue::from_static("http://localhost:3000"),
                        HeaderValue::from_static("http://tauri.localhost"),
                        HeaderValue::from_static("tauri://localhost"),
                    ])
                    .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE])
                    .allow_headers([
                        axum::http::header::AUTHORIZATION,
                        axum::http::header::CONTENT_TYPE,
                        axum::http::header::HeaderName::from_static("x-tenant-id"),
                    ]),
            )
            .with_state(state)
            .merge(health::health_router(pool.clone()).await);

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

async fn health() -> &'static str {
    r#"{"status":"ok"}"#
}

async fn docs() -> axum::Json<ecat_openapi::OpenApiSpec> {
    axum::Json(crate::openapi::build_spec())
}
