use crate::housekeeping::{
    estimate_cost, is_backup_file, parse_cpu_cores, parse_mem_gib, sh_quote, utc_timestamp,
};

#[test]
fn cost_estimate() {
    // 2 核 × ¥0.5/h + 4 GiB × ¥0.25/h（0.5/0.25 均为精确二进制小数），按天 = (1.0 + 1.0) * 24
    assert_eq!(estimate_cost(2.0, 4.0, 0.5, 0.25), 48.0);
}

#[test]
fn backup_name_pattern() {
    assert!(is_backup_file("superops-2026-08-06-0300.sql.gz"));
    assert!(!is_backup_file("../evil.sql.gz"));
    assert!(!is_backup_file("notes.txt"));
}

#[test]
fn cpu_cores_parse() {
    assert_eq!(parse_cpu_cores("2500m"), 2.5);
    assert_eq!(parse_cpu_cores("2"), 2.0);
    assert_eq!(parse_cpu_cores("100m"), 0.1);
    assert_eq!(parse_cpu_cores(""), 0.0);
    assert_eq!(parse_cpu_cores("junk"), 0.0);
}

#[test]
fn mem_gib_parse() {
    assert_eq!(parse_mem_gib("8Gi"), 8.0);
    assert_eq!(parse_mem_gib("8192Mi"), 8.0);
    assert_eq!(parse_mem_gib("8192Ki"), 0.0078125); // 8192 KiB = 8 MiB
    assert_eq!(parse_mem_gib("8589934592"), 8.0);
    assert_eq!(parse_mem_gib("junk"), 0.0);
}

#[test]
fn non_finite_inputs_map_to_zero() {
    // "NaN"/"Infinity" 能成功 parse 为 f64，若不做有限性守卫会以 NaN/Inf 静默污染 capacity 汇总
    assert_eq!(parse_cpu_cores("NaN"), 0.0);
    assert_eq!(parse_cpu_cores("Infinity"), 0.0);
    assert_eq!(parse_mem_gib("NaN"), 0.0);
    assert_eq!(parse_mem_gib("inf"), 0.0);
    assert_eq!(parse_mem_gib("1e999Gi"), 0.0);
}

#[test]
fn sh_quote_wraps_and_escapes() {
    assert_eq!(sh_quote("plain"), "'plain'");
    // 含单引号输入被转义, 无法闭合引号注入
    assert_eq!(sh_quote("it's"), "'it'\\''s'");
    assert_eq!(sh_quote("a'b'c"), "'a'\\''b'\\''c'");
}

#[test]
fn timestamp_format() {
    assert_eq!(utc_timestamp(0), "1970-01-01-000000");
    assert_eq!(utc_timestamp(86400 + 3661), "1970-01-02-010101");
}
