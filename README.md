# SuperOps — 智能运维平台

基于 [e-cat](https://github.com) 框架构建的智能运维平台（Phase 1 MVP）：API 网关（认证/限流/代理）、Kubernetes 资源服务（Pod/Deployment/Node 查询、日志、Watch）、Tauri 桌面前端。

## 架构

```
┌────────────┐   HTTP  ┌─────────────┐   gRPC   ┌────────────┐
│  Frontend  │ ──────► │   Gateway   │ ───────► │  K8s 服务   │ ──► Kubernetes API
│ Tauri/Web  │  :8080  │  auth+limit │  :9090   │   :9091    │
└────────────┘         └──────┬──────┘          └────────────┘
                              │ MySQL（用户） / Redis / ClickHouse（docker）
```

- **gateway**（`services/gateway`）：JWT 认证中间件、登录速率限制（10 次/60s/IP）、CORS 白名单、`/api/k8s/*` 路由代理
- **k8s service**（`services/k8s`）：gRPC 服务，集群管理（kubeconfig）、Pod/Deployment/Node 查询、Pod 日志流、资源 Watch
- **frontend**（`frontend`）：React + Vite + Tauri 2 桌面壳，zustand 状态（token 仅存内存）

## 快速开始

```bash
# 1. 基础设施（MySQL 3307 / Redis 6380 / ClickHouse 8124）
cd deploy && cp .env.example .env && docker compose up -d

# 2. 生成 protobuf（首次或 proto 变更后）
make proto

# 3. 后端（两个服务）
make dev          # cargo build gateway + k8s-service
cd services/gateway && GATEWAY_CONFIG=../../config/gateway.yaml cargo run &
cd services/k8s && K8S_CONFIG=../../config/k8s-service.yaml cargo run &

# 4. 前端
cd frontend && npm install && npm run dev    # http://localhost:3000
```

或一键：`make dev-all`（compose + 两个后端 + 前端）。

测试账号（本地开发库）：`erik / Test1234!`

## 端口

| 组件 | 端口 |
|---|---|
| gateway HTTP | 8080 |
| gateway gRPC | 9090 |
| k8s service gRPC | 9091 |
| frontend (vite) | 3000 |
| MySQL | 3307 |
| Redis | 6380 |
| ClickHouse | 8124 (HTTP) / 9001 (native) |

## 配置

| 环境变量 | 用途 |
|---|---|
| `GATEWAY_CONFIG` | gateway 配置 YAML 路径（默认 `config/gateway.yaml`） |
| `K8S_CONFIG` | k8s 服务配置 YAML 路径（默认 `config/k8s-service.yaml`） |
| `SUPEROPS_JWT_SECRET` | JWT 签名密钥（生产必设，≥32 字节随机值；缺省启动有 WARN 并沿用默认值） |

数据库/Redis/ClickHouse 密码通过 `deploy/.env` 注入 compose（模板见 `deploy/.env.example`）。

## 常用命令

```bash
make proto        # buf generate（protos/ → superops-protos/）
make gateway      # 构建 gateway
make k8s-service  # 构建 k8s 服务
make dev          # proto + 构建两个后端
make dev-all      # compose + 两个后端 + 前端
make stop-all     # 停 compose + 两个后端进程
make clean        # cargo clean + 前端产物清理
```

## 测试与 CI

- 端到端：登录/注册 + 全部 `/api/k8s/*` 路由正常与错误路径（见 `docs/audit-report-2026-08-06.md`）
- 安全：认证绕过、JWT 伪造/篡改、输入校验、CORS、速率限制（15/15 项验证通过）
- CI（`.github/workflows/ci.yml`）：`cargo fmt --check` + `cargo check` + `cargo test`（gateway/k8s）+ `npm run build`（tsc + vite）

## 文档

- `docs/audit-report-2026-08-06.md` — 全量审查报告（测试矩阵、安全清单、修复记录）
- `docs/superpowers/plans/2026-08-06-super-ops-phase1-mvp.md` — Phase 1 实施计划（44 步）
- `CHANGELOG.md` — 变更日志

## 许可与说明

Phase 1 MVP：单实例部署，内存限流；Phase 2 规划：gRPC 数据通道（真实 k8s 查询代理）、终端 WebSocket 桥接、Redis 共享限流、httpOnly cookie 会话。
