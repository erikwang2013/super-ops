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
