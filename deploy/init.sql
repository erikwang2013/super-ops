-- 索引内嵌在 CREATE TABLE 中而非独立 CREATE INDEX：
-- MySQL 8 不支持 CREATE INDEX IF NOT EXISTS，独立语句重跑会中断
CREATE TABLE IF NOT EXISTS users (
    id VARCHAR(36) PRIMARY KEY,
    username VARCHAR(64) NOT NULL UNIQUE,
    email VARCHAR(128) NOT NULL UNIQUE,
    password_hash VARCHAR(255) NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    KEY idx_users_username (username),
    KEY idx_users_email (email)
);

CREATE TABLE IF NOT EXISTS api_keys (
    id VARCHAR(36) PRIMARY KEY,
    user_id VARCHAR(36) NOT NULL,
    name VARCHAR(64) NOT NULL,
    key_hash CHAR(64) NOT NULL UNIQUE,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    KEY idx_api_keys_user (user_id),
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

-- P4-9: 用户管理所需列（role 预留 P5-6 RBAC，status 由用户管理 API 维护）
-- 首个注册用户自动提升为 admin（register 时判断 count=0）；角色: admin/operator/viewer
ALTER TABLE users ADD COLUMN role VARCHAR(32) NOT NULL DEFAULT 'viewer';
ALTER TABLE users ADD COLUMN status VARCHAR(16) NOT NULL DEFAULT 'enabled';

-- P5-3: CMDB 资产表（asset_type+name 唯一，upsert 用）
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

-- P5-5: 批量执行 — 脚本库与 k8s Job
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

-- P6-1: 审批工作流（删除 deployment 等危险操作的事前审批）
CREATE TABLE IF NOT EXISTS approval (
  id BIGINT AUTO_INCREMENT PRIMARY KEY,
  kind VARCHAR(32) NOT NULL,               -- delete / scale / restart / generic
  target VARCHAR(255) NOT NULL,            -- 目标标识（如 cluster_id/ns/name）
  operator VARCHAR(64) NOT NULL,
  reason VARCHAR(512) NOT NULL DEFAULT '',
  status ENUM('pending','approved','rejected','canceled') NOT NULL DEFAULT 'pending',
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  decided_by VARCHAR(64) NULL,
  decided_at DATETIME NULL,
  KEY idx_approval_gate (kind, status, target)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- P6-2: 多租户（tenant）迁移
-- 注：仓库无 deploy/migrations/ 目录，compose 仅挂载 init.sql，
-- 故本迁移 SQL 追加到 init.sql 尾部（与 P4-9 的 ALTER TABLE 追加方式一致）。
-- 非法/缺失 x-tenant-id 请求回退为 'default' 租户，存量数据归入 default。
-- uk_name_type 改为 (tenant_id, asset_type, name) 租户内唯一：
-- upsert 的 ON DUPLICATE KEY UPDATE 只命中同租户行，跨租户同名资产不互相覆写。
-- 存量数据均 tenant_id='default' 且 (asset_type,name) 全局唯一，DROP+ADD 无冲突。
-- idx_script_tenant 覆盖 script 表按租户过滤的查询。
ALTER TABLE users ADD COLUMN tenant_id VARCHAR(64) NOT NULL DEFAULT 'default';
ALTER TABLE cmdb_asset ADD COLUMN tenant_id VARCHAR(64) NOT NULL DEFAULT 'default';
ALTER TABLE script ADD COLUMN tenant_id VARCHAR(64) NOT NULL DEFAULT 'default';
ALTER TABLE cmdb_asset DROP INDEX uk_name_type;
ALTER TABLE cmdb_asset ADD UNIQUE KEY uk_name_type (tenant_id, asset_type, name);
CREATE INDEX idx_script_tenant ON script(tenant_id);

-- P6-3: 凭据保险库（secret 值经 SUPEROPS_MASTER_KEY AES-256-GCM 加密后落库）
CREATE TABLE IF NOT EXISTS secret (
  id BIGINT AUTO_INCREMENT PRIMARY KEY,
  name VARCHAR(128) NOT NULL UNIQUE,
  ciphertext BLOB NOT NULL,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- P6-4: 告警规则引擎（替代 collector 硬编码规则；collector 无 MySQL 时回退内置默认）
CREATE TABLE IF NOT EXISTS alert_rule (
  id BIGINT AUTO_INCREMENT PRIMARY KEY,
  name VARCHAR(128) NOT NULL UNIQUE,
  metric VARCHAR(32) NOT NULL,      -- node_not_ready/node_ready_pct/pod_not_running/pod_running_pct/deployment_unavailable/deployment_ready_pct
  operator VARCHAR(4) NOT NULL DEFAULT 'ge',  -- ge(>=) / le(<=)，与 threshold 比较决定是否触发
  threshold VARCHAR(16) NOT NULL DEFAULT '1', -- 计数类为数量(0-1000)，pct 类为百分比(0-100)
  level VARCHAR(8) NOT NULL DEFAULT 'WARN',   -- INFO / WARN / CRIT
  action VARCHAR(16) NOT NULL DEFAULT 'notify', -- notify / restart / scale（scale/restart 由 P6-5 自愈执行）
  enabled TINYINT(1) NOT NULL DEFAULT 1,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- 内置默认规则（与旧硬编码 evaluate_health 语义等价；可编辑/禁用/删除）
INSERT INTO alert_rule (name, metric, operator, threshold, level) VALUES
  ('node-not-ready', 'node_not_ready', 'ge', '1', 'CRIT'),
  ('pod-not-running', 'pod_not_running', 'ge', '1', 'WARN'),
  ('deployment-unavailable', 'deployment_unavailable', 'ge', '1', 'WARN')
ON DUPLICATE KEY UPDATE metric = VALUES(metric);

-- P6-6: 值班排班（oncall shift；current 查询用 start_at<=NOW() AND end_at>=NOW()）
CREATE TABLE IF NOT EXISTS oncall_schedule (
  id BIGINT AUTO_INCREMENT PRIMARY KEY,
  name VARCHAR(128) NOT NULL,
  assignee VARCHAR(64) NOT NULL,
  start_at DATETIME NOT NULL,
  end_at DATETIME NOT NULL,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  KEY idx_oncall_time (start_at, end_at)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- C1: 工单（ticket；告警中心一键建单 source='alert' 关联 alert_title）
CREATE TABLE IF NOT EXISTS ticket (
  id BIGINT AUTO_INCREMENT PRIMARY KEY,
  title VARCHAR(128) NOT NULL,
  description TEXT,
  severity VARCHAR(8) NOT NULL DEFAULT 'LOW',    -- LOW / MEDIUM / HIGH / CRIT
  status VARCHAR(16) NOT NULL DEFAULT 'open',    -- open / assigned / resolved / closed
  assignee VARCHAR(64) NOT NULL DEFAULT '',
  source VARCHAR(16) NOT NULL DEFAULT 'manual',  -- manual / alert
  alert_title VARCHAR(128) NOT NULL DEFAULT '',  -- source='alert' 时记录来源告警标题
  created_by VARCHAR(64) NOT NULL DEFAULT '',
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  KEY idx_ticket_status (status)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- C4: 发布记录（release；发布流水线每次镜像更新落一条审计；release 为 MySQL 保留字须反引号）
CREATE TABLE IF NOT EXISTS `release` (
  id BIGINT AUTO_INCREMENT PRIMARY KEY,
  cluster_id VARCHAR(64) NOT NULL DEFAULT 'default',
  namespace VARCHAR(128) NOT NULL,
  name VARCHAR(128) NOT NULL,
  old_image VARCHAR(255) NOT NULL DEFAULT '',
  new_image VARCHAR(255) NOT NULL,
  operator VARCHAR(64) NOT NULL DEFAULT '',
  status VARCHAR(16) NOT NULL DEFAULT 'pending',  -- pending / rolling / ok / failed
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  KEY idx_release_target (cluster_id, namespace, name)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- C2: Runbook 剧本（runbook；steps 为 JSON 数组 [{name,script_id,timeout_s}]，按序执行）
CREATE TABLE IF NOT EXISTS runbook (
  id BIGINT AUTO_INCREMENT PRIMARY KEY,
  tenant_id VARCHAR(64) NOT NULL DEFAULT 'default',
  name VARCHAR(128) NOT NULL,
  description TEXT,
  steps JSON NOT NULL,
  created_by VARCHAR(64) NOT NULL DEFAULT '',
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  KEY idx_runbook_tenant (tenant_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- C2: 剧本执行记录（runbook_run；status: pending / running / ok / failed）
CREATE TABLE IF NOT EXISTS runbook_run (
  id BIGINT AUTO_INCREMENT PRIMARY KEY,
  runbook_id BIGINT NOT NULL,
  target_pods VARCHAR(255) NOT NULL DEFAULT '',
  status VARCHAR(16) NOT NULL DEFAULT 'pending',
  output TEXT,
  started_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  finished_at DATETIME NULL,
  KEY idx_runbook_run_rb (runbook_id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- C5: DB 备份状态（backup_status；备份 agent 上报，展示最近备份健康度）
CREATE TABLE IF NOT EXISTS backup_status (
  id BIGINT AUTO_INCREMENT PRIMARY KEY,
  db_name VARCHAR(128) NOT NULL,
  target VARCHAR(255) NOT NULL DEFAULT '',
  status VARCHAR(16) NOT NULL DEFAULT 'running',  -- running / ok / failed
  size_bytes BIGINT NOT NULL DEFAULT 0,
  message VARCHAR(255) NOT NULL DEFAULT '',
  started_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  finished_at DATETIME NULL,
  KEY idx_backup_db (db_name, id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- 混沌演练（chaos_experiment；action: restart / delete，target_type 固定 deployment；status: idle / running / completed / failed）
CREATE TABLE IF NOT EXISTS chaos_experiment (
  id BIGINT AUTO_INCREMENT PRIMARY KEY,
  name VARCHAR(64) NOT NULL,
  cluster_id VARCHAR(64) NOT NULL DEFAULT 'default',
  target_type VARCHAR(16) NOT NULL DEFAULT 'deployment',
  target_name VARCHAR(128) NOT NULL,
  action VARCHAR(16) NOT NULL,
  status VARCHAR(16) NOT NULL DEFAULT 'idle',
  operator VARCHAR(64) NOT NULL DEFAULT '',
  error TEXT,
  started_at DATETIME NULL,
  ended_at DATETIME NULL,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  KEY idx_chaos_status (status)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- P7-3: 资源配额 — 集群命名空间配额登记（cluster_id + namespace 唯一）
CREATE TABLE IF NOT EXISTS resource_quota (
    id BIGINT AUTO_INCREMENT PRIMARY KEY,
    cluster_id VARCHAR(64) NOT NULL DEFAULT 'default',
    namespace VARCHAR(64) NOT NULL,
    cpu_request VARCHAR(32) DEFAULT '',
    memory_request VARCHAR(32) DEFAULT '',
    cpu_limit VARCHAR(32) DEFAULT '',
    memory_limit VARCHAR(32) DEFAULT '',
    replicas INT DEFAULT 0,
    description VARCHAR(256) DEFAULT '',
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    UNIQUE KEY uk_quota (cluster_id, namespace)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;

-- 阶段二：k8s-service 集群注册持久化（Phase 2 persistence）
-- kubeconfig 为注册凭据；生产部署建议配合磁盘加密或后续 at-rest 加密改造
CREATE TABLE IF NOT EXISTS `cluster` (
    id VARCHAR(36) PRIMARY KEY,
    name VARCHAR(128) NOT NULL,
    kubeconfig MEDIUMTEXT NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;
