use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;

use crate::model::release::{list_releases, record_release, validate_release};

#[derive(Debug, Deserialize)]
pub struct ListReleasesQuery {
    pub status: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CreateRelease {
    #[serde(default = "default_cluster")]
    pub cluster_id: String,
    pub namespace: String,
    pub name: String,
    #[serde(default)]
    pub old_image: String,
    pub new_image: String,
    #[serde(default)]
    pub operator: String,
}

fn default_cluster() -> String {
    "default".into()
}

pub async fn list_releases_handler(
    State(state): State<crate::AppState>,
    Query(q): Query<ListReleasesQuery>,
) -> impl IntoResponse {
    match list_releases(&state.pool, q.status.as_deref(), q.limit.unwrap_or(100)).await {
        Ok(releases) => (
            StatusCode::OK,
            Json(serde_json::json!({ "releases": releases })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("release query failed: {e}") })),
        )
            .into_response(),
    }
}

/// 发布：校验 → 调 k8s UpdateDeploymentImage → 落 release 记录（成功 ok / 失败 failed）。
pub async fn create_release_handler(
    State(state): State<crate::AppState>,
    Json(req): Json<CreateRelease>,
) -> impl IntoResponse {
    if req.namespace.is_empty() || req.name.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "namespace/name 不能为空" })),
        )
            .into_response();
    }
    if let Err(msg) = validate_release(&req.new_image, "pending") {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": msg })),
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
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({ "error": format!("k8s backend unreachable: {e}") })),
                )
                    .into_response();
            }
        };
    let update = client
        .update_deployment_image(superops_protos::k8s::v1::UpdateDeploymentImageRequest {
            cluster_id: req.cluster_id.clone(),
            namespace: req.namespace.clone(),
            name: req.name.clone(),
            image: req.new_image.clone(),
        })
        .await;
    let (status, image, err_msg) = match update {
        Ok(resp) => ("ok", resp.into_inner().image, None),
        Err(e) => ("failed", req.new_image.clone(), Some(e.to_string())),
    };
    let old_image = if req.old_image.is_empty() {
        "-"
    } else {
        &req.old_image
    };
    let inserted = record_release(
        &state.pool,
        &req.cluster_id,
        &req.namespace,
        &req.name,
        old_image,
        &image,
        &req.operator,
        status,
    )
    .await;
    if let Some(e) = err_msg {
        return (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("k8s update image failed: {e}") })),
        )
            .into_response();
    }
    match inserted {
        Ok(id) => (
            StatusCode::CREATED,
            Json(serde_json::json!({ "id": id, "image": image, "status": status })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("release record failed: {e}") })),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use crate::model::release::RELEASE_STATUSES;

    #[test]
    fn release_statuses_covered() {
        assert_eq!(RELEASE_STATUSES, ["pending", "rolling", "ok", "failed"]);
    }
}
