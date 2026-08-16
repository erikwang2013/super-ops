use anyhow::Result;
use dashmap::DashMap;
use sqlx::mysql::MySqlPool;
use uuid::Uuid;

use crate::cluster::client::ClusterClient;

/// 集群注册表：内存 DashMap + 可选 MySQL 持久化后端。
/// `pool` 为 None 时为纯内存模式（未配置 database 段，重启后注册丢失）。
pub struct ClusterManager {
    clusters: DashMap<String, ClusterClient>,
    pool: Option<MySqlPool>,
}

impl ClusterManager {
    pub fn new(pool: Option<MySqlPool>) -> Self {
        Self {
            clusters: DashMap::new(),
            pool,
        }
    }

    pub async fn add(&self, name: String, kubeconfig: Vec<u8>) -> Result<ClusterClient> {
        let id = Uuid::new_v4().to_string();
        let client = ClusterClient::new(id.clone(), name, &kubeconfig).await?;
        self.clusters.insert(id.clone(), client.clone());
        if let Some(pool) = &self.pool {
            sqlx::query("INSERT INTO `cluster` (id, name, kubeconfig) VALUES (?, ?, ?)")
                .bind(&id)
                .bind(&client.name)
                .bind(String::from_utf8_lossy(&kubeconfig).to_string())
                .execute(pool)
                .await
                .map_err(|e| anyhow::anyhow!("cluster persist: {e}"))?;
        }
        Ok(client)
    }

    pub async fn remove(&self, cluster_id: &str) -> Result<()> {
        self.clusters
            .remove(cluster_id)
            .map(|_| ())
            .ok_or_else(|| anyhow::anyhow!("cluster not found: {}", cluster_id))?;
        if let Some(pool) = &self.pool {
            sqlx::query("DELETE FROM `cluster` WHERE id = ?")
                .bind(cluster_id)
                .execute(pool)
                .await
                .map_err(|e| anyhow::anyhow!("cluster remove persist: {e}"))?;
        }
        Ok(())
    }

    /// 启动时从数据库加载已注册集群（Phase 2 persistence 落地）。
    /// 单条 kubeconfig 解析失败仅告警跳过，不阻断其余集群加载。
    pub async fn load(&self) -> Result<()> {
        let Some(pool) = &self.pool else {
            return Ok(());
        };
        let rows: Vec<(String, String, String)> =
            sqlx::query_as("SELECT id, name, kubeconfig FROM `cluster`")
                .fetch_all(pool)
                .await
                .map_err(|e| anyhow::anyhow!("cluster load: {e}"))?;
        let mut loaded = 0;
        for (id, name, kubeconfig) in rows {
            match ClusterClient::new(id.clone(), name.clone(), kubeconfig.as_bytes()).await {
                Ok(client) => {
                    self.clusters.insert(id, client);
                    loaded += 1;
                }
                Err(e) => {
                    tracing::warn!(cluster_id = %id, name = %name, error = %e, "load cluster failed; skipping")
                }
            }
        }
        tracing::info!(loaded, "clusters loaded from database");
        Ok(())
    }

    pub fn get(&self, cluster_id: &str) -> Result<ClusterClient> {
        self.clusters
            .get(cluster_id)
            .map(|c| c.clone())
            .ok_or_else(|| anyhow::anyhow!("cluster not found: {}", cluster_id))
    }

    pub fn list(&self) -> Vec<ClusterClient> {
        self.clusters
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_mode_without_pool() {
        let mgr = ClusterManager::new(None);
        assert!(mgr.list().is_empty());
        // 未配置 pool 时 remove 不存在的集群报错
        let rt = tokio::runtime::Runtime::new().unwrap();
        assert!(rt.block_on(mgr.remove("missing")).is_err());
    }
}
