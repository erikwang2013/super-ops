use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct LoginRequest { pub username: String, pub password: String }

#[derive(Debug, Serialize)]
pub struct TokenResponse { pub access_token: String, pub refresh_token: String, pub expires_in: u64 }

#[derive(Debug, Deserialize)]
pub struct RegisterRequest { pub username: String, pub email: String, pub password: String }

pub async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> impl IntoResponse {
    let user = match state.user_store.find_by_username(&req.username).await {
        Ok(Some(u)) => u,
        _ => return (StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "invalid credentials"}))).into_response(),
    };
    if !bcrypt::verify(&req.password, &user.password_hash).unwrap_or(false) {
        return (StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "invalid credentials"}))).into_response();
    }
    let ttl = state.auth.config.access_token_ttl;
    let access = state.auth.create_token(&user.id, &user.username, ttl).unwrap();
    let refresh = state.auth.create_token(&user.id, &user.username, state.auth.config.refresh_token_ttl).unwrap();
    (StatusCode::OK, Json(TokenResponse { access_token: access, refresh_token: refresh, expires_in: ttl })).into_response()
}

pub async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> impl IntoResponse {
    let hash = bcrypt::hash(&req.password, bcrypt::DEFAULT_COST).unwrap();
    match state.user_store.create(&req.username, &req.email, &hash).await {
        Ok(user) => {
            let ttl = state.auth.config.access_token_ttl;
            let access = state.auth.create_token(&user.id, &user.username, ttl).unwrap();
            (StatusCode::CREATED, Json(TokenResponse { access_token: access, refresh_token: String::new(), expires_in: ttl })).into_response()
        }
        Err(_) => (StatusCode::CONFLICT, Json(serde_json::json!({"error": "username or email already exists"}))).into_response(),
    }
}
