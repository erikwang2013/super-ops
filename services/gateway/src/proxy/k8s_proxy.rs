use axum::{
    Json,
    extract::{
        Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use superops_protos::k8s::v1::ExecRequest;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

pub fn k8s_routes() -> axum::Router<crate::AppState> {
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
            "/api/k8s/clusters/{cluster_id}/pods/{namespace}/{pod}/exec",
            axum::routing::get(exec_pod_ws),
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

#[derive(Debug, Deserialize)]
struct PodListQuery {
    namespace: Option<String>,
    #[allow(dead_code)]
    page: Option<i32>,
    #[allow(dead_code)]
    page_size: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct ExecQuery {
    container: Option<String>,
    command: Option<String>,
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
async fn exec_pod_ws(
    State(state): State<crate::AppState>,
    Path((cid, ns, pod)): Path<(String, String, String)>,
    Query(q): Query<ExecQuery>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| exec_socket(socket, state, cid, ns, pod, q))
}

async fn exec_socket(
    socket: WebSocket,
    state: crate::AppState,
    cid: String,
    ns: String,
    pod: String,
    q: ExecQuery,
) {
    let command = q.command.unwrap_or_else(|| "/bin/sh".into());
    let container = q.container.filter(|c| !c.is_empty()).unwrap_or_default();
    let endpoint = { state.k8s_endpoint.read().unwrap().clone() };

    let mut client =
        match superops_protos::k8s::v1::k8s_service_client::K8sServiceClient::connect(endpoint)
            .await
        {
            Ok(c) => c,
            Err(e) => return send_error(socket, &format!("k8s backend unreachable: {e}")).await,
        };
    let (req_tx, req_rx) = mpsc::channel::<ExecRequest>(64);
    let resp = match client.exec_pod(ReceiverStream::new(req_rx)).await {
        Ok(r) => r,
        Err(e) => return send_error(socket, &format!("exec rpc failed: {e}")).await,
    };
    let mut exec_stream = resp.into_inner();
    let (mut ws_tx, mut ws_rx) = socket.split();

    let _ = req_tx
        .send(ExecRequest {
            cluster_id: cid,
            namespace: ns,
            pod_name: pod,
            container,
            command,
            stdin: Vec::new(),
            terminal_size: None,
        })
        .await;

    // ws → gRPC stdin
    let inbound = tokio::spawn(async move {
        while let Some(msg) = ws_rx.next().await {
            let Ok(msg) = msg else { break };
            match msg {
                Message::Text(t) => {
                    if req_tx
                        .send(ExecRequest {
                            stdin: t.as_bytes().to_vec(),
                            ..Default::default()
                        })
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Message::Binary(b) => {
                    if req_tx
                        .send(ExecRequest {
                            stdin: b.to_vec(),
                            ..Default::default()
                        })
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    // gRPC → ws
    while let Some(item) = exec_stream.next().await {
        match item {
            Ok(resp) => {
                let mut combined = resp.stdout;
                if !resp.stderr.is_empty() {
                    combined.extend_from_slice(&resp.stderr);
                }
                if ws_tx.send(Message::Binary(combined.into())).await.is_err() {
                    break;
                }
            }
            Err(e) => {
                let _ = ws_tx
                    .send(Message::Text(format!("exec error: {e}").into()))
                    .await;
                break;
            }
        }
    }
    inbound.abort();
}

async fn send_error(socket: WebSocket, msg: &str) {
    let (mut ws_tx, _ws_rx) = socket.split();
    let _ = ws_tx
        .send(Message::Text(format!("error: {msg}").into()))
        .await;
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
