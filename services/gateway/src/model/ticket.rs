use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use sqlx::mysql::MySqlPool;

pub const TICKET_SEVERITIES: [&str; 4] = ["LOW", "MEDIUM", "HIGH", "CRIT"];
pub const TICKET_STATUSES: [&str; 4] = ["open", "assigned", "resolved", "closed"];
pub const TICKET_SOURCES: [&str; 2] = ["manual", "alert"];

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct TicketRow {
    pub id: i64,
    pub title: String,
    pub description: Option<String>,
    pub severity: String,
    pub status: String,
    pub assignee: String,
    pub source: String,
    pub alert_title: String,
    pub created_by: String,
    // sqlx 的 chrono 仅启用 clock 特性（无 serde），时间列用字符串 + SQL 格式化（同 approval.rs 约定）
    pub created_at: String,
    pub updated_at: String,
}

const SELECT_COLS: &str = "SELECT id, title, description, severity, status, assignee, source, \
         alert_title, created_by, \
         DATE_FORMAT(created_at, '%Y-%m-%d %H:%i:%s'), \
         DATE_FORMAT(updated_at, '%Y-%m-%d %H:%i:%s')";

pub fn validate_ticket(title: &str, severity: &str, status: &str) -> Result<(), String> {
    if title.is_empty() || title.len() > 128 {
        return Err("title 必须为 1..128 字符".into());
    }
    if !TICKET_SEVERITIES.contains(&severity) {
        return Err(format!("severity 必须是 {:?} 之一", TICKET_SEVERITIES));
    }
    if !TICKET_STATUSES.contains(&status) {
        return Err(format!("status 必须是 {:?} 之一", TICKET_STATUSES));
    }
    Ok(())
}

pub async fn list_tickets(
    pool: &MySqlPool,
    status: Option<&str>,
    limit: i64,
) -> sqlx::Result<Vec<TicketRow>> {
    let mut b = sqlx::QueryBuilder::new(format!("{SELECT_COLS} FROM ticket WHERE 1=1"));
    if let Some(s) = status {
        b.push(" AND status = ").push_bind(s);
    }
    b.push(" ORDER BY id DESC LIMIT ").push_bind(limit);
    b.build_query_as::<TicketRow>().fetch_all(pool).await
}

pub async fn create_ticket(
    pool: &MySqlPool,
    title: &str,
    description: Option<&str>,
    severity: &str,
    source: &str,
    alert_title: &str,
    created_by: &str,
) -> sqlx::Result<i64> {
    let r = sqlx::query(
        "INSERT INTO ticket (title, description, severity, source, alert_title, created_by) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(title)
    .bind(description)
    .bind(severity)
    .bind(source)
    .bind(alert_title)
    .bind(created_by)
    .execute(pool)
    .await?;
    Ok(r.last_insert_id() as i64)
}

/// 状态流转：open/assigned/resolved → closed；closed 不可再改（用合法列表约束）。
pub async fn update_ticket_status(
    pool: &MySqlPool,
    id: i64,
    status: &str,
    assignee: Option<&str>,
) -> sqlx::Result<bool> {
    match assignee {
        Some(a) => {
            let r = sqlx::query(
                "UPDATE ticket SET status = ?, assignee = ? WHERE id = ? AND status <> 'closed'",
            )
            .bind(status)
            .bind(a)
            .bind(id)
            .execute(pool)
            .await?;
            Ok(r.rows_affected() > 0)
        }
        None => {
            let r = sqlx::query("UPDATE ticket SET status = ? WHERE id = ? AND status <> 'closed'")
                .bind(status)
                .bind(id)
                .execute(pool)
                .await?;
            Ok(r.rows_affected() > 0)
        }
    }
}

pub async fn delete_ticket(pool: &MySqlPool, id: i64) -> sqlx::Result<bool> {
    let r = sqlx::query("DELETE FROM ticket WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::validate_ticket;

    #[test]
    fn validate_ticket_ok() {
        assert!(validate_ticket("节点 n1 不可用", "HIGH", "open").is_ok());
    }

    #[test]
    fn validate_ticket_rejects_bad_fields() {
        assert!(validate_ticket("", "HIGH", "open").is_err());
        assert!(validate_ticket("x", "URGENT", "open").is_err());
        assert!(validate_ticket("x", "HIGH", "deleted").is_err());
        assert!(validate_ticket("x", "HIGH", "closed").is_ok());
    }
}
