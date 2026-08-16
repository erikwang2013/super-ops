use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use ecat_data::{DataPoint, FieldValue, RdbmsClient};
use ecat_data_clickhouse::ClickhouseClient;
use ecat_mq::MessageQueue;
use futures::future::poll_fn;
use serde::Deserialize;
use std::sync::Arc;
use superops_protos::events::{DomainEvent, event_topic};

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn domain_event_to_point(e: &DomainEvent) -> DataPoint {
    DataPoint::new("domain_event")
        .with_tag("event_type", e.event_type.clone())
        .with_tag("level", e.level.clone())
        .with_field("title", FieldValue::String(e.title.clone()))
        .with_field("message", FieldValue::String(e.message.clone()))
        .with_field("detail", FieldValue::String(e.detail.to_string()))
        .with_timestamp(if e.ts > 0 { e.ts } else { now_secs() })
}

/// 领域事件消费者：订阅 ecat-events 总线（topic = DomainEvent 类型路径），
/// 落 ClickHouse domain_event 表供 /api/events 查询。
pub async fn consume_domain_events(
    mq: Arc<dyn MessageQueue>,
    ch: Arc<ClickhouseClient>,
) -> anyhow::Result<()> {
    let topic = event_topic::<DomainEvent>();
    let mut stream = mq
        .subscribe(&topic)
        .await
        .map_err(|e| anyhow::anyhow!("mq subscribe {topic}: {e}"))?;
    tracing::info!(topic, "domain event consumer started");
    while let Some(msg) = poll_fn(|cx| stream.poll_recv(cx)).await {
        match msg {
            Ok(payload) => match serde_json::from_slice::<DomainEvent>(&payload) {
                Ok(event) => {
                    if let Err(e) =
                        ecat_data::TsdbClient::write(ch.as_ref(), &[domain_event_to_point(&event)])
                            .await
                    {
                        tracing::warn!(error = %e, "domain event write failed");
                    }
                }
                Err(e) => tracing::warn!(error = %e, "domain event parse failed"),
            },
            Err(e) => tracing::warn!(error = %e, "domain event recv failed"),
        }
    }
    Ok(())
}

#[derive(Debug, Default, Deserialize)]
pub struct EventListQuery {
    pub limit: Option<i64>,
    pub event_type: Option<String>,
}

fn clamp_limit(q: &EventListQuery) -> i64 {
    q.limit.unwrap_or(100).clamp(1, 1000)
}

pub async fn list_domain_events(
    State(state): State<crate::AppState>,
    Query(q): Query<EventListQuery>,
) -> impl IntoResponse {
    let limit = clamp_limit(&q);
    let mut sql = String::from(
        "SELECT event_type, level, title, message, \
         formatDateTime(toDateTime(timestamp), '%Y-%m-%d %H:%i:%s') AS ts \
         FROM domain_event",
    );
    if let Some(t) = q.event_type.filter(|s| !s.is_empty()) {
        sql.push_str(&format!(" WHERE event_type = '{}'", t.replace('\'', "''")));
    }
    sql.push_str(&format!(" ORDER BY timestamp DESC LIMIT {limit}"));
    match state.ch.query(&sql).await {
        Ok(rows) => {
            let events: Vec<serde_json::Value> = rows
                .iter()
                .map(|r| {
                    let mut m = serde_json::Map::new();
                    for col in ["event_type", "level", "title", "message", "ts"] {
                        if let Some(v) = r.get(col) {
                            m.insert(col.to_string(), v.clone());
                        }
                    }
                    serde_json::Value::Object(m)
                })
                .collect();
            (
                StatusCode::OK,
                Json(serde_json::json!({ "events": events })),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": format!("domain event query failed: {e}") })),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_event_maps_to_data_point() {
        let e = DomainEvent::new("alert", "WARN", "node-not-ready", "node n1 status=NotReady")
            .with_detail(serde_json::json!({"node": "n1"}))
            .with_ts(1700000000);
        let p = domain_event_to_point(&e);
        assert_eq!(p.measurement, "domain_event");
        assert_eq!(p.tags.get("event_type").map(String::as_str), Some("alert"));
        assert_eq!(p.tags.get("level").map(String::as_str), Some("WARN"));
        match p.fields.get("title") {
            Some(FieldValue::String(s)) => assert_eq!(s, "node-not-ready"),
            other => panic!("unexpected title field: {other:?}"),
        }
        assert_eq!(p.timestamp, Some(1700000000));
    }

    #[test]
    fn domain_event_falls_back_to_now() {
        let e = DomainEvent::new("rollback", "INFO", "rollback", "ok");
        let p = domain_event_to_point(&e);
        assert!(p.timestamp.unwrap_or(0) > 1_700_000_000);
    }

    #[test]
    fn event_list_query_clamps_limit() {
        assert_eq!(
            clamp_limit(&EventListQuery {
                limit: Some(9999),
                ..Default::default()
            }),
            1000
        );
        assert_eq!(clamp_limit(&EventListQuery::default()), 100);
    }
}
