use crate::config::Config;
use ecat_mq::MessageQueue;
use std::sync::Arc;

/// 消息后端选择：mqtt > nats > kafka（默认）。三者均实现 ecat_mq::MessageQueue，
/// 上层（事件总线/审计消费）无需感知具体协议。
pub async fn mq_backend(config: &Config) -> anyhow::Result<Option<Arc<dyn MessageQueue>>> {
    if let Some(cfg) = &config.mqtt {
        let mq = ecat_mq_mqtt::MqttMq::from_config(cfg.clone())
            .await
            .map_err(|e| anyhow::anyhow!("mqtt config: {e}"))?;
        return Ok(Some(Arc::new(mq)));
    }
    if let Some(cfg) = &config.nats {
        let mq = ecat_mq_nats::NatsMq::from_config(cfg.clone())
            .await
            .map_err(|e| anyhow::anyhow!("nats config: {e}"))?;
        return Ok(Some(Arc::new(mq)));
    }
    if let Some(cfg) = &config.mq {
        let mq = ecat_mq_kafka::KafkaMq::from_config(cfg.clone())
            .await
            .map_err(|e| anyhow::anyhow!("kafka config: {e}"))?;
        return Ok(Some(Arc::new(mq)));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use crate::config::Config;

    fn base_yaml() -> &'static str {
        "server:\n  http_port: 8080\n  grpc_port: 9090\nauth:\n  jwt_secret: x\n  access_token_ttl: 1\n  refresh_token_ttl: 2\ndatabase:\n  url: mysql://u:p@h/d\nredis:\n  url: redis://h\nservices:\n  k8s:\n    endpoint: http://h\nch:\n  base_url: http://h\n  database: d\n"
    }

    #[test]
    fn provider_sections_all_parse() {
        let cfg: Config = serde_yaml::from_str(&format!(
            "{}mqtt:\n  url: mqtt://h:1883\nnats:\n  url: nats://h:4222\nmq:\n  brokers: localhost:9092\n",
            base_yaml()
        ))
        .unwrap();
        assert!(cfg.mqtt.is_some() && cfg.nats.is_some() && cfg.mq.is_some());
    }

    #[test]
    fn all_optional_default_to_none() {
        let cfg: Config = serde_yaml::from_str(base_yaml()).unwrap();
        assert!(
            cfg.mqtt.is_none()
                && cfg.nats.is_none()
                && cfg.graph.is_none()
                && cfg.search.is_none()
                && cfg.storage.is_none()
                && cfg.etcd.is_none()
        );
    }
}
