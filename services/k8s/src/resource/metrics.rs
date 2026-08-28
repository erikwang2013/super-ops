use crate::cluster::client::ClusterClient;
use k8s_openapi::api::core::v1::Node;
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use kube::ResourceExt;
use kube::api::{Api, ApiResource, DynamicObject, ListParams};
use kube::core::GroupVersionKind;
use superops_protos::k8s::v1::MetricPoint;

pub fn parse_cpu_millicores(raw: &str) -> Option<f64> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(num) = s.strip_suffix('m') {
        return parse_nonneg(num);
    }
    parse_nonneg(s).map(|v| v * 1000.0)
}

pub fn parse_memory_bytes(raw: &str) -> Option<f64> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    const SUFFIXES: &[(&str, f64)] = &[
        ("Ei", 1_152_921_504_606_846_976.0),
        ("Pi", 1_125_899_906_842_624.0),
        ("Ti", 1_099_511_627_776.0),
        ("Gi", 1_073_741_824.0),
        ("Mi", 1_048_576.0),
        ("Ki", 1024.0),
        ("E", 1e18),
        ("P", 1e15),
        ("T", 1e12),
        ("G", 1e9),
        ("M", 1e6),
        ("K", 1e3),
        ("k", 1e3),
    ];
    for (suf, mul) in SUFFIXES {
        if let Some(num) = s.strip_suffix(suf) {
            return parse_nonneg(num).map(|v| v * *mul);
        }
    }
    parse_nonneg(s)
}

fn parse_nonneg(s: &str) -> Option<f64> {
    let v: f64 = s.trim().parse().ok()?;
    if v.is_finite() && v >= 0.0 {
        Some(v)
    } else {
        None
    }
}

fn quantity_str(q: &Quantity) -> &str {
    q.0.as_str()
}

fn unix_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn point(name: String, value: f64, unit: &str, ts: i64) -> MetricPoint {
    MetricPoint {
        name,
        value,
        unit: unit.into(),
        timestamp: ts,
    }
}

pub fn metrics_from_node_capacity(nodes: &[Node], ts: i64) -> Vec<MetricPoint> {
    let mut out = Vec::new();
    for node in nodes {
        let name = node.name_any();
        let Some(status) = &node.status else {
            continue;
        };
        if let Some(cap) = &status.capacity {
            if let Some(cpu) = cap
                .get("cpu")
                .and_then(|q| parse_cpu_millicores(quantity_str(q)))
            {
                out.push(point(
                    format!("{name}/cpu_capacity_m"),
                    cpu,
                    "millicores",
                    ts,
                ));
            }
            if let Some(mem) = cap
                .get("memory")
                .and_then(|q| parse_memory_bytes(quantity_str(q)))
            {
                out.push(point(
                    format!("{name}/memory_capacity_bytes"),
                    mem,
                    "bytes",
                    ts,
                ));
            }
        }
        if let Some(alloc) = &status.allocatable {
            if let Some(cpu) = alloc
                .get("cpu")
                .and_then(|q| parse_cpu_millicores(quantity_str(q)))
            {
                out.push(point(
                    format!("{name}/cpu_allocatable_m"),
                    cpu,
                    "millicores",
                    ts,
                ));
            }
            if let Some(mem) = alloc
                .get("memory")
                .and_then(|q| parse_memory_bytes(quantity_str(q)))
            {
                out.push(point(
                    format!("{name}/memory_allocatable_bytes"),
                    mem,
                    "bytes",
                    ts,
                ));
            }
        }
    }
    out
}

async fn try_metrics_api(client: &ClusterClient, ts: i64) -> anyhow::Result<Vec<MetricPoint>> {
    let gvk = GroupVersionKind::gvk("metrics.k8s.io", "v1beta1", "NodeMetrics");
    let ar = ApiResource::from_gvk(&gvk);
    let api: Api<DynamicObject> = Api::all_with(client.client.clone(), &ar);
    let list = api.list(&ListParams::default()).await?;
    let mut out = Vec::new();
    for obj in list.items {
        let name = obj.name_any();
        let usage = obj
            .data
            .get("usage")
            .cloned()
            .unwrap_or(serde_json::json!({}));
        if let Some(cpu) = usage
            .get("cpu")
            .and_then(|v| v.as_str())
            .and_then(parse_cpu_millicores)
        {
            out.push(point(format!("{name}/cpu_usage_m"), cpu, "millicores", ts));
        }
        if let Some(mem) = usage
            .get("memory")
            .and_then(|v| v.as_str())
            .and_then(parse_memory_bytes)
        {
            out.push(point(
                format!("{name}/memory_usage_bytes"),
                mem,
                "bytes",
                ts,
            ));
        }
    }
    Ok(out)
}

/// 优先 metrics.k8s.io；不可用时回落到 Node capacity/allocatable，集群有节点则非空。
pub async fn collect_metrics(client: &ClusterClient) -> anyhow::Result<Vec<MetricPoint>> {
    let ts = unix_ts();
    match try_metrics_api(client, ts).await {
        Ok(points) if !points.is_empty() => Ok(points),
        _ => {
            let nodes = crate::resource::node::list_nodes(client).await?;
            Ok(metrics_from_node_capacity(&nodes, ts))
        }
    }
}

#[allow(dead_code)]
pub async fn get_pod_metrics(
    _client: &ClusterClient,
    _namespace: &str,
) -> anyhow::Result<Vec<(String, Vec<(String, String, String)>)>> {
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::NodeStatus;
    use std::collections::BTreeMap;

    #[test]
    fn parse_cpu_millicores_units() {
        assert_eq!(parse_cpu_millicores("100m"), Some(100.0));
        assert_eq!(parse_cpu_millicores("1"), Some(1000.0));
        assert_eq!(parse_cpu_millicores("2.5"), Some(2500.0));
        assert_eq!(parse_cpu_millicores("not-a-qty"), None);
        assert_eq!(parse_cpu_millicores(""), None);
        assert_eq!(parse_cpu_millicores("-1"), None);
    }

    #[test]
    fn parse_memory_bytes_units() {
        assert_eq!(parse_memory_bytes("1024"), Some(1024.0));
        assert_eq!(parse_memory_bytes("1Ki"), Some(1024.0));
        assert_eq!(parse_memory_bytes("1Mi"), Some(1024.0 * 1024.0));
        assert_eq!(
            parse_memory_bytes("4Gi"),
            Some(4.0 * 1024.0 * 1024.0 * 1024.0)
        );
        assert_eq!(parse_memory_bytes("junk"), None);
        assert_eq!(parse_memory_bytes(""), None);
    }

    #[test]
    fn metrics_from_node_capacity_emits_four_points() {
        let mut capacity = BTreeMap::new();
        capacity.insert("cpu".into(), Quantity("2".into()));
        capacity.insert("memory".into(), Quantity("4Gi".into()));
        let mut allocatable = BTreeMap::new();
        allocatable.insert("cpu".into(), Quantity("1900m".into()));
        allocatable.insert("memory".into(), Quantity("3Gi".into()));
        let mut node = Node::default();
        node.metadata.name = Some("n1".into());
        node.status = Some(NodeStatus {
            capacity: Some(capacity),
            allocatable: Some(allocatable),
            ..Default::default()
        });
        let points = metrics_from_node_capacity(&[node], 42);
        assert_eq!(points.len(), 4);
        assert!(
            points
                .iter()
                .any(|p| p.name == "n1/cpu_capacity_m" && p.value == 2000.0)
        );
        assert!(
            points
                .iter()
                .any(|p| p.name == "n1/cpu_allocatable_m" && p.value == 1900.0)
        );
        assert!(
            points
                .iter()
                .any(|p| p.name.ends_with("memory_capacity_bytes") && p.value > 0.0)
        );
        assert!(points.iter().all(|p| p.timestamp == 42));
    }
}
