use crate::cluster::client::ClusterClient;

#[allow(dead_code)]
pub async fn get_pod_metrics(
    _client: &ClusterClient,
    _namespace: &str,
) -> anyhow::Result<Vec<(String, Vec<(String, String, String)>)>> {
    // Requires metrics-server on the cluster.
    // Will be implemented via raw kube API client.
    Ok(Vec::new())
}
