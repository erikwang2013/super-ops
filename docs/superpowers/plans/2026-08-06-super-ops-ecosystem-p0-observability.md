# SuperOps e-cat 生态扩展 P0（可观测地基）实现计划

> **For agentic workers:** 使用 superpowers:subagent-driven-development（推荐）或 superpowers:executing-plans 逐任务执行。步骤用 checkbox（`- [ ]`）跟踪。
> **项目提交规则（CLAUDE.md）：默认不提交。每个任务验证通过后向用户汇报，用户确认后才 `git add` + `git commit`。**

**Goal:** 为 gateway 接入 e-cat 生态的 health + metrics，补齐 compose 生态（Prometheus/Kafka/Consul），同步文档。

**Architecture:** 复用 `ecat-health::HealthRegistry`（/health liveness + /ready readiness，含 MySQL 依赖检查）与 `ecat-metrics`（Prometheus 单例 registry + 文本导出），在现有 axum Router 上新增三个公开端点；compose 增加三个基础设施服务；Makefile 无需变更。

**Tech Stack:** ecat-health、ecat-metrics、prometheus 0.13、axum 0.8、sqlx 0.8（`connect_lazy`）、docker compose（prom/prometheus v2.53、apache/kafka 3.8 KRaft、hashicorp/consul 1.20 dev 模式）。

**前置事实（已核实）：**
- `ecat_health::HealthRegistry::new().with_check(FnCheck::new(name, f)).into_router()` 产出路由 `/health`（恒 200）与 `/ready`（依赖检查；任一 fail → 503 JSON）
- `ecat_metrics::registry() -> &'static prometheus::Registry`（OnceLock 单例）、`metrics_text() -> String`（Prometheus 文本）
- gateway 的 `MySqlPool` 由 `config.database.url` 创建（main.rs:34）；`/api/health` 已存在，与新增 `/health` 不冲突
- 所有新端点（/health、/ready、/metrics）放在**主 Router**（无认证中间件层），k8s 路由组的认证不受影响

---

## Task P0.1: gateway 健康检查（/health + /ready）

**Files:**
- Modify: `services/gateway/Cargo.toml`（+`ecat-health`）
- Create: `services/gateway/src/health.rs`
- Modify: `services/gateway/src/main.rs`

- [ ] **Step 1: 添加依赖**

`services/gateway/Cargo.toml` 的 `[dependencies]` 中 `ecat-data-sqlx` 行之后加：

```toml
ecat-health = { path = "../../ecat-health" }
```

- [ ] **Step 2: 写失败测试** — Create `services/gateway/src/health.rs`（含测试）：

```rust
use axum::Router;
use ecat_health::{FnCheck, HealthRegistry};
use sqlx::MySqlPool;

pub fn health_router(pool: MySqlPool) -> Router {
    let mysql = FnCheck::new("mysql", move || {
        let pool = pool.clone();
        async move {
            pool.acquire()
                .await
                .map(|_| ())
                .map_err(|e| format!("mysql unavailable: {e}"))
        }
    });
    HealthRegistry::new().with_check(mysql).into_router()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::util::ServiceExt;

    #[tokio::test]
    async fn liveness_returns_200() {
        let router = HealthRegistry::new().into_router();
        let res = router
            .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn readiness_503_when_mysql_down() {
        // connect_lazy 不建立真实连接，acquire 时才失败 → 离线可测
        let pool = MySqlPool::connect_lazy("mysql://u:p@127.0.0.1:1/db").unwrap();
        let router = health_router(pool);
        let res = router
            .oneshot(Request::builder().uri("/ready").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
```

- [ ] **Step 3: 运行测试确认失败**

Run: `cd services/gateway && cargo test -p superops-gateway health`
Expected: 编译失败 `unresolved import crate::health` / module 不存在

- [ ] **Step 4: 接线 main.rs**

`services/gateway/src/main.rs`：
1. `mod config;` 前加 `mod health;`
2. 主 Router 的 `.merge(k8s)` 后加 `.merge(health::health_router(pool.clone()))`

- [ ] **Step 5: 运行测试确认通过**

Run: `cd services/gateway && cargo test -p superops-gateway health`
Expected: 2 passed（liveness 200、mysql down → ready 503）

- [ ] **Step 6: 运行时验证**

Run: `curl -s -o /dev/null -w '%{http_code}' http://localhost:8080/health` → `200`；`curl -s http://localhost:8080/ready` → 200 + `{"results":[{"name":"mysql","status":"ok"}]}`
Expected: 上述输出；`cargo check -p superops-gateway` 零警告

## Task P0.2: gateway /metrics（Prometheus 导出）

**Files:**
- Modify: `services/gateway/Cargo.toml`（+`ecat-metrics`、`prometheus`）
- Create: `services/gateway/src/metrics.rs`
- Modify: `services/gateway/src/main.rs`

- [ ] **Step 1: 添加依赖**

`services/gateway/Cargo.toml` 的 `[dependencies]` 中 `ecat-health` 行后加：

```toml
ecat-metrics = { path = "../../ecat-metrics" }
prometheus = "0.13"
```

- [ ] **Step 2: 写失败测试** — Create `services/gateway/src/metrics.rs`（含测试）：

```rust
use axum::body::Body;
use axum::http::Request;
use axum::middleware::Next;
use axum::response::Response;
use prometheus::{IntCounter, opts, register_int_counter_with_registry};
use std::sync::OnceLock;

fn requests_counter() -> &'static IntCounter {
    static COUNTER: OnceLock<IntCounter> = OnceLock::new();
    COUNTER.get_or_init(|| {
        register_int_counter_with_registry!(
            opts!("superops_http_requests_total", "Total HTTP requests handled by gateway"),
            ecat_metrics::registry()
        )
        .expect("metric registration should not conflict")
    })
}

pub async fn metrics_handler() -> String {
    ecat_metrics::metrics_text()
}

pub async fn count_requests(req: Request<Body>, next: Next) -> Response {
    requests_counter().inc();
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::get;
    use axum::http::StatusCode;
    use tower::util::ServiceExt;

    #[tokio::test]
    async fn requests_metric_appears_in_text_format() {
        let router = axum::Router::new()
            .route("/", get(|| async {}))
            .layer(axum::middleware::from_fn(count_requests));
        let res = router
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert!(ecat_metrics::metrics_text().contains("superops_http_requests_total"));
    }
}
```

- [ ] **Step 3: 运行测试确认失败**

Run: `cd services/gateway && cargo test -p superops-gateway requests_metric`
Expected: 编译失败 `unresolved import crate::metrics` / module 不存在

- [ ] **Step 4: 接线 main.rs**

`services/gateway/src/main.rs`：
1. `mod health;` 后加 `mod metrics;`
2. `.route("/api/health", get(health))` 后加 `.route("/metrics", get(metrics::metrics_handler))`
3. `.with_state(state)` 前加 `.layer(middleware::from_fn(metrics::count_requests))`

- [ ] **Step 5: 运行测试确认通过**

Run: `cd services/gateway && cargo test -p superops-gateway requests_metric`
Expected: 1 passed

- [ ] **Step 6: 运行时验证**

Run: `curl -s http://localhost:8080/metrics | grep superops`
Expected: 输出 `# HELP superops_http_requests_total ...` 与 `# TYPE superops_http_requests_total counter` 及计数行

## Task P0.3: compose 补齐 Prometheus / Kafka / Consul

**Files:**
- Modify: `deploy/docker-compose.yml`
- Create: `deploy/prometheus.yml`
- Modify: `deploy/.env.example`

- [ ] **Step 1: 写 compose 配置** — `deploy/docker-compose.yml` 的 `clickhouse:` 服务块后（`volumes:` 前）加：

```yaml
  prometheus:
    image: prom/prometheus:v2.53.0
    ports: ["9095:9090"]
    volumes:
      - ./prometheus.yml:/etc/prometheus/prometheus.yml:ro
    extra_hosts:
      - "host.docker.internal:host-gateway"
    healthcheck:
      test: ["CMD", "wget", "-q", "--spider", "http://localhost:9090/-/healthy"]
      interval: 5s
      retries: 5

  kafka:
    image: apache/kafka:3.8.0
    ports: ["9092:9092"]
    environment:
      KAFKA_NODE_ID: 1
      KAFKA_PROCESS_ROLES: broker,controller
      KAFKA_LISTENERS: PLAINTEXT://:9092,CONTROLLER://:9093
      KAFKA_ADVERTISED_LISTENERS: PLAINTEXT://localhost:9092
      KAFKA_CONTROLLER_LISTENER_NAMES: CONTROLLER
      KAFKA_LISTENER_SECURITY_PROTOCOL_MAP: CONTROLLER:PLAINTEXT,PLAINTEXT:PLAINTEXT
      KAFKA_CONTROLLER_QUORUM_VOTERS: 1@localhost:9093
      KAFKA_OFFSETS_TOPIC_REPLICATION_FACTOR: 1
      KAFKA_TRANSACTION_STATE_LOG_REPLICATION_FACTOR: 1
      KAFKA_TRANSACTION_STATE_LOG_MIN_ISR: 1
      KAFKA_GROUP_INITIAL_REBALANCE_DELAY_MS: 0
    healthcheck:
      test: ["CMD-SHELL", "/opt/kafka/bin/kafka-topics.sh --bootstrap-server localhost:9092 --list >/dev/null 2>&1"]
      interval: 10s
      retries: 12
      start_period: 30s

  consul:
    image: hashicorp/consul:1.20
    ports: ["8500:8500"]
    command: ["agent", "-dev", "-client=0.0.0.0"]
    healthcheck:
      test: ["CMD", "consul", "members"]
      interval: 5s
      retries: 5
```

注：gateway 跑在宿主机（非 compose 网络），Prometheus 通过 `host.docker.internal:8080` 抓取，`extra_hosts: host-gateway` 是 Linux 下的必要映射。

- [ ] **Step 2: 写 Prometheus 配置** — Create `deploy/prometheus.yml`：

```yaml
global:
  scrape_interval: 15s
  evaluation_interval: 15s

scrape_configs:
  - job_name: superops-gateway
    metrics_path: /metrics
    static_configs:
      - targets: ["host.docker.internal:8080"]
```

- [ ] **Step 3: 更新 .env.example**

`deploy/.env.example` 末尾追加（供后续阶段使用，均有默认值）：

```bash
# e-cat ecosystem (P0+)
KAFKA_BROKERS=localhost:9092
CONSUL_ADDR=http://localhost:8500
OTLP_ENDPOINT=http://localhost:4317
CH_URL=http://localhost:8124
PROMETHEUS_PORT=9095
```

- [ ] **Step 4: 校验并启动**

Run: `cd deploy && docker compose config --quiet`
Expected: 无输出（exit 0）

Run: `docker compose up -d`
Expected: 6 个服务启动；`docker compose ps` 中 prometheus/kafka/consul 为 healthy（kafka 需等待 start_period）

- [ ] **Step 5: 运行时验证**

Run: `curl -s http://localhost:9095/api/v1/targets | grep -o '"health":"up"' | head -1`
Expected: `"health":"up"`（gateway target 抓取成功）

Run: `curl -s http://localhost:8500/v1/status/leader | head -c 40`
Expected: 形如 `"127.0.0.1:8300"` 的领导地址（consul 就绪）

## Task P0.4: 文档与 CHANGELOG 同步

**Files:**
- Modify: `README.md` / `README.en.md`
- Modify: `CHANGELOG.md`

- [ ] **Step 1: README 端口表与配置更新**

`README.md` 与 `README.en.md` 的端口表新增三行：

```markdown
| Prometheus | 9095 |
| Kafka | 9092 |
| Consul | 8500 |
```

`配置` 小节的环境变量表新增（中文版）：

```markdown
| `KAFKA_BROKERS` | Kafka broker 地址（P1 事件总线用） |
| `CONSUL_ADDR` | Consul 地址（P2 注册/远程配置用） |
```

并在"测试与 CI"或架构说明处补充一句：gateway 暴露 `/health`、`/ready`、`/metrics`（Prometheus 文本）。

- [ ] **Step 2: CHANGELOG 追加**

`CHANGELOG.md` 顶部（[1.0.3] 之前）加：

```markdown
## [1.0.4] — 2026-08-06 — SuperOps P0 可观测地基

### Added
- gateway 接入 `ecat-health`：`/health`（liveness）与 `/ready`（readiness，含 MySQL 依赖检查，失败 503）
- gateway 接入 `ecat-metrics`：`/metrics` Prometheus 文本导出 + `superops_http_requests_total` 请求计数
- docker-compose 补齐 Prometheus（9095）、Kafka KRaft 单节点（9092）、Consul dev 模式（8500）
- `deploy/prometheus.yml` scrape 配置（gateway:8080/metrics）；`.env.example` 追加 KAFKA_BROKERS/CONSUL_ADDR/OTLP_ENDPOINT/CH_URL/PROMETHEUS_PORT
```

- [ ] **Step 3: 验证**

Run: 重启 gateway（`pkill -f 'debug/superops-gatewa[y]'` 后重新 `cargo run`），`curl -s http://localhost:8080/metrics | head -3` 有输出
Expected: `/health` 200、`/ready` 200、`/metrics` 文本正常

**P0 验收:** 三端点稳定；compose 六服务 healthy；Prometheus target UP；`cargo test -p superops-gateway` 全绿；文档与实际端口一致。验收后向用户汇报，等待确认是否提交。
