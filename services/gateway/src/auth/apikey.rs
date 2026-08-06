use crate::AppState;
use crate::auth::middleware::Claims;
use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
};
use serde_json::Value;

type ApiError = (StatusCode, Json<Value>);

fn err(status: StatusCode, message: &str) -> ApiError {
    (status, Json(serde_json::json!({ "error": message })))
}

pub async fn create_key(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let name = body
        .get("name")
        .and_then(|n| n.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| "default".into());
    if name.len() > 64 {
        return Err(err(StatusCode::BAD_REQUEST, "name must be <= 64 chars"));
    }
    let (id, plain) = state
        .api_keys
        .create(&claims.sub, &name)
        .await
        .map_err(|_| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to create api key",
            )
        })?;
    Ok(Json(serde_json::json!({
        "id": id,
        "name": name,
        "key": plain,
        "warning": "store this key now; it will not be shown again"
    })))
}

pub async fn list_keys(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, ApiError> {
    let keys = state
        .api_keys
        .list(&claims.sub)
        .await
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "failed to list api keys"))?;
    Ok(Json(serde_json::json!({ "keys": keys })))
}

pub async fn delete_key(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let deleted = state.api_keys.delete(&id, &claims.sub).await.map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to delete api key",
        )
    })?;
    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(err(StatusCode::NOT_FOUND, "api key not found"))
    }
}
