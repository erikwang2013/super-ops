# SuperOps 生态缺口补齐 P5 — 日志中心 / CMDB / 批量执行 / RBAC

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. 本文件是 `2026-08-06-ecosystem-gap-completion.md` 的 P5 部分，前置条件：P4 已完成并全绿。

**Goal:** 补齐缺口矩阵 #4 日志中心、#5 CMDB 资产、#6 批量执行/脚本库、#7 RBAC 细化。

**Architecture:** 日志中心 = collector 新增周期任务拉取 Pod 日志写 ClickHouse（沿用 `DataPoint/TsdbClient` 模式）+ gateway 检索 API + 前端检索页；CMDB = MySQL 资产表 + gateway CRUD + dashboard 真实数据；批量执行 = 脚本表 + k8s Job 执行（k8s service 新增 RPC）；RBAC = users.role + JWT claim + gateway 中间件。

**Tech Stack:** 同主计划；k8s Job 用 `kube::api::Api<k8s_openapi::api::batch::v1::Job>`；日志检索复用 ClickHouse（不引入 Elasticsearch，避免新增基础设施）。

**硬性约束：** 不做任何打包；未经明确要求不 commit（commit 步骤均可选）；文件 <500 行；边界校验；编辑前先 Read；改动后跑 `cargo test` / `cargo fmt` / `npm run build`；不提交 secrets/.env。

---

### Task 1: collector — logtail.rs 日志采集任务

**Files:**
- Create: `services/collector/src/logtail.rs`
- Modify: `services/collector/src/lib.rs`（注册模块 + scheduler 挂接，`grep "scheduler\|ecat_scheduler" services/collector/src/lib.rs` 定位现有任务注册方式）
- Modify: `services/collector/src/config.rs` + `config/collector.yaml`

- [ ] **Step 1: 写失败测试（行切分与采样纯函数）**

Create `services/collector/src/logtail.rs`：

```rust
pub fn truncate_line(line: &str, max_bytes: usize) -> String {
    if line.len() <= max_bytes {
        line.to_string()
    } else {
        format!("{}…(truncated)", &line[..max_bytes])
    }
}

pub fn dedup_continuous(lines: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for l in lines {
        if out.last().map(|p| p != l).unwrap_or(true) {
            out.push(l.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_line_cuts_long_lines() {
        let long = "x".repeat(1000);
        assert_eq!(truncate_line(&long, 64).len(), 64 + "…(truncated)".len());
        assert_eq!(truncate_line("short", 64), "short");
    }

    #[test]
    fn dedup_continuous_keeps_only_runs() {
        let lines = vec!["a".into(), "a".into(), "b".into(), "b".into(), "b".into(), "a".into()];
        assert_eq!(dedup_continuous(&lines), vec!["a".to_string(), "b".to_string(), "a".to_string()]);
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p superops-collector logtail::tests 2>&1 | tail -5`
Expected: 编译失败（模块未注册）。

- [ ] **Step 3: 注册模块并实现采集函数**

`lib.rs`：`pub mod logtail;`

logtail.rs 追加（拉取模式：非 follow 的 tail 拉取，每个 namespace 逐 Pod 调用 `GetPodLogs`，超长行截断后写入 ClickHouse）：

```rust
use crate::ch::{clickhouse_from, now_secs};
use crate::config::Config;
use ecat_data::{DataPoint, FieldValue, TsdbClient};
use superops_protos::k8s::v1::{GetPodLogsRequest, ListPodsRequest, k8s_service_client::K8sServiceClient};

pub async fn collect_once(cfg: &Config) -> anyhow::Result<()> {
    let mut client = K8sServiceClient::connect(cfg.k8s.endpoint.clone()).await?;
    let ch = clickhouse_from(cfg)?;
    for ns in &cfg.logtail.namespaces {
        let pods = client
            .list_pods(ListPodsRequest { namespace: ns.clone(), ..Default::default() })
            .await?
            .into_inner()
            .pods;
        for pod in pods {
            let stream = client
                .get_pod_logs(GetPodLogsRequest {
                    cluster_id: String::new(),
                    namespace: ns.clone(),
                    pod_name: pod.name.clone(),
                    container: String::new(),
                    tail_lines: cfg.logtail.tail_lines,
                    follow: false,
                })
                .await
                .map_err(|e| anyhow::anyhow!("logs for {}/{}: {e}", ns, pod.name))?;
            let mut points = Vec::new();
            let mut stream = stream.into_inner();
            while let Some(line) = stream.message().await? {
                let content = truncate_line(&line.content, cfg.logtail.max_line_bytes as usize);
                points.push(
                    DataPoint::new("pod_log")
                        .with_tag("namespace", ns.clone())
                        .with_tag("pod", pod.name.clone())
                        .with_tag("container", line_container_hint(&line.content))
                        .with_field("content", FieldValue::String(content))
                        .with_timestamp(line.timestamp.max(now_secs() - 86400)),
                );
            }
            if !points.is_empty() {
                TsdbClient::write(ch.as_ref(), &points).await?;
            }
        }
    }
    Ok(())
}

fn line_container_hint(_content: &str) -> String {
    "unknown".into() // 简化：容器信息后续经 Pod 元数据补齐
}
```

> 注意：`GetPodLogsRequest` 的 `cluster_id` 语义以现有 `resource::pod::get_pod_logs` 实现为准（`grep "get_pod_logs" services/k8s/src/resource/pod.rs`），若 k8s service 忽略 cluster_id 可直接传空串；`tail_lines`/`max_line_bytes` 见 Step 4 配置。

- [ ] **Step 4: 配置字段与调度注册**

config.rs 追加：

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct LogtailConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub namespaces: Vec<String>,
    #[serde(default = "default_tail")]
    pub tail_lines: i32,
    #[serde(default = "default_max_line")]
    pub max_line_bytes: i32,
}

fn default_tail() -> i32 { 200 }
fn default_max_line() -> i32 { 1024 }
```

Config 新增 `#[serde(default)] pub logtail: LogtailConfig,`。`lib.rs` 中在现有 scheduler 任务注册处（模式与 inspect 任务一致）追加：

```rust
    if cfg.logtail.enabled {
        schedule.logtail = Some(crate::logtail::collect_once); // 周期取 cfg.logtail.interval_secs（默认 30s）
    }
```

`config/collector.yaml` 追加：

```yaml
logtail:
  enabled: false
  namespaces: ["default"]
  tail_lines: 200
  max_line_bytes: 1024
  interval_secs: 30
```

- [ ] **Step 5: 验证**

Run: `cargo test -p superops-collector 2>&1 | tail -5 && cargo fmt --check`
Expected: 全部通过。e2e（需 compose）：`logtail.enabled: true` 后观察 ClickHouse `pod_log` 表有新行（`curl "localhost:8124/?query=SELECT+count(*)+FROM+pod_log"`）。

- [ ] **Step 6（可选）:** commit `feat(collector): pod log collection to ClickHouse`

### Task 2: gateway — 日志检索 API

**Files:**
- Create: `services/gateway/src/logs_api.rs`
- Modify: `services/gateway/src/main.rs`（模块注册 + 路由）

- [ ] **Step 1: 写失败测试（查询参数解析纯函数）**

Create `services/gateway/src/logs_api.rs`：

```rust
use axum::{Json, extract::Query, response::IntoResponse};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
pub struct LogSearchQuery {
    pub namespace: Option<String>,
    pub pod: Option<String>,
    pub keyword: Option<String>,
    pub from: Option<i64>,
    pub to: Option<i64>,
    pub limit: Option<i64>,
}

fn clamp_limit(q: &LogSearchQuery) -> i64 {
    q.limit.unwrap_or(100).clamp(1, 1000)
}

fn build_where(q: &LogSearchQuery) -> Vec<String> {
    let mut w = Vec::new();
    if let Some(ns) = &q.namespace {
        if !ns.is_empty() {
            w.push(format!("namespace = '{}'", ns.replace('\'', "")));
        }
    }
    if let Some(p) = &q.pod {
        if !p.is_empty() {
            w.push(format!("pod = '{}'", p.replace('\'', "")));
        }
    }
    if let Some(k) = &q.keyword {
        if !k.is_empty() {
            w.push(format!("content ILIKE '%{}%'", k.replace('\'', "").replace('%', "")));
        }
    }
    w
}

pub async fn search_logs(Query(q): Query<LogSearchQuery>) -> impl IntoResponse {
    let limit = clamp_limit(&q);
    let where_sql = build_where(&q);
    let sql = if where_sql.is_empty() {
        format!("SELECT * FROM pod_log ORDER BY ts DESC LIMIT {limit}")
    } else {
        format!("SELECT * FROM pod_log WHERE {} ORDER BY ts DESC LIMIT {limit}", where_sql.join(" AND "))
    };
    // TODO(Task 2 Step 3): 用 ClickhouseClient 执行 sql 并映射为 JSON
    Json(serde_json::json!({ "logs": [], "sql": sql }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn where_clause_escapes_and_combines() {
        let q = LogSearchQuery { namespace: Some("default".into()), keyword: Some("error%' OR 1=1".into()), ..Default::default() };
        let w = build_where(&q);
        assert_eq!(w.len(), 2);
        assert!(w.iter().any(|s| s.contains("content ILIKE")));
        assert!(!w.iter().any(|s| s.contains("1=1")));
    }

    #[test]
    fn limit_is_clamped() {
        assert_eq!(clamp_limit(&LogSearchQuery { limit: Some(99999), ..Default::default() }), 1000);
        assert_eq!(clamp_limit(&LogSearchQuery::default()), 100);
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p superops-gateway where_clause_escapes 2>&1 | tail -5`
Expected: 编译失败（模块未注册）。

- [ ] **Step 3: 注册模块、路由并接入 ClickHouse**

`main.rs`：`pub mod logs_api;`，路由（与 `/api/audit/events` 同处注册）：

```rust
        .route("/api/logs/search", axum::routing::get(crate::logs_api::search_logs))
```

handler 改为接收 `State<crate::AppState>`，用与 `audit_api`（P4 Task 8）相同的 ClickHouse 客户端执行 SQL 并映射行 → JSON（时间戳转 ISO8601：`chrono` 已在 workspace，用 `DateTime::from_timestamp`）。

> 安全要点：SQL 中所有字符串值已先去除 `'` 与 `%` 注入字符；关键词仅走 `ILIKE` 白名单列 `content`，不允许任意列名拼接。

- [ ] **Step 4: 验证**

Run: `cargo test -p superops-gateway 2>&1 | tail -5 && cargo fmt --check && cargo build -p superops-gateway`
Expected: 通过。e2e：`curl "localhost:8080/api/logs/search?namespace=default&keyword=error"` 返回日志行。

- [ ] **Step 5（可选）:** commit `feat(gateway): pod log search API`

### Task 3: CMDB — 资产表与 CRUD API

**Files:**
- Modify: `deploy/init.sql`（新增 cmdb_asset 表）
- Create: `services/gateway/src/model/cmdb.rs`
- Modify: `services/gateway/src/main.rs`（模块 + 路由）

- [ ] **Step 1: 建表（先 Read deploy/init.sql 确认表命名与风格）**

在 `deploy/init.sql` 末尾追加：

```sql
CREATE TABLE IF NOT EXISTS cmdb_asset (
  id BIGINT AUTO_INCREMENT PRIMARY KEY,
  asset_type VARCHAR(32) NOT NULL,
  name VARCHAR(128) NOT NULL,
  ip VARCHAR(64),
  env VARCHAR(16) DEFAULT 'prod',
  owner VARCHAR(64) DEFAULT '',
  labels JSON,
  status VARCHAR(16) DEFAULT 'active',
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  UNIQUE KEY uk_name_type (asset_type, name)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
```

- [ ] **Step 2: 写失败测试（字段校验纯函数）**

Create `services/gateway/src/model/cmdb.rs`：

```rust
pub const ASSET_TYPES: [&str; 6] = ["host", "switch", "router", "app", "db", "storage"];

pub fn validate_asset(asset_type: &str, name: &str) -> Result<(), String> {
    if !ASSET_TYPES.contains(&asset_type) {
        return Err(format!("asset_type must be one of {:?}", ASSET_TYPES));
    }
    if name.is_empty() || name.len() > 128 {
        return Err("name must be 1..=128 chars".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_asset_rejects_unknown_type_and_empty_name() {
        assert!(validate_asset("vm", "x").is_err());
        assert!(validate_asset("host", "").is_err());
        assert!(validate_asset("host", "web-01").is_ok());
    }
}
```

- [ ] **Step 3: 实现 CRUD（沿用 P4 Task 9 的 sqlx 模式）**

`model/cmdb.rs` 追加（先 Read `services/gateway/src/model/user.rs` 确认 pool 类型与 Row 结构写法后对齐）：

```rust
#[derive(sqlx::FromRow)]
pub struct AssetRow {
    pub id: i64,
    pub asset_type: String,
    pub name: String,
    pub ip: Option<String>,
    pub env: String,
    pub owner: String,
    pub labels: Option<String>,
    pub status: String,
}

pub async fn list_assets(pool: &sqlx::MySqlPool, asset_type: Option<&str>, env: Option<&str>) -> Result<Vec<AssetRow>, sqlx::Error> {
    let mut sql = "SELECT id, asset_type, name, ip, env, owner, labels, status FROM cmdb_asset WHERE 1=1".to_string();
    if let Some(t) = asset_type { sql.push_str(&format!(" AND asset_type = '{}'", t)); }
    if let Some(e) = env { sql.push_str(&format!(" AND env = '{}'", e)); }
    sql.push_str(" ORDER BY created_at DESC");
    sqlx::query_as::<_, AssetRow>(&sql).fetch_all(pool).await
}

pub async fn upsert_asset(pool: &sqlx::MySqlPool, asset_type: &str, name: &str, ip: Option<&str>, env: &str, owner: &str, labels: Option<&str>) -> Result<i64, sqlx::Error> {
    let r = sqlx::query(
        "INSERT INTO cmdb_asset (asset_type, name, ip, env, owner, labels) VALUES (?, ?, ?, ?, ?, ?) \
         ON DUPLICATE KEY UPDATE ip = VALUES(ip), env = VALUES(env), owner = VALUES(owner), labels = VALUES(labels)",
    )
    .bind(asset_type).bind(name).bind(ip).bind(env).bind(owner).bind(labels)
    .execute(pool).await?;
    Ok(r.last_insert_id())
}

pub async fn delete_asset(pool: &sqlx::MySqlPool, id: i64) -> Result<bool, sqlx::Error> {
    let r = sqlx::query("DELETE FROM cmdb_asset WHERE id = ?").bind(id).execute(pool).await?;
    Ok(r.rows_affected() > 0)
}
```

路由（`main.rs`）：

```rust
        .route("/api/cmdb/assets", axum::routing::get(list_cmdb_assets).post(create_cmdb_asset))
        .route("/api/cmdb/assets/{id}", axum::routing::delete(delete_cmdb_asset))
        .route("/api/cmdb/stats", axum::routing::get(cmdb_stats))
```

handler：`list_cmdb_assets` 支持 `?asset_type=&env=` 筛选；`create_cmdb_asset` 先 `validate_asset`（400）再 upsert（201）；`delete_cmdb_asset` 404 若不存在；`cmdb_stats` 返回 `{"hosts":N,"switches":N,"dbs":N,...}`（GROUP BY asset_type 计数）。

- [ ] **Step 4: 验证**

Run: `cargo test -p superops-gateway cmdb 2>&1 | tail -5 && cargo fmt --check && cargo build -p superops-gateway`
Expected: 通过。e2e：创建/查询/删除资产各一次。

- [ ] **Step 5（可选）:** commit `feat(gateway): CMDB asset CRUD API`

### Task 4: 前端 — CMDB 页 + dashboard 真实数据

**Files:**
- Create: `frontend/src/pages/cmdb/index.tsx`
- Modify: `frontend/src/App.tsx`
- Modify: `frontend/src/pages/dashboard.tsx`（替换硬编码 0）
- Modify: `frontend/src/services/api.ts`

- [ ] **Step 1: CMDB 页面**

ProTable 请求 `/api/cmdb/assets`：列 类型（Tag 映射 host/switch/router/app/db/storage 颜色）/ 名称 / IP / 环境 / 负责人 / 状态；顶部「新建资产」Modal（类型 Select + 名称 + IP + 环境 + 负责人 + labels JSON 文本）；行操作「删除」Popconfirm。

- [ ] **Step 2: dashboard 接入真实数据**

`dashboard.tsx` 的四个 StatisticCard 改为挂载后请求：
- `K8s 集群` ← `GET /api/k8s/clusters` 的 clusters.length
- `Docker 主机` ← `GET /api/cmdb/stats` 的 hosts
- `Pipeline` ← `GET /api/cmdb/stats` 的 apps（临时口径，标注待审批流接入）
- `活跃告警` ← `GET /api/audit/events?limit=1` 或 `GET /api/v1/metrics/query`（若 P6 告警页未就绪，先显示 `-` 并注释 TODO）

数据为空时显示 0 而非 loading 死局。

- [ ] **Step 3: 路由与菜单**

`App.tsx` menuData 追加 `{ path: '/cmdb', name: 'CMDB 资产', icon: <DatabaseOutlined /> }`，路由 `<Route path="/cmdb" element={<CmdbPage />} />`。

- [ ] **Step 4: 验证**

Run: `cd frontend && npm run build`
Expected: 通过。手动验证 dashboard 数字非全 0。

- [ ] **Step 5（可选）:** commit `feat(frontend): CMDB page and real dashboard stats`

### Task 5: 批量执行 — 脚本库与 k8s Job

**Files:**
- Modify: `deploy/init.sql`（script / script_run 表）
- Modify: `protos/k8s/v1/k8s.proto` + `make proto`（RunJob RPC）
- Create: `services/k8s/src/resource/job.rs`（mod.rs 注册）
- Modify: `services/k8s/src/service.rs`
- Create: `services/gateway/src/scripts_api.rs`（main.rs 注册）
- Create: `frontend/src/pages/ops/scripts.tsx`（App.tsx 注册）

- [ ] **Step 1: 建表**

```sql
CREATE TABLE IF NOT EXISTS script (
  id BIGINT AUTO_INCREMENT PRIMARY KEY,
  name VARCHAR(128) NOT NULL,
  description VARCHAR(512) DEFAULT '',
  language VARCHAR(16) DEFAULT 'shell',
  content MEDIUMTEXT NOT NULL,
  timeout_s INT DEFAULT 300,
  created_by VARCHAR(64) DEFAULT '',
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

CREATE TABLE IF NOT EXISTS script_run (
  id BIGINT AUTO_INCREMENT PRIMARY KEY,
  script_id BIGINT NOT NULL,
  target_pods VARCHAR(512) DEFAULT '',
  status VARCHAR(16) DEFAULT 'pending',
  output MEDIUMTEXT,
  started_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  finished_at TIMESTAMP NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
```

- [ ] **Step 2: proto 新增 RunJob**

k8s.proto service 追加：

```proto
  rpc RunJob(RunJobRequest) returns (RunJobResponse);
```

```proto
message RunJobRequest {
  string cluster_id = 1;
  string namespace = 2;
  string job_name = 3;
  string image = 4;
  string command = 5;
  int32 timeout_s = 6;
}
message RunJobResponse { bool created = 1; }
```

Run: `make proto && cargo check -p superops-protos`

- [ ] **Step 3: resource/job.rs 实现（写失败测试先行）**

```rust
pub fn validate_job(cluster_id: &str, namespace: &str, job_name: &str, image: &str, command: &str) -> anyhow::Result<()> {
    if cluster_id.is_empty() || namespace.is_empty() || job_name.is_empty() || image.is_empty() {
        return Err(anyhow::anyhow!("cluster_id/namespace/job_name/image must not be empty"));
    }
    if command.is_empty() {
        return Err(anyhow::anyhow!("command must not be empty"));
    }
    if job_name.len() > 63 || !job_name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
        return Err(anyhow::anyhow!("job_name must be DNS-1123 (<=63, [a-z0-9-])"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_job_rejects_bad_input() {
        assert!(validate_job("", "ns", "j", "img", "cmd").is_err());
        assert!(validate_job("c", "ns", "UPPER", "img", "cmd").is_err());
        assert!(validate_job("c", "ns", "job-1", "img", "cmd").is_ok());
    }
}
```

实现（job.rs 追加）：

```rust
use crate::cluster::client::ClusterClient;
use kube::api::{Api, PostParams};

pub async fn run_job(client: &ClusterClient, namespace: &str, job_name: &str, image: &str, command: &str, timeout_s: i32) -> anyhow::Result<bool> {
    let api = Api::<k8s_openapi::api::batch::v1::Job>::namespaced(client.client.clone(), namespace);
    let job = k8s_openapi::api::batch::v1::Job {
        metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta { name: Some(job_name.into()), ..Default::default() },
        spec: Some(k8s_openapi::api::batch::v1::JobSpec {
            template: k8s_openapi::api::core::v1::PodTemplateSpec {
                spec: Some(k8s_openapi::api::core::v1::PodSpec {
                    restart_policy: Some("Never".into()),
                    containers: vec![k8s_openapi::api::core::v1::Container {
                        name: "runner".into(),
                        image: Some(image.into()),
                        command: Some(vec!["/bin/sh".into(), "-c".into(), command.into()]),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            },
            backoff_limit: Some(0),
            active_deadline_seconds: Some(i64::from(timeout_s.max(1))),
            ..Default::default()
        }),
        ..Default::default()
    };
    api.create(&PostParams::default(), &job).await?;
    Ok(true)
}
```

- [ ] **Step 4: service.rs handler + gateway API**

service.rs 追加 `run_job` handler（模式同 P4 Task 3：`validate_job` → InvalidArgument，`manager.get` → NotFound，创建失败 → Internal）。

`scripts_api.rs`（gateway）：
- `GET /api/scripts` — 脚本列表（sqlx 查询 script 表）
- `POST /api/scripts` — 新建（name 1..=128、language ∈ {shell,python,go}、content 非空且 ≤64KB → 否则 400）
- `DELETE /api/scripts/{id}` — 删除
- `POST /api/scripts/{id}/run` — 入参 `{cluster_id, namespace, target_pods: [..]}`：写 script_run(pending) → 调 k8s service `run_job`（镜像固定 `busybox:1.36`，命令 = `脚本内容 + 目标 pods 循环` 的包装 shell）→ 成功则 script_run 置 running
- `GET /api/scripts/runs?script_id=` — 运行记录列表（status 含 pending/running/succeeded/failed，output 截断 100KB）

> 运行结果回收：collector 下一阶段（P6-11 备份/容量 或后续任务）再补 Job 状态轮询；P5 先落 pending/running 与 Job 创建结果。

- [ ] **Step 5: 前端 /ops/scripts 页**

ProTable 列表 + 新建 Modal（name/description/language/content textarea）+ 行操作「运行」（Modal 选集群/命名空间/目标 Pods 逗号分隔）→ 跳转或内嵌运行记录 Tab；运行记录展示 status Tag 与 output 折叠展示。

`App.tsx`「运维中心」children 追加 `{ path: '/ops/scripts', name: '脚本库' }` 与 Route。

- [ ] **Step 6: 验证**

Run: `cargo test -p superops-k8s-service 2>&1 | tail -5 && cargo test -p superops-gateway 2>&1 | tail -5 && cargo fmt --check && cd frontend && npm run build`
Expected: 全绿。e2e（需 k8s 集群）：创建脚本 → run → 集群中出现 `superops-script-*` Job。

- [ ] **Step 7（可选）:** commit `feat(scripts): script library with k8s job execution`

### Task 6: RBAC — 角色校验中间件

**Files:**
- Modify: `services/gateway/src/auth/middleware.rs`（先 Read，确认现有 JWT 校验与 claims 结构）
- Modify: `services/gateway/src/main.rs`（路由分组加中间件）

- [ ] **Step 1: 写失败测试（权限判定纯函数）**

```rust
pub fn has_permission(user_role: &str, required: &str) -> bool {
    match user_role {
        "admin" => true,
        "operator" => matches!(required, "api:read" | "api:write" | "ops:audit" | "ops:cmdb" | "ops:scripts"),
        "viewer" => matches!(required, "api:read"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_permission_matrix() {
        assert!(has_permission("admin", "ops:users"));
        assert!(has_permission("operator", "api:write"));
        assert!(!has_permission("operator", "ops:users"));
        assert!(has_permission("viewer", "api:read"));
        assert!(!has_permission("viewer", "api:write"));
        assert!(!has_permission("banned", "api:read"));
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test -p superops-gateway role_permission_matrix 2>&1 | tail -5`
Expected: 编译失败。

- [ ] **Step 3: 实现中间件接入**

实现 `require_role(required: &str) -> impl Fn(...) -> middleware`（axum `from_fn`），逻辑：从请求扩展提取 `AuthClaims`（现有 JWT 中间件注入的 claims 结构，`grep "AuthClaims" services/gateway/src/auth/middleware.rs` 确认字段——若无 role 字段，则 login 时从 users 表读取 role 写入 token，注册用户默认 `viewer`，首个用户提升 `admin` 的初始化 SQL 见 init.sql 注释）。

路由分组改造（`main.rs`）：

```rust
let protected = Router::new()
    .route("/api/users", axum::routing::get(list_users))
    .route("/api/users/{id}/status", axum::routing::patch(set_user_status))
    .route("/api/audit/events", axum::routing::get(audit_events))
    .route("/api/cmdb/assets", axum::routing::get(list_cmdb_assets).post(create_cmdb_asset))
    .route("/api/scripts", axum::routing::get(list_scripts).post(create_script))
    .route("/api/scripts/runs", axum::routing::get(list_script_runs))
    .route("/api/scripts/{id}/run", axum::routing::post(run_script))
    .route("/api/scripts/{id}", axum::routing::delete(delete_script))
    .layer(axum::middleware::from_fn(move |req, next| require_role("ops:admin", req, next)));
```

权限映射：`/api/users*` → `ops:users`；`/api/audit/events` → `ops:audit`；`/api/cmdb/*` → `ops:cmdb`；`/api/scripts*` → `ops:scripts`；k8s 写操作（scale/restart/delete）→ `api:write`；其余 k8s 读路径 → `api:read`。无权限返回 403 `{"error":"forbidden"}`。`has_permission` 的 required 取值同步扩展为 `"ops:admin" | "ops:users" | "ops:audit" | "ops:cmdb" | "ops:scripts"`。

- [ ] **Step 4: 验证**

Run: `cargo test -p superops-gateway 2>&1 | tail -5 && cargo fmt --check`
Expected: 通过。e2e：viewer 访问 /api/users → 403；admin → 200。

- [ ] **Step 5（可选）:** commit `feat(gateway): RBAC role middleware`

### Task 7: P5 文档同步

**Files:**
- Modify: `CHANGELOG.md`、`README.md`、`README.en.md`、`docs/images/structure.svg`、`docs/images/tree.svg`

- [ ] **Step 1:** CHANGELOG 新增 P5 条目；README 功能说明与 API 一览补充 `/api/logs/search`、`/api/cmdb/*`、`/api/scripts*`；known limits 中「exec 终端需真实集群」不变。
- [ ] **Step 2:** structure.svg 的 Collector 列补 `logtail.rs 日志采集`、Frontend 列补 `cmdb`、`ops/scripts`；tree.svg 的 services/collector 子树补 `logtail.rs`。XML 校验：`python3 -c "import xml.dom.minidom,sys; [xml.dom.minidom.parse(f) for f in sys.argv[1:]]" docs/images/structure.svg docs/images/tree.svg`
- [ ] **Step 3:** 全量验证：`cargo test --workspace 2>&1 | tail -3 && cargo fmt --check && cd frontend && npm run build`
- [ ] **Step 4（可选）:** commit `docs: P5 ecosystem completion docs & diagrams`

---

## P5 完成标准

- [ ] 日志中心链路通：collector 采集 → ClickHouse → 检索 API → 前端搜索页
- [ ] CMDB CRUD + dashboard 真实计数；scripts 库创建/运行/记录闭环
- [ ] RBAC 生效：viewer/operator/admin 权限矩阵测试全绿，403 语义正确
- [ ] 文档与图集同步；全程未打包、未 push、无 secrets 入库

**下一步：P6 见 `2026-08-06-ecosystem-gap-completion-p6.md`（审批流 / 多租户 / 保险库 / 备份容量成本 / 终端录制文件传输 / gRPC 追踪 / Helm / 指标告警页）。**
