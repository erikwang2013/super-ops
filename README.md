# SuperOps — 智能运维平台

基于 [e-cat](https://github.com/erik/e-cat) 框架生态构建的智能运维平台（v1.2.0）。API 网关统一承载认证、限流、熔断与反向代理；Kubernetes 资源服务提供查询、日志、Watch 与终端 exec；Collector 负责指标采集、巡检与告警；Tauri 桌面前端完成可视化操作。

## 项目说明

SuperOps 面向中小规模基础设施运维，提供一条从「看见」到「处置」的闭环链路：

- **看见**：集群资源（Pod/Deployment/Node）、Pod 日志、实时终端、指标快照与告警
- **韧性**：网关限流（Redis 共享计数 + Consul 动态阈值热更新）、熔断（5xx 连续失败自动半开）、认证（JWT / OAuth2 / API Key 三通道）
- **可观测**：Prometheus 指标抓取、OTLP 链路追踪（Jaeger）、ClickHouse 时序落库
- **协同**：Consul 注册发现 + 远程配置、Kafka 审计事件总线

四阶段演进：P1 MVP（认证 + 资源查询）→ P2 韧性与可观测（熔断/限流/注册/追踪）→ P3 集成与对外（exec 终端、API Key、OAuth2、OpenAPI）→ P0 基础设施补齐（Compose 全家桶）。

## 技术架构

```
┌────────────┐   HTTP/WS  ┌─────────────┐   gRPC   ┌────────────┐
│  Frontend  │ ─────────► │   Gateway   │ ───────► │  K8s 服务   │ ──► Kubernetes API
│ Tauri/Web  │   :8080    │ auth+limit  │  :9091   │   :9091    │
└────────────┘            └──────┬──────┘          └────────────┘
                                 │ gRPC
                          ┌──────▼──────┐
                          │  Collector  │────► ClickHouse（指标快照/告警）
                          └──────┬──────┘
                                 │
     MySQL（用户/Key） Redis（限流/锁） Kafka（审计） Consul（注册/KV） Prometheus/Jaeger
```

![架构总览](docs/images/architecture.svg)

**请求路径**：浏览器 → Gateway（认证 → 限流 → 熔断 → 代理）→ K8s Service（gRPC）→ Kubernetes API；终端场景经 WebSocket 桥接为 gRPC 双向流。**配置路径**：Consul KV `config/superops/gateway/*` 变更经阻塞查询实时推送，`rate.limit.max`/`rate.limit.window` 即时生效。**可观测路径**：Gateway 导出 `/metrics`（Prometheus 抓取）与 OTLP span（Jaeger 展示）。

### 架构图集

![请求处理流程](docs/images/flow.svg)

![分层设计](docs/images/design.svg)

![功能结构](docs/images/structure.svg)

![安全防护](docs/images/security.svg)

![服务生命周期](docs/images/lifecycle.svg)

## 项目结构

```
super-ops/
├── services/                  # 后端服务（每个独立二进制）
│   ├── gateway/               #  API 网关 :8080
│   │   └── src/
│   │       ├── auth/          #    JWT 签发/校验（ecat-auth）、API Key、OAuth2 短路、登录注册
│   │       ├── proxy/         #    k8s 反向代理（HTTP 查询 + WS 终端桥）
│   │       ├── breaker.rs     #    熔断（FiveXx→Error 转换链）
│   │       ├── config_remote.rs #  Consul KV 热更新（DynamicRateLimitStore）
│   │       ├── metrics*.rs    #    Prometheus 导出 + 指标查询 API
│   │       └── openapi.rs     #    /api/docs 文档
│   ├── k8s/                   #  K8s 资源服务 :9091（gRPC）
│   │   └── src/
│   │       ├── cluster/       #    集群管理（kubeconfig 增删查）
│   │       ├── resource/      #    pod/deploy/node/exec/metrics 查询
│   │       └── service.rs     #    tonic 服务实现（日志流/Watch/exec 双向流）
│   └── collector/             #  指标采集与巡检
│       └── src/
│           ├── collect.rs     #    周期采集 → ClickHouse 快照
│           ├── inspect.rs     #    巡检（Redis 分布式锁互斥）
│           ├── alert.rs       #    告警规则与事件写入
│           └── events.rs      #    Kafka 审计事件消费
├── frontend/                  # React + Vite + Tauri 2（:3000）
│   └── src/pages/k8s/         #  集群/Pod/Deployment/Node/终端/日志
├── ecat-*/                    # e-cat 框架组件（workspace 成员）
├── superops-protos/           # protobuf 生成代码（common.v1 / k8s.v1）
├── protos/                    # proto 源文件（buf 管理）
├── bench/                     # ecat-bench 压测入口（login）
├── config/                    # 各服务 YAML 配置
├── deploy/                    # docker-compose.yml + init.sql + prometheus.yml
└── docs/                      # 审查报告 / 实施计划 / 架构图
```

![项目结构](docs/images/tree.svg)

## 功能说明

### Gateway（:8080）
| 功能 | 说明 |
|---|---|
| 认证 | 登录/注册；JWT 签发/校验基于框架 ecat-auth（HS256、`AuthClaims` 统一注入、密钥 ≥32 字节强制校验）；可选 OAuth2 层（ecat-auth，配置 `oauth2` 段即启用）；`X-API-Key` 内存表鉴权（吊销即时生效）；WS 场景支持 query token 回退 |
| 限流 | 登录 10 次/60s/IP；Redis 共享计数（不可用自动回退内存）；阈值经 Consul KV 动态调整 |
| 熔断 | k8s 代理路由 5xx 连续失败熔断 10s（失败率 50%/窗口 30s/半开探针 3），期间 503 |
| 代理 | `/api/k8s/*` → gRPC 转发；`/exec` WebSocket ↔ gRPC 双向流（含 terminal resize） |
| 可观测 | `/health`、`/ready`（含 MySQL 依赖检查）、`/metrics`（Prometheus）、`/api/docs`（OpenAPI 3.0.3）、OTLP span |
| 运维 | Consul 注册 + `discover("superops-k8s")` 端点解析（失败回退静态配置）；KV 热更新 |

### K8s Service（:9091）
集群管理（kubeconfig）、Pod/Deployment/Node 查询（Node 含 Ready 状态与 kubelet 版本）、Pod 日志流（tail/follow）、资源 Watch（ADDED/MODIFIED/DELETED）、exec 双向流（stdin/stdout/stderr + resize）。

### Collector
按调度周期采集指标 → ClickHouse 快照；巡检任务经 Redis 锁保证单实例执行；告警规则（连续 N 次超标）写入事件；Kafka 审计事件消费（ecat-mq-kafka）。

### Frontend（:3000）
登录、集群列表/详情、Pod 列表与日志、Deployment、Node、终端页（WS 双向流，token query 鉴权、binaryType 处理、可选容器）。

### API 一览（OpenAPI 见 /api/docs）
`/api/auth/register|login`、`/api/keys`（CRUD）、`/api/k8s/clusters[/{id}][/pods|/deployments|/nodes|/metrics]`、`/api/k8s/clusters/{id}/pods/{ns}/{pod}/logs|exec`、`/api/v1/metrics/query`、`/api/health`、`/api/docs`。

## 快速开始

```bash
# 1. 基础设施（MySQL 3307 / Redis 6380 / ClickHouse 8124 / Prometheus 9095 /
#    Kafka 9092 / Consul 8500 / Jaeger 4317+16686）
cd deploy && cp .env.example .env && sudo docker compose up -d

# 2. 生成 protobuf（首次或 proto 变更后）
make proto

# 3. 后端（三个服务）
make dev          # cargo build gateway + k8s-service + collector
cd services/gateway && GATEWAY_CONFIG=../../config/gateway.yaml cargo run &
cd services/k8s && K8S_CONFIG=../../config/k8s-service.yaml cargo run &
cd services/collector && COLLECTOR_CONFIG=../../config/collector.yaml cargo run &

# 4. 前端
cd frontend && npm install && npm run dev    # http://localhost:3000
```

或一键：`make dev-all`（compose + 后端 + 前端）。

测试账号（本地开发库）：`erik / Test1234!`

## 端口

| 组件 | 端口 |
|---|---|
| gateway HTTP | 8080 |
| k8s service gRPC | 9091 |
| collector | 0（仅任务） |
| frontend (vite) | 3000 |
| MySQL | 3307 |
| Redis | 6380 |
| ClickHouse | 8124 (HTTP) / 9001 (native) |
| Prometheus | 9095 |
| Kafka | 9092 |
| Consul | 8500 |
| Jaeger | 4317 (OTLP) / 16686 (UI) |

## 配置

| 环境变量 | 用途 |
|---|---|
| `GATEWAY_CONFIG` / `K8S_CONFIG` / `COLLECTOR_CONFIG` | 各服务配置 YAML 路径 |
| `SUPEROPS_JWT_SECRET` | JWT 签名密钥（生产必设，≥32 字节；缺省有 WARN 并沿用默认值） |
| `RUST_LOG` | tracing 级别（缺省 info） |

基础设施密码通过 `deploy/.env` 注入 compose（模板见 `deploy/.env.example`）；各服务 YAML 见 `config/`。

## 测试与 CI

- workspace 单测/集成测试 296 项（gateway/k8s/collector/ecat 组件）
- 端到端与安全验证：登录/注册、k8s 路由正误路径、认证绕过、JWT 伪造、限流 429、熔断 503、API Key 生命周期（见 `docs/audit-report-2026-08-06.md`）
- 运行时验证：Consul 注册/注销、KV 热更新（阈值 3↔10 双向生效）、Jaeger span、Prometheus target up
- CI（`.github/workflows/ci.yml`）：`cargo fmt --check` + `cargo check` + `cargo test` + `npm run build`

## 已知边界

- **OAuth2**：默认关闭；开启需配置 `oauth2` 段（introspection URL + client id/secret）与外部 IdP
- **exec 终端**：需真实 k8s 集群；无集群时返回明确错误帧
- **gRPC 层 span**：目前 OTLP 追踪覆盖 HTTP 路径（gateway）；k8s/collector 的 gRPC/调度 span 未接入
- **单实例部署**：限流计数为 Redis 共享，但服务本身单实例

## 文档

- `docs/audit-report-2026-08-06.md` — 全量审查报告（测试矩阵、安全清单、修复记录）
- `docs/images/` — 架构图与图集（architecture / flow / design / structure / security / lifecycle）
- `docs/superpowers/plans/` — 各阶段实施计划（P1 MVP / P2 韧性 / P3 集成）
- `CHANGELOG.md` — 变更日志

## 支持

如果这个项目对你有帮助，欢迎扫码支持（支付宝 / 微信均可）：

| 支付宝 | 微信 |
|---|---|
| <img src="docs/alipay.png" width="130" height="130" alt="支付宝收款码"> | <img src="docs/weixinpay.png" width="130" height="130" alt="微信收款码"> |
