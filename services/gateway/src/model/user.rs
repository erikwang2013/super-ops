use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::mysql::MySqlPool;
use sqlx::FromRow;

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct User {
    pub id: String,
    pub username: String,
    pub email: String,
    pub password_hash: String,
}

#[derive(Clone)]
pub struct UserStore { pool: MySqlPool }

impl UserStore {
    pub fn new(pool: MySqlPool) -> Self { Self { pool } }

    pub async fn find_by_username(&self, username: &str) -> Result<Option<User>> {
        Ok(sqlx::query_as::<_, User>("SELECT id, username, email, password_hash FROM users WHERE username = ?")
            .bind(username).fetch_optional(&self.pool).await?)
    }

    pub async fn create(&self, username: &str, email: &str, password_hash: &str) -> Result<User> {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO users (id, username, email, password_hash) VALUES (?, ?, ?, ?)")
            .bind(&id).bind(username).bind(email).bind(password_hash).execute(&self.pool).await?;
        Ok(User { id, username: username.to_string(), email: email.to_string(), password_hash: password_hash.to_string() })
    }
}
