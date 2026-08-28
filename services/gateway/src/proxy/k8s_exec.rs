use axum::{
    Json,
    extract::{
        Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use std::time::Duration;
use superops_protos::k8s::v1::{ExecRequest, TerminalSize};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ExecClientMsg {
    Resize { cols: i32, rows: i32 },
    Stdin(String),
}

fn is_nonneg_whole(v: &serde_json::Value) -> Option<i32> {
    match v {
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64()
                && (0..=i32::MAX as i64).contains(&i)
            {
                return Some(i as i32);
            }
            if let Some(f) = n.as_f64()
                && f >= 0.0
                && f.fract() == 0.0
                && f <= i32::MAX as f64
            {
                return Some(f as i32);
            }
            None
        }
        _ => None,
    }
}

/// 仅识别 `{"type":"resize","cols":N,"rows":N}`（非负整数，允许 80.0）；其余文本当 stdin，便于输入 `{`。
pub(crate) fn parse_exec_client_text(t: &str) -> ExecClientMsg {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(t) else {
        return ExecClientMsg::Stdin(t.to_string());
    };
    let Some(obj) = v.as_object() else {
        return ExecClientMsg::Stdin(t.to_string());
    };
    if obj.get("type").and_then(|x| x.as_str()) != Some("resize") {
        return ExecClientMsg::Stdin(t.to_string());
    }
    let (Some(cols), Some(rows)) = (
        obj.get("cols").and_then(is_nonneg_whole),
        obj.get("rows").and_then(is_nonneg_whole),
    ) else {
        return ExecClientMsg::Stdin(t.to_string());
    };
    ExecClientMsg::Resize { cols, rows }
}
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

#[derive(Debug, Deserialize)]
pub struct ExecQuery {
    pub container: Option<String>,
    pub command: Option<String>,
    pub confirm: Option<String>,
}

/// GET /api/k8s/clusters/{cid}/pods/{ns}/{pod}/exec —— WebSocket 交互式终端桥
/// 会话管控（B3）：require_confirm 配置开启时需带 ?confirm=1；会话时长上限 max_session_secs
pub async fn exec_pod_ws(
    State(state): State<crate::AppState>,
    Path((cid, ns, pod)): Path<(String, String, String)>,
    Query(q): Query<ExecQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    if state.terminal.require_confirm && q.confirm.as_deref() != Some("1") {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "terminal session requires ?confirm=1"})),
        )
            .into_response();
    }
    let max_session_secs = state.terminal.max_session_secs;
    ws.on_upgrade(move |socket| exec_socket(socket, state, cid, ns, pod, q, max_session_secs))
        .into_response()
}

async fn exec_socket(
    socket: WebSocket,
    state: crate::AppState,
    cid: String,
    ns: String,
    pod: String,
    q: ExecQuery,
    max_session_secs: u64,
) {
    let command = q.command.unwrap_or_else(|| "/bin/sh".into());
    let container = q.container.filter(|c| !c.is_empty()).unwrap_or_default();
    let endpoint = { state.k8s_endpoint.read().unwrap().clone() };
    // P6-5 终端录制：本会话唯一 session_id，逐帧经 recorder 落 ClickHouse
    let rec_session = crate::recorder::session_id();
    let rec_node = format!("{ns}/{pod}");

    let mut client = match crate::k8s_client::connect(&endpoint, &state.k8s_token).await {
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
                    let req = match parse_exec_client_text(&t) {
                        ExecClientMsg::Resize { cols, rows } => ExecRequest {
                            terminal_size: Some(TerminalSize {
                                width: cols,
                                height: rows,
                            }),
                            ..Default::default()
                        },
                        ExecClientMsg::Stdin(s) => ExecRequest {
                            stdin: s.into_bytes(),
                            ..Default::default()
                        },
                    };
                    if req_tx.send(req).await.is_err() {
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

    // gRPC → ws；B3 会话管控：max_session_secs 到点发送过期消息并断开
    let timeout = tokio::time::sleep(Duration::from_secs(max_session_secs));
    tokio::pin!(timeout);
    loop {
        tokio::select! {
            item = exec_stream.next() => {
                let Some(item) = item else { break };
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
            _ = &mut timeout, if max_session_secs > 0 => {
                let _ = ws_tx
                    .send(Message::Text(
                        format!("session expired: {max_session_secs}s 时长上限，连接已关闭").into(),
                    ))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_resize_json() {
        assert_eq!(
            parse_exec_client_text(r#"{"type":"resize","cols":80,"rows":24}"#),
            ExecClientMsg::Resize { cols: 80, rows: 24 }
        );
    }

    #[test]
    fn parse_resize_float_wholes() {
        assert_eq!(
            parse_exec_client_text(r#"{"type":"resize","cols":80.0,"rows":24.0}"#),
            ExecClientMsg::Resize { cols: 80, rows: 24 }
        );
    }

    #[test]
    fn parse_raw_and_brace_are_stdin() {
        assert_eq!(
            parse_exec_client_text(""),
            ExecClientMsg::Stdin(String::new())
        );
        assert_eq!(
            parse_exec_client_text("{"),
            ExecClientMsg::Stdin("{".into())
        );
        assert_eq!(
            parse_exec_client_text("ls -la"),
            ExecClientMsg::Stdin("ls -la".into())
        );
    }

    #[test]
    fn parse_non_resize_or_missing_fields_are_stdin() {
        let keep = r#"{"type":"stdin","data":"x"}"#;
        assert_eq!(
            parse_exec_client_text(keep),
            ExecClientMsg::Stdin(keep.into())
        );
        let missing = r#"{"type":"resize","cols":80}"#;
        assert_eq!(
            parse_exec_client_text(missing),
            ExecClientMsg::Stdin(missing.into())
        );
        let neg = r#"{"type":"resize","cols":-1,"rows":24}"#;
        assert_eq!(
            parse_exec_client_text(neg),
            ExecClientMsg::Stdin(neg.into())
        );
    }
}
