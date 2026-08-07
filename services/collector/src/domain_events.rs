use crate::config::Config;
use superops_protos::events::DomainEvent;

/// 发布领域事件：经 ecat-events 事件总线（topic = DomainEvent 类型路径）广播，
/// gateway 端 domain_events::consume_domain_events 订阅并落 ClickHouse。
pub async fn publish_domain_event(cfg: &Config, event: &DomainEvent) -> anyhow::Result<()> {
    let mq = crate::mq::mq_backend(cfg).await?;
    let bus = ecat_events::EventBus::remote(mq);
    bus.publish(event)
        .await
        .map_err(|e| anyhow::anyhow!("domain event publish: {e}"))
}
