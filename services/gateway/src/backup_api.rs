use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;

use crate::model::backup::{backup_summary, list_backups, report_backup, validate_backup_report};

#[derive(Debug, Deserialize)]
pub struct ReportBackup {
    pub db_name: String,
    #[serde(default)]
    pub target: String,
    pub status: String,
    #[serde(default)]
    pub size_bytes: i64,
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Default, Deserialize)]
pub struct ListQuery {
    pub limit: Option<i64>,
}

/// 备份 agent 上报（供 cron/mysqldump 等外部工具回调）。
pub async fn report_backup_handler(
    State(state): State<crate::AppState>,
    Json(req): Json<ReportBackup>,
) -> impl IntoResponse {
    if let Err(msg) = validate_backup_report(&req.db_name, &req.status, req.size_bytes) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": msg })),
        )
            .into_response();
    }
    let target: String = req.target.chars().take(255).collect();
    let message: String = req.message.chars().take(255).collect();
    match report_backup(
        &state.pool,
        &req.db_name,
        &target,
        &req.status,
        req.size_bytes,
        &message,
    )
    .await
    {
        Ok(id) => (StatusCode::CREATED, Json(serde_json::json!({ "id": id }))).into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("backup report failed: {e}") })),
        )
            .into_response(),
    }
}

pub async fn list_backup_status_handler(
    State(state): State<crate::AppState>,
    Query(q): Query<ListQuery>,
) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    match list_backups(&state.pool, limit).await {
        Ok(statuses) => (
            StatusCode::OK,
            Json(serde_json::json!({ "backups": statuses })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("backup query failed: {e}") })),
        )
            .into_response(),
    }
}

pub async fn backup_summary_handler(State(state): State<crate::AppState>) -> impl IntoResponse {
    match backup_summary(&state.pool).await {
        Ok(summaries) => (
            StatusCode::OK,
            Json(serde_json::json!({ "summaries": summaries })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("backup summary failed: {e}") })),
        )
            .into_response(),
    }
}
