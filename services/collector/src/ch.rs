use crate::config::Config;
use ecat_data::{DataPoint, FieldValue, TsdbError};
use ecat_data_clickhouse::ClickhouseClient;
use std::sync::Arc;
use superops_protos::k8s::v1::{Deployment, Node, Pod};

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn clickhouse_from(cfg: &Config) -> anyhow::Result<Arc<ClickhouseClient>> {
    let client = ClickhouseClient::from_config(cfg.ch.clone())
        .map_err(|e| anyhow::anyhow!("clickhouse config: {e}"))?;
    Ok(Arc::new(client))
}

fn int(b: bool) -> i64 {
    if b { 1 } else { 0 }
}

pub fn build_snapshot_points(
    nodes: &[Node],
    pods: &[Pod],
    deps: &[Deployment],
    ts: i64,
) -> Vec<DataPoint> {
    let mut out = Vec::new();
    let ready = nodes.iter().filter(|n| n.status == "Ready").count();
    let running = pods.iter().filter(|p| p.status == "Running").count();
    let ready_deps = deps
        .iter()
        .filter(|d| d.replicas > 0 && d.ready_replicas == d.replicas)
        .count();
    out.push(
        DataPoint::new("resource_snapshot")
            .with_tag("type", "__summary__")
            .with_field("node_count", FieldValue::Int(nodes.len() as i64))
            .with_field("node_ready", FieldValue::Int(ready as i64))
            .with_field(
                "node_not_ready",
                FieldValue::Int(nodes.len() as i64 - ready as i64),
            )
            .with_field("pod_count", FieldValue::Int(pods.len() as i64))
            .with_field("pod_running", FieldValue::Int(running as i64))
            .with_field("deployment_count", FieldValue::Int(deps.len() as i64))
            .with_field("deployment_ready", FieldValue::Int(ready_deps as i64))
            .with_timestamp(ts),
    );
    for n in nodes {
        out.push(
            DataPoint::new("resource_snapshot")
                .with_tag("node", n.name.clone())
                .with_tag("role", n.role.clone())
                .with_field("ready", FieldValue::Int(int(n.status == "Ready")))
                .with_field("cpu", FieldValue::String(n.cpu.clone()))
                .with_field("memory", FieldValue::String(n.memory.clone()))
                .with_field("version", FieldValue::String(n.version.clone()))
                .with_timestamp(ts),
        );
    }
    for p in pods {
        out.push(
            DataPoint::new("resource_snapshot")
                .with_tag("pod", p.name.clone())
                .with_tag("namespace", p.namespace.clone())
                .with_tag("node", p.node.clone())
                .with_field("status", FieldValue::String(p.status.clone()))
                .with_field("restarts", FieldValue::Int(p.restarts as i64))
                .with_timestamp(ts),
        );
    }
    for d in deps {
        out.push(
            DataPoint::new("resource_snapshot")
                .with_tag("deployment", d.name.clone())
                .with_tag("namespace", d.namespace.clone())
                .with_field("replicas", FieldValue::Int(d.replicas as i64))
                .with_field("ready_replicas", FieldValue::Int(d.ready_replicas as i64))
                .with_field(
                    "updated_replicas",
                    FieldValue::Int(d.updated_replicas as i64),
                )
                .with_timestamp(ts),
        );
    }
    out
}

pub async fn write_snapshot(ch: &ClickhouseClient, points: &[DataPoint]) -> Result<(), TsdbError> {
    ecat_data::TsdbClient::write(ch, points).await
}
