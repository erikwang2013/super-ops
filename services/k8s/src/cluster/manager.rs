use anyhow::Result;
use dashmap::DashMap;
use uuid::Uuid;

use crate::cluster::client::ClusterClient;

pub struct ClusterManager {
    clusters: DashMap<String, ClusterClient>,
}

impl ClusterManager {
    pub fn new() -> Self {
        Self {
            clusters: DashMap::new(),
        }
    }

    pub async fn add(&self, name: String, kubeconfig: Vec<u8>) -> Result<ClusterClient> {
        let id = Uuid::new_v4().to_string();
        let client = ClusterClient::new(id.clone(), name, &kubeconfig).await?;
        self.clusters.insert(id, client.clone());
        Ok(client)
    }

    pub fn remove(&self, cluster_id: &str) -> Result<()> {
        self.clusters
            .remove(cluster_id)
            .map(|_| ())
            .ok_or_else(|| anyhow::anyhow!("cluster not found: {}", cluster_id))
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
