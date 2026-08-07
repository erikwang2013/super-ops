use anyhow::Result;

pub fn validate_scale(cluster_id: &str, namespace: &str, name: &str, replicas: i32) -> Result<()> {
    if cluster_id.is_empty() || namespace.is_empty() || name.is_empty() {
        return Err(anyhow::anyhow!(
            "cluster_id/namespace/name must not be empty"
        ));
    }
    if !(0..=1000).contains(&replicas) {
        return Err(anyhow::anyhow!(
            "replicas must be in 0..=1000, got {replicas}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_scale_rejects_empty_fields() {
        assert!(validate_scale("", "ns", "name", 2).is_err());
        assert!(validate_scale("c", "", "name", 2).is_err());
        assert!(validate_scale("c", "ns", "", 2).is_err());
    }

    #[test]
    fn validate_scale_rejects_out_of_range_replicas() {
        assert!(validate_scale("c", "ns", "name", -1).is_err());
        assert!(validate_scale("c", "ns", "name", 1001).is_err());
        assert!(validate_scale("c", "ns", "name", 0).is_ok());
        assert!(validate_scale("c", "ns", "name", 1000).is_ok());
    }
}

use k8s_openapi::api::apps::v1::Deployment;
use kube::api::{Api, DeleteParams, Patch, PatchParams, PostParams};

use crate::cluster::client::ClusterClient;

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub async fn scale_deployment(
    client: &ClusterClient,
    namespace: &str,
    name: &str,
    replicas: i32,
) -> Result<i32> {
    let api = Api::<Deployment>::namespaced(client.client.clone(), namespace);
    let mut dep = api.get(name).await?;
    let spec = dep
        .spec
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("deployment {namespace}/{name} has no spec"))?;
    spec.replicas = Some(replicas);
    api.replace(name, &PostParams::default(), &dep).await?;
    Ok(replicas)
}

pub async fn restart_deployment(
    client: &ClusterClient,
    namespace: &str,
    name: &str,
) -> Result<bool> {
    let api = Api::<Deployment>::namespaced(client.client.clone(), namespace);
    let patch = Patch::Strategic(serde_json::json!({
        "spec": { "template": { "metadata": { "annotations": {
            "kubectl.kubernetes.io/restartedAt": now_secs().to_string()
        } } } }
    }));
    api.patch(name, &PatchParams::default(), &patch).await?;
    Ok(true)
}

pub async fn delete_deployment(
    client: &ClusterClient,
    namespace: &str,
    name: &str,
) -> Result<bool> {
    let api = Api::<Deployment>::namespaced(client.client.clone(), namespace);
    api.delete(name, &DeleteParams::default()).await?;
    Ok(true)
}

pub fn validate_image(cluster_id: &str, namespace: &str, name: &str, image: &str) -> Result<()> {
    if cluster_id.is_empty() || namespace.is_empty() || name.is_empty() {
        return Err(anyhow::anyhow!(
            "cluster_id/namespace/name must not be empty"
        ));
    }
    if image.is_empty() || image.len() > 255 {
        return Err(anyhow::anyhow!("image must be 1..255 chars"));
    }
    Ok(())
}

/// 更新首个容器的镜像（发布流水线用）；Strategic patch 保持其余字段不动。
pub async fn update_deployment_image(
    client: &ClusterClient,
    namespace: &str,
    name: &str,
    image: &str,
) -> Result<String> {
    let api = Api::<Deployment>::namespaced(client.client.clone(), namespace);
    let patch = Patch::Strategic(serde_json::json!({
        "spec": { "template": { "spec": { "containers": [ { "name": null, "image": image } ] } } }
    }));
    api.patch(name, &PatchParams::default(), &patch).await?;
    Ok(image.to_string())
}
