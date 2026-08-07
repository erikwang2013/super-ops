use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use sqlx::mysql::MySqlPool;

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct QuotaRow {
    pub id: i64,
    pub cluster_id: String,
    pub namespace: String,
    pub cpu_request: String,
    pub memory_request: String,
    pub cpu_limit: String,
    pub memory_limit: String,
    pub replicas: i32,
    pub description: String,
    pub created_at: String,
}

pub fn validate_quota(namespace: &str) -> Result<(), String> {
    if namespace.is_empty() || namespace.chars().count() > 64 {
        return Err("namespace must be 1..=64 chars".into());
    }
    Ok(())
}

pub async fn list_quotas(
    pool: &MySqlPool,
    cluster_id: Option<&str>,
) -> sqlx::Result<Vec<QuotaRow>> {
    let mut b = sqlx::QueryBuilder::new(
        "SELECT id, cluster_id, namespace, cpu_request, memory_request, cpu_limit, memory_limit, \
         replicas, description, DATE_FORMAT(created_at, '%Y-%m-%d %H:%i:%s') AS created_at \
         FROM resource_quota WHERE 1=1",
    );
    if let Some(c) = cluster_id.filter(|s| !s.is_empty()) {
        b.push(" AND cluster_id = ").push_bind(c);
    }
    b.push(" ORDER BY created_at DESC");
    b.build_query_as::<QuotaRow>().fetch_all(pool).await
}

pub struct QuotaArgs<'a> {
    pub cluster_id: &'a str,
    pub namespace: &'a str,
    pub cpu_request: &'a str,
    pub memory_request: &'a str,
    pub cpu_limit: &'a str,
    pub memory_limit: &'a str,
    pub replicas: i32,
    pub description: &'a str,
}

pub async fn upsert_quota(pool: &MySqlPool, args: QuotaArgs<'_>) -> sqlx::Result<u64> {
    let r = sqlx::query(
        "INSERT INTO resource_quota \
         (cluster_id, namespace, cpu_request, memory_request, cpu_limit, memory_limit, replicas, description) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
         ON DUPLICATE KEY UPDATE cpu_request = VALUES(cpu_request), \
         memory_request = VALUES(memory_request), cpu_limit = VALUES(cpu_limit), \
         memory_limit = VALUES(memory_limit), replicas = VALUES(replicas), \
         description = VALUES(description), id = LAST_INSERT_ID(id)",
    )
    .bind(args.cluster_id)
    .bind(args.namespace)
    .bind(args.cpu_request)
    .bind(args.memory_request)
    .bind(args.cpu_limit)
    .bind(args.memory_limit)
    .bind(args.replicas)
    .bind(args.description)
    .execute(pool)
    .await?;
    Ok(r.last_insert_id())
}

pub async fn delete_quota(pool: &MySqlPool, id: i64) -> sqlx::Result<bool> {
    let r = sqlx::query("DELETE FROM resource_quota WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_quota_rejects_empty_and_long_namespace() {
        assert!(validate_quota("").is_err());
        assert!(validate_quota(&"x".repeat(65)).is_err());
        assert!(validate_quota("default").is_ok());
    }
}
