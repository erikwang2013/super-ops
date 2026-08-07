use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;

use crate::model::oncall::{
    create_shift, current_shift, delete_shift, list_shifts, validate_shift,
};

#[derive(Debug, Deserialize)]
pub struct ListShiftsQuery {
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CreateShift {
    pub name: String,
    pub assignee: String,
    pub start_at: String,
    pub end_at: String,
}

pub async fn list_oncall_shifts(
    State(state): State<crate::AppState>,
    Query(q): Query<ListShiftsQuery>,
) -> impl IntoResponse {
    match list_shifts(&state.pool, q.limit.unwrap_or(100)).await {
        Ok(shifts) => (
            StatusCode::OK,
            Json(serde_json::json!({ "shifts": shifts })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("oncall query failed: {e}") })),
        )
            .into_response(),
    }
}

pub async fn current_oncall_shift(State(state): State<crate::AppState>) -> impl IntoResponse {
    match current_shift(&state.pool).await {
        Ok(Some(shift)) => {
            (StatusCode::OK, Json(serde_json::json!({ "shift": shift }))).into_response()
        }
        Ok(None) => (
            StatusCode::OK,
            Json(serde_json::json!({ "shift": null, "message": "当前无生效值班班次" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("oncall query failed: {e}") })),
        )
            .into_response(),
    }
}

pub async fn create_oncall_shift(
    State(state): State<crate::AppState>,
    Json(req): Json<CreateShift>,
) -> impl IntoResponse {
    if let Err(msg) = validate_shift(&req.name, &req.assignee, &req.start_at, &req.end_at) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": msg })),
        )
            .into_response();
    }
    match create_shift(
        &state.pool,
        &req.name,
        &req.assignee,
        &req.start_at,
        &req.end_at,
    )
    .await
    {
        Ok(id) => (StatusCode::CREATED, Json(serde_json::json!({ "id": id }))).into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("oncall insert failed: {e}") })),
        )
            .into_response(),
    }
}

pub async fn delete_oncall_shift(
    State(state): State<crate::AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    match delete_shift(&state.pool, id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "班次不存在" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("oncall delete failed: {e}") })),
        )
            .into_response(),
    }
}
