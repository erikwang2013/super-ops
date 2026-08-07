use crate::config::Config;
use ecat_mq::MessageQueue;
use std::sync::Arc;

/// 消息后端选择：mqtt > nats > kafka（mq 段必填，作为兜底）。
pub async fn mq_backend(config: &Config) -> anyhow::Result<Arc<dyn MessageQueue>> {
    if let Some(cfg) = &config.mqtt {
        let mq = ecat_mq_mqtt::MqttMq::from_config(cfg.clone())
            .await
            .map_err(|e| anyhow::anyhow!("mqtt config: {e}"))?;
        return Ok(Arc::new(mq));
    }
    if let Some(cfg) = &config.nats {
        let mq = ecat_mq_nats::NatsMq::from_config(cfg.clone())
            .await
            .map_err(|e| anyhow::anyhow!("nats config: {e}"))?;
        return Ok(Arc::new(mq));
    }
    let mq = ecat_mq_kafka::KafkaMq::from_config(config.mq.clone())
        .await
        .map_err(|e| anyhow::anyhow!("kafka config: {e}"))?;
    Ok(Arc::new(mq))
}

#[cfg(test)]
mod tests {
    use crate::config::Config;

    fn base_yaml() -> &'static str {
        "ch:\n  base_url: http://h\n  database: d\nmq:\n  brokers: localhost:9092\nlock:\n  url: redis://h\n"
    }

    #[test]
    fn provider_sections_all_parse() {
        let cfg: Config = serde_yaml::from_str(&format!(
            "{}mqtt:\n  url: mqtt://h:1883\nnats:\n  url: nats://h:4222\n",
            base_yaml()
        ))
        .unwrap();
        assert!(cfg.mqtt.is_some() && cfg.nats.is_some());
    }

    #[test]
    fn kafka_is_fallback() {
        let cfg: Config = serde_yaml::from_str(base_yaml()).unwrap();
        assert!(cfg.mqtt.is_none() && cfg.nats.is_none() && !cfg.mq.brokers.is_empty());
    }
}
