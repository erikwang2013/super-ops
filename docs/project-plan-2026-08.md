# SuperOps 下一阶段项目规划（基于团队侦察，v1.8.0 现状）

- 日期：2026-08（编制于 v1.8.0 之后）
- 方式：7 个领域侦察 agent（网关 / K8s / Collector / 前端 / 框架层 / 平台交付 / 质量横切）并行勘察 + 系统架构师合成 + Lead 验收
- 范围：全系统只读侦察结论，全部条目源自领域侦察报告（标注「报告 N」），未新增需求
- 结论：**功能面已完整、生产面未达标**——下一阶段核心是「数据通路修复 + 安全基线 + 质量门禁 + 发布就绪」，而非新功能

> **执行状态（2026-08-16 更新，详见 CHANGELOG [1.8.1] / [1.8.2]）**：
> - **第 1 周「基线锁定」已完成**：版本归一（workspace/Helm/Tauri/frontend 四口径 → 1.8.0，audit 报告为历史快照保留）；CI 全覆盖（workspace fmt/check/test + clippy `-D warnings` 实测零警告——修复了 gateway `cmdb_topology.rs` 测试辅助函数的 `too_many_arguments`）；echarts 幽灵依赖修复（声明进 `frontend/package.json`，根 `package.json` 收敛）；死 CI 移除（`ecat-deploy/.github`、`.gitlab-ci.yml`）；README 测试数同步；前端 vitest 测试脚手架落地。
> - **W2「数据通路主攻」已完成**：collector `k8s.cluster_id` 配置 + 自动解析（ListClusters 取首个）接线进六类周期任务（collect/logtail/inspect/selfheal/drift/collect_capacity），无集群时任务跳过并告警；新增 `cluster.rs` + 5 项 gRPC 契约测试（mock k8s-service）；collector 测试 57→62，workspace 420→425；容量 tag 硬编码 `"default"` 修复。
> - **W3「安全基线」已完成两项 P0**：① 审批门禁统一下沉为 `model::approval::check_delete_approval`（k8s 删除路由 + 混沌 delete 共用，修复 chaos `delete` 绕过审批），新增回归测试 ×3；② k8s-service Bearer 服务间鉴权（`auth.token` 配置即启用，tonic interceptor 401 拒无凭据；gateway/collector 经 `BearerChannel` 注入 authorization 头，三份 yaml 已加 token 段），新增鉴权测试 ×3。workspace 测试 425→431，clippy/fmt 三服务全绿。限流键可信化（P1）按规划排期 W5。
> - **W4「消息与审计」已完成**：① Kafka 手动 offset commit——`MessageStream` 新增 `commit()`（Kafka `store_offset`+`commit_consumer_state`，其余后端 no-op），collector 审计消费 CH 写入成功后才 commit（at-least-once，重启不丢审计）；② 审计本地兜底——gateway 写操作审计统一收敛到 `audit::publish`（MQ 失败/缺失落 `data/audit/audit-YYYYMMDD.jsonl` JSONL），删除两处重复实现；新增兜底单测 ×2。workspace 测试 431→433。logtail 游标化（P1）按规划排期阶段二。
> - **W5「认证闭环 + 框架快赢」已完成**：① refresh 闭环——`/api/auth/refresh` + `/api/auth/logout`，JWT 带 `jti`，Redis 黑名单（`auth::blacklist`），刷新轮换旧 token 立即作废；② 限流键可信化——登录限流默认取真实对端 IP（ecat-transport-http 注入 ConnectInfo），`rate_limit.trust_proxy` 显式开启才信任代理头；③ etcd 三缺陷修复——lease 保活循环、list_services 前缀修复、deregister 实例级删除（新增测试 ×2 抓出双斜杠真实 bug）。workspace 测试 433→437。
> - **W6「收口」已完成（阶段一全部闭环）**：① **RdbmsClient 收敛决策**——服务数据层保持直连 sqlx（强类型绑定/事务非 Value 参数化可替代），`RdbmsClient`/`SqlxClient` 抽象不采纳为服务层（保留框架储备），清理 gateway/k8s `ecat-data-sqlx` 死依赖（零引用）；② 1.8.0 新功能测试核查——logtail/notify/rollback/drift/mq 纯逻辑测试已齐（侦察报告部分过时，notify 已有 10+ 用例）；③ 前端测试扩充——`services/api.test.ts` ×4（Authorization/401/错误/204），前端 vitest 3→7 用例。
> - **阶段一出口检查通过**：CI 门禁落地（workspace fmt/clippy `-D warnings`/test + 前端 test/build）、六类任务 cluster_id 接线完成、k8s-service 鉴权可用、审批不可绕过、审计不丢、版本单一。**下一阶段（阶段二）**：发布流水线（Dockerfile/镜像/Helm 真值化）、集群注册持久化、告警状态机降噪、logtail 游标化、可观测部署、1.8.0 核心测试继续补强等，见本规划 §3 阶段二。

> 交付前已抽样复核关键 P0 事实，与报告一致：`chaos_api.rs:150` 直接调 `delete_deployment` 绕过 `is_delete_approved`（对比 `k8s_proxy.rs:609` 走审批）；`frontend/package.json` 无 echarts 而根 `package.json` 有（幽灵依赖坐实）；根 `.github/workflows/ci.yml` 仅 gateway+k8s 的 fmt/check/test，clippy 组件安装但从未执行；collector 六类任务 k8s 调用均未携带真实 cluster_id（logtail 硬编码空串，collect/drift/housekeeping 用默认请求，alert/rollback 用 `"default"` 字面量，均无法命中 UUID 主键的真实集群）。

---

## 0. 规划摘要

1. **功能面已完整、生产面未达标**：P1–P6 与生态扩展七连均已交付，但集群注册不持久、collector 六类周期任务 cluster_id 未接线即全链路失效、审计/消息存在静默丢失路径——平台"能演示"但"不可信"（报告 2/4/6）。
2. **存在三个可被直接利用的安全漏洞**：k8s-service 无鉴权无 TLS（到达 :9091 即可 RunJob/ExecPod 任意代码执行）、混沌 delete 绕过审批门禁、refresh token 签发即死（报告 2/4）。
3. **质量门禁未建立**：420 项测试实数但 collector/58 个 ecat crate 零 CI 覆盖、clippy 从未进 CI、前端零测试且有 echarts 幽灵依赖（干净环境 CI 必红）、版本号四套口径（报告 1/3/5）。
4. **框架层"抽象与生产脱节"**：RdbmsClient 被 22 文件绕过、ecat-registry-etcd 三缺陷、约 19 个储备 crate 无消费方——应优先"在用路径的深度"而非"适配器广度"（报告 7）。
5. **无发布/镜像流程**：三服务无 Dockerfile、Helm 镜像占位、P6 明确划为"不做"边界——下一阶段必须跨越，否则一切修复无法交付到真实环境（报告 3）。

**总体策略一句话**：以"数据通路与安全基线"为第一优先，以"CI 全覆盖 + 单一版本号"为一切改动的发布前提，坚持"在用深度优先于适配器广度"，分三阶段把 SuperOps 从"功能完整"推进到"可发布、可观测、可演进"。

## 1. 现状定位

### 1.1 已实现能力矩阵（按领域）

- **网关 gateway（:8080，约 40 模块）**：HTTP/WS 唯一入口，含认证/限流/熔断/WAF/代理/CMDB/GraphQL/告警/混沌/发布/保险库/录制；所有 exec 与写操作经网关，鉴权回归即 CRITICAL（历史上发生过一次认证绕过）（报告 1/4）。
- **k8s 服务（gRPC :9091）**：集群管理（ClusterManager = DashMap 内存态，UUID 主键）、资源查询、写操作（scale 0..=1000 校验/restart/delete/UpdateImage/RunJob）、流式（Watch 仅 Pod、日志 tail/follow、exec 双向流）、GetMetrics 为空实现占位（报告 2）。
- **collector**：周期采集/日志/巡检/告警/漂移/回滚/治理/审计消费/领域事件发布；依赖 Kafka、ClickHouse、MySQL、ES、Redis 锁（报告 1/6）。
- **ecat 框架层**：约 60 个 crate（transport/data/mq/registry/security/auth/tracing/graphql 等）支撑三服务；储备适配器约 10 个无消费方（报告 1/7）。
- **前端**：React 18 + antd + xterm + Vite + Tauri 2，32 路由页；含终端/日志/拓扑/告警/发布等页面（报告 1/5）。
- **交付链**：protos/ 源 + superops-protos/ 生成双管道、deploy（compose + helm + prometheus + init.sql 16 表）、bench（ecat-bench）——但无镜像、无发布流水线（报告 3）。

### 1.2 真实质量基线

- **测试实数 420**：gateway 107 / collector 57 / k8s 14 / ecat 242；README 声称 408（L214）滞后 12 项，CHANGELOG [1.8.0] 可信（报告 1）。
- **CI 缺口**：根 ci.yml 仅 gateway+k8s 的 fmt/check/test；collector、58 个 ecat crate 零覆盖；clippy 从未进 CI（无 `-D warnings`）；无 workspace 全量门禁；无 buf lint/breaking；ecat-deploy 嵌套 ci.yml 与 .gitlab-ci.yml 为死配置永不触发（报告 1/3）。
- **前端**：CI 仅有 build 无 test；echarts 幽灵依赖使干净环境 tsc 必失败；39 处 any 类型绕行（报告 5）。
- **测试真空**：collector 1.8.0 新功能（logtail/rollback/drift/notify/mq/domain_events/main）全无测试；gateway 的 secrets/approval/chaos/cmdb/oncall/quota/backup 等无测试；k8s cluster/resource 核心文件零测试；ecat-data 371 行抽象 0 测试；前端全零测试（报告 1）。
- **代码卫生**：生产 unwrap ≈30 处（collector main.rs 9、k8s_proxy 5、model/api_key 4、config_remote 4）；源码 TODO 为零、panic 仅 1 处（报告 1）。
- **版本号四套口径**：Cargo.toml 1.1.6 / README 1.8.0 / audit 1.0.2 / tauri 0.1.0（报告 1/3）。

### 1.3 四大战略主题

1. **生产就绪**：CI 全覆盖 → 镜像/Helm/发布流水线 → 版本归一 → 可观测部署完善（报告 1/3）。
2. **数据通路修复**：cluster_id 接线 → 审计/消息可靠投递 → logtail 游标化 → 多实例幂等（报告 2/4/6）。
3. **体验闭环**：Pod 日志接通、认证持久化+刷新、录制回放、桌面端 CSP（报告 5）。
4. **框架收敛**：RdbmsClient 决策、etcd 修复、储备适配器收敛、错误/观测体系统一（报告 7）。

## 2. 规划原则

1. **事实驱动**：以 420 测试实数、代码路径与配置实际值为准，优先于 README 与 audit 文档——本次侦察已发现 5 类文档滞后（测试数、文件存储介质、混沌 action、metrics 路径、存储抽象）。（报告 1/3/4/7）
2. **P0 数据通路与安全绝对优先**：cluster_id 接线、k8s 鉴权 TLS、审批门禁下沉、消息不丢，先于一切新功能。（报告 2/4/6）
3. **在用深度优先于适配器广度**：不新增、不扩张无消费方储备 crate；先修在用路径（RdbmsClient、etcd、三服务直接依赖），再谈储备。（报告 7）
4. **契约先行**：protos 为单一事实源，引入 buf lint/breaking 与契约测试，杜绝分页声明未实现、字段恒默认值、文档路径漂移类问题。（报告 2/4）
5. **质量门禁前置**：workspace 全量 test + clippy `-D warnings` 在一切新改动前落地，使后续每项工作在门禁内进行。（报告 1/3）
6. **验收可测试**：每项任务给出可自动化的验收标准（沿用报告 1 的测试实数方法论）。

## 3. 分阶段路线图（3 阶段）

### 阶段一「数据通路与安全基线」（第 1–2 月）

- **主题**：让核心数据链路真实可用、安全边界可信、CI 成为唯一质量裁判。
- **目标**：六类周期任务命中真实集群；k8s-service 不可匿名调用；写操作不可绕过审批；审计/消息不静默丢失；CI 全绿（含前端干净构建）。
- **关键交付物**：cluster_id 接线（配置 + 任务改造 + 契约测试）；k8s-service TLS + 令牌鉴权；审批门禁统一下沉（含混沌 delete 修复）；审计本地兜底 + Kafka 手动 commit/重试；refresh token 闭环（端点/吊销/jti 黑名单）；CI 全覆盖（collector + ecat + clippy `-D warnings` + 前端 test）；版本归一；etcd 三缺陷修复；echarts 幽灵依赖修复；RdbmsClient 收敛决策（评审产出方案）。
- **涉及角色**：Lead、架构师、网关/K8s/Collector 工程师、安全工程师、平台交付、测试质量、前端工程师。
- **退出标准**：CI 全绿且含 clippy `-D warnings` 与前端干净安装构建；契约测试覆盖六类任务真实路径且首调成功率 100%；未授权访问 :9091 被拒；`approval.enabled` 下删除类操作必须经审批（回归测试）；Kafka 故障注入下审计 0 丢失；全仓单一版本号。

### 阶段二「发布就绪与可靠性加固」（第 3–4 月）

- **主题**：可交付（镜像/Helm/流水线）+ 可靠（告警降噪/幂等/游标）+ 闭环（前端体验补全）。
- **关键交付物**：三服务 Dockerfile + 镜像构建 + Helm 真值化 + 发布流水线；集群注册持久化；告警状态机降噪（去重/阈值/节点上限）；自愈冷却；logtail 游标化 + 批量写；周期任务分布式锁；前端 Pod 日志接通；认证持久化 + refresh 使用；可观测部署完善（prometheus 全 target、三服务指标端口、Grafana/Alertmanager）；1.8.0 新功能测试补全；压测纳入 CI；文档同步。
- **涉及角色**：平台交付、Collector/K8s/网关/前端工程师、数据工程师、测试质量、Lead。
- **退出标准**：一条命令从镜像发布到 smoke 通过；同一故障不再 600s 重复落库；logtail 重启后续采不重复；双 collector 实例下同一任务只执行一次；前端 Pod 日志页有真实数据；认证跨刷新存活；prometheus 抓到三服务指标。

### 阶段三「框架收敛与体验增强」（第 5–6 月）

- **主题**：偿还技术债（框架统一）+ 增强（体验/生态）。
- **关键交付物**：RdbmsClient 迁移或废弃落地（按阶段一决策）；错误体系统一（ecat-errors 落地，服务弃用 anyhow 直通）；观测层统一（trace-id 头名一致）；储备适配器质量收敛（删除或补齐）；前端类型安全整改（39 处 any）；录制回放增强（还原 ts 时序 + ANSI 渲染）；iframe sandbox 收敛；构建优化（gzip）；schema 迁移体系（migrations/ 目录，init.sql 可安全重放）；Tauri 打包入 CI；多集群告警聚合（multicloud.md 意向落地）。
- **涉及角色**：架构师、前端工程师、数据工程师、平台交付、网关/Collector 工程师、测试质量。
- **退出标准**：gateway/collector 生产路径不再直接使用 `sqlx::MySqlPool`（或决策为废弃并移除抽象层）；trace-id 全链路一致（契约测试）；无消费方 crate 有明确处置记录；前端 CI 含类型检查 + 测试；schema 变更可重放（migrations 测试）。

## 4. 优先任务清单（P0 全部 + P1 精选）

字段顺序：任务｜来源｜优先级｜工作量｜依赖｜验收标准｜角色。

### P0（12 项，按"数据通路 → 安全 → 质量 → 发布 → 框架"排序）

1. **cluster_id 接线与六类周期任务契约修复**｜报告 2/6（交叉印证）｜P0/S｜依赖：collector.yaml 配置 + 任务改造 + 契约测试。验收：真实集群注册后 collect/logtail/alert/drift/rollback/housekeeping 六类任务首调成功，契约测试覆盖，失败可观测。角色：Collector 工程师 + K8s 工程师。
2. **k8s-service 鉴权 + TLS**｜报告 2｜P0/M｜依赖：令牌/证书方案与网关集成。验收：无凭据调用 :9091 被拒；RunJob/ExecPod 必须持有效令牌；传输层加密（mTLS 或等效）。角色：K8s 工程师 + 安全工程师。
3. **混沌 delete 审批门禁修复**｜报告 4｜P0/S｜无。验收：`approval.enabled` 下 chaos delete 必须经 `is_delete_approved`；回归测试覆盖 chaos 路径（当前 `chaos_api.rs:150` 直连绕过）。角色：网关工程师。
4. **审计本地兜底**｜报告 4｜P0/M｜依赖：审计存储方案。验收：`mq=None` 时写操作审计落本地存储；故障注入下审计 0 丢失。角色：网关工程师 + 数据工程师。
5. **消息可靠投递（手动 commit + 重试）**｜报告 6｜P0/M｜依赖：Kafka 消费重构。验收：进程重启不丢审计（`enable.auto.commit=false` 下手动 commit）；CH 写失败有重试/死信路径而非 warn 丢弃；mpsc(1024) 溢出有背压或降级。角色：Collector 工程师 + 数据工程师。
6. **refresh token 闭环**｜报告 4｜P0/M｜无。验收：`/api/auth/refresh` 可用；refresh_token 可吊销；jti 黑名单生效；无端点签发即死的路径。角色：网关工程师 + 安全工程师。
7. **CI 全覆盖门禁**｜报告 1/3（交叉印证）｜P0/M｜依赖：清理死 CI（ecat-deploy 嵌套、.gitlab-ci.yml）。验收：workspace 全量 fmt/test + clippy `-D warnings` + 前端 `npm ci` 后 tsc/build/test 全绿；collector 与 58 个 ecat crate 进 CI。角色：平台交付 + 测试质量。
8. **echarts 幽灵依赖修复**｜报告 5｜P0/S｜无。验收：frontend/package.json 声明 echarts；干净 clone + `npm ci` + tsc 通过。角色：前端工程师。
9. **版本号归一**｜报告 1/3（交叉印证）｜P0/XS｜无。验收：Cargo.toml 为唯一真值；README/tauri/audit 同步或指向真值；脚本校验一致性。角色：Lead + 平台交付。
10. **服务镜像化 + 发布流水线**｜报告 3｜P0/M｜依赖：CI 门禁（7）、版本归一（9）。验收：三服务 Dockerfile 构建成功；Helm values 无 `registry.example.com` 占位；流水线产出可部署镜像并可回滚。角色：平台交付。
11. **ecat-registry-etcd 三缺陷修复**｜报告 7｜P0/S｜无。验收：lease 续约保活（30s 不静默过期）；`list_services` 前缀正确命中；deregister 只删本实例（实例维度 key）。角色：架构师 + K8s 工程师。
12. **RdbmsClient 收敛决策**｜报告 7｜P0/L（决策先行，实施在阶段三）｜依赖：梳理 22 个直连文件（gateway 18 / collector 4）。验收：评审产出"迁移或废弃"明确方案；22 文件处置清单与影响面（含 k8s 完全未用）评审通过。角色：架构师 + Lead。

### P1 精选（18 项，按"可靠性 → 体验 → 质量"排序）

1. **集群注册持久化**｜报告 2/3｜P1/L｜依赖：ecat-data-sqlx 预埋依赖、config.rs "Phase 2" 注释点。验收：服务重启后集群仍存在；AddCluster/RemoveCluster 有审计记录。角色：K8s 工程师 + 数据工程师。
2. **k8s 契约/实现漂移修复**｜报告 2｜P1/M｜依赖：契约先行规范。验收：分页声明与实现一致；`resource_types` 生效；Cluster/Pod/Deployment/Node 消息字段非恒默认值；WatchEvent.payload/LogLine.timestamp 有值；GetMetrics 返回真实数据。角色：K8s 工程师。
3. **流式连接健壮性**｜报告 2｜P1/M｜无。验收：客户端断开后服务端释放资源；取消/超时/错误帧处理有测试。角色：K8s 工程师。
4. **限流键可信化**｜报告 4/7（交叉印证）｜P1/S｜无。验收：限流键不再信任可伪造 XFF 首 IP；伪造头测试被限。角色：网关工程师 + 安全工程师。
5. **exec resize 实现**｜报告 4｜P1/S｜无。验收：resize 消息生效，`terminal_size` 不再写死 None。角色：网关工程师。
6. **告警降噪状态机**｜报告 6｜P1/M｜依赖：P0-1（cluster_id 接线）。验收：同一故障不重复落库；`alert_consecutive` 配置生效；节点数上限生效；故障风暴场景有测试。角色：Collector 工程师。
7. **自愈冷却**｜报告 6｜P1/S｜依赖：P0-1。验收：scale 动作有冷却期与上限；回滚振荡场景有测试。角色：Collector 工程师。
8. **logtail 游标化 + 批量**｜报告 6｜P1/M｜依赖：P0-1。验收：重启续采不重复写 CH/ES；批量写生效；单 Pod 失败不中断整轮。角色：Collector 工程师 + 数据工程师。
9. **周期任务锁与幂等**｜报告 6｜P1/S｜无。验收：双实例下 drift/rollback/housekeeping 只执行一次；巡检 Redis 锁续租（TTL 300s 内续租）。角色：Collector 工程师。
10. **前端 Pod 日志接通**｜报告 5｜P1/M｜依赖：后端接口已存在（当前 pod-logs.tsx 为 19 行无数据死组件）。验收：Pod 日志页展示真实数据，xterm 渲染。角色：前端工程师 + 网关工程师。
11. **前端认证持久化 + 刷新**｜报告 5｜P1/M｜依赖：P0-6（refresh 闭环）。验收：刷新页面不登出；401 自动续期或明确重登路径。角色：前端工程师。
12. **前端类型安全整改**｜报告 5｜P1/M｜无。验收：39 处 any 归零或收敛到边界；tsc 严格模式通过。角色：前端工程师。
13. **补 1.8.0 新功能测试**｜报告 1｜P1/L｜依赖：P0-1。验收：logtail/rollback/drift/notify/mq/domain_events/main 均有测试；collector 测试数 57 → 100+。角色：测试质量。
14. **k8s 集群管理补测**｜报告 1｜P1/M｜无。验收：`cluster/{manager,client}.rs`、`resource/{pod,deploy,node,metrics}.rs` 覆盖显著提升（k8s 14 → 40+）。角色：测试质量 + K8s 工程师。
15. **可观测部署完善**｜报告 3｜P1/M｜依赖：P0-7。验收：prometheus 抓取三服务指标（补 k8s/collector 指标端口）；Grafana/Alertmanager 配置就绪。角色：平台交付 + 数据工程师。
16. **压测纳入 CI**｜报告 3｜P1/S｜无。验收：bench 有 README；ecat-bench 在 CI 中以 smoke 模式运行。角色：平台交付 + 测试质量。
17. **统一观测层 + 错误体系**｜报告 7｜P1/M（观测）/L（错误）｜依赖：架构评审。验收：trace-id 头名全链路一致（x-ecat-trace-id vs x-trace-id 二选一）；服务错误迁移至 ecat-errors 定义类型，适配器 *Error 不再全 String。角色：架构师。
18. **文档同步 + README 数字**｜报告 1/3/4｜P1/M｜依赖：各领域修复后同步。验收：README 测试数 = 实测值；文件存储介质（MinIO vs 本地盘）、混沌 action、metrics 路径、ticket/runbook 单条 GET、tenant 头行为等描述与代码一致。角色：各工程师 + 测试质量。

## 5. 跨领域依赖与执行顺序（关键路径）

- **关键路径 A（数据通路）**：cluster_id 接线 → 六类周期任务真实命中 → 告警降噪状态机（否则噪音淹没信号）→ 自愈冷却与回滚防振荡。整条链路共享契约测试。（报告 2/6）
- **关键路径 B（安全基线）**：k8s-service 鉴权 TLS → 审批门禁统一下沉（混沌 delete 修复依赖统一审批入口，避免各自打补丁）→ refresh 闭环；限流键可信化可与审批并行。（报告 2/4）
- **关键路径 C（质量 → 发布）**：CI 全覆盖 + echarts 修复（前端 CI 变绿）→ 版本归一 → Dockerfile/Helm 真值化 → 发布流水线。发布红线以 C 全绿为前提。（报告 1/3/5）
- **关键路径 D（框架）**：RdbmsClient 决策先行（影响 22 文件，决策结果决定 gateway/collector 改造范围）→ etcd 修复（注册发现可用）→ 观测/错误体系统一。（报告 7）
- **关键路径 E（体验）**：P0 refresh 闭环 → 前端认证持久化 → Pod 日志接通（后端接口已存在，纯前端工作）→ 录制回放增强。（报告 4/5）
- **执行顺序总则**：决策类与 XS/S 快赢（版本归一、echarts、etcd、混沌修复）第 1 周即动；P0 数据通路与安全并行开工但共享契约测试；CI 门禁先落地再动大改；发布流水线依赖门禁与版本归一，安排在阶段二开头。

## 6. 风险登记册（Top 10）

| # | 风险 | 影响 | 概率 | 缓解 | 置信 |
|---|------|------|------|------|------|
| R1 | collector 六类周期任务全链路失效（cluster_id 未接线） | 高：采集/巡检/告警/回滚空转 | 高（首调即失败） | P0-1 接线 + 契约测试 | **高置信（报告 2/6 交叉印证）** |
| R2 | k8s-service 任意代码执行（无鉴权无 TLS） | 灾难：RunJob/ExecPod 直达 | 中（内网可达） | TLS + 令牌 + 审批 + 网络隔离 | 报告 2 |
| R3 | 混沌 delete 绕过审批门禁 | 高：可删任意 deployment | 中 | 审批统一下沉 + 回归测试 | 报告 4 |
| R4 | 审计/消息静默丢失 | 高：合规与可观测失真 | 高（重启即丢） | 本地兜底 + 手动 commit + 重试 | 报告 4/6 |
| R5 | 前端干净环境 CI 必红（echarts 幽灵依赖） | 中高：阻断合并与发布 | 高（条件已复现） | 声明依赖 + 前端 CI 全量 | 报告 5 |
| R6 | 认证/鉴权回归 | 灾难：网关唯一入口（历史发生一次） | 低 | 审批下沉 + 回归测试 + 门禁 | 报告 4 |
| R7 | 框架抽象腐烂（RdbmsClient 绕过、etcd 缺陷、文档失真） | 中：技术债滚雪球 | 高 | 收敛决策 + 在用深度优先 | 报告 7 |
| R8 | 文档/契约漂移持续（README、audit、OpenAPI 五处失真） | 低中：误导决策 | 高 | buf lint + 文档入 PR 检查 + 数字校验脚本 | **高置信（报告 1/2/3/4/7 多处）** |
| R9 | 多实例不一致（API key 内存表、Redis 计数回退、周期任务重复） | 中 | 中 | 持久化 + 分布式锁 + 共享状态 | 报告 4/6 |
| R10 | 告警风暴（600s 重复落库、阈值 1 无上限） | 中：通道淹没 | 高 | 状态机降噪 + 去重 + 上限 | 报告 6 |

## 7. 质量与度量

### 门禁清单（全部可脚本化执行）

1. `cargo fmt --check`（workspace 全量）
2. `cargo clippy --workspace --all-targets -- -D warnings`（取代"本地零警告、CI 不执行"）
3. `cargo test --workspace`（含集成测试）
4. 前端：干净 `npm ci` → `tsc --noEmit` → 测试 → `build`
5. protos 变更 PR：`buf lint` + `buf breaking`
6. `cargo machete` 无用依赖检查（报告 7 建议）
7. README 测试计数与 `cargo test` 实测数一致性校验脚本（杜绝再次滞后 12 项）

### 测试目标（基线 420）

- 阶段一 ≥ 520：cluster_id 契约、k8s 鉴权、审批门禁回归、审计兜底、refresh 流程。
- 阶段二 ≥ 650：collector 新功能（57 → 100+）、k8s 管理（14 → 40+）、前端从 0 起步 20+ 用例。
- 阶段三 ≥ 720 且前端组件覆盖率 ≥ 40%、类型检查全绿。

### 发布红线（Go/No-Go）

- CI 全绿（含 clippy `-D warnings`、前端干净构建与测试）；
- 全仓单一版本号（脚本校验通过）；
- 镜像可复现构建、Helm 一键部署 smoke < 5 min；
- 审批门禁回归通过（approval.enabled 下删除必经审批）；
- 审计故障注入 0 丢失；k8s-service 未授权访问被拒；
- 关键指标：CI 时长 < 15 min；六类任务首调成功率 100%。

## 8. 团队分工矩阵（角色 × 阶段）

| 角色 | 阶段一 | 阶段二 | 阶段三 |
|------|--------|--------|--------|
| Lead | 版本归一、优先级仲裁、阶段退出评审 | 发布红线把关、里程碑验收 | 处置评审、规划滚动 |
| 架构师 | RdbmsClient 决策、etcd 修复、契约先行规范 | 观测/错误统一方案 | 迁移落地、储备适配器处置 |
| 网关工程师 | 审批下沉、审计兜底、refresh 闭环、限流键、exec resize | 文档同步、服务发现持续更新 | 错误体系接入 |
| K8s 工程师 | 鉴权 TLS、cluster_id 契约配合 | 集群持久化、漂移修复、流式健壮性 | 多集群告警聚合配合 |
| Collector 工程师 | cluster_id 接线、消息可靠投递 | 告警降噪、自愈冷却、logtail 游标、分布式锁 | 回滚防振荡、cron 调度评估 |
| 前端工程师 | echarts 修复 | Pod 日志、认证持久化 | 类型整改、录制回放、CSP/iframe 收敛、构建优化 |
| 数据工程师 | 审计存储方案、CH 可靠性 | ES 批量、Grafana/指标模型 | schema 迁移体系 |
| 测试质量 | CI 门禁搭建、审批/鉴权回归 | 1.8.0 功能测试、k8s 补测、压测入 CI | 前端测试体系、故障注入 |
| 安全工程师 | k8s 鉴权方案、限流键、审批回归 | 安全评审 | iframe/CSP 收敛评审 |
| 平台交付 | CI 重构、死 CI 清理 | Dockerfile/Helm/流水线、可观测部署 | Tauri 打包入 CI |

## 9. 前 4–6 周冲刺建议（按周拆解）

- **第 1 周「基线锁定」**：版本归一（四口径 → Cargo.toml 单一真值）；CI 全覆盖（collector + ecat + clippy `-D warnings` + 前端 test）；echarts 依赖声明修复；删除死 CI（ecat-deploy 嵌套、.gitlab-ci.yml）；README 测试数同步。验收：CI 全绿。角色：Lead、平台交付、前端工程师、测试质量。
- **第 2 周「数据通路主攻」**：cluster_id 接线（collector.yaml 增加真实集群配置 + 六类任务改造 + 契约测试）；并行完成 k8s 鉴权方案评审定稿。验收：六类任务契约测试首轮通过。角色：Collector 工程师、K8s 工程师、安全工程师。
- **第 3 周「安全基线实施」**：k8s-service TLS + 令牌鉴权落地；混沌 delete 审批修复 + 回归测试（红 → 绿）。验收：未授权访问被拒；绕过用例回归通过。角色：K8s 工程师、网关工程师、安全工程师。
- **第 4 周「消息与审计」**：审计本地兜底（mq=None 落本地）；Kafka 手动 commit + 重试 + CH 失败降级。验收：故障注入下审计 0 丢失、重启不丢。角色：网关工程师、Collector 工程师、数据工程师。
- **第 5 周「认证闭环 + 框架快赢」**：refresh 端点 + 吊销 + jti 黑名单；限流键可信化；etcd 三缺陷修复（续约/前缀/实例级 deregister）。验收：refresh 流程测试通过、etcd 注册跨 30s 存活。角色：网关工程师、安全工程师、架构师。
- **第 6 周「收口与测试」**：RdbmsClient 决策评审（22 文件处置清单）；1.8.0 核心功能测试首批（logtail/rollback/drift）；前端测试脚手架 + 首批用例。验收：阶段一 P0 全部闭环或明确 in-flight，CI 全绿可作发布基线。角色：架构师、测试质量、前端工程师、Lead。

**阶段一出口检查**：若第 6 周末 CI 全绿、六类任务首调 100%、未授权访问被拒、审批不可绕过、审计 0 丢失、版本单一，即可进入阶段二（发布流水线与可靠性加固）。

---

## 附录：侦察事实来源（7 份领域报告要点）

| 报告 | 领域 | 核心事实 |
|---|---|---|
| 1 | 质量横切 | 测试实数 420 与 README 408 不符；CI 仅覆盖 gateway+k8s、clippy 未进 CI；1.8.0 新功能与前端测试真空；生产 unwrap ≈30 |
| 2 | K8s 服务与契约 | cluster_id 空串链路断裂（P0）；9091 无鉴权无 TLS（P0）；GetMetrics 占位、契约字段漂移；集群持久化已规划未做 |
| 3 | 平台交付与文档 | 三服务无 Dockerfile、Helm 占位（P0）；版本四套口径（P0）；prometheus 仅抓 gateway；init.sql 不可重放 |
| 4 | API 网关 | 混沌 delete 绕过审批（P0）；审计 MQ 单通道静默丢（P0）；refresh token 签发即死（P0）；exec resize 占位；XFF 限流可伪造 |
| 5 | 前端 | echarts 幽灵依赖 CI 必红（P0）；前端零测试（P0）；Pod 日志死组件；Tauri CSP 无 ws://；39 处 any |
| 6 | Collector | cluster_id 空串（P0，交叉印证）；Kafka 无手动 commit 丢消息（P0）；alert_consecutive 未接线；logtail 无游标重复写；自愈无冷却 |
| 7 | ecat 框架层 | RdbmsClient 被 22 文件绕过（P0）；etcd 三缺陷（P0）；memcached 假实现；三份 TracingLayer 并存；约 19 个储备 crate 无消费方 |
