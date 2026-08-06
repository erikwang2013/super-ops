use crate::vault::{decrypt_value, encrypt_value};

#[test]
fn roundtrip() {
    let key = [7u8; 32];
    let blob = encrypt_value(&key, "s3cret-password").unwrap();
    assert_eq!(decrypt_value(&key, &blob).unwrap(), "s3cret-password");
    let mut bad = blob.clone();
    *bad.last_mut().unwrap() ^= 0x01; // 篡改密文
    assert!(decrypt_value(&key, &bad).is_err());
    assert!(decrypt_value(&key, b"short").is_err());
}
