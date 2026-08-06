use crate::recorder::{frame_valid, session_id};

#[test]
fn session_id_format() {
    let s = session_id();
    assert_eq!(s.len(), 36);
    assert_eq!(s.chars().filter(|&c| c == '-').count(), 4);
}

#[test]
fn frame_size_cap() {
    assert!(frame_valid(&vec![0u8; 256 * 1024]));
    assert!(!frame_valid(&vec![0u8; 256 * 1024 + 1]));
}

#[test]
fn truncate_caps_at_256k() {
    assert_eq!(
        crate::recorder::truncate(&vec![0u8; 256 * 1024 + 1]).len(),
        256 * 1024
    );
    assert_eq!(crate::recorder::truncate(&vec![0u8; 8]).len(), 8);
}

#[test]
fn seq_increments_per_session() {
    let mut map = std::collections::HashMap::new();
    assert_eq!(crate::recorder::bump_seq(&mut map, "s1"), 1);
    assert_eq!(crate::recorder::bump_seq(&mut map, "s1"), 2);
    assert_eq!(crate::recorder::bump_seq(&mut map, "s2"), 1);
    assert_eq!(crate::recorder::bump_seq(&mut map, "s1"), 3);
}
