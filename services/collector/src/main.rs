use ecat_registry::{Registration, Registry, ServiceInfo};
use ecat_registry_consul::ConsulRegistry;
use ecat_scheduler::Scheduler;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use superops_collector::collect::collect_once;
use superops_collector::config::collector_config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = collector_config()?;
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

    let mut app = ecat::App::builder()
        .name("superops-collector")
        .version(env!("CARGO_PKG_VERSION"))
        .on_start(move || {
            let sched = Arc::clone(&sched_start);
            let cfg = cfg_start.clone();
            let reg = Arc::clone(&reg_start);
            async move {
                if let Some(consul) = &cfg.consul {
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
                let audit_cfg = cfg.clone();
                tokio::spawn(async move {
                    if let Err(e) = superops_collector::events::consume_audit(&audit_cfg).await {
                        tracing::warn!("audit consumer stopped: {e}");
                    }
                });
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
