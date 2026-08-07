use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use sqlx::mysql::MySqlPool;

pub const CHAOS_ACTIONS: [&str; 2] = ["restart", "delete"];

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct ChaosRow {
    pub id: i64,
    pub name: String,
    pub cluster_id: String,
    pub target_type: String,
    pub target_name: String,
    pub action: String,
    pub status: String,
    pub operator: String,
    pub error: Option<String>,
    // sqlx 的 chrono 仅启用 clock 特性（无 serde），时间列用字符串 + SQL 格式化（同 release.rs 约定）
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub created_at: String,
}

const SELECT_COLS: &str = "SELECT id, name, cluster_id, target_type, target_name, action, status, \
     operator, error, DATE_FORMAT(started_at, '%Y-%m-%d %H:%i:%s'), \
     DATE_FORMAT(ended_at, '%Y-%m-%d %H:%i:%s'), DATE_FORMAT(created_at, '%Y-%m-%d %H:%i:%s')";

pub fn validate_chaos(name: &str, target_name: &str, action: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 64 {
        return Err("name 必须为 1..64 字符".into());
    }
    if target_name.is_empty() || target_name.len() > 128 {
        return Err("target_name 必须为 1..128 字符".into());
    }
    if !CHAOS_ACTIONS.contains(&action) {
        return Err(format!("action 必须是 {:?} 之一", CHAOS_ACTIONS));
    }
    Ok(())
}

pub async fn list_chaos(pool: &MySqlPool, limit: i64) -> sqlx::Result<Vec<ChaosRow>> {
    let limit = limit.clamp(1, 500);
    sqlx::query_as::<_, ChaosRow>(&format!(
        "{SELECT_COLS} FROM chaos_experiment ORDER BY id DESC LIMIT {limit}"
    ))
    .fetch_all(pool)
    .await
}

pub async fn insert_chaos(
    pool: &MySqlPool,
    name: &str,
    cluster_id: &str,
    target_name: &str,
    action: &str,
    operator: &str,
) -> sqlx::Result<i64> {
    let r = sqlx::query(
        "INSERT INTO chaos_experiment (name, cluster_id, target_name, action, operator) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(name)
    .bind(cluster_id)
    .bind(target_name)
    .bind(action)
    .bind(operator)
    .execute(pool)
    .await?;
    Ok(r.last_insert_id() as i64)
}

pub async fn get_chaos(pool: &MySqlPool, id: i64) -> sqlx::Result<Option<ChaosRow>> {
    sqlx::query_as::<_, ChaosRow>(&format!("{SELECT_COLS} FROM chaos_experiment WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub async fn delete_chaos(pool: &MySqlPool, id: i64) -> sqlx::Result<bool> {
    let r = sqlx::query("DELETE FROM chaos_experiment WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

/// stamp_start/stamp_end: 是否用 SQL NOW() 盖章对应时间（避免引入 chrono 依赖）
pub async fn set_chaos_status(
    pool: &MySqlPool,
    id: i64,
    status: &str,
    stamp_start: bool,
    stamp_end: bool,
    error: Option<&str>,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE chaos_experiment \
         SET status = ?, \
             started_at = IF(? AND started_at IS NULL, NOW(), started_at), \
             ended_at = IF(? AND ended_at IS NULL, NOW(), ended_at), \
             error = ? \
         WHERE id = ?",
    )
    .bind(status)
    .bind(stamp_start)
    .bind(stamp_end)
    .bind(error)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_chaos_accepts_known_actions() {
        assert!(validate_chaos("重启 payment", "payment", "restart").is_ok());
        assert!(validate_chaos("删除 payment", "payment", "delete").is_ok());
        assert!(validate_chaos("", "payment", "restart").is_err());
        assert!(validate_chaos("x", "", "restart").is_err());
        assert!(validate_chaos("x", "payment", "explode").is_err());
    }
}
