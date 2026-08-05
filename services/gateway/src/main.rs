mod auth;
mod config;
mod model;
mod proxy;

use crate::auth::handler::{login, register};
use crate::auth::middleware::{AuthState, auth_middleware};
use crate::auth::rate_limit::{RateLimiter, rate_limit_login};
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
use ecat_transport_http::HttpServer;
use sqlx::mysql::MySqlPool;
use std::sync::Arc;
use tower_http::cors::CorsLayer;

#[derive(Clone)]
pub struct AppState {
    pub user_store: UserStore,
    pub auth: Arc<AuthState>,
    pub rate_limiter: RateLimiter,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::load()?;
    let pool = MySqlPool::connect(&config.database.url).await?;
    let state = AppState {
        user_store: UserStore::new(pool),
        auth: Arc::new(AuthState::new(config.auth.clone())),
        rate_limiter: RateLimiter::default(),
    };

    let k8s = k8s_routes().layer(middleware::from_fn_with_state(
        state.clone(),
        auth_middleware,
    ));
    let login_limited = post(login).layer(middleware::from_fn_with_state(
        state.clone(),
        rate_limit_login,
    ));

    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/auth/login", login_limited)
        .route("/api/auth/register", post(register))
        .merge(k8s)
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
        .with_state(state);

    let http = HttpServer::new(format!("0.0.0.0:{}", config.server.http_port)).router(app);

    let mut app = App::builder()
        .name("superops-gateway")
        .version(env!("CARGO_PKG_VERSION"))
        .server(http)
        .build()
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    app.run().await.map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(())
}

async fn health() -> &'static str {
    r#"{"status":"ok"}"#
}
