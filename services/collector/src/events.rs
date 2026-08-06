use crate::ch::clickhouse_from;
use crate::config::Config;
use ecat_data::{DataPoint, FieldValue};
use ecat_mq::MessageQueue;
use ecat_mq_kafka::KafkaMq;
use futures::future::poll_fn;
use serde::Deserialize;

const AUDIT_TOPIC: &str = "superops.audit";

#[derive(Debug, Clone, Deserialize)]
pub struct AuditEvent {
    #[serde(default)]
    pub ts: i64,
    pub event_type: String,
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
        .with_field("detail", FieldValue::String(e.detail.clone()))
        .with_timestamp(if e.ts > 0 {
            e.ts
        } else {
            crate::ch::now_secs()
        })
}

pub async fn consume_audit(cfg: &Config) -> anyhow::Result<()> {
    let mq = KafkaMq::from_config(cfg.mq.clone())
        .await
        .map_err(|e| anyhow::anyhow!("kafka config: {e}"))?;
    let mut stream = mq
        .subscribe(AUDIT_TOPIC)
        .await
        .map_err(|e| anyhow::anyhow!("kafka subscribe: {e}"))?;
    let ch = clickhouse_from(cfg)?;
    while let Some(msg) = poll_fn(|cx| stream.poll_recv(cx)).await {
        match msg {
            Ok(payload) => match serde_json::from_slice::<AuditEvent>(&payload) {
                Ok(event) => {
                    let point = audit_to_data_point(&event);
                    if let Err(e) = ecat_data::TsdbClient::write(ch.as_ref(), &[point]).await {
                        tracing::warn!("audit write failed: {e}");
                    }
                }
                Err(e) => tracing::warn!("audit parse failed: {e}"),
            },
            Err(e) => tracing::warn!("audit recv failed: {e}"),
        }
    }
    Ok(())
}
