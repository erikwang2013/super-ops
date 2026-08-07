use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;

use crate::model::quota::{QuotaArgs, delete_quota, list_quotas, upsert_quota, validate_quota};
use crate::model::tenant::Tenant;

#[derive(Debug, Default, Deserialize)]
pub struct ListQuotaQuery {
    pub cluster_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateQuota {
    #[serde(default = "default_cluster")]
    pub cluster_id: String,
    pub namespace: String,
    #[serde(default)]
    pub cpu_request: String,
    #[serde(default)]
    pub memory_request: String,
    #[serde(default)]
    pub cpu_limit: String,
    #[serde(default)]
    pub memory_limit: String,
    #[serde(default)]
    pub replicas: i32,
    #[serde(default)]
    pub description: String,
}

fn default_cluster() -> String {
    "default".into()
}

pub async fn list_quotas_handler(
    State(state): State<crate::AppState>,
    Query(q): Query<ListQuotaQuery>,
    Extension(_tenant): Extension<Tenant>,
) -> impl IntoResponse {
    match list_quotas(&state.pool, q.cluster_id.as_deref()).await {
        Ok(quotas) => (
            StatusCode::OK,
            Json(serde_json::json!({ "quotas": quotas })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("quota query failed: {e}") })),
        )
            .into_response(),
    }
}

pub async fn create_quota(
    State(state): State<crate::AppState>,
    Extension(_tenant): Extension<Tenant>,
    Json(req): Json<CreateQuota>,
) -> impl IntoResponse {
    if let Err(msg) = validate_quota(&req.namespace) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": msg })),
        )
            .into_response();
    }
    match upsert_quota(
        &state.pool,
        QuotaArgs {
            cluster_id: &req.cluster_id,
            namespace: &req.namespace,
            cpu_request: &req.cpu_request,
            memory_request: &req.memory_request,
            cpu_limit: &req.cpu_limit,
            memory_limit: &req.memory_limit,
            replicas: req.replicas,
            description: &req.description,
        },
    )
    .await
    {
        Ok(id) => (StatusCode::CREATED, Json(serde_json::json!({ "id": id }))).into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("quota write failed: {e}") })),
        )
            .into_response(),
    }
}

pub async fn delete_quota_handler(
    State(state): State<crate::AppState>,
    Path(id): Path<i64>,
    Extension(_tenant): Extension<Tenant>,
) -> impl IntoResponse {
    match delete_quota(&state.pool, id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "quota not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("quota delete failed: {e}") })),
        )
            .into_response(),
    }
}
