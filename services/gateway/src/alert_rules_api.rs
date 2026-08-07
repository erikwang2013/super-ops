use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;

use crate::model::alert_rule::{
    CreateRuleArgs, create_rule, delete_rule, list_rules, set_rule_enabled, validate_rule,
};

#[derive(Debug, Deserialize)]
pub struct CreateRule {
    pub name: String,
    pub metric: String,
    #[serde(default = "default_operator")]
    pub operator: String,
    #[serde(default = "default_threshold")]
    pub threshold: String,
    #[serde(default = "default_level")]
    pub level: String,
    #[serde(default = "default_action")]
    pub action: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_operator() -> String {
    "ge".into()
}
fn default_threshold() -> String {
    "1".into()
}
fn default_level() -> String {
    "WARN".into()
}
fn default_action() -> String {
    "notify".into()
}
fn default_enabled() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct SetEnabled {
    pub enabled: bool,
}

pub async fn list_alert_rules(State(state): State<crate::AppState>) -> impl IntoResponse {
    match list_rules(&state.pool).await {
        Ok(rules) => (StatusCode::OK, Json(serde_json::json!({ "rules": rules }))).into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("alert rules query failed: {e}") })),
        )
            .into_response(),
    }
}

pub async fn create_alert_rule(
    State(state): State<crate::AppState>,
    Json(req): Json<CreateRule>,
) -> impl IntoResponse {
    if let Err(msg) = validate_rule(
        &req.name,
        &req.metric,
        &req.operator,
        &req.threshold,
        &req.level,
        &req.action,
    ) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": msg })),
        )
            .into_response();
    }
    match create_rule(
        &state.pool,
        CreateRuleArgs {
            name: &req.name,
            metric: &req.metric,
            op: &req.operator,
            threshold: &req.threshold,
            level: &req.level,
            action: &req.action,
            enabled: req.enabled,
        },
    )
    .await
    {
        Ok(id) => (StatusCode::CREATED, Json(serde_json::json!({ "id": id }))).into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("alert rule create failed: {e}") })),
        )
            .into_response(),
    }
}

pub async fn set_alert_rule_enabled(
    State(state): State<crate::AppState>,
    Path(id): Path<i64>,
    Json(req): Json<SetEnabled>,
) -> impl IntoResponse {
    match set_rule_enabled(&state.pool, id, req.enabled).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "alert rule not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("alert rule update failed: {e}") })),
        )
            .into_response(),
    }
}

pub async fn delete_alert_rule(
    State(state): State<crate::AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    match delete_rule(&state.pool, id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "alert rule not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("alert rule delete failed: {e}") })),
        )
            .into_response(),
    }
}
