mod auth;
mod breaker;
mod config;
mod config_remote;
mod health;
mod metrics;
mod metrics_api;
mod model;
mod openapi;
mod proxy;
mod registry;

use crate::auth::handler::{login, register};
use crate::auth::middleware::{AuthState, auth_middleware};
use crate::config::Config;
use crate::model::user::UserStore;
use crate::proxy::k8s_proxy::k8s_routes;
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

#[derive(Clone)]
pub struct AppState {
    pub user_store: UserStore,
    pub auth: Arc<AuthState>,
    pub ch: Arc<ClickhouseClient>,
    pub mq: Option<Arc<KafkaMq>>,
    pub k8s_endpoint: Arc<RwLock<String>>,
    pub api_keys: Arc<crate::model::api_key::ApiKeyStore>,
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
    let state = AppState {
        user_store: UserStore::new(pool.clone()),
        auth: Arc::new(AuthState::new(config.auth.clone())),
        ch,
        mq,
        k8s_endpoint: Arc::new(RwLock::new(config.services.k8s.endpoint.clone())),
        api_keys: Arc::new(crate::model::api_key::ApiKeyStore::new(pool.clone())),
    };
    let api_keys = Arc::clone(&state.api_keys);
    tokio::spawn(async move {
        if let Err(e) = api_keys.load().await {
            tracing::warn!(error = %e, "api_keys load failed; run deploy/init.sql");
        }
    });

    let k8s = k8s_routes()
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
        .layer(
            tower::ServiceBuilder::new()
                .layer(crate::breaker::ErrorToResponseLayer)
                .layer(crate::breaker::breaker_layer())
                .layer(crate::breaker::FiveXxToErrorLayer),
        );
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
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
        .layer(
            tower::ServiceBuilder::new()
                .layer(crate::breaker::ErrorToResponseLayer)
                .layer(crate::breaker::breaker_layer())
                .layer(crate::breaker::FiveXxToErrorLayer),
        );
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

    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/docs", get(docs))
        .route("/api/auth/register", post(register))
        .merge(login_limited)
        .merge(k8s)
        .merge(keys)
        .layer(middleware::from_fn(metrics::count_requests))
        .layer(
            CorsLayer::new()
                .allow_origin([
                    HeaderValue::from_static("http://localhost:3000"),
                    HeaderValue::from_static("http://tauri.localhost"),
                    HeaderValue::from_static("tauri://localhost"),
                ])
                .allow_methods([Method::GET, Method::POST, Method::DELETE])
                .allow_headers([
                    axum::http::header::AUTHORIZATION,
                    axum::http::header::CONTENT_TYPE,
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
