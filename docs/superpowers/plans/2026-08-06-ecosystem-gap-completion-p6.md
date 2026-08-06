# P6 实施计划：审批 / 多租户 / 凭据保险库 / 治理任务 / 录制与文件 / 追踪补齐 / Helm / 前端页

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 补齐缺口矩阵 P6 八个领域：审批工作流、多租户、凭据保险库、运维治理（备份/容量/成本）、终端录制 + 文件传输、gRPC/调度 OTLP 追踪、Helm chart 文件、前端告警/指标页。

**Architecture:** 全部沿用既有模式——MySQL 表 + sqlx 参数化绑定（gateway model 层）、ClickHouse DataPoint（collector）、axum Router 追加（gateway）、ProLayout 追加页面（frontend）。凭据用 AES-256-GCM 加密落库，主密钥走环境变量。终端录制帧写入 ClickHouse `exec_session`；文件上传做文件名白名单 + 大小上限。Helm 仅提供 chart 文件，不构建、不推送镜像。

**Tech Stack:** Rust / axum / sqlx(MySQL) / aes-gcm / clickhouse / React + antd / Helm 3

---

## 硬性约束（与主计划一致）

- **不做任何打包**：不构建 docker 镜像、不 push 镜像、不发布 crate；Helm chart 仅提供文件
- 未经用户明确要求绝不 commit（本计划 commit 步骤均标注「可选，仅当用户要求时」）
- 绝不提交 secrets / credentials / .env；`SUPEROPS_MASTER_KEY` 仅经 env 注入，测试用密钥仅限测试代码
- 文件 <500 行；编辑前先 Read；修改后运行 `cargo fmt --check && cargo check && cargo test` 与 `npm run build`
- 系统边界校验输入（白名单、长度上限、参数化 SQL）

### Task 1: 审批工作流（approval）

**Files:**
- Modify: `deploy/init.sql`（追加 `approval` 表）
- Create: `services/gateway/src/model/approval.rs`、`services/gateway/src/model/approval_test.rs`
- Create: `services/gateway/src/approval_api.rs`
- Modify: `services/gateway/src/main.rs`（挂路由）、`services/gateway/src/proxy/k8s_proxy.rs`（删除门禁）

- [ ] **Step 1: 写失败测试（状态机 + 动作白名单）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn next_status_transitions() {
        assert_eq!(next_status("pending", "approve"), Some("approved"));
        assert_eq!(next_status("pending", "reject"), Some("rejected"));
        assert_eq!(next_status("pending", "cancel"), Some("canceled"));
        assert_eq!(next_status("rejected", "reopen"), Some("pending"));
        assert_eq!(next_status("approved", "approve"), None); // 终态
        assert_eq!(next_status("pending", "hack"), None);
    }
    #[test]
    fn action_whitelist() {
        for a in ["approve", "reject", "cancel", "reopen"] { assert!(validate_approval(a)); }
        assert!(!validate_approval("delete"));
        assert!(!validate_approval(""));
    }
}
```

- [ ] **Step 2: 运行确认失败** `cargo test -p superops-gateway approval_test` → FAIL（mod 不存在）

- [ ] **Step 3: 最小实现 model/approval.rs**

```rust
pub const APPROVAL_ACTIONS: [&str; 4] = ["approve", "reject", "cancel", "reopen"];

pub fn validate_approval(action: &str) -> bool { APPROVAL_ACTIONS.contains(&action) }

pub fn next_status(current: &str, action: &str) -> Option<&'static str> {
    Some(match (current, action) {
        ("pending", "approve") => "approved",
        ("pending", "reject") => "rejected",
        ("pending", "cancel") => "canceled",
        ("rejected", "reopen") => "pending",
        _ => return None,
    })
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Approval {
    pub id: i64,
    pub kind: String,
    pub target: String,
    pub operator: String,
    pub reason: String,
    pub status: String,
    pub created_at: chrono::NaiveDateTime,
}
```

- [ ] **Step 4: 运行确认通过**（同上命令）→ PASS

- [ ] **Step 5: deploy/init.sql 追加 approval 表**

```sql
CREATE TABLE IF NOT EXISTS approval (
  id BIGINT AUTO_INCREMENT PRIMARY KEY,
  kind VARCHAR(32) NOT NULL,               -- delete / scale / restart / generic
  target VARCHAR(255) NOT NULL,            -- 目标标识（如 ns/name）
  operator VARCHAR(64) NOT NULL,
  reason VARCHAR(512) NOT NULL DEFAULT '',
  status ENUM('pending','approved','rejected','canceled') NOT NULL DEFAULT 'pending',
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  decided_by VARCHAR(64) NULL,
  decided_at DATETIME NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
```

- [ ] **Step 6: approval_api.rs（3 个 handler，全部参数化 SQL）**

```rust
// GET /api/approvals?status=pending&limit=50 —— status 白名单校验，limit 夹取 1..=500
pub async fn list_approvals(State(st): State<AppState>, Query(q): Query<ListQuery>)
    -> Result<Json<Vec<Approval>>, ApiError> {
    let status_ok = q.status.as_deref().map(|s| ["pending","approved","rejected","canceled"].contains(&s)).unwrap_or(true);
    if !status_ok { return Err(ApiError::bad_request("invalid status")); }
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    // SELECT id,kind,target,operator,reason,status,created_at FROM approval [WHERE status=?] ORDER BY id DESC LIMIT ?
}
// POST /api/approvals {kind,target,reason} —— kind/target 非空、reason ≤512，插入 pending
// POST /api/approvals/{id}/decide {action} —— validate_approval + next_status，UPDATE status=?,decided_by=?,decided_at=NOW() WHERE id=?
// 错误类型沿用现有 handler 统一错误（若为 (StatusCode, String) 则照旧）
```

- [ ] **Step 7: 删除门禁（config `approval.enabled`，默认 false）**

```rust
if cfg.approval.enabled {
    let approved: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM approval WHERE kind='delete' AND target=? AND status='approved'")
        .bind(format!("{}/{}", ns, name)).fetch_one(&pool).await?;
    if approved == 0 {
        return Err(ApiError::failed_precondition("删除需先通过审批 (POST /api/approvals)"));
    }
}
```

- [ ] **Step 8: 挂路由 + 验证**

```rust
.route("/api/approvals", get(list_approvals).post(create_approval))
.route("/api/approvals/{id}/decide", post(decide_approval))
```

`cargo fmt --check && cargo check && cargo test` 全绿。

- [ ] **Step 9:（可选）commit —— 仅当用户要求**

### Task 2: 多租户（tenant）

**Files:**
- Create: `deploy/migrations/002_multi_tenant.sql`
- Create: `services/gateway/src/tenant.rs`、`services/gateway/src/tenant_test.rs`
- Modify: `services/gateway/src/main.rs`（挂 require_tenant）；cmdb/scripts 路由组

- [ ] **Step 1: 写失败测试（tenant 解析 + 白名单）**

```rust
#[test]
fn tenant_parsing() {
    assert_eq!(tenant_from_header("acme"), Some("acme".to_string()));
    assert_eq!(tenant_from_header("Acme!"), None);        // 非法字符
    assert_eq!(tenant_from_header(&"a".repeat(65)), None); // 超长
    assert_eq!(tenant_from_header(""), None);
}
```

- [ ] **Step 2: 运行确认失败** → FAIL（mod 不存在）

- [ ] **Step 3: 最小实现 tenant.rs**

```rust
pub fn tenant_from_header(h: &str) -> Option<String> {
    let h = h.trim();
    if h.is_empty() || h.len() > 64 { return None; }
    h.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        .then(|| h.to_string())
}
// 注：若 ecat-auth AuthClaims 未含 tenant 字段，租户经请求头 X-Tenant-Id 传递；
// 后续若 AuthClaims 增加 tenant claim，则 tenant_from_claims(claims) 优先 claim、回退头、再回退 'default'。
```

- [ ] **Step 4: 运行确认通过** → PASS

- [ ] **Step 5: 迁移 SQL（在 P5 建表基础上追加列）**

```sql
ALTER TABLE users ADD COLUMN tenant_id VARCHAR(64) NOT NULL DEFAULT 'default';
ALTER TABLE cmdb_asset ADD COLUMN tenant_id VARCHAR(64) NOT NULL DEFAULT 'default';
ALTER TABLE script ADD COLUMN tenant_id VARCHAR(64) NOT NULL DEFAULT 'default';
CREATE INDEX idx_cmdb_tenant ON cmdb_asset(tenant_id);
```

- [ ] **Step 6: require_tenant 中间件 + 查询带租户过滤**

```rust
pub async fn require_tenant(State(st): State<AppState>, headers: HeaderMap,
    mut req: Request, next: Next) -> Result<Response, ApiError> {
    let t = headers.get("x-tenant-id").and_then(|v| v.to_str().ok())
        .and_then(tenant_from_header).unwrap_or_else(|| "default".to_string());
    req.extensions_mut().insert(Tenant(t));
    Ok(next.run(req).await)
}
// cmdb/scripts 全部 SELECT/UPDATE/DELETE 追加 AND tenant_id = ?（参数化绑定）
```

- [ ] **Step 7: 挂载 + 验证**：`/api/cmdb/*`、`/api/scripts/*` 前挂 `axum::middleware::from_fn_with_state`；`cargo fmt --check && cargo check && cargo test` 全绿。

- [ ] **Step 8:（可选）commit —— 仅当用户要求**

### Task 3: 凭据保险库（secrets vault）

**Files:**
- Modify: `services/gateway/Cargo.toml`（aes-gcm 0.10、rand —— 以 workspace 既有版本为准）
- Create: `services/gateway/src/vault.rs`、`services/gateway/src/vault_test.rs`
- Create: `services/gateway/src/secrets_api.rs`
- Modify: `services/gateway/src/main.rs`（读 SUPEROPS_MASTER_KEY 入 AppState + 路由）、`deploy/init.sql`（secret 表）

- [ ] **Step 1: 写失败测试（加密往返 + 篡改检测）**

```rust
#[test]
fn roundtrip() {
    let key = [7u8; 32];
    let blob = encrypt_value(&key, "s3cret-password").unwrap();
    assert_eq!(decrypt_value(&key, &blob).unwrap(), "s3cret-password");
    let mut bad = blob.clone();
    *bad.last_mut().unwrap() ^= 0x01;          // 篡改密文
    assert!(decrypt_value(&key, &bad).is_err());
    assert!(decrypt_value(&key, b"short").is_err());
}
```

- [ ] **Step 2: 运行确认失败** → FAIL（vault mod 不存在）

- [ ] **Step 3: 最小实现 vault.rs**

```rust
use aes_gcm::{Aes256Gcm, Key, Nonce, aead::{Aead, KeyInit}};
use rand::RngCore;

const NONCE_LEN: usize = 12;

pub fn encrypt_value(key: &[u8], plaintext: &str) -> Result<Vec<u8>, String> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let mut nonce = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce);
    let ct = cipher.encrypt(Nonce::from_slice(&nonce), plaintext.as_bytes())
        .map_err(|e| e.to_string())?;
    let mut out = nonce.to_vec();
    out.extend_from_slice(&ct);
    Ok(out)
}

pub fn decrypt_value(key: &[u8], blob: &[u8]) -> Result<String, String> {
    if blob.len() < NONCE_LEN + 1 { return Err("ciphertext too short".into()); }
    let (nonce, ct) = blob.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let pt = cipher.decrypt(Nonce::from_slice(nonce), ct).map_err(|e| e.to_string())?;
    String::from_utf8(pt).map_err(|e| e.to_string())
}
```

- [ ] **Step 4: 运行确认通过** → PASS

- [ ] **Step 5: init.sql 追加 secret 表**

```sql
CREATE TABLE IF NOT EXISTS secret (
  id BIGINT AUTO_INCREMENT PRIMARY KEY,
  name VARCHAR(128) NOT NULL UNIQUE,
  ciphertext BLOB NOT NULL,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
```

- [ ] **Step 6: secrets_api.rs（4 个端点）**

```rust
// POST /api/secrets {name,value}  —— name 白名单 ^[a-zA-Z0-9_.-]{1,128}$，value ≤64KB；存在则 UPDATE
// GET  /api/secrets              —— 仅返回 [{name, created_at}]，绝不返回明文
// GET  /api/secrets/{name}       —— 解密后返回 {value}（解密失败 → 500 且不泄漏密文）
// DELETE /api/secrets/{name}
// 主密钥缺失（AppState.master_key = None）→ 统一 503 {"error":"master key not configured"}
```

- [ ] **Step 7: 主密钥装载（main.rs）**

```rust
let master_key = std::env::var("SUPEROPS_MASTER_KEY").ok()
    .and_then(|k| (k.as_bytes().len() == 32).then(|| k.into_bytes()));
if master_key.is_none() { tracing::warn!("SUPEROPS_MASTER_KEY 未设置或非 32 字节，/api/secrets 将返回 503"); }
// 测试环境示例值（仅测试用，绝不入库）："0123456789abcdef0123456789abcdef"
```

- [ ] **Step 8: 挂路由 + 验证**：`cargo fmt --check && cargo check && cargo test` 全绿；手工 curl 增/查/删一次。

- [ ] **Step 9:（可选）commit —— 仅当用户要求**

### Task 4: 运维治理（备份 / 容量 / 成本）

**Files:**
- Create: `services/collector/src/housekeeping.rs`、`services/collector/src/housekeeping_test.rs`
- Modify: `services/collector/src/main.rs`（挂调度任务）、`config/collector.yaml`（cron）

- [ ] **Step 1: 写失败测试（成本估算 + 备份文件名校验）**

```rust
#[test]
fn cost_estimate() {
    // 2 核 × ¥0.5/h + 4 GiB × ¥0.25/h（0.5/0.25 均为精确二进制小数），按天 = (1.0 + 1.0) * 24
    assert_eq!(estimate_cost(2.0, 4.0, 0.5, 0.25), 48.0);
}
#[test]
fn backup_name_pattern() {
    assert!(is_backup_file("superops-2026-08-06-0300.sql.gz"));
    assert!(!is_backup_file("../evil.sql.gz"));
    assert!(!is_backup_file("notes.txt"));
}
```

- [ ] **Step 2: 运行确认失败** → FAIL（housekeeping mod 不存在）

- [ ] **Step 3: 最小实现 housekeeping.rs**

```rust
pub fn estimate_cost(cpu_cores: f64, mem_gib: f64, cpu_price: f64, mem_price: f64) -> f64 {
    (cpu_cores * cpu_price + mem_gib * mem_price) * 24.0   // 小时价 → 每天
}

pub fn is_backup_file(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".sql.gz") else { return false };
    stem.starts_with("superops-") && !stem.contains('/') && !stem.contains("..")
}

// backup_mysql(cfg)：mysqldump -h HOST -P PORT -uUSER -pPASS DB | gzip > data/backups/superops-{ts}.sql.gz
//   然后 glob data/backups/*.sql.gz 按 mtime 排序，保留最新 7 份，删除其余
// collect_capacity(client, ch)：list 全部 Node → 求和 allocatable cpu(milli→core) 与 memory(Ki→GiB)
//   → DataPoint::new("capacity_snapshot").with_tag("cluster","default")
//       .with_field("cpu_cores", cores).with_field("mem_gib", gib).with_timestamp(now_secs())
//   → TsdbClient::write（与 alert.rs 同构）
```

- [ ] **Step 4: 运行确认通过** → PASS

- [ ] **Step 5: 挂调度（沿用 ecat-scheduler 现有 collect/inspect 任务模式）**

```yaml
# config/collector.yaml 追加
jobs:
  housekeeping:
    cron: "0 3 * * *"      # 每天 03:00
    enabled: true
```

main.rs 按既有 Scheduler 注册模式把 housekeeping 任务加入调度（与 inspect 任务同构）。

- [ ] **Step 6: 验证**：`cargo fmt --check && cargo check && cargo test` 全绿；housekeeping 单测通过。

- [ ] **Step 7:（可选）commit —— 仅当用户要求**

### Task 5: 终端录制 + 文件传输（recording + files）

**Files:**
- Modify: `deploy/docker-compose.yml`（MinIO）、`deploy/.env.example`（MINIO_*）
- Create: `services/collector/src/recorder.rs`、`services/collector/src/recorder_test.rs`
- Modify: `services/collector/src/main.rs`（录制帧写 ClickHouse）
- Create: `services/gateway/src/recordings_api.rs`、`services/gateway/src/files_api.rs`（含白名单测试）
- Modify: `services/gateway/src/main.rs`（路由 + data/uploads 目录）

- [ ] **Step 1: 写失败测试（session_id / 帧上限 / 文件名白名单）**

```rust
#[test]
fn session_id_format() {
    let s = session_id();
    assert_eq!(s.len(), 36);
    assert_eq!(s.chars().filter(|&c| c == '-').count(), 4);
}
#[test]
fn frame_size_cap() {
    assert!(frame_valid(&vec![0u8; 256 * 1024]));
    assert!(!frame_valid(&vec![0u8; 256 * 1024 + 1]));
}
#[test]
fn filename_whitelist() {
    assert!(valid_filename("report-2026.pdf"));
    assert!(valid_filename("a.b_c-1.txt"));
    assert!(!valid_filename("../etc/passwd"));
    assert!(!valid_filename("a/b"));
    assert!(!valid_filename(""));
    assert!(!valid_filename(&"x".repeat(256)));
}
```

- [ ] **Step 2: 运行确认失败** → FAIL（mod 不存在）

- [ ] **Step 3: 最小实现 recorder.rs 校验函数**

```rust
pub fn session_id() -> String {
    let mut b = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut b);
    uuid::Builder::from_bytes(b).as_uuid().to_string()
}
pub fn frame_valid(frame: &[u8]) -> bool { !frame.is_empty() && frame.len() <= 256 * 1024 }

pub fn valid_filename(name: &str) -> bool {
    !name.is_empty() && name.len() <= 255
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        && !name.starts_with('.') && !name.contains("..")
}
// session_id 复用 workspace 既有 uuid/rand；若 workspace 无 uuid，则用 rand 手工拼 36 位 hex+dash
```

- [ ] **Step 4: 运行确认通过** → PASS

- [ ] **Step 5: compose 追加 MinIO（:9002 / 控制台 :9003）**

```yaml
  minio:
    image: minio/minio:latest
    command: server /data --console-address ":9003"
    ports: ["9002:9000", "9003:9001"]
    environment:
      MINIO_ROOT_USER: ${MINIO_ROOT_USER:-minioadmin}
      MINIO_ROOT_PASSWORD: ${MINIO_ROOT_PASSWORD:-minioadmin}
    volumes: [minio-data:/data]
```

`.env.example` 追加 `MINIO_ROOT_USER` / `MINIO_ROOT_PASSWORD`（默认 minioadmin，注释「生产必改」）。

- [ ] **Step 6: 录制帧落库（collector）**

exec 会话期间每帧（≤256KB，超限截断）→ `DataPoint::new("exec_session").with_tag("kind","recording").with_tag("session_id", sid).with_tag("node", node).with_field("frame_b64", base64).with_timestamp(now_secs())` → `TsdbClient::write`。接线：以现有 exec 桥接数据通路为准，在 gateway → k8s exec 旁路中挂 recorder 通道。

- [ ] **Step 7: recordings_api.rs / files_api.rs（4 + 2 端点）**

```rust
// GET  /api/recordings?limit=50        —— ClickHouse exec_session(kind=recording) 按 session_id 聚合列出
// GET  /api/recordings/{sid}/frames    —— 按时间序取该会话全部帧（base64 → 前端回放）
// DELETE /api/recordings/{sid}         —— 删除（ClickHouse DELETE 语法以 clickhouse crate 版本为准）
// POST /api/files                     —— axum Multipart：valid_filename + ≤10MB → data/uploads/{name}
// GET  /api/files/{name}              —— 二次 valid_filename 校验后读取（防路径穿越），异常 404
```

- [ ] **Step 8: 挂路由 + 验证**：`cargo fmt --check && cargo check && cargo test` 全绿；curl 上传/下载往返一次。

- [ ] **Step 9:（可选）commit —— 仅当用户要求**

### Task 6: gRPC / 调度 OTLP 追踪补齐

**Files:**
- Modify: `services/k8s/src/main.rs`、`services/collector/src/main.rs`（OTLP init）
- Modify: `services/k8s/src/service.rs`、`services/collector/src/collect.rs`、`services/collector/src/inspect.rs`（instrument）
- Modify: `config/k8s-service.yaml`、`config/collector.yaml`

- [ ] **Step 1: 复制 gateway OTLP init 片段**

从 `services/gateway/src/main.rs` 复制 `ecat-tracing-otlp` 初始化片段（config `tracing.otlp.endpoint` 读取 + global subscriber 安装），按各服务 `App::builder()` 生命周期适配 init 时机接入 k8s 与 collector 的 main.rs。

- [ ] **Step 2: 关键路径插桩**

```rust
#[tracing::instrument(skip(self, request))]
async fn get_pod(&self, request: Request<GetPodRequest>) -> GrpcResult<GetPodResponse> { ... }
// 同法：get_pod_logs / watch_pods / exec_pod（service.rs）
// collector：#[tracing::instrument(skip_all)] fn collect_once(...)、inspect_once(...)
```

- [ ] **Step 3: 配置追加**

```yaml
# config/k8s-service.yaml 与 config/collector.yaml 追加
tracing:
  otlp:
    endpoint: "http://localhost:4317"   # compose 内联为 http://jaeger:4317
```

- [ ] **Step 4: 验证**：`cargo fmt --check && cargo check && cargo test` 全绿；起 Jaeger 后调用一次 k8s RPC，UI 可见 span。

- [ ] **Step 5:（可选）commit —— 仅当用户要求**

### Task 7: Helm chart 文件（仅文件，不打包）

**Files:**
- Create: `deploy/helm/superops/Chart.yaml`、`values.yaml`、`templates/deployment-gateway.yaml`、`templates/deployment-k8s.yaml`、`templates/deployment-collector.yaml`、`templates/service.yaml`、`templates/configmap.yaml`、`templates/NOTES.txt`

- [ ] **Step 1: Chart.yaml + values.yaml**

```yaml
# Chart.yaml
apiVersion: v2
name: superops
description: SuperOps 智能运维平台（gateway / k8s-service / collector）
type: application
version: 0.1.0
appVersion: "1.2.0"

# values.yaml（示例镜像仓库；构建/推送由用户自行决定）
image:
  repository: registry.example.com/superops
  tag: "1.2.0"
  pullPolicy: IfNotPresent
replicas:
  gateway: 1
  k8s: 1
  collector: 1
config: {}        # 覆盖 configmap 的额外键值；密码仅经 env / Secret 注入，不落 configmap
```

- [ ] **Step 2: deployment-gateway.yaml（k8s/collector 同构，改 name/image/args）**

```yaml
apiVersion: apps/v1
kind: Deployment
metadata: { name: superops-gateway }
spec:
  replicas: {{ .Values.replicas.gateway }}
  selector: { matchLabels: { app: superops-gateway } }
  template:
    metadata: { labels: { app: superops-gateway } }
    spec:
      containers:
        - name: gateway
          image: "{{ .Values.image.repository }}/gateway:{{ .Values.image.tag }}"
          imagePullPolicy: {{ .Values.image.pullPolicy }}
          envFrom: [{ configMapRef: { name: superops-config } }]
          ports: [{ containerPort: 8080 }]
          livenessProbe: { httpGet: { path: /health, port: 8080 } }
          readinessProbe: { httpGet: { path: /ready, port: 8080 } }
```

- [ ] **Step 3: service.yaml + configmap.yaml + NOTES.txt**

```yaml
# service.yaml —— ClusterIP；gateway 8080 / k8s 9091
# configmap.yaml —— 挂 GATEWAY_CONFIG/K8S_CONFIG/COLLECTOR_CONFIG 指向的 YAML（values.config 注入）
# NOTES.txt —— 提示：密码请用外部 Secret 注入，不写入 configmap
```

- [ ] **Step 4: 校验**：`helm lint deploy/helm/superops`（无 helm 环境则跳过并注明）。**不做任何打包/推送**。

- [ ] **Step 5:（可选）commit —— 仅当用户要求**

### Task 8: 前端告警 / 指标页

**Files:**
- Create: `services/gateway/src/alerts_api.rs`
- Modify: `services/gateway/src/main.rs`（路由）
- Create: `frontend/src/pages/ops/alerts.tsx`、`frontend/src/pages/ops/metrics.tsx`
- Modify: `frontend/src/App.tsx`（menuData）、`frontend/src/services/api.ts`

- [ ] **Step 1: gateway alerts_api.rs（无新依赖）**

```rust
// GET /api/alerts?level=&limit=50 —— ClickHouse alert_event SELECT（level 白名单 info/warning/critical，limit 夹取 1..=500）
// POST /api/alerts/{id}/ack       —— Redis SETEX alert:ack:{id} 1h（Redis 不可用回退内存 HashMap<u64, Instant>）
// GET /api/alerts/acks            —— 返回已 ack 的 id 列表（供前端置灰）
// ClickHouse SELECT 语法以 Cargo.lock 中 clickhouse crate 版本与 collector/ch.rs 用法为准
```

- [ ] **Step 2: 前端 alerts.tsx**

```tsx
// antd Table + Tag：level → info(蓝)/warning(橙)/critical(红)；「确认」按钮 → POST /api/alerts/{id}/ack
// 10s 轮询 /api/alerts 与 /api/alerts/acks；已 ack 行置灰并隐藏按钮
// services/api.ts 追加 listAlerts(level?), ackAlert(id), listAcks()
```

- [ ] **Step 3: 前端 metrics.tsx（复用 /api/v1/metrics/query，无新依赖）**

```tsx
// 指标选择器（cpu_usage / mem_usage 等，series 白名单以后端支持为准）
// StatisticCard 展示最近值 + 趋势折线
```

- [ ] **Step 4: App.tsx menuData 追加**

```tsx
{ path: '/ops', name: '运维中心', icon: <SettingOutlined />, children: [
  { path: '/ops/alerts', name: '告警中心' },
  { path: '/ops/metrics', name: '指标看板' },
]}
```

- [ ] **Step 5: 验证**：`cd frontend && npm run build` 通过；`cargo fmt --check && cargo check && cargo test` 全绿。

- [ ] **Step 6:（可选）commit —— 仅当用户要求**

### Task 9: 文档同步 + 全量验证

**Files:**
- Modify: `README.md`、`README.en.md`、`CHANGELOG.md`、`docs/audit-report-2026-08-06.md`

- [ ] **Step 1: 文档同步**

- README / README.en：功能表追加（审批 / 多租户 / 保险库 / 治理 / 录制 / 文件 / 告警 / 指标页）、端口表追加 MinIO 9002/9003、项目结构补 `deploy/helm/` 与 ops 页面、已知边界更新（主密钥与审批开关默认关闭）
- `config/` 各示例 YAML 与新配置块同步
- CHANGELOG 追加 P6 条目

- [ ] **Step 2: 全量验证**

```bash
cargo fmt --check && cargo check && cargo test
cd frontend && npm run build
```

预期：全绿；workspace 测试数 ≥ 296 + P4/P5/P6 新增。

- [ ] **Step 3: 审计报告更新**

`docs/audit-report-2026-08-06.md` 追加 P6 验证记录（新增端点清单；安全校验点：SQL 参数化 / 文件名白名单 / 密文不泄露 / 主密钥 503 / 审批门禁），更新测试计数与测试矩阵。

- [ ] **Step 4:（可选）commit —— 仅当用户要求**

---

## P6 完成标准

- [ ] `cargo fmt --check && cargo check && cargo test` 全绿，测试数 ≥ 296 + 新增
- [ ] `cd frontend && npm run build` 通过
- [ ] 三服务可启动：`/api/approvals`、`/api/secrets`（未配置 503 / 配置后 200 两态）、`/api/alerts`、`/api/files` 可访问
- [ ] ClickHouse 可查询 `exec_session`(recording) 与 `alert_event`；Jaeger 可见 k8s/collector span
- [ ] `helm lint` 通过（如本地有 helm）；未做任何镜像打包/推送
- [ ] README / 审计报告已同步

## 三册收尾（P4 + P5 + P6 总验收）

- **P4**：k8s 写操作（scale/restart/delete）+ 审批门禁、collector 通知（钉钉/企业微信/通用）、网关审计查询、用户管理（list/status）、前端 ops 三页
- **P5**：日志入库与查询（logtail + logs_api）、CMDB 资产、脚本任务（Job）、RBAC 三角色
- **P6**：审批流、多租户、凭据保险库、治理任务（备份/容量/成本）、终端录制 + 文件传输、OTLP 追踪补齐、Helm chart 文件、前端告警/指标页

**交付物**：14 领域缺口矩阵全部落地；新测试全部入 workspace；README（中英）、CHANGELOG、audit report 同步；Helm 文件在 `deploy/helm/`，未打包。

**最终验证命令：**

```bash
cargo fmt --check && cargo check && cargo test
cd frontend && npm run build
```

全绿即视为 P6（及全量扩展）完成。若任一验证失败，回到对应 Task 修复并复验。
