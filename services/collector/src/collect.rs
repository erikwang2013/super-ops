use crate::ch::{build_snapshot_points, clickhouse_from, now_secs, write_snapshot};
use crate::config::Config;
use superops_protos::k8s::v1::{ListDeploymentsRequest, ListNodesRequest, ListPodsRequest};

#[tracing::instrument(skip_all)]
pub async fn collect_once(cfg: &Config) -> anyhow::Result<()> {
    let Some(cfg) = crate::cluster::resolve_and_attach(cfg).await? else {
        tracing::warn!("no registered cluster; skipping collect round");
        return Ok(());
    };
    let cluster_id = cfg.k8s.cluster_id.clone().unwrap_or_default();
    let mut client = crate::cluster::k8s_client(&cfg).await?;
    let nodes = client
        .list_nodes(ListNodesRequest {
            cluster_id: cluster_id.clone(),
        })
        .await?
        .into_inner()
        .nodes;
    let pods = client
        .list_pods(ListPodsRequest {
            cluster_id: cluster_id.clone(),
            ..Default::default()
        })
        .await?
        .into_inner()
        .pods;
    let deps = client
        .list_deployments(ListDeploymentsRequest {
            cluster_id: cluster_id.clone(),
            ..Default::default()
        })
        .await?
        .into_inner()
        .deployments;
    let ch = clickhouse_from(&cfg)?;
    let points = build_snapshot_points(&nodes, &pods, &deps, now_secs());
    write_snapshot(ch.as_ref(), &points).await?;
    Ok(())
}
