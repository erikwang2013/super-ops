use crate::AppState;
use axum::{Json, extract::State, http::StatusCode};
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
        .create_token(&user.id, &user.username, ttl)
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "failed to issue token"))?;
    let refresh = state
        .auth
        .create_token(
            &user.id,
            &user.username,
            state.auth.config.refresh_token_ttl,
        )
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "failed to issue token"))?;
    Ok(Json(TokenResponse {
        access_token: access,
        refresh_token: refresh,
        expires_in: ttl,
    }))
}

pub async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> Result<Json<TokenResponse>, ApiError> {
    validate_credentials(&req.username, &req.email, &req.password)?;
    let hash = bcrypt::hash(&req.password, bcrypt::DEFAULT_COST)
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "failed to hash password"))?;
    match state
        .user_store
        .create(&req.username, &req.email, &hash)
        .await
    {
        Ok(user) => {
            let ttl = state.auth.config.access_token_ttl;
            let access = state
                .auth
                .create_token(&user.id, &user.username, ttl)
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
