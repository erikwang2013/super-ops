# SuperOps P2 韧性与可观测 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** gateway 接入 ecat-circuit-breaker 熔断、Consul 服务注册/发现（ecat-registry-consul）、Consul KV 动态配置热更新（ecat-config-remote）、ecat-middleware 限流迁移（Redis 共享 store）、OTLP 链路追踪（ecat-tracing-otlp），并将 k8s nodes stub 改为真实 gRPC 代理，完成 P2 韧性与可观测闭环。

**Architecture:** 中间件链 `ErrorToResponseLayer → CircuitBreakerLayer → FiveXxToErrorLayer` 包住 k8s 路由组（handler 恒 Ok，需最内层 FiveXxToError 把 5xx response 转成 Service 错误熔断器才能计数；熔断器必须包在 FiveXxToError 之外、ErrorToResponse 之内）；gateway/k8s/collector 三服务经 Consul 注册（`Registration` 存 `Arc<Mutex<Option<Registration>>>`，Drop 自动注销），gateway 启动时 `discover("superops-k8s")` 写入 `AppState.k8s_endpoint: Arc<RwLock<String>>`，失败回退静态配置；Consul KV `config/superops/gateway/*` 经 `ConsulConfigSource::watch()` 推送，`DynamicRateLimitStore` 每次 check 读动态值委托内层 Redis/内存 store；OTLP provider 绑定 main 作用域保活到进程结束。

**Tech Stack:** ecat-circuit-breaker / ecat-registry / ecat-registry-consul / ecat-config-remote / ecat-config / ecat-middleware(redis feature) / ecat-tracing-otlp / tonic gRPC 客户端 / Consul / Jaeger

> 提交步骤全部省略：用户指令「开始执行规划。不要做任何打包。」+ CLAUDE.md「除非用户明确要求，否则永不提交」。

---

## 前置事实（已从 ecat 源码核实）

- `CircuitBreakerLayer` 非泛型 `Layer<S>`（无 body 约束）；`CircuitBreakerService<S>` 要求 `S: Service<Req> + Clone + Send + 'static`、`S::Error: Display + Error + Send + Sync + 'static`；打开条件 `total >= 5 && failure_ratio >= ratio`；打开错误消息 `"circuit breaker is open"`（`std::io::Error::other`）；半开探针超限 `"circuit breaker: too many probes"`。
- `Registration`（ecat-registry）：`Send + Sync`，`Drop` 自动 deregister（`Handle::try_current()` 无 runtime 时仅 WARN）；`Registry` trait 全 `async_trait`。
- `ConsulRegistry::new(base_url: impl Into<String>) -> Self`（**infallible**）；`register(info)` 用第一个 endpoint 解析 address/port；`discover(name)` 走 `/v1/health/service/{name}?dc=dc1&passing=true`，endpoint 格式 `http://addr:port`。
- `ConsulConfigSource::new(addr, key_prefix)`；`watch(&self) -> mpsc::Receiver<Result<HashMap<String, Value>, ConfigError>>`（阻塞查询 wait=5m/超时 330s、首帧强制推送、key strip prefix + `/`→`.`）；`ConfigError` 来自 **ecat-config** crate（需单独加依赖，不在 ecat-config-remote 的 re-export 中）。
- `RateLimitLayer::new(max_requests: u32, window: Duration)`（**Duration 不是 secs**）；`.with_store(Arc<dyn RateLimitStore>)`；`RateLimitStore` 为 `#[async_trait]`：`async fn check(&self, key: &str, max: u32, window_secs: u64) -> Result<(), String>`；错误消息 `"rate limit exceeded"`；`RedisRateLimitStore::connect(url) -> Result<Self, String>`（async，Redis 不可用 fail-open + warn）；feature `redis`。
- `ecat_tracing_otlp::init(service_name, endpoint) -> Result<TracerProvider, String>`（同步）；provider 需保活到进程结束。
- axum 0.8 `Router::layer` 要求 `L::Service::Error: Into<Infallible> + 'static`（**不是** IntoResponse）→ 链最外层用自研 `ErrorToResponseLayer`（Error=Infallible，call 内把 inner error 经 `error_to_response` 转成 Response）。`ServiceBuilder::new().layer(A).layer(B).layer(C).service(S)` = `A(B(C(S)))`，第一个添加的层最外层。`CircuitBreakerService` 要求 `S::Error: Display + Error + Send + Sync` → `FiveXxToErrorService` 的 Error 必须是具体类型 `io::Error`（不能是 `Box<dyn Error>`，否则 `Box<dyn Error + Send + Sync>` 不满足 `Error` bound）；层序必须 `ErrorToResponse → breaker → FiveXxToError`（breaker 包在 FiveXxToError 外才能看到其转换后的错误并计数；反之 breaker 看到 Ok(500) 永不打开）。
- gateway 现有测试 17 个（breaker 11 + health 3 + metrics_api 3）；k8s 0 个；collector 9 个（tests/events_test.rs 3 + tests/alert_test.rs 6）。
- 总纲 P2 复选框位于 `docs/superpowers/plans/2026-08-06-super-ops-ecosystem-expansion.md` 第 162-199 行区间内（`- [ ]` 行：168/175/182/189/196）。

---

### Task 1: 熔断基础设施（breaker.rs + k8s 路由挂链）

**Files:**
- Create: `services/gateway/src/breaker.rs`（含 11 个 inline tests）
- Modify: `services/gateway/Cargo.toml`（`[dependencies]` 加 `ecat-circuit-breaker`）
- Modify: `services/gateway/src/main.rs`（`mod breaker;` + k8s 路由链）

- [ ] **Step 1: 写 breaker.rs（实现 + 11 个测试）**

`services/gateway/src/breaker.rs` 完整内容：

```rust
use axum::http::{Request, StatusCode};
use axum::response::{IntoResponse, Response};
use ecat_circuit_breaker::CircuitBreakerLayer;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tower::Service;

pub fn breaker_layer() -> CircuitBreakerLayer {
    CircuitBreakerLayer::new()
        .failure_ratio(0.5)
        .window(Duration::from_secs(30))
        .half_open_probes(3)
        .open_duration(Duration::from_secs(10))
}

pub fn error_to_response(err: Box<dyn std::error::Error + Send + Sync>) -> Response {
    let msg = err.to_string();
    let status = if msg.contains("circuit breaker is open") {
        StatusCode::SERVICE_UNAVAILABLE
    } else if msg.contains("rate limit exceeded") {
        StatusCode::TOO_MANY_REQUESTS
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    let body = if status == StatusCode::TOO_MANY_REQUESTS {
        "too many login attempts, try again later"
    } else {
        &msg
    };
    (status, axum::Json(serde_json::json!({ "error": body }))).into_response()
}

pub struct FiveXxToErrorLayer;

impl<S> tower::Layer<S> for FiveXxToErrorLayer {
    type Service = FiveXxToErrorService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        FiveXxToErrorService { inner }
    }
}

pub struct FiveXxToErrorService<S> {
    inner: S,
}

impl<S, B> Service<Request<B>> for FiveXxToErrorService<S>
where
    S: Service<Request<B>>,
    S::Response: IntoResponse,
    S::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
    S::Future: Future<Output = Result<S::Response, S::Error>> + Send + 'static,
{
    type Response = Response;
    type Error = Box<dyn std::error::Error + Send + Sync>;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        let fut = self.inner.call(req);
        Box::pin(async move {
            let resp = fut.await.map_err(Into::into)?;
            let response = resp.into_response();
            if response.status().is_server_error() {
                Err(std::io::Error::other(format!(
                    "upstream returned {}",
                    response.status()
                ))
                .into())
            } else {
                Ok(response)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use std::io;
    use tower::service_fn;

    #[test]
    fn error_to_response_maps_open_to_503() {
        let err: Box<dyn std::error::Error + Send + Sync> =
            Box::new(io::Error::other("circuit breaker is open"));
        let resp = error_to_response(err);
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn error_to_response_maps_rate_limit_to_429() {
        let err: Box<dyn std::error::Error + Send + Sync> =
            Box::new(io::Error::other("rate limit exceeded"));
        let resp = error_to_response(err);
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[test]
    fn error_to_response_passthrough_500() {
        let err: Box<dyn std::error::Error + Send + Sync> = Box::new(io::Error::other("boom"));
        let resp = error_to_response(err);
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn five_xx_to_error_converts_500() {
        let mut svc = FiveXxToErrorLayer.layer(service_fn(|_: Request<()>| async {
            Ok::<Response, io::Error>((StatusCode::INTERNAL_SERVER_ERROR, "boom").into_response())
        }));
        let err = svc.call(Request::new(())).await.unwrap_err();
        assert!(err.to_string().contains("upstream returned 500"));
    }

    #[tokio::test]
    async fn five_xx_to_error_passes_2xx() {
        let mut svc = FiveXxToErrorLayer.layer(service_fn(|_: Request<()>| async {
            Ok::<Response, io::Error>((StatusCode::OK, "ok").into_response())
        }));
        let resp = svc.call(Request::new(())).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn breaker_stays_closed_on_success() {
        let layer = breaker_layer();
        let mut svc = layer.layer(service_fn(|_: Request<()>| async {
            Ok::<Response, io::Error>((StatusCode::OK, "ok").into_response())
        }));
        for _ in 0..5 {
            let resp = svc.call(Request::new(())).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
        }
    }

    #[tokio::test]
    async fn breaker_opens_after_five_failures() {
        let layer = breaker_layer();
        let mut svc = layer.layer(service_fn(|_: Request<()>| async {
            Err::<Response, _>(io::Error::other("boom"))
        }));
        for _ in 0..5 {
            assert!(svc.call(Request::new(())).await.is_err());
        }
        let err = svc.call(Request::new(())).await.unwrap_err();
        assert!(err.to_string().contains("circuit breaker is open"));
    }

    #[tokio::test]
    async fn breaker_passes_through_errors_before_open() {
        let layer = breaker_layer();
        let mut svc = layer.layer(service_fn(|_: Request<()>| async {
            Err::<Response, _>(io::Error::other("boom"))
        }));
        let err = svc.call(Request::new(())).await.unwrap_err();
        assert!(err.to_string().contains("boom"));
        assert!(!err.to_string().contains("circuit breaker is open"));
    }

    #[tokio::test]
    async fn breaker_probes_inner_after_open_duration() {
        let layer = CircuitBreakerLayer::new()
            .failure_ratio(0.5)
            .window(Duration::from_secs(30))
            .half_open_probes(3)
            .open_duration(Duration::from_millis(50));
        let mut svc = layer.layer(service_fn(|_: Request<()>| async {
            Err::<Response, _>(io::Error::other("boom"))
        }));
        for _ in 0..5 {
            let _ = svc.call(Request::new(())).await;
        }
        let err = svc.call(Request::new(())).await.unwrap_err();
        assert!(err.to_string().contains("circuit breaker is open"));
        tokio::time::sleep(Duration::from_millis(80)).await;
        let err = svc.call(Request::new(())).await.unwrap_err();
        assert!(err.to_string().contains("boom"));
    }

    #[tokio::test]
    async fn chain_fivexx_feeds_breaker_until_open() {
        let mut svc = tower::ServiceBuilder::new()
            .layer(tower::util::MapErrLayer::new(error_to_response))
            .layer(FiveXxToErrorLayer)
            .layer(breaker_layer())
            .service(service_fn(|_: Request<()>| async {
                Ok::<Response, io::Error>(
                    (StatusCode::INTERNAL_SERVER_ERROR, "upstream broken").into_response(),
                )
            }));
        for _ in 0..5 {
            let resp = svc.call(Request::new(())).await.unwrap();
            assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        }
        let resp = svc.call(Request::new(())).await.unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
```

- [ ] **Step 2: 加依赖**

`services/gateway/Cargo.toml` `[dependencies]` 在 `ecat-mq-kafka` 后加：

```toml
ecat-circuit-breaker = { path = "../../ecat-circuit-breaker" }
```

- [ ] **Step 3: 挂 mod + k8s 路由链**

`services/gateway/src/main.rs`：
- 第 1 行前加 `mod breaker;`（`mod auth;` 之后、`mod config;` 之前）
- 替换第 62-65 行 k8s 构建为：

```rust
    let k8s = k8s_routes()
        .layer(middleware::from_fn_with_state(state.clone(), auth_middleware))
        .layer(
            tower::ServiceBuilder::new()
                .layer(crate::breaker::ErrorToResponseLayer)
                .layer(crate::breaker::breaker_layer())
                .layer(crate::breaker::FiveXxToErrorLayer),
        );
```

- [ ] **Step 4: 运行测试验证**

Run: `cargo test -p superops-gateway breaker`
Expected: PASS — 10 passed（`error_to_response_maps_open_to_503` / `chain_fivexx_feeds_breaker_until_open` 等全部通过）

---

### Task 2: k8s 端点解析辅助 + nodes 真实 gRPC 代理

**Files:**
- Create: `services/gateway/src/registry.rs`（3 个 tests）
- Modify: `services/gateway/src/main.rs`（`mod registry;` + `AppState.k8s_endpoint`）
- Modify: `services/gateway/src/config.rs`（去掉 services/endpoint 的 `#[allow(dead_code)]`）
- Modify: `services/gateway/src/proxy/k8s_proxy.rs`（list_nodes 真实代理）

- [ ] **Step 1: 写 registry.rs（实现 + 3 个测试）**

`services/gateway/src/registry.rs` 完整内容：

```rust
use ecat_registry::ServiceInfo;

pub fn resolve_k8s_endpoint(discovered: &[ServiceInfo], fallback: &str) -> String {
    discovered
        .iter()
        .find_map(|info| info.endpoints.first())
        .cloned()
        .unwrap_or_else(|| fallback.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_discovery_uses_fallback() {
        assert_eq!(
            resolve_k8s_endpoint(&[], "http://localhost:9091"),
            "http://localhost:9091"
        );
    }

    #[test]
    fn first_endpoint_wins() {
        let discovered = vec![
            ServiceInfo::new("superops-k8s", "1.0.0").with_endpoint("http://10.0.0.1:9091"),
            ServiceInfo::new("superops-k8s", "1.0.0").with_endpoint("http://10.0.0.2:9091"),
        ];
        assert_eq!(
            resolve_k8s_endpoint(&discovered, "http://localhost:9091"),
            "http://10.0.0.1:9091"
        );
    }

    #[test]
    fn service_without_endpoints_falls_back() {
        let discovered = vec![ServiceInfo::new("superops-k8s", "1.0.0")];
        assert_eq!(
            resolve_k8s_endpoint(&discovered, "http://localhost:9091"),
            "http://localhost:9091"
        );
    }
}
```

- [ ] **Step 2: main.rs 挂 mod + AppState 字段**

`services/gateway/src/main.rs`：
- `mod registry;`（`mod proxy;` 后）
- 第 26 行 `use std::sync::Arc;` 改为 `use std::sync::{Arc, RwLock};`
- AppState 增删字段：

```rust
#[derive(Clone)]
pub struct AppState {
    pub user_store: UserStore,
    pub auth: Arc<AuthState>,
    pub ch: Arc<ClickhouseClient>,
    pub mq: Option<Arc<KafkaMq>>,
    pub k8s_endpoint: Arc<RwLock<String>>,
}
```

- state 构造加一行：

```rust
        k8s_endpoint: Arc::new(RwLock::new(config.services.k8s.endpoint.clone())),
```

- [ ] **Step 3: config.rs 解除 dead_code**

`services/gateway/src/config.rs`：删除 `pub services: ServicesConfig` 与 `pub endpoint: String` 前的 `#[allow(dead_code)]`（T3 注册后 gateway 会读取这两处）。

- [ ] **Step 4: k8s_proxy.rs 真实代理 list_nodes**

`services/gateway/src/proxy/k8s_proxy.rs`：
- `use axum::{extract::{Path, Query}, ...}` 改为 `use axum::{extract::{Path, Query, State}, ...}`
- 替换第 101-103 行 stub：

```rust
async fn list_nodes(
    State(state): State<crate::AppState>,
    Path(cid): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let endpoint = { state.k8s_endpoint.read().unwrap().clone() };
    let mut client = superops_protos::k8s::v1::k8s_service_client::K8sServiceClient::connect(
        endpoint,
    )
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
    Ok(Json(serde_json::json!({ "cluster_id": cid, "nodes": nodes })))
}
```

- [ ] **Step 5: 运行测试验证**

Run: `cargo test -p superops-gateway registry`
Expected: PASS — 3 passed（`empty_discovery_uses_fallback` / `first_endpoint_wins` / `service_without_endpoints_falls_back`）

---

### Task 3: Consul 注册（gateway/k8s/collector）+ gateway 发现

**Files:**
- Modify: `services/gateway/src/main.rs`（on_start 注册 + discover）
- Modify: `services/gateway/src/config.rs`（`ConsulConfig` + `#[serde(default)] consul`）
- Modify: `services/gateway/Cargo.toml`（+ ecat-registry、ecat-registry-consul）
- Modify: `services/k8s/src/main.rs`（on_start 注册）
- Modify: `services/k8s/src/config.rs`（derive Clone + `ConsulConfig`）
- Modify: `services/k8s/Cargo.toml`（+ ecat-registry、ecat-registry-consul）
- Modify: `services/collector/src/main.rs`（on_start 内注册）
- Modify: `services/collector/src/config.rs`（`ConsulConfig`）
- Modify: `services/collector/Cargo.toml`（+ ecat-registry、ecat-registry-consul）
- Modify: `config/gateway.yaml`、`config/k8s-service.yaml`、`config/collector.yaml`（`consul:` 块）

- [ ] **Step 1: 三个 Cargo.toml 加依赖**

gateway `[dependencies]`（`ecat-circuit-breaker` 后）：

```toml
ecat-registry = { path = "../../ecat-registry" }
ecat-registry-consul = { path = "../../ecat-registry-consul" }
```

k8s `[dependencies]`（`ecat-transport-grpc` 后）：

```toml
ecat-registry = { path = "../../ecat-registry" }
ecat-registry-consul = { path = "../../ecat-registry-consul" }
```

collector `[dependencies]`（`ecat-mq-kafka` 后）：

```toml
ecat-registry = { path = "../../ecat-registry" }
ecat-registry-consul = { path = "../../ecat-registry-consul" }
```

- [ ] **Step 2: 三个 config.rs 加 ConsulConfig**

gateway `config.rs`（`RedisConfig` struct 后加）：

```rust
#[derive(Debug, Deserialize, Clone)]
pub struct ConsulConfig {
    pub address: String,
}
```

`Config` struct `mq` 行后加：

```rust
    #[serde(default)]
    pub consul: Option<ConsulConfig>,
```

k8s `config.rs`（改 derive + 加 struct）：

```rust
#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub server: ServerConfig,
    // Phase 2 (persistence) — deserialized from YAML, not yet read
    #[allow(dead_code)]
    pub database: DatabaseConfig,
    #[serde(default)]
    pub consul: Option<ConsulConfig>,
}
```

`ServerConfig`、`DatabaseConfig` 的 derive 同样加 `Clone`。文件末尾加：

```rust
#[derive(Debug, Deserialize, Clone)]
pub struct ConsulConfig {
    pub address: String,
}
```

collector `config.rs`（`K8sConfig` 前加）：

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct ConsulConfig {
    pub address: String,
}
```

`Config` struct `collector` 行后加：

```rust
    #[serde(default)]
    pub consul: Option<ConsulConfig>,
```

- [ ] **Step 3: 三个 YAML 加 consul 块**

`config/gateway.yaml`、`config/k8s-service.yaml`、`config/collector.yaml` 末尾各加：

```yaml
consul:
  address: "http://localhost:8500"
```

- [ ] **Step 4: k8s main.rs 注册**

`services/k8s/src/main.rs` 完整替换为：

```rust
mod cluster;
mod config;
mod resource;
mod service;

use ecat::App;
use ecat_registry::{Registration, ServiceInfo};
use ecat_registry_consul::ConsulRegistry;
use ecat_transport_grpc::GrpcServer;
use superops_protos::k8s::v1::k8s_service_server::K8sServiceServer;
use std::sync::{Arc, Mutex};

use crate::cluster::manager::ClusterManager;
use crate::config::Config;
use crate::service::K8sServiceImpl;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::load()?;
    let manager = ClusterManager::new();

    let grpc = GrpcServer::new(format!("0.0.0.0:{}", config.server.grpc_port)).routes(
        tonic::service::Routes::new(K8sServiceServer::new(K8sServiceImpl { manager })),
    );

    let reg_holder = Arc::new(Mutex::new(None::<Registration>));
    let reg_start = Arc::clone(&reg_holder);
    let cfg_start = config.clone();

    let mut app = App::builder()
        .name("superops-k8s")
        .version(env!("CARGO_PKG_VERSION"))
        .server(grpc)
        .on_start(move || {
            let reg = Arc::clone(&reg_start);
            let cfg = cfg_start.clone();
            async move {
                if let Some(consul) = &cfg.consul {
                    let registry = ConsulRegistry::new(&consul.address);
                    let info = ServiceInfo::new("superops-k8s", env!("CARGO_PKG_VERSION"))
                        .with_endpoint(format!("http://localhost:{}", cfg.server.grpc_port));
                    let registration = registry.register(info).await?;
                    tracing::info!(service = "superops-k8s", "registered in consul");
                    *reg.lock().unwrap() = Some(registration);
                }
                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
            }
        })
        .build()
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    app.run().await.map_err(|e| anyhow::anyhow!("{e}"))?;

    Ok(())
}
```

- [ ] **Step 5: collector main.rs 注册**

`services/collector/src/main.rs`：
- `use std::sync::{Arc, Mutex};` 后加 `use ecat_registry::{Registration, ServiceInfo};`、`use ecat_registry_consul::ConsulRegistry;`
- `let sched_stop = Arc::clone(&scheduler);` 后加：

```rust
    let reg_holder = Arc::new(Mutex::new(None::<Registration>));
    let reg_start = Arc::clone(&reg_holder);
```

- on_start 闭包内 `let cfg = cfg_start.clone();` 后加 `let reg = Arc::clone(&reg_start);`，async move 块开头加：

```rust
                if let Some(consul) = &cfg.consul {
                    let registry = ConsulRegistry::new(&consul.address);
                    let info = ServiceInfo::new("superops-collector", env!("CARGO_PKG_VERSION"));
                    let registration = registry.register(info).await?;
                    tracing::info!(service = "superops-collector", "registered in consul");
                    *reg.lock().unwrap() = Some(registration);
                }
```

- [ ] **Step 6: gateway main.rs 注册 + 发现（含 T4 的 watcher 一起接线，见 Task 4 Step 4 合并执行）**

`services/gateway/src/main.rs`：
- `use std::sync::{Arc, RwLock};` 改为 `use std::sync::{Arc, Mutex, RwLock};`
- `use crate::config::Config;` 前加：

```rust
use ecat_config_remote::ConsulConfigSource;
use ecat_registry::{Registration, ServiceInfo};
use ecat_registry_consul::ConsulRegistry;
```

- [ ] **Step 7: 编译验证**

Run: `cargo check -p superops-gateway -p superops-k8s -p superops-collector`
Expected: 编译通过、零警告（on_start 闭包为 Fn + clone-in-body 模式，无 E0382/E0507）

---

### Task 4: 远程配置热更新（config_remote.rs）

**Files:**
- Create: `services/gateway/src/config_remote.rs`（4 个 tests）
- Modify: `services/gateway/Cargo.toml`（+ ecat-config-remote、ecat-config、async-trait）
- Modify: `services/gateway/src/main.rs`（`mod config_remote;` + DynamicConfig + watcher spawn）

- [ ] **Step 1: 加依赖**

gateway `[dependencies]`（`ecat-registry-consul` 后）：

```toml
ecat-config = { path = "../../ecat-config" }
ecat-config-remote = { path = "../../ecat-config-remote" }
async-trait.workspace = true
```

- [ ] **Step 2: 写 config_remote.rs（实现 + 4 个测试）**

`services/gateway/src/config_remote.rs` 完整内容（`#[derive(Default)]` 会给出 0 → 拒绝全部请求，必须显式 Default）：

```rust
use ecat_config::ConfigError;
use ecat_middleware::RateLimitStore;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub struct DynamicConfig {
    pub rate_limit_max: RwLock<u32>,
    pub rate_limit_window_secs: RwLock<u64>,
}

impl Default for DynamicConfig {
    fn default() -> Self {
        Self {
            rate_limit_max: RwLock::new(10),
            rate_limit_window_secs: RwLock::new(60),
        }
    }
}

pub fn apply_dynamic(
    values: &HashMap<String, serde_json::Value>,
    cfg: &DynamicConfig,
) -> Vec<String> {
    let mut applied = Vec::new();
    if let Some(v) = values.get("rate.limit.max") {
        if let Some(max) = v.as_u64() {
            *cfg.rate_limit_max.write().unwrap() = max as u32;
            applied.push(format!("rate.limit.max={max}"));
        }
    }
    if let Some(v) = values.get("rate.limit.window") {
        if let Some(secs) = v.as_u64() {
            *cfg.rate_limit_window_secs.write().unwrap() = secs;
            applied.push(format!("rate.limit.window={secs}"));
        }
    }
    applied
}

pub struct DynamicRateLimitStore {
    inner: Arc<dyn RateLimitStore>,
    dynamic: Arc<DynamicConfig>,
}

impl DynamicRateLimitStore {
    pub fn new(inner: Arc<dyn RateLimitStore>, dynamic: Arc<DynamicConfig>) -> Self {
        Self { inner, dynamic }
    }
}

#[async_trait::async_trait]
impl RateLimitStore for DynamicRateLimitStore {
    async fn check(&self, key: &str, _max: u32, _window_secs: u64) -> Result<(), String> {
        let max = *self.dynamic.rate_limit_max.read().unwrap();
        let window_secs = *self.dynamic.rate_limit_window_secs.read().unwrap();
        self.inner.check(key, max, window_secs).await
    }
}

pub async fn run_config_watcher(
    mut rx: tokio::sync::mpsc::Receiver<Result<HashMap<String, serde_json::Value>, ConfigError>>,
    dynamic: Arc<DynamicConfig>,
) {
    while let Some(update) = rx.recv().await {
        match update {
            Ok(values) => {
                let applied = apply_dynamic(&values, &dynamic);
                if !applied.is_empty() {
                    tracing::info!(applied = ?applied, "dynamic config applied");
                }
            }
            Err(e) => tracing::warn!(error = %e, "config watch error"),
        }
    }
    tracing::info!("config watcher channel closed; exiting");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let cfg = DynamicConfig::default();
        assert_eq!(*cfg.rate_limit_max.read().unwrap(), 10);
        assert_eq!(*cfg.rate_limit_window_secs.read().unwrap(), 60);
    }

    #[test]
    fn apply_updates_values() {
        let cfg = DynamicConfig::default();
        let mut values = HashMap::new();
        values.insert("rate.limit.max".into(), serde_json::json!(20));
        values.insert("rate.limit.window".into(), serde_json::json!(30));
        let applied = apply_dynamic(&values, &cfg);
        assert_eq!(applied.len(), 2);
        assert_eq!(*cfg.rate_limit_max.read().unwrap(), 20);
        assert_eq!(*cfg.rate_limit_window_secs.read().unwrap(), 30);
    }

    #[test]
    fn apply_ignores_unknown_and_bad_types() {
        let cfg = DynamicConfig::default();
        let mut values = HashMap::new();
        values.insert("rate.limit.max".into(), serde_json::json!("abc"));
        values.insert("unknown.key".into(), serde_json::json!(5));
        let applied = apply_dynamic(&values, &cfg);
        assert!(applied.is_empty());
        assert_eq!(*cfg.rate_limit_max.read().unwrap(), 10);
        assert_eq!(*cfg.rate_limit_window_secs.read().unwrap(), 60);
    }

    #[tokio::test]
    async fn dynamic_store_delegates_with_live_values() {
        let dynamic = Arc::new(DynamicConfig::default());
        *dynamic.rate_limit_max.write().unwrap() = 2;
        let inner: Arc<dyn RateLimitStore> = Arc::new(ecat_middleware::MemoryStore::new());
        let store = DynamicRateLimitStore::new(inner, dynamic);
        assert!(store.check("ip-1", 0, 0).await.is_ok());
        assert!(store.check("ip-1", 0, 0).await.is_ok());
        assert!(store.check("ip-1", 0, 0).await.is_err());
    }
}
```

- [ ] **Step 3: 运行测试验证**

Run: `cargo test -p superops-gateway config_remote`
Expected: PASS — 4 passed（`defaults_are_sane` / `apply_updates_values` / `apply_ignores_unknown_and_bad_types` / `dynamic_store_delegates_with_live_values`）

- [ ] **Step 4: main.rs 接线（DynamicConfig + watcher spawn，与 T3 Step 6 的 on_start 合并）**

`services/gateway/src/main.rs`：
- `mod registry;` 后加 `mod config_remote;`
- `let config = Config::load()?;` 后加：

```rust
    let dynamic_cfg = Arc::new(crate::config_remote::DynamicConfig::default());
```

- App::builder() 前加（reg_holder/state/cfg/dynamic 的 clone 源）：

```rust
    let reg_holder = Arc::new(Mutex::new(None::<Registration>));
    let reg_start = Arc::clone(&reg_holder);
    let state_start = state.clone();
    let cfg_start = config.clone();
    let dynamic_start = Arc::clone(&dynamic_cfg);
```

- App::builder() 链加 on_start（`http` 后）：

```rust
        .on_start(move || {
            let reg = Arc::clone(&reg_start);
            let state = state_start.clone();
            let cfg = cfg_start.clone();
            let dynamic = Arc::clone(&dynamic_start);
            async move {
                if let Some(consul) = &cfg.consul {
                    let registry = ConsulRegistry::new(&consul.address);
                    let info = ServiceInfo::new("superops-gateway", env!("CARGO_PKG_VERSION"))
                        .with_endpoint(format!("http://localhost:{}", cfg.server.http_port));
                    let registration = registry.register(info).await?;
                    tracing::info!(service = "superops-gateway", "registered in consul");
                    *reg.lock().unwrap() = Some(registration);

                    match registry.discover("superops-k8s").await {
                        Ok(discovered) => {
                            let endpoint = crate::registry::resolve_k8s_endpoint(
                                &discovered,
                                &cfg.services.k8s.endpoint,
                            );
                            tracing::info!(
                                endpoint = %endpoint,
                                source = if discovered.is_empty() { "static" } else { "consul" },
                                "resolved k8s backend"
                            );
                            *state.k8s_endpoint.write().unwrap() = endpoint;
                        }
                        Err(e) => tracing::warn!(
                            error = %e,
                            "k8s discovery failed; keeping static endpoint"
                        ),
                    }

                    let source =
                        ConsulConfigSource::new(&consul.address, "config/superops/gateway");
                    tokio::spawn(crate::config_remote::run_config_watcher(
                        source.watch(),
                        dynamic,
                    ));
                }
                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
            }
        })
```

- [ ] **Step 5: 编译验证**

Run: `cargo check -p superops-gateway`
Expected: 编译通过（`ecat_config::ConfigError` 类型别名解析、`source.watch()` 返回 mpsc Receiver 与签名匹配）

---

### Task 5: 限流迁移 ecat-middleware（删旧自研）

**Files:**
- Delete: `services/gateway/src/auth/rate_limit.rs`
- Modify: `services/gateway/src/auth/mod.rs`（去掉 `pub mod rate_limit;`）
- Modify: `services/gateway/Cargo.toml`（+ ecat-middleware `redis` feature）
- Modify: `services/gateway/src/config.rs`（去掉 redis `#[allow(dead_code)]`）
- Modify: `services/gateway/src/main.rs`（store 选择 + login 链）

- [ ] **Step 1: 加依赖**

gateway `[dependencies]`（`async-trait.workspace = true` 后）：

```toml
ecat-middleware = { path = "../../ecat-middleware", features = ["redis"] }
```

- [ ] **Step 2: 删旧文件 + 清 mod 声明**

Run: `rm services/gateway/src/auth/rate_limit.rs`
`services/gateway/src/auth/mod.rs` 替换为：

```rust
pub mod handler;
pub mod middleware;
```

- [ ] **Step 3: config.rs 解除 redis dead_code**

`services/gateway/src/config.rs`：`pub redis: RedisConfig` 前的 `#[allow(dead_code)]` 删除（T5 后 `config.redis.url` 被 `RedisRateLimitStore::connect` 读取）。`RedisConfig.url` 字段上的 `#[allow(dead_code)]` 也删除。

- [ ] **Step 4: main.rs 接线**

`services/gateway/src/main.rs`：
- 删除第 11 行 `use crate::auth::rate_limit::{RateLimiter, rate_limit_login};`
- `use crate::config::Config;` 前加：

```rust
use ecat_middleware::{MemoryStore, RateLimitLayer, RateLimitStore, RedisRateLimitStore};
```

- AppState 删除 `pub rate_limiter: RateLimiter,` 字段；state 构造删除 `rate_limiter: RateLimiter::default(),` 行
- `let dynamic_cfg = Arc::new(...)` 后加 store 选择：

```rust
    let inner: Arc<dyn RateLimitStore> = match RedisRateLimitStore::connect(&config.redis.url).await {
        Ok(store) => Arc::new(store),
        Err(e) => {
            tracing::warn!(error = %e, "redis rate-limit store unavailable; falling back to in-memory");
            Arc::new(MemoryStore::new())
        }
    };
    let rate_store = Arc::new(crate::config_remote::DynamicRateLimitStore::new(
        inner,
        Arc::clone(&dynamic_cfg),
    ));
```

- 替换第 66-69 行 login 链：

```rust
    let login_limited = axum::Router::new()
        .route("/api/auth/login", post(login))
        .layer(
            tower::ServiceBuilder::new()
                .layer(crate::breaker::ErrorToResponseLayer)
                .layer(
                    RateLimitLayer::new(10, std::time::Duration::from_secs(60))
                        .with_store(rate_store)
                        .with_key_fn(|req: &axum::http::Request<axum::body::Body>| {
                            req.headers()
                                .get("x-forwarded-for")
                                .and_then(|v| v.to_str().ok())
                                .and_then(|v| v.split(',').next().map(str::trim))
                                .filter(|s| !s.is_empty())
                                .or_else(|| {
                                    req.headers()
                                        .get("x-real-ip")
                                        .and_then(|v| v.to_str().ok())
                                })
                                .unwrap_or("global")
                                .to_string()
                        }),
                ),
        );
```

> 注：`RateLimitLayer::new` 产生 `RateLimitLayer<()>`，axum Router 的 body 是 `axum::body::Body`——必须 `with_key_fn` 显式指定 `B2 = Body`。且 `#[derive(Clone)]` 生成的 `Clone` 要求 `B: Clone`（axum Body 不满足），故在 `ecat-middleware/src/ratelimit.rs` 为 `RateLimitLayer`/`RateLimitService` 改手写 `Clone` 实现（仅要求 `S: Clone`），并新增回归测试 `layer_and_service_clone_with_non_clone_body`。

- 第 73 行 `.route("/api/auth/login", login_limited)` 改为 `.merge(login_limited)`

- [ ] **Step 5: 运行测试验证**

Run: `cargo test -p superops-gateway`
Expected: PASS — 24 passed（6 原有 + 11 breaker + 3 registry + 4 config_remote）；无 `rate_limit` 模块编译错误

---

### Task 6: OTLP 追踪 + jaeger

**Files:**
- Modify: `services/gateway/Cargo.toml`、`services/k8s/Cargo.toml`（+ ecat-tracing-otlp）
- Modify: `services/gateway/src/config.rs`、`services/k8s/src/config.rs`（`#[serde(default)] otlp`）
- Modify: `services/gateway/src/main.rs`、`services/k8s/src/main.rs`（`ecat_tracing_otlp::init`，provider 绑定 main 作用域）
- Modify: `config/gateway.yaml`、`config/k8s-service.yaml`（`otlp:`）
- Modify: `deploy/docker-compose.yml`（+ jaeger）

- [ ] **Step 1: 加依赖**

gateway `[dependencies]`（`ecat-middleware` 后）：

```toml
ecat-tracing-otlp = { path = "../../ecat-tracing-otlp" }
```

k8s `[dependencies]`（`ecat-registry-consul` 后）：

```toml
ecat-tracing-otlp = { path = "../../ecat-tracing-otlp" }
```

- [ ] **Step 2: config.rs + YAML**

gateway `config.rs` `Config` struct `consul` 行后加：

```rust
    #[serde(default)]
    pub otlp: Option<String>,
```

k8s `config.rs` `Config` struct `consul` 行后加：

```rust
    #[serde(default)]
    pub otlp: Option<String>,
```

`config/gateway.yaml` 末尾加：

```yaml
otlp: "http://localhost:4317"
```

`config/k8s-service.yaml` 末尾加：

```yaml
otlp: "http://localhost:4317"
```

- [ ] **Step 3: 两个 main 接 OTLP（App build 前）**

gateway `main.rs` `let config = Config::load()?;` 后加：

```rust
    let _otlp = match &config.otlp {
        Some(endpoint) => Some(
            ecat_tracing_otlp::init("superops-gateway", endpoint)
                .map_err(|e| anyhow::anyhow!("otlp init: {e}"))?,
        ),
        None => None,
    };
```

k8s `main.rs` `let config = Config::load()?;` 后加：

```rust
    let _otlp = match &config.otlp {
        Some(endpoint) => Some(
            ecat_tracing_otlp::init("superops-k8s", endpoint)
                .map_err(|e| anyhow::anyhow!("otlp init: {e}"))?,
        ),
        None => None,
    };
```

- [ ] **Step 4: compose 加 jaeger**

`deploy/docker-compose.yml` `consul` 服务块后（`volumes:` 前）加：

```yaml
  jaeger:
    image: jaegertracing/all-in-one:1.58
    ports: ["4317:4317", "16686:16686"]
    environment:
      COLLECTOR_OTLP_ENABLED: "true"
```

- [ ] **Step 5: 编译验证**

Run: `cargo check -p superops-gateway -p superops-k8s`
Expected: 编译通过（`_otlp` 绑定 main 作用域覆盖 `app.run()` 期间）

---

### Task 7: 文档 + 全量验证

**Files:**
- Modify: `CHANGELOG.md`（[1.1.0] 条目）
- Modify: `docs/superpowers/plans/2026-08-06-super-ops-ecosystem-expansion.md`（P2 复选框翻转）

- [ ] **Step 1: CHANGELOG**

`CHANGELOG.md` 顶部（`# Changelog` 后、`## [1.0.5]` 前）插入：

```markdown
## [1.1.0] — 2026-08-06 — SuperOps P2 韧性与可观测

### Added
- gateway 接入 ecat-circuit-breaker 熔断：自定义 `FiveXxToErrorLayer` 将上游 5xx 转为 Service 错误，k8s 代理路由 5 次失败后熔断 10s（失败率 50% / 窗口 30s / 半开探针 3），熔断期间返回 503
- Consul 服务注册与发现（ecat-registry + ecat-registry-consul）：gateway/k8s/collector 启动即注册（Drop 自动注销），gateway 启动时 `discover("superops-k8s")` 解析真实 gRPC 地址，失败回退静态配置
- 远程配置热更新（ecat-config-remote + ecat-config）：gateway 订阅 Consul KV `config/superops/gateway/*`（阻塞查询 + 首帧强制推送），`rate.limit.max`/`rate.limit.window` 变更即时生效（`DynamicRateLimitStore` 动态取值包装）
- 限流迁移 ecat-middleware：`RateLimitLayer` + `RedisRateLimitStore`（Redis 不可用回退内存 store 并 WARN），多实例共享计数；429 文案沿用
- OTLP 链路追踪（ecat-tracing-otlp）：gateway/k8s 导出至 Jaeger（compose 新增 jaeger 4317/16686）
- k8s `/api/k8s/clusters/{cluster_id}/nodes` 由 stub 改为真实 gRPC 代理转发（后端不可达 502，触发熔断计数）

### Removed
- 旧内存限流 `services/gateway/src/auth/rate_limit.rs`（由 ecat-middleware 限流替代）
```

- [ ] **Step 2: 总纲 P2 复选框翻转**

Run: `sed -i '162,199s/^- \[ \]/- [x]/' docs/superpowers/plans/2026-08-06-super-ops-ecosystem-expansion.md`
Expected: 第 168/175/182/189/196 行的 `- [ ]` 变为 `- [x]`

- [ ] **Step 3: 全量验证**

Run: `cargo fmt --check`
Expected: 无输出（如有格式差异：`cargo fmt` 后再跑一次）

Run: `cargo test -p superops-gateway -p superops-collector`
Expected: PASS — gateway 24 passed、collector 9 passed

Run: `cargo test --workspace`
Expected: 全部 passed（ecat 生态 crate 自带测试不受影响）

Run: `cargo build --workspace`
Expected: 构建成功

- [ ] **Step 4: 待运行时复核清单（sudo docker compose up -d 后）**

1. 熔断：停止 k8s 服务进程后 `curl -s -o /dev/null -w '%{http_code}\n' http://localhost:8080/api/k8s/clusters/demo/nodes -H "Authorization: Bearer <token>"` 连续 6 次 → 502，第 7 次 → 503（"circuit breaker is open"）；恢复 k8s 服务 10s 后 half-open 自愈
2. Consul 注册：`curl -s http://localhost:8500/v1/health/service/superops-gateway?passing=true` 有 entry；SIGTERM gateway 后 entry 自动消失
3. 热更新：`consul kv put config/superops/gateway/rate.limit.max 20` → gateway 日志出现 `dynamic config applied`；第 21 次错误登录才 429
4. Jaeger：浏览器 http://localhost:16686 → 选择 superops-gateway / superops-k8s 可见 span
5. 限流：连续 11 次错误登录 → 第 11 次返回 429（`rate limit exceeded`）

---

## Self-Review

**1. Spec 覆盖（对照总纲 P2）：**
- P2.1 熔断 → T1（breaker.rs + FiveXxToErrorLayer + k8s 链）；验收"停 k8s service → 503"依赖 T2 真实代理（stub 不产生 5xx）
- P2.2 Consul 注册/发现 → T3（三服务注册 + gateway discover + 静态回退）；"kill 进程后自动注销"由 Registration Drop 保证
- P2.3 远程配置 → T4（ConsulConfigSource::watch + DynamicRateLimitStore + run_config_watcher）
- P2.4 OTLP → T6（两个服务 + jaeger compose + provider 保活）
- P2.5 限流迁移 → T5（删旧 + RateLimitLayer + Redis store fail-open 回退）

**2. 范围收窄说明（需向用户说明）：** 总纲 P2.3 写"热更新限流阈值/熔断参数"，但 `RateLimitLayer` 与 `CircuitBreakerLayer` 的 max/window 参数均在 layer 构造时冻结 → 熔断参数保持静态，仅限流阈值/窗口经 `DynamicRateLimitStore` 热更新。

**3. Placeholder 扫描：** 全部步骤含完整代码与精确命令；无 TBD/TODO；无"类似 Task N"。

**4. 类型一致性：**
- `error_to_response` 签名 `Box<dyn Error + Send + Sync> -> Response`，由 `ErrorToResponseService` 在 call 内调用（最外层 Error=Infallible 满足 axum `Router::layer` 的 `Into<Infallible>` 约束）
- `resolve_k8s_endpoint(&[ServiceInfo], &str) -> String` 在 T2 定义、T3 使用一致
- `DynamicRateLimitStore::new(Arc<dyn RateLimitStore>, Arc<DynamicConfig>)` 在 T4 定义、T5 使用一致
- `run_config_watcher` 的 Receiver 泛型与 `ConsulConfigSource::watch()` 返回类型一致（ConfigError 来自 ecat-config，依赖已加）
- `breaker_layer()`/`RateLimitLayer::new(10, 60s)` 的冻结值与 `DynamicConfig` 默认值一致（10/60）
- gateway 测试计数：3（metrics_api）+ 10（breaker）+ 3（registry）+ 4（config_remote）= 20
