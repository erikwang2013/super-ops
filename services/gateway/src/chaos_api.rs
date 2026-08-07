use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;

use crate::model::chaos::{
    delete_chaos, get_chaos, insert_chaos, list_chaos, set_chaos_status, validate_chaos,
};

#[derive(Debug, Deserialize)]
pub struct CreateChaos {
    pub name: String,
    #[serde(default = "default_cluster")]
    pub cluster_id: String,
    pub target_name: String,
    pub action: String, // restart | delete
    #[serde(default)]
    pub operator: String,
}

fn default_cluster() -> String {
    "default".into()
}

pub async fn list_chaos_handler(State(state): State<crate::AppState>) -> impl IntoResponse {
    match list_chaos(&state.pool, 100).await {
        Ok(rows) => (
            StatusCode::OK,
            Json(serde_json::json!({ "experiments": rows })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("chaos query failed: {e}") })),
        )
            .into_response(),
    }
}

pub async fn create_chaos_handler(
    State(state): State<crate::AppState>,
    Json(req): Json<CreateChaos>,
) -> impl IntoResponse {
    if let Err(msg) = validate_chaos(&req.name, &req.target_name, &req.action) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": msg })),
        )
            .into_response();
    }
    let id = match insert_chaos(
        &state.pool,
        &req.name,
        &req.cluster_id,
        &req.target_name,
        &req.action,
        &req.operator,
    )
    .await
    {
        Ok(id) => id,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": format!("chaos insert failed: {e}") })),
            )
                .into_response();
        }
    };
    (StatusCode::CREATED, Json(serde_json::json!({ "id": id }))).into_response()
}

pub async fn delete_chaos_handler(
    State(state): State<crate::AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    match delete_chaos(&state.pool, id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "experiment not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("chaos delete failed: {e}") })),
        )
            .into_response(),
    }
}

/// 执行混沌动作：restart → RestartDeployment；delete → DeleteDeployment。
/// 先落 running + started_at(NOW)，再调 k8s-service；成败回填 completed/failed + ended_at + error。
pub async fn run_chaos_handler(
    State(state): State<crate::AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let row = match get_chaos(&state.pool, id).await {
        Ok(Some(r)) => r,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "experiment not found" })),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": format!("chaos query failed: {e}") })),
            )
                .into_response();
        }
    };
    if row.status == "running" {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": "experiment already running" })),
        )
            .into_response();
    }
    if let Err(e) = set_chaos_status(&state.pool, id, "running", true, false, None).await {
        return (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("chaos update failed: {e}") })),
        )
            .into_response();
    }
    let endpoint = { state.k8s_endpoint.read().unwrap().clone() };
    let mut client =
        match superops_protos::k8s::v1::k8s_service_client::K8sServiceClient::connect(endpoint)
            .await
        {
            Ok(c) => c,
            Err(e) => {
                let msg = format!("k8s backend unreachable: {e}");
                let _ = set_chaos_status(&state.pool, id, "failed", false, true, Some(&msg)).await;
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({ "error": msg })),
                )
                    .into_response();
            }
        };
    let result = match row.action.as_str() {
        "delete" => client
            .delete_deployment(superops_protos::k8s::v1::DeleteDeploymentRequest {
                cluster_id: row.cluster_id.clone(),
                namespace: "default".into(),
                name: row.target_name.clone(),
            })
            .await
            .map(|_| ()),
        _ => client
            .restart_deployment(superops_protos::k8s::v1::RestartDeploymentRequest {
                cluster_id: row.cluster_id.clone(),
                namespace: "default".into(),
                name: row.target_name.clone(),
            })
            .await
            .map(|_| ()),
    };
    let (status, err) = match result {
        Ok(()) => ("completed", None),
        Err(e) => ("failed", Some(e.to_string())),
    };
    if let Err(e) = set_chaos_status(&state.pool, id, status, false, true, err.as_deref()).await {
        return (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("chaos update failed: {e}") })),
        )
            .into_response();
    }
    if status == "completed" {
        (StatusCode::OK, Json(serde_json::json!({ "status": status }))).into_response()
    } else {
        (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({
                "error": format!("chaos execution failed: {}", err.as_deref().unwrap_or("unknown")),
                "status": status,
            })),
        )
            .into_response()
    }
}
