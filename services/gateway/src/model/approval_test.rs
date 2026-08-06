use super::approval::{next_status, validate_approval};

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
