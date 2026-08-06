use axum::{
    Json,
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use ecat_auth::{AuthClaims, extract_bearer, extract_query_param};
use serde_json::json;

use crate::AppState;
use crate::config::AuthConfig;

#[derive(Clone)]
pub struct AuthState {
    pub config: AuthConfig,
}

impl AuthState {
    pub fn new(config: AuthConfig) -> Self {
        Self { config }
    }

    /// 签发 JWT：委托框架 ecat-auth 的 HS256 签发（sub + username 附加 claim + role）。
    pub fn create_token(
        &self,
        user_id: &str,
        username: &str,
        role: Option<&str>,
        ttl: u64,
    ) -> Result<String, ecat_auth::JwtAuthError> {
        let claims = AuthClaims {
            sub: user_id.to_string(),
            exp: None,
            iat: None,
            role: role.map(String::from),
            extra: [(
                "username".to_string(),
                serde_json::Value::String(username.to_string()),
            )]
            .into_iter()
            .collect(),
        };
        ecat_auth::sign_token(&self.config.jwt_secret, &claims, ttl)
    }

    /// 校验 JWT：委托框架 ecat-auth（含过期/弱密钥区分）。
    pub fn verify_token(&self, token: &str) -> Result<AuthClaims, ecat_auth::JwtAuthError> {
        ecat_auth::verify_token(&self.config.jwt_secret, token)
    }
}

pub async fn auth_middleware(
    State(state): State<AppState>,
    mut req: Request<Body>,
    next: Next,
) -> Result<Response, Response> {
    // OAuth2 层已把 AuthClaims 放入 extensions（ecat_auth::OAuth2Layer）→ 直接放行
    if req.extensions().get::<AuthClaims>().is_some() {
        return Ok(next.run(req).await);
    }
    // X-API-Key 鉴权（内存表 hash 查询；吊销立即失效）
    if let Some(key) = req
        .headers()
        .get("X-API-Key")
        .and_then(|v| v.to_str().ok())
        .filter(|k| !k.is_empty())
    {
        let hash = crate::model::api_key::hash_key(key);
        if let Some(user_id) = state.api_keys.lookup(&hash) {
            // API-key 无内嵌 role，从 DB 解析用户角色（解析失败时按无权限处理）
            let role = match state.user_store.find_role(&user_id).await {
                Ok(role) => role,
                Err(e) => {
                    tracing::warn!(error = %e, "api-key role lookup failed; denying request");
                    None
                }
            };
            let claims = AuthClaims {
                sub: user_id,
                exp: None,
                iat: None,
                role,
                extra: [(
                    "username".to_string(),
                    serde_json::Value::String("api-key".into()),
                )]
                .into_iter()
                .collect(),
            };
            req.extensions_mut().insert(claims);
            return Ok(next.run(req).await);
        }
    }
    let token = extract_bearer(req.headers(), axum::http::header::AUTHORIZATION.as_str())
        .filter(|t| !t.is_empty())
        .or_else(|| {
            // 浏览器 WebSocket 无法自定义 Authorization header，回退到 query token。
            // JWT 是 base64url 字符集（无 '+','/'），可直接用于 query。
            tracing::warn!("auth via query token — tokens in URLs can leak through logs");
            extract_query_param(req.uri().query(), "token")
        });
    let Some(token) = token else {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "missing authorization token"})),
        )
            .into_response());
    };

    let claims = state.auth.verify_token(&token).map_err(|e| {
        tracing::warn!("auth rejected: {e}");
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "invalid or expired token"})),
        )
            .into_response()
    })?;

    req.extensions_mut().insert(claims);
    Ok(next.run(req).await)
}

/// RBAC 权限判定：角色 → 权限映射。
/// admin 全量；operator 可读写 API + 运维面；viewer 只读。
pub fn has_permission(user_role: &str, required: &str) -> bool {
    match user_role {
        "admin" => true,
        "operator" => matches!(
            required,
            "api:read" | "api:write" | "ops:audit" | "ops:cmdb" | "ops:scripts"
        ),
        "viewer" => matches!(required, "api:read"),
        _ => false,
    }
}

/// 角色校验中间件：读取 extensions 中的 AuthClaims，按 has_permission 判定；
/// 无权限返回 403 {"error":"forbidden"}。须在 auth_middleware 之后挂载。
pub async fn require_role(
    required: &'static str,
    req: Request<Body>,
    next: Next,
) -> Result<Response, Response> {
    let claims = req.extensions().get::<AuthClaims>();
    let role = claims.and_then(|c| c.role.as_deref()).unwrap_or("");
    if has_permission(role, required) {
        Ok(next.run(req).await)
    } else {
        Err((StatusCode::FORBIDDEN, Json(json!({ "error": "forbidden" }))).into_response())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state() -> AuthState {
        AuthState::new(AuthConfig {
            jwt_secret: "0123456789abcdef0123456789abcdef".into(),
            access_token_ttl: 3600,
            refresh_token_ttl: 86400,
        })
    }

    #[test]
    fn token_roundtrip() {
        let state = test_state();
        let token = state
            .create_token("u1", "erik", Some("admin"), 3600)
            .unwrap();
        let claims = state.verify_token(&token).unwrap();
        assert_eq!(claims.subject(), "u1");
        assert_eq!(claims.role.as_deref(), Some("admin"));
        assert_eq!(
            claims.extra.get("username").and_then(|v| v.as_str()),
            Some("erik")
        );
    }

    #[test]
    fn token_without_role() {
        let state = test_state();
        let token = state.create_token("u1", "erik", None, 3600).unwrap();
        let claims = state.verify_token(&token).unwrap();
        assert_eq!(claims.role, None);
    }

    #[test]
    fn verify_rejects_tampered() {
        let state = test_state();
        let token = state.create_token("u1", "erik", None, 3600).unwrap();
        let bad = format!("{}x", &token[..token.len() - 2]);
        assert!(state.verify_token(&bad).is_err());
    }

    #[test]
    fn create_token_rejects_weak_secret() {
        let state = AuthState::new(AuthConfig {
            jwt_secret: "short".into(),
            access_token_ttl: 3600,
            refresh_token_ttl: 86400,
        });
        assert!(matches!(
            state.create_token("u1", "erik", None, 60),
            Err(ecat_auth::JwtAuthError::WeakKey)
        ));
    }

    #[test]
    fn has_permission_matrix() {
        // 计划基线
        assert!(has_permission("admin", "ops:users"));
        assert!(has_permission("operator", "api:write"));
        assert!(!has_permission("operator", "ops:users"));
        assert!(has_permission("viewer", "api:read"));
        assert!(!has_permission("viewer", "api:write"));
        assert!(!has_permission("banned", "api:read"));
        // P5-6 新增断言
        assert!(has_permission("admin", "ops:admin"));
        assert!(!has_permission("operator", "ops:admin"));
        assert!(!has_permission("viewer", "ops:audit"));
        // operator 其余可授权权限
        assert!(has_permission("operator", "api:read"));
        assert!(has_permission("operator", "ops:audit"));
        assert!(has_permission("operator", "ops:cmdb"));
        assert!(has_permission("operator", "ops:scripts"));
    }

    #[tokio::test]
    async fn require_role_enforces_permissions() {
        use axum::body::Body;
        use axum::extract::Extension;
        use axum::http::Request;
        use tower::util::ServiceExt;

        // 测试用认证层：验证 Bearer token 并把 claims 放入 extensions（模拟 auth_middleware）
        async fn inject_claims(
            State(state): State<AuthState>,
            mut req: Request<Body>,
            next: Next,
        ) -> Result<Response, Response> {
            let token =
                extract_bearer(req.headers(), axum::http::header::AUTHORIZATION.as_str()).unwrap();
            let claims = state.verify_token(&token).unwrap();
            req.extensions_mut().insert(claims);
            Ok(next.run(req).await)
        }

        async fn whoami(Extension(claims): Extension<AuthClaims>) -> Json<serde_json::Value> {
            Json(json!({ "role": claims.role }))
        }

        let app = axum::Router::new()
            .route("/whoami", axum::routing::get(whoami))
            .layer(axum::middleware::from_fn(move |req, next| {
                require_role("ops:users", req, next)
            }))
            .layer(axum::middleware::from_fn_with_state(
                test_state(),
                inject_claims,
            ));

        // admin 通过（200）
        let admin = test_state()
            .create_token("u1", "admin", Some("admin"), 3600)
            .unwrap();
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/whoami")
                    .header("authorization", format!("Bearer {admin}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        // viewer 拒绝（403 + 固定错误体）
        let viewer = test_state()
            .create_token("u1", "viewer", Some("viewer"), 3600)
            .unwrap();
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/whoami")
                    .header("authorization", format!("Bearer {viewer}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
        let body = axum::body::to_bytes(res.into_body(), 1024).await.unwrap();
        assert_eq!(&body[..], br#"{"error":"forbidden"}"#);

        // 无 role 也拒绝（403）
        let no_role = test_state()
            .create_token("u1", "ghost", None, 3600)
            .unwrap();
        let res = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/whoami")
                    .header("authorization", format!("Bearer {no_role}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }
}
