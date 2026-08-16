//! refresh token 吊销黑名单（Redis）：登录/刷新签发的 JWT 均带 `jti`，
//! 吊销（logout）或轮换（refresh 后旧 token 作废）时把 jti 写入黑名单，
//! 过期 token 即使未到 exp 也无法再用于刷新。

use redis::AsyncCommands;

pub struct BlacklistStore {
    client: redis::Client,
}

impl BlacklistStore {
    pub fn connect(url: &str) -> Self {
        Self {
            client: redis::Client::open(url)
                .unwrap_or_else(|e| panic!("blacklist redis url invalid: {e}")),
        }
    }

    /// 吊销 jti：TTL 取 refresh token 有效期（黑名单条目在该时长内有效）。
    pub async fn revoke(&self, jti: &str, ttl_secs: u64) {
        let mut conn = match self.client.get_multiplexed_async_connection().await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("blacklist redis connect failed: {e}");
                return;
            }
        };
        let key = format!("auth:revoked:{jti}");
        let _: Result<(), _> = conn.set_ex(key, "1", ttl_secs).await;
    }

    pub async fn is_revoked(&self, jti: &str) -> bool {
        let mut conn = match self.client.get_multiplexed_async_connection().await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("blacklist redis connect failed: {e}");
                return false;
            }
        };
        let key = format!("auth:revoked:{jti}");
        matches!(conn.exists(key).await, Ok(true))
    }
}
