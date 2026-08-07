use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use sqlx::mysql::MySqlPool;

pub const ASSET_TYPES: [&str; 6] = ["host", "switch", "router", "app", "db", "storage"];

pub fn validate_asset(asset_type: &str, name: &str) -> Result<(), String> {
    if !ASSET_TYPES.contains(&asset_type) {
        return Err(format!("asset_type must be one of {:?}", ASSET_TYPES));
    }
    if name.is_empty() || name.chars().count() > 128 {
        return Err("name must be 1..=128 chars".into());
    }
    Ok(())
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct AssetRow {
    pub id: i64,
    pub asset_type: String,
    pub name: String,
    pub ip: Option<String>,
    pub env: String,
    pub owner: String,
    // MySQL 文本协议下 JSON 列以字符串返回；写入时绑定合法 JSON 字符串即可
    pub labels: Option<String>,
    pub status: String,
}

pub async fn list_assets(
    pool: &MySqlPool,
    tenant: &str,
    asset_type: Option<&str>,
    env: Option<&str>,
) -> sqlx::Result<Vec<AssetRow>> {
    let mut b = sqlx::QueryBuilder::new(
        "SELECT id, asset_type, name, ip, env, owner, labels, status FROM cmdb_asset WHERE tenant_id = ",
    );
    b.push_bind(tenant);
    if let Some(t) = asset_type.filter(|s| !s.is_empty()) {
        b.push(" AND asset_type = ").push_bind(t);
    }
    if let Some(e) = env.filter(|s| !s.is_empty()) {
        b.push(" AND env = ").push_bind(e);
    }
    b.push(" ORDER BY created_at DESC");
    b.build_query_as::<AssetRow>().fetch_all(pool).await
}

pub struct AssetArgs<'a> {
    pub tenant: &'a str,
    pub asset_type: &'a str,
    pub name: &'a str,
    pub ip: Option<&'a str>,
    pub env: &'a str,
    pub owner: &'a str,
    pub labels: Option<&'a str>,
}

pub async fn upsert_asset(pool: &MySqlPool, args: AssetArgs<'_>) -> sqlx::Result<u64> {
    let r = sqlx::query(
        "INSERT INTO cmdb_asset (tenant_id, asset_type, name, ip, env, owner, labels) \
         VALUES (?, ?, ?, ?, ?, ?, ?) \
         ON DUPLICATE KEY UPDATE ip = VALUES(ip), env = VALUES(env), owner = VALUES(owner), labels = VALUES(labels), id = LAST_INSERT_ID(id)",
    )
    .bind(args.tenant)
    .bind(args.asset_type)
    .bind(args.name)
    .bind(args.ip)
    .bind(args.env)
    .bind(args.owner)
    .bind(args.labels)
    .execute(pool)
    .await?;
    Ok(r.last_insert_id())
}

pub async fn delete_asset(pool: &MySqlPool, tenant: &str, id: i64) -> sqlx::Result<bool> {
    let r = sqlx::query("DELETE FROM cmdb_asset WHERE id = ? AND tenant_id = ?")
        .bind(id)
        .bind(tenant)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn asset_type_counts(
    pool: &MySqlPool,
    tenant: &str,
) -> sqlx::Result<std::collections::HashMap<String, i64>> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT asset_type, COUNT(*) AS cnt FROM cmdb_asset WHERE tenant_id = ? GROUP BY asset_type",
    )
    .bind(tenant)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_asset_rejects_unknown_type_and_empty_name() {
        assert!(validate_asset("vm", "x").is_err());
        assert!(validate_asset("host", "").is_err());
        assert!(validate_asset("host", "web-01").is_ok());
    }
}
