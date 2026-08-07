use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use ecat_auth::AuthClaims;
use futures::StreamExt;
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
        .route("/api/k8s/aggregate", axum::routing::get(aggregate_clusters))
        .route(
            "/api/metrics/trend",
            axum::routing::get(crate::metrics_api::metrics_trend),
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
            "/api/k8s/clusters/{cluster_id}/deployments/{namespace}/{name}/image",
            axum::routing::post(update_deployment_image),
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

type ApiResult<T> = Result<Json<T>, (StatusCode, Json<serde_json::Value>)>;

async fn connect_client(
    state: &crate::AppState,
) -> Result<
    superops_protos::k8s::v1::k8s_service_client::K8sServiceClient<tonic::transport::Channel>,
    (StatusCode, Json<serde_json::Value>),
> {
    let endpoint = { state.k8s_endpoint.read().unwrap().clone() };
    superops_protos::k8s::v1::k8s_service_client::K8sServiceClient::connect(endpoint)
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": format!("k8s backend unreachable: {e}") })),
            )
        })
}

// D1 跨集群：集群管理/资源列表真实转发 k8s-service（多集群按 cluster_id 路由）
async fn list_clusters(State(state): State<crate::AppState>) -> ApiResult<serde_json::Value> {
    let mut client = connect_client(&state).await?;
    let clusters = client
        .list_clusters(superops_protos::k8s::v1::ListClustersRequest::default())
        .await
        .map_err(status_to_http)?
        .into_inner()
        .clusters;
    let clusters: Vec<serde_json::Value> = clusters
        .iter()
        .map(|c| {
            serde_json::json!({
                "id": c.id, "name": c.name, "version": c.version,
                "status": c.status, "node_count": c.node_count, "pod_count": c.pod_count,
            })
        })
        .collect();
    Ok(Json(serde_json::json!({ "clusters": clusters })))
}
async fn add_cluster(
    State(state): State<crate::AppState>,
    Json(body): Json<serde_json::Value>,
) -> ApiResult<serde_json::Value> {
    let name = body
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or_default()
        .to_string();
    let kubeconfig = body
        .get("kubeconfig")
        .and_then(|k| k.as_str())
        .unwrap_or_default()
        .as_bytes()
        .to_vec();
    let mut client = connect_client(&state).await?;
    let c = client
        .add_cluster(superops_protos::k8s::v1::AddClusterRequest { name, kubeconfig })
        .await
        .map_err(status_to_http)?
        .into_inner()
        .cluster
        .ok_or_else(|| {
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": "add cluster: empty response" })),
            )
        })?;
    Ok(Json(
        serde_json::json!({ "id": c.id, "name": c.name, "status": c.status }),
    ))
}
async fn get_cluster(
    State(state): State<crate::AppState>,
    Path(cluster_id): Path<String>,
) -> ApiResult<serde_json::Value> {
    let mut client = connect_client(&state).await?;
    let c = client
        .get_cluster(superops_protos::k8s::v1::GetClusterRequest {
            cluster_id: cluster_id.clone(),
        })
        .await
        .map_err(status_to_http)?
        .into_inner()
        .cluster
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": format!("cluster {cluster_id} not found") })),
            )
        })?;
    Ok(Json(serde_json::json!({ "cluster": {
        "id": c.id, "name": c.name, "version": c.version,
        "status": c.status, "node_count": c.node_count, "pod_count": c.pod_count,
    } })))
}
async fn remove_cluster(
    State(state): State<crate::AppState>,
    Path(cluster_id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let mut client = connect_client(&state).await?;
    client
        .remove_cluster(superops_protos::k8s::v1::RemoveClusterRequest { cluster_id })
        .await
        .map_err(status_to_http)?;
    Ok(StatusCode::NO_CONTENT)
}
async fn list_pods(
    State(state): State<crate::AppState>,
    Path(cid): Path<String>,
    Query(q): Query<PodListQuery>,
) -> ApiResult<serde_json::Value> {
    let mut client = connect_client(&state).await?;
    let pods = client
        .list_pods(superops_protos::k8s::v1::ListPodsRequest {
            cluster_id: cid.clone(),
            namespace: q.namespace.unwrap_or_default(),
            ..Default::default()
        })
        .await
        .map_err(status_to_http)?
        .into_inner()
        .pods;
    let pods: Vec<serde_json::Value> = pods
        .iter()
        .map(|p| {
            serde_json::json!({
                "name": p.name, "namespace": p.namespace, "status": p.status,
                "node": p.node, "restarts": p.restarts, "age": p.age,
            })
        })
        .collect();
    Ok(Json(serde_json::json!({ "cluster_id": cid, "pods": pods })))
}
async fn get_pod_logs(
    State(state): State<crate::AppState>,
    Path((cid, ns, pod)): Path<(String, String, String)>,
) -> ApiResult<serde_json::Value> {
    let mut client = connect_client(&state).await?;
    let mut stream = client
        .get_pod_logs(superops_protos::k8s::v1::GetPodLogsRequest {
            cluster_id: cid.clone(),
            namespace: ns.clone(),
            pod_name: pod.clone(),
            container: String::new(),
            tail_lines: 200,
            follow: false,
        })
        .await
        .map_err(status_to_http)?
        .into_inner();
    let mut lines = Vec::new();
    while let Some(line) = stream.next().await {
        match line {
            Ok(l) => lines.push(l.content),
            Err(e) => {
                return Err((
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({ "error": format!("log stream: {e}") })),
                ));
            }
        }
    }
    Ok(Json(serde_json::json!({
        "cluster_id": cid, "pod": pod, "namespace": ns, "logs": lines.join("\n"),
    })))
}
async fn list_deployments(
    State(state): State<crate::AppState>,
    Path(cid): Path<String>,
) -> ApiResult<serde_json::Value> {
    let mut client = connect_client(&state).await?;
    let deps = client
        .list_deployments(superops_protos::k8s::v1::ListDeploymentsRequest {
            cluster_id: cid.clone(),
            namespace: String::new(),
        })
        .await
        .map_err(status_to_http)?
        .into_inner()
        .deployments;
    let deps: Vec<serde_json::Value> = deps
        .iter()
        .map(|d| {
            serde_json::json!({
                "name": d.name, "namespace": d.namespace, "replicas": d.replicas,
                "ready_replicas": d.ready_replicas, "age": d.age, "images": d.images,
            })
        })
        .collect();
    Ok(Json(
        serde_json::json!({ "cluster_id": cid, "deployments": deps }),
    ))
}

/// GET /api/k8s/aggregate —— D1 跨集群聚合视图：枚举全部集群并汇总节点/pod 健康度
pub async fn aggregate_clusters(
    State(state): State<crate::AppState>,
) -> ApiResult<serde_json::Value> {
    let mut client = connect_client(&state).await?;
    let clusters = client
        .list_clusters(superops_protos::k8s::v1::ListClustersRequest::default())
        .await
        .map_err(status_to_http)?
        .into_inner()
        .clusters;
    let mut items = Vec::new();
    let mut totals = serde_json::json!({ "clusters": clusters.len(), "nodes": 0, "nodes_ready": 0, "pods": 0, "pods_running": 0 });
    for c in &clusters {
        let nodes_res = client
            .list_nodes(superops_protos::k8s::v1::ListNodesRequest {
                cluster_id: c.id.clone(),
            })
            .await;
        let pods_res = client
            .list_pods(superops_protos::k8s::v1::ListPodsRequest {
                cluster_id: c.id.clone(),
                ..Default::default()
            })
            .await;
        let (nodes_total, nodes_ready) = match nodes_res {
            Ok(r) => {
                let n = r.into_inner().nodes;
                (n.len(), n.iter().filter(|n| n.status == "Ready").count())
            }
            Err(_) => (0, 0),
        };
        let (pods_total, pods_running) = match pods_res {
            Ok(r) => {
                let p = r.into_inner().pods;
                (p.len(), p.iter().filter(|p| p.status == "Running").count())
            }
            Err(_) => (0, 0),
        };
        totals["nodes"] = (totals["nodes"].as_i64().unwrap_or(0) + nodes_total as i64).into();
        totals["nodes_ready"] =
            (totals["nodes_ready"].as_i64().unwrap_or(0) + nodes_ready as i64).into();
        totals["pods"] = (totals["pods"].as_i64().unwrap_or(0) + pods_total as i64).into();
        totals["pods_running"] =
            (totals["pods_running"].as_i64().unwrap_or(0) + pods_running as i64).into();
        items.push(serde_json::json!({
            "id": c.id, "name": c.name, "status": c.status,
            "nodes": nodes_total, "nodes_ready": nodes_ready,
            "pods": pods_total, "pods_running": pods_running,
        }));
    }
    Ok(Json(
        serde_json::json!({ "clusters": items, "totals": totals }),
    ))
}
async fn list_nodes(
    State(state): State<crate::AppState>,
    Path(cid): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let mut client = connect_client(&state).await?;
    let nodes = client
        .list_nodes(superops_protos::k8s::v1::ListNodesRequest {
            cluster_id: cid.clone(),
        })
        .await
        .map_err(status_to_http)?
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

async fn update_deployment_image(
    State(state): State<crate::AppState>,
    Extension(claims): Extension<AuthClaims>,
    headers: HeaderMap,
    Path((cid, ns, name)): Path<(String, String, String)>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let image = body
        .get("image")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "image 必须为非空字符串" })),
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
        .update_deployment_image(superops_protos::k8s::v1::UpdateDeploymentImageRequest {
            cluster_id: cid.clone(),
            namespace: ns.clone(),
            name: name.clone(),
            image: image.clone(),
        })
        .await
        .map_err(status_to_http)?
        .into_inner();
    publish_audit(
        &state,
        "k8s.update-image",
        claims_username(&claims),
        &client_ip(&headers),
        &format!("update deployment {ns}/{name} image to {image}"),
        &serde_json::json!({
            "cluster_id": cid.clone(),
            "namespace": ns.clone(),
            "name": name.clone(),
            "image": image.clone(),
        }),
    )
    .await;
    Ok(Json(
        serde_json::json!({ "cluster_id": cid, "deployment": { "namespace": ns, "name": name, "image": resp.image } }),
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
