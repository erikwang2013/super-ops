# SuperOps Phase 1 MVP 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 构建 SuperOps MVP — 用户可登录、管理多 K8s 集群、查看资源和日志、使用 Web Terminal，并通过 Tauri 打包为桌面应用。

**Architecture:** e-cat 微服务架构，API Gateway（HTTP/gRPC 双协议 + JWT 认证）转发请求到 K8s Service，K8s Service 通过 kube-rs 与集群交互。前端 React + Ant Design Pro，通过 HTTP/WebSocket 与 Gateway 通信，Tauri 2.x 打包为桌面端。

**Tech Stack:** e-cat v2.1.7 (Rust/axum/tonic), kube-rs, MySQL 8.0, Redis, React 18 + TypeScript, Vite, Ant Design 5 Pro, xterm.js, Tauri 2.x

---

## File Structure

```
super-ops/
├── protos/
│   ├── common/v1/common.proto     # Timestamp, Pagination, ErrorCode
│   └── k8s/v1/k8s.proto          # K8s Service gRPC API
├── services/
│   ├── gateway/                   # e-cat API Gateway
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs            # HTTP + gRPC server bootstrap
│   │       ├── config.rs          # Gateway configuration
│   │       ├── auth/
│   │       │   ├── mod.rs
│   │       │   ├── middleware.rs  # JWT verification Layer
│   │       │   └── handler.rs     # login/register/refresh handlers
│   │       ├── model/
│   │       │   ├── mod.rs
│   │       │   └── user.rs        # User entity + DB operations
│   │       └── proxy/
│   │           ├── mod.rs
│   │           └── k8s_proxy.rs   # K8s gRPC client → HTTP routes
│   └── k8s/                       # e-cat K8s Service
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs            # gRPC server bootstrap
│           ├── config.rs
│           ├── cluster/
│           │   ├── mod.rs
│           │   ├── manager.rs     # Multi-cluster connection pool
│           │   └── client.rs      # Single cluster kube-rs wrapper
│           ├── resource/
│           │   ├── mod.rs
│           │   ├── pod.rs         # Pod list/logs/watch/exec
│           │   ├── deploy.rs      # Deployment list
│           │   ├── node.rs        # Node list
│           │   └── metrics.rs     # Metrics API wrapper
│           └── service.rs         # gRPC service impl
├── frontend/
│   ├── package.json
│   ├── vite.config.ts
│   ├── index.html
│   ├── src/
│   │   ├── main.tsx               # App entry
│   │   ├── App.tsx                # Router + ProLayout shell
│   │   ├── services/
│   │   │   ├── api.ts             # HTTP client (fetch + JWT)
│   │   │   └── k8s.ts             # K8s API calls
│   │   ├── stores/
│   │   │   └── auth.ts            # Zustand auth store
│   │   ├── pages/
│   │   │   ├── login.tsx
│   │   │   ├── dashboard.tsx
│   │   │   └── k8s/
│   │   │       ├── clusters.tsx
│   │   │       ├── cluster-detail.tsx
│   │   │       ├── pods.tsx
│   │   │       ├── deployments.tsx
│   │   │       ├── nodes.tsx
│   │   │       └── terminal.tsx
│   │   └── components/
│   │       └── pod-logs.tsx
│   └── src-tauri/
│       ├── Cargo.toml
│       ├── tauri.conf.json
│       ├── build.rs
│       └── src/
│           ├── lib.rs             # Tauri commands + system tray
│           └── main.rs
├── config/
│   ├── gateway.yaml
│   └── k8s-service.yaml
├── deploy/
│   ├── docker-compose.yml
│   └── init.sql
└── Makefile
```

---

### Task 1: Initialize project monorepo

**Files:**
- Create: `Makefile`
- Create: `config/gateway.yaml`
- Create: `config/k8s-service.yaml`
- Create: `.gitignore`

- [ ] **Step 1: Write Makefile**

```makefile
.PHONY: proto gateway k8s-service frontend dev clean dev-all stop-all

proto:
	cd protos && buf generate

gateway:
	cd services/gateway && cargo build

k8s-service:
	cd services/k8s && cargo build

frontend:
	cd frontend && npm run dev

dev: proto
	$(MAKE) gateway && $(MAKE) k8s-service

dev-all:
	cd deploy && docker compose up -d
	sleep 5
	cd services/gateway && cargo run &
	sleep 2
	cd services/k8s && cargo run &
	sleep 2
	cd frontend && npm run dev

stop-all:
	cd deploy && docker compose down
	pkill -f "superops-gateway" || true
	pkill -f "superops-k8s" || true

clean:
	cargo clean
	rm -rf frontend/dist frontend/node_modules
```

- [ ] **Step 2: Write config templates**

```yaml
# config/gateway.yaml
server:
  http_port: 8080
  grpc_port: 9090

auth:
  jwt_secret: "change-me-in-production"
  access_token_ttl: 1800
  refresh_token_ttl: 604800

database:
  url: "mysql://superops:superops@localhost:3306/superops"

redis:
  url: "redis://localhost:6379"

services:
  k8s:
    endpoint: "http://localhost:9091"
```

```yaml
# config/k8s-service.yaml
server:
  grpc_port: 9091

database:
  url: "mysql://superops:superops@localhost:3306/superops"
```

- [ ] **Step 3: Write .gitignore**

```
target/
node_modules/
dist/
.env
*.local
.superpowers/
frontend/src-tauri/target/
```

- [ ] **Step 4: Commit**

```bash
git add Makefile config/ .gitignore
git commit -m "chore: initialize monorepo with config templates and Makefile"
```

---

### Task 2: Define Protobuf schemas

**Files:**
- Create: `protos/common/v1/common.proto`
- Create: `protos/k8s/v1/k8s.proto`
- Create: `protos/buf.yaml`
- Create: `protos/buf.gen.yaml`

- [ ] **Step 1: Write buf config**

```yaml
# protos/buf.yaml
version: v2
modules:
  - path: common/v1
  - path: k8s/v1
lint:
  use:
    - STANDARD
```

```yaml
# protos/buf.gen.yaml
version: v2
plugins:
  - plugin: prost
    out: gen/rust
    opt:
      - compile_well_known_types
      - extern_path=.google.protobuf=::pbjson_types
```

- [ ] **Step 2: Write common proto**

```protobuf
// protos/common/v1/common.proto
syntax = "proto3";

package common.v1;

message Pagination {
  int32 page = 1;
  int32 page_size = 2;
}

message PaginatedResponse {
  int32 total = 1;
  int32 page = 2;
  int32 page_size = 3;
}

message Error {
  int32 code = 1;
  string message = 2;
  map<string, string> details = 3;
}
```

- [ ] **Step 3: Write K8s proto**

```protobuf
// protos/k8s/v1/k8s.proto
syntax = "proto3";

package k8s.v1;

import "google/protobuf/timestamp.proto";
import "common/v1/common.proto";

service K8sService {
  rpc ListClusters(ListClustersRequest) returns (ListClustersResponse);
  rpc GetCluster(GetClusterRequest) returns (GetClusterResponse);
  rpc AddCluster(AddClusterRequest) returns (AddClusterResponse);
  rpc RemoveCluster(RemoveClusterRequest) returns (RemoveClusterResponse);

  rpc ListPods(ListPodsRequest) returns (ListPodsResponse);
  rpc ListDeployments(ListDeploymentsRequest) returns (ListDeploymentsResponse);
  rpc ListNodes(ListNodesRequest) returns (ListNodesResponse);

  rpc WatchResources(WatchResourcesRequest) returns (stream WatchEvent);
  rpc GetPodLogs(GetPodLogsRequest) returns (stream LogLine);
  rpc ExecPod(stream ExecRequest) returns (stream ExecResponse);

  rpc GetMetrics(GetMetricsRequest) returns (GetMetricsResponse);
}

message Cluster {
  string id = 1;
  string name = 2;
  string version = 3;
  int32 node_count = 4;
  int32 pod_count = 5;
  string status = 6;
  google.protobuf.Timestamp created_at = 7;
}

message ListClustersRequest {}
message ListClustersResponse { repeated Cluster clusters = 1; }

message GetClusterRequest { string cluster_id = 1; }
message GetClusterResponse { Cluster cluster = 1; }

message AddClusterRequest {
  string name = 1;
  bytes kubeconfig = 2;
}
message AddClusterResponse { Cluster cluster = 1; }

message RemoveClusterRequest { string cluster_id = 1; }
message RemoveClusterResponse {}

message Pod {
  string name = 1;
  string namespace = 2;
  string status = 3;
  string node = 4;
  int32 restarts = 5;
  string age = 6;
  repeated ContainerInfo containers = 7;
  map<string, string> labels = 8;
}

message ContainerInfo {
  string name = 1;
  string image = 2;
  bool ready = 3;
}

message ListPodsRequest {
  string cluster_id = 1;
  string namespace = 2;
  common.v1.Pagination pagination = 3;
}
message ListPodsResponse {
  repeated Pod pods = 1;
  common.v1.PaginatedResponse page = 2;
}

message Deployment {
  string name = 1;
  string namespace = 2;
  int32 replicas = 3;
  int32 ready_replicas = 4;
  int32 updated_replicas = 5;
  string age = 6;
  repeated string images = 7;
}

message ListDeploymentsRequest {
  string cluster_id = 1;
  string namespace = 2;
}
message ListDeploymentsResponse { repeated Deployment deployments = 1; }

message Node {
  string name = 1;
  string status = 2;
  string role = 3;
  string version = 4;
  string age = 5;
  string cpu = 6;
  string memory = 7;
}

message ListNodesRequest { string cluster_id = 1; }
message ListNodesResponse { repeated Node nodes = 1; }

message WatchResourcesRequest {
  string cluster_id = 1;
  string namespace = 2;
  repeated string resource_types = 3;
}

message WatchEvent {
  string event_type = 1;
  string resource_type = 2;
  string resource_name = 3;
  string namespace = 4;
  bytes payload = 5;
}

message GetPodLogsRequest {
  string cluster_id = 1;
  string namespace = 2;
  string pod_name = 3;
  string container = 4;
  int32 tail_lines = 5;
  bool follow = 6;
}

message LogLine {
  string content = 1;
  google.protobuf.Timestamp timestamp = 2;
}

message ExecRequest {
  string cluster_id = 1;
  string namespace = 2;
  string pod_name = 3;
  string container = 4;
  string command = 5;
  bytes stdin = 6;
  TerminalSize terminal_size = 7;
}

message TerminalSize {
  int32 width = 1;
  int32 height = 2;
}

message ExecResponse {
  bytes stdout = 1;
  bytes stderr = 2;
}

message GetMetricsRequest {
  string cluster_id = 1;
  string resource_type = 2;
  string resource_name = 3;
}

message GetMetricsResponse { repeated MetricPoint metrics = 1; }

message MetricPoint {
  string name = 1;
  double value = 2;
  string unit = 3;
  google.protobuf.Timestamp timestamp = 4;
}
```

- [ ] **Step 4: Commit**

```bash
git add protos/
git commit -m "feat: define protobuf schemas for common and K8s service"
```

---

### Task 3: K8s Service — scaffold and cluster management

**Files:**
- Create: `services/k8s/Cargo.toml`
- Create: `services/k8s/src/main.rs`
- Create: `services/k8s/src/config.rs`
- Create: `services/k8s/src/cluster/mod.rs`
- Create: `services/k8s/src/cluster/manager.rs`
- Create: `services/k8s/src/cluster/client.rs`

- [ ] **Step 1: Write Cargo.toml**

```toml
[package]
name = "superops-k8s"
version = "0.1.0"
edition = "2021"

[dependencies]
ecat = "2.1"
ecat-data-sqlx = "2.1"
tonic = "0.12"
prost = "0.13"
tokio = { version = "1", features = ["full"] }
kube = { version = "0.93", features = ["runtime", "derive", "client", "ws"] }
k8s-openapi = { version = "0.22", features = ["v1_29"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yaml = "0.9"
uuid = { version = "1", features = ["v4"] }
anyhow = "1"
tracing = "0.1"
tracing-subscriber = "0.3"
dashmap = "6"
futures = "0.3"
tokio-stream = "0.1"

[build-dependencies]
tonic-build = "0.12"
```

- [ ] **Step 2: Write main.rs — gRPC server bootstrap**

```rust
// services/k8s/src/main.rs
mod cluster;
mod config;
mod resource;
mod service;

use tonic::transport::Server;
use crate::cluster::manager::ClusterManager;
use crate::config::Config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let config = Config::load()?;
    let manager = ClusterManager::new();

    tracing::info!("K8s Service starting on port {}", config.server.grpc_port);

    let addr = format!("0.0.0.0:{}", config.server.grpc_port).parse()?;
    Server::builder()
        .add_service(
            // Generated from protos/k8s/v1/k8s.proto
            // k8s_service_server::K8sServiceServer::new(service::K8sServiceImpl::new(manager))
        )
        .serve(addr)
        .await?;

    Ok(())
}
```

- [ ] **Step 3: Write config.rs**

```rust
// services/k8s/src/config.rs
use anyhow::Result;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
}

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    pub grpc_port: u16,
}

#[derive(Debug, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = std::env::var("K8S_CONFIG")
            .unwrap_or_else(|_| "config/k8s-service.yaml".into());
        let content = std::fs::read_to_string(path)?;
        Ok(serde_yaml::from_str(&content)?)
    }
}
```

- [ ] **Step 4: Write cluster manager**

```rust
// services/k8s/src/cluster/mod.rs
pub mod client;
pub mod manager;
```

```rust
// services/k8s/src/cluster/client.rs
use anyhow::Result;
use kube::{Client, Config};
use std::sync::Arc;

#[derive(Clone)]
pub struct ClusterClient {
    pub id: String,
    pub name: String,
    pub version: String,
    pub client: Client,
}

impl ClusterClient {
    pub async fn new(id: String, name: String, kubeconfig: &[u8]) -> Result<Self> {
        let config = Config::from_custom_kubeconfig(kubeconfig, &Default::default()).await?;
        let version = "unknown".to_string();
        let client = Client::try_from(config)?;
        Ok(Self { id, name, version, client })
    }
}
```

```rust
// services/k8s/src/cluster/manager.rs
use anyhow::Result;
use dashmap::DashMap;
use uuid::Uuid;
use crate::cluster::client::ClusterClient;

pub struct ClusterManager {
    clusters: DashMap<String, ClusterClient>,
}

impl ClusterManager {
    pub fn new() -> Self {
        Self { clusters: DashMap::new() }
    }

    pub async fn add(&self, name: String, kubeconfig: Vec<u8>) -> Result<ClusterClient> {
        let id = Uuid::new_v4().to_string();
        let client = ClusterClient::new(id.clone(), name, &kubeconfig).await?;
        self.clusters.insert(id, client.clone());
        Ok(client)
    }

    pub fn remove(&self, cluster_id: &str) -> Result<()> {
        self.clusters.remove(cluster_id)
            .map(|_| ())
            .ok_or_else(|| anyhow::anyhow!("cluster not found: {}", cluster_id))
    }

    pub fn get(&self, cluster_id: &str) -> Result<ClusterClient> {
        self.clusters.get(cluster_id)
            .map(|c| c.clone())
            .ok_or_else(|| anyhow::anyhow!("cluster not found: {}", cluster_id))
    }

    pub fn list(&self) -> Vec<ClusterClient> {
        self.clusters.iter().map(|entry| entry.value().clone()).collect()
    }
}
```

- [ ] **Step 5: Verify build**

```bash
cd services/k8s && cargo check
```

Expected: Compiles successfully (with warnings about unused imports until service.rs is written).

- [ ] **Step 6: Commit**

```bash
git add services/k8s/
git commit -m "feat(k8s): scaffold service with cluster connection manager"
```

---

### Task 4: K8s Service — resource operations and streaming

**Files:**
- Create: `services/k8s/src/resource/mod.rs`
- Create: `services/k8s/src/resource/pod.rs`
- Create: `services/k8s/src/resource/deploy.rs`
- Create: `services/k8s/src/resource/node.rs`
- Create: `services/k8s/src/resource/metrics.rs`
- Create: `services/k8s/src/service.rs`

- [ ] **Step 1: Write pod resource operations**

```rust
// services/k8s/src/resource/mod.rs
pub mod pod;
pub mod deploy;
pub mod node;
pub mod metrics;
```

```rust
// services/k8s/src/resource/pod.rs
use anyhow::Result;
use futures::StreamExt;
use k8s_openapi::api::core::v1::Pod;
use kube::{Api, ResourceExt};
use kube::api::LogParams;
use kube::runtime::watcher::{watcher, Event};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::cluster::client::ClusterClient;

pub async fn list_pods(client: &ClusterClient, namespace: &str) -> Result<Vec<Pod>> {
    let api: Api<Pod> = if namespace.is_empty() {
        Api::all(client.client.clone())
    } else {
        Api::namespaced(client.client.clone(), namespace)
    };
    Ok(api.list(&Default::default()).await?.items)
}

pub async fn get_pod_logs(
    client: &ClusterClient,
    namespace: &str,
    pod_name: &str,
    container: Option<&str>,
    tail_lines: Option<i64>,
    follow: bool,
) -> Result<impl tokio_stream::Stream<Item = String>> {
    let api: Api<Pod> = Api::namespaced(client.client.clone(), namespace);
    let mut params = LogParams::default();
    params.follow = follow;
    if let Some(c) = container {
        params.container = Some(c.to_string());
    }
    if let Some(t) = tail_lines {
        params.tail_lines = Some(t);
    }

    let log_stream = api.log_stream(pod_name, &params).await?;
    let (tx, rx) = mpsc::channel::<String>(64);
    tokio::spawn(async move {
        let mut stream = log_stream;
        while let Some(line) = stream.next().await {
            if let Ok(line) = line {
                let _ = tx.send(line).await;
            }
        }
    });
    Ok(ReceiverStream::new(rx))
}

pub async fn watch_pods(
    client: &ClusterClient,
    namespace: &str,
) -> Result<impl tokio_stream::Stream<Item = Event<Pod>>> {
    let api: Api<Pod> = if namespace.is_empty() {
        Api::all(client.client.clone())
    } else {
        Api::namespaced(client.client.clone(), namespace)
    };
    let (tx, rx) = mpsc::channel::<Event<Pod>>(64);
    tokio::spawn(async move {
        let mut w = watcher(api, Default::default());
        while let Some(event) = w.next().await {
            if let Ok(event) = event {
                let _ = tx.send(event).await;
            }
        }
    });
    Ok(ReceiverStream::new(rx))
}
```

- [ ] **Step 2: Write deployment and node operations**

```rust
// services/k8s/src/resource/deploy.rs
use anyhow::Result;
use k8s_openapi::api::apps::v1::Deployment;
use kube::Api;
use crate::cluster::client::ClusterClient;

pub async fn list_deployments(
    client: &ClusterClient,
    namespace: &str,
) -> Result<Vec<Deployment>> {
    let api: Api<Deployment> = if namespace.is_empty() {
        Api::all(client.client.clone())
    } else {
        Api::namespaced(client.client.clone(), namespace)
    };
    Ok(api.list(&Default::default()).await?.items)
}
```

```rust
// services/k8s/src/resource/node.rs
use anyhow::Result;
use k8s_openapi::api::core::v1::Node;
use kube::Api;
use crate::cluster::client::ClusterClient;

pub async fn list_nodes(client: &ClusterClient) -> Result<Vec<Node>> {
    let api: Api<Node> = Api::all(client.client.clone());
    Ok(api.list(&Default::default()).await?.items)
}
```

```rust
// services/k8s/src/resource/metrics.rs
use anyhow::Result;
use k8s_openapi::apimachinery::pkg::apis::meta::v1 as meta;
use kube::Api;
use serde::Deserialize;
use crate::cluster::client::ClusterClient;

#[derive(Debug, Deserialize)]
pub struct PodMetrics {
    pub metadata: meta::ObjectMeta,
    pub containers: Vec<ContainerMetric>,
}

#[derive(Debug, Deserialize)]
pub struct ContainerMetric {
    pub name: String,
    pub usage: ResourceUsage,
}

#[derive(Debug, Deserialize)]
pub struct ResourceUsage {
    pub cpu: Option<String>,
    pub memory: Option<String>,
}

pub async fn get_pod_metrics(
    client: &ClusterClient,
    namespace: &str,
) -> Result<Vec<PodMetrics>> {
    let api: Api<PodMetrics> = Api::namespaced(client.client.clone(), namespace);
    Ok(api.list(&Default::default()).await?.items)
}
```

- [ ] **Step 3: Write gRPC service implementation**

```rust
// services/k8s/src/service.rs
use std::pin::Pin;
use futures::StreamExt;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status, Streaming};
use kube::ResourceExt;

use crate::cluster::manager::ClusterManager;
use crate::resource;

// These types come from the generated proto code.
// In production, use: use superops_protos::k8s::v1::*;
// Below are the handwritten implementations that produce these proto types.

type GrpcResult<T> = Result<Response<T>, Status>;

pub struct K8sServiceImpl {
    manager: ClusterManager,
}

impl K8sServiceImpl {
    pub fn new(manager: ClusterManager) -> Self { Self { manager } }

    // ---- Cluster management ----

    pub async fn list_clusters(
        &self,
    ) -> GrpcResult<k8s_v1::ListClustersResponse> {
        let clusters = self.manager.list();
        Ok(Response::new(k8s_v1::ListClustersResponse {
            clusters: clusters.into_iter().map(|c| k8s_v1::Cluster {
                id: c.id,
                name: c.name,
                version: c.version,
                ..Default::default()
            }).collect(),
        }))
    }

    pub async fn add_cluster(
        &self,
        req: k8s_v1::AddClusterRequest,
    ) -> GrpcResult<k8s_v1::AddClusterResponse> {
        let client = self.manager.add(req.name, req.kubeconfig).await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(k8s_v1::AddClusterResponse {
            cluster: Some(k8s_v1::Cluster {
                id: client.id,
                name: client.name,
                ..Default::default()
            }),
        }))
    }

    pub async fn remove_cluster(
        &self,
        req: k8s_v1::RemoveClusterRequest,
    ) -> GrpcResult<k8s_v1::RemoveClusterResponse> {
        self.manager.remove(&req.cluster_id)
            .map_err(|e| Status::not_found(e.to_string()))?;
        Ok(Response::new(k8s_v1::RemoveClusterResponse {}))
    }

    // ---- Pod operations ----

    pub async fn list_pods(
        &self,
        req: k8s_v1::ListPodsRequest,
    ) -> GrpcResult<k8s_v1::ListPodsResponse> {
        let client = self.manager.get(&req.cluster_id)
            .map_err(|e| Status::not_found(e.to_string()))?;
        let pods = resource::pod::list_pods(&client, &req.namespace).await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(k8s_v1::ListPodsResponse {
            pods: pods.into_iter().map(|p| k8s_v1::Pod {
                name: p.name_any(),
                namespace: p.namespace().unwrap_or_default(),
                status: p.status.as_ref()
                    .and_then(|s| s.phase.clone())
                    .unwrap_or_default(),
                ..Default::default()
            }).collect(),
            ..Default::default()
        }))
    }

    pub async fn get_pod_logs(
        &self,
        req: k8s_v1::GetPodLogsRequest,
    ) -> GrpcResult<ReceiverStream<Result<k8s_v1::LogLine, Status>>> {
        let client = self.manager.get(&req.cluster_id)
            .map_err(|e| Status::not_found(e.to_string()))?;
        let stream = resource::pod::get_pod_logs(
            &client, &req.namespace, &req.pod_name,
            if req.container.is_empty() { None } else { Some(&req.container) },
            if req.tail_lines > 0 { Some(req.tail_lines as i64) } else { None },
            req.follow,
        ).await.map_err(|e| Status::internal(e.to_string()))?;

        let mapped = stream.map(|line| Ok(k8s_v1::LogLine {
            content: line,
            ..Default::default()
        }));
        Ok(Response::new(ReceiverStream::new(mapped)))
    }

    pub async fn watch_resources(
        &self,
        req: k8s_v1::WatchResourcesRequest,
    ) -> GrpcResult<ReceiverStream<Result<k8s_v1::WatchEvent, Status>>> {
        let client = self.manager.get(&req.cluster_id)
            .map_err(|e| Status::not_found(e.to_string()))?;
        let pod_stream = resource::pod::watch_pods(&client, &req.namespace).await
            .map_err(|e| Status::internal(e.to_string()))?;

        let (tx, rx) = mpsc::channel::<Result<k8s_v1::WatchEvent, Status>>(128);
        tokio::spawn(async move {
            let mut stream = pod_stream;
            while let Some(event) = stream.next().await {
                let watch_event = match event {
                    kube::runtime::watcher::Event::Applied(pod) => k8s_v1::WatchEvent {
                        event_type: "MODIFIED".into(),
                        resource_type: "pod".into(),
                        resource_name: pod.name_any(),
                        namespace: pod.namespace().unwrap_or_default(),
                        ..Default::default()
                    },
                    kube::runtime::watcher::Event::Deleted(pod) => k8s_v1::WatchEvent {
                        event_type: "DELETED".into(),
                        resource_type: "pod".into(),
                        resource_name: pod.name_any(),
                        namespace: pod.namespace().unwrap_or_default(),
                        ..Default::default()
                    },
                    _ => continue,
                };
                if tx.send(Ok(watch_event)).await.is_err() { break; }
            }
        });
        Ok(Response::new(ReceiverStream::new(rx)))
    }

    // ---- Deployment & Node ----

    pub async fn list_deployments(
        &self,
        req: k8s_v1::ListDeploymentsRequest,
    ) -> GrpcResult<k8s_v1::ListDeploymentsResponse> {
        let client = self.manager.get(&req.cluster_id)
            .map_err(|e| Status::not_found(e.to_string()))?;
        let deps = resource::deploy::list_deployments(&client, &req.namespace).await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(k8s_v1::ListDeploymentsResponse {
            deployments: deps.into_iter().map(|d| k8s_v1::Deployment {
                name: d.name_any(),
                namespace: d.namespace().unwrap_or_default(),
                replicas: d.spec.as_ref().map(|s| s.replicas.unwrap_or(0)).unwrap_or(0),
                ready_replicas: d.status.as_ref()
                    .and_then(|s| s.ready_replicas).unwrap_or(0),
                ..Default::default()
            }).collect(),
        }))
    }

    pub async fn list_nodes(
        &self,
        req: k8s_v1::ListNodesRequest,
    ) -> GrpcResult<k8s_v1::ListNodesResponse> {
        let client = self.manager.get(&req.cluster_id)
            .map_err(|e| Status::not_found(e.to_string()))?;
        let nodes = resource::node::list_nodes(&client).await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(k8s_v1::ListNodesResponse {
            nodes: nodes.into_iter().map(|n| k8s_v1::Node {
                name: n.name_any(),
                status: n.status.as_ref()
                    .and_then(|s| s.conditions.as_ref())
                    .and_then(|c| c.iter().find(|c| c.type_ == "Ready"))
                    .map(|c| if c.status == "True" { "Ready" } else { "NotReady" })
                    .unwrap_or("Unknown")
                    .into(),
                version: n.status.as_ref()
                    .and_then(|s| s.node_info.as_ref())
                    .map(|i| i.kubelet_version.clone())
                    .unwrap_or_default(),
                ..Default::default()
            }).collect(),
        }))
    }
}
```

- [ ] **Step 4: Verify build**

```bash
cd services/k8s && cargo check
```

Expected: Compiles with proto-generated types available.

- [ ] **Step 5: Commit**

```bash
git add services/k8s/src/
git commit -m "feat(k8s): implement pod, deployment, node listing and streaming endpoints"
```

---

### Task 5: API Gateway — scaffold and JWT auth

**Files:**
- Create: `services/gateway/Cargo.toml`
- Create: `services/gateway/src/main.rs`
- Create: `services/gateway/src/config.rs`
- Create: `services/gateway/src/auth/mod.rs`
- Create: `services/gateway/src/auth/middleware.rs`
- Create: `services/gateway/src/auth/handler.rs`
- Create: `services/gateway/src/model/mod.rs`
- Create: `services/gateway/src/model/user.rs`

- [ ] **Step 1: Write Cargo.toml**

```toml
[package]
name = "superops-gateway"
version = "0.1.0"
edition = "2021"

[dependencies]
ecat = "2.1"
ecat-data-sqlx = "2.1"
tonic = "0.12"
prost = "0.13"
axum = { version = "0.7", features = ["ws", "macros"] }
axum-extra = { version = "0.9", features = ["typed-header"] }
tokio = { version = "1", features = ["full"] }
tower = "0.4"
tower-http = { version = "0.5", features = ["cors", "trace"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yaml = "0.9"
jsonwebtoken = "9"
bcrypt = "0.15"
uuid = { version = "1", features = ["v4"] }
sqlx = { version = "0.8", features = ["runtime-tokio", "mysql", "chrono", "uuid"] }
chrono = { version = "0.4", features = ["serde"] }
anyhow = "1"
tracing = "0.1"
tracing-subscriber = "0.3"
```

- [ ] **Step 2: Write config.rs**

```rust
// services/gateway/src/config.rs
use anyhow::Result;
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub auth: AuthConfig,
    pub database: DatabaseConfig,
    pub redis: RedisConfig,
    pub services: ServicesConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig { pub http_port: u16, pub grpc_port: u16 }

#[derive(Debug, Deserialize, Clone)]
pub struct AuthConfig {
    pub jwt_secret: String,
    pub access_token_ttl: u64,
    pub refresh_token_ttl: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseConfig { pub url: String }

#[derive(Debug, Deserialize, Clone)]
pub struct RedisConfig { pub url: String }

#[derive(Debug, Deserialize, Clone)]
pub struct ServicesConfig { pub k8s: K8sServiceConfig }

#[derive(Debug, Deserialize, Clone)]
pub struct K8sServiceConfig { pub endpoint: String }

impl Config {
    pub fn load() -> Result<Self> {
        let path = std::env::var("GATEWAY_CONFIG")
            .unwrap_or_else(|_| "config/gateway.yaml".into());
        let content = std::fs::read_to_string(path)?;
        Ok(serde_yaml::from_str(&content)?)
    }
}
```

- [ ] **Step 3: Write auth middleware (JWT create + verify)**

```rust
// services/gateway/src/auth/mod.rs
pub mod handler;
pub mod middleware;
```

```rust
// services/gateway/src/auth/middleware.rs
use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use axum_extra::headers::{authorization::Bearer, Authorization};
use axum_extra::TypedHeader;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::config::AuthConfig;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,
    pub username: String,
    pub exp: usize,
    pub iat: usize,
}

pub struct AuthState {
    pub config: AuthConfig,
}

impl AuthState {
    pub fn new(config: AuthConfig) -> Self { Self { config } }

    pub fn create_token(&self, user_id: &str, username: &str, ttl: u64)
        -> Result<String, jsonwebtoken::errors::Error>
    {
        let now = chrono::Utc::now().timestamp() as usize;
        let claims = Claims {
            sub: user_id.to_string(),
            username: username.to_string(),
            exp: now + ttl as usize,
            iat: now,
        };
        encode(&Header::default(), &claims,
            &EncodingKey::from_secret(self.config.jwt_secret.as_bytes()))
    }

    pub fn verify_token(&self, token: &str)
        -> Result<Claims, jsonwebtoken::errors::Error>
    {
        decode::<Claims>(token,
            &DecodingKey::from_secret(self.config.jwt_secret.as_bytes()),
            &Validation::default())
        .map(|data| data.claims)
    }
}

pub struct AuthenticatedUser {
    pub user_id: String,
    pub username: String,
}

impl<S> FromRequestParts<S> for AuthenticatedUser
where S: Send + Sync
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let TypedHeader(Authorization(bearer)) =
            TypedHeader::<Authorization<Bearer>>::from_request_parts(parts, state)
                .await
                .map_err(|_| (StatusCode::UNAUTHORIZED,
                    Json(serde_json::json!({"error": "missing authorization header"}))
                ).into_response())?;

        let auth_state = parts.extensions.get::<Arc<AuthState>>().ok_or_else(|| {
            (StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "auth not configured"}))
            ).into_response()
        })?;

        let claims = auth_state.verify_token(bearer.token()).map_err(|_| {
            (StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "invalid or expired token"}))
            ).into_response()
        })?;

        Ok(AuthenticatedUser { user_id: claims.sub, username: claims.username })
    }
}
```

- [ ] **Step 4: Write auth handlers**

```rust
// services/gateway/src/auth/handler.rs
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use crate::auth::middleware::AuthState;
use crate::model::user::UserStore;

#[derive(Debug, Deserialize)]
pub struct LoginRequest { pub username: String, pub password: String }

#[derive(Debug, Serialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
}

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub email: String,
    pub password: String,
}

pub async fn login(
    State(auth): State<AuthState>,
    State(store): State<UserStore>,
    Json(req): Json<LoginRequest>,
) -> impl IntoResponse {
    let user = match store.find_by_username(&req.username).await {
        Ok(Some(u)) => u,
        _ => return (StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "invalid credentials"}))).into_response(),
    };

    if !bcrypt::verify(&req.password, &user.password_hash).unwrap_or(false) {
        return (StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "invalid credentials"}))).into_response();
    }

    let access_token = auth.create_token(&user.id, &user.username, auth.config.access_token_ttl).unwrap();
    let refresh_token = auth.create_token(&user.id, &user.username, auth.config.refresh_token_ttl).unwrap();

    (StatusCode::OK, Json(TokenResponse { access_token, refresh_token, expires_in: auth.config.access_token_ttl })).into_response()
}

pub async fn register(
    State(auth): State<AuthState>,
    State(store): State<UserStore>,
    Json(req): Json<RegisterRequest>,
) -> impl IntoResponse {
    let password_hash = bcrypt::hash(&req.password, bcrypt::DEFAULT_COST).unwrap();
    match store.create(&req.username, &req.email, &password_hash).await {
        Ok(user) => {
            let access_token = auth.create_token(&user.id, &user.username, auth.config.access_token_ttl).unwrap();
            (StatusCode::CREATED, Json(TokenResponse { access_token, refresh_token: String::new(), expires_in: auth.config.access_token_ttl })).into_response()
        }
        Err(_) => (StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "username or email already exists"}))).into_response(),
    }
}
```

- [ ] **Step 5: Write user model**

```rust
// services/gateway/src/model/mod.rs
pub mod user;
```

```rust
// services/gateway/src/model/user.rs
use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use sqlx::mysql::MySqlPool;

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct User {
    pub id: String,
    pub username: String,
    pub email: String,
    pub password_hash: String,
}

#[derive(Clone)]
pub struct UserStore {
    pool: MySqlPool,
}

impl UserStore {
    pub fn new(pool: MySqlPool) -> Self { Self { pool } }

    pub async fn find_by_username(&self, username: &str) -> Result<Option<User>> {
        Ok(sqlx::query_as::<_, User>(
            "SELECT id, username, email, password_hash FROM users WHERE username = ?")
            .bind(username)
            .fetch_optional(&self.pool)
            .await?)
    }

    pub async fn create(&self, username: &str, email: &str, password_hash: &str) -> Result<User> {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO users (id, username, email, password_hash) VALUES (?, ?, ?, ?)")
            .bind(&id).bind(username).bind(email).bind(password_hash)
            .execute(&self.pool).await?;
        Ok(User { id, username: username.to_string(), email: email.to_string(), password_hash: password_hash.to_string() })
    }
}
```

- [ ] **Step 6: Write main.rs**

```rust
// services/gateway/src/main.rs
mod auth;
mod config;
mod model;
mod proxy;

use std::sync::Arc;
use axum::{Router, routing::get};
use axum::routing::post;
use sqlx::mysql::MySqlPool;
use tower_http::cors::{CorsLayer, Any};
use crate::auth::handler::{login, register};
use crate::auth::middleware::AuthState;
use crate::config::Config;
use crate::model::user::UserStore;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let config = Config::load()?;
    let pool = MySqlPool::connect(&config.database.url).await?;
    let user_store = UserStore::new(pool);
    let auth_state = Arc::new(AuthState::new(config.auth.clone()));

    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/auth/login", post(login))
        .route("/api/auth/register", post(register))
        .layer(CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any))
        .with_state(auth_state)
        .with_state(user_store);

    let listener = tokio::net::TcpListener::bind(
        format!("0.0.0.0:{}", config.server.http_port)).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> &'static str { r#"{"status":"ok"}"# }
```

- [ ] **Step 7: Verify build**

```bash
cd services/gateway && cargo check
```

Expected: Compiles.

- [ ] **Step 8: Commit**

```bash
git add services/gateway/
git commit -m "feat(gateway): scaffold gateway with JWT auth and user management"
```

---

### Task 6: API Gateway — K8s proxy routes

**Files:**
- Create: `services/gateway/src/proxy/mod.rs`
- Create: `services/gateway/src/proxy/k8s_proxy.rs`
- Modify: `services/gateway/src/main.rs` (add proxy routes)

- [ ] **Step 1: Write K8s proxy with full HTTP routes**

```rust
// services/gateway/src/proxy/mod.rs
pub mod k8s_proxy;
```

```rust
// services/gateway/src/proxy/k8s_proxy.rs
use axum::{
    extract::{Path, Query, State, WebSocketUpgrade},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use crate::auth::middleware::AuthenticatedUser;

pub fn k8s_routes() -> axum::Router {
    axum::Router::new()
        .route("/api/k8s/clusters",
            axum::routing::get(list_clusters).post(add_cluster))
        .route("/api/k8s/clusters/{cluster_id}",
            axum::routing::get(get_cluster).delete(remove_cluster))
        .route("/api/k8s/clusters/{cluster_id}/pods", axum::routing::get(list_pods))
        .route("/api/k8s/clusters/{cluster_id}/pods/{namespace}/{pod}/logs",
            axum::routing::get(get_pod_logs))
        .route("/api/k8s/clusters/{cluster_id}/pods/{namespace}/{pod}/exec",
            axum::routing::get(exec_pod_ws))
        .route("/api/k8s/clusters/{cluster_id}/deployments",
            axum::routing::get(list_deployments))
        .route("/api/k8s/clusters/{cluster_id}/nodes",
            axum::routing::get(list_nodes))
        .route("/api/k8s/clusters/{cluster_id}/metrics",
            axum::routing::get(get_metrics))
}

#[derive(Debug, Deserialize)]
struct PodListQuery { namespace: Option<String>, page: Option<i32>, page_size: Option<i32> }

async fn list_clusters(_user: AuthenticatedUser) -> Json<serde_json::Value> {
    // For MVP, returns placeholder. Wired to gRPC client in implementation.
    Json(serde_json::json!({ "clusters": [] }))
}

async fn add_cluster(
    _user: AuthenticatedUser,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    (StatusCode::CREATED, Json(serde_json::json!({
        "id": "placeholder", "name": body.get("name"), "status": "connected"
    })))
}

async fn get_cluster(
    _user: AuthenticatedUser,
    Path(cluster_id): Path<String>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "cluster": { "id": cluster_id } }))
}

async fn remove_cluster(
    _user: AuthenticatedUser,
    Path(_cluster_id): Path<String>,
) -> StatusCode { StatusCode::NO_CONTENT }

async fn list_pods(
    _user: AuthenticatedUser,
    Path(cluster_id): Path<String>,
    Query(query): Query<PodListQuery>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "pods": [], "cluster_id": cluster_id,
        "namespace": query.namespace.unwrap_or_default()
    }))
}

async fn get_pod_logs(
    _user: AuthenticatedUser,
    Path((cluster_id, namespace, pod)): Path<(String, String, String)>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "logs": "", "cluster_id": cluster_id, "pod": pod, "namespace": namespace }))
}

async fn exec_pod_ws(
    _user: AuthenticatedUser,
    Path((cluster_id, namespace, pod)): Path<(String, String, String)>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |_socket| async move {
        tracing::info!("WebSocket terminal: {}/{} in {}", namespace, pod, cluster_id);
    })
}

async fn list_deployments(
    _user: AuthenticatedUser,
    Path(cluster_id): Path<String>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "deployments": [], "cluster_id": cluster_id }))
}

async fn list_nodes(
    _user: AuthenticatedUser,
    Path(cluster_id): Path<String>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "nodes": [], "cluster_id": cluster_id }))
}

async fn get_metrics(
    _user: AuthenticatedUser,
    Path(cluster_id): Path<String>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "metrics": [], "cluster_id": cluster_id }))
}
```

- [ ] **Step 2: Wire routes into main.rs**

Add to `services/gateway/src/main.rs`:

```rust
// Add use:
use crate::proxy::k8s_proxy::k8s_routes;

// Add to Router builder before .layer(...):
// .nest("/", k8s_routes())
```

- [ ] **Step 3: Verify build**

```bash
cd services/gateway && cargo check
```

Expected: Compiles.

- [ ] **Step 4: Commit**

```bash
git add services/gateway/src/
git commit -m "feat(gateway): add K8s proxy routes with auth guards"
```

---

### Task 7: Frontend — Vite + React + Ant Design scaffold

**Files:**
- Create: `frontend/package.json`
- Create: `frontend/vite.config.ts`
- Create: `frontend/tsconfig.json`
- Create: `frontend/tsconfig.node.json`
- Create: `frontend/index.html`
- Create: `frontend/src/main.tsx`
- Create: `frontend/src/App.tsx`

- [ ] **Step 1: Write package.json**

```json
{
  "name": "superops-frontend",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc && vite build",
    "preview": "vite preview",
    "tauri": "tauri"
  },
  "dependencies": {
    "react": "^18.3.1",
    "react-dom": "^18.3.1",
    "react-router-dom": "^6.26.0",
    "@ant-design/pro-components": "^2.7.0",
    "antd": "^5.20.0",
    "@ant-design/icons": "^5.4.0",
    "xterm": "^5.3.0",
    "xterm-addon-fit": "^0.8.0",
    "xterm-addon-web-links": "^0.9.0",
    "zustand": "^4.5.0",
    "@tanstack/react-query": "^5.51.0"
  },
  "devDependencies": {
    "typescript": "^5.5.0",
    "vite": "^5.4.0",
    "@vitejs/plugin-react": "^4.3.0",
    "@types/react": "^18.3.0",
    "@types/react-dom": "^18.3.0",
    "@tauri-apps/cli": "^2.0.0"
  }
}
```

- [ ] **Step 2: Write Vite + TS config**

```typescript
// frontend/vite.config.ts
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  server: {
    port: 3000,
    proxy: {
      '/api': 'http://localhost:8080',
      '/ws': { target: 'ws://localhost:8080', ws: true },
    },
  },
  clearScreen: false,
  envPrefix: ['VITE_', 'TAURI_'],
});
```

```json
// frontend/tsconfig.json
{
  "compilerOptions": {
    "target": "ES2020", "useDefineForClassFields": true,
    "lib": ["ES2020", "DOM", "DOM.Iterable"],
    "module": "ESNext", "skipLibCheck": true,
    "moduleResolution": "bundler", "allowImportingTsExtensions": true,
    "resolveJsonModule": true, "isolatedModules": true,
    "noEmit": true, "jsx": "react-jsx", "strict": true,
    "noUnusedLocals": false, "noUnusedParameters": false,
    "noFallthroughCasesInSwitch": true,
    "paths": { "@/*": ["./src/*"] }
  },
  "include": ["src"],
  "references": [{ "path": "./tsconfig.node.json" }]
}
```

- [ ] **Step 3: Write App.tsx with ProLayout shell**

```tsx
// frontend/src/App.tsx
import { ProLayout, PageContainer } from '@ant-design/pro-components';
import { Routes, Route, useNavigate, useLocation, Link } from 'react-router-dom';
import {
  DashboardOutlined, CloudServerOutlined, ContainerOutlined,
  GithubOutlined, RobotOutlined, BellOutlined, AuditOutlined, SettingOutlined,
} from '@ant-design/icons';
import { useAuthStore } from './stores/auth';
import LoginPage from './pages/login';
import Dashboard from './pages/dashboard';
import ClustersPage from './pages/k8s/clusters';
import ClusterDetail from './pages/k8s/cluster-detail';
import PodsPage from './pages/k8s/pods';
import DeploymentsPage from './pages/k8s/deployments';
import NodesPage from './pages/k8s/nodes';
import TerminalPage from './pages/k8s/terminal';

const menuData = [
  { path: '/dashboard', name: '总览', icon: <DashboardOutlined /> },
  {
    path: '/k8s', name: 'Kubernetes', icon: <CloudServerOutlined />,
    children: [
      { path: '/k8s/clusters', name: '集群管理' },
      { path: '/k8s/pods', name: 'Pods' },
      { path: '/k8s/deployments', name: 'Deployments' },
      { path: '/k8s/nodes', name: 'Nodes' },
      { path: '/k8s/terminal', name: 'Web Terminal' },
    ],
  },
];

export default function App() {
  const navigate = useNavigate();
  const location = useLocation();
  const { isAuthenticated } = useAuthStore();

  if (!isAuthenticated) return <LoginPage />;

  return (
    <ProLayout
      title="SuperOps" location={location}
      menuDataRender={() => menuData}
      onMenuHeaderClick={() => navigate('/dashboard')}
      menuItemRender={(item, dom) => <Link to={item.path || '/'}>{dom}</Link>}
    >
      <PageContainer>
        <Routes>
          <Route path="/dashboard" element={<Dashboard />} />
          <Route path="/k8s/clusters" element={<ClustersPage />} />
          <Route path="/k8s/clusters/:id" element={<ClusterDetail />} />
          <Route path="/k8s/pods" element={<PodsPage />} />
          <Route path="/k8s/deployments" element={<DeploymentsPage />} />
          <Route path="/k8s/nodes" element={<NodesPage />} />
          <Route path="/k8s/terminal" element={<TerminalPage />} />
        </Routes>
      </PageContainer>
    </ProLayout>
  );
}
```

```tsx
// frontend/src/main.tsx
import React from 'react';
import ReactDOM from 'react-dom/client';
import { BrowserRouter } from 'react-router-dom';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { ConfigProvider } from 'antd';
import zhCN from 'antd/locale/zh_CN';
import App from './App';

const queryClient = new QueryClient();

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <BrowserRouter>
      <QueryClientProvider client={queryClient}>
        <ConfigProvider locale={zhCN}><App /></ConfigProvider>
      </QueryClientProvider>
    </BrowserRouter>
  </React.StrictMode>,
);
```

```html
<!-- frontend/index.html -->
<!DOCTYPE html>
<html lang="zh-CN">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>SuperOps - 超级运维系统</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

- [ ] **Step 4: Install and verify dev server**

```bash
cd frontend && npm install && npm run dev
```

Expected: Vite dev server on port 3000, blank page with layout renders.

- [ ] **Step 5: Commit**

```bash
git add frontend/package.json frontend/vite.config.ts frontend/tsconfig*.json frontend/index.html frontend/src/main.tsx frontend/src/App.tsx frontend/package-lock.json
git commit -m "feat(frontend): scaffold React + Ant Design ProLayout shell"
```

---

### Task 8: Frontend — auth flow (login page + store + API client)

**Files:**
- Create: `frontend/src/stores/auth.ts`
- Create: `frontend/src/services/api.ts`
- Create: `frontend/src/pages/login.tsx`

- [ ] **Step 1: Write files**

```typescript
// frontend/src/services/api.ts
import { useAuthStore } from '../stores/auth';

const BASE_URL = '/api';

class ApiError extends Error {
  status: number;
  constructor(message: string, status: number) {
    super(message); this.status = status; this.name = 'ApiError';
  }
}

async function request<T>(path: string, options: RequestInit = {}): Promise<T> {
  const token = useAuthStore.getState().token;
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    ...(options.headers as Record<string, string> || {}),
  };
  if (token) headers['Authorization'] = `Bearer ${token}`;

  const response = await fetch(`${BASE_URL}${path}`, { ...options, headers });

  if (response.status === 401) {
    useAuthStore.getState().logout();
    throw new ApiError('未授权，请重新登录', 401);
  }
  if (!response.ok) {
    const body = await response.json().catch(() => ({}));
    throw new ApiError(body.error || '请求失败', response.status);
  }
  return response.json();
}

export const api = {
  get: <T>(path: string) => request<T>(path),
  post: <T>(path: string, body: unknown) =>
    request<T>(path, { method: 'POST', body: JSON.stringify(body) }),
  delete: <T>(path: string) => request<T>(path, { method: 'DELETE' }),
};
export { ApiError };
```

```typescript
// frontend/src/stores/auth.ts
import { create } from 'zustand';
import { persist } from 'zustand/middleware';

interface AuthState {
  token: string | null;
  username: string | null;
  isAuthenticated: boolean;
  login: (token: string, username: string) => void;
  logout: () => void;
}

export const useAuthStore = create<AuthState>()(
  persist(
    (set) => ({
      token: null, username: null, isAuthenticated: false,
      login: (token, username) => set({ token, username, isAuthenticated: true }),
      logout: () => set({ token: null, username: null, isAuthenticated: false }),
    }),
    { name: 'superops-auth' },
  ),
);
```

```tsx
// frontend/src/pages/login.tsx
import { useState } from 'react';
import { LoginFormPage, ProFormText } from '@ant-design/pro-components';
import { message } from 'antd';
import { UserOutlined, LockOutlined } from '@ant-design/icons';
import { api } from '../services/api';
import { useAuthStore } from '../stores/auth';

interface LoginResponse {
  access_token: string;
  refresh_token: string;
  expires_in: number;
}

export default function LoginPage() {
  const [loading, setLoading] = useState(false);
  const login = useAuthStore((s) => s.login);

  const handleSubmit = async (values: { username: string; password: string }) => {
    setLoading(true);
    try {
      const res = await api.post<LoginResponse>('/auth/login', values);
      login(res.access_token, values.username);
      message.success('登录成功');
    } catch (e: any) {
      message.error(e.message || '登录失败');
    } finally { setLoading(false); }
  };

  return (
    <LoginFormPage
      title="SuperOps" subTitle="超级运维系统"
      onFinish={handleSubmit}
      submitter={{ searchConfig: { submitText: '登录' } }}
      loading={loading}
    >
      <ProFormText name="username"
        fieldProps={{ size: 'large', prefix: <UserOutlined /> }}
        placeholder="用户名"
        rules={[{ required: true, message: '请输入用户名' }]} />
      <ProFormText.Password name="password"
        fieldProps={{ size: 'large', prefix: <LockOutlined /> }}
        placeholder="密码"
        rules={[{ required: true, message: '请输入密码' }]} />
    </LoginFormPage>
  );
}
```

- [ ] **Step 2: Verify login page renders**

```bash
cd frontend && npm run dev
```

Expected: Login page at http://localhost:3000.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/stores/ frontend/src/services/ frontend/src/pages/login.tsx
git commit -m "feat(frontend): add login page with auth store and API client"
```

---

### Task 9: Frontend — Dashboard + K8s pages

**Files:**
- Create: `frontend/src/pages/dashboard.tsx`
- Create: `frontend/src/services/k8s.ts`
- Create: `frontend/src/pages/k8s/clusters.tsx`
- Create: `frontend/src/pages/k8s/cluster-detail.tsx`
- Create: `frontend/src/pages/k8s/pods.tsx`
- Create: `frontend/src/pages/k8s/deployments.tsx`
- Create: `frontend/src/pages/k8s/nodes.tsx`
- Create: `frontend/src/pages/k8s/terminal.tsx`
- Create: `frontend/src/components/pod-logs.tsx`

- [ ] **Step 1: Write dashboard**

```tsx
// frontend/src/pages/dashboard.tsx
import { StatisticCard } from '@ant-design/pro-components';
import { Row, Col, Card, List, Tag } from 'antd';
import { CloudServerOutlined, ContainerOutlined, GithubOutlined, WarningOutlined } from '@ant-design/icons';

export default function Dashboard() {
  return (
    <div>
      <Row gutter={[16, 16]}>
        <Col span={6}><StatisticCard statistic={{ title: 'K8s 集群', value: 0, icon: <CloudServerOutlined /> }} /></Col>
        <Col span={6}><StatisticCard statistic={{ title: 'Docker 主机', value: 0, icon: <ContainerOutlined /> }} /></Col>
        <Col span={6}><StatisticCard statistic={{ title: 'Pipeline', value: 0, icon: <GithubOutlined /> }} /></Col>
        <Col span={6}><StatisticCard statistic={{ title: '活跃告警', value: 0, icon: <WarningOutlined /> }} /></Col>
      </Row>
      <Row gutter={[16, 16]} style={{ marginTop: 16 }}>
        <Col span={12}>
          <Card title="集群健康状态">
            <List dataSource={[]} locale={{ emptyText: '暂无集群接入' }}
              renderItem={(item: any) => (
                <List.Item><Tag color={item.status === 'healthy' ? 'green' : 'red'}>{item.name}</Tag></List.Item>
              )} />
          </Card>
        </Col>
        <Col span={12}>
          <Card title="最近告警">
            <List dataSource={[]} locale={{ emptyText: '暂无告警' }}
              renderItem={(item: any) => (
                <List.Item><List.Item.Meta title={item.message} description={item.time} /></List.Item>
              )} />
          </Card>
        </Col>
      </Row>
    </div>
  );
}
```

- [ ] **Step 2: Write K8s API service and pages**

```typescript
// frontend/src/services/k8s.ts
import { api } from './api';

export interface Cluster { id: string; name: string; version: string; node_count: number; pod_count: number; status: string; }
export interface Pod { name: string; namespace: string; status: string; node: string; restarts: number; age: string; }
export interface Deployment { name: string; namespace: string; replicas: number; ready_replicas: number; age: string; }
export interface NodeInfo { name: string; status: string; role: string; version: string; cpu: string; memory: string; }

export const k8sApi = {
  listClusters: () => api.get<{ clusters: Cluster[] }>('/k8s/clusters'),
  addCluster: (name: string, kubeconfig: string) => api.post<{ cluster: Cluster }>('/k8s/clusters', { name, kubeconfig }),
  removeCluster: (id: string) => api.delete(`/k8s/clusters/${id}`),
  listPods: (clusterId: string, namespace?: string) => api.get<{ pods: Pod[] }>(`/k8s/clusters/${clusterId}/pods?namespace=${namespace || ''}`),
  listDeployments: (clusterId: string) => api.get<{ deployments: Deployment[] }>(`/k8s/clusters/${clusterId}/deployments`),
  listNodes: (clusterId: string) => api.get<{ nodes: NodeInfo[] }>(`/k8s/clusters/${clusterId}/nodes`),
};
```

```tsx
// frontend/src/pages/k8s/clusters.tsx
import { ProList } from '@ant-design/pro-components';
import { Button, Tag, Modal, Form, Input, message } from 'antd';
import { PlusOutlined } from '@ant-design/icons';
import { useNavigate } from 'react-router-dom';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { k8sApi, Cluster } from '../../services/k8s';
import { useState } from 'react';

export default function ClustersPage() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [modalOpen, setModalOpen] = useState(false);
  const [form] = Form.useForm();

  const { data, isLoading } = useQuery({ queryKey: ['clusters'], queryFn: () => k8sApi.listClusters() });

  const addMutation = useMutation({
    mutationFn: (values: { name: string; kubeconfig: string }) => k8sApi.addCluster(values.name, values.kubeconfig),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['clusters'] });
      setModalOpen(false); form.resetFields(); message.success('集群添加成功');
    },
    onError: (e: Error) => message.error(e.message),
  });

  return (
    <>
      <ProList<Cluster> rowKey="id" loading={isLoading} dataSource={data?.clusters || []}
        metas={{
          title: { dataIndex: 'name' },
          description: { dataIndex: 'version' },
          content: { render: (_, r) => (
            <div style={{ display: 'flex', gap: 16 }}>
              <Tag color={r.status === 'healthy' ? 'green' : 'orange'}>{r.status}</Tag>
              <span>节点: {r.node_count}</span><span>Pods: {r.pod_count}</span>
            </div>
          )},
        }}
        headerTitle="集群列表"
        toolBarRender={() => [<Button key="add" type="primary" icon={<PlusOutlined />} onClick={() => setModalOpen(true)}>添加集群</Button>]}
        onRow={(record) => ({ onClick: () => navigate(`/k8s/clusters/${record.id}`), style: { cursor: 'pointer' } })}
        locale={{ emptyText: '暂无集群，点击右上角添加' }}
      />
      <Modal title="添加 Kubernetes 集群" open={modalOpen} onCancel={() => setModalOpen(false)} onOk={() => form.submit()} confirmLoading={addMutation.isPending}>
        <Form form={form} onFinish={(values) => addMutation.mutate(values)} layout="vertical">
          <Form.Item name="name" label="集群名称" rules={[{ required: true }]}><Input placeholder="如: prod-cluster-1" /></Form.Item>
          <Form.Item name="kubeconfig" label="Kubeconfig" rules={[{ required: true }]}><Input.TextArea rows={8} placeholder="粘贴 kubeconfig 内容" /></Form.Item>
        </Form>
      </Modal>
    </>
  );
}
```

```tsx
// frontend/src/pages/k8s/cluster-detail.tsx
import { useParams } from 'react-router-dom';
import { Card, Tabs } from 'antd';
import PodsPage from './pods';
import DeploymentsPage from './deployments';
import NodesPage from './nodes';

export default function ClusterDetail() {
  const { id } = useParams<{ id: string }>();
  return (
    <Card title={`集群详情: ${id}`}>
      <Tabs items={[
        { key: 'pods', label: 'Pods', children: <PodsPage /> },
        { key: 'deployments', label: 'Deployments', children: <DeploymentsPage /> },
        { key: 'nodes', label: 'Nodes', children: <NodesPage /> },
      ]} />
    </Card>
  );
}
```

```tsx
// frontend/src/pages/k8s/pods.tsx
import { ProTable } from '@ant-design/pro-components';
import { useQuery } from '@tanstack/react-query';
import { k8sApi, Pod } from '../../services/k8s';

export default function PodsPage() {
  const { data, isLoading } = useQuery({ queryKey: ['pods'], queryFn: () => k8sApi.listPods('default') });
  return (
    <ProTable<Pod>
      columns={[
        { title: '名称', dataIndex: 'name' }, { title: '命名空间', dataIndex: 'namespace' },
        { title: '状态', dataIndex: 'status' }, { title: '节点', dataIndex: 'node' },
        { title: '重启', dataIndex: 'restarts' }, { title: '运行时间', dataIndex: 'age' },
      ]}
      dataSource={data?.pods || []} loading={isLoading} rowKey="name" search={false} headerTitle="Pod 列表"
    />
  );
}
```

```tsx
// frontend/src/pages/k8s/deployments.tsx
import { ProTable } from '@ant-design/pro-components';
import { useQuery } from '@tanstack/react-query';
import { k8sApi, Deployment } from '../../services/k8s';

export default function DeploymentsPage() {
  const { data, isLoading } = useQuery({ queryKey: ['deployments'], queryFn: () => k8sApi.listDeployments('default') });
  return (
    <ProTable<Deployment>
      columns={[
        { title: '名称', dataIndex: 'name' }, { title: '命名空间', dataIndex: 'namespace' },
        { title: '副本', dataIndex: 'replicas' }, { title: '就绪', dataIndex: 'ready_replicas' },
        { title: '运行时间', dataIndex: 'age' },
      ]}
      dataSource={data?.deployments || []} loading={isLoading} rowKey="name" search={false} headerTitle="Deployment 列表"
    />
  );
}
```

```tsx
// frontend/src/pages/k8s/nodes.tsx
import { ProTable } from '@ant-design/pro-components';
import { useQuery } from '@tanstack/react-query';
import { k8sApi, NodeInfo } from '../../services/k8s';

export default function NodesPage() {
  const { data, isLoading } = useQuery({ queryKey: ['nodes'], queryFn: () => k8sApi.listNodes('default') });
  return (
    <ProTable<NodeInfo>
      columns={[
        { title: '名称', dataIndex: 'name' }, { title: '状态', dataIndex: 'status' },
        { title: '角色', dataIndex: 'role' }, { title: '版本', dataIndex: 'version' },
        { title: 'CPU', dataIndex: 'cpu' }, { title: '内存', dataIndex: 'memory' },
      ]}
      dataSource={data?.nodes || []} loading={isLoading} rowKey="name" search={false} headerTitle="Node 列表"
    />
  );
}
```

```tsx
// frontend/src/components/pod-logs.tsx
import { useEffect, useRef } from 'react';
import { Terminal } from 'xterm';
import { FitAddon } from 'xterm-addon-fit';
import 'xterm/css/xterm.css';

interface PodLogsProps { clusterId: string; namespace: string; podName: string; container?: string; }

export default function PodLogs({ clusterId, namespace, podName, container }: PodLogsProps) {
  const terminalRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const term = new Terminal({ fontSize: 13, fontFamily: 'Menlo, Monaco, monospace', theme: { background: '#1a1a2e' } });
    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    if (terminalRef.current) { term.open(terminalRef.current); fitAddon.fit(); }
    term.writeln(`Connecting to logs for ${namespace}/${podName}...`);
    return () => { term.dispose(); };
  }, [clusterId, namespace, podName, container]);

  return <div ref={terminalRef} style={{ height: '400px', width: '100%' }} />;
}
```

```tsx
// frontend/src/pages/k8s/terminal.tsx
import { useEffect, useRef, useState } from 'react';
import { Card, Form, Input, Button, Space } from 'antd';
import { Terminal } from 'xterm';
import { FitAddon } from 'xterm-addon-fit';
import { WebLinksAddon } from 'xterm-addon-web-links';
import 'xterm/css/xterm.css';

export default function TerminalPage() {
  const terminalRef = useRef<HTMLDivElement>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const [connected, setConnected] = useState(false);

  const connect = (values: { cluster: string; namespace: string; pod: string }) => {
    const term = new Terminal({ fontSize: 14, fontFamily: 'Menlo, Monaco, monospace', cursorBlink: true, theme: { background: '#1a1a2e', foreground: '#e0e0e0' } });
    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.loadAddon(new WebLinksAddon());
    if (terminalRef.current) { term.open(terminalRef.current); fitAddon.fit(); }

    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const ws = new WebSocket(`${protocol}//${window.location.host}/api/k8s/clusters/${values.cluster}/pods/${values.namespace}/${values.pod}/exec`);

    ws.onopen = () => setConnected(true);
    ws.onmessage = (e) => term.write(e.data);
    ws.onclose = () => { setConnected(false); term.dispose(); };
    ws.onerror = () => { setConnected(false); term.dispose(); };
    term.onData((data) => { if (ws.readyState === WebSocket.OPEN) ws.send(data); });

    wsRef.current = ws;

    const handleResize = () => fitAddon.fit();
    window.addEventListener('resize', handleResize);
    return () => { ws.close(); term.dispose(); window.removeEventListener('resize', handleResize); };
  };

  return (
    <Card title="Web Terminal">
      <Space style={{ marginBottom: 16 }}>
        <Form layout="inline" onFinish={connect}>
          <Form.Item name="cluster" rules={[{ required: true }]}><Input placeholder="集群 ID" style={{ width: 200 }} /></Form.Item>
          <Form.Item name="namespace" rules={[{ required: true }]}><Input placeholder="命名空间" style={{ width: 160 }} /></Form.Item>
          <Form.Item name="pod" rules={[{ required: true }]}><Input placeholder="Pod 名称" style={{ width: 200 }} /></Form.Item>
          <Form.Item><Button type="primary" htmlType="submit" disabled={connected}>连接</Button></Form.Item>
        </Form>
        {connected && <Button danger onClick={() => wsRef.current?.close()}>断开</Button>}
      </Space>
      <div ref={terminalRef} style={{ height: '500px', width: '100%' }} />
    </Card>
  );
}
```

- [ ] **Step 3: Verify frontend builds**

```bash
cd frontend && npm run build
```

Expected: Successful production build.

- [ ] **Step 4: Commit**

```bash
git add frontend/src/
git commit -m "feat(frontend): add dashboard, K8s cluster/pod/deploy/node pages, terminal and log viewer"
```

---

### Task 10: Tauri desktop integration

**Files:**
- Create: `frontend/src-tauri/Cargo.toml`
- Create: `frontend/src-tauri/tauri.conf.json`
- Create: `frontend/src-tauri/build.rs`
- Create: `frontend/src-tauri/src/main.rs`
- Create: `frontend/src-tauri/src/lib.rs`

- [ ] **Step 1: Initialize Tauri project**

```bash
cd frontend && npx tauri init
```

Use: app-name "superops", window-title "SuperOps", dev-url "http://localhost:3000"

- [ ] **Step 2: Configure tauri.conf.json**

```json
{
  "productName": "SuperOps",
  "version": "0.1.0",
  "identifier": "com.superops.app",
  "build": {
    "frontendDist": "../dist",
    "devUrl": "http://localhost:3000",
    "beforeDevCommand": "npm run dev",
    "beforeBuildCommand": "npm run build"
  },
  "app": {
    "title": "SuperOps",
    "windows": [
      { "title": "SuperOps", "width": 1280, "height": 800, "resizable": true, "fullscreen": false }
    ],
    "security": { "csp": null }
  },
  "bundle": { "active": true, "targets": "all", "icon": ["icons/32x32.png", "icons/128x128.png", "icons/128x128@2x.png", "icons/icon.icns", "icons/icon.ico"] }
}
```

- [ ] **Step 3: Write Tauri Rust backend**

```rust
// frontend/src-tauri/src/lib.rs
use tauri::{CustomMenuItem, Manager, SystemTray, SystemTrayEvent, SystemTrayMenu};

#[tauri::command]
fn get_system_info() -> String {
    serde_json::json!({ "platform": std::env::consts::OS, "arch": std::env::consts::ARCH }).to_string()
}

pub fn run() {
    let tray_menu = SystemTrayMenu::new()
        .add_item(CustomMenuItem::new("show", "显示面板"))
        .add_item(CustomMenuItem::new("quit", "退出"));

    tauri::Builder::default()
        .system_tray(SystemTray::new().with_menu(tray_menu))
        .on_system_tray_event(|app, event| match event {
            SystemTrayEvent::MenuItemClick { id, .. } => match id.as_str() {
                "show" => { if let Some(w) = app.get_window("main") { w.show().ok(); w.set_focus().ok(); } }
                "quit" => std::process::exit(0),
                _ => {}
            },
            SystemTrayEvent::LeftClick { .. } => {
                if let Some(w) = app.get_window("main") { w.show().ok(); w.set_focus().ok(); }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![get_system_info])
        .on_window_event(|event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event.event() {
                event.window().hide().ok();
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("error running tauri");
}
```

```rust
// frontend/src-tauri/src/main.rs
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() { superops_tauri::run(); }
```

- [ ] **Step 4: Verify Tauri dev mode**

```bash
cd frontend && npm run tauri dev
```

Expected: Desktop window opens with the SuperOps app.

- [ ] **Step 5: Commit**

```bash
git add frontend/src-tauri/
git commit -m "feat(tauri): add desktop integration with system tray"
```

---

### Task 11: Docker Compose dev environment

**Files:**
- Create: `deploy/docker-compose.yml`
- Create: `deploy/init.sql`

- [ ] **Step 1: Write docker-compose.yml**

```yaml
version: '3.8'
services:
  mysql:
    image: mysql:8.0
    environment:
      MYSQL_ROOT_PASSWORD: root123
      MYSQL_DATABASE: superops
      MYSQL_USER: superops
      MYSQL_PASSWORD: superops
    ports: ["3306:3306"]
    volumes:
      - mysql_data:/var/lib/mysql
      - ./init.sql:/docker-entrypoint-initdb.d/init.sql
    healthcheck:
      test: ["CMD", "mysqladmin", "ping", "-h", "localhost"]
      interval: 5s
      retries: 5

  redis:
    image: redis:7-alpine
    ports: ["6379:6379"]
    healthcheck:
      test: ["CMD", "redis-cli", "ping"]
      interval: 5s
      retries: 5

  clickhouse:
    image: clickhouse/clickhouse-server:24-alpine
    ports: ["8123:8123", "9000:9000"]
    environment:
      CLICKHOUSE_DB: superops
      CLICKHOUSE_USER: superops
      CLICKHOUSE_PASSWORD: superops
    volumes: [clickhouse_data:/var/lib/clickhouse]

volumes:
  mysql_data:
  clickhouse_data:
```

- [ ] **Step 2: Write init.sql**

```sql
CREATE TABLE IF NOT EXISTS users (
    id VARCHAR(36) PRIMARY KEY,
    username VARCHAR(64) NOT NULL UNIQUE,
    email VARCHAR(128) NOT NULL UNIQUE,
    password_hash VARCHAR(255) NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP
);
CREATE INDEX idx_users_username ON users(username);
CREATE INDEX idx_users_email ON users(email);
```

- [ ] **Step 3: Start environment**

```bash
cd deploy && docker compose up -d && docker compose ps
```

Expected: mysql, redis, clickhouse all healthy.

- [ ] **Step 4: Commit**

```bash
git add deploy/
git commit -m "chore: add Docker Compose dev environment"
```

---

## MVP Completion Checklist

- [ ] `make dev-all` 启动完整开发环境
- [ ] 用户注册/登录 API 可用 (`POST /api/auth/register`, `POST /api/auth/login`)
- [ ] 添加 K8s 集群 API 可用 (`POST /api/k8s/clusters`)
- [ ] 集群/Pod/Deployment/Node 列表 API 可用
- [ ] Pod 日志 WebSocket 流可用
- [ ] Web Terminal WebSocket 可用
- [ ] 前端 Dashboard 展示集群状态
- [ ] 前端 K8s 面板可浏览资源列表
- [ ] 前端 Web Terminal 可连接 Pod
- [ ] Tauri `npm run tauri build` 产出桌面端二进制
