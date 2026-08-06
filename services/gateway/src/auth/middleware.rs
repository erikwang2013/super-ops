use axum::{
    Json,
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::config::AuthConfig;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,
    pub username: String,
    pub exp: usize,
    pub iat: usize,
}

pub struct AuthState {
    pub config: AuthConfig,
}

impl AuthState {
    pub fn new(config: AuthConfig) -> Self {
        Self { config }
    }

    pub fn create_token(
        &self,
        user_id: &str,
        username: &str,
        ttl: u64,
    ) -> Result<String, jsonwebtoken::errors::Error> {
        let now = chrono::Utc::now().timestamp() as usize;
        let claims = Claims {
            sub: user_id.to_string(),
            username: username.to_string(),
            exp: now + ttl as usize,
            iat: now,
        };
        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(self.config.jwt_secret.as_bytes()),
        )
    }

    pub fn verify_token(&self, token: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
        decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.config.jwt_secret.as_bytes()),
            &Validation::default(),
        )
        .map(|data| data.claims)
    }
}

pub async fn auth_middleware(
    State(state): State<AppState>,
    mut req: Request<Body>,
    next: Next,
) -> Result<Response, Response> {
    // OAuth2 层已把 AuthClaims 放入 extensions（ecat_auth::OAuth2Layer）→ 直接放行
    if req.extensions().get::<ecat_auth::AuthClaims>().is_some() {
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
            let claims = Claims {
                sub: user_id,
                username: "api-key".into(),
                exp: 0,
                iat: 0,
            };
            req.extensions_mut().insert(claims);
            return Ok(next.run(req).await);
        }
    }
    let token = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .or_else(|| {
            // 浏览器 WebSocket 无法自定义 Authorization header，回退到 query token。
            // JWT 是 base64url 字符集（无 '+','/'），可直接用于 query。
            tracing::warn!("auth via query token — tokens in URLs can leak through logs");
            extract_query_token(req.uri())
        });
    let Some(token) = token else {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "missing authorization token"})),
        )
            .into_response());
    };

    let claims = state.auth.verify_token(&token).map_err(|e| {
        tracing::warn!("auth rejected: {e}");
        (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "invalid or expired token"})),
        )
            .into_response()
    })?;

    req.extensions_mut().insert(claims);
    Ok(next.run(req).await)
}

fn extract_query_token(uri: &axum::http::Uri) -> Option<String> {
    let query = uri.query()?;
    for pair in query.split('&') {
        let mut it = pair.splitn(2, '=');
        if it.next() == Some("token") {
            let v = it.next().unwrap_or_default();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uri(s: &str) -> axum::http::Uri {
        s.parse().unwrap()
    }

    #[test]
    fn extract_query_token_reads_token_param() {
        assert_eq!(
            extract_query_token(&uri("/exec?token=abc.def.ghi")),
            Some("abc.def.ghi".to_string())
        );
    }

    #[test]
    fn extract_query_token_ignores_other_params() {
        assert_eq!(
            extract_query_token(&uri("/exec?container=x&token=tok")),
            Some("tok".to_string())
        );
        assert_eq!(extract_query_token(&uri("/exec?container=x")), None);
    }

    #[test]
    fn extract_query_token_handles_no_query() {
        assert_eq!(extract_query_token(&uri("/exec")), None);
    }
}
