use super::tenant::{Tenant, require_tenant, tenant_from_header};

use crate::auth::middleware::require_role;
use axum::{
    Json,
    body::Body,
    extract::Extension,
    http::{Request, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::get,
};
use ecat_auth::AuthClaims;
use tower::util::ServiceExt;

#[test]
fn tenant_parsing() {
    assert_eq!(tenant_from_header("acme"), Some("acme".to_string()));
    assert_eq!(tenant_from_header("Acme!"), None); // 非法字符
    assert_eq!(tenant_from_header(&"a".repeat(65)), None); // 超长
    assert_eq!(tenant_from_header(""), None);
}

#[test]
fn tenant_parsing_edges() {
    // 数字/连字符合法；空白容忍
    assert_eq!(
        tenant_from_header("acme-prod-1"),
        Some("acme-prod-1".to_string())
    );
    assert_eq!(tenant_from_header("  acme  "), Some("acme".to_string()));
    // 纯空白 / 大写 / 下划线均非法
    assert_eq!(tenant_from_header("   "), None);
    assert_eq!(tenant_from_header("ACME"), None);
    assert_eq!(tenant_from_header("acme_prod"), None);
    // 恰好 64 字符合法
    assert_eq!(
        tenant_from_header(&"a".repeat(64)).map(|s| s.len()),
        Some(64)
    );
}

/// 复刻 main.rs cmdb 路由组的 .layer 注册顺序（breaker → require_role → require_tenant → auth_middleware）。
/// axum 中后注册的层先执行，实际链路 = auth → tenant → role → breaker → handler。
/// 若有人把 auth_middleware 注册回最前（P5-6 回归：breaker → role → auth），
/// require_role 先于 auth 执行、拿不到 AuthClaims 会 403，本测试即失败。
/// （AppState 需 MySQL 连接无法在单测构造，故 auth/breaker 用等价的占位中间件）
#[tokio::test]
async fn main_chain_order_auth_then_tenant_then_role() {
    // 认证占位：等价 auth_middleware（注入 AuthClaims）
    async fn fake_auth(mut req: Request<Body>, next: Next) -> Response {
        req.extensions_mut().insert(AuthClaims {
            sub: "u1".into(),
            exp: None,
            iat: None,
            role: Some("admin".into()),
            extra: Default::default(),
        });
        next.run(req).await
    }

    // breaker 占位：透传（等价 breaker ServiceBuilder 栈）
    async fn fake_breaker(req: Request<Body>, next: Next) -> Response {
        next.run(req).await
    }

    async fn whoami(Extension(t): Extension<Tenant>) -> Json<serde_json::Value> {
        Json(serde_json::json!({ "tenant": t.0 }))
    }

    let app = axum::Router::new()
        .route("/whoami", get(whoami))
        .layer(middleware::from_fn(fake_breaker)) // 先注册 → 最内层 → 最后执行
        .layer(middleware::from_fn(move |req, next| {
            require_role("ops:cmdb", req, next)
        }))
        .layer(middleware::from_fn(require_tenant))
        .layer(middleware::from_fn(fake_auth)); // 最后注册 → 最外层 → 最先执行

    // 有效租户头：require_role 拿到 claims（非 403）且 Tenant 到达 handler
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/whoami")
                .header("x-tenant-id", "acme")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 1024).await.unwrap();
    assert_eq!(&body[..], br#"{"tenant":"acme"}"#);

    // 非法租户头：拒绝 400（审计项：malformed 头静默回退会落入错误租户）
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/whoami")
                .header("x-tenant-id", "Acme!")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    // 缺失租户头：回退 "default" 且不 403
    let res = app
        .oneshot(
            Request::builder()
                .uri("/whoami")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 1024).await.unwrap();
    assert_eq!(&body[..], br#"{"tenant":"default"}"#);
}
