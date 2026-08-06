# SuperOps 生态缺口补齐 Implementation Plan（P4–P6 全量）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 补齐生态缺口矩阵全部 14 域：k8s 写操作、告警通知、审计/API Key/用户页、日志中心、CMDB、批量执行、RBAC、审批流、多租户、凭据保险库、备份/容量/成本、终端录制/文件传输、gRPC 调度追踪、前端指标/告警页。

**Architecture:** 前端（React + Vite）→ Gateway（axum，认证/限流/熔断/代理）→ K8s Service（tonic gRPC，写操作经 kube 客户端）+ Collector（ecat-scheduler 周期任务，写 ClickHouse/通知）；MySQL 存业务实体（用户/资产/脚本/审批/凭据），Redis 存去重与 ack 状态，ClickHouse 存时序与日志，Kafka 承载审计事件。所有新后端代码沿用现有模式（`manager.get → resource 模块`、`k8s_routes() → handler`、`DataPoint/TsdbClient` 写入）。

**Tech Stack:** Rust workspace（axum / tonic / kube / sqlx / ecat-* 组件）、protobuf（buf + `make proto`）、React + antd + @ant-design/pro-components、ClickHouse / MySQL / Redis / Kafka / MinIO（P6-5 新增）。

**硬性约束（用户指令，优先级最高）：**
- **不做任何打包**：不构建 docker 镜像、不 push 镜像、不发布 crate。Helm chart 仅交付 chart 文件。
- **未经明确要求不 commit**（本计划的 commit 步骤全部为「可选，仅在用户要求时执行」）。
- 文件 <500 行；系统边界输入校验；编辑前先 Read；改动后跑 `cargo test` / `cargo fmt` / `npm run build`。
- 不提交 secrets / .env。

**执行顺序：** 本文件 = 总览 + **P4 快速获胜（可独立交付）**；P5 见 `2026-08-06-ecosystem-gap-completion-p5.md`；P6 见 `2026-08-06-ecosystem-gap-completion-p6.md`。每个阶段产出可独立测试、独立验证的软件。

---

## 缺口矩阵与阶段路线（回顾）

| # | 域 | 缺口 | 阶段 | 主要文件 |
|---|---|---|---|---|
| 1 | k8s 写操作 | 只有读查询；无 scale/restart/delete | P4 | protos/k8s/v1/k8s.proto、services/k8s/src/resource/write.rs、gateway proxy、frontend deployments.tsx |
| 2 | 告警通知 | 告警仅落 ClickHouse，无推送 | P4 | services/collector/src/notify.rs、config/collector.yaml |
| 3 | 审计/API Key/用户页 | 后端部分存在，前端无页面 | P4 | gateway audit_api.rs、model/user.rs、frontend pages/ops/* |
| 4 | 日志中心 | 无跨 Pod 日志检索 | P5 | collector logtail.rs、gateway logs API、frontend /ops/logs |
| 5 | CMDB 资产 | dashboard 硬编码 0 占位 | P5 | deploy/init.sql、model/cmdb.rs、frontend /cmdb |
| 6 | 批量执行/脚本库 | 无 | P5 | script 表、k8s Job RPC、frontend /ops/scripts |
| 7 | RBAC 细化 | 仅登录校验，无角色控制 | P5 | gateway middleware、JWT claims |
| 8 | 审批工作流 | 无 | P6 | approval 表、gateway API、frontend /ops/approvals |
| 9 | 多租户 | 无 | P6 | tenant_id 列、JWT claim、中间件 |
| 10 | 凭据/证书保险库 | 无 | P6 | secret 表（AES-GCM）、/api/secrets |
| 11 | 备份/容量/成本 | 无 | P6 | collector 任务、frontend /ops/capacity |
| 12 | 终端录制/文件传输 | 无 | P6 | exec_session 录制、MinIO + ecat-data-s3 |
| 13 | gRPC/调度 OTLP span | README 已知限制 | P6 | k8s/collector main 初始化 + instrument |
| 14 | 前端指标/告警页 | 无 | P6 | frontend /ops/metrics、/ops/alerts |

**依赖关系：** P4 无前置依赖，先行；P5-3（批量执行）与 P6-1（审批）有自然衔接；P6-2 多租户改动触及 P5 表结构，须在 P5 之后。文档同步（README/SVG/CHANGELOG）在每个阶段末尾执行。

---

# P4 · 快速获胜（本文件）

## P4-1 k8s 写操作（scale / restart / delete Deployment）

### Task 1: proto 增加写操作 RPC

**Files:**
- Modify: `protos/k8s/v1/k8s.proto`

- [ ] **Step 1: 在 k8s.proto 的 service 块追加 3 个 RPC**

在 `rpc GetMetrics(GetMetricsRequest) returns (GetMetricsResponse);` 之后追加：

```proto
  rpc ScaleDeployment(ScaleDeploymentRequest) returns (ScaleDeploymentResponse);
  rpc RestartDeployment(RestartDeploymentRequest) returns (RestartDeploymentResponse);
  rpc DeleteDeployment(DeleteDeploymentRequest) returns (DeleteDeploymentResponse);
```

- [ ] **Step 2: 在文件末尾追加消息定义**

```proto
message ScaleDeploymentRequest {
  string cluster_id = 1;
  string namespace = 2;
  string name = 3;
  int32 replicas = 4;
}
message ScaleDeploymentResponse { int32 replicas = 1; }

message RestartDeploymentRequest {
  string cluster_id = 1;
  string namespace = 2;
  string name = 3;
}
message RestartDeploymentResponse { bool restarted = 1; }

message DeleteDeploymentRequest {
  string cluster_id = 1;
  string namespace = 2;
  string name = 3;
}
message DeleteDeploymentResponse { bool deleted = 1; }
```

- [ ] **Step 3: 重新生成 protobuf 并验证**

Run: `make proto`
Expected: 无报错；随后：

```bash
grep -c "scale_deployment" superops-protos/src/*.rs  # 生成的 client/server trait 均含该方法
cargo check -p superops-protos
```

Expected: `cargo check` 通过。

- [ ] **Step 4（可选，仅在用户要求时）:** `git add protos/k8s/v1/k8s.proto superops-protos && git commit -m "feat(proto): add deployment write RPCs (scale/restart/delete)"`

### Task 2: k8s service — resource/write.rs 实现

**Files:**
- Create: `services/k8s/src/resource/write.rs`
- Modify: `services/k8s/src/resource/mod.rs`

- [ ] **Step 1: 写失败测试（纯校验函数）**

Create `services/k8s/src/resource/write.rs`：

```rust
pub fn validate_scale(cluster_id: &str, namespace: &str, name: &str, replicas: i32) -> anyhow::Result<()> {
    if cluster_id.is_empty() || namespace.is_empty() || name.is_empty() {
        return Err(anyhow::anyhow!("cluster_id/namespace/name must not be empty"));
    }
    if !(0..=1000).contains(&replicas) {
        return Err(anyhow::anyhow!("replicas must be in 0..=1000, got {replicas}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_scale_rejects_empty_fields() {
        assert!(validate_scale("", "ns", "name", 2).is_err());
        assert!(validate_scale("c", "", "name", 2).is_err());
        assert!(validate_scale("c", "ns", "", 2).is_err());
    }

    #[test]
    fn validate_scale_rejects_out_of_range_replicas() {
        assert!(validate_scale("c", "ns", "name", -1).is_err());
        assert!(validate_scale("c", "ns", "name", 1001).is_err());
        assert!(validate_scale("c", "ns", "name", 0).is_ok());
        assert!(validate_scale("c", "ns", "name", 1000).is_ok());
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p superops-k8s-service write::tests 2>&1 | tail -5`
Expected: 编译失败（`write` 模块未注册）。

- [ ] **Step 3: 注册模块并补全实现**

Modify `services/k8s/src/resource/mod.rs`：`pub mod write;`

在 write.rs 中追加（`ClusterClient` 的 `client` 字段为 `kube::Client`，见 `cluster/client.rs`）：

```rust
use crate::cluster::client::ClusterClient;
use kube::api::{Api, DeleteParams, Patch, PatchParams, ReplaceParams};

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

type Deployment = k8s_openapi::api::apps::v1::Deployment;

pub async fn scale_deployment(client: &ClusterClient, namespace: &str, name: &str, replicas: i32) -> anyhow::Result<i32> {
    let api = Api::<Deployment>::namespaced(client.client.clone(), namespace);
    let mut dep = api.get(name).await?;
    let spec = dep
        .spec
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("deployment {namespace}/{name} has no spec"))?;
    spec.replicas = Some(replicas);
    api.replace(name, &ReplaceParams::default(), &dep).await?;
    Ok(replicas)
}

pub async fn restart_deployment(client: &ClusterClient, namespace: &str, name: &str) -> anyhow::Result<bool> {
    let api = Api::<Deployment>::namespaced(client.client.clone(), namespace);
    let patch = Patch::Strategic(serde_json::json!({
        "spec": { "template": { "metadata": { "annotations": {
            "kubectl.kubernetes.io/restartedAt": now_secs().to_string()
        } } } }
    }));
    api.patch(name, &PatchParams::default(), &patch).await?;
    Ok(true)
}

pub async fn delete_deployment(client: &ClusterClient, namespace: &str, name: &str) -> anyhow::Result<bool> {
    let api = Api::<Deployment>::namespaced(client.client.clone(), namespace);
    api.delete(name, &DeleteParams::default()).await?;
    Ok(true)
}
```

> 注意：若 `resource/deploy.rs` 中 Deployment 类型的导入写法不同（`grep "Deployment" services/k8s/src/resource/deploy.rs`），以该文件为准统一。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p superops-k8s-service write::tests 2>&1 | tail -5`
Expected: `test result: ok`，2 个测试通过。

- [ ] **Step 5（可选）:** commit `feat(k8s): deployment scale/restart/delete resource ops with validation`

### Task 3: k8s service — service.rs 挂接三个 handler

**Files:**
- Modify: `services/k8s/src/service.rs`

- [ ] **Step 1: 写失败测试**

在 `services/k8s/src/service.rs` 的 `mod tests` 中追加（模式参照现有 `exec_pod_unknown_cluster_is_not_found`）：

```rust
    #[tokio::test]
    async fn scale_deployment_invalid_replicas_is_invalid_argument() {
        let svc = K8sServiceImpl { manager: ClusterManager::new() };
        let req = Request::new(superops_protos::k8s::v1::ScaleDeploymentRequest {
            cluster_id: "c".into(),
            namespace: "ns".into(),
            name: "d".into(),
            replicas: 2000,
        });
        let err = svc.scale_deployment(req).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn scale_deployment_unknown_cluster_is_not_found() {
        let svc = K8sServiceImpl { manager: ClusterManager::new() };
        let req = Request::new(superops_protos::k8s::v1::ScaleDeploymentRequest {
            cluster_id: "nope".into(),
            namespace: "ns".into(),
            name: "d".into(),
            replicas: 3,
        });
        let err = svc.scale_deployment(req).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::NotFound);
    }
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p superops-k8s-service scale_deployment 2>&1 | tail -5`
Expected: 编译失败（trait 未实现该方法）。

- [ ] **Step 3: 实现 handler**

在 `impl K8sService for K8sServiceImpl` 中追加（模式参照 `list_deployments`，`manager.get` 错误映射为 `Status::not_found`）：

```rust
    async fn scale_deployment(
        &self,
        request: Request<superops_protos::k8s::v1::ScaleDeploymentRequest>,
    ) -> GrpcResult<superops_protos::k8s::v1::ScaleDeploymentResponse> {
        let req = request.into_inner();
        resource::write::validate_scale(&req.cluster_id, &req.namespace, &req.name, req.replicas)
            .map_err(|e| Status::invalid_argument(e.to_string()))?;
        let client = self
            .manager
            .get(&req.cluster_id)
            .map_err(|e| Status::not_found(e.to_string()))?;
        let replicas = resource::write::scale_deployment(&client, &req.namespace, &req.name, req.replicas)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(superops_protos::k8s::v1::ScaleDeploymentResponse { replicas }))
    }

    async fn restart_deployment(
        &self,
        request: Request<superops_protos::k8s::v1::RestartDeploymentRequest>,
    ) -> GrpcResult<superops_protos::k8s::v1::RestartDeploymentResponse> {
        let req = request.into_inner();
        if req.cluster_id.is_empty() || req.namespace.is_empty() || req.name.is_empty() {
            return Err(Status::invalid_argument("cluster_id/namespace/name must not be empty"));
        }
        let client = self
            .manager
            .get(&req.cluster_id)
            .map_err(|e| Status::not_found(e.to_string()))?;
        let restarted = resource::write::restart_deployment(&client, &req.namespace, &req.name)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(superops_protos::k8s::v1::RestartDeploymentResponse { restarted }))
    }

    async fn delete_deployment(
        &self,
        request: Request<superops_protos::k8s::v1::DeleteDeploymentRequest>,
    ) -> GrpcResult<superops_protos::k8s::v1::DeleteDeploymentResponse> {
        let req = request.into_inner();
        if req.cluster_id.is_empty() || req.namespace.is_empty() || req.name.is_empty() {
            return Err(Status::invalid_argument("cluster_id/namespace/name must not be empty"));
        }
        let client = self
            .manager
            .get(&req.cluster_id)
            .map_err(|e| Status::not_found(e.to_string()))?;
        let deleted = resource::write::delete_deployment(&client, &req.namespace, &req.name)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(superops_protos::k8s::v1::DeleteDeploymentResponse { deleted }))
    }
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p superops-k8s-service 2>&1 | tail -5`
Expected: `test result: ok`，全部通过。

- [ ] **Step 5（可选）:** commit `feat(k8s-service): expose deployment write RPCs`

### Task 4: gateway — 写操作 HTTP 路由

**Files:**
- Modify: `services/gateway/src/proxy/k8s_proxy.rs`

- [ ] **Step 1: 写失败测试（纯函数校验 + 错误映射）**

在 `services/gateway/src/proxy/k8s_proxy.rs` 追加：

```rust
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_scale_body_valid() {
        assert_eq!(parse_scale_body(&serde_json::json!({ "replicas": 3 })), Ok(3));
    }

    #[test]
    fn parse_scale_body_rejects_bad_input() {
        assert!(parse_scale_body(&serde_json::json!({})).is_err());
        assert!(parse_scale_body(&serde_json::json!({ "replicas": 2000 })).is_err());
        assert!(parse_scale_body(&serde_json::json!({ "replicas": -1 })).is_err());
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p superops-gateway parse_scale_body 2>&1 | tail -5`
Expected: 编译失败（函数未定义）。

- [ ] **Step 3: 注册路由并实现 handler**

在 `k8s_routes()` 中追加三个路由（模式参照现有 `list_nodes` handler 的 `State + connect + 转发` 三段式）：

```rust
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
```

在文件末尾追加 handler（错误映射统一走辅助函数）：

```rust
fn status_to_http(e: tonic::Status) -> (StatusCode, Json<serde_json::Value>) {
    let code = match e.code() {
        tonic::Code::NotFound => StatusCode::NOT_FOUND,
        tonic::Code::InvalidArgument => StatusCode::BAD_REQUEST,
        _ => StatusCode::BAD_GATEWAY,
    };
    (code, Json(serde_json::json!({ "error": e.message() })))
}

async fn scale_deployment(
    State(state): State<crate::AppState>,
    Path((cid, ns, name)): Path<(String, String, String)>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let replicas = parse_scale_body(&body).map_err(|e| (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": e }))))?;
    let endpoint = { state.k8s_endpoint.read().unwrap().clone() };
    let mut client = superops_protos::k8s::v1::k8s_service_client::K8sServiceClient::connect(endpoint)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, Json(serde_json::json!({ "error": format!("k8s backend unreachable: {e}") }))))?;
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
    Ok(Json(serde_json::json!({ "cluster_id": cid, "deployment": { "namespace": ns, "name": name, "replicas": resp.replicas } })))
}

async fn restart_deployment(
    State(state): State<crate::AppState>,
    Path((cid, ns, name)): Path<(String, String, String)>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let endpoint = { state.k8s_endpoint.read().unwrap().clone() };
    let mut client = superops_protos::k8s::v1::k8s_service_client::K8sServiceClient::connect(endpoint)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, Json(serde_json::json!({ "error": format!("k8s backend unreachable: {e}") }))))?;
    let _resp = client
        .restart_deployment(superops_protos::k8s::v1::RestartDeploymentRequest {
            cluster_id: cid.clone(),
            namespace: ns.clone(),
            name: name.clone(),
        })
        .await
        .map_err(status_to_http)?;
    Ok(Json(serde_json::json!({ "cluster_id": cid, "deployment": { "namespace": ns, "name": name, "restarted": true } })))
}

async fn delete_deployment(
    State(state): State<crate::AppState>,
    Path((cid, ns, name)): Path<(String, String, String)>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let endpoint = { state.k8s_endpoint.read().unwrap().clone() };
    let mut client = superops_protos::k8s::v1::k8s_service_client::K8sServiceClient::connect(endpoint)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, Json(serde_json::json!({ "error": format!("k8s backend unreachable: {e}") }))))?;
    client
        .delete_deployment(superops_protos::k8s::v1::DeleteDeploymentRequest {
            cluster_id: cid.clone(),
            namespace: ns.clone(),
            name: name.clone(),
        })
        .await
        .map_err(status_to_http)?;
    Ok(StatusCode::NO_CONTENT)
}
```

- [ ] **Step 4: 运行测试与格式检查**

Run: `cargo test -p superops-gateway 2>&1 | tail -5 && cargo fmt --check`
Expected: 全部通过、fmt 无 diff。

- [ ] **Step 5（可选）:** commit `feat(gateway): deployment write routes (scale/restart/delete)`

### Task 5: 写操作审计事件

**Files:**
- Modify: `services/gateway/src/proxy/k8s_proxy.rs`

- [ ] **Step 1: 定位现有审计发布点**

Run: `grep -n "audit\|producer\|kafka" services/gateway/src/auth/handler.rs | head -10`
Expected: 找到登录/注册等操作发布审计事件的既有代码（Kafka producer + topic `superops.audit`）。

- [ ] **Step 2: 在三个写 handler 成功路径后发布审计**

模式：在每个 handler 的 `await` 成功之后、返回之前，调用与 Step 1 相同的发布函数（内容包含 `action`、`cluster_id`、`namespace`、`name`，动作名 `k8s.scale` / `k8s.restart` / `k8s.delete`，当前用户名从请求的 `AuthClaims` 扩展中读取——若 handler 未注入 claims，则 `grep "AuthClaims" services/gateway/src/proxy/` 按现有注入方式补上）。发布失败仅 `tracing::warn!`，不影响主流程（审计不可阻断写操作）。

- [ ] **Step 3: 验证**

Run: `cargo test -p superops-gateway 2>&1 | tail -5 && cargo build -p superops-gateway`
Expected: 通过。e2e 验证（可选，需 compose 起 Kafka）：登录 → POST scale → `docker exec` 查 Kafka topic 或 collector 日志中出现对应审计行。

- [ ] **Step 4（可选）:** commit `feat(gateway): audit k8s write operations`

## P4-2 告警通知（webhook / 钉钉 / 企微）

### Task 6: collector — notify.rs 通知模块

**Files:**
- Create: `services/collector/src/notify.rs`
- Modify: `services/collector/src/lib.rs`（注册模块）
- Modify: `services/collector/Cargo.toml`（新增 `reqwest`；`serde` 已存在）

- [ ] **Step 1: 写失败测试（payload 构造纯函数）**

Create `services/collector/src/notify.rs`：

```rust
use crate::alert::AlertEvent;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct NotifyTarget {
    pub name: String,
    #[serde(default = "default_kind")]
    pub kind: String, // generic | dingtalk | wecom
    pub url: String,
}

fn default_kind() -> String {
    "generic".into()
}

pub fn build_payload(kind: &str, events: &[AlertEvent]) -> serde_json::Value {
    match kind {
        "dingtalk" => serde_json::json!({
            "msgtype": "markdown",
            "markdown": {
                "title": format!("SuperOps 告警 {} 条", events.len()),
                "text": events.iter().map(|e| format!("**{}** {} — {}", e.level, e.title, e.message)).collect::<Vec<_>>().join("\n\n")
            }
        }),
        "wecom" => serde_json::json!({
            "msgtype": "text",
            "text": { "content": format!("[SuperOps] 告警 {} 条: {}", events.len(),
                events.iter().map(|e| format!("{} {}", e.title, e.message)).collect::<Vec<_>>().join("; ")) }
        }),
        _ => serde_json::json!({
            "level": events.first().map(|e| e.level.clone()).unwrap_or_default(),
            "title": events.first().map(|e| e.title.clone()).unwrap_or_default(),
            "message": events.first().map(|e| e.message.clone()).unwrap_or_default(),
            "count": events.len()
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(title: &str) -> AlertEvent {
        AlertEvent { level: "WARN".into(), title: title.into(), message: format!("msg {title}"), node: None }
    }

    #[test]
    fn generic_payload_has_count_and_first_event() {
        let p = build_payload("generic", &[ev("pod-not-running"), ev("node-not-ready")]);
        assert_eq!(p["count"], 2);
        assert_eq!(p["title"], "pod-not-running");
    }

    #[test]
    fn dingtalk_payload_is_markdown() {
        let p = build_payload("dingtalk", &[ev("pod-not-running")]);
        assert_eq!(p["msgtype"], "markdown");
        assert!(p["markdown"]["text"].as_str().unwrap().contains("pod-not-running"));
    }

    #[test]
    fn wecom_payload_is_text() {
        let p = build_payload("wecom", &[ev("node-not-ready")]);
        assert_eq!(p["msgtype"], "text");
        assert!(p["text"]["content"].as_str().unwrap().contains("node-not-ready"));
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p superops-collector notify::tests 2>&1 | tail -5`
Expected: 编译失败（模块未注册 / reqwest 未声明）。

- [ ] **Step 3: 注册模块并实现 dispatch + 去重窗口**

`services/collector/src/lib.rs`：`pub mod notify;`
`services/collector/Cargo.toml` `[dependencies]` 追加 `reqwest = { version = "0.12", features = ["json"] }`（版本以 workspace 既有版本为准，`grep reqwest Cargo.lock`）。

在 notify.rs 追加：

```rust
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct NotifySilencer {
    last_sent: HashMap<String, Instant>,
}

impl NotifySilencer {
    pub fn should_send(&mut self, key: &str, silence_secs: u64) -> bool {
        let now = Instant::now();
        let ok = match self.last_sent.get(key) {
            Some(t) => now.duration_since(*t) >= Duration::from_secs(silence_secs),
            None => true,
        };
        if ok {
            self.last_sent.insert(key.to_string(), now);
        }
        ok
    }
}

pub async fn dispatch(target: &NotifyTarget, events: &[AlertEvent], http: &reqwest::Client) -> Result<(), String> {
    let payload = build_payload(&target.kind, events);
    let resp = http.post(&target.url).json(&payload).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("notify {}: http {}", target.name, resp.status()));
    }
    Ok(())
}
```

在 `mod tests` 中追加：

```rust
    #[test]
    fn silencer_respects_window() {
        let mut s = NotifySilencer::default();
        assert!(s.should_send("pod-not-running:node-1", 300));
        assert!(!s.should_send("pod-not-running:node-1", 300));
        assert!(s.should_send("node-not-ready:node-2", 300)); // 不同 key 不受影响
    }
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p superops-collector notify::tests 2>&1 | tail -5`
Expected: `test result: ok`，4 个测试通过。

- [ ] **Step 5（可选）:** commit `feat(collector): alert notify module (generic/dingtalk/wecom) with silencer`

### Task 7: 接入巡检流程 + 配置

**Files:**
- Modify: `services/collector/src/alert.rs`
- Modify: `services/collector/src/config.rs`
- Modify: `config/collector.yaml`

- [ ] **Step 1: 写失败测试（配置解析）**

在 `services/collector/src/config.rs` 的测试中追加（先读该文件确认现有 Config 结构与测试写法）：

```rust
    #[test]
    fn notify_config_parses() {
        let cfg: Config = serde_yaml::from_str(
            "notify:\n  silence_secs: 300\n  targets:\n    - name: ops\n      kind: dingtalk\n      url: https://oapi.dingtalk.com/robot/send?access_token=x\n",
        )
        .expect("parse");
        assert_eq!(cfg.notify.silence_secs, 300);
        assert_eq!(cfg.notify.targets.len(), 1);
        assert_eq!(cfg.notify.targets[0].kind, "dingtalk");
    }
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p superops-collector notify_config 2>&1 | tail -5`
Expected: 编译失败（`cfg.notify` 不存在）。

- [ ] **Step 3: 实现 Config 字段并在 inspect_work 接入**

`config.rs` 追加（`Default` 语义对齐现有字段）：

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct NotifyConfig {
    #[serde(default = "default_silence")]
    pub silence_secs: u64,
    #[serde(default)]
    pub targets: Vec<crate::notify::NotifyTarget>,
}

fn default_silence() -> u64 {
    300
}
```

Config 结构体新增字段：`#[serde(default)] pub notify: NotifyConfig,`（若 Config 非 `Default`，则按现有构造方式补默认值；`grep "impl Default" services/collector/src/config.rs` 确认）。

`alert.rs` 的 `inspect_work` 在 `TsdbClient::write` 成功之后追加：

```rust
    let http = reqwest::Client::new();
    let mut silencer = crate::notify::NotifySilencer::default();
    for t in &cfg.notify.targets {
        let fresh: Vec<AlertEvent> = health
            .alerts
            .iter()
            .filter(|e| {
                let key = format!("{}:{}", e.title, e.node.as_deref().unwrap_or(""));
                silencer.should_send(&key, cfg.notify.silence_secs)
            })
            .cloned()
            .collect();
        if fresh.is_empty() {
            continue;
        }
        if let Err(e) = crate::notify::dispatch(t, &fresh, &http).await {
            tracing::warn!("notify target {} failed: {e}", t.name);
        }
    }
```

> 注意：`inspect_work` 当前的早退条件 `if health.cluster_ok || health.alerts.is_empty()` 保持不变——通知仅在产生告警时触发。

`config/collector.yaml` 追加：

```yaml
notify:
  silence_secs: 300
  targets: []
```

- [ ] **Step 4: 运行测试**

Run: `cargo test -p superops-collector 2>&1 | tail -5 && cargo fmt --check`
Expected: 全部通过、fmt 无 diff。

- [ ] **Step 5: e2e 验证（可选，需 compose 起 Redis/ClickHouse/K8s 集群）**

临时在 `config/collector.yaml` 配一个 webhook 目标（可用 `nc -l` 或临时 axum 服务接收），触发巡检产生告警后观察请求体符合对应 kind 的 payload 结构；验证后移除临时目标。

- [ ] **Step 6（可选）:** commit `feat(collector): wire alert notifications into inspection`

## P4-3 审计中心 / API Key / 用户管理页面

### Task 8: gateway — 审计事件查询 API

**Files:**
- Create: `services/gateway/src/audit_api.rs`
- Modify: `services/gateway/src/main.rs`（或路由汇总处，`grep "metrics_api" services/gateway/src/` 定位注册点）
- Modify: `services/gateway/Cargo.toml`（新增 `ecat-data-clickhouse`、`ecat-data`）
- Modify: `services/gateway/src/config.rs` + `config/gateway.yaml`（新增 `ch:` 段）

- [ ] **Step 1: 写失败测试（分页参数解析纯函数）**

Create `services/gateway/src/audit_api.rs`：

```rust
use axum::{Json, extract::Query, http::StatusCode, response::IntoResponse};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
pub struct AuditQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub level: Option<String>,
}

fn clamp_pagination(q: &AuditQuery) -> (i64, i64) {
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    let offset = q.offset.unwrap_or(0).max(0);
    (limit, offset)
}

pub async fn audit_events(Query(q): Query<AuditQuery>) -> impl IntoResponse {
    let (limit, offset) = clamp_pagination(&q);
    let sql = match &q.level {
        Some(l) => format!("SELECT * FROM audit_log WHERE level = '{}' ORDER BY ts DESC LIMIT {limit} OFFSET {offset}", l),
        None => format!("SELECT * FROM audit_log ORDER BY ts DESC LIMIT {limit} OFFSET {offset}"),
    };
    // TODO(Task 8 Step 3): 用 ClickhouseClient 执行 sql 并映射为 JSON
    Json(serde_json::json!({ "events": [], "sql": sql, "level": q.level }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pagination_is_clamped() {
        assert_eq!(clamp_pagination(&AuditQuery { limit: None, offset: None, ..Default::default() }), (50, 0));
        assert_eq!(clamp_pagination(&AuditQuery { limit: Some(5000), offset: Some(-3), ..Default::default() }), (500, 0));
        assert_eq!(clamp_pagination(&AuditQuery { limit: Some(1), offset: Some(10), ..Default::default() }), (1, 10));
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p superops-gateway pagination_is_clamped 2>&1 | tail -5`
Expected: 编译失败（模块未注册）。

- [ ] **Step 3: 注册路由、接入 ClickHouse 查询**

注册模块：在 `services/gateway/src/main.rs` 的模块声明区（`grep "^mod " services/gateway/src/main.rs` 定位）追加 `pub mod audit_api;`；在 metrics 相关路由注册处追加：

```rust
        .route("/api/audit/events", axum::routing::get(crate::audit_api::audit_events))
```

`audit_api.rs` 的 handler 改为接收 `State<crate::AppState>`（`grep "pub struct AppState" services/gateway/src/` 确认现有字段与构造方式，为 AppState 增加 `ch: Arc<ClickhouseClient>` 或从配置按需构造——参照 collector `ch.rs` 的 `ClickhouseClient::from_config` 模式），执行 SQL 后把行映射为 `serde_json::Value` 返回。

> 实现提示：`ecat-data-clickhouse` 的查询 API 以 `Cargo.lock` 锁定的 clickhouse crate 版本为准（先 `grep 'name = "clickhouse"' Cargo.lock -A1` 确认版本，再按其 `query().fetch_all::<Row>()` 模式实现）；若 facade 未暴露 SELECT，可直接依赖 clickhouse crate 自身构造客户端，但必须复用 `cfg.ch` 配置段。

- [ ] **Step 4: 验证**

Run: `cargo test -p superops-gateway 2>&1 | tail -5 && cargo fmt --check && cargo build -p superops-gateway`
Expected: 通过。e2e（需 ClickHouse 有 audit_log 数据）：`curl localhost:8080/api/audit/events?limit=5` 返回 `{"events":[...]}`。

- [ ] **Step 5（可选）:** commit `feat(gateway): audit events query API`

### Task 9: gateway — 用户管理 API

**Files:**
- Modify: `services/gateway/src/model/user.rs`（先 Read，沿用现有 sqlx 模式）
- Modify: `services/gateway/src/auth/` 路由注册处（`grep "api/keys\|users" services/gateway/src/auth/ -r` 定位）

- [ ] **Step 1: 写失败测试（handler 行为，参照现有 auth handler 测试写法）**

先 Read `services/gateway/src/auth/handler.rs` 与 `model/user.rs`，按其中既有测试模式为 `list_users` / `set_user_status` 各写一个测试：合法请求 200、非法 status 值 400。

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p superops-gateway users 2>&1 | tail -5`
Expected: 编译失败（API 不存在）。

- [ ] **Step 3: 实现**

`model/user.rs` 追加：

```rust
pub async fn list_users(pool: &sqlx::MySqlPool) -> Result<Vec<UserRow>, sqlx::Error> {
    sqlx::query_as::<_, UserRow>("SELECT id, username, email, role, status, created_at FROM users ORDER BY created_at DESC")
        .fetch_all(pool)
        .await
}

pub async fn set_user_status(pool: &sqlx::MySqlPool, id: i64, status: &str) -> Result<bool, sqlx::Error> {
    let r = sqlx::query("UPDATE users SET status = ? WHERE id = ?")
        .bind(status)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}
```

> 字段名（`role`/`status`/`created_at`）与 `UserRow` 定义以 `model/user.rs` 实际 schema 为准——若表缺列，在 `deploy/init.sql` 追加 `ALTER TABLE users ADD COLUMN ...`（先 Read init.sql 确认 `users` 建表语句）。

路由（注册到 auth 路由区）：

```rust
        .route("/api/users", axum::routing::get(list_users))
        .route("/api/users/{id}/status", axum::routing::patch(set_user_status))
```

handler：`list_users` 返回 `{"users":[...]}`；`set_user_status` 校验 status ∈ {enabled, disabled} 否则 400，行数 0 返回 404，成功 204。

- [ ] **Step 4: 验证**

Run: `cargo test -p superops-gateway 2>&1 | tail -5 && cargo fmt --check`
Expected: 通过。

- [ ] **Step 5（可选）:** commit `feat(gateway): user list and status management API`

### Task 10: 前端 — 审计中心 / API Key / 用户管理页

**Files:**
- Create: `frontend/src/pages/ops/audit.tsx`、`frontend/src/pages/ops/apikeys.tsx`、`frontend/src/pages/ops/users.tsx`
- Modify: `frontend/src/App.tsx`
- Modify: `frontend/src/services/api.ts`（若已有 API key 方法则复用；`grep -n "keys" frontend/src/services/api.ts` 确认）

- [ ] **Step 1: 实现三个页面**

按现有页面模式（先 Read `frontend/src/pages/k8s/pods.tsx` 确认 ProTable 用法与 api 调用风格）：

- `ops/audit.tsx`：ProTable 请求 `/api/audit/events`，列：时间 / 用户 / 动作 / 资源 / 详情，顶部 level 筛选。
- `ops/apikeys.tsx`：ProTable + 「新建」Modal（名称/权限）→ POST `/api/keys`；行操作：复制（`navigator.clipboard`）、吊销（DELETE）；已吊销行置灰。
- `ops/users.tsx`：ProTable 请求 `/api/users`，状态列 Switch 调 PATCH `/api/users/{id}/status`。

- [ ] **Step 2: 注册路由与菜单**

`App.tsx` 的 menuData 追加「运维中心」组：

```tsx
{ path: '/ops', name: '运维中心', icon: <SettingOutlined />, children: [
  { path: '/ops/audit', name: '审计中心' }, { path: '/ops/apikeys', name: 'API Keys' },
  { path: '/ops/users', name: '用户管理' },
]},
```

并追加三个 `<Route>`；`SettingOutlined` 从 `@ant-design/icons` 导入。`frontend/src/services/api.ts` 若缺 `/api/audit/events`、`/api/users` 方法则补齐。

- [ ] **Step 3: 验证**

Run: `cd frontend && npm run build`
Expected: tsc + vite 构建通过。

- [ ] **Step 4（可选）:** commit `feat(frontend): ops center pages (audit / api keys / users)`

### Task 11: P4 文档同步

**Files:**
- Modify: `CHANGELOG.md`、`README.md`、`README.en.md`
- Modify: `docs/images/structure.svg`、`docs/images/tree.svg`

- [ ] **Step 1: 更新变更日志与 README**

`CHANGELOG.md` 顶部新增条目：k8s 写操作（scale/restart/delete + 审计）、告警通知（generic/钉钉/企微 + 静默窗口）、审计/API Key/用户管理页。`README.md` 与 `README.en.md` 的功能说明表（Gateway 行 + Frontend 行）与 API 一览行同步补充新端点（`/api/k8s/clusters/{id}/deployments/{ns}/{name}/(scale|restart)`、`/api/audit/events`、`/api/users`）。

- [ ] **Step 2: 更新 SVG 图集**

`structure.svg` 的 Gateway 列补充写操作行（`proxy/ k8s 写操作 scale/restart/delete`），Collector 列补充 `notify.rs 告警通知`，Frontend 列补充 ops 页面；`tree.svg` 对应行同步；保持既有配色与行距规范。用 `python3 -c "import xml.dom.minidom,sys; xml.dom.minidom.parse(sys.argv[1])" docs/images/structure.svg docs/images/tree.svg` 验证 XML 合法。

- [ ] **Step 3: 全量验证**

Run: `cargo test --workspace 2>&1 | tail -3 && cargo fmt --check && cd frontend && npm run build`
Expected: workspace 测试全绿（数量由 296 增为 296+N）、fmt 无 diff、前端构建通过。

- [ ] **Step 4（可选）:** commit `docs: P4 ecosystem completion docs & diagrams`

---

## P4 完成标准

- [ ] `cargo test --workspace` 全绿，新增测试均覆盖边界校验（空字段、replicas 越界、非法 status）
- [ ] e2e：三个写操作路由在真实 k8s 集群上生效，熔断/限流链路不受影响
- [ ] 告警触发时目标渠道收到正确 payload，静默窗口内不重复推送
- [ ] 前端三页可访问、构建通过；README/SVG/CHANGELOG 同步
- [ ] 全程未打包、未 push、无 secrets 入库

**下一步：P5（日志中心 / CMDB / 批量执行 / RBAC）见 `2026-08-06-ecosystem-gap-completion-p5.md`；P6 见 `2026-08-06-ecosystem-gap-completion-p6.md`。**
