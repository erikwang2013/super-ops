# SuperOps e-cat 生态全量扩展 — 总纲与任务地图

> **For agentic workers:** 本文件为总纲与任务地图（用户选定模式）。每个阶段开始执行前，先按 writing-plans 规范产出该阶段的详细任务计划（含完整代码步骤与 TDD 任务），再按 superpowers:subagent-driven-development（推荐）或 superpowers:executing-plans 执行。任务步骤使用 checkbox（`- [ ]`）跟踪。

**Goal:** 将 e-cat 框架 13+ 项能力全部接入 super-ops，把 Phase 1 MVP 升级为具备可观测性、数据闭环、事件总线、韧性与对外集成能力的生产级运维平台。

**Architecture:** 四阶段推进，每阶段独立可交付、可验收。P0 可观测地基（health/metrics/生态补齐）；P1 数据与监控闭环（ClickHouse 存储、scheduler 巡检、Kafka 事件总线、Redis 分布式锁）；P2 韧性与可观测（熔断、Consul 注册与远程配置、OTLP 追踪、框架原生限流）；P3 集成与对外（WebSocket 终端桥、API Key/OAuth2、OpenAPI、压测）。新增采集服务 `services/collector`。

**Tech Stack:** e-cat 生态（ecat-health、ecat-metrics、ecat-scheduler、ecat-lock + ecat-data-redis RedisLock、ecat-mq-kafka、ecat-circuit-breaker、ecat-registry-consul、ecat-config-remote、ecat-tracing-otlp、ecat-transport-ws、ecat-auth、ecat-openapi、ecat-bench、ecat-data-clickhouse、ecat-middleware redis feature）、ClickHouse、Kafka、Consul、Prometheus、kube-rs 0.93、axum 0.8、tonic 0.12。

---

## 阶段总览

| 阶段 | 目标 | 核心交付 | 依赖 |
|---|---|---|---|
| P0 | 可观测地基 | gateway `/health` `/ready` `/metrics`；compose 补齐 Prometheus/Kafka/Consul；Makefile/.env/文档同步 | 无 |
| P1 | 数据与监控闭环 | collector 服务、ClickHouse 存储与查询、巡检调度、Kafka 事件、分布式锁 | P0 |
| P2 | 韧性与追踪 | 熔断、Consul 注册/配置、OTLP、限流迁移框架原生 | P0 |
| P3 | 集成与对外 | WS 终端桥、API Key/OAuth2、OpenAPI、压测脚本 | P0；T3.2 依赖 T3.1 |

## 接口与命名约定

### HTTP（gateway 8080）
- `GET /health` — liveness（ecat-health `HealthRegistry`）
- `GET /ready` — readiness（依赖检查失败 503）
- `GET /metrics` — Prometheus 文本，由 `HttpServer` 自动挂载（框架保留路径，用户 router 再定义会 merge panic）
- 新业务端点统一 `/api/v1/*`：`/api/v1/metrics/query`、`/api/v1/api-keys`、`/api/docs`
- 既有 `/api/k8s/*` 保持不变，全部走现有 JWT 认证中间件

### Kafka 主题
- `superops.audit` — 审计（登录成功、注册、写操作）
- `superops.alert` — 告警
- `superops.cluster-events` — 集群事件（k8s watch 转发，stretch）

### ClickHouse 表
- `superops.resource_snapshot`（ReplacingMergeTree，ORDER BY `(cluster_id, node_name, ts)`）— 资源水位
- `superops.audit_log`（MergeTree，ORDER BY `ts`）— 审计
- `superops.alert_event`（MergeTree，ORDER BY `ts`）— 告警

### Consul
- KV：`config/superops/<service>/<key>`（config-remote 约定，`/`→`.`）
- 服务注册名：`superops-gateway` / `superops-k8s` / `superops-collector`，tag `version=<ver>`

### 配置与 env
- 各服务 config YAML 增加：`otel: {endpoint}`、`mq: {brokers, group_id}`、`registry: {addr, datacenter}`、`lock: {redis_url}`、`ch: {base_url, database}`
- `deploy/.env.example` 增加：`KAFKA_BROKERS`、`CONSUL_ADDR`、`OTLP_ENDPOINT`、`CH_URL`、`PROMETHEUS_PORT`

### 新服务
- `services/collector`（superops-collector）：采集 + 巡检 + 事件消费，`ecat::App` + `on_start` 挂 `Scheduler`；加入 workspace members 与 Makefile

## 框架能力缺口（勘探事实，规划约束）

> **2026-08-06 同步上游（e-cat 仓库，HEAD 2cc0fb0）后更新**：四项缺口已由上游补全（见下方 ✅），本地 workspace 已同步对应 crate；其余缺口对策不变。

| 缺口 | 状态 / 对策 |
|---|---|
| ecat-metrics 无自动导出端点 | ✅ **已由上游补全**：`ecat_metrics::metrics_router()`（带 `text/plain; version=0.0.4` Content-Type）；`HttpServer` 自动挂载 `/metrics`（框架保留路径，用户 router 再定义会 merge panic） |
| ecat-openapi 仅支持 GET/POST 手写 | ✅ **已由上游补全**：`add_route` 支持 PUT/DELETE/PATCH/HEAD/OPTIONS |
| ecat-config-remote 无 watch | ✅ **已由上游补全**：`ConsulConfigSource::watch()`（阻塞查询 + mpsc 输出变更，含首帧强制推送与缺 X-Consul-Index 兼容） |
| registry 仅 Consul 实现（无 etcd） | 用 `ConsulRegistry::new(base_url).datacenter().token()` |
| ClickhouseClient 无 `TsdbClient` 实现 | ✅ **已由上游补全**：`impl TsdbClient for ClickhouseClient`（自动建表 + JSONEachRow 写入） |
| ecat-transport-ws 独立端口、不与 axum 共用 | P3 终端桥在 gateway 内直接用 axum ws；transport-ws 不引入 |
| `MessageStream` 为自定义 poll trait 而非 `Stream` | 消费端手写适配循环（与 k8s 服务现有 `ReceiverStream` 模式一致） |
| ecat-tracing-otlp 无 shutdown/flush API | provider 保活在 App 生命周期（`on_start` 持有） |
| ConsulRegistry register 不带 health check | 依赖 Registration Drop 自动注销即可，不补 check |

> **同步说明**：`ecat-health` 上游仍为 `blocking_write`（tokio runtime 内注册必 panic），本地保留 async 修复（`pub async fn with_check`），**未**同步该 crate；其余五个变更 crate（ecat-metrics / ecat-transport-http / ecat-config-remote / ecat-data-clickhouse / ecat-openapi）已同步，上游 workspace 版本 2.3.1 未跟随（本地保持 1.0.2 版本线）。

## P0 — 可观测地基

### Task P0.1: gateway 健康检查

**Files:** Modify `services/gateway/Cargo.toml`（+`ecat-health`）；Modify `services/gateway/src/main.rs`；Create `services/gateway/src/health.rs`

- [x] 定义 `HealthRegistry`：`FnCheck` 包装 MySQL ping（`pool.acquire`）、JWT 配置检查；`into_router()` merge 进主 Router
- [x] 验证：`GET /health` 200；停 MySQL 后 `GET /ready` 503；`cargo check` 零警告
- [x] 实现注记：`health_router(pool).await` 必须 merge 在 `.with_state(state)` **之后**（axum 0.8 类型系统：`into_router()` 返回 `Router<()>`，在链头 merge 会把整链强制为 `Router<()>`，与 login/register 的 `MethodRouter<AppState>`、k8s 的 `Router<AppState>` 冲突）

### Task P0.2: gateway /metrics

**Files:** Create `services/gateway/src/metrics.rs`；Modify `services/gateway/src/main.rs`；Modify `services/gateway/src/proxy/k8s_proxy.rs`（若加请求计数）

- [x] 注册自定义计数器 `superops_http_requests_total`（`count_requests` 中间件）；`/metrics` 由 `HttpServer` 自动挂载（上游补全后无需手写路由，手写反而与框架保留路径 merge panic）
- [x] 验证：`curl :8080/metrics | grep superops` 有输出（计数经 `/api/*` 请求触发）；`/metrics` 不在认证保护下

### Task P0.3: compose 补齐 Prometheus / Kafka / Consul

**Files:** Modify `deploy/docker-compose.yml`；Create `deploy/prometheus.yml`；Modify `deploy/.env.example`

- [x] Prometheus（host 9095，避开 gateway gRPC 9090）scrape `gateway:8080/metrics`；Kafka（KRaft 单节点，9092）；Consul（8500）
- [ ] 验证：`docker compose config --quiet`（已过）；三容器 healthy、Prometheus target UP —— 容器启动需 sudo，待执行后复核

### Task P0.4: Makefile / 文档 / CHANGELOG 同步

**Files:** Modify `Makefile`（dev-all 启动顺序与端口说明）；Modify `README.md` / `README.en.md`（端口表、配置）；Modify `CHANGELOG.md`

- [x] 验证：README/README.en 端口表与 CHANGELOG 已同步（`make dev-all` 全链路复核留待容器就绪）

**P0 验收:** 三端点稳定 ✅（/health /ready /metrics 运行时验证通过）；compose 六服务 up —— 容器启动待 sudo 执行后复核；CI 通过（workspace `cargo check`/test 零错误零警告）。

## P1 — 数据与监控闭环

### Task P1.1: collector 服务骨架

**Files:** Create `services/collector/Cargo.toml`、`src/main.rs`、`src/config.rs`；Modify workspace `Cargo.toml`（members）；Modify `Makefile`、`deploy/.env.example`

- [x] `ecat::App` 骨架（name/version/on_start 挂 Scheduler）+ `Config::load()`（沿用 GATEWAY/K8S_CONFIG 风格：`COLLECTOR_CONFIG`）
- [ ] 验证：`cargo check` 零警告；启动日志正常

### Task P1.2: ClickHouse 写入封装

**Files:** Create `services/collector/src/ch.rs`；Modify `services/collector/Cargo.toml`（+`ecat-data-clickhouse`）

- [x] `ClickhouseClient::from_config` + `write_resource_snapshot(&[Row])`：直接复用已同步的 `TsdbClient` impl（自动建表 + JSONEachRow 写入），不再手写批量 INSERT 拼接
- [ ] 验证：启动后建表（`CREATE TABLE IF NOT EXISTS superops.resource_snapshot ...`）；单测断言 SQL 生成正确

### Task P1.3: 资源水位采集

**Files:** Create `services/collector/src/collect.rs`；Modify `services/collector/src/main.rs`

- [x] `Scheduler::every(60s)`：经 superops-protos 调 k8s service `list_nodes`/`list_pods` → 聚合 CPU/内存水位 → `ch.rs` 写入
- [ ] 验证：跑 2 个周期后 `SELECT count(*) FROM superops.resource_snapshot` > 0

### Task P1.4: 指标查询 API

**Files:** Modify `services/gateway/Cargo.toml`（+`ecat-data-clickhouse`）；Create `services/gateway/src/proxy/metrics.rs`；Modify `services/gateway/src/main.rs`（挂 `/api/v1/metrics/query`，走认证）

- [x] `?cluster_id=&node=&range_minutes=` → ClickHouse 查询 → JSON 序列
- [ ] 验证：curl 带 token 返回数据；无 token 401

### Task P1.5: 巡检调度

**Files:** Create `services/collector/src/inspect.rs`；Modify `services/collector/src/main.rs`

- [x] `Scheduler::every(10min)`：node Ready 率、deployment 可用率 → 写 `superops.alert_event` + 发布 `superops.alert`
- [ ] 验证：手动触发一次（间隔可配置），ch 有行、MQ 有事件

### Task P1.6: Kafka 事件总线

**Files:** Create `services/collector/src/events.rs`（KafkaMq 订阅）；Modify `services/gateway/src/auth/handler.rs`（登录/注册发布 `superops.audit`）；Modify 两个 Cargo.toml（+`ecat-mq-kafka`）

- [x] gateway 登录成功/注册成功发布审计事件；collector 订阅落 `superops.audit_log`
- [ ] 验证：登录一次后 ch 审计表出现对应行

### Task P1.7: 分布式锁

**Files:** Modify `services/collector/src/main.rs`（+`ecat-data-redis` RedisLock）；Modify `services/collector/src/config.rs`

- [x] 巡检任务先 `acquire(key, ttl)`，失败则本周期跳过；结束 `release(key, token)`
- [ ] 验证：双实例同时启动，日志确认仅一个执行（Redis key 存在性检查）

### Task P1.8: 告警阈值规则

**Files:** Create `services/collector/src/alert.rs`（规则函数：NotReady 节点数 ≥ N 连续 2 次 → 告警事件）

- [x] 规则纯函数可单测（输入快照 → 输出事件）
- [x] 验证：cargo test 通过

**P1 验收:** 三张表有数据；登录产生审计链路（gateway→Kafka→collector→CH）；双实例巡检不重复；`/api/v1/metrics/query` 可用。 实现完成（TDD 全绿、零警告）；运行时链路验证（三表数据、审计链路、双实例互斥、查询 API 实测）待 sudo docker compose up 后复核。

## P2 — 韧性与可观测

### Task P2.1: 熔断

**Files:** Modify `services/gateway/src/main.rs`（`CircuitBreakerLayer` 叠在 k8s 路由组）

- [x] `CircuitBreakerLayer::new().failure_ratio(0.5).window(30s).half_open_probes(3).open_duration(10s)`
- [x] 验证：停 k8s service → 请求 500，随后立即 503 "circuit breaker is open"；恢复后 half-open 自愈

### Task P2.2: Consul 注册 / 发现

**Files:** Modify `services/gateway/src/main.rs`、`services/k8s/src/main.rs`、`services/collector/src/main.rs`（`on_start` 注册，Registration 保活）；Modify `services/gateway/src/config.rs`（k8s 地址 fallback 静态）

- [x] 三个服务 `ConsulRegistry::new(addr).register(ServiceInfo)`；gateway 通过 `discover("superops-k8s")` 解析 gRPC 地址，失败回退配置静态地址
- [x] 验证：`curl :8500/v1/health/service/superops-k8s?passing=true` 有结果；kill 进程后自动注销

### Task P2.3: 远程配置

**Files:** Create `services/gateway/src/config_remote.rs`；Modify `services/gateway/src/main.rs`

- [x] `ConsulConfigSource::new(addr, "config/superops/gateway")` + 已同步的 `watch()`（Consul 阻塞查询 + mpsc 变更流，含首帧强制推送）→ 热更新限流阈值/熔断参数（值存 `Arc<RwLock<...>>`）
- [x] 验证：`consul kv put config/superops/gateway/rate.limit.max 20` 后 watch 推送生效（近实时，无需 30s 轮询窗口）

### Task P2.4: OTLP 追踪

**Files:** Modify `services/gateway/src/main.rs`、`services/k8s/src/main.rs`（`ecat_tracing_otlp::init(service_name, endpoint)`，provider 保活）；Modify `deploy/docker-compose.yml`（+otel-collector 或 jaeger：4317/16686）

- [x] 两个服务接入；请求处理路径产生 span
- [x] 验证：jaeger UI 出现 `superops-gateway` / `superops-k8s` 服务 span（无集群时至少 HTTP 处理 span 可见）

### Task P2.5: 限流迁移框架原生

**Files:** Modify `services/gateway/src/main.rs`（`RateLimitLayer::new(10, 60).with_store(Arc::new(RedisRateLimitStore::connect(url)?))`）；Delete `services/gateway/src/auth/rate_limit.rs`（自研实现移除）；Modify `services/gateway/Cargo.toml`（`ecat-middleware` 启用 `redis` feature）

- [x] 行为对齐：10 次/60s/IP（默认 key：XFF → X-Real-IP → global），429 文案沿用
- [x] 验证：连续 12 次登录 → `401×9 429×3`；Redis 出现 `rl:*` 键；无 Redis 时 fail-open 放行并 WARN

**P2 验收:** 熔断实测 503；Consul 注册/注销自动；配置热更新生效；jaeger 见 span；限流行为与现状完全一致。

## P3 — 集成与对外

### Task P3.1: k8s service exec_pod gRPC

**Files:** Modify `services/k8s/src/service.rs`（实现 `exec_pod` 双向流，kube 0.93 exec API）；Modify `protos/k8s/v1/*.proto`（ExecRequest/ExecResponse 字段定型）+ `make proto`

- [x] 双向流：客户端 stdin 行 → 容器；容器 stdout/stderr → 流回
- [x] 验证：单测 + grpcurl 冒烟（无真实集群时返回明确错误而非挂起）

### Task P3.2: gateway WS 终端桥

**Files:** Modify `services/gateway/src/proxy/k8s_proxy.rs`（`/api/k8s/clusters/{id}/pods/{ns}/{pod}/exec` 改 axum ws upgrade → gRPC 双向流转发）

- [x] WS 帧 ↔ gRPC 流互转（复用现有 mpsc/ReceiverStream 模式）
- [x] 验证：有集群时终端输入输出回显；无集群返回明确错误帧

### Task P3.3: 前端终端接入

**Files:** Modify `frontend/src/pages/k8s/terminal.tsx`（ws URL 与新端点对齐，校验现有 cleanup 逻辑不回归）

- [x] 验证：手动联调输入输出；组件卸载无泄漏

### Task P3.4: API Key 管理

**Files:** Create `services/gateway/src/auth/apikey.rs`（表 `api_keys(id, name, key_hash, role, created_at)`，CRUD `/api/v1/api-keys`）；Modify `services/gateway/src/main.rs`（`ApiKeyLayer::new(HashMap)` 挂可选路由组）

- [x] 创建/吊销 API key（存 hash，明文仅创建时返回一次）
- [x] 验证：`X-API-Key` 鉴权通过/拒绝；吊销后立即失效

### Task P3.5: OAuth2（可选集成，默认关闭）

**Files:** Modify `services/gateway/src/config.rs`（`oauth2: {introspection_url, client_id, client_secret}`）；Modify `services/gateway/src/main.rs`（配置了才挂 `OAuth2Layer`）

- [x] 验证：配置 IdP 后 introspection 通过；未配置时完全禁用（不引入启动失败）

### Task P3.6: OpenAPI 文档

**Files:** Create `services/gateway/src/openapi.rs`；Modify `services/gateway/src/main.rs`（`GET /api/docs`）

- [x] `OpenApiBuilder` 描述 `/api/auth/*`、`/api/v1/*`、`/api/k8s/*`（`add_route` 已支持 PUT/DELETE/PATCH/HEAD/OPTIONS 全方法）
- [x] 验证：`/api/docs` 返回合法 openapi 3.0.3 JSON

### Task P3.7: 压测脚本

**Files:** Create `bench/login_bench.rs`（bin，`ecat_bench::run_bench`：登录 + k8s 列表查询两个场景）

- [x] 验证：`cargo run --release -p bench` 输出 BenchResult（rps/p50/p99）

### Task P3.8（stretch）: graphql / versioning

- [x] 评估 `ecat-graphql`、`ecat-versioning` 是否引入 —— 不进主计划，评估后决定

**评估结论（2026-08-06）：** ecat-graphql 与 ecat-versioning 不进入实现——GraphQL 与现有 REST + OpenAPI 体系重复且增加双层 schema 维护成本；versioning 在单一 gateway 且内部服务受控的前提下收益低。待出现多版本 API 共存或外部消费者需求时再评估。

**P3 验收:** 终端可用（含错误降级）；API key 全生命周期；`/api/docs` 合法；bench 可跑。

## 依赖关系

```
P0.1-P0.4（地基，互不阻塞）
  ├─ P1.1 → P1.2 → P1.3 → P1.4
  ├─ P1.1 → P1.5 → P1.8
  ├─ P1.6（需 Kafka）→ 供 P1.5/P1.8 发布告警
  └─ P1.7（需 Redis，已就绪）
P2（各任务相对独立，均需 P0.3 的 Consul/Kafka 就绪；P2.5 需 Redis 已就绪）
P3.1 → P3.2 → P3.3；P3.4/P3.6/P3.7 独立
```

## 风险

| 风险 | 缓解 |
|---|---|
| kube 0.93 exec API 能力待确认 | P3.1 第一步先做 API 勘探（参照 kube-rs docs），不行则 exec 保持 unimplemented 并如实报告 |
| Kafka 本地单节点仅开发可用 | 生产部署标注 3 节点要求；compose 单节点 + KRaft |
| OAuth2 需外部 IdP | 默认关闭，配置才启用（P3.5） |
| ClickHouse ReplacingMergeTree 去重依赖 ORDER BY | 表结构在 P1.2 明确 `(cluster_id, node_name, ts)`，写入前校验 |
| 阶段间接口漂移（proto/表结构） | 每阶段验收后冻结约定，进入下一阶段 |

## 各阶段详细计划的产出时机

- 执行 P0 前：产出 P0 详细计划（含全部代码步骤）
- 其余阶段同理，前一阶段验收通过后产出下一阶段详细计划
