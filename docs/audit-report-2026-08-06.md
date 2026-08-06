# SuperOps Phase 1 MVP — 审查报告

- 日期：2026-08-06
- 范围：全系统端到端测试、安全防护核查、生态配置核查、代码质量审查
- 结论：**MVP 可用；两轮审查发现的所有问题已修复并验证，无遗留阻塞项**

---

## 1. 运行状态

| 组件 | 地址 | 状态 |
|---|---|---|
| gateway (superops-gateway 1.0.2) | 0.0.0.0:8080 (HTTP) / 9090 (gRPC) | ✅ 运行 |
| k8s service (superops-k8s 1.0.2) | 0.0.0.0:9091 (gRPC) | ✅ 运行 |
| frontend (vite) | localhost:3000 | ✅ 运行 |
| MySQL (docker) | localhost:3307 | ✅ 运行 |
| Redis (docker) | localhost:6380 | ✅ 运行 |
| ClickHouse (docker) | localhost:8124 / 9001 | ✅ 运行 |

测试账号（本地开发库）：`erik / Test1234!`

## 2. 端到端功能测试 — 21/21 通过

覆盖：health、login、register、全部 `/api/k8s/*` 路由（正常路径 + 错误路径）。
错误路径验证：重复注册 409、错误密码 401、非法 JSON 400、超长 username 400、缺失字段 400、未注册用户 401、404 路由、405 方法不允许。

## 3. 安全防护 — 已修复并验证

### 3.1 已修复（重新验证通过，15/15 项）

| 级别 | 问题 | 修复 | 验证 |
|---|---|---|---|
| **CRITICAL** | 认证中间件只检查 `Bearer ` 前缀，**不验证 JWT**（middleware.rs 注释自述 "skip actual verification and just log"），且未挂载到任何路由 → 全部 k8s 接口无认证 | 重写 `auth_middleware`：`State` 取 `AuthState` → `verify_token()` 真实验签 → Claims 注入 `extensions`；用 `from_fn_with_state` 挂载到 k8s 全部 9 条路由（main.rs） | 无 token → 401；伪造 token → 401；篡改 token → 401；合法 token → 200（clusters/pods/exec） |
| **HIGH** | `jwt_secret` 硬编码在 `config/gateway.yaml`（"change-me-in-production"） | 新增 `SUPEROPS_JWT_SECRET` 环境变量覆盖 + 启动告警 | 环境变量注入生效；默认密钥时输出 WARN |
| **HIGH** | 注册/登录无输入校验（长度、字符集、email 格式） | `validate_credentials`：username 3-32（字母/数字/_/-）、email 5-254 含 @、password 8-72 | 非法输入 → 400 + 明确错误信息 |
| **HIGH** | handler 多处 `.unwrap()` 可 panic（bcrypt hash、create_token） | 全部改为 Result 映射：500 签发失败 / 401 凭据错误 | 正常与错误路径均稳定返回 |
| **MEDIUM** | CORS 全开（`Any`/`Any`/`Any`） | 白名单：`http://localhost:3000`、`http://tauri.localhost`、`tauri://localhost`；方法 GET/POST/DELETE；头 Authorization/Content-Type | 白名单 origin 回显 `allow-origin`；恶意 origin 无 ACAO 头（拦截） |
| **MEDIUM** | login 无速率限制 | `RateLimiter`（内存固定窗口 10 次/60 秒/IP，x-forwarded-for 取值）中间件挂载 login | 连续请求：`401×9 → 429×3`，窗口内总量精确 10 次；429 返回 JSON 文案 |
| **MEDIUM** | 前端 zustand persist 将 token 存 localStorage（XSS 可窃取） | 移除 persist，token 仅存内存；接口不变（token/username/isAuthenticated/login/logout） | 刷新后需重新登录；api.ts 注入逻辑不变 |
| **MEDIUM** | Tauri `csp: null` | 生产 `csp`：`default-src 'self'` + `connect-src` 仅 gateway:8080；`devCsp` 额外放开 vite 3000 与 HMR ws | 打包产物加载正常，外联请求被 CSP 拦截 |
| **MEDIUM** | `frontend/src/pages/k8s/terminal.tsx` 无 useEffect cleanup，WS/terminal 实例泄漏 | unmount 关闭 ws + dispose term；connect 前先清理旧实例；onResize listener 在 onclose/onerror 移除 | 多次进入/退出终端页面无泄漏、无重复连接 |
| **LOW** | docker-compose 内联 MySQL root 密码 | 全部密码变量化 `${VAR:-default}`；新增 `.env.example` 模板 + 注释说明 `SUPEROPS_JWT_SECRET`/`GATEWAY_CONFIG`/`K8S_CONFIG` 用法 | `docker compose config --quiet` 通过；模板无真实密钥 |
| **LOW** | compose 无 healthcheck（尤其 clickhouse） | MySQL/Redis 已有 healthcheck；ClickHouse 补齐 `clickhouse-client SELECT 1`（interval 5s, retries 5） | compose 启动顺序有依赖保障 |

### 3.2 原有良好实践（确认无问题）

- ✅ 密码 bcrypt（cost 默认）存储，登录 `bcrypt::verify`
- ✅ SQL 全部 sqlx 参数化绑定（`?`），无字符串拼接注入
- ✅ 登录失败统一 "invalid credentials"，不枚举用户存在性；注册 409 已收敛为通用文案
- ✅ JWT Claims 含 exp/iat；HS256 对称密钥服务端持有
- ✅ `/api/health`、`/api/auth/*` 保持公开，其余路由默认受保护

### 3.3 未修复（Phase 2 规划范围，不影响 Phase 1 验收）

| 级别 | 问题 | 建议 |
|---|---|---|
| MEDIUM | `exec` 接口返回 JSON placeholder 而非 WebSocket 101 | Phase 2 随 gRPC proxy 实现 |
| MEDIUM | 限流器为单实例内存桶（无 Redis 共享） | 多实例部署时换 Redis 计数或 tower-governor |
| LOW | 前端 exec WS URL 与后端占位接口不匹配 | 随 gRPC proxy 一并落地 |

## 4. 代码质量审查 — 已修复

| 位置 | 问题 | 级别 | 修复 |
|---|---|---|---|
| `services/k8s/src/service.rs` (~L205) | watcher Apply 事件全部标记为 "MODIFIED"，首次观察应为 "ADDED" | LOW | seen-HashSet 区分首次（ADDED）与后续（MODIFIED） |
| `services/k8s/src/resource/pod.rs` (L57-61) | log/watch stream 在 `tx.send` 失败后未 `break`，客户端断开后仍持续读 | LOW | 两处均改为 `is_err() → break` |
| `frontend/src/services/k8s.ts` | `addCluster` 期望 `{cluster}`，后端返回顶层 `{id,name,status}`，类型不一致 | MEDIUM | 类型对齐 `{ id, name, status }`；前后端实测一致（`{"id":"pending","name":"prod-1","status":"connected"}`） |
| `frontend/src/pages/k8s/pods.tsx` 等 | 硬编码 cluster id `'default'` | LOW | cluster-detail 通过 prop 传真实 `clusterId`，queryKey 含 clusterId |
| gateway 编译 | 7 个 dead-code 警告（Phase 2 字段 `endpoint`、`page/page_size` 未读） | LOW | Phase 2 字段加 `#[allow(dead_code)]` 并注明用途；两个 crate `cargo check` **零警告** |
| 前端构建 | — | — | `npm run build`（tsc + vite）通过 |
| **Task 6 gRPC proxy** | k8s 服务端与 gateway 的 gRPC 数据通道未实现（placeholder JSON） | — | **Phase 2 规划范围**，不影响 Phase 1 验收 |

## 5. 生态配置核查

### 5.1 已补齐（本轮）

- ✅ `protos/buf.yaml` + `protos/buf.gen.yaml` — `buf generate` 可用
- ✅ Makefile `dev-all`：`GATEWAY_CONFIG` / `K8S_CONFIG` 环境变量补齐
- ✅ docker-compose：移除过时 `version` 字段；端口重映射 3307/6380/8124 规避本机冲突；ClickHouse healthcheck + 全部密码变量化
- ✅ docker registry mirror（daocloud / 1ms）
- ✅ Tauri 2：`tray-icon` feature、`tauri::menu` API 修正、AppImage 方形图标 + 图标生成、`libayatana-appindicator3-dev` 依赖
- ✅ Tauri 打包产物：`.deb`、`.rpm`、`.AppImage`
- ✅ **CI 补齐**：`.gitignore` 移除 `.github` 忽略；`.github/workflows/ci.yml`（rust: fmt --check + check + test；frontend: tsc + vite build）
- ✅ **`.env.example`**：密码/密钥变量模板，无真实密钥

### 5.2 版本一致性

workspace 版本 **1.0.2**，gateway 与 k8s 服务均编译自 workspace（`version.workspace = true`），运行日志一致显示 1.0.2 —— 无版本漂移。

## 6. e-cat 框架化审计（1.2.2 追加）

全量核查三个服务（gateway / k8s / collector）是否以 e-cat 框架为基础实现。

### 6.1 已基于框架（32 处 ecat 依赖，无需迁移）

| 能力 | 框架组件 |
|---|---|
| 应用生命周期 / 传输 | `ecat::App::builder()` + ecat-transport-http / ecat-transport-grpc |
| 认证 OAuth2 | ecat-auth `OAuth2Layer` |
| 限流 | ecat-middleware `RateLimitLayer` + `RedisRateLimitStore` |
| 熔断 | ecat-circuit-breaker `CircuitBreakerLayer` |
| 配置热更新 | ecat-config-remote + ecat-config |
| 注册发现 | ecat-registry + ecat-registry-consul |
| 消息 | ecat-mq + ecat-mq-kafka |
| 存储 | ecat-data + ecat-data-sqlx / -clickhouse / -redis |
| 分布式锁 | ecat-lock `RedisLock` |
| 调度 | ecat-scheduler |
| 健康 / 指标 | ecat-health / ecat-metrics |
| API 文档 | ecat-openapi |
| 追踪 | ecat-tracing-otlp（OTLP → Jaeger） |

### 6.2 本轮迁移（1.2.2）

- **JWT 签发/校验 → ecat-auth**：框架新增 `sign_token` / `verify_token`（HS256、基于 `AuthClaims`、密钥 ≥32 字节强制校验），`JwtAuthService` 内部复用；gateway 删除自研 `Claims` 结构与 jsonwebtoken/chrono 直接依赖，`create_token`/`verify_token` 委托框架，凭证链复用框架 helpers（`extract_bearer` / `extract_query_param`），X-API-Key 与 OAuth2 统一注入框架 `AuthClaims`（此前双 claims 类型并存）
- **迁移引入问题已修复**：默认开发密钥 24 字节触发框架 `WeakKey` 拒绝签发（登录 500）→ 开发密钥提升至 ≥32 字节并同步 config.rs 哨兵检查

### 6.3 验证

- workspace 296 项测试全过（ecat-auth +6 项：签发/校验 roundtrip、extra claims、过期、篡改、弱密钥、错密钥）
- 运行时回归：login → JWT 200、伪造/缺失 token 401、API Key 创建/使用/删除/吊销 200/204/401、query token 回退 200

## 7. 结论

Phase 1 MVP 目标全部达成（44 个计划步骤已实现）。两轮审查共发现并修复：**1 项 CRITICAL 认证绕过** + 5 项安全问题 + 6 项代码质量/前端问题 + 4 项生态缺口，全部重新验证通过（e2e 21/21、安全专项 15/15、两 crate 零警告、fmt/CI/compose/前端构建全绿）。1.2.2 完成认证功能整体迁移至 ecat-auth，框架化审计确认三个服务的全部能力均已基于 e-cat 组件。剩余项目均为文档明确的 Phase 2 范围（gRPC proxy、终端 WebSocket 桥接）。

## 8. P4–P6 追加验证记录（2026-08-06）

P4（写操作/审计/用户管理）、P5（日志/CMDB/脚本库/RBAC）、P6（审批/多租户/保险库/治理/录制/文件/告警/指标/Helm）全部实现后的一次全量复核。以下为本轮新增验证内容；第 1–7 节为历史记录，保留原样。

### 8.1 新增端点清单（P4–P6 累计，均为鉴权保护）

| 阶段 | 端点 | 方法 | 角色门禁 |
|---|---|---|---|
| P4 | `/api/k8s/clusters/{id}/deployments/{ns}/{name}/scale`、`/restart` | POST | api:write |
| P4 | `/api/k8s/clusters/{id}/deployments/{ns}/{name}` | DELETE | api:write（P6 起受审批门禁） |
| P4 | `/api/audit/events` | GET | 鉴权（limit/offset/level） |
| P4 | `/api/users`、`/api/users/{id}/status` | GET / PATCH | admin |
| P5 | `/api/logs/search` | GET | api:read |
| P5 | `/api/cmdb/assets`、`/api/cmdb/stats`、`/api/cmdb/assets/{id}` | GET/POST、GET、DELETE | ops:cmdb |
| P5 | `/api/scripts`、`/api/scripts/{id}`、`/api/scripts/{id}/run`、`/api/scripts/runs` | GET/POST、DELETE、POST、GET | ops:scripts |
| P6 | `/api/approvals`、`/api/approvals/{id}/decide` | GET/POST、POST | 鉴权（门禁默认关闭） |
| P6 | `/api/secrets`、`/api/secrets/{name}` | GET/POST、GET/DELETE | 鉴权（主密钥未配置 → 503） |
| P6 | `/api/recordings`、`/api/recordings/{sid}`、`/api/recordings/{sid}/frames` | GET、DELETE、GET | api:read / api:write |
| P6 | `/api/files`、`/api/files/{name}` | POST、GET | api:write / api:read |
| P6 | `/api/alerts`、`/api/alerts/{id}/ack`、`/api/alerts/acks` | GET、POST、GET | ops:cmdb / api:write / ops:cmdb |

### 8.2 安全校验点（P6 复查）

| 校验点 | 结果 |
|---|---|
| SQL 参数化 | ✅ 全部新查询（approval/cmdb/script/secret/alert/recording）经 sqlx `?` 绑定，无字符串拼接注入（与既有实践一致） |
| 文件名白名单 | ✅ `POST /api/files` 上传文件名经白名单/规范化校验，存储路径不含用户输入 |
| 密文不泄露 | ✅ `/api/secrets` 密文值不入日志；响应不回显加密载荷之外的原始明文（master key 仅存内存） |
| 主密钥 503 两态 | ✅ `SUPEROPS_MASTER_KEY` 未设置或非 32 字节 → 全部 `/api/secrets` 返回 503，启动输出 WARN；设置后可用（两态明确，无中间态） |
| 审批门禁 | ✅ `approval.enabled` 时 `delete_deployment` 先查 `approval`（kind='delete'、status='approved'、target=`"{cid}/{ns}/{name}"`），未通过返回 412；跨集群同名 deployment 无法绕过（target 含 cluster_id） |

### 8.3 全量验证（cargo fmt --check + cargo check + cargo test + npm run build）

- `cargo fmt --check` ✅ 无 diff
- `cargo check --workspace` ✅ 通过（零错误）
- `cargo test --workspace --no-fail-fast` ✅ **361 项全过，0 失败**（≥296 基线达成；其中 P4/P5/P6 新增 gateway 审批/保险库/录制等测试与 collector 集成测试；复审新增 seq 序号、租户 400 严格化、NaN 守卫等测试）
- `cd frontend && npm run build` ✅ 通过（vite 构建成功；仅 500kB chunk 体积警告，非阻塞）
- 运行时冒烟（尽力而为）：gateway/MySQL/Redis/ClickHouse 进程在跑（docker socket 对本用户无权限，无法 `docker ps` 核对 compose 状态）；`POST /api/auth/login` → 200 + `access_token`；携带 token 访问受保护路由 `GET /api/k8s/clusters` → **200（非 403）** ✅；注意：运行中的 gateway 为 P4 之前的旧二进制（`/api/audit/events`、`/api/secrets`、`/api/approvals`、`/api/alerts`、`/api/recordings`、`/api/files` 均返回 404），新端点冒烟需重新构建部署后补做

### 8.4 测试矩阵（per-crate，本轮实测）

| crate | 单测 | 集成测试 | 合计 |
|---|---|---|---|
| superops-gateway | 73 | — | 73 |
| superops-k8s | 14 | — | 14 |
| superops-collector | 14 | 18（alert/ch/collect/config/events/inspect_test） | 32 |
| ecat-middleware | 23 | — | 23 |
| ecat-auth | 15 | — | 15 |
| ecat-encoding | 15 | — | 15 |
| ecat-cli | 11 | — | 11 |
| ecat-transport | 11 | — | 11 |
| ecat-config | 10 | — | 10 |
| ecat-data-clickhouse | 10 | — | 10 |
| ecat-security | 10 | — | 10 |
| ecat-metadata | 9 | — | 9 |
| ecat-client | 7 | — | 7 |
| ecat-data-sqlx | 7 | — | 7 |
| ecat-config-remote / data-elasticsearch / registry / testing / tls / transport-http | 各 5 | — | 30 |
| ecat / circuit-breaker / data-influxdb / data-opensearch / errors / health / openapi | 各 4 | — | 28 |
| data-memcached / metrics / scheduler / tracing / transport-grpc / versioning | 各 3 | — | 18 |
| 其余 16 个 ecat-* crate | 各 2 | — | 32 |
| 其余 6 个 ecat-* crate（arangodb/iotdb/neo4j/logging/tracing-otlp/transport-ws） | 各 1 | — | 6 |
| 0 测试 crate（bench/ecat-data/ecat-lock/ecat-protos/helloworld/superops-protos） | 0 | — | 0 |
| **合计** | **343** | **18** | **361** |

（grand total 由全部 `test result` 行求和得出；单测行数由 `Running unittests` 二进制归属，并行输出下个别归属可能偏差 ±1，总量与失败数为准。）

### 8.5 已知限制与延期项（deferred，逐项记录 — 2026-08-07 复审后状态）

> 状态列：✅ 已修复（本日复审闭环）/ 📌 保留（设计取舍或既有债务，已记录原因）

| # | 项目（中文 — English） | 状态 |
|---|---|---|
| 1 | k8s_proxy.rs 实测 548 行，超 500 行约束（既有债务）— k8s_proxy.rs measures 548 lines, over the 500-line rule (pre-existing debt) | ✅ 拆分（proxy/k8s_proxy.rs 418 行 + k8s_exec.rs 139 行） |
| 2 | OAuth2 角色解析依赖外部 IdP claims — OAuth2 role resolution depends on external IdP claims (auth) | 📌 依赖引入方：外部 IdP 的 scope/claim 契约，网关侧仅映射（见 routes.rs k8s OAuth2Layer 挂载） |
| 3 | 多租户设计警示：tenant 由客户端 header 断言；users.tenant_id 列未实际使用（init.sql 已加列，model/user.rs 未引用）；畸形 header 静默回落 `default`；回归测试自引用 — Multi-tenancy caveats: tenant asserted from a client-supplied header; `users.tenant_id` column unused; malformed headers silently fall back to `default`; regression tests are self-referential | ✅ 修复：tenant_id 已接线（User/UserRow/list_users/create，注册恒为 'default'）；畸形 x-tenant-id → 400（缺失头仍回退 default），tenant_test 同步覆盖两态。📌 保留：tenant 由客户端头断言（无服务端租户目录，属设计）；回归测试自引用（单测无法起真实 MySQL+gateway，占位中间件等价性已注释） |
| 4 | housekeeping 小项：NaN 静默求和；不可读目录静默跳过；仅备份部分文件；dev yaml root123 默认口令（有 env 覆盖）— housekeeping: NaN sums silently; unreadable dirs skipped; partial backup files; dev YAML ships `root123` default password (env-overridable) | ✅ 修复：parse_cpu_cores/parse_mem_gib 非有限值（NaN/Inf）→ 0.0（+ 单测）；prune_backups 目录不可读 → warn；mysqldump 管道失败删除半成品 .sql.gz。📌 保留：dev yaml root123 默认口令（config/gateway.yaml，env 可覆盖，仅 dev 形态） |
| 5 | secrets 小项：`Key::from_slice` panic 面；SecretRow 实现 Debug；rand 多版本并存（lockfile 0.8/0.9/0.10）— secrets: `Key::from_slice` panics on wrong key length; `SecretRow` derives Debug; multiple rand versions coexist in the lockfile | ✅ 修复：vault.rs `cipher()` 32 字节长度校验（panic→Err，503 语义不变）；SecretRow 手动 Debug 密文打码。📌 保留：rand 多版本（0.8/0.9/0.10 由 upstream 固定：sqlx/opentelemetry_sdk/kube-client/tower 0.4 锁 0.8，bson/mongodb 锁 0.9，hickory/quinn-proto 锁 0.10；workspace 无法统一，各 crate 均直接依赖，无传递混用风险） |
| 6 | ClickhouseClient 不可 Clone（设计说明，非缺陷）— ClickhouseClient is not Clone by design | 📌 设计说明：录制/写入路径经 `&Arc<ClickhouseClient>` 共享（record_frame 签名即如此），无需 Clone |
| 7 | frames 响应上限 5000 帧（理论最坏 ~1.7GB）— frames response capped at 5000 frames (~1.7GB worst case) | 📌 保留：回放接口 `LIMIT 5000`（ORDER BY timestamp, seq），页面前端分页读取，文档已标注 |
| 8 | DELETE 轻量删除异步生效（204 后行才消失）— lightweight deletes apply asynchronously (rows disappear after 204) | 📌 设计说明：recording 删除为 CH lightweight delete，204 后数据仍可查询片刻，属 CH 语义 |
| 9 | 同秒帧无序号（recording 回放顺序在秒内不保证）— same-second frames carry no sequence number (recording order) | ✅ 修复：recorder.rs per-session seq（bump_seq，写入前取号），exec_session 增 seq 列；回放 ORDER BY timestamp, seq；recordings_api 返回 seq 字段；recorder_test 覆盖 |
| 10 | `DefaultBodyLimit` 413 与常规 400 错误形态不一致 — 413 (DefaultBodyLimit) vs 400 error shapes are inconsistent | 📌 保留：axum 默认 BytesRejection → 413 PAYLOAD_TOO_LARGE（语义正确，形态差异记录在案） |
| 11 | `create_dir_all` 启动期硬失败（备份目录/上传目录不可创建即报错）— `create_dir_all` fails hard at startup | ✅ 修复：fail-soft（创建失败仅 warn，功能降级不崩溃） |
| 12 | exec 录制无条件开启（无配置开关）— exec recording is always on (no toggle) | ✅ 修复：config `recording.enabled`（默认 true 保持旧行为；config/gateway.yaml 已同步） |
| 13 | OTLP 流式路径 span 无 resource 字段（非阻塞）— OTLP spans on streaming paths lack resource fields (non-blocking) | 📌 非阻塞：流式（exec/ws）路径 span 走 streaming pipeline，resource 归属性留待 OTel SDK 升级，已记录 |
| 14 | 指标页角色：`/api/v1/metrics/query` 在 k8s_read_routes（api:read），页面在 ops 菜单 — 仅 api:read 用户可看；已记录 — metrics page: query route is api:read while the page sits in the ops menu; only api:read users can view; documented | 📌 已记录：查询端点归 api:read（与 k8s 只读一致），菜单归属 ops 为前端组织，二者可并存 |
| 15 | main.rs 实测 602 行，超 500 行约束（路由组增长点）— main.rs measures 602 lines, over the 500-line rule (route-group growth point) | ✅ 拆分（main.rs 186 行 + routes.rs 374 行，路由组迁至 routes.rs） |
| 16 | ack 对不存在告警 id 也存储 1h（幂等设计，可接受）— ack stores nonexistent alert ids for 1h too (idempotent by design, accepted) | 📌 幂等设计：ack 写入与告警存在性解耦，重复/幽灵 id 返回 202 语义一致，接受 |
| 17 | 502 错误体回显上游错误细节（k8s backend unreachable/rpc failed + 具体错误）— 502 bodies echo upstream error details (`format!("...: {e}")`) | 📌 保留：错误细节对排障有用（熔断 503 已挡绝大多数异常），回显仅含传输错误文本不含凭据；若需对外脱敏可加错误码映射，已记录 |

## 9. P6 结论

P4–P6 全部功能落地并通过全量验证：**workspace 361 项测试全过（0 失败）**、`cargo fmt --check`/`cargo check`/`npm run build` 全绿；运行时冒烟 login→受保护 GET 200（非 403）通过。新增安全校验点（SQL 参数化 / 文件名白名单 / 密文不泄露 / 主密钥 503 两态 / 审批门禁 412）复核无异常。2026-08-07 复审闭环：第 8.5 节 17 项中 **8 项已修复**（行数超限拆分 ×2、tenant_id 接线与畸形头 400、NaN/Inf 守卫与半成品备份清理、Key 长度校验与密文打码、录制 seq 序号、recording 配置开关、create_dir_all fail-soft），其余 9 项为设计取舍或既有债务（IdP 契约 / rand 多版本 / CH 语义 / 413 形态 / OTLP resource / 指标页角色 / ack 幂等 / 5000 帧上限 / 502 错误回显），均已记录原因，无阻塞项；新端点运行时冒烟因运行中的 gateway 为旧二进制（P4 前构建）而推迟，重建部署后补验。
