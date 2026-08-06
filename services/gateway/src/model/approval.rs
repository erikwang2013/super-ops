use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use sqlx::mysql::MySqlPool;

pub const APPROVAL_ACTIONS: [&str; 4] = ["approve", "reject", "cancel", "reopen"];

pub const APPROVAL_STATUSES: [&str; 4] = ["pending", "approved", "rejected", "canceled"];

/// 审批单种类白名单（与 deploy/init.sql approval.kind 注释一致）。
pub const APPROVAL_KINDS: [&str; 4] = ["delete", "scale", "restart", "generic"];

pub fn validate_approval(action: &str) -> bool {
    APPROVAL_ACTIONS.contains(&action)
}

/// 审批状态机：pending → approve/reject/cancel；rejected → reopen。
/// 终态（approved/canceled）与非法动作返回 None。
pub fn next_status(current: &str, action: &str) -> Option<&'static str> {
    Some(match (current, action) {
        ("pending", "approve") => "approved",
        ("pending", "reject") => "rejected",
        ("pending", "cancel") => "canceled",
        ("rejected", "reopen") => "pending",
        _ => return None,
    })
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct ApprovalRow {
    pub id: i64,
    pub kind: String,
    pub target: String,
    pub operator: String,
    pub reason: String,
    pub status: String,
    // sqlx 的 chrono 仅启用 clock 特性（无 serde），故时间列用字符串 + SQL 格式化（同 script.rs 约定）
    pub created_at: String,
    pub decided_by: Option<String>,
    pub decided_at: Option<String>,
}

const SELECT_COLS: &str = "SELECT id, kind, target, operator, reason, status, \
         DATE_FORMAT(created_at, '%Y-%m-%d %H:%i:%s'), \
         decided_by, DATE_FORMAT(decided_at, '%Y-%m-%d %H:%i:%s')";

pub async fn list_approvals(
    pool: &MySqlPool,
    status: Option<&str>,
    limit: i64,
) -> sqlx::Result<Vec<ApprovalRow>> {
    let mut b = sqlx::QueryBuilder::new(format!("{SELECT_COLS} FROM approval WHERE 1=1"));
    if let Some(s) = status {
        b.push(" AND status = ").push_bind(s);
    }
    b.push(" ORDER BY id DESC LIMIT ").push_bind(limit);
    b.build_query_as::<ApprovalRow>().fetch_all(pool).await
}

pub async fn get_approval(pool: &MySqlPool, id: i64) -> sqlx::Result<Option<ApprovalRow>> {
    sqlx::query_as(&format!("{SELECT_COLS} FROM approval WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await
}

pub async fn create_approval(
    pool: &MySqlPool,
    kind: &str,
    target: &str,
    reason: &str,
    operator: &str,
) -> sqlx::Result<i64> {
    let r =
        sqlx::query("INSERT INTO approval (kind, target, reason, operator) VALUES (?, ?, ?, ?)")
            .bind(kind)
            .bind(target)
            .bind(reason)
            .bind(operator)
            .execute(pool)
            .await?;
    Ok(r.last_insert_id() as i64)
}

/// 幂等更新：仅当该行当前仍处于 expected_status 时才生效，返回是否更新成功。
pub async fn update_approval_status(
    pool: &MySqlPool,
    id: i64,
    expected_status: &str,
    new_status: &str,
    decided_by: &str,
) -> sqlx::Result<bool> {
    let r = sqlx::query(
        "UPDATE approval SET status = ?, decided_by = ?, decided_at = NOW() \
         WHERE id = ? AND status = ?",
    )
    .bind(new_status)
    .bind(decided_by)
    .bind(id)
    .bind(expected_status)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() > 0)
}

/// 删除门禁：目标 `{cluster_id}/{ns}/{name}` 是否已有 status='approved' 的 delete 审批单。
pub async fn is_delete_approved(pool: &MySqlPool, target: &str) -> sqlx::Result<bool> {
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM approval WHERE kind = 'delete' AND target = ? AND status = 'approved'",
    )
    .bind(target)
    .fetch_one(pool)
    .await?;
    Ok(n > 0)
}
