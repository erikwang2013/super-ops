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

pub struct AuthState {
    pub config: AuthConfig,
}

impl AuthState {
    pub fn new(config: AuthConfig) -> Self {
        Self { config }
    }

    /// 签发 JWT：委托框架 ecat-auth 的 HS256 签发（sub + username 附加 claim）。
    pub fn create_token(
        &self,
        user_id: &str,
        username: &str,
        ttl: u64,
    ) -> Result<String, ecat_auth::JwtAuthError> {
        let claims = AuthClaims {
            sub: user_id.to_string(),
            exp: None,
            iat: None,
            role: None,
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
            let claims = AuthClaims {
                sub: user_id,
                exp: None,
                iat: None,
                role: None,
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
        let token = state.create_token("u1", "erik", 3600).unwrap();
        let claims = state.verify_token(&token).unwrap();
        assert_eq!(claims.subject(), "u1");
        assert_eq!(
            claims.extra.get("username").and_then(|v| v.as_str()),
            Some("erik")
        );
    }

    #[test]
    fn verify_rejects_tampered() {
        let state = test_state();
        let token = state.create_token("u1", "erik", 3600).unwrap();
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
            state.create_token("u1", "erik", 60),
            Err(ecat_auth::JwtAuthError::WeakKey)
        ));
    }
}
