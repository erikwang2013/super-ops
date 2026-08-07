use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;

use crate::model::cmdb::{
    AssetArgs, asset_type_counts, delete_asset, list_assets, upsert_asset, validate_asset,
};
use crate::model::tenant::Tenant;

#[derive(Debug, Default, Deserialize)]
pub struct ListAssetsQuery {
    pub asset_type: Option<String>,
    pub env: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateAsset {
    pub asset_type: String,
    pub name: String,
    pub ip: Option<String>,
    pub env: Option<String>,
    pub owner: Option<String>,
    pub labels: Option<String>,
}

pub async fn list_cmdb_assets(
    State(state): State<crate::AppState>,
    Query(q): Query<ListAssetsQuery>,
    Extension(tenant): Extension<Tenant>,
) -> impl IntoResponse {
    match list_assets(
        &state.pool,
        &tenant.0,
        q.asset_type.as_deref(),
        q.env.as_deref(),
    )
    .await
    {
        Ok(assets) => (
            StatusCode::OK,
            Json(serde_json::json!({ "assets": assets })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("cmdb query failed: {e}") })),
        )
            .into_response(),
    }
}

pub async fn create_cmdb_asset(
    State(state): State<crate::AppState>,
    Extension(tenant): Extension<Tenant>,
    Json(req): Json<CreateAsset>,
) -> impl IntoResponse {
    if let Err(msg) = validate_asset(&req.asset_type, &req.name) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": msg })),
        )
            .into_response();
    }
    match upsert_asset(
        &state.pool,
        AssetArgs {
            tenant: &tenant.0,
            asset_type: &req.asset_type,
            name: &req.name,
            ip: req.ip.as_deref(),
            env: req.env.as_deref().unwrap_or("prod"),
            owner: req.owner.as_deref().unwrap_or(""),
            labels: req.labels.as_deref(),
        },
    )
    .await
    {
        Ok(id) => (StatusCode::CREATED, Json(serde_json::json!({ "id": id }))).into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("cmdb write failed: {e}") })),
        )
            .into_response(),
    }
}

pub async fn delete_cmdb_asset(
    State(state): State<crate::AppState>,
    Path(id): Path<i64>,
    Extension(tenant): Extension<Tenant>,
) -> impl IntoResponse {
    match delete_asset(&state.pool, &tenant.0, id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "asset not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("cmdb delete failed: {e}") })),
        )
            .into_response(),
    }
}

// stats 响应键：host→hosts、switch→switches，其余取 asset_type 原值（router/app/db/storage）
const STATS_KEYS: [(&str, &str); 6] = [
    ("host", "hosts"),
    ("switch", "switches"),
    ("router", "router"),
    ("app", "app"),
    ("db", "db"),
    ("storage", "storage"),
];

pub async fn cmdb_stats(
    State(state): State<crate::AppState>,
    Extension(tenant): Extension<Tenant>,
) -> impl IntoResponse {
    match asset_type_counts(&state.pool, &tenant.0).await {
        Ok(counts) => {
            let mut m = serde_json::Map::new();
            for (t, key) in STATS_KEYS {
                m.insert(
                    key.to_string(),
                    serde_json::json!(counts.get(t).copied().unwrap_or(0)),
                );
            }
            (StatusCode::OK, Json(serde_json::Value::Object(m))).into_response()
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("cmdb stats failed: {e}") })),
        )
            .into_response(),
    }
}
