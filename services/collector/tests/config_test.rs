use superops_collector::config::{CollectorConfig, Config, collector_config_from};

#[test]
fn default_collector_config() {
    let cfg = CollectorConfig::default();
    assert_eq!(cfg.collect_interval_secs, 60);
    assert_eq!(cfg.inspect_interval_secs, 600);
    assert_eq!(cfg.max_not_ready, 1);
    assert_eq!(cfg.alert_consecutive, 2);
}

#[test]
fn config_from_yaml_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collector.yaml");
    std::fs::write(
        &path,
        r#"ch:
  base_url: "http://localhost:8124"
  database: "superops"
mq:
  brokers: "localhost:9092"
  group_id: "superops-collector"
lock:
  url: "redis://localhost:6380"
k8s:
  endpoint: "http://localhost:9091"
"#,
    )
    .unwrap();
    let cfg = collector_config_from(path.to_str().unwrap()).expect("yaml config should load");
    assert_eq!(cfg.ch.base_url, "http://localhost:8124");
    assert_eq!(cfg.ch.database, "superops");
    assert!(cfg.mq.brokers.contains("9092"));
    assert_eq!(cfg.mq.group_id.as_deref(), Some("superops-collector"));
    assert_eq!(cfg.lock.url, "redis://localhost:6380");
    assert_eq!(cfg.k8s.endpoint, "http://localhost:9091");
    // collector 段缺省 → serde default
    assert_eq!(cfg.collector.collect_interval_secs, 60);
}

#[test]
fn notify_config_parses() {
    let cfg: Config = serde_yaml::from_str(
        "ch:\n  base_url: http://localhost:8124\nmq:\n  brokers: localhost:9092\nlock:\n  url: redis://localhost:6380\nnotify:\n  silence_secs: 300\n  targets:\n    - name: ops\n      kind: dingtalk\n      url: https://oapi.dingtalk.com/robot/send?access_token=x\n",
    )
    .expect("parse");
    assert_eq!(cfg.notify.silence_secs, 300);
    assert_eq!(cfg.notify.targets.len(), 1);
    assert_eq!(cfg.notify.targets[0].kind, "dingtalk");
}
