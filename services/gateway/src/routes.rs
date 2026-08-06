use crate::AppState;
use crate::auth::handler::{login, register};
use crate::auth::middleware::{auth_middleware, require_role};
use crate::config::Config;
use crate::model::tenant::require_tenant;
use crate::proxy::k8s_proxy::{k8s_read_routes, k8s_write_routes};
use axum::{
    Router,
    http::{HeaderValue, Method},
    middleware,
    routing::{get, post},
};
use ecat_middleware::RateLimitLayer;
use sqlx::mysql::MySqlPool;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

// 断路器三层栈，经 ServiceBuilder 合并为单个 .layer()（Router::layer 逐层要求
// Error: Into<Infallible>，只有 ErrorToResponse 满足，须整体组合后一次挂载；
// tower 0.5 ServiceBuilder 先加的层在最外层，故 ErrorToResponse 先加）：
// 请求先经 ErrorToResponse → CircuitBreaker 观测错误熔断 → FiveXxToError 把
// handler 5xx 转为错误；断路器打开时 ErrorToResponse 将其转为 503 响应。
fn breaker(router: axum::Router<crate::AppState>) -> axum::Router<crate::AppState> {
    router.layer(
        tower::ServiceBuilder::new()
            .layer(crate::breaker::ErrorToResponseLayer)
            .layer(crate::breaker::breaker_layer())
            .layer(crate::breaker::FiveXxToErrorLayer),
    )
}

/// 组装全部 HTTP 路由。请求链路（axum 后注册的层先执行）：
/// auth_middleware → require_tenant(仅 cmdb/scripts) → require_role → breaker → handler
pub async fn app(
    state: AppState,
    config: &Config,
    rate_store: Arc<crate::config_remote::DynamicRateLimitStore>,
    pool: MySqlPool,
) -> anyhow::Result<Router> {
    let k8s = breaker(k8s_read_routes())
        .layer(middleware::from_fn(move |req, next| {
            require_role("api:read", req, next)
        }))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));
    let k8s_write = breaker(k8s_write_routes())
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
    let keys = breaker(
        axum::Router::new()
            .route(
                "/api/keys",
                axum::routing::get(crate::auth::apikey::list_keys)
                    .post(crate::auth::apikey::create_key),
            )
            .route(
                "/api/keys/{id}",
                axum::routing::delete(crate::auth::apikey::delete_key),
            ),
    )
    .layer(middleware::from_fn_with_state(
        state.clone(),
        auth_middleware,
    ));
    let audit = breaker(axum::Router::new().route(
        "/api/audit/events",
        axum::routing::get(crate::audit_api::audit_events),
    ))
    .layer(middleware::from_fn(move |req, next| {
        require_role("ops:audit", req, next)
    }))
    .layer(middleware::from_fn_with_state(
        state.clone(),
        auth_middleware,
    ));
    let approvals = breaker(
        axum::Router::new()
            .route(
                "/api/approvals",
                axum::routing::get(crate::approval_api::list_approvals_handler)
                    .post(crate::approval_api::create_approval_handler),
            )
            .route(
                "/api/approvals/{id}/decide",
                axum::routing::post(crate::approval_api::decide_approval_handler),
            ),
    )
    .layer(middleware::from_fn(move |req, next| {
        require_role("ops:audit", req, next)
    }))
    .layer(middleware::from_fn_with_state(
        state.clone(),
        auth_middleware,
    ));
    let users = breaker(
        axum::Router::new()
            .route(
                "/api/users",
                axum::routing::get(crate::auth::handler::list_users),
            )
            .route(
                "/api/users/{id}/status",
                axum::routing::patch(crate::auth::handler::set_user_status),
            ),
    )
    .layer(middleware::from_fn(move |req, next| {
        require_role("ops:users", req, next)
    }))
    .layer(middleware::from_fn_with_state(
        state.clone(),
        auth_middleware,
    ));
    let cmdb = breaker(
        axum::Router::new()
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
            ),
    )
    .layer(middleware::from_fn(move |req, next| {
        require_role("ops:cmdb", req, next)
    }))
    .layer(middleware::from_fn(require_tenant))
    .layer(middleware::from_fn_with_state(
        state.clone(),
        auth_middleware,
    ));
    let scripts = breaker(
        axum::Router::new()
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
            ),
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
    let recordings_read = breaker(
        axum::Router::new()
            .route(
                "/api/recordings",
                axum::routing::get(crate::recordings_api::list_recordings),
            )
            .route(
                "/api/recordings/{sid}/frames",
                axum::routing::get(crate::recordings_api::get_frames),
            ),
    )
    .layer(middleware::from_fn(move |req, next| {
        require_role("api:read", req, next)
    }))
    .layer(middleware::from_fn_with_state(
        state.clone(),
        auth_middleware,
    ));
    let recordings_write = breaker(axum::Router::new().route(
        "/api/recordings/{sid}",
        axum::routing::delete(crate::recordings_api::delete_recording),
    ))
    .layer(middleware::from_fn(move |req, next| {
        require_role("api:write", req, next)
    }))
    .layer(middleware::from_fn_with_state(
        state.clone(),
        auth_middleware,
    ));
    let files_read = breaker(axum::Router::new().route(
        "/api/files/{name}",
        axum::routing::get(crate::files_api::get_file),
    ))
    .layer(middleware::from_fn(move |req, next| {
        require_role("api:read", req, next)
    }))
    .layer(middleware::from_fn_with_state(
        state.clone(),
        auth_middleware,
    ));
    // 超限请求由 axum 默认拒绝：BytesRejection 状态码即 413 PAYLOAD_TOO_LARGE
    let files_write = breaker(
        axum::Router::new()
            .route(
                "/api/files",
                axum::routing::post(crate::files_api::upload_file),
            )
            .layer(axum::extract::DefaultBodyLimit::max(10 * 1024 * 1024)),
    )
    .layer(middleware::from_fn(move |req, next| {
        require_role("api:write", req, next)
    }))
    .layer(middleware::from_fn_with_state(
        state.clone(),
        auth_middleware,
    ));
    let logs = breaker(axum::Router::new().route(
        "/api/logs/search",
        axum::routing::get(crate::logs_api::search_logs),
    ))
    .layer(middleware::from_fn(move |req, next| {
        require_role("api:read", req, next)
    }))
    .layer(middleware::from_fn_with_state(
        state.clone(),
        auth_middleware,
    ));
    let alerts_read = breaker(
        axum::Router::new()
            .route(
                "/api/alerts",
                axum::routing::get(crate::alerts_api::list_alerts),
            )
            .route(
                "/api/alerts/acks",
                axum::routing::get(crate::alerts_api::list_acks),
            ),
    )
    .layer(middleware::from_fn(move |req, next| {
        require_role("ops:cmdb", req, next)
    }))
    .layer(middleware::from_fn_with_state(
        state.clone(),
        auth_middleware,
    ));
    let alerts_write = breaker(axum::Router::new().route(
        "/api/alerts/{id}/ack",
        axum::routing::post(crate::alerts_api::ack_alert),
    ))
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

    Ok(Router::new()
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
        .layer(middleware::from_fn(crate::metrics::count_requests))
        // INFO 级 span：确保经 EnvFilter 后仍进入 tracing→OTLP 导出链路（Jaeger 可见）
        .layer(
            TraceLayer::new_for_http().make_span_with(
                tower_http::trace::DefaultMakeSpan::new().level(tracing::Level::INFO),
            ),
        )
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
        .merge(crate::health::health_router(pool).await))
}

async fn health() -> &'static str {
    r#"{"status":"ok"}"#
}

async fn docs() -> axum::Json<ecat_openapi::OpenApiSpec> {
    axum::Json(crate::openapi::build_spec())
}
