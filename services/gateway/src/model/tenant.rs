use axum::{
    body::Body,
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};

/// 解析租户头（x-tenant-id）：仅允许小写字母/数字/连字符，1..=64 字符。
/// 返回 None 表示头非法（非法字符/超长/纯空白）；头是否缺失由调用方区分。
pub fn tenant_from_header(h: &str) -> Option<String> {
    let h = h.trim();
    if h.is_empty() || h.len() > 64 {
        return None;
    }
    h.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        .then(|| h.to_string())
}

/// 请求扩展中的租户标识（require_tenant 插入，handler 用 Extension<Tenant> 提取）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tenant(pub String);

/// 租户解析中间件：x-tenant-id 缺失时回退 "default"（不传租户头的调用方不受影响）；
/// 头存在但非法（非 UTF-8/非法字符/超长/纯空白）时返回 400，杜绝静默落入错误租户。
/// 不依赖 AppState，故用 middleware::from_fn 挂载（无需 state）。
/// 实际执行位于 auth_middleware 之后、require_role 之前（见 main.rs 请求链路注释）。
pub async fn require_tenant(mut req: Request<Body>, next: Next) -> Response {
    let tenant = match req.headers().get("x-tenant-id") {
        Some(v) => match v.to_str().ok().and_then(tenant_from_header) {
            Some(t) => t,
            None => {
                return (StatusCode::BAD_REQUEST, "invalid x-tenant-id header").into_response();
            }
        },
        None => "default".to_string(),
    };
    req.extensions_mut().insert(Tenant(tenant));
    next.run(req).await
}
