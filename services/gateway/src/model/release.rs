use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use sqlx::mysql::MySqlPool;

pub const RELEASE_STATUSES: [&str; 4] = ["pending", "rolling", "ok", "failed"];

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct ReleaseRow {
    pub id: i64,
    pub cluster_id: String,
    pub namespace: String,
    pub name: String,
    pub old_image: String,
    pub new_image: String,
    pub operator: String,
    pub status: String,
    // sqlx 的 chrono 仅启用 clock 特性（无 serde），时间列用字符串 + SQL 格式化（同 approval.rs 约定）
    pub created_at: String,
}

const SELECT_COLS: &str = "SELECT id, cluster_id, namespace, name, old_image, new_image, \
         operator, status, DATE_FORMAT(created_at, '%Y-%m-%d %H:%i:%s')";

pub fn validate_release(image: &str, status: &str) -> Result<(), String> {
    if image.is_empty() || image.len() > 255 {
        return Err("new_image 必须为 1..255 字符".into());
    }
    if !RELEASE_STATUSES.contains(&status) {
        return Err(format!("status 必须是 {:?} 之一", RELEASE_STATUSES));
    }
    Ok(())
}

pub async fn record_release(
    pool: &MySqlPool,
    cluster_id: &str,
    namespace: &str,
    name: &str,
    old_image: &str,
    new_image: &str,
    operator: &str,
    status: &str,
) -> sqlx::Result<i64> {
    let r = sqlx::query(
        "INSERT INTO release (cluster_id, namespace, name, old_image, new_image, operator, status) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(cluster_id)
    .bind(namespace)
    .bind(name)
    .bind(old_image)
    .bind(new_image)
    .bind(operator)
    .bind(status)
    .execute(pool)
    .await?;
    Ok(r.last_insert_id() as i64)
}

pub async fn list_releases(
    pool: &MySqlPool,
    status: Option<&str>,
    limit: i64,
) -> sqlx::Result<Vec<ReleaseRow>> {
    let mut b = sqlx::QueryBuilder::new(format!("{SELECT_COLS} FROM release WHERE 1=1"));
    if let Some(s) = status {
        b.push(" AND status = ").push_bind(s);
    }
    b.push(" ORDER BY id DESC LIMIT ").push_bind(limit);
    b.build_query_as::<ReleaseRow>().fetch_all(pool).await
}

#[cfg(test)]
mod tests {
    use super::validate_release;

    #[test]
    fn validate_release_ok() {
        assert!(validate_release("nginx:1.27", "pending").is_ok());
    }

    #[test]
    fn validate_release_rejects_bad_fields() {
        assert!(validate_release("", "pending").is_err());
        assert!(validate_release("nginx:1.27", "cancelled").is_err());
    }
}
