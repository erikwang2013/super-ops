use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use sqlx::mysql::MySqlPool;

pub const BACKUP_STATUSES: [&str; 3] = ["running", "ok", "failed"];

pub fn validate_backup_report(db_name: &str, status: &str, size_bytes: i64) -> Result<(), String> {
    let n = db_name.chars().count();
    if !(1..=128).contains(&n) {
        return Err(format!("db_name 长度需 1..=128，当前 {n}"));
    }
    if !BACKUP_STATUSES.contains(&status) {
        return Err(format!("status 必须为 {:?} 之一", BACKUP_STATUSES));
    }
    if !(0..=1_000_000_000_000).contains(&size_bytes) {
        return Err(format!("size_bytes 超出范围: {size_bytes}"));
    }
    Ok(())
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct BackupStatusRow {
    pub id: i64,
    pub db_name: String,
    pub target: String,
    pub status: String,
    pub size_bytes: i64,
    pub message: String,
    pub started_at: String,
    pub finished_at: Option<String>,
}

/// 每个库最近一次备份（用于健康度展示：status / age_hours）。
#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct BackupSummaryRow {
    pub db_name: String,
    pub status: String,
    pub target: String,
    pub size_bytes: i64,
    pub last_ok_at: String,
    pub age_hours: i64,
}

pub async fn report_backup(
    pool: &MySqlPool,
    db_name: &str,
    target: &str,
    status: &str,
    size_bytes: i64,
    message: &str,
) -> sqlx::Result<i64> {
    let r = sqlx::query(
        "INSERT INTO backup_status (db_name, target, status, size_bytes, message, finished_at) \
         VALUES (?, ?, ?, ?, ?, NOW())",
    )
    .bind(db_name)
    .bind(target)
    .bind(status)
    .bind(size_bytes)
    .bind(message)
    .execute(pool)
    .await?;
    Ok(r.last_insert_id() as i64)
}

pub async fn list_backups(pool: &MySqlPool, limit: i64) -> sqlx::Result<Vec<BackupStatusRow>> {
    sqlx::query_as(
        "SELECT id, db_name, target, status, size_bytes, message, \
         DATE_FORMAT(started_at, '%Y-%m-%d %H:%i:%s'), \
         DATE_FORMAT(finished_at, '%Y-%m-%d %H:%i:%s') \
         FROM backup_status ORDER BY id DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
}

pub async fn backup_summary(pool: &MySqlPool) -> sqlx::Result<Vec<BackupSummaryRow>> {
    // 每组 db_name 取最近一条记录（含 ok 状态与年龄），供健康度展示
    sqlx::query_as(
        "SELECT b.db_name, b.status, b.target, b.size_bytes, \
         DATE_FORMAT(b.finished_at, '%Y-%m-%d %H:%i:%s') AS last_ok_at, \
         TIMESTAMPDIFF(HOUR, b.finished_at, NOW()) AS age_hours \
         FROM backup_status b \
         JOIN (SELECT db_name, MAX(id) AS max_id FROM backup_status GROUP BY db_name) t \
           ON t.db_name = b.db_name AND t.max_id = b.id \
         ORDER BY b.db_name",
    )
    .fetch_all(pool)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_accepts_valid_report() {
        assert!(validate_backup_report("orders", "ok", 10485760).is_ok());
        assert!(validate_backup_report("orders", "running", 0).is_ok());
    }

    #[test]
    fn validate_rejects_bad_report() {
        assert!(validate_backup_report("", "ok", 0).is_err());
        assert!(validate_backup_report(&"a".repeat(129), "ok", 0).is_err());
        assert!(validate_backup_report("orders", "pending", 0).is_err());
        assert!(validate_backup_report("orders", "ok", -1).is_err());
        assert!(validate_backup_report("orders", "ok", 1_000_000_000_001).is_err());
    }
}
