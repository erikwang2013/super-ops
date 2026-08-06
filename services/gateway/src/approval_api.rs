use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use ecat_auth::AuthClaims;
use serde::Deserialize;

use crate::model::approval::{
    APPROVAL_ACTIONS, APPROVAL_KINDS, APPROVAL_STATUSES, create_approval, get_approval,
    list_approvals, next_status, update_approval_status, validate_approval,
};
use crate::proxy::k8s_proxy::claims_username;

#[derive(Debug, Default, Deserialize)]
pub struct ListApprovalsQuery {
    pub status: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
pub struct CreateApproval {
    pub kind: String,
    pub target: String,
    pub reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct DecideApproval {
    pub action: String,
}

pub async fn list_approvals_handler(
    State(state): State<crate::AppState>,
    Query(q): Query<ListApprovalsQuery>,
) -> impl IntoResponse {
    if let Some(s) = &q.status {
        if !APPROVAL_STATUSES.contains(&s.as_str()) {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!("status must be one of {:?}", APPROVAL_STATUSES)
                })),
            )
                .into_response();
        }
    }
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    match list_approvals(&state.pool, q.status.as_deref(), limit).await {
        Ok(rows) => (
            StatusCode::OK,
            Json(serde_json::json!({ "approvals": rows })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("approval query failed: {e}") })),
        )
            .into_response(),
    }
}

pub async fn create_approval_handler(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<AuthClaims>,
    Json(req): Json<CreateApproval>,
) -> impl IntoResponse {
    let kind = req.kind.trim();
    if !APPROVAL_KINDS.contains(&kind) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": format!("kind must be one of {:?}", APPROVAL_KINDS)
            })),
        )
            .into_response();
    }
    let target = req.target.trim();
    if target.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "target must not be empty" })),
        )
            .into_response();
    }
    if target.chars().count() > 255 {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "target too long (max 255 chars)" })),
        )
            .into_response();
    }
    let reason = req.reason.unwrap_or_default();
    let reason = reason.trim();
    if reason.chars().count() > 512 {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "reason too long (max 512 chars)" })),
        )
            .into_response();
    }
    let operator = claims_username(&claims).to_string();
    match create_approval(&state.pool, kind, target, reason, &operator).await {
        Ok(id) => (
            StatusCode::CREATED,
            Json(serde_json::json!({ "id": id, "status": "pending" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("approval write failed: {e}") })),
        )
            .into_response(),
    }
}

pub async fn decide_approval_handler(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<AuthClaims>,
    Path(id): Path<i64>,
    Json(req): Json<DecideApproval>,
) -> impl IntoResponse {
    if !validate_approval(&req.action) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": format!("action must be one of {:?}", APPROVAL_ACTIONS)
            })),
        )
            .into_response();
    }
    let mut current = match get_approval(&state.pool, id).await {
        Ok(Some(row)) => row,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "approval not found" })),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": format!("approval query failed: {e}") })),
            )
                .into_response();
        }
    };
    let Some(next) = next_status(&current.status, &req.action) else {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": format!("status {} does not allow action {}", current.status, req.action)
            })),
        )
            .into_response();
    };
    let decided_by = claims_username(&claims).to_string();
    match update_approval_status(&state.pool, id, &current.status, next, &decided_by).await {
        Ok(true) => {
            // CAS 成功后直接构造响应，避免 TOCTOU 重查；decided_at 由 DB NOW() 写入，留待下次查询回读
            current.status = next.to_string();
            current.decided_by = Some(decided_by);
            (StatusCode::OK, Json(current)).into_response()
        }
        Ok(false) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": "approval state changed concurrently" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("approval update failed: {e}") })),
        )
            .into_response(),
    }
}
