# Changelog

## [1.9.1] — 2026-09-26 — CI 转绿：补 protoc、C++ 工具链、clippy 1.98 生成代码放行（均为既有缺陷）

> v1.9.0 发布后发现 CI 五个 job 中三个红（`rust` check/test、`clippy`、`images`），根因均与 v1.9.0 改动无关 —— 是 CI/Docker 配置长期缺依赖，以及 CI 浮动 `stable` 升到 1.98 后新 lint 命中零改动代码。本版一并补齐。

### Fixed
- **clippy 1.98 的 `result_large_err`（P1，`clippy` job 红）**：CI 的 `stable` 已到 **1.98.1**（cargo 0.99.0），本地为 1.97.1 —— 该 lint 对本项目零改动代码在 1.98 才生效，故本地始终绿。两处根因分开处置：
  - **生成代码 16 处**：全部在 `superops-protos` 的 OUT_DIR 文件 `k8s.v1.rs` —— tonic 生成的 service trait 一律返回 `Result<Response<T>, tonic::Status>`，`Status` ≥176 字节，非本仓库代码 → 在 `include_proto!` 所在模块加 `#[allow(clippy::result_large_err)]`，只放行生成代码
  - **手写代码 3 处**：`services/gateway/src/auth/middleware.rs` 的 `auth_middleware` / `require_role` / 测试用 `inject_claims`，签名 `Result<Response, Response>`（≥128 字节）由 axum 中间件约定（错误类型须实现 `IntoResponse`，无法按 clippy 建议装箱）→ 逐函数加 `#[allow]`，其余代码仍受该 lint 约束
- **CI 缺 `protoc`（P1，`rust` 与 `clippy` job 长期红）**：`ecat-protos/build.rs` 与 `superops-protos/build.rs` 均调用 `tonic_build::compile_protos(...).unwrap()`，而 workflow 从未安装 `protobuf-compiler`，构建脚本 panic 于 `Could not find protoc`。本地因 `~/.local/bin/protoc`（28.3）存在而从不复现 —— 属「本地绿 ≠ CI 绿」的典型。修复：两个 Rust job 各加一步 `apt-get install -y protobuf-compiler`（`bench-smoke` 依赖图中无 protos，保持不动）
- **Docker 构建缺 C++ 编译器（P1，`images` job 红）**：gateway 依赖 `rdkafka-sys`，其 build script 探测结果为 `checking for gcc (by command)... ok` 但 `checking for C++ compiler (g++)... failed (fail)` —— `rust:1.88-slim` 自带 gcc 没有 g++，librdkafka 构建中止。修复：三个 Dockerfile 的 builder 阶段补 `build-essential`（runtime 阶段不变，镜像体积不受影响）

### Verified
- 本地 `docker build` 至 `Step 3/12` 实测装上 `build-essential`（`Setting up g++ (4:12.2.0-3)`）——正是 v1.9.0 失败的那一步；后续 `cargo build` 因容器内无法访问 `static.rust-lang.org`（rustup 解析 `rust-toolchain.toml` 的 `stable` 通道超时；宿主可达、容器网络受限）未能跑完，**镜像构建的最终验证以 CI `images` job 为准**
- workspace 444 项测试全过；`cargo fmt --check` 通过
- **`cargo +1.98 clippy --workspace --all-targets -- -D warnings` exit 0**（本地额外装 1.98.1 工具链复现 CI 版本后实测；1.97.1 复现不出该 lint）
- CI 五个 job 全绿（`rust` / `clippy` / `frontend` / `images` / `bench-smoke`）

## [1.9.0] — 2026-09-26 — 项目宠物「超猫 SuperCat」+ 文档对齐代码现状

### Added
- **项目宠物「超猫 SuperCat」**：三张 SVG 沿用架构图集配色 —— `docs/images/pet.svg`（设定图：耳尖信号波/扫描眼/折线尾巴/项圈 LED/吊牌时钟/胸前终端 六处特征↔平台能力引线注解）、`pet-states.svg`（健康/降级/熔断三态与表情）、`pet-icon.svg`（图标版）
- **超猫进入前端代码**：`frontend/src/components/super-pet.tsx`（内联 SVG 组件，`state = ok | degraded | alert` 驱动耳形、眼神与项圈 LED）接入登录页 Logo、侧边栏 Logo、favicon（`frontend/public/pet-icon.svg`）；Tauri 桌面应用图标 32/128/256/512 全部替换为超猫
- **总览页「超猫值守」卡片**：按后端未确认告警级别实时切换三态（`CRIT*` → 熔断红、`WARN*` → 降级黄、无告警 → 健康绿）
- **`AlertRow` 消费接线**：`alertApi.listAlerts` / `listAcks` 在总览页落地

### Changed
- **README 重排（中英同步）**：超猫设定图置顶（替换原 96px 小图标），新增「项目宠物 / Project Mascot」章节并**前移至项目说明之前**，含「身体特征 ↔ 平台能力 ↔ 代码位置」三列映射表与代码落地清单
- **架构图集补齐分组标题**：架构设计（flow / design）、功能设计（structure）、安全防护（security）、生命周期（lifecycle）
- **文档对齐阶段二成果**：集群注册 MySQL 持久化、服务间 gRPC Bearer 鉴权、k8s `GetMetrics`、gRPC TLS、logtail Redis 游标、周期任务分布式锁、告警降噪（`alert_consecutive` + open 去重）、自愈冷却与副本上限、CI 四条流水线（含镜像构建与 bench 冒烟）——README 功能表 / 已知边界 / 测试与 CI / 项目结构四处
- **「活跃告警」指标卡换真实口径**：原先复用审计事件数（占位），改为未确认告警数（与超猫状态同源）
- **版本归一 1.9.0**：workspace `version`、frontend `package.json`（含 `package-lock.json`）、Tauri `version`、Helm `version` / `appVersion` / `image.tag` 五处同步
- Helm `NOTES.txt` 镜像版本改为读 `.Values.image`，三个 Dockerfile 构建示例 tag 改为 `:dev` —— 原先硬编码版本号，每次发版都会滞后

### Fixed
- **前端 `tsc` 构建失败（P1，CI 阻塞）**：`frontend/src/services/api.test.ts` 的 `as Response` 因 TS 结构比较收紧报 TS2352，`npm run build`（`tsc && vite build`）在第一步即失败；改为 `as unknown as Response`

### Verified
- workspace 444 项测试全过（`cargo test --workspace` exit 0；本轮未改动任何 `.rs` 源码，与 1.8.10 同基线）；`cargo fmt --check` 通过；clippy `--all-targets -- -D warnings` 零警告
- 前端 vitest 8/8 通过；`npm run build`（tsc + vite）通过，vendor-antd 787.5 kB（分包策略不变）
- SVG 全部经 `rsvg-convert` 实际渲染核对（非仅语法检查）

## [1.8.10] — 2026-08-16 — 阶段二：自愈冷却 + 周期任务分布式锁

### Added
- **自愈冷却（P1）**：`SelfhealConfig` 新增 `cooldown_secs`（默认 300，Redis `selfheal:cooldown:*` 冷却键，冷却期内同 deployment 不重复 restart/scale）与 `max_replicas`（默认 50，scale 副本上限防无限扩容）；Redis 不可用降级执行
- **周期任务分布式锁（P1）**：`with_task_lock`（key 参数化）抽出；drift/rollback/housekeeping 接入 Redis 锁（`superops:drift|rollback|housekeeping:lock`），多 collector 实例下只执行一次（防重复漂移告警/双重回滚/重复备份+剪枝竞态）

### Verified
- 新测试 ×1（task lock 常量）；workspace 443 → 444 全过；clippy `-D warnings` 零警告；fmt 通过

## [1.8.9] — 2026-08-16 — 阶段二：告警状态机降噪（alert_consecutive 接线 + open 去重 + 节点告警上限）

### Added
- **`alert_consecutive` 接线（P1，此前配置字段从未使用）**：`streak_progress` 纯函数 + `filter_by_streak`（Redis `alert:streak:*` 计数，连续 N 个周期超标才告警，TTL 超时自动重置；Redis 不可用降级全放行）
- **告警 open 去重**：写入 ClickHouse 前查 Redis `alert:open:*`，同一目标在 open 窗口内不重复落库/通知（TTL 自动过期 ≈ 恢复清除）
- **节点告警风暴上限**：`evaluate_rules` 增加 `max_not_ready` 参数，node_not_ready 告警超限保留前 N 条 + 汇总一条（原规则引擎不生效，故障时逐节点轰炸）
- 通知/自愈路径重构：`notify_alerts` 提取，去重后的告警才触发通知与自愈

### Verified
- 新测试 ×2（streak_progress 连续阈值 / evaluate_rules 节点告警截断）；workspace 441 → 443 全过；clippy `-D warnings` 零警告；fmt 通过

## [1.8.8] — 2026-08-16 — 阶段二：集群注册持久化（Phase 2 persistence）

### Added
- **集群注册持久化（P1）**：k8s-service `ClusterManager` 接入 MySQL（`config.database.url`）：`AddCluster` 落库、`RemoveCluster` 删库、启动时 `load()` 恢复注册（单条 kubeconfig 解析失败仅告警跳过）；未配置 database 段时保持纯内存模式（兼容）
- `deploy/init.sql` 新增 `cluster` 表（id/name/kubeconfig/created_at）；kubeconfig 明文落库已注明生产需 at-rest 加密（后续项）
- k8s-service 新增 `sqlx` 依赖；`ClusterManager::new(None)` 内存模式单测

### Fixed
- **WAF 误伤集群管理（P0，集成验证发现）**：`POST /api/k8s/clusters` 的 kubeconfig 含内网 `server` 地址被 SSRF 规则误判 403。修复：`skip_paths` 增加 `/api/k8s/clusters`（集群管理 body 为结构化配置，非攻击 payload）

### Verified
- 集成验证（compose MySQL）：AddCluster → MySQL 落库 → 重启 k8s-service `loaded=1` → `GET /api/k8s/clusters` 恢复集群
- workspace 440 → 441 全过；clippy `-D warnings` 零警告；fmt 通过

## [1.8.7] — 2026-08-16 — 冒烟修复（WAF JWT 误伤 ×2 + init.sql release 保留字 + 审计落盘 flush）

### Fixed
- **WAF 误伤全部需认证 API（P0，compose 冒烟发现）**：`ecat-security` 头扫描把 `Authorization: Bearer <JWT>` 判定为 `jwt_attack`，所有带 token 的请求 403。修复：认证凭据头不进入扫描面（其余头照扫）；新增回归测试（凭据头跳过 / 其他头仍扫）
- **WAF 误伤 refresh/logout（P0，冒烟发现）**：refresh/logout 请求体含 JWT 被 body 扫描拦截。修复：`SecurityBodyLayer::skip_paths()` 按前缀跳过认证凭据端点（gateway 配置 refresh/logout）
- **init.sql release 表无法创建（P1，冒烟发现）**：`release` 为 MySQL 保留字，`CREATE TABLE IF NOT EXISTS release` 语法错误导致 16 表中 release 缺失。修复：表名反引号
- **审计兜底落盘不稳定（P1，测试发现）**：`append_line` 写入后未 flush，tokio 多线程下偶发丢行（20 次循环复现 ~50% 失败）。修复：write_all 后 flush（20 次全过）

### Added
- ecat-security 测试 +3（JWT body 默认检测 / skip_paths 前缀匹配 / Authorization 头不扫描）
- 三服务 Dockerfile（gateway/k8s/collector 多阶段构建）+ .dockerignore（阶段二「发布就绪」第一步）

### Verified
- compose 冒烟：refresh 轮换 200 / 旧重放 401 / logout 204 / 吊销后刷新 401 全通过；带 token API 正常且 SQLi body 仍 403；审计链路 login→Kafka→collector→ClickHouse `audit_log` 3 条落库；collector 无集群时正确跳过（cluster_id 接线生效）
- workspace 437 → 440 全过；clippy `-D warnings` 零警告；fmt 通过

## [1.8.6] — 2026-08-16 — W6 收口（RdbmsClient 收敛决策 + 前端 api 层测试）

### Changed
- **RdbmsClient 收敛决策落地（P0-12）**：服务数据层保持直连 `sqlx::mysql::MySqlPool`（强类型绑定 / 事务非 `serde_json::Value` 参数化可替代），`ecat-data` 的 `RdbmsClient` / `SqlxClient` 抽象不采纳为服务层（trait 保留为框架储备）；清理 gateway/k8s 的 `ecat-data-sqlx` 死依赖（源码零引用）
- **前端测试扩充**：新增 `services/api.test.ts` ×4（Authorization 头注入 / 401 自动登出 / 后端错误透传含 status / 204 处理），前端 vitest 3 → 7 用例

### Verified
- 前端 vitest 7/7 通过；gateway/k8s 删依赖后编译通过；workspace clippy `-D warnings` 零警告、437 测试全过

## [1.8.5] — 2026-08-16 — W5 认证闭环 + 框架快赢（refresh 闭环 / 限流键可信化 / etcd 三缺陷）

### Added
- **refresh token 闭环（P0）**：`POST /api/auth/refresh`（校验 refresh → Redis jti 黑名单检查 → 轮换签发新 access+refresh，旧 jti 立即作废防重放）+ `POST /api/auth/logout`（吊销 refresh）；登录/注册/刷新签发的 JWT 均带 `jti` claim；新增 `auth::blacklist`（Redis，`get_multiplexed_async_connection`）
- **限流键可信化（P1）**：登录限流默认取真实对端 IP（axum `ConnectInfo`，ecat-transport-http 已注入）；`rate_limit.trust_proxy` 开启时才信任 X-Forwarded-For / X-Real-IP，防 XFF 伪造绕过
- **etcd 注册中心三缺陷修复（P0）**：① lease 保活循环（原只创建不续约，30s 后注册静默过期）；② `list_services` 服务层前缀修复（原 `discover("")` 双斜杠永不命中）；③ `deregister` 改为实例级 key 精确删除（原按前缀删全部实例）

### Changed
- `ecat-transport-http`：`axum::serve` 注入 ConnectInfo（`into_make_service_with_connect_info`）

### Verified
- gateway 112 → 114（含恢复 validate_user_status 测试 ×2）、ecat-registry-etcd 2 → 4；workspace 433 → 437 全过；clippy（gateway/etcd/http-transport）`-D warnings` 零警告；fmt 通过

## [1.8.4] — 2026-08-16 — W4 消息与审计（Kafka 手动 offset commit + 审计本地兜底）

### Fixed
- **Kafka 审计消费丢消息（P0）**：`enable.auto.commit=false` 但无手动 commit，进程重启后停机期间审计消息全丢。`MessageStream` 新增 `commit()`（Kafka 后端 `store_offset` + `commit_consumer_state`，其余后端 no-op）；collector 审计消费在 CH 写入成功后才 commit（at-least-once：写失败不提交，重启重放不丢审计）
- **审计 MQ 缺失/失败静默丢弃（P0）**：gateway 写操作审计（k8s 写操作 + 登录/注册）统一收敛到 `audit::publish`：MQ 发布失败或 `mq=None` 时落本地 JSONL 兜底（`audit.fallback_dir`，默认 `data/audit/audit-YYYYMMDD.jsonl`），审计不静默丢失；删除 k8s_proxy 与 auth/handler 的重复 publish_audit 实现（此前各自 `if let Some(mq)` 静默丢弃）

### Added
- 审计兜底 JSONL 单测 ×2（UTC 日期格式 / 两行追加与内容完整）

### Verified
- gateway 110 → 112，workspace 431 → 433 全过；clippy（gateway/collector/ecat-mq/ecat-mq-kafka）`-D warnings` 零警告；fmt 通过

## [1.8.3] — 2026-08-16 — W3 安全基线（审批门禁统一 + k8s-service 服务间鉴权）

### Fixed
- **混沌 delete 绕过审批门禁（P0）**：`/api/chaos/{id}/run` 的 delete 动作此前直连 `delete_deployment`、不经 `is_delete_approved`；现与 k8s 删除路由统一走共享门禁 `check_delete_approval`（`approval.enabled` 时未审批 412 拦截，实验标记 failed）
- **审批门禁统一下沉**：删除类审批检查收敛为 `model::approval::check_delete_approval`（k8s 删除路由 + 混沌 delete 共用），target 约定 `{cluster_id}/{ns}/{name}` 防跨集群同名绕过
- **k8s-service 无鉴权（P0）**：配置 `auth.token` 时启用 Bearer 鉴权（tonic interceptor，无凭据/错误令牌 → 401 Unauthenticated）；gateway/collector 经 `BearerChannel` 透明层注入 `authorization` 头（token 为空保持明文兼容）；`config/gateway.yaml` / `k8s-service.yaml` / `collector.yaml` 增加 token 段与注释

### Added
- 审批门禁回归测试 ×3（target 格式 / 关闭直通 / 开启时无库 fail-closed）
- k8s-service 鉴权单元测试 ×3（缺令牌拒 / 匹配放行 / 错令牌与畸形令牌拒）

### Verified
- gateway 107 → 110、k8s 14 → 17，workspace 425 → 431 全过；`cargo clippy -p superops-gateway -p superops-k8s -p superops-collector --all-targets -- -D warnings` 零警告；fmt 通过

## [1.8.2] — 2026-08-16 — W2 数据通路（collector cluster_id 接线 + 契约测试）

### Fixed
- **cluster_id 数据通路修复（P0）**：collector 六类周期任务（采集 collect / 日志 logtail / 巡检 inspect / 自愈 selfheal / 漂移 drift / 容量 collect_capacity）此前以空 `cluster_id` 调用 k8s-service，`ClusterManager.get("")` 必返回 NotFound——真实集群注册后首调即失败、全链路失效。新增 `k8s.cluster_id` 配置与自动解析（未配置时经 `ListClusters` 取首个注册集群），任务统一携带真实 cluster_id；无注册集群时任务跳过本轮并告警（不崩溃、不阻断服务）
- 容量快照 `capacity_snapshot` 的 `cluster` tag 由硬编码 `"default"` 改为真实 cluster_id

### Added
- `cluster.rs` 目标集群解析模块 + 5 项契约测试（mock k8s-service gRPC：配置优先 / `ListClusters` 回退取首个 / 无集群 None / 端点不可达 Err / 取首个纯函数）
- `config/collector.yaml` k8s 段新增 `cluster_id` 配置说明（多集群巡检可显式指定）

### Verified
- collector 测试 57 → 62 全过；`cargo clippy -p superops-collector --all-targets -- -D warnings` 零警告（修复 `clippy::needless_update`×3）；workspace 测试 420 → 425

## [1.8.1] — 2026-08-16 — 工程基线（项目规划落盘 + 版本归一 + CI 全覆盖 + 前端测试脚手架）

### Added
- **下一阶段项目规划**：`docs/project-plan-2026-08.md` —— 7 个领域侦察 agent 并行勘察 + 系统架构师合成 + Lead 验收（数据通路修复 / 安全基线 / 质量门禁 / 发布就绪：P0×12 + P1×18 + 风险 Top10 + 分工矩阵 + 6 周冲刺）
- **前端测试脚手架**：vitest + `stores/auth.test.ts` 冒烟用例（`npm run test`），前端从零测试起步

### Changed
- **版本归一**：workspace `version` 1.1.6 → 1.8.0（三服务与全部 ecat-* crate 随 workspace 对齐产品版本）；Helm `appVersion` 1.2.0 → 1.8.0；Tauri `version` 0.1.0 → 1.8.0；frontend `version` → 1.8.0
- **echarts 依赖修复**：`echarts@^6.1.0` 声明进 `frontend/package.json`（此前为根 `package.json` 幽灵依赖，干净环境 `npm ci` 构建必失败）；根 `package.json` 收敛为 `{"private": true}`
- **CI 全覆盖**：`.github/workflows/ci.yml` 改为 workspace 全量 `cargo fmt --check` + `cargo check --workspace` + `cargo test --workspace` + 独立 clippy job（`--all-targets -- -D warnings`）+ 前端 `npm run test` + `npm run build`；移除永不触发的死配置（`ecat-deploy/.github`、`ecat-deploy/.gitlab-ci.yml`）
- **README 测试数同步**：408 → 420（gateway 97→107、collector 55→57），中英文同步；CI 段更新

### Fixed
- gateway `cmdb_topology.rs` 测试辅助函数触发 `clippy::too_many_arguments`（rust 1.97 下 `-D warnings` 失败）——README 原「clippy 零警告」声明在真实门禁下不成立，已修复并实测验证

### Verified
- `cargo clippy --workspace --all-targets -- -D warnings` 实测零警告；前端 `npm install` + `npm run test` 通过；README 数字与实测一致（420）

## [1.8.0] — 2026-08-07 — 生态扩展七连（图拓扑 + ES 检索 + S3 备份 + MQ 多协议 + etcd + GraphQL + 领域事件）


### Added
- **CMDB 资产拓扑（图数据库）**：gateway `cmdb_topology.rs` —— `POST /api/cmdb/topology/sync`（资产 → 图节点 / depends_on 依赖边 upsert，孤儿节点清理）、`GET /api/cmdb/topology`（节点 + 边查询）、`POST /api/cmdb/topology/explore`（原生图查询，结果上限 4KB）；图后端 provider 抽象（neo4j / nebulagraph / arangodb，gateway.yaml `graph:` 段，未配置时返回明确降级信息）；前端 `/cmdb` 新增「拓扑图」Tab（手写 SVG 圆环布局：资产类型着色条 + 状态描边 + 名称截断 + 同步按钮）
- **日志检索 ES / OpenSearch**：collector `logtail.rs` 采集时按行索引到搜索后端（collector.yaml `search:` 段，elasticsearch / opensearch，含行号 id 与时间戳字段）；gateway `logs_api.rs` 检索时按配置分流（ClickHouse 或 ES bool / term / match / range DSL），搜索后端不可用自动回退 ClickHouse（503/400 仅告警）
- **备份存储 S3 / MinIO**：gateway `backup_api.rs` —— `GET /api/backups/objects`（桶内对象键列表，gateway.yaml `storage:` 段配置后可用，ops:cmdb）；前端 `/ops/backups` 页新增「备份存储对象」卡片（provider + 对象键滚动列表）
- **更多消息协议（MQTT / NATS）**：collector `mq.rs` 多后端装配 —— `mqtt:` / `nats:` 配置段优先，缺省回退 Kafka（`mq:` 段仍为必需）；审计事件消费与领域事件发布共用该后端
- **etcd 注册中心**：collector `on_start` 支持 etcd 注册（collector.yaml `etcd:` 段，endpoints + `superops/services` 前缀 + 30s lease），未配置时回落 Consul
- **GraphQL API**：gateway 接入 ecat-graphql（`graphql.rs` schema：`health` / `cmdbStats` / `alerts` / `backups` 查询），挂载 `POST /api/graphql`（api:read）
- **领域事件（ecat-events）**：collector 告警（alert.rs）/ 配置漂移（drift.rs）/ 自动回滚（rollback.rs）三处经 EventBus 远程总线发布 `DomainEvent`（复用 MQ 后端，`domain_events.rs`）；gateway `domain_events.rs` 消费并落 ClickHouse `domain_event`；`GET /api/events`（ops:audit，event_type 过滤 + limit 钳制）；前端 `/ops/events` 领域事件页（类型过滤 + 级别着色）

### Changed
- 功能表 / API 一览 / 项目结构 / 已知边界 / 架构图集同步：新增拓扑、检索分流、备份对象、MQ 优先级、etcd、GraphQL、领域事件条目
- `docs/images/` 7 张 SVG 全部重绘：architecture / design / flow / lifecycle / security / structure / tree —— 新增图库 / 搜索 / 备份存储 / 注册中心 / 事件总线元素；内容多的区域换行并增大画布与盒边界（XML 完整性 + 溢出复验零溢出）

### Verified
- workspace 测试全过（gateway 107 / collector 57 / k8s 14 / ecat 组件）；`cargo fmt --check` / `cargo clippy --all-targets` 零警告；前端 `tsc` + `npm run build` 全绿

## [1.7.1] — 2026-08-07 — 修复轮（clippy 零警告 + 前端代码拆分 + 图集修整）

### Fixed
- workspace `cargo clippy --all-targets` 降至零警告（ecat-* 与三个服务 crate）
- `docs/images/` 图集修整：tree / structure / flow / design / architecture 五个 SVG 按真实字体度量（tree 等宽 12px/字符，其余比例 13px·CJK / 7.8px·Latin / 3.9px·空格）修复文本溢出——长行换行、盒与画布边界按需增大（tree.svg 画布 960→1012、structure.svg 盒区增高）；全部经 XML 完整性 + 溢出脚本复验零溢出；security / lifecycle 两图验证无溢出未改动
- 前端构建：React.lazy 路由级代码拆分 + Vite manualChunks 供应商分包（vendor-antd 786.5kB / index 15.3kB），构建零警告

### Verified
- workspace 408 项测试全过（0 失败）；`cargo fmt --check` / `cargo clippy --all-targets` / 前端 `tsc` + `npm run build` 全绿

## [1.7.0] — 2026-08-07 — 扩展点实施（安全纵深 + 发布闭环 + 告警精细化 + 长尾）

### Added
- **安全纵深（WAF 层）**：gateway 接入 `ecat-security` `SecurityBodyLayer`（URI + headers + body 攻击扫描，10MB 上限，High/Critical → 403，其余 → 500），置于路由最外层；新增 `security.rs` 错误→响应转换层（downcast `SecurityError`）+ 4 个攻击拦截测试（SQLi/XSS/header 注入）
- **发布回滚闭环**：`POST /api/releases/{id}/rollback` 手动回滚；collector `rollback.rs` 自动回滚（发布后观察窗口内健康检查 `ready==0` 且存在旧镜像 → 自动回滚并出审计）
- **配置漂移检测**：collector `drift.rs` 任务（CMDB 记录 vs 集群实际资源对比 → `drift_event` 告警）
- **混沌演练**：`chaos` 表 + `/api/chaos` CRUD/触发（action=kill-pod/scale，演练入审计）；前端 `/ops/chaos` 页
- **告警精细化**：通知规则支持 `levels` 过滤 + 值班人联动（匹配 oncall 排班）
- **资源配额**：`quota` 表（UNIQUE cluster_id+namespace）+ `/api/quota` upsert；前端 `/ops/quota` 页
- **Grafana 嵌入页**：`/ops/grafana`（iframe sandbox，URL 优先级：配置 → k8s NodePort → 默认）
- **ecat-bench 压测入口**：`BENCH_TARGET=health|login` 场景；**MySQL TLS**：`database.tls` 段（ecat-tls，ca_cert/client_cert/client_key/skip_verify）

### Changed
- Gateway 功能表、API 一览、已知边界、Collector/Frontend 段同步更新（新增安全扫描、回滚、配额、混沌、Grafana、漂移条目）
- 请求链路文档更新：SecurityBodyLayer → SecurityErrorToResponse → Cors → Trace → auth → tenant → role → breaker → handler

### Verified
- workspace **408 项测试全过（0 失败）**（gateway 97 / k8s 14 / collector 55 / ecat 组件）；`cargo fmt --check` / `cargo check --workspace` / `npm run build` 全绿

## [1.6.0] — 2026-08-07 — 生态缺口闭环（B 类修复 + C 类缺失域 + D 类扩展）

### Added
- **B1 告警规则引擎**：MySQL `alert_rule` 表 + `/api/alert-rules` CRUD（ops:cmdb）；collector 按规则求值（连续 N 次超标触发，`max_not_ready` 节点数限制）；前端 `/ops/alert-rules` 规则页（CRUD + 启用开关）
- **B2 追踪 UI**：前端 `/ops/traces` 链路追踪页（Jaeger UI iframe 嵌入，16686）
- **B3 终端安全管控**：exec WS 需 `?confirm=1`（`terminal.require_confirm` 默认开，缺失 400）；`terminal.max_session_secs` 超时强制断开（默认 1800s）；前端会话前 Modal.confirm
- **B4 通知通道扩展**：collector notify 支持 SMTP 邮件（`kind=email`，收件人逗号分隔多个，`SUPEROPS_SMTP_PASSWORD` 覆盖密码；smtp 段未配置时该 target 明确报错）；`email_body`/`send_email` + 2 新测试
- **C1 工单系统**：`ticket` 表 + `/api/tickets` CRUD/状态流转（open→in_progress→resolved/closed + reopen）；前端 `/ops/tickets` 页；告警中心一键建单
- **C2 Runbook 剧本**：`runbook` 表（steps JSON）+ `/api/runbooks` CRUD + `/run` 顺序执行（逐步结果）；前端 `/ops/runbooks` 页
- **C3 值班排班**：oncall 表 + `/api/oncall` CRUD；前端 `/ops/oncall` 页
- **C4 发布流水线**：k8s-service gRPC `UpdateImage`（deployment 镜像更新）+ `release` 表 + `/api/releases`；前端 `/ops/releases` 页
- **C5 DB 运维**：备份状态上报 API（`POST /api/backups/status`，agent 经 API key）+ 汇总（`/api/backups/summary`，时效统计）；前端 `/ops/backups` 页
- **C6 自愈动作**：规则 `action=restart/scale` 时 collector 执行 k8s restart/scale（`selfheal.enabled` 默认关闭、`max_actions_per_cycle` 限制）；动作入审计
- **C7 容量/成本**：`/api/capacity/summary|trend`（ClickHouse 汇聚）；前端 `/ops/capacity` 趋势页（手写 SVG）
- **D1 跨集群聚合**：`/api/k8s/aggregate`（全集群节点/Pod 健康汇总 + totals）；总览页「集群健康」卡片真实数据（每集群 Ready/Running 计数）；gateway 集群管理 7 个 stub handler 全部改为真实 gRPC 转发（list_clusters/add/get/remove、list_pods、get_pod_logs 流式、list_deployments）；修复 `list_nodes` 丢失 cluster_id 的 bug
- **D3 配置管理 UI**：`/api/config/remote/keys[/{key}]`（Consul KV CRUD，`config/superops` 前缀校验 400、Consul 不可达 502，ops:cmdb）；前端 `/ops/config` 配置中心页（ProTable + 编辑 Modal + 删除确认）
- 前端 API 层：`api.put` 方法 + `configRemoteApi`；`k8sApi.aggregate`；`AggregateCluster`/`AggregateTotals` 类型

### Changed
- 运维中心菜单新增 9 项：告警规则 / 链路追踪 / 工单系统 / Runbook 剧本 / 值班排班 / 发布流水线 / DB 备份状态 / 容量成本 / 配置中心
- Gateway 功能表、API 一览、已知边界、Collector/Frontend 段同步更新；新增 `docs/multicloud.md` 多云接入指南

### Fixed
- gateway `list_nodes` 未传 `cluster_id` 导致跨集群节点查询恒查默认集群
- gateway k8s 集群管理此前全部为 stub（新增/删除/详情返回假数据）

### Verified
- workspace 391 项测试全过（0 失败，含 config_remote 新增 `kv_validate_key_requires_prefix`、collector notify email 2 项）；`cargo fmt --check` / `cargo check --workspace` / `npm run build` 全绿

## [1.5.2] — 2026-08-07 — 前端功能补齐（生态缺口 A 类 + 多集群）

### Added
- 日志检索页 `/ops/logs`：namespace/pod/关键字 + 时间范围（RangePicker → unix 秒 from/to），结果 ProTable 展示（最多 100 条）
- 录制回放页 `/ops/recordings`：录制列表（会话 ID/开始时间/帧数）、回放 Modal（帧按 seq 排序，base64 → UTF-8 解码，播放/暂停/显示全部）、删除
- 审批中心页 `/ops/approvals`：按状态过滤、新建审批单（kind/target/reason）、pending→通过/拒绝/取消、rejected→重新打开
- 保险库页 `/ops/secrets`：列表/新建（name 正则校验）/查看（解密值 + 复制）/删除；master key 未配置时展示不可用提示（503 错误回显）
- 文件管理页 `/ops/files`：multipart 上传（Dragger）、本会话已上传快速下载、按文件名下载（后端无列表 API，未补后端）
- 多集群修复：Pods/Deployments/Nodes 独立页（`/k8s/pods|deployments|nodes`）移除 `clusterId="default"` 硬编码，新增集群选择器（`cluster-select.tsx`，默认首个集群）；ClusterDetail 内嵌页不受影响
- 前端 API 层扩展：`services/api.ts` 新增 `logApi`/`recordingApi`/`approvalApi`/`secretApi`/`fileApi`（fileApi 走原生 fetch multipart/字节流，不经 JSON 包装）

### Changed
- 运维中心菜单新增 5 项：日志检索 / 录制回放 / 审批中心 / 保险库 / 文件管理

## [1.5.1] — 2026-08-07 — 复审修复（审计报告 §8.5 闭环）

### Fixed
- 录制回放定序：recorder.rs 会话级帧序号（`bump_seq`，写入前取号），`exec_session` 新增 `seq` 列，回放 `ORDER BY timestamp, seq`；`GET /api/recordings/{sid}/frames` 返回 `seq` 字段
- 录制开关：config 新增 `recording.enabled`（默认 true 保持旧行为，`config/gateway.yaml` 同步）
- 多租户严格化：`x-tenant-id` 存在但畸形（非 UTF-8/非法字符/超长）→ 400，缺失头仍回落 `default`；`users.tenant_id` 列接线（User/UserRow/list_users/create，注册恒 `'default'`），移除死列
- 保险库加固：vault.rs `cipher()` 主密钥 32 字节长度校验（`Key::from_slice` panic → Err，503 两态语义不变）；SecretRow 手动 Debug 密文打码（`[redacted]`）
- housekeeping：`parse_cpu_cores`/`parse_mem_gib` 非有限值（NaN/Inf）守卫 → 0.0，杜绝静默污染 capacity 汇总；备份目录不可读 → warn 而非静默；mysqldump 管道失败删除半成品 `.sql.gz`
- 文件拆分合规：main.rs 602→186 行（路由组迁至 routes.rs 374 行）、k8s_proxy.rs 548→418 行（+k8s_exec.rs 139 行）；`create_dir_all` 启动期 fail-soft（创建失败仅 warn 降级）
- 验证：workspace **361 项测试全过（0 失败）**、`cargo fmt --check`/`cargo check`/`npm run build` 全绿

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
