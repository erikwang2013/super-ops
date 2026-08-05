use anyhow::Result;
use k8s_openapi::api::apps::v1::Deployment;
use kube::Api;

use crate::cluster::client::ClusterClient;

pub async fn list_deployments(client: &ClusterClient, namespace: &str) -> Result<Vec<Deployment>> {
    let api: Api<Deployment> = if namespace.is_empty() {
        Api::all(client.client.clone())
    } else {
        Api::namespaced(client.client.clone(), namespace)
    };
    Ok(api.list(&Default::default()).await?.items)
}
