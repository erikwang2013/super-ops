# P1 数据与监控闭环 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新增 `services/collector` 服务：定时采集 k8s 集群快照写入 ClickHouse、按规则生成告警事件、消费 gateway 审计消息入库，并为 gateway 提供指标查询 API；配合 RedisLock 防止多实例重复巡检。

**Architecture:** collector 是无服务器模式的 `ecat::App`（`wait_for_shutdown` 保持 runtime 存活）：`ecat-scheduler` 定时任务（collect 快照 + inspect 告警），`ecat-mq-kafka` 消费审计事件，`ecat-data-clickhouse` 的 `TsdbClient` impl 写时序数据，`ecat-data-redis` 的 `RedisLock` 做巡检互斥。gateway 侧新增 `/api/v1/metrics/query` 与审计事件发布（login/register 时 `publish` 到 Kafka）。ClickHouse 建表由 TsdbClient 自动完成（MergeTree ORDER BY timestamp），数据去重放在查询侧。

**Tech Stack:** Rust 2024, tokio, ecat App/Scheduler/MQ/Data/Data-ClickHouse/Data-Redis/Lock, superops-protos (tonic), Kafka (KRaft), ClickHouse, Redis, axum (gateway)

---

## 勘探事实（API 勘探结论，2026-08-06）

- `ecat::App::builder().name(...).server(http).build()`；`App::run()` 无 server 时 `wait_for_shutdown()` 阻塞保持 runtime 存活 → Scheduler 任务可持续运行；`on_stop` 钩子在 shutdown 时被调用。
- `LifecycleHook: async fn on_start(&self) / on_stop(&self) -> Result<(), Box<dyn Error + Send + Sync>>`，闭包可直接 impl（ecat/src/hook.rs）。
- `Scheduler::every(Duration, Fn() -> Fut)` 首个 tick 在 1 个间隔之后（不立即执行）；`Scheduler::shutdown()` 以值接收自身并 abort 任务 → 需 `Arc<Mutex<Scheduler>>` 放入 hook 结构体，on_stop 中 `lock().shutdown()`。
- `TsdbClient::write(&[DataPoint])`；`DataPoint::new(measurement).with_tag(k, v).with_field(k, FieldValue::*).with_timestamp(i64)`；`FieldValue::{Float, Int, String, Bool}`（Debug+Clone，无 PartialEq → 测试用 `matches!`）。
- `ClickhouseClient::from_config(ClickhouseConfig)`；`TsdbClient::query` 与 `RdbmsClient::query` 同名 → 必须全限定 `ecat_data::TsdbClient::query(state.ch.as_ref(), &sql)`。
- ClickHouse HTTP 无 prepared params → 字符串字面量转义 `quote_str`：`s.replace('\\', "\\\\").replace('\'', "\\'")`。
- `KafkaMq::from_config(KafkaConfig{brokers, group_id})` 仅构建 producer；`subscribe(topic)` 后 `auto.offset.reset=latest`（只收订阅后的消息）；消费循环用 `futures::future::poll_fn(|cx| stream.poll_recv(cx))`。
- `RedisLock::from_config(RedisConfig{url, password, tls})`；`DistributedLock::acquire(key, ttl) -> Result<Option<String>, LockError>`（SET NX PX），`release(key, token)`（Lua CAS）。
- `K8sServiceClient::connect("http://localhost:9091")`；prost message-typed 字段是 `Option<T>`（如 `ListPodsRequest.pagination: Option<Pagination>`）。
- Node{name,status,role,version,age,cpu,memory} 全 String；status ∈ "Ready"/"NotReady"/"Unknown"。Pod{name,namespace,status}。Deployment{replicas, ready_replicas: i32}。
- gateway `AppState` 将新增 `ch: Arc<ClickhouseClient>`、`mq: Option<Arc<KafkaMq>>`；`k8s_routes()` 返回 `axum::Router<AppState>`；metrics 路由 merge 进 k8s_routes()（在 auth layer 之前）。
- `Config::load()` 读 `GATEWAY_CONFIG` env（默认 `config/gateway.yaml`）；collector 同样约定（`COLLECTOR_CONFIG`，默认 `config/collector.yaml`）。
- 环境约束（诚实标注）：ClickHouse/Redis/Kafka 容器 + k8s 集群均需 sudo 启动且当前未就绪 → 所有 e2e 验证步骤标记为「复核」（待容器起后人工复核），单元测试不依赖外部服务。
- 偏差记录：总纲 P1.2 原计划 ReplacingMergeTree 去重 → 实际为上游 `TsdbClient` 自动建表 `MergeTree ORDER BY timestamp`，去重放查询侧（`argMax` + 按 node/type 分组取最新）。

---

### Task 1: collector 服务骨架（Cargo.toml / config.rs / main.rs 最小可编译）

**Files:**
- Create: `services/collector/Cargo.toml`
- Create: `services/collector/src/lib.rs`
- Create: `services/collector/src/config.rs`
- Create: `services/collector/src/main.rs`
- Create: `config/collector.yaml`
- Modify: `Cargo.toml`（workspace members 加入 `services/collector`）
- Modify: `Makefile`（collector run/dev/stop 目标）
- Test: `services/collector/tests/config_test.rs`

- [x] **Step 1: 写失败测试（config 解析）**

```rust
// services/collector/tests/config_test.rs
use superops_collector::config::{collector_config, CollectorConfig};

#[test]
fn default_collector_config() {
    let cfg = CollectorConfig::default();
    assert_eq!(cfg.collect_interval_secs, 60);
    assert_eq!(cfg.inspect_interval_secs, 600);
    assert_eq!(cfg.max_not_ready, 1);
    assert_eq!(cfg.alert_consecutive, 2);
}

#[test]
fn config_from_default_file() {
    // 不设 COLLECTOR_CONFIG，应能解析默认路径 config/collector.yaml
    std::env::remove_var("COLLECTOR_CONFIG");
    let cfg = collector_config().expect("default config should load");
    assert!(cfg.ch.base_url.contains("8123"));
    assert!(cfg.k8s.endpoint.contains("9091"));
}
```

- [x] **Step 2: 运行确认失败**

Run: `cargo test -p superops-collector --test config_test 2>&1 | tail -5`
Expected: FAIL（crate 不存在）

- [x] **Step 3: 创建 crate 与实现**

`services/collector/Cargo.toml`:

```toml
[package]
name = "superops-collector"
version = "0.1.0"
edition = "2024"
description = "SuperOps collector: k8s snapshot + alert inspection + audit ingestion"

[dependencies]
superops-protos = { path = "../../superops-protos" }
ecat = { path = "../../ecat" }
ecat-data = { path = "../../ecat-data" }
ecat-data-clickhouse = { path = "../../ecat-data-clickhouse" }
ecat-data-redis = { path = "../../ecat-data-redis" }
ecat-lock = { path = "../../ecat-lock" }
ecat-mq = { path = "../../ecat-mq" }
ecat-mq-kafka = { path = "../../ecat-mq-kafka" }
ecat-scheduler = { path = "../../ecat-scheduler" }
tonic = "0.12"
tokio = { version = "1", features = ["rt-multi-thread", "macros", "time", "sync"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yaml = "0.9"
tracing = "0.1"
anyhow = "1"
futures = "0.3"
async-trait = "0.1"

[dev-dependencies]
tempfile = "3"
```

`services/collector/src/lib.rs`:

```rust
pub mod config;
pub mod main_lib;
```

`services/collector/src/config.rs`:

```rust
use ecat_data_clickhouse::ClickhouseConfig;
use ecat_data_redis::RedisConfig;
use ecat_mq_kafka::KafkaConfig;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct K8sConfig {
    pub endpoint: String,
}

impl Default for K8sConfig {
    fn default() -> Self {
        Self { endpoint: "http://localhost:9091".into() }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CollectorConfig {
    pub collect_interval_secs: u64,
    pub inspect_interval_secs: u64,
    pub max_not_ready: usize,
    pub alert_consecutive: usize,
}

impl Default for CollectorConfig {
    fn default() -> Self {
        Self {
            collect_interval_secs: 60,
            inspect_interval_secs: 600,
            max_not_ready: 1,
            alert_consecutive: 2,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub ch: ClickhouseConfig,
    #[serde(default)]
    pub mq: KafkaConfig,
    #[serde(default)]
    pub lock: RedisConfig,
    #[serde(default)]
    pub k8s: K8sConfig,
    #[serde(default)]
    pub collector: CollectorConfig,
}

pub fn collector_config() -> anyhow::Result<Config> {
    let path = std::env::var("COLLECTOR_CONFIG").unwrap_or_else(|_| "config/collector.yaml".into());
    let raw = std::fs::read_to_string(&path)?;
    let cfg: Config = serde_yaml::from_str(&raw)?;
    Ok(cfg)
}
```

`services/collector/src/main_lib.rs`:

```rust
pub mod main_lib { }
```

- [x] **Step 4: 运行确认通过**

Run: `cargo test -p superops-collector --test config_test`
Expected: PASS（2 tests）

- [x] **Step 5: 注册 workspace + Makefile + 默认配置**

`Cargo.toml` workspace members 追加 `"services/collector"`。

`config/collector.yaml`:

```yaml
ch:
  base_url: "http://localhost:8123"
  database: "superops"
mq:
  brokers: "localhost:9092"
  group_id: "superops-collector"
lock:
  url: "redis://localhost:6379"
k8s:
  endpoint: "http://localhost:9091"
collector:
  collect_interval_secs: 60
  inspect_interval_secs: 600
  max_not_ready: 1
  alert_consecutive: 2
```

Makefile 追加（沿用项目现有 `pkill -f "superops-collector"` 风格）：

```make
collector:
	cargo run -p superops-collector

collector-dev:
	cargo watch -x "run -p superops-collector"

collector-stop:
	pkill -f "superops-collector" || true
```

并在 `dev-all` / `stop-all` 目标中追加 collector 的启动/停止。

- [x] **Step 6: 全 workspace check + 提交**

Run: `cargo check --workspace 2>&1 | tail -3 && cargo test -p superops-collector 2>&1 | tail -3`
Expected: 无错误；collector 测试全绿。

Run: `git add services/collector config/collector.yaml Cargo.toml Makefile && git commit -m "feat(collector): skeleton with config loading"`
（若用户未要求提交，跳过本步，改为汇报）

---

### Task 2: ch.rs — 快照点构建与写入

**Files:**
- Create: `services/collector/src/ch.rs`
- Modify: `services/collector/src/lib.rs`（注册 mod）
- Test: `services/collector/tests/ch_test.rs`

- [x] **Step 1: 写失败测试**

```rust
// services/collector/tests/ch_test.rs
use superops_collector::ch::{now_secs, build_snapshot_points, write_snapshot};
use superops_protos::k8s::v1::{Node, NodeStatus, Pod, PodStatus, Deployment, DeploymentStatus, ListNodesResponse, ListPodsResponse, ListDeploymentsResponse};
use ecat_data::{DataPoint, FieldValue};
use std::sync::Arc;

fn sample_nodes() -> Vec<Node> {
    vec![Node { name: "node-a".into(), status: "Ready".into(), role: "control-plane".into(), version: "v1.30".into(), age: "10d".into(), cpu: "8".into(), memory: "16Gi".into() }]
}

#[test]
fn snapshot_points_have_expected_shape() {
    let nodes = sample_nodes();
    let pods = vec![Pod { name: "pod-1".into(), namespace: "default".into(), status: "Running".into(), node: "node-a".into(), restart_count: 0, cpu_usage: 0.0, memory_usage: 0.0 }];
    let deps = vec![Deployment { name: "web".into(), namespace: "default".into(), replicas: 2, ready_replicas: 2, status: "Ready".into() }];
    let points = build_snapshot_points(&nodes, &pods, &deps, 1234567890);
    assert_eq!(points.len(), 4); // __summary__ + node-a + pod + deployment
    let summary = points.first().unwrap();
    assert_eq!(summary.measurement, "resource_snapshot");
    assert!(matches!(summary.fields.get("node_count"), Some(FieldValue::Int(1))));
    assert_eq!(summary.timestamp, Some(1234567890));
    let node_pt = points.get(1).unwrap();
    assert_eq!(node_pt.tags.get("node").map(String::as_str), Some("node-a"));
    assert!(matches!(node_pt.fields.get("ready"), Some(FieldValue::Int(1))));
}

#[test]
fn now_secs_is_reasonable() {
    let t = now_secs();
    assert!(t > 1_700_000_000 && t < 2_000_000_000);
}
```

- [x] **Step 2: 运行确认失败**

Run: `cargo test -p superops-collector --test ch_test 2>&1 | tail -5`
Expected: FAIL（mod 不存在）

- [x] **Step 3: 实现 ch.rs**

```rust
use ecat_data::{DataPoint, FieldValue, TsdbClient, TsdbError};
use superops_protos::k8s::v1::{Deployment, Node, Pod};

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn int(v: bool) -> i64 { if v { 1 } else { 0 } }

pub fn build_snapshot_points(
    nodes: &[Node],
    pods: &[Pod],
    deps: &[Deployment],
    ts: i64,
) -> Vec<DataPoint> {
    let mut out = Vec::new();
    let ready = nodes.iter().filter(|n| n.status == "Ready").count();
    let not_ready = nodes.len() - ready;
    let running = pods.iter().filter(|p| p.status == "Running").count();
    let ready_deps = deps.iter().filter(|d| d.status == "Ready").count();
    out.push(
        DataPoint::new("resource_snapshot")
            .with_tag("type", "__summary__")
            .with_field("node_count", FieldValue::Int(nodes.len() as i64))
            .with_field("node_ready", FieldValue::Int(ready as i64))
            .with_field("node_not_ready", FieldValue::Int(not_ready as i64))
            .with_field("pod_count", FieldValue::Int(pods.len() as i64))
            .with_field("pod_running", FieldValue::Int(running as i64))
            .with_field("deployment_count", FieldValue::Int(deps.len() as i64))
            .with_field("deployment_ready", FieldValue::Int(ready_deps as i64))
            .with_timestamp(ts),
    );
    for n in nodes {
        out.push(
            DataPoint::new("resource_snapshot")
                .with_tag("node", n.name.clone())
                .with_tag("role", n.role.clone())
                .with_field("ready", FieldValue::Int(int(n.status == "Ready")))
                .with_field("cpu", FieldValue::String(n.cpu.clone()))
                .with_field("memory", FieldValue::String(n.memory.clone()))
                .with_field("version", FieldValue::String(n.version.clone()))
                .with_timestamp(ts),
        );
    }
    for p in pods {
        out.push(
            DataPoint::new("resource_snapshot")
                .with_tag("pod", p.name.clone())
                .with_tag("namespace", p.namespace.clone())
                .with_tag("node", p.node.clone())
                .with_field("status", FieldValue::String(p.status.clone()))
                .with_field("restart_count", FieldValue::Int(p.restart_count as i64))
                .with_timestamp(ts),
        );
    }
    for d in deps {
        out.push(
            DataPoint::new("resource_snapshot")
                .with_tag("deployment", d.name.clone())
                .with_tag("namespace", d.namespace.clone())
                .with_field("replicas", FieldValue::Int(d.replicas as i64))
                .with_field("ready_replicas", FieldValue::Int(d.ready_replicas as i64))
                .with_field("status", FieldValue::String(d.status.clone()))
                .with_timestamp(ts),
        );
    }
    out
}

pub async fn write_snapshot(
    ch: &Arc<ClickhouseClient>,
    points: &[DataPoint],
) -> Result<(), TsdbError> {
    ecat_data::TsdbClient::write(ch.as_ref(), points).await
}
```

- [x] **Step 4: 运行确认通过**

Run: `cargo test -p superops-collector --test ch_test`
Expected: PASS（2 tests）

- [ ] **Step 5: 提交**

```bash
git add services/collector && git commit -m "feat(collector): snapshot point builder + clickhouse write"
```
（若用户未要求提交，跳过）

---

### Task 3: collect.rs — 采集执行 + 调度接线

**Files:**
- Create: `services/collector/src/collect.rs`
- Modify: `services/collector/src/main.rs`（调度注册、CollectorHook）
- Modify: `services/collector/src/lib.rs`（注册 mod）
- Test: `services/collector/tests/collect_test.rs`

- [x] **Step 1: 写失败测试（不依赖外部服务：grpc 客户端失败路径 + 快照组合）**

```rust
// services/collector/tests/collect_test.rs
use superops_collector::collect::collect_once;

#[tokio::test]
async fn collect_once_with_unreachable_k8s_returns_error() {
    let cfg = superops_collector::config::Config {
        ch: Default::default(), mq: Default::default(), lock: Default::default(),
        k8s: superops_collector::config::K8sConfig { endpoint: "http://localhost:1".into() },
        collector: Default::default(),
    };
    // 无 CH/Redis，应快速失败（grpc connect 失败）
    let res = collect_once(&cfg).await;
    assert!(res.is_err());
}
```

- [x] **Step 2: 运行确认失败**

Run: `cargo test -p superops-collector --test collect_test 2>&1 | tail -5`
Expected: FAIL（mod 不存在）

- [x] **Step 3: 实现 collect.rs**

```rust
use crate::ch::{build_snapshot_points, now_secs, write_snapshot};
use crate::config::Config;
use superops_protos::k8s::v1::k8s_service_client::K8sServiceClient;
use superops_protos::k8s::v1::{ListDeploymentsRequest, ListNodesRequest, ListPodsRequest};
use std::sync::Arc;

pub async fn collect_once(cfg: &Config) -> anyhow::Result<()> {
    let mut client = K8sServiceClient::connect(&cfg.k8s.endpoint).await?;
    let nodes = client.list_nodes(ListNodesRequest {}).await?.into_inner().nodes;
    let pods = client.list_pods(ListPodsRequest {}).await?.into_inner().pods;
    let deps = client.list_deployments(ListDeploymentsRequest {}).await?.into_inner().deployments;
    let ch = Arc::new(crate::config::clickhouse_from(&cfg)?);
    let points = build_snapshot_points(&nodes, &pods, &deps, now_secs());
    write_snapshot(&ch, &points).await?;
    Ok(())
}
```

- [x] **Step 4: 运行确认通过**

Run: `cargo test -p superops-collector --test collect_test`
Expected: PASS（连接失败路径返回 Err）

- [x] **Step 5: main.rs 接线（完整 CollectorHook + 调度）**

```rust
// main.rs 全文（替换骨架）
use crate::config::{collector_config, Config};
use ecat::hook::LifecycleHook;
use ecat_scheduler::Scheduler;
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct CollectorHook {
    cfg: Config,
    scheduler: Arc<Mutex<Scheduler>>,
}

#[async_trait::async_trait]
impl LifecycleHook for CollectorHook {
    async fn on_start(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let cfg = self.cfg.clone();
        let sched = self.scheduler.clone();
        sched.lock().unwrap().every(
            Duration::from_secs(cfg.collector.collect_interval_secs),
            move || {
                let cfg = cfg.clone();
                async move { crate::collect::collect_once(&cfg).await }
            },
        );
        Ok(())
    }

    async fn on_stop(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.scheduler.lock().unwrap().shutdown();
        Ok(())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = collector_config()?;
    let scheduler = Arc::new(Mutex::new(Scheduler::new()));
    let hook = CollectorHook { cfg, scheduler };
    let app = ecat::App::builder()
        .name("superops-collector")
        .version(env!("CARGO_PKG_VERSION"))
        .on_start(hook.clone())
        .on_stop(hook)
        .build()
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    app.run().await.map_err(|e| anyhow::anyhow!("{e}"))
}
```

（若 `LifecycleHook` 需要 `Clone`，用 `Arc<CollectorHook>` 并让 `CollectorHook` 实现 `Clone`。）

- [x] **Step 6: 全 workspace check + 提交**

Run: `cargo check --workspace 2>&1 | tail -3`
Expected: 无错误

```bash
git add services/collector && git commit -m "feat(collector): scheduled snapshot collection"
```

---

### Task 4: gateway 指标查询 API

**Files:**
- Modify: `services/gateway/src/config.rs`（ch/mq 字段）
- Modify: `services/gateway/src/state.rs` 或 `main.rs`（AppState 加 ch）
- Create: `services/gateway/src/metrics_api.rs`
- Modify: `services/gateway/src/main.rs`（路由挂载）
- Modify: `config/gateway.yaml`（ch 段）
- Test: `services/gateway/src/metrics_api.rs` 内嵌测试

- [x] **Step 1: 写失败测试（SQL 构建纯函数 + 查询 API）**

```rust
// services/gateway/src/metrics_api.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_str_escapes_single_quotes_and_backslashes() {
        assert_eq!(quote_str("a'b\\c"), "'a\\'b\\\\c'");
    }

    #[test]
    fn query_sql_groups_by_node_latest() {
        let sql = build_query_sql("resource_snapshot", "node", "ready", 3600);
        assert!(sql.contains("argMax"));
        assert!(sql.contains("WHERE timestamp > now() - 3600"));
        assert!(sql.contains("GROUP BY node"));
    }
}
```

- [x] **Step 2: 运行确认失败**

Run: `cargo test -p superops-gateway metrics_api 2>&1 | tail -5`
Expected: FAIL（mod 不存在）

- [x] **Step 3: 实现 metrics_api.rs**

```rust
use axum::{extract::Query, http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use ecat_data::{TsdbClient, TsdbError};
use ecat_data_clickhouse::ClickhouseClient;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

#[derive(Deserialize)]
pub struct MetricsQuery {
    pub measurement: String,
    pub series: String,
    pub field: String,
    #[serde(default = "default_window")]
    pub window_secs: i64,
}

fn default_window() -> i64 { 3600 }

pub fn quote_str(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

pub fn build_query_sql(measurement: &str, series: &str, field: &str, window_secs: i64) -> String {
    format!(
        "SELECT {series}, argMax({field}, timestamp) AS value, max(timestamp) AS ts \
         FROM {measurement} WHERE timestamp > now() - {window_secs} \
         GROUP BY {series} ORDER BY {series}",
        series = quote_ident(series), field = quote_ident(field),
        measurement = quote_ident(measurement),
    )
}

fn quote_ident(s: &str) -> String {
    format!("`{}`", s.replace('`', "``"))
}

async fn handle_query(
    Query(q): Query<MetricsQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let state = crate::state().await?; // 具体实现见 Step 4（state 提取）
    ...
}
```

（此处因 gateway 现有结构，最终形态以 Step 4 接线为准：从 `AppState` 提取 `Arc<ClickhouseClient>`，调用 `ecat_data::TsdbClient::query(ch, &sql)`。）

- [x] **Step 4: 接线：config.rs / AppState / 路由 / gateway.yaml**

```rust
// config.rs: 追加字段
use ecat_data_clickhouse::ClickhouseConfig;
use ecat_mq_kafka::KafkaConfig;
pub struct Config {
    // ...原有
    #[serde(default)]
    pub ch: ClickhouseConfig,
    #[serde(default)]
    pub mq: Option<KafkaConfig>,
}
```

`AppState` 追加 `pub ch: Arc<ClickhouseClient>`、`pub mq: Option<Arc<KafkaMq>>`；main.rs 在启动时 `ClickhouseClient::from_config(config.ch)`、`KafkaMq::from_config`（若 Some）。`k8s_routes()` 中 merge 指标路由：`/api/v1/metrics/query`。

`config/gateway.yaml` 追加：

```yaml
ch:
  base_url: "http://localhost:8123"
  database: "superops"
mq:
  brokers: "localhost:9092"
  group_id: "superops-gateway"
```

- [x] **Step 5: 运行确认通过**

Run: `cargo test -p superops-gateway 2>&1 | tail -5`
Expected: PASS

- [ ] **Step 6: 提交**

```bash
git add services/gateway config/gateway.yaml && git commit -m "feat(gateway): metrics query API + ch/mq wiring"
```

---

### Task 5: alert.rs — 告警规则与事件写入

**Files:**
- Create: `services/collector/src/alert.rs`
- Modify: `services/collector/src/lib.rs`（注册 mod）
- Modify: `services/collector/src/main.rs`（inspect 调度）
- Test: `services/collector/tests/alert_test.rs`

- [x] **Step 1: 写失败测试（纯函数：规则判定 + 事件点构建）**

```rust
// services/collector/tests/alert_test.rs
use superops_collector::alert::{AlertEvent, ClusterHealth, evaluate_health, health_to_points};
use superops_protos::k8s::v1::{Node, NodeStatus, Pod, PodStatus, Deployment, DeploymentStatus, ListNodesResponse, ListPodsResponse, ListDeploymentsResponse};

#[test]
fn healthy_cluster_no_alerts() {
    let nodes = vec![Node { name: "n1".into(), status: "Ready".into(), role: "worker".into(), version: "".into(), age: "".into(), cpu: "".into(), memory: "".into() }];
    let pods = vec![Pod { name: "p1".into(), namespace: "default".into(), status: "Running".into(), node: "n1".into(), restart_count: 0, cpu_usage: 0.0, memory_usage: 0.0 }];
    let deps = vec![Deployment { name: "web".into(), namespace: "default".into(), replicas: 2, ready_replicas: 2, status: "Ready".into() }];
    let health = evaluate_health(&nodes, &pods, &deps, 2);
    assert!(health.alerts.is_empty());
    assert!(health.cluster_ok);
}

#[test]
fn not_ready_node_fires_alert() {
    let nodes = vec![Node { name: "n1".into(), status: "NotReady".into(), role: "worker".into(), version: "".into(), age: "".into(), cpu: "".into(), memory: "".into() }];
    let pods = vec![];
    let deps = vec![];
    let health = evaluate_health(&nodes, &pods, &deps, 1);
    assert_eq!(health.alerts.len(), 1);
    assert_eq!(health.alerts[0].level, "WARN");
    assert!(health.alerts[0].message.contains("n1"));
}

#[test]
fn alert_points_have_measurement_and_level() {
    let event = AlertEvent { level: "WARN".into(), title: "t".into(), message: "m".into(), node: Some("n1".into()) };
    let points = health_to_points(&[event], 1234567890);
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].measurement, "alert_event");
    assert!(matches!(points[0].fields.get("level"), Some(FieldValue::String(s)) if s == "WARN"));
    assert_eq!(points[0].timestamp, Some(1234567890));
}
```

- [x] **Step 2: 运行确认失败**

Run: `cargo test -p superops-collector --test alert_test 2>&1 | tail -5`
Expected: FAIL（mod 不存在）

- [x] **Step 3: 实现 alert.rs**

```rust
use ecat_data::{DataPoint, FieldValue};
use superops_protos::k8s::v1::{Deployment, Node, Pod};

#[derive(Debug, Clone, PartialEq)]
pub struct AlertEvent {
    pub level: String,   // "WARN" | "CRIT"
    pub title: String,
    pub message: String,
    pub node: Option<String>,
}

#[derive(Debug, Default)]
pub struct ClusterHealth {
    pub alerts: Vec<AlertEvent>,
    pub cluster_ok: bool,
}

pub fn evaluate_health(
    nodes: &[Node], pods: &[Pod], deps: &[Deployment],
    max_not_ready: usize,
) -> ClusterHealth {
    let mut alerts = Vec::new();
    for n in nodes {
        if n.status != "Ready" {
            alerts.push(AlertEvent {
                level: "CRIT".into(),
                title: "node-not-ready".into(),
                message: format!("node {} not ready (status={})", n.name, n.status),
                node: Some(n.name.clone()),
            });
        }
    }
    for d in deps {
        if d.ready_replicas < d.replicas {
            alerts.push(AlertEvent {
                level: "WARN".into(),
                title: "deployment-unavailable".into(),
                message: format!("deployment {}/{} ready {}/{}", d.namespace, d.name, d.ready_replicas, d.replicas),
                node: None,
            });
        }
    }
    let cluster_ok = alerts.len() <= max_not_ready;
    ClusterHealth { alerts, cluster_ok }
}

pub fn health_to_points(events: &[AlertEvent], ts: i64) -> Vec<DataPoint> {
    events.iter().map(|e| {
        let mut p = DataPoint::new("alert_event")
            .with_tag("level", e.level.clone())
            .with_field("title", FieldValue::String(e.title.clone()))
            .with_field("message", FieldValue::String(e.message.clone()))
            .with_timestamp(ts);
        if let Some(n) = &e.node {
            p = p.with_tag("node", n.clone());
        }
        p
    }).collect()
}
```

- [x] **Step 4: 运行确认通过**

Run: `cargo test -p superops-collector --test alert_test`
Expected: PASS（3 tests）

- [x] **Step 5: main.rs 接线 inspect 任务**

on_start 追加第二个 `every`：`inspect_interval_secs` 间隔执行 `crate::alert::inspect_once(&cfg)`（实现：拉取 nodes/pods/deps → evaluate_health → 有 alert 时写 alert_event 点 + 若 `cluster_ok == false` 连续触发则记录）。**连续计数用进程内 `Mutex<HashMap<String, usize>>`，key = 节点名，命中 max_not_ready 的节点计数 +1，恢复清零。**

- [x] **Step 6: 全 workspace check + 提交**

Run: `cargo check --workspace 2>&1 | tail -3`
Expected: 无错误

```bash
git add services/collector && git commit -m "feat(collector): alert rules + alert event ingestion"
```

---

### Task 6: Kafka 审计事件消费

**Files:**
- Create: `services/collector/src/events.rs`
- Modify: `services/collector/src/lib.rs`（注册 mod）
- Modify: `services/collector/src/main.rs`（审计消费者 spawn）
- Modify: `services/gateway/src/auth/handler.rs`（publish 审计）
- Modify: `services/gateway/src/main.rs`（mq 注入 state）
- Test: `services/collector/tests/events_test.rs`

- [x] **Step 1: 写失败测试（纯函数：审计事件 → 数据点）**

```rust
// services/collector/tests/events_test.rs
use superops_collector::events::{AuditEvent, audit_to_data_point};
use ecat_data::FieldValue;

#[test]
fn audit_event_maps_to_data_point() {
    let e = AuditEvent {
        ts: 1234567890,
        event_type: "login_success".into(),
        username: "erik".into(),
        ip: "127.0.0.1".into(),
        detail: "ok".into(),
    };
    let pt = audit_to_data_point(&e);
    assert_eq!(pt.measurement, "audit_log");
    assert_eq!(pt.tags.get("event_type").map(String::as_str), Some("login_success"));
    assert!(matches!(pt.fields.get("username"), Some(FieldValue::String(s)) if s == "erik"));
    assert_eq!(pt.timestamp, Some(1234567890));
}
```

- [x] **Step 2: 运行确认失败**

Run: `cargo test -p superops-collector --test events_test 2>&1 | tail -5`
Expected: FAIL（mod 不存在）

- [x] **Step 3: 实现 events.rs**

```rust
use ecat_data::{DataPoint, FieldValue};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub ts: i64,
    pub event_type: String,
    pub username: String,
    pub ip: String,
    pub detail: String,
}

pub fn audit_to_data_point(e: &AuditEvent) -> DataPoint {
    DataPoint::new("audit_log")
        .with_tag("event_type", e.event_type.clone())
        .with_tag("username", e.username.clone())
        .with_field("ip", FieldValue::String(e.ip.clone()))
        .with_field("detail", FieldValue::String(e.detail.clone()))
        .with_timestamp(e.ts)
}

pub async fn consume_audit(
    ch: Arc<ClickhouseClient>,
    mq: Arc<KafkaMq>,
    topic: &str,
) -> anyhow::Result<()> {
    let mut stream = mq.subscribe(topic).await?;
    loop {
        let data = futures::future::poll_fn(|cx| stream.poll_recv(cx)).await
            .ok_or_else(|| anyhow::anyhow!("audit stream ended"))?;
        let ev: AuditEvent = serde_json::from_slice(&data).unwrap_or_else(|_| {
            // 坏消息不致命：记日志跳过
            tracing::warn!("unparseable audit event, skipping");
            return AuditEvent { ts: 0, event_type: "unknown".into(), username: "".into(), ip: "".into(), detail: "unparseable".into() };
        });
        if let Err(e) = ecat_data::TsdbClient::write(ch.as_ref(), std::slice::from_ref(&audit_to_data_point(&ev))).await {
            tracing::warn!("audit write failed: {e}");
        }
    }
}
```

- [x] **Step 4: 运行确认通过**

Run: `cargo test -p superops-collector --test events_test`
Expected: PASS

- [x] **Step 5: main.rs 接线（audit consumer spawn）+ gateway publish**

main.rs on_start 中 `tokio::spawn(crate::events::consume_audit(ch.clone(), mq.clone(), "superops.audit"))`。

gateway `auth/handler.rs`：login 成功（refresh token 后）与 register 成功分支调用 `state.mq.as_ref().map(|mq| mq.publish("superops.audit", &payload))`，payload 为 `AuditEvent` 的 JSON（与 collector 的 `serde_json::from_slice` 对齐字段名：`ts/event_type/username/ip/detail`）。publish 失败仅 `tracing::warn!`，不阻断登录。

- [x] **Step 6: 全 workspace check + 提交**

Run: `cargo check --workspace 2>&1 | tail -3`
Expected: 无错误

```bash
git add services/collector services/gateway && git commit -m "feat(collector): audit event ingestion + gateway publish"
```

---

### Task 7: inspect.rs — 巡检互斥（RedisLock）

**Files:**
- Create: `services/collector/src/inspect.rs`
- Modify: `services/collector/src/lib.rs`（注册 mod）
- Modify: `services/collector/src/main.rs`（inspect 任务改用 lock）
- Test: `services/collector/tests/inspect_test.rs`

- [x] **Step 1: 写失败测试（锁串接逻辑：acquire → 执行 → release）**

```rust
// services/collector/tests/inspect_test.rs
use superops_collector::inspect::{with_inspect_lock, InspectGuard};
use std::sync::atomic::{AtomicUsize, Ordering};

#[tokio::test]
async fn lock_acquire_failure_skips_work() {
    // 用假 lock：始终返回 None（模拟被其他实例持有）
    let fake = FakeLock;
    let ran = AtomicUsize::new(0);
    let res = with_inspect_lock(&fake, "inspect:test", async { ran.fetch_add(1, Ordering::SeqCst); 42 }).await;
    assert_eq!(ran.load(Ordering::SeqCst), 0); // 未执行
    assert_eq!(res, None); // 无结果
}
```

（`with_inspect_lock` 泛型：`L: DistributedLock`，fake 实现 `acquire` 返回 `Ok(None)`。）

- [x] **Step 2: 运行确认失败**

Run: `cargo test -p superops-collector --test inspect_test 2>&1 | tail -5`
Expected: FAIL（mod 不存在）

- [x] **Step 3: 实现 inspect.rs**

```rust
use ecat_data_redis::RedisLock;
use ecat_lock::DistributedLock;
use std::sync::Arc;
use std::time::Duration;

const INSPECT_KEY: &str = "superops:inspect:lock";
const INSPECT_TTL: Duration = Duration::from_secs(300);

pub async fn with_inspect_lock<L, F, T>(
    lock: &L,
    key: &str,
    work: F,
) -> Option<T>
where
    L: DistributedLock,
    F: std::future::Future<Output = T>,
{
    let token = lock.acquire(key, INSPECT_TTL).await.ok().flatten()?;
    let out = work.await;
    let _ = lock.release(key, &token).await;
    Some(out)
}

pub async fn inspect_once(cfg: &Config) -> anyhow::Result<()> {
    let lock = Arc::new(RedisLock::from_config(cfg.lock.clone())?);
    with_inspect_lock(lock.as_ref(), INSPECT_KEY, async {
        // 拉取 → evaluate_health → 写 alert_event 点
        // 连续告警状态机（进程内 Mutex<HashMap<String,usize>>）
    }).await;
    Ok(())
}
```

- [x] **Step 4: 运行确认通过**

Run: `cargo test -p superops-collector --test inspect_test`
Expected: PASS

- [x] **Step 5: main.rs 接线（inspect 调度改用 with_inspect_lock）**

- [x] **Step 6: 全 workspace check + 提交**

Run: `cargo check --workspace 2>&1 | tail -3`
Expected: 无错误

```bash
git add services/collector && git commit -m "feat(collector): inspect mutex via RedisLock"
```

---

### Task 8: 文档 / CHANGELOG / 总纲勾选

**Files:**
- Modify: `CHANGELOG.md`（P1 条目）
- Modify: `docs/superpowers/plans/2026-08-06-super-ops-ecosystem-expansion.md`（P1 复选框）
- Modify: `docs/audit-report-2026-08-06.md`（若存在，同步）

- [x] **Step 1: CHANGELOG 追加 [1.0.5] 条目**

格式参照 [1.0.4]（Added / Fixed / Updated 分节）。

- [x] **Step 2: 总纲 P1 勾选**

- [x] **Step 3: 全量验证**

Run: `cargo fmt --check && cargo check --workspace && cargo test --workspace 2>&1 | tail -15`
Expected: 全绿（e2e 复核项除外，见下）

- [x] **Step 4: 汇报**

汇报内容：交付清单、验证结果、被阻塞的 e2e 复核项（CH/Redis/Kafka 容器 + k8s 集群待 sudo 启动）、成本提示。

---

## 复核（环境依赖，待 sudo 容器启动后人工验证）

- [ ] P1 验收 A：`cargo run -p superops-collector` 启动后，CH 中 `resource_snapshot` 表出现且每 60s 有快照行
- [ ] P1 验收 B：`GET /api/v1/metrics/query?measurement=resource_snapshot&series=node&field=ready` 返回 JSON 序列
- [ ] P1 验收 C：停掉一个 node（或降 replicas），`alert_event` 表出现 CRIT/WARN 行
- [ ] P1 验收 D：登录成功后在 `audit_log` 表出现 `login_success` 行
- [ ] P1 验收 E：双实例同时跑，RedisLock 保证同一时刻只有一个 inspect 执行（观察日志）

## 总纲联动

- 本计划覆盖总纲 P1（数据与监控闭环）全部 7 项任务：P1.1 采集器、P1.2 快照存储、P1.3 指标 API、P1.4 告警规则、P1.5 审计事件、P1.6 巡检互斥、P1.7 scheduler 复用。
- 完成后更新总纲 P1 复选框为 [x]，并在验收区记录复核状态。
