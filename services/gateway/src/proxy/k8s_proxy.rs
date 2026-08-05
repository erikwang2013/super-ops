use axum::{extract::{Path, Query}, http::StatusCode, response::IntoResponse, Json};
use serde::Deserialize;

pub fn k8s_routes() -> axum::Router<crate::AppState> {
    axum::Router::<crate::AppState>::new()
        .route("/api/k8s/clusters", axum::routing::get(list_clusters).post(add_cluster))
        .route("/api/k8s/clusters/{cluster_id}", axum::routing::get(get_cluster).delete(remove_cluster))
        .route("/api/k8s/clusters/{cluster_id}/pods", axum::routing::get(list_pods))
        .route("/api/k8s/clusters/{cluster_id}/pods/{namespace}/{pod}/logs", axum::routing::get(get_pod_logs))
        .route("/api/k8s/clusters/{cluster_id}/pods/{namespace}/{pod}/exec", axum::routing::get(exec_pod_ws))
        .route("/api/k8s/clusters/{cluster_id}/deployments", axum::routing::get(list_deployments))
        .route("/api/k8s/clusters/{cluster_id}/nodes", axum::routing::get(list_nodes))
        .route("/api/k8s/clusters/{cluster_id}/metrics", axum::routing::get(get_metrics))
}

#[derive(Debug, Deserialize)]
struct PodListQuery { namespace: Option<String>, page: Option<i32>, page_size: Option<i32> }

async fn list_clusters() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "clusters": [] }))
}
async fn add_cluster(Json(body): Json<serde_json::Value>) -> impl IntoResponse {
    (StatusCode::CREATED, Json(serde_json::json!({ "id": "pending", "name": body.get("name"), "status": "connected" })))
}
async fn get_cluster(Path(cluster_id): Path<String>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "cluster": { "id": cluster_id } }))
}
async fn remove_cluster(Path(_id): Path<String>) -> StatusCode { StatusCode::NO_CONTENT }
async fn list_pods(Path(cid): Path<String>, Query(q): Query<PodListQuery>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "pods": [], "cluster_id": cid, "namespace": q.namespace.unwrap_or_default() }))
}
async fn get_pod_logs(Path((cid, ns, pod)): Path<(String, String, String)>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "logs": "", "cluster_id": cid, "pod": pod, "namespace": ns }))
}
async fn exec_pod_ws(Path((cid, ns, pod)): Path<(String, String, String)>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "terminal endpoint ready", "cluster_id": cid, "namespace": ns, "pod": pod }))
}
async fn list_deployments(Path(cid): Path<String>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "deployments": [], "cluster_id": cid }))
}
async fn list_nodes(Path(cid): Path<String>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "nodes": [], "cluster_id": cid }))
}
async fn get_metrics(Path(cid): Path<String>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "metrics": [], "cluster_id": cid }))
}
