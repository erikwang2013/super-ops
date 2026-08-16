use ecat_data::FieldValue;
use superops_collector::events::{AuditEvent, audit_to_data_point};

#[test]
fn audit_point_shape() {
    let e = AuditEvent {
        ts: 1700000000,
        event_type: "login_success".into(),
        level: "INFO".into(),
        username: "alice".into(),
        ip: "10.0.0.1".into(),
        detail: "login ok".into(),
    };
    let p = audit_to_data_point(&e);
    assert_eq!(p.measurement, "audit_log");
    assert_eq!(
        p.tags.get("event_type").map(String::as_str),
        Some("login_success")
    );
    assert_eq!(p.tags.get("level").map(String::as_str), Some("INFO"));
    assert_eq!(p.tags.get("username").map(String::as_str), Some("alice"));
    assert_eq!(p.tags.get("ip").map(String::as_str), Some("10.0.0.1"));
    assert!(matches!(
        p.fields.get("detail"),
        Some(FieldValue::String(s)) if s == "login ok"
    ));
    assert_eq!(p.timestamp, Some(1700000000));
}

#[test]
fn audit_json_parses() {
    let e: AuditEvent = serde_json::from_str(
        r#"{"ts":1700000001,"event_type":"register_success","username":"bob","ip":"10.0.0.2","detail":"registered"}"#,
    )
    .unwrap();
    assert_eq!(e.event_type, "register_success");
    assert_eq!(e.username, "bob");
}

#[test]
fn missing_ts_falls_back_to_now() {
    let e: AuditEvent = serde_json::from_str(
        r#"{"event_type":"login_success","username":"carol","ip":"10.0.0.3","detail":"ok"}"#,
    )
    .unwrap();
    assert_eq!(e.ts, 0);
    let p = audit_to_data_point(&e);
    assert!(p.timestamp.is_some_and(|ts| ts > 0));
}
