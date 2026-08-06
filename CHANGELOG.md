# Changelog

## [1.2.0] — 2026-08-06 — SuperOps P3 集成与对外

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

## [1.0.5] — 2026-08-06 — SuperOps P1 数据与监控闭环

### Added
- 新增 `superops-collector` 服务（ecat-scheduler 调度）：每 60s 采集 k8s 节点/Pod/Deployment 快照，经 `ecat-data` TsdbClient 写入 ClickHouse `resource_snapshot`（summary + 逐资源点）
- 巡检告警（每 600s）：node 非 Ready → CRIT、Pod 非 Running/Succeeded → WARN、Deployment ready < replicas → WARN，写入 `alert_event`；`ecat-lock` RedisLock 互斥保证多实例下仅单实例巡检
- Kafka 审计事件闭环：gateway login/register 发布 `superops.audit`（ecat-mq-kafka），collector 消费写入 `audit_log`
- gateway 新增 `GET /api/v1/metrics/query`：按 series 分组、`argMax(field, timestamp)` 取最新值，返回 JSON 序列

### Updated
- gateway/collector 接线 e-cat 框架 API：`TsdbClient::write/query` 全限定调用、`MessageStream::poll_recv` 经 `futures::future::poll_fn` 消费、`DataPoint` 消费式 builder、生命周期钩子闭包模式

## [1.0.4] — 2026-08-06 — SuperOps P0 可观测地基

### Added
- gateway 接入 `ecat-health`：`/health`（liveness）与 `/ready`（readiness，含 MySQL 依赖检查，失败 503）
- gateway 接入 `ecat-metrics`：`/metrics` Prometheus 文本导出 + `superops_http_requests_total` 请求计数
- docker-compose 补齐 Prometheus（9095）、Kafka KRaft 单节点（9092）、Consul dev 模式（8500）
- `deploy/prometheus.yml` scrape 配置（gateway:8080/metrics）；`.env.example` 追加 KAFKA_BROKERS/CONSUL_ADDR/OTLP_ENDPOINT/CH_URL/PROMETHEUS_PORT

### Fixed
- `ecat-health::HealthRegistry::with_check` 由 `blocking_write` 改为异步写锁（`pub async fn`），修复在 tokio runtime 内注册检查必然 panic 的问题

### Updated
- 同步 e-cat 上游最新代码（HEAD 2cc0fb0）：`ecat-metrics` 新增 `metrics_router()`、`ecat-transport-http` 自动挂载 `/metrics`（框架保留路径）、`ecat-config-remote` 新增 `watch()`、`ecat-data-clickhouse` 新增 `TsdbClient` 实现、`ecat-openapi` 支持 PUT/DELETE/PATCH/HEAD/OPTIONS；gateway 移除手动 `/metrics` 路由改用框架自动挂载

## [1.0.3] — 2026-08-06 — SuperOps services

### Security
- 认证绕过（CRITICAL）修复：gateway `auth_middleware` 重写为真实 JWT 验签（`verify_token` + Claims 注入 extensions），并挂载到全部 `/api/k8s/*` 路由；此前仅检查 `Bearer ` 前缀且未挂载
- login 增加速率限制：内存固定窗口 10 次/60 秒/IP（x-forwarded-for 取值），超限 429；Redis 共享留待 Phase 2 多实例
- `SUPEROPS_JWT_SECRET` 环境变量覆盖默认密钥，生产默认密钥启动输出 WARN
- 注册/登录输入校验（username 3-32、email 5-254、password 8-72），handler 移除全部 `.unwrap()`
- CORS 白名单化（localhost:3000 / tauri.localhost / tauri://localhost），方法/头收紧
- Tauri 启用 CSP：生产 `connect-src` 仅 gateway:8080；devCsp 放开 vite HMR
- 前端 token 移除 zustand persist（仅存内存，不再落 localStorage）

### Fixed
- k8s watcher：首次观察事件区分 ADDED/MODIFIED（seen-set），不再全部标 MODIFIED
- k8s pod log/watch stream：`tx.send` 失败后 break，客户端断开不再持续读
- 前端 terminal.tsx：useEffect cleanup（WS/terminal dispose、onResize listener 移除）
- 前端类型对齐：addCluster 响应 `{id,name,status}`、cluster-detail 真实 clusterId 传 props
- 两个服务 crate `cargo check` 零警告（Phase 2 预留字段 `#[allow(dead_code)]` 并注明用途）

### Infrastructure
- 补齐 CI：`.github/workflows/ci.yml`（rust fmt --check + check + test；frontend tsc + vite build），移除 `.gitignore` 对 `.github` 的忽略
- docker-compose：ClickHouse healthcheck（clickhouse-client SELECT 1）、全部密码变量化；新增 `deploy/.env.example` 模板
- 文档：README.md / README.en.md 补齐；审查报告 `docs/audit-report-2026-08-06.md` 更新至本轮修复状态

## [2.3.1] — 2026-08-06

### Fixed
- 端口绑定规范化：`HttpServer` 空 host 统一为 `0.0.0.0`，示例/文档/CLI 模板的监听地址从 `:8000` 改为 `0.0.0.0:8000`（修复无 IPv6 环境启动失败）
- 全部 HTTP 数据库适配器（ES/OpenSearch/ClickHouse/InfluxDB/IoTDB/QuestDB/TDengine/Neo4j/NebulaGraph/ArangoDB）与 TLS 客户端统一设置 connect/timeout，修复请求永久悬挂
- `ecat-data-memcached` 标记为内存实现并明确文档警告，禁止生产误用（静默数据丢失风险）
- TDengine 写入 SQL 拼接转义标识符与字符串值（`"`/`\`），修复注入逃逸
- 限流修复：`key_fn` 支持按请求取客户端 key；Redis 限流区分存储错误（fail-open）；内存桶定期清理防止无界增长
- JWT 最小密钥长度校验（≥32 字节随机密钥）与错误泛化；OAuth2 客户端复用、设置超时并强制 HTTPS
- Redis 凭据改为 `ConnectionInfo` 单独传参，错误消息不再泄露口令；锁 TTL 溢出统一钳制
- Elasticsearch `search`/`delete` 补充 HTTP 状态码检查；index/id 路径 URL 编码（IDOR）
- etcd deregister 修正为按完整注册键删除，修复实例退出后注册信息残留
- GitHub Actions CI 增加 `protobuf-compiler` 安装，与 GitLab CI 对齐（修复 protoc 缺失必然失败）
- Dockerfile 修复：拷贝实际 `ecat` 二进制（原 `ecat-app` 不存在）、安装 curl 以支持 HEALTHCHECK、builder 镜像升至 1.85（edition 2024）
- 其他：Helm appVersion 更新为 2.3.0；配置示例默认口令全部注释化；consul 注册端口从端点解析、discover 版本不再硬编码；MQ `from_config` 签名统一为 async；11 处 Cargo.toml 依赖收敛至 `workspace.dependencies`；`ecat new` 增加 crate 名校验（防路径穿越与注入）；README.en.md 同步至 v2.3.0

## [2.3.0] — 2026-08-06

### Added
- `ecat-mq-kafka` 真 Kafka 实现（rdkafka，替换内存存根）
- 消息后端：`ecat-mq-rabbitmq`（lapin）、`ecat-mq-mqtt`（rumqttc）、`ecat-mq-nats`（async-nats）
- 数据后端：`ecat-data-mongodb`（DocumentClient）、`ecat-data-s3`（StorageClient，rust-s3）、`ecat-data-tdengine`（REST 时序）
- `ecat-lock` 分布式锁 trait + `ecat-data-redis` 的 `RedisLock`（SET NX PX + token 校验）
- `ecat-scheduler` tokio 定时任务调度（every / once）
- `ecat-tracing-otlp` OpenTelemetry OTLP/gRPC 追踪导出
- `ecat-data` trait 扩展：`DocumentClient`、`StorageClient`；`Cache::increment/ttl/multi_get`、`SearchClient::bulk_index/update`、`TsdbClient::delete` 加法默认方法
- `ecat-middleware` 限流后端抽象（`RateLimitStore`）+ `RedisRateLimitStore`（可选 feature）
- CLI：`--version`、`upgrade`（批量更新 ecat-* 依赖）、`run --watch`（notify 文件监听 + 500ms 防抖重启）
- `.gitlab-ci.yml`（镜像 GitHub Actions CI）

### Changed
- Workspace 扩展至 55 crates
- 数据库后端增至 18 个（+MongoDB、S3、TDengine）

## [2.1.8] — 2026-08-01

### Added
- Per-crate `license.workspace` and `description` metadata for crates.io publishing
- Workspace `repository` and `documentation` URLs
- `.gitignore` for Rust project conventions

### Changed
- `EncryptedSource` → `ObfuscatedSource` (honest naming: XOR is obfuscation, not encryption)
- Config prefix `enc:` → `obfs:`
- All `from_config()` methods return `Result` instead of panicking on TLS errors
- `RdbmsError` gains `Config` variant
- `execute_with`/`query_with` default impls return error instead of silently dropping params
- QuestDB client: GET → POST for SQL execution
- Redis TTL: `set_ex` → `pset_ex` for sub-second precision
- `ecat-data-memcached`: `std::sync::Mutex` → `tokio::sync::Mutex`
- `ecat-registry-etcd`: hand-rolled base64 → `base64` crate
- `ecat-client`: `RandomBalancer` uses `RandomState` instead of `Instant::now()` hash
- `ecat-client`: `StaticResolver::add_service` uses `blocking_write` instead of `try_write`

### Fixed
- `ecat-versioning` header-based routing now actually validates version headers
- Credential URL encoding in `connect_with_auth` methods
- Missing `json` feature for reqwest in `ecat-data-influxdb` and `ecat-data-clickhouse`
- Content-Type headers on HTTP requests (InfluxDB, ClickHouse, IoTDB)
- Removed `#[allow(dead_code)]` annotations via field renaming

### Split
- `ecat-auth` (540 lines) → `claims.rs` + `jwt.rs` + `apikey.rs` + `oauth2.rs` + `helpers.rs` + `lib.rs`

## [2.1.7] — 2026-07-29

### Added
- 11 new database backends: ArangoDB, ClickHouse, Elasticsearch, InfluxDB, IoTDB,
  Memcached, NebulaGraph, Neo4j, OpenSearch, QuestDB, Redis
- `ecat-tls` crate for shared TLS configuration
- `ecat-transport-ws` WebSocket server
- `ecat-versioning` API version routing
- `ecat-deploy` Docker/K8s/Helm deployment templates
- `ecat-registry-etcd` backend
- `ecat-mq-kafka` backend

### Changed
- All data backend configs include optional TLS fields
- `ecat-data` trait system: RdbmsClient, Cache, GraphClient, SearchClient, TsdbClient
