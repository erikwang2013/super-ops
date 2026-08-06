use crate::AppState;
use crate::model::secret;
use crate::vault;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde_json::Value;

type ApiError = (StatusCode, Json<Value>);

fn err(status: StatusCode, message: &str) -> ApiError {
    (status, Json(serde_json::json!({ "error": message })))
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

async fn require_key(state: &AppState) -> Result<&[u8], ApiError> {
    state
        .master_key
        .as_deref()
        .ok_or_else(|| err(StatusCode::SERVICE_UNAVAILABLE, "master key not configured"))
}

pub async fn create_secret(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let key = require_key(&state).await?;
    let name = body
        .get("name")
        .and_then(|n| n.as_str())
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "name is required"))?;
    if !valid_name(name) {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "name must be 1-128 chars: letters, digits, '.', '_' or '-'",
        ));
    }
    let value = body
        .get("value")
        .and_then(|v| v.as_str())
        .ok_or_else(|| err(StatusCode::BAD_REQUEST, "value is required"))?;
    if value.is_empty() || value.len() > 65536 {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "value must be 1..=65536 bytes",
        ));
    }
    let blob = vault::encrypt_value(key, value).map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to encrypt secret",
        )
    })?;
    let row = secret::upsert_secret(&state.pool, name, &blob)
        .await
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "failed to store secret"))?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "name": row.name, "created_at": row.created_at })),
    ))
}

pub async fn list_secrets(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    require_key(&state).await?;
    let rows = secret::list_secrets(&state.pool)
        .await
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "failed to list secrets"))?;
    let items: Vec<Value> = rows
        .into_iter()
        .map(|r| serde_json::json!({ "name": r.name, "created_at": r.created_at }))
        .collect();
    Ok(Json(Value::Array(items)))
}

pub async fn get_secret(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let key = require_key(&state).await?;
    if !valid_name(&name) {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "name must be 1-128 chars: letters, digits, '.', '_' or '-'",
        ));
    }
    let Some(row) = secret::get_secret(&state.pool, &name)
        .await
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "failed to get secret"))?
    else {
        return Err(err(StatusCode::NOT_FOUND, "secret not found"));
    };
    let value = vault::decrypt_value(key, &row.ciphertext).map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to decrypt secret",
        )
    })?;
    Ok(Json(serde_json::json!({ "value": value })))
}

pub async fn delete_secret(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<StatusCode, ApiError> {
    require_key(&state).await?;
    if !valid_name(&name) {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "name must be 1-128 chars: letters, digits, '.', '_' or '-'",
        ));
    }
    let deleted = secret::delete_secret(&state.pool, &name)
        .await
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "failed to delete secret"))?;
    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(err(StatusCode::NOT_FOUND, "secret not found"))
    }
}
