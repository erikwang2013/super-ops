use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use ecat_auth::AuthClaims;
use ecat_mq::MessageQueue;
use serde::Deserialize;

/// 只读 k8s 路由（api:read）：含集群管理（add/remove cluster 为 stub，P5-6 未归类为写权限）。
pub fn k8s_read_routes() -> axum::Router<crate::AppState> {
    axum::Router::<crate::AppState>::new()
        .route(
            "/api/k8s/clusters",
            axum::routing::get(list_clusters).post(add_cluster),
        )
        .route(
            "/api/k8s/clusters/{cluster_id}",
            axum::routing::get(get_cluster).delete(remove_cluster),
        )
        .route(
            "/api/k8s/clusters/{cluster_id}/pods",
            axum::routing::get(list_pods),
        )
        .route(
            "/api/k8s/clusters/{cluster_id}/pods/{namespace}/{pod}/logs",
            axum::routing::get(get_pod_logs),
        )
        .route(
            "/api/k8s/clusters/{cluster_id}/deployments",
            axum::routing::get(list_deployments),
        )
        .route(
            "/api/k8s/clusters/{cluster_id}/nodes",
            axum::routing::get(list_nodes),
        )
        .route(
            "/api/k8s/clusters/{cluster_id}/metrics",
            axum::routing::get(get_metrics),
        )
        .route(
            "/api/v1/metrics/query",
            axum::routing::get(crate::metrics_api::metrics_query),
        )
}

/// 写操作 k8s 路由（api:write）：scale / restart / delete deployment；
/// exec 为交互式 shell（等效写通道），也归入写路由，终端页需 operator+。
pub fn k8s_write_routes() -> axum::Router<crate::AppState> {
    axum::Router::<crate::AppState>::new()
        .route(
            "/api/k8s/clusters/{cluster_id}/pods/{namespace}/{pod}/exec",
            axum::routing::get(crate::proxy::k8s_exec::exec_pod_ws),
        )
        .route(
            "/api/k8s/clusters/{cluster_id}/deployments/{namespace}/{name}/scale",
            axum::routing::post(scale_deployment),
        )
        .route(
            "/api/k8s/clusters/{cluster_id}/deployments/{namespace}/{name}/restart",
            axum::routing::post(restart_deployment),
        )
        .route(
            "/api/k8s/clusters/{cluster_id}/deployments/{namespace}/{name}",
            axum::routing::delete(delete_deployment),
        )
}

#[derive(Debug, Deserialize)]
struct PodListQuery {
    namespace: Option<String>,
    #[allow(dead_code)]
    page: Option<i32>,
    #[allow(dead_code)]
    page_size: Option<i32>,
}

async fn list_clusters() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "clusters": [] }))
}
async fn add_cluster(Json(body): Json<serde_json::Value>) -> impl IntoResponse {
    let name = body
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or_default()
        .to_string();
    (
        StatusCode::CREATED,
        Json(serde_json::json!({ "id": "pending", "name": name, "status": "connected" })),
    )
}
async fn get_cluster(Path(cluster_id): Path<String>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "cluster": { "id": cluster_id } }))
}
async fn remove_cluster(Path(_id): Path<String>) -> StatusCode {
    StatusCode::NO_CONTENT
}
async fn list_pods(
    Path(cid): Path<String>,
    Query(q): Query<PodListQuery>,
) -> Json<serde_json::Value> {
    Json(
        serde_json::json!({ "pods": [], "cluster_id": cid, "namespace": q.namespace.unwrap_or_default() }),
    )
}
async fn get_pod_logs(
    Path((cid, ns, pod)): Path<(String, String, String)>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "logs": "", "cluster_id": cid, "pod": pod, "namespace": ns }))
}
async fn list_deployments(Path(cid): Path<String>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "deployments": [], "cluster_id": cid }))
}
async fn list_nodes(
    State(state): State<crate::AppState>,
    Path(cid): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let endpoint = { state.k8s_endpoint.read().unwrap().clone() };
    let mut client =
        superops_protos::k8s::v1::k8s_service_client::K8sServiceClient::connect(endpoint)
            .await
            .map_err(|e| {
                (
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({ "error": format!("k8s backend unreachable: {e}") })),
                )
            })?;
    let nodes = client
        .list_nodes(superops_protos::k8s::v1::ListNodesRequest::default())
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": format!("k8s rpc failed: {e}") })),
            )
        })?
        .into_inner()
        .nodes;
    let nodes: Vec<serde_json::Value> = nodes
        .iter()
        .map(|n| {
            serde_json::json!({
                "name": n.name,
                "status": n.status,
                "role": n.role,
                "version": n.version,
                "age": n.age,
                "cpu": n.cpu,
                "memory": n.memory,
            })
        })
        .collect();
    Ok(Json(
        serde_json::json!({ "cluster_id": cid, "nodes": nodes }),
    ))
}
async fn get_metrics(Path(cid): Path<String>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "metrics": [], "cluster_id": cid }))
}

fn parse_scale_body(body: &serde_json::Value) -> Result<i32, String> {
    let replicas = body
        .get("replicas")
        .and_then(|r| r.as_i64())
        .ok_or_else(|| "replicas (int) is required".to_string())?;
    if !(0..=1000).contains(&replicas) {
        return Err(format!("replicas must be in 0..=1000, got {replicas}"));
    }
    Ok(replicas as i32)
}

pub(crate) fn status_to_http(e: tonic::Status) -> (StatusCode, Json<serde_json::Value>) {
    let code = match e.code() {
        tonic::Code::NotFound => StatusCode::NOT_FOUND,
        tonic::Code::InvalidArgument => StatusCode::BAD_REQUEST,
        _ => StatusCode::BAD_GATEWAY,
    };
    (code, Json(serde_json::json!({ "error": e.message() })))
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn client_ip(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("unknown")
        .to_string()
}

pub(crate) fn claims_username(claims: &AuthClaims) -> &str {
    claims
        .extra
        .get("username")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
}

/// 发布审计事件，与 auth/handler.rs 的 publish_audit 同构；失败仅告警，绝不阻塞主流程。
async fn publish_audit(
    state: &crate::AppState,
    event_type: &str,
    username: &str,
    ip: &str,
    detail: &str,
    extra: &serde_json::Value,
) {
    if let Some(mq) = &state.mq {
        let mut payload = serde_json::json!({
            "ts": now_secs(),
            "event_type": event_type,
            "level": "INFO",
            "username": username,
            "ip": ip,
            "detail": detail,
        });
        if let Some(extra_obj) = extra.as_object() {
            for (k, v) in extra_obj {
                payload[k] = v.clone();
            }
        }
        if let Err(e) = mq
            .publish(
                "superops.audit",
                &serde_json::to_vec(&payload).unwrap_or_default(),
            )
            .await
        {
            tracing::warn!("audit publish failed: {e}");
        }
    }
}

async fn scale_deployment(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<AuthClaims>,
    headers: HeaderMap,
    Path((cid, ns, name)): Path<(String, String, String)>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let replicas = parse_scale_body(&body).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e })),
        )
    })?;
    let endpoint = { state.k8s_endpoint.read().unwrap().clone() };
    let mut client =
        superops_protos::k8s::v1::k8s_service_client::K8sServiceClient::connect(endpoint)
            .await
            .map_err(|e| {
                (
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({ "error": format!("k8s backend unreachable: {e}") })),
                )
            })?;
    let resp = client
        .scale_deployment(superops_protos::k8s::v1::ScaleDeploymentRequest {
            cluster_id: cid.clone(),
            namespace: ns.clone(),
            name: name.clone(),
            replicas,
        })
        .await
        .map_err(status_to_http)?
        .into_inner();
    publish_audit(
        &state,
        "k8s.scale",
        claims_username(&claims),
        &client_ip(&headers),
        &format!("scale deployment {ns}/{name} to {replicas} replicas"),
        &serde_json::json!({
            "cluster_id": cid.clone(),
            "namespace": ns.clone(),
            "name": name.clone(),
            "replicas": replicas,
        }),
    )
    .await;
    Ok(Json(
        serde_json::json!({ "cluster_id": cid, "deployment": { "namespace": ns, "name": name, "replicas": resp.replicas } }),
    ))
}

async fn restart_deployment(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<AuthClaims>,
    headers: HeaderMap,
    Path((cid, ns, name)): Path<(String, String, String)>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let endpoint = { state.k8s_endpoint.read().unwrap().clone() };
    let mut client =
        superops_protos::k8s::v1::k8s_service_client::K8sServiceClient::connect(endpoint)
            .await
            .map_err(|e| {
                (
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({ "error": format!("k8s backend unreachable: {e}") })),
                )
            })?;
    let _resp = client
        .restart_deployment(superops_protos::k8s::v1::RestartDeploymentRequest {
            cluster_id: cid.clone(),
            namespace: ns.clone(),
            name: name.clone(),
        })
        .await
        .map_err(status_to_http)?;
    publish_audit(
        &state,
        "k8s.restart",
        claims_username(&claims),
        &client_ip(&headers),
        &format!("restart deployment {ns}/{name}"),
        &serde_json::json!({
            "cluster_id": cid.clone(),
            "namespace": ns.clone(),
            "name": name.clone(),
        }),
    )
    .await;
    Ok(Json(
        serde_json::json!({ "cluster_id": cid, "deployment": { "namespace": ns, "name": name, "restarted": true } }),
    ))
}

async fn delete_deployment(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<AuthClaims>,
    headers: HeaderMap,
    Path((cid, ns, name)): Path<(String, String, String)>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    // P6-1: 审批门禁 — enabled 时删除需先通过审批（kind='delete' 且 status='approved'）。
    // target 约定为 "{cluster_id}/{ns}/{name}"，避免跨集群同名 deployment 绕过门禁。
    if state.approval_enabled {
        let approved =
            crate::model::approval::is_delete_approved(&state.pool, &format!("{cid}/{ns}/{name}"))
                .await
                .map_err(|e| {
                    (
                        StatusCode::BAD_GATEWAY,
                        Json(serde_json::json!({
                            "error": format!("approval check failed: {e}")
                        })),
                    )
                })?;
        if !approved {
            return Err((
                StatusCode::PRECONDITION_FAILED,
                Json(serde_json::json!({
                    "error": "删除需先通过审批 (POST /api/approvals)"
                })),
            ));
        }
    }
    let endpoint = { state.k8s_endpoint.read().unwrap().clone() };
    let mut client =
        superops_protos::k8s::v1::k8s_service_client::K8sServiceClient::connect(endpoint)
            .await
            .map_err(|e| {
                (
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({ "error": format!("k8s backend unreachable: {e}") })),
                )
            })?;
    client
        .delete_deployment(superops_protos::k8s::v1::DeleteDeploymentRequest {
            cluster_id: cid.clone(),
            namespace: ns.clone(),
            name: name.clone(),
        })
        .await
        .map_err(status_to_http)?;
    publish_audit(
        &state,
        "k8s.delete",
        claims_username(&claims),
        &client_ip(&headers),
        &format!("delete deployment {ns}/{name}"),
        &serde_json::json!({
            "cluster_id": cid.clone(),
            "namespace": ns.clone(),
            "name": name.clone(),
        }),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_scale_body_valid() {
        assert_eq!(
            parse_scale_body(&serde_json::json!({ "replicas": 3 })),
            Ok(3)
        );
    }

    #[test]
    fn parse_scale_body_rejects_bad_input() {
        assert!(parse_scale_body(&serde_json::json!({})).is_err());
        assert!(parse_scale_body(&serde_json::json!({ "replicas": 2000 })).is_err());
        assert!(parse_scale_body(&serde_json::json!({ "replicas": -1 })).is_err());
    }
}
