use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use sqlx::mysql::MySqlPool;

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct OncallShiftRow {
    pub id: i64,
    pub name: String,
    pub assignee: String,
    // sqlx 的 chrono 仅启用 clock 特性（无 serde），时间列用字符串 + SQL 格式化（同 approval.rs 约定）
    pub start_at: String,
    pub end_at: String,
    pub created_at: String,
}

const SELECT_COLS: &str = "SELECT id, name, assignee, \
         DATE_FORMAT(start_at, '%Y-%m-%d %H:%i:%s'), \
         DATE_FORMAT(end_at, '%Y-%m-%d %H:%i:%s'), \
         DATE_FORMAT(created_at, '%Y-%m-%d %H:%i:%s')";

/// 时间串校验：必须为 "YYYY-MM-DD HH:MM:SS" 且 end 晚于 start（字符串比较即时间序）。
pub fn validate_shift(
    name: &str,
    assignee: &str,
    start_at: &str,
    end_at: &str,
) -> Result<(), String> {
    if name.is_empty() || name.len() > 128 {
        return Err("name 必须为 1..128 字符".into());
    }
    if assignee.is_empty() || assignee.len() > 64 {
        return Err("assignee 必须为 1..64 字符".into());
    }
    if start_at.len() != 19 || end_at.len() != 19 {
        return Err("start_at/end_at 必须为 'YYYY-MM-DD HH:MM:SS'".into());
    }
    if end_at <= start_at {
        return Err("end_at 必须晚于 start_at".into());
    }
    Ok(())
}

pub async fn list_shifts(pool: &MySqlPool, limit: i64) -> sqlx::Result<Vec<OncallShiftRow>> {
    sqlx::query_as(&format!(
        "{SELECT_COLS} FROM oncall_schedule ORDER BY start_at DESC LIMIT ?"
    ))
    .bind(limit)
    .fetch_all(pool)
    .await
}

/// 当前生效班次（start_at <= NOW() AND end_at >= NOW()），重叠时取最近开始的一条。
pub async fn current_shift(pool: &MySqlPool) -> sqlx::Result<Option<OncallShiftRow>> {
    sqlx::query_as(&format!(
        "{SELECT_COLS} FROM oncall_schedule \
         WHERE start_at <= NOW() AND end_at >= NOW() ORDER BY start_at DESC LIMIT 1"
    ))
    .fetch_optional(pool)
    .await
}

pub async fn create_shift(
    pool: &MySqlPool,
    name: &str,
    assignee: &str,
    start_at: &str,
    end_at: &str,
) -> sqlx::Result<i64> {
    let r = sqlx::query(
        "INSERT INTO oncall_schedule (name, assignee, start_at, end_at) VALUES (?, ?, ?, ?)",
    )
    .bind(name)
    .bind(assignee)
    .bind(start_at)
    .bind(end_at)
    .execute(pool)
    .await?;
    Ok(r.last_insert_id() as i64)
}

pub async fn delete_shift(pool: &MySqlPool, id: i64) -> sqlx::Result<bool> {
    let r = sqlx::query("DELETE FROM oncall_schedule WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::validate_shift;

    #[test]
    fn validate_shift_ok() {
        assert!(
            validate_shift("晚班", "erik", "2026-08-07 18:00:00", "2026-08-08 02:00:00").is_ok()
        );
    }

    #[test]
    fn validate_shift_rejects_bad_times() {
        assert!(validate_shift("x", "erik", "2026-08-07 18:00:00", "2026-08-07 18:00:00").is_err());
        assert!(validate_shift("x", "erik", "2026-08-07 18:00", "2026-08-07 18:00:00").is_err());
        assert!(validate_shift("", "erik", "2026-08-07 18:00:00", "2026-08-08 02:00:00").is_err());
    }
}
