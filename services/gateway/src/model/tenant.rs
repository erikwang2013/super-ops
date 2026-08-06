use axum::{body::Body, extract::Request, middleware::Next, response::Response};

/// 解析租户头（x-tenant-id）：仅允许小写字母/数字/连字符，1..=64 字符。
/// 非法/缺失时由 require_tenant 回退为 "default"，本函数不产生失败路径。
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

/// 租户解析中间件：读取 x-tenant-id 头，解析失败/缺失时回退 "default"（无失败路径）。
/// 不依赖 AppState，故用 middleware::from_fn 挂载（无需 state）。
/// 实际执行位于 auth_middleware 之后、require_role 之前（见 main.rs 请求链路注释）。
pub async fn require_tenant(mut req: Request<Body>, next: Next) -> Response {
    let tenant = req
        .headers()
        .get("x-tenant-id")
        .and_then(|v| v.to_str().ok())
        .and_then(tenant_from_header)
        .unwrap_or_else(|| "default".to_string());
    req.extensions_mut().insert(Tenant(tenant));
    next.run(req).await
}
