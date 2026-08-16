use crate::ch::clickhouse_from;
use crate::config::Config;
use ecat_data::{DataPoint, FieldValue};
use ecat_mq::MessageQueue;
use futures::future::poll_fn;
use serde::Deserialize;
use std::sync::Arc;

const AUDIT_TOPIC: &str = "superops.audit";

#[derive(Debug, Clone, Deserialize)]
pub struct AuditEvent {
    #[serde(default)]
    pub ts: i64,
    pub event_type: String,
    #[serde(default)]
    pub level: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub ip: String,
    #[serde(default)]
    pub detail: String,
}

pub fn audit_to_data_point(e: &AuditEvent) -> DataPoint {
    DataPoint::new("audit_log")
        .with_tag("event_type", e.event_type.clone())
        .with_tag("username", e.username.clone())
        .with_tag("ip", e.ip.clone())
        .with_tag("level", e.level.clone())
        .with_field("detail", FieldValue::String(e.detail.clone()))
        .with_timestamp(if e.ts > 0 {
            e.ts
        } else {
            crate::ch::now_secs()
        })
}

pub async fn consume_audit(mq: Arc<dyn MessageQueue>, cfg: &Config) -> anyhow::Result<()> {
    let mut stream = mq
        .subscribe(AUDIT_TOPIC)
        .await
        .map_err(|e| anyhow::anyhow!("mq subscribe: {e}"))?;
    let ch = clickhouse_from(cfg)?;
    while let Some(msg) = poll_fn(|cx| stream.poll_recv(cx)).await {
        match msg {
            Ok(payload) => match serde_json::from_slice::<AuditEvent>(&payload) {
                Ok(event) => {
                    let point = audit_to_data_point(&event);
                    match ecat_data::TsdbClient::write(ch.as_ref(), &[point]).await {
                        // 写入成功才提交 offset（at-least-once：失败不提交，重启重放不丢审计）
                        Ok(_) => stream.commit(),
                        Err(e) => tracing::warn!("audit write failed: {e}"),
                    }
                }
                Err(e) => {
                    // 坏消息无法重放，跳过该条
                    tracing::warn!("audit parse failed: {e}");
                    stream.commit();
                }
            },
            Err(e) => tracing::warn!("audit recv failed: {e}"),
        }
    }
    Ok(())
}
