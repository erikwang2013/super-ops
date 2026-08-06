# P3 集成与对外（exec 终端 / API Key / OAuth2 / OpenAPI / bench）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 完成 super-ops P3 阶段：k8s exec 双向流、gateway WebSocket 终端桥、前端终端页打通、API Key、OAuth2（默认关闭）、OpenAPI 文档、bench 压测，并收尾全量验证。

**Architecture:** k8s 服务实现 `ExecPod` 双向流（kube `Api::exec` → `AttachedProcess` 三个 take 访问器：stdin writer + stdout/stderr reader）；gateway 用 axum ws（非 ecat-transport-ws——它是独立服务器不能嵌路由）把浏览器 WS 桥接为 gRPC 双向流；鉴权链扩展三种凭据（Bearer JWT → query token 回退（浏览器 WS 限制）→ X-API-Key）；API Key 用 `Arc<RwLock<HashMap<key_hash, user_id>>>` 内存表（满足"吊销立即失效"，偏离总纲的静态 ApiKeyLayer）；OAuth2 用 `ecat_auth::OAuth2Layer` 条件挂载（默认关闭）；OpenAPI 用 `ecat_openapi::OpenApiBuilder`；bench 用 `ecat_bench::run_bench`。

**Tech Stack:** tonic 0.12 双向流、kube 0.93.1 `Api::exec`（feature ws）、axum 0.8 ws（新增 feature）、futures/tokio-stream、sha2、ecat-auth/ecat-openapi/ecat-bench。

**已验证的前置事实（写计划时确认）：**
- kube 0.93.1 `Api::<Pod>::exec(name, command, &AttachParams) -> Result<AttachedProcess>`（subresource.rs:558）；`AttachParams{container, stdin, stdout, stderr, tty, ...}` 字段 pub + Default；**无 terminal_size 字段**（resize 通过 `AttachedProcess::terminal_size() -> Option<TerminalSizeSender>`（Sink）在运行中发送，`TerminalSize{height: u32, width: u32}`）。
- `AttachedProcess` 访问器均为 **take 语义**：`stdin()/stdout()/stderr() -> Option<impl AsyncRead/Write + Unpin>`（每次 &mut self 取走，可顺序取三次；**无 `split(self)` 方法**——0.93.1 没有）。`abort(&self)` 可兜底。
- proto `ExecRequest{cluster_id, namespace, pod_name, container, command: String, stdin: bytes, terminal_size: Option<TerminalSize>}` / `ExecResponse{stdout: bytes, stderr: bytes}` **已定型无需改 proto**。
- 浏览器 WebSocket **无法自定义 Authorization header** → auth_middleware 需 query `token` 回退。前端 token 存 zustand（`useAuthStore.getState().token`，frontend/src/stores/auth.ts），非 localStorage。
- 现有 `auth_middleware`（services/gateway/src/auth/middleware.rs）只认 Bearer JWT；gateway 自研 `Claims{sub, username, exp, iat}`（jsonwebtoken）。
- `ecat_auth::OAuth2Layer::new(introspection_url, client_id, client_secret) -> Result<Self, String>`（非 test 强制 https，缓存 300s）；鉴权失败返回 **401 Response**（非 Err），只有 inner 失败才是 Err → 上层仍需 `ErrorToResponseLayer` 满足 axum `Router::layer` 的 `Into<Infallible>` 约束。
- `ecat_auth::AuthClaims{sub, exp, iat, role, extra}` 可插入 request extensions，供 OAuth2 短路 JWT。
- `ecat_bench::run_bench(name, concurrency, total, f)` 闭包**无参数**且 `Future<Output = ()>`；`BenchResult::print()`。
- `ecat_openapi::OpenApiBuilder::new(title, version).add_route(path, method, summary, tags).add_schema(name, HashMap<String, Schema>).build() -> OpenApiSpec`（derive Serialize，`openapi` 字段 = "3.0.3"）。
- workspace tower 因 axum 传递启用 "util" feature → gateway 内 `tower::service_fn` 可用（breaker.rs 测试已用）。
- workspace `axum = "0.8"`（workspace 声明），gateway 需 `axum = { workspace = true, features = ["ws"] }`；gateway 缺 futures / tokio-stream / sha2 / ecat-auth / ecat-openapi。
- `ApiKeyLayer`（ecat-auth）静态 map 无法运行时吊销 → **偏离总纲**：改用自研内存表 + `Arc<RwLock<HashMap<String, String>>>`（key_hash → user_id），复用 DB 表 + 内存表双写。
- 前端无测试框架（package.json 无 test script）→ 验证 = `npm run build`（tsc + vite build）。
- MySQL 在 compose（`sudo docker compose up -d` 由用户运行）；DB 相关测试在无 DB 环境无法跑 → 只测纯函数，DB 路径靠编译 + 运行时。
- bench 需新建 workspace 成员 `bench/`（包名 "bench"，匹配 `cargo run -p bench`）；总纲基准 `cargo run --release -p bench`。

---

### Task 1: k8s exec 双向流（resource/exec.rs + service.rs exec_pod）

**Files:**
- Create: `services/k8s/src/resource/exec.rs`
- Modify: `services/k8s/src/resource/mod.rs`（加 `pub mod exec;`）
- Modify: `services/k8s/src/service.rs`（exec_pod 实现 + imports + 测试）

- [ ] **Step 1: 写失败测试（exec.rs 纯函数测试 + service.rs 错误路径测试）**

`services/k8s/src/resource/exec.rs`（完整内容，含实现与测试——实现简单，测试先行执行验证）：

```rust
use anyhow::Result;
use k8s_openapi::api::core::v1::Pod;
use kube::api::{AttachParams, AttachedProcess};
use kube::Api;

use crate::cluster::client::ClusterClient;

pub fn parse_command(command: &str) -> Vec<String> {
    command.split_whitespace().map(String::from).collect()
}

pub fn exec_params(container: Option<String>) -> AttachParams {
    AttachParams {
        container,
        stdin: true,
        stdout: true,
        stderr: true,
        tty: true,
        ..Default::default()
    }
}

pub async fn exec_pod(
    client: &ClusterClient,
    namespace: String,
    pod_name: String,
    container: Option<String>,
    command: &str,
) -> Result<AttachedProcess> {
    let api: Api<Pod> = Api::namespaced(client.client.clone(), &namespace);
    api.exec(&pod_name, parse_command(command), &exec_params(container))
        .await
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_command_splits_whitespace() {
        assert_eq!(parse_command("ls -la /tmp"), vec!["ls", "-la", "/tmp"]);
        assert_eq!(parse_command(""), Vec::<String>::new());
    }

    #[test]
    fn exec_params_forces_tty_and_three_streams() {
        let p = exec_params(Some("sidecar".into()));
        assert_eq!(p.container.as_deref(), Some("sidecar"));
        assert!(p.stdin && p.stdout && p.stderr && p.tty);
        let p = exec_params(None);
        assert!(p.container.is_none());
        assert!(p.tty);
    }
}
```

`services/k8s/src/service.rs` 追加测试模块：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn exec_pod_rejects_empty_stream() {
        let svc = K8sServiceImpl { manager: ClusterManager::new() };
        let req = Request::new(tonic::Streaming::new(Box::pin(
            tokio_stream::empty::<Result<superops_protos::k8s::v1::ExecRequest, Status>>(),
        )));
        let err = svc.exec_pod(req).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn exec_pod_unknown_cluster_is_not_found() {
        let svc = K8sServiceImpl { manager: ClusterManager::new() };
        let req = Request::new(tonic::Streaming::new(Box::pin(
            tokio_stream::iter(vec![Ok(superops_protos::k8s::v1::ExecRequest {
                cluster_id: "nope".into(),
                ..Default::default()
            })]),
        )));
        let err = svc.exec_pod(req).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::NotFound);
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cd /home/wwwroot/super-ops && cargo test -p superops-k8s 2>&1 | tail -20`
Expected: `exec_pod` 返回 Unimplemented → `exec_pod_unknown_cluster_is_not_found` FAIL（错误码不匹配）。

- [ ] **Step 3: 实现 exec_pod 双向流**

`services/k8s/src/resource/mod.rs`：`pub mod deploy;` 行后加 `pub mod exec;`。

`services/k8s/src/service.rs` 修改：
- imports 加：`use tokio::io::{AsyncReadExt, AsyncWriteExt};`
- 替换 `exec_pod` 方法（原 292-299 行）：

```rust
    type ExecPodStream = ReceiverStream<Result<superops_protos::k8s::v1::ExecResponse, Status>>;

    async fn exec_pod(
        &self,
        request: Request<tonic::Streaming<superops_protos::k8s::v1::ExecRequest>>,
    ) -> GrpcResult<Self::ExecPodStream> {
        let mut in_stream = request.into_inner();
        let first = in_stream
            .message()
            .await
            .map_err(|e| Status::internal(format!("read exec request: {e}")))?
            .ok_or_else(|| Status::invalid_argument("exec stream must start with a connect message"))?;
        let client = self
            .manager
            .get(&first.cluster_id)
            .map_err(|e| Status::not_found(e.to_string()))?;
        let container = if first.container.is_empty() {
            None
        } else {
            Some(first.container.clone())
        };

        let mut attached = resource::exec::exec_pod(
            &client,
            first.namespace.clone(),
            first.pod_name.clone(),
            container,
            &first.command,
        )
        .await
        .map_err(|e| Status::internal(format!("exec failed: {e}")))?;
        let mut stdin = attached.stdin();
        let mut stdout = attached.stdout();
        let mut stderr = attached.stderr();
        let mut resize = attached.terminal_size();

        let (tx, rx) =
            mpsc::channel::<Result<superops_protos::k8s::v1::ExecResponse, Status>>(128);

        // outbound stdout → client
        let out_tx = tx.clone();
        tokio::spawn(async move {
            let mut reader = match stdout {
                Some(r) => r,
                None => return,
            };
            let mut buf = vec![0u8; 8192];
            loop {
                match reader.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if out_tx
                            .send(Ok(superops_protos::k8s::v1::ExecResponse {
                                stdout: buf[..n].to_vec(),
                                stderr: Vec::new(),
                            }))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
        });

        // outbound stderr → client
        let out_tx2 = tx.clone();
        tokio::spawn(async move {
            let mut reader = match stderr {
                Some(r) => r,
                None => return,
            };
            let mut buf = vec![0u8; 8192];
            loop {
                match reader.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if out_tx2
                            .send(Ok(superops_protos::k8s::v1::ExecResponse {
                                stdout: Vec::new(),
                                stderr: buf[..n].to_vec(),
                            }))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            }
        });

        // inbound: client → stdin / resize
        tokio::spawn(async move {
            let mut writer = stdin;
            let mut resize = resize;
            if !first.stdin.is_empty() {
                if let Some(w) = writer.as_mut() {
                    if w.write_all(&first.stdin).await.is_err() {
                        return;
                    }
                }
            }
            while let Some(msg) = in_stream.next().await {
                let Ok(msg) = msg else { break };
                if let Some(ws) = resize.as_mut() {
                    if let Some(ts) = msg.terminal_size {
                        if ws
                            .send(kube::api::TerminalSize {
                                height: ts.height as u32,
                                width: ts.width as u32,
                            })
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                }
                if !msg.stdin.is_empty() {
                    if let Some(w) = writer.as_mut() {
                        if w.write_all(&msg.stdin).await.is_err() {
                            return;
                        }
                    }
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cd /home/wwwroot/super-ops && cargo test -p superops-k8s 2>&1 | tail -15`
Expected: 全部通过（含新增 2 个 exec 测试：InvalidArgument + NotFound），无编译错误。

- [ ] **Step 5: Commit（仅当用户要求）**

```bash
git add services/k8s/src/resource/exec.rs services/k8s/src/resource/mod.rs services/k8s/src/service.rs
git commit -m "feat(k8s): implement exec_pod bidirectional stream"
```

---

### Task 2: gateway WS 终端桥（auth_middleware query token 回退 + exec_pod_ws 真实现）

**Files:**
- Modify: `services/gateway/Cargo.toml`（axum ws feature、futures、tokio-stream）
- Modify: `services/gateway/src/auth/middleware.rs`（query token 回退 + 纯函数测试）
- Modify: `services/gateway/src/proxy/k8s_proxy.rs`（exec_pod_ws 真实现）

- [ ] **Step 1: 写失败测试（extract_query_token 纯函数）**

`services/gateway/src/auth/middleware.rs` 追加：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn uri(s: &str) -> axum::http::Uri {
        s.parse().unwrap()
    }

    #[test]
    fn extract_query_token_reads_token_param() {
        assert_eq!(
            extract_query_token(&uri("/exec?token=abc.def.ghi")),
            Some("abc.def.ghi".to_string())
        );
    }

    #[test]
    fn extract_query_token_ignores_other_params() {
        assert_eq!(
            extract_query_token(&uri("/exec?container=x&token=tok")),
            Some("tok".to_string())
        );
        assert_eq!(extract_query_token(&uri("/exec?container=x")), None);
    }

    #[test]
    fn extract_query_token_handles_no_query() {
        assert_eq!(extract_query_token(&uri("/exec")), None);
    }
}
```

（`extract_query_token` 尚不存在 → 编译失败即测试失败。）

- [ ] **Step 2: 运行测试确认失败**

Run: `cd /home/wwwroot/super-ops && cargo test -p superops-gateway auth::middleware 2>&1 | tail -10`
Expected: 编译错误 `cannot find function extract_query_token`。

- [ ] **Step 3: 实现 query token 回退 + 修改 Cargo.toml**

`services/gateway/Cargo.toml` 修改三行：
```toml
axum = { workspace = true, features = ["ws"] }
futures = "0.3"
tokio-stream = "0.1"
```
（替换原 `axum.workspace = true` 行，后两行加在 `tokio.workspace = true` 附近。）

`services/gateway/src/auth/middleware.rs` 修改 auth_middleware 顶部（保留原 Bearer 逻辑，在 token 提取链加回退）：

```rust
pub async fn auth_middleware(
    State(state): State<AppState>,
    mut req: Request<Body>,
    next: Next,
) -> Result<Response, Response> {
    let token = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .or_else(|| {
            // 浏览器 WebSocket 无法自定义 Authorization header，回退到 query token。
            // JWT 是 base64url 字符集（无 '+','/'），可直接用于 query。
            tracing::warn!("auth via query token — tokens in URLs can leak through logs");
            extract_query_token(req.uri())
        });
    let Some(token) = token else {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "missing authorization token"})),
        )
            .into_response());
    };

    let claims = state.auth.verify_token(token).map_err(|e| {
        tracing::warn!("auth rejected: {e}");
        (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "invalid or expired token"})),
        )
            .into_response()
    })?;

    req.extensions_mut().insert(claims);
    Ok(next.run(req).await)
}

fn extract_query_token(uri: &axum::http::Uri) -> Option<String> {
    let query = uri.query()?;
    for pair in query.split('&') {
        let mut it = pair.splitn(2, '=');
        if it.next() == Some("token") {
            let v = it.next().unwrap_or_default();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cd /home/wwwroot/super-ops && cargo test -p superops-gateway auth::middleware 2>&1 | tail -10`
Expected: 3 个测试全过，编译通过。

- [ ] **Step 5: 实现 exec_pod_ws 桥**

`services/gateway/src/proxy/k8s_proxy.rs`：
- imports 加：

```rust
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use futures::{SinkExt, StreamExt};
use superops_protos::k8s::v1::ExecRequest;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
```

- 替换 `exec_pod_ws`（原 91-97 行）与 `PodListQuery` 后加 query 结构：

```rust
#[derive(Debug, Deserialize)]
struct ExecQuery {
    container: Option<String>,
    command: Option<String>,
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

    let mut client = match superops_protos::k8s::v1::k8s_service_client::K8sServiceClient::connect(
        endpoint,
    )
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
```

- [ ] **Step 6: 编译验证**

Run: `cd /home/wwwroot/super-ops && cargo build -p superops-gateway 2>&1 | tail -5`
Expected: 编译成功（无警告）。若报 `ExecRequest` 字段名/Default 问题，按编译错误修正。

---

### Task 3: 前端终端页打通（token query + binaryType + container）

**Files:**
- Modify: `frontend/src/pages/k8s/terminal.tsx`

- [ ] **Step 1: 修改 terminal.tsx**

顶部 import 加：

```tsx
import { useAuthStore } from '../../stores/auth';
```

`connect` 函数修改（原 23-35 行）：

```tsx
  const connect = (v: { cluster: string; namespace: string; pod: string; container?: string }) => {
    wsRef.current?.close();
    termRef.current?.dispose();

    const term = new Terminal({ fontSize: 14, fontFamily: 'Menlo,monospace', cursorBlink: true, theme: { background: '#1a1a2e', foreground: '#e0e0e0' } });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.loadAddon(new WebLinksAddon());
    termRef.current = term;
    if (ref.current) { term.open(ref.current); fit.fit(); }

    const token = useAuthStore.getState().token;
    const container = v.container ? `&container=${encodeURIComponent(v.container)}` : '';
    const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
    const ws = new WebSocket(`${proto}//${location.host}/api/k8s/clusters/${v.cluster}/pods/${v.namespace}/${v.pod}/exec?token=${encodeURIComponent(token || '')}${container}`);
    // xterm 接收二进制更快且不受 utf-8 拆分影响
    ws.binaryType = 'arraybuffer';
    wsRef.current = ws;
```

Form 加 container 项（原 63 行 `Form.Item name="pod"` 后）：

```tsx
          <Form.Item name="container" rules={[{ required: false }]}><Input placeholder="容器（可选）" style={{ width: 160 }} /></Form.Item>
```

- [ ] **Step 2: 构建验证**

Run: `cd /home/wwwroot/super-ops/frontend && npm run build 2>&1 | tail -15`
Expected: `tsc && vite build` 成功（tsc 无类型错误）。

- [ ] **Step 3: 运行时验证（需 compose 全栈 + 真实集群，交给用户）**

```bash
! cd /home/wwwroot/super-ops && sudo docker compose -f deploy/docker-compose.yml up -d
```
验证路径：登录 → 添加集群（kubeconfig）→ 终端页输入 cluster/namespace/pod → 连接 → 输入命令有回显。

---

### Task 4: API Key（表 + 存储 + CRUD + X-API-Key 鉴权）

**Files:**
- Modify: `deploy/init.sql`（api_keys 表）
- Modify: `services/gateway/Cargo.toml`（sha2）
- Create: `services/gateway/src/model/api_key.rs`
- Create: `services/gateway/src/auth/apikey.rs`
- Modify: `services/gateway/src/model/mod.rs`（加 `pub mod api_key;`）与 `services/gateway/src/auth/mod.rs`（加 `pub mod apikey;`）
- Modify: `services/gateway/src/auth/middleware.rs`（X-API-Key 分支）
- Modify: `services/gateway/src/main.rs`（AppState.api_keys + keys 路由 + load）

- [ ] **Step 1: 写失败测试（hash_key / generate_key 纯函数）**

`services/gateway/src/model/api_key.rs` 完整内容（含实现与测试）：

```rust
use anyhow::Result;
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::mysql::MySqlPool;
use sqlx::FromRow;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub fn generate_key() -> String {
    format!("sk_{}", uuid::Uuid::new_v4().simple())
}

pub fn hash_key(plain: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(plain.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[derive(Debug, Serialize, FromRow)]
pub struct ApiKeyMeta {
    pub id: String,
    pub name: String,
}

#[derive(Clone)]
pub struct ApiKeyStore {
    pool: MySqlPool,
    keys: Arc<RwLock<HashMap<String, String>>>, // key_hash -> user_id
}

impl ApiKeyStore {
    pub fn new(pool: MySqlPool) -> Self {
        Self { pool, keys: Arc::new(RwLock::new(HashMap::new())) }
    }

    pub async fn load(&self) -> Result<()> {
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT key_hash, user_id FROM api_keys")
                .fetch_all(&self.pool)
                .await?;
        let mut map = self.keys.write().unwrap();
        map.clear();
        for (hash, uid) in rows {
            map.insert(hash, uid);
        }
        Ok(())
    }

    pub fn lookup(&self, hash: &str) -> Option<String> {
        self.keys.read().unwrap().get(hash).cloned()
    }

    pub async fn create(&self, user_id: &str, name: &str) -> Result<(String, String)> {
        let id = uuid::Uuid::new_v4().to_string();
        let plain = generate_key();
        let hash = hash_key(&plain);
        sqlx::query("INSERT INTO api_keys (id, user_id, name, key_hash) VALUES (?, ?, ?, ?)")
            .bind(&id)
            .bind(user_id)
            .bind(name)
            .bind(&hash)
            .execute(&self.pool)
            .await?;
        self.keys.write().unwrap().insert(hash, user_id.to_string());
        Ok((id, plain))
    }

    pub async fn delete(&self, id: &str, user_id: &str) -> Result<bool> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT key_hash FROM api_keys WHERE id = ? AND user_id = ?")
                .bind(id)
                .bind(user_id)
                .fetch_optional(&self.pool)
                .await?;
        sqlx::query("DELETE FROM api_keys WHERE id = ? AND user_id = ?")
            .bind(id)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        if let Some((hash,)) = row {
            self.keys.write().unwrap().remove(&hash);
        }
        Ok(row.is_some())
    }

    pub async fn list(&self, user_id: &str) -> Result<Vec<ApiKeyMeta>> {
        Ok(sqlx::query_as::<_, ApiKeyMeta>(
            "SELECT id, name FROM api_keys WHERE user_id = ?",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_key_has_prefix_and_length() {
        let key = generate_key();
        assert!(key.starts_with("sk_"));
        assert_eq!(key.len(), 3 + 32);
    }

    #[test]
    fn hash_key_is_stable_sha256_hex() {
        // sha256("abc") = ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
        assert_eq!(
            hash_key("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(hash_key("abc"), hash_key("abc"));
        assert_ne!(hash_key("abc"), hash_key("abd"));
    }
}
```

- [ ] **Step 2: 运行测试确认通过**

Run: `cd /home/wwwroot/super-ops && cargo test -p superops-gateway model::api_key 2>&1 | tail -10`
Expected: 先注册 `mod api_key`（model/mod.rs）否则编译失败；注册后 2 测试通过。

- [ ] **Step 3: 建表 + 依赖 + 注册模块**

`deploy/init.sql` 追加：

```sql
CREATE TABLE IF NOT EXISTS api_keys (
    id VARCHAR(36) PRIMARY KEY,
    user_id VARCHAR(36) NOT NULL,
    name VARCHAR(64) NOT NULL,
    key_hash CHAR(64) NOT NULL UNIQUE,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
CREATE INDEX idx_api_keys_user ON api_keys(user_id);
```

`services/gateway/Cargo.toml` 加 `sha2 = "0.10"`。

`services/gateway/src/model/mod.rs` 加 `pub mod api_key;`；`services/gateway/src/auth/mod.rs` 加 `pub mod apikey;`（若 auth/mod.rs 用 mod 声明则照现状追加）。

- [ ] **Step 4: 鉴权分支 + handler + 路由**

`services/gateway/src/auth/middleware.rs`——在 `let token = ...` 提取之前插入 API key 分支（优先级：Bearer/query JWT → X-API-Key；OAuth2 短路在 T5 加）：

```rust
    // X-API-Key 鉴权（内存表 hash 查询；吊销立即失效）
    if let Some(key) = req
        .headers()
        .get("X-API-Key")
        .and_then(|v| v.to_str().ok())
        .filter(|k| !k.is_empty())
    {
        let hash = crate::model::api_key::hash_key(key);
        if let Some(user_id) = state.api_keys.lookup(&hash) {
            let claims = Claims {
                sub: user_id,
                username: "api-key".into(),
                exp: 0,
                iat: 0,
            };
            req.extensions_mut().insert(claims);
            return Ok(next.run(req).await);
        }
    }
```

`services/gateway/src/auth/apikey.rs`（新建）：

```rust
use crate::AppState;
use crate::auth::middleware::Claims;
use axum::{
    Json,
    extract::{Extension, Path, State},
    http::StatusCode,
};
use serde_json::Value;

type ApiError = (StatusCode, Json<Value>);

fn err(status: StatusCode, message: &str) -> ApiError {
    (status, Json(serde_json::json!({ "error": message })))
}

pub async fn create_key(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let name = body
        .get("name")
        .and_then(|n| n.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| "default".into());
    if name.len() > 64 {
        return Err(err(StatusCode::BAD_REQUEST, "name must be <= 64 chars"));
    }
    let (id, plain) = state
        .api_keys
        .create(&claims.sub, &name)
        .await
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "failed to create api key"))?;
    Ok(Json(serde_json::json!({
        "id": id,
        "name": name,
        "key": plain,
        "warning": "store this key now; it will not be shown again"
    })))
}

pub async fn list_keys(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, ApiError> {
    let keys = state
        .api_keys
        .list(&claims.sub)
        .await
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "failed to list api keys"))?;
    Ok(Json(serde_json::json!({ "keys": keys })))
}

pub async fn delete_key(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let deleted = state
        .api_keys
        .delete(&id, &claims.sub)
        .await
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "failed to delete api key"))?;
    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(err(StatusCode::NOT_FOUND, "api key not found"))
    }
}
```

`services/gateway/src/main.rs`：
- AppState 加字段：`pub api_keys: Arc<crate::model::api_key::ApiKeyStore>,`
- state 构建加：`api_keys: Arc::new(crate::model::api_key::ApiKeyStore::new(pool.clone())),`
- state 构建后启动加载（`let state = AppState {...};` 之后）：

```rust
    let api_keys = Arc::clone(&state.api_keys);
    tokio::spawn(async move {
        if let Err(e) = api_keys.load().await {
            tracing::warn!(error = %e, "api_keys load failed; run deploy/init.sql");
        }
    });
```

- keys 路由（k8s 链后，同链）：

```rust
    let keys = axum::Router::new()
        .route(
            "/api/keys",
            axum::routing::get(crate::auth::apikey::list_keys)
                .post(crate::auth::apikey::create_key),
        )
        .route("/api/keys/{id}", axum::routing::delete(crate::auth::apikey::delete_key))
        .layer(middleware::from_fn_with_state(state.clone(), auth_middleware))
        .layer(
            tower::ServiceBuilder::new()
                .layer(crate::breaker::ErrorToResponseLayer)
                .layer(crate::breaker::breaker_layer())
                .layer(crate::breaker::FiveXxToErrorLayer),
        );
```

- `Router::new()` 加 `.merge(keys)`。

- [ ] **Step 5: 编译 + 测试**

Run: `cd /home/wwwroot/super-ops && cargo test -p superops-gateway 2>&1 | tail -15`
Expected: 全过（新增 2 个 api_key 纯函数测试），编译成功。

---

### Task 5: OAuth2 条件挂载（默认关闭）

**Files:**
- Modify: `services/gateway/Cargo.toml`（ecat-auth）
- Modify: `services/gateway/src/config.rs`（OAuth2Config + 测试）
- Modify: `services/gateway/src/auth/middleware.rs`（AuthClaims 短路）
- Modify: `services/gateway/src/main.rs`（条件挂载）

- [ ] **Step 1: 写失败测试（config 解析）**

`services/gateway/src/config.rs` 追加：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn base_yaml() -> &'static str {
        "server:\n  http_port: 8080\n  grpc_port: 9090\nauth:\n  jwt_secret: x\n  access_token_ttl: 1\n  refresh_token_ttl: 2\ndatabase:\n  url: mysql://u:p@h/d\nredis:\n  url: redis://h\nservices:\n  k8s:\n    endpoint: http://h\nch:\n  base_url: http://h\n  database: d\n"
    }

    #[test]
    fn oauth2_defaults_to_none() {
        let cfg: Config = serde_yaml::from_str(base_yaml()).unwrap();
        assert!(cfg.oauth2.is_none());
    }

    #[test]
    fn oauth2_parses_when_present() {
        let yaml = format!(
            "{}oauth2:\n  introspection_url: https://idp/oauth/introspect\n  client_id: cid\n  client_secret: secret\n",
            base_yaml()
        );
        let cfg: Config = serde_yaml::from_str(&yaml).unwrap();
        let o = cfg.oauth2.unwrap();
        assert_eq!(o.introspection_url, "https://idp/oauth/introspect");
        assert_eq!(o.client_id, "cid");
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cd /home/wwwroot/super-ops && cargo test -p superops-gateway config 2>&1 | tail -10`
Expected: 编译错误 `no field oauth2`。

- [ ] **Step 3: 实现 config + 依赖 + 短路 + 挂载**

`services/gateway/src/config.rs`：
- `Config` 加字段（otlp 后）：`#[serde(default)] pub oauth2: Option<OAuth2Config>,`
- 新 struct：

```rust
#[derive(Debug, Deserialize, Clone)]
pub struct OAuth2Config {
    pub introspection_url: String,
    pub client_id: String,
    pub client_secret: String,
}
```

`services/gateway/Cargo.toml` 加 `ecat-auth = { path = "../../ecat-auth" }`。

`services/gateway/src/auth/middleware.rs` 顶部加短路（auth_middleware 函数第一行）：

```rust
    // OAuth2 层已把 AuthClaims 放入 extensions（ecat_auth::OAuth2Layer）→ 直接放行
    if req.extensions().get::<ecat_auth::AuthClaims>().is_some() {
        return Ok(next.run(req).await);
    }
```

`services/gateway/src/main.rs`——k8s 链构建后追加：

```rust
    let k8s = match &config.oauth2 {
        Some(oauth2) => k8s
            .layer(
                ecat_auth::OAuth2Layer::new(
                    &oauth2.introspection_url,
                    &oauth2.client_id,
                    &oauth2.client_secret,
                )
                .map_err(|e| anyhow::anyhow!("oauth2 config: {e}"))?,
            )
            .layer(crate::breaker::ErrorToResponseLayer),
        None => k8s,
    };
```

注意：OAuth2Layer 鉴权失败返回 401 Response（非 Err），`ErrorToResponseLayer` 只兜 inner Err——层序正确。`config/gateway.yaml` **不加** oauth2（保持默认关闭）。

- [ ] **Step 4: 运行测试确认通过**

Run: `cd /home/wwwroot/super-ops && cargo test -p superops-gateway config 2>&1 | tail -10 && cargo build -p superops-gateway 2>&1 | tail -5`
Expected: config 2 测试通过，编译成功（OAuth2 挂载为条件分支，默认路径不变）。

---

### Task 6: OpenAPI 文档（/api/docs）

**Files:**
- Modify: `services/gateway/Cargo.toml`（ecat-openapi）
- Create: `services/gateway/src/openapi.rs`
- Modify: `services/gateway/src/main.rs`（`mod openapi;` + 路由）

- [ ] **Step 1: 写失败测试 + 实现（openapi.rs 完整内容）**

`services/gateway/src/openapi.rs`：

```rust
use ecat_openapi::{OpenApiBuilder, OpenApiSpec, string_schema};
use std::collections::HashMap;

pub fn build_spec() -> OpenApiSpec {
    OpenApiBuilder::new("SuperOps Gateway API", env!("CARGO_PKG_VERSION"))
        .add_route("/api/health", "GET", "Health check", vec!["system".into()])
        .add_route("/api/auth/register", "POST", "Register a new user", vec!["auth".into()])
        .add_route("/api/auth/login", "POST", "Login and get tokens", vec!["auth".into()])
        .add_route("/api/k8s/clusters", "GET", "List clusters", vec!["k8s".into()])
        .add_route("/api/k8s/clusters/{cluster_id}/pods", "GET", "List pods", vec!["k8s".into()])
        .add_route("/api/k8s/clusters/{cluster_id}/deployments", "GET", "List deployments", vec!["k8s".into()])
        .add_route("/api/k8s/clusters/{cluster_id}/nodes", "GET", "List nodes", vec!["k8s".into()])
        .add_route("/api/k8s/clusters/{cluster_id}/metrics", "GET", "Cluster metrics", vec!["k8s".into()])
        .add_route("/api/keys", "GET", "List API keys", vec!["auth".into()])
        .add_route("/api/keys", "POST", "Create API key", vec!["auth".into()])
        .add_route("/api/keys/{id}", "DELETE", "Delete API key", vec!["auth".into()])
        .add_route("/api/docs", "GET", "This OpenAPI document", vec!["system".into()])
        .add_schema(
            "LoginRequest",
            HashMap::from([
                ("username".into(), string_schema()),
                ("password".into(), string_schema()),
            ]),
        )
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_is_openapi_3_0_3() {
        let spec = build_spec();
        assert_eq!(spec.openapi, "3.0.3");
        assert_eq!(spec.info.title, "SuperOps Gateway API");
    }

    #[test]
    fn spec_covers_core_routes() {
        let spec = build_spec();
        assert!(spec.paths.contains_key("/api/auth/login"));
        assert!(spec.paths.contains_key("/api/k8s/clusters"));
        let login = spec.paths.get("/api/auth/login").unwrap();
        assert!(login.post.is_some());
        let docs = spec.paths.get("/api/docs").unwrap();
        assert!(docs.get.is_some());
    }
}
```

- [ ] **Step 2: 运行测试确认通过**

Run: `cd /home/wwwroot/super-ops && cargo test -p superops-gateway openapi 2>&1 | tail -10`
Expected: 编译失败（`mod openapi` 未注册 + ecat-openapi 未加依赖）→ 先注册 mod、加依赖，再跑 2 测试通过。

- [ ] **Step 3: 接线路由**

`services/gateway/Cargo.toml` 加 `ecat-openapi = { path = "../../ecat-openapi" }`。

`services/gateway/src/main.rs`：`mod proxy;` 后加 `mod openapi;`；`Router::new()` 加 `.route("/api/docs", get(openapi::docs))`；末尾加：

```rust
async fn docs() -> Json<ecat_openapi::OpenApiSpec> {
    Json(crate::openapi::build_spec())
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cd /home/wwwroot/super-ops && cargo test -p superops-gateway openapi 2>&1 | tail -10`
Expected: 2 测试通过，编译成功。

---

### Task 7: bench workspace 成员（login 压测）

**Files:**
- Modify: `Cargo.toml`（workspace members 加 "bench"）
- Create: `bench/Cargo.toml`
- Create: `bench/src/main.rs`

- [ ] **Step 1: 写 bench 程序**

`Cargo.toml` members 末尾（"services/collector" 后）加 `"bench",`。

`bench/Cargo.toml`：

```toml
[package]
name = "bench"
version.workspace = true
edition.workspace = true
license.workspace = true
description = "SuperOps load benchmarks"

[dependencies]
ecat-bench = { path = "../ecat-bench" }
tokio.workspace = true
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "json"] }
serde_json.workspace = true
anyhow = "1"
```

`bench/src/main.rs`：

```rust
use ecat_bench::run_bench;

const DEFAULT_URL: &str = "http://localhost:8080/api/auth/login";

async fn login_once(client: &reqwest::Client, url: &str) {
    let resp = client
        .post(url)
        .json(&serde_json::json!({"username": "bench", "password": "bench-bench"}))
        .send()
        .await;
    match resp {
        Ok(r) if r.status().is_success() => {}
        Ok(r) => eprintln!("login returned {}", r.status()),
        Err(e) => eprintln!("login request failed: {e}"),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let url = std::env::var("BENCH_GATEWAY_URL").unwrap_or_else(|_| DEFAULT_URL.into());
    let client = reqwest::Client::new();
    let result = run_bench("login", 10, 500, || {
        let client = client.clone();
        let url = url.clone();
        async move { login_once(&client, &url).await }
    })
    .await;
    result.print();
    Ok(())
}
```

- [ ] **Step 2: 编译验证**

Run: `cd /home/wwwroot/super-ops && cargo build -p bench 2>&1 | tail -5`
Expected: 编译成功。

- [ ] **Step 3: 运行验证（需 gateway 在线，可选）**

Run: `cd /home/wwwroot/super-ops && cargo run --release -p bench 2>&1 | tail -8`
Expected: `=== login ===` + requests/duration/throughput/p50/p99。gateway 未运行时仍打印（错误计入延迟）。

---

### Task 8: 收尾（CHANGELOG、总纲勾选、stretch 评估、全量验证）

**Files:**
- Modify: `CHANGELOG.md`
- Modify: `docs/superpowers/plans/2026-08-06-super-ops-ecosystem-expansion.md`（总纲 P3 复选框 + stretch 结论）

- [ ] **Step 1: CHANGELOG [1.2.0]**

`CHANGELOG.md` 顶部插入：

```markdown
## [1.2.0] - 2026-08-06

### Added
- k8s exec 双向流（ExecPod gRPC → kube exec，stdin/stdout/stderr/terminal resize）
- Gateway WebSocket 终端桥（/api/k8s/.../exec，浏览器 WS ↔ gRPC 双向流）
- 前端终端页打通（token query 鉴权、binaryType、可选容器）
- API Key 管理（表 + CRUD + X-API-Key 鉴权，吊销即时生效）
- OAuth2 可选鉴权层（ecat-auth OAuth2Layer，默认关闭）
- OpenAPI 文档（/api/docs，ecat-openapi）
- bench workspace 成员（login 压测，cargo run -p bench）

### Changed
- auth_middleware 支持三种凭据：Bearer JWT → query token（WS 回退）→ X-API-Key
- gateway 依赖：axum ws feature、futures、tokio-stream、sha2、ecat-auth、ecat-openapi
```

- [ ] **Step 2: 总纲 P3 复选框 + stretch 结论**

Run: `grep -n "P3" /home/wwwroot/super-ops/docs/superpowers/plans/2026-08-06-super-ops-ecosystem-expansion.md | head` 定位 P3 行号范围，翻转对应 `- [ ]` → `- [x]`（同 P2 的 sed 方式）。

stretch 评估结论（总纲 stretch 小节追加）：

```markdown
**评估结论（2026-08-06）：** ecat-graphql 与 ecat-versioning 不进入实现——GraphQL 与现有 REST + OpenAPI 体系重复且增加双层 schema 维护成本；versioning 在单一 gateway 且内部服务受控的前提下收益低。待出现多版本 API 共存或外部消费者需求时再评估。
```

- [ ] **Step 3: 全量验证**

Run: `cd /home/wwwroot/super-ops && cargo fmt && cargo test --workspace 2>&1 | tail -10 && cargo build --workspace 2>&1 | tail -5`
Expected: fmt 无差异、全部测试通过、workspace 编译成功。

- [ ] **Step 4: 交付说明**

向用户输出：P3 完成清单、运行时复核清单（exec 终端端到端、X-API-Key 创建/吊销、OAuth2 开启后鉴权、/api/docs、cargo run -p bench、前端 npm run build）、待用户运行的 compose 命令。

---

## Self-Review

**总纲 P3 覆盖检查：**
- P3.1 exec_pod gRPC → Task 1 ✓
- P3.2 WS 终端桥 → Task 2 ✓
- P3.3 前端终端 → Task 3 ✓
- P3.4 API Key → Task 4 ✓
- P3.5 OAuth2（默认关闭）→ Task 5 ✓
- P3.6 OpenAPI → Task 6 ✓
- P3.7 bench → Task 7 ✓
- P3.8 stretch 评估 → Task 8 Step 2 ✓

**类型一致性：** `extract_query_token` 在 T2 定义、T3 前端用 query `token` 参数名一致；`hash_key`/`generate_key` 在 T4 定义与使用一致；`Claims` 复用现有类型；`ExecRequest{stdin, terminal_size, ..Default}` 与 proto 字段一致；`AuthClaims` 短路在 T5 定义并只被 main.rs 挂载使用。

**已知边界（诚实记录）：** 数据库相关路径（ApiKeyStore create/delete/load、exec 全链路）无法在无 MySQL/集群环境单测，靠编译 + 运行时清单验证；前端无测试框架，验证为 `npm run build`。
