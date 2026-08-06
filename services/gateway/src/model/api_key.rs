use anyhow::Result;
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::FromRow;
use sqlx::mysql::MySqlPool;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub fn generate_key() -> String {
    format!("sk_{}", uuid::Uuid::new_v4().simple())
}

pub fn hash_key(plain: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(plain.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[derive(Debug, Serialize, FromRow)]
pub struct ApiKeyMeta {
    pub id: String,
    pub name: String,
}

#[derive(Clone)]
pub struct ApiKeyStore {
    pool: MySqlPool,
    keys: Arc<RwLock<HashMap<String, String>>>, // key_hash -> user_id
}

impl ApiKeyStore {
    pub fn new(pool: MySqlPool) -> Self {
        Self {
            pool,
            keys: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn load(&self) -> Result<()> {
        let rows: Vec<(String, String)> = sqlx::query_as("SELECT key_hash, user_id FROM api_keys")
            .fetch_all(&self.pool)
            .await?;
        let mut map = self.keys.write().unwrap();
        map.clear();
        for (hash, uid) in rows {
            map.insert(hash, uid);
        }
        Ok(())
    }

    pub fn lookup(&self, hash: &str) -> Option<String> {
        self.keys.read().unwrap().get(hash).cloned()
    }

    pub async fn create(&self, user_id: &str, name: &str) -> Result<(String, String)> {
        let id = uuid::Uuid::new_v4().to_string();
        let plain = generate_key();
        let hash = hash_key(&plain);
        sqlx::query("INSERT INTO api_keys (id, user_id, name, key_hash) VALUES (?, ?, ?, ?)")
            .bind(&id)
            .bind(user_id)
            .bind(name)
            .bind(&hash)
            .execute(&self.pool)
            .await?;
        self.keys.write().unwrap().insert(hash, user_id.to_string());
        Ok((id, plain))
    }

    pub async fn delete(&self, id: &str, user_id: &str) -> Result<bool> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT key_hash FROM api_keys WHERE id = ? AND user_id = ?")
                .bind(id)
                .bind(user_id)
                .fetch_optional(&self.pool)
                .await?;
        let existed = row.is_some();
        sqlx::query("DELETE FROM api_keys WHERE id = ? AND user_id = ?")
            .bind(id)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        if let Some((hash,)) = row {
            self.keys.write().unwrap().remove(&hash);
        }
        Ok(existed)
    }

    pub async fn list(&self, user_id: &str) -> Result<Vec<ApiKeyMeta>> {
        Ok(
            sqlx::query_as::<_, ApiKeyMeta>("SELECT id, name FROM api_keys WHERE user_id = ?")
                .bind(user_id)
                .fetch_all(&self.pool)
                .await?,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_key_has_prefix_and_length() {
        let key = generate_key();
        assert!(key.starts_with("sk_"));
        assert_eq!(key.len(), 3 + 32);
    }

    #[test]
    fn hash_key_is_stable_sha256_hex() {
        // sha256("abc") = ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
        assert_eq!(
            hash_key("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(hash_key("abc"), hash_key("abc"));
        assert_ne!(hash_key("abc"), hash_key("abd"));
    }
}
