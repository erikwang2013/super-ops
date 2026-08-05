# 超级运维系统 — 设计文档

**日期：** 2026-08-06 | **版本：** v1.0

---

## 1. 概述

构建企业级智能运维中台 SuperOps，将 Kubernetes、Docker、GitLab 整合为统一管理平台。支持 Web 端和桌面端（Tauri），具备 LLM + ML 双通道的智能运维能力。

### 核心目标

- 统一的 K8s/Docker/GitLab 管理界面，替代 kubectl + docker CLI + GitLab Web UI 的分散操作
- AI 辅助诊断：LLM 交互式问答 + 内置模型异常检测和预测
- 一套代码多端运行：Tauri 2.x 打包 React 应用，Web + 桌面共享 100% 代码
- 企业级安全：自建账号 + SSO（OIDC/LDAP），RBAC 权限，完整审计日志

---

## 2. 总体架构

```
┌──────────────────────────────────────────────────┐
│              前端层 (Tauri + React)               │
│  K8s 面板 │ Docker 面板 │ GitLab 面板 │ AI 助手   │
│       Ant Design Pro + @antv/g6 + xterm.js       │
└────────────────────┬─────────────────────────────┘
                     │ HTTP/WebSocket
┌────────────────────┴─────────────────────────────┐
│            API Gateway (e-cat)                    │
│    Auth │ RBAC │ RateLimit │ 路由转发             │
└────┬────────┬────────┬────────┬──────────────────┘
     │ gRPC   │ gRPC   │ gRPC   │ gRPC
┌────┴──┐ ┌───┴───┐ ┌─┴────┐ ┌┴──────────┐
│  K8s  │ │Docker │ │GitLab│ │ AI Engine  │
│Service│ │Service│ │Service│ │ Service    │
└───┬───┘ └───┬───┘ └──┬────┘ └──┬─────────┘
    │         │         │         │
┌───┴─────────┴─────────┴─────────┴──────────────┐
│               基础设施                           │
│  MySQL │ Redis │ ClickHouse │ EventBus/NATS    │
└─────────────────────────────────────────────────┘
```

### 设计原则

- **领域隔离**：每个领域独立 e-cat 微服务，gRPC 通信
- **API-first**：Protobuf 定义全部接口，e-cat CLI 生成服务骨架
- **事件驱动**：集群事件、CI/CD 状态变更通过 NATS 异步流转
- **中间件复用**：所有服务共用 e-cat 内置 Recovery/Tracing/Logging/Auth/Security 中间件

---

## 3. 技术栈

| 层级 | 选型 | 说明 |
|------|------|------|
| 后端框架 | e-cat v2.1.7 (Rust) | 47 crates，对标 go-kratos |
| HTTP | axum | REST + WebSocket |
| gRPC | tonic | 服务间通信 |
| 业务 DB | MySQL 8.0 | 用户、权限、审计、配置 |
| 时序 DB | ClickHouse | 指标、事件、Pipeline 记录 |
| 缓存 | Redis | 会话、热点、分布式锁 |
| 消息 | NATS | 事件总线 |
| 前端 | React 18 + TypeScript | |
| 构建 | Vite + Tauri 2.x | Web + 桌面双端 |
| UI | Ant Design 5 + ProComponents | |
| 拓扑 | @antv/g6 | K8s 集群可视化 |
| 图表 | @antv/g2 + ECharts | |
| 终端 | xterm.js | Web Terminal |
| 编辑器 | Monaco Editor | YAML 编辑 |
| 状态 | Zustand + React Query | |

---

## 4. 服务设计

### 4.1 K8s Service

管理多 Kubernetes 集群，兼容任意 CNCF 兼容集群。

**核心功能：** 多集群连接管理（kubeconfig / SA Token / 云 SDK）、资源 CRUD、Watch 实时监听、指标采集、Web Terminal（ExecPod 双向流）、YAML 部署与 Diff 预览、HPA 管理

**gRPC API：** ListClusters / AddCluster / WatchResources(stream) / ApplyManifest / ScaleDeployment / GetPodLogs(stream) / ExecPod(stream) / GetMetrics

### 4.2 Docker Service

管理 Docker 宿主机，支持本地和远程 Docker Engine。

**核心功能：** 容器生命周期管理、镜像管理、网络/存储卷、Compose 解析、Registry 集成

**gRPC API：** ListContainers / InspectContainer / StartContainer / StopContainer / GetContainerLogs(stream) / ListImages / PullImage / WatchStats(stream)

### 4.3 GitLab Service

对接 GitLab 实例（自建 + gitlab.com）。

**核心功能：** 多实例管理、项目浏览、Pipeline 全生命周期、Job 日志流、Runner 管理、MR 审批、Registry 浏览

**gRPC API：** ListProjects / ListPipelines / TriggerPipeline / GetJobLog(stream) / ListRunners / ListMergeRequests

### 4.4 AI Engine Service

双通道智能运维引擎。

**LLM 通道（交互）：** 自然语言查询、日志异常分析、告警根因诊断、Pipeline 失败分析、YAML 辅助生成。Provider 抽象：OpenAI / Claude / Ollama / 自定义 Endpoint

**ML 通道（后台）：** IsolationForest 异常检测、Prophet/LSTM 时序预测、Pod 故障预测、资源优化建议、容量规划

**Action Engine：** 告警分级 P0-P4、自动修复（需确认）、钉钉/企微通知、自动 GitLab Issue

**gRPC API：** Chat(stream) / AnalyzeLogs / DiagnoseAlert / GetPredictions / ListAnomalies / EvaluateRule

---

## 5. 服务间交互

### 关键流程

```
AI 诊断：用户 → Gateway → AI.Chat → K8s.GetMetrics + GitLab.ListPipelines → LLM → 流式返回

部署编排：GitLab Pipeline ✅ → NATS → K8s.Service 部署 → 通知 → 审计

事件 Topic：
  k8s.cluster.<id>.alert     k8s.cluster.<id>.scale
  gitlab.project.<id>.pipeline    docker.host.<id>.health
  ai.anomaly.detected         ai.prediction.ready
```

---

## 6. 前端架构

### 页面结构

```
App Shell (ProLayout)
├── Dashboard — 多集群健康、告警时间线、Pipeline 概览
├── K8s — 集群管理 / 工作负载 / 网络 / 配置 / 拓扑图
├── Docker — 容器 / 镜像 / 网络与存储
├── GitLab — 项目 / Pipeline DAG / Runner
├── AI 助手 — 右侧 Drawer，对话 + 异常 + 预测
├── 告警中心 — 告警分级 / 确认 / 规则配置
├── 审计日志
└── 系统设置 — 用户 / 角色 / SSO / 集群接入
```

### Tauri 桌面端差异

| 能力 | Web | 桌面 |
|------|-----|------|
| 系统托盘 | - | CPU/告警数 |
| 通知 | 浏览器 | 原生弹窗 |
| 文件关联 | - | .yaml 默认打开 |
| 离线缓存 | 有限 | 完整离线 |
| 快捷键 | - | Cmd+Shift+O |

---

## 7. 安全与认证

- **本地认证**：bcrypt + JWT（access 30min / refresh 7d）
- **SSO**：OIDC（Keycloak/Azure AD/Okta）+ LDAP
- **RBAC**：Resource × Action × Role，资源级权限控制
- **安全防护**：e-cat SecurityLayer（SQL注入/XSS/SSRF）+ RateLimit + AES-256 加密敏感字段
- **审计**：所有操作写入 MySQL audit_logs 表

---

## 8. 数据库设计（核心表）

### MySQL（业务数据）

```
users              — id, username, email, password_hash, mfa_secret
user_sso_bindings  — user_id, provider, external_id
roles / user_roles — 角色与权限
clusters           — id, name, kubeconfig_encrypted, status
docker_hosts       — id, name, endpoint, tls_config
gitlab_instances   — id, name, base_url, token_encrypted
audit_logs         — user_id, action, resource, detail_json, ip, created_at
alert_rules        — name, condition_json, severity, enabled
notification_configs — type, webhook_url, enabled
```

### ClickHouse（时序数据）

```
cluster_metrics     — cluster_id, metric_name, labels, value, ts
container_stats     — host_id, container_id, cpu_pct, mem_mb, net_rx_tx, ts
pipeline_executions — instance_id, project_id, status, duration_ms, ts
ai_anomalies        — cluster_id, anomaly_type, severity, detail_json, ts
ai_predictions      — cluster_id, metric_name, predicted_value, horizon, ts
```

---

## 9. 项目结构

```
super-ops/
├── protos/               # Protobuf 定义 (common/k8s/docker/gitlab/ai)
├── services/             # e-cat 微服务 (gateway/k8s/docker/gitlab/ai-engine)
├── frontend/             # Tauri + React
│   ├── src/              # pages/components/stores/hooks/services
│   └── src-tauri/        # Tauri Rust 后端
├── config/               # 配置文件模板
├── deploy/               # Docker Compose / Helm Charts
└── docs/superpowers/     # 设计文档与实现计划
```

---

## 10. 分阶段实施

| 阶段 | 内容 |
|------|------|
| **P1 MVP** | API Gateway + Auth、K8s Service（查看+日志+终端）、Dashboard + K8s 面板、Tauri 打包 |
| **P2 扩展** | Docker Service、GitLab Service、告警中心、RBAC、审计日志 |
| **P3 智能** | AI Engine LLM + ML 通道、Action Engine、规则引擎 |
| **P4 企业级** | SSO/LDAP、多租户、高可用、性能优化 |

---

## 11. 关键风险

| 风险 | 缓解 |
|------|------|
| e-cat 框架成熟度 | 框架作者即项目负责人，可及时修复 |
| 多 K8s 集群并发连接 | Watch 连接池 + 按集群限流 |
| LLM 响应延迟 | 流式返回 + 可中断 + 30s timeout |
| MySQL/ClickHouse 双写一致性 | 指标走异步 NATS→ClickHouse，允许秒级延迟 |
| Tauri 跨平台 | CI 矩阵构建 Win/Mac/Linux + E2E 覆盖 |
