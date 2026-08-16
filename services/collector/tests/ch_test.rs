use ecat_data::FieldValue;
use superops_collector::ch::{build_snapshot_points, now_secs};
use superops_protos::k8s::v1::{Deployment, Node, Pod};

fn sample_nodes() -> Vec<Node> {
    vec![Node {
        name: "node-a".into(),
        status: "Ready".into(),
        role: "control-plane".into(),
        version: "v1.30".into(),
        age: "10d".into(),
        cpu: "8".into(),
        memory: "16Gi".into(),
    }]
}

fn sample_pods() -> Vec<Pod> {
    vec![Pod {
        name: "pod-1".into(),
        namespace: "default".into(),
        status: "Running".into(),
        node: "node-a".into(),
        restarts: 2,
        age: "1h".into(),
        containers: vec![],
        labels: std::collections::HashMap::new(),
    }]
}

fn sample_deps() -> Vec<Deployment> {
    vec![Deployment {
        name: "web".into(),
        namespace: "default".into(),
        replicas: 2,
        ready_replicas: 2,
        updated_replicas: 2,
        age: "5d".into(),
        images: vec![],
    }]
}

#[test]
fn snapshot_points_have_expected_shape() {
    let points = build_snapshot_points(&sample_nodes(), &sample_pods(), &sample_deps(), 1234567890);
    assert_eq!(points.len(), 4); // __summary__ + node + pod + deployment
    let summary = points.first().unwrap();
    assert_eq!(summary.measurement, "resource_snapshot");
    assert!(matches!(
        summary.fields.get("node_count"),
        Some(FieldValue::Int(1))
    ));
    assert!(matches!(
        summary.fields.get("pod_count"),
        Some(FieldValue::Int(1))
    ));
    assert!(matches!(
        summary.fields.get("deployment_ready"),
        Some(FieldValue::Int(1))
    ));
    assert_eq!(summary.timestamp, Some(1234567890));
    let node_pt = points.get(1).unwrap();
    assert_eq!(node_pt.tags.get("node").map(String::as_str), Some("node-a"));
    assert!(matches!(
        node_pt.fields.get("ready"),
        Some(FieldValue::Int(1))
    ));
    let pod_pt = points.get(2).unwrap();
    assert!(matches!(
        pod_pt.fields.get("restarts"),
        Some(FieldValue::Int(2))
    ));
    let dep_pt = points.get(3).unwrap();
    assert!(matches!(
        dep_pt.fields.get("ready_replicas"),
        Some(FieldValue::Int(2))
    ));
}

#[test]
fn not_ready_node_reported_as_zero() {
    let mut nodes = sample_nodes();
    nodes[0].status = "NotReady".into();
    let points = build_snapshot_points(&nodes, &[], &[], 1);
    let summary = points.first().unwrap();
    assert!(matches!(
        summary.fields.get("node_ready"),
        Some(FieldValue::Int(0))
    ));
    assert!(matches!(
        summary.fields.get("node_not_ready"),
        Some(FieldValue::Int(1))
    ));
}

#[test]
fn now_secs_is_reasonable() {
    let t = now_secs();
    assert!(t > 1_700_000_000 && t < 2_000_000_000);
}
