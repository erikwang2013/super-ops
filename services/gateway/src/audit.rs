//! 审计发布统一封装：MQ 可用时发布到 `superops.audit`；MQ 缺失或发布失败时落本地 JSONL 兜底，
//! 保证写操作审计不静默丢失（P0：审计可靠性）。

use crate::AppState;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;

/// 统一发布审计事件（构造 payload → MQ 发布 → 失败/缺失落本地兜底文件）。
pub async fn publish(
    state: &AppState,
    event_type: &str,
    username: &str,
    ip: &str,
    detail: &str,
    extra: &serde_json::Value,
) {
    let mut payload = serde_json::json!({
        "ts": now_secs(),
        "event_type": event_type,
        "level": "INFO",
        "username": username,
        "ip": ip,
        "detail": detail,
    });
    if let Some(extra_obj) = extra.as_object() {
        for (k, v) in extra_obj {
            payload[k] = v.clone();
        }
    }
    let delivered = match &state.mq {
        Some(mq) => mq
            .publish(
                "superops.audit",
                &serde_json::to_vec(&payload).unwrap_or_default(),
            )
            .await
            .is_ok(),
        None => false,
    };
    if !delivered {
        append_fallback(state, &payload).await;
    }
}

/// 追加一条审计到本地兜底文件 `{audit_fallback_dir}/audit-YYYYMMDD.jsonl`（JSONL）。
pub async fn append_fallback(state: &AppState, payload: &serde_json::Value) {
    append_line(&state.audit_fallback_dir, payload).await;
}

/// 向目录内当天的 JSONL 文件追加一行（目录不存在则创建；写入失败仅告警，不阻塞主流程）。
pub async fn append_line(dir: &str, payload: &serde_json::Value) {
    if let Err(e) = tokio::fs::create_dir_all(dir).await {
        tracing::warn!("audit fallback dir create failed: {e}");
        return;
    }
    let path = PathBuf::from(dir).join(format!("audit-{}.jsonl", utc_date()));
    let mut f = match tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .await
    {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!("audit fallback open failed: {e}");
            return;
        }
    };
    let line = format!("{}\n", serde_json::to_string(payload).unwrap_or_default());
    if let Err(e) = f.write_all(line.as_bytes()).await {
        tracing::warn!("audit fallback write failed: {e}");
    }
    if let Err(e) = f.flush().await {
        tracing::warn!("audit fallback flush failed: {e}");
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// UTC 日期 `YYYY-MM-DD`（Howard Hinnant 算法，避免 chrono 依赖）。
fn utc_date() -> String {
    let (y, m, d) = civil_from_days(now_secs() / 86400);
    format!("{y:04}-{m:02}-{d:02}")
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_date_shape() {
        let d = utc_date();
        assert_eq!(d.len(), 10);
        assert_eq!(d.as_bytes()[4], b'-');
        assert_eq!(d.as_bytes()[7], b'-');
    }

    #[tokio::test]
    async fn append_line_writes_jsonl_and_appends() {
        let dir = tempfile::tempdir().unwrap();
        let dir_str = dir.path().to_string_lossy().into_owned();
        let payload = serde_json::json!({
            "event_type": "k8s.scale",
            "username": "erik",
            "detail": "scale test",
            "ts": 12345,
        });
        append_line(&dir_str, &payload).await;
        append_line(&dir_str, &payload).await;

        // 文件系统元数据在并行测试下可能有可见性延迟，重试读取；聚合所有当天文件行数
        let entries = loop {
            let entries: Vec<_> = std::fs::read_dir(dir.path())
                .unwrap()
                .filter_map(|e| e.ok())
                .collect();
            if !entries.is_empty() {
                break entries;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        };
        let mut total_lines = 0;
        let mut first: Option<serde_json::Value> = None;
        for e in entries {
            let content = std::fs::read_to_string(e.path()).unwrap();
            for line in content.lines() {
                if first.is_none() {
                    first = serde_json::from_str(line).ok();
                }
                total_lines += 1;
            }
        }
        assert_eq!(total_lines, 2, "两行 JSONL 追加");
        let parsed = first.expect("至少解析出第一行");
        assert_eq!(parsed["event_type"], "k8s.scale");
        assert_eq!(parsed["username"], "erik");
    }
}
