mod auth;
mod config;
mod model;
mod proxy;

use std::sync::Arc;
use axum::{routing::get, Router};
use axum::routing::post;
use ecat::App;
use ecat_transport_http::HttpServer;
use sqlx::mysql::MySqlPool;
use tower_http::cors::{Any, CorsLayer};
use crate::auth::handler::{login, register};
use crate::auth::middleware::AuthState;
use crate::config::Config;
use crate::model::user::UserStore;
use crate::proxy::k8s_proxy::k8s_routes;

#[derive(Clone)]
pub struct AppState {
    pub user_store: UserStore,
    pub auth: Arc<AuthState>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::load()?;
    let pool = MySqlPool::connect(&config.database.url).await?;
    let state = AppState {
        user_store: UserStore::new(pool),
        auth: Arc::new(AuthState::new(config.auth.clone())),
    };

    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/auth/login", post(login))
        .route("/api/auth/register", post(register))
        .merge(k8s_routes())
        .layer(CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any))
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

async fn health() -> &'static str { r#"{"status":"ok"}"# }
