use ecat_registry::{Registration, Registry, ServiceInfo};
use ecat_registry_consul::ConsulRegistry;
use ecat_registry_etcd::EtcdRegistry;
use ecat_scheduler::Scheduler;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use superops_collector::collect::collect_once;
use superops_collector::config::collector_config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = collector_config()?;
    let _otlp = match &cfg.otlp {
        Some(endpoint) => Some(
            ecat_tracing_otlp::init("superops-collector", endpoint)
                .map_err(|e| anyhow::anyhow!("otlp init: {e}"))?,
        ),
        None => None,
    };
    tracing::info!(
        k8s = %cfg.k8s.endpoint,
        ch = %cfg.ch.base_url,
        "collector config loaded"
    );

    let scheduler = Arc::new(Mutex::new(Scheduler::new()));
    let sched_start = Arc::clone(&scheduler);
    let cfg_start = cfg.clone();
    let sched_stop = Arc::clone(&scheduler);

    let reg_holder = Arc::new(Mutex::new(None::<Registration>));
    let reg_start = Arc::clone(&reg_holder);

    let mq = match superops_collector::mq::mq_backend(&cfg).await {
        Ok(m) => Some(m),
        Err(e) => {
            tracing::warn!(error = %e, "mq backend init failed; audit/domain-event consumers disabled");
            None
        }
    };
    let mq_start = mq.clone();

    let mut app = ecat::App::builder()
        .name("superops-collector")
        .version(env!("CARGO_PKG_VERSION"))
        .on_start(move || {
            let sched = Arc::clone(&sched_start);
            let cfg = cfg_start.clone();
            let reg = Arc::clone(&reg_start);
            let mq = mq_start.clone();
            async move {
                if let Some(etcd) = &cfg.etcd {
                    let registry = EtcdRegistry::new(etcd.endpoints.clone(), &etcd.prefix)
                        .lease_ttl(30);
                    let info = ServiceInfo::new("superops-collector", env!("CARGO_PKG_VERSION"));
                    let registration = registry.register(info).await?;
                    tracing::info!(service = "superops-collector", "registered in etcd");
                    *reg.lock().unwrap() = Some(registration);
                } else if let Some(consul) = &cfg.consul {
                    let registry = ConsulRegistry::new(&consul.address);
                    let info = ServiceInfo::new("superops-collector", env!("CARGO_PKG_VERSION"));
                    let registration = registry.register(info).await?;
                    tracing::info!(service = "superops-collector", "registered in consul");
                    *reg.lock().unwrap() = Some(registration);
                }
                let collect_cfg = cfg.clone();
                let inspect_cfg = cfg.clone();
                sched.lock().unwrap().every(
                    Duration::from_secs(cfg.collector.collect_interval_secs),
                    move || {
                        let cfg = collect_cfg.clone();
                        async move {
                            if let Err(e) = collect_once(&cfg).await {
                                tracing::warn!("collect failed: {e}");
                            }
                        }
                    },
                );
                sched.lock().unwrap().every(
                    Duration::from_secs(cfg.collector.inspect_interval_secs),
                    move || {
                        let cfg = inspect_cfg.clone();
                        async move {
                            if let Err(e) = superops_collector::alert::inspect_once(&cfg).await {
                                tracing::warn!("inspect failed: {e}");
                            }
                        }
                    },
                );
                let logtail_cfg = cfg.clone();
                sched.lock().unwrap().every(
                    Duration::from_secs(cfg.logtail.interval_secs),
                    move || {
                        let cfg = logtail_cfg.clone();
                        async move {
                            if cfg.logtail.enabled
                                && let Err(e) =
                                    superops_collector::logtail::collect_once(&cfg).await
                            {
                                tracing::warn!("logtail failed: {e}");
                            }
                        }
                    },
                );
                let hk_cfg = cfg.clone();
                sched.lock().unwrap().every(
                    Duration::from_secs(cfg.housekeeping.interval_secs),
                    move || {
                        let cfg = hk_cfg.clone();
                        async move {
                            if cfg.housekeeping.enabled
                                && let Err(e) =
                                    superops_collector::housekeeping::housekeeping_once(&cfg).await
                            {
                                tracing::warn!("housekeeping failed: {e}");
                            }
                        }
                    },
                );
                let drift_cfg = cfg.clone();
                sched.lock().unwrap().every(
                    Duration::from_secs(cfg.drift.interval_secs),
                    move || {
                        let cfg = drift_cfg.clone();
                        async move {
                            if cfg.drift.enabled
                                && let Err(e) = superops_collector::drift::drift_once(&cfg).await
                            {
                                tracing::warn!("drift check failed: {e}");
                            }
                        }
                    },
                );
                let rollback_cfg = cfg.clone();
                sched.lock().unwrap().every(
                    Duration::from_secs(cfg.rollback.interval_secs),
                    move || {
                        let cfg = rollback_cfg.clone();
                        async move {
                            if cfg.rollback.enabled
                                && let Err(e) =
                                    superops_collector::rollback::rollback_once(&cfg).await
                            {
                                tracing::warn!("auto rollback failed: {e}");
                            }
                        }
                    },
                );
                let audit_cfg = cfg.clone();
                if let Some(mq) = mq.clone() {
                    tokio::spawn(async move {
                        if let Err(e) =
                            superops_collector::events::consume_audit(mq, &audit_cfg).await
                        {
                            tracing::warn!("audit consumer stopped: {e}");
                        }
                    });
                }
                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
            }
        })
        .on_stop(move || {
            let sched = Arc::clone(&sched_stop);
            async move {
                let mut guard = sched.lock().unwrap();
                std::mem::take(&mut *guard).shutdown();
                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
            }
        })
        .build()
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    app.run().await.map_err(|e| anyhow::anyhow!("{e}"))
}
