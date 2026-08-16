use ecat_data::FieldValue;
use superops_collector::alert::{
    AlertEvent, AlertRule, default_rules, evaluate_health, evaluate_rule, evaluate_rules,
    health_to_points, selfheal_targets, streak_progress,
};
use superops_protos::k8s::v1::{Deployment, Node, Pod};

fn node(name: &str, status: &str) -> Node {
    Node {
        name: name.into(),
        status: status.into(),
        role: "worker".into(),
        version: "v1.30.0".into(),
        age: "10d".into(),
        cpu: "4".into(),
        memory: "8Gi".into(),
    }
}

fn pod(name: &str, status: &str) -> Pod {
    Pod {
        name: name.into(),
        namespace: "default".into(),
        status: status.into(),
        node: "n1".into(),
        restarts: 0,
        age: "1h".into(),
        containers: vec![],
        labels: Default::default(),
    }
}

fn dep(name: &str, replicas: i32, ready: i32) -> Deployment {
    Deployment {
        name: name.into(),
        namespace: "default".into(),
        replicas,
        ready_replicas: ready,
        updated_replicas: ready,
        age: "2d".into(),
        images: vec![],
    }
}

#[test]
fn healthy_cluster_no_alerts() {
    let h = evaluate_health(
        &[node("n1", "Ready")],
        &[pod("p1", "Running")],
        &[dep("web", 2, 2)],
        1,
    );
    assert!(h.cluster_ok);
    assert!(h.alerts.is_empty());
}

#[test]
fn not_ready_node_fires_crit_alert() {
    let h = evaluate_health(&[node("n1", "NotReady")], &[], &[], 1);
    assert!(h.cluster_ok);
    assert_eq!(h.alerts.len(), 1);
    let a = &h.alerts[0];
    assert_eq!(a.level, "CRIT");
    assert_eq!(a.title, "node-not-ready");
    assert!(a.message.contains("n1"));
    assert_eq!(a.node.as_deref(), Some("n1"));
}

#[test]
fn too_many_not_ready_marks_cluster_down() {
    let h = evaluate_health(
        &[node("n1", "NotReady"), node("n2", "NotReady")],
        &[],
        &[],
        1,
    );
    assert!(!h.cluster_ok);
    assert_eq!(h.alerts.len(), 2);
}

#[test]
fn deployment_short_ready_fires_warn() {
    let h = evaluate_health(&[], &[], &[dep("web", 3, 1)], 1);
    assert!(h.cluster_ok);
    assert_eq!(h.alerts.len(), 1);
    let a = &h.alerts[0];
    assert_eq!(a.level, "WARN");
    assert_eq!(a.title, "deployment-unavailable");
    assert!(a.message.contains("web"));
}

#[test]
fn pod_not_running_fires_warn() {
    let h = evaluate_health(&[], &[pod("p1", "Pending")], &[], 1);
    assert!(h.cluster_ok);
    assert_eq!(h.alerts.len(), 1);
    assert_eq!(h.alerts[0].title, "pod-not-running");
}

fn rule(name: &str, metric: &str, op: &str, threshold: f64, level: &str) -> AlertRule {
    AlertRule {
        id: 1,
        name: name.into(),
        metric: metric.into(),
        operator: op.into(),
        threshold,
        level: level.into(),
        action: "notify".into(),
    }
}

#[test]
fn default_rules_cover_three_legacy_metrics() {
    let rules = default_rules();
    assert_eq!(rules.len(), 3);
    assert!(rules.iter().all(|r| r.action == "notify"));
}

#[test]
fn rule_node_not_ready_respects_threshold() {
    let r = rule("r1", "node_not_ready", "ge", 2.0, "CRIT");
    let a = evaluate_rule(&r, &[node("n1", "NotReady")], &[], &[]);
    assert!(a.is_empty());
    let a = evaluate_rule(
        &r,
        &[node("n1", "NotReady"), node("n2", "NotReady")],
        &[],
        &[],
    );
    assert_eq!(a.len(), 2);
    assert_eq!(a[0].node.as_deref(), Some("n1"));
    assert_eq!(a[0].level, "CRIT");
}

#[test]
fn rule_node_ready_pct_uses_le_operator() {
    let r = rule("r2", "node_ready_pct", "le", 80.0, "WARN");
    let nodes = [
        node("n1", "Ready"),
        node("n2", "NotReady"),
        node("n3", "NotReady"),
        node("n4", "NotReady"),
    ];
    let a = evaluate_rule(&r, &nodes, &[], &[]);
    assert_eq!(a.len(), 1);
    assert!(a[0].message.contains("25.0%"));
    let ok = rule("r2b", "node_ready_pct", "le", 20.0, "WARN");
    assert!(evaluate_rule(&ok, &nodes, &[], &[]).is_empty());
}

#[test]
fn rule_pod_running_pct_empty_cluster_no_alert() {
    let r = rule("r3", "pod_running_pct", "le", 90.0, "WARN");
    assert!(evaluate_rule(&r, &[], &[], &[]).is_empty());
}

#[test]
fn rule_deployment_ready_pct_fires_per_deployment() {
    let r = rule("r4", "deployment_ready_pct", "le", 90.0, "WARN");
    let a = evaluate_rule(&r, &[], &[], &[dep("web", 3, 2)]);
    assert_eq!(a.len(), 1);
    assert!(a[0].message.contains("web"));
    assert!(a[0].message.contains("66.7%"));
    let a = evaluate_rule(&r, &[], &[], &[dep("web", 2, 2)]);
    assert!(a.is_empty());
}

#[test]
fn evaluate_rules_aggregates_across_rules() {
    let h = evaluate_rules(
        &default_rules(),
        &[node("n1", "NotReady")],
        &[pod("p1", "Pending")],
        &[dep("web", 3, 1)],
        5,
    );
    assert_eq!(h.alerts.len(), 3);
    assert_eq!(h.alerts[0].level, "CRIT");
    assert!(!h.cluster_ok);
}

#[test]
fn evaluate_rules_truncates_node_alert_storm() {
    // 3 个坏节点 + max_not_ready=1 → 保留 1 条节点告警 + 1 条汇总
    let nodes = vec![
        node("n1", "NotReady"),
        node("n2", "NotReady"),
        node("n3", "Unknown"),
    ];
    let h = evaluate_rules(&default_rules(), &nodes, &[], &[], 1);
    assert_eq!(h.alerts.len(), 2, "截断后应为 1 条节点告警 + 1 条汇总");
    assert!(h.alerts[0].node.is_some());
    assert!(h.alerts[1].node.is_none());
    assert!(h.alerts[1].message.contains("截断"));
    // max_not_ready=5 时 3 个坏节点不截断
    let h2 = evaluate_rules(&default_rules(), &nodes, &[], &[], 5);
    assert_eq!(h2.alerts.len(), 3);
}

#[test]
fn streak_progress_reaches_consecutive_threshold() {
    assert_eq!(streak_progress(0, 2), (false, 1));
    assert_eq!(streak_progress(1, 2), (true, 2));
    assert_eq!(streak_progress(2, 3), (true, 3));
    // consecutive=1 时立即通过
    assert_eq!(streak_progress(0, 1), (true, 1));
}

fn rule_with_action(name: &str, metric: &str, action: &str) -> AlertRule {
    let mut r = rule(name, metric, "ge", 1.0, "WARN");
    r.action = action.into();
    r
}

#[test]
fn selfheal_targets_only_deployment_metrics_with_actions() {
    let mut scaler = rule_with_action("scaler", "deployment_ready_pct", "scale");
    scaler.operator = "le".into();
    scaler.threshold = 90.0;
    let rules = vec![
        rule_with_action("restarter", "deployment_unavailable", "restart"),
        scaler,
        rule_with_action("notify-only", "deployment_unavailable", "notify"),
        rule_with_action("wrong-metric", "node_not_ready", "restart"),
    ];
    let deps = [dep("web", 3, 1), dep("api", 2, 2)];
    let targets = selfheal_targets(&rules, &deps, 10);
    // web 触发 restart+scale；api 就绪不触发；notify/wrong-metric 跳过
    assert_eq!(targets.len(), 2);
    assert!(targets.contains(&("default".into(), "web".into(), "restart".into())));
    assert!(targets.contains(&("default".into(), "web".into(), "scale".into())));
}

#[test]
fn selfheal_targets_respects_limit() {
    let rules = vec![rule_with_action("r", "deployment_unavailable", "restart")];
    let deps = [dep("a", 2, 0), dep("b", 2, 0), dep("c", 2, 0)];
    let targets = selfheal_targets(&rules, &deps, 2);
    assert_eq!(targets.len(), 2);
}

#[test]
fn selfheal_targets_ready_pct_uses_operator() {
    let r = rule_with_action("s", "deployment_ready_pct", "scale");
    let mut r = r;
    r.operator = "le".into();
    r.threshold = 90.0;
    let targets = selfheal_targets(&[r], &[dep("web", 3, 2)], 5);
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].2, "scale");
    let ok = rule_with_action("ok", "deployment_ready_pct", "restart");
    let mut ok = ok;
    ok.operator = "ge".into();
    ok.threshold = 95.0;
    assert!(selfheal_targets(&[ok], &[dep("web", 3, 2)], 5).is_empty());
}

#[test]
fn rule_invalid_operator_never_fires() {
    let r = rule("bad", "node_not_ready", "xx", 1.0, "WARN");
    assert!(evaluate_rule(&r, &[node("n1", "NotReady")], &[], &[]).is_empty());
}

#[test]
fn alert_points_have_measurement_and_level() {
    let pts = health_to_points(
        &[AlertEvent {
            level: "WARN".into(),
            title: "pod-not-running".into(),
            message: "pod default/p1 status=Pending".into(),
            node: Some("n1".into()),
        }],
        1234567890,
    );
    assert_eq!(pts.len(), 1);
    let p = &pts[0];
    assert_eq!(p.measurement, "alert_event");
    assert_eq!(p.tags.get("level").map(String::as_str), Some("WARN"));
    assert_eq!(p.tags.get("node").map(String::as_str), Some("n1"));
    assert!(matches!(p.fields.get("title"), Some(FieldValue::String(_))));
    assert_eq!(p.timestamp, Some(1234567890));
}
