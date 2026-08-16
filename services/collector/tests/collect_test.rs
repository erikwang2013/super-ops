use ecat_data_clickhouse::ClickhouseConfig;
use ecat_data_redis::RedisConfig;
use ecat_mq_kafka::KafkaConfig;
use superops_collector::collect::collect_once;
use superops_collector::config::{Config, K8sConfig};

#[tokio::test]
async fn collect_once_with_unreachable_k8s_returns_error() {
    let cfg = Config {
        ch: ClickhouseConfig {
            base_url: "http://localhost:8124".into(),
            database: "superops".into(),
            username: None,
            password: None,
            tls: None,
        },
        mq: KafkaConfig {
            brokers: "localhost:9092".into(),
            group_id: None,
        },
        lock: RedisConfig {
            url: "redis://localhost:6380".into(),
            password: None,
            tls: None,
        },
        k8s: K8sConfig {
            endpoint: "http://localhost:1".into(),
            cluster_id: None,
            token: None,
        },
        collector: Default::default(),
        mqtt: None,
        nats: None,
        search: None,
        etcd: None,
        consul: None,
        otlp: None,
        notify: Default::default(),
        logtail: Default::default(),
        housekeeping: Default::default(),
        drift: Default::default(),
        selfheal: Default::default(),
        rollback: Default::default(),
        mysql: None,
        smtp: None,
    };
    // k8s 端点不可达 → 连接阶段快速失败，不触碰 CH/Redis
    let res = collect_once(&cfg).await;
    assert!(res.is_err());
}
