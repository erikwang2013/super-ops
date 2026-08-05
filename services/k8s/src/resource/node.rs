use anyhow::Result;
use k8s_openapi::api::core::v1::Node;
use kube::Api;

use crate::cluster::client::ClusterClient;

pub async fn list_nodes(client: &ClusterClient) -> Result<Vec<Node>> {
    let api: Api<Node> = Api::all(client.client.clone());
    Ok(api.list(&Default::default()).await?.items)
}
