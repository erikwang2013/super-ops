use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;

use crate::model::ticket::{
    TICKET_SOURCES, TICKET_STATUSES, create_ticket, delete_ticket, list_tickets,
    update_ticket_status, validate_ticket,
};

#[derive(Debug, Deserialize)]
pub struct ListTicketsQuery {
    pub status: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CreateTicket {
    pub title: String,
    pub description: Option<String>,
    #[serde(default = "default_severity")]
    pub severity: String,
    #[serde(default = "default_source")]
    pub source: String,
    #[serde(default)]
    pub alert_title: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateTicketStatus {
    pub status: String,
    pub assignee: Option<String>,
}

fn default_severity() -> String {
    "LOW".into()
}
fn default_source() -> String {
    "manual".into()
}

pub async fn list_tickets_handler(
    State(state): State<crate::AppState>,
    Query(q): Query<ListTicketsQuery>,
) -> impl IntoResponse {
    match list_tickets(&state.pool, q.status.as_deref(), q.limit.unwrap_or(100)).await {
        Ok(tickets) => (
            StatusCode::OK,
            Json(serde_json::json!({ "tickets": tickets })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("ticket query failed: {e}") })),
        )
            .into_response(),
    }
}

pub async fn create_ticket_handler(
    State(state): State<crate::AppState>,
    Json(req): Json<CreateTicket>,
) -> impl IntoResponse {
    if !TICKET_SOURCES.contains(&req.source.as_str()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(
                serde_json::json!({ "error": format!("source 必须是 {:?} 之一", TICKET_SOURCES) }),
            ),
        )
            .into_response();
    }
    if let Err(msg) = validate_ticket(&req.title, &req.severity, "open") {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": msg })),
        )
            .into_response();
    }
    match create_ticket(
        &state.pool,
        &req.title,
        req.description.as_deref(),
        &req.severity,
        &req.source,
        &req.alert_title,
        "",
    )
    .await
    {
        Ok(id) => (StatusCode::CREATED, Json(serde_json::json!({ "id": id }))).into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("ticket insert failed: {e}") })),
        )
            .into_response(),
    }
}

pub async fn update_ticket_status_handler(
    State(state): State<crate::AppState>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateTicketStatus>,
) -> impl IntoResponse {
    if !TICKET_STATUSES.contains(&req.status.as_str()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(
                serde_json::json!({ "error": format!("status 必须是 {:?} 之一", TICKET_STATUSES) }),
            ),
        )
            .into_response();
    }
    match update_ticket_status(&state.pool, id, &req.status, req.assignee.as_deref()).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "工单不存在或已关闭" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("ticket update failed: {e}") })),
        )
            .into_response(),
    }
}

pub async fn delete_ticket_handler(
    State(state): State<crate::AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    match delete_ticket(&state.pool, id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "工单不存在" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("ticket delete failed: {e}") })),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use crate::model::ticket::TICKET_SEVERITIES;

    #[test]
    fn severities_cover_frontend_levels() {
        assert_eq!(TICKET_SEVERITIES, ["LOW", "MEDIUM", "HIGH", "CRIT"]);
    }
}
