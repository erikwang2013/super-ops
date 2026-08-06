mod cluster;
mod config;
mod resource;
mod service;

use ecat::App;
use ecat_registry::{Registration, Registry, ServiceInfo};
use ecat_registry_consul::ConsulRegistry;
use ecat_transport_grpc::GrpcServer;
use std::sync::{Arc, Mutex};
use superops_protos::k8s::v1::k8s_service_server::K8sServiceServer;

use crate::cluster::manager::ClusterManager;
use crate::config::Config;
use crate::service::K8sServiceImpl;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::load()?;
    let _otlp = match &config.otlp {
        Some(endpoint) => Some(
            ecat_tracing_otlp::init("superops-k8s", endpoint)
                .map_err(|e| anyhow::anyhow!("otlp init: {e}"))?,
        ),
        None => None,
    };
    let manager = ClusterManager::new();

    let grpc = GrpcServer::new(format!("0.0.0.0:{}", config.server.grpc_port)).routes(
        tonic::service::Routes::new(K8sServiceServer::new(K8sServiceImpl { manager })),
    );

    let reg_holder = Arc::new(Mutex::new(None::<Registration>));
    let reg_start = Arc::clone(&reg_holder);
    let cfg_start = config.clone();

    let mut app = App::builder()
        .name("superops-k8s")
        .version(env!("CARGO_PKG_VERSION"))
        .server(grpc)
        .on_start(move || {
            let reg = Arc::clone(&reg_start);
            let cfg = cfg_start.clone();
            async move {
                if let Some(consul) = &cfg.consul {
                    let registry = ConsulRegistry::new(&consul.address);
                    let info = ServiceInfo::new("superops-k8s", env!("CARGO_PKG_VERSION"))
                        .with_endpoint(format!("http://localhost:{}", cfg.server.grpc_port));
                    let registration = registry.register(info).await?;
                    tracing::info!(service = "superops-k8s", "registered in consul");
                    *reg.lock().unwrap() = Some(registration);
                }
                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
            }
        })
        .build()
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    app.run().await.map_err(|e| anyhow::anyhow!("{e}"))?;

    Ok(())
}
