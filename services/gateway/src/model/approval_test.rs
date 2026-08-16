use super::approval::{check_delete_approval, delete_target, next_status, validate_approval};

#[test]
fn next_status_transitions() {
    assert_eq!(next_status("pending", "approve"), Some("approved"));
    assert_eq!(next_status("pending", "reject"), Some("rejected"));
    assert_eq!(next_status("pending", "cancel"), Some("canceled"));
    assert_eq!(next_status("rejected", "reopen"), Some("pending"));
    assert_eq!(next_status("approved", "approve"), None); // 终态
    assert_eq!(next_status("pending", "hack"), None);
}

#[test]
fn action_whitelist() {
    for a in ["approve", "reject", "cancel", "reopen"] {
        assert!(validate_approval(a));
    }
    assert!(!validate_approval("delete"));
    assert!(!validate_approval(""));
}

#[test]
fn delete_target_uses_cluster_ns_name_format() {
    // 门禁 target 约定，k8s 删除路由与混沌 delete 共用，避免跨集群同名绕过
    assert_eq!(delete_target("c1", "prod", "api"), "c1/prod/api");
}

#[tokio::test]
async fn approval_gate_skipped_when_disabled() {
    // enabled=false 时不触碰数据库（pool 传 None），k8s 删除与混沌 delete 均直通
    assert!(
        check_delete_approval(None, false, "c1", "prod", "api")
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn approval_gate_fails_closed_without_db_when_enabled() {
    // enabled=true 但无 pool：报错而非静默放行（fail-closed）
    let err = check_delete_approval(None, true, "c1", "prod", "api")
        .await
        .unwrap_err();
    assert_eq!(err.0, 500);
}
