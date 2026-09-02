# SuperOps — 智能运维平台

基于 [e-cat](https://github.com/erik/e-cat) 框架生态构建的智能运维平台（v1.8.0）。API 网关统一承载认证、限流、熔断、反向代理与 GraphQL；Kubernetes 资源服务提供查询、日志、Watch 与终端 exec；Collector 负责指标采集、巡检、告警与领域事件发布；Tauri 桌面前端完成可视化操作。

## 项目说明

SuperOps 面向中小规模基础设施运维，提供一条从「看见」到「处置」的闭环链路：

- **看见**：集群资源（Pod/Deployment/Node）、Pod 日志、实时终端、指标快照与告警
- **韧性**：网关限流（Redis 共享计数 + Consul 动态阈值热更新）、熔断（5xx 连续失败自动半开）、认证（JWT / OAuth2 / API Key 三通道）
- **可观测**：Prometheus 指标抓取、OTLP 链路追踪（Jaeger）、ClickHouse 时序落库
- **协同**：Consul 注册发现 + 远程配置、Kafka 审计事件总线

四阶段演进（已全部完成）：P1 MVP（认证 + 资源查询）→ P2 韧性与可观测（熔断/限流/注册/追踪）→ P3 集成与对外（exec 终端、API Key、OAuth2、OpenAPI）→ P4–P6 运营闭环（P4 写操作/审计/用户管理 → P5 日志/CMDB/脚本库/RBAC → P6 治理/审批/多租户/保险库/录制/文件/告警/指标/Helm）→ 生态扩展（混沌演练/资源配额/发布回滚/WAF 扫描/配置漂移/告警通知精细化/压测与 TLS）→ 框架能力深挖（CMDB 资产拓扑图数据库 / ES 日志检索 / S3 备份存储 / MQTT·NATS 消息协议 / etcd 注册中心 / GraphQL API / ecat-events 领域事件）。

## 技术架构

```
┌────────────┐   HTTP/WS  ┌─────────────┐   gRPC   ┌────────────┐
│  Frontend  │ ─────────► │   Gateway   │ ───────► │  K8s 服务   │ ──► Kubernetes API
│ Tauri/Web  │   :8080    │ auth+limit  │  :9091   │   :9091    │
└────────────┘            └──────┬──────┘          └────────────┘
                                 │ gRPC
                          ┌──────▼──────┐
                          │  Collector  │────► ClickHouse（快照/告警/领域事件）
                          └──────┬──────┘
                                 │
     MySQL（用户/Key） Redis（限流/锁） Kafka/MQTT/NATS（消息） Consul/etcd（注册/KV） Prometheus/Jaeger
     Neo4j（资产拓扑） Elasticsearch/OpenSearch（日志检索） MinIO（备份对象） GraphQL（/api/graphql）
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
│   │       ├── security.rs    #    WAF 错误转换层（SecurityError → 403/500）
│   │       ├── config_remote.rs #  Consul KV 热更新（DynamicRateLimitStore）
│   │       ├── metrics*.rs    #    Prometheus 导出 + 指标查询 API
│   │       ├── cmdb_api.rs / logs_api.rs / scripts_api.rs #  日志检索 / CMDB / 脚本库
│   │       ├── cmdb_topology.rs #  CMDB 资产拓扑（图数据库 sync / 查询 / explore）
│   │       ├── graphql.rs    #    GraphQL schema（ecat-graphql，/api/graphql）
│   │       ├── domain_events.rs # 领域事件消费 → ClickHouse domain_event
│   │       ├── quota_api.rs   #    资源配额（resource_quota upsert）
│   │       ├── chaos_api.rs   #    混沌演练（实验 CRUD + 执行）
│   │       ├── approval_api.rs #   审批工作流（delete 门禁）
│   │       ├── secrets_api.rs / vault.rs # 凭据保险库（AES-256-GCM，主密钥未配置 → 503）
│   │       ├── recordings_api.rs / recorder.rs # 终端录制（ClickHouse exec_session）
│   │       ├── files_api.rs   #    文件上传/下载（10MB multipart，MinIO）
│   │       ├── alerts_api.rs  #    告警中心（列表/确认）
│   │       ├── release_api.rs #    发布流水线（记录 + 手动回滚）
│   │       └── openapi.rs     #    /api/docs 文档
│   ├── k8s/                   #  K8s 资源服务 :9091（gRPC）
│   │   └── src/
│   │       ├── cluster/       #    集群管理（kubeconfig 增删查）
│   │       ├── resource/      #    pod/deploy/node/exec/metrics + job(RunJob) 查询
│   │       └── service.rs     #    tonic 服务实现（日志流/Watch/exec 双向流 + RunJob）
│   └── collector/             #  指标采集、巡检与日志采集
│       └── src/
│           ├── collect.rs     #    周期采集 → ClickHouse 快照
│           ├── logtail.rs     #    Pod 日志采集 → ClickHouse
│           ├── inspect.rs     #    巡检（Redis 分布式锁互斥）
│           ├── alert.rs       #    告警规则与事件写入
│           ├── drift.rs       #    配置漂移检测（CMDB vs 实际）
│           ├── rollback.rs    #    发布自动回滚（观察窗口）
│           ├── housekeeping.rs #   治理：MySQL 备份 / 容量与成本估算
│           ├── events.rs      #    审计事件消费（Kafka / MQTT / NATS）
│           ├── domain_events.rs #  告警 / 漂移 / 回滚领域事件发布（ecat-events）
│           └── mq.rs          #    消息后端装配（mqtt > nats > kafka）
├── frontend/                  # React + Vite + Tauri 2（:3000）
│   └── src/pages/            #  k8s（集群/Pod/Node/终端/日志）+ cmdb + ops（审计/API Keys/用户/脚本/告警/指标/日志检索/录制/审批/保险库/文件）
├── ecat-*/                    # e-cat 框架组件（workspace 成员）
├── superops-protos/           # protobuf 生成代码（common.v1 / k8s.v1）
├── protos/                    # proto 源文件（buf 管理）
├── bench/                     # ecat-bench 压测入口（BENCH_TARGET=health|login）
├── config/                    # 各服务 YAML 配置
├── deploy/                    # docker-compose.yml + init.sql + prometheus.yml + helm/superops（Chart）
└── docs/                      # 审查报告 / 实施计划 / 架构图
```

![项目结构](docs/images/tree.svg)

## 功能说明

### Gateway（:8080）
| 功能 | 说明 |
|---|---|
| 认证 | 登录/注册；JWT 签发/校验基于框架 ecat-auth（HS256、`AuthClaims` 统一注入、密钥 ≥32 字节强制校验）；可选 OAuth2 层（ecat-auth，配置 `oauth2` 段即启用）；`X-API-Key` 内存表鉴权（吊销即时生效）；WS 场景支持 query token 回退；角色体系 admin/operator/viewer（首个注册用户自动 admin；operator 含 api:read/api:write/ops:audit/ops:cmdb/ops:scripts，viewer 仅 api:read；API Key 按用户角色走同一映射） |
| 限流 | 登录 10 次/60s/IP；Redis 共享计数（不可用自动回退内存）；阈值经 Consul KV 动态调整 |
| 熔断 | k8s 代理路由 5xx 连续失败熔断 10s（失败率 50%/窗口 30s/半开探针 3），期间 503 |
| 代理 | `/api/k8s/*` → gRPC 转发（查询 + scale/restart/delete 写操作）；`/exec` WebSocket ↔ gRPC 双向流（含 terminal resize，写路由 require_role("api:write")）；写操作成功发布 Kafka 审计事件（`k8s.scale`/`k8s.restart`/`k8s.delete`，payload 含 `level: "INFO"`） |
| 日志检索 | `GET /api/logs/search`：collector logtail 采集双写（ClickHouse 落库 + 可选索引到搜索后端）；检索时按 `search:` 段配置分流（Elasticsearch / OpenSearch bool/term/match/range DSL，后端不可用自动回退 ClickHouse），require_role("api:read") |
| CMDB 资产 | `/api/cmdb/assets`（GET 列表 / POST 创建）、`/api/cmdb/assets/{id}`（DELETE）、`/api/cmdb/stats`（GET 统计），require_role("ops:cmdb") |
| CMDB 资产拓扑 | 图数据库后端（`graph:` 段，neo4j / nebulagraph / arangodb）：`POST /api/cmdb/topology/sync`（资产 → 图节点 + depends_on 依赖边 upsert、孤儿清理）、`GET /api/cmdb/topology`（节点/边）、`POST /api/cmdb/topology/explore`（原生图查询，上限 4KB）；前端 `/cmdb`「拓扑图」Tab（手写 SVG 圆环布局），ops:cmdb |
| GraphQL | `POST /api/graphql`：ecat-graphql schema（`health` / `cmdbStats` / `alerts` / `backups` 查询），require_role("api:read") |
| 领域事件 | collector 告警 / 配置漂移 / 自动回滚经 ecat-events 事件总线发布 DomainEvent → gateway 消费落 ClickHouse `domain_event`；`GET /api/events?limit=&event_type=`（ops:audit）；前端 `/ops/events` 页 |
| 脚本库 | `/api/scripts`（GET/POST）、`/api/scripts/{id}`（DELETE）、`/api/scripts/{id}/run`（POST → k8s Job 批量执行）、`/api/scripts/runs`（GET 运行记录），仅支持 shell（busybox:1.36），require_role("ops:scripts") |
| 审批 | 删除 deployment 门禁（`approval.enabled` 时未审批删除返回 412）；`GET/POST /api/approvals`（kind=delete，target 约定 `"{cluster_id}/{ns}/{name}"`）、`POST /api/approvals/{id}/decide`；**默认关闭** |
| 多租户 | `x-tenant-id` 请求头经 `require_tenant` 中间件解析（仅小写字母/数字/连字符 1..=64，非法/缺失回落 `default`）；cmdb/scripts 数据按 `tenant_id` 隔离 |
| 保险库 | `/api/secrets`（GET 列表 / POST 创建 / GET+DELETE 单条，AES-256-GCM 加密）；`SUPEROPS_MASTER_KEY` 未配置或非 32 字节 → 全部接口 503；密文值不入日志 |
| 录制 | exec WebSocket 帧旁路写入 ClickHouse `exec_session`（帧含会话级 seq 序号，回放按 timestamp, seq 定序）；`/api/recordings`（GET 列表）、`/api/recordings/{sid}/frames`（GET，上限 5000 帧）、`/api/recordings/{sid}`（DELETE），read api:read / write api:write；`recording.enabled` 配置开关（默认开启） |
| 文件 | `POST /api/files`（multipart 10MB 上限，文件名白名单）+ `GET /api/files/{name}`，MinIO 存储，api:write/api:read |
| 告警中心 | `GET /api/alerts?level=&limit=50`（ClickHouse `alert_event`，ops:cmdb）、`POST /api/alerts/{id}/ack`（api:write，幂等）、`GET /api/alerts/acks` |
| 告警规则 | `/api/alert-rules`（GET/POST/PATCH/DELETE，MySQL `alert_rule` 表，ops:cmdb）；collector 按规则求值（连续 N 次超标触发，`max_not_ready` 节点数限制），规则 `action=restart/scale` 触发自愈 |
| 告警通知 | 通知通道 generic/dingtalk/wecom webhook + SMTP 邮件（`kind=email`，收件人逗号分隔多个，密码可经 `SUPEROPS_SMTP_PASSWORD` 覆盖）；`levels` 字段按级别过滤投递（不写则全量）；邮件与 webhook 通知附加当前值班人（需配置 mysql）；按 target 静默窗口（默认 300s） |
| 混沌演练 | `/api/chaos`（GET/POST 实验 CRUD）、`/api/chaos/{id}`（DELETE）、`/api/chaos/{id}/run`（POST 执行 restart/delete，转发 k8s-service），ops:cmdb；前端 `/ops/chaos` 页 |
| 自愈 | 规则 `action=restart/scale` 时 collector 调用 k8s restart/scale（`selfheal.enabled` **默认关闭**，`max_actions_per_cycle` 限制），动作入审计 |
| 工单 | `/api/tickets`（GET/POST，status 流转 open→in_progress→resolved/closed，reopen）、`/api/tickets/{id}`（GET/PATCH/DELETE）；告警一键建单（`GET /api/alerts` 行内操作） |
| Runbook | `/api/runbooks`（GET/POST，`steps` JSON 数组）、`/api/runbooks/{id}`（GET/PATCH/DELETE）、`/api/runbooks/{id}/run`（顺序执行各步骤并返回逐步结果） |
| 值班排班 | `/api/oncall`（GET/POST，排班记录 + 值班人员字段） |
| 发布流水线 | k8s-service gRPC `UpdateImage`（deployment 镜像更新）+ `/api/releases`（GET/POST 发布记录）+ `/api/releases/{id}/rollback`（手动回滚到旧镜像，无旧镜像 400）+ 前端发布页（回滚按钮 + 确认）；collector `rollback` 任务自动回滚（发布 ok 后 delay 秒进入观察窗口，deployment ready==0 或缺失时恢复旧镜像并标记 failed，**默认关闭**） |
| 容量/成本 | `/api/capacity/summary`、`/api/capacity/trend`（ClickHouse 汇聚，节点/Pod 数、成本估算时间序列） |
| 备份状态 | `/api/backups/status`（GET 列表 / POST 上报，agent 经 API key 回调）、`/api/backups/summary`（按库汇总 + 时效统计）、`/api/backups/objects`（S3/MinIO 桶对象列表，`storage:` 段配置后可用）；前端 `/ops/backups` 页含备份存储对象卡片 |
| 终端管控 | exec WS 需 `?confirm=1`（`terminal.require_confirm` 默认开，未带返回 400）+ `terminal.max_session_secs` 超时强制断开（默认 1800s），前端会话前弹确认框 |
| 配置中心 | `/api/config/remote/keys`（GET 递归列出 `config/superops` 前缀 Consul KV）、`/api/config/remote/keys/{key}`（GET/PUT/DELETE，key 越界 400，Consul 不可达 502），ops:cmdb |
| 资源配额 | `resource_quota` 表（UNIQUE cluster_id+namespace）+ `/api/quota`（GET/POST upsert，replicas 限额）、`/api/quota/{id}`（DELETE），ops:cmdb；前端 `/ops/quota` 页 |
| 安全扫描 | 全请求 WAF 层（ecat-security `SecurityBodyLayer`，最外层）：扫描 URI+headers+body（最多 10MB，body 读后透传），SQLi/XSS 等 High/Critical 命中 → 403 `{"error":...}`，其余级别仅记日志 |
| MySQL TLS | `database.tls` 配置段（ecat-tls）：ca_cert/client_cert/client_key PEM 路径 + skip_verify（true=仅加密不校验域名 / false=VerifyIdentity 全校验） |
| 跨集群聚合 | `/api/k8s/aggregate`：全集群节点/Pod 健康汇总（`nodes_ready`=Ready 节点数、`pods_running`=Running Pod 数 + totals） |
| 指标看板 | 前端 `/ops/metrics` 复用 `GET /api/v1/metrics/query`（api:read） |
| 治理 | collector `housekeeping`：MySQL 备份（mysqldump → `data/backups`，保留 N 份）、磁盘容量与成本估算（collector.yaml `housekeeping:` 段） |
| 可观测 | `/health`、`/ready`（含 MySQL 依赖检查）、`/metrics`（Prometheus）、`/api/docs`（OpenAPI 3.0.3）、OTLP span（gateway/k8s/collector 三服务） |
| 运维 | Consul 注册 + `discover("superops-k8s")` 端点解析（失败回退静态配置）；KV 热更新 |

### K8s Service（:9091）
集群管理（kubeconfig）、Pod/Deployment/Node 查询（Node 含 Ready 状态与 kubelet 版本）、Deployment scale/restart/delete 写操作（gRPC `ScaleDeployment`/`RestartDeployment`/`DeleteDeployment`）、批量执行（gRPC `RunJob` → batch/v1 Job，shell 脚本）、Pod 日志流（tail/follow）、资源 Watch（ADDED/MODIFIED/DELETED）、exec 双向流（stdin/stdout/stderr + resize）。

### Collector
按调度周期采集指标 → ClickHouse 快照；日志采集（logtail.rs：周期采集 Pod 日志 → ClickHouse，配置 `search:` 段时按行索引到 Elasticsearch / OpenSearch）；巡检任务经 Redis 锁保证单实例执行；告警规则求值（读 MySQL `alert_rule`，连续 N 次超标触发，`max_not_ready` 限制节点级告警，`action=restart/scale` 时执行自愈动作——`selfheal.enabled` 默认关闭，告警触发后经 ecat-events 发布领域事件）；告警通知（generic/钉钉/企微 webhook + SMTP 邮件 `kind=email`，`levels` 级别过滤 + 值班人附加，按 target 静默窗口，默认 300s）；配置漂移检测（drift.rs：CMDB deployment 资产 vs 集群实际 deployment，差异写 ClickHouse `drift_event` 并发布领域事件，`drift.enabled` **默认关闭**）；发布自动回滚（rollback.rs：`status=ok` 的发布在 `delay_secs` 后进入 `window_secs` 观察窗口，deployment 缺失或 ready==0 且 replicas>0 时恢复旧镜像并标记 failed，回滚后发布领域事件，`rollback.enabled` **默认关闭**）；治理任务（housekeeping.rs：MySQL 备份 + 容量/成本估算）；审计事件消费（events.rs，消息后端按 `mqtt:` / `nats:` / `mq:` 优先级装配，见 mq.rs）；领域事件发布（domain_events.rs，ecat-events 远程总线复用消息后端）；注册中心（on_start：`etcd:` 段配置则注册 etcd，30s lease + `superops/services` 前缀，否则回落 Consul）；OTLP span 导出。

### Frontend（:3000）
登录、集群列表/详情、Pod 列表与日志、Deployment、Node、终端页（WS 双向流，token query 鉴权、binaryType 处理、可选容器、会话前确认框 + 服务端超时断开）；dashboard 卡片接入真实接口（集群数 / Docker 主机数（metrics query，临时指标 app）/ 活跃告警（告警中心接口））+ 集群健康聚合卡片（每集群节点/Pod Ready 汇总 + totals）；CMDB 资产页（`/cmdb`，ProTable + 新建弹窗 + 「拓扑图」Tab（手写 SVG 圆环布局：资产类型着色 + 状态描边 + 同步按钮），菜单位于 Kubernetes 与运维中心之间）；脚本库页（`/ops/scripts`，脚本列表 + 运行记录双 ProTable）；运维中心（`/ops/audit` 审计、`/ops/apikeys` API Key、`/ops/users` 用户管理、`/ops/alerts` 告警中心（10s 轮询 + 确认 + 一键建单）、`/ops/alert-rules` 告警规则（CRUD + 启用开关）、`/ops/metrics` 指标看板（复用 metrics query）、`/ops/logs` 日志检索（条件 + 时间范围）、`/ops/recordings` 录制回放（帧按 seq 定序）、`/ops/approvals` 审批中心（状态过滤 + 通过/拒绝/取消/reopen）、`/ops/secrets` 保险库（加密存储 + 解密查看/复制，主密钥未配置时展示 503 提示）、`/ops/files` 文件管理（multipart 上传 / 按名下载）、`/ops/oncall` 值班排班、`/ops/traces` 链路追踪（Jaeger UI 嵌入）、`/ops/tickets` 工单系统（状态流转）、`/ops/releases` 发布流水线（镜像更新 + 记录 + 回滚按钮）、`/ops/chaos` 混沌演练（实验 CRUD + 执行）、`/ops/runbooks` Runbook 剧本（步骤执行）、`/ops/capacity` 容量/成本趋势（手写 SVG 图）、`/ops/backups` DB 备份状态（含备份存储对象卡片）、`/ops/events` 领域事件（类型过滤 + 级别着色）、`/ops/config` 配置中心（Consul KV 浏览/编辑/删除）、`/ops/quota` 资源配额（集群+命名空间限额）、`/ops/grafana` Grafana 嵌入页（iframe sandbox，URL 优先级 `?url=` > `VITE_GRAFANA_URL` > localhost:3000））；Pods/Deployments/Nodes 独立页带集群选择器（默认首个集群，不再硬编码 `default`；集群详情页内嵌场景不受影响）。

### API 一览（OpenAPI 见 /api/docs）
`/api/auth/register|login`、`/api/keys`（CRUD）、`/api/users`（`PATCH /{id}/status` 启用/禁用）、`/api/audit/events`（审计查询，limit/offset/level）、`/api/logs/search`（日志检索，ClickHouse / ES-OpenSearch 分流）、`/api/cmdb/assets`（GET/POST）、`/api/cmdb/stats`（GET）、`/api/cmdb/topology`（GET 拓扑查询）、`/api/cmdb/topology/sync`（POST 拓扑同步）、`/api/cmdb/topology/explore`（POST 原生图查询）、`/api/cmdb/assets/{id}`（DELETE）、`/api/scripts`（GET/POST）、`/api/scripts/{id}`（DELETE）、`/api/scripts/{id}/run`（POST）、`/api/scripts/runs`（GET）、`/api/approvals`（GET/POST，delete 审批门禁）、`/api/approvals/{id}/decide`（POST）、`/api/secrets`（GET/POST）、`/api/secrets/{name}`（GET/DELETE）、`/api/recordings`（GET）、`/api/recordings/{sid}`（DELETE）、`/api/recordings/{sid}/frames`（GET）、`/api/files`（POST 上传）、`/api/files/{name}`（GET 下载）、`/api/alerts`（GET，level/limit）、`/api/alerts/{id}/ack`（POST）、`/api/alerts/acks`（GET）、`/api/alert-rules`（GET/POST/PATCH/DELETE，规则引擎）、`/api/tickets`（GET/POST）、`/api/tickets/{id}`（GET/PATCH/DELETE）、`/api/runbooks`（GET/POST）、`/api/runbooks/{id}`（GET/PATCH/DELETE）、`/api/runbooks/{id}/run`（POST）、`/api/oncall`（GET/POST）、`/api/releases`（GET/POST）、`/api/releases/{id}/rollback`（POST）、`/api/chaos`（GET/POST）、`/api/chaos/{id}`（DELETE）、`/api/chaos/{id}/run`（POST）、`/api/quota`（GET/POST）、`/api/quota/{id}`（DELETE）、`/api/capacity/summary|trend`（GET）、`/api/backups/status`（GET/POST）、`/api/backups/summary`（GET）、`/api/backups/objects`（GET，S3/MinIO 对象列表）、`/api/events`（GET，领域事件，event_type 过滤）、`/api/graphql`（POST，GraphQL）、`/api/config/remote/keys`（GET）、`/api/config/remote/keys/{key}`（GET/PUT/DELETE，Consul KV）、`/api/k8s/aggregate`（GET，跨集群汇总）、`/api/k8s/clusters[/{id}][/pods|/deployments|/nodes|/metrics]`、`/api/k8s/clusters/{id}/pods/{ns}/{pod}/logs|exec`（exec 需 `?confirm=1`）、`/api/k8s/clusters/{id}/deployments/{ns}/{name}/scale|restart`（`DELETE` 删除）、`/api/v1/metrics/query`、`/api/health`、`/api/docs`。

## 快速开始

```bash
# 1. 基础设施（MySQL 3307 / Redis 6380 / ClickHouse 8124 / Prometheus 9095 /
#    Kafka 9092 / Consul 8500 / Jaeger 4317+16686 / MinIO 9002+9003）
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
| gateway gRPC | 9090 |
| k8s service gRPC | 9091 |
| collector | 0（仅任务） |
| frontend (vite) | 3000 |
| MinIO | 9002 (API) / 9003 (console) |
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
| `SUPEROPS_MASTER_KEY` | 保险库主密钥（32 字节；未设置时 `/api/secrets` 503） |
| `MINIO_ROOT_USER` / `MINIO_ROOT_PASSWORD` | MinIO 凭据（`deploy/.env`，生产必改） |
| `RUST_LOG` | tracing 级别（缺省 info） |

基础设施密码通过 `deploy/.env` 注入 compose（模板见 `deploy/.env.example`）；各服务 YAML 见 `config/`。

## 测试与 CI

- workspace 单测/集成测试 444 项全过（gateway 114 / k8s 18 / collector 65 / ecat 组件，见 `docs/audit-report-2026-08-06.md` 测试矩阵）
- 端到端与安全验证：登录/注册、k8s 路由正误路径、认证绕过、JWT 伪造、限流 429、熔断 503、API Key 生命周期（见 `docs/audit-report-2026-08-06.md`）
- 运行时验证：Consul 注册/注销、KV 热更新（阈值 3↔10 双向生效）、Jaeger span、Prometheus target up
- workspace `cargo clippy --all-targets` 零警告；前端构建零警告（路由级代码拆分 + 供应商分包，vendor-antd 786.5kB / index 15.3kB）
- CI（`.github/workflows/ci.yml`）：workspace `cargo fmt --check` + `cargo check --workspace` + `cargo test --workspace` + `cargo clippy --workspace --all-targets -- -D warnings` + 前端 `npm run test` + `npm run build`

## 已知边界

- **OAuth2**：默认关闭；开启需配置 `oauth2` 段（introspection URL + client id/secret）与外部 IdP
- **主密钥**：`SUPEROPS_MASTER_KEY` 默认未设置（未设置或非 32 字节时 `/api/secrets` 全部 503，两态可用）；生产启用保险库前必须设置
- **审批开关**：`approval.enabled` 默认关闭（`config/gateway.yaml`），开启后删除 deployment 需先提交并审批通过 delete 审批单（412 门禁）
- **exec 终端**：需真实 k8s 集群，且角色需 operator+（写路由 require_role("api:write")）；无集群时返回明确错误帧；`?confirm=1` 缺失返回 400，会话超时（`terminal.max_session_secs`，默认 1800s）强制断开
- **终端确认/超时**：`terminal.require_confirm` 默认开启（config/gateway.yaml `terminal:` 段）；无需确认可显式关闭
- **告警通知邮件**：`kind=email` 的 notify target 需配置 `smtp:` 段（collector.yaml），未配置时该 target 报错；SMTP 密码可用 `SUPEROPS_SMTP_PASSWORD` 覆盖
- **自愈动作**：`selfheal.enabled` 默认关闭（collector.yaml），开启后规则 `action=restart/scale` 才会实际执行，`max_actions_per_cycle` 限制每周期动作数
- **配置中心**：Consul KV 仅限 `config/superops` 前缀（越界 400）；Consul 不可达时 502；服务重启后 k8s 集群注册需重新添加（ClusterManager 内存存储）
- **OTLP 覆盖**：gateway HTTP 路径 + k8s gRPC + collector 调度均接入 OTLP span；录制旁路写入等非阻塞路径无 resource 字段（非阻塞项）
- **多租户**：tenant 由客户端 `x-tenant-id` 头断言（无服务端租户目录）；头缺失回落 `default`，存在但畸形返回 400（严格模式）
- **录制开关**：`recording.enabled` 默认开启（`config/gateway.yaml`）；回放帧上限 5000（ORDER BY timestamp, seq）
- **单实例部署**：限流计数为 Redis 共享，但服务本身单实例
- **WAF 边界**：安全扫描对 headers/body 明文 payload 有效；URI 中的百分号编码 payload 不在扫描范围（scanner 不解码）；`/api/files` 上传等大 body 扫描上限 10MB（超限 500）
- **自动回滚**：`rollback.enabled` 默认关闭（collector.yaml）；开启后仅在观察窗口内（`delay_secs`~`delay_secs+window_secs`）对 `status=ok` 且存在旧镜像的发布生效，deployment 缺失或 `ready==0 && replicas>0` 才触发（扩容中 replicas=0 不误判）
- **图数据库（拓扑）**：`graph:` 段未配置时 `/api/cmdb/topology*` 返回 provider 降级信息（前端拓扑页提示「未配置」）；`explore` 原生查询仅 neo4j（Cypher）支持，nebulagraph / arangodb 返回不支持
- **搜索后端（日志）**：`search:` 段未配置时日志检索恒走 ClickHouse；已配置但搜索后端请求失败（503/400）时该次检索自动回退 ClickHouse 并仅告警
- **备份对象**：`storage:` 段未配置时 `/api/backups/objects` 返回空对象列表 + provider 占位（前端展示「未配置 S3/MinIO」提示）；S3/MinIO 为可选后端，不进 docker-compose
- **MQ 多协议**：collector 消息后端优先级 `mqtt:` > `nats:` > `mq:`（Kafka）；MQ 初始化失败时审计消费与领域事件发布/消费禁用（WARN 降级，服务继续运行）
- **etcd 注册**：`etcd:` 段未配置时 collector 回落 Consul 注册；etcd 端点不可达时注册失败仅告警，不阻断启动

## 文档

- `docs/audit-report-2026-08-06.md` — 全量审查报告（测试矩阵、安全清单、修复记录）
- `docs/project-plan-2026-08.md` — 下一阶段项目规划（团队侦察合成：任务清单 / 风险登记册 / 分工矩阵 / 冲刺计划）
- `docs/multicloud.md` — 多云接入指南（多集群注册流程、按集群操作端点、跨集群聚合视图）
- `docs/images/` — 架构图与图集（architecture / flow / design / structure / security / lifecycle / tree）
- `docs/superpowers/plans/` — 各阶段实施计划（P1 MVP / P2 韧性 / P3 集成）
- `CHANGELOG.md` — 变更日志

## 支持

如果这个项目对你有帮助，欢迎扫码支持（支付宝 / 微信均可）：

| 支付宝 | 微信 |
|---|---|
| <img src="docs/alipay.png" width="130" height="130" alt="支付宝收款码"> | <img src="docs/weixinpay.png" width="130" height="130" alt="微信收款码"> |

### 虚拟币打赏 (Crypto Donation)

如果这个项目对你有帮助，欢迎扫描二维码打赏支持，谢谢！

| 主网 (Network) | 二维码 (QR Code) | 钱包地址 (Wallet Address) |
|---|---|---|
| BNB Smart Chain (BEP20) | [<img src="docs/coin/1.jpg" width="150" alt="BNB Smart Chain (BEP20)">](docs/coin/1.jpg) | `0x355d429f97511897ccb4e271ec888205f9ab6629` |
| Tron (TRC20) | [<img src="docs/coin/2.jpg" width="150" alt="Tron (TRC20)">](docs/coin/2.jpg) | `TEdDHWLajt1XvqtPDWmQctdrJaC3pzZZzz` |
| Ethereum (ERC20) | [<img src="docs/coin/3.jpg" width="150" alt="Ethereum (ERC20)">](docs/coin/3.jpg) | `0x355d429f97511897ccb4e271ec888205f9ab6629` |
| Aptos | [<img src="docs/coin/4.jpg" width="150" alt="Aptos">](docs/coin/4.jpg) | `0x836e3780edfc3f7b2372b39e2a1a3a5d7adfaccd96c726f21cfde1b50dd68030` |
| Plasma | [<img src="docs/coin/5.jpg" width="150" alt="Plasma">](docs/coin/5.jpg) | `0x355d429f97511897ccb4e271ec888205f9ab6629` |
| Polygon POS | [<img src="docs/coin/6.jpg" width="150" alt="Polygon POS">](docs/coin/6.jpg) | `0x355d429f97511897ccb4e271ec888205f9ab6629` |
| Solana | [<img src="docs/coin/7.jpg" width="150" alt="Solana">](docs/coin/7.jpg) | `2hfhboHdmdrYsY25XfQSsEWxq5ip4EQsR7f4AzSRMUyr` |
| The Open Network (TON) | [<img src="docs/coin/8.jpg" width="150" alt="The Open Network (TON)">](docs/coin/8.jpg) | `UQB9kFQohzmXUir9QSSZq01iwl9aQZIDdBpNmDklljRtCoGK` |
| Arbitrum One | [<img src="docs/coin/9.jpg" width="150" alt="Arbitrum One">](docs/coin/9.jpg) | `0x355d429f97511897ccb4e271ec888205f9ab6629` |
| AVAX C-Chain | [<img src="docs/coin/10.jpg" width="150" alt="AVAX C-Chain">](docs/coin/10.jpg) | `0x355d429f97511897ccb4e271ec888205f9ab6629` |
