use axum::{
    extract::{
        Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use superops_protos::k8s::v1::ExecRequest;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

#[derive(Debug, Deserialize)]
pub struct ExecQuery {
    pub container: Option<String>,
    pub command: Option<String>,
}

/// GET /api/k8s/clusters/{cid}/pods/{ns}/{pod}/exec —— WebSocket 交互式终端桥
pub async fn exec_pod_ws(
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
    // P6-5 终端录制：本会话唯一 session_id，逐帧经 recorder 落 ClickHouse
    let rec_session = crate::recorder::session_id();
    let rec_node = format!("{ns}/{pod}");

    let mut client =
        match superops_protos::k8s::v1::k8s_service_client::K8sServiceClient::connect(endpoint)
            .await
        {
            Ok(c) => c,
            Err(e) => return send_error(socket, &format!("k8s backend unreachable: {e}")).await,
        };
    let (req_tx, req_rx) = mpsc::channel::<ExecRequest>(64);
    // 先入队初始化消息再发起双向流：服务端 handler 会先读第一个消息才返回响应头，
    // 若在 exec_pod().await 之后才 send 会造成客户端等响应头、服务端等首消息的死锁
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
    let resp = match client.exec_pod(ReceiverStream::new(req_rx)).await {
        Ok(r) => r,
        Err(e) => return send_error(socket, &format!("exec rpc failed: {e}")).await,
    };
    let mut exec_stream = resp.into_inner();
    let (mut ws_tx, mut ws_rx) = socket.split();

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
                if state.recording_enabled {
                    crate::recorder::record_frame(&state.ch, &rec_session, &rec_node, &combined);
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
