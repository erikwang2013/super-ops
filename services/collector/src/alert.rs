use crate::ch::{clickhouse_from, now_secs};
use crate::config::Config;
use ecat_data::{DataPoint, FieldValue};
use ecat_data_redis::RedisLock;
use superops_protos::k8s::v1::k8s_service_client::K8sServiceClient;
use superops_protos::k8s::v1::{
    Deployment, ListDeploymentsRequest, ListNodesRequest, ListPodsRequest, Node, Pod,
};

#[derive(Debug, Clone, PartialEq)]
pub struct AlertEvent {
    pub level: String,
    pub title: String,
    pub message: String,
    pub node: Option<String>,
}

#[derive(Debug, Default)]
pub struct ClusterHealth {
    pub alerts: Vec<AlertEvent>,
    pub cluster_ok: bool,
}

pub fn evaluate_health(
    nodes: &[Node],
    pods: &[Pod],
    deps: &[Deployment],
    max_not_ready: usize,
) -> ClusterHealth {
    let mut alerts = Vec::new();
    let mut not_ready_nodes = 0usize;
    for n in nodes {
        if n.status != "Ready" {
            not_ready_nodes += 1;
            alerts.push(AlertEvent {
                level: "CRIT".into(),
                title: "node-not-ready".into(),
                message: format!("node {} status={}", n.name, n.status),
                node: Some(n.name.clone()),
            });
        }
    }
    for p in pods
        .iter()
        .filter(|p| p.status != "Running" && p.status != "Succeeded")
    {
        alerts.push(AlertEvent {
            level: "WARN".into(),
            title: "pod-not-running".into(),
            message: format!("pod {}/{} status={}", p.namespace, p.name, p.status),
            node: None,
        });
    }
    for d in deps {
        if d.ready_replicas < d.replicas {
            alerts.push(AlertEvent {
                level: "WARN".into(),
                title: "deployment-unavailable".into(),
                message: format!(
                    "deployment {}/{} ready {}/{}",
                    d.namespace, d.name, d.ready_replicas, d.replicas
                ),
                node: None,
            });
        }
    }
    ClusterHealth {
        alerts,
        cluster_ok: not_ready_nodes <= max_not_ready,
    }
}

pub fn health_to_points(events: &[AlertEvent], ts: i64) -> Vec<DataPoint> {
    events
        .iter()
        .map(|e| {
            let mut p = DataPoint::new("alert_event")
                .with_tag("level", e.level.clone())
                .with_field("title", FieldValue::String(e.title.clone()))
                .with_field("message", FieldValue::String(e.message.clone()))
                .with_timestamp(ts);
            if let Some(n) = &e.node {
                p = p.with_tag("node", n.clone());
            }
            p
        })
        .collect()
}

pub async fn inspect_once(cfg: &Config) -> anyhow::Result<()> {
    let lock = RedisLock::from_config(cfg.lock.clone())
        .await
        .map_err(|e| anyhow::anyhow!("redis lock config: {e}"))?;
    let ran = crate::inspect::with_inspect_lock(&lock, || inspect_work(cfg))
        .await
        .map_err(|e| anyhow::anyhow!("inspect lock: {e}"))?;
    if ran.is_none() {
        tracing::debug!("inspect skipped: lock held by another instance");
    }
    Ok(())
}

async fn inspect_work(cfg: &Config) -> anyhow::Result<()> {
    let mut client = K8sServiceClient::connect(cfg.k8s.endpoint.clone()).await?;
    let nodes = client
        .list_nodes(ListNodesRequest::default())
        .await?
        .into_inner()
        .nodes;
    let pods = client
        .list_pods(ListPodsRequest::default())
        .await?
        .into_inner()
        .pods;
    let deps = client
        .list_deployments(ListDeploymentsRequest::default())
        .await?
        .into_inner()
        .deployments;

    let health = evaluate_health(&nodes, &pods, &deps, cfg.collector.max_not_ready);
    if health.cluster_ok || health.alerts.is_empty() {
        return Ok(());
    }
    let ch = clickhouse_from(cfg)?;
    let points = health_to_points(&health.alerts, now_secs());
    ecat_data::TsdbClient::write(ch.as_ref(), &points).await?;
    Ok(())
}
