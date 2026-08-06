# Changelog

## [1.5.0] — 2026-08-06 — SuperOps P6 治理与运维闭环

### Added
- 审批工作流：`POST /api/approvals`（kind=delete，target 约定 `"{cluster_id}/{ns}/{name}"`，避免跨集群同名绕过）、`GET /api/approvals`、`POST /api/approvals/{id}/decide`（pending→approved/rejected/canceled）；`delete_deployment` 门禁（`services/gateway/src/proxy/k8s_proxy.rs`，`approval.enabled` 时未审批删除返回 412）；config `approval:` 段默认关闭
- 多租户：`x-tenant-id` 请求头 + `require_tenant` 中间件（仅允许小写字母/数字/连字符 1..=64，非法/缺失回落 `default`，无失败路径）；cmdb/scripts 查询按 `tenant_id` 过滤
- 凭据保险库：`/api/secrets`（GET 列表 / POST 创建 / GET+DELETE 单条，AES-256-GCM 加密）；`SUPEROPS_MASTER_KEY` 未配置或非 32 字节时全部接口 503（两态可用），密文值不入日志
- 治理任务（collector `housekeeping.rs`）：MySQL 备份（mysqldump → `data/backups`，保留 BACKUP_KEEP 份）、磁盘容量与成本估算上报；collector.yaml 新增 `housekeeping:` 配置段
- 终端录制旁路（gateway `recorder.rs`）：exec WebSocket 帧异步写入 ClickHouse `exec_session`（kind=recording，busy/超时不阻塞主链路）；`/api/recordings`（GET 列表 / GET `{sid}/frames` / DELETE `{sid}`），read 路由 api:read、write 路由 api:write
- 文件服务：`POST /api/files`（multipart，`DefaultBodyLimit` 10MB，文件名白名单校验）与 `GET /api/files/{name}`，MinIO 存储（bucket 自动创建）
- 告警中心：`GET /api/alerts?level=&limit=50`（ClickHouse `alert_event`）、`POST /api/alerts/{id}/ack`、`GET /api/alerts/acks`；前端 `/ops/alerts` 页（10s 轮询 + 确认），dashboard 活跃告警卡片改接告警中心接口
- 指标看板：前端 `/ops/metrics` 页复用 `GET /api/v1/metrics/query` 展示节点/Pod/Deployment 指标
- OTLP 追踪扩展（P6-6）：collector 调度与 k8s gRPC 服务接入 OTLP span（ecat-tracing-otlp），config 改用扁平字符串 `otlp: "http://localhost:4317"`
- Helm chart：`deploy/helm/superops/`（Chart.yaml / values.yaml / configmap + gateway/k8s/collector 三 Deployment + Service + NOTES，共 8 文件，仅提供未打包）
- MinIO（compose）：`minio` 服务 9002(API)/9003(console)，`minio_data` volume；`deploy/.env.example` 追加 `MINIO_ROOT_USER`/`MINIO_ROOT_PASSWORD`（生产必改）

### Changed
- 端口表/结构文档同步：gateway gRPC 9090、MinIO 9002/9003、`deploy/helm/` 与 `deploy/prometheus.yml`
- 已知边界更新：主密钥（`SUPEROPS_MASTER_KEY`）与审批开关（`approval.enabled`）默认关闭

## [1.4.0] — 2026-08-06 — SuperOps P5 日志/CMDB/脚本库/RBAC

### Added
- 日志采集（collector `logtail.rs`）：周期采集 Pod 日志 → ClickHouse（按命名空间轮询，tail_lines / max_line_bytes 可配）
- 日志检索 API：`GET /api/logs/search`（ClickHouse 查询，require_role("api:read")）
- CMDB 资产：MySQL `cmdb_asset` 表 + `/api/cmdb/assets`（GET 列表 / POST 创建）、`/api/cmdb/assets/{id}`（DELETE）、`/api/cmdb/stats`（GET），require_role("ops:cmdb")；前端新增 `/cmdb` 页（ProTable + 新建弹窗，菜单位于 Kubernetes 与运维中心之间）
- dashboard 真实数据：集群数 / Docker 主机数（metrics query，临时指标 app）/ 活跃告警（审计事件前 100 条计数，P6 替换为告警中心）三卡片接入真实接口
- 脚本库：MySQL `script`/`script_run` 表 + `/api/scripts`（GET/POST）、`/api/scripts/{id}`（DELETE）、`/api/scripts/{id}/run`（POST）、`/api/scripts/runs`（GET），require_role("ops:scripts")，仅支持 shell（busybox:1.36）
- k8s 批量执行：service 新增 gRPC `RunJob`（`services/k8s/src/resource/job.rs`，batch/v1 Job 创建）
- 前端脚本库页：`/ops/scripts`（脚本列表 + 运行记录双 ProTable）

### Changed
- RBAC 角色体系：admin / operator / viewer（首个注册用户自动 admin，其余默认 viewer）；权限映射 admin 全量、operator 含 api:read/api:write/ops:audit/ops:cmdb/ops:scripts、viewer 仅 api:read；API Key 鉴权按用户角色走同一映射（查找失败回退拒绝）；exec 终端路由移至写路由（require_role("api:write")，需 operator+）

## [1.3.0] — 2026-08-06 — SuperOps P4 写操作与运营闭环

### Added
- k8s 写操作闭环：gateway 新增 `POST /api/k8s/clusters/{cluster_id}/deployments/{namespace}/{name}/scale|restart` 与 `DELETE /api/k8s/clusters/{cluster_id}/deployments/{namespace}/{name}` 路由，转发 k8s gRPC `ScaleDeployment`/`RestartDeployment`/`DeleteDeployment` RPC
- 审计事件随写操作发布：`k8s.scale`/`k8s.restart`/`k8s.delete` 动作写入 Kafka `superops.audit`（payload 含 `level: "INFO"`）
- 告警通知（collector `notify.rs`）：generic/钉钉/企微 webhook 通知目标（`NotifyTarget`），按目标静默窗口（`NotifySilencer`，默认 300s），collector.yaml 新增 `notify:` 配置段
- 审计中心 API：`GET /api/audit/events?limit&offset&level`（ClickHouse `audit_log`，鉴权保护）
- 用户管理 API：`GET /api/users`、`PATCH /api/users/{id}/status`（启用/禁用；users 表新增 role/status 列）
- 前端运维中心：`/ops/audit`、`/ops/apikeys`、`/ops/users` 页面（运维中心菜单组）

## [1.2.2] — 2026-08-06 — 认证迁移至 e-cat 框架

### Changed
- gateway JWT 签发/校验迁移到框架 ecat-auth：新增框架级 `sign_token`/`verify_token` API（HS256、基于 `AuthClaims`、密钥 ≥32 字节强制校验），gateway 删除自研 `Claims` 结构与 jsonwebtoken/chrono 直接依赖；`JwtAuthService` 内部复用 `verify_token`
- 凭证链改用框架 helpers（`extract_bearer`/`extract_query_param`），X-API-Key 与 OAuth2 统一注入框架 `AuthClaims` 类型（此前 OAuth2 短路用 `AuthClaims`、JWT/API Key 用自研 `Claims`，双类型并存）
- 开发默认 JWT 密钥提升至 ≥32 字节（`change-me-in-production-0123456789abcdef`），适配框架 WeakKey 校验；README/架构图同步（认证链路已全部基于 ecat-auth）

### Fixed
- 迁移后旧默认密钥（24 字节）触发框架 `JwtAuthError::WeakKey` 拒绝签发 → 登录 500；已换用合规开发密钥并更新 config.rs 哨兵检查

### Verified
- workspace 296 项测试全过（ecat-auth +6 项：签发/校验 roundtrip、extra claims、过期、篡改、弱密钥、错密钥）
- 运行时回归：login → JWT 200、伪造/缺失 token 401、API Key 创建/使用/删除/吊销 200/204/401、query token 回退 200

## [1.2.1] — 2026-08-06 — 修复与文档

### Fixed
- gateway 挂载 HTTP 追踪层（TraceLayer，INFO 级 span）：此前 tower-http 已启用 `trace` 特性但从未使用，OTLP 链路无请求级 span 源，Jaeger 收不到 gateway 追踪
- `ecat-tracing-otlp` EnvFilter 对齐 `ecat-logging`：无 `RUST_LOG` 时回退 `info`（此前空 filter 过滤全部事件与 span，含 HTTP span）
- `deploy/init.sql` 幂等：`CREATE INDEX`（MySQL 8 无 `IF NOT EXISTS`）改为内嵌于 `CREATE TABLE IF NOT EXISTS` 的 KEY 定义，重跑不再中断

### Changed
- README.md / README.en.md 重写：项目说明、技术架构（含 `docs/images/architecture.svg` 架构图）、项目结构、功能说明、端口/配置/测试/已知边界
- 运行时验证补完：Consul KV 热更新双向生效（阈值 3↔10）、Jaeger span 可见、Prometheus target up

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
