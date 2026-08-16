// Copyright (c) 2026 erik <erik@erik.xyz> — https://erik.xyz
use async_trait::async_trait;
use ecat_mq::{MessageQueue, MessageStream, MqError};
use rdkafka::Message;
use rdkafka::config::ClientConfig;
use rdkafka::consumer::{BaseConsumer, Consumer};
use rdkafka::producer::{FutureProducer, FutureRecord};
use serde::Deserialize;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Deserialize)]
pub struct KafkaConfig {
    pub brokers: String,
    #[serde(default)]
    pub group_id: Option<String>,
}

pub struct KafkaMq {
    producer: FutureProducer,
    brokers: String,
    group_id: Option<String>,
}

impl KafkaMq {
    pub async fn connect(brokers: &str) -> Result<Self, MqError> {
        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", brokers)
            .set("message.timeout.ms", "5000")
            .create()
            .map_err(|e| MqError::Other(format!("kafka producer: {e}")))?;
        Ok(Self {
            producer,
            brokers: brokers.to_string(),
            group_id: None,
        })
    }

    pub async fn from_config(cfg: KafkaConfig) -> Result<Self, MqError> {
        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", &cfg.brokers)
            .set("message.timeout.ms", "5000")
            .create()
            .map_err(|e| MqError::Other(format!("kafka producer: {e}")))?;
        Ok(Self {
            producer,
            brokers: cfg.brokers,
            group_id: cfg.group_id,
        })
    }
}

#[async_trait]
impl MessageQueue for KafkaMq {
    async fn publish(&self, topic: &str, payload: &[u8]) -> Result<(), MqError> {
        let record: FutureRecord<'_, str, [u8]> = FutureRecord::to(topic).payload(payload);
        self.producer
            .send(record, Duration::from_secs(5))
            .await
            .map_err(|(e, _)| MqError::Other(format!("kafka publish: {e}")))?;
        Ok(())
    }

    async fn subscribe(&self, topic: &str) -> Result<Box<dyn MessageStream>, MqError> {
        let mut config = ClientConfig::new();
        config
            .set("bootstrap.servers", &self.brokers)
            .set("enable.auto.commit", "false")
            .set("auto.offset.reset", "latest");
        if let Some(group) = &self.group_id {
            config.set("group.id", group);
        }
        let consumer: Arc<BaseConsumer> = Arc::new(
            config
                .create()
                .map_err(|e| MqError::Other(format!("kafka consumer: {e}")))?,
        );
        consumer
            .subscribe(&[topic])
            .map_err(|e| MqError::Other(format!("kafka subscribe: {e}")))?;

        // 转发 (partition, offset, payload)：消费方处理成功后经 commit() 手动提交 offset
        let poll_consumer = Arc::clone(&consumer);
        let stream_topic = topic.to_string();
        let (tx, rx) = mpsc::channel::<(i32, i64, Vec<u8>)>(1024);
        tokio::spawn(async move {
            loop {
                if let Some(Ok(msg)) = poll_consumer.poll(Duration::from_millis(100))
                    && let Some(payload) = msg.payload()
                    && tx
                        .send((msg.partition(), msg.offset(), payload.to_vec()))
                        .await
                        .is_err()
                {
                    break;
                }
                // Yield the worker thread between polls.
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        });
        Ok(Box::new(KafkaStream {
            rx,
            consumer,
            topic: stream_topic,
            last_partition: None,
            last_offset: None,
        }))
    }
}

struct KafkaStream {
    rx: mpsc::Receiver<(i32, i64, Vec<u8>)>,
    consumer: Arc<BaseConsumer>,
    topic: String,
    last_partition: Option<i32>,
    last_offset: Option<i64>,
}

impl MessageStream for KafkaStream {
    fn poll_recv(&mut self, cx: &mut Context<'_>) -> Poll<Option<Result<Vec<u8>, MqError>>> {
        match self.rx.poll_recv(cx) {
            Poll::Ready(Some((partition, offset, data))) => {
                self.last_partition = Some(partition);
                self.last_offset = Some(offset);
                Poll::Ready(Some(Ok(data)))
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }

    /// 手动提交已处理消息的 offset（store + commit_consumer_state）。
    /// 仅在消费方确认写入成功后调用，避免未处理消息被跳过（at-least-once）。
    fn commit(&mut self) {
        if let (Some(partition), Some(offset)) = (self.last_partition, self.last_offset)
            && self
                .consumer
                .store_offset(&self.topic, partition, offset)
                .is_ok()
        {
            let _ = self
                .consumer
                .commit_consumer_state(rdkafka::consumer::CommitMode::Async);
        }
    }
}

impl Unpin for KafkaStream {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_deserializes() {
        let cfg: KafkaConfig = serde_json::from_value(serde_json::json!({
            "brokers": "localhost:9092",
            "group_id": "my-group",
        }))
        .unwrap();
        assert_eq!(cfg.group_id.as_deref(), Some("my-group"));
    }

    #[tokio::test]
    async fn producer_constructs() {
        let _mq = KafkaMq::connect("localhost:9092").await.unwrap();
    }
}
