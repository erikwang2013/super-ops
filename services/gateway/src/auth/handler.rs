use crate::AppState;
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
}

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub email: String,
    pub password: String,
}

type ApiError = (StatusCode, Json<serde_json::Value>);

fn err(status: StatusCode, message: &str) -> ApiError {
    (status, Json(serde_json::json!({ "error": message })))
}

fn validate_credentials(username: &str, email: &str, password: &str) -> Result<(), ApiError> {
    if !(3..=32).contains(&username.len())
        || !username
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "username must be 3-32 chars: letters, digits, '_' or '-'",
        ));
    }
    if !(5..=254).contains(&email.len()) || !email.contains('@') || email.contains(' ') {
        return Err(err(StatusCode::BAD_REQUEST, "invalid email"));
    }
    if !(8..=72).contains(&password.len()) {
        return Err(err(StatusCode::BAD_REQUEST, "password must be 8-72 chars"));
    }
    Ok(())
}

pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<LoginRequest>,
) -> Result<Json<TokenResponse>, ApiError> {
    if req.username.is_empty() || req.password.is_empty() {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "username and password are required",
        ));
    }
    let Some(user) = state
        .user_store
        .find_by_username(&req.username)
        .await
        .ok()
        .flatten()
    else {
        return Err(err(StatusCode::UNAUTHORIZED, "invalid credentials"));
    };
    if !bcrypt::verify(&req.password, &user.password_hash).unwrap_or(false) {
        return Err(err(StatusCode::UNAUTHORIZED, "invalid credentials"));
    }
    let ttl = state.auth.config.access_token_ttl;
    let access = state
        .auth
        .create_token(&user.id, &user.username, Some(&user.role), ttl)
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "failed to issue token"))?;
    let refresh = state
        .auth
        .create_token(
            &user.id,
            &user.username,
            Some(&user.role),
            state.auth.config.refresh_token_ttl,
        )
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "failed to issue token"))?;
    crate::audit::publish(
        &state,
        "login_success",
        &user.username,
        &client_ip(&headers),
        "login ok",
        &serde_json::json!({}),
    )
    .await;
    Ok(Json(TokenResponse {
        access_token: access,
        refresh_token: refresh,
        expires_in: ttl,
    }))
}

pub async fn register(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RegisterRequest>,
) -> Result<Json<TokenResponse>, ApiError> {
    validate_credentials(&req.username, &req.email, &req.password)?;
    // 首个注册用户自动提升为 admin（count=0 判定存在并发注册双 admin 的竞态，内部工具可接受）；其余默认 viewer
    let role = if state.user_store.count().await.unwrap_or(1) == 0 {
        "admin"
    } else {
        "viewer"
    };
    let hash = bcrypt::hash(&req.password, bcrypt::DEFAULT_COST)
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "failed to hash password"))?;
    match state
        .user_store
        .create(&req.username, &req.email, &hash, role)
        .await
    {
        Ok(user) => {
            crate::audit::publish(
                &state,
                "register_success",
                &user.username,
                &client_ip(&headers),
                "new user registered",
                &serde_json::json!({}),
            )
            .await;
            let ttl = state.auth.config.access_token_ttl;
            let access = state
                .auth
                .create_token(&user.id, &user.username, Some(&user.role), ttl)
                .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "failed to issue token"))?;
            Ok(Json(TokenResponse {
                access_token: access,
                refresh_token: String::new(),
                expires_in: ttl,
            }))
        }
        Err(_) => Err(err(
            StatusCode::CONFLICT,
            "username or email already exists",
        )),
    }
}

fn client_ip(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("unknown")
        .to_string()
}

#[derive(Debug, Deserialize)]
pub struct SetUserStatusRequest {
    pub status: String,
}

fn validate_user_status(status: &str) -> Result<(), String> {
    match status {
        "enabled" | "disabled" => Ok(()),
        _ => Err(format!(
            "invalid status: {status} (must be 'enabled' or 'disabled')"
        )),
    }
}

pub async fn list_users(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let users = crate::model::user::list_users(&state.pool)
        .await
        .map_err(|_| err(StatusCode::BAD_GATEWAY, "failed to list users"))?;
    Ok(Json(serde_json::json!({ "users": users })))
}

pub async fn set_user_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SetUserStatusRequest>,
) -> Result<StatusCode, ApiError> {
    validate_user_status(&body.status).map_err(|m| err(StatusCode::BAD_REQUEST, &m))?;
    let updated = crate::model::user::set_user_status(&state.pool, &id, &body.status)
        .await
        .map_err(|_| err(StatusCode::BAD_GATEWAY, "failed to update user status"))?;
    if updated {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(err(StatusCode::NOT_FOUND, "user not found"))
    }
}

#[derive(Debug, Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

/// 从 claims 取 `jti`（黑名单吊销用）。
pub fn claims_jti(claims: &ecat_auth::AuthClaims) -> Option<String> {
    claims
        .extra
        .get("jti")
        .and_then(|v| v.as_str())
        .map(String::from)
}

/// POST /api/auth/refresh：校验 refresh token → 黑名单检查 → 轮换（旧 jti 作废）→ 签发新 token 对。
pub async fn refresh(
    State(state): State<AppState>,
    Json(req): Json<RefreshRequest>,
) -> Result<Json<TokenResponse>, ApiError> {
    if req.refresh_token.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "refresh_token is required"));
    }
    let claims = state
        .auth
        .verify_token(&req.refresh_token)
        .map_err(|_| err(StatusCode::UNAUTHORIZED, "invalid or expired refresh token"))?;
    let Some(jti) = claims_jti(&claims) else {
        return Err(err(StatusCode::UNAUTHORIZED, "invalid refresh token"));
    };
    if state.blacklist.is_revoked(&jti).await {
        return Err(err(StatusCode::UNAUTHORIZED, "refresh token revoked"));
    }
    let sub = claims.sub.clone();
    let username = claims
        .extra
        .get("username")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let role = claims.role.clone();

    // 轮换：旧 refresh 立即作废（jti 入黑名单），防止重放
    state
        .blacklist
        .revoke(&jti, state.auth.config.refresh_token_ttl)
        .await;

    let ttl = state.auth.config.access_token_ttl;
    let access = state
        .auth
        .create_token(&sub, &username, role.as_deref(), ttl)
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "failed to issue token"))?;
    let refresh = state
        .auth
        .create_token(
            &sub,
            &username,
            role.as_deref(),
            state.auth.config.refresh_token_ttl,
        )
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "failed to issue token"))?;
    Ok(Json(TokenResponse {
        access_token: access,
        refresh_token: refresh,
        expires_in: ttl,
    }))
}

/// POST /api/auth/logout：吊销 refresh token（jti 入黑名单）。
pub async fn logout(
    State(state): State<AppState>,
    Json(req): Json<RefreshRequest>,
) -> Result<StatusCode, ApiError> {
    if req.refresh_token.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "refresh_token is required"));
    }
    let claims = state
        .auth
        .verify_token(&req.refresh_token)
        .map_err(|_| err(StatusCode::UNAUTHORIZED, "invalid refresh token"))?;
    if let Some(jti) = claims_jti(&claims) {
        state
            .blacklist
            .revoke(&jti, state.auth.config.refresh_token_ttl)
            .await;
    }
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn mk_claims(jti: &str) -> ecat_auth::AuthClaims {
        let mut extra = HashMap::new();
        extra.insert(
            "username".to_string(),
            serde_json::Value::String("erik".into()),
        );
        extra.insert("jti".to_string(), serde_json::Value::String(jti.into()));
        ecat_auth::AuthClaims {
            sub: "u1".into(),
            exp: None,
            iat: None,
            role: Some("admin".into()),
            extra,
        }
    }

    #[test]
    fn claims_jti_extracts_jti() {
        let c = mk_claims("jt-123");
        assert_eq!(claims_jti(&c).as_deref(), Some("jt-123"));
    }

    #[test]
    fn claims_jti_missing_returns_none() {
        let c = ecat_auth::AuthClaims {
            sub: "u1".into(),
            exp: None,
            iat: None,
            role: None,
            extra: Default::default(),
        };
        assert_eq!(claims_jti(&c), None);
    }

    #[test]
    fn validate_user_status_accepts_enabled_and_disabled() {
        assert!(validate_user_status("enabled").is_ok());
        assert!(validate_user_status("disabled").is_ok());
    }

    #[test]
    fn validate_user_status_rejects_unknown() {
        for bad in ["", "active", "ENABLED", "enabled "] {
            assert!(validate_user_status(bad).is_err(), "should reject {bad:?}");
        }
    }
}
