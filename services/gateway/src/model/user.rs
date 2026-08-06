use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use sqlx::mysql::MySqlPool;

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct User {
    pub id: String,
    pub username: String,
    pub email: String,
    pub password_hash: String,
    pub role: String,
    // 与 init.sql 的 tenant_id 列保持一致（注册流程无租户上下文，恒为 "default"）
    pub tenant_id: String,
}

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct UserRow {
    pub id: String,
    pub username: String,
    pub email: String,
    pub role: String,
    pub tenant_id: String,
    pub status: String,
    // sqlx 的 chrono 仅启用 clock 特性（无 serde），故 created_at 用字符串 + SQL 格式化
    pub created_at: String,
}

pub async fn list_users(pool: &MySqlPool) -> sqlx::Result<Vec<UserRow>> {
    sqlx::query_as::<_, UserRow>(
        "SELECT id, username, email, role, tenant_id, status, DATE_FORMAT(created_at, '%Y-%m-%d %H:%i:%s') FROM users ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await
}

pub async fn set_user_status(pool: &MySqlPool, id: &str, status: &str) -> sqlx::Result<bool> {
    let r = sqlx::query("UPDATE users SET status = ? WHERE id = ?")
        .bind(status)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

#[derive(Clone)]
pub struct UserStore {
    pool: MySqlPool,
}

impl UserStore {
    pub fn new(pool: MySqlPool) -> Self {
        Self { pool }
    }

    pub async fn find_by_username(&self, username: &str) -> Result<Option<User>> {
        Ok(sqlx::query_as::<_, User>(
            "SELECT id, username, email, password_hash, role, tenant_id FROM users WHERE username = ?",
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await?)
    }

    pub async fn count(&self) -> Result<i64> {
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.0)
    }

    pub async fn find_role(&self, id: &str) -> Result<Option<String>> {
        Ok(sqlx::query_scalar("SELECT role FROM users WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?)
    }

    pub async fn create(
        &self,
        username: &str,
        email: &str,
        password_hash: &str,
        role: &str,
    ) -> Result<User> {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO users (id, username, email, password_hash, role, tenant_id) VALUES (?, ?, ?, ?, ?, 'default')",
        )
        .bind(&id)
        .bind(username)
        .bind(email)
        .bind(password_hash)
        .bind(role)
        .execute(&self.pool)
        .await?;
        Ok(User {
            id,
            username: username.to_string(),
            email: email.to_string(),
            password_hash: password_hash.to_string(),
            role: role.to_string(),
            tenant_id: "default".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_row_json_shape() {
        let row = UserRow {
            id: "u1".into(),
            username: "erik".into(),
            email: "erik@example.com".into(),
            role: "viewer".into(),
            tenant_id: "default".into(),
            status: "enabled".into(),
            created_at: "2026-08-06 10:00:00".into(),
        };
        let v = serde_json::to_value(&row).unwrap();
        assert_eq!(v["id"], "u1");
        assert_eq!(v["username"], "erik");
        assert_eq!(v["role"], "viewer");
        assert_eq!(v["tenant_id"], "default");
        assert_eq!(v["status"], "enabled");
        assert_eq!(v["created_at"], "2026-08-06 10:00:00");
    }
}
