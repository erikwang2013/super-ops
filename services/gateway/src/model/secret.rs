use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use sqlx::mysql::MySqlPool;

#[derive(Serialize, Deserialize, FromRow)]
pub struct SecretRow {
    pub id: i64,
    pub name: String,
    pub created_at: String,
    pub ciphertext: Vec<u8>,
}

// 手动 Debug：ciphertext 打码，避免日志意外泄露密文
impl std::fmt::Debug for SecretRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretRow")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("created_at", &self.created_at)
            .field("ciphertext", &"[redacted]")
            .finish()
    }
}

pub async fn upsert_secret(
    pool: &MySqlPool,
    name: &str,
    ciphertext: &[u8],
) -> sqlx::Result<SecretRow> {
    let r = sqlx::query(
        "INSERT INTO secret (name, ciphertext) VALUES (?, ?) \
         ON DUPLICATE KEY UPDATE ciphertext = VALUES(ciphertext), id = LAST_INSERT_ID(id)",
    )
    .bind(name)
    .bind(ciphertext)
    .execute(pool)
    .await?;
    let id = r.last_insert_id() as i64;
    sqlx::query_as::<_, SecretRow>(
        "SELECT id, name, DATE_FORMAT(created_at, '%Y-%m-%d %H:%i:%s') AS created_at, ciphertext \
         FROM secret WHERE id = ?",
    )
    .bind(id)
    .fetch_one(pool)
    .await
}

pub async fn list_secrets(pool: &MySqlPool) -> sqlx::Result<Vec<SecretRow>> {
    sqlx::query_as::<_, SecretRow>(
        "SELECT id, name, DATE_FORMAT(created_at, '%Y-%m-%d %H:%i:%s') AS created_at, \
         CAST('' AS BINARY) AS ciphertext FROM secret ORDER BY name",
    )
    .fetch_all(pool)
    .await
}

pub async fn get_secret(pool: &MySqlPool, name: &str) -> sqlx::Result<Option<SecretRow>> {
    sqlx::query_as::<_, SecretRow>(
        "SELECT id, name, DATE_FORMAT(created_at, '%Y-%m-%d %H:%i:%s') AS created_at, ciphertext \
         FROM secret WHERE name = ?",
    )
    .bind(name)
    .fetch_optional(pool)
    .await
}

pub async fn delete_secret(pool: &MySqlPool, name: &str) -> sqlx::Result<bool> {
    let r = sqlx::query("DELETE FROM secret WHERE name = ?")
        .bind(name)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}
